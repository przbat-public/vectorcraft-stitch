//! Tatami fill ([`Style::Fill`](super::Style::Fill)): rows of stitches across a shape, sewn region
//! by region over an edge run and, for a large shape, a sparse tatami underlay.

use vectorcraft_geom::{BezPath, FillRule, Point};

use super::area::{Area, Grid, TRAVEL_STITCH, Travels};
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
/// How much of a travel run in a fill's top stitching may show (millimetres over stitching already
/// sewn; any length of it may run under stitching still to come): a travel that would show more
/// lifts the needle instead. Travels under the top stitching (in the underlay) have no limit: the
/// top stitching hides them. An estimate.
const MAX_SEEN: f64 = 25.0;
/// The most rows one layer may have (a guard against absurd parameters).
const MAX_ROWS: usize = 100_000;

/// A tatami fill of `path`: rows of stitches at [`Params::angle`] degrees, [`Params::density`]
/// apart, each split into stitches at most [`Params::stitch_length`] long, over an edge run and,
/// for a large shape, a tatami underlay when [`Params::underlay`] is on. A subpath that encloses
/// no area is a line: it is sewn as a running line.
pub fn fill(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    fill_from(path, rule, p, None)
}

/// [`fill`], the needle at `from` before it starts: the fill starts as near it as it can (its edge
/// run at the corner nearest, its first region the one nearest), as Ink/Stitch starts fills.
pub(super) fn fill_from(path: &BezPath, rule: FillRule, p: &Params, from: Option<Point>) -> Vec<Stitch> {
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
    let mut runs = Runs::from(from);
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
    let grid = Grid::new(&outlines);
    let shape = Area { outlines: &outlines, rule, grid: grid.as_ref() };
    let mut travels = Travels::new(shape);
    if p.underlay {
        edge_runs(&mut travels, &p, &mut runs);
        tatami_underlay(&mut travels, &p, &mut runs);
    }
    let axes = Axes::new(p.angle);
    if let Some((lo, hi)) = shape.extent(axes.across)
        && hi - lo >= p.density * 0.5
    {
        let layer = Layer { axes, spacing: p.density, inset: 0.0 };
        let regions = layer.regions(shape, lo, hi);
        sew(&regions, &mut travels, MAX_SEEN, true, &p, &mut runs);
    }
    runs.join(&p)
}

/// An edge run along each outline, to hold the fabric under the fill: each starts at the corner
/// nearest the needle, and is reached by travelling inside the shape (hidden under the fill, so of
/// any length).
fn edge_runs(travels: &mut Travels<'_>, p: &Params, runs: &mut Runs) {
    for sub in travels.area.outlines {
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
        let travel = runs.approach(travels, first, f64::INFINITY, &|_| 0.0, p);
        runs.push(under, travel);
    }
}

/// A large fill's second underlay: sparse tatami rows across the top rows' direction, kept a
/// little inside the edge so they never peek out.
fn tatami_underlay(travels: &mut Travels<'_>, p: &Params, runs: &mut Runs) {
    let shape = travels.area;
    let under = Params { angle: p.angle + 90.0, density: TATAMI_UNDERLAY_SPACING, stitch_length: TATAMI_UNDERLAY_STITCH, ..*p }.sane();
    let axes = Axes::new(under.angle);
    let (Some(w), Some(h)) = (shape.extent(axes.along), shape.extent(axes.across)) else { return };
    if w.1 - w.0 < TATAMI_UNDERLAY_MIN || h.1 - h.0 < TATAMI_UNDERLAY_MIN {
        return;
    }
    let layer = Layer { axes, spacing: under.density, inset: TATAMI_UNDERLAY_INSET };
    let regions = layer.regions(shape, h.0, h.1);
    // Under the top stitching, a travel of any length is hidden: the nearest region next will do.
    sew(&regions, travels, f64::INFINITY, false, &under, runs);
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
    /// merges (round a hole, into the arms of a letter) new regions start, linked to the regions
    /// they meet. Rows sit half a spacing inside the extent: no row lies on a tangent, and the
    /// thread's own width covers to the edge.
    fn regions(&self, shape: Area<'_>, lo: f64, hi: f64) -> Regions {
        let rows = (((hi - lo) / self.spacing).floor().max(1.0) as usize).min(MAX_ROWS);
        let overlap = |x: (f64, f64), y: (f64, f64)| x.0 < y.1 && y.0 < x.1;
        let mut out: Vec<Region> = Vec::new();
        let mut links: Vec<(usize, usize)> = Vec::new();
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
                let touching: Vec<usize> = open.iter().filter(|(_, prev)| overlap(*prev, span)).map(|(r, _)| *r).collect();
                let joined = match touching.as_slice() {
                    [r] if open.iter().find(|(o, _)| o == r).is_some_and(|(_, prev)| spans.iter().filter(|s| overlap(*prev, **s)).count() == 1) => {
                        Some(*r)
                    }
                    _ => None,
                };
                let r = joined.unwrap_or_else(|| {
                    out.push(Region::default());
                    let r = out.len() - 1;
                    // A new region starts where it meets the ones above it.
                    links.extend(touching.iter().map(|&above| (above, r)));
                    r
                });
                if let Some(region) = out.get_mut(r) {
                    region.rows.push(Row { index: row, t, a: span.0, b: span.1 });
                }
                next.push((r, span));
            }
            open = next;
        }
        // The rows each region sews, for telling which region a point lies under (before the
        // overlaps below, which belong to both).
        let mut cover: Vec<Vec<(usize, f64, f64)>> = vec![Vec::new(); all.len()];
        for (r, region) in out.iter().enumerate() {
            for row in &region.rows {
                if let Some(c) = cover.get_mut(row.index) {
                    c.push((r, row.a, row.b));
                }
            }
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
        Regions { list: out, links, cover: Cover { axes: self.axes, lo, spacing: self.spacing, rows: cover } }
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

/// A layer's regions, where they meet, and which region's rows lie where.
struct Regions {
    list: Vec<Region>,
    /// Region `a`'s last row meets region `b`'s first row.
    links: Vec<(usize, usize)>,
    cover: Cover,
}

/// Which regions' rows lie where: the stretches of each row, by row number.
struct Cover {
    axes: Axes,
    lo: f64,
    spacing: f64,
    rows: Vec<Vec<(usize, f64, f64)>>,
}

impl Cover {
    /// How far a way through `route` runs over stitching already sewn (the regions marked in
    /// `sewn`), millimetres: where every region whose rows lie there is sewn. A way under a region
    /// still to come is hidden by it.
    fn seen(&self, route: &[Point], sewn: &[bool]) -> f64 {
        /// How finely a way is measured (millimetres).
        const STEP: f64 = 0.5;
        let mut shows = 0.0;
        for w in route.windows(2) {
            let [a, b] = w else { continue };
            let len = a.distance(*b);
            if !len.is_finite() {
                continue;
            }
            let steps = (len / STEP).ceil().clamp(1.0, 10_000.0) as usize;
            for k in 0..steps {
                let q = a.lerp(*b, (k as f64 + 0.5) / steps as f64);
                if self.sewn_at(q, sewn) {
                    shows += len / steps as f64;
                }
            }
        }
        shows
    }

    /// Whether `q` lies on stitching already sewn: on some region's row, every such region sewn.
    fn sewn_at(&self, q: Point, sewn: &[bool]) -> bool {
        let k = ((self.axes.across(q) - self.lo) / self.spacing).floor();
        if !k.is_finite() || k < 0.0 {
            return false;
        }
        let s = self.axes.along(q);
        let mut under = self.rows.get(k as usize).into_iter().flatten().filter(|(_, a, b)| s >= *a && s <= *b).peekable();
        under.peek().is_some() && under.all(|(r, ..)| sewn.get(*r).copied().unwrap_or(false))
    }
}

/// The most regions tried as the one sewn last when ordering a fill's regions, and the most region
/// plannings all the tries of one piece may take together (a piece of many regions tries fewer):
/// a bound on the work.
const MAX_ROOTS: usize = 8;
const ROOT_BUDGET: usize = 320;
/// The most regions in one connected piece for which orders are compared (beyond, the nearest
/// region is sewn next): a bound on the work.
const MAX_SEARCHED: usize = 200;

/// The regions of one layer sewn after `runs`, each region whole, row after row. Moves into a
/// region travel inside the shape (showing at most `max_seen` millimetres over stitching already
/// sewn) or lift the needle.
///
/// The pieces of the shape (regions linked where they meet) are sewn one after another, the
/// nearest next. In each, when `search` is on, several orders are planned and the cheapest kept
/// (by [`Runs::cost`]: travels that show, jumps and trims): the nearest region next (the needle
/// goes wherever is closest), and orders that sew one region last and every other one before the
/// region it leads to. The latter is the approach of US patent 6,397,120 (Soft Sight, 2002),
/// which orders a fill's fragments by a recursive walk from the one where sewing ends, so that
/// travels between fragments run where the stitching still to come covers them: in a letter E
/// with vertical rows, the arms are sewn first, each towards the spine, and the spine last.
fn sew(regions: &Regions, travels: &mut Travels<'_>, max_seen: f64, search: bool, p: &Params, runs: &mut Runs) {
    let n = regions.list.len();
    let mut sewn = vec![false; n];
    let mut neighbours: Vec<Vec<(usize, bool)>> = vec![Vec::new(); n];
    for &(a, b) in &regions.links {
        // `b` lies at `a`'s last row; `a` at `b`'s first.
        if let Some(v) = neighbours.get_mut(a) {
            v.push((b, true));
        }
        if let Some(v) = neighbours.get_mut(b) {
            v.push((a, false));
        }
    }
    let mut pieces = connected(&neighbours);
    while !pieces.is_empty() && !runs.full() {
        // The piece with a region that starts nearest the needle.
        let here = runs.here();
        let Some(i) = pieces
            .iter()
            .enumerate()
            .map(|(i, piece)| (i, piece.iter().map(|&r| near_from(regions, r, here)).fold(f64::INFINITY, f64::min)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
        else {
            break;
        };
        let piece = pieces.swap_remove(i);
        let mut orders: Vec<Option<usize>> = vec![None];
        if search && piece.len() > 1 && piece.len() <= MAX_SEARCHED {
            orders.extend(roots(&piece, regions).into_iter().map(Some));
        }
        let mut best: Option<(Runs, Vec<bool>)> = None;
        for root in orders {
            let mut local = Runs::continuing(runs);
            let mut done = sewn.clone();
            let mut ctx = Sewing { regions, travels: &mut *travels, max_seen, p };
            match root {
                None => {
                    let mut left: Vec<usize> = piece.clone();
                    while !left.is_empty() && !local.full() {
                        let here = local.here();
                        let Some(k) = left
                            .iter()
                            .enumerate()
                            .map(|(k, &r)| (k, near_from(regions, r, here)))
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|(k, _)| k)
                        else {
                            break;
                        };
                        let r = left.swap_remove(k);
                        ctx.region(r, None, &mut local, &mut done);
                    }
                }
                Some(root) => ctx.tree(&neighbours, root, &mut local, &mut done),
            }
            if best.as_ref().is_none_or(|(b, _)| local.cost < b.cost) {
                best = Some((local, done));
            }
        }
        if let Some((local, done)) = best {
            runs.append(local);
            sewn = done;
        }
    }
}

/// Where the needle can start sewing `region` nearest `here`: whether descending (when not
/// given), whether forward, and how far that is.
fn nearest_start(region: &Region, descending: Option<bool>, here: Option<Point>, axes: Axes) -> (bool, bool, f64) {
    let mut best = (descending.unwrap_or(false), true, f64::INFINITY);
    for desc in descending.map_or(vec![false, true], |d| vec![d]) {
        for forward in [true, false] {
            let Some(start) = region.start(desc, forward, axes) else { continue };
            let d = here.map_or(0.0, |h| h.distance(start));
            if d < best.2 {
                best = (desc, forward, d);
            }
        }
    }
    best
}

/// How far from `here` region `r` can start.
fn near_from(regions: &Regions, r: usize, here: Option<Point>) -> f64 {
    regions.list.get(r).map_or(f64::INFINITY, |region| nearest_start(region, None, here, regions.cover.axes).2)
}

/// The connected pieces of the regions linked as `neighbours` says, each its regions in order.
fn connected(neighbours: &[Vec<(usize, bool)>]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; neighbours.len()];
    let mut out = Vec::new();
    for start in 0..neighbours.len() {
        if seen.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut piece = vec![start];
        if let Some(s) = seen.get_mut(start) {
            *s = true;
        }
        let mut k = 0;
        while let Some(&r) = piece.get(k) {
            for &(o, _) in neighbours.get(r).into_iter().flatten() {
                if let Some(s) = seen.get_mut(o)
                    && !*s
                {
                    *s = true;
                    piece.push(o);
                }
            }
            k += 1;
        }
        out.push(piece);
    }
    out
}

/// The regions of `piece` tried as the one sewn last: all of them when there are few, else those
/// with the most links (where a shape branches), the largest first among equals: at most
/// [`MAX_ROOTS`], fewer in a piece of many regions ([`ROOT_BUDGET`]).
fn roots(piece: &[usize], regions: &Regions) -> Vec<usize> {
    let links = |r: usize| regions.links.iter().filter(|(a, b)| *a == r || *b == r).count();
    let rows = |r: usize| regions.list.get(r).map_or(0, |g| g.rows.len());
    let mut v: Vec<usize> = piece.to_vec();
    v.sort_by(|&a, &b| links(b).cmp(&links(a)).then(rows(b).cmp(&rows(a))));
    v.truncate((ROOT_BUDGET / piece.len().max(1)).clamp(1, MAX_ROOTS));
    v
}

/// What sewing one layer's regions works with: the regions, the travels inside the shape, how far a
/// travel may show, and the parameters.
struct Sewing<'r, 't, 'a> {
    regions: &'r Regions,
    travels: &'t mut Travels<'a>,
    max_seen: f64,
    p: &'r Params,
}

impl Sewing<'_, '_, '_> {
    /// Sew the piece of regions round `root` so that `root` is sewn last and every other region
    /// before the region it leads to (towards `root`), ending next to it: a walk from `root` that
    /// sews a region once every region beyond it is sewn. Of a region's unsewn neighbours, the one
    /// that starts nearest the needle is taken first.
    fn tree(&mut self, neighbours: &[Vec<(usize, bool)>], root: usize, local: &mut Runs, done: &mut [bool]) {
        let regions = self.regions;
        let mut taken = vec![false; regions.list.len()];
        if let Some(t) = taken.get_mut(root) {
            *t = true;
        }
        // Each region on the walk, with the way it is sewn: towards the region before it
        // (descending when that one lies at its first row), or either way for the root.
        let mut stack: Vec<(usize, Option<bool>)> = vec![(root, None)];
        while let Some(&(r, descending)) = stack.last() {
            if local.full() {
                return;
            }
            let here = local.here();
            let next = neighbours
                .get(r)
                .into_iter()
                .flatten()
                .filter(|(o, _)| !taken.get(*o).copied().unwrap_or(true))
                .map(|&(o, at_last)| {
                    // `o` lies at `r`'s last row (`at_last`): `r` lies at `o`'s first, so `o` is
                    // sewn descending, ending at its first row, next to `r`.
                    let d = regions.list.get(o).map_or(f64::INFINITY, |g| nearest_start(g, Some(at_last), here, regions.cover.axes).2);
                    (o, at_last, d)
                })
                .min_by(|a, b| a.2.total_cmp(&b.2));
            match next {
                Some((o, desc, _)) => {
                    if let Some(t) = taken.get_mut(o) {
                        *t = true;
                    }
                    stack.push((o, Some(desc)));
                }
                None => {
                    stack.pop();
                    self.region(r, descending, local, done);
                }
            }
        }
    }

    /// Region `r` sewn after `runs`, from the start nearest the needle (descending as given, or
    /// whichever is nearer), and marked sewn in `sewn`.
    fn region(&mut self, r: usize, descending: Option<bool>, runs: &mut Runs, sewn: &mut [bool]) {
        let (regions, p, max_seen) = (self.regions, self.p, self.max_seen);
        let Some(region) = regions.list.get(r) else { return };
        let axes = regions.cover.axes;
        let (descending, forward, _) = nearest_start(region, descending, runs.here(), axes);
        let rows = region.runs(descending, forward, axes, p);
        if let Some(first) = rows.first().and_then(|run| run.first().copied()) {
            let seen = |route: &[Point]| regions.cover.seen(route, sewn);
            let into = runs.approach(self.travels, first, max_seen, &seen, p);
            for (k, run) in rows.into_iter().enumerate() {
                let travel = match (k, runs.here(), run.first()) {
                    (0, ..) => into,
                    // A turn into the region's next row is sewn while it stays inside the shape.
                    // Along a concave edge (the flat top of a round hole) it would cut across the
                    // hole: then it travels round, or lifts, unless it only shaves a corner.
                    (_, Some(h), Some(&f)) if h.distance(f) > 1.0 && !self.travels.area.contains_segment(h, f) => {
                        runs.approach(self.travels, f, max_seen, &seen, p)
                    }
                    // A long turn (where the edge runs nearly along the rows) would be one long
                    // stitch lying loose on top: it travels in short stitches instead.
                    (_, Some(h), Some(&f)) if h.distance(f) > p.stitch_length => {
                        runs.push(resample(&[h, f], TRAVEL_STITCH, false), Travel::Sew);
                        Travel::Sew
                    }
                    _ => Travel::Sew,
                };
                runs.push(run, travel);
            }
        }
        if let Some(s) = sewn.get_mut(r) {
            *s = true;
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
