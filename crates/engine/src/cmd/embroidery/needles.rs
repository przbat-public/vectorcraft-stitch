//! A path of literal stitches as the stitch commands edit it, needle point by needle point: how a
//! needle point is addressed, how the path is read and written back, how the selection follows an
//! edit, and the lengths of the stitches an edit changed.
//!
//! - **The address** of a needle point is `{id, index}`: the path holding the stitches (a run of
//!   literal stitches, opened from a machine file or converted from an object) and the needle
//!   point's place in it, from 0, across its runs (its subpaths, which the needle jumps between).
//!   An id stays put across edits and an index shifts only inside the path an edit changes, so no
//!   plan is needed: record numbers are for showing and selecting, never for editing.
//! - **The needle points are the anchors** (the walk sews them so, handles ignored), but for a run
//!   stored as a line to its own start, which is one needle point (two when it is the path's only
//!   run and the path says so, `double_point`).
//! - **Read** before any arithmetic: every anchor a finite number within the document's
//!   coordinates, and at most [`MAX_NEEDLE_POINTS`] of them (a hostile file is refused, never
//!   measured).
//! - **Written back** straight: each run one open subpath of corner anchors (a run of one needle
//!   point as a line to its own start, the form a machine file opens as), runs left with no needle
//!   point dropped (a cut before one passes to the run after it), and the marks (the cuts between
//!   runs, the stops after needle points, the exact millimetres) written from the needle points
//!   that carry them. Writing back changes nothing that is sewn.
//! - **The selection** of needle points is `Selection.anchors`: read as needle points, and written
//!   back as the anchors of the needle points each edit leaves selected.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{Exact, StitchRun};
use vectorcraft_doc::{Document, MAX_COORD, Node, NodeId, NodeKind, Selection};
use vectorcraft_geom::{Anchor, PathData, Point, SubPath};
use vectorcraft_stitch::{MM_PER_PT, SAME_POINT, pt_to_mm};

use crate::cmd::bad;
use crate::{EngineError, Result};

/// The most needle points one path may hold and one command may address: the most stitches a
/// design may have (`digitize::MAX_DESIGN_STITCHES`), a bound on what a file or a call can make a
/// command hold.
pub(crate) const MAX_NEEDLE_POINTS: usize = 2_000_000;
/// The most paths one command edits: the Layers panel's and the settings commands' bound on the
/// objects a call names (`layer::MAX_ROWS`).
pub(crate) const MAX_PATHS: usize = crate::cmd::layer::MAX_ROWS;
/// The most entries a reply lists (its counts are never cut): enough to show what an edit did.
pub(crate) const MAX_LISTED: usize = 1000;

/// Why a path's stitches can't be edited needle point by needle point.
pub(crate) mod reason {
    /// Not a path holding a run of literal stitches.
    pub(crate) const NOT_STITCHES: &str = "notStitches";
    /// Locked or hidden, or in a locked layer or group, or on a locked artboard.
    pub(crate) const LOCKED: &str = "locked";
    /// Anchors that aren't numbers, or lie outside the document's coordinates.
    pub(crate) const OUT_OF_RANGE: &str = "outOfRange";
    /// More needle points than a design may have.
    pub(crate) const TOO_MANY: &str = "tooManyNeedlePoints";
}

/// One needle point: where it is (document points), whether the machine stops after it, and the
/// exact millimetres it keeps ([`StitchRun::exact`]) while it stays where it was stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Needle {
    pub(crate) p: Point,
    pub(crate) stop: bool,
    pub(crate) exact: [Option<f64>; 2],
}

/// One run: needle points sewn one after another without lifting the needle (one of the path's
/// subpaths), and whether the thread is cut before the needle reaches it from the run before.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Run {
    pub(crate) cut_before: bool,
    pub(crate) needles: Vec<Needle>,
}

/// A path of literal stitches as the commands edit it: its runs, and the rest of its marks (the cut
/// before it, the cut and the stop after it, the mark of manual stitches, its thread record, what
/// a newer version kept), which an edit carries over.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stitches {
    pub(crate) runs: Vec<Run>,
    pub(crate) marks: StitchRun,
    /// The object's own Trim After and Stop After ([`vectorcraft_doc::embroidery::Embroidery`]'s
    /// `trim_after`, `stop_after`): a cut and a stop after its last needle point, as the run's are.
    pub(crate) object_after: [bool; 2],
}

/// Whether two points (document points) lie at one spot, as the walk tells a line to its own
/// start: within [`SAME_POINT`] millimetres.
fn same(a: Point, b: Point) -> bool {
    a.distance(b) * MM_PER_PT < SAME_POINT
}

/// Whether `p` is a finite point within the document's coordinates.
fn in_range(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD
}

/// The path holding stitches `n` is, as the walk sews it; `Err` with the reason when `n` isn't
/// one, or holds what can't be edited (anchors that aren't numbers or lie outside the document's
/// coordinates, or more needle points than a design may have), checked before any arithmetic.
pub(crate) fn read(n: &Node) -> std::result::Result<Stitches, &'static str> {
    let NodeKind::Path { path, .. } = &n.kind else { return Err(reason::NOT_STITCHES) };
    let Some(e) = n.extras().embroidery.as_deref() else { return Err(reason::NOT_STITCHES) };
    let Some(marks) = e.stitches.clone() else { return Err(reason::NOT_STITCHES) };
    let object_after = [e.trim_after, e.stop_after];
    let anchors: usize = path.subpaths.iter().map(|sp| sp.anchors.len()).sum();
    if anchors > 2 * MAX_NEEDLE_POINTS {
        return Err(reason::TOO_MANY);
    }
    if !path.subpaths.iter().flat_map(|sp| &sp.anchors).all(|a| in_range(a.p)) {
        return Err(reason::OUT_OF_RANGE);
    }
    let trims: BTreeSet<usize> = marks.trims.iter().copied().collect();
    let stops: BTreeSet<(usize, usize)> = marks.stops.iter().copied().collect();
    let exact: BTreeMap<(usize, usize), &Exact> = marks.exact.iter().map(|e| ((e.subpath, e.anchor), e)).collect();
    let alone = path.subpaths.len() == 1;
    let mut runs = Vec::with_capacity(path.subpaths.len());
    for (s, sp) in path.subpaths.iter().enumerate() {
        let points: Vec<Point> = match sp.anchors.as_slice() {
            // A line to its own start: one needle point, unless the path's only run is a tack.
            [a, b] if same(a.p, b.p) && !(alone && marks.double_point) => vec![a.p],
            all => all.iter().map(|a| a.p).collect(),
        };
        let last = points.len().saturating_sub(1);
        let needles = points
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                // A stop named on an anchor past the last needle point (the second of a line to its
                // own start) is the last needle point's.
                let stop = stops.contains(&(s, i)) || (i == last && (last + 1..sp.anchors.len()).any(|a| stops.contains(&(s, a))));
                let exact = exact.get(&(s, i)).map_or([None, None], |e| [e.x, e.y]);
                Needle { p, stop, exact }
            })
            .collect();
        runs.push(Run { cut_before: trims.contains(&s), needles });
    }
    let st = Stitches { runs, marks, object_after };
    if st.len() > MAX_NEEDLE_POINTS {
        return Err(reason::TOO_MANY);
    }
    Ok(st)
}

/// Where a path's needle points lie among its runs, so that one is found by its index without
/// walking the runs before it (a path may hold as many runs as needle points): the index of each
/// run's first needle point, and the subpath each run is written as (a run with no needle point
/// isn't written). Made from the stitches as they are: an edit that adds or removes a needle point,
/// or a run, makes the stitches another index.
#[derive(Clone, Debug, Default)]
pub(crate) struct Index {
    /// Run `r`'s first needle point's index, and one past the last needle point at the end.
    starts: Vec<usize>,
    /// The subpath run `r` is written as: its place among the runs holding needle points.
    subpaths: Vec<usize>,
}

impl Index {
    /// The index of `st` as it is.
    pub(crate) fn of(st: &Stitches) -> Self {
        let mut starts = Vec::with_capacity(st.runs.len() + 1);
        let mut subpaths = Vec::with_capacity(st.runs.len());
        let (mut at, mut written) = (0usize, 0usize);
        for r in &st.runs {
            starts.push(at);
            subpaths.push(written);
            at += r.needles.len();
            written += usize::from(!r.needles.is_empty());
        }
        starts.push(at);
        Self { starts, subpaths }
    }

    /// How many needle points the path holds.
    pub(crate) fn len(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// Needle point `index`'s run and place in it: the run whose needle points start at or before
    /// it and end after it (a run with no needle point never holds one).
    pub(crate) fn locate(&self, index: usize) -> Option<(usize, usize)> {
        if index >= self.len() {
            return None;
        }
        let r = self.starts.partition_point(|&start| start <= index).checked_sub(1)?;
        Some((r, index - self.starts.get(r)?))
    }

    /// The index of needle point `i` of run `r` (one past the run's last, for `i` its length).
    pub(crate) fn index_of(&self, r: usize, i: usize) -> usize {
        self.starts.get(r).map_or(self.len(), |start| start + i)
    }

    /// How many needle points run `r` holds.
    pub(crate) fn run_len(&self, r: usize) -> usize {
        match (self.starts.get(r), self.starts.get(r + 1)) {
            (Some(a), Some(b)) => b - a,
            _ => 0,
        }
    }

    /// The anchors needle point `index` is written as ([`Stitches::written`]): one, or both of a run
    /// of one needle point.
    pub(crate) fn anchors_of(&self, index: usize) -> Vec<(usize, usize)> {
        let Some((r, i)) = self.locate(index) else { return Vec::new() };
        let Some(&s) = self.subpaths.get(r) else { return Vec::new() };
        match self.run_len(r) {
            1 => vec![(s, 0), (s, 1)],
            _ => vec![(s, i)],
        }
    }

    /// The needle point anchor `(subpath, anchor)` is, of stitches read from a path (a run a
    /// subpath): the second anchor of a line to its own start is its one needle point.
    fn needle_of_anchor(&self, (s, a): (usize, usize)) -> Option<usize> {
        let i = a.min(self.run_len(s).checked_sub(1)?);
        Some(self.index_of(s, i))
    }
}

impl Stitches {
    /// How many needle points the path holds.
    pub(crate) fn len(&self) -> usize {
        self.runs.iter().map(|r| r.needles.len()).sum()
    }

    /// The path's [`Index`], as it is now.
    pub(crate) fn index(&self) -> Index {
        Index::of(self)
    }

    /// Needle point `i` of run `r`.
    pub(crate) fn needle(&self, (r, i): (usize, usize)) -> Option<&Needle> {
        self.runs.get(r)?.needles.get(i)
    }

    /// Needle point `i` of run `r`, to change.
    pub(crate) fn needle_mut(&mut self, (r, i): (usize, usize)) -> Option<&mut Needle> {
        self.runs.get_mut(r)?.needles.get_mut(i)
    }

    /// Runs left with no needle point dropped: a cut before one passes to the run after it.
    pub(crate) fn drop_empty_runs(&mut self) {
        let mut kept: Vec<Run> = Vec::with_capacity(self.runs.len());
        let mut cut = false;
        for mut r in std::mem::take(&mut self.runs) {
            if r.needles.is_empty() {
                cut |= r.cut_before;
                continue;
            }
            r.cut_before |= std::mem::take(&mut cut);
            kept.push(r);
        }
        self.runs = kept;
    }

    /// The path and the marks to write back: each run one open subpath of corner anchors (a run of
    /// one needle point a line to its own start), the cuts between runs, the stops after needle
    /// points and the exact millimetres from the needle points that carry them; the tack mark set
    /// when the path's only run is two needle points at one spot. `None` when no needle point is
    /// left.
    pub(crate) fn written(mut self) -> Option<(PathData, StitchRun)> {
        self.drop_empty_runs();
        let Self { runs, mut marks, .. } = self;
        if runs.is_empty() {
            return None;
        }
        let (mut trims, mut stops, mut exact) = (Vec::new(), Vec::new(), Vec::new());
        let mut subpaths = Vec::with_capacity(runs.len());
        for (s, r) in runs.iter().enumerate() {
            if s > 0 && r.cut_before {
                trims.push(s);
            }
            let mut anchors: Vec<Anchor> = r.needles.iter().map(|n| Anchor::corner(n.p)).collect();
            if let [only] = anchors.as_slice() {
                anchors.push(*only);
            }
            for (i, n) in r.needles.iter().enumerate() {
                if n.stop {
                    stops.push((s, i));
                }
                if let [x, y] = n.exact
                    && (x.is_some() || y.is_some())
                {
                    // A run of one needle point is two anchors at it: both are read.
                    let count = if r.needles.len() == 1 { 2 } else { 1 };
                    exact.extend((0..count).map(|k| Exact { subpath: s, anchor: i + k, x, y }));
                }
            }
            subpaths.push(SubPath { anchors, closed: false });
        }
        marks.double_point = matches!(runs.as_slice(), [r] if matches!(r.needles.as_slice(), [a, b] if same(a.p, b.p)));
        (marks.trims, marks.stops, marks.exact) = (trims, stops, exact);
        Some((PathData::new(subpaths), marks))
    }
}

/// Write `st` back into path `n` (its path and its run's marks); `false` when no needle point is
/// left, and nothing is written.
pub(crate) fn write(n: &mut Node, st: Stitches) -> bool {
    let object_after = st.object_after;
    let Some((path, marks)) = st.written() else { return false };
    if let NodeKind::Path { path: p, .. } = &mut n.kind {
        *p = path;
    }
    if let Some(e) = n.extras_mut().embroidery.as_deref_mut() {
        e.stitches = Some(marks);
        let [trim, stop] = object_after;
        if e.trim_after != trim {
            e.set_trim_after(trim);
        }
        if e.stop_after != stop {
            e.set_stop_after(stop);
        }
    }
    true
}

/// The needle points of path `id` (read from it, `ix` its index) that `sel` selects: those whose
/// anchors it selects, or all of them when it selects the path whole.
pub(crate) fn selected_in(sel: &Selection, id: NodeId, ix: &Index) -> BTreeSet<usize> {
    match sel.anchors.get(&id) {
        Some(anchors) => anchors.iter().filter_map(|&a| ix.needle_of_anchor(a)).collect(),
        None if sel.objects.contains(&id) => (0..ix.len()).collect(),
        None => BTreeSet::new(),
    }
}

/// Select needle points `indices` of path `id` (`ix` the index of its stitches as they will be
/// written) in `sel`, in place of what it selected of the path: their anchors, the path among the
/// objects. None: the path leaves the selection.
pub(crate) fn select_in(sel: &mut Selection, id: NodeId, ix: &Index, indices: &BTreeSet<usize>) {
    let anchors: BTreeSet<(usize, usize)> = indices.iter().flat_map(|&k| ix.anchors_of(k)).collect();
    if anchors.is_empty() {
        sel.remove(id);
        sel.anchors.remove(&id);
        return;
    }
    if !sel.objects.contains(&id) {
        sel.objects.push(id);
    }
    sel.anchors.insert(id, anchors);
}

/// What a command acts on: the needle points addressed in each path, in the order the paths were
/// first named, and what of the selection it passed over (and why).
#[derive(Debug, Default)]
pub(crate) struct Targets {
    pub(crate) paths: Vec<(NodeId, Stitches, BTreeSet<usize>)>,
    pub(crate) passed_over: Vec<(NodeId, &'static str)>,
}

impl Targets {
    /// How many needle points are addressed.
    pub(crate) fn count(&self) -> usize {
        self.paths.iter().map(|(_, _, k)| k.len()).sum()
    }
}

/// The message a command gives when nothing it could edit was given or selected: translated
/// (the interface shows it).
pub(crate) const NOTHING_ADDRESSED: &str = "select needle points of a run of stitches, or give needlePoints";

/// A non-negative whole number `v` (an id or an index), refused by command `cmd` as `what`
/// otherwise.
fn whole(v: Option<&Value>, what: &str, cmd: &str) -> Result<u64> {
    v.and_then(Value::as_u64).ok_or_else(|| bad(cmd, format!("{what} {}: a whole number, 0 or more", v.unwrap_or(&Value::Null))))
}

/// The paths named in `o`'s `needlePoints` (`[{id, index}…]`) and `ranges` (`[{id, from, to}…]`,
/// inclusive), each refused by `cmd` when it isn't one that can be edited; with neither, the
/// selection's needle points (`sel`: of each path of stitches it selects, the needle points whose
/// anchors it selects, or all when it selects the path whole; a group selected whole stands for
/// the paths of stitches in it), the other objects passed over. Checked before anything changes or
/// is counted: no index past what a path can hold, at most [`MAX_PATHS`] paths and
/// [`MAX_NEEDLE_POINTS`] needle points, and a path's needle points listed only once every index
/// given is known to be one of them.
pub(crate) fn targets(doc: &Document, sel: &Selection, o: &Map<String, Value>, cmd: &str) -> Result<Targets> {
    // Each path's ranges as given (inclusive), listed one by one only once the path is read.
    let mut named: BTreeMap<NodeId, Vec<(usize, usize)>> = BTreeMap::new();
    let mut order: Vec<NodeId> = Vec::new();
    let mut given = false;
    let mut count = 0usize;
    for key in ["needlePoints", "ranges"] {
        let Some(v) = o.get(key) else { continue };
        let items = v.as_array().ok_or_else(|| bad(cmd, format!("`{key}` {v}: a list")))?;
        given = true;
        if items.len() > MAX_NEEDLE_POINTS {
            return Err(bad(cmd, format!("`{key}` lists {} entries: at most {MAX_NEEDLE_POINTS}", items.len())));
        }
        for item in items {
            let item = item.as_object().ok_or_else(|| bad(cmd, format!("`{key}` entry {item}: an object")))?;
            let id = NodeId(whole(item.get("id"), "`id`", cmd)?);
            let (from, to) = match key {
                "needlePoints" => {
                    let k = whole(item.get("index"), "`index`", cmd)?;
                    (k, k)
                }
                _ => (whole(item.get("from"), "`from`", cmd)?, whole(item.get("to"), "`to`", cmd)?),
            };
            if from > to {
                return Err(bad(cmd, format!("`ranges` entry from {from} to {to}: `from` after `to`")));
            }
            // No path holds more needle points than a design may have: an index past that is refused
            // before it is counted (the count of a range from 0 to the largest number overflows).
            let (Some(from), Some(to)) = (usize::try_from(from).ok(), usize::try_from(to).ok().filter(|to| *to < MAX_NEEDLE_POINTS)) else {
                return Err(bad(cmd, format!("index {to}: past any path's needle points (a path holds at most {MAX_NEEDLE_POINTS})")));
            };
            count = count.saturating_add(to - from + 1);
            if count > MAX_NEEDLE_POINTS {
                return Err(bad(cmd, format!("more than {MAX_NEEDLE_POINTS} needle points addressed")));
            }
            if !named.contains_key(&id) {
                order.push(id);
            }
            named.entry(id).or_default().push((from, to));
            if named.len() > MAX_PATHS {
                return Err(bad(cmd, format!("more than {MAX_PATHS} paths of stitches addressed")));
            }
        }
    }
    let mut out = Targets::default();
    if given {
        for id in order {
            let n = doc.node(id).ok_or(EngineError::NoNode(id))?;
            if !doc.is_editable(id) {
                return Err(bad(cmd, format!("object {}: {}", id.0, said(reason::LOCKED))));
            }
            let st = read(n).map_err(|why| bad(cmd, format!("object {}: {}", id.0, said(why))))?;
            let ranges = named.remove(&id).unwrap_or_default();
            let len = st.len();
            if let Some(past) = ranges.iter().map(|&(_, to)| to).filter(|&to| to >= len).max() {
                return Err(bad(cmd, format!("object {}: needle point {past}, of {len} (from 0)", id.0)));
            }
            let k = ranges.into_iter().flat_map(|(from, to)| from..=to).collect();
            out.paths.push((id, st, k));
        }
        return Ok(out);
    }
    // The selection: each path of stitches it selects, and those in a group it selects whole.
    let mut seen = BTreeSet::new();
    for &id in &sel.objects {
        let Some(n) = doc.node(id) else { continue };
        let mut found: Vec<&Node> = Vec::new();
        n.walk(&mut |c| {
            if read_marks(c) {
                found.push(c);
            }
        });
        if found.is_empty() {
            out.passed_over.push((id, reason::NOT_STITCHES));
            continue;
        }
        for c in found {
            if !seen.insert(c.id) {
                continue;
            }
            if !doc.is_editable(c.id) {
                out.passed_over.push((c.id, reason::LOCKED));
                continue;
            }
            match read(c) {
                Ok(st) => {
                    let k = if c.id == id { selected_in(sel, id, &st.index()) } else { (0..st.len()).collect() };
                    count = count.saturating_add(k.len());
                    if count > MAX_NEEDLE_POINTS || out.paths.len() >= MAX_PATHS {
                        return Err(bad(cmd, format!("more than {MAX_NEEDLE_POINTS} needle points or {MAX_PATHS} paths of stitches selected")));
                    }
                    if !k.is_empty() {
                        out.paths.push((c.id, st, k));
                    }
                }
                Err(why) => out.passed_over.push((c.id, why)),
            }
        }
    }
    Ok(out)
}

/// Whether `n` is a path holding a run of literal stitches.
fn read_marks(n: &Node) -> bool {
    matches!(n.kind, NodeKind::Path { .. }) && n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some())
}

/// Reason `why` in words, for a refusal.
pub(crate) fn said(why: &str) -> &'static str {
    match why {
        reason::LOCKED => "it is locked or hidden, or in a locked layer, group or artboard",
        reason::OUT_OF_RANGE => "its anchors aren't numbers, or lie outside the document's coordinates",
        reason::TOO_MANY => "it holds more needle points than a design may have",
        _ => "it isn't a run of stitches",
    }
}

/// The lengths of the needle-down stitches an edit made or changed, millimetres, against the
/// document's shortest stitch (`min`) and the longest a machine sews well (`max`): what an agent
/// and the interface check an edit by, without a plan.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lengths {
    count: usize,
    shortest: Option<f64>,
    longest: Option<f64>,
    below: usize,
    above: usize,
}

impl Lengths {
    /// Count the stitch from `a` to `b` (document points).
    pub(crate) fn add(&mut self, a: Point, b: Point, min: f64, max: f64) {
        let mm = pt_to_mm(a.distance(b));
        if !mm.is_finite() {
            return;
        }
        self.count += 1;
        self.shortest = Some(self.shortest.map_or(mm, |s| s.min(mm)));
        self.longest = Some(self.longest.map_or(mm, |l| l.max(mm)));
        self.below += usize::from(mm < min);
        self.above += usize::from(mm > max);
    }

    /// Count the stitches into and out of needle point `index` of `st` (within its run; `ix` its
    /// index).
    pub(crate) fn around(&mut self, st: &Stitches, ix: &Index, index: usize, min: f64, max: f64) {
        let Some((s, i)) = ix.locate(index) else { return };
        let Some(r) = st.runs.get(s) else { return };
        let p = |i: usize| r.needles.get(i).map(|n| n.p);
        if let (Some(a), Some(b)) = (i.checked_sub(1).and_then(p), p(i)) {
            self.add(a, b, min, max);
        }
        if let (Some(a), Some(b)) = (p(i), p(i + 1)) {
            self.add(a, b, min, max);
        }
    }

    /// As a reply gives it.
    pub(crate) fn json(&self, min: f64, max: f64) -> Value {
        json!({
            "count": self.count,
            "shortest": self.shortest,
            "longest": self.longest,
            "belowMin": self.below,
            "aboveMax": self.above,
            "min": min,
            "max": max,
        })
    }
}

/// The shortest stitch the document sews (Document Setup's machine value, else the built-in
/// default), millimetres: the live check's lower line.
pub(crate) fn shortest_stitch(d: &Document) -> f64 {
    super::resolve::DocLayer::of(d)
        .min_stitch()
        .unwrap_or(vectorcraft_stitch::plan::Params::for_style(vectorcraft_stitch::plan::Style::Running).min_stitch)
}

/// The longest stitch a machine sews well, millimetres: the Check tab's `stitchTooLong` line, the
/// live check's upper one.
pub(crate) const LONGEST: f64 = super::check::LONGEST_STITCH;
