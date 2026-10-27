//! Stitch editing on the canvas, headless with real input: the selection layer drawn without
//! drawing the stitches again, a drag with the mouse that moves the needle points (checked before
//! Escape puts them back) and one undo step when let go, the live check's words, the edit look's
//! circles from 250 % and its marks on a stitch that breaks the limits, the current needle point's
//! two-tone ring, the Contextual Task Bar's stitch actions, and frames that build nothing while
//! nothing changes.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;
use vectorcraft_geom::Point;

use crate::embroidery::stitch_preview;
use crate::{Services, VectorcraftApp, canvas};

/// An app with a filled square converted to manual stitches, isolated, in the stitch view, the
/// Embroidery Tool chosen: editing its stitches → the app and the converted object.
fn editing() -> (VectorcraftApp, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#c81e1e"})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [id]})).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, id)
}

/// One headless frame at `time` with `events`, as the app runs one → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::shortcuts::handle(app, ui.ctx());
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

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<Shape> {
    let mut shapes = vec![];
    for _ in 0..4 {
        *time += 0.05;
        shapes = frame(app, ctx, *time, vec![]);
    }
    shapes
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

/// The converted object's first run's anchor `k`, in the document.
fn needle(app: &VectorcraftApp, object: u64, k: usize) -> Point {
    let st = app.session.active().unwrap();
    let group = st.doc.node(NodeId(object)).unwrap();
    group.children().unwrap().first().unwrap().path_data().unwrap().subpaths[0].anchors[k].p
}

fn on_screen(app: &VectorcraftApp, p: Point) -> Pos2 {
    canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(p)
}

/// A click on a needle point selects it; the selection layer draws it in its layer's colour, built
/// once for the change, with no frame of stitches drawn again; idle frames build nothing.
#[test]
fn a_selection_change_draws_no_stitches_again() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let (uploads, builds) = (app.embroidery.layer.uploads, app.embroidery.selection.builds);
    let at = on_screen(&app, needle(&app, object, 4));
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let shapes = settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.layer.uploads, uploads, "a selection change draws no stitches again");
    assert_eq!(app.embroidery.selection.builds, builds + 1, "built once for the change");
    let ink = crate::embroidery::marks::readable(egui::Color32::from_rgb(0x4f, 0x80, 0xff), &[super::light()], false);
    let filled = shapes.iter().any(|s| matches!(s, Shape::Circle(c) if c.center.distance(at) < 0.5 && c.fill == ink));
    assert!(filled, "the selected needle point filled in its layer's colour");
    let builds = app.embroidery.selection.builds;
    for _ in 0..10 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(30.0, 30.0))]);
    }
    assert_eq!(app.embroidery.selection.builds, builds, "idle and hover frames build nothing");
}

/// A drag of a selected needle point with the mouse moves it while it drags (checked before
/// Escape), Escape puts the document back as it was at the press, and a drag let go is one undo
/// step; the live check says the move while it drags.
#[test]
fn a_drag_moves_needle_points_and_escape_puts_them_back() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let from = needle(&app, object, 4);
    let at = on_screen(&app, from);
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let before = app.session.active().unwrap().doc.clone();
    let steps = app.session.active().unwrap().history.undo.len();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at, true)]);
    for k in 1..=6 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(6.0 * k as f32, 0.0))]);
    }
    assert!(needle(&app, object, 4).x > from.x + 1.0, "the drag moved the needle point before Escape");
    let lines = super::bubble(&mut app).expect("the live check");
    assert!(lines.iter().any(|(_, w)| w.starts_with("Moved X")), "{lines:?}");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
    assert!(app.session.active().unwrap().doc == before, "Escape puts the document back as it was at the press");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at + egui::vec2(36.0, 0.0), false)]);
    // Again, let go this time: one undo step.
    for e in [
        press(at, true),
        Event::PointerMoved(at + egui::vec2(10.0, 0.0)),
        Event::PointerMoved(at + egui::vec2(20.0, 0.0)),
        press(at + egui::vec2(20.0, 0.0), false),
    ] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().history.undo.len(), steps + 1);
    assert!(needle(&app, object, 4).x > from.x + 1.0);
}

/// The live check: the needle points selected, and their stitches' shortest and longest with a
/// passed mark; a stitch made longer than the limit warns.
#[test]
fn the_live_check_judges_the_stitches_of_the_selection() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}, {"id": run.0, "index": 5}]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let lines = super::bubble(&mut app).unwrap();
    assert!(lines[0].1.starts_with("2 needle points"), "{lines:?}");
    assert!(lines.iter().any(|(m, w)| *m == Some(super::Mark::Passed) && w.starts_with("Shortest:")), "{lines:?}");
    app.run("embroidery.moveNeedlePoints", json!({"dx": 40})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let lines = super::bubble(&mut app).unwrap();
    assert!(lines.iter().any(|(m, w)| *m == Some(super::Mark::Warning) && w.contains("too long")), "{lines:?}");
}

/// The edit look: needle points as circles from 250 % and not under it; a stitch longer than the
/// limit marked, and none before.
#[test]
fn the_edit_look_shows_circles_from_250_percent_and_marks_rule_breakers() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    let centre = needle(&app, object, 4);
    app.run("view.setZoom", json!({"zoom": 200, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.layer.stats().circles, 0, "no circles under 250 %");
    assert_eq!(app.embroidery.layer.stats().flagged, 0, "nothing breaks the limits after converting");
    app.run("view.setZoom", json!({"zoom": 400, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.stats().circles > 0, "circles from 250 %");
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}], "dx": 30})).unwrap();
    app.run("view.setZoom", json!({"zoom": 100, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.stats().flagged > 0, "a stitch longer than the limit is marked");
}

/// `]` makes a needle point the current one: a two-tone ring round it (dark, light, its colour).
#[test]
fn the_current_needle_point_has_a_two_tone_ring() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    time += 0.05;
    // Typed, as the shortcuts read brackets (layouts where a bracket takes Option or AltGr count).
    frame(&mut app, &ctx, time, vec![Event::Text("]".into())]);
    let shapes = settle(&mut app, &ctx, &mut time);
    let at = on_screen(&app, needle(&app, object, 0));
    let rings: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Circle(c) if c.fill == egui::Color32::TRANSPARENT && c.center.distance(at) < 0.5 && c.radius > super::SELECTED_RADIUS_PT => {
                Some(c.stroke.color)
            }
            _ => None,
        })
        .collect();
    assert!(rings.contains(&super::dark()) && rings.contains(&super::light()), "a two-tone ring: {rings:?}");
}

/// With needle points selected, the Contextual Task Bar offers the stitch actions in place of
/// upstream's; Delete there deletes them, one undo step.
#[test]
fn the_task_bar_offers_the_stitch_actions() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}]})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    let bar = crate::canvas::task_bar_rect(&ctx).expect("the task bar shows");
    let mut labels: Vec<(String, egui::Rect)> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) if bar.contains(t.visual_bounding_rect().center()) => Some((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => None,
        })
        .collect();
    labels.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    let words: Vec<&str> = labels.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(words, ["Insert", "Delete", "Split Run", "Join Runs", "Trim", "Stop", "Split Long Stitches"], "{words:?}");
    let count = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors.len();
    let steps = app.session.active().unwrap().history.undo.len();
    let delete = labels.iter().find(|(w, _)| w == "Delete").unwrap().1.center();
    for e in [Event::PointerMoved(delete), press(delete, true), press(delete, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().history.undo.len(), steps + 1);
    let now = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors.len();
    assert_eq!(now, count - 1, "the needle point deleted");
}
