//! Centre lines: a thin line's middle, as the path a running stitch follows (the project owner's
//! choices, 2026-11-01: "centre lines are only for lines under 1 mm, sewn as running stitch",
//! "branches are sewn once each, going back along a few"). An area the rules found to be a line
//! (narrower than a satin column, long along itself: `super::rules`) is made into a stroke along its
//! middle rather than a fill, so that the planner sews it as one running line through its junctions
//! (`vectorcraft_stitch::plan`'s routes) instead of a running line per arm with a jump at each.
//!
//! Written here from the papers, in a structure of its own:
//! - **The skeleton, by two-subiteration parallel thinning** (Guo and Hall, 1989, *Parallel thinning
//!   with two-subiteration algorithms*, Communications of the ACM 32(3), 359–373, algorithm A1): the
//!   line's pixels are peeled from its edge in passes, each pass in two halves that take away, all
//!   at once, the edge pixels whose removal keeps the line in one piece and its ends where they are,
//!   from one side and then the other ([`peeled`]); what is left is about one pixel wide along the
//!   middle, its branches and loops kept, a band two pixels wide thinned to one rather than worn
//!   away from its tip.
//! - **The stairs left taken away, by the connectivity number** of Yokoi, Toriwaki and Fukumura
//!   (1975, *An analysis of topological properties of digitized binary pictures using local
//!   features*, Computer Graphics and Image Processing 4(1), 63–73): a pixel whose removal neither
//!   parts the line nor opens a hole (its number one, [`connectivity`]) and that is no end goes, so
//!   that a diagonal is one pixel wide and no stair corner looks like a junction.
//! - **The skeleton as a graph**: its pixels with one neighbour are ends, those with three or more
//!   (side by side, one node) junctions, and the runs of pixels between them its pieces; a run with
//!   no end or junction is a loop.
//! - **Spurs pruned** as the medial axis's are (Blum, 1967, *A transformation for extracting new
//!   descriptors of shape*; Shaked and Bruckstein, 1998, *Pruning medial axes*, CVIU 69(2)): a piece
//!   from a junction to an end that reaches hardly past the largest disc the line holds at the
//!   junction draws nothing the disc doesn't ([`SPUR_SLACK_PX`], [`SPUR_SHARE`]); two junctions
//!   whose discs overlap are one (a crossing thinning splits in two).
//! - **The ends carried to the ink**: thinning stops short of a line's end by about its half width;
//!   each end goes on along its piece to where the ink ends, less half the line's width, so that the
//!   stroke made, round at its ends, ends where the line does.
//! - **The pieces made smooth** by Visvalingam and Whyatt's line generalisation (1993, *Line
//!   generalisation by repeated elimination of points*, The Cartographic Journal 30(1)): the pixels'
//!   stairs taken away, the point whose triangle with its neighbours has the least area removed in
//!   turn until none is smaller than [`SIMPLIFY_AREA_PX2`] ([`smoothed`]).
//!
//! Every step takes time in proportion to the line's pixels (thinning looks again only beside what
//! it took away), or their logarithm besides (the smoothing's heap); a cancel stops it between
//! lines and within one every [`CANCEL_EVERY`] pixels looked at.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use vectorcraft_geom::Point;

use super::regions::{NONE, Regions, distances};
use crate::Result;

/// How many passes of thinning a line takes at most: each peels a pixel off every side, so a line
/// is thinned in half its width in pixels; a thousand peels an area two hundred millimetres wide at
/// ten pixels a millimetre, more than any design holds. A bound on the work.
const MAX_THIN_PASSES: usize = 1_000;

/// How many times the skeleton is gone over at the end for stair pixels, at most: each pass takes
/// those it can, and one or two leave none. An estimate, a bound on the work.
const MAX_CLEANUP_PASSES: usize = 8;

/// How far past the largest disc at its junction a spur may reach and be pruned (working pixels): a
/// pixel and a half, a pixel's diagonal and a little, what a bump of anti-aliasing on the line's
/// edge and the thinning's own pixel make. An estimate, the session's choice.
pub(super) const SPUR_SLACK_PX: f64 = 1.5;

/// The share of the junction's disc a spur may reach past it and be pruned, on a wider line: half,
/// past the (√2 − 1) ≈ 0.41 of it that the corners of a line's square end reach. An estimate, the
/// session's choice.
pub(super) const SPUR_SHARE: f64 = 0.5;

/// The least area a point's triangle with its neighbours keeps it by (square working pixels): 8, so
/// that the stairs pixels leave along a gentle curve go and a corner whose arms reach 0.4 mm stays
/// (at 10 pixels a millimetre). An estimate, measured on the line art of the tests (2026-11-07): at
/// 1 the centre lines of a ring 29 mm across and of an S curve turned more sharply than the
/// planner's corners (30°) at 245 and 145 of their points, each a needle point of its own, and were
/// sewn with 1.9 and 1.8 times the vector path's needle points; at 8 they turn so at none but where
/// a closed piece starts, and are sewn with as many. A point taken away lies about a pixel off the
/// line kept, along a ring that size (a triangle of 8 square pixels on a base of 13 is 1.2 high).
pub(super) const SIMPLIFY_AREA_PX2: f64 = 8.0;

/// How many pixels are thinned between two cancel checks.
const CANCEL_EVERY: usize = 1 << 14;

/// The half of a pixel from its corner to its centre: a pixel `(x, y)` lies at `(x + ½, y + ½)`.
const HALF_PIXEL: f64 = 0.5;

/// How far an end is followed back along its piece to find which way it points: at least this
/// many pixels, and at least [`END_DIRECTION_WIDTHS`] of the line's widths, past the stair thinning
/// leaves at a line's tip.
const END_DIRECTION_PX: usize = 4;
const END_DIRECTION_WIDTHS: f64 = 2.0;

/// The steps an end is carried along toward the ink's end (pixels).
const END_STEP_PX: f64 = 0.5;

/// How far an end may be carried, as a share of the line's width: the ink ends within about half a
/// width of where thinning stopped; twice the width bounds a slanted end.
const END_REACH_SHARE: f64 = 2.0;

/// The eight neighbours of a pixel in turn round it, as Yokoi's connectivity number counts them:
/// east, then counter-clockwise as seen on the page (north is up, `y − 1`).
const AROUND: [(i64, i64); 8] = [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];

/// A centre line: its pieces (working pixels, through the pixels' centres), each open from an end
/// or a junction to another, or a closed loop; its width and its length (working pixels), and its
/// ends and junctions counted.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CentreLine {
    pub(super) pieces: Vec<Piece>,
    pub(super) width_px: f64,
    pub(super) length_px: f64,
    pub(super) ends: usize,
    pub(super) junctions: usize,
}

/// One piece of a centre line.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    pub(super) pts: Vec<Point>,
    pub(super) closed: bool,
}

impl CentreLine {
    /// Its pieces as one path (working pixels): each a subpath, a loop closed.
    pub(super) fn path(&self) -> vectorcraft_geom::BezPath {
        let mut bp = vectorcraft_geom::BezPath::new();
        for piece in &self.pieces {
            let Some((&first, rest)) = piece.pts.split_first() else { continue };
            bp.move_to(first);
            for &q in rest {
                bp.line_to(q);
            }
            if piece.closed {
                bp.close_path();
            }
        }
        bp
    }
}

/// The connectivity number of a pixel whose eight neighbours (in [`AROUND`]'s order) `set` says
/// are the line's: of the four side neighbours, how many are off with the corner after them and the
/// side after that not both on (Yokoi, Toriwaki and Fukumura's eight-connected number, each term
/// `x̄ₖ − x̄ₖ·x̄ₖ₊₁·x̄ₖ₊₂` with `x̄` a neighbour off). One: taking the pixel away keeps the line in one
/// piece and opens no hole; none: it lies inside; two or more: it joins pieces.
fn connectivity(set: [bool; 8]) -> u32 {
    let off = |k: usize| u32::from(!set[k % 8]);
    [0usize, 2, 4, 6].iter().map(|&k| off(k) - off(k) * off(k + 1) * off(k + 2)).sum()
}

/// One line's pixels and the map they lie in: which of its pixels are left as it is thinned.
struct Line<'a> {
    regions: &'a Regions,
    label: u32,
    alive: &'a mut [bool],
}

impl Line<'_> {
    /// Pixel `i`'s neighbour `k` ([`AROUND`]), when it lies in the map.
    fn neighbour(&self, i: usize, k: usize) -> Option<usize> {
        let (w, h) = (i64::from(self.regions.w), i64::from(self.regions.h));
        let (x, y) = ((i as i64) % w, (i as i64) / w);
        let (dx, dy) = AROUND.get(k).copied()?;
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && nx < w && ny < h).then(|| (ny * w + nx) as usize)
    }

    /// Whether pixel `j` is the line's and still there.
    fn on(&self, j: usize) -> bool {
        self.alive.get(j).copied().unwrap_or(false) && self.regions.labels.get(j) == Some(&self.label)
    }

    /// Which of pixel `i`'s eight neighbours are the line's.
    fn around(&self, i: usize) -> [bool; 8] {
        std::array::from_fn(|k| self.neighbour(i, k).is_some_and(|j| self.on(j)))
    }

    /// Whether pixel `i` may be taken away: not an end (it has two neighbours or more) and simple.
    fn removable(&self, i: usize) -> bool {
        let set = self.around(i);
        set.iter().filter(|&&s| s).count() >= 2 && connectivity(set) == 1
    }

    fn take(&mut self, i: usize) {
        if let Some(a) = self.alive.get_mut(i) {
            *a = false;
        }
    }
}

/// Whether a pixel whose eight neighbours (in [`AROUND`]'s order) `set` says are the line's is
/// taken away in half `second` of a pass (Guo and Hall's algorithm A1, the module's notes): its
/// neighbours form one piece (their `C(p)` one: of the four side neighbours, those off with the
/// corner or the side after them on), it has two or three neighbours by both of the paper's counts
/// (`N(p)`, so that an end and a pixel inside stay), and it lies on the side the half takes: the
/// first half the east and north-east edge (`(x₂ ∨ x₃ ∨ ¬x₈) ∧ x₁` false), the second the west and
/// south-west.
fn peeled(set: [bool; 8], second: bool) -> bool {
    // The paper counts x₁ to x₈ round the pixel from the east, as [`AROUND`] does.
    let x = |k: usize| set[(k + 7) % 8];
    let c = (1..=4).filter(|&i| !x(2 * i - 1) && (x(2 * i) || x(2 * i + 1))).count();
    let n1 = (1..=4).filter(|&k| x(2 * k - 1) || x(2 * k)).count();
    let n2 = (1..=4).filter(|&k| x(2 * k) || x(2 * k + 1)).count();
    let n = n1.min(n2);
    let side = if second { (x(6) || x(7) || !x(4)) && x(5) } else { (x(2) || x(3) || !x(8)) && x(1) };
    c == 1 && (2..=3).contains(&n) && !side
}

/// `pixels` (one line's, each `on` in `line`) thinned (the module's notes): passes of two halves,
/// each half's pixels found from the line as it was and taken away together, the pixels beside
/// those taken looked at again in the next, until a pass takes none; then the stairs left taken away.
/// A cancel stops it.
fn thinned(line: &mut Line<'_>, pixels: &[usize]) -> Result<()> {
    // The pixels to look at: at first every one on the line's edge, then those beside the pixels
    // taken away; each once a half.
    let mut todo: Vec<usize> = pixels.iter().copied().filter(|&i| line.around(i).iter().any(|&s| !s)).collect();
    let mut queued = std::collections::BTreeSet::new();
    let mut done = 0usize;
    for _ in 0..MAX_THIN_PASSES {
        let mut taken_any = false;
        for second in [false, true] {
            let taken: Vec<usize> = todo.iter().copied().filter(|&i| line.on(i) && peeled(line.around(i), second)).collect();
            for &i in &taken {
                line.take(i);
            }
            taken_any |= !taken.is_empty();
            done += todo.len();
            // The next half looks again at what it looked at and is still there (the other half
            // may take it), and at what lies beside what went (the edge moved in).
            queued.clear();
            let mut next: Vec<usize> = Vec::with_capacity(todo.len());
            let beside: Vec<usize> = taken.iter().flat_map(|&i| (0..8).map(move |k| (i, k))).filter_map(|(i, k)| line.neighbour(i, k)).collect();
            for j in todo.iter().copied().chain(beside) {
                if line.on(j) && queued.insert(j) {
                    next.push(j);
                }
            }
            todo = next;
            if done >= CANCEL_EVERY {
                done = 0;
                vectorcraft_doc::cancel::check()?;
            }
        }
        if !taken_any {
            break;
        }
    }
    for _ in 0..MAX_CLEANUP_PASSES {
        let mut removed = false;
        for &i in pixels {
            if line.on(i) && line.removable(i) {
                line.take(i);
                removed = true;
            }
        }
        if !removed {
            break;
        }
    }
    Ok(())
}

/// A node of a skeleton's graph: its point (working pixels), the radius of the largest disc the line
/// holds there (the most of its pixels'), and whether it is a junction.
#[derive(Clone, Copy, Debug)]
struct Node {
    at: Point,
    radius: f64,
    junction: bool,
}

/// A piece of a skeleton's graph: its two nodes and its pixels in order, from the first's to the
/// second's (a loop's first pixel again at its end).
#[derive(Clone, Debug)]
struct Edge {
    a: usize,
    b: usize,
    pixels: Vec<usize>,
    gone: bool,
}

/// The point of pixel `i`'s centre in a map `w` wide.
fn centre_of(i: usize, w: usize) -> Point {
    Point::new((i % w) as f64 + HALF_PIXEL, (i / w) as f64 + HALF_PIXEL)
}

/// The length of polyline `pts` (working pixels).
fn length(pts: &[Point]) -> f64 {
    pts.windows(2).map(|w| w.first().zip(w.get(1)).map_or(0.0, |(a, b)| a.distance(*b))).sum()
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

/// Joins `a`'s group and `b`'s in `parent`'s, under the lower root.
fn unite(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (root(parent, a), root(parent, b));
    if ra != rb
        && let Some(slot) = parent.get_mut(ra.max(rb))
    {
        *slot = ra.min(rb);
    }
}

/// A thinned line's skeleton: its pixels (each `on`), each one's place among them, how many of its
/// eight neighbours are on, and the map's squared distances to the line's edge.
struct Skeleton<'s, 'a> {
    line: &'s Line<'a>,
    pixels: &'s [usize],
    index: std::collections::BTreeMap<usize, usize>,
    count: Vec<usize>,
    d2: &'s [f64],
}

impl<'s, 'a> Skeleton<'s, 'a> {
    fn new(line: &'s Line<'a>, pixels: &'s [usize], d2: &'s [f64]) -> Self {
        let index = pixels.iter().enumerate().map(|(k, &i)| (i, k)).collect();
        let count = pixels.iter().map(|&i| line.around(i).iter().filter(|&&s| s).count()).collect();
        Self { line, pixels, index, count, d2 }
    }

    fn len(&self) -> usize {
        self.pixels.len()
    }

    /// How many neighbours the skeleton's pixel `k` (its place) has.
    fn count(&self, k: usize) -> usize {
        self.count.get(k).copied().unwrap_or(0)
    }

    /// The skeleton's pixels beside its pixel `k` (their places).
    fn neighbours(&self, k: usize) -> Vec<usize> {
        let Some(&i) = self.pixels.get(k) else { return Vec::new() };
        (0..8).filter_map(|n| self.line.neighbour(i, n)).filter(|&j| self.line.on(j)).filter_map(|j| self.index.get(&j).copied()).collect()
    }

    /// The map's pixels of a run of the skeleton's places.
    fn pixels_of(&self, run: &[usize]) -> Vec<usize> {
        run.iter().filter_map(|&x| self.pixels.get(x).copied()).collect()
    }

    /// The centre of the map's pixel `i` (working pixels).
    fn centre(&self, i: usize) -> Point {
        centre_of(i, self.line.regions.w as usize)
    }

    /// The radius of the largest disc the line holds at the map's pixel `i` (working pixels).
    fn radius(&self, i: usize) -> f64 {
        self.d2.get(i).copied().unwrap_or(0.0).sqrt() + HALF_PIXEL
    }
}

/// The graph of a thinned line's skeleton (its pixels `skeleton`, each `on`; `d2` the map's squared
/// distances to the line's edge): its nodes, and its pieces between them, loops with no node among
/// them as edges from a node of their own to itself.
fn graph(line: &Line<'_>, skeleton: &[usize], d2: &[f64]) -> (Vec<Node>, Vec<Edge>) {
    let sk = Skeleton::new(line, skeleton, d2);
    let (mut nodes, node_of) = nodes_of(&sk);
    let mut visited = vec![false; sk.len()];
    let mut edges = pieces(&sk, &node_of, &mut visited);
    for (node, pixels) in loops(&sk, &node_of, &mut visited) {
        nodes.push(node);
        let id = nodes.len() - 1;
        edges.push(Edge { a: id, b: id, pixels, gone: false });
    }
    (nodes, edges)
}

/// The skeleton's nodes: junction pixels side by side one node, at the centre of its deepest pixels
/// (of the largest disc the line holds there); each other pixel with other than two neighbours a
/// node of its own. With each of the skeleton's pixels' node, if any.
fn nodes_of(sk: &Skeleton<'_, '_>) -> (Vec<Node>, Vec<Option<usize>>) {
    let mut parent: Vec<usize> = (0..sk.len()).collect();
    for k in (0..sk.len()).filter(|&k| sk.count(k) >= 3) {
        for m in sk.neighbours(k).into_iter().filter(|&m| sk.count(m) >= 3) {
            unite(&mut parent, k, m);
        }
    }
    let mut node_of: Vec<Option<usize>> = vec![None; sk.len()];
    let mut nodes: Vec<Node> = Vec::new();
    // Each node's deepest pixels' coordinates added up, and how many.
    let mut sums: Vec<(f64, f64, f64)> = Vec::new();
    for k in (0..sk.len()).filter(|&k| sk.count(k) != 2) {
        let r = root(&mut parent, k);
        let id = match node_of.get(r).copied().flatten() {
            Some(id) => id,
            None => {
                nodes.push(Node { at: Point::ZERO, radius: 0.0, junction: sk.count(k) >= 3 });
                sums.push((0.0, 0.0, 0.0));
                if let Some(slot) = node_of.get_mut(r) {
                    *slot = Some(nodes.len() - 1);
                }
                nodes.len() - 1
            }
        };
        if let Some(slot) = node_of.get_mut(k) {
            *slot = Some(id);
        }
        let (Some(&i), Some(s), Some(node)) = (sk.pixels.get(k), sums.get_mut(id), nodes.get_mut(id)) else { continue };
        let (c, r) = (sk.centre(i), sk.radius(i));
        if r > node.radius {
            *s = (0.0, 0.0, 0.0);
            node.radius = r;
        }
        if r >= node.radius {
            *s = (s.0 + c.x, s.1 + c.y, s.2 + 1.0);
        }
    }
    for (node, &(sx, sy, n)) in nodes.iter_mut().zip(&sums) {
        if n > 0.0 {
            node.at = Point::new(sx / n, sy / n);
        }
    }
    (nodes, node_of)
}

/// The skeleton's pieces between its nodes (`node_of`, each pixel's): from each node's pixel along
/// each run of two-neighbour pixels to the next node, each run's pixels marked `visited`; two nodes
/// side by side a piece of two pixels.
fn pieces(sk: &Skeleton<'_, '_>, node_of: &[Option<usize>], visited: &mut [bool]) -> Vec<Edge> {
    let node = |k: usize| node_of.get(k).copied().flatten();
    let mut edges: Vec<Edge> = Vec::new();
    for k in 0..sk.len() {
        let Some(from) = node(k) else { continue };
        for m in sk.neighbours(k) {
            let piece = match node(m) {
                // From the lower node, once.
                Some(to) => (from < to).then(|| (to, vec![k, m])),
                None => run_from(sk, &node, visited, k, m),
            };
            if let Some((to, run)) = piece {
                edges.push(Edge { a: from, b: to, pixels: sk.pixels_of(&run), gone: false });
            }
        }
    }
    edges
}

/// The run of two-neighbour pixels from node pixel `k` through its neighbour `m` to the next node
/// (`node`, each pixel's): that node and the run's places, each but `k` marked `visited`; `None`
/// when `m` was visited before or the run reaches no node.
fn run_from(sk: &Skeleton<'_, '_>, node: &impl Fn(usize) -> Option<usize>, visited: &mut [bool], k: usize, m: usize) -> Option<(usize, Vec<usize>)> {
    if visited.get(m).copied().unwrap_or(true) {
        return None;
    }
    let mut run = vec![k, m];
    if let Some(v) = visited.get_mut(m) {
        *v = true;
    }
    let (mut prev, mut cur) = (k, m);
    for _ in 0..sk.len() {
        let next = sk.neighbours(cur).into_iter().find(|&x| x != prev)?;
        if let Some(to) = node(next) {
            run.push(next);
            return Some((to, run));
        }
        if visited.get(next).copied().unwrap_or(true) {
            return None;
        }
        if let Some(v) = visited.get_mut(next) {
            *v = true;
        }
        run.push(next);
        (prev, cur) = (cur, next);
    }
    None
}

/// The skeleton's loops with no node (pixels neither `visited` nor a node's, `node_of`): each one's
/// node, at its first pixel, and its pixels round from there back to it, each marked `visited`.
fn loops(sk: &Skeleton<'_, '_>, node_of: &[Option<usize>], visited: &mut [bool]) -> Vec<(Node, Vec<usize>)> {
    let mut found = Vec::new();
    for k in 0..sk.len() {
        if visited.get(k).copied().unwrap_or(true) || node_of.get(k).copied().flatten().is_some() {
            continue;
        }
        let Some(&i) = sk.pixels.get(k) else { continue };
        let node = Node { at: sk.centre(i), radius: sk.radius(i), junction: false };
        let mut run = vec![k];
        if let Some(v) = visited.get_mut(k) {
            *v = true;
        }
        // Round the loop from its first pixel, the first neighbour first, until it comes back.
        let (mut prev, mut cur) = (None, k);
        for _ in 0..sk.len() {
            let Some(next) = sk.neighbours(cur).into_iter().find(|&x| Some(x) != prev) else { break };
            run.push(next);
            if next == k || visited.get(next).copied().unwrap_or(true) {
                break;
            }
            if let Some(v) = visited.get_mut(next) {
                *v = true;
            }
            (prev, cur) = (Some(cur), next);
        }
        found.push((node, sk.pixels_of(&run)));
    }
    found
}

/// Each node's edges still there (a loop twice at its node).
fn degrees(nodes: &[Node], edges: &[Edge]) -> Vec<usize> {
    let mut deg = vec![0usize; nodes.len()];
    for e in edges.iter().filter(|e| !e.gone) {
        for n in [e.a, e.b] {
            if let Some(d) = deg.get_mut(n) {
                *d += 1;
            }
        }
    }
    deg
}

/// How many times spurs are pruned and nodes joined through, at most: a spur pruned can leave its
/// junction with two pieces, joined into a longer one that may be a spur of the next junction in
/// turn. An estimate, a bound on the work.
const PRUNE_ROUNDS: usize = 4;

/// The graph pruned and tidied (the module's notes): junctions whose discs overlap made one, spurs
/// that reach hardly past their junction's disc taken away (only from a junction that keeps two
/// pieces or more), and each node left with two pieces joined through. Each round goes over every
/// piece once, the nodes' degrees kept as pieces go.
fn pruned(nodes: &mut [Node], edges: &mut Vec<Edge>, w: usize) {
    let len = |e: &Edge| length(&e.pixels.iter().map(|&i| centre_of(i, w)).collect::<Vec<_>>());
    merge_junctions(nodes, edges, &len);
    let mut lengths: Vec<f64> = edges.iter().map(len).collect();
    for _ in 0..PRUNE_ROUNDS {
        let pruned_any = prune_spurs(nodes, edges, &lengths);
        let joined_any = join_through(nodes.len(), edges, &mut lengths);
        if !(pruned_any || joined_any) {
            break;
        }
    }
    edges.retain(|e| !e.gone);
}

/// Junctions whose discs overlap made one: the piece between them (`len` long) taken away, the
/// node lying where the deepest of them does, and every piece's ends the merged node's.
fn merge_junctions(nodes: &mut [Node], edges: &mut [Edge], len: &impl Fn(&Edge) -> f64) {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    for e in edges.iter_mut() {
        let (Some(a), Some(b)) = (nodes.get(e.a), nodes.get(e.b)) else { continue };
        if e.a != e.b && a.junction && b.junction && len(e) <= a.radius.max(b.radius) {
            unite(&mut parent, e.a, e.b);
            e.gone = true;
        }
    }
    let mut deepest: Vec<Option<Node>> = vec![None; nodes.len()];
    for n in 0..nodes.len() {
        let r = root(&mut parent, n);
        let (Some(node), Some(d)) = (nodes.get(n).copied(), deepest.get_mut(r)) else { continue };
        if d.is_none_or(|x| node.radius > x.radius) {
            *d = Some(node);
        }
    }
    for n in 0..nodes.len() {
        let r = root(&mut parent, n);
        if let (Some(Some(d)), Some(node)) = (deepest.get(r).copied(), nodes.get_mut(n)) {
            (node.at, node.radius) = (d.at, d.radius);
        }
    }
    for e in edges.iter_mut() {
        e.a = root(&mut parent, e.a);
        e.b = root(&mut parent, e.b);
    }
}

/// Spurs taken away: each piece (`lengths` long) from an end to a junction that keeps two pieces or
/// more, reaching hardly past the junction's disc ([`SPUR_SLACK_PX`], [`SPUR_SHARE`]), the nodes'
/// degrees kept as they go. Whether any went.
fn prune_spurs(nodes: &[Node], edges: &mut [Edge], lengths: &[f64]) -> bool {
    let mut deg = degrees(nodes, edges);
    let mut pruned_any = false;
    for (k, e) in edges.iter_mut().enumerate() {
        if e.gone {
            continue;
        }
        let (Some(&da), Some(&db)) = (deg.get(e.a), deg.get(e.b)) else { continue };
        let (end, junction) = match (da, db) {
            (1, d) if d >= 3 => (e.a, e.b),
            (d, 1) if d >= 3 => (e.b, e.a),
            _ => continue,
        };
        let (Some(end_node), Some(junction_node)) = (nodes.get(end), nodes.get(junction)) else { continue };
        let reach = lengths.get(k).copied().unwrap_or(0.0) + end_node.radius - junction_node.radius;
        if reach <= SPUR_SLACK_PX.max(SPUR_SHARE * junction_node.radius) {
            e.gone = true;
            pruned_any = true;
            for n in [end, junction] {
                if let Some(d) = deg.get_mut(n) {
                    *d = d.saturating_sub(1);
                }
            }
        }
    }
    pruned_any
}

/// Each of `node_count` nodes left with two pieces, neither a loop: the two joined through it into
/// the first (its length in `lengths` theirs together), the second gone. Whether any were.
fn join_through(node_count: usize, edges: &mut [Edge], lengths: &mut [f64]) -> bool {
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    for (k, e) in edges.iter().enumerate().filter(|(_, e)| !e.gone) {
        for n in [e.a, e.b] {
            if let Some(list) = at.get_mut(n) {
                list.push(k);
            }
        }
    }
    let mut joined_any = false;
    for n in 0..node_count {
        let live: Vec<usize> = at.get(n).map_or(Vec::new(), |l| l.iter().copied().filter(|&k| edges.get(k).is_some_and(|e| !e.gone)).collect());
        let [first, second] = live[..] else { continue };
        let (Some(e1), Some(e2)) = (edges.get(first).cloned(), edges.get(second).cloned()) else { continue };
        if first == second || e1.a == e1.b || e2.a == e2.b {
            continue;
        }
        let (from, to, pixels) = joined(e1, e2, n);
        let joined_len = lengths.get(first).copied().unwrap_or(0.0) + lengths.get(second).copied().unwrap_or(0.0);
        if let Some(e) = edges.get_mut(first) {
            *e = Edge { a: from, b: to, pixels, gone: false };
        }
        if let Some(e) = edges.get_mut(second) {
            e.gone = true;
        }
        if let Some(l) = lengths.get_mut(first) {
            *l = joined_len;
        }
        // The far node of the second piece now meets the first.
        if let Some(list) = at.get_mut(to) {
            for k in list.iter_mut().filter(|k| **k == second) {
                *k = first;
            }
        }
        joined_any = true;
    }
    joined_any
}

/// Pieces `e1` and `e2`, which meet at node `n`, as one: its far ends (`e1`'s, then `e2`'s) and its
/// pixels, the first's toward the node, then the second's away from it.
fn joined(e1: Edge, e2: Edge, n: usize) -> (usize, usize, Vec<usize>) {
    let mut p1 = e1.pixels;
    let from = if e1.b == n {
        e1.a
    } else {
        p1.reverse();
        e1.b
    };
    let mut p2 = e2.pixels;
    let to = if e2.a == n {
        e2.b
    } else {
        p2.reverse();
        e2.a
    };
    p1.extend(p2.into_iter().skip(1));
    (from, to, p1)
}

/// `pts` (an open piece, or a closed one whose first point is repeated at its end) with the points
/// whose triangle with their neighbours has less area than [`SIMPLIFY_AREA_PX2`] taken away, the
/// least first (the module's notes), its ends kept.
fn smoothed(pts: &[Point]) -> Vec<Point> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let triangle = |a: Point, b: Point, c: Point| ((b - a).cross(c - a) / 2.0).abs();
    let mut prev: Vec<usize> = (0..n).map(|i| i.saturating_sub(1)).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1).min(n - 1)).collect();
    let mut gone = vec![false; n];
    let mut version = vec![0u32; n];
    let area_of = |i: usize, prev: &[usize], next: &[usize]| -> f64 {
        match (prev.get(i).and_then(|&p| pts.get(p)), pts.get(i), next.get(i).and_then(|&q| pts.get(q))) {
            (Some(&a), Some(&b), Some(&c)) => triangle(a, b, c),
            _ => f64::INFINITY,
        }
    };
    // The heap holds each point's area by its bits (areas are never negative, so their bits sort
    // as they do), with the point's version when pushed.
    let mut heap: BinaryHeap<Reverse<(u64, usize, u32)>> = (1..n - 1).map(|i| Reverse((area_of(i, &prev, &next).to_bits(), i, 0))).collect();
    while let Some(Reverse((bits, i, v))) = heap.pop() {
        if gone.get(i).copied().unwrap_or(true) || version.get(i) != Some(&v) {
            continue;
        }
        if f64::from_bits(bits) >= SIMPLIFY_AREA_PX2 {
            break;
        }
        let (Some(&p), Some(&q)) = (prev.get(i), next.get(i)) else { continue };
        if let Some(g) = gone.get_mut(i) {
            *g = true;
        }
        if let Some(slot) = next.get_mut(p) {
            *slot = q;
        }
        if let Some(slot) = prev.get_mut(q) {
            *slot = p;
        }
        for j in [p, q] {
            if j == 0 || j == n - 1 {
                continue;
            }
            if let Some(ver) = version.get_mut(j) {
                *ver = ver.wrapping_add(1);
                heap.push(Reverse((area_of(j, &prev, &next).to_bits(), j, *ver)));
            }
        }
    }
    pts.iter().zip(&gone).filter(|(_, g)| !**g).map(|(p, _)| *p).collect()
}

/// The centre lines of the areas `wanted` picks among `regions`' (their places in its map): thinned,
/// made a graph, pruned, its ends carried to the ink and its pieces smoothed (the module's notes).
/// Each is `None` where it isn't wanted, has no skeleton, or comes to a centre line shorter than
/// the line is wide: a dot, or a blob, has no line to sew along, and is sewn as the area it is
/// (the project owner's choice, 2026-11-07, „Szyć jako kształt (polecam)”). A cancel stops it.
pub(super) fn lines(regions: &Regions, wanted: &dyn Fn(usize) -> bool) -> Result<Vec<Option<CentreLine>>> {
    let count = regions.areas.len();
    let want: Vec<bool> = (0..count).map(wanted).collect();
    let mut out: Vec<Option<CentreLine>> = vec![None; count];
    if !want.iter().any(|&w| w) {
        return Ok(out);
    }
    let (w, h) = (regions.w as usize, regions.h as usize);
    // Each pixel's squared distance from its area's edge, as the region map measures widths.
    let labels = &regions.labels;
    let mut edge = vec![false; labels.len()];
    for (i, &l) in labels.iter().enumerate() {
        if l == NONE {
            continue;
        }
        let (x, y) = (i % w, i / w);
        let other = |j: usize| labels.get(j) != Some(&l);
        edge[i] = x == 0 || y == 0 || x + 1 == w || y + 1 == h || other(i - 1) || other(i + 1) || other(i - w) || other(i + w);
    }
    let d2 = distances(&edge, w, h);
    drop(edge);
    vectorcraft_doc::cancel::check()?;
    let mut pixels: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut alive = vec![false; labels.len()];
    for (i, &l) in labels.iter().enumerate() {
        if want.get(l as usize).copied().unwrap_or(false)
            && let Some(list) = pixels.get_mut(l as usize)
        {
            list.push(i);
            if let Some(a) = alive.get_mut(i) {
                *a = true;
            }
        }
    }
    for (a, own) in pixels.iter().enumerate() {
        if own.is_empty() {
            continue;
        }
        vectorcraft_doc::cancel::check()?;
        let label = u32::try_from(a).unwrap_or(NONE);
        let mut line = Line { regions, label, alive: &mut alive };
        thinned(&mut line, own)?;
        let skeleton: Vec<usize> = own.iter().copied().filter(|&i| line.on(i)).collect();
        let (mut nodes, mut edges) = graph(&line, &skeleton, &d2);
        pruned(&mut nodes, &mut edges, w);
        let total: f64 = edges.iter().map(|e| length(&e.pixels.iter().map(|&i| centre_of(i, w)).collect::<Vec<_>>())).sum();
        let widest = regions.areas.get(a).map_or(1.0, |s| s.widest_px).max(1.0);
        let width = if total > 0.0 { (own.len() as f64 / total).clamp(1.0, widest) } else { widest };
        let deg = degrees(&nodes, &edges);
        let mut pieces: Vec<Piece> = Vec::with_capacity(edges.len());
        for e in &edges {
            let closed = e.a == e.b && deg.get(e.a) == Some(&2);
            let mut pts: Vec<Point> = e.pixels.iter().map(|&i| centre_of(i, w)).collect();
            if !closed {
                // The ends on their nodes, an end carried to the ink.
                for (end, first) in [(e.a, true), (e.b, false)] {
                    let Some(node) = nodes.get(end) else { continue };
                    let at = if deg.get(end) == Some(&1) { carried(&line, &pts, first, width).unwrap_or(node.at) } else { node.at };
                    let slot = if first { pts.first_mut() } else { pts.last_mut() };
                    if let Some(slot) = slot {
                        *slot = at;
                    }
                }
            }
            let pts = smoothed(&pts);
            if pts.len() >= 2 {
                pieces.push(Piece { pts, closed });
            }
        }
        let length_px: f64 = pieces.iter().map(|p| length(&p.pts)).sum();
        if pieces.is_empty() || length_px < width {
            continue;
        }
        let ends = deg.iter().filter(|&&d| d == 1).count();
        let junctions = deg.iter().filter(|&&d| d >= 3).count();
        if let Some(slot) = out.get_mut(a) {
            *slot = Some(CentreLine { pieces, width_px: width, length_px, ends, junctions });
        }
    }
    Ok(out)
}

/// Where an end of piece `pts` (its `first` point, or its last) is carried to: on along the way it
/// points, to where `line`'s ink ends, less half the line's `width` (so a round end drawn there
/// ends with the ink); `None` when it can't be told which way it points.
fn carried(line: &Line<'_>, pts: &[Point], first: bool, width: f64) -> Option<Point> {
    let along: Vec<Point> = if first { pts.to_vec() } else { pts.iter().rev().copied().collect() };
    let end = *along.first()?;
    let steps = END_DIRECTION_PX.max((width * END_DIRECTION_WIDTHS).ceil() as usize);
    let back = *along.get(steps.min(along.len().saturating_sub(1)))?;
    let d = end - back;
    let len = d.hypot();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let d = d / len;
    let (w, h) = (f64::from(line.regions.w), f64::from(line.regions.h));
    let ink = |q: Point| {
        q.x >= 0.0 && q.y >= 0.0 && q.x < w && q.y < h && line.regions.labels.get((q.y.floor() * w + q.x.floor()) as usize) == Some(&line.label)
    };
    let most = width * END_REACH_SHARE;
    let mut t = 0.0;
    while t + END_STEP_PX <= most && ink(end + d * (t + END_STEP_PX)) {
        t += END_STEP_PX;
    }
    // The ink ends half a step past the last point inside it, about; less half the width.
    let reach = (t + END_STEP_PX / 2.0 - width / 2.0).max(0.0);
    Some(end + d * reach)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;

    /// A pixel's eight neighbours (in [`AROUND`]'s order) from a 3 × 3 picture read row by row,
    /// `#` on, the pixel itself in the middle.
    fn around(rows: [&str; 3]) -> [bool; 8] {
        AROUND.map(|(dx, dy)| rows[(dy + 1) as usize].as_bytes()[(dx + 1) as usize] == b'#')
    }

    /// A region map `w` × `h` of one area (its label 0) where `on(x, y)`, no area's elsewhere.
    fn map(w: u32, h: u32, on: impl Fn(u32, u32) -> bool) -> Regions {
        let labels = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| if on(x, y) { 0 } else { NONE }).collect();
        Regions::from_labels(labels, w, h, 1, None)
    }

    fn line_of(regions: &Regions) -> CentreLine {
        lines(regions, &|a| a == 0).unwrap().into_iter().next().flatten().expect("a centre line")
    }

    /// Yokoi's connectivity number: none for a pixel alone or inside, one for an end and for a
    /// block's corner (taking it away parts nothing), two for a line's middle (it joins two pieces);
    /// a crossing's middle none (its arms touch each other corner to corner: it lies inside).
    #[test]
    fn the_connectivity_number_counts_what_a_pixel_joins() {
        assert_eq!(connectivity(around(["...", ".#.", "..."])), 0);
        assert_eq!(connectivity(around(["###", "###", "###"])), 0);
        assert_eq!(connectivity(around(["...", "##.", "..."])), 1);
        assert_eq!(connectivity(around(["##.", "##.", "..."])), 1);
        assert_eq!(connectivity(around(["...", "###", "..."])), 2);
        assert_eq!(connectivity(around([".#.", "###", ".#."])), 0);
    }

    /// Each half of a thinning pass takes its own side (Guo and Hall's A1): the first a band's east
    /// edge, the second its west; an end, a pixel inside and a line's middle stay in both.
    #[test]
    fn each_half_of_a_pass_peels_its_own_side() {
        let east = around(["##.", "##.", "##."]);
        assert!(peeled(east, false) && !peeled(east, true));
        let west = around([".##", ".##", ".##"]);
        assert!(!peeled(west, false) && peeled(west, true));
        for set in [around(["...", "##.", "..."]), around(["###", "###", "###"]), around(["...", "###", "..."])] {
            assert!(!peeled(set, false) && !peeled(set, true), "{set:?}");
        }
    }

    /// A plus of lines three pixels wide is one line with one junction and four ends, every point
    /// on a line's middle, its ends carried to the ink less half the width; a ring is one closed
    /// piece with neither.
    #[test]
    fn a_plus_and_a_ring_thin_to_their_middles() {
        let plus = map(41, 41, |x, y| ((19..22).contains(&x) && (2..39).contains(&y)) || ((19..22).contains(&y) && (2..39).contains(&x)));
        let line = line_of(&plus);
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (1, 4, 4), "{line:?}");
        let pts: Vec<Point> = line.pieces.iter().flat_map(|p| p.pts.iter().copied()).collect();
        assert!(pts.iter().all(|q| (q.x - 20.5).abs() <= 1.0 || (q.y - 20.5).abs() <= 1.0), "off the middle: {pts:?}");
        let (lo, hi) = (pts.iter().map(|q| q.x).fold(f64::INFINITY, f64::min), pts.iter().map(|q| q.x).fold(f64::NEG_INFINITY, f64::max));
        assert!((lo - 3.5).abs() <= 1.0 && (hi - 37.5).abs() <= 1.0, "the ends at {lo} and {hi}");
        assert!((line.width_px - 3.0).abs() <= 0.5, "{}", line.width_px);
        let ring = map(41, 41, |x, y| (13.5..16.5).contains(&(f64::from(x) - 20.0).hypot(f64::from(y) - 20.0)));
        let line = line_of(&ring);
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (0, 0, 1), "{line:?}");
        assert!(line.pieces[0].closed);
    }

    /// A bump of a pixel on a bar's edge is a spur that reaches hardly past the bar's middle: pruned,
    /// the bar one piece with two ends and no junction.
    #[test]
    fn a_bump_on_a_lines_edge_is_pruned() {
        let bumped = map(60, 15, |x, y| ((5..55).contains(&x) && (5..10).contains(&y)) || (x == 30 && y == 10));
        let line = line_of(&bumped);
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (0, 2, 1), "{line:?}");
    }

    /// A dot has no centre line wherever it lies among the pixels: it is sewn as the area it is.
    #[test]
    fn a_dot_has_no_centre_line_wherever_it_lies() {
        for (cx, cy) in [(20.0, 20.0), (20.5, 20.0), (20.5, 20.5)] {
            let dot = map(40, 40, |x, y| (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) <= 5.0);
            let found = lines(&dot, &|a| a == 0).unwrap();
            assert!(found[0].is_none(), "a dot at ({cx}, {cy}): {:?}", found[0]);
        }
    }
}
