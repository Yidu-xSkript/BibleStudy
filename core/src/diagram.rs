//! BibleWorks Diagramming Module files (`.dgm`): Reed-Kellogg sentence diagrams, read from the
//! user's own install and drawn as SVG.
//!
//! Container: a gzip stream (deflate, no file name) holding CRLF text. Inflated by hand below so the
//! crate needs no new dependency. A header of `##key value` lines (`##DGMF`, `##ver 1.0`, `##Title`,
//! a `##DESCRIPTION` ... `##END_DESCRIPTION##` block, `##pageWidth`/`##pageHeight` in inches, grid and
//! guide settings) ends at `##HEADER_END`. The body is a flat list of `##<Kind>Begin` ...
//! `##<Kind>End` blocks of `##key value` lines, tied together by `##gid` numbers:
//!   * `Widget` - one symbol (`WidgetClass` SubjectVerb, PrepositionalPhrase, Conjunction2L ...) with
//!     its `suggestedScale`, `widthIdx` (line thickness) and `colorIdx`, and the gids of its
//!     `WidgetLine`s and `WidgetSlot`s. Type `DGWGT_LABEL` wraps a free `Label` (`labelGid`); the
//!     connector types `DGWGT_6CURVE_CONNECTOR` / `DGWGT_2LINE_CONNECTOR` (relative clause, genitive
//!     absolute ...) are drawn by their label's polyline, which repeats the points of their lines.
//!   * `WidgetPoint` - `xy` in canvas pixels (96 dpi, y down, the saved final position, so no layout
//!     is needed); `connectedWgGid` when it is attached to another symbol; `ptIsConnectionPoint`.
//!   * `WidgetLine` - segment `pt1gid` -> `pt2gid`. `widthStyleIdx a b`: b is the line style
//!     (0 solid, 1 dotted, 3 double, 4 dashed guide); `slotIDx` names the slot whose words sit on
//!     it; `hideWhenConnected` / `hideWhenNotSelected` mark editor-only helper lines;
//!     `wordSlotTextOpaque` / `wordMayOverhangLine` mean the word sits ON the line (dotted
//!     conjunction stubs) instead of above it.
//!   * `WidgetSlot` - `slotId`, `slotType` (subject, verb, preposition ...), `align` L/C/R and
//!     `connectedWidgetWordGid` (one or more words, shown in that order).
//!   * `WidgetWord` - `str` (as displayed), `strForm` (lemma), `strMorph` (`@vipa3s`), font, size in
//!     points, `isX` for the "(X)" ellipsis marker, `strVersion`, and `bcvp book chapter verse
//!     word` (all 0-based except chapter and verse; book 0 = Matthew in BNT) pointing at the text it
//!     was taken from.
//!   * `Label` - `textbox` (`optsXYWH 0 x y w h`, `font`, `sizeAngSp size angle spacing ? bkColor`,
//!     `##textBegin` ... `##textEnd`), `callout` (as a textbox plus a pointer target in
//!     `szAngleSpXYMk`), `polyline`/`polyarrow` (`numPts-W-Sty-Clridx n width style color`, then `n`
//!     bare `x y` lines; six points are a curve).
//!
//! Words are not positioned in the file: a slot's words are written centred / left / right on its
//! line, just above it (BibleWorks sizes a line to its text plus 16 px). The editor's connection
//! bookkeeping (`xyOrig`, `xySlot`, `ptAttach`, `linkedPtIdxXY`, `relPosOnWidgetLine`,
//! `anchorSlotOnRight`) only matters while dragging symbols and is not read.
//!
//! Greek words use the BibleWorks "Bwgrkl" font encoding (see `bwgrkl_to_unicode`); the English text
//! of the examples is Arial. Text is Latin-1.

use crate::canon;
use std::{collections::HashMap, fmt::Write as _, fs, path::{Path, PathBuf}};
use unicode_normalization::UnicodeNormalization;

// ---------------------------------------------------------------------------- model

/// A canvas position in pixels (96 dpi, y down).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// `widthStyleIdx`: how a line is stroked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dotted,
    Double,
    Dashed,
}

/// Where a slot's words sit on its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Page setup and editor settings from the header. Only `width_in`/`height_in` describe the print
/// page; the SVG is sized to the content instead.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub width_in: f32,
    pub height_in: f32,
    pub magnification: f32,
    pub layout_type: u8,
    pub grid_size: u32,
}

impl Default for Page {
    fn default() -> Self {
        Page { width_in: 7.5, height_in: 10.0, magnification: 1.0, layout_type: 0, grid_size: 20 }
    }
}

/// Where a word came from: book (0 = Matthew when `version` is BNT), chapter and verse (1-based) and
/// the word's 0-based position in the verse.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WordRef {
    pub book: u16,
    pub chapter: u16,
    pub verse: u16,
    pub word: u16,
}

impl WordRef {
    /// BibleWorks book code for an NT reference (`Joh`); `None` outside the 27 NT books.
    pub fn nt_book_code(&self) -> Option<&'static str> {
        (self.book < 27).then(|| canon::BOOKS[39 + self.book as usize].code)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    /// Display text; Unicode (NFC) for Greek words.
    pub text: String,
    /// The text as stored (font encoding).
    pub raw: String,
    /// Lemma in Unicode (Greek) or as stored; empty when the file has none.
    pub lemma: String,
    /// Morphology code without the leading `@`, e.g. `vipa3s`; empty when untagged.
    pub morph: String,
    pub greek: bool,
    pub font: String,
    /// Points.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Index into the 16-colour palette; 0 is black ("ink").
    pub color: u8,
    pub background: u8,
    /// The "(X)" marker for an understood or elided word.
    pub placeholder: bool,
    /// Module the word was taken from (`BNT`, `BGT`, `KJV` ...); empty for typed text.
    pub version: String,
    pub reference: Option<WordRef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    pub id: u32,
    /// `subject`, `verb`, `directObject`, `preposition`, `conjunction`, `genericItem` ...
    pub kind: String,
    pub align: Align,
    pub words: Vec<Word>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymLine {
    pub a: Pt,
    pub b: Pt,
    /// Semantic id of the segment (`s1`, `s2` ...); empty for decoration.
    pub role: String,
    pub style: LineStyle,
    /// Index into `Symbol::slots` of the slot written on this line.
    pub slot: Option<usize>,
    /// The slot's words sit on the line (with the line broken behind them) rather than above it.
    pub word_on_line: bool,
    pub hide_when_connected: bool,
    pub hide_when_unselected: bool,
    pub a_connected: bool,
    pub b_connected: bool,
    pub a_is_connector: bool,
    pub b_is_connector: bool,
}

impl SymLine {
    /// Whether the line is part of the picture: editor helper lines are not drawn, and a stub whose
    /// connection points are all attached has served its purpose.
    pub fn visible(&self) -> bool {
        if self.hide_when_unselected {
            return false;
        }
        if self.hide_when_connected {
            let ends = [(self.a_is_connector, self.a_connected), (self.b_is_connector, self.b_connected)];
            let any = ends.iter().any(|e| e.0);
            if any && ends.iter().all(|e| !e.0 || e.1) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub id: u32,
    /// `WidgetClass`: SubjectVerb, PrepositionalPhrase, Conjunction2, Adjective-below ...
    pub kind: String,
    /// `WidgetName`: the exact variant (Adjective-below2, Conjunction2L ...).
    pub name: String,
    /// `DisplayName` from the symbol list ("Subject-Verb").
    pub label: String,
    pub scale: f32,
    /// `widthIdx`: line thickness step, 0 = normal.
    pub thickness: u8,
    pub color: u8,
    pub floating_word: bool,
    pub lines: Vec<SymLine>,
    pub slots: Vec<Slot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    pub x: f32,
    pub y: f32,
    /// Box size; 0 means "as long as the text".
    pub w: f32,
    pub h: f32,
    pub text: String,
    pub font: String,
    pub size: f32,
    pub angle: f32,
    /// A callout points at this spot.
    pub callout: Option<Pt>,
}

/// A free line, arrow or curve, or the path of a connector symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub points: Vec<Pt>,
    pub curve: bool,
    pub arrow: bool,
    pub style: LineStyle,
    pub thickness: u8,
    pub color: u8,
}

/// Drawing order is file order (later items are on top).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Symbol(Symbol),
    Text(TextBox),
    Stroke(Stroke),
}

/// A parsed `.dgm` file; `to_svg` draws it, `words` reads its text.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Diagram {
    pub title: String,
    pub description: String,
    pub page: Page,
    pub items: Vec<Item>,
}

// ---------------------------------------------------------------------------- gzip / deflate

const MAX_INFLATED: usize = 256 << 20;

struct Bits<'a> {
    src: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
    /// Zero bits invented past the end of the input (always the newest bits in `buf`).
    pad: u32,
}

impl<'a> Bits<'a> {
    fn refill(&mut self) {
        while self.cnt <= 56 {
            let b = match self.src.get(self.pos) {
                Some(&b) => {
                    self.pos += 1;
                    b
                }
                None => {
                    self.pad += 8;
                    0
                }
            };
            self.buf |= (b as u64) << self.cnt;
            self.cnt += 8;
        }
    }

    fn eat(&mut self, n: u32) -> Result<(), String> {
        self.buf >>= n;
        self.cnt -= n;
        if self.cnt < self.pad {
            return Err("truncated deflate stream".into());
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, String> {
        if self.cnt < n {
            self.refill();
        }
        let v = (self.buf & ((1u64 << n) - 1)) as u32;
        self.eat(n)?;
        Ok(v)
    }
}

/// Canonical Huffman code as a flat lookup table indexed by the next `bits` input bits.
struct Huff {
    table: Vec<u32>,
    bits: u32,
}

impl Huff {
    fn new(lens: &[u8]) -> Result<Huff, String> {
        let mut count = [0u32; 16];
        for &l in lens {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left = 1i32;
        for c in &count[1..] {
            left = (left << 1) - *c as i32;
            if left < 0 {
                return Err("over-subscribed Huffman code".into());
            }
        }
        let bits = lens.iter().copied().max().unwrap_or(0) as u32;
        let mut next = [0u32; 17];
        let mut code = 0;
        for l in 1..16 {
            code = (code + count[l - 1]) << 1;
            next[l] = code;
        }
        let mut table = vec![0u32; 1 << bits];
        for (sym, &l) in lens.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            let rev = c.reverse_bits() >> (32 - l as u32);
            let mut i = rev as usize;
            while i < table.len() {
                table[i] = ((sym as u32) << 4) | l as u32;
                i += 1 << l;
            }
        }
        Ok(Huff { table, bits })
    }

    fn decode(&self, r: &mut Bits) -> Result<usize, String> {
        if r.cnt < self.bits {
            r.refill();
        }
        let e = self.table[(r.buf & ((1u64 << self.bits) - 1)) as usize];
        let l = e & 15;
        if l == 0 {
            return Err("bad Huffman code".into());
        }
        r.eat(l)?;
        Ok((e >> 4) as usize)
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn inflate(src: &[u8]) -> Result<Vec<u8>, String> {
    let mut r = Bits { src, pos: 0, buf: 0, cnt: 0, pad: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(src.len() * 8);
    loop {
        let last = r.take(1)?;
        match r.take(2)? {
            0 => {
                let drop = r.cnt % 8;
                r.take(drop)?;
                let len = r.take(16)?;
                let nlen = r.take(16)?;
                if len != !nlen & 0xFFFF {
                    return Err("bad stored block".into());
                }
                for _ in 0..len {
                    out.push(r.take(8)? as u8);
                }
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (Huff::new(&l)?, Huff::new(&[5u8; 30])?)
                } else {
                    let nlen = r.take(5)? as usize + 257;
                    let ndist = r.take(5)? as usize + 1;
                    let ncode = r.take(4)? as usize + 4;
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in &ORDER[..ncode] {
                        cl[o] = r.take(3)? as u8;
                    }
                    let clh = Huff::new(&cl)?;
                    let mut lens = vec![0u8; nlen + ndist];
                    let mut i = 0;
                    while i < lens.len() {
                        let s = clh.decode(&mut r)?;
                        let (v, n) = match s {
                            0..=15 => (s as u8, 1),
                            16 => (*lens.get(i.wrapping_sub(1)).ok_or("repeat with no previous length")?, 3 + r.take(2)? as usize),
                            17 => (0, 3 + r.take(3)? as usize),
                            _ => (0, 11 + r.take(7)? as usize),
                        };
                        if i + n > lens.len() {
                            return Err("code lengths overrun".into());
                        }
                        lens[i..i + n].fill(v);
                        i += n;
                    }
                    (Huff::new(&lens[..nlen])?, Huff::new(&lens[nlen..])?)
                };
                loop {
                    let s = lit.decode(&mut r)?;
                    match s {
                        0..=255 => out.push(s as u8),
                        256 => break,
                        257..=285 => {
                            let k = s - 257;
                            let len = LBASE[k] as usize + r.take(LEXT[k] as u32)? as usize;
                            let d = dist.decode(&mut r)?;
                            if d >= 30 {
                                return Err("bad distance code".into());
                            }
                            let dist = DBASE[d] as usize + r.take(DEXT[d] as u32)? as usize;
                            if dist > out.len() {
                                return Err("distance too far back".into());
                            }
                            let start = out.len() - dist;
                            if dist >= len {
                                out.extend_from_within(start..start + len);
                            } else {
                                for j in 0..len {
                                    out.push(out[start + j]);
                                }
                            }
                        }
                        _ => return Err("bad length code".into()),
                    }
                    if out.len() > MAX_INFLATED {
                        return Err("inflated data too large".into());
                    }
                }
            }
            _ => return Err("bad deflate block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

/// Inflate a gzip member (header flags honoured, trailer CRC not checked).
fn gunzip(b: &[u8]) -> Result<Vec<u8>, String> {
    if b.len() < 18 || b[0] != 0x1f || b[1] != 0x8b || b[2] != 8 {
        return Err("not a gzip stream".into());
    }
    let flags = b[3];
    let mut p = 10;
    if flags & 4 != 0 {
        let n = *b.get(p).ok_or("short gzip header")? as usize | (*b.get(p + 1).ok_or("short gzip header")? as usize) << 8;
        p += 2 + n;
    }
    for bit in [8, 16] {
        if flags & bit != 0 {
            p += b.get(p..).and_then(|s| s.iter().position(|&c| c == 0)).ok_or("unterminated gzip header")? + 1;
        }
    }
    if flags & 2 != 0 {
        p += 2;
    }
    inflate(b.get(p..).ok_or("short gzip header")?)
}

// ---------------------------------------------------------------------------- Greek font encoding

const LOWER: &str = "abcdefghijklmnopqrstuwxyz";
const LOWER_GREEK: &str = "αβχδεφγηιςκλμνοπθρστυωξψζ";
const UPPER: &str = "ABCDEFGHIKLMNOPQRSTUWXYZ";
const UPPER_GREEK: &str = "ΑΒΧΔΕΦΓΗΙΚΛΜΝΟΠΘΡΣΤΥΩΞΨΖ";

fn letter(c: char) -> Option<char> {
    let (from, to) = if c.is_ascii_lowercase() { (LOWER, LOWER_GREEK) } else { (UPPER, UPPER_GREEK) };
    from.chars().position(|f| f == c).and_then(|i| to.chars().nth(i))
}

const ACUTE: char = '\u{301}';
const GRAVE: char = '\u{300}';
const CIRC: char = '\u{342}';
const SMOOTH: char = '\u{313}';
const ROUGH: char = '\u{314}';
const DIAER: char = '\u{308}';
const IOTA_SUB: char = '\u{345}';

/// Zero-width accent glyphs that follow the letter they sit on.
fn suffix_marks(c: char) -> Option<&'static [char]> {
    Some(match c {
        ',' => &[ACUTE],
        '.' => &[GRAVE],
        '/' => &[CIRC],
        'v' => &[SMOOTH],
        '`' => &[ROUGH],
        ';' => &[SMOOTH, ACUTE],
        '[' => &[ROUGH, ACUTE],
        '\'' => &[SMOOTH, GRAVE],
        ']' => &[ROUGH, GRAVE],
        '=' => &[SMOOTH, CIRC],
        '-' => &[ROUGH, CIRC],
        '?' => &[DIAER],
        '<' => &[DIAER, ACUTE],
        '>' => &[DIAER, GRAVE],
        '_' => &[DIAER, CIRC],
        '|' => &[IOTA_SUB],
        _ => return None,
    })
}

/// Breathing glyphs placed before a capital (the font has no precomposed capitals).
fn prefix_marks(c: char) -> Option<&'static [char]> {
    Some(match c {
        'V' => &[SMOOTH],
        '~' => &[ROUGH],
        '{' => &[ROUGH, ACUTE],
        '}' => &[ROUGH, GRAVE],
        ':' => &[SMOOTH, ACUTE],
        '"' => &[SMOOTH, GRAVE],
        '+' => &[SMOOTH, CIRC],
        _ => return None,
    })
}

fn punct(c: char) -> Option<char> {
    Some(match c {
        '(' => ',',
        ')' => '.',
        '*' => ';',
        '\\' => '·',
        '$' | '¿' => '(',
        '%' | 'À' => ')',
        '@' | 'Î' => '[',
        '#' | 'Ð' => ']',
        '&' => '-',
        '!' => '+',
        '^' => '*',
        'Å' => '.',
        _ => return None,
    })
}

/// Text in the BibleWorks "Bwgrkl" font encoding -> Unicode (NFC).
///
/// The font is plain ASCII with the accents as zero-width glyphs: `a`..`z`, `A`..`Z` are the letters
/// (`j` = final sigma, `q` theta, `c` chi, `x` xi, `y` psi, `f` phi, `C` Chi, `Q` Theta ...), an
/// accent follows its letter (`,` acute `.` grave `/` circumflex `v` smooth `` ` `` rough `;` smooth +
/// acute `[` rough + acute `'` smooth + grave `]` rough + grave `=` smooth + circumflex `-` rough +
/// circumflex `?` diaeresis `<`/`>` diaeresis + acute/grave `|` iota subscript), a capital takes its
/// breathing as a prefix (`V` smooth, `~` rough, `{` rough + acute, `:` smooth + acute ...), and a
/// `V` after a letter is the elision apostrophe. Brackets and punctuation sit on other ASCII keys
/// (`$ %` parentheses, `@ #` square brackets, `Î Ð` the thin brackets of textual variants).
pub fn bwgrkl_to_unicode(raw: &str) -> String {
    struct Cell {
        ch: char,
        marks: Vec<char>,
        letter: bool,
    }
    let mut cells: Vec<Cell> = Vec::new();
    let mut pending: Vec<char> = Vec::new();
    for c in raw.chars() {
        if let Some(g) = letter(c) {
            cells.push(Cell { ch: g, marks: std::mem::take(&mut pending), letter: true });
        } else if c == 'V' && cells.last().is_some_and(|l| l.letter) {
            cells.push(Cell { ch: '’', marks: Vec::new(), letter: false });
        } else if let Some(m) = prefix_marks(c) {
            pending.extend(m);
        } else if let Some(m) = suffix_marks(c) {
            match cells.last_mut() {
                Some(l) if l.letter => l.marks.extend(m),
                _ => cells.push(Cell { ch: c, marks: Vec::new(), letter: false }),
            }
        } else {
            cells.push(Cell { ch: punct(c).unwrap_or(c), marks: Vec::new(), letter: false });
        }
    }
    if !pending.is_empty() {
        cells.push(Cell { ch: '’', marks: Vec::new(), letter: false }); // lone `V` at the end of a word
    }
    let mut out = String::new();
    for c in &cells {
        out.push(c.ch);
        out.extend(&c.marks);
    }
    out.nfc().collect()
}

/// Latin-1 bytes as text.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

// ---------------------------------------------------------------------------- parsing

#[derive(Default)]
struct RawWidget {
    gid: u32,
    ty: String,
    display: String,
    name: String,
    class: String,
    scale: f32,
    width: u8,
    color: u8,
    floating: bool,
    slots: Vec<u32>,
    lines: Vec<u32>,
    label: Option<u32>,
}

#[derive(Default)]
struct RawPoint {
    gid: u32,
    p: Pt,
    connected: bool,
    connector: bool,
}

#[derive(Default)]
struct RawLine {
    gid: u32,
    p1: u32,
    p2: u32,
    role: String,
    slot: u32,
    style: u8,
    on_line: bool,
    hide_connected: bool,
    hide_unselected: bool,
}

struct RawSlot {
    gid: u32,
    id: u32,
    kind: String,
    align: Align,
    words: Vec<u32>,
}

#[derive(Default)]
struct RawLabel {
    gid: u32,
    ty: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    font: String,
    size: f32,
    angle: f32,
    text: String,
    pts: Vec<Pt>,
    style: u8,
    thick: u8,
    color: u8,
    arrow: bool,
    target: Option<Pt>,
}

enum Cur {
    None,
    Widget(RawWidget),
    Point(RawPoint),
    Line(RawLine),
    Slot(RawSlot),
    Word(u32, Word),
    Label(RawLabel),
}

fn num<T: std::str::FromStr>(v: &[u8]) -> Option<T> {
    std::str::from_utf8(v).ok()?.trim().parse().ok()
}

fn nums(v: &[u8]) -> Vec<f32> {
    std::str::from_utf8(v).map(|s| s.split_whitespace().filter_map(|t| t.parse().ok()).collect()).unwrap_or_default()
}

fn flag(v: &[u8]) -> bool {
    v.first() == Some(&b'y')
}

fn style_of(n: u8) -> LineStyle {
    match n {
        1 => LineStyle::Dotted,
        3 => LineStyle::Double,
        4 => LineStyle::Dashed,
        _ => LineStyle::Solid,
    }
}

fn null_to_empty(s: String) -> String {
    if s == "<null>" {
        String::new()
    } else {
        s
    }
}

fn new_word() -> Word {
    Word {
        text: String::new(),
        raw: String::new(),
        lemma: String::new(),
        morph: String::new(),
        greek: false,
        font: String::new(),
        size: 11.0,
        bold: false,
        italic: false,
        underline: false,
        color: 0,
        background: 15,
        placeholder: false,
        version: String::new(),
        reference: None,
    }
}

/// Decode a finished word record: Greek words go through `bwgrkl_to_unicode`.
fn finish_word(mut w: Word, lang: &str) -> Word {
    w.greek = w.font.eq_ignore_ascii_case("bwgrkl") || lang == "Greek";
    let lemma = std::mem::take(&mut w.lemma);
    if w.greek {
        w.text = bwgrkl_to_unicode(&w.raw);
        w.lemma = bwgrkl_to_unicode(&lemma);
    } else {
        w.text = w.raw.clone();
        w.lemma = lemma;
    }
    w
}

impl Diagram {
    /// Parse a `.dgm` file (gzip, or the same text uncompressed).
    pub fn parse(bytes: &[u8]) -> Result<Diagram, String> {
        parse(bytes)
    }

    /// Read and parse a `.dgm` file.
    pub fn open(path: impl AsRef<Path>) -> Result<Diagram, String> {
        let b = fs::read(path.as_ref()).map_err(|e| format!("{}: {e}", path.as_ref().display()))?;
        parse(&b)
    }
}

/// Parse the bytes of a `.dgm` file. Anything that is not a diagram is an `Err`, never a panic.
pub fn parse(bytes: &[u8]) -> Result<Diagram, String> {
    let inflated;
    let text: &[u8] = if bytes.starts_with(b"##DGMF") {
        bytes
    } else {
        inflated = gunzip(bytes)?;
        &inflated
    };
    if !text.starts_with(b"##DGMF") {
        return Err("not a BibleWorks diagram (missing ##DGMF)".into());
    }
    let mut d = Diagram::default();
    let mut widgets: Vec<RawWidget> = Vec::new();
    let mut points: HashMap<u32, RawPoint> = HashMap::new();
    let mut lines: HashMap<u32, RawLine> = HashMap::new();
    let mut slots: HashMap<u32, RawSlot> = HashMap::new();
    let mut words: HashMap<u32, Word> = HashMap::new();
    let mut labels: HashMap<u32, RawLabel> = HashMap::new();
    let mut cur = Cur::None;
    let mut in_desc = false;
    let mut in_text = false;
    let mut desc: Vec<String> = Vec::new();
    let mut lang = String::new();

    for line in text.split(|&c| c == b'\n') {
        let line = {
            let mut l = line;
            while let [rest @ .., b'\r'] = l {
                l = rest;
            }
            l
        };
        if in_text {
            if line == b"##textEnd" {
                in_text = false;
            } else if let Cur::Label(l) = &mut cur {
                if !l.text.is_empty() || l.gid != 0 {
                    if !l.text.is_empty() {
                        l.text.push('\n');
                    }
                    l.text.push_str(&latin1(line));
                }
            }
            continue;
        }
        if !line.starts_with(b"##") && !line.starts_with(b"fSnapToHVGuides") {
            if in_desc {
                desc.push(latin1(line));
            } else if let (Cur::Label(l), true) = (&mut cur, !line.is_empty()) {
                let n = nums(line);
                if n.len() == 2 {
                    l.pts.push(Pt { x: n[0], y: n[1] });
                }
            }
            continue;
        }
        let body = line.strip_prefix(b"##").unwrap_or(line);
        let (key, val) = match body.iter().position(|&c| c == b' ') {
            Some(i) => (&body[..i], &body[i + 1..]),
            None => (body, &body[body.len()..]),
        };
        // block boundaries
        match key {
            b"WidgetBegin" => cur = Cur::Widget(RawWidget { scale: 1.0, ..Default::default() }),
            b"WidgetPointBegin" => cur = Cur::Point(RawPoint::default()),
            b"WidgetLineBegin" => cur = Cur::Line(RawLine::default()),
            b"WidgetSlotBegin" => cur = Cur::Slot(RawSlot { gid: 0, id: 0, kind: String::new(), align: Align::Center, words: Vec::new() }),
            b"WidgetWordBegin" => {
                cur = Cur::Word(0, new_word());
                lang.clear();
            }
            b"LabelBegin" => cur = Cur::Label(RawLabel { size: 12.0, ..Default::default() }),
            b"textBegin" => in_text = true,
            b"WidgetEnd" | b"WidgetPointEnd" | b"WidgetLineEnd" | b"WidgetSlotEnd" | b"WidgetWordEnd" | b"LabelEnd" => {
                match std::mem::replace(&mut cur, Cur::None) {
                    Cur::Widget(w) => widgets.push(w),
                    Cur::Point(p) => {
                        points.insert(p.gid, p);
                    }
                    Cur::Line(l) => {
                        lines.insert(l.gid, l);
                    }
                    Cur::Slot(s) => {
                        slots.insert(s.gid, s);
                    }
                    Cur::Word(g, w) => {
                        words.insert(g, finish_word(w, &lang));
                    }
                    Cur::Label(l) => {
                        labels.insert(l.gid, l);
                    }
                    Cur::None => {}
                }
            }
            b"END_DESCRIPTION##" => in_desc = false,
            b"DESCRIPTION" if matches!(cur, Cur::None) => in_desc = true,
            _ => match &mut cur {
                Cur::None => header_key(&mut d, key, val),
                Cur::Widget(w) => match key {
                    b"gid" => w.gid = num(val).unwrap_or(0),
                    b"type" => w.ty = latin1(val),
                    b"DisplayName" => w.display = latin1(val),
                    b"WidgetName" => w.name = latin1(val),
                    b"WidgetClass" => w.class = latin1(val),
                    b"suggestedScale" => w.scale = num(val).unwrap_or(1.0),
                    b"widthIdx" => w.width = num(val).unwrap_or(0),
                    b"colorIdx" => w.color = num(val).unwrap_or(0),
                    b"fFloatingWord" => w.floating = flag(val),
                    b"slotGid" => w.slots.extend(num::<u32>(val)),
                    b"lineGid" => w.lines.extend(num::<u32>(val)),
                    b"labelGid" => w.label = num(val),
                    _ => {}
                },
                Cur::Point(p) => match key {
                    b"gid" => p.gid = num(val).unwrap_or(0),
                    b"xy" => {
                        let n = nums(val);
                        if n.len() == 2 {
                            p.p = Pt { x: n[0], y: n[1] };
                        }
                    }
                    b"connectedWgGid" => p.connected = true,
                    b"ptIsConnectionPoint" => p.connector = flag(val),
                    _ => {}
                },
                Cur::Line(l) => match key {
                    b"gid" => l.gid = num(val).unwrap_or(0),
                    b"pt1gid" => l.p1 = num(val).unwrap_or(0),
                    b"pt2gid" => l.p2 = num(val).unwrap_or(0),
                    b"semId" => l.role = latin1(val),
                    b"slotIDx" => l.slot = num(val).unwrap_or(0),
                    b"widthStyleIdx" => l.style = nums(val).get(1).map_or(0, |&v| v as u8),
                    b"wordSlotTextOpaque" | b"wordMayOverhangLine" => l.on_line |= flag(val),
                    b"hideWhenConnected" => l.hide_connected = flag(val),
                    b"hideWhenNotSelected" => l.hide_unselected = flag(val),
                    _ => {}
                },
                Cur::Slot(s) => match key {
                    b"gid" => s.gid = num(val).unwrap_or(0),
                    b"slotId" => s.id = num(val).unwrap_or(0),
                    b"slotType" => s.kind = latin1(val),
                    b"align" => {
                        s.align = match val.first() {
                            Some(b'L') => Align::Left,
                            Some(b'R') => Align::Right,
                            _ => Align::Center,
                        }
                    }
                    b"connectedWidgetWordGid" => s.words.extend(num::<u32>(val)),
                    _ => {}
                },
                Cur::Word(g, w) => match key {
                    b"gid" => *g = num(val).unwrap_or(0),
                    b"isX" => w.placeholder = flag(val),
                    b"str" => w.raw = latin1(val),
                    b"strMorph" => w.morph = null_to_empty(latin1(val)).trim_start_matches('@').to_string(),
                    b"strForm" => w.lemma = null_to_empty(latin1(val)),
                    b"langName" => lang = latin1(val),
                    b"fontName" => w.font = latin1(val),
                    b"fontSize" => w.size = num(val).unwrap_or(11.0),
                    b"bold" => w.bold = flag(val),
                    b"italic" => w.italic = flag(val),
                    b"underline" => w.underline = flag(val),
                    b"colorIdx" => w.color = num(val).unwrap_or(0),
                    b"bkColorIdx" => w.background = num(val).unwrap_or(15),
                    b"strVersion" => w.version = latin1(val).trim().to_string(),
                    b"bcvp" => {
                        let n = nums(val);
                        if n.len() == 4 && n[1] > 0.0 && n[2] > 0.0 {
                            w.reference = Some(WordRef { book: n[0] as u16, chapter: n[1] as u16, verse: n[2] as u16, word: n[3] as u16 });
                        }
                    }
                    _ => {}
                },
                Cur::Label(l) => match key {
                    b"gid" => l.gid = num(val).unwrap_or(0),
                    b"type" => l.ty = latin1(val),
                    b"optsXYWH" => {
                        let n = nums(val);
                        if n.len() == 5 {
                            (l.x, l.y, l.w, l.h) = (n[1], n[2], n[3], n[4]);
                        }
                    }
                    b"font" => l.font = latin1(val),
                    b"sizeAngSp" | b"szAngleSpXYMk" => {
                        let n = nums(val);
                        if n.len() >= 2 {
                            (l.size, l.angle) = (n[0], n[1]);
                        }
                        if key == b"szAngleSpXYMk" && n.len() >= 7 {
                            l.target = Some(Pt { x: n[5], y: n[6] });
                        }
                    }
                    b"numPts-W-Sty-Clridx" => {
                        let n = nums(val);
                        if n.len() == 4 {
                            (l.thick, l.style, l.color) = (n[1] as u8, n[2] as u8, n[3] as u8);
                        }
                    }
                    b"arrow" => l.arrow = flag(val),
                    _ => {}
                },
            },
        }
    }
    if widgets.is_empty() && labels.is_empty() {
        return Err("diagram has no symbols".into());
    }
    d.description = desc.join("\n").trim().to_string();
    for w in widgets {
        if let Some(item) = assemble(w, &points, &lines, &slots, &mut words, &labels) {
            d.items.push(item);
        }
    }
    Ok(d)
}

fn header_key(d: &mut Diagram, key: &[u8], val: &[u8]) {
    match key {
        b"Title" => d.title = latin1(val).trim().to_string(),
        b"pageWidth" => d.page.width_in = num(val).unwrap_or(d.page.width_in),
        b"pageHeight" => d.page.height_in = num(val).unwrap_or(d.page.height_in),
        b"magLevel" => d.page.magnification = num(val).unwrap_or(1.0),
        b"layoutType" => d.page.layout_type = num(val).unwrap_or(0),
        b"fGridSize" => d.page.grid_size = num(val).unwrap_or(20),
        _ => {}
    }
}

fn assemble(
    w: RawWidget,
    points: &HashMap<u32, RawPoint>,
    lines: &HashMap<u32, RawLine>,
    slots: &HashMap<u32, RawSlot>,
    words: &mut HashMap<u32, Word>,
    labels: &HashMap<u32, RawLabel>,
) -> Option<Item> {
    let label = w.label.and_then(|g| labels.get(&g));
    match w.ty.as_str() {
        "DGWGT_LABEL" => {
            let l = label?;
            match l.ty.as_str() {
                "textbox" | "callout" => Some(Item::Text(TextBox {
                    x: l.x,
                    y: l.y,
                    w: l.w,
                    h: l.h,
                    text: l.text.clone(),
                    font: l.font.clone(),
                    size: l.size,
                    angle: l.angle,
                    callout: if l.ty == "callout" { l.target } else { None },
                })),
                _ => stroke_of(l, false),
            }
        }
        "DGWGT_6CURVE_CONNECTOR" | "DGWGT_2LINE_CONNECTOR" => {
            let curve = w.ty == "DGWGT_6CURVE_CONNECTOR";
            if let Some(l) = label.filter(|l| l.pts.len() >= 2) {
                return stroke_of(l, curve);
            }
            // no polyline: chain the symbol's own segments
            let mut pts: Vec<Pt> = Vec::new();
            for g in &w.lines {
                let l = lines.get(g)?;
                let (a, b) = (points.get(&l.p1)?.p, points.get(&l.p2)?.p);
                if pts.last() != Some(&a) {
                    pts.push(a);
                }
                pts.push(b);
            }
            (pts.len() >= 2).then_some(Item::Stroke(Stroke { points: pts, curve, arrow: false, style: LineStyle::Dotted, thickness: w.width, color: w.color }))
        }
        _ => {
            let mut sym_slots: Vec<Slot> = Vec::new();
            for g in &w.slots {
                if let Some(s) = slots.get(g) {
                    let ws = s.words.iter().filter_map(|g| words.remove(g)).collect();
                    sym_slots.push(Slot { id: s.id, kind: s.kind.clone(), align: s.align, words: ws });
                }
            }
            let mut sym_lines = Vec::new();
            for g in &w.lines {
                let Some(l) = lines.get(g) else { continue };
                let (Some(a), Some(b)) = (points.get(&l.p1), points.get(&l.p2)) else { continue };
                // slot id 0 means "no slot" except for the floating word, whose only slot is 0
                let slot = sym_slots.iter().position(|s| s.id == l.slot);
                sym_lines.push(SymLine {
                    a: a.p,
                    b: b.p,
                    role: l.role.clone(),
                    style: style_of(l.style),
                    slot,
                    word_on_line: l.on_line,
                    hide_when_connected: l.hide_connected,
                    hide_when_unselected: l.hide_unselected,
                    a_connected: a.connected,
                    b_connected: b.connected,
                    a_is_connector: a.connector,
                    b_is_connector: b.connector,
                });
            }
            Some(Item::Symbol(Symbol {
                id: w.gid,
                kind: w.class,
                name: w.name,
                label: w.display,
                scale: w.scale,
                thickness: w.width,
                color: w.color,
                floating_word: w.floating,
                lines: sym_lines,
                slots: sym_slots,
            }))
        }
    }
}

fn stroke_of(l: &RawLabel, curve: bool) -> Option<Item> {
    (l.pts.len() >= 2).then(|| {
        Item::Stroke(Stroke {
            points: l.pts.clone(),
            curve: curve || (l.ty == "polyarrow" && l.pts.len() > 2),
            arrow: l.arrow,
            style: style_of(l.style),
            thickness: l.thick,
            color: l.color,
        })
    })
}

// ---------------------------------------------------------------------------- reading order

impl Diagram {
    /// The diagram's words ("(X)" markers and parenthesised repeats excluded) in reading order: by the verse and
    /// word position they were taken from, then typed text top to bottom, left to right.
    pub fn words(&self) -> Vec<&Word> {
        let mut v: Vec<(Option<&WordRef>, f32, f32, &Word)> = Vec::new();
        for it in &self.items {
            let Item::Symbol(s) = it else { continue };
            for (i, slot) in s.slots.iter().enumerate() {
                let at = s.lines.iter().find(|l| l.slot == Some(i)).map_or(Pt::default(), |l| Pt { x: (l.a.x + l.b.x) / 2.0, y: (l.a.y + l.b.y) / 2.0 });
                // "(...)" words are understood words written in again, not text of the passage
                for w in slot.words.iter().filter(|w| !w.placeholder && !w.text.trim().is_empty() && !w.text.starts_with('(')) {
                    v.push((w.reference.as_ref(), at.y, at.x, w));
                }
            }
        }
        v.sort_by(|a, b| match (a.0, b.0) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)),
        });
        v.into_iter().map(|t| t.3).collect()
    }

    /// `words()` as one space-separated string.
    pub fn text(&self) -> String {
        self.words().iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

// ---------------------------------------------------------------------------- SVG

/// The 16 colours of the Line Properties list (`colorIdx`). 0 is black, 15 white (`bkColorIdx` 15 is
/// the page).
// ponytail: only indices 0 and 3 occur in the shipped files and the index->colour table is not stored
// anywhere in them; this is the standard Windows 16-colour order, a guess for everything but 0.
const PALETTE: [&str; 16] = [
    "", "#800000", "#008000", "#808000", "#000080", "#800080", "#008080", "#c0c0c0", "#808080", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
];

fn ink(idx: u8) -> &'static str {
    match PALETTE.get(idx as usize) {
        Some(c) if !c.is_empty() => c,
        _ => "currentColor",
    }
}

// ponytail: BibleWorks stores a thickness step (0, 1, ...) but not its pixel width; 1.25 px for 0 and
// +1 px per step is a guess (only five symbols in the shipped files use a non-zero step).
fn stroke_width(idx: u8) -> f32 {
    1.25 + idx as f32
}

const PX_PER_PT: f32 = 4.0 / 3.0;
const GREEK_FONT: &str = "BS Greek, Galatia SIL, Cambria, serif";
const LATIN_FONT: &str = "Arial, Helvetica, sans-serif";
const MARGIN: f32 = 20.0;
/// Space between a slot's text and the ends of its line (BibleWorks sizes lines to text + 16 px).
const SLOT_PAD: f32 = 8.0;

fn fin(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

fn n(v: f32) -> String {
    let s = format!("{:.1}", fin(v));
    match s.strip_suffix(".0") {
        Some(t) => t.to_string(),
        None => s,
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n') => {}
            c => o.push(c),
        }
    }
    o
}

/// Advance widths of the Bwgrkl font in 1/1000 em for ASCII 0x20..=0x7e; accents advance 0.
const BW_ADV: [u16; 95] = [
    400, 526, 374, 324, 300, 300, 300, 0, 250, 250, 250, 329, 0, 0, 0, 0, 445, 445, 445, 445, 445, 445, 445, 445, 445, 445, 424, 0, 0, 0, 0, 0, 312, 692, 630, 741, 599, 625, 655, 593, 727, 333, 0, 708, 674,
    888, 731, 616, 695, 613, 590, 587, 677, 683, 222, 602, 564, 686, 564, 0, 281, 0, 350, 333, 0, 476, 417, 473, 426, 376, 546, 470, 479, 375, 368, 461, 467, 485, 473, 448, 485, 444, 481, 472, 415, 451, 0,
    622, 351, 570, 435, 378, 0, 401, 232,
];

/// Width in em of text in the Bwgrkl encoding, as the Diagramming Module laid it out.
fn bw_em(raw: &str) -> f32 {
    raw.chars()
        .map(|c| match c {
            ' '..='~' => BW_ADV[c as usize - 0x20] as f32,
            '¿' | 'À' => 300.0,
            'Î' => 331.0,
            'Ð' => 337.0,
            'Å' => 250.0,
            _ => 450.0,
        })
        .sum::<f32>()
        / 1000.0
}

/// Rough text width in px for text without font metrics (the SVG itself centres with `text-anchor`).
fn est_width(s: &str, px: f32) -> f32 {
    s.chars().filter(|c| !('\u{300}'..='\u{36F}').contains(c)).count() as f32 * 0.55 * px
}

#[derive(Default)]
struct Bounds {
    min: Option<Pt>,
    max: Option<Pt>,
}

impl Bounds {
    fn add(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let (lo, hi) = (self.min.get_or_insert(Pt { x, y }), self.max.get_or_insert(Pt { x, y }));
        lo.x = lo.x.min(x);
        lo.y = lo.y.min(y);
        hi.x = hi.x.max(x);
        hi.y = hi.y.max(y);
    }
}

impl Diagram {
    /// Extent of everything drawn, in canvas pixels: (min x, min y, max x, max y).
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut b = Bounds::default();
        self.draw(&mut String::new(), &mut b);
        Some((b.min?.x, b.min?.y, b.max?.x, b.max?.y))
    }

    /// A self-contained SVG of the diagram, sized to its content.
    ///
    /// Black ink is `currentColor`, so inlined SVG follows the page's text colour. Words that sit on
    /// a dotted stub get a halo in `var(--dgm-bg)` (unset = none) so the dots stop around the text.
    /// Greek text is Unicode with `font-family` "BS Greek, Galatia SIL, Cambria, serif"; `data-ref`
    /// on a word's `<tspan>` is `book.chapter.verse.word` for words taken from a Bible text.
    pub fn to_svg(&self) -> String {
        let mut body = String::new();
        let mut b = Bounds::default();
        self.draw(&mut body, &mut b);
        let (lo, hi) = match (b.min, b.max) {
            (Some(lo), Some(hi)) => (lo, hi),
            _ => (Pt::default(), Pt { x: 160.0, y: 60.0 }),
        };
        let (x0, y0) = (lo.x - MARGIN, lo.y - MARGIN);
        let (w, h) = (hi.x - lo.x + 2.0 * MARGIN, hi.y - lo.y + 2.0 * MARGIN);
        let mut s = String::with_capacity(body.len() + 400);
        let _ = writeln!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" width=\"{}\" height=\"{}\" font-family=\"{GREEK_FONT}\" role=\"img\"{}>",
            n(x0),
            n(y0),
            n(w),
            n(h),
            n(w),
            n(h),
            if self.title.is_empty() { String::new() } else { format!(" aria-label=\"{}\"", esc(&self.title)) },
        );
        s.push_str(&body);
        s.push_str("</svg>\n");
        s
    }

    /// Emit the drawing into `out` and grow `b` by everything placed.
    fn draw(&self, out: &mut String, b: &mut Bounds) {
        for it in &self.items {
            match it {
                Item::Symbol(s) => draw_symbol(s, out, b),
                Item::Text(t) => draw_text_box(t, out, b),
                Item::Stroke(s) => draw_stroke(s, out, b),
            }
        }
    }
}

fn line_path(a: Pt, bb: Pt, style: LineStyle, d: &mut String) {
    let _ = write!(d, "M{} {}L{} {}", n(a.x), n(a.y), n(bb.x), n(bb.y));
    if style == LineStyle::Double {
        let (dx, dy) = (bb.x - a.x, bb.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 {
            let (nx, ny) = (-dy / len * 1.6, dx / len * 1.6);
            d.clear();
            let _ = write!(d, "M{} {}L{} {}M{} {}L{} {}", n(a.x + nx), n(a.y + ny), n(bb.x + nx), n(bb.y + ny), n(a.x - nx), n(a.y - ny), n(bb.x - nx), n(bb.y - ny));
        }
    }
}

fn draw_symbol(s: &Symbol, out: &mut String, b: &mut Bounds) {
    let (mut solid, mut dotted, mut dashed) = (String::new(), String::new(), String::new());
    for l in s.lines.iter().filter(|l| l.visible()) {
        b.add(l.a.x, l.a.y);
        b.add(l.b.x, l.b.y);
        let mut d = String::new();
        line_path(l.a, l.b, l.style, &mut d);
        match l.style {
            LineStyle::Dotted => dotted.push_str(&d),
            LineStyle::Dashed => dashed.push_str(&d),
            _ => solid.push_str(&d),
        }
    }
    let _ = write!(out, "<g data-sym=\"{}\">", esc(&s.kind));
    let (col, sw) = (ink(s.color), n(stroke_width(s.thickness)));
    if !solid.is_empty() {
        let _ = write!(out, "<path d=\"{solid}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{sw}\" stroke-linecap=\"round\"/>");
    }
    if !dotted.is_empty() {
        let _ = write!(out, "<path d=\"{dotted}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{sw}\" stroke-dasharray=\"1.5 3\"/>");
    }
    if !dashed.is_empty() {
        let _ = write!(out, "<path d=\"{dashed}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{sw}\" stroke-dasharray=\"6 4\"/>");
    }
    for (i, slot) in s.slots.iter().enumerate() {
        let Some(line) = s.lines.iter().find(|l| l.slot == Some(i)) else { continue };
        if slot.words.is_empty() {
            continue;
        }
        draw_slot(slot, line, out, b);
    }
    out.push_str("</g>\n");
}

fn draw_slot(slot: &Slot, line: &SymLine, out: &mut String, b: &mut Bounds) {
    let size = slot.words.iter().map(|w| w.size).fold(0.0, f32::max).max(6.0) * PX_PER_PT;
    let space = 0.4 * size;
    let width: f32 = slot.words.iter().map(|w| if w.greek { bw_em(&w.raw) * w.size * PX_PER_PT } else { est_width(&w.text, w.size * PX_PER_PT) }).sum::<f32>()
        + space * slot.words.len().saturating_sub(1) as f32;
    // Lines were sized to the text in the Bwgrkl font; stretching Greek runs to that width keeps the
    // layout intact in whatever font the viewer substitutes.
    let fit = if slot.words.iter().all(|w| w.greek) { format!(" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"", n(width)) } else { String::new() };
    let (lx, rx) = (line.a.x.min(line.b.x), line.a.x.max(line.b.x));
    let vertical = (line.a.x - line.b.x).abs() < 0.5 && (line.a.y - line.b.y).abs() > 0.5;
    let (x, y, anchor) = if vertical {
        // ponytail: a word on a vertical stilt (quotation) is written to the right of it, unrotated
        (lx + 6.0, (line.a.y + line.b.y) / 2.0 + 0.32 * size, "start")
    } else {
        let ly = (line.a.y + line.b.y) / 2.0;
        let y = if line.word_on_line { ly + 0.32 * size } else { ly - 0.28 * size };
        match slot.align {
            Align::Center => ((lx + rx) / 2.0, y, "middle"),
            Align::Left => (lx + SLOT_PAD, y, "start"),
            Align::Right => (rx - SLOT_PAD, y, "end"),
        }
    };
    let (x0, x1) = match anchor {
        "middle" => (x - width / 2.0, x + width / 2.0),
        "start" => (x, x + width),
        _ => (x - width, x),
    };
    b.add(x0, y - size);
    b.add(x1, y + 0.3 * size);
    let halo = if line.word_on_line { " stroke-width=\"5\" paint-order=\"stroke\" stroke-linejoin=\"round\" style=\"stroke:var(--dgm-bg,none)\"" } else { "" };
    let _ = write!(out, "<text x=\"{}\" y=\"{}\" text-anchor=\"{anchor}\" font-size=\"{}\" fill=\"currentColor\"{fit}{halo}>", n(x), n(y), n(size));
    for (i, w) in slot.words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str("<tspan");
        if !w.greek {
            let _ = write!(out, " font-family=\"{LATIN_FONT}\"");
        }
        if (w.size * PX_PER_PT - size).abs() > 0.1 {
            let _ = write!(out, " font-size=\"{}\"", n(w.size * PX_PER_PT));
        }
        if w.color != 0 {
            let _ = write!(out, " fill=\"{}\"", ink(w.color));
        }
        if w.bold {
            out.push_str(" font-weight=\"bold\"");
        }
        if w.italic {
            out.push_str(" font-style=\"italic\"");
        }
        if w.underline {
            out.push_str(" text-decoration=\"underline\"");
        }
        if let Some(r) = &w.reference {
            let _ = write!(out, " data-ref=\"{}.{}.{}.{}\"", r.book, r.chapter, r.verse, r.word);
        }
        let _ = write!(out, ">{}</tspan>", esc(&w.text));
    }
    out.push_str("</text>");
}

fn draw_text_box(t: &TextBox, out: &mut String, b: &mut Bounds) {
    let size = t.size.max(6.0) * PX_PER_PT;
    let lines: Vec<&str> = t.text.split('\n').collect();
    let width = lines.iter().map(|l| est_width(l, size)).fold(0.0, f32::max).max(t.w);
    let height = (lines.len() as f32 * 1.25 * size).max(t.h);
    b.add(t.x, t.y);
    b.add(t.x + width, t.y + height);
    if let Some(p) = t.callout {
        // leader from the nearest point of the text box to the target
        let (cx, cy) = (p.x.clamp(t.x, t.x + width), p.y.clamp(t.y, t.y + height));
        b.add(p.x, p.y);
        let _ = write!(
            out,
            "<path d=\"M{} {}L{} {}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\"/>",
            n(cx),
            n(cy),
            n(p.x),
            n(p.y)
        );
    }
    let fam = if t.font.eq_ignore_ascii_case("bwgrkl") { GREEK_FONT } else { LATIN_FONT };
    let rot = if t.angle.abs() > 0.01 { format!(" transform=\"rotate({} {} {})\"", n(-t.angle), n(t.x), n(t.y)) } else { String::new() };
    // ponytail: no word wrap inside `w`; only the author's own line breaks are honoured
    let _ = write!(out, "<text x=\"{}\" y=\"{}\" font-family=\"{fam}\" font-size=\"{}\" fill=\"currentColor\"{rot}>", n(t.x), n(t.y + size), n(size));
    for (i, l) in lines.iter().enumerate() {
        let _ = write!(out, "<tspan x=\"{}\" dy=\"{}\">{}</tspan>", n(t.x), if i == 0 { "0".to_string() } else { n(1.25 * size) }, esc(l));
    }
    out.push_str("</text>\n");
}

fn draw_stroke(s: &Stroke, out: &mut String, b: &mut Bounds) {
    let p = &s.points;
    if p.len() < 2 {
        return;
    }
    for q in p {
        b.add(q.x, q.y);
    }
    let mut d = String::new();
    let _ = write!(d, "M{} {}", n(p[0].x), n(p[0].y));
    if s.curve && p.len() > 2 {
        // Catmull-Rom through the points, as cubic Beziers
        for i in 0..p.len() - 1 {
            let p0 = p[i.saturating_sub(1)];
            let (p1, p2) = (p[i], p[i + 1]);
            let p3 = p[(i + 2).min(p.len() - 1)];
            let _ = write!(
                d,
                "C{} {} {} {} {} {}",
                n(p1.x + (p2.x - p0.x) / 6.0),
                n(p1.y + (p2.y - p0.y) / 6.0),
                n(p2.x - (p3.x - p1.x) / 6.0),
                n(p2.y - (p3.y - p1.y) / 6.0),
                n(p2.x),
                n(p2.y)
            );
        }
    } else {
        for q in &p[1..] {
            let _ = write!(d, "L{} {}", n(q.x), n(q.y));
        }
    }
    let dash = match s.style {
        LineStyle::Dotted => " stroke-dasharray=\"1.5 3\"",
        LineStyle::Dashed => " stroke-dasharray=\"6 4\"",
        _ => "",
    };
    let _ = write!(out, "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{dash}/>", ink(s.color), n(stroke_width(s.thickness)));
    if s.arrow {
        let (a, bb) = (p[p.len() - 2], p[p.len() - 1]);
        let (dx, dy) = (bb.x - a.x, bb.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 {
            let (ux, uy) = (dx / len, dy / len);
            let _ = write!(
                out,
                "<path d=\"M{} {}L{} {}L{} {}z\" fill=\"{}\"/>",
                n(bb.x),
                n(bb.y),
                n(bb.x - 8.0 * ux + 3.5 * uy),
                n(bb.y - 8.0 * uy - 3.5 * ux),
                n(bb.x - 8.0 * ux - 3.5 * uy),
                n(bb.y - 8.0 * uy + 3.5 * ux),
                ink(s.color)
            );
        }
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------- GNT index

/// One of Randy Leedy's Greek New Testament diagrams, `gntdgm\<Book>_<CC>.<VV>[-<VV>].dgm`.
#[derive(Clone, Debug, PartialEq)]
pub struct GntDiagram {
    pub path: PathBuf,
    /// File stem, e.g. `1Co_01.01-09`.
    pub name: String,
    /// BibleWorks book code (`Mat`, `1Co` ...), as in `canon`.
    pub book: String,
    pub chapter: u16,
    pub verse: u16,
    /// Last chapter and verse covered; the start for a single verse, a later chapter for a
    /// passage like `Act_21.40-22.05`.
    pub end_chapter: u16,
    pub end_verse: u16,
    /// A letter after a verse number (`2Pe_02.01-10a`, `2Pe_02.10b-16`) means the diagram starts or
    /// ends part-way through that verse.
    pub start_part: Option<char>,
    pub end_part: Option<char>,
}

impl GntDiagram {
    /// Whether verse `chapter:verse` of `book` falls inside this diagram's passage.
    pub fn covers(&self, book: &str, chapter: u32, verse: u32) -> bool {
        self.book == book && (chapter, verse) >= (self.chapter as u32, self.verse as u32) && (chapter, verse) <= (self.end_chapter as u32, self.end_verse as u32)
    }

    pub fn load(&self) -> Result<Diagram, String> {
        Diagram::open(&self.path)
    }

    /// The passage notes (`<name>.nt.rtf`, RTF in the Bwgrkl font), when the diagram has any.
    pub fn notes_path(&self) -> Option<PathBuf> {
        let p = self.path.with_file_name(format!("{}.nt.rtf", self.name));
        p.is_file().then_some(p)
    }
}

fn verse_part(s: &str) -> Option<(u16, Option<char>)> {
    let (digits, rest) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let mut rest = rest.chars();
    let part = rest.next();
    if rest.next().is_some() || part.is_some_and(|c| !c.is_ascii_lowercase()) {
        return None;
    }
    Some((digits.parse().ok()?, part))
}

/// `1Co_01.01-09`, `Act_21.40-22.05`, `Phi_01.18c-26`, `1Co_04.21` -> its parts.
fn parse_gnt_name(stem: &str) -> Option<GntDiagram> {
    let (book, rest) = stem.split_once('_')?;
    if book.chars().count() != 3 {
        return None;
    }
    let (from, to) = match rest.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (ch, v) = from.split_once('.')?;
    let (chapter, (verse, sp)) = (ch.parse().ok()?, verse_part(v)?);
    let (ech, (ev, ep)) = match to {
        None => (chapter, (verse, sp)),
        Some(t) => match t.split_once('.') {
            Some((c, v)) => (c.parse().ok()?, verse_part(v)?),
            None => (chapter, verse_part(t)?),
        },
    };
    Some(GntDiagram { path: PathBuf::new(), name: stem.to_string(), book: book.to_string(), chapter, verse, end_chapter: ech, end_verse: ev, start_part: sp, end_part: ep })
}

/// Every diagram in `<install>\gntdgm`, in canonical order (book, chapter, verse, then part).
pub fn gnt_index(install: &Path) -> Vec<GntDiagram> {
    let Ok(rd) = fs::read_dir(install.join("gntdgm")) else { return Vec::new() };
    let mut v: Vec<GntDiagram> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            if !path.extension()?.eq_ignore_ascii_case("dgm") {
                return None;
            }
            let name = path.file_stem()?.to_str()?;
            Some(GntDiagram { path: path.clone(), ..parse_gnt_name(name)? })
        })
        .collect();
    v.sort_by(|a, b| {
        canon::canon_order(&a.book)
            .cmp(&canon::canon_order(&b.book))
            .then(a.book.cmp(&b.book))
            .then((a.chapter, a.verse, a.start_part).cmp(&(b.chapter, b.verse, b.start_part)))
            .then((a.end_chapter, a.end_verse, a.end_part).cmp(&(b.end_chapter, b.end_verse, b.end_part)))
    });
    v
}

/// The diagrams of `index` (from `gnt_index`) that include a verse; two when a verse is split
/// between diagrams (`2Pe 2:10`).
pub fn gnt_find<'a>(index: &'a [GntDiagram], book: &str, chapter: u32, verse: u32) -> Vec<&'a GntDiagram> {
    index.iter().filter(|g| g.covers(book, chapter, verse)).collect()
}

/// The example diagrams in `<install>\diagram`, sorted by name.
pub fn example_files(install: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(install.join("diagram")) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dgm"))).collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{install_dir, module::Module};

    fn nfc(s: &str) -> String {
        s.nfc().collect()
    }

    /// A small hand-made diagram (Subject-Verb plus a text box), gzipped, so the container code is
    /// tested without the install.
    const MINI_GZ: &str = concat!(
        "1f8b080000000000020ad555df6fda30107eafd4ffe124a43e6d5d9cb0fe98a66940529a89b2aa41a57baa12e205b7c68e6243617ffd6c27509b7574dada87893cf8bbb3bfefeecc9d5badb07f71b6bfd76a2d7005e8d0d3cb119114c3056144a3304a7a57f1e528fe3adcdf4be100326d8d86e1ade569b5b4b14c0b3c26b99cc2f1e1fbb5e11c93622a017935f979d409a3ab5b755e23b5bbc0b28b0bc2342c480e482fe4aac410f6c7fdd1ed380efbd1c844424449d3d5309d6148e6d91d9ec8b7d7b8ca1e896c97ebe9d154886d979817051612e7c9245519a3c3c084f8a05388f3251834e194571b2428977d15a56f8340034a18d6a06d83631b20ef319e88e5bfc9dff7b60a30e874a381e149334c8d38fa8568a07d2ecf6321255eca8c2f35e6a51437dfc6e7e0a970f4e7d5797de74c42a722293589911fb8c38aa404e483d903c85ca8666a54bef02983e0033a828f079fd63e3b1a27c54b4e989ba7e15baea0adc330319432163dce98ba21c2993901ab2d8a67488f1a52c5f882ac27afc27aba663d794956845e2558e45bb4477fc73a507de0909a56117816e720504d89367f8d52fa9b1bd58d1687cb7a3698f64ce48a62d395e0b90abb258f2d49df923cb1244f6d49ff9f259167e920640921df56f29e520a762825eaa0dbf21bba668ceaf5484f00510f3e6d4b292918f4ead1666e0fe735df9857b9995381abb15b34b0447d4774d18cd93f506cef50d49bdc8206db79127103c62d6405d7f154f0f9bbbb065ff0aa9cc267c666a26c4c67bc9a39db68ca0af376f42b8cefd7f3d058ba0f45754fd7a6440dc6e60ed511f566984bcb38cd6b7d2255b2937a3d6739aef4ecafa1fb8e64f7bd35ae27ab0a4b3d4b4275127487e6b9cb268b1202f55323b6ed96e399fab477d687e23705266e7516a44c03619747ef7af82f6af3fe89dafc041602823bd8080000"
    );

    fn mini() -> Vec<u8> {
        (0..MINI_GZ.len() / 2).map(|i| u8::from_str_radix(&MINI_GZ[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    /// Just enough XML checking to catch broken output: tags nest, attributes are quoted, `&` starts
    /// a known entity, no stray `<` or `>`.
    fn check_xml(s: &str) -> Result<(), String> {
        let mut stack: Vec<&str> = Vec::new();
        let mut rest = s;
        let mut roots = 0;
        while let Some(i) = rest.find('<') {
            let text = &rest[..i];
            for (k, _) in text.match_indices('&') {
                let t = &text[k..];
                if !["&amp;", "&lt;", "&gt;", "&quot;", "&apos;", "&#"].iter().any(|e| t.starts_with(e)) {
                    return Err(format!("bad entity near {:?}", t.chars().take(12).collect::<String>()));
                }
            }
            if text.contains('>') {
                return Err("stray > in text".into());
            }
            let end = rest[i..].find('>').ok_or("unterminated tag")? + i;
            let tag = &rest[i + 1..end];
            if tag.contains('<') || tag.matches('"').count() % 2 != 0 {
                return Err(format!("bad tag <{tag}>"));
            }
            if let Some(name) = tag.strip_prefix('/') {
                if stack.pop() != Some(name.trim()) {
                    return Err(format!("mismatched </{name}>"));
                }
            } else if !tag.starts_with('?') && !tag.starts_with('!') {
                let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
                if name.is_empty() {
                    return Err("empty tag name".into());
                }
                if stack.is_empty() {
                    roots += 1;
                }
                if !tag.ends_with('/') {
                    stack.push(name);
                }
            }
            rest = &rest[end + 1..];
        }
        if !stack.is_empty() || roots != 1 {
            return Err(format!("unclosed {stack:?}, {roots} roots"));
        }
        Ok(())
    }

    fn viewbox(svg: &str) -> [f32; 4] {
        let i = svg.find("viewBox=\"").unwrap() + 9;
        let v: Vec<f32> = svg[i..i + svg[i..].find('"').unwrap()].split(' ').map(|t| t.parse().unwrap()).collect();
        [v[0], v[1], v[2], v[3]]
    }

    #[test]
    fn decodes_bwgrkl_encoding() {
        for (raw, want) in [
            ("VIhsou/j", "Ἰησοῦς"),
            ("avpekri,qh", "ἀπεκρίθη"),
            ("o`", "ὁ"),
            ("i[na", "ἵνα"),
            ("auvtw/|", "αὐτῷ"),
            ("auvtw|/", "αὐτῷ"),
            ("~Hrw,|dhj", "Ἡρῴδης"),
            ("$h=n%", "(ἦν)"),
            ("ei=pen", "εἶπεν"),
            ("kaqw,j", "καθώς"),
            ("avllV", "ἀλλ’"),
            ("{Ellhnej", "Ἕλληνες"),
            ("e;ch|", "ἔχῃ"),
            ("ou-toj", "οὗτος"),
            ("o]", "ὃ"),
            ("a'n", "ἂν"),
            ("e`wra,kamen", "ἑωράκαμεν"),
            ("Mwu?sh/j", "Μωϋσῆς"),
            ("prau<thj", "πραΰτης"),
            ("r`h/ma", "ῥῆμα"),
            ("Îlo,goijÐ", "[λόγοις]"),
            ("to,pw|Å", "τόπῳ."),
            ("Cristo,j Qeo,j Yalmo,j", "Χριστός Θεός Ψαλμός"),
            ("+Asson", "Ἆσσον"),
            ("", ""),
        ] {
            assert_eq!(bwgrkl_to_unicode(raw), nfc(want), "{raw}");
        }
        // unknown or stray marks never panic
        let _ = bwgrkl_to_unicode("VVV~~,,//J{}|||");
    }

    #[test]
    fn parses_gnt_file_names() {
        let p = |s: &str| parse_gnt_name(s).map(|g| (g.book, g.chapter, g.verse, g.start_part, g.end_chapter, g.end_verse, g.end_part));
        assert_eq!(p("1Co_01.01-09"), Some(("1Co".into(), 1, 1, None, 1, 9, None)));
        assert_eq!(p("1Co_04.21"), Some(("1Co".into(), 4, 21, None, 4, 21, None)));
        assert_eq!(p("Act_21.40-22.05"), Some(("Act".into(), 21, 40, None, 22, 5, None)));
        assert_eq!(p("Phi_01.18c-26"), Some(("Phi".into(), 1, 18, Some('c'), 1, 26, None)));
        assert_eq!(p("2Pe_02.01-10a"), Some(("2Pe".into(), 2, 1, None, 2, 10, Some('a'))));
        assert_eq!(p("_dgmIndex"), None);
        assert_eq!(p("Joh_xx.01"), None);
        let g = GntDiagram {
            path: PathBuf::new(),
            name: String::new(),
            book: "Act".into(),
            chapter: 21,
            verse: 40,
            end_chapter: 22,
            end_verse: 5,
            start_part: None,
            end_part: None,
        };
        assert!(g.covers("Act", 21, 40) && g.covers("Act", 21, 45) && g.covers("Act", 22, 5));
        assert!(!g.covers("Act", 21, 39) && !g.covers("Act", 22, 6) && !g.covers("Mar", 21, 45));
    }

    #[test]
    fn reads_a_small_gzipped_diagram() {
        let d = parse(&mini()).unwrap();
        assert_eq!((d.title.as_str(), d.description.as_str()), ("Mini", "a & b"));
        assert_eq!(d.items.len(), 2);
        let Item::Symbol(s) = &d.items[0] else { panic!("symbol first") };
        assert_eq!((s.kind.as_str(), s.lines.len(), s.slots.len()), ("SubjectVerb", 3, 2));
        assert_eq!(s.slots[0].kind, "subject");
        assert_eq!(s.slots[0].words[0].text, nfc("Ἰησοῦς"));
        assert_eq!(s.slots[1].words[0].lemma, "λέγω");
        assert_eq!(s.slots[1].words[0].morph, "vipa3s");
        assert_eq!(s.lines[0].slot, Some(0));
        assert_eq!(s.lines[2].slot, None);
        assert_eq!(s.lines[2].style, LineStyle::Double);
        let r = s.slots[0].words[0].reference.clone().unwrap();
        assert_eq!((r.nt_book_code(), r.chapter, r.verse, r.word), (Some("Joh"), 3, 16, 4));
        assert_eq!(d.text(), "Ἰησοῦς λέγει");
        let svg = d.to_svg();
        check_xml(&svg).unwrap();
        assert!(svg.contains(">Ἰησοῦς</tspan>") && svg.contains("John 3:16 &lt;&amp;&gt;") && svg.contains("data-ref=\"3.3.16.4\""), "{svg}");
        // the same document uncompressed
        assert_eq!(parse(&gunzip(&mini()).unwrap()).unwrap(), d);
    }

    #[test]
    fn odd_input_is_an_error_not_a_panic() {
        assert!(parse(b"").is_err());
        assert!(parse(b"hello").is_err());
        assert!(parse(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(parse(b"##DGMF\r\n##ver 1.0\r\n").is_err()); // header only
        let good = mini();
        // every truncation, and a flipped byte at every position, in the gzip and in the text
        for cut in 0..good.len() {
            let _ = parse(&good[..cut]);
        }
        for i in 0..good.len() {
            let mut b = good.clone();
            b[i] ^= 0x5a;
            let _ = parse(&b);
        }
        let text = gunzip(&good).unwrap();
        for cut in (0..text.len()).step_by(7) {
            let _ = parse(&text[..cut]).map(|d| d.to_svg());
        }
        for i in (0..text.len()).step_by(3) {
            let mut b = text.clone();
            b[i] = b'#';
            if let Ok(d) = parse(&b) {
                let _ = d.to_svg();
            }
        }
        // an empty diagram still renders
        check_xml(&Diagram::default().to_svg()).unwrap();
    }

    #[test]
    fn example_diagrams_have_the_expected_symbols() {
        let Some(dir) = install_dir() else { return };
        let d = Diagram::open(dir.join("diagram/example_subject-verb.dgm")).unwrap();
        let symbols: Vec<&Symbol> = d.items.iter().filter_map(|i| if let Item::Symbol(s) = i { Some(s) } else { None }).collect();
        let kinds: Vec<&str> = symbols.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, ["SubjectVerb", "SubjectVerb", "SubjectVerb", "predicateNominative", "Adjective-below"]);
        assert_eq!(d.words().iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["I", "ran", "He", "is", "friend", "my"]);
        // "I ran": subject line, verb line, then the stilts above and below the join
        let sv = symbols[1];
        assert_eq!(sv.lines.len(), 4);
        assert_eq!((sv.lines[0].a, sv.lines[0].b), (Pt { x: 68.0, y: 226.0 }, Pt { x: 122.0, y: 226.0 }));
        let svg = d.to_svg();
        check_xml(&svg).unwrap();
        // the subject sits centred above its baseline
        assert!(svg.contains("<text x=\"95\" y=\"221.5\" text-anchor=\"middle\""), "{svg}");
    }

    #[test]
    fn gnt_index_covers_the_new_testament() {
        let Some(dir) = install_dir() else { return };
        let idx = gnt_index(&dir);
        assert!(idx.len() >= 990, "{} diagrams", idx.len());
        let order: Vec<_> = idx.iter().map(|g| (canon::canon_order(&g.book), g.chapter, g.verse)).collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]), "not in canonical order");
        assert_eq!(idx[0].book, "Mat");
        assert_eq!(idx.last().unwrap().book, "Rev");
        // John 3:16 and Romans 8:28 are covered by one diagram each
        let names = |v: Vec<&GntDiagram>| v.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(gnt_find(&idx, "Joh", 3, 16)), ["Joh_03.09-18"]);
        assert_eq!(names(gnt_find(&idx, "Rom", 8, 28)), ["Rom_08.22-32"]);
        assert_eq!(gnt_find(&idx, "Act", 22, 3).len(), 1); // inside Act_21.40-22.05
        assert_eq!(gnt_find(&idx, "2Pe", 2, 10).len(), 2); // 2Pe_02.01-10a and 2Pe_02.10b-16
        assert!(gnt_find(&idx, "Gen", 1, 1).is_empty());
        // each of the 27 books has diagrams, and every file in the folder was indexed once
        for b in &canon::BOOKS[39..] {
            assert!(idx.iter().any(|g| g.book == b.code), "{}", b.name);
        }
        let mut n: Vec<_> = idx.iter().map(|g| &g.name).collect();
        n.dedup();
        assert_eq!(n.len(), idx.len());
        let files = fs::read_dir(dir.join("gntdgm")).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == "dgm")).count();
        assert_eq!(files, idx.len());
        assert!(idx.iter().any(|g| g.notes_path().is_some()));
        assert!(example_files(&dir).len() >= 24);
    }

    /// Word texts must be real Greek, agree with the passage notes and with the BNT text they were
    /// copied from.
    #[test]
    fn greek_matches_notes_and_bnt() {
        let Some(dir) = install_dir() else { return };
        let idx = gnt_index(&dir);
        let g = gnt_find(&idx, "Joh", 3, 16)[0];
        let d = g.load().unwrap();
        let text = d.text();
        assert!(text.starts_with(&nfc("ἀπεκρίθη Νικόδημος")), "{text}");
        let jn316: Vec<String> = d.words().iter().filter(|w| w.reference.as_ref().map_or(false, |r| r.verse == 16)).map(|w| w.text.clone()).collect();
        assert_eq!(jn316[..4], [nfc("οὕτως"), nfc("γάρ"), nfc("ἠγάπησεν"), nfc("ὁ")], "{jn316:?}");
        // the passage notes quote the diagram's own words in the same font encoding
        let rtf = latin1(&fs::read(g.notes_path().unwrap()).unwrap());
        assert!(rtf.contains("pisteu,wn") && d.words().iter().any(|w| w.raw == "pisteu,wn" && w.text == nfc("πιστεύων")));
        assert!(rtf.contains("evn auvtw|/") && d.words().iter().any(|w| w.text == nfc("αὐτῷ")));
        assert_eq!(bwgrkl_to_unicode("evn auvtw|/"), nfc("ἐν αὐτῷ"));

        // every referenced word of a spread of diagrams against the BNT text (the diagrams keep the
        // isolated form of a sentence-final word, `kai,` for BNT's `kai.`, so grave counts as acute)
        let Ok(bnt) = Module::open(dir.join("databases/bnt.dbx")) else { return };
        let marks = |c: &char| ('\u{300}'..='\u{36F}').contains(c);
        let exact = |s: &str| -> String {
            s.nfd().filter(|c| c.is_alphabetic() || marks(c)).map(|c| if c == '\u{300}' { '\u{301}' } else { c }).collect::<String>().to_lowercase().replace('ς', "σ")
        };
        let bare = |s: &str| -> String { s.nfd().filter(|c| c.is_alphabetic() && !marks(c)).collect::<String>().to_lowercase().replace('ς', "σ") };
        let (mut seen, mut same, mut letters) = (0, 0, 0);
        let mut misses: Vec<String> = Vec::new();
        for g in idx.iter().step_by(25) {
            let d = g.load().unwrap();
            for it in &d.items {
                let Item::Symbol(s) = it else { continue };
                for w in s.slots.iter().flat_map(|s| &s.words) {
                    let Some(r) = w.reference.as_ref().filter(|_| w.version == "BNT" && w.greek && !w.text.starts_with('(')) else { continue };
                    let Some(book) = r.nt_book_code().and_then(|c| bnt.vers.book_index(c)) else { continue };
                    let Some(verse) = bnt.vers.index(book, r.chapter as usize, r.verse as usize).and_then(|v| bnt.raw(v)) else { continue };
                    let toks: Vec<String> = verse.split_whitespace().map(crate::greek::greek).collect();
                    let Some(t) = toks.get(r.word as usize) else { continue };
                    seen += 1;
                    if exact(&w.text) == exact(t) {
                        same += 1;
                    }
                    if bare(&w.text) == bare(t) {
                        letters += 1;
                    } else if misses.len() < 12 {
                        misses.push(format!("{} {}.{}.{}.{}: diagram {} / bnt {} ({})", g.name, r.book, r.chapter, r.verse, r.word, w.text, t, w.raw));
                    }
                }
            }
        }
        eprintln!("bnt cross-check: {seen} words, {same} same accents, {letters} same letters; misses {misses:#?}");
        assert!(seen > 3000);
        assert!(letters * 100 >= seen * 97, "{letters}/{seen}: {misses:#?}");
        assert!(same * 100 >= seen * 90, "{same}/{seen}: {misses:#?}");
    }

    /// Every shipped diagram (examples and the GNT set) parses, and its SVG is well formed with
    /// finite, sensible geometry.
    #[test]
    fn all_installed_diagrams_parse_and_render() {
        let Some(dir) = install_dir() else { return };
        let mut files = example_files(&dir);
        files.extend(gnt_index(&dir).into_iter().map(|g| g.path));
        assert!(files.len() >= 1000, "{} files", files.len());
        let out = std::env::var_os("DGM_SVG_OUT").map(PathBuf::from);
        let (mut failures, mut words, mut symbols) = (Vec::new(), 0, 0);
        for f in &files {
            let d = match Diagram::open(f) {
                Ok(d) => d,
                Err(e) => {
                    failures.push(format!("{}: {e}", f.display()));
                    continue;
                }
            };
            let svg = d.to_svg();
            if let Err(e) = check_xml(&svg) {
                failures.push(format!("{}: svg {e}", f.display()));
            }
            for bad in ["NaN", "Linf", "Minf", "Cinf", "x=\"inf", "y=\"inf", "=\"-inf"] {
                if svg.contains(bad) {
                    failures.push(format!("{}: {bad} in svg", f.display()));
                }
            }
            let [x, y, w, h] = viewbox(&svg);
            if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 40.0 || h <= 40.0 || w > 20_000.0 || h > 40_000.0 {
                failures.push(format!("{}: viewBox {x} {y} {w} {h}", f.display()));
            }
            // everything drawn lies inside the viewBox
            for it in &d.items {
                let Item::Symbol(s) = it else { continue };
                symbols += 1;
                for l in s.lines.iter().filter(|l| l.visible()) {
                    for p in [l.a, l.b] {
                        if !(p.x >= x && p.x <= x + w && p.y >= y && p.y <= y + h) {
                            failures.push(format!("{}: point {p:?} outside viewBox", f.display()));
                        }
                    }
                }
            }
            words += d.words().len();
            if let Some(o) = &out {
                let name = f.file_stem().unwrap().to_string_lossy().replace(' ', "_");
                if name.starts_with("example_") || ["Joh_03.09-18", "Rom_08.22-32", "Mat_05.01-12", "Rom_08.28-30"].contains(&name.as_str()) {
                    let _ = fs::create_dir_all(o);
                    let _ = fs::write(o.join(format!("{name}.svg")), &svg);
                }
            }
        }
        eprintln!("{} diagrams, {symbols} symbols, {words} words", files.len());
        assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
        assert!(words > 100_000);
    }
}
