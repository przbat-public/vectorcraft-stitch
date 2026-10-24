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
               "inherited": {"value": 135.0, "source": "default", "section": null}})
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
    assert_eq!((&r["fill"]["sewn"], &r["fill"]["rule"]), (&json!("none"), &Value::Null));
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
            let Some(u) = of.first() else { continue };
            assert_eq!((&r[part]["sewn"], &r[part]["rule"]), (&u["sewn"], &u["rule"]), "{id} {part}");
            assert_eq!(r[part]["stitches"]["unitCount"], json!(of.len()));
            assert!(r[part]["stitches"]["stitches"].as_u64().unwrap() > 0, "{r}");
        }
        assert_eq!(r["planned"], json!(true));
    }
}

/// The stitch preview's planning thread answers `embroidery.info` (objects given) as the command
/// does with `plan`.
#[test]
fn a_planner_answers_info_as_the_command_does() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 3.0);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "pull": 0.4}));
    let planner = s.embroidery_planner().unwrap();
    let queried = planner.query("embroidery.info", &json!({"ids": [id]})).unwrap();
    let ran = info(&mut s, json!({"ids": [id], "plan": true}));
    assert_eq!(queried, ran);
    assert!(planner.query("embroidery.info", &json!({"ids": [id], "selectedOnly": true})).is_err());
}
