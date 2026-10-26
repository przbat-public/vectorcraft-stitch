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
    frame_in(app, ctx, time, events, WIDTH)
}

/// [`frame`] in a panel `width` points wide.
fn frame_in(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>, width: f32) -> Drawn {
    let raw = egui::RawInput {
        time: Some(time),
        focused: true,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 900.0))),
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
    let pill = |drawn: &Drawn, w: &str| drawn.words.iter().any(|(word, _)| word == w);
    assert!(pill(&drawn, "…") && drawn.has("Row spacing") && drawn.has("Auto: tatami"), "no plan yet: {:?}", drawn.words);
    assert!(!drawn.has(" stitches"), "no count before the first plan: {:?}", drawn.words);
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
    let mut time = 0.0;
    let sequence = app.run("embroidery.sequence", json!({})).unwrap();
    let units: Vec<(u64, u64)> = sequence["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|b| b["units"].as_array().unwrap())
        .map(|u| (u["id"].as_u64().unwrap(), u["stitches"].as_u64().unwrap()))
        .collect();
    let of = |ids: &[u64]| units.iter().filter(|(id, _)| ids.contains(id)).map(|(_, n)| n).sum::<u64>();
    // One of the two: its own stitches, not the design's.
    select(&mut app, &[a]);
    let drawn = ready(&mut app, &ctx, &mut time);
    let words = crate::etn!(of(&[a]), "{count} stitch", "{count} stitches");
    assert!(of(&[a]) < of(&[a, b]) && drawn.has(&words), "{words}: {:?}", drawn.words);
    select(&mut app, &[a, b]);
    let drawn = ready(&mut app, &ctx, &mut time);
    let words = crate::etn!(of(&[a, b]), "{count} stitch", "{count} stitches");
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
    let badge = |w: &str| drawn.words.iter().any(|(word, _)| word == w);
    assert!(badge("fabric") && badge("default"), "badges: {:?}", drawn.words);
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

/// Every setting an object's part takes has a label and a tip, and every reason, fabric, stabiliser
/// and palette the engine lists has words of its own: tied to the engine's lists, not to a list of
/// the test's.
#[test]
fn every_part_setting_has_words() {
    // The engine's lists: every part key, every reason an object is skipped, every fabric preset and
    // its stabiliser, every palette.
    for key in vectorcraft_doc::embroidery::PartKey::ALL.iter().map(|k| k.name()) {
        assert_ne!(vocabulary::label(key, None), crate::et!("Setting"), "{key}");
        assert!(!vocabulary::tip(key).is_empty(), "{key}");
    }
    assert_eq!(vocabulary::label("spacing", Some("tatami")), crate::et!("Row spacing"));
    for reason in vectorcraft_engine::cmd::embroidery::skip_reasons() {
        // A part a command was given that the object hasn't: no object-wide reason of its own.
        if reason != "noPart" {
            assert_ne!(vocabulary::takes_none(reason), crate::et!("It takes no embroidery settings."), "{reason}");
        }
    }
    let fabrics = Session::new().execute("embroidery.fabrics", &json!({})).unwrap();
    for f in fabrics["fabrics"].as_array().unwrap() {
        let id = f["id"].as_str().unwrap();
        assert_ne!(vocabulary::fabric(id), id, "{id}");
        if let Some(s) = f["values"]["stabilizer"].as_str() {
            assert_ne!(vocabulary::information(s), s, "{s}");
        }
    }
    for id in vectorcraft_doc::embroidery::ThreadPalette::KNOWN.iter().filter_map(vectorcraft_doc::embroidery::ThreadPalette::id) {
        assert_ne!(vocabulary::palette(id), id, "{id}");
    }
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
    // A value typed is taken as typed: the engine refuses one past the field's range, and the status
    // bar says the range.
    type_into(&mut app, &ctx, &mut time, "Narrow parts as satin", "50");
    let set = info(&mut app, &[a])["objects"][0]["settings"]["fill"]["narrowAsSatin"].clone();
    assert!(set.is_null(), "not set: {set}");
    assert_eq!(steps(&app), before, "no undo step");
    assert!(app.ui.status.contains("12"), "the range said: {:?}", app.ui.status);
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
/// A drag of `dx` points from `at` (Escape before the release when `escape`) → object `watch`'s fill
/// angle as the drag stood just before its end.
fn drag(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, at: Pos2, dx: f32, escape: bool, watch: u64) -> serde_json::Value {
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
    let during = fill_setting(app, watch, "angle");
    if escape {
        step(app, vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    }
    step(app, vec![button(at + vec2(dx, 0.0), false)]);
    step(app, vec![]);
    during
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
    drag(&mut app, &ctx, &mut time, at, 40.0, false, a);
    assert_eq!((fill_setting(&mut app, a, "angle"), steps(&app)), (json!(50.0), before + 1), "twenty steps of a degree, one undo step");
    let at = words_at(&ready(&mut app, &ctx, &mut time), "Angle");
    let during = drag(&mut app, &ctx, &mut time, at, 40.0, true, a);
    assert_eq!(during, json!(70.0), "the second drag scrubbed before Escape");
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

/// Copy takes the one object selected's settings as they are then (a snapshot: a later change to
/// it, or its undo, changes nothing pasted); Paste gives the selection them in one undo step, also
/// in another document, a part alone with `part`, and says what it passed over; both off when they
/// can't run.
#[test]
fn copy_and_paste_settings() {
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "spacing": 0.5})).unwrap();
    assert!(!crate::menus::enabled(&app, "embroidery.copySettings"), "nothing selected");
    select(&mut app, &[a, b]);
    assert!(!crate::menus::enabled(&app, "embroidery.copySettings"), "two selected");
    assert!(!crate::menus::enabled(&app, "embroidery.pasteSettings"), "nothing copied");
    select(&mut app, &[a]);
    assert_eq!(app.run("embroidery.copySettings", json!({})).unwrap(), json!({"source": a, "passedOver": []}));
    // Changed after Copy: the copy keeps what it took.
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 60})).unwrap();
    select(&mut app, &[b]);
    assert!(crate::menus::enabled(&app, "embroidery.pasteSettings"));
    let before = steps(&app);
    app.run("embroidery.pasteSettings", json!({})).unwrap();
    assert_eq!((fill_setting(&mut app, b, "angle"), fill_setting(&mut app, b, "spacing"), steps(&app)), (json!(30.0), json!(0.5), before + 1));
    // A part alone.
    app.run("embroidery.set", json!({"ids": [b], "part": "fill", "angle": null, "spacing": null})).unwrap();
    app.run("embroidery.pasteSettings", json!({"part": "stroke"})).unwrap();
    assert_eq!(fill_setting(&mut app, b, "angle"), json!(null), "the fill left as it was");
    // Into another document, with type selected beside a path: the type passed over, and said.
    app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let c = app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 40, "height": 30})).unwrap()["id"].as_u64().unwrap();
    let t = app.run("text.create", json!({"x": 20, "y": 150, "text": "Haft"})).unwrap()["id"].as_u64().unwrap();
    select(&mut app, &[c, t]);
    let r = app.run("embroidery.pasteSettings", json!({})).unwrap();
    assert_eq!((fill_setting(&mut app, c, "angle"), r["skippedCount"].clone()), (json!(30.0), json!(1)));
    let said = crate::etn!(
        1,
        "{count} selected object takes no settings and was passed over",
        "{count} selected objects take no settings and were passed over"
    );
    assert!(app.ui.status.contains(&said), "{:?}", app.ui.status);
    let group = app.run("object.group", json!({"ids": [c, t]})).unwrap()["id"].as_u64().unwrap();
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

/// Trim After is about what is sewn: a path marked beside type shows ticked, not mixed, and a click
/// sets nothing on the type.
#[test]
fn trim_after_leaves_out_what_isnt_sewn() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    let t = app.run("text.create", json!({"x": 20, "y": 150, "text": "Haft"})).unwrap()["id"].as_u64().unwrap();
    app.run("embroidery.setTrimAfter", json!({"ids": [a], "on": true})).unwrap();
    select(&mut app, &[a, t]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let name = crate::etn!(2, "Trim the thread after the object", "Trim the thread after the objects");
    assert_eq!(drawn.node(&name).toggled(), Some(egui::accesskit::Toggled::True), "the type isn't counted");
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(&name)]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    assert_eq!(info(&mut app, &[a])["trimAfter"]["value"], json!(false));
    let n = app.session.active().unwrap().doc.node(vectorcraft_doc::NodeId(t)).unwrap().extras().embroidery.clone();
    assert!(n.is_none(), "nothing set on the type");
}

/// With objects that take settings and one that takes none selected, the header says how many take
/// none, not that they "aren't sewn" (a symbol's instance and stitches from a file are sewn).
#[test]
fn the_header_counts_the_objects_that_take_no_settings() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    let t = app.run("text.create", json!({"x": 20, "y": 150, "text": "Haft"})).unwrap()["id"].as_u64().unwrap();
    select(&mut app, &[a, t]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let words = crate::etn!(1, "{count} selected object takes no settings", "{count} selected objects take no settings");
    assert!(drawn.words.iter().any(|(w, _)| *w == words), "{words}: {:?}", drawn.words);
}

/// `ui.inspect`'s report of the tab is of the active document's selection as it is: null once the
/// selection changed until the tab is drawn again; a value the objects differ in isn't `set`; and
/// `ui.embroideryPanel` answers the part the tab shows.
#[test]
fn the_inspected_tab_is_of_the_selection_as_it_is() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a, b], "part": "fill", "angle": 30})).unwrap();
    app.run("embroidery.set", json!({"ids": [b], "part": "fill", "angle": 60})).unwrap();
    select(&mut app, &[a, b]);
    let mut time = 0.0;
    ready(&mut app, &ctx, &mut time);
    assert_eq!((row(&app, "angle")["set"].clone(), row(&app, "angle")["mixed"].clone()), (json!(false), json!(true)));
    select(&mut app, &[a]);
    assert_eq!(inspect(&app), serde_json::Value::Null, "not drawn for this selection yet");
    ready(&mut app, &ctx, &mut time);
    assert_eq!(inspect(&app)["ids"], json!([a]));
    let r = app.run("ui.embroideryPanel", json!({"part": "stroke"})).unwrap();
    assert_eq!(r["part"], json!("fill"), "the red rectangle paints no stroke: the fill shown");
}

/// The tab's commands leave out `ids` when they are the selection (each defaults to it), so that a
/// selection larger than the commands' list of ids (100,000) takes edits too; for objects other
/// than the selection (it changed since the fields were drawn) they keep them.
#[test]
fn the_selection_goes_without_its_ids() {
    let (a, b) = (vectorcraft_doc::NodeId(1), vectorcraft_doc::NodeId(2));
    let mut p = json!({"ids": [1, 2], "part": "fill", "angle": 30});
    for_selection(&mut p, &[a, b], &[a, b]);
    assert_eq!(p, json!({"part": "fill", "angle": 30}));
    let mut p = json!({"ids": [1, 2], "part": "fill"});
    for_selection(&mut p, &[a, b], &[a]);
    assert_eq!(p["ids"], json!([1, 2]), "not the selection: kept");
}

/// The footer names the document's fabric and its thread from the document alone: its preset, or
/// "Custom" for values of its own.
#[test]
fn the_footer_names_the_fabric_and_the_thread() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.setup", json!({"fabric": {"tatami": {"pull": 0.3}}, "threadWeight": 60})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let line = text::fill(crate::et!("Fabric: {fabric} · thread {weight} wt"), &[("fabric", crate::et!("Custom")), ("weight", "60")]);
    assert!(drawn.words.iter().any(|(w, _)| *w == line), "{line}: {:?}", drawn.words);
}

/// The selection's settings are read once a snapshot: not again on idle frames, nor on the preview
/// frames of a drag on the canvas (their settings don't change), but once the drag is committed.
#[test]
fn the_settings_are_read_once_a_snapshot_and_not_during_a_drag() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let mut time = 0.0;
    ready(&mut app, &ctx, &mut time);
    let reads = app.embroidery.object.reads;
    for _ in 0..5 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(app.embroidery.object.reads, reads, "idle frames read nothing");
    app.session.begin_interaction("Move").unwrap();
    for k in 1..=5 {
        app.session.preview("object.move", &json!({"dx": k, "dy": 0})).unwrap();
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(app.embroidery.object.reads, reads, "a drag's frames read nothing");
    app.session.commit_interaction().unwrap();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert_eq!(app.embroidery.object.reads, reads + 1, "read again once committed");
}

/// The stitch choices are `embroidery.set`'s own: a stroke is offered each, a fill each but satin
/// and a running line (the project owner's choice: a fill has no width to sew across).
#[test]
fn the_stitch_choices_are_the_engines() {
    let Some(SettingField::Choice(all)) = part_field("stitch") else { panic!("a choice") };
    let (mut app, _, b) = app_with_art();
    let info = app.session.embroidery_merged(&[vectorcraft_doc::NodeId(b)]).unwrap();
    let ids = |part: Part| {
        let p = part.of(&info).unwrap();
        match choice_of(p, part, "stitch", &p.settings["stitch"]) {
            OwnedField::Choice { options, .. } => options.into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
            _ => panic!("a choice"),
        }
    };
    assert_eq!(ids(Part::Stroke), all.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let fill = ids(Part::Fill);
    assert!(!fill.iter().any(|id| id == "satin"), "satin isn't offered for a fill: {fill:?}");
    assert!(["auto", "tatami", "none"].iter().all(|id| fill.iter().any(|f| f == id)), "{fill:?}");
}

/// An id this version doesn't know, or a file's design name, is shown cut to a named length with
/// "…": a hostile file can hold any string, laid out on every frame.
#[test]
fn an_unknown_id_is_shown_cut_short() {
    let long = "x".repeat(10_000);
    for words in [vocabulary::stitch(&long, false), vocabulary::underlay(&long), vocabulary::fabric(&long), vocabulary::source(&long, None)] {
        assert_eq!(words.chars().count(), vocabulary::MAX_SHOWN + 1, "cut, with its ellipsis");
        assert!(words.ends_with('…'));
    }
    assert_eq!(vocabulary::stitch("tatamiX", false), "tatamiX", "a short one whole");
}

/// A symbol's instance takes only Trim After (its art takes the settings, the project owner's
/// choice): the tab says why and offers the cut alone.
#[test]
fn a_symbol_instance_takes_only_trim_after() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    select(&mut app, &[a]);
    let instance = app.run("symbol.new", json!({"name": "Square"})).unwrap()["id"].as_u64().unwrap();
    select(&mut app, &[instance]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    assert!(drawn.has(vocabulary::takes_none("symbolInstance")), "{:?}", drawn.words);
    let trim = crate::etn!(1, "Trim the thread after the object", "Trim the thread after the objects");
    assert!(drawn.nodes.iter().any(|(_, n)| n.label() == Some(trim.as_str())), "Trim After offered");
    assert!(!drawn.has(vocabulary::label("spacing", Some("tatami"))), "no settings rows: {:?}", drawn.words);
}

/// Where the panel is too narrow for the hint after ↺ ("default 135°"), it moves into ↺'s tooltip
/// (one of the choices taken under the owner's "accept the rest"), which screen readers hear as
/// ↺'s description; where there is room it shows after ↺.
#[test]
fn the_hint_moves_into_restores_tooltip_where_the_panel_is_narrow() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30})).unwrap();
    select(&mut app, &[a]);
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", vocabulary::label("angle", None))]);
    let at = |app: &mut VectorcraftApp, width: f32, time: &mut f64| {
        let mut drawn = frame_in(app, &ctx, *time, vec![], width);
        for _ in 0..4 {
            *time += 0.05;
            drawn = frame_in(app, &ctx, *time, vec![], width);
        }
        drawn
    };
    let mut time = 0.0;
    let wide = at(&mut app, 480.0, &mut time);
    let hint = wide.words.iter().find(|(w, _)| w.starts_with("default ")).map(|(w, _)| w.clone()).expect("the hint after ↺ where there is room");
    assert_eq!(wide.node(&restore).description(), None, "shown, so not in ↺'s description");
    let narrow = at(&mut app, 240.0, &mut time);
    assert!(!narrow.words.iter().any(|(w, _)| *w == hint), "not after ↺ where there is no room");
    assert_eq!(narrow.node(&restore).description(), Some(hint.as_str()), "in ↺'s tooltip and description");
}

/// A millimetre field scrubs in tenths of a millimetre (two points of drag a step, as the angle's
/// degrees), and ↑ in a focused field steps it once, one undo step.
#[test]
fn a_millimetre_field_steps_in_tenths() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "spacing": 0.4, "angle": 30})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let at = words_at(&ready(&mut app, &ctx, &mut time), "Row spacing");
    // Moves past egui's drag threshold in the first, as the angle's scrub test drags.
    drag(&mut app, &ctx, &mut time, at, 40.0, false, a);
    let spacing = fill_setting(&mut app, a, "spacing").as_f64().unwrap();
    assert!((spacing - 2.4).abs() < 1e-9, "twenty steps of a tenth: {spacing}");
    let before = steps(&app);
    let drawn = ready(&mut app, &ctx, &mut time);
    let field = field_beside(&drawn, vocabulary::label("angle", None));
    let button = |pressed| Event::PointerButton { pos: field, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let up = Event::Key { key: Key::ArrowUp, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    for events in [vec![Event::PointerMoved(field), button(true)], vec![button(false)], vec![up]] {
        time += 0.05;
        frame(&mut app, &ctx, time, events);
    }
    ready(&mut app, &ctx, &mut time);
    assert_eq!((fill_setting(&mut app, a, "angle"), steps(&app)), (json!(31.0), before + 1), "a degree up, one undo step");
}

/// The entry and exit rows: a set entry reads "Set", marked, ↺ unsetting it alone, one undo step;
/// an exit left to the planner reads "Auto" with "nearest". One object's rows are buttons, opening
/// the point's dialog (`entry_and_exit_rows_open_the_points_dialog`).
#[test]
fn the_entry_and_exit_rows_and_their_restore() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30})).unwrap();
    app.run("embroidery.setEntryExit", json!({"id": a, "part": "fill", "entry": [30, 30]})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let entry = vocabulary::label("entry", None);
    let set = text::fill(crate::et!("{section}: {setting}"), &[("section", entry), ("setting", crate::et!("Set"))]);
    // One object's row opens the point's dialog: screen readers hear a button named with its words.
    assert_eq!(drawn.node(&set).role(), egui::accesskit::Role::Button, "{set}");
    assert!(drawn.has(crate::et!("nearest")), "the exit's Auto: {:?}", drawn.words);
    let before = steps(&app);
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", entry)]);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(&restore)]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    let fill = info(&mut app, &[a])["objects"][0]["settings"]["fill"].clone();
    assert_eq!((fill["entry"].clone(), fill["angle"].clone(), steps(&app)), (json!(null), json!(30.0), before + 1), "the entry alone, one step");
}

/// One object's entry and exit rows open the point's dialog: a set entry at its point, ticked set;
/// an exit left to the planner where the plan shown ends the part, Auto. Several objects' rows are
/// read only (a label, its words its value). Set on Canvas picks the embroidery tool.
#[test]
fn entry_and_exit_rows_open_the_points_dialog() {
    let ctx = context();
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.setEntryExit", json!({"id": a, "part": "fill", "entry": [30, 30]})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let entry = vocabulary::label("entry", None);
    let exit = vocabulary::label("exit", None);
    let named = |section: &str, setting: &str| text::fill(crate::et!("{section}: {setting}"), &[("section", section), ("setting", setting)]);
    let set = named(entry, crate::et!("Set"));
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(&set)]);
    time += 0.1;
    let d = app.embroidery.point_dialog.clone().expect("the entry's dialog");
    let opened = crate::embroidery::point_dialog::PointDialog::new("entry", a, "fill", 30.0, 30.0, false);
    assert_eq!(d, opened, "on the entry, set");
    app.embroidery.point_dialog = None;
    let drawn = ready(&mut app, &ctx, &mut time);
    let auto = drawn.nodes.iter().filter_map(|(_, n)| n.label()).find(|l| l.starts_with(&named(exit, ""))).expect("the exit's row").to_string();
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(&auto)]);
    time += 0.1;
    let d = app.embroidery.point_dialog.clone().expect("the exit's dialog");
    let scene = app.embroidery.preview.shown(app.session.active().unwrap().uid).unwrap().planned.built_scene().unwrap();
    let last = scene.units().iter().rfind(|u| u.object == vectorcraft_doc::NodeId(a)).and_then(|u| u.exit).unwrap();
    assert_eq!(d, crate::embroidery::point_dialog::PointDialog::new("exit", a, "fill", last.x, last.y, true), "where the plan ends it, Auto");
    app.embroidery.point_dialog = None;
    // Set on Canvas: the embroidery tool.
    let drawn = ready(&mut app, &ctx, &mut time);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(crate::et!("Set on Canvas"))]);
    time += 0.1;
    assert_eq!(app.session.tool_id(), vectorcraft_tools::embroidery::ID);
    // Several objects: read only, nothing opens.
    app.run("embroidery.setEntryExit", json!({"id": b, "part": "fill", "entry": [160, 30]})).unwrap();
    select(&mut app, &[a, b]);
    let drawn = ready(&mut app, &ctx, &mut time);
    let of_entry = named(entry, "");
    let read_only = |n: &egui::accesskit::Node| n.role() == egui::accesskit::Role::Label && n.value().is_some_and(|v| v.starts_with(&of_entry));
    assert!(drawn.nodes.iter().any(|(_, n)| read_only(n)), "the entry's words as a label's value");
    assert!(
        !drawn.nodes.iter().any(|(_, n)| n.role() == egui::accesskit::Role::Button && n.label().is_some_and(|l| l.starts_with(&of_entry))),
        "no button"
    );
}

/// A point the stitches miss by more than the Check tab lets pass says by how much, from the plan
/// shown; one they meet says nothing: an exit far off the rectangle, an entry on its side.
#[test]
fn a_missed_point_says_by_how_much() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    // The rectangle's corner nearest the exit, (100, 70), lies 304.8 pt (107.5 mm) from it.
    app.run("embroidery.setEntryExit", json!({"id": a, "part": "fill", "entry": [20, 45], "exit": [300, 300]})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let (before, after) = crate::et!("missed by {value} mm").split_once("{value}").unwrap();
    let misses: Vec<f64> = drawn.words.iter().filter_map(|(w, _)| w.strip_prefix(before)?.strip_suffix(after)?.parse().ok()).collect();
    assert_eq!(misses.len(), 1, "the exit alone: {:?}", drawn.words);
    assert!((misses[0] - 107.5).abs() < 1.0, "{misses:?}");
}

/// A pull whose two sides differ shows both in its blank field ("0.2 mm / 0.3 mm"), which screen
/// readers hear as its placeholder.
#[test]
fn a_two_sided_pull_shows_both_sides() {
    let ctx = context();
    let (mut app, a, _) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "pull": [0.2, 0.3]})).unwrap();
    select(&mut app, &[a]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    let pull = drawn.node(vocabulary::label("pull", None));
    assert_eq!(pull.placeholder(), Some("0.2 mm / 0.3 mm"));
}

/// A click on the part switch shows that part; Restore from Fabric unsets a stroke's own values
/// too.
#[test]
fn the_part_switch_and_restore_from_fabric_on_a_stroke() {
    let ctx = context();
    let (mut app, _, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [b], "part": "stroke", "stitch": "satin", "pull": 0.3})).unwrap();
    select(&mut app, &[b]);
    let mut time = 0.0;
    let drawn = ready(&mut app, &ctx, &mut time);
    assert_eq!(app.ui.embroidery_panel.part, Part::Fill);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click(crate::et!("Stroke"))]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    assert_eq!((app.ui.embroidery_panel.part, inspect(&app)["part"].clone()), (Part::Stroke, json!("stroke")));
    let drawn = ready(&mut app, &ctx, &mut time);
    frame(&mut app, &ctx, time + 0.05, vec![drawn.click("Restore from Fabric")]);
    time += 0.1;
    ready(&mut app, &ctx, &mut time);
    let stroke = info(&mut app, &[b])["objects"][0]["settings"]["stroke"].clone();
    assert_eq!((stroke["stitch"].clone(), stroke["pull"].clone()), (json!(null), json!(null)), "{stroke}");
}

/// Copy is a snapshot: Paste still gives the settings after the object copied is deleted.
#[test]
fn paste_after_the_copied_object_is_deleted() {
    let (mut app, a, b) = app_with_art();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30})).unwrap();
    select(&mut app, &[a]);
    app.run("embroidery.copySettings", json!({})).unwrap();
    app.run("edit.clear", json!({})).unwrap();
    select(&mut app, &[b]);
    app.run("embroidery.pasteSettings", json!({})).unwrap();
    assert_eq!(fill_setting(&mut app, b, "angle"), json!(30.0));
}
