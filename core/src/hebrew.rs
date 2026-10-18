//! Westminster/Michigan-Claremont Hebrew (as used in BibleWorks WTT/WTM) -> Unicode.
//! Codes follow Groves' supplement to the Michigan Old Testament code manual.
//!
//! Letters are ASCII (`)`=aleph ... `$`=shin `&`=sin), vowels `A F E " I O U :`, `.` dagesh,
//! `-` maqqef, `_` and `/` morpheme boundaries (dropped), and two-digit codes are cantillation.
//! Final forms (ך ם ן ף ץ) are derived: the file stores the plain letter.

use unicode_normalization::UnicodeNormalization;

fn consonant(c: char) -> Option<(char, &'static str)> {
    Some(match c {
        ')' => ('א', ""),
        'B' => ('ב', ""),
        'G' => ('ג', ""),
        'D' => ('ד', ""),
        'H' => ('ה', ""),
        'W' => ('ו', ""),
        'Z' => ('ז', ""),
        'X' => ('ח', ""),
        '+' => ('ט', ""),
        'Y' => ('י', ""),
        'K' => ('כ', ""),
        'L' => ('ל', ""),
        'M' => ('מ', ""),
        'N' => ('נ', ""),
        'S' => ('ס', ""),
        '(' => ('ע', ""),
        'P' => ('פ', ""),
        'C' => ('צ', ""),
        'Q' => ('ק', ""),
        'R' => ('ר', ""),
        'T' => ('ת', ""),
        '$' => ('ש', "\u{5C1}"),
        '&' => ('ש', "\u{5C2}"),
        '#' => ('ש', ""), // unmarked shin (e.g. the second letter of Issachar)
        _ => return None,
    })
}

fn vowel(c: char) -> Option<char> {
    Some(match c {
        'A' => '\u{5B7}',
        'F' => '\u{5B8}',
        'E' => '\u{5B6}',
        '"' => '\u{5B5}',
        'I' => '\u{5B4}',
        'U' => '\u{5BB}',
        _ => return None,
    })
}

/// Cantillation code -> mark. `(mark, always)`: meteg/silluq-type marks survive "vowels only".
fn accent(code: u32) -> Option<(char, bool)> {
    Some(match code {
        0 => ('\u{5C3}', true), // sof pasuq
        1 => ('\u{592}', false),
        2 => ('\u{598}', false),
        3 => ('\u{599}', false),
        4 | 24 => ('\u{5A9}', false),
        5 => ('\u{5C0}', true), // paseq
        10 => ('\u{59A}', false),
        11 => ('\u{59D}', false),
        13 => ('\u{5AD}', false),
        14 | 44 => ('\u{5A0}', false),
        33 => ('\u{599}', false),
        35 | 75 | 95 => ('\u{5BD}', true), // meteg / silluq
        52 => ('\u{5C4}', true), // puncta extraordinaria, above
        53 => ('\u{5C5}', true), // below
        60 => ('\u{5AB}', false),
        61 => ('\u{59C}', false),
        62 => ('\u{59E}', false),
        63 => ('\u{5A8}', false),
        64 => ('\u{5AC}', false),
        65 => ('\u{593}', false),
        70 => ('\u{5A4}', false),
        71 => ('\u{5A5}', false),
        72 => ('\u{5A6}', false),
        73 => ('\u{596}', false),
        74 => ('\u{5A3}', false),
        80 => ('\u{594}', false),
        81 => ('\u{597}', false),
        82 => ('\u{5AE}', false),
        83 => ('\u{5A1}', false),
        84 => ('\u{59F}', false),
        85 => ('\u{595}', false),
        91 => ('\u{59B}', false),
        92 => ('\u{591}', false),
        93 => ('\u{5AA}', false),
        94 => ('\u{5A7}', false),
        _ => return None,
    })
}

fn final_form(c: char) -> char {
    match c {
        'כ' => 'ך',
        'מ' => 'ם',
        'נ' => 'ן',
        'פ' => 'ף',
        'צ' => 'ץ',
        other => other,
    }
}

#[derive(Default)]
struct Letter {
    base: char,
    marks: String,
}

fn flush(letters: &mut Vec<Letter>, out: &mut String) {
    let n = letters.len();
    for (k, l) in letters.drain(..).enumerate() {
        out.push(if k + 1 == n { final_form(l.base) } else { l.base });
        out.push_str(&l.marks);
    }
}

/// Two digits at `cs[i..]` as an accent code.
fn code_at(cs: &[char], i: usize) -> Option<u32> {
    match (cs.get(i)?.to_digit(10), cs.get(i + 1)?.to_digit(10)) {
        (Some(a), Some(b)) => Some(a * 10 + b),
        _ => None,
    }
}

/// `cantillation = false` keeps vowels, meteg and sof pasuq but drops the other accents.
pub fn hebrew(raw: &str, cantillation: bool) -> String {
    let mut out = String::new();
    let mut letters: Vec<Letter> = Vec::new();
    let cs: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if let Some((base, dot)) = consonant(c) {
            letters.push(Letter { base, marks: dot.to_string() });
        } else if let Some(v) = vowel(c) {
            if let Some(l) = letters.last_mut() {
                l.marks.push(v);
            }
        } else if c == ':' {
            let (m, skip) = match cs.get(i + 1) {
                Some('A') => ('\u{5B2}', 1),
                Some('E') => ('\u{5B1}', 1),
                Some('F') => ('\u{5B3}', 1),
                _ => ('\u{5B0}', 0),
            };
            if let Some(l) = letters.last_mut() {
                l.marks.push(m);
            }
            i += skip;
        } else if c == '.' {
            if let Some(l) = letters.last_mut() {
                l.marks.push('\u{5BC}');
            }
        } else if c == 'O' {
            // Holem. `OW` is holem male: the dot sits on the vav (accents may sit between O and W).
            let mut j = i + 1;
            let mut accents = String::new();
            while let Some(code) = code_at(&cs, j) {
                if let Some((m, always)) = accent(code) {
                    if always || cantillation {
                        accents.push(m);
                    }
                }
                j += 2;
            }
            let carries_own_vowel = cs.get(j + 1).map_or(false, |n| vowel(*n).is_some() || matches!(n, 'O' | ':' | '.'));
            if cs.get(j) == Some(&'W') && !carries_own_vowel {
                letters.push(Letter { base: 'ו', marks: format!("\u{5B9}{accents}") });
                i = j;
            } else if let Some(l) = letters.last_mut() {
                l.marks.push('\u{5B9}');
            }
        } else if c.is_ascii_digit() {
            if let Some(code) = code_at(&cs, i) {
                if let Some((m, always)) = accent(code) {
                    if always || cantillation {
                        match letters.last_mut() {
                            Some(l) => l.marks.push(m),
                            None => out.push(m),
                        }
                    }
                }
                i += 1;
            }
        } else if c == '-' {
            flush(&mut letters, &mut out);
            out.push('־');
        } else if matches!(c, '_' | '/' | '*' | '?' | '^') {
            // morpheme boundaries, ketiv/qere and line-end markers carry no text
        } else {
            flush(&mut letters, &mut out);
            out.push(c);
        }
        i += 1;
    }
    flush(&mut letters, &mut out);
    out.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(s: &str) -> String {
        s.nfc().collect()
    }

    const GEN_1_1: &str = "B.:_R\")$I73YT B.FRF74) ):ELOHI92YM )\"71T HA_$.FMA73YIM W:_)\"71T HF_)F75REC";

    #[test]
    fn genesis_1_1_vowels_only() {
        assert_eq!(hebrew(GEN_1_1, false), nfc("בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָֽרֶץ"));
    }

    #[test]
    fn genesis_1_1_with_cantillation() {
        let h = hebrew(GEN_1_1, true);
        assert_eq!(h, nfc("בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ"));
    }

    #[test]
    fn holem_male_shureq_and_maqqef() {
        assert_eq!(hebrew("MOWT", false), nfc("מוֹת")); // holem male on the vav
        assert_eq!(hebrew("YO74WM", false), nfc("יוֹם")); // accent between O and W
        assert_eq!(hebrew("(AL-_KFL-", false), nfc("עַל־כָל־"));
        assert_eq!(hebrew("W.", false), nfc("וּ")); // shureq
        assert_eq!(hebrew("YI&:RF)\"L", false), nfc("יִשְׂרָאֵל")); // sin dot, final lamed unchanged
    }

    #[test]
    fn final_letters() {
        assert_eq!(hebrew("MELEK:", false), nfc("מֶלֶךְ"));
        assert_eq!(hebrew("(OWLFM", false), nfc("עוֹלָם"));
    }
}
