//! An object's own embroidery settings, the first layer a unit's values come from: each part key
//! reaching the unit of its part, the object beating the export and the document, its `stitch`
//! deciding first (`none` not sewn), what a newer version wrote sewn as this version can, an
//! expanded object's pieces and a symbol's art, and the entry point starting the part.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::Embroidery;
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::{Style, Underlay};

use super::digitize::digitize;
use super::plan::{Choice, DesignPlan, Unit};
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// A rectangle filled red at `x, y`, `w × h` points, with a blue stroke `stroke` mm wide (none at
/// 0) → its id.
fn shape(s: &mut Session, x: f64, y: f64, w: f64, h: f64, stroke: f64) -> u64 {
    if stroke > 0.0 {
        run(s, "paint.setStroke", json!({"color": "#2233cc"}));
    } else {
        run(s, "paint.setStroke", json!({"none": true}));
    }
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap();
    if stroke > 0.0 {
        run(s, "stroke.set", json!({"weight": stroke * PT_PER_MM, "ids": [id]}));
    }
    run(s, "select.none", json!({}));
    id
}

/// Give object `id` the embroidery settings `e`, as a file holds them.
fn give(s: &mut Session, id: u64, e: Value) {
    let e: Embroidery = serde_json::from_value(e).unwrap();
    s.edit("Embroidery Object Settings", |d, _| {
        let n = d.node_mut(NodeId(id)).ok_or(EngineError::NoNode(NodeId(id)))?;
        n.extras_mut().embroidery = Some(Box::new(e));
        Ok(())
    })
    .unwrap();
}

/// The active document's plan with the export options `opts`.
fn plan_with(s: &Session, opts: Value) -> DesignPlan {
    digitize(&s.doc().unwrap().doc, &Options::from_json(&opts).unwrap(), None).unwrap()
}

/// The plan's first unit sewn in `style`.
fn sewn(plan: &DesignPlan, style: Style) -> &Unit {
    plan.units.iter().find(|u| u.sewn() == Some(style)).unwrap_or_else(|| panic!("no {style:?} unit"))
}

/// Every part key an object sets reaches the unit of its part, as a value given to the export
/// would: the fill's to the fill, the stroke's to the stroke.
#[test]
fn each_part_key_on_an_object_reaches_its_unit() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 3.0);
    give(
        &mut s,
        id,
        json!({
            "fill": {"spacing": 0.5, "length": 3.0, "angle": 30.0, "pull": [0.3, 0.1], "pull_percent": [5.0, 5.0], "narrow_as_satin": 4.0,
                     "underlay": "edge", "split_length": 8.0},
            "stroke": {"spacing": 0.45, "push": [0.5, 0.2], "pull": 0.25, "underlay": "center", "split_length": 7.0}
        }),
    );
    let plan = plan_with(&s, json!({}));
    let fill = sewn(&plan, Style::Fill).params.unwrap();
    assert_eq!((fill.density, fill.stitch_length, fill.angle), (0.5, 3.0, options::planner_angle(30.0)));
    // A split length is a satin's: a fill's narrow parts take it, its rows don't.
    let rows_split = vectorcraft_stitch::plan::Params::for_style(Style::Fill).max_satin;
    assert_eq!((fill.pull, fill.pull_percent, fill.narrow, fill.underlay, fill.max_satin), ([0.3, 0.1], [5.0; 2], 4.0, Underlay::Edge, rows_split));
    let stroke = sewn(&plan, Style::Satin).params.unwrap();
    assert_eq!(
        (stroke.density, stroke.auto_spacing, stroke.push, stroke.pull, stroke.underlay, stroke.max_satin),
        (0.45, false, [0.5, 0.2], [0.25; 2], Underlay::Center, 7.0)
    );
    // Sewn as the same values given to the export sew the shape without settings of its own.
    let mut bare = blank();
    shape(&mut bare, 20.0, 20.0, 80.0, 60.0, 3.0);
    let exported = plan_with(
        &bare,
        json!({"spacing": 0.5, "length": 3, "angle": 30, "pull": [0.3, 0.1], "pullPercent": 5, "narrowAsSatin": 4, "underlay": "edge", "splitLength": 8}),
    );
    assert_eq!(sewn(&plan, Style::Fill).params, sewn(&exported, Style::Fill).params);
    assert_eq!(plan.unit_stitches(0), exported.unit_stitches(0), "the fill sewn as the export sews it");
}

/// An object's own `expand` grows the area its fill sews all round, and beats the export's: the
/// rows reach 1 mm farther at each side with the object's 1 mm, whatever the export gives, and
/// 0.5 mm with the export's 0.5 mm alone.
#[test]
fn an_objects_expand_grows_its_fill_and_beats_the_exports() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 20.0 * PT_PER_MM, 20.0 * PT_PER_MM, 0.0);
    let width = |s: &Session, opts: Value| {
        let plan = plan_with(s, opts);
        let k = plan.units.iter().position(|u| u.sewn() == Some(Style::Fill)).unwrap();
        let xs: Vec<f64> = plan.unit_stitches(k).iter().filter(|st| st.command == Command::Stitch).map(|st| st.x).collect();
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max) - xs.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let near = |a: f64, b: f64| (a - b).abs() < 0.15;
    let bare = width(&s, json!({}));
    assert!(near(width(&s, json!({"expand": 0.5})), bare + 1.0), "the export's 0.5 mm");
    give(&mut s, id, json!({"fill": {"expand": 1.0}}));
    assert!(near(width(&s, json!({})), bare + 2.0), "the object's 1 mm: {} against {bare}", width(&s, json!({})));
    assert!(near(width(&s, json!({"expand": 0.5})), bare + 2.0), "the object's beats the export's");
}

/// The object's value beats the export's, which beats the document's, which beats the default.
#[test]
fn the_object_beats_the_export_and_the_document() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = Some(Box::new(serde_json::from_value(json!({"fabric": {"tatami": {"pull": 0.35}}})).unwrap()));
        Ok(())
    })
    .unwrap();
    let pull = |s: &Session, opts: Value| sewn(&plan_with(s, opts), Style::Fill).params.unwrap().pull[0];
    assert_eq!(pull(&s, json!({})), 0.35, "the document's");
    assert_eq!(pull(&s, json!({"pull": 0.1})), 0.1, "the export's");
    give(&mut s, id, json!({"fill": {"pull": 0.3}}));
    assert_eq!(pull(&s, json!({"pull": 0.1})), 0.3, "the object's");
    assert_eq!(pull(&s, json!({})), 0.3);
    // Another object, with none of its own, still takes the export's.
    let other = shape(&mut s, 150.0, 20.0, 40.0, 40.0, 0.0);
    let plan = plan_with(&s, json!({"pull": 0.1}));
    let of = |id: u64| plan.units.iter().find(|u| u.origin.node == NodeId(id)).unwrap().params.unwrap().pull[0];
    assert_eq!((of(id), of(other)), (0.3, 0.1));
}

/// The object's `stitch` decides first: `auto` sews by the rule even where the export forces a
/// style, a style beats the export's, and `none` leaves the part unsewn, saying nothing.
#[test]
fn an_objects_stitch_decides_first() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 3.0);
    give(&mut s, id, json!({"fill": {"stitch": "auto"}, "stroke": {"stitch": "tatami"}}));
    let plan = plan_with(&s, json!({"stitch": "running"}));
    let whys: Vec<Choice> = plan.units.iter().map(|u| u.why).collect();
    assert_eq!(whys, [Choice::Fill, Choice::StrokeFilled], "the fill by the rule, the stroke filled");
    give(&mut s, id, json!({"fill": {"stitch": "none"}, "stroke": {"stitch": "satin"}}));
    let plan = plan_with(&s, json!({"stitch": "running"}));
    assert_eq!(
        plan.units.iter().map(|u| (u.sewn(), u.why)).collect::<Vec<_>>(),
        [(Some(Style::Satin), Choice::Forced(super::plan::ForcedBy::Object))],
        "the fill not sewn"
    );
    assert!(plan.messages().is_empty(), "{:?}", plan.messages());
    // Not sewn on both parts: nothing to sew.
    give(&mut s, id, json!({"fill": {"stitch": "none"}, "stroke": {"stitch": "none"}}));
    assert!(plan_with(&s, json!({})).units.is_empty());
}

/// What a newer version wrote is sewn as this version can: a value out of its range at the nearest
/// it takes, a stitch, an underlay or a key it doesn't know left out (the layers below give it); the
/// plan says each once.
#[test]
fn a_newer_versions_object_settings_are_sewn_as_this_version_can() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    give(&mut s, id, json!({"fill": {"pull": 5.0, "length": 0.01}}));
    let plan = plan_with(&s, json!({}));
    let p = sewn(&plan, Style::Fill).params.unwrap();
    assert_eq!((p.pull, p.stitch_length), ([vectorcraft_stitch::plan::PULL_RANGE.1; 2], vectorcraft_stitch::plan::STITCH_LENGTH_RANGE.0));
    assert_eq!(plan.notes, [super::resolve::OBJECT_SETTINGS_OUT_OF_RANGE]);
    give(&mut s, id, json!({"fill": {"stitch": "zigzagFill", "underlay": "triple", "grain": 45}}));
    let plan = plan_with(&s, json!({"stitch": "running", "underlay": "edge"}));
    let u = &plan.units[0];
    assert_eq!((u.sewn(), u.params.unwrap().underlay), (Some(Style::Running), Underlay::Edge), "the export's");
    assert_eq!(plan.notes, [super::resolve::OBJECT_SETTINGS_UNREAD]);
}

/// An expanded object's pieces are sewn with the settings of the part each came from.
#[test]
fn an_expanded_objects_pieces_are_sewn_with_their_parts_settings() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    run(&mut s, "appearance.addFill", json!({"ids": [id]}));
    give(&mut s, id, json!({"fill": {"angle": 30.0, "pull": 0.3}}));
    let plan = plan_with(&s, json!({}));
    let fills: Vec<_> = plan.units.iter().filter_map(|u| u.params).collect();
    assert_eq!(fills.len(), 2, "both fills");
    assert!(fills.iter().all(|p| p.angle == options::planner_angle(30.0) && p.pull == [0.3; 2]), "{fills:?}");
}

/// A symbol's art keeps its settings in every instance, its angle turned with the instance.
#[test]
fn a_symbols_art_keeps_its_settings_in_every_instance() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 40.0, 0.0);
    give(&mut s, id, json!({"fill": {"angle": 30.0, "pull": 0.3}}));
    run(&mut s, "select.set", json!({"ids": [id]}));
    let first = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    let second = run(&mut s, "symbol.place", json!({"name": "Badge", "x": 200, "y": 200}))["id"].as_u64().unwrap();
    run(&mut s, "object.rotate", json!({"ids": [second], "angle": 90}));
    let plan = plan_with(&s, json!({}));
    let of = |instance: u64| plan.units.iter().find(|u| u.origin.instance == Some(NodeId(instance))).unwrap().params.unwrap();
    let (a, b) = (of(first), of(second));
    assert_eq!((a.pull, b.pull), ([0.3; 2], [0.3; 2]));
    assert!((a.angle - options::planner_angle(30.0)).abs() < 1e-9, "{}", a.angle);
    assert!((b.angle - options::planner_angle(120.0)).abs() < 1e-6, "turned with its instance: {}", b.angle);
}

/// A part starts at the entry point its object sets, whatever came before: exactly there where its
/// stitches can start anywhere (a fill, a little inside its outline, where its edge run runs; a
/// closed line), at the nearer end of an open line. Mutation caught: the entry taken as where the
/// needle is starts the fill at the corner nearest, millimetres off.
#[test]
fn an_entry_point_starts_the_part() {
    let first_point = |plan: &DesignPlan, unit: usize| {
        let st = plan.unit_stitches(unit).iter().find(|s| s.command == Command::Stitch).copied().unwrap();
        (st.x, st.y)
    };
    let off = |(x, y): (f64, f64), (px, py): (f64, f64)| (x - px / PT_PER_MM).hypot(y - py / PT_PER_MM);
    // An open line drawn left to right, entered near its right end: it starts at that end.
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let line = run(&mut s, "path.create", json!({"d": "M 20 100 L 200 100"}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    assert!(off(first_point(&plan_with(&s, json!({})), 0), (20.0, 100.0)) < 1e-6, "drawn from its left end");
    give(&mut s, line, json!({"stroke": {"points": {"entry": [150.0, 100.0]}}}));
    assert!(off(first_point(&plan_with(&s, json!({})), 0), (200.0, 100.0)) < 1e-6, "entered from its right end");
    // A rectangle, filled with and without underlay, entered in the middle of its sides and near a
    // corner: it starts there, within half a row spacing (0.19 mm) inside.
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    for underlay in ["none", "auto"] {
        for at in [[60.0, 20.0], [100.0, 50.0], [25.0, 80.0], [20.0, 30.0]] {
            give(&mut s, id, json!({"fill": {"points": {"entry": at}, "underlay": underlay}}));
            let start = first_point(&plan_with(&s, json!({})), 0);
            assert!(off(start, (at[0], at[1])) < 0.2, "underlay {underlay}, {at:?}: {start:?}");
        }
    }
    // The same rectangle's outline sewn as a running line: it starts exactly at the entry, between
    // two corners.
    give(&mut s, id, json!({"fill": {"stitch": "running", "points": {"entry": [60.0, 80.0]}}}));
    let start = first_point(&plan_with(&s, json!({})), 0);
    assert!(off(start, (60.0, 80.0)) < 1e-6, "a closed running line: {start:?}");
}

/// A part ends at its exit point as near as its stitches can reach it, without a jump, a cut or a
/// stitch that shows more: an open running line whose start is free ends at its end nearest the
/// exit, from wherever the needle comes; entered at a set point it ends where its stitches take it;
/// a closed one goes on round to the exit in its own holes. Mutation caught: the exit read and not
/// planned toward leaves the line ending at its far end.
#[test]
fn an_exit_point_ends_the_part() {
    let last_point = |plan: &DesignPlan, unit: usize| {
        let st = plan.unit_stitches(unit).iter().rev().find(|s| s.command == Command::Stitch).copied().unwrap();
        (st.x, st.y)
    };
    let off = |(x, y): (f64, f64), (px, py): (f64, f64)| (x - px / PT_PER_MM).hypot(y - py / PT_PER_MM);
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let line = run(&mut s, "path.create", json!({"d": "M 20 100 L 200 100"}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    assert!(off(last_point(&plan_with(&s, json!({})), 0), (200.0, 100.0)) < 1e-6, "drawn to its right end");
    give(&mut s, line, json!({"stroke": {"points": {"exit": [30.0, 104.0]}}}));
    assert!(off(last_point(&plan_with(&s, json!({})), 0), (20.0, 100.0)) < 1e-6, "turned to end at its left end, by the exit");
    give(&mut s, line, json!({"stroke": {"points": {"entry": [20.0, 100.0], "exit": [30.0, 104.0]}}}));
    assert!(off(last_point(&plan_with(&s, json!({})), 0), (200.0, 100.0)) < 1e-6, "its entry set: it ends at its other end");
    // A satin column 6 mm wide (its underlay an edge run and a zigzag, which bring its top
    // stitching back to where it starts), entered at its left end: a pass along its middle ends it
    // at its right end, by the exit.
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let satin = run(&mut s, "path.create", json!({"d": "M 20 100 L 200 100"}))["id"].as_u64().unwrap();
    run(&mut s, "stroke.set", json!({"weight": 6.0 * PT_PER_MM, "ids": [satin]}));
    run(&mut s, "select.none", json!({}));
    give(&mut s, satin, json!({"stroke": {"points": {"entry": [20.0, 100.0]}}}));
    assert!(off(last_point(&plan_with(&s, json!({})), 0), (20.0, 100.0)) < 4.0, "its top stitching comes back to its left end");
    give(&mut s, satin, json!({"stroke": {"points": {"entry": [20.0, 100.0], "exit": [200.0, 100.0]}}}));
    assert!(off(last_point(&plan_with(&s, json!({})), 0), (200.0, 100.0)) < 4.0, "turned round by a pass along its middle");
    // A rectangle's outline sewn as a running line goes on round to its exit, in its own holes.
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    give(&mut s, id, json!({"fill": {"stitch": "running", "points": {"entry": [20.0, 20.0], "exit": [100.0, 50.0]}}}));
    let plan = plan_with(&s, json!({}));
    let stitch = plan.units[0].params.unwrap().stitch_length;
    assert!(off(last_point(&plan, 0), (100.0, 50.0)) <= 0.65 * stitch, "ends at its exit: {:?}", last_point(&plan, 0));
}

/// A fill angle the object doesn't set turns with the object, from its bounding box's angle: the
/// default rows and an export's angle alike; a set angle turns as it is stored; turned back
/// upright, the object is planned as before, bit for bit.
#[test]
fn an_unset_angle_turns_with_its_object() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 40.0, 0.0);
    let angle = |s: &Session, opts: Value| sewn(&plan_with(s, opts), Style::Fill).params.unwrap().angle;
    let upright = plan_with(&s, json!({}));
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 30}));
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(angle(&s, json!({})), options::planner_angle(135.0 + 30.0)), "the default rows turned");
    assert!(close(angle(&s, json!({"angle": 100})), options::planner_angle(100.0 + 30.0)), "the export's turned");
    give(&mut s, id, json!({"fill": {"angle": 10.0}}));
    assert!(close(angle(&s, json!({"angle": 100})), options::planner_angle(10.0)), "a set angle as it is stored");
    give(&mut s, id, json!({}));
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": -30}));
    let back = plan_with(&s, json!({}));
    assert_eq!(sewn(&back, Style::Fill).params.unwrap().angle, sewn(&upright, Style::Fill).params.unwrap().angle);
}

/// A mirror maps an object's box to the image of its axis nearest it: an upright object's unset
/// rows stay as they were; a turned object's go with its box (a box at 30° goes to −30°, its rows
/// from 165° to 105°, not to their mirror image, 15°); a set angle mirrors with the object.
#[test]
fn a_mirror_turns_unset_rows_with_the_box() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 40.0, 0.0);
    let angle = |s: &Session| sewn(&plan_with(s, json!({})), Style::Fill).params.unwrap().angle;
    let upright = angle(&s);
    run(&mut s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
    assert_eq!(angle(&s), upright, "upright, mirrored: as it was");
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 30}));
    run(&mut s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
    assert!((angle(&s) - options::planner_angle(135.0 - 30.0)).abs() < 1e-9, "{}", angle(&s));
    give(&mut s, id, json!({"fill": {"angle": 165.0}}));
    run(&mut s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
    // Rows are lines: 15° and 195° (the half turn a mirror adds) are the same rows.
    let rows = (angle(&s) - options::planner_angle(15.0)).rem_euclid(180.0);
    assert!(rows.min(180.0 - rows) < 1e-9, "a set angle mirrored: {}", angle(&s));
}

/// The fill angles of every unit the plan sews as a fill area.
fn fill_angles(plan: &DesignPlan) -> Vec<f64> {
    plan.units.iter().filter(|u| u.sewn() == Some(Style::Fill)).filter_map(|u| u.params).map(|p| p.angle).collect()
}

/// A member of a group the walk expands (a drop shadow on the group, left off; a stroke of the
/// group's own) turns its unset rows with its own box, as it does on its own.
#[test]
fn a_member_of_an_expanded_group_turns_with_its_own_box() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 40.0, 0.0);
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 45}));
    let group = run(&mut s, "object.group", json!({"ids": [id]}))["id"].as_u64().unwrap();
    let alone = fill_angles(&plan_with(&s, json!({})));
    assert!(alone.len() == 1 && (alone[0] - options::planner_angle(135.0 + 45.0)).abs() < 1e-9, "{alone:?}");
    run(&mut s, "effect.apply", json!({"effect": "stylize.dropShadow", "ids": [group]}));
    assert_eq!(fill_angles(&plan_with(&s, json!({}))), alone, "a drop shadow on the group");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    run(&mut s, "appearance.addStroke", json!({"ids": [group]}));
    assert!(fill_angles(&plan_with(&s, json!({}))).contains(&alone[0]), "a stroke of the group's own");
}

/// An object the walk expands (two fills, a stroke an effect reshapes) turned: each of its pieces,
/// the stroke's outline included, turns its unset rows with the object.
#[test]
fn an_expanded_objects_pieces_turn_with_it() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 3.0);
    run(&mut s, "appearance.addFill", json!({"ids": [id]}));
    let items = s.doc().unwrap().doc.node(NodeId(id)).unwrap().appearance.items.clone();
    let stroke = items.iter().position(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_))).unwrap();
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [id], "item": stroke}));
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 30}));
    let angles = fill_angles(&plan_with(&s, json!({})));
    assert_eq!(angles.len(), 3, "two fills and the stroke's area");
    assert!(angles.iter().all(|a| (a - options::planner_angle(135.0 + 30.0)).abs() < 1e-9), "{angles:?}");
}

/// A symbol placed twice, one instance turned: the turned instance's art turns its unset rows with
/// it, the other sews as the art does unplaced; art the walk expands (two fills) and an instance
/// with an effect of its own (its art turned in the symbol too) alike.
#[test]
fn a_symbols_art_turns_its_unset_rows_with_its_instance() {
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 40.0, 0.0);
    let unplaced = fill_angles(&plan_with(&s, json!({})));
    run(&mut s, "select.set", json!({"ids": [id]}));
    let first = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    let second = run(&mut s, "symbol.place", json!({"name": "Badge", "x": 200, "y": 200}))["id"].as_u64().unwrap();
    run(&mut s, "object.rotate", json!({"ids": [second], "angle": 30}));
    let of = |plan: &DesignPlan, instance: u64| -> Vec<f64> {
        let units = plan.units.iter().filter(|u| u.origin.instance == Some(NodeId(instance)) && u.sewn() == Some(Style::Fill));
        units.filter_map(|u| u.params).map(|p| p.angle).collect()
    };
    let turned = options::planner_angle(135.0 + 30.0);
    let plan = plan_with(&s, json!({}));
    assert_eq!(of(&plan, first), unplaced, "the upright instance as the art unplaced");
    assert!(of(&plan, second).iter().all(|a| (a - turned).abs() < 1e-9), "{:?}", of(&plan, second));
    // Art the walk expands: two fills.
    fn second_fill(n: &mut vectorcraft_doc::Node) {
        if matches!(n.kind, vectorcraft_doc::NodeKind::Path { .. })
            && let Some(fill) = n.appearance.items.first().cloned()
        {
            n.appearance.items.push(fill);
        }
        for c in n.children_mut().into_iter().flatten() {
            second_fill(std::sync::Arc::make_mut(c));
        }
    }
    s.edit("Two Fills", |d, _| {
        second_fill(std::sync::Arc::make_mut(&mut d.symbols.first_mut().unwrap().art));
        Ok(())
    })
    .unwrap();
    let plan = plan_with(&s, json!({}));
    assert_eq!(of(&plan, first).len(), 2, "both fills");
    assert!(of(&plan, second).iter().all(|a| (a - turned).abs() < 1e-9), "{:?}", of(&plan, second));
    // An instance with an effect of its own, its art turned 20° in the symbol.
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [second]}));
    s.edit("Art Turned", |d, _| {
        let sym = d.symbols.first_mut().unwrap();
        std::sync::Arc::make_mut(&mut sym.art).transform(vectorcraft_geom::Affine::rotate(20f64.to_radians()), false);
        Ok(())
    })
    .unwrap();
    // The art's own box (turned 20°, as `transform` keeps it) as the instance places it, scaled.
    let doc = &s.doc().unwrap().doc;
    let vectorcraft_doc::NodeKind::SymbolInstance { xf, .. } = doc.node(NodeId(second)).unwrap().kind else { panic!() };
    let both = options::planner_angle(135.0 + vectorcraft_doc::orient::transformed_angle(doc.symbols[0].art.bbox_angle, xf));
    let angles = of(&plan_with(&s, json!({})), second);
    assert!(!angles.is_empty() && angles.iter().all(|a| (a - both).abs() < 1e-6), "{angles:?}");
}

/// The skip reasons listed for interfaces are every one, each once: a variant left out of the list
/// would have no words in a panel.
#[test]
fn every_skip_reason_is_listed_once() {
    let ids = super::skip_reasons();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    for id in ["literal", "guide", "type", "image", "symbolInstance", "live", "placed", "intertwine", "noPart"] {
        assert!(ids.contains(&id), "{id}");
    }
}
