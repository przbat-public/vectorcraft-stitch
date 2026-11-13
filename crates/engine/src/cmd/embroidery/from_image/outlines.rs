//! The outlines a design may be given: a running line along every colour edge and round the
//! design, and a satin border round the design (the project owner's choice of 2026-11-11,
//! „Prosty + satyna z brzegu (polecam)”: both off by default; research note 06 §3.4).
//!
//! - **Each colour edge once**, by the area sewn later: once the area sewn first grows under it,
//!   the edge is that area's visible seam. Every curve piece of an area's own outline, as traced
//!   and before the overlap ([`super::overlap::Shapes::own`]), is asked which area lies across it,
//!   half a pixel out either side, as the overlap asks the region map, and kept when that one is
//!   sewn before it.
//! - **The silhouette**: the pieces with nothing sewn across them (the background, an area left
//!   unsewn, the image's edge), holes included. The colour outlines run along it too, unless the
//!   border does (each edge sewn once).
//! - **Chained** where the pieces meet, the straightest way on, so that where three areas meet one
//!   line runs through and the third piece goes on from it.
//!
//! A piece the points asked along it disagree on (near a corner the trace rounded) takes what lies
//! across the piece before it along the outline.

use std::collections::HashMap;

use vectorcraft_geom::kurbo::{BezPath, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathEl, PathSeg, Vec2};
use vectorcraft_geom::{PathData, Point};

use super::overlap::{BEYOND_PX, SAMPLES_ALONG};
use super::regions::{NONE, Regions};
use super::stages::Digitized;
use crate::Result;

/// How near the ends of two pieces lie for them to be chained (millimetres): half the formats'
/// 0.1 mm step (an estimate, research note 06), far over the hundredth of a pixel by which the
/// path operations may move the ends of an edge's two sides apart.
const MEET_MM: f64 = 0.05;

/// How exactly a piece's length is measured (working pixels): a thousandth of a pixel.
const LENGTH_ACCURACY_PX: f64 = 1e-3;

/// A run of an outline's curve pieces kept, in order, and whether it runs all round.
type Run = (Vec<PathSeg>, bool);

/// What lies across a piece of an area's outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Across {
    /// A sewn area, by its index.
    Area(usize),
    /// Nothing sewn: the background, an area left unsewn, the image's edge.
    Nothing,
}

/// One line of outline (working pixels): its path (closed when it closes on itself) and its length.
pub(super) struct Chain {
    pub(super) path: BezPath,
    pub(super) length_px: f64,
}

/// A digitized image's outlines (working pixels).
pub(super) struct Outlines {
    /// The colour outlines: each colour edge once, and the silhouette too when asked, chained.
    pub(super) colour: Vec<Chain>,
    /// The silhouette's loops, holes included, chained.
    pub(super) silhouette: Vec<Chain>,
    /// How long the colour outlines run beside an area sewn along its middle (working pixels).
    pub(super) beside_lines_px: f64,
}

impl Outlines {
    /// How long the colour outlines are (working pixels).
    pub(super) fn colour_px(&self) -> f64 {
        self.colour.iter().map(|c| c.length_px).sum()
    }
}

/// The label of the region map at `p` (working pixels): [`NONE`] past the image.
fn label_at(regions: &Regions, p: Point) -> u32 {
    if p.x >= 0.0 && p.y >= 0.0 && p.x.is_finite() && p.y.is_finite() { regions.at(p.x as u32, p.y as u32).unwrap_or(NONE) } else { NONE }
}

/// The label across curve piece `seg` of area `own`'s outline at `t`: the other side's, when one
/// side, half a pixel out, is `own`'s and the other isn't.
fn across_at(regions: &Regions, seg: &PathSeg, t: f64, own: u32) -> Option<u32> {
    let tangent = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    let len = tangent.hypot();
    if len.is_nan() || len <= 0.0 {
        return None;
    }
    let n = Vec2::new(-tangent.y, tangent.x) / len * BEYOND_PX;
    let m = seg.eval(t);
    match (label_at(regions, m + n), label_at(regions, m - n)) {
        (l, o) | (o, l) if l == own && o != own => Some(o),
        _ => None,
    }
}

/// What lies across each of `segs`, curve pieces of area `own`'s outline, as `across` names a
/// label: what two of [`SAMPLES_ALONG`] agree on; a piece they don't agree on takes the piece's
/// before it along the outline (round from its end), and an outline none is agreed on lies against
/// nothing sewn.
fn across_each(regions: &Regions, segs: &[PathSeg], own: u32, across: &dyn Fn(u32) -> Across) -> Vec<Across> {
    let found: Vec<Option<Across>> = segs
        .iter()
        .map(|seg| {
            let seen: Vec<Across> = SAMPLES_ALONG.iter().filter_map(|&t| across_at(regions, seg, t, own)).map(across).collect();
            seen.iter().copied().find(|x| seen.iter().filter(|y| *y == x).count() * 2 > SAMPLES_ALONG.len())
        })
        .collect();
    let Some(last) = found.iter().rev().flatten().next().copied() else { return vec![Across::Nothing; segs.len()] };
    let mut before = last;
    found
        .into_iter()
        .map(|f| {
            before = f.unwrap_or(before);
            before
        })
        .collect()
}

/// The runs of `segs` (an outline's curve pieces, round in order) that `kept` keeps, each of one
/// thing across it (a piece's key: `None`, not kept): each run's pieces in order, and whether it
/// runs all round. A run ends where what lies across changes, as at a junction of three areas, so
/// that the chaining meets every edge's end there.
fn runs(segs: &[PathSeg], kept: &[Option<Across>]) -> Vec<Run> {
    let n = segs.len().min(kept.len());
    let key = |i: usize| kept.get(i % n.max(1)).copied().flatten();
    if n == 0 {
        return Vec::new();
    }
    if key(0).is_some() && (0..n).all(|i| key(i) == key(0)) {
        return vec![(segs.iter().take(n).copied().collect(), true)];
    }
    // From a piece whose key differs from the one before it, so that no run is cut where the
    // outline starts.
    let first = (0..n).find(|&i| key(i) != key(i + n - 1)).unwrap_or(0);
    let mut out = Vec::new();
    let mut run: Vec<PathSeg> = Vec::new();
    let mut current = None;
    for i in (0..n).map(|k| (first + k) % n) {
        let here = key(i);
        if here != current {
            if !run.is_empty() {
                out.push((std::mem::take(&mut run), false));
            }
            current = here;
        }
        if let (Some(_), Some(seg)) = (here, segs.get(i)) {
            run.push(*seg);
        }
    }
    if !run.is_empty() {
        out.push((run, false));
    }
    out
}

/// The outlines of what `d` digitized ([`Outlines`]); the colour outlines along the silhouette too
/// when `with_silhouette`. A cancel stops it between areas.
pub(super) fn outlined(d: &Digitized, with_silhouette: bool) -> Result<Outlines> {
    let areas = d.areas();
    let regions = &areas.regions;
    let colours = d.colours();
    let lines = d.lines();
    let own_shapes = &d.shapes().own;
    let sewn = |a: usize| areas.areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(super::colours::Colour::sewn);
    let line = |a: usize| lines.get(a).is_some_and(Option::is_some);
    // Where each area lies in the sewing order.
    let mut place = vec![usize::MAX; areas.areas.len()];
    for (i, &a) in d.order().sewn.iter().enumerate() {
        if let Some(p) = place.get_mut(a) {
            *p = i;
        }
    }
    let across = |l: u32| match usize::try_from(l) {
        Ok(o) if l != NONE && sewn(o) => Across::Area(o),
        _ => Across::Nothing,
    };
    let (mut colour, mut rim): (Vec<Run>, Vec<Run>) = (Vec::new(), Vec::new());
    let mut beside_lines_px = 0.0;
    for &a in &d.order().sewn {
        vectorcraft_doc::cancel::check()?;
        let (true, Some(shape), Ok(own)) = (sewn(a), own_shapes.get(a), u32::try_from(a)) else { continue };
        let here = place.get(a).copied().unwrap_or(usize::MAX);
        for sp in &shape.subpaths {
            let segs: Vec<PathSeg> = PathData::new(vec![sp.clone()]).to_bezpath().segments().collect();
            let what = across_each(regions, &segs, own, &across);
            // Each piece kept by what lies across it: an area sewn before this one, or nothing sewn.
            let along: Vec<Option<Across>> = what
                .iter()
                .map(|&w| match w {
                    Across::Area(o) if place.get(o).is_some_and(|&p| p < here) => Some(w),
                    Across::Nothing if with_silhouette => Some(w),
                    _ => None,
                })
                .collect();
            let against_nothing: Vec<Option<Across>> = what.iter().map(|&w| (w == Across::Nothing).then_some(w)).collect();
            for ((seg, w), kept) in segs.iter().zip(&what).zip(&along) {
                if kept.is_some() && (line(a) || matches!(w, Across::Area(o) if line(*o))) {
                    beside_lines_px += seg.arclen(LENGTH_ACCURACY_PX);
                }
            }
            colour.extend(runs(&segs, &along));
            rim.extend(runs(&segs, &against_nothing));
        }
    }
    let meet_px = MEET_MM * d.working().px_per_mm;
    Ok(Outlines { colour: chained(colour, meet_px), silhouette: chained(rim, meet_px), beside_lines_px })
}

/// The way `seg` runs at its start, or at its end: its tangent there, else its chord (a curve
/// whose handle lies on its point).
fn heading(seg: &PathSeg, at_end: bool) -> Vec2 {
    let t = if at_end { 1.0 } else { 0.0 };
    let tangent = match seg {
        PathSeg::Line(l) => l.p1 - l.p0,
        PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
        PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
    };
    if tangent.hypot() > 0.0 { tangent } else { seg.end() - seg.start() }
}

/// The turn from heading `a` to heading `b` (radians, 0 to π); a full turn back for a heading of
/// no length.
fn turn(a: Vec2, b: Vec2) -> f64 {
    let (la, lb) = (a.hypot(), b.hypot());
    if la <= 0.0 || lb <= 0.0 || !(la * lb).is_finite() {
        return std::f64::consts::PI;
    }
    (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos()
}

/// `segs` run the other way.
fn reversed(segs: &[PathSeg]) -> Vec<PathSeg> {
    segs.iter().rev().map(PathSeg::reverse).collect()
}

/// The cell of a grid of `size` that point `p` lies in.
fn cell(p: Point, size: f64) -> (i64, i64) {
    ((p.x / size).floor() as i64, (p.y / size).floor() as i64)
}

/// The cells round a cell, and itself.
const AROUND: [(i64, i64); 9] = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 0), (0, 1), (1, -1), (1, 0), (1, 1)];

/// The root of `i` in the union-find `up` (each points at another of its set, a root at itself),
/// the paths walked halved on the way.
fn root(up: &mut [usize], mut i: usize) -> usize {
    while let Some(&next) = up.get(i).filter(|&&n| n != i) {
        let above = up.get(next).copied().unwrap_or(next);
        if let Some(slot) = up.get_mut(i) {
            *slot = above;
        }
        i = next;
    }
    i
}

/// `up`'s sets of `i` and `j` made one, under the lower root.
fn unite(up: &mut [usize], i: usize, j: usize) {
    let (a, b) = (root(up, i), root(up, j));
    if let Some(slot) = up.get_mut(a.max(b)) {
        *slot = a.min(b);
    }
}

/// `points` (each with what it is of) in sets of those within `meet` of each other, each set by the
/// lowest index in it.
fn near_sets(points: &[Point], meet: f64) -> Vec<usize> {
    let size = if meet.is_finite() && meet > 0.0 { meet } else { 1.0 };
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, &p) in points.iter().enumerate() {
        grid.entry(cell(p, size)).or_default().push(i);
    }
    let mut up: Vec<usize> = (0..points.len()).collect();
    for (i, &p) in points.iter().enumerate() {
        let (cx, cy) = cell(p, size);
        for (dx, dy) in AROUND {
            for &j in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                if j > i && points.get(j).is_some_and(|&q| (p - q).hypot() <= meet) {
                    unite(&mut up, i, j);
                }
            }
        }
    }
    (0..points.len()).map(|i| root(&mut up, i)).collect()
}

/// `runs` (pieces in order, and whether each runs all round) chained where their ends lie within
/// `meet_px` of each other: at each place where ends meet, the two that turn least from one into
/// the other are joined first, then the two that turn least of the rest, and so on, so that where
/// three areas meet the line runs straight through and the third piece goes on from it; the chains
/// are then followed from each end left free, and a ring of pieces all joined closes.
fn chained(runs: Vec<Run>, meet_px: f64) -> Vec<Chain> {
    let mut out = Vec::new();
    let mut open: Vec<Vec<PathSeg>> = Vec::new();
    for (segs, closed) in runs {
        match (closed, segs.is_empty()) {
            (_, true) => {}
            (true, false) => out.push(chain_of(&segs, true)),
            (false, false) => open.push(segs),
        }
    }
    // The ends: piece `i`'s start is end `2 i`, its end `2 i + 1`. The way one goes along the piece
    // arriving at an end, and leaving from it, the arrival turned round.
    let first = |e: usize| open.get(e / 2).and_then(|s| s.first());
    let last = |e: usize| open.get(e / 2).and_then(|s| s.last());
    let point = |e: usize| if e.is_multiple_of(2) { first(e).map(PathSeg::start) } else { last(e).map(PathSeg::end) };
    let arriving =
        |e: usize| if e.is_multiple_of(2) { first(e).map(|s| -heading(s, false)) } else { last(e).map(|s| heading(s, true)) }.unwrap_or_default();
    let ends: Vec<Point> = (0..2 * open.len()).map(|e| point(e).unwrap_or_default()).collect();
    let sets = near_sets(&ends, meet_px);
    let mut meeting: HashMap<usize, Vec<usize>> = HashMap::new();
    for (e, &set) in sets.iter().enumerate() {
        meeting.entry(set).or_default().push(e);
    }
    let mut places: Vec<Vec<usize>> = meeting.into_values().filter(|m| m.len() > 1).collect();
    places.sort_unstable();
    let mut partner: Vec<Option<usize>> = vec![None; ends.len()];
    for place in &places {
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (k, &a) in place.iter().enumerate() {
            for &b in place.iter().skip(k + 1) {
                pairs.push((turn(arriving(a), -arriving(b)), a, b));
            }
        }
        pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
        for (_, a, b) in pairs {
            if partner.get(a).is_some_and(Option::is_none) && partner.get(b).is_some_and(Option::is_none) {
                if let Some(slot) = partner.get_mut(a) {
                    *slot = Some(b);
                }
                if let Some(slot) = partner.get_mut(b) {
                    *slot = Some(a);
                }
            }
        }
    }
    // A chain from end `from`: along its piece to the piece's other end, on to the end joined to
    // that one, and so on, until an end left free or the piece it started with.
    let mut used = vec![false; open.len()];
    let mut walk = |from: usize| -> Vec<PathSeg> {
        let mut segs = Vec::new();
        let mut e = from;
        while let (Some(false), Some(piece)) = (used.get(e / 2).copied(), open.get(e / 2)) {
            if let Some(u) = used.get_mut(e / 2) {
                *u = true;
            }
            if e.is_multiple_of(2) {
                segs.extend(piece.iter().copied());
            } else {
                segs.extend(reversed(piece));
            }
            match partner.get(e ^ 1).copied().flatten() {
                Some(next) => e = next,
                None => break,
            }
        }
        segs
    };
    for e in 0..ends.len() {
        if partner.get(e).is_some_and(Option::is_none) {
            let segs = walk(e);
            if !segs.is_empty() {
                out.push(chain_of(&segs, false));
            }
        }
    }
    for i in 0..open.len() {
        let segs = walk(2 * i);
        if !segs.is_empty() {
            out.push(chain_of(&segs, true));
        }
    }
    out
}

/// A chain of `segs`, one after another from the first's start (each from where the one before
/// ended), closed when `closed`.
fn chain_of(segs: &[PathSeg], closed: bool) -> Chain {
    let mut path = BezPath::new();
    let mut length_px = 0.0;
    if let Some(first) = segs.first() {
        path.move_to(first.start());
    }
    for seg in segs {
        path.push(seg.as_path_el());
        length_px += seg.arclen(LENGTH_ACCURACY_PX);
    }
    if closed {
        path.close_path();
    }
    Chain { path, length_px }
}

/// The most lines put in order nearest first; past it, the rest keep the order they were chained
/// in. An estimate: each line put in order is compared with every line left, so 4,096 lines are
/// some eight million comparisons, a few milliseconds, against seconds for the rest of a design that
/// big.
const NEAREST_FIRST_MAX: usize = 4096;

/// `chains` in the order they are sewn, each an object of its own (the needle lifting from one to
/// the next): from the first, each time the one with an end nearest where the last ended (a closed
/// one ends where it starts), an open one turned round when its far end is the nearer; a tie goes
/// to the earlier chain, unturned. Each chain is measured by its ends alone; past
/// [`NEAREST_FIRST_MAX`] chains the rest keep their order.
pub(super) fn nearest_first(chains: Vec<Chain>) -> Vec<Chain> {
    let ends: Vec<(Point, Point, bool)> = chains.iter().map(ends_of).collect();
    let mut left: Vec<Option<Chain>> = chains.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(left.len());
    let mut at: Option<Point> = None;
    while out.len() < NEAREST_FIRST_MAX {
        let mut best: Option<(f64, usize, bool)> = None;
        for (i, (chain, &(start, end, closed))) in left.iter().zip(&ends).enumerate() {
            if chain.is_none() {
                continue;
            }
            let here = at.unwrap_or(start);
            let mut take = |d: f64, turn: bool| {
                if best.is_none_or(|(b, ..)| d < b) {
                    best = Some((d, i, turn));
                }
            };
            take((start - here).hypot(), false);
            if !closed {
                take((end - here).hypot(), true);
            }
        }
        let Some((_, i, turn)) = best else { break };
        let (Some(slot), Some(&(start, end, _))) = (left.get_mut(i), ends.get(i)) else { break };
        let Some(mut chain) = slot.take() else { break };
        if turn {
            chain.path = chain.path.reverse_subpaths();
        }
        at = Some(if turn { start } else { end });
        out.push(chain);
    }
    out.extend(left.into_iter().flatten());
    out
}

/// Where chain `c` starts and ends (working pixels), and whether it closes on itself (it then ends
/// where it starts).
fn ends_of(c: &Chain) -> (Point, Point, bool) {
    let closed = matches!(c.path.elements().last(), Some(PathEl::ClosePath));
    let start = c.path.segments().next().map_or(Point::ZERO, |s| s.start());
    let end = if closed { start } else { c.path.segments().last().map_or(start, |s| s.end()) };
    (start, end, closed)
}
