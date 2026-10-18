//! The Treasury of Scripture Knowledge (`tskdic.dat`): cross-references for every verse.
//!
//! One entry per KJV verse, New Testament first. An entry may open with the chapter outline
//! (`  1  The genealogy of Christ...`), then phrase blocks: ` * phrase` followed by ` # references`
//! in TSK's own abbreviations. References without a book belong to the verse's own book, a bare
//! number is a verse of the chapter last named, and `Jude 14` means chapter 1.

use crate::{canon, lexicon::Table, versification::Versification};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const NT_VERSES: usize = 7957;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TskRef {
    pub code: String,
    pub chapter: usize,
    pub verse: usize,
    /// Last verse of a range (`== verse` for a single verse).
    pub end: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Group {
    /// The words of the verse the references are for (may be empty).
    pub phrase: String,
    pub refs: Vec<TskRef>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Entry {
    /// Chapter outline lines: (first verse, summary). Only on the first verse of a chapter.
    pub outline: Vec<(usize, String)>,
    pub groups: Vec<Group>,
}

pub struct Tsk {
    table: Table,
    kjv: Versification,
}

/// Book code for a TSK abbreviation.
fn book(abbr: &str) -> Option<&'static str> {
    Some(match abbr {
        "Ge" => "Gen", "Ex" | "Exo" => "Exo", "Le" => "Lev", "Nu" => "Num", "De" => "Deu", "Jos" => "Jos", "Jud" => "Jdg", "Ru" => "Rut",
        "1Sa" => "1Sa", "2Sa" => "2Sa", "1Ki" => "1Ki", "2Ki" => "2Ki", "1Ch" => "1Ch", "2Ch" | "2ch" => "2Ch", "Ezr" => "Ezr", "Ne" => "Neh",
        "Es" => "Est", "Job" => "Job", "Ps" => "Psa", "Pr" => "Pro", "Ec" => "Ecc", "So" => "Sol", "Isa" | "Is" => "Isa", "Jer" => "Jer",
        "La" => "Lam", "Eze" => "Eze", "Da" => "Dan", "Ho" => "Hos", "Joe" => "Joe", "Am" => "Amo", "Ob" => "Oba", "Jon" => "Jon",
        "Mic" => "Mic", "Na" => "Nah", "Hab" => "Hab", "Zep" => "Zep", "Hag" => "Hag", "Zec" => "Zec", "Mal" => "Mal",
        "Mt" | "Mat" => "Mat", "Mr" | "Mar" => "Mar", "Lu" => "Luk", "Joh" | "joh" => "Joh", "Ac" => "Act", "Ro" => "Rom", "1Co" => "1Co",
        "2Co" => "2Co", "Ga" => "Gal", "Eph" => "Eph", "Php" => "Phi", "Col" => "Col", "1Th" => "1Th", "2Th" => "2Th", "1Ti" => "1Ti",
        "2Ti" => "2Ti", "Tit" => "Tit", "Phm" => "Phm", "Heb" => "Heb", "Jas" => "Jam", "1Pe" => "1Pe", "2Pe" => "2Pe", "1Jo" => "1Jo",
        "2Jo" => "2Jo", "3Jo" => "3Jo", "Jude" => "Jud", "Re" => "Rev",
        _ => return None,
    })
}

/// References in one `#` line. `code`/`chapter` are the context carried between lines.
fn parse_refs(text: &str, code: &mut String, chapter: &mut usize, out: &mut Vec<TskRef>) {
    for tok in text.split_whitespace() {
        if tok.starts_with('*') {
            continue; // editorial markers such as *etc: and *Gr:
        }
        if let Some(c) = book(tok) {
            *code = c.to_string();
            *chapter = 0;
            continue;
        }
        // "c:v,v-v" sets the chapter; a bare "v,v-v" stays in the chapter last named.
        let verses = match tok.split_once(':') {
            Some((c, v)) => match c.parse::<usize>() {
                Ok(c) => {
                    *chapter = c;
                    v
                }
                Err(_) => continue,
            },
            None => {
                if *chapter == 0 {
                    *chapter = 1; // a single-chapter book: "Jude 14"
                }
                tok
            }
        };
        for piece in verses.split(',') {
            let (a, b) = piece.split_once('-').unwrap_or((piece, piece));
            let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) else { continue };
            if a > 0 {
                // a damaged range such as "46:8-2" keeps just its first verse
                out.push(TskRef { code: code.clone(), chapter: *chapter, verse: a, end: if b >= a { b } else { a } });
            }
        }
    }
}

/// Parse one entry. `code` and `chapter` name the verse the entry belongs to.
pub fn parse_entry(text: &str, code: &str, chapter: usize) -> Entry {
    let mut e = Entry::default();
    let mut lines = text.lines();
    lines.next(); // the entry number
    let mut in_outline = true;
    let (mut ctx_code, mut ctx_chapter) = (code.to_string(), chapter);
    for line in lines {
        if in_outline {
            if line.trim().is_empty() {
                in_outline = false;
                continue;
            }
            if line.trim_start().starts_with(['*', '#']) {
                in_outline = false; // no outline: references start right away
            } else {
                let t = line.trim_start();
                let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
                match (digits.parse::<usize>(), t[digits.len()..].strip_prefix("  ")) {
                    (Ok(v), Some(rest)) if !digits.is_empty() => e.outline.push((v, rest.trim().to_string())),
                    _ => {
                        if let Some(last) = e.outline.last_mut() {
                            last.1.push(' ');
                            last.1.push_str(t.trim());
                        }
                    }
                }
                continue;
            }
        }
        if let Some(phrase) = line.strip_prefix(" * ") {
            e.groups.push(Group { phrase: phrase.trim().to_string(), refs: vec![] });
            (ctx_code, ctx_chapter) = (code.to_string(), chapter);
        } else if let Some(refs) = line.strip_prefix(" # ") {
            if e.groups.is_empty() {
                e.groups.push(Group::default());
            }
            let g = e.groups.last_mut().unwrap();
            parse_refs(refs, &mut ctx_code, &mut ctx_chapter, &mut g.refs);
        } else if let (Some(g), false) = (e.groups.last_mut(), line.trim().is_empty()) {
            if g.refs.is_empty() {
                g.phrase.push(' '); // a long note wraps onto following lines
                g.phrase.push_str(line.trim());
            }
        }
    }
    e
}

impl Tsk {
    pub fn open(databases: &Path) -> io::Result<Self> {
        Ok(Tsk { table: Table::open(databases.join("tskdic.dat"))?, kjv: Versification::kjv() })
    }

    /// Cross-references for a verse, given in KJV numbering. `None` when TSK has no entry for it.
    pub fn entry(&self, code: &str, chapter: usize, verse: usize) -> Option<Entry> {
        let book = self.kjv.book_index(code)?;
        let i = self.kjv.index(book, chapter, verse)?;
        let slot = if i >= canon::TOTAL_VERSES - NT_VERSES { i - (canon::TOTAL_VERSES - NT_VERSES) } else { i + NT_VERSES };
        let mut e = parse_entry(&self.table.get_sparse(slot)?, code, chapter);
        // keep only references to verses that exist
        for g in &mut e.groups {
            g.refs.retain(|r| self.kjv.book_index(&r.code).and_then(|b| self.kjv.index(b, r.chapter, r.verse)).is_some());
        }
        Some(e)
    }

    /// The outline of a chapter (kept on its first verse's entry).
    pub fn outline(&self, code: &str, chapter: usize) -> Vec<(usize, String)> {
        self.entry(code, chapter, 1).map(|e| e.outline).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_context_rules() {
        let mut out = Vec::new();
        let (mut code, mut ch) = ("Mat".to_string(), 1);
        parse_refs("9:27 15:22 22:42-45 2Sa 7:13,16 Ps 89:36", &mut code, &mut ch, &mut out);
        let s: Vec<String> = out.iter().map(|r| format!("{} {}:{}-{}", r.code, r.chapter, r.verse, r.end)).collect();
        assert_eq!(s, ["Mat 9:27-27", "Mat 15:22-22", "Mat 22:42-45", "2Sa 7:13-13", "2Sa 7:16-16", "Psa 89:36-36"]);

        // a bare number is a verse of the chapter named last; the verse's own chapter to begin with
        let (mut out, mut code, mut ch) = (Vec::new(), "Joh".to_string(), 3);
        parse_refs("15 Mt 9:13 1Ti 1:15,16", &mut code, &mut ch, &mut out);
        assert_eq!((out[0].code.as_str(), out[0].chapter, out[0].verse), ("Joh", 3, 15));
        assert_eq!((out[1].code.as_str(), out[1].chapter), ("Mat", 9));
        // single-chapter books, editorial markers and damaged ranges
        let (mut out, mut code, mut ch) = (Vec::new(), "Gen".to_string(), 1);
        parse_refs("Jude 14 *etc: Ge 46:8-2 *Gr: Zzz 1:1", &mut code, &mut ch, &mut out);
        let s: Vec<String> = out.iter().map(|r| format!("{} {}:{}-{}", r.code, r.chapter, r.verse, r.end)).collect();
        assert_eq!(s, ["Jud 1:14-14", "Gen 46:8-8", "Gen 1:1-1"]);
    }

    #[test]
    fn entry_layout() {
        let text = "00001\r\n  1  The genealogy of Christ from Abraham to Joseph.\r\n 18  He is miraculously conceived of the Holy Ghost by the Virgin\r\n     Mary.\r\n\r\n * generation.\r\n # Ge 2:4 5:1 Lu 3:23-38\r\n * a long note that\r\n wraps here.\r\n # Isa 5:18,19\r\n";
        let e = parse_entry(text, "Mat", 1);
        assert_eq!(e.outline.len(), 2);
        assert_eq!(e.outline[1].0, 18);
        assert!(e.outline[1].1.ends_with("Virgin Mary."));
        assert_eq!(e.groups.len(), 2);
        assert_eq!(e.groups[0].phrase, "generation.");
        assert_eq!(e.groups[0].refs.len(), 3); // Ge 2:4, Ge 5:1, Lu 3:23-38
        assert_eq!(e.groups[1].phrase, "a long note that wraps here.");
    }

    #[test]
    fn reads_the_real_treasury() {
        let Some(dir) = crate::install_dir() else { return };
        let tsk = Tsk::open(&dir.join("databases")).unwrap();
        let has = |g: &Group, c: &str, ch: usize, v: usize| g.refs.iter().any(|r| r.code == c && r.chapter == ch && r.verse <= v && v <= r.end);

        // Genesis 1:1 ("beginning" points to John 1:1-3), Old Testament entries sit after the New
        let e = tsk.entry("Gen", 1, 1).unwrap();
        assert!(e.outline.iter().any(|(v, t)| *v == 1 && t.starts_with("God creates")));
        let beginning = e.groups.iter().find(|g| g.phrase.starts_with("beginning")).unwrap();
        assert!(has(beginning, "Joh", 1, 2), "{beginning:?}");

        // John 3:16: "that whosoever" -> John 3:15 (bare number) and Matthew 9:13
        let e = tsk.entry("Joh", 3, 16).unwrap();
        let g = e.groups.iter().find(|g| g.phrase.starts_with("that whosoever")).unwrap();
        assert!(has(g, "Joh", 3, 15) && has(g, "Mat", 9, 13), "{g:?}");
        assert!(e.groups.iter().any(|g| has(g, "Rom", 5, 8)));

        // Matthew 1:1 (the very first slot) has an outline and "the son of David" references
        let e = tsk.entry("Mat", 1, 1).unwrap();
        assert_eq!(e.outline.len(), 3);
        assert!(e.groups.iter().any(|g| has(g, "2Sa", 7, 13)));

        // Revelation 22:21 exists (last NT slot) and chapter outlines work for any verse
        assert!(tsk.entry("Rev", 22, 21).is_some());
        assert!(!tsk.outline("Joh", 3).is_empty());

        // most verses have an entry; the few that don't are simply None, never garbage
        let (mut have, mut missing) = (0, 0);
        for i in 0..canon::TOTAL_VERSES {
            let (b, c, v) = tsk.kjv.locate(i).unwrap();
            match tsk.entry(&tsk.kjv.books[b].code, c, v) {
                Some(e) => {
                    have += 1;
                    assert!(e.groups.iter().all(|g| g.refs.iter().all(|r| r.verse >= 1 && r.end >= r.verse)), "{c}:{v}");
                }
                None => missing += 1,
            }
        }
        println!("TSK entries: {have} present, {missing} empty");
        assert!(have > 30000, "{have} {missing}");
    }
}
