//! The geometry stitch planning works on: paths flattened to polylines, points spaced along them,
//! and the axes a fill's rows are laid along.

use std::f64::consts::TAU;

use vectorcraft_geom::{BezPath, PathEl, Point, Vec2, kurbo};

use super::MAX_STITCHES;

/// How far curves are flattened before stitching, millimetres: far below the 0.1 mm a machine
/// counts in.
const FLATTEN: f64 = 0.05;
/// A turn sharper than this (radians, about 30°) is a corner the stitches keep.
const CORNER: f64 = 0.52;

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
            && a.distance(b) > 1e-9
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
/// beside it. No stitch at a corner or at the end is a stub: a point that would fall within 0.3 of
/// a step of one moves onto it.
pub(super) fn resample(pts: &[Point], step: f64, closed: bool) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
    let step = step.max(0.01);
    let mut out: Vec<Point> = vec![first];
    let mut travelled = 0.0;
    let mut next = step;
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let at = |i: usize| pts.get(if i >= n { i - n } else { i }).copied();
    for i in 0..segs {
        let (Some(a), Some(b)) = (at(i), at(i + 1)) else { continue };
        let seg = a.distance(b);
        if seg <= 1e-12 {
            continue;
        }
        while next <= travelled + seg {
            let f = (next - travelled) / seg;
            out.push(a + (b - a) * f);
            next += step;
            if out.len() >= MAX_STITCHES {
                return out;
            }
        }
        travelled += seg;
        // A sharp corner at `b` (not the polyline's end): stitch into it, and count the next step
        // from it.
        let has_next = i + 1 < segs;
        if has_next
            && let Some(c) = at(i + 2)
            && let (Some(u), Some(v)) = (unit(b - a), unit(c - b))
            && u.dot(v).clamp(-1.0, 1.0).acos() > CORNER
        {
            land_on(&mut out, b, step);
            next = travelled + step;
        }
    }
    land_on(&mut out, if closed { first } else { last }, step);
    out
}

/// `q` as the next point of `out`, unless the last point is so close to it (within 0.3 of `step`)
/// that the stitch would be a stub: then that point moves onto `q`.
fn land_on(out: &mut Vec<Point>, q: Point, step: f64) {
    let movable = out.len() > 1;
    match out.last_mut() {
        Some(last) if last.distance(q) <= 1e-9 => {}
        Some(last) if movable && last.distance(q) < 0.3 * step => *last = q,
        _ => out.push(q),
    }
}

/// The total length of a polyline, millimetres.
pub(super) fn length(pts: &[Point], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs).filter_map(|i| Some(pts.get(i)?.distance(*pts.get((i + 1) % n)?))).filter(|d| d.is_finite()).sum()
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
    if t.hypot() < 1e-9
        && let Some(q) = next.and_then(|j| pts.get(j))
        && let Some(p) = pts.get(i)
    {
        t = *q - *p;
    }
    unit(t).unwrap_or(Vec2::new(1.0, 0.0))
}

/// `v` scaled to length one, `None` when it has no length.
pub(super) fn unit(v: Vec2) -> Option<Vec2> {
    let len = v.hypot();
    if len.is_finite() && len > 1e-12 { Some(v / len) } else { None }
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

/// The subpaths of a line (a running line, a satin's centreline) in the order a needle starting at
/// `here` sews them: each time the nearest one, from its end nearest the needle (a closed one from
/// its point nearest the needle), as its points ([`SubPath::points`]) and whether it is closed.
/// Unknown `here` keeps the drawing's order and directions; so does a path of more than
/// [`MAX_REORDERED`] subpaths, beyond its first, a bound on the work.
pub(super) fn nearest_order(subs: Vec<SubPath>, here: Option<Point>) -> Vec<(Vec<Point>, bool)> {
    /// The most subpaths ordered by distance.
    const MAX_REORDERED: usize = 64;
    let reorder = subs.len() <= MAX_REORDERED;
    let mut pending: Vec<SubPath> = subs;
    let mut out: Vec<(Vec<Point>, bool)> = Vec::with_capacity(pending.len());
    let mut here = here;
    while !pending.is_empty() {
        let at = |s: &SubPath| here.map_or(0.0, |h| s.nearest(h).1);
        let i = if reorder { pending.iter().enumerate().min_by(|a, b| at(a.1).total_cmp(&at(b.1))).map_or(0, |(i, _)| i) } else { 0 };
        let sub = pending.remove(i);
        let pts = match here {
            Some(h) if out.is_empty() || reorder => sub.starting_near(h),
            _ => sub.points(),
        };
        here = pts.last().copied();
        out.push((pts, sub.closed));
    }
    out
}

impl SubPath {
    /// The point to start from nearest `q` (an end of an open polyline, any point of a closed
    /// one): its index and distance.
    fn nearest(&self, q: Point) -> (usize, f64) {
        let d = |i: usize| self.pts.get(i).map_or(f64::INFINITY, |p| p.distance(q));
        if self.closed {
            (0..self.pts.len()).map(|i| (i, d(i))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, f64::INFINITY))
        } else {
            let last = self.pts.len().saturating_sub(1);
            if d(last) < d(0) { (last, d(last)) } else { (0, d(0)) }
        }
    }

    /// The points ([`SubPath::points`]) starting at the one nearest `q`: an open polyline reversed
    /// when its end is nearer, a closed one turned to start there.
    fn starting_near(&self, q: Point) -> Vec<Point> {
        let (i, _) = self.nearest(q);
        if i == 0 {
            return self.points();
        }
        if !self.closed {
            let mut v = self.pts.clone();
            v.reverse();
            return v;
        }
        let n = self.pts.len();
        SubPath { pts: (0..n).filter_map(|k| self.pts.get((i + k) % n).copied()).collect(), closed: true }.points()
    }
}
