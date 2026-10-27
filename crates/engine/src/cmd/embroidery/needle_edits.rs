//! The stitch commands that edit needle points ([`super::needles`] reads and writes them): each
//! takes millimetres, addresses needle points by `{id, index}` (else the selection's), is one undo
//! step (none when nothing changes), leaves the selection on what it says, and answers the lengths
//! of the stitches it made or changed (`lengths`), without a plan.

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::StitchRun;
use vectorcraft_doc::{Document, NodeId, Selection};
use vectorcraft_geom::{Point, Vec2};
use vectorcraft_stitch::{mm_to_pt, pt_to_mm};

use super::needles::{self, LONGEST, Lengths, MAX_LISTED, NOTHING_ADDRESSED, Stitches, Targets};
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
/// `label`.
fn apply(s: &mut Session, label: &str, edits: Vec<(NodeId, Left)>, sel: Selection) -> Result<()> {
    s.edit(label, |d, selection| {
        for (id, left) in edits {
            match left {
                Left::Stitches(st) => {
                    let n = d.node_mut(id).ok_or(EngineError::NoNode(id))?;
                    if !needles::write(n, &st) {
                        d.remove(id).map_err(|e| EngineError::Other(e.to_string()))?;
                    }
                }
                Left::Removed => {
                    d.remove(id).map_err(|e| EngineError::Other(e.to_string()))?;
                }
                Left::Marks(marks) => {
                    let n = d.node_mut(id).ok_or(EngineError::NoNode(id))?;
                    if let Some(e) = n.extras_mut().embroidery.as_deref_mut() {
                        e.stitches = Some(marks);
                    }
                }
            }
        }
        *selection = sel;
        Ok(())
    })
}

/// The paths an edit changed, as a reply lists them: each id with its needle points now (none:
/// removed), at most [`MAX_LISTED`].
fn listed(edits: &[(NodeId, Left)]) -> Vec<Value> {
    edits
        .iter()
        .take(MAX_LISTED)
        .filter_map(|(id, left)| match left {
            Left::Stitches(st) => Some(json!({"id": id.0, "needlePoints": st.len()})),
            Left::Removed => Some(json!({"id": id.0, "needlePoints": 0})),
            Left::Marks(_) => None,
        })
        .collect()
}

/// The selection `sel` with path `id`'s needle points kept as they were (an edit that moves no
/// needle point to another place in the path), written as `st` now lays them out.
fn kept_selection(sel: &mut Selection, before: &Selection, id: NodeId, was: &Stitches, st: &Stitches) {
    if before.anchors.contains_key(&id) {
        let k = needles::selected_in(before, id, was);
        needles::select_in(sel, id, st, &k);
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
    let (min, max) = (needles::shortest_stitch(&s.doc()?.doc), LONGEST);
    let mut lengths = Lengths::default();
    let (mut edits, mut moved) = (Vec::new(), 0usize);
    let mut sel = before.clone();
    for (id, was, indices) in t.paths {
        let mut st = was.clone();
        let mut changed = false;
        for &k in &indices {
            let Some(n) = st.needle_mut(k) else { continue };
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
        if !changed {
            continue;
        }
        for &k in &indices {
            lengths.around(&st, k, min, max);
        }
        kept_selection(&mut sel, &before, id, &was, &st);
        edits.push((id, Left::Stitches(st)));
    }
    let reply = json!({
        "changedCount": moved,
        "paths": listed(&edits),
        "pathCount": edits.len(),
        "lengths": lengths.json(min, max),
        "passedOver": t.passed_over.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect::<Vec<_>>(),
    });
    if !edits.is_empty() {
        apply(s, MOVE_LABEL, edits, sel)?;
    }
    Ok(reply)
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
    let doc = s.doc()?.doc.clone();
    let before = s.doc()?.selection.clone();
    let (min, max) = (needles::shortest_stitch(&doc), LONGEST);
    let mut lengths = Lengths::default();
    let mut sel = before.clone();
    let (mut edits, mut inserted, mut passed_over) = (Vec::new(), Vec::new(), Vec::new());
    let mut added_all = 0usize;
    if let Some((id, index)) = place {
        let n = doc.node(id).ok_or(EngineError::NoNode(id))?;
        let mut st = needles::read(n).map_err(|why| bad(INSERT, format!("object {}: {}", id.0, needles::said(why))))?;
        if !doc.is_editable(id) {
            return Err(bad(INSERT, format!("object {}: {}", id.0, needles::said(needles::reason::LOCKED))));
        }
        let len = st.len();
        if index > len {
            return Err(bad(INSERT, format!("object {}: index {index}, past its {len} needle points (at most {len}: after the last)", id.0)));
        }
        let points = match at {
            Some(points) => points,
            // The middle of the stitch arriving at it: a stitch, not the jump onto a run's first.
            None => {
                let (r, i) = st.locate(index).filter(|&(_, i)| i > 0).ok_or_else(|| {
                    bad(
                        INSERT,
                        format!("object {}: no stitch arrives at needle point {index} (a run starts there, or the path ends): give `at`", id.0),
                    )
                })?;
                let run = st.runs.get(r).ok_or_else(|| bad(INSERT, "no such run"))?;
                let (Some(a), Some(b)) = (run.needles.get(i - 1), run.needles.get(i)) else { return Err(bad(INSERT, "no such needle point")) };
                vec![a.p.midpoint(b.p)]
            }
        };
        if len + points.len() > needles::MAX_NEEDLE_POINTS {
            return Err(bad(INSERT, format!("object {}: more than {} needle points", id.0, needles::MAX_NEEDLE_POINTS)));
        }
        // Before needle point `index`, in its run; after the last, in the last run.
        let (r, i) = st.locate(index).unwrap_or_else(|| (st.runs.len().saturating_sub(1), st.runs.last().map_or(0, |r| r.needles.len())));
        if st.runs.is_empty() {
            st.runs.push(needles::Run::default());
        }
        let Some(run) = st.runs.get_mut(r) else { return Err(bad(INSERT, "no such run")) };
        let new = points.iter().map(|&p| needles::Needle { p, stop: false, exact: [None, None] });
        run.needles.splice(i..i, new);
        let first = st.index_of(r, i);
        let added: std::collections::BTreeSet<usize> = (first..first + points.len()).collect();
        added_all = added.len();
        for &k in &added {
            lengths.around(&st, k, min, max);
            if inserted.len() < MAX_LISTED {
                inserted.push(json!({"id": id.0, "index": k}));
            }
        }
        needles::select_in(&mut sel, id, &st, &added);
        edits.push((id, Left::Stitches(st)));
    } else {
        let (t, _) = targets(s, &o, INSERT)?;
        passed_over = t.passed_over;
        for (id, st, indices) in t.paths {
            let mut out = st.clone();
            let mut added = std::collections::BTreeSet::new();
            let mut k = 0usize;
            let mut at_now = 0usize;
            for (r, run) in st.runs.iter().enumerate() {
                let mut needles: Vec<needles::Needle> = Vec::with_capacity(run.needles.len());
                for (i, n) in run.needles.iter().enumerate() {
                    needles.push(*n);
                    at_now += 1;
                    if indices.contains(&k)
                        && let Some(next) = run.needles.get(i + 1)
                    {
                        needles.push(needles::Needle { p: n.p.midpoint(next.p), stop: false, exact: [None, None] });
                        added.insert(at_now);
                        at_now += 1;
                    }
                    k += 1;
                }
                if let Some(slot) = out.runs.get_mut(r) {
                    slot.needles = needles;
                }
            }
            if added.is_empty() {
                continue;
            }
            added_all += added.len();
            if added_all > MAX_ADDED || out.len() > needles::MAX_NEEDLE_POINTS {
                return Err(bad(INSERT, format!("more than {MAX_ADDED} needle points added, or a path past {}", needles::MAX_NEEDLE_POINTS)));
            }
            for &k in &added {
                lengths.around(&out, k, min, max);
                if inserted.len() < MAX_LISTED {
                    inserted.push(json!({"id": id.0, "index": k}));
                }
            }
            needles::select_in(&mut sel, id, &out, &added);
            edits.push((id, Left::Stitches(out)));
        }
    }
    let reply = json!({
        "changedCount": added_all,
        "inserted": inserted,
        "paths": listed(&edits),
        "pathCount": edits.len(),
        "lengths": lengths.json(min, max),
        "passedOver": passed_over.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect::<Vec<_>>(),
    });
    if !edits.is_empty() {
        apply(s, INSERT_LABEL, edits, sel)?;
    }
    Ok(reply)
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

/// The path of stitches sewn beside `id` in its parent, the one before it when `before`, else the
/// one after, passing over those in `gone` (removed by the same edit).
fn sibling(doc: &Document, id: NodeId, before: bool, gone: &std::collections::BTreeSet<NodeId>) -> Option<NodeId> {
    let parent = doc.parent_of(id).and_then(|p| doc.node(p))?;
    let children = parent.children()?;
    let at = children.iter().position(|c| c.id == id)?;
    let stitches = |c: &&std::sync::Arc<vectorcraft_doc::Node>| !gone.contains(&c.id) && needles::read(c).is_ok() && doc.is_editable(c.id);
    if before { children.get(..at)?.iter().rev().find(stitches).map(|c| c.id) } else { children.get(at + 1..)?.iter().find(stitches).map(|c| c.id) }
}

/// `embroidery.deleteNeedlePoints {needlePoints?, ranges?}`: each needle point addressed (or
/// selected) deleted, the stitches beside it becoming one (neighbours merge); a stop after one
/// passes to the needle point before it that stays. A path left with no needle point is removed:
/// its cut and stop pass to the path of stitches sewn before it in its parent (a cut, else to the
/// one after), or are dropped, and the reply says which. The paths it changed leave the selection,
/// so a second Delete deletes nothing.
pub(super) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let o = params(p, DELETE, &[])?;
    let (t, before) = targets(s, &o, DELETE)?;
    let doc = s.doc()?.doc.clone();
    let (min, max) = (needles::shortest_stitch(&doc), LONGEST);
    let mut lengths = Lengths::default();
    let mut sel = before.clone();
    let (mut edits, mut moved, mut dropped, mut removed) = (Vec::new(), Vec::new(), Vec::new(), std::collections::BTreeSet::new());
    let mut deleted = 0usize;
    for (id, st, indices) in t.paths {
        let mut out = st.clone();
        // Each kept needle point a deleted one followed (its stitch merged), by its new index.
        let mut merged = std::collections::BTreeSet::new();
        let (mut k, mut kept) = (0usize, 0usize);
        // The last needle point kept, by run and place: a stop of one deleted after it passes to it.
        let mut last: Option<(usize, usize)> = None;
        for (r, run) in st.runs.iter().enumerate() {
            let mut needles: Vec<needles::Needle> = Vec::with_capacity(run.needles.len());
            for n in &run.needles {
                if indices.contains(&k) {
                    deleted += 1;
                    // The stitch from the needle point kept before it, in its run, now goes on.
                    if !needles.is_empty() {
                        merged.insert(kept.saturating_sub(1));
                    }
                    if n.stop {
                        let to = match last {
                            Some((lr, li)) if lr == r => needles.get_mut(li),
                            Some((lr, li)) => out.runs.get_mut(lr).and_then(|run| run.needles.get_mut(li)),
                            None => None,
                        };
                        match to {
                            Some(m) => {
                                m.stop = true;
                                moved.push(function_json("stop", id, Some(kept.saturating_sub(1)), false));
                            }
                            None => dropped.push(function_json("stop", id, None, false)),
                        }
                    }
                } else {
                    needles.push(*n);
                    last = Some((r, needles.len() - 1));
                    kept += 1;
                }
                k += 1;
            }
            if let Some(slot) = out.runs.get_mut(r) {
                slot.needles = needles;
            }
        }
        sel.remove(id);
        sel.anchors.remove(&id);
        if out.len() > 0 {
            for &m in &merged {
                lengths.around(&out, m, min, max);
            }
            edits.push((id, Left::Stitches(out)));
            continue;
        }
        removed.insert(id);
        edits.push((id, Left::Removed));
    }
    // A path removed: its cut and its stop pass to its neighbour, as the gap it leaves.
    let mut passed: Vec<(NodeId, StitchRun)> = Vec::new();
    for &id in &removed {
        let Some(n) = doc.node(id) else { continue };
        let e = n.extras().embroidery.as_deref();
        let marks = e.and_then(|e| e.stitches.as_ref());
        let cut = marks.is_some_and(|m| m.trim_before || m.trim_after) || e.is_some_and(|e| e.trim_after);
        let stop = marks.is_some_and(|m| m.stop_after || !m.stops.is_empty()) || e.is_some_and(|e| e.stop_after);
        let mut give = |to: NodeId, f: &dyn Fn(&mut StitchRun), edits: &mut Vec<(NodeId, Left)>| {
            if let Some((_, Left::Stitches(st))) = edits.iter_mut().find(|(e, _)| *e == to) {
                f(&mut st.marks);
                return;
            }
            let slot = passed.iter_mut().find(|(p, _)| *p == to);
            match slot {
                Some((_, m)) => f(m),
                None => {
                    if let Some(mut m) = doc.node(to).and_then(|n| n.extras().embroidery.as_deref()).and_then(|e| e.stitches.clone()) {
                        f(&mut m);
                        passed.push((to, m));
                    }
                }
            }
        };
        let before_it = sibling(&doc, id, true, &removed);
        let after_it = sibling(&doc, id, false, &removed);
        let last_of = |to: NodeId| doc.node(to).and_then(|n| needles::read(n).ok()).map(|st| st.len().saturating_sub(1));
        if cut {
            match (before_it, after_it) {
                (Some(to), _) => {
                    give(to, &|m| m.trim_after = true, &mut edits);
                    moved.push(function_json("trim", to, last_of(to), false));
                }
                (None, Some(to)) => {
                    give(to, &|m| m.trim_before = true, &mut edits);
                    moved.push(function_json("trim", to, None, true));
                }
                (None, None) => dropped.push(function_json("trim", id, None, false)),
            }
        }
        if stop {
            match before_it {
                Some(to) => {
                    give(to, &|m| m.stop_after = true, &mut edits);
                    moved.push(function_json("stop", to, last_of(to), false));
                }
                None => dropped.push(function_json("stop", id, None, false)),
            }
        }
    }
    edits.extend(passed.into_iter().map(|(id, m)| (id, Left::Marks(m))));
    let reply = json!({
        "changedCount": deleted,
        "paths": listed(&edits),
        "pathCount": edits.iter().filter(|(_, l)| !matches!(l, Left::Marks(_))).count(),
        "removed": removed.iter().take(MAX_LISTED).map(|id| id.0).collect::<Vec<_>>(),
        "removedCount": removed.len(),
        "moved": moved.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "dropped": dropped.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "lengths": lengths.json(min, max),
        "passedOver": t.passed_over.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect::<Vec<_>>(),
    });
    if deleted > 0 {
        apply(s, DELETE_LABEL, edits, sel)?;
    }
    Ok(reply)
}

/// A path's runs rebuilt needle point by needle point, each needle point of the path it was rebuilt
/// from given its new index.
struct Rebuilt {
    runs: Vec<needles::Run>,
    /// The new index of each needle point of the path before, by its index there.
    moved_to: Vec<usize>,
    count: usize,
}

impl Rebuilt {
    fn new(before: usize) -> Self {
        Self { runs: Vec::new(), moved_to: vec![0; before], count: 0 }
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
            *slot = self.count;
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

    /// The path rebuilt, with the marks of `from`.
    fn done(self, from: &Stitches) -> Stitches {
        Stitches { runs: self.runs, marks: from.marks.clone() }
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
fn split_before(st: &Stitches, at: &std::collections::BTreeSet<usize>, ties: Option<f64>, cut: bool, stop: bool) -> Rebuilt {
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

/// The selection `sel` with path `id`'s needle points (as `before` selected them, of `was`) taken to
/// their new places `moved_to` in `st`.
fn moved_selection(sel: &mut Selection, before: &Selection, id: NodeId, was: &Stitches, st: &Stitches, moved_to: &[usize]) {
    if before.anchors.contains_key(&id) {
        let k = needles::selected_in(before, id, was).into_iter().filter_map(|k| moved_to.get(k).copied()).collect();
        needles::select_in(sel, id, st, &k);
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
    let doc = s.doc()?.doc.clone();
    let (min, max) = (needles::shortest_stitch(&doc), LONGEST);
    let ties = ties(&o, SPLIT, min)?;
    let mut lengths = Lengths::default();
    let mut sel = before.clone();
    let (mut edits, mut jumps, mut split, mut unchanged) = (Vec::new(), Vec::new(), 0usize, 0usize);
    for (id, st, indices) in t.paths {
        // A run's first already follows a lift.
        let firsts: std::collections::BTreeSet<usize> = st
            .runs
            .iter()
            .scan(0usize, |at, r| {
                let first = *at;
                *at += r.needles.len();
                Some(first)
            })
            .collect();
        let at: std::collections::BTreeSet<usize> = indices.iter().copied().filter(|k| !firsts.contains(k)).collect();
        unchanged += indices.len() - at.len();
        if at.is_empty() {
            continue;
        }
        split += at.len();
        let rebuilt = split_before(&st, &at, ties, false, false);
        let moved_to = rebuilt.moved_to.clone();
        let out = rebuilt.done(&st);
        for &k in &at {
            let Some(&landing) = moved_to.get(k) else { continue };
            lengths.around(&out, landing, min, max);
            let from = landing.checked_sub(1).and_then(|b| needle_at(&out, b));
            if let (Some(a), Some(b)) = (from, needle_at(&out, landing))
                && jumps.len() < MAX_LISTED
            {
                jumps.push(json!({"id": id.0, "index": landing, "length": pt_to_mm(a.distance(b))}));
            }
        }
        moved_selection(&mut sel, &before, id, &st, &out, &moved_to);
        edits.push((id, Left::Stitches(out)));
    }
    let reply = json!({
        "changedCount": split,
        "unchangedCount": unchanged,
        "paths": listed(&edits),
        "pathCount": edits.len(),
        "jumps": jumps,
        "lengths": lengths.json(min, max),
        "passedOver": t.passed_over.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect::<Vec<_>>(),
    });
    if !edits.is_empty() {
        apply(s, SPLIT_LABEL, edits, sel)?;
    }
    Ok(reply)
}

/// Needle point `index` of `st`, where it is.
fn needle_at(st: &Stitches, index: usize) -> Option<Point> {
    let (r, i) = st.locate(index)?;
    st.runs.get(r)?.needles.get(i).map(|n| n.p)
}

pub(super) const JOIN: &str = "embroidery.joinRuns";
/// The undo step's name, the command's label.
pub(super) const JOIN_LABEL: &str = "Join Runs";
/// The message Join Runs gives for paths it can't join: translated (the interface shows it).
pub(super) const NOT_JOINABLE: &str = "these runs aren't sewn one right after another in one colour block";

/// `st` with each run whose first needle point is in `at` joined to the run before it: the jump
/// onto it becomes a stitch (a first needle point lying where the run before ends is kept once), a
/// cut before it and a stop after the run before go; how many of each went.
fn join_within(st: &Stitches, at: &std::collections::BTreeSet<usize>) -> (Rebuilt, usize, usize) {
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
                    *slot = out.count.saturating_sub(1);
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
    let doc = s.doc()?.doc.clone();
    let before = s.doc()?.selection.clone();
    let (min, max) = (needles::shortest_stitch(&doc), LONGEST);
    let mut lengths = Lengths::default();
    let mut sel = before.clone();
    let (mut edits, mut dropped, mut joined) = (Vec::new(), Vec::new(), 0usize);
    let mut removed = Vec::new();
    if let Some(ids) = o.get("ids") {
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
        let mut paths = Vec::with_capacity(ids.len());
        for &id in &ids {
            let n = doc.node(id).ok_or(EngineError::NoNode(id))?;
            let st = needles::read(n).map_err(|why| bad(JOIN, format!("object {}: {}", id.0, needles::said(why))))?;
            if !doc.is_editable(id) {
                return Err(bad(JOIN, format!("object {}: {}", id.0, needles::said(needles::reason::LOCKED))));
            }
            paths.push((id, n, st));
        }
        // Next to one another in one parent, in the order given, in one colour, of one kind.
        let parent = doc.parent_of(ids.first().copied().unwrap_or(NodeId(0)));
        let children: Vec<NodeId> =
            parent.and_then(|p| doc.node(p)).and_then(|p| p.children()).map(|c| c.iter().map(|n| n.id).collect()).unwrap_or_default();
        let first_at = children.iter().position(|c| Some(c) == ids.first());
        let next_to = first_at.is_some_and(|at| children.get(at..at + ids.len()).is_some_and(|run| run == ids.as_slice()));
        let colour = paths.first().and_then(|(_, n, _)| stroke_colour(n));
        let kind = paths.first().map(|(_, _, st)| st.marks.manual);
        let alike = paths.iter().all(|(_, n, st)| stroke_colour(n) == colour && colour.is_some() && Some(st.marks.manual) == kind);
        if !next_to || !alike {
            return Err(EngineError::Other(NOT_JOINABLE.to_string()));
        }
        let mut whole = paths.first().map(|(_, _, st)| st.clone()).ok_or_else(|| bad(JOIN, "no path"))?;
        let first_id = ids.first().copied().unwrap_or(NodeId(0));
        // Each path's needle points, by the index they take in the joined path.
        let mut offsets = vec![0usize];
        for (id, n, st) in paths.iter().skip(1) {
            let e = n.extras().embroidery.as_deref();
            // The gap: the cut and stop after the path before, the cut before this one.
            if whole.marks.trim_after || st.marks.trim_before {
                dropped.push(json!({"function": "trim", "id": id.0}));
            }
            if whole.marks.stop_after || whole.runs.last().and_then(|r| r.needles.last()).is_some_and(|n| n.stop) {
                dropped.push(json!({"function": "stop", "id": id.0}));
            }
            if let Some(last) = whole.runs.last_mut().and_then(|r| r.needles.last_mut()) {
                last.stop = false;
            }
            let at = whole.len();
            offsets.push(at);
            whole.runs.extend(st.runs.iter().cloned());
            let (rebuilt, _, _) = join_within(&whole, &[at].into_iter().collect());
            whole = rebuilt.done(&whole);
            // The joined path ends as the last one did.
            whole.marks.trim_after = st.marks.trim_after || e.is_some_and(|e| e.trim_after);
            whole.marks.stop_after = st.marks.stop_after || e.is_some_and(|e| e.stop_after);
            for (k, v) in st.marks.extra.iter() {
                if !whole.marks.extra.contains_key(k) {
                    std::sync::Arc::make_mut(&mut whole.marks.extra).insert(k.clone(), v.clone());
                }
            }
            removed.push(*id);
            joined += 1;
            lengths.around(&whole, at.saturating_sub(1), min, max);
        }
        // The selection: each path's needle points at their places in the joined one.
        let mut selected = std::collections::BTreeSet::new();
        for ((id, _, st), offset) in paths.iter().zip(&offsets) {
            selected.extend(needles::selected_in(&before, *id, st).into_iter().map(|k| (k + offset).min(whole.len().saturating_sub(1))));
            sel.remove(*id);
            sel.anchors.remove(id);
        }
        if !selected.is_empty() {
            needles::select_in(&mut sel, first_id, &whole, &selected);
        }
        edits.push((first_id, Left::Stitches(whole)));
        edits.extend(removed.iter().map(|id| (*id, Left::Removed)));
    } else {
        let (t, _) = targets(s, &o, JOIN)?;
        for (id, st, indices) in t.paths {
            let (rebuilt, cuts, stops) = join_within(&st, &indices);
            let gaps = st.runs.len() - rebuilt.runs.len();
            if gaps == 0 {
                continue;
            }
            joined += gaps;
            dropped.extend(std::iter::repeat_n(json!({"function": "trim", "id": id.0}), cuts));
            dropped.extend(std::iter::repeat_n(json!({"function": "stop", "id": id.0}), stops));
            let moved_to = rebuilt.moved_to.clone();
            let out = rebuilt.done(&st);
            for &k in &indices {
                if let Some(&at) = moved_to.get(k) {
                    lengths.around(&out, at, min, max);
                }
            }
            moved_selection(&mut sel, &before, id, &st, &out, &moved_to);
            edits.push((id, Left::Stitches(out)));
        }
    }
    let reply = json!({
        "changedCount": joined,
        "paths": listed(&edits),
        "pathCount": edits.iter().filter(|(_, l)| matches!(l, Left::Stitches(_))).count(),
        "removed": removed.iter().take(MAX_LISTED).map(|id| id.0).collect::<Vec<_>>(),
        "dropped": dropped.into_iter().take(MAX_LISTED).collect::<Vec<_>>(),
        "lengths": lengths.json(min, max),
    });
    if joined > 0 {
        apply(s, JOIN_LABEL, edits, sel)?;
    }
    Ok(reply)
}
