//! The stitch commands that edit needle points ([`super::needles`] reads and writes them): each
//! takes millimetres, addresses needle points by `{id, index}` (else the selection's), is one undo
//! step (none when nothing changes), leaves the selection on what it says, and answers the lengths
//! of the stitches it made or changed (`lengths`), without a plan. Each goes through one [`Edit`],
//! which holds what they share.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::StitchRun;
use vectorcraft_doc::{Document, NodeId, Selection};
use vectorcraft_geom::{Point, Vec2};
use vectorcraft_stitch::{mm_to_pt, pt_to_mm};

use super::needles::{self, Index, LONGEST, Lengths, MAX_LISTED, MAX_NEEDLE_POINTS, NOTHING_ADDRESSED, Stitches, Targets};
use super::{object, only};
use crate::cmd::bad;
use crate::{EngineError, Result, Session};

/// What an edit leaves of one path: its stitches written back (removed when no needle point is
/// left), the path removed, or only its run's marks changed (a cut or a stop passed to it).
pub(super) enum Left {
    Stitches(Stitches),
    Removed,
    Marks(StitchRun),
}

/// The addressing keys every edit takes.
const ADDRESSES: [&str; 2] = ["needlePoints", "ranges"];

/// The params of `cmd` as an object, its keys `own` besides the addresses.
fn params<'a>(p: &'a Value, cmd: &str, own: &[&str]) -> Result<std::borrow::Cow<'a, Map<String, Value>>> {
    let o = object(p, cmd)?;
    let keys: Vec<&str> = ADDRESSES.iter().chain(own).copied().collect();
    only(&o, &keys, cmd)?;
    Ok(o)
}

/// What command `cmd` acts on ([`needles::targets`]); nothing to act on is an error.
fn targets(s: &Session, o: &Map<String, Value>, cmd: &str) -> Result<(Targets, Selection)> {
    let st = s.doc()?;
    let sel = st.selection.clone();
    let t = needles::targets(&st.doc, &sel, o, cmd)?;
    if t.paths.is_empty() {
        return Err(EngineError::Other(NOTHING_ADDRESSED.to_string()));
    }
    Ok((t, sel))
}

/// A finite number `v` (millimetres) of `cmd`'s param `key`, `None` when left out.
fn millimetres(o: &Map<String, Value>, key: &str, cmd: &str) -> Result<Option<f64>> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` {v}: a number of millimetres"))),
    }
}

/// A point `[x, y]` in millimetres of `cmd`'s param `key`, `None` when left out.
fn point_mm(o: &Map<String, Value>, key: &str, cmd: &str) -> Result<Option<Point>> {
    let Some(v) = o.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(Some(Point::new(x, y))),
            _ => Err(bad(cmd, format!("`{key}` {v}: [x, y], millimetres"))),
        },
        _ => Err(bad(cmd, format!("`{key}` {v}: [x, y], millimetres"))),
    }
}

/// Write `edits` into the active document and set the selection to `sel`, as one undo step named
/// `label`: the paths removed (and those left with no needle point) removed together, in one pass
/// over each parent.
fn apply(s: &mut Session, label: &str, edits: Vec<(NodeId, Left)>, sel: Selection) -> Result<()> {
    s.edit(label, |d, selection| {
        let mut gone = Vec::new();
        for (id, left) in edits {
            match left {
                Left::Stitches(st) => {
                    let n = d.node_mut(id).ok_or(EngineError::NoNode(id))?;
                    if !needles::write(n, st) {
                        gone.push(id);
                    }
                }
                Left::Removed => gone.push(id),
                Left::Marks(marks) => {
                    let n = d.node_mut(id).ok_or(EngineError::NoNode(id))?;
                    if let Some(e) = n.extras_mut().embroidery.as_deref_mut() {
                        e.stitches = Some(marks);
                    }
                }
            }
        }
        if !gone.is_empty() {
            // Every id is an edited path the document holds, each once: a failure is a bug.
            d.remove_many(&gone).map_err(|e| EngineError::Internal { cmd: label.to_string(), msg: e.to_string() })?;
        }
        *selection = sel;
        Ok(())
    })
}

/// The paths an edit changed or removed, as a reply lists them: each id with its needle points now
/// (none: removed), at most [`MAX_LISTED`] (a path only given a cut or a stop isn't one).
fn listed(edits: &[(NodeId, Left)]) -> Vec<Value> {
    edits
        .iter()
        .filter_map(|(id, left)| match left {
            Left::Stitches(st) => Some(json!({"id": id.0, "needlePoints": st.len()})),
            Left::Removed => Some(json!({"id": id.0, "needlePoints": 0})),
            Left::Marks(_) => None,
        })
        .take(MAX_LISTED)
        .collect()
}

/// Where an edit leaves the needle points of a path that the selection selected.
enum Kept<'a> {
    /// Where they were: no needle point moved to another place in the path.
    Same,
    /// Each to its new index ([`Rebuilt::moved_to`]; `None`: no longer there).
    Moved(&'a [Option<usize>]),
    /// Only these needle points of the path, whatever was selected of it.
    Only(BTreeSet<usize>),
    /// None of it: the path leaves the selection.
    Left,
    /// The path whole.
    Whole,
}

/// One edit of needle points under way, from [`Edit::begin`] to [`Edit::finish`]: the selection as
/// it was and as the edit leaves it, the live check's limits and what it measured, the paths it
/// changed and what of the selection it passed over. Every command that edits needle points goes
/// through it, so that they keep to one set of rules: no path past [`MAX_NEEDLE_POINTS`], the
/// selection carried to where its needle points went, the same reply keys, and an undo step only
/// when something changed.
struct Edit {
    /// The command's id, which its refusals name.
    cmd: &'static str,
    before: Selection,
    sel: Selection,
    min: f64,
    max: f64,
    lengths: Lengths,
    edits: Vec<(NodeId, Left)>,
    passed_over: Vec<(NodeId, &'static str)>,
    /// Runs of two needle points at one spot in a path of several, made one needle point
    /// ([`Stitches::collapse_tacks`]).
    collapsed: usize,
}

impl Edit {
    /// An edit of the active document's needle points by command `cmd`, the selection as it is
    /// `before` and what of it the command passed over `passed_over`; measured against the
    /// document's shortest stitch.
    fn begin(s: &Session, cmd: &'static str, before: Selection, passed_over: Vec<(NodeId, &'static str)>) -> Result<Self> {
        let min = needles::shortest_stitch(&s.doc()?.doc);
        Ok(Self { cmd, sel: before.clone(), before, min, max: LONGEST, lengths: Lengths::default(), edits: Vec::new(), passed_over, collapsed: 0 })
    }

    /// The needle points of path `id` (read from it, `ix` its index) that the selection selected
    /// before the edit, when it selected some of them by their anchors (`None`: the path whole or
    /// not at all, which the edit leaves as it is).
    fn selected(&self, id: NodeId, ix: &Index) -> Option<BTreeSet<usize>> {
        self.before.anchors.contains_key(&id).then(|| needles::selected_in(&self.before, id, ix))
    }

    /// Path `id` as the edit leaves it, `out` (`ix` its index): refused past
    /// [`MAX_NEEDLE_POINTS`]; the stitches into and out of its needle points `touched` measured; its
    /// needle points the selection selected (`selected`, of the path before) kept as `kept` says.
    fn rebuilt(
        &mut self,
        id: NodeId,
        mut out: Stitches,
        ix: &Index,
        selected: Option<BTreeSet<usize>>,
        kept: Kept,
        touched: impl IntoIterator<Item = usize>,
    ) -> Result<()> {
        if ix.len() > MAX_NEEDLE_POINTS {
            return Err(bad(self.cmd, format!("object {}: more than {MAX_NEEDLE_POINTS} needle points", id.0)));
        }
        // A run of two needle points brought to one spot is one needle point when written: made so
        // now, the selection and the stitches measured taken along.
        let collapse = out.collapse_tacks();
        self.collapsed += collapse.as_ref().map_or(0, |map| map.iter().filter(|k| k.is_none()).count());
        let index = collapse.as_ref().map(|_| out.index());
        let ix = index.as_ref().unwrap_or(ix);
        let after = |k: usize| match &collapse {
            Some(map) => map.get(k).copied().flatten(),
            None => Some(k),
        };
        self.lengths.around(&out, ix, touched.into_iter().filter_map(&after), self.min, self.max);
        let carried = |k: BTreeSet<usize>| -> BTreeSet<usize> { k.into_iter().filter_map(&after).collect() };
        match kept {
            Kept::Same => {
                if let Some(k) = selected {
                    needles::select_in(&mut self.sel, id, ix, &carried(k));
                }
            }
            Kept::Moved(moved_to) => {
                if let Some(k) = selected {
                    let k = k.into_iter().filter_map(|k| moved_to.get(k).copied().flatten()).collect();
                    needles::select_in(&mut self.sel, id, ix, &carried(k));
                }
            }
            Kept::Only(k) => needles::select_in(&mut self.sel, id, ix, &carried(k)),
            Kept::Left => self.leave(id),
            Kept::Whole => {
                self.sel.anchors.remove(&id);
                if !self.sel.objects.contains(&id) {
                    self.sel.objects.push(id);
                }
            }
        }
        self.edits.push((id, Left::Stitches(out)));
        Ok(())
    }

    /// Path `id` removed, and out of the selection.
    fn removed(&mut self, id: NodeId) {
        self.leave(id);
        self.edits.push((id, Left::Removed));
    }

    /// Path `id` out of the selection.
    fn leave(&mut self, id: NodeId) {
        self.sel.remove(id);
        self.sel.anchors.remove(&id);
    }

    /// The reply: `own`'s keys and what every edit answers (the paths changed or removed and how
    /// many, the lengths measured, what was passed over), the edit applied as one undo step named
    /// `label` only when `changed`.
    fn finish(self, s: &mut Session, label: &str, changed: bool, own: Value) -> Result<Value> {
        let mut reply = match own {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        let paths = listed(&self.edits);
        let count = self.edits.iter().filter(|(_, l)| !matches!(l, Left::Marks(_))).count();
        reply.insert("paths".into(), Value::Array(paths));
        reply.insert("pathCount".into(), json!(count));
        reply.insert("lengths".into(), self.lengths.json(self.min, self.max));
        reply.insert("collapsedCount".into(), json!(self.collapsed));
        let passed: Vec<Value> = self.passed_over.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect();
        reply.insert("passedOver".into(), Value::Array(passed));
        if changed {
            apply(s, label, self.edits, self.sel)?;
        }
        Ok(Value::Object(reply))
    }
}

pub(super) const MOVE: &str = "embroidery.moveNeedlePoints";
/// The undo step's name, the command's label.
pub(super) const MOVE_LABEL: &str = "Move Needle Points";

/// `embroidery.moveNeedlePoints {needlePoints?, ranges?, dx?, dy?, to?}`: each needle point
/// addressed moved by `dx, dy` millimetres, or the one addressed to `to`. A result outside the
/// document's coordinates is refused before anything changes; no step when nothing moves.
pub(super) fn move_points(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, MOVE, &["dx", "dy", "to"])?;
    let (dx, dy) = (millimetres(&o, "dx", MOVE)?, millimetres(&o, "dy", MOVE)?);
    let to = point_mm(&o, "to", MOVE)?;
    if to.is_some() && (dx.is_some() || dy.is_some()) {
        return Err(bad(MOVE, "give `to`, or `dx` and `dy`, not both"));
    }
    let (t, before) = targets(s, &o, MOVE)?;
    if to.is_some() && t.count() != 1 {
        return Err(bad(MOVE, format!("`to` places one needle point: {} are addressed", t.count())));
    }
    let by = Vec2::new(mm_to_pt(dx.unwrap_or(0.0)), mm_to_pt(dy.unwrap_or(0.0)));
    let to = to.map(|p| Point::new(mm_to_pt(p.x), mm_to_pt(p.y)));
    let mut e = Edit::begin(s, MOVE, before, t.passed_over)?;
    let mut moved = 0usize;
    for (id, mut st, indices) in t.paths {
        // A move adds and removes no needle point: one index serves before and after.
        let ix = st.index();
        let selected = e.selected(id, &ix);
        let mut changed = false;
        for &k in &indices {
            let Some(n) = ix.locate(k).and_then(|at| st.needle_mut(at)) else { continue };
            let p = to.unwrap_or(n.p + by);
            if !(p.x.is_finite() && p.y.is_finite() && p.x.abs() <= vectorcraft_doc::MAX_COORD && p.y.abs() <= vectorcraft_doc::MAX_COORD) {
                return Err(bad(MOVE, format!("object {}: needle point {k} would leave the document's coordinates", id.0)));
            }
            if p != n.p {
                // Its exact millimetres stay: they apply only while it lies where they were kept
                // for, so moved there again it is sewn exactly as before.
                n.p = p;
                changed = true;
                moved += 1;
            }
        }
        if changed {
            e.rebuilt(id, st, &ix, selected, Kept::Same, indices)?;
        }
    }
    e.finish(s, MOVE_LABEL, moved > 0, json!({"changedCount": moved}))
}

pub(super) const INSERT: &str = "embroidery.insertNeedlePoints";
/// The undo step's name, the command's label.
pub(super) const INSERT_LABEL: &str = "Insert Needle Points";
/// The most needle points one `at` list places: far more than one gesture or list makes, a bound
/// on what one call can allocate. An estimate.
const MAX_INSERT_AT: usize = 10_000;
/// The most needle points one call adds in all: far above any real edit. An estimate.
const MAX_ADDED: usize = 100_000;

/// `embroidery.insertNeedlePoints {id?, index?, at?, needlePoints?, ranges?}`: with `id` and
/// `index`, the points `at` (millimetres) put before needle point `index` of path `id` (`index`
/// its length: after its last), or one at the middle of the stitch arriving at it; else one needle
/// point midway after each needle point addressed (or selected) that a stitch leaves. The new
/// needle points are left selected.
pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, INSERT, &["id", "index", "at"])?;
    let place = match (o.get("id"), o.get("index")) {
        (None, None) => None,
        (Some(id), Some(index)) => {
            if o.contains_key("needlePoints") || o.contains_key("ranges") {
                return Err(bad(INSERT, "give `id` and `index`, or needle points to insert after, not both"));
            }
            let id = id.as_u64().ok_or_else(|| bad(INSERT, format!("`id` {id}: an object's id")))?;
            let index = index
                .as_u64()
                .and_then(|k| usize::try_from(k).ok())
                .ok_or_else(|| bad(INSERT, format!("`index` {index}: a whole number, 0 or more")))?;
            Some((NodeId(id), index))
        }
        _ => return Err(bad(INSERT, "`id` and `index` go together")),
    };
    let at: Option<Vec<Point>> = match o.get("at").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            let items = v.as_array().ok_or_else(|| bad(INSERT, format!("`at` {v}: a list of [x, y], millimetres")))?;
            if items.is_empty() || items.len() > MAX_INSERT_AT {
                return Err(bad(INSERT, format!("`at` lists {} points: 1 to {MAX_INSERT_AT}", items.len())));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let one = Map::from_iter([("at".to_string(), item.clone())]);
                let p = point_mm(&one, "at", INSERT)?.ok_or_else(|| bad(INSERT, "`at`: [x, y], millimetres"))?;
                let p = Point::new(mm_to_pt(p.x), mm_to_pt(p.y));
                if !(p.x.abs() <= vectorcraft_doc::MAX_COORD && p.y.abs() <= vectorcraft_doc::MAX_COORD) {
                    return Err(bad(INSERT, format!("`at` {item}: outside the document's coordinates")));
                }
                out.push(p);
            }
            Some(out)
        }
    };
    if at.is_some() && place.is_none() {
        return Err(bad(INSERT, "`at` places needle points before `index` of path `id`: give them"));
    }
    let (mut inserted, mut added_all) = (Vec::new(), 0usize);
    let mut listed_added = |id: NodeId, added: &BTreeSet<usize>| {
        inserted.extend(added.iter().take(MAX_LISTED.saturating_sub(inserted.len())).map(|k| json!({"id": id.0, "index": k})));
    };
    let mut e;
    if let Some((id, index)) = place {
        let before = s.doc()?.selection.clone();
        e = Edit::begin(s, INSERT, before, Vec::new())?;
        let doc = &s.doc()?.doc;
        let n = doc.node(id).ok_or(EngineError::NoNode(id))?;
        if !doc.is_editable(id) {
            return Err(bad(INSERT, format!("object {}: {}", id.0, needles::said(needles::reason::LOCKED))));
        }
        let mut st = needles::read(n).map_err(|why| bad(INSERT, format!("object {}: {}", id.0, needles::said(why))))?;
        let ix = st.index();
        let len = ix.len();
        if index > len {
            return Err(bad(INSERT, format!("object {}: index {index}, past its {len} needle points (at most {len}: after the last)", id.0)));
        }
        let points = match at {
            Some(points) => points,
            // The middle of the stitch arriving at it: a stitch, not the jump onto a run's first.
            None => {
                let (r, i) = ix.locate(index).filter(|&(_, i)| i > 0).ok_or_else(|| {
                    bad(
                        INSERT,
                        format!("object {}: no stitch arrives at needle point {index} (a run starts there, or the path ends): give `at`", id.0),
                    )
                })?;
                let (Some(a), Some(b)) = (st.needle((r, i - 1)), st.needle((r, i))) else { return Err(bad(INSERT, "no such needle point")) };
                vec![a.p.midpoint(b.p)]
            }
        };
        if len + points.len() > MAX_NEEDLE_POINTS {
            return Err(bad(INSERT, format!("object {}: more than {MAX_NEEDLE_POINTS} needle points", id.0)));
        }
        // Before needle point `index`, in its run; after the last, in the last run.
        let (r, i) = ix.locate(index).unwrap_or_else(|| (st.runs.len().saturating_sub(1), st.runs.last().map_or(0, |r| r.needles.len())));
        if st.runs.is_empty() {
            st.runs.push(needles::Run::default());
        }
        let Some(run) = st.runs.get_mut(r) else { return Err(bad(INSERT, "no such run")) };
        run.needles.splice(i..i, points.iter().map(|&p| needles::Needle { p, stop: false, exact: [None, None] }));
        // The runs before it are as they were: its place among the path's needle points is too.
        let first = ix.index_of(r, i);
        let added: BTreeSet<usize> = (first..first + points.len()).collect();
        added_all = added.len();
        listed_added(id, &added);
        let ix = st.index();
        e.rebuilt(id, st, &ix, None, Kept::Only(added.clone()), added)?;
    } else {
        let (t, before) = targets(s, &o, INSERT)?;
        e = Edit::begin(s, INSERT, before, t.passed_over)?;
        for (id, mut st, indices) in t.paths {
            let mut added = BTreeSet::new();
            let (mut k, mut at_now) = (0usize, 0usize);
            for run in &mut st.runs {
                let was = std::mem::take(&mut run.needles);
                run.needles.reserve(was.len());
                for (i, n) in was.iter().enumerate() {
                    run.needles.push(*n);
                    at_now += 1;
                    if indices.contains(&k)
                        && let Some(next) = was.get(i + 1)
                    {
                        run.needles.push(needles::Needle { p: n.p.midpoint(next.p), stop: false, exact: [None, None] });
                        added.insert(at_now);
                        at_now += 1;
                    }
                    k += 1;
                }
            }
            if added.is_empty() {
                continue;
            }
            added_all += added.len();
            if added_all > MAX_ADDED {
                return Err(bad(INSERT, format!("more than {MAX_ADDED} needle points added")));
            }
            listed_added(id, &added);
            let ix = st.index();
            e.rebuilt(id, st, &ix, None, Kept::Only(added.clone()), added)?;
        }
    }
    e.finish(s, INSERT_LABEL, added_all > 0, json!({"changedCount": added_all, "inserted": inserted}))
}

pub(super) const DELETE: &str = "embroidery.deleteNeedlePoints";
/// The undo step's name, the command's label.
pub(super) const DELETE_LABEL: &str = "Delete Needle Points";

/// Where a cut or a stop went, or that it was dropped, as a reply lists it.
fn function_json(function: &str, id: NodeId, index: Option<usize>, before: bool) -> Value {
    match (index, before) {
        (_, true) => json!({"function": function, "id": id.0, "before": true}),
        (Some(k), false) => json!({"function": function, "id": id.0, "index": k}),
        (None, false) => json!({"function": function, "id": id.0}),
    }
}

/// The paths of stitches sewn beside each of `gone` (paths an edit removes) in their parents, the
/// one before it and the one after it, passing over those also gone and those that can't be
/// edited: one walk of each parent's children.
fn beside(doc: &Document, gone: &BTreeSet<NodeId>) -> std::collections::HashMap<NodeId, [Option<NodeId>; 2]> {
    let editable = doc.editable();
    let mut out: std::collections::HashMap<NodeId, [Option<NodeId>; 2]> = std::collections::HashMap::new();
    let mut parents = BTreeSet::new();
    for &id in gone {
        let Some(parent) = doc.parent_of(id).filter(|p| parents.insert(*p)).and_then(|p| doc.node(p)) else { continue };
        let children = parent.children().map(Vec::as_slice).unwrap_or_default();
        let open = |c: &std::sync::Arc<vectorcraft_doc::Node>| !gone.contains(&c.id) && needles::is_stitches(c) && editable(c.id);
        let mut last = None;
        for c in children {
            if gone.contains(&c.id) {
                out.entry(c.id).or_default()[0] = last;
            } else if open(c) {
                last = Some(c.id);
            }
        }
        let mut next = None;
        for c in children.iter().rev() {
            if gone.contains(&c.id) {
                out.entry(c.id).or_default()[1] = next;
            } else if open(c) {
                next = Some(c.id);
            }
        }
    }
    out
}

/// `embroidery.deleteNeedlePoints {needlePoints?, ranges?}`: each needle point addressed (or
/// selected) deleted, the stitches beside it becoming one (neighbours merge); a stop after one
/// passes to the needle point before it that stays. A run left with no needle point goes, its cut
/// passing to the run after it: to the path's own cut before it when no run with needle points is
/// before, to its cut after it when none is after. A path left with no needle point is removed: its
/// cut and stop pass to the path of stitches sewn before it in its parent (a cut, else to the one
/// after), or are dropped, and the reply says which, once each. The paths it changed leave the
/// selection, so a second Delete deletes nothing.
pub(super) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, DELETE, &[])?;
    let (t, before) = targets(s, &o, DELETE)?;
    let doc = s.doc()?.doc.clone();
    let mut e = Edit::begin(s, DELETE, before, t.passed_over)?;
    let (mut moved, mut dropped, mut removed) = (Vec::new(), Vec::new(), BTreeSet::new());
    // Each needle point a merged stitch now leaves from, as numbered after the edit.
    let mut merged_listed = Vec::new();
    let mut deleted = 0usize;
    for (id, mut st, indices) in t.paths {
        // Each kept needle point a deleted one followed (its stitch merged), by its new index.
        let mut merged = BTreeSet::new();
        let (mut k, mut kept) = (0usize, 0usize);
        // The last needle point kept, by run and place: a stop of one deleted after it passes to it.
        let mut last: Option<(usize, usize)> = None;
        // What happened to the stops of the needle points deleted, said only if the path stays (a
        // path removed passes its stop on once, below).
        let (mut stops_moved, mut stops_dropped) = (Vec::new(), Vec::new());
        for r in 0..st.runs.len() {
            let Some(run) = st.runs.get_mut(r) else { continue };
            let was = std::mem::take(&mut run.needles);
            let mut needles: Vec<needles::Needle> = Vec::with_capacity(was.len());
            for n in &was {
                if indices.contains(&k) {
                    deleted += 1;
                    // The stitch from the needle point kept before it, in its run, now goes on.
                    if !needles.is_empty() {
                        merged.insert(kept.saturating_sub(1));
                    }
                    if n.stop {
                        let to = match last {
                            Some((lr, li)) if lr == r => needles.get_mut(li),
                            Some(at) => st.needle_mut(at),
                            None => None,
                        };
                        match to {
                            Some(m) => {
                                m.stop = true;
                                stops_moved.push(function_json("stop", id, Some(kept.saturating_sub(1)), false));
                            }
                            None => stops_dropped.push(function_json("stop", id, None, false)),
                        }
                    }
                } else {
                    needles.push(*n);
                    last = Some((r, needles.len() - 1));
                    kept += 1;
                }
                k += 1;
            }
            if let Some(run) = st.runs.get_mut(r) {
                run.needles = needles;
            }
        }
        if kept == 0 {
            removed.insert(id);
            e.removed(id);
            continue;
        }
        moved.extend(stops_moved);
        dropped.extend(stops_dropped);
        // A cut before a run left empty at the path's start or end has no run after it in the path
        // to go before: the path's own cut before it, or after it.
        let first = st.runs.iter().position(|r| !r.needles.is_empty()).unwrap_or(0);
        let last_run = st.runs.iter().rposition(|r| !r.needles.is_empty()).unwrap_or(0);
        // Every such cut taken (none written where no run follows it), whether any was there.
        let (mut lead, mut tail) = (false, false);
        for r in st.runs.iter_mut().take(first + 1).skip(1) {
            lead |= std::mem::take(&mut r.cut_before);
        }
        for r in st.runs.iter_mut().skip(last_run + 1) {
            tail |= std::mem::take(&mut r.cut_before);
        }
        if lead && !st.marks.trim_before {
            st.marks.trim_before = true;
            moved.push(function_json("trim", id, None, true));
        }
        if tail && !st.marks.trim_after {
            st.marks.trim_after = true;
            moved.push(function_json("trim", id, kept.checked_sub(1), false));
        }
        let ix = st.index();
        merged_listed.extend(merged.iter().take(MAX_LISTED.saturating_sub(merged_listed.len())).map(|k| json!({"id": id.0, "index": k})));
        e.rebuilt(id, st, &ix, None, Kept::Left, merged)?;
    }
    // A path removed: its cut and its stop pass to its neighbour, as the gap it leaves. Each
    // neighbour is found once, and its edit (or its marks, given only a cut or a stop) by its id.
    let neighbours = beside(&doc, &removed);
    let edited: std::collections::HashMap<NodeId, usize> = e.edits.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
    let mut passed: std::collections::BTreeMap<NodeId, StitchRun> = std::collections::BTreeMap::new();
    for &id in &removed {
        let Some(n) = doc.node(id) else { continue };
        let ext = n.extras().embroidery.as_deref();
        let marks = ext.and_then(|e| e.stitches.as_ref());
        let cut = marks.is_some_and(|m| m.trim_before || m.trim_after) || ext.is_some_and(|e| e.trim_after);
        let stop = marks.is_some_and(|m| m.stop_after || !m.stops.is_empty()) || ext.is_some_and(|e| e.stop_after);
        let [before_it, after_it] = neighbours.get(&id).copied().unwrap_or_default();
        // Give neighbour `to` a mark, and say where it went: the needle point it follows now.
        let mut give = |to: NodeId, f: fn(&mut StitchRun), function: &str, before: bool| {
            let index = match edited.get(&to).and_then(|&i| e.edits.get_mut(i)) {
                Some((_, Left::Stitches(st))) => {
                    f(&mut st.marks);
                    st.len().checked_sub(1)
                }
                _ => {
                    let n = doc.node(to);
                    if let Some(m) = passed.get_mut(&to) {
                        f(m);
                    } else if let Some(mut m) = n.and_then(|n| n.extras().embroidery.as_deref()).and_then(|e| e.stitches.clone()) {
                        f(&mut m);
                        passed.insert(to, m);
                    }
                    n.and_then(needles::count).and_then(|c| c.checked_sub(1))
                }
            };
            function_json(function, to, if before { None } else { index }, before)
        };
        if cut {
            match (before_it, after_it) {
                (Some(to), _) => moved.push(give(to, |m| m.trim_after = true, "trim", false)),
                (None, Some(to)) => moved.push(give(to, |m| m.trim_before = true, "trim", true)),
                (None, None) => dropped.push(function_json("trim", id, None, false)),
            }
        }
        if stop {
            match before_it {
                Some(to) => moved.push(give(to, |m| m.stop_after = true, "stop", false)),
                None => dropped.push(function_json("stop", id, None, false)),
            }
        }
    }
    e.edits.extend(passed.into_iter().map(|(id, m)| (id, Left::Marks(m))));
    let own = json!({
        "changedCount": deleted,
        "merged": merged_listed,
        "removed": removed.iter().take(MAX_LISTED).map(|id| id.0).collect::<Vec<_>>(),
        "removedCount": removed.len(),
        "moved": moved.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "dropped": dropped.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
    });
    e.finish(s, DELETE_LABEL, deleted > 0, own)
}

/// A path's runs rebuilt needle point by needle point, each needle point of the path it was rebuilt
/// from given its new index.
struct Rebuilt {
    runs: Vec<needles::Run>,
    /// The new index of each needle point of the path before, by its index there (`None`: not
    /// carried over).
    moved_to: Vec<Option<usize>>,
    count: usize,
}

impl Rebuilt {
    fn new(before: usize) -> Self {
        Self { runs: Vec::new(), moved_to: vec![None; before], count: 0 }
    }

    /// Start a new run, the thread cut before it when `cut_before`.
    fn run(&mut self, cut_before: bool) {
        self.runs.push(needles::Run { cut_before, needles: Vec::new() });
    }

    /// Add needle point `n` to the last run: needle point `old` of the path before, or a new one.
    fn push(&mut self, n: needles::Needle, old: Option<usize>) {
        if self.runs.is_empty() {
            self.run(false);
        }
        if let Some(r) = self.runs.last_mut() {
            r.needles.push(n);
        }
        if let Some(slot) = old.and_then(|o| self.moved_to.get_mut(o)) {
            *slot = Some(self.count);
        }
        self.count += 1;
    }

    /// The last run's last needle point, to change.
    fn last_mut(&mut self) -> Option<&mut needles::Needle> {
        self.runs.last_mut()?.needles.last_mut()
    }

    /// A tie-off at the last run's end: a short stitch back along its last stitch and forward
    /// again, as the planner ties a run off before a cut (`plan::lock_step`, `min` millimetres the
    /// shortest stitch); a stop after its last needle point moves to the needle point it ends on.
    fn tie_off(&mut self, min: f64) {
        let Some(r) = self.runs.last() else { return };
        let [.., a, b] = r.needles.as_slice() else { return };
        let (a, b) = (*a, *b);
        let Some(step) = lock_step(b.p, a.p, min) else { return };
        if let Some(last) = self.last_mut() {
            last.stop = false;
        }
        self.push(needles::Needle { p: step, stop: false, exact: [None, None] }, None);
        self.push(b, None);
    }

    /// A tie-in at the last run's start, its first needle point already there and `next` the one
    /// after it: a short stitch forward along its first stitch and back, as the planner ties a run
    /// in after a cut.
    fn tie_in(&mut self, next: Option<Point>, min: f64) {
        let Some(first) = self.runs.last().and_then(|r| r.needles.first().copied()) else { return };
        let Some(step) = next.and_then(|next| lock_step(first.p, next, min)) else { return };
        // The first needle point is sewn again after the step: a stop after it goes after that.
        let stop = self.last_mut().is_some_and(|n| std::mem::take(&mut n.stop));
        self.push(needles::Needle { p: step, stop: false, exact: [None, None] }, None);
        self.push(needles::Needle { stop, ..first }, None);
    }

    /// The path rebuilt, with the marks of `from` (which it takes), and each needle point of `from`'s
    /// new index.
    fn done(self, from: Stitches) -> (Stitches, Vec<Option<usize>>) {
        (Stitches { runs: self.runs, marks: from.marks, object_after: from.object_after }, self.moved_to)
    }
}

/// [`vectorcraft_stitch::plan::lock_step`] on document points, `min` the shortest stitch in
/// millimetres.
fn lock_step(at: Point, toward: Point, min: f64) -> Option<Point> {
    let mm = |p: Point| Point::new(pt_to_mm(p.x), pt_to_mm(p.y));
    let step = vectorcraft_stitch::plan::lock_step(mm(at), mm(toward), min)?;
    let back = Point::new(mm_to_pt(step.x), mm_to_pt(step.y));
    (back.x.is_finite() && back.y.is_finite()).then_some(back)
}

/// `st` split before each needle point in `at` (indices of `st`, none a run's first): the needle
/// lifts there and comes down on it, a new run starting with it. With `ties` (the shortest stitch,
/// millimetres), the run before is tied off and the new one tied in, as the planner ties a run
/// around a cut. The thread is cut at the lift when `cut`, the machine stops there when `stop`.
/// → the path rebuilt and each needle point's new index.
fn split_before(st: &Stitches, at: &BTreeSet<usize>, ties: Option<f64>, cut: bool, stop: bool) -> Rebuilt {
    let mut out = Rebuilt::new(st.len());
    let mut k = 0usize;
    for run in &st.runs {
        out.run(run.cut_before);
        for (i, n) in run.needles.iter().enumerate() {
            if i > 0 && at.contains(&k) {
                if let Some(min) = ties {
                    out.tie_off(min);
                }
                if let Some(last) = out.last_mut() {
                    last.stop |= stop;
                }
                out.run(cut);
                out.push(*n, Some(k));
                if let Some(min) = ties {
                    out.tie_in(run.needles.get(i + 1).map(|n| n.p), min);
                }
            } else {
                out.push(*n, Some(k));
            }
            k += 1;
        }
    }
    out
}

/// The `ties` param of `cmd`: tie off and in around each lift it makes (the project owner's choice:
/// on unless `ties: false`), as `Some` of the document's shortest stitch.
fn ties(o: &Map<String, Value>, cmd: &str, min: f64) -> Result<Option<f64>> {
    match o.get("ties") {
        None | Some(Value::Null) | Some(Value::Bool(true)) => Ok(Some(min)),
        Some(Value::Bool(false)) => Ok(None),
        Some(v) => Err(bad(cmd, format!("`ties` {v}: true or false"))),
    }
}

pub(super) const SPLIT: &str = "embroidery.splitRun";
/// The undo step's name, the command's label.
pub(super) const SPLIT_LABEL: &str = "Split Run";

/// `embroidery.splitRun {needlePoints?, ranges?, ties?}`: the needle lifts before each needle point
/// addressed (or selected), a new run starting with it (a jump onto it); a run's first is passed
/// by. With `ties` (the default), the run before is tied off and the new one tied in.
pub(super) fn split_run(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, SPLIT, &["ties"])?;
    let (t, before) = targets(s, &o, SPLIT)?;
    let mut e = Edit::begin(s, SPLIT, before, t.passed_over)?;
    let ties = ties(&o, SPLIT, e.min)?;
    let (mut jumps, mut split, mut unchanged) = (Vec::new(), 0usize, 0usize);
    for (id, st, indices) in t.paths {
        let was = st.index();
        // A run's first already follows a lift.
        let at: BTreeSet<usize> = indices.iter().copied().filter(|&k| was.locate(k).is_some_and(|(_, i)| i > 0)).collect();
        unchanged += indices.len() - at.len();
        if at.is_empty() {
            continue;
        }
        split += at.len();
        let selected = e.selected(id, &was);
        let (out, moved_to) = split_before(&st, &at, ties, false, false).done(st);
        let ix = out.index();
        let landings: Vec<usize> = at.iter().filter_map(|&k| moved_to.get(k).copied().flatten()).collect();
        for &landing in &landings {
            let from = landing.checked_sub(1).and_then(|b| needle_at(&out, &ix, b));
            if let (Some(a), Some(b)) = (from, needle_at(&out, &ix, landing))
                && jumps.len() < MAX_LISTED
            {
                jumps.push(json!({"id": id.0, "index": landing, "length": pt_to_mm(a.distance(b))}));
            }
        }
        e.rebuilt(id, out, &ix, selected, Kept::Moved(&moved_to), landings)?;
    }
    e.finish(s, SPLIT_LABEL, split > 0, json!({"changedCount": split, "unchangedCount": unchanged, "jumps": jumps}))
}

/// Needle point `index` of `st` (`ix` its index), where it is.
fn needle_at(st: &Stitches, ix: &Index, index: usize) -> Option<Point> {
    st.needle(ix.locate(index)?).map(|n| n.p)
}

pub(super) const JOIN: &str = "embroidery.joinRuns";
/// The undo step's name, the command's label.
pub(super) const JOIN_LABEL: &str = "Join Runs";
/// The message Join Runs gives for paths it can't join: translated (the interface shows it).
pub(super) const NOT_JOINABLE: &str = "these runs aren't sewn one right after another in one colour block";
/// What Join (⌘J) says of runs of stitches selected with art: translated.
pub(super) const RUNS_WITH_ART: &str = "runs of stitches join only with runs of stitches: select only them";
/// What Join (⌘J) says of one run of stitches selected alone: translated.
pub(super) const ONE_RUN: &str = "select two or more runs of stitches sewn one right after another to join them";

/// `st` with each run whose first needle point is in `at` joined to the run before it: the jump
/// onto it becomes a stitch (a first needle point lying where the run before ends is kept once), a
/// cut before it and a stop after the run before go; how many of each went.
fn join_within(st: &Stitches, at: &BTreeSet<usize>) -> (Rebuilt, usize, usize) {
    let mut out = Rebuilt::new(st.len());
    let (mut cuts, mut stops) = (0usize, 0usize);
    let mut k = 0usize;
    for (s, run) in st.runs.iter().enumerate() {
        let joined = s > 0 && !run.needles.is_empty() && at.contains(&k) && out.runs.last().is_some_and(|r| !r.needles.is_empty());
        if joined {
            cuts += usize::from(run.cut_before);
            if let Some(last) = out.last_mut() {
                stops += usize::from(std::mem::take(&mut last.stop));
            }
        } else {
            out.run(run.cut_before);
        }
        for (i, n) in run.needles.iter().enumerate() {
            let same_spot = joined
                && i == 0
                && out
                    .runs
                    .last()
                    .and_then(|r| r.needles.last())
                    .is_some_and(|last| last.p.distance(n.p) * vectorcraft_stitch::MM_PER_PT < vectorcraft_stitch::SAME_POINT);
            if same_spot {
                // Kept once: the needle point it repeats takes its place.
                if let Some(slot) = out.moved_to.get_mut(k) {
                    *slot = out.count.checked_sub(1);
                }
            } else {
                out.push(*n, Some(k));
            }
            k += 1;
        }
    }
    (out, cuts, stops)
}

/// The colour a path of stitches is sewn in, as the walk reads it: its first visible stroke's
/// solid colour, in 8-bit RGB.
fn stroke_colour(n: &vectorcraft_doc::Node) -> Option<[u8; 3]> {
    n.appearance.items.iter().filter(|i| i.visible()).find_map(|i| match i {
        vectorcraft_doc::AppearanceItem::Stroke(s) => match &s.paint {
            vectorcraft_color::Paint::Solid { color, .. } => {
                let [r, g, b, _] = color.to_rgba8(1.0);
                Some([r, g, b])
            }
            _ => None,
        },
        vectorcraft_doc::AppearanceItem::Fill(_) => None,
    })
}

/// `embroidery.joinRuns {needlePoints?, ranges?, ids?}`: with `ids`, the paths of stitches given
/// (sewn one right after another: next to one another in one parent, in one colour, all opened or
/// all manual) joined into the first, the jump between each two becoming a stitch; else each run
/// whose first needle point is addressed (or selected) joined to the run before it in its path.
/// A cut or a stop at a gap joined goes, and the reply says so. Never reversed.
pub(super) fn join_runs(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, JOIN, &["ids"])?;
    let mut dropped = Vec::new();
    let (e, joined, removed) = if let Some(ids) = o.get("ids") {
        if o.contains_key("needlePoints") || o.contains_key("ranges") {
            return Err(bad(JOIN, "give the paths to join (`ids`), or the runs' first needle points, not both"));
        }
        let items = ids.as_array().ok_or_else(|| bad(JOIN, format!("`ids` {ids}: a list of paths of stitches")))?;
        if items.len() < 2 || items.len() > needles::MAX_PATHS {
            return Err(bad(JOIN, format!("`ids` lists {} paths: 2 to {}", items.len(), needles::MAX_PATHS)));
        }
        let ids: Vec<NodeId> = items
            .iter()
            .map(|v| v.as_u64().map(NodeId).ok_or_else(|| bad(JOIN, format!("`ids` entry {v}: an object's id"))))
            .collect::<Result<_>>()?;
        let before = s.doc()?.selection.clone();
        let mut e = Edit::begin(s, JOIN, before, Vec::new())?;
        let doc = s.doc()?.doc.clone();
        let editable = doc.editable();
        let mut paths = Vec::with_capacity(ids.len());
        for &id in &ids {
            let n = doc.node(id).ok_or(EngineError::NoNode(id))?;
            if !editable(id) {
                return Err(bad(JOIN, format!("object {}: {}", id.0, needles::said(needles::reason::LOCKED))));
            }
            let st = needles::read(n).map_err(|why| bad(JOIN, format!("object {}: {}", id.0, needles::said(why))))?;
            paths.push((id, stroke_colour(n), st));
        }
        // Next to one another in one parent, in the order given, in one colour, of one kind.
        let parent = doc.parent_of(ids.first().copied().unwrap_or(NodeId(0)));
        let children: Vec<NodeId> =
            parent.and_then(|p| doc.node(p)).and_then(|p| p.children()).map(|c| c.iter().map(|n| n.id).collect()).unwrap_or_default();
        let first_at = children.iter().position(|c| Some(c) == ids.first());
        let next_to = first_at.is_some_and(|at| children.get(at..at + ids.len()).is_some_and(|run| run == ids.as_slice()));
        let colour = paths.first().and_then(|(_, c, _)| *c);
        let kind = paths.first().map(|(_, _, st)| st.marks.manual);
        let alike = paths.iter().all(|(_, c, st)| *c == colour && colour.is_some() && Some(st.marks.manual) == kind);
        if !next_to || !alike {
            return Err(EngineError::Other(NOT_JOINABLE.to_string()));
        }
        // What the selection selected of each path, before anything moves.
        let paths: Vec<ToJoin> =
            paths.into_iter().map(|(id, _, st)| ToJoin { picked: e.selected(id, &st.index()), whole: e.before.contains(id), id, st }).collect();
        let removed = join_into_first(&mut e, paths, &mut dropped)?;
        (e, removed.len(), removed)
    } else {
        let (t, before) = targets(s, &o, JOIN)?;
        let mut e = Edit::begin(s, JOIN, before, t.passed_over)?;
        let doc = s.doc()?.doc.clone();
        let mut joined = 0usize;
        // Each path's runs joined within it (their first needle points addressed), kept by id until
        // the paths whose first needle point is addressed are joined to the path before them.
        let mut within: std::collections::BTreeMap<NodeId, (Stitches, Option<BTreeSet<usize>>, bool)> = Default::default();
        let mut firsts = Vec::new();
        // A path's first needle point addressed (given, or its anchor selected), not a path selected
        // whole, joins the path to the one before it.
        let by_address = o.contains_key("needlePoints") || o.contains_key("ranges");
        for (id, st, indices) in t.paths {
            let selected = e.selected(id, &st.index());
            if indices.contains(&0) && (by_address || selected.is_some()) {
                firsts.push(id);
            }
            let (rebuilt, cuts, stops) = join_within(&st, &indices);
            let gaps = st.runs.len() - rebuilt.runs.len();
            if gaps == 0 {
                within.insert(id, (st, selected, false));
                continue;
            }
            joined += gaps;
            dropped.extend(std::iter::repeat_n(json!({"function": "trim", "id": id.0}), cuts));
            dropped.extend(std::iter::repeat_n(json!({"function": "stop", "id": id.0}), stops));
            let (out, moved_to) = rebuilt.done(st);
            let ix = out.index();
            // The stitches joined: into each needle point that started a run.
            let touched: Vec<usize> = indices.iter().filter_map(|&k| moved_to.get(k).copied().flatten()).collect();
            e.lengths.around(&out, &ix, touched, e.min, e.max);
            let selected = selected.map(|k| k.into_iter().filter_map(|k| moved_to.get(k).copied().flatten()).collect());
            within.insert(id, (out, selected, true));
        }
        // A path's first needle point addressed: the path joined to the path of stitches sewn right
        // before it in its parent (an opened design keeps each run a path), one chain of paths
        // sewn one after another joined into its first.
        let mut removed = Vec::new();
        for chain in chains(&doc, &firsts, &mut e.passed_over) {
            let mut paths = Vec::with_capacity(chain.len());
            for id in chain {
                let (st, picked) = match within.remove(&id) {
                    Some((st, picked, _)) => (st, picked),
                    None => {
                        let Some(st) = doc.node(id).and_then(|n| needles::read(n).ok()) else { continue };
                        let picked = e.selected(id, &st.index());
                        (st, picked)
                    }
                };
                paths.push(ToJoin { whole: e.before.contains(id), id, st, picked });
            }
            if paths.len() > 1 {
                let taken = join_into_first(&mut e, paths, &mut dropped)?;
                joined += taken.len();
                removed.extend(taken);
            }
        }
        // The paths joined within only.
        for (id, (out, selected, changed)) in within {
            if changed {
                let ix = out.index();
                e.rebuilt(id, out, &ix, selected, Kept::Same, std::iter::empty())?;
            }
        }
        (e, joined, removed)
    };
    let own = json!({
        "changedCount": joined,
        "removed": removed.iter().take(MAX_LISTED).map(|id| id.0).collect::<Vec<_>>(),
        "dropped": dropped.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
    });
    e.finish(s, JOIN_LABEL, joined > 0, own)
}

/// A path of stitches to join ([`join_into_first`]): its id, its stitches (as an edit may have
/// left them), the needle points the selection selected of it by their anchors (of these
/// stitches), and whether it selected it whole.
struct ToJoin {
    id: NodeId,
    st: Stitches,
    picked: Option<BTreeSet<usize>>,
    whole: bool,
}

/// `paths` (sewn one right after another, in order) joined into the first in one pass: their runs
/// one after another, joined at every seam; the gaps' cuts and stops dropped into `dropped`; the
/// selection carried to where its needle points went (a path selected only whole leaves the joined
/// one selected whole). → the paths taken in, removed.
fn join_into_first(e: &mut Edit, paths: Vec<ToJoin>, dropped: &mut Vec<Value>) -> Result<Vec<NodeId>> {
    let picked: Vec<(Option<BTreeSet<usize>>, bool)> = paths.iter().map(|p| (p.picked.clone(), p.whole)).collect();
    let mut paths = paths.into_iter();
    let Some(ToJoin { id: first_id, st: mut whole, .. }) = paths.next() else { return Ok(Vec::new()) };
    let (mut offsets, mut removed) = (vec![0usize], Vec::new());
    let mut count = whole.len();
    for ToJoin { id, st, .. } in paths {
        let after = whole.marks.after() | whole.object_after;
        if after.trim || st.marks.trim_before {
            dropped.push(json!({"function": "trim", "id": id.0}));
        }
        if after.stop || whole.runs.last().and_then(|r| r.needles.last()).is_some_and(|n| n.stop) {
            dropped.push(json!({"function": "stop", "id": id.0}));
        }
        if let Some(last) = whole.runs.last_mut().and_then(|r| r.needles.last_mut()) {
            last.stop = false;
        }
        let Stitches { runs, marks, object_after } = st;
        offsets.push(count);
        count += runs.iter().map(|r| r.needles.len()).sum::<usize>();
        whole.runs.extend(runs);
        // The joined path ends as the last one did, and keeps what any of them kept unread.
        (whole.marks.trim_after, whole.marks.stop_after) = (marks.trim_after, marks.stop_after);
        whole.object_after = object_after;
        for (k, v) in marks.extra.iter() {
            if !whole.marks.extra.contains_key(k) {
                std::sync::Arc::make_mut(&mut whole.marks.extra).insert(k.clone(), v.clone());
            }
        }
        removed.push(id);
    }
    let seams: BTreeSet<usize> = offsets.iter().skip(1).copied().collect();
    let (out, moved_to) = join_within(&whole, &seams).0.done(whole);
    let ix = out.index();
    // Each path's selected needle points at their places in the joined one (all of a path selected
    // whole among paths selected by needle points); paths selected only whole leave it selected
    // whole.
    let ends = offsets.iter().skip(1).copied().chain([count]);
    let kept = if picked.iter().any(|(k, _)| k.is_some()) {
        let mut kept = BTreeSet::new();
        for (((k, whole), &from), to) in picked.iter().zip(&offsets).zip(ends) {
            let mine: Vec<usize> = match k {
                Some(k) => k.iter().map(|k| k + from).collect(),
                None if *whole => (from..to).collect(),
                None => Vec::new(),
            };
            kept.extend(mine.into_iter().filter_map(|k| moved_to.get(k).copied().flatten()));
        }
        Kept::Only(kept)
    } else if picked.iter().any(|(_, whole)| *whole) {
        Kept::Whole
    } else {
        Kept::Same
    };
    // The stitches that join them: out of the last needle point before each seam.
    let touched: Vec<usize> = seams.iter().filter_map(|&at| at.checked_sub(1).and_then(|k| moved_to.get(k).copied().flatten())).collect();
    for &id in &removed {
        e.removed(id);
    }
    e.rebuilt(first_id, out, &ix, None, kept, touched)?;
    // The joined path before the paths it took in, among the edits.
    let n = e.edits.len();
    if let Some(tail) = e.edits.get_mut(n.saturating_sub(removed.len() + 1)..) {
        tail.rotate_right(1);
    }
    Ok(removed)
}

/// The paths `firsts` (whose first needle points were addressed) each with the path of stitches
/// sewn right before it in its parent (the child just before it, a path of stitches that can be
/// edited, in its colour and of its kind), as chains of paths sewn one right after another, in
/// order; one with none such is passed over (`notJoinable`, into `passed`).
fn chains(doc: &Document, firsts: &[NodeId], passed: &mut Vec<(NodeId, &'static str)>) -> Vec<Vec<NodeId>> {
    let editable = doc.editable();
    // Each path's place among its parent's children, each parent's read once.
    let mut place: std::collections::HashMap<NodeId, (NodeId, usize)> = Default::default();
    let mut parents = BTreeSet::new();
    for &id in firsts {
        let Some(parent) = doc.parent_of(id).filter(|p| parents.insert(*p)) else { continue };
        let children = doc.node(parent).and_then(|p| p.children()).map(Vec::as_slice).unwrap_or_default();
        place.extend(children.iter().enumerate().map(|(i, c)| (c.id, (parent, i))));
    }
    let mut ordered: Vec<NodeId> = firsts.to_vec();
    ordered.sort_by_key(|id| place.get(id).copied().map(|(p, i)| (p.0, i)));
    let mut out: Vec<Vec<NodeId>> = Vec::new();
    for id in ordered {
        let before = place.get(&id).and_then(|&(parent, i)| {
            let children = doc.node(parent)?.children()?;
            let (b, n) = (children.get(i.checked_sub(1)?)?, children.get(i)?);
            let alike = needles::is_stitches(b)
                && editable(b.id)
                && stroke_colour(b).is_some_and(|c| Some(c) == stroke_colour(n))
                && b.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).map(|m| m.manual)
                    == n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).map(|m| m.manual);
            alike.then_some(b.id)
        });
        let Some(before) = before else {
            passed.push((id, needles::reason::NOT_JOINABLE));
            continue;
        };
        match out.last_mut() {
            Some(chain) if chain.last() == Some(&before) => chain.push(id),
            _ => out.push(vec![before, id]),
        }
    }
    out
}

pub(super) const FUNCTION: &str = "embroidery.setMachineFunction";
/// The undo step's name, the command's label.
pub(super) const FUNCTION_LABEL: &str = "Machine Function";

/// A machine function the stitch commands set after a needle point.
#[derive(Clone, Copy, PartialEq)]
enum Function {
    /// The thread is cut.
    Trim,
    /// The machine stops.
    Stop,
}

/// The needle point a function after needle point `k` of `st` (`ix` its index) comes after: `k`
/// itself when it ends its run; else, when all its run has after it is a tie-off back to its spot
/// (as Machine Function ties a run off after the needle point addressed), the tie's end, so that a
/// function set with ties is found again, and turned off, at the needle point it was set at;
/// `None` inside a run.
fn function_end(st: &Stitches, ix: &Index, k: usize) -> Option<usize> {
    let (r, i) = ix.locate(k)?;
    let n = ix.run_len(r);
    if i + 1 == n {
        return Some(k);
    }
    let tied = i + 3 == n && st.needle((r, i + 2)).zip(st.needle((r, i))).is_some_and(|(end, at)| same_spot(end.p, at.p));
    tied.then_some(k + 2)
}

/// Whether `st` (`ix` its index) has function `f` after needle point `k`: a cut before the next run
/// (or after the path, the path's own or kept before the path sewn after it, `cut_next`, as an
/// opened design keeps the cut between two of its runs) where `k`'s run ends ([`function_end`]), a
/// stop after it, after the tie that follows it, or after the path.
fn has(st: &Stitches, ix: &Index, k: usize, f: Function, cut_next: bool) -> bool {
    let end = function_end(st, ix, k);
    let last = end.is_some_and(|e| e + 1 == ix.len());
    let mut after = st.marks.after() | st.object_after;
    after.trim |= cut_next;
    let stop_at = |k: usize| ix.locate(k).and_then(|at| st.needle(at)).is_some_and(|n| n.stop);
    match (f, end) {
        (Function::Trim, Some(_)) if last => after.trim,
        // The run the next needle point is in, past any run with none.
        (Function::Trim, Some(e)) => ix.locate(e + 1).and_then(|(next, _)| st.runs.get(next)).is_some_and(|r| r.cut_before),
        (Function::Trim, None) => false,
        (Function::Stop, end) => stop_at(k) || end.is_some_and(|e| e != k && stop_at(e)) || (last && after.stop),
    }
}

/// The path of stitches sewn right after path `id` in its parent (the child just after it), when it
/// keeps a cut before it, as an opened design keeps the cut between two of its runs.
fn next_cut(doc: &Document, id: NodeId) -> Option<NodeId> {
    let parent = doc.node(doc.parent_of(id)?)?;
    let children = parent.children()?;
    let at = children.iter().position(|c| c.id == id)?;
    let next = children.get(at + 1)?;
    next.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).filter(|m| m.trim_before).map(|_| next.id)
}

/// `embroidery.setMachineFunction {needlePoints?, ranges?, function, on?, ties?}`: the thread cut
/// (`trim`), or the machine stopped (`stop`), after each needle point addressed (or selected), or
/// no more. Inside a run the run splits there first, the run before tied off and the next tied in
/// (on unless `ties: false`, the project owner's choice); at a run's end it is the gap's own, at the
/// path's end the path's. `on` left out is on unless every needle point has it already. Off, the
/// runs stay split.
pub(super) fn machine_function(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, FUNCTION, &["function", "on", "ties"])?;
    let f = match o.get("function").and_then(Value::as_str) {
        Some("trim") => Function::Trim,
        Some("stop") => Function::Stop,
        _ => return Err(bad(FUNCTION, format!("`function` {}: \"trim\" or \"stop\"", o.get("function").unwrap_or(&Value::Null)))),
    };
    let on = match o.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(FUNCTION, format!("`on` {v}: true or false")))?),
    };
    let (t, before) = targets(s, &o, FUNCTION)?;
    let mut e = Edit::begin(s, FUNCTION, before, t.passed_over)?;
    let ties = ties(&o, FUNCTION, e.min)?;
    // The path of stitches sewn right after each path addressed that keeps a cut before it (an
    // opened design keeps the cut between two of its runs so): the cut after the path's end.
    let doc = s.doc()?.doc.clone();
    let cut_next: std::collections::HashMap<NodeId, NodeId> = t.paths.iter().filter_map(|(id, ..)| Some((*id, next_cut(&doc, *id)?))).collect();
    let on = on.unwrap_or_else(|| {
        !t.paths.iter().all(|(id, st, k)| {
            let ix = st.index();
            k.iter().all(|&k| has(st, &ix, k, f, cut_next.contains_key(id)))
        })
    });
    // The paths whose cut before them is turned off.
    let mut uncut = BTreeSet::new();
    let (mut changed, mut unchanged, mut splits) = (0usize, 0usize, 0usize);
    // Each needle point addressed, as numbered after the edit.
    let mut addressed = Vec::new();
    for (id, mut out, indices) in t.paths {
        // Setting a function moves no needle point: one index serves throughout.
        let ix = out.index();
        let selected = e.selected(id, &ix);
        let mut inside = BTreeSet::new();
        let mut any = false;
        for &k in &indices {
            if has(&out, &ix, k, f, cut_next.get(&id).is_some_and(|next| !uncut.contains(next))) == on {
                unchanged += 1;
                continue;
            }
            changed += 1;
            any = true;
            let Some(end) = function_end(&out, &ix, k) else {
                // Inside a run, on: the run splits after it, below. Off, only a stop can be there.
                if on {
                    inside.insert(k + 1);
                } else if let Some(n) = ix.locate(k).and_then(|at| out.needle_mut(at)) {
                    n.stop = false;
                }
                continue;
            };
            let Some((r, _)) = ix.locate(end) else { continue };
            // The run the next needle point is in, past any run with none.
            let next = ix.locate(end + 1).map(|(n, _)| n).filter(|&n| n > r);
            match (f, next) {
                (Function::Trim, Some(n)) => {
                    if let Some(next) = out.runs.get_mut(n) {
                        next.cut_before = on;
                    }
                }
                (Function::Trim, None) => {
                    out.marks.trim_after = on;
                    out.object_after.trim = on;
                    // Off: the cut kept before the path sewn after it too.
                    if let Some(next) = cut_next.get(&id).filter(|_| !on) {
                        uncut.insert(*next);
                    }
                }
                (Function::Stop, next) => {
                    // On: after the run's end (the tie's, when one follows it); off: wherever it is.
                    for at in if on { vec![end] } else { vec![k, end] } {
                        if let Some(n) = ix.locate(at).and_then(|at| out.needle_mut(at)) {
                            n.stop = on;
                        }
                    }
                    if next.is_none() && !on {
                        out.marks.stop_after = false;
                        out.object_after.stop = false;
                    }
                }
            }
        }
        if !any {
            addressed.extend(indices.iter().take(MAX_LISTED.saturating_sub(addressed.len())).map(|k| json!({"id": id.0, "index": k})));
            continue;
        }
        if inside.is_empty() {
            addressed.extend(indices.iter().take(MAX_LISTED.saturating_sub(addressed.len())).map(|k| json!({"id": id.0, "index": k})));
            e.rebuilt(id, out, &ix, selected, Kept::Same, indices)?;
            continue;
        }
        splits += inside.len();
        let (out, moved_to) = split_before(&out, &inside, ties, f == Function::Trim, f == Function::Stop).done(out);
        let ix = out.index();
        let touched: Vec<usize> = indices.iter().filter_map(|&k| moved_to.get(k).copied().flatten()).collect();
        addressed.extend(touched.iter().take(MAX_LISTED.saturating_sub(addressed.len())).map(|k| json!({"id": id.0, "index": k})));
        e.rebuilt(id, out, &ix, selected, Kept::Moved(&moved_to), touched)?;
    }
    // The cuts kept before the paths sewn after, turned off: in the path's own edit, or its marks.
    for next in uncut {
        match e.edits.iter_mut().find(|(id, _)| *id == next) {
            Some((_, Left::Stitches(st))) => st.marks.trim_before = false,
            _ => {
                if let Some(mut m) = doc.node(next).and_then(|n| n.extras().embroidery.as_deref()).and_then(|e| e.stitches.clone()) {
                    m.trim_before = false;
                    e.edits.push((next, Left::Marks(m)));
                }
            }
        }
    }
    let own = json!({
        "function": if f == Function::Trim { "trim" } else { "stop" },
        "on": on,
        "changedCount": changed,
        "unchangedCount": unchanged,
        "splitCount": splits,
        "needlePoints": addressed,
    });
    e.finish(s, FUNCTION_LABEL, changed > 0, own)
}

pub(super) const SPLIT_LONG: &str = "embroidery.splitLongStitches";
/// The undo step's name, the command's label.
pub(super) const SPLIT_LONG_LABEL: &str = "Split Long Stitches";
/// The range `maxLength` is taken in, millimetres: from Melco's 1 mm, the shortest stitch it keeps
/// for most needles (`check::SHORT`), to the 12.7 mm a machine sews well (`check::LONGEST_STITCH`),
/// the default.
const MAX_LENGTH_RANGE: (f64, f64) = (1.0, LONGEST);

/// The needle points (document points) a stitch from `a` to `b` (millimetres, as the walk sews
/// them) is split into, besides its ends: with `grid` (a format's, and its records' reach in
/// tenths), where that format's writer would cut it (each end on the grid the writer counts the
/// design on, the fewest equal steps of at most one record's reach along x and along y,
/// `format::steps`); else the fewest equal stitches no longer than `max` millimetres. Refused past
/// `room` new needle points.
fn pieces(
    a: Point,
    b: Point,
    grid: Option<(&vectorcraft_stitch::format::Grid, i32)>,
    max: f64,
    room: usize,
) -> std::result::Result<Vec<Point>, String> {
    let pt = |p: Point| Point::new(mm_to_pt(p.x), mm_to_pt(p.y));
    match grid {
        Some((grid, reach)) => {
            let ((ax, ay), (bx, by)) = (grid.units(a), grid.units(b));
            let (dx, dy) = (bx - ax, by - ay);
            let span = dx.unsigned_abs().max(dy.unsigned_abs());
            if span <= reach.unsigned_abs() {
                return Ok(Vec::new());
            }
            let n = span.div_ceil(reach.unsigned_abs().max(1)) as usize;
            if n > room {
                return Err(format!("a stitch would take {n} needle points"));
            }
            let mut out = Vec::with_capacity(n);
            let (mut x, mut y) = (ax, ay);
            for (sx, sy) in vectorcraft_stitch::format::steps(dx, dy, reach) {
                (x, y) = (x + sx, y + sy);
                out.push(pt(grid.point((x, y))));
            }
            // The last step ends on the stitch's own end.
            out.pop();
            Ok(out)
        }
        None => {
            let length = a.distance(b);
            // Within the limit, or no number (no split).
            if !length.is_finite() || length <= max {
                return Ok(Vec::new());
            }
            let n = (length / max).ceil();
            if !n.is_finite() || n > room as f64 {
                return Err(format!("a stitch {length:.1} mm long would take more needle points than one edit adds"));
            }
            let n = n as usize;
            Ok((1..n).map(|i| pt(a.lerp(b, i as f64 / n as f64))).collect())
        }
    }
}

/// `embroidery.splitLongStitches {needlePoints?, ranges?, maxLength?, format?}`: each stitch arriving
/// at a needle point addressed (or selected; a path or group selected whole: all of its stitches)
/// longer than `maxLength` (millimetres, its own length; default 12.7) split into the fewest equal
/// stitches within it, or, with `format`, longer along x or y than one of that format's records, cut
/// where its writer would cut it (that format's export unchanged). More needle points, never jumps;
/// a jump between runs is never split (the writers split long moves themselves).
pub(super) fn split_long(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, SPLIT_LONG, &["maxLength", "format"])?;
    let format = match o.get("format").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(
            v.as_str()
                .and_then(vectorcraft_stitch::format::Format::from_id)
                .ok_or_else(|| bad(SPLIT_LONG, format!("`format` {v}: one of dst, tap, t01, exp, pes, pec, jef, vp3")))?,
        ),
    };
    let max = millimetres(&o, "maxLength", SPLIT_LONG)?;
    if format.is_some() && max.is_some() {
        return Err(bad(SPLIT_LONG, "give `maxLength` or `format`, not both"));
    }
    let (lo, hi) = MAX_LENGTH_RANGE;
    let max = max.unwrap_or(hi);
    if !(lo..=hi).contains(&max) {
        return Err(bad(SPLIT_LONG, format!("`maxLength` {max}: {lo} to {hi} millimetres")));
    }
    let (t, before) = targets(s, &o, SPLIT_LONG)?;
    // With a format, the grid its writer counts the design on: the design planned as the export
    // plans it (splitting a stitch on its segment changes neither where it starts nor its extent).
    let grid = match format {
        Some(f) => {
            let (plan, _) = super::planned(s, &super::Options::default(), false, SPLIT_LONG)?;
            let grid = f.grid(&plan.to_pattern()).map_err(|e| EngineError::Other(e.to_string()))?;
            Some((grid, vectorcraft_stitch::quantize(f.max_step())))
        }
        None => None,
    };
    let mut e = Edit::begin(s, SPLIT_LONG, before, t.passed_over)?;
    // The live check measures against the limit asked for.
    e.max = max;
    let (mut split, mut added) = (0usize, 0usize);
    // Each needle point addressed, as numbered after the edit.
    let mut addressed = Vec::new();
    for (id, st, indices) in t.paths {
        let mut out = Rebuilt::new(st.len());
        let (mut k, mut here) = (0usize, 0usize);
        for run in &st.runs {
            out.run(run.cut_before);
            for (i, n) in run.needles.iter().enumerate() {
                if let (true, Some(prev)) = (indices.contains(&k), i.checked_sub(1).and_then(|j| run.needles.get(j))) {
                    let room = MAX_ADDED.saturating_sub(added);
                    let grid = grid.as_ref().map(|(g, reach)| (g, *reach));
                    let new = pieces(prev.mm(), n.mm(), grid, max, room).map_err(|why| bad(SPLIT_LONG, format!("object {}: {why}", id.0)))?;
                    if !new.is_empty() {
                        split += 1;
                        added += new.len();
                        here += new.len();
                        for p in new {
                            out.push(needles::Needle { p, stop: false, exact: [None, None] }, None);
                        }
                    }
                }
                out.push(*n, Some(k));
                k += 1;
            }
        }
        if here == 0 {
            addressed.extend(indices.iter().take(MAX_LISTED.saturating_sub(addressed.len())).map(|k| json!({"id": id.0, "index": k})));
            continue;
        }
        let selected = e.selected(id, &st.index());
        let (out, moved_to) = out.done(st);
        let ix = out.index();
        let touched: Vec<usize> = indices.iter().filter_map(|&k| moved_to.get(k).copied().flatten()).collect();
        addressed.extend(touched.iter().take(MAX_LISTED.saturating_sub(addressed.len())).map(|k| json!({"id": id.0, "index": k})));
        e.rebuilt(id, out, &ix, selected, Kept::Moved(&moved_to), touched)?;
    }
    let limit = match format {
        Some(f) => json!({"format": f.id(), "step": f.max_step()}),
        None => json!({"length": max}),
    };
    e.finish(s, SPLIT_LONG_LABEL, split > 0, json!({"changedCount": split, "addedCount": added, "limit": limit, "needlePoints": addressed}))
}

/// Delete with needle points selected (the marked hook in `edit.clear`): when every object the
/// selection holds is a path of stitches and it selects needle points of them (no segment), the
/// needle points are deleted as stitches (`embroidery.deleteNeedlePoints`: the neighbours merge,
/// the paths leave the selection); `None` otherwise, for upstream's Delete.
pub(crate) fn delete_selected(s: &mut Session) -> Option<Result<Value>> {
    let st = s.doc().ok()?;
    let sel = &st.selection;
    let stitches = |id: &NodeId| st.doc.node(*id).is_some_and(|n| needles::read(n).is_ok());
    let all = !sel.anchors.is_empty() && sel.segments.is_empty() && sel.objects.iter().all(stitches) && sel.anchors.keys().all(stitches);
    all.then(|| delete(s, &json!({})))
}

/// Join (the marked hook in `path.join`) of paths `ids` when any is a path of stitches: all of them
/// joined as `embroidery.joinRuns {ids}` joins them, in the order they are sewn (their stacking
/// order in their parent), never reversed; a path of stitches with art is refused. `None` when none
/// is a path of stitches, for upstream's Join.
pub(crate) fn join_paths(s: &mut Session, ids: &[NodeId]) -> Option<Result<Value>> {
    let st = s.doc().ok()?;
    let doc = st.doc.clone();
    let stitches = |id: &NodeId| doc.node(*id).is_some_and(needles::is_stitches);
    if !ids.iter().any(stitches) {
        return None;
    }
    if !ids.iter().all(stitches) {
        return Some(Err(EngineError::Other(RUNS_WITH_ART.to_string())));
    }
    if ids.len() < 2 {
        return Some(Err(EngineError::Other(ONE_RUN.to_string())));
    }
    // Each one's place among its parent's children, each parent's read once.
    let mut place: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    let mut parents = BTreeSet::new();
    for &id in ids {
        let Some(parent) = doc.parent_of(id).filter(|p| parents.insert(*p)).and_then(|p| doc.node(p)) else { continue };
        place.extend(parent.children().into_iter().flatten().enumerate().map(|(i, c)| (c.id, i)));
    }
    let mut sorted = ids.to_vec();
    sorted.sort_by_key(|id| place.get(id).copied().unwrap_or(usize::MAX));
    Some(join_runs(s, &json!({ "ids": sorted.iter().map(|i| i.0).collect::<Vec<_>>() })))
}

/// A path of stitches `id` of `d` as it is before it is cut (the marked hooks in
/// `draw2::replace_with_pieces`, `draw2::split` and `path::cut_at_anchors`), for [`run_cut`] and
/// [`cut_in_place`]; `None` for anything else.
pub(crate) fn before_cut(d: &Document, id: NodeId) -> Option<Stitches> {
    d.node(id).and_then(|n| needles::read(n).ok())
}

/// Where each needle point of a run lay before it was cut, by its coordinates' bits: its place, its
/// marks and whether a cut came before it, each taken once, in sewing order.
struct Lay {
    at: std::collections::HashMap<(u64, u64), std::collections::VecDeque<(needles::Needle, bool)>>,
}

impl Lay {
    fn of(run: &Stitches) -> Self {
        let mut at: std::collections::HashMap<(u64, u64), std::collections::VecDeque<(needles::Needle, bool)>> = Default::default();
        for r in &run.runs {
            for (i, n) in r.needles.iter().enumerate() {
                at.entry((n.p.x.to_bits(), n.p.y.to_bits())).or_default().push_back((*n, i == 0 && r.cut_before));
            }
        }
        Self { at }
    }

    /// The next needle point of the run before that lay at `p` and isn't taken yet.
    fn take(&mut self, p: Point) -> Option<(needles::Needle, bool)> {
        self.at.get_mut(&(p.x.to_bits(), p.y.to_bits()))?.pop_front()
    }
}

/// Whether two points (document points) lie at one spot, as the walk tells.
fn same_spot(a: Point, b: Point) -> bool {
    a.distance(b) * vectorcraft_stitch::MM_PER_PT < vectorcraft_stitch::SAME_POINT
}

/// `pieces` (in sewing order) cut from the path of stitches `run` was: each piece's needle points
/// take the stops and exact millimetres of the needle points of `run` they lie on, each taken once
/// (a stop on the needle point a cut made twice stays on the first piece), each of its runs the cut
/// before it that `run` had there; the first piece keeps the cut before the path, the last the cut
/// and the stop after it (the object's own too), as they were once. A needle point made by the cut
/// has none. A file's run's later piece starts after the needle point the cut made twice (the walk
/// jumps onto a file's run's first needle point and would sew it again); a manual run's keeps it
/// (art goes on from the hole the needle is in without sewing it twice).
pub(crate) fn run_cut(run: Option<&Stitches>, pieces: &mut [&mut vectorcraft_doc::Node]) {
    let Some(run) = run else { return };
    let mut lay = Lay::of(run);
    let count = pieces.len();
    let mut before: Option<Point> = None;
    for (p, piece) in pieces.iter_mut().enumerate() {
        let Ok(mut st) = needles::read(piece) else { continue };
        let more_than_one = st.len() > 1;
        if p > 0
            && !run.marks.manual
            && more_than_one
            && let Some(first) = st.runs.first_mut()
            && first.needles.first().zip(before).is_some_and(|(n, b)| same_spot(n.p, b))
        {
            first.needles.remove(0);
        }
        for r in &mut st.runs {
            r.cut_before = false;
            for (i, n) in r.needles.iter_mut().enumerate() {
                match lay.take(n.p) {
                    Some((was, cut)) => {
                        (n.stop, n.exact) = (was.stop, was.exact);
                        if i == 0 && cut {
                            r.cut_before = true;
                        }
                    }
                    None => (n.stop, n.exact) = (false, [None, None]),
                }
            }
        }
        before = st.runs.iter().rev().find_map(|r| r.needles.last()).map(|n| n.p);
        let (first, final_piece) = (p == 0, p + 1 == count);
        st.marks.trim_before = first && run.marks.trim_before;
        st.marks.trim_after = final_piece && run.marks.trim_after;
        st.marks.stop_after = final_piece && run.marks.stop_after;
        st.object_after = if final_piece { run.object_after } else { Default::default() };
        needles::write(piece, st);
    }
}

/// Path of stitches `id` of `d` (as `run` before) cut where it lies by a tool that keeps the pieces
/// as its subpaths: a needle point the cut made twice, ending a run and starting the next at one
/// spot where no run ended and started before, is kept once (the next run starts after it), as the
/// walk would sew it twice; the marks follow their needle points as they always do.
pub(crate) fn cut_in_place(d: &mut Document, id: NodeId, run: Option<&Stitches>) {
    let Some(run) = run else { return };
    let Some(n) = d.node_mut(id) else { return };
    let Ok(mut st) = needles::read(n) else { return };
    // Where runs met at one spot before, each spot as often as it was.
    let mut met: std::collections::HashMap<(u64, u64), usize> = Default::default();
    for w in run.runs.windows(2) {
        if let [a, b] = w
            && let (Some(end), Some(start)) = (a.needles.last(), b.needles.first())
            && same_spot(end.p, start.p)
        {
            *met.entry((start.p.x.to_bits(), start.p.y.to_bits())).or_default() += 1;
        }
    }
    let mut changed = false;
    let mut end: Option<Point> = None;
    for r in &mut st.runs {
        if let (Some(e), Some(first)) = (end, r.needles.first().copied())
            && same_spot(e, first.p)
            && r.needles.len() > 1
        {
            match met.get_mut(&(first.p.x.to_bits(), first.p.y.to_bits())).filter(|c| **c > 0) {
                Some(c) => *c -= 1,
                None => {
                    r.needles.remove(0);
                    changed = true;
                }
            }
        }
        if let Some(last) = r.needles.last() {
            end = Some(last.p);
        }
    }
    if changed {
        needles::write(n, st);
    }
}
