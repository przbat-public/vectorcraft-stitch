//! The embroidery tool's handles on the canvas, headless with real pointer events: drawn where the
//! tool catches them, in the art view too (a plan asked for), in place of the passive markers in the
//! stitch view; a drag that follows the pointer and Escape that takes it back; nothing with Hide
//! Edges or in Presentation Mode; found again only when something changed; a tatami fill's rows'
//! arrow, drawn and turned with the mouse.

use egui::{Color32, Event, Modifiers, PointerButton, Pos2, Shape};
use serde_json::json;
use vectorcraft_engine::Session;
use vectorcraft_tools::embroidery::Kind;

use crate::embroidery::stitch_preview;
use crate::{Services, VectorcraftApp, canvas};

/// An app with two filled squares, both selected, and the embroidery tool chosen.
fn app() -> (VectorcraftApp, [u64; 2]) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let mut ids = [0; 2];
    for (k, x) in [40, 170].into_iter().enumerate() {
        ids[k] = app.run("shape.rectangle", json!({"x": x, "y": 60, "width": 80, "height": 80})).unwrap()["id"].as_u64().unwrap();
    }
    app.run("select.set", json!({"ids": ids})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, ids)
}

/// One headless frame at `time` with `events` → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        crate::dialogs::show(app, ui.ctx());
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut all = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            s => all.push(s),
        }
    }
    all
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<Shape> {
    let mut shapes = vec![];
    for _ in 0..4 {
        *time += 0.05;
        shapes = frame(app, ctx, *time, vec![]);
    }
    shapes
}

/// Where each handle the tool catches is on screen, by kind.
fn on_screen(app: &mut VectorcraftApp) -> Vec<(Kind, Pos2)> {
    let view = app.view_info();
    let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    app.session.embroidery_handles(view).0.iter().map(|h| (h.id.kind, xf.to_screen(h.at))).collect()
}

/// The diamonds drawn (an entry's shape, its face in `face`), by their centre.
fn diamonds(shapes: &[Shape], face: Color32) -> Vec<Pos2> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) if p.closed && p.points.len() == 4 && p.fill == face => {
                Some(Pos2::new((p.points[1].x + p.points[3].x) / 2.0, (p.points[0].y + p.points[2].y) / 2.0))
            }
            _ => None,
        })
        .collect()
}

/// The rings drawn round a `kind` handle at `at` (circles with no face, outside the handle): their
/// widths and colours, widest first.
fn rings(shapes: &[Shape], at: Pos2, kind: Kind) -> Vec<(f32, Color32)> {
    let outside = kind.size_px() as f32 / 2.0 * super::handles::HOVER_SCALE + 1.0;
    let mut found: Vec<(f32, Color32)> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Circle(c) if c.fill == Color32::TRANSPARENT && c.center.distance(at) < 0.5 && c.radius > outside => {
                Some((c.stroke.width, c.stroke.color))
            }
            _ => None,
        })
        .collect();
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    found
}

fn light() -> Color32 {
    let [r, g, b] = vectorcraft_engine::cmd::embroidery::scene::MARK_LIGHT;
    Color32::from_rgb(r, g, b)
}

/// In the art view the tool asks for a plan (without turning the status bar's counters on), and each
/// selected square's Auto entry is drawn hollow where the tool catches it (two squares, two
/// entries, two exits).
#[test]
fn handles_are_drawn_where_the_tool_catches_them_in_the_art_view() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    assert!(!app.embroidery.layer.drawn(), "the art view");
    let caught = on_screen(&mut app);
    assert_eq!(caught.iter().filter(|(k, _)| *k == Kind::Entry).count(), 2, "{caught:?}");
    assert!(!app.embroidery.preview.plan_wanted(), "planned without turning the status bar's counters on");
    let drawn = diamonds(&shapes, light());
    for (kind, at) in &caught {
        if *kind == Kind::Entry {
            assert!(drawn.iter().any(|d| d.distance(*at) < 0.5), "an entry at {at:?}, drawn {drawn:?}");
        }
    }
}

/// In the stitch view the tool's handles take the passive markers' place: no marker drawn beside
/// them.
#[test]
fn the_passive_markers_give_way_in_the_stitch_view() {
    let (mut app, _) = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let ctx = egui::Context::default();
    let accent = crate::theme::Tokens::get(&ctx).accent;
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.drawn());
    // The passive markers' entries are filled in the accent; the tool's Auto ones are hollow.
    assert_eq!(diamonds(&shapes, accent).len(), 0);
    assert_eq!(diamonds(&shapes, light()).len(), 2);
}

/// A press on an entry and a drag with the mouse: the entry follows the pointer in the same frame;
/// Escape takes the drag back (the point unset, no undo step).
#[test]
fn a_drag_follows_the_pointer_and_escape_takes_it_back() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let undo = app.session.doc().unwrap().history.undo.len();
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(at, true)]);
    let to = at + egui::vec2(30.0, 12.0);
    let mut shapes = vec![];
    for k in 1..=3 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + (to - at) * (k as f32 / 3.0))]);
    }
    assert!(app.session.tool_options()["drag"].is_object(), "dragging: {}", app.session.tool_options());
    assert!(
        diamonds(&shapes, light())
            .iter()
            .chain(diamonds(&shapes, super::handles::entry_color(&crate::theme::Tokens::get(&ctx))).iter())
            .any(|d| d.distance(to) < 1.0),
        "the entry drawn under the pointer"
    );
    time += 0.05;
    frame(
        &mut app,
        &ctx,
        time,
        vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }],
    );
    time += 0.05;
    frame(&mut app, &ctx, time, vec![button(to, false)]);
    let entry = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
    assert_eq!((entry, app.session.doc().unwrap().history.undo.len()), (json!(null), undo), "taken back");
}

/// Hide Edges and Presentation Mode draw no handles.
#[test]
fn no_handles_with_hide_edges_or_in_presentation_mode() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 2);
    app.ui.view.edges = false;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 0, "Hide Edges");
    app.ui.view.edges = true;
    app.ui.screen_mode = 3;
    assert_eq!(diamonds(&settle(&mut app, &ctx, &mut time), light()).len(), 0, "Presentation Mode");
}

/// The handles are found again when the selection or the plan changes, not on idle frames nor as
/// the pointer moves: neither the canvas nor the tool (which asks for them on every move and cursor
/// query) walks the selection again.
#[test]
fn the_handles_are_found_again_only_when_something_changed() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    // The pointer's first move: the tool finds the handles once for itself.
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(Pos2::new(60.0, 300.0))]);
    let (found, walks) = (app.embroidery.handles.found, vectorcraft_tools::embroidery::handle_walks());
    settle(&mut app, &ctx, &mut time);
    for x in [100.0, 140.0, 180.0] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(Pos2::new(x, 300.0))]);
    }
    assert_eq!(app.embroidery.handles.found, found, "idle frames find nothing again");
    assert_eq!(vectorcraft_tools::embroidery::handle_walks(), walks, "nor do the pointer's moves");
    app.run("select.set", json!({"ids": [a]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.handles.found > found);
    assert_eq!(diamonds(&frame(&mut app, &ctx, time + 0.05, vec![]), light()).len(), 1);
}

/// The drawn words.
fn words(shapes: &[Shape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// Press the second square's entry and drag it by `by` in `steps` → the shapes of the last frame.
fn drag_second_entry(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64, by: egui::Vec2, steps: usize) -> Vec<Shape> {
    let [_, b] = [0, 1];
    let entries: Vec<Pos2> = on_screen(app).iter().filter(|(k, _)| *k == Kind::Entry).map(|(_, at)| *at).collect();
    let at = entries[b];
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    *time += 0.05;
    frame(app, ctx, *time, vec![Event::PointerMoved(at), button(at, true)]);
    let mut shapes = vec![];
    for k in 1..=steps {
        *time += 0.02;
        shapes = frame(app, ctx, *time, vec![Event::PointerMoved(at + by * (k as f32 / steps as f32))]);
    }
    shapes
}

/// While a point is dragged, a ghost joins it to where the needle comes from (the first square's
/// end, along the thread), saying how long the jump is.
#[test]
fn a_drag_shows_the_jump_from_the_part_before() {
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let shapes = drag_second_entry(&mut app, &ctx, &mut time, egui::vec2(10.0, 20.0), 3);
    let said = words(&shapes);
    // The squares lie 21 mm apart, past the 10 mm trim length: the jump is cut.
    assert!(said.iter().any(|w| w.starts_with("jump ") && w.ends_with(" mm, cut")), "{said:?}");
    assert!(shapes.iter().filter(|s| matches!(s, Shape::LineSegment { .. })).count() > 4, "dashes");
}

/// On a design slow to plan, the status bar says when the stitches catch up while a point is
/// dragged, and a pause in the drag plans (on the desktop: a runner off the frame, as the planning
/// thread is), the button still held.
#[test]
fn a_pause_in_a_drag_plans_a_design_slow_to_plan() {
    let (mut app, _) = app();
    app.embroidery.preview = stitch_preview::StitchPreview::with_runner(Box::new(stitch_preview::OneStepAPoll::default()));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let uid = app.session.active().unwrap().uid;
    app.embroidery.preview.log.pinned_ms = Some(stitch_preview::LIVE_PLAN_MS * 4.0);
    app.embroidery.preview.set_plan_ms(uid, stitch_preview::LIVE_PLAN_MS * 4.0);
    assert!(!app.embroidery.preview.live(uid));
    drag_second_entry(&mut app, &ctx, &mut time, egui::vec2(15.0, 5.0), 3);
    assert_eq!(app.ui.status, "Stitches update when you pause or let go.");
    let sent = app.embroidery.preview.log.sent.len();
    // Moving on: nothing planned.
    time += 0.02;
    frame(&mut app, &ctx, time, vec![]);
    assert_eq!(app.embroidery.preview.log.sent.len(), sent, "on the move");
    // Holding still past the pause: planned.
    time += stitch_preview::PAUSE_TO_PLAN_SECS + 0.05;
    frame(&mut app, &ctx, time, vec![]);
    assert!(app.embroidery.preview.log.sent.len() > sent, "planned on the pause");
    assert!(app.session.tool_busy(), "the button still held");
}

/// Whether the Entry Point or Exit Point dialog is open.
fn point_dialog(app: &VectorcraftApp) -> bool {
    app.ui.dialog.as_ref().is_some_and(|d| d.kind == crate::embroidery::point_dialog::KIND)
}

/// A double-click on an entry opens the Entry Point dialog on it; Enter sets the point there (taken
/// onto the outline), one undo step; Escape closes it changing nothing.
#[test]
fn a_double_click_opens_the_point_dialog() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let b = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), b(true), b(false), b(true), b(false)]);
    time += 0.05;
    let shapes = frame(&mut app, &ctx, time, vec![]);
    assert!(point_dialog(&app), "open");
    assert!(words(&shapes).iter().any(|w| w == "Entry Point"), "{:?}", words(&shapes));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(egui::Key::Escape)]);
    assert!(!point_dialog(&app), "Escape closes it");
    let undo = app.session.doc().unwrap().history.undo.len();
    // A second double-click, a second later (egui counts clicks within its double-click time).
    time += 1.0;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), b(true), b(false), b(true), b(false)]);
    assert!(point_dialog(&app), "open again");
    // The dialog opens on an Auto point: Auto is ticked; untick it (as `ui.dialog.set` does), then
    // Enter.
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("auto".into(), json!(false));
    }
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(egui::Key::Enter)]);
    assert!(!point_dialog(&app));
    let entry = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
    assert!(entry.is_array(), "set: {entry}");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
}

/// An agent's drag (`ui.pointer` in document points), the interface's drag (the mouse) and the
/// command itself (`embroidery.setEntryExit {snap: true}` at the point the drag moves it to: where
/// it was, plus the drag) set the same entry.
#[test]
fn agents_and_the_interface_drag_a_point_alike() {
    use crate::control::{ControlRequest, Outcome, handle};
    let ctx = egui::Context::default();
    let entry_of = |app: &mut VectorcraftApp, id: u64| {
        app.run("embroidery.info", json!({"ids": [id]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone()
    };
    // The interface: the mouse.
    let (mut ui_app, [a, _]) = app();
    let mut time = 0.0;
    settle(&mut ui_app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut ui_app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let view = ui_app.view_info();
    let Some(was) = ui_app.session.embroidery_handles(view).0.iter().find(|h| h.id.kind == Kind::Entry).map(|h| h.point) else { panic!("an entry") };
    let to = at + egui::vec2(25.0, 40.0);
    let xf = canvas::Xf::new(ui_app.canvas_rect.unwrap(), ui_app.view().unwrap());
    let (from_doc, to_doc) = (xf.to_doc(at), xf.to_doc(to));
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut ui_app, &ctx, time, vec![Event::PointerMoved(at), button(at, true)]);
    for k in 1..=4 {
        time += 0.02;
        frame(&mut ui_app, &ctx, time, vec![Event::PointerMoved(at + (to - at) * (k as f32 / 4.0))]);
    }
    time += 0.02;
    frame(&mut ui_app, &ctx, time, vec![button(to, false)]);
    let by_mouse = entry_of(&mut ui_app, a);
    // An agent: `ui.pointer`, in document points.
    let (mut agent, [b, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut agent, &ctx, &mut time);
    let ev = |kind: &str, p: vectorcraft_geom::Point| json!({"kind": kind, "x": p.x, "y": p.y});
    let mid = from_doc.midpoint(to_doc);
    let events = json!([ev("down", from_doc), ev("drag", mid), ev("drag", to_doc), ev("up", to_doc)]);
    let (req, _) = ControlRequest::new("ui.pointer", json!({ "events": events }));
    let Outcome::Done(r) = handle(&mut agent, &ctx, &req) else { panic!("not done") };
    assert_eq!(r["ok"], true, "{r}");
    let by_agent = entry_of(&mut agent, b);
    // The command, at the point dropped.
    let (mut by_command, [c, _]) = app();
    let dropped = was + (to_doc - from_doc);
    by_command.run("embroidery.setEntryExit", json!({"id": c, "part": "fill", "entry": [dropped.x, dropped.y], "snap": true})).unwrap();
    let by_command = entry_of(&mut by_command, c);
    assert!(by_mouse.is_array(), "{by_mouse}");
    assert_eq!((&by_mouse, &by_agent), (&by_command, &by_command));
}

/// A tatami fill's rows: an arrow through the square's centre, its head ▲ drawn where the tool
/// catches it, its angle beside it for the one object selected (135°, as the Object tab's angle
/// field writes it); the head dragged with the mouse to straight above the centre turns the rows
/// to 90° (counter-clockwise), the label following the drag, one undo step.
#[test]
fn the_rows_arrow_is_drawn_and_its_head_turns_them() {
    let (mut app, [a, _]) = app();
    app.run("select.set", json!({"ids": [a]})).unwrap();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let shapes = settle(&mut app, &ctx, &mut time);
    let Some(&(_, head)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Angle) else { panic!("the square's arrow") };
    let degrees = |d: f64| super::fields::format(crate::i18n::current(), "°", 1).show(d);
    assert!(words(&shapes).contains(&degrees(135.0)), "{:?}", words(&shapes));
    let triangle = |s: &Shape, near: Pos2| matches!(s, Shape::Path(p) if p.points.len() == 3 && p.points.iter().all(|q| q.distance(near) < 10.0));
    assert!(shapes.iter().any(|s| triangle(s, head)), "its head where it is caught");
    // The square spans 40 to 120 across and 60 to 140 down: its centre is (80, 100).
    let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let up = xf.to_screen(vectorcraft_geom::Point::new(80.0, 100.0)) - egui::vec2(0.0, 60.0);
    let undo = app.session.doc().unwrap().history.undo.len();
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(head), button(head, true)]);
    let mut shapes = vec![];
    for k in 1..=3 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(head + (up - head) * (k as f32 / 3.0))]);
    }
    assert!(words(&shapes).contains(&degrees(90.0)), "the label follows the drag: {:?}", words(&shapes));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![button(up, false)]);
    let angle = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["fill"]["settings"]["angle"]["value"].clone();
    assert_eq!((angle, app.session.doc().unwrap().history.undo.len()), (json!(90.0), undo + 1));
}

/// The handle the keys act on is ringed as the handles are edged: a dark line, a light one over it
/// and the accent inside that, so the ring reads on white or black fabric and on a saturated thread
/// alike; no other handle is ringed.
#[test]
fn the_focused_handle_is_ringed_in_the_two_tones_and_the_accent() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    app.run("tool.setOption", json!({"key": "focus", "value": {"kind": "entry", "id": a, "part": "fill"}})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    let view = app.view_info();
    let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let handles = app.session.embroidery_handles(view).0;
    let focused = |h: &vectorcraft_tools::embroidery::Handle| h.id.kind == Kind::Entry && h.id.owner.part().0.0 == a;
    let at = xf.to_screen(handles.iter().find(|h| focused(h)).unwrap().at);
    let entry = super::handles::entry_color(&crate::theme::Tokens::get(&ctx));
    let tones: Vec<Color32> = rings(&shapes, at, Kind::Entry).into_iter().map(|(_, c)| c).collect();
    assert_eq!(tones, [super::handles::dark(), light(), entry], "dark, light, the accent");
    use super::marks::{NON_TEXT_CONTRAST, contrast};
    for thread in [Color32::WHITE, Color32::BLACK, Color32::from_rgb(0xc8, 0x10, 0x2e)] {
        let best = tones.iter().map(|c| contrast(*c, thread)).fold(0.0, f32::max);
        assert!(best >= NON_TEXT_CONTRAST, "a line of the ring stands out from {thread:?}");
    }
    for h in handles.iter().filter(|h| !focused(h)) {
        assert_eq!(rings(&shapes, xf.to_screen(h.at), h.id.kind), [], "{:?} isn't ringed", h.id);
    }
}

/// A handle from a plan of the object as it was (stale, still draggable) is dimmed in its face
/// only: its two-tone edge stays opaque, so it reads on any fabric as a fresh one does.
#[test]
fn a_stale_handle_dims_its_face_and_keeps_its_edge() {
    use super::handles::{STALE_ALPHA, dark, entry, exit};
    let at = Pos2::new(50.0, 50.0);
    let colour = Color32::from_rgb(0x20, 0x40, 0xa0);
    for solid in [true, false] {
        for (name, shapes) in [("entry", entry(at, 14.0, colour, solid, STALE_ALPHA)), ("exit", exit(at, 14.0, colour, solid, STALE_ALPHA))] {
            let edges: Vec<Color32> = shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Path(p) => match p.stroke.color {
                        egui::epaint::ColorMode::Solid(c) => Some(c),
                        _ => None,
                    },
                    Shape::Circle(c) => Some(c.stroke.color),
                    _ => None,
                })
                .collect();
            assert!(edges.contains(&dark()) && edges.contains(&light()), "{name} solid {solid}: its edge's two lines opaque, {edges:?}");
            let faces: Vec<Color32> = shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Path(p) if p.fill != Color32::TRANSPARENT => Some(p.fill),
                    Shape::Circle(c) if c.fill != Color32::TRANSPARENT => Some(c.fill),
                    _ => None,
                })
                .collect();
            assert!(!faces.is_empty() && faces.iter().all(|c| !c.is_opaque()), "{name} solid {solid}: its face dimmed, {faces:?}");
        }
    }
}

/// At the closest zoom, a drag's ghost to a neighbour far off the canvas is drawn only where it
/// crosses the canvas, a budget of dashes at most: the frame stays as light as at any zoom.
#[test]
fn the_ghost_is_clipped_to_the_canvas_at_the_closest_zoom() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    // Two squares 10 pt apart (3.5 mm, under the trim length): the thread runs from one to the next.
    let ids: Vec<u64> = [40, 110]
        .into_iter()
        .map(|x| app.run("shape.rectangle", json!({"x": x, "y": 60, "width": 60, "height": 60})).unwrap()["id"].as_u64().unwrap())
        .collect();
    app.run("select.set", json!({"ids": [ids[1]]})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let view = app.view_info();
    let handles = app.session.embroidery_handles(view).0;
    let entry = handles.iter().find(|h| h.id.kind == Kind::Entry).expect("the second square's entry");
    assert!(entry.neighbour.is_some_and(|n| n.distance(entry.point) > 5.0), "a neighbour along the thread: {entry:?}");
    app.run("view.setZoom", json!({"zoom": 64000, "center": [entry.at.x, entry.at.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("the entry on screen") };
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(at, true)]);
    let mut shapes = vec![];
    for k in 1..=3 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(8.0 * k as f32, 0.0))]);
    }
    assert!(app.session.tool_options()["drag"].is_object(), "dragging");
    let segments = shapes.iter().filter(|s| matches!(s, Shape::LineSegment { .. })).count();
    // Unclipped, the jump of a few millimetres at 640× would be thousands of dashes a tone.
    assert!((1..1_000).contains(&segments), "{segments} line segments drawn");
}

/// The handles read on white fabric and on dark fabric in every theme (both schemes, every
/// brightness and accent): the two-tone edge's dark line stands out from white and its light line
/// from black and from the dark line, by WCAG's non-text 3:1 (1.4.11); an exit's face from the
/// light line, whatever its layer's colour, and an entry's and an arrow head's in every theme's
/// accent (a set point's face against an Auto one's); each label's words from its pill by WCAG's
/// 4.5:1 (1.4.3), the entry's and the arrow's in the accent, the exit's in its layer's colour.
#[test]
fn handles_and_labels_read_on_any_fabric_in_every_theme() {
    use super::handles::{dark, entry_color, exit_color, light};
    use super::marks::{NON_TEXT_CONTRAST, TEXT_CONTRAST, contrast, text_on};
    use crate::theme::{Accent, Brightness, Scheme, Tokens};
    for fabric in [Color32::WHITE, Color32::BLACK] {
        let edge = if fabric == Color32::WHITE { dark() } else { light() };
        assert!(contrast(edge, fabric) >= NON_TEXT_CONTRAST, "the edge on {fabric:?}");
    }
    assert!(contrast(light(), dark()) >= NON_TEXT_CONTRAST, "the two lines of the edge");
    for (name, [r, g, b]) in vectorcraft_doc::LAYER_COLORS {
        let exit = exit_color([r, g, b]);
        assert!(contrast(exit, light()) >= NON_TEXT_CONTRAST, "{name}'s exit on the light line");
        assert!(contrast(text_on(exit), exit) >= TEXT_CONTRAST, "{name}'s exit label");
    }
    for scheme in Scheme::ALL {
        for brightness in Brightness::ALL {
            for accent in Accent::ALL {
                let t = Tokens::for_scheme(scheme, brightness, accent);
                let entry = entry_color(&t);
                assert!(contrast(entry, light()) >= NON_TEXT_CONTRAST, "{scheme:?} {brightness:?} {accent:?}: the entry on the light line");
                assert!(contrast(text_on(entry), entry) >= TEXT_CONTRAST, "{scheme:?} {brightness:?} {accent:?}: the entry's label");
            }
        }
    }
}

/// The tool's Auto handles come from the plan the canvas draws, handed to the session: an agent
/// planning the document with five other option sets pushes the preview's plan out of the planning
/// service's memory (four a document), and the handles stay; and they are judged stale by plan:
/// an edit (the first square's entry set) dims every Auto handle, the other square's too, until the
/// plan of the document as it is is drawn (a runner that lands the plan a poll after it is sent),
/// then they are Auto again.
#[test]
fn auto_handles_come_from_the_plan_drawn_and_go_stale_with_it() {
    use vectorcraft_tools::embroidery::State;
    let (mut app, [a, b]) = app();
    app.embroidery.preview = stitch_preview::StitchPreview::with_runner(Box::new(stitch_preview::OneStepAPoll::default()));
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let states = |app: &mut VectorcraftApp| {
        let view = app.view_info();
        // The points (an arrow's angle is read from the document as it is, never stale).
        let points = app.session.embroidery_handles(view).0.into_iter().filter(|h| h.id.kind != Kind::Angle);
        points.map(|h| (h.id.owner.part().0.0, h.id.kind, h.state)).collect::<Vec<_>>()
    };
    let auto = states(&mut app);
    assert!(auto.len() == 4 && auto.iter().all(|h| h.2 == State::Auto), "{auto:?}");
    for spacing in [0.3, 0.31, 0.32, 0.33, 0.34] {
        app.run("embroidery.sequence", json!({"spacing": spacing})).unwrap();
    }
    assert_eq!(states(&mut app), auto, "the plan drawn, not the newest remembered");
    app.run("embroidery.setEntryExit", json!({"id": a, "part": "fill", "entry": [45, 65]})).unwrap();
    time += 0.02;
    frame(&mut app, &ctx, time, vec![]);
    let edited = states(&mut app);
    assert!(edited.iter().any(|h| h.0 == a && h.1 == Kind::Entry && h.2 == State::Set), "{edited:?}");
    assert!(edited.iter().filter(|h| h.2 != State::Set).all(|h| h.2 == State::Stale), "every Auto handle stale: {edited:?}");
    assert!(edited.iter().any(|h| h.0 == b && h.2 == State::Stale), "the other square's too");
    settle(&mut app, &ctx, &mut time);
    settle(&mut app, &ctx, &mut time);
    let caught_up = states(&mut app);
    assert!(caught_up.iter().all(|h| h.2 != State::Stale), "the plan drawn caught up: {caught_up:?}");
}

/// What is drawn is what is caught at any zoom: an arrow's head lies 40 screen pixels from the
/// fill's centre, so zooming in moves it in the document; the canvas draws it where the tool then
/// catches it, without the pointer moving (before, the drawing kept the head found at the old zoom).
#[test]
fn the_arrows_head_is_drawn_where_it_is_caught_after_a_zoom() {
    let (mut app, [a, _]) = app();
    app.run("select.set", json!({"ids": [a]})).unwrap();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let head = |app: &mut VectorcraftApp| on_screen(app).into_iter().find(|(k, _)| *k == Kind::Angle).map(|(_, at)| at).unwrap();
    let before = head(&mut app);
    app.run("view.zoomIn", json!({})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    let caught = head(&mut app);
    let triangle = |s: &Shape, near: Pos2| matches!(s, Shape::Path(p) if p.points.len() == 3 && p.points.iter().all(|q| q.distance(near) < 10.0));
    assert!(shapes.iter().any(|s| triangle(s, caught)), "drawn where it is caught: {caught:?} (was {before:?})");
}

/// The tool's part and the Object tab's stay in step both ways: the tab leads when the tool is taken
/// up, an agent's `tool.setOption {part}` sticks and the tab follows, the tab's switch sets the
/// tool's, and the tool's `null` leaves the tab as it is.
#[test]
fn the_tools_part_and_the_tabs_stay_in_step() {
    use crate::panels::embroidery::object::Part;
    let (mut app, _) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let parts = |app: &VectorcraftApp| (app.session.tool_options()["part"].clone(), app.ui.embroidery_panel.part);
    assert_eq!(parts(&app), (json!("fill"), Part::Fill), "the tab's at first");
    app.run("tool.setOption", json!({"key": "part", "value": "stroke"})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(parts(&app), (json!("stroke"), Part::Stroke), "an agent's part sticks, and the tab follows");
    app.ui.embroidery_panel.part = Part::Fill;
    settle(&mut app, &ctx, &mut time);
    assert_eq!(parts(&app), (json!("fill"), Part::Fill), "the tab's switch sets the tool's");
    app.run("tool.setOption", json!({"key": "part", "value": null})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(parts(&app), (json!(null), Part::Fill), "null leaves the tab as it is");
}

/// Enter on the point last pressed opens its dialog, and that Enter, still held, isn't the dialog's
/// OK: it stays open until Enter is let go and pressed again, which sets the point. Its first field
/// has the keyboard.
#[test]
fn the_enter_that_opens_the_point_dialog_is_not_its_ok() {
    let (mut app, [a, _]) = app();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let Some(&(_, at)) = on_screen(&mut app).iter().find(|(k, _)| *k == Kind::Entry) else { panic!("an entry") };
    let b = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let enter = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    // A press on the entry, let go in the next frame: the point the keys act on.
    for events in [vec![Event::PointerMoved(at), b(true)], vec![b(false)]] {
        time += 0.05;
        frame(&mut app, &ctx, time, events);
    }
    let undo = app.session.doc().unwrap().history.undo.len();
    // Enter on the point, as the app's keys give it to the tool, in the frame it is pressed.
    let view = app.view_info();
    let asked = app.session.tool_key(vectorcraft_tools::ToolKey::Enter, vectorcraft_tools::Mods::default(), view);
    crate::canvas::apply_requests(&mut app, asked);

    time += 0.05;
    frame(&mut app, &ctx, time, vec![enter(true)]);
    assert!(point_dialog(&app), "opened by Enter");
    for _ in 0..3 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![]);
    }
    assert!(point_dialog(&app), "the Enter still held isn't its OK");
    assert!(ctx.memory(|m| m.focused()).is_some(), "its first field has the keyboard");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![enter(false)]);
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields.insert("auto".into(), json!(false));
    }
    time += 0.05;
    frame(&mut app, &ctx, time, vec![enter(true), enter(false)]);
    assert!(!point_dialog(&app), "Enter, pressed again, is its OK");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    let entry = app.run("embroidery.info", json!({"ids": [a]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
    assert!(entry.is_array(), "set: {entry}");
}

/// While a fill's entry is dragged into the fill, its point is stored on the outline (where the
/// stitches can start): a dot marks it there, and the ghost says the jump from the square sewn
/// before to that point, not to the pointer.
#[test]
fn the_ghost_measures_the_jump_to_the_point_stored() {
    let (mut app, [_, b]) = app();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let view = app.view_info();
    let entry = |app: &mut VectorcraftApp| {
        app.session.embroidery_handles(view).0.into_iter().find(|h| h.id.kind == Kind::Entry && h.id.owner.part().0.0 == b)
    };
    let Some(h) = entry(&mut app) else { panic!("the second square's entry") };
    let xf = canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let at = xf.to_screen(h.at);
    let into = xf.to_screen(vectorcraft_geom::Point::new(210.0, 100.0));
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(at), button(at, true)]);
    let mut shapes = vec![];
    for k in 1..=4 {
        time += 0.02;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + (into - at) * (k as f32 / 4.0))]);
    }
    let Some(held) = entry(&mut app) else { panic!("still there") };
    let stored = xf.to_screen(held.point);
    assert!(stored.distance(into) > crate::embroidery::handles::LEADER_MIN_PT, "stored on the outline, away from the pointer");
    let neighbour = held.neighbour.expect("the square sewn before");
    let mm = neighbour.distance(held.point) / vectorcraft_engine::cmd::embroidery::mm_to_pt(1.0);
    let said = format!("jump {mm:.1} mm");
    assert!(words(&shapes).iter().any(|w| w.starts_with(&said)), "{said} in {:?}", words(&shapes));
    let dot = shapes.iter().any(|s| matches!(s, Shape::Circle(c) if c.center.distance(stored) < 0.5));
    assert!(dot, "a dot where the point is stored");
    time += 0.02;
    frame(&mut app, &ctx, time, vec![button(into, false)]);
}

/// More objects selected than show their points: the status bar says so once (the first 32 show
/// theirs), not again while the selection stays that large.
#[test]
fn the_points_cap_is_said() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let ids: Vec<u64> = (0..33)
        .map(|k| {
            app.run("shape.rectangle", json!({"x": 10 + (k % 11) * 50, "y": 10 + (k / 11) * 50, "width": 30, "height": 30})).unwrap()["id"]
                .as_u64()
                .unwrap()
        })
        .collect();
    app.run("select.set", json!({"ids": ids})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let ctx = egui::Context::default();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.ui.status, "Entry and exit points show for the first 32 objects selected");
    app.status("");
    app.run("select.set", json!({"ids": &ids[1..]})).unwrap();
    app.run("select.set", json!({"ids": ids})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.ui.status, "", "said once");
}
