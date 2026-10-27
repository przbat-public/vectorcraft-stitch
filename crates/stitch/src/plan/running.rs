//! Running stitch ([`Style::Running`](super::Style::Running)): a single line of stitches along a
//! path, locked at both ends.

use vectorcraft_geom::{BezPath, Point};

use super::geometry::{Line, length, resample, subpaths};
use super::runs::{COST_HIDDEN, Runs, Travel, lock_step};
use super::toward::{Finish, ordered_toward};
use super::{Aim, Params};
use crate::Stitch;

/// How a running line ends where it is asked to ([`ordered_toward`]): an open line at its far end,
/// never sewn back over itself; a closed one goes on past its start, again in its own holes, to its
/// point nearest the exit, the doubled stitches weighed as a travel hidden under stitching (they lie
/// in the line's own holes).
const RUNNING: Finish = Finish { returns: false, flip: None, reach: COST_HIDDEN };

/// A running line of stitches along `path`, [`Params::stitch_length`] apart, each run locked at
/// both ends when [`Params::lock`] is on (a closed one too, where it starts and ends, so that a cut
/// after it can't pull it out), sewn from `aim`'s start: each line from its end nearest the needle,
/// the nearest first; from an entry point, the line it lies nearest first, a closed one starting
/// exactly there. Toward an exit point, its lines are ordered and turned to end nearest it, and a
/// closed one sewn last goes on round to it ([`ordered_toward`], [`lapped`]). With the stitches, the
/// connecting stitches sewn between its lines (millimetres, [`Runs::connectors`]).
pub(super) fn running(path: &BezPath, p: &Params, aim: Aim) -> (Vec<Stitch>, f64) {
    let p = p.sane();
    let mut runs = Runs::from(aim.start.point());
    for Line { pts, closed, toward, .. } in ordered_toward(subpaths(path), aim, &RUNNING, &p) {
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
    (runs.join(&p), connectors)
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
