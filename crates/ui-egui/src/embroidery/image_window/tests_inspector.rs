//! The area inspector (`super::inspector`) through whole app frames: opened by a click on an area of
//! the preview and closed on the background, its choices by the area's kind, Satin greyed out with
//! its reason, a choice made a change of the window's that the next result sews, the angle and its
//! ↺, Escape closing it before the window, and following its area through a recompute.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use serde_json::json;
use vectorcraft_engine::cmd::embroidery::AreaFacts;

use super::tests::{app_with, frame, key, live, nodes, open_dialog, opened_with, press, ready, reply, set, type_into, until, window_field};
use crate::VectorcraftApp;

/// A picture placed at its 72 ppi (a pixel 0.35 mm): an orange square 35 × 28 mm, a navy bar
/// 21 × 3.5 mm and a navy line 0.7 mm wide (sewn along its middle), on white.
fn picture() -> image::RgbaImage {
    image::RgbaImage::from_fn(200, 100, |x, y| {
        let inside = |[x0, y0, x1, y1]: [u32; 4]| (x0..x1).contains(&x) && (y0..y1).contains(&y);
        image::Rgba(if inside([10, 10, 110, 90]) {
            [240, 130, 20, 255]
        } else if inside([130, 40, 190, 50]) || inside([150, 60, 152, 95]) {
            [20, 40, 120, 255]
        } else {
            [255, 255, 255, 255]
        })
    })
}

/// The window open on [`picture`], its first result shown.
fn opened() -> (VectorcraftApp, egui::Context, f64) {
    let (mut app, ctx, mut time) = opened_with(app_with(picture()));
    until(&mut app, &ctx, &mut time, ready);
    (app, ctx, time)
}

/// Where the middle of working pixel `(x, y)` is drawn in the preview, as it last drew.
fn screen_of(app: &VectorcraftApp, x: u32, y: u32) -> egui::Pos2 {
    let l = live(app);
    let (_, xf) = l.preview.last_view.expect("the preview drawn");
    let to_doc = l.schedule.shown.as_ref().and_then(|s| s.outputs.working_to_doc()).expect("a result");
    xf.to_screen(to_doc * vectorcraft_geom::Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5))
}

/// A click on working pixel `(x, y)` of the preview.
fn click_at(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, x: u32, y: u32) {
    let at = screen_of(app, x, y);
    let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::PointerMoved(at), button(true), button(false)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![]);
}

/// The area the inspector is open on, as its key finds it in the result shown.
fn inspected(app: &VectorcraftApp) -> Option<AreaFacts> {
    let l = live(app);
    let outputs = &l.schedule.shown.as_ref()?.outputs;
    let (x, y) = outputs.find(&l.inspector.as_ref()?.key)?;
    outputs.area_at(x, y)
}

/// The inspector's stitch choices as screen readers read them, in the words' order: each tab's name,
/// and whether it can be chosen.
fn choices(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<(String, bool)> {
    let words = ["Auto", "Tatami", "Running stitch", "Satin", "Don't sew"];
    let mut found: Vec<(String, bool)> = nodes(app, ctx, time)
        .iter()
        .filter(|(_, n)| n.role() == Role::Tab && n.label().is_some_and(|l| words.contains(&l)))
        .map(|(_, n)| (n.label().unwrap_or_default().to_string(), !n.is_disabled()))
        .collect();
    found.sort_by_key(|(w, _)| words.iter().position(|x| x == w));
    found
}

/// Whether a text drawn in the frame says `words`.
fn says(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, words: &str) -> bool {
    nodes(app, ctx, time).iter().any(|(_, n)| n.value().is_some_and(|v| v.contains(words)) || n.label().is_some_and(|l| l.contains(words)))
}

#[test]
fn a_click_on_an_area_opens_its_inspector_and_one_on_the_background_closes_it() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    let facts = inspected(&app).expect("open on the navy bar");
    assert!(!facts.line && facts.thread_name.is_some(), "{facts:?}");
    let name = super::preview::area_name(&facts);
    assert!(says(&mut app, &ctx, &mut time, &name), "its name, {name}, drawn");
    click_at(&mut app, &ctx, &mut time, 3, 3);
    assert!(live(&app).inspector.is_none(), "closed on the background");
}

#[test]
fn the_choices_follow_the_areas_kind() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    let fill: Vec<String> = choices(&mut app, &ctx, &mut time).into_iter().map(|(w, _)| w).collect();
    assert_eq!(fill, ["Auto", "Tatami", "Satin", "Don't sew"], "a filled area's");
    click_at(&mut app, &ctx, &mut time, 150, 80);
    assert!(inspected(&app).is_some_and(|f| f.line), "the line inspected");
    let line: Vec<String> = choices(&mut app, &ctx, &mut time).into_iter().map(|(w, _)| w).collect();
    assert_eq!(line, ["Auto", "Running stitch", "Satin", "Don't sew"], "a line's");
}

#[test]
fn satin_is_greyed_out_with_its_reason_where_it_cant_sew_the_area() {
    let (mut app, ctx, mut time) = opened();
    // Each area clicked left of the popover the one before it opened (it lies beside its area, to
    // its right, over what is there).
    // The navy bar, 3.5 mm: offered.
    click_at(&mut app, &ctx, &mut time, 160, 45);
    assert!(choices(&mut app, &ctx, &mut time).contains(&("Satin".to_string(), true)), "offered");
    // The line, 0.7 mm: narrower than a satin column.
    click_at(&mut app, &ctx, &mut time, 150, 80);
    assert!(choices(&mut app, &ctx, &mut time).contains(&("Satin".to_string(), false)), "greyed out on the line");
    assert!(says(&mut app, &ctx, &mut time, "Satin needs a line at least 1 mm wide"));
    // The orange square, 28 mm at its widest.
    click_at(&mut app, &ctx, &mut time, 60, 50);
    assert!(choices(&mut app, &ctx, &mut time).contains(&("Satin".to_string(), false)), "greyed out on the square");
    assert!(says(&mut app, &ctx, &mut time, "Satin sews parts up to 12 mm"), "its reason");
}

#[test]
fn a_stitch_chosen_is_a_change_the_next_result_sews_and_auto_removes_it() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    let changes = window_field(&app, "areaChanges").expect("a change");
    assert_eq!((&changes[0]["stitch"], &changes[0]["kind"], changes.as_array().map(Vec::len)), (&json!("tatami"), &json!("fill"), Some(1)));
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    assert_eq!(r["areaChanges"][0]["applies"], json!(true), "{}", r["areaChanges"]);
    // Chosen again, the same change is set, not a second one.
    press(&mut app, &ctx, &mut time, "Don't sew", Role::Tab);
    assert_eq!(window_field(&app, "areaChanges").and_then(|c| c.as_array().map(Vec::len)), Some(1));
    press(&mut app, &ctx, &mut time, "Auto", Role::Tab);
    assert_eq!(window_field(&app, "areaChanges"), None, "Auto with no angle: no change");
}

#[test]
fn an_angle_typed_is_the_areas_and_its_reset_goes_back_to_auto() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 60, 50);
    type_into(&mut app, &ctx, &mut time, "Angle", "30");
    let angle = window_field(&app, "areaChanges").and_then(|c| c[0]["angle"].as_f64());
    assert_eq!(angle, Some(30.0));
    press(&mut app, &ctx, &mut time, "Back to Auto", Role::Button);
    assert_eq!(window_field(&app, "areaChanges"), None);
}

#[test]
fn escape_closes_the_inspector_before_the_window() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    assert!(live(&app).inspector.is_some());
    let released = Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape, Modifiers::NONE)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![released.clone()]);
    assert!(live(&app).inspector.is_none() && open_dialog(&app).is_some(), "the inspector closed, the window open");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(open_dialog(&app).is_none(), "then the window");
}

#[test]
fn the_inspector_follows_its_area_through_a_recompute() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    let before = inspected(&app).and_then(|f| f.thread_name);
    set(&mut app, "colors", json!(3));
    until(&mut app, &ctx, &mut time, ready);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(before.is_some() && inspected(&app).and_then(|f| f.thread_name) == before, "the navy bar still");
}

/// A click on another area moves the inspector there (the line, which the bar's popover, lying to
/// the bar's left, leaves uncovered).
#[test]
fn a_click_on_another_area_moves_the_inspector_there() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    click_at(&mut app, &ctx, &mut time, 150, 80);
    let facts = inspected(&app).expect("still open");
    assert!(facts.line, "on the line now: {facts:?}");
    // The click is no click elsewhere that closes it: the popup open in egui's memory.
    assert!(egui::Popup::is_id_open(&ctx, super::inspector::popup_id()), "open in egui's memory");
}

/// A picture placed at its 72 ppi: two orange squares 14 mm a side and a navy one, on white.
fn three_squares() -> image::RgbaImage {
    image::RgbaImage::from_fn(200, 100, |x, y| {
        let inside = |[x0, y0, x1, y1]: [u32; 4]| (x0..x1).contains(&x) && (y0..y1).contains(&y);
        image::Rgba(if inside([10, 10, 50, 50]) || inside([70, 10, 110, 50]) {
            [240, 130, 20, 255]
        } else if inside([130, 10, 170, 50]) {
            [20, 40, 120, 255]
        } else {
            [255, 255, 255, 255]
        })
    })
}

/// A screen reader's click of the control it calls `name`, whatever its role.
fn press_named(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, name: &str) {
    let role = nodes(app, ctx, time).iter().find(|(_, n)| n.label() == Some(name)).map(|(_, n)| n.role()).unwrap_or_else(|| panic!("no {name}"));
    press(app, ctx, time, name, role);
}

/// Apply to Similar Areas sets the area's stitch on each area like it: the two orange squares.
#[test]
fn apply_to_similar_areas_changes_each() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    click_at(&mut app, &ctx, &mut time, 30, 30);
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    press_named(&mut app, &ctx, &mut time, "Apply to 1 Similar Area");
    let changes = window_field(&app, "areaChanges").expect("changes");
    let all = changes.as_array().cloned().unwrap_or_default();
    assert_eq!(all.len(), 2, "{changes}");
    assert!(all.iter().all(|c| c["stitch"] == json!("tatami") && c["color"] == all[0]["color"]), "{changes}");
    until(&mut app, &ctx, &mut time, ready);
    let r = reply(&mut app);
    assert!(r["areaChanges"].as_array().is_some_and(|s| s.iter().all(|s| s["applies"] == json!(true))), "{}", r["areaChanges"]);
}

/// While Apply to Similar Areas is hovered, the preview outlines the areas it would change.
#[test]
fn the_similar_areas_are_outlined_while_the_button_is_hovered() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    click_at(&mut app, &ctx, &mut time, 30, 30);
    let at = super::tests::centre_of(&mut app, &ctx, &mut time, "Apply to 1 Similar Area", Role::Button);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at)]);
    assert!(live(&app).inspector.as_ref().is_some_and(|i| i.similar_shown), "hovered");
}

/// Show in Sewing Order says where the area is sewn, and ▶ moves the inspector to the area sewn
/// next.
#[test]
fn show_in_sewing_order_says_its_place_and_steps_through() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    click_at(&mut app, &ctx, &mut time, 30, 30);
    let first = inspected(&app).and_then(|f| f.number).expect("a number");
    press_named(&mut app, &ctx, &mut time, "Show in Sewing Order");
    assert!(says(&mut app, &ctx, &mut time, "Sewn "), "its place said");
    let name = if first < 3 { "Next area in sewing order" } else { "Previous area in sewing order" };
    press(&mut app, &ctx, &mut time, name, Role::Button);
    let now = inspected(&app).and_then(|f| f.number).expect("still open");
    assert_eq!(now.abs_diff(first), 1, "moved to the area sewn next to it");
    assert!(live(&app).inspector.as_ref().is_some_and(|i| i.order), "still showing the order");
}

/// The number in sewing order of the area the preview's keyboard is on.
fn focused_number(app: &VectorcraftApp) -> Option<usize> {
    let l = live(app);
    let outputs = &l.schedule.shown.as_ref()?.outputs;
    let (x, y) = outputs.find(l.preview.focused.as_ref()?)?;
    outputs.area_at(x, y)?.number
}

/// A frame with key `k` pressed with `modifiers`.
fn keyed(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, k: Key, modifiers: Modifiers) {
    *time += 0.05;
    frame(app, ctx, *time, vec![key(k, modifiers)]);
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers }]);
}

/// `[` and `]` step through the areas in sewing order while the preview has the keyboard, and
/// Enter opens the inspector on the area it is on.
#[test]
fn brackets_step_through_the_areas_in_sewing_order() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    // A click on the background: the preview takes the keyboard, no inspector opens.
    click_at(&mut app, &ctx, &mut time, 190, 90);
    assert!(live(&app).inspector.is_none());
    keyed(&mut app, &ctx, &mut time, Key::CloseBracket, Modifiers::NONE);
    assert_eq!(focused_number(&app), Some(1), "the first area");
    keyed(&mut app, &ctx, &mut time, Key::CloseBracket, Modifiers::NONE);
    assert_eq!(focused_number(&app), Some(2));
    keyed(&mut app, &ctx, &mut time, Key::OpenBracket, Modifiers::NONE);
    assert_eq!(focused_number(&app), Some(1));
    keyed(&mut app, &ctx, &mut time, Key::Enter, Modifiers::NONE);
    assert_eq!(inspected(&app).and_then(|f| f.number), Some(1), "Enter opens the inspector on it");
}

/// Step 4 lists each change, what it sets and its area, or why it applies to none; ↺ removes one,
/// Reset All every one.
#[test]
fn the_list_shows_each_change_and_removes_them() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    set(&mut app, super::STEP, json!(4));
    let changes = json!([
        {"at": [150.5, 30.5], "color": "#142878", "kind": "fill", "stitch": "tatami"},
        {"at": [150.5, 30.5], "color": "#f08214", "kind": "fill", "stitch": "none"},
    ]);
    set(&mut app, "areaChanges", changes);
    until(&mut app, &ctx, &mut time, ready);
    assert!(says(&mut app, &ctx, &mut time, "Another color lies here now"), "the kept one's reason");
    press(&mut app, &ctx, &mut time, "Remove this change", Role::Button);
    assert_eq!(window_field(&app, "areaChanges").and_then(|c| c.as_array().map(Vec::len)), Some(1), "one removed");
    press_named(&mut app, &ctx, &mut time, "Reset All");
    assert_eq!(window_field(&app, "areaChanges"), None, "every one removed");
    assert!(says(&mut app, &ctx, &mut time, "Click an area in the preview"), "how to make one");
}

/// Cmd/Ctrl+Z undoes an area change and a setting in turn, Shift+Cmd/Ctrl+Z redoes; the document's
/// own undo is untouched.
#[test]
fn cmd_z_undoes_a_setting_and_a_change_and_shift_redoes() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    let document = app.session.doc().map(|d| d.history.undo.len()).ok();
    let before = window_field(&app, "colors");
    set(&mut app, "colors", json!(3));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 30, 30);
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    assert!(window_field(&app, "areaChanges").is_some());
    // Escape first, so that the inspector's popover doesn't hold the keyboard.
    keyed(&mut app, &ctx, &mut time, Key::Escape, Modifiers::NONE);
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND);
    assert_eq!((window_field(&app, "areaChanges"), window_field(&app, "colors")), (None, Some(json!(3))), "the change undone");
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND);
    assert_eq!(window_field(&app, "colors"), before, "the setting undone");
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(window_field(&app, "colors"), Some(json!(3)), "redone");
    assert_eq!(app.session.doc().map(|d| d.history.undo.len()).ok(), document, "the document's undo untouched");
}

/// The history survives Embroidery Settings…: a setting changed before the window stepped aside is
/// undone after it comes back.
#[test]
fn the_history_survives_embroidery_settings() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    set(&mut app, super::STEP, json!(4));
    let before = window_field(&app, "colors");
    set(&mut app, "colors", json!(3));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    until(&mut app, &ctx, &mut time, ready);
    press(&mut app, &ctx, &mut time, "Embroidery Settings…", Role::Button);
    crate::dialogs::cancel(&mut app);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(open_dialog(&app).is_some(), "the window back");
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND);
    assert_eq!(window_field(&app, "colors"), before, "undone after coming back");
}
