//! Search over an English (or other Latin-script) version module. Query syntax:
//!   word            whole word, case-insensitive; `*` matches any run of letters and `?` one letter
//!                   anywhere in the word (`love*`, `wom?n`, `*belie*`)
//!   "a phrase"      contiguous words (wildcards allowed)
//!   H430 / G3056    Strong's number (H = Old Testament verses, G = New)
//! Terms are ANDed within a verse. A leading `/` ORs them instead, a leading `'` makes the whole
//! query one phrase, and a leading `.` is the default AND (as in the BibleWorks command line).
// ponytail: linear scan of ~31k verses (milliseconds); add an inverted index if a bigger corpus needs it.

use crate::{
    module::{tokens, Module, Token},
    osearch::glob_str,
};

enum Term {
    /// A word pattern (lowercase, may contain `*` and `?`).
    Word(String),
    Phrase(Vec<String>),
    Strongs(bool, u32),
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && !matches!(c, '\'' | '*' | '?')).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// (terms, any) where `any` means a verse needs only one of the terms.
fn parse(q: &str) -> (Vec<Term>, bool) {
    let mut rest = q.trim();
    let (mut any, mut one_phrase) = (false, false);
    match rest.chars().next() {
        Some('/') => { any = true; rest = &rest[1..]; }
        Some('\'') => { one_phrase = true; rest = &rest[1..]; }
        Some('.') => rest = &rest[1..],
        _ => {}
    }
    let mut terms = Vec::new();
    if one_phrase {
        let w = words(rest);
        if !w.is_empty() {
            terms.push(Term::Phrase(w));
        }
        return (terms, false);
    }
    rest = rest.trim_start();
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
            let mut cs = tok.chars();
            let head = cs.next().map(|c| c.to_ascii_uppercase());
            match (head, cs.as_str().parse::<u32>()) {
                (Some('H'), Ok(n)) => terms.push(Term::Strongs(true, n)),
                (Some('G'), Ok(n)) => terms.push(Term::Strongs(false, n)),
                _ => {
                    if let Some(w) = words(tok).into_iter().next() {
                        terms.push(Term::Word(w));
                    }
                }
            }
        }
    }
    (terms, any)
}

/// Which of `toks` the query matched (for result highlighting).
pub fn highlight(query: &str, toks: &[Token], hebrew: bool) -> Vec<bool> {
    let (terms, _) = parse(query);
    toks.iter()
        .map(|t| {
            let tw = words(&t.text);
            terms.iter().any(|term| match term {
                Term::Word(p) => tw.iter().any(|x| glob_str(p, x)),
                Term::Phrase(p) => tw.iter().any(|x| p.iter().any(|q| glob_str(q, x))),
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
        Term::Word(p) => plain_words.iter().any(|x| glob_str(p, x)),
        Term::Phrase(p) => plain_words.windows(p.len()).any(|win| win.iter().zip(p).all(|(w, q)| glob_str(q, w))),
        Term::Strongs(h, n) => *h == hebrew && toks.iter().any(|t| t.strongs.as_deref().and_then(strongs_num) == Some(*n)),
    }
}

/// True when verse `i` of `m` is in an Old Testament book (selects Hebrew `H` vs Greek `G` Strong's numbers).
pub fn is_ot(m: &Module, i: usize) -> bool {
    m.vers.locate(i).map_or(false, |(b, _, _)| crate::canon::BOOKS[..39].iter().any(|k| k.code == m.vers.books[b].code))
}

/// Verse indices matching `query`, in canon order. Empty query -> no hits.
pub fn search(m: &Module, query: &str, limit: usize) -> Vec<usize> {
    let (terms, any) = parse(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for i in 0..m.verse_count() {
        let Some(raw) = m.raw(i) else { continue };
        let toks = tokens(&raw);
        let pw = words(&toks.iter().map(|t| t.text.as_str()).collect::<String>());
        let hebrew = is_ot(m, i);
        let ok = if any { terms.iter().any(|t| matches(t, &toks, &pw, hebrew)) } else { terms.iter().all(|t| matches(t, &toks, &pw, hebrew)) };
        if ok {
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

    #[test]
    fn wildcards_anywhere_and_control_prefixes() {
        let Some(dir) = crate::install_dir() else { return };
        let m = Module::open(dir.join("databases/kjv.dbx")).unwrap();
        let n = |q: &str| search(&m, q, 40000).len();
        assert!(n("*belie*") > n("believe")); // "believeth", "unbelief" ...
        assert!(n("wom?n") > 0 && n("wom?n") == n("/ woman women") && n("woman") < n("wom?n"));
        assert!(n("/ faith hope charity") > n("faith hope charity")); // OR vs AND
        assert_eq!(n("'faith hope charity"), n("\"faith hope charity\""));
        assert!(n("'faith hope charity") >= 1); // 1 Cor 13:13
        assert_eq!(n(".faith"), n("faith")); // leading . is the plain AND form
        assert_eq!(n("héllo wörld"), 0); // accented letters don't crash the parser
        let toks = tokens("For God so loved <25> the world");
        assert!(highlight("lov*", &toks, false).iter().any(|h| *h));
    }
}
