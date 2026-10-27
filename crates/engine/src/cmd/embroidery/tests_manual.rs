//! Manual stitches: what the plan records of each unit to freeze it (its own stitches, the needle
//! point it shares with the unit before, its place in walk order).

use serde_json::json;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::plan::Part;
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
/// back, while the needle point lies where it was stored; moved with its object, the run drops them.
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
    let moved = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).and_then(|n| n.extras().embroidery.as_deref()?.stitches.clone()).unwrap();
    assert!(moved.exact.is_empty(), "a transform drops them");
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
    let instance = json!({"id": 9999, "kind": {"type": "symbolInstance", "symbol": "Gone", "xf": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]}});
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
    let r = s.execute("embroidery.restoreObject", &json!({"ids": [plain, junk, lost]})).unwrap_err().to_string();
    for want in [format!("{plain} (notConverted)"), format!("{junk} (unreadable)")] {
        assert!(r.contains(&want), "{want}: {r}");
    }
    assert!(r.contains(&format!("{lost} (missingSymbol)")) || r.contains(&format!("{lost} (unreadable)")), "{r}");
    let mut s = Session::new();
    let mut run_fx = |c: &str, p: serde_json::Value| run(&mut s, c, p);
    vectorcraft_testkit::embroidery::opened_design(&mut run_fx, "dst");
    run(&mut s, "select.all", json!({}));
    assert!(s.execute("embroidery.restoreObject", &json!({})).unwrap_err().to_string().contains("notConverted"));
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
