//! The routes rows of `vectorcraft-cli perf --embroidery`: running lines whose pieces meet end to
//! end, which the stitch planner sews as one route each (`vectorcraft_stitch::plan`'s routes), and
//! fills whose rows run across their shape (`embroidery.setup {fillAngle: "acrossShape"}`), each
//! planned cold.
//!
//! The documents, built here:
//! - a running line of 32,766 pieces meeting as an H-tree fourteen levels deep: a tree, each piece
//!   sewn once but those the needle goes back along;
//! - a running line of a 100 × 100 grid's 19,800 pieces: cycles, the pieces sewn twice chosen among
//!   them;
//! - the 1,000-path document with its fills' rows across their shape.

use serde_json::json;
use vectorcraft_doc::Document;
use vectorcraft_engine::Session;

use super::{PATHS, cold, err, fresh};
use crate::perf::synthetic;

/// The rows' names, in order (at most 42 characters, the report's column).
pub(super) const ROWS: [&str; 3] =
    ["routes: a running line, 32,766-piece tree", "routes: a running line, 100 × 100 grid", "rows across shapes, 1,000 paths"];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: about 2.5 times what each measured
/// ([`MEASURED_MS`]), as the other rows', so that a regression of that size shows.
pub(super) const BUDGETS: [f64; 3] = [MEASURED_MS[0] * 2.5, MEASURED_MS[1] * 2.5, MEASURED_MS[2] * 2.5];

/// What each row measured on 2026-11-07 (release, 12-core Apple Silicon, load about 3 to 5, the
/// median of three, the later of two runs within 2 % of the first): the tree routed in 24.5 ms, the
/// grid in 18.6 ms, and the 1,000-path document with its rows across their shape planned in 82.5 ms
/// (78.5 ms with the fixed rows in the same run).
const MEASURED_MS: [f64; 3] = [24.5, 18.6, 82.5];

/// The H-tree's depth (each level splits every end in two: 2¹⁵ − 2 pieces), its first piece's
/// length and how much shorter each level's are (points): 400 points, shorter by √2 a level, so
/// that the last level's are 4.4 points (1.6 mm) and no two pieces touch but at their nodes.
const TREE_DEPTH: u32 = 14;
const TREE_FIRST_PT: f64 = 400.0;

/// The grid: nodes a side, and how far apart (points: 7 mm).
const GRID_NODES: usize = 100;
const GRID_STEP_PT: f64 = 20.0;

/// The lines' weight (points): 0.35 mm, a running line.
const LINE_PT: f64 = 1.0;

/// A piece of a running line: from one point to another (points).
type Piece = ((f64, f64), (f64, f64));

/// A document of one running line in navy, its pieces `pieces` (each from one point to another,
/// in points), on a page `side` points square.
fn running_line(pieces: &[Piece], side: f64) -> Result<Document, String> {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": side, "height": side, "artboards": 1})).map_err(err)?;
    s.execute("paint.setFill", &json!({"none": true})).map_err(err)?;
    s.execute("paint.setStroke", &json!({"color": "#1d2a6b"})).map_err(err)?;
    let d: String = pieces.iter().map(|&((x0, y0), (x1, y1))| format!("M {x0} {y0} L {x1} {y1} ")).collect();
    let id = s.execute("path.create", &json!({"d": d})).map_err(err)?["id"].clone();
    s.execute("stroke.set", &json!({"ids": [id], "weight": LINE_PT})).map_err(err)?;
    Ok((*s.doc().map_err(err)?.doc).clone())
}

/// The H-tree's pieces round `(x, y)`: at each level every end splits along one axis and then the
/// other, its pieces `length` long.
fn h_tree(x: f64, y: f64, length: f64, depth: u32, across: bool, out: &mut Vec<Piece>) {
    if depth == 0 {
        return;
    }
    let (dx, dy) = if across { (length, 0.0) } else { (0.0, length) };
    for sign in [-1.0, 1.0] {
        let end = (x + sign * dx, y + sign * dy);
        out.push(((x, y), end));
        h_tree(end.0, end.1, length / std::f64::consts::SQRT_2, depth - 1, !across, out);
    }
}

/// The routes rows' milliseconds, in the order of [`ROWS`].
pub(super) fn times() -> Result<Vec<f64>, String> {
    let mut tree = Vec::new();
    let side = 4.0 * TREE_FIRST_PT;
    h_tree(side / 2.0, side / 2.0, TREE_FIRST_PT, TREE_DEPTH, true, &mut tree);
    let tree_ms = cold(&running_line(&tree, side)?)?;
    let at = |k: usize| GRID_STEP_PT * (k as f64 + 1.0);
    let mut grid = Vec::new();
    for i in 0..GRID_NODES {
        for j in 0..GRID_NODES - 1 {
            grid.push(((at(j), at(i)), (at(j + 1), at(i))));
            grid.push(((at(i), at(j)), (at(i), at(j + 1))));
        }
    }
    let grid_ms = cold(&running_line(&grid, at(GRID_NODES))?)?;
    let mut s = fresh(&synthetic(PATHS)?);
    s.execute("embroidery.setup", &json!({"fillAngle": "acrossShape"})).map_err(err)?;
    let across = (*s.doc().map_err(err)?.doc).clone();
    Ok(vec![tree_ms, grid_ms, cold(&across)?])
}
