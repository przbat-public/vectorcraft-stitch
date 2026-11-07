//! The document's rows (`embroidery.setup {fillAngle}`, the project owner's choices of 2026-11-01:
//! rows "by shape" across the shape's length, a document setting, fixed rows the default): set,
//! reset and refused; the rule across each fill's shape; a number of degrees turning with each
//! object; the object's and the export's angles beating the document's; and what `embroidery.info`
//! and the embroidery tool's arrow say.

use serde_json::{Value, json};

use super::digitize::digitize;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// The difference between two directions of rows (degrees), whichever way round each is said.
fn apart(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(180.0);
    d.min(180.0 - d)
}

/// A rectangle filled red, `w` × `h` millimetres at (`x`, `y`) millimetres, unstroked → its id.
fn bar(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    let r = run(s, "shape.rectangle", json!({"x": x * PT_PER_MM, "y": y * PT_PER_MM, "width": w * PT_PER_MM, "height": h * PT_PER_MM}));
    run(s, "select.none", json!({}));
    r["id"].as_u64().unwrap()
}

/// A disc filled red, `d` millimetres across at (`x`, `y`) millimetres → its id.
fn disc(s: &mut Session, x: f64, y: f64, d: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#2233cc"}));
    let r = run(s, "shape.ellipse", json!({"x": x * PT_PER_MM, "y": y * PT_PER_MM, "width": d * PT_PER_MM, "height": d * PT_PER_MM}));
    run(s, "select.none", json!({}));
    r["id"].as_u64().unwrap()
}

/// The planner's angle of each fill unit of `s`'s document, by its object, planned with `opts`.
fn rows(s: &Session, opts: Value) -> Vec<(u64, f64)> {
    let plan = digitize(&s.doc().unwrap().doc, &Options::from_json(&opts).unwrap(), None).unwrap();
    plan.units
        .iter()
        .filter(|u| u.sewn() == Some(vectorcraft_stitch::plan::Style::Fill))
        .map(|u| (u.origin.node.0, u.params.unwrap().angle))
        .collect()
}

fn rows_of(s: &Session, id: u64) -> f64 {
    rows(s, json!({})).into_iter().find(|(n, _)| *n == id).map(|(_, a)| a).unwrap()
}

/// `fillAngle` takes a number of degrees, the rule `"acrossShape"` or null, each one undo step,
/// reported as stored and as the plan uses it; anything else is refused, naming what it takes, and
/// changes nothing.
#[test]
fn the_documents_rows_are_set_reset_and_refused() {
    let mut s = blank();
    let undo = |s: &Session| s.doc().unwrap().history.undo.len();
    let before = undo(&s);
    let r = run(&mut s, "embroidery.setup", json!({"fillAngle": 30}));
    assert_eq!((&r["settings"]["fillAngle"], &r["resolved"]["fillAngle"]), (&json!(30.0), &json!({"value": 30.0, "source": "document"})));
    let r = run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    assert_eq!(r["resolved"]["fillAngle"], json!({"value": "acrossShape", "source": "document"}));
    assert_eq!(undo(&s), before + 2);
    for junk in [json!("alongShape"), json!(400), json!("30"), json!(true), json!([1, 2])] {
        let e = s.execute("embroidery.setup", &json!({"fillAngle": junk})).unwrap_err().to_string();
        assert!(e.contains("`fillAngle`") && e.contains("acrossShape"), "{junk}: {e}");
    }
    assert_eq!(undo(&s), before + 2, "a refusal changes nothing");
    let r = run(&mut s, "embroidery.setup", json!({"fillAngle": null}));
    assert_eq!((&r["settings"]["fillAngle"], &r["resolved"]["fillAngle"]), (&Value::Null, &json!({"value": 135.0, "source": "default"})));
    let e = s.execute("embroidery.setup", &json!({"angle": 30})).unwrap_err().to_string();
    assert!(e.contains("fillAngle"), "{e}");
}

/// Under the rule, each fill's rows run square to its length: a tall bar's across it, a wide one's
/// up and down; a disc, about as long as wide, keeps the fixed rows (45° to the planner, 135° on
/// the page). Unset, every fill is planned with the fixed rows, as before.
#[test]
fn the_rows_run_across_each_fills_length() {
    let mut s = blank();
    let tall = bar(&mut s, 20.0, 20.0, 10.0, 40.0);
    let wide = bar(&mut s, 40.0, 20.0, 40.0, 10.0);
    let round = disc(&mut s, 40.0, 40.0, 20.0);
    let fixed = vectorcraft_stitch::plan::FILL_ANGLE;
    let unset = rows(&s, json!({}));
    assert!(unset.iter().all(|(_, a)| *a == fixed), "{unset:?}");
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    assert!(apart(rows_of(&s, tall), 0.0) < 0.05, "the tall bar's rows at {}", rows_of(&s, tall));
    assert!(apart(rows_of(&s, wide), 90.0) < 0.05, "the wide bar's rows at {}", rows_of(&s, wide));
    assert_eq!(rows_of(&s, round), fixed, "the disc's rows");
}

/// A set angle beats the rule and the document's number: the object's own, then the export's.
#[test]
fn an_objects_and_an_exports_angle_beat_the_documents() {
    let mut s = blank();
    let tall = bar(&mut s, 20.0, 20.0, 10.0, 40.0);
    for rows_set in [json!("acrossShape"), json!(30)] {
        run(&mut s, "embroidery.setup", json!({"fillAngle": rows_set}));
        let by_export = rows(&s, json!({"angle": 60})).into_iter().find(|(n, _)| *n == tall).unwrap().1;
        assert_eq!(by_export, options::planner_angle(60.0), "{rows_set}: the export's");
        run(&mut s, "embroidery.set", json!({"ids": [tall], "part": "fill", "angle": 75}));
        assert_eq!(rows_of(&s, tall), options::planner_angle(75.0), "{rows_set}: the object's");
        run(&mut s, "embroidery.reset", json!({"ids": [tall], "part": "fill", "keys": ["angle"]}));
    }
}

/// The document's number is the rows of every fill that sets none, turned with its object as the
/// built-in rows are: an upright bar at it, one turned a quarter turn at it plus the turn.
#[test]
fn the_documents_number_turns_with_each_object() {
    let mut s = blank();
    let upright = bar(&mut s, 20.0, 20.0, 30.0, 10.0);
    let turned = bar(&mut s, 60.0, 20.0, 30.0, 10.0);
    run(&mut s, "object.rotate", json!({"ids": [turned], "angle": 90}));
    let turn = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(turned)).unwrap().bbox_angle;
    run(&mut s, "embroidery.setup", json!({"fillAngle": 30}));
    assert_eq!(rows_of(&s, upright), options::planner_angle(30.0));
    assert!(apart(rows_of(&s, turned), options::planner_angle(30.0 + turn)) < 1e-9, "turned {turn}: {}", rows_of(&s, turned));
}

/// `embroidery.info` says where the rows come from: the document's number as a number, the rule as
/// `"auto"` with what it chose (the rows across, how much longer than wide); the embroidery tool's
/// arrow points at the rule's choice, hollow (unset on the object).
#[test]
fn info_and_the_arrow_say_what_the_rows_are() {
    let mut s = blank();
    let tall = bar(&mut s, 20.0, 20.0, 10.0, 40.0);
    run(&mut s, "embroidery.setup", json!({"fillAngle": 30}));
    let r = run(&mut s, "embroidery.info", json!({"ids": [tall]}));
    let angle = &r["fill"]["settings"]["angle"];
    assert_eq!((&angle["value"], &angle["source"], &angle["section"]), (&json!(30.0), &json!("document"), &json!("fillAngle")));
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    let r = run(&mut s, "embroidery.info", json!({"ids": [tall]}));
    let angle = &r["fill"]["settings"]["angle"];
    assert_eq!((&angle["value"], &angle["source"]), (&json!("auto"), &json!("document")));
    let rule = &angle["rule"];
    assert_eq!((&rule["id"], &rule["values"]["across"]), (&json!("rowsAcrossShape"), &json!(true)), "{rule}");
    let chose = rule["chose"].as_f64().unwrap();
    assert!(apart(chose, 0.0) < 0.05, "chose {chose}");
    assert!((rule["values"]["elongation"].as_f64().unwrap() - 4.0).abs() < 0.01, "{rule}");
    let doc = &s.doc().unwrap().doc;
    let n = doc.node(vectorcraft_doc::NodeId(tall)).unwrap();
    let (degrees, set) = super::info::fill_rows(doc, n, &Options::default(), &super::resolve::DocLayer::of(doc)).unwrap();
    assert!(apart(degrees, chose) < 1e-9 && !set, "the arrow at {degrees}, set {set}");
}
