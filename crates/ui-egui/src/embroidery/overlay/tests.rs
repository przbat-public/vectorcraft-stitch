//! The selection in the stitch view: the frame alone with the Selection tool; an entry and an exit
//! marker per selected object, labelled for one; changed objects outlined dashed until their plan
//! comes; the selected objects that aren't sewn counted.

use egui::{Color32, Shape};
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::embroidery::stitch_preview;
use crate::{Services, canvas};

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for x in [20, 110, 200] {
        app.run("shape.rectangle", json!({"x": x, "y": 40, "width": 70, "height": 70})).unwrap();
    }
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app
}

/// One headless frame at `time` → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut all = vec![];
    flatten(out.shapes.into_iter().map(|c| c.shape).collect(), &mut all);
    all
}

fn flatten(shapes: Vec<Shape>, out: &mut Vec<Shape>) {
    for s in shapes {
        match s {
            Shape::Vec(v) => flatten(v, out),
            s => out.push(s),
        }
    }
}

/// The entry markers drawn: diamonds filled in `accent`.
fn entries(shapes: &[Shape], accent: Color32) -> usize {
    shapes.iter().filter(|s| matches!(s, Shape::Path(p) if p.closed && p.points.len() == 4 && p.fill == accent)).count()
}

/// The exit markers drawn: white circles.
fn exits(shapes: &[Shape]) -> usize {
    shapes.iter().filter(|s| matches!(s, Shape::Circle(c) if c.fill == Color32::WHITE && (c.radius - EXIT_MARKER_PT / 2.0).abs() < 1e-3)).count()
}

/// The words drawn.
fn words(shapes: &[Shape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<Shape> {
    let mut shapes = vec![];
    for _ in 0..3 {
        *time += 0.05;
        shapes = frame(app, ctx, *time);
    }
    shapes
}

#[test]
fn each_selected_object_gets_its_entry_and_exit() {
    let mut app = app();
    let ctx = egui::Context::default();
    let accent = crate::theme::Tokens::get(&ctx).accent;
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.drawn());
    let ids: Vec<u64> = app.session.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    app.run("select.set", json!({"ids": [ids[0]]})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    assert_eq!((entries(&shapes, accent), exits(&shapes)), (1, 1));
    let said = words(&shapes);
    assert!(said.contains(&"Entry".to_string()) && said.contains(&"Exit".to_string()), "labelled for one: {said:?}");
    assert!(frame_only(&app), "the Selection tool shows the frame alone");
    app.run("select.all", json!({})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    assert_eq!((entries(&shapes, accent), exits(&shapes)), (3, 3));
    assert!(!words(&shapes).contains(&"Entry".to_string()), "no labels for several");
    // The ends are the scene's.
    let uid = app.session.active().unwrap().uid;
    let scene = app.embroidery.preview.shown(uid).unwrap().planned.scene();
    let doc = app.session.doc().unwrap().doc.clone();
    let selected = app.session.doc().unwrap().selection.objects.clone();
    assert_eq!(*app.embroidery.ends.of.as_ref().unwrap().1, scene.ends_of(&doc, &selected));
    // Another tool edits the art: its outlines and anchors show again.
    app.run("tool.select", json!({"tool": "directSelection"})).unwrap();
    assert!(!frame_only(&app));
}

#[test]
fn no_more_than_the_most_marked() {
    let mut app = app();
    let ctx = egui::Context::default();
    let accent = crate::theme::Tokens::get(&ctx).accent;
    for k in 0..MARKED_OBJECTS_MAX as u64 {
        app.run("shape.rectangle", json!({"x": 5 * (k % 10), "y": 150 + 5 * (k / 10), "width": 4, "height": 4})).unwrap();
    }
    app.run("select.all", json!({})).unwrap();
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    assert_eq!(entries(&shapes, accent), MARKED_OBJECTS_MAX);
}

/// A selected object moved while a slow design waits to be planned again is outlined dashed in its
/// layer's colour until its plan comes.
#[test]
fn a_changed_object_is_outlined_dashed_until_its_plan_comes() {
    let mut app = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let uid = app.session.active().unwrap().uid;
    let ids: Vec<u64> = app.session.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    app.run("select.set", json!({"ids": [ids[1]]})).unwrap();
    let before = settle(&mut app, &ctx, &mut time);
    assert!(pending(&app).is_empty());
    let color = layer_color(&app.session.doc().unwrap().doc, NodeId(ids[1]));
    let segments = |shapes: &[Shape]| shapes.iter().filter(|s| matches!(s, Shape::LineSegment { stroke, .. } if stroke.color == color)).count();
    app.embroidery.preview.set_plan_ms(uid, 1000.0);
    app.run("object.move", json!({"dx": 3, "dy": 0})).unwrap();
    time += 0.01;
    let shapes = frame(&mut app, &ctx, time);
    assert_eq!(pending(&app), [NodeId(ids[1])], "changed since the plan shown");
    assert!(segments(&shapes) > segments(&before) + 4, "dashed while stale");
    settle(&mut app, &ctx, &mut time);
    assert!(pending(&app).is_empty(), "planned");
    let shapes = frame(&mut app, &ctx, time + 0.05);
    assert_eq!(segments(&shapes), segments(&before), "no more dashes");
}

/// Selected objects that sew nothing are counted, and the canvas's notice says so.
#[test]
fn the_notice_counts_selected_objects_that_arent_sewn() {
    let mut app = app();
    let ctx = egui::Context::default();
    // Nothing selected, so that only the new art takes no fill.
    app.run("select.set", json!({"ids": []})).unwrap();
    app.run("paint.setFill", json!({"none": true})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 200, "width": 50, "height": 50})).unwrap();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    assert_eq!(not_sewn(&app, time), 1);
    let (said, _) = crate::embroidery::notice::text(&app, time).unwrap();
    assert_eq!(said, "1 selected object isn't sewn");
    app.run("select.all", json!({})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(not_sewn(&app, time), 1);
}
