//! `embroidery.setEntryExit`: where one object's part starts and ends, in document points, and,
//! with `snap`, the point taken to where its stitches can start or end ([`snapped`]): anywhere on a
//! fill's outlines (a hole's edge too), anywhere on a closed line, the nearer end of an open one.
//! The embroidery tool passes `snap`, so that a point dragged on the canvas and one an agent gives
//! land in the same place.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::outline;
use vectorcraft_doc::embroidery::{PartKey, PartKind, PartValue};
use vectorcraft_doc::{Node, NodeId};
use vectorcraft_geom::Point;

use super::objects::{ENTRY_EXIT, edit_each, has_part, one_taker, part_param, point_param, value_json};
use super::{edit_counted, object, only};
use crate::cmd::bad;
use crate::{EngineError, Result, Session};

/// `embroidery.setEntryExit`.
pub(super) fn set_entry_exit(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setEntryExit";
    let o = object(p, C)?;
    only(&o, &["id", "part", "entry", "exit", "snap"], C)?;
    let kind = part_param(o.get("part").unwrap_or(&Value::Null), C)?;
    let snap = match o.get("snap") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("`snap` {v}: true or false")))?,
    };
    let mut points = Vec::new();
    for (key, name) in [(PartKey::Entry, "entry"), (PartKey::Exit, "exit")] {
        if let Some(v) = o.get(name) {
            points.push((key, point_param(v, name, C)?));
        }
    }
    if points.is_empty() {
        return Err(bad(C, "give entry, exit or both: [x, y] in points, or null to unset one"));
    }
    let d = &s.doc()?.doc;
    let id = match o.get("id").filter(|v| !v.is_null()) {
        Some(v) => NodeId(v.as_u64().ok_or_else(|| bad(C, format!("`id` {v}: an object id")))?),
        None => match s.doc()?.selection.objects.as_slice() {
            [one] => *one,
            _ => return Err(bad(C, "select one object, or give its id")),
        },
    };
    let id = one_taker(d, id)?;
    let Some(n) = d.node(id).filter(|n| has_part(n, kind)) else {
        return Err(EngineError::Other(format!("object {} has no `{}`", id.0, kind.id())));
    };
    let points: Vec<(PartKey, Option<PartValue>)> =
        points.into_iter().map(|(key, at)| (key, at.map(|q| PartValue::Point(if snap { snapped(n, kind, q) } else { q })))).collect();
    let changed = edit_counted(s, ENTRY_EXIT, |d| {
        edit_each(d, &[id], |e| {
            e.edit_part(kind, |part| {
                for (key, value) in &points {
                    part.set(*key, value.clone());
                }
            })
        })
    })?;
    let d = &s.doc()?.doc;
    let stored =
        |key: PartKey| d.node(id).and_then(|n| n.extras().embroidery.as_deref()?.part(kind)?.get(key)).as_ref().map_or(Value::Null, value_json);
    Ok(json!({"id": id.0, "part": kind.id(), "entry": stored(PartKey::Entry), "exit": stored(PartKey::Exit), "changedCount": changed}))
}

/// `q` (document points) taken to where part `kind` of `n` can start or end: a fill's nearest point
/// on any of its outlines (a fill closes each subpath); a stroke's nearest point on a closed subpath,
/// or the nearer end of an open one, of whichever subpath is nearest. `q` itself where `n` has no
/// path to snap to, or the point found isn't a number.
pub(crate) fn snapped(n: &Node, kind: PartKind, q: Point) -> Point {
    let mut best: Option<(f64, Point)> = None;
    let mut keep = |p: Point| {
        let d = p.distance(q);
        if d.is_finite() && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, p));
        }
    };
    for (pts, closed) in outline::subpaths(n) {
        let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { continue };
        if kind == PartKind::Stroke && !closed {
            keep(first);
            keep(last);
            continue;
        }
        let mut ring = pts.clone();
        ring.push(first);
        keep(outline::nearest_on(&ring, q).unwrap_or(first));
    }
    best.map_or(q, |(_, p)| p)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_support::{blank, run};

    fn info(s: &mut crate::Session, id: u64, part: &str) -> serde_json::Value {
        run(s, "embroidery.info", json!({"ids": [id]}))["objects"][0]["settings"][part].clone()
    }

    /// With `snap`, a fill's point lands on its nearest outline, a hole's edge too; an open
    /// stroke's at its nearer end; a closed stroke's anywhere on it; without, where it is given.
    #[test]
    fn a_snapped_point_lands_where_the_stitches_can_start() {
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let square =
            run(&mut s, "path.create", json!({"d": "M 0 0 L 100 0 L 100 100 L 0 100 Z M 40 40 L 40 60 L 60 60 L 60 40 Z"}))["id"].as_u64().unwrap();
        run(&mut s, "embroidery.setEntryExit", json!({"id": square, "part": "fill", "entry": [30, 3], "exit": [50, 45], "snap": true}));
        let fill = info(&mut s, square, "fill");
        assert_eq!((fill["entry"].clone(), fill["exit"].clone()), (json!([30.0, 0.0]), json!([50.0, 40.0])), "the outline, the hole's edge");
        run(&mut s, "embroidery.setEntryExit", json!({"id": square, "part": "stroke", "entry": [97, 50], "snap": true}));
        assert_eq!(info(&mut s, square, "stroke")["entry"], json!([100.0, 50.0]), "a closed stroke: anywhere on it");
        run(&mut s, "paint.setFill", json!({"none": true}));
        let line = run(&mut s, "path.create", json!({"d": "M 200 0 L 300 0"}))["id"].as_u64().unwrap();
        run(&mut s, "embroidery.setEntryExit", json!({"id": line, "part": "stroke", "entry": [280, 5], "exit": [210, -2], "snap": true}));
        let stroke = info(&mut s, line, "stroke");
        assert_eq!((stroke["entry"].clone(), stroke["exit"].clone()), (json!([300.0, 0.0]), json!([200.0, 0.0])), "an open line's nearer ends");
        run(&mut s, "embroidery.setEntryExit", json!({"id": line, "part": "stroke", "entry": [280, 5]}));
        assert_eq!(info(&mut s, line, "stroke")["entry"], json!([280.0, 5.0]), "without snap, as given");
        let e = s.execute("embroidery.setEntryExit", &json!({"id": line, "part": "stroke", "entry": [1, 1], "snap": "yes"})).unwrap_err().to_string();
        assert!(e.contains("true or false"), "{e}");
    }
}
