//! The stitch commands' paths of stitches ([`super::needles`]): every kind reads and writes back as
//! it was, hostile anchors are refused before any arithmetic, the selection maps to needle points
//! and back; and Move Needle Points.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Mark, StitchRun};
use vectorcraft_doc::{Node, NodeId};
use vectorcraft_testkit::embroidery as fx;

use super::needles::{self, Unfit};
use super::test_support::{PT_PER_MM, blank, run};
use crate::Session;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The active document's exports in every format.
fn exports(s: &mut Session) -> Vec<Value> {
    vectorcraft_stitch::format::Format::ALL
        .iter()
        .map(|f| s.execute("document.export", &json!({"format": f.id()})).unwrap()["dataBase64"].clone())
        .collect()
}

fn node(s: &Session, id: u64) -> Node {
    s.doc().unwrap().doc.node(NodeId(id)).unwrap().clone()
}

/// The places (subpath, anchor) marks are kept at.
fn places(marks: &[Mark]) -> Vec<(usize, usize)> {
    marks.iter().map(|m| (m.subpath, m.anchor)).collect()
}

fn stitches(n: &Node) -> StitchRun {
    n.extras().embroidery.as_deref().and_then(|e| e.stitches.clone()).unwrap()
}

/// A path of literal stitches through `points` (millimetres), one run, in the first layer → its id.
fn path_mm(s: &mut Session, points: &[(f64, f64)], marks: StitchRun) -> u64 {
    let d: Vec<String> =
        points.iter().enumerate().map(|(i, (x, y))| format!("{} {} {}", if i == 0 { "M" } else { "L" }, x * PT_PER_MM, y * PT_PER_MM)).collect();
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"color": "#c81e1e"}));
    let id = run(s, "path.create", json!({"d": d.join(" ")}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    s.edit("Stitches", |d, _| {
        let n = d.node_mut(NodeId(id)).unwrap();
        n.extras_mut().embroidery = Some(Box::new(vectorcraft_doc::embroidery::Embroidery { stitches: Some(marks), ..Default::default() }));
        Ok(())
    })
    .unwrap();
    id
}

/// Needle point `k` of path `id`, millimetres to a millionth (what points to millimetres and back
/// leaves aside).
fn at(s: &Session, id: u64, k: usize) -> (f64, f64) {
    let st = needles::read(&node(s, id)).unwrap();
    let (r, i) = st.index().locate(k).unwrap();
    let p = st.runs[r].needles[i].p;
    let mm = |v: f64| (v / PT_PER_MM * 1e6).round() / 1e6;
    (mm(p.x), mm(p.y))
}

/// Every path of stitches of the manual fixture (the runs of the four opened designs, the fixture's
/// own runs of one needle point, a tack, cuts and stops, and the converted objects' runs with their
/// exact millimetres) reads and writes back as it was, path and marks, and the exports with it;
/// only the run edited into curves is written straight.
#[test]
fn every_kind_of_path_reads_and_writes_back_as_it_was() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let before = exports(&mut s);
    let doc = s.doc().unwrap().doc.clone();
    let mut paths = Vec::new();
    for l in &doc.layers {
        l.walk(&mut |n| {
            if let Ok(st) = needles::read(n) {
                paths.push((n.clone(), st));
            }
        });
    }
    assert!(paths.len() > 25, "{} paths", paths.len());
    let (mut exact, mut tack) = (0, 0);
    for (n, st) in &paths {
        let mut back = n.clone();
        assert!(needles::write(&mut back, st.clone()));
        if n.id.0 == m.curved {
            continue;
        }
        assert_eq!(back.path_data(), n.path_data(), "{}: its path", n.id.0);
        assert_eq!(stitches(&back), stitches(n), "{}: its marks", n.id.0);
        exact += usize::from(!stitches(n).exact.is_empty());
        tack += usize::from(stitches(n).double_point);
    }
    assert!(exact > 0 && tack == 1, "converted runs with exact millimetres ({exact}) and the tack ({tack}) among them");
    // Written back in the document: the same exports.
    s.edit("Written Back", |d, _| {
        for (n, st) in &paths {
            assert!(needles::write(d.node_mut(n.id).unwrap(), st.clone()));
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(exports(&mut s), before);
}

/// A path whose anchors aren't numbers or lie past the document's coordinates is refused before any
/// arithmetic: read, addressed (an error), selected (passed over).
#[test]
fn hostile_anchors_are_refused_before_any_arithmetic() {
    for bad in [f64::NAN, f64::INFINITY, 1e308, -4.0e6 - 1.0] {
        let mut s = blank();
        let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun::default());
        s.edit("Hostile", |d, _| {
            let n = d.node_mut(NodeId(id)).unwrap();
            if let vectorcraft_doc::NodeKind::Path { path, .. } = &mut n.kind {
                path.subpaths[0].anchors[1].p.x = bad;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(needles::read(&node(&s, id)).err(), Some(Unfit::OutOfRange), "{bad}");
        let r = s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": 0}], "dx": 1}));
        assert!(matches!(r, Err(crate::EngineError::BadParams { .. })), "{bad}: {r:?}");
        run(&mut s, "select.set", json!({"ids": [id]}));
        let r = s.execute("embroidery.moveNeedlePoints", &json!({"dx": 1}));
        assert!(r.is_err(), "{bad}: nothing else to move: {r:?}");
    }
}

/// The selection maps to needle points and back: a run stored as a line to its own start is one
/// needle point whichever of its two anchors is selected, and selecting it selects both.
#[test]
fn the_selection_maps_to_needle_points_and_back() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    s.edit("Two Runs", |d, _| {
        let n = d.node_mut(NodeId(id)).unwrap();
        if let vectorcraft_doc::NodeKind::Path { path, .. } = &mut n.kind {
            let p = vectorcraft_geom::Point::new(20.0 * PT_PER_MM, 20.0 * PT_PER_MM);
            path.subpaths.push(vectorcraft_geom::SubPath { anchors: vec![vectorcraft_geom::Anchor::corner(p); 2], closed: false });
        }
        Ok(())
    })
    .unwrap();
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!(st.len(), 4, "three, and one of a line to its own start");
    let mut sel = s.doc().unwrap().selection.clone();
    sel.objects = vec![NodeId(id)];
    sel.anchors.insert(NodeId(id), [(1, 1), (0, 2)].into_iter().collect());
    let ix = st.index();
    assert_eq!(needles::selected_in(&sel, NodeId(id), &ix), [2, 3].into_iter().collect());
    needles::select_in(&mut sel, NodeId(id), &ix, &[3].into_iter().collect());
    assert_eq!(sel.anchors[&NodeId(id)], [(1, 0), (1, 1)].into_iter().collect(), "both anchors of the one needle point");
    needles::select_in(&mut sel, NodeId(id), &ix, &Default::default());
    assert!(!sel.contains(NodeId(id)) && !sel.anchors.contains_key(&NodeId(id)), "nothing of it: it leaves the selection");
    sel.anchors.clear();
    sel.objects = vec![NodeId(id)];
    assert_eq!(needles::selected_in(&sel, NodeId(id), &ix).len(), 4, "selected whole: all of it");
    // An anchor past a run's needle points (left stale by an edit since) is none of them; the
    // second anchor of a line to its own start is its one.
    sel.anchors.insert(NodeId(id), [(0, 9), (1, 1)].into_iter().collect());
    assert_eq!(needles::selected_in(&sel, NodeId(id), &ix), [3].into_iter().collect());
}

/// The index finds a needle point by its place across the runs, passing over a run with none (an
/// edit may leave one before it is written back), and gives the anchors it is written as: a run with
/// none isn't written, and a run of one needle point is two anchors.
#[test]
fn the_index_finds_needle_points_across_runs() {
    let needle = |x: f64| needles::Needle { p: vectorcraft_geom::Point::new(x, 0.0), stop: false, exact: [None, None] };
    let run = |xs: &[f64]| needles::Run { cut_before: false, needles: xs.iter().map(|&x| needle(x)).collect() };
    let st = needles::Stitches {
        runs: vec![run(&[0.0, 1.0, 2.0]), run(&[]), run(&[5.0]), run(&[7.0, 8.0])],
        marks: StitchRun::default(),
        object_after: Default::default(),
    };
    let ix = st.index();
    assert_eq!(ix.len(), 6);
    let found: Vec<_> = (0..7).map(|k| ix.locate(k)).collect();
    assert_eq!(found, [Some((0, 0)), Some((0, 1)), Some((0, 2)), Some((2, 0)), Some((3, 0)), Some((3, 1)), None]);
    assert_eq!((ix.index_of(2, 0), ix.index_of(3, 1), ix.index_of(1, 0), ix.index_of(9, 0)), (3, 5, 3, 6));
    assert_eq!((ix.run_len(1), ix.run_len(2), ix.run_len(3), ix.run_len(4)), (0, 1, 2, 0));
    assert_eq!(ix.anchors_of(3), [(1, 0), (1, 1)], "the run with none isn't written: the one-point run is subpath 1");
    assert_eq!(ix.anchors_of(5), [(2, 1)]);
    assert!(ix.anchors_of(6).is_empty());
    for k in 0..6 {
        let (r, i) = ix.locate(k).unwrap();
        assert_eq!(st.needle((r, i)).unwrap().p.x, [0.0, 1.0, 2.0, 5.0, 7.0, 8.0][k]);
    }
}

/// An address takes only its own keys, and `of` (the needle points the caller counted) refuses one
/// an edit since made stale; the edits that renumber needle points say where those addressed are
/// now (Machine Function, Split Long Stitches), or where each merged stitch leaves from (Delete).
#[test]
fn addresses_are_strict_and_replies_give_new_ones() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (18.0, 10.0), (48.0, 10.0)], StitchRun::default());
    let steps = s.doc().unwrap().history.undo.len();
    for p in [
        json!({"needlePoints": [{"id": id, "index": 0, "to": 7}], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 0, "run": 1}], "dx": 1}),
        json!({"ranges": [{"id": id, "from": 0, "to": 1, "index": 3}], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 0, "of": 5}], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 0, "of": 4}, {"id": id, "index": 1, "of": 3}], "dx": 1}),
        json!({"ranges": [{"id": id, "from": 0, "to": 1, "of": -1}], "dx": 1}),
    ] {
        let r = s.execute("embroidery.moveNeedlePoints", &p);
        assert!(matches!(r, Err(crate::EngineError::BadParams { .. })), "{p}: {r:?}");
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), steps);
    let r = run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 0, "of": 4}], "dx": 0.5}));
    assert_eq!(r["changedCount"], json!(1), "the count right: moved");
    // The 30 mm stitch into needle point 3 split into three: needle point 3 is now 5.
    let r = run(&mut s, "embroidery.splitLongStitches", json!({"needlePoints": [{"id": id, "index": 3, "of": 4}], "maxLength": 10}));
    assert_eq!((r["addedCount"].clone(), r["needlePoints"].clone()), (json!(2), json!([{"id": id, "index": 5}])), "{r}");
    assert_eq!(at(&s, id, 5), (48.0, 10.0));
    // A cut after needle point 1, the run split there and tied: needle point 1 stays 1.
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1, "of": 6}], "function": "trim"}));
    assert_eq!(r["needlePoints"], json!([{"id": id, "index": 1}]), "{r}");
    // 10.5 14 13 14 ✂ 18 19 18 28 38 48: needle point 8 deleted, the stitch from needle point 7
    // goes on.
    let len = needles::read(&node(&s, id)).unwrap().len();
    assert_eq!(len, 10);
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": id, "index": 8, "of": len}]}));
    assert_eq!(r["merged"], json!([{"id": id, "index": 7}]), "{r}");
}

/// Each edit's label constant (its undo step's name) is the command's own label, as the command
/// table gives it; Machine Function names its step by what it did, a cut or a stop.
#[test]
fn the_edits_name_their_undo_steps_as_their_commands() {
    use super::needle_edits as e;
    let specs = super::specs();
    let label = |id: &str| specs.iter().find(|c| c.id == id).map(|c| c.label);
    for (id, constant) in [
        (e::MOVE, e::MOVE_LABEL),
        (e::INSERT, e::INSERT_LABEL),
        (e::DELETE, e::DELETE_LABEL),
        (e::SPLIT, e::SPLIT_LABEL),
        (e::JOIN, e::JOIN_LABEL),
        (e::SPLIT_LONG, e::SPLIT_LONG_LABEL),
    ] {
        assert_eq!(label(id), Some(constant), "{id}");
    }
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (18.0, 10.0)], StitchRun::default());
    for (function, step) in [("trim", "Cut After Needle Point"), ("stop", "Stop After Needle Point")] {
        run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1}], "function": function, "ties": false}));
        assert_eq!(s.doc().unwrap().history.undo.last().map(|h| h.label.clone()), Some(step.to_string()));
    }
}

/// Move Needle Points moves the needle points addressed by millimetres, or one to a point, one undo
/// step that undo takes back with the selection; nothing moved is no step; the live check measures
/// the stitches into and out of each; a two-point run brought to one spot sews both (a tack), and
/// apart again it sews two.
#[test]
fn needle_points_move_by_millimetres() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let steps = s.doc().unwrap().history.undo.len();
    let r = run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 1}], "dx": 0.5, "dy": -1}));
    assert_eq!((r["changedCount"].clone(), r["pathCount"].clone()), (json!(1), json!(1)), "{r}");
    let (x, y) = at(&s, id, 1);
    assert!((x - 12.5).abs() < 1e-9 && (y - 9.0).abs() < 1e-9, "{x} {y}");
    assert_eq!(r["lengths"]["count"], json!(2), "{r}");
    let longest = r["lengths"]["longest"].as_f64().unwrap();
    assert!((longest - (2.5f64.powi(2) + 1.0).sqrt()).abs() < 1e-9, "{r}");
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "one step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(at(&s, id, 1), (12.0, 10.0));
    // To a point; nothing moved is no step.
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 2}], "to": [20, 30]}));
    assert_eq!(at(&s, id, 2), (20.0, 30.0));
    let steps = s.doc().unwrap().history.undo.len();
    let r = run(&mut s, "embroidery.moveNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 2}], "dx": 0}));
    assert_eq!((r["changedCount"].clone(), s.doc().unwrap().history.undo.len()), (json!(0), steps));
    // The selection's needle points by default, and it stays on them.
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 0]]}));
    run(&mut s, "embroidery.moveNeedlePoints", json!({"dy": 2}));
    assert_eq!(at(&s, id, 0), (10.0, 12.0));
    assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(id)], [(0, 0)].into_iter().collect());
    // A tack, and apart again.
    let mut s = blank();
    let two = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": two, "index": 1}], "to": [10, 10]}));
    assert!(stitches(&node(&s, two)).double_point && needles::read(&node(&s, two)).unwrap().len() == 2, "a tack");
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": two, "index": 1}], "dx": 3}));
    assert!(!stitches(&node(&s, two)).double_point && needles::read(&node(&s, two)).unwrap().len() == 2);
}

/// The live check counts each stitch an edit changed once, however many of its ends were addressed,
/// against the document's shortest stitch and the longest a machine sews well.
#[test]
fn the_live_check_counts_each_stitch_once() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (18.0, 10.0), (22.0, 10.0)], StitchRun::default());
    // Needle point 2 brought 0.2 mm from needle point 1, then both moved together: three stitches
    // changed (into 1, between them, out of 2), the short one counted once.
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 2}], "dx": -3.8}));
    let r = run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 1}, {"id": id, "index": 2}], "dy": 0.1}));
    let l = &r["lengths"];
    assert_eq!((l["count"].clone(), l["belowMin"].clone(), l["aboveMax"].clone()), (json!(3), json!(1), json!(0)), "{r}");
    let (shortest, min) = (l["shortest"].as_f64().unwrap(), l["min"].as_f64().unwrap());
    assert!((shortest - 0.2).abs() < 1e-9 && min > shortest, "{r}");
}

/// A run of two needle points brought to one spot in a path of several runs is one needle point
/// (only a path's only run can be a tack): the reply counts what is written, says how many collapsed,
/// measures no stitch of no length, and the selection leaves the needle point that went.
#[test]
fn a_two_point_run_brought_to_one_spot_among_others_is_one_needle_point() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (20.0, 10.0), (22.0, 10.0), (24.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": 2}], "ties": false}));
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 1]]}));
    let r = run(&mut s, "embroidery.moveNeedlePoints", json!({"to": [10, 10]}));
    assert_eq!((r["paths"].clone(), r["collapsedCount"].clone()), (json!([{"id": id, "needlePoints": 4}]), json!(1)), "{r}");
    assert_eq!((r["lengths"]["count"].clone(), r["lengths"]["belowMin"].clone()), (json!(0), json!(0)), "{r}");
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!((st.len(), at(&s, id, 1)), (4, (20.0, 10.0)), "written as it says: 20 is needle point 1 now");
    assert!(!s.doc().unwrap().selection.anchors.contains_key(&NodeId(id)), "the needle point that went left the selection");
}

/// What Move Needle Points refuses, changing nothing: `to` with more than one needle point, or with
/// `dx`; an index past the path's needle points; an object that isn't a path of stitches; a locked
/// one; a needle point taken outside the document's coordinates; nothing addressed or selected.
#[test]
fn move_needle_points_refuses_before_changing_anything() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let art = run(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 20, "height": 20}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    let steps = s.doc().unwrap().history.undo.len();
    for p in [
        json!({"ranges": [{"id": id, "from": 0, "to": 1}], "to": [1, 1]}),
        json!({"needlePoints": [{"id": id, "index": 0}], "to": [1, 1], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 3}], "dx": 1}),
        json!({"needlePoints": [{"id": art, "index": 0}], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 0}], "dx": 2e6}),
        json!({"needlePoints": [{"id": id, "index": -1}], "dx": 1}),
        json!({"ranges": [{"id": id, "from": 2, "to": 1}], "dx": 1}),
        json!({"needlePoints": [{"id": id, "index": 0}], "dx": "far"}),
        json!({"needlePoints": [{"id": id, "index": 0}], "distance": 1}),
        json!({"dx": 1}),
    ] {
        let r = s.execute("embroidery.moveNeedlePoints", &p);
        assert!(r.is_err(), "{p}: {r:?}");
    }
    run(&mut s, "select.set", json!({"ids": [id]}));
    run(&mut s, "object.lock", json!({}));
    let r = s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": 0}], "dx": 1}));
    assert!(r.is_err(), "locked: {r:?}");
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "only the lock");
    assert_eq!(at(&s, id, 0), (10.0, 10.0));
}

/// An index past what any path can hold is refused before it is counted, of a path of stitches or
/// of no object at all: a range from 0 to the largest number, whose count would overflow, included.
/// One within that bound but past the path's needle points is refused for the path; nothing changes.
#[test]
fn indices_past_any_path_are_refused_before_they_are_counted() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let steps = s.doc().unwrap().history.undo.len();
    for target in [id, 0, u64::MAX] {
        for (command, p) in [
            ("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": target, "from": 0, "to": u64::MAX}]})),
            ("embroidery.selectNeedlePoints", json!({"ranges": [{"id": target, "from": 0, "to": u64::MAX}]})),
            ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": target, "from": u32::MAX, "to": u64::MAX}], "dx": 1})),
            ("embroidery.splitRun", json!({"needlePoints": [{"id": target, "index": u64::MAX}]})),
            ("embroidery.setMachineFunction", json!({"needlePoints": [{"id": target, "index": u32::MAX}], "function": "stop"})),
        ] {
            let r = s.execute(command, &p);
            let said = r.as_ref().err().map(ToString::to_string).unwrap_or_default();
            assert!(said.contains("past any path's needle points"), "{command} {p}: {r:?}");
        }
        let r = s.execute("embroidery.moveNeedlePoints", &json!({"ranges": [{"id": target, "from": 0, "to": 1_999_999}], "dx": 1}));
        let refused = match r {
            Err(crate::EngineError::BadParams { msg, .. }) => target == id && msg.contains("needle point 1999999, of 3"),
            Err(crate::EngineError::NoNode(n)) => n.0 == target,
            _ => false,
        };
        assert!(refused, "within the bound, past the path: refused for the path");
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), steps);
    assert_eq!(at(&s, id, 2), (14.0, 10.0));
}

/// A converted run's exact millimetres stay with its needle points: a needle point moved away and
/// back is sewn exactly as before, and so are the others, so the export is the same bytes.
#[test]
fn moved_away_and_back_a_converted_run_exports_the_same() {
    let mut s = blank();
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#c81e1e"}));
    let square =
        run(&mut s, "shape.rectangle", json!({"x": 51.75 * PT_PER_MM, "y": 20.0 * PT_PER_MM, "width": 30.0 * PT_PER_MM, "height": 20.0 * PT_PER_MM}))
            ["id"]
            .as_u64()
            .unwrap();
    run(&mut s, "embroidery.convertToManual", json!({"ids": [square]}));
    let before = exports(&mut s);
    let runs: Vec<u64> = node(&s, square).children().unwrap().iter().map(|n| n.id.0).collect();
    let id = runs[0];
    let all = needles::read(&node(&s, id)).unwrap().len();
    let address = json!([{"id": id, "index": all / 2}, {"id": id, "index": 0}]);
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": address, "dx": 3.3, "dy": 1.7}));
    assert_ne!(exports(&mut s), before);
    run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": address, "dx": -3.3, "dy": -1.7}));
    assert_eq!(exports(&mut s), before, "moved back: the same bytes");
}

/// Insert Needle Points: at the middle of the stitch arriving at a needle point, the points given
/// at either end, or one midway after each needle point selected that a stitch leaves (a run's last
/// passed by); the new ones take their places, those after move up, a stop stays after the needle
/// point it followed, and the new needle points are left selected; one undo step, none when nothing
/// is added.
#[test]
fn needle_points_are_inserted() {
    let mut s = blank();
    let marks = StitchRun { stops: vec![Mark::place(0, 1)], ..StitchRun::default() };
    let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (14.0, 14.0)], marks);
    let r = run(&mut s, "embroidery.insertNeedlePoints", json!({"id": id, "index": 1}));
    assert_eq!((r["changedCount"].clone(), r["inserted"].clone()), (json!(1), json!([{"id": id, "index": 1}])), "{r}");
    assert_eq!((at(&s, id, 1), at(&s, id, 2)), ((12.0, 10.0), (14.0, 10.0)));
    assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(id)], [(0, 1)].into_iter().collect(), "the new one selected");
    assert_eq!(places(&stitches(&node(&s, id)).stops), [(0, 2)], "the stop stays after the needle point it followed");
    run(&mut s, "embroidery.insertNeedlePoints", json!({"id": id, "index": 0, "at": [[5, 5], [7, 7]]}));
    run(&mut s, "embroidery.insertNeedlePoints", json!({"id": id, "index": 6, "at": [[20, 20]]}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!(st.len(), 7);
    assert_eq!((at(&s, id, 0), at(&s, id, 1), at(&s, id, 2), at(&s, id, 6)), ((5.0, 5.0), (7.0, 7.0), (10.0, 10.0), (20.0, 20.0)));
    // Midway after the selected needle points that a stitch leaves.
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 0], [0, 6]]}));
    let steps = s.doc().unwrap().history.undo.len();
    let r = run(&mut s, "embroidery.insertNeedlePoints", json!({}));
    assert_eq!(r["inserted"], json!([{"id": id, "index": 1}]), "the last needle point passed by: {r}");
    assert_eq!(at(&s, id, 1), (6.0, 6.0));
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1);
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 7]]}));
    let r = run(&mut s, "embroidery.insertNeedlePoints", json!({}));
    assert_eq!((r["changedCount"].clone(), s.doc().unwrap().history.undo.len()), (json!(0), steps + 1), "nothing added, no step");
    // Refused: no stitch arrives where a run starts; `at` without a place; too many points.
    for p in [
        json!({"id": id, "index": 0}),
        json!({"at": [[1, 1]]}),
        json!({"id": id, "index": 99, "at": [[1, 1]]}),
        json!({"id": id, "index": 1, "at": []}),
        json!({"id": id, "index": 1, "at": vec![[1.0, 1.0]; 10_001]}),
        json!({"id": id, "index": 1, "at": [[1e308, 1]]}),
        json!({"id": id}),
        json!({"id": id, "index": 1, "needlePoints": [{"id": id, "index": 0}]}),
    ] {
        assert!(s.execute("embroidery.insertNeedlePoints", &p).is_err(), "{p}");
    }
}

/// Delete Needle Points: the stitches beside a deleted needle point become one, the others stay;
/// a stop after it passes to the needle point before it; a run left empty goes with its cut passed
/// on; the path leaves the selection; insert then delete gives the path back.
#[test]
fn deleted_needle_points_merge_their_neighbours() {
    let mut s = blank();
    // A stop after needle point 2, as this version writes it: where it lies, with the path's shape.
    let lies = vectorcraft_geom::Point::new(14.0 * PT_PER_MM, 10.0 * PT_PER_MM);
    let marks = StitchRun { stops: vec![Mark::at(0, 2, lies)], shape: Some([1, 5]), ..StitchRun::default() };
    let points = [(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (16.0, 10.0), (18.0, 10.0)];
    let id = path_mm(&mut s, &points, marks);
    let original = node(&s, id);
    run(&mut s, "embroidery.insertNeedlePoints", json!({"id": id, "index": 3}));
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": id, "index": 3}]}));
    assert_eq!(r["changedCount"], json!(1), "{r}");
    let back = node(&s, id);
    assert_eq!((back.path_data(), stitches(&back)), (original.path_data(), stitches(&original)), "insert then delete: as it was");
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": id, "index": 2}]}));
    assert_eq!((at(&s, id, 1), at(&s, id, 2)), ((12.0, 10.0), (16.0, 10.0)), "the others stay");
    assert_eq!(r["lengths"]["count"], json!(2), "the merged stitch, and the one after: {r}");
    assert_eq!(r["lengths"]["longest"], json!(4.0));
    assert_eq!(places(&stitches(&node(&s, id)).stops), [(0, 1)], "the stop passes to the needle point before");
    assert_eq!(r["moved"], json!([{"function": "stop", "id": id, "index": 1}]));
    assert!(!s.doc().unwrap().selection.contains(NodeId(id)));
    // A run left empty goes, its cut passed to the run after it.
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun { trims: vec![Mark::place(1, 0), Mark::place(2, 0)], ..StitchRun::default() });
    s.edit("Runs", |d, _| {
        let n = d.node_mut(NodeId(id)).unwrap();
        if let vectorcraft_doc::NodeKind::Path { path, .. } = &mut n.kind {
            for y in [20.0, 30.0] {
                let a = |x: f64| vectorcraft_geom::Anchor::corner(vectorcraft_geom::Point::new(x * PT_PER_MM, y * PT_PER_MM));
                path.subpaths.push(vectorcraft_geom::SubPath { anchors: vec![a(10.0), a(12.0)], closed: false });
            }
        }
        Ok(())
    })
    .unwrap();
    run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": 2, "to": 3}]}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!((st.runs.len(), st.len()), (2, 4));
    assert_eq!(places(&stitches(&node(&s, id)).trims), [(1, 0)], "one cut before the run after it, not two");
}

/// A path of stitches left with no needle point is removed: its cut and its stop pass to the path
/// sewn before it in its parent (the cut, when there is none, to the one after it, before it), or
/// are dropped, and the reply says which; one undo step brings it all back.
#[test]
fn a_path_left_empty_passes_its_cut_and_stop_on() {
    let mut s = blank();
    let marks = StitchRun { trim_after: true, stop_after: true, ..StitchRun::default() };
    let a = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(20.0, 10.0), (22.0, 10.0)], marks.clone());
    let c = path_mm(&mut s, &[(30.0, 10.0), (32.0, 10.0)], StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [a, b, c]}));
    run(&mut s, "object.group", json!({}));
    let saved = vectorcraft_format::save(&s.doc().unwrap().doc, false);
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": b, "from": 0, "to": 1}]}));
    assert_eq!((r["removed"].clone(), r["changedCount"].clone()), (json!([b]), json!(2)), "{r}");
    assert!(s.doc().unwrap().doc.node(NodeId(b)).is_none());
    let to_a = stitches(&node(&s, a));
    assert!(to_a.trim_after && to_a.stop_after, "both passed to the path before it: {r}");
    assert_eq!(r["moved"], json!([{"function": "trim", "id": a, "index": 1}, {"function": "stop", "id": a, "index": 1}]));
    run(&mut s, "edit.undo", json!({}));
    assert!(vectorcraft_format::save(&s.doc().unwrap().doc, false) == saved, "one undo step brings it all back");
    // The first of its parent: its cut goes before the one after it, its stop is dropped.
    run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": a, "from": 0, "to": 1}, {"id": b, "from": 0, "to": 1}]}));
    assert!(stitches(&node(&s, c)).trim_before);
}

/// A run left empty at a path's start or end passes the cut before it to the path's own cut before
/// or after it, and the reply says so; a path removed says once where its stop went; a neighbour
/// edited in the same call is numbered as after the edit; `path.deleteAnchors` (the Delete key's
/// command) deletes needle points as stitches too.
#[test]
fn delete_keeps_the_cuts_at_a_paths_ends_and_says_each_once() {
    // [10 12] ✂ [20 22]: the second run deleted, then (undone) the first.
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (20.0, 10.0), (22.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1}], "function": "trim", "ties": false}));
    assert_eq!(places(&stitches(&node(&s, id)).trims), [(1, 0)]);
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": 2, "to": 3}]}));
    assert_eq!((r["moved"].clone(), r["dropped"].clone()), (json!([{"function": "trim", "id": id, "index": 1}]), json!([])), "{r}");
    assert!(stitches(&node(&s, id)).trim_after, "the cut after the path now");
    run(&mut s, "edit.undo", json!({}));
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 1}]}));
    assert_eq!(r["moved"], json!([{"function": "trim", "id": id, "before": true}]), "{r}");
    assert!(stitches(&node(&s, id)).trim_before, "the cut before the path now");
    // B's stop, on its last needle point, said once: passed to A.
    let mut s = blank();
    let a = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(20.0, 10.0), (22.0, 10.0)], StitchRun { stops: vec![Mark::place(0, 1)], trim_after: true, ..StitchRun::default() });
    let c = path_mm(&mut s, &[(30.0, 10.0), (32.0, 10.0)], StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [a, b, c]}));
    run(&mut s, "object.group", json!({}));
    let r = run(&mut s, "embroidery.deleteNeedlePoints", json!({"ranges": [{"id": a, "from": 2, "to": 2}, {"id": b, "from": 0, "to": 1}]}));
    let moved = json!([{"function": "trim", "id": a, "index": 1}, {"function": "stop", "id": a, "index": 1}]);
    assert_eq!((r["moved"].clone(), r["dropped"].clone()), (moved, json!([])), "A numbered as after the edit, B's stop once: {r}");
    // The Delete key's command.
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 1]]}));
    run(&mut s, "path.deleteAnchors", json!({}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!((st.runs.len(), st.len()), (1, 2), "neighbours merged, no gap");
}

/// A stop set after a path's last needle point is its stop after it: the Object tab shows it as Stop
/// After, and Stop After off clears it, so the export stops there no more.
#[test]
fn a_stop_at_a_paths_end_is_its_stop_after() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    path_mm(&mut s, &[(20.0, 10.0), (22.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 2}], "function": "stop"}));
    let marks = stitches(&node(&s, id));
    assert!(marks.stops.is_empty() && marks.stop_after, "written as its stop after it");
    let info = run(&mut s, "embroidery.info", json!({"ids": [id]}));
    assert_eq!(info["stopAfter"]["value"], json!(true), "{info}");
    let stats = |s: &mut Session| run(s, "embroidery.stats", json!({}))["stops"].as_u64().unwrap();
    assert_eq!(stats(&mut s), 1);
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [id], "on": false}));
    assert_eq!(stats(&mut s), 0, "cleared by Stop After");
}

/// Scissors, at an anchor or on a segment, cuts a run as Cut Path does: the halves take the marks of
/// the needle points they hold (a stop on the cut point stays on the first), the first the cut before
/// the run, the second the cut and the stop after it, and a file's run's second half starts after
/// the cut point. A run of several subpaths cut where it lies (Scissors, Cut Path at Anchors) keeps
/// its pieces as subpaths, the cut point once, its cuts before the runs they were before.
#[test]
fn scissors_and_cuts_in_place_cut_runs_as_stitches() {
    let points = [(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (16.0, 10.0), (18.0, 10.0)];
    let marks = || StitchRun {
        trim_before: true,
        trim_after: true,
        stop_after: true,
        stops: vec![Mark::place(0, 2), Mark::place(0, 3)],
        ..StitchRun::default()
    };
    // At an anchor: 10 12 14 | 14 16 18.
    let mut s = blank();
    let id = path_mm(&mut s, &points, marks());
    let r = run(&mut s, "path.split", json!({"id": id, "anchor": 2}));
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let (first, last) = (stitches(&node(&s, ids[0])), stitches(&node(&s, ids[1])));
    // 14's stop on the first half only, after its last needle point: its stop after it.
    assert!(first.trim_before && !first.trim_after && first.stop_after && first.stops.is_empty(), "{first:?}");
    assert!(!last.trim_before && last.trim_after && last.stop_after, "{last:?}");
    assert_eq!(places(&last.stops), [(0, 0)], "16's on the second");
    assert_eq!(at(&s, ids[1], 0), (16.0, 10.0), "the second half starts after the cut point");
    assert_eq!(jumps_and_stitches(&mut s).1, 5);
    // On a segment: a needle point made by the cut, sewn once.
    let mut s = blank();
    let id = path_mm(&mut s, &points, marks());
    let r = run(&mut s, "path.split", json!({"id": id, "segment": 1, "t": 0.5}));
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!((at(&s, ids[0], 2), at(&s, ids[1], 0)), ((13.0, 10.0), (14.0, 10.0)));
    assert_eq!(jumps_and_stitches(&mut s).1, 6, "five and the one the cut made");
    // In place: a converted run of two subpaths, a cut before the second, cut in its first.
    let mut s = blank();
    let id = path_mm(&mut s, &points, StitchRun { manual: true, ..StitchRun::default() });
    run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": 3}], "ties": false}));
    run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 2}], "function": "trim", "ties": false}));
    assert_eq!(places(&stitches(&node(&s, id)).trims), [(1, 0)]);
    for (command, p) in [("path.split", json!({"id": id, "subpath": 0, "anchor": 1})), ("path.cutAtAnchors", json!({}))] {
        let mut t = Session::new();
        t.add_document(s.doc().unwrap().doc.as_ref().clone(), None);
        if command == "path.cutAtAnchors" {
            run(&mut t, "select.anchors", json!({"id": id, "anchors": [[0, 1]]}));
        }
        run(&mut t, command, p);
        let st = needles::read(&node(&t, id)).unwrap();
        let runs: Vec<(bool, usize)> = st.runs.iter().map(|r| (r.cut_before, r.needles.len())).collect();
        assert_eq!(runs, [(false, 2), (false, 1), (true, 2)], "{command}: 10 12 | 14 ✂ 16 18");
        assert_eq!(st.len(), 5, "{command}: 12 kept once");
    }
}

/// With ties on (the default), a function set after a needle point is found again at that needle
/// point, past the tie-off that follows it: the same call toggles it off (the runs stay split) and on
/// again without splitting or tying again, so set then unset gives the runs back, cut or stopped
/// nowhere; the reply and the selection stay on the needle point addressed.
#[test]
fn a_function_set_with_ties_toggles_at_its_needle_point() {
    for function in ["trim", "stop"] {
        let mut s = blank();
        let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (18.0, 10.0), (22.0, 10.0)], StitchRun::default());
        run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 1]]}));
        let at = |s: &mut Session| run(s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1}], "function": function}));
        let shape = |s: &Session| needles::read(&node(s, id)).unwrap().runs.iter().map(|r| r.needles.len()).collect::<Vec<_>>();
        let r = at(&mut s);
        assert_eq!(
            (r["on"].clone(), r["splitCount"].clone(), r["needlePoints"].clone()),
            (json!(true), json!(1), json!([{"id": id, "index": 1}])),
            "{r}"
        );
        assert_eq!(shape(&s), [4, 4], "{function}: 10 14 13 14 | 18 19 18 22");
        assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(id)], [(0, 1)].into_iter().collect(), "the selection stays on 14");
        let records =
            |s: &mut Session| records(s, if function == "trim" { vectorcraft_stitch::Command::Trim } else { vectorcraft_stitch::Command::Stop });
        let set = records(&mut s);
        let r = at(&mut s);
        assert_eq!(
            (r["on"].clone(), r["changedCount"].clone(), r["splitCount"].clone()),
            (json!(false), json!(1), json!(0)),
            "{function}: off at 14: {r}"
        );
        assert_eq!(shape(&s), [4, 4], "{function}: the runs stay split");
        assert_eq!(records(&mut s), set - 1, "{function}: no longer there");
        let r = at(&mut s);
        assert_eq!((r["on"].clone(), r["splitCount"].clone()), (json!(true), json!(0)), "{function}: on again, no second split: {r}");
        assert_eq!((shape(&s), records(&mut s)), (vec![4, 4], set), "{function}");
    }
}

/// The plan's records of the active document: how many jumps and needle-down stitches.
fn jumps_and_stitches(s: &mut Session) -> (usize, usize) {
    let p = s.embroidery_plan(&super::Options::default()).unwrap();
    let all: Vec<_> = p.blocks.iter().flat_map(|b| b.stitches.iter()).collect();
    (
        all.iter().filter(|s| s.command == vectorcraft_stitch::Command::Jump).count(),
        all.iter().filter(|s| s.command == vectorcraft_stitch::Command::Stitch).count(),
    )
}

/// Split Run lifts the needle before a needle point (one jump more, nothing else sewn changes),
/// Join Runs joins it again (split then join gives the path back); with ties (the default) the run
/// before is tied off and the new one tied in, 1 mm back and forward; a run's first is passed by;
/// the selection follows its needle points.
#[test]
fn runs_split_and_join() {
    let mut s = blank();
    let points = [(10.0, 10.0), (14.0, 10.0), (18.0, 10.0), (22.0, 10.0)];
    let id = path_mm(&mut s, &points, StitchRun::default());
    let original = node(&s, id);
    let (jumps, sewn) = jumps_and_stitches(&mut s);
    let r = run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": 2}, {"id": id, "index": 0}], "ties": false}));
    assert_eq!((r["changedCount"].clone(), r["unchangedCount"].clone()), (json!(1), json!(1)), "{r}");
    let jump = &r["jumps"][0];
    assert_eq!((jump["id"].clone(), jump["index"].clone()), (json!(id), json!(2)), "{r}");
    assert!((jump["length"].as_f64().unwrap() - 4.0).abs() < 1e-9, "{r}");
    assert_eq!(needles::read(&node(&s, id)).unwrap().runs.len(), 2);
    assert_eq!(jumps_and_stitches(&mut s), (jumps + 1, sewn), "one jump more, the same needle points");
    let r = run(&mut s, "embroidery.joinRuns", json!({"needlePoints": [{"id": id, "index": 2}]}));
    assert_eq!(r["changedCount"], json!(1), "{r}");
    let back = node(&s, id);
    assert_eq!((back.path_data(), stitches(&back)), (original.path_data(), stitches(&original)), "split then join: as it was");
    // With ties: a tie-off and a tie-in, two needle points each; the selection follows.
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 3]]}));
    run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": 2}]}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!(st.runs.iter().map(|r| r.needles.len()).collect::<Vec<_>>(), [4, 4]);
    assert_eq!((at(&s, id, 2), at(&s, id, 3)), ((13.0, 10.0), (14.0, 10.0)), "tied off: 1 mm back and forward");
    assert_eq!((at(&s, id, 4), at(&s, id, 5), at(&s, id, 6), at(&s, id, 7)), ((18.0, 10.0), (19.0, 10.0), (18.0, 10.0), (22.0, 10.0)), "tied in");
    assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(id)], [(1, 3)].into_iter().collect(), "the selected needle point at its new place");
}

/// Joined paths carry the selection to where its needle points went: a needle point after one kept
/// once (a later path starting where the one before ends) is selected where it is, not one on; paths
/// selected whole leave the joined path selected whole, not every anchor of it.
#[test]
fn joined_paths_keep_the_selection_on_its_needle_points() {
    let mut s = blank();
    // As Scissors leaves a run cut at a2: the second piece starts where the first ends.
    let a = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(14.0, 10.0), (16.0, 10.0), (18.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": b, "index": 1}]}));
    let r = run(&mut s, "embroidery.joinRuns", json!({"ids": [a, b]}));
    assert_eq!(r["paths"], json!([{"id": a, "needlePoints": 5}, {"id": b, "needlePoints": 0}]), "{r}");
    assert_eq!(r["pathCount"], json!(2), "the joined path and the one it took in");
    let sel = s.doc().unwrap().selection.clone();
    assert_eq!(sel.anchors[&NodeId(a)], [(0, 3)].into_iter().collect(), "16 mm: index 3 of the joined path, not 4");
    assert_eq!(at(&s, a, 3), (16.0, 10.0));
    // Selected whole: the joined path selected whole.
    let mut s = blank();
    let a = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(20.0, 10.0), (22.0, 10.0)], StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    run(&mut s, "embroidery.joinRuns", json!({"ids": [a, b]}));
    let sel = s.doc().unwrap().selection.clone();
    assert!(sel.objects == [NodeId(a)] && !sel.anchors.contains_key(&NodeId(a)), "{sel:?}");
}

/// Every edit keeps to one set of rules: no path past the most needle points a design may have (a
/// split with its ties, a cut or a stop inside a run, a join), and what of the selection it passed
/// over said, Join Runs' too.
#[test]
fn every_edit_keeps_to_the_cap_and_says_what_it_passed_over() {
    let mut s = blank();
    let big = path_mm(&mut s, &[(0.0, 0.0), (1.0, 0.0)], StitchRun::default());
    let small = path_mm(&mut s, &[(5.0, 5.0), (6.0, 5.0)], StitchRun::default());
    // One run of one needle point short of the most a path may hold, on a 0.1 mm grid.
    let most = needles::MAX_NEEDLE_POINTS;
    s.edit("Big", |d, _| {
        if let vectorcraft_doc::NodeKind::Path { path, .. } = &mut d.node_mut(NodeId(big)).unwrap().kind {
            let at = |i: usize| vectorcraft_geom::Point::new((i % 1000) as f64 * 0.1 * PT_PER_MM, (i / 1000) as f64 * 0.1 * PT_PER_MM);
            path.subpaths[0].anchors = (0..most - 1).map(|i| vectorcraft_geom::Anchor::corner(at(i))).collect();
        }
        Ok(())
    })
    .unwrap();
    let steps = s.doc().unwrap().history.undo.len();
    for (command, p) in [
        ("embroidery.splitRun", json!({"needlePoints": [{"id": big, "index": 5}]})),
        ("embroidery.setMachineFunction", json!({"needlePoints": [{"id": big, "index": 5}], "function": "trim"})),
        ("embroidery.setMachineFunction", json!({"needlePoints": [{"id": big, "index": 5}], "function": "stop"})),
        ("embroidery.joinRuns", json!({"ids": [big, small]})),
    ] {
        let r = s.execute(command, &p);
        let said = r.as_ref().err().map(ToString::to_string).unwrap_or_default();
        assert!(said.contains(&format!("more than {most} needle points")), "{command}: {r:?}");
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), steps, "nothing changed");
    // Join Runs by needle points says what it passed over, as every edit does.
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0), (18.0, 10.0)], StitchRun::default());
    run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": 1}], "ties": false}));
    let art = run(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 20, "height": 20}))["id"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"ids": [id, art]}));
    let r = run(&mut s, "embroidery.joinRuns", json!({}));
    assert_eq!((r["changedCount"].clone(), r["passedOver"].clone()), (json!(1), json!([{"id": art, "reason": "notStitches"}])), "{r}");
}

/// Join Runs joins paths of stitches sewn one right after another (next to one another in one
/// parent, in one colour, of one kind) into the first, never reversed, dropping a cut at the gap;
/// others are refused, with a message the interface translates.
#[test]
fn paths_join_when_sewn_one_after_another() {
    let mut s = blank();
    let a = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0)], StitchRun { trim_after: true, ..StitchRun::default() });
    let b = path_mm(&mut s, &[(20.0, 10.0), (22.0, 10.0)], StitchRun { stop_after: true, ..StitchRun::default() });
    let c = path_mm(&mut s, &[(30.0, 10.0), (32.0, 10.0)], StitchRun { manual: true, ..StitchRun::default() });
    let message = |r: crate::Result<Value>| match r {
        Err(crate::EngineError::Other(m)) => m,
        r => panic!("{r:?}"),
    };
    assert_eq!(message(s.execute("embroidery.joinRuns", &json!({"ids": [b, a]}))), super::needle_edits::NOT_JOINABLE, "not in the order sewn");
    assert_eq!(message(s.execute("embroidery.joinRuns", &json!({"ids": [b, c]}))), super::needle_edits::NOT_JOINABLE, "a file's and a manual");
    let r = run(&mut s, "embroidery.joinRuns", json!({"ids": [a, b]}));
    assert_eq!((r["removed"].clone(), r["dropped"].clone()), (json!([b]), json!([{"function": "trim", "id": b}])), "{r}");
    let joined = needles::read(&node(&s, a)).unwrap();
    assert_eq!((joined.runs.len(), joined.len()), (1, 4), "one run: the jump a stitch");
    assert_eq!((at(&s, a, 1), at(&s, a, 2)), ((12.0, 10.0), (20.0, 10.0)), "never reversed");
    let marks = stitches(&node(&s, a));
    assert!(!marks.trim_after && marks.stop_after, "the last one's stop after it");
    assert!(s.doc().unwrap().doc.node(NodeId(b)).is_none());
}

/// The plan's records of the active document of `command`'s kind.
fn records(s: &mut Session, command: vectorcraft_stitch::Command) -> usize {
    let p = s.embroidery_plan(&super::Options::default()).unwrap();
    p.blocks.iter().flat_map(|b| b.stitches.iter()).filter(|s| s.command == command).count()
}

/// Machine Function cuts the thread, or stops the machine, after a needle point: inside a run the
/// run splits there, tied off and in (or not, with `ties: false`); at the path's end it is the
/// path's Trim After or Stop After, the object's own included when turned off; left out, `on`
/// turns the function on unless every needle point has it, so a second call takes it off.
#[test]
fn machine_functions_go_after_needle_points() {
    use vectorcraft_stitch::Command;
    let mut s = blank();
    let points = [(10.0, 10.0), (14.0, 10.0), (18.0, 10.0), (22.0, 10.0)];
    let id = path_mm(&mut s, &points, StitchRun::default());
    let trims = records(&mut s, Command::Trim);
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1}], "function": "trim"}));
    assert_eq!((r["on"].clone(), r["changedCount"].clone(), r["splitCount"].clone()), (json!(true), json!(1), json!(1)), "{r}");
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!(st.runs.iter().map(|r| (r.cut_before, r.needles.len())).collect::<Vec<_>>(), [(false, 4), (true, 4)], "split, tied off and in, cut");
    assert_eq!(records(&mut s, Command::Trim), trims + 1);
    // Left out, `on` takes it off again: the runs stay split.
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 3}], "function": "trim"}));
    assert_eq!(r["on"], json!(false), "{r}");
    assert_eq!(needles::read(&node(&s, id)).unwrap().runs.iter().map(|r| r.cut_before).collect::<Vec<_>>(), [false, false]);
    // A stop inside a run, without ties: the machine stops between two needle points.
    let mut s = blank();
    let id = path_mm(&mut s, &points, StitchRun::default());
    run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 1}], "function": "stop", "ties": false}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!(st.runs.iter().map(|r| r.needles.len()).collect::<Vec<_>>(), [2, 2]);
    assert!(st.runs[0].needles[1].stop);
    assert_eq!(records(&mut s, Command::Stop), 1);
    // At the path's end: its Trim After, the object's own cleared when off.
    let mut s = blank();
    let id = path_mm(&mut s, &points, StitchRun::default());
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id], "on": true}));
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 3}], "function": "trim"}));
    assert_eq!(r["on"], json!(false), "it has one already: {r}");
    let n = node(&s, id);
    assert!(!stitches(&n).trim_after && !n.extras().embroidery.as_deref().unwrap().trim_after);
    run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 3}], "function": "stop"}));
    assert!(stitches(&node(&s, id)).stop_after, "a stop after the last needle point: the run's stop after it");
    for p in [
        json!({"needlePoints": [{"id": id, "index": 0}], "function": "colorChange"}),
        json!({"needlePoints": [{"id": id, "index": 0}]}),
        json!({"needlePoints": [{"id": id, "index": 0}], "function": "trim", "on": "yes"}),
    ] {
        assert!(s.execute("embroidery.setMachineFunction", &p).is_err(), "{p}");
    }
}

/// Split Long Stitches cuts each long stitch into the fewest equal stitches within the limit (12.7 mm
/// by default), its new needle points on the stitch; with a format, where that format's writer would
/// cut it, so that format's export is the same bytes; the jump between runs is never split.
#[test]
fn long_stitches_split_as_the_writer_would() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (40.0, 10.0), (41.0, 10.0)], StitchRun::default());
    s.edit("A Second Run", |d, _| {
        let n = d.node_mut(NodeId(id)).unwrap();
        if let vectorcraft_doc::NodeKind::Path { path, .. } = &mut n.kind {
            let a = |x: f64| vectorcraft_geom::Anchor::corner(vectorcraft_geom::Point::new(x * PT_PER_MM, 60.0 * PT_PER_MM));
            path.subpaths.push(vectorcraft_geom::SubPath { anchors: vec![a(100.0), a(102.0)], closed: false });
        }
        Ok(())
    })
    .unwrap();
    run(&mut s, "select.set", json!({"ids": [id]}));
    let r = run(&mut s, "embroidery.splitLongStitches", json!({}));
    assert_eq!((r["changedCount"].clone(), r["addedCount"].clone(), r["limit"].clone()), (json!(1), json!(2), json!({"length": 12.7})), "{r}");
    assert_eq!((at(&s, id, 1), at(&s, id, 2), at(&s, id, 3)), ((20.0, 10.0), (30.0, 10.0), (40.0, 10.0)), "equal stitches on the stitch");
    assert_eq!(needles::read(&node(&s, id)).unwrap().runs[1].needles.len(), 2, "the jump to the second run isn't split");
    run(&mut s, "edit.undo", json!({}));
    // With DST: cut where DST's writer cuts it, the file the same bytes.
    let dst = |s: &mut Session| s.execute("document.export", &json!({"format": "dst"})).unwrap()["dataBase64"].clone();
    let before = dst(&mut s);
    let r = run(&mut s, "embroidery.splitLongStitches", json!({"needlePoints": [{"id": id, "index": 1}], "format": "dst"}));
    assert_eq!((r["addedCount"].clone(), r["limit"]["format"].clone()), (json!(2), json!("dst")), "{r}");
    assert_eq!(dst(&mut s), before, "the same DST");
    let again = run(&mut s, "embroidery.splitLongStitches", json!({"ranges": [{"id": id, "from": 0, "to": 5}], "format": "dst"}));
    assert_eq!(again["changedCount"], json!(0), "nothing longer than a record left: {again}");
    for p in [json!({"maxLength": 0.5}), json!({"maxLength": 20}), json!({"maxLength": 5, "format": "dst"}), json!({"format": "svg"})] {
        assert!(s.execute("embroidery.splitLongStitches", &p).is_err(), "{p}");
    }
    // Every format, from a manual run's needle point kept at a half tenth its document point can't
    // give back (11.55 mm, kept exactly; from the design's left edge, 2 mm, a round trip through
    // points rounds the other way): split on the grid the writer counts the design on, from
    // the millimetres the walk sews, the file the same bytes.
    for f in vectorcraft_stitch::format::Format::ALL {
        let mut s = blank();
        let exact = vec![vectorcraft_doc::embroidery::Exact { subpath: 0, anchor: 0, x: Some(11.55), y: Some(10.05) }];
        path_mm(&mut s, &[(2.0, 3.0), (4.0, 3.0)], StitchRun::default());
        let id = path_mm(&mut s, &[(11.55, 10.05), (262.35, 13.05), (264.0, 13.0)], StitchRun { manual: true, exact, ..StitchRun::default() });
        assert_eq!(needles::read(&node(&s, id)).unwrap().runs[0].needles[0].mm(), vectorcraft_geom::Point::new(11.55, 10.05));
        let export = |s: &mut Session| s.execute("document.export", &json!({"format": f.id()})).unwrap()["dataBase64"].clone();
        let before = export(&mut s);
        let r = run(&mut s, "embroidery.splitLongStitches", json!({"needlePoints": [{"id": id, "index": 1}], "format": f.id()}));
        assert!(r["addedCount"].as_u64().unwrap() >= 1, "{}: {r}", f.id());
        assert_eq!(export(&mut s), before, "{}: the same file, from a half tenth", f.id());
    }
    // Every format: a stitch of 300 mm split where its writer cuts it, its file the same bytes.
    for f in vectorcraft_stitch::format::Format::ALL {
        let mut s = blank();
        let id = path_mm(&mut s, &[(10.0, 10.0), (310.0, 50.0), (312.0, 50.0)], StitchRun::default());
        let export = |s: &mut Session| s.execute("document.export", &json!({"format": f.id()})).unwrap()["dataBase64"].clone();
        let before = export(&mut s);
        let r = run(&mut s, "embroidery.splitLongStitches", json!({"needlePoints": [{"id": id, "index": 1}], "format": f.id()}));
        assert!(r["addedCount"].as_u64().unwrap() >= 1, "{}: {r}", f.id());
        assert_eq!(export(&mut s), before, "{}: the same file", f.id());
    }
}

/// Every record `embroidery.stitches` lists of `s`'s design, page by page.
fn all_records(s: &mut Session, params: Value) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let mut p = params.clone();
        p["offset"] = json!(out.len());
        p["limit"] = json!(10_000);
        let r = run(s, "embroidery.stitches", p);
        let page = r["records"].as_array().unwrap().clone();
        out.extend(page);
        if r["truncated"] == json!(false) {
            return out;
        }
    }
}

/// An opened design keeps each run a path, the cut between two runs before the second: the stitch
/// list numbers the jump onto a run (its first needle point) and the cut before it (the needle point
/// before, in the path before), Machine Function at that needle point finds that cut and turns it
/// off and on, and Join Runs given a path's first needle point joins the path to the one before it
/// (passing over one with none to join to).
#[test]
fn an_opened_designs_gaps_between_runs_can_be_edited() {
    let mut s = blank();
    let a = path_mm(&mut s, &[(10.0, 10.0), (14.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(30.0, 10.0), (34.0, 10.0)], StitchRun { trim_before: true, ..StitchRun::default() });
    let c = path_mm(&mut s, &[(50.0, 10.0), (54.0, 10.0)], StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [a, b, c]}));
    run(&mut s, "object.group", json!({}));
    run(&mut s, "select.none", json!({}));
    let rows = all_records(&mut s, json!({}));
    let row = |kind: &str, node: u64| rows.iter().find(|r| r["kind"] == json!(kind) && r["node"] == json!(node)).cloned();
    let cut = row("trim", a).unwrap_or_else(|| panic!("the cut before b, after a's last: {rows:?}"));
    assert_eq!((cut["index"].clone(), cut["editable"].clone()), (json!(1), json!(true)));
    let jump = row("jump", b).unwrap();
    assert_eq!((jump["index"].clone(), jump["editable"].clone()), (json!(0), json!(true)), "the jump onto b lands on its first");
    // Machine Function at a's last needle point: the file's cut there, turned off, then on again.
    let trims = |s: &mut Session| records(s, vectorcraft_stitch::Command::Trim);
    let before = trims(&mut s);
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": a, "index": 1}], "function": "trim"}));
    assert_eq!((r["on"].clone(), r["changedCount"].clone()), (json!(false), json!(1)), "it has the file's cut: {r}");
    assert!(!stitches(&node(&s, b)).trim_before && trims(&mut s) == before - 1);
    let r = run(&mut s, "embroidery.setMachineFunction", json!({"needlePoints": [{"id": a, "index": 1}], "function": "trim"}));
    assert_eq!(r["on"], json!(true), "{r}");
    assert_eq!(trims(&mut s), before);
    // Selected by its cut's record: the needle point the cut follows.
    let r = run(&mut s, "embroidery.selectNeedlePoints", json!({"kinds": ["trim"]}));
    assert_eq!(r["selectedCount"], json!(1), "{r}");
    assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(a)], [(0, 1)].into_iter().collect());
    run(&mut s, "select.none", json!({}));
    // Join Runs at b's first needle point: b joined to a; then c (now after a) to a.
    let r = run(&mut s, "embroidery.joinRuns", json!({"needlePoints": [{"id": b, "index": 0}]}));
    assert_eq!((r["changedCount"].clone(), r["removed"].clone()), (json!(1), json!([b])), "{r}");
    assert_eq!(needles::read(&node(&s, a)).unwrap().len(), 4);
    run(&mut s, "embroidery.joinRuns", json!({"needlePoints": [{"id": c, "index": 0}]}));
    assert_eq!(needles::read(&node(&s, a)).unwrap().len(), 6);
    let r = run(&mut s, "embroidery.joinRuns", json!({"needlePoints": [{"id": a, "index": 0}]}));
    assert_eq!((r["changedCount"].clone(), r["passedOver"].clone()), (json!(0), json!([{"id": a, "reason": "notJoinable"}])), "{r}");
}

/// `embroidery.stitches` numbers every record from 1, and where the stitch commands can edit one
/// its `{id, index}` is the needle point the record sews (a jump: the one it lands on; a cut or a
/// stop: the one before), in every path of the manual fixture that can be edited (opened designs,
/// the fixture's own runs, converted objects); planned art, a symbol's art and a locked run have
/// none. Its filters and pages.
#[test]
fn the_records_name_the_needle_points_they_sew() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#2080d0"}));
    let art = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 700, "width": 40, "height": 40}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    let rows = all_records(&mut s, json!({}));
    let total = run(&mut s, "embroidery.stitches", json!({"limit": 0}))["recordCount"].as_u64().unwrap() as usize;
    assert_eq!(rows.len(), total);
    assert!(rows.iter().enumerate().all(|(k, r)| r["record"] == json!(k + 1)), "numbered from 1");
    let doc = s.doc().unwrap().doc.clone();
    let mut paths: std::collections::HashMap<u64, needles::Stitches> = Default::default();
    let mut editable = std::collections::HashSet::new();
    for r in &rows {
        let Some(k) = r["index"].as_u64() else { continue };
        let node = r["node"].as_u64().unwrap();
        editable.insert(node);
        let st = paths.entry(node).or_insert_with(|| needles::read(doc.node(NodeId(node)).unwrap()).unwrap());
        let (ri, i) = st.index().locate(k as usize).unwrap_or_else(|| panic!("{r}: no needle point {k}"));
        if r["kind"] == "stitch" || r["kind"] == "jump" {
            let p = st.runs[ri].needles[i].p;
            let at = (r["at"][0].as_f64().unwrap(), r["at"][1].as_f64().unwrap());
            assert!((p.x / PT_PER_MM - at.0).abs() < 1e-6 && (p.y / PT_PER_MM - at.1).abs() < 1e-6, "{r}: {p:?}");
        }
    }
    let c = m.converted;
    for id in [m.opened[0].runs[0], m.one_point, m.double_point, m.stopped, m.trim_after] {
        assert!(editable.contains(&id), "{id} is editable");
    }
    let converted_runs = doc.node(NodeId(c.fill)).unwrap().children().unwrap().iter().map(|n| n.id.0).collect::<Vec<_>>();
    assert!(converted_runs.iter().all(|r| editable.contains(r)), "a converted object's runs");
    for r in rows.iter().filter(|r| [art, m.instance, m.locked].contains(&r["id"].as_u64().unwrap())) {
        assert_eq!((r["index"].clone(), r["editable"].clone()), (Value::Null, json!(false)), "{r}");
    }
    // Filters and pages.
    let jumps = all_records(&mut s, json!({"kinds": ["jump"]}));
    assert!(!jumps.is_empty() && jumps.iter().all(|r| r["kind"] == "jump"));
    let short = all_records(&mut s, json!({"shorterThan": 1.0, "kinds": ["stitch"]}));
    assert!(short.iter().all(|r| r["length"].as_f64().unwrap() < 1.0));
    let some = run(&mut s, "embroidery.stitches", json!({"from": 10, "to": 19, "limit": 4}));
    assert_eq!((some["matchedCount"].clone(), some["truncated"].clone(), some["records"][0]["record"].clone()), (json!(10), json!(true), json!(10)));
    let of_art = all_records(&mut s, json!({"ids": [art]}));
    assert!(!of_art.is_empty() && of_art.iter().all(|r| r["id"] == json!(art)));
    for p in [
        json!({"from": 0}),
        json!({"from": 5, "to": 2}),
        json!({"kinds": ["needle"]}),
        json!({"shorterThan": -1}),
        json!({"selectedOnly": true}),
        json!({"limit": 10_001}),
    ] {
        assert!(s.execute("embroidery.stitches", &p).is_err(), "{p}");
    }
}

/// Select Needle Points selects needle points by the records that sew them (passing over what the
/// stitch commands can't edit) or by their addresses, in four modes, and changes no undo history:
/// every needle point ending a stitch shorter than a length selected, then deleted.
#[test]
fn needle_points_are_selected_by_record_or_address() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (10.2, 10.0), (14.0, 10.0), (14.3, 10.0), (20.0, 10.0)], StitchRun::default());
    run(&mut s, "paint.setFill", json!({"color": "#2080d0"}));
    run(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 40, "height": 40}));
    run(&mut s, "select.none", json!({}));
    let steps = s.doc().unwrap().history.undo.len();
    let r = run(&mut s, "embroidery.selectNeedlePoints", json!({"records": [1, 3]}));
    assert_eq!((r["selectedCount"].clone(), r["pathCount"].clone()), (json!(3), json!(1)), "{r}");
    assert_eq!(s.doc().unwrap().selection.anchors[&NodeId(id)], [(0, 0), (0, 1), (0, 2)].into_iter().collect());
    let all = run(&mut s, "embroidery.selectNeedlePoints", json!({"records": [1, 100000]}));
    assert!(all["passedOver"].as_u64().unwrap() > 0, "the rectangle's records: {all}");
    run(&mut s, "embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": id, "index": 4}], "mode": "toggle"}));
    assert!(!s.doc().unwrap().selection.anchors[&NodeId(id)].contains(&(0, 4)));
    run(&mut s, "embroidery.selectNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 3}], "mode": "subtract"}));
    assert!(!s.doc().unwrap().selection.contains(NodeId(id)), "nothing of it left");
    // The needle points ending stitches shorter than 0.5 mm, then deleted.
    let r = run(&mut s, "embroidery.selectNeedlePoints", json!({"shorterThan": 0.5, "kinds": ["stitch"]}));
    assert_eq!(r["selectedCount"], json!(2), "{r}");
    assert_eq!(s.doc().unwrap().history.undo.len(), steps, "selections take no undo step");
    run(&mut s, "embroidery.deleteNeedlePoints", json!({}));
    assert_eq!(needles::read(&node(&s, id)).unwrap().len(), 3);
    for p in [
        json!({}),
        json!({"records": [1, 2], "needlePoints": [{"id": id, "index": 0}]}),
        json!({"records": [3]}),
        json!({"records": [1, 2], "mode": "xor"}),
    ] {
        assert!(s.execute("embroidery.selectNeedlePoints", &p).is_err(), "{p}");
    }
}

/// The three marked upstream hooks: Delete with needle points of a run selected merges their
/// neighbours (upstream's anchor Delete would leave a gap the machine still sews), while an art
/// path's anchors are deleted as before; Cut Path puts the run's cut before on the first piece,
/// its cut and stop after on the last, and each stop on the piece its needle point is on; Join of
/// two runs never reverses them (upstream's joins the nearest ends), and a run with art is refused.
#[test]
fn delete_cut_path_and_join_treat_runs_as_stitches() {
    let mut s = blank();
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (16.0, 10.0)], StitchRun::default());
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 1]]}));
    run(&mut s, "edit.clear", json!({}));
    let st = needles::read(&node(&s, id)).unwrap();
    assert_eq!((st.runs.len(), st.len()), (1, 3), "one run, its neighbours merged");
    assert!(!s.doc().unwrap().selection.contains(NodeId(id)));
    let art = run(&mut s, "path.create", json!({"d": "M 100 100 L 120 100 L 140 100 L 160 100"}))["id"].as_u64().unwrap();
    run(&mut s, "select.anchors", json!({"id": art, "anchors": [[0, 1]]}));
    run(&mut s, "edit.clear", json!({}));
    assert_eq!(node(&s, art).path_data().unwrap().subpaths.len(), 1, "art as before: the surviving stretch (a lone anchor goes)");
    // Cut Path.
    let mut s = blank();
    let marks = StitchRun {
        trim_before: true,
        trim_after: true,
        stop_after: true,
        stops: vec![Mark::place(0, 1), Mark::place(0, 3)],
        ..StitchRun::default()
    };
    let id = path_mm(&mut s, &[(10.0, 10.0), (12.0, 10.0), (14.0, 10.0), (16.0, 10.0), (18.0, 10.0)], marks);
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 2]]}));
    run(&mut s, "path.cutAtAnchors", json!({}));
    let doc = s.doc().unwrap().doc.clone();
    let pieces: Vec<u64> = doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    let (first, last) = (stitches(&node(&s, pieces[0])), stitches(&node(&s, pieces[1])));
    assert!(first.trim_before && !first.trim_after && !first.stop_after, "{first:?}");
    assert!(!last.trim_before && last.trim_after && last.stop_after, "{last:?}");
    // A file's run: the second piece starts after the needle point the cut made twice, so it isn't
    // sewn twice; the stop after 16 is the second piece's first.
    assert_eq!((at(&s, pieces[0], 2), at(&s, pieces[1], 0)), ((14.0, 10.0), (16.0, 10.0)));
    assert_eq!((places(&first.stops), places(&last.stops)), (vec![(0, 1)], vec![(0, 0)]), "each stop on its piece");
    let needle_points = |s: &mut Session| jumps_and_stitches(s).1;
    assert_eq!(needle_points(&mut s), 5, "five needle points, none sewn twice");
    // Join: never reversed; with art, refused.
    let mut s = blank();
    let a = path_mm(&mut s, &[(10.0, 10.0), (20.0, 10.0)], StitchRun::default());
    let b = path_mm(&mut s, &[(11.0, 12.0), (30.0, 10.0)], StitchRun::default());
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    run(&mut s, "path.join", json!({}));
    let joined = needles::read(&node(&s, a)).unwrap();
    assert_eq!((at(&s, a, 0), at(&s, a, 1), at(&s, a, 2)), ((10.0, 10.0), (20.0, 10.0), (11.0, 12.0)), "in sewing order, never reversed");
    assert_eq!(joined.len(), 4);
    let art = run(&mut s, "path.create", json!({"d": "M 200 200 L 220 200"}))["id"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"ids": [a, art]}));
    let said = |r: crate::Result<Value>| match r {
        Err(crate::EngineError::Other(m)) => m,
        r => panic!("a translated refusal: {r:?}"),
    };
    assert_eq!(said(s.execute("path.join", &json!({}))), super::needle_edits::RUNS_WITH_ART, "a run with art");
    run(&mut s, "select.set", json!({"ids": [a]}));
    assert_eq!(said(s.execute("path.join", &json!({}))), super::needle_edits::ONE_RUN, "one run alone");
}
