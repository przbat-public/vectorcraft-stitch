//! The image window on a live object: Keep as Live Object, a live object opened again with its
//! settings (its threads let go when the document's palette changed since; the defaults, said, when
//! they can't be read), remembered angles turning with the image, the question before objects
//! changed by hand are replaced (its arming, its keyboard and what screen readers hear), and agents
//! keeping one with `ui.dialog.set {keep: "live"}`.

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
const QUESTION: &str = "Its objects were changed by hand. Digitizing it again replaces those changes; Undo brings them back.";
const DONT_REPLACE: &str = "Don't Replace";
const REPLACE: &str = "Replace";

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

/// Keep as Live Object or Create on a live object whose objects were changed by hand asks first, in
/// the Live bar: Don't Replace takes the keyboard, screen readers hear the question on its buttons,
/// Don't Replace and Escape keep the window open with nothing made (the keyboard back on the button
/// that asked), and Replace makes it.
#[test]
fn replacing_hand_edits_is_asked_first() {
    let (mut app, ctx, mut time, id) = edited_live_object_open();
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, "Keep as Live Object", Role::Button);
    assert!(asking(&app) && open_dialog(&app).is_some() && undo_steps(&app) == steps, "asked, nothing made");
    assert_eq!(focused(&mut app, &ctx, &mut time).as_deref(), Some(DONT_REPLACE), "the safe answer has the keyboard");
    let all = nodes(&mut app, &ctx, &mut time);
    for name in [REPLACE, DONT_REPLACE] {
        assert_eq!(node(&all, name, Role::Button).and_then(|n| n.description()), Some(QUESTION), "{name}");
    }
    until(&mut app, &ctx, &mut time, armed);
    press(&mut app, &ctx, &mut time, DONT_REPLACE, Role::Button);
    assert!(!asking(&app) && open_dialog(&app).is_some(), "Don't Replace keeps the window");
    assert_eq!(focused(&mut app, &ctx, &mut time).as_deref(), Some("Keep as Live Object"), "the keyboard back where it asked");
    // Create (Cmd/Ctrl+Enter) asks too; Escape answers the question, not the window.
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(asking(&app), "Create asks too");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![let_go(Key::Enter, Modifiers::COMMAND), key(Key::Escape, Modifiers::NONE)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(!asking(&app) && open_dialog(&app).is_some(), "Escape leaves the question, the window open");
    assert_eq!(undo_steps(&app), steps, "nothing made");
    press(&mut app, &ctx, &mut time, "Keep as Live Object", Role::Button);
    until(&mut app, &ctx, &mut time, armed);
    press(&mut app, &ctx, &mut time, REPLACE, Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1, "made");
    assert_eq!(selected(&app), id, "the live object kept its id");
    let edited = app.session.doc().unwrap().doc.node(id).and_then(vectorcraft_engine::cmd::embroidery::live_edited);
    assert_eq!(edited, Some(false), "its objects as made");
}

/// The question answers neither the second click of a double-click on the button that asked (its
/// Replace lies where Create was) nor a key held from asking: only once the double-click time has
/// passed with nothing held. Cmd/Ctrl+Enter while it shows doesn't turn Keep as Live Object's
/// question into Create's.
#[test]
fn the_question_answers_neither_a_double_click_nor_a_held_key() {
    let (mut app, ctx, mut time, _) = edited_live_object_open();
    let steps = undo_steps(&app);
    press(&mut app, &ctx, &mut time, "Keep as Live Object", Role::Button);
    // The next click at once, as the second of a double-click: answers nothing.
    press(&mut app, &ctx, &mut time, REPLACE, Role::Button);
    assert!(asking(&app) && undo_steps(&app) == steps, "a double-click's second click answers nothing");
    // Cmd/Ctrl+Enter while it shows asks nothing anew.
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert_eq!(live(&app).question.map(|q| q.making), Some(super::Making::LiveObject), "still Keep as Live Object's");
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

/// Agents keep a live object as they create: `ui.dialog.set {keep: "live"}` (taken though the
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
