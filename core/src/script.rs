//! Which writing system a BibleWorks version id uses, and how to turn its raw text into Unicode.
// ponytail: explicit id lists (BibleWorks has no language flag in its version table); add ids here
// as more original-language modules are supported.

use crate::{greek::greek, hebrew::hebrew};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Latin,
    Greek,
    Hebrew,
}

/// Greek text modules and their `lemma@code` morphology companions.
const GREEK_TEXT: &[&str] = &["byz", "bgt", "bnt", "gnt", "scr", "tis", "ste", "who", "goc", "lxt"];
const GREEK_MORPH: &[&str] = &["bym", "bgm", "bnm", "gnm", "scm", "tim", "stm", "whm", "blm"];
const HEBREW_TEXT: &[&str] = &["wtt"];
const HEBREW_MORPH: &[&str] = &["wtm"];

pub fn script_of(id: &str) -> Script {
    if GREEK_TEXT.contains(&id) || GREEK_MORPH.contains(&id) {
        Script::Greek
    } else if HEBREW_TEXT.contains(&id) || HEBREW_MORPH.contains(&id) {
        Script::Hebrew
    } else {
        Script::Latin
    }
}

/// `lemma@code` modules: used for parsing, never shown as a reading version.
pub fn is_morph(id: &str) -> bool {
    GREEK_MORPH.contains(&id) || HEBREW_MORPH.contains(&id)
}

/// Original-language versions that are supported as readable text.
pub fn is_original_text(id: &str) -> bool {
    GREEK_TEXT.contains(&id) || HEBREW_TEXT.contains(&id)
}

/// The morphology module paired with a text module (same words, `lemma@code` form).
pub fn morph_companion(id: &str) -> Option<&'static str> {
    Some(match id {
        "byz" => "bym",
        "bgt" => "bgm",
        "bnt" => "bnm",
        "gnt" => "gnm",
        "scr" => "scm",
        "tis" => "tim",
        "ste" => "stm",
        "who" => "whm",
        "lxt" => "blm",
        "wtt" => "wtm",
        _ => return None,
    })
}

pub fn decode(script: Script, raw: &str, cantillation: bool) -> String {
    match script {
        Script::Latin => raw.to_string(),
        Script::Greek => greek(raw),
        Script::Hebrew => hebrew(raw, cantillation),
    }
}
