//! Runs against the real BibleWorks install when present; each test returns early otherwise.
use bible_study_core::{install_dir, module::Module, script};
use unicode_normalization::UnicodeNormalization;

fn starts(actual: &str, expected: &str) -> bool {
    actual.starts_with(&expected.nfc().collect::<String>())
}

fn open(id: &str) -> Option<Module> {
    let dir = install_dir()?;
    Some(Module::open(dir.join(format!("databases/{id}.dbx"))).unwrap_or_else(|e| panic!("{id}: {e}")))
}

/// Decoded text of a verse in `m` (code is the book's BibleWorks code).
fn verse(m: &Module, id: &str, code: &str, c: usize, v: usize) -> String {
    let b = m.vers.book_index(code).unwrap_or_else(|| panic!("{id} has no {code}"));
    let i = m.vers.index(b, c, v).unwrap_or_else(|| panic!("{id} has no {code} {c}:{v}"));
    script::decode(script::script_of(id), m.raw(i).unwrap().trim(), false)
}

fn consonants(s: &str) -> String {
    s.chars().filter(|c| ('\u{5D0}'..='\u{5EA}').contains(c) || *c == ' ').collect()
}

#[test]
fn hebrew_wtt_lines_up_with_hebrew_numbering() {
    let Some(m) = open("wtt") else { return };
    let t = |c: &str, ch, v| consonants(&verse(&m, "wtt", c, ch, v));
    assert!(t("Gen", 1, 1).starts_with("בראשית ברא אלהים את השמים ואת הארץ"));
    assert!(t("Deu", 1, 1).starts_with("אלה הדברים אשר דבר משה"), "{}", t("Deu", 1, 1));
    assert!(t("Psa", 23, 1).starts_with("מזמור לדוד יהוה רעי"), "{}", t("Psa", 23, 1));
    assert!(t("Psa", 3, 1).starts_with("מזמור לדוד"), "{}", t("Psa", 3, 1)); // title is verse 1
    assert!(t("Psa", 3, 2).starts_with("יהוה מהרבו צרי"), "{}", t("Psa", 3, 2));
    assert!(t("Joe", 4, 1).starts_with("כי הנה"), "{}", t("Joe", 4, 1)); // KJV Joel 3
    assert!(t("Mal", 3, 23).contains("הנביא")); // Heb. Malachi ends at 3:24
    assert!(t("Mal", 3, 24).ends_with("הארץ חרם"), "{}", t("Mal", 3, 24));
    assert!(t("Neh", 1, 1).starts_with("דברי נחמיה"), "{}", t("Neh", 1, 1));
}

#[test]
fn greek_byz_and_bgt_new_testament() {
    let Some(byz) = open("byz") else { return };
    assert!(starts(&verse(&byz, "byz", "Mat", 1, 1), "Βίβλος γενέσεως Ἰησοῦ"));
    assert!(starts(&verse(&byz, "byz", "Joh", 3, 16), "Οὕτως γὰρ ἠγάπησεν ὁ θεὸς τὸν κόσμον"));
    assert!(starts(&verse(&byz, "byz", "Rev", 22, 21), "Ἡ χάρις"), "{}", verse(&byz, "byz", "Rev", 22, 21));

    let bgt = open("bgt").unwrap();
    assert!(starts(&verse(&bgt, "bgt", "Gen", 1, 1), "ἐν ἀρχῇ ἐποίησεν ὁ θεὸς"));
    assert!(starts(&verse(&bgt, "bgt", "Joh", 3, 16), "οὕτως γὰρ ἠγάπησεν ὁ θεὸς τὸν κόσμον"), "{}", verse(&bgt, "bgt", "Joh", 3, 16));
    assert!(bgt.vers.book_index("Wis").is_some()); // LXX includes the Apocrypha
}
