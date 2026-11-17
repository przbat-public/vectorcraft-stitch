//! The area inspector (`super::inspector`) through whole app frames: opened by a click on an area of
//! the preview and closed on the background, its choices by the area's kind, Satin greyed out with
//! its reason, a choice made a change of the window's that the next result sews, the angle and its
//! ↺, Escape closing it before the window, and following its area through a recompute.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use serde_json::json;
use vectorcraft_engine::cmd::embroidery::{AreaFacts, AreaKey, DigitizeStage, MAX_AREA_CHANGES};

use super::tests::{app_with, click, frame, key, live, nodes, open_dialog, opened_with, press, ready, reply, set, type_into, until, window_field};
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

/// How many of the window's texts hold `words`: a label's text run, a node of its own under it
/// holding the same words, not counted apart.
fn times_said(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, words: &str) -> usize {
    let holds = |n: &egui::accesskit::Node| n.value().is_some_and(|v| v.contains(words)) || n.label().is_some_and(|l| l.contains(words));
    nodes(app, ctx, time).iter().filter(|(_, n)| n.role() != Role::TextRun && holds(n)).count()
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

/// One frame of the inspector alone, open on `key` over `outputs`, a run under way (`running`) or
/// not: the inspector as that frame leaves it.
fn inspector_frame(
    outputs: &vectorcraft_engine::cmd::embroidery::DigitizeOutputs,
    key: AreaKey,
    running: bool,
) -> Option<super::inspector::Inspected> {
    let ctx = egui::Context::default();
    let mut inspected = Some(super::inspector::Inspected { key, order: false, similar_shown: false, similar: None, fresh: false });
    let mut fields = serde_json::Map::new();
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1440.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        egui::Popup::open_id(ui.ctx(), super::inspector::popup_id());
        let pane = ui.max_rect();
        super::inspector::show(ui, pane, &mut fields, &mut inspected, outputs, running, |p| egui::pos2(p.x as f32, p.y as f32));
    });
    out.textures_delta.clear();
    inspected
}

/// A run shows its stages as they come, the areas found only once its shapes are done: the inspector
/// waits, open, through a result shown so far without them while the run is under way, and is on
/// its area once they are; a result left without them (a run that failed or was refused) closes it.
#[test]
fn the_inspector_waits_through_a_run_and_closes_on_a_result_left_without_shapes() {
    let (app, _, _) = opened();
    let whole = live(&app).schedule.shown.clone().expect("a result");
    let key = whole.outputs.area_key(160, 45).expect("the navy bar's key");
    // The result as a run shows it after its centre lines, before its shapes.
    let mut so_far = None;
    app.session
        .digitize_pipeline()
        .run(&whole.request, &|_, _| true, &mut |stage, out, _| {
            if stage == DigitizeStage::Lines {
                so_far = Some(out.clone());
            }
        })
        .expect("a run");
    let partial = so_far.expect("the outputs up to the centre lines");
    assert!(inspector_frame(&partial, key.clone(), true).is_some(), "waiting, open, while the run is under way");
    assert!(inspector_frame(&partial, key.clone(), false).is_none(), "a result left without shapes closes it");
    assert!(inspector_frame(&whole.outputs, key, false).is_some_and(|i| i.similar.is_some()), "on its area with the shapes");
}

/// Through whole app frames: a result shown without shapes keeps the inspector while a change waits
/// to run (a heavy one waits for the settings to rest), and closes it once nothing is under way.
#[test]
fn the_window_keeps_the_inspector_only_while_a_run_is_under_way() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    assert!(live(&app).inspector.is_some(), "open on the navy bar");
    let whole = live(&app).schedule.shown.clone().expect("a result");
    let mut so_far = None;
    app.session
        .digitize_pipeline()
        .run(&whole.request, &|_, _| true, &mut |stage, out, _| {
            if stage == DigitizeStage::Lines {
                so_far = Some(out.clone());
            }
        })
        .expect("a run");
    let partial = super::schedule::Shown { outputs: so_far.expect("the outputs up to the centre lines"), whole: false, ..whole.clone() };
    // A heavy change waiting for the settings to rest: under way.
    super::tests::live_mut(&mut app).schedule.time_stages(super::schedule::LIVE_PLAN_MS * 10.0);
    set(&mut app, "smoothing", json!(30));
    time += 0.01;
    frame(&mut app, &ctx, time, vec![]);
    super::tests::live_mut(&mut app).schedule.shown = Some(partial.clone());
    time += 0.01;
    frame(&mut app, &ctx, time, vec![]);
    assert!(live(&app).schedule.busy() && live(&app).inspector.is_some(), "kept while the change waits");
    // Nothing under way: the change taken back before it ran, the result left without shapes.
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    assert!(!live(&app).schedule.busy(), "nothing under way");
    super::tests::live_mut(&mut app).schedule.shown = Some(partial);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(live(&app).inspector.is_none(), "closed: no shapes are coming");
}

/// An angle typed into the inspector and left by a click on another area is the first area's: the
/// inspector takes what was typed in it before it moves.
#[test]
fn an_angle_left_by_a_click_on_another_area_is_the_first_areas() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    click(&mut app, &ctx, &mut time, "Angle", Role::TextInput);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::A, Modifiers::COMMAND), Event::Text("30".into())]);
    click_at(&mut app, &ctx, &mut time, 150, 80);
    assert!(inspected(&app).is_some_and(|f| f.line), "moved to the line");
    let changes = window_field(&app, "areaChanges").expect("the angle set");
    let all = changes.as_array().cloned().unwrap_or_default();
    assert_eq!(all.len(), 1, "{changes}");
    assert_eq!((&all[0]["angle"], &all[0]["kind"], &all[0]["color"]), (&json!(30.0), &json!("fill"), &json!("#142878")), "the bar's: {changes}");
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

/// A picture placed at its 72 ppi: two orange areas of about one size, one 9.9 mm wide, the other
/// 13 mm, on white.
fn narrow_and_wide() -> image::RgbaImage {
    image::RgbaImage::from_fn(200, 100, |x, y| {
        let inside = |[x0, y0, x1, y1]: [u32; 4]| (x0..x1).contains(&x) && (y0..y1).contains(&y);
        image::Rgba(if inside([10, 8, 38, 93]) || inside([60, 18, 97, 83]) { [240, 130, 20, 255] } else { [255, 255, 255, 255] })
    })
}

/// Apply to Similar Areas with Satin sets it only on the areas Satin is offered on: the wider of two
/// alike orange areas isn't, so with Satin there is none to apply it to, with Tatami one.
#[test]
fn apply_to_similar_areas_with_satin_leaves_out_the_areas_too_wide() {
    let (mut app, ctx, mut time) = opened_with(app_with(narrow_and_wide()));
    click_at(&mut app, &ctx, &mut time, 24, 50);
    let similar = |app: &mut VectorcraftApp, time: &mut f64| nodes(app, &ctx, time).iter().any(|(_, n)| n.label() == Some("Apply to 1 Similar Area"));
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    assert!(similar(&mut app, &mut time), "with Tatami: the wider one");
    press(&mut app, &ctx, &mut time, "Satin", Role::Tab);
    assert!(!similar(&mut app, &mut time), "with Satin: none");
}

/// Apply to Similar Areas never makes more changes than the engine takes: with the window holding
/// all but one of the most, it is greyed out.
#[test]
fn apply_to_similar_areas_stops_at_the_most_changes() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    // Changes on the background, applying to none, one short of the most.
    let kept: Vec<serde_json::Value> =
        (0..MAX_AREA_CHANGES - 1).map(|i| json!({"at": [195.5, 95.5], "color": "#f08214", "kind": "fill", "angle": i % 90})).collect();
    set(&mut app, "areaChanges", json!(kept));
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 30, 30);
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    assert_eq!(window_field(&app, "areaChanges").and_then(|c| c.as_array().map(Vec::len)), Some(MAX_AREA_CHANGES), "the most");
    let button = nodes(&mut app, &ctx, &mut time).into_iter().find(|(_, n)| n.label() == Some("Apply to 1 Similar Area")).map(|(_, n)| n);
    assert!(button.is_some_and(|n| n.is_disabled()), "greyed out");
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

/// Show in Sewing Order turned on in an inspector already open grows it to hold its place row while
/// the preview has room for it: the row isn't left scrolled out of sight.
#[test]
fn the_inspector_grows_to_hold_its_place_row() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    let height = |ctx: &egui::Context| ctx.memory(|m| m.area_rect(super::inspector::popup_id())).map(|r| r.height()).expect("the inspector drawn");
    click_at(&mut app, &ctx, &mut time, 30, 30);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    let before = height(&ctx);
    press_named(&mut app, &ctx, &mut time, "Show in Sewing Order");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(height(&ctx) > before, "grown from {before} to hold its place row");
}

/// One frame of the inspector alone, held inside `pane`, over `outputs`, with `events` → the texts
/// screen readers are given, with their boxes, and the inspector's box.
fn inspector_in(
    ctx: &egui::Context,
    pane: egui::Rect,
    outputs: &vectorcraft_engine::cmd::embroidery::DigitizeOutputs,
    inspected: &mut Option<super::inspector::Inspected>,
    time: f64,
    events: Vec<Event>,
) -> (Vec<(String, egui::Rect)>, Option<egui::Rect>) {
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1440.0, 900.0));
    let raw = egui::RawInput { time: Some(time), screen_rect: Some(screen), events, ..Default::default() };
    let mut fields = serde_json::Map::new();
    let mut out = ctx.run_ui(raw, |ui| {
        egui::Popup::open_id(ui.ctx(), super::inspector::popup_id());
        super::inspector::show(ui, pane, &mut fields, inspected, outputs, false, |p| egui::pos2(p.x as f32, p.y as f32));
    });
    out.textures_delta.clear();
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
    let texts = nodes
        .into_iter()
        .filter_map(|(_, n)| {
            let text = n.value().or(n.label())?.to_string();
            let b = n.bounds()?;
            Some((text, egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))))
        })
        .collect();
    (texts, ctx.memory(|m| m.area_rect(super::inspector::popup_id())))
}

/// A popover taller than the preview scrolls; opened on another area by a click, it starts at its
/// top, the area's name in sight, wherever the one before was scrolled to.
#[test]
fn the_inspector_opens_on_another_area_scrolled_to_its_top() {
    let (app, _, _) = opened();
    let shown = live(&app).schedule.shown.clone().expect("a result");
    let outputs = &shown.outputs;
    let name = |x, y| super::preview::area_name(&outputs.area_at(x, y).expect("an area"));
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    // A pane shorter than any popover: its body scrolls.
    let pane = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1440.0, 160.0));
    let fields = serde_json::Map::new();
    let mut inspected = None;
    let mut time = 0.0;
    super::inspector::clicked(&ctx, &mut inspected, &fields, outputs, Some((60, 50)));
    for _ in 0..3 {
        time += 0.05;
        inspector_in(&ctx, pane, outputs, &mut inspected, time, vec![]);
    }
    // The orange square's popover scrolled down, its name out of sight.
    let (_, popover) = inspector_in(&ctx, pane, outputs, &mut inspected, time, vec![]);
    let popover = popover.expect("the inspector drawn");
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -400.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
        source: egui::MouseWheelSource::Unknown,
    };
    time += 0.05;
    inspector_in(&ctx, pane, outputs, &mut inspected, time, vec![Event::PointerMoved(popover.center()), wheel]);
    let mut texts = vec![];
    for _ in 0..10 {
        time += 0.05;
        texts = inspector_in(&ctx, pane, outputs, &mut inspected, time, vec![]).0;
    }
    let orange = name(60, 50);
    let at = |texts: &[(String, egui::Rect)], text: &str| texts.iter().find(|(t, _)| t == text).map(|(_, r)| *r);
    assert!(at(&texts, &orange).is_some_and(|r| r.top() < popover.top()), "scrolled: {orange} above the popover {popover:?}");
    // The navy bar's popover, opened by a click: at its top.
    super::inspector::clicked(&ctx, &mut inspected, &fields, outputs, Some((160, 45)));
    let (mut texts, mut popover) = (vec![], None);
    for _ in 0..3 {
        time += 0.05;
        (texts, popover) = inspector_in(&ctx, pane, outputs, &mut inspected, time, vec![]);
    }
    let (navy, popover) = (name(160, 45), popover.expect("the inspector drawn"));
    assert!(at(&texts, &navy).is_some_and(|r| r.top() >= popover.top()), "{navy} in sight at the popover's top {popover:?}");
}

/// The picture of the shapes satin is judged on, placed at its 72 ppi (a pixel 0.35 mm): a navy bar
/// 21 × 3.5 mm and a navy square as large (8.8 mm a side), a red dot 4 mm across, a green square
/// 5 mm a side, and a purple stick 3 mm wide with a round head 15 mm across, on white.
fn satin_shapes() -> image::RgbaImage {
    let round = |x: u32, y: u32, (cx, cy): (f64, f64), r: f64| (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) <= r;
    image::RgbaImage::from_fn(200, 100, |x, y| {
        let inside = |[x0, y0, x1, y1]: [u32; 4]| (x0..x1).contains(&x) && (y0..y1).contains(&y);
        image::Rgba(if inside([10, 10, 70, 20]) || inside([170, 10, 195, 35]) {
            [20, 40, 120, 255]
        } else if round(x, y, (20.0, 60.0), 5.7) {
            [200, 30, 30, 255]
        } else if inside([40, 53, 54, 67]) {
            [30, 140, 60, 255]
        } else if inside([70, 56, 120, 65]) || round(x, y, (140.0, 60.0), 21.3) {
            [120, 40, 140, 255]
        } else {
            [255, 255, 255, 255]
        })
    })
}

/// The inspector's Auto line and its reason say what the planner sews of the area: the long bar all
/// as satin; the 4 mm dot and the 5 mm square, too short for a satin column, tatami (the reason said
/// once, though it is also why Satin is greyed); the square wider than narrow parts as satin,
/// tatami; the stick with its round head, tatami with its narrower parts as satin; with narrow parts
/// as satin off, tatami.
#[test]
fn auto_says_what_the_planner_sews() {
    let (mut app, ctx, mut time) = opened_with(app_with(satin_shapes()));
    let look = |app: &mut VectorcraftApp, time: &mut f64, (x, y): (u32, u32), auto: &str, why: &str, not: &str| {
        click_at(app, &ctx, time, x, y);
        assert!(says(app, &ctx, time, auto), "({x}, {y}): {auto}");
        assert_eq!(times_said(app, &ctx, time, why), 1, "({x}, {y}): {why}, said once");
        let other = if auto == "Auto: satin" { "Auto: tatami" } else { "Auto: satin" };
        assert!(!says(app, &ctx, time, other), "({x}, {y}): not {other}");
        assert!(!says(app, &ctx, time, not), "({x}, {y}): not {not}");
        keyed(app, &ctx, time, Key::Escape, Modifiers::NONE);
    };
    look(&mut app, &mut time, (40, 15), "Auto: satin", "No part wider than ", "Too short for satin");
    look(&mut app, &mut time, (20, 60), "Auto: tatami", "Too short for satin", "Wider than ");
    look(&mut app, &mut time, (47, 60), "Auto: tatami", "Too short for satin", "Wider than ");
    // The navy square as large, wider than 7 mm and too short for a column: tatami, no narrower part.
    click_at(&mut app, &ctx, &mut time, 182, 22);
    assert!(says(&mut app, &ctx, &mut time, "Auto: tatami") && says(&mut app, &ctx, &mut time, " mm: tatami"), "the wide square");
    assert!(!says(&mut app, &ctx, &mut time, "its narrower parts as satin"), "the wide square has none");
    keyed(&mut app, &ctx, &mut time, Key::Escape, Modifiers::NONE);
    look(&mut app, &mut time, (95, 60), "Auto: tatami", "its narrower parts as satin", "Too short for satin");
    // Navy's narrow parts sewn as tatami (a fill job): the bar is tatami, no reason of satin.
    set(&mut app, "colorChoices", json!([{"color": "#142878", "job": "fill"}]));
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 40, 15);
    assert!(says(&mut app, &ctx, &mut time, "Auto: tatami"), "narrow parts as satin off: tatami");
    assert!(!says(&mut app, &ctx, &mut time, "Auto: satin") && !says(&mut app, &ctx, &mut time, "No part wider than "), "no satin said");
}

/// Satin is offered only where it sews satin: on the long bar; greyed, saying why, on the 4 mm dot
/// and the 5 mm square, where the planner finds no satin column; Apply to Similar Areas with Satin
/// on the bar leaves out the navy square as large, too short; and a Satin kept on the dot still
/// shows chosen, its reason that it is sewn as tatami.
#[test]
fn satin_is_offered_only_where_it_sews_satin() {
    let (mut app, ctx, mut time) = opened_with(app_with(satin_shapes()));
    let satin = |app: &mut VectorcraftApp, time: &mut f64| choices(app, &ctx, time).into_iter().find(|(w, _)| w == "Satin").map(|(_, on)| on);
    click_at(&mut app, &ctx, &mut time, 40, 15);
    assert_eq!(satin(&mut app, &mut time), Some(true), "the bar");
    let similar = |app: &mut VectorcraftApp, time: &mut f64| nodes(app, &ctx, time).iter().any(|(_, n)| n.label() == Some("Apply to 1 Similar Area"));
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    assert!(similar(&mut app, &mut time), "with Tatami: the navy square as large");
    press(&mut app, &ctx, &mut time, "Satin", Role::Tab);
    assert!(!similar(&mut app, &mut time), "with Satin: none, the square too short");
    keyed(&mut app, &ctx, &mut time, Key::Escape, Modifiers::NONE);
    for (x, y) in [(20, 60), (47, 60)] {
        click_at(&mut app, &ctx, &mut time, x, y);
        assert_eq!(satin(&mut app, &mut time), Some(false), "({x}, {y}): greyed");
        assert!(says(&mut app, &ctx, &mut time, "Too short for satin"), "({x}, {y}): why");
        keyed(&mut app, &ctx, &mut time, Key::Escape, Modifiers::NONE);
    }
    // A Satin kept on the dot (a live object made before): chosen, sewn as tatami.
    set(&mut app, "areaChanges", json!([{"at": [20.5, 60.5], "color": "#c81e1e", "kind": "fill", "stitch": "satin"}]));
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 20, 60);
    assert_eq!(satin(&mut app, &mut time), Some(true), "kept: shown, chosen");
    assert!(says(&mut app, &ctx, &mut time, "Too short for satin: sewn as tatami"), "its reason");
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

/// While a newer list of changes waits to run, step 4 names each change's own area: the first of two
/// removed (as its ↺ does), the second is named by its area, not by the area of the change that
/// stood first in the list the result shown was made with.
#[test]
fn the_list_names_each_changes_own_area_while_a_newer_list_waits() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    set(&mut app, super::STEP, json!(4));
    let orange = json!({"at": [30.5, 30.5], "color": "#f08214", "kind": "fill", "stitch": "tatami"});
    let navy = json!({"at": [150.5, 30.5], "color": "#142878", "kind": "fill", "angle": 45.0});
    set(&mut app, "areaChanges", json!([orange, navy]));
    until(&mut app, &ctx, &mut time, ready);
    let outputs = &live(&app).schedule.shown.as_ref().expect("a result").outputs;
    let name = |x, y| super::preview::area_name(&outputs.area_at(x, y).expect("an area"));
    let (orange_name, navy_name) = (name(30, 30), name(150, 30));
    assert!(says(&mut app, &ctx, &mut time, &format!("{navy_name} · ")), "the navy square's change named");
    // The newer list waits for the settings to rest, as a heavy change does.
    super::tests::live_mut(&mut app).schedule.time_stages(super::schedule::LIVE_PLAN_MS * 10.0);
    set(&mut app, "areaChanges", json!([navy]));
    assert!(says(&mut app, &ctx, &mut time, &format!("{navy_name} · ")), "named by its own area");
    assert!(!says(&mut app, &ctx, &mut time, &format!("{orange_name} · ")), "not by the removed change's area");
    let shown = live(&app).schedule.shown.as_ref().map(|s| s.request.changes().len());
    assert_eq!(shown, Some(2), "those frames showed the result of the two changes");
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

/// At the most changes the engine takes, an area no change applies to yet can't take one more: its
/// stitch choices and its angle are greyed out, saying why, while one changed already keeps them.
#[test]
fn an_areas_own_choices_are_greyed_out_at_the_most_changes() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    let kept: Vec<serde_json::Value> =
        (0..MAX_AREA_CHANGES - 1).map(|i| json!({"at": [195.5, 95.5], "color": "#f08214", "kind": "fill", "angle": i % 90})).collect();
    set(&mut app, "areaChanges", json!(kept));
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 30, 30);
    press(&mut app, &ctx, &mut time, "Tatami", Role::Tab);
    let own = choices(&mut app, &ctx, &mut time);
    assert!(own.iter().filter(|(w, _)| w != "Satin").all(|(_, can)| *can), "its own change: every choice but Satin, too wide here: {own:?}");
    let angle_open = |app: &mut VectorcraftApp, time: &mut f64| {
        let found = nodes(app, &ctx, time);
        super::tests::node(&found, "Angle", Role::TextInput).map(|n| !n.is_disabled())
    };
    assert_eq!(angle_open(&mut app, &mut time), Some(true), "its own change: its angle too");
    click_at(&mut app, &ctx, &mut time, 150, 30);
    let navy = choices(&mut app, &ctx, &mut time);
    assert!(navy.iter().filter(|(w, _)| w != "Auto").all(|(_, can)| !*can), "one more change: greyed out: {navy:?}");
    assert_eq!(angle_open(&mut app, &mut time), Some(false), "its angle greyed out too");
    assert!(says(&mut app, &ctx, &mut time, "That would be more than"), "saying why");
}

/// A screen reader's click on the preview opens the inspector on the area the keyboard is on, as
/// Enter does: the first area when it is on none.
#[test]
fn a_screen_readers_click_on_the_preview_opens_the_area_the_keyboard_is_on() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    let views = ["Colors", "Shapes", "Stitches", "Compare"];
    let preview = nodes(&mut app, &ctx, &mut time)
        .into_iter()
        .find_map(|(_, n)| n.label().filter(|l| n.role() == Role::Image && views.contains(l)).map(str::to_string))
        .expect("the preview");
    press(&mut app, &ctx, &mut time, &preview, Role::Image);
    assert_eq!(inspected(&app).and_then(|f| f.number), Some(1), "the first area");
}

/// A screen reader's click on the preview, moving the inspector to the area the keyboard is on,
/// leaves an angle being typed with the area it was typed for, as a pointer's click does.
#[test]
fn an_angle_left_by_a_screen_readers_click_is_the_first_areas() {
    let (mut app, ctx, mut time) = opened();
    click_at(&mut app, &ctx, &mut time, 160, 45);
    click(&mut app, &ctx, &mut time, "Angle", Role::TextInput);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::A, Modifiers::COMMAND), Event::Text("30".into())]);
    let views = ["Colors", "Shapes", "Stitches", "Compare"];
    let preview = nodes(&mut app, &ctx, &mut time)
        .into_iter()
        .find_map(|(_, n)| n.label().filter(|l| n.role() == Role::Image && views.contains(l)).map(str::to_string))
        .expect("the preview");
    press(&mut app, &ctx, &mut time, &preview, Role::Image);
    let changes = window_field(&app, "areaChanges").and_then(|c| c.as_array().cloned()).unwrap_or_default();
    assert!(
        changes.iter().any(|c| c["angle"] == json!(30.0) && c["color"] == json!("#142878") && c["kind"] == json!("fill")),
        "the bar's: {changes:?}"
    );
}

/// Apply to Similar Areas sets the angle typed just before it, though the click that ends the
/// typing lands in the same frame: the similar area and this one get the same.
#[test]
fn apply_to_similar_areas_takes_an_angle_typed_in_the_same_frame() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    click_at(&mut app, &ctx, &mut time, 30, 30);
    type_into(&mut app, &ctx, &mut time, "Angle", "30");
    click(&mut app, &ctx, &mut time, "Angle", Role::TextInput);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::A, Modifiers::COMMAND), Event::Text("45".into())]);
    click(&mut app, &ctx, &mut time, "Apply to 1 Similar Area", Role::Button);
    let changes = window_field(&app, "areaChanges").and_then(|c| c.as_array().cloned()).unwrap_or_default();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(changes.iter().all(|c| c["angle"] == json!(45.0)), "both at 45°: {changes:?}");
}

/// Cmd/Ctrl+Z undoes in the window while the preview has the keyboard (after a click on it), as it
/// does while nothing has it: only a field being typed in keeps the keys for its own undo.
#[test]
fn cmd_z_undoes_while_the_preview_has_the_keyboard() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    let before = window_field(&app, "colors");
    set(&mut app, "colors", json!(3));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    until(&mut app, &ctx, &mut time, ready);
    click_at(&mut app, &ctx, &mut time, 190, 90);
    assert!(live(&app).inspector.is_none() && ctx.memory(|m| m.focused().is_some()), "the preview has the keyboard");
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND);
    assert_eq!(window_field(&app, "colors"), before, "undone");
}

/// Undo brings back the background colour chosen before "Leave the background out" was unticked, as
/// its tick does: ticked again after an undo, the colour chosen is left out, not the one found.
#[test]
fn cmd_z_keeps_the_background_chosen_from_the_picture() {
    let (mut app, ctx, mut time) = opened_with(app_with(three_squares()));
    set(&mut app, super::STEP, json!(1));
    let navy = json!("#142878");
    set(&mut app, "background", navy.clone());
    time += 0.05;
    frame(&mut app, &ctx, time, vec![]);
    until(&mut app, &ctx, &mut time, ready);
    let leave_out = "Leave the background out";
    press(&mut app, &ctx, &mut time, leave_out, Role::CheckBox);
    assert_eq!(window_field(&app, "background"), Some(json!("none")), "unticked: every colour sewn");
    press(&mut app, &ctx, &mut time, leave_out, Role::CheckBox);
    assert_eq!(window_field(&app, "background"), Some(navy.clone()), "ticked again: the navy");
    keyed(&mut app, &ctx, &mut time, Key::Z, Modifiers::COMMAND);
    assert_eq!(window_field(&app, "background"), Some(json!("none")), "the tick undone");
    press(&mut app, &ctx, &mut time, leave_out, Role::CheckBox);
    assert_eq!(window_field(&app, "background"), Some(navy), "the colour chosen, not the one found");
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
