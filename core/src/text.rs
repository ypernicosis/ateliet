//! Position helpers. Obsidian computes positions in JavaScript, so `col` and
//! `offset` are measured in UTF-16 code units. We keep a byte → UTF-16 map per
//! line so plugins can `content.slice(pos.start.offset, pos.end.offset)`.

use serde_json::{json, Value};

pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,   // byte offset of each line start
    starts16: Vec<usize>, // UTF-16 offset of each line start
}

impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let mut starts = vec![0];
        let mut starts16 = vec![0];
        let mut u16pos = 0usize;
        for (i, ch) in text.char_indices() {
            u16pos += ch.len_utf16();
            if ch == '\n' {
                starts.push(i + 1);
                starts16.push(u16pos);
            }
        }
        LineIndex { text, starts, starts16 }
    }

    pub fn line_of(&self, byte: usize) -> usize {
        match self.starts.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.starts[line.min(self.starts.len() - 1)]
    }

    /// Byte offset of the end of `line`, excluding the newline.
    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 < self.starts.len() {
            let mut e = self.starts[line + 1] - 1;
            if e > 0 && self.text.as_bytes().get(e - 1) == Some(&b'\r') {
                e -= 1;
            }
            e
        } else {
            self.text.len()
        }
    }

    pub fn line_text(&self, line: usize) -> &'a str {
        &self.text[self.line_start(line)..self.line_end(line)]
    }

    pub fn loc(&self, byte: usize) -> Value {
        let byte = byte.min(self.text.len());
        let line = self.line_of(byte);
        let col: usize = self.text[self.starts[line]..byte].encode_utf16().count();
        json!({ "line": line, "col": col, "offset": self.starts16[line] + col })
    }

    pub fn pos(&self, start: usize, end: usize) -> Value {
        json!({ "start": self.loc(start), "end": self.loc(end) })
    }

    /// Trim trailing whitespace/newlines from a byte range end.
    pub fn trim_end(&self, start: usize, mut end: usize) -> usize {
        let b = self.text.as_bytes();
        while end > start && matches!(b[end - 1], b'\n' | b'\r' | b' ' | b'\t') {
            end -= 1;
        }
        end
    }
}

/// Replace every non-newline byte in `range` with a space, keeping byte offsets
/// (and UTF-8 validity) intact. Used to hide frontmatter/comments from parsers.
pub fn blank_range(buf: &mut [u8], start: usize, end: usize) {
    for b in &mut buf[start..end] {
        if *b != b'\n' && *b != b'\r' {
            *b = b' ';
        }
    }
}

/// Compare like `localeCompare(…, { numeric: true })`: case and accents
/// matter only to break ties, runs of digits compare as numbers.
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    use unicode_normalization::UnicodeNormalization;
    let fold = |s: &str| -> Vec<char> { s.nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).flat_map(char::to_lowercase).collect() };
    let (x, y) = (fold(a), fold(b));
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let si = i;
            while i < x.len() && x[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < y.len() && y[j].is_ascii_digit() {
                j += 1;
            }
            let n1: String = x[si..i].iter().collect::<String>().trim_start_matches('0').to_string();
            let n2: String = y[sj..j].iter().collect::<String>().trim_start_matches('0').to_string();
            let o = n1.len().cmp(&n2.len()).then_with(|| n1.cmp(&n2));
            if o != Ordering::Equal {
                return o;
            }
            continue;
        }
        let o = x[i].cmp(&y[j]);
        if o != Ordering::Equal {
            return o;
        }
        i += 1;
        j += 1;
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| a.cmp(b))
}
