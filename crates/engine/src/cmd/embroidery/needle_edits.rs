//! The stitch commands that edit needle points ([`super::needles`] reads and writes them): each
//! takes millimetres, addresses needle points by `{id, index}` (else the selection's), is one undo
//! step (none when nothing changes), leaves the selection on what it says, and answers the lengths
//! of the stitches it made or changed (`lengths`), without a plan.

use serde_json::{Map, Value, json};
use vectorcraft_doc::{NodeId, Selection};
use vectorcraft_geom::{Point, Vec2};
use vectorcraft_stitch::mm_to_pt;

use super::needles::{self, LONGEST, Lengths, MAX_LISTED, NOTHING_ADDRESSED, Stitches, Targets};
use super::{object, only};
use crate::cmd::bad;
use crate::{EngineError, Result, Session};

/// What an edit leaves of one path: its stitches written back (removed when no needle point is
/// left).
pub(super) enum Left {
    Stitches(Stitches),
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
        .map(|(id, left)| match left {
            Left::Stitches(st) => json!({"id": id.0, "needlePoints": st.len()}),
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
