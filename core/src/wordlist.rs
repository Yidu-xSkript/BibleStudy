//! Word lists: frequency lists compiled from a version and range, saved by name, and compared,
//! as in BibleWorks' Word List Manager.
//!
//! English counts words of the text. Greek and Hebrew count lemmas through the paired
//! morphology module, and can be limited to one part of speech.

use crate::{
    greek::greek,
    hebrew::hebrew,
    module::{tokens, Module},
    morph::morphemes,
    script::Script,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ops::Range};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WordEntry {
    pub word: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WordList {
    pub id: String,
    pub name: String,
    /// Where it came from, e.g. "KJV · James".
    pub source: String,
    pub entries: Vec<WordEntry>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct WordLists {
    pub lists: Vec<WordList>,
    next: u64,
}

impl WordLists {
    pub fn create(&mut self, name: &str, source: &str, entries: Vec<WordEntry>) -> &WordList {
        self.next += 1;
        let name = if name.trim().is_empty() { format!("Word list {}", self.next) } else { name.trim().to_string() };
        self.lists.push(WordList { id: format!("w{}", self.next), name, source: source.to_string(), entries });
        self.lists.last().unwrap()
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let n = self.lists.len();
        self.lists.retain(|l| l.id != id);
        self.lists.len() != n
    }
}

fn english_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'').map(|w| w.trim_matches('\'').to_lowercase()).filter(|w| !w.is_empty())
}

/// Counts for the verses in `range`. `pos` limits Greek/Hebrew lemmas to one part-of-speech letter
/// of the morphology code (`n` noun, `v` verb ...).
pub fn frequency(text: &Module, script: Script, morph: Option<&Module>, range: Range<usize>, pos: Option<char>) -> Vec<WordEntry> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for i in range {
        match (script, morph) {
            (Script::Latin, _) => {
                let raw = text.raw(i).unwrap_or_default();
                let plain: String = tokens(&raw).into_iter().map(|t| t.text).collect();
                for w in english_words(&plain) {
                    *counts.entry(w).or_default() += 1;
                }
            }
            (sc, Some(m)) => {
                for word in m.raw(i).unwrap_or_default().split_whitespace() {
                    let parts = morphemes(word);
                    // A Hebrew word's own lemma is its last morpheme that isn't a prefix particle.
                    let part = if sc == Script::Hebrew { parts.iter().rev().find(|p| !p.code.starts_with('P')).or(parts.last()) } else { parts.first() };
                    let Some(p) = part else { continue };
                    if p.code.starts_with('x') && sc == Script::Hebrew {
                        continue; // paragraph marker
                    }
                    if pos.map_or(false, |c| p.code.chars().next() != Some(c)) {
                        continue;
                    }
                    let lemma = if sc == Script::Hebrew { hebrew(&p.lemma, false) } else { greek(&p.lemma) };
                    *counts.entry(lemma).or_default() += 1;
                }
            }
            (sc, None) => {
                for w in text.raw(i).unwrap_or_default().split_whitespace() {
                    *counts.entry(crate::script::decode(sc, w, false)).or_default() += 1;
                }
            }
        }
    }
    let mut out: Vec<WordEntry> = counts.into_iter().map(|(word, count)| WordEntry { word, count }).collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    out
}

pub use crate::lists::Op;

/// Compare two lists by word. Counts come from the list the word is taken from; a union adds them.
pub fn combine(op: Op, a: &[WordEntry], b: &[WordEntry]) -> Vec<WordEntry> {
    let (ma, mb): (HashMap<&str, usize>, HashMap<&str, usize>) = (a.iter().map(|e| (e.word.as_str(), e.count)).collect(), b.iter().map(|e| (e.word.as_str(), e.count)).collect());
    let mut out: Vec<WordEntry> = match op {
        Op::Common => a.iter().filter(|e| mb.contains_key(e.word.as_str())).cloned().collect(),
        Op::OnlyFirst => a.iter().filter(|e| !mb.contains_key(e.word.as_str())).cloned().collect(),
        Op::OnlySecond => b.iter().filter(|e| !ma.contains_key(e.word.as_str())).cloned().collect(),
        Op::Union => {
            let mut all: HashMap<&str, usize> = ma.clone();
            for (w, c) in &mb {
                *all.entry(w).or_default() += c;
            }
            all.into_iter().map(|(w, count)| WordEntry { word: w.to_string(), count }).collect()
        }
    };
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(w: &str, c: usize) -> WordEntry {
        WordEntry { word: w.into(), count: c }
    }

    #[test]
    fn comparing_lists() {
        let a = vec![e("love", 5), e("faith", 3), e("grace", 1)];
        let b = vec![e("faith", 9), e("hope", 2)];
        assert_eq!(combine(Op::Common, &a, &b), vec![e("faith", 3)]);
        assert_eq!(combine(Op::OnlyFirst, &a, &b), vec![e("love", 5), e("grace", 1)]);
        assert_eq!(combine(Op::OnlySecond, &a, &b), vec![e("hope", 2)]);
        assert_eq!(combine(Op::Union, &a, &b), vec![e("faith", 12), e("love", 5), e("hope", 2), e("grace", 1)]);
    }

    #[test]
    fn english_word_splitting() {
        let w: Vec<_> = english_words("The LORD's 'anointed', God-fearing; thou art.").collect();
        assert_eq!(w, ["the", "lord's", "anointed", "god", "fearing", "thou", "art"]);
    }

    #[test]
    fn counts_real_modules() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        let kjv = Module::open(db.join("kjv.dbx")).unwrap();
        let james = kjv.vers.parse_span("Jam").unwrap();
        let f = frequency(&kjv, Script::Latin, None, james, None);
        let count = |w: &str| f.iter().find(|e| e.word == w).map_or(0, |e| e.count);
        assert_eq!(count("faith"), 16); // James uses "faith" 16 times in the KJV
        assert!(count("the") > 100);
        assert_eq!(f[0].count, f.iter().map(|e| e.count).max().unwrap()); // sorted by frequency

        // Greek lemmas: the article leads; filtering to verbs excludes it.
        let (byz, bym) = (Module::open(db.join("byz.dbx")).unwrap(), Module::open(db.join("bym.dbx")).unwrap());
        let span = byz.vers.parse_span("Jam").unwrap();
        let all = frequency(&byz, Script::Greek, Some(&bym), span.clone(), None);
        assert_eq!(all[0].word, "ὁ");
        let verbs = frequency(&byz, Script::Greek, Some(&bym), span, Some('v'));
        assert!(verbs.iter().all(|e| e.word != "ὁ") && verbs.len() > 50);

        // Hebrew lemmas with the article/prefix particles skipped as the main lemma.
        let (wtt, wtm) = (Module::open(db.join("wtt.dbx")).unwrap(), Module::open(db.join("wtm.dbx")).unwrap());
        let ruth = frequency(&wtt, Script::Hebrew, Some(&wtm), wtt.vers.parse_span("Rut").unwrap(), Some('v'));
        assert!(!ruth.is_empty());
    }
}
