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
/// of `out`'s, a closed piece's within half a stitch.
fn every_piece_sewn(path: &BezPath, out: &[Stitch]) {
    let got = needle(out);
    for sub in subpaths(path) {
        // A closed piece is entered at its point nearest the needle, so its needle points lie
        // elsewhere along it than its own resampling's: each of those lies within half a stitch
        // of one sewn.
        let near = if sub.closed { running().stitch_length / 2.0 + 1e-9 } else { 1e-9 };
        for q in resample(&sub.pts, running().stitch_length, sub.closed) {
            assert!(got.iter().any(|g| g.distance(q) < near), "{q:?} of a piece isn't sewn");
        }
    }
}

/// The millimetres a route through all of `path`'s pieces (one connected part) sews in all.
fn route_length(path: &BezPath) -> f64 {
    let subs = subpaths(path);
    let graph = Graph::of(&subs).expect("pieces that meet");
    let parts = graph.components();
    assert_eq!(parts.len(), 1, "one drawing");
    let r = route(&graph, &parts[0], None, None, &|| false).expect("a route");
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

/// Whether every node of the drawing lies well clear of the pieces it isn't on (twice the gap), so
/// that no piece is cut at a node on its middle and the drawing is the graph drawn.
fn clear(nodes: &[Point], edges: &[(usize, usize)]) -> bool {
    edges.iter().all(|&(a, b)| {
        let (p, q) = (nodes[a], nodes[b]);
        nodes.iter().enumerate().filter(|&(n, _)| n != a && n != b).all(|(_, &c)| {
            let ab = q - p;
            let t = ((c - p).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
            c.distance(p + ab * t) > 2.0 * GAP_BRIDGE
        })
    })
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
        if !clear(&nodes, &edges) {
            continue;
        }
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
/// sewn twice than twice the least (found by trying every set); measured (2026-11-08): at most 1.005
/// times it on these, 1.157 on 5,310 drawings of forty other seeds (24.4 when the odd nodes were
/// paired through the spanning tree).
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
        if edges.len() > 12 || !clear(&nodes, &edges) {
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

/// Many drawings in one line are each sewn, in turn: 20,000 V's of two pieces each, every V's apex
/// a needle point; and two pieces that meet among 20,000 closed specks, the two sewn as one route.
/// (How long such lines take is the perf report's: `perf_embroidery/routes.rs`.)
#[test]
fn many_drawings_in_one_line_are_each_sewn() {
    let vs: Vec<Vec<(f64, f64)>> = (0..20_000)
        .flat_map(|k: i32| {
            let (x, y) = (f64::from(k % 200) * 3.0, f64::from(k / 200) * 3.0);
            [vec![(x, y), (x + 0.5, y + 1.0)], vec![(x + 0.5, y + 1.0), (x + 1.0, y)]]
        })
        .collect();
    let (out, _) = sewn(&pieces(&vs), Aim::from_needle(None));
    let got: std::collections::BTreeSet<(i64, i64)> =
        needle(&out).iter().map(|q| ((q.x * 1000.0).round() as i64, (q.y * 1000.0).round() as i64)).collect();
    for k in 0..20_000 {
        let apex = (f64::from(k % 200) * 3.0 + 0.5, f64::from(k / 200) * 3.0 + 1.0);
        assert!(got.contains(&((apex.0 * 1000.0).round() as i64, (apex.1 * 1000.0).round() as i64)), "the V at {apex:?} isn't sewn");
    }
    let mut specks = pieces(&[vec![(-10.0, -10.0), (-5.0, -10.0)], vec![(-5.0, -10.0), (-5.0, -5.0)]]);
    for k in 0..20_000 {
        let (x, y) = (f64::from(k % 200) * 0.5, f64::from(k / 200) * 0.5);
        specks.move_to((x, y));
        specks.line_to((x + 0.5, y));
        specks.line_to((x, y + 0.5));
        specks.close_path();
    }
    let (out, _) = sewn(&specks, Aim::from_needle(Some(Point::new(-10.0, -10.0))));
    let first: Vec<Point> = needle(&out).into_iter().take_while(|q| q.x < -1.0).collect();
    assert!(first.iter().any(|q| q.distance(Point::new(-5.0, -5.0)) < 1e-9), "the two pieces that meet aren't one run: {first:?}");
}

/// A running line's routes stop at a cancel: asked at once, or once a few of its drawings are
/// planned, the plan of 1,000 V's is dropped; never asked to, it is made.
#[test]
fn a_running_line_stops_at_a_cancel() {
    let vs = pieces(
        &(0..1_000)
            .flat_map(|k: i32| {
                let x = f64::from(k) * 3.0;
                [vec![(x, 0.0), (x + 0.5, 1.0)], vec![(x + 0.5, 1.0), (x + 1.0, 0.0)]]
            })
            .collect::<Vec<_>>(),
    );
    let plan = |stopped: &dyn Fn() -> bool| try_stitches_aimed_until(&vs, FillRule::NonZero, &running(), Aim::from_needle(None), stopped).unwrap();
    assert!(plan(&|| true).is_none(), "planned though stopped at once");
    let asked = std::cell::Cell::new(0usize);
    let later = || {
        asked.set(asked.get() + 1);
        asked.get() > 10
    };
    assert!(plan(&later).is_none() && asked.get() > 10, "planned though stopped partway");
    assert!(plan(&|| false).is_some_and(|s| !s.is_empty()));
}

/// A line that ends on another's middle joins its route (the project owner's choice, 2026-11-07,
/// „Zrobić teraz”): a T drawn as two strokes, its stem's end on the bar's middle, and a K whose two
/// arms end on its stem's middle, each a stroke of its own, are sewn in one go, no jump and no
/// connecting stitch, the touched line cut where the other ends; the T sews its stem twice, the
/// shortest way, as a T of three pieces does. A line ending 1 mm off the other stays apart.
#[test]
fn a_line_that_ends_on_anothers_middle_joins_its_route() {
    let t = pieces(&[vec![(0.0, 0.0), (20.0, 0.0)], vec![(10.0, 0.0), (10.0, 6.0)]]);
    let k = pieces(&[vec![(0.0, 0.0), (0.0, 20.0)], vec![(9.0, 0.0), (0.0, 10.0)], vec![(0.0, 10.0), (9.0, 20.0)]]);
    for (name, path) in [("T", &t), ("K", &k)] {
        let (out, connectors) = sewn(path, Aim::from_needle(None));
        assert_eq!(jumps(&out), 0, "{name}: a jump");
        assert!(connectors.abs() < 1e-9, "{name}: {connectors} mm of connecting stitches");
        let got = needle(&out);
        for sub in subpaths(path) {
            for q in resample(&sub.pts, 0.5, false) {
                assert!(got.windows(2).any(|w| to_segment(q, w[0], w[1]) < 1e-6), "{name}: {q:?} isn't sewn");
            }
        }
    }
    assert!((route_length(&t) - (26.0 + 6.0)).abs() < 1e-9, "the T sews its stem twice");
    let apart = pieces(&[vec![(0.0, 0.0), (20.0, 0.0)], vec![(10.0, 1.0), (10.0, 6.0)]]);
    assert!(Graph::of(&subpaths(&apart)).is_none(), "a line ending 1 mm off the bar joins it");
}

/// How far `q` lies from the segment `a`–`b`.
fn to_segment(q: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let t = if ab.hypot2() > 0.0 { ((q - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
    q.distance(a + ab * t)
}

/// Ends within a thread's width meet wherever the drawing lies: two pieces 10 mm long whose ends lie
/// 0.8 of the gap apart, the gap moved along x by hundredths of a millimetre, and to x = 0.28 and
/// 3 mm, where the cells fall differently.
#[test]
fn pieces_that_nearly_meet_are_one_drawing_wherever_they_lie() {
    for x in (0..100).map(|k| 10.0 + f64::from(k) * 0.01).chain([0.28, 3.0]) {
        let near = pieces(&[vec![(x - 10.0, 0.0), (x, 0.0)], vec![(x + 0.8 * GAP_BRIDGE, 0.0), (x + 10.0, 0.0)]]);
        assert!(Graph::of(&subpaths(&near)).is_some(), "ends {} mm apart at x = {x} don't meet", 0.8 * GAP_BRIDGE);
    }
}

/// Ends meet within the gap of a node's point, not through a chain: thirty strands whose roots lie
/// 0.25 mm apart in a row (7.25 mm end to end) make no node whose ends lie farther than the gap
/// from its point.
#[test]
fn a_node_is_no_wider_than_the_gap() {
    let strands: Vec<Vec<(f64, f64)>> = (0..30)
        .map(|k| {
            let x = 0.25 * f64::from(k);
            vec![(x, 0.0), (x + f64::from(k) - 15.0, 3.0)]
        })
        .collect();
    let subs = subpaths(&pieces(&strands));
    let graph = Graph::of(&subs).expect("pieces that meet");
    for e in &graph.edges {
        for (end, node) in [(e.pts[0], e.a), (*e.pts.last().unwrap(), e.b)] {
            assert!(end.distance(graph.nodes[node]) <= GAP_BRIDGE + 1e-9, "an end {} mm from its node", end.distance(graph.nodes[node]));
        }
    }
}

/// A route sews no stitch across a node longer than a stitch: two hundred dashes 0.2 mm long 0.25 mm
/// apart, drawn out of order, joined end to end through a chain of ends, sewed a stitch 45 mm long
/// across the one node they made.
#[test]
fn a_route_sews_no_stitch_across_its_node() {
    let dash = |k: u32| {
        let x = 0.45 * f64::from(k);
        vec![(x, 0.0), (x + 0.2, 0.0)]
    };
    let path = pieces(&(0..100).flat_map(|k| [dash(k), dash(k + 100)]).collect::<Vec<_>>());
    let (out, _) = sewn(&path, Aim::from_needle(None));
    let longest = out
        .windows(2)
        .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
        .map(|w| w[0].point().distance(w[1].point()))
        .fold(0.0, f64::max);
    assert!(longest <= running().stitch_length * 1.5, "a stitch {longest:.1} mm long");
}

/// A mesh drawn as one running line (a 20 × 20 grid of pieces 7 mm long) sews twice little more
/// than the least route does: every border node but the corners meets three pieces, and the least
/// pairs them along the border, 35 pieces with the route's two ends among them. Paired through a
/// spanning tree, it sewed 1,589 mm twice where 245 do.
#[test]
fn a_mesh_sews_few_pieces_twice() {
    const N: usize = 20;
    const STEP: f64 = 7.0;
    let at = |k: usize| STEP * (k as f64 + 1.0);
    let mut lines = Vec::new();
    for i in 0..N {
        for j in 0..N - 1 {
            lines.push(vec![(at(j), at(i)), (at(j + 1), at(i))]);
            lines.push(vec![(at(i), at(j)), (at(i), at(j + 1))]);
        }
    }
    let path = pieces(&lines);
    let total = (2 * N * (N - 1)) as f64 * STEP;
    let least = ((4 * (N - 2) - 2) / 2) as f64 * STEP;
    let twice = route_length(&path) - total;
    assert!(twice <= 1.5 * least + 1e-6, "{twice} mm sewn twice, the least {least} mm");
}

/// A grid of lines sews twice little more than its odd nodes need: a 20 × 20 grid of 1 mm squares
/// needs 35 mm (its 72 border nodes paired along the border, the route's two ends left odd), where
/// pairing through a spanning tree sewed 227.
#[test]
fn a_grid_sews_twice_about_the_least() {
    const SIDE: usize = 20;
    let mut lines = Vec::new();
    for i in 0..SIDE {
        for j in 0..SIDE - 1 {
            let (a, b) = (i as f64, j as f64);
            lines.push(vec![(b, a), (b + 1.0, a)]);
            lines.push(vec![(a, b), (a, b + 1.0)]);
        }
    }
    let total = (2 * SIDE * (SIDE - 1)) as f64;
    let least = (2 * (SIDE - 2) - 1) as f64;
    let twice = route_length(&pieces(&lines)) - total;
    assert!(twice <= 2.0 * least, "{twice} mm sewn twice, the least {least} mm");
}

/// A ring with two long whiskers at neighbouring corners sews the ring's side between them twice,
/// not a whisker: an octagon 10 mm round, whiskers 30 mm long out of corners 4 and 5.
#[test]
fn a_ring_with_two_whiskers_sews_the_short_way_twice() {
    let corner = |k: usize| {
        let a = std::f64::consts::TAU * k as f64 / 8.0;
        (10.0 * a.cos(), 10.0 * a.sin())
    };
    let mut lines: Vec<Vec<(f64, f64)>> = (0..8).map(|k| vec![corner(k), corner((k + 1) % 8)]).collect();
    for k in [4, 5] {
        let (x, y) = corner(k);
        lines.push(vec![(x, y), (4.0 * x, 4.0 * y)]);
    }
    let side = 20.0 * (std::f64::consts::PI / 8.0).sin();
    let total = 8.0 * side + 2.0 * 30.0;
    let twice = route_length(&pieces(&lines)) - total;
    assert!(twice <= side + 1e-6, "{twice} mm sewn twice, the side between the whiskers is {side} mm");
}

/// A ladder of six 5 mm rungs, its rails in 3 mm pieces, sews the least, 9 mm, twice.
#[test]
fn a_ladder_sews_twice_only_the_least() {
    let mut lines = Vec::new();
    for i in 0..5 {
        let x = f64::from(i) * 3.0;
        lines.push(vec![(x, 0.0), (x + 3.0, 0.0)]);
        lines.push(vec![(x, 5.0), (x + 3.0, 5.0)]);
    }
    for i in 0..6 {
        let x = f64::from(i) * 3.0;
        lines.push(vec![(x, 0.0), (x, 5.0)]);
    }
    let total = 10.0 * 3.0 + 6.0 * 5.0;
    assert!((route_length(&pieces(&lines)) - total - 9.0).abs() < 1e-9);
}

/// The cycles sewn twice more than half way are swapped: a drawing of seven nodes and eight pieces
/// (one of the random drawings), whose odd nodes paired nearest first sew 87.3 mm twice, sews the
/// least, 54.2 mm (found by trying every set), once its cycles are.
#[test]
fn swapping_a_cycle_reaches_the_least() {
    let nodes: Vec<Point> = [(8.727, 10.331), (33.37, 36.779), (15.347, 31.259), (4.689, 18.693), (3.219, 25.254), (6.045, 4.065), (4.156, 11.742)]
        .into_iter()
        .map(|(x, y)| Point::new(x, y))
        .collect();
    let edges = [(0, 1), (2, 1), (2, 3), (4, 1), (1, 5), (1, 6), (3, 0), (0, 2)];
    let path = path_of(&nodes, &edges);
    let total: f64 = edges.iter().map(|&(a, b)| nodes[a].distance(nodes[b])).sum();
    let (twice, least) = (route_length(&path) - total, least_twice(&nodes, &edges));
    assert!((twice - least).abs() < 1e-6, "{twice} mm sewn twice, the least {least}");
}

/// A drawing whose spanning tree pairs its odd nodes through 17.3 mm where 5.39 mm will do sews the
/// least twice (found by trying every set).
#[test]
fn pairing_the_odd_nodes_reaches_the_least() {
    let nodes: Vec<Point> = [(32.0, 32.0), (30.0, 27.0), (35.0, 18.0), (21.0, 35.0), (29.0, 33.0), (37.0, 36.0), (30.0, 20.0), (29.0, 30.0)]
        .into_iter()
        .map(|(x, y)| Point::new(x, y))
        .collect();
    let edges = [(0, 1), (2, 1), (3, 2), (1, 4), (5, 3), (4, 6), (5, 7), (6, 1), (2, 6), (0, 3)];
    let path = path_of(&nodes, &edges);
    let total: f64 = edges.iter().map(|&(a, b)| nodes[a].distance(nodes[b])).sum();
    let (twice, least) = (route_length(&path) - total, least_twice(&nodes, &edges));
    assert!((twice - least).abs() < 1e-6, "{twice} mm sewn twice, the least {least}");
}

/// A drawing of pieces starts nearest the needle where that sews no more twice, as a line's
/// subpaths do: a rectangle of four pieces, the needle by its bottom-right corner, starts there,
/// where it started at its first piece, a 45 mm move away; a plus of four equal arms, the needle by
/// its right arm's tip, starts at that tip.
#[test]
fn a_route_starts_nearest_the_needle_when_that_costs_nothing() {
    let rect =
        pieces(&[vec![(0.0, 0.0), (40.0, 0.0)], vec![(40.0, 0.0), (40.0, 20.0)], vec![(40.0, 20.0), (0.0, 20.0)], vec![(0.0, 20.0), (0.0, 0.0)]]);
    let (out, _) = sewn(&rect, Aim::from_needle(Some(Point::new(40.5, 20.5))));
    let first = needle(&out).first().copied();
    assert!(first.is_some_and(|q| q.distance(Point::new(40.0, 20.0)) < 1e-9), "the rectangle starts at {first:?}");
    let plus =
        pieces(&[vec![(0.0, 0.0), (0.0, -10.0)], vec![(0.0, 0.0), (10.0, 0.0)], vec![(0.0, 0.0), (0.0, 10.0)], vec![(0.0, 0.0), (-10.0, 0.0)]]);
    let (out, _) = sewn(&plus, Aim::from_needle(Some(Point::new(10.5, 0.0))));
    let first = needle(&out).first().copied();
    assert!(first.is_some_and(|q| q.distance(Point::new(10.0, 0.0)) < 1e-9), "the plus starts at {first:?}");
}

/// An entry is met whatever the exit: two V's drawn as pieces, the entry and the exit at the right
/// V's two tips, start at the entry, where the exit took the right V and the line started at the
/// left one.
#[test]
fn an_exit_on_the_entrys_drawing_leaves_the_entry_met() {
    let path = pieces(&[vec![(0.0, 0.0), (2.0, 4.0)], vec![(2.0, 4.0), (4.0, 0.0)], vec![(-7.0, 0.0), (-5.0, 4.0)], vec![(-5.0, 4.0), (-3.0, 0.0)]]);
    let entry = Point::new(0.0, 0.0);
    let out = stitches_aimed(&path, FillRule::NonZero, &running(), Aim { start: Start::Entry(entry), exit: Some(Point::new(4.0, 0.0)) });
    let first = needle(&out).first().copied();
    assert!(first.is_some_and(|q| q.distance(entry) < 1e-9), "starts at {first:?}");
}

/// A route given only an entry, or only an exit, is free at its other end: a T from its stem's tip
/// sews the least, one arm twice, and ends elsewhere than it starts.
#[test]
fn a_route_with_one_end_given_sews_least_twice() {
    let t = pieces(&[vec![(0.0, 0.0), (10.0, 0.0)], vec![(10.0, 0.0), (20.0, 0.0)], vec![(10.0, 0.0), (10.0, 6.0)]]);
    let subs = subpaths(&t);
    let graph = Graph::of(&subs).expect("pieces that meet");
    let parts = graph.components();
    let tip = graph.nodes.iter().position(|q| q.distance(Point::new(10.0, 6.0)) < 1e-9).expect("the stem's tip");
    for (start, end) in [(Some(tip), None), (None, Some(tip))] {
        let r = route(&graph, &parts[0], start, end, &|| false).expect("a route");
        let twice: f64 = r.steps.iter().filter(|s| s.again).map(|s| graph.edges[s.edge].len).sum();
        assert!((twice - 10.0).abs() < 1e-9 && r.start != r.end, "{start:?} {end:?}: {twice} mm twice");
    }
}

/// A drawing shorter than the shortest stitch is left out, as each of its pieces would be: a 0.1 mm V
/// 20 mm past a line adds no jump and no needle point, where it was sewn as a lone needle point
/// with a jump to it.
#[test]
fn a_drawing_shorter_than_the_shortest_stitch_is_left_out() {
    let path = pieces(&[vec![(0.0, 0.0), (30.0, 0.0)], vec![(50.0, 0.0), (50.1, 0.0)], vec![(50.1, 0.0), (50.1, 0.1)]]);
    let (out, _) = sewn(&path, Aim::from_needle(None));
    assert_eq!(jumps(&out), 0, "a jump to a speck");
    assert!(needle(&out).iter().all(|q| q.x <= 30.0 + 1e-9), "a speck sewn");
}

/// A line cut into pieces shorter than the shortest stitch is sewn, as the drawing they make is long
/// enough (each piece on its own would be left out): 100 pieces 0.3 mm long in a row.
#[test]
fn a_line_of_tiny_pieces_is_sewn() {
    let line: Vec<Vec<(f64, f64)>> = (0..100)
        .map(|k| {
            let x = 0.3 * f64::from(k);
            vec![(x, 0.0), (x + 0.3, 0.0)]
        })
        .collect();
    let (out, _) = sewn(&pieces(&line), Aim::from_needle(None));
    let xs: Vec<f64> = needle(&out).iter().map(|q| q.x).collect();
    assert!(xs.iter().any(|&x| x < 0.5) && xs.iter().any(|&x| x > 29.5), "{xs:?}");
}
