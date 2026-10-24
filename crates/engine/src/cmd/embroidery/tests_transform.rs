//! An object's embroidery settings under the commands that transform it: every one goes through
//! `Node::transform`, which moves the settings' points and turns a set angle with the object
//! (`doc::embroidery`'s transform rules).

use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartValue};
use vectorcraft_geom::{Affine, Point};

use super::test_support::{blank, run};
use super::*;

/// A red rectangle (no stroke) at `x, y`, `w × h` points → its id.
fn rect(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    id
}

/// Object `id`'s fill settings: angle 30°, pull 0.3 and 0.1 a side, entry and exit points.
fn set_fill(s: &mut Session, id: u64) {
    s.edit("Embroidery Object Settings", |d, _| {
        let n = d.node_mut(NodeId(id)).ok_or(EngineError::NoNode(NodeId(id)))?;
        let mut e = Embroidery::default();
        e.edit_part(PartKind::Fill, |p| {
            p.set(PartKey::Angle, Some(PartValue::Number(30.0)));
            p.set(PartKey::Pull, Some(PartValue::Sides([0.3, 0.1])));
            p.set(PartKey::Entry, Some(PartValue::Point(Point::new(20.0, 20.0))));
            p.set(PartKey::Exit, Some(PartValue::Point(Point::new(60.0, 40.0))));
        });
        n.extras_mut().embroidery = Some(Box::new(e));
        Ok(())
    })
    .unwrap();
}

/// Object `id`'s embroidery, as the active document holds it.
fn embroidery(s: &Session, id: u64) -> Embroidery {
    s.doc().unwrap().doc.node(NodeId(id)).and_then(|n| n.extras().embroidery.as_deref().cloned()).unwrap_or_default()
}

/// What `before` becomes under `a`, by the document crate's rules.
fn transformed(before: &Embroidery, a: Affine) -> Embroidery {
    let mut e = before.clone();
    e.transform(a);
    e
}

/// The settings `id`'s embroidery holds now are `before`'s under some transform whose points move
/// as `entry` and `exit` say: the points are where they went, the angle turned as `a` turns it.
fn moved_as(s: &Session, id: u64, before: &Embroidery, a: Affine) {
    let now = embroidery(s, id);
    let want = transformed(before, a);
    let (p, q) = (now.part(PartKind::Fill).unwrap(), want.part(PartKind::Fill).unwrap());
    let near = |x: Option<PartValue>, y: Option<PartValue>| match (x, y) {
        (Some(PartValue::Point(x)), Some(PartValue::Point(y))) => x.distance(y) < 1e-6,
        _ => false,
    };
    assert!(near(p.get(PartKey::Entry), q.get(PartKey::Entry)), "entry {:?}, not {:?}", p.get(PartKey::Entry), q.get(PartKey::Entry));
    assert!(near(p.get(PartKey::Exit), q.get(PartKey::Exit)), "exit {:?}, not {:?}", p.get(PartKey::Exit), q.get(PartKey::Exit));
    let (angle, want_angle) = (p.angle.unwrap(), q.angle.unwrap());
    assert!(vectorcraft_geom::normalize_deg(angle - want_angle).abs() < 1e-6, "angle {angle}, not {want_angle}");
    assert_eq!(p.pull, q.pull);
}

/// Rotating, reflecting, moving and scaling an object move its settings' points with it and turn
/// its set angle as the object turns (counter-clockwise on the page, half round beyond the image
/// under a mirror, its pull's sides swapped); a length never scales; undo gives them back.
#[test]
fn the_transform_commands_move_and_turn_an_objects_settings() {
    let mut s = blank();
    let id = rect(&mut s, 20.0, 20.0, 40.0, 20.0);
    set_fill(&mut s, id);
    let before = embroidery(&s, id);
    // About the rectangle's centre (40, 30).
    let about = |a: Affine| Affine::translate((40.0, 30.0)) * a * Affine::translate((-40.0, -30.0));
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 90}));
    moved_as(&s, id, &before, about(Affine::rotate(-90f64.to_radians())));
    assert_eq!(embroidery(&s, id).part(PartKind::Fill).and_then(|p| p.angle), Some(120.0));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(embroidery(&s, id), before, "undo gives them back exactly");
    run(&mut s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
    moved_as(&s, id, &before, about(Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0])));
    let fill = embroidery(&s, id).part(PartKind::Fill).cloned().unwrap();
    assert_eq!((fill.angle, fill.pull), (Some(-30.0), Some([0.1, 0.3])), "a mirror turns the rows and swaps the sides");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "object.move", json!({"ids": [id], "dx": 5, "dy": -3}));
    let moved = embroidery(&s, id);
    assert_eq!(moved, transformed(&before, Affine::translate((5.0, -3.0))), "a move moves the points by exactly its offset");
    assert_eq!(moved.part(PartKind::Fill).and_then(|p| p.angle).map(f64::to_bits), Some(30f64.to_bits()));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "object.scale", json!({"ids": [id], "sx": 200, "sy": 200}));
    moved_as(&s, id, &before, about(Affine::scale(2.0)));
    assert_eq!(embroidery(&s, id).part(PartKind::Fill).map(|p| p.pull), Some(Some([0.3, 0.1])), "lengths never scale");
}

/// Copies keep their settings, moved with them: a duplicate with an offset, a pasted copy, and a
/// copy made by a transform (Transform Each with copy); each keeps its own.
#[test]
fn copies_keep_their_settings_moved_with_them() {
    let mut s = blank();
    let id = rect(&mut s, 20.0, 20.0, 40.0, 20.0);
    set_fill(&mut s, id);
    let before = embroidery(&s, id);
    let ids = run(&mut s, "edit.duplicate", json!({"ids": [id], "dx": 10, "dy": 5}))["ids"].clone();
    let copy = ids[0].as_u64().unwrap();
    assert_ne!(copy, id);
    assert_eq!(embroidery(&s, copy), transformed(&before, Affine::translate((10.0, 5.0))));
    assert_eq!(embroidery(&s, id), before, "the original keeps its own");
    run(&mut s, "select.set", json!({"ids": [id]}));
    run(&mut s, "edit.copy", json!({}));
    let pasted = run(&mut s, "edit.paste", json!({}))["ids"][0].as_u64().unwrap();
    let e = embroidery(&s, pasted);
    let fill = e.part(PartKind::Fill).unwrap();
    assert_eq!((fill.angle, fill.pull), (Some(30.0), Some([0.3, 0.1])), "pasted: the settings come along");
    run(&mut s, "select.set", json!({"ids": [id]}));
    run(&mut s, "object.transformEach", json!({"rotate": 90, "copy": true}));
    let made = s.doc().unwrap().selection.objects.clone();
    let turned = made.iter().find(|n| n.0 != id).copied().unwrap();
    assert_eq!(embroidery(&s, turned.0).part(PartKind::Fill).and_then(|p| p.angle), Some(120.0));
}

/// With Transform Objects off the art stays, and so do its settings; an object without embroidery
/// keeps none after any transform.
#[test]
fn settings_stay_when_the_art_stays() {
    let mut s = blank();
    let id = rect(&mut s, 20.0, 20.0, 40.0, 20.0);
    set_fill(&mut s, id);
    let before = embroidery(&s, id);
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 45, "objects": false}));
    assert_eq!(embroidery(&s, id), before);
    let plain = rect(&mut s, 100.0, 100.0, 20.0, 20.0);
    run(&mut s, "object.rotate", json!({"ids": [plain], "angle": 30}));
    run(&mut s, "object.reflect", json!({"ids": [plain], "axis": "horizontal"}));
    assert!(s.doc().unwrap().doc.node(NodeId(plain)).unwrap().extras().embroidery.is_none());
}

/// Settings on a symbol's art turn with an instance: breaking the link of an instance turned by
/// 90° leaves art whose angle is turned by 90°.
#[test]
fn a_symbols_art_turns_with_its_instance() {
    let mut s = blank();
    let id = rect(&mut s, 20.0, 20.0, 40.0, 20.0);
    set_fill(&mut s, id);
    run(&mut s, "select.set", json!({"ids": [id]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    run(&mut s, "object.rotate", json!({"ids": [instance], "angle": 90}));
    let art = run(&mut s, "symbol.breakLink", json!({"ids": [instance]}))["ids"][0].as_u64().unwrap();
    let angle = embroidery(&s, art).part(PartKind::Fill).and_then(|p| p.angle).unwrap();
    assert!((angle - 120.0).abs() < 1e-6, "{angle}");
}
