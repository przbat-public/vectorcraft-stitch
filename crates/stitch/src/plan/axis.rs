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

use vectorcraft_geom::{BezPath, FillRule};

use super::area::Area;
use super::fill::{closed_subpaths, outlines};
use super::geometry::Axes;

/// How many bands across the shape its moments are summed over. An estimate: on a 10 × 30 mm bar
/// turned every 15°, the axis comes within a hundredth of a degree.
pub const SHAPE_AXIS_BANDS: usize = 128;

/// How much longer than wide a shape must be (the square root of its moments' eigenvalues' ratio)
/// for its rows to follow it: rounder, its axis turns at a touch and the rows keep the fixed angle.
/// An estimate (the research's), the session's choice: a 3 × 2 rectangle is 1.5 times as long as
/// wide by this measure.
pub const SHAPE_AXIS_MIN_ELONGATION: f64 = 1.5;

/// The step the rows' angle is rounded to (degrees): the embroidery tool's arrow keeps an angle to
/// a tenth of a degree, and two systems' arctangents that differ in their last bit give the same
/// rows.
pub const SHAPE_ANGLE_STEP: f64 = 0.1;

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
/// module's notes); `None` for a shape with no area, as wide every way (its spread the same in
/// every direction), or not finite.
pub fn shape_axis(path: &BezPath, rule: FillRule) -> Option<ShapeAxis> {
    if !super::in_range(path) {
        return None;
    }
    let subs = closed_subpaths(path);
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return None;
    }
    let shape = Area { outlines: &outlines, rule, grid: None };
    let axes = Axes::new(0.0);
    let (lo, hi) = shape.extent(axes.across)?;
    let band = (hi - lo) / SHAPE_AXIS_BANDS as f64;
    if !(band.is_finite() && band > 0.0) {
        return None;
    }
    let mut sweep = shape.sweep(axes);
    // The moments: the area, the first moments, and the second ones.
    let (mut a, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for k in 0..SHAPE_AXIS_BANDS {
        let y = lo + (k as f64 + 0.5) * band;
        for (x0, x1) in sweep.spans(y) {
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
        return None;
    }
    let angle = (0.5 * (2.0 * m11).atan2(m20 - m02)).to_degrees().rem_euclid(HALF_TURN);
    Some(ShapeAxis { angle, elongation: (long / short).sqrt() })
}

/// The angle a fill's rows run at across `path`'s length (degrees as the planner counts them,
/// rounded to [`SHAPE_ANGLE_STEP`]): square to its long axis ([`shape_axis`]), when it is at least
/// [`SHAPE_AXIS_MIN_ELONGATION`] times as long as wide; `None` otherwise (the rows keep the fixed
/// angle). With the axis.
pub fn rows_across(path: &BezPath, rule: FillRule) -> Option<(f64, ShapeAxis)> {
    let axis = shape_axis(path, rule)?;
    if axis.elongation < SHAPE_AXIS_MIN_ELONGATION {
        return None;
    }
    let across = ((axis.angle + QUARTER_TURN) / SHAPE_ANGLE_STEP).round() * SHAPE_ANGLE_STEP;
    Some((across.rem_euclid(HALF_TURN), axis))
}
