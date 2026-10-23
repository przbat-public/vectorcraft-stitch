//! The embroidery rows of `vectorcraft-cli perf` and the embroidery times of `vectorcraft-cli
//! bench`: how long the stitch plan takes on documents built to stress each part of it. Kept apart
//! from the rest of `perf.rs`, which calls [`rows`] from one place.
//!
//! The documents:
//! - the synthetic document of [`PATHS`] mixed paths, planned cold, warm (every object's stitches
//!   kept from an earlier plan) and after a nudge, with and without `expand`, and the sewing order
//!   and checks read from its kept plan;
//! - fills whose narrow parts are many satin columns in one shape: concentric rings, diagonal
//!   stripes and separate bars in one compound path;
//! - a lattice: a fill with a hundred holes, which its travels go round;
//! - strokes inside a clip group whose clipping path has thousands of segments;
//! - thousands of objects each in a colour of its own;
//! - a symbol placed a thousand times (its art copied for each instance on every plan);
//! - the embroidery export, which plans its document afresh rather than from what was kept;
//! - the 50,000-path document, which needs more stitches than a design may have: how long it takes
//!   to be refused, the first time and again;
//! - stitch pictures of 10,000 to 2,000,000 stitches ([`stitch_picture`]: fill rows, satin columns
//!   and running lines in twelve threads, built in code, no planner), drawn by the stitch preview's
//!   renderer at fit and zoomed in, on the default threads and on one (as the web build draws).
//!
//! The budgets are estimates of what a stitch preview that follows edits needs: a cold plan of a
//! heavy shape within a fraction of a second, a warm plan and a re-plan after a nudge within 100 ms.
//! A design too large is refused only once it is planned up to the limit (the stitch estimate is an
//! upper bound, so refusing on it would refuse designs that fit): its first refusal is given twice
//! the time of a cold plan of the two million stitches (the 1,000-path document's 90,000 stitches
//! plan in about 0.1 s), and refusing it again is at once. The drawing budgets are the stitch
//! preview's gate (estimates, from a cost model calibrated on the art renderer's rows): a frame of
//! 100,000 stitches at fit within 50 ms and of 500,000 within 100 ms (the art's refresh after a pan),
//! one zoomed in within a frame (16 ms), 10,000 on one thread within 20 ms (the web build, which has
//! no threads, is two to four times slower), and 2,000,000 within a second.

use std::time::Instant;

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, Node, NodeKind};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, Point, Shape, kurbo};
use vectorcraft_render::lines::{LinePicture, LinePictureBuilder, LineRenderer, LineStyle, LinkKind, MarkKind, edge_against};

use crate::perf::synthetic;

/// The paths of the synthetic document whose embroidery is planned: few enough to stay within the
/// 2 million stitches a design may have (the 50,000-path document needs more), about a hundred
/// embroidery designs' worth of objects.
pub const PATHS: usize = 1000;
/// The paths of the document that needs more stitches than a design may have.
const REFUSED_PATHS: usize = 50_000;
/// How far an object is moved to edit it before a re-plan, millimetres: a nudge.
const NUDGE_MM: f64 = 0.5;
/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// How many times each row is measured (the median is reported): the heavy shapes are planned once
/// a run.
const RUNS: usize = 3;

/// How many times each drawing row is drawn (the median is reported).
const DRAW_RUNS: usize = 5;

/// The rows' names, as the report lists them (at most 42 characters, the report's column).
pub const ROWS: [&str; 25] = [
    "embroidery: cold plan, 1,000 paths",
    "embroidery: warm plan, 1,000 paths",
    "embroidery: nudge and re-plan, 1,000 paths",
    "embroidery: expand 0.3, warm, 1,000 paths",
    "embroidery: expand 0.3, nudge, 1,000 paths",
    "embroidery: sequence, 1,000 paths (kept)",
    "embroidery: check, 1,000 paths (kept)",
    "embroidery: 10 rings in one fill, cold",
    "embroidery: 20 rings in one fill, cold",
    "embroidery: 30 stripes in one fill, cold",
    "embroidery: 100 bars in one fill, cold",
    "embroidery: 200 bars in one fill, cold",
    "embroidery: lattice of 100 holes, cold",
    "embroidery: 500 clipped strokes, cold",
    "embroidery: 500 clipped strokes, warm",
    "embroidery: 5,000 colours, warm",
    "embroidery: 1,000 symbol instances, warm",
    "embroidery: DST export, 1,000 paths",
    "embroidery: refused, 50,000 paths, first",
    "embroidery: refused, 50,000 paths, again",
    "stitches: draw 10,000 at fit, one thread",
    "stitches: draw 100,000 at fit",
    "stitches: draw 100,000 at 400%",
    "stitches: draw 500,000 at fit",
    "stitches: draw 2,000,000 at fit",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: estimates.
const BUDGETS: [f64; 25] = [
    400.0, 100.0, 100.0, 100.0, 100.0, 50.0, 50.0, 300.0, 600.0, 300.0, 300.0, 600.0, 400.0, 1000.0, 100.0, 100.0, 100.0, 400.0, 4000.0, 50.0, 20.0,
    50.0, 16.0, 100.0, 1000.0,
];

/// The median of `samples` (milliseconds), 0 for none.
fn middle(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples.get(samples.len() / 2).copied().unwrap_or(0.0)
}

fn err(e: EngineError) -> String {
    e.to_string()
}

/// A session that has never planned `doc`.
fn fresh(doc: &Document) -> Session {
    let mut s = Session::new();
    s.add_document(doc.clone(), None);
    s
}

/// Milliseconds `command` takes with `params`, and its answer.
fn timed(s: &mut Session, command: &str, params: &Value) -> Result<(f64, Value), String> {
    let t = Instant::now();
    let v = s.execute(command, params).map_err(err)?;
    Ok((t.elapsed().as_secs_f64() * 1000.0, v))
}

/// Milliseconds to plan `doc`'s embroidery (`embroidery.stats` with the export's `options`):
/// cold, in a session that never planned it; warm, every unit kept from earlier plans of the same
/// art (another snapshot of it: an undo); and after moving one object 0.5 mm; each the median of
/// `runs` (at least one). `None` when the document has nothing to stitch.
pub fn embroidery_times(doc: &Document, runs: usize, options: &Value) -> Result<Option<(f64, f64, f64)>, String> {
    let runs = runs.max(1);
    let mut s = fresh(doc);
    let order = s.execute("embroidery.sequence", options).map_err(err)?;
    let Some(id) = order["blocks"].get(0).and_then(|b| b["units"].get(0)).and_then(|u| u["id"].as_u64()) else { return Ok(None) };
    let mut cold = Vec::with_capacity(runs);
    for _ in 0..runs {
        cold.push(timed(&mut fresh(doc), "embroidery.stats", options)?.0);
    }
    // Moved and planned, then each undo and redo a plan of a snapshot whose units are all kept.
    let nudge = json!({"ids": [id], "dx": NUDGE_MM * PT_PER_MM, "dy": 0});
    s.execute("object.move", &nudge).map_err(err)?;
    timed(&mut s, "embroidery.stats", options)?;
    let mut warm = Vec::with_capacity(runs);
    for k in 0..runs {
        s.execute(if k % 2 == 0 { "edit.undo" } else { "edit.redo" }, &json!({})).map_err(err)?;
        warm.push(timed(&mut s, "embroidery.stats", options)?.0);
    }
    let mut edited = Vec::with_capacity(runs);
    for _ in 0..runs {
        s.execute("object.move", &nudge).map_err(err)?;
        edited.push(timed(&mut s, "embroidery.stats", options)?.0);
    }
    Ok(Some((middle(cold), middle(warm), middle(edited))))
}

/// Milliseconds a cold plan of `doc` takes (`embroidery.stats`), the median of [`RUNS`].
fn cold(doc: &Document) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        samples.push(timed(&mut fresh(doc), "embroidery.stats", &json!({}))?.0);
    }
    Ok(middle(samples))
}

/// Milliseconds `command` takes on `doc` once its plan is kept (planned once before), the median
/// of [`RUNS`]. The export (as DST) plans afresh, by design: what a file holds never depends on
/// what was kept.
fn kept(doc: &Document, command: &str) -> Result<f64, String> {
    let mut s = fresh(doc);
    s.execute("embroidery.stats", &json!({})).map_err(err)?;
    let params = if command == "document.export" { json!({"format": "dst"}) } else { json!({}) };
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        samples.push(timed(&mut s, command, &params)?.0);
    }
    Ok(middle(samples))
}

/// Milliseconds a warm plan of `doc` takes: another snapshot of the same art, every unit kept.
fn warm(doc: &Document) -> Result<f64, String> {
    let mut s = fresh(doc);
    s.execute("embroidery.stats", &json!({})).map_err(err)?;
    let id = s.execute("embroidery.sequence", &json!({})).map_err(err)?["blocks"][0]["units"][0]["id"].clone();
    s.execute("object.move", &json!({"ids": [id], "dx": NUDGE_MM * PT_PER_MM, "dy": 0})).map_err(err)?;
    s.execute("embroidery.stats", &json!({})).map_err(err)?;
    let mut samples = Vec::with_capacity(RUNS);
    for k in 0..RUNS {
        s.execute(if k % 2 == 0 { "edit.undo" } else { "edit.redo" }, &json!({})).map_err(err)?;
        samples.push(timed(&mut s, "embroidery.stats", &json!({}))?.0);
    }
    Ok(middle(samples))
}

/// Milliseconds until a plan of `doc` is refused (it needs more stitches than a design may have),
/// the first time and again in the same session; an error when it is not refused.
fn refused(doc: &Document) -> Result<(f64, f64), String> {
    let mut s = fresh(doc);
    let mut once = || {
        let t = Instant::now();
        match s.execute("embroidery.stats", &json!({})) {
            Ok(_) => Err("the 50,000-path document was planned: it should need more stitches than a design may have".to_string()),
            Err(_) => Ok(t.elapsed().as_secs_f64() * 1000.0),
        }
    };
    let first = once()?;
    Ok((first, once()?))
}

/// The embroidery rows: each name ([`ROWS`]), what it measured (milliseconds) and its budget.
pub fn rows() -> Result<Vec<(&'static str, f64, f64)>, String> {
    let mut ms: Vec<f64> = Vec::with_capacity(ROWS.len());
    let doc = synthetic(PATHS)?;
    let Some((c, w, n)) = embroidery_times(&doc, RUNS, &json!({}))? else {
        return Err("embroidery: the synthetic document has nothing to stitch".into());
    };
    ms.extend([c, w, n]);
    let Some((_, w, n)) = embroidery_times(&doc, RUNS, &json!({"expand": 0.3}))? else {
        return Err("embroidery: the synthetic document has nothing to stitch".into());
    };
    ms.extend([w, n]);
    ms.push(kept(&doc, "embroidery.sequence")?);
    ms.push(kept(&doc, "embroidery.check")?);
    ms.push(cold(&one_fill(rings(10), FillRule::EvenOdd))?);
    ms.push(cold(&one_fill(rings(20), FillRule::EvenOdd))?);
    ms.push(cold(&one_fill(stripes(30), FillRule::NonZero))?);
    ms.push(cold(&one_fill(bars(100), FillRule::NonZero))?);
    ms.push(cold(&one_fill(bars(200), FillRule::NonZero))?);
    ms.push(cold(&one_fill(lattice(10), FillRule::EvenOdd))?);
    let clipped = clipped_strokes(500, 2000)?;
    ms.push(cold(&clipped)?);
    ms.push(warm(&clipped)?);
    ms.push(warm(&colours(5000)?)?);
    ms.push(warm(&symbols(1000)?)?);
    ms.push(kept(&doc, "document.export")?);
    let (first, again) = refused(&synthetic(REFUSED_PATHS)?)?;
    ms.extend([first, again]);
    ms.push(draw(&stitch_picture(10_000), FIT, Some(0)));
    let hundred = stitch_picture(100_000);
    ms.push(draw(&hundred, FIT, None));
    ms.push(draw(&hundred, ZOOMED, None));
    ms.push(draw(&stitch_picture(500_000), FIT, None));
    ms.push(draw(&stitch_picture(2_000_000), FIT, None));
    Ok(ROWS.iter().zip(ms).zip(BUDGETS).map(|((name, ms), budget)| (*name, ms, budget)).collect())
}

/// `vectorcraft-cli perf --embroidery`: only the embroidery rows, each printed by `row` (its name,
/// milliseconds, budget, and whether it is within it). With `VECTORCRAFT_STITCH_SWEEP` set, the
/// drawing's batch sizes are swept instead ([`sweep`]); with `VECTORCRAFT_STITCH_PNG` set to a
/// folder, the drawing rows' pictures are written there as PNG, to look at.
pub fn report(row: fn(&str, f64, f64, bool) -> String) -> Result<(), String> {
    if std::env::var_os("VECTORCRAFT_STITCH_SWEEP").is_some() {
        return sweep();
    }
    if let Some(dir) = std::env::var_os("VECTORCRAFT_STITCH_PNG") {
        pictures(std::path::Path::new(&dir))?;
    }
    let rows = rows()?;
    outln!("  {:<42} {:>10} {:>10}", "", "measured", "budget");
    let mut over = 0;
    for (name, ms, budget) in rows {
        over += usize::from(ms > budget);
        outln!("{}", row(name, ms, budget, ms <= budget));
    }
    if over == 0 { Ok(()) } else { Err(format!("{over} embroidery budget(s) exceeded")) }
}

/// The canvas the drawing rows draw on, device pixels: a 15-inch laptop's retina window.
const CANVAS: (u32, u32) = (2880, 1800);
/// How much of the canvas a design fitted to it fills (the fit leaves a margin).
const FIT_SHARE: f64 = 0.95;
/// The views the drawing rows draw: fitted, and zoomed in four times about the centre.
const FIT: f64 = 1.0;
const ZOOMED: f64 = 4.0;

/// The thread colours of the stitch pictures: twelve, white among them (a thread close to the white
/// fabric, drawn with an edge).
const THREADS: [[u8; 3]; 12] = [
    [200, 30, 30],
    [30, 60, 200],
    [40, 150, 60],
    [250, 250, 250],
    [20, 20, 20],
    [240, 200, 40],
    [150, 60, 170],
    [250, 130, 30],
    [90, 200, 220],
    [120, 80, 40],
    [240, 150, 180],
    [130, 130, 130],
];

/// One tile of a stitch picture, millimetres: its side, the distance between its fill rows and the
/// longest stitch along them (the planner's tatami defaults), its satin column's width and the
/// advance of each crossing, and its running line's stitch.
const TILE: f64 = 20.0;
const ROW_SPACING: f64 = 0.38;
const FILL_STITCH: f64 = 4.0;
const SATIN_WIDTH: f64 = 3.0;
const SATIN_ADVANCE: f64 = 0.2;
const RUN_STITCH: f64 = 2.5;
/// How wide the threads are laid, millimetres (40 wt).
const LAID: f32 = 0.4;
/// Every how many tiles the thread is cut before the next one (the other moves keep it).
const CUT_EVERY: usize = 4;

/// A stitch picture of about `n` stitches, as the stitch preview draws a design: square tiles in a
/// grid, each a fill of rows, a satin column along its side and a running line round it, in a
/// thread of its own; a move between tiles, cut every few tiles.
pub fn stitch_picture(n: usize) -> LinePicture {
    let mut b = LinePictureBuilder::new(Point::ZERO, PT_PER_MM).straight_within(0.002);
    let (mut number, mut tag, mut tile) = (1u32, 0u32, 0usize);
    let columns = (n as f64 / 450.0).sqrt().ceil().max(1.0) as usize;
    let mut last: Option<Point> = None;
    while (number as usize) < n {
        let (x0, y0) = ((tile % columns) as f64 * TILE, (tile / columns) as f64 * TILE);
        let group = (tile % THREADS.len()) as u32;
        let first = Point::new(x0, y0);
        if let Some(from) = last {
            let cut = tile.is_multiple_of(CUT_EVERY);
            if cut {
                b.mark(from, MarkKind::Cross, tag, number);
            }
            b.link(from, first, if cut { LinkKind::Cut } else { LinkKind::Thread }, group, tag, number);
            number += 1;
        }
        // The fill: rows across the tile, there and back.
        b.begin_line(group, tag, LAID);
        let rows = (TILE / ROW_SPACING) as usize;
        let per_row = (TILE / FILL_STITCH).ceil() as usize;
        for r in 0..rows {
            let y = y0 + r as f64 * ROW_SPACING;
            for k in 0..=per_row {
                let t = k as f64 / per_row as f64;
                let x = if r % 2 == 0 { x0 + t * TILE } else { x0 + (1.0 - t) * TILE };
                b.point(Point::new(x, y), number);
                number += 1;
            }
        }
        tag += 1;
        // The satin column: a zigzag down the tile's left side.
        b.begin_line(group, tag, LAID);
        let crossings = (TILE / SATIN_ADVANCE) as usize;
        for k in 0..crossings {
            let x = if k % 2 == 0 { x0 } else { x0 + SATIN_WIDTH };
            b.point(Point::new(x, y0 + k as f64 * SATIN_ADVANCE), number);
            number += 1;
        }
        tag += 1;
        // The running line round the tile.
        b.begin_line(group, tag, LAID);
        let per_side = (TILE / RUN_STITCH) as usize;
        let corners = [(0.0, 0.0), (TILE, 0.0), (TILE, TILE), (0.0, TILE), (0.0, 0.0)];
        for side in corners.windows(2) {
            for k in 0..per_side {
                let t = k as f64 / per_side as f64;
                let (x, y) = (side[0].0 + (side[1].0 - side[0].0) * t, side[0].1 + (side[1].1 - side[0].1) * t);
                b.point(Point::new(x0 + x, y0 + y), number);
                number += 1;
            }
        }
        b.end_line();
        tag += 1;
        last = Some(first);
        tile += 1;
    }
    b.build()
}

/// Milliseconds a frame of `picture` takes on the drawing rows' canvas, fitted and zoomed by
/// `zoom` about its centre, on `threads` threads (`None`: the default), the median of
/// [`DRAW_RUNS`].
fn draw(picture: &LinePicture, zoom: f64, threads: Option<u16>) -> f64 {
    draw_with(picture, zoom, threads, vectorcraft_render::lines::MAX_BATCH_SEGMENTS)
}

/// [`draw`] with paths of at most `batch` segments.
fn draw_with(picture: &LinePicture, zoom: f64, threads: Option<u16>, batch: usize) -> f64 {
    let mut r = LineRenderer::new();
    r.batch_segments = batch;
    if let Some(t) = threads {
        r.threads = t;
    }
    let (style, view) = framed(picture, zoom);
    let (w, h) = CANVAS;
    // A first frame makes the contexts.
    drop(r.render(picture, w, h, view, &style));
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        let t = Instant::now();
        drop(r.render(picture, w, h, view, &style));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    middle(samples)
}

/// The batch sizes [`sweep`] measures: from a few lines a path to a whole run of one colour.
const SWEPT_BATCHES: [usize; 7] = [64, 128, 256, 512, 1024, 4096, usize::MAX];

/// `VECTORCRAFT_STITCH_SWEEP`: the drawing rows at each of [`SWEPT_BATCHES`].
fn sweep() -> Result<(), String> {
    let (ten, hundred, five) = (stitch_picture(10_000), stitch_picture(100_000), stitch_picture(500_000));
    outln!("  {:>10} {:>14} {:>14} {:>14} {:>14}", "batch", "100k fit", "100k 400%", "500k fit", "10k 1 thread");
    for batch in SWEPT_BATCHES {
        let shown = if batch == usize::MAX { "a run".to_string() } else { batch.to_string() };
        outln!(
            "  {shown:>10} {:>11.2} ms {:>11.2} ms {:>11.2} ms {:>11.2} ms",
            draw_with(&hundred, FIT, None, batch),
            draw_with(&hundred, ZOOMED, None, batch),
            draw_with(&five, FIT, None, batch),
            draw_with(&ten, FIT, Some(0), batch)
        );
    }
    Ok(())
}

/// `VECTORCRAFT_STITCH_PNG`: the 10,000-stitch picture at fit and zoomed in, written to `dir`.
fn pictures(dir: &std::path::Path) -> Result<(), String> {
    let p = stitch_picture(10_000);
    let mut r = LineRenderer::new();
    let (w, h) = CANVAS;
    for (name, zoom) in [("stitches-fit.png", FIT), ("stitches-zoomed.png", ZOOMED)] {
        let (style, view) = framed(&p, zoom);
        let style = LineStyle { background: Some([255, 255, 255, 255]), ..style };
        let png = r.render(&p, w, h, view, &style).to_png()?;
        std::fs::write(dir.join(name), png).map_err(|e| format!("{}: {e}", dir.join(name).display()))?;
    }
    Ok(())
}

/// The style the drawing rows draw with, and the view that fits `picture` to their canvas, zoomed
/// by `zoom` about its centre.
fn framed(picture: &LinePicture, zoom: f64) -> (LineStyle, Affine) {
    let (w, h) = CANVAS;
    let style = LineStyle {
        colors: THREADS.to_vec(),
        edges: THREADS.iter().map(|c| edge_against(*c, [255, 255, 255])).collect(),
        px_per_pt: 2.0,
        ..LineStyle::default()
    };
    let Some(bounds) = picture.bounds() else { return (style, Affine::IDENTITY) };
    let fit = (f64::from(w) / bounds.width()).min(f64::from(h) / bounds.height()) * FIT_SHARE * zoom;
    let view = Affine::translate((f64::from(w) / 2.0, f64::from(h) / 2.0)) * Affine::scale(fit) * Affine::translate(-bounds.center().to_vec2());
    (style, view)
}

/// A document holding one filled path of `path` (millimetres) under `rule`.
fn one_fill(path: BezPath, rule: FillRule) -> Document {
    let mut d = Document::new(600.0, 600.0);
    let mut pts = path;
    pts.apply_affine(kurbo::Affine::scale(PT_PER_MM));
    let id = d.alloc_id();
    let mut node = Node::path(id, PathData::from_bezpath(&pts), Appearance::basic(Paint::solid(Color::rgb(0.8, 0.1, 0.1)), Paint::None, 0.0));
    if let NodeKind::Path { rule: r, .. } = &mut node.kind {
        *r = rule;
    }
    let layer = d.layers.first().map(|l| l.id);
    // A fresh document always has a layer to insert into.
    let _ = d.insert(layer, usize::MAX, node);
    d
}

/// A closed polygon through `corners` (millimetres).
fn polygon(path: &mut BezPath, corners: &[(f64, f64)]) {
    for (k, &(x, y)) in corners.iter().enumerate() {
        if k == 0 { path.move_to((x, y)) } else { path.line_to((x, y)) }
    }
    path.close_path();
}

/// `n` concentric rings 2 mm wide, 2 mm apart, the first 5 mm from the centre (both circles of
/// each, to be filled even-odd): every ring a narrow part sewn as a satin column.
fn rings(n: usize) -> BezPath {
    /// The rings' centre, their width, the gap between them and the first one's inner radius
    /// (millimetres).
    const CENTRE: f64 = 150.0;
    const WIDTH: f64 = 2.0;
    const GAP: f64 = 2.0;
    const INNER: f64 = 5.0;
    let mut path = BezPath::new();
    for k in 0..n {
        let r = INNER + k as f64 * (WIDTH + GAP);
        for radius in [r, r + WIDTH] {
            path.extend(kurbo::Circle::new((CENTRE, CENTRE), radius).path_elements(0.1));
        }
    }
    path
}

/// `n` stripes 3 mm wide and 150 mm long at 45°, 6 mm apart: each a narrow part.
fn stripes(n: usize) -> BezPath {
    /// A stripe's width and length, the distance from one to the next, and where the first starts
    /// (millimetres).
    const WIDTH: f64 = 3.0;
    const LENGTH: f64 = 150.0;
    const PITCH: f64 = 6.0;
    const START: (f64, f64) = (20.0, 120.0);
    let along = kurbo::Vec2::new(std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2);
    let across = kurbo::Vec2::new(-along.y, along.x);
    let mut path = BezPath::new();
    for k in 0..n {
        let o = Point::new(START.0, START.1) + across * (k as f64 * PITCH);
        let corners = [o, o + along * LENGTH, o + along * LENGTH + across * WIDTH, o + across * WIDTH].map(|q| (q.x, q.y));
        polygon(&mut path, &corners);
    }
    path
}

/// `n` separate bars 3 × 30 mm, 3 mm apart, in rows of 50: each a narrow part.
fn bars(n: usize) -> BezPath {
    /// A bar's width and length, the distance from one to the next, and the bars a row holds.
    const WIDTH: f64 = 3.0;
    const LENGTH: f64 = 30.0;
    const PITCH: f64 = 6.0;
    const PER_ROW: usize = 50;
    let mut path = BezPath::new();
    for k in 0..n {
        let (x, y) = (10.0 + (k % PER_ROW) as f64 * PITCH, 10.0 + (k / PER_ROW) as f64 * (LENGTH + PITCH));
        polygon(&mut path, &[(x, y), (x + WIDTH, y), (x + WIDTH, y + LENGTH), (x, y + LENGTH)]);
    }
    path
}

/// A square plate with `side × side` square holes 5 mm wide, 8 mm apart (wider than a satin
/// column is sewn, so the plate is tatami that travels round the holes); filled even-odd.
fn lattice(side: usize) -> BezPath {
    /// A hole's width, the web between holes, and the margin round them (millimetres).
    const HOLE: f64 = 5.0;
    const WEB: f64 = 8.0;
    const MARGIN: f64 = 8.0;
    let size = 2.0 * MARGIN + side as f64 * (HOLE + WEB) - WEB;
    let mut path = BezPath::new();
    polygon(&mut path, &[(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)]);
    for i in 0..side {
        for j in 0..side {
            let (x, y) = (MARGIN + i as f64 * (HOLE + WEB), MARGIN + j as f64 * (HOLE + WEB));
            polygon(&mut path, &[(x, y), (x + HOLE, y), (x + HOLE, y + HOLE), (x, y + HOLE)]);
        }
    }
    path
}

/// `strokes` straight running lines across a clip group whose clipping path, a wavy circle 150 mm
/// across, has `segments` straight segments: every stitch tested against it.
fn clipped_strokes(strokes: usize, segments: usize) -> Result<Document, String> {
    /// The clipping circle's centre and radius, and how far and how often its edge waves
    /// (millimetres, waves round).
    const CENTRE: f64 = 100.0;
    const RADIUS: f64 = 75.0;
    const WAVE: f64 = 3.0;
    const WAVES: f64 = 8.0;
    /// A line's stroke width (points: narrower than a satin column, so a running line).
    const LINE_PT: f64 = 1.0;
    let mm = |x: f64, y: f64| Point::new(x * PT_PER_MM, y * PT_PER_MM);
    let mut d = Document::new(600.0, 600.0);
    let mut clip = BezPath::new();
    for k in 0..segments {
        let a = std::f64::consts::TAU * k as f64 / segments as f64;
        let r = RADIUS + WAVE * (WAVES * a).sin();
        let p = mm(CENTRE + r * a.cos(), CENTRE + r * a.sin());
        if k == 0 { clip.move_to(p) } else { clip.line_to(p) }
    }
    clip.close_path();
    let mut clip_node = Node::path(d.alloc_id(), PathData::from_bezpath(&clip), Appearance::basic(Paint::None, Paint::None, 0.0));
    if let NodeKind::Path { clipping, .. } = &mut clip_node.kind {
        *clipping = true;
    }
    let mut children = vec![std::sync::Arc::new(clip_node)];
    for k in 0..strokes {
        let x = 10.0 + 180.0 * k as f64 / strokes as f64;
        let mut line = BezPath::new();
        line.move_to(mm(x, 10.0));
        line.line_to(mm(x + 20.0, 190.0));
        let ink = Color::rgb(0.1, 0.2, 0.7);
        children.push(std::sync::Arc::new(Node::path(
            d.alloc_id(),
            PathData::from_bezpath(&line),
            Appearance::basic(Paint::None, Paint::solid(ink), LINE_PT),
        )));
    }
    let group = Node::new(d.alloc_id(), NodeKind::Group { children, clip: true });
    let layer = d.layers.first().map(|l| l.id);
    d.insert(layer, usize::MAX, group).map_err(|e| format!("building the clipped strokes: {e}"))?;
    Ok(d)
}

/// `n` small squares, each in a colour of its own: as many thread blocks to sequence and match.
fn colours(n: usize) -> Result<Document, String> {
    /// A square's side and the distance between neighbours (points), and how many a row holds.
    const SIDE: f64 = 6.0;
    const PITCH: f64 = 9.0;
    const PER_ROW: usize = 100;
    let mut d = Document::new(1000.0, 1000.0);
    let layer = d.layers.first().map(|l| l.id);
    for k in 0..n {
        let (x, y) = ((k % PER_ROW) as f64 * PITCH, (k / PER_ROW) as f64 * PITCH);
        let mut square = BezPath::new();
        square.move_to((x, y));
        square.line_to((x + SIDE, y));
        square.line_to((x + SIDE, y + SIDE));
        square.line_to((x, y + SIDE));
        square.close_path();
        // Every colour different: a walk through the RGB cube.
        let c = Color::rgb8(((k * 37) % 256) as u8, ((k * 91) % 256) as u8, ((k * 53 + k / 256) % 256) as u8);
        let node = Node::path(d.alloc_id(), PathData::from_bezpath(&square), Appearance::basic(Paint::solid(c), Paint::None, 0.0));
        d.insert(layer, usize::MAX, node).map_err(|e| format!("building the coloured squares: {e}"))?;
    }
    Ok(d)
}

/// A symbol of a few filled shapes placed `n` times in a grid: every instance's art is copied,
/// transformed and keyed on every plan.
fn symbols(n: usize) -> Result<Document, String> {
    /// The shapes in the symbol, the side of each (points), and how far apart the instances are
    /// (points) and how many a row holds.
    const SHAPES: usize = 8;
    const SIDE: f64 = 4.0;
    const PITCH: f64 = 40.0;
    const PER_ROW: usize = 40;
    let mut d = Document::new(1600.0, 1600.0);
    let mut children = Vec::with_capacity(SHAPES);
    for k in 0..SHAPES {
        let x = (k % 4) as f64 * 2.0 * SIDE;
        let y = (k / 4) as f64 * 2.0 * SIDE;
        let mut square = BezPath::new();
        polygon(&mut square, &[(x, y), (x + SIDE, y), (x + SIDE, y + SIDE), (x, y + SIDE)]);
        let c = if k % 2 == 0 { Color::rgb(0.8, 0.1, 0.1) } else { Color::rgb(0.1, 0.2, 0.7) };
        children.push(std::sync::Arc::new(Node::path(
            d.alloc_id(),
            PathData::from_bezpath(&square),
            Appearance::basic(Paint::solid(c), Paint::None, 0.0),
        )));
    }
    let art = Node::new(d.alloc_id(), NodeKind::Group { children, clip: false });
    let name = "Tile".to_string();
    d.symbols.push(vectorcraft_doc::Symbol { name: name.clone(), art: std::sync::Arc::new(art) });
    let layer = d.layers.first().map(|l| l.id);
    for k in 0..n {
        let xf = kurbo::Affine::translate(((k % PER_ROW) as f64 * PITCH, (k / PER_ROW) as f64 * PITCH));
        let instance = Node::new(d.alloc_id(), NodeKind::SymbolInstance { symbol: name.clone(), xf });
        d.insert(layer, usize::MAX, instance).map_err(|e| format!("placing the symbol: {e}"))?;
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row has a budget, and every name fits the report's column.
    #[test]
    fn every_row_has_a_budget_and_fits() {
        assert_eq!(ROWS.len(), BUDGETS.len());
        assert!(ROWS.iter().all(|r| r.chars().count() <= 42), "{ROWS:?}");
    }

    /// The documents built for the rows hold what they say: one fill, a clip group of strokes,
    /// distinct colours.
    #[test]
    fn the_documents_hold_what_they_say() {
        let doc = one_fill(rings(3), FillRule::EvenOdd);
        let (count, colours) = (doc.layers[0].children().map_or(0, Vec::len), colours(10).unwrap());
        assert_eq!(count, 1);
        assert_eq!(colours.layers[0].children().map_or(0, Vec::len), 10);
        let tiles = symbols(3).unwrap();
        assert_eq!((tiles.layers[0].children().map_or(0, Vec::len), tiles.symbols.len()), (3, 1));
        let clipped = clipped_strokes(5, 100).unwrap();
        assert!(matches!(&clipped.layers[0].children().unwrap()[0].kind, NodeKind::Group { clip: true, children } if children.len() == 6));
    }

    /// A stitch picture holds about the stitches asked for, in tiles of three lines with a move
    /// between tiles, and draws.
    #[test]
    fn a_stitch_picture_holds_about_the_stitches_asked_for() {
        let p = stitch_picture(10_000);
        let numbers = p.point_count();
        assert!((10_000..12_000).contains(&numbers), "{numbers}");
        assert!(p.links().len() > 10 && !p.marks().is_empty());
        assert!(draw(&p, FIT, Some(0)) > 0.0);
    }
}
