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
