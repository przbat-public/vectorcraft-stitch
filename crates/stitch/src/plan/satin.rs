//! Satin columns ([`Style::Satin`](super::Style::Satin)): the thread zig-zags across a band along
//! a path, over an underlay chosen by the band's width, with long crossings split.

use vectorcraft_geom::{BezPath, Point, Vec2};

use super::geometry::{length, resample, subpaths, tangent};
use super::runs::{Runs, Travel};
use super::{Params, Style, UNDERLAY_STITCH};
use crate::Stitch;

/// Satin underlay by column width (millimetres): from this width an edge run instead of a centre
/// run, from this one an edge run and a zigzag; and the zigzag's spacing. Estimates.
const SATIN_EDGE_UNDERLAY: f64 = 2.0;
const SATIN_ZIGZAG_UNDERLAY: f64 = 4.5;
const ZIGZAG_UNDERLAY_SPACING: f64 = 2.0;

/// A satin column sewn along `path`'s centreline, [`Params::width`] wide.
pub fn satin(path: &BezPath, p: &Params) -> Vec<Stitch> {
    let mut p = p.sane();
    p.style = Style::Satin;
    if p.width <= 0.0 {
        return Vec::new();
    }
    let half = (p.width + 2.0 * p.pull) / 2.0;
    let mut runs = Runs::default();
    for sub in subpaths(path) {
        let pts = sub.points();
        if length(&pts, sub.closed) < p.density * 2.0 {
            continue;
        }
        // The first run of each column is reached as any move is; the column's later runs
        // (underlay to top stitching) start inside the column, so the needle sews there.
        let mut travel = Travel::Auto;
        // The underlay, by the column's width; it may end at the column's far end, and then the
        // top stitching starts there and comes back.
        let mut back = false;
        if p.underlay {
            for (under, ends_far) in satin_underlay(&pts, sub.closed, p.width) {
                if under.len() >= 2 {
                    runs.push(under, travel);
                    travel = Travel::Sew;
                }
                back = ends_far;
            }
        }
        let spacing = if p.auto_spacing { satin_spacing(p.width) } else { p.density };
        let mut samples = resample(&pts, spacing, sub.closed);
        if samples.len() < 2 {
            continue;
        }
        if back && !sub.closed {
            samples.reverse();
        }
        let mut zigzag: Vec<Point> = (0..samples.len())
            .filter_map(|i| {
                let q = *samples.get(i)?;
                let t = tangent(&samples, i, sub.closed);
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                Some(q + Vec2::new(-t.y, t.x) * half * side)
            })
            .collect();
        // A closed column ends where it started: the last crossing takes the remaining side.
        if sub.closed
            && let (Some(first), Some(last)) = (zigzag.first().copied(), zigzag.last().copied())
            && first.distance(last) > p.min_stitch
        {
            zigzag.push(first);
        }
        runs.push(split_long(&zigzag, p.max_satin), travel);
        if runs.full() {
            break;
        }
    }
    runs.join(&p)
}

/// The spacing of a satin column's crossings for its `width` (auto spacing): more open in a narrow
/// column, where the thread bunches at the rails and covers anyway, denser in a wide one so it
/// covers. A stepped table, as professional software keeps one; the values are estimates.
pub fn satin_spacing(width: f64) -> f64 {
    const TABLE: [(f64, f64); 3] = [(2.0, 0.5), (4.0, 0.45), (6.0, 0.4)];
    TABLE.iter().find(|(w, _)| width < *w).map_or(0.37, |(_, spacing)| *spacing)
}

/// The underlay of a satin column along `pts` (its centreline), `width` wide, by width: a centre
/// run for a narrow column, an edge run (along one side and back along the other) for a medium one,
/// and an edge run then a zigzag between its sides for a wide one. Each run comes with whether it
/// ends at the column's far end. The thresholds are estimates from the usual practice.
fn satin_underlay(pts: &[Point], closed: bool, width: f64) -> Vec<(Vec<Point>, bool)> {
    let centre = resample(pts, UNDERLAY_STITCH, closed);
    if width < SATIN_EDGE_UNDERLAY {
        return vec![(centre, !closed)];
    }
    // The sides, kept inside the column so the underlay doesn't show at its edges.
    let side = (width / 2.0 - (width / 4.0).min(0.5)).max(0.0);
    let offset = |samples: &[Point], i: usize, d: f64| -> Option<Point> {
        let q = *samples.get(i)?;
        let t = tangent(samples, i, closed);
        Some(q + Vec2::new(-t.y, t.x) * d)
    };
    let mut edge: Vec<Point> = (0..centre.len()).filter_map(|i| offset(&centre, i, side)).collect();
    edge.extend((0..centre.len()).rev().filter_map(|i| offset(&centre, i, -side)));
    let mut out = vec![(edge, false)];
    if width >= SATIN_ZIGZAG_UNDERLAY {
        let steps = resample(pts, ZIGZAG_UNDERLAY_SPACING, closed);
        let zigzag: Vec<Point> = (0..steps.len()).filter_map(|i| offset(&steps, i, if i % 2 == 0 { side } else { -side })).collect();
        out.push((zigzag, !closed));
    }
    out
}

/// `zigzag`'s crossings longer than `max` split into shorter stitches (auto split). Each long
/// crossing is cut into equal parts shifted by a fraction that changes from one crossing to the
/// next (the golden ratio's sequence), so the splits of neighbouring crossings never line up into
/// a seam down the column; no part is longer than `max`.
fn split_long(zigzag: &[Point], max: f64) -> Vec<Point> {
    /// How far the cuts move along a crossing, as a share of one part (either way).
    const JITTER: f64 = 0.25;
    let mut out: Vec<Point> = Vec::with_capacity(zigzag.len());
    let mut crossing = 0u32;
    for (i, &q) in zigzag.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).and_then(|j| zigzag.get(j)) {
            let len = p.distance(q);
            if len.is_finite() && len > max {
                // The longest part is (1 + 2·JITTER)/n of the crossing: enough parts that it fits.
                let n = ((len * (1.0 + 2.0 * JITTER)) / max).ceil().clamp(2.0, 64.0) as u32;
                crossing += 1;
                let shift = ((f64::from(crossing) * 0.618_033_988_75).fract() - 0.5) * 2.0 * JITTER;
                out.extend((1..n).map(|k| p.lerp(q, (f64::from(k) + shift) / f64::from(n))));
            }
        }
        out.push(q);
    }
    out
}
