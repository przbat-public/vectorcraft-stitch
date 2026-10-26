//! An expanded object's strokes (an appearance of two strokes, a stroke reshaped by an effect):
//! the strokes nothing reshapes are sewn along their path as a basic object's stroke is, by their
//! width or their settings, one line where a running stitch is set; a stroke an effect reshapes is
//! sewn as the area it paints, and a running or satin stitch set on it is said not to follow its
//! path; turned and mirrored objects alike; the cut after the object once, after its last piece;
//! all of it for an object in a group the walk expands (a group with a stroke of its own), and for
//! its pieces when Expand Appearance expands that group.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKind};
use vectorcraft_doc::{AppearanceItem, NodeId};
use vectorcraft_geom::kurbo::{ParamCurveNearest, Point};
use vectorcraft_geom::{Affine, BezPath};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::Style;

use super::digitize::digitize;
use super::plan::{Choice, DesignPlan, Part};
use super::printed::STROKE_LOSES_ITS_PATH;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// A rectangle filled red with two strokes, the lower 3 mm wide (a satin column by its width), the
/// upper 0.5 mm (a running line), at `x` → its id.
fn two_strokes(s: &mut Session, x: f64) -> u64 {
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    run(s, "paint.setStroke", json!({"color": "#2233cc"}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": 20, "width": 80, "height": 60}))["id"].as_u64().unwrap();
    run(s, "stroke.set", json!({"weight": 3.0 * PT_PER_MM, "ids": [id]}));
    run(s, "appearance.addStroke", json!({"ids": [id]}));
    s.edit("Second Stroke", |d, _| {
        let n = d.node_mut(NodeId(id)).ok_or(EngineError::NoNode(NodeId(id)))?;
        if let Some(AppearanceItem::Stroke(st)) = n.appearance.items.last_mut() {
            st.width = 0.5 * PT_PER_MM;
        }
        Ok(())
    })
    .unwrap();
    run(s, "select.none", json!({}));
    id
}

fn plan_of(s: &Session, opts: Value) -> DesignPlan {
    digitize(&s.doc().unwrap().doc, &Options::from_json(&opts).unwrap(), None).unwrap()
}

/// Object `id`'s path in the active document, millimetres.
fn path_mm(s: &Session, id: u64) -> BezPath {
    let mut bp = s.doc().unwrap().doc.node(NodeId(id)).unwrap().path_data().unwrap().to_bezpath();
    bp.apply_affine(Affine::scale(1.0 / PT_PER_MM));
    bp
}

/// How far the farthest needle point of unit `k` of `plan` lies from `path` (millimetres).
fn farthest(plan: &DesignPlan, k: usize, path: &BezPath) -> f64 {
    let near = |p: Point| path.segments().map(|seg| seg.nearest(p, 1e-6).distance_sq).fold(f64::INFINITY, f64::min).sqrt();
    plan.unit_stitches(k).iter().filter(|st| st.command == Command::Stitch).map(|st| near(Point::new(st.x, st.y))).fold(0.0, f64::max)
}

/// The units of object `id`'s strokes: each one's index, style and rule.
fn strokes(plan: &DesignPlan, id: u64) -> Vec<(usize, Option<Style>, Choice)> {
    plan.units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.origin.selected() == NodeId(id) && matches!(u.origin.part, Part::Stroke(_)))
        .map(|(k, u)| (k, u.sewn(), u.why))
        .collect()
}

/// Two strokes on one object (an appearance the walk expands) are sewn as a basic object's stroke
/// is: a satin column for the wide one and a running line for the narrow one, along the path,
/// each as a stroke of the object (not as the outline of the area it paints).
#[test]
fn an_expanded_objects_strokes_are_sewn_along_their_path() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    let plan = plan_of(&s, json!({}));
    let units = strokes(&plan, id);
    let styles: Vec<(Option<Style>, Choice)> = units.iter().map(|(_, style, why)| (*style, *why)).collect();
    assert_eq!(styles, [(Some(Style::Satin), Choice::WideStroke), (Some(Style::Running), Choice::NarrowStroke)]);
    let path = path_mm(&s, id);
    let (satin, running) = (units[0].0, units[1].0);
    assert!(farthest(&plan, satin, &path) < 1.5 + 0.5, "the satin's rails about the path: {}", farthest(&plan, satin, &path));
    assert!(farthest(&plan, running, &path) < 0.05, "the running line on the path: {}", farthest(&plan, running, &path));
    // The fill is sewn as before, as the object's fill.
    assert!(plan.units.iter().any(|u| u.origin.selected() == NodeId(id) && matches!(u.origin.part, Part::Fill(_)) && u.why == Choice::Fill));
}

/// A running stitch set on the object's strokes sews each of them as one line along the path,
/// never two round the outline of the area it paints; satin on both, a column each.
#[test]
fn a_running_stitch_set_on_an_expanded_objects_strokes_sews_one_line() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    let path = path_mm(&s, id);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "running"}));
    let plan = plan_of(&s, json!({}));
    let units = strokes(&plan, id);
    assert_eq!(units.iter().map(|(_, style, _)| *style).collect::<Vec<_>>(), [Some(Style::Running); 2]);
    for (k, ..) in &units {
        assert!(farthest(&plan, *k, &path) < 0.05, "unit {k}: {}", farthest(&plan, *k, &path));
    }
    assert!(plan.messages().is_empty(), "{:?}", plan.messages());
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "satin"}));
    let plan = plan_of(&s, json!({}));
    assert_eq!(strokes(&plan, id).iter().map(|(_, style, _)| *style).collect::<Vec<_>>(), [Some(Style::Satin); 2]);
    // The export's stitch reaches them too.
    run(&mut s, "embroidery.reset", json!({"ids": [id]}));
    let plan = plan_of(&s, json!({"stitch": "running"}));
    for (k, style, _) in strokes(&plan, id) {
        assert_eq!(style, Some(Style::Running));
        assert!(farthest(&plan, k, &path) < 0.05);
    }
}

/// A stroke an effect reshapes is sewn as the area it paints, as before; the other stroke stays a
/// stroke. A running stitch set on the strokes (or given to the export) can't follow the reshaped
/// one's path, and the plan says so, naming the object; with none set, nothing is said.
#[test]
fn a_stroke_an_effect_reshapes_is_sewn_as_its_area_and_said() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    // The 3 mm stroke (item 1, above the fill) roughened.
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [id], "item": 1}));
    let plan = plan_of(&s, json!({}));
    let units = strokes(&plan, id);
    assert_eq!(units.iter().map(|(_, style, _)| *style).collect::<Vec<_>>(), [Some(Style::Running)], "the unreshaped stroke alone a stroke");
    assert!(
        plan.units.iter().filter(|u| u.origin.selected() == NodeId(id) && matches!(u.origin.part, Part::Fill(_))).count() >= 2,
        "the fill and the roughened stroke's area"
    );
    assert!(!plan.warnings.iter().any(|w| w == STROKE_LOSES_ITS_PATH));
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "running"}));
    let plan = plan_of(&s, json!({}));
    assert!(plan.warnings.iter().any(|w| w == STROKE_LOSES_ITS_PATH), "{:?}", plan.warnings);
    assert_eq!(plan.warned.get(STROKE_LOSES_ITS_PATH).map(Vec::as_slice), Some(&[NodeId(id)][..]));
    run(&mut s, "embroidery.reset", json!({"ids": [id]}));
    assert!(plan_of(&s, json!({"stitch": "satin"})).warnings.iter().any(|w| w == STROKE_LOSES_ITS_PATH), "the export's satin too");
}

/// Turned and mirrored, an expanded object's strokes still follow its path; the settings turn and
/// mirror with it (a mirror swaps the pull compensation's sides).
#[test]
fn turned_and_mirrored_strokes_follow_the_path() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "pull": [0.3, 0.1]}));
    run(&mut s, "object.rotate", json!({"ids": [id], "angle": 30}));
    run(&mut s, "object.reflect", json!({"ids": [id], "axis": "vertical"}));
    let path = path_mm(&s, id);
    let plan = plan_of(&s, json!({}));
    let units = strokes(&plan, id);
    assert_eq!(units.iter().map(|(_, style, _)| *style).collect::<Vec<_>>(), [Some(Style::Satin), Some(Style::Running)]);
    assert!(farthest(&plan, units[1].0, &path) < 0.05, "the running line on the turned path");
    assert!(farthest(&plan, units[0].0, &path) < 2.0, "the satin about it");
    assert_eq!(plan.units[units[0].0].params.unwrap().pull, [0.1, 0.3], "mirrored: the sides swapped");
    let e: Embroidery = s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().embroidery.as_deref().cloned().unwrap();
    assert_eq!(e.part(PartKind::Stroke).and_then(|p| p.pull), Some([0.1, 0.3]));
}

/// The cut after an expanded object comes once, after its last piece; `embroidery.info` counts
/// its strokes' units under its strokes.
#[test]
fn an_expanded_object_is_cut_once_and_its_strokes_counted_as_strokes() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    two_strokes(&mut s, 120.0);
    // Each of the object's units' cuts, in sewing order.
    let cuts = |plan: &DesignPlan| -> Vec<usize> {
        let units = plan.units.iter().enumerate().filter(|(_, u)| u.origin.selected() == NodeId(id));
        units.map(|(k, _)| plan.unit_stitches(k).iter().filter(|st| st.command == Command::Trim).count()).collect()
    };
    let without = cuts(&plan_of(&s, json!({})));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id], "on": true}));
    let plan = plan_of(&s, json!({}));
    let with = cuts(&plan);
    let (last, before) = with.split_last().unwrap();
    assert_eq!(before, &without[..without.len() - 1], "no cut between its pieces");
    assert!(*last >= 1, "a cut after its last piece");
    let last_unit = plan.units.iter().rposition(|u| u.origin.selected() == NodeId(id)).unwrap();
    assert_eq!(plan.unit_stitches(last_unit).last().map(|st| st.command), Some(Command::Trim));
    let info = run(&mut s, "embroidery.info", json!({"ids": [id], "plan": true}));
    assert_eq!(info["stroke"]["sewing"]["unitCount"], json!(2));
}

/// `two_strokes` at x 20 in a group with a stroke of its own (an appearance the walk expands, and
/// the object with it) → (the object's id, the group's id).
fn in_a_stroked_group(s: &mut Session) -> (u64, u64) {
    let id = two_strokes(s, 20.0);
    let group = run(s, "object.group", json!({"ids": [id]}))["id"].as_u64().unwrap();
    run(s, "appearance.addStroke", json!({"ids": [group]}));
    run(s, "select.none", json!({}));
    (id, group)
}

/// An object in a group the walk expands is sewn as it is on its own: its strokes along its path,
/// none of them where its strokes are set to sew nothing, and the cut after it once, after its last
/// piece.
#[test]
fn an_object_in_an_expanded_group_keeps_its_settings() {
    let mut s = blank();
    let (id, _) = in_a_stroked_group(&mut s);
    let path = path_mm(&s, id);
    let stroke_units = |plan: &DesignPlan| -> Vec<(usize, Option<Style>)> {
        plan.units.iter().enumerate().filter(|(_, u)| matches!(u.origin.part, Part::Stroke(_))).map(|(k, u)| (k, u.sewn())).collect()
    };
    let plan = plan_of(&s, json!({}));
    let units = stroke_units(&plan);
    assert_eq!(units.iter().map(|(_, style)| *style).collect::<Vec<_>>(), [Some(Style::Satin), Some(Style::Running)]);
    assert!(farthest(&plan, units[1].0, &path) < 0.05, "the running line on the path");
    let all = plan.units.len();
    // Its strokes set to sew nothing: neither is sewn, as a stroke or as the area it paints.
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "none"}));
    let plan = plan_of(&s, json!({}));
    assert!(stroke_units(&plan).is_empty());
    assert_eq!(plan.units.len(), all - 2, "its fill and the group's stroke alone");
    // The cut after it: once, after its last piece (its upper stroke).
    run(&mut s, "embroidery.reset", json!({"ids": [id]}));
    let cuts = |plan: &DesignPlan| -> Vec<usize> {
        (0..plan.units.len()).map(|k| plan.unit_stitches(k).iter().filter(|st| st.command == Command::Trim).count()).collect()
    };
    let without = cuts(&plan_of(&s, json!({})));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id], "on": true}));
    let plan = plan_of(&s, json!({}));
    let with = cuts(&plan);
    let changed: Vec<usize> = (0..with.len()).filter(|k| with[*k] != without[*k]).collect();
    let upper = stroke_units(&plan)[1].0;
    assert_eq!(changed, [upper], "{without:?} → {with:?}");
    assert_eq!(plan.unit_stitches(upper).last().map(|st| st.command), Some(Command::Trim));
}

/// Expand Appearance of a group gives the pieces of an object in it the settings of the parts they
/// come from, and the cut after the object to what stands for it alone.
#[test]
fn expanding_a_group_gives_its_objects_pieces_their_parts() {
    let mut s = blank();
    let (id, group) = in_a_stroked_group(&mut s);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 10}));
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "stitch": "none"}));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id], "on": true}));
    run(&mut s, "effect.expandAppearance", json!({"ids": [group]}));
    let d = &s.doc().unwrap().doc;
    let object = d.node(NodeId(id)).unwrap();
    let e = |n: &vectorcraft_doc::Node| n.extras().embroidery.as_deref().cloned().unwrap_or_default();
    assert!(e(object).trim_after && e(object).part(PartKind::Fill).is_none(), "the object's stand-in: the cut alone");
    let pieces = object.children().unwrap();
    assert_eq!(pieces.len(), 3, "its fill and its two strokes");
    let fills: Vec<Option<String>> = pieces.iter().map(|p| e(p).part(PartKind::Fill).and_then(|f| f.stitch.clone())).collect();
    assert_eq!(fills, [None, Some("none".into()), Some("none".into())], "the fill's copy, then the strokes' outlines");
    assert_eq!(e(&pieces[0]).part(PartKind::Fill).and_then(|f| f.angle), Some(10.0));
    assert!(pieces.iter().all(|p| !e(p).trim_after), "no cut between its pieces");
}

/// The walk splits an object's two strokes into a copy each, both on its path: the stroke's entry
/// goes on the first copy and its exit on the last (they lie on one another), so the stroke is
/// started at its entry and ended at its exit once. Mutation caught: `split_strokes` losing its
/// `points_on_nearest`, which would plan each stroke from the entry toward the exit.
#[test]
fn an_expanded_objects_stroke_points_go_on_its_first_and_last_strokes() {
    let mut s = blank();
    let id = two_strokes(&mut s, 20.0);
    run(&mut s, "embroidery.setEntryExit", json!({"id": id, "part": "stroke", "entry": [20, 20], "exit": [100, 80]}));
    let plan = plan_of(&s, json!({}));
    let asked: Vec<(bool, bool)> =
        strokes(&plan, id).iter().filter_map(|(k, ..)| plan.units.get(*k)).map(|u| (u.asked.entry.is_some(), u.asked.exit.is_some())).collect();
    assert_eq!(asked, vec![(true, false), (false, true)], "the first stroke from the entry, the last toward the exit");
}
