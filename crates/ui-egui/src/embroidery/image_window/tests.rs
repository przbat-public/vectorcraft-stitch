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
pub(super) fn app() -> VectorcraftApp {
    app_with(image::RgbaImage::from_fn(200, 100, |x, y| {
        image::Rgba(match ((20..180).contains(&x), (45..55).contains(&y), (10..90).contains(&y)) {
            (true, true, _) => [20, 40, 120, 255],
            (true, _, true) => [240, 130, 20, 255],
            _ => [255, 255, 255, 255],
        })
    }))
}

/// An app with `picture` placed and selected.
pub(super) fn app_with(picture: image::RgbaImage) -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(picture).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let data = vectorcraft_format::base64_encode(&png);
    app.run("file.place", json!({"name": "logo.png", "dataBase64": data})).unwrap();
    app
}

/// One headless frame at `time` with `events`, as the app runs one: its logic (the fork's poll and
/// the shortcuts among it) and the dialogs.
pub(super) fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) {
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

pub(super) fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

/// Frames until `done(app)`, time moving on, or a deadline generous for a busy machine.
pub(super) fn until(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, done: impl Fn(&VectorcraftApp) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(app) {
        assert!(Instant::now() < deadline, "the window never got there");
        *time += 0.05;
        frame(app, ctx, *time, vec![]);
    }
}

/// Whether the window's result is of its settings as they are (the open dialog's: a test may have
/// set one since the last frame), every stage done.
pub(super) fn ready(app: &VectorcraftApp) -> bool {
    let (Some(l), Some(d)) = (app.embroidery.image_window.live.as_ref(), open_dialog(app)) else { return false };
    l.schedule.current(&params_of(&d.fields)).is_some()
}

pub(super) fn open_dialog(app: &VectorcraftApp) -> Option<&Dialog> {
    app.ui.dialog.as_ref().filter(|d| d.kind == KIND)
}

pub(super) fn undo_steps(app: &VectorcraftApp) -> usize {
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

/// The settings a window was left with are its picture's: another picture placed after an Undo,
/// which takes the same id in the document, opens with its own defaults; the first picture placed
/// again by Redo continues where its window was left.
#[test]
fn a_picture_placed_after_an_undo_under_the_same_id_opens_with_its_own_settings() {
    let ctx = egui::Context::default();
    let mut app = app();
    let first = app.session.doc().unwrap().selection.objects[0];
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    app.ui.dialog.as_mut().unwrap().fields.insert("colors".into(), json!(5));
    frame(&mut app, &ctx, 0.0, vec![]);
    frame(&mut app, &ctx, 0.05, vec![key(Key::Escape, Modifiers::NONE)]);
    frame(&mut app, &ctx, 0.1, vec![]);
    assert!(open_dialog(&app).is_none(), "closed, its settings remembered");
    let colors = |app: &VectorcraftApp| open_dialog(app).and_then(|d| d.fields.get("colors").cloned());
    // The placement undone and redone: the same picture, its window as it was left.
    app.run("edit.undo", json!({})).unwrap();
    app.run("edit.redo", json!({})).unwrap();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert_eq!(colors(&app), Some(json!(5)), "the same picture continues");
    frame(&mut app, &ctx, 0.15, vec![key(Key::Escape, Modifiers::NONE)]);
    frame(&mut app, &ctx, 0.2, vec![]);
    // The placement undone, another picture placed in its place: the same id, not the same picture.
    app.run("edit.undo", json!({})).unwrap();
    let other = image::RgbaImage::from_fn(120, 80, |x, _| image::Rgba(if x < 60 { [20, 120, 40, 255] } else { [255, 255, 255, 255] }));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(other).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    app.run("file.place", json!({"name": "other.png", "dataBase64": vectorcraft_format::base64_encode(&png)})).unwrap();
    assert_eq!(app.session.doc().unwrap().selection.objects.first(), Some(&first), "the same id");
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    assert_eq!(colors(&app), None, "its own defaults");
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
        for a in [r.title, r.strip, r.preview, r.settings, r.bar] {
            assert!(inner.contains_rect(a) && a.width() > 0.0 && a.height() > 0.0, "{a:?} in {inner:?}");
        }
        assert!(!r.preview.intersects(r.settings) && !r.strip.intersects(r.preview) && !r.title.intersects(r.strip) && !r.preview.intersects(r.bar));
        let column = if frame::compact(width) { frame::COMPACT_SETTINGS_WIDTH } else { frame::SETTINGS_WIDTH };
        assert!((r.settings.width() - column).abs() < 0.5);
    }
}

/// The settings column, longer than its room in the smallest window, shows its scroll bar before
/// the pointer is over it, in a column of its own, as the app's panels do.
#[test]
fn the_settings_column_shows_its_bar_when_it_scrolls() {
    fn flatten(s: egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(2));
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 650.0));
    let mut shapes = Vec::new();
    // A few frames: the column learns its content's height, the window its size.
    for _ in 0..3 {
        time += 0.05;
        let raw = egui::RawInput { time: Some(time), screen_rect: Some(screen), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            crate::dialogs::show(&mut app, ui.ctx());
        });
        out.textures_delta.clear();
        shapes = Vec::new();
        out.shapes.into_iter().for_each(|c| flatten(c.shape, &mut shapes));
    }
    let window = ctx.memory(|m| m.area_rect(egui::Id::unique("embroidery-digitize-image"))).expect("the window drawn");
    assert_eq!(window.size().round(), (frame::MIN_SIZE + vec2(2.0, 2.0)).round(), "the smallest window, its stroke round it");
    let settings = frame::regions(window.shrink(17.0)).settings;
    // A narrow, tall, visible rectangle at the column's right edge is its bar.
    let bars = shapes
        .iter()
        .filter(|s| {
            matches!(s, egui::Shape::Rect(r) if r.fill.a() > 0 && r.rect.width() < 10.0 && r.rect.height() > 30.0
            && (settings.right() - 16.0..=settings.right() + 2.0).contains(&r.rect.center().x)
            && settings.y_range().contains(r.rect.center().y))
        })
        .count();
    assert!(bars > 0, "the column's bar shown in {settings:?}");
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
pub(super) fn live(app: &VectorcraftApp) -> &Live {
    app.embroidery.image_window.live.as_ref().expect("the window is live")
}

/// The window's live state, to change.
pub(super) fn live_mut(app: &mut VectorcraftApp) -> &mut Live {
    app.embroidery.image_window.live.as_mut().expect("the window is live")
}

/// Set field `key` of the open window.
pub(super) fn set(app: &mut VectorcraftApp, key: &str, value: Value) {
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

/// One frame with screen readers on (the context's) → its nodes.
pub(super) fn nodes(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<(egui::accesskit::NodeId, egui::accesskit::Node)> {
    *time += 0.05;
    let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
    out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default()
}

/// The node screen readers call `name`, of `role`: named itself, or by a label beside it.
pub(super) fn node<'a>(
    nodes: &'a [(egui::accesskit::NodeId, egui::accesskit::Node)],
    name: &str,
    role: egui::accesskit::Role,
) -> Option<&'a egui::accesskit::Node> {
    node_with_id(nodes, name, role).map(|(_, n)| n)
}

/// [`node`] with its id.
pub(super) fn node_with_id<'a>(
    nodes: &'a [(egui::accesskit::NodeId, egui::accesskit::Node)],
    name: &str,
    role: egui::accesskit::Role,
) -> Option<&'a (egui::accesskit::NodeId, egui::accesskit::Node)> {
    let labels: Vec<egui::accesskit::NodeId> =
        nodes.iter().filter(|(_, n)| n.role() == egui::accesskit::Role::Label && n.value() == Some(name)).map(|(id, _)| *id).collect();
    nodes.iter().find(|(_, n)| n.role() == role && (n.label() == Some(name) || n.labelled_by().iter().any(|l| labels.contains(l))))
}

/// The centre of the control screen readers call `name`, of `role`.
pub(super) fn centre_of(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, role: egui::accesskit::Role) -> egui::Pos2 {
    let nodes = nodes(app, ctx, time);
    let b = node(&nodes, name, role).and_then(|n| n.bounds()).unwrap_or_else(|| panic!("no {role:?} {name}"));
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// A click of the control screen readers call `name`, of `role`.
pub(super) fn click(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, role: egui::accesskit::Role) {
    let at = centre_of(app, ctx, time, name, role);
    let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::PointerMoved(at), button(true), button(false)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// A screen reader's click of the control it calls `name`, of `role`: pressed wherever the control
/// lies, below the part of the step in view too, where a pointer's click would miss it.
pub(super) fn press(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, role: egui::accesskit::Role) {
    use egui::accesskit::{Action, ActionRequest, TreeId};
    let nodes = nodes(app, ctx, time);
    let target_node = node_with_id(&nodes, name, role).map(|(id, _)| *id).unwrap_or_else(|| panic!("no {role:?} {name}"));
    let request = ActionRequest { action: Action::Click, target_tree: TreeId::ROOT, target_node, data: None };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::AccessKitActionRequest(request)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// `text` typed into the field screen readers call `name`, over what it held, then Enter.
pub(super) fn type_into(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, text: &str) {
    click(app, ctx, time, name, egui::accesskit::Role::TextInput);
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::A, Modifiers::COMMAND), Event::Text(text.into())]);
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::Enter, Modifiers::NONE)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// The window open on the test picture, its first result shown, screen readers on.
pub(super) fn opened() -> (VectorcraftApp, egui::Context, f64) {
    opened_with(app())
}

/// [`opened`] in `app`.
pub(super) fn opened_with(mut app: VectorcraftApp) -> (VectorcraftApp, egui::Context, f64) {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    (app, ctx, time)
}

/// The reply of the result shown.
pub(super) fn reply(app: &mut VectorcraftApp) -> Value {
    app.embroidery.image_window.live.as_mut().and_then(|l| l.reply().cloned()).expect("a result")
}

/// A step's card opens its section and shows its view; with Compare on, Compare stays, showing the
/// original against the step chosen.
#[test]
fn a_card_opens_its_section_and_view() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    assert!(node(&nodes_now, "Crop to the Art", Role::Button).is_none(), "the Image step's section is closed");
    set(&mut app, VIEW, json!("colors"));
    click(&mut app, &ctx, &mut time, "Shapes", Role::Tab);
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.fields[STEP].clone(), d.str(VIEW)), (json!(3), "shapes".to_string()));
    set(&mut app, VIEW, json!("compare"));
    click(&mut app, &ctx, &mut time, "Image", Role::Tab);
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.fields[STEP].clone(), d.str(VIEW)), (json!(1), "compare".to_string()));
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    assert!(node(&nodes_now, "Crop to the Art", Role::Button).is_some(), "the Image step's section is open");
    assert!(node(&nodes_now, "Image", Role::Tab).is_some_and(|n| n.is_selected() == Some(true)));
}

/// Each card says its step, what it made and how long it took, in words.
#[test]
fn summaries_are_said_in_words() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let said = |name: &str| node(&nodes_now, name, Role::Tab).and_then(|n| n.description()).map(str::to_string).unwrap_or_default();
    let image = said("Image");
    assert!(image.starts_with("Step 1 of 4: Image. ") && image.contains(" mm · background left out. ") && image.ends_with(" ms"), "{image}");
    let colours = said("Colors");
    assert!(colours.starts_with("Step 2 of 4: Colors. 2 threads + background. "), "{colours}");
    let shapes = said("Shapes");
    assert!(shapes.starts_with("Step 3 of 4: Shapes. ") && shapes.contains(" areas. "), "{shapes}");
    let stitches = said("Stitches");
    assert!(stitches.starts_with("Step 4 of 4: Stitches. ") && stitches.contains(" stitches. "), "{stitches}");
}

/// The width typed sets the size, the height following the picture (2 : 1), and the height typed
/// sets it the other way; Enter in a field commits the field and creates nothing.
#[test]
fn width_and_height_stay_linked() {
    let (mut app, ctx, mut time) = opened();
    let steps = undo_steps(&app);
    set(&mut app, STEP, json!(1));
    type_into(&mut app, &ctx, &mut time, "Width", "100");
    let d = open_dialog(&app).expect("still open: Enter in a field creates nothing");
    assert_eq!((d.fields.get("width"), d.fields.get("height")), (Some(&json!(100.0)), None));
    until(&mut app, &ctx, &mut time, ready);
    let size = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.size_mm()).expect("a size");
    assert!((size.0 - 100.0).abs() < 0.01 && (size.1 - 50.0).abs() < 0.01, "{size:?}");
    type_into(&mut app, &ctx, &mut time, "Height", "40");
    until(&mut app, &ctx, &mut time, ready);
    let size = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.size_mm()).expect("a size");
    assert!((size.0 - 80.0).abs() < 0.01 && (size.1 - 40.0).abs() < 0.01, "{size:?}");
    assert_eq!(undo_steps(&app), steps, "nothing made");
}

/// A size of nothing, or one past the engine's top, is refused: the status bar says why and the
/// fields keep what they held.
#[test]
fn a_value_out_of_range_is_refused_and_kept() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    let before = open_dialog(&app).map(|d| d.fields.clone()).expect("open");
    type_into(&mut app, &ctx, &mut time, "Width", "0");
    assert_eq!(app.ui.status, "A size is more than 0 mm.");
    type_into(&mut app, &ctx, &mut time, "Height", "5000");
    assert!(app.ui.status.ends_with(" is too large: a side is at most 1000 mm."), "{}", app.ui.status);
    let after = open_dialog(&app).map(|d| d.fields.clone()).expect("open");
    for key in SETTING_FIELDS {
        assert_eq!(after.get(key), before.get(key), "{key}");
    }
}

/// Crop to the Art crops to the box of what isn't the background (the orange square), the white
/// it found in the corners still left out, and that box is what is digitized; Whole Image takes the
/// crop off, the whole picture digitized again (only the crop changes then).
#[test]
fn crop_to_the_art_crops_to_what_isnt_background() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    let worked = |app: &mut VectorcraftApp| reply(app)["image"]["worked"].clone();
    let whole = worked(&mut app);
    assert_eq!(whole, reply(&mut app)["image"]["pixels"], "worked pixel for pixel at this size");
    let art = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.art_bounds());
    assert_eq!(art, Some([20, 10, 180, 90]));
    click(&mut app, &ctx, &mut time, "Crop to the Art", Role::Button);
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.fields.get("crop"), d.fields.get("background")), (Some(&json!([20, 10, 180, 90])), Some(&json!("#ffffff"))));
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    assert_eq!((r["image"]["crop"].clone(), r["colorCount"].clone()), (json!([20, 10, 180, 90]), json!(2)), "{}", r["image"]);
    assert_eq!(worked(&mut app), json!([160, 80]), "the art's box digitized");
    click(&mut app, &ctx, &mut time, "Whole Image", Role::Button);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("crop").cloned()), None);
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(worked(&mut app), whole, "the whole picture digitized again");
}

/// The background is left out, or sewn (every colour, the white too), or one of the picture's
/// colours chosen as it.
#[test]
fn the_background_can_be_left_out_or_sewn() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    click(&mut app, &ctx, &mut time, "Leave the background out", Role::CheckBox);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("background").cloned()), Some(json!("none")));
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    assert_eq!((r["image"]["background"]["from"].clone(), r["colorCount"].clone()), (json!("none"), json!(3)));
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let image = node(&nodes_now, "Image", Role::Tab).and_then(|n| n.description()).unwrap_or_default().to_string();
    assert!(image.contains(" mm · every color sewn. "), "{image}");
    click(&mut app, &ctx, &mut time, "Leave the background out", Role::CheckBox);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("background").cloned()), None);
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    let sewn =
        r["colors"].as_array().and_then(|a| a.iter().find(|c| c["background"] == json!(false))).map(|c| c["color"].clone()).expect("a colour sewn");
    click(&mut app, &ctx, &mut time, &format!("Color {}", sewn.as_str().unwrap_or_default()), Role::Button);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("background").cloned()), Some(sewn));
}

/// A background colour chosen from the picture comes back when "Leave the background out" is
/// unticked and ticked again, not the colour found in the corners (the project owner's choice,
/// 2026-11-12).
#[test]
fn a_background_colour_chosen_comes_back_when_left_out_again() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    let r = reply(&mut app);
    let colours = r["colors"].as_array().cloned().unwrap_or_default();
    let chosen = colours.iter().find(|c| c["background"] == json!(false)).and_then(|c| c["color"].as_str()).expect("a colour sewn").to_string();
    let background = |app: &VectorcraftApp| open_dialog(app).and_then(|d| d.fields.get("background").cloned());
    click(&mut app, &ctx, &mut time, &format!("Color {chosen}"), Role::Button);
    assert_eq!(background(&app), Some(json!(chosen)), "chosen as the background");
    click(&mut app, &ctx, &mut time, "Leave the background out", Role::CheckBox);
    assert_eq!(background(&app), Some(json!("none")), "every colour sewn");
    click(&mut app, &ctx, &mut time, "Leave the background out", Role::CheckBox);
    assert_eq!(background(&app), Some(json!(chosen)), "the colour chosen before, not the corners' again");
}

/// A click of the control screen readers call `name`, whatever its role.
fn click_named(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str) {
    let nodes_now = nodes(app, ctx, time);
    let role = nodes_now.iter().find(|(_, n)| n.label() == Some(name)).map(|(_, n)| n.role()).unwrap_or_else(|| panic!("no {name}"));
    click(app, ctx, time, name, role);
}

/// A thread chosen for a colour in its list is a colour choice of the engine's, and the window
/// recomputes from the colours on, the trace kept.
#[test]
fn a_choice_becomes_a_color_choice_and_recomputes_from_the_colours() {
    use egui::accesskit::Role;
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(2));
    let number = |app: &VectorcraftApp, stage| live(app).schedule.shown.as_ref().and_then(|s| s.outputs.number(stage));
    let (trace, colours) = (number(&app, DigitizeStage::Trace), number(&app, DigitizeStage::Colours));
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let label = |role: Role, test: &dyn Fn(&str) -> bool| -> Vec<String> {
        nodes_now.iter().filter(|(_, n)| n.role() == role).filter_map(|(_, n)| n.label()).filter(|l| test(l)).map(str::to_string).collect()
    };
    let orange = label(Role::ListBoxOption, &|l| l.starts_with("#f08214 in "));
    assert_eq!(orange.len(), 1, "the orange's row");
    click(&mut app, &ctx, &mut time, &orange[0], Role::ListBoxOption);
    click(&mut app, &ctx, &mut time, "Choose Thread…", Role::Button);
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    // The document's palette, a new document's: Janome's.
    let mut listed: Vec<&egui::accesskit::Node> =
        nodes_now.iter().map(|(_, n)| n).filter(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("  JEF "))).collect();
    listed.sort_by(|a, b| a.bounds().map_or(0.0, |r| r.y0).total_cmp(&b.bounds().map_or(0.0, |r| r.y0)));
    let threads: Vec<String> = listed.iter().filter_map(|n| n.label()).map(str::to_string).collect();
    assert!(threads.first().is_some_and(|l| l.ends_with(" · nearest")), "nearest first: {threads:?}");
    assert_eq!(threads.iter().filter(|l| l.ends_with(" · nearest")).count(), 1, "{threads:?}");
    // The second, in view under the nearest.
    let other = threads.get(1).expect("another thread").clone();
    let index: u64 = other.split("  JEF ").nth(1).and_then(|r| r.split(',').next()).and_then(|n| n.parse().ok()).expect("a number");
    click(&mut app, &ctx, &mut time, &other, Role::Button);
    let choices = open_dialog(&app).map(|d| d.fields["colorChoices"].clone()).expect("open");
    assert_eq!(choices, json!([{"color": "#f08214", "thread": {"palette": "jef", "index": index}}]));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(number(&app, DigitizeStage::Trace), trace, "the trace kept");
    assert_ne!(number(&app, DigitizeStage::Colours), colours, "the colours made again");
    let r = reply(&mut app);
    let row = r["colors"].as_array().and_then(|a| a.iter().find(|c| c["color"] == "#f08214")).cloned().expect("the orange");
    assert_eq!((row["reason"].clone(), row["thread"]["index"].clone()), (json!("chosen"), json!(index)));
}

/// Choosing another palette than the document's (a new document's, Janome's) says that Create makes
/// it the document's and leaves out the threads chosen in the other; Create sets it, in the same
/// undo step.
#[test]
fn the_palette_change_is_said_and_applied_on_create() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(2));
    set(&mut app, "colorChoices", json!([{"color": "#f08214", "thread": {"palette": "jef", "index": 30}}]));
    until(&mut app, &ctx, &mut time, ready);
    let said = |nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)]| {
        nodes.iter().any(|(_, n)| n.value() == Some("Making the objects sets the document's thread palette to Brother (PES)."))
    };
    assert!(!said(&nodes(&mut app, &ctx, &mut time)), "the document's own: nothing to say");
    click_named(&mut app, &ctx, &mut time, "Match to threads");
    click_named(&mut app, &ctx, &mut time, "Brother (PES) · 61 threads");
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.str(PALETTE), d.fields.get("colorChoices")), ("pec".to_string(), None));
    assert!(said(&nodes(&mut app, &ctx, &mut time)));
    until(&mut app, &ctx, &mut time, ready);
    let steps = undo_steps(&app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1, "one undo step");
    let doc = &app.session.doc().expect("a document").doc;
    assert_eq!(vectorcraft_engine::cmd::embroidery::thread_palette_id(doc), "pec");
}

/// What the Live bar's status tells screen readers.
fn status_said(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> String {
    let nodes_now = nodes(app, ctx, time);
    nodes_now.iter().find(|(_, n)| n.role() == egui::accesskit::Role::Status).and_then(|(_, n)| n.label()).unwrap_or_default().to_string()
}

/// One frame → the texts it drew.
fn frame_texts(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<String> {
    fn texts(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    *time += 0.05;
    let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut all = Vec::new();
    out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
    all
}

/// The figures are told to screen readers once a run settles: while a change is recomputed they
/// are told nothing new, then the new figures.
#[test]
fn the_figures_are_announced_once_a_recompute_settles() {
    let (mut app, ctx, mut time) = opened();
    let before = status_said(&mut app, &ctx, &mut time);
    assert!(before.contains(" stitches · ") && before.contains(" threads · ") && before.ends_with(" mm"), "{before}");
    set(&mut app, "width", json!(120.0));
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut recomputing = 0;
    while !ready(&app) {
        assert!(Instant::now() < deadline, "never ready");
        let said = status_said(&mut app, &ctx, &mut time);
        if live(&app).schedule.busy() {
            recomputing += 1;
            assert_eq!(said, before, "nothing new while it recomputes");
        }
    }
    assert!(recomputing > 0, "a recompute was seen");
    let after = status_said(&mut app, &ctx, &mut time);
    assert!(after != before && after.contains(" stitches · ") && after.ends_with(" mm"), "{after}");
}

/// Settings that can't be digitized say why, the last figures kept on screen; screen readers are
/// told why.
#[test]
fn stale_and_failed_states_keep_the_last_figures() {
    let (mut app, ctx, mut time) = opened();
    let figures = status_said(&mut app, &ctx, &mut time);
    set(&mut app, "width", json!(5000.0));
    until(&mut app, &ctx, &mut time, |app| matches!(live(app).schedule.freshness(0.0), super::schedule::Freshness::Failed(_)));
    let said = status_said(&mut app, &ctx, &mut time);
    assert!(said.starts_with("Can't digitize it: "), "{said}");
    let drawn = frame_texts(&mut app, &ctx, &mut time);
    assert!(drawn.iter().any(|t| t.contains("Can't digitize it: ") && t.contains(&figures)), "the last figures kept: {drawn:?}");
    // Put right: while it recomputes, screen readers aren't told the old figures again; then the
    // new ones.
    set(&mut app, "width", json!(80.0));
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut recomputing = 0;
    while !ready(&app) {
        assert!(Instant::now() < deadline, "never ready");
        let now = status_said(&mut app, &ctx, &mut time);
        if live(&app).schedule.busy() {
            recomputing += 1;
            assert_eq!(now, said, "still the reason while it recomputes");
        }
    }
    assert!(recomputing > 0, "a recompute was seen");
    let settled = status_said(&mut app, &ctx, &mut time);
    assert!(settled.contains(" stitches · ") && settled != figures, "{settled}");
}

/// With every colour left out nothing would be made: Create is disabled with its reason, and
/// Cmd/Ctrl+Enter says it and makes nothing.
#[test]
fn create_is_disabled_with_its_reason_when_nothing_is_sewn() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, "colorChoices", json!([{"color": "#f08214", "job": "none"}, {"color": "#142878", "job": "none"}]));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(status_said(&mut app, &ctx, &mut time), "Nothing to sew: every color is left out.");
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    assert!(node(&nodes_now, "Create Embroidery Objects", Role::Button).is_some_and(|n| n.is_disabled()), "disabled");
    let steps = undo_steps(&app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert_eq!(app.ui.status, "Nothing to create: every color is left out.");
    assert!(open_dialog(&app).is_some(), "still open");
    assert_eq!(undo_steps(&app), steps);
}

/// A crop of the background alone has nothing to sew: the bar says why and what to do, without
/// saying that every colour was left out (none was), and Cmd/Ctrl+Enter says the same of Create
/// and makes nothing.
#[test]
fn nothing_to_sew_says_its_cause() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, "crop", json!([0, 0, 15, 15]));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(status_said(&mut app, &ctx, &mut time), "Nothing to sew: every area is the background. Sew the background, or crop less.");
    let steps = undo_steps(&app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert_eq!(app.ui.status, "Nothing to create: every area is the background. Sew the background, or crop less.");
    assert!(open_dialog(&app).is_some(), "still open");
    assert_eq!(undo_steps(&app), steps, "nothing made");
}

/// Create applies the run shown as one undo step, selects what it made, and says what it made, in
/// how many threads, and the template layer the image moved to.
#[test]
fn create_applies_the_run_shown_in_one_undo_step() {
    let (mut app, ctx, mut time) = opened();
    let steps = undo_steps(&app);
    let threads = reply(&mut app)["stats"]["threadCount"].as_u64().expect("figures");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(open_dialog(&app).is_none(), "closed");
    assert_eq!(undo_steps(&app), steps + 1);
    let status = app.ui.status.clone();
    assert!(
        status.starts_with("Created ") && status.contains(&format!(" ({threads} threads). The image is on the hidden template layer “")),
        "{status}"
    );
    assert!(status.ends_with("”. Undo removes them."), "{status}");
    let st = app.session.doc().expect("a document");
    assert_eq!(st.selection.len(), 1, "the design's group selected");
}

/// Create on an image whose place isn't sewn (its layer hidden) leaves the image there, and the
/// status bar names no template layer: it said "The image is on the hidden template layer" of the
/// image's own layer.
#[test]
fn create_names_no_template_layer_where_the_image_stays_in_its_place() {
    let mut app = app();
    let image = app.session.doc().ok().and_then(|st| st.selection.objects.first().copied()).expect("the picture selected");
    let layer = app.session.doc().ok().and_then(|st| st.doc.layer_of(image)).expect("its layer");
    app.session
        .edit("hide", |d, _| {
            d.node_mut(layer).ok_or(vectorcraft_engine::EngineError::NoNode(layer))?.visible = false;
            Ok(())
        })
        .unwrap();
    let (mut app, ctx, mut time) = opened_with(app);
    let threads = reply(&mut app)["stats"]["threadCount"].as_u64().expect("figures");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(open_dialog(&app).is_none(), "closed");
    let st = app.session.doc().expect("a document");
    assert_eq!(st.doc.layer_of(image), Some(layer), "the image in its place");
    let status = app.ui.status.clone();
    assert!(status.starts_with("Created ") && status.ends_with(&format!(" ({threads} threads). Undo removes them.")), "{status}");
    assert!(!status.contains("template layer"), "{status}");
}

/// An edit to the document under the window makes it digitize again before Create: Create asked
/// for right after waits for that run, and applies it; the run of the earlier document is refused.
#[test]
fn create_refuses_a_run_made_of_an_earlier_document() {
    let (mut app, ctx, mut time) = opened();
    let steps = undo_steps(&app);
    let earlier = live(&app).schedule.shown.clone().expect("a result");
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 20, "height": 20})).unwrap();
    assert!(app.session.apply_digitize_window(&earlier.request, &earlier.outputs, false).is_err(), "made of the earlier document");
    assert_eq!(undo_steps(&app), steps + 1, "nothing applied");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 2, "the rectangle, then Create");
}

/// Escape while Create waits for its run closes the window and makes nothing, the settings kept for
/// the image.
#[test]
fn escape_stops_a_create_waiting_for_its_run() {
    let ctx = egui::Context::default();
    let mut app = app();
    let steps = undo_steps(&app);
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    frame(&mut app, &ctx, 0.0, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(live(&app).create_waiting, "waiting for its run");
    frame(&mut app, &ctx, 0.01, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(open_dialog(&app).is_none(), "closed");
    for k in 0..10 {
        frame(&mut app, &ctx, 0.05 * f64::from(k + 1), vec![]);
    }
    assert_eq!(undo_steps(&app), steps, "nothing made");
    assert_eq!(app.embroidery.image_window.remembered.len(), 1, "its settings kept");
}

/// One frame → the texts it drew, with where.
fn frame_text_rects(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<(String, Rect)> {
    fn texts(s: &egui::Shape, out: &mut Vec<(String, Rect)>) {
        match s {
            egui::Shape::Text(t) => out.push((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
    *time += 0.05;
    let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::dialogs::show(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut all = Vec::new();
    out.shapes.iter().for_each(|c| texts(&c.shape, &mut all));
    all
}

/// A step's card shows its problems at its top right, how many there are beside the worst one's
/// mark: the test picture is coarse at its size, which step 1 warns of; step 3 has none.
#[test]
fn a_card_shows_its_steps_problems() {
    use crate::embroidery::marks::Mark;
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    let notes = super::live_bar::current_notes(app.embroidery.image_window.live.as_mut().expect("live"), crate::i18n::Lang::EN);
    let problems = |step: u8| notes.iter().filter(|n| n.step == step && matches!(n.mark, Mark::Error | Mark::Warning)).count();
    assert!(problems(1) >= 1, "{notes:?}");
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let card = |name: &str| {
        let b = node(&nodes_now, name, Role::Tab).and_then(|n| n.bounds()).expect("a card");
        Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32))
    };
    let (image, shapes) = (card("Image"), card("Shapes"));
    let drawn = frame_text_rects(&mut app, &ctx, &mut time);
    let top_right = |c: Rect, words: &str| {
        drawn.iter().any(|(t, r)| t == words && c.contains(r.center()) && r.center().x > c.center().x && r.center().y < c.center().y)
    };
    assert!(top_right(image, &problems(1).to_string()), "step 1's count");
    assert_eq!(problems(3), 0);
    assert!(
        !drawn.iter().any(|(t, r)| shapes.contains(r.center()) && r.center().y < shapes.center().y && t.chars().all(|c| c.is_ascii_digit())),
        "nothing on step 3"
    );
}

/// The window ends above the app's bottom bars (the hint line and the status bar) instead of
/// cutting through them, its top where it would be; on a small screen never shorter than its least
/// size.
#[test]
fn the_window_ends_above_the_hint_line() {
    use super::frame::{ABOVE_BARS, MIN_SIZE, placed, size};
    // Whole app frames, the hint line and the status bar drawn under the window.
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0));
    for k in 0..3 {
        let raw = egui::RawInput { time: Some(f64::from(k) * 0.05), screen_rect: Some(screen), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }
    let window = ctx.memory(|m| m.area_rect(egui::Id::unique("embroidery-digitize-image"))).expect("the window drawn");
    let floor = crate::widgets::menu_floor(&ctx);
    assert!(floor < screen.bottom() - 40.0, "the hint line and the status bar drawn: {floor}");
    // The window's frame adds its stroke round the size placed.
    let stroke = 1.0;
    assert!(window.bottom() <= floor - ABOVE_BARS + stroke, "{window:?} over the bars at {floor}");
    assert!((window.top() - (screen.center().y - size(screen).y / 2.0)).abs() <= stroke, "the top kept: {window:?}");
    let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 640.0));
    assert!(placed(small, small.bottom() - 52.0).0.y >= MIN_SIZE.y.min(size(small).y), "never shorter than its least");
}

/// In a design that encloses the background's colour (an orange ring on white, white inside it),
/// the Colors step says what its count counts: the colours besides the background, and that colour
/// sewn inside too, a thread more; its row says so as well.
#[test]
fn the_count_says_what_it_counts_when_the_design_encloses_the_backgrounds_colour() {
    let ring = image::RgbaImage::from_fn(200, 100, |x, y| {
        let outer = (20..180).contains(&x) && (10..90).contains(&y);
        let inner = (60..140).contains(&x) && (30..70).contains(&y);
        image::Rgba(if outer && !inner { [240, 130, 20, 255] } else { [255, 255, 255, 255] })
    });
    let (mut app, ctx, mut time) = opened_with(app_with(ring));
    set(&mut app, STEP, json!(2));
    let r = reply(&mut app);
    assert_eq!((r["settings"]["colors"].as_u64(), r["colorCount"].as_u64()), (Some(6), Some(2)), "{}", r["colors"]);
    let texts = frame_texts(&mut app, &ctx, &mut time);
    let said = "1 color besides the background, and the background's color sewn where the design encloses it.";
    assert!(texts.iter().any(|t| t == said), "{texts:?}");
    assert!(texts.iter().any(|t| t.ends_with(" · the background's color inside the design")), "{texts:?}");
}

/// `ui.dialog.confirm` with settings the engine refuses makes nothing and says why, rather than
/// the last result, which is of the settings before.
#[test]
fn ui_dialog_confirm_with_a_refused_setting_makes_nothing() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, "colors", json!(500));
    until(&mut app, &ctx, &mut time, |app| matches!(live(app).schedule.freshness(0.0), super::schedule::Freshness::Failed(_)));
    let steps = undo_steps(&app);
    let made = crate::dialogs::confirm(&mut app);
    assert!(made.is_err(), "made: {:?}", made.map(|r| r["settings"]["colors"].clone()));
    assert_eq!(undo_steps(&app), steps, "nothing made");
}

/// A Create waiting for a change the engine then refuses makes nothing: the status bar and the
/// Live bar say why, and nothing waits any more. A person gets there by asking 70 colours in
/// Janome's palette (78 threads), then choosing Brother's (61) and pressing Create before the
/// change has run.
#[test]
fn a_create_waiting_for_a_refused_change_makes_nothing_and_says_why() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, "colors", json!(70));
    until(&mut app, &ctx, &mut time, ready);
    let steps = undo_steps(&app);
    // Every stage heavy, so that the change waits for the settings to rest.
    live_mut(&mut app).schedule.time_stages(super::schedule::LIVE_PLAN_MS * 10.0);
    set(&mut app, PALETTE, json!("pec"));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    assert!(live(&app).create_waiting, "Create waits for the change");
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none() || !live(app).create_waiting);
    assert_eq!(undo_steps(&app), steps, "nothing made");
    assert!(open_dialog(&app).is_some(), "still open");
    assert!(app.ui.status.starts_with("Can't digitize it: "), "{}", app.ui.status);
    assert!(status_said(&mut app, &ctx, &mut time).starts_with("Can't digitize it: "));
}

/// The width typed in the next two tests (millimetres), and the share of the picture's width its
/// art takes: the orange square is 160 of its 200 pixels across.
const TYPED_WIDTH_MM: f64 = 100.0;
const ART_SHARE: f64 = 0.8;

/// How near a design's width must come to the one asked (millimetres): well under a pixel of the
/// picture at that size.
const NEAR_MM: f64 = 0.1;

/// The text typed into the field screen readers call `name`, over what it held, with no Enter.
fn type_only(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, text: &str) {
    click(app, ctx, time, name, egui::accesskit::Role::TextInput);
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::A, Modifiers::COMMAND), Event::Text(text.into())]);
}

/// Cmd/Ctrl+Enter while a width is typed and not yet committed commits it, as Enter would, and
/// creates the design at that width once its result is ready, not at the width before.
#[test]
fn command_enter_creates_with_the_value_being_typed() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    type_only(&mut app, &ctx, &mut time, "Width", &TYPED_WIDTH_MM.to_string());
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    let bounds = app.session.execute("document.exportSelection", &json!({"format": "svg"})).unwrap()["bounds"].clone();
    let width_mm = vectorcraft_engine::cmd::embroidery::pt_to_mm(bounds[2].as_f64().unwrap_or(0.0));
    assert!((width_mm - ART_SHARE * TYPED_WIDTH_MM).abs() < NEAR_MM, "made {width_mm:.2} mm wide (bounds {bounds})");
}

/// A click on Create while a width is typed and not yet committed: the press ends the field's
/// editing, which commits the width in the click's own frame, and Create waits for its run, as
/// Cmd/Ctrl+Enter does.
#[test]
fn create_clicked_while_a_width_is_typed_takes_it() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    type_only(&mut app, &ctx, &mut time, "Width", &TYPED_WIDTH_MM.to_string());
    click(&mut app, &ctx, &mut time, "Create Embroidery Objects", egui::accesskit::Role::Button);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    let bounds = app.session.execute("document.exportSelection", &json!({"format": "svg"})).unwrap()["bounds"].clone();
    let width_mm = vectorcraft_engine::cmd::embroidery::pt_to_mm(bounds[2].as_f64().unwrap_or(0.0));
    assert!((width_mm - ART_SHARE * TYPED_WIDTH_MM).abs() < NEAR_MM, "made {width_mm:.2} mm wide (bounds {bounds})");
}

/// Cmd/Ctrl+Enter while a value the field refuses is typed says why, as Enter would, and makes
/// nothing: the window stays open, its width as it was.
#[test]
fn command_enter_with_a_value_refused_makes_nothing() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    let steps = undo_steps(&app);
    type_only(&mut app, &ctx, &mut time, "Width", "0");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    // Time for a Create waiting to make something, were there one.
    let deadline = time + 1.0;
    while time < deadline && open_dialog(&app).is_some() {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(app.ui.status, "A size is more than 0 mm.");
    assert!(open_dialog(&app).is_some_and(|d| d.fields.get("width").is_none()), "still open, its width as it was");
    assert_eq!(undo_steps(&app), steps, "nothing made");
}

/// How near the area a press on the smallest area's slider takes lies to the one asked: a share of
/// it, for a press rounded to the point.
const SLIDER_NEAR: f64 = 0.05;

/// The Colors step's smallest area slider starts at one working pixel's area: a press at the middle
/// of its track, let go in the next frame, takes the area halfway on its logarithmic scale between
/// a working pixel's area and the top of the engine's range, as the window's field then says.
#[test]
fn the_colors_steps_smallest_area_slider_starts_at_a_working_pixel() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(2));
    let top = super::image_step::range_of(&reply(&mut app), "minArea").map(|r| r.1).expect("the engine's range");
    // One working pixel's area at the engine's working resolution: where the slider starts.
    let working = vectorcraft_engine::cmd::embroidery::DIGITIZE_WORKING_PX_PER_MM;
    let pixel = 1.0 / (working * working);
    let at = centre_of(&mut app, &ctx, &mut time, "Smallest area", egui::accesskit::Role::Slider);
    let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(true)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![button(false)]);
    let taken = open_dialog(&app).and_then(|d| d.fields.get("minArea")).and_then(Value::as_f64).expect("a smallest area taken");
    let halfway = (pixel * top).sqrt();
    assert!((taken - halfway).abs() < SLIDER_NEAR * halfway, "took {taken} mm², halfway is {halfway} mm²");
}

/// The numbers of the runs that made each stage's output shown, in the stages' order.
fn made_by(app: &VectorcraftApp) -> Vec<Option<u64>> {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let shown = live(app).schedule.shown.as_ref().expect("a result shown");
    DigitizeStage::ALL.iter().map(|&s| shown.outputs.number(s)).collect()
}

/// The first stage the result shown made again since `before` ([`made_by`]'s numbers), every stage
/// after it made again too.
fn made_again_from(app: &VectorcraftApp, before: &[Option<u64>]) -> Option<vectorcraft_engine::cmd::embroidery::DigitizeStage> {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let now = made_by(app);
    let first = before.iter().zip(&now).position(|(b, n)| b != n)?;
    let after: Vec<bool> = before.iter().zip(&now).skip(first).map(|(b, n)| b != n).collect();
    assert!(after.iter().all(|&again| again), "every stage after the first made again: {before:?} then {now:?}");
    DigitizeStage::ALL.get(first).copied()
}

/// The Shapes step's outlines: the smoothness typed, the corners from their slider (a press at the
/// middle of its track, let go in the next frame, takes the middle of 0 to 100) and straightening
/// ticked each write their setting, and the window recomputes from the trace.
#[test]
fn the_shapes_steps_outlines_recompute_from_the_trace() {
    use egui::accesskit::Role;
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(3));
    let field = |app: &VectorcraftApp, key: &str| open_dialog(app).and_then(|d| d.fields.get(key).cloned());
    let before = made_by(&app);
    type_into(&mut app, &ctx, &mut time, "Smoothness", "80");
    assert_eq!(field(&app, "smoothing"), Some(json!(80.0)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Trace));
    assert_eq!(reply(&mut app)["settings"]["smoothing"], json!(80.0));
    let before = made_by(&app);
    let at = centre_of(&mut app, &ctx, &mut time, "Corners", Role::Slider);
    let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(true)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![button(false)]);
    assert_eq!(field(&app, "corners"), Some(json!(50.0)), "the middle of 0 to 100");
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Trace));
    let before = made_by(&app);
    click(&mut app, &ctx, &mut time, "Straighten nearly straight lines", Role::CheckBox);
    assert_eq!(field(&app, "straightLines"), Some(json!(true)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Trace));
    assert_eq!(reply(&mut app)["settings"]["straightLines"], json!(true));
    click(&mut app, &ctx, &mut time, "Straighten nearly straight lines", Role::CheckBox);
    assert_eq!(field(&app, "straightLines"), Some(json!(false)), "unticked");
}

/// The overlap typed writes its setting and the window recomputes from the shapes, the areas and
/// their centre lines kept.
#[test]
fn the_overlap_typed_recomputes_from_the_shapes() {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(3));
    let before = made_by(&app);
    type_into(&mut app, &ctx, &mut time, "Colors overlap by", "0.8");
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("overlap").cloned()), Some(json!(0.8)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Shapes));
    assert_eq!(reply(&mut app)["settings"]["overlap"], json!(0.8));
}

/// The thin lines' width typed writes its setting and the window recomputes from the colours (where
/// the areas narrower than it become centre lines), the trace kept.
#[test]
fn the_thin_lines_width_typed_recomputes_from_the_colours() {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(3));
    let before = made_by(&app);
    type_into(&mut app, &ctx, &mut time, "Thin lines sewn along their middle below", "2");
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("thinLine").cloned()), Some(json!(2.0)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Colours));
    assert_eq!(reply(&mut app)["settings"]["thinLine"], json!(2.0));
}

/// A Shapes value past the engine's range is refused: the status bar says the range, and the
/// setting keeps what it held.
#[test]
fn a_shapes_value_out_of_range_is_refused_and_kept() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(3));
    type_into(&mut app, &ctx, &mut time, "Colors overlap by", "5");
    assert_eq!(app.ui.status, "5 mm is out of range: 0 mm to 2 mm.");
    type_into(&mut app, &ctx, &mut time, "Smoothness", "150");
    assert_eq!(app.ui.status, "150 is out of range: 0 to 100.");
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.fields.get("overlap"), d.fields.get("smoothing")), (None, None));
}

/// A choice made in a row's list: the row's button screen readers call `name`, then its option
/// `option`.
fn choose(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, option: &str) {
    click_named(app, ctx, time, name);
    click_named(app, ctx, time, option);
}

/// The note saying that Create sets the window's document settings in the document.
const SETS_THEM: &str = "Making the objects sets these in the document's embroidery settings.";

/// Whether `nodes` say `words`.
fn says(nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)], words: &str) -> bool {
    nodes.iter().any(|(_, n)| n.value() == Some(words))
}

/// The Stitches step's fabric is the one the design is planned with, from the objects on, the
/// window saying that Create sets it in the document; Create makes it the document's in the same
/// undo step (the project owner's choice, 2026-11-12, as the palette); the document's own again
/// changes nothing.
#[test]
fn the_stitches_steps_fabric_is_planned_and_made_the_documents_on_create() {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    assert!(!says(&nodes(&mut app, &ctx, &mut time), SETS_THEM), "nothing changed yet");
    choose(&mut app, &ctx, &mut time, "Fabric", "Built-in values");
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("fabric").cloned()), None, "the document's own: no change");
    let before = made_by(&app);
    choose(&mut app, &ctx, &mut time, "Fabric", "Denim or heavy twill");
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("fabric").cloned()), Some(json!("denim")));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Objects));
    assert!(says(&nodes(&mut app, &ctx, &mut time), SETS_THEM));
    let steps = undo_steps(&app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    until(&mut app, &ctx, &mut time, |app| open_dialog(app).is_none());
    assert_eq!(undo_steps(&app), steps + 1, "one undo step");
    let settings = app.session.embroidery_settings().expect("settings");
    assert_eq!(settings.settings.fabric.and_then(|f| f.preset).as_deref(), Some("denim"));
}

/// The fill angle across the shape is what the design is planned with, and Fixed takes the degrees
/// typed; degrees past a full turn are refused and kept.
#[test]
fn the_fill_angle_is_fixed_or_across_the_shape() {
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let angle = |app: &VectorcraftApp| open_dialog(app).and_then(|d| d.fields.get("fillAngle").cloned());
    let before = made_by(&app);
    choose(&mut app, &ctx, &mut time, "Fill angle", "Across the shape");
    assert_eq!(angle(&app), Some(json!("acrossShape")));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Objects));
    assert_eq!(live_mut(&mut app).document_settings().map(|s| s.resolved.fill_angle.value.clone()), Some(json!("acrossShape")), "planned so");
    choose(&mut app, &ctx, &mut time, "Fill angle", "Fixed");
    assert_eq!(angle(&app), None, "the document's own rows again");
    type_into(&mut app, &ctx, &mut time, "Angle", "45");
    assert_eq!(angle(&app), Some(json!(45.0)));
    type_into(&mut app, &ctx, &mut time, "Angle", "500");
    assert_eq!(app.ui.status, "500° is out of range: -360° to 360°.");
    assert_eq!(angle(&app), Some(json!(45.0)), "kept");
}

/// Joining one thread's objects is a checkbox, from the objects on.
#[test]
fn joining_one_threads_objects_is_a_setting() {
    use egui::accesskit::Role;
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let before = made_by(&app);
    click(&mut app, &ctx, &mut time, "Join one thread's objects when nothing lies between them", Role::CheckBox);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("joinSameColor").cloned()), Some(json!(false)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Objects));
    click(&mut app, &ctx, &mut time, "Join one thread's objects when nothing lies between them", Role::CheckBox);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("joinSameColor").cloned()), None, "the document's own again");
}

/// The rules are read only, each with its value and where it comes from: the thin lines from the
/// Shapes step, the narrow parts' satin by default, the rows' spacing by default and then from the
/// fabric chosen, fleece's closer rows (what the step draws: its read-only rows are painted words).
#[test]
fn the_rules_say_where_their_values_come_from() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let drawn = frame_texts(&mut app, &ctx, &mut time);
    for words in ["Thin lines", "Running stitch", "Shapes: below 1 mm", "Narrow parts as satin", "below 7 mm", "Row spacing"] {
        assert!(drawn.iter().any(|w| w == words), "{words} not drawn: {drawn:?}");
    }
    let tatami = |drawn: &[String]| drawn.iter().find(|w| w.starts_with("Tatami, ")).cloned().unwrap_or_default();
    let built_in = tatami(&drawn);
    assert_eq!(drawn.iter().filter(|w| *w == "default").count(), 2, "the narrow parts' and the rows' source: {drawn:?}");
    // Fleece's rows are closer than the built-in ones (its own spacing, from the heavy knit class).
    choose(&mut app, &ctx, &mut time, "Fabric", "Fleece");
    until(&mut app, &ctx, &mut time, ready);
    let drawn = frame_texts(&mut app, &ctx, &mut time);
    assert!(drawn.iter().any(|w| w == "fabric"), "the rows' spacing from the fabric: {drawn:?}");
    assert_eq!((built_in.as_str(), tatami(&drawn).as_str()), ("Tatami, 0.38 mm", "Tatami, 0.33 mm"));
}

/// The outlines are two checkboxes, both off by default (the project owner's choice, 2026-11-12):
/// each reruns from the objects; the colour outlines say what they came to and the darkest thread
/// they are sewn in; the border shows its width, 5 mm typed taken and 12 mm refused and kept.
#[test]
fn the_outlines_are_two_checkboxes_off_by_default() {
    use egui::accesskit::Role;
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let field = |app: &VectorcraftApp, key: &str| open_dialog(app).and_then(|d| d.fields.get(key).cloned());
    let r = reply(&mut app);
    assert_eq!(
        (r["settings"]["colorOutlines"].clone(), r["settings"]["border"].clone(), r["outlined"].clone()),
        (json!(false), json!(false), Value::Null)
    );
    let before = made_by(&app);
    click(&mut app, &ctx, &mut time, "Color outlines (running stitch)", Role::CheckBox);
    assert_eq!(field(&app, "colorOutlines"), Some(json!(true)));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Objects));
    let r = reply(&mut app);
    assert!(r["outlined"]["colorOutlines"]["lines"].as_u64() >= Some(1), "{}", r["outlined"]);
    let drawn = frame_texts(&mut app, &ctx, &mut time);
    assert!(drawn.iter().any(|w| w.starts_with("Color outlines: ")), "{drawn:?}");
    assert!(drawn.iter().any(|w| w.ends_with(" · darkest of the design")), "{drawn:?}");
    click(&mut app, &ctx, &mut time, "Design border (satin)", Role::CheckBox);
    assert_eq!(field(&app, "border"), Some(json!(true)));
    until(&mut app, &ctx, &mut time, ready);
    type_into(&mut app, &ctx, &mut time, "Border width", "5");
    assert_eq!(field(&app, "borderWidth"), Some(json!(5.0)));
    type_into(&mut app, &ctx, &mut time, "Border width", "12");
    assert_eq!(app.ui.status, "12 mm is out of range: 1 mm to 10 mm.");
    assert_eq!(field(&app, "borderWidth"), Some(json!(5.0)), "kept");
}

/// The outlines' thread is chosen from Choose Thread…'s list, the darkest threads first, and goes
/// back to the darkest of the design's with ↺.
#[test]
fn the_outlines_thread_is_chosen_and_goes_back_to_the_darkest() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorOutlines", json!(true));
    until(&mut app, &ctx, &mut time, ready);
    let darkest = reply(&mut app)["outlined"]["thread"]["index"].clone();
    click(&mut app, &ctx, &mut time, "Choose Thread…", Role::Button);
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let mut listed: Vec<&egui::accesskit::Node> =
        nodes_now.iter().map(|(_, n)| n).filter(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("  JEF "))).collect();
    listed.sort_by(|a, b| a.bounds().map_or(0.0, |r| r.y0).total_cmp(&b.bounds().map_or(0.0, |r| r.y0)));
    let other = listed.iter().filter_map(|n| n.label()).find(|l| !l.ends_with(&format!(" {darkest}"))).expect("another thread").to_string();
    let index: u64 = other.rsplit(' ').next().and_then(|n| n.parse().ok()).expect("a number");
    click(&mut app, &ctx, &mut time, &other, Role::Button);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("outlineThread").cloned()), Some(json!({"palette": "jef", "index": index})));
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(reply(&mut app)["outlined"]["thread"]["index"], json!(index));
    click(&mut app, &ctx, &mut time, "Back to the darkest thread", Role::Button);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("outlineThread").cloned()), None);
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(reply(&mut app)["outlined"]["thread"]["index"], darkest);
}

/// A few frames on a screen `screen` points large (the column learns its content's size, the window
/// its own) → the last one's shapes, flattened, the window and its settings column.
pub(super) fn drawn_on(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, screen: egui::Vec2) -> (Vec<egui::Shape>, Rect, Rect) {
    fn flatten(s: egui::Shape, out: &mut Vec<egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let mut shapes = Vec::new();
    for _ in 0..3 {
        *time += 0.05;
        let raw = egui::RawInput { time: Some(*time), screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), screen)), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            crate::dialogs::show(app, ui.ctx());
        });
        out.textures_delta.clear();
        shapes = Vec::new();
        out.shapes.into_iter().for_each(|c| flatten(c.shape, &mut shapes));
    }
    let window = ctx.memory(|m| m.area_rect(egui::Id::unique("embroidery-digitize-image"))).expect("the window drawn");
    (shapes, window, frame::regions(window.shrink(17.0)).settings)
}

/// The Live bar's first note is drawn whole where the bar has room beside its buttons, on up to two
/// rows (it was cut mid-word at 300 points with room to spare): the test picture's coarse note, in
/// English and Polish.
#[test]
fn the_live_bars_note_is_drawn_whole_where_the_bar_has_room() {
    for lang in ["en", "pl"] {
        let mut app = app();
        app.session.prefs.interface_language = lang.into();
        let (mut app, ctx, mut time) = opened_with(app);
        until(&mut app, &ctx, &mut time, ready);
        let code = crate::i18n::Lang::from_code(lang).expect("a language");
        let first = super::live_bar::current_notes(live_mut(&mut app), code).first().map(|n| n.words.clone()).expect("a note");
        // Longer than the 300 points it was cut at: about 60 characters at the chip's size.
        const LONGER_THAN_THE_OLD_ROOM: usize = 70;
        assert!(first.chars().count() > LONGER_THAN_THE_OLD_ROOM, "{lang}: a long note: {first}");
        let (shapes, window, _) = drawn_on(&mut app, &ctx, &mut time, vec2(1440.0, 900.0));
        crate::i18n::set_current(crate::i18n::Lang::EN);
        let galley = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == first => Some((t.galley.clone(), t.pos)),
                _ => None,
            })
            .expect("the note drawn");
        assert!(!galley.0.elided, "{lang}: whole: {first}");
        let bar = frame::regions(window.shrink(17.0)).bar;
        assert!(bar.contains_rect(Rect::from_min_size(galley.1, galley.0.size())), "{lang}: inside the bar");
    }
}

/// The window open in language `lang` on step 4, its outlines and border on, their result shown.
fn step_four_outlined(lang: &str) -> (VectorcraftApp, egui::Context, f64) {
    let mut app = app();
    app.session.prefs.interface_language = lang.into();
    let (mut app, ctx, mut time) = opened_with(app);
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorOutlines", json!(true));
    set(&mut app, "border", json!(true));
    until(&mut app, &ctx, &mut time, ready);
    (app, ctx, time)
}

/// Step 4's rows are drawn whole at the window's usual size, in English and in Polish: the fabric
/// chosen, the rules' values, the outlines' rows and their thread (a cut galley keeps its whole
/// text, marked elided).
#[test]
fn step_four_draws_its_rows_whole() {
    for lang in ["en", "pl"] {
        let (mut app, ctx, mut time) = step_four_outlined(lang);
        let (shapes, _, settings) = drawn_on(&mut app, &ctx, &mut time, vec2(1440.0, 900.0));
        let cut: Vec<String> = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) if t.galley.elided && settings.x_range().contains(t.visual_bounding_rect().center().x) => {
                    Some(t.galley.text().to_string())
                }
                _ => None,
            })
            .collect();
        crate::i18n::set_current(crate::i18n::Lang::EN);
        assert!(cut.is_empty(), "{lang}: cut short: {cut:?}");
    }
}

/// The outlines' Choose Thread… list shows each thread's colour beside its name, as the colour
/// list's does: the darkest threads, first in it, drawn as swatches.
#[test]
fn the_outlines_thread_list_shows_each_threads_colour() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorOutlines", json!(true));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Choose Thread…", Role::Button);
    let (shapes, ..) = drawn_on(&mut app, &ctx, &mut time, vec2(1440.0, 900.0));
    let fills: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Circle(c) => Some(c.fill),
            _ => None,
        })
        .collect();
    for thread in super::colour_list::picker_rows("jef", "#000000", "").iter().take(3) {
        let [r, g, b] = thread.color;
        assert!(fills.contains(&egui::Color32::from_rgb(r, g, b)), "{} not drawn: {fills:?}", thread.name);
    }
}

/// Turning step 4's outlines on keeps the window its size, at its smallest (the compact column) and
/// at its usual size, in English and in Polish: the outlines' thread row wraps rather than widening
/// the column.
#[test]
fn step_fours_outlines_keep_the_window_its_size() {
    for lang in ["en", "pl"] {
        for screen in [vec2(1000.0, 650.0), vec2(1440.0, 900.0)] {
            let mut app = app();
            app.session.prefs.interface_language = lang.into();
            let (mut app, ctx, mut time) = opened_with(app);
            set(&mut app, STEP, json!(4));
            let before = drawn_on(&mut app, &ctx, &mut time, screen).1.size();
            set(&mut app, "colorOutlines", json!(true));
            set(&mut app, "border", json!(true));
            until(&mut app, &ctx, &mut time, ready);
            let after = drawn_on(&mut app, &ctx, &mut time, screen).1.size();
            crate::i18n::set_current(crate::i18n::Lang::EN);
            assert_eq!(after.round(), before.round(), "{lang} on {screen:?}");
        }
    }
}

/// Embroidery Settings… steps the window aside for Document Setup's Embroidery tab, its settings
/// and its runs kept; Cancel brings it back as it was, nothing recomputed (the project owner's
/// choice, 2026-11-12).
#[test]
fn embroidery_settings_steps_aside_and_cancel_brings_it_back() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "fillAngle", json!("acrossShape"));
    until(&mut app, &ctx, &mut time, ready);
    let before = made_by(&app);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    let d = app.ui.dialog.clone().expect("Document Setup");
    assert_eq!((d.kind == KIND, d.fields.get("tab").cloned()), (false, Some(json!("Embroidery"))));
    assert!(app.embroidery.image_window.live.is_some(), "its runs kept meanwhile");
    crate::dialogs::cancel(&mut app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    let d = open_dialog(&app).expect("the window back");
    assert_eq!(d.fields.get("fillAngle"), Some(&json!("acrossShape")), "as it was left");
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_by(&app), before, "nothing recomputed");
}

/// Document Setup's OK brings the window back too, recomputing from the objects on with the
/// document's settings as they now are.
#[test]
fn embroidery_settings_ok_brings_it_back_recomputed_from_the_objects() {
    use egui::accesskit::Role;
    use vectorcraft_engine::cmd::embroidery::DigitizeStage;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let before = made_by(&app);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    app.ui.dialog.as_mut().expect("Document Setup").fields.insert("embroidery.threadWeight".into(), json!(30));
    crate::dialogs::confirm(&mut app).expect("OK");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(open_dialog(&app).is_some(), "the window back");
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(made_again_from(&app, &before), Some(DigitizeStage::Objects));
    assert_eq!(app.session.embroidery_settings().expect("settings").settings.thread_weight, Some(30.0));
}

/// The document's embroidery settings, as the document keeps them, in their JSON.
fn document_settings_json(app: &VectorcraftApp) -> Value {
    serde_json::to_value(&app.session.embroidery_settings().expect("settings").settings).unwrap_or_default()
}

/// Field `key` of the open dialog, whichever it is (Document Setup's while the window is aside).
fn dialog_field(app: &VectorcraftApp, key: &str) -> Option<Value> {
    app.ui.dialog.as_ref().and_then(|d| d.fields.get(key).cloned())
}

/// Field `key` of the window, open again.
pub(super) fn window_field(app: &VectorcraftApp, key: &str) -> Option<Value> {
    open_dialog(app).and_then(|d| d.fields.get(key).cloned())
}

/// Document Setup's OK, and the frame after it that brings the window back.
fn setup_ok(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) {
    crate::dialogs::confirm(app).expect("OK");
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// Create with the window's fields as they are (Cmd/Ctrl+Enter), until the window has closed.
fn create_now(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) {
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::Enter, Modifiers::COMMAND)]);
    until(app, ctx, time, |app| open_dialog(app).is_none());
}

/// Embroidery Settings… opens Document Setup showing what the window plans with, the window's own
/// fabric among it; a fabric chosen there and confirmed with OK is the document's, the window lets
/// go of its own and plans with the document's, and Create keeps it (the project owner's choice,
/// 2026-11-15).
#[test]
fn a_fabric_chosen_in_document_setup_is_the_documents_after_create() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    choose(&mut app, &ctx, &mut time, "Fabric", "Denim or heavy twill");
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    assert_eq!(dialog_field(&app, "embroidery.fabric"), Some(json!("denim")), "Document Setup shows the window's fabric");
    choose(&mut app, &ctx, &mut time, "Fabric", "Fleece");
    setup_ok(&mut app, &ctx, &mut time);
    assert_eq!(window_field(&app, "fabric"), None, "the window's own fabric let go");
    until(&mut app, &ctx, &mut time, ready);
    create_now(&mut app, &ctx, &mut time);
    let fabric = app.session.embroidery_settings().expect("settings").settings.fabric.and_then(|f| f.preset);
    assert_eq!(fabric.as_deref(), Some("fleece"));
}

/// A thread palette chosen in Document Setup through Embroidery Settings… and confirmed with OK is
/// the document's and the window's: the window matches the threads to it, and Create keeps it.
#[test]
fn a_palette_chosen_in_document_setup_is_the_documents_after_create() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    assert_eq!(dialog_field(&app, "embroidery.palette"), Some(json!("jef")), "the window's palette shown");
    app.ui.dialog.as_mut().expect("Document Setup").fields.insert("embroidery.palette".into(), json!("pec"));
    setup_ok(&mut app, &ctx, &mut time);
    assert_eq!(window_field(&app, "palette"), Some(json!("pec")), "the window matches to the document's palette");
    until(&mut app, &ctx, &mut time, ready);
    assert!(reply(&mut app)["colors"].as_array().unwrap().iter().filter(|c| c["sewn"] == true).all(|c| c["thread"]["palette"] == "pec"));
    create_now(&mut app, &ctx, &mut time);
    assert_eq!(document_settings_json(&app)["palette"], json!("pec"));
}

/// The fill angle and joining one thread's objects, shown in Document Setup as the window holds
/// them, chosen there and confirmed with OK, are the document's after Create.
#[test]
fn the_fill_angle_and_joining_chosen_in_document_setup_are_the_documents_after_create() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "fillAngle", json!("acrossShape"));
    set(&mut app, "joinSameColor", json!(false));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    assert_eq!(
        (dialog_field(&app, "embroidery.fillAngle"), dialog_field(&app, "embroidery.joinSameColor")),
        (Some(json!("acrossShape")), Some(json!(false))),
        "the window's shown"
    );
    let d = app.ui.dialog.as_mut().expect("Document Setup");
    d.fields.insert("embroidery.fillAngle".into(), json!(45.0));
    d.fields.insert("embroidery.joinSameColor".into(), json!(true));
    setup_ok(&mut app, &ctx, &mut time);
    assert_eq!((window_field(&app, "fillAngle"), window_field(&app, "joinSameColor")), (None, None), "the window's own let go");
    until(&mut app, &ctx, &mut time, ready);
    create_now(&mut app, &ctx, &mut time);
    let resolved = app.session.embroidery_settings().expect("settings").resolved;
    assert_eq!((resolved.fill_angle.value, resolved.join_same_color.value), (json!(45.0), true));
}

/// Document Setup cancelled leaves the document and the window's own settings as they were: the
/// window plans with its own fabric, palette, fill angle and joining still, and Create sets them.
#[test]
fn document_setup_cancelled_leaves_the_windows_own_settings() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let own = json!({"fabric": "denim", "palette": "pec", "fillAngle": "acrossShape", "joinSameColor": false});
    for (k, v) in own.as_object().unwrap() {
        set(&mut app, k, v.clone());
    }
    until(&mut app, &ctx, &mut time, ready);
    let before = document_settings_json(&app);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    choose(&mut app, &ctx, &mut time, "Fabric", "Fleece");
    crate::dialogs::cancel(&mut app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    for (k, v) in own.as_object().unwrap() {
        assert_eq!(window_field(&app, k).as_ref(), Some(v), "{k} kept");
    }
    assert_eq!(document_settings_json(&app), before, "the document as it was");
    until(&mut app, &ctx, &mut time, ready);
    create_now(&mut app, &ctx, &mut time);
    let s = app.session.embroidery_settings().expect("settings");
    assert_eq!(s.settings.fabric.and_then(|f| f.preset).as_deref(), Some("denim"));
    assert_eq!(document_settings_json(&app)["palette"], json!("pec"));
}

/// The document's own palette chosen back in Document Setup, over the window's, is the window's
/// too: OK makes what Document Setup holds the document's, even where that changes nothing in the
/// document.
#[test]
fn the_documents_own_palette_chosen_back_in_document_setup_is_the_windows_too() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "palette", json!("pec"));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    assert_eq!(dialog_field(&app, "embroidery.palette"), Some(json!("pec")), "the window's palette shown");
    app.ui.dialog.as_mut().expect("Document Setup").fields.insert("embroidery.palette".into(), json!("jef"));
    setup_ok(&mut app, &ctx, &mut time);
    assert_eq!(window_field(&app, "palette"), Some(json!("jef")));
}

/// An OK given earlier to Document Setup opened from the menu leaves a later Cancel from the
/// window's Embroidery Settings… a Cancel: the window still holds its own fabric, and Create sets
/// it.
#[test]
fn an_earlier_document_setup_ok_leaves_a_later_cancel_a_cancel() {
    use egui::accesskit::Role;
    let mut app = app();
    app.run("file.documentSetup", json!({})).expect("Document Setup from the menu");
    crate::dialogs::confirm(&mut app).expect("OK");
    let (mut app, ctx, mut time) = opened_with(app);
    set(&mut app, STEP, json!(4));
    choose(&mut app, &ctx, &mut time, "Fabric", "Denim or heavy twill");
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    crate::dialogs::cancel(&mut app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert_eq!(window_field(&app, "fabric"), Some(json!("denim")), "the window's own fabric kept");
    until(&mut app, &ctx, &mut time, ready);
    create_now(&mut app, &ctx, &mut time);
    let fabric = app.session.embroidery_settings().expect("settings").settings.fabric.and_then(|f| f.preset);
    assert_eq!(fabric.as_deref(), Some("denim"));
}

/// Another palette chosen in Document Setup and confirmed drops the threads the window had chosen
/// of the old one, a colour's and the outlines', which the engine would refuse: the window's next
/// run is made, and Create works on the new palette.
#[test]
fn another_palette_chosen_in_document_setup_drops_the_old_palettes_threads() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorChoices", json!([{"color": "#f08214", "thread": {"palette": "jef", "index": 5}}]));
    set(&mut app, "colorOutlines", json!(true));
    set(&mut app, "outlineThread", json!({"palette": "jef", "index": 2}));
    until(&mut app, &ctx, &mut time, ready);
    // The outlines' rows put the button below the part of the step in view.
    press(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    assert_eq!(dialog_field(&app, "embroidery.palette"), Some(json!("jef")), "Document Setup open, the window's palette shown");
    app.ui.dialog.as_mut().expect("Document Setup").fields.insert("embroidery.palette".into(), json!("pec"));
    setup_ok(&mut app, &ctx, &mut time);
    let threads = |app: &VectorcraftApp| (window_field(app, "outlineThread"), window_field(app, "colorChoices"));
    assert_eq!(threads(&app), (None, None), "the old palette's threads gone");
    until(&mut app, &ctx, &mut time, ready);
    create_now(&mut app, &ctx, &mut time);
    assert_eq!(document_settings_json(&app)["palette"], json!("pec"));
}

/// "Built-in values" chosen in the window over a preset the document has plans with no fabric, and
/// Create unsets the document's.
#[test]
fn built_in_values_chosen_over_a_document_preset_unset_it_on_create() {
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": "denim"})).unwrap();
    let (mut app, ctx, mut time) = opened_with(app);
    set(&mut app, STEP, json!(4));
    choose(&mut app, &ctx, &mut time, "Fabric", "Built-in values");
    assert_eq!(window_field(&app, "fabric"), Some(json!("builtIn")));
    until(&mut app, &ctx, &mut time, ready);
    assert!(live_mut(&mut app).document_settings().is_some_and(|s| s.settings.fabric.is_none()), "planned with no fabric");
    create_now(&mut app, &ctx, &mut time);
    assert!(app.session.embroidery_settings().expect("settings").settings.fabric.is_none(), "the document's fabric unset");
}

/// After Document Setup's OK changed the document's fabric, the window's fabric row shows the
/// document's as it now is.
#[test]
fn the_rows_show_the_documents_settings_after_document_setup_changed_them() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    choose(&mut app, &ctx, &mut time, "Fabric", "Fleece");
    setup_ok(&mut app, &ctx, &mut time);
    until(&mut app, &ctx, &mut time, ready);
    let drawn = frame_texts(&mut app, &ctx, &mut time);
    assert!(drawn.iter().any(|w| w == "Fleece"), "the fabric row: {drawn:?}");
}

/// The document switched while Document Setup is open over the window: the window doesn't wait to
/// come back to a document no longer active; its runs stop and its settings are remembered for its
/// image.
#[test]
fn a_document_switched_while_document_setup_is_open_closes_the_window() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    let (uid, image) = (live(&app).uid, live(&app).image);
    set(&mut app, STEP, json!(4));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(app.embroidery.image_window.live.is_none(), "its runs stopped");
    let remembered = app.embroidery.image_window.remembered.get(&(uid, image)).and_then(|r| r.fields.get(STEP).cloned());
    assert_eq!(remembered, Some(json!(4)), "its settings remembered");
}

/// The double-outline warning is the Stitches step's, where the outlines are set: a thin navy line
/// across an orange band, sewn along its middle, with the colour outlines on beside it.
#[test]
fn the_double_outline_warning_is_the_stitches_steps() {
    use crate::embroidery::marks::Mark;
    let line = image::RgbaImage::from_fn(200, 100, |x, y| {
        image::Rgba(match ((20..180).contains(&x), (49..51).contains(&y), (10..90).contains(&y)) {
            (true, true, _) => [20, 40, 120, 255],
            (true, _, true) => [240, 130, 20, 255],
            _ => [255, 255, 255, 255],
        })
    });
    let (mut app, ctx, mut time) = opened_with(app_with(line));
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorOutlines", json!(true));
    until(&mut app, &ctx, &mut time, ready);
    let warnings = reply(&mut app)["warnings"].clone();
    assert!(warnings.as_array().unwrap().iter().any(|w| w["id"] == "outlinesBesideLines"), "{warnings}");
    let notes = super::live_bar::current_notes(live_mut(&mut app), crate::i18n::Lang::EN);
    let beside = notes.iter().find(|n| n.words.starts_with("The color outlines run beside")).expect("the warning's note");
    assert!(beside.step == 4 && matches!(beside.mark, Mark::Warning), "{beside:?}");
}

/// How many of Choose Thread…'s first rows a test compares: those the list shows before it scrolls.
const FIRST_ROWS: usize = 5;

/// The outlines' Choose Thread… lists the palette's threads darkest first, by CIE L*, as the
/// outlines take the darkest of a design's threads.
#[test]
fn the_outlines_thread_list_is_darkest_first() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    set(&mut app, "colorOutlines", json!(true));
    until(&mut app, &ctx, &mut time, ready);
    click(&mut app, &ctx, &mut time, "Choose Thread…", Role::Button);
    let nodes_now = nodes(&mut app, &ctx, &mut time);
    let mut listed: Vec<&egui::accesskit::Node> =
        nodes_now.iter().map(|(_, n)| n).filter(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("  JEF "))).collect();
    listed.sort_by(|a, b| a.bounds().map_or(0.0, |r| r.y0).total_cmp(&b.bounds().map_or(0.0, |r| r.y0)));
    let shown: Vec<usize> = listed.iter().take(FIRST_ROWS).filter_map(|n| n.label()?.rsplit(' ').next()?.parse().ok()).collect();
    let darkest: Vec<usize> = vectorcraft_engine::cmd::embroidery::threads_darkest_first("jef").iter().take(FIRST_ROWS).map(|t| t.index).collect();
    assert_eq!(shown, darkest);
}

/// A thread chosen for the outlines is of the palette it was chosen in: choosing another palette
/// leaves it out with the colours' threads, so the run that follows isn't refused for it; the
/// outlines then take the darkest thread of the new palette.
#[test]
fn another_palette_leaves_out_the_outlines_thread_chosen() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(2));
    set(&mut app, "colorOutlines", json!(true));
    set(&mut app, "outlineThread", json!({"palette": "jef", "index": 2}));
    until(&mut app, &ctx, &mut time, ready);
    click_named(&mut app, &ctx, &mut time, "Match to threads");
    click_named(&mut app, &ctx, &mut time, "Brother (PES) · 61 threads");
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("outlineThread").cloned()), None, "left out");
    until(&mut app, &ctx, &mut time, ready);
    assert_eq!(reply(&mut app)["outlined"]["thread"]["palette"], json!("pec"));
}

/// A Create waiting for its run when Embroidery Settings… is pressed is let go: back from Document
/// Setup, the window makes nothing until Create is asked again.
#[test]
fn embroidery_settings_lets_a_waiting_create_go() {
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(4));
    let steps = undo_steps(&app);
    // Stepped aside in the frame Create was asked in (no frame between, which would make it).
    live_mut(&mut app).create_waiting = true;
    let fields = open_dialog(&app).map(|d| d.fields.clone()).unwrap_or_default();
    super::step_aside(&mut app, &fields);
    assert!(!live(&app).create_waiting, "let go");
    crate::dialogs::cancel(&mut app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    until(&mut app, &ctx, &mut time, ready);
    for _ in 0..5 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert!(open_dialog(&app).is_some() && undo_steps(&app) == steps, "nothing made");
}
