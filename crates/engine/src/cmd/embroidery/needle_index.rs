//! The needle points stitch editing works on, found fast ([`NeedleIndex`]): the paths of stitches
//! inside the object isolated (a converted object, an opened design's thread block, a run), built
//! from the document, so that it always answers for the document as it is: a press right after an
//! edit never lands where a needle point was before it.
//!
//! - **Bounded to the paths being edited**, never the whole design: each path's needle points are
//!   cut into chunks of at most [`CHUNK_POINTS`] (as many as a line of the stitch scene holds), each
//!   with the box its needle points (and the stitch leaving its last) fall in, so a query tests the
//!   boxes first and the points of a few chunks.
//! - **Kept by node identity:** a path's chunks are built again only when its node is another one
//!   (an edit copies only the paths it changes), so after an edit the index costs what the edited
//!   paths hold, not the design. The engine keeps one index a document while the embroidery tool
//!   works in isolation of an object holding stitches, and drops it when that ends
//!   ([`super::service::Services::needle_index`]).
//! - **The needle points are the anchors** of each path, in order, as the stitch commands address
//!   them ([`super::needles`]): a line to its own start is one needle point.
//! - **Shown but not edited:** a path locked or hidden (or in a locked layer, group or artboard), a
//!   machine file's run cut by a clip group (manual stitches never are), one whose anchors aren't
//!   numbers or lie outside the document's coordinates, or one holding more needle points than a
//!   design may have: counted, never indexed.

use std::collections::HashMap;
use std::sync::Arc;

use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_geom::{Point, Rect};
use vectorcraft_tools::embroidery::{NeedleHit, NeedleRef, NeedleScope, ThreadHit};

use super::needles::{self, MAX_NEEDLE_POINTS, Unfit};

/// The most needle points a chunk of a path's index holds: as many as a line of the stitch scene
/// (`vectorcraft_render::lines::MAX_LINE_POINTS`), so that a query tests as many points a box as a
/// press on the stitches does.
pub const CHUNK_POINTS: usize = vectorcraft_render::lines::MAX_LINE_POINTS;

/// The most points a lasso may have (a selection by a polygon): a loop round a 4K display at a
/// screen pixel a point, after the loop is simplified. An estimate.
pub const MAX_LASSO_POINTS: usize = 4096;

/// The most horizontal bands a lasso's edges are sorted into: a needle point is tested against the
/// edges of its band alone, not every edge. An estimate (a band a screen pixel high on a 4K display
/// is narrower than any edge needs).
const LASSO_BANDS: usize = 256;

thread_local! {
    /// How many paths' chunks were built on this thread: what the tests count, so that an edit of
    /// one path builds one path's again, and a query or an idle frame builds none.
    static BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many paths' chunks were built on this thread ([`BUILT`]).
#[cfg(test)]
pub(crate) fn paths_built() -> usize {
    BUILT.with(std::cell::Cell::get)
}

/// One subpath of a path of stitches: its first needle point's index in the path, how many needle
/// points it holds, and whether it is a line to its own start (two anchors, one needle point).
#[derive(Clone, Copy, Debug)]
struct Sub {
    first: usize,
    needles: usize,
    collapsed: bool,
}

/// At most [`CHUNK_POINTS`] needle points of one subpath: anchors `from..from + len` (a line to its
/// own start: anchor 0 alone), and the box they and the needle point after the last (the stitch
/// leaving it) fall in, document points.
#[derive(Clone, Copy, Debug)]
struct Chunk {
    sub: usize,
    from: usize,
    len: usize,
    bounds: Rect,
}

/// One path of stitches as the index finds its needle points: the node it was built from (its
/// identity says whether it is still the document's), its subpaths and chunks, how many needle
/// points it holds, and the box they fall in.
pub struct PathIndex {
    node: Arc<Node>,
    subs: Vec<Sub>,
    chunks: Vec<Chunk>,
    needles: usize,
    bounds: Option<Rect>,
}

impl PathIndex {
    /// The index of path `node`; `Err` with why it can't be edited needle point by needle point,
    /// as the stitch commands judge it ([`needles::check`]: before any arithmetic).
    fn of(node: &Arc<Node>) -> Result<Self, Unfit> {
        needles::check(node)?;
        let NodeKind::Path { path, .. } = &node.kind else { return Err(Unfit::NotStitches) };
        let Some(marks) = node.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()) else { return Err(Unfit::NotStitches) };
        BUILT.with(|b| b.set(b.get().saturating_add(1)));
        let alone = path.subpaths.len() == 1;
        let mut subs = Vec::with_capacity(path.subpaths.len());
        let mut chunks = Vec::new();
        let mut needles = 0usize;
        let mut bounds: Option<Rect> = None;
        for (s, sp) in path.subpaths.iter().enumerate() {
            // A line to its own start is one needle point, as every reader counts it.
            let count = needles::needle_count(&sp.anchors, alone, marks.double_point);
            let collapsed = count < sp.anchors.len();
            subs.push(Sub { first: needles, needles: count, collapsed });
            let mut from = 0;
            while from < count {
                let len = (count - from).min(CHUNK_POINTS);
                // The stitch leaving the chunk's last needle point is caught by it too.
                let last = (from + len).min(count - 1);
                let b = sp.anchors.get(from..=last).unwrap_or_default().iter().map(|a| Rect::from_points(a.p, a.p)).reduce(|a, b| a.union(b));
                if let Some(b) = b {
                    chunks.push(Chunk { sub: s, from, len, bounds: b });
                    bounds = Some(bounds.map_or(b, |o| o.union(b)));
                }
                from += len;
            }
            needles += count;
        }
        if needles > MAX_NEEDLE_POINTS {
            return Err(Unfit::TooMany);
        }
        Ok(Self { node: node.clone(), subs, chunks, needles, bounds })
    }

    /// The path's anchors of subpath `s`.
    fn anchors(&self, s: usize) -> &[vectorcraft_geom::Anchor] {
        match &self.node.kind {
            NodeKind::Path { path, .. } => path.subpaths.get(s).map_or(&[], |sp| sp.anchors.as_slice()),
            _ => &[],
        }
    }

    /// Needle point `index`'s subpath and its place among the subpath's needle points.
    fn locate(&self, index: usize) -> Option<(usize, usize)> {
        let s = self.subs.partition_point(|sub| sub.first + sub.needles <= index);
        let sub = self.subs.get(s)?;
        (index >= sub.first && index < sub.first + sub.needles).then(|| (s, index - sub.first))
    }

    /// Where needle point `index` is.
    fn point(&self, index: usize) -> Option<Point> {
        let (s, i) = self.locate(index)?;
        self.anchors(s).get(i).map(|a| a.p)
    }

    /// The anchors needle point `index` is: one, or both of a line to its own start.
    fn anchors_of(&self, index: usize) -> Vec<(usize, usize)> {
        match self.locate(index) {
            Some((s, _)) if self.subs.get(s).is_some_and(|sub| sub.collapsed) => vec![(s, 0), (s, 1)],
            Some((s, i)) => vec![(s, i)],
            None => Vec::new(),
        }
    }

    /// The needle point anchor `(s, a)` is (the second anchor of a line to its own start is its one
    /// needle point).
    fn of_anchor(&self, (s, a): (usize, usize)) -> Option<usize> {
        let sub = self.subs.get(s)?;
        let i = if sub.collapsed { 0 } else { a };
        (i < sub.needles).then_some(sub.first + i)
    }

    /// The needle points of its chunks whose box `test_box` passes, each tested by `test`: `(index,
    /// point)` to `f`.
    fn each_in(&self, test_box: impl Fn(Rect) -> bool, mut f: impl FnMut(usize, Point)) {
        for c in &self.chunks {
            if !test_box(c.bounds) {
                continue;
            }
            let Some(sub) = self.subs.get(c.sub) else { continue };
            for (k, a) in self.anchors(c.sub).iter().enumerate().skip(c.from).take(c.len) {
                f(sub.first + if sub.collapsed { 0 } else { k }, a.p);
            }
        }
    }
}

/// The paths of stitches inside an object (or several, or the whole document): those whose needle
/// points can be edited, in document order (the order they are sewn in, within the object), and
/// how many more are shown but can't be edited.
pub struct NeedleIndex {
    object: NodeId,
    paths: Vec<(NodeId, Arc<PathIndex>)>,
    order: HashMap<NodeId, usize>,
    refused: usize,
    needles: usize,
    bounds: Option<Rect>,
}

/// The node `id` of `doc`, as its parent holds it (its identity is what an edit changes).
fn arc_of(doc: &Document, id: NodeId) -> Option<Arc<Node>> {
    let siblings = match doc.parent_of(id) {
        Some(parent) => doc.children(Some(parent))?,
        None => &doc.layers,
    };
    siblings.iter().find(|n| n.id == id).cloned()
}

/// Whether `n` is a path holding a run of literal stitches.
fn holds_stitches(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Path { .. }) && n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some())
}

/// Whether `n`'s run of stitches is manual (converted in VectorCraft), not a machine file's.
fn manual(n: &Node) -> bool {
    n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|s| s.manual)
}

impl NeedleIndex {
    /// The index of the paths of stitches inside `roots` of `doc` (each root itself when it is one),
    /// each path reusing `kept`'s chunks when its node is the same; `None` when they hold no path of
    /// stitches at all. `roots`' first is the object it says it edits.
    pub fn of(doc: &Document, roots: &[NodeId], kept: Option<&NeedleIndex>) -> Option<Self> {
        let editable = doc.editable();
        let mut paths: Vec<(NodeId, Arc<PathIndex>)> = Vec::new();
        let mut order = HashMap::new();
        let mut refused = 0usize;
        // A node, whether a clip group holds it, and whether it is reached already.
        fn walk(n: &Arc<Node>, clipped: bool, out: &mut Vec<(Arc<Node>, bool)>) {
            if holds_stitches(n) {
                out.push((n.clone(), clipped));
                return;
            }
            let clips = clipped || n.clips();
            for c in n.children().into_iter().flatten() {
                walk(c, clips, out);
            }
        }
        let mut found: Vec<(Arc<Node>, bool)> = Vec::new();
        for &root in roots {
            let Some(n) = arc_of(doc, root) else { continue };
            let clipped = doc.ancestry(root).unwrap_or_default().iter().filter(|a| **a != root).any(|a| doc.node(*a).is_some_and(Node::clips));
            walk(&n, clipped, &mut found);
        }
        if found.is_empty() {
            return None;
        }
        let mut needles = 0usize;
        let mut bounds: Option<Rect> = None;
        for (n, clipped) in found {
            if order.contains_key(&n.id) {
                continue;
            }
            if !editable(n.id) || (clipped && !manual(&n)) {
                refused += 1;
                continue;
            }
            let reused = kept.and_then(|k| k.path(n.id)).filter(|p| Arc::ptr_eq(&p.node, &n));
            let path = match reused {
                Some(p) => p,
                None => match PathIndex::of(&n) {
                    Ok(p) => Arc::new(p),
                    Err(_) => {
                        refused += 1;
                        continue;
                    }
                },
            };
            if needles.saturating_add(path.needles) > MAX_NEEDLE_POINTS {
                refused += 1;
                continue;
            }
            needles += path.needles;
            bounds = match (bounds, path.bounds) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            };
            order.insert(n.id, paths.len());
            paths.push((n.id, path));
        }
        Some(Self { object: roots.first().copied().unwrap_or(NodeId(0)), paths, order, refused, needles, bounds })
    }

    /// Path `id`'s index, when it is one being edited.
    fn path(&self, id: NodeId) -> Option<Arc<PathIndex>> {
        self.order.get(&id).and_then(|&k| self.paths.get(k)).map(|(_, p)| p.clone())
    }

    /// What it edits.
    pub fn scope(&self) -> NeedleScope {
        NeedleScope { object: self.object, paths: self.paths.len(), needles: self.needles, bounds: self.bounds, refused: self.refused }
    }

    /// Whether path `id` is one being edited.
    pub(crate) fn holds(&self, id: NodeId) -> bool {
        self.order.contains_key(&id)
    }

    /// The paths being edited, in order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.paths.iter().map(|(id, _)| *id)
    }

    /// Path `id`'s place among the paths being edited (the order they are sewn in).
    pub(crate) fn order_of(&self, id: NodeId) -> Option<usize> {
        self.order.get(&id).copied()
    }

    /// How many needle points path `id` holds (0: not one being edited).
    pub(crate) fn count(&self, id: NodeId) -> usize {
        self.path(id).map_or(0, |p| p.needles)
    }

    /// The needle point nearest `at` within `tol`; of equally near ones, the one sewn later.
    pub fn needle_at(&self, at: Point, tol: f64) -> Option<NeedleHit> {
        if !(at.x.is_finite() && at.y.is_finite() && tol.is_finite() && tol >= 0.0) {
            return None;
        }
        let mut best: Option<(f64, NeedleHit)> = None;
        for (id, p) in &self.paths {
            if !p.bounds.is_some_and(|b| b.inflate(tol, tol).contains(at)) {
                continue;
            }
            p.each_in(
                |b| b.inflate(tol, tol).contains(at),
                |index, q| {
                    let d = q.distance(at);
                    if d <= tol && best.is_none_or(|(e, _)| d <= e) {
                        best = Some((d, NeedleHit { needle: NeedleRef { id: *id, index }, point: q }));
                    }
                },
            );
        }
        best.map(|(_, h)| h)
    }

    /// The stitch nearest `at` within `tol` (between two needle points of one run), and the place on
    /// it nearest `at`; of equally near ones, the one sewn later.
    pub fn thread_at(&self, at: Point, tol: f64) -> Option<ThreadHit> {
        if !(at.x.is_finite() && at.y.is_finite() && tol.is_finite() && tol >= 0.0) {
            return None;
        }
        let mut best: Option<(f64, ThreadHit)> = None;
        for (id, p) in &self.paths {
            if !p.bounds.is_some_and(|b| b.inflate(tol, tol).contains(at)) {
                continue;
            }
            for c in &p.chunks {
                if !c.bounds.inflate(tol, tol).contains(at) {
                    continue;
                }
                let Some(sub) = p.subs.get(c.sub).filter(|s| !s.collapsed && s.needles > 1) else { continue };
                let anchors = p.anchors(c.sub);
                for k in c.from..(c.from + c.len).min(sub.needles - 1) {
                    let (Some(a), Some(b)) = (anchors.get(k), anchors.get(k + 1)) else { continue };
                    let q = nearest_on(a.p, b.p, at);
                    let d = q.distance(at);
                    if d <= tol && best.is_none_or(|(e, _)| d <= e) {
                        best = Some((d, ThreadHit { from: NeedleRef { id: *id, index: sub.first + k }, ends: [a.p, b.p], point: q }));
                    }
                }
            }
        }
        best.map(|(_, h)| h)
    }

    /// Where needle point `n` is.
    pub(crate) fn point(&self, n: NeedleRef) -> Option<Point> {
        self.path(n.id)?.point(n.index)
    }

    /// The needle point after `n` along the thread (`forward`), or before it, round from the last
    /// path's last to the first path's first.
    pub(crate) fn step(&self, n: NeedleRef, forward: bool) -> Option<NeedleRef> {
        let &k = self.order.get(&n.id)?;
        let count = self.paths.get(k)?.1.needles;
        if forward && n.index + 1 < count {
            return Some(NeedleRef { id: n.id, index: n.index + 1 });
        }
        if !forward && n.index > 0 && n.index <= count {
            return Some(NeedleRef { id: n.id, index: n.index - 1 });
        }
        // On to the next path that holds a needle point, or back to the one before.
        let m = self.paths.len();
        (1..=m).find_map(|d| {
            let j = if forward { (k + d) % m } else { (k + m - d % m) % m };
            let (id, p) = self.paths.get(j)?;
            (p.needles > 0).then(|| NeedleRef { id: *id, index: if forward { 0 } else { p.needles - 1 } })
        })
    }

    /// The first and the last needle point of the run `n` is in (their places in its path).
    pub(crate) fn run(&self, n: NeedleRef) -> Option<(usize, usize)> {
        let p = self.path(n.id)?;
        let (s, _) = p.locate(n.index)?;
        let sub = p.subs.get(s)?;
        Some((sub.first, sub.first + sub.needles.checked_sub(1)?))
    }

    /// The first needle point of the first path holding one.
    pub(crate) fn first(&self) -> Option<NeedleRef> {
        self.paths.iter().find(|(_, p)| p.needles > 0).map(|(id, _)| NeedleRef { id: *id, index: 0 })
    }

    /// The anchors needle point `n` is.
    pub(crate) fn anchors(&self, n: NeedleRef) -> Vec<(usize, usize)> {
        self.path(n.id).map_or_else(Vec::new, |p| p.anchors_of(n.index))
    }

    /// The needle point anchor `anchor` of path `id` is.
    pub(crate) fn of_anchor(&self, id: NodeId, anchor: (usize, usize)) -> Option<usize> {
        self.path(id)?.of_anchor(anchor)
    }

    /// The needle points inside `area` (document points), path by path, in order.
    pub fn within(&self, area: &Area) -> Vec<(NodeId, Vec<usize>)> {
        let mut out = Vec::new();
        for (id, p) in &self.paths {
            if !p.bounds.is_some_and(|b| area.overlaps(b)) {
                continue;
            }
            let mut inside = Vec::new();
            p.each_in(
                |b| area.overlaps(b),
                |index, q| {
                    if area.contains(q) && inside.last() != Some(&index) {
                        inside.push(index);
                    }
                },
            );
            if !inside.is_empty() {
                out.push((*id, inside));
            }
        }
        out
    }
}

/// The point of segment `a`–`b` nearest `p`.
fn nearest_on(a: Point, b: Point, p: Point) -> Point {
    let ab = b - a;
    let len2 = ab.hypot2();
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

/// What a selection by a shape takes: the needle points inside a rectangle, or inside a polygon (a
/// lasso: inside by the even-odd rule, as the Lasso tool selects anchors), document points.
pub enum Area {
    Rect(Rect),
    Lasso(Lasso),
}

impl Area {
    /// Whether `p` lies inside it.
    fn contains(&self, p: Point) -> bool {
        match self {
            Area::Rect(r) => p.x >= r.x0 && p.x <= r.x1 && p.y >= r.y0 && p.y <= r.y1,
            Area::Lasso(l) => l.contains(p),
        }
    }

    /// Whether box `b` may hold a point inside it.
    fn overlaps(&self, b: Rect) -> bool {
        let r = match self {
            Area::Rect(r) => *r,
            Area::Lasso(l) => l.bounds,
        };
        b.x1 >= r.x0 && b.x0 <= r.x1 && b.y1 >= r.y0 && b.y0 <= r.y1
    }
}

/// A polygon whose edges are sorted into horizontal bands ([`LASSO_BANDS`]), so that a point is
/// tested against the few edges of its band.
pub struct Lasso {
    bounds: Rect,
    bands: Vec<Vec<(Point, Point)>>,
    band_height: f64,
}

impl Lasso {
    /// The lasso through `points` (closed back to its first); `None` with fewer than three points,
    /// a point that isn't a number, or no area.
    pub fn new(points: &[Point]) -> Option<Self> {
        if points.len() < 3 || !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let first = *points.first()?;
        let bounds = points.iter().map(|p| Rect::from_points(*p, *p)).reduce(|a, b| a.union(b))?;
        if !(bounds.height() > 0.0 && bounds.width() > 0.0) {
            return None;
        }
        let count = LASSO_BANDS.min(points.len());
        let band_height = bounds.height() / count as f64;
        let mut bands = vec![Vec::new(); count];
        let band = |y: f64| (((y - bounds.y0) / band_height).floor().max(0.0) as usize).min(count - 1);
        for (k, &a) in points.iter().enumerate() {
            let b = points.get(k + 1).copied().unwrap_or(first);
            if a.y == b.y {
                continue;
            }
            let (lo, hi) = (band(a.y.min(b.y)), band(a.y.max(b.y)));
            for slot in bands.iter_mut().take(hi + 1).skip(lo) {
                slot.push((a, b));
            }
        }
        Some(Self { bounds, bands, band_height })
    }

    /// Whether `p` lies inside (even-odd: a ray to the right crosses an odd number of edges).
    fn contains(&self, p: Point) -> bool {
        if !(p.x >= self.bounds.x0 && p.x <= self.bounds.x1 && p.y >= self.bounds.y0 && p.y <= self.bounds.y1) {
            return false;
        }
        let count = self.bands.len();
        let k = (((p.y - self.bounds.y0) / self.band_height).floor().max(0.0) as usize).min(count.saturating_sub(1));
        let mut inside = false;
        for &(a, b) in self.bands.get(k).map_or(&[][..], Vec::as_slice) {
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

#[cfg(test)]
mod tests;
