//! Flashcard decks from BibleWorks' vocabulary files (`init/*.vrt`) and a small review schedule.
//!
//! A `.vrt` is tab-separated text: a few `Title:` / `Language:` / `Comment:` header lines, then one
//! card per line: `key, display, language, lesson, count, parts of speech, gloss, audio`. Fields may
//! be wrapped in double quotes (`""` is a quote). The word itself is in a legacy font encoding: Greek
//! with accents typed after the letter, Hebrew in visual order with the vowels after their consonant.
//! The `.vrc` next to each deck is a binary index and is not needed.

use crate::lexenc::finish_hebrew;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::Path};
use unicode_normalization::UnicodeNormalization;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Card {
    /// Unique within the deck; progress is saved under it.
    pub id: String,
    pub front: String,
    pub back: String,
    /// Lesson or chapter of the course the deck follows; `None` when the deck has no lessons.
    pub lesson: Option<u32>,
    /// Occurrences in the text, as counted by the deck.
    pub count: u32,
    /// The deck's part-of-speech tally, e.g. `n 5` or `vq 746 vn 45`.
    pub parts: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Deck {
    pub id: String,
    pub title: String,
    pub hebrew: bool,
    pub comment: String,
    pub cards: Vec<Card>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DeckInfo {
    pub id: String,
    pub title: String,
    pub hebrew: bool,
    pub cards: usize,
    /// Highest lesson number, 0 when the deck has no lessons.
    pub lessons: u32,
}

// ---------------------------------------------------------------- fonts

fn greek_letter(c: char) -> Option<char> {
    Some(match c {
        'a' => 'α', 'b' => 'β', 'g' => 'γ', 'd' => 'δ', 'e' => 'ε', 'z' => 'ζ', 'h' => 'η', 'q' => 'θ',
        'i' => 'ι', 'k' => 'κ', 'l' => 'λ', 'm' => 'μ', 'n' => 'ν', 'x' => 'ξ', 'o' => 'ο', 'p' => 'π',
        'r' => 'ρ', 's' => 'σ', 'j' => 'ς', 't' => 'τ', 'u' => 'υ', 'f' => 'φ', 'c' => 'χ', 'y' => 'ψ', 'w' => 'ω',
        _ => return None,
    })
}

fn greek_marks(c: char) -> Option<&'static str> {
    Some(match c {
        ',' => "\u{301}",
        'v' => "\u{313}",
        '`' => "\u{314}",
        '/' => "\u{342}",
        ';' => "\u{313}\u{301}",
        '[' => "\u{314}\u{301}",
        '?' => "\u{308}",
        '<' => "\u{308}\u{301}",
        '|' => "\u{345}",
        '=' => "\u{313}\u{342}",
        '-' => "\u{314}\u{342}",
        '.' => "\u{300}",
        _ => return None,
    })
}

/// Greek in the vocabulary-file font -> Unicode. `-` is not a mark inside a word's ending (`&ou`);
/// a leading `&` is written as a hyphen.
fn vrt_greek(s: &str, capital: bool) -> String {
    let mut out = String::new();
    let mut first = true;
    for c in s.chars() {
        if let Some(g) = greek_letter(c) {
            if first && capital {
                out.extend(g.to_uppercase());
            } else {
                out.push(g);
            }
            first = false;
        } else if let (Some(m), false) = (greek_marks(c), out.is_empty()) {
            out.push_str(m);
        } else if c == '&' {
            out.push('-');
        } else if c == ' ' {
            out.push(' ');
        }
    }
    out.nfc().collect()
}

/// The word of a Greek card: the lemma from the key (which carries all accents) with the capital
/// the display field shows, then the endings and article that follow it, separated by commas.
fn greek_front(key: &str, display: &str) -> String {
    let mut parts = display.split('(').map(str::trim);
    let head = parts.next().unwrap_or("");
    let capital = head.chars().any(|c| c.is_ascii_uppercase() && c != 'V');
    let word = if key.is_empty() { head } else { key };
    let mut out = vrt_greek(word, capital);
    for p in parts.filter(|p| !p.is_empty()) {
        out.push_str(", ");
        out.push_str(&vrt_greek(p, false));
    }
    out
}

fn hebrew_vowel(c: char) -> Option<char> {
    Some(match c {
        '\'' | '"' => '\u{5B8}',
        ';' | ':' => '\u{5B7}',
        '.' | '>' => '\u{5B0}',
        'i' | 'I' => '\u{5B4}',
        ',' | '<' => '\u{5B6}',
        'e' | 'E' => '\u{5B5}',
        ']' => '\u{5B2}',
        '/' => '\u{5B1}',
        '\\' => '\u{5B3}',
        'o' | 'O' | '{' => '\u{5B9}',
        'u' | 'U' => '\u{5BB}',
        _ => return None,
    })
}

/// (letter, shin/sin dot) for a consonant character; capitals add a dagesh in the caller.
fn hebrew_letter(c: char) -> Option<(char, Option<char>)> {
    let l = c.to_ascii_lowercase();
    Some(match l {
        'a' => ('א', None), 'b' => ('ב', None), 'g' => ('ג', None), 'd' => ('ד', None), 'h' => ('ה', None),
        'w' => ('ו', None), 'z' => ('ז', None), 'x' => ('ח', None), 'j' => ('ט', None), 'y' => ('י', None),
        'k' => ('כ', None), 'l' => ('ל', None), 'm' => ('מ', None), 'n' => ('נ', None), 's' => ('ס', None),
        '[' => ('ע', None), 'p' => ('פ', None), 'c' => ('צ', None), 'q' => ('ק', None), 'r' => ('ר', None),
        'v' => ('ש', Some('\u{5C1}')), 'f' => ('ש', Some('\u{5C2}')), 't' => ('ת', None),
        '~' => ('ם', None), '!' => ('ן', None), '#' => ('ץ', None), '%' => ('ך', None), '@' => ('ף', None),
        _ => return None,
    })
}

/// Hebrew in the vocabulary-file font -> pointed Unicode in reading order.
fn vrt_hebrew(s: &str) -> String {
    let mut v: Vec<(char, String)> = Vec::new();
    for c in s.chars() {
        if c == ' ' {
            v.push((' ', String::new()));
        } else if c == 'A' {
            v.push(('ו', "\u{5B9}".into())); // vav with holam
        } else if c == 'J' {
            v.push(('ה', "\u{5BC}".into())); // he with mappiq
        } else if let Some((l, dot)) = hebrew_letter(c).filter(|_| !matches!(c, 'i' | 'e' | 'o' | 'u' | 'I' | 'E' | 'O' | 'U')) {
            let mut pts = String::new();
            if c.is_ascii_uppercase() {
                pts.push('\u{5BC}');
            }
            pts.extend(dot);
            v.push((l, pts));
        } else if let (Some(m), Some(last)) = (hebrew_vowel(c), v.last_mut().filter(|l| l.0 != ' ')) {
            last.1.push(m);
        }
    }
    finish_hebrew(v)
}

// ---------------------------------------------------------------- files

/// Split a tab-separated line whose fields may be quoted.
fn fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if cur.is_empty() => quoted = true,
            ('\t', false) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Windows-1252 text (only the Latin-1 range matters here).
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn parse_deck(id: &str, text: &str) -> Deck {
    let (mut title, mut comment, mut hebrew) = (id.to_string(), String::new(), false);
    let mut cards: Vec<Card> = Vec::new();
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for line in text.lines() {
        let f = fields(line);
        let lang = f.get(2).map(String::as_str).unwrap_or("");
        if f.len() >= 8 && matches!(lang, "G" | "H" | "A") {
            hebrew = lang != "G";
            let front = if hebrew { vrt_hebrew(&f[1]) } else { greek_front(&f[0], &f[1]) };
            let mut gloss: Vec<&str> = f[6].split("<p>").map(str::trim).collect();
            if gloss.len() > 1 && !gloss[0].contains(' ') {
                gloss.remove(0); // verb drills start with the parsing code
            }
            let n = seen.entry(f[0].clone()).or_default();
            *n += 1;
            let id = if *n == 1 { f[0].clone() } else { format!("{}#{n}", f[0]) };
            let lesson = f[3].trim().parse::<u32>().ok().filter(|&n| n != 999);
            cards.push(Card { id, front, back: gloss.join("\n"), lesson, count: f[4].trim().parse().unwrap_or(0), parts: f[5].trim().to_string() });
        } else if let Some(t) = f[0].trim().strip_prefix("Title:") {
            title = t.trim().to_string();
        } else if let Some(c) = f[0].trim().strip_prefix("Comment:") {
            comment = c.trim().to_string();
        }
    }
    Deck { id: id.to_string(), title, hebrew, comment, cards }
}

fn deck_files(install: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(rd) = fs::read_dir(install.join("init")) else { return Vec::new() };
    let mut v: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vrt")))
        .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
        .collect();
    v.sort();
    v
}

pub fn open_deck(install: &Path, id: &str) -> io::Result<Deck> {
    let (_, path) = deck_files(install).into_iter().find(|(i, _)| i == id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such deck"))?;
    Ok(parse_deck(id, &latin1(&fs::read(path)?)))
}

pub fn list_decks(install: &Path) -> Vec<DeckInfo> {
    deck_files(install)
        .into_iter()
        .filter_map(|(id, path)| {
            let d = parse_deck(&id, &latin1(&fs::read(path).ok()?));
            (!d.cards.is_empty()).then(|| DeckInfo {
                lessons: d.cards.iter().filter_map(|c| c.lesson).max().unwrap_or(0),
                cards: d.cards.len(),
                title: d.title,
                hebrew: d.hebrew,
                id,
            })
        })
        .collect()
}

// ---------------------------------------------------------------- review

/// Days until a card in each box comes round again; a miss puts it back in box 0.
// ponytail: fixed Leitner boxes, swap in per-card ease factors if this proves too blunt.
const INTERVALS: [i64; 6] = [0, 1, 3, 7, 21, 60];
/// Boxes from here up count as learned.
const LEARNED_BOX: u8 = 3;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardState {
    pub box_: u8,
    /// Day number (days since 1970-01-01) the card is next due.
    pub due: i64,
    pub seen: u32,
}

/// Saved progress: deck id -> card id -> state.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Progress {
    pub decks: BTreeMap<String, BTreeMap<String, CardState>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Filter {
    pub min_lesson: Option<u32>,
    pub max_lesson: Option<u32>,
    pub min_count: Option<u32>,
}

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub total: usize,
    pub new: usize,
    pub due: usize,
    pub learned: usize,
}

impl Filter {
    fn keeps(&self, c: &Card) -> bool {
        let l = c.lesson.unwrap_or(0);
        self.min_lesson.map_or(true, |m| l >= m) && self.max_lesson.map_or(true, |m| l <= m) && self.min_count.map_or(true, |m| c.count >= m)
    }
}

impl Progress {
    /// Record an answer: right moves the card up a box, wrong sends it back to box 0.
    pub fn grade(&mut self, deck: &str, card: &str, right: bool, today: i64) {
        let s = self.decks.entry(deck.to_string()).or_default().entry(card.to_string()).or_default();
        s.box_ = if right { (s.box_ + 1).min(INTERVALS.len() as u8 - 1) } else { 0 };
        s.due = today + INTERVALS[s.box_ as usize];
        s.seen += 1;
    }

    /// Mark a card learned (or not) by hand.
    pub fn set_learned(&mut self, deck: &str, card: &str, learned: bool, today: i64) {
        let s = self.decks.entry(deck.to_string()).or_default().entry(card.to_string()).or_default();
        s.box_ = if learned { INTERVALS.len() as u8 - 1 } else { 0 };
        s.due = today + INTERVALS[s.box_ as usize];
    }

    /// Forget all progress for a deck.
    pub fn reset(&mut self, deck: &str) {
        self.decks.remove(deck);
    }

    pub fn stats(&self, deck: &Deck, filter: &Filter, today: i64) -> Stats {
        let saved = self.decks.get(&deck.id);
        let mut st = Stats::default();
        for c in deck.cards.iter().filter(|c| filter.keeps(c)) {
            st.total += 1;
            match saved.and_then(|m| m.get(&c.id)) {
                None => st.new += 1,
                Some(s) => {
                    st.learned += (s.box_ >= LEARNED_BOX) as usize;
                    st.due += (s.due <= today) as usize;
                }
            }
        }
        st
    }

    /// Indexes into `deck.cards` to study now: cards that are due (most overdue first), then new
    /// ones in deck order. At most `limit`.
    pub fn queue(&self, deck: &Deck, filter: &Filter, today: i64, limit: usize) -> Vec<usize> {
        let saved = self.decks.get(&deck.id);
        let mut due: Vec<(i64, usize)> = Vec::new();
        let mut new: Vec<usize> = Vec::new();
        for (i, c) in deck.cards.iter().enumerate().filter(|(_, c)| filter.keeps(c)) {
            match saved.and_then(|m| m.get(&c.id)) {
                None => new.push(i),
                Some(s) if s.due <= today => due.push((s.due, i)),
                _ => {}
            }
        }
        due.sort();
        due.into_iter().map(|d| d.1).chain(new).take(limit).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(s: &str) -> String {
        s.nfc().collect()
    }

    #[test]
    fn greek_font() {
        assert_eq!(greek_front("lu,w", "lu,w"), nfc("λύω"));
        assert_eq!(greek_front("avbarh,j", "avbarh,j( &e,j"), nfc("ἀβαρής, -ές"));
        assert_eq!(greek_front("avarw,n", "VAarw,n( o`"), nfc("Ἀαρών, ὁ"));
        assert_eq!(greek_front("a[bel", "{Abel( o`"), nfc("Ἅβελ, ὁ"));
        assert_eq!(greek_front("avqh/nai", "VAqh/nai( &w/n( h`"), nfc("ἀθῆναι, -ῶν, ἡ").replace('ἀ', "Ἀ"));
    }

    #[test]
    fn hebrew_font() {
        assert_eq!(vrt_hebrew("ba'"), nfc("אָב"));
        assert_eq!(vrt_hebrew("db;['"), nfc("עָבַד"));
        assert_eq!(vrt_hebrew("dB;k.YIw:"), nfc("וַיִּכְבַּד"));
        assert_eq!(vrt_hebrew("~D'"), nfc("דָּם"));
        assert_eq!(vrt_hebrew("tAxa'"), nfc("אָחוֹת"));
    }

    #[test]
    fn quoted_fields() {
        assert_eq!(fields("\"a\"\t\"b\"\"c\"\td"), vec!["a", "b\"c", "d"]);
    }

    #[test]
    fn schedule() {
        let deck = Deck {
            id: "d".into(),
            title: "t".into(),
            hebrew: false,
            comment: String::new(),
            cards: (0..4).map(|i| Card { id: format!("c{i}"), front: String::new(), back: String::new(), lesson: Some(i), count: 10 * i, parts: String::new() }).collect(),
        };
        let mut p = Progress::default();
        let f = Filter::default();
        assert_eq!(p.queue(&deck, &f, 100, 10), vec![0, 1, 2, 3]);
        p.grade("d", "c1", true, 100); // box 1: back tomorrow
        p.grade("d", "c2", false, 100); // box 0: due now
        assert_eq!(p.queue(&deck, &f, 100, 10), vec![2, 0, 3]);
        assert_eq!(p.queue(&deck, &f, 101, 10), vec![2, 1, 0, 3]);
        assert_eq!(p.queue(&deck, &Filter { min_count: Some(20), ..Filter::default() }, 100, 10), vec![2, 3]);
        for _ in 0..3 {
            p.grade("d", "c1", true, 100);
        }
        assert_eq!(p.stats(&deck, &f, 100), Stats { total: 4, new: 2, due: 1, learned: 1 });
        p.reset("d");
        assert_eq!(p.stats(&deck, &f, 100).new, 4);
    }

    /// Every shipped deck decodes to clean Greek or Hebrew only.
    #[test]
    fn real_decks_decode() {
        let Some(dir) = crate::install_dir() else { return };
        let decks = list_decks(&dir);
        assert!(decks.len() >= 8, "{decks:?}");
        for info in decks {
            let d = open_deck(&dir, &info.id).unwrap();
            for c in &d.cards {
                let bad = c.front.chars().find(|&ch| {
                    let ok = if d.hebrew {
                        matches!(ch, '\u{5B0}'..='\u{5C2}' | '\u{5D0}'..='\u{5EA}' | ' ' | ',' | '-')
                    } else {
                        matches!(ch, '\u{370}'..='\u{3FF}' | '\u{1F00}'..='\u{1FFF}' | '\u{300}'..='\u{345}' | ' ' | ',' | '-')
                    };
                    !ok
                });
                assert!(bad.is_none(), "{}: {:?} in {:?}", d.id, bad, c.front);
                assert!(!c.front.trim().is_empty() && !c.back.is_empty(), "{}: empty card {:?}", d.id, c);
            }
        }
    }
}
