//! A running line whose pieces meet end to end, sewn as routes: no jump and no connecting stitch
//! inside a drawing, every piece's needle points sewn, on a tree exactly what the least route sews
//! twice, on a drawing with cycles never more than twice, a long way back jumped, an entry and an
//! exit met, and the same stitches every time.

use vectorcraft_geom::{BezPath, FillRule, Point};

use super::geometry::{length, resample, subpaths};
use super::route::{GAP_BRIDGE, Graph, route};
use super::*;
use crate::Command;

/// A path of open pieces, each a polyline through its points.
fn pieces(lines: &[Vec<(f64, f64)>]) -> BezPath {
    let mut p = BezPath::new();
    for line in lines {
        for (k, &q) in line.iter().enumerate() {
            if k == 0 {
                p.move_to(q);
            } else {
                p.line_to(q);
            }
        }
    }
    p
}

fn running() -> Params {
    Params::for_style(Style::Running)
}

fn needle(v: &[Stitch]) -> Vec<Point> {
    v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
}

fn jumps(v: &[Stitch]) -> usize {
    v.iter().filter(|s| s.command == Command::Jump).count()
}

/// The stitches and connecting stitches (millimetres) of `path` sewn as a running line from where
/// `aim` says.
fn sewn(path: &BezPath, aim: Aim) -> (Vec<Stitch>, f64) {
    super::running::running(path, &running(), aim)
}

/// Every piece's needle points, as a piece on its own is sewn, are sewn: each within a hair of one
/// of `out`'s.
fn every_piece_sewn(path: &BezPath, out: &[Stitch]) {
    let got = needle(out);
    for sub in subpaths(path) {
        for q in resample(&sub.pts, running().stitch_length, sub.closed) {
            assert!(got.iter().any(|g| g.distance(q) < 1e-9), "{q:?} of a piece isn't sewn");
        }
    }
}

/// The millimetres a route through all of `path`'s pieces (one connected part) sews in all.
fn route_length(path: &BezPath) -> f64 {
    let subs = subpaths(path);
    let graph = Graph::of(&subs).expect("pieces that meet");
    let parts = graph.components();
    assert_eq!(parts.len(), 1, "one drawing");
    let r = route(&graph, &parts[0], None, None).expect("a route");
    assert_eq!(r.steps.iter().filter(|s| !s.again).count(), graph.edges.len(), "every piece once");
    r.steps.iter().map(|s| graph.edges[s.edge].len).sum()
}

/// A T and an X: one run each, no jump, no connecting stitch, every piece sewn; the T sews its
/// shortest arm twice and the X its two shortest, as the least route through a tree does (every
/// edge twice but those of its longest path).
#[test]
fn a_t_and_an_x_are_sewn_in_one_go() {
    let t = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0, 0.0), (20.0, 0.0)], vec![(10.0, 0.0), (10.0, 6.0)]]);
    let x = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0, 0.0), (22.0, 0.0)], vec![(10.0, 0.0), (10.0, 8.0)], vec![(10.0, -9.0), (10.0, 0.0)]]);
    for (name, path, twice) in [("T", &t, 6.0), ("X", &x, 17.0)] {
        let (out, connectors) = sewn(path, Aim::from_needle(None));
        assert_eq!(jumps(&out), 0, "{name}: a jump");
        assert!(connectors.abs() < 1e-9, "{name}: {connectors} mm of connecting stitches");
        every_piece_sewn(path, &out);
        let total: f64 = subpaths(path).iter().map(|s| length(&s.pts, false)).sum();
        assert!((route_length(path) - (total + twice)).abs() < 1e-9, "{name}: {} sewn, {total} long", route_length(path));
    }
}

/// A tiny random number generator (a linear congruential one, Knuth's MMIX constants) for the
/// trees and graphs drawn here: the same every time.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n.saturating_sub(1))
    }
}

/// The least distance between the random drawings' nodes (millimetres): well past the gap
/// within which ends are one node ([`GAP_BRIDGE`]), so that the drawing is the graph drawn.
const NODES_APART: f64 = 1.0;

/// A random tree of `n` nodes scattered in a 40 mm square, at least [`NODES_APART`] apart, each
/// node after the first joined to an earlier one by a straight piece, drawn either way round.
fn random_tree(rng: &mut Lcg, n: usize) -> (Vec<Point>, Vec<(usize, usize)>) {
    let mut nodes: Vec<Point> = Vec::with_capacity(n);
    while nodes.len() < n {
        let q = Point::new(rng.next() * 40.0, rng.next() * 40.0);
        if nodes.iter().all(|o| o.distance(q) >= NODES_APART) {
            nodes.push(q);
        }
    }
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(n);
    for v in 1..n {
        let a = rng.below(v);
        edges.push(if rng.next() < 0.5 { (a, v) } else { (v, a) });
    }
    (nodes, edges)
}

fn path_of(nodes: &[Point], edges: &[(usize, usize)]) -> BezPath {
    pieces(&edges.iter().map(|&(a, b)| vec![(nodes[a].x, nodes[a].y), (nodes[b].x, nodes[b].y)]).collect::<Vec<_>>())
}

/// The longest path of a tree, by two searches (the farthest node from any, then the farthest from
/// it): written here apart from the planner's.
fn tree_diameter(nodes: &[Point], edges: &[(usize, usize)]) -> f64 {
    let farthest = |from: usize| -> (usize, f64) {
        let mut dist = vec![f64::NAN; nodes.len()];
        dist[from] = 0.0;
        let mut stack = vec![from];
        while let Some(v) = stack.pop() {
            for &(a, b) in edges {
                let w = if a == v {
                    b
                } else if b == v {
                    a
                } else {
                    continue;
                };
                if dist[w].is_nan() {
                    dist[w] = dist[v] + nodes[a].distance(nodes[b]);
                    stack.push(w);
                }
            }
        }
        dist.iter().copied().enumerate().max_by(|x, y| x.1.total_cmp(&y.1)).unwrap()
    };
    let (far, _) = farthest(0);
    farthest(far).1
}

/// Random trees (fixed seeds): the route sews exactly what the least route through a tree sews,
/// every edge twice but those of its longest path, found apart from the planner by two searches;
/// with no jump, no connecting stitch, and every piece sewn.
#[test]
fn a_random_tree_is_sewn_as_the_least_route_sews_it() {
    let mut rng = Lcg(0x5eed_0001);
    for case in 0..200 {
        let n = 3 + rng.below(10);
        let (nodes, edges) = random_tree(&mut rng, n);
        let path = path_of(&nodes, &edges);
        let total: f64 = edges.iter().map(|&(a, b)| nodes[a].distance(nodes[b])).sum();
        let least = 2.0 * total - tree_diameter(&nodes, &edges);
        let got = route_length(&path);
        assert!((got - least).abs() < 1e-6, "case {case}: {got} sewn, the least {least}");
        let (out, connectors) = sewn(&path, Aim::from_needle(Some(Point::new(-5.0, -5.0))));
        assert!(connectors.abs() < 1e-9, "case {case}: {connectors} mm of connecting stitches");
        every_piece_sewn(&path, &out);
        // A way back longer than a jump is worth is jumped: on these small trees, none.
        assert_eq!(jumps(&out), 0, "case {case}: a jump");
    }
}

/// The least a route through the pieces `edges` between `nodes` sews twice, by trying every set of
/// edges to sew twice (a graph of a dozen edges at most): those whose ends, added to the graph's,
/// leave no node odd but where the route starts and ends.
fn least_twice(nodes: &[Point], edges: &[(usize, usize)]) -> f64 {
    let mut best = f64::INFINITY;
    for mask in 0u32..(1 << edges.len()) {
        let mut odd = vec![false; nodes.len()];
        let mut twice = 0.0;
        for (k, &(a, b)) in edges.iter().enumerate() {
            let times = if mask & (1 << k) != 0 { 2 } else { 1 };
            if times == 2 {
                twice += nodes[a].distance(nodes[b]);
            }
            for _ in 0..times {
                odd[a] = !odd[a];
                odd[b] = !odd[b];
            }
        }
        if odd.iter().filter(|&&o| o).count() <= 2 {
            best = best.min(twice);
        }
    }
    best
}

/// Random drawings with cycles (fixed seeds): every piece sewn, none more than twice, and no more
/// sewn twice than twice the least (found by trying every set); measured: within it on every case.
#[test]
fn a_drawing_with_cycles_is_sewn_whole() {
    let mut rng = Lcg(0x5eed_0002);
    let mut worst: f64 = 1.0;
    for case in 0..200 {
        let n = 3 + rng.below(6);
        let (nodes, mut edges) = random_tree(&mut rng, n);
        // A few more pieces between nodes not yet joined: cycles.
        for _ in 0..1 + rng.below(4) {
            let (a, b) = (rng.below(n), rng.below(n));
            if a != b && !edges.iter().any(|&(x, y)| (x, y) == (a, b) || (x, y) == (b, a)) {
                edges.push((a, b));
            }
        }
        if edges.len() > 12 {
            continue;
        }
        let path = path_of(&nodes, &edges);
        let total: f64 = edges.iter().map(|&(a, b)| nodes[a].distance(nodes[b])).sum();
        let got = route_length(&path) - total;
        let least = least_twice(&nodes, &edges);
        assert!(got <= 2.0 * least + 1e-6, "case {case}: {got} mm sewn twice, the least {least}");
        if least > 1e-9 {
            worst = worst.max(got / least);
        }
        let (out, _) = sewn(&path, Aim::from_needle(None));
        every_piece_sewn(&path, &out);
    }
    eprintln!("drawings with cycles: at most {worst:.3} times the least sewn twice");
}

/// Pieces whose ends lie within a thread's width of each other are one drawing, the gap sewn as a
/// stitch; farther apart, two lines with a connecting stitch between them.
#[test]
fn pieces_that_nearly_meet_are_one_drawing() {
    let near = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0 + 0.8 * GAP_BRIDGE, 0.0), (20.0, 0.0)], vec![(10.0, 0.0), (10.0, 6.0)]]);
    let (out, connectors) = sewn(&near, Aim::from_needle(None));
    assert_eq!(jumps(&out), 0);
    assert!(connectors.abs() < 1e-9, "{connectors} mm of connecting stitches across a gap narrower than a thread");
    let apart = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0 + 3.0 * GAP_BRIDGE, 0.0), (20.0, 0.0)]]);
    assert!(Graph::of(&subpaths(&apart)).is_none(), "pieces a gap apart are routed");
}

/// A way back much longer than the jump across it is jumped: a star of two long straight arms and
/// a 150 mm hook whose tip comes back to 9 mm from the middle, sewn twice by the least route, is
/// jumped back from its tip, tied off and in.
#[test]
fn a_long_way_back_is_jumped() {
    // The hook: out 75 mm and back, ending 9 mm below the middle.
    let hook = vec![(0.0, 0.0), (0.0, 75.0), (4.0, 75.0), (4.0, 9.0)];
    let path = pieces(&[vec![(-200.0, 0.0), (0.0, 0.0)], vec![(0.0, 0.0), (200.0, 0.0)], hook]);
    let (out, _) = sewn(&path, Aim::from_needle(Some(Point::new(-200.0, 0.0))));
    assert!(jumps(&out) > 0, "the way back along the hook is sewn");
    every_piece_sewn(&path, &out);
}

/// From an entry point at a branch's tip, the route starts there; toward an exit at another tip, it
/// ends there.
#[test]
fn a_route_starts_at_its_entry_and_ends_at_its_exit() {
    let t = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0, 0.0), (20.0, 0.0)], vec![(10.0, 0.0), (10.0, 6.0)]]);
    let entry = Point::new(10.0, 6.0);
    let exit = Point::new(0.0, 0.0);
    let out = stitches_aimed(&t, FillRule::NonZero, &running(), Aim { start: Start::Entry(entry), exit: Some(exit) });
    let got = needle(&out);
    assert!(got.first().is_some_and(|q| q.distance(entry) < 1e-9), "starts at {:?}", got.first());
    assert!(got.last().is_some_and(|q| q.distance(exit) < 1e-9), "ends at {:?}", got.last());
    every_piece_sewn(&t, &out);
}

/// The same drawing sews the same stitches every time.
#[test]
fn a_route_is_the_same_every_time() {
    let mut rng = Lcg(0x5eed_0003);
    let (nodes, edges) = random_tree(&mut rng, 12);
    let path = path_of(&nodes, &edges);
    let aim = Aim::from_needle(Some(Point::new(1.0, 2.0)));
    assert_eq!(sewn(&path, aim), sewn(&path, aim));
}

/// Drawings as large as a hostile file can make them are routed in time: 50,000 pieces meeting at
/// one point, and a line of 100,000 pieces end to end (sewn as one run, nothing twice).
#[test]
fn huge_drawings_are_routed_in_time() {
    let star =
        pieces(&(0..50_000).map(|k| f64::from(k) * 0.000_125_663_7).map(|a| vec![(0.0, 0.0), (10.0 * a.cos(), 10.0 * a.sin())]).collect::<Vec<_>>());
    let started = std::time::Instant::now();
    let (out, _) = sewn(&star, Aim::from_needle(None));
    assert!(!out.is_empty());
    let chain = pieces(&(0..100_000).map(|k| f64::from(k) * 0.5).map(|x| vec![(x, 0.0), (x + 0.5, 0.0)]).collect::<Vec<_>>());
    assert!((route_length(&chain) - 50_000.0).abs() < 1e-6, "a chain sews nothing twice");
    let (out, _) = sewn(&chain, Aim::from_needle(None));
    assert_eq!(jumps(&out), 0);
    let took = started.elapsed();
    assert!(took.as_secs_f64() < 30.0, "{took:?}");
}
