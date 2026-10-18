//! Runs joined into one block of stitches: how the needle gets from one run to the next (sewn,
//! jumped, or trimmed and jumped), the tie-ins and tie-offs around every cut, and the stitches too
//! short to sew.

use vectorcraft_geom::Point;

use super::area::Area;
use super::geometry::unit;
use super::{MAX_RUNS, MAX_STITCHES, Params, Style};
use crate::{Command, Stitch};

/// The reach of a lock stitch, tie-in or tie-off, millimetres.
pub(super) const LOCK: f64 = 1.0;

/// How the needle gets to a run from the end of the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Travel {
    /// Sewn when it is no longer than [`Params::jump`], else trimmed and jumped.
    Auto,
    /// Never sewn (it would cross a hole or leave the shape): the needle jumps, and the thread is
    /// cut first when the move is longer than [`Params::jump`] (a short jump leaves no tail worth
    /// cutting, as embroidery software leaves short connectors as jumps).
    Lift,
    /// Always sewn (inside the shape: a row turn, a travel run).
    Sew,
}

/// A shape's runs in sewing order, each with how the needle gets to it.
#[derive(Default)]
pub(super) struct Runs {
    runs: Vec<(Vec<Point>, Travel)>,
}

impl Runs {
    /// Where the needle is after the last run.
    pub(super) fn here(&self) -> Option<Point> {
        self.runs.last().and_then(|(run, _)| run.last().copied())
    }

    /// Add `run`, reached as `travel` says. Runs past [`MAX_RUNS`] are dropped (a guard against
    /// absurd parameters; [`super::try_stitches`] refuses such shapes up front).
    pub(super) fn push(&mut self, run: Vec<Point>, travel: Travel) {
        if self.runs.len() < MAX_RUNS {
            self.runs.push((run, travel));
        }
    }

    /// Whether no more runs fit.
    pub(super) fn full(&self) -> bool {
        self.runs.len() >= MAX_RUNS
    }

    /// How the needle gets to `to`, the start of the next run inside `area`: a short move that
    /// stays inside is just the next stitch; a longer one, or one that would leave the shape or
    /// cross a hole, travels inside the shape when it can (a travel run at most `max` long is
    /// added here), and is lifted when it can't.
    pub(super) fn approach(&mut self, area: Area<'_>, to: Point, max: f64, p: &Params) -> Travel {
        let Some(here) = self.here() else { return Travel::Auto };
        if here.distance(to) <= p.jump && area.contains_segment(here, to) {
            return Travel::Auto;
        }
        match area.travel(here, to, max) {
            Some(route) => {
                self.push(route, Travel::Sew);
                Travel::Sew
            }
            None => Travel::Lift,
        }
    }

    /// The runs as one block of stitches. Satin and fill tie the thread in after every cut (and at
    /// the start) and off before every cut (and at the end); a running line locks each of its own
    /// runs ([`super::running`]). A stitch shorter than [`Params::min_stitch`] is left out: a
    /// repeated point is noise, and a tiny stitch can break the thread or cut the fabric.
    pub(super) fn join(self, p: &Params) -> Vec<Stitch> {
        let tie = p.lock && p.style != Style::Running;
        let mut out: Vec<Stitch> = Vec::new();
        let mut fresh = true;
        for (mut run, travel) in self.runs {
            run.dedup_by(|b, a| a.distance(*b) <= 1e-9);
            let Some(&first) = run.first() else { continue };
            if let Some(last) = out.last().copied()
                && let d = last.distance(Stitch::sew(first))
                && d > 1e-9
            {
                let trim = d > p.jump;
                match travel {
                    Travel::Sew => {}
                    Travel::Auto if !trim => {}
                    // A short lift: the needle jumps with the thread on.
                    Travel::Lift if !trim => out.push(Stitch::jump(first)),
                    Travel::Auto | Travel::Lift => {
                        if tie {
                            tie_off(&mut out);
                        }
                        let last = out.last().copied().unwrap_or(last);
                        out.push(Stitch::new(last.x, last.y, Command::Trim));
                        out.push(Stitch::jump(first));
                        fresh = true;
                    }
                }
            }
            if tie && fresh {
                run = tie_in(&run);
            }
            fresh = false;
            for q in run {
                if out.len() >= MAX_STITCHES {
                    return out;
                }
                // The stitch where a jump lands is always sewn: the needle goes down there,
                // holding the thread.
                if out.last().is_none_or(|last| last.command != Command::Stitch || last.point().distance(q) >= p.min_stitch) {
                    out.push(Stitch::sew(q));
                }
            }
        }
        if tie {
            tie_off(&mut out);
        }
        out
    }
}

/// `run` started with a tie-in: a short stitch forward along it and back, so the cut thread's
/// tail is held.
fn tie_in(run: &[Point]) -> Vec<Point> {
    let (Some(&first), Some(&next)) = (run.first(), run.get(1)) else { return run.to_vec() };
    let Some(d) = unit(next - first) else { return run.to_vec() };
    let mut out = Vec::with_capacity(run.len() + 2);
    out.push(first);
    out.push(first + d * LOCK.min(first.distance(next)));
    out.extend_from_slice(run);
    out
}

/// A tie-off after the last stitch: a short stitch back along it and forward again, before the
/// thread is cut.
fn tie_off(out: &mut Vec<Stitch>) {
    let [.., a, b] = out.as_slice() else { return };
    let (a, b) = (*a, *b);
    if a.command != Command::Stitch || b.command != Command::Stitch {
        return;
    }
    let Some(d) = unit(a.point() - b.point()) else { return };
    out.push(Stitch::sew(b.point() + d * LOCK.min(a.distance(b))));
    out.push(b);
}
