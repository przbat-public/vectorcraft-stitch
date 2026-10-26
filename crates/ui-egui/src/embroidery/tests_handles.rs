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

/// The drawn words.
fn words(shapes: &[Shape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Press the second square's entry and drag it by `by` in `steps` → the shapes of the last frame.
fn drag_second_entry(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, by: egui::Vec2, steps: usize) -> Vec<Shape> {
    let [_, b] = [0, 1];
    let entries: Vec<Pos2> = on_screen(app).iter().filter(|(k, _)| *k == Kind::Entry).map(|(_, at)| *at).collect();
    let at = entries[b];
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::PointerMoved(at), button(at, true)]);
    let mut shapes = vec![];
    for k in 1..=steps {
        *time += 0.02;
        shapes = frame(app, ctx, *time, vec![Event::PointerMoved(at + by * (k as f32 / steps as f32))]);
    }
    shapes
}

/// While a point is dragged, a ghost joins it to where the needle comes from (the first square's
/// end, along the thread), saying how long the jump is.
#[test]
fn a_drag_shows_the_jump_from_the_part_before() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let shapes = drag_second_entry(&mut app, &ctx, &mut time, egui::vec2(10.0, 20.0), 3);
    let said = words(&shapes);
    // The squares lie 21 mm apart, past the 10 mm trim length: the jump is cut.
    assert!(said.iter().any(|w| w.starts_with("jump ") && w.ends_with(" mm, cut")), "{said:?}");
    assert!(shapes.iter().filter(|s| matches!(s, Shape::LineSegment { .. })).count() > 4, "dashes");
}

/// On a design slow to plan, the status bar says when the stitches catch up while a point is
/// dragged, and a pause in the drag plans (on the desktop: a runner off the frame, as the planning
/// thread is), the button still held.
#[test]
fn a_pause_in_a_drag_plans_a_design_slow_to_plan() {
    let (mut app, _) = app();
    app.embroidery.preview = stitch_preview::StitchPreview::with_runner(Box::new(stitch_preview::OneStepAPoll::default()));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let uid = app.session.active().unwrap().uid;
    app.embroidery.preview.log.pinned_ms = Some(stitch_preview::LIVE_PLAN_MS * 4.0);
    app.embroidery.preview.set_plan_ms(uid, stitch_preview::LIVE_PLAN_MS * 4.0);
    assert!(!app.embroidery.preview.live(uid));
    drag_second_entry(&mut app, &ctx, &mut time, egui::vec2(15.0, 5.0), 3);
    assert_eq!(app.ui.status, "Stitches update when you pause or let go.");
    let sent = app.embroidery.preview.log.sent.len();
    // Moving on: nothing planned.
    time += 0.02;
    frame(&mut app, &ctx, time, vec![]);
    assert_eq!(app.embroidery.preview.log.sent.len(), sent, "on the move");
    // Holding still past the pause: planned.
    time += stitch_preview::PAUSE_TO_PLAN_SECS + 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(app.embroidery.preview.log.sent.len() > sent, "planned on the pause");
    assert!(app.session.tool_busy(), "the button still held");
}

/// A double-click on an entry opens the Entry Point dialog on it; Enter sets the point there (taken
/// onto the outline), one undo step; Escape closes it changing nothing.
#[test]
fn a_double_click_opens_the_point_dialog() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let b = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), b(true), b(false), b(true), b(false)]);
    time += 0.05;
    let shapes = frame(&mut app, &ctx, time, vec![]);
    assert!(app.embroidery.point_dialog.is_some(), "open");
    assert!(words(&shapes).iter().any(|w| w == "Entry Point"), "{:?}", words(&shapes));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(egui::Key::Escape)]);
    assert!(app.embroidery.point_dialog.is_none(), "Escape closes it");
    let undo = app.session.doc().unwrap().history.undo.len();
    // A second double-click, a second later (egui counts clicks within its double-click time).
    time += 1.0;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), b(true), b(false), b(true), b(false)]);
    assert!(app.embroidery.point_dialog.is_some(), "open again");
    // The dialog opens on an Auto point: Auto is ticked; untick it, then Enter.
    app.embroidery.point_dialog = app.embroidery.point_dialog.take().map(|d| crate::embroidery::point_dialog::PointDialog::set_auto(d, false));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(egui::Key::Enter)]);
    assert!(app.embroidery.point_dialog.is_none());
    let entry = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
    assert!(entry.is_array(), "set: {entry}");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
}
