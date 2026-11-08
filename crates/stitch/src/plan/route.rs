//! A running line whose pieces meet end to end, sewn as one route: every piece once, and back along
//! a few of them in their own holes where the needle must return, with no jump and no connecting
//! stitch inside a drawing (the project owner's choices: "branches are sewn once each, going back
//! along a few", 2026-11-01, and „Wszędzie w planiście (polecam)”, 2026-11-07: for every running
//! line, not only those digitizing makes).
//!
//! The pieces are the edges of a graph whose nodes are where their ends meet ([`Graph`]: an end
//! within [`GAP_BRIDGE`] of a node's point, its first end, is that node's, so that two ends that
//! close meet wherever they lie, and no node is wider than the gap either side of its point). A
//! piece whose middle passes within the gap of a node is cut there, so that a line ending on
//! another's middle (a "T" drawn as two strokes, a "K" whose arms end on its stem) joins its route
//! (the project owner's choice, 2026-11-07, „Zrobić teraz”).
//!
//! Sewing every edge and coming back along as few as possible is the route inspection ("Chinese
//! postman") problem: Edmonds and Johnson (1973, *Matching, Euler tours and the Chinese postman*,
//! Mathematical Programming 5, §1–2) set it out, an Euler path needing every node even but its two
//! ends, the pieces sewn twice the copies that make them so: a *T-join* for T the nodes where an
//! odd number of pieces meet, the least one the fewest millimetres sewn twice. Their §3 pairs the
//! odd nodes along shortest paths, by a matching (p. 92). An open route leaves two nodes odd, where
//! it starts and ends (Hierholzer's case (c), below).
//!
//! Written here in a structure of its own, with no matching algorithm:
//! - **The odd nodes paired nearest first** ([`paired`]): a shortest-path search from all of them
//!   at once (Dijkstra's, 1959, *A note on two problems in connexion with graphs*, Numerische
//!   Mathematik 1, with many sources) reaches each node from the odd node nearest it; each piece
//!   between two nodes reached from different odd nodes offers those two a pair, as long as the way
//!   through it, and the pairs are taken shortest first, each odd node once (the matching of
//!   Edmonds and Johnson's §3 done greedily). The odd nodes left unpaired are paired so among
//!   themselves in turn, and, past [`PAIR_ROUNDS`], along a spanning tree. The pieces of each pair's
//!   way are sewn twice, a piece two ways pass not at all.
//! - **The route's two ends** ([`ends`]): taking the path between two nodes out of the join leaves
//!   those two odd, so the route ends there; the path that takes most off (its pieces in the join
//!   counted for, those out of it against) is found on a spanning tree as its heaviest path, the
//!   way a tree's longest path is, from an end asked for (an entry or an exit) when one is. On a
//!   tree, whose join is the only one, that is the least route: every piece twice but those of the
//!   path between its ends.
//! - **Each cycle sewn twice more than half way made shorter** ([`shorten_cycles`]): the pieces sewn
//!   twice on a cycle (one a spanning tree leaves out, with the tree's path between its ends)
//!   swapped with those sewn once leave the same nodes odd, since every node of a cycle meets it an
//!   even number of times; swapped where more than half of the cycle's length is sewn twice (the
//!   condition Kwan Mei-Ko's least route meets on every cycle: 1962, *Graphic programming using odd
//!   or even points*, Chinese Mathematics 1), again until none is, within [`MAX_CYCLE_WORK`].
//! - **The Euler path** ([`euler`]): Hierholzer's (1873, *Ueber die Möglichkeit, einen Linienzug ohne
//!   Wiederholung und ohne Unterbrechung zu umfahren*, Mathematische Annalen 6) walk from the start,
//!   each edge as many times as it is sewn, the closed tours it finds spliced in where they leave the
//!   walk; an open one runs between the two odd nodes (his case (c), p. 31).
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

/// The grid the nodes' points are looked up in ([`Graph::of`], [`Cells`]): cells at least
/// [`GAP_BRIDGE`] wide, so that a node an end meets lies in the end's cell or one beside it,
/// corner to corner at most. Two nodes' points lie farther apart than the gap, so a cell holds a
/// few at most.
const CELL: f64 = GAP_BRIDGE;

/// The cells the search for nodes on the pieces' middles walks along them ([`cuts`]): at least four
/// times the ends' cells, so that it takes a quarter of the steps, and at least twice the gap, so
/// that a node within the gap of any point of a piece lies in the 3 × 3 cells round the step
/// nearest that point (half a cell along the piece and the gap across it are within one cell
/// either way).
const CUT_CELL: f64 = 4.0 * CELL;

/// How many steps along the pieces, and nodes looked at, the search for nodes on their middles
/// takes at most ([`cuts`]): one step a cell along every piece. Past them no piece is cut, the
/// pieces meeting end to end only. A bound on the work, an estimate: two million, the cells along
/// 3.2 km of line.
const MAX_CUT_STEPS: usize = 2_000_000;

/// The most pieces a line is routed with: past them it is sewn piece by piece, as before routing.
/// Every step here takes time in proportion to the pieces (or their logarithm besides), so this
/// only bounds memory; an estimate, far past a colouring page's few thousand.
pub(super) const MAX_ROUTED_PIECES: usize = 200_000;

/// The most edges [`shorten_cycles`] walks in all: past them the cycles left are not shortened (the
/// route stays right, sewing a little more twice). An estimate: a drawing of a thousand cycles of
/// a thousand edges.
const MAX_CYCLE_WORK: usize = 1_000_000;

/// How many times the odd nodes left unpaired are paired among themselves ([`paired`]) before the
/// rest are paired along a spanning tree: each time pairs some, the nodes left fewer, a handful in
/// a drawing (an estimate, a bound on the work: each time is one search over the drawing).
const PAIR_ROUNDS: usize = 8;

/// The nodes a component's walk is cut short at ([`euler`]): a guard, never reached by a graph
/// whose degrees are right, against looping on a wrong one.
const WALK_GUARD: usize = 4;

/// A running line's pieces as a graph: where their ends meet, and the pieces between.
pub(super) struct Graph {
    /// Each node's point: the first end met there, in the pieces' order.
    pub(super) nodes: Vec<Point>,
    pub(super) edges: Vec<Edge>,
}

/// A piece of the line: the subpath it is (or is cut from: its place in the line's subpaths), the
/// nodes at its first and last points, its length (millimetres) and its points.
#[derive(Clone, Debug)]
pub(super) struct Edge {
    pub(super) sub: usize,
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) len: f64,
    pub(super) pts: Vec<Point>,
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

/// The most cells a [`Cells`] grid takes, a bound on its memory (four bytes a cell): past it, its
/// cells are wider than asked. An estimate: a thousand a side.
const MAX_CELLS: f64 = 1_048_576.0;

/// No point: the end of a cell's list ([`Cells`]).
const NO_POINT: u32 = u32::MAX;

/// Points sorted into square cells over a box, to find those near a point: each cell's points a
/// list, from its last point added (`head`) through each point's next (`next`), so that points can
/// be added one at a time as they are found. The cells are at least as wide as asked, wider where
/// the box would need more than [`MAX_CELLS`]: a point within a cell's width of another, either
/// way along each axis, lies in its cell or one beside it.
struct Cells {
    x0: f64,
    y0: f64,
    side: f64,
    nx: usize,
    ny: usize,
    head: Vec<u32>,
    next: Vec<u32>,
}

impl Cells {
    /// Empty cells at least `side` wide over the box of `pts` (millimetres); `None` for no point,
    /// or one that isn't finite.
    fn over(pts: impl Iterator<Item = Point>, side: f64) -> Option<Self> {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in pts {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        let (w, h) = (x1 - x0, y1 - y0);
        if !(w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0 && side > 0.0) {
            return None;
        }
        let side = side.max(((w + side) * (h + side) / MAX_CELLS).sqrt());
        let (nx, ny) = ((w / side).floor() + 1.0, (h / side).floor() + 1.0);
        // A guard: the side chosen keeps them within MAX_CELLS and a row and a column besides (the
        // box and the side are finite numbers here).
        if nx * ny > 2.0 * MAX_CELLS {
            return None;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        Some(Self { x0, y0, side, nx, ny, head: vec![NO_POINT; nx * ny], next: Vec::new() })
    }

    /// Where `p` lies in the grid, in cells from its first (beyond the box too); `None` for a point
    /// that isn't finite.
    fn cell(&self, p: Point) -> Option<(i64, i64)> {
        let (x, y) = (((p.x - self.x0) / self.side).floor(), ((p.y - self.y0) / self.side).floor());
        (x.is_finite() && y.is_finite()).then_some((x as i64, y as i64))
    }

    /// The cell at `(cx, cy)`'s place in `head`, `None` outside the grid.
    fn at(&self, cx: i64, cy: i64) -> Option<usize> {
        let (x, y) = (usize::try_from(cx).ok()?, usize::try_from(cy).ok()?);
        (x < self.nx && y < self.ny).then(|| y * self.nx + x)
    }

    /// Point `p` added, numbered as the points added before it are counted; `false` when it lies
    /// outside the grid (or the count is past what the lists hold).
    fn add(&mut self, p: Point) -> bool {
        let Some(c) = self.cell(p).and_then(|(cx, cy)| self.at(cx, cy)) else { return false };
        let (Ok(id), Some(slot)) = (u32::try_from(self.next.len()), self.head.get_mut(c)) else { return false };
        if id == NO_POINT {
            return false;
        }
        self.next.push(*slot);
        *slot = id;
        true
    }

    /// `visit` called with every point added in the 3 × 3 cells round `p`'s.
    fn near(&self, p: Point, mut visit: impl FnMut(usize)) {
        let Some((cx, cy)) = self.cell(p) else { return };
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (Some(x), Some(y)) = (cx.checked_add(dx), cy.checked_add(dy)) else { continue };
                let Some(c) = self.at(x, y) else { continue };
                let mut k = self.head.get(c).copied().unwrap_or(NO_POINT);
                while k != NO_POINT {
                    visit(k as usize);
                    k = self.next.get(k as usize).copied().unwrap_or(NO_POINT);
                }
            }
        }
    }
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
        // Each end in turn meets the node whose point lies nearest it within the gap, or is a new
        // node's point: the nodes' points are each farther than the gap from the others.
        let mut cells = Cells::over(ends.iter().copied(), CELL)?;
        let mut nodes: Vec<Point> = Vec::new();
        let mut node_of: Vec<usize> = Vec::with_capacity(ends.len());
        for &q in &ends {
            let mut met: Option<(f64, usize)> = None;
            cells.near(q, |n| {
                let d = nodes.get(n).map_or(f64::INFINITY, |p| p.distance(q));
                if d <= GAP_BRIDGE && met.is_none_or(|(best, m)| d < best || (d == best && n < m)) {
                    met = Some((d, n));
                }
            });
            let n = match met {
                Some((_, n)) => n,
                None => {
                    if !cells.add(q) {
                        return None;
                    }
                    nodes.push(q);
                    nodes.len() - 1
                }
            };
            node_of.push(n);
        }
        // Each piece, cut where a node lies on its middle.
        let cut = cuts(subs, &open, &nodes);
        let mut edges: Vec<Edge> = Vec::with_capacity(open.len());
        for (j, &sub) in open.iter().enumerate() {
            let (a, b) = (node_of.get(2 * j).copied()?, node_of.get(2 * j + 1).copied()?);
            let pts = &subs.get(sub)?.pts;
            let mut from = (a, 0usize, *pts.first()?);
            for &(seg, at, n) in cut.get(j).map_or(&[][..], Vec::as_slice) {
                let mut part = vec![from.2];
                part.extend(pts.get(from.1 + 1..=seg).unwrap_or(&[]));
                part.push(at);
                edges.push(Edge { sub, a: from.0, b: n, len: length(&part, false), pts: part });
                from = (n, seg, at);
            }
            let mut part = vec![from.2];
            part.extend(pts.get(from.1 + 1..).unwrap_or(&[]));
            edges.push(Edge { sub, a: from.0, b, len: length(&part, false), pts: part });
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

/// Where each of the line's pieces (`subs`, the open ones at `open`'s places) is cut: at each node
/// (`nodes`) that lies within [`GAP_BRIDGE`] of its middle, farther than that along it from its own
/// ends, nearest there, the nodes looked up in the cells (at least [`CUT_CELL`] wide) round each
/// step along the piece, a cell apart. By piece (its place in `open`), each cut's segment, its point
/// and its node, in order along the piece. None at all past [`MAX_CUT_STEPS`] steps and nodes
/// looked at.
fn cuts(subs: &[SubPath], open: &[usize], nodes: &[Point]) -> Vec<Vec<(usize, Point, usize)>> {
    let none = || vec![Vec::new(); open.len()];
    let Some(mut cells) = Cells::over(nodes.iter().copied(), CUT_CELL) else { return none() };
    for &q in nodes {
        if !cells.add(q) {
            return none();
        }
    }
    let mut steps = 0usize;
    let mut out: Vec<Vec<(usize, Point, usize)>> = Vec::with_capacity(open.len());
    for &i in open {
        let Some(pts) = subs.get(i).map(|s| &s.pts) else {
            out.push(Vec::new());
            continue;
        };
        let total = length(pts, false);
        // The nearest point of the piece to each node within the gap of it: its distance, where it
        // lies along the piece, its segment and the point.
        let mut found: BTreeMap<usize, (f64, f64, usize, Point)> = BTreeMap::new();
        let mut along = 0.0;
        for (seg, w) in pts.windows(2).enumerate() {
            let (Some(&p0), Some(&p1)) = (w.first(), w.get(1)) else { continue };
            let len = p0.distance(p1);
            let samples = (len / cells.side).ceil().max(1.0);
            if !samples.is_finite() {
                return none();
            }
            for k in 0..=samples as usize {
                steps += 1;
                cells.near(p0 + (p1 - p0) * (k as f64 / samples), |n| {
                    steps += 1;
                    let Some(&q) = nodes.get(n) else { return };
                    let ab = p1 - p0;
                    let t = if ab.hypot2() > 0.0 { ((q - p0).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
                    let at = p0 + ab * t;
                    let (d, s) = (at.distance(q), along + len * t);
                    let middle = s > GAP_BRIDGE && total - s > GAP_BRIDGE;
                    if d <= GAP_BRIDGE && middle && found.get(&n).is_none_or(|&(best, ..)| d < best) {
                        found.insert(n, (d, s, seg, at));
                    }
                });
                if steps > MAX_CUT_STEPS {
                    return none();
                }
            }
            along += len;
        }
        let mut here: Vec<(f64, usize, Point, usize)> = found.into_iter().map(|(n, (_, s, seg, at))| (s, seg, at, n)).collect();
        here.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.3.cmp(&b.3)));
        out.push(here.into_iter().map(|(_, seg, at, n)| (seg, at, n)).collect());
    }
    out
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
/// of it when more than half the cycle's length is in it (the module's notes), passes over every
/// cycle again until one changes none, or `stopped` says so. At most [`MAX_CYCLE_WORK`] edges walked
/// in all.
fn shorten_cycles(part: &Part<'_>, t: &Tree, join: &mut [bool], stopped: &dyn Fn() -> bool) {
    let mut work = 0usize;
    loop {
        if stopped() {
            return;
        }
        let mut swapped = false;
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
                swapped = true;
            }
        }
        if !swapped {
            return;
        }
    }
}

/// What a shortest-path search from many nodes at once reached ([`reached`]): each node's
/// distance, the source it is nearest and the edge it was reached by, with the node before.
struct Reached {
    dist: Vec<f64>,
    source: Vec<Option<usize>>,
    via: Vec<Option<(usize, usize)>>,
}

/// Dijkstra's search over a part from `sources` at once: each node reached from the source nearest
/// it, ties to the first reached.
fn reached(part: &Part<'_>, sources: &[usize]) -> Reached {
    use std::cmp::Reverse;
    let n = part.nodes.len();
    let mut r = Reached { dist: vec![f64::INFINITY; n], source: vec![None; n], via: vec![None; n] };
    // The heap holds each distance by its bits (distances are never negative, so their bits sort
    // as they do).
    let mut heap: std::collections::BinaryHeap<Reverse<(u64, usize)>> = std::collections::BinaryHeap::new();
    for &v in sources {
        if let (Some(d), Some(src)) = (r.dist.get_mut(v), r.source.get_mut(v)) {
            *d = 0.0;
            *src = Some(v);
            heap.push(Reverse((0.0f64.to_bits(), v)));
        }
    }
    while let Some(Reverse((bits, v))) = heap.pop() {
        let d = f64::from_bits(bits);
        if r.dist.get(v).is_none_or(|&best| d > best) {
            continue;
        }
        let from = r.source.get(v).copied().flatten();
        for &(k, w) in part.around.get(v).map_or(&[][..], Vec::as_slice) {
            let next = d + part.len(k);
            if r.dist.get(w).is_some_and(|&best| next < best) {
                if let Some(slot) = r.dist.get_mut(w) {
                    *slot = next;
                }
                if let Some(slot) = r.source.get_mut(w) {
                    *slot = from;
                }
                if let Some(slot) = r.via.get_mut(w) {
                    *slot = Some((k, v));
                }
                heap.push(Reverse((next.to_bits(), w)));
            }
        }
    }
    r
}

/// The edges from node `v` back to the source the search reached it from.
fn way_back(r: &Reached, mut v: usize) -> Vec<usize> {
    let mut path = Vec::new();
    while let Some(Some((k, u))) = r.via.get(v).copied() {
        path.push(k);
        v = u;
        if path.len() > r.via.len() {
            break;
        }
    }
    path
}

/// A pair of odd nodes: the two, the length of the way between them and its edges.
struct Pair {
    nodes: (usize, usize),
    len: f64,
    way: Vec<usize>,
}

/// One round of pairing the nodes `left` of a part ([`paired`]): the pairs each piece between two of
/// their regions offers, shortest first, each node once; `left` keeps those not paired.
fn pair_round(part: &Part<'_>, left: &mut Vec<usize>, pairs: &mut Vec<Pair>) {
    let r = reached(part, left);
    let dist = |v: usize| r.dist.get(v).copied().unwrap_or(f64::INFINITY);
    let source = |v: usize| r.source.get(v).copied().flatten();
    // Each piece between nodes reached from two of them offers those two a pair.
    let mut offers: Vec<(f64, usize)> = Vec::new();
    for (k, &(a, b)) in part.ends.iter().enumerate() {
        if let (Some(sa), Some(sb)) = (source(a), source(b))
            && sa != sb
        {
            offers.push((dist(a) + part.len(k) + dist(b), k));
        }
    }
    offers.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    let mut taken = vec![false; part.nodes.len()];
    for (len, k) in offers {
        let Some(&(a, b)) = part.ends.get(k) else { continue };
        let (Some(sa), Some(sb)) = (source(a), source(b)) else { continue };
        if taken.get(sa).copied().unwrap_or(true) || taken.get(sb).copied().unwrap_or(true) {
            continue;
        }
        for v in [sa, sb] {
            if let Some(x) = taken.get_mut(v) {
                *x = true;
            }
        }
        let mut way = way_back(&r, a);
        way.push(k);
        way.extend(way_back(&r, b));
        pairs.push(Pair { nodes: (sa, sb), len, way });
    }
    left.retain(|&v| !taken.get(v).copied().unwrap_or(false));
}

/// The odd nodes `odd` marks of a part paired along shortest ways, nearest first (the module's
/// notes), those left past [`PAIR_ROUNDS`] (or once `stopped` says so) paired in turn along the
/// spanning tree `t`; and the one left over when they are odd in number.
fn paired(part: &Part<'_>, t: &Tree, odd: &[bool], stopped: &dyn Fn() -> bool) -> (Vec<Pair>, Option<usize>) {
    let mut left: Vec<usize> = (0..part.nodes.len()).filter(|&v| odd.get(v).copied().unwrap_or(false)).collect();
    let mut pairs: Vec<Pair> = Vec::new();
    for _ in 0..PAIR_ROUNDS {
        if left.len() < 2 || stopped() {
            break;
        }
        pair_round(part, &mut left, &mut pairs);
    }
    let one = if left.len() % 2 == 1 { left.pop() } else { None };
    for &[a, b] in left.as_chunks::<2>().0 {
        let way = tree_path(t, a, b);
        let len = way.iter().map(|&k| part.len(k)).sum();
        pairs.push(Pair { nodes: (a, b), len, way });
    }
    (pairs, one)
}

/// The edges an odd number of `pairs`' ways pass, as a mark on each of a part's `edges`: the
/// T-join the pairs make.
fn join_of(pairs: &[Pair], edges: usize) -> Vec<bool> {
    let mut join = vec![false; edges];
    for &k in pairs.iter().flat_map(|p| &p.way) {
        if let Some(j) = join.get_mut(k) {
            *j = !*j;
        }
    }
    join
}

/// How long the edges `join` marks are, in all.
fn join_len(part: &Part<'_>, join: &[bool]) -> f64 {
    join.iter().enumerate().filter(|&(_, &j)| j).map(|(k, _)| part.len(k)).sum()
}

/// The route's ends and its join (the module's notes), by the better of two ways: the odd nodes
/// all paired and the ends on the spanning tree's heaviest path ([`ends`]); or the ends left out of
/// the pairing, both free the two of the longest pair, one asked for paired with what the pairing
/// leaves over. Each shortened on its cycles; the one that sews less twice, the tree's on a tie.
fn ends_and_join(part: &Part<'_>, t: &Tree, start: Option<usize>, end: Option<usize>, stopped: &dyn Fn() -> bool) -> (usize, usize, Vec<bool>) {
    let odd = odd_nodes(part);
    let (mut pairs, _) = paired(part, t, &odd, stopped);
    let all = join_of(&pairs, part.ends.len());
    let (s, e) = ends(part, t, &all, start, end);
    let mut by_tree = all.clone();
    for k in tree_path(t, s, e) {
        if let Some(j) = by_tree.get_mut(k) {
            *j = !*j;
        }
    }
    shorten_cycles(part, t, &mut by_tree, stopped);
    let left_out = match (start, end) {
        (None, None) => {
            let longest = pairs.iter().enumerate().max_by(|a, b| a.1.len.total_cmp(&b.1.len).then(b.0.cmp(&a.0))).map(|(i, _)| i);
            longest.map(|i| pairs.swap_remove(i)).map(|p| (p.nodes.0, p.nodes.1, join_of(&pairs, part.ends.len())))
        }
        _ => {
            // The ends asked for counted odd once more, the other end what the pairing leaves.
            let mut asked = odd;
            for v in [start, end].into_iter().flatten() {
                if let Some(o) = asked.get_mut(v) {
                    *o = !*o;
                }
            }
            let (pairs, one) = paired(part, t, &asked, stopped);
            let join = join_of(&pairs, part.ends.len());
            match (start, end) {
                (Some(a), Some(b)) => Some((a, b, join)),
                (Some(a), None) => one.map(|b| (a, b, join)),
                (None, Some(b)) => one.map(|a| (a, b, join)),
                (None, None) => None,
            }
        }
    };
    match left_out {
        Some((a, b, mut join)) => {
            shorten_cycles(part, t, &mut join, stopped);
            if join_len(part, &join) < join_len(part, &by_tree) { (a, b, join) } else { (s, e, by_tree) }
        }
        None => (s, e, by_tree),
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
/// (each the graph's node, or free: [`ends_and_join`] picks it): every edge once, and those of the
/// join twice ([`paired`], [`shorten_cycles`]), the second time marked [`Step::again`]. `None` when
/// there is no such route (a part with no edge), and then its pieces are sewn as before; and once
/// `stopped`, asked between the pairing's rounds and the cycles' passes, says to stop.
pub(super) fn route(graph: &Graph, edges: &[usize], start: Option<usize>, end: Option<usize>, stopped: &dyn Fn() -> bool) -> Option<Route> {
    let part = Part::of(graph, edges)?;
    let local = |n: usize| part.nodes.iter().position(|&m| m == n);
    let (s0, e0) = (start.map(local), end.map(local));
    if matches!(s0, Some(None)) || matches!(e0, Some(None)) {
        return None;
    }
    let (s0, e0) = (s0.flatten(), e0.flatten());
    let root = s0.or(e0).unwrap_or(0);
    let t = tree(&part, root);
    let (s, e, join) = ends_and_join(&part, &t, s0, e0, stopped);
    if stopped() {
        return None;
    }
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
