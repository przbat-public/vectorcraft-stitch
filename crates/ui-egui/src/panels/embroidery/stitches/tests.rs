//! The Stitches tab: its rows against `embroidery.stitches`, worked out once a change and never on
//! an idle frame; a click outside stitch editing showing the stitch, inside it selecting the needle
//! point (lit in the list); the list's keys moving its focus without nudging the canvas; Go to
//! refusing junk; Move by moving the selected needle points one undo step; the grid for screen
//! readers. Headless frames with real input.

use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::embroidery::stitch_list::Filter;

use super::{Listed, Scope, listed};
use crate::{Services, VectorcraftApp};

/// An app with a filled square converted to manual stitches and selected, the Embroidery panel at
/// its Stitches tab → the app and the converted object.
fn app() -> (VectorcraftApp, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#c81e1e"})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": [id]})).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("ui.embroideryPanel", json!({"tab": "stitches", "scope": "selection", "filter": "all"})).unwrap();
    (app, id)
}

/// A node of the accessibility tree: its id, role, name, row index and whether it is selected.
type Node = (egui::accesskit::NodeId, Role, String, Option<usize>, Option<bool>);

/// One frame: the shortcuts, the panel on the right and the canvas → the shapes drawn and the
/// accessibility tree's nodes.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> (Vec<Shape>, Vec<Node>) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1100.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::shortcuts::handle(app, ui.ctx());
        egui::Panel::right("embroidery-test").exact_size(300.0).resizable(false).show(ui, |ui| super::super::show(app, ui));
        crate::canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        crate::embroidery::stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let nodes = out
        .platform_output
        .accesskit_update
        .map(|t| t.nodes.iter().map(|(id, n)| (*id, n.role(), n.label().unwrap_or_default().to_string(), n.row_index(), n.is_selected())).collect())
        .unwrap_or_default();
    let mut all = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            s => all.push(s),
        }
    }
    (all, nodes)
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    ctx.enable_accesskit();
    ctx
}

fn settle(app: &mut VectorcraftApp, ctx: &egui::Context, time: &mut f64) -> (Vec<Shape>, Vec<Node>) {
    let mut last = (vec![], vec![]);
    for _ in 0..5 {
        *time += 0.05;
        last = frame(app, ctx, *time, vec![]);
    }
    last
}

/// Where the row of record `number` is drawn: its number's words.
fn row_at(shapes: &[Shape], number: usize) -> Pos2 {
    let words = number.to_string();
    shapes
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) if t.galley.text() == words && t.pos.x > 800.0 => Some(t.visual_bounding_rect().center()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no row {number}"))
}

fn press(at: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers }
}

fn key(k: Key) -> Event {
    key_with(k, Modifiers::NONE)
}

fn key_with(k: Key, modifiers: Modifiers) -> Event {
    Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
}

/// Rows as ranges and as picked numbers: row to record and back.
#[test]
fn rows_and_records_round_trip() {
    let all = Listed { rows: super::Rows::Ranges(vec![3..7, 10..12], vec![0, 4]), counts: [0; 6], span: Some((3, 11)) };
    assert_eq!(all.len(), 6);
    assert_eq!((0..6).map(|i| all.record(i).unwrap()).collect::<Vec<_>>(), [3, 4, 5, 6, 10, 11]);
    assert_eq!((all.row_of(10), all.row_of(7), all.row_of(2), all.record(6)), (Some(4), None, None, None));
    let picked = Listed { rows: super::Rows::Picked(vec![2, 9, 40]), counts: [0; 6], span: Some((2, 40)) };
    assert_eq!((picked.record(1), picked.row_of(40), picked.row_of(3)), (Some(9), Some(2), None));
}

/// The rows the tab lists for the selection are `embroidery.stitches`' records of it; each filter's
/// count is what that filter lists; they are worked out once, and idle and hover frames work out
/// nothing; the grid tells screen readers how many rows there are.
#[test]
fn the_tab_lists_the_selections_records_once() {
    let (mut app, id) = app();
    let ctx = context();
    let mut time = 0.0;
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let listed_rows = app.run("embroidery.stitches", json!({"ids": [id], "limit": 10000})).unwrap();
    let want = listed_rows["matchedCount"].as_u64().unwrap() as usize;
    let grid = nodes.iter().find(|n| n.1 == Role::Grid).expect("a grid");
    assert_eq!(grid.2, "Stitches");
    let (_, list) = app.embroidery.stitches.list.as_ref().expect("the rows listed");
    assert_eq!(list.len(), want);
    assert_eq!(list.counts[0], want, "All");
    let rows: Vec<_> = nodes.iter().filter(|n| n.1 == Role::Row).collect();
    assert!(!rows.is_empty() && rows.iter().all(|r| r.3.is_some() && r.2.starts_with("Record ")), "{rows:?}");
    let builds = app.embroidery.stitches.builds;
    for k in 0..10 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![Event::PointerMoved(Pos2::new(900.0, 300.0 + k as f32))]);
    }
    assert_eq!(app.embroidery.stitches.builds, builds, "idle and hover frames work out nothing");
    // The whole design and the jumps: worked out once more each.
    app.run("ui.embroideryPanel", json!({"scope": "design", "filter": "jumps"})).unwrap();
    settle(&mut app, &ctx, &mut time);
    assert_eq!(app.embroidery.stitches.builds, builds + 1);
    let jumps = app.run("embroidery.stitches", json!({"kinds": ["jump"], "limit": 0})).unwrap()["matchedCount"].as_u64().unwrap() as usize;
    assert_eq!(app.embroidery.stitches.list.as_ref().unwrap().1.len(), jumps);
    assert_eq!(app.ui.embroidery_panel.stitches_scope, Scope::Design);
    assert_eq!(app.ui.embroidery_panel.stitches_filter, Filter::Jumps);
}

/// Outside stitch editing a click on a row only shows the stitch (the project owner's choice): the
/// stitch marked, nothing selected or changed.
#[test]
fn a_click_outside_editing_shows_the_stitch() {
    let (mut app, _) = app();
    let ctx = context();
    let mut time = 0.0;
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    let first = app.embroidery.stitches.list.as_ref().unwrap().1.record(3).unwrap();
    let at = row_at(&shapes, first);
    let (selection, revision) = (app.session.active().unwrap().selection.clone(), app.session.active().unwrap().doc.clone());
    for e in [Event::PointerMoved(at), press(at, true, Modifiers::NONE), press(at, false, Modifiers::NONE)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let uid = app.session.active().unwrap().uid;
    assert_eq!(app.embroidery.stitches.docs.get(&uid).and_then(|d| d.shown), Some(first), "shown");
    assert_eq!(app.session.active().unwrap().selection, selection, "nothing selected");
    assert!(app.session.active().unwrap().doc == revision, "nothing changed");
}

/// While stitches are edited a click on a row selects its needle point, lit in the list; the list's
/// ↓ moves its focus and leaves the document as it is (no nudge on the canvas); Escape gives the
/// focus back.
#[test]
fn while_editing_a_click_selects_the_needle_point_and_the_keys_stay_the_lists() {
    let (mut app, id) = app();
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    let list = app.embroidery.stitches.list.as_ref().unwrap().1.clone();
    // A needle point of the first run: a stitch record past the first few.
    let number = (0..list.len()).filter_map(|i| list.record(i)).find(|n| {
        let r = vectorcraft_engine::cmd::embroidery::stitch_list::row(
            &app.embroidery.preview.shown(app.session.active().unwrap().uid).unwrap().planned.plan,
            *n,
        )
        .unwrap();
        r.kind == vectorcraft_engine::cmd::embroidery::stitch_list::RecordKind::Stitch && r.length.is_some()
    });
    let number = number.expect("a stitch");
    let at = row_at(&shapes, number);
    for e in [Event::PointerMoved(at), press(at, true, Modifiers::NONE), press(at, false, Modifiers::NONE)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let selected: usize = app.session.active().unwrap().selection.anchors.values().map(std::collections::BTreeSet::len).sum();
    assert_eq!(selected, 1, "{:?}", app.session.active().unwrap().selection);
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    assert!(nodes.iter().any(|n| n.1 == Role::Row && n.4 == Some(true) && n.2.starts_with(&format!("Record {number}"))), "lit");
    let doc = app.session.active().unwrap().doc.clone();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::ArrowDown)]);
    let uid = app.session.active().unwrap().uid;
    let focus = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.focus);
    assert_eq!(focus, list.row_of(number).and_then(|r| list.record(r + 1)), "↓ moves the list's focus");
    assert!(app.session.active().unwrap().doc == doc, "the canvas's arrow didn't nudge the needle point");
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::Escape)]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::ArrowRight)]);
    assert!(app.session.active().unwrap().doc != doc, "the focus given back: the canvas's arrow nudges again");
}

/// Page Down moves the focus a screenful: as many rows as the list shows, the rows drawn.
#[test]
fn page_down_moves_a_screenful() {
    let (mut app, _) = app();
    let ctx = context();
    let mut time = 0.0;
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    let list = app.embroidery.stitches.list.as_ref().unwrap().1.clone();
    let first = list.record(0).unwrap();
    let at = row_at(&shapes, first);
    for e in [Event::PointerMoved(at), press(at, true, Modifiers::NONE), press(at, false, Modifiers::NONE)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let rows = nodes.iter().filter(|n| n.1 == Role::Row).count();
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key(Key::PageDown)]);
    let uid = app.session.active().unwrap().uid;
    let focus = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.focus).and_then(|f| list.row_of(f));
    // The rows drawn are those in view, and a part row at either end at most.
    assert!(focus.is_some_and(|r| r + 2 >= rows && r <= rows), "{focus:?} of {rows} drawn");
}

/// While stitches are edited a click on a cut's row selects the needle point the cut follows, as
/// `embroidery.selectNeedlePoints` selects it by that record (and once: Cmd takes it away again).
#[test]
fn a_click_on_a_cuts_row_selects_the_needle_point_it_follows() {
    let (mut app, id) = app();
    let run = app.session.active().unwrap().doc.node(NodeId(id)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.setMachineFunction", json!({"needlePoints": [{"id": run.0, "index": 5}], "function": "trim"})).unwrap();
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let cuts = app.run("embroidery.stitches", json!({"ids": [id], "kinds": ["trim"]})).unwrap();
    let cut = cuts["records"].as_array().unwrap().iter().find(|r| r["index"].is_u64()).expect("a cut the commands edit by a needle point").clone();
    let number = cut["record"].as_u64().unwrap() as usize;
    // What the command selects by that record.
    app.run("embroidery.selectNeedlePoints", json!({"records": [number, number]})).unwrap();
    let wanted = app.session.active().unwrap().selection.anchors.clone();
    assert_eq!(wanted.values().map(std::collections::BTreeSet::len).sum::<usize>(), 1, "{wanted:?}");
    app.run("select.set", json!({"ids": []})).unwrap();
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 0}]})).unwrap();
    // The cut's row, clicked.
    app.run("ui.embroideryPanel", json!({"filter": "trims"})).unwrap();
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    let at = row_at(&shapes, number);
    for e in [Event::PointerMoved(at), press(at, true, Modifiers::NONE), press(at, false, Modifiers::NONE)] {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![e]);
    }
    assert_eq!(app.session.active().unwrap().selection.anchors, wanted);
    // Cmd on it again, a second later (not a double-click), takes it away.
    time += 1.0;
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    let at = row_at(&shapes, number);
    // Cmd held as a keyboard says it (egui reads the keys held from these).
    let cmd = [Event::ModifiersChanged(Modifiers::COMMAND), Event::PointerMoved(at), press(at, true, Modifiers::COMMAND)];
    for e in [cmd.to_vec(), vec![press(at, false, Modifiers::COMMAND)], vec![Event::ModifiersChanged(Modifiers::NONE)]] {
        time += 0.05;
        frame(&mut app, &ctx, time, e);
    }
    let left: usize = app.session.active().unwrap().selection.anchors.values().map(std::collections::BTreeSet::len).sum();
    assert_eq!(left, 0);
}

/// Go to takes a number in the scope (the focus moves to it), and says so when one isn't there;
/// junk and huge numbers are refused without a panic.
#[test]
fn go_to_takes_a_number_and_refuses_junk() {
    let (mut app, _) = app();
    let ctx = context();
    let mut time = 0.0;
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let field = nodes.iter().find(|n| n.2 == "Record number").map(|n| n.0).expect("the field");
    let uid = app.session.active().unwrap().uid;
    let first = app.embroidery.stitches.list.as_ref().unwrap().1.record(5).unwrap();
    for (typed, ok) in [(first.to_string(), true), ("abc".to_string(), false), ("99999999999999999999".to_string(), false), ("0".to_string(), false)]
    {
        time += 0.05;
        let focus = Event::AccessKitActionRequest(ActionRequest { action: Action::Focus, target_node: field, target_tree: TreeId::ROOT, data: None });
        frame(&mut app, &ctx, time, vec![focus]);
        time += 0.05;
        frame(&mut app, &ctx, time, vec![key_with(Key::A, Modifiers::COMMAND), Event::Text(typed.clone()), key(Key::Enter)]);
        let d = app.embroidery.stitches.docs.get(&uid).unwrap();
        if ok {
            assert_eq!(d.focus, Some(first), "{typed}");
        } else {
            assert!(d.said.as_deref().is_some_and(|s| s.starts_with("No record numbered")), "{typed}: {:?}", d.said);
        }
    }
}

/// Go to a record far down the list (in its middle, so the end's clamp can't bring it in) scrolls
/// its row into view.
#[test]
fn go_to_scrolls_the_record_into_view() {
    let (mut app, _) = app();
    app.run("ui.embroideryPanel", json!({"scope": "design"})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    let (shapes, nodes) = settle(&mut app, &ctx, &mut time);
    let list = app.embroidery.stitches.list.as_ref().unwrap().1.clone();
    let last = list.record(list.len() / 2).unwrap();
    assert!(!shapes.iter().any(|s| matches!(s, Shape::Text(t) if t.galley.text() == last.to_string())), "not in view at first");
    let field = nodes.iter().find(|n| n.2 == "Record number").map(|n| n.0).expect("the field");
    let focus = Event::AccessKitActionRequest(ActionRequest { action: Action::Focus, target_node: field, target_tree: TreeId::ROOT, data: None });
    time += 0.05;
    frame(&mut app, &ctx, time, vec![focus]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key_with(Key::A, Modifiers::COMMAND), Event::Text(last.to_string()), key(Key::Enter)]);
    let (shapes, nodes) = settle(&mut app, &ctx, &mut time);
    row_at(&shapes, last);
    // Rows are numbered for screen readers by their place: the one shown is the one in focus.
    let shown = nodes.iter().find(|n| n.1 == Role::Row && n.2.starts_with(&format!("Record {last}:"))).and_then(|n| n.3);
    assert_eq!(shown, list.row_of(last));
}

/// A record shown from the list: its row in view, ringed in focus, and its needle point ringed on
/// the canvas while the tab shows.
#[test]
fn a_record_shown_is_in_view_and_marked_on_the_canvas() {
    let (mut app, _) = app();
    app.run("ui.embroideryPanel", json!({"scope": "design"})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let ctx = context();
    let mut time = 0.0;
    settle(&mut app, &ctx, &mut time);
    let list = app.embroidery.stitches.list.as_ref().unwrap().1.clone();
    let number = list.record(list.len() / 2).unwrap();
    super::reveal(&mut app, number);
    let (shapes, nodes) = settle(&mut app, &ctx, &mut time);
    row_at(&shapes, number);
    assert!(nodes.iter().any(|n| n.1 == Role::Row && n.2.starts_with(&format!("Record {number}:"))));
    // Rings round the needle point, where the canvas draws it.
    let uid = app.session.active().unwrap().uid;
    let row = vectorcraft_engine::cmd::embroidery::stitch_list::row(&app.embroidery.preview.shown(uid).unwrap().planned.plan, number).unwrap();
    let pt = vectorcraft_engine::cmd::embroidery::mm_to_pt;
    let at =
        crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap()).to_screen(vectorcraft_geom::Point::new(pt(row.at.x), pt(row.at.y)));
    let rings = |shapes: &[Shape]| {
        shapes.iter().filter(|s| matches!(s, Shape::Circle(c) if c.radius == super::MARK_RING && c.center.distance(at) < 0.5)).count()
    };
    assert_eq!(rings(&shapes), 2, "a two-tone ring");
    app.run("ui.embroideryPanel", json!({"tab": "check"})).unwrap();
    let (shapes, _) = settle(&mut app, &ctx, &mut time);
    assert_eq!(rings(&shapes), 0, "only while the tab shows");
}

/// X and Y are millimetres from the rulers' zero point (the project owner's choice): moved an inch
/// right and half an inch down, every row's X is 25.4 less and its Y 12.7 less.
#[test]
fn x_and_y_are_measured_from_the_rulers_zero() {
    let (mut app, _) = app();
    let ctx = context();
    let mut time = 0.0;
    let places = |nodes: &[Node]| -> Vec<(String, f64, f64)> {
        let mut out: Vec<(String, f64, f64)> = nodes
            .iter()
            .filter(|n| n.1 == Role::Row)
            .filter_map(|n| {
                let (head, rest) = n.2.split_once(", X ")?;
                let (x, rest) = rest.split_once(" mm, Y ")?;
                let y = rest.split(" mm").next()?;
                Some((head.to_string(), x.parse().ok()?, y.parse().ok()?))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let before = places(&nodes);
    assert!(before.len() > 5, "{nodes:?}");
    app.run("document.setRulerOrigin", json!({"x": 72, "y": 36})).unwrap();
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let after = places(&nodes);
    assert_eq!(before.len(), after.len());
    for ((name, x0, y0), (same, x1, y1)) in before.iter().zip(&after) {
        assert_eq!(name, same);
        assert!((x0 - x1 - 25.4).abs() < 0.051 && (y0 - y1 - 12.7).abs() < 0.051, "{name}: {x0},{y0} → {x1},{y1}");
    }
}

/// With needle points selected, Move by X 0,3 typed in Polish (a comma) moves them 0.3 mm in one undo
/// step.
#[test]
fn move_by_moves_the_selected_needle_points() {
    let (mut app, id) = app();
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let run = app.session.active().unwrap().doc.node(NodeId(id)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 2}]})).unwrap();
    let before = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors[2].p;
    // The interface in Polish (the app sets the language from its preference each frame).
    app.session.prefs.interface_language = "pl".into();
    let ctx = context();
    let mut time = 0.0;
    let (_, nodes) = settle(&mut app, &ctx, &mut time);
    let field = nodes.iter().find(|n| n.2 == "Przesuń o X").map(|n| n.0).expect("the Move by X field, named in Polish");
    let steps = app.session.active().unwrap().history.undo.len();
    let focus = Event::AccessKitActionRequest(ActionRequest { action: Action::Focus, target_node: field, target_tree: TreeId::ROOT, data: None });
    time += 0.05;
    frame(&mut app, &ctx, time, vec![focus]);
    time += 0.05;
    frame(&mut app, &ctx, time, vec![key_with(Key::A, Modifiers::COMMAND), Event::Text("0,3".into()), key(Key::Enter)]);
    settle(&mut app, &ctx, &mut time);
    crate::i18n::set_current(crate::i18n::Lang::EN);
    let after = app.session.active().unwrap().doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors[2].p;
    assert!((after.x - before.x - vectorcraft_engine::cmd::embroidery::mm_to_pt(0.3)).abs() < 1e-6, "{before:?} → {after:?}");
    assert_eq!(app.session.active().unwrap().history.undo.len(), steps + 1);
}

/// One pass over the scope gives every filter's count; a filter's rows are its matches.
#[test]
fn every_filters_count_in_one_pass() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 100, "height": 60})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 180, "y": 20, "width": 60, "height": 60})).unwrap();
    let opts = vectorcraft_engine::EmbroideryOptions::default();
    let plan = s.embroidery_plan(&opts).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let limits = vectorcraft_engine::cmd::embroidery::stitch_limits(&doc, None);
    let flags = vectorcraft_engine::cmd::embroidery::stitch_list::Flags::of(&plan, &limits);
    let all = listed(&flags, None, Filter::All);
    for (k, f) in Filter::ALL.into_iter().enumerate() {
        let picked = listed(&flags, None, f);
        assert_eq!(picked.len(), all.counts[k], "{f:?}");
        assert!((0..picked.len()).all(|i| flags.matches(picked.record(i).unwrap(), f)), "{f:?}");
    }
}

/// A seeded sweep, as a property test would (this crate has no proptest): clicks on the rows drawn
/// (with Shift, Cmd or neither), the list's keys and others, words typed into Go to, the scope and
/// the filter switched, needle points moved and deleted, undo and redo, the selection changed and
/// stitch editing started and left, a frame after each. Nothing panics, caught by the engine or not
/// (the panic hook counts them on this thread, where the light designs are planned), and at the end
/// the whole design's rows are `embroidery.stitches`' records.
#[test]
fn random_input_never_breaks_the_tab() {
    use std::cell::Cell;
    thread_local!(static CAUGHT: Cell<usize> = const { Cell::new(0) });
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let printed = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            CAUGHT.with(|c| c.set(c.get() + 1));
            printed(info);
        }));
    });
    /// A small linear congruential generator: the same numbers every run.
    struct Seeded(u64);
    impl Seeded {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
        fn pick<'a, T>(&mut self, of: &'a [T]) -> &'a T {
            &of[(self.next() as usize) % of.len()]
        }
    }
    const RUNS: u64 = 4;
    const STEPS: usize = 160;
    let keys = [
        Key::ArrowUp,
        Key::ArrowDown,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::PageUp,
        Key::PageDown,
        Key::Home,
        Key::End,
        Key::Space,
        Key::Enter,
        Key::Escape,
        Key::Delete,
        Key::Tab,
    ];
    let mods = [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND, Modifiers::ALT];
    let typed = ["12", "abc", "0", "-3", "99999999999999999999", "4–9", "١٢", "", " 7 ", "1e9"];
    let scopes = ["selection", "design"];
    let filters = ["all", "jumps", "trims", "stops", "short", "long"];
    CAUGHT.with(|c| c.set(0));
    for seed in 0..RUNS {
        let (mut app, id) = app();
        let other = app.run("shape.rectangle", json!({"x": 200, "y": 160, "width": 60, "height": 50})).unwrap()["id"].as_u64().unwrap();
        let ctx = context();
        let mut time = 0.0;
        let mut rng = Seeded(seed + 1);
        let (mut shapes, mut nodes) = settle(&mut app, &ctx, &mut time);
        for step in 0..STEPS {
            let events = match rng.next() % 10 {
                0 | 1 => {
                    let rows: Vec<Pos2> = shapes
                        .iter()
                        .filter_map(|s| match s {
                            Shape::Text(t)
                                if t.pos.x > 800.0
                                    && !t.galley.text().is_empty()
                                    && t.galley.text().chars().all(|c| c.is_ascii_digit() || c == ',') =>
                            {
                                Some(t.visual_bounding_rect().center())
                            }
                            _ => None,
                        })
                        .collect();
                    match rows.is_empty() {
                        true => vec![],
                        false => {
                            let at = *rng.pick(&rows);
                            let m = *rng.pick(&mods);
                            vec![
                                Event::PointerMoved(at),
                                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: m },
                                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: m },
                            ]
                        }
                    }
                }
                2 | 3 => vec![key_with(*rng.pick(&keys), *rng.pick(&mods))],
                4 => match nodes.iter().find(|n| n.2 == "Record number").map(|n| n.0) {
                    Some(field) => {
                        let focus = Event::AccessKitActionRequest(ActionRequest {
                            action: Action::Focus,
                            target_node: field,
                            target_tree: TreeId::ROOT,
                            data: None,
                        });
                        time += 0.05;
                        frame(&mut app, &ctx, time, vec![focus]);
                        vec![key_with(Key::A, Modifiers::COMMAND), Event::Text(rng.pick(&typed).to_string()), key(Key::Enter)]
                    }
                    None => vec![],
                },
                5 => {
                    let p = json!({"scope": rng.pick(&scopes), "filter": rng.pick(&filters)});
                    app.run("ui.embroideryPanel", p).unwrap();
                    vec![]
                }
                6 => {
                    let dx = (rng.next() % 9) as f64 - 4.0;
                    let cmd = *rng.pick(&["embroidery.moveNeedlePoints", "embroidery.deleteNeedlePoints", "edit.undo", "edit.redo"]);
                    let p = if cmd == "embroidery.moveNeedlePoints" { json!({"dx": dx}) } else { json!({}) };
                    if let Err(e) = app.run(cmd, p) {
                        assert!(!e.contains("internal"), "seed {seed}, step {step}: {cmd}: {e}");
                    }
                    vec![]
                }
                7 => {
                    let ids = match rng.next() % 3 {
                        0 => json!([id]),
                        1 => json!([other]),
                        _ => json!([]),
                    };
                    let _ = app.run("select.set", json!({ "ids": ids }));
                    vec![]
                }
                _ => {
                    if app.session.active().is_some_and(|st| st.isolation.is_some()) {
                        let _ = app.run("object.exitIsolation", json!({}));
                    } else if app.session.active().is_some_and(|st| st.doc.node(NodeId(id)).is_some()) {
                        let _ = app.run("object.isolate", json!({ "id": id }));
                        let _ = app.run("view.stitchPreview", json!({"on": true}));
                        let _ = app.run("tool.select", json!({"tool": "embroidery"}));
                    }
                    vec![]
                }
            };
            time += 0.05;
            (shapes, nodes) = frame(&mut app, &ctx, time, events);
            assert_eq!(CAUGHT.with(Cell::get), 0, "seed {seed}, step {step}: a panic");
        }
        // The whole design's rows are the command's records.
        app.run("ui.embroideryPanel", json!({"scope": "design", "filter": "all"})).unwrap();
        settle(&mut app, &ctx, &mut time);
        let want = app.run("embroidery.stitches", json!({"limit": 0})).unwrap()["recordCount"].as_u64().unwrap() as usize;
        let got = app.embroidery.stitches.list.as_ref().map(|(_, l)| l.len());
        assert_eq!(got, Some(want), "seed {seed}");
    }
}
