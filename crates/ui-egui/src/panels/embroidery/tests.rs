//! The Embroidery panel: where Window › Embroidery puts it, `ui.embroideryPanel`, the open tab read
//! leniently, and its states drawn headless.

use egui::Shape;
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;

fn blank_app() -> VectorcraftApp {
    VectorcraftApp::new(Session::new(), Services::default())
}

fn with_art() -> VectorcraftApp {
    let mut app = blank_app();
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 100, "height": 100})).unwrap();
    app
}

/// The panel drawn alone in a headless frame at `time`, then the preview's scheduling → the
/// words drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) -> Vec<String> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 600.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        show(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut words = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) => words.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    words
}

fn has(words: &[String], what: &str) -> bool {
    words.iter().any(|w| w.contains(what))
}

/// The first Window › Embroidery docks it as the last tab beside Properties, Layers and Libraries,
/// turning the dock into the shared layout; the item then closes it, and the next opens it where it
/// was.
#[test]
fn window_embroidery_docks_it_beside_the_three_tabs() {
    let mut app = blank_app();
    assert!(app.ui.docking.is_none(), "the classic dock to begin with");
    let item = json!({"panel": ID, "toggle": true});
    app.run("window.panel", item.clone()).unwrap();
    let layout = app.ui.docking.as_ref().expect("the shared layout");
    let root = craft_ui::docking::Layout { root: layout.root.clone(), floating: Vec::new() };
    let panels: Vec<String> = root.panels().into_iter().cloned().collect();
    assert_eq!(panels, ["properties", "layers", "libraries", ID], "the last tab of the group");
    let before = app.ui.docking.clone();
    assert_eq!(crate::menus::checked(&app, "window.panel", &item), Some(true));
    app.run("window.panel", item.clone()).unwrap();
    assert_eq!(crate::menus::checked(&app, "window.panel", &item), Some(false), "the item closes it");
    app.run("window.panel", item.clone()).unwrap();
    assert_eq!(app.ui.docking, before, "back where it was");
    assert_eq!(crate::menus::checked(&app, "window.panel", &item), Some(true));
}

#[test]
fn ui_embroidery_panel_shows_it_at_a_tab() {
    let mut app = blank_app();
    assert_eq!(app.run("ui.embroideryPanel", json!({"tab": "check"})).unwrap(), json!({"tab": "check"}));
    assert_eq!(app.ui.embroidery_panel.tab, Tab::Check);
    assert!(app.ui.docking.as_ref().is_some_and(|l| l.contains(&ID.to_string())));
    assert_eq!(app.run("ui.embroideryPanel", json!({})).unwrap(), json!({"tab": "check"}), "the tab stays");
    assert!(app.run("ui.embroideryPanel", json!({"tab": "stitches"})).is_err(), "not shown yet");
    assert!(app.run("ui.embroideryPanel", json!({"tab": 1})).is_err());
    assert!(app.run("ui.embroideryPanel", json!({"tabs": "order"})).is_err());
    assert_eq!(app.ui.embroidery_panel.tab, Tab::Check);
}

/// A tab this version doesn't show, or anything else, reads as the first; the interface's state
/// still loads.
#[test]
fn the_open_tab_is_read_leniently() {
    for (saved, tab) in
        [(json!({"tab": "check"}), Tab::Check), (json!({"tab": "object"}), Tab::Order), (json!({"tab": 7}), Tab::Order), (json!({}), Tab::Order)]
    {
        let s: Settings = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(s.tab, tab, "{saved}");
    }
    let mut ui = serde_json::to_value(crate::state::UiState::default()).unwrap();
    ui["embroidery_panel"] = json!({"tab": "stitches", "zoom": 3});
    assert!(serde_json::from_value::<crate::state::UiState>(ui).is_ok());
}

#[test]
fn its_states() {
    let ctx = egui::Context::default();
    // No document.
    let mut app = blank_app();
    let words = frame(&mut app, &ctx, 0.0);
    assert!(has(&words, "No document"), "{words:?}");
    // Not planned yet: blank (the strip only), then planned and ready.
    let mut app = with_art();
    let words = frame(&mut app, &ctx, 1.0);
    assert!(has(&words, "Sewing Order") && has(&words, "Check"), "{words:?}");
    assert!(!has(&words, "Planning"), "no busy mark at once: {words:?}");
    frame(&mut app, &ctx, 1.05);
    let words = frame(&mut app, &ctx, 1.1);
    // A square filled and stroked: two blocks.
    assert!(has(&words, "2 thread blocks · "), "{words:?}");
    assert!(words.iter().any(|w| w.ends_with(" stitches")), "{words:?}");
    // Updating: an edit of a slow design shows the last result, and says so after a while.
    let uid = app.session.active().unwrap().uid;
    app.embroidery.preview.set_plan_ms(uid, 1000.0);
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    let words = frame(&mut app, &ctx, 2.0);
    assert!(has(&words, "2 thread blocks · ") && !has(&words, "Updating"), "{words:?}");
    let words = frame(&mut app, &ctx, 2.15);
    assert!(has(&words, "Updating…"), "{words:?}");
    // Nothing to sew.
    let mut app = blank_app();
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    for t in [0.0, 0.1, 0.2] {
        frame(&mut app, &ctx, 3.0 + t);
    }
    let words = frame(&mut app, &ctx, 3.3);
    assert!(has(&words, "Nothing to sew") && has(&words, "Paths with a fill or a stroke are sewn"), "{words:?}");
    // Can't be planned: three metres a side.
    let mut app = with_art();
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    for t in [0.0, 0.1, 0.2] {
        frame(&mut app, &ctx, 4.0 + t);
    }
    let words = frame(&mut app, &ctx, 4.3);
    assert!(has(&words, "Can't plan the stitches:") && has(&words, "Change the design"), "{words:?}");
}

/// Screen readers find the tabs as tabs, the open one selected; a click opens the other.
#[test]
fn the_tabs_are_tabs() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = with_art();
    let raw = |time: f64| egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 600.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw(0.0), |ui| show(&mut app, ui));
    out.textures_delta.clear();
    let mut out = ctx.run_ui(raw(0.1), |ui| show(&mut app, ui));
    out.textures_delta.clear();
    let tree = out.platform_output.accesskit_update.expect("an accessibility tree");
    let tabs: Vec<_> = tree
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == egui::accesskit::Role::Tab)
        .map(|(_, n)| (n.label().map(str::to_string), n.is_selected()))
        .collect();
    assert_eq!(tabs.len(), 2, "{tabs:?}");
    assert!(tabs.contains(&(Some("Sewing Order".into()), Some(true))) && tabs.contains(&(Some("Check".into()), Some(false))), "{tabs:?}");
}
