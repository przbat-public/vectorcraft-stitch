//! Satin columns ([`Style::Satin`](super::Style::Satin)): the thread zig-zags across a band along
//! a path, over an underlay chosen by the band's width, with long crossings split, the rails
//! widened by pull compensation and the ends shortened by push compensation.

use vectorcraft_geom::{BezPath, Point, Vec2};

use super::area::TRAVEL_STITCH;
use super::geometry::{CORNER, Line, NO_LENGTH, SubPath, length, nearest_on, resample, resample_by, subpaths, tangent, trimmed, turn, unit};
use super::narrow::Column;
use super::runs::{COST_HIDDEN, Runs, Travel};
use super::toward::{Finish, ordered_toward};
use super::{Aim, MAX_STITCHES, MIN_PUSHED, Params, UNDERLAY_STITCH, Underlay};
use crate::{SAME_POINT, Stitch};

/// The satin column width (millimetres) from which its automatic underlay is an edge run instead
/// of a centre run. An estimate.
pub const SATIN_EDGE_UNDERLAY: f64 = 2.0;
/// The satin column width (millimetres) from which its automatic underlay is an edge run and a
/// zigzag. An estimate.
pub const SATIN_ZIGZAG_UNDERLAY: f64 = 4.5;
/// The zigzag underlay's spacing (millimetres). An estimate.
pub(super) const ZIGZAG_UNDERLAY_SPACING: f64 = 2.0;
/// How far inside the column's sides the edge underlay runs: this share of the column's width, at
/// most [`UNDERLAY_INSET_MAX`] millimetres, so the underlay doesn't show at the edges. Estimates.
const UNDERLAY_INSET: f64 = 0.25;
const UNDERLAY_INSET_MAX: f64 = 0.5;
/// A column shorter than this many of its own spacings ([`Params::column_spacing`]) is left out:
/// it would be a knot, not a column. A stroke's and a fill's narrow part alike. By judgement.
const MIN_CROSSINGS: f64 = 2.0;
/// A satin's spacing is measured between two penetrations on the same rail (peak to peak), as
/// Madeira's stitch density chart (51 satin stitches a centimetre for its 0.40 mm, 40 wt:
/// <https://www.madeirausa.com/_resources/common/userfiles/file/Resources/TH.Reads/Density.pdf>),
/// Ink/Stitch ("zig-zag spacing (peak-to-peak)",
/// <https://inkstitch.org/docs/stitches/satin-column/>) and Hatch measure it: the crossings, which
/// go to one rail and the other in turn, lie this share of it apart.
pub(super) const CROSSINGS_PER_SPACING: f64 = 2.0;
/// Auto spacing ([`satin_spacing`]): for a column narrower than each width (millimetres), the
/// spacing of its penetrations on each rail (millimetres, peak to peak), and [`WIDE_SPACING`] for
/// wider ones. Estimates.
const SPACING_BY_WIDTH: [(f64, f64); 3] = [(2.0, 0.5), (4.0, 0.45), (6.0, 0.4)];
const WIDE_SPACING: f64 = 0.37;
/// The most stitches a long crossing is split into ([`split_long`]): enough for the widest column.
const MAX_SPLIT: f64 = 64.0;
/// How far the cuts of a split crossing move along it, as a share of one part (either way): see
/// [`split_long`]. By judgement: enough that no two neighbours' cuts line up.
const SPLIT_JITTER: f64 = 0.25;

/// The most stitches [`split_long`] cuts a crossing `len` long into with `max` its longest: one
/// when it is no longer.
pub(super) fn splits(len: f64, max: f64) -> f64 {
    ((len * (1.0 + SPLIT_JITTER)) / max).ceil().clamp(1.0, MAX_SPLIT)
}
/// The golden ratio's fractional part: its multiples' fractions spread evenly over the unit
/// interval and never repeat, so the shift of a split moves on from one crossing to the next
/// without a pattern ([`split_long`]).
const GOLDEN: f64 = 0.618_033_988_75;

/// A satin column sewn along `path`'s centreline, [`Params::width`] wide and wider at each rail by
/// its pull compensation ([`Params::compensation`] of the width: the left rail as the path is
/// drawn, then the right one), each open column shorter at its ends by [`Params::push`]. The
/// penetrations on each rail are [`satin_spacing`] apart for 40 wt thread, closer for finer thread
/// ([`thread_scale`]), or [`Params::density`] when auto spacing is off; the crossings, rail to rail
/// in turn, lie half that apart ([`CROSSINGS_PER_SPACING`]). Round a bend the spacing is kept on
/// the outer rail, and the crossings turn with the path ([`Centre`]). From `aim`'s start, where the
/// needle is: each column starts at its end nearest the needle (a closed one at its point
/// nearest), the nearest first, as Ink/Stitch starts satins; from an entry point, the column it
/// lies nearest first, a ring starting exactly there ([`ordered`]). `p`'s style is satin (the
/// caller, [`super::stitches_aimed`], sews a satin only then).
pub(super) fn satin(path: &BezPath, p: &Params, aim: Aim) -> Vec<Stitch> {
    let p = p.sane();
    if p.width <= 0.0 {
        return Vec::new();
    }
    // Pull compensation widens the column at each rail ([`Params::compensation`]): the left one as
    // the path is drawn, then the right one.
    let [left, right] = p.compensation(p.width);
    let half = p.width / 2.0;
    let spacing = p.column_spacing(p.width);
    // The crossings go to one rail and the other in turn: half the spacing apart.
    let pitch = spacing / CROSSINGS_PER_SPACING;
    let mut runs = Runs::from(aim.start.point());
    // An open column whose underlay goes to its far end comes back with its top stitching: it ends
    // where it starts. Toward an exit, a pass along its middle under the top stitching ends it at its
    // other end, and a ring travels along its middle to the exit before its top stitching; both are
    // weighed as travel hidden under stitching still to come.
    let returns = matches!(satin_kind(p.underlay, p.width), Some(SatinUnderlay::Center | SatinUnderlay::EdgeZigzag));
    let finish = Finish { returns, flip: Some(COST_HIDDEN), reach: COST_HIDDEN };
    for Line { pts, closed, reversed, toward } in ordered_toward(subpaths(path), aim, &finish, &p) {
        // Push compensation: an open column is sewn shorter at its ends, the underlay with it.
        let shortest = spacing * MIN_CROSSINGS;
        let pts = if closed { pts } else { pushed(&pts, if reversed { [p.push[1], p.push[0]] } else { p.push }, shortest) };
        if length(&pts, closed) < shortest {
            continue;
        }
        // The first run of each column is reached as any move is; the column's later runs
        // (underlay to top stitching) start inside the column, so the needle sews there.
        let mut travel = Travel::Auto;
        // The underlay, by the column's width; it may end at the column's far end, and then the
        // top stitching starts there and comes back.
        let mut back = false;
        if let Some(kind) = satin_kind(p.underlay, p.width) {
            for (under, ends_far) in satin_underlay(&pts, closed, p.width, kind) {
                if under.len() >= 2 {
                    push_split(&mut runs, under, travel, p.max_satin);
                    travel = Travel::Sew;
                }
                back = ends_far;
            }
        }
        // Toward an exit: an open column whose top stitching would end at its end farther from the
        // exit first runs along its middle to its other end; a ring's top stitching starts at its
        // point nearest the exit, the needle travelling there along its middle.
        let pts = match toward {
            Some(exit) if closed => {
                let (from_exit, way) = ring_toward(&pts, exit);
                if way.len() >= 2 {
                    runs.push(way, travel);
                    travel = Travel::Sew;
                }
                from_exit
            }
            Some(exit) => {
                if let (Some(&a), Some(&b)) = (pts.first(), pts.last()) {
                    let ends_at = if back { a } else { b };
                    let nearer = match a.distance(exit).total_cmp(&b.distance(exit)) {
                        std::cmp::Ordering::Less => Some(a),
                        std::cmp::Ordering::Greater => Some(b),
                        std::cmp::Ordering::Equal => None,
                    };
                    if nearer.is_some_and(|q| q != ends_at) {
                        let mut middle = pts.clone();
                        if back {
                            middle.reverse();
                        }
                        runs.push(resample(&middle, TRAVEL_STITCH, false), travel);
                        travel = Travel::Sew;
                        back = !back;
                    }
                }
                pts
            }
            None => pts,
        };
        // The crossings lie `pitch` apart on the farther rail: round a bend the outer one, as for a
        // fill's columns ([`Along::crossings`]). Along `pts` the rail on the right (y runs down the
        // page) reaches `plus`, the one on the left `minus`: the drawing's right and left, or its
        // left and right when `pts` runs against it.
        let centre = Centre::new(&pts, closed);
        let (plus, minus) = if reversed { (half + left, half + right) } else { (half + right, half + left) };
        let mut samples = resample_by(&pts, closed, &|s| pitch / centre.stretch(s, plus, minus));
        if samples.len() < 2 {
            continue;
        }
        let back = back && !closed;
        if back {
            samples.reverse();
        }
        // Whether the column is sewn against the way it is drawn, so its rails swap sides.
        let against = reversed != back;
        let points: Vec<Point> = samples.iter().map(|&(q, _)| q).collect();
        let crossings: Vec<(Point, Vec2)> = samples
            .iter()
            .enumerate()
            .map(|(i, &(q, s))| {
                // The way the needle goes along the column: the centre line's, turning smoothly;
                // at a sharp corner from the crossing before it to the next, across the corner.
                let t = match centre.direction(s) {
                    Some(d) if !centre.sharp_at(s) => {
                        if back {
                            -d
                        } else {
                            d
                        }
                    }
                    _ => tangent(&points, i, closed),
                };
                // Even crossings go to one rail, odd ones to the other. The normal points to the
                // right of the way the needle goes along the column (y runs down the page).
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                let reach = half + if (side > 0.0) != against { right } else { left };
                (q, Vec2::new(-t.y, t.x) * reach * side)
            })
            .collect();
        let mut zigzag = short_stitches(&crossings, pitch, p.width + left + right);
        // A closed column ends where it started: the last crossing takes the remaining side.
        if closed
            && let (Some(first), Some(last)) = (zigzag.first().copied(), zigzag.last().copied())
            && first.distance(last) > p.min_stitch
        {
            zigzag.push(first);
        }
        push_split(&mut runs, zigzag, travel, p.max_satin);
        if runs.full() {
            break;
        }
    }
    runs.join(&p)
}

/// A pass's or a travel's points beyond its stitches ([`TRAVEL_STITCH`] apart along it), at most:
/// both of its ends, and the one its rounding may add.
pub(super) const PASS_ENDS: f64 = 3.0;

/// A ring's centreline `pts` (closed, its first point repeated at its end) started instead at its
/// point nearest `exit`, and the way along it there from its start, the shorter way round, in
/// travel stitches ([`TRAVEL_STITCH`]): the needle goes there under the top stitching still to come.
/// The way is empty when the start is that point already.
fn ring_toward(pts: &[Point], exit: Point) -> (Vec<Point>, Vec<Point>) {
    let ring: Vec<Point> = match pts.split_last() {
        Some((last, rest)) if rest.first().is_some_and(|f| f.distance(*last) <= SAME_POINT) => rest.to_vec(),
        _ => pts.to_vec(),
    };
    let sub = SubPath { pts: ring.clone(), closed: true };
    let Some((seg, q, _)) = nearest_on(&ring, true, exit) else { return (pts.to_vec(), Vec::new()) };
    let Some(&start) = ring.first() else { return (pts.to_vec(), Vec::new()) };
    if q.distance(start) <= SAME_POINT {
        return (pts.to_vec(), Vec::new());
    }
    let (from_exit, _) = sub.starting_at(exit);
    // Forward: the start, the points up to the segment, the exit's point; back: the other way round.
    let mut ahead: Vec<Point> = ring.get(..=seg).map(<[Point]>::to_vec).unwrap_or_default();
    ahead.push(q);
    let mut behind: Vec<Point> = vec![start];
    behind.extend(ring.iter().skip(seg + 1).rev().copied());
    behind.push(q);
    let way = if length(&ahead, false) <= length(&behind, false) { ahead } else { behind };
    (from_exit, resample(&way, TRAVEL_STITCH, false))
}

/// An open column's centreline `pts` with push compensation: `push` millimetres cut off its start
/// and its end, both in proportion when together they would leave less than [`MIN_PUSHED`] of it,
/// or less than `shortest` (the shortest column that is sewn): push compensation never takes away
/// a column that would be sewn without it.
fn pushed(pts: &[Point], push: [f64; 2], shortest: f64) -> Vec<Point> {
    let [start, end] = push;
    if start + end <= 0.0 {
        return pts.to_vec();
    }
    let k = push_share(length(pts, false), start, end, shortest);
    trimmed(pts, start * k, end * k)
}

/// The share of push compensation `start` and `end` (millimetres off a column's two ends) that a
/// column `len` long takes: all of it, or both in proportion when together they would leave less
/// than [`MIN_PUSHED`] of it, or less than `shortest` (the shortest column that is sewn).
fn push_share(len: f64, start: f64, end: f64, shortest: f64) -> f64 {
    let keep = (len * MIN_PUSHED).max(shortest).min(len);
    let most = len - keep;
    if start + end > most { most / (start + end) } else { 1.0 }
}

/// A stroke's centre line measured along, its direction turning smoothly round its curves. Where
/// the line turns gently (at most [`CORNER`]) its direction is the mean of the segments either
/// side, and along a segment it turns evenly from the direction at one end to the one at the other;
/// at a sharp corner and at an open end a segment keeps its own. So a satin's crossings turn with a
/// curve rather than in steps where it was flattened (round a 6 mm bend, a 5 mm satin's outer rail
/// had its penetrations 0.4 mm apart along each flattened segment and up to 0.97 mm where two met),
/// and the rail on the outside of a bend can be measured as it runs ([`Centre::stretch`]).
struct Centre {
    /// The points, none on the one before, and the distance along to each (as [`resample_by`]
    /// counts it).
    pts: Vec<Point>,
    at: Vec<f64>,
    /// Each segment's direction at its start and at its end.
    ends: Vec<(Vec2, Vec2)>,
    /// Whether the line turns sharply (more than [`CORNER`]) at each point.
    sharp: Vec<bool>,
    len: f64,
    closed: bool,
}

impl Centre {
    fn new(line: &[Point], closed: bool) -> Self {
        let mut pts: Vec<Point> = Vec::with_capacity(line.len());
        for &q in line {
            if pts.last().is_none_or(|p| p.distance(q) > NO_LENGTH) {
                pts.push(q);
            }
        }
        // A closed line's first point, repeated at its end, is one point.
        if closed && pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.distance(*b) <= NO_LENGTH) {
            pts.pop();
        }
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        let dirs: Vec<Vec2> =
            (0..segs).map(|i| pts.get(i).zip(pts.get((i + 1) % n.max(1))).and_then(|(a, b)| unit(*b - *a)).unwrap_or(Vec2::ZERO)).collect();
        let incoming = |k: usize| {
            if k > 0 {
                dirs.get(k - 1)
            } else if closed {
                dirs.last()
            } else {
                None
            }
        };
        let sharp: Vec<bool> = (0..n)
            .map(|k| match (incoming(k), dirs.get(k)) {
                (Some(u), Some(v)) => turn(*u, *v) > CORNER,
                _ => false,
            })
            .collect();
        // The direction where the line turns gently: the mean of the segments either side.
        let gentle = |k: usize| match (incoming(k), dirs.get(k), sharp.get(k)) {
            (Some(u), Some(v), Some(false)) => unit(*u + *v),
            _ => None,
        };
        let ends = (0..segs)
            .map(|i| {
                let d = dirs.get(i).copied().unwrap_or(Vec2::ZERO);
                (gentle(i).unwrap_or(d), gentle((i + 1) % n.max(1)).unwrap_or(d))
            })
            .collect();
        let mut at = Vec::with_capacity(n);
        let mut len = 0.0;
        for k in 0..n {
            if let (Some(a), Some(b)) = (k.checked_sub(1).and_then(|j| pts.get(j)), pts.get(k)) {
                len += a.distance(*b);
            }
            at.push(len);
        }
        if closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
            len += a.distance(*b);
        }
        Self { pts, at, ends, sharp, len, closed }
    }

    /// The segment `s` along (round to the start of a closed line) and the share of the way along
    /// it.
    fn segment(&self, s: f64) -> Option<(usize, f64)> {
        let segs = self.ends.len();
        let last = segs.checked_sub(1)?;
        let s = if self.closed { s.rem_euclid(self.len.max(NO_LENGTH)) } else { s.clamp(0.0, self.len) };
        let i = self.at.partition_point(|&a| a <= s).saturating_sub(1).min(last);
        let start = *self.at.get(i)?;
        let end = if i + 1 < self.at.len() { *self.at.get(i + 1)? } else { self.len };
        let f = if end - start > NO_LENGTH { ((s - start) / (end - start)).clamp(0.0, 1.0) } else { 0.0 };
        Some((i, f))
    }

    /// The line's direction `s` along.
    fn direction(&self, s: f64) -> Option<Vec2> {
        let (i, f) = self.segment(s)?;
        let &(t0, t1) = self.ends.get(i)?;
        unit(t0 + (t1 - t0) * f).or(unit(t0))
    }

    /// Whether `s` along is a point where the line turns sharply.
    fn sharp_at(&self, s: f64) -> bool {
        let Some((i, _)) = self.segment(s) else { return false };
        let s = if self.closed { s.rem_euclid(self.len.max(NO_LENGTH)) } else { s };
        // The segment's two ends: its start and its end (a closed line's last one ends at its first
        // point, its length along).
        let n = self.pts.len().max(1);
        let ends = [(i, self.at.get(i).copied().unwrap_or(0.0)), ((i + 1) % n, self.at.get(i + 1).copied().unwrap_or(self.len))];
        ends.iter().any(|&(k, at)| (at - s).abs() <= SAME_POINT && self.sharp.get(k).copied().unwrap_or(false))
    }

    /// How far the farther of the two rails runs for each millimetre of the line `s` along, the one
    /// on the right (y runs down the page) `plus` from it and the one on the left `minus`: one
    /// along a straight stretch, `1 + R / r` round a bend of radius r with the outer rail R out; at
    /// most [`MAX_RAIL_STRETCH`].
    fn stretch(&self, s: f64, plus: f64, minus: f64) -> f64 {
        let Some((i, _)) = self.segment(s) else { return 1.0 };
        let (Some(&(t0, t1)), Some(&start)) = (self.ends.get(i), self.at.get(i)) else { return 1.0 };
        let end = self.at.get(i + 1).copied().unwrap_or(self.len);
        // How fast the direction turns (radians a millimetre, towards the right rail when
        // positive): the right rail, then on the inside, runs `1 - plus × turn` as far as the line
        // and the left one `1 + minus × turn`.
        let turn = t0.cross(t1).atan2(t0.dot(t1)) / (end - start);
        let farther = (1.0 - plus * turn).abs().max((1.0 + minus * turn).abs());
        if farther.is_finite() { farther.clamp(1.0, MAX_RAIL_STRETCH) } else { 1.0 }
    }
}

/// Short stitches: how far a bunched penetration may move in from its rail, as shares of the
/// column's width, tried in order. Ink/Stitch's satin columns take a list of insets like this
/// ("short stitch inset", <https://inkstitch.org/docs/stitches/satin-column/>), and Hatch shortens
/// stitches on a satin's inner edge to reduce bunching (stitch shortening,
/// <https://hatch.embroideryhelp.net/v3/en/downloads/UserGuide.pdf>); 15–25 % is the usual
/// shortening. Estimates.
const SHORT_INSETS: [f64; 3] = [0.0, 0.2, 0.4];
/// How many earlier penetrations on the same rail a penetration keeps its distance from. An
/// estimate.
const SHORT_LOOKBACK: usize = 4;

/// A satin column's needle points from its `crossings` (each a point on the centreline and the
/// offset to the rail it goes to, the rails alternating), with short stitches where a curve bunches
/// the penetrations on its inner side. A penetration closer than `pitch` (the distance between
/// crossings, half the distance between two on one rail) to one of the last [`SHORT_LOOKBACK`] on
/// its rail moves in by the first of
/// [`SHORT_INSETS`] (shares of the column's `width`, never past its centre) that keeps it that far
/// from them all, or, when none does, by the one that keeps it farthest. On a gentle curve that
/// shortens every other stitch on the inner side; on a tight one the penetrations spread over the
/// insets; along a straight column nothing moves.
fn short_stitches(crossings: &[(Point, Vec2)], pitch: f64, width: f64) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(crossings.len());
    for (i, &(q, offset)) in crossings.iter().enumerate() {
        // The earlier penetrations on this rail: every other one back.
        let before: [Option<Point>; SHORT_LOOKBACK] = std::array::from_fn(|k| i.checked_sub(2 * (k + 1)).and_then(|j| out.get(j)).copied());
        let clearance = |r: Point| before.iter().flatten().map(|b| b.distance(r)).fold(f64::INFINITY, f64::min);
        // An inset is a share of the whole width, taken off this side's offset.
        let reach = offset.hypot();
        let inward = unit(offset).unwrap_or(Vec2::ZERO);
        let candidates = SHORT_INSETS.iter().map(|inset| q + offset - inward * (inset * width).min(reach));
        let mut best: Option<(Point, f64)> = None;
        for r in candidates {
            let c = clearance(r);
            if c >= pitch {
                best = Some((r, c));
                break;
            }
            if best.is_none_or(|(_, b)| c > b) {
                best = Some((r, c));
            }
        }
        out.push(best.map_or(q + offset, |(r, _)| r));
    }
    out
}

/// `run` added to a column's `runs`, its stitches split to at most `max` ([`split_long`]). A run
/// sewn on from the last (`Travel::Sew`: from the underlay to the next layer) is split together
/// with the stitch that leads into it, which can cross the whole column.
fn push_split(runs: &mut Runs, run: Vec<Point>, travel: Travel, max: f64) {
    match (travel, runs.here()) {
        (Travel::Sew, Some(here)) => {
            let mut joined = Vec::with_capacity(run.len() + 1);
            joined.push(here);
            joined.extend(run);
            runs.push(split_long(&joined, max).into_iter().skip(1).collect(), travel);
        }
        _ => runs.push(split_long(&run, max), travel),
    }
}

/// The spacing of a satin column's penetrations on each rail (peak to peak) for its `width` (auto
/// spacing): more open in a narrow column, where the thread bunches at the rails and covers anyway,
/// denser in a wide one so it covers. A stepped table, as professional software keeps one; the
/// values are estimates (0.40 mm, Madeira's for 40 wt, at 4–6 mm).
pub fn satin_spacing(width: f64) -> f64 {
    SPACING_BY_WIDTH.iter().find(|(w, _)| width < *w).map_or(WIDE_SPACING, |(_, spacing)| *spacing)
}

/// The underlays a satin column takes ([`Underlay`] as a column sews it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SatinUnderlay {
    Center,
    Edge,
    EdgeZigzag,
}

/// The underlay [`Underlay::Auto`] sews under a satin column `width` wide (millimetres): a centre
/// run under [`SATIN_EDGE_UNDERLAY`], an edge run, an edge run and a zigzag from
/// [`SATIN_ZIGZAG_UNDERLAY`]. The planner sews by it, and what Auto chose is reported by it.
pub fn satin_auto_underlay(width: f64) -> Underlay {
    if width < SATIN_EDGE_UNDERLAY {
        Underlay::Center
    } else if width < SATIN_ZIGZAG_UNDERLAY {
        Underlay::Edge
    } else {
        Underlay::EdgeZigzag
    }
}

/// The underlay `u` asks of a satin column `width` wide: by its width with [`Underlay::Auto`]
/// ([`satin_auto_underlay`]); `None` for none.
fn satin_kind(u: Underlay, width: f64) -> Option<SatinUnderlay> {
    match u {
        Underlay::None => None,
        Underlay::Auto => satin_kind(satin_auto_underlay(width), width),
        Underlay::EdgeZigzag | Underlay::EdgeTatami => Some(SatinUnderlay::EdgeZigzag),
        Underlay::Center => Some(SatinUnderlay::Center),
        Underlay::Edge => Some(SatinUnderlay::Edge),
    }
}

/// The underlay of a satin column along `pts` (its centreline), `width` wide, of `kind`: a centre
/// run, an edge run (along one side and back along the other), or an edge run then a zigzag
/// between its sides. Each run comes with whether it
/// ends at the column's far end. The thresholds are estimates from the usual practice. (The
/// zigzag's stitches run at an angle across the column, longer than its width, and the edge run
/// crosses the column at its far end: the caller splits them like the top stitching, as
/// Ink/Stitch's underlay guide recommends; an unbounded zigzag destabilises the column.)
fn satin_underlay(pts: &[Point], closed: bool, width: f64, kind: SatinUnderlay) -> Vec<(Vec<Point>, bool)> {
    let centre = resample(pts, UNDERLAY_STITCH, closed);
    if kind == SatinUnderlay::Center {
        return vec![(centre, !closed)];
    }
    // The sides, kept inside the column so the underlay doesn't show at its edges.
    let side = (width / 2.0 - (width * UNDERLAY_INSET).min(UNDERLAY_INSET_MAX)).max(0.0);
    let offset = |samples: &[Point], i: usize, d: f64| -> Option<Point> {
        let q = *samples.get(i)?;
        let t = tangent(samples, i, closed);
        Some(q + Vec2::new(-t.y, t.x) * d)
    };
    let mut edge: Vec<Point> = (0..centre.len()).filter_map(|i| offset(&centre, i, side)).collect();
    edge.extend((0..centre.len()).rev().filter_map(|i| offset(&centre, i, -side)));
    let mut out = vec![(edge, false)];
    if kind == SatinUnderlay::EdgeZigzag {
        let steps = resample(pts, ZIGZAG_UNDERLAY_SPACING, closed);
        let zigzag: Vec<Point> = (0..steps.len()).filter_map(|i| offset(&steps, i, if i % 2 == 0 { side } else { -side })).collect();
        out.push((zigzag, !closed));
    }
    out
}

/// `zigzag`'s crossings longer than `max` split into shorter stitches (auto split). A long crossing
/// is cut into `n` parts: the cuts sit `1/n` of it apart, all shifted by a fraction (at most
/// `JITTER` of a part either way) that changes from one crossing to the next (the golden ratio's
/// sequence), so the splits of neighbouring crossings never line up into a seam down the column.
/// The inner parts are `1/n` long and the two end parts at most `(1 + JITTER)/n`, so `n` is chosen
/// to keep that under `max`.
fn split_long(zigzag: &[Point], max: f64) -> Vec<Point> {
    const JITTER: f64 = SPLIT_JITTER;
    let mut out: Vec<Point> = Vec::with_capacity(zigzag.len());
    let mut crossing = 0u32;
    for (i, &q) in zigzag.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).and_then(|j| zigzag.get(j)) {
            let len = p.distance(q);
            if len.is_finite() && len > max {
                // The longest part, an end one, is (1 + JITTER)/n of the crossing: enough parts that
                // it fits.
                let n = ((len * (1.0 + JITTER)) / max).ceil().clamp(2.0, MAX_SPLIT) as u32;
                crossing += 1;
                // A fraction from 0 to 1, centred on 0 (-1 to 1), times the jitter.
                let shift = ((f64::from(crossing) * GOLDEN).fract() - 0.5) * 2.0 * JITTER;
                out.extend((1..n).map(|k| p.lerp(q, (f64::from(k) + shift) / f64::from(n))));
            }
        }
        out.push(q);
    }
    out
}

/// A column narrower than this (its typical width) is sewn as a running line along its middle, as
/// the export sews strokes that narrow as running lines ([`super::SATIN_MIN_WIDTH`]).
pub(super) const MIN_COLUMN_WIDTH: f64 = super::SATIN_MIN_WIDTH;
/// The most a column's farther rail is taken to run for each millimetre of its middle when its
/// crossings are spaced along that rail ([`Along::crossings`], [`Centre::stretch`]). Round a bend
/// whose middle has radius r, a rail R out runs (r + R) / r as far; the inner side's radius can't
/// fall below nothing, so R is at most r and the outer rail runs at most twice as far (a little
/// more with pull compensation). Where the rails run farther still they are turning across the
/// column, round a round end's tip, or a stroke is drawn round a tighter radius than half its width
/// and its inner rail folds over: the crossings there come no closer along the middle than half
/// their pitch.
pub(super) const MAX_RAIL_STRETCH: f64 = 2.0;

/// `column` (a narrow part of a fill, [`Column`]) sewn after `runs` as a satin column whose width
/// follows the shape, from its end nearest the needle (a ring from its station nearest):
/// - an underlay by its typical width, as a stroke's;
/// - crossings `p`'s spacing apart, picked by each crossing's own width with auto spacing (and
///   the thread's weight), so the spacing follows the column as it narrows or widens; round a
///   bend the spacing is kept on the outer rail, and the inner one, shorter, takes its
///   penetrations closer (shortened where they bunch);
/// - pull compensation at both rails, the mean of `p`'s two sides (a fill has no drawn direction
///   to tell its sides apart), and push compensation, the mean of its two values, at each end
///   where the shape ends ([`Column::tips`]): where the column meets the rest of the shape, which
///   is cut out round its whole length, a shorter column would leave a band sewn by neither.
///
/// `reach` says how the needle gets from where it is to the column's first stitch (given that
/// stitch and where it would be without pull compensation, on the shape's outline). A column
/// narrower than [`MIN_COLUMN_WIDTH`] is a running line along its middle.
///
/// Sewn last `toward` an exit point, it ends at its end nearest the exit, as a stroke's column does
/// (a pass along its middle under its top stitching where its underlay would end it at the other
/// end; a running line from its end farther from the exit), and a ring starts at its station
/// nearest the exit.
pub(super) fn sew_column(
    column: &Column,
    p: &Params,
    runs: &mut Runs,
    reach: &mut dyn FnMut(&mut Runs, Point, Point) -> Travel,
    toward: Option<Point>,
) {
    let closed = column.closed;
    let mut stations = column.stations.clone();
    let mut tips = column.tips;
    if stations.len() < 2 {
        return;
    }
    // Start nearest the needle (a ring sewn toward an exit: nearest the exit).
    if let (true, Some(exit)) = (closed, toward) {
        let from = |s: Option<&(Point, Point)>| s.map_or(f64::INFINITY, |(a, b)| a.midpoint(*b).distance(exit));
        let nearest = (0..stations.len()).min_by(|&i, &j| from(stations.get(i)).total_cmp(&from(stations.get(j))));
        stations.rotate_left(nearest.unwrap_or(0));
    } else if let Some(here) = runs.here() {
        let from = |s: Option<&(Point, Point)>| s.map_or(f64::INFINITY, |(a, b)| a.midpoint(*b).distance(here));
        if closed {
            let nearest = (0..stations.len()).min_by(|&i, &j| from(stations.get(i)).total_cmp(&from(stations.get(j))));
            stations.rotate_left(nearest.unwrap_or(0));
        } else if from(stations.last()) < from(stations.first()) {
            stations.reverse();
            tips.reverse();
        }
    }
    let width = Column { stations: stations.clone(), closed, ..Column::default() }.width();
    let along = Along::new(&stations, closed);
    if width < MIN_COLUMN_WIDTH {
        let mut line = resample(&along.middles, p.stitch_length, closed);
        // Toward an exit, an open line from its end farther from it.
        if let (false, Some(exit), Some(&a), Some(&b)) = (closed, toward, line.first(), line.last())
            && a.distance(exit) < b.distance(exit)
        {
            line.reverse();
        }
        if line.len() >= 2
            && let Some(&first) = line.first()
        {
            let travel = reach(runs, first, first);
            runs.push(line, travel);
        }
        return;
    }
    // Push compensation: an open column is sewn shorter at each end where the shape ends, at most
    // to MIN_PUSHED of it.
    let along = if closed {
        along
    } else {
        let push = ((p.push[0] + p.push[1]) / 2.0).max(0.0);
        let [start, end] = tips.map(|tip| if tip { push } else { 0.0 });
        // At most down to MIN_PUSHED of it, and never shorter than a column that is sewn.
        let k = push_share(along.len, start, end, p.column_spacing(width) * MIN_CROSSINGS);
        Along::new(&along.between(start * k, along.len - end * k), false)
    };
    if along.len < p.column_spacing(width) * MIN_CROSSINGS {
        return;
    }
    let mut first = true;
    let mut back = false;
    if let Some(kind) = satin_kind(p.underlay, width) {
        for (under, ends_far) in column_underlay(&along, kind) {
            if under.len() >= 2
                && let Some(&start) = under.first()
            {
                let travel = if first { reach(runs, start, start) } else { Travel::Sew };
                push_split(runs, under, travel, p.max_satin);
                first = false;
            }
            back = ends_far;
        }
    }
    // Toward an exit: where the top stitching would end at the end farther from it, a pass along the
    // middle turns it round.
    if let (false, Some(exit), Some(&a), Some(&b)) = (closed, toward, along.middles.first(), along.middles.last()) {
        let ends_at = if back { a } else { b };
        let nearer = match a.distance(exit).total_cmp(&b.distance(exit)) {
            std::cmp::Ordering::Less => Some(a),
            std::cmp::Ordering::Greater => Some(b),
            std::cmp::Ordering::Equal => None,
        };
        if nearer.is_some_and(|q| q != ends_at) {
            let mut middle = along.middles.clone();
            if back {
                middle.reverse();
            }
            let pass = resample(&middle, TRAVEL_STITCH, false);
            if let Some(&start) = pass.first() {
                let travel = if first { reach(runs, start, start) } else { Travel::Sew };
                runs.push(pass, travel);
                first = false;
                back = !back;
            }
        }
    }
    let (mut crossings, mut inner, spacing, reach_width) = along.crossings(p);
    if back && !closed {
        crossings.reverse();
        inner.reverse();
    }
    let mut zigzag = short_stitches(&crossings, spacing, reach_width);
    // A ring ends where it started: the last crossing takes the remaining side.
    if closed
        && let (Some(a), Some(b)) = (zigzag.first().copied(), zigzag.last().copied())
        && a.distance(b) > p.min_stitch
    {
        zigzag.push(a);
    }
    let (Some(&start), Some(&start_inside)) = (zigzag.first(), inner.first()) else { return };
    // The column's last stitch reaches past the outline by the pull compensation: the needle's
    // next way inside the shape starts where it would be without it.
    let end_inside = if closed { inner.first() } else { inner.last() }.copied();
    let travel = if first { reach(runs, start, start_inside) } else { Travel::Sew };
    push_split(runs, zigzag, travel, p.max_satin);
    if let Some(end) = end_inside {
        runs.stretched_to(end);
    }
}

/// Whether sewing `column` with `p` ([`sew_column`]) ends at the end it starts from: its underlay
/// (a centre run, or a zigzag after the edge run) goes to the far end and the top stitching comes
/// back. A ring ends where it starts anyway; a running line, a column with an edge run only or
/// with no underlay end at the far end.
pub(super) fn column_returns(column: &Column, p: &Params) -> bool {
    if column.closed {
        return true;
    }
    let width = column.width();
    // A centre run or an edge run then a zigzag ends at the far end, and the top stitching comes
    // back; an edge run alone ends where it began, and the top goes on.
    width >= MIN_COLUMN_WIDTH && matches!(satin_kind(p.underlay, width), Some(SatinUnderlay::Center | SatinUnderlay::EdgeZigzag))
}

/// A column measured along its middle: each station's middle, the half crossing from it to the
/// first side, the distance along to each, and the length (round to the start for a ring).
pub(super) struct Along {
    middles: Vec<Point>,
    halves: Vec<Vec2>,
    pub(super) at: Vec<f64>,
    len: f64,
    closed: bool,
}

impl Along {
    pub(super) fn new(stations: &[(Point, Point)], closed: bool) -> Self {
        let middles: Vec<Point> = stations.iter().map(|(a, b)| a.midpoint(*b)).collect();
        let halves: Vec<Vec2> = stations.iter().map(|(a, b)| (*a - *b) / 2.0).collect();
        let mut at = Vec::with_capacity(middles.len());
        let mut len = 0.0;
        for (k, m) in middles.iter().enumerate() {
            if let Some(prev) = k.checked_sub(1).and_then(|j| middles.get(j)) {
                let d = prev.distance(*m);
                len += if d.is_finite() { d } else { 0.0 };
            }
            at.push(len);
        }
        if closed && let (Some(a), Some(b)) = (middles.first(), middles.last()) {
            let d = a.distance(*b);
            len += if d.is_finite() { d } else { 0.0 };
        }
        Self { middles, halves, at, len, closed }
    }

    /// The middle and the half crossing `s` along, between the stations either side.
    fn station(&self, s: f64) -> Option<(Point, Vec2)> {
        let n = self.middles.len();
        let s = if self.closed { s.rem_euclid(self.len.max(NO_LENGTH)) } else { s.clamp(0.0, self.len) };
        let i = self.at.partition_point(|&a| a <= s).saturating_sub(1);
        let j = if i + 1 < n {
            i + 1
        } else if self.closed {
            0
        } else {
            i
        };
        let (m0, m1, h0, h1) = (*self.middles.get(i)?, *self.middles.get(j)?, *self.halves.get(i)?, *self.halves.get(j)?);
        let a0 = *self.at.get(i)?;
        let a1 = if j > i { *self.at.get(j)? } else { self.len };
        let f = if a1 - a0 > NO_LENGTH { ((s - a0) / (a1 - a0)).clamp(0.0, 1.0) } else { 0.0 };
        Some((m0.lerp(m1, f), h0 + (h1 - h0) * f))
    }

    /// The stations from `s0` to `s1` along: one at each, and the column's own between.
    fn between(&self, s0: f64, s1: f64) -> Vec<(Point, Point)> {
        let at = |s: f64| self.station(s).map(|(m, h)| (m + h, m - h));
        let mut out: Vec<(Point, Point)> = at(s0).into_iter().collect();
        for (k, &a) in self.at.iter().enumerate() {
            if a > s0
                && a < s1
                && let (Some(&m), Some(&h)) = (self.middles.get(k), self.halves.get(k))
            {
                out.push((m + h, m - h));
            }
        }
        out.extend(at(s1));
        out
    }

    /// The column's crossings, in order: each a point on its middle and the offset to the side it
    /// goes to, the sides alternating, widened by pull compensation; where each would reach
    /// without the compensation (on the outline); and the typical distance from one to the next
    /// (on the farther rail) and the typical width they reach across (for short stitches). Each
    /// crossing's spacing to the next is picked by its own width (auto spacing), or is `p`'s
    /// density, and is kept on the farther rail: round a bend the outer one, as Embrilliance holds
    /// a satin's density on the outside of a turn and shortens stitches on the inside
    /// (<https://embrilliance.com/Help/Platform%20Mac%201170/satincolumninputlevel2.htm>). Along a
    /// straight column the rails run as far as the middle and the crossings lie that far apart
    /// along it. An open column's last crossing lies on its end.
    fn crossings(&self, p: &Params) -> (Vec<(Point, Vec2)>, Vec<Point>, f64, f64) {
        // The distance between crossings at a width: half the spacing on a rail (peak to peak).
        let pitch_at = |width: f64| p.column_spacing(width) / CROSSINGS_PER_SPACING;
        // How far a crossing reaches either side of the middle, half its width `half` widened by
        // pull compensation.
        let reach_at = |half: f64| {
            let [left, right] = p.compensation(2.0 * half);
            half + (left + right) / 2.0
        };
        // Where a crossing `s` along goes into the fabric on each side: the column's two rails.
        let rails = |s: f64| -> Option<(Point, Point)> {
            let (m, h) = self.station(s)?;
            let out = unit(h).unwrap_or(Vec2::ZERO) * reach_at(h.hypot());
            Some((m + out, m - out))
        };
        // How far the farther rail runs for each millimetre of the middle from `s` over the next
        // `ds`: one along a straight column, (r + R) / r round a bend whose middle has radius r
        // with the outer rail R out.
        let stretch = |s: f64, ds: f64| -> f64 {
            let end = if self.closed { s + ds } else { (s + ds).min(self.len) };
            let (Some((a0, b0)), Some((a1, b1))) = (rails(s), rails(end)) else { return 1.0 };
            let farther = a0.distance(a1).max(b0.distance(b1)) / (end - s);
            if farther.is_finite() { farther.clamp(1.0, MAX_RAIL_STRETCH) } else { 1.0 }
        };
        // Where the crossings go along the middle.
        let mut at: Vec<f64> = Vec::new();
        let mut pitches: Vec<f64> = Vec::new();
        let mut s = 0.0;
        while at.len() < MAX_STITCHES {
            at.push(s);
            let Some((_, h)) = self.station(s) else { break };
            let pitch = pitch_at(2.0 * h.hypot());
            pitches.push(pitch);
            // The step along the middle that moves the farther rail on by the pitch.
            let step = pitch / stretch(s, pitch);
            let next = s + step;
            if self.closed {
                // Round a ring, the last crossing leaves at least half a step to the first.
                if next > self.len - step / 2.0 {
                    break;
                }
            } else if next >= self.len {
                // An open column ends on a crossing: the last one moves there when the step to the
                // end would be short, else one is added.
                if self.len - s < step / 2.0 && at.len() > 1 {
                    at.pop();
                }
                at.push(self.len);
                break;
            }
            s = next;
        }
        let mut widths: Vec<f64> = Vec::with_capacity(at.len());
        let mut inner: Vec<Point> = Vec::with_capacity(at.len());
        let crossings: Vec<(Point, Vec2)> = at
            .iter()
            .enumerate()
            .filter_map(|(k, &s)| {
                let (m, h) = self.station(s)?;
                let reach = reach_at(h.hypot());
                widths.push(2.0 * reach);
                let side = if k % 2 == 0 { 1.0 } else { -1.0 };
                inner.push(m + h * side);
                Some((m, unit(h).unwrap_or(Vec2::ZERO) * reach * side))
            })
            .collect();
        let typical = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(0.0)
        };
        (crossings, inner, typical(pitches), typical(widths))
    }
}

/// The underlay of a column measured `along` its middle, of `kind` (chosen by its typical width
/// unless asked for), as a stroke's ([`satin_underlay`]): a centre run, an edge run (along one side
/// and back along the other, kept inside), or an edge run then a zigzag; each with whether it ends
/// at the column's far end.
fn column_underlay(along: &Along, kind: SatinUnderlay) -> Vec<(Vec<Point>, bool)> {
    let closed = along.closed;
    let centre = resample(&along.middles, UNDERLAY_STITCH, closed);
    if kind == SatinUnderlay::Center {
        return vec![(centre, !closed)];
    }
    // A point on one side (`sign`), kept inside the column by the inset.
    let inside = |m: Point, h: Vec2, sign: f64| {
        let half = h.hypot();
        let inset = (2.0 * half * UNDERLAY_INSET).min(UNDERLAY_INSET_MAX);
        m + unit(h).unwrap_or(Vec2::ZERO) * (half - inset).max(0.0) * sign
    };
    let side = |sign: f64| -> Vec<Point> { along.middles.iter().zip(&along.halves).map(|(m, h)| inside(*m, *h, sign)).collect() };
    let mut edge = resample(&side(1.0), UNDERLAY_STITCH, closed);
    let mut other = resample(&side(-1.0), UNDERLAY_STITCH, closed);
    other.reverse();
    edge.extend(other);
    let mut out = vec![(edge, false)];
    if kind == SatinUnderlay::EdgeZigzag {
        let steps = (along.len / ZIGZAG_UNDERLAY_SPACING).ceil().clamp(1.0, MAX_STITCHES as f64) as usize;
        let zigzag: Vec<Point> = (0..=steps)
            .filter_map(|k| {
                let (m, h) = along.station(along.len * k as f64 / steps as f64)?;
                Some(inside(m, h, if k % 2 == 0 { 1.0 } else { -1.0 }))
            })
            .collect();
        out.push((zigzag, !closed));
    }
    out
}
