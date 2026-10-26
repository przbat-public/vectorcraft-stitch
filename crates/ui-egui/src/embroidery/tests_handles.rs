//! The embroidery tool's handles on the canvas, headless with real pointer events: drawn where the
//! tool catches them, in the art view too (a plan asked for), in place of the passive markers in the
//! stitch view; a drag that follows the pointer and Escape that takes it back; nothing with Hide
//! Edges or in Presentation Mode; found again only when something changed.

use egui::{Color32, Event, Modifiers, PointerButton, Pos2, Shape};
use serde_json::json;
use vectorcraft_engine::Session;
use vectorcraft_tools::embroidery::Kind;

use crate::embroidery::stitch_preview;
use crate::{Services, VectorcraftApp, canvas};

/// An app with two filled squares, both selected, and the embroidery tool chosen.
fn app() -> (VectorcraftApp, [u64; 2]) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let mut ids = [0; 2];
    for (k, x) in [40, 170].into_iter().enumerate() {
        ids[k] = app.run("shape.rectangle", json!({"x": x, "y": 60, "width": 80, "height": 80})).unwrap()["id"].as_u64().unwrap();
    }
    app.run("select.set", json!({"ids": ids})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, ids)
}

/// One headless frame at `time` with `events` → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0))),
        events,
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
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            s => all.push(s),
        }
    }
    all
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<Shape> {
    let mut shapes = vec![];
    for _ in 0..4 {
        *time += 0.05;
        shapes = frame(app, ctx, *time, vec![]);
    }
    shapes
}

/// Where each handle the tool catches is on screen, by kind.
fn on_screen(app: &mut VectorcraftApp) -> Vec<(Kind, Pos2)> {
    let view = app.view_info();
    let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    app.session.embroidery_handles(view).0.iter().map(|h| (h.id.kind, xf.to_screen(h.at))).collect()
}

/// The diamonds drawn (an entry's shape, its face in `face`), by their centre.
fn diamonds(shapes: &[Shape], face: Color32) -> Vec<Pos2> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.closed && p.points.len() == 4 && p.fill == face => {
                Some(Pos2::new((p.points[1].x + p.points[3].x) / 2.0, (p.points[0].y + p.points[2].y) / 2.0))
            }
            _ => None,
        })
        .collect()
}

fn light() -> Color32 {
    let [r, g, b] = vectorcraft_engine::cmd::embroidery::scene::MARK_LIGHT;
    Color32::from_rgb(r, g, b)
}

/// In the art view the tool asks for a plan, and each selected square's Auto entry is drawn hollow
/// where the tool catches it (two squares, two entries, two exits).
#[test]
fn handles_are_drawn_where_the_tool_catches_them_in_the_art_view() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    assert!(!app.embroidery.layer.drawn(), "the art view");
    let caught = on_screen(&mut app);
    assert_eq!(caught.iter().filter(|(k, _)| *k == Kind::Entry).count(), 2, "{caught:?}");
    let drawn = diamonds(&shapes, light());
    for (kind, at) in &caught {
        if *kind == Kind::Entry {
            assert!(drawn.iter().any(|d| d.distance(*at) < 0.5), "an entry at {at:?}, drawn {drawn:?}");
        }
    }
}

/// In the stitch view the tool's handles take the passive markers' place: no marker drawn beside
/// them.
#[test]
fn the_passive_markers_give_way_in_the_stitch_view() {
    let (mut app, _) = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    let accent = crate::theme::Tokens::get(&ctx).accent;
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.drawn());
    // The passive markers' entries are filled in the accent; the tool's Auto ones are hollow.
    assert_eq!(diamonds(&shapes, accent).len(), 0);
    assert_eq!(diamonds(&shapes, light()).len(), 2);
}

/// A press on an entry and a drag with the mouse: the entry follows the pointer in the same frame;
/// Escape takes the drag back (the point unset, no undo step).
#[test]
fn a_drag_follows_the_pointer_and_escape_takes_it_back() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let undo = app.session.doc().unwrap().history.undo.len();
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(at, true)]);
    let to = at + egui::vec2(30.0, 12.0);
    let mut shapes = vec![];
    for k in 1..=3 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + (to - at) * (k as f32 / 3.0))]);
    }
    assert!(app.session.tool_options()["drag"].is_object(), "dragging: {}", app.session.tool_options());
    assert!(
        diamonds(&shapes, light()).iter().chain(diamonds(&shapes, crate::theme::Tokens::get(&ctx).accent).iter()).any(|d| d.distance(to) < 1.0),
        "the entry drawn under the pointer"
    );
    time += 0.05;
    frame(
        &mut app,
        &ctx,
        time,
        vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    );
    time += 0.05;
    frame(&mut app, &ctx, time, vec![button(to, false)]);
    let entry = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
    assert_eq!((entry, app.session.doc().unwrap().history.undo.len()), (json!(null), undo), "taken back");
}

/// Hide Edges and Presentation Mode draw no handles.
#[test]
fn no_handles_with_hide_edges_or_in_presentation_mode() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 2);
    app.ui.view.edges = false;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 0, "Hide Edges");
    app.ui.view.edges = true;
    app.ui.screen_mode = 3;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 0, "Presentation Mode");
}

/// The handles are found again when the selection or the plan changes, not on idle frames.
#[test]
fn the_handles_are_found_again_only_when_something_changed() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let found = app.embroidery.handles.found;
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.handles.found, found, "idle frames find nothing again");
    app.run("select.set", json!({"ids": [a]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.handles.found > found);
    assert_eq!(diamonds(&frame(&mut app, &ctx, time + 0.05, vec![]), light()).len(), 1);
}
