//! How much of the Greek and Hebrew text resolves to Strong's and full-lexicon entries.
//! Runs against the real install; returns early otherwise. Run with `--nocapture` for the rates.
use bible_study_core::{
    greek::greek,
    install_dir,
    lexicon::{Language, Lexicon, Strongs},
    module::Module,
    morph::morphemes,
};
use std::collections::{HashMap, HashSet};

/// Distinct lemmas of a morphology module with how often each occurs.
fn lemmas(m: &Module, hebrew: bool) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for i in 0..m.verse_count() {
        for w in m.raw(i).unwrap().split_whitespace() {
            let ms = morphemes(w);
            // For Hebrew, the word's own lemma is the last morpheme unless it is only a prefix particle.
            let pick = if hebrew { ms.last() } else { ms.first() };
            if let Some(x) = pick {
                *out.entry(x.lemma.clone()).or_insert(0) += 1;
            }
        }
    }
    out
}

fn rate(label: &str, all: &HashMap<String, usize>, ok: impl Fn(&str) -> bool) -> f64 {
    let (mut hit, mut hit_tokens, mut tokens) = (0, 0, 0);
    let mut misses: Vec<(&String, &usize)> = Vec::new();
    for (l, n) in all {
        tokens += n;
        if ok(l) {
            hit += 1;
            hit_tokens += n;
        } else {
            misses.push((l, n));
        }
    }
    misses.sort_by(|a, b| b.1.cmp(a.1));
    let shown: Vec<String> = misses.iter().take(12).map(|(l, n)| format!("{l}({n})")).collect();
    println!("{label}: {hit}/{} lemmas, {:.1}% of words. top misses: {}", all.len(), 100.0 * hit_tokens as f64 / tokens as f64, shown.join(" "));
    hit_tokens as f64 / tokens as f64
}

#[test]
fn greek_and_hebrew_lemmas_find_entries() {
    let Some(dir) = install_dir() else { return };
    let db = dir.join("databases");
    let strongs = Strongs::open(&db).unwrap();

    let bgm = Module::open(db.join("bgm.dbx")).unwrap();
    let g = lemmas(&bgm, false);
    let uni = |l: &str| greek(l);
    let g_strongs = rate("Greek -> Strong's", &g, |l| !strongs.greek_lemma(&uni(l)).is_empty());
    let thayer = Lexicon::open(&db, "thdict", Language::Greek).unwrap();
    let g_thayer = rate("Greek -> Thayer", &g, |l| !thayer.greek(&uni(l)).is_empty());

    let wtm = Module::open(db.join("wtm.dbx")).unwrap();
    let h = lemmas(&wtm, true);
    let h_strongs = rate("Hebrew -> Strong's", &h, |l| !strongs.hebrew_lemma(l).is_empty());
    let bdb = Lexicon::open(&db, "fbdblex", Language::Hebrew).unwrap();
    let h_bdb = rate("Hebrew -> BDB (by consonants)", &h, |l| !bdb.hebrew(l, &[]).is_empty());

    assert!(g_strongs > 0.90, "greek strongs {g_strongs}");
    assert!(g_thayer > 0.85, "greek thayer {g_thayer}");
    assert!(h_strongs > 0.80, "hebrew strongs {h_strongs}");
    assert!(h_bdb > 0.70, "hebrew bdb {h_bdb}");
    let _: HashSet<u8> = HashSet::new();
}
