//! Stops in the plan: a run's subpaths, cuts and stops sewn as it marks them, Stop After on any
//! object, a stop kept only where a machine file writes it (between two needle points of its
//! block), counted by the stats, the sewing order and the check, read back from a file, and set by
//! `embroidery.setStopAfter`.

use std::sync::Arc;

use serde_json::json;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::embroidery::{Embroidery, Mark, StitchRun};
use vectorcraft_doc::{Appearance, Node, NodeId};
use vectorcraft_geom::{BezPath, PathData, Point};
use vectorcraft_stitch::format::Format as StitchFormat;
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};

use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// The thread colour the runs are painted with.
const RED: [u8; 3] = [200, 30, 30];

/// A run of literal stitches added to the document's first layer, stroked in `color`: its subpaths
/// through `subpaths` (millimetres), marked as `stitches` says → its id.
fn add_run(s: &mut Session, color: [u8; 3], subpaths: &[&[(f64, f64)]], stitches: StitchRun) -> u64 {
    let mut bp = BezPath::new();
    for sub in subpaths {
        for (i, &(x, y)) in sub.iter().enumerate() {
            let p = Point::new(x * PT_PER_MM, y * PT_PER_MM);
            if i == 0 { bp.move_to(p) } else { bp.line_to(p) }
        }
    }
    let mut id = 0;
    s.edit("Add Run", |d, _| {
        let layer = d.layers.first().map(|l| l.id);
        let node_id = d.alloc_id();
        let paint = Paint::solid(Color::rgb8(color[0], color[1], color[2]));
        let mut node = Node::path(node_id, PathData::from_bezpath(&bp), Appearance::basic(Paint::None, paint, 1.0));
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(stitches.clone()), ..Embroidery::default() }));
        d.insert(layer, usize::MAX, node).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        id = node_id.0;
        Ok(())
    })
    .unwrap();
    id
}

/// The active document's plan, as the read commands plan it.
fn plan_of(s: &mut Session) -> Arc<plan::DesignPlan> {
    s.embroidery_plan(&Options::default()).unwrap()
}

/// A block's records as (command, x, y) in tenths of a millimetre, rounded.
fn records(b: &plan::ThreadBlock) -> Vec<(Command, i64, i64)> {
    b.stitches.iter().map(|s| (s.command, (s.x * 10.0).round() as i64, (s.y * 10.0).round() as i64)).collect()
}

/// A run's subpaths are runs of their own, the needle jumping between them (with a cut first where
/// `trims` names the subpath reached); a stop named inside a subpath lifts the needle to the next
/// needle point, one named at a subpath's end comes after its cut. A stop at the block's end is the
/// thread change's (or the design's end): the plan leaves it out, as a machine file does.
#[test]
fn a_runs_subpaths_cuts_and_stops_are_sewn_as_marked() {
    use Command::{Jump, Stitch as Sew, Stop, Trim};
    let mut s = blank();
    let run_marks =
        StitchRun { trims: vec![Mark::place(2, 0)], stops: vec![Mark::place(0, 1), Mark::place(1, 1)], stop_after: true, ..StitchRun::default() };
    add_run(&mut s, RED, &[&[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0)], &[(10.0, 0.0), (12.0, 0.0)], &[(20.0, 0.0), (22.0, 0.0)]], run_marks);
    let plan = plan_of(&mut s);
    assert_eq!(plan.blocks.len(), 1);
    assert_eq!(
        records(&plan.blocks[0]),
        [
            (Sew, 0, 0),
            (Sew, 20, 0),
            (Stop, 20, 0),
            (Jump, 40, 0),
            (Sew, 40, 0),
            (Jump, 100, 0),
            (Sew, 100, 0),
            (Sew, 120, 0),
            (Trim, 120, 0),
            (Stop, 120, 0),
            (Jump, 200, 0),
            (Sew, 200, 0),
            (Sew, 220, 0),
        ],
        "the stop after the run, at the design's end, left out"
    );
    // A second run of the block after it: the stop after the first is between two needle points.
    let mut s = blank();
    let first = add_run(&mut s, RED, &[&[(0.0, 0.0), (2.0, 0.0)]], StitchRun { stop_after: true, trim_after: true, ..StitchRun::default() });
    let group = run(&mut s, "object.group", json!({ "ids": [first] }))["id"].as_u64().unwrap();
    s.edit("Add Run To Group", |d, _| {
        let id = d.alloc_id();
        let mut bp = BezPath::new();
        bp.move_to(Point::new(5.0 * PT_PER_MM, 0.0));
        bp.line_to(Point::new(7.0 * PT_PER_MM, 0.0));
        let paint = Paint::solid(Color::rgb8(RED[0], RED[1], RED[2]));
        let mut node = Node::path(id, PathData::from_bezpath(&bp), Appearance::basic(Paint::None, paint, 1.0));
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(StitchRun::default()), ..Embroidery::default() }));
        d.insert(Some(NodeId(group)), usize::MAX, node).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        Ok(())
    })
    .unwrap();
    let plan = plan_of(&mut s);
    assert_eq!(records(&plan.blocks[0]), [(Sew, 0, 0), (Sew, 20, 0), (Trim, 20, 0), (Stop, 20, 0), (Jump, 50, 0), (Sew, 50, 0), (Sew, 70, 0)]);
}

/// A stop takes a record number, as every record does: the numbers after it are one higher, and
/// the stats and the sewing order count it.
#[test]
fn a_stop_takes_a_record_number_and_is_counted() {
    let mut s = blank();
    add_run(&mut s, RED, &[&[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (6.0, 0.0)]], StitchRun { stops: vec![Mark::place(0, 1)], ..StitchRun::default() });
    let plan = plan_of(&mut s);
    let b = &plan.blocks[0];
    // Two needle points, the stop, the jump the needle is lifted by, two more needle points.
    assert_eq!((b.first_number, b.last_number()), (plan::FIRST_NUMBER, plan::FIRST_NUMBER + 5));
    let stats = run(&mut s, "embroidery.stats", json!({}));
    assert_eq!((stats["stops"].clone(), stats["records"].clone(), stats["stitches"].clone()), (json!(1), json!(6), json!(4)));
    assert_eq!(stats["blocks"][0]["stops"], json!(1));
    let order = run(&mut s, "embroidery.sequence", json!({}));
    assert_eq!((order["blocks"][0]["stops"].clone(), order["blocks"][0]["units"][0]["stops"].clone()), (json!(1), json!(1)));
}

/// Run `id` put in a clip group whose clipping path is the rectangles `keep` (millimetres, x0 to
/// x1, y from -5 to 15), made one compound path when there are several.
fn clipped(s: &mut Session, id: u64, keep: &[[f64; 2]]) {
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"none": true}));
    let mm = |v: f64| v * PT_PER_MM;
    let rects: Vec<u64> = keep
        .iter()
        .map(|[x0, x1]| {
            run(s, "shape.rectangle", json!({"x": mm(*x0), "y": mm(-5.0), "width": mm(x1 - x0), "height": mm(20.0)}))["id"].as_u64().unwrap()
        })
        .collect();
    let clip = match rects.as_slice() {
        [one] => *one,
        _ => {
            run(s, "select.set", json!({ "ids": rects }));
            run(s, "object.compoundPath.make", json!({}))["id"].as_u64().unwrap()
        }
    };
    run(s, "select.set", json!({"ids": [id, clip]}));
    run(s, "object.clippingMask.make", json!({}));
}

/// The cuts, stops and jumps of the plan's only block, in order, with the x (tenths of a
/// millimetre) each is at.
fn lifts(s: &mut Session) -> Vec<(Command, i64)> {
    let plan = plan_of(s);
    records(&plan.blocks[0]).into_iter().filter(|(c, _, _)| *c != Command::Stitch).map(|(c, x, _)| (c, x)).collect()
}

/// Inside a clip group, a run read from a file keeps the stops and cuts it names where the clip
/// cuts it, rather than losing them with the jumps there: those between two pieces left are made at
/// their joint, the cut and then the stop before the needle lifts; a cut before the first piece is
/// the cut before the run, and a stop there comes after its first needle point; one after the last
/// piece comes after the run. The stats count each stop.
#[test]
fn a_clip_carries_a_runs_stops_and_cuts_across_its_edges() {
    use Command::{Jump, Stop, Trim};
    let line = |xs: std::ops::RangeInclusive<i32>| -> Vec<(f64, f64)> { xs.map(|x| (f64::from(x), 5.0)).collect() };
    // A stop after the needle point at 3 mm and a cut before the subpath from 6 mm; the clip leaves
    // 0 to 3 and 7 to 9.
    let mut s = blank();
    let marks = StitchRun { stops: vec![Mark::place(0, 3)], trims: vec![Mark::place(1, 0)], ..StitchRun::default() };
    let id = add_run(&mut s, RED, &[&line(0..=5), &line(6..=9)], marks);
    clipped(&mut s, id, &[[-5.0, 3.5], [6.5, 15.0]]);
    let at = lifts(&mut s);
    let joint: Vec<Command> = at.iter().map(|(c, _)| *c).collect();
    assert_eq!(joint, [Trim, Stop, Jump], "the cut, the stop, then the needle lifted: {at:?}");
    assert!(at.iter().take(2).all(|(_, x)| *x == 30), "at the last needle point before the edge: {at:?}");
    assert_eq!(run(&mut s, "embroidery.stats", json!({}))["stops"], json!(1));
    // A stop and a cut ending a subpath the clip takes away whole: the cut before the run (the move
    // into it cut), the stop after its first needle point.
    let mut s = blank();
    let marks = StitchRun { stops: vec![Mark::place(0, 1)], trims: vec![Mark::place(1, 0)], ..StitchRun::default() };
    let id = add_run(&mut s, RED, &[&line(0..=1), &line(3..=5)], marks);
    clipped(&mut s, id, &[[2.5, 15.0]]);
    assert_eq!(lifts(&mut s), [(Trim, 30), (Stop, 30), (Jump, 40)], "the cut before the run, the stop after its first needle point");
    // The clip itself, on a run's records (millimetres): the cut before the run, as above; and a
    // stop after a needle point past the edge, after the run.
    let sew = |x: i32| Stitch::new(f64::from(x), 5.0, Command::Stitch);
    let mark = |x: i32, c| Stitch::new(f64::from(x), 5.0, c);
    let clip = |points: Vec<Stitch>, [x0, x1]: [f64; 2]| {
        let mut keep = BezPath::new();
        keep.move_to((x0, -5.0));
        keep.line_to((x1, -5.0));
        keep.line_to((x1, 15.0));
        keep.line_to((x0, 15.0));
        keep.close_path();
        super::unit::cut_run_to(points, &(keep, vectorcraft_geom::FillRule::NonZero), &vectorcraft_stitch::plan::Params::default())
    };
    let before = clip(vec![sew(0), sew(1), mark(1, Trim), mark(1, Stop), Stitch::jump(Point::new(3.0, 5.0)), sew(3), sew(4), sew(5)], [2.5, 15.0]);
    assert!(before.cut_before && before.after == vectorcraft_stitch::plan::Carried::default());
    let after = clip(vec![sew(0), sew(1), sew(2), sew(3), mark(3, Stop), Stitch::jump(Point::new(4.0, 5.0)), sew(4), sew(5)], [-5.0, 2.5]);
    assert!(after.after.stop && !after.after.cut && !after.cut_before);
    assert!(after.stitches.iter().all(|s| s.command == Command::Stitch && s.x < 2.5), "{:?}", after.stitches);
}

/// Two rectangles of `color` 40 mm apart (sewn in one block, the thread cut between), the first
/// marked with `first`: their ids.
fn two_apart(s: &mut Session, first: Embroidery) -> [u64; 2] {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#c81e1e"}));
    let side = 10.0 * PT_PER_MM;
    let a = run(s, "shape.rectangle", json!({"x": 20, "y": 20, "width": side, "height": side}))["id"].as_u64().unwrap();
    let b = run(s, "shape.rectangle", json!({"x": 20.0 + 50.0 * PT_PER_MM, "y": 20, "width": side, "height": side}))["id"].as_u64().unwrap();
    s.edit("Mark", |d, _| {
        let n = d.node_mut(NodeId(a)).ok_or(crate::EngineError::NoNode(NodeId(a)))?;
        n.extras_mut().embroidery = Some(Box::new(first.clone()));
        Ok(())
    })
    .unwrap();
    [a, b]
}

/// Stop After on an object stops the machine after its stitches, after its cut: between two objects
/// of one colour it is a stop the export writes as a change to the same thread (the Check tab
/// counts it as a colour of the format's and a colour change of the machine's); on the design's
/// last object it is the end's, and nothing is written.
#[test]
fn stop_after_on_an_object_stops_the_machine_after_it() {
    let mut stop = Embroidery::default();
    stop.set_stop_after(true);
    let mut s = blank();
    two_apart(&mut s, stop.clone());
    let plan = plan_of(&mut s);
    assert_eq!(plan.blocks.len(), 1, "one colour, one block");
    let commands: Vec<Command> = plan.blocks[0].stitches.iter().map(|s| s.command).filter(|c| *c != Command::Stitch).collect();
    let at = commands.iter().position(|c| *c == Command::Stop).expect("a stop");
    assert_eq!(commands.get(at + 1), Some(&Command::Trim), "the move after the stop is long: cut, then jumped");
    let stats = run(&mut s, "embroidery.stats", json!({}));
    assert_eq!((stats["stops"].clone(), stats["colorChanges"].clone()), (json!(1), json!(0)));
    let check = run(&mut s, "embroidery.check", json!({"format": "pes", "rules": ["formatColors", "machineColorChanges"]}));
    let measured = |id: &str| check["rules"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).unwrap()["measured"].clone();
    assert_eq!((measured("formatColors"), measured("machineColorChanges")), (json!(2), json!(1)));
    // Written: one colour change, read back as two blocks of the same thread.
    let pattern = plan.to_pattern();
    let back = StitchFormat::Pes.read(&StitchFormat::Pes.write(&pattern).unwrap()).unwrap();
    assert_eq!(back.blocks.len(), 2);
    assert_eq!(back.blocks[0].thread, back.blocks[1].thread);
    // On the design's last object: the end's, nothing written.
    let mut s = blank();
    let [_, b] = two_apart(&mut s, Embroidery::default());
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [b]}));
    assert_eq!(run(&mut s, "embroidery.stats", json!({}))["stops"], json!(0));
    // Trim After and Stop After both, the next object of the colour far: cut once, then stopped,
    // not cut again after the stop.
    let mut both = stop;
    both.set_trim_after(true);
    let mut s = blank();
    two_apart(&mut s, both);
    let plan = plan_of(&mut s);
    let commands: Vec<Command> = plan.blocks[0].stitches.iter().map(|s| s.command).filter(|c| *c != Command::Stitch).collect();
    assert_eq!(commands.iter().filter(|c| **c == Command::Trim).count(), 1, "{commands:?}");
    let at = commands.iter().position(|c| *c == Command::Stop).expect("a stop");
    assert_eq!((commands.get(at.wrapping_sub(1)), commands.get(at + 1)), (Some(&Command::Trim), Some(&Command::Jump)), "{commands:?}");
    assert_eq!(run(&mut s, "embroidery.stats", json!({}))["trims"], json!(1));
}

/// A design whose neighbouring blocks are in one thread: a red block, a stop (written as a change to
/// the same red), the red again, then a blue block.
fn stopped_design() -> Pattern {
    let row = |color: [u8; 3], y: f64, from: i32, to: i32| {
        let mut b = Block::new(Thread::rgb(color));
        b.stitches = (from..to).map(|i| Stitch::new(f64::from(i) * 2.0, y, Command::Stitch)).collect();
        b
    };
    let mut red = row(RED, 0.0, 0, 6);
    let at = red.stitches[2];
    red.stitches.insert(3, Stitch::new(at.x, at.y, Command::Stop));
    Pattern { name: "stops".into(), blocks: vec![red, row([30, 30, 200], 6.0, 0, 5)], ..Pattern::default() }
}

/// Opening a file whose neighbouring blocks are in one thread (PES, PEC, JEF, VP3, which name their
/// threads) gives one group with a stop after the run before the change, and the design is written
/// back to the same bytes; DST names no threads, so its blocks stay apart.
#[test]
fn a_change_to_the_same_thread_opens_as_a_stop() {
    let pattern = stopped_design();
    for format in [StitchFormat::Pes, StitchFormat::Pec, StitchFormat::Jef, StitchFormat::Vp3, StitchFormat::Dst] {
        let bytes = format.write(&pattern).unwrap();
        let mut s = Session::new();
        let name = format!("stops.{}", format.extension());
        run(&mut s, "document.open", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(&bytes)}));
        let doc = s.doc().unwrap().doc.clone();
        let groups = doc.layers[0].children().unwrap();
        let stopped: Vec<bool> = doc.layers[0]
            .children()
            .into_iter()
            .flatten()
            .flat_map(|g| g.children().into_iter().flatten())
            .map(|r| r.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|r| r.stop_after))
            .collect();
        let merges = format != StitchFormat::Dst;
        assert_eq!(groups.len(), if merges { 2 } else { 3 }, "{format:?}");
        assert_eq!(stopped, if merges { vec![true, false, false] } else { vec![false; 3] }, "{format:?}");
        let again = run(&mut s, "document.export", json!({"format": format.id()}));
        let again = vectorcraft_format::base64_decode(again["dataBase64"].as_str().unwrap()).unwrap();
        assert!(again == bytes, "{format:?}: written back to the same bytes");
        if merges {
            assert_eq!(run(&mut s, "embroidery.stats", json!({}))["stops"], json!(1), "{format:?}");
        }
    }
}

/// `embroidery.setStopAfter` marks the objects given (or selected), toggles when `on` is left out,
/// clears a run's own stop when off, takes one undo step, and refuses what isn't sewn.
#[test]
fn set_stop_after_marks_toggles_and_refuses() {
    let mut s = blank();
    let [a, b] = two_apart(&mut s, Embroidery::default());
    let stopped =
        |s: &Session, id: u64| s.doc().unwrap().doc.node(NodeId(id)).and_then(|n| n.extras().embroidery.as_deref()).is_some_and(|e| e.stop_after);
    let steps = |s: &Session| s.doc().unwrap().history.undo.len();
    let before = steps(&s);
    assert_eq!(run(&mut s, "embroidery.setStopAfter", json!({"ids": [a, b]})), json!({"on": true, "changed": 2}));
    assert!(stopped(&s, a) && stopped(&s, b));
    assert_eq!(steps(&s), before + 1, "one undo step");
    assert_eq!(run(&mut s, "embroidery.setStopAfter", json!({"ids": [a, b]})), json!({"on": false, "changed": 2}), "all marked: off");
    assert_eq!(run(&mut s, "embroidery.setStopAfter", json!({"ids": [a], "on": false})), json!({"on": false, "changed": 0}));
    assert_eq!(steps(&s), before + 2, "none when nothing changes");
    let run_id = add_run(&mut s, RED, &[&[(0.0, 50.0), (2.0, 50.0)]], StitchRun { stop_after: true, ..StitchRun::default() });
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [run_id], "on": false}));
    let run_stop = s.doc().unwrap().doc.node(NodeId(run_id)).and_then(|n| n.extras().embroidery.as_deref()?.stitches.clone()).map(|r| r.stop_after);
    assert_eq!(run_stop, Some(false), "a run's own stop cleared");
    let text = run(&mut s, "text.create", json!({"x": 20, "y": 200, "text": "Hi"}))["id"].as_u64().unwrap();
    for (p, refused) in [(json!({"ids": [text]}), "none of the objects given is sewn"), (json!({"ids": [a], "on": "yes"}), "`on`")] {
        let e = s.execute("embroidery.setStopAfter", &p).unwrap_err().to_string();
        assert!(e.contains(refused), "{p}: {e}");
    }
    assert!(matches!(s.execute("embroidery.setStopAfter", &json!({"ids": [999_999]})), Err(crate::EngineError::NoNode(_))));
    run(&mut s, "select.none", json!({}));
    assert!(s.execute("embroidery.setStopAfter", &json!({})).is_err(), "nothing selected");
}
