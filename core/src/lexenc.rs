//! Text encodings used inside BibleWorks lexicon files (not the Beta Code of Bible modules).
//!
//! Greek is a font-map scheme: ASCII letters plus mark characters that follow the letter
//! (`v` acute, `j` smooth, `J` rough, `'` circumflex, `[` smooth+acute ...) and `"` for final sigma;
//! capitals take a prefix mark (`!A` = Ἀ). Worked out by aligning ~5,000 Thayer headwords with the
//! lemmas of the Greek morphology module.
//!
//! Hebrew is stored in visual (right-to-left) order with vowels as separate marks. Only the
//! consonants are decoded here (reversed into reading order): enough to match words to lemmas.
// ponytail: Hebrew vowels and dagesh in lexicon text are not decoded (consonants only), and the
// capital-prefix marks beyond `! @ # $` are left literal.

use unicode_normalization::UnicodeNormalization;

fn greek_letter(c: char) -> Option<char> {
    let g = match c.to_ascii_lowercase() {
        'a' => 'α',
        'b' => 'β',
        'g' => 'γ',
        'd' => 'δ',
        'e' => 'ε',
        'z' => 'ζ',
        'h' => 'η',
        'q' => 'θ',
        'i' => 'ι',
        'k' => 'κ',
        'l' => 'λ',
        'm' => 'μ',
        'n' => 'ν',
        'x' => 'ξ',
        'o' => 'ο',
        'p' => 'π',
        'r' => 'ρ',
        's' => 'σ',
        't' => 'τ',
        'u' => 'υ',
        'f' => 'φ',
        'c' => 'χ',
        'y' => 'ψ',
        'w' => 'ω',
        _ => return None,
    };
    Some(if c.is_ascii_uppercase() { g.to_uppercase().next().unwrap() } else { g })
}

const ACUTE: char = '\u{301}';
const SMOOTH: char = '\u{313}';
const ROUGH: char = '\u{314}';
const CIRC: char = '\u{342}';
const DIAER: char = '\u{308}';
const SUB: char = '\u{345}';

fn suffix_marks(c: char) -> Option<&'static [char]> {
    Some(match c {
        'v' => &[ACUTE],
        'j' => &[SMOOTH],
        'J' => &[ROUGH],
        '\'' => &[CIRC],
        '[' => &[SMOOTH, ACUTE],
        '{' => &[ROUGH, ACUTE],
        '>' => &[DIAER],
        '?' => &[DIAER, ACUTE],
        '\\' => &[SMOOTH, CIRC],
        '|' => &[ROUGH, CIRC],
        '/' => &[SUB],
        _ => return None,
    })
}

fn prefix_marks(c: char) -> Option<&'static [char]> {
    Some(match c {
        '!' => &[SMOOTH],
        '@' => &[ROUGH],
        '#' => &[SMOOTH, ACUTE],
        '$' => &[ROUGH, ACUTE],
        _ => return None,
    })
}

/// Greek in lexicon encoding -> Unicode (NFC).
pub fn lex_greek(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut after_letter = false;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if let (Some(m), Some(next)) = (prefix_marks(c), cs.get(i + 1)) {
            if next.is_ascii_uppercase() && greek_letter(*next).is_some() {
                out.push(greek_letter(*next).unwrap());
                out.extend(m);
                after_letter = true;
                i += 2;
                continue;
            }
        }
        if let Some(g) = greek_letter(c) {
            out.push(g);
            after_letter = true;
        } else if c == '"' {
            out.push('ς');
            after_letter = true;
        } else if let (true, Some(m)) = (after_letter, suffix_marks(c)) {
            out.extend(m);
        } else if c == '±' {
            // vowel-quantity marker (LSJ); no Unicode equivalent worth keeping
        } else {
            out.push(c);
            after_letter = false;
        }
        i += 1;
    }
    out.nfc().collect()
}

fn hebrew_consonant(c: char) -> Option<char> {
    Some(match c {
        'a' | 'A' => 'א',
        'b' | 'B' => 'ב',
        'c' | 'C' | 'v' | 'V' => 'ש', // sin and shin
        'd' | 'D' => 'ד',
        'f' | 'F' => 'ט',
        'g' | 'G' => 'ג',
        'h' | 'H' => 'ה',
        'j' | 'J' => 'ח',
        'k' | 'K' | '&' => 'כ',
        'l' | 'L' => 'ל',
        'm' | 'M' | '!' => 'מ',
        'n' | 'N' | '@' => 'נ',
        'p' | 'P' | '#' => 'פ',
        'q' | 'Q' => 'ק',
        'r' | 'R' => 'ר',
        's' | 'S' => 'ס',
        't' | 'T' => 'ת',
        'w' | 'W' | '/' => 'ו', // `/` is holam male (vav with its vowel)
        'x' | 'X' | '$' => 'צ',
        'y' | 'Y' => 'י',
        'z' | 'Z' => 'ז',
        '[' => 'ע',
        _ => return None,
    })
}

/// Hebrew in lexicon encoding -> its consonants in reading order (final forms as plain letters).
pub fn lex_hebrew_consonants(s: &str) -> String {
    // Spaces are kept so a multi-word run reverses into words in reading order.
    let mut v: Vec<char> = s.chars().filter_map(|c| hebrew_consonant(c).or((c == ' ').then_some(' '))).collect();
    v.reverse();
    v.into_iter().collect::<String>().trim().to_string()
}

/// Consonant skeleton of a Westminster (Michigan-Claremont) lemma, same alphabet as above.
pub fn mc_consonants(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            Some(match c {
                ')' => 'א',
                'B' => 'ב',
                'G' => 'ג',
                'D' => 'ד',
                'H' => 'ה',
                'W' => 'ו',
                'Z' => 'ז',
                'X' => 'ח',
                '+' => 'ט',
                'Y' => 'י',
                'K' => 'כ',
                'L' => 'ל',
                'M' => 'מ',
                'N' => 'נ',
                'S' => 'ס',
                '(' => 'ע',
                'P' => 'פ',
                'C' => 'צ',
                'Q' => 'ק',
                'R' => 'ר',
                '$' | '&' | '#' => 'ש',
                'T' => 'ת',
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(s: &str) -> String {
        s.nfc().collect()
    }

    #[test]
    fn greek_headwords() {
        assert_eq!(lex_greek("ajgapavw"), nfc("ἀγαπάω"));
        assert_eq!(lex_greek("!Aarwvn"), nfc("Ἀαρών"));
        assert_eq!(lex_greek("ajbarhv\""), nfc("ἀβαρής"));
        assert_eq!(lex_greek("a[dolo\""), nfc("ἄδολος"));
        assert_eq!(lex_greek("oJ"), nfc("ὁ"));
        assert_eq!(lex_greek("-w'no\""), nfc("-ῶνος"));
        assert_eq!(lex_greek("lovgo\""), nfc("λόγος"));
        assert_eq!(lex_greek("hjgavphsa"), nfc("ἠγάπησα"));
        assert_eq!(lex_greek("ajga±pavw"), nfc("ἀγαπάω")); // LSJ quantity mark is dropped
    }

    #[test]
    fn hebrew_consonants_in_reading_order() {
        assert_eq!(lex_hebrew_consonants("ba;"), "אב"); // 'ab, stored visually
        assert_eq!(lex_hebrew_consonants("tyviare"), "ראשית"); // re'shiyth
        assert_eq!(mc_consonants("R\")$IYT"), "ראשית");
        assert_eq!(mc_consonants("BR)"), "ברא");
    }
}
