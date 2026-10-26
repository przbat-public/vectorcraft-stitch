//! Tatami fill ([`Style::Fill`](super::Style::Fill)): rows of stitches across a shape, sewn region
//! by region over an edge run and, for a large shape, a sparse tatami underlay.

use vectorcraft_geom::{BezPath, FillRule, Point, Vec2};

use super::area::{Area, Grid, TRAVEL_STITCH, Travels};
use super::geometry::{Axes, SubPath, area, length, nearest_on, resample, subpaths, unit};
use super::narrow::Column;
use super::runs::{COST_SEEN, Runs, Travel};
use super::satin::{column_returns, sew_column};
use super::{Aim, MAX_STITCHES, Params, Start, Style, UNDERLAY_STITCH, Underlay};
use crate::{SAME_POINT, Stitch};

/// The smallest area (square millimetres) a subpath must enclose for a fill to treat it as a
/// shape rather than a line.
pub const MIN_AREA: f64 = 0.01;
/// A large fill's second underlay, sparse tatami rows across the top ones: the spacing of its rows
/// and its stitch length (millimetres), how far inside the edge it stays, and the smallest fill
/// (both ways, millimetres) that gets it. Estimates.
pub(super) const TATAMI_UNDERLAY_SPACING: f64 = 2.0;
pub(super) const TATAMI_UNDERLAY_STITCH: f64 = 3.0;
const TATAMI_UNDERLAY_INSET: f64 = 0.6;
/// The smallest fill, both ways (millimetres), that gets the sparse tatami underlay under its edge
/// run; a smaller one gets the edge run alone. An estimate.
pub const TATAMI_UNDERLAY_MIN: f64 = 10.0;
/// How much of a travel run in a fill's top stitching may show (millimetres over stitching already
/// sewn; any length of it may run under stitching still to come): a travel that would show more
/// lifts the needle instead. Travels under the top stitching (in the underlay) have no limit: the
/// top stitching hides them. An estimate.
const MAX_SEEN: f64 = 25.0;
/// The most rows one layer may have: a shape that needs more is refused ([`super::try_stitches`]).
pub(super) const MAX_ROWS: usize = 100_000;
/// The thinnest a shape may be across the rows to get any, as a share of the row spacing: half a
/// spacing. The rows are centred across the shape's extent (as many as the spacing fits, rounded),
/// so the margin left at each side is the same, a quarter to three quarters of a spacing: no row
/// lies on a tangent, and the thread's own width covers to the edge on both sides alike.
const FIRST_ROW: f64 = 0.5;
/// How far inside its outline a fill's edge-run underlay runs, as a share of the row spacing: half
/// a spacing, about where the outermost rows lie, so that the top stitching covers it (on the
/// outline itself it would lie outside the rows wherever their margin is wide).
const EDGE_RUN_INSET: f64 = 0.5;
/// The farthest a corner of the inset edge run moves from the outline's corner, as a multiple of
/// the inset: a sharp corner's bisector would reach far into the shape (a miter limit, as strokes
/// have).
const EDGE_RUN_MITER: f64 = 2.0;
/// The stagger: each row's penetrations sit [`STAGGER`] of a stitch along from the row before's,
/// through a cycle of this many rows, after which they are a whole stitch along, back where they
/// started; so neighbouring rows never line their penetrations up into a groove. Four rows make a
/// quarter stitch, Wilcom's default tatami offset fraction
/// (<https://wilcom.com/resources/blog/mastering-basic-stitch-types-satin-stitch-and-tatami-stitch>).
const STAGGER_ROWS: usize = 4;
/// The share of a stitch each row is shifted by: one cycle of [`STAGGER_ROWS`] is a whole stitch.
const STAGGER: f64 = 1.0 / STAGGER_ROWS as f64;
/// A row's last stitch is left out when it is no longer than this share of the stitch length:
/// the needle turns diagonally into the next row instead of crawling a spacing along the edge. By
/// judgement.
const SHORT_LAST: f64 = 0.5;
/// A turn into the next row that leaves the shape is taken round inside it when it is longer
/// than this (millimetres); a shorter one only shaves a corner and is sewn straight. An estimate.
const SHAVED_CORNER: f64 = 1.0;
/// A row shorter than this many spacings, or than [`MIN_ROW`] millimetres, has no room for a fill
/// and is left out. Estimates.
const MIN_ROW_SPACINGS: f64 = 2.0;
const MIN_ROW: f64 = 0.8;
/// The tatami underlay's rows run this many degrees from the top rows: across them, so the
/// underlay holds the fabric the other way. By judgement (square to the top rows).
pub(super) const UNDERLAY_TURN: f64 = 90.0;

/// A tatami fill of `path`: rows of stitches at [`Params::angle`] degrees, [`Params::density`]
/// apart, each split into stitches at most [`Params::stitch_length`] long and reaching past the
/// edge at its ends by its pull compensation ([`Params::compensation`] of its length), over an
/// edge run and, for a large shape, a tatami underlay when [`Params::underlay`] is on. A subpath
/// that encloses no area is a line: it is sewn as a running line. The needle is at `from` before it
/// starts: the fill starts as near it as it can (its edge run at the corner nearest, its first
/// region the one nearest), as Ink/Stitch starts fills.
pub(super) fn fill(path: &BezPath, rule: FillRule, p: &Params, aim: Aim) -> Vec<Stitch> {
    let p = p.sane();
    fill_runs(path, rule, &p, aim).join(&p)
}

/// [`fill`]'s runs before they are joined into stitches: what the moves between them are
/// ([`Runs::moves`]) is known then. `p` is sane.
pub(super) fn fill_runs(path: &BezPath, rule: FillRule, p: &Params, aim: Aim) -> Runs {
    let subs = closed_subpaths(path);
    let mut runs = Runs::from(aim.start.point());
    for sub in subs.iter().filter(|s| !s.closed) {
        let line = sub.points();
        if length(&line, false) >= p.min_stitch {
            runs.push(resample(&line, p.stitch_length, false), Travel::Auto);
        }
    }
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return runs;
    }
    let grid = Grid::new(&outlines);
    let shape = Area { outlines: &outlines, rule, grid: grid.as_ref() };
    let mut travels = Travels::new(shape);
    sew_area(shape, &mut travels, p, entered(aim.start), &[], &mut runs, &|_| 0.0);
    runs
}

/// The entry point a fill is sewn from, when `start` is one (and a number).
fn entered(start: Start) -> Option<Point> {
    match start {
        Start::Entry(e) if e.x.is_finite() && e.y.is_finite() => Some(e),
        _ => None,
    }
}

/// The closed outlines of `subs` (at least three points each): what a fill fills.
pub(super) fn outlines(subs: &[SubPath]) -> Vec<&SubPath> {
    subs.iter().filter(|s| s.closed && s.pts.len() >= 3).collect()
}

/// `path`'s subpaths as a fill takes them: closed as paint closes them (SVG, PDF), so a shape drawn
/// without a final close is still an area; a subpath that encloses no area stays a line.
pub(super) fn closed_subpaths(path: &BezPath) -> Vec<SubPath> {
    subpaths(path)
        .into_iter()
        .map(|mut s| {
            s.closed = s.closed || (s.pts.len() >= 3 && area(&s.pts).abs() > MIN_AREA);
            s
        })
        .collect()
}

/// The fill of `rows` (its underlay and its top rows) sewn after `runs`, the needle travelling
/// inside the area `travels` crosses (`rows` itself, or a larger shape that holds it). When
/// `toward` names places sewn next, the top rows end as near one of them as their order allows
/// ([`sew`]).
///
/// From an `entry` point a person set, the needle goes in exactly there, as near as it lies to the
/// fill's inset outline (half a row spacing inside, where the edge run runs): the edge run starts
/// there; with no underlay, the needle travels from there to the first row under the rows still to
/// come ([`Runs::lead_from`]).
///
/// `finished` measures how much of a way lies over other parts of the shape already sewn (a fill with
/// narrow columns sews its pieces one after another): what a travel shows there counts against
/// [`MAX_SEEN`], in the underlay's approach and the top rows' alike.
fn sew_area(
    rows: Area<'_>,
    travels: &mut Travels<'_>,
    p: &Params,
    entry: Option<Point>,
    toward: &[Point],
    runs: &mut Runs,
    finished: &dyn Fn(&[Point]) -> f64,
) {
    // A fill's underlay: an edge run, and the tatami layer (by its size, or whatever its size).
    let tatami = match p.underlay {
        Underlay::None => None,
        Underlay::Center | Underlay::Edge => Some(Tatami::Never),
        Underlay::Auto => Some(Tatami::Large),
        Underlay::EdgeZigzag | Underlay::EdgeTatami => Some(Tatami::Always),
    };
    if let Some(tatami) = tatami {
        edge_runs(rows, travels, p, entry, runs, finished);
        if tatami != Tatami::Never {
            tatami_underlay(rows, travels, p, tatami == Tatami::Always, runs, finished);
        }
    } else if let Some((_, _, _, at)) = entry.and_then(|e| entered_outline(rows, p.density * EDGE_RUN_INSET, e)) {
        // No underlay: the needle goes in at the entry and travels to the first row.
        runs.lead_from(at);
    }
    let axes = Axes::new(p.angle);
    if let Some((lo, hi)) = rows.extent(axes.across)
        && hi - lo >= p.density * FIRST_ROW
    {
        let layer = Layer { axes, spacing: p.density, inset: 0.0 };
        let regions = layer.regions(rows, lo, hi, p);
        sew(&regions, travels, MAX_SEEN, true, p, toward, runs, finished);
    }
}

/// The smallest piece of a shape's rest that is filled once its narrow columns are cut out (square
/// millimetres): a smaller one is a sliver where a column's end meets the outline, too small for a
/// row of its own (the shortest is 0.8 mm long), and the column's own pull compensation covers it.
const MIN_PIECE: f64 = 1.0;

/// A filled shape sewn in parts: `rest`, the shape without its narrow columns (cut out by the
/// caller with path operations), as a tatami fill, and each of `columns` as a satin column whose
/// width follows the shape ([`sew_column`]). The pieces of the rest (each outline with the holes
/// inside it) and the columns are sewn one after another, whichever starts nearest the needle
/// next, so that the needle moves on to what lies next to where it is. It travels inside the whole
/// `shape` (`rule` decides what it covers) from part to part, under the parts still to come where
/// it can; over a piece of the rest already sewn a travel may show for at most [`MAX_SEEN`], and
/// over a column already sewn it is not counted as showing. `fill` and `satin`
/// are the two kinds of parts' parameters. A piece of the rest smaller than [`MIN_PIECE`] is left
/// out.
pub(crate) fn fill_with_columns(
    shape: &BezPath,
    rule: FillRule,
    rest: &BezPath,
    columns: &[Column],
    fill: &Params,
    satin: &Params,
    aim: Aim,
) -> Vec<Stitch> {
    let fill = Params { style: Style::Fill, ..*fill }.sane();
    let satin = Params { style: Style::Satin, ..*satin }.sane();
    let mut runs = Runs::from(aim.start.point());
    // An entry point starts the first part, when it is a piece of the rest.
    let entry = entered(aim.start);
    let subs = closed_subpaths(shape);
    let outlines = outlines(&subs);
    if outlines.is_empty() {
        return runs.join(&fill);
    }
    let grid = Grid::new(&outlines);
    let whole = Area { outlines: &outlines, rule, grid: grid.as_ref() };
    let mut travels = Travels::new(whole);
    let rest_subs = closed_subpaths(rest);
    let pieces = pieces(&rest_subs);
    let grids: Vec<Option<Grid>> = pieces.iter().map(|p| Grid::new(p)).collect();
    let areas: Vec<Area<'_>> = pieces.iter().zip(&grids).map(|(p, g)| Area { outlines: p, rule: FillRule::NonZero, grid: g.as_ref() }).collect();
    let parts: Vec<Part> = (0..areas.len()).map(Part::Piece).chain((0..columns.len()).map(Part::Column)).collect();
    let contacts: Vec<Vec<Point>> = parts
        .iter()
        .map(|part| match *part {
            Part::Piece(i) => areas.get(i).map(|a| spread(a.outlines.iter().flat_map(|s| s.pts.iter().copied()).collect())).unwrap_or_default(),
            Part::Column(i) => columns.get(i).map(|c| if c.closed { spread(c.middles()) } else { ends(c) }).unwrap_or_default(),
        })
        .collect();
    // An open column that does not come back is left at its other end.
    let through =
        |k: usize| matches!(parts.get(k), Some(Part::Column(i)) if columns.get(*i).is_some_and(|c| !c.closed && !column_returns(c, &satin)));
    let order = Order { contacts: &contacts, through: &through }.best(runs.here_inside());
    let mut sewn: Vec<bool> = vec![false; areas.len()];
    for (step, &k) in order.iter().enumerate() {
        if runs.full() {
            break;
        }
        match parts.get(k) {
            Some(&Part::Piece(i)) => {
                // It ends as near the part sewn next as it can.
                let toward = order.get(step + 1).and_then(|&next| contacts.get(next)).cloned().unwrap_or_default();
                // Its travels show where they cross a piece already sewn, as a column's do.
                let finished = |route: &[Point]| areas.iter().zip(&sewn).filter(|(_, s)| **s).map(|(a, _)| inside(*a, route)).sum::<f64>();
                if let Some(&area) = areas.get(i) {
                    sew_area(area, &mut travels, &fill, entry.filter(|_| step == 0), &toward, &mut runs, &finished);
                }
                if let Some(s) = sewn.get_mut(i) {
                    *s = true;
                }
            }
            Some(&Part::Column(i)) => {
                // A travel shows where it crosses a piece of the rest already sewn.
                let seen = |route: &[Point]| areas.iter().zip(&sewn).filter(|(_, s)| **s).map(|(a, _)| inside(*a, route)).sum::<f64>();
                let mut reach = |runs: &mut Runs, to: Point, to_inside: Point| runs.approach(&mut travels, to, to_inside, MAX_SEEN, &seen, &fill);
                if let Some(column) = columns.get(i) {
                    sew_column(column, &satin, &mut runs, &mut reach);
                }
            }
            None => {}
        }
    }
    runs.join(&fill)
}

/// The pieces of a shape's rest (`subs`, from path operations): each outline wound as the largest
/// one with the outlines wound the other way inside it (its holes), the smallest that holds them.
/// A piece smaller than [`MIN_PIECE`] is left out: a sliver where a column's end meets the outline.
fn pieces(subs: &[SubPath]) -> Vec<Vec<&SubPath>> {
    let closed = outlines(subs);
    let outer = closed.iter().map(|s| area(&s.pts)).fold(0.0f64, |a, b| if b.abs() > a.abs() { b } else { a }).signum();
    let (outers, holes): (Vec<&SubPath>, Vec<&SubPath>) = closed.into_iter().partition(|s| area(&s.pts).signum() == outer);
    let mut out: Vec<Vec<&SubPath>> = outers.iter().filter(|s| area(&s.pts).abs() >= MIN_PIECE).map(|s| vec![*s]).collect();
    for hole in holes {
        let Some(&q) = hole.pts.first() else { continue };
        let owner = out
            .iter_mut()
            .filter(|p| p.first().is_some_and(|o| Area { outlines: std::slice::from_ref(o), rule: FillRule::NonZero, grid: None }.contains(q)))
            .min_by(|a, b| {
                let size = |p: &Vec<&SubPath>| p.first().map_or(f64::INFINITY, |o| area(&o.pts).abs());
                size(a).total_cmp(&size(b))
            });
        if let Some(p) = owner {
            p.push(hole);
        }
    }
    out
}

/// One part of a shape sewn with its columns: a piece of its rest, or a column.
#[derive(Clone, Copy)]
enum Part {
    Piece(usize),
    Column(usize),
}

/// The most points of a part's outline weighed when ordering the parts ([`Order`]): enough to
/// tell which parts touch, a bound on the work.
const CONTACTS: usize = 64;
/// The most parts whose every order is weighed; beyond, the nearest part is sewn next.
const MAX_ORDERED: usize = 6;

/// At most [`CONTACTS`] of `pts`, spread evenly along them.
fn spread(pts: Vec<Point>) -> Vec<Point> {
    let step = pts.len().div_ceil(CONTACTS).max(1);
    pts.into_iter().step_by(step).collect()
}

/// An open column's two ends (the middles of its first and last stations).
fn ends(column: &Column) -> Vec<Point> {
    let m = column.middles();
    [m.first().copied(), m.last().copied()].into_iter().flatten().collect()
}

/// The order a shape's parts are sewn in: each part by the points where the needle can come in or
/// leave (`contacts`: a piece's outline, a ring's middle, an open column's two ends), the order
/// whose moves from part to part are shortest in all. A column sewn `through` is left at the end
/// it did not come in by, one that comes back ([`column_returns`]) at the end it came in by, and a
/// piece anywhere (its fill ends as near the next part as it can). Every order is weighed for up
/// to [`MAX_ORDERED`] parts; beyond, the nearest next.
struct Order<'a> {
    contacts: &'a [Vec<Point>],
    through: &'a dyn Fn(usize) -> bool,
}

/// Where the needle is between parts: at a point, anywhere on a part, or nowhere yet.
#[derive(Clone, Copy)]
enum At {
    Point(Point),
    Part(usize),
    Nowhere,
}

impl Order<'_> {
    /// The parts' order, from `here`.
    fn best(&self, here: Option<Point>) -> Vec<usize> {
        let n = self.contacts.len();
        let start = here.map_or(At::Nowhere, At::Point);
        if n > MAX_ORDERED {
            // The nearest part next.
            let mut left: Vec<usize> = (0..n).collect();
            let mut at = start;
            let mut out = Vec::with_capacity(n);
            // Each part's step worked out once a round; of equally near ones, the first.
            while let Some((k, (_, next))) = left.iter().map(|&part| self.step(at, Some(part))).enumerate().min_by(|a, b| a.1.0.total_cmp(&b.1.0)) {
                let part = left.swap_remove(k);
                at = next;
                out.push(part);
            }
            return out;
        }
        let mut best: (f64, Vec<usize>) = (f64::INFINITY, (0..n).collect());
        let mut path: Vec<usize> = Vec::with_capacity(n);
        self.search(start, 0.0, &mut path, &mut best);
        best.1
    }

    /// Every order that goes on from `path` (reached at `at`, `cost` so far), the cheapest kept in
    /// `best`.
    fn search(&self, at: At, cost: f64, path: &mut Vec<usize>, best: &mut (f64, Vec<usize>)) {
        if cost >= best.0 {
            return;
        }
        if path.len() == self.contacts.len() {
            *best = (cost, path.clone());
            return;
        }
        for k in 0..self.contacts.len() {
            if path.contains(&k) {
                continue;
            }
            let (d, next) = self.step(at, Some(k));
            path.push(k);
            self.search(next, cost + d, path, best);
            path.pop();
        }
    }

    /// How far the needle moves from `at` into part `k`, and where it is after it.
    fn step(&self, at: At, k: Option<usize>) -> (f64, At) {
        let Some(k) = k else { return (f64::INFINITY, at) };
        let Some(points) = self.contacts.get(k) else { return (f64::INFINITY, at) };
        let one: [Point; 1];
        let from: &[Point] = match at {
            At::Point(p) => {
                one = [p];
                &one
            }
            At::Part(j) => self.contacts.get(j).map_or(&[], Vec::as_slice),
            At::Nowhere => &[],
        };
        // The nearest pair of a point the needle may be at and a point of the part.
        let nearest = points
            .iter()
            .enumerate()
            .map(|(e, q)| (e, from.iter().map(|p| p.distance(*q)).fold(if from.is_empty() { 0.0 } else { f64::INFINITY }, f64::min)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((entry, d)) = nearest else { return (f64::INFINITY, at) };
        if (self.through)(k) {
            // In at one end, out at the other.
            let exit = points.get(1 - entry.min(1)).copied();
            (d, exit.map_or(At::Part(k), At::Point))
        } else if points.len() == 2 {
            // An open column that comes back: out where it came in.
            (d, points.get(entry).copied().map_or(At::Part(k), At::Point))
        } else {
            (d, At::Part(k))
        }
    }
}

/// How finely a route is measured (millimetres), and the most steps one stretch of it takes (a
/// bound on the work).
const ROUTE_STEP: f64 = 0.5;
const ROUTE_MAX_STEPS: f64 = 10_000.0;

/// `f` called with each stretch of `route` (each pair of points one after another): its length,
/// the steps it is measured in (every [`ROUTE_STEP`], at most [`ROUTE_MAX_STEPS`]), and the middle
/// of each step. A stretch whose length isn't finite is passed by.
fn each_stretch(route: &[Point], mut f: impl FnMut(f64, usize, &mut dyn Iterator<Item = Point>)) {
    for w in route.windows(2) {
        let [a, b] = w else { continue };
        let len = a.distance(*b);
        if !len.is_finite() {
            continue;
        }
        let steps = (len / ROUTE_STEP).ceil().clamp(1.0, ROUTE_MAX_STEPS) as usize;
        f(len, steps, &mut (0..steps).map(|k| a.lerp(*b, (k as f64 + 0.5) / steps as f64)));
    }
}

/// How much of `route` lies inside `shape` (millimetres), measured every half millimetre.
fn inside(shape: Area<'_>, route: &[Point]) -> f64 {
    let mut total = 0.0;
    each_stretch(route, |len, steps, middles| total += middles.filter(|q| shape.contains(*q)).count() as f64 * len / steps as f64);
    total
}

/// An edge run along each outline of `rows`, to hold the fabric under the fill: each starts at the
/// corner nearest the needle, and is reached by travelling inside the shape (hidden under the fill
/// to come, so of any length, except over other parts already sewn, `finished`). From an `entry`
/// point, the outline whose run passes nearest it goes first, starting at the point of its run
/// nearest the entry, and the others follow in their order.
fn edge_runs(rows: Area<'_>, travels: &mut Travels<'_>, p: &Params, entry: Option<Point>, runs: &mut Runs, finished: &dyn Fn(&[Point]) -> f64) {
    let inset_by = p.density * EDGE_RUN_INSET;
    let entered = entry.and_then(|e| entered_outline(rows, inset_by, e));
    let order: Vec<usize> = match &entered {
        Some((k, ..)) => std::iter::once(*k).chain((0..rows.outlines.len()).filter(|i| i != k)).collect(),
        None => (0..rows.outlines.len()).collect(),
    };
    for (n, &k) in order.iter().enumerate() {
        let Some(sub) = rows.outlines.get(k) else { continue };
        let mut pts: Vec<Point> = match &entered {
            // The entered outline's run, from the point of it nearest the entry round to it.
            Some((_, inset, i, at)) if n == 0 => around(inset, *i, *at),
            _ => {
                // The corner nearest the needle (an outline is closed).
                let start = runs.here_inside().map(|h| sub.nearest(h).0);
                let inset = inset_outline(sub, rows, inset_by);
                inset.iter().cycle().skip(start.unwrap_or(0)).take(inset.len()).copied().collect()
            }
        };
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
        let under = resample(&pts, UNDERLAY_STITCH, true);
        let Some(&first) = under.first() else { continue };
        let travel = runs.approach(travels, first, first, MAX_SEEN, finished, p);
        runs.push(under, travel);
    }
}

/// The outline of `shape` whose inset (`d` millimetres inside it, [`inset_outline`]) passes nearest
/// `entry`, and where: its index, the inset outline's points, the segment of them the nearest
/// point lies on (from point `i` to the next) and that point.
fn entered_outline(shape: Area<'_>, d: f64, entry: Point) -> Option<(usize, Vec<Point>, usize, Point)> {
    shape
        .outlines
        .iter()
        .enumerate()
        .filter_map(|(k, sub)| {
            let inset = inset_outline(sub, shape, d);
            let (i, at, dist) = nearest_on(&inset, true, entry)?;
            Some((dist, (k, inset, i, at)))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, found)| found)
}

/// Closed polyline `pts` from `at`, a point of its segment `i` (from point `i` to the next), round
/// to its segment's start: `at` first, then the points after it (a point of the polyline `at` lies
/// on is not put in twice).
fn around(pts: &[Point], i: usize, at: Point) -> Vec<Point> {
    let n = pts.len();
    let mut out: Vec<Point> = Vec::with_capacity(n + 1);
    out.push(at);
    for k in 1..=n {
        if let Some(&q) = pts.get((i + k) % n)
            && q.distance(at) > SAME_POINT
        {
            out.push(q);
        }
    }
    out
}

/// The corners of closed outline `sub` of `shape`, each moved `d` millimetres into the shape along
/// its corner's bisector (by up to [`EDGE_RUN_MITER`] times `d` at a sharp corner), as far as the
/// shape allows: a corner that would leave the shape, or whose way from the corner before would,
/// stays where it is.
fn inset_outline(sub: &SubPath, shape: Area<'_>, d: f64) -> Vec<Point> {
    let pts = &sub.pts;
    let n = pts.len();
    if n < 3 || !(d > 0.0 && d.is_finite()) {
        return pts.clone();
    }
    let mut out: Vec<Point> = Vec::with_capacity(n);
    for (i, &v) in pts.iter().enumerate() {
        let (Some(&prev), Some(&next)) = (pts.get((i + n - 1) % n), pts.get((i + 1) % n)) else {
            out.push(v);
            continue;
        };
        let moved = (|| {
            let (a, b) = (unit(v - prev)?, unit(next - v)?);
            // The left normals of the two edges, and their bisector.
            let (na, nb) = (Vec2::new(-a.y, a.x), Vec2::new(-b.y, b.x));
            let m = unit(na + nb)?;
            let reach = (d / m.dot(na).max(1.0 / EDGE_RUN_MITER)).min(d * EDGE_RUN_MITER);
            [v + m * reach, v - m * reach].into_iter().find(|&q| shape.contains(q))
        })();
        out.push(moved.unwrap_or(v));
    }
    // A corner whose way from the corner before leaves the shape stays on the outline.
    for i in 0..n {
        let prev = out.get((i + n - 1) % n).copied();
        if let (Some(a), Some(b)) = (prev, out.get(i).copied())
            && !shape.contains_segment(a, b)
            && let (Some(slot), Some(&orig)) = (out.get_mut(i), pts.get(i))
        {
            *slot = orig;
        }
    }
    out
}

/// When a fill gets its tatami underlay: never, when it is large, or always.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tatami {
    Never,
    Large,
    Always,
}

/// A large fill's second underlay (any fill's when `always`): sparse tatami rows across the top
/// rows' direction, kept a little inside the edge of `shape` so they never peek out.
fn tatami_underlay(shape: Area<'_>, travels: &mut Travels<'_>, p: &Params, always: bool, runs: &mut Runs, finished: &dyn Fn(&[Point]) -> f64) {
    // Under the top stitching, the underlay needs no pull compensation: it stays inside the edge.
    let under = Params {
        angle: p.angle + UNDERLAY_TURN,
        density: TATAMI_UNDERLAY_SPACING,
        stitch_length: TATAMI_UNDERLAY_STITCH,
        pull: [0.0; 2],
        pull_percent: [0.0; 2],
        ..*p
    }
    .sane();
    let axes = Axes::new(under.angle);
    let (Some(w), Some(h)) = (shape.extent(axes.along), shape.extent(axes.across)) else { return };
    if !always && (w.1 - w.0 < TATAMI_UNDERLAY_MIN || h.1 - h.0 < TATAMI_UNDERLAY_MIN) {
        return;
    }
    let layer = Layer { axes, spacing: under.density, inset: TATAMI_UNDERLAY_INSET };
    let regions = layer.regions(shape, h.0, h.1, &under);
    // Under the top stitching, a travel of any length is hidden: the nearest region next will do.
    // (What it crosses of other parts already sewn, `finished`, weighs against a way, unlimited.)
    sew(&regions, travels, f64::INFINITY, false, &under, &[], runs, finished);
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
    ///
    /// Each row reaches past the edge at its ends by `p`'s pull compensation
    /// ([`Params::compensation`] of its length: the fabric draws a row in along its length), at
    /// most half way to the next stretch of the same row, so that rows either side of a narrow gap
    /// or hole never meet across it.
    fn regions(&self, shape: Area<'_>, lo: f64, hi: f64, p: &Params) -> Regions {
        // As many rows as the spacing fits, rounded, centred: the same margin at both sides.
        let rows = (((hi - lo) / self.spacing).round().max(1.0) as usize).min(MAX_ROWS);
        let first = lo + ((hi - lo) - self.spacing * (rows as f64 - 1.0)) / 2.0;
        let overlap = |x: (f64, f64), y: (f64, f64)| x.0 < y.1 && y.0 < x.1;
        let mut out: Vec<Region> = Vec::new();
        let mut links: Vec<(usize, usize)> = Vec::new();
        // The regions the previous row's spans went to, with those spans.
        let mut open: Vec<(usize, (f64, f64))> = Vec::new();
        // Every row's position, spans and how far each reaches past its ends, for the overlaps
        // below.
        let mut all: Vec<Scan> = Vec::with_capacity(rows);
        let mut sweep = shape.sweep(self.axes);
        for row in 0..rows {
            let t = first + self.spacing * row as f64;
            let spans: Vec<(f64, f64)> = sweep.spans(t).into_iter().map(|(a, b)| (a + self.inset, b - self.inset)).filter(|(a, b)| b > a).collect();
            let reaches: Vec<(f64, f64)> = (0..spans.len()).map(|j| reach(&spans, j, p)).collect();
            all.push(Scan { t, spans: spans.clone(), reaches: reaches.clone() });
            let mut next: Vec<(usize, (f64, f64))> = Vec::with_capacity(spans.len());
            for (j, &span) in spans.iter().enumerate() {
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
                    let reach = reaches.get(j).copied().unwrap_or((0.0, 0.0));
                    region.rows.push(Row { index: row, t, a: span.0, b: span.1, reach });
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
                let Some(Scan { t, spans, reaches }) = all.get(next) else { continue };
                // The neighbouring span that shares the most with this end. Where the stretch ends
                // at that span's own end (the edge) it reaches past it as the span does; where it
                // is cut inside the span, not.
                let shared = spans
                    .iter()
                    .zip(reaches)
                    .map(|(s, r)| (s.0.max(a), s.1.min(b), (if s.0 >= a { r.0 } else { 0.0 }, if s.1 <= b { r.1 } else { 0.0 })))
                    .filter(|(x, y, _)| y - x > SAME_POINT)
                    .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)));
                if let Some((x, y, reach)) = shared {
                    let row = Row { index: next, t: *t, a: x, b: y, reach };
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

/// One row across a layer's shape: where it lies across the rows, its stretches inside the shape
/// (in order along it), and how far each reaches past its start and its end ([`reach`]).
struct Scan {
    t: f64,
    spans: Vec<(f64, f64)>,
    reaches: Vec<(f64, f64)>,
}

/// One row's stretch inside the shape: the row's number (for the stagger), where it sits across
/// the rows, where the stretch starts and ends along them, and how far its stitches reach past its
/// start and its end (pull compensation).
#[derive(Clone, Copy, Debug)]
struct Row {
    index: usize,
    t: f64,
    a: f64,
    b: f64,
    reach: (f64, f64),
}

/// How far span `j` of a row's `spans` (in order along it) reaches past its start and its end:
/// `p`'s pull compensation for its length, the first side at its start and the second at its end,
/// each at most half the gap to the span before or after.
fn reach(spans: &[(f64, f64)], j: usize, p: &Params) -> (f64, f64) {
    let Some(&(a, b)) = spans.get(j) else { return (0.0, 0.0) };
    let [start, end] = p.compensation(b - a);
    let before = j.checked_sub(1).and_then(|i| spans.get(i)).map_or(f64::INFINITY, |s| (a - s.1) / 2.0);
    let after = spans.get(j + 1).map_or(f64::INFINITY, |s| (s.0 - b) / 2.0);
    (start.min(before).max(0.0), end.min(after).max(0.0))
}

/// One row as it is sewn: its needle points, and where it starts and ends inside the shape (on the
/// edge, before pull compensation stretched it; `None` at the end when the row's last stitch was
/// left out and it ends where it is).
struct RowRun {
    pts: Vec<Point>,
    first_inside: Point,
    last_inside: Option<Point>,
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
    fn runs(&self, descending: bool, forward: bool, axes: Axes, p: &Params) -> Vec<RowRun> {
        let mut out: Vec<RowRun> = Vec::with_capacity(self.rows.len());
        // The stitch left out of the last row sewn so far, with where it reaches without pull
        // compensation.
        let mut left_out: Option<(Point, Point)> = None;
        let mut forward = forward;
        let order: Vec<&Row> = if descending { self.rows.iter().rev().collect() } else { self.rows.iter().collect() };
        for row in order {
            // The stagger walks a quarter stitch at a time through a cycle of four rows: no two
            // rows line their penetrations up into a groove.
            let phase = STAGGER * (row.index % STAGGER_ROWS) as f64;
            let along = row_points(row.a, row.b, phase, p);
            let n = along.len();
            if n < 2 {
                continue;
            }
            // The stitch at the row's far end as it is sewn, before pull compensation.
            let (Some(&tail), Some(&before_tail)) = (if forward { (along.get(n - 1), along.get(n - 2)) } else { (along.first(), along.get(1)) })
            else {
                continue;
            };
            let end_stitch = (tail - before_tail).abs();
            // Pull compensation moves the row's two end points out past the edge. Nothing else
            // changes (the grid, which stitch is left out below, where the needle goes next), so
            // the compensation changes no sewing order.
            let mut stretched = along;
            if let Some(s) = stretched.first_mut() {
                *s -= row.reach.0;
            }
            if let Some(s) = stretched.last_mut() {
                *s += row.reach.1;
            }
            let mut pts: Vec<Point> = stretched.iter().map(|s| axes.point(*s, row.t)).collect();
            let (mut first_inside, mut last_inside) = (axes.point(row.a, row.t), axes.point(row.b, row.t));
            if !forward {
                pts.reverse();
                (first_inside, last_inside) = (last_inside, first_inside);
            }
            let mut kept_inside = Some(last_inside);
            left_out = None;
            // Skip the last stitch of the row: the needle turns diagonally into the next row
            // instead of crawling one row spacing along the edge — when the stitch it leaves out
            // is short, so the turn stays a short stitch too.
            if n > 2
                && end_stitch <= SHORT_LAST * p.stitch_length
                && let Some(q) = pts.pop()
            {
                kept_inside = None;
                left_out = Some((q, last_inside));
            }
            out.push(RowRun { pts, first_inside, last_inside: kept_inside });
            forward = !forward;
        }
        // The region's last row keeps its last stitch: there is no next row to turn into, and
        // without it the row would stop up to half a stitch short of the edge (in a region of one
        // row, a thin fill, nothing else covers that end). The needle's way on is planned from the
        // row's end on the edge, as every row's is: from the stitch before, a straight way that
        // stays inside could leave the shape once the needle starts it from the row's end.
        if let (Some(last), Some((q, edge))) = (out.last_mut(), left_out) {
            last.last_inside = Some(edge);
            last.pts.push(q);
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
        let mut shows = 0.0;
        each_stretch(route, |len, steps, middles| {
            for q in middles {
                if self.sewn_at(q, sewn) {
                    shows += len / steps as f64;
                }
            }
        });
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
#[allow(clippy::too_many_arguments)]
fn sew(
    regions: &Regions,
    travels: &mut Travels<'_>,
    max_seen: f64,
    search: bool,
    p: &Params,
    toward: &[Point],
    runs: &mut Runs,
    finished: &dyn Fn(&[Point]) -> f64,
) {
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
        // The piece with a region that starts nearest the needle. (Where the needle is, here and
        // below, is taken inside the shape, where pull compensation stretched the last row from:
        // the compensation changes no sewing order.)
        let here = runs.here_inside();
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
        let mut best: Option<(Runs, Vec<bool>, f64)> = None;
        for root in orders {
            let mut local = Runs::continuing(runs);
            let mut done = sewn.clone();
            let mut ctx = Sewing { regions, travels: &mut *travels, max_seen, p, finished };
            match root {
                None => {
                    let mut left: Vec<usize> = piece.clone();
                    while !left.is_empty() && !local.full() {
                        let here = local.here_inside();
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
            // The last piece of the layer ends where the needle goes on from: the move from there
            // to the nearest place sewn next counts as a travel that shows.
            let exit = match (pieces.is_empty(), local.here()) {
                (true, Some(h)) if !toward.is_empty() => COST_SEEN * toward.iter().map(|t| t.distance(h)).fold(f64::INFINITY, f64::min),
                _ => 0.0,
            };
            let total = local.cost + exit;
            if best.as_ref().is_none_or(|(_, _, b)| total < *b) {
                best = Some((local, done, total));
            }
        }
        if let Some((local, done, _)) = best {
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
    // Each region's links, counted once (not in every comparison of the sort).
    let mut counts = vec![0usize; regions.list.len()];
    for &(a, b) in &regions.links {
        if let Some(c) = counts.get_mut(a) {
            *c += 1;
        }
        if b != a
            && let Some(c) = counts.get_mut(b)
        {
            *c += 1;
        }
    }
    let links = |r: usize| counts.get(r).copied().unwrap_or(0);
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
    /// How much of a way lies over other parts of the shape already sewn ([`sew_area`]).
    finished: &'r dyn Fn(&[Point]) -> f64,
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
            let here = local.here_inside();
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
        let (descending, forward, _) = nearest_start(region, descending, runs.here_inside(), axes);
        let rows = region.runs(descending, forward, axes, p);
        if let Some((first, first_inside)) = rows.first().and_then(|run| Some((*run.pts.first()?, run.first_inside))) {
            let finished = self.finished;
            let seen = |route: &[Point]| regions.cover.seen(route, sewn) + finished(route);
            let into = runs.approach(self.travels, first, first_inside, max_seen, &seen, p);
            for (k, run) in rows.into_iter().enumerate() {
                let travel = match (k, runs.here(), runs.here_inside(), run.pts.first()) {
                    (0, ..) => into,
                    // A turn into the region's next row is sewn while it stays inside the shape
                    // (judged between the rows' ends on the edge, before pull compensation
                    // stretched them). Along a concave edge (the flat top of a round hole) it
                    // would cut across the hole: then it travels round, or lifts, unless it only
                    // shaves a corner.
                    (_, Some(_), Some(inside), Some(&f))
                        if inside.distance(run.first_inside) > SHAVED_CORNER && !self.travels.area.contains_segment(inside, run.first_inside) =>
                    {
                        runs.approach(self.travels, f, run.first_inside, max_seen, &seen, p)
                    }
                    // A long turn (where the edge runs nearly along the rows) would be one long
                    // stitch lying loose on top: it travels in short stitches instead.
                    (_, Some(h), _, Some(&f)) if h.distance(f) > p.stitch_length => {
                        runs.push(resample(&[h, f], TRAVEL_STITCH, false), Travel::Sew);
                        Travel::Sew
                    }
                    _ => Travel::Sew,
                };
                runs.push_stretched(run.pts, travel, run.last_inside);
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
/// at most that much longer, and pull compensation's reach more).
pub(super) fn row_points(a: f64, b: f64, phase: f64, p: &Params) -> Vec<f64> {
    let len = b - a;
    let shortest = (MIN_ROW_SPACINGS * p.density).max(MIN_ROW);
    if !len.is_finite() || len < shortest || len < p.min_stitch {
        return Vec::new();
    }
    let step = p.stitch_length;
    let mut out: Vec<f64> = vec![a];
    // The first grid point past `a`.
    let mut g = ((a / step - phase).floor() + 1.0 + phase) * step;
    // The grid points a row this long can hold, and two more for rounding: a bound on the loop, so
    // that it ends where a step no longer moves `g` (coordinates past 2^52 steps).
    let most = ((len / step).ceil() + 2.0).min(MAX_STITCHES as f64) as usize;
    for _ in 0..most {
        if g >= b - p.min_stitch {
            break;
        }
        if g - a >= p.min_stitch {
            out.push(g);
        }
        g += step;
    }
    out.push(b);
    out
}

#[cfg(test)]
mod tests_finished {
    use vectorcraft_geom::{BezPath, FillRule, Point};

    use super::super::{Params, Style};
    use super::*;
    use crate::Command;

    /// A closed rectangle from `(x0, y0)` to `(x1, y1)`.
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x0, y0));
        p.line_to((x1, y0));
        p.line_to((x1, y1));
        p.line_to((x0, y1));
        p.close_path();
        p
    }

    /// A piece of a fill reached across other parts already sewn: its travel shows there, and past
    /// [`MAX_SEEN`] the needle lifts instead; across nothing sewn, it travels hidden.
    #[test]
    fn a_travel_over_parts_already_sewn_shows() {
        let p = Params::for_style(Style::Fill);
        // A bar 60 mm long; the piece to sew is its right end, the needle 48 mm away at its left.
        let (whole_subs, piece_subs) = (closed_subpaths(&rect(0.0, 0.0, 60.0, 10.0)), closed_subpaths(&rect(50.0, 0.0, 60.0, 10.0)));
        let (whole_outlines, piece_outlines) = (outlines(&whole_subs), outlines(&piece_subs));
        let whole = Area { outlines: &whole_outlines, rule: FillRule::NonZero, grid: None };
        let piece = Area { outlines: &piece_outlines, rule: FillRule::NonZero, grid: None };
        let lifts = |finished: &dyn Fn(&[Point]) -> f64| {
            let mut runs = Runs::from(Some(Point::new(2.0, 5.0)));
            runs.push(vec![Point::new(1.0, 5.0), Point::new(2.0, 5.0)], Travel::Auto);
            let mut travels = Travels::new(whole);
            sew_area(piece, &mut travels, &p, None, &[], &mut runs, finished);
            runs.join(&p).iter().filter(|s| s.command == Command::Jump).count()
        };
        assert_eq!(lifts(&|_| 0.0), 0, "across nothing sewn, a hidden travel");
        assert!(lifts(&|route| length(route, false)) > 0, "across 48 mm already sewn, a lift");
    }
}
