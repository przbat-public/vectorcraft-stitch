//! Stitch editing in the app, headless with real input: a double-click on converted stitches with
//! the Embroidery Tool isolates them and shows the stitch view (the chip says so); a click selects a
//! needle point; Escape deselects, then ends editing (the project owner's order); the chip is read
//! once a change, never on an idle frame; the hint names stitch editing's gestures; Edit Stitches in
//! the Object tab starts editing; in the art view the chip offers the stitches.

use egui::accesskit::{Action, ActionRequest, TreeId};
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

use crate::embroidery::stitch_preview;
use crate::{Services, VectorcraftApp, canvas};

/// An app with a filled square converted to manual stitches, nothing selected, the Embroidery Tool
/// chosen → the app and the converted object (a group of runs).
fn app() -> (VectorcraftApp, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#c81e1e"})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [id]})).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("select.none", json!({})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, id)
}

/// One headless frame at `time` with `events`, as the app runs one (its shortcuts, the canvas and
/// what the fork draws over it, the Embroidery panel on the right) → its accessibility tree's named
/// nodes.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Vec<(egui::accesskit::NodeId, String)> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::shortcuts::handle(app, ui.ctx());
        egui::Panel::right("embroidery-test").exact_size(300.0).resizable(false).show(ui, |ui| crate::panels::embroidery::show(app, ui));
        canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    out.platform_output
        .accesskit_update
        .map(|t| t.nodes.iter().filter_map(|(id, n)| Some((*id, n.label()?.trim().to_string()))).collect())
        .unwrap_or_default()
}

/// Frames until the stitches are planned and drawn.
fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<(egui::accesskit::NodeId, String)> {
    let mut nodes = vec![];
    for _ in 0..4 {
        *time += 0.05;
        nodes = frame(app, ctx, *time, vec![]);
    }
    nodes
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

/// Where needle point `k` of the converted object's first run is on screen.
fn needle_on_screen(app: &VectorcraftApp, object: u64, k: usize) -> Pos2 {
    let st = app.session.active().unwrap();
    let group = st.doc.node(NodeId(object)).unwrap();
    let run = group.children().unwrap().first().unwrap();
    let p = run.path_data().unwrap().subpaths[0].anchors[k].p;
    canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(p)
}

/// A double-click on converted stitches isolates them and shows the stitches; the chip says what is
/// edited; a click selects one needle point; Escape deselects it (still editing); Escape again ends
/// editing (isolation left, as upstream's Escape leaves it).
#[test]
fn a_double_click_starts_editing_and_escape_deselects_then_leaves() {
    let (mut app, object) = app();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let uid = app.session.active().unwrap().uid;
    assert!(!app.session.stitch_view_of(uid).on, "the art view to begin with");
    let at = needle_on_screen(&app, object, 3);
    for e in [Event::PointerMoved(at), press(at, true), press(at, false), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().isolation, Some(NodeId(object)), "the converted object isolated");
    assert!(app.session.stitch_view_of(uid).on, "entering shows the stitches (the project owner's choice)");
    settle(&mut app, &ctx, &mut time);
    let (title, said) = super::lines(&mut app).expect("the chip");
    assert_eq!(title, "Editing stitches");
    assert!(said[0].ends_with("needle points"), "{said:?}");
    // A click on a needle point (drawn as the plan shown has it): selected alone. Wait out the double
    // click before it.
    time += 1.0;
    let at = needle_on_screen(&app, object, 5);
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let selected: usize = app.session.active().unwrap().selection.anchors.values().map(std::collections::BTreeSet::len).sum();
    assert_eq!(selected, 1, "{:?}", app.session.active().unwrap().selection);
    settle(&mut app, &ctx, &mut time);
    assert!(super::lines(&mut app).unwrap().1[0].contains("1 selected"), "{:?}", super::lines(&mut app));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
    assert!(app.session.active().unwrap().selection.is_empty(), "Escape deselects first");
    assert_eq!(app.session.active().unwrap().isolation, Some(NodeId(object)), "still editing");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
    assert_eq!(app.session.active().unwrap().isolation, None, "then Escape ends editing");
    assert!(super::lines(&mut app).is_none(), "no chip");
}

/// The chip asks the engine what is edited once a change: idle frames and a pointer resting or
/// moving over the canvas ask nothing.
#[test]
fn the_chip_is_read_once_a_change() {
    let (mut app, object) = app();
    app.run("object.isolate", json!({"id": object})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    assert!(super::editing(&mut app).is_some());
    let reads = app.embroidery.editing.reads;
    for k in 0..10 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(Pos2::new(300.0 + k as f32, 300.0))]);
    }
    assert_eq!(app.embroidery.editing.reads, reads, "idle and hover frames read nothing");
    app.run("embroidery.selectNeedlePoints", json!({"rect": [0, 0, 1000, 1000]})).unwrap();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert_eq!(app.embroidery.editing.reads, reads + 1, "a selection change reads once");
}

/// While the tool edits stitches the status bar's hint names stitch editing's gestures, each in the
/// interface's language; outside it the tool's own hint.
#[test]
fn the_hint_names_stitch_editings_gestures() {
    let (mut app, object) = app();
    assert_eq!(crate::embroidery::tool_bar::hint_id(&mut app, vectorcraft_tools::embroidery::ID), vectorcraft_tools::embroidery::ID);
    app.run("object.isolate", json!({"id": object})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    assert_eq!(crate::embroidery::tool_bar::hint_id(&mut app, vectorcraft_tools::embroidery::ID), crate::embroidery::tool_bar::EDITING_HINT_ID);
    let pl = crate::i18n::Lang::from_code("pl").unwrap();
    for &(text, bold) in crate::embroidery::tool_bar::EDITING_HINT {
        let text = if bold { text.rsplit_once('+').map_or(text, |(_, k)| k) } else { text };
        assert!(crate::i18n::tr(pl, text) != text || ["Esc", "←↑→↓"].contains(&text), "untranslated: {text:?}");
    }
}

/// Edit Stitches, in the Object tab of the selected converted object, isolates it, takes up the
/// Embroidery Tool and shows the stitches; pressed from the keyboard too.
#[test]
fn edit_stitches_from_the_object_tab() {
    let (mut app, object) = app();
    app.run("tool.select", json!({"tool": "selection"})).unwrap();
    app.run("select.set", json!({"ids": [object]})).unwrap();
    app.run("ui.embroideryPanel", json!({"tab": "object"})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    let mut time = 0.0;
    let nodes = settle(&mut app, &ctx, &mut time);
    let button = nodes.iter().find(|(_, n)| n == "Edit Stitches").map(|(id, _)| *id).unwrap_or_else(|| panic!("{nodes:?}"));
    time += 0.05;
    let click = Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_node: button, target_tree: TreeId::ROOT, data: None });
    frame(&mut app, &ctx, time, vec![click]);
    let uid = app.session.active().unwrap().uid;
    assert_eq!(app.session.active().unwrap().isolation, Some(NodeId(object)));
    assert_eq!(app.session.tool_id(), vectorcraft_tools::embroidery::ID);
    assert!(app.session.stitch_view_of(uid).on);
    settle(&mut app, &ctx, &mut time);
    assert!(super::editing(&mut app).is_some(), "editing its needle points");
}

/// Editing in the art view (the stitches turned off), the chip says needle points are edited on the
/// stitches and its button shows them.
#[test]
fn in_the_art_view_the_chip_offers_the_stitches() {
    let (mut app, object) = app();
    app.run("object.isolate", json!({"id": object})).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    let mut time = 0.0;
    let nodes = settle(&mut app, &ctx, &mut time);
    assert_eq!(super::lines(&mut app).map(|(_, l)| l), Some(vec!["Needle points are edited in the Stitches view.".to_string()]));
    let button = nodes.iter().find(|(_, n)| n == "Show Stitches").map(|(id, _)| *id).unwrap_or_else(|| panic!("{nodes:?}"));
    time += 0.05;
    let click = Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_node: button, target_tree: TreeId::ROOT, data: None });
    frame(&mut app, &ctx, time, vec![click]);
    let uid = app.session.active().unwrap().uid;
    assert!(app.session.stitch_view_of(uid).on);
}
