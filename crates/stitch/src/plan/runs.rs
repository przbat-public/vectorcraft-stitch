//! Runs joined into one block of stitches: how the needle gets from one run to the next (sewn,
//! jumped, or trimmed and jumped), the tie-ins and tie-offs around every long jump, and the
//! stitches too short to sew.

use vectorcraft_geom::Point;

use super::area::{TRAVEL_STITCH, Travels};
use super::geometry::{length, resample, unit};
use super::{MAX_RUNS, MAX_STITCHES, POINT_MISS_MM, Params, Style};
use crate::{Command, SAME_POINT, Stitch};

/// The reach of a lock stitch, tie-in or tie-off (millimetres): how far the needle steps along the
/// line and back so the thread holds. Longer where the shortest stitch asked for is longer, shorter
/// where the line's first stitch is shorter. An estimate.
pub(super) const LOCK: f64 = 1.0;

/// Where a lock stitch, tie-in or tie-off at `at` steps to: [`LOCK`] along the stitch towards
/// `toward`, or `min_stitch` when that is longer, so that the lock is no stitch the planner leaves
/// out as too short; all of the stitch when it is shorter still. `None` when the two are one point.
pub fn lock_step(at: Point, toward: Point, min_stitch: f64) -> Option<Point> {
    unit(toward - at).map(|d| at + d * LOCK.max(min_stitch).min(at.distance(toward)))
}

/// How the needle gets to a run from the end of the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Travel {
    /// Sewn when it is no longer than [`Params::jump`], else lifted.
    Auto,
    /// Never sewn (it would cross a hole or leave the shape): the needle jumps. A jump longer than
    /// [`Params::jump`] is tied off and in, and the thread is cut first when it is longer than
    /// [`Params::trim`]; a short jump leaves no tail worth cutting.
    Lift,
    /// Always sewn (inside the shape: a row turn, a travel run).
    Sew,
}

// What the moves between runs cost, to compare ways of ordering a fill's regions ([`Runs::cost`]).
// The weights are chosen by judgement, not measured: a visible travel is to be avoided much more
// than a hidden one, a jump more than a few millimetres of visible travel, and a trim more than
// both. A jump also costs its length, a millimetre a millimetre.

/// Per millimetre of travel under stitching still to come: it adds stitches but is hidden.
pub(super) const COST_HIDDEN: f64 = 0.25;
/// Per millimetre of travel over stitching already sewn: it shows on top.
pub(super) const COST_SEEN: f64 = 3.0;
/// A jump: the tie-off and tie-in around it, and a thread across the design until it is cut.
pub(super) const COST_LIFT: f64 = 20.0;
/// A trim, on top of its jump: the machine stops to cut the thread.
pub(super) const COST_TRIM: f64 = 50.0;
/// Per millimetre from where the needle came from outside the shape (the object sewn before): a
/// little, so that of two orders the one starting nearer wins.
pub(super) const COST_ENTRY: f64 = 0.1;
/// Per millimetre a shape's end lies from the exit point its object sets, when ways of sewing it
/// are weighed: as much as a travel that shows, since the person asked for that end (twelve
/// millimetres of travel hidden under stitching still to come are worth one millimetre nearer).
pub(super) const COST_EXIT: f64 = COST_SEEN;

/// A shape's runs in sewing order, each with how the needle gets to it.
#[derive(Default)]
pub(super) struct Runs {
    runs: Vec<(Vec<Point>, Travel)>,
    /// Where the needle is before the first run (the end of the object sewn before, or of the
    /// shape's runs so far), if known.
    start: Option<Point>,
    /// The needle starts inside the shape (its runs so far end there), so the first run is
    /// reached by travelling too.
    inside: bool,
    /// The needle goes in where it starts (an entry point, [`Runs::lead_from`]): the way to the
    /// first run is sewn from there, however short.
    lead: bool,
    /// Where the first run reached by a move of this shape's starts inside the shape (before pull
    /// compensation stretched it past the edge), when one was.
    opened: Option<Point>,
    /// Where the last run ends before pull compensation stretched it past the shape's edge (a fill
    /// row's end on the outline), `None` when it ends where it is: the travels inside the shape
    /// start there.
    inner: Option<Point>,
    /// What the moves between runs cost so far ([`COST_HIDDEN`] and the rest).
    pub(super) cost: f64,
    /// What those moves are, counted beside their cost (which they leave as it is): the lifts of
    /// the needle, those of them that cut the thread, and how far travels run over stitching
    /// already sewn (millimetres). A plan aimed at an exit is kept only when it has no more of each
    /// than the plan without it.
    pub(super) moves: Moves,
}

/// The moves between a shape's runs that show, counted ([`Runs::moves`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Moves {
    /// Lifts of the needle inside the shape (jumps).
    pub(super) lifts: usize,
    /// Of those, the ones longer than [`Params::trim`]: the thread is cut first.
    pub(super) trims: usize,
    /// Millimetres of travel over stitching already sewn: it shows on top.
    pub(super) seen: f64,
}

impl Moves {
    /// Whether these are no more than `other`'s: no more lifts, no more cuts, and no more travel
    /// over stitching already sewn than `slack` millimetres beyond `other`'s (float noise aside,
    /// [`SAME_POINT`]): [`FILL_SEEN_SLACK`] for a fill's rows, none for a line.
    pub(super) fn within(&self, other: &Moves, slack: f64) -> bool {
        self.lifts <= other.lifts && self.trims <= other.trims && self.seen <= other.seen + slack + SAME_POINT
    }

    /// These with the move in from `from` (where the needle is before the shape, if known) to
    /// `first` (its first needle point) counted, as the sequence joins objects: a lift past
    /// [`Params::jump`], a cut past [`Params::trim`].
    pub(super) fn moved_in(self, from: Option<Point>, first: Option<Point>, p: &Params) -> Self {
        let Some(d) = from.zip(first).map(|(a, b)| a.distance(b)).filter(|d| d.is_finite()) else { return self };
        Self { lifts: self.lifts + usize::from(d > p.jump), trims: self.trims + usize::from(d > p.trim), ..self }
    }
}

/// The stitch a lead sews from where the needle goes in when no way inside leads on, millimetres:
/// something to tie on to before the jump (an estimate: a short running stitch); and how many
/// directions it tries, an eighth of a turn apart, for one that stays inside.
const LEAD_TIE: f64 = 1.0;
const TIE_TRIES: usize = 8;

/// How much more travel over stitching already sewn a fill's rows aimed at an exit may show than
/// without it, millimetres: the miss the Check tab lets pass ([`POINT_MISS_MM`]), and no more (the
/// project owner's choice, 2026-10-26). The turn round a row's end at the cut an exit makes shows
/// about half a millimetre: measured on a rectangle with its exit mid-side, 0.47 mm of travel ends it
/// 0.30 mm from its exit, against 29.4 mm with none allowed. Lines and satins may show none more.
pub(super) const FILL_SEEN_SLACK: f64 = POINT_MISS_MM;

/// Whether a plan aimed at an exit, ending `aimed` millimetres from it, gains enough over the plan
/// without it, ending `plain` from it, to be kept: at least [`POINT_MISS_MM`] nearer.
pub(super) fn gains(aimed: f64, plain: f64) -> bool {
    aimed + POINT_MISS_MM <= plain
}

/// Whether a plan aimed at an exit could gain enough ([`gains`]) on the plan without it, ending
/// `plain` millimetres from the exit: not when that ends within [`POINT_MISS_MM`] of it, and then
/// the aimed plan isn't sewn at all (a part given the points it starts and ends at now, as
/// Nearest Connection gives every part it joins, plans once).
pub(super) fn can_gain(plain: f64) -> bool {
    gains(0.0, plain)
}

impl Runs {
    /// No runs yet, the needle at `start` (the end of the object sewn before), if known: the shape
    /// starts as near it as it can.
    pub(super) fn from(start: Option<Point>) -> Self {
        Self { start: start.filter(|q| q.x.is_finite() && q.y.is_finite()), ..Self::default() }
    }

    /// No runs yet: the needle goes in at `at` (where a fill is entered, [`super::fill`]; `inside`,
    /// where that is inside the shape when pull compensation stretched it past the edge), and its
    /// first stitches travel from there to the first run, hidden under the stitching to come. A
    /// shape that has runs already is left as it is.
    pub(super) fn lead_from(&mut self, at: Point, inside: Option<Point>) {
        if self.runs.is_empty() && at.x.is_finite() && at.y.is_finite() {
            (self.start, self.inside, self.lead, self.inner) = (Some(at), true, true, inside);
        }
    }

    /// Where the first run starts inside the shape, when a move of this shape reached it.
    pub(super) fn opened(&self) -> Option<Point> {
        self.opened
    }

    /// No runs yet, to follow `before`'s runs: from where they end, inside the shape when they
    /// have any.
    pub(super) fn continuing(before: &Runs) -> Self {
        let lead = before.lead && before.runs.is_empty();
        Self { start: before.here(), inside: !before.runs.is_empty() || before.inside, lead, inner: before.inner, ..Self::default() }
    }

    /// Whether no run is sewn yet.
    pub(super) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Whether the needle starts inside the shape (its runs so far end there, or it goes in at an
    /// entry point), not reached from the object sewn before.
    pub(super) fn inside(&self) -> bool {
        self.inside
    }

    /// The first needle point of the runs, if any.
    pub(super) fn first(&self) -> Option<Point> {
        self.runs.first().and_then(|(run, _)| run.first().copied())
    }

    /// Where the needle is after the last run (before the first: where it starts from).
    pub(super) fn here(&self) -> Option<Point> {
        match self.runs.last() {
            Some((run, _)) => run.last().copied(),
            None => self.start,
        }
    }

    /// Where the needle is, taken back inside the shape: where the last run ends before pull
    /// compensation stretched it ([`Runs::push_stretched`]).
    pub(super) fn here_inside(&self) -> Option<Point> {
        self.inner.or_else(|| self.here())
    }

    /// The last run ends past the shape's edge by pull compensation: `inner` is where it would end
    /// without it ([`Runs::here_inside`]).
    pub(super) fn stretched_to(&mut self, inner: Point) {
        if !self.runs.is_empty() {
            self.inner = Some(inner);
        }
    }

    /// Add `run`, reached as `travel` says. Runs past [`MAX_RUNS`] are dropped (a guard against
    /// absurd parameters; [`super::try_stitches`] refuses such shapes up front).
    pub(super) fn push(&mut self, run: Vec<Point>, travel: Travel) {
        self.push_stretched(run, travel, None);
    }

    /// [`Runs::push`] a run whose end pull compensation stretched past the shape's edge: `inner` is
    /// where it ends without it, inside the shape.
    pub(super) fn push_stretched(&mut self, run: Vec<Point>, travel: Travel, inner: Option<Point>) {
        if self.runs.len() < MAX_RUNS {
            self.runs.push((run, travel));
            self.inner = inner;
        }
    }

    /// Whether no more runs fit.
    pub(super) fn full(&self) -> bool {
        self.runs.len() >= MAX_RUNS
    }

    /// How the needle gets to `to`, the start of the next run inside the area `travels` crosses:
    /// a short move that stays inside is just the next stitch; a longer one, or one that would
    /// leave the shape or cross a hole, travels inside the shape when it can (a travel run is added
    /// here), and is lifted when it can't. A travel may show at most `max_seen` millimetres over
    /// stitching already sewn (`seen` measures that along a way); under stitching still to come it
    /// is hidden, so of any length. The first run of a shape reached from outside it (from the
    /// object sewn before) is reached as the sequence joins objects.
    ///
    /// `to_inside` is `to` without pull compensation (a fill row's start on the outline; `to`
    /// itself when it has none): whether the way stays inside is judged from where the needle is
    /// inside the shape ([`Runs::here_inside`]) to there, and the stitches then reach the
    /// stretched ends, a fraction of a millimetre past the edge.
    ///
    /// What the move costs is added to [`Runs::cost`].
    pub(super) fn approach(
        &mut self,
        travels: &mut Travels<'_>,
        to: Point,
        to_inside: Point,
        max_seen: f64,
        seen: &dyn Fn(&[Point]) -> f64,
        p: &Params,
    ) -> Travel {
        if self.runs.is_empty() {
            self.opened = Some(to_inside);
        }
        let (Some(here), Some(from)) = (self.here(), self.here_inside()) else { return Travel::Auto };
        // The first way from where the needle goes in is sewn, so that it goes in there.
        let lead = std::mem::take(&mut self.lead) && self.runs.is_empty();
        let d = here.distance(to);
        // What the move costs is judged inside the shape, between the places pull compensation
        // stretched the ends from: the compensation changes no sewing order.
        let inside = from.distance(to_inside);
        if self.runs.is_empty() && !self.inside {
            self.cost += COST_ENTRY * inside;
            return Travel::Auto;
        }
        if d <= p.jump && travels.area.contains_segment(from, to_inside) {
            let shows = seen(&[from, to_inside]);
            self.cost += COST_HIDDEN * (inside - shows) + COST_SEEN * shows;
            self.moves.seen += shows;
            if lead {
                self.push(vec![here, to], Travel::Sew);
                return Travel::Sew;
            }
            return Travel::Auto;
        }
        if let Some(mut route) = travels.way(from, to_inside) {
            let shows = seen(&route);
            if shows <= max_seen {
                self.cost += COST_HIDDEN * (length(&route, false) - shows) + COST_SEEN * shows;
                self.moves.seen += shows;
                // The way runs between the two places inside; the needle is at, and goes to, the
                // ends pull compensation stretched.
                if let Some(first) = route.first_mut() {
                    *first = here;
                }
                if let Some(last) = route.last_mut() {
                    *last = to;
                }
                self.push(resample(&route, TRAVEL_STITCH, false), Travel::Sew);
                return Travel::Sew;
            }
        }
        self.cost += COST_LIFT + inside + if d > p.trim { COST_TRIM } else { 0.0 };
        // Counted as [`Runs::join`] sews it: a lift shorter than the shortest stitch is none there.
        self.moves.lifts += usize::from(d >= p.min_stitch);
        self.moves.trims += usize::from(d > p.trim);
        if lead {
            // No way inside from where the needle goes in: it goes in there all the same, with a
            // stitch to tie on to before the jump (a needle point alone can't be tied off): toward
            // where it goes next, else the first of the eighths of a turn from there that stays
            // inside.
            let toward = to_inside - from;
            let step = (0..TIE_TRIES)
                .map(|k| vectorcraft_geom::Affine::rotate(std::f64::consts::TAU * k as f64 / TIE_TRIES as f64) * toward.to_point())
                .map(|v| v.to_vec2().normalize() * LEAD_TIE)
                .find(|v| v.x.is_finite() && v.y.is_finite() && travels.area.contains_segment(from, from + *v));
            self.push(step.map_or_else(|| vec![here], |v| vec![here, here + v]), Travel::Auto);
        }
        Travel::Lift
    }

    /// How far the needle is sewn between runs reached as any move is ([`Travel::Auto`]) and no
    /// longer than [`Params::jump`] (millimetres): a line's connecting stitches between its
    /// subpaths, which [`Runs::join`] sews straight across, on top of what is sewn.
    pub(super) fn connectors(&self, p: &Params) -> f64 {
        self.runs
            .windows(2)
            .filter_map(|w| {
                let [(before, _), (run, Travel::Auto)] = w else { return None };
                let d = before.last()?.distance(*run.first()?);
                (d > SAME_POINT && d <= p.jump).then_some(d)
            })
            .sum()
    }

    /// `other`'s runs (planned from where these end, [`Runs::continuing`]) after these.
    pub(super) fn append(&mut self, other: Runs) {
        if self.runs.is_empty() && self.opened.is_none() {
            self.opened = other.opened;
        }
        let stretched = !other.runs.is_empty();
        for (run, travel) in other.runs {
            self.push(run, travel);
        }
        if stretched {
            self.inner = other.inner;
        }
        self.cost += other.cost;
        self.moves.lifts += other.moves.lifts;
        self.moves.trims += other.moves.trims;
        self.moves.seen += other.moves.seen;
    }

    /// The runs as one block of stitches. Satin and fill tie the thread in after every jump longer
    /// than [`Params::jump`] (and at the start) and off before it (and at the end); a running line
    /// locks each of its own runs ([`super::running`]). A stitch shorter than [`Params::min_stitch`]
    /// is left out: a repeated point is noise, and a tiny stitch can break the thread or cut the
    /// fabric.
    pub(super) fn join(self, p: &Params) -> Vec<Stitch> {
        let tie = p.lock && p.style != Style::Running;
        let mut out: Vec<Stitch> = Vec::new();
        // Whether the next run is tied in: the first one, and each one after a long jump.
        let mut fresh = true;
        for (mut run, travel) in self.runs {
            run.dedup_by(|b, a| a.distance(*b) <= SAME_POINT);
            let Some(&first) = run.first() else { continue };
            if let Some(last) = out.last().copied()
                && let d = last.distance(Stitch::sew(first))
                && d > SAME_POINT
            {
                let long = d > p.jump;
                match travel {
                    // Shorter than the shortest stitch: nothing to lift the needle over.
                    Travel::Sew | Travel::Lift if d < p.min_stitch => {}
                    Travel::Sew => {}
                    Travel::Auto if !long => {}
                    // A short lift: the needle jumps with the thread on, too short a connector to
                    // cut.
                    Travel::Lift if !long => out.push(Stitch::jump(first)),
                    Travel::Auto | Travel::Lift => {
                        connect(&mut out, first, d, p, tie);
                        fresh = true;
                    }
                }
            }
            if tie && fresh {
                run = tie_in(&run, p.min_stitch);
            }
            fresh = false;
            for q in run {
                if out.len() >= MAX_STITCHES {
                    return out;
                }
                // The stitch where a jump lands is always sewn: the needle goes down there,
                // holding the thread. (A stitch exactly the shortest allowed is kept, rounding
                // aside: the locks step that far.)
                if out.last().is_none_or(|last| last.command != Command::Stitch || last.point().distance(q) + SAME_POINT >= p.min_stitch) {
                    out.push(Stitch::sew(q));
                }
            }
        }
        if tie {
            tie_off(&mut out, p.min_stitch);
        }
        out
    }
}

/// A jump `d` long from the end of `out` to `to`, longer than [`Params::jump`]: tied off first when
/// `tie`, and the thread cut when it is longer than [`Params::trim`].
fn connect(out: &mut Vec<Stitch>, to: Point, d: f64, p: &Params, tie: bool) {
    if tie {
        tie_off(out, p.min_stitch);
    }
    if d > p.trim
        && let Some(last) = out.last().copied()
    {
        out.push(Stitch::new(last.x, last.y, Command::Trim));
    }
    out.push(Stitch::jump(to));
}

/// The pieces of one block that a cut separated (a stroke's stitches cut by a clipping path), each
/// kept as it is, joined again: the needle lifts from one piece to the next as [`Runs::join`]
/// lifts it, in any style (a cut running line comes undone as easily as a satin): a jump, tied off
/// before and in after when it is longer than [`Params::jump`] and [`Params::lock`] is on, and the
/// thread cut first when it is longer than [`Params::trim`].
pub fn join_cut(pieces: Vec<Vec<Stitch>>, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    let mut out: Vec<Stitch> = Vec::new();
    for piece in pieces {
        // A piece starts where the needle goes down.
        let start = piece.iter().position(|s| s.command == Command::Stitch).unwrap_or(piece.len());
        let piece = piece.get(start..).unwrap_or_default();
        let Some(&first) = piece.first() else { continue };
        let mut tie = false;
        if let Some(last) = out.iter().rev().find(|s| s.command == Command::Stitch).copied()
            && let d = last.distance(first)
            && d > SAME_POINT
        {
            if d <= p.jump {
                out.push(Stitch::jump(first.point()));
            } else {
                connect(&mut out, first.point(), d, &p, p.lock);
                tie = p.lock;
            }
        }
        if tie && let Some(next) = piece.get(1).filter(|s| s.command == Command::Stitch) {
            // The tie-in's three points: down at the start, a step forward, back at the start.
            out.extend(tie_in(&[first.point(), next.point()], p.min_stitch).into_iter().take(3).map(Stitch::sew));
            out.extend(piece.get(1..).unwrap_or_default());
        } else {
            out.extend_from_slice(piece);
        }
        if out.len() >= MAX_STITCHES {
            break;
        }
    }
    out
}

/// `run` started with a tie-in: a short stitch forward along it and back, so the cut thread's
/// tail is held; no shorter than `min_stitch` ([`lock_step`]).
fn tie_in(run: &[Point], min_stitch: f64) -> Vec<Point> {
    let (Some(&first), Some(&next)) = (run.first(), run.get(1)) else { return run.to_vec() };
    let Some(step) = lock_step(first, next, min_stitch) else { return run.to_vec() };
    let mut out = Vec::with_capacity(run.len() + 2);
    out.push(first);
    out.push(step);
    out.extend_from_slice(run);
    out
}

/// A tie-off after the last stitch: a short stitch back along it and forward again, before the
/// thread is cut; no shorter than `min_stitch` ([`lock_step`]).
fn tie_off(out: &mut Vec<Stitch>, min_stitch: f64) {
    let [.., a, b] = out.as_slice() else { return };
    let (a, b) = (*a, *b);
    if a.command != Command::Stitch || b.command != Command::Stitch {
        return;
    }
    let Some(step) = lock_step(b.point(), a.point(), min_stitch) else { return };
    out.push(Stitch::sew(step));
    out.push(b);
}
