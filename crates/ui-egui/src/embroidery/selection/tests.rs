//! Stitch editing on the canvas, headless with real input: the selection layer drawn without
//! drawing the stitches again, a drag with the mouse that moves the needle points (checked before
//! Escape puts them back) and one undo step when let go, the live check's words (and no notice that
//! the run selected isn't sewn), the edit look's circles from 250 % and its marks on a stitch that
//! breaks the limits, the current needle point's two-tone ring, the Contextual Task Bar's stitch
//! actions, and frames that build nothing while nothing changes.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape};
use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;
use vectorcraft_geom::Point;

use crate::embroidery::stitch_preview;
use crate::{Services, VectorcraftApp, canvas};

/// An app with a filled square converted to manual stitches, isolated, in the stitch view, the
/// Embroidery Tool chosen: editing its stitches → the app and the converted object.
fn editing() -> (VectorcraftApp, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#c81e1e"})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [id]})).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, id)
}

/// One headless frame at `time` with `events`, as the app runs one → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::shortcuts::handle(app, ui.ctx());
        canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
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

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> Vec<Shape> {
    let mut shapes = vec![];
    for _ in 0..4 {
        *time += 0.05;
        shapes = frame(app, ctx, *time, vec![]);
    }
    shapes
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(k: Key) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

/// The converted object's first run's anchor `k`, in the document.
fn needle(app: &VectorcraftApp, object: u64, k: usize) -> Point {
    let st = app.session.active().unwrap();
    let group = st.doc.node(NodeId(object)).unwrap();
    group.children().unwrap().first().unwrap().path_data().unwrap().subpaths[0].anchors[k].p
}

fn on_screen(app: &VectorcraftApp, p: Point) -> Pos2 {
    canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(p)
}

/// A click on a needle point selects it; the selection layer draws it in its layer's colour, built
/// once for the change, with no frame of stitches drawn again; idle frames build nothing.
#[test]
fn a_selection_change_draws_no_stitches_again() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let (uploads, builds) = (app.embroidery.layer.uploads, app.embroidery.selection.builds);
    let at = on_screen(&app, needle(&app, object, 4));
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let shapes = settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.layer.uploads, uploads, "a selection change draws no stitches again");
    assert_eq!(app.embroidery.selection.builds, builds + 1, "built once for the change");
    let ink = crate::embroidery::marks::readable(egui::Color32::from_rgb(0x4f, 0x80, 0xff), &[super::light()], false);
    let filled = shapes.iter().any(|s| matches!(s, Shape::Circle(c) if c.center.distance(at) < 0.5 && c.fill == ink));
    assert!(filled, "the selected needle point filled in its layer's colour");
    let builds = app.embroidery.selection.builds;
    for _ in 0..10 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(30.0, 30.0))]);
    }
    assert_eq!(app.embroidery.selection.builds, builds, "idle and hover frames build nothing");
}

/// The pointer moving along a stitch finds the Embroidery Tool's handles again no more than its
/// moves elsewhere do: what lies under the pointer changes with every move along a stitch, and the
/// handles aren't found for it.
#[test]
fn moving_along_a_stitch_finds_no_handles_again() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let centre = needle(&app, object, 4);
    // Zoomed in until the rows lie apart: midway along a stitch, no needle point is near.
    app.run("view.setZoom", json!({"zoom": 1600, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let (a, b) = (on_screen(&app, needle(&app, object, 4)), on_screen(&app, needle(&app, object, 5)));
    let along: Vec<Pos2> = [0.4, 0.45, 0.5, 0.55, 0.6].iter().map(|&t| a + (b - a) * t).collect();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(along[0])]);
    settle(&mut app, &ctx, &mut time);
    let (found, walks) = (app.embroidery.handles.found, vectorcraft_tools::embroidery::handle_walks());
    let mut under = vec![];
    for &at in &along[1..] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at)]);
        under.push(app.session.tool_options()["stitches"]["hover"]["at"].clone());
    }
    assert!(under.iter().all(Value::is_array) && under.windows(2).all(|w| w[0] != w[1]), "the pointer moves along a stitch: {under:?}");
    assert_eq!(app.embroidery.handles.found, found, "moves along a stitch find no handles again");
    assert_eq!(vectorcraft_tools::embroidery::handle_walks(), walks, "nor does the tool");
}

/// A drag of a selected needle point with the mouse moves it while it drags (checked before
/// Escape), Escape puts the document back as it was at the press, and a drag let go is one undo
/// step; the live check says the move while it drags.
#[test]
fn a_drag_moves_needle_points_and_escape_puts_them_back() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let from = needle(&app, object, 4);
    let at = on_screen(&app, from);
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let before = app.session.active().unwrap().doc.clone();
    let steps = app.session.active().unwrap().history.undo.len();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at, true)]);
    for k in 1..=6 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(6.0 * k as f32, 0.0))]);
    }
    assert!(needle(&app, object, 4).x > from.x + 1.0, "the drag moved the needle point before Escape");
    let lines = super::bubble(&mut app).expect("the live check");
    assert!(lines.iter().any(|(_, w)| w.starts_with("Moved X")), "{lines:?}");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
    assert!(app.session.active().unwrap().doc == before, "Escape puts the document back as it was at the press");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at + egui::vec2(36.0, 0.0), false)]);
    // Again, let go this time: one undo step.
    for e in [
        press(at, true),
        Event::PointerMoved(at + egui::vec2(10.0, 0.0)),
        Event::PointerMoved(at + egui::vec2(20.0, 0.0)),
        press(at + egui::vec2(20.0, 0.0), false),
    ] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().history.undo.len(), steps + 1);
    assert!(needle(&app, object, 4).x > from.x + 1.0);
}

/// While a needle point is dragged, its range pill and the current needle point's ring go with its
/// circle: drawn from the document the drag started from, moved once.
#[test]
fn the_pill_and_the_ring_follow_the_drag_once() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let at = on_screen(&app, needle(&app, object, 4));
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    settle(&mut app, &ctx, &mut time);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at, true)]);
    let mut shapes = vec![];
    for k in 1..=8 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(8.0 * k as f32, 0.0))]);
    }
    let circle = shapes
        .iter()
        .find_map(|s| match s {
            Shape::Circle(c) if (c.radius - super::SELECTED_RADIUS_PT).abs() < 1e-3 && c.fill != egui::Color32::TRANSPARENT => Some(c.center),
            _ => None,
        })
        .expect("the selected circle");
    assert!(circle.x > at.x + 30.0, "the circle moved with the drag: {circle:?} from {at:?}");
    let ring = shapes
        .iter()
        .find_map(|s| match s {
            Shape::Circle(c) if (c.radius - (super::SELECTED_RADIUS_PT + super::RING_OUT_PT)).abs() < 1e-3 => Some(c.center),
            _ => None,
        })
        .expect("the current needle point's ring");
    assert!(ring.distance(circle) < 0.5, "the ring {ring:?} on the circle {circle:?}");
    let pill = shapes
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) if t.galley.text().starts_with('#') => Some(t.visual_bounding_rect().center()),
            _ => None,
        })
        .expect("the range pill");
    assert!(pill.distance(circle) < 30.0, "the pill {pill:?} beside the circle {circle:?}, not a drag further");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
}

/// A marquee dragged far off the canvas: the tool keeps it within the document's coordinates, so
/// the frames that draw it end, whether the pointer is the mouse's (held across frames) or an
/// agent's (`ui.pointer`, whose release selects the needle points it covers).
#[test]
fn a_marquee_dragged_far_off_stays_within_the_document() {
    let (mut app, _) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let max = vectorcraft_doc::MAX_COORD;
    // With the mouse, from an empty spot of the artboard to far past the window.
    let empty = on_screen(&app, Point::new(20.0, 20.0));
    let far = egui::pos2(3e17, 3e17);
    for e in [Event::PointerMoved(empty), press(empty, true), Event::PointerMoved(empty + egui::vec2(30.0, 30.0)), Event::PointerMoved(far)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let marquee = app.session.tool_options()["stitches"]["marquee"].clone();
    let corner = |m: &serde_json::Value, k: usize| m[k].as_f64().unwrap();
    assert!((corner(&marquee, 0) - 20.0).abs() < 1e-6 && (corner(&marquee, 1) - 20.0).abs() < 1e-6, "{marquee}");
    assert_eq!([corner(&marquee, 2), corner(&marquee, 3)], [max, max], "kept within the document's coordinates");
    settle(&mut app, &ctx, &mut time);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(far, false)]);
    settle(&mut app, &ctx, &mut time);
    let editing = app.session.stitch_editing(app.view_info()).unwrap();
    assert!(editing.selected > 0 && editing.selected == editing.scope.needles, "{} of {}", editing.selected, editing.scope.needles);
    // An agent's, in one request (a command ends a gesture: each request ends reading the
    // document).
    app.run("select.none", json!({})).unwrap();
    let events = json!([{"kind": "down", "x": 20, "y": 20}, {"kind": "drag", "x": 3e17, "y": 3e17}, {"kind": "up", "x": 3e17, "y": 3e17}]);
    let (req, _) = crate::control::ControlRequest::new("ui.pointer", json!({"events": events}));
    let crate::control::Outcome::Done(r) = crate::control::handle(&mut app, &ctx, &req) else { panic!("not done") };
    assert_eq!(r["ok"], true, "{r}");
    settle(&mut app, &ctx, &mut time);
    let editing = app.session.stitch_editing(app.view_info()).unwrap();
    assert!(editing.selected > 0 && editing.selected == editing.scope.needles, "{} of {}", editing.selected, editing.scope.needles);
}

/// A run of the edited object out of the document's range, as a file can hold one: the needle
/// index refuses it, so Select All selects the other runs' needle points alone, the box drawn for
/// them (past the shapes' cap) stays where they are, and the frames that draw it end.
#[test]
fn a_run_out_of_range_is_left_out_of_the_selection_and_its_box() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#c81e1e"})).unwrap();
    let big = app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 280, "height": 200})).unwrap()["id"].as_u64().unwrap();
    let small = app.run("shape.rectangle", json!({"x": 10, "y": 230, "width": 60, "height": 50})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [big, small]})).unwrap();
    let group = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    // The small shape's first run with its last needle point far off.
    {
        let doc = std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().doc);
        let run = doc.node(NodeId(small)).unwrap().children().unwrap().first().unwrap().id;
        let path = doc.node_mut(run).unwrap().path_data_mut().unwrap();
        path.subpaths.last_mut().unwrap().anchors.last_mut().unwrap().p = Point::new(1e17, 1e17);
    }
    app.run("select.none", json!({})).unwrap();
    app.run("object.isolate", json!({"id": group})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    app.run("select.all", json!({})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let editing = app.session.stitch_editing(app.view_info()).unwrap();
    assert_eq!(editing.scope.refused.of(vectorcraft_tools::embroidery::Refusal::OutOfRange), 1, "{:?}", editing.scope);
    let (_, said) = crate::embroidery::editing::lines(&mut app).unwrap();
    assert!(said.iter().any(|l| l == "1 run can't be edited: its points lie outside the document"), "the chip says why: {said:?}");
    assert!(!said.iter().any(|l| l.contains("locked")), "and no other reason: {said:?}");
    assert!(editing.selected > super::SELECTION_SHAPES_MAX, "the box is drawn: {}", editing.selected);
    assert_eq!(editing.selected, editing.scope.needles, "every needle point being edited, the far run's left out");
    let k = &app.embroidery.selection;
    assert_eq!(k.count, editing.selected, "the selection layer counts as the chip does");
    let b = k.bounds.unwrap();
    assert!(b.x1 < 300.0 && b.y1 < 300.0, "the box stays on the artboard: {b:?}");
}

/// The live check: the needle points selected, and their stitches' shortest and longest with a
/// passed mark; a stitch made longer than the limit warns.
#[test]
fn the_live_check_judges_the_stitches_of_the_selection() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}, {"id": run.0, "index": 5}]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let lines = super::bubble(&mut app).unwrap();
    assert!(lines[0].1.starts_with("2 needle points"), "{lines:?}");
    assert!(lines.iter().any(|(m, w)| *m == Some(super::Mark::Passed) && w.starts_with("Shortest:")), "{lines:?}");
    // The run they are of is selected with them, and it is sewn: no "isn't sewn" notice.
    assert!(app.session.active().unwrap().selection.objects.contains(&run), "the run selected");
    assert_eq!(crate::embroidery::overlay::not_sewn(&app, time), 0);
    assert_eq!(crate::embroidery::notice::text(&app, time), None);
    app.run("embroidery.moveNeedlePoints", json!({"dx": 40})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let lines = super::bubble(&mut app).unwrap();
    assert!(lines.iter().any(|(m, w)| *m == Some(super::Mark::Warning) && w.contains("too long")), "{lines:?}");
    // More stitches with one end selected than a drag measures again each frame: at rest the check
    // still says what they are (judged once), not "Updating…".
    app.embroidery.selection.moved_ends = None;
    let lines = super::bubble(&mut app).unwrap();
    assert!(lines.iter().any(|(m, w)| *m == Some(super::Mark::Warning) && w.contains("too long")), "at rest: {lines:?}");
    assert!(!lines.iter().any(|(_, w)| w == "Updating…"), "{lines:?}");
}

/// The edit look: needle points as circles from 250 % and not under it; a stitch longer than the
/// limit marked, and none before.
#[test]
fn the_edit_look_shows_circles_from_250_percent_and_marks_rule_breakers() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    assert_eq!(super::super::editing::NEEDLE_CIRCLES_MIN_ZOOM, 2.5, "the project owner's 250 % (2026-10-27)");
    let centre = needle(&app, object, 4);
    let zoom_in = "Zoom in to see the needle points.";
    let says = |app: &mut VectorcraftApp| crate::embroidery::editing::lines(app).is_some_and(|(_, l)| l.iter().any(|w| w == zoom_in));
    app.run("view.setZoom", json!({"zoom": 249, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.layer.stats().circles, 0, "no circles at 249 %");
    assert!(says(&mut app), "the chip asks to zoom in at 249 %");
    assert_eq!(app.embroidery.layer.stats().flagged, 0, "nothing breaks the limits after converting");
    app.run("view.setZoom", json!({"zoom": 250, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.stats().circles > 0, "circles from 250 %");
    assert!(!says(&mut app), "and the chip no longer asks");
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}], "dx": 30})).unwrap();
    app.run("view.setZoom", json!({"zoom": 100, "center": [centre.x, centre.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert!(app.embroidery.layer.stats().flagged > 0, "a stitch longer than the limit is marked");
}

/// Stitches exactly on a limit pass and those a grid step past it break it, alike on the canvas, in
/// the live check and in the Stitches tab: with DST checked, runs of 0.4 mm (the shortest stitch),
/// 0.3 mm, 12.1 mm (one DST record) and 12.2 mm, six times over at shifting places.
#[test]
fn a_stitch_on_a_limit_is_judged_alike_everywhere() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 600, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setFill", json!({"none": true})).unwrap();
    app.run("paint.setStroke", json!({"color": "#1e50c8"})).unwrap();
    let line = app.run("path.create", json!({"d": "M 30 100 L 90 100"})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [line]})).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    let run = app.session.active().unwrap().doc.node(NodeId(line)).unwrap().children().unwrap()[0].id;
    let count = |app: &VectorcraftApp| app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors.len();
    // The run made of exact places: its first needle point, then each stitch's length in turn.
    let (x0, y) = (10.0, 30.0);
    let mut at = vec![];
    let mut x = x0;
    for r in 0..6 {
        for length in [0.4, 0.3, 12.1, 12.2, 2.0 + 0.1 * f64::from(r)] {
            x += length;
            at.push(json!([x, y]));
        }
    }
    let original = count(&app);
    app.run("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 0}], "to": [x0, y]})).unwrap();
    app.run("embroidery.insertNeedlePoints", json!({"id": run.0, "index": 1, "at": at})).unwrap();
    let last = count(&app) - 1;
    app.run("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": run.0, "from": at.len() + 1, "to": last}]})).unwrap();
    assert_eq!(count(&app), at.len() + 1, "the run as made (from {original} needle points)");
    app.run("select.none", json!({})).unwrap();
    app.run("object.isolate", json!({"id": line})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    app.run("ui.embroideryPanel", json!({"tab": "stitches", "scope": "selection", "filter": "all"})).unwrap();
    let uid = app.session.active().unwrap().uid;
    app.embroidery.check.format.insert(uid, vectorcraft_engine::cmd::embroidery::Format::Dst);
    let centre = vectorcraft_engine::cmd::embroidery::mm_to_pt((x0 + x) / 2.0);
    app.run("view.setZoom", json!({"zoom": 100, "center": [centre, vectorcraft_engine::cmd::embroidery::mm_to_pt(y)]})).unwrap();
    app.run("select.all", json!({})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    for _ in 0..3 {
        settle(&mut app, &ctx, &mut time);
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::Panel::right("embroidery-test").exact_size(300.0).resizable(false).show(ui, |ui| crate::panels::embroidery::show(&mut app, ui));
        });
        out.textures_delta.clear();
    }
    // The tab.
    let (_, listed) = app.embroidery.stitches.list.as_ref().expect("the rows listed");
    let (short, long) = (listed.counts[4], listed.counts[5]);
    assert_eq!((short, long), (6, 6), "the tab's Too short and Too long");
    // The live check, every needle point selected.
    let k = &app.embroidery.selection;
    assert_eq!((k.fixed.short, k.fixed.long), (6, 6), "the live check");
    // The canvas, every stitch on it.
    assert_eq!(app.embroidery.layer.stats().flagged, 12, "the canvas's marks");
}

/// `]` makes a needle point the current one: a two-tone ring round it (dark, light, its colour).
#[test]
fn the_current_needle_point_has_a_two_tone_ring() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    time += 0.05;
    // Typed, as the shortcuts read brackets (layouts where a bracket takes Option or AltGr count).
    frame(&mut app, &ctx, time, vec![Event::Text("]".into())]);
    let shapes = settle(&mut app, &ctx, &mut time);
    let at = on_screen(&app, needle(&app, object, 0));
    let rings: Vec<egui::Color32> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Circle(c) if c.fill == egui::Color32::TRANSPARENT && c.center.distance(at) < 0.5 && c.radius > super::SELECTED_RADIUS_PT => {
                Some(c.stroke.color)
            }
            _ => None,
        })
        .collect();
    assert!(rings.contains(&super::dark()) && rings.contains(&super::light()), "a two-tone ring: {rings:?}");
}

/// With needle points selected, the Contextual Task Bar offers the stitch actions in place of
/// upstream's; Delete there deletes them, one undo step.
#[test]
fn the_task_bar_offers_the_stitch_actions() {
    let (mut app, object) = editing();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let run = app.session.active().unwrap().doc.node(NodeId(object)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 4}]})).unwrap();
    let shapes = settle(&mut app, &ctx, &mut time);
    let bar = crate::canvas::task_bar_rect(&ctx).expect("the task bar shows");
    let mut labels: Vec<(String, egui::Rect)> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) if bar.contains(t.visual_bounding_rect().center()) => Some((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => None,
        })
        .collect();
    labels.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
    let words: Vec<&str> = labels.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(words, ["Insert", "Delete", "Split Run", "Join Runs", "Trim", "Stop", "Split Long Stitches"], "{words:?}");
    let count = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors.len();
    let steps = app.session.active().unwrap().history.undo.len();
    let delete = labels.iter().find(|(w, _)| w == "Delete").unwrap().1.center();
    for e in [Event::PointerMoved(delete), press(delete, true), press(delete, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().history.undo.len(), steps + 1);
    let now = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors.len();
    assert_eq!(now, count - 1, "the needle point deleted");
}

/// The selection's lines and box stand out from the fabric whatever the layer's colour: black on
/// black fabric is lifted to show, white on white fabric darkened.
#[test]
fn the_selections_lines_show_on_any_fabric() {
    let (black, white) = ([0, 0, 0], [255, 255, 255]);
    for (layer, fabric) in [(black, black), (white, white), ([20, 30, 90], black), ([200, 200, 60], white)] {
        let line = super::line_colour(layer, fabric);
        let [r, g, b] = fabric;
        let c = crate::embroidery::marks::contrast(line, egui::Color32::from_rgb(r, g, b));
        assert!(c >= 3.0, "{layer:?} on {fabric:?}: {line:?}, {c}");
    }
}

/// The Contextual Task Bar sits under the selected needle points, not under their paths' whole box:
/// zoomed in on a fill whose box runs off the canvas, needle points selected on its left put the bar
/// further left than needle points selected further right (under the fill's box both would put it
/// in one place, pushed to the canvas's edge).
#[test]
fn the_task_bar_sits_under_the_selected_needle_points() {
    let (mut app, _) = editing();
    let ctx = context();
    let mut time = 0.0;
    app.run("view.setZoom", json!({"zoom": 1000, "center": [70, 105]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let mm = vectorcraft_engine::cmd::embroidery::pt_to_mm;
    let mut bars = vec![];
    for (from, to) in [(60.0, 75.0), (95.0, 110.0)] {
        app.run("embroidery.selectNeedlePoints", json!({"rect": [mm(from), mm(60.0), mm(to), mm(150.0)]})).unwrap();
        settle(&mut app, &ctx, &mut time);
        let b = super::selected_box(&mut app).expect("needle points selected");
        assert!(b.x0 >= from - 1.0 && b.x1 <= to + 1.0, "the needle points' box: {b:?}");
        bars.push(crate::canvas::task_bar_rect(&ctx).expect("the task bar"));
    }
    assert!(bars[0].min.x + 100.0 < bars[1].min.x, "the bar follows the needle points: {bars:?}");
}

/// What editing kept goes when it ends: the selection drawn and the units edited, when the object
/// is left and when the stitch view is turned off.
#[test]
fn what_editing_kept_goes_when_it_ends() {
    for (end, params) in [("object.exitIsolation", json!({})), ("view.stitchPreview", json!({"on": false}))] {
        let (mut app, object) = editing();
        let ctx = context();
        let mut time = 0.0;
        settle(&mut app, &ctx, &mut time);
        let at = on_screen(&app, needle(&app, object, 4));
        for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
            time += 0.05;
            frame(&mut app, &ctx, time, vec![e]);
        }
        settle(&mut app, &ctx, &mut time);
        assert!(app.embroidery.selection.key.is_some() && app.embroidery.layer.keeps_edited(), "{end}: kept while editing");
        app.run(end, params).unwrap();
        settle(&mut app, &ctx, &mut time);
        assert!(app.embroidery.selection.key.is_none(), "{end}: the selection drawn goes");
        assert!(!app.embroidery.layer.keeps_edited(), "{end}: the units edited go");
    }
}

/// The chip and the live check are said as they change, politely; the live check not at each step
/// of a drag, but once it is let go.
#[test]
fn the_chip_and_the_live_check_are_said() {
    let (mut app, object) = editing();
    let ctx = context();
    ctx.enable_accesskit();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let at = on_screen(&app, needle(&app, object, 4));
    for e in [Event::PointerMoved(at), press(at, true), press(at, false)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    // The two nodes' live settings in a frame with `events`.
    let live = |app: &mut VectorcraftApp, time: f64, events: Vec<Event>| {
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            crate::shortcuts::handle(app, ui.ctx());
            canvas::show(app, ui);
            crate::embroidery::canvas_controls(app, ui);
            stitch_preview::schedule(app, ui.ctx());
        });
        out.textures_delta.clear();
        let update = out.platform_output.accesskit_update.expect("a tree");
        let of = |name: &str| update.nodes.iter().find(|(id, _)| *id == egui::Id::new(name).accesskit_id()).and_then(|(_, n)| n.live());
        (of("embroidery-stitch-editing"), of("embroidery-live-check"))
    };
    time += 0.05;
    let polite = Some(egui::accesskit::Live::Polite);
    assert_eq!(live(&mut app, time, vec![]), (polite, polite), "the chip and the live check, at rest");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at, true)]);
    for k in 1..=3 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(at + egui::vec2(6.0 * k as f32, 0.0))]);
    }
    time += 0.05;
    let dragging = live(&mut app, time, vec![Event::PointerMoved(at + egui::vec2(24.0, 0.0))]);
    assert_eq!(dragging.1, Some(egui::accesskit::Live::Off), "not at each step of the drag");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(at + egui::vec2(24.0, 0.0), false)]);
    time += 0.05;
    assert_eq!(live(&mut app, time, vec![]).1, polite, "said once let go");
}

/// Alt-click on a stitch of a path's later run inserts the needle point into that run, where it was
/// pressed: the stitch named is that run's own, not one of the first run's.
#[test]
fn an_alt_click_on_a_later_runs_stitch_inserts_into_that_run() {
    let (mut app, object) = editing();
    let run = {
        let st = app.session.active().unwrap();
        st.doc.node(NodeId(object)).unwrap().children().unwrap().first().unwrap().id
    };
    app.run("embroidery.splitRun", json!({"needlePoints": [{"id": run.0, "index": 6}], "ties": false})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let anchors = |app: &VectorcraftApp, s: usize| {
        let st = app.session.active().unwrap();
        st.doc.node(run).unwrap().path_data().unwrap().subpaths[s].anchors.iter().map(|a| a.p).collect::<Vec<_>>()
    };
    let (first, later) = (anchors(&app, 0).len(), anchors(&app, 1));
    assert!(later.len() > 3, "a later run of {} needle points", later.len());
    let (a, b) = (later[1], later[2]);
    app.run("view.setZoom", json!({"zoom": 1600, "center": [a.x, a.y]})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let at = on_screen(&app, mid);
    let alt_press = |pressed: bool| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::ALT };
    for e in [
        Event::ModifiersChanged(Modifiers::ALT),
        Event::PointerMoved(at),
        alt_press(true),
        alt_press(false),
        Event::ModifiersChanged(Modifiers::NONE),
    ] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(anchors(&app, 0).len(), first, "the first run as it was");
    let now = anchors(&app, 1);
    assert_eq!(now.len(), later.len() + 1, "one needle point more, in the later run");
    assert!(now[2].distance(mid) < 0.3, "between the stitch's two needle points, where it was pressed: {:?}", now[2]);
}

/// An app editing one run of `n` needle points (rows of a hundred a millimetre apart, two
/// millimetres between rows, back and forth), in a group of its own → the app and the run. The
/// Stitches tab's tests use it too.
pub(crate) fn editing_run(n: usize) -> (VectorcraftApp, NodeId) {
    const ROW: usize = 100;
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    let mm = vectorcraft_engine::cmd::embroidery::mm_to_pt;
    let d: Vec<String> = (0..n)
        .map(|k| {
            let (row, col) = (k / ROW, k % ROW);
            let x = if row % 2 == 0 { col } else { ROW - 1 - col };
            format!("{} {} {}", if k == 0 { "M" } else { "L" }, mm(5.0 + x as f64), mm(5.0 + 2.0 * row as f64))
        })
        .collect();
    app.run("paint.setFill", json!({"none": true})).unwrap();
    app.run("paint.setStroke", json!({"color": "#1e64c8"})).unwrap();
    let id = NodeId(app.run("path.create", json!({"d": d.join(" ")})).unwrap()["id"].as_u64().unwrap());
    app.session
        .edit("Stitches", |doc, _| {
            doc.node_mut(id).unwrap().extras_mut().embroidery = Some(Box::new(vectorcraft_doc::embroidery::Embroidery {
                stitches: Some(vectorcraft_doc::embroidery::StitchRun::default()),
                ..Default::default()
            }));
            Ok(())
        })
        .unwrap();
    app.run("select.set", json!({"ids": [id.0]})).unwrap();
    let group = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();
    app.run("select.none", json!({})).unwrap();
    app.run("object.isolate", json!({"id": group})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    (app, id)
}

/// The left edge of the selection's box as drawn: its dark dashes' leftmost point (screen).
fn box_left(shapes: &[Shape]) -> f32 {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::LineSegment { points, stroke } if stroke.width == super::STITCH_UNDER_PT && stroke.color == super::dark() => {
                Some(points[0].x.min(points[1].x))
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

/// Past [`super::SELECTION_SHAPES_MAX`] needle points selected the selection is drawn as its box,
/// no circle of its own, and the box follows a drag of them.
#[test]
fn past_2000_selected_the_box_is_drawn_and_follows_a_drag() {
    let (mut app, run) = editing_run(2_500);
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    app.run("embroidery.selectNeedlePoints", json!({"ranges": [{"id": run.0, "from": 0, "to": 2_499}]})).unwrap();
    let before = box_left(&settle(&mut app, &ctx, &mut time));
    let k = &app.embroidery.selection;
    assert!(k.count > super::SELECTION_SHAPES_MAX && k.points.is_empty() && k.bounds.is_some(), "the box: {} selected", k.count);
    assert!(before.is_finite(), "its dashes drawn");
    let grip = {
        let st = app.session.active().unwrap();
        on_screen(&app, st.doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors[0].p)
    };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(grip), press(grip, true)]);
    let mut shapes = vec![];
    for k in 1..=5 {
        time += 0.05;
        shapes = frame(&mut app, &ctx, time, vec![Event::PointerMoved(grip + egui::vec2(8.0 * k as f32, 0.0))]);
    }
    let during = box_left(&shapes);
    assert!(during > before + 30.0, "the box moved with the drag: {before} → {during}");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(grip + egui::vec2(40.0, 0.0), false)]);
}

/// Past [`super::RUBBER_BANDS_MAX`] stitches with one end selected, the live check says
/// "Updating…" while a drag goes on (it measures them again only when the drag ends) and judges
/// them at rest.
#[test]
fn past_20000_stretched_the_live_check_says_updating_during_a_drag() {
    let (mut app, run) = editing_run(45_000);
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    // Every other needle point: each selected one has two stitches stretched by a move.
    let ranges: Vec<_> = (0..45_000).step_by(2).map(|k| json!({"id": run.0, "from": k, "to": k})).collect();
    app.run("embroidery.selectNeedlePoints", json!({"ranges": ranges})).unwrap();
    settle(&mut app, &ctx, &mut time);
    let updating = |app: &mut VectorcraftApp| super::bubble(app).is_some_and(|l| l.iter().any(|(_, w)| w == "Updating…"));
    assert!(!updating(&mut app), "judged at rest");
    let grip = {
        let st = app.session.active().unwrap();
        on_screen(&app, st.doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors[0].p)
    };
    time += 0.05;
    frame(&mut app, &ctx, time, vec![Event::PointerMoved(grip), press(grip, true)]);
    for k in 1..=3 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(grip + egui::vec2(8.0 * k as f32, 0.0))]);
    }
    assert!(updating(&mut app), "during the drag: {:?}", super::bubble(&mut app));
    time += 0.05;
    frame(&mut app, &ctx, time, vec![press(grip + egui::vec2(24.0, 0.0), false)]);
    settle(&mut app, &ctx, &mut time);
    assert!(!updating(&mut app), "judged again once let go");
}
