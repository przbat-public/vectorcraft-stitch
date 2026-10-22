//! A filled area: what is inside it, where a row crosses it, and the ways the needle can travel
//! inside it from one place to another ([`Travels`]).

use vectorcraft_geom::{FillRule, Point, Vec2};

use super::geometry::{Axes, NO_LENGTH, SubPath, unit};
use crate::SAME_POINT;

/// A travel run's stitch length (millimetres): short, so it follows curves and hides under the
/// fill. An estimate.
pub(super) const TRAVEL_STITCH: f64 = 2.0;
/// How far inside the shape a travel passes its corners (millimetres), so it doesn't show at the
/// edge. An estimate.
const TRAVEL_MARGIN: f64 = 0.5;
/// How closely a travel's graph follows the outlines (millimetres), and the most corners it may
/// have: enough for any real outline, a bound on the work a travel costs.
const TRAVEL_SIMPLIFY: f64 = 0.3;
const MAX_TRAVEL_NODES: usize = 400;

/// A filled shape: its closed outlines (holes included) and the rule that decides what they
/// enclose; with the outlines' [`Grid`], whether a point is inside and whether a segment stays
/// inside look only at the edges near them ([`Area::spans`], once a row, looks at every edge).
#[derive(Clone, Copy)]
pub(super) struct Area<'a> {
    pub(super) outlines: &'a [&'a SubPath],
    pub(super) rule: FillRule,
    pub(super) grid: Option<&'a Grid>,
}

impl Area<'_> {
    /// The edges of every outline, each from a point to the next (the last back to the first).
    fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        edges(self.outlines)
    }

    /// Whether `hit` holds for an edge that segment `a`–`b` may meet: with the grid, the edges in
    /// the cells the segment passes (an edge in several of them may come more than once); without,
    /// every edge.
    fn any_edge_along(&self, a: Point, b: Point, mut hit: impl FnMut(Point, Point) -> bool) -> bool {
        match self.grid {
            Some(grid) => grid.any_along(a, b, hit),
            None => self.edges().any(|(c, d)| hit(c, d)),
        }
    }

    /// Whether `q` is inside.
    pub(super) fn contains(&self, q: Point) -> bool {
        let winding = match self.grid {
            Some(grid) => winding(grid.right_of(q).into_iter(), q),
            None => winding(self.edges(), q),
        };
        match self.rule {
            FillRule::NonZero => winding != 0,
            FillRule::EvenOdd => winding % 2 != 0,
        }
    }

    /// Whether the straight line from `a` to `b` stays inside: it crosses no edge (touching one
    /// at its ends is fine: row ends lie on the edge) and its middle is inside, or it runs along an
    /// edge (the outline belongs to the area: a turn between two rows that end on one straight
    /// edge follows it, and the inside test can't tell a point on the edge). A line whose middle
    /// lies on the outline is tested stretch by stretch between the corners it passes: along two
    /// edges in line (the bottoms of two squares side by side) it would otherwise cross the gap
    /// between them unseen, since no edge crosses it.
    pub(super) fn contains_segment(&self, a: Point, b: Point) -> bool {
        if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
            return false;
        }
        if a.distance(b) < SAME_POINT {
            return self.contains(a);
        }
        // The corners of the outline the line passes, as shares of it, noted while looking for a
        // crossing.
        let ab = b - a;
        let len2 = ab.hypot2();
        let mut cuts: Vec<f64> = vec![0.0, 1.0];
        let crosses = self.any_edge_along(a, b, |c, d| {
            for corner in [c, d] {
                if near_segment(corner, a, b) {
                    cuts.push(((corner - a).dot(ab) / len2).clamp(0.0, 1.0));
                }
            }
            crossing(a, b, c, d)
        });
        if crosses {
            return false;
        }
        if self.along_edge(a, b) {
            return true;
        }
        let middle = a.midpoint(b);
        // A corner passed inside the line (not at its ends): the line may leave there without
        // crossing an edge (a corner's contacts count for neither edge), so it is tested stretch by
        // stretch, as one along the outline is.
        let through_corner = cuts.iter().any(|t| *t > CORNER_INSIDE && *t < 1.0 - CORNER_INSIDE);
        if !through_corner && !self.on_outline(middle) {
            return self.contains(middle);
        }
        // Where the line passes a corner of the outline, it may leave it: every stretch between
        // two such corners must lie on the outline or inside.
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < f64::EPSILON);
        cuts.windows(2).all(|w| {
            let [t0, t1] = w else { return true };
            let q = a + ab * ((t0 + t1) / 2.0);
            self.on_outline(q) || self.contains(q)
        })
    }

    /// `visit` called with every edge that segment `a`–`b` may meet (and maybe others), with its
    /// place in the outlines' list of edges (outline by outline, each from a point to the next):
    /// with the grid, the edges in the cells the segment passes; without, all of them. An edge may
    /// come more than once.
    pub(super) fn edges_along(&self, a: Point, b: Point, mut visit: impl FnMut(usize, Point, Point)) {
        match self.grid {
            Some(grid) => {
                grid.any_along_index(a, b, |i| {
                    if let (Some(&id), Some(&(c, d))) = (grid.ids.get(i), grid.edges.get(i)) {
                        visit(id, c, d);
                    }
                    false
                });
            }
            None => self.edges().enumerate().for_each(|(id, (c, d))| visit(id, c, d)),
        }
    }

    /// Whether `q` lies on one of the outlines' edges (within [`ON_EDGE`]).
    pub(super) fn on_outline(&self, q: Point) -> bool {
        self.any_edge_along(q, q, |c, d| near_segment(q, c, d))
    }

    /// Whether the segment from `a` to `b` lies on one of the outlines' edges, both its ends within
    /// [`ON_EDGE`] of it.
    fn along_edge(&self, a: Point, b: Point) -> bool {
        self.any_edge_along(a, b, |c, d| near_segment(a, c, d) && near_segment(b, c, d))
    }

    /// Where the outlines' points fall along `v`: the lowest and highest, `None` when none is
    /// finite.
    pub(super) fn extent(&self, v: Vec2) -> Option<(f64, f64)> {
        let (lo, hi) = self
            .outlines
            .iter()
            .flat_map(|s| s.pts.iter())
            .map(|q| q.to_vec2().dot(v))
            .filter(|t| t.is_finite())
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
        (lo <= hi).then_some((lo, hi))
    }

    /// Where the row at `t` (across `axes`) crosses the area: the stretches along it that are
    /// inside, by the fill rule.
    pub(super) fn spans(&self, axes: Axes, t: f64) -> Vec<(f64, f64)> {
        let mut crossings: Vec<(f64, i32)> = Vec::new();
        for (a, b) in self.edges() {
            let (ta, tb) = (axes.across(a), axes.across(b));
            // Half-open: a vertex exactly on the row counts once.
            if (ta > t) == (tb > t) {
                continue;
            }
            let f = (t - ta) / (tb - ta);
            if !f.is_finite() {
                continue;
            }
            let s = axes.along(a) + f * (axes.along(b) - axes.along(a));
            crossings.push((s, if tb > ta { 1 } else { -1 }));
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut spans: Vec<(f64, f64)> = Vec::new();
        match self.rule {
            FillRule::EvenOdd => {
                for pair in crossings.chunks(2) {
                    if let [a, b] = pair
                        && b.0 - a.0 > SAME_POINT
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
                    } else if before != 0 && winding == 0 && s - start > SAME_POINT {
                        spans.push((start, s));
                    }
                }
            }
        }
        spans
    }

    /// Corner `i` of outline `corners` moved [`TRAVEL_MARGIN`] inside along the bisector of its two
    /// edges, so that a travel past it runs inside; `None` when neither side of it is inside (a
    /// sliver).
    fn corner_inside(&self, corners: &[Point], i: usize) -> Option<Point> {
        let n = corners.len();
        let (a, q, b) = (*corners.get((i + n - 1) % n)?, *corners.get(i)?, *corners.get((i + 1) % n)?);
        let (u, v) = (unit(q - a)?, unit(b - q)?);
        let across = unit(Vec2::new(-(u.y + v.y), u.x + v.x))?;
        [across, -across].into_iter().map(|side| q + side * TRAVEL_MARGIN).find(|&moved| self.contains(moved))
    }

    /// Whether corner `i` of outline `corners` is reflex: the inside wraps round it by more than a
    /// half turn (a notch in the outline, any corner of a hole). Only there can a shortest way
    /// inside the shape turn; at a convex corner it would cut across.
    fn reflex(&self, corners: &[Point], i: usize) -> bool {
        let n = corners.len();
        let (Some(&a), Some(&q), Some(&b)) = (corners.get((i + n - 1) % n), corners.get(i), corners.get((i + 1) % n)) else { return false };
        let (Some(u), Some(v)) = (unit(a - q), unit(b - q)) else { return false };
        // Between the two edges, just by the corner: inside at a convex corner, outside at a reflex
        // one. (Two edges folded back on each other: a spike, kept.)
        let between = q + (u + v) * (TRAVEL_MARGIN / 2.0);
        between.distance(q) < SAME_POINT || !self.contains(between)
    }
}

/// The ways a travel can take inside one area, found on a visibility graph: the outlines' reflex
/// corners (simplified to [`TRAVEL_SIMPLIFY`]; a shortest way turns at no other) moved
/// [`TRAVEL_MARGIN`] inside, linked where the straight
/// line between two of them stays inside, searched with Dijkstra's algorithm (as Ink/Stitch's
/// "underpath" travels inside a shape). The corners, and whether two of them see each other, are
/// worked out once per area as travels first need them: a fill makes many travels in one shape.
pub(super) struct Travels<'a> {
    pub(super) area: Area<'a>,
    /// The graph's corners, once built; empty when the outlines have more than
    /// [`MAX_TRAVEL_NODES`] (then only straight travels are made).
    corners: Option<Vec<Point>>,
    /// Whether two corners see each other ([`UNKNOWN`], [`SEES`] or [`BLOCKED`]; row-major, the
    /// lower index first).
    sees: Vec<u8>,
}

/// Whether two of a travel graph's corners see each other: not worked out yet, yes (the straight
/// line between them stays inside), or no.
const UNKNOWN: u8 = 0;
const SEES: u8 = 1;
const BLOCKED: u8 = 2;

impl<'a> Travels<'a> {
    pub(super) fn new(area: Area<'a>) -> Self {
        Self { area, corners: None, sees: Vec::new() }
    }

    /// The graph's corners, built the first time: how many there are.
    fn corners(&mut self) -> usize {
        if self.corners.is_none() {
            let mut nodes: Vec<Point> = Vec::new();
            for sub in self.area.outlines {
                let corners = simplify(&sub.pts, TRAVEL_SIMPLIFY);
                nodes.extend((0..corners.len()).filter(|&i| self.area.reflex(&corners, i)).filter_map(|i| self.area.corner_inside(&corners, i)));
                if nodes.len() > MAX_TRAVEL_NODES {
                    nodes.clear();
                    break;
                }
            }
            self.sees = vec![UNKNOWN; nodes.len() * nodes.len()];
            self.corners = Some(nodes);
        }
        self.corners.as_ref().map_or(0, Vec::len)
    }

    fn corner(&self, i: usize) -> Option<Point> {
        self.corners.as_ref().and_then(|c| c.get(i)).copied()
    }

    /// Whether corners `i` and `j` see each other (worked out once).
    fn corners_see(&mut self, i: usize, j: usize) -> bool {
        let n = self.corners.as_ref().map_or(0, Vec::len);
        let (lo, hi) = (i.min(j), i.max(j));
        let k = lo * n + hi;
        match self.sees.get(k).copied() {
            Some(SEES) => true,
            Some(BLOCKED) => false,
            Some(_) => {
                let (Some(a), Some(b)) = (self.corner(lo), self.corner(hi)) else { return false };
                let yes = self.area.contains_segment(a, b);
                if let Some(v) = self.sees.get_mut(k) {
                    *v = if yes { SEES } else { BLOCKED };
                }
                yes
            }
            None => false,
        }
    }

    /// The shortest way from `a` to `b` that stays inside the area (straight when that does), as
    /// the points it turns at, both ends included; `None` when there is none (two separate
    /// shapes).
    pub(super) fn way(&mut self, a: Point, b: Point) -> Option<Vec<Point>> {
        if !a.distance(b).is_finite() {
            return None;
        }
        if self.area.contains_segment(a, b) {
            return Some(vec![a, b]);
        }
        let n = self.corners();
        // Node 0 is `a`, node 1 is `b`, node 2 + i is corner i; what the ends see is tested once.
        let sees_a: Vec<bool> = (0..n).map(|i| self.corner(i).is_some_and(|c| self.area.contains_segment(a, c))).collect();
        let sees_b: Vec<bool> = (0..n).map(|i| self.corner(i).is_some_and(|c| self.area.contains_segment(c, b))).collect();
        let total = n + 2;
        let at = |t: &Self, u: usize| match u {
            0 => Some(a),
            1 => Some(b),
            _ => t.corner(u - 2),
        };
        let mut dist = vec![f64::INFINITY; total];
        let mut prev: Vec<Option<usize>> = vec![None; total];
        let mut done = vec![false; total];
        *dist.first_mut()? = 0.0;
        loop {
            // The nearest node not done yet.
            let u = (0..total)
                .filter(|&i| !done.get(i).copied().unwrap_or(true))
                .min_by(|&i, &j| dist.get(i).unwrap_or(&f64::INFINITY).total_cmp(dist.get(j).unwrap_or(&f64::INFINITY)))?;
            let (du, pu) = (*dist.get(u)?, at(self, u)?);
            if !du.is_finite() {
                return None;
            }
            *done.get_mut(u)? = true;
            if u == 1 {
                break;
            }
            for v in 1..total {
                if done.get(v).copied().unwrap_or(true) {
                    continue;
                }
                let Some(pv) = at(self, v) else { continue };
                let dv = du + pu.distance(pv);
                if dv >= dist.get(v).copied().unwrap_or(f64::INFINITY) {
                    continue;
                }
                let sees = match (u, v) {
                    // Straight from end to end was tested first.
                    (0, 1) => false,
                    (0, c) => sees_a.get(c - 2).copied().unwrap_or(false),
                    (c, 1) => sees_b.get(c - 2).copied().unwrap_or(false),
                    (c, d) => self.corners_see(c - 2, d - 2),
                };
                if sees {
                    *dist.get_mut(v)? = dv;
                    *prev.get_mut(v)? = Some(u);
                }
            }
        }
        let mut route: Vec<Point> = vec![b];
        let mut node = 1;
        while let Some(Some(u)) = prev.get(node) {
            route.push(at(self, *u)?);
            node = *u;
            if route.len() > total {
                return None;
            }
        }
        route.reverse();
        Some(route)
    }
}

/// The edges of `outlines`, each from a point to the next (the last back to the first).
fn edges<'a>(outlines: &'a [&'a SubPath]) -> impl Iterator<Item = (Point, Point)> + 'a {
    outlines.iter().flat_map(|sub| {
        let n = sub.pts.len();
        (0..n).filter_map(move |i| Some((*sub.pts.get(i)?, *sub.pts.get((i + 1) % n)?)))
    })
}

/// The winding number of `edges` round `q`: each edge crossing the horizontal line through `q` to
/// its right counts one, plus or minus by its direction. Edges that don't cross it count nothing,
/// so any set that includes every crossing one will do; none may come twice.
fn winding(edges: impl Iterator<Item = (Point, Point)>, q: Point) -> i32 {
    let mut winding = 0i32;
    for (a, b) in edges {
        let side = (b - a).cross(q - a);
        if a.y <= q.y {
            if b.y > q.y && side > 0.0 {
                winding += 1;
            }
        } else if b.y <= q.y && side < 0.0 {
            winding -= 1;
        }
    }
    winding
}

/// How near a point must be to an edge to lie on it (millimetres): row ends are computed on the
/// edges, so they lie within rounding of them.
const ON_EDGE: f64 = 1e-6;
/// How far inside a line (a share of its length) a corner it passes counts as passed inside it,
/// not at its ends, where row ends and corners lie within rounding.
const CORNER_INSIDE: f64 = 1e-6;

/// Whether `q` lies within [`ON_EDGE`] of the segment from `c` to `d`.
fn near_segment(q: Point, c: Point, d: Point) -> bool {
    let cd = d - c;
    let len2 = cd.hypot2();
    if !len2.is_finite() || len2 < SAME_POINT * SAME_POINT {
        return q.distance(c) < ON_EDGE;
    }
    let t = ((q - c).dot(cd) / len2).clamp(0.0, 1.0);
    q.distance(c + cd * t) < ON_EDGE
}

/// Whether segment `a`–`b` crosses edge `c`–`d` (touching it at `a` or `b` does not count).
fn crossing(a: Point, b: Point, c: Point, d: Point) -> bool {
    /// Below this the two are parallel (square millimetres: the cross product of the two
    /// segments), and parallel segments never cross.
    const PARALLEL: f64 = 1e-12;
    /// How near its ends (a share of its length) the tested segment may meet the edge without
    /// crossing it: its ends lie on edges (row ends, corners) within rounding.
    const SEGMENT_END: f64 = 1e-6;
    /// How near the edge's ends (a share of its length) a meeting doesn't count: the segment
    /// passes through the corner there rather than across the edge.
    const EDGE_END: f64 = 1e-9;
    let (ab, cd) = (b - a, d - c);
    let denom = ab.cross(cd);
    if denom.abs() < PARALLEL {
        return false;
    }
    // Where the two lines meet, as a share of each segment.
    let t = (c - a).cross(cd) / denom;
    let u = (c - a).cross(ab) / denom;
    t > SEGMENT_END && t < 1.0 - SEGMENT_END && u > EDGE_END && u < 1.0 - EDGE_END
}

/// The outlines' edges sorted into square cells, so that a test along a segment or a ray looks
/// only at the edges in the cells it passes: a fill tests thousands of segments (row turns,
/// travels, the travel graph) against outlines of thousands of edges.
pub(super) struct Grid {
    x0: f64,
    y0: f64,
    cell: f64,
    nx: usize,
    ny: usize,
    /// The edges in each cell (row-major), as indices into `edges`.
    cells: Vec<Vec<u32>>,
    edges: Vec<(Point, Point)>,
    /// Each edge's place in the outlines' own list of edges ([`Area::edges_along`]).
    ids: Vec<usize>,
}

impl Grid {
    /// The most cells along a side.
    const MAX_SIDE: usize = 256;
    /// The most cells an edge takes on average (on top of one entry per cell): past that, the
    /// grid is not built and tests look at every edge.
    const ENTRIES_PER_EDGE: usize = 32;
    /// How many edges a cell holds on average: the cell size is chosen for it.
    const EDGES_PER_CELL: f64 = 2.0;
    /// The thinnest an outline's extent counts as (millimetres): a flat one still gets cells.
    const MIN_EXTENT: f64 = 1e-6;
    /// How far each edge reaches past the cells it passes, as a share of a cell, so that a point on
    /// a cell's border finds the edge from either side.
    const HAIR: f64 = 1e-6;

    /// The grid of `outlines`' edges; `None` when they have no finite extent, or when their edges
    /// are so long for their number that the grid would cost more than it saves.
    pub(super) fn new(outlines: &[&SubPath]) -> Option<Self> {
        let (ids, edges): (Vec<usize>, Vec<(Point, Point)>) = edges(outlines)
            .enumerate()
            .filter(|(_, (a, b))| a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite())
            .take(u32::MAX as usize)
            .unzip();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (a, _) in &edges {
            (x0, y0, x1, y1) = (x0.min(a.x), y0.min(a.y), x1.max(a.x), y1.max(a.y));
        }
        let (w, h) = (x1 - x0, y1 - y0);
        if !(w.is_finite() && h.is_finite()) || edges.is_empty() {
            return None;
        }
        // About EDGES_PER_CELL edges per cell, never more than MAX_SIDE cells a side; a flat
        // outline (no height or no width) counts as MIN_EXTENT thick.
        let side = w.max(h).max(Self::MIN_EXTENT);
        let cell = ((w.max(Self::MIN_EXTENT) * h.max(Self::MIN_EXTENT) * Self::EDGES_PER_CELL / edges.len() as f64).sqrt())
            .max(side / Self::MAX_SIDE as f64);
        let nx = ((w / cell).floor() as usize + 1).min(Self::MAX_SIDE);
        let ny = ((h / cell).floor() as usize + 1).min(Self::MAX_SIDE);
        let mut grid = Self { x0, y0, cell, nx, ny, cells: vec![Vec::new(); nx * ny], edges: Vec::new(), ids };
        // Each edge goes in the cells it passes through, row by row: in each row, the cells under
        // the part of the edge within the row's band. Everything is grown a hair so that a point
        // on a cell's border finds the edge from either side. An edge's own box would be simpler,
        // but a long diagonal edge fills its whole box: thousands of edges across a shape would
        // take gigabytes.
        let hair = cell * Self::HAIR;
        let budget = edges.len().saturating_mul(Self::ENTRIES_PER_EDGE).saturating_add(nx * ny);
        let mut entries = 0usize;
        for (i, &(a, b)) in edges.iter().enumerate() {
            let (r0, r1) = (grid.row(a.y.min(b.y) - hair), grid.row(a.y.max(b.y) + hair));
            for r in r0..=r1 {
                // The row's band; the first and last rows reach out, as `row` clamps to them.
                let lo = if r == 0 { f64::NEG_INFINITY } else { y0 + cell * r as f64 - hair };
                let hi = if r + 1 >= ny { f64::INFINITY } else { y0 + cell * (r + 1) as f64 + hair };
                let (xa, xb) = if (b.y - a.y).abs() < NO_LENGTH {
                    (a.x, b.x)
                } else {
                    let at = |y: f64| a.x + (b.x - a.x) * ((y - a.y) / (b.y - a.y)).clamp(0.0, 1.0);
                    (at(lo.max(a.y.min(b.y))), at(hi.min(a.y.max(b.y))))
                };
                let (c0, c1) = (grid.col(xa.min(xb) - hair), grid.col(xa.max(xb) + hair));
                entries = entries.saturating_add(c1.saturating_sub(c0) + 1);
                if entries > budget {
                    // Edges this long for their number: testing every edge costs less.
                    return None;
                }
                for c in c0..=c1 {
                    if let Some(cell) = grid.cells.get_mut(r * nx + c) {
                        cell.push(i as u32);
                    }
                }
            }
        }
        grid.edges = edges;
        Some(grid)
    }

    /// How many edge entries the cells hold.
    #[cfg(test)]
    pub(super) fn entries(&self) -> usize {
        self.cells.iter().map(Vec::len).sum()
    }

    fn col(&self, x: f64) -> usize {
        (((x - self.x0) / self.cell).floor().max(0.0) as usize).min(self.nx - 1)
    }

    fn row(&self, y: f64) -> usize {
        (((y - self.y0) / self.cell).floor().max(0.0) as usize).min(self.ny - 1)
    }

    /// Every edge that may cross the horizontal line through `q` to its right (each once).
    fn right_of(&self, q: Point) -> Vec<(Point, Point)> {
        if !(q.x.is_finite() && q.y.is_finite()) || q.y < self.y0 || q.y > self.y0 + self.cell * self.ny as f64 {
            return Vec::new();
        }
        let r = self.row(q.y);
        let mut ids: Vec<u32> = (self.col(q.x)..self.nx).filter_map(|c| self.cells.get(r * self.nx + c)).flatten().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter().filter_map(|i| self.edges.get(i as usize).copied()).collect()
    }

    /// Whether `hit` holds for any edge in the cells segment `a`–`b` passes through (walked cell by
    /// cell, Amanatides and Woo's traversal; an edge in several of them may come more than once).
    fn any_along(&self, a: Point, b: Point, mut hit: impl FnMut(Point, Point) -> bool) -> bool {
        self.any_along_index(a, b, |i| self.edges.get(i).is_some_and(|&(p, q)| hit(p, q)))
    }

    /// [`Grid::any_along`], `hit` given each edge's index in the grid's list.
    fn any_along_index(&self, a: Point, b: Point, mut hit: impl FnMut(usize) -> bool) -> bool {
        let (mut c, mut r) = (self.col(a.x), self.row(a.y));
        let (ec, er) = (self.col(b.x), self.row(b.y));
        let d = b - a;
        let next = |at: f64, origin: f64, dir: f64, k: usize| -> (f64, f64) {
            // The segment doesn't move this way (millimetres): it never crosses a cell border.
            const STILL: f64 = 1e-15;
            if dir.abs() < STILL {
                return (f64::INFINITY, f64::INFINITY);
            }
            let edge = origin + self.cell * (k as f64 + if dir > 0.0 { 1.0 } else { 0.0 });
            (((edge - at) / dir).max(0.0), self.cell / dir.abs())
        };
        let (mut tx, dx) = next(a.x, self.x0, d.x, c);
        let (mut ty, dy) = next(a.y, self.y0, d.y, r);
        for _ in 0..(self.nx + self.ny + 2) {
            let found = self.cells.get(r * self.nx + c).into_iter().flatten().any(|&i| hit(i as usize));
            if found {
                return true;
            }
            if c == ec && r == er {
                return false;
            }
            if tx < ty {
                tx += dx;
                c = if d.x > 0.0 { c + 1 } else { c.wrapping_sub(1) };
            } else {
                ty += dy;
                r = if d.y > 0.0 { r + 1 } else { r.wrapping_sub(1) };
            }
            if c >= self.nx || r >= self.ny {
                return false;
            }
        }
        // The walk should have reached the end's cell: look at every edge to be sure.
        (0..self.edges.len()).any(hit)
    }
}

/// A closed polyline simplified to within `tolerance` (Douglas–Peucker), so that a travel's graph
/// has the shape's corners, not every point a curve was flattened to.
fn simplify(pts: &[Point], tolerance: f64) -> Vec<Point> {
    let n = pts.len();
    if n <= 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    // Split the closed loop at its first point and the point farthest from it.
    let Some(&start) = pts.first() else { return Vec::new() };
    let farthest = (1..n)
        .max_by(|&i, &j| pts.get(i).map_or(0.0, |p| p.distance(start)).total_cmp(&pts.get(j).map_or(0.0, |p| p.distance(start))))
        .unwrap_or(n / 2);
    for k in [0, farthest] {
        if let Some(keep) = keep.get_mut(k) {
            *keep = true;
        }
    }
    let mut stack: Vec<(usize, usize)> = vec![(0, farthest), (farthest, n)];
    while let Some((i, j)) = stack.pop() {
        let (Some(&p), Some(&q)) = (pts.get(i), pts.get(j % n)) else { continue };
        let pq = q - p;
        let len = pq.hypot();
        // The point between the two farthest from the line through them.
        let far = (i + 1..j)
            .filter_map(|k| Some((k, *pts.get(k)?)))
            .map(|(k, r)| (k, if len > SAME_POINT { (r - p).cross(pq).abs() / len } else { r.distance(p) }))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((k, d)) = far
            && d > tolerance
        {
            if let Some(keep) = keep.get_mut(k) {
                *keep = true;
            }
            stack.push((i, k));
            stack.push((k, j));
        }
    }
    pts.iter().zip(keep).filter_map(|(p, k)| k.then_some(*p)).collect()
}

#[cfg(test)]
mod tests_corner {
    use vectorcraft_geom::{FillRule, Point};

    use super::super::geometry::SubPath;
    use super::*;

    /// A line that leaves the shape through a convex corner, its middle inside, doesn't stay inside:
    /// two squares 2.5 mm apart, A = [0, 10]², B = [12.5, 20] × [−10, 0], and the line from (5, 5)
    /// to (12.5, −2.5), through A's corner (10, 0), whose middle lies in A.
    #[test]
    fn a_line_out_through_a_corner_leaves() {
        let square = |x0: f64, y0: f64, x1: f64, y1: f64| SubPath {
            pts: vec![Point::new(x0, y0), Point::new(x1, y0), Point::new(x1, y1), Point::new(x0, y1)],
            closed: true,
        };
        let subs = [square(0.0, 0.0, 10.0, 10.0), square(12.5, -10.0, 20.0, 0.0)];
        let outlines: Vec<&SubPath> = subs.iter().collect();
        let area = Area { outlines: &outlines, rule: FillRule::NonZero, grid: None };
        assert!(!area.contains_segment(Point::new(5.0, 5.0), Point::new(12.5, -2.5)));
        // Inside one square, through nothing: inside.
        assert!(area.contains_segment(Point::new(1.0, 1.0), Point::new(9.0, 9.0)));
        // Into a corner and back along its edge: on the outline, inside.
        assert!(area.contains_segment(Point::new(5.0, 5.0), Point::new(10.0, 0.0)));
    }
}
