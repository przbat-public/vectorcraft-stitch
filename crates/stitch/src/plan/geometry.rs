//! The geometry stitch planning works on: paths flattened to polylines, points spaced along them,
//! and the axes a fill's rows are laid along.

use std::f64::consts::TAU;

use vectorcraft_geom::{BezPath, PathEl, Point, Vec2, kurbo};

use super::{MAX_STITCHES, Start};
use crate::SAME_POINT;

/// How far curves are flattened before stitching, millimetres: far below the 0.1 mm a machine
/// counts in.
const FLATTEN: f64 = 0.05;
/// A turn sharper than this (radians, about 30°) is a corner the stitches keep. An estimate.
pub(super) const CORNER: f64 = 0.52;
/// A vector or segment shorter than this (millimetres) has no direction: dividing by its length
/// would give noise. A guard on that arithmetic rather than a geometric choice, so it sits well
/// below [`SAME_POINT`]: anything it rejects is the same point anyway.
pub(super) const NO_LENGTH: f64 = 1e-12;
/// The shortest step [`resample`] takes (millimetres): a bound on the points it makes.
const MIN_STEP: f64 = 0.01;
/// How close to a corner or the end a point may fall, as a share of a step, before it moves onto
/// it rather than leave a stub of a stitch ([`resample`]). An estimate.
const STUB: f64 = 0.3;

/// The farthest a shape's control points may lie from the origin, millimetres: 1,500 m, just
/// beyond the document's whole range (`vectorcraft_doc::MAX_COORD`, 4,000,000 points, about
/// 1,411 m). A shape beyond it (from a malformed file) is refused before it is flattened: a curve's
/// flattened points grow with the square root of its size, without bound.
pub const MAX_COORD_MM: f64 = 1.5e6;

/// Whether every control point of `path` is a finite number within [`MAX_COORD_MM`] of the origin:
/// what may be flattened and planned.
pub fn in_range(path: &BezPath) -> bool {
    path.elements().iter().all(|el| {
        let pts: &[Point] = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => std::slice::from_ref(p),
            PathEl::QuadTo(a, b) => return [*a, *b].iter().all(|p| near(*p)),
            PathEl::CurveTo(a, b, c) => return [*a, *b, *c].iter().all(|p| near(*p)),
            PathEl::ClosePath => &[],
        };
        pts.iter().all(|p| near(*p))
    })
}

/// Whether `p` is finite and within [`MAX_COORD_MM`] of the origin on both axes.
fn near(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD_MM && p.y.abs() <= MAX_COORD_MM
}

/// A polyline: its points and whether it closes.
pub(super) struct SubPath {
    pub(super) pts: Vec<Point>,
    pub(super) closed: bool,
}

impl SubPath {
    /// The points, with a closed polyline's first point repeated at the end.
    pub(super) fn points(&self) -> Vec<Point> {
        let mut v = self.pts.clone();
        if self.closed
            && let (Some(&a), Some(&b)) = (v.first(), v.last())
            && a.distance(b) > SAME_POINT
        {
            v.push(a);
        }
        v
    }
}

/// `path`'s curves as polylines, within [`FLATTEN`] of them.
pub(super) fn subpaths(path: &BezPath) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut cur: Option<SubPath> = None;
    kurbo::flatten(path.iter(), FLATTEN, |el| match el {
        PathEl::MoveTo(p) => {
            if let Some(s) = cur.take()
                && s.pts.len() >= 2
            {
                out.push(s);
            }
            cur = Some(SubPath { pts: vec![p], closed: false });
        }
        PathEl::LineTo(p) => match &mut cur {
            Some(s) => s.pts.push(p),
            None => cur = Some(SubPath { pts: vec![p], closed: false }),
        },
        PathEl::ClosePath => {
            if let Some(s) = &mut cur {
                s.closed = true;
            }
        }
        _ => {}
    });
    if let Some(s) = cur
        && s.pts.len() >= 2
    {
        out.push(s);
    }
    out
}

/// Points along a polyline every `step` millimetres, both ends included. A closed polyline ends
/// where it started. Sharp corners get a point of their own (the steps start again from them),
/// so stitches follow the outline instead of cutting across a corner — into a notch or a gap
/// beside it. No stitch at a corner is a stub: a point that would fall within [`STUB`] of a step of
/// one moves onto it, and so does one by the end. A point moves once at most: a corner that close to
/// the start or to a corner already stitched into is passed, the stitch at most [`STUB`] of a step
/// off it (along a fine zigzag the stitches keep their length rather than one point moving from corner
/// to corner), and no stitch is longer than `1 + STUB` steps.
pub(super) fn resample(pts: &[Point], step: f64, closed: bool) -> Vec<Point> {
    resample_by(pts, closed, &|_| step).into_iter().map(|(q, _)| q).collect()
}

/// [`resample`] with a step that may change along the line: `step(s)` is the step on from a point
/// `s` millimetres along it (a satin's crossings come closer round a bend). Each point comes with
/// how far along the line it lies.
pub(super) fn resample_by(pts: &[Point], closed: bool, step: &dyn Fn(f64) -> f64) -> Vec<(Point, f64)> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
    let step = |s: f64| step(s).max(MIN_STEP);
    let mut out: Vec<(Point, f64)> = vec![(first, 0.0)];
    // The last point stays where it is: the start, or a corner stitched into.
    let mut fixed = true;
    let mut travelled = 0.0;
    let mut next = step(0.0);
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let at = |i: usize| pts.get(if i >= n { i - n } else { i }).copied();
    // The step on from the last point.
    let step_on = |out: &[(Point, f64)]| step(out.last().map_or(0.0, |&(_, s)| s));
    for i in 0..segs {
        let (Some(a), Some(b)) = (at(i), at(i + 1)) else { continue };
        let seg = a.distance(b);
        if seg <= NO_LENGTH {
            continue;
        }
        while next <= travelled + seg {
            let f = (next - travelled) / seg;
            out.push((a + (b - a) * f, next));
            fixed = false;
            next += step(next);
            if out.len() >= MAX_STITCHES {
                return out;
            }
        }
        travelled += seg;
        // A sharp corner at `b` (not the polyline's end): stitch into it, and count the next step
        // from it.
        let has_next = i + 1 < segs;
        let last_step = step_on(&out);
        if has_next
            && let Some(c) = at(i + 2)
            && let (Some(u), Some(v)) = (unit(b - a), unit(c - b))
            && turn(u, v) > CORNER
            && land_on(&mut out, (b, travelled), last_step, fixed, false)
        {
            fixed = true;
            next = travelled + step(travelled);
        }
    }
    let last_step = step_on(&out);
    land_on(&mut out, (if closed { first } else { last }, travelled), last_step, fixed, true);
    out
}

/// Stitch into `q`, a corner or (`end`) the line's end, with how far along it lies: a point of its
/// own, unless the last point is so close to it (within [`STUB`] of `step`, the step on from that
/// point) that the stitch would be a stub: then that point moves onto `q`. When the last point must
/// stay (`fixed`), a corner that close is passed and the end gets its short stitch. Whether `q` got
/// a point.
fn land_on(out: &mut Vec<(Point, f64)>, q: (Point, f64), step: f64, fixed: bool, end: bool) -> bool {
    let Some(last) = out.last_mut() else {
        out.push(q);
        return true;
    };
    let d = last.0.distance(q.0);
    if d <= SAME_POINT {
        return true;
    }
    if d < STUB * step {
        if !fixed {
            *last = q;
            return true;
        }
        if !end {
            return false;
        }
    }
    out.push(q);
    true
}

/// The total length of a polyline, millimetres.
pub(super) fn length(pts: &[Point], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs).filter_map(|i| Some(pts.get(i)?.distance(*pts.get((i + 1) % n)?))).filter(|d| d.is_finite()).sum()
}

/// An open polyline with `start` millimetres cut off its start and `end` off its end; nothing left
/// (one point at most) when the two meet.
pub(super) fn trimmed(pts: &[Point], start: f64, end: f64) -> Vec<Point> {
    let total = length(pts, false);
    let (from, to) = (start.max(0.0), total - end.max(0.0));
    if !(from.is_finite() && to.is_finite()) || to - from <= NO_LENGTH {
        return Vec::new();
    }
    let mut out: Vec<Point> = Vec::new();
    let mut travelled = 0.0;
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        let seg = a.distance(*b);
        if !seg.is_finite() {
            continue;
        }
        let (lo, hi) = (travelled, travelled + seg);
        travelled = hi;
        if hi < from || lo > to || seg <= NO_LENGTH {
            continue;
        }
        let at = |d: f64| a.lerp(*b, ((d - lo) / seg).clamp(0.0, 1.0));
        if out.is_empty() {
            out.push(at(from.max(lo)));
        }
        // A cut on a point of the line lies in two segments: it is one point.
        let q = at(to.min(hi));
        if out.last().is_none_or(|last| last.distance(q) > SAME_POINT) {
            out.push(q);
        }
    }
    out
}

/// The signed area a closed polyline encloses (the shoelace formula), square millimetres.
pub(super) fn area(pts: &[Point]) -> f64 {
    let n = pts.len();
    (0..n).filter_map(|i| Some((*pts.get(i)?, *pts.get((i + 1) % n)?))).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() / 2.0
}

/// The unit tangent at point `i` of `pts`: the direction the line runs through it.
pub(super) fn tangent(pts: &[Point], i: usize, closed: bool) -> Vec2 {
    let n = pts.len();
    let prev = if i > 0 {
        Some(i - 1)
    } else if closed && n > 1 {
        Some(n - 1)
    } else {
        None
    };
    let next = if i + 1 < n {
        Some(i + 1)
    } else if closed && n > 1 {
        Some(0)
    } else {
        None
    };
    let mut t = Vec2::ZERO;
    if let (Some(p), Some(q)) = (prev.and_then(|j| pts.get(j)), next.and_then(|j| pts.get(j))) {
        t = *q - *p;
    }
    // At an end (or where the points either side are one): along the segment from the point, or
    // at the last point the one into it.
    if t.hypot() < SAME_POINT
        && let Some(q) = next.and_then(|j| pts.get(j))
        && let Some(p) = pts.get(i)
    {
        t = *q - *p;
    }
    if t.hypot() < SAME_POINT
        && let Some(q) = pts.get(i)
        && let Some(p) = prev.and_then(|j| pts.get(j))
    {
        t = *q - *p;
    }
    // A line that goes nowhere: any direction will do.
    unit(t).unwrap_or(Vec2::new(1.0, 0.0))
}

/// The angle between two unit directions `u` and `v` (radians, 0 to π): how sharply a line turns
/// from one to the other.
pub(super) fn turn(u: Vec2, v: Vec2) -> f64 {
    u.dot(v).clamp(-1.0, 1.0).acos()
}

/// `v` scaled to length one, `None` when it has no length.
pub(super) fn unit(v: Vec2) -> Option<Vec2> {
    let len = v.hypot();
    if len.is_finite() && len > NO_LENGTH { Some(v / len) } else { None }
}

/// The two directions a fill's rows are laid along: `along` (the row direction) and `across`
/// (the direction the rows step in).
#[derive(Clone, Copy, Debug)]
pub(super) struct Axes {
    pub(super) along: Vec2,
    pub(super) across: Vec2,
}

impl Axes {
    /// The axes of rows at `angle_deg` degrees (0: left to right).
    pub(super) fn new(angle_deg: f64) -> Self {
        // Degrees to radians.
        let (s, c) = (angle_deg * TAU / 360.0).sin_cos();
        let (along, across) = (Vec2::new(c, s), Vec2::new(-s, c));
        if along.hypot().is_finite() && across.hypot().is_finite() { Self { along, across } } else { Self::new(0.0) }
    }

    /// Where `p` falls along the rows.
    pub(super) fn along(&self, p: Point) -> f64 {
        p.to_vec2().dot(self.along)
    }

    /// Where `p` falls across the rows (which row it is on).
    pub(super) fn across(&self, p: Point) -> f64 {
        p.to_vec2().dot(self.across)
    }

    /// The point `s` along and `t` across.
    pub(super) fn point(&self, s: f64, t: f64) -> Point {
        (self.along * s + self.across * t).to_point()
    }
}

/// The most subpaths of a line ordered by distance ([`nearest_order`], [`ordered`]): a bound on the
/// work.
const MAX_REORDERED: usize = 64;

/// The subpaths of a line (a running line, a satin's centreline) in the order a needle starting at
/// `here` sews them: each time the nearest one, from its end nearest the needle (a closed one from
/// its point nearest the needle), as [`Line`]s. Unknown `here` (nothing sewn before), the first
/// subpath is sewn as drawn and the others follow from where it ends. A path of more than
/// [`MAX_REORDERED`] subpaths keeps the drawing's order and directions beyond its first, a bound on
/// the work.
pub(super) fn nearest_order(subs: Vec<SubPath>, here: Option<Point>) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::with_capacity(subs.len());
    let mut here = here;
    if subs.len() > MAX_REORDERED {
        // In the drawing's order, taken one after another (not removed from the front, which
        // would shift the rest each time).
        for sub in subs {
            let (pts, reversed) = match here {
                Some(h) if out.is_empty() => sub.starting_near(h),
                _ => (sub.points(), false),
            };
            here = pts.last().copied();
            out.push(Line { pts, closed: sub.closed, reversed });
        }
        return out;
    }
    let mut pending: Vec<SubPath> = subs;
    while !pending.is_empty() {
        // The nearest (the first of equally near ones), each distance worked out once.
        let i = here.and_then(|h| pending.iter().map(|s| s.nearest(h).1).enumerate().min_by(|a, b| a.1.total_cmp(&b.1))).map_or(0, |(i, _)| i);
        let sub = pending.remove(i);
        let (pts, reversed) = match here {
            Some(h) => sub.starting_near(h),
            None => (sub.points(), false),
        };
        here = pts.last().copied();
        out.push(Line { pts, closed: sub.closed, reversed });
    }
    out
}

/// The subpaths of a line in the order they are sewn from `start` ([`Start`]): from an entry point
/// a person set, the subpath it lies nearest first, a closed one started exactly at the point of it
/// nearest the entry (an open one can start only at an end: its nearer one), then each time the
/// nearest one, as [`nearest_order`] goes on; from where the needle is, or nowhere, as
/// [`nearest_order`] orders them. Past [`MAX_REORDERED`] subpaths, the one the entry lies nearest
/// first and the rest in the drawing's order.
pub(super) fn ordered(subs: Vec<SubPath>, start: Start) -> Vec<Line> {
    let entry = match start {
        Start::Entry(e) if e.x.is_finite() && e.y.is_finite() => e,
        _ => return nearest_order(subs, start.point()),
    };
    let mut subs = subs;
    let Some(i) = subs.iter().map(|s| s.entered(entry).1).enumerate().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i) else {
        return Vec::new();
    };
    let first = subs.remove(i);
    let (pts, reversed) = first.starting_at(entry);
    let here = pts.last().copied();
    let mut out = vec![Line { pts, closed: first.closed, reversed }];
    if subs.len() + 1 > MAX_REORDERED {
        out.extend(subs.into_iter().map(|sub| Line { pts: sub.points(), closed: sub.closed, reversed: false }));
        return out;
    }
    out.extend(nearest_order(subs, here));
    out
}

/// The point of polyline `pts` (closed when `closed`) nearest `q`: the segment it lies on (from
/// point `i` to the next), the point, and its distance from `q`; `None` for no points.
pub(super) fn nearest_on(pts: &[Point], closed: bool, q: Point) -> Option<(usize, Point, f64)> {
    let n = pts.len();
    if n == 1 {
        let p = *pts.first()?;
        return Some((0, p, p.distance(q)));
    }
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .filter_map(|i| {
            let (a, b) = (*pts.get(i)?, *pts.get((i + 1) % n)?);
            let ab = b - a;
            let len2 = ab.hypot2();
            let t = if len2 > NO_LENGTH * NO_LENGTH { ((q - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let at = a + ab * t;
            let d = at.distance(q);
            d.is_finite().then_some((i, at, d))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// One subpath of a line as it is sewn ([`nearest_order`]): its points, whether it closes, and
/// whether it runs against the way it is drawn (an open one sewn from its end).
pub(super) struct Line {
    pub(super) pts: Vec<Point>,
    pub(super) closed: bool,
    pub(super) reversed: bool,
}

impl SubPath {
    /// The point to start from nearest `q` (an end of an open polyline, any point of a closed
    /// one): its index and distance.
    pub(super) fn nearest(&self, q: Point) -> (usize, f64) {
        let d = |i: usize| self.pts.get(i).map_or(f64::INFINITY, |p| p.distance(q));
        if self.closed {
            (0..self.pts.len()).map(|i| (i, d(i))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, f64::INFINITY))
        } else {
            let last = self.pts.len().saturating_sub(1);
            if d(last) < d(0) { (last, d(last)) } else { (0, d(0)) }
        }
    }

    /// Where it starts nearest an entry point `q`: an open polyline at its nearer end, a closed one
    /// at the point of it nearest `q` ([`nearest_on`]); its index, and how far that is from `q`.
    fn entered(&self, q: Point) -> (usize, f64) {
        if self.closed { nearest_on(&self.pts, true, q).map_or((0, f64::INFINITY), |(i, _, d)| (i, d)) } else { self.nearest(q) }
    }

    /// The points ([`SubPath::points`]) starting where it starts nearest an entry point `q`
    /// ([`SubPath::entered`]): an open polyline from its nearer end (reversed when that is its
    /// last), a closed one from the point of it nearest `q`, put in among its points unless it is
    /// one of them; and whether they were reversed.
    fn starting_at(&self, q: Point) -> (Vec<Point>, bool) {
        if !self.closed {
            return self.starting_near(q);
        }
        let Some((i, at, _)) = nearest_on(&self.pts, true, q) else { return (self.points(), false) };
        let n = self.pts.len();
        let mut pts: Vec<Point> = Vec::with_capacity(n + 2);
        pts.push(at);
        // From the segment's far end round to its near end; a point of the polyline the start
        // lies on is not put in twice.
        for k in 1..=n {
            if let Some(&p) = self.pts.get((i + k) % n)
                && p.distance(at) > SAME_POINT
            {
                pts.push(p);
            }
        }
        pts.push(at);
        (pts, false)
    }

    /// The points ([`SubPath::points`]) starting at the one nearest `q`: an open polyline reversed
    /// when its end is nearer, a closed one turned to start there; and whether they were reversed.
    fn starting_near(&self, q: Point) -> (Vec<Point>, bool) {
        let (i, _) = self.nearest(q);
        if i == 0 {
            return (self.points(), false);
        }
        if !self.closed {
            let mut v = self.pts.clone();
            v.reverse();
            return (v, true);
        }
        let n = self.pts.len();
        (SubPath { pts: (0..n).filter_map(|k| self.pts.get((i + k) % n).copied()).collect(), closed: true }.points(), false)
    }
}
