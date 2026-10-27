//! The Embroidery panel: where Window › Embroidery puts it, `ui.embroideryPanel`, the open tab read
//! leniently, and its states drawn headless.

use egui::Shape;
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;

/// An app showing the panel's Sewing Order tab, whose states these tests draw (the Object tab draws
/// in all of them: `object/tests.rs`).
fn blank_app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.ui.embroidery_panel.tab = Tab::Order;
    app
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
    frame_and_tree(app, ctx, time).0
}

/// [`frame`], with the screen-reader tree when screen readers are on.
fn frame_and_tree(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) -> (Vec<String>, Option<egui::accesskit::TreeUpdate>) {
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
    (words, out.platform_output.accesskit_update)
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
    let none = Value::Null;
    assert_eq!(app.run("ui.embroideryPanel", json!({"tab": "check"})).unwrap(), json!({"tab": "check", "part": "fill", "format": none}));
    assert_eq!(app.ui.embroidery_panel.tab, Tab::Check);
    assert!(app.ui.docking.as_ref().is_some_and(|l| l.contains(&ID.to_string())));
    assert_eq!(app.run("ui.embroideryPanel", json!({})).unwrap(), json!({"tab": "check", "part": "fill", "format": none}), "the tab stays");
    assert!(app.run("ui.embroideryPanel", json!({"tab": "stitches"})).is_err(), "not shown yet");
    assert!(app.run("ui.embroideryPanel", json!({"tab": 1})).is_err());
    assert!(app.run("ui.embroideryPanel", json!({"tabs": "order"})).is_err());
    assert_eq!(app.ui.embroidery_panel.tab, Tab::Check);
    // The Object tab, at its stroke; a part refused changes neither.
    let r = app.run("ui.embroideryPanel", json!({"tab": "object", "part": "stroke"})).unwrap();
    assert_eq!((&r["tab"], &r["part"]), (&json!("object"), &json!("stroke")));
    assert!(app.run("ui.embroideryPanel", json!({"tab": "check", "part": "both"})).is_err());
    assert_eq!((app.ui.embroidery_panel.tab, app.ui.embroidery_panel.part), (Tab::Object, object::Part::Stroke));
    // The format the check is for: no document, none to set.
    assert!(app.run("ui.embroideryPanel", json!({"format": "dst"})).is_err());
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    let uid = app.session.active().unwrap().uid;
    assert_eq!(app.run("ui.embroideryPanel", json!({"format": "dst"})).unwrap()["format"], "dst");
    assert_eq!(app.embroidery.check.format.get(&uid), Some(&"dst"));
    for bad in [json!({"format": "zzz"}), json!({"format": 5})] {
        assert!(app.run("ui.embroideryPanel", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(app.embroidery.check.format.get(&uid), Some(&"dst"), "a refused format changes nothing");
    assert_eq!(app.run("ui.embroideryPanel", json!({"format": null})).unwrap()["format"], none, "back to the palette's");
    assert!(!app.embroidery.check.format.contains_key(&uid));
}

/// A tab this version doesn't show, or anything else, reads as the first, the Object tab, and a
/// part it doesn't know as the fill; the interface's state still loads.
#[test]
fn the_open_tab_is_read_leniently() {
    use object::Part;
    for (saved, tab, part) in [
        (json!({"tab": "check"}), Tab::Check, Part::Fill),
        (json!({"tab": "object", "part": "stroke"}), Tab::Object, Part::Stroke),
        (json!({"tab": "stitches", "part": 3}), Tab::Object, Part::Fill),
        (json!({"tab": 7}), Tab::Object, Part::Fill),
        (json!({}), Tab::Object, Part::Fill),
    ] {
        let s: Settings = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!((s.tab, s.part), (tab, part), "{saved}");
    }
    // Whatever is saved for the panel, the rest of the interface's state loads.
    let mut ui = serde_json::to_value(crate::state::UiState::default()).unwrap();
    ui["recent_files"] = json!(["kept.svg"]);
    for saved in [json!({"tab": "stitches", "zoom": 3}), json!(null), json!("check"), json!(5), json!([1, 2]), json!({"tab": "check"})] {
        ui["embroidery_panel"] = saved.clone();
        let read = serde_json::from_value::<crate::state::UiState>(ui.clone()).unwrap_or_else(|e| panic!("{saved}: {e}"));
        assert_eq!(read.recent_files, ["kept.svg"], "{saved}");
        let tab = if saved == json!({"tab": "check"}) { Tab::Check } else { Tab::Object };
        assert_eq!(read.embroidery_panel.tab, tab, "{saved}");
    }
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

/// A design that takes long to plan: blank at first, then "Planning stitches…" with how far the
/// plan got; once planned, an edit that can't be planned says why over the last stitches, which
/// stay shown.
#[test]
fn planning_with_its_progress_then_a_failure_over_the_last_stitches() {
    use crate::embroidery::stitch_preview::{Held, StitchPreview};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = with_art();
    let held = Held::default();
    app.embroidery.preview = StitchPreview::with_runner(Box::new(held.clone()));
    // The big square's plan sent in its first frame, before the release that runs it.
    app.embroidery.preview.pin_light();
    let words = frame(&mut app, &ctx, 0.0);
    assert!(!has(&words, "Planning"), "no busy mark at once: {words:?}");
    held.progress(2, 5);
    let (words, tree) = frame_and_tree(&mut app, &ctx, 0.2);
    assert!(has(&words, "Planning stitches…"), "{words:?}");
    let tree = tree.expect("an accessibility tree");
    let bar = tree.nodes.iter().find(|(_, n)| n.role() == egui::accesskit::Role::ProgressIndicator).map(|(_, n)| n.numeric_value());
    assert_eq!(bar, Some(Some(40.0)), "two steps of five");
    held.release();
    frame(&mut app, &ctx, 0.3);
    let words = frame(&mut app, &ctx, 0.4);
    assert!(has(&words, "2 thread blocks · ") && !has(&words, "Planning"), "{words:?}");
    // Three metres a side: it can't be planned; the last stitches stay, with why.
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    frame(&mut app, &ctx, 1.0);
    held.release();
    frame(&mut app, &ctx, 1.1);
    let words = frame(&mut app, &ctx, 1.2);
    assert!(has(&words, "Can't plan the stitches:") && has(&words, "Change the design"), "{words:?}");
    assert!(has(&words, "2 thread blocks · "), "the last stitches stay: {words:?}");
    assert!(!has(&words, "Updating"), "nothing is planned meanwhile, from the first frame that says why: {words:?}");
}

/// The ≡ menu drawn alone at `time` with `events` → its items as a screen reader has them: the
/// node, the words, and whether the item is on.
/// Frames until Nearest Connection's search under way is applied (a second at most).
fn finish_nearest(app: &mut VectorcraftApp, ctx: &egui::Context) {
    for _ in 0..1000 {
        if !crate::embroidery::nearest::running(app) {
            return;
        }
        crate::embroidery::nearest::poll(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("Nearest Connection still running");
}

fn menu_items(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> Vec<(egui::accesskit::NodeId, String, bool)> {
    let raw = egui::RawInput { time: Some(time), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| menu(app, ui));
    out.textures_delta.clear();
    let tree = out.platform_output.accesskit_update.expect("an accessibility tree");
    tree.nodes
        .iter()
        .filter(|(_, n)| n.role() == egui::accesskit::Role::Button)
        .map(|(id, n)| (*id, n.label().unwrap_or_default().trim().to_string(), !n.is_disabled()))
        .collect()
}

/// The ≡ menu: the Sew items on while something selected is sewn and the list's plan is of the
/// document as it is, and they move it as the panel's steps do; Collapse All folds every block and
/// Expand All unfolds them; Next and Previous Finding on only while a finding is shown.
#[test]
fn the_panels_menu() {
    use egui::accesskit::{Action, ActionRequest, TreeId};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = blank_app();
    // Each change planned within the frames `panel` draws after it.
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let mut ids = vec![];
    for (x, color) in [(20, "#cc2222"), (40, "#2233cc"), (60, "#22aa33")] {
        app.run("select.set", json!({"ids": []})).unwrap();
        app.run("paint.setFill", json!({ "color": color })).unwrap();
        ids.push(app.run("shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40})).unwrap()["id"].as_u64().unwrap());
    }
    app.run("select.set", json!({"ids": []})).unwrap();
    let mut time = 0.0;
    let mut panel = |app: &mut VectorcraftApp| {
        for _ in 0..3 {
            time += 0.05;
            frame(app, &ctx, time);
        }
        time
    };
    let t = panel(&mut app);
    let items = menu_items(&mut app, &ctx, t, vec![]);
    let item = |items: &[(egui::accesskit::NodeId, String, bool)], words: &str| {
        items.iter().find(|(_, w, _)| w == words).map(|(id, _, on)| (*id, *on)).unwrap_or_else(|| panic!("no {words}: {items:?}"))
    };
    for words in ["Sew Earlier", "Sew Later", "Sew First", "Sew Last", "Next Finding", "Previous Finding"] {
        assert!(!item(&items, words).1, "{words} off with nothing selected and no finding shown");
    }
    assert!(item(&items, "Collapse All").1 && item(&items, "Expand All").1);
    let click =
        |id| egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_node: id, target_tree: TreeId::ROOT, data: None });
    // Collapse All folds every block; Expand All unfolds them.
    let uid = app.session.active().unwrap().uid;
    menu_items(&mut app, &ctx, t + 0.01, vec![click(item(&items, "Collapse All").0)]);
    assert_eq!(app.embroidery.order.folded(uid).len(), 3, "three threads' blocks folded");
    menu_items(&mut app, &ctx, t + 0.02, vec![click(item(&items, "Expand All").0)]);
    assert!(app.embroidery.order.folded(uid).is_empty());
    // The top square selected: sewn first from the menu, as one undo step.
    app.run("select.set", json!({ "ids": [ids[2]] })).unwrap();
    let items = menu_items(&mut app, &ctx, t + 0.03, vec![]);
    assert!(["Sew Earlier", "Sew Later", "Sew First", "Sew Last"].iter().all(|w| item(&items, w).1), "{items:?}");
    let undo = app.session.doc().unwrap().history.undo.len();
    menu_items(&mut app, &ctx, t + 0.04, vec![click(item(&items, "Sew First").0)]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    // Off until the plan of the document as it is comes.
    let items = menu_items(&mut app, &ctx, t + 0.05, vec![]);
    assert!(!item(&items, "Sew Last").1, "{items:?}");
    let t = panel(&mut app);
    let first = app.embroidery.preview.shown(uid).unwrap().planned.plan.units.first().map(|u| u.origin.selected());
    assert_eq!(first, Some(vectorcraft_doc::NodeId(ids[2])), "sewn first");
    assert!(item(&menu_items(&mut app, &ctx, t, vec![]), "Sew Last").1);
    // A finding shown: Next and Previous Finding step through its hits.
    app.ui.embroidery_panel.tab = Tab::Check;
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
    let t = panel(&mut app);
    let reply = app.embroidery.preview.shown(uid).unwrap().reply(&check::query(&app.embroidery.check, uid)).unwrap().result.clone().unwrap();
    check::show_findings(&mut app, uid, &reply, "layers", None).unwrap();
    let items = menu_items(&mut app, &ctx, t, vec![]);
    assert!(item(&items, "Next Finding").1 && item(&items, "Previous Finding").1, "{items:?}");
    menu_items(&mut app, &ctx, t + 0.01, vec![click(item(&items, "Next Finding").0)]);
    assert_eq!(app.embroidery.check.focus.as_ref().and_then(|f| f.hit), Some(0), "the first hit");
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
    assert_eq!(tabs.len(), 3, "{tabs:?}");
    for (name, open) in [("Object", false), ("Sewing Order", true), ("Check", false)] {
        assert!(tabs.contains(&(Some(name.into()), Some(open))), "{name}: {tabs:?}");
    }
}

/// The tab labels the whole window draws along the dock's top, `window` points wide: each one's
/// words, where they lie, and the box they are clipped to.
fn dock_tab_labels(app: &mut VectorcraftApp, ctx: &egui::Context, window: egui::Vec2, labels: &[&str]) -> Vec<(String, egui::Rect, egui::Rect)> {
    let mut found = vec![];
    for _ in 0..4 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, window)), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        found.clear();
        let mut pending: Vec<(egui::Rect, Shape)> = out.shapes.into_iter().map(|c| (c.clip_rect, c.shape)).collect();
        while let Some((clip, s)) = pending.pop() {
            match s {
                Shape::Vec(v) => pending.extend(v.into_iter().map(|s| (clip, s))),
                Shape::Text(t) if labels.contains(&t.galley.text()) => found.push((t.galley.text().to_string(), t.visual_bounding_rect(), clip)),
                _ => {}
            }
        }
    }
    found
}

/// Docked as the fourth tab beside Properties, Layers and Libraries, at the dock's usual width,
/// every tab's label is drawn whole (craft-ui squeezes tabs that don't fit and clips their words),
/// in English and in Polish.
#[test]
fn the_docks_four_tabs_show_their_labels_whole() {
    for lang in ["en", "pl"] {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut app = blank_app();
        app.session.prefs.interface_language = lang.into();
        app.run("window.panel", json!({"panel": ID})).unwrap();
        crate::i18n::set_current(crate::i18n::Lang::from_code(lang).unwrap());
        let labels: Vec<&str> = ["Properties", "Layers", "Libraries", "Embroidery"].into_iter().map(crate::i18n::t).collect();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        let drawn = dock_tab_labels(&mut app, &ctx, egui::vec2(1440.0, 900.0), &labels);
        for label in &labels {
            let (_, words, clip) = drawn.iter().find(|(t, ..)| t == label).unwrap_or_else(|| panic!("{lang}: no tab {label:?} in {drawn:?}"));
            assert!(
                words.left() >= clip.left() - 0.5 && words.right() <= clip.right() + 0.5,
                "{lang}: {label:?} {words:?} is cut by its tab {clip:?}"
            );
        }
    }
}

/// The dock is widened only in the frames just after Window › Embroidery: a dock that doesn't draw
/// the panel's tab by then (collapsed, or the panel elsewhere) isn't widened later when it does,
/// at a moment that has nothing to do with the command; its tabs are then as the dock lays them
/// out (at its usual width, one is cut).
#[test]
fn the_dock_is_widened_only_just_after_window_embroidery() {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut app = blank_app();
    app.run("window.panel", json!({"panel": ID})).unwrap();
    assert!(app.embroidery.fit_dock_frames > 0, "armed by the command");
    for _ in 0..super::FIT_DOCK_FRAMES {
        super::widen_dock(&mut app);
    }
    assert_eq!(app.embroidery.fit_dock_frames, 0, "the command's frames gone");
    let labels = ["Properties", "Layers", "Libraries", "Embroidery"];
    let drawn = dock_tab_labels(&mut app, &ctx, egui::vec2(1440.0, 900.0), &labels);
    assert!(
        drawn.iter().any(|(_, words, clip)| words.left() < clip.left() - 0.5 || words.right() > clip.right() + 0.5),
        "not widened later: {drawn:?}"
    );
}

/// Nearest Connection from the ≡ menu, on the whole design with nothing selected: one undo step,
/// and a line on the status bar saying how many parts it set and what it saved; run again, that the
/// points are the nearest already.
#[test]
fn nearest_connection_from_the_menu_says_what_it_saved() {
    use egui::accesskit::{Action, ActionRequest, TreeId};
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = blank_app();
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setFill", json!({"none": true})).unwrap();
    app.run("paint.setStroke", json!({"color": "#2233cc"})).unwrap();
    // Three running lines (millimetres from 30 mm): the second, started at its end nearer the first,
    // would end 13 mm from the third, past the 10 mm trim length.
    let pt = |v: f64| (30.0 + v) * 72.0 / 25.4;
    for ((ax, ay), (bx, by)) in [((-20.0, 10.0), (0.0, 0.0)), ((1.0, 0.0), (-6.0, 0.0)), ((7.0, 0.0), (27.0, 0.0))] {
        let d = format!("M {} {} L {} {}", pt(ax), pt(ay), pt(bx), pt(by));
        let id = app.run("path.create", json!({ "d": d })).unwrap()["id"].clone();
        app.run("stroke.set", json!({"weight": 1.0, "ids": [id]})).unwrap();
    }
    app.run("select.set", json!({"ids": []})).unwrap();
    let click =
        |id| egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_node: id, target_tree: TreeId::ROOT, data: None });
    let nearest = |items: &[(egui::accesskit::NodeId, String, bool)]| {
        items.iter().find(|(_, w, _)| w == "Nearest Connection").map(|(id, _, on)| (*id, *on)).unwrap_or_else(|| panic!("{items:?}"))
    };
    let (id, on) = nearest(&menu_items(&mut app, &ctx, 0.0, vec![]));
    assert!(on, "on with a document, nothing selected: the whole design");
    let undo = app.session.doc().unwrap().history.undo.len();
    menu_items(&mut app, &ctx, 0.01, vec![click(id)]);
    // It searches off the interface thread: the status bar says how far, until it is applied.
    assert!(crate::embroidery::nearest::running(&app) && app.ui.status.starts_with("Nearest Connection… "), "{}", app.ui.status);
    finish_nearest(&mut app, &ctx);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    assert!(app.ui.status.starts_with("Nearest Connection · 3 parts set · jumps ") && app.ui.status.ends_with(" · trims 1 → 0"), "{}", app.ui.status);
    let (id, _) = nearest(&menu_items(&mut app, &ctx, 0.02, vec![]));
    menu_items(&mut app, &ctx, 0.03, vec![click(id)]);
    finish_nearest(&mut app, &ctx);
    assert_eq!(app.ui.status, "Nearest Connection · The entry and exit points are already the nearest.");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "no step the second time");
}

/// Preferences saved before the embroidery tool's group (the toolbar's last) keep the tool chosen
/// in every other group; the new group takes its tool.
#[test]
fn preferences_from_before_the_embroidery_tool_keep_their_flyout_choices() {
    let mut saved = crate::state::UiState::default();
    saved.group_tool.pop();
    let lasso = vectorcraft_tools::catalog::group_of("directSelection").unwrap();
    saved.group_tool[lasso] = "groupSelection".into();
    let loaded = saved.sanitized();
    assert_eq!(loaded.group_tool.len(), vectorcraft_tools::TOOL_GROUPS.len());
    assert_eq!(loaded.group_tool[lasso], "groupSelection");
    assert_eq!(loaded.group_tool.last().map(String::as_str), Some(vectorcraft_tools::embroidery::ID));
}

/// Nearest Connection's line says what it did: parts set, or already the nearest when it joined
/// parts and moved none, or that nothing in scope can take points when it joined none (lettering
/// converted to outlines, symbol instances…), with how many objects it passed over. Before, a scope
/// of nothing it could set read "already the nearest", which is false.
#[test]
fn nearest_connection_says_when_nothing_can_take_points() {
    let lang = crate::i18n::Lang::from_code("en").unwrap();
    let reply = |changed: u64, unchanged: u64, skipped: u64| json!({"changedCount": changed, "unchangedCount": unchanged, "skippedCount": skipped});
    assert_eq!(nearest_words(&reply(0, 3, 0), lang), "The entry and exit points are already the nearest.");
    assert_eq!(nearest_words(&reply(0, 0, 2), lang), "Nothing here can take entry and exit points · 2 objects skipped");
    assert_eq!(nearest_words(&reply(0, 0, 1), lang), "Nothing here can take entry and exit points · 1 object skipped");
    assert_eq!(nearest_words(&reply(0, 0, 0), lang), "Nothing here can take entry and exit points");
}
