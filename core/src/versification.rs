//! Book/chapter/verse structure of a module. A module's verse `i` is its i-th text record;
//! the versification says which (book, chapter, verse) that is.
//!
//! Three sources: the KJV table (`canon`), the Hebrew master table (read from BW800.exe), and
//! the structure block most modules carry after their declared end (`from_tail`).

use crate::{canon, hebrew_table::HEBREW_VERSES};

#[derive(Clone, Debug)]
pub struct VBook {
    pub code: String,
    pub chapters: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct Versification {
    pub books: Vec<VBook>,
    starts: Vec<usize>,
    total: usize,
}

/// Chapters per book in the Hebrew OT (Joel has 4, Malachi 3).
const HEBREW_CHAPTERS: [usize; 39] = [
    50, 40, 27, 36, 34, 24, 21, 4, 31, 24, 22, 25, 29, 36, 10, 13, 10, 42, 150, 31, 12, 8, 66, 52, 5, 48, 12, 14, 4, 9, 1, 4, 7, 3, 3, 3,
    2, 14, 3,
];

impl Versification {
    fn new(books: Vec<VBook>) -> Self {
        let mut starts = Vec::with_capacity(books.len());
        let mut total = 0;
        for b in &books {
            starts.push(total);
            total += b.chapters.iter().map(|&v| v as usize).sum::<usize>();
        }
        Versification { books, starts, total }
    }

    pub fn kjv() -> Self {
        Self::new(canon::BOOKS.iter().map(|b| VBook { code: b.code.to_string(), chapters: b.chapters.iter().map(|&v| v as u16).collect() }).collect())
    }

    /// The WTT master structure (Old Testament, Hebrew numbering).
    pub fn hebrew() -> Self {
        let mut at = 0;
        let books = HEBREW_CHAPTERS
            .iter()
            .zip(canon::BOOKS.iter())
            .map(|(&n, kb)| {
                let chapters = HEBREW_VERSES[at..at + n].iter().map(|&v| v as u16).collect();
                at += n;
                VBook { code: kb.code.to_string(), chapters }
            })
            .collect();
        Self::new(books)
    }

    /// Parse the structure block at the end of a .dbx file (everything after the declared size):
    /// u32 chapters, u32 verses, u16 books, u16 pad, 3-byte book codes, u16 chapters per book,
    /// u16 verses per chapter. Totals must agree or it's rejected.
    pub fn from_tail(t: &[u8]) -> Option<Self> {
        let u16at = |p: usize| t.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let u32at = |p: usize| t.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let (nch, nv, nb) = (u32at(0)?, u32at(4)?, u16at(8)?);
        if nb == 0 || nb > 200 || nch > 5000 {
            return None;
        }
        let mut p = 12;
        let codes: Vec<String> = (0..nb).map(|i| t.get(p + i * 3..p + i * 3 + 3).map(|b| b.iter().map(|&c| c as char).collect())).collect::<Option<_>>()?;
        p += nb * 3;
        let per_book: Vec<usize> = (0..nb).map(|i| u16at(p + i * 2)).collect::<Option<_>>()?;
        p += nb * 2;
        if per_book.iter().sum::<usize>() != nch {
            return None;
        }
        let mut books = Vec::with_capacity(nb);
        for (code, n) in codes.into_iter().zip(per_book) {
            let chapters: Vec<u16> = (0..n).map(|i| u16at(p + i * 2).map(|v| v as u16)).collect::<Option<_>>()?;
            p += n * 2;
            books.push(VBook { code, chapters });
        }
        let v = Self::new(books);
        (v.total == nv).then_some(v)
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn book_index(&self, code: &str) -> Option<usize> {
        self.books.iter().position(|b| b.code == code)
    }

    /// Verse index of (book, chapter, verse); book is 0-based, chapter and verse 1-based.
    pub fn index(&self, book: usize, chapter: usize, verse: usize) -> Option<usize> {
        let b = self.books.get(book)?;
        if chapter == 0 || chapter > b.chapters.len() || verse == 0 || verse > b.chapters[chapter - 1] as usize {
            return None;
        }
        let in_book: usize = b.chapters[..chapter - 1].iter().map(|&v| v as usize).sum();
        Some(self.starts[book] + in_book + verse - 1)
    }

    /// Inverse of [`index`](Self::index).
    pub fn locate(&self, i: usize) -> Option<(usize, usize, usize)> {
        if i >= self.total {
            return None;
        }
        let book = self.starts.partition_point(|&s| s <= i) - 1;
        let mut left = i - self.starts[book];
        for (ci, &n) in self.books[book].chapters.iter().enumerate() {
            if left < n as usize {
                return Some((book, ci + 1, left + 1));
            }
            left -= n as usize;
        }
        None
    }

    pub fn chapter_range(&self, book: usize, chapter: usize) -> Option<std::ops::Range<usize>> {
        let start = self.index(book, chapter, 1)?;
        Some(start..start + self.books[book].chapters[chapter - 1] as usize)
    }

    /// Previous and next (book, chapter) across book boundaries.
    pub fn neighbors(&self, book: usize, chapter: usize) -> (Option<(usize, usize)>, Option<(usize, usize)>) {
        let prev = if chapter > 1 { Some((book, chapter - 1)) } else { book.checked_sub(1).map(|b| (b, self.books[b].chapters.len())) };
        let next = if chapter < self.books[book].chapters.len() { Some((book, chapter + 1)) } else { (book + 1 < self.books.len()).then(|| (book + 1, 1)) };
        (prev, next)
    }

    /// Parse "John 3:16", "1 Co 13", "Gen" (case-insensitive code or name prefix); chapter/verse default to 1.
    pub fn parse_ref(&self, s: &str) -> Option<(usize, usize, usize)> {
        let (bi, ch, v) = self.parse_parts(s)?;
        let (ch, v) = (ch.unwrap_or(1), v.unwrap_or(1));
        self.index(bi, ch, v).map(|_| (bi, ch, v))
    }

    /// Book index plus the chapter and verse if they were written.
    fn parse_parts(&self, s: &str) -> Option<(usize, Option<usize>, Option<usize>)> {
        let s = s.trim();
        let split = s.rfind(|c: char| c.is_alphabetic() || c == '.')? + 1;
        let (name, nums) = s.split_at(split);
        let name = norm(name);
        if name.is_empty() {
            return None;
        }
        let names: Vec<String> = self.books.iter().map(|b| norm(&canon::book_name(&b.code))).collect();
        let bi = self
            .books
            .iter()
            .position(|b| norm(&b.code) == name)
            .or_else(|| names.iter().position(|n| *n == name))
            .or_else(|| names.iter().position(|n| n.starts_with(&name)))?;
        let mut it = nums.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
        let ch = match it.next() {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        let v = match it.next() {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        Some((bi, ch, v))
    }

    /// Verse indices of a whole book.
    pub fn book_range(&self, book: usize) -> Option<std::ops::Range<usize>> {
        let start = *self.starts.get(book)?;
        Some(start..start + self.books[book].chapters.iter().map(|&v| v as usize).sum::<usize>())
    }

    /// One reference as the verses it covers: a book, a chapter, or a single verse.
    fn unit(&self, s: &str) -> Option<std::ops::Range<usize>> {
        match self.parse_parts(s)? {
            (b, None, _) => self.book_range(b),
            (b, Some(c), None) => self.chapter_range(b, c),
            (b, Some(c), Some(v)) => self.index(b, c, v).map(|i| i..i + 1),
        }
    }

    /// A passage as a verse range: "Jam", "Jam 3", "Jam 3:13", "Eph 2:8-10", "Jam 3-5", "Jam 3:13 - Jam 5:20".
    pub fn parse_span(&self, s: &str) -> Option<std::ops::Range<usize>> {
        let s = s.trim().replace(['–', '—'], "-");
        let joined = |a: &str, b: &str| {
            let (a, b) = (self.unit(a.trim())?, self.unit(b.trim())?);
            (a.start < b.end).then(|| a.start..b.end)
        };
        if let Some((a, b)) = s.split_once(" - ") {
            return joined(a, b);
        }
        if let Some((a, b)) = s.rsplit_once('-') {
            let b = b.trim();
            if let (false, true, Ok(n)) = (b.is_empty(), b.chars().all(|c| c.is_ascii_digit()), b.parse::<usize>()) {
                let first = self.unit(a.trim())?;
                let (book, chapter, _) = self.locate(first.start)?;
                return if a.contains(':') {
                    let end = self.index(book, chapter, n)?;
                    (first.start <= end).then(|| first.start..end + 1)
                } else {
                    let last = self.chapter_range(book, n)?; // "Jam 3-5"
                    (first.start < last.end).then(|| first.start..last.end)
                };
            }
            return joined(a, b);
        }
        self.unit(&s)
    }
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kjv_roundtrip_and_parse() {
        let v = Versification::kjv();
        assert_eq!(v.total(), canon::TOTAL_VERSES);
        assert_eq!(v.index(0, 1, 1), Some(0));
        assert_eq!(v.locate(v.index(42, 3, 16).unwrap()), Some((42, 3, 16)));
        assert_eq!(v.locate(v.total() - 1), Some((65, 22, 21)));
        assert_eq!(v.parse_ref("john 3:16"), Some((42, 3, 16)));
        assert_eq!(v.parse_ref("1 Co 13"), Some((45, 13, 1)));
        assert_eq!(v.parse_ref("Ps 119:176").map(|r| r.0), Some(18));
        assert_eq!(v.parse_ref("Song 2:1"), Some((21, 2, 1)));
        assert_eq!(v.parse_ref("Gen 1:99"), None);
    }

    #[test]
    fn spans() {
        let v = Versification::kjv();
        let at = |c: &str, ch, vs| v.index(v.book_index(c).unwrap(), ch, vs).unwrap();
        assert_eq!(v.parse_span("Jam 3:13 - Jam 5:20"), Some(at("Jam", 3, 13)..at("Jam", 5, 20) + 1));
        assert_eq!(v.parse_span("Eph 2:8-10"), Some(at("Eph", 2, 8)..at("Eph", 2, 10) + 1));
        assert_eq!(v.parse_span("Jam 3-5"), Some(at("Jam", 3, 1)..at("Jam", 5, 20) + 1));
        assert_eq!(v.parse_span("jam").map(|r| r.len()), Some(108));
        assert_eq!(v.parse_span("Joh 3").map(|r| r.len()), Some(36));
        assert_eq!(v.parse_span("Joh 3:16").map(|r| r.len()), Some(1));
        assert_eq!(v.parse_span("Eph 2:10-8"), None); // backwards
        assert_eq!(v.parse_span("nonsense"), None);
    }

    #[test]
    fn hebrew_master_matches_wtt() {
        let h = Versification::hebrew();
        assert_eq!(h.total(), 23_213);
        assert_eq!(h.books.len(), 39);
        let joel = h.book_index("Joe").unwrap();
        assert_eq!(h.books[joel].chapters.len(), 4); // KJV has 3
        assert_eq!(h.books[h.book_index("Num").unwrap()].chapters.iter().sum::<u16>(), 1289);
        assert_eq!(h.books[h.book_index("Neh").unwrap()].chapters.iter().sum::<u16>(), 405);
        // Psalm 3 has a titled verse 1 in Hebrew numbering: 9 verses vs KJV's 8.
        assert_eq!(h.books[h.book_index("Psa").unwrap()].chapters[2], 9);
    }

    #[test]
    fn tail_roundtrip() {
        // 2 books (Mat: 2 chapters of 3,2 verses; Mar: 1 chapter of 4 verses)
        let mut t = Vec::new();
        t.extend(3u32.to_le_bytes());
        t.extend(9u32.to_le_bytes());
        t.extend(2u16.to_le_bytes());
        t.extend(0u16.to_le_bytes());
        t.extend(b"MatMar");
        t.extend([2u8, 0, 1, 0]);
        t.extend([3u8, 0, 2, 0, 4, 0]);
        let v = Versification::from_tail(&t).unwrap();
        assert_eq!(v.total(), 9);
        assert_eq!(v.locate(5), Some((1, 1, 1)));
        t[4] = 10; // wrong verse total is rejected
        assert!(Versification::from_tail(&t).is_none());
    }
}
