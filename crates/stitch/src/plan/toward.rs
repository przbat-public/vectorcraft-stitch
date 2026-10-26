//! A line (a running line, a satin's centreline) sewn toward an exit point its object sets: its
//! subpaths ordered and turned so that it ends as near the exit as its stitches can take it,
//! without a jump, a cut or more connecting stitches between its subpaths than it sews without the
//! exit ([`ordered_toward`]).
//!
//! - **One open subpath** whose start is free (no entry point set) starts at whichever end leaves it
//!   nearer the exit: the exit beats where the needle happens to be. Where its style can also end it
//!   at its other end ([`Finish::flip`]: a satin column's pass along its middle), it is turned round
//!   only when that costs less than the pass, the move in from the needle weighed, and lifts and
//!   cuts no more often ([`turned_or_passed`]). Started at a set entry, it ends where its stitches
//!   take it (a running line is never sewn back over itself), unless its style can end it at either
//!   end.
//! - **A closed subpath** starts where it would without the exit, and goes on past its start to its
//!   point nearest the exit ([`Finish::reach`]: a running line's stitches again in its own holes, the
//!   shorter way round; a satin ring's travel along its middle under its top stitching).
//! - **Several subpaths:** the first is the one it would sew first without the exit (nearest the
//!   entry, or the needle); of the rest, each is tried as the last, turned or carried on toward the
//!   exit, the others before it nearest next, and the cheapest kept by the planner's own costs (a
//!   jump and a cut as `runs` weighs them, a connecting stitch as a travel that shows) with
//!   [`COST_EXIT`] for each millimetre its end lies from the exit. The order without the exit is
//!   one of the tries, and no try is kept that jumps, cuts or connects more than it does. Past
//!   [`MAX_REORDERED`] subpaths, or past [`MAX_REORDER_WORK`] (the subpaths squared times their
//!   points), the one ending nearest the exit is moved last, the rest as without, kept only when
//!   the moves between them lift, cut and connect no more than without the exit.
//!
//! The stitches sewn so are kept only when, counting the move in from where the needle is, they
//! lift the needle and cut the thread no more often than without the exit, sew no more connecting
//! stitches, and end at least [`super::POINT_MISS_MM`] nearer it ([`nearer`]): an exit changes a plan only
//! to bring its end nearer, as it does a fill's rows.

use vectorcraft_geom::Point;

use super::geometry::{Line, MAX_REORDERED, SubPath, length, nearest_on, nearest_order, ordered};
use super::runs::{COST_EXIT, COST_LIFT, COST_SEEN, COST_TRIM, Moves, can_gain, gains};
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

/// `subs` in the order and directions a line sewn from `aim`'s start toward its exit takes them
/// (the module's rules), each line's own end left as `finish` says and its last one marked with
/// the exit when it is to be carried on toward it ([`Line::toward`]). Without an exit (or one that
/// isn't a number), [`ordered`]: as before.
pub(super) fn ordered_toward(subs: Vec<SubPath>, aim: Aim, finish: &Finish, p: &Params) -> Vec<Line> {
    let Some(exit) = aim.exit.filter(|x| x.x.is_finite() && x.y.is_finite()) else { return ordered(subs, aim.start) };
    match subs.len() {
        0 => Vec::new(),
        1 => one(subs, aim.start, exit, finish, p),
        n if n > MAX_REORDERED || reorder_work(&subs) > MAX_REORDER_WORK => many(subs, aim.start, exit, finish, p),
        _ => several(subs, aim.start, exit, finish, p),
    }
}

/// The stitches `sew` sews toward `aim`'s exit, kept only when they are no worse than those it sews
/// without the exit ([`Moves::within`]: no more lifts of the needle or cuts of the thread, the move
/// in from where the needle starts among them, and no more connecting stitches between subpaths,
/// which `sew` gives beside its stitches) and end at least [`super::POINT_MISS_MM`] nearer the exit
/// ([`gains`]); else those. Without an exit (or one that isn't a number), those alone, planned once.
pub(super) fn nearer(aim: Aim, p: &Params, sew: impl Fn(Aim) -> (Vec<Stitch>, f64)) -> Vec<Stitch> {
    let (plain, plain_connectors) = sew(Aim { exit: None, ..aim });
    let Some(exit) = aim.exit.filter(|x| finite(*x)) else { return plain };
    let from = aim.start.point();
    let from_exit = |end: Option<Point>| end.map_or(f64::INFINITY, |q| q.distance(exit));
    let (plain_moves, plain_end) = breaks(&plain, from, plain_connectors, p);
    if !can_gain(from_exit(plain_end)) {
        return plain;
    }
    let (aimed, connectors) = sew(aim);
    let (moves, end) = breaks(&aimed, from, connectors, p);
    // No connecting stitch more at all (the project owner's choice, 2026-10-26: a fill's rows may
    // turn round a row's end, a line has no such turn to spare).
    if moves.within(&plain_moves, 0.0) && gains(from_exit(end), from_exit(plain_end)) { aimed } else { plain }
}

/// How `stitches` move the needle, and their last needle point: how often they lift it between two
/// needle points (a jump, however many records it takes) and cut the thread, and the `connectors`
/// sewn between their subpaths (millimetres) as what shows; with the move in from `from`, where the
/// needle starts, as the sequence joins objects: a lift past [`Params::jump`], a cut past
/// [`Params::trim`].
fn breaks(stitches: &[Stitch], from: Option<Point>, connectors: f64, p: &Params) -> (Moves, Option<Point>) {
    let mut moves = Moves { seen: connectors, ..Moves::default() };
    let (mut up, mut end) = (false, None);
    for s in stitches {
        match s.command {
            Command::Stitch => {
                if end.is_none()
                    && let Some(d) = from.map(|q| q.distance(s.point())).filter(|d| d.is_finite())
                {
                    moves.lifts += usize::from(d > p.jump);
                    moves.trims += usize::from(d > p.trim);
                }
                moves.lifts += usize::from(up && end.is_some());
                (up, end) = (false, Some(s.point()));
            }
            Command::Trim => moves.trims += 1,
            _ => up = true,
        }
    }
    (moves, end)
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
fn one(subs: Vec<SubPath>, start: Start, exit: Point, finish: &Finish, p: &Params) -> Vec<Line> {
    let entered = matches!(start, Start::Entry(e) if finite(e));
    let open = subs.first().is_some_and(|s| !s.closed);
    if open && !entered {
        let Some(sub) = subs.into_iter().next() else { return Vec::new() };
        let (Some(&a), Some(&b)) = (sub.pts.first(), sub.pts.last()) else { return Vec::new() };
        let (from_a, from_b) = (natural_end(a, b, true, finish).distance(exit), natural_end(a, b, false, finish).distance(exit));
        if from_a == from_b {
            // Both ends as near: as without the exit.
            return ordered(vec![sub], start);
        }
        let turned = if from_a < from_b {
            Line { pts: sub.points(), closed: false, reversed: false, toward: None }
        } else {
            let mut pts = sub.pts.clone();
            pts.reverse();
            Line { pts, closed: false, reversed: true, toward: None }
        };
        return match (finish.flip, start) {
            (Some(flip), Start::Needle(h)) if finite(h) => turned_or_passed(sub, h, exit, flip, turned, p),
            _ => vec![turned],
        };
    }
    let mut lines = ordered(subs, start);
    if let Some(line) = lines.first_mut()
        && (line.closed || finish.flip.is_some())
    {
        line.toward = Some(exit);
    }
    lines
}

/// A free open subpath whose style can also end it at its other end (`flip`, a millimetre of its
/// length: a satin column's pass along its middle), the needle at `here`: `turned` (started at the
/// end that ends it nearer `exit`), or started at its end nearest the needle and carried on toward
/// the exit, whichever costs less by the planner's weights with the move in from the needle (the
/// project owner's choice, 2026-10-26); turned only when that move lifts the needle and cuts the
/// thread no more often.
fn turned_or_passed(sub: SubPath, here: Point, exit: Point, flip: f64, turned: Line, p: &Params) -> Vec<Line> {
    let mut passed = ordered(vec![sub], Start::Needle(here));
    let (Some(&from), Some(&turned_from)) = (passed.first().and_then(|l| l.pts.first()), turned.pts.first()) else { return vec![turned] };
    if from == turned_from {
        // Started nearest the needle, it ends nearer the exit already.
        return vec![turned];
    }
    let (mut turn, mut pass) = (Hops::default(), Hops::default());
    turn.add(here.distance(turned_from), p);
    pass.add(here.distance(from), p);
    let way = length(&turned.pts, false) * flip;
    if turn.lifts <= pass.lifts && turn.trims <= pass.trims && turn.cost <= pass.cost + way {
        return vec![turned];
    }
    if let Some(line) = passed.first_mut() {
        line.toward = Some(exit);
    }
    passed
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

    /// Whether these are no more than `plain`'s: no more lifts or cuts, and no more connecting
    /// stitches (float noise aside, [`SAME_POINT`]).
    fn within(&self, plain: &Hops) -> bool {
        self.lifts <= plain.lifts && self.trims <= plain.trims && self.seen <= plain.seen + SAME_POINT
    }
}

/// Where a subpath sewn nearest next from `here` starts and ends ([`nearest_order`]'s rule): an
/// open one from its end nearest (the first on a tie) to the other, or back to it where its style
/// ends it where it starts ([`Finish::returns`]: a satin column whose underlay goes to its far end);
/// a closed one from its point nearest round to it.
fn sewn_from(sub: &SubPath, here: Point, finish: &Finish) -> Option<(Point, Point)> {
    let (i, _) = sub.nearest(here);
    let at = *sub.pts.get(i)?;
    if sub.closed {
        return Some((at, at));
    }
    let other = if i == 0 { *sub.pts.last()? } else { *sub.pts.first()? };
    Some((at, if finish.returns { at } else { other }))
}

/// `subs[k]` for each `k` of `which`, sewn nearest next from `here` as [`nearest_order`] sews them,
/// each ended as `finish` says: where the needle is after them, and the moves into each.
fn greedy(subs: &[SubPath], which: &[usize], here: Point, finish: &Finish, p: &Params) -> (Point, Hops) {
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
        let Some((from, to)) = subs.get(i).and_then(|s| sewn_from(s, here, finish)) else { continue };
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
    let (plain_end, plain) = greedy(&subs, &rest, here, finish, p);
    let mut best: Option<(usize, Last)> = None;
    let mut best_cost = plain.cost + COST_EXIT * plain_end.distance(exit);
    for &j in &rest {
        let others: Vec<usize> = rest.iter().copied().filter(|&i| i != j).collect();
        let (there, hops) = greedy(&subs, &others, here, finish, p);
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

/// How much work weighing each subpath as the last takes ([`several`]: for each, the rest sewn
/// nearest next, each step measuring every subpath left): the subpaths squared times their points.
fn reorder_work(subs: &[SubPath]) -> f64 {
    let n = subs.len() as f64;
    n * n * subs.iter().map(|s| s.pts.len() as f64).sum::<f64>()
}

/// The most work ([`reorder_work`]) a line's subpaths are weighed each as the last with: past it,
/// [`many`]'s one move (an estimate: a few milliseconds of distances; 64 traced contours of 100
/// points each come to 26 million).
const MAX_REORDER_WORK: f64 = 4.0e6;

/// The moves between `lines` sewn in turn, each ended as `finish` says (a closed line where it
/// starts, an open one at its other end or back at its start).
fn hops_of(lines: &[Line], finish: &Finish, p: &Params) -> Hops {
    let mut hops = Hops::default();
    let mut here: Option<Point> = None;
    for line in lines {
        if let (Some(h), Some(&s)) = (here, line.pts.first()) {
            hops.add(h.distance(s), p);
        }
        here = if line.closed || finish.returns { line.pts.first() } else { line.pts.last() }.copied();
    }
    hops
}

/// Past [`MAX_REORDERED`] subpaths, or [`MAX_REORDER_WORK`]: as without the exit (the first, then
/// the rest as drawn past [`MAX_REORDERED`], else nearest next), but the subpath of the rest that
/// can end nearest the exit sewn last, turned or carried on toward it, kept only when the moves
/// between them lift, cut and connect no more than without the exit; else as without it.
fn many(subs: Vec<SubPath>, start: Start, exit: Point, finish: &Finish, p: &Params) -> Vec<Line> {
    let as_drawn = subs.len() > MAX_REORDERED;
    let Some((first_index, first)) = first_of(&subs, start, as_drawn) else { return Vec::new() };
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
    let rest = subs.iter().enumerate().filter(|(i, _)| *i != first_index && Some(*i) != moved).map(|(_, s)| s);
    if as_drawn {
        out.extend(rest.map(|s| Line { pts: s.points(), closed: s.closed, reversed: false, toward: None }));
    } else {
        out.extend(nearest_order(rest.cloned().collect(), Some(here)));
    }
    if let Some((j, l)) = last
        && let Some(sub) = subs.get(j)
    {
        out.push(last_line(sub, &l, exit));
    }
    let plain = ordered(subs, start);
    if hops_of(&out, finish, p).within(&hops_of(&plain, finish, p)) { out } else { plain }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Style;

    fn open(a: (f64, f64), b: (f64, f64)) -> SubPath {
        SubPath { pts: vec![Point::new(a.0, a.1), Point::new(b.0, b.1)], closed: false }
    }

    const RUNNING: Finish = Finish { returns: false, flip: None, reach: 0.25 };
    const RETURNING: Finish = Finish { returns: true, flip: Some(0.25), reach: 0.25 };

    /// A column whose style ends it where it starts (a satin's underlay to its far end, its top
    /// stitching back) leaves the needle at the end it came in by, in the model the reordering
    /// weighs; a running line at its other end.
    #[test]
    fn a_returning_column_leaves_the_needle_where_it_came_in() {
        let sub = open((0.0, 0.0), (20.0, 0.0));
        let near = Point::new(-1.0, 0.0);
        assert_eq!(sewn_from(&sub, near, &RUNNING), Some((Point::new(0.0, 0.0), Point::new(20.0, 0.0))));
        assert_eq!(sewn_from(&sub, near, &RETURNING), Some((Point::new(0.0, 0.0), Point::new(0.0, 0.0))));
        let p = Params::for_style(Style::Satin).sane();
        let subs = [sub, open((2.0, 3.0), (2.0, 30.0))];
        let (end, _) = greedy(&subs, &[0, 1], near, &RETURNING, &p);
        assert_eq!(end, Point::new(2.0, 3.0), "each column back at the end it came in by");
    }

    /// Weighing each subpath as the last is bounded: 64 traced contours of 100 points each are past
    /// it (the one-move rule then), 20 short ones within it.
    #[test]
    fn weighing_every_subpath_last_is_bounded() {
        let contour = |n: usize| SubPath { pts: (0..n).map(|k| Point::new(k as f64, 0.0)).collect(), closed: true };
        assert!(reorder_work(&vec![contour(100); 64]) > MAX_REORDER_WORK);
        assert!(reorder_work(&vec![contour(50); 20]) <= MAX_REORDER_WORK);
    }

    /// Past the bound, the subpath moved last is kept only when the moves between the lines lift,
    /// cut and connect no more than without the exit: seventy hatch lines 1 mm apart keep their
    /// order, as moving the one nearest an exit at line 68 would sew two connecting stitches more.
    #[test]
    fn a_line_moved_last_past_the_bound_connects_no_more() {
        let p = Params::for_style(Style::Running).sane();
        let hatches: Vec<SubPath> =
            (0..70).map(|k| if k % 2 == 0 { open((0.0, k as f64), (30.0, k as f64)) } else { open((30.0, k as f64), (0.0, k as f64)) }).collect();
        let start = Start::Needle(Point::new(-1.0, 0.0));
        let plain = ordered(hatches.clone(), start);
        let out = many(hatches, start, Point::new(31.0, 68.0), &RUNNING, &p);
        let starts = |lines: &[Line]| lines.iter().map(|l| l.pts.first().copied()).collect::<Vec<_>>();
        assert_eq!(starts(&out), starts(&plain));
    }
}
