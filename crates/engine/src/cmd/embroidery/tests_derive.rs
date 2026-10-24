//! Objects made from others keep the embroidery settings of the objects they come from: one test
//! per operation (`derive`'s rules).

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartValue};
use vectorcraft_doc::{Node, NodeId, NodeKind};

use super::test_support::{blank, run};
use super::*;

/// A rectangle filled `color` (no stroke) at `x, y`, `w × h` points → its id.
fn rect(s: &mut Session, color: &str, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": color}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    id
}

/// Settings: a fill angle of `angle`, a stroke stitch `running` with stitch length 2, and the cut
/// after the object when `cut`.
fn settings(angle: f64, cut: bool) -> Embroidery {
    let mut e = Embroidery::default();
    e.edit_part(PartKind::Fill, |p| p.set(PartKey::Angle, Some(PartValue::Number(angle))));
    e.edit_part(PartKind::Stroke, |p| {
        p.set(PartKey::Stitch, Some(PartValue::Id("running".into())));
        p.set(PartKey::Length, Some(PartValue::Number(2.0)));
    });
    e.set_trim_after(cut);
    e
}

/// Give object `id` the embroidery `e`.
fn give(s: &mut Session, id: u64, e: &Embroidery) {
    s.edit("Embroidery Object Settings", |d, _| {
        let n = d.node_mut(NodeId(id)).ok_or(EngineError::NoNode(NodeId(id)))?;
        n.extras_mut().embroidery = Some(Box::new(e.clone()));
        Ok(())
    })
    .unwrap();
}

/// Object `id`'s embroidery (none: the default).
fn embroidery(s: &Session, id: u64) -> Embroidery {
    node(s, id).extras().embroidery.as_deref().cloned().unwrap_or_default()
}

fn node(s: &Session, id: u64) -> Node {
    s.doc().unwrap().doc.node(NodeId(id)).cloned().unwrap_or_else(|| panic!("no object {id}"))
}

/// The ids a command answered with.
fn ids(reply: &Value) -> Vec<u64> {
    reply["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

/// The fill angle an embroidery sets.
fn angle(e: &Embroidery) -> Option<f64> {
    e.part(PartKind::Fill).and_then(|p| p.angle)
}

/// The object `id`'s direct members' ids, in paint order.
fn members(s: &Session, id: u64) -> Vec<u64> {
    node(s, id).children().map(|c| c.iter().map(|n| n.id.0).collect()).unwrap_or_default()
}

/// Pathfinder's results take the settings of the object whose paint they take, less its literal
/// stitches: Unite's of the shape it keeps the paint of, Divide's pieces each of its own source.
#[test]
fn pathfinder_results_take_their_paint_sources_settings() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let b = rect(&mut s, "#2233cc", 60.0, 30.0, 60.0, 40.0);
    give(&mut s, a, &settings(30.0, true));
    give(&mut s, b, &settings(60.0, false));
    let paint_of_a = node(&s, a).appearance.clone();
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    let united = ids(&run(&mut s, "object.pathfinder.unite", json!({})))[0];
    let source = if node(&s, united).appearance == paint_of_a { settings(30.0, true) } else { settings(60.0, false) };
    assert_eq!(embroidery(&s, united), source, "the settings of the shape whose paint it takes");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    let group = ids(&run(&mut s, "object.pathfinder.divide", json!({})))[0];
    let angles: Vec<Option<f64>> = members(&s, group).into_iter().map(|id| angle(&embroidery(&s, id))).collect();
    assert_eq!(angles.len(), 3, "three pieces");
    assert!(angles.contains(&Some(30.0)) && angles.contains(&Some(60.0)), "{angles:?}");
}

/// Merge joins touching shapes of one colour only when their settings are the same too.
#[test]
fn merge_keeps_shapes_of_one_colour_apart_when_their_settings_differ() {
    for (differ, pieces) in [(false, 1), (true, 2)] {
        let mut s = blank();
        let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
        let b = rect(&mut s, "#cc2222", 60.0, 30.0, 60.0, 40.0);
        give(&mut s, a, &settings(30.0, false));
        give(&mut s, b, &settings(if differ { 60.0 } else { 30.0 }, false));
        run(&mut s, "select.set", json!({"ids": [a, b]}));
        let group = ids(&run(&mut s, "object.pathfinder.merge", json!({})))[0];
        assert_eq!(members(&s, group).len(), pieces, "settings differ: {differ}");
    }
}

/// Offset Path's new object takes its source's settings.
#[test]
fn an_offset_path_takes_its_sources_settings() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    give(&mut s, a, &settings(30.0, false));
    run(&mut s, "select.set", json!({"ids": [a]}));
    let made = ids(&run(&mut s, "object.path.offsetPath", json!({"offset": 4})));
    assert!(!made.is_empty());
    for id in made {
        assert_eq!(embroidery(&s, id), settings(30.0, false), "{id}");
    }
}

/// Outline Stroke: the fill's copy keeps the fill's settings, the stroke's outline takes the
/// stroke's (as a fill: it is filled art now), and the cut after the object goes to what stands for
/// it.
#[test]
fn outline_stroke_gives_each_part_its_settings() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc", "ids": [a]}));
    run(&mut s, "stroke.set", json!({"weight": 6, "ids": [a]}));
    give(&mut s, a, &settings(30.0, true));
    run(&mut s, "select.set", json!({"ids": [a]}));
    let group = ids(&run(&mut s, "object.path.outlineStroke", json!({})))[0];
    let parts = members(&s, group);
    assert_eq!(parts.len(), 2, "the fill and the stroke's outline");
    let (fill, outline) = (embroidery(&s, parts[0]), embroidery(&s, parts[1]));
    assert_eq!((angle(&fill), fill.trim_after, fill.stroke.is_none()), (Some(30.0), false, true), "{fill:?}");
    let outline_fill = outline.part(PartKind::Fill).cloned().unwrap_or_default();
    assert_eq!((outline_fill.stitch.as_deref(), outline_fill.length, outline.trim_after), (Some("running"), Some(2.0), false), "{outline:?}");
    assert!(embroidery(&s, group).trim_after, "the cut after the object, once");
}

/// Make Compound Path takes the settings of the backmost path, whose paint it takes; Release
/// gives every part the compound path's, the cut to the last; Ungroup gives the group's cut to its
/// last member.
#[test]
fn compound_paths_and_groups_hand_their_settings_on() {
    let mut s = blank();
    let back = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let front = rect(&mut s, "#2233cc", 100.0, 20.0, 60.0, 40.0);
    give(&mut s, back, &settings(30.0, true));
    give(&mut s, front, &settings(60.0, false));
    run(&mut s, "select.set", json!({"ids": [back, front]}));
    let compound = run(&mut s, "object.compoundPath.make", json!({}))["id"].as_u64().unwrap();
    assert_eq!(embroidery(&s, compound), settings(30.0, true));
    run(&mut s, "select.set", json!({"ids": [compound]}));
    run(&mut s, "object.compoundPath.release", json!({}));
    let parts = s.doc().unwrap().selection.objects.clone();
    assert_eq!(parts.len(), 2);
    let cuts: Vec<bool> = parts.iter().map(|id| embroidery(&s, id.0).trim_after).collect();
    let angles: Vec<Option<f64>> = parts.iter().map(|id| angle(&embroidery(&s, id.0))).collect();
    assert_eq!((angles, cuts.iter().filter(|c| **c).count()), (vec![Some(30.0); 2], 1), "every part the compound's, one cut");
    run(&mut s, "select.set", json!({"ids": parts.iter().map(|id| id.0).collect::<Vec<_>>()}));
    let group = run(&mut s, "object.group", json!({}))["id"].as_u64().unwrap();
    let mut cut = Embroidery::default();
    cut.set_trim_after(true);
    give(&mut s, group, &cut);
    let last = *members(&s, group).last().unwrap();
    give(&mut s, last, &Embroidery::default());
    run(&mut s, "select.set", json!({"ids": [group]}));
    run(&mut s, "object.ungroup", json!({}));
    assert!(embroidery(&s, last).trim_after, "the group's cut on its last member");
}

/// Create Outlines gives the cut after the type to the group of its outlines; Break Link gives the
/// cut after an instance to its art.
#[test]
fn outlines_and_broken_links_keep_the_cut() {
    let mut s = blank();
    let text = run(&mut s, "text.create", json!({"x": 20, "y": 100, "text": "Hi"}))["id"].as_u64().unwrap();
    let mut cut = Embroidery::default();
    cut.set_trim_after(true);
    give(&mut s, text, &cut);
    run(&mut s, "select.set", json!({"ids": [text]}));
    let outlines = ids(&run(&mut s, "type.createOutlines", json!({})))[0];
    assert!(embroidery(&s, outlines).trim_after);
    let art = rect(&mut s, "#cc2222", 200.0, 20.0, 40.0, 30.0);
    run(&mut s, "select.set", json!({"ids": [art]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    give(&mut s, instance, &cut);
    let placed = ids(&run(&mut s, "symbol.breakLink", json!({"ids": [instance]})))[0];
    assert!(embroidery(&s, placed).trim_after);
}

/// An object with two fills and a stroke, Trim After on, expanded (Expand Appearance): each fill's
/// piece takes the fill's settings, the stroke's outline the stroke's, and the cut stays after the
/// object, once. Sewn without expanding it first, the same: one cut after it, none between its
/// fills (each fill's piece once asked for its own).
#[test]
fn an_expanded_appearance_gives_each_piece_its_part_and_cuts_once() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    run(&mut s, "appearance.addFill", json!({"ids": [a]}));
    run(&mut s, "paint.setStroke", json!({"color": "#cc2222", "ids": [a]}));
    run(&mut s, "stroke.set", json!({"weight": 6, "ids": [a]}));
    give(&mut s, a, &settings(30.0, true));
    // Another object of the colour a short jump after it: only a cut asked for cuts there.
    rect(&mut s, "#cc2222", 90.0, 20.0, 20.0, 20.0);
    let trims = |s: &mut Session| run(s, "embroidery.stats", json!({}))["trims"].as_u64().unwrap();
    assert_eq!(trims(&mut s), 1, "sewn as it prints: one cut, after the object");
    run(&mut s, "effect.expandAppearance", json!({"ids": [a]}));
    assert!(matches!(node(&s, a).kind, NodeKind::Group { .. }));
    let pieces: Vec<Embroidery> = members(&s, a).into_iter().map(|id| embroidery(&s, id)).collect();
    assert_eq!(pieces.len(), 3, "two fills and the stroke's outline");
    assert_eq!(pieces.iter().filter(|e| angle(e) == Some(30.0)).count(), 2, "{pieces:?}");
    assert_eq!(pieces.iter().filter(|e| e.part(PartKind::Fill).and_then(|p| p.stitch.as_deref()) == Some("running")).count(), 1, "{pieces:?}");
    assert!(pieces.iter().all(|e| !e.trim_after) && embroidery(&s, a).trim_after);
    assert_eq!(trims(&mut s), 1, "expanded: still one cut");
}
