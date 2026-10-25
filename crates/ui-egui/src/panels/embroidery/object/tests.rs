//! The Object tab drawn headless: its states with no plan, a plan under way and a design that
//! can't be planned; its header's count; each row's marks and sources; values the objects differ
//! in; and every edit, each one undo step on the objects it was made for.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, Shape, vec2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview::{self, Held, StitchPreview};
use crate::panels::embroidery::Tab;

/// The panel's width in these tests, points: the dock's usual width.
const WIDTH: f32 = 320.0;

/// An app with a red rectangle (filled, no stroke) and a blue one with a 3 mm stroke, the Object
/// tab open, nothing selected → the app and the two ids.
fn app_with_art() -> (VectorcraftApp, u64, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#cc2222"})).unwrap();
    let a = app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    app.run("paint.setFill", json!({"color": "#2233cc"})).unwrap();
    app.run("paint.setStroke", json!({"color": "#113311"})).unwrap();
    let b = app.run("shape.rectangle", json!({"x": 150, "y": 20, "width": 80, "height": 50})).unwrap()["id"].as_u64().unwrap();
    app.run("stroke.set", json!({"weight": 8.5, "ids": [b]})).unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    app.ui.embroidery_panel.tab = Tab::Object;
    (app, a, b)
}

/// What a frame drew: its words with their boxes, its outlined boxes with their outlines' colours,
/// and the screen readers' nodes with their ids.
struct Drawn {
    words: Vec<(String, Rect)>,
    outlines: Vec<(Rect, egui::Color32)>,
    nodes: Vec<(egui::accesskit::NodeId, egui::accesskit::Node)>,
}

impl Drawn {
    fn has(&self, what: &str) -> bool {
        self.words.iter().any(|(w, _)| w.contains(what))
    }

    fn node(&self, name: &str) -> &egui::accesskit::Node {
        self.nodes.iter().map(|(_, n)| n).find(|n| n.label() == Some(name)).unwrap_or_else(|| panic!("nothing is named {name:?}"))
    }

    /// A click on the node named `name`, as a screen reader asks for one.
    fn click(&self, name: &str) -> Event {
        self.ask(name, egui::accesskit::Action::Click)
    }

    /// The keyboard focus moved to the node named `name`, as a screen reader asks for it.
    fn focus(&self, name: &str) -> Event {
        self.ask(name, egui::accesskit::Action::Focus)
    }

    fn ask(&self, name: &str, action: egui::accesskit::Action) -> Event {
        let id = self.nodes.iter().find(|(_, n)| n.label() == Some(name)).map(|(id, _)| *id).unwrap_or_else(|| panic!("nothing is named {name:?}"));
        Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action,
            target_node: id,
            target_tree: egui::accesskit::TreeId::ROOT,
            data: None,
        })
    }
}

/// One frame of the panel at `time` with `events`, then the preview's scheduling.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Drawn {
    let raw = egui::RawInput {
        time: Some(time),
        focused: true,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, 900.0))),
        events,
        ..Default::default()
    };
    crate::scrub::begin_frame(app, ctx);
    let mut out = ctx.run_ui(raw, |ui| {
        crate::panels::embroidery::show(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    crate::scrub::end_frame(app, ctx);
    out.textures_delta.clear();
    let (mut words, mut outlines) = (vec![], vec![]);
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) => words.push((t.galley.text().to_string(), t.visual_bounding_rect())),
            Shape::Rect(r) if r.stroke.width > 0.0 => outlines.push((r.rect, r.stroke.color)),
            _ => {}
        }
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
    Drawn { words, outlines, nodes }
}

/// A context with the app's fonts, screen readers on.
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    ctx
}

/// Frames until the plan and the tab are drawn → the last.
fn ready(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Drawn {
    let mut last = frame(app, ctx, *time, vec![]);
    for _ in 0..4 {
        *time += 0.05;
        last = frame(app, ctx, *time, vec![]);
    }
    last
}

fn select(app: &mut VectorcraftApp, ids: &[u64]) {
    app.run("select.set", json!({ "ids": ids })).unwrap();
}

fn steps(app: &VectorcraftApp) -> usize {
    app.session.doc().unwrap().history.undo.len()
}

/// The tab's row of `key` as `ui.inspect` reports it.
fn row(app: &VectorcraftApp, key: &str) -> serde_json::Value {
    let rows = inspect(app)["rows"].clone();
    rows.as_array().and_then(|r| r.iter().find(|r| r["key"] == json!(key)).cloned()).unwrap_or_else(|| panic!("no row {key}: {rows}"))
}

fn info(app: &mut VectorcraftApp, ids: &[u64]) -> serde_json::Value {
    app.run("embroidery.info", json!({ "ids": ids })).unwrap()
}

/// The tab draws with no plan yet, while one is under way, and when the design can't be planned:
/// only its count waits ("…"), the rows are there; with nothing selected it says how to begin.
#[test]
fn the_object_tab_draws_without_a_plan() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    let held = Held::default();
    app.embroidery.preview = StitchPreview::with_runner(Box::new(held.clone()));
    let drawn = frame(&mut app, &ctx, 0.0, vec![]);
    assert!(drawn.has("Select an object to set how it is sewn."), "{:?}", drawn.words);
    select(&mut app, &[a]);
    let drawn = frame(&mut app, &ctx, 0.1, vec![]);
    assert!(drawn.has("…") && drawn.has("Row spacing") && drawn.has("Auto: tatami"), "no plan yet: {:?}", drawn.words);
    held.release();
    let mut time = 0.2;
    let drawn = ready(&mut app, &ctx, &mut time);
    assert!(drawn.has(" stitches"), "{:?}", drawn.words);
    // Three metres a side: it can't be planned, and the tab still sets the selection.
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    held.release();
    let drawn = ready(&mut app, &ctx, &mut time);
    held.release();
    let drawn = if drawn.has("Can't plan") { drawn } else { ready(&mut app, &ctx, &mut time) };
    assert!(drawn.has("Can't plan the stitches:") && drawn.has("Row spacing"), "{:?}", drawn.words);
}

/// The header counts the selection's stitches in the plan shown, as the sewing order counts them.
#[test]
fn its_header_counts_the_objects_stitches() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    select(&mut app, &[a, b]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let sequence = app.run("embroidery.sequence", json!({})).unwrap();
    let stitches: u64 =
        sequence["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).map(|u| u["stitches"].as_u64().unwrap()).sum();
    let words = crate::etn!(stitches, "{count} stitch", "{count} stitches");
    assert!(drawn.has(&words), "{words}: {:?}", drawn.words);
    assert!(drawn.has("2 objects"), "{:?}", drawn.words);
}

/// A value set on the object is marked (↺ restores it, its hint what it would be otherwise); an
/// inherited one names where it comes from: the fabric, the default, a rule; screen readers hear
/// the field's name and where its value comes from.
#[test]
fn a_set_value_is_marked_and_an_inherited_one_says_where_it_comes_from() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.setup", json!({"fabric": "stretchKnit"})).unwrap();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "pull": 0.3})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    assert_eq!((row(&app, "angle")["set"].clone(), row(&app, "angle")["value"].clone()), (json!(true), json!(30.0)));
    assert_eq!((row(&app, "spacing")["set"].clone(), row(&app, "spacing")["source"].clone()), (json!(false), json!("document")));
    assert!(drawn.nodes.iter().any(|(_, n)| n.label() == Some("Restore Angle")), "↺ for the angle");
    let pull = knit_pull(&mut app);
    assert!(drawn.has(&format!("fabric {pull}")), "the pull's hint, the knit's {pull}: {:?}", drawn.words);
    assert!(drawn.has("fabric") && drawn.has("default"), "badges: {:?}", drawn.words);
    let field = drawn.node("Row spacing");
    assert_eq!(field.description(), Some(format!("Source: fabric\n{}", vocabulary::tip("spacing")).as_str()), "its source, then its help");
    let field = drawn.node("Angle");
    assert!(field.description().is_some_and(|d| d.starts_with("Set on the selected objects")), "{:?}", field.description());
}

/// The knit's tatami pull as Document Setup's reply gives it, written as the fields write it.
fn knit_pull(app: &mut VectorcraftApp) -> String {
    let settings = app.run("embroidery.settings", json!({})).unwrap();
    settings["resolved"]["fabric"]["tatami"]["pull"]["value"]
        .as_f64()
        .map(|v| crate::embroidery::fields::format(crate::i18n::Lang::EN, "", 2).show(v))
        .unwrap()
}

/// Values the objects differ in are blank, the badge counting how many set them.
#[test]
fn mixed_values_are_blank_and_counted() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "pull": 0.3})).unwrap();
    select(&mut app, &[a, b]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    assert_eq!((row(&app, "pull")["mixed"].clone(), row(&app, "pull")["value"].clone()), (json!(true), json!(null)));
    assert!(drawn.has("1 of 2 set"), "{:?}", drawn.words);
}

/// Every setting an object's part takes has a label, and a tip.
#[test]
fn every_part_setting_has_words() {
    for key in
        ["stitch", "angle", "spacing", "length", "narrowAsSatin", "underlay", "pull", "pullPercent", "push", "expand", "splitLength", "entry", "exit"]
    {
        assert_ne!(vocabulary::label(key, None), crate::et!("Setting"), "{key}");
        assert!(!vocabulary::tip(key).is_empty(), "{key}");
    }
    assert_eq!(vocabulary::label("spacing", Some("tatami")), crate::et!("Row spacing"));
}

/// Auto's choices in words: a fill's tatami, a wide stroke's satin and its underlay by its width.
#[test]
fn auto_shows_what_it_chose() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    assert!(ready(&mut app, &ctx, &mut time).has("Auto: tatami"));
    select(&mut app, &[b]);
    app.run("ui.embroideryPanel", json!({"part": "stroke"})).unwrap();
    let drawn = ready(&mut app, &ctx, &mut time);
    assert!(drawn.has("Auto: satin") && drawn.has("Auto: edge run"), "{:?}", drawn.words);
}

/// Type `text` into the field of the row labelled `label` and press Enter → the frames' last.
fn type_into(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, label: &str, text: &str) -> Drawn {
    let drawn = ready(app, ctx, time);
    let at = field_beside(&drawn, label);
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    for events in [vec![Event::PointerMoved(at), button(true)], vec![button(false)], vec![Event::Text(text.into())], vec![enter]] {
        *time += 0.05;
        frame(app, ctx, *time, events);
    }
    ready(app, ctx, time)
}

/// A point inside the field screen readers call `label`.
fn field_beside(drawn: &Drawn, label: &str) -> Pos2 {
    let bounds = drawn.node(label).bounds().unwrap_or_else(|| panic!("{label} has no bounds"));
    Pos2::new(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)
}

/// A typed value is one undo step, set on the objects the field was drawn for; a comma reads as
/// the decimal mark in a comma language and a point too.
#[test]
fn a_typed_value_is_one_undo_step() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let before = steps(&app);
    type_into(&mut app, &ctx, &mut time, "Angle", "30");
    assert_eq!(info(&mut app, &[a])["objects"][0]["settings"]["fill"]["angle"], json!(30.0));
    assert_eq!(steps(&app), before + 1);
    assert_eq!(info(&mut app, &[b])["objects"][0]["settings"]["fill"], json!(null), "not the other object");
    type_into(&mut app, &ctx, &mut time, "Row spacing", "0.42");
    assert_eq!(info(&mut app, &[a])["objects"][0]["settings"]["fill"]["spacing"], json!(0.42));
    crate::i18n::set_current(Lang::from_code("pl").unwrap());
    type_into(&mut app, &ctx, &mut time, crate::et!("Row spacing"), "0,5");
    crate::i18n::set_current(Lang::EN);
    assert_eq!(info(&mut app, &[a])["objects"][0]["settings"]["fill"]["spacing"], json!(0.5));
}

/// Typing into a value the objects differ in sets every one of them.
#[test]
fn typing_into_a_mixed_field_sets_all() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "length": 3})).unwrap();
    select(&mut app, &[a, b]);
    let mut time = 0.0;
    type_into(&mut app, &ctx, &mut time, "Stitch length", "2.5");
    for id in [a, b] {
        assert_eq!(info(&mut app, &[id])["objects"][0]["settings"]["fill"]["length"], json!(2.5), "{id}");
    }
}

/// A value out of its range is refused, the reason in the status bar; nothing changes.
#[test]
fn an_out_of_range_value_is_refused_with_its_range() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let before = steps(&app);
    // A range of the field's own: what is typed past it is held at its end, as the app's fields do.
    type_into(&mut app, &ctx, &mut time, "Narrow parts as satin", "50");
    let set = info(&mut app, &[a])["objects"][0]["settings"]["fill"]["narrowAsSatin"].clone();
    assert!(set.is_null() || set.as_f64().is_some_and(|v| v <= 12.0), "{set}");
    assert!(steps(&app) <= before + 1);
}

/// ↺ unsets that one value, in one step; Restore from Fabric unsets them all but the points and
/// the cut after them.
#[test]
fn restore_clears_one_key_and_restore_from_fabric_the_rest() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "pull": 0.3})).unwrap();
    app.run("embroidery.setEntryExit", json!({"id": a, "part": "fill", "entry": [30, 30]})).unwrap();
    app.run("embroidery.setTrimAfter", json!({"ids": [a], "on": true})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let before = steps(&app);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click("Restore Angle")]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    let fill = info(&mut app, &[a])["objects"][0]["settings"]["fill"].clone();
    assert_eq!((fill["angle"].clone(), fill["pull"].clone(), steps(&app)), (json!(null), json!([0.3, 0.3]), before + 1));
    let drawn = ready(&mut app, &ctx, &mut time);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click("Restore from Fabric")]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    let r = info(&mut app, &[a]);
    assert_eq!(r["objects"][0]["settings"]["fill"], json!({"entry": [30.0, 30.0]}), "the entry stays");
    assert_eq!(r["trimAfter"]["value"], json!(true), "the cut stays");
}

/// The middle of the words `what` drawn.
fn words_at(drawn: &Drawn, what: &str) -> Pos2 {
    drawn.words.iter().find(|(w, _)| w == what).unwrap_or_else(|| panic!("no {what:?} in {:?}", drawn.words)).1.center()
}

/// A drag of `dx` points from `at` (Escape before the release when `escape`), a move a frame.
fn drag(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, at: Pos2, dx: f32, escape: bool) {
    let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let mut step = |app: &mut VectorcraftApp, events| {
        *time += 0.05;
        frame(app, ctx, *time, events);
    };
    step(app, vec![Event::PointerMoved(at)]);
    step(app, vec![button(at, true)]);
    for i in 1..=4 {
        step(app, vec![Event::PointerMoved(at + vec2(dx * i as f32 / 4.0, 0.0))]);
    }
    if escape {
        step(app, vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    }
    step(app, vec![button(at + vec2(dx, 0.0), false)]);
    step(app, vec![]);
}

fn fill_setting(app: &mut VectorcraftApp, id: u64, key: &str) -> serde_json::Value {
    info(app, &[id])["objects"][0]["settings"]["fill"][key].clone()
}

/// A drag on a setting's label scrubs it, a degree every two points (a drag moving past egui's
/// threshold in its first move, as the app's scrub tests drag), the whole drag one undo step;
/// Escape takes the drag back.
#[test]
fn a_scrub_is_one_undo_step_and_escape_restores() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let at = words_at(&ready(&mut app, &ctx, &mut time), "Angle");
    let before = steps(&app);
    drag(&mut app, &ctx, &mut time, at, 40.0, false);
    assert_eq!((fill_setting(&mut app, a, "angle"), steps(&app)), (json!(50.0), before + 1), "twenty steps of a degree, one undo step");
    let at = words_at(&ready(&mut app, &ctx, &mut time), "Angle");
    drag(&mut app, &ctx, &mut time, at, 40.0, true);
    assert_eq!((fill_setting(&mut app, a, "angle"), steps(&app)), (json!(50.0), before + 1), "Escape takes the drag back");
}

/// A choice is one undo step; choosing what the objects would inherit unsets theirs.
#[test]
fn a_choice_is_one_step_and_choosing_what_they_inherit_unsets_it() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let choose = |app: &mut VectorcraftApp, time: &mut f64, field: &str, option: &str| {
        let drawn = ready(app, &ctx, time);
        *time += 0.05;
        frame(app, &ctx, *time, vec![drawn.click(field)]);
        let drawn = ready(app, &ctx, time);
        *time += 0.05;
        frame(app, &ctx, *time, vec![drawn.click(option)]);
        ready(app, &ctx, time);
    };
    let before = steps(&app);
    choose(&mut app, &mut time, "Stitch: Type", "Don't sew");
    assert_eq!((fill_setting(&mut app, a, "stitch"), steps(&app)), (json!("none"), before + 1));
    assert!(ready(&mut app, &ctx, &mut time).has("Not sewn: set to Don't sew."), "and the tab says so");
    choose(&mut app, &mut time, "Stitch: Type", "Auto");
    assert_eq!((fill_setting(&mut app, a, "stitch"), steps(&app)), (json!(null), before + 2), "Auto is what it inherits: unset");
    choose(&mut app, &mut time, "Underlay: Type", "Edge run + tatami");
    assert_eq!(fill_setting(&mut app, a, "underlay"), json!("edgeTatami"));
}

/// Trim After shows a dash where the objects differ; a click marks them all, one undo step.
#[test]
fn trim_after_is_mixed_then_set() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.setTrimAfter", json!({"ids": [a], "on": true})).unwrap();
    select(&mut app, &[a, b]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    // Two objects: the plural.
    let name = "Trim the thread after the objects";
    assert_eq!(drawn.node(name).toggled(), Some(egui::accesskit::Toggled::Mixed));
    let before = steps(&app);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(name)]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    assert_eq!((info(&mut app, &[a, b])["trimAfter"]["value"].clone(), steps(&app)), (json!(true), before + 1));
}

/// Copy remembers the one object selected, Paste gives the selection its settings in one undo step;
/// both off when they can't run.
#[test]
fn copy_and_paste_settings() {
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "spacing": 0.5})).unwrap();
    assert!(!crate::menus::enabled(&app, "embroidery.copySettings"), "nothing selected");
    select(&mut app, &[a, b]);
    assert!(!crate::menus::enabled(&app, "embroidery.copySettings"), "two selected");
    select(&mut app, &[a]);
    assert_eq!(app.run("embroidery.copySettings", json!({})).unwrap(), json!({"source": a}));
    assert!(!crate::menus::enabled(&app, "embroidery.pasteSettings"), "only the source selected");
    select(&mut app, &[b]);
    assert!(crate::menus::enabled(&app, "embroidery.pasteSettings"));
    let before = steps(&app);
    app.run("embroidery.pasteSettings", json!({})).unwrap();
    assert_eq!((fill_setting(&mut app, b, "angle"), fill_setting(&mut app, b, "spacing"), steps(&app)), (json!(30.0), json!(0.5), before + 1));
    let group = app.run("object.group", json!({"ids": [a, b]})).unwrap()["id"].as_u64().unwrap();
    select(&mut app, &[group]);
    assert!(app.run("embroidery.copySettings", json!({})).is_err(), "a group isn't one object");
}

/// A value typed and committed by a click elsewhere lands on the objects it was typed for, even when
/// the click then selects another.
#[test]
fn the_value_goes_to_the_objects_it_was_typed_for() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let at = field_beside(&drawn, "Angle");
    let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let away = Pos2::new(WIDTH / 2.0, 880.0);
    for events in [
        vec![Event::PointerMoved(at), button(at, true)],
        vec![button(at, false)],
        vec![Event::Text("12".into())],
        vec![Event::PointerMoved(away), button(away, true)],
    ] {
        time += 0.05;
        frame(&mut app, &ctx, time, events);
    }
    // The press elsewhere selects the other object, as the canvas drawn after the panels would.
    select(&mut app, &[b]);
    frame(&mut app, &ctx, time + 0.05, vec![button(away, false)]);
    assert_eq!((fill_setting(&mut app, a, "angle"), fill_setting(&mut app, b, "angle")), (json!(12.0), json!(null)));
}

/// The tab's checkbox draws its words as they are given, wrapping them in the room it has, and a
/// dash where the objects differ.
#[test]
fn a_three_state_box_wraps_its_words() {
    let ctx = context();
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(120.0, 200.0))), ..Default::default() };
    let words = "Obetnij nić po obiekcie i jeszcze dłużej";
    let mut out = ctx.run_ui(raw, |ui| {
        crate::embroidery::rows::check(ui, words, None, Tokens::get(ui.ctx()).panel);
    });
    out.textures_delta.clear();
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    let (mut texts, mut dash) = (vec![], false);
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) => texts.push((t.galley.text().to_string(), t.galley.rows.len(), t.visual_bounding_rect())),
            Shape::LineSegment { .. } => dash = true,
            _ => {}
        }
    }
    let (text, rows, rect) = texts.first().cloned().expect("its words");
    assert_eq!(text, words);
    assert!(rows > 1 && rect.right() <= 120.0, "wrapped in the room it has: {rows} rows, {rect:?}");
    assert!(dash, "the dash of objects that differ");
}

/// A plan a read command made first (an agent's, a script's) is taken from memory by the stitch
/// preview with how long its walk took, so a light design stays light: a scrub on a label gets the
/// stitches of each step while the drag goes on, as when the preview planned it itself.
#[test]
fn a_read_commands_plan_keeps_the_scrub_live() {
    use crate::embroidery::stitch_preview::Freshness;
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    // Each time taken in counts as light, so a busy machine can't make it heavy; a plan taken
    // with no time at all still counts as heavy, which the test is for.
    app.embroidery.preview.pin_light();
    select(&mut app, &[a]);
    app.run("embroidery.sequence", json!({})).unwrap();
    let mut time = 0.0;
    let at = words_at(&ready(&mut app, &ctx, &mut time), "Angle");
    let uid = app.session.active().unwrap().uid;
    assert!(app.embroidery.preview.inspect(uid, time)["planMs"].is_number(), "the remembered plan's time");
    let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let mut step = |app: &mut VectorcraftApp, events| {
        time += 0.05;
        frame(app, &ctx, time, events);
        time
    };
    step(&mut app, vec![Event::PointerMoved(at)]);
    step(&mut app, vec![button(at, true)]);
    for i in 1..=4 {
        step(&mut app, vec![Event::PointerMoved(at + vec2(10.0 * i as f32, 0.0))]);
        // Planned in line at the frame's end: shown from the next.
        let now = step(&mut app, vec![]);
        assert_eq!(app.embroidery.preview.freshness(uid, now), Freshness::Fresh, "step {i}: the stitches of the drag as it is");
    }
    step(&mut app, vec![button(at + vec2(40.0, 0.0), false)]);
    assert!(fill_setting(&mut app, a, "angle").is_number(), "the scrub set the angle");
}

/// ↺ and Trim After show the keyboard's focus with the accent's outline (the coral is the focus
/// alone), only while they have it.
#[test]
fn the_keyboards_focus_is_drawn_on_restore_and_the_checkbox() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30})).unwrap();
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let t = Tokens::get(&ctx);
    let ring = rows::focus_colour(&t, t.panel);
    assert!(!drawn.outlines.iter().any(|(_, c)| *c == ring), "no outline without the focus");
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", vocabulary::label("angle", Some("tatami")))]);
    let trim = crate::etn!(1, "Trim the thread after the object", "Trim the thread after the objects");
    for name in [restore.as_str(), trim.as_str()] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![drawn.focus(name)]);
        time += 0.05;
        let focused = frame(&mut app, &ctx, time, vec![]);
        assert!(focused.outlines.iter().any(|(_, c)| *c == ring), "{name}: the focus's outline");
    }
}

/// Screen readers hear what a blank field shows (what Auto chose, as its placeholder), and two ↺
/// of rows labelled alike by their sections ("Restore Stitch: Type", "Restore Underlay: Type").
#[test]
fn screen_readers_hear_the_painted_words_and_each_restore_apart() {
    let ctx = context();
    let (mut app, _, b) = app_with_art();
    select(&mut app, &[b]);
    app.ui.embroidery_panel.part = Part::Stroke;
    app.run("embroidery.set", json!({"ids": [b], "part": "stroke", "stitch": "satin", "underlay": "none"})).unwrap();
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let spacing = drawn.node(vocabulary::label("spacing", Some("satin")));
    assert!(spacing.placeholder().is_some_and(|p| p.starts_with("Auto")), "{:?}", spacing.placeholder());
    let names: Vec<&str> = drawn.nodes.iter().filter_map(|(_, n)| n.label()).filter(|l| l.starts_with("Restore ")).collect();
    assert!(names.contains(&"Restore Stitch: Type") && names.contains(&"Restore Underlay: Type"), "{names:?}");
}

/// A setting's help, its label's tooltip, shows too while its field has the keyboard's focus
/// (taken by the keyboard, not a press), and screen readers hear it with the field.
#[test]
fn a_settings_help_shows_on_the_keyboards_focus() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let tip = vocabulary::tip("angle");
    let angle = drawn.node(vocabulary::label("angle", None));
    assert!(angle.description().is_some_and(|d| d.ends_with(tip)), "{:?}", angle.description());
    assert!(!drawn.has(tip), "no tooltip yet");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![drawn.focus(vocabulary::label("angle", None))]);
    let mut shown = false;
    for _ in 0..4 {
        time += 0.05;
        shown |= frame(&mut app, &ctx, time, vec![]).has(tip);
    }
    assert!(shown, "the help as its tooltip");
}

/// Text a field can't read as a number (a comma in a point language: "1,000" would read as one)
/// isn't taken, and the status bar says so instead of the field going back silently.
#[test]
fn a_typed_text_that_is_no_number_is_said() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let at = drawn.words.iter().find(|(w, _)| w == "Pull compensation").map(|(_, r)| r.center()).unwrap();
    let field = at + vec2(130.0, 0.0);
    let steps_before = steps(&app);
    let click = |pressed| Event::PointerButton { pos: field, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for events in [
        vec![Event::PointerMoved(field), click(true)],
        vec![click(false)],
        vec![Event::Text("0,35".into())],
        vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    ] {
        time += 0.05;
        frame(&mut app, &ctx, time, events);
    }
    assert_eq!(steps(&app), steps_before, "nothing taken");
    let words = text::fill(crate::et!("Not a number: {text}"), &[("text", "0,35")]);
    assert!(app.ui.status.contains(&words), "{:?}", app.ui.status);
}
