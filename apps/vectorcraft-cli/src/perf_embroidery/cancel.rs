//! The cancel row of `vectorcraft-cli perf --embroidery`: how long a plan takes to stop once
//! cancelled inside one large object, at its worst. The plan runs on another thread, as the stitch
//! preview's worker plans, and is cancelled at five points spread over its run, each a fresh plan.
//!
//! The document, built here: one fill with 48 × 48 square holes, a quarter of the dot grid the first
//! digitizing PR measured a cancel on (a navy grid set to Fill, 9,216 holes: its rows break into
//! tens of thousands of regions), each hole 1.2 mm wide and 1.5 mm from the next. It is planned with
//! no narrow parts sewn as satin (`narrowAsSatin` 0), all in rows, as that grid was: the row times
//! the planner's own steps. With its narrow parts as satin, its bands are 4,512 of them, and a
//! cancel waits for upstream's path operations that cut them out, which check for none (uniting
//! them takes 0.39 s in release, 2026-11-09): the one place the tenth of a second does not hold.

use std::time::{Duration, Instant};

use serde_json::json;
use vectorcraft_doc::Document;
use vectorcraft_engine::EmbroideryOptions;
use vectorcraft_engine::cancel::{self, CancelToken};
use vectorcraft_geom::{BezPath, FillRule};

use super::{err, fresh, one_fill, polygon};

/// The rows' names, in order (at most 42 characters, the report's column).
pub(super) const ROWS: [&str; 1] = ["preview: cancel, worst, 2,304 holes, rows"];

/// Each row's budget, milliseconds: the longest a cancel may wait in the planner, a tenth of a
/// second (the project owner's choice, 2026-11-09, „0,1 s (polecam)”: the limit within which a
/// response feels instant, Jakob Nielsen, *Response Times: The 3 Important Limits*,
/// <https://www.nngroup.com/articles/response-times-3-important-limits/>).
pub(super) const BUDGETS: [f64; 1] = [CANCEL_BOUND_MS];

/// The longest a cancel may wait in the planner (milliseconds), as [`BUDGETS`] says.
const CANCEL_BOUND_MS: f64 = 100.0;

/// The holes a side, how wide each is and how far one starts from the next (millimetres), and how
/// far in from the fill's corner the first starts.
const HOLES: usize = 48;
const HOLE_MM: f64 = 1.2;
const PITCH_MM: f64 = 1.5;
const MARGIN_MM: f64 = 2.0;

/// Where in a whole plan's run each cancel comes, as shares of it.
const AT: [f64; 5] = [0.1, 0.3, 0.5, 0.7, 0.9];

/// The fill: a square with [`HOLES`] × [`HOLES`] square holes (millimetres), filled even-odd.
fn holed() -> BezPath {
    let side = 2.0 * MARGIN_MM + PITCH_MM * HOLES as f64;
    let mut path = BezPath::new();
    polygon(&mut path, &[(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)]);
    for i in 0..HOLES {
        for j in 0..HOLES {
            let (x, y) = (MARGIN_MM + PITCH_MM * i as f64, MARGIN_MM + PITCH_MM * j as f64);
            polygon(&mut path, &[(x, y), (x + HOLE_MM, y), (x + HOLE_MM, y + HOLE_MM), (x, y + HOLE_MM)]);
        }
    }
    path
}

/// The options the row plans with: no narrow parts sewn as satin, the fill all in rows.
fn in_rows() -> Result<EmbroideryOptions, String> {
    EmbroideryOptions::from_json(&json!({"narrowAsSatin": 0})).map_err(err)
}

/// How long a fresh plan of `doc` with `options` takes, whole.
fn whole(doc: &Document, options: &EmbroideryOptions) -> Result<Duration, String> {
    let mut s = fresh(doc);
    let planner = s.embroidery_planner().map_err(err)?;
    let t = Instant::now();
    cancel::scope(&CancelToken::new(), None, || planner.plan(options)).map_err(err)?;
    Ok(t.elapsed())
}

/// Milliseconds from a cancel `after` into a fresh plan of `doc` with `options`, on another thread,
/// until the plan returns (stopped, or done when it was done first).
fn wait(doc: &Document, options: &EmbroideryOptions, after: Duration) -> Result<f64, String> {
    let mut s = fresh(doc);
    let planner = s.embroidery_planner().map_err(err)?;
    let token = CancelToken::new();
    let worker_token = token.clone();
    let options = *options;
    let worker = std::thread::spawn(move || cancel::scope(&worker_token, None, || planner.plan(&options).is_err()));
    std::thread::sleep(after);
    let t = Instant::now();
    token.cancel();
    worker.join().map_err(|_| "the plan panicked".to_string())?;
    Ok(t.elapsed().as_secs_f64() * 1000.0)
}

/// The rows' milliseconds, in the order of [`ROWS`]: the longest of the waits at [`AT`].
pub(super) fn times() -> Result<Vec<f64>, String> {
    let doc = one_fill(holed(), FillRule::EvenOdd);
    let options = in_rows()?;
    let full = whole(&doc, &options)?;
    let mut worst: f64 = 0.0;
    for share in AT {
        worst = worst.max(wait(&doc, &options, full.mul_f64(share))?);
    }
    Ok(vec![worst])
}
