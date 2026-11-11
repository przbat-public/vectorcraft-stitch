//! The digitizing rows of `vectorcraft-cli perf --embroidery`: how long `embroidery.digitizeImage`
//! takes, stage by stage as its reply times them (`timings`), and in all, on pictures made here.
//!
//! The pictures, none from the corpus:
//! - a badge of 98 × 88 mm in six colours on white (an oval, a ring, a star, a band, bars and
//!   dots), drawn as art and exported by the engine as a JPEG at 10 pixels a millimetre over its
//!   box and a 5 mm margin ([`BADGE_PX`]);
//! - 2,500 specks of 0.36 mm² strewn over an orange field 100 mm square: larger than the trace's
//!   own noise (0.16 mm²), so that it keeps them, and smaller than the smallest area (1 mm²), so
//!   that every one joins the field;
//! - grids of squares in five colours, none beside another of its own: 300 of 4 mm and 1,000 of
//!   3 mm;
//! - 3,600 dots of 1.2 mm in a field, each sewn over it, the field made solid under them;
//! - a line one pixel wide winding between 27,750 bars of 0.2 by 0.9 mm in two colours, too small to
//!   keep, which join the rows they lie in: a thin area with a hundred and fifty neighbours, each
//!   once a thousand specks;
//! - 1,500 × 1,500 pixels of noise in 16 colours, more than a design may be made of;
//! - line art 1,500 pixels a side: a mesh of lines 0.6 mm wide every 2 mm (one line, about 74 × 74
//!   junctions, sewn along its middle) and 75 such hatch lines.

use std::time::Instant;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_render::Rendered;

use super::{PT_PER_MM, RUNS, err, middle};

/// The badge's art (millimetres), the margin round it, and the picture's size at
/// [`PX_PER_MM`] (pixels).
const BADGE_MM: (f64, f64) = (98.0, 88.0);
const MARGIN_MM: f64 = 5.0;
const BADGE_PX: (u32, u32) = (1080, 980);

/// The pictures' resolution: the working resolution digitizing aims for, so that each is worked
/// pixel for pixel (`ppi` as the export takes it: 254 a inch).
pub(super) const PX_PER_MM: f64 = 10.0;
const MM_PER_INCH: f64 = 25.4;

/// The JPEG quality the badge is exported at: a photo editor's usual.
const JPEG_QUALITY: u8 = 80;

/// The badge's colours: its own six, as the command is asked for.
pub(super) const BADGE_COLORS: u32 = 6;

/// The specks: a grid of 50 by 50, each 6 pixels square (0.36 mm²), 20 pixels apart, on a field
/// of 1,000 pixels a side.
const SPECKS: u32 = 50;
const SPECK_PX: u32 = 6;
const SPECK_STEP_PX: u32 = 20;
const FIELD_PX: u32 = 1000;

/// The grids' colours, none of which is beside another of its own (each square's is its column
/// plus twice its row, of five).
const GRID_COLOURS: [[u8; 3]; 5] = [[29, 42, 107], [232, 177, 0], [210, 32, 48], [31, 138, 60], [126, 200, 232]];

/// The grids: 20 by 15 squares of 40 pixels (300 of 4 mm), and 40 by 25 of 30 pixels (1,000 of
/// 3 mm), each as `(columns, rows, side)`.
const SQUARES: (u32, u32, u32) = (20, 15, 40);
const MANY_SQUARES: (u32, u32, u32) = (40, 25, 30);

/// The dots: 60 by 60 squares of 12 pixels (1.2 mm, narrower than an island sewn over its field
/// may be), 24 pixels apart, in a navy field inside a white margin of 20 pixels: 1,480 pixels a side.
const DOTS: u32 = 60;
const DOT_PX: u32 = 12;
const DOT_STEP_PX: u32 = 24;
const DOTS_MARGIN_PX: u32 = 20;
const DOTS_PX: u32 = DOTS * DOT_STEP_PX + 2 * DOTS_MARGIN_PX;

/// The winding line: one pixel wide, a row every 10 pixels, each joined to the next at alternate
/// ends, across 750 pixels; between its rows, bars 2 pixels wide alternating in two colours.
const WINDING_PX: u32 = 750;
const WINDING_PITCH_PX: u32 = 10;
const BAR_PX: u32 = 2;
const WINDING_ROWS: u32 = WINDING_PX / WINDING_PITCH_PX;

/// The noise: 1,500 pixels a side, the working copy's largest, in 16 colours.
const NOISE_PX: u32 = 1500;
const NOISE_COLORS: u32 = 16;

/// The rows' names, in order (at most 42 characters, the report's column).
pub(super) const ROWS: [&str; 14] = [
    "image: prepare a 1,080 × 980 JPEG",
    "image: trace and colours, 6 threads",
    "image: logo mode, 1,080 × 980 JPEG",
    "image: areas merged, 2,500 specks",
    "image: overlap 0.4 mm, 300 areas",
    "image: objects made, 3,601 areas",
    "image: stitches, 98 × 88 mm badge",
    "image: whole dry run, 98 × 88 mm badge",
    "image: 1,500 × 1,500 noise, refused",
    "image: dry run reply, 1,000 areas",
    "image: shapes, 3,600 dots in a field",
    "image: shapes, a line round 27,750 bars",
    "image: centre lines, a 1,500 px mesh",
    "image: whole dry run, 75 hatch lines",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: estimates, about 2.5 times what each
/// measured on 2026-11-03 (release, 12-core Apple Silicon, load about 4, the median of three), as
/// the other rows', so that a regression of that size shows: the JPEG prepared in 1.96 ms, traced
/// with its colours in 126 ms (199 in logo mode), 2,500 specks merged in 46.8 ms, 300 areas grown
/// under each other in 29.2 ms, 3,601 areas made into objects in 0.53 ms, the badge's stitches
/// planned in 33.7 ms and its whole dry run in 208 ms, noise refused in 1.17 s, a reply of 1,000
/// areas built in 5.22 ms, 3,600 dots and their field shaped in 59.5 ms, and the winding line and
/// its rows of joined bars in 429 ms (472 ms on 2026-11-07, its line found along its middle); the
/// line rows', added on 2026-11-07, from [`LINES_MS`].
pub(super) const BUDGETS: [f64; 14] =
    [5.0, 315.0, 500.0, 120.0, 75.0, 1.3, 85.0, 520.0, 2930.0, 13.0, 150.0, 1070.0, LINES_MS[0] * 2.5, LINES_MS[1] * 2.5];

/// What the line rows measured on 2026-11-07 (release, 12-core Apple Silicon, load about 3 to 5,
/// the median of three, in two runs): the mesh's shapes (its centre line found, its holes none) in
/// 393 and 394 ms, and the hatch's whole dry run in 274 and 273 ms.
const LINES_MS: [f64; 2] = [394.0, 274.0];

/// The line pictures: 1,500 pixels a side (150 mm at [`PX_PER_MM`]), navy lines 6 pixels wide
/// (0.6 mm, sewn along their middle) every 20 pixels, the rest transparent: 75 hatch lines across,
/// and a mesh of them both ways inside a transparent margin of 10 pixels (one line with about
/// 74 × 74 junctions; with no margin, three of its corners would be navy, the background).
const LINES_PX: u32 = 1500;
const LINE_PX: u32 = 6;
const LINE_STEP_PX: u32 = 20;
const MESH_MARGIN_PX: u32 = 10;

/// The line pictures' navy, and their transparency.
const LINE_NAVY: [u8; 4] = [29, 42, 107, 255];
const LINE_CLEAR: [u8; 4] = [0, 0, 0, 0];

/// A picture of `width` × `height` opaque pixels, each `colour(x, y)`, as PNG.
fn png(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = colour(x, y);
            pixels.extend_from_slice(&[r, g, b, u8::MAX]);
        }
    }
    Rendered { width, height, pixels }.to_png()
}

/// A picture of `width` × `height` pixels, each `colour(x, y)` with its opacity, as PNG.
fn rgba(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 4]) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&colour(x, y));
        }
    }
    Rendered { width, height, pixels }.to_png()
}

/// A rectangle `[x, y, width, height]` in points from millimetres.
fn rect_pt(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!([x * PT_PER_MM, y * PT_PER_MM, w * PT_PER_MM, h * PT_PER_MM])
}

/// A session holding picture `bytes` (a file named `name`) placed at [`PX_PER_MM`], selected.
fn placed(name: &str, bytes: &[u8], width_px: u32, height_px: u32) -> Result<Session, String> {
    let (w, h) = (f64::from(width_px) / PX_PER_MM, f64::from(height_px) / PX_PER_MM);
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": w * PT_PER_MM, "height": h * PT_PER_MM, "artboards": 1})).map_err(err)?;
    let data = vectorcraft_format::base64_encode(bytes);
    s.execute("file.place", &json!({"name": name, "dataBase64": data, "rect": rect_pt(0.0, 0.0, w, h)})).map_err(err)?;
    s.execute("select.all", &json!({})).map_err(err)?;
    Ok(s)
}

/// The badge, drawn as art and exported by the engine as a JPEG at [`PX_PER_MM`].
fn badge() -> Result<Vec<u8>, String> {
    badge_at(PX_PER_MM)
}

/// The badge, drawn as art and exported by the engine as a JPEG at `px_per_mm` pixels a millimetre.
pub(super) fn badge_at(px_per_mm: f64) -> Result<Vec<u8>, String> {
    let (w, h) = (BADGE_MM.0 + 2.0 * MARGIN_MM, BADGE_MM.1 + 2.0 * MARGIN_MM);
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": w * PT_PER_MM, "height": h * PT_PER_MM, "artboards": 1})).map_err(err)?;
    s.execute("paint.setStroke", &json!({"none": true})).map_err(err)?;
    let at = |v: f64| (v + MARGIN_MM) * PT_PER_MM;
    let size = |v: f64| v * PT_PER_MM;
    let mut shapes: Vec<(&str, &str, Value)> = vec![
        ("#1d2a6b", "shape.ellipse", json!({"x": at(0.0), "y": at(0.0), "width": size(98.0), "height": size(88.0)})),
        ("#e8b100", "shape.ellipse", json!({"x": at(9.0), "y": at(9.0), "width": size(80.0), "height": size(70.0)})),
        ("#1d2a6b", "shape.ellipse", json!({"x": at(13.0), "y": at(13.0), "width": size(72.0), "height": size(62.0)})),
        ("#d22030", "shape.star", json!({"cx": at(49.0), "cy": at(42.0), "radius1": size(22.0), "radius2": size(9.0), "points": 5})),
        ("#1f8a3c", "shape.rectangle", json!({"x": at(19.0), "y": at(62.0), "width": size(60.0), "height": size(7.0)})),
    ];
    for k in 0..5 {
        let x = 27.0 + 10.0 * f64::from(k);
        shapes.push(("#f07a1a", "shape.rectangle", json!({"x": at(x), "y": at(16.0), "width": size(4.0), "height": size(8.0)})));
    }
    for (x, y) in [(20.0, 40.0), (75.0, 40.0), (30.0, 52.0), (65.0, 52.0), (40.0, 74.0), (55.0, 74.0)] {
        shapes.push(("#7ec8e8", "shape.ellipse", json!({"x": at(x), "y": at(y), "width": size(5.0), "height": size(5.0)})));
    }
    for (colour, command, params) in shapes {
        s.execute("paint.setFill", &json!({"color": colour})).map_err(err)?;
        s.execute(command, &params).map_err(err)?;
    }
    let export = json!({"format": "jpg", "ppi": px_per_mm * MM_PER_INCH, "quality": JPEG_QUALITY, "rect": rect_pt(0.0, 0.0, w, h)});
    let r = s.execute("document.export", &export).map_err(err)?;
    let data = r["dataBase64"].as_str().ok_or("perf: the badge's export gave no data")?;
    vectorcraft_format::base64_decode(data).ok_or_else(|| "perf: the badge's export isn't base64".into())
}

/// A session holding the badge's JPEG `bytes`, exported at `px_per_mm`, placed at [`PX_PER_MM`]
/// (so a badge drawn finer is placed larger, its working copy as large as its pixels), selected.
pub(super) fn placed_badge(bytes: &[u8], px_per_mm: f64) -> Result<Session, String> {
    let side = |mm: f64| ((mm + 2.0 * MARGIN_MM) * px_per_mm).round() as u32;
    placed("badge.jpg", bytes, side(BADGE_MM.0), side(BADGE_MM.1))
}

/// Squares `side` pixels a side, `cols` by `rows`, in [`GRID_COLOURS`], as PNG.
fn grid(cols: u32, rows: u32, side: u32) -> Result<Vec<u8>, String> {
    let colours = GRID_COLOURS.len() as u32;
    png(cols * side, rows * side, |x, y| GRID_COLOURS.get(((x / side + 2 * (y / side)) % colours) as usize).copied().unwrap_or_default())
}

/// A dry run of `params` on `s`, `RUNS` times: each run's reply and its milliseconds in all.
fn dry_runs(s: &mut Session, params: Value) -> Result<Vec<(Value, f64)>, String> {
    let mut p = params;
    p["dryRun"] = json!(true);
    let mut out = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        let r = s.execute("embroidery.digitizeImage", &p).map_err(err)?;
        out.push((r, t.elapsed().as_secs_f64() * 1000.0));
    }
    Ok(out)
}

/// The median of the stages `stages` of `runs`' timings, added up.
fn staged(runs: &[(Value, f64)], stages: &[&str]) -> f64 {
    middle(runs.iter().map(|(r, _)| stages.iter().filter_map(|k| r["timings"][*k].as_f64()).sum()).collect())
}

/// The digitizing rows' milliseconds, in the order of [`ROWS`].
pub(super) fn times() -> Result<Vec<f64>, String> {
    let mut ms = Vec::with_capacity(ROWS.len());
    let jpeg = badge()?;
    let mut s = placed("badge.jpg", &jpeg, BADGE_PX.0, BADGE_PX.1)?;
    let runs = dry_runs(&mut s, json!({"colors": BADGE_COLORS}))?;
    ms.push(staged(&runs, &["image"]));
    ms.push(staged(&runs, &["trace", "areas"]));
    let logo = dry_runs(&mut s, json!({"colors": BADGE_COLORS, "mode": "logo"}))?;
    ms.push(staged(&logo, &["trace", "areas"]));
    let specks = png(FIELD_PX, FIELD_PX, |x, y| {
        let speck = |v: u32| v % SPECK_STEP_PX < SPECK_PX && v / SPECK_STEP_PX < SPECKS;
        if speck(x) && speck(y) { [29, 42, 107] } else { [240, 130, 20] }
    })?;
    let mut field = placed("specks.png", &specks, FIELD_PX, FIELD_PX)?;
    let merged = dry_runs(&mut field, json!({"colors": 2, "background": "none"}))?;
    if merged.iter().any(|(r, _)| r["joined"] != json!(SPECKS * SPECKS)) {
        return Err(format!("perf: the specks weren't all merged: {}", merged.first().map_or(Value::Null, |(r, _)| r["joined"].clone())));
    }
    ms.push(staged(&merged, &["areas"]));
    let (cols, rows, side) = SQUARES;
    let mut squares = placed("squares.png", &grid(cols, rows, side)?, cols * side, rows * side)?;
    let squares_runs = dry_runs(&mut squares, json!({"colors": GRID_COLOURS.len()}))?;
    ms.push(staged(&squares_runs, &["shapes"]));
    let dots = png(DOTS_PX, DOTS_PX, |x, y| {
        let inner = |v: u32| (DOTS_MARGIN_PX..DOTS_PX - DOTS_MARGIN_PX).contains(&v);
        let dot = |v: u32| {
            (v - DOTS_MARGIN_PX) % DOT_STEP_PX >= (DOT_STEP_PX - DOT_PX) / 2 && (v - DOTS_MARGIN_PX) % DOT_STEP_PX < (DOT_STEP_PX + DOT_PX) / 2
        };
        match (inner(x) && inner(y), inner(x) && inner(y) && dot(x) && dot(y)) {
            (_, true) => [240, 130, 20],
            (true, false) => [29, 42, 107],
            (false, _) => [255, 255, 255],
        }
    })?;
    let mut dotted = placed("dots.png", &dots, DOTS_PX, DOTS_PX)?;
    let dots_runs = dry_runs(&mut dotted, json!({"colors": 2}))?;
    if dots_runs.iter().any(|(r, _)| r["areaCount"] != json!(DOTS * DOTS + 1)) {
        return Err(format!(
            "perf: the dots aren't {} areas: {}",
            DOTS * DOTS + 1,
            dots_runs.first().map_or(Value::Null, |(r, _)| r["areaCount"].clone())
        ));
    }
    ms.push(staged(&dots_runs, &["objects"]));
    ms.push(staged(&runs, &["stitches"]));
    ms.push(middle(runs.iter().map(|(_, t)| *t).collect()));
    let noise = png(NOISE_PX, NOISE_PX, |x, y| {
        let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761) >> 28;
        let v = (h * (255 / (NOISE_COLORS - 1))) as u8;
        [v, 255 - v, v / 2]
    })?;
    let mut noisy = placed("noise.png", &noise, NOISE_PX, NOISE_PX)?;
    let refusal = json!({"dryRun": true, "colors": NOISE_COLORS, "minArea": 0, "smoothing": 0});
    let mut refused = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        if noisy.execute("embroidery.digitizeImage", &refusal).is_ok() {
            return Err("perf: noise was digitized, not refused as more than a design may be made of".into());
        }
        refused.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.push(middle(refused));
    let (cols, rows, side) = MANY_SQUARES;
    let mut many = placed("many.png", &grid(cols, rows, side)?, cols * side, rows * side)?;
    let replies = dry_runs(&mut many, json!({"colors": GRID_COLOURS.len()}))?;
    let stages = ["image", "trace", "areas", "shapes", "objects", "stitches"];
    ms.push(middle(replies.iter().map(|(r, t)| t - stages.iter().filter_map(|k| r["timings"][*k].as_f64()).sum::<f64>()).collect()));
    ms.push(staged(&dots_runs, &["shapes"]));
    let height = WINDING_ROWS * WINDING_PITCH_PX - (WINDING_PITCH_PX - 1);
    let winding = png(WINDING_PX, height, |x, y| {
        let row = y / WINDING_PITCH_PX;
        let joint = x == if row.is_multiple_of(2) { WINDING_PX - 1 } else { 0 };
        if y.is_multiple_of(WINDING_PITCH_PX) || joint {
            [210, 32, 48]
        } else if (x / BAR_PX).is_multiple_of(2) {
            [29, 42, 107]
        } else {
            [31, 138, 60]
        }
    })?;
    let mut wound = placed("winding.png", &winding, WINDING_PX, height)?;
    ms.push(staged(&dry_runs(&mut wound, json!({"colors": 3, "background": "none"}))?, &["shapes"]));
    let inner = |v: u32| (MESH_MARGIN_PX..LINES_PX - MESH_MARGIN_PX).contains(&v);
    let lines = |mesh: bool| {
        rgba(LINES_PX, LINES_PX, |x, y| {
            let on =
                if mesh { inner(x) && inner(y) && (x % LINE_STEP_PX < LINE_PX || y % LINE_STEP_PX < LINE_PX) } else { y % LINE_STEP_PX < LINE_PX };
            if on { LINE_NAVY } else { LINE_CLEAR }
        })
    };
    let mut mesh = placed("mesh.png", &lines(true)?, LINES_PX, LINES_PX)?;
    let mesh_runs = dry_runs(&mut mesh, json!({"colors": 2}))?;
    if mesh_runs.iter().any(|(r, _)| r["areas"].as_array().is_none_or(|a| a.iter().all(|a| a["line"].is_null()))) {
        return Err("perf: the mesh wasn't sewn along its middle".into());
    }
    ms.push(staged(&mesh_runs, &["shapes"]));
    let mut hatch = placed("hatch.png", &lines(false)?, LINES_PX, LINES_PX)?;
    ms.push(middle(dry_runs(&mut hatch, json!({"colors": 2}))?.iter().map(|(_, t)| *t).collect()));
    Ok(ms)
}
