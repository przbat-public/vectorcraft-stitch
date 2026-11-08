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
//!   junction draws nothing the disc doesn't ([`SPUR_SLACK_PX`], [`SPUR_WIDTH_SHARE`]); two
//!   junctions whose discs overlap are one (a crossing thinning splits in two).
//! - **The ends carried to the ink**: thinning stops short of a line's end by about its half width,
//!   and at a square end turns into a corner; each end's corner is taken off (its pixels' discs
//!   under [`END_TRIM_SHARE`] of the line's a little way in), and the end goes on along its piece
//!   to where the ink ends, less half a thread's width (or half the line's, for a line narrower
//!   than a thread), so that the running stitch sewn along it ends with the ink, on the line's
//!   middle, however wide the line.
//! - **The pieces made smooth** by Visvalingam and Whyatt's line generalisation (1993, *Line
//!   generalisation by repeated elimination of points*, The Cartographic Journal 30(1)): the pixels'
//!   stairs taken away, the point whose triangle with its neighbours has the least area removed in
//!   turn until none is smaller than [`SIMPLIFY_AREA_MM2`], at most [`SIMPLIFY_AREA_PX2`] ([`smoothed`]),
//!   a closed piece keeping three points and an open one its two ends.
//!
//! Every step takes time in proportion to the line's pixels (thinning looks again only beside what
//! it took away), or their logarithm besides (the smoothing's heap, and a chain of nodes joined
//! through, the shorter piece moved onto the longer at each); a cancel stops it between lines and
//! within one every [`CANCEL_EVERY`] pixels looked at, walked or joined and points smoothed.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use vectorcraft_geom::Point;

use super::regions::{NONE, Regions, distances, width_at};
use crate::Result;

/// How many passes of thinning a line takes at most: each peels a pixel off every side, so a line
/// is thinned in half its width in pixels; a thousand peels an area two hundred millimetres wide at
/// ten pixels a millimetre, more than any design holds. A bound on the work.
const MAX_THIN_PASSES: usize = 1_000;

/// How many times the skeleton is gone over at the end for stair pixels, at most: each pass takes
/// those it can, and one or two leave none. An estimate, a bound on the work.
const MAX_CLEANUP_PASSES: usize = 8;

/// How far past the largest disc at its junction a spur may reach and be pruned (working pixels):
/// two, what anti-aliasing leaves on a line's edge: a hair a pixel wide and two long thins to a
/// spur reaching 1.6 to 1.8 pixels past its junction's disc (measured on drawn pixels in the
/// review, 2026-11-07). An estimate.
pub(super) const SPUR_SLACK_PX: f64 = 2.0;

/// The share of the line's width (`AreaStats::ridge_px`) a branch must reach past its junction's
/// disc to be kept, on a wider line: a third (the project owner's choice, 2026-11-07, „Zostawić
/// dłuższe (polecam)”), so that the top of a "t" rising 0.8 mm above its bar on lines 0.9 mm wide
/// stays, and the corners of a square end, which reach (√2 − 1) ≈ 0.41 of the line's half width,
/// go.
pub(super) const SPUR_WIDTH_SHARE: f64 = 1.0 / 3.0;

/// The largest disc a corner's tip holds (working pixels): a pixel and a half, the centre of a pixel
/// beside the edge. A spur ending in one that reaches past its junction's disc no farther than a
/// square end's corner does ((√2 − 1) of the disc, and [`SPUR_SLACK_PX`]) is that corner, and goes
/// however wide the line, so that a line of changing width ends on its middle at a square end.
const CORNER_TIP_PX: f64 = 1.5;

/// The least area a point's triangle with its neighbours keeps it by (square working pixels): 8, so
/// that the stairs pixels leave along a gentle curve go and a corner whose arms reach 0.4 mm stays
/// (at 10 pixels a millimetre). An estimate, measured on the line art of the tests (2026-11-07): at
/// 1 the centre lines of a ring 29 mm across and of an S curve turned more sharply than the
/// planner's corners (30°) at 245 and 145 of their points, each a needle point of its own, and were
/// sewn with 1.9 and 1.8 times the vector path's needle points; at 8 they turn so at none but where
/// a closed piece starts, and are sewn with as many. A point taken away lies about a pixel off the
/// line kept, along a ring that size (a triangle of 8 square pixels on a base of 13 is 1.2 high).
pub(super) const SIMPLIFY_AREA_PX2: f64 = 8.0;

/// The same least area in square millimetres: 0.08, [`SIMPLIFY_AREA_PX2`] at 10 pixels a
/// millimetre, so that a coarser working copy (5 pixels a millimetre for a design 300 mm wide)
/// takes away no more of a line than a fine one; the stairs pixels leave are a pixel high however
/// coarse, so at finer resolutions [`SIMPLIFY_AREA_PX2`] bounds it.
pub(super) const SIMPLIFY_AREA_MM2: f64 = 0.08;

/// The fewest points smoothing leaves a closed piece (its first point repeated at its end): three
/// and the repeat, a triangle round the ring, so that a small ring keeps its length.
const CLOSED_LEAST_POINTS: usize = 4;

/// How many pixels are thinned, walked or joined, or points smoothed, between two cancel checks.
const CANCEL_EVERY: usize = 1 << 14;

/// The work a line's steps have done since the last cancel check ([`CANCEL_EVERY`]).
#[derive(Default)]
struct Ticks(usize);

impl Ticks {
    /// `n` more pixels or points worked on; a cancel stops it once they pass [`CANCEL_EVERY`].
    fn add(&mut self, n: usize) -> Result<()> {
        self.0 = self.0.saturating_add(n);
        if self.0 >= CANCEL_EVERY {
            self.0 = 0;
            vectorcraft_doc::cancel::check()?;
        }
        Ok(())
    }
}

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

/// The share of the largest disc a little way in ([`END_TRIM_LOOK`]) under which a pixel at a
/// piece's free end is taken off: about three quarters, so that the corner a square end's skeleton
/// turns into, its discs narrowing to nothing, goes, and a round end, whose discs stay about the
/// line's half width to its tip, stays (the review's method, 2026-11-07). An estimate.
const END_TRIM_SHARE: f64 = 0.75;

/// How far in from a piece's free end the discs are looked at to judge its tip, as a share of the
/// tip's own radius, and a pixel more: twice it, past the corner of a square end the tip lies in.
const END_TRIM_LOOK: f64 = 2.0;

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

/// Whether an area's centre line is wanted, and how wide it may be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Wanted {
    /// No: the area is sewn as it is.
    No,
    /// Whatever its width: an area of a colour set to Lines.
    AnyWidth,
    /// Only where no stretch of it away from its junctions is wider than this (working pixels) for
    /// as long as it is wide: a line by the rules, the extra width where its lines meet alone (the
    /// project owner's choice, 2026-11-07, „Tylko na skrzyżowaniach (polecam)”).
    NoWiderThan(f64),
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
/// A cancel stops it. How many pixels it looked at, in its passes.
fn thinned(line: &mut Line<'_>, pixels: &[usize]) -> Result<usize> {
    // The pixels to look at: at first every one on the line's edge, then those beside the pixels
    // taken away; each once a half.
    let mut todo: Vec<usize> = pixels.iter().copied().filter(|&i| line.around(i).iter().any(|&s| !s)).collect();
    // How many halves in a row each pixel to look at has been looked at with nothing beside it
    // taken: once both halves have kept it, it stays until a neighbour goes (whether a half takes
    // a pixel depends on its neighbours alone), and is looked at again only then.
    let mut looked: std::collections::HashMap<usize, u8> = std::collections::HashMap::new();
    let mut queued = std::collections::BTreeSet::new();
    let (mut done, mut looked_at) = (0usize, 0usize);
    for _ in 0..MAX_THIN_PASSES {
        let mut taken_any = false;
        for second in [false, true] {
            let taken: Vec<usize> = todo.iter().copied().filter(|&i| line.on(i) && peeled(line.around(i), second)).collect();
            for &i in &taken {
                line.take(i);
            }
            taken_any |= !taken.is_empty();
            done += todo.len();
            looked_at = looked_at.saturating_add(todo.len());
            // The next half looks at what lies beside what went (the edge moved in), afresh, and
            // again at what it looked at and is still there (the other half may take it), until
            // both halves have kept it.
            queued.clear();
            let mut next: Vec<usize> = Vec::with_capacity(todo.len());
            let beside: Vec<usize> = taken.iter().flat_map(|&i| (0..8).map(move |k| (i, k))).filter_map(|(i, k)| line.neighbour(i, k)).collect();
            for j in beside {
                if line.on(j) && queued.insert(j) {
                    looked.insert(j, 0);
                    next.push(j);
                }
            }
            for &j in &todo {
                if !line.on(j) || queued.contains(&j) {
                    continue;
                }
                let halves = looked.entry(j).or_insert(0);
                *halves = halves.saturating_add(1);
                if *halves < 2 && queued.insert(j) {
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
    Ok(looked_at)
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
fn graph(line: &Line<'_>, skeleton: &[usize], d2: &[f64], ticks: &mut Ticks) -> Result<(Vec<Node>, Vec<Edge>)> {
    let sk = Skeleton::new(line, skeleton, d2);
    ticks.add(sk.len())?;
    let (mut nodes, node_of) = nodes_of(&sk);
    ticks.add(sk.len())?;
    let mut visited = vec![false; sk.len()];
    let mut edges = pieces(&sk, &node_of, &mut visited, ticks)?;
    for (node, pixels) in loops(&sk, &node_of, &mut visited, ticks)? {
        nodes.push(node);
        let id = nodes.len() - 1;
        edges.push(Edge { a: id, b: id, pixels, gone: false });
    }
    Ok((nodes, edges))
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
fn pieces(sk: &Skeleton<'_, '_>, node_of: &[Option<usize>], visited: &mut [bool], ticks: &mut Ticks) -> Result<Vec<Edge>> {
    let node = |k: usize| node_of.get(k).copied().flatten();
    let mut edges: Vec<Edge> = Vec::new();
    for k in 0..sk.len() {
        ticks.add(1)?;
        let Some(from) = node(k) else { continue };
        for m in sk.neighbours(k) {
            let piece = match node(m) {
                // From the lower node, once.
                Some(to) => (from < to).then(|| (to, vec![k, m])),
                None => run_from(sk, &node, visited, k, m),
            };
            if let Some((to, run)) = piece {
                ticks.add(run.len())?;
                edges.push(Edge { a: from, b: to, pixels: sk.pixels_of(&run), gone: false });
            }
        }
    }
    Ok(edges)
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
fn loops(sk: &Skeleton<'_, '_>, node_of: &[Option<usize>], visited: &mut [bool], ticks: &mut Ticks) -> Result<Vec<(Node, Vec<usize>)>> {
    let mut found = Vec::new();
    for k in 0..sk.len() {
        ticks.add(1)?;
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
        ticks.add(run.len())?;
        found.push((node, sk.pixels_of(&run)));
    }
    Ok(found)
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

/// The graph of a line `line_px` wide pruned and tidied (the module's notes): junctions whose discs
/// overlap made one, spurs that reach hardly past their junction's disc taken away (only from a
/// junction that keeps two pieces or more), and each node left with two pieces joined through.
/// Each round goes over every piece once, the nodes' degrees kept as pieces go.
fn pruned(nodes: &mut [Node], edges: &mut Vec<Edge>, w: usize, line_px: f64, ticks: &mut Ticks) -> Result<()> {
    let len = |e: &Edge| length(&e.pixels.iter().map(|&i| centre_of(i, w)).collect::<Vec<_>>());
    merge_junctions(nodes, edges, &len);
    let mut lengths: Vec<f64> = edges.iter().map(len).collect();
    let most = SPUR_SLACK_PX.max(SPUR_WIDTH_SHARE * line_px);
    for _ in 0..PRUNE_ROUNDS {
        ticks.add(edges.len())?;
        let pruned_any = prune_spurs(nodes, edges, &lengths, most);
        let joined_any = join_through(nodes.len(), edges, &mut lengths, ticks)?;
        if !(pruned_any || joined_any) {
            break;
        }
    }
    edges.retain(|e| !e.gone);
    Ok(())
}

/// Junctions whose discs overlap made one: two joined by a piece (`len` long) no longer than their
/// two radii together, the piece taken away, the node lying where the deeper of them does, and
/// every piece's ends the merged node's.
fn merge_junctions(nodes: &mut [Node], edges: &mut [Edge], len: &impl Fn(&Edge) -> f64) {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    for e in edges.iter_mut() {
        let (Some(a), Some(b)) = (nodes.get(e.a), nodes.get(e.b)) else { continue };
        if e.a != e.b && a.junction && b.junction && len(e) <= a.radius + b.radius {
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

/// Spurs taken away: each piece (`lengths` long) from an end to a junction, both as they are at the
/// round's start, reaching past the junction's disc no more than `most` (working pixels:
/// [`SPUR_SLACK_PX`], [`SPUR_WIDTH_SHARE`]) or ending in a corner ([`CORNER_TIP_PX`]), the least
/// reaching first, a junction left with one piece at the least (an end), never none: so that both
/// corners of a square end go, the line ending where they forked. Whether any went.
fn prune_spurs(nodes: &[Node], edges: &mut [Edge], lengths: &[f64], most: f64) -> bool {
    let mut deg = degrees(nodes, edges);
    // The spurs by the degrees at the round's start: how far each reaches, which it is, its junction.
    let mut spurs: Vec<(f64, usize, usize)> = Vec::new();
    for (k, e) in edges.iter().enumerate().filter(|(_, e)| !e.gone) {
        let (Some(&da), Some(&db)) = (deg.get(e.a), deg.get(e.b)) else { continue };
        let (end, junction) = match (da, db) {
            (1, d) if d >= 3 => (e.a, e.b),
            (d, 1) if d >= 3 => (e.b, e.a),
            _ => continue,
        };
        let (Some(end_node), Some(junction_node)) = (nodes.get(end), nodes.get(junction)) else { continue };
        let reach = lengths.get(k).copied().unwrap_or(0.0) + end_node.radius - junction_node.radius;
        let corner = end_node.radius <= CORNER_TIP_PX && reach <= (std::f64::consts::SQRT_2 - 1.0) * junction_node.radius + SPUR_SLACK_PX;
        if reach <= most || corner {
            spurs.push((reach, k, junction));
        }
    }
    spurs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut pruned_any = false;
    for (_, k, junction) in spurs {
        if deg.get(junction).is_some_and(|&d| d > 1)
            && let Some(e) = edges.get_mut(k)
        {
            e.gone = true;
            pruned_any = true;
            if let Some(d) = deg.get_mut(junction) {
                *d -= 1;
            }
        }
    }
    pruned_any
}

/// Each of `node_count` nodes left with two pieces, neither a loop: the two joined through it into
/// the first (its length in `lengths` theirs together), the second gone. Whether any were.
///
/// A chain of such nodes joins one piece onto the next again and again: each piece's pixels are kept
/// as a [`Chain`] while it is joined, so that each join moves the shorter's pixels onto the longer's
/// end, neither copied nor turned round, and the chain takes time in proportion to its pixels (and
/// their logarithm). A cancel stops it.
fn join_through(node_count: usize, edges: &mut [Edge], lengths: &mut [f64], ticks: &mut Ticks) -> Result<bool> {
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    for (k, e) in edges.iter().enumerate().filter(|(_, e)| !e.gone) {
        for n in [e.a, e.b] {
            if let Some(list) = at.get_mut(n) {
                list.push(k);
            }
        }
    }
    let mut chains: Vec<Option<Chain>> = (0..edges.len()).map(|_| None).collect();
    let mut joined_any = false;
    for n in 0..node_count {
        let live: Vec<usize> = at.get(n).map_or(Vec::new(), |l| l.iter().copied().filter(|&k| edges.get(k).is_some_and(|e| !e.gone)).collect());
        let [first, second] = live[..] else { continue };
        let (Some(&Edge { a: a1, b: b1, .. }), Some(&Edge { a: a2, b: b2, .. })) = (edges.get(first), edges.get(second)) else { continue };
        if first == second || a1 == b1 || a2 == b2 {
            continue;
        }
        let (Some(c1), Some(c2)) = (Chain::take(&mut chains, edges, first), Chain::take(&mut chains, edges, second)) else { continue };
        ticks.add(c1.len().min(c2.len()))?;
        // The first toward the node, the second away from it.
        let (from, c1) = if b1 == n { (a1, c1) } else { (b1, c1.turned()) };
        let (to, c2) = if a2 == n { (b2, c2) } else { (a2, c2.turned()) };
        if let Some(slot) = chains.get_mut(first) {
            *slot = Some(Chain::joined(c1, c2));
        }
        let joined_len = lengths.get(first).copied().unwrap_or(0.0) + lengths.get(second).copied().unwrap_or(0.0);
        if let Some(e) = edges.get_mut(first) {
            (e.a, e.b, e.gone) = (from, to, false);
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
    // The pieces joined, their pixels back in order.
    for (k, chain) in chains.into_iter().enumerate() {
        if let (Some(chain), Some(e)) = (chain, edges.get_mut(k)) {
            e.pixels = chain.into_pixels();
        }
    }
    Ok(joined_any)
}

/// A piece's pixels while it is joined through its nodes ([`join_through`]): a double-ended queue,
/// read from its back when `turned`, so that turning it round takes no time and pixels go onto
/// either end.
struct Chain {
    pixels: std::collections::VecDeque<usize>,
    turned: bool,
}

impl Chain {
    /// Piece `k`'s pixels as a chain: the one it is already, or its pixels taken from `edges`.
    fn take(chains: &mut [Option<Chain>], edges: &mut [Edge], k: usize) -> Option<Chain> {
        match chains.get_mut(k)?.take() {
            Some(chain) => Some(chain),
            None => Some(Chain { pixels: std::mem::take(&mut edges.get_mut(k)?.pixels).into(), turned: false }),
        }
    }

    fn len(&self) -> usize {
        self.pixels.len()
    }

    /// The same pixels the other way round.
    fn turned(self) -> Self {
        Self { turned: !self.turned, ..self }
    }

    /// Its pixels in order.
    fn into_pixels(self) -> Vec<usize> {
        let mut v: Vec<usize> = self.pixels.into();
        if self.turned {
            v.reverse();
        }
        v
    }

    /// `a`'s pixels, then `b`'s but its first (the node's pixel, `a`'s last): the shorter's moved
    /// onto the longer's end.
    fn joined(a: Chain, mut b: Chain) -> Chain {
        let _ = if b.turned { b.pixels.pop_back() } else { b.pixels.pop_front() };
        if a.len() >= b.len() {
            let mut a = a;
            for p in b.into_pixels() {
                if a.turned { a.pixels.push_front(p) } else { a.pixels.push_back(p) }
            }
            a
        } else {
            for p in a.into_pixels().into_iter().rev() {
                if b.turned { b.pixels.push_back(p) } else { b.pixels.push_front(p) }
            }
            b
        }
    }
}

/// `pts` (an open piece, or a `closed` one whose first point is repeated at its end) with the points
/// whose triangle with their neighbours has less area than `least_px2` (square working pixels)
/// taken away, the least first (the module's notes), its ends kept, and a closed piece three points
/// at least ([`CLOSED_LEAST_POINTS`]).
fn smoothed(pts: &[Point], closed: bool, least_px2: f64, ticks: &mut Ticks) -> Result<Vec<Point>> {
    let n = pts.len();
    let keep = if closed { CLOSED_LEAST_POINTS } else { 2 };
    if n < 3 || n <= keep {
        return Ok(pts.to_vec());
    }
    let mut left = n;
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
        ticks.add(1)?;
        if gone.get(i).copied().unwrap_or(true) || version.get(i) != Some(&v) {
            continue;
        }
        if f64::from_bits(bits) >= least_px2 || left <= keep {
            break;
        }
        let (Some(&p), Some(&q)) = (prev.get(i), next.get(i)) else { continue };
        if let Some(g) = gone.get_mut(i) {
            *g = true;
            left -= 1;
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
    Ok(pts.iter().zip(&gone).filter(|(_, g)| !**g).map(|(p, _)| *p).collect())
}

/// Whether a stretch of a centre line (`nodes` and `edges`, each node's degree `deg`) away from its
/// junctions is wider than `limit` (working pixels, `width` at each of the map's pixels, a map `w`
/// wide) for as long as `limit`: a thick stretch, not a bump of a pixel or two nor a junction's
/// disc. A pixel's disc overlapping a junction's lies by it.
fn widens_away_from_junctions(nodes: &[Node], edges: &[Edge], deg: &[usize], width: &dyn Fn(usize) -> f64, limit: f64, w: usize) -> bool {
    let junctions: Vec<&Node> = nodes.iter().zip(deg).filter(|&(_, &d)| d >= 3).map(|(n, _)| n).collect();
    for e in edges {
        let (mut run, mut last): (f64, Option<Point>) = (0.0, None);
        for &i in &e.pixels {
            let (c, wide) = (centre_of(i, w), width(i));
            // Narrow enough, or wider only by a junction's disc: the junctions looked through only
            // for a pixel wider than the limit, so that a thin line's thousands of junctions cost
            // nothing at its narrow pixels.
            if wide <= limit || junctions.iter().any(|j| c.distance(j.at) <= j.radius + wide / 2.0) {
                (run, last) = (0.0, None);
                continue;
            }
            run += last.map_or(0.0, |p| c.distance(p));
            last = Some(c);
            if run >= limit {
                return true;
            }
        }
    }
    false
}

/// The centre lines of the areas `wanted` picks among `regions`' (their places in its map): thinned,
/// made a graph, pruned, its ends carried to the ink and its pieces smoothed (the module's notes).
/// Each is `None` where it isn't wanted, has no skeleton, comes to a centre line shorter than the
/// line is wide (a dot, or a blob, has no line to sew along, and is sewn as the area it is: the
/// project owner's choice, 2026-11-07, „Szyć jako kształt (polecam)”), or, held to a width, widens
/// past it away from its junctions. A cancel stops it.
pub(super) fn lines(regions: &Regions, wanted: &dyn Fn(usize) -> Wanted, px_per_mm: f64) -> Result<Vec<Option<CentreLine>>> {
    let thread_px = vectorcraft_stitch::plan::THREAD_WIDTH * px_per_mm;
    let least_px2 = SIMPLIFY_AREA_PX2.min(SIMPLIFY_AREA_MM2 * px_per_mm * px_per_mm);
    let count = regions.areas.len();
    let want: Vec<Wanted> = (0..count).map(wanted).collect();
    let mut out: Vec<Option<CentreLine>> = vec![None; count];
    if want.iter().all(|&w| w == Wanted::No) {
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
        if want.get(l as usize).is_some_and(|&w| w != Wanted::No)
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
        let mut ticks = Ticks::default();
        let (mut nodes, mut edges) = graph(&line, &skeleton, &d2, &mut ticks)?;
        let ridge = regions.areas.get(a).map_or(0.0, |s| s.ridge_px);
        pruned(&mut nodes, &mut edges, w, ridge, &mut ticks)?;
        let deg = degrees(&nodes, &edges);
        if let Some(&Wanted::NoWiderThan(limit)) = want.get(a)
            && widens_away_from_junctions(&nodes, &edges, &deg, &|i| width_at(labels, &d2, w, h, i), limit, w)
        {
            continue;
        }
        let total: f64 = edges.iter().map(|e| length(&e.pixels.iter().map(|&i| centre_of(i, w)).collect::<Vec<_>>())).sum();
        let widest = regions.areas.get(a).map_or(1.0, |s| s.widest_px).max(1.0);
        let width = if total > 0.0 { (own.len() as f64 / total).clamp(1.0, widest) } else { widest };
        let mut pieces: Vec<Piece> = Vec::with_capacity(edges.len());
        let radius = |i: usize| d2.get(i).copied().unwrap_or(0.0).sqrt() + HALF_PIXEL;
        for e in &edges {
            // Its ends trimmed and carried, and its pixels copied as points.
            ticks.add(e.pixels.len())?;
            let closed = e.a == e.b && deg.get(e.a) == Some(&2);
            // A free end's corner taken off.
            let free = |n: usize| !closed && deg.get(n) == Some(&1);
            let from = if free(e.a) { trimmed(&e.pixels, true, &radius) } else { 0 };
            let to = e.pixels.len() - if free(e.b) { trimmed(&e.pixels, false, &radius) } else { 0 };
            let kept = e.pixels.get(from..to).filter(|k| k.len() >= 2).unwrap_or(&e.pixels);
            let mut pts: Vec<Point> = kept.iter().map(|&i| centre_of(i, w)).collect();
            if !closed {
                // The ends on their nodes, a free end carried to the ink.
                for (end, first) in [(e.a, true), (e.b, false)] {
                    let Some(node) = nodes.get(end) else { continue };
                    let tip = if first { pts.first() } else { pts.last() }.copied().unwrap_or(node.at);
                    let at = if free(end) { carried(&line, &pts, first, width, thread_px).unwrap_or(tip) } else { node.at };
                    let slot = if first { pts.first_mut() } else { pts.last_mut() };
                    if let Some(slot) = slot {
                        *slot = at;
                    }
                }
            }
            let pts = smoothed(&pts, closed, least_px2, &mut ticks)?;
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

/// How many of piece `pixels`' pixels are taken off at its `first` end, or its last, as the corner
/// of a square end (the module's notes): those from the tip whose disc (`radius`) is under
/// [`END_TRIM_SHARE`] of the largest within [`END_TRIM_LOOK`] of their own radius in; at most half
/// the piece.
fn trimmed(pixels: &[usize], first: bool, radius: &dyn Fn(usize) -> f64) -> usize {
    let along: Vec<f64> = if first { pixels.iter().map(|&i| radius(i)).collect() } else { pixels.iter().rev().map(|&i| radius(i)).collect() };
    let mut k = 0;
    while k < along.len() / 2 {
        let Some(&r) = along.get(k) else { break };
        let look = (END_TRIM_LOOK * r).ceil() as usize + 1;
        let ahead = along.iter().skip(k + 1).take(look).copied().fold(0.0, f64::max);
        if r >= END_TRIM_SHARE * ahead {
            break;
        }
        k += 1;
    }
    k
}

/// Where an end of piece `pts` (its `first` point, or its last) is carried to: on along the way it
/// points, to where `line`'s ink ends, less half a thread's width (`thread_px`) or half the line's
/// `width` where that is less, so that the stitch sewn there ends with the ink; `None` when it
/// can't be told which way it points.
fn carried(line: &Line<'_>, pts: &[Point], first: bool, width: f64, thread_px: f64) -> Option<Point> {
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
    // The ink ends half a step past the last point inside it, about; less half a thread.
    let reach = (t + END_STEP_PX / 2.0 - width.min(thread_px) / 2.0).max(0.0);
    Some(end + d * reach)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;

    /// The working copy's resolution the maps here stand for (pixels a millimetre), and a thread's
    /// width at it (working pixels).
    const PX_PER_MM: f64 = 10.0;
    const THREAD_PX: f64 = vectorcraft_stitch::plan::THREAD_WIDTH * PX_PER_MM;

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
        lines(regions, &|a| if a == 0 { Wanted::AnyWidth } else { Wanted::No }, PX_PER_MM)
            .unwrap()
            .into_iter()
            .next()
            .flatten()
            .expect("a centre line")
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

    /// Bumps on a bar's edge thin to spurs that reach hardly past the bar's middle: pruned, the bar
    /// joined through, one piece with two ends and no junction. A bump 3 pixels square on a bar 5
    /// wide, one 4 pixels square on a bar 5 wide, and a hair a pixel wide and two long on a bar 6
    /// wide, as anti-aliasing leaves one (it reaches 1.6 to 1.8 pixels past the junction's disc).
    /// With the pruning off, each is a junction with three ends.
    #[test]
    fn bumps_and_hairs_on_a_lines_edge_are_pruned() {
        let bars = [
            map(60, 15, |x, y| ((5..55).contains(&x) && (5..10).contains(&y)) || ((30..33).contains(&x) && (10..13).contains(&y))),
            map(70, 25, |x, y| ((5..65).contains(&x) && (8..13).contains(&y)) || ((30..34).contains(&x) && (4..8).contains(&y))),
            map(70, 25, |x, y| ((5..65).contains(&x) && (8..14).contains(&y)) || (x == 30 && (6..8).contains(&y))),
        ];
        for (k, bar) in bars.iter().enumerate() {
            let line = line_of(bar);
            assert_eq!((line.junctions, line.ends, line.pieces.len()), (0, 2, 1), "bar {k}: {line:?}");
        }
    }

    /// A real branch is kept, short as it is, and a nub shorter than a third of the line's width
    /// is pruned (the project owner's choice, 2026-11-07, „Zostawić dłuższe (polecam)”): the top of a
    /// "t" of lines 13 pixels wide rising 9 pixels above its bar reaches 4.78 pixels past the
    /// junction's disc, more than a third of the width (4.33): one junction, four ends; rising 8 it
    /// reaches 3.78, and is pruned: a T, one junction and three ends.
    #[test]
    fn the_top_of_a_t_is_kept_and_a_nub_pruned() {
        let t = |rise: u32| {
            map(120, 90, move |x, y| ((10..110).contains(&x) && (25..38).contains(&y)) || ((50..63).contains(&x) && (25 - rise..80).contains(&y)))
        };
        let line = line_of(&t(9));
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (1, 4, 4), "{line:?}");
        let line = line_of(&t(8));
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (1, 3, 3), "{line:?}");
    }

    /// A dot has no centre line wherever it lies among the pixels: it is sewn as the area it is.
    #[test]
    fn a_dot_has_no_centre_line_wherever_it_lies() {
        for (cx, cy) in [(20.0, 20.0), (20.5, 20.0), (20.5, 20.5)] {
            let dot = map(40, 40, |x, y| (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) <= 5.0);
            let found = lines(&dot, &|a| if a == 0 { Wanted::AnyWidth } else { Wanted::No }, PX_PER_MM).unwrap();
            assert!(found[0].is_none(), "a dot at ({cx}, {cy}): {:?}", found[0]);
        }
    }

    /// A bar at 30°, its ends square, 6 and 20 pixels wide: its centre line keeps to its middle and
    /// reaches its ends. Thinning forks at a square end to its two corners, or runs into one of
    /// them: both corners are pruned (the degrees as the round starts), the corner a tip runs into
    /// is taken off, and the end carried along the line to the ink, less half a thread.
    #[test]
    fn a_slanted_square_end_ends_on_the_middle() {
        let (c, a) = (Point::new(70.0, 70.0), 30f64.to_radians());
        let (u, n) = (vectorcraft_geom::Vec2::new(a.cos(), a.sin()), vectorcraft_geom::Vec2::new(-a.sin(), a.cos()));
        for half_width in [3.0, 10.0] {
            let bar = map(140, 140, |x, y| {
                let q = Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5) - c;
                q.dot(u).abs() <= 50.0 && q.dot(n).abs() <= half_width
            });
            let line = line_of(&bar);
            let off = line.pieces.iter().flat_map(|p| &p.pts).map(|&q| (q - c).dot(n).abs()).fold(0.0, f64::max);
            assert!(off <= 1.5, "half width {half_width}: a point {off} px off the middle: {line:?}");
            let reach = line.pieces.iter().flat_map(|p| &p.pts).map(|&q| (q - c).dot(u).abs()).fold(0.0, f64::max);
            assert!(reach >= 50.0 - THREAD_PX / 2.0 - 1.5, "half width {half_width}: reaching {reach} px of 50: {line:?}");
        }
    }

    /// A small ring (its middle 5 pixels across, a line 2 pixels wide) keeps a closed centre line
    /// with a length: smoothing leaves a closed piece three points, where it took every point but
    /// its start, a piece of no length.
    #[test]
    fn a_small_ring_keeps_its_length() {
        let ring = map(30, 30, |x, y| ((f64::from(x) + 0.5 - 15.0).hypot(f64::from(y) + 0.5 - 15.0) - 2.5).abs() <= 1.0);
        let line = line_of(&ring);
        assert!(line.pieces.len() == 1 && line.pieces[0].closed && line.length_px >= 10.0, "{line:?}");
    }

    /// An X of lines 4 pixels wide crossing at 60° thins to two forks, whose discs overlap: they are
    /// one junction, four ends. With the merging off, two junctions.
    #[test]
    fn a_crossing_thinning_splits_is_one_junction() {
        let a = 30f64.to_radians();
        let (c, d) = (Point::new(50.0, 30.0), vectorcraft_geom::Vec2::new(40.0 * a.cos(), 40.0 * a.sin()));
        let e = vectorcraft_geom::Vec2::new(d.x, -d.y);
        let near = |q: Point, p0: Point, p1: Point| {
            let ab = p1 - p0;
            q.distance(p0 + ab * ((q - p0).dot(ab) / ab.hypot2()).clamp(0.0, 1.0)) <= 2.0
        };
        let x = map(100, 60, |x, y| {
            let q = Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            near(q, c - d, c + d) || near(q, c - e, c + e)
        });
        let line = line_of(&x);
        assert_eq!((line.junctions, line.ends), (1, 4), "{line:?}");
    }

    /// A long line with a bump on its edge every few pixels (each a spur pruned, its junction left
    /// with two pieces, joined through) is one piece with two ends: a serpentine 3 pixels wide, its
    /// rows 7 apart, a bump 1 × 2 pixels below each row every 4, 1,500 pixels a side.
    #[test]
    fn a_long_bumpy_line_is_one_piece() {
        let side = 1500u32;
        let rows = (side - 8) / 7;
        let on = move |x: u32, y: u32| {
            if y < 4 || x < 4 || x >= side - 4 {
                return false;
            }
            let (k, dy) = ((y - 4) / 7, (y - 4) % 7);
            if k >= rows {
                return false;
            }
            let bump = (3..5).contains(&dy) && (13..side - 13).contains(&x) && (x - 13).is_multiple_of(4);
            let turn = dy >= 3 && k + 1 < rows && if k.is_multiple_of(2) { x >= side - 7 } else { x < 7 };
            dy < 3 || bump || turn
        };
        let line = line_of(&map(side, side, on));
        assert_eq!((line.junctions, line.ends, line.pieces.len()), (0, 2, 1));
    }

    /// A chain of nodes of two pieces each is joined into one piece, its pixels in order, every
    /// other piece drawn the other way, in time in proportion to its pixels: 200,000 pieces, which
    /// joined by copying the piece built so far at each node moved about 4 × 10¹⁰ pixels.
    #[test]
    fn a_chain_of_nodes_is_joined_in_time() {
        let n = 200_000usize;
        let mut edges: Vec<Edge> = (0..n)
            .map(|k| {
                let pixels = vec![2 * k, 2 * k + 1, 2 * k + 2];
                if k.is_multiple_of(2) {
                    Edge { a: k, b: k + 1, pixels, gone: false }
                } else {
                    Edge { a: k + 1, b: k, pixels: pixels.into_iter().rev().collect(), gone: false }
                }
            })
            .collect();
        let mut lengths = vec![2.0; n];
        let started = std::time::Instant::now();
        assert!(join_through(n + 1, &mut edges, &mut lengths, &mut Ticks::default()).unwrap());
        let took = started.elapsed();
        let live: Vec<&Edge> = edges.iter().filter(|e| !e.gone).collect();
        assert_eq!(live.len(), 1);
        assert_eq!((live[0].a, live[0].b), (0, n));
        assert!(live[0].pixels.iter().copied().eq(0..=2 * n), "the pixels in order");
        assert_eq!(lengths[0], 2.0 * n as f64);
        assert!(took.as_secs_f64() < 10.0, "{took:?}");
    }

    /// Two pieces joined through their node are the first toward it and then the second away from
    /// it, the node's pixel once, whichever way each is drawn and whichever is longer.
    #[test]
    fn two_pieces_join_in_order_whichever_way_they_run() {
        for (short_first, a_turned, b_turned) in (0..8).map(|k| (k & 1 == 1, k & 2 == 2, k & 4 == 4)) {
            let (la, lb) = if short_first { (3, 9) } else { (9, 3) };
            let first: Vec<usize> = (0..la).collect();
            let second: Vec<usize> = (la - 1..la - 1 + lb).collect();
            let edge = |a: usize, b: usize, pixels: &[usize], turned: bool| {
                if turned {
                    Edge { a: b, b: a, pixels: pixels.iter().rev().copied().collect(), gone: false }
                } else {
                    Edge { a, b, pixels: pixels.to_vec(), gone: false }
                }
            };
            let mut edges = vec![edge(0, 1, &first, a_turned), edge(1, 2, &second, b_turned)];
            let mut lengths = vec![1.0, 1.0];
            assert!(join_through(3, &mut edges, &mut lengths, &mut Ticks::default()).unwrap());
            let e = &edges[0];
            let want: Vec<usize> = (0..la + lb - 1).collect();
            let (from, to, got) = if e.a == 0 { (e.a, e.b, e.pixels.clone()) } else { (e.b, e.a, e.pixels.iter().rev().copied().collect()) };
            assert_eq!((from, to, got), (0, 2, want), "short first {short_first}, turned {a_turned} {b_turned}");
            assert!(edges[1].gone);
        }
    }

    /// The graph's walk, the pruning and joining, and the smoothing each stop at a cancel within
    /// [`CANCEL_EVERY`] of their work: each given work just short of a check, a cancel set.
    #[test]
    fn every_step_stops_at_a_cancel() {
        use vectorcraft_doc::cancel::{self, CancelToken};
        let regions = map(60, 60, |x, y| (20..23).contains(&y) || (20..23).contains(&x));
        let mut alive: Vec<bool> = regions.labels.iter().map(|&l| l == 0).collect();
        let pixels: Vec<usize> = (0..alive.len()).filter(|&i| alive[i]).collect();
        let d2 = vec![1.0; alive.len()];
        let token = CancelToken::new();
        token.cancel();
        let almost = || Ticks(CANCEL_EVERY - 1);
        let mut line = Line { regions: &regions, label: 0, alive: &mut alive };
        thinned(&mut line, &pixels).unwrap();
        let skeleton: Vec<usize> = pixels.iter().copied().filter(|&i| line.on(i)).collect();
        assert!(cancel::scope(&token, None, || graph(&line, &skeleton, &d2, &mut almost())).is_err(), "the graph");
        let (mut nodes, mut edges) = graph(&line, &skeleton, &d2, &mut Ticks::default()).unwrap();
        assert!(!edges.is_empty(), "a cross has arms");
        assert!(cancel::scope(&token, None, || pruned(&mut nodes, &mut edges, 60, 3.0, &mut almost())).is_err(), "the pruning");
        let pts: Vec<Point> = (0..50).map(|k| Point::new(f64::from(k), f64::from(k % 2))).collect();
        assert!(cancel::scope(&token, None, || smoothed(&pts, false, 8.0, &mut almost())).is_err(), "the smoothing");
        assert!(smoothed(&pts, false, 8.0, &mut almost()).is_ok(), "not cancelled");
    }

    /// Thinning looks again only beside what it took away: a disc joined by a column to a long
    /// serpentine (its skeleton found early, while the disc takes as many passes as its radius) is
    /// looked at about as often a pixel at four times the size as at one (2.4 times a pixel at 150
    /// and 600 pixels), where every pixel kept was looked at again at every pass (12 and 44).
    #[test]
    fn thinning_looks_again_only_beside_what_it_took() {
        let looks = |side: u32| {
            let r = side / 3;
            let (cx, cy) = (r + 2, r + 2);
            let on = move |x: u32, y: u32| {
                let disc = (f64::from(x) - f64::from(cx)).hypot(f64::from(y) - f64::from(cy)) <= f64::from(r);
                // A column from the disc's right to the serpentine, and the serpentine's bars every
                // 6 rows on the right two thirds, turning at alternate ends.
                let column = (cx..2 * r + 6).contains(&x) && (cy..cy + 3).contains(&y);
                let (x0, x1) = (2 * r + 6, side - 2);
                let k = y / 6;
                let bar = (x0..x1).contains(&x) && y % 6 < 3 && y >= 2 && y + 2 < side;
                let turn = y % 6 >= 3 && y + 6 < side && if k.is_multiple_of(2) { x + 3 >= x1 && x < x1 } else { (x0..x0 + 3).contains(&x) };
                disc || column || bar || turn
            };
            let regions = map(side, side, on);
            let mut alive: Vec<bool> = regions.labels.iter().map(|&l| l == 0).collect();
            let pixels: Vec<usize> = (0..alive.len()).filter(|&i| alive[i]).collect();
            let mut line = Line { regions: &regions, label: 0, alive: &mut alive };
            thinned(&mut line, &pixels).unwrap() as f64 / pixels.len() as f64
        };
        let (small, large) = (looks(150), looks(600));
        assert!(large < 2.0 * small, "{large} looks a pixel at 600 pixels, {small} at 150");
    }

    /// The wide stretches of a line held to a width are looked for through its junctions only at
    /// its pixels past the width: a piece of a million pixels no wider than the limit, among 5,000
    /// junctions (a mesh's), is passed within a quarter of a second (measured 2026-11-08: 2 ms),
    /// where looking through every junction at every pixel took 2.5 s.
    #[test]
    fn a_narrow_line_among_many_junctions_is_passed_at_once() {
        let w = 2_000usize;
        let nodes: Vec<Node> =
            (0..5_000).map(|k| Node { at: Point::new(f64::from(k % 100) * 20.0, f64::from(k / 100) * 20.0), radius: 4.0, junction: true }).collect();
        let deg = vec![3usize; nodes.len()];
        let edges = vec![Edge { a: 0, b: 1, pixels: (0..1_000_000).collect(), gone: false }];
        let started = std::time::Instant::now();
        assert!(!widens_away_from_junctions(&nodes, &edges, &deg, &|_| 1.0, 5.0, w));
        assert!(started.elapsed().as_secs_f64() < 0.25, "{:?}", started.elapsed());
    }
}
