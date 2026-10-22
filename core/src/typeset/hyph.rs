//! Portuguese hyphenation: Liang's algorithm over the TeX patterns
//! (hyph-pt from hyph-utf8, BSD 3-clause; see `core/assets/hyph-pt.txt`).

use once_cell::sync::Lazy;
use std::collections::HashMap;

const SRC: &str = include_str!("../../assets/hyph-pt.txt");
const LEFT: usize = 2;
const RIGHT: usize = 3;

struct Patterns {
    map: HashMap<String, Vec<u8>>,
    max: usize,
    exceptions: HashMap<String, Vec<usize>>,
}

static PT: Lazy<Patterns> = Lazy::new(|| {
    let mut map = HashMap::new();
    let mut exceptions = HashMap::new();
    let mut max = 0;
    let mut exc = false;
    for l in SRC.lines() {
        let l = l.trim();
        if l == "%exceptions" {
            exc = true;
            continue;
        }
        if l.is_empty() || l.starts_with('%') {
            continue;
        }
        if exc {
            let word: String = l.replace('-', "");
            let mut cuts = Vec::new();
            let mut n = 0;
            for c in l.chars() {
                if c == '-' {
                    cuts.push(n);
                } else {
                    n += 1;
                }
            }
            exceptions.insert(word, cuts);
            continue;
        }
        let mut letters = String::new();
        let mut digits = vec![0u8];
        for c in l.chars() {
            if let Some(d) = c.to_digit(10) {
                *digits.last_mut().unwrap() = d as u8;
            } else {
                letters.push(c);
                digits.push(0);
            }
        }
        max = max.max(letters.chars().count());
        map.insert(letters, digits);
    }
    Patterns { map, max, exceptions }
});

/// Where a word may break: the char counts before each allowed hyphen.
pub fn points(word: &str) -> Vec<usize> {
    let lower: Vec<char> = word.to_lowercase().chars().collect();
    let n = lower.len();
    if n < LEFT + RIGHT || !lower.iter().all(|c| c.is_alphabetic()) {
        return Vec::new();
    }
    let p = &*PT;
    if let Some(e) = p.exceptions.get(&lower.iter().collect::<String>()) {
        return e.clone();
    }
    let w: Vec<char> = std::iter::once('.').chain(lower.iter().cloned()).chain(std::iter::once('.')).collect();
    let mut score = vec![0u8; w.len() + 1];
    for i in 0..w.len() {
        let mut s = String::new();
        for c in &w[i..(i + p.max).min(w.len())] {
            s.push(*c);
            if let Some(d) = p.map.get(&s) {
                for (k, v) in d.iter().enumerate() {
                    if score[i + k] < *v {
                        score[i + k] = *v;
                    }
                }
            }
        }
    }
    // score[k] sits before w[k]; w[1] is the word's first letter
    (LEFT..=n - RIGHT).filter(|&c| score[c + 1] % 2 == 1).collect()
}

/// The word with soft hyphens where it may break (for tests and debugging).
pub fn show(word: &str) -> String {
    let pts = points(word);
    let mut out = String::new();
    for (i, c) in word.chars().enumerate() {
        if pts.contains(&i) {
            out.push('-');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portuguese() {
        assert_eq!(show("tempestade"), "tem-pes-tade");
        assert_eq!(show("construtor"), "cons-tru-tor");
        assert_eq!(show("Ninguém"), "Nin-guém");
        assert_eq!(show("água"), "água");
        assert_eq!(show("software"), "soft-ware");
        assert!(points("pai").is_empty());
    }
}
