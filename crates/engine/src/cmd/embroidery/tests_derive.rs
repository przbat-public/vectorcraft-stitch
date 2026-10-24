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

/// An object cut into several pieces keeps the cut after it once, on its last piece (sewn last),
/// its other settings on every piece: Divide, Split Into Grid and Divide Objects Below.
#[test]
fn an_object_cut_into_pieces_is_cut_once_after_them() {
    let cuts = |s: &Session, ids: &[u64]| ids.iter().filter(|id| embroidery(s, **id).trim_after).count();
    let angles = |s: &Session, ids: &[u64]| ids.iter().map(|id| angle(&embroidery(s, *id))).collect::<Vec<_>>();
    // Divide: the object with the cut is cut into two pieces (the overlap and the rest).
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let b = rect(&mut s, "#2233cc", 60.0, 30.0, 60.0, 40.0);
    give(&mut s, a, &settings(30.0, false));
    give(&mut s, b, &settings(60.0, true));
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    let group = ids(&run(&mut s, "object.pathfinder.divide", json!({})))[0];
    let pieces = members(&s, group);
    let of_b: Vec<u64> = pieces.iter().copied().filter(|id| angle(&embroidery(&s, *id)) == Some(60.0)).collect();
    assert_eq!(of_b.len(), 2, "{:?}", angles(&s, &pieces));
    assert_eq!(cuts(&s, &pieces), 1);
    assert!(embroidery(&s, *of_b.last().unwrap()).trim_after, "on its last piece");
    // Split Into Grid: nine cells, every one with the object's settings, the last with the cut.
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
    give(&mut s, a, &settings(30.0, true));
    run(&mut s, "select.set", json!({"ids": [a]}));
    let cells = ids(&run(&mut s, "object.path.splitIntoGrid", json!({"rows": 3, "columns": 3})));
    assert_eq!(cells.len(), 9);
    assert_eq!(angles(&s, &cells), vec![Some(30.0); 9]);
    assert_eq!(cuts(&s, &cells), 1);
    assert!(embroidery(&s, *cells.last().unwrap()).trim_after);
    // Divide Objects Below: the object below cut in two by the one above.
    let mut s = blank();
    let below = rect(&mut s, "#cc2222", 20.0, 20.0, 80.0, 60.0);
    give(&mut s, below, &settings(30.0, true));
    let cutter = rect(&mut s, "#2233cc", 60.0, 40.0, 80.0, 60.0);
    run(&mut s, "select.set", json!({"ids": [cutter]}));
    let pieces = ids(&run(&mut s, "object.path.divideObjectsBelow", json!({})));
    assert_eq!(pieces.len(), 2);
    assert_eq!(angles(&s, &pieces), vec![Some(30.0); 2]);
    assert_eq!((cuts(&s, &pieces), embroidery(&s, pieces[1]).trim_after), (1, true));
}

/// Shape Builder's merged shape takes the settings of the object whose look it takes (the
/// front-most under the drag's first point).
#[test]
fn shape_builder_takes_the_settings_of_the_object_it_takes_its_look_from() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let b = rect(&mut s, "#2233cc", 60.0, 30.0, 60.0, 40.0);
    give(&mut s, a, &settings(30.0, false));
    give(&mut s, b, &settings(60.0, false));
    let r = run(&mut s, "shapeBuilder.merge", json!({"ids": [a, b], "points": [[30, 30], [110, 60]]}));
    let made = ids(&r);
    assert!(!made.is_empty(), "{r}");
    let merged: Vec<Option<f64>> = made.iter().map(|id| angle(&embroidery(&s, *id))).collect();
    assert!(merged.contains(&Some(30.0)), "the look and settings of the object under the first point: {merged:?}");
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
    // Settings that differ only for the strokes Merge removes don't keep the shapes apart.
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let b = rect(&mut s, "#cc2222", 60.0, 30.0, 60.0, 40.0);
    give(&mut s, a, &settings(30.0, false));
    let mut other_stroke = settings(30.0, false);
    other_stroke.edit_part(PartKind::Stroke, |p| p.set(PartKey::Length, Some(PartValue::Number(3.0))));
    give(&mut s, b, &other_stroke);
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    let group = ids(&run(&mut s, "object.pathfinder.merge", json!({})))[0];
    let merged = members(&s, group);
    assert_eq!(merged.len(), 1, "one shape");
    assert_eq!(embroidery(&s, merged[0]).stroke, None, "no stroke, no stroke settings");
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

/// A stroke drawn with a scatter brush (its art a group of copies) takes the stroke's settings on
/// every path of its art, beside a fill: a stroke set to sew nothing sews nothing, as it prints and
/// once expanded.
#[test]
fn a_brushs_art_takes_the_strokes_settings_on_every_path() {
    let mut s = blank();
    let dot = rect(&mut s, "#2233cc", 300.0, 20.0, 6.0, 6.0);
    let brush = run(&mut s, "brush.new", json!({"type": "scatter", "ids": [dot]}))["name"].as_str().unwrap().to_string();
    run(&mut s, "select.set", json!({"ids": [dot]}));
    run(&mut s, "edit.clear", json!({}));
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 120.0, 80.0);
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc", "ids": [a]}));
    run(&mut s, "brush.apply", json!({"name": brush, "ids": [a]}));
    let units = |s: &mut Session| super::digitize::digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap().units.len();
    let with_art = units(&mut s);
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "stroke", "stitch": "none"}));
    assert_eq!(units(&mut s), 1, "its fill alone, of {with_art}");
    run(&mut s, "effect.expandAppearance", json!({"ids": [a]}));
    assert_eq!(units(&mut s), 1, "expanded: its fill alone");
    let mut stroke_paths = 0;
    node(&s, a).walk(&mut |n| {
        let stitch = n.extras().embroidery.as_deref().and_then(|e| e.part(PartKind::Fill)).and_then(|p| p.stitch.clone());
        if matches!(n.kind, NodeKind::Path { .. }) && stitch.as_deref() == Some("none") {
            stroke_paths += 1;
        }
    });
    assert!(stroke_paths >= 2, "every path of the brush's art: {stroke_paths}");
}

/// Expand Appearance of a symbol instance with an effect of its own: what stands for it keeps the
/// cut after it, and the walk cuts once after it, expanded or not.
#[test]
fn an_expanded_instance_keeps_the_cut_after_it() {
    let mut s = blank();
    let art = rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    run(&mut s, "select.set", json!({"ids": [art]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [instance]}));
    let mut cut = Embroidery::default();
    cut.set_trim_after(true);
    give(&mut s, instance, &cut);
    // Another object of the colour a short jump after it: only a cut asked for cuts there.
    rect(&mut s, "#cc2222", 70.0, 20.0, 20.0, 20.0);
    let trims = |s: &mut Session| run(s, "embroidery.stats", json!({}))["trims"].as_u64().unwrap();
    assert_eq!(trims(&mut s), 1, "sewn as it prints");
    run(&mut s, "effect.expandAppearance", json!({"ids": [instance]}));
    assert!(embroidery(&s, instance).trim_after, "on what stands for it");
    assert_eq!(trims(&mut s), 1, "expanded: one cut");
}
