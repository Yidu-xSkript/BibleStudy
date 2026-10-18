//! Commands for the study tools beyond reading and search: flashcards and timelines.

use crate::{library::now, App};
use bible_study_core::{
    diagram::{self, GntDiagram},
    flashcards::{self, Card, Deck, DeckInfo, Filter, Stats},
    maps::{BaseStyle, MapData, OverlayKind, Viewport},
    timeline::{self, Timeline},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{ipc::Response, State};

#[derive(Default)]
pub struct Study {
    decks: Mutex<HashMap<String, Arc<Deck>>>,
    timelines: Mutex<Option<Arc<Vec<Timeline>>>>,
    maps: Mutex<Option<Arc<MapData>>>,
    dgm_index: Mutex<Option<Arc<Vec<GntDiagram>>>>,
}

fn today() -> i64 {
    (now() / 86_400) as i64
}

impl App {
    fn maps(&self) -> Result<Arc<MapData>, String> {
        let mut slot = self.study.maps.lock().unwrap();
        if slot.is_none() {
            let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
            *slot = Some(Arc::new(MapData::open(dir).map_err(|e| format!("no map data: {e}"))?));
        }
        Ok(slot.clone().unwrap())
    }

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

// ---------------------------------------------------------------- maps

#[derive(Serialize)]
pub struct MapViewInfo {
    id: String,
    title: String,
}

#[derive(Serialize)]
pub struct MapOverlayInfo {
    id: String,
    label: String,
    /// Folder path in BibleWorks' overlay tree.
    group: Vec<String>,
    /// "sites" (labels and routes) or "lines" (coasts, rivers, boundaries).
    kind: &'static str,
}

#[derive(Serialize)]
pub struct MapCatalog {
    views: Vec<MapViewInfo>,
    overlays: Vec<MapOverlayInfo>,
}

#[tauri::command]
pub async fn map_catalog(app: State<'_, App>) -> Result<MapCatalog, String> {
    let m = app.maps()?;
    let views = m.views.iter().map(|v| MapViewInfo { id: v.id.clone(), title: v.title.clone() }).collect();
    let overlays = m
        .overlays
        .iter()
        .filter_map(|o| {
            let kind = match o.kind {
                OverlayKind::Sites => "sites",
                OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers => "lines",
                _ => return None,
            };
            Some(MapOverlayInfo { id: o.id.clone(), label: o.label.clone(), group: o.group.clone(), kind })
        })
        .collect();
    Ok(MapCatalog { views, overlays })
}

#[derive(Serialize)]
pub struct MapViewOut {
    title: String,
    lat: f64,
    lon: f64,
    ppd: f64,
    overlays: Vec<String>,
    white_on_black: bool,
}

/// A saved view: where it looks and which vector overlays it shows (its bitmaps are the base style's job).
#[tauri::command]
pub async fn map_view(app: State<'_, App>, id: String) -> Result<MapViewOut, String> {
    let m = app.maps()?;
    let v = m.views.iter().find(|v| v.id == id).ok_or("no such view")?;
    let overlays = v
        .overlays
        .iter()
        .filter_map(|f| m.overlay_by_datafile(f))
        .filter(|o| matches!(o.kind, OverlayKind::Sites | OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers))
        .map(|o| o.id.clone())
        .collect();
    Ok(MapViewOut { title: v.title.clone(), lat: v.lat, lon: v.lon, ppd: v.ppd, overlays, white_on_black: v.white_on_black })
}

fn viewport(lat: f64, lon: f64, ppd: f64, width: u32, height: u32) -> Result<Viewport, String> {
    if ![lat, lon, ppd].iter().all(|v| v.is_finite()) || !(1.0..=20_000.0).contains(&ppd) {
        return Err("bad map position".into());
    }
    Ok(Viewport { lat: lat.clamp(-85.0, 85.0), lon, ppd, width: width.clamp(64, 3000), height: height.clamp(64, 3000) })
}

/// The base picture (PNG; sea and unmapped areas are transparent).
#[tauri::command]
pub async fn map_render(app: State<'_, App>, style: String, lat: f64, lon: f64, ppd: f64, width: u32, height: u32) -> Result<Response, String> {
    let m = app.maps()?;
    let style = match style.as_str() {
        "satellite" => BaseStyle::Satellite,
        "elevation" => BaseStyle::Elevation,
        "landcover" => BaseStyle::LandCover,
        _ => BaseStyle::Relief,
    };
    m.render_base_png(style, &viewport(lat, lon, ppd, width, height)?).map(Response::new).map_err(|e| e.to_string())
}

/// Lines, routes and labels as an SVG in the picture's pixel coordinates.
#[tauri::command]
pub async fn map_vectors(app: State<'_, App>, overlays: Vec<String>, white_on_black: bool, lat: f64, lon: f64, ppd: f64, width: u32, height: u32) -> Result<String, String> {
    let m = app.maps()?;
    let ids: Vec<&str> = overlays.iter().map(String::as_str).collect();
    m.svg_overlay(&viewport(lat, lon, ppd, width, height)?, &ids, white_on_black).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct PlaceOut {
    name: String,
    lat: f64,
    lon: f64,
    overlay: String,
    classification: String,
}

const MAX_PLACES: usize = 40;

#[tauri::command]
pub async fn map_find(app: State<'_, App>, query: String) -> Result<Vec<PlaceOut>, String> {
    let m = app.maps()?;
    let mut seen = std::collections::HashSet::new();
    Ok(m.find_place(&query)
        .into_iter()
        .filter(|h| (h.lat, h.lon) != (0.0, 0.0) && seen.insert((h.display.to_lowercase(), (h.lat * 100.0) as i64, (h.lon * 100.0) as i64)))
        .take(MAX_PLACES)
        .map(|h| PlaceOut { name: if h.display.is_empty() { h.name } else { h.display }, lat: h.lat, lon: h.lon, overlay: h.overlay_label, classification: h.classification })
        .collect())
}

// ---------------------------------------------------------------- diagrams

#[derive(Serialize)]
pub struct DiagramInfo {
    /// File stem; pass back to `dg_open`.
    name: String,
    book: String,
    chapter: u16,
    verse: u16,
    end_chapter: u16,
    end_verse: u16,
}

#[derive(Serialize)]
pub struct DiagramList {
    gnt: Vec<DiagramInfo>,
    /// Names of the example diagrams (file stems).
    examples: Vec<String>,
}

impl App {
    fn dgm_index(&self) -> Result<Arc<Vec<GntDiagram>>, String> {
        let mut slot = self.study.dgm_index.lock().unwrap();
        if slot.is_none() {
            let dir = self.dir.as_ref().ok_or("BibleWorks not found")?;
            *slot = Some(Arc::new(diagram::gnt_index(dir)));
        }
        Ok(slot.clone().unwrap())
    }
}

#[tauri::command]
pub async fn dg_list(app: State<'_, App>) -> Result<DiagramList, String> {
    let idx = app.dgm_index()?;
    let dir = app.dir.as_ref().ok_or("BibleWorks not found")?;
    let examples = diagram::example_files(dir).iter().filter_map(|p| Some(p.file_stem()?.to_str()?.to_string())).collect();
    let gnt = idx.iter().map(|g| DiagramInfo { name: g.name.clone(), book: g.book.clone(), chapter: g.chapter, verse: g.verse, end_chapter: g.end_chapter, end_verse: g.end_verse }).collect();
    Ok(DiagramList { gnt, examples })
}

#[derive(Serialize)]
pub struct DiagramOut {
    title: String,
    description: String,
    svg: String,
    /// The Greek of the diagram in reading order.
    text: String,
}

/// One diagram by name, as an SVG. `example` picks the grammar examples instead of the GNT passages.
#[tauri::command]
pub async fn dg_open(app: State<'_, App>, name: String, example: bool) -> Result<DiagramOut, String> {
    let dir = app.dir.as_ref().ok_or("BibleWorks not found")?;
    let d = if example {
        let path = diagram::example_files(dir).into_iter().find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(name.as_str())).ok_or("no such example")?;
        diagram::Diagram::open(path)?
    } else {
        app.dgm_index()?.iter().find(|g| g.name == name).ok_or("no such diagram")?.load()?
    };
    Ok(DiagramOut { svg: d.to_svg(), text: d.text(), title: d.title.clone(), description: d.description.clone() })
}
