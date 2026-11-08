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
    let (degrees, set) = super::info::fill_rows(doc, n, &Options::default(), &super::resolve::DocLayer::of(doc), None).unwrap();
    assert!(apart(degrees, chose) < 1e-9 && !set, "the arrow at {degrees}, set {set}");
}

/// Two upright bars 10 × 40 mm in one path, 40 mm apart (as an SVG keeps a word's letters): under
/// the rule each bar's rows run across it (0° to the planner), as each alone's do, though the two
/// together are wider than tall; `embroidery.info` says so, the path's two pieces measured apart.
#[test]
fn each_part_of_one_path_has_rows_across_it() {
    let mut s = blank();
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    let bar = |x: f64| {
        let (a, b, t, u) = (x * PT_PER_MM, (x + 10.0) * PT_PER_MM, 20.0 * PT_PER_MM, 60.0 * PT_PER_MM);
        format!("M {a} {t} L {b} {t} L {b} {u} L {a} {u} Z ")
    };
    let id = run(&mut s, "path.create", json!({"d": bar(10.0) + &bar(60.0)}))["id"].as_u64().unwrap();
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    assert!(apart(rows_of(&s, id), 0.0) < 0.05, "rows at {}", rows_of(&s, id));
    let r = run(&mut s, "embroidery.info", json!({"ids": [id]}));
    let rule = &r["fill"]["settings"]["angle"]["rule"];
    assert_eq!((&rule["values"]["pieces"], &rule["values"]["across"]), (&json!(2), &json!(true)), "{rule}");
    assert!(apart(rule["chose"].as_f64().unwrap(), 0.0) < 0.05, "{rule}");
}

/// The direction most of the needle-down stitches longer than a millimetre with both ends inside
/// `(x0, y0, x1, y1)` (millimetres) run in, planned without underlay so that a fill's stitches are
/// its rows: degrees as the planner counts them, from 0 up to a half turn (the median).
fn sewn_rows(s: &Session, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let plan = digitize(&s.doc().unwrap().doc, &Options::from_json(&json!({"underlay": false})).unwrap(), None).unwrap();
    let inside = |p: &vectorcraft_stitch::Stitch| p.x > x0 && p.x < x1 && p.y > y0 && p.y < y1;
    let mut angles: Vec<f64> = plan
        .blocks
        .iter()
        .flat_map(|b| b.stitches.windows(2))
        .filter_map(|w| {
            let [a, z] = w else { return None };
            let sewn = a.command == vectorcraft_stitch::Command::Stitch && z.command == vectorcraft_stitch::Command::Stitch;
            (sewn && inside(a) && inside(z) && a.distance(*z) > 1.0).then(|| (z.y - a.y).atan2(z.x - a.x).to_degrees().rem_euclid(180.0))
        })
        .collect();
    assert!(!angles.is_empty(), "no stitch inside");
    angles.sort_by(f64::total_cmp);
    angles[angles.len() / 2]
}

/// Two pieces apart in one path, one with a narrow tab sewn as a satin column: the rest of each is
/// sewn across its own piece's length, the tall bar's rows across it (0° to the planner) and the
/// wide bar's up and down (90°).
#[test]
fn each_piece_with_narrow_parts_has_rows_across_it() {
    let mut s = blank();
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    let at = |pts: &[(f64, f64)]| {
        let mut d = String::new();
        for (k, (x, y)) in pts.iter().enumerate() {
            d += &format!("{} {} {} ", if k == 0 { "M" } else { "L" }, x * PT_PER_MM, y * PT_PER_MM);
        }
        d + "Z "
    };
    // A tall bar 10 × 40 mm with a tab 3 mm wide reaching 15 mm out of its middle, and a wide bar
    // 40 × 10 mm.
    let tall = at(&[(10.0, 20.0), (20.0, 20.0), (20.0, 38.5), (35.0, 38.5), (35.0, 41.5), (20.0, 41.5), (20.0, 60.0), (10.0, 60.0)]);
    let wide = at(&[(50.0, 20.0), (90.0, 20.0), (90.0, 30.0), (50.0, 30.0)]);
    let id = run(&mut s, "path.create", json!({"d": tall + &wide}))["id"].as_u64().unwrap();
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let unit = plan.units.iter().find(|u| u.origin.node.0 == id).unwrap();
    assert!(unit.columns.is_some(), "the tab is sewn as a satin column");
    assert!(apart(sewn_rows(&s, (11.0, 21.0, 19.0, 37.0)), 0.0) < 1.0, "the tall bar's rows");
    assert!(apart(sewn_rows(&s, (51.0, 21.0, 89.0, 29.0)), 90.0) < 1.0, "the wide bar's rows");
}

/// The rows `embroidery.info` reports for part `part` of object `id` under the rule (planner
/// degrees), whether they run across it, and how many pieces it measured.
fn info_rows(s: &mut Session, id: u64, part: &str) -> (f64, bool, u64) {
    let r = run(s, "embroidery.info", json!({"ids": [id]}));
    let rule = &r[part]["settings"]["angle"]["rule"];
    (options::planner_angle(rule["chose"].as_f64().unwrap()), rule["values"]["across"] == json!(true), rule["values"]["pieces"].as_u64().unwrap())
}

/// The rows the embroidery tool's arrow shows for object `id`'s fill (planner degrees): from the
/// plan, and from the document alone when there is none.
fn arrow_rows(s: &Session, id: u64) -> (f64, f64) {
    let doc = &s.doc().unwrap().doc;
    let n = doc.node(vectorcraft_doc::NodeId(id)).unwrap();
    let layer = super::resolve::DocLayer::of(doc);
    let plan = digitize(doc, &Options::default(), None).unwrap();
    let from =
        |plan: Option<&super::plan::DesignPlan>| options::planner_angle(super::info::fill_rows(doc, n, &Options::default(), &layer, plan).unwrap().0);
    (from(Some(&plan)), from(None))
}

/// `embroidery.info` and the embroidery tool's arrow measure the rows across the shape on the area
/// the plan sews, not on the object's own path: a stroke sewn as tatami is its painted band (80 ×
/// 10 mm and 50 × 5 mm: rows up and down, 90° to the planner, though its path has no area), dashed
/// it is the dashes it paints; a tall bar a transform effect turns a quarter turn is sewn wide; a
/// wide bar a clip group cuts to a sliver 4 mm wide is sewn tall (rows across it, 0°).
#[test]
fn info_and_the_arrow_measure_the_area_the_plan_sews() {
    for (length, weight, dash) in [(80.0, 10.0, None), (50.0, 5.0, None), (80.0, 10.0, Some(20.0))] {
        let mut s = blank();
        run(&mut s, "paint.setFill", json!({"none": true}));
        run(&mut s, "paint.setStroke", json!({"color": "#cc2222"}));
        let d = format!("M {} {} L {} {}", 10.0 * PT_PER_MM, 40.0 * PT_PER_MM, (10.0 + length) * PT_PER_MM, 40.0 * PT_PER_MM);
        let id = run(&mut s, "path.create", json!({"d": d}))["id"].as_u64().unwrap();
        run(&mut s, "stroke.set", json!({"ids": [id], "weight": weight * PT_PER_MM}));
        if let Some(dash) = dash {
            run(&mut s, "stroke.set", json!({"ids": [id], "dash": [dash * PT_PER_MM, dash * PT_PER_MM]}));
        }
        run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "tatami"}));
        run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
        let planned = rows_of(&s, id);
        assert!(apart(planned, 90.0) < 0.05, "{length} × {weight}, dash {dash:?}: planned at {planned}");
        let (chose, across, pieces) = info_rows(&mut s, id, "stroke");
        assert!(
            apart(chose, planned) < 0.05 && across && pieces >= 1,
            "{length} × {weight}, dash {dash:?}: info says {chose}, the plan sews {planned}"
        );
    }
    // A tall bar turned a quarter turn by an effect: the walk sews the turned piece.
    let mut s = blank();
    let tall = bar(&mut s, 20.0, 20.0, 10.0, 40.0);
    run(&mut s, "effect.apply", json!({"effect": "distort.transform", "ids": [tall], "item": null, "params": {"rotate": 90}}));
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    let planned = rows_of(&s, tall);
    assert!(apart(planned, 90.0) < 0.05, "the turned bar planned at {planned}");
    let (chose, across, _) = info_rows(&mut s, tall, "fill");
    assert!(apart(chose, planned) < 0.05 && across, "info says {chose}, the plan sews {planned}");
    let (arrow, unplanned) = arrow_rows(&s, tall);
    assert!(apart(arrow, planned) < 0.05 && apart(unplanned, planned) < 0.05, "the arrow at {arrow} and {unplanned}");
    // A wide bar a clip group cuts to a sliver 4 mm wide and 10 mm tall.
    let mut s = blank();
    let wide = bar(&mut s, 20.0, 20.0, 40.0, 10.0);
    let clip = bar(&mut s, 20.0, 10.0, 4.0, 30.0);
    run(&mut s, "select.set", json!({"ids": [wide, clip]}));
    run(&mut s, "object.clippingMask.make", json!({}));
    run(&mut s, "select.none", json!({}));
    run(&mut s, "embroidery.setup", json!({"fillAngle": "acrossShape"}));
    let planned = rows_of(&s, wide);
    assert!(apart(planned, 0.0) < 0.05, "the clipped bar planned at {planned}");
    let (chose, across, _) = info_rows(&mut s, wide, "fill");
    assert!(apart(chose, planned) < 0.05 && across, "info says {chose}, the plan sews {planned}");
    let (arrow, unplanned) = arrow_rows(&s, wide);
    assert!(apart(arrow, planned) < 0.05 && apart(unplanned, planned) < 0.05, "the arrow at {arrow} and {unplanned}");
}
