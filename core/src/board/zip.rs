//! The zip container of `.storyboard` files: entries are stored, not
//! deflated (almost everything inside is PNG already), like the web editor
//! writes them. Reading also accepts deflated entries from other zip tools.

use std::io::Read;

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

pub fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// All entries of a zip, in directory order.
pub fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let start = bytes.len().saturating_sub(22 + 65535);
    let eocd = (start..=bytes.len().saturating_sub(22)).rev().find(|&i| u32le(bytes, i) == Some(0x0605_4b50)).ok_or("not a storyboard file (no zip directory)")?;
    let count = u16le(bytes, eocd + 10).ok_or("damaged zip")? as usize;
    let mut at = u32le(bytes, eocd + 16).ok_or("damaged zip")? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32le(bytes, at) != Some(0x0201_4b50) {
            return Err("damaged storyboard file".into());
        }
        let method = u16le(bytes, at + 10).ok_or("damaged zip")?;
        let csize = u32le(bytes, at + 20).ok_or("damaged zip")? as usize;
        let usize_ = u32le(bytes, at + 24).ok_or("damaged zip")? as usize;
        let name_len = u16le(bytes, at + 28).ok_or("damaged zip")? as usize;
        let extra_len = u16le(bytes, at + 30).ok_or("damaged zip")? as usize;
        let comment_len = u16le(bytes, at + 32).ok_or("damaged zip")? as usize;
        let local = u32le(bytes, at + 42).ok_or("damaged zip")? as usize;
        let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len).ok_or("damaged zip")?).into_owned();
        at += 46 + name_len + extra_len + comment_len;
        let l_name = u16le(bytes, local + 26).ok_or("damaged zip")? as usize;
        let l_extra = u16le(bytes, local + 28).ok_or("damaged zip")? as usize;
        let data_at = local + 30 + l_name + l_extra;
        let raw = bytes.get(data_at..data_at + csize).ok_or("damaged zip entry")?;
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let mut d = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(raw).read_to_end(&mut d).map_err(|e| format!("{name}: {e}"))?;
                d
            }
            m => return Err(format!("{name}: unsupported compression {m}")),
        };
        if !name.ends_with('/') {
            out.push((name, data));
        }
    }
    Ok(out)
}

/// A stored (uncompressed) zip of the given entries.
pub fn write(entries: &[(String, &[u8])]) -> Vec<u8> {
    let e: Vec<(String, &[u8], bool)> = entries.iter().map(|(n, d)| (n.clone(), *d, false)).collect();
    write_with(&e)
}

/// A zip where entries marked `true` are deflated (the others stored).
pub fn write_with(entries: &[(String, &[u8], bool)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in entries {
        let crc = crc32(data);
        let offset = out.len() as u32;
        let n = name.as_bytes();
        let packed: Vec<u8>;
        let (method, body): (u16, &[u8]) = if *deflate {
            let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            let _ = enc.write_all(data);
            packed = enc.finish().unwrap_or_default();
            if packed.len() < data.len() { (8, &packed) } else { (0, data) }
        } else {
            (0, data)
        };
        // local header
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // utf-8 names
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // time/date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(n.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(n);
        out.extend_from_slice(body);
        // central directory entry
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(n.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 8]); // extra and comment lengths, disk, internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(n);
    }
    let cd_offset = out.len() as u32;
    let cd_len = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let a = b"hello".to_vec();
        let b = vec![7u8; 1000];
        let z = write(&[("board.json".into(), &a), ("tiles/l/0_0.png".into(), &b)]);
        let back = read(&z).unwrap();
        assert_eq!(back.len(), 2);
        let z2 = write_with(&[("a.txt".into(), &b, true), ("b".into(), &a, false)]);
        let back2 = read(&z2).unwrap();
        assert_eq!(back2[0].1, b);
        assert!(z2.len() < 400);
        assert_eq!(back[0].0, "board.json");
        assert_eq!(back[0].1, a);
        assert_eq!(back[1].1, b);
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
