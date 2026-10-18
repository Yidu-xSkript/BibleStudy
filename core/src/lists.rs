//! Verse lists: named lists of verses, each tagged with the version (numbering) it came from,
//! as in BibleWorks' Verse List Manager. Hebrew, Greek and English numbering differ in places, so
//! the tag keeps "Psalm 51:3" from silently meaning a different verse.

use crate::{canon::canon_order, versification::Versification};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, ops::Range};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ListRef {
    /// Version whose numbering this reference uses (lowercase id, e.g. `kjv`).
    pub ns: String,
    pub code: String,
    pub chapter: usize,
    pub verse: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VerseList {
    pub id: String,
    pub name: String,
    pub description: String,
    pub refs: Vec<ListRef>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct VerseLists {
    pub lists: Vec<VerseList>,
    next: u64,
}

impl VerseLists {
    pub fn create(&mut self, name: &str, refs: Vec<ListRef>) -> &mut VerseList {
        self.next += 1;
        let name = if name.trim().is_empty() { format!("List {}", self.next) } else { name.trim().to_string() };
        self.lists.push(VerseList { id: format!("v{}", self.next), name, description: String::new(), refs });
        self.lists.last_mut().unwrap()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut VerseList> {
        self.lists.iter_mut().find(|l| l.id == id)
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let n = self.lists.len();
        self.lists.retain(|l| l.id != id);
        self.lists.len() != n
    }
}

fn sort_key(r: &ListRef) -> (usize, usize, usize, &str) {
    (canon_order(&r.code), r.chapter, r.verse, r.ns.as_str())
}

/// Canonical order, duplicates removed.
pub fn sort_dedupe(refs: &mut Vec<ListRef>) {
    refs.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
    refs.dedup();
}

/// Append `add` to `refs`, skipping verses already present (order kept).
pub fn append_unique(refs: &mut Vec<ListRef>, add: Vec<ListRef>) {
    let mut seen: HashSet<ListRef> = refs.iter().cloned().collect();
    refs.extend(add.into_iter().filter(|r| seen.insert(r.clone())));
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Common,
    OnlyFirst,
    OnlySecond,
    Union,
}

/// Compare two lists, as the Verse List Manager does. The result is in canonical order.
pub fn combine(op: Op, a: &[ListRef], b: &[ListRef]) -> Vec<ListRef> {
    let (sa, sb): (HashSet<&ListRef>, HashSet<&ListRef>) = (a.iter().collect(), b.iter().collect());
    let mut out: Vec<ListRef> = match op {
        Op::Common => a.iter().filter(|r| sb.contains(r)).cloned().collect(),
        Op::OnlyFirst => a.iter().filter(|r| !sb.contains(r)).cloned().collect(),
        Op::OnlySecond => b.iter().filter(|r| !sa.contains(r)).cloned().collect(),
        Op::Union => a.iter().chain(b.iter()).cloned().collect(),
    };
    sort_dedupe(&mut out);
    out
}

/// The verses of a range of `vers`, as references in numbering `ns`.
pub fn refs_from_span(vers: &Versification, ns: &str, span: Range<usize>) -> Vec<ListRef> {
    span.filter_map(|i| {
        let (b, chapter, verse) = vers.locate(i)?;
        Some(ListRef { ns: ns.to_string(), code: vers.books[b].code.clone(), chapter, verse })
    })
    .collect()
}

/// Read a BibleWorks `.vls` file: `BWRL 1`, a title, then `VERSION Book c:v` lines.
pub fn parse_vls(text: &str) -> Option<(String, Vec<ListRef>)> {
    let mut lines = text.lines();
    if !lines.next()?.trim_start_matches('\u{feff}').starts_with("BWRL") {
        return None;
    }
    let title = lines.next()?.trim().to_string();
    let mut refs = Vec::new();
    for line in lines {
        let mut t = line.split_whitespace();
        let (Some(ver), Some(code), Some(cv)) = (t.next(), t.next(), t.next()) else { continue };
        let Some((c, v)) = cv.split_once(':') else { continue };
        let Ok(chapter) = c.parse() else { continue };
        // "Eph 2:8-10" is a range within one chapter.
        let (lo, hi) = match v.split_once('-') {
            Some((a, b)) => (a.parse().ok(), b.parse().ok()),
            None => (v.parse().ok(), v.parse().ok()),
        };
        if let (Some(lo), Some(hi)) = (lo, hi) {
            let (lo, hi): (usize, usize) = (lo, hi);
            for verse in lo..=hi.min(lo + 200) {
                refs.push(ListRef { ns: ver.to_lowercase(), code: code.to_string(), chapter, verse });
            }
        }
    }
    Some((title, refs))
}

pub fn to_vls(title: &str, refs: &[ListRef]) -> String {
    let mut s = format!("BWRL 1\r\n{}\r\n", title.replace(['\r', '\n'], " "));
    for r in refs {
        s.push_str(&format!("{} {} {}:{}\r\n", r.ns.to_uppercase(), r.code, r.chapter, r.verse));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(code: &str, c: usize, v: usize) -> ListRef {
        ListRef { ns: "kjv".into(), code: code.into(), chapter: c, verse: v }
    }

    #[test]
    fn set_operations() {
        let a = vec![r("Joh", 3, 16), r("Gen", 1, 1), r("Rom", 8, 1)];
        let b = vec![r("Rom", 8, 1), r("Gen", 1, 1), r("Rev", 1, 1)];
        assert_eq!(combine(Op::Common, &a, &b), vec![r("Gen", 1, 1), r("Rom", 8, 1)]);
        assert_eq!(combine(Op::OnlyFirst, &a, &b), vec![r("Joh", 3, 16)]);
        assert_eq!(combine(Op::OnlySecond, &a, &b), vec![r("Rev", 1, 1)]);
        assert_eq!(combine(Op::Union, &a, &b).len(), 4);
        // the same chapter and verse in a different numbering is a different reference
        let heb = ListRef { ns: "wtt".into(), ..r("Psa", 51, 3) };
        assert_eq!(combine(Op::Common, &[r("Psa", 51, 3)], &[heb]), vec![]);
    }

    #[test]
    fn sorting_dedupes_and_appending_skips_known() {
        let mut v = vec![r("Rev", 1, 1), r("Gen", 2, 1), r("Gen", 1, 1), r("Gen", 2, 1)];
        sort_dedupe(&mut v);
        assert_eq!(v, vec![r("Gen", 1, 1), r("Gen", 2, 1), r("Rev", 1, 1)]);
        append_unique(&mut v, vec![r("Gen", 1, 1), r("Exo", 1, 1)]);
        assert_eq!(v.len(), 4);
        assert_eq!(v.last(), Some(&r("Exo", 1, 1)));
    }

    #[test]
    fn vls_roundtrip_and_ranges() {
        let (title, refs) = parse_vls("BWRL 1\r\nMy list\r\nKJV Eph 2:8-10\r\nNAS Joh 3:16\r\nbad line\r\n").unwrap();
        assert_eq!(title, "My list");
        assert_eq!(refs.len(), 4);
        assert_eq!(refs[3].ns, "nas");
        let text = to_vls(&title, &refs);
        assert_eq!(parse_vls(&text).unwrap().1, refs);
        assert!(parse_vls("not a list").is_none());
    }

    #[test]
    fn lists_collection() {
        let mut l = VerseLists::default();
        let id = l.create("", vec![]).id.clone();
        assert_eq!(l.lists[0].name, "List 1");
        let id2 = l.create("Love", vec![r("Joh", 3, 16)]).id.clone();
        assert_ne!(id, id2);
        assert!(l.delete(&id));
        assert!(!l.delete(&id));
        assert_eq!(l.get_mut(&id2).unwrap().refs.len(), 1);
    }

    #[test]
    fn spans_become_refs() {
        let v = Versification::kjv();
        let refs = refs_from_span(&v, "kjv", v.parse_span("Eph 2:8-10").unwrap());
        assert_eq!(refs, vec![r("Eph", 2, 8), r("Eph", 2, 9), r("Eph", 2, 10)]);
    }

    #[test]
    fn reads_a_real_bibleworks_vls() {
        let Some(dir) = crate::install_dir() else { return };
        let Ok(text) = std::fs::read_to_string(dir.join("ase/results1.vls")) else { return };
        let (title, refs) = parse_vls(&text).unwrap();
        assert_eq!(title, "Results 1");
        assert_eq!(refs[0], r("Gal", 5, 22));
        assert!(refs.len() > 10);
    }
}
