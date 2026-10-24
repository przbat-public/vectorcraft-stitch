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
            "stroke": {"spacing": 0.45, "push": [0.5, 0.2], "pull": 0.25, "underlay": "center"}
        }),
    );
    let plan = plan_with(&s, json!({}));
    let fill = sewn(&plan, Style::Fill).params.unwrap();
    assert_eq!((fill.density, fill.stitch_length, fill.angle), (0.5, 3.0, options::planner_angle(30.0)));
    assert_eq!((fill.pull, fill.pull_percent, fill.narrow, fill.underlay, fill.max_satin), ([0.3, 0.1], [5.0; 2], 4.0, Underlay::Edge, 8.0));
    let stroke = sewn(&plan, Style::Satin).params.unwrap();
    assert_eq!(
        (stroke.density, stroke.auto_spacing, stroke.push, stroke.pull, stroke.underlay),
        (0.45, false, [0.5, 0.2], [0.25; 2], Underlay::Center)
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
    assert_eq!(plan.units.iter().map(|u| (u.sewn(), u.why)).collect::<Vec<_>>(), [(Some(Style::Satin), Choice::Forced)], "the fill not sewn");
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

/// A part starts at the entry point its object sets: its first needle point is the start it can
/// take nearest to it (an end of a line, the corner a fill's rows begin at), whatever came before.
#[test]
fn an_entry_point_starts_the_part() {
    let first_point = |plan: &DesignPlan, unit: usize| {
        let st = plan.unit_stitches(unit).iter().find(|s| s.command == Command::Stitch).copied().unwrap();
        (st.x, st.y)
    };
    let close = |(x, y): (f64, f64), (px, py): (f64, f64)| (x - px / PT_PER_MM).hypot(y - py / PT_PER_MM) < 1.5;
    // A line drawn left to right, entered from its right end.
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let line = run(&mut s, "path.create", json!({"d": "M 20 100 L 200 100"}))["id"].as_u64().unwrap();
    run(&mut s, "select.none", json!({}));
    assert!(close(first_point(&plan_with(&s, json!({})), 0), (20.0, 100.0)), "drawn from its left end");
    give(&mut s, line, json!({"stroke": {"points": {"entry": [200.0, 100.0]}}}));
    assert!(close(first_point(&plan_with(&s, json!({})), 0), (200.0, 100.0)), "entered from its right end");
    // A rectangle, whose rows (135°) can start at its upper right or lower left corner: entered at
    // either.
    let mut s = blank();
    let id = shape(&mut s, 20.0, 20.0, 80.0, 60.0, 0.0);
    for corner in [[100.0, 20.0], [20.0, 80.0]] {
        give(&mut s, id, json!({"fill": {"points": {"entry": corner}, "underlay": "none"}}));
        let start = first_point(&plan_with(&s, json!({})), 0);
        assert!(close(start, (corner[0], corner[1])), "{corner:?}: {start:?}");
    }
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
