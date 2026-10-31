//! Manual stitches: what the plan records of each unit to freeze it (its own stitches, the needle
//! point it shares with the unit before, its place in walk order).

use serde_json::{Value, json};
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::needles;
use super::plan::{Choice, DesignPlan, Part};
use super::test_support::{PT_PER_MM, blank, plan, run, unit};
use super::*;

/// A unit's own stitches are what it sews itself: not the move that reaches it (the jump, and the
/// cut before a long one), not the cut its object asks for after it. A unit that starts where the
/// needle already is gets its first needle point back, which the plan left to the unit before.
#[test]
fn a_units_own_stitches_leave_out_the_moves_around_it() {
    let red = [200, 30, 30];
    // The second starts where the first ends; the third far away (a cut, then a jump).
    let p = plan(vec![
        unit(1, Style::Running, red, &[(0.0, 0.0), (2.0, 0.0)], None),
        unit(2, Style::Running, red, &[(2.0, 0.0), (4.0, 0.0)], None),
        unit(3, Style::Running, red, &[(40.0, 0.0), (42.0, 0.0)], None),
    ]);
    assert_eq!(p.blocks.len(), 1);
    let commands = |u: usize| p.own_stitches(u).iter().map(|s| (s.command, s.x)).collect::<Vec<_>>();
    assert_eq!(commands(0), [(Command::Stitch, 0.0), (Command::Stitch, 2.0)]);
    assert!(p.units[1].met, "the second starts where the needle is");
    assert_eq!(commands(1), [(Command::Stitch, 2.0), (Command::Stitch, 4.0)], "its first needle point put back");
    assert!(!p.units[2].met);
    assert_eq!(commands(2), [(Command::Stitch, 40.0), (Command::Stitch, 42.0)], "without the cut and the jump that reach it");
    let range = |u: usize| p.units[u].stitches.clone();
    assert_eq!((range(2).len(), p.units[2].own.len()), (4, 2), "its range holds the cut and the jump too");
}

/// The plan keeps the order the walk added the units in: a unit that joins an earlier block of its
/// colour is sewn before units walked ahead of it. And the cut its object asks for after it is not
/// its own.
#[test]
fn units_keep_their_place_in_walk_order() {
    let mut s = blank();
    let side = 20.0 * PT_PER_MM;
    run(&mut s, "paint.setStroke", json!({"none": true}));
    let mut ids = Vec::new();
    for (color, x, y) in [("#d01818", 20.0, 20.0), ("#1830c8", 20.0, 200.0), ("#d01818", 150.0, 20.0)] {
        run(&mut s, "paint.setFill", json!({ "color": color }));
        ids.push(run(&mut s, "shape.rectangle", json!({"x": x, "y": y, "width": side, "height": side}))["id"].as_u64().unwrap());
        run(&mut s, "select.none", json!({}));
    }
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [ids[0]]}));
    let p = s.embroidery_plan(&Options::default()).unwrap();
    let sewn: Vec<(u64, usize)> = p.units.iter().map(|u| (u.origin.selected().0, u.walked)).collect();
    assert_eq!(sewn, [(ids[0], 0), (ids[2], 2), (ids[1], 1)], "the reds joined, sewn before the blue walked between them");
    let first = &p.units[0];
    let block = &p.blocks[first.block];
    assert_eq!(block.stitches.get(first.own.end).map(|s| s.command), Some(Command::Trim), "the cut asked for after it follows its own stitches");
    assert!(p.own_stitches(0).iter().all(|s: &Stitch| s.command != Command::Trim || s.x != block.stitches[first.own.end].x));
}

/// A path of literal stitches in the document's first layer (or in group `into`), stroked in
/// `color`, through `points` (millimetres), marked as `run` says → its id.
fn literal_path(s: &mut Session, into: Option<u64>, color: [u8; 3], points: &[(f64, f64)], run: vectorcraft_doc::embroidery::StitchRun) -> u64 {
    use vectorcraft_doc::embroidery::Embroidery;
    let mut bp = vectorcraft_geom::BezPath::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        let p = vectorcraft_geom::Point::new(x * PT_PER_MM, y * PT_PER_MM);
        if i == 0 { bp.move_to(p) } else { bp.line_to(p) }
    }
    let mut id = 0;
    s.edit("Add Run", |d, _| {
        let parent = into.map(vectorcraft_doc::NodeId).or_else(|| d.layers.first().map(|l| l.id));
        let node_id = d.alloc_id();
        let paint = vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(color[0], color[1], color[2]));
        let look = vectorcraft_doc::Appearance::basic(vectorcraft_color::Paint::None, paint, 1.0);
        let mut node = vectorcraft_doc::Node::path(node_id, vectorcraft_geom::PathData::from_bezpath(&bp), look);
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(run.clone()), ..Embroidery::default() }));
        d.insert(parent, usize::MAX, node).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        id = node_id.0;
        Ok(())
    })
    .unwrap();
    id
}

/// A red square of `side` millimetres at `x, y` (millimetres), filled, left unselected → its id.
fn red_square(s: &mut Session, x: f64, y: f64, side: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#c81e1e"}));
    let id = run(s, "shape.rectangle", json!({"x": x * PT_PER_MM, "y": y * PT_PER_MM, "width": side * PT_PER_MM, "height": side * PT_PER_MM}))["id"]
        .as_u64()
        .unwrap();
    run(s, "select.none", json!({}));
    id
}

/// A run of manual stitches is placed in the sewing order as art is: it joins the block of a square
/// of its colour sewn before it, the move into it cut when long. A machine file's run never joins
/// art: it keeps a block of its own.
#[test]
fn a_manual_run_joins_a_block_of_its_colour_as_art_does() {
    use vectorcraft_doc::embroidery::StitchRun;
    for (manual, blocks) in [(true, 1), (false, 2)] {
        let mut s = blank();
        let square = red_square(&mut s, 5.0, 5.0, 10.0);
        let line = literal_path(&mut s, None, [200, 30, 30], &[(40.0, 5.0), (42.0, 5.0), (44.0, 5.0)], StitchRun { manual, ..StitchRun::default() });
        let p = s.embroidery_plan(&Options::default()).unwrap();
        assert_eq!(p.blocks.len(), blocks, "manual {manual}");
        let parts: Vec<(u64, Part)> = p.units.iter().map(|u| (u.origin.selected().0, u.origin.part)).collect();
        let part = if manual { Part::Manual } else { Part::Literal };
        assert_eq!(parts, [(square, Part::Fill(0)), (line, part)]);
        if manual {
            let reached = &p.blocks[0].stitches[p.units[1].stitches.clone()];
            assert_eq!(reached.first().map(|s| s.command), Some(Command::Trim), "a move longer than the trim length is cut");
        }
    }
}

/// A run of manual stitches inside a clip group is sewn whole: its stitches were cut when they were
/// planned, and a fill's edge lies on the cut, which a second cut would chop. A file's run is cut to
/// the clipping path, as its line is drawn.
#[test]
fn a_manual_run_in_a_clip_group_is_not_cut_again() {
    use vectorcraft_doc::embroidery::StitchRun;
    for (manual, all) in [(true, true), (false, false)] {
        let mut s = blank();
        let points: Vec<(f64, f64)> = (0..20).map(|i| (5.0 + f64::from(i) * 2.0, 20.0)).collect();
        let line = literal_path(&mut s, None, [200, 30, 30], &points, StitchRun { manual, ..StitchRun::default() });
        run(&mut s, "paint.setFill", json!({"none": true}));
        run(&mut s, "paint.setStroke", json!({"none": true}));
        let clip = run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 20.0 * PT_PER_MM, "height": 40.0 * PT_PER_MM}))["id"].clone();
        run(&mut s, "select.set", json!({"ids": [line, clip]}));
        run(&mut s, "object.clippingMask.make", json!({}));
        let p = s.embroidery_plan(&Options::default()).unwrap();
        let needle: usize = p.blocks.iter().map(|b| b.needle_points()).sum();
        assert_eq!(needle == points.len(), all, "manual {manual}: {needle} needle points sewn");
    }
}

/// The exact millimetres a manual run keeps are sewn where its document points can't give them
/// back, while the needle point lies where it was stored: moved with its object, the run sews where
/// it lies now; moved back, as it did (a nudge right then left changes nothing sewn).
#[test]
fn a_manual_runs_exact_millimetres_hold_while_its_points_stay() {
    use vectorcraft_doc::embroidery::{Exact, StitchRun};
    use vectorcraft_stitch::{MM_PER_PT, mm_to_pt};
    // An odd quarter of a millimetre (exactly halfway between two tenths) its document point can't
    // give back.
    let lost = (1..4000).map(|i| f64::from(2 * i + 1) * 0.25).find(|&m| mm_to_pt(m) * MM_PER_PT != m).unwrap();
    let mut s = blank();
    let exact = vec![Exact { subpath: 0, anchor: 0, x: Some(lost), y: None }];
    let id = literal_path(&mut s, None, [200, 30, 30], &[(lost, 5.0), (lost + 2.0, 5.0)], StitchRun { manual: true, exact, ..StitchRun::default() });
    // The path stores the document point the convert would: the millimetres divided.
    s.edit("Store", |d, _| {
        if let Some(n) = d.node_mut(vectorcraft_doc::NodeId(id))
            && let Some(path) = n.path_data_mut()
        {
            path.subpaths[0].anchors[0] = vectorcraft_geom::Anchor::corner(vectorcraft_geom::Point::new(mm_to_pt(lost), mm_to_pt(5.0)));
        }
        Ok(())
    })
    .unwrap();
    let first = |s: &mut Session| s.embroidery_plan(&Options::default()).unwrap().blocks[0].stitches[0].x;
    assert_eq!(first(&mut s), lost, "the exact millimetres");
    run(&mut s, "object.move", json!({"ids": [id], "dx": 10, "dy": 0}));
    assert!((first(&mut s) - (lost + 10.0 * MM_PER_PT)).abs() < 1e-6, "moved: where it lies now");
    run(&mut s, "object.move", json!({"ids": [id], "dx": -10, "dy": 0}));
    assert_eq!(first(&mut s), lost, "moved back: the exact millimetres again");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(first(&mut s), lost, "undone: back");
}

/// Each machine format's export of the active document.
fn exports(s: &mut Session) -> Vec<Vec<u8>> {
    vectorcraft_stitch::format::Format::ALL
        .iter()
        .map(|f| vectorcraft_format::base64_decode(run(s, "document.export", json!({"format": f.id()}))["dataBase64"].as_str().unwrap()).unwrap())
        .collect()
}

/// Converting an object freezes it in place: the export writes the same bytes in every format, the
/// object becomes a group keeping its id and name, holding a run of manual stitches for each of its
/// units (a square filled and stroked: two), each in its thread's art colour, keeping the object as
/// it was, with nothing edited yet (its fingerprint), and Trim After and Stop After on the group.
#[test]
fn converting_freezes_an_object_in_place() {
    use vectorcraft_doc::embroidery::Embroidery;
    let mut s = blank();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    let [_, blue, red] = vectorcraft_testkit::embroidery::three_threads(&mut run_fx, false);
    run(&mut s, "paint.setStroke", json!({"color": "#1830c8", "ids": [red]}));
    run(&mut s, "stroke.set", json!({"weight": 4.0 * PT_PER_MM, "ids": [red]}));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [red]}));
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [red]}));
    run(&mut s, "embroidery.set", json!({"ids": [red], "part": "fill", "angle": 30}));
    let before = exports(&mut s);
    let saved = serde_json::to_value(s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(red)).unwrap()).unwrap();
    let r = run(&mut s, "embroidery.convertToManual", json!({"ids": [red, blue]}));
    assert_eq!((r["convertedCount"].clone(), r["skippedCount"].clone()), (json!(2), json!(0)), "{r}");
    assert_eq!(exports(&mut s), before, "the same bytes in every format");
    let doc = s.doc().unwrap().doc.clone();
    let group = doc.node(vectorcraft_doc::NodeId(red)).unwrap();
    assert!(matches!(group.kind, vectorcraft_doc::NodeKind::Group { .. }), "the object is a group");
    let e: &Embroidery = group.extras().embroidery.as_deref().unwrap();
    let original = e.original.as_ref().unwrap();
    assert_eq!((e.trim_after, e.stop_after, *original.node == saved), (true, true, true));
    assert!(original.fingerprint.is_some() && original.frame == vectorcraft_geom::Affine::IDENTITY);
    let runs: Vec<_> = group.children().unwrap().iter().map(|c| c.extras().embroidery.as_deref().and_then(|e| e.stitches.clone()).unwrap()).collect();
    assert_eq!(runs.len(), 2, "its fill and its stroke");
    assert!(runs.iter().all(|r| r.manual && !r.trim_after && !r.stop_after));
    let plan = s.embroidery_plan(&Options::default()).unwrap();
    assert!(plan.units.iter().filter(|u| u.origin.selected().0 == red).all(|u| u.origin.part == Part::Manual), "placed by the object");
}

/// What can't be converted is passed over with why: stitches from a file, manual stitches, a
/// symbol's instance, a clip group's clipping path, a locked object, what isn't sewn (type, a hidden
/// object). A group stands for the objects in it, each converted on its own.
#[test]
fn converting_passes_over_what_it_cant_freeze() {
    let mut s = Session::new();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    let st = vectorcraft_testkit::embroidery::structures(&mut run_fx);
    let mut ids: Vec<u64> = vec![st.group, st.clip_group, st.filled_and_stroked, st.locked, st.hidden, st.text];
    ids.extend(st.instances);
    let r = run(&mut s, "embroidery.convertToManual", json!({ "ids": ids }));
    let reasons: std::collections::BTreeMap<u64, String> =
        r["skipped"].as_array().unwrap().iter().map(|k| (k["id"].as_u64().unwrap(), k["reason"].as_str().unwrap().to_string())).collect();
    let converted: Vec<u64> = r["converted"].as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
    for id in st.grouped.iter().chain([&st.filled_and_stroked]) {
        assert!(converted.contains(id), "{id} converted: {r}");
    }
    for (id, why) in [
        (st.instances[0], "symbolInstance"),
        (st.instances[1], "symbolInstance"),
        (st.locked, "locked"),
        (st.hidden, "notSewn"),
        (st.text, "notSewn"),
    ] {
        assert_eq!(reasons.get(&id).map(String::as_str), Some(why), "{id}: {r}");
    }
    assert!(reasons.values().any(|w| w == "clippingPath"), "the clipping path: {r}");
    // Again: manual stitches already.
    let again = s.execute("embroidery.convertToManual", &json!({"ids": [st.filled_and_stroked]})).unwrap_err().to_string();
    assert!(again.contains("nothing selected can be converted to manual stitches") && again.contains("(manual)"), "{again}");
    // Stitches from a file.
    let mut s = Session::new();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    vectorcraft_testkit::embroidery::opened_design(&mut run_fx, "pes");
    run(&mut s, "select.all", json!({}));
    let e = s.execute("embroidery.convertToManual", &json!({})).unwrap_err().to_string();
    assert!(e.contains("(literal)"), "{e}");
}

/// Converting is one undo step, Undo gives the document back exactly, and nothing converted takes
/// none: the error names each object and why. An id naming no object is refused before anything
/// changes, as are params of another shape.
#[test]
fn converting_is_one_step_and_refuses_before_changing_anything() {
    let mut s = blank();
    let square = red_square(&mut s, 5.0, 5.0, 10.0);
    let text = run(&mut s, "text.create", json!({"x": 20, "y": 200, "text": "Hi"}))["id"].as_u64().unwrap();
    let before = s.doc().unwrap().doc.clone();
    let steps = |s: &Session| s.doc().unwrap().history.undo.len();
    let undo = steps(&s);
    assert!(matches!(s.execute("embroidery.convertToManual", &json!({"ids": [square, 999_999]})), Err(crate::EngineError::NoNode(_))));
    for junk in [json!({"ids": [square], "spacing": "close"}), json!({"ids": "all"}), json!(5)] {
        assert!(s.execute("embroidery.convertToManual", &junk).is_err(), "{junk}");
    }
    assert!(s.execute("embroidery.convertToManual", &json!({"ids": [text]})).is_err());
    assert_eq!(steps(&s), undo, "nothing done, no step");
    run(&mut s, "embroidery.convertToManual", json!({"ids": [square]}));
    assert_eq!(steps(&s), undo + 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(*s.doc().unwrap().doc == *before, "undo gives the document back");
}

/// Converted, then restored at once: the document is the one it was, saved the same but for the id
/// counter, and its export too. Restore is one undo step.
#[test]
fn converting_and_restoring_gives_the_document_back() {
    let mut s = Session::new();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    let st = vectorcraft_testkit::embroidery::structures(&mut run_fx);
    let saved = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let before = exports(&mut s);
    let ids = [st.group, st.clip_group, st.filled_and_stroked];
    run(&mut s, "embroidery.convertToManual", json!({ "ids": ids }));
    let steps = s.doc().unwrap().history.undo.len();
    let r = run(&mut s, "embroidery.restoreObject", json!({ "ids": ids }));
    assert_eq!((r["editedCount"].clone(), s.doc().unwrap().history.undo.len()), (json!(0), steps + 1), "{r}");
    // The same file but for the id counter, which never goes back (the runs' ids are not handed out
    // again).
    let file = |bytes: &[u8]| {
        let mut v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        v["document"].as_object_mut().unwrap().remove("next_id");
        v
    };
    assert!(file(&vectorcraft_format::save(&s.doc().unwrap().doc, false)) == file(&saved), "saved the same but for the id counter");
    assert_eq!(exports(&mut s), before);
}

/// A converted object moved, turned, mirrored and scaled comes back where its stitches are, as the
/// object transformed so would be; its strokes scaled as the preferences say (or as asked).
#[test]
fn a_restored_object_comes_back_where_its_stitches_are() {
    let mut s = blank();
    let square = red_square(&mut s, 5.0, 5.0, 10.0);
    let mut twin = blank();
    let other = red_square(&mut twin, 5.0, 5.0, 10.0);
    assert_eq!(square, other);
    let transform = |s: &mut Session, id: u64| {
        run(s, "object.move", json!({"ids": [id], "dx": 30, "dy": -4}));
        run(s, "object.rotate", json!({"ids": [id], "angle": 30}));
        run(s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
        run(s, "object.scale", json!({"ids": [id], "sx": 120, "sy": 90}));
    };
    run(&mut s, "embroidery.convertToManual", json!({"ids": [square]}));
    transform(&mut s, square);
    transform(&mut twin, other);
    run(&mut s, "embroidery.restoreObject", json!({"ids": [square]}));
    let anchors = |s: &Session, id: u64| -> Vec<vectorcraft_geom::Point> {
        s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).unwrap().path_data().unwrap().subpaths[0].anchors.iter().map(|a| a.p).collect()
    };
    let (got, want) = (anchors(&s, square), anchors(&twin, other));
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert!(g.distance(*w) < 1e-9, "{g:?} against {w:?}");
    }
}

/// The dry run says which objects' stitches were edited since converting (so the interface asks
/// first only then): a needle point moved is an edit, the object moved whole is not. Restoring
/// discards the edits.
#[test]
fn the_dry_run_tells_edited_stitches_from_moved_ones() {
    let mut s = blank();
    let a = red_square(&mut s, 5.0, 5.0, 10.0);
    let b = red_square(&mut s, 40.0, 5.0, 10.0);
    run(&mut s, "embroidery.convertToManual", json!({"ids": [a, b]}));
    run(&mut s, "object.move", json!({"ids": [a], "dx": 10, "dy": 10}));
    let run_of_b = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(b)).unwrap().children().unwrap()[0].id.0;
    run(&mut s, "select.anchors", json!({"id": run_of_b, "anchors": [[0, 3]]}));
    run(&mut s, "path.moveAnchors", json!({"dx": 2, "dy": 0}));
    let doc = s.doc().unwrap().doc.clone();
    let dry = run(&mut s, "embroidery.restoreObject", json!({"ids": [a, b], "dryRun": true}));
    assert_eq!((dry["edited"].clone(), dry["restoredCount"].clone()), (json!([b]), json!(2)), "{dry}");
    assert!(*s.doc().unwrap().doc == *doc, "a dry run changes nothing");
    run(&mut s, "embroidery.restoreObject", json!({"ids": [run_of_b]}));
    assert!(
        matches!(s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(b)).unwrap().kind, vectorcraft_doc::NodeKind::Path { .. }),
        "a run stands for its object"
    );
}

/// What comes after a run is part of its stitches wherever it is kept: Stop After set on one run of
/// a converted object (the run's own, the object-level flag), or a stop or a cut at a run's end by
/// Machine Function, is an edit the dry run names, so Restore asks before dropping it.
#[test]
fn a_stop_or_cut_after_a_run_is_an_edit() {
    for edit in ["setStopAfter", "setTrimAfter", "stopAtTheEnd", "cutAtAGap"] {
        let mut s = blank();
        let a = red_square(&mut s, 5.0, 5.0, 10.0);
        run(&mut s, "embroidery.convertToManual", json!({"ids": [a]}));
        let group = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(a)).unwrap().clone();
        let first = group.children().unwrap()[0].id.0;
        let last = needles::read(&group.children().unwrap()[0]).unwrap().len() - 1;
        let fresh = run(&mut s, "embroidery.restoreObject", json!({"ids": [a], "dryRun": true}));
        assert_eq!(fresh["edited"], json!([]), "{edit}: nothing edited yet");
        match edit {
            "setStopAfter" | "setTrimAfter" => run(&mut s, &format!("embroidery.{edit}"), json!({"ids": [first], "on": true})),
            "stopAtTheEnd" => {
                run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": first, "index": last}], "function": "stop"}))
            }
            _ => run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": first, "index": last}], "function": "trim"})),
        };
        let dry = run(&mut s, "embroidery.restoreObject", json!({"ids": [a], "dryRun": true}));
        assert_eq!(dry["edited"], json!([a]), "{edit}: {dry}");
    }
}

/// An id given many times, or inside another id given, is walked once: Convert, Restore's dry run
/// and the Object tab's summary count each object once, and say each one passed over once.
#[test]
fn repeated_and_nested_ids_are_walked_once() {
    let mut s = blank();
    let a = red_square(&mut s, 5.0, 5.0, 10.0);
    let b = red_square(&mut s, 40.0, 5.0, 10.0);
    let layer = s.doc().unwrap().doc.layers[0].id.0;
    let mut ids = vec![layer; 1000];
    ids.extend([a, b, a]);
    let r = run(&mut s, "embroidery.convertToManual", json!({ "ids": ids }));
    assert_eq!((r["convertedCount"].clone(), r["skippedCount"].clone()), (json!(2), json!(0)), "{r}");
    let info = run(&mut s, "embroidery.info", json!({ "ids": ids }));
    assert_eq!(info["manual"]["objects"], json!(2), "{info}");
    let dry = run(&mut s, "embroidery.restoreObject", json!({ "ids": ids, "dryRun": true }));
    assert_eq!(dry["restoredCount"], json!(2), "{dry}");
    // Converted already: each passed over once.
    let again = s.execute("embroidery.convertToManual", &json!({ "ids": [a, a, b, a] }));
    let said = again.err().map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(said.matches(&format!("{a} (manual)")).count(), 1, "{said}");
}

/// While Global Edit is on, Convert to Manual Stitches and Restore Object wait (Global Edit would copy
/// a converted object's runs onto the other members without their originals), whether the objects
/// are selected or given; stopped, they work.
#[test]
fn convert_and_restore_wait_while_global_edit_is_on() {
    let mut s = blank();
    let a = red_square(&mut s, 5.0, 5.0, 10.0);
    red_square(&mut s, 40.0, 5.0, 10.0);
    run(&mut s, "select.set", json!({"ids": [a]}));
    run(&mut s, "select.globalEdit", json!({"artboards": "all", "includeCanvas": true}));
    assert!(s.doc().unwrap().global_edit.is_some());
    let steps = s.doc().unwrap().history.undo.len();
    for (command, p) in [
        ("embroidery.convertToManual", json!({})),
        ("embroidery.convertToManual", json!({"ids": [a]})),
        ("embroidery.restoreObject", json!({"ids": [a]})),
    ] {
        match s.execute(command, &p) {
            Err(crate::EngineError::Disabled(_, why)) => assert_eq!(why, super::GLOBAL_EDIT_ON, "{command}"),
            r => panic!("{command} {p}: {r:?}"),
        }
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), steps, "nothing changed");
    run(&mut s, "select.globalEdit", json!({}));
    run(&mut s, "select.set", json!({"ids": [a]}));
    assert_eq!(run(&mut s, "embroidery.convertToManual", json!({}))["convertedCount"], json!(1));
}

/// A converted compound path and its duplicate, restored in one step, give every node an id of its
/// own: the second original's members, which hold the same ids as the first's, take new ones.
#[test]
fn restored_duplicates_keep_every_id_unique() {
    let mut s = blank();
    let outer = red_square(&mut s, 5.0, 5.0, 20.0);
    let inner = red_square(&mut s, 10.0, 10.0, 8.0);
    run(&mut s, "select.set", json!({"ids": [outer, inner]}));
    let compound = run(&mut s, "object.compoundPath.make", json!({}))["id"].as_u64().unwrap();
    run(&mut s, "embroidery.convertToManual", json!({"ids": [compound]}));
    run(&mut s, "select.set", json!({"ids": [compound]}));
    let copy = run(&mut s, "edit.duplicate", json!({}))["ids"][0].as_u64().unwrap();
    let r = run(&mut s, "embroidery.restoreObject", json!({"ids": [compound, copy]}));
    assert_eq!(r["restoredCount"], json!(2), "{r}");
    let mut ids = Vec::new();
    for l in &s.doc().unwrap().doc.layers {
        l.walk(&mut |n| ids.push(n.id.0));
    }
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "every id once");
    let members = |id: u64| s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).unwrap().children().map_or(0, |c| c.len());
    assert_eq!((members(compound), members(copy)), (2, 2), "both compounds back, each with its two members");
}

/// A cut before a manual run is sewn, whatever the move into it: the first run of two deleted, its
/// cut after it passes to the second as the cut before it, and the export still cuts there (the
/// runs lie near a square of their colour, so no move is long enough to be cut for its length).
#[test]
fn a_cut_before_a_manual_run_is_sewn() {
    use vectorcraft_doc::embroidery::StitchRun;
    let mut s = blank();
    red_square(&mut s, 10.0, 6.0, 3.0);
    let a = literal_path(
        &mut s,
        None,
        [200, 30, 30],
        &[(10.0, 5.0), (12.0, 5.0), (14.0, 5.0)],
        StitchRun { manual: true, trim_after: true, ..StitchRun::default() },
    );
    let b = literal_path(&mut s, None, [200, 30, 30], &[(16.0, 5.0), (18.0, 5.0)], StitchRun { manual: true, ..StitchRun::default() });
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    run(&mut s, "object.group", json!({}));
    let trims = |s: &mut Session| run(s, "embroidery.stats", json!({}))["trims"].as_u64().unwrap();
    assert_eq!(trims(&mut s), 1, "a's cut after it");
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": a, "from": 0, "to": 2}]}));
    assert_eq!(r["moved"], json!([{"function": "trim", "id": b, "before": true}]), "{r}");
    assert_eq!(trims(&mut s), 1, "b's cut before it, sewn");
}

/// A group the plan sews whole (a drop shadow on it) of two near squares of one colour: Trim After,
/// then Stop After, on the first square carried onto the run its last unit became, so the export
/// right after converting is the same bytes in every format and the run says so (the project
/// owner's choice, 2026-10-30).
#[test]
fn a_members_trim_and_stop_after_in_an_object_sewn_whole_are_carried() {
    for mark in ["setTrimAfter", "setStopAfter"] {
        let mut s = blank();
        let a = red_square(&mut s, 5.0, 5.0, 10.0);
        let b = red_square(&mut s, 18.0, 5.0, 10.0);
        let group = run(&mut s, "object.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap();
        run(&mut s, "effect.apply", json!({"effect": "stylize.dropShadow", "ids": [group]}));
        run(&mut s, &format!("embroidery.{mark}"), json!({"ids": [a], "on": true}));
        let before = exports(&mut s);
        let records = |s: &mut Session| run(s, "embroidery.stats", json!({}));
        let (trims, stops) = (records(&mut s)["trims"].clone(), records(&mut s)["stops"].clone());
        let r = run(&mut s, "embroidery.convertToManual", json!({"ids": [group]}));
        assert_eq!(r["convertedCount"], json!(1), "{mark}: {r}");
        assert!(exports(&mut s) == before, "{mark}: the same bytes right after converting");
        assert_eq!((records(&mut s)["trims"].clone(), records(&mut s)["stops"].clone()), (trims, stops), "{mark}");
        let runs = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(group)).unwrap().children().unwrap().clone();
        let carried = runs.iter().filter_map(|c| c.extras().embroidery.as_deref()?.stitches.as_ref()).any(|r| r.trim_after || r.stop_after);
        assert!(carried, "{mark}: on a run");
    }
}

/// A group the plan sews whole holding what isn't sewn (type, a hidden or a locked object) is passed
/// over as `holdsUnsewn`: converting would hide it inside the object kept.
#[test]
fn an_object_sewn_whole_holding_what_isnt_sewn_is_passed_over() {
    for what in ["type", "hidden", "locked"] {
        let mut s = blank();
        let a = red_square(&mut s, 5.0, 5.0, 10.0);
        let other = match what {
            "type" => run(&mut s, "text.create", json!({"x": 60, "y": 60, "text": "Label"}))["id"].as_u64().unwrap(),
            _ => red_square(&mut s, 30.0, 5.0, 10.0),
        };
        let group = run(&mut s, "object.group", json!({"ids": [a, other]}))["id"].as_u64().unwrap();
        run(&mut s, "effect.apply", json!({"effect": "stylize.dropShadow", "ids": [group]}));
        match what {
            "hidden" => run(&mut s, "object.hide", json!({"ids": [other]})),
            "locked" => run(&mut s, "object.lock", json!({"ids": [other]})),
            _ => json!({}),
        };
        let r = s.execute("embroidery.convertToManual", &json!({"ids": [group]}));
        let said = r.as_ref().err().map(ToString::to_string).unwrap_or_default();
        assert!(said.contains(&format!("{group} (holdsUnsewn)")), "{what}: {r:?}");
    }
}

/// Art in a converted object that isn't one of its runs (pasted in while it was isolated), and a run
/// hidden or recoloured, are edits Restore asks before; restoring keeps that art, beside the
/// restored object in its parent, right above it (the project owner's choice, 2026-10-30).
#[test]
fn restore_asks_before_foreign_art_and_keeps_it_beside_the_object() {
    let edited = |s: &mut Session, g: u64| run(s, "embroidery.restoreObject", json!({"ids": [g], "dryRun": true}))["edited"].clone();
    for edit in ["hide", "recolour"] {
        let mut s = blank();
        let a = red_square(&mut s, 5.0, 5.0, 10.0);
        run(&mut s, "embroidery.convertToManual", json!({"ids": [a]}));
        let first = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(a)).unwrap().children().unwrap()[0].id.0;
        assert_eq!(edited(&mut s, a), json!([]));
        match edit {
            "hide" => run(&mut s, "object.hide", json!({"ids": [first]})),
            _ => {
                run(&mut s, "select.set", json!({"ids": [first]}));
                run(&mut s, "paint.setStroke", json!({"color": "#1e1ec8"}))
            }
        };
        assert_eq!(edited(&mut s, a), json!([a]), "{edit}");
    }
    let mut s = blank();
    let a = red_square(&mut s, 5.0, 5.0, 10.0);
    run(&mut s, "embroidery.convertToManual", json!({"ids": [a]}));
    let art = red_square(&mut s, 40.0, 40.0, 5.0);
    s.edit("Into the converted object", |d, _| {
        let n = d.remove(vectorcraft_doc::NodeId(art)).unwrap();
        d.insert(Some(vectorcraft_doc::NodeId(a)), usize::MAX, (*n).clone()).map(|_| ()).map_err(|e| crate::EngineError::Other(e.to_string()))
    })
    .unwrap();
    assert_eq!(edited(&mut s, a), json!([a]), "foreign art in it");
    run(&mut s, "embroidery.restoreObject", json!({"ids": [a]}));
    let doc = s.doc().unwrap().doc.clone();
    let (parent, at, _) = doc.position(vectorcraft_doc::NodeId(art)).expect("the art kept");
    let (object_parent, object_at, _) = doc.position(vectorcraft_doc::NodeId(a)).unwrap();
    assert_eq!((parent, at), (object_parent, object_at + 1), "beside the restored object, right above it");
    assert!(matches!(doc.node(vectorcraft_doc::NodeId(a)).unwrap().kind, vectorcraft_doc::NodeKind::Path { .. }), "the object back");
}

/// What can't be restored is passed over with why: an object never converted, stitches from a file,
/// an original that doesn't read as an object, one placing a symbol the document no longer has.
#[test]
fn restoring_passes_over_what_it_cant_restore() {
    use vectorcraft_doc::embroidery::Original;
    let mut s = blank();
    let plain = red_square(&mut s, 5.0, 5.0, 10.0);
    let junk = red_square(&mut s, 30.0, 5.0, 10.0);
    let lost = red_square(&mut s, 60.0, 5.0, 10.0);
    run(&mut s, "embroidery.convertToManual", json!({"ids": [junk, lost]}));
    let gone = vectorcraft_doc::NodeKind::SymbolInstance { symbol: "Gone".into(), xf: vectorcraft_geom::Affine::IDENTITY };
    let instance = serde_json::to_value(vectorcraft_doc::Node::new(vectorcraft_doc::NodeId(9999), gone)).unwrap();
    s.edit("Damage", |d, _| {
        for (id, node) in [(junk, json!({"kind": 5})), (lost, instance.clone())] {
            let n = d.node_mut(vectorcraft_doc::NodeId(id)).ok_or(crate::EngineError::NoNode(vectorcraft_doc::NodeId(id)))?;
            if let Some(e) = n.extras_mut().embroidery.as_deref_mut() {
                let o = e.original.clone().unwrap();
                e.original = Some(Original { node: std::sync::Arc::new(node), ..o });
            }
        }
        Ok(())
    })
    .unwrap();
    // A dry run passes over what Restore would.
    let dry = run(&mut s, "embroidery.restoreObject", json!({"ids": [plain, junk, lost], "dryRun": true}));
    assert_eq!((dry["restored"].clone(), dry["restoredCount"].clone()), (json!([]), json!(0)), "{dry}");
    let reasons: Vec<(u64, &str)> =
        dry["skipped"].as_array().unwrap().iter().map(|k| (k["id"].as_u64().unwrap(), k["reason"].as_str().unwrap())).collect();
    assert_eq!(reasons, [(plain, "notConverted"), (junk, "unreadable"), (lost, "missingSymbol")]);
    let r = s.execute("embroidery.restoreObject", &json!({"ids": [plain, junk, lost]})).unwrap_err().to_string();
    for want in [format!("{plain} (notConverted)"), format!("{junk} (unreadable)"), format!("{lost} (missingSymbol)")] {
        assert!(r.contains(&want), "{want}: {r}");
    }
    let mut s = Session::new();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    vectorcraft_testkit::embroidery::opened_design(&mut run_fx, "dst");
    run(&mut s, "select.all", json!({}));
    assert!(s.execute("embroidery.restoreObject", &json!({})).unwrap_err().to_string().contains("notConverted"));
}

/// What Restore puts back is file data, so it is held to what opening a file holds an object to.
/// Put back where its stitches are, an object that would lie past the document's coordinates is
/// passed over as outOfRange: a frame of six finite numbers can still scale it past them, or past
/// any number (which no file could hold again). One kept as a layer is unreadable. Type comes back
/// with its values in range and measured.
#[test]
fn restore_holds_what_it_puts_back_to_what_opening_a_file_does() {
    use vectorcraft_doc::embroidery::Original;
    use vectorcraft_doc::text::{CharPosition, CharStyle, RubySpan, ScriptMetrics};
    use vectorcraft_doc::{NodeId, NodeKind};
    use vectorcraft_geom::Affine;
    let mut s = blank();
    let [past, endless, layer, typed] = [5.0, 30.0, 60.0, 90.0].map(|x| red_square(&mut s, x, 5.0, 10.0));
    let t = run(&mut s, "text.create", json!({"x": 0, "y": 0, "text": "Ab"}))["id"].as_u64().unwrap();
    let mut text = s.doc().unwrap().doc.node(NodeId(t)).unwrap().clone();
    if let NodeKind::Text(tx) = &mut text.kind {
        tx.runs[0].style.size = -5.0;
        tx.runs[0].style.position = CharPosition::Superscript(ScriptMetrics { size: 9000.0, position: -9000.0 });
        tx.ruby = vec![RubySpan::new(9, 1, " x ")];
    }
    run(&mut s, "edit.clear", json!({"ids": [t]}));
    let mut emptied = (*s.doc().unwrap().doc.layers[0]).clone();
    emptied.children_mut().unwrap().clear();
    run(&mut s, "embroidery.convertToManual", json!({"ids": [past, endless, layer, typed]}));
    // A frame scaling the square past the coordinates, one past any number, a layer, the type.
    let kept = [
        (past, Affine::scale(1e6), None),
        (endless, Affine::scale(1e308), None),
        (layer, Affine::IDENTITY, Some(serde_json::to_value(&emptied).unwrap())),
        (typed, Affine::IDENTITY, Some(serde_json::to_value(&text).unwrap())),
    ];
    s.edit("Damage", |d, _| {
        for (id, frame, node) in &kept {
            let n = d.node_mut(NodeId(*id)).unwrap();
            if let Some(e) = n.extras_mut().embroidery.as_deref_mut() {
                let o = e.original.clone().unwrap();
                let node = node.clone().map_or(o.node.clone(), std::sync::Arc::new);
                e.original = Some(Original { frame: *frame, node, ..o });
            }
        }
        Ok(())
    })
    .unwrap();
    let dry = run(&mut s, "embroidery.restoreObject", json!({"ids": [past, endless, layer, typed], "dryRun": true}));
    let r = run(&mut s, "embroidery.restoreObject", json!({"ids": [past, endless, layer, typed]}));
    assert_eq!((&dry["restored"], &dry["skipped"]), (&r["restored"], &r["skipped"]), "a dry run passes over the same");
    let skipped: Vec<(u64, &str)> =
        r["skipped"].as_array().unwrap().iter().map(|k| (k["id"].as_u64().unwrap(), k["reason"].as_str().unwrap())).collect();
    assert_eq!(skipped, [(past, "outOfRange"), (endless, "outOfRange"), (layer, "unreadable")]);
    assert_eq!(r["restored"], json!([typed]));
    let doc = &s.doc().unwrap().doc;
    let NodeKind::Text(tx) = &doc.node(NodeId(typed)).unwrap().kind else { panic!("the type put back") };
    assert_eq!(tx.runs[0].style.size, CharStyle::default().size, "a size that isn't one: the default");
    assert_eq!(tx.runs[0].style.position, CharPosition::Superscript(ScriptMetrics { size: 500.0, position: -500.0 }));
    assert_eq!(tx.ruby, [RubySpan::new(1, 2, "x")], "the reading's range within the text, ordered, trimmed");
    assert!(tx.cached_bounds.is_some(), "measured, as an opened file's type is");
    assert!(doc.layers.iter().all(|l| l.children().unwrap().iter().all(|c| !c.is_layer())));
}

/// The scale check on `s`: its one rule's reply.
fn scale_rule(s: &mut Session) -> serde_json::Value {
    run(s, "embroidery.check", json!({"rules": ["stitchesScaled"]}))["rules"][0].clone()
}

/// Manual stitches scaled past ±10 % since they were converted are warned about, saying how much
/// and which way they changed most; exactly 90 % or 110 % passes (a hundredth of a percent decides,
/// not the arithmetic's last bit), and so do a turn and a mirror. A run only moved since it was made
/// was never scaled: the rule judges nothing.
#[test]
fn manual_stitches_scaled_past_ten_percent_are_warned_about() {
    let scaled = |steps: &[(&str, serde_json::Value)]| {
        let mut s = blank();
        let square = red_square(&mut s, 5.0, 5.0, 10.0);
        run(&mut s, "embroidery.convertToManual", json!({"ids": [square]}));
        for (command, params) in steps {
            let mut params = params.clone();
            params["ids"] = json!([square]);
            run(&mut s, command, params);
        }
        (square, scale_rule(&mut s))
    };
    let scale = |sx: f64, sy: f64| ("object.scale", json!({"sx": sx, "sy": sy}));
    for (sx, sy, measured) in [(105.0, 105.0, 5.0), (110.0, 100.0, 10.0), (90.0, 90.0, 10.0), (100.0, 109.99, 9.99)] {
        let (_, rule) = scaled(&[scale(sx, sy)]);
        assert_eq!((rule["hits"].clone(), rule["measured"].clone()), (json!(0), json!(measured)), "{sx} × {sy} %: {rule}");
    }
    for (sx, sy, measured, note) in
        [(111.0, 100.0, 11.0, "larger"), (100.0, 89.0, 11.0, "smaller"), (120.0, 95.0, 20.0, "larger"), (85.0, 105.0, 15.0, "smaller")]
    {
        let (square, rule) = scaled(&[scale(sx, sy)]);
        assert_eq!((rule["severity"].as_str(), rule["objects"].clone()), (Some("warning"), json!([square])), "{rule}");
        assert!(rule["hits"].as_u64().unwrap() >= 1, "{rule}");
        let hit = &rule["examples"][0];
        assert_eq!(
            (hit["value"].clone(), hit["note"].as_str(), hit["id"].as_u64()),
            (json!(measured), Some(note), Some(square)),
            "{sx} × {sy} %: {hit}"
        );
        assert_eq!(rule["measured"], json!(measured));
    }
    // Scaled up and back down: the record composes, so the stitches are their size again.
    let (_, rule) = scaled(&[scale(150.0, 150.0), scale(100.0 / 1.5, 100.0 / 1.5)]);
    assert_eq!((rule["hits"].clone(), rule["measured"].clone()), (json!(0), json!(0.0)), "{rule}");
    let turned = [("object.rotate", json!({"angle": 30})), ("object.reflect", json!({"axis": "vertical"}))];
    let (_, rule) = scaled(&turned);
    assert_eq!((rule["hits"].clone(), rule["measured"].clone()), (json!(0), json!(0.0)), "turned and mirrored: {rule}");
    let (_, rule) = scaled(&[("object.move", json!({"dx": 30, "dy": -4}))]);
    assert_eq!((rule["hits"].clone(), rule["measured"].clone()), (json!(0), json!(null)), "only moved: {rule}");
}

/// The same holds for a machine file's runs (the owner's decision: opened files too), and for runs
/// a symbol instance places: the instance's scale counts, as the walk places its art.
#[test]
fn a_files_runs_scaled_or_placed_scaled_are_warned_about() {
    use vectorcraft_doc::embroidery::StitchRun;
    let points = [(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (14.0, 12.0)];
    let mut s = blank();
    let line = literal_path(&mut s, None, [200, 30, 30], &points, StitchRun::default());
    assert_eq!(scale_rule(&mut s)["measured"], json!(null), "as opened");
    run(&mut s, "object.scale", json!({"ids": [line], "sx": 125, "sy": 125}));
    let rule = scale_rule(&mut s);
    assert_eq!(
        (rule["hits"].clone(), rule["examples"][0]["value"].clone(), rule["objects"].clone()),
        (json!(1), json!(25.0), json!([line])),
        "{rule}"
    );
    let mut s = blank();
    let line = literal_path(&mut s, None, [200, 30, 30], &points, StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [line]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Run"}))["id"].as_u64().unwrap();
    assert_eq!(scale_rule(&mut s)["hits"], json!(0), "placed at its size");
    run(&mut s, "object.scale", json!({"ids": [instance], "sx": 150, "sy": 150}));
    let rule = scale_rule(&mut s);
    assert_eq!(
        (rule["hits"].clone(), rule["examples"][0]["value"].clone(), rule["objects"].clone()),
        (json!(1), json!(50.0), json!([instance])),
        "{rule}"
    );
}

/// The plan of `s` as the export plans it.
fn export_plan(s: &mut Session) -> DesignPlan {
    (*s.embroidery_plan(&Options::default()).unwrap()).clone()
}

/// The needle points units of object `id` sew, and how each was sewn.
fn sewn_by(p: &DesignPlan, id: u64) -> (Vec<(f64, f64)>, Vec<Choice>) {
    let mut points = Vec::new();
    let mut why = Vec::new();
    for (k, u) in p.units.iter().enumerate() {
        if u.origin.selected() == vectorcraft_doc::NodeId(id) {
            points.extend(p.own_stitches(k).iter().filter(|s| s.command == Command::Stitch).map(|s| (s.x, s.y)));
            why.push(u.why);
        }
    }
    (points, why)
}

/// A change of an object's appearance: given the session and the object's id.
type Look = fn(&mut Session, u64);

/// A run is sewn as its points whatever its appearance (it is never expanded, which would plan its
/// stitches again as art): a second stroke, an effect that reshapes it, a stroke's opacity or a
/// brush on a file's run or a manual one, and an effect on a converted object's group, leave its
/// stitches as they were, and the plan says what isn't sewn, naming the object; a drop shadow alone
/// is said as raster effects are.
#[test]
fn a_run_is_sewn_as_its_points_whatever_its_appearance() {
    use super::printed::{RASTER_LEFT_OUT, RUN_LOOK_LEFT_OUT};
    use vectorcraft_doc::embroidery::StitchRun;
    let points = [(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (14.0, 12.0)];
    let looks: [(&str, Look); 5] = [
        ("a second stroke", |s, id| {
            run(s, "appearance.addStroke", json!({"ids": [id]}));
        }),
        ("a reshaping effect", |s, id| {
            run(s, "effect.apply", json!({"effect": "distort.roughen", "ids": [id]}));
        }),
        ("a stroke's opacity", |s, id| {
            s.edit("Opacity", |d, _| {
                let n = d.node_mut(vectorcraft_doc::NodeId(id)).ok_or(EngineError::NoNode(vectorcraft_doc::NodeId(id)))?;
                if let Some(vectorcraft_doc::AppearanceItem::Stroke(st)) = n.appearance.items.last_mut() {
                    st.opacity = 0.5;
                }
                Ok(())
            })
            .unwrap();
        }),
        ("a brush", |s, id| {
            let dot = red_square(s, 100.0, 100.0, 1.0);
            let brush = run(s, "brush.new", json!({"type": "scatter", "ids": [dot]}))["name"].as_str().unwrap().to_string();
            run(s, "edit.clear", json!({"ids": [dot]}));
            run(s, "brush.apply", json!({"name": brush, "ids": [id]}));
        }),
        ("a drop shadow", |s, id| {
            run(s, "effect.apply", json!({"effect": "stylize.dropShadow", "ids": [id]}));
        }),
    ];
    for manual in [false, true] {
        for (what, give) in &looks {
            let mut s = blank();
            let id = literal_path(&mut s, None, [200, 30, 30], &points, StitchRun { manual, ..StitchRun::default() });
            let before = sewn_by(&export_plan(&mut s), id);
            give(&mut s, id);
            let p = export_plan(&mut s);
            assert_eq!(sewn_by(&p, id), before, "{what}, manual {manual}: sewn as its points");
            let named = p.warned.get(RUN_LOOK_LEFT_OUT).cloned().unwrap_or_default();
            if *what == "a drop shadow" {
                assert!(named.is_empty() && p.warnings.iter().any(|w| w == RASTER_LEFT_OUT), "{what}: {:?}", p.warnings);
            } else {
                assert_eq!(named, [vectorcraft_doc::NodeId(id)], "{what}, manual {manual}: {:?}", p.warnings);
            }
        }
    }
    // A converted object's group given an effect.
    let mut s = blank();
    let square = red_square(&mut s, 5.0, 5.0, 10.0);
    run(&mut s, "embroidery.convertToManual", json!({"ids": [square]}));
    let before = sewn_by(&export_plan(&mut s), square);
    assert!(before.1.iter().all(|w| *w == Choice::Manual), "{:?}", before.1);
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [square]}));
    let p = export_plan(&mut s);
    assert_eq!(sewn_by(&p, square), before, "the group isn't expanded");
    assert_eq!(p.warned.get(RUN_LOOK_LEFT_OUT).cloned().unwrap_or_default(), [vectorcraft_doc::NodeId(square)]);
}

/// A run's needle points are its anchors: a run edited into curves (handles pulled out, a closed
/// subpath) sews each anchor once, in order, the curves between them straight stitches, and no
/// closing stitch; an empty subpath doesn't move the marks of those after it.
#[test]
fn a_run_sews_its_anchors_whatever_its_handles() {
    use vectorcraft_doc::embroidery::StitchRun;
    let points = [(10.0, 10.0), (20.0, 10.0), (30.0, 15.0), (40.0, 10.0)];
    let mut s = blank();
    let id = literal_path(
        &mut s,
        None,
        [200, 30, 30],
        &points,
        StitchRun { stops: vec![vectorcraft_doc::embroidery::Mark::place(2, 0)], ..StitchRun::default() },
    );
    let before = sewn_by(&export_plan(&mut s), id).0;
    assert_eq!(before.len(), points.len());
    s.edit("Curves", |d, _| {
        let n = d.node_mut(vectorcraft_doc::NodeId(id)).ok_or(EngineError::NoNode(vectorcraft_doc::NodeId(id)))?;
        let vectorcraft_doc::NodeKind::Path { path, .. } = &mut n.kind else { return Ok(()) };
        let sp = &mut path.subpaths[0];
        for a in &mut sp.anchors {
            a.h_in = a.p + vectorcraft_geom::Vec2::new(-5.0, 8.0);
            a.h_out = a.p + vectorcraft_geom::Vec2::new(5.0, -8.0);
            a.kind = vectorcraft_geom::AnchorKind::Smooth;
        }
        sp.closed = true;
        // An empty subpath, then a second run of two needle points with a stop after its first.
        path.subpaths.push(vectorcraft_geom::SubPath { anchors: Vec::new(), closed: false });
        let at = |x: f64| vectorcraft_geom::Anchor::corner(vectorcraft_geom::Point::new(x * PT_PER_MM, 30.0 * PT_PER_MM));
        path.subpaths.push(vectorcraft_geom::SubPath { anchors: vec![at(10.0), at(14.0)], closed: false });
        Ok(())
    })
    .unwrap();
    let p = export_plan(&mut s);
    let (sewn, _) = sewn_by(&p, id);
    assert_eq!(&sewn[..points.len()], &before[..], "the anchors, not the curves");
    assert_eq!(sewn.len(), points.len() + 2, "no closing stitch; the second run's two points");
    let stops = p.blocks.iter().flat_map(|b| &b.stitches).filter(|s| s.command == Command::Stop).count();
    assert_eq!(stops, 1, "the stop named on the third subpath's first needle point");
}

/// `embroidery.info` sums up the converted objects among those given (a manual run standing for its
/// object): how many, how many can be restored, how many were edited since converting, their runs
/// and needle points, and how much they were scaled since; and reads Stop After as Trim After.
#[test]
fn info_sums_up_converted_objects() {
    let mut s = blank();
    let a = red_square(&mut s, 5.0, 5.0, 10.0);
    let b = red_square(&mut s, 40.0, 5.0, 10.0);
    run(&mut s, "embroidery.convertToManual", json!({"ids": [a, b]}));
    let info = |s: &mut Session, ids: &[u64]| run(s, "embroidery.info", json!({ "ids": ids }));
    let m = info(&mut s, &[a, b])["manual"].clone();
    assert_eq!(
        (m["objects"].clone(), m["restorable"].clone(), m["edited"].clone(), m["scale"].clone()),
        (json!(2), json!(2), json!(0), json!([1.0, 1.0])),
        "{m}"
    );
    assert!(m["runs"].as_u64().unwrap() >= 2 && m["needlePoints"].as_u64().unwrap() > 20, "{m}");
    run(&mut s, "object.scale", json!({"ids": [b], "sx": 120, "sy": 100}));
    let run_of_a = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(a)).unwrap().children().unwrap()[0].id.0;
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": run_of_a, "index": 1}], "dx": 1}));
    run(&mut s, "object.lock", json!({"ids": [b]}));
    let m = info(&mut s, &[run_of_a, b])["manual"].clone();
    assert_eq!(
        (m["objects"].clone(), m["restorable"].clone(), m["edited"].clone()),
        (json!(2), json!(1), json!(1)),
        "a run stands for its object: {m}"
    );
    let scale = m["scale"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!((scale[0] - 1.0).abs() < 1e-9 && (scale[1] - 1.2).abs() < 1e-9, "{m}");
    let art = red_square(&mut s, 80.0, 5.0, 10.0);
    assert_eq!(info(&mut s, &[art])["manual"], Value::Null);
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [a], "on": true}));
    assert_eq!(info(&mut s, &[a])["stopAfter"], json!({"value": true, "setCount": 1, "mixed": false}));
    assert_eq!(info(&mut s, &[a, art])["stopAfter"]["mixed"], json!(true));
}
