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
