//! The resolution order as a matrix (the project owner's order, 2026-10-20: the object, the
//! export's option, the document, the default): for each setting, at each place it can sit, every
//! combination of the layers that give it, the unit planned with the first of them, and
//! `embroidery.info` saying which. And the same value given at any level writes the same bytes.

use serde_json::{Value, json};
use vectorcraft_stitch::plan::{Params, Style};

use super::digitize::digitize;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// A rectangle filled red with a blue stroke 3 mm wide (a satin column) → the session and its id.
fn sheet() -> (Session, u64) {
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 60}))["id"].as_u64().unwrap();
    run(&mut s, "stroke.set", json!({"weight": 3.0 * PT_PER_MM, "ids": [id]}));
    run(&mut s, "select.none", json!({}));
    (s, id)
}

/// One setting at one place: its name, the part it is set on (and so the style its unit is sewn
/// in), the value each layer gives (`object`: `None` for a design setting no object takes; `doc` as
/// `embroidery.setup` takes it, `None`: the document can't set it), and how a unit's parameters
/// show it, in the commands' units.
struct Row {
    key: &'static str,
    part: &'static str,
    object: Option<Value>,
    export: Value,
    doc: Option<Value>,
    read: fn(&Params) -> Value,
}

fn rows() -> Vec<Row> {
    let pull: fn(&Params) -> Value = |p| json!(p.pull[0]);
    let underlay: fn(&Params) -> Value = |p| json!(p.underlay.id());
    vec![
        Row {
            key: "pull",
            part: "fill",
            object: Some(json!(0.3)),
            export: json!(0.25),
            doc: Some(json!({"fabric": {"tatami": {"pull": 0.35}}})),
            read: pull,
        },
        Row {
            key: "pull",
            part: "stroke",
            object: Some(json!(0.3)),
            export: json!(0.25),
            doc: Some(json!({"fabric": {"satin": {"pull": 0.4}}})),
            read: pull,
        },
        Row {
            key: "spacing",
            part: "fill",
            object: Some(json!(0.5)),
            export: json!(0.45),
            doc: Some(json!({"fabric": {"tatami": {"spacing": 0.33}}})),
            read: |p| json!(p.density),
        },
        Row {
            key: "length",
            part: "fill",
            object: Some(json!(3.0)),
            export: json!(2.5),
            doc: Some(json!({"fabric": {"tatami": {"length": 3.5}}})),
            read: |p| json!(p.stitch_length),
        },
        Row {
            key: "underlay",
            part: "fill",
            object: Some(json!("edge")),
            export: json!("center"),
            doc: Some(json!({"fabric": {"tatami": {"underlay": "none"}}})),
            read: underlay,
        },
        Row {
            key: "underlay",
            part: "stroke",
            object: Some(json!("edge")),
            export: json!("center"),
            doc: Some(json!({"fabric": {"satin": {"underlay": "none"}}})),
            read: underlay,
        },
        Row {
            key: "splitLength",
            part: "stroke",
            object: Some(json!(8.0)),
            export: json!(7.0),
            doc: Some(json!({"machine": {"splitLength": 6}})),
            read: |p| json!(p.max_satin),
        },
        Row {
            key: "angle",
            part: "fill",
            object: Some(json!(30.0)),
            export: json!(60.0),
            doc: None,
            read: |p| json!(options::planner_angle(p.angle)),
        },
        Row { key: "pullPercent", part: "fill", object: Some(json!(5.0)), export: json!(10.0), doc: None, read: |p| json!(p.pull_percent[0]) },
        Row { key: "push", part: "stroke", object: Some(json!(0.5)), export: json!(0.3), doc: None, read: |p| json!(p.push[0]) },
        Row { key: "narrowAsSatin", part: "fill", object: Some(json!(4.0)), export: json!(5.0), doc: None, read: |p| json!(p.narrow) },
        Row {
            key: "stitch",
            part: "stroke",
            object: Some(json!("running")),
            export: json!("tatami"),
            doc: None,
            read: |p| json!(options::stitch_id(p.style)),
        },
        // The design's: the thread and the machine, given to an export or kept by the document.
        Row {
            key: "threadWeight",
            part: "stroke",
            object: None,
            export: json!(60.0),
            doc: Some(json!({"threadWeight": 30})),
            read: |p| json!(p.thread_weight),
        },
        Row {
            key: "jumpLength",
            part: "fill",
            object: None,
            export: json!(3.0),
            doc: Some(json!({"machine": {"jumpLength": 4}})),
            read: |p| json!(p.jump),
        },
        Row {
            key: "trimLength",
            part: "fill",
            object: None,
            export: json!(12.0),
            doc: Some(json!({"machine": {"trimLength": 15}})),
            read: |p| json!(p.trim),
        },
        Row {
            key: "minStitch",
            part: "stroke",
            object: None,
            export: json!(0.3),
            doc: Some(json!({"machine": {"minStitch": 0.5}})),
            read: |p| json!(p.min_stitch),
        },
    ]
}

/// The style a part's unit is sewn in when nothing forces another.
fn style_of(part: &str) -> Style {
    if part == "fill" { Style::Fill } else { Style::Satin }
}

/// For every setting at every place, every combination of the layers giving it: the unit is
/// planned with the first of object, export, document that gives it, else the default, and
/// `embroidery.info` says which layer that is.
#[test]
fn the_first_layer_that_gives_a_value_wins() {
    let mut cases = 0;
    for row in rows() {
        let style = style_of(row.part);
        let default = {
            let (s, _) = sheet();
            let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
            (row.read)(&plan.units.iter().find(|u| u.sewn() == Some(style)).unwrap().params.unwrap())
        };
        let doc_layers: &[bool] = if row.doc.is_some() { &[false, true] } else { &[false] };
        let object_layers: &[bool] = if row.object.is_some() { &[false, true] } else { &[false] };
        for &object in object_layers {
            for export in [false, true] {
                for &doc in doc_layers {
                    let (mut s, id) = sheet();
                    if let (true, Some(value)) = (object, &row.object) {
                        run(&mut s, "embroidery.set", json!({"ids": [id], "part": row.part, row.key: value}));
                    }
                    if let (true, Some(setup)) = (doc, &row.doc) {
                        run(&mut s, "embroidery.setup", setup.clone());
                    }
                    let opts = if export { json!({row.key: row.export}) } else { json!({}) };
                    let plan = digitize(&s.doc().unwrap().doc, &Options::from_json(&opts).unwrap(), None).unwrap();
                    let unit = plan
                        .units
                        .iter()
                        .find(|u| matches!((u.origin.part, row.part), (plan::Part::Fill(_), "fill") | (plan::Part::Stroke(_), "stroke")))
                        .unwrap();
                    let got = (row.read)(&unit.params.unwrap());
                    let (want, source) = match (object, export, doc) {
                        (true, ..) => (row.object.clone().unwrap_or_default(), "object"),
                        (false, true, _) => (row.export.clone(), "export"),
                        (false, false, true) => (default_of_doc(&row), "document"),
                        (false, false, false) => (default.clone(), "default"),
                    };
                    let what = format!("{} on the {} (object {object}, export {export}, document {doc})", row.key, row.part);
                    assert!(same(&got, &want), "{what}: planned with {got}, not {want}");
                    let mut ask = opts.clone();
                    ask["ids"] = json!([id]);
                    let info = run(&mut s, "embroidery.info", ask);
                    // The part's own keys: the design's are reported by embroidery.settings.
                    if super::resolve::report::REPORTED.contains(&row.key) {
                        let reported = &info[row.part]["settings"][row.key];
                        assert_eq!(reported["source"], json!(source), "{what}");
                        // The value reported is the one planned (a pull's sides as one), unless a
                        // rule chooses it (`"auto"`: the part's `sewn` and the key's `rule` say what).
                        let value = match &reported["value"] {
                            Value::Array(sides) => sides[0].clone(),
                            v => v.clone(),
                        };
                        assert!(value == json!("auto") || same(&value, &got), "{what}: info says {value}, planned with {got}");
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 7 * 8 + 5 * 4 + 4 * 4, "every combination");
}

/// A fill's narrow parts, sewn as satin columns: a spacing set on the fill or given to the export is
/// one distance for every thread of the part, the columns' too (fixed, no longer picked by width);
/// the fabric gives tatami and satin values apart, so its tatami spacing is the rows' alone (the
/// columns keep the satin's spacing, picked by width) and its satin spacing percent is the
/// columns'.
#[test]
fn a_fills_columns_take_the_parts_spacing_and_the_fabrics_satin_values() {
    let columns = |object: Option<f64>, export: Option<f64>, setup: Option<Value>| -> Params {
        // A filled band 4 mm wide: narrower than `narrowAsSatin`, so it is sewn as a column.
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 120, "height": 4.0 * PT_PER_MM}))["id"].as_u64().unwrap();
        if let Some(v) = object {
            run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "spacing": v}));
        }
        if let Some(setup) = setup {
            run(&mut s, "embroidery.setup", setup);
        }
        let opts = export.map_or_else(|| json!({}), |v| json!({"spacing": v}));
        let plan = digitize(&s.doc().unwrap().doc, &Options::from_json(&opts).unwrap(), None).unwrap();
        plan.units.iter().find(|u| u.sewn() == Some(Style::Fill)).and_then(|u| u.columns).unwrap()
    };
    let by_width = columns(None, None, None);
    assert!(by_width.auto_spacing);
    for (what, c) in [("on the fill", columns(Some(0.5), None, None)), ("given to the export", columns(None, Some(0.5), None))] {
        assert_eq!((c.density, c.auto_spacing), (0.5, false), "a spacing {what}");
    }
    let tatami = columns(None, None, Some(json!({"fabric": {"tatami": {"spacing": 0.5}}})));
    assert_eq!((tatami.density, tatami.auto_spacing), (by_width.density, true), "the fabric's tatami spacing");
    let satin = columns(None, None, Some(json!({"fabric": {"satin": {"spacingPercent": 80}}})));
    assert_eq!((satin.auto_spacing, satin.spacing_scale), (true, 0.8), "the fabric's satin spacing percent");
}

/// The value a row's document setting gives, in the commands' units.
fn default_of_doc(row: &Row) -> Value {
    let doc = row.doc.as_ref().unwrap();
    let leaf = |v: &Value| -> Value {
        let mut v = v.clone();
        while let Value::Object(o) = v {
            v = o.into_iter().next().unwrap().1;
        }
        v
    };
    leaf(doc)
}

/// Two values the same: numbers to the last bit but for float noise in a sum.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

/// The same value given at any level — the object's own, an export's option, the document's
/// fabric — writes the same bytes.
#[test]
fn the_same_value_at_any_level_writes_the_same_bytes() {
    let dst = |s: &mut Session, opts: Value| {
        let mut p = opts;
        p["format"] = json!("dst");
        let r = run(s, "document.export", p);
        vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()
    };
    for (key, value, doc) in [
        ("pull", json!(0.3), json!({"fabric": {"tatami": {"pull": 0.3}, "satin": {"pull": 0.3}}})),
        ("length", json!(3.0), json!({"fabric": {"tatami": {"length": 3.0}}})),
        ("underlay", json!("edge"), json!({"fabric": {"tatami": {"underlay": "edge"}, "satin": {"underlay": "edge"}}})),
    ] {
        let (mut on_object, id) = sheet();
        for part in ["fill", "stroke"] {
            run(&mut on_object, "embroidery.set", json!({"ids": [id], "part": part, key: value}));
        }
        let (mut on_export, _) = sheet();
        let (mut in_document, _) = sheet();
        run(&mut in_document, "embroidery.setup", doc);
        let bytes = dst(&mut on_object, json!({}));
        assert!(bytes == dst(&mut on_export, json!({key: value})), "{key}: the object's and the export's");
        assert!(bytes == dst(&mut in_document, json!({})), "{key}: the object's and the document's");
    }
}
