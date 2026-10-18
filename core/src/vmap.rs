//! Verse maps between versifications, read from BibleWorks' `.vmf` files.
//!
//! Every module's numbering is related to a master numbering: Hebrew OT (WTT) and Greek NT (GNT).
//! A `.vmf` line `A B WTT` says "A in this module is B in the master"; verses without a rule are
//! the same in both. Mapping module P to module T goes P -> master -> T.
// ponytail: ranges inside one chapter only (all that occurs for WTT/GNT lines), and only the
// WTT/GNT masters; LXX/Vulgate maps (LXT, VUL ...) are ignored. BibleWorks' own merges that its
// .vmf files omit (e.g. KJV Num 26:1 = Heb 25:19 + 26:1) map by identity.

use crate::versification::Versification;
use std::{collections::HashMap, fs, path::Path};

/// (book code, chapter, verse)
pub type Ref = (String, usize, usize);

#[derive(Default, Debug)]
pub struct Vmap {
    to_master: HashMap<Ref, Vec<Ref>>,
    from_master: HashMap<Ref, Vec<Ref>>,
}

fn parse_range(book: &str, cv: &str) -> Option<Vec<Ref>> {
    let (c, v) = cv.split_once(':')?;
    let c: usize = c.parse().ok()?;
    let (a, b) = match v.split_once('-') {
        Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
        None => (v.parse().ok()?, v.parse().ok()?),
    };
    let book = if book == "Sol" { "Sol" } else { book };
    (a <= b && b - a < 400).then(|| (a..=b).map(|v| (book.to_string(), c, v)).collect())
}

impl Vmap {
    /// Rules for one module, from the text of its `.vmf` file.
    pub fn parse(text: &str) -> Self {
        let mut m = Vmap::default();
        for line in text.lines() {
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() < 5 || !matches!(t[4], "WTT" | "GNT") {
                continue;
            }
            let (Some(src), Some(dst)) = (parse_range(t[0], t[1]), parse_range(t[2], t[3])) else { continue };
            let pairs: Vec<(Ref, Ref)> = if src.len() == dst.len() {
                src.into_iter().zip(dst).collect()
            } else if src.len() == 1 {
                dst.into_iter().map(|d| (src[0].clone(), d)).collect()
            } else if dst.len() == 1 {
                src.into_iter().map(|s| (s, dst[0].clone())).collect()
            } else {
                continue;
            };
            for (s, d) in pairs {
                push_unique(m.to_master.entry(s.clone()).or_default(), d.clone());
                push_unique(m.from_master.entry(d).or_default(), s);
            }
        }
        m
    }

    pub fn load(databases: &Path, id: &str) -> Self {
        // File names vary in case (KJV.vmf, byz.VMF); match case-insensitively.
        let want = format!("{id}.vmf");
        let found = fs::read_dir(databases).into_iter().flatten().flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&want));
        found.and_then(|e| fs::read(e.path()).ok()).map(|b| Self::parse(&b.iter().map(|&c| c as char).collect::<String>())).unwrap_or_default()
    }

    fn master(&self, r: &Ref) -> Vec<Ref> {
        self.to_master.get(r).cloned().unwrap_or_else(|| vec![r.clone()])
    }

    fn local(&self, master: &Ref) -> Vec<Ref> {
        self.from_master.get(master).cloned().unwrap_or_else(|| vec![master.clone()])
    }
}

fn push_unique(v: &mut Vec<Ref>, r: Ref) {
    if !v.contains(&r) {
        v.push(r);
    }
}

/// Verses of `to` (as module verse indices, canon order) that correspond to verse `r` of `from`.
pub fn map_verse(from: (&Vmap, &Versification), to: (&Vmap, &Versification), r: &Ref) -> Vec<usize> {
    let _ = from.1;
    let mut out: Vec<usize> = Vec::new();
    for m in from.0.master(r) {
        for t in to.0.local(&m) {
            let idx = to.1.book_index(&t.0).and_then(|b| to.1.index(b, t.1, t.2));
            if let Some(i) = idx {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
    }
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psalm_titles_and_joel() {
        let kjv = Vmap::parse("Psa 3:1 Psa 3:1 WTT\nPsa 3:1-8 Psa 3:2-9 WTT\nJoe 2:28-32 Joe 3:1-5 WTT\nJoe 3:1-21 Joe 4:1-21 WTT\n");
        let (kv, hv) = (Versification::kjv(), Versification::hebrew());
        let none = Vmap::default();
        let r = |c: &str, ch, v| (c.to_string(), ch, v);
        let hidx = |c: &str, ch, v| hv.index(hv.book_index(c).unwrap(), ch, v).unwrap();
        // KJV Psalm 3:1 is the Hebrew title (3:1) plus Hebrew 3:2; KJV 3:2 is Hebrew 3:3.
        assert_eq!(map_verse((&kjv, &kv), (&none, &hv), &r("Psa", 3, 1)), vec![hidx("Psa", 3, 1), hidx("Psa", 3, 2)]);
        assert_eq!(map_verse((&kjv, &kv), (&none, &hv), &r("Psa", 3, 2)), vec![hidx("Psa", 3, 3)]);
        // And back: Hebrew Joel 4:1 is KJV Joel 3:1.
        let k = |c: &str, ch, v| kv.index(kv.book_index(c).unwrap(), ch, v).unwrap();
        assert_eq!(map_verse((&none, &hv), (&kjv, &kv), &r("Joe", 4, 1)), vec![k("Joe", 3, 1)]);
        // No rule -> identity.
        assert_eq!(map_verse((&kjv, &kv), (&none, &hv), &r("Gen", 1, 1)), vec![0]);
    }

    #[test]
    fn real_kjv_to_wtt() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        let (kjv, wtt) = (Vmap::load(&db, "kjv"), Vmap::default());
        let (kv, hv) = (Versification::kjv(), Versification::hebrew());
        // KJV Psalm 51:1 holds the two-verse Hebrew title (51:1-2) plus Hebrew 51:3.
        let got = map_verse((&kjv, &kv), (&wtt, &hv), &("Psa".to_string(), 51, 1));
        let want: Vec<usize> = [1, 2, 3].iter().map(|&v| hv.index(hv.book_index("Psa").unwrap(), 51, v).unwrap()).collect();
        assert_eq!(got, want);
    }
}
