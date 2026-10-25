//! `embroidery.info`: each value of a part with where it comes from (the object, the call's option,
//! the document's fabric, the default) and what unsetting it gives; values merged over several
//! objects; how each part is sewn, the walk's own rule; what the plan sews of each part; the same
//! answer from the stitch preview's planning thread.

use serde_json::{Value, json};

use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// A rectangle filled red, with a blue stroke `stroke` mm wide (none at 0), at `x` → its id.
fn shape(s: &mut Session, x: f64, stroke: f64) -> u64 {
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    if stroke > 0.0 {
        run(s, "paint.setStroke", json!({"color": "#2233cc"}));
    } else {
        run(s, "paint.setStroke", json!({"none": true}));
    }
    let id = run(s, "shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
    if stroke > 0.0 {
        run(s, "stroke.set", json!({"weight": stroke * PT_PER_MM, "ids": [id]}));
    }
    run(s, "select.none", json!({}));
    id
}

fn info(s: &mut Session, params: Value) -> Value {
    run(s, "embroidery.info", params)
}

/// Each value with where it comes from and what unsetting the object's own gives: the object's,
/// the document's fabric, the defaults (a rule as `"auto"`); how each part is sewn and why.
#[test]
fn each_value_says_where_it_comes_from() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 3.0);
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = Some(Box::new(serde_json::from_value(json!({"fabric": {"tatami": {"pull": 0.35}}})).unwrap()));
        Ok(())
    })
    .unwrap();
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 30, "pull": 0.3}));
    let r = info(&mut s, json!({"ids": [id]}));
    assert_eq!((&r["objectCount"], &r["planned"]), (&json!(1), &json!(false)));
    assert_eq!(r["objects"][0]["settings"], json!({"fill": {"angle": 30.0, "pull": [0.3, 0.3]}, "stroke": null}));
    let fill = &r["fill"];
    assert_eq!((&fill["sewn"], &fill["rule"], &fill["itemCount"]), (&json!("tatami"), &json!("fill"), &json!(1)));
    let key = |k: &str| fill["settings"][k].clone();
    assert_eq!(
        key("angle"),
        json!({"value": 30.0, "source": "object", "section": null, "setCount": 1, "mixed": false,
               "inherited": {"value": 135.0, "source": "default", "section": null}, "rule": null, "applies": true})
    );
    assert_eq!(key("pull")["inherited"], json!({"value": [0.35, 0.35], "source": "document", "section": "fabric"}));
    assert_eq!(
        (&key("spacing")["value"], &key("spacing")["source"]),
        (&json!(vectorcraft_stitch::plan::Params::for_style(vectorcraft_stitch::plan::Style::Fill).density), &json!("default"))
    );
    assert_eq!((&key("stitch")["value"], &key("underlay")["value"]), (&json!("auto"), &json!("auto")));
    let stroke = &r["stroke"];
    assert_eq!((&stroke["sewn"], &stroke["rule"]), (&json!("satin"), &json!("wideStroke")));
    assert_eq!(stroke["settings"]["spacing"]["value"], json!("auto"), "picked by its width");
    // An option given to the call is the export's layer.
    let r = info(&mut s, json!({"ids": [id], "length": 3}));
    assert_eq!((&r["fill"]["settings"]["length"]["value"], &r["fill"]["settings"]["length"]["source"]), (&json!(3.0), &json!("export")));
}

/// Over several objects: a value they share, else none and `mixed`, with how many set it; a part
/// none of them has is `null`; no objects, an empty answer.
#[test]
fn values_are_merged_over_the_objects() {
    let mut s = blank();
    let (a, b) = (shape(&mut s, 20.0, 0.0), shape(&mut s, 100.0, 0.0));
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "pull": 0.3}));
    run(&mut s, "embroidery.set", json!({"ids": [b], "part": "fill", "angle": 60, "pull": 0.3}));
    let r = info(&mut s, json!({"ids": [a, b]}));
    let angle = &r["fill"]["settings"]["angle"];
    assert_eq!((&angle["value"], &angle["mixed"], &angle["setCount"], &angle["source"]), (&Value::Null, &json!(true), &json!(2), &json!("object")));
    assert_eq!(r["fill"]["settings"]["pull"]["value"], json!([0.3, 0.3]));
    // The rectangles drawn without a stroke may keep an unpainted one: a part is null only when no
    // object has it.
    let none = info(&mut s, json!({}));
    assert_eq!((&none["objectCount"], &none["fill"], &none["stroke"]), (&json!(0), &Value::Null, &Value::Null), "nothing selected");
    run(&mut s, "embroidery.set", json!({"ids": [b], "part": "fill", "stitch": "none"}));
    let r = info(&mut s, json!({"ids": [b]}));
    assert_eq!((&r["fill"]["sewn"], &r["fill"]["rule"], &r["fill"]["reason"]), (&json!("none"), &Value::Null, &json!("off")));
}

/// A part the walk sews nothing of says so, and why, as the plan has it: a fill that paints nothing
/// (a line's), a hidden object's parts, an object on a non-printing layer; a brushed stroke says
/// that the walk sews it as the area its brush paints.
#[test]
fn a_part_sewn_as_the_walk_sews_it_or_why_not() {
    let mut s = blank();
    let line = shape(&mut s, 20.0, 1.0);
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [line]}));
    let r = info(&mut s, json!({"ids": [line]}));
    assert_eq!((&r["fill"]["sewn"], &r["fill"]["reason"]), (&json!("none"), &json!("unpainted")));
    assert_eq!((&r["stroke"]["sewn"], &r["stroke"]["reason"]), (&json!("satin"), &Value::Null));
    let hidden = shape(&mut s, 100.0, 1.0);
    run(&mut s, "object.hide", json!({"ids": [hidden]}));
    let r = info(&mut s, json!({"ids": [hidden]}));
    assert_eq!((&r["fill"]["reason"], &r["stroke"]["reason"]), (&json!("hidden"), &json!("hidden")));
    // A stroke drawn with a scatter brush, beside a fill: the walk sews its brush's art as areas.
    let dot = shape(&mut s, 300.0, 0.0);
    let brush = run(&mut s, "brush.new", json!({"type": "scatter", "ids": [dot]}))["name"].as_str().unwrap().to_string();
    let brushed = shape(&mut s, 20.0, 1.0);
    run(&mut s, "brush.apply", json!({"name": brush, "ids": [brushed]}));
    let r = info(&mut s, json!({"ids": [brushed]}));
    assert_eq!((&r["stroke"]["sewn"], &r["stroke"]["rule"]), (&json!("tatami"), &json!("strokeOutlined")));
    run(&mut s, "embroidery.set", json!({"ids": [brushed], "part": "stroke", "stitch": "running"}));
    let r = info(&mut s, json!({"ids": [brushed]}));
    assert_eq!((&r["stroke"]["sewn"], &r["stroke"]["rule"]), (&json!("running"), &json!("forced")));
    let layer = run(&mut s, "layer.new", json!({}))["id"].as_u64().unwrap();
    let off_print = shape(&mut s, 180.0, 0.0);
    run(&mut s, "layer.setProps", json!({"ids": [layer], "printable": false}));
    assert_eq!(info(&mut s, json!({"ids": [off_print]}))["fill"]["reason"], json!("notPrinted"));
}

/// How each part is sewn is the walk's own rule: for every kind of part, `info` says what the plan
/// sews, and with `plan`, what it sews of the part.
#[test]
fn info_says_what_the_walk_sews() {
    let mut s = blank();
    let fill = shape(&mut s, 20.0, 0.0);
    let wide = shape(&mut s, 100.0, 3.0);
    let narrow = shape(&mut s, 180.0, 0.5);
    run(&mut s, "embroidery.set", json!({"ids": [narrow], "part": "stroke", "stitch": "tatami"}));
    let sequence = run(&mut s, "embroidery.sequence", json!({}));
    let units: Vec<&Value> = sequence["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).collect();
    for id in [fill, wide, narrow] {
        let r = info(&mut s, json!({"ids": [id], "plan": true}));
        for part in ["fill", "stroke"] {
            let of: Vec<&&Value> = units.iter().filter(|u| u["id"] == json!(id) && u["part"] == json!(part)).collect();
            let Some(u) = of.first() else {
                // No unit: a part none of its items of is sewn says so.
                assert!(r[part].is_null() || r[part]["sewn"] == json!("none"), "{id} {part}: {}", r[part]);
                continue;
            };
            assert_eq!((&r[part]["sewn"], &r[part]["rule"]), (&u["sewn"], &u["rule"]), "{id} {part}");
            assert_eq!(r[part]["sewing"]["unitCount"], json!(of.len()));
            assert!(r[part]["sewing"]["stitches"].as_u64().unwrap() > 0, "{r}");
        }
        assert_eq!(r["planned"], json!(true));
    }
}

/// What a plan already made sews of a selection (the Object tab's count): each part as `info`
/// counts it with `plan`, a group for what it holds, nothing for an object the plan sews nothing
/// of.
#[test]
fn a_selections_stitches_are_read_from_the_plan() {
    let mut s = blank();
    let fill = shape(&mut s, 20.0, 0.0);
    let both = shape(&mut s, 100.0, 3.0);
    let off = shape(&mut s, 180.0, 0.0);
    run(&mut s, "embroidery.set", json!({"ids": [off], "part": "fill", "stitch": "none"}));
    let plan = s.embroidery_plan(&Options::default()).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let ids = |v: &[u64]| v.iter().map(|id| vectorcraft_doc::NodeId(*id)).collect::<Vec<_>>();
    for id in [fill, both] {
        let r = info(&mut s, json!({"ids": [id], "plan": true}));
        let sewn = info::selection_sewing(&doc, &plan, &ids(&[id]));
        let counted = |part: &str| r[part]["sewing"]["stitches"].as_u64().unwrap_or(0) as usize;
        assert_eq!((sewn.fill.stitches, sewn.stroke.stitches), (counted("fill"), counted("stroke")), "{id}");
        assert_eq!(sewn.stitches(), counted("fill") + counted("stroke"));
    }
    assert_eq!(info::selection_sewing(&doc, &plan, &ids(&[off])), info::SelectionSewing::default(), "not sewn");
    let parts = |v: &[u64]| info::selection_sewing(&doc, &plan, &ids(v)).stitches();
    let group = run(&mut s, "object.group", json!({"ids": [fill, both]}))["id"].as_u64().unwrap();
    let (plan, doc) = (s.embroidery_plan(&Options::default()).unwrap(), s.doc().unwrap().doc.clone());
    assert_eq!(info::selection_sewing(&doc, &plan, &ids(&[group])).stitches(), parts(&[fill, both]), "a group for what it holds");
}

/// The stitch preview's planning thread answers `embroidery.info` (objects given) as the command
/// does, planning only with `plan`; a design too large to plan still has its settings answered,
/// the reason in `warnings`.
#[test]
fn a_planner_answers_info_as_the_command_does() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 3.0);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "pull": 0.4}));
    let planner = s.embroidery_planner().unwrap();
    for plan in [false, true] {
        let ask = json!({"ids": [id], "plan": plan});
        let queried = planner.query("embroidery.info", &ask).unwrap();
        assert_eq!(queried, info(&mut s, ask), "plan {plan}");
        assert_eq!((&queried["planned"], queried["stroke"]["sewing"].is_null()), (&json!(plan), !plan));
    }
    assert!(planner.query("embroidery.info", &json!({"ids": [id], "selectedOnly": true})).is_err());
    // Three metres a side: more stitches than one shape may have.
    let huge = run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500}))["id"].as_u64().unwrap();
    let planner = s.embroidery_planner().unwrap();
    let queried = planner.query("embroidery.info", &json!({"ids": [huge], "plan": true})).unwrap();
    assert_eq!((&queried["planned"], &queried["fill"]["sewn"]), (&json!(false), &json!("tatami")));
    assert!(queried["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("stitches")), "{}", queried["warnings"]);
}

/// What Auto chose is reported where the value is `"auto"`: a satin's underlay by its width
/// (strokes 1.5, 3 and 5 mm wide: a centre run, an edge run, an edge run and a zigzag) and its
/// spacing by its width; a fill's underlay by its size, which only the planned area tells. A value
/// set is no rule's.
#[test]
fn what_auto_chose_is_reported() {
    use vectorcraft_stitch::plan::{Params, Style, satin_auto_underlay};
    for (width, chose) in [(1.5, "center"), (3.0, "edge"), (5.0, "edgeZigzag")] {
        assert_eq!(satin_auto_underlay(width).id(), chose);
        let mut s = blank();
        let id = shape(&mut s, 20.0, width);
        let r = info(&mut s, json!({"ids": [id]}));
        let underlay = &r["stroke"]["settings"]["underlay"];
        assert_eq!(
            (&underlay["value"], &underlay["rule"]["id"], &underlay["rule"]["chose"]),
            (&json!("auto"), &json!("underlayByWidth"), &json!(chose)),
            "{width} mm"
        );
        let spacing = &r["stroke"]["settings"]["spacing"]["rule"];
        let p = Params { width, ..Params::for_style(Style::Satin) };
        assert_eq!((&spacing["id"], &spacing["chose"]), (&json!("spacingByWidth"), &json!(p.column_spacing(width))));
        assert_eq!(
            (&r["fill"]["settings"]["underlay"]["rule"]["id"], &r["fill"]["settings"]["underlay"]["rule"]["chose"]),
            (&json!("underlayBySize"), &Value::Null)
        );
        run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "underlay": "center"}));
        assert_eq!(info(&mut s, json!({"ids": [id]}))["stroke"]["settings"]["underlay"]["rule"], Value::Null, "set: no rule's");
    }
}

/// The sewing order says which setting forced a style: the object's own `stitch`, or the option
/// given to the call.
#[test]
fn the_sewing_order_says_which_setting_forced_a_style() {
    let mut s = blank();
    let (a, b) = (shape(&mut s, 20.0, 3.0), shape(&mut s, 100.0, 3.0));
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "stroke", "stitch": "running"}));
    let order = run(&mut s, "embroidery.sequence", json!({"stitch": "running"}));
    let units: Vec<&Value> = order["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).collect();
    let of = |id: u64, part: &str| units.iter().find(|u| u["id"] == json!(id) && u["part"] == json!(part)).unwrap()["ruleValues"].clone();
    assert_eq!(of(a, "stroke"), json!({"stitch": "running", "source": "object"}));
    assert_eq!(of(b, "stroke")["source"], json!("export"));
    assert_eq!(of(a, "fill"), json!({"stitch": "running", "source": "export"}));
}

/// What a newer version wrote that this version doesn't read is listed as kept, and a stitch or an
/// underlay it doesn't know counts as unset: the layers below give the value, and `setCount` says
/// none of the objects sets it.
#[test]
fn what_this_version_cant_read_is_kept_and_not_counted_as_set() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 0.0);
    s.edit("Newer Settings", |d, _| {
        let e = serde_json::from_value(json!({"branch": true, "fill": {"stitch": "motif", "underlay": "doubleZigzag", "grain": 2, "angle": 30}}))
            .map_err(|e| EngineError::Other(e.to_string()))?;
        d.node_mut(vectorcraft_doc::NodeId(id)).unwrap().extras_mut().embroidery = Some(Box::new(e));
        Ok(())
    })
    .unwrap();
    let r = info(&mut s, json!({"ids": [id]}));
    let mut kept: Vec<String> = serde_json::from_value(r["objects"][0]["kept"].clone()).unwrap();
    kept.sort();
    assert_eq!(kept, ["branch", "fill.grain", "fill.stitch", "fill.underlay"]);
    let settings = &r["fill"]["settings"];
    for key in ["stitch", "underlay"] {
        assert_eq!((&settings[key]["setCount"], &settings[key]["source"]), (&json!(0), &json!("default")), "{key}");
    }
    assert_eq!((&settings["angle"]["setCount"], &settings["angle"]["source"]), (&json!(1), &json!("object")));
}

/// A key the planner doesn't use for the stitch a part is sewn in is reported as set, and as not
/// applying: a running line's pull and angle; a fill's push reaches its narrow parts' columns.
#[test]
fn a_key_a_stitch_doesnt_use_says_so() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 0.5);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "pull": 0.3, "angle": 20}));
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "push": 0.3}));
    let r = info(&mut s, json!({"ids": [id]}));
    let stroke = &r["stroke"]["settings"];
    assert_eq!(r["stroke"]["sewn"], json!("running"));
    for key in ["pull", "angle"] {
        assert_eq!((&stroke[key]["source"], &stroke[key]["applies"]), (&json!("object"), &json!(false)), "{key}");
    }
    assert_eq!((&stroke["length"]["applies"], &r["fill"]["settings"]["push"]["applies"]), (&json!(true), &json!(true)));
}

/// The reply is the typed answer serialised: the command's reply reads back as the answer the
/// session gives typed (`Session::embroidery_info`), for the Object tab to read without keys typed
/// as strings.
#[test]
fn the_reply_reads_back_as_the_typed_answer() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 3.0);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 30}));
    let mut r = info(&mut s, json!({"ids": [id], "plan": true}));
    r.as_object_mut().unwrap().remove("options");
    let read: super::info::ObjectSettings = serde_json::from_value(r).unwrap();
    let typed = s.embroidery_info(&[vectorcraft_doc::NodeId(id)], true, &Options::default()).unwrap();
    assert_eq!(read, typed);
    let fill = typed.fill.unwrap();
    assert_eq!((fill.settings["angle"].value.clone(), fill.sewing.unwrap().unit_count), (json!(30.0), 1));
}

/// The underlays a panel offers under a fill and under a satin (`distinct_underlays`) are the kinds
/// the planner sews apart: each kind left out sews exactly as one offered does, and the kinds
/// offered (but Auto, a rule by size that may pick one of them) sew differently from each other.
#[test]
fn the_underlays_offered_are_those_the_planner_sews_apart() {
    use super::plan::Part;
    use vectorcraft_stitch::plan::Underlay;
    for (stroke, part, sewn) in [(0.0, "fill", "tatami"), (3.0, "stroke", "satin")] {
        let sewn_with = |underlay: &str| {
            let mut s = blank();
            let id = shape(&mut s, 20.0, stroke);
            if part == "stroke" {
                run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "satin"}));
            }
            run(&mut s, "embroidery.set", json!({"ids": [id], "part": part, "underlay": underlay}));
            let plan = s.embroidery_plan(&Options::default()).unwrap();
            plan.units
                .iter()
                .filter(|u| matches!((u.origin.part, part), (Part::Fill(_), "fill") | (Part::Stroke(_), "stroke")))
                .map(|u| format!("{:?}", plan.blocks.get(u.block).and_then(|b| b.stitches.get(u.stitches.clone()))))
                .collect::<String>()
        };
        let all: Vec<(&str, String)> = Underlay::ALL.iter().map(|u| (u.id(), sewn_with(u.id()))).collect();
        let of = |id: &str| all.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone()).unwrap();
        let offered = distinct_underlays(sewn);
        let ruled: Vec<&str> = offered.iter().copied().filter(|id| *id != "auto").collect();
        for (i, a) in ruled.iter().enumerate() {
            for b in &ruled[i + 1..] {
                assert_ne!(of(a), of(b), "{sewn}: {a} and {b} sew alike, but both are offered");
            }
        }
        for (id, stitches) in all.iter().filter(|(id, _)| !offered.contains(id)) {
            assert!(ruled.iter().any(|o| of(o) == *stitches), "{sewn}: {id} sews as none offered");
        }
    }
}
