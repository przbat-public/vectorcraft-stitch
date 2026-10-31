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
//! - entry and exit points: the 20 rings sewn toward an exit on their outermost ring (checked against
//!   the plain rings of the same run), a running line of 64 traced contours and the lattice's tatami
//!   of many regions each sewn toward an exit, and a fill's exit and entry on the 1,000-path document
//!   and the 20 rings' entry moved a nudge at a time, the design planned again after each move;
//! - Nearest Connection on the 1,000- and 22,000-path documents: the design planned, then the
//!   command's search and the plan of its result (a dry run, so each run starts from the same);
//! - picking by stitch on the 22,000-path document: the median, the 95th percentile and the slowest
//!   of a grid of presses, some on empty fabric;
//! - the embroidery tool through the session: its handles found for 32 objects selected and for
//!   Select All on 22,000 paths, a hover frame there, and an exit and a fill's rows' arrow dragged a
//!   step at a time, the design planned again after each;
//! - one needle point of a run of literal stitches moved as Direct Selection moves it
//!   (`path.moveAnchors`) and the design planned again: on the 1,000-path document written as DST
//!   and opened (a design of literal runs, about 90,000 needle points), and on the 22,000-path
//!   document's plan built into literal runs here (about 2 million needle points, more records than
//!   a machine file's reader takes);
//! - every object of the 1,000- and 22,000-path documents converted to manual stitches (Select All,
//!   then the command, which plans the design afresh as the export does), then all restored;
//! - the stitch commands on the 22,000-path document's plan built into literal runs (about 2
//!   million needle points): one needle point moved by `embroidery.moveNeedlePoints` and the design
//!   planned again, a page of 1,000 of `embroidery.stitches` from its middle (the plan kept), and
//!   100,000 needle points selected by their records (`embroidery.selectNeedlePoints`);
//! - the Embroidery panel's Object tab's work on a new snapshot (each frame of a drag or a scrub):
//!   its read of 1,000 selected objects' settings, and its footer's fabric and thread on the
//!   22,000-path document;
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

/// The synthetic documents whose plans the scene rows build scenes of: 1,000 paths (about 90,000
/// stitches), 5,000 (about 450,000) and 22,000 (about 2 million, the most a design may have).
const SCENE_PATHS: [usize; 3] = [1_000, 5_000, 22_000];

/// How long the cancel row lets the 20 rings plan before it cancels them, milliseconds: inside
/// their one unit, which takes about half a second.
const CANCEL_AFTER_MS: u64 = 50;

/// The rows' names, as the report lists them (at most 42 characters, the report's column).
pub const ROWS: [&str; 79] = [
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
    "stitches: draw 100,000 at fit, 45° grid",
    "stitches: draw 100,000 at fit, satin",
    "stitches: draw a corpus PES at fit",
    "stitches: emphasis of a block, 500,000",
    "stitch scene: build, 1,000 paths",
    "stitch scene: build, 5,000 paths",
    "stitch scene: build, 22,000 paths",
    "reply: stats, 22,000 paths (kept)",
    "reply: sequence, 22,000 paths (kept)",
    "reply: check, 22,000 paths (kept)",
    "preview: nudge to frame, 1,000 paths",
    "preview: nudge to frame, a drop shadow",
    "preview: cancel, 20 rings in one fill",
    "sewing order: move one object, re-plan",
    "sewing order: drag half of 22,000 paths",
    "settings: re-plan after one object's angle",
    "settings: re-plan after a fabric change",
    "settings: info of 1,000 objects",
    "object tab: re-read after a nudge, 1,000",
    "object tab: footer after a nudge, 22,000",
    "embroidery: 20 rings toward an exit, cold",
    "points: exit moved, 1,000 paths",
    "points: entry moved, 1,000 paths",
    "nearest connection: 1,000 paths",
    "nearest connection: 22,000 paths",
    "picking: a press on stitches, 22,000 paths",
    "embroidery: 64 contours to an exit, cold",
    "embroidery: lattice toward an exit, cold",
    "points: entry moved, 20 rings in one fill",
    "picking: 95th percentile, 22,000 paths",
    "picking: slowest press, 22,000 paths",
    "tool: handles of 32 objects selected",
    "tool: handles, Select All, 22,000 paths",
    "tool: a hover frame, 22,000 paths",
    "tool: an exit dragged a step, 1,000 paths",
    "tool: an arrow turned a step, 1,000 paths",
    "needle point: moved, opened 1,000 paths",
    "needle point: moved, 2,000,000 points",
    "manual stitches: convert, 1,000 paths",
    "manual stitches: restore, 1,000 paths",
    "manual stitches: convert, 22,000 paths",
    "manual stitches: restore, 22,000 paths",
    "stitch commands: move one of 2,000,000",
    "stitch commands: page 1,000 of 2,000,000",
    "stitch commands: select 100,000 points",
    "stitch commands: move alone, 2,000,000",
    "stitch commands: move, many-run paths",
    "stitch commands: delete, many-run paths",
    "manual stitches: convert 22,000, apply",
    "object tab: read of 22,000 converted",
    "needle points: index of 2,000,000 built",
    "needle points: 95th percentile, 2,000,000",
    "needle points: slowest press, 2,000,000",
    "needle points: a marquee over 2,000,000",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`], all estimates. The stitch preview's
/// rows (from the drawing's on) are about 2.5 times what they measured on 2026-10-24 (release,
/// 12-core Apple Silicon, load about 2, median of five, twice), so that a regression of that size
/// shows; the cancel's is 5 ms, above the noise of a thread's wake-up. The planning rows keep the
/// budgets they were given with the planning service. The settings rows: an object's setting
/// plans again its own units (about 2.5 times the 2.4 ms it measured, so that planning every fill
/// again, about 70 ms, fails it), a fabric change the fills of the document (the cold plan's), and
/// reading the settings of 1,000 objects plans nothing (half the kept replies' 50 ms). The Object
/// tab's rows run on the interface thread on a new snapshot (each frame of a drag or a scrub): its
/// re-read of 1,000 selected objects within a third of a frame (6 ms of 16.7; it measured 2.7 ms on
/// 2026-10-25, the same machine, load about 2.5, twice), its footer within 1 ms whatever the
/// document's size (it reads two of the document's fields, under 0.01 ms; 1 ms is the least the
/// report's whole-millisecond budget column shows). The points rows are about 2.5 times what they
/// measured on 2026-10-26 (release, the same machine, load about 25, twice): the rings toward an
/// exit 475 and 507 ms (the median of three; they plan their parts with and without it, the plain
/// row 467 and 469 ms beside them), an exit or an entry moved 2.6 to 2.8 ms (the median of five;
/// it plans again its own unit).
const BUDGETS: [f64; 79] = [
    400.0,
    100.0,
    100.0,
    100.0,
    100.0,
    50.0,
    50.0,
    300.0,
    600.0,
    300.0,
    300.0,
    600.0,
    400.0,
    1000.0,
    100.0,
    100.0,
    100.0,
    400.0,
    4000.0,
    50.0,
    20.0,
    13.0,
    7.0,
    32.0,
    95.0,
    20.0,
    30.0,
    7.0,
    5.0,
    2.5,
    13.0,
    60.0,
    55.0,
    75.0,
    280.0,
    20.0,
    20.0,
    5.0,
    6.0,
    25.0,
    6.0,
    400.0,
    25.0,
    6.0,
    1.0,
    RINGS_TOWARD_EXIT_BUDGET,
    POINT_MOVED_BUDGET,
    POINT_MOVED_BUDGET,
    NEAREST_BUDGETS[0],
    NEAREST_BUDGETS[1],
    PICK_BUDGET,
    CONTOURS_TOWARD_EXIT_BUDGET,
    LATTICE_TOWARD_EXIT_BUDGET,
    RINGS_POINT_MOVED_BUDGET,
    PICK_BUDGET,
    PICK_SLOWEST_BUDGET,
    TOOL_BUDGETS[0],
    TOOL_BUDGETS[1],
    TOOL_BUDGETS[2],
    TOOL_BUDGETS[3],
    TOOL_BUDGETS[4],
    NEEDLE_MOVED_BUDGETS[0],
    NEEDLE_MOVED_BUDGETS[1],
    MANUAL_BUDGETS[0],
    MANUAL_BUDGETS[1],
    MANUAL_BUDGETS[2],
    MANUAL_BUDGETS[3],
    STITCH_COMMAND_BUDGETS[0],
    STITCH_COMMAND_BUDGETS[1],
    STITCH_COMMAND_BUDGETS[2],
    EDIT_ALONE_BUDGETS[0],
    EDIT_ALONE_BUDGETS[1],
    EDIT_ALONE_BUDGETS[2],
    CONVERT_PARTS_BUDGETS[0],
    CONVERT_PARTS_BUDGETS[1],
    NEEDLE_INDEX_BUDGETS[0],
    NEEDLE_INDEX_BUDGETS[1],
    NEEDLE_INDEX_BUDGETS[2],
    NEEDLE_INDEX_BUDGETS[3],
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
    ms.push(draw(&stitch_picture_of(100_000, Tiles::Grid45), FIT, None));
    ms.push(draw(&stitch_picture_of(100_000, Tiles::Satin), FIT, None));
    ms.push(corpus_frame()?.unwrap_or(f64::NAN));
    ms.push(emphasis(&stitch_picture(500_000)));
    let mut largest = None;
    for paths in SCENE_PATHS {
        let doc = synthetic(paths)?;
        ms.push(scene_build(&doc)?);
        largest = Some(doc);
    }
    let largest = largest.ok_or("embroidery: no scene document")?;
    ms.extend(replies(&largest)?);
    ms.push(nudge_to_frame(&doc, false)?);
    ms.push(nudge_to_frame(&doc, true)?);
    ms.push(cancel_latency(&one_fill(rings(20), FillRule::EvenOdd))?);
    ms.push(reorder(&doc)?);
    ms.push(drag_half(&largest)?);
    ms.extend(settings(&doc)?);
    ms.extend(object_tab(&doc, &largest)?);
    let rings_toward = rings_toward_an_exit()?;
    ms.push(cold(&rings_toward)?);
    ms.extend(points_moved(&doc)?);
    ms.push(nearest(&doc, RUNS)?);
    ms.push(nearest(&largest, 1)?);
    let [median, p95, slowest] = pick(&largest)?;
    ms.push(median);
    ms.push(cold(&contours_toward_an_exit()?)?);
    ms.push(cold(&lattice_toward_an_exit()?)?);
    ms.push(point_moved(&rings_toward, "entry")?);
    ms.extend([p95, slowest]);
    ms.extend(tool(&doc, &largest)?);
    ms.extend(needle_point_moved(&doc, &largest)?);
    ms.extend(convert_and_restore(&doc, RUNS)?);
    ms.extend(convert_and_restore(&largest, 1)?);
    let literal = literal_runs(&largest)?;
    let [moved, paged, selected, alone] = stitch_commands(&literal)?;
    ms.extend([moved, paged, selected, alone]);
    ms.extend(many_runs(&literal)?);
    ms.extend(convert_parts(&largest)?);
    ms.extend(needle_index(&literal)?);
    // The rings toward an exit plan their parts with and without it: within a share of the plain
    // rings' time in the same run, whatever the machine's load, and the absolute budget.
    let mut budgets = BUDGETS;
    let at = |name: &str| ROWS.iter().position(|r| *r == name);
    if let (Some(plain), Some(aimed)) = (at("embroidery: 20 rings in one fill, cold"), at("embroidery: 20 rings toward an exit, cold"))
        && let (Some(plain_ms), Some(budget)) = (ms.get(plain).copied(), budgets.get_mut(aimed))
    {
        *budget = budget.min(RINGS_TOWARD_EXIT_RATIO * plain_ms);
    }
    // A row with nothing to measure (no corpus) is left out.
    Ok(ROWS.iter().zip(ms).zip(budgets).filter(|((_, ms), _)| !ms.is_nan()).map(|((name, ms), budget)| (*name, ms, budget)).collect())
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
    if rows.len() < ROWS.len() {
        outln!("  (the corpus PES row is left out: no corpus; `cargo xtask corpus --embroidery` fetches it)");
    }
    outln!("  {:<42} {:>10} {:>10}", "", "measured", "budget");
    let mut over = 0;
    for (name, ms, budget) in rows {
        over += usize::from(ms > budget);
        outln!("{}", row(name, ms, budget, ms <= budget));
    }
    if over == 0 { Ok(()) } else { Err(format!("{over} embroidery budget(s) exceeded")) }
}

/// Milliseconds to build the stitch scene of `doc`'s plan (planned once before), the median of
/// [`DRAW_RUNS`].
fn scene_build(doc: &Document) -> Result<f64, String> {
    let mut s = fresh(doc);
    let plan = s.embroidery_plan(&vectorcraft_engine::EmbroideryOptions::default()).map_err(err)?;
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        let t = Instant::now();
        drop(vectorcraft_engine::cmd::embroidery::scene::scene(&plan));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(middle(samples))
}

/// Milliseconds from moving one object 0.5 mm to the stitch preview's frame of the design as it is
/// then: the plan (its unchanged units kept), its scene and a frame at fit, the median of
/// [`DRAW_RUNS`]. With `shadow`, another object has a drop shadow (its appearance is expanded on a
/// copy of the document at every plan).
fn nudge_to_frame(doc: &Document, shadow: bool) -> Result<f64, String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut s = fresh(doc);
    let order = s.execute("embroidery.sequence", &json!({})).map_err(err)?;
    let id = |k: usize| order["blocks"].get(k).and_then(|b| b["units"].get(0)).and_then(|u| u["id"].as_u64());
    let Some(moved) = id(0) else { return Err("embroidery: the document has nothing to move".into()) };
    if shadow {
        let other = id(1).ok_or("embroidery: the synthetic document has too few objects")?;
        s.execute("effect.apply", &json!({"effect": "stylize.dropShadow", "ids": [other]})).map_err(err)?;
    }
    s.embroidery_plan(&opts).map_err(err)?;
    let mut r = LineRenderer::new();
    let (w, h) = CANVAS;
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for k in 0..DRAW_RUNS {
        let dx = if k % 2 == 0 { NUDGE_MM } else { -NUDGE_MM } * PT_PER_MM;
        s.execute("object.move", &json!({"ids": [moved], "dx": dx, "dy": 0})).map_err(err)?;
        let t = Instant::now();
        let planned = s.embroidery_planner().and_then(|p| p.planned(&opts)).map_err(err)?;
        let scene = planned.scene();
        let (style, view) = framed(scene.picture(), FIT);
        let style = LineStyle { colors: scene.thread_colors(), ..style };
        drop(r.render(scene.picture(), w, h, view, &style));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(middle(samples))
}

/// Milliseconds from moving the object sewn first to the end of the sewing order (and back, in
/// turn) with the Sewing order list's own rule (`embroidery.sewLast`, `embroidery.sewFirst`) to the
/// plan of the document then, the median of [`DRAW_RUNS`]: what a drop in the list costs before its
/// stitches can show.
fn reorder(doc: &Document) -> Result<f64, String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut s = fresh(doc);
    let order = s.execute("embroidery.sequence", &json!({"limit": 1})).map_err(err)?;
    let moved = order["blocks"].get(0).and_then(|b| b["units"].get(0)).and_then(|u| u["id"].as_u64());
    let Some(moved) = moved else { return Err("embroidery: the document has nothing to move".into()) };
    s.embroidery_plan(&opts).map_err(err)?;
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for k in 0..DRAW_RUNS {
        let command = if k % 2 == 0 { "embroidery.sewLast" } else { "embroidery.sewFirst" };
        let t = Instant::now();
        s.execute(command, &json!({"ids": [moved]})).map_err(err)?;
        s.embroidery_plan(&opts).map_err(err)?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(middle(samples))
}

/// The points rows' budgets, milliseconds (estimates, [`BUDGETS`]): the rings toward an exit, and a
/// point moved.
const RINGS_TOWARD_EXIT_BUDGET: f64 = 1200.0;
const POINT_MOVED_BUDGET: f64 = 7.0;
/// The most the rings toward an exit may take beside the plain rings of the same run: planning
/// their parts twice (with the exit and without) costs a fraction of their narrow-part search, which
/// is done once; doing that search twice would pass an absolute budget under load, not this (an
/// estimate: they measured 1.02 to 1.08 times on 2026-10-26).
const RINGS_TOWARD_EXIT_RATIO: f64 = 1.25;
/// The running line of traced contours toward an exit: its contours, and the points of each (a
/// traced outline's), past the reordering's work bound (the subpaths squared times their points).
const CONTOURS: usize = 64;
const CONTOUR_POINTS: usize = 100;
/// The contours' radius and the grid they lie on, millimetres.
const CONTOUR_RADIUS_MM: f64 = 3.0;
const CONTOUR_GRID_MM: f64 = 10.0;
/// The budgets of the line of contours and of the lattice toward an exit, cold, milliseconds: the
/// contours about four times the 2.5 ms they measured on 2026-10-27 (release, the same machine,
/// load about 8), where weighing each of them as the last, past the reordering's bound, would take
/// tens of milliseconds; the lattice about 2.5 times its 96 ms (76 ms with no exit beside it).
const CONTOURS_TOWARD_EXIT_BUDGET: f64 = 10.0;
const LATTICE_TOWARD_EXIT_BUDGET: f64 = 250.0;
/// The budget of the 20 rings' entry moved a nudge and planned again, milliseconds: about 2.5 times
/// the 19 ms it measured on 2026-10-27, where planning the fill afresh takes 480 ms: its narrow
/// parts and its rest are kept, its stitches planned again.
const RINGS_POINT_MOVED_BUDGET: f64 = 50.0;
/// Nearest Connection's budgets on 1,000 and on 22,000 paths, milliseconds: about 2.5 times what they
/// measured on 2026-10-27 (release, the same machine, load 2 to 3, twice: 32 ms and 1.8 s; 120 ms
/// and 6.1 s on 2026-10-26, before the review's fixes). Every part it joins gets points, so the plan
/// of its result plans nearly every unit afresh: on the largest design, about a cold plan's time.
const NEAREST_BUDGETS: [f64; 2] = [80.0, 4500.0];

/// The fill angle the settings row gives the object sewn first, degrees, and the step between
/// runs: each run an angle not planned before, so that its units are planned again, not found
/// kept.
const ANGLE_FROM: f64 = 30.0;
const ANGLE_STEP: f64 = 1.0;

/// The fabric's tatami pull compensation the settings row sets, millimetres, and the step between
/// runs: each run a fabric not planned before.
const PULL_FROM: f64 = 0.21;
const PULL_STEP: f64 = 0.01;

/// Where the rings row's exit lies, millimetres: on the outer edge of the 20 rings' outermost, east
/// of their centre (150 mm, 150 mm), 83 mm out (5 mm, then 19 rings and gaps of 4 mm, then its 2 mm).
const RINGS_EXIT_MM: (f64, f64) = (233.0, 150.0);

/// The 20 rings ([`rings`]) with an exit point on their outermost ring ([`RINGS_EXIT_MM`]).
fn rings_toward_an_exit() -> Result<Document, String> {
    let mut s = fresh(&one_fill(rings(20), FillRule::EvenOdd));
    let id = s.doc().map_err(err)?.doc.layers.first().and_then(|l| l.children()).and_then(|c| c.first()).map(|n| n.id.0);
    let id = id.ok_or("embroidery: the rings document holds no fill")?;
    let exit = [RINGS_EXIT_MM.0 * PT_PER_MM, RINGS_EXIT_MM.1 * PT_PER_MM];
    s.execute("embroidery.setEntryExit", &json!({"id": id, "part": "fill", "exit": exit})).map_err(err)?;
    Ok((*s.doc().map_err(err)?.doc).clone())
}

/// The points rows on `doc`, milliseconds ([`point_moved`]): the exit point of the fill sewn first,
/// then its entry.
fn points_moved(doc: &Document) -> Result<[f64; 2], String> {
    Ok([point_moved(doc, "exit")?, point_moved(doc, "entry")?])
}

/// Milliseconds to move the `key` point ("entry" or "exit") of `doc`'s fill sewn first, set where
/// its stitches start and moved a nudge ([`NUDGE_MM`]) at a time, the design planned again after
/// each move (that object's unit planned afresh, the rest kept), the median of [`DRAW_RUNS`].
fn point_moved(doc: &Document, key: &str) -> Result<f64, String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut s = fresh(doc);
    let order = s.execute("embroidery.sequence", &json!({})).map_err(err)?;
    let units = order["blocks"].as_array().into_iter().flatten().filter_map(|b| b["units"].as_array()).flatten();
    let first =
        units.filter(|u| u["part"] == json!("fill")).find_map(|u| Some((u["id"].as_u64()?, u["entry"][0].as_f64()?, u["entry"][1].as_f64()?)));
    let Some((id, x, y)) = first else { return Err("embroidery: the document has no fill to set points on".into()) };
    s.embroidery_plan(&opts).map_err(err)?;
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for k in 0..=DRAW_RUNS {
        let mut params = json!({"id": id, "part": "fill"});
        // Read positions are millimetres; the command takes document points.
        params[key] = json!([(x + NUDGE_MM * k as f64) * PT_PER_MM, y * PT_PER_MM]);
        let t = Instant::now();
        s.execute("embroidery.setEntryExit", &params).map_err(err)?;
        s.embroidery_plan(&opts).map_err(err)?;
        // The first sets the point; the rest move it.
        if k > 0 {
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    Ok(middle(samples))
}

/// The needle-point rows' budgets, milliseconds (estimates): about 2.5 times what they measured
/// ([`NEEDLE_MOVED_MS`]): one needle point moved and the design planned again, on the opened
/// 1,000-path document and on 2 million literal needle points. A literal run is sewn as its points
/// are, never planned, but every run is walked and sequenced again at each plan.
const NEEDLE_MOVED_BUDGETS: [f64; 2] = [NEEDLE_MOVED_MS[0] * 2.5, NEEDLE_MOVED_MS[1] * 2.5];
/// What the needle-point rows measured on 2026-10-31 (release, 12-core Apple Silicon, load 2.6 to
/// 3.2, the median of five, twice), milliseconds (see [`NEEDLE_MOVED_BUDGETS`]).
const NEEDLE_MOVED_MS: [f64; 2] = [1.9, 50.0];
/// The manual-stitches rows' budgets, milliseconds (estimates): about 2.5 times what they measured
/// ([`MANUAL_MS`]): converting every object of the 1,000-path document and restoring them all, then
/// of the 22,000-path document. A convert plans the design afresh, as the export does: over a
/// second on 22,000 paths, so the Object tab converts off the interface thread.
const MANUAL_BUDGETS: [f64; 4] = [MANUAL_MS[0] * 2.5, MANUAL_MS[1] * 2.5, MANUAL_MS[2] * 2.5, MANUAL_MS[3] * 2.5];
/// What the manual-stitches rows measured on 2026-10-31 (release, 12-core Apple Silicon, load 2.6
/// to 3.2, twice; the median of three on 1,000 paths, one run on 22,000), milliseconds (see
/// [`MANUAL_BUDGETS`]).
const MANUAL_MS: [f64; 4] = [78.0, 5.2, 1760.0, 120.0];

/// Milliseconds to convert every object of `doc` to manual stitches (Select All, then the command)
/// and to restore them all (Select All, then Restore Object), the medians of `runs`, each on a
/// session that has never planned `doc`.
fn convert_and_restore(doc: &Document, runs: usize) -> Result<[f64; 2], String> {
    let (mut convert, mut restore) = (Vec::with_capacity(runs), Vec::with_capacity(runs));
    for _ in 0..runs {
        let mut s = fresh(doc);
        s.execute("select.all", &json!({})).map_err(err)?;
        let (ms, done) = timed(&mut s, "embroidery.convertToManual", &json!({}))?;
        if done["convertedCount"].as_u64().unwrap_or(0) == 0 {
            return Err("embroidery: nothing was converted to manual stitches".into());
        }
        convert.push(ms);
        s.execute("select.all", &json!({})).map_err(err)?;
        restore.push(timed(&mut s, "embroidery.restoreObject", &json!({}))?.0);
    }
    Ok([middle(convert), middle(restore)])
}

/// The stitch commands' rows' budgets, milliseconds (estimates): about 2.5 times what they measured
/// ([`STITCH_COMMAND_MS`]): one needle point moved by the command and the design planned again, a
/// page of the stitch list, 100,000 needle points selected, all on 2 million literal needle points.
const STITCH_COMMAND_BUDGETS: [f64; 3] = [STITCH_COMMAND_MS[0] * 2.5, STITCH_COMMAND_MS[1] * 2.5, STITCH_COMMAND_MS[2] * 2.5];
/// What the stitch commands' rows measured on 2026-10-31 (release, 12-core Apple Silicon, load 2.6
/// to 3.2, the median of five, twice), milliseconds (see [`STITCH_COMMAND_BUDGETS`]): moving one
/// needle point costs what Direct Selection's move does (`NEEDLE_MOVED_MS`), the plan again
/// dominating.
const STITCH_COMMAND_MS: [f64; 3] = [52.5, 37.0, 43.3];
/// The needle points the selection row selects by their records.
const SELECTED_POINTS: usize = 100_000;
/// The records the list row lists a page of.
const LIST_PAGE: usize = 1000;

/// The stitch commands' rows on `doc` (literal runs, [`literal_runs`]): one needle point of its
/// largest path moved a nudge by `embroidery.moveNeedlePoints`, back and forth, and the design
/// planned again after each; a page of the stitch list from its middle, the plan kept; the first
/// [`SELECTED_POINTS`] needle points selected by their records; and the move's command alone, the
/// plan aside (what a copy of the whole design in the command would show, the plan's tens of
/// milliseconds hiding it in the first row). Medians of [`DRAW_RUNS`].
fn stitch_commands(doc: &Document) -> Result<[f64; 4], String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut largest: Option<(u64, usize)> = None;
    for l in &doc.layers {
        l.walk(&mut |n| {
            if let Some(path) = n.path_data() {
                let anchors: usize = path.subpaths.iter().map(|sp| sp.anchors.len()).sum();
                if largest.is_none_or(|(_, most)| anchors > most) {
                    largest = Some((n.id.0, anchors));
                }
            }
        });
    }
    let Some((id, anchors)) = largest else { return Err("embroidery: the document has no run to edit".into()) };
    let mut s = fresh(doc);
    s.embroidery_plan(&opts).map_err(err)?;
    let (mut moved, mut paged, mut selected, mut alone) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let records = s.execute("embroidery.stitches", &json!({"limit": 0})).map_err(err)?["recordCount"].as_u64().unwrap_or(0) as usize;
    for k in 0..DRAW_RUNS {
        let dx = if k % 2 == 0 { NUDGE_MM } else { -NUDGE_MM };
        let t = Instant::now();
        s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": anchors / 2}], "dx": dx})).map_err(err)?;
        alone.push(t.elapsed().as_secs_f64() * 1000.0);
        s.embroidery_plan(&opts).map_err(err)?;
        moved.push(t.elapsed().as_secs_f64() * 1000.0);
        paged.push(timed(&mut s, "embroidery.stitches", &json!({"offset": records / 2, "limit": LIST_PAGE}))?.0);
        selected.push(timed(&mut s, "embroidery.selectNeedlePoints", &json!({"records": [1, SELECTED_POINTS]}))?.0);
    }
    Ok([middle(moved), middle(paged), middle(selected), middle(alone)])
}

/// The rows of the commands alone on paths of many runs (budgets, milliseconds, estimates: about
/// 2.5 times what they measured, [`EDIT_ALONE_MS`]): one needle point moved in 2 million literal
/// needle points, the plan aside; one moved, and one deleted, in the largest of `doc`'s blocks made
/// one path of all its runs (as a converted unit holds its runs), each command alone.
const EDIT_ALONE_BUDGETS: [f64; 3] = [EDIT_ALONE_MS[0] * 2.5, EDIT_ALONE_MS[1] * 2.5, EDIT_ALONE_MS[2] * 2.5];
/// What those rows measured on 2026-10-31 (release, 12-core Apple Silicon, load 2.6 to 3.2, the
/// median of five, twice), milliseconds (see [`EDIT_ALONE_BUDGETS`]; recorded in
/// `docs/embroidery.md`).
const EDIT_ALONE_MS: [f64; 3] = [4.8, 1.9, 0.7];

/// The commands alone on `doc`'s literal runs merged into paths of many runs ([`many_run_paths`]):
/// one needle point of the largest path moved a nudge, back and forth, and one deleted (then put
/// back by Undo, untimed), the medians of [`DRAW_RUNS`].
fn many_runs(doc: &Document) -> Result<[f64; 2], String> {
    let merged = many_run_paths(doc)?;
    let mut largest: Option<(u64, usize)> = None;
    for l in &merged.layers {
        l.walk(&mut |n| {
            if let Some(path) = n.path_data() {
                let anchors: usize = path.subpaths.iter().map(|sp| sp.anchors.len()).sum();
                if largest.is_none_or(|(_, most)| anchors > most) {
                    largest = Some((n.id.0, anchors));
                }
            }
        });
    }
    let Some((id, anchors)) = largest else { return Err("embroidery: the document has no path of many runs".into()) };
    let mut s = fresh(&merged);
    let at = json!([{"id": id, "index": anchors / 4}]);
    let (mut moved, mut deleted) = (Vec::with_capacity(DRAW_RUNS), Vec::with_capacity(DRAW_RUNS));
    for k in 0..DRAW_RUNS {
        let dx = if k % 2 == 0 { NUDGE_MM } else { -NUDGE_MM };
        moved.push(timed(&mut s, "embroidery.moveNeedlePoints", &json!({"needlePoints": at, "dx": dx}))?.0);
        deleted.push(timed(&mut s, "embroidery.deleteNeedlePoints", &json!({"needlePoints": at}))?.0);
        s.execute("edit.undo", &json!({})).map_err(err)?;
    }
    Ok([middle(moved), middle(deleted)])
}

/// `doc` (literal runs, one path a run) with each group's runs made one path of as many subpaths,
/// as a converted object's unit holds its runs: the needle jumping between them.
fn many_run_paths(doc: &Document) -> Result<Document, String> {
    use vectorcraft_doc::embroidery::{Embroidery, StitchRun};
    let mut out = doc.clone();
    let groups: Vec<std::sync::Arc<Node>> = out.layers.first().and_then(|l| l.children()).cloned().unwrap_or_default();
    for g in groups {
        let runs = g.children().cloned().unwrap_or_default();
        let Some(first) = runs.first() else { continue };
        let subpaths: Vec<vectorcraft_geom::SubPath> = runs.iter().filter_map(|r| r.path_data()).flat_map(|p| p.subpaths.iter().cloned()).collect();
        let mut node = Node::path(out.alloc_id(), PathData::new(subpaths), Appearance::default());
        node.appearance = first.appearance.clone();
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(StitchRun::default()), ..Embroidery::default() }));
        if let Some(slot) = out.node_mut(g.id) {
            *slot = Node::group(g.id, vec![std::sync::Arc::new(node)]);
        }
    }
    Ok(out)
}

/// The convert rows' budgets, milliseconds (estimates: about 2.5 times what they measured,
/// [`CONVERT_PARTS_MS`]): converting every object of the 22,000-path document, the part on the
/// interface thread alone (the job prepared and its plan applied, the plan itself aside, which the
/// interface runs on a thread of its own); then the Object tab's read of all of them selected, after
/// each nudge of one.
const CONVERT_PARTS_BUDGETS: [f64; 2] = [CONVERT_PARTS_MS[0] * 2.5, CONVERT_PARTS_MS[1] * 2.5];
/// What those rows measured on 2026-10-31 (release, 12-core Apple Silicon, load 2.6 to 3.2, twice;
/// the convert once, the read the median of five), milliseconds (see [`CONVERT_PARTS_BUDGETS`];
/// recorded in `docs/embroidery.md`): the read takes about 1 µs a converted object, where an object
/// of art takes about 3 µs (the Object tab's re-read of 1,000, 3.1 to 3.6 ms in the same runs).
const CONVERT_PARTS_MS: [f64; 2] = [167.0, 22.0];

/// The convert's interface-thread part on `doc` (Select All; the job prepared, then its plan
/// applied, the plan between them untimed), once; then the Object tab's read of every converted
/// object selected after a nudge of one, the median of [`DRAW_RUNS`].
fn convert_parts(doc: &Document) -> Result<[f64; 2], String> {
    let mut s = fresh(doc);
    s.execute("select.all", &json!({})).map_err(err)?;
    let t = Instant::now();
    let job = s.convert_to_manual_job(&json!({})).map_err(err)?;
    let prepared = t.elapsed().as_secs_f64() * 1000.0;
    let planned = job.plan().map_err(err)?;
    let t = Instant::now();
    let done = s.apply_convert_to_manual(planned).map_err(err)?;
    let parts = prepared + t.elapsed().as_secs_f64() * 1000.0;
    if done["convertedCount"].as_u64().unwrap_or(0) == 0 {
        return Err("embroidery: nothing was converted to manual stitches".into());
    }
    s.execute("select.all", &json!({})).map_err(err)?;
    let ids = s.doc().map_err(err)?.selection.objects.clone();
    let first = *ids.first().ok_or("embroidery: nothing converted is selected")?;
    let mut reads = Vec::with_capacity(DRAW_RUNS);
    for k in 0..DRAW_RUNS {
        let dx = if k % 2 == 0 { NUDGE_MM } else { -NUDGE_MM } * PT_PER_MM;
        s.execute("object.move", &json!({"ids": [first.0], "dx": dx, "dy": 0})).map_err(err)?;
        let t = Instant::now();
        std::hint::black_box(s.embroidery_merged(&ids).map_err(err)?);
        reads.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok([parts, middle(reads)])
}

/// The needle-point index's rows' budgets, milliseconds (estimates): the index of 2 million needle
/// points built (once, when stitch editing starts on them) and a marquee over half the design
/// selecting its needle points, about 2.5 times what they measured ([`NEEDLE_INDEX_MS`]); the
/// nearest needle point to a press, its 95th percentile within a quarter of the 1 ms a press on the
/// stitches may take (a hover asks for it twice a frame), its slowest of [`NEEDLE_QUERIES`] presses
/// anywhere over the design (misses included) within that 1 ms.
const NEEDLE_INDEX_BUDGETS: [f64; 4] = [NEEDLE_INDEX_MS[0] * 2.5, NEEDLE_PRESS_P95_MS, NEEDLE_PRESS_MS, NEEDLE_INDEX_MS[3] * 2.5];
/// A press's nearest needle point: its 95th percentile's budget and its slowest's, milliseconds
/// (see [`NEEDLE_INDEX_BUDGETS`]).
const NEEDLE_PRESS_P95_MS: f64 = 0.25;
const NEEDLE_PRESS_MS: f64 = 1.0;
/// What the needle-point index's rows measured on 2026-10-27 (release, 12-core Apple Silicon, load
/// about 4 to 8), milliseconds: built 16.3, the nearest needle point 0.04 at the 95th percentile and
/// 0.15 at worst, the marquee 156 (it writes a million needle points into the selection).
const NEEDLE_INDEX_MS: [f64; 4] = [16.3, 0.04, 0.15, 156.0];
/// How many presses the nearest needle point's rows time, at points spread over the design's box
/// and as far again round it (misses included).
const NEEDLE_QUERIES: usize = 1000;
/// How far a press reaches, points: the selection tolerance's default at 100 %.
const NEEDLE_REACH_PT: f64 = 3.0;

/// The needle-point index's rows on `doc` (literal runs, [`literal_runs`]): the index of every path
/// of stitches in its layers built; the nearest needle point to [`NEEDLE_QUERIES`] presses (the
/// 95th percentile and the slowest); `embroidery.selectNeedlePoints {rect}` over the left half of
/// the design, its layer isolated, the median of [`DRAW_RUNS`].
fn needle_index(doc: &Document) -> Result<[f64; 4], String> {
    use vectorcraft_engine::cmd::embroidery::needle_index::NeedleIndex;
    let roots: Vec<_> = doc.layers.iter().map(|l| l.id).collect();
    let mut built = Vec::with_capacity(DRAW_RUNS);
    let mut index = None;
    for _ in 0..DRAW_RUNS {
        let t = Instant::now();
        index = NeedleIndex::of(doc, &roots, None);
        built.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let index = index.ok_or("needle points: the design holds no path of stitches")?;
    let b = index.scope().bounds.ok_or("needle points: none")?;
    // Spread over the design's box and as far again round it: a fixed sequence, the same each run.
    let mut times = Vec::with_capacity(NEEDLE_QUERIES);
    let mut hits = 0usize;
    for k in 0..NEEDLE_QUERIES {
        let (u, v) = ((k * 7919 % NEEDLE_QUERIES) as f64 / NEEDLE_QUERIES as f64, (k * 104_729 % NEEDLE_QUERIES) as f64 / NEEDLE_QUERIES as f64);
        let at = Point::new(b.x0 - b.width() / 2.0 + 2.0 * b.width() * u, b.y0 - b.height() / 2.0 + 2.0 * b.height() * v);
        let t = Instant::now();
        hits += usize::from(index.needle_at(at, NEEDLE_REACH_PT).is_some());
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    if hits == 0 || hits == NEEDLE_QUERIES {
        return Err(format!("needle points: {hits} of {NEEDLE_QUERIES} presses found one; the row needs hits and misses"));
    }
    times.sort_by(f64::total_cmp);
    let p95 = times.get(NEEDLE_QUERIES * 95 / 100).copied().unwrap_or(f64::NAN);
    let slowest = times.last().copied().unwrap_or(f64::NAN);
    let mut s = fresh(doc);
    let layer = roots.first().ok_or("needle points: no layer")?;
    s.execute("object.isolate", &json!({"id": layer.0})).map_err(err)?;
    let mm = |v: f64| v / PT_PER_MM;
    let rect = json!([mm(b.x0), mm(b.y0), mm(b.center().x), mm(b.y1)]);
    let mut marquee = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        let (ms, done) = timed(&mut s, "embroidery.selectNeedlePoints", &json!({ "rect": rect }))?;
        if done["selectedCount"].as_u64().unwrap_or(0) == 0 {
            return Err(format!("needle points: the marquee selected nothing: {done}"));
        }
        marquee.push(ms);
    }
    Ok([middle(built), p95, slowest, middle(marquee)])
}

/// The width of a literal run's line, points: a thread's 0.4 mm, as opening a machine file draws it.
const THREAD_WIDTH_PT: f64 = 0.4 * PT_PER_MM;
/// The page of the literal design when the document has no artboard: the synthetic documents'.
const SYNTHETIC_PAGE: (f64, f64) = (1600.0, 1200.0);

/// The needle-point rows ([`needle_moved`]): the 1,000-path document `doc` written as DST and
/// opened, then `largest` (the 22,000-path document) built into literal runs ([`literal_runs`]).
fn needle_point_moved(doc: &Document, largest: &Document) -> Result<[f64; 2], String> {
    let mut s = fresh(doc);
    let dst = s.execute("document.export", &json!({"format": "dst"})).map_err(err)?;
    let mut opened = Session::new();
    opened.execute("document.open", &json!({"name": "perf.dst", "dataBase64": dst["dataBase64"]})).map_err(err)?;
    let opened = (*opened.doc().map_err(err)?.doc).clone();
    Ok([needle_moved(&opened)?, needle_moved(&literal_runs(largest)?)?])
}

/// `doc`'s plan as literal stitch runs, as opening a machine file makes them: a group a block, a path
/// a run (the needle-down stitches between two lifts), each stroked in its block's thread. Built here
/// rather than opened, so that it can hold as many needle points as a design may.
fn literal_runs(doc: &Document) -> Result<Document, String> {
    use vectorcraft_doc::embroidery::{Embroidery, StitchRun};
    let mut s = fresh(doc);
    let plan = s.embroidery_plan(&vectorcraft_engine::EmbroideryOptions::default()).map_err(err)?;
    let page = doc.artboards.first().map_or(SYNTHETIC_PAGE, |a| (a.rect.width(), a.rect.height()));
    let mut out = Document::new(page.0, page.1);
    let layer = out.layers.first().map(|l| l.id);
    for block in &plan.blocks {
        let [r, g, b] = block.thread.art;
        let paint = Paint::solid(Color::rgb8(r, g, b));
        let mut runs: Vec<std::sync::Arc<Node>> = Vec::new();
        let mut line: Vec<Point> = Vec::new();
        let flush = |line: &mut Vec<Point>, out: &mut Document, runs: &mut Vec<std::sync::Arc<Node>>| {
            let Some(&first) = line.first() else { return };
            let mut bp = BezPath::new();
            bp.move_to(first);
            // A run of one needle point is a line from the point to itself, as a file opens it.
            for &p in line.iter().skip(1).chain((line.len() == 1).then_some(&first)) {
                bp.line_to(p);
            }
            let mut node = Node::path(out.alloc_id(), PathData::from_bezpath(&bp), Appearance::basic(Paint::None, paint.clone(), THREAD_WIDTH_PT));
            node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(StitchRun::default()), ..Embroidery::default() }));
            runs.push(std::sync::Arc::new(node));
            line.clear();
        };
        for st in &block.stitches {
            if st.command.needle_down() {
                line.push(Point::new(st.x * PT_PER_MM, st.y * PT_PER_MM));
            } else {
                flush(&mut line, &mut out, &mut runs);
            }
        }
        flush(&mut line, &mut out, &mut runs);
        let group = Node::group(out.alloc_id(), runs);
        out.insert(layer, usize::MAX, group).map_err(|e| format!("building the literal runs: {e}"))?;
    }
    Ok(out)
}

/// Milliseconds to move one needle point of `doc`'s largest run (its middle anchor, selected as
/// Direct Selection selects it) a nudge ([`NUDGE_MM`]) with `path.moveAnchors`, back and forth, the
/// design planned again after each move, the median of [`DRAW_RUNS`].
fn needle_moved(doc: &Document) -> Result<f64, String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut largest: Option<(u64, usize)> = None;
    for l in &doc.layers {
        l.walk(&mut |n| {
            if let Some(path) = n.path_data() {
                let anchors: usize = path.subpaths.iter().map(|sp| sp.anchors.len()).sum();
                if largest.is_none_or(|(_, most)| anchors > most) {
                    largest = Some((n.id.0, anchors));
                }
            }
        });
    }
    let Some((id, anchors)) = largest else { return Err("embroidery: the document has no run to edit".into()) };
    let mut s = fresh(doc);
    s.embroidery_plan(&opts).map_err(err)?;
    s.execute("select.anchors", &json!({"id": id, "anchors": [[0, anchors / 2]]})).map_err(err)?;
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for k in 0..DRAW_RUNS {
        let dx = if k % 2 == 0 { NUDGE_MM } else { -NUDGE_MM } * PT_PER_MM;
        let t = Instant::now();
        s.execute("path.moveAnchors", &json!({"dx": dx, "dy": 0})).map_err(err)?;
        s.embroidery_plan(&opts).map_err(err)?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(middle(samples))
}

/// A running line of [`CONTOURS`] closed contours of [`CONTOUR_POINTS`] points each, on a grid,
/// its exit on the last contour.
fn contours_toward_an_exit() -> Result<Document, String> {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 600, "artboards": 1})).map_err(err)?;
    s.execute("paint.setFill", &json!({"none": true})).map_err(err)?;
    s.execute("paint.setStroke", &json!({"color": "#2233cc"})).map_err(err)?;
    let side = (CONTOURS as f64).sqrt().ceil() as usize;
    let mut d = String::new();
    let mut last = (0.0, 0.0);
    for k in 0..CONTOURS {
        let centre = (CONTOUR_GRID_MM * (1.0 + (k % side) as f64), CONTOUR_GRID_MM * (1.0 + (k / side) as f64));
        for i in 0..CONTOUR_POINTS {
            let a = std::f64::consts::TAU * i as f64 / CONTOUR_POINTS as f64;
            let (x, y) = ((centre.0 + CONTOUR_RADIUS_MM * a.cos()) * PT_PER_MM, (centre.1 + CONTOUR_RADIUS_MM * a.sin()) * PT_PER_MM);
            d.push_str(&format!("{} {x:.3} {y:.3} ", if i == 0 { "M" } else { "L" }));
        }
        d.push_str("Z ");
        last = ((centre.0 + CONTOUR_RADIUS_MM) * PT_PER_MM, centre.1 * PT_PER_MM);
    }
    let id = s.execute("path.create", &json!({ "d": d })).map_err(err)?["id"].clone();
    s.execute("stroke.set", &json!({"weight": 1.0, "ids": [id]})).map_err(err)?;
    s.execute("embroidery.setEntryExit", &json!({"id": id, "part": "stroke", "exit": [last.0, last.1]})).map_err(err)?;
    Ok((*s.doc().map_err(err)?.doc).clone())
}

/// The lattice of 100 holes, a tatami fill of many regions, its exit on the middle of its right
/// side, taken onto the outline.
fn lattice_toward_an_exit() -> Result<Document, String> {
    let path = lattice(10);
    let b = kurbo::Shape::bounding_box(&path);
    let mut s = fresh(&one_fill(path, FillRule::EvenOdd));
    let id = s.doc().map_err(err)?.doc.layers.first().and_then(|l| l.children()).and_then(|c| c.first()).map(|n| n.id.0);
    let id = id.ok_or("embroidery: the lattice document holds no fill")?;
    let exit = [b.x1 * PT_PER_MM, (b.y0 + b.y1) / 2.0 * PT_PER_MM];
    s.execute("embroidery.setEntryExit", &json!({"id": id, "part": "fill", "exit": exit, "snap": true})).map_err(err)?;
    Ok((*s.doc().map_err(err)?.doc).clone())
}

/// The embroidery tool's rows' budgets, milliseconds (estimates): its handles found for 32 objects
/// and for Select All on 22,000 paths, each within a millisecond (the least the report's budget
/// column shows; they measured 0.02 and 0.03 ms on 2026-10-27, the first 32 objects alone taken);
/// a hover frame about 4.5 times its 0.44 ms; a drag step of an exit or of an arrow, planned again,
/// as a point moved ([`POINT_MOVED_BUDGET`]: they measured 2.6 and 2.7 ms).
const TOOL_BUDGETS: [f64; 5] = [1.0, 1.0, 2.0, POINT_MOVED_BUDGET, POINT_MOVED_BUDGET];

/// How many hover frames the hover row times, across the design.
const HOVERS: usize = 200;

/// How far the drags move before they are timed (past the tool's drag threshold), millimetres, and
/// the turn an arrow's first step makes, degrees.
const DRAG_START_MM: f64 = 5.0;
const TURN_START_DEGREES: f64 = 10.0;

/// The embroidery tool's rows, milliseconds, through the session as the canvas drives it: its
/// handles found for 32 objects of `doc` selected, and for Select All on `largest`, each the median
/// of [`DRAW_RUNS`] changes of the selection; a hover frame on `largest` (a pointer move, the
/// cursor and the handles, as a frame asks them), the median of [`HOVERS`]; an exit of `doc`
/// dragged a nudge ([`NUDGE_MM`]) at a time and a fill's rows' arrow turned a degree
/// ([`ANGLE_STEP`]) at a time, the design planned again after each step, the median of
/// [`DRAW_RUNS`].
fn tool(doc: &Document, largest: &Document) -> Result<[f64; 5], String> {
    use vectorcraft_engine::tools::{Mods, PointerEvent, PointerKind};
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let view = vectorcraft_engine::ViewInfo::default();
    let ids = |s: &Session| -> Vec<u64> {
        s.doc().map(|d| d.doc.layers.iter().filter_map(|l| l.children()).flatten().map(|n| n.id.0).collect()).unwrap_or_default()
    };
    let found = |s: &mut Session, selections: &[Value]| -> Result<f64, String> {
        let mut samples = Vec::with_capacity(DRAW_RUNS);
        for k in 0..DRAW_RUNS {
            s.execute("select.set", selections.get(k % selections.len()).unwrap_or(&json!({"ids": []}))).map_err(err)?;
            let t = Instant::now();
            std::hint::black_box(s.embroidery_handles(view));
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(middle(samples))
    };
    // 32 objects, two sets in turn (the handles are found again for each).
    let mut s = fresh(doc);
    s.embroidery_plan(&opts).map_err(err)?;
    s.select_tool("embroidery", view).map_err(err)?;
    let all = ids(&s);
    let sets: Vec<Value> = [0_usize, 1].iter().map(|k| json!({"ids": all.iter().skip(k * 32).take(32).collect::<Vec<_>>()})).collect();
    let thirty_two = found(&mut s, &sets)?;
    // Select All on the largest, and none, in turn.
    let mut big = fresh(largest);
    big.embroidery_plan(&opts).map_err(err)?;
    big.select_tool("embroidery", view).map_err(err)?;
    let big_ids = ids(&big);
    let every = json!({ "ids": big_ids });
    let select_all = found(&mut big, &[every.clone(), json!({"ids": big_ids.first().into_iter().collect::<Vec<_>>()})])?;
    // Hover frames across the largest, every object selected.
    big.execute("select.set", &every).map_err(err)?;
    let bounds = big
        .doc()
        .map_err(err)?
        .doc
        .layers
        .iter()
        .filter_map(|l| l.children())
        .flatten()
        .filter_map(|n| n.geometric_bounds())
        .reduce(|a, b| a.union(b));
    let bounds = bounds.ok_or("embroidery: the largest document has no art")?;
    let side = (HOVERS as f64).sqrt().ceil() as usize;
    let mut samples = Vec::with_capacity(HOVERS);
    for k in 0..HOVERS {
        let at = Point::new(
            bounds.x0 + bounds.width() * ((k % side) as f64 + 0.5) / side as f64,
            bounds.y0 + bounds.height() * ((k / side) as f64 + 0.5) / side as f64,
        );
        let t = Instant::now();
        big.pointer(&PointerEvent::new(PointerKind::Move, at.x, at.y), view).map_err(err)?;
        std::hint::black_box(big.cursor(at, Mods::default(), view));
        std::hint::black_box(big.embroidery_handles(view));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let hover = middle(samples);
    // A drag of the handle `kind` of the fill sewn first, by `step(k)` from where it was pressed.
    let drag = |kind: &str, step: &dyn Fn(Point, Option<Point>, usize) -> Point| -> Result<f64, String> {
        let mut s = fresh(doc);
        let order = s.execute("embroidery.sequence", &json!({})).map_err(err)?;
        let units = order["blocks"].as_array().into_iter().flatten().filter_map(|b| b["units"].as_array()).flatten();
        let first = units.filter(|u| u["part"] == json!("fill")).find_map(|u| u["id"].as_u64());
        let Some(id) = first else { return Err("embroidery: the document has no fill to drag the points of".into()) };
        s.execute("select.set", &json!({"ids": [id]})).map_err(err)?;
        s.embroidery_plan(&opts).map_err(err)?;
        s.select_tool("embroidery", view).map_err(err)?;
        let handle = s.embroidery_handles(view).0.into_iter().find(|h| h.id.kind.id() == kind);
        let Some(h) = handle else { return Err(format!("embroidery: the fill sewn first has no {kind} handle")) };
        let centre = h.arrow.map(|a| a.centre);
        s.pointer(&PointerEvent::new(PointerKind::Down, h.at.x, h.at.y), view).map_err(err)?;
        let mut samples = Vec::with_capacity(DRAW_RUNS);
        for k in 0..=DRAW_RUNS {
            let to = step(h.at, centre, k);
            let t = Instant::now();
            s.pointer(&PointerEvent::new(PointerKind::Drag, to.x, to.y), view).map_err(err)?;
            s.embroidery_plan(&opts).map_err(err)?;
            // The first begins the drag; the rest move it.
            if k > 0 {
                samples.push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        s.pointer(&PointerEvent::new(PointerKind::Up, h.at.x, h.at.y), view).map_err(err)?;
        Ok(middle(samples))
    };
    let exit = drag("exit", &|at, _, k| Point::new(at.x + (DRAG_START_MM + NUDGE_MM * k as f64) * PT_PER_MM, at.y))?;
    let arrow = drag("angle", &|at, centre, k| {
        let Some(c) = centre else { return at };
        let turn = (TURN_START_DEGREES + ANGLE_STEP * k as f64).to_radians();
        let (dx, dy) = (at.x - c.x, at.y - c.y);
        Point::new(c.x + dx * turn.cos() - dy * turn.sin(), c.y + dx * turn.sin() + dy * turn.cos())
    })?;
    Ok([thirty_two, select_all, hover, exit, arrow])
}

/// How many presses the picking rows time across the design, how many of them on empty fabric
/// beside it (a miss, which scans every line), and the tolerance each is given, document points
/// (the selection tolerance's default, 3 px at 100 %).
const PICKS: usize = 1000;
const PICKS_MISSED: usize = 20;
const PICK_TOL_PT: f64 = 3.0;
/// How far beside the design the misses lie, millimetres.
const MISS_BESIDE_MM: f64 = 10.0;
/// The budget of the slowest press, milliseconds: a miss scans every line (an estimate; the
/// slowest measured 0.06 ms on 2026-10-27, the 95th percentile 0.03 ms, both within
/// [`PICK_BUDGET`]'s millisecond).
const PICK_SLOWEST_BUDGET: f64 = 1.0;

/// The budget of a press on the stitches, milliseconds: well under the art's 2 ms hit test, which a
/// press with the selection tools makes (an estimate, to be set from what it measures).
const PICK_BUDGET: f64 = 1.0;

/// Milliseconds a press on the stitches takes to find the object on top (`StitchScene::pick`), on
/// `doc`'s scene, of [`PICKS`] presses spread over the design in a grid and [`PICKS_MISSED`] on
/// empty fabric beside it: the median, the 95th percentile and the slowest.
fn pick(doc: &Document) -> Result<[f64; 3], String> {
    let mut s = fresh(doc);
    let planned = s.embroidery_planner().map_err(err)?.planned(&vectorcraft_engine::EmbroideryOptions::default()).map_err(err)?;
    let scene = planned.scene();
    let Some(b) = scene.picture().bounds() else { return Err("embroidery: the picking document has no stitches".into()) };
    let side = (PICKS as f64).sqrt().ceil() as usize;
    let mut samples = Vec::with_capacity(PICKS);
    for k in 0..PICKS {
        let (i, j) = (k % side, k / side);
        let at = vectorcraft_geom::Point::new(b.x0 + b.width() * (i as f64 + 0.5) / side as f64, b.y0 + b.height() * (j as f64 + 0.5) / side as f64);
        let t = Instant::now();
        std::hint::black_box(scene.pick(at, PICK_TOL_PT));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    for k in 0..PICKS_MISSED {
        let at = vectorcraft_geom::Point::new(b.x1 + MISS_BESIDE_MM * PT_PER_MM, b.y0 + b.height() * (k as f64 + 0.5) / PICKS_MISSED as f64);
        let t = Instant::now();
        std::hint::black_box(scene.pick(at, PICK_TOL_PT));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let share = |q: f64| samples.get(((samples.len() as f64 * q).ceil() as usize).saturating_sub(1)).copied().unwrap_or(0.0);
    Ok([share(0.5), share(0.95), share(1.0)])
}

/// Milliseconds Nearest Connection takes on `doc` once its plan is kept (planned once before, in a
/// fresh session each run, so each starts from the same units kept), the median of `runs`: its
/// walk recording each unit's ends, the search, and the plan of its result (`dryRun`, which plans it
/// all the same).
fn nearest(doc: &Document, runs: usize) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs.max(1) {
        let mut s = fresh(doc);
        s.execute("embroidery.stats", &json!({})).map_err(err)?;
        samples.push(timed(&mut s, "embroidery.nearestConnection", &json!({"all": true, "dryRun": true}))?.0);
    }
    Ok(middle(samples))
}

/// The settings rows on `doc`, milliseconds, each the median of [`DRAW_RUNS`]: one object's fill
/// angle set and the design planned again (its other units kept); the fabric's tatami values
/// changed and the design planned again (its fills planned afresh); and `embroidery.info` of every
/// object (no plan).
fn settings(doc: &Document) -> Result<[f64; 3], String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut s = fresh(doc);
    // The first unit sewn as a fill: the angle row sets a fill's angle.
    let order = s.execute("embroidery.sequence", &json!({})).map_err(err)?;
    let units = order["blocks"].as_array().into_iter().flatten().filter_map(|b| b["units"].as_array()).flatten();
    let first = units.filter(|u| u["part"] == json!("fill")).find_map(|u| u["id"].as_u64());
    let Some(first) = first else { return Err("embroidery: the document has no fill to set".into()) };
    s.embroidery_plan(&opts).map_err(err)?;
    let timed_edits = |s: &mut Session, command: &str, params: &dyn Fn(usize) -> Value| -> Result<f64, String> {
        let mut samples = Vec::with_capacity(DRAW_RUNS);
        for k in 0..DRAW_RUNS {
            let t = Instant::now();
            s.execute(command, &params(k)).map_err(err)?;
            s.embroidery_plan(&opts).map_err(err)?;
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(middle(samples))
    };
    let angle = timed_edits(&mut s, "embroidery.set", &|k| json!({"ids": [first], "part": "fill", "angle": ANGLE_FROM + ANGLE_STEP * k as f64}))?;
    let fabric = timed_edits(&mut s, "embroidery.setup", &|k| json!({"fabric": {"tatami": {"pull": PULL_FROM + PULL_STEP * k as f64}}}))?;
    let ids: Vec<u64> = s.doc().map_err(err)?.doc.layers.iter().filter_map(|l| l.children()).flatten().map(|c| c.id.0).collect();
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        samples.push(timed(&mut s, "embroidery.info", &json!({"ids": ids}))?.0);
    }
    Ok([angle, fabric, middle(samples)])
}

/// The Object tab's rows, milliseconds, each the median of [`DRAW_RUNS`] after a nudge (a new
/// snapshot, as each frame of a drag or a scrub makes): its read of the settings of `doc`'s 1,000
/// objects selected (merged, none listed: `Session::embroidery_merged`), and its footer's fabric
/// and thread on `largest`, the 22,000-path document (`setup::fabric_and_thread`).
fn object_tab(doc: &Document, largest: &Document) -> Result<[f64; 2], String> {
    let ids_of = |s: &Session| -> Result<Vec<vectorcraft_doc::NodeId>, String> {
        Ok(s.doc().map_err(err)?.doc.layers.iter().filter_map(|l| l.children()).flatten().map(|c| c.id).collect())
    };
    let nudge = |s: &mut Session, id: vectorcraft_doc::NodeId| s.execute("object.move", &json!({"ids": [id.0], "dx": NUDGE_MM * PT_PER_MM, "dy": 0}));
    let mut s = fresh(doc);
    let ids = ids_of(&s)?;
    let first = *ids.first().ok_or("embroidery: the document has no object")?;
    let mut reads = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        nudge(&mut s, first).map_err(err)?;
        let t = Instant::now();
        s.embroidery_merged(&ids).map_err(err)?;
        reads.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mut s = fresh(largest);
    let first = *ids_of(&s)?.first().ok_or("embroidery: the document has no object")?;
    let mut footers = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        nudge(&mut s, first).map_err(err)?;
        let t = Instant::now();
        std::hint::black_box(vectorcraft_engine::cmd::embroidery::setup::fabric_and_thread(&s.doc().map_err(err)?.doc));
        footers.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok([middle(reads), middle(footers)])
}

/// `vectorcraft-cli bench FILE`'s stitch preview line for a design to stitch: its scene's build
/// and a frame at fit (the drawing rows' canvas), and from moving one object 0.5 mm to the frame of
/// the design then (plan, scene and frame); each the median of `runs` (at least one). `None` when
/// the document has nothing to stitch.
pub fn preview_line(doc: &Document, runs: usize) -> Option<String> {
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let mut s = fresh(doc);
    let planned = match s.embroidery_planner().and_then(|p| p.planned(&opts)) {
        Ok(p) => p,
        Err(e) => return Some(format!("  stitch preview: {e}")),
    };
    planned.plan.units.first()?;
    let (mut built, mut frames) = (Vec::new(), Vec::new());
    let mut r = LineRenderer::new();
    let (w, h) = CANVAS;
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        let scene = vectorcraft_engine::cmd::embroidery::scene::scene(&planned.plan);
        built.push(t.elapsed().as_secs_f64() * 1000.0);
        let (style, view) = framed(scene.picture(), FIT);
        let style = LineStyle { colors: scene.thread_colors(), ..style };
        let t = Instant::now();
        drop(r.render(scene.picture(), w, h, view, &style));
        frames.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let edited = match nudge_to_frame(doc, false) {
        Ok(ms) => format!("{ms:.1} ms"),
        Err(e) => e,
    };
    Some(format!(
        "  stitch preview: scene {:.2} ms, frame at fit {:.1} ms, after moving one object to its frame {edited}",
        middle(built),
        middle(frames)
    ))
}

/// Milliseconds each of the read commands the Embroidery panel and the status bar ask for takes on
/// `doc` once its plan is kept, as they ask (the Sewing order's list at its limit): `embroidery.stats`,
/// `sequence` and `check`, each the median of [`RUNS`]. One plan for the three.
fn replies(doc: &Document) -> Result<[f64; 3], String> {
    let mut s = fresh(doc);
    s.execute("embroidery.stats", &json!({})).map_err(err)?;
    let limit = vectorcraft_engine::cmd::embroidery::MAX_LIST_LIMIT;
    let mut out = [0.0; 3];
    for (k, (command, params)) in
        [("embroidery.stats", json!({})), ("embroidery.sequence", json!({ "limit": limit })), ("embroidery.check", json!({}))].into_iter().enumerate()
    {
        let mut samples = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            samples.push(timed(&mut s, command, &params)?.0);
        }
        if let Some(slot) = out.get_mut(k) {
            *slot = middle(samples);
        }
    }
    Ok(out)
}

/// Milliseconds a drag of every other object of `doc` (which is planned first) takes to start and
/// to find where it would drop halfway down the Sewing order, as the list works them out on the
/// plan it shows: the objects' movers and what they cover, then the drop's move; the median of
/// [`DRAW_RUNS`]. Every other object, so that the rows dragged lie apart and the drop is worked out
/// in full.
fn drag_half(doc: &Document) -> Result<f64, String> {
    use vectorcraft_engine::cmd::embroidery::reorder::{Moving, movers_of, place_moving};
    let mut s = fresh(doc);
    let plan = s.embroidery_plan(&vectorcraft_engine::EmbroideryOptions::default()).map_err(err)?;
    let doc = s.doc().map(|st| st.doc.clone()).map_err(err)?;
    let ids: Vec<vectorcraft_doc::NodeId> = doc.layers.iter().flat_map(|l| l.children().into_iter().flatten().map(|n| n.id)).step_by(2).collect();
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        let t = Instant::now();
        let moving = Moving::new(&doc, movers_of(&doc, &plan, &ids));
        let placed = place_moving(&doc, &plan, &moving, plan.units.len() / 2);
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
        if placed.is_err() {
            return Err(format!("embroidery: the drag of half the objects is refused: {placed:?}"));
        }
    }
    Ok(middle(samples))
}

/// Milliseconds a plan of `doc` takes to stop once cancelled [`CANCEL_AFTER_MS`] after it started
/// (on another thread, as the stitch preview's worker plans), the median of [`RUNS`].
fn cancel_latency(doc: &Document) -> Result<f64, String> {
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let mut s = fresh(doc);
        let planner = s.embroidery_planner().map_err(err)?;
        let token = vectorcraft_engine::cancel::CancelToken::new();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            vectorcraft_engine::cancel::scope(&worker_token, None, || planner.plan(&vectorcraft_engine::EmbroideryOptions::default()).is_err())
        });
        std::thread::sleep(std::time::Duration::from_millis(CANCEL_AFTER_MS));
        let t = Instant::now();
        token.cancel();
        let cancelled = worker.join().map_err(|_| "the plan panicked".to_string())?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
        if !cancelled {
            return Err(format!("the plan finished within {CANCEL_AFTER_MS} ms, before it was cancelled: nothing to measure"));
        }
    }
    Ok(middle(samples))
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

/// What a stitch picture's tiles hold besides their satin column and running line: a fill of rows
/// across (the points as planned, so most of a row's are left out as straight), the same fill's
/// rows at 45° with their points on a machine file's 0.1 mm grid (as an opened design's are, so
/// none is straight), or satin columns side by side across the whole tile instead of the fill.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tiles {
    Flat,
    Grid45,
    Satin,
}

/// The grid a machine file's points lie on, millimetres: the 0.1 mm its records count in (DST,
/// PES's PEC section, JEF, EXP).
const FILE_GRID: f64 = 0.1;

/// A stitch picture of about `n` stitches, as the stitch preview draws a design: square tiles in a
/// grid, each a fill of rows, a satin column along its side and a running line round it, in a
/// thread of its own; a move between tiles, cut every few tiles.
pub fn stitch_picture(n: usize) -> LinePicture {
    stitch_picture_of(n, Tiles::Flat)
}

/// [`stitch_picture`] with tiles of `tiles`.
pub fn stitch_picture_of(n: usize, tiles: Tiles) -> LinePicture {
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
        // The fill: rows across the tile, there and back (turned 45° about the tile's centre and
        // put on the file's grid for `Grid45`); or satin columns side by side across it.
        let centre = (x0 + TILE / 2.0, y0 + TILE / 2.0);
        let at = |x: f64, y: f64| match tiles {
            Tiles::Grid45 => {
                let (dx, dy) = (x - centre.0, y - centre.1);
                let (x, y) = (centre.0 + (dx - dy) * std::f64::consts::FRAC_1_SQRT_2, centre.1 + (dx + dy) * std::f64::consts::FRAC_1_SQRT_2);
                Point::new((x / FILE_GRID).round() * FILE_GRID, (y / FILE_GRID).round() * FILE_GRID)
            }
            Tiles::Flat | Tiles::Satin => Point::new(x, y),
        };
        if tiles == Tiles::Satin {
            let crossings = (TILE / SATIN_ADVANCE) as usize;
            for column in 0..(TILE / SATIN_WIDTH) as usize {
                b.begin_line(group, tag, LAID);
                let left = x0 + column as f64 * SATIN_WIDTH;
                for k in 0..crossings {
                    let x = if k % 2 == 0 { left } else { left + SATIN_WIDTH };
                    b.point(Point::new(x, y0 + k as f64 * SATIN_ADVANCE), number);
                    number += 1;
                }
                tag += 1;
            }
        } else {
            b.begin_line(group, tag, LAID);
            let rows = (TILE / ROW_SPACING) as usize;
            let per_row = (TILE / FILL_STITCH).ceil() as usize;
            for r in 0..rows {
                let y = y0 + r as f64 * ROW_SPACING;
                for k in 0..=per_row {
                    let t = k as f64 / per_row as f64;
                    let x = if r % 2 == 0 { x0 + t * TILE } else { x0 + (1.0 - t) * TILE };
                    b.point(at(x, y), number);
                    number += 1;
                }
            }
            tag += 1;
        }
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

/// Milliseconds a frame of one thread's lines of `picture` alone (the rest hidden) takes at fit, as
/// a moment's emphasis draws them (a Sewing order header hovered), the median of [`DRAW_RUNS`].
fn emphasis(picture: &LinePicture) -> f64 {
    let rest = picture.lines().iter().filter(|l| l.group != 0).map(|l| l.tag);
    let hidden = vectorcraft_render::lines::TagSet::of(rest);
    let mut r = LineRenderer::new();
    let (style, view) = framed(picture, FIT);
    let style = LineStyle { hidden: Some(hidden), ..style };
    let (w, h) = CANVAS;
    drop(r.render(picture, w, h, view, &style));
    let mut samples = Vec::with_capacity(DRAW_RUNS);
    for _ in 0..DRAW_RUNS {
        let t = Instant::now();
        drop(r.render(picture, w, h, view, &style));
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    middle(samples)
}

/// The corpus design the drawing's corpus row draws (`cargo xtask corpus --embroidery`): its largest
/// PES, a person's file, opened as stitches.
const CORPUS_PES: &str = "zebra-twemoji-inkstitch.pes";

/// Milliseconds a frame of [`CORPUS_PES`] opened as stitches takes at fit, as [`draw`] times it;
/// `None` without the corpus.
fn corpus_frame() -> Result<Option<f64>, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/embroidery").join(CORPUS_PES);
    let Ok(bytes) = std::fs::read(&path) else { return Ok(None) };
    let mut s = Session::new();
    s.execute("document.open", &json!({"name": CORPUS_PES, "dataBase64": vectorcraft_format::base64_encode(&bytes)})).map_err(err)?;
    let planned = s.embroidery_planner().and_then(|p| p.planned(&vectorcraft_engine::EmbroideryOptions::default())).map_err(err)?;
    Ok(Some(draw(planned.scene().picture(), FIT, None)))
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

    /// The variants hold about as many stitches, and draw more segments than the flat fill: the
    /// 45° rows on the file's grid have none left out as straight, the satin tiles are zigzags.
    #[test]
    fn the_variants_draw_more_than_the_flat_fill() {
        // The segments a frame at fit draws.
        let segments = |p: &LinePicture| {
            let mut r = LineRenderer::new();
            let (style, view) = framed(p, FIT);
            drop(r.render(p, CANVAS.0, CANVAS.1, view, &style));
            r.stats.segments
        };
        let flat = segments(&stitch_picture(10_000));
        for tiles in [Tiles::Grid45, Tiles::Satin] {
            let p = stitch_picture_of(10_000, tiles);
            assert!((10_000..12_000).contains(&p.point_count()), "{tiles:?}: {}", p.point_count());
            let drawn = segments(&p);
            assert!(drawn > flat * 3 / 2, "{tiles:?}: {drawn} segments against {flat}");
        }
    }
}
