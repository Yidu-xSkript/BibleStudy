//! Commands for the user's own data: notes, verse lists and word lists.
//! Everything is stored as JSON in the app data folder (`store.rs`), never in the BibleWorks install.

use crate::App;
use bible_study_core::{
    canon::book_name,
    flashcards,
    lists::{self, ListRef, Op, VerseLists},
    module::tokens,
    notes::{self, Notes},
    script::{self, Script},
    store,
    tsk::TskRef,
    vmap::map_verse,
    wordlist::{self, WordEntry, WordList, WordLists},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;

/// Loaded user data plus where each part is saved.
pub struct Library {
    dir: PathBuf,
    notes: Mutex<Notes>,
    vlists: Mutex<VerseLists>,
    wlists: Mutex<WordLists>,
    pub(crate) flash: Mutex<flashcards::Progress>,
}

impl Library {
    pub fn open(dir: PathBuf) -> Self {
        Library {
            notes: Mutex::new(store::load(&dir.join("notes.json"))),
            vlists: Mutex::new(store::load(&dir.join("verse-lists.json"))),
            wlists: Mutex::new(store::load(&dir.join("word-lists.json"))),
            flash: Mutex::new(store::load(&dir.join("flashcards.json"))),
            dir,
        }
    }

    pub(crate) fn save<T: Serialize>(&self, file: &str, value: &T) -> Result<(), String> {
        store::save(&self.dir.join(file), value).map_err(|e| format!("couldn't save {file}: {e}"))
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl App {
    /// A reference in `from`'s numbering as the (numbering, code, chapter, verse) notes are keyed by:
    /// the KJV's when it can be mapped there, otherwise the original. `verse` 0 means the chapter,
    /// which is keyed by the KJV chapter its first verse falls in.
    fn canonical(&self, from: &str, code: &str, chapter: usize, verse: usize) -> (String, String, usize, usize) {
        let own = || (from.to_string(), code.to_string(), chapter, verse);
        if from == "kjv" {
            return own();
        }
        (|| {
            let (fm, km) = (self.module(from).ok()?, self.module("kjv").ok()?);
            let (fv, kv) = (self.vmap(from), self.vmap("kjv"));
            let probe = (code.to_string(), chapter, verse.max(1));
            let i = map_verse((&fv, &fm.vers), (&kv, &km.vers), &probe).into_iter().next()?;
            let (b, c, v) = km.vers.locate(i)?;
            Some(("kjv".to_string(), km.vers.books[b].code.clone(), c, if verse == 0 { 0 } else { v }))
        })()
        .unwrap_or_else(own)
    }
}

/// Which rows of a chapter have a note: (verse numbers with a verse note, whether the chapter has a note).
pub fn chapter_notes(app: &App, version: &str, code: &str, chapter: usize, verses: usize) -> (Vec<usize>, bool) {
    let notes = app.library.notes.lock().unwrap();
    if notes.len() == 0 {
        return (vec![], false);
    }
    let rows = (1..=verses).filter(|&n| {
        let (ns, c, ch, v) = app.canonical(version, code, chapter, n);
        notes.get(&notes::key(&ns, &c, ch, v)).is_some()
    });
    let has_chapter = {
        let (ns, c, ch, _) = app.canonical(version, code, chapter, 0);
        notes.get(&notes::key(&ns, &c, ch, 0)).is_some()
    };
    (rows.collect(), has_chapter)
}

// ---------------------------------------------------------------- notes

#[derive(Serialize)]
pub struct NoteOut {
    key: String,
    text: String,
    updated: u64,
    ns: String,
    code: String,
    chapter: usize,
    verse: usize,
    /// "John 3:16" / "John 3"
    label: String,
}

fn note_out(key: &str, n: Option<&notes::Note>) -> Option<NoteOut> {
    let (ns, code, chapter, verse) = notes::parse_key(key)?;
    let label = if verse == 0 { format!("{} {}", book_name(&code), chapter) } else { format!("{} {}:{}", book_name(&code), chapter, verse) };
    Some(NoteOut { key: key.to_string(), text: n.map_or(String::new(), |n| n.text.clone()), updated: n.map_or(0, |n| n.updated), ns, code, chapter, verse, label })
}

/// The note for a verse (or the chapter when `verse` is 0) in `version`'s numbering. Empty text if none.
#[tauri::command]
pub fn note_get(app: State<App>, version: String, code: String, chapter: usize, verse: usize) -> Option<NoteOut> {
    let (ns, c, ch, v) = app.canonical(&version, &code, chapter, verse);
    let key = notes::key(&ns, &c, ch, v);
    let notes = app.library.notes.lock().unwrap();
    note_out(&key, notes.get(&key))
}

#[tauri::command]
pub fn note_set(app: State<App>, key: String, text: String) -> Result<(), String> {
    notes::parse_key(&key).ok_or("bad note key")?;
    let mut notes = app.library.notes.lock().unwrap();
    if notes.set(&key, &text, now()) {
        app.library.save("notes.json", &*notes)?;
    }
    Ok(())
}

#[tauri::command]
pub fn notes_all(app: State<App>, query: String) -> Vec<NoteOut> {
    let notes = app.library.notes.lock().unwrap();
    notes.all(&query).into_iter().filter_map(|(k, n)| note_out(k, Some(n))).collect()
}

// ---------------------------------------------------------------- verse lists

#[derive(Serialize)]
pub struct ListSummary {
    id: String,
    name: String,
    count: usize,
}

#[tauri::command]
pub fn vl_all(app: State<App>) -> Vec<ListSummary> {
    app.library.vlists.lock().unwrap().lists.iter().map(|l| ListSummary { id: l.id.clone(), name: l.name.clone(), count: l.refs.len() }).collect()
}

#[derive(Serialize)]
pub struct RefOut {
    #[serde(flatten)]
    r: ListRef,
    label: String,
}

fn ref_out(r: &ListRef) -> RefOut {
    RefOut { label: format!("{} {}:{}", book_name(&r.code), r.chapter, r.verse), r: r.clone() }
}

#[derive(Serialize)]
pub struct ListOut {
    id: String,
    name: String,
    description: String,
    refs: Vec<RefOut>,
}

fn list_out(l: &lists::VerseList) -> ListOut {
    ListOut { id: l.id.clone(), name: l.name.clone(), description: l.description.clone(), refs: l.refs.iter().map(ref_out).collect() }
}

#[tauri::command]
pub fn vl_get(app: State<App>, id: String) -> Option<ListOut> {
    app.library.vlists.lock().unwrap().lists.iter().find(|l| l.id == id).map(list_out)
}

/// Run `f` on the lists and save if it returns `Ok`.
fn edit_vlists<T>(app: &App, f: impl FnOnce(&mut VerseLists) -> Result<T, String>) -> Result<T, String> {
    let mut g = app.library.vlists.lock().unwrap();
    let out = f(&mut g)?;
    app.library.save("verse-lists.json", &*g)?;
    Ok(out)
}

#[tauri::command]
pub fn vl_create(app: State<App>, name: String, refs: Vec<ListRef>) -> Result<String, String> {
    edit_vlists(&app, |l| {
        let list = l.create(&name, vec![]);
        lists::append_unique(&mut list.refs, refs);
        Ok(list.id.clone())
    })
}

#[tauri::command]
pub fn vl_rename(app: State<App>, id: String, name: String, description: String) -> Result<(), String> {
    edit_vlists(&app, |l| {
        let list = l.get_mut(&id).ok_or("no such list")?;
        if !name.trim().is_empty() {
            list.name = name.trim().to_string();
        }
        list.description = description;
        Ok(())
    })
}

#[tauri::command]
pub fn vl_delete(app: State<App>, id: String) -> Result<(), String> {
    edit_vlists(&app, |l| l.delete(&id).then_some(()).ok_or_else(|| "no such list".to_string()))
}

/// Add verses typed as a passage ("Eph 2:8-10", "Jam 3") in `version`'s numbering.
#[tauri::command]
pub async fn vl_add_span(app: State<'_, App>, id: String, version: String, span: String) -> Result<usize, String> {
    let m = app.module(&version)?;
    let range = m.vers.parse_span(&span).ok_or_else(|| format!("Couldn't read “{span}” as a passage in {}.", version.to_uppercase()))?;
    let refs = lists::refs_from_span(&m.vers, &version, range);
    let n = refs.len();
    edit_vlists(&app, |l| {
        lists::append_unique(&mut l.get_mut(&id).ok_or("no such list")?.refs, refs);
        Ok(n)
    })
}

#[tauri::command]
pub fn vl_add_refs(app: State<App>, id: String, refs: Vec<ListRef>) -> Result<(), String> {
    edit_vlists(&app, |l| {
        lists::append_unique(&mut l.get_mut(&id).ok_or("no such list")?.refs, refs);
        Ok(())
    })
}

#[tauri::command]
pub fn vl_remove(app: State<App>, id: String, index: usize) -> Result<(), String> {
    edit_vlists(&app, |l| {
        let refs = &mut l.get_mut(&id).ok_or("no such list")?.refs;
        if index < refs.len() {
            refs.remove(index);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn vl_sort(app: State<App>, id: String) -> Result<(), String> {
    edit_vlists(&app, |l| {
        lists::sort_dedupe(&mut l.get_mut(&id).ok_or("no such list")?.refs);
        Ok(())
    })
}

/// Compare two lists into a new list; returns its id.
#[tauri::command]
pub fn vl_combine(app: State<App>, a: String, b: String, op: Op, name: String) -> Result<String, String> {
    edit_vlists(&app, |l| {
        let find = |l: &VerseLists, id: &str| l.lists.iter().find(|x| x.id == id).map(|x| x.refs.clone());
        let (ra, rb) = (find(l, &a).ok_or("no such list")?, find(l, &b).ok_or("no such list")?);
        let list = l.create(&name, lists::combine(op, &ra, &rb));
        Ok(list.id.clone())
    })
}

#[tauri::command]
pub fn vl_export(app: State<App>, id: String) -> Result<(String, String), String> {
    let g = app.library.vlists.lock().unwrap();
    let l = g.lists.iter().find(|l| l.id == id).ok_or("no such list")?;
    Ok((l.name.clone(), lists::to_vls(&l.name, &l.refs)))
}

#[tauri::command]
pub fn vl_import(app: State<App>, text: String, name: Option<String>) -> Result<String, String> {
    let (title, refs) = lists::parse_vls(&text).ok_or("That isn't a BibleWorks verse list (.vls) file.")?;
    edit_vlists(&app, |l| {
        let list = l.create(name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&title), vec![]);
        lists::append_unique(&mut list.refs, refs);
        Ok(list.id.clone())
    })
}

/// Plain text of one verse (given in `ns`'s numbering) as it reads in the version `tm`/`tv`, mapped through the verse maps.
fn text_in(app: &App, tm: &bible_study_core::module::Module, tv: &bible_study_core::vmap::Vmap, sc: Script, ns: &str, code: &str, chapter: usize, verse: usize) -> String {
    (|| {
        let fm = app.module(ns).ok()?;
        let fv = app.vmap(ns);
        let idx = map_verse((&fv, &fm.vers), (tv, &tm.vers), &(code.to_string(), chapter, verse));
        let parts: Vec<String> = idx
            .into_iter()
            .filter_map(|i| {
                let raw = tm.raw(i)?;
                Some(if sc == Script::Latin { tokens(&raw).into_iter().map(|t| t.text).collect::<String>() } else { raw.split_whitespace().map(|w| script::decode(sc, w, false)).collect::<Vec<_>>().join(" ") })
            })
            .collect();
        Some(parts.join(" ").trim().to_string())
    })()
    .unwrap_or_default()
}

/// Text of each reference in `version`, mapped from the reference's own numbering. Long lists are cut off.
#[tauri::command]
pub async fn vl_texts(app: State<'_, App>, id: String, version: String) -> Result<Vec<String>, String> {
    const MAX: usize = 300;
    let refs = app.library.vlists.lock().unwrap().lists.iter().find(|l| l.id == id).map(|l| l.refs.clone()).ok_or("no such list")?;
    let (tm, tv, sc) = (app.module(&version)?, app.vmap(&version), app.script(&version));
    Ok(refs.iter().take(MAX).map(|r| text_in(&app, &tm, &tv, sc, &r.ns, &r.code, r.chapter, r.verse)).collect())
}

// ---------------------------------------------------------------- cross-references (TSK)

#[derive(Serialize)]
pub struct TskRefOut {
    #[serde(flatten)]
    r: TskRef,
    /// "Romans 5:8" / "John 1:1-3"
    label: String,
}

#[derive(Serialize)]
pub struct TskGroupOut {
    phrase: String,
    refs: Vec<TskRefOut>,
}

#[derive(Serialize)]
pub struct TskOut {
    /// The verse these belong to, in KJV numbering ("John 3:16").
    label: String,
    outline: Vec<(usize, String)>,
    groups: Vec<TskGroupOut>,
    /// Why there is nothing to show, when there isn't.
    note: Option<String>,
}

fn tsk_label(r: &TskRef) -> String {
    let end = if r.end > r.verse { format!("-{}", r.end) } else { String::new() };
    format!("{} {}:{}{}", book_name(&r.code), r.chapter, r.verse, end)
}

/// Cross-references for a verse of `version` (any numbering: it is mapped to the KJV's, which TSK uses).
#[tauri::command]
pub async fn tsk_get(app: State<'_, App>, version: String, code: String, chapter: usize, verse: usize) -> Result<TskOut, String> {
    let (ns, kcode, kch, kv) = app.canonical(&version, &code, chapter, verse);
    let label = format!("{} {}:{}", book_name(&kcode), kch, kv);
    let empty = |note: &str| TskOut { label: label.clone(), outline: vec![], groups: vec![], note: Some(note.to_string()) };
    if ns != "kjv" {
        return Ok(empty("The Treasury is organised by KJV verses, and this verse has no KJV counterpart."));
    }
    let tsk = app.tsk()?;
    let outline = tsk.outline(&kcode, kch);
    let Some(e) = tsk.entry(&kcode, kch, kv) else {
        return Ok(TskOut { outline, ..empty("The Treasury has no cross-references for this verse.") });
    };
    let groups = e.groups.into_iter().map(|g| TskGroupOut { phrase: g.phrase, refs: g.refs.into_iter().map(|r| TskRefOut { label: tsk_label(&r), r }).collect() }).collect();
    Ok(TskOut { label, outline, groups, note: None })
}

/// The text of KJV-numbered references as they read in `version` (ranges are capped at ten verses).
#[tauri::command]
pub async fn tsk_texts(app: State<'_, App>, version: String, refs: Vec<TskRef>) -> Result<Vec<String>, String> {
    let (tm, tv, sc) = (app.module(&version)?, app.vmap(&version), app.script(&version));
    Ok(refs
        .iter()
        .map(|r| (r.verse..=r.end.min(r.verse + 9)).map(|v| text_in(&app, &tm, &tv, sc, "kjv", &r.code, r.chapter, v)).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" "))
        .collect())
}

// ---------------------------------------------------------------- word lists

#[derive(Serialize)]
pub struct Frequency {
    source: String,
    entries: Vec<WordEntry>,
}

/// Compile a frequency list for `version` over a passage ("" = the whole version).
#[tauri::command]
pub async fn wl_generate(app: State<'_, App>, version: String, range: String, pos: Option<String>) -> Result<Frequency, String> {
    let m = app.module(&version)?;
    let sc = app.script(&version);
    let span = if range.trim().is_empty() { 0..m.verse_count() } else { m.vers.parse_span(&range).ok_or_else(|| format!("Couldn't read “{}” as a passage in {}.", range.trim(), version.to_uppercase()))? };
    let morph = match (sc, script::morph_companion(&version)) {
        (Script::Latin, _) | (_, None) => None,
        (_, Some(id)) => Some(app.module(id)?),
    };
    let pos = pos.and_then(|p| p.chars().next());
    let entries = wordlist::frequency(&m, sc, morph.as_deref(), span, pos);
    let what = if range.trim().is_empty() { "whole text".to_string() } else { range.trim().to_string() };
    Ok(Frequency { source: format!("{} · {}", version.to_uppercase(), what), entries })
}

#[derive(Serialize)]
pub struct WordListOut {
    id: String,
    name: String,
    source: String,
    entries: Vec<WordEntry>,
}

fn wl_out(l: &WordList) -> WordListOut {
    WordListOut { id: l.id.clone(), name: l.name.clone(), source: l.source.clone(), entries: l.entries.clone() }
}

#[tauri::command]
pub fn wl_all(app: State<App>) -> Vec<ListSummary> {
    app.library.wlists.lock().unwrap().lists.iter().map(|l| ListSummary { id: l.id.clone(), name: l.name.clone(), count: l.entries.len() }).collect()
}

#[tauri::command]
pub fn wl_get(app: State<App>, id: String) -> Option<WordListOut> {
    app.library.wlists.lock().unwrap().lists.iter().find(|l| l.id == id).map(wl_out)
}

#[tauri::command]
pub fn wl_save(app: State<App>, name: String, source: String, entries: Vec<WordEntry>) -> Result<String, String> {
    let mut g = app.library.wlists.lock().unwrap();
    let id = g.create(&name, &source, entries).id.clone();
    app.library.save("word-lists.json", &*g)?;
    Ok(id)
}

#[tauri::command]
pub fn wl_delete(app: State<App>, id: String) -> Result<(), String> {
    let mut g = app.library.wlists.lock().unwrap();
    if !g.delete(&id) {
        return Err("no such list".into());
    }
    app.library.save("word-lists.json", &*g)
}

/// Compare two saved word lists (the result isn't saved until the caller asks).
#[tauri::command]
pub fn wl_combine(app: State<App>, a: String, b: String, op: Op) -> Result<Vec<WordEntry>, String> {
    let g = app.library.wlists.lock().unwrap();
    let find = |id: &str| g.lists.iter().find(|l| l.id == id).map(|l| l.entries.clone()).ok_or("no such list");
    Ok(wordlist::combine(op, &find(&a)?, &find(&b)?))
}
