//! The stitch commands' paths of stitches ([`super::needles`]): every kind reads and writes back as
//! it was, hostile anchors are refused before any arithmetic, the selection maps to needle points
//! and back; and Move Needle Points.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::StitchRun;
use vectorcraft_doc::{Node, NodeId};
use vectorcraft_testkit::embroidery as fx;

use super::needles::{self, reason};
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

/// Needle point `k` of path `id`, millimetres.
fn at(s: &Session, id: u64, k: usize) -> (f64, f64) {
    let st = needles::read(&node(s, id)).unwrap();
    let (r, i) = st.locate(k).unwrap();
    let p = st.runs[r].needles[i].p;
    (p.x / PT_PER_MM, p.y / PT_PER_MM)
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
        assert!(needles::write(&mut back, st));
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
            assert!(needles::write(d.node_mut(n.id).unwrap(), st));
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
        assert_eq!(needles::read(&node(&s, id)).err(), Some(reason::OUT_OF_RANGE), "{bad}");
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
    assert_eq!(needles::selected_in(&sel, NodeId(id), &st), [2, 3].into_iter().collect());
    needles::select_in(&mut sel, NodeId(id), &st, &[3].into_iter().collect());
    assert_eq!(sel.anchors[&NodeId(id)], [(1, 0), (1, 1)].into_iter().collect(), "both anchors of the one needle point");
    needles::select_in(&mut sel, NodeId(id), &st, &Default::default());
    assert!(!sel.contains(NodeId(id)) && !sel.anchors.contains_key(&NodeId(id)), "nothing of it: it leaves the selection");
    sel.anchors.clear();
    sel.objects = vec![NodeId(id)];
    assert_eq!(needles::selected_in(&sel, NodeId(id), &st).len(), 4, "selected whole: all of it");
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
