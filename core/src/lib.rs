pub mod canon;
pub mod greek;
pub mod hebrew;
mod hebrew_table;
pub mod lexenc;
pub mod lexicon;
pub mod lists;
pub mod module;
pub mod morph;
pub mod notes;
pub mod osearch;
pub mod script;
pub mod search;
pub mod store;
pub mod tsk;
pub mod versification;
pub mod vmap;
pub mod wordlist;

use std::path::PathBuf;

/// The user's BibleWorks 8 install: `$BW8_DIR` or the default location, if it exists.
pub fn install_dir() -> Option<PathBuf> {
    let p = std::env::var_os("BW8_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\BibleWorks 8"));
    p.join("databases").is_dir().then_some(p)
}
