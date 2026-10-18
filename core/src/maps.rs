//! BibleWorks 8 Maps data (read-only, from the user's own install): `<install>/mapdata`.
//!
//! The module is a stack of *overlays* drawn over each other. Nothing is documented; the formats
//! below were worked out from the files and from `BWMaps.exe`:
//!
//!   * `BWMaps.cfg` - the overlay tree shown in the Overlays window. One entry per line, the
//!     depth is the number of leading `+`: `+f Name` is a folder, `+++ file.ohd` an overlay.
//!   * `overlays/*.ohd` - overlay header, CRLF text: `##MLH 1.0`, `##Label`, `##Description` (a
//!     character count line, the text, `##END`), `##DateRange`, `##BibleRef`, `##Type xx`,
//!     `##LLBounds` (`north west south east` in degrees), optional `##Clip` (`lat lon` pairs of a
//!     polygon outside which a bitmap is empty), `##Datafile`, and for bitmaps `##BMbitsPerPixel`
//!     and `##BMHeightWidth h w`. The type decides the data file: `l` = `.lab` sites and routes,
//!     `b?` = `.fei` bitmap (`be`, `bl`, `bi`: the second letter is a resolution tier, not a
//!     subject), `vb` / `vc` / `vr` = `.bwv` vector lines (boundaries, coasts/islands/lakes,
//!     rivers/water; from the CIA World DataBank II).
//!   * `overlays/*.lab` - Windows-1252 text, CRLF. First line: record count. Records run from
//!     `##lbBegin` to `##lbEnd` and are `key value` lines. `##type`: `st` site (marker + label),
//!     `sr` free text label, `pa` smooth curve through points, `pl` straight polyline. Sites and
//!     labels carry `LL1`/`LL2` (`lat lon`; a site's marker is LL1), `labLL` (label anchor),
//!     `marker`, `PFASS ppd size angle spread style` (font size in points *at* that zoom),
//!     `zoom` (smallest ppd at which it is shown), `dispName` (index key), `scnDisp` (text drawn),
//!     `siteID`, `altVN <version> <spelling>`, `sameAs`, `Strongs1/2`, `HebSp`, `ArSp`,
//!     `Classif`, `Notes`. Paths carry `WSC width style colour` (colour = index into the 16 stock
//!     Windows colours, unverified), `numPt`, `ptsBegin`/`ptsEnd` with one `lat lon` per line,
//!     `shArr y` (arrowhead), `isCl y` (closed), `tcbTn` (curve tension).
//!   * `overlays/*.bwv` - binary vector lines, little endian. Header (20 bytes): u16 version 2,
//!     u16 0x2077, u32 header size, u32 record count, u32 and u32 not needed. Then `count` index
//!     records of 8 x i32: `id, maxLat, minLat, maxLon, minLon` (arc seconds), `offset`, `size`
//!     (unreliable), `rank` (1 = most important; picks the drawing style). At `offset`: i32 pairs
//!     `x y` (arc seconds, x = lon, y = lat): the first vertex, then `(id, n)`, then `n` more
//!     vertices. Longitudes below -50 degrees mean "add 360" (east of the date line).
//!   * `overlays/*.fei` - plain Windows BMP (4 or 8 bit, palette, uncompressed, up to 700 MB), but
//!     stored **north row first** although the header height is positive. Equirectangular (plate
//!     carree) in `LLBounds`: pixel (c, r) centre is lon `west + (c+.5)/w*(east-west)` and lat
//!     `north - (r+.5)/h*(north-south)`. `overlays/reduced/<name>.fei.<pct>` are the same image
//!     shrunk to `pct` percent (a pyramid: 07, 10, 25, 40 exist). Satellite scenes are rotated
//!     inside their box, hence the `##Clip` polygon.
//!   * `elevation/*.bil` + `.hdr`: ESRI BIL rasters of real SRTM heights (i16 metres, -9999 =
//!     void/sea), geographic, `ULXMAP`/`ULYMAP` = centre of the top-left cell. Not referenced by
//!     BWMaps; used here for height queries and a computed hillshade.
//!   * `views/**.mst` - saved map views: `##Title`, `##Overlays` (data file names), then
//!     `##latInit`/`##lonInit`/`##ppdInit` (centre and pixels per degree) and display flags.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path as FsPath, PathBuf},
};

fn bad(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.into())
}

/// Windows-1252 to text (the `.lab` files use it; bytes 0x81, 0x8D, 0x8F, 0x90, 0x9D stay C1 controls).
pub fn decode_cp1252(b: &[u8]) -> String {
    const HI: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}',
        '\u{17D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}',
        '\u{153}', '\u{9D}', '\u{17E}', '\u{178}',
    ];
    b.iter().map(|&c| if (0x80..0xA0).contains(&c) { HI[(c - 0x80) as usize] } else { c as char }).collect()
}

// ---------------------------------------------------------------- geometry

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub lat: f64,
    pub lon: f64,
}

/// A geographic box in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub north: f64,
    pub west: f64,
    pub south: f64,
    pub east: f64,
}

impl Bounds {
    pub fn new(north: f64, west: f64, south: f64, east: f64) -> Self {
        Bounds { north, west, south, east }
    }
    pub fn is_valid(&self) -> bool {
        [self.north, self.west, self.south, self.east].iter().all(|v| v.is_finite()) && self.north > self.south && self.east > self.west
    }
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat <= self.north && lat >= self.south && lon >= self.west && lon <= self.east
    }
    pub fn intersects(&self, o: &Bounds) -> bool {
        self.north >= o.south && self.south <= o.north && self.east >= o.west && self.west <= o.east
    }
    pub fn union(&self, o: &Bounds) -> Bounds {
        Bounds::new(self.north.max(o.north), self.west.min(o.west), self.south.min(o.south), self.east.max(o.east))
    }
    pub fn lat_span(&self) -> f64 {
        self.north - self.south
    }
    pub fn lon_span(&self) -> f64 {
        self.east - self.west
    }
}

/// A map window in BibleWorks' own terms: centre and pixels per degree (`ppd`), equal on both axes
/// (a plate carree projection, like the bitmaps). `x` grows east, `y` grows south.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub lat: f64,
    pub lon: f64,
    pub ppd: f64,
    pub width: u32,
    pub height: u32,
}

impl Viewport {
    pub fn bounds(&self) -> Bounds {
        let (hw, hh) = (self.width as f64 / 2.0 / self.ppd, self.height as f64 / 2.0 / self.ppd);
        Bounds::new(self.lat + hh, self.lon - hw, self.lat - hh, self.lon + hw)
    }
    /// Pixel position of a coordinate (may lie outside the image).
    pub fn project(&self, lat: f64, lon: f64) -> (f64, f64) {
        let b = self.bounds();
        ((lon - b.west) * self.ppd, (b.north - lat) * self.ppd)
    }
    pub fn unproject(&self, x: f64, y: f64) -> Point {
        let b = self.bounds();
        Point { lat: b.north - y / self.ppd, lon: b.west + x / self.ppd }
    }
    /// The viewport that shows all of `b` in a `width` x `height` image.
    pub fn fit(b: &Bounds, width: u32, height: u32) -> Viewport {
        let ppd = (width as f64 / b.lon_span()).min(height as f64 / b.lat_span());
        Viewport { lat: (b.north + b.south) / 2.0, lon: (b.west + b.east) / 2.0, ppd, width, height }
    }
}

fn in_polygon(poly: &[Point], lat: f64, lon: f64) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.lat > lat) != (b.lat > lat) && lon < (b.lon - a.lon) * (lat - a.lat) / (b.lat - a.lat) + a.lon {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------- catalogue

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    /// `##Type l`: sites, text labels and routes from a `.lab` file.
    Sites,
    /// `##Type b?`: a `.fei` bitmap (BMP).
    Bitmap,
    /// `vb`: modern political boundaries (`.bwv`).
    Boundaries,
    /// `vc`: coasts, islands and lakes (`.bwv`).
    Coasts,
    /// `vr`: rivers and water (`.bwv`).
    Rivers,
    Other,
}

/// One overlay: its `.ohd` header and, for site overlays, the parsed `.lab` content.
#[derive(Clone, Debug)]
pub struct Overlay {
    /// File stem of the `.ohd`, lower-cased (`bk00-gen`, `era23`, `band1-004`).
    pub id: String,
    pub label: String,
    pub description: String,
    pub date_range: String,
    pub bible_ref: String,
    pub type_code: String,
    pub kind: OverlayKind,
    pub datafile: String,
    pub bounds: Option<Bounds>,
    /// Polygon where a bitmap has data (empty: the whole box).
    pub clip: Vec<Point>,
    /// `(width, height)` of a bitmap in pixels.
    pub bitmap_size: Option<(u32, u32)>,
    /// Folder names from `BWMaps.cfg` (`["Background bitmaps", "Satellite imagery"]`); empty if unlisted.
    pub group: Vec<String>,
    pub labels: Vec<Label>,
    pub paths: Vec<Path>,
}

impl Overlay {
    /// Pixels per degree of a bitmap overlay (0 for others).
    pub fn bitmap_ppd(&self) -> f64 {
        match (self.bitmap_size, self.bounds) {
            (Some((w, _)), Some(b)) if b.is_valid() => w as f64 / b.lon_span(),
            _ => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    /// `st`: a marker at (lat, lon) with a name next to it.
    Site,
    /// `sr`: free text (regions, deserts, seas, ...) anchored at its upper-left corner.
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    /// Zoom (ppd) at which `size_pt` applies.
    pub ref_ppd: f64,
    pub size_pt: i32,
    /// Degrees, counter-clockwise (text labels only).
    pub angle: i32,
    /// Extra letter spacing.
    pub spread: i32,
    pub style: i32,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub kind: LabelKind,
    /// `dispName`: the name the Find window indexes (trimmed).
    pub name: String,
    /// `scnDisp`: the text drawn on the map (defaults to `name`).
    pub display: String,
    pub lat: f64,
    pub lon: f64,
    pub lat2: f64,
    pub lon2: f64,
    /// `labLL`: where the text sits, when it is not next to the marker.
    pub text_at: Option<Point>,
    pub marker: i32,
    pub font: Font,
    /// `zoom`: the label is shown from this ppd up (0 = always).
    pub min_ppd: f64,
    pub opts: i32,
    pub site_id: String,
    pub same_as: String,
    /// `altVN`: `(Bible version abbreviation, spelling)`.
    pub alt_names: Vec<(String, String)>,
    pub strongs: Vec<String>,
    pub hebrew: String,
    pub arabic: String,
    pub classification: String,
    pub notes: String,
    pub notes_ej: String,
    pub line_to_marker: bool,
    pub bounding_box: bool,
}

impl Label {
    pub fn point(&self) -> Point {
        Point { lat: self.lat, lon: self.lon }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// `pa`: a smooth curve through the points.
    Curve,
    /// `pl`: straight segments.
    Polyline,
}

/// A route or outline (journeys, boundaries of empires, city walls).
#[derive(Clone, Debug)]
pub struct Path {
    pub kind: PathKind,
    pub points: Vec<Point>,
    /// `WSC`: pen width (0 = hairline).
    pub width: i32,
    /// `WSC`: pen style (0 solid; 1 and 2 are dashed/dotted variants).
    pub style: i32,
    /// `WSC`: colour index 0..15, see [`stock_colour`].
    pub colour: i32,
    pub arrow: bool,
    pub closed: bool,
    /// `tcbTn`, the spline tension of a curve.
    pub tension: f64,
}

/// The 16 stock Windows colours, in the usual DIB palette order.
// ponytail: the order of the colour indices in `WSC` is assumed to be this one (consistent with
// Paul's four journeys getting four different colours); the exe's own table was not found.
pub fn stock_colour(i: i32) -> [u8; 3] {
    const T: [[u8; 3]; 16] = [
        [0, 0, 0], [128, 0, 0], [0, 128, 0], [128, 128, 0], [0, 0, 128], [128, 0, 128], [0, 128, 128], [192, 192, 192],
        [128, 128, 128], [255, 0, 0], [0, 255, 0], [255, 255, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255],
    ];
    T[(i.clamp(0, 15)) as usize]
}

impl Path {
    /// The vertices to draw: the points themselves for a polyline, a Kochanek-Bartels spline
    /// (tension `tcbTn`, continuity and bias 0) with `steps` samples per segment for a curve.
    pub fn tessellate(&self, steps: usize) -> Vec<Point> {
        let p = &self.points;
        if self.kind == PathKind::Polyline || p.len() < 3 || steps < 2 {
            let mut v = p.clone();
            if self.closed && v.len() > 2 {
                v.push(v[0]);
            }
            return v;
        }
        let n = p.len();
        let at = |i: isize| -> Point {
            if self.closed {
                p[i.rem_euclid(n as isize) as usize]
            } else {
                p[i.clamp(0, n as isize - 1) as usize]
            }
        };
        let k = (1.0 - self.tension) / 2.0;
        let segs = if self.closed { n } else { n - 1 };
        let mut out = Vec::with_capacity(segs * steps + 1);
        for s in 0..segs as isize {
            let (p0, p1, p2, p3) = (at(s - 1), at(s), at(s + 1), at(s + 2));
            // Tangents; the end points of an open curve use a one-sided difference.
            let m1 = (k * (p2.lat - p0.lat), k * (p2.lon - p0.lon));
            let m2 = (k * (p3.lat - p1.lat), k * (p3.lon - p1.lon));
            for j in 0..steps {
                let t = j as f64 / steps as f64;
                let (t2, t3) = (t * t, t * t * t);
                let (h00, h10, h01, h11) = (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2);
                out.push(Point {
                    lat: h00 * p1.lat + h10 * m1.0 + h01 * p2.lat + h11 * m2.0,
                    lon: h00 * p1.lon + h10 * m1.1 + h01 * p2.lon + h11 * m2.1,
                });
            }
        }
        out.push(if self.closed { p[0] } else { p[n - 1] });
        out
    }
}

/// A saved map view (`views/**.mst`).
#[derive(Clone, Debug)]
pub struct View {
    /// Path below `views/` without extension (`Books of the Bible/43 Acts`).
    pub id: String,
    pub title: String,
    /// Data file names exactly as listed (`band1-004.fei`, `paul1.lab`, `afr-cil.bwv`).
    pub overlays: Vec<String>,
    pub white_on_black: bool,
    pub grid: bool,
    pub lat: f64,
    pub lon: f64,
    pub ppd: f64,
    pub grayscale: bool,
    pub contrast: i32,
    pub brightness: i32,
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub file: String,
    pub message: String,
}

/// What [`MapData::render_base`] paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseStyle {
    /// Stylised elevation bitmaps (coarse to fine: `elevmed`, `band1-4`, `elevdet`).
    Elevation,
    /// Wide elevation as backdrop, then the Landsat scenes.
    Satellite,
    /// The land cover maps.
    LandCover,
    /// Computed from the SRTM heights: hypsometric colours plus hillshade (see [`MapData::render_relief`]).
    Relief,
}

/// A raster vector line (`.bwv`): river, coast, boundary.
#[derive(Clone, Debug)]
pub struct Line {
    pub id: i32,
    /// Importance class, 1 = most important.
    pub rank: u32,
    pub points: Vec<Point>,
}

/// Drawing style `BWMaps.exe` gives to `.bwv` lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineStyle {
    pub rgb: [u8; 3],
    pub dotted: bool,
}

// ponytail: which overlay type (`vb`/`vc`/`vr`) the exe's three colour schemes belong to is
// inferred from their order (boundary, coast, river); the rank thresholds are as in the exe.
pub fn bwv_style(kind: OverlayKind, rank: u32) -> LineStyle {
    match kind {
        OverlayKind::Boundaries => LineStyle { rgb: if rank == 1 { [0, 0, 0] } else { [128, 128, 128] }, dotted: false },
        OverlayKind::Coasts => LineStyle { rgb: [192, 0, 0], dotted: false },
        OverlayKind::Rivers => match rank {
            1..=5 => LineStyle { rgb: [0, 0, 255], dotted: false },
            6..=9 => LineStyle { rgb: [192, 192, 255], dotted: false },
            _ => LineStyle { rgb: [192, 192, 255], dotted: true },
        },
        _ => LineStyle { rgb: [0, 0, 0], dotted: false },
    }
}

/// A search result of [`MapData::find_place`].
#[derive(Clone, Debug)]
pub struct PlaceHit {
    pub overlay: String,
    pub overlay_label: String,
    pub index: usize,
    pub name: String,
    pub display: String,
    pub lat: f64,
    pub lon: f64,
    pub site_id: String,
    pub classification: String,
    pub kind: LabelKind,
}

// ---------------------------------------------------------------- text file parsing

/// `BWMaps.cfg`: `(folders, file name)` of every overlay, in display order.
fn parse_cfg(text: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        let depth = line.chars().take_while(|&c| c == '+').count();
        if depth == 0 {
            continue;
        }
        let rest = &line[depth..]; // '+' is ASCII
        stack.truncate(depth - 1);
        if let Some(name) = rest.strip_prefix("f ") {
            stack.push(name.trim().to_string());
        } else if !rest.trim().is_empty() {
            out.push((stack.clone(), rest.trim().to_string()));
        }
    }
    out
}

fn nums(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|t| t.parse().ok()).collect()
}

fn parse_ohd(id: &str, text: &str) -> Result<Overlay, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    if lines.first().map(|l| !l.starts_with("##MLH")).unwrap_or(true) {
        return Err("not an MLH overlay header".into());
    }
    let mut o = Overlay {
        id: id.to_string(),
        label: String::new(),
        description: String::new(),
        date_range: String::new(),
        bible_ref: String::new(),
        type_code: String::new(),
        kind: OverlayKind::Other,
        datafile: String::new(),
        bounds: None,
        clip: Vec::new(),
        bitmap_size: None,
        group: Vec::new(),
        labels: Vec::new(),
        paths: Vec::new(),
    };
    let next = |i: usize| lines.get(i + 1).copied().unwrap_or("").trim();
    let mut i = 1;
    while i < lines.len() {
        let l = lines[i];
        if l == "##Label" {
            o.label = next(i).to_string();
        } else if l == "##Description" {
            // a character count line, then text up to `##END`
            let mut j = i + 2;
            let mut d = Vec::new();
            while j < lines.len() && lines[j] != "##END" {
                d.push(lines[j]);
                j += 1;
            }
            o.description = d.join("\n").trim().to_string();
            i = j;
        } else if l == "##DateRange" {
            o.date_range = next(i).to_string();
        } else if l == "##BibleRef" {
            o.bible_ref = next(i).to_string();
        } else if l == "##LLBounds" {
            let v = nums(next(i));
            if v.len() == 4 {
                let b = Bounds::new(v[0], v[1], v[2], v[3]);
                if b.is_valid() {
                    o.bounds = Some(b);
                }
            }
        } else if l == "##Clip" {
            let v = nums(next(i));
            o.clip = v.chunks_exact(2).map(|c| Point { lat: c[0], lon: c[1] }).collect();
        } else if l == "##Datafile" {
            o.datafile = next(i).to_string();
        } else if let Some(t) = l.strip_prefix("##Type ") {
            o.type_code = t.trim().to_string();
        } else if let Some(v) = l.strip_prefix("##BMHeightWidth ") {
            let v = nums(v);
            if v.len() == 2 {
                o.bitmap_size = Some((v[1] as u32, v[0] as u32));
            }
        }
        i += 1;
    }
    o.kind = match o.type_code.as_str() {
        "l" => OverlayKind::Sites,
        "vb" => OverlayKind::Boundaries,
        "vc" => OverlayKind::Coasts,
        "vr" => OverlayKind::Rivers,
        t if t.starts_with('b') => OverlayKind::Bitmap,
        _ => OverlayKind::Other,
    };
    if o.datafile.is_empty() {
        return Err("no ##Datafile".into());
    }
    Ok(o)
}

fn parse_lab(text: &str) -> (Vec<Label>, Vec<Path>, usize) {
    let mut labels = Vec::new();
    let mut paths = Vec::new();
    let mut skipped = 0;
    let mut cur: Option<Vec<(&str, &str)>> = None;
    let mut pts: Vec<Point> = Vec::new();
    let mut in_pts = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line == "##lbBegin" {
            cur = Some(Vec::new());
            pts.clear();
            in_pts = false;
        } else if line == "##lbEnd" {
            if let Some(kv) = cur.take() {
                let get = |k: &str| kv.iter().find(|(a, _)| *a == k).map(|(_, v)| v.trim());
                match lab_record(&kv, &get, std::mem::take(&mut pts)) {
                    Some(Rec::L(l)) => labels.push(l),
                    Some(Rec::P(p)) => paths.push(p),
                    None => skipped += 1,
                }
            }
        } else if cur.is_some() {
            if line == "##ptsBegin" {
                in_pts = true;
            } else if line == "##ptsEnd" {
                in_pts = false;
            } else if in_pts {
                if let [la, lo] = nums(line)[..] {
                    pts.push(Point { lat: la, lon: lo });
                }
            } else if let Some(rest) = line.strip_prefix("##") {
                let (k, v) = rest.split_once(' ').unwrap_or((rest, ""));
                cur.as_mut().unwrap().push((k, v));
            }
        }
    }
    (labels, paths, skipped)
}

enum Rec {
    L(Label),
    P(Path),
}

fn lab_record<'a>(kv: &[(&'a str, &'a str)], get: &dyn Fn(&str) -> Option<&'a str>, pts: Vec<Point>) -> Option<Rec> {
    let ty = get("type")?;
    let f = |k: &str| get(k).map(nums).unwrap_or_default();
    let s = |k: &str| get(k).unwrap_or("").to_string();
    match ty {
        "st" | "sr" => {
            let ll1 = f("LL1");
            if ll1.len() != 2 || !ll1.iter().all(|v| v.is_finite()) {
                return None;
            }
            let ll2 = f("LL2");
            let (lat2, lon2) = if ll2.len() == 2 { (ll2[0], ll2[1]) } else { (ll1[0], ll1[1]) };
            let name = s("dispName");
            let display = get("scnDisp").filter(|d| !d.is_empty()).map(str::to_string).unwrap_or_else(|| name.clone());
            let pf = f("PFASS");
            let font = if pf.len() == 5 {
                Font { ref_ppd: pf[0], size_pt: pf[1] as i32, angle: pf[2] as i32, spread: pf[3] as i32, style: pf[4] as i32 }
            } else {
                Font { ref_ppd: 100.0, size_pt: 10, angle: 0, spread: 0, style: 0 }
            };
            let lab_ll = f("labLL");
            let alt_names = kv
                .iter()
                .filter(|(k, _)| *k == "altVN")
                .filter_map(|(_, v)| v.trim().split_once(' ').map(|(a, b)| (a.to_string(), b.trim().to_string())))
                .collect();
            let strongs = kv.iter().filter(|(k, _)| *k == "Strongs1" || *k == "Strongs2").map(|(_, v)| v.trim().to_string()).collect();
            Some(Rec::L(Label {
                kind: if ty == "st" { LabelKind::Site } else { LabelKind::Text },
                name: if name.is_empty() { display.clone() } else { name },
                display,
                lat: ll1[0],
                lon: ll1[1],
                lat2,
                lon2,
                // `labLL 0 0` appears in a few records and means "not set"
                text_at: (lab_ll.len() == 2 && (lab_ll[0] != 0.0 || lab_ll[1] != 0.0)).then(|| Point { lat: lab_ll[0], lon: lab_ll[1] }),
                marker: f("marker").first().map(|&v| v as i32).unwrap_or(0),
                font,
                min_ppd: f("zoom").first().copied().unwrap_or(0.0),
                opts: f("opts").first().map(|&v| v as i32).unwrap_or(0),
                site_id: s("siteID"),
                same_as: s("sameAs"),
                alt_names,
                strongs,
                hebrew: s("HebSp"),
                arabic: s("ArSp"),
                classification: s("Classif").trim_matches('\'').to_string(),
                notes: s("Notes"),
                notes_ej: s("NotesEJ"),
                line_to_marker: get("lineToMarker") == Some("y"),
                bounding_box: get("fHasBoundingBox") == Some("y"),
            }))
        }
        "pa" | "pl" => {
            if pts.len() < 2 || !pts.iter().all(|p| p.lat.is_finite() && p.lon.is_finite()) {
                return None;
            }
            let w = f("WSC");
            let (width, style, colour) = if w.len() == 3 { (w[0] as i32, w[1] as i32, w[2] as i32) } else { (1, 0, 0) };
            Some(Rec::P(Path {
                kind: if ty == "pa" { PathKind::Curve } else { PathKind::Polyline },
                points: pts,
                width,
                style,
                colour,
                arrow: get("shArr") == Some("y"),
                closed: get("isCl") == Some("y"),
                tension: f("tcbTn").first().copied().unwrap_or(0.0),
            }))
        }
        _ => None,
    }
}

fn parse_mst(id: &str, text: &str) -> Option<View> {
    let mut v = View {
        id: id.to_string(),
        title: String::new(),
        overlays: Vec::new(),
        white_on_black: false,
        grid: false,
        lat: 0.0,
        lon: 0.0,
        ppd: 0.0,
        grayscale: false,
        contrast: 100,
        brightness: 0,
    };
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l == "##Title" {
            v.title = lines.get(i + 1).unwrap_or(&"").trim().to_string();
        } else if l == "##Overlays" {
            let mut j = i + 1;
            while j < lines.len() && !lines[j].is_empty() && !lines[j].starts_with("##") {
                v.overlays.push(lines[j].trim().to_string());
                j += 1;
            }
            i = j;
            continue;
        } else if let Some(r) = l.strip_prefix("##") {
            let (k, val) = r.split_once(' ').unwrap_or((r, ""));
            let num = val.trim().parse::<f64>().unwrap_or(0.0);
            match k {
                "latInit" => v.lat = num,
                "lonInit" => v.lon = num,
                "ppdInit" => v.ppd = num,
                "fWhiteOnBlack" => v.white_on_black = val.trim() == "y",
                "fGrid" => v.grid = val.trim() == "y",
                "fMakeGrayscale" => v.grayscale = val.trim() == "y",
                "contrastLevel" => v.contrast = num as i32,
                "brightness" => v.brightness = num as i32,
                _ => {}
            }
        }
        i += 1;
    }
    (!v.title.is_empty()).then_some(v)
}

// ---------------------------------------------------------------- BMP rasters

struct Bmp {
    file: File,
    w: usize,
    h: usize,
    bpp: u16,
    off: u64,
    stride: u64,
    pal: Vec<[u8; 3]>,
}

fn le16(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}
fn le32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}

impl Bmp {
    fn open(path: &FsPath) -> io::Result<Bmp> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut h = vec![0u8; 1100.min(len as usize)];
        file.read_exact(&mut h)?;
        if h.len() < 54 || &h[..2] != b"BM" || le32(&h, 14) != 40 {
            return Err(bad("not a BITMAPINFOHEADER BMP"));
        }
        let off = le32(&h, 10) as u64;
        let (w, hh, bpp, comp) = (le32(&h, 18) as i32, le32(&h, 22) as i32, le16(&h, 28), le32(&h, 30));
        if comp != 0 || !(bpp == 4 || bpp == 8) || w <= 0 || hh == 0 {
            return Err(bad(format!("unsupported BMP ({bpp} bpp, compression {comp}, {w}x{hh})")));
        }
        let (w, hh) = (w as usize, hh.unsigned_abs() as usize);
        let stride = (w as u64 * bpp as u64).div_ceil(32) * 4;
        if off + stride * (hh as u64 - 1) + (w as u64 * bpp as u64).div_ceil(8) > len {
            return Err(bad("BMP pixel data runs past the end of the file"));
        }
        let used = le32(&h, 46) as usize;
        let n = if used == 0 { 1usize << bpp } else { used.min(1 << bpp) };
        let avail = (off as usize).min(h.len()).saturating_sub(54) / 4;
        let pal = (0..n.min(avail)).map(|i| [h[56 + i * 4], h[55 + i * 4], h[54 + i * 4]]).collect();
        Ok(Bmp { file, w, h: hh, bpp, off, stride, pal })
    }

    /// Palette indices of columns `c0..c1` of file row `r` (row 0 is the northern row).
    fn indices(&mut self, r: usize, c0: usize, c1: usize, out: &mut Vec<u8>) -> io::Result<()> {
        out.clear();
        if r >= self.h || c0 >= c1 || c1 > self.w {
            return Err(bad("BMP read out of range"));
        }
        let per = if self.bpp == 8 { 1 } else { 2 };
        let (b0, b1) = (c0 / per, (c1 - 1) / per + 1);
        let mut raw = vec![0u8; b1 - b0];
        self.file.seek(SeekFrom::Start(self.off + r as u64 * self.stride + b0 as u64))?;
        self.file.read_exact(&mut raw)?;
        if self.bpp == 8 {
            *out = raw;
        } else {
            out.extend((c0..c1).map(|c| {
                let b = raw[c / 2 - b0];
                if c % 2 == 0 { b >> 4 } else { b & 15 }
            }));
        }
        Ok(())
    }
}

/// For `n` output pixels along one axis: where each starts in source pixels and how many source
/// pixels it spans, or `None` when its centre is outside `0..src_n`.
fn axis(n: usize, start: f64, step: f64, src_n: usize) -> Vec<Option<(f64, f64)>> {
    (0..n)
        .map(|i| {
            let (a, c) = (start + i as f64 * step, start + (i as f64 + 0.5) * step);
            (c >= 0.0 && c < src_n as f64).then_some((a, step))
        })
        .collect()
}

/// Up to 4 source positions evenly spread over a pixel's footprint (one when it is smaller than a source pixel).
fn taps(start: f64, span: f64, src_n: usize) -> impl Iterator<Item = usize> {
    let n = if span > 1.0 { (span.ceil() as usize).min(4) } else { 1 };
    (0..n).map(move |k| ((start + (k as f64 + 0.5) * span / n as f64).floor().max(0.0) as usize).min(src_n - 1))
}

// ---------------------------------------------------------------- DEM (BIL)

struct Dem {
    path: PathBuf,
    ncols: usize,
    nrows: usize,
    ulx: f64,
    uly: f64,
    dx: f64,
    dy: f64,
    big_endian: bool,
}

const VOID: i16 = -9999;

impl Dem {
    fn open(hdr: &FsPath, bil: PathBuf) -> io::Result<Dem> {
        let text = fs::read_to_string(hdr)?;
        let g = |k: &str| text.lines().find_map(|l| l.split_whitespace().collect::<Vec<_>>().as_slice().split_first().filter(|(a, _)| a.eq_ignore_ascii_case(k)).and_then(|(_, r)| r.first().map(|s| s.to_string())));
        let f = |k: &str| g(k).and_then(|s| s.parse::<f64>().ok()).ok_or_else(|| bad(format!("{k} missing in .hdr")));
        if f("NBITS")? != 16.0 || f("NBANDS").unwrap_or(1.0) != 1.0 || !g("LAYOUT").map(|l| l.eq_ignore_ascii_case("BIL")).unwrap_or(true) {
            return Err(bad("only 1-band 16-bit BIL is supported"));
        }
        let d = Dem {
            ncols: f("NCOLS")? as usize,
            nrows: f("NROWS")? as usize,
            ulx: f("ULXMAP")?,
            uly: f("ULYMAP")?,
            dx: f("XDIM")?,
            dy: f("YDIM")?,
            big_endian: g("BYTEORDER").map(|b| b.eq_ignore_ascii_case("M")).unwrap_or(false),
            path: bil,
        };
        if d.ncols == 0 || d.nrows == 0 || !(d.dx > 0.0 && d.dy > 0.0) {
            return Err(bad("bad .hdr geometry"));
        }
        if fs::metadata(&d.path)?.len() < (d.ncols * d.nrows * 2) as u64 {
            return Err(bad(".bil is shorter than the header says"));
        }
        Ok(d)
    }
    fn extent(&self) -> Bounds {
        Bounds::new(self.uly + self.dy / 2.0, self.ulx - self.dx / 2.0, self.uly - (self.nrows as f64 - 0.5) * self.dy, self.ulx + (self.ncols as f64 - 0.5) * self.dx)
    }
    /// Cells `c0..c1` of row `r`.
    fn row(&self, file: &mut File, r: usize, c0: usize, c1: usize, out: &mut Vec<i16>) -> io::Result<()> {
        let mut raw = vec![0u8; (c1 - c0) * 2];
        file.seek(SeekFrom::Start(((r * self.ncols + c0) * 2) as u64))?;
        file.read_exact(&mut raw)?;
        out.clear();
        out.extend(raw.chunks_exact(2).map(|b| if self.big_endian { i16::from_be_bytes([b[0], b[1]]) } else { i16::from_le_bytes([b[0], b[1]]) }));
        Ok(())
    }
}

// ---------------------------------------------------------------- MapData

pub struct MapData {
    root: PathBuf,
    pub overlays: Vec<Overlay>,
    pub views: Vec<View>,
    /// Files that could not be read or parsed (the rest of the data is still usable).
    pub problems: Vec<Problem>,
    dems: Vec<Dem>,
    by_id: HashMap<String, usize>,
    files: HashMap<String, PathBuf>,
    reduced: HashMap<String, Vec<(u32, PathBuf)>>,
}

fn dir_files(dir: &FsPath) -> Vec<(String, PathBuf)> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| (e.file_name().to_string_lossy().to_lowercase(), e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

impl MapData {
    /// Opens `<install>/mapdata` (or `install` itself when it already is that folder).
    /// Fails only when there is no map data at all; unreadable files are listed in `problems`.
    pub fn open(install: &FsPath) -> io::Result<MapData> {
        let root = if install.join("mapdata").is_dir() { install.join("mapdata") } else { install.to_path_buf() };
        let cfg = fs::read(root.join("BWMaps.cfg")).map_err(|e| io::Error::new(e.kind(), format!("no BWMaps.cfg in {}: {e}", root.display())))?;
        let tree = parse_cfg(&decode_cp1252(&cfg));
        let files: HashMap<String, PathBuf> = dir_files(&root.join("overlays")).into_iter().collect();
        let mut reduced: HashMap<String, Vec<(u32, PathBuf)>> = HashMap::new();
        for (name, path) in dir_files(&root.join("overlays").join("reduced")) {
            if let Some((base, pct)) = name.rsplit_once('.') {
                if let Ok(p) = pct.parse::<u32>() {
                    reduced.entry(base.to_string()).or_default().push((p, path));
                }
            }
        }
        reduced.values_mut().for_each(|v| v.sort());

        let mut md = MapData { root: root.clone(), overlays: Vec::new(), views: Vec::new(), problems: Vec::new(), dems: Vec::new(), by_id: HashMap::new(), files, reduced };

        // overlays: cfg order first, then any .ohd the cfg does not list
        let mut order: Vec<(Vec<String>, String)> = tree.into_iter().map(|(g, f)| (g, f.to_lowercase())).collect();
        let listed: std::collections::HashSet<String> = order.iter().map(|(_, f)| f.clone()).collect();
        let mut extra: Vec<String> = md.files.keys().filter(|k| k.ends_with(".ohd") && !listed.contains(*k)).cloned().collect();
        extra.sort();
        order.extend(extra.into_iter().map(|f| (Vec::new(), f)));
        for (group, file) in order {
            let id = file.trim_end_matches(".ohd").to_string();
            let Some(path) = md.files.get(&file).cloned() else {
                md.problem(&file, "listed in BWMaps.cfg but missing");
                continue;
            };
            let res = fs::read(&path).map_err(|e| e.to_string()).and_then(|b| parse_ohd(&id, &decode_cp1252(&b)));
            match res {
                Ok(mut o) => {
                    o.group = group;
                    md.by_id.insert(id, md.overlays.len());
                    md.overlays.push(o);
                }
                Err(e) => md.problem(&file, &e),
            }
        }
        for i in 0..md.overlays.len() {
            let (kind, datafile) = (md.overlays[i].kind, md.overlays[i].datafile.to_lowercase());
            let Some(path) = md.files.get(&datafile).cloned() else {
                md.problem(&datafile, "data file missing");
                continue;
            };
            match kind {
                OverlayKind::Sites => match fs::read(&path) {
                    Ok(b) => {
                        let (labels, paths, skipped) = parse_lab(&decode_cp1252(&b));
                        if skipped > 0 {
                            md.problem(&datafile, &format!("{skipped} unusable records skipped"));
                        }
                        md.overlays[i].labels = labels;
                        md.overlays[i].paths = paths;
                    }
                    Err(e) => md.problem(&datafile, &e.to_string()),
                },
                OverlayKind::Bitmap => {
                    if md.overlays[i].bounds.is_none() {
                        md.problem(&datafile, "bitmap overlay without usable LLBounds");
                    }
                    if let Err(e) = Bmp::open(&path) {
                        md.problem(&datafile, &e.to_string());
                    }
                }
                OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers => {
                    if let Err(e) = BwvIndex::read(&path) {
                        md.problem(&datafile, &e.to_string());
                    }
                }
                OverlayKind::Other => md.problem(&datafile, "unknown overlay type"),
            }
        }

        // views
        let mut stack = vec![root.join("views")];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().map(|e| e.eq_ignore_ascii_case("mst")).unwrap_or(false) {
                    let rel = p.strip_prefix(root.join("views")).unwrap_or(&p).with_extension("");
                    let id = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
                    match fs::read(&p).ok().and_then(|b| parse_mst(&id, &decode_cp1252(&b))) {
                        Some(v) => md.views.push(v),
                        None => md.problem(&format!("views/{id}.mst"), "unreadable view"),
                    }
                }
            }
        }
        md.views.sort_by(|a, b| a.id.cmp(&b.id));

        // elevation grids, coarse to fine
        for (name, hdr) in dir_files(&root.join("elevation")) {
            if let Some(stem) = name.strip_suffix(".hdr") {
                match md.files_in("elevation", &format!("{stem}.bil")).ok_or_else(|| bad("no matching .bil")).and_then(|bil| Dem::open(&hdr, bil)) {
                    Ok(d) => md.dems.push(d),
                    Err(e) => md.problem(&name, &e.to_string()),
                }
            }
        }
        md.dems.sort_by(|a, b| b.dx.total_cmp(&a.dx));
        Ok(md)
    }

    fn problem(&mut self, file: &str, message: &str) {
        self.problems.push(Problem { file: file.to_string(), message: message.to_string() });
    }

    fn files_in(&self, sub: &str, lname: &str) -> Option<PathBuf> {
        dir_files(&self.root.join(sub)).into_iter().find(|(n, _)| n == lname).map(|(_, p)| p)
    }

    pub fn overlay(&self, id: &str) -> Option<&Overlay> {
        self.by_id.get(&id.to_lowercase()).map(|&i| &self.overlays[i])
    }

    /// Finds the overlay a view entry (`paul1.lab`, `band1-004.fei`) refers to.
    pub fn overlay_by_datafile(&self, datafile: &str) -> Option<&Overlay> {
        let d = datafile.to_lowercase();
        self.overlays.iter().find(|o| o.datafile.to_lowercase() == d)
    }

    // ------------------------------------------------------------ search

    /// Searches site and label names (`name`, `display`, Bible-version spellings, `sameAs`),
    /// case-insensitively and like the Find Place window: `*` and `?` are wildcards (then the
    /// whole name must match, so `*journey*`); without them the query is a substring. Exact
    /// matches come first, then prefix matches. A place listed in many overlays appears once per
    /// overlay; group by `site_id` if you want one.
    pub fn find_place(&self, query: &str) -> Vec<PlaceHit> {
        let q: Vec<char> = query.trim().to_lowercase().chars().collect();
        if q.is_empty() {
            return Vec::new();
        }
        let wild = q.iter().any(|&c| c == '*' || c == '?');
        let mut hits: Vec<(u8, PlaceHit)> = Vec::new();
        for o in &self.overlays {
            for (index, l) in o.labels.iter().enumerate() {
                let mut best: Option<u8> = None;
                let names = [l.name.as_str(), l.display.as_str(), l.same_as.as_str()].into_iter().chain(l.alt_names.iter().map(|(_, n)| n.as_str()));
                for n in names {
                    let n: Vec<char> = n.trim().trim_end_matches('*').to_lowercase().chars().collect();
                    if n.is_empty() {
                        continue;
                    }
                    let rank = if wild {
                        wildcard(&q, &n).then_some(1)
                    } else if n == q {
                        Some(0)
                    } else if n.starts_with(&q) {
                        Some(1)
                    } else if n.windows(q.len()).any(|w| w == &q[..]) {
                        Some(2)
                    } else {
                        None
                    };
                    if let Some(r) = rank {
                        best = Some(best.map_or(r, |b| b.min(r)));
                    }
                }
                if let Some(rank) = best {
                    hits.push((
                        rank,
                        PlaceHit {
                            overlay: o.id.clone(),
                            overlay_label: o.label.clone(),
                            index,
                            name: l.name.clone(),
                            display: l.display.clone(),
                            lat: l.lat,
                            lon: l.lon,
                            site_id: l.site_id.clone(),
                            classification: l.classification.clone(),
                            kind: l.kind,
                        },
                    ));
                }
            }
        }
        hits.sort_by(|a, b| (a.0, a.1.name.to_lowercase()).cmp(&(b.0, b.1.name.to_lowercase())));
        hits.into_iter().map(|(_, h)| h).collect()
    }

    // ------------------------------------------------------------ vector lines

    /// The `.bwv` lines of overlay `id` whose box touches `bounds`.
    pub fn lines(&self, id: &str, bounds: &Bounds) -> io::Result<Vec<Line>> {
        let o = self.overlay(id).ok_or_else(|| bad(format!("no overlay {id}")))?;
        if !matches!(o.kind, OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers) {
            return Err(bad(format!("{id} is not a vector-line overlay")));
        }
        let path = self.files.get(&o.datafile.to_lowercase()).ok_or_else(|| bad("data file missing"))?;
        BwvIndex::read(path)?.lines(path, bounds)
    }

    /// Number of lines in a `.bwv` overlay.
    pub fn line_count(&self, id: &str) -> io::Result<usize> {
        let o = self.overlay(id).ok_or_else(|| bad(format!("no overlay {id}")))?;
        let path = self.files.get(&o.datafile.to_lowercase()).ok_or_else(|| bad("data file missing"))?;
        Ok(BwvIndex::read(path)?.recs.len())
    }

    // ------------------------------------------------------------ elevation

    /// SRTM height in metres at a point (finest grid that covers it); `None` outside the data or in a
    /// void (open sea). Coastal sea in some tiles is stored as 0, so `Some(0)` can still be water.
    pub fn elevation_at(&self, lat: f64, lon: f64) -> Option<i16> {
        for d in self.dems.iter().rev() {
            if !d.extent().contains(lat, lon) {
                continue;
            }
            let c = (((lon - d.ulx) / d.dx).round().max(0.0) as usize).min(d.ncols - 1);
            let r = (((d.uly - lat) / d.dy).round().max(0.0) as usize).min(d.nrows - 1);
            let mut f = File::open(&d.path).ok()?;
            let mut v = Vec::new();
            d.row(&mut f, r, c, c + 1, &mut v).ok()?;
            match v[0] {
                VOID => continue, // a coarser grid may know better
                h => return Some(h),
            }
        }
        None
    }

    // ------------------------------------------------------------ rendering

    /// Bitmap overlays of a base style, coarse to fine.
    fn base_layers(&self, style: BaseStyle) -> Vec<&Overlay> {
        let root = |o: &Overlay| o.group.iter().find(|g| !g.starts_with("Background")).cloned().unwrap_or_default();
        let mut v: Vec<&Overlay> = self.overlays.iter().filter(|o| o.kind == OverlayKind::Bitmap && o.bounds.is_some()).collect();
        let elevation = |o: &Overlay| root(o).starts_with("Elevation");
        v.retain(|o| match style {
            BaseStyle::Elevation => elevation(o),
            BaseStyle::Satellite => root(o).starts_with("Satellite") || (elevation(o) && o.bitmap_ppd() <= 300.0),
            BaseStyle::LandCover => root(o).starts_with("Land cover"),
            BaseStyle::Relief => false,
        });
        v.sort_by(|a, b| a.bitmap_ppd().total_cmp(&b.bitmap_ppd()));
        v
    }

    /// Renders the bitmap base for `bounds` into a `width` x `height` RGBA buffer (row-major, row 0
    /// north). Layers are painted coarse to fine; each is read straight from its BMP, seeking only
    /// the rows and columns that are needed, from the smallest pyramid level that is still at least
    /// as sharp as the output, so a 700 MB scene costs a few MB of I/O. Pixels no layer covers (or
    /// that a scene's clip polygon excludes) stay fully transparent.
    // ponytail: no smoothing when zooming in (nearest neighbour), box-ish sampling of up to 4x4
    // source pixels when zooming out; no colour adjustments (brightness/contrast are the UI's job).
    pub fn render_base(&self, style: BaseStyle, bounds: &Bounds, width: u32, height: u32) -> io::Result<Vec<u8>> {
        let (w, h) = (width as usize, height as usize);
        if !bounds.is_valid() || w == 0 || h == 0 || w.checked_mul(h).is_none_or(|n| n > (1 << 26)) {
            return Err(bad("bad viewport"));
        }
        if style == BaseStyle::Relief {
            return self.render_relief(bounds, width, height);
        }
        let mut out = vec![0u8; w * h * 4];
        let view_ppd = (w as f64 / bounds.lon_span()).max(h as f64 / bounds.lat_span());
        for o in self.base_layers(style) {
            let lb = o.bounds.unwrap();
            if !lb.intersects(bounds) {
                continue;
            }
            // smallest pyramid level at least as sharp as the view
            let need = view_ppd / o.bitmap_ppd().max(1e-9);
            let datafile = o.datafile.to_lowercase();
            let mut path = self.files.get(&datafile).cloned();
            if need < 1.0 {
                if let Some(p) = self.reduced.get(&datafile).and_then(|v| v.iter().find(|(pct, _)| *pct as f64 / 100.0 >= need)) {
                    path = Some(p.1.clone());
                }
            }
            let Some(path) = path else { continue };
            // the Landsat mosaic of Israel/Egypt has no clip polygon: its sea and scene edge are palette index 0
            let nodata = (o.clip.is_empty() && o.group.iter().any(|g| g.starts_with("Satellite"))).then_some(0u8);
            paint_bitmap(&path, &lb, &o.clip, nodata, bounds, w, h, &mut out)?;
        }
        Ok(out)
    }

    /// Hillshaded hypsometric relief from the SRTM grids. The coarse grid (about 1 km) is the
    /// backdrop; the fine one (about 90 m) covers lon 30..37, lat 27..35 over it. Sea and void
    /// cells (and cells at exactly 0 m, which the SRTM tiles use for sea; offshore cells of the fine grid
    /// that the coarse grid calls sea) are blue; the Dead Sea surface (about -416 m) is drawn as water too.
    // ponytail: sun from the north-west at 45 degrees, fixed 2x vertical exaggeration; heights are
    // averaged over at most 3x3 cells when zoomed out, nearest cell when zoomed in.
    pub fn render_relief(&self, bounds: &Bounds, width: u32, height: u32) -> io::Result<Vec<u8>> {
        let (w, h) = (width as usize, height as usize);
        if !bounds.is_valid() || w == 0 || h == 0 || w.checked_mul(h).is_none_or(|n| n > (1 << 26)) {
            return Err(bad("bad viewport"));
        }
        let (dlon, dlat) = (bounds.lon_span() / w as f64, bounds.lat_span() / h as f64);
        let mut out = vec![0u8; w * h * 4];
        // pixels a coarser grid called sea: the fine grid fills offshore cells with small heights (2-4 m)
        let mut sea_before = vec![false; w * h];
        for d in &self.dems {
            let ext = d.extent();
            if !ext.intersects(bounds) {
                continue;
            }
            // height grid with a one-pixel margin, in this DEM
            let (gw, gh) = (w + 2, h + 2);
            let mut grid = vec![f32::NAN; gw * gh];
            let mut cover = vec![false; gw * gh];
            let cx = |lon: f64| (lon - d.ulx) / d.dx;
            let ry = |lat: f64| (d.uly - lat) / d.dy;
            let (sx, sy) = (dlon / d.dx, dlat / d.dy); // cells per output pixel
            let nx = (sx.ceil() as usize).clamp(1, 3);
            let ny = (sy.ceil() as usize).clamp(1, 3);
            let mut file = File::open(&d.path)?;
            let (mut cmin, mut cmax) = (usize::MAX, 0usize);
            let cols: Vec<Option<Vec<usize>>> = (0..gw)
                .map(|gx| {
                    let lon0 = bounds.west + (gx as f64 - 1.0) * dlon;
                    if lon0 + dlon * 0.5 < ext.west || lon0 + dlon * 0.5 > ext.east {
                        return None;
                    }
                    let v: Vec<usize> = (0..nx).map(|k| ((cx(lon0 + (k as f64 + 0.5) * dlon / nx as f64) + 0.5).floor().max(0.0) as usize).min(d.ncols - 1)).collect();
                    cmin = cmin.min(v[0]);
                    cmax = cmax.max(*v.last().unwrap());
                    Some(v)
                })
                .collect();
            if cmin == usize::MAX {
                continue;
            }
            let mut row = Vec::new();
            for gy in 0..gh {
                let lat0 = bounds.north - (gy as f64 - 1.0) * dlat;
                if lat0 - dlat * 0.5 > ext.north || lat0 - dlat * 0.5 < ext.south {
                    continue;
                }
                let rows: Vec<usize> = (0..ny).map(|k| ((ry(lat0 - (k as f64 + 0.5) * dlat / ny as f64) + 0.5).floor().max(0.0) as usize).min(d.nrows - 1)).collect();
                let mut sum = vec![0f32; gw];
                let mut cnt = vec![0u8; gw];
                let mut tot = vec![0u8; gw];
                for &r in &rows {
                    d.row(&mut file, r, cmin, cmax + 1, &mut row)?;
                    for (gx, c) in cols.iter().enumerate() {
                        for &cc in c.iter().flatten() {
                            tot[gx] += 1;
                            let v = row[cc - cmin];
                            if v != VOID && v != 0 {
                                sum[gx] += v as f32;
                                cnt[gx] += 1;
                            }
                        }
                    }
                }
                for gx in 0..gw {
                    if tot[gx] > 0 {
                        cover[gy * gw + gx] = true;
                        if cnt[gx] > 0 {
                            grid[gy * gw + gx] = sum[gx] / cnt[gx] as f32;
                        }
                    }
                }
            }
            // shade
            for y in 0..h {
                let lat = bounds.north - (y as f64 + 0.5) * dlat;
                let (mx, my) = (111_320.0 * lat.to_radians().cos().max(0.01) * dlon, 110_540.0 * dlat);
                for x in 0..w {
                    let g = (y + 1) * gw + x + 1;
                    if !cover[g] {
                        continue;
                    }
                    let hgt = grid[g];
                    let px = 4 * (y * w + x);
                    let sea = hgt.is_nan() || (sea_before[y * w + x] && hgt < 15.0) || (hgt < -405.0 && hgt > -430.0 && d.dx < 0.002);
                    sea_before[y * w + x] = sea;
                    if sea {
                        out[px..px + 4].copy_from_slice(&[141, 180, 222, 255]);
                        continue;
                    }
                    let at = |i: usize| {
                        let v = grid[i];
                        if v.is_nan() { hgt } else { v }
                    };
                    let (z, e, wv, n, s) = (hgt, at(g + 1), at(g - 1), at(g - gw), at(g + gw));
                    let (dzdx, dzdy) = (2.0 * (e - wv) as f64 / (2.0 * mx), 2.0 * (n - s) as f64 / (2.0 * my));
                    let slope = (dzdx * dzdx + dzdy * dzdy).sqrt().atan();
                    let aspect = dzdy.atan2(-dzdx);
                    let (zen, az) = (45f64.to_radians(), 315f64.to_radians());
                    let il = (zen.cos() * slope.cos() + zen.sin() * slope.sin() * (az - aspect).cos()).clamp(0.0, 1.0);
                    let base = hypsometric(z);
                    let k = 0.45 + 0.75 * il;
                    for c in 0..3 {
                        out[px + c] = (base[c] as f64 * k).clamp(0.0, 255.0) as u8;
                    }
                    out[px + 3] = 255;
                }
            }
        }
        Ok(out)
    }

    /// A viewport as a PNG: the base plus the lines, routes and site markers of `overlays`
    /// (overlay ids). Text is not drawn (no font here); use [`svg_overlay`](Self::svg_overlay)
    /// for labels. Sea and unmapped areas are transparent.
    pub fn render_png(&self, style: BaseStyle, vp: &Viewport, overlays: &[&str]) -> io::Result<Vec<u8>> {
        let b = vp.bounds();
        let mut img = self.render_base(style, &b, vp.width, vp.height)?;
        let mut canvas = Canvas { w: vp.width as usize, h: vp.height as usize, px: &mut img };
        for id in overlays {
            let Some(o) = self.overlay(id) else { continue };
            match o.kind {
                OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers => {
                    for l in self.lines(id, &b)? {
                        let st = bwv_style(o.kind, l.rank);
                        let xy: Vec<(f64, f64)> = l.points.iter().map(|p| vp.project(p.lat, p.lon)).collect();
                        canvas.polyline(&xy, st.rgb, 1, st.dotted);
                    }
                }
                OverlayKind::Sites => {
                    for p in &o.paths {
                        let xy: Vec<(f64, f64)> = p.tessellate(12).iter().map(|q| vp.project(q.lat, q.lon)).collect();
                        canvas.polyline(&xy, stock_colour(p.colour), p.width.max(1) as usize, p.style == 2);
                    }
                    for l in o.labels.iter().filter(|l| l.kind == LabelKind::Site && l.min_ppd <= vp.ppd) {
                        let (x, y) = vp.project(l.lat, l.lon);
                        canvas.dot(x, y, 2, [255, 255, 255]);
                        canvas.dot(x, y, 1, [200, 0, 0]);
                    }
                }
                _ => {}
            }
        }
        Ok(encode_png(vp.width, vp.height, &img))
    }

    /// The vector content of `overlays` for a viewport as an SVG fragment in pixel coordinates
    /// (`width` x `height`, same as the base image): lines, routes with arrowheads, site markers
    /// and text labels (zoom-dependent visibility and sizes as BibleWorks does, roughly). Meant to be
    /// laid over the PNG from [`render_base_png`](Self::render_base_png); text is XML-escaped.
    // ponytail: label size = points * viewport ppd / the label's own reference ppd, clamped to
    // 8..22 px, instead of BibleWorks' exact auto-sizing; no collision avoidance of labels.
    pub fn svg_overlay(&self, vp: &Viewport, overlays: &[&str], white_on_black: bool) -> io::Result<String> {
        let b = vp.bounds();
        let mut s = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n",
            w = vp.width,
            h = vp.height
        );
        let (fg, halo) = if white_on_black { ("#fff", "#000") } else { ("#111", "#fff") };
        let pts = |v: &[Point]| v.iter().map(|p| { let (x, y) = vp.project(p.lat, p.lon); format!("{x:.1},{y:.1}") }).collect::<Vec<_>>().join(" ");
        for id in overlays {
            let Some(o) = self.overlay(id) else { continue };
            match o.kind {
                OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers => {
                    for l in self.lines(id, &b)? {
                        let st = bwv_style(o.kind, l.rank);
                        s += &format!(
                            "<polyline points=\"{}\" stroke=\"{}\" stroke-width=\"1\"{}/>\n",
                            pts(&l.points),
                            rgb_hex(st.rgb),
                            if st.dotted { " stroke-dasharray=\"1 3\"" } else { "" }
                        );
                    }
                }
                OverlayKind::Sites => {
                    for p in &o.paths {
                        let t = p.tessellate(12);
                        let bb = Bounds::new(t.iter().map(|q| q.lat).fold(f64::MIN, f64::max), t.iter().map(|q| q.lon).fold(f64::MAX, f64::min), t.iter().map(|q| q.lat).fold(f64::MAX, f64::min), t.iter().map(|q| q.lon).fold(f64::MIN, f64::max));
                        if !bb.intersects(&b) {
                            continue;
                        }
                        let col = rgb_hex(stock_colour(p.colour));
                        s += &format!("<polyline points=\"{}\" stroke=\"{col}\" stroke-width=\"{}\"{}/>\n", pts(&t), p.width.max(1), if p.style == 2 { " stroke-dasharray=\"2 4\"" } else if p.style == 1 { " stroke-dasharray=\"6 3\"" } else { "" });
                        if p.arrow && t.len() >= 2 {
                            let (a, z) = (vp.project(t[t.len() - 2].lat, t[t.len() - 2].lon), vp.project(t[t.len() - 1].lat, t[t.len() - 1].lon));
                            let ang = (z.1 - a.1).atan2(z.0 - a.0);
                            let (l, wd) = (8.0 + 2.0 * p.width as f64, 0.45);
                            s += &format!(
                                "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{col}\" stroke=\"none\"/>\n",
                                z.0, z.1, z.0 - l * (ang - wd).cos(), z.1 - l * (ang - wd).sin(), z.0 - l * (ang + wd).cos(), z.1 - l * (ang + wd).sin()
                            );
                        }
                    }
                    for l in &o.labels {
                        if l.min_ppd > vp.ppd || !b.contains(l.lat, l.lon) && !b.contains(l.text_at.map_or(l.lat, |t| t.lat), l.text_at.map_or(l.lon, |t| t.lon)) {
                            continue;
                        }
                        let px = (l.font.size_pt as f64 * vp.ppd / l.font.ref_ppd.max(1.0)).clamp(8.0, 22.0);
                        let (mx, my) = vp.project(l.lat, l.lon);
                        let (tx, ty) = match (l.kind, l.text_at) {
                            (LabelKind::Site, Some(t)) => vp.project(t.lat, t.lon),
                            (LabelKind::Site, None) => (mx + 5.0, my - 2.0),
                            (LabelKind::Text, _) => (mx, my),
                        };
                        if l.kind == LabelKind::Site {
                            s += &format!("<circle cx=\"{mx:.1}\" cy=\"{my:.1}\" r=\"3\" fill=\"#c00\" stroke=\"#fff\" stroke-width=\"1\"/>\n");
                        }
                        let base = ty + px * 0.8; // (lat, lon) of a label is its upper-left corner
                        let rot = if l.font.angle != 0 { format!(" transform=\"rotate({} {tx:.1} {ty:.1})\"", -l.font.angle) } else { String::new() };
                        let spacing = if l.font.spread > 0 { format!(" letter-spacing=\"{}\"", l.font.spread) } else { String::new() };
                        s += &format!(
                            "<text x=\"{tx:.1}\" y=\"{base:.1}\" font-size=\"{px:.1}\" font-family=\"Georgia, serif\" fill=\"{fg}\" stroke=\"{halo}\" stroke-width=\"{:.1}\" paint-order=\"stroke\"{}{rot}{spacing}>{}</text>\n",
                            px / 5.0,
                            if l.font.style & 1 != 0 { " font-weight=\"bold\"" } else { "" },
                            xml_escape(l.display.trim())
                        );
                    }
                }
                _ => {}
            }
        }
        s.push_str("</svg>\n");
        Ok(s)
    }

    /// [`render_base`](Self::render_base) as a PNG file in memory (no overlays).
    pub fn render_base_png(&self, style: BaseStyle, vp: &Viewport) -> io::Result<Vec<u8>> {
        Ok(encode_png(vp.width, vp.height, &self.render_base(style, &vp.bounds(), vp.width, vp.height)?))
    }
}

fn wildcard(p: &[char], s: &[char]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((&'*', rest)) => (0..=s.len()).any(|i| wildcard(rest, &s[i..])),
        Some((&'?', rest)) => !s.is_empty() && wildcard(rest, &s[1..]),
        Some((c, rest)) => s.first() == Some(c) && wildcard(rest, &s[1..]),
    }
}

fn rgb_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn xml_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            c if (c as u32) < 0x20 => String::new(),
            c => c.to_string(),
        })
        .collect()
}

fn hypsometric(z: f32) -> [u8; 3] {
    const T: [(f32, [f32; 3]); 9] = [
        (-450.0, [86.0, 130.0, 104.0]),
        (0.0, [110.0, 158.0, 98.0]),
        (150.0, [160.0, 190.0, 112.0]),
        (500.0, [206.0, 206.0, 132.0]),
        (900.0, [216.0, 186.0, 118.0]),
        (1500.0, [176.0, 140.0, 100.0]),
        (2200.0, [146.0, 116.0, 96.0]),
        (3000.0, [225.0, 222.0, 220.0]),
        (4200.0, [255.0, 255.0, 255.0]),
    ];
    if z <= T[0].0 {
        return T[0].1.map(|v| v as u8);
    }
    for w in T.windows(2) {
        if z <= w[1].0 {
            let t = (z - w[0].0) / (w[1].0 - w[0].0);
            return [0, 1, 2].map(|i| (w[0].1[i] + (w[1].1[i] - w[0].1[i]) * t) as u8);
        }
    }
    T[8].1.map(|v| v as u8)
}

/// Paints one bitmap file (full or reduced) onto the view buffer.
fn paint_bitmap(path: &FsPath, lb: &Bounds, clip: &[Point], nodata: Option<u8>, view: &Bounds, w: usize, h: usize, out: &mut [u8]) -> io::Result<()> {
    let mut bmp = Bmp::open(path)?;
    let (dlon, dlat) = (view.lon_span() / w as f64, view.lat_span() / h as f64);
    let xs = axis(w, (view.west - lb.west) / lb.lon_span() * bmp.w as f64, dlon / lb.lon_span() * bmp.w as f64, bmp.w);
    let ys = axis(h, (lb.north - view.north) / lb.lat_span() * bmp.h as f64, dlat / lb.lat_span() * bmp.h as f64, bmp.h);
    let (mut c0, mut c1) = (usize::MAX, 0usize);
    for &(a, s) in xs.iter().flatten() {
        c0 = c0.min(taps(a, s, bmp.w).next().unwrap());
        c1 = c1.max(taps(a, s, bmp.w).last().unwrap() + 1);
    }
    if c0 >= c1 {
        return Ok(());
    }
    let mut rowbuf: Vec<Vec<u8>> = Vec::new();
    for (oy, y) in ys.iter().enumerate() {
        let Some((ya, ys_span)) = *y else { continue };
        let rows: Vec<usize> = taps(ya, ys_span, bmp.h).collect();
        rowbuf.resize(rows.len(), Vec::new());
        for (k, &r) in rows.iter().enumerate() {
            bmp.indices(r, c0, c1, &mut rowbuf[k])?;
        }
        let lat = view.north - (oy as f64 + 0.5) * dlat;
        for (ox, x) in xs.iter().enumerate() {
            let Some((xa, xs_span)) = *x else { continue };
            if !clip.is_empty() && !in_polygon(clip, lat, view.west + (ox as f64 + 0.5) * dlon) {
                continue;
            }
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for c in taps(xa, xs_span, bmp.w) {
                for row in &rowbuf {
                    if nodata == Some(row[c - c0]) {
                        continue;
                    }
                    let p = bmp.pal.get(row[c - c0] as usize).copied().unwrap_or([0, 0, 0]);
                    r += p[0] as u32;
                    g += p[1] as u32;
                    b += p[2] as u32;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            let px = 4 * (oy * w + ox);
            out[px..px + 4].copy_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8, 255]);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- BWV

struct BwvRec {
    id: i32,
    max_lat: i32,
    min_lat: i32,
    max_lon: i32,
    min_lon: i32,
    offset: u64,
    rank: u32,
}

struct BwvIndex {
    recs: Vec<BwvRec>,
    len: u64,
}

/// Arc seconds to degrees of longitude (below -50 means east of the date line: add 360).
fn arc(v: i32) -> f64 {
    let d = v as f64 / 3600.0;
    if d < -50.0 { d + 360.0 } else { d }
}

fn arc_lat(v: i32) -> f64 {
    v as f64 / 3600.0
}

impl BwvIndex {
    fn read(path: &FsPath) -> io::Result<BwvIndex> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        let mut h = [0u8; 20];
        f.read_exact(&mut h)?;
        let hs = le32(&h, 4) as u64;
        let n = le32(&h, 8) as u64;
        if le16(&h, 0) != 2 || le16(&h, 2) != 0x2077 || hs < 20 || hs + n * 32 > len {
            return Err(bad("not a BWV v2 file"));
        }
        f.seek(SeekFrom::Start(hs))?;
        let mut raw = vec![0u8; (n * 32) as usize];
        f.read_exact(&mut raw)?;
        let recs = raw
            .chunks_exact(32)
            .map(|c| BwvRec {
                id: le32(c, 0) as i32,
                max_lat: le32(c, 4) as i32,
                min_lat: le32(c, 8) as i32,
                max_lon: le32(c, 12) as i32,
                min_lon: le32(c, 16) as i32,
                offset: le32(c, 20) as u64,
                rank: le32(c, 28),
            })
            .collect();
        Ok(BwvIndex { recs, len })
    }

    fn lines(&self, path: &FsPath, bounds: &Bounds) -> io::Result<Vec<Line>> {
        let mut f = File::open(path)?;
        let mut out = Vec::new();
        let mut buf = Vec::new();
        for r in &self.recs {
            // a box that straddles the date line wraps to west > east; it is tiny, so just reorder
            let (w, e) = (arc(r.min_lon), arc(r.max_lon));
            let rb = Bounds::new(arc_lat(r.max_lat), w.min(e), arc_lat(r.min_lat), w.max(e));
            if rb.north < rb.south || !rb.intersects(bounds) {
                continue;
            }
            f.seek(SeekFrom::Start(r.offset))?;
            let mut head = [0u8; 16];
            if r.offset + 16 > self.len || f.read_exact(&mut head).is_err() {
                continue;
            }
            let n = le32(&head, 12) as i32;
            if n < 0 || r.offset + 16 + 8 * n as u64 > self.len {
                continue;
            }
            buf.resize(8 * n as usize, 0);
            f.read_exact(&mut buf)?;
            let mut points = Vec::with_capacity(n as usize + 1);
            points.push(Point { lat: arc_lat(le32(&head, 4) as i32), lon: arc(le32(&head, 0) as i32) });
            for c in buf.chunks_exact(8) {
                let p = Point { lat: arc_lat(le32(c, 4) as i32), lon: arc(le32(c, 0) as i32) };
                if points.last() != Some(&p) {
                    points.push(p);
                }
            }
            out.push(Line { id: r.id, rank: r.rank, points });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- drawing onto RGBA

struct Canvas<'a> {
    w: usize,
    h: usize,
    px: &'a mut [u8],
}

impl Canvas<'_> {
    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = 4 * (y as usize * self.w + x as usize);
            self.px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    fn dot(&mut self, x: f64, y: f64, r: i64, c: [u8; 3]) {
        for dy in -r..=r {
            for dx in -r..=r {
                self.put(x.round() as i64 + dx, y.round() as i64 + dy, c);
            }
        }
    }
    fn polyline(&mut self, pts: &[(f64, f64)], c: [u8; 3], width: usize, dotted: bool) {
        let mut step = 0usize;
        for w in pts.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) || (x0.abs() > 1e6 || y0.abs() > 1e6 || x1.abs() > 1e6 || y1.abs() > 1e6) {
                continue;
            }
            // skip segments wholly off one side of the image
            if (x0 < 0.0 && x1 < 0.0) || (y0 < 0.0 && y1 < 0.0) || (x0 >= self.w as f64 && x1 >= self.w as f64) || (y0 >= self.h as f64 && y1 >= self.h as f64) {
                continue;
            }
            let n = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as usize;
            let r = (width as i64 - 1) / 2;
            for i in 0..=n {
                let t = i as f64 / n as f64;
                step += 1;
                if dotted && !step.is_multiple_of(4) {
                    continue;
                }
                let (x, y) = ((x0 + (x1 - x0) * t).round() as i64, (y0 + (y1 - y0) * t).round() as i64);
                for dy in -r..=r + (width as i64 % 2 == 0) as i64 {
                    for dx in -r..=r + (width as i64 % 2 == 0) as i64 {
                        self.put(x + dx, y + dy, c);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- PNG

fn crc32(data: &[u8], mut crc: u32) -> u32 {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    crc = !crc;
    for &b in data {
        crc = t[((crc ^ b as u32) & 255) as usize] ^ (crc >> 8);
    }
    !crc
}

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code (sent most significant bit first).
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }
    fn lit(&mut self, s: u32) {
        match s {
            0..=143 => self.code(0x30 + s, 8),
            144..=255 => self.code(0x190 + s - 144, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(0xC0 + s - 280, 8),
        }
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// zlib stream: one fixed-Huffman deflate block with greedy hash-chain LZ77.
fn zlib_fixed(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut b = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
    b.put(1, 1);
    b.put(1, 2);
    let hash = |i: usize| ((data[i] as usize) << 10 ^ (data[i + 1] as usize) << 5 ^ data[i + 2] as usize) & 0x7FFF;
    let mut head = vec![u32::MAX; 1 << 15];
    let mut prev = vec![u32::MAX; n];
    let mut i = 0;
    while i < n {
        let (mut best, mut dist) = (0usize, 0usize);
        if i + 3 <= n {
            let hsh = hash(i);
            let (mut cand, mut tries) = (head[hsh], 0);
            while cand != u32::MAX && tries < 24 {
                let c = cand as usize;
                if i - c > 32768 {
                    break;
                }
                let max = (n - i).min(258);
                let mut l = 0;
                while l < max && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best {
                    best = l;
                    dist = i - c;
                    if l == max {
                        break;
                    }
                }
                cand = prev[c];
                tries += 1;
            }
            prev[i] = head[hsh];
            head[hsh] = i as u32;
        }
        if best >= 3 {
            let lc = LEN_BASE.iter().rposition(|&v| v as usize <= best).unwrap();
            b.lit(257 + lc as u32);
            b.put((best - LEN_BASE[lc] as usize) as u32, LEN_EXTRA[lc] as u32);
            let dc = DIST_BASE.iter().rposition(|&v| v as usize <= dist).unwrap();
            b.code(dc as u32, 5);
            b.put((dist - DIST_BASE[dc] as usize) as u32, DIST_EXTRA[dc] as u32);
            for k in 1..best {
                let p = i + k;
                if p + 3 <= n {
                    let hp = hash(p);
                    prev[p] = head[hp];
                    head[hp] = p as u32;
                }
            }
            i += best;
        } else {
            b.lit(data[i] as u32);
            i += 1;
        }
    }
    b.lit(256);
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            s += a;
        }
        a %= 65521;
        s %= 65521;
    }
    b.out.extend(((s << 16) | a).to_be_bytes());
    b.out
}

/// Encodes 8-bit RGBA (row-major, `width * height * 4` bytes; short input is zero-padded) as a PNG.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let stride = w * 4;
    let owned;
    let rgba = if rgba.len() == stride * h {
        rgba
    } else {
        let mut v = rgba.to_vec();
        v.resize(stride * h, 0);
        owned = v;
        &owned
    };
    // per-row filter (none, sub, up, paeth) chosen by the smallest sum of absolute values
    let mut raw = Vec::with_capacity((stride + 1) * h);
    let zero = vec![0u8; stride];
    let mut cand: [Vec<u8>; 4] = std::array::from_fn(|_| vec![0u8; stride]);
    for y in 0..h {
        let row = &rgba[y * stride..(y + 1) * stride];
        let up = if y > 0 { &rgba[(y - 1) * stride..y * stride] } else { &zero[..] };
        for i in 0..stride {
            let (a, b, c) = (if i >= 4 { row[i - 4] } else { 0 }, up[i], if i >= 4 { up[i - 4] } else { 0 });
            let p = a as i32 + b as i32 - c as i32;
            let (pa, pb, pc) = ((p - a as i32).abs(), (p - b as i32).abs(), (p - c as i32).abs());
            let paeth = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
            cand[0][i] = row[i];
            cand[1][i] = row[i].wrapping_sub(a);
            cand[2][i] = row[i].wrapping_sub(b);
            cand[3][i] = row[i].wrapping_sub(paeth);
        }
        let best = (0..4).min_by_key(|&k| cand[k].iter().map(|&v| (v as i8).unsigned_abs() as u64).sum::<u64>()).unwrap();
        raw.push([0u8, 1, 2, 4][best]);
        raw.extend_from_slice(&cand[best]);
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend((data.len() as u32).to_be_bytes());
        png.extend(kind);
        png.extend(data);
        png.extend(crc32(data, crc32(kind, 0)).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &zlib_fixed(&raw));
    chunk(b"IEND", &[]);
    png
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn open() -> Option<MapData> {
        let dir = crate::install_dir()?;
        let t = Instant::now();
        let md = MapData::open(&dir).ok()?;
        eprintln!("MapData::open: {:?}, {} overlays, {} views", t.elapsed(), md.overlays.len(), md.views.len());
        Some(md)
    }

    // ---- a tiny inflate for fixed-Huffman blocks, to check our own PNG output without a decoder

    fn inflate_fixed(z: &[u8]) -> Vec<u8> {
        let d = &z[2..];
        let mut pos = 0usize;
        let bit = |n: u32, pos: &mut usize| -> u32 {
            let mut v = 0;
            for i in 0..n {
                v |= (((d[*pos / 8] >> (*pos % 8)) & 1) as u32) << i;
                *pos += 1;
            }
            v
        };
        assert_eq!(bit(1, &mut pos), 1);
        assert_eq!(bit(2, &mut pos), 1);
        let mut out = Vec::new();
        loop {
            // read a fixed literal/length symbol
            let mut code = 0u32;
            let mut len = 0;
            let sym = loop {
                code = (code << 1) | bit(1, &mut pos);
                len += 1;
                match (len, code) {
                    (7, 0..=0x17) => break code + 256,
                    (8, 0x30..=0xBF) => break code - 0x30,
                    (8, 0xC0..=0xC7) => break code - 0xC0 + 280,
                    (9, 0x190..=0x1FF) => break code - 0x190 + 144,
                    _ => assert!(len < 9, "bad code"),
                }
            };
            match sym {
                256 => break,
                0..=255 => out.push(sym as u8),
                s => {
                    let i = (s - 257) as usize;
                    let l = LEN_BASE[i] as usize + bit(LEN_EXTRA[i] as u32, &mut pos) as usize;
                    let mut dc = 0;
                    for _ in 0..5 {
                        dc = (dc << 1) | bit(1, &mut pos);
                    }
                    let dist = DIST_BASE[dc as usize] as usize + bit(DIST_EXTRA[dc as usize] as u32, &mut pos) as usize;
                    for _ in 0..l {
                        out.push(out[out.len() - dist]);
                    }
                }
            }
        }
        out
    }

    fn idat(png: &[u8]) -> Vec<u8> {
        let mut p = 8;
        let mut z = Vec::new();
        while p + 8 <= png.len() {
            let n = u32::from_be_bytes(png[p..p + 4].try_into().unwrap()) as usize;
            let kind = &png[p + 4..p + 8];
            let data = &png[p + 8..p + 8 + n];
            let crc = u32::from_be_bytes(png[p + 8 + n..p + 12 + n].try_into().unwrap());
            assert_eq!(crc, crc32(data, crc32(kind, 0)), "chunk CRC");
            if kind == b"IDAT" {
                z.extend_from_slice(data);
            }
            p += 12 + n;
        }
        z
    }

    #[test]
    fn png_roundtrips_through_our_own_inflate() {
        // a gradient with repeats, so literals, matches and several filters all occur
        let (w, h) = (37usize, 29usize);
        let mut img = Vec::new();
        for y in 0..h {
            for x in 0..w {
                img.extend([(x * 7) as u8, (y * 5) as u8, if (x / 8 + y / 8) % 2 == 0 { 255 } else { 10 }, if x > 30 { 0 } else { 255 }]);
            }
        }
        let png = encode_png(w as u32, h as u32, &img);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), w as u32);
        let raw = inflate_fixed(&idat(&png));
        assert_eq!(raw.len(), (w * 4 + 1) * h);
        // undo the filters
        let mut got = Vec::new();
        let stride = w * 4;
        for y in 0..h {
            let f = raw[y * (stride + 1)];
            let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
            for i in 0..stride {
                let a = if i >= 4 { got[y * stride + i - 4] as i32 } else { 0 };
                let b = if y > 0 { got[(y - 1) * stride + i] as i32 } else { 0 };
                let c = if y > 0 && i >= 4 { got[(y - 1) * stride + i - 4] as i32 } else { 0 };
                let p = a + b - c;
                let paeth = if (p - a).abs() <= (p - b).abs() && (p - a).abs() <= (p - c).abs() { a } else if (p - b).abs() <= (p - c).abs() { b } else { c };
                let add = match f { 0 => 0, 1 => a, 2 => b, 4 => paeth, _ => panic!("filter {f}") };
                got.push((line[i] as i32 + add) as u8);
            }
        }
        assert_eq!(got, img);
        // empty and short input must not panic
        assert!(!encode_png(0, 0, &[]).is_empty());
        assert!(!encode_png(3, 2, &[1, 2, 3]).is_empty());
    }

    #[test]
    fn parsers_cope_with_odd_input() {
        assert!(parse_cfg("garbage\n++ x.ohd\n").len() == 1);
        assert!(parse_ohd("x", "").is_err());
        assert!(parse_ohd("x", "##MLH 1.0\r\n##Label\r\nA\r\n").is_err());
        let (l, p, s) = parse_lab("3\r\n##lbBegin\r\n##type st\r\n##LL1 nan x\r\n##lbEnd\r\n##lbBegin\r\n##type pa\r\n##ptsBegin\r\n1 2\r\n##ptsEnd\r\n##lbEnd\r\n##lbBegin\r\n##type st\r\n##LL1 1.5 2.5\r\n##dispName Caf\u{e9}\r\n##lbEnd\r\n");
        assert_eq!((l.len(), p.len(), s), (1, 0, 2));
        assert_eq!(l[0].name, "Caf\u{e9}");
        assert!(parse_mst("v", "##Title\n\n").is_none());
        assert_eq!(decode_cp1252(&[b'a', 0x92, 0xB0, 0xE9]), "a\u{2019}\u{b0}\u{e9}");
        assert!(wildcard(&"*jour?ey*".chars().collect::<Vec<_>>(), &"paul's journey".chars().collect::<Vec<_>>()));
        assert!(MapData::open(FsPath::new("/no/such/place")).is_err());
        assert_eq!(xml_escape("a<b & \"c\">"), "a&lt;b &amp; &quot;c&quot;&gt;");
    }

    #[test]
    fn curve_tessellation_passes_through_its_points() {
        let pts: Vec<Point> = [(0.0, 0.0), (1.0, 1.0), (0.0, 2.0), (1.0, 3.0)].iter().map(|&(a, b)| Point { lat: a, lon: b }).collect();
        let p = Path { kind: PathKind::Curve, points: pts.clone(), width: 1, style: 0, colour: 0, arrow: false, closed: false, tension: 0.0 };
        let t = p.tessellate(8);
        assert_eq!(t.len(), 3 * 8 + 1);
        assert_eq!(t[0], pts[0]);
        assert_eq!(t[8], pts[1]);
        assert_eq!(*t.last().unwrap(), pts[3]);
    }

    #[test]
    fn catalogue_matches_the_install() {
        let Some(md) = open() else { return };
        for p in &md.problems {
            eprintln!("PROBLEM {}: {}", p.file, p.message);
        }
        assert!(md.problems.is_empty(), "files that did not parse: {:?}", md.problems);
        let count = |k: OverlayKind| md.overlays.iter().filter(|o| o.kind == k).count();
        assert_eq!(md.overlays.len(), 153);
        assert_eq!(count(OverlayKind::Bitmap), 14);
        assert_eq!(count(OverlayKind::Boundaries), 3);
        assert_eq!(count(OverlayKind::Coasts), 3);
        assert_eq!(count(OverlayKind::Rivers), 4);
        assert_eq!(count(OverlayKind::Sites), 129);
        // every overlay is listed in BWMaps.cfg, so every one knows its folder
        assert!(md.overlays.iter().all(|o| !o.group.is_empty() || o.kind == OverlayKind::Sites), "unlisted: {:?}", md.overlays.iter().filter(|o| o.group.is_empty()).map(|o| &o.id).collect::<Vec<_>>());
        assert_eq!(md.overlay("paul1").unwrap().group.last().map(String::as_str), Some("Journeys of Paul"));
        assert_eq!(md.overlay("mosaic").unwrap().group.last().map(String::as_str), Some("Satellite imagery"));
        assert_eq!(md.overlay("high38").unwrap().group, ["Background bitmaps", "Satellite imagery", "Landsat, detailed"]);
        let acts = md.overlay("bk43-act").unwrap();
        assert_eq!(acts.label, "Places mentioned in Acts");
        assert!(acts.group.iter().any(|g| g.contains("NT")), "{:?}", acts.group);
        assert_eq!(acts.labels.len(), 79);
        assert!(md.overlay("BK43-ACT").is_some());
        // the 14 bitmaps: size from the .ohd matches the BMP header, pyramid levels exist
        let mosaic = md.overlay("mosaic").unwrap();
        assert_eq!(mosaic.bitmap_size, Some((11949, 18220)));
        assert!((mosaic.bitmap_ppd() - 3816.0).abs() < 2.0);
        assert!(!mosaic.clip.is_empty());
        assert!(md.reduced.get("mosaic.fei").is_some_and(|v| v.iter().any(|(p, _)| *p == 25)));
        // views
        assert!(md.views.len() >= 100, "{}", md.views.len());
        let paul = md.views.iter().find(|v| v.title == "Journeys of Paul").unwrap();
        assert!(paul.overlays.iter().any(|f| f == "paul1.lab"));
        assert!(paul.overlays.iter().all(|f| md.overlay_by_datafile(f).is_some()), "view lists an unknown overlay: {:?}", paul.overlays);
        // the shipped data has a few dangling references (the Song of Solomon view names `Bk21-son.lab`,
        // the file is `Bk21-Sol.lab`); anything beyond a handful would mean we resolve names wrongly
        let mut dangling: Vec<String> = md.views.iter().flat_map(|v| v.overlays.iter().filter(|f| md.overlay_by_datafile(f).is_none()).map(|f| format!("{}: {f}", v.id))).collect();
        dangling.sort();
        eprintln!("dangling view references: {dangling:?}");
        assert!(dangling.len() <= 3, "{dangling:?}");
    }

    #[test]
    fn sites_are_where_they_should_be() {
        let Some(md) = open() else { return };
        let near = |name: &str, lat: f64, lon: f64, tol: f64| {
            let hits = md.find_place(name);
            assert!(!hits.is_empty(), "{name} not found");
            assert!(hits.iter().any(|h| (h.lat - lat).abs() < tol && (h.lon - lon).abs() < tol), "{name}: {:?}", hits.iter().map(|h| (h.lat, h.lon)).take(5).collect::<Vec<_>>());
        };
        near("Jerusalem", 31.78, 35.22, 0.1);
        near("Capernaum", 32.88, 35.57, 0.1);
        near("Rome", 41.9, 12.5, 0.2);
        near("Nazareth", 32.70, 35.30, 0.1);
        near("Damascus", 33.5, 36.3, 0.2);
        near("Corinth", 37.9, 22.9, 0.2);
        near("Ephesus", 37.93, 27.33, 0.2);
        near("Sinai", 28.5, 33.97, 0.1); // Mt. Sinai (Horeb)
        // names are matched case-insensitively, by Bible-version spelling, and with wildcards
        assert!(md.find_place("capernaum").iter().any(|h| h.overlay == "bk39-mat"));
        assert!(md.find_place("Salem").iter().any(|h| h.display.contains("Jerusalem")));
        assert!(md.find_place("*journey*").iter().any(|h| h.overlay == "abraham"), "{:?}", md.find_place("*journey*").len());
        assert!(md.find_place("").is_empty());
        assert!(md.find_place("zzzzqqq").is_empty());
        // exact matches come first
        assert_eq!(md.find_place("Rome")[0].name, "Rome");
        // Hermon at 2814 m
        near("Hermon", 33.41, 35.857, 0.05);
    }

    #[test]
    fn all_coordinates_lie_inside_the_dataset() {
        let Some(md) = open() else { return };
        // union of the bitmap boxes, with a little slack (a few sites sit on the 45N edge)
        let world = md.overlays.iter().filter(|o| o.kind == OverlayKind::Bitmap).filter_map(|o| o.bounds).reduce(|a, b| a.union(&b)).unwrap();
        let world = Bounds::new(world.north + 1.0, world.west - 1.0, world.south - 1.0, world.east + 1.0);
        let mut n = 0;
        for o in &md.overlays {
            for l in &o.labels {
                assert!(world.contains(l.lat, l.lon), "{}: {} at {},{}", o.id, l.name, l.lat, l.lon);
                // LL2 is the far corner of a text label, or 0,0 when unused
                assert!((l.lat2 == 0.0 && l.lon2 == 0.0) || world.contains(l.lat2, l.lon2), "{}: {} LL2 {},{}", o.id, l.name, l.lat2, l.lon2);
                if let Some(t) = l.text_at {
                    assert!(world.contains(t.lat, t.lon), "{}: {} label at {},{}", o.id, l.name, t.lat, t.lon);
                }
                n += 1;
            }
            for p in &o.paths {
                assert!(p.points.len() >= 2);
                assert!(p.points.iter().all(|q| world.contains(q.lat, q.lon)), "{}: path leaves the dataset", o.id);
                assert!((0..=15).contains(&p.colour), "{}: colour {}", o.id, p.colour);
            }
        }
        assert!(n > 6000, "{n} labels");
        // the routes
        let exodus = md.overlay("exodusrt").unwrap();
        assert_eq!(exodus.paths.len(), 16);
        assert!(exodus.paths.iter().all(|p| p.kind == PathKind::Curve));
        let paul: usize = ["paul1", "paul2", "paul3", "paul4"].iter().map(|id| md.overlay(id).unwrap().paths.len()).sum();
        assert_eq!(paul, 8 + 13 + 12 + 7);
        let abraham = md.overlay("abraham").unwrap();
        assert_eq!(abraham.paths.len(), 7);
        let curve = abraham.paths.iter().find(|p| p.kind == PathKind::Curve && p.points.len() >= 3).expect("a multi-point curve");
        assert!(curve.tessellate(10).len() > curve.points.len());
        // Paul's journeys begin in Syria/Cilicia, so they stay north of 30N and between Cyprus and Italy
        for id in ["paul1", "paul2", "paul3", "paul4"] {
            for p in &md.overlay(id).unwrap().paths {
                assert!(p.points.iter().all(|q| (30.0..45.0).contains(&q.lat) && (10.0..40.0).contains(&q.lon)), "{id}");
            }
        }
    }

    #[test]
    fn every_vector_file_parses_and_stays_inside_its_index_boxes() {
        let Some(md) = open() else { return };
        let world = Bounds::new(90.0, -180.0, -90.0, 360.0);
        let mut total = 0;
        for o in md.overlays.iter().filter(|o| matches!(o.kind, OverlayKind::Boundaries | OverlayKind::Coasts | OverlayKind::Rivers)) {
            let t = Instant::now();
            let lines = md.lines(&o.id, &world).unwrap_or_else(|e| panic!("{}: {e}", o.id));
            assert_eq!(lines.len(), md.line_count(&o.id).unwrap(), "{}: some records were dropped", o.id);
            let pts: usize = lines.iter().map(|l| l.points.len()).sum();
            eprintln!("{}: {} lines, {} points, {:?}", o.id, lines.len(), pts, t.elapsed());
            for l in &lines {
                assert!(!l.points.is_empty() && l.rank >= 1, "{} line {}", o.id, l.id);
                assert!(l.points.iter().all(|p| (-50.0..360.0).contains(&p.lon) && (-60.0..=90.0).contains(&p.lat)), "{} line {} leaves the world: {:?}", o.id, l.id, l.points.iter().find(|p| !((-50.0..360.0).contains(&p.lon) && (-60.0..=90.0).contains(&p.lat))));
            }
            total += pts;
        }
        assert!(total > 1_000_000, "{total}");
        // the Mediterranean coast near Haifa: water.bwv + the Europe/Asia/Africa coast files hold lines there
        let holy = Bounds::new(33.5, 34.0, 31.0, 36.0);
        let n: usize = ["eur-cil", "asi-cil", "afr-cil"].iter().map(|id| md.lines(id, &holy).unwrap().len()).sum();
        assert!(n > 0);
        let water = md.lines("water", &holy).unwrap();
        assert!(water.len() > 20, "{}", water.len());
        // a viewport query is much smaller than the world
        assert!(water.len() < md.line_count("water").unwrap());
        // the Jordan or the Nile appear as rivers somewhere in the box (rank <= 5 = major)
        assert!(md.lines("afr-riv", &Bounds::new(31.5, 29.5, 29.5, 33.0)).unwrap().iter().any(|l| l.rank <= 5));
    }

    #[test]
    fn heights_make_sense() {
        let Some(md) = open() else { return };
        let dead_sea = md.elevation_at(31.5, 35.5).unwrap();
        assert!((-440..-390).contains(&dead_sea), "Dead Sea {dead_sea}");
        let hermon = md.elevation_at(33.416, 35.857).unwrap();
        assert!(hermon > 2600, "Hermon {hermon}");
        let jerusalem = md.elevation_at(31.78, 35.22).unwrap();
        assert!((600..900).contains(&jerusalem), "Jerusalem {jerusalem}");
        let tabor = md.elevation_at(32.687, 35.390).unwrap();
        assert!((400..650).contains(&tabor), "Tabor {tabor}");
        // outside the coarse grid (Arabia is inside, the Indian Ocean is not) and over the open sea
        assert_eq!(md.elevation_at(0.0, 80.0), None);
        assert_eq!(md.elevation_at(33.5, 30.0), None);
        // the coarse grid alone knows Rome and Etna
        assert!(md.elevation_at(41.9, 12.5).unwrap() < 200);
        assert!(md.elevation_at(37.75, 14.99).unwrap() > 2500);
    }

    fn px(img: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = 4 * (y * w + x);
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }
    /// Pixel for a coordinate in an image of `b`.
    fn at(img: &[u8], b: &Bounds, w: usize, h: usize, lat: f64, lon: f64) -> [u8; 4] {
        let x = ((lon - b.west) / b.lon_span() * w as f64) as usize;
        let y = ((b.north - lat) / b.lat_span() * h as f64) as usize;
        px(img, w, x.min(w - 1), y.min(h - 1))
    }
    fn is_blue(p: [u8; 4]) -> bool {
        p[2] as i32 > p[0] as i32 + 25 && p[3] == 255
    }

    #[test]
    fn rendered_bases_put_land_and_sea_in_the_right_places() {
        let Some(md) = open() else { return };
        let holy = Bounds::new(33.6, 34.2, 30.9, 36.2);
        let (w, h) = (640usize, 448usize);

        let t = Instant::now();
        let elev = md.render_base(BaseStyle::Elevation, &holy, w as u32, h as u32).unwrap();
        eprintln!("elevation {w}x{h}: {:?}", t.elapsed());
        assert_eq!(elev.len(), w * h * 4);
        assert!(is_blue(at(&elev, &holy, w, h, 32.5, 34.2 + 0.05)), "Mediterranean");
        assert!(is_blue(at(&elev, &holy, w, h, 31.5, 35.5)), "Dead Sea {:?}", at(&elev, &holy, w, h, 31.5, 35.5));
        assert!(!is_blue(at(&elev, &holy, w, h, 31.78, 35.22)), "Jerusalem is land");
        assert!(elev.chunks(4).all(|p| p[3] == 255), "fully covered");

        let t = Instant::now();
        let sat = md.render_base(BaseStyle::Satellite, &holy, w as u32, h as u32).unwrap();
        eprintln!("satellite {w}x{h}: {:?}", t.elapsed());
        let dead = at(&sat, &holy, w, h, 31.5, 35.5);
        let jer = at(&sat, &holy, w, h, 31.78, 35.2);
        let med = at(&sat, &holy, w, h, 32.5, 34.3);
        assert!((dead[0] as u32 + dead[1] as u32 + dead[2] as u32) < 300 && dead[2] >= dead[0], "Dead Sea is dark water: {dead:?}");
        assert!(is_blue(med), "the sea beyond the Landsat scene is the elevation map's blue: {med:?}");
        assert!(jer[0] > 100, "Judean hills are bright: {jer:?}");

        let t = Instant::now();
        let rel = md.render_base(BaseStyle::Relief, &holy, w as u32, h as u32).unwrap();
        eprintln!("relief {w}x{h}: {:?}", t.elapsed());
        assert!(is_blue(at(&rel, &holy, w, h, 31.5, 35.5)), "Dead Sea is water in the relief");
        assert!(is_blue(at(&rel, &holy, w, h, 32.5, 34.4)));
        let hermon = at(&rel, &holy, w, h, 33.41, 35.857);
        assert!(hermon[0] > 140 && hermon[1] > 120, "Hermon is high and pale: {hermon:?}");

        // the same view through a PNG with overlays and SVG
        let vp = Viewport::fit(&holy, w as u32, h as u32);
        let png = md.render_png(BaseStyle::Satellite, &vp, &["paul1", "bk39-mat", "water", "eur-cil"]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let svg = md.svg_overlay(&Viewport { lat: 32.5, lon: 35.5, ppd: 400.0, width: 800, height: 600 }, &["bk39-mat", "jesus_min_path"], false).unwrap();
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("Capernaum") && svg.contains("<polyline"), "{}", &svg[..svg.len().min(600)]);
        assert!(!svg.contains("<text") || !svg.contains("&&"));
    }

    #[test]
    fn rendering_is_cheap_even_for_the_biggest_files() {
        let Some(md) = open() else { return };
        // Galilee at full resolution of the 700 MB Landsat mosaic (about 4000 ppd: 0.1 degree is 400 px)
        let b = Bounds::new(32.90, 35.50, 32.70, 35.70);
        let t = Instant::now();
        let img = md.render_base(BaseStyle::Satellite, &b, 800, 800).unwrap();
        let dt = t.elapsed();
        eprintln!("full-resolution 800x800 satellite window: {dt:?}");
        assert_eq!(img.len(), 800 * 800 * 4);
        assert!(dt.as_secs_f64() < 5.0, "{dt:?}");
        // the lake is dark and surrounded by brighter land
        let lake = at(&img, &b, 800, 800, 32.82, 35.60);
        assert!(lake[0] < 90, "Sea of Galilee: {lake:?}");
        // the whole Mediterranean at low zoom
        let t = Instant::now();
        let wide = Bounds::new(45.0, -10.0, 25.0, 50.0);
        let img = md.render_base(BaseStyle::Satellite, &wide, 1200, 400).unwrap();
        eprintln!("1200x400 of the whole eastern Mediterranean: {:?}", t.elapsed());
        assert_eq!(img.len(), 1200 * 400 * 4);
        let _ = md.render_base(BaseStyle::LandCover, &wide, 600, 200).unwrap();
        // degenerate requests are errors, not panics
        assert!(md.render_base(BaseStyle::Elevation, &Bounds::new(0.0, 0.0, 1.0, 1.0), 10, 10).is_err());
        assert!(md.render_base(BaseStyle::Elevation, &b, 0, 10).is_err());
        // a window entirely off the data is empty and transparent
        let off = md.render_base(BaseStyle::Satellite, &Bounds::new(-10.0, 100.0, -20.0, 110.0), 32, 32).unwrap();
        assert!(off.iter().all(|&v| v == 0));
    }

    /// Writes sample images when `BW_MAPS_OUT` names a folder (not part of the normal run).
    #[test]
    fn write_samples_when_asked() {
        let (Some(md), Some(out)) = (open(), std::env::var_os("BW_MAPS_OUT")) else { return };
        let out = PathBuf::from(out);
        let holy = Viewport { lat: 32.0, lon: 35.3, ppd: 250.0, width: 800, height: 900 };
        for (name, style, ov) in [
            ("holyland-satellite", BaseStyle::Satellite, vec!["water", "bk39-mat"]),
            ("holyland-elevation", BaseStyle::Elevation, vec!["water"]),
            ("holyland-relief", BaseStyle::Relief, vec!["eur-cil", "asi-cil", "afr-cil", "water", "jesus_min_path", "jesus_min_path2"]),
        ] {
            fs::write(out.join(format!("{name}.png")), md.render_png(style, &holy, &ov).unwrap()).unwrap();
            fs::write(out.join(format!("{name}.svg")), md.svg_overlay(&holy, &ov, false).unwrap()).unwrap();
        }
        let med = Viewport { lat: 36.0, lon: 25.0, ppd: 18.0, width: 1300, height: 600 };
        fs::write(out.join("med-satellite.png"), md.render_png(BaseStyle::Satellite, &med, &["afr-cil", "eur-cil", "asi-cil", "afr-bdy", "eur-bdy", "paul1", "paul2", "paul3", "paul4"]).unwrap()).unwrap();
        fs::write(out.join("med-landcover.png"), md.render_png(BaseStyle::LandCover, &med, &["afr-cil", "eur-cil", "asi-cil"]).unwrap()).unwrap();
        let jer = Viewport { lat: 31.772, lon: 35.235, ppd: 20000.0, width: 900, height: 700 };
        fs::write(out.join("jerusalem.png"), md.render_png(BaseStyle::Satellite, &jer, &["jerusdet"]).unwrap()).unwrap();
    }
}
