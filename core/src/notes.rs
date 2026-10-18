//! Notes on verses and chapters.
//!
//! A note is keyed by the numbering it was written in (`kjv|Joh 3:16`, chapter notes `kjv|Joh 3`),
//! so it can follow the reader across versions through the verse maps. Plain text; saved as JSON.

use crate::canon::canon_order;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Note {
    pub text: String,
    /// Unix seconds of the last edit.
    pub updated: u64,
}

/// Key for a verse note, or a chapter note when `verse` is 0.
pub fn key(ns: &str, code: &str, chapter: usize, verse: usize) -> String {
    if verse == 0 {
        format!("{ns}|{code} {chapter}")
    } else {
        format!("{ns}|{code} {chapter}:{verse}")
    }
}

/// (numbering, book code, chapter, verse) of a key; verse 0 for a chapter note.
pub fn parse_key(k: &str) -> Option<(String, String, usize, usize)> {
    let (ns, rest) = k.split_once('|')?;
    let (code, cv) = rest.split_once(' ')?;
    let (c, v) = match cv.split_once(':') {
        Some((c, v)) => (c, v.parse().ok()?),
        None => (cv, 0),
    };
    Some((ns.to_string(), code.to_string(), c.parse().ok()?, v))
}

#[derive(Serialize, Deserialize, Default)]
pub struct Notes {
    notes: BTreeMap<String, Note>,
}

impl Notes {
    pub fn get(&self, key: &str) -> Option<&Note> {
        self.notes.get(key)
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    /// Store `text` (blank text deletes the note). Returns whether anything changed.
    pub fn set(&mut self, key: &str, text: &str, now: u64) -> bool {
        if text.trim().is_empty() {
            return self.notes.remove(key).is_some();
        }
        if self.notes.get(key).map_or(false, |n| n.text == text) {
            return false;
        }
        self.notes.insert(key.to_string(), Note { text: text.to_string(), updated: now });
        true
    }

    /// All notes in canonical order whose text or reference contains `query` (case-insensitive).
    pub fn all(&self, query: &str) -> Vec<(&String, &Note)> {
        let q = query.trim().to_lowercase();
        let mut out: Vec<_> = self.notes.iter().filter(|(k, n)| q.is_empty() || n.text.to_lowercase().contains(&q) || k.to_lowercase().contains(&q)).collect();
        out.sort_by_key(|(k, _)| parse_key(k).map_or((usize::MAX, 0, 0), |(_, code, c, v)| (canon_order(&code), c, v)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_roundtrip() {
        assert_eq!(key("kjv", "Joh", 3, 16), "kjv|Joh 3:16");
        assert_eq!(key("kjv", "Joh", 3, 0), "kjv|Joh 3");
        assert_eq!(parse_key("kjv|Joh 3:16"), Some(("kjv".into(), "Joh".into(), 3, 16)));
        assert_eq!(parse_key("wtt|1Sa 20"), Some(("wtt".into(), "1Sa".into(), 20, 0)));
        assert_eq!(parse_key("garbage"), None);
    }

    #[test]
    fn set_edit_delete_and_order() {
        let mut n = Notes::default();
        assert!(n.set("kjv|Rev 1:1", "last book", 1));
        assert!(n.set("kjv|Gen 1:1", "In the beginning", 2));
        assert!(n.set("kjv|Gen 1", "creation chapter", 3));
        assert!(!n.set("kjv|Gen 1:1", "In the beginning", 4)); // unchanged
        let order: Vec<_> = n.all("").iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(order, ["kjv|Gen 1", "kjv|Gen 1:1", "kjv|Rev 1:1"]);
        assert_eq!(n.all("BEGINNING").len(), 1); // case-insensitive text match
        assert_eq!(n.all("rev").len(), 1); // matches the reference too
        assert!(n.set("kjv|Gen 1:1", "   ", 5)); // blank deletes
        assert!(n.get("kjv|Gen 1:1").is_none());
        assert!(!n.set("kjv|Gen 1:1", "", 6)); // deleting nothing changes nothing
    }
}
