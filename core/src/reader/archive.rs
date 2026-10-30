//! Archives read on demand: a zip's directory is read once and each entry
//! is inflated when asked for (an EPUB chapter, one page of a 500 MB
//! comic). RAR and 7z (CBR, CB7) go through the system's `bsdtar` (the
//! libarchive that comes with macOS reads RAR4, RAR5 and 7z), or `unrar` /
//! `7z` when there is no `bsdtar`, unpacked once into a cache folder.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    method: u16,
    csize: u64,
    size: u64,
    local: u64,
}

/// A zip whose entries are read one at a time.
pub struct Zip {
    file: Mutex<File>,
    pub entries: Vec<Entry>,
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64le(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

impl Zip {
    pub fn open(path: &Path) -> Result<Zip, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        // the end of central directory record is in the last 64 KiB
        let tail_len = len.min(22 + 65_535 + 20);
        f.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
        let mut tail = vec![0u8; tail_len as usize];
        f.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..=tail.len().saturating_sub(22)).rev().find(|&i| u32le(&tail, i) == Some(0x0605_4b50)).ok_or("não é um arquivo zip")?;
        let mut count = u16le(&tail, eocd + 10).ok_or("zip danificado")? as u64;
        let mut dir_size = u32le(&tail, eocd + 12).ok_or("zip danificado")? as u64;
        let mut dir_at = u32le(&tail, eocd + 16).ok_or("zip danificado")? as u64;
        // zip64: the locator sits just before the record
        if (dir_at == 0xffff_ffff || count == 0xffff) && eocd >= 20 && u32le(&tail, eocd - 20) == Some(0x0706_4b50) {
            let rec_at = u64le(&tail, eocd - 20 + 8).ok_or("zip danificado")?;
            let mut rec = [0u8; 56];
            f.seek(SeekFrom::Start(rec_at)).map_err(|e| e.to_string())?;
            f.read_exact(&mut rec).map_err(|e| e.to_string())?;
            count = u64le(&rec, 32).unwrap_or(count);
            dir_size = u64le(&rec, 40).unwrap_or(dir_size);
            dir_at = u64le(&rec, 48).unwrap_or(dir_at);
        }
        let mut dir = vec![0u8; dir_size as usize];
        f.seek(SeekFrom::Start(dir_at)).map_err(|e| e.to_string())?;
        f.read_exact(&mut dir).map_err(|e| e.to_string())?;
        let mut entries = Vec::with_capacity(count as usize);
        let mut at = 0usize;
        for _ in 0..count {
            if u32le(&dir, at) != Some(0x0201_4b50) {
                break;
            }
            let flags = u16le(&dir, at + 8).unwrap_or(0);
            let method = u16le(&dir, at + 10).unwrap_or(0);
            let mut csize = u32le(&dir, at + 20).unwrap_or(0) as u64;
            let mut size = u32le(&dir, at + 24).unwrap_or(0) as u64;
            let name_len = u16le(&dir, at + 28).unwrap_or(0) as usize;
            let extra_len = u16le(&dir, at + 30).unwrap_or(0) as usize;
            let comment_len = u16le(&dir, at + 32).unwrap_or(0) as usize;
            let mut local = u32le(&dir, at + 42).unwrap_or(0) as u64;
            let raw_name = dir.get(at + 46..at + 46 + name_len).unwrap_or(&[]);
            // bit 11: UTF-8 names; older tools wrote CP437, read as Latin-1
            let name = if flags & 0x800 != 0 { String::from_utf8_lossy(raw_name).into_owned() } else { std::str::from_utf8(raw_name).map(|s| s.to_string()).unwrap_or_else(|_| raw_name.iter().map(|&b| b as char).collect()) };
            // zip64 sizes in the extra field
            let extra = dir.get(at + 46 + name_len..at + 46 + name_len + extra_len).unwrap_or(&[]);
            let mut e = 0;
            while e + 4 <= extra.len() {
                let id = u16le(extra, e).unwrap_or(0);
                let n = u16le(extra, e + 2).unwrap_or(0) as usize;
                if id == 1 {
                    let mut k = e + 4;
                    if size == 0xffff_ffff {
                        size = u64le(extra, k).unwrap_or(size);
                        k += 8;
                    }
                    if csize == 0xffff_ffff {
                        csize = u64le(extra, k).unwrap_or(csize);
                        k += 8;
                    }
                    if local == 0xffff_ffff {
                        local = u64le(extra, k).unwrap_or(local);
                    }
                }
                e += 4 + n;
            }
            at += 46 + name_len + extra_len + comment_len;
            if !name.ends_with('/') {
                entries.push(Entry { name, method, csize, size, local });
            }
        }
        Ok(Zip { file: Mutex::new(f), entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn find(&self, name: &str) -> Option<&Entry> {
        let name = name.trim_start_matches('/');
        self.entries.iter().find(|e| e.name == name).or_else(|| {
            // some books' links differ from the entries only in case or escapes
            let want = percent_encoding::percent_decode_str(name).decode_utf8_lossy().to_lowercase();
            self.entries.iter().find(|e| e.name.to_lowercase() == want)
        })
    }

    /// One entry, inflated.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let e = self.find(name).ok_or_else(|| format!("{name}: não está no arquivo"))?.clone();
        let mut f = self.file.lock().map_err(|_| "arquivo ocupado")?;
        let mut head = [0u8; 30];
        f.seek(SeekFrom::Start(e.local)).map_err(|x| x.to_string())?;
        f.read_exact(&mut head).map_err(|x| x.to_string())?;
        if u32le(&head, 0) != Some(0x0403_4b50) {
            return Err(format!("{}: entrada danificada", e.name));
        }
        let skip = u16le(&head, 26).unwrap_or(0) as u64 + u16le(&head, 28).unwrap_or(0) as u64;
        f.seek(SeekFrom::Start(e.local + 30 + skip)).map_err(|x| x.to_string())?;
        let mut raw = vec![0u8; e.csize as usize];
        f.read_exact(&mut raw).map_err(|x| x.to_string())?;
        drop(f);
        match e.method {
            0 => Ok(raw),
            8 => {
                let mut d = Vec::with_capacity(e.size as usize);
                flate2::read::DeflateDecoder::new(&raw[..]).read_to_end(&mut d).map_err(|x| format!("{}: {x}", e.name))?;
                Ok(d)
            }
            m => Err(format!("{}: compressão {m} não suportada", e.name)),
        }
    }

    pub fn read_string(&self, name: &str) -> Result<String, String> {
        let b = self.read(name)?;
        Ok(match String::from_utf8(b) {
            Ok(s) => s.trim_start_matches('\u{feff}').to_string(),
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        })
    }
}

/// What a file is, by its first bytes (a `.cbr` is often a renamed zip).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Magic {
    Zip,
    Rar,
    SevenZip,
    Tar,
    Pdf,
    Other,
}

pub fn magic(path: &Path) -> Magic {
    let mut b = [0u8; 264];
    let n = File::open(path).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
    let b = &b[..n];
    if b.starts_with(b"PK\x03\x04") || b.starts_with(b"PK\x05\x06") {
        Magic::Zip
    } else if b.starts_with(b"Rar!\x1a\x07") {
        Magic::Rar
    } else if b.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Magic::SevenZip
    } else if b.starts_with(b"%PDF") {
        Magic::Pdf
    } else if b.len() >= 262 && &b[257..262] == b"ustar" {
        Magic::Tar
    } else {
        Magic::Other
    }
}

fn run(cmd: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    std::process::Command::new(cmd).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())
}

/// The libarchive `tar`: `bsdtar`, or the system `tar` when it is one (macOS).
fn bsdtar() -> Option<&'static str> {
    static T: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *T.get_or_init(|| {
        for c in ["bsdtar", "/usr/bin/bsdtar", "/opt/homebrew/bin/bsdtar", "/usr/local/bin/bsdtar", "tar", "/usr/bin/tar"] {
            if let Ok(o) = run(c, &["--version".as_ref()]) {
                if String::from_utf8_lossy(&o.stdout).contains("bsdtar") {
                    return Some(c);
                }
            }
        }
        None
    })
}

/// A RAR, 7z or tar unpacked whole into `dest` (once: a later call finds
/// it done). Tries `bsdtar`, then `unrar`, then `7z`.
pub fn unpack(path: &Path, dest: &Path) -> Result<(), String> {
    let done = dest.join(".ateliet-ok");
    if done.exists() {
        return Ok(());
    }
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let p = path.as_os_str();
    let d = dest.as_os_str();
    let mut tried = Vec::new();
    if let Some(t) = bsdtar() {
        match run(t, &["-xf".as_ref(), p, "-C".as_ref(), d]) {
            Ok(o) if o.status.success() => {
                let _ = std::fs::write(&done, b"");
                return Ok(());
            }
            Ok(o) => tried.push(format!("bsdtar: {}", String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => tried.push(format!("bsdtar: {e}")),
        }
    }
    let mut dest_arg = dest.as_os_str().to_owned();
    dest_arg.push(std::path::MAIN_SEPARATOR_STR);
    for (cmd, args) in [("unrar", vec!["x".as_ref(), "-o+".as_ref(), "-y".as_ref(), p, dest_arg.as_os_str()]), ("unar", vec!["-f".as_ref(), "-D".as_ref(), "-o".as_ref(), d, p])] {
        if let Ok(o) = run(cmd, &args) {
            if o.status.success() {
                let _ = std::fs::write(&done, b"");
                return Ok(());
            }
            tried.push(format!("{cmd}: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
    }
    let mut out_arg = std::ffi::OsString::from("-o");
    out_arg.push(dest.as_os_str());
    for cmd in ["7z", "7zz", "7za"] {
        if let Ok(o) = run(cmd, &["x".as_ref(), "-y".as_ref(), out_arg.as_os_str(), p]) {
            if o.status.success() {
                let _ = std::fs::write(&done, b"");
                return Ok(());
            }
            tried.push(format!("{cmd}: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
    }
    if tried.is_empty() {
        Err("para ler CBR e CB7 é preciso o bsdtar (já vem no macOS), o unrar ou o 7z".into())
    } else {
        Err(tried.join("; "))
    }
}

/// Where an archive is unpacked: a folder of the system's cache, named by
/// the file's fingerprint.
pub fn cache_dir(fingerprint: &str) -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("ateliet").join("leitor").join(fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_one_by_one() {
        let dir = std::env::temp_dir().join(format!("ateliet-zip-{}", crate::journal::short_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.zip");
        let z = crate::board::zip::write_with(&[("mimetype".into(), b"application/epub+zip", false), ("OEBPS/a.xhtml".into(), "<p>Olá, mundo, olá, mundo, olá, mundo</p>".as_bytes(), true)]);
        std::fs::write(&p, z).unwrap();
        let zip = Zip::open(&p).unwrap();
        assert_eq!(zip.names().count(), 2);
        assert_eq!(zip.read_string("OEBPS/a.xhtml").unwrap(), "<p>Olá, mundo, olá, mundo, olá, mundo</p>");
        assert_eq!(zip.read("mimetype").unwrap(), b"application/epub+zip");
        assert_eq!(magic(&p), Magic::Zip);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
