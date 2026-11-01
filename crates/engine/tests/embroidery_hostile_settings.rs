//! Embroidery settings as hostile as a file can hold them, on objects and in the document: numbers
//! at JSON's edges (±1.7e308, 1e-300), long strings, values of every type under every key the
//! settings read, points of any shape. Planned, read by `embroidery.info`, edited by the settings
//! commands (and by every embroidery command's junk), transformed (singular transforms among them)
//! and cut into pieces by the commands that make objects from others, they never panic; what is
//! planned is finite, and the document saves and reads back as it was.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod embroidery_common;

use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Map, Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::{Embroidery, EmbroiderySetup};
use vectorcraft_engine::{EmbroideryOptions, Session};
use vectorcraft_geom::Affine;
use vectorcraft_testkit::embroidery::arb_json;
use vectorcraft_testkit::invariants::check_native_roundtrip;

/// The keys a part's settings are saved under, and its points'.
const PART_KEYS: [&str; 12] =
    ["stitch", "spacing", "length", "angle", "pull", "pull_percent", "push", "expand", "narrow_as_satin", "underlay", "split_length", "points"];
const POINT_KEYS: [&str; 4] = ["entry", "exit", "rails", "frame"];
/// The keys the document's settings are saved under, and its fabric's and machine's.
const SETUP_KEYS: [&str; 7] = ["palette", "start", "label", "fabric", "thread_weight", "join_same_color", "machine"];
const FABRIC_KEYS: [&str; 9] = ["preset", "tatami", "satin", "stabilizer", "topping", "needle_point", "thickness", "color", "values"];
const STITCH_KEYS: [&str; 5] = ["pull", "spacing", "spacing_percent", "length", "underlay"];
const MACHINE_KEYS: [&str; 5] = ["jump_length", "trim_length", "min_stitch", "split_length", "speed"];

/// The keys a run of stitches' marks are saved under.
const RUN_KEYS: [&str; 10] = ["manual", "trim_before", "trim_after", "stop_after", "double_point", "trims", "stops", "shape", "exact", "transformed"];
/// A string far longer than any setting holds.
const LONG: usize = 10_000;

/// A value at the edges of what JSON holds, or any value at all.
fn edge() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!(1.7e308)),
        Just(json!(-1.7e308)),
        Just(json!(1e-300)),
        Just(json!(-0.0)),
        Just(json!([1.7e308, -1.7e308])),
        Just(json!([4e6, -4e6])),
        Just(json!("x".repeat(LONG))),
        Just(json!("nearest")),
        Just(json!([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])),
        arb_json(),
    ]
}

/// An object of `keys`, each with a value `value` gives.
fn object_of(keys: &'static [&'static str], value: BoxedStrategy<Value>) -> impl Strategy<Value = Value> {
    prop::collection::btree_map(prop::sample::select(keys), value, 0..keys.len())
        .prop_map(|m| Value::Object(m.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

/// A part's settings: its keys with edge values, its points with points of any shape.
fn part() -> impl Strategy<Value = Value> {
    let points = object_of(&POINT_KEYS, edge().boxed()).boxed();
    let value = prop_oneof![4 => edge(), 1 => points].boxed();
    object_of(&PART_KEYS, value)
}

/// A run of stitches' marks as hostile as a file holds them: indices at the integers' edges, marks
/// at 1e300, frames past any number, or anything at all under their keys.
fn run_marks() -> impl Strategy<Value = Value> {
    let marks = prop_oneof![
        edge(),
        Just(json!([[0, 0, 1e300, -1e300], [u32::MAX, u64::MAX], [0, 1], [-1, 2]])),
        Just(json!((0..2_000).map(|i| json!([i % 3, i])).collect::<Vec<_>>())),
        Just(json!({"frame": [1e300, 0, 0, 1e300, 0, 0]})),
        Just(json!([u64::MAX, u64::MAX])),
    ];
    object_of(&RUN_KEYS, prop_oneof![2 => edge(), 1 => marks].boxed())
}

/// A kept original as hostile as a file holds it: a frame past the document's coordinates, a node
/// that is a layer, type with values out of range, or anything at all.
fn original() -> impl Strategy<Value = Value> {
    let frame = prop_oneof![Just(json!([1e300, 0, 0, 1e300, 0, 0])), Just(json!([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])), edge()];
    let node = prop_oneof![
        Just(json!({"id": 1, "kind": {"type": "layer", "color": "blue", "children": []}})),
        Just(json!({"id": 1, "kind": {"type": "path", "path": {"subpaths": [{"anchors": [{"p": [1e300, 1e300]}], "closed": false}]}}})),
        edge(),
    ];
    prop_oneof![1 => edge(), 3 => (frame, node, edge()).prop_map(|(frame, node, fingerprint)| json!({"frame": frame, "node": node, "fingerprint": fingerprint}))]
}

/// An object's settings: its parts (or something else under their keys), its cut and stop, a run's
/// marks, a kept original, kept keys.
fn object_settings() -> impl Strategy<Value = Value> {
    (
        prop_oneof![3 => part(), 1 => edge()],
        prop_oneof![3 => part(), 1 => edge()],
        any::<bool>(),
        prop::option::of(arb_json()),
        prop::option::of(run_marks()),
        prop::option::of(original()),
        prop::option::of(edge()),
    )
        .prop_map(|(fill, stroke, cut, extra, stitches, original, stop)| {
            let mut m = Map::new();
            m.insert("fill".into(), fill);
            m.insert("stroke".into(), stroke);
            m.insert("trim_after".into(), json!(cut));
            if let Some(extra) = extra {
                m.insert("branch".into(), extra);
            }
            if let Some(stitches) = stitches {
                m.insert("stitches".into(), stitches);
            }
            if let Some(original) = original {
                m.insert("original".into(), original);
            }
            if let Some(stop) = stop {
                m.insert("stop_after".into(), stop);
            }
            Value::Object(m)
        })
}

/// The document's settings: its keys with edge values, its fabric's and machine's too.
fn document_settings() -> impl Strategy<Value = Value> {
    let stitch = object_of(&STITCH_KEYS, edge().boxed()).boxed();
    let fabric = object_of(&FABRIC_KEYS, prop_oneof![2 => edge(), 1 => stitch].boxed()).boxed();
    let machine = object_of(&MACHINE_KEYS, edge().boxed()).boxed();
    object_of(&SETUP_KEYS, prop_oneof![2 => edge(), 1 => fabric, 1 => machine].boxed())
}

/// A transform: ordinary ones, and singular or enormous ones.
fn transform() -> impl Strategy<Value = Affine> {
    prop_oneof![
        (-180.0..180.0f64).prop_map(|deg| Affine::rotate(deg.to_radians())),
        Just(Affine::scale_non_uniform(-1.0, 1.0)),
        Just(Affine::scale(0.0)),
        Just(Affine::new([1.0, 2.0, 2.0, 4.0, 0.0, 0.0])),
        Just(Affine::scale(1e300)),
        Just(Affine::new([f64::MAX, 0.0, 0.0, f64::MAX, f64::MAX, -f64::MAX])),
    ]
}

/// A document of a rectangle filled and stroked, a compound path of two rectangles, a rectangle with
/// two strokes (an appearance the walk expands), and two overlapping rectangles grouped with a
/// stroke of the group's own → the session and its objects' ids.
fn document() -> (Session, Vec<u64>) {
    let mut s = Session::new();
    let run = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap();
    run(&mut s, "file.new", json!({"width": 400, "height": 300, "artboards": 1}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let id = |v: Value| v["id"].as_u64().unwrap();
    let a = id(run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50})));
    let b = id(run(&mut s, "shape.rectangle", json!({"x": 120, "y": 20, "width": 40, "height": 40})));
    let c = id(run(&mut s, "shape.rectangle", json!({"x": 170, "y": 20, "width": 40, "height": 40})));
    let compound = id(run(&mut s, "object.compoundPath.make", json!({"ids": [b, c]})));
    let two = id(run(&mut s, "shape.rectangle", json!({"x": 20, "y": 120, "width": 80, "height": 50})));
    run(&mut s, "appearance.addStroke", json!({"ids": [two]}));
    let d = id(run(&mut s, "shape.rectangle", json!({"x": 200, "y": 120, "width": 60, "height": 60})));
    let e = id(run(&mut s, "shape.rectangle", json!({"x": 230, "y": 150, "width": 60, "height": 60})));
    let group = id(run(&mut s, "object.group", json!({"ids": [d, e]})));
    run(&mut s, "appearance.addStroke", json!({"ids": [group]}));
    run(&mut s, "select.none", json!({}));
    (s, vec![a, compound, two, d, e])
}

/// Every needle point of the active document's plan is finite, when it can be planned.
fn plan_is_finite(s: &mut Session) {
    if let Ok(plan) = s.embroidery_plan(&EmbroideryOptions::default()) {
        for k in 0..plan.units.len() {
            assert!(plan.unit_stitches(k).iter().all(|st| st.x.is_finite() && st.y.is_finite()), "unit {k}");
        }
    }
}

/// A path a hostile file holds out of range (control points at 1e15, 1e100 and the largest float,
/// past anything the commands make), with two strokes and both of a stroke's points set: planning
/// it (which splits the strokes and keeps each point on the copy nearest it), snapping a point to
/// it and reading it never flatten it to points without bound (1e15 would make some 7·10⁸ points),
/// nor panic; the walk refuses the shape.
#[test]
fn a_path_out_of_range_is_never_flattened() {
    use vectorcraft_doc::NodeKind;
    use vectorcraft_geom::{BezPath, PathData};
    for far in [1e15, 1e100, f64::MAX] {
        let (mut s, ids) = document();
        let two = NodeId(ids[2]);
        s.edit("Hostile Geometry", |d, _| {
            let n = d.node_mut(two).unwrap();
            let mut bp = BezPath::new();
            bp.move_to((20.0, 120.0));
            bp.curve_to((far, -far), (-far, far), (100.0, 170.0));
            bp.close_path();
            if let NodeKind::Path { path, .. } = &mut n.kind {
                *path = PathData::from_bezpath(&bp);
            }
            n.extras_mut().embroidery = Some(Box::new(Embroidery::from(json!({"stroke": {"points": {"entry": [20, 120], "exit": [100, 170]}}}))));
            Ok(())
        })
        .unwrap();
        plan_is_finite(&mut s);
        let snapped = no_panic(s.execute("embroidery.setEntryExit", &json!({"id": two.0, "part": "stroke", "entry": [30, 130], "snap": true})));
        assert_eq!(snapped.unwrap()["entry"], json!([30.0, 130.0]), "{far}: nothing to snap to, kept where given");
        no_panic(s.execute("embroidery.info", &json!({"ids": [two.0], "plan": true}))).unwrap();
    }
}

/// `r`, failing the test when it is a panic the engine caught (`EngineError::Internal`).
fn no_panic(r: Result<Value, vectorcraft_engine::EngineError>) -> Result<Value, vectorcraft_engine::EngineError> {
    if let Err(vectorcraft_engine::EngineError::Internal { cmd, msg }) = &r {
        panic!("{cmd} panicked: {msg}");
    }
    r
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(24))]

    /// Hostile settings never panic anything that plans, reads, edits, transforms or derives from
    /// them (a panic the engine catches fails the case), and the document still saves and reads
    /// back as it was.
    #[test]
    fn hostile_settings_never_panic(
        on_objects in prop::collection::vec(object_settings(), 5),
        in_document in document_settings(),
        xf in transform(),
    ) {
        let (mut s, ids) = document();
        s.edit("Hostile Settings", |d, _| {
            for (id, e) in ids.iter().zip(&on_objects) {
                if let Some(n) = d.node_mut(NodeId(*id)) {
                    n.extras_mut().embroidery = Some(Box::new(Embroidery::from(e.clone())));
                }
            }
            d.embroidery = Some(Box::new(EmbroiderySetup::from(in_document.clone())));
            Ok(())
        })
        .unwrap();
        plan_is_finite(&mut s);
        let ask = json!({"ids": ids});
        let _ = no_panic(s.execute("embroidery.info", &json!({"ids": ids, "plan": true})));
        let _ = no_panic(s.execute("embroidery.settings", &json!({})));
        let r = no_panic(s.execute("embroidery.settings", &json!({}))).unwrap();
        let _ = no_panic(s.execute("embroidery.setup", &json!({"settings": r["settings"]})));
        let _ = no_panic(s.execute("embroidery.copyFrom", &json!({"source": ids[0], "ids": ids})));
        let _ = no_panic(s.execute("embroidery.set", &json!({"ids": ids, "part": "fill", "angle": 30})));
        let _ = no_panic(s.execute("embroidery.applyFabric", &ask));
        // Transformed, by command and by a transform no command makes (the edit refuses one that
        // takes the art off the canvas, which is the right answer to it).
        let _ = no_panic(s.execute("object.rotate", &json!({"ids": ids, "angle": 33})));
        let _ = no_panic(s.execute("object.reflect", &json!({"ids": ids, "axis": "vertical"})));
        let _ = s.edit("Transform", |d, _| {
            for id in &ids {
                if let Some(n) = d.node_mut(NodeId(*id)) {
                    n.transform(xf, false);
                }
            }
            Ok(())
        });
        plan_is_finite(&mut s);
        // Objects made from others.
        let _ = no_panic(s.execute("object.compoundPath.release", &json!({"ids": [ids[1]]})));
        let _ = no_panic(s.execute("effect.expandAppearance", &json!({"ids": [ids[2]]})));
        let _ = no_panic(s.execute("object.path.outlineStroke", &json!({"ids": [ids[0]]})));
        let _ = no_panic(s.execute("select.set", &json!({"ids": [ids[3], ids[4]]})));
        let _ = no_panic(s.execute("object.pathfinder.divide", &json!({})));
        let _ = no_panic(s.execute("embroidery.reset", &ask));
        plan_is_finite(&mut s);
        // Converted, restored, stopped, and edited as stitches, marks and originals as they are.
        let _ = no_panic(s.execute("embroidery.restoreObject", &json!({"ids": ids, "dryRun": true})));
        let _ = no_panic(s.execute("embroidery.restoreObject", &ask));
        let _ = no_panic(s.execute("embroidery.setStopAfter", &ask));
        let _ = no_panic(s.execute("embroidery.stitches", &json!({"limit": 50})));
        for id in &ids {
            let _ = no_panic(s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": 0}], "dx": 1})));
            let _ = no_panic(s.execute("embroidery.setMachineFunction", &json!({"needlePoints": [{"id": id, "index": 1}], "function": "stop"})));
            let _ = no_panic(s.execute("embroidery.deleteNeedlePoints", &json!({"needlePoints": [{"id": id, "index": 0}]})));
        }
        let _ = no_panic(s.execute("embroidery.convertToManual", &ask));
        let _ = no_panic(s.execute("embroidery.restoreObject", &ask));
        plan_is_finite(&mut s);
        // Every embroidery command's junk on it.
        for (command, params) in embroidery_common::junk::structured() {
            let _ = no_panic(s.execute(command, &params));
        }
        let doc = Arc::clone(&s.doc().unwrap().doc);
        check_native_roundtrip(&doc).map_err(TestCaseError::fail)?;
    }
}
