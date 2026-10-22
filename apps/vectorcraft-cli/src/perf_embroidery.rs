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
//! - the 50,000-path document, which needs more stitches than a design may have: how long it takes
//!   to be refused, the first time and again.
//!
//! The budgets are estimates of what a stitch preview that follows edits needs: a cold plan of a
//! heavy shape within a fraction of a second, a warm plan and a re-plan after a nudge within 100 ms.

use std::time::Instant;

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{Appearance, Document, Node, NodeKind};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_geom::{BezPath, FillRule, PathData, Point, Shape, kurbo};

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
/// How many times each row is measured (its middle is reported): the heavy shapes are planned once
/// a run.
const RUNS: usize = 3;

/// The rows' names, as the report lists them (at most 42 characters, the report's column).
pub const ROWS: [&str; 18] = [
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
    "embroidery: refused, 50,000 paths, first",
    "embroidery: refused, 50,000 paths, again",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: estimates.
const BUDGETS: [f64; 18] =
    [400.0, 100.0, 100.0, 100.0, 100.0, 50.0, 50.0, 300.0, 600.0, 300.0, 300.0, 600.0, 400.0, 1000.0, 100.0, 100.0, 1000.0, 50.0];

/// The middle of `samples` (milliseconds), 0 for none.
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
/// art (another snapshot of it: an undo); and after moving one object 0.5 mm; each the middle of
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

/// Milliseconds a cold plan of `doc` takes (`embroidery.stats`), the middle of [`RUNS`].
fn cold(doc: &Document) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        samples.push(timed(&mut fresh(doc), "embroidery.stats", &json!({}))?.0);
    }
    Ok(middle(samples))
}

/// Milliseconds `command` takes on `doc` once its plan is kept (planned once before), the middle
/// of [`RUNS`].
fn kept(doc: &Document, command: &str) -> Result<f64, String> {
    let mut s = fresh(doc);
    s.execute("embroidery.stats", &json!({})).map_err(err)?;
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        samples.push(timed(&mut s, command, &json!({}))?.0);
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
    let (first, again) = refused(&synthetic(REFUSED_PATHS)?)?;
    ms.extend([first, again]);
    Ok(ROWS.iter().zip(ms).zip(BUDGETS).map(|((name, ms), budget)| (*name, ms, budget)).collect())
}

/// `vectorcraft-cli perf --embroidery`: only the embroidery rows, each printed by `row` (its name,
/// milliseconds, budget, and whether it is within it).
pub fn report(row: fn(&str, f64, f64, bool) -> String) -> Result<(), String> {
    let rows = rows()?;
    outln!("  {:<42} {:>10} {:>10}", "", "measured", "budget");
    let mut over = 0;
    for (name, ms, budget) in rows {
        over += usize::from(ms > budget);
        outln!("{}", row(name, ms, budget, ms <= budget));
    }
    if over == 0 { Ok(()) } else { Err(format!("{over} embroidery budget(s) exceeded")) }
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
        let clipped = clipped_strokes(5, 100).unwrap();
        assert!(matches!(&clipped.layers[0].children().unwrap()[0].kind, NodeKind::Group { clip: true, children } if children.len() == 6));
    }
}
