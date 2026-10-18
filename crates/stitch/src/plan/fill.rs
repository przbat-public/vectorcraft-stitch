//! Tatami fill ([`Style::Fill`](super::Style::Fill)): rows of stitches across a shape, sewn region
//! by region over an edge run and, for a large shape, a sparse tatami underlay.

use vectorcraft_geom::{BezPath, FillRule, Point};

use super::area::{Area, TRAVEL_STITCH};
use super::geometry::{Axes, SubPath, area, length, resample, subpaths};
use super::runs::{Runs, Travel};
use super::{MAX_STITCHES, Params, UNDERLAY_STITCH};
use crate::Stitch;

/// The smallest area (square millimetres) a subpath must enclose for a fill to treat it as a
/// shape rather than a line.
const MIN_AREA: f64 = 0.01;
/// A large fill's second underlay, sparse tatami rows across the top ones: the spacing of its rows
/// and its stitch length (millimetres), how far inside the edge it stays, and the smallest fill
/// (both ways, millimetres) that gets it. Estimates.
const TATAMI_UNDERLAY_SPACING: f64 = 2.0;
const TATAMI_UNDERLAY_STITCH: f64 = 3.0;
const TATAMI_UNDERLAY_INSET: f64 = 0.6;
const TATAMI_UNDERLAY_MIN: f64 = 10.0;
/// The longest travel run in a fill's top stitching (millimetres), where it may show: a longer
/// way lifts the needle instead. Travels under the top stitching (in the underlay) have no limit:
/// the top stitching hides them. An estimate.
const MAX_TRAVEL: f64 = 25.0;
/// The most rows one layer may have (a guard against absurd parameters).
const MAX_ROWS: usize = 100_000;

/// A tatami fill of `path`: rows of stitches at [`Params::angle`] degrees, [`Params::density`]
/// apart, each split into stitches at most [`Params::stitch_length`] long, over an edge run and,
/// for a large shape, a tatami underlay when [`Params::underlay`] is on. A subpath that encloses
/// no area is a line: it is sewn as a running line.
pub fn fill(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    // A fill closes its subpaths, as paint does (SVG, PDF): a shape drawn without a final close
    // is still an area.
    let subs: Vec<SubPath> = subpaths(path)
        .into_iter()
        .map(|mut s| {
            s.closed = s.closed || (s.pts.len() >= 3 && area(&s.pts).abs() > MIN_AREA);
            s
        })
        .collect();
    let mut runs = Runs::default();
    for sub in subs.iter().filter(|s| !s.closed) {
        let line = sub.points();
        if length(&line, false) >= p.min_stitch {
            runs.push(resample(&line, p.stitch_length, false), Travel::Auto);
        }
    }
    let outlines: Vec<&SubPath> = subs.iter().filter(|s| s.closed && s.pts.len() >= 3).collect();
    if outlines.is_empty() {
        return runs.join(&p);
    }
    let shape = Area { outlines: &outlines, rule };
    if p.underlay {
        edge_runs(shape, &p, &mut runs);
        tatami_underlay(shape, &p, &mut runs);
    }
    let axes = Axes::new(p.angle);
    if let Some((lo, hi)) = shape.extent(axes.across)
        && hi - lo >= p.density * 0.5
    {
        let layer = Layer { axes, spacing: p.density, inset: 0.0 };
        let regions = layer.regions(shape, lo, hi);
        sew(&regions, shape, axes, MAX_TRAVEL, &p, &mut runs);
    }
    runs.join(&p)
}

/// An edge run along each outline, to hold the fabric under the fill: each starts at the corner
/// nearest the needle, and is reached by travelling inside the shape (hidden under the fill, so of
/// any length).
fn edge_runs(shape: Area<'_>, p: &Params, runs: &mut Runs) {
    for sub in shape.outlines {
        let start = runs.here().and_then(|h| {
            (0..sub.pts.len()).min_by(|&i, &j| {
                sub.pts.get(i).map_or(f64::INFINITY, |q| q.distance(h)).total_cmp(&sub.pts.get(j).map_or(f64::INFINITY, |q| q.distance(h)))
            })
        });
        let mut pts: Vec<Point> = sub.pts.iter().cycle().skip(start.unwrap_or(0)).take(sub.pts.len()).copied().collect();
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
        let under = resample(&pts, UNDERLAY_STITCH, true);
        let Some(&first) = under.first() else { continue };
        let travel = runs.approach(shape, first, f64::INFINITY, p);
        runs.push(under, travel);
    }
}

/// A large fill's second underlay: sparse tatami rows across the top rows' direction, kept a
/// little inside the edge so they never peek out.
fn tatami_underlay(shape: Area<'_>, p: &Params, runs: &mut Runs) {
    let under = Params { angle: p.angle + 90.0, density: TATAMI_UNDERLAY_SPACING, stitch_length: TATAMI_UNDERLAY_STITCH, ..*p }.sane();
    let axes = Axes::new(under.angle);
    let (Some(w), Some(h)) = (shape.extent(axes.along), shape.extent(axes.across)) else { return };
    if w.1 - w.0 < TATAMI_UNDERLAY_MIN || h.1 - h.0 < TATAMI_UNDERLAY_MIN {
        return;
    }
    let layer = Layer { axes, spacing: under.density, inset: TATAMI_UNDERLAY_INSET };
    let regions = layer.regions(shape, h.0, h.1);
    // Under the top stitching, a travel of any length is hidden.
    sew(&regions, shape, axes, f64::INFINITY, &under, runs);
}

/// One layer of rows: their direction, the distance between them, and how far they stay inside
/// the edge.
struct Layer {
    axes: Axes,
    spacing: f64,
    inset: f64,
}

impl Layer {
    /// The layer's rows across `shape` (whose extent across the rows is `lo` to `hi`), grouped into
    /// regions: a span joins the region of the previous row's span it overlaps when that is the
    /// only one it overlaps and it is the only span overlapping it; where the shape branches or
    /// merges (round a hole, into the arms of a letter) new regions start. Rows sit half a
    /// spacing inside the extent: no row lies on a tangent, and the thread's own width covers to
    /// the edge.
    fn regions(&self, shape: Area<'_>, lo: f64, hi: f64) -> Vec<Region> {
        let rows = (((hi - lo) / self.spacing).floor().max(1.0) as usize).min(MAX_ROWS);
        let overlap = |x: (f64, f64), y: (f64, f64)| x.0 < y.1 && y.0 < x.1;
        let mut out: Vec<Region> = Vec::new();
        // The regions the previous row's spans went to, with those spans.
        let mut open: Vec<(usize, (f64, f64))> = Vec::new();
        // Every row's position and spans, for the overlaps below.
        let mut all: Vec<(f64, Vec<(f64, f64)>)> = Vec::with_capacity(rows);
        for row in 0..rows {
            let t = lo + self.spacing * (row as f64 + 0.5);
            let spans: Vec<(f64, f64)> =
                shape.spans(self.axes, t).into_iter().map(|(a, b)| (a + self.inset, b - self.inset)).filter(|(a, b)| b > a).collect();
            all.push((t, spans.clone()));
            let mut next: Vec<(usize, (f64, f64))> = Vec::with_capacity(spans.len());
            for &span in &spans {
                let mut touching = open.iter().filter(|(_, prev)| overlap(*prev, span));
                let joined = match (touching.next(), touching.next()) {
                    (Some(&(r, prev)), None) if spans.iter().filter(|s| overlap(prev, **s)).count() == 1 => Some(r),
                    _ => None,
                };
                let r = joined.unwrap_or_else(|| {
                    out.push(Region::default());
                    out.len() - 1
                });
                if let Some(region) = out.get_mut(r) {
                    region.rows.push(Row { index: row, t, a: span.0, b: span.1 });
                }
                next.push((r, span));
            }
            open = next;
        }
        // Where a region meets another (round a hole, at a branch), it takes one more row of its
        // neighbour's, cut to the stretch the two share: the fabric's pull would otherwise open a
        // gap along the seam.
        for region in &mut out {
            for after in [false, true] {
                let end = if after { region.rows.last() } else { region.rows.first() };
                let Some(&Row { index, a, b, .. }) = end else { continue };
                let Some(next) = (if after { index.checked_add(1) } else { index.checked_sub(1) }) else { continue };
                let Some((t, spans)) = all.get(next) else { continue };
                // The neighbouring span that shares the most with this end.
                let shared =
                    spans.iter().map(|s| (s.0.max(a), s.1.min(b))).filter(|(x, y)| y - x > 1e-9).max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)));
                if let Some((x, y)) = shared {
                    let row = Row { index: next, t: *t, a: x, b: y };
                    if after {
                        region.rows.push(row);
                    } else {
                        region.rows.insert(0, row);
                    }
                }
            }
        }
        out
    }
}

/// One row's stretch inside the shape: the row's number (for the stagger), where it sits across
/// the rows, and where the stretch starts and ends along them.
#[derive(Clone, Copy, Debug)]
struct Row {
    index: usize,
    t: f64,
    a: f64,
    b: f64,
}

/// A stretch of rows that follow one another without the shape branching: each row has one span,
/// and it overlaps the previous row's.
#[derive(Default)]
struct Region {
    rows: Vec<Row>,
}

impl Region {
    /// Where sewing the region starts: its first row (its last when `descending`), from its start
    /// when `forward`, else from its end.
    fn start(&self, descending: bool, forward: bool, axes: Axes) -> Option<Point> {
        let row = if descending { self.rows.last()? } else { self.rows.first()? };
        Some(axes.point(if forward { row.a } else { row.b }, row.t))
    }

    /// The region's rows as runs: alternating direction, the first `forward`, last row first when
    /// `descending`.
    fn runs(&self, descending: bool, forward: bool, axes: Axes, p: &Params) -> Vec<Vec<Point>> {
        let mut out: Vec<Vec<Point>> = Vec::with_capacity(self.rows.len());
        let mut forward = forward;
        let order: Vec<&Row> = if descending { self.rows.iter().rev().collect() } else { self.rows.iter().collect() };
        for row in order {
            // The stagger walks a quarter stitch at a time through a cycle of four rows: no two
            // rows line their penetrations up into a groove.
            let phase = 0.25 * (row.index % 4) as f64;
            let along = row_points(row.a, row.b, phase, p);
            if along.len() < 2 {
                continue;
            }
            let mut pts: Vec<Point> = along.iter().map(|s| axes.point(*s, row.t)).collect();
            if !forward {
                pts.reverse();
            }
            // Skip the last stitch of the row: the needle turns diagonally into the next row
            // instead of crawling one row spacing along the edge — when the stitch it leaves out
            // is short, so the turn stays a short stitch too.
            if let [.., prev, last] = pts.as_slice()
                && pts.len() > 2
                && prev.distance(*last) <= 0.5 * p.stitch_length
            {
                pts.pop();
            }
            out.push(pts);
            forward = !forward;
        }
        out
    }
}

/// The regions of one layer sewn after `runs`: each region whole, row after row; the next is the
/// one that starts nearest where the last one ended, sewn from whichever of its ends and sides is
/// closer. Moves into a region travel inside `shape` (at most `max_travel` long) or lift the
/// needle.
fn sew(regions: &[Region], shape: Area<'_>, axes: Axes, max_travel: f64, p: &Params, runs: &mut Runs) {
    let mut pending: Vec<&Region> = regions.iter().collect();
    while !pending.is_empty() && !runs.full() {
        let here = runs.here();
        let mut best: Option<(f64, usize, bool, bool)> = None;
        for (i, region) in pending.iter().enumerate() {
            for descending in [false, true] {
                for forward in [true, false] {
                    let Some(start) = region.start(descending, forward, axes) else { continue };
                    let d = here.map_or(0.0, |h| h.distance(start));
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, i, descending, forward));
                    }
                }
            }
        }
        let Some((_, i, descending, forward)) = best else { break };
        let region = pending.swap_remove(i);
        let sewn = region.runs(descending, forward, axes, p);
        let Some(first) = sewn.first().and_then(|r| r.first().copied()) else { continue };
        let into = runs.approach(shape, first, max_travel, p);
        for (k, run) in sewn.into_iter().enumerate() {
            let travel = match (k, runs.here(), run.first()) {
                (0, ..) => into,
                // A turn into the region's next row is sewn while it stays inside the shape. Along
                // a concave edge (the flat top of a round hole) it would cut across the hole: then
                // it travels round, or lifts, unless it only shaves a corner.
                (_, Some(h), Some(&f)) if h.distance(f) > 1.0 && !shape.contains_segment(h, f) => runs.approach(shape, f, max_travel, p),
                // A long turn (where the edge runs nearly along the rows) would be one long stitch
                // lying loose on top: it travels in short stitches instead.
                (_, Some(h), Some(&f)) if h.distance(f) > p.stitch_length => {
                    runs.push(resample(&[h, f], TRAVEL_STITCH, false), Travel::Sew);
                    Travel::Sew
                }
                _ => Travel::Sew,
            };
            runs.push(run, travel);
        }
    }
}

/// The positions along a row (from `a` to `b`) that get a needle penetration: both ends, and
/// between them a grid every [`Params::stitch_length`], shifted by `phase` (a share of a stitch).
/// The grid is measured along the rows' direction, not from the row's start, so the penetrations
/// of neighbouring rows — in any region — fall into one pattern (the stagger), and no stitch is
/// longer than a stitch length; only a row's first and last stitches are shorter. A grid point
/// within [`Params::min_stitch`] of an end is left out, so the end stitch isn't a stub (and is then
/// at most that much longer).
fn row_points(a: f64, b: f64, phase: f64, p: &Params) -> Vec<f64> {
    let len = b - a;
    // A row shorter than two row spacings (or 0.8 mm) has no room for a fill: leave it out.
    let shortest = (2.0 * p.density).max(0.8);
    if !len.is_finite() || len < shortest || len < p.min_stitch {
        return Vec::new();
    }
    let step = p.stitch_length;
    let mut out: Vec<f64> = vec![a];
    // The first grid point past `a`.
    let mut g = ((a / step - phase).floor() + 1.0 + phase) * step;
    while g < b - p.min_stitch && out.len() < MAX_STITCHES {
        if g - a >= p.min_stitch {
            out.push(g);
        }
        g += step;
    }
    out.push(b);
    out
}
