//! The image window's flows through whole app frames: opening and refusals, the palette item and
//! the menu, Cmd/Ctrl+Enter, Escape remembering it for the session, Start Over, `ui.dialog.set`
//! and `ui.dialog.confirm`, closing with its document, and its size.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Rect, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::Session;

use super::*;
use crate::Services;

/// An app with a picture placed and selected: an orange square with a navy bar, on white.
fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    let picture = image::RgbaImage::from_fn(200, 100, |x, y| {
        image::Rgba(match ((20..180).contains(&x), (45..55).contains(&y), (10..90).contains(&y)) {
            (true, true, _) => [20, 40, 120, 255],
            (true, _, true) => [240, 130, 20, 255],
            _ => [255, 255, 255, 255],
        })
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(picture).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let data = vectorcraft_format::base64_encode(&png);
    app.run("file.place", json!({"name": "logo.png", "dataBase64": data})).unwrap();
    app
}

/// One headless frame at `time` with `events`, as the app runs one: its logic (the fork's poll and
/// the shortcuts among it) and the dialogs.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

/// Frames until `done(app)`, time moving on, or a deadline generous for a busy machine.
fn until(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, done: impl Fn(&VectorcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(app) {
        assert!(Instant::now() < deadline, "the window never got there");
        *time += 0.05;
        frame(app, ctx, *time, vec![]);
    }
}

/// Whether the window's result is of its settings as they are, every stage done.
fn ready(app: &VectorcraftApp) -> bool {
    app.embroidery.image_window.live.as_ref().is_some_and(|l| l.schedule.current(&params_of(&l.fields)).is_some())
}

fn open_dialog(app: &VectorcraftApp) -> Option<&Dialog> {
    app.ui.dialog.as_ref().filter(|d| d.kind == KIND)
}

fn undo_steps(app: &VectorcraftApp) -> usize {
    app.session.doc().unwrap().history.undo.len()
}

/// Digitize Image… opens the window on the image selected, with the document's palette; with no
/// single object selected, or something that isn't an image, it opens nothing and says why.
#[test]
fn it_opens_on_the_selected_image_and_is_refused_without_one() {
    let mut app = app();
    let image = app.session.doc().unwrap().selection.objects[0];
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let d = open_dialog(&app).expect("the window opens");
    assert_eq!((d.fields["id"].as_u64(), d.str("palette")), (Some(image.0), "jef".to_string()));
    crate::dialogs::cancel(&mut app);
    app.run("select.none", json!({})).unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert!(app.ui.dialog.is_none());
    assert_eq!(app.ui.status, "Select one image to digitize it.");
    app.run("shape.rectangle", json!({"x": 300, "y": 300, "width": 20, "height": 20})).unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert!(app.ui.dialog.is_none(), "not an image");
    assert!(app.ui.status.contains("not an image"), "{}", app.ui.status);
}

/// The palette's Digitize Image and Object › Image Trace › Digitize Image… open the window; an
/// agent's `embroidery.digitizeImage` still digitizes at once, as every command runs.
#[test]
fn the_palette_item_and_the_menu_open_the_window_and_agents_still_digitize_at_once() {
    let mut app = app();
    assert!(crate::embroidery::menu::invoked(&mut app, "embroidery.digitizeImage"));
    assert!(open_dialog(&app).is_some());
    crate::dialogs::cancel(&mut app);
    let steps = undo_steps(&app);
    app.run("embroidery.digitizeImage", json!({})).unwrap();
    assert_eq!(undo_steps(&app), steps + 1, "made at once");
}

/// Cmd/Ctrl+Enter is Create: once the window's result is ready it applies it as one undo step,
/// closes the window and says what was made.
#[test]
fn command_enter_creates() {
    let ctx = egui::Context::default();
    let mut app = app();
    let steps = undo_steps(&app);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    frame(&mut app, &ctx, time + 0.05, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(open_dialog(&app).is_none(), "closed");
    assert_eq!(undo_steps(&app), steps + 1);
    assert!(app.ui.status.starts_with("Created ") && app.ui.status.ends_with("Undo removes them."), "{}", app.ui.status);
}

/// Create asked for before the result is ready waits for it, then applies it.
#[test]
fn create_waits_for_the_result() {
    let ctx = egui::Context::default();
    let mut app = app();
    let steps = undo_steps(&app);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![key(Key::Enter, Modifiers::COMMAND)]);
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1);
}

/// Escape closes the window keeping its settings for that image, for the session: Digitize Image…
/// on it again continues where it was left; Start Over goes back to the defaults.
#[test]
fn escape_closes_the_window_remembering_it_and_start_over_resets() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    frame(&mut app, &ctx, time, vec![]);
    app.ui.dialog.as_mut().unwrap().fields.insert("colors".into(), json!(3));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(open_dialog(&app).is_none(), "Escape closed it");
    frame(&mut app, &ctx, time + 0.05, vec![]);
    assert!(app.embroidery.image_window.live.is_none(), "its runs stopped");
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("colors")).cloned(), Some(json!(3)), "continued");
    start_over(&mut app);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("colors")).cloned(), None, "the defaults");
}

/// Agents drive the window as every dialog: `ui.dialog.set` sets a field, `ui.dialog.confirm` is
/// Create, in one go, with the fields as they are.
#[test]
fn ui_dialog_set_and_confirm_drive_it() {
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    app.ui.dialog.as_mut().unwrap().fields.insert("colors".into(), json!(3));
    let steps = undo_steps(&app);
    let reply = crate::dialogs::confirm(&mut app).unwrap();
    assert!(reply["created"]["objects"].as_u64().is_some_and(|n| n > 0), "{reply}");
    assert_eq!(reply["settings"]["colors"], json!(3));
    assert!(open_dialog(&app).is_none());
    assert_eq!(undo_steps(&app), steps + 1);
}

/// Another document made active closes the window as Cancel, keeping its settings.
#[test]
fn it_closes_as_cancel_when_its_document_is_no_longer_active() {
    let ctx = egui::Context::default();
    let mut app = app();
    let (uid, image) = (app.session.doc().unwrap().uid, app.session.doc().unwrap().selection.objects[0]);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![]);
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    frame(&mut app, &ctx, 0.05, vec![]);
    frame(&mut app, &ctx, 0.1, vec![]);
    assert!(open_dialog(&app).is_none());
    assert!(app.embroidery.image_window.remembered.contains_key(&(uid, image)));
}

/// The window takes most of the app's window, never less than its smallest size where that fits,
/// never more than the app's window; its regions lie inside it apart from each other.
#[test]
fn it_draws_whole_at_its_smallest_and_compact_sizes() {
    let big = frame::size(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 874.0)));
    assert!((big.x - 1440.0 * frame::WIDTH_SHARE).abs() < 0.5 && (big.y - 874.0 * frame::HEIGHT_SHARE).abs() < 0.5);
    let small = frame::size(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 650.0)));
    assert_eq!(small, frame::MIN_SIZE);
    let tiny = frame::size(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 500.0)));
    assert!(tiny.x <= 800.0 && tiny.y <= 500.0);
    for width in [frame::MIN_SIZE.x, frame::COMPACT_BELOW + 100.0] {
        let inner = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, frame::MIN_SIZE.y));
        let r = frame::regions(inner);
        for a in [r.title, r.preview, r.settings, r.bar] {
            assert!(inner.contains_rect(a) && a.width() > 0.0 && a.height() > 0.0, "{a:?} in {inner:?}");
        }
        assert!(!r.preview.intersects(r.settings) && !r.title.intersects(r.preview) && !r.preview.intersects(r.bar));
        let column = if frame::compact(width) { frame::COMPACT_SETTINGS_WIDTH } else { frame::SETTINGS_WIDTH };
        assert!((r.settings.width() - column).abs() < 0.5);
    }
}

/// `__state` says how the window's result stands, for agents.
#[test]
fn its_state_is_said_in_its_fields() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    frame(&mut app, &ctx, time + 0.05, vec![]);
    let state: Value = open_dialog(&app).map(|d| d.fields["__state"].clone()).unwrap_or_default();
    assert_eq!(state["state"], "fresh", "{state}");
}

/// The window's live state.
fn live(app: &VectorcraftApp) -> &Live {
    app.embroidery.image_window.live.as_ref().expect("the window is live")
}

/// Set field `key` of the open window.
fn set(app: &mut VectorcraftApp, key: &str, value: Value) {
    app.ui.dialog.as_mut().expect("the window is open").fields.insert(key.into(), value);
}

/// Switching the preview's view recomputes nothing: the result shown stays the run's.
#[test]
fn switching_views_reruns_nothing() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    let plan = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.number(vectorcraft_engine::cmd::embroidery::DigitizeStage::Plan));
    for view in preview::VIEWS.iter().chain(preview::VIEWS.iter()) {
        set(&mut app, VIEW, json!(view));
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
        assert!(!live(&app).schedule.busy(), "{view}: nothing run");
    }
    let now = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.number(vectorcraft_engine::cmd::embroidery::DigitizeStage::Plan));
    assert_eq!(now, plan);
}

/// Each view uploads its texture once a result: going round the views again uploads nothing, and
/// dragging Compare's split uploads nothing.
#[test]
fn views_upload_once_and_a_compare_drag_uploads_nothing() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    for view in preview::VIEWS {
        set(&mut app, VIEW, json!(view));
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    let uploads = live(&app).preview.uploads;
    assert!(uploads >= 4, "the original, the colours, the shapes and the stitches: {uploads}");
    for view in preview::VIEWS {
        set(&mut app, VIEW, json!(view));
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(live(&app).preview.uploads, uploads, "kept");
    set(&mut app, VIEW, json!("compare"));
    set(&mut app, STEP, json!(4));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    let before = live(&app).preview.uploads;
    for k in 0..30 {
        set(&mut app, preview::SPLIT, json!(f64::from(k) / 30.0));
        time += 1.0 / 60.0;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(live(&app).preview.uploads, before, "the split only clips");
}

/// The area under the pointer is named in the window: its thread and number, its size and place.
#[test]
fn hovering_names_the_area_under_the_pointer() {
    let ctx = egui::Context::default();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    set(&mut app, VIEW, json!("colors"));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    let (_, xf) = live(&app).preview.last_view.expect("drawn");
    let outputs = live(&app).schedule.shown.as_ref().map(|s| s.outputs.clone()).expect("shown");
    // The orange band above the navy bar, in the working copy's pixels.
    let doc = outputs.working_to_doc().expect("an image") * vectorcraft_geom::Point::new(100.0, 25.0);
    let at = xf.to_screen(doc);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at)]);
    let facts = live(&app).preview.hovered.clone().expect("an area under the pointer");
    assert!(facts.sewn && facts.own_number == Some(1), "{facts:?}");
    let line = preview::area_line(&facts);
    assert!(line.starts_with(&format!("{} 1 · ", facts.thread_name.clone().unwrap_or_default())) && line.contains("mm²"), "{line}");
}
