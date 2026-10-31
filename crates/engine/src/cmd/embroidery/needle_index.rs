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
//!   (an edit copies only the paths it changes), and a path refused for what it holds is refused
//!   again without a look while it is the same node, so after an edit the index costs a walk of the
//!   object's nodes and what the edited paths hold, not the design. The engine keeps one index a
//!   document while the embroidery tool works in isolation of an object holding stitches, as it is
//!   through a gesture under way (a drag's previews), serves a selection by a shape in the object
//!   isolated from it, and drops it when that ends ([`super::service::Services::needle_index`]).
//! - **The needle points are the anchors** of each path, in order, as the stitch commands address
//!   them ([`super::needles`]): a line to its own start is one needle point.
//! - **Shown but not edited:** a path locked or hidden (or in a locked layer, group or artboard), a
//!   machine file's run cut by a clip group (manual stitches never are), one whose anchors aren't
//!   numbers or lie outside the document's coordinates, or one holding more needle points than a
//!   design may have: counted, never indexed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use vectorcraft_doc::{Document, Node, NodeId, NodeKind, Selection};
use vectorcraft_geom::{Point, Rect};
use vectorcraft_tools::embroidery::{NeedleHit, NeedleRef, NeedleScope, ThreadHit};

use super::digitize;
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
/// The most entries a lasso's bands hold in all (each edge in every band it crosses): as if each of
/// [`MAX_LASSO_POINTS`] edges crossed a quarter of the bands, 8 MB. A polygon past it (a zigzag whose
/// every edge spans its whole height puts a million there) is refused before anything is tested. An
/// estimate.
pub const LASSO_ENTRIES_MAX: usize = MAX_LASSO_POINTS * LASSO_BANDS / 4;
/// The most edge tests a selection by a lasso makes (each needle point it looks at against the edges
/// of its band): past it the selection stops and is refused, nothing selected. About a second at a
/// few nanoseconds a test. An estimate.
pub const LASSO_TESTS_MAX: usize = 200_000_000;
/// How many needle points a selection by a shape looks at between two looks at whether its work was
/// cancelled.
const CANCEL_EVERY: usize = 1 << 16;

thread_local! {
    /// How many paths' chunks were built on this thread: what the tests count, so that an edit of
    /// one path builds one path's again, and a query or an idle frame builds none.
    static BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many paths were looked at to be built (refused or not) on this thread: the tests count
    /// that a path refused for what it holds isn't looked at again while it is the same node.
    static LOOKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many paths were looked at to be built on this thread ([`LOOKED`]).
#[cfg(test)]
pub(crate) fn paths_looked_at() -> usize {
    LOOKED.with(std::cell::Cell::get)
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
/// points it holds, the box they fall in, and the needle points the machine stops after (the
/// needle is then lifted to the next: no stitch between them).
pub struct PathIndex {
    node: Arc<Node>,
    subs: Vec<Sub>,
    chunks: Vec<Chunk>,
    needles: usize,
    bounds: Option<Rect>,
    stops: std::collections::BTreeSet<usize>,
}

impl PathIndex {
    /// The index of path `node`; `Err` with why it can't be edited needle point by needle point,
    /// as the stitch commands judge it ([`needles::check`]: before any arithmetic).
    fn of(node: &Arc<Node>) -> Result<Self, Unfit> {
        LOOKED.with(|b| b.set(b.get().saturating_add(1)));
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
        // The stops where their needle points are now, as the walk places them; one named past a
        // subpath's last needle point (the second of a line to its own start) is that one's.
        let stops = marks
            .placed(path)
            .stops
            .iter()
            .filter_map(|&(s, a)| subs.get(s).filter(|sub| sub.needles > 0).map(|sub| sub.first + a.min(sub.needles - 1)))
            .collect();
        Ok(Self { node: node.clone(), subs, chunks, needles, bounds, stops })
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
    /// The paths refused for what they hold (out of range, too many needle points, not a run),
    /// by node: refused again without a look while they are the same node.
    unfit: HashMap<NodeId, Arc<Node>>,
}

/// A node found for the index: the node, whether a clip group holds it, and whether the walk
/// sews what is above it (shown, on a layer that prints: [`digitize::shows`], [`digitize::printed`]).
type Found = (Arc<Node>, bool, bool);

/// The node at index path `path` of `doc`, as its parent holds it (its identity is what an edit
/// changes), whether a clip group above it holds it, and whether the walk sews what is above it.
fn arc_at(doc: &Document, path: &[usize]) -> Option<Found> {
    let (last, up) = path.split_last()?;
    let mut nodes = &doc.layers;
    let (mut clipped, mut sewn) = (false, true);
    for &i in up {
        let n = nodes.get(i)?;
        clipped |= n.clips();
        sewn &= digitize::shows(n) && digitize::printed(n);
        nodes = n.children()?;
    }
    nodes.get(*last).map(|n| (n.clone(), clipped, sewn))
}

/// `roots` of `doc` each once, and only the outermost (one inside another is walked with it), with
/// whether a clip group holds them: so each node is walked once however often, or however nested,
/// `ids` names it. Looked up in one walk of the document, so an id that isn't in it costs nothing
/// more.
fn outermost(doc: &Document, roots: &[NodeId]) -> Vec<Found> {
    let found = doc.index_paths(roots.iter().copied());
    let mut seen: HashSet<NodeId> = HashSet::with_capacity(found.len());
    let mut out = Vec::new();
    for &root in roots {
        let Some(path) = found.get(&root) else { continue };
        if !seen.insert(root) {
            continue;
        }
        let above = doc.ancestry_at(path).unwrap_or_default();
        if above.iter().any(|a| *a != root && found.contains_key(a)) {
            continue;
        }
        if let Some(n) = arc_at(doc, path) {
            out.push(n);
        }
    }
    out
}

/// How many needle points path `n` has, as [`PathIndex::of`] counts them (0 when it isn't a path
/// of stitches): weighed against the budget before the path is indexed.
fn needles_in(n: &Node) -> usize {
    let (NodeKind::Path { path, .. }, Some(marks)) = (&n.kind, n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref())) else {
        return 0;
    };
    let alone = path.subpaths.len() == 1;
    path.subpaths.iter().map(|sp| needles::needle_count(&sp.anchors, alone, marks.double_point)).fold(0, usize::saturating_add)
}

/// Whether `n` is a path holding a run of literal stitches.
fn holds_stitches(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Path { .. }) && n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some())
}

impl NeedleIndex {
    /// The index of the paths of stitches inside `roots` of `doc` (each root itself when it is one;
    /// each root once, and none inside another: [`outermost`]), each path reusing `kept`'s chunks
    /// when its node is the same; `None` when they hold no path of stitches at all. `roots`' first
    /// is the object it says it edits.
    pub fn of(doc: &Document, roots: &[NodeId], kept: Option<&NeedleIndex>) -> Option<Self> {
        let editable = doc.editable();
        let mut paths: Vec<(NodeId, Arc<PathIndex>)> = Vec::new();
        let mut order = HashMap::new();
        let mut refused = 0usize;
        let mut unfit: HashMap<NodeId, Arc<Node>> = HashMap::new();
        // The paths of stitches under a node, each with whether a clip group holds it and whether
        // the walk sews it, by the walk's own rules: into layers and groups only (a compound path is
        // sewn as a shape; blends, envelopes and repeats aren't sewn), nothing hidden, at no
        // opacity or on a layer that doesn't print, no guide, no run without a colour.
        fn walk(n: &Arc<Node>, clipped: bool, sewn: bool, out: &mut Vec<Found>) {
            let sewn = sewn && digitize::shows(n) && digitize::printed(n);
            if holds_stitches(n) {
                let guide = matches!(n.kind, NodeKind::Path { guide: true, .. });
                out.push((n.clone(), clipped, sewn && !guide && digitize::run_colour(n).is_some()));
                return;
            }
            if !matches!(n.kind, NodeKind::Layer { .. } | NodeKind::Group { .. }) {
                return;
            }
            let clips = clipped || n.clips();
            for c in n.children().into_iter().flatten() {
                walk(c, clips, sewn, out);
            }
        }
        let mut found: Vec<Found> = Vec::new();
        for (n, clipped, sewn) in outermost(doc, roots) {
            walk(&n, clipped, sewn, &mut found);
        }
        if found.is_empty() {
            return None;
        }
        let mut needles = 0usize;
        let mut bounds: Option<Rect> = None;
        for (n, clipped, sewn) in found {
            if order.contains_key(&n.id) {
                continue;
            }
            // Sewn and drawn, where it lies as the stitch commands judge it (its needle points
            // next), and within what the budget has left: past it, a path isn't indexed at all.
            if !sewn || needles::placed_unfit(&n, editable(n.id), clipped).is_some() || needles.saturating_add(needles_in(&n)) > MAX_NEEDLE_POINTS {
                refused += 1;
                continue;
            }
            if kept.and_then(|k| k.unfit.get(&n.id)).is_some_and(|u| Arc::ptr_eq(u, &n)) {
                refused += 1;
                unfit.insert(n.id, n);
                continue;
            }
            let reused = kept.and_then(|k| k.path(n.id)).filter(|p| Arc::ptr_eq(&p.node, &n));
            let path = match reused {
                Some(p) => p,
                None => match PathIndex::of(&n) {
                    Ok(p) => Arc::new(p),
                    Err(_) => {
                        refused += 1;
                        unfit.insert(n.id, n);
                        continue;
                    }
                },
            };
            needles += path.needles;
            bounds = match (bounds, path.bounds) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, b) => a.or(b),
            };
            order.insert(n.id, paths.len());
            paths.push((n.id, path));
        }
        Some(Self { object: roots.first().copied().unwrap_or(NodeId(0)), paths, order, refused, needles, bounds, unfit })
    }

    /// The paths being edited that `sel` selects, in the selection's order, each with whether it
    /// selects all of the path's needle points (the path, or an object holding it, selected whole)
    /// or only those whose anchors it selects: the one rule the chip, the selection layer, the live
    /// check, the tool's keys and the stitch commands' selection share (`needles::targets` takes a
    /// group selected whole as the paths of stitches in it).
    pub fn selected_paths(&self, doc: &Document, sel: &Selection) -> Vec<(NodeId, bool)> {
        let mut seen: HashSet<NodeId> = HashSet::new();
        let mut out = Vec::new();
        for &id in &sel.objects {
            if self.holds(id) {
                if seen.insert(id) {
                    out.push((id, !sel.anchors.contains_key(&id)));
                }
                continue;
            }
            let Some(n) = doc.node(id) else { continue };
            n.walk(&mut |c| {
                if c.id != id && self.holds(c.id) && seen.insert(c.id) {
                    out.push((c.id, true));
                }
            });
        }
        out
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
                    // After a stop the needle is lifted to the next: no stitch.
                    if p.stops.contains(&(sub.first + k)) {
                        continue;
                    }
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

    /// The needle points inside `area` (document points), path by path, in order; `None` when a
    /// lasso would test past [`LASSO_TESTS_MAX`] edges, or the work was cancelled.
    pub fn within(&self, area: &Area) -> Option<Vec<(NodeId, Vec<usize>)>> {
        self.within_tested(area, LASSO_TESTS_MAX)
    }

    /// [`NeedleIndex::within`], with at most `most` edge tests.
    fn within_tested(&self, area: &Area, most: usize) -> Option<Vec<(NodeId, Vec<usize>)>> {
        let mut out = Vec::new();
        let (mut tests, mut looked) = (0usize, 0usize);
        // Set once the tests run past `most` or the work is cancelled: the chunks left are skipped.
        let stopped = std::cell::Cell::new(false);
        for (id, p) in &self.paths {
            if !p.bounds.is_some_and(|b| area.overlaps(b)) {
                continue;
            }
            let mut inside = Vec::new();
            p.each_in(
                |b| !stopped.get() && area.overlaps(b),
                |index, q| {
                    looked += 1;
                    tests = tests.saturating_add(area.tests(q));
                    if tests > most || (looked.is_multiple_of(CANCEL_EVERY) && vectorcraft_doc::cancel::is_cancelled()) {
                        stopped.set(true);
                    }
                    if !stopped.get() && area.contains(q) && inside.last() != Some(&index) {
                        inside.push(index);
                    }
                },
            );
            if stopped.get() || vectorcraft_doc::cancel::is_cancelled() {
                return None;
            }
            if !inside.is_empty() {
                out.push((*id, inside));
            }
        }
        Some(out)
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

    /// How many edges telling whether `p` lies inside tests (none for a rectangle).
    fn tests(&self, p: Point) -> usize {
        match self {
            Area::Rect(_) => 0,
            Area::Lasso(l) => l.band(p).map_or(0, <[(Point, Point)]>::len),
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

/// Why a lasso isn't made ([`Lasso::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LassoRefused {
    /// Fewer than three points, one that isn't a number, or no area.
    NoArea,
    /// Its edges would fill its bands past [`LASSO_ENTRIES_MAX`].
    TooIntricate,
}

impl Lasso {
    /// The lasso through `points` (closed back to its first), or why it isn't one.
    pub fn new(points: &[Point]) -> Result<Self, LassoRefused> {
        if points.len() < 3 || !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            return Err(LassoRefused::NoArea);
        }
        let first = *points.first().ok_or(LassoRefused::NoArea)?;
        let bounds = points.iter().map(|p| Rect::from_points(*p, *p)).reduce(|a, b| a.union(b)).ok_or(LassoRefused::NoArea)?;
        if !(bounds.height() > 0.0 && bounds.width() > 0.0) {
            return Err(LassoRefused::NoArea);
        }
        let count = LASSO_BANDS.min(points.len());
        let band_height = bounds.height() / count as f64;
        let mut bands = vec![Vec::new(); count];
        let band = |y: f64| (((y - bounds.y0) / band_height).floor().max(0.0) as usize).min(count - 1);
        let edge = |k: usize| (points.get(k).copied(), points.get(k + 1).copied().unwrap_or(first));
        // What the bands would hold, counted before any is filled.
        let entries = (0..points.len()).filter_map(|k| match edge(k) {
            (Some(a), b) if a.y != b.y => Some(band(a.y.max(b.y)) - band(a.y.min(b.y)) + 1),
            _ => None,
        });
        if entries.fold(0usize, usize::saturating_add) > LASSO_ENTRIES_MAX {
            return Err(LassoRefused::TooIntricate);
        }
        for k in 0..points.len() {
            let (Some(a), b) = edge(k) else { continue };
            if a.y == b.y {
                continue;
            }
            let (lo, hi) = (band(a.y.min(b.y)), band(a.y.max(b.y)));
            for slot in bands.iter_mut().take(hi + 1).skip(lo) {
                slot.push((a, b));
            }
        }
        Ok(Self { bounds, bands, band_height })
    }

    /// The edges of the band `p` lies in (`None` outside the lasso's box).
    fn band(&self, p: Point) -> Option<&[(Point, Point)]> {
        if !(p.x >= self.bounds.x0 && p.x <= self.bounds.x1 && p.y >= self.bounds.y0 && p.y <= self.bounds.y1) {
            return None;
        }
        let count = self.bands.len();
        let k = (((p.y - self.bounds.y0) / self.band_height).floor().max(0.0) as usize).min(count.saturating_sub(1));
        self.bands.get(k).map(Vec::as_slice)
    }

    /// Whether `p` lies inside (even-odd: a ray to the right crosses an odd number of edges).
    fn contains(&self, p: Point) -> bool {
        let Some(edges) = self.band(p) else { return false };
        let mut inside = false;
        for &(a, b) in edges {
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
