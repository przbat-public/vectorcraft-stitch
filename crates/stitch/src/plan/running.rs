//! Running stitch ([`Style::Running`](super::Style::Running)): a single line of stitches along a
//! path, locked at both ends. A line whose pieces meet end to end is sewn as routes through them
//! ([`super::route`]), every piece once and a few back in their own holes, with no jump and no
//! connecting stitch inside a drawing.

use vectorcraft_geom::{BezPath, Point};

use super::geometry::{Line, MAX_REORDERED, SubPath, length, resample, subpaths};
use super::route::{Graph, Route, Step, route};
use super::runs::{COST_HIDDEN, COST_LIFT, COST_TRIM, Runs, Travel, lock_step};
use super::toward::{Finish, ordered_toward};
use super::{Aim, Params, Start};
use crate::{SAME_POINT, Stitch};

/// How a running line ends where it is asked to ([`ordered_toward`]): an open line at its far end,
/// never sewn back over itself; a closed one goes on past its start, again in its own holes, to its
/// point nearest the exit, the doubled stitches weighed as a travel hidden under stitching (they lie
/// in the line's own holes).
const RUNNING: Finish = Finish { returns: false, flip: None, reach: COST_HIDDEN };

/// [`running_until`] never asked to stop, for this crate's tests.
#[cfg(test)]
pub(super) fn running(path: &BezPath, p: &Params, aim: Aim) -> (Vec<Stitch>, f64) {
    running_until(path, p, aim, &|| false)
}

/// A running line of stitches along `path`, [`Params::stitch_length`] apart, each run locked at
/// both ends when [`Params::lock`] is on (a closed one too, where it starts and ends, so that a cut
/// after it can't pull it out), sewn from `aim`'s start: each line from its end nearest the needle,
/// the nearest first; from an entry point, the line it lies nearest first, a closed one starting
/// exactly there. Toward an exit point, its lines are ordered and turned to end nearest it, and a
/// closed one sewn last goes on round to it ([`ordered_toward`], [`lapped`]). Pieces that meet end
/// to end are sewn as routes ([`routed`]), `stopped` asked between their drawings and in their
/// routes' steps whether to stop: what it then returns is cut short. With the stitches, the
/// connecting stitches sewn between its lines (millimetres, [`Runs::connectors`]).
pub(super) fn running_until(path: &BezPath, p: &Params, aim: Aim, stopped: &dyn Fn() -> bool) -> (Vec<Stitch>, f64) {
    let p = p.sane();
    let subs = subpaths(path);
    if let Some(graph) = Graph::of(&subs) {
        return routed(&subs, &graph, &p, aim, stopped);
    }
    let mut runs = Runs::from(aim.start.point(), &p);
    for Line { pts, closed, toward, .. } in ordered_toward(subs, aim, &RUNNING, &p) {
        if length(&pts, closed) < p.min_stitch {
            continue;
        }
        let mut run = resample(&pts, p.stitch_length, closed);
        if let (true, Some(exit)) = (closed, toward) {
            run = lapped(run, exit);
        }
        if p.lock {
            run = locked(&run, p.min_stitch);
        }
        runs.push(run, Travel::Auto);
    }
    let connectors = runs.connectors(&p);
    (runs.join(), connectors)
}

/// A run with a lock stitch at each end: a short stitch back along the run and forward again, so
/// the thread stays put. Both stay on the run: forward from its start, back from its end (never
/// past it).
fn locked(pts: &[Point], min_stitch: f64) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return pts.to_vec() };
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out: Vec<Point> = Vec::with_capacity(pts.len() + 4);
    if let Some(step) = pts.get(1).and_then(|&next| lock_step(first, next, min_stitch)) {
        out.push(first);
        out.push(step);
    }
    out.extend_from_slice(pts);
    if let Some(step) = pts.len().checked_sub(2).and_then(|i| pts.get(i)).and_then(|&prev| lock_step(last, prev, min_stitch)) {
        out.push(step);
        out.push(last);
    }
    out
}

/// A closed line's stitches `lap` (from its start round to it again) carried on to its needle point
/// nearest `exit`, again in its own holes, the shorter way: the lap sewn the other way round when
/// that reaches the exit sooner (the same holes, in the other order). Unchanged when the start is
/// already the point nearest.
pub(super) fn lapped(lap: Vec<Point>, exit: Point) -> Vec<Point> {
    let n = lap.len().saturating_sub(1);
    if n < 2 {
        return lap;
    }
    let nearest = (0..n).map(|i| lap.get(i).map_or(f64::INFINITY, |q| q.distance(exit))).enumerate().min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((k, _)) = nearest else { return lap };
    if k == 0 {
        return lap;
    }
    let (mut lap, k) = if k <= n - k {
        (lap, k)
    } else {
        let mut back = lap;
        back.reverse();
        (back, n - k)
    };
    let on: Vec<Point> = lap.get(1..=k).map(<[Point]>::to_vec).unwrap_or_default();
    lap.extend(on);
    lap
}

/// What a line whose pieces meet sews in turn: a route through a connected part of its pieces (its
/// place in [`Graph::components`]), or one piece on its own (its place in the line's subpaths): a
/// closed one, or one that meets no other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Part(usize),
    Piece(usize),
}

/// The point of a node of `graph`.
fn node(graph: &Graph, n: usize) -> Point {
    graph.nodes.get(n).copied().unwrap_or(Point::new(f64::NAN, f64::NAN))
}

/// The node of `part` (its edges) nearest `q`, and how far it is.
fn nearest_node(graph: &Graph, part: &[usize], q: Point) -> Option<(usize, f64)> {
    part.iter()
        .filter_map(|&e| graph.edges.get(e))
        .flat_map(|e| [e.a, e.b])
        .map(|n| (n, node(graph, n).distance(q)))
        .filter(|(_, d)| d.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
}

/// How many millimetres route `r` through `graph` sews twice.
fn twice(graph: &Graph, r: &Route) -> f64 {
    r.steps.iter().filter(|s| s.again).map(|s| graph.edges.get(s.edge).map_or(0.0, |e| e.len)).sum()
}

/// The route through `graph`'s part `edges` from node `start` to node `end` (free when `None`), as
/// near the needle (`here`) as it can start, as a line's subpaths start nearest it: with its start
/// free, from its node nearest the needle when that sews nothing more twice (a closed one always,
/// one whose ends cost the same), else, both ends free, from its end nearer the needle. `None` when
/// there is no route through it.
fn route_near(
    graph: &Graph,
    edges: &[usize],
    start: Option<usize>,
    end: Option<usize>,
    here: Option<Point>,
    stopped: &dyn Fn() -> bool,
) -> Option<Route> {
    let free = route(graph, edges, start, end, stopped)?;
    let (Some(h), None) = (here, start) else { return Some(free) };
    let free = if end.is_none() && node(graph, free.end).distance(h) < node(graph, free.start).distance(h) { reversed(free) } else { free };
    let Some((nearest, _)) = nearest_node(graph, edges, h) else { return Some(free) };
    if free.start == nearest {
        return Some(free);
    }
    match route(graph, edges, Some(nearest), end, stopped) {
        Some(near) if twice(graph, &near) <= twice(graph, &free) + SAME_POINT => Some(near),
        _ => Some(free),
    }
}

/// `r` walked the other way, from its end to its start, each piece again sewn the first time it is
/// reached.
fn reversed(r: Route) -> Route {
    let mut steps: Vec<Step> = r.steps.into_iter().rev().map(|s| Step { forward: !s.forward, ..s }).collect();
    let mut sewn: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for s in &mut steps {
        s.again = !sewn.insert(s.edge);
    }
    Route { steps, start: r.end, end: r.start }
}

/// A line whose pieces meet end to end ([`Graph::of`]) sewn from `aim`'s start: each connected part
/// of its pieces as one route ([`route`]), every other piece as a line is sewn, in turn by the rules
/// a line's subpaths take ([`super::geometry::ordered`]): from an entry point, what it lies nearest
/// first, a route starting at its node nearest it; then each time the nearest, a route as near the
/// needle as it can start ([`route_near`]); toward an exit point, what lies nearest it last of the
/// rest, a route ending at its node nearest it, a closed piece going on round to it. Past [`MAX_REORDERED`] of them, the drawing's
/// order beyond the first, each in turn, so that the line's drawings take time in proportion to
/// them. A route with no exit ends where taking it back sews least twice; a stretch sewn back over
/// that costs more than a jump ([`sewn_back`]) is jumped instead. `stopped` is asked between the
/// drawings and in each route's steps, and once it says so the rest is left out. With the stitches,
/// the connecting stitches sewn between them (millimetres, [`Runs::connectors`]).
fn routed(subs: &[SubPath], graph: &Graph, p: &Params, aim: Aim, stopped: &dyn Fn() -> bool) -> (Vec<Stitch>, f64) {
    let parts = graph.components();
    // Each part of two pieces or more is routed, but one shorter than the shortest stitch, a speck
    // left out as each of its pieces would be; the others' pieces are sewn as they are.
    let mut in_part = vec![false; subs.len()];
    let mut items: Vec<(usize, Item)> = Vec::new();
    for (c, edges) in parts.iter().enumerate() {
        if edges.len() < 2 {
            continue;
        }
        let mut first = usize::MAX;
        let mut total = 0.0;
        for e in edges.iter().filter_map(|&e| graph.edges.get(e)) {
            if let Some(slot) = in_part.get_mut(e.sub) {
                *slot = true;
            }
            first = first.min(e.sub);
            total += e.len;
        }
        if total >= p.min_stitch {
            items.push((first, Item::Part(c)));
        }
    }
    items.extend((0..subs.len()).filter(|&i| !in_part.get(i).copied().unwrap_or(true)).map(|i| (i, Item::Piece(i))));
    items.sort_by_key(|&(first, _)| first);
    let mut pending: Vec<Item> = items.into_iter().map(|(_, item)| item).collect();
    let part = |c: usize| parts.get(c).map_or(&[][..], Vec::as_slice);
    let piece = |i: usize| subs.get(i);
    // How near `q` an item can start (or end): a route at a node, a piece at an end (or any point
    // of a closed one).
    let reach = |item: Item, q: Point| match item {
        Item::Part(c) => nearest_node(graph, part(c), q).map_or(f64::INFINITY, |(_, d)| d),
        Item::Piece(i) => piece(i).map_or(f64::INFINITY, |s| s.entered(q).1),
    };
    let nearest_to = |pending: &[Item], q: Point| -> Option<usize> {
        pending.iter().map(|&item| reach(item, q)).enumerate().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    };
    let exit = aim.finite_exit();
    let entry = match aim.start {
        Start::Entry(e) if e.x.is_finite() && e.y.is_finite() => Some(e),
        _ => None,
    };
    let node_near = |item: Item, q: Option<Point>| match (item, q) {
        (Item::Part(c), Some(q)) => nearest_node(graph, part(c), q).map(|(n, _)| n),
        _ => None,
    };
    let mut here = aim.start.point();
    let mut runs = Runs::from(here, p);
    let sew = |item: Item, start: Option<usize>, end: Option<usize>, from: From, toward: Option<Point>, runs: &mut Runs, here: &mut Option<Point>| {
        match item {
            Item::Part(c) => {
                let edges = part(c);
                match route_near(graph, edges, start, end, *here, stopped) {
                    Some(r) => sew_route(graph, &r, p, runs, here),
                    // No route through it (never, for a connected part): its pieces as they are.
                    None => {
                        for e in edges.iter().filter_map(|&e| graph.edges.get(e)) {
                            let from = here.map_or(From::Drawn, From::Needle);
                            sew_piece(&SubPath { pts: e.pts.clone(), closed: false }, from, None, p, runs, here);
                        }
                    }
                }
            }
            Item::Piece(i) => {
                if let Some(s) = subs.get(i) {
                    sew_piece(s, from, toward, p, runs, here);
                }
            }
        }
    };
    if let [only] = pending[..] {
        // One item: from the entry toward the exit.
        let from = entry.map_or(here.map_or(From::Drawn, From::Needle), From::Entry);
        sew(only, node_near(only, entry), node_near(only, exit), from, exit, &mut runs, &mut here);
        let connectors = runs.connectors(p);
        return (runs.join(), connectors);
    }
    // Sewn first: what the entry lies nearest, from it; then last, of the rest, what lies nearest
    // the exit, so that an exit on the entry's own drawing never takes it from the entry.
    let first = entry.and_then(|e| nearest_to(&pending, e).map(|i| (e, pending.remove(i))));
    let last = exit.and_then(|x| nearest_to(&pending, x)).map(|i| pending.remove(i));
    if let Some((e, item)) = first {
        sew(item, node_near(item, Some(e)), None, From::Entry(e), None, &mut runs, &mut here);
    }
    // Then each time the nearest, or past MAX_REORDERED, the drawing's order, each in turn.
    if pending.len() <= MAX_REORDERED {
        while !pending.is_empty() && !stopped() {
            let i = here.and_then(|h| nearest_to(&pending, h)).unwrap_or(0);
            let item = pending.remove(i);
            sew(item, None, None, here.map_or(From::Drawn, From::Needle), None, &mut runs, &mut here);
        }
    } else {
        for item in pending {
            if stopped() {
                break;
            }
            sew(item, None, None, here.map_or(From::Drawn, From::Needle), None, &mut runs, &mut here);
        }
    }
    if let Some(item) = last
        && !stopped()
    {
        sew(item, None, node_near(item, exit), here.map_or(From::Drawn, From::Needle), exit, &mut runs, &mut here);
    }
    let connectors = runs.connectors(p);
    (runs.join(), connectors)
}

/// Where a piece starts ([`sew_piece`]), as a line's subpaths do (`super::geometry::ordered`):
/// nearest an entry point (a closed one exactly there), nearest the needle (an end, or a closed
/// one's point), or as drawn.
#[derive(Clone, Copy, Debug)]
enum From {
    Entry(Point),
    Needle(Point),
    Drawn,
}

/// One piece `s` sewn as a line is: from where `from` says, carried on round to `toward` when it is
/// closed, ending at its end nearer `toward` when it is open; locked; `here` moved to its end.
fn sew_piece(s: &SubPath, from: From, toward: Option<Point>, p: &Params, runs: &mut Runs, here: &mut Option<Point>) {
    let (pts, _) = match from {
        From::Entry(q) => s.starting_at(q),
        From::Needle(q) => s.starting_near(q),
        From::Drawn => (s.points(), false),
    };
    // An open piece sewn last toward an exit ends at its end nearer it.
    let pts = match (s.closed, toward, pts.first(), pts.last()) {
        (false, Some(x), Some(&a), Some(&b)) if a.distance(x) < b.distance(x) => pts.into_iter().rev().collect(),
        _ => pts,
    };
    if length(&pts, s.closed) < p.min_stitch {
        return;
    }
    let mut run = resample(&pts, p.stitch_length, s.closed);
    if let (true, Some(exit)) = (s.closed, toward) {
        run = lapped(run, exit);
    }
    if p.lock {
        run = locked(&run, p.min_stitch);
    }
    *here = run.last().copied().or(*here);
    runs.push(run, Travel::Auto);
}

/// Whether sewing a stretch `back` millimetres long over pieces already sewn, between needle points
/// `from` and `to`, costs more than jumping across (the planner's weights, `super::runs`: travel
/// under stitching [`COST_HIDDEN`] a millimetre; a jump [`COST_LIFT`] and its length, and
/// [`COST_TRIM`] more past the trim length).
fn sewn_back(back: f64, from: Point, to: Point, p: &Params) -> bool {
    let d = from.distance(to);
    let jump = COST_LIFT + d + if d > p.trim { COST_TRIM } else { 0.0 };
    d.is_finite() && back * COST_HIDDEN > jump
}

/// Route `r` sewn: each piece's needle points [`Params::stitch_length`] apart, the same points when
/// it is sewn back over (in its own holes), the pieces' points run on into one another; a stretch
/// sewn back over that costs more than a jump ([`sewn_back`]) jumped instead; each run locked;
/// `here` moved to its end.
fn sew_route(graph: &Graph, r: &Route, p: &Params, runs: &mut Runs, here: &mut Option<Point>) {
    // Each piece's needle points, from its first point to its last, made once: kept for the route's
    // own pieces only, so that a route costs its own pieces however many the line has.
    let mut points: std::collections::BTreeMap<usize, Vec<Point>> = std::collections::BTreeMap::new();
    let mut needle = |edge: usize| -> Vec<Point> {
        points.entry(edge).or_insert_with(|| graph.edges.get(edge).map_or_else(Vec::new, |e| resample(&e.pts, p.stitch_length, false))).clone()
    };
    let mut run: Vec<Point> = Vec::new();
    // A stretch sewn back over, held until it is known whether it is sewn or jumped: its points and
    // its length.
    let mut back: Vec<Point> = Vec::new();
    let mut back_len = 0.0;
    let finish = |run: &mut Vec<Point>, travel: Travel, runs: &mut Runs, here: &mut Option<Point>| {
        if run.len() >= 2 {
            let pts = if p.lock { locked(run, p.min_stitch) } else { std::mem::take(run) };
            *here = pts.last().copied().or(*here);
            runs.push(pts, travel);
        }
        run.clear();
    };
    let append = |to: &mut Vec<Point>, pts: &[Point]| {
        for &q in pts {
            if to.last().is_none_or(|l| l.distance(q) > SAME_POINT) {
                to.push(q);
            }
        }
    };
    let mut travel = Travel::Auto;
    for step in &r.steps {
        let mut pts = needle(step.edge);
        if !step.forward {
            pts.reverse();
        }
        if step.again {
            back_len += graph.edges.get(step.edge).map_or(0.0, |e| e.len);
            append(&mut back, &pts);
            continue;
        }
        if !back.is_empty() {
            // The stretch sewn back over ends here: sewn, or jumped when that costs less.
            let (from, to) = (run.last().copied(), pts.first().copied());
            match (from, to) {
                (Some(a), Some(b)) if sewn_back(back_len, a, b, p) => {
                    finish(&mut run, travel, runs, here);
                    travel = Travel::Lift;
                }
                _ => append(&mut run, &back),
            }
            back.clear();
            back_len = 0.0;
        }
        append(&mut run, &pts);
    }
    // A stretch sewn back over at the route's end leads it to where it is asked to end: sewn.
    append(&mut run, &back);
    finish(&mut run, travel, runs, here);
}
