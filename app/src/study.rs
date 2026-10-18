//! Commands for the study tools beyond reading and search: flashcards and timelines.

use crate::{library::now, App};
use bible_study_core::{
    flashcards::{self, Card, Deck, DeckInfo, Filter, Stats},
    timeline::{self, Timeline},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::State;

#[derive(Default)]
pub struct Study {
    decks: Mutex<HashMap<String, Arc<Deck>>>,
    timelines: Mutex<Option<Arc<Vec<Timeline>>>>,
}

fn today() -> i64 {
    (now() / 86_400) as i64
}

impl App {
    fn deck(&self, id: &str) -> Result<Arc<Deck>, String> {
        if let Some(d) = self.study.decks.lock().unwrap().get(id) {
            return Ok(d.clone());
        }
        let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
        let d = Arc::new(flashcards::open_deck(dir, id).map_err(|e| format!("{id}: {e}"))?);
        self.study.decks.lock().unwrap().insert(id.to_string(), d.clone());
        Ok(d)
    }
}

#[tauri::command]
pub fn fc_decks(app: State<App>) -> Vec<DeckInfo> {
    app.dir.as_ref().map(|d| flashcards::list_decks(d)).unwrap_or_default()
}

#[derive(Serialize)]
pub struct CardOut {
    #[serde(flatten)]
    card: Card,
    /// Leitner box, `None` for a card not studied yet.
    level: Option<u8>,
}

#[derive(Serialize)]
pub struct DeckOut {
    title: String,
    hebrew: bool,
    comment: String,
    cards: Vec<CardOut>,
    /// Indexes into `cards` to study now.
    queue: Vec<usize>,
    stats: Stats,
}

#[tauri::command]
pub fn fc_open(app: State<App>, deck: String, filter: Filter, limit: usize) -> Result<DeckOut, String> {
    let d = app.deck(&deck)?;
    let progress = app.library.flash.lock().unwrap();
    let saved = progress.decks.get(&deck);
    let cards = d.cards.iter().map(|c| CardOut { card: c.clone(), level: saved.and_then(|m| m.get(&c.id)).map(|s| s.box_) }).collect();
    let t = today();
    Ok(DeckOut { title: d.title.clone(), hebrew: d.hebrew, comment: d.comment.clone(), cards, queue: progress.queue(&d, &filter, t, limit), stats: progress.stats(&d, &filter, t) })
}

#[tauri::command]
pub fn fc_grade(app: State<App>, deck: String, card: String, right: bool) -> Result<(), String> {
    let mut p = app.library.flash.lock().unwrap();
    p.grade(&deck, &card, right, today());
    app.library.save("flashcards.json", &*p)
}

#[tauri::command]
pub fn fc_learned(app: State<App>, deck: String, card: String, learned: bool) -> Result<(), String> {
    let mut p = app.library.flash.lock().unwrap();
    p.set_learned(&deck, &card, learned, today());
    app.library.save("flashcards.json", &*p)
}

#[tauri::command]
pub fn fc_reset(app: State<App>, deck: String) -> Result<(), String> {
    let mut p = app.library.flash.lock().unwrap();
    p.reset(&deck);
    app.library.save("flashcards.json", &*p)
}

#[tauri::command]
pub fn tl_all(app: State<App>) -> Result<Vec<Timeline>, String> {
    let mut slot = app.study.timelines.lock().unwrap();
    if slot.is_none() {
        let dir = app.dir.as_ref().ok_or("BibleWorks not found")?;
        *slot = Some(Arc::new(timeline::open_all(dir)));
    }
    Ok(slot.as_ref().unwrap().as_ref().clone())
}
