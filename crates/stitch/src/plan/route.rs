//! A running line whose pieces meet end to end, sewn as one route: every piece once, and back along
//! a few of them in their own holes where the needle must return, with no jump and no connecting
//! stitch inside a drawing (the project owner's choices: "branches are sewn once each, going back
//! along a few", 2026-11-01, and „Wszędzie w planiście (polecam)”, 2026-11-07: for every running
//! line, not only those digitizing makes).
//!
//! The pieces are the edges of a graph whose nodes are where their ends meet ([`Graph`]: ends within
//! [`GAP_BRIDGE`] of each other are one node). Sewing every edge and coming back along as few as
//! possible is the route inspection ("Chinese postman") problem of Edmonds and Johnson (1973,
//! *Matching, Euler tours and the Chinese postman*, Mathematical Programming 5, §1–2): the edges
//! sewn twice must make every node even (an Euler path then exists, §2), so they form a *T-join*
//! for T the nodes where an odd number of edges meet, and the fewest millimetres sewn twice is a
//! T-join of least length. An open route may leave two nodes odd, where it starts and ends.
//!
//! Written here in a structure of its own, with no matching algorithm:
//! - **The T-join on a spanning tree** ([`tree_join`]): an edge of the tree is in it exactly when
//!   the part of the tree below it holds an odd number of T's nodes, the one T-join a tree has. On a
//!   tree that is the least one (every edge of a tree is sewn twice but those of the path between
//!   the route's two ends, Edmonds and Johnson's §3 on trees).
//! - **The route's two ends** ([`ends`]): taking the path between two nodes out of the join leaves
//!   those two odd, so the route ends there; the path that takes most off (its edges in the join
//!   counted for, those out of it against) is found on the tree as its heaviest path, the way a
//!   tree's longest path is.
//! - **Each cycle the tree leaves out made shorter** ([`shorten_cycles`]): a join changed on a
//!   cycle (its edges in and out of it swapped) leaves the same nodes odd, since every node of a
//!   cycle meets it an even number of times; it is swapped where that makes it shorter (more than
//!   half of the cycle's length in it). On a graph with cycles this is not always the least, but it
//!   never sews more twice than the tree's join, and it is exact on trees.
//! - **The Euler path** ([`euler`]): Hierholzer's (1873, *Ueber die Möglichkeit, einen Linienzug ohne
//!   Wiederholung und ohne Unterbrechung zu umfahren*, Mathematische Annalen 6) walk from the start,
//!   each edge as many times as it is sewn, the closed tours it finds spliced in where they leave the
//!   walk.
//!
//! The planner's own weights then decide where coming back costs more than lifting the needle: a
//! stretch sewn back over longer than a jump is worth ([`COST_HIDDEN`] a millimetre against
//! [`COST_LIFT`], and [`COST_TRIM`] past the trim length) is jumped instead (`super::running`).

use std::collections::BTreeMap;

use vectorcraft_geom::Point;

use super::THREAD_WIDTH;
use super::geometry::{SubPath, length};

/// Two ends of a running line's pieces closer than this meet (millimetres): a thread's width
/// (`super::THREAD_WIDTH`, 0.4 mm for 40 wt), the narrowest gap that can show between two lines
/// sewn in it. A line broken by a picture's noise into pieces that nearly touch is one line. The
/// research's estimate, the session's choice.
pub(super) const GAP_BRIDGE: f64 = THREAD_WIDTH;

/// The grid ends are looked up in ([`Graph::of`]): cells whose diagonal is [`GAP_BRIDGE`], so that
/// any two ends in one cell meet.
const CELL: f64 = GAP_BRIDGE / std::f64::consts::SQRT_2;

/// How many ends of a cell are compared with a neighbouring cell's, at most: a bound on the work at
/// a point where very many pieces end (every end in one cell meets the others anyway). An estimate.
const MAX_CELL_ENDS: usize = 16;

/// The most pieces a line is routed with: past them it is sewn piece by piece, as before routing.
/// Every step here takes time in proportion to the pieces (or their logarithm besides), so this
/// only bounds memory; an estimate, far past a colouring page's few thousand.
pub(super) const MAX_ROUTED_PIECES: usize = 200_000;

/// The most edges [`shorten_cycles`] walks in all: past them the cycles left are not shortened (the
/// route stays right, sewing a little more twice). An estimate: a drawing of a thousand cycles of
/// a thousand edges.
const MAX_CYCLE_WORK: usize = 1_000_000;

/// The nodes a component's walk is cut short at ([`euler`]): a guard, never reached by a graph
/// whose degrees are right, against looping on a wrong one.
const WALK_GUARD: usize = 4;

/// A running line's pieces as a graph: where their ends meet, and the pieces between.
pub(super) struct Graph {
    /// Each node's point: the first end met there, in the pieces' order.
    pub(super) nodes: Vec<Point>,
    pub(super) edges: Vec<Edge>,
}

/// A piece of the line: its subpath (its place in the line's subpaths), the nodes at its first and
/// last points, and its length (millimetres).
#[derive(Clone, Copy, Debug)]
pub(super) struct Edge {
    pub(super) sub: usize,
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) len: f64,
}

/// The root of `i` among `parent`'s groups, the path halved on the way.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let grand = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = grand;
        }
        i = p;
    }
    i
}

/// `a`'s group and `b`'s made one, the lower root kept.
fn join(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra != rb
        && let Some(slot) = parent.get_mut(ra.max(rb))
    {
        *slot = ra.min(rb);
    }
}

/// A point's cell in the grid of [`CELL`]; `None` for a point that isn't finite.
fn cell(p: Point) -> Option<(i64, i64)> {
    let (x, y) = ((p.x / CELL).floor(), (p.y / CELL).floor());
    (x.is_finite() && y.is_finite()).then_some((x as i64, y as i64))
}

impl Graph {
    /// The graph of `subs`' open pieces (closed ones have no ends, and are sewn on their own):
    /// `None` when no two pieces meet, so that the line is sewn as before, piece by piece; and
    /// when it has more than [`MAX_ROUTED_PIECES`].
    pub(super) fn of(subs: &[SubPath]) -> Option<Self> {
        let open: Vec<usize> = (0..subs.len())
            .filter(|&i| {
                subs.get(i).is_some_and(|s| !s.closed && s.pts.len() >= 2 && length(&s.pts, false) > 0.0 && s.pts.iter().all(|q| cell(*q).is_some()))
            })
            .collect();
        if open.len() < 2 || open.len() > MAX_ROUTED_PIECES {
            return None;
        }
        // Each piece's two ends, its first then its last.
        let ends: Vec<Point> =
            open.iter().filter_map(|&i| subs.get(i)).flat_map(|s| [s.pts.first().copied(), s.pts.last().copied()]).flatten().collect();
        if ends.len() != 2 * open.len() {
            return None;
        }
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (k, &q) in ends.iter().enumerate() {
            cells.entry(cell(q)?).or_default().push(k);
        }
        let mut parent: Vec<usize> = (0..ends.len()).collect();
        for (&(cx, cy), here) in &cells {
            // Every end of a cell meets the others.
            if let Some((&first, rest)) = here.split_first() {
                for &k in rest {
                    join(&mut parent, first, k);
                }
            }
            // A neighbouring cell's ends, each pair of cells once.
            for (dx, dy) in [(1, -1), (1, 0), (1, 1), (0, 1)] {
                let Some(there) = cells.get(&(cx + dx, cy + dy)) else { continue };
                for &k in here.iter().take(MAX_CELL_ENDS) {
                    for &m in there.iter().take(MAX_CELL_ENDS) {
                        if let (Some(p), Some(q)) = (ends.get(k), ends.get(m))
                            && p.distance(*q) <= GAP_BRIDGE
                        {
                            join(&mut parent, k, m);
                        }
                    }
                }
            }
        }
        // A node for each group, numbered in the order of its first end.
        let mut node_of: Vec<Option<usize>> = vec![None; ends.len()];
        let mut nodes: Vec<Point> = Vec::new();
        for k in 0..ends.len() {
            let r = root(&mut parent, k);
            if node_of.get(r).copied().flatten().is_none() {
                if let Some(slot) = node_of.get_mut(r) {
                    *slot = Some(nodes.len());
                }
                nodes.push(*ends.get(r)?);
            }
        }
        let mut edges: Vec<Edge> = Vec::with_capacity(open.len());
        for (j, &sub) in open.iter().enumerate() {
            let a = node_of.get(root(&mut parent, 2 * j)).copied().flatten()?;
            let b = node_of.get(root(&mut parent, 2 * j + 1)).copied().flatten()?;
            let len = subs.get(sub).map_or(0.0, |s| length(&s.pts, false));
            edges.push(Edge { sub, a, b, len });
        }
        let graph = Self { nodes, edges };
        // Nothing to route unless two pieces meet.
        graph.components().iter().any(|c| c.len() >= 2).then_some(graph)
    }

    /// The graph's connected parts, each as its edges (their places in [`Graph::edges`]), in the
    /// order of their first edge.
    pub(super) fn components(&self) -> Vec<Vec<usize>> {
        let mut parent: Vec<usize> = (0..self.nodes.len()).collect();
        for e in &self.edges {
            join(&mut parent, e.a, e.b);
        }
        let mut of_root: BTreeMap<usize, usize> = BTreeMap::new();
        let mut out: Vec<Vec<usize>> = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            let r = root(&mut parent, e.a);
            let k = *of_root.entry(r).or_insert_with(|| {
                out.push(Vec::new());
                out.len() - 1
            });
            if let Some(list) = out.get_mut(k) {
                list.push(i);
            }
        }
        out
    }
}

/// One step of a route: an edge (its place in [`Graph::edges`]), whether it is sewn from its first
/// point to its last, and whether it is sewn there again, back in its own holes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) edge: usize,
    pub(super) forward: bool,
    pub(super) again: bool,
}

/// A way through one connected part of a [`Graph`]: its steps, from node `start` to node `end`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Route {
    pub(super) steps: Vec<Step>,
    pub(super) start: usize,
    pub(super) end: usize,
}

/// A connected part's nodes and edges numbered from 0, with each node's edges (each edge once at
/// each of its ends: a loop twice at its node).
struct Part<'a> {
    graph: &'a Graph,
    /// The part's edges (their places in the graph), and their nodes as numbered here.
    edges: Vec<usize>,
    ends: Vec<(usize, usize)>,
    /// The graph's node of each node here.
    nodes: Vec<usize>,
    /// Each node's edges here, with the node at the other end.
    around: Vec<Vec<(usize, usize)>>,
}

impl<'a> Part<'a> {
    fn of(graph: &'a Graph, edges: &[usize]) -> Option<Self> {
        let mut index: BTreeMap<usize, usize> = BTreeMap::new();
        let mut nodes: Vec<usize> = Vec::new();
        let mut local = |n: usize| {
            *index.entry(n).or_insert_with(|| {
                nodes.push(n);
                nodes.len() - 1
            })
        };
        let mut ends: Vec<(usize, usize)> = Vec::with_capacity(edges.len());
        for &e in edges {
            let edge = graph.edges.get(e)?;
            let (a, b) = (local(edge.a), local(edge.b));
            ends.push((a, b));
        }
        let mut around: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nodes.len()];
        for (k, &(a, b)) in ends.iter().enumerate() {
            around.get_mut(a)?.push((k, b));
            around.get_mut(b)?.push((k, a));
        }
        Some(Self { graph, edges: edges.to_vec(), ends, nodes, around })
    }

    fn len(&self, k: usize) -> f64 {
        self.edges.get(k).and_then(|&e| self.graph.edges.get(e)).map_or(0.0, |e| e.len)
    }
}

/// A spanning tree of a part, from node `root`: each node's parent and the edge to it (`None` at the
/// root), and the nodes in the order they were reached (breadth first, by the order of each node's
/// edges).
struct Tree {
    up: Vec<Option<(usize, usize)>>,
    depth: Vec<usize>,
    order: Vec<usize>,
    in_tree: Vec<bool>,
}

fn tree(part: &Part<'_>, root: usize) -> Tree {
    let n = part.nodes.len();
    let mut up: Vec<Option<(usize, usize)>> = vec![None; n];
    let mut depth = vec![0usize; n];
    let mut seen = vec![false; n];
    let mut in_tree = vec![false; part.ends.len()];
    let mut order = Vec::with_capacity(n);
    if let Some(s) = seen.get_mut(root) {
        *s = true;
        order.push(root);
    }
    let mut head = 0;
    while let Some(&v) = order.get(head) {
        head += 1;
        for &(k, w) in part.around.get(v).map_or(&[][..], Vec::as_slice) {
            if seen.get(w).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = seen.get_mut(w) {
                *s = true;
            }
            if let Some(slot) = up.get_mut(w) {
                *slot = Some((v, k));
            }
            let d = depth.get(v).copied().unwrap_or(0) + 1;
            if let Some(slot) = depth.get_mut(w) {
                *slot = d;
            }
            if let Some(t) = in_tree.get_mut(k) {
                *t = true;
            }
            order.push(w);
        }
    }
    Tree { up, depth, order, in_tree }
}

/// Which of a part's nodes an odd number of its edges meet (a loop meets its node twice).
fn odd_nodes(part: &Part<'_>) -> Vec<bool> {
    let mut odd = vec![false; part.nodes.len()];
    for &(a, b) in &part.ends {
        for v in [a, b] {
            if let Some(o) = odd.get_mut(v) {
                *o = !*o;
            }
        }
    }
    odd
}

/// The T-join of a part's spanning tree for T the nodes `odd` marks: the tree's edges whose part of
/// the tree below them holds an odd number of them (the module's notes). As a mark on each of the
/// part's edges.
fn tree_join(part: &Part<'_>, t: &Tree, odd: &[bool]) -> Vec<bool> {
    let mut join = vec![false; part.ends.len()];
    let mut parity: Vec<bool> = odd.to_vec();
    for &v in t.order.iter().rev() {
        let Some(Some((p, k))) = t.up.get(v).copied() else { continue };
        if parity.get(v).copied().unwrap_or(false) {
            if let Some(j) = join.get_mut(k) {
                *j = true;
            }
            if let Some(q) = parity.get_mut(p) {
                *q = !*q;
            }
        }
    }
    join
}

/// The tree's edges on the path between nodes `u` and `v`, walked up from both to where they meet.
fn tree_path(t: &Tree, mut u: usize, mut v: usize) -> Vec<usize> {
    let mut path = Vec::new();
    let depth = |n: usize| t.depth.get(n).copied().unwrap_or(0);
    let step = |n: &mut usize, path: &mut Vec<usize>| -> bool {
        match t.up.get(*n).copied().flatten() {
            Some((p, k)) => {
                path.push(k);
                *n = p;
                true
            }
            None => false,
        }
    };
    while depth(u) > depth(v) {
        if !step(&mut u, &mut path) {
            break;
        }
    }
    while depth(v) > depth(u) {
        if !step(&mut v, &mut path) {
            break;
        }
    }
    while u != v {
        if !(step(&mut u, &mut path) && step(&mut v, &mut path)) {
            break;
        }
    }
    path
}

/// `join` changed on each cycle the tree leaves out where that makes it shorter: the cycle of a
/// non-tree edge and the tree's path between its ends, its edges in the join swapped with those out
/// of it when more than half the cycle's length is in it (the module's notes). At most
/// [`MAX_CYCLE_WORK`] edges walked in all.
fn shorten_cycles(part: &Part<'_>, t: &Tree, join: &mut [bool]) {
    let mut work = 0usize;
    for (k, &(a, b)) in part.ends.iter().enumerate() {
        if t.in_tree.get(k).copied().unwrap_or(true) || a == b {
            continue;
        }
        let mut cycle = tree_path(t, a, b);
        cycle.push(k);
        work = work.saturating_add(cycle.len());
        if work > MAX_CYCLE_WORK {
            return;
        }
        let total: f64 = cycle.iter().map(|&e| part.len(e)).sum();
        let joined: f64 = cycle.iter().filter(|&&e| join.get(e).copied().unwrap_or(false)).map(|&e| part.len(e)).sum();
        if 2.0 * joined > total {
            for &e in &cycle {
                if let Some(j) = join.get_mut(e) {
                    *j = !*j;
                }
            }
        }
    }
}

/// The route's two ends on the tree, `start` and `end` given or free: where taking the path between
/// them out of `join` takes most off (its edges in the join counted for, those out of it against);
/// a free end on its own is the node the given one's heaviest path ends at (the tree is rooted at
/// the given one), and two free ends the tree's heaviest path, found as its longest path is
/// ([`heaviest_path`]). A route that saves nothing by ending elsewhere comes back to where it
/// starts.
fn ends(part: &Part<'_>, t: &Tree, join: &[bool], start: Option<usize>, end: Option<usize>) -> (usize, usize) {
    let gain = |k: usize| if join.get(k).copied().unwrap_or(false) { part.len(k) } else { -part.len(k) };
    match (start, end) {
        (Some(s), Some(e)) => (s, e),
        (Some(fixed), None) | (None, Some(fixed)) => {
            // The heaviest path from the fixed end, the tree's root: each node's weight from it,
            // down the tree.
            let mut weight = vec![0.0f64; part.nodes.len()];
            let mut best = (fixed, 0.0f64);
            for &v in &t.order {
                let Some(Some((p, k))) = t.up.get(v).copied() else { continue };
                let w = weight.get(p).copied().unwrap_or(0.0) + gain(k);
                if let Some(slot) = weight.get_mut(v) {
                    *slot = w;
                }
                if w > best.1 {
                    best = (v, w);
                }
            }
            if start.is_some() { (fixed, best.0) } else { (best.0, fixed) }
        }
        (None, None) => heaviest_path(part, t, &gain).unwrap_or_else(|| {
            let r = t.order.first().copied().unwrap_or(0);
            (r, r)
        }),
    }
}

/// The tree's heaviest path between two nodes, each edge weighing `gain(edge)`: the two heaviest
/// ways down from each node, from the leaves up (as a tree's longest path is found), `None` when no
/// path weighs more than nothing.
fn heaviest_path(part: &Part<'_>, t: &Tree, gain: &dyn Fn(usize) -> f64) -> Option<(usize, usize)> {
    let n = part.nodes.len();
    // The two heaviest ways down through each node's children, each with the node it ends at (the
    // heaviest is the node's own way down).
    let mut top: Vec<[(f64, usize); 2]> = (0..n).map(|v| [(0.0, v), (0.0, v)]).collect();
    for &v in t.order.iter().rev() {
        let (w, at) = top.get(v).map_or((0.0, v), |t| t[0]);
        let Some(Some((p, k))) = t.up.get(v).copied() else { continue };
        let through = (w + gain(k), at);
        if through.0 <= 0.0 {
            continue;
        }
        if let Some(t) = top.get_mut(p) {
            if through.0 > t[0].0 {
                t[1] = t[0];
                t[0] = through;
            } else if through.0 > t[1].0 {
                t[1] = through;
            }
        }
    }
    let mut best: Option<(f64, usize, usize)> = None;
    for v in 0..n {
        let Some([a, b]) = top.get(v).copied() else { continue };
        let w = a.0 + b.0;
        if w > 0.0 && best.is_none_or(|(bw, _, _)| w > bw) {
            best = Some((w, a.1, b.1));
        }
    }
    best.map(|(_, a, b)| (a, b))
}

/// The Euler path from `start` through a part, each edge `times[edge]` times (Hierholzer's walk,
/// the module's notes): its steps, each an edge with the node it is sewn from; `None` when the walk
/// can't take every edge (degrees that don't allow it), which a right join never leaves.
fn euler(part: &Part<'_>, start: usize, times: &[u8]) -> Option<Vec<(usize, usize)>> {
    let total: usize = times.iter().map(|&t| usize::from(t)).sum();
    let mut left: Vec<u8> = times.to_vec();
    let mut next = vec![0usize; part.nodes.len()];
    // The walk so far, as (node, the step that led there).
    let mut walk: Vec<(usize, Option<(usize, usize)>)> = vec![(start, None)];
    let mut path: Vec<(usize, usize)> = Vec::with_capacity(total);
    let guard = total.saturating_mul(WALK_GUARD).saturating_add(part.nodes.len());
    let mut moves = 0usize;
    while let Some(&(v, via)) = walk.last() {
        moves += 1;
        if moves > guard {
            return None;
        }
        let edges = part.around.get(v).map_or(&[][..], Vec::as_slice);
        let cursor = next.get_mut(v)?;
        while edges.get(*cursor).is_some_and(|&(k, _)| left.get(k).copied().unwrap_or(0) == 0) {
            *cursor += 1;
        }
        match edges.get(*cursor) {
            Some(&(k, w)) => {
                if let Some(l) = left.get_mut(k) {
                    *l -= 1;
                }
                walk.push((w, Some((k, v))));
            }
            None => {
                walk.pop();
                if let Some(step) = via {
                    path.push(step);
                }
            }
        }
    }
    path.reverse();
    (path.len() == total).then_some(path)
}

/// The route through the connected part of `graph` made of `edges` from node `start` to node `end`
/// (each the graph's node, or free: [`ends`] picks it): every edge once, and those of the join twice
/// ([`tree_join`], [`shorten_cycles`]), the second time marked [`Step::again`]. `None` when there is
/// no such route (a part with no edge), and then its pieces are sewn as before.
pub(super) fn route(graph: &Graph, edges: &[usize], start: Option<usize>, end: Option<usize>) -> Option<Route> {
    let part = Part::of(graph, edges)?;
    let local = |n: usize| part.nodes.iter().position(|&m| m == n);
    let (s0, e0) = (start.map(local), end.map(local));
    if matches!(s0, Some(None)) || matches!(e0, Some(None)) {
        return None;
    }
    let (s0, e0) = (s0.flatten(), e0.flatten());
    let root = s0.or(e0).unwrap_or(0);
    let t = tree(&part, root);
    let odd = odd_nodes(&part);
    let mut join = tree_join(&part, &t, &odd);
    let (s, e) = ends(&part, &t, &join, s0, e0);
    // The path between the ends taken out of the join leaves them odd: the route ends at them.
    for k in tree_path(&t, s, e) {
        if let Some(j) = join.get_mut(k) {
            *j = !*j;
        }
    }
    // Then shortened on the cycles the tree leaves out, which keeps the same nodes odd.
    shorten_cycles(&part, &t, &mut join);
    let times: Vec<u8> = join.iter().map(|&j| if j { 2 } else { 1 }).collect();
    let walked = euler(&part, s, &times)?;
    let mut sewn = vec![false; part.ends.len()];
    let steps = walked
        .into_iter()
        .filter_map(|(k, from)| {
            let edge = *part.edges.get(k)?;
            let again = std::mem::replace(sewn.get_mut(k)?, true);
            let a = part.ends.get(k)?.0;
            Some(Step { edge, forward: from == a, again })
        })
        .collect();
    Some(Route { steps, start: *part.nodes.get(s)?, end: *part.nodes.get(e)? })
}
