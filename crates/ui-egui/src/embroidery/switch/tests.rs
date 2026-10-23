//! The Art | Stitches switch: a click on a segment sets the stitch view; screen readers get two
//! radio buttons, the selected one checked; disabled while a pattern is edited; hidden in
//! Presentation Mode.

use egui::{Event, PointerButton, Pos2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::{Services, canvas};

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 100, "height": 100})).unwrap();
    app
}

/// One headless frame with `events`, as the app draws the canvas and the controls over it.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
    });
    out.textures_delta.clear();
}

/// Click the middle of `id`'s widget, as drawn in the last frame.
fn click(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, id: Id) {
    let at = ctx.read_response(id).map(|r| r.rect.center()).unwrap_or_else(|| panic!("{id:?} isn't drawn"));
    let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(app, ctx, time, vec![Event::PointerMoved(at), press(true)]);
    frame(app, ctx, time + 0.05, vec![press(false)]);
}

fn info(ctx: &egui::Context, id: Id) -> Option<egui::Response> {
    ctx.read_response(id)
}

#[test]
fn a_click_on_a_segment_sets_the_stitch_view() {
    let mut app = app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, 0.0, vec![]);
    frame(&mut app, &ctx, 0.1, vec![]);
    let (art, stitches) = (group_id().with(false), group_id().with(true));
    assert!(info(&ctx, art).is_some() && info(&ctx, stitches).is_some(), "both segments drawn");
    click(&mut app, &ctx, 0.2, stitches);
    assert!(app.session.stitch_view().on, "Stitches");
    frame(&mut app, &ctx, 0.4, vec![]);
    click(&mut app, &ctx, 0.5, art);
    assert!(!app.session.stitch_view().on, "Art");
    // The canvas under the switch didn't take the clicks: the selection is as it was.
    assert_eq!(app.session.doc().unwrap().selection.objects.len(), 1);
}

#[test]
fn screen_readers_get_two_radio_buttons() {
    let mut app = app();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let raw = |time| egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0))),
        ..Default::default()
    };
    let mut tree = None;
    for time in [0.0, 0.1] {
        let mut out = ctx.run_ui(raw(time), |ui| {
            canvas::show(&mut app, ui);
            crate::embroidery::canvas_controls(&mut app, ui);
        });
        out.textures_delta.clear();
        tree = out.platform_output.accesskit_update;
    }
    let tree = tree.expect("an accessibility tree");
    let radios: Vec<_> = tree.nodes.iter().filter(|(_, n)| n.role() == egui::accesskit::Role::RadioButton).collect();
    assert_eq!(radios.len(), 2);
    let checked: Vec<Option<egui::accesskit::Toggled>> = radios.iter().map(|(_, n)| n.toggled()).collect();
    assert!(checked.contains(&Some(egui::accesskit::Toggled::True)) && checked.contains(&Some(egui::accesskit::Toggled::False)));
    let labels: Vec<Option<String>> = radios.iter().map(|(_, n)| n.label().map(str::to_string)).collect();
    assert!(labels.contains(&Some("Art".into())) && labels.contains(&Some("Stitches".into())), "{labels:?}");
    // In their group: a screen reader says "View" and that it holds two.
    let (_, group) = tree.nodes.iter().find(|(_, n)| n.role() == egui::accesskit::Role::RadioGroup).expect("the group");
    assert_eq!(group.label(), Some("View"));
    assert!(radios.iter().all(|(id, _)| group.children().contains(id)), "the radios are the group's");
}

#[test]
fn hidden_in_presentation_mode_and_disabled_while_a_pattern_is_edited() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("select.all", json!({})).unwrap();
    app.run("object.pattern.make", json!({})).unwrap();
    frame(&mut app, &ctx, 0.0, vec![]);
    frame(&mut app, &ctx, 0.1, vec![]);
    let stitches = group_id().with(true);
    click(&mut app, &ctx, 0.2, stitches);
    assert!(!app.session.stitch_view().on, "disabled");
    assert!(app.session.active().unwrap().doc.pattern_edit.is_some(), "the canvas didn't take the click");
    app.run("object.pattern.cancel", json!({})).unwrap();
    app.ui.screen_mode = 3;
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, 0.2, vec![]);
    assert!(info(&ctx, stitches).is_none(), "hidden");
}
