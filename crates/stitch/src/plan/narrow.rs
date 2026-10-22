//! Narrow parts of a filled shape, to be sewn as satin columns instead of tatami ([`columns`]):
//! where two stretches of the outline face each other closer than a satin column may be wide.
//! Auto-digitizers choose by width too, satin for narrow objects and fill for wide ones; here each
//! part of a shape is chosen on its own.
//!
//! How they are found. The outline is sampled every [`SAMPLE_STEP`], and from each sample a ray
//! goes into the shape, square to the outline (its direction turning smoothly from one flattened
//! segment to the next, as the curve's does), to where it leaves the shape again. The two points
//! are opposite each other when all of these hold:
//! - the ray is no longer than the widest column ([`RUNS_ON`] times the limit: a column may run
//!   on through parts a little wider, as long as it is typically within the limit);
//! - the outline there faces back within [`FACING`];
//! - the ray back from the far point lands near the first, so a corner or a round end that looks
//!   down the length of a stem pairs with nothing.
//!
//! Consecutive samples whose opposite points move steadily the other way along the outline form a
//! *side*. Two sides opposite each other are a column, matched point to point (its *stations*),
//! and its ends follow the outline round a pointed or round tip. A column must be [`MIN_ASPECT`]
//! times as long as it is wide, so a compact shape (a square, a dot) stays a fill, and it runs on
//! through a stretch wider than the limit only when that is short ([`RUN_ON_LENGTH`]), so it does
//! not fan across a wide part. Points repeated where flattened curves join are dropped first
//! ([`REPEATED`]): at one, the rays stopped turning and a ring broke open.

use vectorcraft_geom::{BezPath, FillRule, Point, Rect, Shape, Vec2};

use super::MAX_SATIN_WIDTH;
use super::area::{Area, Grid};
use super::fill::{closed_subpaths, outlines};
use super::geometry::{CORNER, NO_LENGTH, SubPath, turn, unit};
use super::satin::Along;
use crate::SAME_POINT;

/// How far apart the outline is sampled (millimetres): fine enough that a 1 mm wide column's end
/// is four samples across. An estimate.
const SAMPLE_STEP: f64 = 0.25;
/// The most samples one shape takes, a bound on the work (two rays each): a longer outline is
/// sampled more coarsely.
const MAX_SAMPLES: usize = 100_000;
/// How far a probe steps off the outline or past a crossing to tell inside from outside
/// (millimetres): far below any column's width, far above rounding.
const PROBE: f64 = 0.02;
/// Two stretches of outline face each other when the angle between them is at most 60° (the
/// cosine of the angle between their normals' opposites): the sides of a pointed leaf converge
/// within it. An estimate.
const FACING: f64 = 0.5;
/// How near the first point the ray back from the far one must land: this share of the width,
/// plus two samples. On sides that converge at the angle [`FACING`] allows, it lands within about
/// half the width. Estimates, as are the other bounds below that tell a column's sides apart: each
/// tuned on test shapes and the real designs' artwork, none from a source.
const MUTUAL_SHARE: f64 = 0.5;
const MUTUAL_SAMPLES: f64 = 2.0;
/// Two points of one outline less than this far apart along it are the two sides of a pointed tip
/// next to it (millimetres): they pair with nothing, so each side is a stretch of its own, and the
/// column's end then follows the outline round the tip.
const TIP_ARC: f64 = 1.0;
/// How far the opposite point may move backwards between two samples, as a share of the column's
/// width, and at least this many samples: on the inner side of a tight curve the opposite (outer)
/// points spread out. A larger step is another column.
const STEP_SHARE: f64 = 2.0;
const STEP_SAMPLES: f64 = 4.0;
/// The most edge tests a shape without an edge grid may take (its samples' rays against every
/// edge): a shape of thousands of edges from corner to corner has none ([`Grid::new`]), and its
/// narrow parts are not worth that much work. A bound on the work.
const MAX_UNGRIDDED_TESTS: f64 = 2e7;
/// How much wider than the widest column a column may be where it runs on: a stroke whose width
/// wavers about the limit is sewn whole, not cut into satin and tatami where it crosses it; the
/// column itself must be typically (its median) no wider than the limit. An estimate.
const RUNS_ON: f64 = 1.5;
/// How long a stretch wider than the limit may be for a column to run on through it, as a multiple
/// of the column's typical width where it is within the limit: a waver about the limit, not a wide
/// part (a knot, the bowl at a dumbbell's end) the column would fan across. A longer stretch is cut
/// out of the column and left to the tatami. As long as [`MIN_ASPECT`] asks a column to be: a
/// stretch that long is a part of its own. An estimate.
const RUN_ON_LENGTH: f64 = MIN_ASPECT;
/// A side has at least this many samples.
const MIN_SIDE: usize = 3;
/// How many samples, or sides, are worked through between two asks whether to stop
/// ([`columns_until`]): often enough to stop within milliseconds, rarely enough to cost nothing.
const STOP_CHECK: usize = 1024;
/// A column is at least this many times as long as it is wide; a compact shape stays a fill.
/// An estimate.
const MIN_ASPECT: f64 = 2.0;
/// The share of a side's samples that must look across at the other side for the two to be paired.
const MIN_PAIRED: f64 = 1.0 / 3.0;
/// A column's end follows the outline round its tip when the stretch of outline from one side to
/// the other there is at most this many times the column's width (a round or pointed end); a
/// longer one leads elsewhere (a junction).
const END_STRETCH: f64 = 3.0;
/// A column that has this share of its stations inside a longer column is the same part seen
/// across: it is left out.
const MAX_OVERLAP: f64 = 0.25;
/// Points of an outline closer together than this are one point (millimetres): flattening repeats
/// the point where two curves join, and a segment this short has no direction to turn the rays by.
/// Far below [`SAMPLE_STEP`], far above rounding.
const REPEATED: f64 = 1e-3;
/// How far past a column's end the shape is looked for, to tell whether the shape ends there
/// (millimetres): past the last station of a square, round or pointed end, which lies on the
/// outline or within a sample of it, and well into the rest of the shape where the column meets
/// it.
const END_PROBE: f64 = 2.0 * SAMPLE_STEP;

/// A narrow part of a filled shape, sewn as a satin column: its stations, each a point on one
/// side of it and the point opposite on the other, in order along it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Column {
    pub stations: Vec<(Point, Point)>,
    /// It goes round (a ring): its last station leads back to its first.
    pub closed: bool,
    /// Whether its first and its last station end where the shape does (the outline turns across
    /// the column there: a pointed, round or square end), rather than where the column meets the
    /// rest of the shape. A ring has neither.
    pub tips: [bool; 2],
}

impl Column {
    /// The area the column covers, as a path: its first side out and its second back, or for a
    /// ring its two sides as two loops wound opposite ways.
    pub fn outline(&self) -> BezPath {
        let mut path = BezPath::new();
        let first = self.stations.iter().map(|(a, _)| *a);
        let second = self.stations.iter().rev().map(|(_, b)| *b);
        if self.closed {
            for side in [first.collect::<Vec<_>>(), second.collect::<Vec<_>>()] {
                for (k, q) in side.into_iter().enumerate() {
                    if k == 0 { path.move_to(q) } else { path.line_to(q) }
                }
                path.close_path();
            }
        } else {
            for (k, q) in first.chain(second).enumerate() {
                if k == 0 { path.move_to(q) } else { path.line_to(q) }
            }
            path.close_path();
        }
        path
    }

    /// The middle of each station.
    pub fn middles(&self) -> Vec<Point> {
        self.stations.iter().map(|(a, b)| a.midpoint(*b)).collect()
    }

    /// How long the column is along its middle (millimetres), round to its start for a ring.
    pub fn length(&self) -> f64 {
        let m = self.middles();
        let open: f64 = m.windows(2).map(|w| w[0].distance(w[1])).filter(|d| d.is_finite()).sum();
        match (self.closed, m.first(), m.last()) {
            (true, Some(a), Some(b)) => open + a.distance(*b),
            _ => open,
        }
    }

    /// Its typical width: the middle one of its stations' widths (millimetres).
    pub fn width(&self) -> f64 {
        let mut w: Vec<f64> = self.stations.iter().map(|(a, b)| a.distance(*b)).filter(|d| d.is_finite()).collect();
        w.sort_by(f64::total_cmp);
        w.get(w.len() / 2).copied().unwrap_or(0.0)
    }

    /// Whether it is sewn as a satin column: one narrower than a satin's crossings need is a
    /// running line along its middle.
    pub fn is_satin(&self) -> bool {
        self.width() >= super::satin::MIN_COLUMN_WIDTH
    }
}

/// The narrow parts of the filled `path` (`rule` decides what it covers): the columns typically at
/// most `max_width` wide (millimetres, at most [`MAX_SATIN_WIDTH`]; [`RUNS_ON`] times that where
/// one runs on), longest first. None when nothing is that narrow, or `max_width` is not above 0.
pub fn columns(path: &BezPath, rule: FillRule, max_width: f64) -> Vec<Column> {
    columns_until(path, rule, max_width, &|| false).unwrap_or_default()
}

/// [`columns`], asking `stopped` every [`STOP_CHECK`] samples, between rings and between sides
/// whether to stop (a plan that is cancelled): `None` once it says so.
pub fn columns_until(path: &BezPath, rule: FillRule, max_width: f64, stopped: &dyn Fn() -> bool) -> Option<Vec<Column>> {
    if !(max_width.is_finite() && max_width > 0.0 && super::geometry::in_range(path)) {
        return Some(Vec::new());
    }
    let max_width = max_width.min(MAX_SATIN_WIDTH);
    let subs: Vec<SubPath> = closed_subpaths(path).into_iter().map(without_repeats).collect();
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return Some(Vec::new());
    }
    let grid = Grid::new(&outlines);
    let shape = Area { outlines: &outlines, rule, grid: grid.as_ref() };
    let rings: Vec<Option<Ring>> = outlines.iter().map(|s| Ring::new(&s.pts)).collect();
    let total: f64 = rings.iter().flatten().map(|r| r.len).sum();
    if !total.is_finite() {
        return Some(Vec::new());
    }
    let step = SAMPLE_STEP.max(total / MAX_SAMPLES as f64);
    let edges: usize = outlines.iter().map(|s| s.pts.len()).sum();
    if grid.is_none() && (total / step) * edges as f64 > MAX_UNGRIDDED_TESTS {
        return Some(Vec::new());
    }
    let mut finder = Finder { shape, rings, offsets: Vec::new(), step, max_width, samples: Vec::new(), by_ring: Vec::new(), stopped };
    finder.offsets = outlines
        .iter()
        .scan(0usize, |acc, s| {
            let at = *acc;
            *acc += s.pts.len();
            Some(at)
        })
        .collect();
    finder.sample()?;
    let sides = finder.sides();
    finder.pair(&sides)
}

/// `sub` without repeated points ([`REPEATED`]), a closed outline's last point included when it
/// repeats its first: every point then has a direction either side of it, so the rays turn
/// smoothly through the joins of a curve (where a point repeated, the turn stopped, the far points
/// jumped, and a ring broke open).
fn without_repeats(sub: SubPath) -> SubPath {
    let mut pts: Vec<Point> = Vec::with_capacity(sub.pts.len());
    for q in sub.pts {
        // (A point that is not a number stays, so that the outline's length is not one either and
        // the shape is left a fill, as `columns` leaves any such shape.)
        if pts.last().map(|p| p.distance(q)).is_none_or(|d| d >= REPEATED || d.is_nan()) {
            pts.push(q);
        }
    }
    while sub.closed && pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.distance(*b) < REPEATED) {
        pts.pop();
    }
    SubPath { pts, closed: sub.closed }
}

/// One closed outline measured along: its points, the distance along it to each, its direction
/// at each point where it turns smoothly (none at a sharp corner), its length, and whether the
/// shape's inside lies on its other side (then it is walked backwards, so that every outline is
/// walked with the inside on one side).
struct Ring<'a> {
    pts: &'a [Point],
    at: Vec<f64>,
    smooth: Vec<Option<Vec2>>,
    len: f64,
    flipped: bool,
}

impl<'a> Ring<'a> {
    fn new(pts: &'a [Point]) -> Option<Self> {
        let n = pts.len();
        let mut at = Vec::with_capacity(n);
        let mut len = 0.0;
        for i in 0..n {
            at.push(len);
            len += pts.get(i)?.distance(*pts.get((i + 1) % n)?);
        }
        // A flattened curve's direction, between the directions of the segments either side of
        // each point: so the normals turn smoothly along it, as the curve's do, rather than in
        // steps a segment long (the rays across a column would then hit its other side in a
        // sawtooth). A sharp corner keeps each segment's own.
        let smooth = (0..n)
            .map(|i| {
                let (a, q, b) = (*pts.get((i + n - 1) % n)?, *pts.get(i)?, *pts.get((i + 1) % n)?);
                let (u, v) = (unit(q - a)?, unit(b - q)?);
                // A sharp corner (geometry's CORNER): the direction is not smoothed across it.
                if turn(u, v) > CORNER { None } else { unit(u + v) }
            })
            .collect();
        (len.is_finite() && len > NO_LENGTH).then_some(Self { pts, at, smooth, len, flipped: false })
    }

    /// The point `s` along the ring (as drawn), and the ring's direction there.
    fn point(&self, s: f64) -> Option<(Point, Vec2)> {
        let s = s.rem_euclid(self.len);
        let n = self.pts.len();
        let i = self.at.partition_point(|&a| a <= s).saturating_sub(1);
        let (a, b) = (*self.pts.get(i)?, *self.pts.get((i + 1) % n)?);
        let seg = a.distance(b);
        let f = if seg > NO_LENGTH { ((s - self.at.get(i)?) / seg).clamp(0.0, 1.0) } else { 0.0 };
        let own = unit(b - a)?;
        let (t0, t1) = (self.smooth.get(i).copied().flatten().unwrap_or(own), self.smooth.get((i + 1) % n).copied().flatten().unwrap_or(own));
        Some((a.lerp(b, f), unit(t0 + (t1 - t0) * f).unwrap_or(own)))
    }

    /// `s` (as drawn) measured the way the ring is walked; the same turns a walked position back.
    fn walk(&self, s: f64) -> f64 {
        if self.flipped { (self.len - s).rem_euclid(self.len) } else { s.rem_euclid(self.len) }
    }

    /// How far on from walked position `a` walked position `b` is.
    fn ahead(&self, a: f64, b: f64) -> f64 {
        (b - a).rem_euclid(self.len)
    }

    /// The point at walked position `w`.
    fn at_walk(&self, w: f64) -> Option<Point> {
        self.point(self.walk(w)).map(|(q, _)| q)
    }

    /// The walked positions of the ring's sharp corners (turns over geometry's `CORNER`): at a
    /// column's end the column's side reaches one (a square end).
    fn corners(&self) -> Vec<f64> {
        let n = self.pts.len();
        (0..n)
            .filter_map(|i| {
                let (a, q, b) = (*self.pts.get((i + n - 1) % n)?, *self.pts.get(i)?, *self.pts.get((i + 1) % n)?);
                let (u, v) = (unit(q - a)?, unit(b - q)?);
                let at = *self.at.get(i)?;
                (turn(u, v) > CORNER).then(|| self.walk(at))
            })
            .collect()
    }
}

/// A sample of the outline: its ring, where it is along it (as drawn), the point, the direction
/// into the shape there, and the point opposite it, if any.
struct Sample {
    ring: usize,
    s: f64,
    p: Point,
    opposite: Option<Opposite>,
}

/// The point opposite a sample: its ring, where it is along it (as drawn), and how far across.
#[derive(Clone, Copy)]
struct Opposite {
    ring: usize,
    s: f64,
    width: f64,
}

/// Where a ray left the shape: the ring, where along it (as drawn), the point, and the ring's
/// direction there.
struct Hit {
    ring: usize,
    s: f64,
    q: Point,
    edge: Vec2,
}

/// A side: a stretch of samples of one ring, in walked order, each with an opposite point, the
/// opposites moving steadily backwards; `whole` when it goes all the way round.
struct Side {
    ring: usize,
    samples: Vec<usize>,
    whole: bool,
}

/// The search: the shape, its rings, where each ring's edges start in the shape's list of edges,
/// the sample step, the widest column, the samples (all of them, and each ring's in order), and
/// whether to stop ([`columns_until`]).
struct Finder<'a> {
    shape: Area<'a>,
    rings: Vec<Option<Ring<'a>>>,
    offsets: Vec<usize>,
    step: f64,
    max_width: f64,
    samples: Vec<Sample>,
    by_ring: Vec<Vec<usize>>,
    stopped: &'a dyn Fn() -> bool,
}

impl Finder<'_> {
    /// Whether to stop, asked every [`STOP_CHECK`]th time `i` comes round.
    fn stop_at(&self, i: usize) -> bool {
        i.is_multiple_of(STOP_CHECK) && (self.stopped)()
    }

    /// Sample every ring, decide which way each is walked, and find each sample's opposite point;
    /// `None` when stopped.
    fn sample(&mut self) -> Option<()> {
        let mut inward: Vec<Vec2> = Vec::new();
        self.by_ring = vec![Vec::new(); self.rings.len()];
        let stopped = self.stopped;
        for (k, ring) in self.rings.iter_mut().enumerate() {
            if stopped() {
                return None;
            }
            let Some(ring) = ring else { continue };
            let n = (ring.len / self.step).floor().max(1.0) as usize;
            let (mut one, mut other) = (0usize, 0usize);
            let mut ids = Vec::with_capacity(n);
            for j in 0..n {
                let s = j as f64 * self.step;
                // Every sample keeps its place in the list (the opposite points are found by it),
                // one where the ring has no direction with no opposite.
                let Some((p, t)) = ring.point(s) else {
                    ids.push(self.samples.len());
                    self.samples.push(Sample { ring: k, s, p: Point::ORIGIN, opposite: None });
                    inward.push(Vec2::ZERO);
                    continue;
                };
                // The normal on the side the inside is: one side in, the other out.
                let left = Vec2::new(-t.y, t.x);
                let (l, r) = (self.shape.contains(p + left * PROBE), self.shape.contains(p - left * PROBE));
                let n = match (l, r) {
                    (true, false) => {
                        one += 1;
                        left
                    }
                    (false, true) => {
                        other += 1;
                        -left
                    }
                    // Inside on both sides (an edge within the area) or neither (a sliver): no
                    // boundary here.
                    _ => Vec2::ZERO,
                };
                ids.push(self.samples.len());
                self.samples.push(Sample { ring: k, s, p, opposite: None });
                inward.push(n);
            }
            ring.flipped = other > one;
            if ring.flipped {
                ids.reverse();
            }
            if let Some(v) = self.by_ring.get_mut(k) {
                *v = ids;
            }
        }
        for i in 0..self.samples.len() {
            if self.stop_at(i) {
                return None;
            }
            let (Some(sample), Some(&n)) = (self.samples.get(i), inward.get(i)) else { continue };
            if n == Vec2::ZERO {
                continue;
            }
            let opposite = self.opposite(sample.ring, sample.s, sample.p, n);
            if let Some(sample) = self.samples.get_mut(i) {
                sample.opposite = opposite;
            }
        }
        Some(())
    }

    /// The point opposite `p` (at `s` along ring `ring`, `n` pointing into the shape), when the
    /// two face each other across a column ([`FACING`], the ray back, [`TIP_ARC`]).
    fn opposite(&self, ring: usize, s: f64, p: Point, n: Vec2) -> Option<Opposite> {
        let widest = (self.max_width * RUNS_ON).min(MAX_SATIN_WIDTH);
        let reach = widest + 2.0 * PROBE;
        let hit = self.exit(p, n, reach)?;
        let width = p.distance(hit.q);
        if width > widest {
            return None;
        }
        // The far side's normal into the shape: of the edge's two, the one facing back.
        let m = Vec2::new(-hit.edge.y, hit.edge.x);
        let m = if m.dot(n) < 0.0 { m } else { -m };
        if -m.dot(n) < FACING {
            return None;
        }
        let back = self.exit(hit.q, m, reach)?;
        if back.q.distance(p) > MUTUAL_SHARE * width + MUTUAL_SAMPLES * self.step {
            return None;
        }
        if hit.ring == ring {
            let len = self.rings.get(ring)?.as_ref()?.len;
            let along = (hit.s - s).rem_euclid(len);
            if along.min(len - along) < TIP_ARC {
                return None;
            }
        }
        Some(Opposite { ring: hit.ring, s: hit.s, width })
    }

    /// Where a ray from `p` along `dir` (a unit vector) first leaves the shape, at most `reach`
    /// away.
    fn exit(&self, p: Point, dir: Vec2, reach: f64) -> Option<Hit> {
        let (a, b) = (p + dir * PROBE, p + dir * reach);
        let mut hits: Vec<(f64, usize, f64, Point, Point)> = Vec::new();
        self.shape.edges_along(a, b, |id, c, d| {
            if let Some((t, u)) = intersect(a, b, c, d) {
                hits.push((t, id, u, c, d));
            }
        });
        hits.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        hits.dedup_by(|x, y| x.1 == y.1);
        for (t, id, u, c, d) in hits {
            let q = a.lerp(b, t);
            if self.shape.contains(q + dir * PROBE) {
                continue;
            }
            // The edge's ring: the last one whose edges start at or before it.
            let ring = self.offsets.partition_point(|&o| o <= id).checked_sub(1)?;
            let r = self.rings.get(ring)?.as_ref()?;
            let seg = id - self.offsets.get(ring)?;
            let s = r.at.get(seg)? + u * c.distance(d);
            return Some(Hit { ring, s, q, edge: unit(d - c)? });
        }
        None
    }

    /// The sides: on each ring, the stretches of samples whose opposite points move steadily
    /// backwards along one ring.
    fn sides(&self) -> Vec<Side> {
        let mut out = Vec::new();
        for (k, ids) in self.by_ring.iter().enumerate() {
            let n = ids.len();
            let follows = |i: usize, j: usize| -> bool {
                let (Some(a), Some(b)) = (self.sample_at(ids, i).and_then(|s| s.opposite), self.sample_at(ids, j).and_then(|s| s.opposite)) else {
                    return false;
                };
                let Some(r) = self.rings.get(a.ring).and_then(Option::as_ref) else { return false };
                if a.ring != b.ring {
                    return false;
                }
                // How far the opposite point moved, the way the far ring is walked.
                let moved = r.ahead(r.walk(a.s), r.walk(b.s));
                let moved = if moved > r.len / 2.0 { moved - r.len } else { moved };
                let most = (STEP_SAMPLES * self.step).max(STEP_SHARE * a.width.max(b.width));
                moved <= self.step && moved >= -most
            };
            // Start after a break, so that no side is cut where the ring's samples start.
            let Some(start) = (0..n).find(|&i| !follows((i + n - 1) % n, i)) else {
                if n >= MIN_SIDE {
                    out.push(Side { ring: k, samples: ids.clone(), whole: true });
                }
                continue;
            };
            let mut side: Vec<usize> = Vec::new();
            for step in 0..n {
                let i = (start + step) % n;
                let valid = self.sample_at(ids, i).is_some_and(|s| s.opposite.is_some());
                if !side.is_empty() && !follows((i + n - 1) % n, i) {
                    if side.len() >= MIN_SIDE {
                        out.push(Side { ring: k, samples: std::mem::take(&mut side), whole: false });
                    }
                    side.clear();
                }
                if valid && let Some(&id) = ids.get(i) {
                    side.push(id);
                }
            }
            if side.len() >= MIN_SIDE {
                out.push(Side { ring: k, samples: side, whole: false });
            }
        }
        out
    }

    fn sample_at(&self, ids: &[usize], i: usize) -> Option<&Sample> {
        self.samples.get(*ids.get(i)?)
    }

    /// The columns: each side paired with the side most of its samples look across at (or, when
    /// that makes no column, the next most, while a third of them look across at it), the two
    /// matched point to point, longest first; one that lies mostly inside a longer one is left out.
    /// A side is used up only by a column that is kept, so a narrow part whose side runs on along
    /// a wider block (a bar with a thicker end, a flag on its pole) still finds its column.
    /// `None` when stopped.
    fn pair(&self, sides: &[Side]) -> Option<Vec<Column>> {
        // Which side each sample belongs to.
        let mut side_of: Vec<Option<usize>> = vec![None; self.samples.len()];
        for (si, side) in sides.iter().enumerate() {
            for &id in &side.samples {
                if let Some(v) = side_of.get_mut(id) {
                    *v = Some(si);
                }
            }
        }
        let mut order: Vec<usize> = (0..sides.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sides.get(i).map_or(0, |s| s.samples.len())));
        let mut used = vec![false; sides.len()];
        let mut found: Vec<Column> = Vec::new();
        for (k, si) in order.into_iter().enumerate() {
            if self.stop_at(k) {
                return None;
            }
            let Some(side) = sides.get(si) else { continue };
            if used.get(si).copied().unwrap_or(true) {
                continue;
            }
            // The side its samples look across at most.
            let mut tally: Vec<(usize, usize)> = Vec::new();
            for &id in &side.samples {
                let Some(o) = self.samples.get(id).and_then(|s| s.opposite) else { continue };
                let Some(other) = self.nearest_sample(o.ring, o.s).and_then(|j| side_of.get(j).copied().flatten()) else { continue };
                match tally.iter_mut().find(|(s, _)| *s == other) {
                    Some((_, c)) => *c += 1,
                    None => tally.push((other, 1)),
                }
            }
            // Most looked across at first; of two looked at as often, the one met last.
            tally.reverse();
            tally.sort_by_key(|&(_, c)| std::cmp::Reverse(c));
            let least = MIN_PAIRED * side.samples.len() as f64;
            for (other, count) in tally {
                if (count as f64) < least {
                    break;
                }
                if other == si || used.get(other).copied().unwrap_or(true) {
                    continue;
                }
                let Some(across) = sides.get(other) else { continue };
                let Some(column) = self.column(side, across) else { continue };
                let pieces: Vec<Column> = self
                    .runs_on(column)
                    .into_iter()
                    .filter(|c| self.takes(c))
                    .map(|c| if c.closed { c } else { Column { tips: self.tips(&c.stations), ..c } })
                    .collect();
                if pieces.is_empty() {
                    continue;
                }
                for i in [si, other] {
                    if let Some(u) = used.get_mut(i) {
                        *u = true;
                    }
                }
                found.extend(pieces);
                break;
            }
        }
        found.sort_by(|a, b| b.length().total_cmp(&a.length()));
        // Each kept column with its outline and the outline's bounds: a middle outside the bounds
        // is outside the outline, which spares the winding test of every middle against every
        // column kept before it.
        let mut kept: Vec<(Column, BezPath, Rect)> = Vec::new();
        for column in found {
            let middles = column.middles();
            let inside = middles.iter().filter(|m| kept.iter().any(|(_, o, bounds)| bounds.contains(**m) && o.winding(**m) != 0)).count();
            if (inside as f64) <= MAX_OVERLAP * middles.len() as f64 {
                let outline = column.outline();
                let bounds = outline.bounding_box().inflate(SAME_POINT, SAME_POINT);
                kept.push((column, outline, bounds));
            }
        }
        Some(kept.into_iter().map(|(c, ..)| c).collect())
    }

    /// Whether `column` is one: at least [`MIN_SIDE`] stations, [`MIN_ASPECT`] times as long as
    /// it is typically wide, and typically no wider than the limit.
    fn takes(&self, column: &Column) -> bool {
        let (length, width) = (column.length(), column.width());
        column.stations.len() >= MIN_SIDE && length >= MIN_ASPECT * width && width <= self.max_width
    }

    /// Whether the shape ends past each end of the open column `stations` ([`Column::tips`]): a
    /// point [`END_PROBE`] on from its end station's middle, the way the column runs there, is
    /// outside the shape. (When the way it runs is not to be had, it is taken to end there.)
    fn tips(&self, stations: &[(Point, Point)]) -> [bool; 2] {
        // `m` in order from the end looked past.
        let ends = |m: &[Point]| -> bool {
            let Some((&last, before)) = m.split_first() else { return true };
            // The way it runs at its end: from a middle at least a sample back.
            let Some(&back) = before.iter().find(|q| q.distance(last) >= SAMPLE_STEP) else { return true };
            let Some(way) = unit(last - back) else { return true };
            !self.shape.contains(last + way * END_PROBE)
        };
        let mut middles: Vec<Point> = stations.iter().map(|(a, b)| a.midpoint(*b)).collect();
        let start = ends(&middles);
        middles.reverse();
        [start, ends(&middles)]
    }

    /// `column` without the stretches where it would run on wider than the limit for longer than
    /// [`RUN_ON_LENGTH`] times its typical width within the limit: each such stretch is cut out,
    /// and the parts either side are columns of their own (open). A column with no such stretch
    /// comes back whole.
    fn runs_on(&self, column: Column) -> Vec<Column> {
        // (A width that is not a number counts as too wide.)
        let over = |(a, b): &(Point, Point)| {
            let w = a.distance(*b);
            w > self.max_width || w.is_nan()
        };
        if !column.stations.iter().any(over) {
            return vec![column];
        }
        let mut within: Vec<f64> = column.stations.iter().filter(|s| !over(s)).map(|(a, b)| a.distance(*b)).collect();
        within.sort_by(f64::total_cmp);
        let longest = RUN_ON_LENGTH * within.get(within.len() / 2).copied().unwrap_or(0.0);
        let Column { mut stations, closed, .. } = column;
        // A ring is walked from a station within the limit just after one beyond it, so that no
        // stretch of either kind goes round past its start.
        let start = if closed {
            let n = stations.len();
            let start = (0..n).find(|&i| stations.get(i).is_some_and(|s| !over(s)) && stations.get((i + n - 1) % n).is_some_and(over)).unwrap_or(0);
            stations.rotate_left(start);
            start
        } else {
            0
        };
        // Distance along the middle to each station.
        let at = Along::new(&stations, false).at;
        let n = stations.len();
        let mut pieces: Vec<Vec<(Point, Point)>> = vec![Vec::new()];
        let mut cut = false;
        let mut i = 0;
        while i < n {
            let first = i;
            let wide = stations.get(i).is_some_and(over);
            while i < n && stations.get(i).is_some_and(over) == wide {
                i += 1;
            }
            let stretch = stations.get(first..i).unwrap_or_default();
            let length = at.get(i - 1).copied().unwrap_or(0.0) - at.get(first).copied().unwrap_or(0.0);
            if wide && (length > longest || length.is_nan()) {
                cut = true;
                pieces.push(Vec::new());
            } else if let Some(piece) = pieces.last_mut() {
                piece.extend_from_slice(stretch);
            }
        }
        if !cut {
            // (Round a ring, from its own start again.)
            stations.rotate_right(start);
            return vec![Column { stations, closed, ..Column::default() }];
        }
        pieces.into_iter().filter(|s| !s.is_empty()).map(|stations| Column { stations, ..Column::default() }).collect()
    }

    /// The sample of ring `ring` nearest `s` along it (as drawn).
    fn nearest_sample(&self, ring: usize, s: f64) -> Option<usize> {
        let ids = self.by_ring.get(ring)?;
        let r = self.rings.get(ring)?.as_ref()?;
        let n = ids.len();
        if n == 0 {
            return None;
        }
        // Samples sit every step as drawn; the list is in walked order.
        let j = ((s.rem_euclid(r.len) / self.step).round() as usize) % n;
        let j = if r.flipped { n - 1 - j } else { j };
        ids.get(j).copied()
    }

    /// The column between side `a` and side `b` opposite it: the pairs of opposite points both
    /// sides found, in order along the column and kept only where both move on (so the matching
    /// never folds back), then its open ends carried round a tip.
    fn column(&self, a: &Side, b: &Side) -> Option<Column> {
        let ra = self.rings.get(a.ring)?.as_ref()?;
        let rb = self.rings.get(b.ring)?.as_ref()?;
        let first = self.samples.get(*a.samples.first()?)?;
        let (a0, b0) = (ra.walk(first.s), rb.walk(first.opposite?.s));
        // Positions on from the column's start: along side a forwards, along side b backwards
        // (the inside on one side of both, so the two run opposite ways). A little before the
        // start counts as before it, not as all the way round.
        let tol = STEP_SAMPLES * self.step;
        let closed = a.whole && b.whole;
        // (Round a ring, every position counts on from the start.)
        let wrap = |x: f64, len: f64| if !closed && x > len - tol { x - len } else { x };
        let on_a = |w: f64| wrap(ra.ahead(a0, w), ra.len);
        let on_b = |w: f64| wrap(rb.ahead(w, b0), rb.len);
        let mut pairs: Vec<(f64, f64)> = Vec::with_capacity(a.samples.len() + b.samples.len());
        for &id in &a.samples {
            let Some(s) = self.samples.get(id) else { continue };
            let Some(o) = s.opposite.filter(|o| o.ring == b.ring) else { continue };
            pairs.push((on_a(ra.walk(s.s)), on_b(rb.walk(o.s))));
        }
        let (end_a, end_b) = pairs.iter().fold((0.0f64, 0.0f64), |(x, y), p| (x.max(p.0), y.max(p.1)));
        for &id in &b.samples {
            let Some(s) = self.samples.get(id) else { continue };
            let Some(o) = s.opposite.filter(|o| o.ring == a.ring) else { continue };
            let pair = (on_a(ra.walk(o.s)), on_b(rb.walk(s.s)));
            // Within side a's stretch (side b may go on past it, towards another column).
            if a.whole || (pair.0 >= -tol && pair.0 <= end_a + tol && pair.1 >= -tol && pair.1 <= end_b + tol) {
                pairs.push(pair);
            }
        }
        pairs.sort_by(|x, y| (x.0 + x.1).total_cmp(&(y.0 + y.1)));
        let mut kept: Vec<(f64, f64)> = Vec::with_capacity(pairs.len());
        for p in pairs {
            match kept.last() {
                Some(&(x, y)) if p.0 < x || p.1 < y => {}
                Some(&(x, y)) if (p.0 - x) + (p.1 - y) < SAME_POINT => {}
                _ => kept.push(p),
            }
        }
        let mut stations: Vec<(Point, Point)> = kept.iter().filter_map(|&(x, y)| Some((ra.at_walk(a0 + x)?, rb.at_walk(b0 - y)?))).collect();
        if !closed && a.ring == b.ring {
            let (x, y) = *kept.last()?;
            let end = self.tip(ra, a0 + x, b0 - y, false);
            let (x, y) = *kept.first()?;
            let start = self.tip(ra, a0 + x, b0 - y, true);
            stations.extend(end);
            stations.splice(0..0, start.into_iter().rev());
        }
        Some(Column { stations, closed, tips: [false; 2] })
    }

    /// Stations that carry a column's end round its tip, from its last station (side a at walked
    /// position `wa`, side b at `wb`) on, or before its first when `before`: the stretch of the
    /// outline between the two sides there, when it is short ([`END_STRETCH`]), walked from both
    /// ends towards its middle, each station kept while its middle lies inside the shape (a round
    /// or pointed end narrows to the tip; a square end's corners are reached and it stops there).
    fn tip(&self, ring: &Ring<'_>, wa: f64, wb: f64, before: bool) -> Vec<(Point, Point)> {
        // The way on from each side: at the end side a walks on and side b back; before the start
        // the other way round.
        let (from, to) = if before { (wb, wa) } else { (wa, wb) };
        let stretch = ring.ahead(from, to);
        let width = match (ring.at_walk(wa), ring.at_walk(wb)) {
            (Some(p), Some(q)) => p.distance(q),
            _ => return Vec::new(),
        };
        // (A pointed tip is narrow where the sides stop, short of it by about TIP_ARC.)
        if !(stretch > 0.0 && stretch <= END_STRETCH * width.max(TIP_ARC)) {
            return Vec::new();
        }
        let sign = if before { -1.0 } else { 1.0 };
        let mut out: Vec<(Point, Point)> = Vec::new();
        // A square end: corners just past each side's end are where the column's sides end.
        let corners = ring.corners();
        let near = |w: f64, dir: f64| {
            corners
                .iter()
                .map(|&c| (c, if dir > 0.0 { ring.ahead(w, c) } else { ring.ahead(c, w) }))
                .filter(|&(_, d)| d > 0.0 && d <= MUTUAL_SAMPLES * self.step)
                .min_by(|x, y| x.1.total_cmp(&y.1))
        };
        let (mut x, mut y) = (0.0, 0.0);
        if let (Some((ca, da)), Some((cb, db))) = (near(wa, sign), near(wb, -sign))
            && let (Some(p), Some(q)) = (ring.at_walk(ca), ring.at_walk(cb))
            && p.distance(q) <= self.max_width * RUNS_ON
        {
            out.push((p, q));
            (x, y) = (da, db);
        }
        loop {
            let (nx, ny) = (x + self.step, y + self.step);
            if nx + ny >= stretch {
                break;
            }
            let (Some(p), Some(q)) = (ring.at_walk(wa + sign * nx), ring.at_walk(wb - sign * ny)) else { break };
            let middle = p.midpoint(q);
            let width = p.distance(q);
            if width < PROBE || width > self.max_width * RUNS_ON || !self.shape.contains(middle) || self.shape.on_outline(middle) {
                break;
            }
            out.push((p, q));
            (x, y) = (nx, ny);
        }
        out
    }
}

/// Where segment `a`–`b` meets segment `c`–`d`: the share of the way along each, when they meet.
fn intersect(a: Point, b: Point, c: Point, d: Point) -> Option<(f64, f64)> {
    /// Below this the two are parallel (square millimetres: the cross product of the segments).
    const PARALLEL: f64 = 1e-12;
    let (ab, cd) = (b - a, d - c);
    let denom = ab.cross(cd);
    if !denom.is_finite() || denom.abs() < PARALLEL {
        return None;
    }
    let t = (c - a).cross(cd) / denom;
    let u = (c - a).cross(ab) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}
