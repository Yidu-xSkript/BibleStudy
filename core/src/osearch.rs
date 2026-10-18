//! Search in Greek and Hebrew text.
//!
//! Terms are separated by spaces and all must occur in a verse (a leading `/` makes it any term,
//! a leading `'` makes the whole query one phrase; a leading `.` is the default AND):
//!   λόγος  logos      a word form
//!   λογ*  ?ογος       wildcards
//!   "ἐν ἀρχῇ"         a phrase of consecutive words
//!   ἀγαπάω@v*         lemma and morphology code (either side may be `*`); the code has an implied `*` at the end
//!   G25  H7225        a Strong's number
//! Accents, breathings, vowel points and cantillation are ignored unless the term itself contains
//! some; Latin letters are read as Beta Code (Greek) or Westminster letters (Hebrew).
// ponytail: no `[ae]` character ranges and no NOT; Hebrew prefixes (ה ו ב ...) stay attached to the
// word, so find every form of a word with `lemma@*`.

use crate::{
    greek::greek,
    hebrew::hebrew,
    lexenc::mc_consonants,
    lexicon::Strongs,
    module::Module,
    morph::morphemes,
    script::{decode, Script},
};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

const MAQQEF: char = '\u{5BE}';

fn is_mark(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}' | '\u{591}'..='\u{5BD}' | '\u{5BF}' | '\u{5C1}'..='\u{5C7}')
}

/// Marks that distinguish forms (accents, breathings, vowel points); cantillation is never one of them.
fn is_form_mark(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36F}' | '\u{5B0}'..='\u{5BC}' | '\u{5C1}' | '\u{5C2}')
}

fn base_letter(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        c => c,
    }
}

/// Letters only, no marks, folded: how words are compared by default.
fn fold_loose(s: &str, wild: bool) -> String {
    s.nfd().filter(|c| !is_mark(*c)).flat_map(|c| c.to_lowercase()).map(base_letter).filter(|c| c.is_alphabetic() || (wild && matches!(c, '*' | '?'))).collect()
}

/// Letters plus the marks that distinguish forms.
fn fold_strict(s: &str, wild: bool) -> String {
    s.nfd()
        .filter(|c| is_form_mark(*c) || !is_mark(*c))
        .flat_map(|c| c.to_lowercase())
        .map(base_letter)
        .filter(|c| c.is_alphabetic() || is_form_mark(*c) || (wild && matches!(c, '*' | '?')))
        .nfc()
        .collect()
}

/// The loose form of a raw module word without decoding it to Unicode (about 10x faster):
/// Beta Code letters, or the consonants of Westminster Hebrew split at maqqef.
fn raw_subs(script: Script, raw: &str) -> Vec<String> {
    match script {
        Script::Greek => {
            let g: String = raw.chars().filter_map(raw_greek_letter).collect();
            vec![g].into_iter().filter(|s| !s.is_empty()).collect()
        }
        Script::Hebrew => raw.split('-').map(mc_consonants).filter(|s| !s.is_empty()).collect(),
        Script::Latin => vec![],
    }
}

fn raw_greek_letter(c: char) -> Option<char> {
    Some(match c.to_ascii_lowercase() {
        'a' => 'α', 'b' => 'β', 'g' => 'γ', 'd' => 'δ', 'e' => 'ε', 'z' => 'ζ', 'h' => 'η', 'q' => 'θ', 'i' => 'ι', 'k' => 'κ',
        'l' => 'λ', 'm' => 'μ', 'n' => 'ν', 'c' => 'ξ', 'o' => 'ο', 'p' => 'π', 'r' => 'ρ', 's' => 'σ', 't' => 'τ', 'u' => 'υ',
        'f' => 'φ', 'x' => 'χ', 'y' => 'ψ', 'w' => 'ω', 'v' => 'ϝ',
        _ => return None,
    })
}

/// `*` matches any run of characters (including none), `?` exactly one.
fn glob(p: &[char], t: &[char]) -> bool {
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

pub(crate) fn glob_str(p: &str, t: &str) -> bool {
    glob(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>())
}

/// A word pattern: the loose form used for the first pass, and the strict form if the user typed marks.
#[derive(Clone, Debug)]
struct Pat {
    loose: String,
    strict: Option<String>,
}

impl Pat {
    fn new(unicode: &str) -> Pat {
        let typed_marks = unicode.nfd().any(is_form_mark);
        Pat { loose: fold_loose(unicode, true), strict: typed_marks.then(|| fold_strict(unicode, true)) }
    }
}

#[derive(Clone, Debug)]
enum Term {
    Word(Pat),
    Phrase(Vec<Pat>),
    /// `lemma@code`: either side may be absent.
    Morph { lemma: Option<String>, code: String },
    /// A Strong's number resolved to the lemmas it stands for (already folded).
    Lemmas(BTreeSet<String>),
}

#[derive(Debug)]
pub struct Query {
    terms: Vec<Term>,
    any: bool,
}

/// Latin letters typed for a Greek or Hebrew version become that script; Unicode passes through.
fn to_script(term: &str, script: Script) -> String {
    let has_native = term.chars().any(|c| ('\u{370}'..='\u{3FF}').contains(&c) || ('\u{1F00}'..='\u{1FFF}').contains(&c) || ('\u{590}'..='\u{5FF}').contains(&c));
    if has_native || !term.chars().any(|c| c.is_ascii_alphabetic() || ")(/\\^|+$&#".contains(c)) {
        return term.to_string();
    }
    match script {
        Script::Greek => greek(term),
        // Westminster letters are upper case; the wildcards and punctuation pass through unchanged.
        Script::Hebrew => hebrew(&term.to_uppercase(), false),
        Script::Latin => term.to_string(),
    }
}

/// Split on spaces, keeping "quoted phrases" together.
fn split_terms(q: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = q.trim();
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            out.push((true, r[..end].to_string()));
            rest = r[(end + 1).min(r.len())..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push((false, rest[..end].to_string()));
            rest = rest[end..].trim_start();
        }
    }
    out
}

fn is_strongs(t: &str) -> Option<(bool, u32)> {
    let mut cs = t.chars();
    let head = cs.next()?;
    let n = cs.as_str().parse().ok()?;
    match head {
        'H' | 'h' => Some((true, n)),
        'G' | 'g' => Some((false, n)),
        _ => None,
    }
}

pub fn parse(q: &str, script: Script, strongs: Option<&Strongs>) -> Result<Query, String> {
    let mut q = q.trim();
    let (mut any, mut all_phrase) = (false, false);
    match q.chars().next() {
        Some('/') => { any = true; q = &q[1..]; }
        Some('\'') => { all_phrase = true; q = &q[1..]; }
        Some('.') => q = &q[1..],
        _ => {}
    }
    let mut terms = Vec::new();
    if all_phrase {
        let words: Vec<Pat> = q.split_whitespace().map(|w| Pat::new(&to_script(w, script))).collect();
        if !words.is_empty() {
            terms.push(Term::Phrase(words));
        }
    } else {
        for (quoted, t) in split_terms(q) {
            if quoted {
                let words: Vec<Pat> = t.split_whitespace().map(|w| Pat::new(&to_script(w, script))).collect();
                if !words.is_empty() {
                    terms.push(Term::Phrase(words));
                }
            } else if let Some((hebrew_n, n)) = is_strongs(&t) {
                let s = strongs.ok_or("Strong's numbers aren't available")?;
                let set: BTreeSet<String> = s.lemmas_of(hebrew_n, n).iter().map(|l| fold_loose(l, false)).filter(|l| !l.is_empty()).collect();
                if set.is_empty() {
                    return Err(format!("There is no Strong's number {}{}.", if hebrew_n { 'H' } else { 'G' }, n));
                }
                terms.push(Term::Lemmas(set));
            } else if let Some((lemma, code)) = t.split_once('@') {
                let lemma = (!lemma.is_empty() && lemma != "*").then(|| fold_loose(&to_script(lemma, script), true));
                terms.push(Term::Morph { lemma, code: format!("{}*", code) });
            } else {
                terms.push(Term::Word(Pat::new(&to_script(&t, script))));
            }
        }
    }
    if terms.is_empty() {
        return Err("Type a word, a \"phrase\", lemma@code or a Strong's number.".into());
    }
    Ok(Query { terms, any })
}

/// One text word: its comparable pieces (Hebrew compounds split at the maqqef) and its morphemes.
struct IWord {
    subs: Vec<String>,
    /// (folded lemma, code) per morpheme.
    morphs: Vec<(String, String)>,
}

/// Folded words and lemmas of a version, built once and reused for every query.
pub struct Index {
    script: Script,
    verses: Vec<Vec<IWord>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    /// Verse index in the text module.
    pub verse: usize,
    /// Indices of the matching words within the verse.
    pub words: Vec<usize>,
}

impl Index {
    pub fn build(text: &Module, script: Script, morph: Option<&Module>) -> Index {
        let verses = (0..text.verse_count())
            .map(|i| {
                let raw = text.raw(i).unwrap_or_default();
                let mraw = morph.and_then(|m| m.raw(i)).unwrap_or_default();
                let mtokens: Vec<&str> = mraw.split_whitespace().collect();
                raw.split_whitespace()
                    .enumerate()
                    .map(|(k, w)| {
                        let subs = raw_subs(script, w);
                        let morphs = mtokens
                            .get(k)
                            .map(|t| {
                                morphemes(t)
                                    .into_iter()
                                    .map(|m| (raw_subs(script, &m.lemma.replace('-', "")).concat(), m.code))
                                    .collect()
                            })
                            .unwrap_or_default();
                        IWord { subs, morphs }
                    })
                    .collect()
            })
            .collect();
        Index { script, verses }
    }

    pub fn has_morphology(&self) -> bool {
        self.verses.iter().take(50).any(|v| v.iter().any(|w| !w.morphs.is_empty()))
    }

    /// Verses matching `q`, in order, at most `limit`.
    pub fn search(&self, text: &Module, q: &Query, limit: usize) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (i, words) in self.verses.iter().enumerate() {
            let mut matched: BTreeSet<usize> = BTreeSet::new();
            let mut ok = !q.any;
            for t in &q.terms {
                let m = self.match_term(text, i, words, t);
                if q.any {
                    ok |= !m.is_empty();
                } else {
                    ok &= !m.is_empty();
                }
                matched.extend(m);
                if !q.any && !ok {
                    break;
                }
            }
            if ok && !matched.is_empty() {
                hits.push(Hit { verse: i, words: matched.into_iter().collect() });
                if hits.len() >= limit {
                    break;
                }
            }
        }
        hits
    }

    /// Words of verse `i` that satisfy `t` (empty when it doesn't occur).
    fn match_term(&self, text: &Module, i: usize, words: &[IWord], t: &Term) -> Vec<usize> {
        match t {
            Term::Word(p) => {
                let mut out = Vec::new();
                for (k, w) in words.iter().enumerate() {
                    if w.subs.iter().any(|s| glob_str(&p.loose, s)) && self.strict_ok(text, i, k, std::slice::from_ref(p)) {
                        out.push(k);
                    }
                }
                out
            }
            Term::Phrase(pats) => {
                // Flatten to sub-words so a phrase can run through maqqef compounds.
                let flat: Vec<(&str, usize)> = words.iter().enumerate().flat_map(|(k, w)| w.subs.iter().map(move |s| (s.as_str(), k))).collect();
                let mut out = Vec::new();
                for start in 0..flat.len().saturating_sub(pats.len() - 1) {
                    if pats.iter().enumerate().all(|(j, p)| glob_str(&p.loose, flat[start + j].0)) {
                        let ks: BTreeSet<usize> = (0..pats.len()).map(|j| flat[start + j].1).collect();
                        if ks.iter().all(|k| self.strict_ok(text, i, *k, pats)) {
                            out.extend(ks);
                        }
                    }
                }
                out
            }
            Term::Morph { lemma, code } => words
                .iter()
                .enumerate()
                .filter(|(_, w)| w.morphs.iter().any(|(l, c)| lemma.as_ref().map_or(true, |p| glob_str(p, l)) && glob_str(code, c)))
                .map(|(k, _)| k)
                .collect(),
            Term::Lemmas(set) => words.iter().enumerate().filter(|(_, w)| w.morphs.iter().any(|(l, _)| set.contains(l))).map(|(k, _)| k).collect(),
        }
    }

    /// When the user typed marks, the candidate word must also match with them.
    fn strict_ok(&self, text: &Module, i: usize, k: usize, pats: &[Pat]) -> bool {
        let strict: Vec<&String> = pats.iter().filter_map(|p| p.strict.as_ref()).collect();
        if strict.is_empty() {
            return true;
        }
        let Some(raw) = text.raw(i) else { return false };
        let Some(w) = raw.split_whitespace().nth(k) else { return false };
        let subs: Vec<String> = decode(self.script, w, false).split(MAQQEF).map(|s| fold_strict(s, false)).collect();
        strict.iter().any(|p| subs.iter().any(|s| glob_str(p, s)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_and_globs() {
        assert_eq!(fold_loose("λόγος", false), "λογοσ");
        assert_eq!(fold_loose("Ἰησοῦς,", false), "ιησουσ");
        assert_eq!(fold_loose("בְּרֵאשִׁ֖ית", false), "בראשית");
        assert_eq!(fold_loose("מֶלֶךְ", false), "מלכ"); // final kaf folds to kaf
        assert_eq!(fold_strict("λόγος", false), fold_strict("λόγος", false));
        assert_ne!(fold_strict("λογος", false), fold_strict("λόγος", false)); // accent is kept
        assert!(glob_str("λογ*", "λογοσ"));
        assert!(glob_str("?ογοσ", "λογοσ"));
        assert!(glob_str("*ογ*", "λογοσ"));
        assert!(!glob_str("λογ?", "λογοσ"));
        assert!(glob_str("v*", "viaa3s"));
    }

    #[test]
    fn query_parsing() {
        let q = parse("λόγος \"ἐν ἀρχῇ\" ἀγαπάω@v*", Script::Greek, None).unwrap();
        assert_eq!(q.terms.len(), 3);
        assert!(!q.any);
        assert!(parse("/ a b", Script::Greek, None).unwrap().any);
        assert!(matches!(&parse("'ἐν ἀρχῇ", Script::Greek, None).unwrap().terms[0], Term::Phrase(p) if p.len() == 2));
        assert!(parse("   ", Script::Greek, None).is_err());
        // Latin letters become Greek (Beta Code), accent marks in the term make it strict
        let q = parse("logos", Script::Greek, None).unwrap();
        assert!(matches!(&q.terms[0], Term::Word(p) if p.loose == "λογοσ" && p.strict.is_none()));
        let q = parse("λόγος", Script::Greek, None).unwrap();
        assert!(matches!(&q.terms[0], Term::Word(p) if p.strict.is_some()));
        let q = parse("λογος", Script::Greek, None).unwrap();
        assert!(matches!(&q.terms[0], Term::Word(p) if p.strict.is_none()));
        // Westminster letters for Hebrew
        let q = parse("BR)", Script::Hebrew, None).unwrap();
        assert!(matches!(&q.terms[0], Term::Word(p) if p.loose == "ברא"));
    }

    fn open(db: &std::path::Path, id: &str) -> Module {
        Module::open(db.join(format!("{id}.dbx"))).unwrap()
    }

    fn refs(text: &Module, hits: &[Hit]) -> Vec<String> {
        hits.iter()
            .filter_map(|h| {
                let (b, c, v) = text.vers.locate(h.verse)?;
                Some(format!("{} {}:{}", text.vers.books[b].code, c, v))
            })
            .collect()
    }

    #[test]
    fn greek_search_on_the_real_text() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        let strongs = Strongs::open(&db).unwrap();
        let (byz, bym) = (open(&db, "byz"), open(&db, "bym"));
        let t0 = std::time::Instant::now();
        let idx = Index::build(&byz, Script::Greek, Some(&bym));
        println!("byz index built in {:?}", t0.elapsed());
        assert!(idx.has_morphology());
        let find = |q: &str| {
            let q = parse(q, Script::Greek, Some(&strongs)).unwrap();
            let t = std::time::Instant::now();
            let h = idx.search(&byz, &q, 5000);
            println!("{} -> {} hits in {:?}", format!("{q:?}").chars().take(40).collect::<String>(), h.len(), t.elapsed());
            refs(&byz, &h)
        };
        // a surface form, with and without accents typed
        let r = find("ἠγάπησεν");
        assert!(r.contains(&"Joh 3:16".to_string()), "{r:?}");
        // an unaccented search is looser: it also finds the enclitic ἠγάπησέν (Joh 15:9) that the accented one excludes
        let loose = find("ηγαπησεν");
        assert!(r.iter().all(|x| loose.contains(x)));
        assert!(loose.contains(&"Joh 15:9".to_string()) && !r.contains(&"Joh 15:9".to_string()));
        assert!(find("hgaphsen").contains(&"Joh 3:16".to_string())); // Beta Code
        // wildcards and phrases
        assert!(find("ηγαπ*").len() > r.len());
        assert!(find("\"ὁ θεὸς τὸν κόσμον\"").contains(&"Joh 3:16".to_string()));
        // lemma + morphology: every aorist form of ἀγαπάω
        let aor = find("ἀγαπάω@via*");
        assert!(aor.contains(&"Joh 3:16".to_string()));
        assert!(find("ἀγαπάω@*").len() > aor.len());
        assert!(find("ἀγαπάω@n*").is_empty() || !find("ἀγαπάω@n*").contains(&"Joh 3:16".to_string()));
        // Strong's number
        assert!(find("G25").contains(&"Joh 3:16".to_string()));
        assert_eq!(find("G25").len(), find("ἀγαπάω@*").len());
        // strict: the accented form excludes unaccented-only matches; OR mode unions
        assert!(find("λόγος").len() <= find("λογος").len());
        let q = parse("/ ἠγάπησεν ἐπίστευσεν", Script::Greek, Some(&strongs)).unwrap();
        assert!(idx.search(&byz, &q, 5000).len() > r.len());
        // an unknown Strong's number is an error, not silence
        assert!(parse("G99999", Script::Greek, Some(&strongs)).is_err());
    }

    #[test]
    fn raw_folding_matches_unicode_folding() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        for (id, sc) in [("byz", Script::Greek), ("wtt", Script::Hebrew)] {
            let m = open(&db, id);
            for i in (0..m.verse_count()).step_by(7) {
                for w in m.raw(i).unwrap().split_whitespace() {
                    let via_unicode: Vec<String> = decode(sc, w, false).split(MAQQEF).map(|s| fold_loose(s, false)).filter(|s| !s.is_empty()).collect();
                    assert_eq!(raw_subs(sc, w), via_unicode, "{id} verse {i} word {w}");
                }
            }
        }
    }

    #[test]
    fn septuagint_index_is_usable() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        let (bgt, bgm) = (open(&db, "bgt"), open(&db, "bgm"));
        let t = std::time::Instant::now();
        let idx = Index::build(&bgt, Script::Greek, Some(&bgm));
        println!("bgt index built in {:?}", t.elapsed());
        let q = parse("\"ἐν ἀρχῇ ἐποίησεν\"", Script::Greek, None).unwrap();
        let hits = idx.search(&bgt, &q, 100);
        assert_eq!(refs(&bgt, &hits).first().map(String::as_str), Some("Gen 1:1"));
    }

    #[test]
    fn hebrew_search_on_the_real_text() {
        let Some(dir) = crate::install_dir() else { return };
        let db = dir.join("databases");
        let strongs = Strongs::open(&db).unwrap();
        let (wtt, wtm) = (open(&db, "wtt"), open(&db, "wtm"));
        let idx = Index::build(&wtt, Script::Hebrew, Some(&wtm));
        let find = |q: &str| refs(&wtt, &idx.search(&wtt, &parse(q, Script::Hebrew, Some(&strongs)).unwrap(), 20000));
        assert!(find("בראשית").contains(&"Gen 1:1".to_string())); // typed without points
        assert!(find("בְּרֵאשִׁית").contains(&"Gen 1:1".to_string())); // typed with points
        assert!(find("\"בראשית ברא אלהים\"").contains(&"Gen 1:1".to_string()));
        assert!(find("ברא@vq*").contains(&"Gen 1:1".to_string())); // Qal forms of ברא
        assert!(find("ברא@vqp3ms").contains(&"Gen 1:1".to_string()));
        assert!(!find("ברא@vh*").contains(&"Gen 1:1".to_string())); // not Hiphil
        assert!(find("H7225").contains(&"Gen 1:1".to_string())); // reshit
        assert!(find("H1254").len() >= find("ברא@vq*").len().min(1));
        // a compound joined by maqqef is searchable by either part: Psalm 3:2 has מה־רבו
        assert!(find("רבו").contains(&"Psa 3:2".to_string()));
        // Westminster letters
        assert!(find("BR)").contains(&"Gen 1:1".to_string()));
    }
}
