//! The embroidery data the `.vectorcraft` format keeps (VectorCraft Stitch): whatever a newer
//! version writes under an object's `embroidery` key opens and saves back, the second time as the
//! first.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::Value;
use vectorcraft_doc::{Document, Node};
use vectorcraft_format::{load, save};
use vectorcraft_testkit::embroidery::arb_json;

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Whatever a newer version writes under an object's `embroidery` key, the file opens, and saved
    /// again it holds the same, but for what reads as unset (a cut that is false or null, a run or
    /// a part that is null or empty) and what is written in one form (numbers as floats, one value
    /// for both sides as two); saved a second time, the file is the same.
    #[test]
    fn any_embroidery_an_object_carries_opens_and_saves_back(written in arb_json()) {
        let (once, id) = saved_with(&written);
        let kept = node_json(&mut once.clone(), id).expect("the object is in the file").get("embroidery").cloned().unwrap_or(Value::Null);
        prop_assert_eq!(kept, as_saved(&written));
        let twice: Value = serde_json::from_slice(&save(&load(&serde_json::to_vec(&once).unwrap()).unwrap(), false)).unwrap();
        prop_assert_eq!(twice, once);
    }

    /// Whatever a newer version writes under the document's `embroidery` key, the file opens, and
    /// saved again it holds the same, but for what reads as unset (`null`; a fabric, machine or a
    /// stitch's values with nothing left in them) and what is written in one form (numbers as
    /// floats, a colour in small letters, a start point's coordinates as floats); saved a second
    /// time, the file is the same.
    #[test]
    fn any_embroidery_a_document_carries_opens_and_saves_back(written in arb_json()) {
        let mut file: Value = serde_json::from_slice(&save(&Document::new(100.0, 100.0), false)).unwrap();
        file["document"]["embroidery"] = written.clone();
        let back = load(&serde_json::to_vec(&file).unwrap());
        prop_assert!(back.is_ok(), "{written}: {back:?}");
        let once: Value = serde_json::from_slice(&save(&back.unwrap(), false)).unwrap();
        prop_assert_eq!(once["document"].get("embroidery").cloned().unwrap_or(Value::Null), setup_as_saved(&written));
        let twice: Value = serde_json::from_slice(&save(&load(&serde_json::to_vec(&once).unwrap()).unwrap(), false)).unwrap();
        prop_assert_eq!(twice, once);
    }
}

/// A document of one square carrying `embroidery` as a file holds it, opened and saved again (the
/// file's JSON), and the square's id.
fn saved_with(embroidery: &Value) -> (Value, u64) {
    let mut d = Document::new(100.0, 100.0);
    let layer = d.layers[0].id;
    let id = d.alloc_id();
    let square = vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0));
    d.insert(Some(layer), 0, Node::path(id, square, vectorcraft_doc::Appearance::default_art())).unwrap();
    let mut file: Value = serde_json::from_slice(&save(&d, false)).unwrap();
    let node = node_json(&mut file, id.0).expect("the object is in the file");
    node["embroidery"] = embroidery.clone();
    let back = load(&serde_json::to_vec(&file).unwrap()).unwrap_or_else(|e| panic!("{embroidery}: {e:?}"));
    (serde_json::from_slice(&save(&back, false)).unwrap(), id.0)
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

/// A part's keys read as one number.
const NUMBERS: [&str; 6] = ["spacing", "length", "angle", "expand", "narrow_as_satin", "split_length"];
/// A part's keys read as one number for both sides, or two.
const SIDES: [&str; 3] = ["pull", "pull_percent", "push"];
/// A part's keys read as an id.
const IDS: [&str; 2] = ["stitch", "underlay"];

/// An object's embroidery as it is saved again (the reading rules of `doc::embroidery`, told
/// apart): what reads as unset dropped (`null`; a cut that is false or null; a run that is null; a
/// part, or a part's points, with nothing left in it), what is read written in one form (a part's
/// numbers as floats, one value for both sides as two, a point's coordinates as floats), the rest
/// as written.
fn as_saved(v: &Value) -> Value {
    let Value::Object(m) = v else { return v.clone() };
    let mut out = serde_json::Map::new();
    for (k, x) in m {
        match (k.as_str(), x) {
            ("trim_after", Value::Bool(false) | Value::Null) | ("stitches" | "fill" | "stroke", Value::Null) => {}
            ("stitches", Value::Object(run)) => {
                let run: serde_json::Map<String, Value> = run
                    .iter()
                    .filter(|(k, x)| !(matches!(k.as_str(), "trim_after" | "trim_before") && matches!(x, Value::Bool(false) | Value::Null)))
                    .map(|(k, x)| (k.clone(), x.clone()))
                    .collect();
                out.insert(k.clone(), Value::Object(run));
            }
            ("fill" | "stroke", Value::Object(part)) => {
                if let Some(part) = part_as_saved(part) {
                    out.insert(k.clone(), part);
                }
            }
            _ => {
                out.insert(k.clone(), x.clone());
            }
        }
    }
    Value::Object(out)
}

/// A part's settings as they are saved again ([`as_saved`]); `None` when nothing is left.
fn part_as_saved(m: &serde_json::Map<String, Value>) -> Option<Value> {
    let float = |x: &Value| x.as_f64().map(Value::from);
    let point = |x: &Value| match x.as_array().map(Vec::as_slice) {
        Some([a, b]) => Some(serde_json::json!([a.as_f64()?, b.as_f64()?])),
        _ => None,
    };
    let mut out = serde_json::Map::new();
    for (k, x) in m {
        let key = k.as_str();
        let known = NUMBERS.contains(&key) || SIDES.contains(&key) || IDS.contains(&key) || key == "points";
        let read = match x {
            Value::Null if known => continue,
            _ if NUMBERS.contains(&key) => float(x),
            _ if SIDES.contains(&key) => match x {
                Value::Array(a) => match a.as_slice() {
                    [a, b] => a.as_f64().zip(b.as_f64()).map(|(a, b)| serde_json::json!([a, b])),
                    _ => None,
                },
                one => one.as_f64().map(|v| serde_json::json!([v, v])),
            },
            Value::Object(points) if key == "points" => {
                let mut kept = serde_json::Map::new();
                for (p, q) in points {
                    match (p.as_str(), q) {
                        ("entry" | "exit", Value::Null) => {}
                        ("entry" | "exit", q) if point(q).is_some() => {
                            kept.insert(p.clone(), point(q).unwrap_or_default());
                        }
                        _ => {
                            kept.insert(p.clone(), q.clone());
                        }
                    }
                }
                if kept.is_empty() {
                    continue;
                }
                Some(Value::Object(kept))
            }
            _ => None,
        };
        out.insert(k.clone(), read.unwrap_or_else(|| x.clone()));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// A number as it is saved again: a float.
fn float(x: &Value) -> Option<Value> {
    x.as_f64().map(Value::from)
}

/// `m`'s keys as they are saved again: each key `read` reads written as it gives (a null of a key
/// it knows dropped, `Some(None)` dropping it too), the others as they were. `None` when nothing
/// is left.
fn read_keys(m: &serde_json::Map<String, Value>, read: impl Fn(&str, &Value) -> Option<Option<Value>>) -> Option<Value> {
    let mut out = serde_json::Map::new();
    for (k, x) in m {
        match read(k, x) {
            Some(Some(v)) => {
                out.insert(k.clone(), v);
            }
            Some(None) => {}
            None => {
                out.insert(k.clone(), x.clone());
            }
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// A stitch's values of the document's fabric as they are saved again (`None`: nothing left).
fn stitch_values_as_saved(m: &serde_json::Map<String, Value>) -> Option<Value> {
    read_keys(m, |k, x| match (k, x) {
        ("pull" | "spacing" | "spacing_percent" | "length" | "underlay", Value::Null) => Some(None),
        ("pull" | "spacing" | "spacing_percent" | "length", x) => float(x).map(Some),
        _ => None,
    })
}

/// A fabric's values (in force, or a preset's) as they are saved again: `m`'s value keys read, its
/// other keys as `other` says.
fn fabric_values_as_saved(m: &serde_json::Map<String, Value>, other: impl Fn(&str, &Value) -> Option<Option<Value>>) -> Option<Value> {
    read_keys(m, |k, x| match (k, x) {
        ("tatami" | "satin" | "stabilizer" | "topping" | "needle_point" | "thickness", Value::Null) => Some(None),
        ("tatami" | "satin", Value::Object(v)) => Some(stitch_values_as_saved(v)),
        ("stabilizer" | "topping" | "needle_point", Value::String(_)) => Some(Some(x.clone())),
        ("thickness", x) => float(x).map(Some),
        _ => other(k, x),
    })
}

/// A colour as it is saved again, when it reads as one: `#rrggbb`, in small letters.
fn color_as_saved(x: &Value) -> Option<Value> {
    let hex = x.as_str()?.strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| Value::String(format!("#{}", hex.to_ascii_lowercase())))
}

/// The document's embroidery settings as they are saved again (the reading rules of
/// `doc::embroidery`, told apart): what reads as unset dropped, what is read written in one form,
/// the rest as written.
fn setup_as_saved(v: &Value) -> Value {
    let Value::Object(m) = v else { return v.clone() };
    let fabric = |f: &serde_json::Map<String, Value>| {
        fabric_values_as_saved(f, |k, x| match (k, x) {
            ("preset" | "color", Value::Null) => Some(None),
            ("color", x) => color_as_saved(x).map(Some),
            ("preset", Value::Object(p)) if p.get("id").is_some_and(Value::is_string) => Some(read_keys(p, |k, x| match (k, x) {
                ("values", Value::Null) => Some(None),
                ("values", Value::Object(values)) => Some(fabric_values_as_saved(values, |_, _| None)),
                _ => None,
            })),
            _ => None,
        })
    };
    let machine = |m: &serde_json::Map<String, Value>| {
        read_keys(m, |k, x| match (k, x) {
            ("jump_length" | "trim_length" | "min_stitch" | "split_length" | "speed", Value::Null) => Some(None),
            ("jump_length" | "trim_length" | "min_stitch" | "split_length" | "speed", x) => float(x).map(Some),
            _ => None,
        })
    };
    let point = |x: &Value| match x.as_array().map(Vec::as_slice) {
        Some([a, b]) => Some(serde_json::json!([a.as_f64()?, b.as_f64()?])),
        _ => None,
    };
    read_keys(m, |k, x| match (k, x) {
        ("palette" | "fabric" | "thread_weight" | "join_same_color" | "machine", Value::Null) => Some(None),
        ("start", x) => point(x).map(Some),
        ("fabric", Value::Object(f)) => Some(fabric(f)),
        ("machine", Value::Object(mc)) => Some(machine(mc)),
        ("thread_weight", x) => float(x).map(Some),
        _ => None,
    })
    .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}
