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
        for a in [r.title, r.strip, r.preview, r.settings, r.bar] {
            assert!(inner.contains_rect(a) && a.width() > 0.0 && a.height() > 0.0, "{a:?} in {inner:?}");
        }
        assert!(!r.preview.intersects(r.settings) && !r.strip.intersects(r.preview) && !r.title.intersects(r.strip) && !r.preview.intersects(r.bar));
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

/// One frame with screen readers on (the context's) → its nodes.
fn nodes(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<(egui::accesskit::NodeId, egui::accesskit::Node)> {
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
fn node<'a>(
    nodes: &'a [(egui::accesskit::NodeId, egui::accesskit::Node)],
    name: &str,
    role: egui::accesskit::Role,
) -> Option<&'a egui::accesskit::Node> {
    let labels: Vec<egui::accesskit::NodeId> =
        nodes.iter().filter(|(_, n)| n.role() == egui::accesskit::Role::Label && n.value() == Some(name)).map(|(id, _)| *id).collect();
    nodes.iter().map(|(_, n)| n).find(|n| n.role() == role && (n.label() == Some(name) || n.labelled_by().iter().any(|l| labels.contains(l))))
}

/// The centre of the control screen readers call `name`, of `role`.
fn centre_of(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, role: egui::accesskit::Role) -> egui::Pos2 {
    let nodes = nodes(app, ctx, time);
    let b = node(&nodes, name, role).and_then(|n| n.bounds()).unwrap_or_else(|| panic!("no {role:?} {name}"));
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

/// A click of the control screen readers call `name`, of `role`.
fn click(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, role: egui::accesskit::Role) {
    let at = centre_of(app, ctx, time, name, role);
    let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::PointerMoved(at), button(true), button(false)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// `text` typed into the field screen readers call `name`, over what it held, then Enter.
fn type_into(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str, text: &str) {
    click(app, ctx, time, name, egui::accesskit::Role::TextInput);
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::A, Modifiers::COMMAND), Event::Text(text.into())]);
    *time += 0.05;
    frame(app, ctx, *time, vec![key(Key::Enter, Modifiers::NONE)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// The window open on the test picture, its first result shown, screen readers on.
fn opened() -> (VectorcraftApp, egui::Context, f64) {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    let mut app = app();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    let mut time = 0.0;
    until(&mut app, &ctx, &mut time, ready);
    (app, ctx, time)
}

/// The reply of the result shown.
fn reply(app: &mut VectorcraftApp) -> Value {
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
/// it found in the corners still left out; Whole Image takes the crop off.
#[test]
fn crop_to_the_art_crops_to_what_isnt_background() {
    use egui::accesskit::Role;
    let (mut app, ctx, mut time) = opened();
    set(&mut app, STEP, json!(1));
    let art = live(&app).schedule.shown.as_ref().and_then(|s| s.outputs.art_bounds());
    assert_eq!(art, Some([20, 10, 180, 90]));
    click(&mut app, &ctx, &mut time, "Crop to the Art", Role::Button);
    let d = open_dialog(&app).expect("open");
    assert_eq!((d.fields.get("crop"), d.fields.get("background")), (Some(&json!([20, 10, 180, 90])), Some(&json!("#ffffff"))));
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    assert_eq!((r["image"]["crop"].clone(), r["colorCount"].clone()), (json!([20, 10, 180, 90]), json!(2)), "{}", r["image"]);
    click(&mut app, &ctx, &mut time, "Whole Image", Role::Button);
    assert_eq!(open_dialog(&app).and_then(|d| d.fields.get("crop").cloned()), None);
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
        nodes.iter().any(|(_, n)| n.value() == Some("Create sets the document's thread palette to Brother (PES)."))
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
