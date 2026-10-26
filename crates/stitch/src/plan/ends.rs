//! Where a shape can start and end ([`ends`]), by the planner's own rules, for choosing its entry
//! and exit points with the shapes sewn before and after it in view (`plan::joins`).
//!
//! - **An open running line** starts at either end and ends at the other: never sewn back over
//!   itself.
//! - **An open satin column** starts at either end and ends at the end its underlay leaves it at (the
//!   same end where the top stitching comes back over the underlay, the other where it doesn't), or,
//!   for a pass along its middle under its top stitching, at the other.
//! - **A closed running line or a satin ring** starts at any point of it and ends there, or goes on
//!   round, the shorter way, to any other.
//! - **A fill** is entered anywhere on its outline and ends at any row end (where the outline doesn't
//!   run along the rows), its rows travelling hidden from where it is entered to where they must
//!   start to end there.
//!
//! The points are spread evenly by length along the shape (about [`MAX_JOIN_STARTS`] starts and
//! [`MAX_JOIN_EXITS`] ends a shape: a fill's every outline at least one, and a shape's own start and
//! end besides, [`Ends::own_first`]), in the order the shape is drawn, so the same shape gives the
//! same points. A line of several subpaths and a fill whose narrow parts are sewn as satin have no
//! ends here: their order is the planner's search, which these points don't model.

use vectorcraft_geom::{BezPath, Point, Vec2};

use super::geometry::{Axes, SubPath, length, subpaths};
use super::runs::COST_HIDDEN;
use super::{Params, Style, sewn_style};
use crate::SAME_POINT;

/// The most starts a closed line, a ring or a fill offers, spread evenly by length along it: on a
/// 100 mm outline, one about every 3 mm, about a stitch (an estimate).
pub const MAX_JOIN_STARTS: usize = 32;

/// The most ends a fill offers, spread evenly by length along its outline where its rows end (an
/// estimate: half its starts, as a fill is cut at the row nearest its exit anyway).
pub const MAX_JOIN_EXITS: usize = 16;

/// The least angle between a fill's outline and its rows where the rows end on it, degrees: along a
/// side within this of the rows' direction there are no row ends, and an exit there ends at the
/// side's nearer corner (an estimate).
const ROW_END_MIN_ANGLE: f64 = 15.0;

/// Where a shape can start and end, millimetres, and what each way through it costs, in the
/// planner's units ([`super::runs`]'s weights).
#[derive(Clone, Debug, PartialEq)]
pub struct Ends {
    /// The points it can start at: each the entry point to set, and about where its first needle
    /// point then lies.
    pub starts: Vec<Point>,
    /// The points it can end at.
    pub ends: Vec<Point>,
    /// Which ends each start can reach, and at what cost.
    pub through: Through,
}

/// Which of a shape's ends each of its starts can reach ([`Ends`]), and at what cost.
#[derive(Clone, Debug, PartialEq)]
pub enum Through {
    /// For each start, each end's cost, `None` where that start can't end there: an open line or
    /// column, two by two.
    Table(Vec<Vec<Option<f64>>>),
    /// Any end from any start, round a closed line `length` long (`line`, its first point repeated
    /// at its end), at `per_mm` a millimetre of the shorter way round from the start to the end;
    /// `starts` and `ends` are how far along the line each lies.
    Round { line: Vec<Point>, length: f64, starts: Vec<f64>, ends: Vec<f64>, per_mm: f64 },
    /// Any end from any start, at no cost of its own: a fill's rows travel hidden from where it is
    /// entered to where they must start to end there.
    Free,
}

impl Ends {
    /// The cost of starting at start `s` and ending at end `e`, `None` where it can't.
    pub fn cost(&self, s: usize, e: usize) -> Option<f64> {
        if s >= self.starts.len() || e >= self.ends.len() {
            return None;
        }
        match &self.through {
            Through::Table(rows) => rows.get(s).and_then(|r| r.get(e).copied().flatten()),
            Through::Round { length, starts, ends, per_mm, .. } => {
                let (a, b) = (starts.get(s)?, ends.get(e)?);
                let way = (a - b).abs();
                Some(per_mm * way.min(length - way).max(0.0))
            }
            Through::Free => Some(0.0),
        }
    }

    /// One start and one end, at no cost: a shape sewn as it is (where it starts and ends now).
    pub fn fixed(start: Point, end: Point) -> Self {
        Self { starts: vec![start], ends: vec![end], through: Through::Table(vec![vec![Some(0.0)]]) }
    }

    /// Each start with only the ends it reaches at no cost: how the shape is sewn with no exit set
    /// (an open line at its other end, a column where its underlay leaves it, a closed line where it
    /// starts; a fill anywhere).
    pub fn natural(&self) -> Self {
        let table = (0..self.starts.len()).map(|s| (0..self.ends.len()).map(|e| self.cost(s, e).filter(|c| *c == 0.0)).collect()).collect();
        Self { starts: self.starts.clone(), ends: self.ends.clone(), through: Through::Table(table) }
    }

    /// A shape's own start and end (where it starts and ends now) put first, as its first choices,
    /// which ties keep. A fill takes them as they are, before its other points, and so does a closed
    /// line or a ring, each at its place along the line (so that a shape left as it is keeps its
    /// stitches); an open line or column puts first the end nearest each.
    pub fn own_first(&mut self, first: Point, last: Point) {
        if self.through == Through::Free {
            self.starts.insert(0, first);
            self.ends.insert(0, last);
            return;
        }
        if let Through::Round { line, starts, ends, .. } = &mut self.through {
            let (Some(a), Some(b)) = (along(line, first), along(line, last)) else { return };
            self.starts.insert(0, first);
            starts.insert(0, a);
            self.ends.insert(0, last);
            ends.insert(0, b);
            return;
        }
        let nearest = |pts: &[Point], q: Point| pts.iter().enumerate().min_by(|a, b| a.1.distance(q).total_cmp(&b.1.distance(q))).map(|(i, _)| i);
        let (Some(s), Some(e)) = (nearest(&self.starts, first), nearest(&self.ends, last)) else { return };
        self.starts.swap(0, s);
        self.ends.swap(0, e);
        match &mut self.through {
            Through::Table(rows) => {
                rows.swap(0, s);
                rows.iter_mut().for_each(|r| r.swap(0, e));
            }
            Through::Round { starts, ends, .. } => {
                starts.swap(0, s);
                ends.swap(0, e);
            }
            Through::Free => {}
        }
    }

    /// At most `n` starts and `n` ends (at least the first of each), the first kept and the rest
    /// picked evenly: a shape offering fewer points, where a chain is too long to weigh them all
    /// (`plan::thinned_to`). An open line or column (two by two) keeps its own.
    pub fn thinned(&self, n: usize) -> Self {
        let keep = |len: usize| -> Vec<usize> {
            let n = n.max(1);
            if len <= n {
                return (0..len).collect();
            }
            (0..n).map(|k| k * len / n).collect()
        };
        let (s, e) = (keep(self.starts.len()), keep(self.ends.len()));
        let pick = |pts: &[Point], idx: &[usize]| idx.iter().filter_map(|&i| pts.get(i).copied()).collect::<Vec<_>>();
        let through = match &self.through {
            Through::Table(_) => return self.clone(),
            Through::Round { line, length, starts, ends, per_mm } => Through::Round {
                line: line.clone(),
                length: *length,
                starts: s.iter().filter_map(|&i| starts.get(i).copied()).collect(),
                ends: e.iter().filter_map(|&i| ends.get(i).copied()).collect(),
                per_mm: *per_mm,
            },
            Through::Free => Through::Free,
        };
        Self { starts: pick(&self.starts, &s), ends: pick(&self.ends, &e), through }
    }
}

/// Where `path` (millimetres), sewn with `p` (a fill with its narrow parts as satin columns when
/// `columns`), can start and end; `None` where these rules don't say (nothing to sew, a line of
/// several subpaths, a fill with narrow parts sewn as satin). A fill's outlines are all outlines
/// whatever its fill rule: it can be entered on a hole's edge too.
pub fn ends(path: &BezPath, p: &Params, columns: bool) -> Option<Ends> {
    let p = p.sane();
    let subs: Vec<SubPath> = subpaths(path).into_iter().filter(|s| s.pts.len() >= 2).collect();
    match sewn_style(&p) {
        Style::Running | Style::Satin => {
            let [sub] = &subs[..] else { return None };
            let pts = sub.points();
            if sub.closed {
                return round(&pts, MAX_JOIN_STARTS);
            }
            let (&a, &b) = (pts.first()?, pts.last()?);
            if a.distance(b) <= SAME_POINT {
                return None;
            }
            let table = if sewn_style(&p) == Style::Running {
                vec![vec![None, Some(0.0)], vec![Some(0.0), None]]
            } else {
                // Where its underlay leaves it, and the other end at a pass along its middle.
                let pass = Some(COST_HIDDEN * length(&pts, false));
                let (home, away) = if super::satin::line_returns(&p) { (Some(0.0), pass) } else { (pass, Some(0.0)) };
                vec![vec![home, away], vec![away, home]]
            };
            Some(Ends { starts: vec![a, b], ends: vec![a, b], through: Through::Table(table) })
        }
        Style::Fill if columns => None,
        Style::Fill => fill(&subs, &p),
    }
}

/// A closed line's (or ring's) starts and ends: `n` points spread evenly along `pts` (its first
/// point repeated at its end), each a start and an end.
fn round(pts: &[Point], n: usize) -> Option<Ends> {
    let total = length(pts, false);
    if total <= SAME_POINT || !total.is_finite() {
        return None;
    }
    let spread = spread(pts, n);
    let at: Vec<Point> = spread.iter().map(|(q, _)| *q).collect();
    let along: Vec<f64> = spread.iter().map(|(_, s)| *s).collect();
    let line = pts.to_vec();
    Some(Ends {
        starts: at.clone(),
        ends: at,
        through: Through::Round { line, length: total, starts: along.clone(), ends: along, per_mm: COST_HIDDEN },
    })
}

/// How far along polyline `pts` its point nearest `q` lies, `None` for no polyline or a `q` that
/// isn't a number.
fn along(pts: &[Point], q: Point) -> Option<f64> {
    if !(q.x.is_finite() && q.y.is_finite()) {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    let mut walked = 0.0;
    for w in pts.windows(2) {
        let [a, b] = w else { continue };
        let ab = *b - *a;
        let len2 = ab.hypot2();
        let t = if len2 > 0.0 { ((q - *a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (*a + ab * t).distance(q);
        let seg = len2.sqrt();
        if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, walked + seg * t));
        }
        walked += if seg.is_finite() { seg } else { 0.0 };
    }
    best.map(|(_, s)| s)
}

/// A fill's starts (points spread along all its outlines) and ends (those of them where its rows
/// end on the outline).
fn fill(subs: &[SubPath], p: &Params) -> Option<Ends> {
    let outlines: Vec<Vec<Point>> = subs
        .iter()
        .map(|s| {
            let mut pts = s.pts.clone();
            if let Some(&a) = pts.first() {
                pts.push(a);
            }
            pts
        })
        .collect();
    let lengths: Vec<f64> = outlines.iter().map(|o| length(o, false)).collect();
    let total: f64 = lengths.iter().sum();
    if total <= SAME_POINT || !total.is_finite() {
        return None;
    }
    // Each outline its share of the points, by its length; at least one each.
    let share = |len: f64, n: usize| ((len / total * n as f64).round() as usize).max(1);
    let starts: Vec<Point> =
        outlines.iter().zip(&lengths).flat_map(|(o, &len)| spread(o, share(len, MAX_JOIN_STARTS)).into_iter().map(|(q, _)| q)).collect();
    let rows = Axes::new(p.angle).along;
    let min_sin = (ROW_END_MIN_ANGLE.to_radians()).sin();
    let ends: Vec<Point> = outlines
        .iter()
        .zip(&lengths)
        .flat_map(|(o, &len)| spread_with_direction(o, share(len, MAX_JOIN_EXITS)))
        .filter(|(_, dir)| dir.cross(rows).abs() >= min_sin)
        .map(|(q, _)| q)
        .collect();
    if starts.is_empty() || ends.is_empty() {
        return None;
    }
    Some(Ends { starts, ends, through: Through::Free })
}

/// `n` points spread evenly by length along the polyline `pts` (a closed one with its first point
/// repeated at its end, so that the last point isn't the first again), from its first point, each
/// with how far along it lies.
fn spread(pts: &[Point], n: usize) -> Vec<(Point, f64)> {
    spread_on(pts, n).into_iter().map(|(q, s, _)| (q, s)).collect()
}

/// [`spread`]'s points with the direction of the polyline there (a unit vector).
fn spread_with_direction(pts: &[Point], n: usize) -> Vec<(Point, Vec2)> {
    spread_on(pts, n).into_iter().map(|(q, _, d)| (q, d)).collect()
}

/// `n` points evenly by length along `pts`: each point, how far along it lies, and the polyline's
/// direction there.
fn spread_on(pts: &[Point], n: usize) -> Vec<(Point, f64, Vec2)> {
    let total = length(pts, false);
    if n == 0 || total <= SAME_POINT || !total.is_finite() {
        return Vec::new();
    }
    let step = total / n as f64;
    let mut out = Vec::with_capacity(n);
    let (mut walked, mut k) = (0.0, 0usize);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = a.distance(b);
        if seg.is_nan() || seg <= SAME_POINT {
            continue;
        }
        let dir = (b - a) / seg;
        while k < n {
            let target = step * k as f64;
            if target > walked + seg {
                break;
            }
            out.push((a + dir * (target - walked), target, dir));
            k += 1;
        }
        walked += seg;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(pts: &[(f64, f64)], closed: bool) -> BezPath {
        let mut p = BezPath::new();
        for (i, &q) in pts.iter().enumerate() {
            if i == 0 {
                p.move_to(q);
            } else {
                p.line_to(q);
            }
        }
        if closed {
            p.close_path();
        }
        p
    }

    /// An open running line starts at either end and ends at the other; a satin column whose top
    /// stitching comes back over its underlay ends where it starts, or at its other end for a pass.
    #[test]
    fn an_open_line_or_column_ends_by_its_style() {
        let open = line(&[(0.0, 0.0), (30.0, 0.0)], false);
        let running = ends(&open, &Params::for_style(Style::Running), false).unwrap();
        assert_eq!((running.cost(0, 0), running.cost(0, 1), running.cost(1, 0)), (None, Some(0.0), Some(0.0)));
        // 1.5 mm wide: a centre run under it, which the top stitching comes back over.
        let satin = ends(&open, &Params { width: 1.5, ..Params::for_style(Style::Satin) }, false).unwrap();
        assert_eq!(satin.cost(0, 0), Some(0.0));
        assert!(satin.cost(0, 1).unwrap() > 0.0, "a pass along its middle");
    }

    /// A closed line offers points spread along it, each reachable from each, the shorter way round.
    #[test]
    fn a_closed_line_ends_anywhere_round_it() {
        let square = line(&[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)], true);
        let e = ends(&square, &Params::for_style(Style::Running), false).unwrap();
        assert_eq!(e.starts.len(), MAX_JOIN_STARTS);
        assert_eq!(e.cost(0, 0), Some(0.0));
        // Half way round, 80 mm either way.
        let half = MAX_JOIN_STARTS / 2;
        assert!((e.cost(0, half).unwrap() - COST_HIDDEN * 80.0).abs() < 1e-9);
        assert!((e.cost(0, MAX_JOIN_STARTS - 1).unwrap() - COST_HIDDEN * 5.0).abs() < 1e-9, "the shorter way");
    }

    /// A fill is entered anywhere on its outline and ends only where its rows end on it: on a
    /// rectangle with level rows, its left and right sides.
    #[test]
    fn a_fill_ends_where_its_rows_end() {
        let bar = line(&[(0.0, 0.0), (60.0, 0.0), (60.0, 20.0), (0.0, 20.0)], true);
        let e = ends(&bar, &Params { angle: 0.0, ..Params::for_style(Style::Fill) }, false).unwrap();
        assert!(e.starts.iter().any(|q| q.y.abs() < 1e-9 && q.x > 10.0 && q.x < 50.0), "on its long sides");
        assert!(!e.ends.is_empty() && e.ends.iter().all(|q| q.x.abs() < 1e-9 || (q.x - 60.0).abs() < 1e-9), "{:?}", e.ends);
        assert_eq!(e.cost(0, 0), Some(0.0));
        assert!(ends(&bar, &Params::for_style(Style::Fill), true).is_none(), "narrow parts as satin");
    }

    fn needle(v: &[crate::Stitch]) -> Vec<Point> {
        v.iter().filter(|s| s.command == crate::Command::Stitch).map(|s| s.point()).collect()
    }

    /// Planned from each of its starts toward each end it can reach, a shape starts and ends where
    /// its ends say: an open running line and a closed one exactly (a closed one's end at its needle
    /// point nearest, within half a stitch), a satin within its half width and push of its middle
    /// line, a fill within half a row spacing of its outline where it starts (its inset outline's
    /// mitre at a corner) and within a row spacing and a half of the row end where it ends.
    #[test]
    fn a_shapes_ends_are_the_ends_it_is_planned_with() {
        let open = line(&[(0.0, 0.0), (30.0, 5.0), (60.0, 0.0)], false);
        let square = line(&[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)], true);
        let bar = line(&[(0.0, 0.0), (60.0, 0.0), (60.0, 20.0), (0.0, 20.0)], true);
        let running = Params::for_style(Style::Running).sane();
        let satin = |underlay| Params { width: 3.0, underlay, ..Params::for_style(Style::Satin) }.sane();
        let fill = Params { angle: 0.0, ..Params::for_style(Style::Fill) }.sane();
        let cases: Vec<(&str, &BezPath, Params, f64, f64)> = vec![
            ("open running line", &open, running, 1e-9, 1e-9),
            ("closed running line", &square, running, 1e-9, running.stitch_length / 2.0 + 1e-9),
            ("satin, centre run", &open, satin(super::super::Underlay::Center), 2.0, 2.0),
            ("satin, edge run", &open, satin(super::super::Underlay::Edge), 2.0, 2.0),
            ("satin, none", &open, satin(super::super::Underlay::None), 2.0, 2.0),
            // At a corner the inset outline's mitre lies √2 times the inset away.
            ("fill", &bar, fill, fill.density / 2.0 * std::f64::consts::SQRT_2 * 1.01, 1.5 * fill.density + 1e-6),
        ];
        for (name, shape, p, near_start, near_end) in cases {
            let e = ends(shape, &p, false).unwrap();
            for (s, &start) in e.starts.iter().enumerate() {
                for (k, &end) in e.ends.iter().enumerate() {
                    if e.cost(s, k).is_none() {
                        continue;
                    }
                    let aim = super::super::Aim { start: super::super::Start::Entry(start), exit: Some(end) };
                    let sew = |aim| super::super::stitches_aimed(shape, vectorcraft_geom::FillRule::NonZero, &p, aim);
                    let (aimed, plain) = (sew(aim), sew(super::super::Aim { exit: None, ..aim }));
                    let out = needle(&aimed);
                    let (first, last) = (out.first().copied().unwrap(), out.last().copied().unwrap());
                    assert!(first.distance(start) <= near_start, "{name}, start {start:?}: first needle point {first:?}");
                    // Where reaching the end would show travel over finished stitching or lift the
                    // needle more, or gain under the miss the Check tab lets pass, the planner's
                    // guard keeps the stitches as without it.
                    assert!(
                        last.distance(end) <= near_end.max(super::super::POINT_MISS_MM) || aimed == plain,
                        "{name}, {start:?} to {end:?}: last needle point {last:?}, {:.3} mm off",
                        last.distance(end)
                    );
                }
            }
        }
    }

    /// A shape sewn with no exit set ends where its start leads, at no cost; its own start and end
    /// can be put first; and it can offer fewer points, its first kept.
    #[test]
    fn natural_own_first_and_thinned() {
        let square = line(&[(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)], true);
        let mut e = ends(&square, &Params::for_style(Style::Running), false).unwrap();
        let n = e.natural();
        assert_eq!((n.cost(3, 3), n.cost(3, 4)), (Some(0.0), None), "a closed line ends where it starts");
        let (first, last) = (e.starts[5], e.ends[9]);
        e.own_first(first, last);
        assert_eq!((e.starts[0], e.ends[0]), (first, last));
        let way = e.cost(0, 0).unwrap();
        assert!(way > 0.0, "start 5 to end 9 is still four points round: {way}");
        // A start and end between the spread points are taken as they are, at their place along it:
        // started and ended there, it costs nothing.
        let mut r = ends(&square, &Params::for_style(Style::Running), false).unwrap();
        let own = Point::new(13.7, 0.0);
        r.own_first(own, own);
        assert_eq!((r.starts[0], r.ends[0], r.cost(0, 0)), (own, own, Some(0.0)), "its own start, exactly");
        assert!(r.cost(0, 1).unwrap() > 0.0, "to another point round it");
        let t = e.thinned(4);
        assert_eq!((t.starts.len(), t.ends.len(), t.starts[0], t.ends[0]), (4, 4, first, last));
        assert_eq!(t.cost(0, 0), Some(way));
        let bar = line(&[(0.0, 0.0), (60.0, 0.0), (60.0, 20.0), (0.0, 20.0)], true);
        let mut f = ends(&bar, &Params { angle: 0.0, ..Params::for_style(Style::Fill) }, false).unwrap();
        let count = f.starts.len();
        f.own_first(Point::new(0.2, 0.2), Point::new(59.8, 19.8));
        assert_eq!((f.starts.len(), f.starts[0]), (count + 1, Point::new(0.2, 0.2)), "a fill takes its own points as they are");
    }

    /// A line of several subpaths has no ends here; neither has nothing.
    #[test]
    fn several_subpaths_have_none() {
        let mut two = line(&[(0.0, 0.0), (10.0, 0.0)], false);
        two.extend(line(&[(20.0, 0.0), (30.0, 0.0)], false));
        assert!(ends(&two, &Params::for_style(Style::Running), false).is_none());
        assert!(ends(&BezPath::new(), &Params::for_style(Style::Fill), false).is_none());
    }
}
