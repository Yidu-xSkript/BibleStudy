//! User data (notes, lists) as small JSON files in the app's data folder.
//! Saves go to a temp file and are renamed into place, so a crash can't leave a half-written file.

use serde::{de::DeserializeOwned, Serialize};
use std::{fs, io, path::Path};

/// Load `path`, or the default when it is missing. A file that won't parse is kept as `<name>.bad`
/// (never silently overwritten) and the default is used.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
            let _ = fs::rename(path, path.with_extension("json.bad"));
            T::default()
        }),
        Err(_) => T::default(),
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(io::Error::other)?)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bs-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d.join("data.json")
    }

    #[test]
    fn roundtrip_missing_and_corrupt() {
        let p = temp("rt");
        assert!(load::<BTreeMap<String, u32>>(&p).is_empty()); // missing -> default
        let mut m = BTreeMap::new();
        m.insert("a".to_string(), 1u32);
        save(&p, &m).unwrap();
        assert_eq!(load::<BTreeMap<String, u32>>(&p), m);
        fs::write(&p, "{ not json").unwrap();
        assert!(load::<BTreeMap<String, u32>>(&p).is_empty());
        assert!(p.with_extension("json.bad").exists(), "corrupt file is kept, not lost");
    }
}
