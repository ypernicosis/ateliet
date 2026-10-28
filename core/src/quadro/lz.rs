//! LZString's `compressToBase64` / `decompressFromBase64` (pieroxy's
//! lz-string 1.5), the compression the Excalidraw plugin uses for the
//! `compressed-json` drawing. JavaScript strings are UTF-16, so both work
//! on UTF-16 code units and give the same output as the library.

use std::collections::{HashMap, HashSet};

const KEY: &[u8; 65] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";

/// Bits written six at a time as base64 characters.
struct Bits {
    out: Vec<u8>,
    val: u32,
    pos: u32,
}

impl Bits {
    fn push(&mut self, bit: u32) {
        self.val = (self.val << 1) | bit;
        if self.pos == 5 {
            self.pos = 0;
            self.out.push(KEY[self.val as usize]);
            self.val = 0;
        } else {
            self.pos += 1;
        }
    }

    /// `n` bits of `value`, lowest first (lz-string's order).
    fn value(&mut self, mut value: u32, n: u32) {
        for _ in 0..n {
            self.push(value & 1);
            value >>= 1;
        }
    }
}

pub fn compress_to_base64(input: &str) -> String {
    let units: Vec<u16> = input.encode_utf16().collect();
    let mut b = Bits { out: Vec::new(), val: 0, pos: 0 };
    // the dictionary as a trie: (code of w, next unit) → code of w + unit
    let mut single: HashMap<u16, u32> = HashMap::new();
    let mut trie: HashMap<(u32, u16), u32> = HashMap::new();
    let mut to_create: HashSet<u16> = HashSet::new();
    let (mut enlarge, mut size, mut bits) = (2u32, 3u32, 2u32);
    // w: its code, and its unit when it is a single one
    let mut w: Option<(u32, Option<u16>)> = None;
    fn emit(b: &mut Bits, w: (u32, Option<u16>), to_create: &mut HashSet<u16>, enlarge: &mut u32, bits: &mut u32) {
        match w.1.filter(|c| to_create.contains(c)) {
            Some(c) => {
                if c < 256 {
                    b.value(0, *bits);
                    b.value(c as u32, 8);
                } else {
                    b.value(1, *bits);
                    b.value(c as u32, 16);
                }
                *enlarge -= 1;
                if *enlarge == 0 {
                    *enlarge = 1 << *bits;
                    *bits += 1;
                }
                to_create.remove(&c);
            }
            None => b.value(w.0, *bits),
        }
        *enlarge -= 1;
        if *enlarge == 0 {
            *enlarge = 1 << *bits;
            *bits += 1;
        }
    }
    for &c in &units {
        if let std::collections::hash_map::Entry::Vacant(e) = single.entry(c) {
            e.insert(size);
            size += 1;
            to_create.insert(c);
        }
        let Some(cur) = w else {
            w = Some((single[&c], Some(c)));
            continue;
        };
        if let Some(&code) = trie.get(&(cur.0, c)) {
            w = Some((code, None));
        } else {
            emit(&mut b, cur, &mut to_create, &mut enlarge, &mut bits);
            trie.insert((cur.0, c), size);
            size += 1;
            w = Some((single[&c], Some(c)));
        }
    }
    if let Some(cur) = w {
        emit(&mut b, cur, &mut to_create, &mut enlarge, &mut bits);
    }
    b.value(2, bits);
    // flush the last character
    loop {
        b.val <<= 1;
        if b.pos == 5 {
            b.out.push(KEY[b.val as usize]);
            break;
        }
        b.pos += 1;
    }
    let mut s = String::from_utf8(b.out).unwrap_or_default();
    match s.len() % 4 {
        1 => s.push_str("==="),
        2 => s.push_str("=="),
        3 => s.push('='),
        _ => {}
    }
    s
}

/// None when the data is not valid lz-string (the library's `null`).
pub fn decompress_from_base64(input: &str) -> Option<String> {
    let chars: Vec<u8> = input.bytes().filter(|c| *c != b'\n' && *c != b'\r').collect();
    if chars.is_empty() {
        return None;
    }
    let value = |i: usize| -> u32 { chars.get(i).and_then(|c| KEY.iter().position(|k| k == c)).map(|v| v as u32).unwrap_or(0) };
    let len = chars.len();
    let (mut val, mut pos, mut index) = (value(0), 32u32, 1usize);
    let read = |n: u32, val: &mut u32, pos: &mut u32, index: &mut usize| -> u32 {
        let mut out = 0;
        for i in 0..n {
            let bit = *val & *pos;
            *pos >>= 1;
            if *pos == 0 {
                *pos = 32;
                *val = value(*index);
                *index += 1;
            }
            if bit > 0 {
                out |= 1 << i;
            }
        }
        out
    };
    let mut dict: Vec<Vec<u16>> = vec![Vec::new(), Vec::new(), Vec::new()];
    let (mut enlarge, mut bits) = (4u32, 3u32);
    let first = match read(2, &mut val, &mut pos, &mut index) {
        0 => read(8, &mut val, &mut pos, &mut index) as u16,
        1 => read(16, &mut val, &mut pos, &mut index) as u16,
        _ => return Some(String::new()),
    };
    dict.push(vec![first]);
    let mut w = vec![first];
    let mut result: Vec<u16> = vec![first];
    loop {
        if index > len {
            return Some(String::new());
        }
        let mut c = read(bits, &mut val, &mut pos, &mut index) as usize;
        match c {
            0 | 1 => {
                let n = if c == 0 { 8 } else { 16 };
                let unit = read(n, &mut val, &mut pos, &mut index) as u16;
                dict.push(vec![unit]);
                c = dict.len() - 1;
                enlarge -= 1;
            }
            2 => return Some(String::from_utf16_lossy(&result)),
            _ => {}
        }
        if enlarge == 0 {
            enlarge = 1 << bits;
            bits += 1;
        }
        let entry = if c < dict.len() {
            dict[c].clone()
        } else if c == dict.len() {
            let mut e = w.clone();
            e.push(w[0]);
            e
        } else {
            return None;
        };
        result.extend_from_slice(&entry);
        let mut add = w.clone();
        add.push(entry[0]);
        dict.push(add);
        enlarge -= 1;
        w = entry;
        if enlarge == 0 {
            enlarge = 1 << bits;
            bits += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // made with lz-string 1.5.0 (`LZString.compressToBase64`)
    const VECTORS: &[(&str, &str)] = &[
        ("", "Q==="),
        ("a", "IZA="),
        ("hello hello hello", "BYUwNmD2AEoTcpA="),
        ("ação — “olá” 🎨", "IYc4xw9gBIKARYOAQQDYEPC4BFQPBuAr9oA="),
        (r#"{"type":"excalidraw","version":2,"elements":[{"id":"abc","type":"rectangle","x":10,"y":20}]}"#, "N4IgLgngDgpiBcIYA8DGBDANgSwCYCd0B3EAGhADcZ8BnbAewDsEAmcmTGAWxkbBoQBtUHgQh0AI1Rlw0OInwxUYdIwDmnGcgQBGAAzkIrPQF8AuiaA="),
        ("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab", "IY18ZXTsQRkA"),
    ];

    #[test]
    fn matches_the_library() {
        for (plain, packed) in VECTORS {
            assert_eq!(&compress_to_base64(plain), packed, "compress {plain:?}");
            if !plain.is_empty() {
                assert_eq!(decompress_from_base64(packed).as_deref(), Some(*plain), "decompress {packed:?}");
            }
        }
    }

    #[test]
    fn round_trip_long_text() {
        let text: String = (0..3000).map(|i| format!("{{\"id\":\"el{i}\",\"x\":{},\"text\":\"linha {i} — ü\"}},", i * 7 % 113)).collect();
        let packed = compress_to_base64(&text);
        // the plugin breaks the base64 in lines of 256 with blank lines between
        let chunked: String = packed.as_bytes().chunks(256).map(|c| format!("{}\n\n", std::str::from_utf8(c).unwrap())).collect();
        assert_eq!(decompress_from_base64(chunked.trim()).unwrap(), text);
    }
}
