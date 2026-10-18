//! A filled area: what is inside it, where a row crosses it, and how the needle travels inside it
//! from one place to another.

use vectorcraft_geom::{FillRule, Point, Vec2};

use super::geometry::{Axes, SubPath, resample, unit};

/// A travel run's stitch length (millimetres): short, so it follows curves and hides under the
/// fill.
pub(super) const TRAVEL_STITCH: f64 = 2.0;
/// How far inside the shape a travel passes its corners (millimetres), so it doesn't show at the
/// edge.
const TRAVEL_MARGIN: f64 = 0.5;
/// How closely a travel's graph follows the outlines (millimetres), and the most corners it may
/// have: enough for any real outline, a bound on the work a travel costs.
const TRAVEL_SIMPLIFY: f64 = 0.3;
const MAX_TRAVEL_NODES: usize = 400;

/// A filled shape: its closed outlines (holes included) and the rule that decides what they
/// enclose.
#[derive(Clone, Copy)]
pub(super) struct Area<'a> {
    pub(super) outlines: &'a [&'a SubPath],
    pub(super) rule: FillRule,
}

impl Area<'_> {
    /// The edges of every outline, each from a point to the next (the last back to the first).
    fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.outlines.iter().flat_map(|sub| {
            let n = sub.pts.len();
            (0..n).filter_map(move |i| Some((*sub.pts.get(i)?, *sub.pts.get((i + 1) % n)?)))
        })
    }

    /// Whether `q` is inside.
    pub(super) fn contains(&self, q: Point) -> bool {
        let mut winding = 0i32;
        for (a, b) in self.edges() {
            let side = (b - a).cross(q - a);
            if a.y <= q.y {
                if b.y > q.y && side > 0.0 {
                    winding += 1;
                }
            } else if b.y <= q.y && side < 0.0 {
                winding -= 1;
            }
        }
        match self.rule {
            FillRule::NonZero => winding != 0,
            FillRule::EvenOdd => winding % 2 != 0,
        }
    }

    /// Whether the straight line from `a` to `b` stays inside: it crosses no edge (touching one
    /// at its ends is fine: row ends lie on the edge) and its middle is inside.
    pub(super) fn contains_segment(&self, a: Point, b: Point) -> bool {
        if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
            return false;
        }
        if a.distance(b) < 1e-9 {
            return self.contains(a);
        }
        let ab = b - a;
        let crosses = self.edges().any(|(c, d)| {
            let cd = d - c;
            let denom = ab.cross(cd);
            if denom.abs() < 1e-12 {
                return false;
            }
            // Where the two lines meet, as a share of each segment.
            let t = (c - a).cross(cd) / denom;
            let u = (c - a).cross(ab) / denom;
            t > 1e-6 && t < 1.0 - 1e-6 && u > 1e-9 && u < 1.0 - 1e-9
        });
        !crosses && self.contains(a.midpoint(b))
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

    /// A travel run from `a` to `b` inside the area, as stitches [`TRAVEL_STITCH`] long: the
    /// shortest way that stays inside (straight when that does), passing the outlines' corners
    /// [`TRAVEL_MARGIN`] inside them. `None` when there is no such way (two separate shapes) or it
    /// is longer than `max`: then the needle is lifted instead.
    ///
    /// The way is found on a visibility graph: the two ends and the outlines' corners (simplified
    /// to [`TRAVEL_SIMPLIFY`]), linked where the straight line between two of them stays inside,
    /// searched with Dijkstra's algorithm.
    pub(super) fn travel(&self, a: Point, b: Point, max: f64) -> Option<Vec<Point>> {
        let straight = a.distance(b);
        if !straight.is_finite() || straight > max {
            return None;
        }
        if self.contains_segment(a, b) {
            return Some(resample(&[a, b], TRAVEL_STITCH, false));
        }
        // The graph's nodes: the ends, then every corner moved inside.
        let mut nodes: Vec<Point> = vec![a, b];
        for sub in self.outlines {
            let corners = simplify(&sub.pts, TRAVEL_SIMPLIFY);
            nodes.extend((0..corners.len()).filter_map(|i| self.corner_inside(&corners, i)));
            if nodes.len() > MAX_TRAVEL_NODES {
                return None;
            }
        }
        let route = self.shortest_way(&nodes, max)?;
        Some(resample(&route, TRAVEL_STITCH, false))
    }

    /// The shortest way inside from `nodes[0]` to `nodes[1]` through the other nodes (Dijkstra's
    /// algorithm, each edge tested as it is reached), `None` when there is none no longer than
    /// `max`.
    fn shortest_way(&self, nodes: &[Point], max: f64) -> Option<Vec<Point>> {
        let n = nodes.len();
        let end = *nodes.get(1)?;
        let mut dist = vec![f64::INFINITY; n];
        let mut prev: Vec<Option<usize>> = vec![None; n];
        let mut done = vec![false; n];
        *dist.first_mut()? = 0.0;
        loop {
            // The nearest node not done yet.
            let u = (0..n)
                .filter(|&i| !done.get(i).copied().unwrap_or(true))
                .min_by(|&i, &j| dist.get(i).unwrap_or(&f64::INFINITY).total_cmp(dist.get(j).unwrap_or(&f64::INFINITY)))?;
            let (&du, &pu) = (dist.get(u)?, nodes.get(u)?);
            if !du.is_finite() || du > max {
                return None;
            }
            *done.get_mut(u)? = true;
            if u == 1 {
                break;
            }
            for v in 0..n {
                let (Some(&pv), Some(false)) = (nodes.get(v), done.get(v).copied()) else { continue };
                let dv = du + pu.distance(pv);
                // Nodes from which the end is out of reach can't be on a way short enough.
                if dv < dist.get(v).copied().unwrap_or(f64::INFINITY) && dv + pv.distance(end) <= max && self.contains_segment(pu, pv) {
                    *dist.get_mut(v)? = dv;
                    *prev.get_mut(v)? = Some(u);
                }
            }
        }
        let mut route: Vec<Point> = vec![end];
        let mut at = 1;
        while let Some(Some(u)) = prev.get(at) {
            route.push(*nodes.get(*u)?);
            at = *u;
            if route.len() > n {
                return None;
            }
        }
        route.reverse();
        Some(route)
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
            .map(|(k, r)| (k, if len > 1e-9 { (r - p).cross(pq).abs() / len } else { r.distance(p) }))
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
