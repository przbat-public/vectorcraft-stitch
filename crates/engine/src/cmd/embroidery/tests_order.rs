//! The sewing order as `embroidery.sequence` lists it: each unit maps back to the object a person
//! selects and to its stitches, says how it is sewn and why, and an object whose parts are sewn in
//! different blocks is named.

use serde_json::{Value, json};
use vectorcraft_stitch::Command;

use super::digitize::digitize;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// A rectangle filled with `fill` (or none) and stroked with `stroke` at `weight` points (or not)
/// → its id.
fn rect(s: &mut Session, fill: Option<&str>, stroke: Option<(&str, f64)>, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(s, "paint.setFill", fill.map_or(json!({"none": true}), |c| json!({"color": c})));
    run(s, "paint.setStroke", json!({"none": true}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap();
    if let Some((color, weight)) = stroke {
        run(s, "paint.setStroke", json!({"color": color, "ids": [id]}));
        run(s, "stroke.set", json!({"weight": weight, "ids": [id]}));
    }
    run(s, "select.none", json!({}));
    id
}

/// Every unit listed: its object, part, stitch numbers, entry and exit; how and why it is sewn.
fn units(v: &Value) -> Vec<Value> {
    v["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap().clone()).collect()
}

/// Each unit maps to its object and its stitches: the numbers are the plan's, the entry and exit
/// its first and last needle points, the blocks' units are the plan's in order.
#[test]
fn each_unit_maps_to_its_object_and_its_stitches() {
    let mut s = blank();
    let framed = rect(&mut s, Some("#cc2222"), Some(("#2233cc", 6.0)), 20.0, 20.0, 60.0, 40.0);
    let far = rect(&mut s, Some("#cc2222"), None, 200.0, 200.0, 40.0, 30.0);
    let v = run(&mut s, "embroidery.sequence", json!({}));
    let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let listed = units(&v);
    assert_eq!(listed.len(), plan.units.len());
    for (k, (u, l)) in plan.units.iter().zip(&listed).enumerate() {
        assert_eq!(l["unit"], json!(k + 1));
        assert_eq!(l["id"], json!(u.origin.node.0));
        let numbers = plan.numbers(k).unwrap();
        assert_eq!((&l["first"], &l["last"]), (&json!(numbers.start), &json!(numbers.end - 1)), "unit {k}");
        let block = &plan.blocks[u.block];
        let needle: Vec<_> = block.stitches[u.stitches.clone()].iter().filter(|s| s.command == Command::Stitch).collect();
        assert_eq!(l["stitches"], json!(needle.len()));
        let point = |s: &vectorcraft_stitch::Stitch| json!([stats::mm(s.x), stats::mm(s.y)]);
        assert_eq!((&l["entry"], &l["exit"]), (&point(needle[0]), &point(needle[needle.len() - 1])), "unit {k}");
    }
    // The framed rectangle's fill, then the far one joining its block after a cut; its stroke in a
    // block of its own.
    let ids: Vec<(Value, Value)> = listed.iter().map(|l| (l["id"].clone(), l["part"].clone())).collect();
    assert_eq!(ids, [(json!(framed), json!("fill 1")), (json!(far), json!("fill 1")), (json!(framed), json!("stroke 1"))]);
    let blocks = v["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["units"].as_array().unwrap().len(), 2);
    assert_eq!(blocks[0]["trims"], json!(1), "the move to the far rectangle is cut");
    assert_eq!(blocks[0]["then"]["colorChange"], json!(true));
    assert_eq!(blocks[1]["then"], json!(null), "nothing after the last block");
    // The framed rectangle's fill and stroke are sewn in different blocks.
    assert_eq!(v["split"], json!([{"id": framed, "blocks": [1, 2]}]));
}

/// A unit of one needle point where the needle already is adds no record to its block: it is
/// listed without stitch numbers, rather than with a last number before its first.
#[test]
fn a_unit_that_adds_no_record_has_no_numbers() {
    use vectorcraft_stitch::plan::{Params, Style};
    use vectorcraft_stitch::thread::Palette;

    use super::sequence::Sequence;
    use super::test_support::unit;

    let unit = |node: u64, points: &[(f64, f64)]| unit(node, Style::Running, [200, 30, 30], points, None);
    let mut seq = Sequence::new(Params::for_style(Style::Running));
    seq.add(unit(1, &[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0)]));
    seq.add(unit(2, &[(4.0, 0.0)]));
    let plan = seq.plan(String::new(), Vec::new(), Palette::Jef);
    let units = units(&super::order::sequence(&plan));
    assert_eq!(units.len(), 2);
    assert_eq!((&units[0]["first"], &units[0]["last"]), (&json!(1), &json!(3)));
    assert_eq!((&units[1]["first"], &units[1]["last"], &units[1]["stitches"]), (&json!(null), &json!(null), &json!(0)));
}

/// How each unit is sewn and the rule that chose it.
#[test]
fn each_unit_says_how_it_is_sewn_and_why() {
    let mut s = blank();
    rect(&mut s, Some("#cc2222"), None, 20.0, 20.0, 60.0, 40.0);
    rect(&mut s, None, Some(("#2233cc", 6.0)), 100.0, 20.0, 60.0, 40.0);
    rect(&mut s, None, Some(("#22aa44", 0.5)), 180.0, 20.0, 60.0, 40.0);
    // A letter T whose strokes are 3 mm wide: tatami with satin columns.
    let t = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];
    let d: Vec<String> = t
        .iter()
        .enumerate()
        .map(|(k, (x, y))| format!("{} {} {}", if k == 0 { "M" } else { "L" }, 20.0 + x * PT_PER_MM, 150.0 + y * PT_PER_MM))
        .collect();
    run(&mut s, "paint.setFill", json!({"color": "#aa22aa"}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "path.create", json!({"d": format!("{} Z", d.join(" "))}));
    let sewn = |s: &mut Session, p: Value| -> Vec<(String, String)> {
        units(&run(s, "embroidery.sequence", p))
            .iter()
            .map(|l| (l["sewn"].as_str().unwrap().to_string(), l["why"].as_str().unwrap().to_string()))
            .collect()
    };
    let auto = sewn(&mut s, json!({}));
    let how: Vec<&str> = auto.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(how, ["tatami", "satin", "running", "tatami with satin columns"]);
    assert!(auto[0].1.contains("no part narrower than 7 mm"), "{}", auto[0].1);
    assert!(auto[1].1.contains("at least 1 mm wide is a satin column"), "{}", auto[1].1);
    assert!(auto[2].1.contains("narrower than 1 mm is a running line"), "{}", auto[2].1);
    assert!(auto[3].1.contains("narrower than 7 mm are satin columns"), "{}", auto[3].1);
    // Forced by stitchStyle: every unit says so; a stroke filled is the area it paints.
    // Satin forced on a fill, which has no width to sew across: the planner fills it, and the list
    // says so.
    let satin = sewn(&mut s, json!({"stitchStyle": "satin"}));
    let fill_as_satin = ("tatami".to_string(), "stitchStyle satin: a fill has no width to sew a satin across, so it is filled".to_string());
    let stroke_as_satin = ("satin".to_string(), "stitchStyle satin".to_string());
    assert_eq!(satin, [fill_as_satin.clone(), stroke_as_satin.clone(), stroke_as_satin, fill_as_satin]);
    let fill = sewn(&mut s, json!({"stitchStyle": "fill"}));
    assert_eq!(fill[1].1, "stitchStyle fill: the area the stroke paints is filled");
}

/// Art inside a symbol is listed under the instance a person selects, with the object sewn; a
/// design opened from a file is listed as its stitches, sewn as they are.
#[test]
fn symbols_and_opened_designs_are_listed_by_what_a_person_selects() {
    let mut s = blank();
    let square = rect(&mut s, Some("#2233cc"), None, 20.0, 20.0, 20.0, 20.0);
    run(&mut s, "select.set", json!({"ids": [square]}));
    let first = run(&mut s, "symbol.new", json!({"name": "Square"}))["id"].as_u64().unwrap();
    let second = run(&mut s, "symbol.place", json!({"name": "Square", "x": 150, "y": 40}))["id"].as_u64().unwrap();
    let art = s.doc().unwrap().doc.symbols[0].art.id.0;
    let listed = units(&run(&mut s, "embroidery.sequence", json!({})));
    let ids: Vec<(Value, Value, Value)> = listed.iter().map(|l| (l["id"].clone(), l["node"].clone(), l["instance"].clone())).collect();
    assert_eq!(ids, [(json!(first), json!(art), json!(first)), (json!(second), json!(art), json!(second))]);
    let r = run(&mut s, "document.export", json!({"format": "pes"}));
    run(&mut s, "document.open", json!({"name": "design.pes", "dataBase64": r["dataBase64"]}));
    let listed = units(&run(&mut s, "embroidery.sequence", json!({})));
    assert!(!listed.is_empty());
    assert!(listed.iter().all(|l| l["part"] == json!("stitches") && l["sewn"] == json!("literal") && l["instance"].is_null()), "{listed:?}");
}
