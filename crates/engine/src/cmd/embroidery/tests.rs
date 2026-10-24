//! The design's stitch plan: every stitch of every block belongs to one sewing unit, a unit says
//! what it sews and what it was planned with, stitches are numbered from 1 in sewing order, and
//! the export writes the plan. The document's embroidery settings: its thread palette, kept in its
//! file. The threads: each block's from the document's palette, which the exports name and
//! `embroidery.threads` lists.

use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::ThreadPalette;
use vectorcraft_geom::Point;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::thread::{MISMATCH_DELTA_E, Palette};
use vectorcraft_stitch::{Command, pt_to_mm};

use super::digitize::digitize;
use super::plan::{BlockThread, DELTA_E_HUNDREDTHS, DesignPlan, FIRST_NUMBER, Part};
use super::test_support::blank;
use super::*;

/// Points per millimetre: the documents count points, the designs millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// How far outside its bounds a unit's needle point may lie: none but rounding.
const ROUNDING: f64 = 1e-9;

/// A filled, unstroked rectangle in `color` (points), left unselected → its id.
fn rect(s: &mut Session, color: &str, x: f64, y: f64, w: f64, h: f64) -> u64 {
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": color})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": w, "height": h})).unwrap()["id"].as_u64().unwrap();
    s.execute("select.none", &json!({})).unwrap();
    id
}

/// Stroke `id` in `color`, `weight` points wide.
fn stroke(s: &mut Session, id: u64, color: &str, weight: f64) {
    s.execute("paint.setStroke", &json!({"color": color, "ids": [id]})).unwrap();
    s.execute("stroke.set", &json!({"weight": weight, "ids": [id]})).unwrap();
}

/// A filled letter T in `color`, its strokes 3 mm wide (narrow enough to be sewn as satin
/// columns), placed at (`x`, `y`) points → its id.
fn letter_t(s: &mut Session, color: &str, x: f64, y: f64) -> u64 {
    let corners = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];
    let d: Vec<String> = corners
        .iter()
        .enumerate()
        .map(|(k, (cx, cy))| format!("{} {} {}", if k == 0 { "M" } else { "L" }, x + cx * PT_PER_MM, y + cy * PT_PER_MM))
        .collect();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": color})).unwrap();
    let id = s.execute("path.create", &json!({"d": format!("{} Z", d.join(" "))})).unwrap()["id"].as_u64().unwrap();
    s.execute("select.none", &json!({})).unwrap();
    id
}

fn plan_of(s: &Session) -> DesignPlan {
    digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap()
}

fn export(s: &mut Session, format: Format) -> Vec<u8> {
    let r = s.execute("document.export", &json!({"format": format.id()})).unwrap();
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()
}

/// Every stitch of every block belongs to exactly one unit, the block's units in order without a
/// gap; each unit's runs are its stretches of needle-down stitches; a unit has its block's thread
/// and its needle points lie within its bounds; blocks are numbered on from the one before.
fn check(plan: &DesignPlan) {
    let (mut number, mut next_unit) = (FIRST_NUMBER, 0);
    for (k, b) in plan.blocks.iter().enumerate() {
        assert_eq!(b.first_number, number, "block {k} is numbered on from the one before");
        assert_eq!(b.units.start, next_unit, "block {k}'s units follow the block before's");
        assert!(!b.units.is_empty(), "block {k} holds a unit");
        let mut at = 0;
        for i in b.units.clone() {
            let u = &plan.units[i];
            assert_eq!((u.block, &u.thread), (k, &b.thread), "unit {i}");
            assert_eq!(u.stitches.start, at, "unit {i} starts where the one before ends");
            at = u.stitches.end;
            let down: Vec<usize> = u.stitches.clone().filter(|&j| b.stitches[j].command == Command::Stitch).collect();
            let in_runs: Vec<usize> = u.runs.iter().flat_map(|r| r.clone()).collect();
            assert_eq!(in_runs, down, "unit {i}'s runs hold its needle-down stitches, each once, in order");
            for r in &u.runs {
                assert!(!r.is_empty() && u.stitches.start <= r.start && r.end <= u.stitches.end, "unit {i}: run {r:?}");
                assert!(r.start == u.stitches.start || b.stitches[r.start - 1].command != Command::Stitch, "unit {i}: run {r:?} starts after a lift");
                assert!(r.end == u.stitches.end || b.stitches[r.end].command != Command::Stitch, "unit {i}: run {r:?} ends at a lift");
            }
            let bounds = u.bounds.unwrap();
            for j in &down {
                let p: Point = b.stitches[*j].point();
                let inside = |v: f64, lo: f64, hi: f64| lo - ROUNDING <= v && v <= hi + ROUNDING;
                assert!(inside(p.x, bounds.x0, bounds.x1) && inside(p.y, bounds.y0, bounds.y1), "unit {i}: {p:?} outside {bounds:?}");
            }
        }
        assert_eq!(at, b.stitches.len(), "block {k}'s units hold all its stitches");
        next_unit = b.units.end;
        number += b.stitches.len();
    }
    assert_eq!(next_unit, plan.units.len(), "every unit is in a block");
    assert_eq!(plan.len(), number - FIRST_NUMBER);
}

/// Every stitch belongs to one unit in fills and strokes, an object both filled and stroked, a clip
/// group, symbols, objects that ask for a trim after them, and an opened design's literal runs.
#[test]
fn every_stitch_belongs_to_one_unit() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    let both = rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 40.0);
    stroke(&mut s, both, "#22aa44", 6.0);
    letter_t(&mut s, "#cc2222", 20.0, 120.0);
    let far = rect(&mut s, "#cc2222", 200.0, 200.0, 30.0, 30.0);
    s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [far]})).unwrap();
    rect(&mut s, "#cc2222", 250.0, 200.0, 30.0, 30.0);
    let plan = plan_of(&s);
    check(&plan);
    assert_eq!(plan, plan_of(&s), "planned twice, the same plan");

    // A clip group, and a symbol placed three times.
    let mut s = blank();
    rect(&mut s, "#cc2222", 0.0, 0.0, 120.0, 120.0);
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 30, "y": 30, "width": 50, "height": 50})).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    s.execute("object.clippingMask.make", &json!({})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    let square = rect(&mut s, "#2233cc", 150.0, 150.0, 20.0, 20.0);
    s.execute("select.set", &json!({"ids": [square]})).unwrap();
    s.execute("symbol.new", &json!({"name": "Square"})).unwrap();
    s.execute("symbol.place", &json!({"name": "Square", "x": 200, "y": 40})).unwrap();
    s.execute("symbol.place", &json!({"name": "Square", "x": 250, "y": 250})).unwrap();
    check(&plan_of(&s));

    // An opened design: literal runs, joined by the file's jumps and trims.
    let pes = export(&mut s, Format::Pes);
    s.execute("document.open", &json!({"name": "design.pes", "dataBase64": vectorcraft_format::base64_encode(&pes)})).unwrap();
    check(&plan_of(&s));
}

/// A unit says what it sews and what it was planned with: an object filled and stroked is two
/// units, a fill and a stroke, each with its own parameters; a fill with narrow parts records the
/// satin columns' parameters too.
#[test]
fn units_say_what_they_sew_and_with_what() {
    let mut s = blank();
    let square = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 60.0);
    stroke(&mut s, square, "#2233cc", 6.0);
    let t = letter_t(&mut s, "#22aa44", 20.0, 120.0);
    let plan = plan_of(&s);
    let of = |id: u64| plan.units.iter().filter(|u| u.origin.selected() == NodeId(id)).collect::<Vec<_>>();
    let parts: Vec<Part> = of(square).iter().map(|u| u.origin.part).collect();
    assert_eq!(parts, [Part::Fill(0), Part::Stroke(0)]);
    let (fill, line) = (of(square)[0], of(square)[1]);
    assert_eq!(fill.params.map(|p| p.style), Some(Style::Fill));
    assert_eq!(fill.columns, None, "a square has no narrow parts");
    let satin = line.params.unwrap();
    assert_eq!(satin.style, Style::Satin);
    assert!((satin.width - pt_to_mm(6.0)).abs() < 1e-9, "the stroke's width: {}", satin.width);
    assert_eq!((fill.thread.art, line.thread.art), ([0xcc, 0x22, 0x22], [0x22, 0x33, 0xcc]));
    let [letter] = of(t)[..] else { panic!("one unit for the T: {:?}", of(t)) };
    assert_eq!(letter.origin.part, Part::Fill(0));
    assert_eq!(letter.columns.map(|p| p.style), Some(Style::Satin), "its strokes are sewn as satin columns");
    assert!(plan.units.iter().all(|u| u.origin.instance.is_none()), "no symbols here");
}

/// A symbol's art is sewn once per instance: each instance's units name the object in the symbol's
/// art and the instance that places it.
#[test]
fn a_symbols_units_are_placed_by_their_instances() {
    let mut s = blank();
    let square = rect(&mut s, "#cc2222", 20.0, 20.0, 20.0, 20.0);
    s.execute("select.set", &json!({"ids": [square]})).unwrap();
    let first = s.execute("symbol.new", &json!({"name": "Square"})).unwrap()["id"].as_u64().unwrap();
    s.execute("select.none", &json!({})).unwrap();
    let second = s.execute("symbol.place", &json!({"name": "Square", "x": 150, "y": 40})).unwrap()["id"].as_u64().unwrap();
    let third = s.execute("symbol.place", &json!({"name": "Square", "x": 250, "y": 250})).unwrap()["id"].as_u64().unwrap();
    let art = s.doc().unwrap().doc.symbols[0].art.id;
    let plan = plan_of(&s);
    let mut instances: Vec<u64> = plan.units.iter().filter_map(|u| u.origin.instance).map(|i| i.0).collect();
    instances.sort_unstable();
    assert_eq!(instances, [first, second, third]);
    assert!(plan.units.iter().all(|u| u.origin.node == art && u.origin.part == Part::Fill(0)), "{:?}", plan.units);
}

/// An opened design's runs are literal units, sewn as their points are: no parameters.
#[test]
fn an_opened_designs_runs_are_literal_units() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#cc2222", 20.0, 200.0, 40.0, 30.0);
    let pes = export(&mut s, Format::Pes);
    s.execute("document.open", &json!({"name": "design.pes", "dataBase64": vectorcraft_format::base64_encode(&pes)})).unwrap();
    let mut runs = 0;
    s.doc().unwrap().doc.walk(|n| runs += usize::from(n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some())));
    let plan = plan_of(&s);
    assert_eq!(plan.units.len(), runs, "a unit per run of stitches");
    assert!(plan.units.iter().all(|u| u.origin.part == Part::Literal && u.params.is_none() && u.columns.is_none()));
    assert_eq!(plan.blocks.len(), 2, "the file's two colour blocks");
}

/// Objects of one thread that nothing lies between share its block, in document order: the second
/// one's stitches start with the cut and the jump that take the needle to it.
#[test]
fn units_of_one_thread_share_its_block_in_document_order() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 40.0);
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 40.0);
    let c = rect(&mut s, "#cc2222", 20.0, 200.0, 40.0, 40.0);
    let plan = plan_of(&s);
    check(&plan);
    assert_eq!(plan.blocks.len(), 2);
    let red: Vec<NodeId> = plan.units[plan.blocks[0].units.clone()].iter().map(|u| u.origin.selected()).collect();
    assert_eq!(red, [NodeId(a), NodeId(c)]);
    let second = &plan.units[plan.blocks[0].units.start + 1];
    let lead: Vec<Command> = plan.blocks[0].stitches[second.stitches.clone()].iter().take(2).map(|s| s.command).collect();
    assert_eq!(lead, [Command::Trim, Command::Jump], "a move longer than the trim length");
}

/// An object that asks for a trim after it ends its unit with the cut.
#[test]
fn a_unit_that_asks_for_a_trim_ends_with_it() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 15.0, 15.0);
    rect(&mut s, "#cc2222", 40.0, 20.0, 15.0, 15.0);
    let plan = plan_of(&s);
    let last = |plan: &DesignPlan| plan.blocks[0].stitches[plan.units[0].stitches.end - 1].command;
    assert_ne!(last(&plan), Command::Trim, "near each other, no cut");
    s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [a]})).unwrap();
    let plan = plan_of(&s);
    check(&plan);
    assert_eq!(last(&plan), Command::Trim);
}

/// Stitches are numbered from 1 in sewing order, block after block, every needle point, jump and
/// cut taking a number: each number finds its block, its index and its unit, and nothing else
/// has one.
#[test]
fn stitches_are_numbered_from_one_in_sewing_order() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#cc2222", 20.0, 200.0, 40.0, 30.0);
    let plan = plan_of(&s);
    let total = plan.len();
    assert_eq!(plan.blocks.len(), 2, "red, then blue");
    assert!(total > 100, "{total} stitches");
    let mut n = 0;
    for (k, b) in plan.blocks.iter().enumerate() {
        for i in 0..b.stitches.len() {
            n += 1;
            assert_eq!(plan.number(k, i), Some(n));
            assert_eq!(plan.locate(n), Some((k, i)));
            let u = plan.unit_at(n).unwrap();
            assert!(plan.units[u].block == k && plan.units[u].stitches.contains(&i) && plan.numbers(u).unwrap().contains(&n));
        }
        assert_eq!(plan.number(k, b.stitches.len()), None);
    }
    assert_eq!(n, total);
    assert_eq!((plan.number(0, 0), plan.locate(FIRST_NUMBER)), (Some(FIRST_NUMBER), Some((0, 0))));
    for none in [0, total + 1] {
        assert_eq!((plan.locate(none), plan.unit_at(none)), (None, None), "{none}");
    }
    assert_eq!((plan.number(2, 0), plan.numbers(plan.units.len())), (None, None));
}

/// The export writes the plan: each format's file is the plan's pattern, written.
#[test]
fn the_export_writes_the_plan() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    letter_t(&mut s, "#22aa44", 20.0, 120.0);
    let plan = plan_of(&s);
    assert_eq!(plan.name, s.doc().unwrap().doc.title);
    for format in Format::ALL {
        assert_eq!(export(&mut s, format), format.write(&plan.to_pattern()).unwrap(), "{}", format.id());
    }
}

/// The document's native file as bytes (`document.serialize`).
fn native(s: &mut Session) -> Vec<u8> {
    let saved = s.execute("document.serialize", &json!({})).unwrap();
    match saved["dataBase64"].as_str() {
        Some(b) => vectorcraft_format::base64_decode(b).unwrap(),
        None => saved["text"].as_str().unwrap().as_bytes().to_vec(),
    }
}

/// The settings of a document that sets none: the palette unset, Janome's by default.
fn unset() -> Value {
    json!({"settings": {"palette": null, "start": null, "label": null}, "resolved": {"palette": {"value": "jef", "source": "default"}}})
}

/// `embroidery.setup` sets the document's thread palette in one undo step and unsets it with null;
/// setting what is set already, or passing its reply's settings back, changes nothing and takes no
/// step; a palette it doesn't know, a setting, or the resolved values are refused.
/// `embroidery.settings` reads them without a step.
#[test]
fn the_document_sets_its_thread_palette() {
    let mut s = blank();
    let steps = |s: &Session| s.doc().unwrap().history.undo.len();
    let before = steps(&s);
    assert_eq!(s.execute("embroidery.settings", &json!({})).unwrap(), unset());
    assert_eq!(s.execute("embroidery.setup", &json!({})).unwrap(), unset());
    let r = s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    assert_eq!(
        r,
        json!({"settings": {"palette": "pec", "start": null, "label": null}, "resolved": {"palette": {"value": "pec", "source": "document"}}})
    );
    assert_eq!(steps(&s), before + 1);
    assert_eq!(s.execute("embroidery.setup", &json!({"settings": r["settings"]})).unwrap(), r);
    assert_eq!(s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap(), r);
    assert_eq!(s.execute("embroidery.settings", &json!({})).unwrap(), r);
    assert_eq!(steps(&s), before + 1, "no change, no step");
    for bad in [
        json!({"palette": "madeira"}),
        json!({"palette": 5}),
        json!({"palette": ["pec"]}),
        json!({"palete": "pec"}),
        json!([1]),
        // The reply's resolved values are only reported; the same setting twice must agree.
        r.clone(),
        json!({"palette": "jef", "settings": {"palette": "pec"}}),
        json!({"settings": "pec"}),
    ] {
        assert!(s.execute("embroidery.setup", &bad).is_err(), "{bad}");
    }
    // The params check every frontend runs refuses it first; the command says why.
    assert!(s.execute("embroidery.setup", &r).unwrap_err().to_string().contains("`resolved`"));
    assert!(super::setup::setup(&mut s, &r).unwrap_err().to_string().contains("`resolved` is read only"));
    assert_eq!(steps(&s), before + 1);
    assert_eq!(s.execute("embroidery.setup", &json!({"palette": null})).unwrap(), unset());
    assert!(s.doc().unwrap().doc.embroidery.is_none(), "nothing set, nothing kept");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("embroidery.settings", &json!({})).unwrap()["settings"]["palette"], "pec");
    // The getter is a query: it is not recorded as an action.
    assert!(!crate::cmd::find_command("embroidery.settings").unwrap().journal);
    // The document's palettes are the stitch planner's, by name.
    let known: Vec<Option<Palette>> = ThreadPalette::KNOWN.iter().map(|p| p.id().and_then(Palette::from_id)).collect();
    assert_eq!(known, Palette::ALL.map(Some));
}

/// `embroidery.setup` sets where the machine starts the needle (document points) in one undo step
/// and unsets it with null; the DST export starts there, its header measuring the design from it;
/// a start that isn't two numbers within a document's coordinates is refused.
#[test]
fn the_document_sets_its_start_point() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let dst = |s: &mut Session| {
        let r = s.execute("document.export", &json!({"format": "dst"})).unwrap();
        vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()
    };
    let field = |bytes: &[u8], name: &str| -> i32 {
        let text = String::from_utf8_lossy(&bytes[..512]).to_string();
        let at = text.find(name).unwrap() + name.len();
        text[at..at + 5].trim().parse().unwrap()
    };
    let corner = dst(&mut s);
    assert_eq!((field(&corner, "-X:"), field(&corner, "-Y:")), (0, 0), "from the corner");
    // The rectangle's centre (its stroke reaches a little past it).
    let r = s.execute("embroidery.setup", &json!({"start": [50.0, 40.0]})).unwrap();
    assert_eq!(r["settings"]["start"], json!([50.0, 40.0]));
    let centred = dst(&mut s);
    let (right, left, up, down) = (field(&centred, "+X:"), field(&centred, "-X:"), field(&centred, "+Y:"), field(&centred, "-Y:"));
    assert!((right - left).abs() <= 1 && (up - down).abs() <= 1 && left > 100, "{right} {left} {up} {down}");
    // The same size, to a tenth: each end rounds to the grid from the start, not the width.
    assert!((right + left - field(&corner, "+X:")).abs() <= 1 && (up + down - field(&corner, "+Y:")).abs() <= 1, "the same size");
    let read = Format::Dst.read(&centred).unwrap();
    let start = read.start.unwrap();
    let b = read.bounds().unwrap();
    assert!((start.x - b.center().x).abs() < 0.2 && (start.y - b.center().y).abs() < 0.2, "{start:?} {b:?}");
    for bad in [json!({"start": [1.0]}), json!({"start": "centre"}), json!({"start": [1.0, 1e9]}), json!({"start": [1.0, null]})] {
        assert!(s.execute("embroidery.setup", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.execute("embroidery.setup", &json!({"start": null, "label": null})).unwrap(), unset());
    assert_eq!(dst(&mut s), corner);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(dst(&mut s), centred);
}

/// Settings with nothing in them, as a file may hold them, are as good as none: asking for the
/// settings, or unsetting the palette, takes no undo step and leaves the document unchanged.
#[test]
fn empty_settings_in_a_file_are_none() {
    let mut file: serde_json::Value = serde_json::from_slice(&native(&mut blank())).unwrap();
    for empty in [json!({}), json!({"palette": null})] {
        file["document"]["embroidery"] = empty.clone();
        let data = vectorcraft_format::base64_encode(&serde_json::to_vec(&file).unwrap());
        let mut s = Session::new();
        s.execute("document.open", &json!({"name": "empty.vectorcraft", "dataBase64": data})).unwrap();
        let steps = s.doc().unwrap().history.undo.len();
        for p in [json!({}), json!({"palette": null})] {
            assert_eq!(s.execute("embroidery.setup", &p).unwrap(), unset(), "{empty}: {p}");
        }
        let st = s.doc().unwrap();
        assert_eq!(st.history.undo.len(), steps, "{empty}: no step");
        assert!(!st.is_dirty(), "{empty}: unchanged");
    }
    // A frontend's params check refuses another key first; the command says it too.
    let refused = super::setup::setup(&mut blank(), &json!({"fabric": "knit"})).unwrap_err().to_string();
    assert!(refused.contains("unknown setting `fabric`: the settings are palette, start and label"), "{refused}");
}

/// A file whose embroidery settings a newer version wrote in another shape than an object opens:
/// its palette is unset, saving keeps the settings as they were, and setting a palette replaces
/// them.
#[test]
fn a_newer_versions_settings_of_another_shape_open() {
    let saved = native(&mut blank());
    for newer in [json!(5), json!("pec"), json!([{"palette": "pec"}])] {
        let mut file: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        file["document"]["embroidery"] = newer.clone();
        let data = vectorcraft_format::base64_encode(&serde_json::to_vec(&file).unwrap());
        let mut s = Session::new();
        s.execute("document.open", &json!({"name": "newer.vectorcraft", "dataBase64": data})).unwrap_or_else(|e| panic!("{newer}: {e}"));
        assert_eq!(s.execute("embroidery.settings", &json!({})).unwrap(), unset(), "{newer}");
        assert!(!s.doc().unwrap().is_dirty(), "{newer}: unchanged");
        let kept: serde_json::Value = serde_json::from_slice(&native(&mut s)).unwrap();
        assert_eq!(kept["document"]["embroidery"], newer);
        s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
        let replaced: serde_json::Value = serde_json::from_slice(&native(&mut s)).unwrap();
        assert_eq!(replaced["document"]["embroidery"], json!({"palette": "pec"}), "{newer}");
    }
}

/// The palette is saved with the document. A file a newer version saved keeps what this one
/// doesn't know when saved again: a setting it doesn't read, and a palette it doesn't know, which
/// it reports as it is and matches as unset.
#[test]
fn the_thread_palette_is_saved_and_a_newer_versions_settings_are_kept() {
    let mut s = blank();
    s.execute("embroidery.setup", &json!({"palette": "jef"})).unwrap();
    let saved = native(&mut s);
    let mut again = Session::new();
    again.execute("document.open", &json!({"name": "threads.vectorcraft", "dataBase64": vectorcraft_format::base64_encode(&saved)})).unwrap();
    assert_eq!(again.execute("embroidery.settings", &json!({})).unwrap()["settings"]["palette"], "jef");
    // As a newer version might write it.
    let mut file: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    let newer = json!({"palette": {"chart": "madeira", "line": "polyneon"}, "fabric": {"preset": "knit", "pull": 0.35}});
    file["document"]["embroidery"] = newer.clone();
    let bytes = serde_json::to_vec(&file).unwrap();
    let mut s = Session::new();
    s.execute("document.open", &json!({"name": "newer.vectorcraft", "dataBase64": vectorcraft_format::base64_encode(&bytes)})).unwrap();
    let r = s.execute("embroidery.settings", &json!({})).unwrap();
    assert_eq!(
        r,
        json!({"settings": {"palette": newer["palette"], "start": null, "label": null}, "resolved": {"palette": {"value": "jef", "source": "default"}}})
    );
    // Its own settings passed back change nothing; another palette replaces it, keeping the fabric.
    s.execute("embroidery.setup", &json!({"settings": r["settings"]})).unwrap();
    let kept: serde_json::Value = serde_json::from_slice(&native(&mut s)).unwrap();
    assert_eq!(kept["document"]["embroidery"], newer);
    // Its threads are Janome's, and the plan says why.
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    let threads = s.execute("embroidery.threads", &json!({})).unwrap();
    assert_eq!(threads["palette"], "jef");
    assert!(threads["blocks"].as_array().unwrap().iter().all(|b| b["thread"]["palette"] == "jef"));
    let warnings = threads["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("not one this version knows"))), "{warnings:?}");
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    let kept: serde_json::Value = serde_json::from_slice(&native(&mut s)).unwrap();
    assert_eq!(kept["document"]["embroidery"], json!({"palette": "pec", "fabric": newer["fabric"]}));
    // The palette is known now; what the newer version's fabric says is still not read, and the
    // plan says so.
    let warnings = s.execute("embroidery.threads", &json!({})).unwrap()["warnings"].clone();
    assert_eq!(warnings, json!([super::resolve::DOCUMENT_SETTINGS_UNREAD]));
}

/// A document that sets no palette is sewn in Janome's threads, the default, and its PES export
/// still names Brother's: the threads nearest the same art.
#[test]
fn a_document_without_a_palette_is_sewn_in_janomes_threads() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 30.0);
    let plan = plan_of(&s);
    assert!(plan.blocks.iter().all(|b| b.thread.palette() == Palette::Jef), "{:?}", plan.blocks.iter().map(|b| &b.thread).collect::<Vec<_>>());
    let unset = export(&mut s, Format::Pes);
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    assert_eq!(export(&mut s, Format::Pes), unset, "the PES export matches to Brother's palette either way");
}

/// A design opened from a PES or a JEF file is in that format's palette, so its threads are the
/// ones the file names: each block's art is its thread's own colour, none off and none flagged.
/// One opened from DST or EXP, which name no threads, sets none. Opening is no edit.
#[test]
fn an_opened_design_is_in_its_formats_palette() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#4080ff", 100.0, 20.0, 40.0, 30.0);
    for format in Format::ALL {
        let data = vectorcraft_format::base64_encode(&export(&mut s, format));
        let mut opened = Session::new();
        opened.execute("document.open", &json!({"name": format!("design.{}", format.id()), "dataBase64": data})).unwrap();
        assert!(!opened.doc().unwrap().is_dirty(), "{format:?}");
        let palette = format.palette().map(Palette::id);
        assert_eq!(opened.execute("embroidery.settings", &json!({})).unwrap()["settings"]["palette"], json!(palette), "{format:?}");
        let threads = opened.execute("embroidery.threads", &json!({})).unwrap();
        let blocks = threads["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "{format:?}: {blocks:?}");
        if palette.is_some() {
            assert_eq!(threads["palette"], json!(palette));
            assert!(blocks.iter().all(|b| b["thread"]["deltaE"] == json!(0.0) && b["thread"]["mismatch"] == json!(false)), "{format:?}: {blocks:?}");
        }
    }
}

/// Brother's thread 38, Peacock Blue, as a colour.
const PEACOCK_BLUE: &str = "#134a46";

/// Each block is sewn in a thread of the document's palette, the one nearest the colour of its
/// art, with their difference recorded; art painted with a swatch of the palette's thread library
/// has that thread exactly. The PES and JEF exports name the plan's threads.
#[test]
fn blocks_are_sewn_in_threads_of_the_documents_palette() {
    let mut s = blank();
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    let swatched = rect(&mut s, "#000000", 100.0, 20.0, 40.0, 30.0);
    s.execute("select.set", &json!({"ids": [swatched]})).unwrap();
    s.execute("swatch.library.add", &json!({"library": "brother-threads-pec", "names": ["PEC 38 Peacock Blue"], "apply": "fill"})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    let plan = plan_of(&s);
    let [red, blue] = &plan.blocks[..] else { panic!("two blocks: {:?}", plan.blocks.len()) };
    assert_eq!((red.thread.palette(), red.thread.art), (Palette::Pec, [0xcc, 0x22, 0x22]));
    assert_eq!(red.thread.index(), Palette::Pec.nearest([0xcc, 0x22, 0x22], &[]).index);
    assert!(red.thread.delta_e > 0.0);
    assert_eq!((blue.thread.index(), blue.thread.name(), blue.thread.delta_e), (38, "Peacock Blue", 0.0));
    assert_eq!(blue.thread.art, blue.thread.color());
    assert!(!blue.thread.mismatch());
    let names = |plan: &DesignPlan| plan.blocks.iter().map(|b| b.thread.name().to_string()).collect::<Vec<_>>();
    let named = |bytes: &[u8], format: Format| format.read(bytes).unwrap().blocks.iter().map(|b| b.thread.name.clone()).collect::<Vec<_>>();
    assert_eq!(named(&export(&mut s, Format::Pes), Format::Pes), names(&plan));
    // In Janome's palette, the plan's threads are Janome's, and the JEF export names them.
    s.execute("embroidery.setup", &json!({"palette": "jef"})).unwrap();
    let plan = plan_of(&s);
    assert!(plan.blocks.iter().all(|b| b.thread.palette() == Palette::Jef), "{:?}", plan.blocks.iter().map(|b| &b.thread).collect::<Vec<_>>());
    assert_eq!(named(&export(&mut s, Format::Jef), Format::Jef), names(&plan));
}

/// A swatch of the other palette's thread library is matched like any colour in the document's
/// palette, while the export in that library's own format still names that thread exactly: Brother's
/// Peacock Blue in Janome's palette (the default), Janome's Sola Blue in Brother's.
#[test]
fn a_swatch_of_the_other_palette_is_matched_and_its_own_format_names_it() {
    for (palette, library, swatch, theirs, number) in [
        (Palette::Jef, "brother-threads-pec", "PEC 38 Peacock Blue", Format::Pes, 38),
        (Palette::Pec, "janome-threads-jef", "JEF 61 Sola Blue", Format::Jef, 61),
    ] {
        let mut s = blank();
        s.execute("embroidery.setup", &json!({"palette": palette.id()})).unwrap();
        let swatched = rect(&mut s, "#000000", 100.0, 20.0, 40.0, 30.0);
        s.execute("select.set", &json!({"ids": [swatched]})).unwrap();
        s.execute("swatch.library.add", &json!({"library": library, "names": [swatch], "apply": "fill"})).unwrap();
        s.execute("select.none", &json!({})).unwrap();
        let own = theirs.palette().and_then(|p| p.get(number)).map(|(rgb, _)| rgb).unwrap();
        let plan = plan_of(&s);
        let [block] = &plan.blocks[..] else { panic!("{swatch}: one block, not {}", plan.blocks.len()) };
        assert_eq!((block.thread.palette(), block.thread.art), (palette, own), "{swatch}");
        assert_eq!(block.thread.index(), palette.nearest(own, &[]).index, "{swatch}");
        assert!(block.thread.delta_e > 0.0, "{swatch}: no thread of the document's palette has its colour");
        let written = theirs.read(&export(&mut s, theirs)).unwrap();
        assert_eq!(written.blocks[0].thread, theirs.palette().and_then(|p| p.thread(number)).unwrap(), "{swatch}");
    }
}

/// `embroidery.threads` lists each block's art colour, its thread and their difference, flagging
/// a thread past the threshold, as the plan has them; it takes the export's options and refuses
/// bad ones.
#[test]
fn the_threads_command_lists_each_blocks_thread() {
    let mut s = blank();
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    rect(&mut s, PEACOCK_BLUE, 100.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#4080ff", 200.0, 20.0, 40.0, 30.0);
    rect(&mut s, "#cc2222", 20.0, 200.0, 40.0, 30.0);
    let r = s.execute("embroidery.threads", &json!({})).unwrap();
    let plan = plan_of(&s);
    assert_eq!((&r["palette"], &r["threshold"]), (&json!("pec"), &json!(MISMATCH_DELTA_E)));
    let blocks = r["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 3, "red, blue, light blue: {blocks:?}");
    for (k, (b, p)) in blocks.iter().zip(&plan.blocks).enumerate() {
        assert_eq!((&b["block"], &b["unitCount"]), (&json!(k + 1), &json!(p.units.len())));
        // The one thread object every command gives.
        assert_eq!(b["thread"], super::stats::thread_json(&p.thread));
        assert_eq!(
            (&b["thread"]["palette"], &b["thread"]["index"], &b["thread"]["name"]),
            (&json!("pec"), &json!(p.thread.index()), &json!(p.thread.name()))
        );
        let t = &b["thread"];
        assert!((t["deltaE"].as_f64().unwrap() - p.thread.delta_e).abs() <= 0.005, "{b}");
        assert_eq!((&t["deltaE"], &t["mismatch"]), (&json!(p.thread.shown_delta_e()), &json!(p.thread.mismatch())));
    }
    assert_eq!((&blocks[0]["thread"]["art"], &blocks[0]["unitCount"]), (&json!("#cc2222"), &json!(2)), "both red squares");
    let peacock = &blocks[1]["thread"];
    assert_eq!((&peacock["index"], &peacock["color"], &peacock["deltaE"]), (&json!(38), &json!(PEACOCK_BLUE), &json!(0.0)));
    assert_eq!((&peacock["mismatch"], &blocks[2]["thread"]["mismatch"]), (&json!(false), &json!(true)), "no Brother thread is near #4080ff");
    let red = blocks[0]["thread"]["deltaE"].as_f64().unwrap();
    assert!(red > 5.0 && blocks[0]["thread"]["mismatch"] == json!(false), "Brother's Red is {red} from #cc2222: off, not unlike");
    // The export's options; bad ones refused.
    for bad in [json!({"stitchStyle": "cross"}), json!({"density": "dense"}), json!({"pullCompensation": [1, 2, 3]})] {
        assert!(s.execute("embroidery.threads", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("embroidery.threads", &json!({"stitchStyle": "running"})).is_ok());
}

/// A thread is flagged by its difference as a list shows it: one shown as the threshold itself is
/// not flagged, one shown past it is.
#[test]
fn a_mismatch_is_flagged_by_the_difference_shown() {
    let thread = |delta_e| BlockThread::new([0; 3], Palette::Pec, vectorcraft_stitch::thread::Match { index: 1, delta_e });
    let step = 1.0 / DELTA_E_HUNDREDTHS;
    let (at, past) = (thread(MISMATCH_DELTA_E + 0.4 * step), thread(MISMATCH_DELTA_E + 0.6 * step));
    assert_eq!((at.shown_delta_e(), at.mismatch()), (MISMATCH_DELTA_E, false));
    assert_eq!((past.shown_delta_e(), past.mismatch()), (MISMATCH_DELTA_E + step, true));
}

/// `embroidery.threads` takes the export's embroidery options, the stretch of blocks it lists, and
/// no other key: another format's option, or one misspelt, is refused by the params check every
/// frontend runs, not ignored.
#[test]
fn the_threads_command_takes_only_the_embroidery_options() {
    let spec = crate::cmd::find_command("embroidery.threads").unwrap();
    let named = crate::cmd::params::doc_keys(spec.params).unwrap();
    let options: Vec<&str> = crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).collect();
    let mut names: Vec<&str> = super::vocabulary::options().flat_map(|n| std::iter::once(n.name).chain(n.earlier)).collect();
    // And the stretch of blocks it lists, and whether the selection alone.
    names.extend(["offset", "limit", "selectedOnly"]);
    // The export's options, under the names the options take (each earlier name after its own).
    assert_eq!(super::vocabulary::options().map(|n| n.name).collect::<Vec<_>>(), options);
    assert_eq!((named.keys, named.more.len()), (names, 0));
    for (given, why) in [
        (json!({"densitty": 0.4}), "did you mean `density`?"),
        (json!({"format": "pes"}), "unknown parameter `format`"),
        (json!({"speed": 800}), "unknown parameter `speed`"),
    ] {
        let e = crate::check_params("embroidery.threads", &given).unwrap_err().to_string();
        assert!(e.contains(why), "{given}: {e}");
    }
}

/// Art far outside the document's range (a malformed file's curve, or a symbol scaled enormously)
/// is left out with a warning, quickly, instead of being flattened into more points than memory
/// holds; the rest of the design is planned.
#[test]
fn art_out_of_range_is_left_out_with_a_warning() {
    use std::sync::Arc;

    use vectorcraft_color::{Color, Paint};
    use vectorcraft_doc::{Appearance, Document, Node, NodeKind, Symbol};
    use vectorcraft_geom::{Affine, BezPath, PathData};

    let mut doc = Document::new(300.0, 300.0);
    let layer = doc.layers.first().map(|l| l.id);
    let red = Appearance::basic(Paint::solid(Color::rgb8(200, 30, 30)), Paint::None, 0.0);
    let mut far = BezPath::new();
    far.move_to((0.0, 0.0));
    far.curve_to((1e16, 0.0), (1e16, 1e16), (0.0, 10.0));
    far.close_path();
    let id = doc.alloc_id();
    doc.insert(layer, usize::MAX, Node::path(id, PathData::from_bezpath(&far), red.clone())).unwrap();
    // A symbol of a small square, placed scaled a hundred trillion times.
    let mut square = BezPath::new();
    square.move_to((0.0, 0.0));
    square.line_to((10.0, 0.0));
    square.line_to((10.0, 10.0));
    square.close_path();
    let art = Node::path(doc.alloc_id(), PathData::from_bezpath(&square), red.clone());
    doc.symbols.push(Symbol { name: "huge".into(), art: Arc::new(art) });
    let id = doc.alloc_id();
    doc.insert(layer, usize::MAX, Node::new(id, NodeKind::SymbolInstance { symbol: "huge".into(), xf: Affine::scale(1e14) })).unwrap();
    // And a square that is in range.
    let id = doc.alloc_id();
    let mut near = BezPath::new();
    near.move_to((20.0, 20.0));
    near.line_to((60.0, 20.0));
    near.line_to((60.0, 60.0));
    near.close_path();
    doc.insert(layer, usize::MAX, Node::path(id, PathData::from_bezpath(&near), red)).unwrap();
    let started = std::time::Instant::now();
    let plan = digitize(&doc, &Options::default(), None).unwrap();
    assert!(started.elapsed().as_secs() < 5);
    assert_eq!(plan.units.len(), 1, "only the square in range");
    assert!(plan.warnings.iter().any(|w| w.contains("far outside the document's range")), "{:?}", plan.warnings);
}

/// The read commands plan what the export writes: with `selectedOnly`, the selected objects alone,
/// as `document.export {selectedOnly}` writes them; while an opacity mask is edited, without its
/// editing layer, which no export writes.
#[test]
fn the_read_commands_plan_what_the_export_writes() {
    let mut s = blank();
    let red = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    rect(&mut s, "#2233cc", 150.0, 20.0, 60.0, 40.0);
    let stitches = |s: &mut Session, p: serde_json::Value| s.execute("embroidery.stats", &p).unwrap()["stitches"].clone();
    let exported = |s: &mut Session, p: serde_json::Value| {
        let mut p = p;
        p["format"] = json!("pes");
        let r = s.execute("document.export", &p).unwrap();
        Format::Pes.read(&vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()).unwrap().sewn()
    };
    let whole = stitches(&mut s, json!({}));
    assert_eq!(whole, json!(exported(&mut s, json!({}))));
    // Nothing selected: refused, naming the key.
    let e = s.execute("embroidery.stats", &json!({"selectedOnly": true})).unwrap_err().to_string();
    assert!(e.contains("selectedOnly"), "{e}");
    s.execute("select.set", &json!({"ids": [red]})).unwrap();
    let alone = stitches(&mut s, json!({"selectedOnly": true}));
    assert_eq!(alone, json!(exported(&mut s, json!({"selectedOnly": true}))));
    assert!(alone.as_u64().unwrap() < whole.as_u64().unwrap());
    for command in ["embroidery.threads", "embroidery.sequence", "embroidery.check"] {
        let r = s.execute(command, &json!({"selectedOnly": true})).unwrap();
        let listed = if command == "embroidery.check" { r["rules"].as_array().unwrap().len() } else { r["blocks"].as_array().unwrap().len() };
        assert!(listed > 0, "{command}: {r}");
    }
    assert_eq!(s.execute("embroidery.threads", &json!({"selectedOnly": true})).unwrap()["blockCount"], json!(1));
    // The blue rectangle made the red one's opacity mask, then edited: the editing layer holds a
    // copy of the mask art, which neither the export nor the read commands sew.
    s.execute("select.all", &json!({})).unwrap();
    s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
    let masked = stitches(&mut s, json!({}));
    s.execute("transparency.editOpacityMask", &json!({"id": red})).unwrap();
    assert!(s.doc().unwrap().doc.mask_edit.is_some());
    assert_eq!(stitches(&mut s, json!({})), masked);
    assert_eq!(masked, json!(exported(&mut s, json!({}))));
}

/// An object's embroidery as a newer version might write it (settings this version doesn't know,
/// a cut of another type, a run of another shape) opens; the object is sewn as art, and what this
/// version doesn't read is saved back, also after Embroidery Trim After sets and clears its cut.
#[test]
fn a_newer_versions_object_settings_open_and_are_kept() {
    let mut s = blank();
    let id = rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    let plain = s.execute("embroidery.stats", &json!({})).unwrap()["stitches"].clone();
    let newer = json!({"motif": {"name": "wave", "scale": 1.5}, "trim_after": "auto", "stitches": "later"});
    let mut file: serde_json::Value = serde_json::from_slice(&native(&mut s)).unwrap();
    fn node(v: &mut serde_json::Value, id: u64) -> Option<&mut serde_json::Value> {
        if v.get("id").and_then(serde_json::Value::as_u64) == Some(id) && v.get("kind").is_some() {
            return Some(v);
        }
        match v {
            serde_json::Value::Object(m) => m.values_mut().find_map(|c| node(c, id)),
            serde_json::Value::Array(a) => a.iter_mut().find_map(|c| node(c, id)),
            _ => None,
        }
    }
    node(&mut file, id).unwrap()["embroidery"] = newer.clone();
    let mut s = Session::new();
    let data = vectorcraft_format::base64_encode(&serde_json::to_vec(&file).unwrap());
    s.execute("document.open", &json!({"name": "newer.vectorcraft", "dataBase64": data})).unwrap();
    // Sewn as art, the same as the plain object (`plain`): a run of another shape is no run, and
    // a cut of another type no cut.
    assert_eq!(s.execute("embroidery.stats", &json!({})).unwrap()["stitches"], plain);
    let saved = |s: &mut Session| node(&mut serde_json::from_slice(&native(s)).unwrap(), id).unwrap()["embroidery"].clone();
    assert_eq!(saved(&mut s), newer);
    s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [id]})).unwrap();
    let mut on = newer.clone();
    on["trim_after"] = json!(true);
    assert_eq!(saved(&mut s), on);
    s.execute("embroidery.setTrimAfter", &json!({"on": false, "ids": [id]})).unwrap();
    // Set, the cut is this version's: the value of another type is gone, the rest stays.
    let mut off = newer.clone();
    off.as_object_mut().unwrap().remove("trim_after");
    assert_eq!(saved(&mut s), off, "the box stays: it holds what a newer version wrote");
}

/// Trim After acts on `ids`, else the selection: given `ids` it runs with nothing selected, given
/// neither it is off; it takes no `id`.
#[test]
fn trim_after_takes_ids_in_place_of_the_selection() {
    let mut s = blank();
    let id = rect(&mut s, "#2233cc", 20.0, 20.0, 30.0, 30.0);
    s.execute("select.none", &json!({})).unwrap();
    assert_eq!(s.execute("embroidery.setTrimAfter", &json!({"ids": [id]})).unwrap(), json!({"on": true, "changed": 1}));
    let e = s.execute("embroidery.setTrimAfter", &json!({})).unwrap_err().to_string();
    assert!(e.contains("not available right now: nothing selected"), "{e}");
    let e = s.execute("embroidery.setTrimAfter", &json!({"id": id})).unwrap_err().to_string();
    assert!(e.contains("`ids`"), "{e}");
}

/// A fill whose paint isn't sewn is passed by before any path operation runs on its area: a
/// negative `expand` that would leave nothing of it doesn't warn about art that was never to be
/// sewn.
#[test]
fn an_unpainted_fill_is_passed_by_before_it_is_expanded() {
    let mut s = blank();
    rect(&mut s, "#2233cc", 100.0, 100.0, 60.0, 60.0);
    let small = rect(&mut s, "#cc2222", 20.0, 20.0, 8.0, 8.0);
    let expanded_away = |s: &mut Session| {
        let r = s.execute("document.export", &json!({"format": "dst", "expand": -2})).unwrap();
        r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("negative expand"))
    };
    assert!(expanded_away(&mut s), "a painted 2.8 mm square shrunk by 2 mm all round is left out, and said so");
    s.edit("Unpaint", |d, _| {
        let n = d.node_mut(NodeId(small)).unwrap();
        if let Some(fill) = n.appearance.fill_at_mut(None) {
            fill.paint = vectorcraft_color::Paint::None;
        }
        Ok(())
    })
    .unwrap();
    assert!(!expanded_away(&mut s), "unpainted, it was never to be sewn");
}

/// Objects the planner leaves out are named for what they are: an intertwine, not "live blends,
/// envelopes, meshes and repeats".
#[test]
fn an_intertwine_is_named_when_it_is_left_out() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let b = rect(&mut s, "#2233cc", 50.0, 30.0, 60.0, 40.0);
    s.execute("object.intertwine.make", &json!({"ids": [a, b]})).unwrap();
    let r = s.execute("embroidery.threads", &json!({})).unwrap();
    let warnings: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert!(warnings.iter().any(|w| w.starts_with("intertwined objects are not stitched")), "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("live blends")), "{warnings:?}");
}

/// Trim After on a group whose units join different blocks cuts after its unit sewn last: the one
/// ending the latest block, not the one added last, which may have joined an earlier block of its
/// colour.
#[test]
fn trim_after_a_group_cuts_after_its_unit_sewn_last() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    // The group: a blue square, then a red one, which joins the first red block (sewn first).
    let blue = rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 30.0);
    let red = rect(&mut s, "#cc2222", 200.0, 20.0, 40.0, 30.0);
    s.execute("select.set", &json!({"ids": [blue, red]})).unwrap();
    let group = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    let before = plan_of(&s);
    assert_eq!(before.blocks.len(), 2, "red (with the group's red), then blue");
    let ends_cut = |p: &DesignPlan| p.blocks.iter().map(|b| b.stitches.last().is_some_and(|s| s.command == Command::Trim)).collect::<Vec<_>>();
    assert_eq!(ends_cut(&before), [false, false]);
    s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [group]})).unwrap();
    // Cut after the blue square, sewn last; not after the red one, sewn before it.
    assert_eq!(ends_cut(&plan_of(&s)), [false, true]);
}

/// Only what would print is sewn: of a document holding a guide, a hidden object, an object whose
/// only fill is hidden, a non-printing layer and a template layer, besides one rectangle, the
/// export sews the rectangle alone, the sewing order lists one unit, and the plan's progress counts
/// that one unit as all there is.
#[test]
fn only_what_would_print_is_sewn() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use vectorcraft_doc::cancel::{self, CancelToken, ProgressSink};

    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    let guide = rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 30.0);
    s.execute("select.set", &json!({"ids": [guide]})).unwrap();
    s.execute("view.guides.make", &json!({})).unwrap();
    let hidden = rect(&mut s, "#22aa44", 160.0, 20.0, 40.0, 30.0);
    s.execute("object.hide", &json!({"ids": [hidden]})).unwrap();
    let unfilled = rect(&mut s, "#aa22aa", 20.0, 100.0, 40.0, 30.0);
    s.execute("appearance.setItem", &json!({"ids": [unfilled], "index": 0, "visible": false})).unwrap();
    for props in [json!({"printable": false}), json!({"template": true})] {
        let layer = s.execute("layer.new", &json!({})).unwrap()["id"].as_u64().unwrap();
        rect(&mut s, "#22aaaa", 100.0, 100.0, 40.0, 30.0);
        let mut props = props;
        props["ids"] = json!([layer]);
        s.execute("layer.setProps", &props).unwrap();
    }
    let r = s.execute("document.export", &json!({"format": "dst"})).unwrap();
    let file = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    let p = Format::Dst.read(&file).unwrap();
    assert_eq!(p.blocks.len(), 1, "the rectangle alone");
    let b = p.bounds().unwrap();
    // 40 × 30 pt is 14.1 × 10.6 mm; pull compensation reaches a little past each side.
    assert!((b.width() - pt_to_mm(40.0)).abs() < 1.0 && (b.height() - pt_to_mm(30.0)).abs() < 1.0, "{b:?}");
    assert_eq!(s.execute("embroidery.sequence", &json!({})).unwrap()["unitCount"], json!(1));
    let steps = Rc::new(RefCell::new(Vec::new()));
    let sink: ProgressSink = {
        let steps = steps.clone();
        Rc::new(move |done, total, _| steps.borrow_mut().push((done, total)))
    };
    let doc = s.doc().unwrap().doc.clone();
    let plan = cancel::scope(&CancelToken::new(), Some(sink), || digitize(&doc, &Options::default(), None)).unwrap();
    assert_eq!((plan.units.len(), steps.borrow().clone()), (1, vec![(1, 1)]));
}
