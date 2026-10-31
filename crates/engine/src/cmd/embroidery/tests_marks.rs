//! A run's cuts and stops follow their needle points through what upstream's path commands do to
//! its anchors (the project owner's choice, 2026-10-30): Reverse Path Direction, Add and Delete
//! Anchor Point, Remove Anchor Points, Simplify, Join's merge of coincident anchors and Close Path.
//! One whose needle point is gone is left out, and the Check tab says so.

use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::{Embroidery, Mark, StitchRun, shape_of};
use vectorcraft_geom::{Anchor, PathData, Point, SubPath};

use super::needles;
use super::test_support::{PT_PER_MM, blank, run};
use crate::Session;

/// A point in millimetres as document points.
fn pt(x: f64, y: f64) -> Point {
    Point::new(x * PT_PER_MM, y * PT_PER_MM)
}

/// A run opened from a file, in the first layer: one subpath through each of `subpaths`' x
/// positions (millimetres, at y 10), a stop after the needle point at `stop` (millimetres along
/// the first subpath) and a cut before the second subpath, written as this version writes them
/// → its id.
fn run_with_marks(s: &mut Session, subpaths: &[&[f64]], stop: f64) -> u64 {
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"color": "#c81e1e"}));
    let id = run(s, "path.create", json!({"d": "M 10 10 L 20 10"}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    let path = PathData::new(
        subpaths.iter().map(|xs| SubPath { anchors: xs.iter().map(|&x| Anchor::corner(pt(x, 10.0))).collect(), closed: false }).collect(),
    );
    let at = |s: usize, x: f64| subpaths[s].iter().position(|&v| v == x).unwrap();
    let marks = StitchRun {
        stops: vec![Mark::at(0, at(0, stop), pt(stop, 10.0))],
        trims: vec![Mark::at(1, 0, pt(subpaths[1][0], 10.0))],
        shape: Some(shape_of(&path)),
        ..StitchRun::default()
    };
    s.edit("Run", |d, _| {
        let n = d.node_mut(NodeId(id)).unwrap();
        if let vectorcraft_doc::NodeKind::Path { path: p, .. } = &mut n.kind {
            *p = path;
        }
        n.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(marks), ..Default::default() }));
        Ok(())
    })
    .unwrap();
    id
}

/// Where the run's stops and cuts fall: the x (millimetres) of each needle point a stop follows,
/// and of each run's first needle point a cut comes before.
fn marks_at(s: &Session, id: u64) -> (Vec<f64>, Vec<f64>) {
    let st = needles::read(s.doc().unwrap().doc.node(NodeId(id)).unwrap()).unwrap();
    let mm = |p: Point| (p.x / PT_PER_MM * 1e6).round() / 1e6;
    let stops = st.runs.iter().flat_map(|r| &r.needles).filter(|n| n.stop).map(|n| mm(n.p)).collect();
    let cuts = st.runs.iter().filter(|r| r.cut_before).filter_map(|r| r.needles.first()).map(|n| mm(n.p)).collect();
    (stops, cuts)
}

/// Whether the Check tab says a cut or a stop was left out.
fn check_says_left_out(s: &mut Session) -> bool {
    let r = run(s, "embroidery.check", json!({}));
    r.to_string().contains(super::digitize::MARKS_LEFT_OUT)
}

/// Each upstream command that adds, removes or reorders a run's anchors leaves its stop after the
/// needle point it followed and its cut before the run it came before; a stop whose needle point
/// is deleted (by Delete Anchor Point, or Simplify) is left out, and the Check tab says so.
#[test]
fn cuts_and_stops_follow_their_needle_points_through_path_commands() {
    let s0: &[f64] = &[10.0, 12.0, 14.0, 16.0, 18.0];
    let s1: &[f64] = &[30.0, 32.0, 34.0];
    let fresh = || {
        let mut s = blank();
        let id = run_with_marks(&mut s, &[s0, s1], 12.0);
        (s, id)
    };
    let (mut s, id) = fresh();
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]), "as written");
    assert!(!check_says_left_out(&mut s));
    // Reverse Path Direction: each subpath reversed in place.
    run(&mut s, "path.reverse", json!({"ids": [id]}));
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![34.0]), "the stop after 12, the cut before the subpath holding 30");
    // Add Anchor Point before it (a Pen click on a segment).
    let (mut s, id) = fresh();
    run(&mut s, "path.insertAnchor", json!({"id": id, "subpath": 0, "segment": 0, "t": 0.5}));
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]));
    // Delete Anchor Point: another anchor, then the stop's own.
    let (mut s, id) = fresh();
    run(&mut s, "path.removeAnchor", json!({"id": id, "subpath": 0, "anchor": 3}));
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]));
    run(&mut s, "path.removeAnchor", json!({"id": id, "subpath": 0, "anchor": 1}));
    assert_eq!(marks_at(&s, id), (vec![], vec![30.0]), "its needle point gone: the stop left out");
    assert!(check_says_left_out(&mut s), "and the Check tab says so");
    // Remove Anchor Points (the direct-selected ones).
    let (mut s, id) = fresh();
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 0]]}));
    run(&mut s, "path.removeAnchors", json!({}));
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]));
    // Simplify: the straight run's middle needle points go, the stop's among them.
    let (mut s, id) = fresh();
    run(&mut s, "select.set", json!({"ids": [id]}));
    run(&mut s, "object.path.simplify", json!({"tolerance": 1}));
    let path = s.doc().unwrap().doc.node(NodeId(id)).unwrap().path_data().unwrap().clone();
    let xs: Vec<Vec<f64>> = path.subpaths.iter().map(|sp| sp.anchors.iter().map(|a| (a.p.x / PT_PER_MM * 1e6).round() / 1e6).collect()).collect();
    assert_eq!(xs, [vec![10.0, 18.0], vec![30.0, 34.0]], "straight runs keep their ends");
    assert_eq!(marks_at(&s, id), (vec![], vec![30.0]), "the stop's needle point gone; the cut before the subpath holding 30");
    assert!(check_says_left_out(&mut s), "the stop left out: said");
    // Close Path: no anchor moves.
    let (mut s, id) = fresh();
    run(&mut s, "path.close", json!({"id": id}));
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]));
    // Join's merge of two coincident anchors selected: one of them goes, the stop stays at 12.
    let mut s = blank();
    let id = run_with_marks(&mut s, &[&[10.0, 12.0, 12.0, 14.0], s1], 12.0);
    run(&mut s, "select.anchors", json!({"id": id, "anchors": [[0, 1], [0, 2]]}));
    run(&mut s, "path.join", json!({}));
    let path = s.doc().unwrap().doc.node(NodeId(id)).unwrap().path_data().unwrap().clone();
    assert_eq!(path.subpaths[0].anchors.len(), 3, "merged");
    assert_eq!(marks_at(&s, id), (vec![12.0], vec![30.0]));
}

/// A whole-object transform moves where each mark lay with its needle point: moved, turned and
/// scaled, the stop and the cut stay where they were on the run, and nothing is left out.
#[test]
fn a_transform_moves_the_marks_with_their_needle_points() {
    let mut s = blank();
    let id = run_with_marks(&mut s, &[&[10.0, 12.0, 14.0, 16.0], &[30.0, 32.0]], 14.0);
    run(&mut s, "select.set", json!({"ids": [id]}));
    run(&mut s, "object.move", json!({"dx": 3.5, "dy": -2}));
    run(&mut s, "object.rotate", json!({"angle": 33}));
    run(&mut s, "object.scale", json!({"sx": 1.7, "sy": 0.6}));
    let st = needles::read(s.doc().unwrap().doc.node(NodeId(id)).unwrap()).unwrap();
    let stopped: Vec<usize> = st.runs.iter().flat_map(|r| &r.needles).enumerate().filter(|(_, n)| n.stop).map(|(k, _)| k).collect();
    let cut: Vec<usize> = st.runs.iter().enumerate().filter(|(_, r)| r.cut_before).map(|(r, _)| r).collect();
    assert_eq!((stopped, cut), (vec![2], vec![1]));
    assert!(!check_says_left_out(&mut s));
}
