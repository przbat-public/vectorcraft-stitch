//! The embroidery data the `.vectorcraft` format keeps (VectorCraft Stitch): whatever a newer
//! version writes under an object's `embroidery` key opens and saves back.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::Value;
use vectorcraft_doc::{Document, Node};
use vectorcraft_format::{load, save};
use vectorcraft_testkit::embroidery::arb_json;

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Whatever a newer version writes under an object's `embroidery` key, the file opens, and saved
    /// again it holds the same, but for what reads as unset (a cut that is false or null, a run
    /// that is null).
    #[test]
    fn any_embroidery_an_object_carries_opens_and_saves_back(written in arb_json()) {
        let mut d = Document::new(100.0, 100.0);
        let layer = d.layers[0].id;
        let id = d.alloc_id();
        let square = vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0));
        d.insert(Some(layer), 0, Node::path(id, square, vectorcraft_doc::Appearance::default_art())).unwrap();
        let mut file: Value = serde_json::from_slice(&save(&d, false)).unwrap();
        let node = node_json(&mut file, id.0).expect("the object is in the file");
        node["embroidery"] = written.clone();
        let back = load(&serde_json::to_vec(&file).unwrap());
        prop_assert!(back.is_ok(), "{written}: {back:?}");
        let mut again: Value = serde_json::from_slice(&save(&back.unwrap(), false)).unwrap();
        let kept = node_json(&mut again, id.0).expect("the object is in the file").get("embroidery").cloned().unwrap_or(Value::Null);
        prop_assert_eq!(kept, unset_dropped(&written));
    }
}

/// The node of id `id` in a saved file's JSON.
fn node_json(v: &mut Value, id: u64) -> Option<&mut Value> {
    if v.get("id").and_then(Value::as_u64) == Some(id) && v.get("kind").is_some() {
        return Some(v);
    }
    match v {
        Value::Object(m) => m.values_mut().find_map(|c| node_json(c, id)),
        Value::Array(a) => a.iter_mut().find_map(|c| node_json(c, id)),
        _ => None,
    }
}

/// An object's embroidery as it is saved again: what reads as unset dropped (`null`; a cut that is
/// false or null; a run that is null), the rest as written.
fn unset_dropped(v: &Value) -> Value {
    let Value::Object(m) = v else { return v.clone() };
    let mut out = serde_json::Map::new();
    for (k, x) in m {
        match (k.as_str(), x) {
            ("trim_after", Value::Bool(false) | Value::Null) | ("stitches", Value::Null) => {}
            ("stitches", Value::Object(run)) => {
                let run: serde_json::Map<String, Value> = run
                    .iter()
                    .filter(|(k, x)| !(matches!(k.as_str(), "trim_after" | "trim_before") && matches!(x, Value::Bool(false) | Value::Null)))
                    .map(|(k, x)| (k.clone(), x.clone()))
                    .collect();
                out.insert(k.clone(), Value::Object(run));
            }
            _ => {
                out.insert(k.clone(), x.clone());
            }
        }
    }
    Value::Object(out)
}
