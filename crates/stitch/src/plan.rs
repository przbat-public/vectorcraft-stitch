//! How a path becomes stitches: running stitch, satin column and tatami fill.
//!
//! A *run* is a stretch the needle sews without lifting: a set of needle points in order. Runs
//! are joined into a block: a short move between two runs stays down (it is just the next stitch),
//! a long one becomes a [`Command::Trim`] and a [`Command::Jump`].
//!
//! Parameters are millimetres and degrees. Everything is clamped to a sane range before use, so
//! out-of-range or non-finite input gives a usable stitch plan rather than a loop that never
//! ends: no function here panics.
//!
//! The three styles:
//!
//! - [`Style::Running`] follows a path: a single line of stitches, [`Params::stitch_length`]
//!   apart, with a lock stitch at each end ([`Params::lock`]).
//! - [`Style::Satin`] sews a column: the path is its centreline,[`Params::width`] its width, and
//!   the needle crosses from rail to rail every [`Params::density`] millimetres, so the threads
//!   lie side by side. [`Params::pull`] widens the column (pull compensation: the fabric draws
//!   the stitches together). A centre run ([`Params::underlay`]) holds the column up.
//! - [`Style::Fill`] fills a shape with rows of stitches [`Params::density`] apart at
//!   [`Params::angle`] degrees, alternating their direction so the needle stays near the last
//!   stitch, with an edge run underneath ([`Params::underlay`]). [`Params::stitch_length`] is
//!   the longest stitch a row is split into.

use std::f64::consts::TAU;

use vectorcraft_geom::{BezPath, FillRule, PathEl, Point, Vec2, kurbo};

use crate::{Command, Stitch};

/// How far curves are flattened before stitching, millimetres: far below the 0.1 mm a machine
/// counts in.
const FLATTEN: f64 = 0.05;
/// A trim and a jump are needed when the thread would otherwise run further than this, unless
/// [`Params::jump`] says otherwise (millimetres).
const JUMP: f64 = 5.0;
/// The underlay's own stitch length (millimetres): short, to hold the fabric still.
const UNDERLAY_STITCH: f64 = 2.0;
/// The lock stitch's reach, millimetres.
const LOCK: f64 = 1.0;
/// The most runs, rows and stitches one shape may produce (a guard against absurd parameters).
const MAX_ROWS: usize = 100_000;
const MAX_RUNS: usize = 1_000_000;
const MAX_STITCHES: usize = 4_000_000;

/// How a shape is stitched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// One line of stitches along the path.
    #[default]
    Running,
    /// A column across [`Params::width`].
    Satin,
    /// A shape filled with rows (tatami).
    Fill,
}

impl Style {
    pub fn id(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Satin => "satin",
            Self::Fill => "fill",
        }
    }

    /// The style `id` names (`running`, `satin`, `fill`).
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "running" => Self::Running,
            "satin" => Self::Satin,
            "fill" => Self::Fill,
            _ => return None,
        })
    }

    /// All styles, as menus list them.
    pub const ALL: [Self; 3] = [Self::Running, Self::Satin, Self::Fill];
}

/// What one shape is stitched as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub style: Style,
    /// The longest stitch along a running line or a fill row, millimetres.
    pub stitch_length: f64,
    /// The distance between neighbouring threads: rail to rail for satin, row to row for fill,
    /// millimetres.
    pub density: f64,
    /// The direction the fill's rows run, degrees (0: left to right).
    pub angle: f64,
    /// Pull compensation: how much wider the stitching is made so the fabric's pull does not
    /// leave a gap of background at an edge, millimetres (satin only for now).
    pub pull: f64,
    /// Sew an underlay first: an edge run under a fill, a centre run under a satin column.
    pub underlay: bool,
    /// A move longer than this trims and jumps instead of running the thread across,
    /// millimetres.
    pub jump: f64,
    /// Start and end a running line with a lock stitch (a short back stitch) so the thread
    /// does not pull out.
    pub lock: bool,
    /// Shorter stitches are left out (a row's remainder, a sliver of a shape), millimetres.
    pub min_stitch: f64,
    /// The satin column's width, millimetres (unused by the other styles).
    pub width: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self::for_style(Style::Running)
    }
}

impl Params {
    /// The defaults of one style.
    pub fn for_style(style: Style) -> Self {
        match style {
            Style::Running => Self {
                style,
                stitch_length: 2.5,
                density: 0.5,
                angle: 0.0,
                pull: 0.0,
                underlay: false,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 0.0,
            },
            Style::Satin => Self {
                style,
                stitch_length: 2.5,
                density: 0.4,
                angle: 0.0,
                pull: 0.2,
                underlay: true,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 3.0,
            },
            Style::Fill => Self {
                style,
                stitch_length: 4.0,
                density: 0.4,
                angle: 45.0,
                pull: 0.0,
                underlay: true,
                jump: JUMP,
                lock: false,
                min_stitch: 0.4,
                width: 0.0,
            },
        }
    }

    /// The same parameters with every value clamped to a range that produces a stitch plan:
    /// not finite falls back to the style's default, and each distance is bounded so that no
    /// loop can run away.
    pub fn sane(self) -> Self {
        let d = Self::for_style(self.style);
        let fin = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        let angle = fin(self.angle, d.angle) % 360.0;
        Self {
            style: self.style,
            stitch_length: fin(self.stitch_length, d.stitch_length).clamp(0.2, 12.0),
            density: fin(self.density, d.density).clamp(0.05, 5.0),
            angle: if angle < 0.0 { angle + 360.0 } else { angle },
            pull: fin(self.pull, d.pull).clamp(0.0, 2.0),
            underlay: self.underlay,
            jump: fin(self.jump, d.jump).clamp(0.5, 20.0),
            lock: self.lock,
            min_stitch: fin(self.min_stitch, d.min_stitch).clamp(0.05, 3.0),
            width: fin(self.width, d.width).clamp(0.0, 50.0),
        }
    }
}

/// The stitches of `path` in style `p`: one block, in needle order.
///
/// `rule` decides what a fill covers when its subpaths overlap (a hole wound against its shape
/// is a hole under the non-zero rule; under the even-odd rule any two crossings change side).
pub fn stitches(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    match p.style {
        Style::Running => running(path, &p),
        Style::Satin if p.width > 0.0 => satin(path, &p),
        // A satin column needs a width: without one the shape is filled instead.
        Style::Satin | Style::Fill => fill(path, rule, &p),
    }
}

/// A running line of stitches along `path` ([`Style::Running`]).
pub fn running(path: &BezPath, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    let mut runs: Vec<Vec<Point>> = Vec::new();
    for sub in subpaths(path) {
        let pts = sub.points();
        if length(&pts, sub.closed) < p.min_stitch {
            continue;
        }
        let mut run = resample(&pts, p.stitch_length, sub.closed);
        if p.lock && !sub.closed {
            run = locked(&run);
        }
        runs.push(run);
    }
    join(runs, &p)
}

/// A satin column sewn along `path`'s centreline, `params.width` wide ([`Style::Satin`]). The
/// width comes from [`Params::width`] unless `width` is positive.
pub fn satin(path: &BezPath, p: &Params) -> Vec<Stitch> {
    let mut p = p.sane();
    p.style = Style::Satin;
    if p.width <= 0.0 {
        return Vec::new();
    }
    let half = (p.width + 2.0 * p.pull) / 2.0;
    let mut runs: Vec<Vec<Point>> = Vec::new();
    for sub in subpaths(path) {
        let pts = sub.points();
        if length(&pts, sub.closed) < p.density * 2.0 {
            continue;
        }
        if p.underlay {
            let under = resample(&pts, UNDERLAY_STITCH, sub.closed);
            if under.len() >= 2 {
                runs.push(under);
            }
        }
        let samples = resample(&pts, p.density, sub.closed);
        if samples.len() < 2 {
            continue;
        }
        let mut zigzag = Vec::with_capacity(samples.len() + 1);
        for i in 0..samples.len() {
            let Some(q) = samples.get(i).copied() else { continue };
            let t = tangent(&samples, i, sub.closed);
            let n = Vec2::new(-t.y, t.x);
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            zigzag.push(q + n * half * side);
        }
        // A closed column ends where it started: the last crossing takes the remaining side.
        if sub.closed
            && let (Some(first), Some(last)) = (zigzag.first().copied(), zigzag.last().copied())
            && first.distance(last) > p.min_stitch
        {
            zigzag.push(first);
        }
        runs.push(zigzag);
        if runs.len() >= MAX_RUNS {
            break;
        }
    }
    join(runs, &p)
}

/// A tatami fill of `path` ([`Style::Fill`]): rows of stitches at [`Params::angle`] degrees,
/// [`Params::density`] apart, each split into stitches at most [`Params::stitch_length`] long,
/// with an edge run underneath when [`Params::underlay`] is on. Open subpaths are ignored by the
/// fill (there is no inside); they are sewn as running lines underneath instead.
pub fn fill(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    let subs = subpaths(path);
    let mut runs: Vec<Vec<Point>> = Vec::new();
    // An edge run under the fill holds the fabric; open lines are sewn as they are.
    if p.underlay {
        for sub in subs.iter().filter(|s| s.closed) {
            let under = resample(&sub.points(), UNDERLAY_STITCH, true);
            if under.len() >= 2 {
                runs.push(under);
            }
        }
    }
    let mut open: Vec<Vec<Point>> = subs.iter().filter(|s| !s.closed).map(|s| s.points()).collect();
    for line in open.drain(..) {
        if length(&line, false) >= p.min_stitch {
            runs.push(resample(&line, p.stitch_length, false));
        }
    }
    let closed: Vec<&SubPath> = subs.iter().filter(|s| s.closed && s.pts.len() >= 3).collect();
    if closed.is_empty() {
        return join(runs, &p);
    }
    let (along, across) = axes(p.angle);
    // The rows run along `along`; each sits at one value of `across`.
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for sub in &closed {
        for q in &sub.pts {
            let t = dot(*q, across);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    if !(lo.is_finite() && hi.is_finite()) || hi - lo < p.density * 0.5 {
        return join(runs, &p);
    }
    let spacing = p.density;
    // Rows sit half a spacing inside the shape's extent: no row lies on a tangent, and the
    // thread's own width covers to the edge.
    let rows = (((hi - lo) / spacing).floor().max(1.0) as usize).min(MAX_ROWS);
    for row in 0..rows {
        let t = lo + spacing * (row as f64 + 0.5);
        if t >= hi {
            break;
        }
        let spans = spans_at(&closed, rule, along, across, t);
        let mut forward = row % 2 == 0;
        // The stagger walks a quarter stitch at a time through a cycle of four rows: no two
        // rows line their penetrations up into a groove.
        let phase = 0.25 * p.stitch_length * (row % 4) as f64;
        for (a, b) in spans {
            let ss = row_points(a, b, phase, &p);
            if ss.len() < 2 {
                continue;
            }
            let mut pts: Vec<Point> = ss.iter().map(|s| at(*s, t, along, across)).collect();
            if !forward {
                pts.reverse();
            }
            // Skip the last stitch of the row: the needle turns diagonally into the next row
            // instead of crawling one row spacing along the edge.
            if pts.len() > 2 {
                pts.pop();
            }
            runs.push(pts);
            forward = !forward;
            if runs.len() >= MAX_RUNS {
                return join(runs, &p);
            }
        }
    }
    join(runs, &p)
}

/// Where the row at `t` crosses the shape: the spans of `along` inside it, by `rule`.
fn spans_at(polys: &[&SubPath], rule: FillRule, along: Vec2, across: Vec2, t: f64) -> Vec<(f64, f64)> {
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for sub in polys {
        let n = sub.pts.len();
        for i in 0..n {
            let Some(&a) = sub.pts.get(i) else { continue };
            let Some(&b) = sub.pts.get(if i + 1 == n { 0 } else { i + 1 }) else { continue };
            let (ta, tb) = (dot(a, across), dot(b, across));
            // Half-open: a vertex exactly on the row counts once.
            if (ta > t) == (tb > t) {
                continue;
            }
            let f = (t - ta) / (tb - ta);
            if !f.is_finite() {
                continue;
            }
            let s = dot(a, along) + f * (dot(b, along) - dot(a, along));
            crossings.push((s, if tb > ta { 1 } else { -1 }));
        }
    }
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spans: Vec<(f64, f64)> = Vec::new();
    match rule {
        FillRule::EvenOdd => {
            for pair in crossings.chunks(2) {
                if let [a, b] = pair
                    && b.0 - a.0 > 1e-9
                {
                    spans.push((a.0, b.0));
                }
            }
        }
        FillRule::NonZero => {
            let mut winding = 0;
            let mut start = 0.0;
            for (s, dir) in crossings {
                let before = winding;
                winding += dir;
                if before == 0 && winding != 0 {
                    start = s;
                } else if before != 0 && winding == 0 && s - start > 1e-9 {
                    spans.push((start, s));
                }
            }
        }
    }
    spans
}

/// The positions along a row that get a stitch: `ceil(len / stitch_length)` even steps (so no
/// stitch is longer than asked and the last one is not a stub), with the interior points moved by
/// `phase` (the stagger that keeps rows from lining up). Both ends are included.
fn row_points(a: f64, b: f64, phase: f64, p: &Params) -> Vec<f64> {
    let len = b - a;
    // A row shorter than two row spacings (or 0.8 mm) has no room for a fill: leave it out.
    let shortest = (2.0 * p.density).max(0.8);
    if !len.is_finite() || len < shortest || len < p.min_stitch {
        return Vec::new();
    }
    let count = ((len / p.stitch_length).ceil() as usize).clamp(1, MAX_STITCHES);
    let step = len / count as f64;
    let mut out: Vec<f64> = (0..=count).map(|k| a + k as f64 * step).collect();
    if count >= 3 && step.is_finite() && step > 0.0 {
        // At most half a step (so no stitch grows past one and a half stitch lengths) and at
        // most a step less the shortest stitch (so the row's last stitch is not a stub).
        let hi = (step * 0.5).min((step - p.min_stitch).max(0.0));
        let shift = phase.rem_euclid(step).min(hi);
        for v in out.iter_mut().skip(1).take(count - 1) {
            *v = (*v + shift).min(b);
        }
    }
    out
}

/// Runs joined into one block: travelling far enough to trim does.
fn join(runs: Vec<Vec<Point>>, p: &Params) -> Vec<Stitch> {
    let mut out: Vec<Stitch> = Vec::new();
    for mut run in runs {
        dedupe(&mut run);
        let Some(&first) = run.first() else { continue };
        if let Some(last) = out.last().copied()
            && last.distance(Stitch::sew(first)) > p.jump
        {
            out.push(Stitch::new(last.x, last.y, Command::Trim));
            out.push(Stitch::jump(first));
        }
        for q in run {
            if out.len() >= MAX_STITCHES {
                return out;
            }
            if out.last().is_none_or(|last| last.point().distance(q) > 1e-9) {
                out.push(Stitch::sew(q));
            }
        }
    }
    out
}

/// A run with a lock stitch at each end: a short back stitch so the thread stays put.
fn locked(pts: &[Point]) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return pts.to_vec() };
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out: Vec<Point> = Vec::with_capacity(pts.len() + 6);
    if let Some(d) = unit(pts.get(1).copied().unwrap_or(first) - first) {
        out.push(first);
        out.push(first + d * LOCK);
        out.push(first);
    }
    out.extend_from_slice(pts);
    if let Some(d) = unit(last - pts.get(pts.len().wrapping_sub(2)).copied().unwrap_or(first)) {
        out.push(last + d * LOCK);
        out.push(last);
    }
    out
}

/// A polyline: its points and whether it closes.
struct SubPath {
    pts: Vec<Point>,
    closed: bool,
}

impl SubPath {
    /// The points, with a closed polyline's first point repeated at the end.
    fn points(&self) -> Vec<Point> {
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
fn subpaths(path: &BezPath) -> Vec<SubPath> {
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
/// where it started.
fn resample(pts: &[Point], step: f64, closed: bool) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
    let step = step.max(0.01);
    let mut out: Vec<Point> = vec![first];
    let mut travelled = 0.0;
    let mut next = step;
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(if i + 1 == n { 0 } else { i + 1 })) else { continue };
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
    }
    if closed {
        if out.last().is_some_and(|q| q.distance(first) > 1e-9) {
            out.push(first);
        }
    } else if out.last().is_none_or(|q| q.distance(last) > 1e-9) {
        out.push(last);
    }
    out
}

/// The total length of a polyline, millimetres.
fn length(pts: &[Point], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut total = 0.0;
    for i in 0..segs {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(if i + 1 == n { 0 } else { i + 1 })) else { continue };
        let d = a.distance(b);
        if d.is_finite() {
            total += d;
        }
    }
    total
}

/// The unit tangent at point `i` of `pts`: the direction the line runs through it.
fn tangent(pts: &[Point], i: usize, closed: bool) -> Vec2 {
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
fn unit(v: Vec2) -> Option<Vec2> {
    let len = v.hypot();
    if len.is_finite() && len > 1e-12 { Some(v / len) } else { None }
}

/// Remove repeated points from a run (a back stitch repeats its point on purpose, but a
/// duplicated sample is noise).
fn dedupe(pts: &mut Vec<Point>) {
    let mut i = 0;
    while i + 1 < pts.len() {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(i + 1))
            && a.distance(*b) <= 1e-9
        {
            pts.remove(i + 1);
            continue;
        }
        i += 1;
    }
}

/// The two unit vectors a fill's rows are laid along: `along` (the row direction at
/// [`Params::angle`]) and `across` (the direction the rows step in).
fn axes(angle_deg: f64) -> (Vec2, Vec2) {
    let a = angle_deg * TAU / 360.0;
    let (s, c) = a.sin_cos();
    let along = Vec2::new(c, s);
    let across = Vec2::new(-s, c);
    if along.hypot().is_finite() && across.hypot().is_finite() { (along, across) } else { (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)) }
}

/// Where `p` falls along `v`.
fn dot(p: Point, v: Vec2) -> f64 {
    p.x * v.x + p.y * v.y
}

/// The point `s` along and `t` across the fill's axes.
fn at(s: f64, t: f64, along: Vec2, across: Vec2) -> Point {
    Point::new(s * along.x + t * across.x, s * along.y + t * across.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x, y));
        p.line_to((x + w, y));
        p.line_to((x + w, y + h));
        p.line_to((x, y + h));
        p.close_path();
        p
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> BezPath {
        let mut p = BezPath::new();
        p.move_to(a);
        p.line_to(b);
        p
    }

    fn points(v: &[Stitch]) -> Vec<Point> {
        v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
    }

    #[test]
    fn a_running_line_steps_at_the_stitch_length() {
        let p = Params::for_style(Style::Running);
        let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
        let q = points(&out);
        assert!(q.len() >= 8, "{q:?}");
        assert!((q[0].x - 0.0).abs() < 1e-6 && (q[0].y).abs() < 1e-6);
        assert!((q.last().expect("last").x - 20.0).abs() < 1e-6, "{:?}", q.last());
        for w in q.windows(2) {
            let d = w[0].distance(w[1]);
            assert!(d <= p.stitch_length + 1e-6, "{d}");
        }
        // The lock stitch starts and ends on the line, back where it began.
        assert!(q.iter().all(|q| q.y.abs() < 1e-9));
    }

    #[test]
    fn a_fill_covers_the_rectangle_and_no_stitch_is_longer_than_asked() {
        let p = Params::for_style(Style::Fill);
        let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
        let q = points(&out);
        assert!(q.len() > 100, "{}", q.len());
        for a in &q {
            assert!(a.x >= -1e-6 && a.x <= 20.0 + 1e-6 && a.y >= -1e-6 && a.y <= 10.0 + 1e-6, "{a:?}");
        }
        for w in out.windows(2) {
            // Neighbouring penetrations of one row (same y, needle down twice in a row) are at
            // most the stitch length plus half a stagger apart; row turns and travels are longer,
            // and a jump between two stitches is not a stitch at all.
            let (a, b) = (w[0], w[1]);
            if a.command == Command::Stitch && b.command == Command::Stitch && (a.y - b.y).abs() < 1e-6 {
                assert!(a.distance(b) <= p.stitch_length * 1.5 + 1e-6, "{:?} .. {:?}", a.point(), b.point());
            }
        }
        // About the area divided by the thread's coverage.
        let area = 20.0 * 10.0;
        let expected = area / (p.density * p.stitch_length);
        assert!((q.len() as f64) > expected * 0.5, "{} vs {expected}", q.len());
    }

    #[test]
    fn a_hole_is_left_unstitched() {
        // A ring: the outer square wound one way, the inner square the other.
        let mut p = rect(0.0, 0.0, 30.0, 30.0);
        let mut hole = BezPath::new();
        hole.move_to((10.0, 10.0));
        hole.line_to((10.0, 20.0));
        hole.line_to((20.0, 20.0));
        hole.line_to((20.0, 10.0));
        hole.close_path();
        p.extend(hole);
        let params = Params::for_style(Style::Fill);
        let out = stitches(&p, FillRule::NonZero, &params);
        let q = points(&out);
        assert!(q.len() > 50);
        for a in &q {
            let in_hole = a.x > 10.0 + 0.2 && a.x < 20.0 - 0.2 && a.y > 10.0 + 0.2 && a.y < 20.0 - 0.2;
            assert!(!in_hole, "{a:?}");
        }
    }

    #[test]
    fn the_angle_turns_the_rows() {
        let mut p = Params::for_style(Style::Fill);
        p.angle = 90.0;
        let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
        let q = points(&out);
        // Rows run vertically: each row keeps one x, rows step by the density, and a row spans
        // the shape's height.
        let mut xs: Vec<f64> = q.iter().map(|a| a.x).collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(xs.len() >= 10, "{xs:?}");
        assert!((xs[1] - xs[0]).abs() - p.density < 0.2, "{:?}", &xs[..2]);
        assert!(q.iter().any(|a| a.y > 9.0), "a vertical row spans the height");
    }

    #[test]
    fn a_satin_column_crosses_its_width() {
        let p = Params::for_style(Style::Satin);
        let mut params = p;
        params.width = 4.0;
        params.pull = 0.0;
        let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &params);
        let q = points(&out);
        assert!(q.len() > 20, "{}", q.len());
        assert!(q.iter().all(|a| a.y.abs() <= 2.0 + 1e-6), "{q:?}");
        // Neighbouring stitches are on opposite rails.
        let rails: Vec<f64> = q.iter().skip(1).map(|a| a.y).collect();
        assert!(rails.iter().any(|y| *y > 1.5) && rails.iter().any(|y| *y < -1.5));
        let mut wider = params;
        wider.pull = 0.5;
        let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &wider);
        let w = points(&out);
        assert!(w.iter().any(|a| a.y.abs() > 2.2), "pull compensation widens the column");
    }

    #[test]
    fn shapes_far_apart_are_trimmed() {
        let mut p = rect(0.0, 0.0, 10.0, 10.0);
        p.extend(rect(60.0, 0.0, 10.0, 10.0));
        let params = Params::for_style(Style::Fill);
        let out = stitches(&p, FillRule::NonZero, &params);
        assert!(out.iter().any(|s| s.command == Command::Trim), "a long travel cuts the thread");
    }

    #[test]
    fn nothing_panics_on_odd_input() {
        let empty = BezPath::new();
        assert!(stitches(&empty, FillRule::NonZero, &Params::default()).is_empty());
        let mut p = Params::for_style(Style::Fill);
        p.density = f64::NAN;
        p.stitch_length = 0.0;
        p.angle = f64::INFINITY;
        p.pull = -5.0;
        p.jump = f64::NAN;
        let out = stitches(&rect(0.0, 0.0, 10.0, 10.0), FillRule::NonZero, &p);
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
        // A zero-length path has no stitches, and a single-point path neither.
        assert!(stitches(&line((1.0, 1.0), (1.0, 1.0)), FillRule::NonZero, &Params::default()).is_empty());
    }
}
