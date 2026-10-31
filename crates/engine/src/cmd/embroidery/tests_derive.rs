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

/// Ungroup gives the group's cut to its last member the walk sews: past a label of type on top.
#[test]
fn ungroup_gives_the_cut_to_the_last_member_sewn() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let label = run(&mut s, "text.create", json!({"x": 20, "y": 100, "text": "Hi"}))["id"].as_u64().unwrap();
    let group = run(&mut s, "object.group", json!({"ids": [a, label]}))["id"].as_u64().unwrap();
    assert_eq!(*members(&s, group).last().unwrap(), label, "the label on top");
    let mut cut = Embroidery::default();
    cut.set_trim_after(true);
    give(&mut s, group, &cut);
    run(&mut s, "select.set", json!({"ids": [group]}));
    run(&mut s, "object.ungroup", json!({}));
    assert!(embroidery(&s, a).trim_after && !embroidery(&s, label).trim_after);
}

/// What an object's settings keep that this version doesn't read is shared by the pieces cut from
/// it, not copied into each: a file's kept value can't make Split Into Grid hold it once a cell.
#[test]
fn pieces_share_what_their_source_keeps() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 600.0, 600.0);
    let kept: Embroidery =
        serde_json::from_value(json!({"fill": {"grain": "x".repeat(100_000), "angle": 30.0}, "branch": "y".repeat(100_000)})).unwrap();
    give(&mut s, a, &kept);
    run(&mut s, "select.set", json!({"ids": [a]}));
    let cells = ids(&run(&mut s, "object.path.splitIntoGrid", json!({"rows": 10, "columns": 10})));
    assert_eq!(cells.len(), 100);
    let e: Vec<Embroidery> = cells.iter().map(|id| embroidery(&s, *id)).collect();
    let first = &e[0];
    for other in &e[1..] {
        assert!(std::sync::Arc::ptr_eq(&first.extra, &other.extra), "the object's kept keys shared");
        let (p, q) = (first.part(PartKind::Fill).unwrap(), other.part(PartKind::Fill).unwrap());
        assert!(std::sync::Arc::ptr_eq(&p.extra, &q.extra), "the part's kept keys shared");
    }
    assert_eq!(angle(first), Some(30.0));
}

/// The entry and exit points `e`'s fill part sets, document points.
fn fill_points(e: &Embroidery) -> (Option<[f64; 2]>, Option<[f64; 2]>) {
    let p = e.part(PartKind::Fill).and_then(|p| p.points.as_ref());
    (p.and_then(|p| p.entry).map(|q| [q.x, q.y]), p.and_then(|p| p.exit).map(|q| [q.x, q.y]))
}

/// Settings whose fill sets an entry and an exit point.
fn with_points(entry: [f64; 2], exit: [f64; 2]) -> Embroidery {
    let mut e = settings(30.0, false);
    e.edit_part(PartKind::Fill, |p| {
        p.set(PartKey::Entry, Some(PartValue::Point(vectorcraft_geom::Point::new(entry[0], entry[1]))));
        p.set(PartKey::Exit, Some(PartValue::Point(vectorcraft_geom::Point::new(exit[0], exit[1]))));
    });
    e
}

/// An object cut into pieces keeps each of its points only on the piece nearest it, the others'
/// back to Auto, their other settings the same: Split Into Grid's cells, Release Compound Path's
/// parts. Mutation caught: every piece keeping the points starts and ends each at the same place.
#[test]
fn an_objects_points_stay_on_the_pieces_nearest_them() {
    let (entry, exit) = ([22.0, 22.0], [108.0, 108.0]);
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
    give(&mut s, a, &with_points(entry, exit));
    run(&mut s, "select.set", json!({"ids": [a]}));
    let cells = ids(&run(&mut s, "object.path.splitIntoGrid", json!({"rows": 3, "columns": 3})));
    let points: Vec<_> = cells.iter().map(|id| fill_points(&embroidery(&s, *id))).collect();
    assert_eq!(points.iter().filter(|p| p.0 == Some(entry)).count(), 1, "{points:?}");
    assert_eq!(points.iter().filter(|p| p.1 == Some(exit)).count(), 1, "{points:?}");
    assert_eq!(points.first().map(|p| p.0), Some(Some(entry)), "the entry on the cell it lies in (the first)");
    assert_eq!(points.last().map(|p| p.1), Some(Some(exit)), "the exit on the cell it lies in (the last)");
    assert!(cells.iter().all(|id| angle(&embroidery(&s, *id)) == Some(30.0)), "the other settings on every cell");
    // A compound path of two squares, its points by the second: both on the second part.
    let mut s = blank();
    let back = rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 40.0);
    let front = rect(&mut s, "#cc2222", 100.0, 20.0, 40.0, 40.0);
    run(&mut s, "select.set", json!({"ids": [back, front]}));
    let compound = run(&mut s, "object.compoundPath.make", json!({}))["id"].as_u64().unwrap();
    give(&mut s, compound, &with_points([140.0, 30.0], [100.0, 60.0]));
    run(&mut s, "select.set", json!({"ids": [compound]}));
    run(&mut s, "object.compoundPath.release", json!({}));
    let parts: Vec<u64> = s.doc().unwrap().selection.objects.iter().map(|id| id.0).collect();
    let points: Vec<_> = parts.iter().map(|id| fill_points(&embroidery(&s, *id))).collect();
    let near_second: Vec<_> = parts
        .iter()
        .zip(&points)
        .filter(|(id, _)| s.doc().unwrap().doc.bounds_of(&[NodeId(**id)], false).is_some_and(|b| b.x0 > 90.0))
        .map(|(_, p)| *p)
        .collect();
    assert_eq!(near_second, vec![(Some([140.0, 30.0]), Some([100.0, 60.0]))], "{points:?}");
    assert_eq!(points.iter().filter(|p| **p == (None, None)).count(), 1, "the other part Auto");
}

/// An object with two fills expanded (its pieces lie on one another): the first piece takes the
/// entry and the last the exit, as the walk sews it without expanding it first: its first unit starts
/// at the entry and its last ends at the exit, the one between follows the needle.
#[test]
fn a_part_sewn_as_several_units_takes_its_entry_on_the_first_and_its_exit_on_the_last() {
    let (entry, exit) = ([20.0, 40.0], [80.0, 40.0]);
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    run(&mut s, "appearance.addFill", json!({"ids": [a]}));
    run(&mut s, "appearance.addFill", json!({"ids": [a]}));
    give(&mut s, a, &with_points(entry, exit));
    let plan = |s: &Session| super::digitize::digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let sewn = plan(&s);
    let units: Vec<&super::plan::Unit> = sewn.units.iter().filter(|u| u.origin.selected().0 == a).collect();
    assert_eq!(units.len(), 3, "three fills, three units");
    let ends = |k: usize| {
        let pts: Vec<_> = sewn.unit_stitches(k).iter().filter(|s| s.command == vectorcraft_stitch::Command::Stitch).map(|s| (s.x, s.y)).collect();
        (*pts.first().unwrap(), *pts.last().unwrap())
    };
    let mm = |q: [f64; 2]| (q[0] * 25.4 / 72.0, q[1] * 25.4 / 72.0);
    let off = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
    let index = |u: &super::plan::Unit| sewn.units.iter().position(|v| std::ptr::eq(v, u)).unwrap();
    let (first, last) = (index(units[0]), index(units[2]));
    assert!(off(ends(first).0, mm(entry)) < 0.3, "the first unit starts at the entry: {:?}", ends(first));
    assert!(off(ends(last).1, mm(exit)) < 1.0, "the last unit ends at the exit: {:?}", ends(last));
    // Asked only there: the unit between follows the needle (a walk keeping the points on every
    // unit would start and end each at them).
    let asked: Vec<_> = units.iter().map(|u| (u.asked.entry.is_some(), u.asked.exit.is_some())).collect();
    assert_eq!(asked, vec![(true, false), (false, false), (false, true)], "the unit between is free");
    run(&mut s, "effect.expandAppearance", json!({"ids": [a]}));
    let pieces: Vec<_> = members(&s, a).into_iter().map(|id| fill_points(&embroidery(&s, id))).collect();
    assert_eq!(pieces, vec![(Some(entry), None), (None, None), (None, Some(exit))], "expanded: the first the entry, the last the exit");
}

/// The tools that cut one object into several keep its points only on the piece nearest each, and
/// the cut after it on the last piece: the Knife and the Eraser through a filled square, Cut Path at
/// two of its anchors, Shape Builder's merge of the square's overlap with another (the merged shape
/// takes the square's look, so its points too, and the square's remnant keeps them). Mutation
/// caught: a tool copying the source onto every piece gives each one both points and the cut.
#[test]
fn the_cutting_tools_keep_each_point_on_the_nearest_piece() {
    let (entry, exit) = ([22.0, 22.0], [108.0, 108.0]);
    let mut source = with_points(entry, exit);
    source.set_trim_after(true);
    let square = |s: &mut Session| {
        let a = rect(s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
        give(s, a, &source);
        a
    };
    let check = |s: &Session, pieces: &[u64], what: &str| {
        assert!(pieces.len() >= 2, "{what}: {pieces:?}");
        let points: Vec<_> = pieces.iter().map(|id| fill_points(&embroidery(s, *id))).collect();
        assert_eq!(points.iter().filter(|p| p.0 == Some(entry)).count(), 1, "{what}: {points:?}");
        assert_eq!(points.iter().filter(|p| p.1 == Some(exit)).count(), 1, "{what}: {points:?}");
        assert_eq!(pieces.iter().filter(|id| embroidery(s, **id).trim_after).count(), 1, "{what}: cut once");
        assert!(pieces.iter().all(|id| angle(&embroidery(s, *id)) == Some(30.0)), "{what}: the other settings on every piece");
    };
    for (command, params) in [
        ("path.knife", json!({"points": [[65.0, 0.0], [65.0, 130.0]]})),
        ("path.eraseRegion", json!({"points": [[65.0, 0.0], [65.0, 130.0]], "size": 10.0})),
    ] {
        let mut s = blank();
        let a = square(&mut s);
        run(&mut s, "select.set", json!({"ids": [a]}));
        let pieces = ids(&run(&mut s, command, params));
        check(&s, &pieces, command);
    }
    let mut s = blank();
    let a = square(&mut s);
    run(&mut s, "select.anchors", json!({"id": a, "anchors": [[0, 1], [0, 3]], "mode": "set"}));
    let pieces = ids(&run(&mut s, "path.cutAtAnchors", json!({})));
    check(&s, &pieces, "path.cutAtAnchors");
    // Shape Builder: another square under this one's right part; a click on their overlap merges it
    // with the look of the front one, the square.
    let mut s = blank();
    let under = rect(&mut s, "#2233cc", 80.0, 20.0, 90.0, 90.0);
    let a = square(&mut s);
    run(&mut s, "select.set", json!({"ids": [under, a]}));
    run(&mut s, "shapeBuilder.merge", json!({"points": [[95.0, 60.0]]}));
    // The square's pieces: those with its settings (the other square has none).
    let squares: Vec<u64> =
        s.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).filter(|id| angle(&embroidery(&s, *id)) == Some(30.0)).collect();
    check(&s, &squares, "shapeBuilder.merge");
    // Scissors on an open line: its stroke's points on the halves nearest them.
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#113311"}));
    let line = run(&mut s, "path.create", json!({"d": "M 20 20 L 120 20"}))["id"].as_u64().unwrap();
    let mut e = settings(30.0, true);
    e.edit_part(PartKind::Stroke, |p| {
        p.set(PartKey::Entry, Some(PartValue::Point(vectorcraft_geom::Point::new(22.0, 20.0))));
        p.set(PartKey::Exit, Some(PartValue::Point(vectorcraft_geom::Point::new(118.0, 20.0))));
    });
    give(&mut s, line, &e);
    let halves = ids(&run(&mut s, "path.split", json!({"id": line, "segment": 0, "t": 0.5})));
    let points: Vec<_> = halves
        .iter()
        .map(|id| embroidery(&s, *id).part(PartKind::Stroke).and_then(|p| p.points.as_ref()).map(|p| (p.entry.is_some(), p.exit.is_some())))
        .collect();
    assert_eq!(points, vec![Some((true, false)), Some((false, true))], "path.split");
    assert_eq!(halves.iter().filter(|id| embroidery(&s, **id).trim_after).count(), 1, "path.split: cut once");
}

/// Expand Appearance of a path whose Transform effect copies it (with Scale Strokes & Effects on,
/// so each copy is an object of its own): each point back where the path holds it, on the piece
/// nearest it only, the copy's points back to Auto. Mutation caught: each copy keeping the points
/// moved with it starts and ends at its own.
#[test]
fn a_transform_effects_copies_keep_each_point_on_the_nearest_piece() {
    let (entry, exit) = ([22.0, 22.0], [108.0, 108.0]);
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
    give(&mut s, a, &with_points(entry, exit));
    run(&mut s, "effect.apply", json!({"ids": [a], "effect": "distort.transform", "params": {"moveH": 200, "copies": 1, "strokes": true}}));
    run(&mut s, "effect.expandAppearance", json!({"ids": [a]}));
    let mut pieces = Vec::new();
    fn paths(n: &Node, out: &mut Vec<Node>) {
        match &n.kind {
            NodeKind::Path { .. } | NodeKind::Compound { .. } => out.push(n.clone()),
            _ => n.children().into_iter().flatten().for_each(|c| paths(c, out)),
        }
    }
    for n in s.doc().unwrap().doc.layers[0].children().unwrap() {
        paths(n, &mut pieces);
    }
    let points: Vec<_> = pieces
        .iter()
        .map(|n| (n.geometric_bounds().map(|b| b.x0), fill_points(n.extras().embroidery.as_deref().unwrap_or(&Embroidery::default()))))
        .collect();
    assert!(pieces.len() >= 2, "{points:?}");
    assert_eq!(points.iter().filter(|(_, p)| p.0 == Some(entry)).count(), 1, "{points:?}");
    assert_eq!(points.iter().filter(|(_, p)| p.1 == Some(exit)).count(), 1, "{points:?}");
    assert!(points.iter().all(|(x0, p)| *p == (None, None) || x0.is_some_and(|x| x < 100.0)), "only the original's piece: {points:?}");
}

/// A fill's entry and exit points, as `fill_points` gives them.
type Points = (Option<[f64; 2]>, Option<[f64; 2]>);

/// Pathfinder's Divide and Divide Objects Below keep each point on the piece nearest it: a square
/// with both points divided by another over its right half (Divide's pieces of the square and of
/// the overlap take their paint sources' settings), and cut by a rectangle above it (Divide
/// Objects Below). Mutation caught: `cut_once` copying the points onto every piece.
#[test]
fn divide_and_divide_objects_below_keep_each_point_on_the_nearest_piece() {
    let (entry, exit) = ([22.0, 22.0], [108.0, 108.0]);
    let with_points_of = |s: &Session| -> Vec<Points> {
        s.doc().unwrap().doc.layers[0]
            .children()
            .unwrap()
            .iter()
            .flat_map(|n| std::iter::once((**n).clone()).chain(n.children().into_iter().flatten().map(|c| (**c).clone())))
            .map(|n| fill_points(n.extras().embroidery.as_deref().unwrap_or(&Embroidery::default())))
            .filter(|p| *p != (None, None))
            .collect()
    };
    for (command, cutter_on_top) in [("object.pathfinder.divide", false), ("object.path.divideObjectsBelow", true)] {
        let mut s = blank();
        let a = rect(&mut s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
        give(&mut s, a, &with_points(entry, exit));
        let b = rect(&mut s, "#2233cc", 65.0, 0.0, 90.0, 130.0);
        run(&mut s, "select.set", json!({"ids": if cutter_on_top { vec![b] } else { vec![a, b] }}));
        run(&mut s, command, json!({}));
        let held = with_points_of(&s);
        assert_eq!(held.iter().filter(|p| p.0 == Some(entry)).count(), 1, "{command}: {held:?}");
        assert_eq!(held.iter().filter(|p| p.1 == Some(exit)).count(), 1, "{command}: {held:?}");
    }
}

/// An object's exit turns and mirrors with it, and the plan follows: a filled rectangle with its
/// exit on its right side, turned a quarter and mirrored, ends at its exit where the turn and the
/// mirror took it (within the Check tab's 1 mm).
#[test]
fn the_plan_follows_a_turned_and_mirrored_exit() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 80.0, 50.0);
    run(&mut s, "embroidery.setEntryExit", json!({"id": a, "part": "fill", "exit": [100, 45]}));
    for (command, params) in [("object.rotate", json!({"ids": [a], "angle": 90})), ("object.reflect", json!({"ids": [a], "axis": "vertical"}))] {
        run(&mut s, command, params);
        let exit = fill_points(&embroidery(&s, a)).1.expect("the exit moved with the object");
        let unit = run(&mut s, "embroidery.sequence", json!({}))["blocks"][0]["units"][0].clone();
        let asked = unit["asked"]["exit"].as_array().map(|q| [q[0].as_f64().unwrap(), q[1].as_f64().unwrap()]).unwrap();
        let mm = |v: f64| v * 25.4 / 72.0;
        assert!((asked[0] - mm(exit[0])).abs() < 0.01 && (asked[1] - mm(exit[1])).abs() < 0.01, "{command}: asked {asked:?} for {exit:?}");
        assert!(unit["missedBy"]["exit"].as_f64().unwrap() <= POINT_MISS_MM, "{command}: {unit}");
    }
}

/// Stop After goes wherever Trim After goes when objects are made from others: on the last piece of
/// an object cut into several (Divide, Split Into Grid), the last part of a compound path
/// released, the last member of a group ungrouped, the group of type's outlines, a symbol's art
/// placed by Break Link, and what stands for an object whose appearance is expanded. Each operation
/// runs twice, its source marked once with the cut and once with the stop: the same objects end up
/// marked.
#[test]
fn stop_after_goes_where_trim_after_goes() {
    // It lays out type, which may load fonts into the shared font database: held as the tests
    // that add fonts hold it, so as not to clear their caches mid-test.
    let _fonts = crate::adding_fonts();
    type Scenario = fn(&mut Session, &Embroidery) -> Vec<u64>;
    let scenarios: [(&str, Scenario); 7] = [
        ("divide", |s, mark| {
            let a = rect(s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
            let b = rect(s, "#2233cc", 60.0, 30.0, 60.0, 40.0);
            give(s, b, mark);
            run(s, "select.set", json!({"ids": [a, b]}));
            let group = ids(&run(s, "object.pathfinder.divide", json!({})))[0];
            members(s, group)
        }),
        ("split into grid", |s, mark| {
            let a = rect(s, "#cc2222", 20.0, 20.0, 90.0, 90.0);
            give(s, a, mark);
            run(s, "select.set", json!({"ids": [a]}));
            ids(&run(s, "object.path.splitIntoGrid", json!({"rows": 2, "columns": 2})))
        }),
        ("release compound path", |s, mark| {
            let back = rect(s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
            let front = rect(s, "#cc2222", 100.0, 20.0, 60.0, 40.0);
            give(s, back, mark);
            run(s, "select.set", json!({"ids": [back, front]}));
            let compound = run(s, "object.compoundPath.make", json!({}))["id"].as_u64().unwrap();
            run(s, "select.set", json!({"ids": [compound]}));
            run(s, "object.compoundPath.release", json!({}));
            s.doc().unwrap().selection.objects.iter().map(|id| id.0).collect()
        }),
        ("ungroup", |s, mark| {
            let a = rect(s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
            let label = run(s, "text.create", json!({"x": 20, "y": 100, "text": "Hi"}))["id"].as_u64().unwrap();
            let group = run(s, "object.group", json!({"ids": [a, label]}))["id"].as_u64().unwrap();
            give(s, group, mark);
            run(s, "select.set", json!({"ids": [group]}));
            run(s, "object.ungroup", json!({}));
            vec![a, label]
        }),
        ("create outlines", |s, mark| {
            let text = run(s, "text.create", json!({"x": 20, "y": 100, "text": "Hi"}))["id"].as_u64().unwrap();
            give(s, text, mark);
            run(s, "select.set", json!({"ids": [text]}));
            ids(&run(s, "type.createOutlines", json!({})))
        }),
        ("break link", |s, mark| {
            let art = rect(s, "#cc2222", 200.0, 20.0, 40.0, 30.0);
            run(s, "select.set", json!({"ids": [art]}));
            let instance = run(s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
            give(s, instance, mark);
            ids(&run(s, "symbol.breakLink", json!({"ids": [instance]})))
        }),
        ("expand appearance", |s, mark| {
            let a = rect(s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
            run(s, "appearance.addFill", json!({"ids": [a]}));
            give(s, a, mark);
            run(s, "effect.expandAppearance", json!({"ids": [a]}));
            [a].into_iter().chain(members(s, a)).collect()
        }),
    ];
    let mut cut = Embroidery::default();
    cut.set_trim_after(true);
    let mut stop = Embroidery::default();
    stop.set_stop_after(true);
    for (what, scenario) in scenarios {
        let (mut a, mut b) = (blank(), blank());
        let cuts: Vec<bool> = scenario(&mut a, &cut).into_iter().map(|id| embroidery(&a, id).trim_after).collect();
        let stops: Vec<bool> = scenario(&mut b, &stop).into_iter().map(|id| embroidery(&b, id).stop_after).collect();
        assert!(cuts.contains(&true), "{what}: the cut kept: {cuts:?}");
        assert_eq!(stops, cuts, "{what}");
    }
}

/// An object made from a converted object (Pathfinder, Offset Path) is art: it carries the cut and
/// the stop after the object, never the original the manual object keeps to be restored.
#[test]
fn an_object_made_from_a_converted_one_keeps_no_original() {
    let mut s = blank();
    let a = rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    let original = vectorcraft_doc::embroidery::Original {
        frame: vectorcraft_geom::Affine::IDENTITY,
        node: json!({"id": 1}).to_string().into(),
        fingerprint: None,
        extra: Default::default(),
    };
    let mut converted = settings(30.0, true);
    converted.original = Some(original);
    give(&mut s, a, &converted);
    run(&mut s, "select.set", json!({"ids": [a]}));
    let offset = ids(&run(&mut s, "object.path.offsetPath", json!({"offset": 4})))[0];
    let e = embroidery(&s, offset);
    assert!(e.original.is_none() && e.trim_after && angle(&e) == Some(30.0), "{e:?}");
}
