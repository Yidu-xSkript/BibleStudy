//! BibleWorks lexicons and dictionaries (read-only, from the user's own install).
//!
//! Container (all `*.dbx` / `*.dat` lexicon files): u32 entry count, (count+1) u32 absolute
//! offsets, then the entries. Two families:
//!   * Strong's: `grkdic.dat`, `hebdic.dat` - plain text keyed by Strong's number.
//!   * Full lexicons (`thdict`, `gntdict`, `ging`, `lsdict`, `lndict`, `fbdblex`, `hollex`): an
//!     optional ASCII key, then styled text: \x01 Greek, \x02 bold, \x03 italic, \x05 Hebrew,
//!     \x04 resets, `<<ref:BHS/GNT_1Jo 4:10$$1 John 4:10>>` links. Everything after the first NUL
//!     is index data and is ignored.

use crate::lexenc::{lex_greek, lex_hebrew, lex_hebrew_consonants, mc_consonants};
use std::{collections::HashMap, fs, io, path::Path};
use unicode_normalization::UnicodeNormalization;

pub struct Table {
    data: Vec<u8>,
    n: usize,
}

impl Table {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let data = fs::read(path)?;
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        if data.len() < 8 {
            return Err(bad("too short"));
        }
        let n = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        let end = 4 + (n + 1) * 4;
        if n == 0 || end > data.len() {
            return Err(bad("not a lexicon table"));
        }
        let t = Table { data, n };
        let (first, last) = (t.off(0), t.off(n));
        if first != end || last > t.data.len() || first > last {
            return Err(bad("bad lexicon offsets"));
        }
        Ok(t)
    }

    pub fn len(&self) -> usize {
        self.n
    }

    fn off(&self, i: usize) -> usize {
        let p = 4 + i * 4;
        u32::from_le_bytes(self.data[p..p + 4].try_into().unwrap()) as usize
    }

    /// Entry `i` of a table where unused slots have offset 0 (e.g. verses with no TSK entry).
    /// An entry runs to the next real offset, not to the next slot. `None` for an unused slot.
    pub fn get_sparse(&self, i: usize) -> Option<String> {
        let first = 4 + (self.n + 1) * 4;
        let start = self.off(i.min(self.n));
        if i >= self.n || start < first {
            return None;
        }
        let end = (i + 1..=self.n).map(|j| self.off(j)).find(|&o| o >= first && o > start).unwrap_or(self.data.len()).min(self.data.len());
        let b = self.data.get(start..end)?;
        let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
        Some(b.iter().map(|&c| c as char).collect())
    }

    /// Entry `i` as Latin-1 text, cut at the first NUL.
    pub fn get(&self, i: usize) -> Option<String> {
        if i >= self.n {
            return None;
        }
        let b = self.data.get(self.off(i)..self.off(i + 1))?;
        let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
        Some(b.iter().map(|&c| c as char).collect())
    }
}

/// Lemma comparison keys: exact (NFC) and accent-insensitive.
pub fn greek_keys(s: &str) -> (String, String) {
    let exact: String = s.nfc().collect();
    let loose: String = s.nfd().filter(|c| !('\u{300}'..='\u{36F}').contains(c)).collect::<String>().to_lowercase().replace('ς', "σ");
    (exact, loose)
}

// ---------------------------------------------------------------- Strong's

#[derive(Clone, Debug, PartialEq)]
pub struct StrongsEntry {
    pub number: u32,
    pub hebrew: bool,
    /// Greek headword in Unicode; empty for Hebrew (its encoding isn't fully decoded).
    pub word: String,
    pub translit: String,
    pub pronunciation: String,
    /// Derivation, usage and definition, with line breaks normalised.
    pub body: String,
}

pub struct Strongs {
    greek: Table,
    hebrew: Table,
    g_exact: HashMap<String, Vec<u32>>,
    g_loose: HashMap<String, Vec<u32>>,
    h_skeleton: HashMap<String, Vec<u32>>,
}

/// The `\G<word>\n` / `\h<word>\n` tags on a Strong's header line. A tag starts after whitespace and
/// ends at the first `\n` that is followed by whitespace (a mark `\` can precede the letter `n`).
fn header_words(line: &str) -> (Vec<(char, String)>, String) {
    let (mut out, mut plain) = (Vec::new(), String::new());
    let b: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == '\\' && (b[i + 1] == 'G' || b[i + 1] == 'h') && (i == 0 || b[i - 1].is_whitespace()) {
            let mut j = i + 2;
            while j + 1 < b.len() && !(b[j] == '\\' && b[j + 1] == 'n' && b.get(j + 2).map_or(true, |c| c.is_whitespace())) {
                j += 1;
            }
            out.push((b[i + 1], b[i + 2..j.min(b.len())].iter().collect()));
            i = j + 2;
            plain.push(' ');
        } else {
            plain.push(b[i]);
            i += 1;
        }
    }
    (out, plain)
}

/// (number, words, transliteration, pronunciation, body) of a Strong's entry.
fn strongs_header(text: &str) -> Option<(u32, Vec<(char, String)>, String, String, String)> {
    let mut lines = text.lines();
    let _id = lines.next()?;
    let head = lines.next()?.trim();
    let (num, rest) = head.split_once(' ')?;
    let number: u32 = num.trim().parse().ok()?;
    let (words, plain) = header_words(rest);
    if words.is_empty() {
        return None;
    }
    // Once the tags are gone the line starts with the transliteration and pronunciation of the first word.
    let after = plain.trim();
    let first = after.split(" or ").next().unwrap_or(after);
    let (translit, pron) = match first.find('{') {
        Some(p) => (first[..p].trim().to_string(), first[p + 1..].split('}').next().unwrap_or("").trim().to_string()),
        None => (first.trim().to_string(), String::new()),
    };
    let mut body: Vec<&str> = lines.collect();
    // A long header wraps: the pronunciation can start the next line.
    let pron = match (pron.is_empty(), body.first().map(|l| l.trim())) {
        (true, Some(l)) if l.starts_with('{') => {
            let p = l.trim_start_matches('{').split('}').next().unwrap_or("").trim().to_string();
            body.remove(0);
            p
        }
        _ => pron,
    };
    Some((number, words, translit, pron, body.join("\n").trim().to_string()))
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

/// The lemma itself, then the active form of a deponent (`-ομαι` -> `-ω`), which dictionaries often list instead.
pub fn lemma_variants(lemma: &str) -> Vec<String> {
    let l: String = lemma.nfc().collect();
    let mut v = vec![l.clone()];
    if let Some(stem) = l.strip_suffix("ομαι") {
        v.push(format!("{stem}ω"));
    }
    v
}

impl Strongs {
    pub fn open(databases: &Path) -> io::Result<Self> {
        let greek = Table::open(databases.join("grkdic.dat"))?;
        let hebrew = Table::open(databases.join("hebdic.dat"))?;
        let (mut g_exact, mut g_loose, mut h_skeleton) = (HashMap::new(), HashMap::new(), HashMap::new());
        for i in 0..greek.len() {
            let Some((n, words, ..)) = greek.get(i).and_then(|t| strongs_header(&t)) else { continue };
            for (_, w) in words.iter().filter(|(k, _)| *k == 'G') {
                let (exact, loose) = greek_keys(&lex_greek(w));
                push_unique(g_exact.entry(exact).or_default(), n);
                push_unique(g_loose.entry(loose).or_default(), n);
            }
        }
        for i in 0..hebrew.len() {
            let Some((n, words, ..)) = hebrew.get(i).and_then(|t| strongs_header(&t)) else { continue };
            for (_, w) in words.iter().filter(|(k, _)| *k == 'h') {
                push_unique(h_skeleton.entry(lex_hebrew_consonants(w)).or_default(), n);
            }
        }
        Ok(Strongs { greek, hebrew, g_exact, g_loose, h_skeleton })
    }

    pub fn entry(&self, hebrew: bool, number: u32) -> Option<StrongsEntry> {
        let table = if hebrew { &self.hebrew } else { &self.greek };
        // Entry i is Strong's number i+1 (Greek) or i (Hebrew, entry 0 is a note), but verify.
        let guess = if hebrew { number as usize } else { number as usize - 1 };
        for i in [guess, guess.saturating_sub(1), guess + 1] {
            if let Some((n, words, translit, pron, body)) = table.get(i).and_then(|t| strongs_header(&t)) {
                if n == number {
                    let word = if hebrew { words.iter().find(|(k, _)| *k == 'h').map(|(_, w)| lex_hebrew(w)).unwrap_or_default() } else { lex_greek(&words[0].1) };
                    return Some(StrongsEntry { number, hebrew, word, translit, pronunciation: pron, body });
                }
            }
        }
        None
    }

    /// Strong's numbers for a Greek lemma (Unicode). Exact match first, then accent-insensitive.
    pub fn greek_lemma(&self, lemma: &str) -> Vec<u32> {
        for l in lemma_variants(lemma) {
            let (exact, loose) = greek_keys(&l);
            if let Some(v) = self.g_exact.get(&exact).or_else(|| self.g_loose.get(&loose)) {
                return v.clone();
            }
        }
        Vec::new()
    }

    /// The words a Strong's number stands for, to match against text lemmas: Greek lemmas in
    /// Unicode (with the deponent form of any `-ω` verb), or Hebrew consonant skeletons.
    pub fn lemmas_of(&self, hebrew: bool, number: u32) -> Vec<String> {
        let table = if hebrew { &self.hebrew } else { &self.greek };
        let guess = if hebrew { number as usize } else { number as usize - 1 };
        for i in [guess, guess.saturating_sub(1), guess + 1] {
            let Some((n, words, ..)) = table.get(i).and_then(|t| strongs_header(&t)) else { continue };
            if n != number {
                continue;
            }
            let mut out = Vec::new();
            for (_, w) in &words {
                if hebrew {
                    push_unique(&mut out, lex_hebrew_consonants(w));
                } else {
                    let g = lex_greek(w);
                    if let Some(stem) = g.strip_suffix('ω') {
                        push_unique(&mut out, format!("{stem}ομαι"));
                    }
                    push_unique(&mut out, g);
                }
            }
            return out;
        }
        Vec::new()
    }

    /// Strong's numbers whose Hebrew word has the same consonants as a Westminster lemma.
    pub fn hebrew_lemma(&self, mc_lemma: &str) -> Vec<u32> {
        self.h_skeleton.get(&mc_consonants(mc_lemma)).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------- full lexicons

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub greek: bool,
    pub hebrew: bool,
    /// Scripture reference this text links to: (book code, chapter, verse).
    pub link: Option<(String, usize, usize)>,
}

fn parse_ref(target: &str) -> Option<(String, usize, usize)> {
    let t = target.rsplit('_').next().unwrap_or(target); // "BHS/GNT_1Jo 4:10" -> "1Jo 4:10"
    let (book, cv) = t.trim().split_once([' ', '.'])?;
    let (c, v) = cv.split_once(':')?;
    let v: String = v.chars().take_while(char::is_ascii_digit).collect();
    (book.len() == 3).then(|| Some((book.to_string(), c.trim().parse().ok()?, v.parse().ok()?))).flatten()
}

/// Parse styled lexicon text into spans (Greek decoded to Unicode, Hebrew to its consonants).
pub fn parse_spans(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let (mut bold, mut italic, mut greek, mut hebrew) = (false, false, false, false);
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut Vec<Span>, b, i, g, h| {
        if !buf.is_empty() {
            let raw = std::mem::take(buf);
            let text = if g { lex_greek(&raw) } else if h { lex_hebrew(&raw) } else { raw };
            if !text.is_empty() {
                out.push(Span { text, bold: b, italic: i, greek: g, hebrew: h, link: None });
            }
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("<<ref:") {
            if let Some(end) = r.find(">>") {
                let inner = &r[..end];
                if let Some((target, label)) = inner.split_once("$$") {
                    flush(&mut buf, &mut out, bold, italic, greek, hebrew);
                    let label: String = label.chars().map(|c| if c == '\u{19}' { ':' } else { c }).filter(|c| !c.is_control()).collect();
                    out.push(Span { text: label, bold, italic, greek: false, hebrew: false, link: parse_ref(target) });
                    rest = &r[end + 2..];
                    continue;
                }
            }
        }
        rest = &rest[c.len_utf8()..];
        match c {
            '\u{1}' | '\u{2}' | '\u{3}' | '\u{4}' | '\u{5}' | '\u{6}' => {
                flush(&mut buf, &mut out, bold, italic, greek, hebrew);
                match c {
                    '\u{1}' => greek = true,
                    '\u{2}' | '\u{6}' => bold = true,
                    '\u{3}' => italic = true,
                    '\u{5}' => hebrew = true,
                    _ => (bold, italic, greek, hebrew) = (false, false, false, false),
                }
            }
            '\r' | '\n' => buf.push(' '),
            c if (c as u32) < 0x20 => {}
            c => buf.push(c),
        }
    }
    flush(&mut buf, &mut out, bold, italic, greek, hebrew);
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    Greek,
    Hebrew,
}

pub struct Lexicon {
    table: Table,
    lang: Language,
    exact: HashMap<String, Vec<usize>>,
    loose: HashMap<String, Vec<usize>>,
    skeleton: HashMap<String, Vec<usize>>,
    by_strongs: HashMap<u32, Vec<usize>>,
}

/// The entry text from the first `\x15` (end of the search key) on, if there is a key.
fn body_of(text: &str) -> &str {
    match text.find('\u{15}') {
        Some(p) if p < 120 => &text[p + 1..],
        _ => text,
    }
}

/// Headword as raw lexicon encoding: the key before `\x15`, else the first styled run.
fn headword(text: &str) -> String {
    let key = text.split(['\u{15}', '\u{4}']).next().unwrap_or("");
    let key = key.trim_start_matches(|c: char| (c as u32) < 0x20);
    let key = key.split(['\u{1}', '\u{5}']).next().unwrap_or(key);
    key.trim().split(' ').next().unwrap_or("").to_string()
}

impl Lexicon {
    pub fn open(databases: &Path, id: &str, lang: Language) -> io::Result<Self> {
        let table = Table::open(databases.join(format!("{id}.dbx")))?;
        let (mut exact, mut loose, mut skeleton, mut by_strongs) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        for i in 0..table.len() {
            let Some(text) = table.get(i) else { continue };
            match lang {
                Language::Greek => {
                    let w = lex_greek(&headword(&text));
                    if w.is_empty() {
                        continue;
                    }
                    let (e, l) = greek_keys(&w);
                    exact.entry(e).or_insert_with(Vec::new).push(i);
                    loose.entry(l).or_insert_with(Vec::new).push(i);
                }
                Language::Hebrew => {
                    // The headword is the first Hebrew run (`\x05word\x04`).
                    let run = text.split('\u{5}').nth(1).and_then(|r| r.split('\u{4}').next()).unwrap_or("");
                    let sk = lex_hebrew_consonants(run);
                    if !sk.is_empty() {
                        skeleton.entry(sk).or_insert_with(Vec::new).push(i);
                    }
                    if let Some(p) = text.find("(Strong ") {
                        let n: String = text[p + 8..].chars().take_while(char::is_ascii_digit).collect();
                        if let Ok(n) = n.parse() {
                            by_strongs.entry(n).or_insert_with(Vec::new).push(i);
                        }
                    }
                }
            }
        }
        Ok(Lexicon { table, lang, exact, loose, skeleton, by_strongs })
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn language(&self) -> Language {
        self.lang
    }

    /// Entries for a Greek lemma (Unicode).
    pub fn greek(&self, lemma: &str) -> Vec<usize> {
        for v in lemma_variants(lemma) {
            let (e, l) = greek_keys(&v);
            if let Some(hits) = self.exact.get(&e).or_else(|| self.loose.get(&l)) {
                return hits.clone();
            }
        }
        Vec::new()
    }

    /// Entries for a Hebrew word: by Strong's number where the lexicon records it, else by consonants.
    pub fn hebrew(&self, mc_lemma: &str, strongs: &[u32]) -> Vec<usize> {
        let mut out: Vec<usize> = strongs.iter().filter_map(|n| self.by_strongs.get(n)).flatten().copied().collect();
        if out.is_empty() {
            out = self.skeleton.get(&mc_consonants(mc_lemma)).cloned().unwrap_or_default();
        }
        out.dedup();
        out
    }

    pub fn entry(&self, i: usize) -> Option<Vec<Span>> {
        let text = self.table.get(i)?;
        let mut spans = parse_spans(body_of(&text));
        // Greek lexicons set the headword in bold or plain text without a Greek-run marker.
        let hw = headword(&text);
        if self.lang == Language::Greek && !hw.is_empty() {
            if let Some(first) = spans.first() {
                if !first.greek && !first.hebrew {
                    if let Some(rest) = first.text.trim_start().strip_prefix(hw.as_str()) {
                        let (rest, bold, italic) = (rest.to_string(), first.bold, first.italic);
                        spans[0] = Span { text: lex_greek(&hw), bold, italic, greek: true, ..Span::default() };
                        if !rest.is_empty() {
                            spans.insert(1, Span { text: rest, bold, italic, ..Span::default() });
                        }
                    }
                }
            }
        }
        Some(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_links() {
        let s = parse_spans("\u{2}\u{1}ajgapavw\u{4}, \u{3}to love\u{4}: <<ref:BHS/GNT_Joh 3:16$$John 3:16>>; <<ref:Job.8:12$$Jb 8\u{19}12\u{4}>>");
        assert_eq!(s[0].text, "ἀγαπάω");
        assert!(s[0].greek && s[0].bold);
        assert_eq!(s[2].text, "to love");
        assert!(s[2].italic);
        let links: Vec<_> = s.iter().filter_map(|x| x.link.clone()).collect();
        assert_eq!(links, vec![("Joh".to_string(), 3, 16), ("Job".to_string(), 8, 12)]);
        assert_eq!(s.iter().find(|x| x.link.is_some()).unwrap().text, "John 3:16");
    }

    #[test]
    fn strongs_header_parses() {
        let t = "00025\r\n  25 \\Gajgapavw\\n agapao {ag-ap-ah'-o}\r\n\r\n perhaps from agan (much); v\r\n\r\n 1) of persons\r\n";
        let (n, words, tr, p, body) = strongs_header(t).unwrap();
        assert_eq!((n, words[0].0, words[0].1.as_str(), tr.as_str(), p.as_str()), (25, 'G', "ajgapavw", "agapao", "ag-ap-ah'-o"));
        assert!(body.contains("1) of persons"));
        // a mark `\` followed by the letter n belongs to the word (ou\n = οὖν)
        let t = "03767\r\n  3767 \\Gou\\n\\n oun {oon}\r\n";
        assert_eq!(strongs_header(t).unwrap().1, vec![('G', "ou\\n".to_string())]);
        assert_eq!(lex_greek("ou\\n"), "οὖν");
        // a phrase entry has several tagged words; none of them may leak into the transliteration
        let t = "03801\r\n  3801 \\GoJ\\n \\Gw]n\\n \\Gkai;\\n ho on kai {ho own kahee}\r\n";
        let (_, w, tr, p, _) = strongs_header(t).unwrap();
        assert_eq!((w.len(), tr.as_str(), p.as_str()), (3, "ho on kai", "ho own kahee"));
        // alternate words follow " or "
        let t = "03475\r\n  3475 \\GMwseu'\"\\n Moseus {moce-yoos'} or \\GMwsh'\"\\n Moses {mo-sace'}\r\n";
        let w = strongs_header(t).unwrap().1;
        assert_eq!(w.len(), 2);
        assert_eq!(lex_greek(&w[1].1), "Μωσῆς");
        assert_eq!(lemma_variants("πορεύομαι"), vec!["πορεύομαι".to_string(), "πορεύω".to_string()]);
    }

    fn db() -> Option<std::path::PathBuf> {
        crate::install_dir().map(|d| d.join("databases"))
    }

    #[test]
    fn real_strongs_and_lexicons() {
        let Some(db) = db() else { return };
        let s = Strongs::open(&db).unwrap();
        let g = s.entry(false, 25).unwrap();
        assert_eq!((g.word.as_str(), g.translit.as_str()), ("ἀγαπάω", "agapao"));
        assert_eq!(s.greek_lemma("ἀγαπάω"), vec![25]);
        assert_eq!(s.greek_lemma("λόγος"), vec![3056]);
        assert_eq!(s.entry(true, 1).unwrap().translit, "'ab");
        assert!(s.hebrew_lemma("R\")$IYT").contains(&7225));
        assert!(s.hebrew_lemma("BR)").contains(&1254));

        let th = Lexicon::open(&db, "thdict", Language::Greek).unwrap();
        let hits = th.greek("ἀγαπάω");
        assert!(!hits.is_empty());
        let text: String = th.entry(hits[0]).unwrap().iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("to love"), "{text}");
        assert!(text.starts_with("ἀγαπάω"), "headword is decoded: {text}");

        let bdb = Lexicon::open(&db, "fbdblex", Language::Hebrew).unwrap();
        let hits = bdb.hebrew("R\")$IYT", &[7225]);
        assert!(!hits.is_empty());
        let text: String = bdb.entry(hits[0]).unwrap().iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("beginning") || text.contains("first"), "{text}");
    }
}
