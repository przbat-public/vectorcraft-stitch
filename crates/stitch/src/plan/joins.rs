//! The entry and exit points that join a chain of shapes, sewn in a fixed order, by the shortest
//! connections ([`joins`]): of each shape's starts and ends ([`super::Ends`]), the ones whose moves
//! between shapes cost the least in all, by the planner's own weights.
//!
//! With the order fixed, choosing one start and one end a shape is a shortest path through a graph
//! of two layers a shape (its starts, then its ends), which a dynamic programme finds exactly over
//! the points offered: the cluster optimisation of the generalised travelling salesman problem
//! (Fischetti, Salazar-González and Toth, Operations Research 45(3), 1997; Gutin and Karapetyan,
//! arXiv:0804.0722 §3), a shortest path in a directed acyclic graph (Cormen et al., Introduction to
//! Algorithms, 3rd ed., §24.2). Each shape's own start and end (its index 0, where it starts and
//! ends now) are among the options, so the joins found never cost more than those it has.
//!
//! A move along the thread (inside a block, no cut before it) costs its length, [`COST_LIFT`] when
//! the needle lifts for it and [`COST_TRIM`] when it is longer than the trim length; across a cut
//! (a colour change, a Trim After) no thread lies, and only the frame's travel counts, at
//! [`COST_ENTRY`]. A shape that follows (one whose points aren't set, so that it starts as near the
//! needle as it can) takes the start nearest the end before it. Distances are square roots of sums
//! of squares, which IEEE arithmetic rounds the same on every platform.

use vectorcraft_geom::Point;

use super::Ends;
use super::runs::{COST_ENTRY, COST_LIFT, COST_TRIM};
use crate::SAME_POINT;

/// The most moves the programme weighs in all (each a previous end against a start, or a start
/// against an end): past it, each shape offers fewer points (an estimate: about 0.2 s).
pub const MAX_JOIN_EDGES: usize = 50_000_000;

/// How the needle comes to a shape from the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// The design's first shape: from nowhere.
    First,
    /// Along the thread: inside a block, no cut before it.
    Thread,
    /// Across a cut: a colour change, or a Trim After.
    Cut,
}

/// What the programme may choose of a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Any of its starts and ends: its points are set by the result.
    Free,
    /// Its start nearest the end before it, and the end that leads to: a shape whose points aren't
    /// set and aren't set by the result.
    Follows,
}

/// One shape of the chain: where it can start and end, how the needle comes to it, and what may be
/// chosen of it.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinShape {
    pub ends: Ends,
    pub link: Link,
    pub role: Role,
}

/// The start and end chosen for each shape ([`Ends::starts`], [`Ends::ends`] indices, in the
/// chain's order) and what the moves between them cost in all.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    pub chosen: Vec<(usize, usize)>,
    pub cost: f64,
}

/// What a move of `d` millimetres into a shape costs, by `link` and the design's `trim` length.
pub fn move_cost(d: f64, link: Link, trim: f64) -> f64 {
    match link {
        Link::First => 0.0,
        Link::Cut => COST_ENTRY * d,
        Link::Thread => d + if d > SAME_POINT { COST_LIFT } else { 0.0 } + if d > trim { COST_TRIM } else { 0.0 },
    }
}

/// The distance from `a` to `b`, rounded the same on every platform.
fn apart(a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    (dx * dx + dy * dy).sqrt()
}

/// The starts and ends of `chain` that join it by the cheapest moves ([`move_cost`], with the
/// design's `trim` length), each shape's inner cost ([`Ends::cost`]) counted: exact over the points
/// offered, ties kept to the lowest index (a shape's own points first). `None` for an empty chain
/// or a shape with no way through it.
pub fn joins(chain: &[JoinShape], trim: f64) -> Option<Joined> {
    // The cheapest cost to each end of the shape before, and of this one's starts.
    let mut at_ends: Vec<f64> = Vec::new();
    let mut prev_ends: &[Point] = &[];
    // For each shape: each start's best previous end, and each end's best start.
    let mut back: Vec<(Vec<usize>, Vec<usize>)> = Vec::with_capacity(chain.len());
    for (i, shape) in chain.iter().enumerate() {
        let e = &shape.ends;
        if e.starts.is_empty() || e.ends.is_empty() {
            return None;
        }
        let mut at_starts = vec![f64::INFINITY; e.starts.len()];
        let mut start_from = vec![0usize; e.starts.len()];
        if i == 0 {
            at_starts.iter_mut().for_each(|c| *c = 0.0);
        } else {
            match shape.role {
                Role::Free => {
                    for (s, &start) in e.starts.iter().enumerate() {
                        for (x, &end) in prev_ends.iter().enumerate() {
                            let c = at_ends.get(x).copied().unwrap_or(f64::INFINITY) + move_cost(apart(end, start), shape.link, trim);
                            if c < at_starts[s] {
                                (at_starts[s], start_from[s]) = (c, x);
                            }
                        }
                    }
                }
                Role::Follows => {
                    // From each previous end, the start nearest it: the one the planner takes.
                    for (x, &end) in prev_ends.iter().enumerate() {
                        let Some(s) = nearest(&e.starts, end) else { continue };
                        let c = at_ends.get(x).copied().unwrap_or(f64::INFINITY) + move_cost(apart(end, e.starts[s]), shape.link, trim);
                        if c < at_starts[s] {
                            (at_starts[s], start_from[s]) = (c, x);
                        }
                    }
                }
            }
        }
        let mut next = vec![f64::INFINITY; e.ends.len()];
        let mut end_from = vec![0usize; e.ends.len()];
        for (s, &cs) in at_starts.iter().enumerate() {
            if !cs.is_finite() {
                continue;
            }
            for (k, slot) in next.iter_mut().enumerate() {
                let Some(inner) = e.cost(s, k) else { continue };
                if cs + inner < *slot {
                    (*slot, end_from[k]) = (cs + inner, s);
                }
            }
        }
        back.push((start_from, end_from));
        at_ends = next;
        prev_ends = &e.ends;
    }
    // The cheapest last end, then back through the choices.
    let (mut end, cost) = at_ends.iter().enumerate().fold((0usize, f64::INFINITY), |best, (k, &c)| if c < best.1 { (k, c) } else { best });
    if !cost.is_finite() {
        return None;
    }
    let mut chosen = vec![(0, 0); chain.len()];
    for (i, (start_from, end_from)) in back.iter().enumerate().rev() {
        let start = *end_from.get(end)?;
        chosen[i] = (start, end);
        end = *start_from.get(start)?;
    }
    Some(Joined { chosen, cost })
}

/// What `chain` costs joined at `chosen` (each shape's start and end), by the same weights as
/// [`joins`]; `None` where a choice isn't a way through its shape.
pub fn joined_cost(chain: &[JoinShape], chosen: &[(usize, usize)], trim: f64) -> Option<f64> {
    let mut total = 0.0;
    let mut before: Option<Point> = None;
    for (shape, &(s, e)) in chain.iter().zip(chosen) {
        let start = *shape.ends.starts.get(s)?;
        if let Some(b) = before {
            total += move_cost(apart(b, start), shape.link, trim);
        }
        total += shape.ends.cost(s, e)?;
        before = Some(*shape.ends.ends.get(e)?);
    }
    Some(total)
}

/// The index of the point of `pts` nearest `q`, the first of equal ones.
fn nearest(pts: &[Point], q: Point) -> Option<usize> {
    pts.iter()
        .enumerate()
        .fold(None, |best: Option<(usize, f64)>, (i, &p)| {
            let d = apart(q, p);
            if best.is_none_or(|(_, b)| d < b) { Some((i, d)) } else { best }
        })
        .map(|(i, _)| i)
}

/// How many of `chain`'s points to keep a shape so that the programme weighs at most
/// [`MAX_JOIN_EDGES`] moves: `None` when it is within that already, else the most points a shape
/// may offer (at least two).
pub fn thinned_to(chain: &[JoinShape]) -> Option<usize> {
    let work: usize = chain
        .windows(2)
        .map(|w| w[0].ends.ends.len().saturating_mul(w[1].ends.starts.len()))
        .chain(chain.iter().map(|s| s.ends.starts.len().saturating_mul(s.ends.ends.len())))
        .fold(0usize, usize::saturating_add);
    if work <= MAX_JOIN_EDGES {
        return None;
    }
    // Every shape offering `n` starts and ends weighs about 2n² moves.
    let per_shape = MAX_JOIN_EDGES / chain.len().max(1) / 2;
    Some(((per_shape as f64).sqrt() as usize).max(2))
}

#[cfg(test)]
mod tests {
    use super::super::Through;
    use super::*;

    fn open(a: (f64, f64), b: (f64, f64)) -> Ends {
        let (a, b) = (Point::new(a.0, a.1), Point::new(b.0, b.1));
        Ends { starts: vec![a, b], ends: vec![a, b], through: Through::Table(vec![vec![None, Some(0.0)], vec![Some(0.0), None]]) }
    }

    fn shape(ends: Ends, link: Link) -> JoinShape {
        JoinShape { ends, link, role: Role::Free }
    }

    /// Three lines: the needle leaves the first at the origin, the second runs from x = −6 to x = 1,
    /// the third starts 6 mm right of the second's right end. Started at its nearer end, the second
    /// ends 13 mm from the third (a cut, past the 10 mm trim length); turned round, it starts 6 mm
    /// away and ends 6 mm from the third: no cut.
    #[test]
    fn a_line_is_turned_round_for_the_next() {
        let chain = vec![
            shape(Ends::fixed(Point::ORIGIN, Point::ORIGIN), Link::First),
            shape(open((1.0, 0.0), (-6.0, 0.0)), Link::Thread),
            shape(Ends::fixed(Point::new(7.0, 0.0), Point::new(20.0, 0.0)), Link::Thread),
        ];
        let j = joins(&chain, 10.0).unwrap();
        assert_eq!(j.chosen[1], (1, 0), "from its left end to its right");
        let before = joined_cost(&chain, &[(0, 0), (0, 1), (0, 0)], 10.0).unwrap();
        assert!(j.cost < before && (j.cost - (6.0 + 6.0 + 2.0 * COST_LIFT)).abs() < 1e-9, "{} against {before}", j.cost);
    }

    /// Across a cut, only the frame's travel counts: it decides nothing but ties.
    #[test]
    fn a_cut_breaks_the_chain() {
        let chain = vec![shape(Ends::fixed(Point::ORIGIN, Point::ORIGIN), Link::First), shape(open((1.0, 0.0), (-6.0, 0.0)), Link::Cut)];
        let j = joins(&chain, 10.0).unwrap();
        assert!((j.cost - COST_ENTRY * 1.0).abs() < 1e-9);
    }

    /// A shape that follows starts at its start nearest the end before it, whatever comes next.
    #[test]
    fn a_shape_that_follows_starts_nearest_the_end_before_it() {
        let mut follows = shape(open((1.0, 0.0), (-6.0, 0.0)), Link::Thread);
        follows.role = Role::Follows;
        let chain = vec![
            shape(Ends::fixed(Point::ORIGIN, Point::ORIGIN), Link::First),
            follows,
            shape(Ends::fixed(Point::new(7.0, 0.0), Point::new(20.0, 0.0)), Link::Thread),
        ];
        assert_eq!(joins(&chain, 10.0).unwrap().chosen[1], (0, 1));
    }

    /// Each shape's own points (index 0) are kept where nothing is cheaper: joined again, a chain
    /// keeps the points it was joined at.
    #[test]
    fn a_second_run_keeps_its_points() {
        let chain = vec![
            shape(Ends::fixed(Point::ORIGIN, Point::ORIGIN), Link::First),
            shape(open((1.0, 0.0), (-6.0, 0.0)), Link::Thread),
            shape(Ends::fixed(Point::new(7.0, 0.0), Point::new(20.0, 0.0)), Link::Thread),
        ];
        let j = joins(&chain, 10.0).unwrap();
        // The chosen points put first, as the shapes' own.
        let again: Vec<JoinShape> = chain
            .iter()
            .zip(&j.chosen)
            .map(|(sh, &(s, e))| {
                let mut ends = sh.ends.clone();
                ends.starts.swap(0, s);
                ends.ends.swap(0, e);
                if let Through::Table(rows) = &mut ends.through {
                    rows.swap(0, s);
                    rows.iter_mut().for_each(|r| r.swap(0, e));
                }
                JoinShape { ends, ..sh.clone() }
            })
            .collect();
        let k = joins(&again, 10.0).unwrap();
        assert!(k.chosen.iter().all(|&c| c == (0, 0)), "{:?}", k.chosen);
        assert_eq!(k.cost, j.cost);
    }

    /// Past the most moves weighed, each shape offers fewer points, at least two.
    #[test]
    fn a_huge_chain_is_thinned() {
        let big = Ends { starts: vec![Point::ORIGIN; 40], ends: vec![Point::ORIGIN; 40], through: Through::Free };
        let chain: Vec<JoinShape> = (0..20_000).map(|_| shape(big.clone(), Link::Thread)).collect();
        let n = thinned_to(&chain).unwrap();
        assert!((2..40).contains(&n), "{n}");
        assert_eq!(thinned_to(&chain[..10]), None);
    }
}
