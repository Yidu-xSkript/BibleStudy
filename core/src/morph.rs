//! Reading the morphology modules (`lemma@code` per word) and turning codes into plain language.
//!
//! Greek: BibleWorks scheme (BNM/BLM/BGM) and the modified scheme (BYM/SCM/TIM/STM/WHM).
//! Hebrew: Westminster WTM, where a word is one or more morphemes joined by `_`.
// ponytail: Friberg (GNM) codes and Targum/Peshitta schemes are not decoded; their words show the
// lemma and raw code.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GreekScheme {
    BibleWorks,
    Modified,
    /// Friberg's Analytical Greek New Testament (GNM): positional codes such as `viaa--3s`, `n-nm-s`.
    Friberg,
}

pub fn greek_scheme(morph_id: &str) -> Option<GreekScheme> {
    match morph_id {
        "bnm" | "blm" | "bgm" => Some(GreekScheme::BibleWorks),
        "bym" | "scm" | "tim" | "stm" | "whm" => Some(GreekScheme::Modified),
        "gnm" => Some(GreekScheme::Friberg),
        _ => None,
    }
}

/// One `lemma@code` unit. `sep` is `@` (Hebrew, Greek) or `%` (Aramaic).
#[derive(Clone, Debug, PartialEq)]
pub struct Morpheme {
    pub lemma: String,
    pub code: String,
}

/// Split a morphology word into morphemes (`_`-joined for Hebrew).
pub fn morphemes(word: &str) -> Vec<Morpheme> {
    word.split('_')
        .filter(|m| !m.is_empty())
        .map(|m| match m.find(['@', '%']) {
            Some(p) => Morpheme { lemma: m[..p].to_string(), code: m[p + 1..].to_string() },
            None => Morpheme { lemma: m.to_string(), code: String::new() },
        })
        .collect()
}

fn pick<'a>(c: Option<char>, table: &[(char, &'a str)]) -> Option<&'a str> {
    c.and_then(|c| table.iter().find(|(k, _)| *k == c).map(|(_, v)| *v))
}

const CASE: &[(char, &str)] = &[('n', "nominative"), ('g', "genitive"), ('d', "dative"), ('a', "accusative"), ('v', "vocative")];
const GENDER: &[(char, &str)] = &[('m', "masculine"), ('f', "feminine"), ('n', "neuter")];
const NUMBER: &[(char, &str)] = &[('s', "singular"), ('p', "plural")];
const TENSE: &[(char, &str)] = &[('p', "present"), ('f', "future"), ('a', "aorist"), ('i', "imperfect"), ('x', "perfect"), ('y', "pluperfect"), ('z', "future perfect"), ('v', "adverbial imperative")];
const VOICE: &[(char, &str)] = &[
    ('a', "active"), ('m', "middle"), ('p', "passive"), ('e', "middle/passive"), ('d', "middle deponent"), ('o', "passive deponent"),
    ('n', "middle or passive deponent"), ('q', "impersonal active"), ('x', "no voice stated"),
];
const MOOD: &[(char, &str)] = &[('i', "indicative"), ('d', "imperative"), ('s', "subjunctive"), ('o', "optative")];
const PRONOUN: &[(char, &str)] = &[
    ('p', "personal"), ('r', "relative"), ('d', "demonstrative"), ('q', "interrogative"), ('i', "indefinite"), ('t', "intensive"),
    ('x', "reflexive"), ('e', "reciprocal"), ('c', "correlative"), ('o', "correlative or interrogative"), ('s', "possessive"),
];
const ADJ_TYPE: &[(char, &str)] = &[('n', "normal"), ('s', "possessive"), ('d', "demonstrative"), ('q', "interrogative"), ('i', "indefinite"), ('t', "intensive"), ('c', "cardinal number"), ('o', "ordinal number"), ('m', "numeral")];
const DEGREE: &[(char, &str)] = &[('c', "comparative"), ('s', "superlative")];

fn person(c: Option<char>) -> Option<&'static str> {
    pick(c, &[('1', "1st person"), ('2', "2nd person"), ('3', "3rd person")])
}

/// Plain-language parsing of a Greek code, e.g. `viaa3s` -> "verb, indicative, aorist, active, 3rd person, singular".
pub fn greek_parse(code: &str, scheme: GreekScheme) -> String {
    if scheme == GreekScheme::Friberg {
        return friberg_parse(code);
    }
    code.split('/').map(|c| greek_one(c, scheme)).collect::<Vec<_>>().join(" or ")
}

const FR_MOOD: &[(char, &str)] = &[('i', "indicative"), ('s', "subjunctive"), ('o', "optative"), ('m', "imperative"), ('n', "infinitive"), ('p', "participle"), ('r', "participle (imperative sense)")];
const FR_TENSE: &[(char, &str)] = &[('p', "present"), ('i', "imperfect"), ('f', "future"), ('a', "aorist"), ('r', "perfect"), ('l', "pluperfect")];
const FR_ADJ: &[(char, &str)] = &[('c', "cardinal"), ('o', "ordinal"), ('r', "relative"), ('i', "indefinite"), ('t', "interrogative"), ('d', "demonstrative"), ('m', "comparative"), ('s', "superlative")];

/// Friberg codes. Alternatives are joined by `/` ("or"), `!` (parsing depends on punctuation or accent),
/// `&` (crasis) and `^` ("functions as"); a leading or trailing `+` marks a periphrastic or
/// otherwise paired form and a leading `-` a relative pronoun without antecedent.
pub fn friberg_parse(code: &str) -> String {
    for (sep, word) in [('/', " or "), ('!', " or, depending on the accent or punctuation, "), ('&', " + "), ('^', ", functioning as ")] {
        if code.contains(sep) {
            return code.split(sep).map(friberg_parse).collect::<Vec<_>>().join(word);
        }
    }
    let periphrastic = code.starts_with('+') || code.ends_with('+');
    let no_antecedent = code.starts_with('-');
    let body = code.trim_matches(|c| c == '+' || c == '-');
    let cs: Vec<char> = body.chars().collect();
    let at = |i: usize| cs.get(i).copied();
    let mut parts: Vec<&str> = Vec::new();
    let mut push = |v: Option<&'static str>| {
        if let Some(v) = v {
            parts.push(v)
        }
    };
    let name = match at(0) {
        Some('n') => {
            push(pick(at(2), CASE));
            push(pick(at(3), GENDER));
            push(person(at(4)));
            push(pick(at(5), NUMBER));
            if at(1) == Some('p') { "pronoun" } else { "noun" }
        }
        Some('v') => {
            push(pick(at(1), FR_MOOD));
            push(pick(at(2), FR_TENSE));
            push(pick(at(3), VOICE));
            push(pick(at(4), CASE));
            push(pick(at(5), GENDER));
            push(person(at(6)));
            push(pick(at(7), NUMBER));
            "verb"
        }
        Some('a') => {
            push(match at(1) {
                Some('p') => Some("pronominal"),
                Some('b') => Some("adverb"),
                _ => None,
            });
            push(pick(at(2), FR_ADJ));
            push(pick(at(3), CASE));
            push(pick(at(4), GENDER));
            push(person(at(5)));
            push(pick(at(6), NUMBER));
            "adjective"
        }
        Some('d') => {
            push(pick(at(1), CASE));
            push(pick(at(2), GENDER));
            push(pick(at(3), NUMBER));
            "article"
        }
        Some('p') => {
            push(pick(at(1), &[('g', "with genitive"), ('d', "with dative"), ('a', "with accusative")]));
            "preposition"
        }
        Some('c') => {
            push(pick(at(1), &[('c', "coordinating"), ('h', "superordinating"), ('s', "subordinating")]));
            "conjunction"
        }
        Some('q') => {
            push(pick(at(1), &[('n', "negative"), ('s', "sentence"), ('t', "interrogative"), ('v', "verbal")]));
            "particle"
        }
        _ => return code.to_string(),
    };
    let mut s = name.to_string();
    for p in parts {
        s.push_str(", ");
        s.push_str(p);
    }
    if periphrastic {
        s.push_str(" (periphrastic or paired form)");
    }
    if no_antecedent {
        s.push_str(" (without antecedent)");
    }
    s
}

fn greek_one(code: &str, scheme: GreekScheme) -> String {
    let mut ch = code.chars();
    let Some(pos) = ch.next() else { return String::new() };
    let rest: Vec<char> = ch.collect();
    let at = |i: usize| rest.get(i).copied();
    let mut parts: Vec<&str> = Vec::new();
    let mut push = |v: Option<&'static str>| {
        if let Some(v) = v {
            parts.push(v)
        }
    };
    let name = match pos {
        'n' => {
            push(pick(at(0), CASE));
            push(pick(at(1), GENDER));
            push(pick(at(2), NUMBER));
            push(pick(at(3), &[('c', "common"), ('p', "proper name")]));
            "noun"
        }
        'r' => {
            push(pick(at(0), PRONOUN));
            push(pick(at(1), CASE));
            push(pick(at(2), GENDER));
            push(pick(at(3), NUMBER));
            "pronoun"
        }
        'd' => {
            push(pick(at(0), CASE));
            push(pick(at(1), GENDER));
            push(pick(at(2), NUMBER));
            "article"
        }
        'v' => match at(0) {
            Some('p') => {
                push(Some("participle"));
                push(pick(at(1), TENSE));
                push(pick(at(2), VOICE));
                push(pick(at(3), CASE));
                push(pick(at(4), GENDER));
                push(pick(at(5), NUMBER));
                "verb"
            }
            Some('n') => {
                push(Some("infinitive"));
                push(pick(at(1), TENSE));
                push(pick(at(2), VOICE));
                "verb"
            }
            _ => {
                push(pick(at(0), MOOD));
                push(pick(at(1), TENSE));
                push(pick(at(2), VOICE));
                push(person(at(3)));
                push(pick(at(4), NUMBER));
                "verb"
            }
        },
        'a' => {
            let o = if scheme == GreekScheme::BibleWorks { 1 } else { 0 };
            if o == 1 {
                push(pick(at(0), ADJ_TYPE));
            }
            push(pick(at(o), CASE));
            push(pick(at(o + 1), GENDER));
            push(pick(at(o + 2), NUMBER));
            push(pick(at(o + 3), DEGREE));
            "adjective"
        }
        'b' => {
            push(pick(at(0), &[('c', "comparative"), ('s', "superlative"), ('i', "interrogative")]));
            "adverb"
        }
        'c' => {
            push(pick(at(0), &[('s', "subordinate"), ('c', "coordinate")]));
            "conjunction"
        }
        'p' => {
            push(pick(at(0), &[('g', "with genitive"), ('d', "with dative"), ('a', "with accusative")]));
            "preposition"
        }
        'q' => {
            push(pick(at(0), &[('c', "contracted"), ('o', "not contracted")]));
            "conjunctive particle"
        }
        'x' => {
            push(pick(at(0), &[('i', "interrogative"), ('n', "negative"), ('o', "other")]));
            "particle"
        }
        't' => {
            push(pick(at(0), &[('a', "Aramaic"), ('h', "Hebrew"), ('p', "proper noun"), ('n', "numeral"), ('l', "letter"), ('o', "other")]));
            "indeclinable"
        }
        'i' => "interjection",
        'z' => "unknown lemma",
        _ => return code.to_string(),
    };
    let mut s = name.to_string();
    for p in parts {
        s.push_str(", ");
        s.push_str(p);
    }
    s
}

const HEB_STEM: &[(char, &str)] = &[
    ('q', "Qal"), ('p', "Piel"), ('u', "Pual"), ('n', "Niphal"), ('h', "Hiphil"), ('o', "Hophal"), ('t', "Hithpael"), ('s', "Hishtaphel"),
    ('r', "Hothpaal"), ('v', "Hithpolel"), ('w', "Hithpalpel"), ('x', "Nithpael"), ('a', "Palel"), ('b', "Pealal"), ('c', "Pilel"),
    ('d', "Pilpel"), ('e', "Polel"), ('f', "Polal"), ('g', "Polpal"), ('i', "Pulal"), ('y', "Qal passive"), ('k', "Poel"), ('l', "Poal"),
    ('m', "Tiphil"),
    // Aramaic stems are capitals
    ('A', "Aphel"), ('B', "Haphel"), ('H', "Hishtaphel"), ('E', "Hithaphel"), ('S', "Hithpaal"), ('F', "Hithpeel"), ('G', "Hithpolel"),
    ('T', "Hithpalpel"), ('I', "Ishtaphel"), ('J', "Ithpeel"), ('P', "Polel"), ('R', "Shaphel"), ('N', "Peal"), ('M', "Pael"),
    ('O', "Peil"), ('D', "Hophal"), ('Q', "Saphal"), ('K', "Ithpaal"), ('V', "Ithpoel"), ('L', "Itpeel"), ('W', "Tiphel"), ('C', "Hephal"),
];
const HEB_ASPECT: &[(char, &str)] = &[
    ('p', "perfect"), ('i', "imperfect"), ('v', "imperative"), ('c', "infinitive construct"), ('a', "infinitive absolute"), ('P', "participle"),
    ('s', "qal passive participle"), ('w', "waw-consecutive imperfect"), ('q', "waw-consecutive perfect"),
];
const HEB_GENDER: &[(char, &str)] = &[('m', "masculine"), ('f', "feminine"), ('b', "masculine or feminine"), ('c', "common")];
const HEB_NUMBER: &[(char, &str)] = &[('s', "singular"), ('p', "plural"), ('d', "dual")];
const HEB_STATE: &[(char, &str)] = &[('c', "construct"), ('a', "absolute"), ('d', "determined")];
const HEB_PARTICLE: &[(char, &str)] = &[
    ('a', "article"), ('s', "article with inseparable preposition"), ('c', "conjunction"), ('d', "adverb"), ('g', "interrogative"),
    ('i', "interjection"), ('n', "negative"), ('o', "direct object marker"), ('p', "preposition"), ('r', "relative"),
];

/// Plain-language parsing of one Westminster morpheme code, e.g. `vqp3ms+Sxxx...`.
pub fn hebrew_parse(code: &str) -> String {
    let (primary, secondary) = code.split_once('+').unwrap_or((code, ""));
    let cs: Vec<char> = primary.chars().collect();
    let at = |i: usize| cs.get(i).copied();
    let mut parts: Vec<&str> = Vec::new();
    let mut push = |v: Option<&'static str>| {
        if let Some(v) = v {
            parts.push(v)
        }
    };
    let name = match at(0) {
        Some('n') => {
            push(pick(at(1), &[('c', "common"), ('p', "proper name"), ('g', "gentilic")]));
            push(pick(at(2), HEB_GENDER));
            push(pick(at(3), HEB_NUMBER));
            push(pick(at(4), HEB_STATE));
            "noun"
        }
        Some('P') => {
            push(pick(at(1), HEB_PARTICLE));
            "particle"
        }
        Some('p') => {
            push(pick(at(1), &[('i', "independent"), ('q', "interrogative"), ('d', "demonstrative"), ('r', "relative")]));
            push(person(at(2)));
            push(pick(at(3), HEB_GENDER));
            push(pick(at(4), HEB_NUMBER));
            "pronoun"
        }
        Some('a') => {
            push(pick(at(1), HEB_GENDER));
            push(pick(at(2), HEB_NUMBER));
            push(pick(at(3), HEB_STATE));
            "adjective"
        }
        Some('u') => {
            push(pick(at(1), &[('c', "cardinal"), ('o', "ordinal")]));
            push(pick(at(2), HEB_GENDER));
            push(pick(at(3), HEB_NUMBER));
            push(pick(at(4), HEB_STATE));
            "numeral"
        }
        Some('x') => "paragraph marker",
        Some('v') => {
            push(pick(at(1), HEB_STEM));
            push(pick(at(2), HEB_ASPECT));
            match at(2) {
                Some('P') | Some('s') => {
                    push(pick(at(3), HEB_GENDER));
                    push(pick(at(4), HEB_NUMBER));
                    push(pick(at(5), HEB_STATE));
                }
                Some('v') => {
                    push(pick(at(3), HEB_GENDER));
                    push(pick(at(4), HEB_NUMBER));
                }
                Some('c') | Some('a') => {}
                _ => {
                    push(person(at(3)));
                    push(pick(at(4), HEB_GENDER));
                    push(pick(at(5), HEB_NUMBER));
                }
            }
            "verb"
        }
        _ => return code.to_string(),
    };
    let mut s = name.to_string();
    for p in parts {
        s.push_str(", ");
        s.push_str(p);
    }
    // Secondary codes: only the pronominal suffix is worth showing, e.g. `Sxxx` = none, `S3ms` = "his".
    let sc: Vec<char> = secondary.chars().collect();
    if sc.first() == Some(&'S') {
        let (p, g, n) = (sc.get(1).copied(), sc.get(2).copied(), sc.get(3).copied());
        if let Some(p) = person(p) {
            s.push_str(&format!(", with {} {} {} suffix", p, pick(g, HEB_GENDER).unwrap_or(""), pick(n, HEB_NUMBER).unwrap_or("")).replace("  ", " "));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friberg_codes() {
        let p = |c: &str| greek_parse(c, GreekScheme::Friberg);
        assert_eq!(p("viaa--3s"), "verb, indicative, aorist, active, 3rd person, singular");
        assert_eq!(p("vppanm-s"), "verb, participle, present, active, nominative, masculine, singular");
        assert_eq!(p("vnaa"), "verb, infinitive, aorist, active");
        assert_eq!(p("n-nm-s"), "noun, nominative, masculine, singular");
        assert_eq!(p("npgm3s"), "pronoun, genitive, masculine, 3rd person, singular");
        assert_eq!(p("a--nm-s"), "adjective, nominative, masculine, singular");
        assert_eq!(p("ab"), "adjective, adverb");
        assert_eq!(p("dnms"), "article, nominative, masculine, singular");
        assert_eq!(p("pa"), "preposition, with accusative");
        assert_eq!(p("cs"), "conjunction, subordinating");
        assert_eq!(p("qn"), "particle, negative");
        assert_eq!(p("n-nm-s/n-am-s"), "noun, nominative, masculine, singular or noun, accusative, masculine, singular");
        assert!(p("+vppanm-s").ends_with("(periphrastic or paired form)"));
        assert!(p("-npnm3s").ends_with("(without antecedent)"));
        assert_eq!(greek_scheme("gnm"), Some(GreekScheme::Friberg));
    }

    /// Every code the Friberg module actually uses decodes to something other than the bare code.
    #[test]
    fn every_friberg_code_in_the_real_module_decodes() {
        let Some(dir) = crate::install_dir() else { return };
        let m = crate::module::Module::open(dir.join("databases/gnm.dbx")).unwrap();
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..m.verse_count() {
            for w in m.raw(i).unwrap().split_whitespace() {
                if let Some(c) = morphemes(w).first().map(|x| x.code.clone()) {
                    seen.insert(c);
                }
            }
        }
        let undecoded: Vec<&String> = seen.iter().filter(|c| friberg_parse(c) == **c).collect();
        assert!(seen.len() > 1000);
        assert!(undecoded.is_empty(), "{undecoded:?}");
    }

    #[test]
    fn greek_codes() {
        use GreekScheme::*;
        assert_eq!(greek_parse("viaa3s", BibleWorks), "verb, indicative, aorist, active, 3rd person, singular");
        assert_eq!(greek_parse("ndfsc", BibleWorks), "noun, dative, feminine, singular, common");
        assert_eq!(greek_parse("dnms", BibleWorks), "article, nominative, masculine, singular");
        assert_eq!(greek_parse("pd", BibleWorks), "preposition, with dative");
        assert_eq!(greek_parse("p", Modified), "preposition");
        assert_eq!(greek_parse("vpaanmp", BibleWorks), "verb, participle, aorist, active, nominative, masculine, plural");
        assert_eq!(greek_parse("vnpa", Modified), "verb, infinitive, present, active");
        assert_eq!(greek_parse("nnfs", Modified), "noun, nominative, feminine, singular");
        assert_eq!(greek_parse("annmsn", BibleWorks), "adjective, normal, nominative, masculine, singular");
        assert_eq!(greek_parse("anmsn", Modified), "adjective, nominative, masculine, singular");
        assert_eq!(greek_parse("cc", BibleWorks), "conjunction, coordinate");
        assert_eq!(greek_parse("nnms/ndms", Modified), "noun, nominative, masculine, singular or noun, dative, masculine, singular");
    }

    #[test]
    fn hebrew_codes() {
        assert_eq!(hebrew_parse("ncfsa+SxxxExHxNxRx"), "noun, common, feminine, singular, absolute");
        assert_eq!(hebrew_parse("vqp3ms+SxxxxJxCxAxExHaNxRx"), "verb, Qal, perfect, 3rd person, masculine, singular");
        assert_eq!(hebrew_parse("Pp+SxxxExHxNxRx"), "particle, preposition");
        assert_eq!(hebrew_parse("vqPmsa"), "verb, Qal, participle, masculine, singular, absolute");
        assert_eq!(hebrew_parse("vhv"), "verb, Hiphil, imperative");
        assert_eq!(hebrew_parse("ncmsc+S3ms"), "noun, common, masculine, singular, construct, with 3rd person masculine singular suffix");
    }

    #[test]
    fn splits_words() {
        let m = morphemes("B.:@Pp+SxxxExHxNxRx_R\")$IYT@ncfsa+SxxxExHxNxRx");
        assert_eq!(m.len(), 2);
        assert_eq!(m[1], Morpheme { lemma: "R\")$IYT".into(), code: "ncfsa+SxxxExHxNxRx".into() });
        assert_eq!(morphemes("a)rxh/@ndfsc")[0].lemma, "a)rxh/");
        assert_eq!(greek_scheme("bgm"), Some(GreekScheme::BibleWorks));
    }
}
