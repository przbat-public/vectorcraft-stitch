//! The image window on a live object: Keep as Live Object, a live object opened again with its
//! settings (its threads let go when the document's palette changed since; the defaults, said, when
//! they can't be read), remembered angles turning with the image, the question before objects
//! changed by hand are replaced (its arming, its keyboard and what screen readers hear), and agents
//! keeping one with `ui.dialog.set {field: "keep", value: "live"}`.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, Rect, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::doc::NodeId;
use vectorcraft_geom::Affine;

use super::tests::{app, frame, key, live, node, nodes, open_dialog, opened, opened_with, press, undo_steps, until};
use crate::VectorcraftApp;

/// The image window's orange, as the engine writes a colour.
const ORANGE: &str = "#f08214";

/// The question's words, its safe answer and its other one.
const QUESTION: &str = "The contents of this live object were changed by hand. Digitizing it again replaces those changes; Undo brings them back.";
const DONT_REPLACE: &str = "Don't Replace";
const REPLACE: &str = "Replace";

/// The coral and the grey button on a live object opened again.
const UPDATE: &str = "Update Live Object";
const CONVERT: &str = "Convert to Plain Objects";

/// The one object selected.
fn selected(app: &VectorcraftApp) -> NodeId {
    let st = app.session.doc().expect("a document");
    assert_eq!(st.selection.objects.len(), 1, "one object selected");
    st.selection.objects[0]
}

/// Whether `id` is a live object of a digitized image.
fn is_live(app: &VectorcraftApp, id: NodeId) -> bool {
    app.session.doc().unwrap().doc.node(id).is_some_and(|n| vectorcraft_engine::cmd::embroidery::live_digitizing(n).is_some())
}

/// The test picture kept as a live object digitized with `params`, selected → the app and its id.
fn live_object(params: Value) -> (VectorcraftApp, NodeId) {
    let mut app = app();
    let mut p = params;
    p["live"] = json!(true);
    let reply = app.run("embroidery.digitizeImage", p).unwrap();
    let id = NodeId(reply["created"]["group"].as_u64().expect("made"));
    app.run("select.set", json!({"ids": [id.0]})).unwrap();
    (app, id)
}

/// A live object of the test picture whose first object was moved by hand, selected, and the
/// window open on it, its first result shown.
fn edited_live_object_open() -> (VectorcraftApp, egui::Context, f64, NodeId) {
    let (mut app, id) = live_object(json!({"colors": 3}));
    let child = {
        let doc = &app.session.doc().unwrap().doc;
        let blocks = doc.node(id).and_then(|n| n.children()).expect("its children");
        blocks.get(1).and_then(|b| b.children()).and_then(|c| c.first()).map(|o| o.id).expect("an object")
    };
    app.run("select.set", json!({"ids": [child.0]})).unwrap();
    app.run("object.move", json!({"dx": 5, "dy": 0})).unwrap();
    app.run("select.set", json!({"ids": [id.0]})).unwrap();
    let (app, ctx, time) = opened_with(app);
    (app, ctx, time, id)
}

/// Field `key` of the window `d` (null when it doesn't hold it).
fn field(d: &crate::state::Dialog, key: &str) -> Value {
    d.fields.get(key).cloned().unwrap_or(Value::Null)
}

/// `key` let go (egui holds a key down from its press until then).
fn let_go(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }
}

/// Whether the window's Live bar asks before replacing objects changed by hand.
fn asking(app: &VectorcraftApp) -> bool {
    live(app).question.is_some()
}

/// Whether the question's buttons answer yet.
fn armed(app: &VectorcraftApp) -> bool {
    live(app).question.is_some_and(|q| q.armed)
}

/// One frame with screen readers on → the name of the control that has the keyboard.
fn focused(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Option<String> {
    *time += 0.05;
    let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
    let update = out.platform_output.accesskit_update?;
    update.nodes.iter().find(|(id, _)| *id == update.focus).and_then(|(_, n)| n.label().map(str::to_string))
}

/// Keep as Live Object makes what Create would as a live object, in one undo step, selected, and
/// says so; the window closes.
#[test]
fn keep_as_live_object_from_the_window() {
    let (mut app, ctx, mut time) = opened();
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, "Keep as Live Object", Role::Button);
    assert!(open_dialog(&app).is_none(), "closed");
    assert_eq!(undo_steps(&app), steps + 1, "one undo step");
    assert!(is_live(&app, selected(&app)), "the live object selected");
    let status = app.ui.status.clone();
    assert!(status.starts_with("Created a live object: ") && status.ends_with("Undo removes it."), "{status}");
}

/// Digitize Image… on a live object opens the window with the settings it was made with (its area
/// changes too; its size its image's, though it was made at a width given), unless the window was
/// left on it in this session.
#[test]
fn the_window_opens_a_live_object_with_its_settings() {
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "stitch": "none"}]);
    let (mut app, id) = live_object(json!({"colors": 3, "width": 30, "areaChanges": changes}));
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("the window opens");
    assert_eq!((field(d, "id").as_u64(), field(d, "colors").as_u64()), (Some(id.0), Some(3)));
    assert_eq!(field(d, "areaChanges")[0]["stitch"], json!("none"), "{}", field(d, "areaChanges"));
    assert!(["width", "height", "fit"].iter().all(|k| d.fields.get(*k).is_none()), "the size its image's: {:?}", d.fields);
    // Left with four colours, it opens again with four in this session.
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, 0.0, vec![]);
    app.ui.dialog.as_mut().unwrap().fields.insert("colors".into(), json!(4));
    frame(&mut app, &ctx, 0.05, vec![]);
    frame(&mut app, &ctx, 0.1, vec![key(Key::Escape, Modifiers::NONE)]);
    frame(&mut app, &ctx, 0.15, vec![]);
    assert!(open_dialog(&app).is_none(), "closed");
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert_eq!(field(open_dialog(&app).expect("open again"), "colors"), json!(4), "as it was left");
}

/// The window opened on the image a live object holds is the live object's: keyed on it, with the
/// settings it was made with, as the command takes that image.
#[test]
fn the_window_opened_on_a_live_objects_image_is_the_live_objects() {
    let (mut app, id) = live_object(json!({"colors": 3, "width": 30}));
    let image = app.session.doc().ok().and_then(|st| st.doc.node(id).and_then(|n| n.children()?.first().map(|c| c.id))).expect("its image");
    super::open(&mut app, Some(image));
    let d = open_dialog(&app).expect("the window opens");
    assert_eq!((field(d, "id").as_u64(), field(d, "colors").as_u64()), (Some(id.0), Some(3)), "{:?}", d.fields);
}

/// A live object made in another thread palette than the document's now opens with its settings,
/// its threads let go (the colours matched to the document's palette again), and says so.
#[test]
fn a_live_object_made_in_another_palette_opens_with_its_threads_let_go() {
    let choices = json!([{"color": ORANGE, "thread": {"palette": "jef", "index": 1}}]);
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "stitch": "none"}]);
    let (mut app, _) = live_object(json!({"colors": 3, "colorChoices": choices, "areaChanges": changes}));
    app.run("embroidery.setup", json!({"palette": "pec"})).unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("the window opens");
    assert_eq!((d.str("palette"), field(d, "colors").as_u64()), ("pec".to_string(), Some(3)));
    assert_eq!(field(d, "areaChanges")[0]["stitch"], json!("none"), "the rest kept");
    assert!(field(d, "colorChoices").as_array().is_none_or(|c| c.iter().all(|c| c.get("thread").is_none())), "{:?}", d.fields);
    assert_eq!(app.ui.status, "This live object was made in another thread palette: its colors are matched to the document's palette again.");
}

/// Settings of a live object the engine refuses (a newer version's, or a file edited by hand): the
/// window opens with the defaults and says so.
#[test]
fn a_live_object_with_refused_settings_opens_with_the_defaults() {
    let (mut app, id) = live_object(json!({"colors": 3}));
    app.session
        .edit("unreadable", |d, _| {
            let node = d.node_mut(id).expect("the live object");
            let mut extras = node.extras_mut();
            let digitizing = extras.embroidery.as_mut().and_then(|e| e.digitizing.as_mut()).expect("its digitizing");
            let mut settings = (*digitizing.settings).clone();
            settings.insert("colors".into(), json!("many"));
            digitizing.settings = settings.into();
            Ok(())
        })
        .unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("the window opens");
    assert!(d.fields.get("colors").is_none(), "the defaults");
    assert_eq!(app.ui.status, "This live object's settings couldn't be read: the window starts from the defaults.");
}

/// A window left on an image remembers its change's angle on the page as the image lay then: the
/// image turned on the canvas since, the angle turns with it, the same to the art.
#[test]
fn a_remembered_windows_angles_turn_with_its_image() {
    let ctx = egui::Context::default();
    let mut app = app();
    let image = selected(&app);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![]);
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "angle": 30.0}]);
    app.ui.dialog.as_mut().unwrap().fields.insert("areaChanges".into(), changes);
    frame(&mut app, &ctx, 0.05, vec![]);
    frame(&mut app, &ctx, 0.1, vec![key(Key::Escape, Modifiers::NONE)]);
    frame(&mut app, &ctx, 0.15, vec![]);
    app.session
        .edit("turn", |d, _| {
            d.node_mut(image).expect("the image").transform(Affine::rotate(std::f64::consts::FRAC_PI_2), true);
            Ok(())
        })
        .unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("open again");
    let angle = field(d, "areaChanges")[0]["angle"].as_f64().expect("its angle");
    let off = (angle - 120.0).rem_euclid(180.0);
    assert!(!(0.01..=179.99).contains(&off), "turned with the image: {angle}");
    assert!(d.fields.get("__placed").is_none(), "no field of its own: {:?}", d.fields);
}

/// A window closed by another document becoming active remembers its image's placement as its own
/// document has it: the image turned there after, the change's angle turns with it.
#[test]
fn a_window_left_for_another_document_remembers_its_images_placement() {
    let ctx = egui::Context::default();
    let mut app = app();
    let image = selected(&app);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![]);
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "angle": 30.0}]);
    app.ui.dialog.as_mut().unwrap().fields.insert("areaChanges".into(), changes);
    frame(&mut app, &ctx, 0.05, vec![]);
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    frame(&mut app, &ctx, 0.1, vec![]);
    frame(&mut app, &ctx, 0.15, vec![]);
    assert!(open_dialog(&app).is_none() && app.embroidery.image_window.live.is_none(), "closed with its document");
    assert!(app.session.set_active(0), "back to the first document");
    app.session
        .edit("turn", |d, _| {
            d.node_mut(image).expect("the image").transform(Affine::rotate(std::f64::consts::FRAC_PI_2), true);
            Ok(())
        })
        .unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("open again");
    let angle = field(d, "areaChanges")[0]["angle"].as_f64().expect("its angle");
    let off = (angle - 120.0).rem_euclid(180.0);
    assert!(!(0.01..=179.99).contains(&off), "turned with the image: {angle}");
}

/// Update Live Object or Convert to Plain Objects on a live object whose objects were changed by
/// hand asks first, in the Live bar: Don't Replace takes the keyboard, screen readers hear the
/// question on its buttons, Don't Replace and Escape keep the window open with nothing made (the
/// keyboard back on the button that asked), and Replace makes it, saying so: Undo brings the hand
/// edits back.
#[test]
fn replacing_hand_edits_is_asked_first() {
    let (mut app, ctx, mut time, id) = edited_live_object_open();
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    assert!(asking(&app) && open_dialog(&app).is_some() && undo_steps(&app) == steps, "asked, nothing made");
    assert_eq!(focused(&mut app, &ctx, &mut time).as_deref(), Some(DONT_REPLACE), "the safe answer has the keyboard");
    let all = nodes(&mut app, &ctx, &mut time);
    for name in [REPLACE, DONT_REPLACE] {
        assert_eq!(node(&all, name, Role::Button).and_then(|n| n.description()), Some(QUESTION), "{name}");
    }
    until(&mut app, &ctx, &mut time, armed);
    press(&mut app, &ctx, &mut time, DONT_REPLACE, Role::Button);
    assert!(!asking(&app) && open_dialog(&app).is_some(), "Don't Replace keeps the window");
    assert_eq!(focused(&mut app, &ctx, &mut time).as_deref(), Some(UPDATE), "the keyboard back where it asked");
    // Convert to Plain Objects asks too, its Replace converting; Escape answers the question, not
    // the window.
    press(&mut app, &ctx, &mut time, CONVERT, Role::Button);
    assert_eq!(live(&app).question.map(|q| q.making), Some(super::Making::Objects), "Convert asks too");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape, Modifiers::NONE)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(!asking(&app), "Escape answers it");
    // Cmd/Ctrl+Enter is the coral button's: Update.
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert_eq!(live(&app).question.map(|q| q.making), Some(super::Making::LiveObject), "Cmd/Ctrl+Enter asks as Update");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![let_go(Key::Enter, Modifiers::COMMAND), key(Key::Escape, Modifiers::NONE)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(!asking(&app) && open_dialog(&app).is_some(), "Escape leaves the question, the window open");
    assert_eq!(undo_steps(&app), steps, "nothing made");
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    until(&mut app, &ctx, &mut time, armed);
    press(&mut app, &ctx, &mut time, REPLACE, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1, "made");
    assert_eq!(selected(&app), id, "the live object kept its id");
    let edited = |app: &VectorcraftApp| app.session.doc().unwrap().doc.node(id).and_then(vectorcraft_engine::cmd::embroidery::live_edited);
    assert_eq!(edited(&app), Some(false), "its objects as made");
    let status = app.ui.status.clone();
    assert!(status.starts_with("Updated the live object: ") && status.ends_with("Undo brings back the earlier version."), "{status}");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(edited(&app), Some(true), "Undo brings the hand edits back");
}

/// One frame's shapes, each group flattened into its parts, and its accessibility nodes.
fn drawn(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> (Vec<egui::Shape>, Vec<(egui::accesskit::NodeId, egui::accesskit::Node)>) {
    fn flatten(s: egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    *time += 0.05;
    let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut shapes = Vec::new();
    out.shapes.into_iter().for_each(|c| flatten(c.shape, &mut shapes));
    (shapes, out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default())
}

/// The box of the button screen readers call `name`.
fn bounds(all: &[(egui::accesskit::NodeId, egui::accesskit::Node)], name: &str) -> Rect {
    let b = node(all, name, Role::Button).and_then(|n| n.bounds()).unwrap_or_else(|| panic!("{name} drawn"));
    Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32))
}

/// Whether a button at `r` is filled coral: a box filled so about where it lies (its centre within
/// a point, rounding of a box drawn on whole points).
fn coral(shapes: &[egui::Shape], ctx: &egui::Context, r: Rect) -> bool {
    const NEAR: f32 = 1.0;
    let t = crate::theme::Tokens::get(ctx);
    shapes.iter().any(|s| matches!(s, egui::Shape::Rect(b) if b.fill == t.accent_strong && b.rect.center().distance(r.center()) < NEAR))
}

/// Whether the keyboard's ring is drawn round `r`, as the panels draw it on the window's fill.
fn ringed(shapes: &[egui::Shape], ctx: &egui::Context, r: Rect) -> bool {
    let t = crate::theme::Tokens::get(ctx);
    let ring = crate::embroidery::rows::focus_colour(&t, t.panel);
    shapes.iter().any(|s| matches!(s, egui::Shape::Rect(b) if b.stroke.color == ring && b.stroke.width > 0.0 && b.rect.contains_rect(r)))
}

/// The question's safe answer, Don't Replace, is the coral button, in the coral button's place at
/// the bar's right and at least as wide, and shows the keyboard with the panels' focus ring; Replace
/// is grey, unringed. Answered, the keyboard and its ring go back to the button that asked.
#[test]
fn the_questions_safe_answer_is_coral_and_shows_the_keyboard() {
    let (mut app, ctx, mut time, _) = edited_live_object_open();
    let (_, all) = drawn(&mut app, &ctx, &mut time);
    let update = bounds(&all, UPDATE);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    assert!(asking(&app), "asked");
    let (shapes, all) = drawn(&mut app, &ctx, &mut time);
    let (keep, replace) = (bounds(&all, DONT_REPLACE), bounds(&all, REPLACE));
    assert!(coral(&shapes, &ctx, keep), "Don't Replace coral");
    assert!(!coral(&shapes, &ctx, replace), "Replace grey");
    assert!(keep.left() > replace.right(), "Don't Replace at the right, the coral button's place");
    // Whole points either way.
    const ROUNDING: f32 = 0.5;
    assert!(keep.width() >= update.width() - ROUNDING, "as wide as the coral button at least: {keep:?} for {update:?}");
    assert!(ringed(&shapes, &ctx, keep), "the keyboard's ring round Don't Replace");
    assert!(!ringed(&shapes, &ctx, replace), "none round Replace");
    until(&mut app, &ctx, &mut time, armed);
    press(&mut app, &ctx, &mut time, DONT_REPLACE, Role::Button);
    assert!(!asking(&app), "answered");
    let (shapes, all) = drawn(&mut app, &ctx, &mut time);
    assert!(ringed(&shapes, &ctx, bounds(&all, UPDATE)), "the ring back round Update Live Object");
    assert!(!ringed(&shapes, &ctx, bounds(&all, CONVERT)), "none round Convert to Plain Objects");
}

/// A picture that comes to lie first in a live object while the window is open on it (Redo of Keep
/// as Live Object from the menu bar, which works over the window) has the window digitize that live
/// object: it offers Update Live Object and Convert to Plain Objects, and asks before replacing
/// objects changed by hand. Both read the picture alone, a plain image: Create, and no question.
#[test]
fn a_picture_that_comes_to_lie_in_a_live_object_is_digitized_as_it() {
    let mut app = app();
    let picture = app.session.doc().ok().and_then(|st| st.selection.objects.first().copied()).expect("the picture selected");
    let reply = app.run("embroidery.digitizeImage", json!({"colors": 3, "live": true})).unwrap();
    let id = NodeId(reply["created"]["group"].as_u64().expect("made"));
    let first =
        |app: &VectorcraftApp| app.session.doc().ok().and_then(|st| st.doc.node(id)).and_then(|n| n.children()).and_then(|c| c.first()).map(|c| c.id);
    assert_eq!(first(&app), Some(picture), "the live object holds the picture itself");
    app.run("edit.undo", json!({})).unwrap();
    app.run("select.set", json!({"ids": [picture.0]})).unwrap();
    let (mut app, ctx, mut time) = opened_with(app);
    assert!(node(&nodes(&mut app, &ctx, &mut time), "Create Embroidery Objects", Role::Button).is_some(), "a plain picture's");
    app.run("edit.redo", json!({})).unwrap();
    assert_eq!(first(&app), Some(picture), "in the live object again");
    until(&mut app, &ctx, &mut time, super::tests::ready);
    let all = nodes(&mut app, &ctx, &mut time);
    assert!(node(&all, UPDATE, Role::Button).is_some() && node(&all, CONVERT, Role::Button).is_some(), "the live object's");
    // An object of it moved by hand: Update asks first.
    let child = {
        let doc = &app.session.doc().unwrap().doc;
        let blocks = doc.node(id).and_then(|n| n.children()).expect("its children");
        blocks.get(1).and_then(|b| b.children()).and_then(|c| c.first()).map(|o| o.id).expect("an object")
    };
    app.run("select.set", json!({"ids": [child.0]})).unwrap();
    app.run("object.move", json!({"dx": 5, "dy": 0})).unwrap();
    until(&mut app, &ctx, &mut time, super::tests::ready);
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    // Read without `asking`, which needs the window open: replacing them unasked closed it.
    let asked = app.embroidery.image_window.live.as_ref().is_some_and(|l| l.question.is_some());
    assert!(asked && undo_steps(&app) == steps, "asked first, nothing made");
}

/// On a live object opened again the coral button is Update Live Object and the grey one Convert to
/// Plain Objects (the project owner's choice, 2026-11-18); Update digitizes it again in place,
/// keeping it live, and says Undo brings back the earlier version.
#[test]
fn a_live_object_opened_again_offers_update_and_convert() {
    let (app, id) = live_object(json!({"colors": 3}));
    let (mut app, ctx, mut time) = opened_with(app);
    let all = nodes(&mut app, &ctx, &mut time);
    for (name, shown) in [(UPDATE, true), (CONVERT, true), ("Create Embroidery Objects", false), ("Keep as Live Object", false)] {
        assert_eq!(node(&all, name, Role::Button).is_some(), shown, "{name}");
    }
    let (shapes, all) = drawn(&mut app, &ctx, &mut time);
    assert!(coral(&shapes, &ctx, bounds(&all, UPDATE)), "Update Live Object coral");
    assert!(!coral(&shapes, &ctx, bounds(&all, CONVERT)), "Convert to Plain Objects grey");
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1, "one undo step");
    assert!(is_live(&app, id) && selected(&app) == id, "still live, its id kept");
    let status = app.ui.status.clone();
    assert!(status.starts_with("Updated the live object: ") && status.ends_with("Undo brings back the earlier version."), "{status}");
}

/// Convert to Plain Objects on a live object opened again makes plain objects in its place, saying
/// Undo brings the live object back, which Undo does.
#[test]
fn converting_a_live_object_says_undo_brings_it_back() {
    let (app, id) = live_object(json!({"colors": 3}));
    let (mut app, ctx, mut time) = opened_with(app);
    press(&mut app, &ctx, &mut time, CONVERT, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert!(app.session.doc().unwrap().doc.node(id).is_none(), "the live object gone");
    assert!(!is_live(&app, selected(&app)), "plain objects selected");
    let status = app.ui.status.clone();
    assert!(status.starts_with("Converted to plain objects: ") && status.ends_with("Undo brings the live object back."), "{status}");
    app.run("edit.undo", json!({})).unwrap();
    assert!(is_live(&app, id), "Undo brings it back");
}

/// The window opened on a live object names it by its picture, as the Object tab and the Check tab
/// do: its group's own name, upstream's mark of a trace, is "Image Trace" for every one.
#[test]
fn the_window_names_the_picture_a_live_object_holds() {
    let (mut app, _) = live_object(json!({"colors": 3}));
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let title = super::words::title_facts(&app, open_dialog(&app).expect("the window opens"));
    assert!(title.as_deref().is_some_and(|t| t.starts_with("logo.png · ")), "{title:?}");
}

/// The question answers neither the second click of a double-click on the button that asked nor a
/// key held from asking: only once the double-click time has passed with no mouse button, Enter or
/// Space held. Cmd/Ctrl+Enter (Update's) while it shows doesn't turn Convert to Plain Objects'
/// question into Update's.
#[test]
fn the_question_answers_neither_a_double_click_nor_a_held_key() {
    let (mut app, ctx, mut time, _) = edited_live_object_open();
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, CONVERT, Role::Button);
    // The next click at once, as the second of a double-click: answers nothing.
    press(&mut app, &ctx, &mut time, REPLACE, Role::Button);
    assert!(asking(&app) && undo_steps(&app) == steps, "a double-click's second click answers nothing");
    // Cmd/Ctrl+Enter while it shows asks nothing anew.
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert_eq!(live(&app).question.map(|q| q.making), Some(super::Making::Objects), "still Convert's");
    // Enter held down: never armed, however long.
    let held = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![held]);
    for _ in 0..20 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert!(!armed(&app), "not while Enter is held");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![let_go(Key::Enter, Modifiers::NONE)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(armed(&app), "armed once let go");
    assert_eq!(undo_steps(&app), steps, "nothing made");
}

/// Agents keep a live object as they create: `ui.dialog.set {field: "keep", value: "live"}` (taken though the
/// window doesn't hold it yet, as every setting is), then `ui.dialog.confirm`; any other `keep` is
/// refused at confirm, nothing made, and a window closed without Create doesn't remember it.
#[test]
fn ui_dialog_set_keep_live_and_confirm_keep_a_live_object() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    crate::tests_synthetic::control(&mut app, &ctx, "ui.dialog.set", json!({"field": "colors", "value": 3}));
    crate::tests_synthetic::control(&mut app, &ctx, "ui.dialog.set", json!({"field": "keep", "value": "live"}));
    let reply = crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!((reply["created"]["live"].clone(), reply["settings"]["colors"].clone()), (json!(true), json!(3)), "{reply}");
    assert!(is_live(&app, selected(&app)));
    let mut app = super::tests::app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![]);
    crate::tests_synthetic::control(&mut app, &ctx, "ui.dialog.set", json!({"field": "keep", "value": "forever"}));
    frame(&mut app, &ctx, 0.05, vec![]);
    let steps = undo_steps(&app);
    assert!(crate::dialogs::confirm(&mut app).is_err(), "refused");
    assert_eq!(undo_steps(&app), steps, "nothing made");
    crate::dialogs::cancel(&mut app);
    frame(&mut app, &ctx, 0.1, vec![]);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert!(open_dialog(&app).expect("open again").fields.get("keep").is_none(), "not remembered");
}

/// The settings live object `id` keeps, as the window and the command take them.
fn kept_settings(app: &VectorcraftApp, id: NodeId) -> serde_json::Map<String, Value> {
    app.session.doc().unwrap().doc.node(id).and_then(vectorcraft_engine::cmd::embroidery::live_settings).expect("a live object")
}

/// Start Over on a live object's window, then Update Live Object, keeps none of the settings it
/// had kept: the window's own runs start from its fields alone, never from what the live object
/// keeps, so that no kept one comes back.
#[test]
fn start_over_then_update_keeps_none_of_its_settings() {
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "stitch": "none"}]);
    let (app, id) = live_object(json!({"colors": 3, "areaChanges": changes, "colorOutlines": true}));
    let (mut app, ctx, mut time) = opened_with(app);
    assert_eq!(field(open_dialog(&app).unwrap(), "areaChanges")[0]["stitch"], json!("none"), "opened with its change");
    super::start_over(&mut app);
    until(&mut app, &ctx, &mut time, super::tests::ready);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    let kept = kept_settings(&app, id);
    assert!(kept.get("areaChanges").is_none_or(|c| c.as_array().is_some_and(Vec::is_empty)), "no change: {kept:?}");
    assert_eq!(kept.get("colorOutlines"), Some(&json!(false)), "the default: {kept:?}");
}

/// Reset All on a live object's window, then Update Live Object, keeps none of the changes it had
/// kept, and its other settings as they were.
#[test]
fn reset_all_then_update_keeps_none_of_its_changes() {
    let changes = json!([{"at": [100.5, 20.5], "color": ORANGE, "kind": "fill", "stitch": "none"}]);
    let (app, id) = live_object(json!({"colors": 3, "areaChanges": changes, "colorOutlines": true}));
    let (mut app, ctx, mut time) = opened_with(app);
    super::tests::set(&mut app, super::STEP, json!(4));
    press(&mut app, &ctx, &mut time, "Reset All", Role::Button);
    assert_eq!(field(open_dialog(&app).unwrap(), "areaChanges"), Value::Null, "the changes reset");
    until(&mut app, &ctx, &mut time, super::tests::ready);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    let kept = kept_settings(&app, id);
    assert!(kept.get("areaChanges").is_none_or(|c| c.as_array().is_some_and(Vec::is_empty)), "no change: {kept:?}");
    assert_eq!((kept.get("colors"), kept.get("colorOutlines")), (Some(&json!(3)), Some(&json!(true))), "the rest as it was: {kept:?}");
}

/// A live object keeping no fingerprint (a file of an older version, or one edited by hand) is taken
/// as edited: Update Live Object on it asks before replacing its objects.
#[test]
fn a_live_object_keeping_no_fingerprint_is_asked_about() {
    let (mut app, id) = live_object(json!({"colors": 3}));
    app.session
        .edit("no fingerprint", |d, _| {
            let node = d.node_mut(id).expect("the live object");
            let mut extras = node.extras_mut();
            extras.embroidery.as_mut().and_then(|e| e.digitizing.as_mut()).expect("its digitizing").fingerprint = None;
            Ok(())
        })
        .unwrap();
    let (mut app, ctx, mut time) = opened_with(app);
    press(&mut app, &ctx, &mut time, UPDATE, Role::Button);
    assert!(asking(&app), "asked first");
}

/// Object › Image Trace › Make and Make and Expand are greyed out where what they would trace is a
/// live object, which Image Trace refuses: with the live object selected, or the image it holds
/// first, the engine refusing each as the menu greys it; on a plain image they aren't, and Digitize
/// Image… stays on.
#[test]
fn image_traces_make_is_greyed_on_a_live_object() {
    let (mut app, id) = live_object(json!({"colors": 3}));
    let image =
        app.session.doc().ok().and_then(|st| st.doc.node(id)).and_then(|n| n.children()).and_then(|c| c.first()).map(|c| c.id).expect("its image");
    for selected in [id, image] {
        app.run("select.set", json!({"ids": [selected.0]})).unwrap();
        for cmd in ["imageTrace.make", "imageTrace.makeAndExpand"] {
            assert!(!crate::menus::enabled(&app, cmd), "{cmd} on {selected:?}");
            let refused = app.run(cmd, json!({})).expect_err("refused");
            assert!(refused.contains(vectorcraft_engine::cmd::embroidery::LIVE_EMBROIDERY_REFUSED), "{cmd} on {selected:?}: {refused}");
        }
        assert!(crate::menus::enabled(&app, "embroidery.digitizeImage"), "Digitize Image… on {selected:?}");
    }
    let plain = super::tests::app();
    for cmd in ["imageTrace.make", "imageTrace.makeAndExpand"] {
        assert!(crate::menus::enabled(&plain, cmd), "{cmd} on a plain image");
    }
}
