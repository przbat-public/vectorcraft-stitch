//! Running stitch ([`Style::Running`](super::Style::Running)): a single line of stitches along a
//! path, locked at both ends.

use vectorcraft_geom::{BezPath, Point};

use super::geometry::{Line, length, ordered, resample, subpaths};
use super::runs::{Runs, Travel, lock_step};
use super::{Aim, Params};
use crate::Stitch;

/// A running line of stitches along `path`, [`Params::stitch_length`] apart, each run locked at
/// both ends when [`Params::lock`] is on (a closed one too, where it starts and ends, so that a cut
/// after it can't pull it out), sewn from `aim`'s start: each line from its end nearest the needle,
/// the nearest first; from an entry point, the line it lies nearest first, a closed one starting
/// exactly there ([`ordered`]).
pub(super) fn running(path: &BezPath, p: &Params, aim: Aim) -> Vec<Stitch> {
    let p = p.sane();
    let mut runs = Runs::from(aim.start.point());
    for Line { pts, closed, .. } in ordered(subpaths(path), aim.start) {
        if length(&pts, closed) < p.min_stitch {
            continue;
        }
        let mut run = resample(&pts, p.stitch_length, closed);
        if p.lock {
            run = locked(&run, p.min_stitch);
        }
        runs.push(run, Travel::Auto);
    }
    runs.join(&p)
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
