//! Satin columns ([`Style::Satin`](super::Style::Satin)): the thread zig-zags across a band along
//! a path, over an underlay chosen by the band's width, with long crossings split, the rails
//! widened by pull compensation and the ends shortened by push compensation.

use vectorcraft_geom::{BezPath, Point, Vec2};

use super::geometry::{Line, length, nearest_order, resample, subpaths, tangent, trimmed, unit};
use super::runs::{Runs, Travel};
use super::{MIN_PUSHED, Params, Style, UNDERLAY_STITCH, thread_scale};
use crate::Stitch;

/// Satin underlay by column width (millimetres): from this width an edge run instead of a centre
/// run, from this one an edge run and a zigzag; and the zigzag's spacing. Estimates.
const SATIN_EDGE_UNDERLAY: f64 = 2.0;
const SATIN_ZIGZAG_UNDERLAY: f64 = 4.5;
const ZIGZAG_UNDERLAY_SPACING: f64 = 2.0;
/// How far inside the column's sides the edge underlay runs: this share of the column's width, at
/// most [`UNDERLAY_INSET_MAX`] millimetres, so the underlay doesn't show at the edges.
const UNDERLAY_INSET: f64 = 0.25;
const UNDERLAY_INSET_MAX: f64 = 0.5;
/// A column shorter than this many crossings is left out: it would be a knot, not a column.
const MIN_CROSSINGS: f64 = 2.0;
/// Auto spacing ([`satin_spacing`]): for a column narrower than each width (millimetres), the
/// spacing of its crossings (millimetres), and [`WIDE_SPACING`] for wider ones. Estimates.
const SPACING_BY_WIDTH: [(f64, f64); 3] = [(2.0, 0.5), (4.0, 0.45), (6.0, 0.4)];
const WIDE_SPACING: f64 = 0.37;
/// The most stitches a long crossing is split into ([`split_long`]): enough for the widest column.
const MAX_SPLIT: f64 = 64.0;
/// The golden ratio's fractional part: its multiples' fractions spread evenly over the unit
/// interval and never repeat, so the shift of a split moves on from one crossing to the next
/// without a pattern ([`split_long`]).
const GOLDEN: f64 = 0.618_033_988_75;

/// A satin column sewn along `path`'s centreline, [`Params::width`] wide and wider at each rail by
/// its pull compensation ([`Params::compensation`] of the width: the left rail as the path is
/// drawn, then the right one), each open column shorter at its ends by [`Params::push`]. The
/// crossings are [`satin_spacing`] apart for 40 wt thread, closer for finer thread
/// ([`thread_scale`]), or [`Params::density`] when auto spacing is off.
pub fn satin(path: &BezPath, p: &Params) -> Vec<Stitch> {
    satin_from(path, p, None)
}

/// [`satin`], the needle at `from` before it starts: each column starts at its end nearest the
/// needle (a closed one at its point nearest), the nearest first, as Ink/Stitch starts satins.
pub(super) fn satin_from(path: &BezPath, p: &Params, from: Option<Point>) -> Vec<Stitch> {
    let mut p = p.sane();
    p.style = Style::Satin;
    if p.width <= 0.0 {
        return Vec::new();
    }
    // Pull compensation widens the column at each rail ([`Params::compensation`]): the left one as
    // the path is drawn, then the right one.
    let [left, right] = p.compensation(p.width);
    let half = p.width / 2.0;
    let spacing = if p.auto_spacing { satin_spacing(p.width) * thread_scale(p.thread_weight) } else { p.density };
    let mut runs = Runs::from(from);
    for Line { pts, closed, reversed } in nearest_order(subpaths(path), from) {
        // Push compensation: an open column is sewn shorter at its ends, the underlay with it.
        let pts = if closed { pts } else { pushed(&pts, if reversed { [p.push[1], p.push[0]] } else { p.push }) };
        if length(&pts, closed) < p.density * MIN_CROSSINGS {
            continue;
        }
        // The first run of each column is reached as any move is; the column's later runs
        // (underlay to top stitching) start inside the column, so the needle sews there.
        let mut travel = Travel::Auto;
        // The underlay, by the column's width; it may end at the column's far end, and then the
        // top stitching starts there and comes back.
        let mut back = false;
        if p.underlay {
            for (under, ends_far) in satin_underlay(&pts, closed, p.width) {
                if under.len() >= 2 {
                    push_split(&mut runs, under, travel, p.max_satin);
                    travel = Travel::Sew;
                }
                back = ends_far;
            }
        }
        let mut samples = resample(&pts, spacing, closed);
        if samples.len() < 2 {
            continue;
        }
        let back = back && !closed;
        if back {
            samples.reverse();
        }
        // Whether the column is sewn against the way it is drawn, so its rails swap sides.
        let against = reversed != back;
        let crossings: Vec<(Point, Vec2)> = (0..samples.len())
            .filter_map(|i| {
                let q = *samples.get(i)?;
                let t = tangent(&samples, i, closed);
                // Even crossings go to one rail, odd ones to the other. The normal points to the
                // right of the way the needle goes along the column (y runs down the page).
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                let reach = half + if (side > 0.0) != against { right } else { left };
                Some((q, Vec2::new(-t.y, t.x) * reach * side))
            })
            .collect();
        let mut zigzag = short_stitches(&crossings, spacing, p.width + left + right);
        // A closed column ends where it started: the last crossing takes the remaining side.
        if closed
            && let (Some(first), Some(last)) = (zigzag.first().copied(), zigzag.last().copied())
            && first.distance(last) > p.min_stitch
        {
            zigzag.push(first);
        }
        push_split(&mut runs, zigzag, travel, p.max_satin);
        if runs.full() {
            break;
        }
    }
    runs.join(&p)
}

/// An open column's centreline `pts` with push compensation: `push` millimetres cut off its start
/// and its end, both in proportion when together they would leave less than [`MIN_PUSHED`] of it.
fn pushed(pts: &[Point], push: [f64; 2]) -> Vec<Point> {
    let [start, end] = push;
    if start + end <= 0.0 {
        return pts.to_vec();
    }
    let most = length(pts, false) * (1.0 - MIN_PUSHED);
    let k = if start + end > most { most / (start + end) } else { 1.0 };
    trimmed(pts, start * k, end * k)
}

/// Short stitches: how far a bunched penetration may move in from its rail, as shares of the
/// column's width, tried in order. Ink/Stitch's satin columns take a list of insets like this
/// ("short stitch inset", <https://inkstitch.org/docs/stitches/satin-column/>), and Hatch shortens
/// stitches on a satin's inner edge to reduce bunching (stitch shortening,
/// <https://hatch.embroideryhelp.net/v3/en/downloads/UserGuide.pdf>); 15–25 % is the usual
/// shortening. Estimates.
const SHORT_INSETS: [f64; 3] = [0.0, 0.2, 0.4];
/// How many earlier penetrations on the same rail a penetration keeps its distance from.
const SHORT_LOOKBACK: usize = 4;

/// A satin column's needle points from its `crossings` (each a point on the centreline and the
/// offset to the rail it goes to, the rails alternating), with short stitches where a curve bunches
/// the penetrations on its inner side. A penetration closer than `spacing` (the distance between
/// threads) to one of the last [`SHORT_LOOKBACK`] on its rail moves in by the first of
/// [`SHORT_INSETS`] (shares of the column's `width`, never past its centre) that keeps it that far
/// from them all, or, when none does, by the one that keeps it farthest. On a gentle curve that
/// shortens every other stitch on the inner side; on a tight one the penetrations spread over the
/// insets; along a straight column nothing moves.
fn short_stitches(crossings: &[(Point, Vec2)], spacing: f64, width: f64) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(crossings.len());
    for (i, &(q, offset)) in crossings.iter().enumerate() {
        // The earlier penetrations on this rail: every other one back.
        let before: Vec<Point> = (1..=SHORT_LOOKBACK).filter_map(|k| i.checked_sub(2 * k).and_then(|j| out.get(j)).copied()).collect();
        let clearance = |r: Point| before.iter().map(|b| b.distance(r)).fold(f64::INFINITY, f64::min);
        // An inset is a share of the whole width, taken off this side's offset.
        let reach = offset.hypot();
        let inward = unit(offset).unwrap_or(Vec2::ZERO);
        let candidates = SHORT_INSETS.iter().map(|inset| q + offset - inward * (inset * width).min(reach));
        let mut best: Option<(Point, f64)> = None;
        for r in candidates {
            let c = clearance(r);
            if c >= spacing {
                best = Some((r, c));
                break;
            }
            if best.is_none_or(|(_, b)| c > b) {
                best = Some((r, c));
            }
        }
        out.push(best.map_or(q + offset, |(r, _)| r));
    }
    out
}

/// `run` added to a column's `runs`, its stitches split to at most `max` ([`split_long`]). A run
/// sewn on from the last (`Travel::Sew`: from the underlay to the next layer) is split together
/// with the stitch that leads into it, which can cross the whole column.
fn push_split(runs: &mut Runs, run: Vec<Point>, travel: Travel, max: f64) {
    match (travel, runs.here()) {
        (Travel::Sew, Some(here)) => {
            let mut joined = Vec::with_capacity(run.len() + 1);
            joined.push(here);
            joined.extend(run);
            runs.push(split_long(&joined, max).into_iter().skip(1).collect(), travel);
        }
        _ => runs.push(split_long(&run, max), travel),
    }
}

/// The spacing of a satin column's crossings for its `width` (auto spacing): more open in a narrow
/// column, where the thread bunches at the rails and covers anyway, denser in a wide one so it
/// covers. A stepped table, as professional software keeps one; the values are estimates.
pub fn satin_spacing(width: f64) -> f64 {
    SPACING_BY_WIDTH.iter().find(|(w, _)| width < *w).map_or(WIDE_SPACING, |(_, spacing)| *spacing)
}

/// The underlay of a satin column along `pts` (its centreline), `width` wide, by width: a centre
/// run for a narrow column, an edge run (along one side and back along the other) for a medium one,
/// and an edge run then a zigzag between its sides for a wide one. Each run comes with whether it
/// ends at the column's far end. The thresholds are estimates from the usual practice. (The
/// zigzag's stitches run at an angle across the column, longer than its width, and the edge run
/// crosses the column at its far end: the caller splits them like the top stitching, as
/// Ink/Stitch's underlay guide recommends; an unbounded zigzag destabilises the column.)
fn satin_underlay(pts: &[Point], closed: bool, width: f64) -> Vec<(Vec<Point>, bool)> {
    let centre = resample(pts, UNDERLAY_STITCH, closed);
    if width < SATIN_EDGE_UNDERLAY {
        return vec![(centre, !closed)];
    }
    // The sides, kept inside the column so the underlay doesn't show at its edges.
    let side = (width / 2.0 - (width * UNDERLAY_INSET).min(UNDERLAY_INSET_MAX)).max(0.0);
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

/// `zigzag`'s crossings longer than `max` split into shorter stitches (auto split). A long crossing
/// is cut into `n` parts: the cuts sit `1/n` of it apart, all shifted by a fraction (at most
/// `JITTER` of a part either way) that changes from one crossing to the next (the golden ratio's
/// sequence), so the splits of neighbouring crossings never line up into a seam down the column.
/// The inner parts are `1/n` long and the two end parts at most `(1 + JITTER)/n`, so `n` is chosen
/// to keep that under `max`.
fn split_long(zigzag: &[Point], max: f64) -> Vec<Point> {
    /// How far the cuts move along a crossing, as a share of one part (either way).
    const JITTER: f64 = 0.25;
    let mut out: Vec<Point> = Vec::with_capacity(zigzag.len());
    let mut crossing = 0u32;
    for (i, &q) in zigzag.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).and_then(|j| zigzag.get(j)) {
            let len = p.distance(q);
            if len.is_finite() && len > max {
                // The longest part, an end one, is (1 + JITTER)/n of the crossing: enough parts that
                // it fits.
                let n = ((len * (1.0 + JITTER)) / max).ceil().clamp(2.0, MAX_SPLIT) as u32;
                crossing += 1;
                // A fraction from 0 to 1, centred on 0 (-1 to 1), times the jitter.
                let shift = ((f64::from(crossing) * GOLDEN).fract() - 0.5) * 2.0 * JITTER;
                out.extend((1..n).map(|k| p.lerp(q, (f64::from(k) + shift) / f64::from(n))));
            }
        }
        out.push(q);
    }
    out
}
