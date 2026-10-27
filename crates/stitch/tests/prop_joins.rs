//! The shortest joins of a chain of shapes (`plan::joins`) against brute force: for any chain of up
//! to six shapes, each offering up to four starts and four ends with any ways through it, any links
//! (along the thread, across a cut) and any roles (free, or following the end before it), the
//! programme finds the cheapest choice of every start and end, and the choice it gives costs what it
//! says; on chains of free shapes, it never costs more than each shape's own points.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_geom::Point;
use vectorcraft_stitch::plan::{Ends, JoinShape, Link, Role, Through, joined_cost, joins};

/// The trim length the chains are joined with, millimetres: the planner's default.
const TRIM: f64 = 10.0;

fn arb_point() -> impl Strategy<Value = Point> {
    (0.0..30.0f64, 0.0..30.0f64).prop_map(|(x, y)| Point::new(x, y))
}

/// A shape: one to four starts and ends, each way through it a cost or none.
fn arb_ends() -> impl Strategy<Value = Ends> {
    (prop::collection::vec(arb_point(), 1..5), prop::collection::vec(arb_point(), 1..5)).prop_flat_map(|(starts, ends)| {
        let (n, m) = (starts.len(), ends.len());
        prop::collection::vec(prop::collection::vec(prop::option::weighted(0.7, 0.0..20.0f64), m), n).prop_map(move |table| Ends {
            starts: starts.clone(),
            ends: ends.clone(),
            through: Through::Table(table),
        })
    })
}

fn arb_chain() -> impl Strategy<Value = Vec<JoinShape>> {
    prop::collection::vec(
        (arb_ends(), prop_oneof![3 => Just(Link::Thread), 1 => Just(Link::Cut)], prop_oneof![3 => Just(Role::Free), 1 => Just(Role::Follows)]),
        1..7,
    )
    .prop_map(|v| {
        v.into_iter().enumerate().map(|(i, (ends, link, role))| JoinShape { ends, link: if i == 0 { Link::First } else { link }, role }).collect()
    })
}

/// The index of the point of `pts` nearest `q`, the first of equal ones (as the programme takes a
/// following shape's start).
fn nearest(pts: &[Point], q: Point) -> usize {
    let d = |p: &Point| {
        let (dx, dy) = (p.x - q.x, p.y - q.y);
        (dx * dx + dy * dy).sqrt()
    };
    pts.iter().enumerate().fold((0, f64::INFINITY), |best, (i, p)| if d(p) < best.1 { (i, d(p)) } else { best }).0
}

/// The cheapest cost of `chain` over every choice its roles allow, `None` when none is a way
/// through every shape.
fn brute(chain: &[JoinShape], at: usize, before: Option<Point>, chosen: &mut Vec<(usize, usize)>) -> Option<f64> {
    let Some(shape) = chain.get(at) else { return joined_cost(chain, chosen, TRIM) };
    let starts: Vec<usize> = match (shape.role, before) {
        (Role::Follows, Some(b)) => vec![nearest(&shape.ends.starts, b)],
        _ => (0..shape.ends.starts.len()).collect(),
    };
    let mut best: Option<f64> = None;
    for s in starts {
        for e in 0..shape.ends.ends.len() {
            if shape.ends.cost(s, e).is_none() {
                continue;
            }
            chosen.push((s, e));
            if let Some(c) = brute(chain, at + 1, Some(shape.ends.ends[e]), chosen) {
                best = Some(best.map_or(c, |b: f64| b.min(c)));
            }
            chosen.pop();
        }
    }
    best
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// The programme's choice is the cheapest of every choice, and costs what it says.
    #[test]
    fn the_shortest_joins_are_the_best_of_every_choice(chain in arb_chain()) {
        let best = brute(&chain, 0, None, &mut Vec::new());
        let found = joins(&chain, TRIM);
        match (best, found) {
            (None, None) => {}
            (Some(b), Some(j)) => {
                prop_assert!((b - j.cost).abs() < 1e-9, "brute force {b}, the programme {}", j.cost);
                let again = joined_cost(&chain, &j.chosen, TRIM).unwrap();
                prop_assert!((again - j.cost).abs() < 1e-9, "its choice costs {again}, it says {}", j.cost);
            }
            (b, j) => prop_assert!(false, "brute force {b:?}, the programme {j:?}"),
        }
    }

    /// On a chain of free shapes, each offering a way through its own points (its first start and
    /// end), the joins never cost more than those.
    #[test]
    fn the_joins_never_cost_more_than_the_points_they_start_from(chain in arb_chain()) {
        let chain: Vec<JoinShape> = chain
            .into_iter()
            .map(|mut s| {
                s.role = Role::Free;
                if let Through::Table(rows) = &mut s.ends.through {
                    rows[0][0] = Some(rows[0][0].unwrap_or(0.0));
                }
                s
            })
            .collect();
        let own = joined_cost(&chain, &vec![(0, 0); chain.len()], TRIM).unwrap();
        let j = joins(&chain, TRIM).unwrap();
        prop_assert!(j.cost <= own + 1e-9, "{} against its own {own}", j.cost);
    }
}
