//! The document's embroidery settings set by command: a fabric chosen (its values copied, the
//! colour kept), changed value by value and restored, the thread, the joining and the machine;
//! what is refused; the reply passed back changing nothing; the presets listed with their sources;
//! the objects' own fabric values counted and applied away.

use serde_json::{Value, json};
use vectorcraft_stitch::fabric::{DEFAULT_FABRIC, FABRICS, ONE_FAMILY};

use super::test_support::{blank, run};
use super::*;

fn steps(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

/// A filled rectangle at `x` → its id.
fn rect(s: &mut Session, x: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    id
}

/// Choosing a preset copies its values into the document (in force and as the preset's), keeps the
/// colour, and the plan sews with them; a value changed is marked against the preset's and restored
/// with null; the same preset again restores them all; null removes the fabric.
#[test]
fn a_fabric_is_copied_changed_and_restored() {
    let mut s = blank();
    rect(&mut s, 20.0);
    let before = steps(&s);
    run(&mut s, "embroidery.setup", json!({"fabric": {"color": "#102030"}}));
    let r = run(&mut s, "embroidery.setup", json!({"fabric": "stretchKnit"}));
    assert_eq!(steps(&s), before + 2);
    let knit = vectorcraft_stitch::fabric::fabric("stretchKnit").unwrap();
    let stored = &r["settings"]["fabric"];
    assert_eq!((&stored["preset"], &stored["color"]), (&json!("stretchKnit"), &json!("#102030")), "the colour kept");
    let pull = knit.tatami.pull.map(|p| p.value).unwrap();
    assert_eq!(stored["tatami"]["pull"], json!(pull));
    let resolved = &r["resolved"]["fabric"];
    assert_eq!(resolved["preset"], json!({"id": "stretchKnit", "known": true}));
    assert_eq!(resolved["tatami"]["pull"], json!({"value": pull, "source": "document", "preset": pull, "changed": false}));
    let fill = |s: &mut Session| {
        run(s, "embroidery.info", json!({"ids": [s.doc().unwrap().doc.layers[0].children().unwrap()[0].id.0]}))["fill"]["settings"].clone()
    };
    assert_eq!(
        fill(&mut s)["pull"],
        json!({"value": [pull, pull], "source": "document", "section": "fabric", "setCount": 0, "mixed": false, "inherited": {"value": [pull, pull], "source": "document", "section": "fabric"}, "rule": null, "applies": true})
    );
    // A value changed, marked; restored with null.
    let r = run(&mut s, "embroidery.setup", json!({"fabric": {"tatami": {"length": 3.5}}}));
    assert_eq!(
        (&r["resolved"]["fabric"]["tatami"]["length"]["value"], &r["resolved"]["fabric"]["tatami"]["length"]["changed"]),
        (&json!(3.5), &json!(true))
    );
    let r = run(&mut s, "embroidery.setup", json!({"fabric": {"tatami": {"length": null}}}));
    assert_eq!(r["resolved"]["fabric"]["tatami"]["length"]["changed"], json!(false));
    run(&mut s, "embroidery.setup", json!({"fabric": {"satin": {"pull": 1.5}, "thickness": 2}}));
    let r = run(&mut s, "embroidery.setup", json!({"fabric": "stretchKnit"}));
    assert!(r["resolved"]["fabric"]["satin"]["pull"]["changed"] == json!(false) && r["settings"]["fabric"]["color"] == json!("#102030"), "{r}");
    // The reply passed back changes nothing.
    let at = steps(&s);
    assert_eq!(run(&mut s, "embroidery.setup", json!({"settings": r["settings"]})), r);
    assert_eq!(steps(&s), at, "no change, no step");
    let r = run(&mut s, "embroidery.setup", json!({"fabric": null}));
    assert_eq!(r["settings"]["fabric"], Value::Null);
}

/// The thread's weight, the joining of one colour's objects and the machine are set, merged and
/// unset; a trim length below the jump length, values out of range and unknown keys are refused.
#[test]
fn the_thread_joining_and_machine_are_set() {
    let mut s = blank();
    let r = run(&mut s, "embroidery.setup", json!({"threadWeight": 60, "joinSameColor": false, "machine": {"jumpLength": 4, "speed": 800}}));
    assert_eq!((&r["settings"]["threadWeight"], &r["settings"]["joinSameColor"]), (&json!(60.0), &json!(false)));
    assert_eq!(r["settings"]["machine"], json!({"jumpLength": 4.0, "trimLength": null, "minStitch": null, "splitLength": null, "speed": 800.0}));
    assert_eq!(r["resolved"]["machine"]["trimLength"], json!({"value": 10.0, "source": "default"}));
    // The fabric's spacing, given for 40 wt, is reported for 60 wt.
    let spacing = r["resolved"]["fabric"]["tatami"]["spacing"]["value"].as_f64().unwrap();
    assert!((spacing - 0.38 * vectorcraft_stitch::plan::thread_scale(60.0)).abs() < 1e-12, "{spacing}");
    let r = run(&mut s, "embroidery.setup", json!({"machine": {"speed": null, "trimLength": 8}}));
    assert_eq!(r["settings"]["machine"], json!({"jumpLength": 4.0, "trimLength": 8.0, "minStitch": null, "splitLength": null, "speed": null}));
    for (bad, says) in [
        (json!({"machine": {"trimLength": 2}}), "at least `jumpLength`"),
        (json!({"threadWeight": 5}), "10 to 150"),
        (json!({"machine": {"speed": 5000}}), "60 to 1500"),
        (json!({"machine": {"hoop": 1}}), "the machine's values"),
        (json!({"joinSameColor": "yes"}), "true, false"),
        (json!({"fabric": "velvet"}), "one of cotton"),
        (json!({"fabric": {"tatami": {"spacingPercent": 90}}}), "tatami takes"),
        (json!({"fabric": {"satin": {"pull": 3}}}), "0 to 2"),
        (json!({"fabric": {"color": "red"}}), "#rrggbb"),
        (json!({"fabric": {"grain": 45}}), "the fabric's values"),
        (json!({"fabric": {"tatami": {"underlay": "triple"}}}), "one of"),
        // A 40 wt spacing that a thread's weight could scale out of the planner's range.
        (json!({"fabric": {"tatami": {"spacing": 4}}}), "0.1 to 2.5"),
    ] {
        let e = super::setup::setup(&mut s, &bad).unwrap_err().to_string();
        assert!(e.contains(says), "{bad}: {e}");
    }
    let r = run(&mut s, "embroidery.setup", json!({"threadWeight": null, "joinSameColor": null, "machine": null}));
    assert_eq!((&r["settings"]["threadWeight"], &r["settings"]["machine"]), (&Value::Null, &Value::Null));
}

/// `embroidery.fabrics` lists the eight presets, the default first, each value with what it rests
/// on, the estimates among them, and the caveat that one family of software gives them.
#[test]
fn the_fabrics_are_listed_with_their_sources() {
    let mut s = Session::new();
    let r = run(&mut s, "embroidery.fabrics", json!({}));
    assert_eq!((&r["default"], &r["caveat"]), (&json!(DEFAULT_FABRIC), &json!(ONE_FAMILY)));
    let fabrics = r["fabrics"].as_array().unwrap();
    assert_eq!(fabrics.len(), FABRICS.len());
    assert_eq!(fabrics[0]["id"], json!(DEFAULT_FABRIC));
    for f in fabrics {
        let sources = f["sources"].as_array().unwrap();
        // Every value given has at least one source.
        for (kind, keys) in [
            ("tatami", vec!["pull", "spacing", "spacingPercent", "length", "underlay"]),
            ("satin", vec!["pull", "spacing", "spacingPercent", "length", "underlay"]),
        ] {
            for k in keys {
                if !f["values"][kind][k].is_null() {
                    let key = format!("{kind}.{k}");
                    assert!(sources.iter().any(|s| s["key"] == json!(key)), "{}: {key}", f["id"]);
                }
            }
        }
        for e in f["estimates"].as_array().unwrap() {
            assert!(sources.iter().any(|s| s["key"] == *e && s["kind"] == json!("estimate")), "{}: {e}", f["id"]);
        }
    }
    assert!(s.execute("embroidery.fabrics", &json!({"id": "stretchKnit"})).is_err());
}

/// Objects setting a fabric's value themselves are counted, by key; `embroidery.applyFabric`
/// unsets those values (every object with nothing selected, or those given; other values stay), so that they
/// follow the document's fabric; a key a fabric doesn't set is refused.
#[test]
fn objects_own_fabric_values_are_counted_and_applied_away() {
    let mut s = blank();
    let (a, b) = (rect(&mut s, 20.0), rect(&mut s, 100.0));
    run(&mut s, "embroidery.set", json!({"ids": [a, b], "part": "fill", "pull": 0.3, "angle": 10}));
    run(&mut s, "embroidery.set", json!({"ids": [b], "part": "fill", "length": 3}));
    let r = run(&mut s, "embroidery.settings", json!({}));
    assert_eq!(r["overrides"], json!({"pull": 2, "spacing": 0, "length": 1, "underlay": 0, "objectCount": 2}));
    let r = run(&mut s, "embroidery.applyFabric", json!({"ids": [a], "keys": ["pull"]}));
    assert_eq!((&r["changedCount"], &r["keys"]), (&json!(1), &json!(["pull"])));
    let r = run(&mut s, "embroidery.applyFabric", json!({}));
    assert_eq!(r["changedCount"], json!(1));
    assert_eq!(
        run(&mut s, "embroidery.settings", json!({}))["overrides"],
        json!({"pull": 0, "spacing": 0, "length": 0, "underlay": 0, "objectCount": 0})
    );
    let info = run(&mut s, "embroidery.info", json!({"ids": [a, b]}));
    assert_eq!(info["fill"]["settings"]["angle"]["value"], json!(10.0), "other values stay");
    let e = s.execute("embroidery.applyFabric", &json!({"keys": ["angle"]})).unwrap_err().to_string();
    assert!(e.contains("embroidery.reset"), "{e}");
}

/// A stroke's stitch length is no fabric's value (running lines and satins take none from it): it
/// is neither counted as overriding the fabric nor unset by applying it; a stroke's pull is.
#[test]
fn a_strokes_length_is_no_fabric_value() {
    let mut s = blank();
    let a = rect(&mut s, 20.0);
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "stroke", "stitch": "running", "length": 1.5, "pull": 0.3}));
    assert_eq!(
        run(&mut s, "embroidery.settings", json!({}))["overrides"],
        json!({"pull": 1, "spacing": 0, "length": 0, "underlay": 0, "objectCount": 1})
    );
    run(&mut s, "embroidery.applyFabric", json!({}));
    let info = run(&mut s, "embroidery.info", json!({"ids": [a]}));
    let stroke = &info["stroke"]["settings"];
    // The pull unset: a running line takes none of the fabric's values, so the default.
    assert_eq!((&stroke["length"]["value"], &stroke["pull"]["source"]), (&json!(1.5), &json!("default")));
}

/// Applying the fabric to the whole document copies only the object it changes: every other object
/// stays shared with the step before (so the undo history doesn't hold the document twice), and
/// when nothing changes no step is taken and nothing is copied.
#[test]
fn applying_the_fabric_copies_only_the_objects_it_changes() {
    let mut s = blank();
    let ids: Vec<u64> = (0..4).map(|i| rect(&mut s, 20.0 + 80.0 * f64::from(i))).collect();
    run(&mut s, "embroidery.set", json!({"ids": [ids[1]], "part": "fill", "pull": 0.3}));
    let nodes = |s: &Session| -> Vec<std::sync::Arc<vectorcraft_doc::Node>> {
        let d = &s.doc().unwrap().doc;
        let layer = d.layer_of(NodeId(ids[0])).unwrap();
        ids.iter().map(|id| d.children(Some(layer)).unwrap().iter().find(|n| n.id == NodeId(*id)).unwrap().clone()).collect()
    };
    let before = nodes(&s);
    assert_eq!(run(&mut s, "embroidery.applyFabric", json!({}))["changedCount"], json!(1));
    let after = nodes(&s);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        assert_eq!(std::sync::Arc::ptr_eq(a, b), i != 1, "object {i}");
    }
    let steps_before = steps(&s);
    assert_eq!(run(&mut s, "embroidery.applyFabric", json!({}))["changedCount"], json!(0));
    assert_eq!(steps(&s), steps_before);
    assert!(nodes(&s).iter().zip(&after).all(|(a, b)| std::sync::Arc::ptr_eq(a, b)));
}

/// Document Setup's bar counts the objects that set the fabric's values themselves, once each
/// however many they set, symbols' art included (its instances sew it); applying the fabric with
/// nothing selected reaches the symbols' art too.
#[test]
fn overrides_count_objects_and_symbols_art() {
    let mut s = blank();
    let a = rect(&mut s, 20.0);
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "fill", "pull": 0.3, "spacing": 0.5}));
    let overrides = |s: &mut Session| run(s, "embroidery.settings", json!({}))["overrides"].clone();
    assert_eq!(overrides(&mut s), json!({"pull": 1, "spacing": 1, "length": 0, "underlay": 0, "objectCount": 1}));
    run(&mut s, "select.set", json!({"ids": [a]}));
    run(&mut s, "symbol.new", json!({"name": "Badge"}));
    run(&mut s, "select.none", json!({}));
    assert_eq!(overrides(&mut s)["objectCount"], json!(1), "the symbol's art");
    let r = run(&mut s, "embroidery.applyFabric", json!({}));
    assert_eq!(r["changedCount"], json!(1));
    assert_eq!(overrides(&mut s)["objectCount"], json!(0));
}
