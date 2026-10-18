//! Search over a version module. Query syntax:
//!   word            whole word, case-insensitive (`love*` = prefix)
//!   "a phrase"      contiguous words
//!   H430 / G3056    Strong's number (H = Old Testament verses, G = New)
//! Terms are ANDed within a verse.
// ponytail: linear scan of ~31k verses (milliseconds); add an inverted index if a bigger corpus needs it.

use crate::module::{tokens, Module, Token};

enum Term {
    Word(String, bool), // text, prefix match
    Phrase(Vec<String>),
    Strongs(bool, u32), // is_hebrew, number
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

fn parse(q: &str) -> Vec<Term> {
    let mut terms = Vec::new();
    let mut rest = q.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            let w = words(&r[..end]);
            if !w.is_empty() {
                terms.push(Term::Phrase(w));
            }
            rest = r[(end + 1).min(r.len())..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let tok = &rest[..end];
            rest = rest[end..].trim_start();
            let (head, num) = tok.split_at(1.min(tok.len()));
            match (head.to_ascii_uppercase().as_str(), num.parse::<u32>()) {
                ("H", Ok(n)) => terms.push(Term::Strongs(true, n)),
                ("G", Ok(n)) => terms.push(Term::Strongs(false, n)),
                _ => {
                    let prefix = tok.ends_with('*');
                    if let Some(w) = words(tok).into_iter().next() {
                        terms.push(Term::Word(w, prefix));
                    }
                }
            }
        }
    }
    terms
}

/// Which of `toks` the query matched (for result highlighting).
pub fn highlight(query: &str, toks: &[Token], hebrew: bool) -> Vec<bool> {
    let terms = parse(query);
    toks.iter()
        .map(|t| {
            let tw = words(&t.text);
            terms.iter().any(|term| match term {
                Term::Word(w, prefix) => tw.iter().any(|x| if *prefix { x.starts_with(w.as_str()) } else { x == w }),
                Term::Phrase(p) => tw.iter().any(|x| p.contains(x)),
                Term::Strongs(h, n) => *h == hebrew && t.strongs.as_deref().and_then(strongs_num) == Some(*n),
            })
        })
        .collect()
}

fn strongs_num(s: &str) -> Option<u32> {
    s.trim_end_matches(|c: char| c.is_ascii_lowercase()).parse().ok()
}

fn matches(term: &Term, toks: &[Token], plain_words: &[String], hebrew: bool) -> bool {
    match term {
        Term::Word(w, prefix) => plain_words.iter().any(|x| if *prefix { x.starts_with(w.as_str()) } else { x == w }),
        Term::Phrase(p) => plain_words.windows(p.len()).any(|win| win == p.as_slice()),
        Term::Strongs(h, n) => *h == hebrew && toks.iter().any(|t| t.strongs.as_deref().and_then(strongs_num) == Some(*n)),
    }
}

/// True when verse `i` of `m` is in an Old Testament book (selects Hebrew `H` vs Greek `G` Strong's numbers).
pub fn is_ot(m: &Module, i: usize) -> bool {
    m.vers.locate(i).map_or(false, |(b, _, _)| crate::canon::BOOKS[..39].iter().any(|k| k.code == m.vers.books[b].code))
}

/// Verse indices matching `query`, in canon order. Empty query -> no hits.
pub fn search(m: &Module, query: &str, limit: usize) -> Vec<usize> {
    let terms = parse(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for i in 0..m.verse_count() {
        let Some(raw) = m.raw(i) else { continue };
        let toks = tokens(&raw);
        let pw = words(&toks.iter().map(|t| t.text.as_str()).collect::<String>());
        let hebrew = is_ot(m, i);
        if terms.iter().all(|t| matches(t, &toks, &pw, hebrew)) {
            hits.push(i);
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_installed_kjv() {
        let Some(dir) = crate::install_dir() else { return };
        let m = Module::open(dir.join("databases/kjv.dbx")).unwrap();
        let jn316 = m.vers.index(42, 3, 16).unwrap();
        assert!(search(&m, "\"so loved the world\"", 50).contains(&jn316));
        assert!(search(&m, "G25 God", 5000).contains(&jn316)); // agapao
        assert!(!search(&m, "H25 God", 5000).contains(&jn316));
        assert_eq!(search(&m, "H7225 God", 10).first(), Some(&0)); // Gen 1:1
        assert!(search(&m, "", 10).is_empty());
        assert!(search(&m, "lov*", 10).len() == 10);
    }
}
