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
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(256))]

    /// Whatever a newer version writes under an object's `embroidery` key, the file opens, and saved
    /// again it holds the same, but for what reads as unset (a cut that is false or null, a run or
    /// a part that is null or empty) and what is written in one form (numbers as floats, one value
    /// for both sides as two); saved a second time, the file is the same.
    #[test]
    fn any_embroidery_an_object_carries_opens_and_saves_back(written in arb_json()) {
        let (kept, once, twice) = saved_once_and_twice(&written);
        prop_assert_eq!(kept, as_saved(&written));
        prop_assert_eq!(twice, once);
    }

    /// A run's marks (its cuts, stops and exact needle points, its record of transforms, its flags)
    /// and an object's original and Stop After, as a newer version might write them: mostly in the
    /// forms this version reads, which random JSON seldom reaches (a frame is six numbers), and
    /// sometimes not. The file opens and saves back as the first property says.
    #[test]
    fn marks_and_originals_open_and_save_back(written in arb_marks()) {
        let (kept, once, twice) = saved_once_and_twice(&written);
        prop_assert_eq!(kept, as_saved(&written));
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

/// `written` under an object's `embroidery` key, opened and saved: what the saved file holds there,
/// and the file saved once and a second time.
fn saved_once_and_twice(written: &Value) -> (Value, Value, Value) {
    let (once, id) = saved_with(written);
    let kept = node_json(&mut once.clone(), id).expect("the object is in the file").get("embroidery").cloned().unwrap_or(Value::Null);
    let twice: Value = serde_json::from_slice(&save(&load(&serde_json::to_vec(&once).unwrap()).unwrap(), false)).unwrap();
    (kept, once, twice)
}

/// A value of the shapes a run's marks and an original take, as a newer version might write it:
/// mostly as this version reads it (weights first), sometimes not.
fn arb_index() -> impl Strategy<Value = Value> {
    prop_oneof![6 => (0u64..12).prop_map(Value::from), 1 => Just(serde_json::json!(-1)), 1 => Just(serde_json::json!(1.5)), 1 => Just(Value::Null)]
}

fn arb_coordinate() -> impl Strategy<Value = Value> {
    prop_oneof![
        4 => (-4000i32..4000).prop_map(|n| Value::from(f64::from(n) / 4.0)),
        2 => (-50i64..50).prop_map(Value::from),
        1 => Just(Value::Null),
        1 => Just(Value::from("x")),
    ]
}

fn arb_list(item: impl Strategy<Value = Value>) -> impl Strategy<Value = Value> {
    prop_oneof![6 => prop::collection::vec(item, 0..5).prop_map(Value::Array), 1 => Just(Value::Null), 1 => Just(Value::from("x"))]
}

fn arb_frame() -> impl Strategy<Value = Value> {
    prop_oneof![6 => prop::collection::vec(arb_coordinate(), 6), 1 => prop::collection::vec(arb_coordinate(), 0..8)].prop_map(Value::Array)
}

fn arb_flag() -> impl Strategy<Value = Value> {
    prop_oneof![Just(Value::Bool(true)), Just(Value::Bool(false)), Just(Value::Null), Just(Value::from("x"))]
}

/// An object holding each of `entries` that is there.
fn object(entries: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(entries.into_iter().filter_map(|(k, v)| Some((k.to_string(), v?))).collect())
}

/// An object's embroidery holding a run's marks, an original and Stop After, each maybe.
fn arb_marks() -> impl Strategy<Value = Value> {
    // A needle point a cut or a stop is kept at: its place, with where it lay or without.
    let mark = prop_oneof![
        3 => prop::collection::vec(arb_index(), 2).prop_map(Value::Array),
        3 => (arb_index(), arb_index(), arb_coordinate(), arb_coordinate()).prop_map(|(s, a, x, y)| Value::Array(vec![s, a, x, y])),
        1 => prop::collection::vec(arb_index(), 0..4).prop_map(Value::Array),
    ];
    let shape = prop_oneof![6 => prop::collection::vec(arb_index(), 2), 1 => prop::collection::vec(arb_index(), 0..4)].prop_map(Value::Array);
    let exact = prop_oneof![
        6 => (arb_index(), arb_index(), arb_coordinate(), arb_coordinate()).prop_map(|(s, a, x, y)| Value::Array(vec![s, a, x, y])),
        1 => prop::collection::vec(arb_coordinate(), 0..6).prop_map(Value::Array),
    ];
    let transformed = prop_oneof![
        6 => arb_frame().prop_map(|f| serde_json::json!({ "frame": f })),
        1 => arb_frame().prop_map(|f| serde_json::json!({ "frame": f, "by": "a newer version" })),
        1 => arb_frame(),
    ];
    let o = Just;
    let run = (
        prop::option::of(arb_list(prop_oneof![arb_index(), mark.clone()])),
        prop::option::of(arb_list(mark)),
        prop::option::of(prop_oneof![4 => shape, 1 => Just(Value::Null)]),
        prop::option::of(arb_list(exact)),
        prop::option::of(transformed),
        prop::option::of(arb_flag()),
        prop::option::of(arb_flag()),
    )
        .prop_map(|(trims, stops, shape, exact, transformed, manual, stop_after)| {
            object([
                ("trims", trims),
                ("stops", stops),
                ("shape", shape),
                ("exact", exact),
                ("transformed", transformed),
                ("manual", manual),
                ("stop_after", stop_after),
            ])
        });
    let node = prop_oneof![6 => o(serde_json::json!({"kind": "path", "points": [[1.0, 2.0]]})), 1 => o(serde_json::json!([1.0]))];
    let fingerprint = prop_oneof!["[0-9a-fA-F]{16}".prop_map(Value::from), "[0-9a-f]{0,20}".prop_map(Value::from), o(Value::Null)];
    let original = (prop::option::of(arb_frame()), prop::option::of(node), prop::option::of(fingerprint), prop::option::of(o(Value::from("kept"))))
        .prop_map(|(frame, node, fingerprint, by)| object([("frame", frame), ("node", node), ("fingerprint", fingerprint), ("by", by)]));
    (prop::option::of(run), prop::option::of(original), prop::option::of(arb_flag()))
        .prop_map(|(run, original, stop_after)| object([("stitches", run), ("original", original), ("stop_after", stop_after)]))
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

/// The flags a run reads: unset when false or null.
const RUN_FLAGS: [&str; 5] = ["trim_before", "trim_after", "double_point", "manual", "stop_after"];

/// An object's embroidery as it is saved again (the reading rules of `doc::embroidery`, told
/// apart): what reads as unset dropped (`null`; a cut or a stop that is false or null; a run that
/// is null; a part, or a part's points, with nothing left in it), what is read written in one form
/// (a part's numbers as floats, one value for both sides as two, a point's coordinates as floats,
/// a run's marks as [`run_as_saved`] writes them, an original's frame as floats), the rest as
/// written.
fn as_saved(v: &Value) -> Value {
    let Value::Object(m) = v else { return v.clone() };
    let mut out = serde_json::Map::new();
    for (k, x) in m {
        match (k.as_str(), x) {
            ("trim_after" | "stop_after", Value::Bool(false) | Value::Null) | ("stitches" | "fill" | "stroke", Value::Null) => {}
            ("stitches", Value::Object(run)) => {
                out.insert(k.clone(), run_as_saved(run));
            }
            ("original", Value::Object(o)) if original_as_saved(o).is_some() => {
                out.insert(k.clone(), original_as_saved(o).unwrap_or_default());
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

/// A run as it is saved again: its flags unset when false or null; its lists of cuts, stops and
/// exact millimetres, when every entry is one it reads, written as it writes them (indices, and
/// coordinates as floats; an empty or null list dropped); its transforms since it was made, an
/// object holding only a frame, with the frame as floats; the rest as written.
fn run_as_saved(run: &serde_json::Map<String, Value>) -> Value {
    let list = |items: &[Value], read: &dyn Fn(&Value) -> Option<Value>| {
        items.iter().map(read).collect::<Option<Vec<_>>>().map(|l| (!l.is_empty()).then_some(Value::Array(l)))
    };
    let index = |v: &Value| v.as_u64().map(Value::from);
    // A needle point a cut or a stop is kept at: two indices, and where it lay as two finite
    // numbers (written as floats) or nothing.
    let mark = |v: &Value| match v.as_array().map(Vec::as_slice) {
        Some([subpath, anchor]) => Some(serde_json::json!([index(subpath)?, index(anchor)?])),
        Some([subpath, anchor, x, y]) => {
            let c = |c: &Value| c.as_f64().filter(|c| c.is_finite()).map(Value::from);
            Some(serde_json::json!([index(subpath)?, index(anchor)?, c(x)?, c(y)?]))
        }
        _ => None,
    };
    // A cut: a mark, or a bare subpath number (an earlier form), written as that subpath's first
    // anchor.
    let trim = |v: &Value| index(v).map(|s| serde_json::json!([s, 0])).or_else(|| mark(v));
    let shape = |v: &[Value]| match v {
        [subpaths, anchors] => Some(serde_json::json!([index(subpaths)?, index(anchors)?])),
        _ => None,
    };
    // An exact needle point: two indices, then each coordinate a finite number or null, not both
    // null.
    let exact = |v: &Value| match v.as_array().map(Vec::as_slice) {
        Some([subpath, anchor, x, y]) => {
            let coordinate = |c: &Value| match c {
                Value::Null => Some(Value::Null),
                c => c.as_f64().filter(|c| c.is_finite()).map(Value::from),
            };
            let (x, y) = (coordinate(x)?, coordinate(y)?);
            let (subpath, anchor) = (index(subpath)?, index(anchor)?);
            (!(x.is_null() && y.is_null())).then(|| serde_json::json!([subpath, anchor, x, y]))
        }
        _ => None,
    };
    let read = read_keys(run, |k, x| match (k, x) {
        (k, Value::Bool(false) | Value::Null) if RUN_FLAGS.contains(&k) => Some(None),
        ("trims" | "stops" | "exact" | "shape", Value::Null) => Some(None),
        ("trims", Value::Array(items)) => list(items, &trim),
        ("stops", Value::Array(items)) => list(items, &mark),
        ("shape", Value::Array(items)) => shape(items).map(Some),
        ("exact", Value::Array(items)) => list(items, &exact),
        ("transformed", Value::Object(t)) if t.len() == 1 => t.get("frame").and_then(frame_as_saved).map(|f| Some(serde_json::json!({ "frame": f }))),
        _ => None,
    });
    read.unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}

/// An original as it is saved again, when it is one the reader reads (a frame of six finite
/// numbers, and a node that is an object): its frame as floats, a fingerprint of sixteen
/// hexadecimal digits in small letters, the rest as written.
fn original_as_saved(o: &serde_json::Map<String, Value>) -> Option<Value> {
    let frame = frame_as_saved(o.get("frame")?)?;
    o.get("node").filter(|n| n.is_object())?;
    let mut out = o.clone();
    out.insert("frame".into(), frame);
    let fingerprint = o.get("fingerprint").and_then(Value::as_str).filter(|f| f.len() == 16).and_then(|f| u64::from_str_radix(f, 16).ok());
    if let Some(f) = fingerprint {
        out.insert("fingerprint".into(), Value::from(format!("{f:016x}")));
    }
    Some(Value::Object(out))
}

/// A frame as it is saved again, when it is one (six finite numbers): as floats.
fn frame_as_saved(v: &Value) -> Option<Value> {
    let coeffs: Vec<f64> = v.as_array().filter(|a| a.len() == 6)?.iter().map(Value::as_f64).collect::<Option<_>>()?;
    coeffs.iter().all(|c| c.is_finite()).then(|| serde_json::json!(coeffs))
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
