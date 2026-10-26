//! A line (a running line, a satin's centreline) sewn toward an exit point its object sets: its
//! subpaths ordered and turned so that it ends as near the exit as its stitches can take it,
//! without a jump, a cut or more connecting stitches between its subpaths than it sews without the
//! exit ([`ordered_toward`]).
//!
//! - **One open subpath** whose start is free (no entry point set) starts at whichever end leaves it
//!   nearer the exit: the exit beats where the needle happens to be. Started at a set entry, it ends
//!   where its stitches take it (a running line is never sewn back over itself), unless its style can
//!   end it at either end ([`Finish::flip`]: a satin column's pass along its middle).
//! - **A closed subpath** starts where it would without the exit, and goes on past its start to its
//!   point nearest the exit ([`Finish::reach`]: a running line's stitches again in its own holes, the
//!   shorter way round; a satin ring's travel along its middle under its top stitching).
//! - **Several subpaths:** the first is the one it would sew first without the exit (nearest the
//!   entry, or the needle); of the rest, each is tried as the last, turned or carried on toward the
//!   exit, the others before it nearest next, and the cheapest kept by the planner's own costs (a
//!   jump and a cut as `runs` weighs them, a connecting stitch as a travel that shows) with
//!   [`COST_EXIT`] for each millimetre its end lies from the exit. The order without the exit is
//!   one of the tries, and no try is kept that jumps, cuts or connects more than it does. Past
//!   [`MAX_REORDERED`] subpaths, the one ending nearest the exit is moved last, the rest as without.
//!
//! The stitches sewn so are kept only when they lift the needle and cut the thread no more often
//! than without the exit and end nearer it ([`nearer`]): an exit changes a plan only to bring its
//! end nearer, as it does a fill's rows.

use vectorcraft_geom::Point;

use super::area::TRAVEL_STITCH;
use super::geometry::{Line, MAX_REORDERED, SubPath, length, nearest_on, nearest_order, ordered};
use super::runs::{COST_EXIT, COST_LIFT, COST_SEEN, COST_TRIM};
use super::{Aim, Params, Start};
use crate::{Command, SAME_POINT, Stitch};

/// How a style can end a line's subpath where it is asked to.
#[derive(Clone, Copy, Debug)]
pub(super) struct Finish {
    /// An open subpath sewn from one end ends back at it (a satin column whose underlay goes to its
    /// far end, the top stitching coming back); otherwise at its other end.
    pub(super) returns: bool,
    /// It can also end at the other end, at this cost a millimetre of its length (a satin column's
    /// pass along its middle under its top stitching); `None`: never (a running line is not sewn
    /// back over itself).
    pub(super) flip: Option<f64>,
    /// A closed subpath can go on past its start to its point nearest the exit, at this cost a
    /// millimetre of the way.
    pub(super) reach: f64,
}

/// How much more of a line's moves between subpaths may be sewn as connecting stitches than
/// without the exit, millimetres: one travel stitch, by judgement (a move that only shifts a
/// stitch's length between two subpaths).
const SEEN_SLACK: f64 = TRAVEL_STITCH;

/// `subs` in the order and directions a line sewn from `aim`'s start toward its exit takes them
/// (the module's rules), each line's own end left as `finish` says and its last one marked with
/// the exit when it is to be carried on toward it ([`Line::toward`]). Without an exit (or one that
/// isn't a number), [`ordered`]: as before.
pub(super) fn ordered_toward(subs: Vec<SubPath>, aim: Aim, finish: &Finish, p: &Params) -> Vec<Line> {
    let Some(exit) = aim.exit.filter(|x| x.x.is_finite() && x.y.is_finite()) else { return ordered(subs, aim.start) };
    match subs.len() {
        0 => Vec::new(),
        1 => one(subs, aim.start, exit, finish),
        n if n > MAX_REORDERED => many(subs, aim.start, exit, finish),
        _ => several(subs, aim.start, exit, finish, p),
    }
}

/// The stitches `sew` sews toward `aim`'s exit, kept only when they lift the needle and cut the
/// thread no more often than those it sews without the exit and end nearer the exit; else those.
/// Without an exit (or one that isn't a number), those alone, planned once.
pub(super) fn nearer(aim: Aim, sew: impl Fn(Aim) -> Vec<Stitch>) -> Vec<Stitch> {
    let plain = sew(Aim { exit: None, ..aim });
    let Some(exit) = aim.exit.filter(|x| finite(*x)) else { return plain };
    let aimed = sew(aim);
    let ((plain_lifts, plain_cuts, plain_end), (lifts, cuts, end)) = (breaks(&plain), breaks(&aimed));
    let from_exit = |end: Option<Point>| end.map_or(f64::INFINITY, |q| q.distance(exit));
    if lifts <= plain_lifts && cuts <= plain_cuts && from_exit(end) < from_exit(plain_end) { aimed } else { plain }
}

/// How often `stitches` lift the needle between two needle points (a jump, however many records it
/// takes) and cut the thread, and their last needle point.
fn breaks(stitches: &[Stitch]) -> (usize, usize, Option<Point>) {
    let (mut lifts, mut cuts, mut up) = (0, 0, false);
    let mut end = None;
    for s in stitches {
        match s.command {
            Command::Stitch => {
                lifts += usize::from(up && end.is_some());
                (up, end) = (false, Some(s.point()));
            }
            Command::Trim => cuts += 1,
            _ => up = true,
        }
    }
    (lifts, cuts, end)
}

/// Whether `q` is a number on both axes.
fn finite(q: Point) -> bool {
    q.x.is_finite() && q.y.is_finite()
}

/// The end an open subpath from `a` to `b` ends at when it is started at `a` (`from_a`) or at `b`.
fn natural_end(a: Point, b: Point, from_a: bool, finish: &Finish) -> Point {
    if finish.returns == from_a { a } else { b }
}

/// The line of one subpath.
fn one(subs: Vec<SubPath>, start: Start, exit: Point, finish: &Finish) -> Vec<Line> {
    let entered = matches!(start, Start::Entry(e) if finite(e));
    let open = subs.first().is_some_and(|s| !s.closed);
    if open && !entered {
        let Some(sub) = subs.into_iter().next() else { return Vec::new() };
        let (Some(&a), Some(&b)) = (sub.pts.first(), sub.pts.last()) else { return Vec::new() };
        let (from_a, from_b) = (natural_end(a, b, true, finish).distance(exit), natural_end(a, b, false, finish).distance(exit));
        if from_a < from_b {
            return vec![Line { pts: sub.points(), closed: false, reversed: false, toward: None }];
        }
        if from_b < from_a {
            let mut pts = sub.pts;
            pts.reverse();
            return vec![Line { pts, closed: false, reversed: true, toward: None }];
        }
        // Both ends as near: as without the exit.
        return ordered(vec![sub], start);
    }
    let mut lines = ordered(subs, start);
    if let Some(line) = lines.first_mut()
        && (line.closed || finish.flip.is_some())
    {
        line.toward = Some(exit);
    }
    lines
}

/// The moves between a line's subpaths, as the planner weighs them: their cost, the lifts and cuts
/// among them, and the connecting stitches (millimetres).
#[derive(Clone, Copy, Debug, Default)]
struct Hops {
    cost: f64,
    lifts: usize,
    trims: usize,
    seen: f64,
}

impl Hops {
    /// A move of `d` millimetres: sewn when no longer than the jump length, else lifted, and cut
    /// past the trim length.
    fn add(&mut self, d: f64, p: &Params) {
        if !d.is_finite() || d <= SAME_POINT {
            return;
        }
        if d <= p.jump {
            self.cost += COST_SEEN * d;
            self.seen += d;
        } else {
            self.cost += COST_LIFT + d;
            self.lifts += 1;
            if d > p.trim {
                self.cost += COST_TRIM;
                self.trims += 1;
            }
        }
    }

    /// Whether these are no more than `plain`'s: no more lifts or cuts, and connecting stitches at
    /// most [`SEEN_SLACK`] longer in all.
    fn within(&self, plain: &Hops) -> bool {
        self.lifts <= plain.lifts && self.trims <= plain.trims && self.seen <= plain.seen + SEEN_SLACK
    }
}

/// Where a subpath sewn nearest next from `here` starts and ends ([`nearest_order`]'s rule): an
/// open one from its end nearest (the first on a tie) to the other, a closed one from its point
/// nearest round to it.
fn sewn_from(sub: &SubPath, here: Point) -> Option<(Point, Point)> {
    let (i, _) = sub.nearest(here);
    let at = *sub.pts.get(i)?;
    if sub.closed {
        return Some((at, at));
    }
    let other = if i == 0 { *sub.pts.last()? } else { *sub.pts.first()? };
    Some((at, other))
}

/// `subs[k]` for each `k` of `which`, sewn nearest next from `here` as [`nearest_order`] sews them:
/// where the needle is after them, and the moves into each.
fn greedy(subs: &[SubPath], which: &[usize], here: Point, p: &Params) -> (Point, Hops) {
    let mut pending: Vec<usize> = which.to_vec();
    let (mut here, mut hops) = (here, Hops::default());
    while !pending.is_empty() {
        let pick = pending
            .iter()
            .enumerate()
            .map(|(k, &i)| (k, subs.get(i).map_or(f64::INFINITY, |s| s.nearest(here).1)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((k, _)) = pick else { break };
        let i = pending.remove(k);
        let Some((from, to)) = subs.get(i).and_then(|s| sewn_from(s, here)) else { continue };
        hops.add(here.distance(from), p);
        here = to;
    }
    (here, hops)
}

/// One way of sewing a line's last subpath from where the needle is: where it starts and ends, what
/// it costs beyond its moves, and whether it is carried on toward the exit.
struct Last {
    start: Point,
    end: Point,
    extra: f64,
    reversed: bool,
    toward: bool,
}

/// The ways subpath `sub` can be sewn last, the needle at `here`, toward `exit`.
fn lasts(sub: &SubPath, here: Point, exit: Point, finish: &Finish) -> Vec<Last> {
    let mut out = Vec::new();
    if sub.closed {
        let (i, _) = sub.nearest(here);
        let Some(&at) = sub.pts.get(i) else { return out };
        out.push(Last { start: at, end: at, extra: 0.0, reversed: false, toward: false });
        if let Some((seg, q, _)) = nearest_on(&sub.pts, true, exit) {
            let way = round_way(&sub.pts, i, seg, q);
            out.push(Last { start: at, end: q, extra: finish.reach * way, reversed: false, toward: true });
        }
        return out;
    }
    let (Some(&a), Some(&b)) = (sub.pts.first(), sub.pts.last()) else { return out };
    let len = length(&sub.pts, false);
    for from_a in [true, false] {
        let (start, end) = (if from_a { a } else { b }, natural_end(a, b, from_a, finish));
        out.push(Last { start, end, extra: 0.0, reversed: !from_a, toward: false });
        if let Some(flip) = finish.flip {
            let other = if end == a { b } else { a };
            out.push(Last { start, end: other, extra: flip * len, reversed: !from_a, toward: true });
        }
    }
    out
}

/// The shorter way round closed polyline `pts` from its point `from` to `q`, a point of its segment
/// `seg` (from point `seg` to the next), millimetres.
fn round_way(pts: &[Point], from: usize, seg: usize, q: Point) -> f64 {
    let n = pts.len();
    // How far along the polyline each of its points lies.
    let mut along = Vec::with_capacity(n + 1);
    let mut total = 0.0;
    along.push(0.0);
    for i in 0..n {
        let d = match (pts.get(i), pts.get((i + 1) % n)) {
            (Some(a), Some(b)) => a.distance(*b),
            _ => 0.0,
        };
        total += if d.is_finite() { d } else { 0.0 };
        along.push(total);
    }
    if total <= SAME_POINT {
        return 0.0;
    }
    let at = |i: usize| along.get(i).copied().unwrap_or(0.0);
    let to_q = at(seg) + pts.get(seg).map_or(0.0, |s| s.distance(q));
    let ahead = (to_q - at(from)).rem_euclid(total);
    ahead.min(total - ahead).max(0.0)
}

/// The subpath a line sews first from `start`, and how: as [`ordered`] and [`nearest_order`] pick
/// and turn it (the one nearest an entry, started there; the one nearest the needle, from its
/// point nearest; else the first as drawn). Past [`MAX_REORDERED`] subpaths (`many`), from the
/// needle or nowhere, the first as drawn, turned toward the needle, as [`nearest_order`] sews it.
fn first_of(subs: &[SubPath], start: Start, many: bool) -> Option<(usize, Line)> {
    let nearest = |d: &dyn Fn(&SubPath) -> f64| subs.iter().map(d).enumerate().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
    let (i, (pts, reversed)) = match start {
        Start::Entry(e) if finite(e) => {
            let i = nearest(&|s| s.entered(e).1)?;
            (i, subs.get(i)?.starting_at(e))
        }
        Start::Needle(h) if finite(h) => {
            let i = if many { 0 } else { nearest(&|s| s.nearest(h).1)? };
            (i, subs.get(i)?.starting_near(h))
        }
        _ => (0, (subs.first()?.points(), false)),
    };
    let closed = subs.get(i)?.closed;
    Some((i, Line { pts, closed, reversed, toward: None }))
}

/// The line of several subpaths (up to [`MAX_REORDERED`]): the first as without the exit, then each
/// of the rest tried as the last, the cheapest kept.
fn several(subs: Vec<SubPath>, start: Start, exit: Point, finish: &Finish, p: &Params) -> Vec<Line> {
    let Some((first_index, first)) = first_of(&subs, start, false) else { return Vec::new() };
    let Some(&here) = first.pts.last() else { return ordered(subs, start) };
    let rest: Vec<usize> = (0..subs.len()).filter(|&i| i != first_index).collect();
    let (plain_end, plain) = greedy(&subs, &rest, here, p);
    let mut best: Option<(usize, Last)> = None;
    let mut best_cost = plain.cost + COST_EXIT * plain_end.distance(exit);
    for &j in &rest {
        let others: Vec<usize> = rest.iter().copied().filter(|&i| i != j).collect();
        let (there, hops) = greedy(&subs, &others, here, p);
        let Some(sub) = subs.get(j) else { continue };
        for last in lasts(sub, there, exit, finish) {
            let mut all = hops;
            all.add(there.distance(last.start), p);
            let cost = all.cost + last.extra + COST_EXIT * last.end.distance(exit);
            if all.within(&plain) && cost < best_cost {
                best_cost = cost;
                best = Some((j, last));
            }
        }
    }
    let mut out = vec![first];
    match best {
        // As without the exit.
        None => out.extend(nearest_order(rest.iter().filter_map(|&i| subs.get(i)).cloned().collect(), Some(here))),
        Some((j, last)) => {
            out.extend(nearest_order(rest.iter().filter(|&&i| i != j).filter_map(|&i| subs.get(i)).cloned().collect(), Some(here)));
            if let Some(sub) = subs.get(j) {
                out.push(last_line(sub, &last, exit));
            }
        }
    }
    out
}

/// Past [`MAX_REORDERED`] subpaths: as without the exit (the first, then the rest as drawn), but the
/// subpath of the rest that can end nearest the exit sewn last, turned or carried on toward it.
fn many(subs: Vec<SubPath>, start: Start, exit: Point, finish: &Finish) -> Vec<Line> {
    let Some((first_index, first)) = first_of(&subs, start, true) else { return Vec::new() };
    let here = first.pts.last().copied().unwrap_or(exit);
    let pick = subs
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != first_index)
        .filter_map(|(i, s)| {
            lasts(s, here, exit, finish).into_iter().map(|l| (l.end.distance(exit), l)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(d, l)| (d, i, l))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = vec![first];
    let last = pick.map(|(_, j, l)| (j, l));
    let moved = last.as_ref().map(|(j, _)| *j);
    out.extend(subs.iter().enumerate().filter(|(i, _)| *i != first_index && Some(*i) != moved).map(|(_, s)| Line {
        pts: s.points(),
        closed: s.closed,
        reversed: false,
        toward: None,
    }));
    if let Some((j, l)) = last
        && let Some(sub) = subs.get(j)
    {
        out.push(last_line(sub, &l, exit));
    }
    out
}

/// `sub` sewn last as `last` says.
fn last_line(sub: &SubPath, last: &Last, exit: Point) -> Line {
    let toward = last.toward.then_some(exit);
    if sub.closed {
        let (pts, _) = sub.starting_near(last.start);
        return Line { pts, closed: true, reversed: false, toward };
    }
    let mut pts = sub.pts.clone();
    if last.reversed {
        pts.reverse();
    }
    Line { pts, closed: false, reversed: last.reversed, toward }
}
