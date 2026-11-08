//! A filled shape's long axis, and the rows that run across it (the project owner's choice,
//! 2026-11-01, „W poprzek kształtu (polecam)”: "by shape" lays a fill's rows across the shape's
//! length, against pucker, as Brother advises a stitch direction "perpendicular to the larger edge"
//! of an area, <https://support.brother.com/g/s/hf/htmldoc/ped/im/ped11/en/PED11_EN/pages/15_1161138.html>).
//!
//! The axis comes from the second moments of the shape's area (the textbook orientation of a
//! region: the direction `½·atan2(2μ₁₁, μ₂₀ − μ₀₂)` of the largest spread, with the moments'
//! eigenvalues telling how much longer the shape is than wide). They are integrated over the
//! planner's own row sweep (`super::area::Sweep`, by the shape's fill rule, holes left out),
//! exactly along each of [`SHAPE_AXIS_BANDS`] bands across the shape and by the band's middle
//! across it: so a shape's holes, crossings and either fill rule are measured as its rows are sewn.
//!
//! A fill made of separate pieces (outlines that neither cross, touch nor lie one inside the
//! other: a word's letters, or a logo's parts, saved as one shape) is measured piece by piece, each
//! piece's rows across its own length (the project owner's choice, 2026-11-07, „Każdy kawałek
//! osobno (polecam)”): measured together, two upright bars side by side are wider than tall, and
//! their rows would run along each bar.

use vectorcraft_geom::{BezPath, FillRule};

use super::area::{Area, Grid};
use super::fill::{closed_subpaths, outlines};
use super::geometry::{Axes, SubPath};

/// How many bands across the shape its moments are summed over. An estimate: on a 10 × 30 mm bar
/// turned every 15°, the axis comes within a hundredth of a degree.
pub const SHAPE_AXIS_BANDS: usize = 128;

/// How much longer than wide a shape must be (the square root of its moments' eigenvalues' ratio,
/// as [`told_elongation`] tells it) for its rows to follow it: rounder, its axis turns at a touch
/// and the rows keep the fixed angle. An estimate (the research's), the session's choice: a 3 × 2
/// rectangle is 1.5 times as long as wide by this measure, on the line, and its rows run across it.
pub const SHAPE_AXIS_MIN_ELONGATION: f64 = 1.5;

/// How finely a shape's elongation is told: in hundredths. The rule decides on it so rounded, and
/// `embroidery.info` reports it so: the two agree, and a shape on the line, which the bands
/// measure a few ten-thousandths either side of it as it is turned, keeps one kind of rows at
/// every turn.
pub const ELONGATION_STEPS: f64 = 100.0;

/// `elongation` as the rule decides on it and `embroidery.info` reports it: rounded to
/// hundredths ([`ELONGATION_STEPS`]).
pub fn told_elongation(elongation: f64) -> f64 {
    (elongation * ELONGATION_STEPS).round() / ELONGATION_STEPS
}

/// The step the rows' angle is rounded to (degrees): the embroidery tool's arrow keeps an angle to
/// a tenth of a degree, and two systems' arctangents that differ in their last bit give the same
/// rows.
pub const SHAPE_ANGLE_STEP: f64 = 0.1;

/// [`SHAPE_ANGLE_STEP`]s in a degree: the rows' angle is rounded by dividing by it, so that it is
/// the decimal number of tenths (94.1, not the 94.10000000000001 that 941 steps of 0.1 make).
pub const ANGLE_STEPS: f64 = 10.0;

/// `degrees` (a direction of rows) as its rows are told: from 0 up to a half turn, rounded to
/// [`SHAPE_ANGLE_STEP`] by [`ANGLE_STEPS`] (an upright bar's rows across are 0°, never 180°).
pub fn told_rows(degrees: f64) -> f64 {
    ((degrees * ANGLE_STEPS).round() / ANGLE_STEPS).rem_euclid(HALF_TURN)
}

/// Degrees in a half turn: a line's direction is the same either way round it.
const HALF_TURN: f64 = 180.0;

/// A quarter turn (degrees): across is square to along.
const QUARTER_TURN: f64 = 90.0;

/// A shape's long axis: its direction (degrees, as the planner counts them, clockwise as seen on
/// the page from left to right, from 0 up to a half turn) and how much longer the shape is than
/// wide along it (at least 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeAxis {
    pub angle: f64,
    pub elongation: f64,
}

/// The long axis of `path` (millimetres) filled by `rule`, from its area's second moments (the
/// module's notes), all its outlines together; `None` for a shape with no area, as wide every way
/// (its spread the same in every direction), or not finite.
pub fn shape_axis(path: &BezPath, rule: FillRule) -> Option<ShapeAxis> {
    if !super::in_range(path) {
        return None;
    }
    let subs = closed_subpaths(path);
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return None;
    }
    measured(Area { outlines: &outlines, rule, grid: None })?.1
}

/// `shape`'s area (square millimetres, by its fill rule) and its long axis ([`shape_axis`]), `None`
/// for an area that isn't a positive number; the axis `None` for a shape as wide every way.
fn measured(shape: Area<'_>) -> Option<(f64, Option<ShapeAxis>)> {
    let axes = Axes::new(0.0);
    let (lo, hi) = shape.extent(axes.across)?;
    let band = (hi - lo) / SHAPE_AXIS_BANDS as f64;
    if !(band.is_finite() && band > 0.0) {
        return None;
    }
    // The moments are taken from the middle of the shape's box: far from the page's origin, the
    // coordinates' cubes would lose the shape's own size to rounding (a 3 × 1 mm bar a kilometre
    // out read its axis 26° off).
    let (left, right) = shape.extent(axes.along)?;
    let (mx, my) = ((left + right) / 2.0, (lo + hi) / 2.0);
    let mut sweep = shape.sweep(axes);
    // The moments: the area, the first moments, and the second ones.
    let (mut a, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for k in 0..SHAPE_AXIS_BANDS {
        let row = lo + (k as f64 + 0.5) * band;
        let y = row - my;
        for (x0, x1) in sweep.spans(row) {
            let (x0, x1) = (x0 - mx, x1 - mx);
            let w = x1 - x0;
            a += w * band;
            sx += (x1 * x1 - x0 * x0) / 2.0 * band;
            sy += y * w * band;
            sxx += (x1 * x1 * x1 - x0 * x0 * x0) / 3.0 * band;
            // A band's own spread across it: its height squared over twelve.
            syy += (y * y + band * band / 12.0) * w * band;
            sxy += y * (x1 * x1 - x0 * x0) / 2.0 * band;
        }
    }
    if !(a.is_finite() && a > 0.0) {
        return None;
    }
    let (cx, cy) = (sx / a, sy / a);
    let (m20, m02, m11) = (sxx / a - cx * cx, syy / a - cy * cy, sxy / a - cx * cy);
    let half_sum = (m20 + m02) / 2.0;
    let spread = (((m20 - m02) / 2.0).powi(2) + m11 * m11).sqrt();
    let (long, short) = (half_sum + spread, half_sum - spread);
    if !(long.is_finite() && short.is_finite()) || short <= 0.0 || spread <= 0.0 {
        return Some((a, None));
    }
    let angle = (0.5 * (2.0 * m11).atan2(m20 - m02)).to_degrees().rem_euclid(HALF_TURN);
    Some((a, Some(ShapeAxis { angle, elongation: (long / short).sqrt() })))
}

/// The angle a fill's rows run at across `path`'s length (degrees as the planner counts them,
/// rounded to [`SHAPE_ANGLE_STEP`]): square to its long axis ([`shape_axis`]), all its outlines
/// together, when it is at least [`SHAPE_AXIS_MIN_ELONGATION`] times as long as wide (told in
/// hundredths, [`told_elongation`]); `None` otherwise (the rows keep the fixed angle). With the
/// axis.
pub fn rows_across(path: &BezPath, rule: FillRule) -> Option<(f64, ShapeAxis)> {
    let axis = shape_axis(path, rule)?;
    Some((across(axis)?, axis))
}

/// The rows across a shape with long axis `axis` ([`rows_across`]), `None` for one about as long
/// as wide (its elongation, as [`told_elongation`] tells it, under [`SHAPE_AXIS_MIN_ELONGATION`]).
fn across(axis: ShapeAxis) -> Option<f64> {
    if told_elongation(axis.elongation) < SHAPE_AXIS_MIN_ELONGATION {
        return None;
    }
    Some(told_rows(axis.angle + QUARTER_TURN))
}

/// One separate piece of a fill as the rows across the shape measure it ([`rows_by_piece`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceRows {
    /// The direction its rows run (degrees as the planner counts them): across it
    /// ([`rows_across`]), or the fixed angle given for a piece about as long as wide.
    pub angle: f64,
    /// Whether they run across it: it is at least [`SHAPE_AXIS_MIN_ELONGATION`] times as long as
    /// wide.
    pub across: bool,
    /// Its long axis; `None` for a piece with no area or as wide every way.
    pub axis: Option<ShapeAxis>,
    /// Its area by the fill rule (square millimetres).
    pub area: f64,
}

impl PieceRows {
    /// The rows of `shape`, one piece, `fixed` the angle it keeps when about as long as wide.
    fn of(shape: Area<'_>, fixed: f64) -> Self {
        let (area, axis) = measured(shape).unwrap_or((0.0, None));
        let rows = axis.and_then(across);
        Self { angle: rows.unwrap_or(fixed), across: rows.is_some(), axis, area }
    }

    /// The largest of `pieces` (the first of equally large ones): the one whose rows an object's
    /// rows are reported by.
    pub fn largest(pieces: &[PieceRows]) -> Option<PieceRows> {
        pieces.iter().copied().reduce(|a, b| if b.area > a.area { b } else { a })
    }
}

/// The most directions a fill's pieces are sewn in, one after another ([`Rows::AcrossShape`]):
/// past it, the shape's rows are measured all its pieces together, a bound on the work of ordering
/// them (each step of the order weighs every direction left). An estimate: a sunburst of 36 rays
/// saved as one shape has 36.
///
/// [`Rows::AcrossShape`]: super::Rows::AcrossShape
pub(super) const MAX_DIRECTIONS: usize = 64;

/// The rows of each separate piece of `shape` ([`Area::pieces_apart`]), with the piece's outlines,
/// in the order of the pieces' first outlines; `fixed` the angle a piece about as long as wide
/// keeps. Past [`MAX_DIRECTIONS`] directions, one piece of every outline.
pub(super) fn piece_rows<'a>(shape: Area<'a>, fixed: f64) -> Vec<(Vec<&'a SubPath>, PieceRows)> {
    let pieces: Vec<(Vec<&'a SubPath>, PieceRows)> = shape
        .pieces_apart()
        .into_iter()
        .map(|piece| {
            let outlines: Vec<&'a SubPath> = piece.iter().filter_map(|&i| shape.outlines.get(i).copied()).collect();
            let rows = PieceRows::of(Area { outlines: &outlines, rule: shape.rule, grid: None }, fixed);
            (outlines, rows)
        })
        .collect();
    if directions(pieces.iter().map(|(_, r)| r.angle)).len() > MAX_DIRECTIONS {
        return vec![(shape.outlines.to_vec(), PieceRows::of(Area { grid: None, ..shape }, fixed))];
    }
    pieces
}

/// The different directions among `angles` (degrees), in the order they first come: two within
/// half a [`SHAPE_ANGLE_STEP`] are one.
pub(super) fn directions(angles: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for a in angles {
        if !out.iter().any(|b| same_direction(a, *b)) {
            out.push(a);
        }
    }
    out
}

/// Whether two directions of rows (degrees) are one: within half a [`SHAPE_ANGLE_STEP`].
pub(super) fn same_direction(a: f64, b: f64) -> bool {
    (a - b).abs() < SHAPE_ANGLE_STEP / 2.0
}

/// The rows of each separate piece of `path` (millimetres) filled by `rule`, as a fill whose rows
/// run across the shape sews them ([`Rows::AcrossShape`]): outlines that neither cross, touch nor
/// lie one inside the other are pieces apart, each measured alone; `fixed` the angle a piece about
/// as long as wide keeps. In the order of the pieces' first outlines; none for a shape with no
/// closed outline or out of range.
///
/// [`Rows::AcrossShape`]: super::Rows::AcrossShape
pub fn rows_by_piece(path: &BezPath, rule: FillRule, fixed: f64) -> Vec<PieceRows> {
    if !super::in_range(path) {
        return Vec::new();
    }
    let subs = closed_subpaths(path);
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return Vec::new();
    }
    // The grid only speeds telling outlines apart: one outline is one piece.
    let grid = if outlines.len() > 1 { Grid::new(&outlines) } else { None };
    piece_rows(Area { outlines: &outlines, rule, grid: grid.as_ref() }, fixed).into_iter().map(|(_, r)| r).collect()
}
