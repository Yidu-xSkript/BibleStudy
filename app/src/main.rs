#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod library;

use bible_study_core::{
    canon::book_name,
    greek::greek,
    hebrew::hebrew,
    install_dir,
    lexicon::{Language, Lexicon, Span, Strongs, StrongsEntry},
    module::{tokens, Module},
    morph::{greek_parse, greek_scheme, hebrew_parse, morphemes},
    osearch,
    script::{self, Script},
    search,
    vmap::{map_verse, Vmap},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{ipc::Response, State};

const HIT_CAP: usize = 500;
/// Share of high bytes above which a Latin-script module is treated as another encoding (CJK, Cyrillic...).
const MAX_HIGH_RATIO: f32 = 0.06;

struct ModInfo {
    path: PathBuf,
    script: Script,
}

/// Lexicons offered per language: (file id, display name).
const GREEK_LEXICONS: &[(&str, &str)] = &[("thdict", "Thayer"), ("gntdict", "Friberg"), ("ging", "Gingrich"), ("lsdict", "Liddell-Scott"), ("lndict", "Louw-Nida")];
const HEBREW_LEXICONS: &[(&str, &str)] = &[("fbdblex", "BDB"), ("hollex", "Holladay")];
/// Entries shown per lexicon lookup (homographs can be many).
const MAX_ENTRIES: usize = 6;

struct App {
    dir: Option<PathBuf>,
    versions: BTreeMap<String, ModInfo>,
    cache: Mutex<HashMap<String, Arc<Module>>>,
    vmaps: Mutex<HashMap<String, Arc<Vmap>>>,
    strongs: Mutex<Option<Arc<Strongs>>>,
    tsk: Mutex<Option<Arc<bible_study_core::tsk::Tsk>>>,
    lexicons: Mutex<HashMap<String, Arc<Lexicon>>>,
    library: library::Library,
    /// Folded-word indexes for Greek/Hebrew search, built the first time a version is searched.
    oindex: Mutex<HashMap<String, Arc<osearch::Index>>>,
}

impl App {
    /// Any verse module in the install, including morphology companions that aren't listed as versions.
    fn module(&self, id: &str) -> Result<Arc<Module>, String> {
        if let Some(m) = self.cache.lock().unwrap().get(id) {
            return Ok(m.clone());
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err("bad version id".into());
        }
        let path = match self.versions.get(id) {
            Some(info) => info.path.clone(),
            None => self.dir.as_ref().ok_or("BibleWorks not found")?.join("databases").join(format!("{id}.dbx")),
        };
        let m = Arc::new(Module::open(&path).map_err(|e| format!("{id}: {e}"))?);
        self.cache.lock().unwrap().insert(id.to_string(), m.clone());
        Ok(m)
    }

    fn oindex(&self, version: &str) -> Result<Arc<osearch::Index>, String> {
        if let Some(i) = self.oindex.lock().unwrap().get(version) {
            return Ok(i.clone());
        }
        let text = self.module(version)?;
        let morph = script::morph_companion(version).map(|id| self.module(id)).transpose()?;
        let idx = Arc::new(osearch::Index::build(&text, self.script(version), morph.as_deref()));
        self.oindex.lock().unwrap().insert(version.to_string(), idx.clone());
        Ok(idx)
    }

    fn tsk(&self) -> Result<Arc<bible_study_core::tsk::Tsk>, String> {
        let mut slot = self.tsk.lock().unwrap();
        if let Some(t) = slot.as_ref() {
            return Ok(t.clone());
        }
        let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
        let t = Arc::new(bible_study_core::tsk::Tsk::open(&dir.join("databases")).map_err(|e| format!("tskdic: {e}"))?);
        *slot = Some(t.clone());
        Ok(t)
    }

    fn strongs(&self) -> Result<Arc<Strongs>, String> {
        let mut slot = self.strongs.lock().unwrap();
        if let Some(s) = slot.as_ref() {
            return Ok(s.clone());
        }
        let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
        let s = Arc::new(Strongs::open(&dir.join("databases")).map_err(|e| e.to_string())?);
        *slot = Some(s.clone());
        Ok(s)
    }

    fn lexicon(&self, id: &str) -> Result<Arc<Lexicon>, String> {
        let lang = if GREEK_LEXICONS.iter().any(|(i, _)| *i == id) {
            Language::Greek
        } else if HEBREW_LEXICONS.iter().any(|(i, _)| *i == id) {
            Language::Hebrew
        } else {
            return Err("unknown lexicon".into());
        };
        if let Some(l) = self.lexicons.lock().unwrap().get(id) {
            return Ok(l.clone());
        }
        let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
        let l = Arc::new(Lexicon::open(&dir.join("databases"), id, lang).map_err(|e| format!("{id}: {e}"))?);
        self.lexicons.lock().unwrap().insert(id.to_string(), l.clone());
        Ok(l)
    }

    fn vmap(&self, id: &str) -> Arc<Vmap> {
        if let Some(v) = self.vmaps.lock().unwrap().get(id) {
            return v.clone();
        }
        let v = Arc::new(self.dir.as_ref().map_or_else(Vmap::default, |d| Vmap::load(&d.join("databases"), id)));
        self.vmaps.lock().unwrap().insert(id.to_string(), v.clone());
        v
    }

    fn script(&self, id: &str) -> Script {
        self.versions.get(id).map_or(Script::Latin, |i| i.script)
    }
}

fn discover(dir: &Option<PathBuf>) -> BTreeMap<String, ModInfo> {
    let mut out = BTreeMap::new();
    let Some(dir) = dir else { return out };
    for e in fs::read_dir(dir.join("databases")).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.extension().map_or(false, |x| x.eq_ignore_ascii_case("dbx")) {
            continue;
        }
        let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(str::to_lowercase) else { continue };
        let sc = script::script_of(&id);
        let Some(probe) = Module::probe(&p) else { continue };
        let readable = match sc {
            Script::Latin => !probe.is_morph && probe.high_ratio < MAX_HIGH_RATIO,
            _ => script::is_original_text(&id),
        };
        if readable {
            out.insert(id, ModInfo { path: p, script: sc });
        }
    }
    out
}

fn script_name(s: Script) -> &'static str {
    match s {
        Script::Latin => "latin",
        Script::Greek => "greek",
        Script::Hebrew => "hebrew",
    }
}

#[derive(Serialize)]
struct VersionOut {
    id: String,
    script: &'static str,
}

#[derive(Serialize)]
struct Status {
    dir: Option<String>,
    versions: Vec<VersionOut>,
}

#[derive(Serialize)]
struct BookOut {
    code: String,
    name: String,
    chapters: usize,
}

#[derive(Serialize)]
struct TokenOut {
    t: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    s: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    m: Option<String>,
    /// Word index within the verse (Greek/Hebrew), for morphology lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    w: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    h: bool,
}

#[derive(Serialize)]
struct VerseOut {
    /// Verse number, and the book index and chapter, in this version's own numbering.
    n: usize,
    b: usize,
    c: usize,
    tokens: Vec<TokenOut>,
}

#[derive(Serialize)]
struct RowOut {
    n: usize,
    /// One entry per column; a compared version may map to several verses, or none.
    cells: Vec<Vec<VerseOut>>,
}

#[derive(Serialize)]
struct ChapterOut {
    rows: Vec<RowOut>,
    /// Verse numbers (of the primary version) that have a note, and whether the chapter has one.
    notes: Vec<usize>,
    chapter_note: bool,
    prev: Option<(usize, usize)>,
    next: Option<(usize, usize)>,
}

#[derive(Serialize)]
struct HitOut {
    code: String,
    book: String,
    chapter: usize,
    verse: usize,
    tokens: Vec<TokenOut>,
}

#[derive(Serialize)]
struct FindOut {
    hits: Vec<HitOut>,
    capped: bool,
}

fn verse_tokens(m: &Module, i: usize, sc: Script, query: Option<&str>, cantillation: bool, hit_words: &[usize]) -> Vec<TokenOut> {
    let raw = m.raw(i).unwrap_or_default();
    if sc != Script::Latin {
        // Original-language text: one token per word, decoded to Unicode.
        let mut out = Vec::new();
        let words: Vec<&str> = raw.split_whitespace().collect();
        let last = words.len().saturating_sub(1);
        for (w, word) in words.iter().enumerate() {
            let mut t = script::decode(sc, word, cantillation);
            if sc == Script::Hebrew && w == last && !t.ends_with('\u{5C3}') {
                t.push('\u{5C3}'); // sof pasuq
            }
            if w > 0 {
                out.push(TokenOut { t: " ".into(), s: None, m: None, w: None, h: false });
            }
            out.push(TokenOut { t, s: None, m: None, w: Some(w), h: hit_words.contains(&w) });
        }
        return out;
    }
    let toks = tokens(&raw);
    let hl = query.map(|q| search::highlight(q, &toks, search::is_ot(m, i)));
    toks.into_iter()
        .enumerate()
        .map(|(k, t)| TokenOut { t: t.text, s: t.strongs, m: t.morph, w: None, h: hl.as_ref().map_or(false, |h| h[k]) })
        .collect()
}

#[tauri::command]
fn status(app: State<App>) -> Status {
    let rank = |s: Script| match s {
        Script::Latin => 0,
        Script::Greek => 1,
        Script::Hebrew => 2,
    };
    let mut versions: Vec<_> = app.versions.iter().map(|(id, i)| (rank(i.script), id.clone(), script_name(i.script))).collect();
    versions.sort();
    Status {
        dir: app.dir.as_ref().map(|d| d.display().to_string()),
        versions: versions.into_iter().map(|(_, id, script)| VersionOut { id, script }).collect(),
    }
}

#[tauri::command]
async fn books(app: State<'_, App>, version: String) -> Result<Vec<BookOut>, String> {
    let m = app.module(&version)?;
    Ok(m.vers.books.iter().map(|b| BookOut { code: b.code.clone(), name: book_name(&b.code), chapters: b.chapters.len() }).collect())
}

/// Resolve text like "john 3:16" in a version's own numbering to (book, chapter, verse).
#[tauri::command]
async fn goto(app: State<'_, App>, version: String, text: String) -> Result<Option<(usize, usize, usize)>, String> {
    Ok(app.module(&version)?.vers.parse_ref(&text))
}

/// Where a reference in one version's numbering is in another's: (book index, chapter, verse).
#[tauri::command]
async fn convert(app: State<'_, App>, from: String, to: String, code: String, chapter: usize, verse: usize) -> Result<Option<(usize, usize, usize)>, String> {
    let (fm, tm) = (app.module(&from)?, app.module(&to)?);
    let (fv, tv) = (app.vmap(&from), app.vmap(&to));
    let hit = map_verse((&fv, &fm.vers), (&tv, &tm.vers), &(code, chapter, verse)).into_iter().next();
    Ok(hit.and_then(|i| tm.vers.locate(i)))
}

#[tauri::command]
async fn chapter(app: State<'_, App>, version: String, book: usize, chapter: usize, compare: Vec<String>, cantillation: bool) -> Result<ChapterOut, String> {
    let pm = app.module(&version)?;
    let range = pm.vers.chapter_range(book, chapter).ok_or("no such chapter")?;
    let (prev, next) = pm.vers.neighbors(book, chapter);
    let code = pm.vers.books[book].code.clone();
    let psc = app.script(&version);
    let pv = app.vmap(&version);
    let others: Vec<_> = compare.iter().map(|id| Ok((id.clone(), app.module(id)?, app.vmap(id), app.script(id)))).collect::<Result<_, String>>()?;

    let rows: Vec<RowOut> = range
        .enumerate()
        .map(|(k, i)| {
            let n = k + 1;
            let mut cells = vec![vec![VerseOut { n, b: book, c: chapter, tokens: verse_tokens(&pm, i, psc, None, cantillation, &[]) }]];
            for (_, om, ov, osc) in &others {
                let idx = map_verse((&pv, &pm.vers), (ov, &om.vers), &(code.clone(), chapter, n));
                cells.push(
                    idx.into_iter()
                        .filter_map(|j| {
                            let (b, c, v) = om.vers.locate(j)?;
                            Some(VerseOut { n: v, b, c, tokens: verse_tokens(om, j, *osc, None, cantillation, &[]) })
                        })
                        .collect(),
                );
            }
            RowOut { n, cells }
        })
        .collect();
    let (notes, chapter_note) = library::chapter_notes(&app, &version, &code, chapter, rows.len());
    Ok(ChapterOut { rows, notes, chapter_note, prev, next })
}

/// Search a version. English follows `search.rs`; Greek and Hebrew follow `osearch.rs` (the first search of a
/// version builds its index, which takes a moment).
#[tauri::command]
async fn find(app: State<'_, App>, version: String, query: String) -> Result<FindOut, String> {
    let m = app.module(&version)?;
    let sc = app.script(&version);
    let mut found: Vec<(usize, Vec<usize>)>;
    if sc == Script::Latin {
        found = search::search(&m, &query, HIT_CAP + 1).into_iter().map(|i| (i, vec![])).collect();
    } else {
        let strongs = app.strongs()?;
        let q = osearch::parse(&query, sc, Some(&strongs))?;
        let idx = app.oindex(&version)?;
        found = idx.search(&m, &q, HIT_CAP + 1).into_iter().map(|h| (h.verse, h.words)).collect();
    }
    let capped = found.len() > HIT_CAP;
    found.truncate(HIT_CAP);
    let hits = found
        .into_iter()
        .filter_map(|(i, words)| {
            let (b, chapter, verse) = m.vers.locate(i)?;
            let code = m.vers.books[b].code.clone();
            let q = (sc == Script::Latin).then_some(query.as_str());
            Some(HitOut { book: book_name(&code), code, chapter, verse, tokens: verse_tokens(&m, i, sc, q, false, &words) })
        })
        .collect();
    Ok(FindOut { hits, capped })
}

#[derive(Serialize)]
struct StrongsOut {
    label: String,
    number: u32,
    hebrew: bool,
    word: String,
    translit: String,
    pronunciation: String,
    body: String,
}

fn strongs_out(e: StrongsEntry) -> StrongsOut {
    StrongsOut { label: format!("{}{}", if e.hebrew { 'H' } else { 'G' }, e.number), number: e.number, hebrew: e.hebrew, word: e.word, translit: e.translit, pronunciation: e.pronunciation, body: e.body }
}

#[derive(Serialize)]
struct LexInfo {
    id: &'static str,
    name: &'static str,
}

fn lexinfo(hebrew: bool) -> Vec<LexInfo> {
    let list = if hebrew { HEBREW_LEXICONS } else { GREEK_LEXICONS };
    list.iter().map(|(id, name)| LexInfo { id, name }).collect()
}

/// One morpheme of an original-language word (Hebrew words often have prefix morphemes).
#[derive(Serialize)]
struct PartOut {
    lemma: String,
    lemma_raw: String,
    code: String,
    parsing: String,
    hebrew: bool,
    /// Prefix particles and markers: shown, but there is no lexicon entry to look up.
    particle: bool,
    strongs: Vec<StrongsOut>,
}

#[derive(Serialize)]
struct WordInfo {
    parts: Vec<PartOut>,
    lexicons: Vec<LexInfo>,
}

/// Parsing, lemma and Strong's entries for word `word` of a Greek or Hebrew verse.
#[tauri::command]
async fn word_info(app: State<'_, App>, version: String, book: usize, chapter: usize, verse: usize, word: usize) -> Result<WordInfo, String> {
    let sc = app.script(&version);
    let morph_id = script::morph_companion(&version).ok_or("no morphology for this version")?;
    let (tm, mm) = (app.module(&version)?, app.module(morph_id)?);
    let i = tm.vers.index(book, chapter, verse).ok_or("no such verse")?;
    let raw = mm.raw(i).ok_or("no morphology for this verse")?;
    let token = raw.split_whitespace().nth(word).ok_or("no such word")?;
    let strongs = app.strongs()?;
    let hebrew_text = sc == Script::Hebrew;
    let scheme = greek_scheme(morph_id);

    let mut parts = Vec::new();
    for m in morphemes(token) {
        let particle = hebrew_text && m.code.starts_with('P');
        let (lemma, parsing, nums) = if hebrew_text {
            let nums = if particle { vec![] } else { strongs.hebrew_lemma(&m.lemma) };
            (hebrew(&m.lemma, false), hebrew_parse(&m.code), nums)
        } else {
            let uni = greek(&m.lemma);
            let parsing = scheme.map_or_else(|| m.code.clone(), |s| greek_parse(&m.code, s));
            let nums = strongs.greek_lemma(&uni);
            (uni, parsing, nums)
        };
        let entries = nums.into_iter().take(4).filter_map(|n| strongs.entry(hebrew_text, n)).map(strongs_out).collect();
        parts.push(PartOut { lemma, lemma_raw: m.lemma, code: m.code, parsing, hebrew: hebrew_text, particle, strongs: entries });
    }
    Ok(WordInfo { parts, lexicons: lexinfo(hebrew_text) })
}

/// Strong's entry (and the lexicons that can expand it) for a number tagged in an English text.
#[tauri::command]
async fn strongs_info(app: State<'_, App>, number: u32, hebrew: bool) -> Result<Option<WordInfoStrongs>, String> {
    let strongs = app.strongs()?;
    Ok(strongs.entry(hebrew, number).map(|e| WordInfoStrongs { lemma: e.word.clone(), strongs: strongs_out(e), lexicons: lexinfo(hebrew) }))
}

#[derive(Serialize)]
struct WordInfoStrongs {
    /// Greek lemma (Unicode); empty for Hebrew.
    lemma: String,
    strongs: StrongsOut,
    lexicons: Vec<LexInfo>,
}

#[derive(Serialize)]
struct SpanOut {
    t: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    b: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    i: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    g: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    h: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<(String, usize, usize)>,
}

fn span_out(s: Span) -> SpanOut {
    SpanOut { t: s.text, b: s.bold, i: s.italic, g: s.greek, h: s.hebrew, link: s.link }
}

/// Entries of one lexicon for a word. Greek: the Unicode lemma. Hebrew: the Westminster lemma and/or Strong's numbers.
#[tauri::command]
async fn lookup(app: State<'_, App>, lexicon: String, lemma: String, strongs: Vec<u32>) -> Result<Vec<Vec<SpanOut>>, String> {
    let lex = app.lexicon(&lexicon)?;
    let hits = match lex.language() {
        Language::Greek => lex.greek(&lemma),
        Language::Hebrew => lex.hebrew(&lemma, &strongs),
    };
    Ok(hits.into_iter().take(MAX_ENTRIES).filter_map(|i| lex.entry(i)).map(|spans| spans.into_iter().map(span_out).collect()).collect())
}

/// Raw bytes of a font that ships with BibleWorks, for rendering Hebrew and Greek.
#[tauri::command]
fn font(app: State<App>, name: String) -> Result<Response, String> {
    let file = match name.as_str() {
        "hebrew" => "SBL_Hbrw.ttf",
        "greek" => "GalSILR201.ttf",
        _ => return Err("unknown font".into()),
    };
    let dir = app.dir.as_ref().ok_or("BibleWorks not found")?;
    fs::read(dir.join(file)).map(Response::new).map_err(|e| e.to_string())
}

/// Where the user's own data lives: `$BS_DATA_DIR`, else `%APPDATA%\local.biblestudy.app`.
fn data_dir() -> PathBuf {
    std::env::var_os("BS_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("local.biblestudy.app")))
        .unwrap_or_else(|| PathBuf::from("bible-study-data"))
}

fn main() {
    let dir = install_dir();
    let versions = discover(&dir);
    tauri::Builder::default()
        .manage(App { dir, versions, cache: Mutex::new(HashMap::new()), vmaps: Mutex::new(HashMap::new()), strongs: Mutex::new(None), tsk: Mutex::new(None), lexicons: Mutex::new(HashMap::new()), library: library::Library::open(data_dir()), oindex: Mutex::new(HashMap::new()) })
        .invoke_handler(tauri::generate_handler![
            status, books, goto, convert, chapter, find, word_info, strongs_info, lookup, font,
            library::note_get, library::note_set, library::notes_all,
            library::vl_all, library::vl_get, library::vl_create, library::vl_rename, library::vl_delete, library::vl_add_span,
            library::vl_add_refs, library::vl_remove, library::vl_sort, library::vl_combine, library::vl_export, library::vl_import, library::vl_texts,
            library::tsk_get, library::tsk_texts, library::wl_generate, library::wl_all, library::wl_get, library::wl_save, library::wl_delete, library::wl_combine
        ])
        .run(tauri::generate_context!())
        .expect("error while running Bible Study");
}
