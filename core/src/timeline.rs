//! BibleWorks Timeline data (read-only, from the user's own install).
//!
//! Everything lives in `<install>\timeline\`, as plain Windows-1252 text with CRLF line ends:
//!   * `*.TL` - one timeline. Line 1: `<unit> <first> <last> <label-every> <label-space> <f1> <f2> <f3>`
//!     (unit `d`/`w`/`y`; the three flags are small ints whose meaning, probably axis colours or
//!     grid, is not decoded). Line 2: era-file count N. Then N lines `<era-file-stem> <x> <y> <h>`:
//!     the era's caption width (-1 = automatic), its row (pixels from the top) and a height.
//!     Optional trailer: `##ADDITIONAL FLAGS`, then `##customUnit <text>` for a user-defined unit.
//!   * `*.ERA` - one era plus all its events, two lines per record: a caption line, then numbers.
//!     Record 0 is the era: `<from> <to> 16 3 4 <event-count>`. Records 1..=count are events:
//!     `<from> <to> <y> <width> <style> <extent-colour> <caption-colour>`. `y` is pixels relative
//!     to the era's row (can be negative), `width` the caption box width (-1 = automatic),
//!     `style` 8 = thin line, 16 = era bar.
//!   * The caption is `text|link;link;...`. A link is `Gen 1:1` (a verse or range), `Eas term`
//!     (Easton's dictionary), `Nav term` (Nave's topics), `.words` (a BibleWorks search), a URL, or
//!     `TRE ref` (Treasury of Scripture Knowledge). A caption starting `(?)` or `(-?)` marks an
//!     uncertain date. A second `|` also separates links (one ASSYR event has two).
//!   * `*.erc` - a date index of the same era, followed by a dump of uninitialised memory. Fully
//!     derived from the `.ERA`; ignored here. `CRC.DEF` and `check.exe` are an integrity checker.
//!
//! Dates are BibleWorks' own year numbers: negative = BC, `-586` is 586 BC, positive = AD, and
//! there is no year 0 (a literal 0 occurs in the data and means "about the BC/AD boundary").
//! Use [`astronomical`] when you need a gap-free number line. For week/day timelines the numbers
//! are simply counts in that unit; no such timeline ships.
//!
//! The colour and style fields are palette indices, not RGB; see [`palette_rgb`].

use crate::canon::BOOKS;
use serde::Serialize;
use std::{collections::HashMap, fs, io, path::{Path, PathBuf}};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Years,
    Weeks,
    Days,
}

/// The time axis of a timeline.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Axis {
    pub unit: Unit,
    pub first: i32,
    pub last: i32,
    /// Units between axis labels.
    pub label_every: i32,
    /// Pixels between labels.
    pub label_space: i32,
    /// Three further small ints from the header line, meaning not decoded.
    pub flags: [i32; 3],
    /// Name of a user-defined unit (`##customUnit`); no shipped timeline has one.
    pub custom_unit: Option<String>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Thin line under the caption (raw 8).
    Line,
    /// Thick bar behind the caption (raw 16).
    EraBar,
    Other(i16),
}

impl Style {
    fn from_raw(n: i16) -> Style {
        match n {
            8 => Style::Line,
            16 => Style::EraBar,
            n => Style::Other(n),
        }
    }
}

/// What a caption's context-menu entry points at.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub enum Link {
    Bible(BibleRef),
    /// Treasury of Scripture Knowledge entry (`TRE Gen 1:1`).
    Tsk(BibleRef),
    /// `dict` is the module code as written, `Eas` or `Nav`.
    Dictionary { dict: String, term: String },
    /// A BibleWorks word search (`.psalm david`), without the dot.
    Search(String),
    Url(String),
    /// Anything unrecognised, verbatim.
    Other(String),
}

/// A verse, verse range or chapter reference. `book` is the BibleWorks 3-letter code (`Gen`, `1Ki`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct BibleRef {
    pub book: &'static str,
    pub chapter: u16,
    /// `None` for a whole chapter.
    pub verse: Option<u16>,
    /// End of a range (`Jer 1:1-11` -> verse 11; `Ezr 3-4` -> chapter 4).
    pub end_chapter: Option<u16>,
    pub end_verse: Option<u16>,
}

/// A long period (a bar across the timeline) and the file it came from.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Era {
    pub name: String,
    pub uncertain: bool,
    pub links: Vec<Link>,
    /// File stem as listed in the `.TL` (`CREATION`, `modern`).
    pub file: String,
    pub start: i32,
    pub end: i32,
    /// Row of the era inside the timeline, pixels from the top. Event `y` is relative to it.
    pub y: i32,
    pub caption_width: Option<i32>,
    pub height: i32,
    pub style: Style,
    pub extent_color: u8,
    pub caption_color: u8,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Event {
    /// Index into `Timeline::eras`.
    pub era: usize,
    pub name: String,
    pub uncertain: bool,
    pub links: Vec<Link>,
    pub start: i32,
    /// Equal to `start` for a single date.
    pub end: i32,
    /// Pixels relative to the owning era's `y`.
    pub y: i32,
    pub width: Option<i32>,
    pub style: Style,
    pub extent_color: u8,
    pub caption_color: u8,
    /// The file had `start > end` (two events do); the dates here are a guess at the intent.
    pub repaired: bool,
}

impl Event {
    pub fn is_span(&self) -> bool {
        self.end != self.start
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Timeline {
    /// File stem of the `.TL` (`BIBLE`, `NT_DET`, `WORLD`).
    pub name: String,
    pub axis: Axis,
    pub eras: Vec<Era>,
    pub events: Vec<Event>,
}

/// BibleWorks year -> astronomical year (1 BC = 0, 2 BC = -1). Positive years are unchanged.
pub fn astronomical(date: i32) -> i32 {
    if date < 0 { date + 1 } else { date }
}

/// RGB for a colour index.
// ponytail: the files hold indices into a palette the exe builds in code, which I did not locate.
// This is the classic 16-colour EGA order; the choice fits the data (links blue 9, books green 10,
// era bars cyan 3) but is a guess. Ceiling: wrong hues for some indices; the index is kept raw.
pub fn palette_rgb(index: u8) -> [u8; 3] {
    const P: [[u8; 3]; 16] = [
        [0, 0, 0], [0, 0, 170], [0, 170, 0], [0, 170, 170],
        [170, 0, 0], [170, 0, 170], [170, 85, 0], [170, 170, 170],
        [85, 85, 85], [85, 85, 255], [85, 255, 85], [85, 255, 255],
        [255, 85, 85], [255, 85, 255], [255, 255, 85], [255, 255, 255],
    ];
    P[(index & 15) as usize]
}

fn bad(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.into())
}

/// Windows-1252 bytes to text. Never fails; the 5 undefined bytes become U+FFFD.
fn cp1252(b: &[u8]) -> String {
    const HI: [char; 32] = [
        '€', '\u{FFFD}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{FFFD}', 'Ž', '\u{FFFD}',
        '\u{FFFD}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{FFFD}', 'ž', 'Ÿ',
    ];
    b.iter().map(|&c| if (0x80..0xA0).contains(&c) { HI[(c - 0x80) as usize] } else { c as char }).collect()
}

fn read_text(p: &Path) -> io::Result<String> {
    Ok(cp1252(&fs::read(p)?))
}

fn ints<const N: usize>(line: &str) -> Option<[i32; N]> {
    let mut it = line.split_whitespace();
    let mut out = [0; N];
    for o in &mut out {
        *o = it.next()?.parse().ok()?;
    }
    Some(out)
}

fn small(n: i32) -> Option<i16> {
    i16::try_from(n).ok()
}

// ---------------------------------------------------------------- links

fn parse_u16(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `1`, `1:2`, `1:2-5`, `1:2-3:4`, `1-3`: what follows the book code.
fn parse_cv(book: &'static str, s: &str) -> Option<BibleRef> {
    let s = s.trim();
    let (head, tail) = match s.split_once('-') {
        Some((h, t)) => (h.trim(), Some(t.trim())),
        None => (s, None),
    };
    let (chapter, verse) = match head.split_once(':') {
        Some((c, v)) => (parse_u16(c.trim())?, Some(parse_u16(v.trim())?)),
        None => (parse_u16(head)?, None),
    };
    let (mut end_chapter, mut end_verse) = (None, None);
    if let Some(t) = tail {
        match (t.split_once(':'), verse) {
            (Some((c, v)), Some(_)) => (end_chapter, end_verse) = (Some(parse_u16(c.trim())?), Some(parse_u16(v.trim())?)),
            (None, Some(_)) => end_verse = Some(parse_u16(t)?),
            (None, None) => end_chapter = Some(parse_u16(t)?),
            _ => return None,
        }
    }
    Some(BibleRef { book, chapter, verse, end_chapter, end_verse })
}

/// `Gen 1:1`, `jer 52:31`, `Eze29:17`: three letters (case-insensitive), optional space, numbers.
fn parse_ref(s: &str) -> Option<BibleRef> {
    let s = s.trim();
    let mut it = s.char_indices().skip(3);
    let cut = it.next().map_or(s.len(), |(i, _)| i);
    let book = BOOKS.iter().find(|b| b.code.eq_ignore_ascii_case(&s[..cut]))?.code;
    parse_cv(book, &s[cut..])
}

fn parse_link(item: &str) -> Option<Link> {
    let item = item.trim();
    if item.is_empty() {
        return None;
    }
    if let Some(q) = item.strip_prefix('.') {
        return Some(Link::Search(q.trim().to_string()));
    }
    let lower = item.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return Some(Link::Url(item.to_string()));
    }
    let (tag, rest) = item.split_once(char::is_whitespace).unwrap_or((item, ""));
    if tag.eq_ignore_ascii_case("eas") || tag.eq_ignore_ascii_case("nav") {
        return Some(Link::Dictionary { dict: tag.to_string(), term: rest.trim().to_string() });
    }
    if tag.eq_ignore_ascii_case("tre") {
        if let Some(r) = parse_ref(rest) {
            return Some(Link::Tsk(r));
        }
    }
    Some(parse_ref(item).map_or_else(|| Link::Other(item.to_string()), Link::Bible))
}

/// Caption line -> (name, uncertain, links).
fn parse_caption(line: &str) -> (String, bool, Vec<Link>) {
    let (text, links) = line.split_once('|').unwrap_or((line, ""));
    let links = links.replace('|', ";").split(';').filter_map(parse_link).collect();
    let text = text.trim();
    let (text, uncertain) = match text.strip_prefix("(?)").or_else(|| text.strip_prefix("(-?)")) {
        Some(t) => (t.trim_start(), true),
        None => (text, false),
    };
    (text.to_string(), uncertain, links)
}

// ---------------------------------------------------------------- files

fn lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// An era file: the era (placed by the caller) and its events, owned by era index `era_index`.
fn parse_era(text: &str, file: &str, tl: [i32; 3], era_index: usize) -> io::Result<(Era, Vec<Event>)> {
    let l = lines(text);
    let ctx = |m: &str| bad(format!("{file}: {m}"));
    if l.len() < 2 {
        return Err(ctx("too short"));
    }
    let [start, end, style, ec, cc, n] = ints::<6>(l[1]).ok_or_else(|| ctx("bad era line"))?;
    let n = usize::try_from(n).map_err(|_| ctx("negative event count"))?;
    if l.len() < 2 + 2 * n {
        return Err(ctx("fewer events than declared"));
    }
    let (name, uncertain, links) = parse_caption(l[0]);
    let era = Era {
        name,
        uncertain,
        links,
        file: file.to_string(),
        start,
        end,
        y: tl[1],
        caption_width: (tl[0] >= 0).then_some(tl[0]),
        height: tl[2],
        style: Style::from_raw(small(style).ok_or_else(|| ctx("bad style"))?),
        extent_color: u8::try_from(ec).map_err(|_| ctx("bad colour"))?,
        caption_color: u8::try_from(cc).map_err(|_| ctx("bad colour"))?,
    };
    let mut events = Vec::with_capacity(n);
    for k in 0..n {
        let (cap, nums) = (l[2 + 2 * k], l[3 + 2 * k]);
        let [from, to, y, w, st, ec, cc] = ints::<7>(nums).ok_or_else(|| ctx(&format!("bad event line {}", 4 + 2 * k)))?;
        let (name, uncertain, links) = parse_caption(cap);
        let (mut start, mut end, mut repaired) = (from, to, false);
        if start > end {
            // ponytail: two typos in the shipped data (a lost minus sign, a zero end date).
            // Rule: opposite signs -> negate start, otherwise end = start. Ceiling: a guess.
            repaired = true;
            if start > 0 && end < 0 { start = -start } else { end = start }
        }
        events.push(Event {
            era: era_index,
            name,
            uncertain,
            links,
            start,
            end,
            y,
            width: (w >= 0).then_some(w),
            style: Style::from_raw(small(st).ok_or_else(|| ctx("bad style"))?),
            extent_color: u8::try_from(ec).map_err(|_| ctx("bad colour"))?,
            caption_color: u8::try_from(cc).map_err(|_| ctx("bad colour"))?,
            repaired,
        });
    }
    Ok((era, events))
}

fn parse_tl(text: &str, name: &str, files: &HashMap<String, PathBuf>) -> io::Result<Timeline> {
    let l = lines(text);
    let ctx = |m: &str| bad(format!("{name}.TL: {m}"));
    let head = l.first().ok_or_else(|| ctx("empty"))?;
    let (u, rest) = head.trim().split_at_checked(1).ok_or_else(|| ctx("no unit"))?;
    let unit = match u {
        "d" | "D" => Unit::Days,
        "w" | "W" => Unit::Weeks,
        "y" | "Y" => Unit::Years,
        _ => return Err(ctx("unknown unit")),
    };
    let [first, last, label_every, label_space, f1, f2, f3] = ints::<7>(rest).ok_or_else(|| ctx("bad axis line"))?;
    let n: usize = l.get(1).and_then(|s| s.trim().parse().ok()).ok_or_else(|| ctx("bad era count"))?;
    if l.len() < 2 + n {
        return Err(ctx("fewer era entries than declared"));
    }
    let custom_unit = l[2 + n..].iter().find_map(|s| s.strip_prefix("##customUnit")).map(|s| s.trim().to_string());
    let (mut eras, mut events) = (Vec::new(), Vec::new());
    for e in &l[2..2 + n] {
        let t: Vec<&str> = e.split_whitespace().collect();
        if t.len() < 4 {
            return Err(ctx("bad era entry"));
        }
        let stem = t[..t.len() - 3].join(" ");
        let nums = ints::<3>(&t[t.len() - 3..].join(" ")).ok_or_else(|| ctx("bad era entry"))?;
        let path = files.get(&format!("{}.era", stem.to_lowercase())).ok_or_else(|| ctx(&format!("era file {stem}.ERA missing")))?;
        let (era, ev) = parse_era(&read_text(path)?, &stem, nums, eras.len())?;
        eras.push(era);
        events.extend(ev);
    }
    let axis = Axis { unit, first, last, label_every, label_space, flags: [f1, f2, f3], custom_unit };
    Ok(Timeline { name: name.to_string(), axis, eras, events })
}

/// Lower-cased file name -> path for everything in `dir` (installs differ in case: `Church.erc`).
fn dir_files(dir: &Path) -> io::Result<HashMap<String, PathBuf>> {
    let mut m = HashMap::new();
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
            m.insert(n.to_lowercase(), p);
        }
    }
    Ok(m)
}

/// Every `.TL` in `<install>\timeline`, each with its parse result (sorted by name).
pub fn open_all_checked(install: &Path) -> Vec<(PathBuf, io::Result<Timeline>)> {
    let Ok(files) = dir_files(&install.join("timeline")) else { return Vec::new() };
    let mut tls: Vec<&PathBuf> = files.iter().filter(|(k, _)| k.ends_with(".tl")).map(|(_, v)| v).collect();
    tls.sort();
    tls.into_iter()
        .map(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let t = read_text(p).and_then(|t| parse_tl(&t, &stem, &files));
            (p.clone(), t)
        })
        .collect()
}

/// All timelines in the install that parse, sorted by file name.
pub fn open_all(install: &Path) -> Vec<Timeline> {
    open_all_checked(install).into_iter().filter_map(|(_, t)| t.ok()).collect()
}

/// One era file on its own (also for ones no `.TL` lists); the era's row fields are left at 0/-1.
pub fn open_era(path: &Path) -> io::Result<(Era, Vec<Event>)> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    parse_era(&read_text(path)?, stem, [-1, 0, 0], 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> Option<PathBuf> {
        crate::install_dir().map(|d| d.join("timeline")).filter(|d| d.is_dir())
    }

    #[test]
    fn refs_and_links() {
        let r = parse_ref("Jer 1:1-11").unwrap();
        assert_eq!((r.book, r.chapter, r.verse, r.end_verse), ("Jer", 1, Some(1), Some(11)));
        let r = parse_ref("Eze29:17").unwrap();
        assert_eq!((r.book, r.chapter, r.verse), ("Eze", 29, Some(17)));
        assert_eq!(parse_ref("2ki 14:25").unwrap().book, "2Ki");
        assert_eq!(parse_ref("Ezr 7").unwrap().verse, None);
        assert_eq!(parse_ref("Ezr 3-4").unwrap().end_chapter, Some(4));
        assert!(parse_ref("Zzz 1:1").is_none());
        assert!(parse_ref("é").is_none());
        assert_eq!(parse_link(".psalm david"), Some(Link::Search("psalm david".into())));
        assert_eq!(parse_link("Eas Jesus"), Some(Link::Dictionary { dict: "Eas".into(), term: "Jesus".into() }));
        assert!(matches!(parse_link("Nonsense words"), Some(Link::Other(_))));
        let (n, u, l) = parse_caption("(?) Nahum|Nav Nahum;Nah 3:8|.Ninevah");
        assert_eq!((n.as_str(), u, l.len()), ("Nahum", true, 3));
        assert_eq!(cp1252(b"a\x93b\xe9"), "a\u{201C}bé");
    }

    #[test]
    fn real_timelines() {
        let Some(d) = dir() else { return };
        let all = open_all(d.parent().unwrap());
        let names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["BIBLE", "NT_DET", "WORLD"]);
        let count = |n: &str| all.iter().find(|t| t.name == n).unwrap();
        let (bible, nt, world) = (count("BIBLE"), count("NT_DET"), count("WORLD"));
        assert_eq!((bible.eras.len(), bible.events.len()), (1, 38));
        assert_eq!((nt.eras.len(), nt.events.len()), (2, 89));
        assert_eq!((world.eras.len(), world.events.len()), (14, 780));
        assert_eq!(world.axis.unit, Unit::Years);
        assert_eq!((world.axis.first, world.axis.last), (-3950, 2000));
        assert_eq!((bible.axis.first, bible.axis.last), (0, 2200));
        assert_eq!((nt.axis.first, nt.axis.last), (-70, 180));
        assert_eq!(world.eras[0].name, "The Creation to the Flood");
        assert_eq!(world.eras[0].links[0], Link::Bible(parse_ref("Gen 1:1").unwrap()));
        assert_eq!(world.eras[13].file, "Church");
    }

    #[test]
    fn known_events() {
        let Some(d) = dir() else { return };
        let all = open_all(d.parent().unwrap());
        let world = all.iter().find(|t| t.name == "WORLD").unwrap();
        let find = |s: &str| world.events.iter().find(|e| e.name.starts_with(s)).unwrap_or_else(|| panic!("{s}"));
        let fall = find("Fall of Jerusalem");
        assert_eq!((fall.start, fall.end, fall.is_span()), (-586, -586, false));
        assert_eq!(world.eras[fall.era].file, "BABYLON");
        let pent = find("Pentecost");
        assert_eq!((pent.start, pent.end), (30, 30));
        assert_eq!(world.eras[pent.era].file, "NT");
        let exodus = world.events.iter().find(|e| e.name == "Exodus" && e.start == -1480).unwrap();
        assert_eq!(exodus.end, -1446);
        assert!(exodus.links.contains(&Link::Dictionary { dict: "Nav".into(), term: "Exodus".into() }));
        let creation = find("The Creation");
        assert_eq!(creation.start, -3901);
        assert!(creation.links.iter().any(|l| matches!(l, Link::Bible(r) if r.book == "Gen" && r.chapter == 1)));
        let saul = find("Conversion & Ministry of Saul");
        assert_eq!((saul.start, saul.end, saul.style), (32, 67, Style::EraBar));
        let uncertain = world.events.iter().find(|e| e.name == "Jesus Christ").unwrap();
        assert!(uncertain.uncertain);
        assert_eq!(astronomical(-586), -585);
    }

    #[test]
    fn invariants() {
        let Some(d) = dir() else { return };
        let all = open_all(d.parent().unwrap());
        assert!(!all.is_empty());
        let mut outside = Vec::new();
        for t in &all {
            for e in &t.events {
                assert!(e.era < t.eras.len(), "{}: {}", t.name, e.name);
                assert!(e.start <= e.end, "{}: {}", t.name, e.name);
                if e.start < t.axis.first || e.end > t.axis.last {
                    outside.push(format!("{}: {} {}..{}", t.name, e.name, e.start, e.end));
                }
            }
        }
        // Known: one WORLD event is dated 2611 in the file (a lost minus sign).
        let n_repaired = all.iter().flat_map(|t| &t.events).filter(|e| e.repaired).count();
        assert_eq!(n_repaired, 2);
        assert!(outside.is_empty(), "outside axis: {outside:?}");
    }

    #[test]
    fn every_file_parses() {
        let Some(d) = dir() else { return };
        let mut failures = Vec::new();
        let mut eras = 0;
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if ext == "era" {
                eras += 1;
                if let Err(err) = open_era(&p) {
                    failures.push(format!("{}: {err}", p.display()));
                }
            }
        }
        for (p, t) in open_all_checked(d.parent().unwrap()) {
            if let Err(err) = t {
                failures.push(format!("{}: {err}", p.display()));
            }
        }
        assert_eq!(eras, 17);
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
