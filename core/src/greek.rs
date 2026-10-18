//! BibleWorks Greek (Beta Code) -> Unicode.
//!
//! Lowercase Latin = lowercase Greek, uppercase = capital. `)` smooth `(` rough `/` acute
//! `\` grave `^` circumflex `+` diaeresis `|` iota subscript. A diacritic belongs to the letter
//! before it, or, at the start of a word, to the letter after it (`)Ihsou^` = Ἰησοῦ).
//! `s` is sigma, final (ς) when no letter follows.

use unicode_normalization::UnicodeNormalization;

fn letter(c: char) -> Option<char> {
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
        'c' => 'ξ',
        'o' => 'ο',
        'p' => 'π',
        'r' => 'ρ',
        's' => 'σ',
        't' => 'τ',
        'u' => 'υ',
        'f' => 'φ',
        'x' => 'χ',
        'y' => 'ψ',
        'w' => 'ω',
        'v' => 'ϝ',
        _ => return None,
    };
    Some(if c.is_ascii_uppercase() { g.to_uppercase().next().unwrap() } else { g })
}

fn mark(c: char) -> Option<char> {
    Some(match c {
        ')' => '\u{313}',
        '(' => '\u{314}',
        '/' => '\u{301}',
        '\\' => '\u{300}',
        '^' | '=' => '\u{342}',
        '+' => '\u{308}',
        '|' => '\u{345}',
        _ => return None,
    })
}

struct Cell {
    ch: char,
    marks: String,
    sigma: bool,
}

pub fn greek(raw: &str) -> String {
    let mut cells: Vec<Cell> = Vec::new();
    let mut pending = String::new();
    for c in raw.chars() {
        if let Some(m) = mark(c) {
            match cells.last_mut() {
                Some(last) if last.ch.is_alphabetic() => last.marks.push(m),
                _ => pending.push(m),
            }
        } else if let Some(g) = letter(c) {
            cells.push(Cell { ch: g, marks: std::mem::take(&mut pending), sigma: c == 's' });
        } else {
            let ch = match c {
                ':' => '·',
                '\'' => '’',
                other => other,
            };
            cells.push(Cell { ch, marks: String::new(), sigma: false });
        }
    }
    let mut out = String::new();
    for k in 0..cells.len() {
        let cell = &cells[k];
        let ch = if cell.sigma && !cells.get(k + 1).map_or(false, |n| n.ch.is_alphabetic()) { 'ς' } else { cell.ch };
        // Rho takes its breathing as a precomposed letter.
        if ch == 'ρ' && cell.marks.contains('\u{314}') {
            out.push('ῥ');
        } else if ch == 'ρ' && cell.marks.contains('\u{313}') {
            out.push('ῤ');
        } else {
            out.push(ch);
            out.push_str(&cell.marks);
        }
    }
    out.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nfc(s: &str) -> String {
        s.nfc().collect()
    }

    #[test]
    fn decodes_septuagint_and_byzantine_samples() {
        assert_eq!(greek("e)n a)rxh^| e)poi/hsen o( qeo\\s to\\n ou)rano\\n kai\\ th\\n gh^n"), nfc("ἐν ἀρχῇ ἐποίησεν ὁ θεὸς τὸν οὐρανὸν καὶ τὴν γῆν"));
        assert_eq!(greek("Bi/blos gene/sews )Ihsou^ xristou^, ui(ou^ Daui/d"), nfc("Βίβλος γενέσεως Ἰησοῦ χριστοῦ, υἱοῦ Δαυίδ"));
        assert_eq!(greek("I)hsou^"), nfc("Ἰησοῦ")); // diacritic after a capital
        assert_eq!(greek("Daui+/d"), nfc("Δαυΐδ")); // diaeresis + acute
        assert_eq!(greek("r(h^ma"), nfc("ῥῆμα"));
        assert_eq!(greek("par' au)toi^s"), nfc("παρ’ αὐτοῖς"));
    }

    #[test]
    fn lemmas_and_final_sigma() {
        assert_eq!(greek("qeo/s"), nfc("θεός"));
        assert_eq!(greek("poie/w"), nfc("ποιέω"));
        assert_eq!(greek("sofo/s"), nfc("σοφός")); // initial and final sigma
    }
}
