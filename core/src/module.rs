//! Reader for BibleWorks 8 `.dbx` version modules (read-only, from the user's own install).
//!
//! Layout (reverse-engineered from the shipped files, little-endian u32 header):
//!   h[3]..h[4]  verse offset table, one u32 per verse plus an end sentinel
//!   h[4]..h[5]  verse text; verse i = text[off[i]..off[i+1]], Latin-1 bytes. English modules tag
//!               Strong's as `<01254>`, morph as `(08804)`, footnote markers as `<Ra>`, notes as `{...}`.
//!   h[5]..      optional structure block (see `Versification::from_tail`); without it the module
//!               uses the KJV (31,102 verses) or Hebrew master (23,213) structure.

use crate::versification::Versification;
use std::{
    fs, io,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub struct Module {
    data: Vec<u8>,
    table: usize,
    text: usize,
    verses: usize,
    pub vers: Versification,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub text: String,
    pub strongs: Option<String>,
    pub morph: Option<String>,
}

/// What can be learned about a module from its header and the start of its text.
pub struct Probe {
    pub vers: Versification,
    /// Share of bytes >= 0x80 in the first verses: high for Chinese, Cyrillic, etc.
    pub high_ratio: f32,
    /// `lemma@code` morphology module.
    pub is_morph: bool,
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

fn header(h: &[u8; 28]) -> (usize, usize, usize) {
    let w = |i: usize| u32::from_le_bytes(h[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    (w(3), w(4), w(5))
}

/// Structure for a module with `verses` records, from its tail block or the default for its count.
fn structure(tail: &[u8], verses: usize) -> Option<Versification> {
    Versification::from_tail(tail)
        .or_else(|| match verses {
            31_102 => Some(Versification::kjv()),
            23_213 => Some(Versification::hebrew()),
            _ => None,
        })
        .filter(|v| v.total() == verses)
}

impl Module {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let data = fs::read(path)?;
        if data.len() < 28 {
            return Err(bad("too short"));
        }
        let (table, text, declared) = header(data[..28].try_into().unwrap());
        if table >= text || text > data.len() || (text - table) % 4 != 0 || text - table < 8 {
            return Err(bad("not a verse module"));
        }
        let verses = (text - table) / 4 - 1;
        let tail = data.get(declared.min(data.len())..).unwrap_or(&[]);
        let vers = structure(tail, verses).ok_or_else(|| bad("unknown versification"))?;
        let m = Module { data, table, text, verses, vers };
        // Cheap sanity check: offsets must be monotonic and in range at both ends.
        if m.off(0) != 0 || m.text + m.off(verses) > m.data.len() || m.off(verses) < m.off(verses - 1) {
            return Err(bad("bad verse table"));
        }
        Ok(m)
    }

    /// Header-only inspection (a few KB read per file), `None` if `path` isn't a readable verse module.
    pub fn probe(path: impl AsRef<Path>) -> Option<Probe> {
        let mut f = fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len() as usize;
        let mut h = [0u8; 28];
        f.read_exact(&mut h).ok()?;
        let (table, text, declared) = header(&h);
        if table >= text || text > len || (text - table) % 4 != 0 || text - table < 8 || declared > len {
            return None;
        }
        let verses = (text - table) / 4 - 1;
        let mut tail = vec![0u8; (len - declared).min(1 << 16)];
        f.seek(SeekFrom::Start(declared as u64)).ok()?;
        f.read_exact(&mut tail).ok()?;
        let vers = structure(&tail, verses)?;
        let mut sample = vec![0u8; 4096.min(declared.saturating_sub(text))];
        f.seek(SeekFrom::Start(text as u64)).ok()?;
        f.read_exact(&mut sample).ok()?;
        let high = sample.iter().filter(|&&b| b >= 0x80).count() as f32 / sample.len().max(1) as f32;
        let at = sample.iter().filter(|&&b| b == b'@').count();
        Some(Probe { vers, high_ratio: high, is_morph: at > 20 })
    }

    pub fn verse_count(&self) -> usize {
        self.verses
    }

    fn off(&self, i: usize) -> usize {
        let p = self.table + i * 4;
        u32::from_le_bytes(self.data[p..p + 4].try_into().unwrap()) as usize
    }

    /// Raw tagged text of verse `i`.
    pub fn raw(&self, i: usize) -> Option<String> {
        if i >= self.verses {
            return None;
        }
        let bytes = self.data.get(self.text + self.off(i)..self.text + self.off(i + 1))?;
        // Latin-1; Greek and Hebrew modules are pure ASCII and go through `script::decode`.
        Some(bytes.iter().map(|&b| b as char).collect())
    }
}

fn is_strongs(s: &str) -> bool {
    let d = s.trim_end_matches(|c: char| c.is_ascii_lowercase());
    !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())
}

/// Append display text, normalising the gaps that removed tags leave behind.
fn push_text(out: &mut Vec<Token>, mut s: &str) {
    if s.contains('>') {
        // real tags are consumed whole, so a `>` that reaches here is damage in the module
        return push_text(out, &s.replace('>', ""));
    }
    if let Some(last) = out.last_mut() {
        if last.text.ends_with(char::is_whitespace) {
            if s.starts_with(char::is_whitespace) {
                s = s.trim_start();
            } else if s.starts_with([',', '.', ';', ':', '!', '?', ')']) {
                last.text.truncate(last.text.trim_end().len());
                if last.text.is_empty() && last.strongs.is_none() {
                    out.pop();
                }
            }
        }
    }
    if !s.is_empty() {
        out.push(Token { text: s.to_string(), strongs: None, morph: None });
    }
}

/// Split raw tagged text into display tokens; a Strong's tag attaches to the word before it.
// ponytail: tag binds to the last word only (multi-word glosses lose the rest) and bare
// markers like <0853> with no word before them are dropped.
pub fn tokens(raw: &str) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let b = raw.as_bytes();
    let (mut i, mut start) = (0, 0);
    while i < b.len() {
        // `<<...>>` wraps a Psalm title; keep the words, drop the delimiters.
        if raw[i..].starts_with("<<") || raw[i..].starts_with(">>") {
            push_text(&mut out, &raw[start..i]);
            i += 2;
            start = i;
            continue;
        }
        let close = match b[i] {
            b'<' => '>',
            b'{' => '}',
            b'(' => ')',
            _ => {
                i += 1;
                continue;
            }
        };
        // `<...>` tags may nest in damaged text (`<N<N2> 1>`), so match them by depth.
        let rel = if b[i] == b'<' { tag_end(&raw[i + 1..]) } else { raw[i + 1..].find(close) };
        let Some(rel) = rel else {
            i += 1;
            continue;
        };
        let inner = &raw[i + 1..i + 1 + rel];
        let numeric = !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit());
        if b[i] == b'(' && !numeric {
            // An ordinary parenthesis: keep just the bracket and carry on, so tags inside the
            // parenthetical ("(I speak <3004> (5719) as a man)") are still read.
            push_text(&mut out, &raw[start..i + 1]);
            i += 1;
            start = i;
            continue;
        } else {
            push_text(&mut out, &raw[start..i]);
            match b[i] {
                b'<' if is_strongs(inner) => attach_strongs(&mut out, inner),
                b'(' => {
                    if let Some(t) = out.iter_mut().rev().find(|t| t.strongs.is_some()) {
                        t.morph = Some(inner.to_string());
                    }
                }
                _ => {} // other tags and {notes}
            }
        }
        i += rel + 2;
        start = i;
    }
    push_text(&mut out, &raw[start..]);
    out
}

/// Offset of the `>` that closes a tag whose `<` came just before `s`, counting nested `<`.
fn tag_end(s: &str) -> Option<usize> {
    let mut depth = 1;
    for (k, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

fn attach_strongs(out: &mut Vec<Token>, num: &str) {
    let Some(last) = out.last_mut() else { return };
    if last.strongs.is_some() {
        return;
    }
    let t = last.text.clone();
    let body = t.trim_end();
    let Some(word_start) = body.rfind(char::is_whitespace).map(|p| p + 1).or(Some(0)) else { return };
    let word = &body[word_start..];
    if word.is_empty() {
        return;
    }
    let (prefix, trail) = (&t[..word_start], &t[body.len()..]);
    let num = Some(num.to_string());
    if prefix.is_empty() {
        last.text = word.to_string();
        last.strongs = num;
    } else {
        last.text = prefix.to_string();
        out.push(Token { text: word.to_string(), strongs: num, morph: None });
    }
    if !trail.is_empty() {
        out.push(Token { text: trail.to_string(), strongs: None, morph: None });
    }
}

/// Clean display text with all tags and notes removed.
pub fn plain(raw: &str) -> String {
    tokens(raw).into_iter().map(|t| t.text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_strongs_and_morph() {
        let t = tokens("In the beginning <07225> God <0430> created <01254> (08804) <0853> the heaven <08064>.");
        let words: Vec<_> = t.iter().filter(|t| t.strongs.is_some()).map(|t| (t.text.as_str(), t.strongs.as_deref().unwrap())).collect();
        assert_eq!(words, [("beginning", "07225"), ("God", "0430"), ("created", "01254"), ("heaven", "08064")]);
        assert_eq!(t.iter().find(|t| t.text == "created").unwrap().morph.as_deref(), Some("08804"));
        assert_eq!(plain("In the beginning <07225> God <0430>, {note} and (see) <Ra>all."), "In the beginning God, and (see) all.");
    }

    #[test]
    fn nested_tags_in_damaged_text() {
        assert_eq!(plain("of land<N<N2> 1> <07704>. more"), "of land. more");
        assert_eq!(plain("a < b and <0430> God"), "a < b and God"); // a lone `<` is just text
        assert_eq!(plain("<<kill him.> <04191> (08687)>> Deliver"), "kill him. Deliver"); // a stray `>` is dropped
    }

    #[test]
    fn tags_inside_a_parenthesis_are_still_read() {
        assert_eq!(plain("(I speak <3004> (5719) as a man) and <2532> more (see) ok"), "(I speak as a man) and more (see) ok");
        let t = tokens("(I speak <3004> (5719) as a man)");
        let speak = t.iter().find(|t| t.text == "speak").unwrap();
        assert_eq!((speak.strongs.as_deref(), speak.morph.as_deref()), (Some("3004"), Some("5719")));
    }

    /// No tag fragments may leak into displayed text, in any verse of the tagged English versions.
    #[test]
    fn no_tag_fragments_leak_into_display_text() {
        let Some(dir) = crate::install_dir() else { return };
        for id in ["kjv", "nkj", "nas", "nau", "rwb", "kjg"] {
            let Ok(m) = Module::open(dir.join(format!("databases/{id}.dbx"))) else { continue };
            for i in 0..m.verse_count() {
                let text = plain(&m.raw(i).unwrap());
                assert!(!text.contains(['<', '>']), "{id} verse {i}: {text}");
            }
        }
    }

    #[test]
    fn psalm_title_delimiters() {
        assert_eq!(plain("<<A Psalm <04210> of David <01732>.>> LORD <03068>, how"), "A Psalm of David. LORD, how");
    }

    #[test]
    fn adjacent_tags_keep_word_spacing() {
        assert_eq!(plain("gave <1325> His <N1><Rc>only <3439> begotten <3439> Son <5207>, that He <Rb>gave"), "gave His only begotten Son, that He gave");
        assert_eq!(plain("created <01254> (08804) <0853> the heaven <08064> and"), "created the heaven and");
    }

    /// Runs against the real install when present; skipped otherwise.
    #[test]
    fn reads_installed_kjv() {
        let Some(dir) = crate::install_dir() else { return };
        let m = Module::open(dir.join("databases/kjv.dbx")).unwrap();
        assert_eq!(m.verse_count(), crate::canon::TOTAL_VERSES);
        assert!(plain(&m.raw(0).unwrap()).starts_with("In the beginning God created the heaven and the earth."));
        let (b, c, v) = m.vers.locate(m.verse_count() - 1).unwrap();
        assert_eq!((b, c, v), (65, 22, 21));
        let jn316 = plain(&m.raw(m.vers.index(42, 3, 16).unwrap()).unwrap());
        assert!(jn316.starts_with("For God so loved the world"), "{jn316}");
    }
}
