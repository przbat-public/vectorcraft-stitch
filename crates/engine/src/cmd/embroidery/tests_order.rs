//! The sewing order as `embroidery.sequence` lists it: each unit maps back to the object a person
//! selects and to its stitches, says how it is sewn and why, and an object whose parts are sewn in
//! different blocks is named.

use serde_json::{Value, json};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::MAX_SATIN_WIDTH;

use super::digitize::digitize;
use super::test_support::{ALL, PT_PER_MM, blank, run};
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
        assert_eq!(l["id"], json!(u.origin.selected().0));
        let numbers = plan.numbers(k).unwrap();
        assert_eq!((&l["firstRecord"], &l["lastRecord"]), (&json!(numbers.start), &json!(numbers.end - 1)), "unit {k}");
        assert_eq!(l["records"], json!(numbers.len()), "unit {k}");
        let block = &plan.blocks[u.block];
        let needle: Vec<_> = block.stitches[u.stitches.clone()].iter().filter(|s| s.command == Command::Stitch).collect();
        assert_eq!(l["stitches"], json!(needle.len()));
        let point = |s: &vectorcraft_stitch::Stitch| json!([stats::mm(s.x), stats::mm(s.y)]);
        assert_eq!((&l["entry"], &l["exit"]), (&point(needle[0]), &point(needle[needle.len() - 1])), "unit {k}");
    }
    // The framed rectangle's fill, then the far one joining its block after a cut; its stroke in a
    // block of its own.
    let ids: Vec<(Value, Value, Value, Value)> =
        listed.iter().map(|l| (l["id"].clone(), l["part"].clone(), l["partIndex"].clone(), l["occurrence"].clone())).collect();
    let one = json!(1);
    assert_eq!(
        ids,
        [
            (json!(framed), json!("fill"), one.clone(), one.clone()),
            (json!(far), json!("fill"), one.clone(), one.clone()),
            (json!(framed), json!("stroke"), one.clone(), one.clone())
        ]
    );
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
    let units = units(&super::order::sequence(&plan, ALL));
    assert_eq!(units.len(), 2);
    assert_eq!((&units[0]["firstRecord"], &units[0]["lastRecord"]), (&json!(1), &json!(3)));
    assert_eq!((&units[1]["firstRecord"], &units[1]["lastRecord"], &units[1]["stitches"]), (&json!(null), &json!(null), &json!(0)));
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
    assert_eq!(how, ["tatami", "satin", "running", "tatami"]);
    // The rule as an id with its values, and whether a fill's narrow parts are satin columns.
    let listed = units(&run(&mut s, "embroidery.sequence", json!({})));
    let rules: Vec<(Value, Value, Value)> = listed.iter().map(|l| (l["rule"].clone(), l["ruleValues"].clone(), l["columns"].clone())).collect();
    assert_eq!(
        rules,
        [
            (json!("fill"), json!({"narrowAsSatin": 7.0}), json!(false)),
            (json!("wideStroke"), json!({"width": 2.12, "satinFrom": 1.0, "widest": MAX_SATIN_WIDTH}), json!(false)),
            (json!("narrowStroke"), json!({"satinFrom": 1.0}), json!(false)),
            (json!("fill"), json!({"narrowAsSatin": 7.0}), json!(true)),
        ]
    );
    assert!(auto[0].1.contains("no part narrower than 7 mm"), "{}", auto[0].1);
    assert!(auto[1].1.contains("at least 1 mm wide is a satin column"), "{}", auto[1].1);
    assert!(auto[2].1.contains("narrower than 1 mm is a running line"), "{}", auto[2].1);
    assert!(auto[3].1.contains("narrower than 7 mm are satin columns"), "{}", auto[3].1);
    // Forced by the stitch option: every unit says so; a stroke filled is the area it paints.
    // Satin forced on a fill, which has no width to sew across: the planner fills it, and the list
    // says so.
    let satin = sewn(&mut s, json!({"stitchStyle": "satin"}));
    let fill_as_satin = ("tatami".to_string(), "stitch satin: a fill has no width to sew a satin across, so it is filled".to_string());
    let stroke_as_satin = ("satin".to_string(), "stitch satin".to_string());
    // Filled as without the option: the T's narrow strokes are satin columns.
    let t_as_satin = ("tatami".to_string(), format!("{}, its narrow parts as satin columns", fill_as_satin.1));
    assert_eq!(satin, [fill_as_satin, stroke_as_satin.clone(), stroke_as_satin, t_as_satin]);
    let fill = sewn(&mut s, json!({"stitchStyle": "fill"}));
    assert_eq!(fill[1].1, "stitch fill: the area the stroke paints is filled");
    let forced = units(&run(&mut s, "embroidery.sequence", json!({"stitch": "satin"})));
    assert_eq!((&forced[1]["rule"], &forced[1]["ruleValues"]), (&json!("forced"), &json!({"stitch": "satin"})));
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
    assert!(
        listed.iter().all(|l| l["part"] == json!("literal")
            && l["partIndex"].is_null()
            && l["sewn"] == json!("literal")
            && l["rule"] == json!("literal")
            && l["instance"].is_null()),
        "{listed:?}"
    );
}

/// A listed unit's key: `id`, `part`, `partIndex` and `occurrence`.
type Key = (Value, Value, Value, Value);

/// Each listed unit's key, and the object it sews.
fn keys_of(v: &Value) -> (Vec<Key>, Vec<Value>) {
    let listed = units(v);
    let keys = listed.iter().map(|l| (l["id"].clone(), l["part"].clone(), l["partIndex"].clone(), l["occurrence"].clone())).collect();
    (keys, listed.iter().map(|l| l["node"].clone()).collect())
}

/// The keys of `keys`, each once, sorted: what a plan's units are told apart by.
fn unique(keys: &[Key]) -> Vec<String> {
    let mut seen: Vec<String> = keys.iter().map(|k| format!("{k:?}")).collect();
    seen.sort();
    let all = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), all, "a key twice: {keys:?}");
    seen
}

/// A symbol whose art is two paths, placed twice: each instance's two units share its id and part
/// ("fill 1") and are told apart by their occurrence, 1 and 2; the keys are unique and the same
/// after an edit elsewhere.
#[test]
fn a_symbols_paths_are_told_apart_by_their_occurrence() {
    let mut s = blank();
    let a = rect(&mut s, Some("#cc2222"), None, 20.0, 20.0, 20.0, 20.0);
    let b = rect(&mut s, Some("#cc2222"), None, 50.0, 20.0, 20.0, 20.0);
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    let first = run(&mut s, "symbol.new", json!({"name": "Pair"}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    let second = run(&mut s, "symbol.place", json!({"name": "Pair", "x": 150, "y": 150}))["id"].as_u64().unwrap();
    let (keys, _) = keys_of(&run(&mut s, "embroidery.sequence", json!({})));
    let before = unique(&keys);
    for instance in [first, second] {
        let mine: Vec<&Value> = keys.iter().filter(|k| k.0 == json!(instance)).map(|k| &k.3).collect();
        assert_eq!(mine, [&json!(1), &json!(2)], "instance {instance}: {keys:?}");
        assert!(keys.iter().filter(|k| k.0 == json!(instance)).all(|k| k.1 == json!("fill") && k.2 == json!(1)));
    }
    rect(&mut s, Some("#2233cc"), None, 250.0, 250.0, 20.0, 20.0);
    let (after, _) = keys_of(&run(&mut s, "embroidery.sequence", json!({})));
    let after = unique(&after);
    assert!(before.iter().all(|k| after.contains(k)), "{before:?} → {after:?}");
}

/// An object whose appearance expands into several pieces (two fills): its pieces are sewn as new
/// objects, with new ids at every plan, all selected as the object and each "fill 1"; their
/// occurrences tell them apart, and the keys are the same at the next plan while the pieces' own
/// ids are not.
#[test]
fn an_expanded_objects_pieces_are_told_apart_by_their_occurrence() {
    let mut s = blank();
    let id = rect(&mut s, Some("#cc2222"), None, 20.0, 20.0, 80.0, 60.0);
    run(&mut s, "appearance.addFill", json!({"ids": [id]}));
    run(&mut s, "appearance.setItem", json!({"ids": [id], "index": 0, "color": "#2233cc"}));
    let v = run(&mut s, "embroidery.sequence", json!({}));
    let (keys, nodes) = keys_of(&v);
    let before = unique(&keys);
    let mine: Vec<&Key> = keys.iter().filter(|k| k.0 == json!(id)).collect();
    assert_eq!(mine.len(), 2, "two pieces: {v}");
    assert!(mine.iter().all(|k| k.1 == json!("fill") && k.2 == json!(1)), "{mine:?}");
    assert_eq!(mine.iter().map(|k| k.3.clone()).collect::<Vec<_>>(), [json!(1), json!(2)]);
    assert!(nodes.iter().all(|n| *n != json!(id)), "the pieces are sewn as new objects: {nodes:?}");
    // An edit that makes a new object: the pieces get other ids, the keys stay.
    rect(&mut s, Some("#22aa44"), None, 200.0, 200.0, 20.0, 20.0);
    let (keys_after, nodes_after) = keys_of(&run(&mut s, "embroidery.sequence", json!({})));
    let after = unique(&keys_after);
    assert!(before.iter().all(|k| after.contains(k)), "{before:?} → {after:?}");
    let pieces =
        |keys: &[Key], nodes: &[Value]| -> Vec<Value> { keys.iter().zip(nodes).filter(|(k, _)| k.0 == json!(id)).map(|(_, n)| n.clone()).collect() };
    assert_ne!(pieces(&keys, &nodes), pieces(&keys_after, &nodes_after), "node is no key: the pieces' ids change");
}

/// `embroidery.sequence`'s answer as it was built before the typed model, by hand: the reference
/// the typed model's serialisation must equal.
fn sequence_as_json_was(plan: &super::plan::DesignPlan, page: super::stats::Page) -> Value {
    let (blocks_counted, units) = super::stats::counts(plan);
    // Each object (as a person selects it) and the blocks its units are in.
    let mut blocks_of: std::collections::BTreeMap<u64, Vec<usize>> = std::collections::BTreeMap::new();
    for u in &plan.units {
        let blocks = blocks_of.entry(u.origin.selected().0).or_default();
        if !blocks.contains(&(u.block + 1)) {
            blocks.push(u.block + 1);
        }
    }
    let listed = page.range(plan.units.len());
    let blocks: Vec<Value> = plan
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.units.start < listed.end && listed.start < b.units.end)
        .map(|(k, b)| {
            let list: Vec<Value> = (b.units.start.max(listed.start)..b.units.end.min(listed.end))
                .filter_map(|i| Some((i, plan.units.get(i)?)))
                .map(|(i, u)| {
                    let t = units.get(i).copied().unwrap_or_default();
                    let needle = || plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch);
                    // A unit of one needle point where the needle already is adds no record: no numbers.
                    let numbers = plan.numbers(i).filter(|n| !n.is_empty());
                    let (part, part_index) = super::order::part_of(u.origin.part);
                    let (rule, values, why) = super::order::rule_of(u);
                    json!({
                        "unit": i + 1,
                        "id": u.origin.selected().0,
                        "node": u.origin.node.0,
                        "instance": u.origin.instance.map(|n| n.0),
                        "part": part,
                        "partIndex": part_index,
                        "occurrence": u.occurrence,
                        "sewn": super::order::sewn_of(u),
                        "columns": u.columns.is_some(),
                        "rule": rule,
                        "ruleValues": values,
                        "why": why,
                        "stitches": t.needle,
                        "records": u.stitches.len(),
                        "firstRecord": numbers.as_ref().map(|n| n.start),
                        "lastRecord": numbers.as_ref().map(|n| n.end.saturating_sub(1)),
                        "jumps": t.jumps,
                        "trims": t.trims,
                        "entry": needle().next().map(|s| super::stats::point(s.point())),
                        "exit": needle().next_back().map(|s| super::stats::point(s.point())),
                        "bounds": u.bounds.map(super::stats::rect),
                    })
                })
                .collect();
            let t = blocks_counted.get(k).copied().unwrap_or_default();
            json!({
                "block": k + 1,
                "thread": super::stats::thread_json(&b.thread),
                "stitches": t.needle,
                "records": b.stitches.len(),
                "firstRecord": b.first_number,
                "lastRecord": b.last_number(),
                "jumps": t.jumps,
                "trims": t.trims,
                "unitCount": b.units.len(),
                "units": list,
                // What the machine does before the next block: a colour change, after a cut when the
                // block ends with one.
                "then": (k + 1 < plan.blocks.len()).then(|| json!({
                    "colorChange": true,
                    "trim": b.stitches.last().is_some_and(|s| s.command == Command::Trim),
                })),
            })
        })
        .collect();
    let split: Vec<Value> = blocks_of.into_iter().filter(|(_, b)| b.len() > 1).map(|(id, blocks)| json!({"id": id, "blocks": blocks})).collect();
    let split_count = split.len();
    let split: Vec<Value> = split.into_iter().take(page.limit).collect();
    json!({
        "blockCount": plan.blocks.len(),
        "unitCount": plan.units.len(),
        "offset": listed.start,
        "truncated": page.truncated(plan.units.len()),
        "blocks": blocks,
        "splitCount": split_count,
        "split": split,
        "warnings": plan.messages(),
    })
}

/// The typed sewing order serialises to exactly the reply built by hand before it, on documents
/// of every structure a unit maps back through and on opened designs, page by page.
#[test]
fn the_typed_sewing_order_is_the_reply_it_was() {
    use vectorcraft_testkit::embroidery as fx;
    let mut docs = vec![];
    let mut s = Session::new();
    let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"));
    fx::blank(&mut run);
    fx::structures(&mut run);
    fx::letter_t(&mut run, fx::BLUE, 300.0, 300.0);
    docs.push(s.doc().unwrap().doc.clone());
    for format in ["pes", "dst"] {
        let mut s = Session::new();
        let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"));
        fx::opened_design(&mut run, format);
        docs.push(s.doc().unwrap().doc.clone());
    }
    for doc in docs {
        let plan = super::service::plan_afresh(&doc, &Options::default()).unwrap();
        for (offset, limit) in [(0, super::stats::LIST_LIMIT), (1, 2), (3, 1), (0, 0), (plan.units.len(), 5)] {
            let page = super::stats::Page { offset, limit };
            assert_eq!(super::order::sequence(&plan, page), sequence_as_json_was(&plan, page), "{offset} {limit}");
        }
        let typed: super::order::SewingOrder =
            serde_json::from_value(super::order::sequence(&plan, super::stats::Page { offset: 0, limit: 10 })).unwrap();
        assert_eq!(typed.unit_count, plan.units.len(), "reads back");
    }
}
