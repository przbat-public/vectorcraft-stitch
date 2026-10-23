//! The Sewing order tab: its rows and their heights, collapsing, the rows laid out only in view,
//! selection both ways, and the figures under the list.

use egui::Shape;
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;
use vectorcraft_engine::cmd::embroidery::reorder::place;

fn app_with(squares: usize) -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for k in 0..squares {
        let color = ["#d01818", "#1830c8"][k % 2];
        // Nothing selected, so that only the new square takes the colour.
        app.run("select.set", json!({"ids": []})).unwrap();
        app.run("paint.setFill", json!({ "color": color })).unwrap();
        let (x, y) = (10 + 60 * (k % 18), 10 + 60 * (k / 18));
        app.run("shape.rectangle", json!({"x": x, "y": y, "width": 25, "height": 25})).unwrap();
    }
    app.run("select.set", json!({"ids": []})).unwrap();
    app
}

/// One headless frame of the panel at `time` with `events` → the words drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> Vec<String> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        super::super::show(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut words = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) => words.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    words
}

fn ready(app: &mut VectorcraftApp, ctx: &egui::Context) -> Vec<String> {
    for t in [0.0, 0.05] {
        frame(app, ctx, t, vec![]);
    }
    frame(app, ctx, 0.1, vec![])
}

fn order_of(app: &VectorcraftApp) -> Arc<SewingOrder> {
    listed_of(app).order.clone()
}

fn listed_of(app: &VectorcraftApp) -> Arc<Listed> {
    app.embroidery.order.read.as_ref().map(|(_, l)| l.clone()).expect("the order read")
}

#[test]
fn the_rows_their_heights_and_collapsing() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    // Two colours side by side, nothing on anything: red's two squares join, then blue's.
    assert_eq!(order.blocks.len(), 2);
    let (all, tops) = rows(&order, &HashSet::new());
    assert_eq!(all, [Row::Header(0), Row::Unit(0, 0), Row::Unit(0, 1), Row::Change(0), Row::Header(1), Row::Unit(1, 0), Row::Unit(1, 1)]);
    assert_eq!(tops.last().copied(), Some(2.0 * HEADER_ROW + 4.0 * UNIT_ROW + CHANGE_ROW));
    let collapsed: HashSet<usize> = [1].into();
    let (shown, _) = rows(&order, &collapsed);
    assert_eq!(shown, [Row::Header(0), Row::Change(0), Row::Header(1), Row::Unit(1, 0), Row::Unit(1, 1)]);
}

#[test]
fn the_list_and_its_figures() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    let words = ready(&mut app, &ctx);
    assert!(words.iter().any(|w| w == "2 thread blocks · 2,800 stitches" || w.starts_with("2 thread blocks · ")), "{words:?}");
    assert!(words.iter().any(|w| w.starts_with("Sewn from the bottom")), "{words:?}");
    assert!(words.iter().any(|w| w == "Thread change"), "{words:?}");
    assert!(words.iter().any(|w| w.starts_with("fill · tatami")), "{words:?}");
    assert!(words.iter().filter(|w| w.starts_with("<Rectangle>")).count() == 4, "{words:?}");
    for figure in ["Design", "Stitches", "Colours", "Thread changes", "Size", "Time"] {
        assert!(words.iter().any(|w| w == figure), "{figure}: {words:?}");
    }
    assert!(words.iter().any(|w| w.starts_with("The time at 650 stitches a minute")), "{words:?}");
}

/// Only the rows in view are laid out: a long list draws a screenful.
#[test]
fn only_the_rows_in_view_are_drawn() {
    let mut app = app_with(300);
    let ctx = egui::Context::default();
    let words = ready(&mut app, &ctx);
    let order = order_of(&app);
    assert_eq!(order.unit_count, 300);
    let drawn = words.iter().filter(|w| w.starts_with("<Rectangle>")).count();
    assert!(drawn > 3 && drawn < 40, "{drawn} rows drawn");
}

/// A click on a row selects its object; Shift-click the rows between; a click on a header its
/// block's objects. A row whose object is selected from the canvas is highlighted.
#[test]
fn rows_and_the_selection() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let listed = listed_of(&app);
    let order = listed.order.clone();
    let (list, _) = rows(&order, &HashSet::new());
    let first = order.blocks[0].units[0].id;
    select(&mut app, &listed, &list, Row::Unit(0, 0), egui::Modifiers::NONE);
    assert_eq!(app.session.doc().unwrap().selection.objects, [NodeId(first)]);
    let last = order.blocks[1].units[1].id;
    select(&mut app, &listed, &list, Row::Unit(1, 1), egui::Modifiers::SHIFT);
    assert_eq!(app.session.doc().unwrap().selection.objects.len(), 4, "the range");
    assert!(app.session.doc().unwrap().selection.objects.contains(&NodeId(last)));
    select(&mut app, &listed, &list, Row::Header(1), egui::Modifiers::NONE);
    let block: Vec<NodeId> = order.blocks[1].units.iter().map(|u| NodeId(u.id)).collect();
    assert_eq!(app.session.doc().unwrap().selection.objects, block);
    select(&mut app, &listed, &list, Row::Unit(0, 0), egui::Modifiers::COMMAND);
    assert_eq!(app.session.doc().unwrap().selection.objects.len(), 3, "added");
    // From the canvas: the selection lights its rows, and a header when all its rows are.
    app.run("select.set", json!({ "ids": [first] })).unwrap();
    let st = app.session.doc().unwrap();
    let lit = covered(&mut OrderState::default(), st.uid, st.revision, &st.doc.clone(), &st.selection.objects.clone());
    assert!(row_selected(&order, Row::Unit(0, 0), &lit));
    assert!(!row_selected(&order, Row::Unit(0, 1), &lit));
    assert!(!row_selected(&order, Row::Header(0), &lit));
    app.run("select.all", json!({})).unwrap();
    let st = app.session.doc().unwrap();
    let lit = covered(&mut OrderState::default(), st.uid, st.revision, &st.doc.clone(), &st.selection.objects.clone());
    assert!(row_selected(&order, Row::Header(0), &lit) && row_selected(&order, Row::Header(1), &lit));
}

/// A collapsed block shows its header alone.
/// What the rows show is worked out once for an order: a block's objects, each counted once, and
/// whether a unit's part is numbered (its object has another of that part in the block).
#[test]
fn a_blocks_objects_and_parts_are_worked_out_once() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let mut order = (*order_of(&app)).clone();
    let block = &mut order.blocks[0];
    let n = block.units.len();
    assert!(n >= 2, "{n} units");
    let mut twin = block.units[0].clone();
    twin.part_index = Some(2);
    block.units.push(twin);
    let listed = Listed::new(order);
    assert_eq!(listed.object_count(0), n, "the second part's object counted once");
    assert_eq!(listed.objects_of(Row::Header(0)).len(), n);
    assert!(listed.several(0, 0) && listed.several(0, n), "both parts numbered");
    assert!(!listed.several(0, 1));
    assert!(how_sewn(&listed.order.blocks[0].units[n], true).contains(" 2"));
}

/// A row dragged to the list's bottom edge and held there scrolls the list down, and back up at
/// its top edge.
#[test]
fn a_drag_held_at_the_lists_edge_scrolls_it() {
    let mut app = app_with(300);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let r = app.embroidery.order.list_rect.expect("the list shown");
    let row = egui::pos2(r.center().x, r.top() + HEADER_ROW + UNIT_ROW / 2.0);
    let moved = |p: egui::Pos2| egui::Event::PointerMoved(p);
    let button = |p: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut time = 0.2;
    for events in [vec![moved(row)], vec![button(row, true)], vec![moved(row + egui::vec2(0.0, 12.0))]] {
        time += 0.05;
        frame(&mut app, &ctx, time, events);
    }
    assert!(app.embroidery.order.drag.is_some(), "dragging");
    assert_eq!(app.embroidery.order.scrolled, 0.0);
    let bottom = egui::pos2(row.x, r.bottom() - 4.0);
    for _ in 0..10 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![moved(bottom)]);
    }
    let down = app.embroidery.order.scrolled;
    assert!(down > 50.0, "scrolled down {down} points");
    // A scroll asked shows a pass or two later (egui takes it in as the list ends).
    let top = egui::pos2(row.x, r.top() + 4.0);
    for _ in 0..8 {
        time += 0.05;
        frame(&mut app, &ctx, time, vec![moved(top)]);
    }
    assert!(app.embroidery.order.scrolled < down, "and back up");
    frame(&mut app, &ctx, time + 0.05, vec![button(top, false)]);
}

/// Where `row` shows in the list drawn last (not scrolled): the middle of its upper or lower half,
/// left of its step buttons.
fn at_row(app: &VectorcraftApp, row: Row, upper: bool) -> egui::Pos2 {
    let r = app.embroidery.order.list_rect.expect("the list shown");
    let (all, tops) = rows(&order_of(app), &HashSet::new());
    let i = all.iter().position(|x| *x == row).expect("the row listed");
    egui::pos2(r.left() + r.width() * 0.4, r.top() + tops[i] + row.height() * if upper { 0.25 } else { 0.75 })
}

fn moved(p: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(p)
}

fn button(p: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

/// Press on `from` and drag to `to`, frame by frame from `time` → the words of the last frame,
/// the button still down.
fn drag_to(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, from: egui::Pos2, to: egui::Pos2) -> Vec<String> {
    let mut words = vec![];
    for (k, events) in
        [vec![moved(from)], vec![button(from, true)], vec![moved(from + egui::vec2(0.0, 12.0))], vec![moved(to)], vec![]].into_iter().enumerate()
    {
        words = frame(app, ctx, time + 0.05 * k as f64, events);
    }
    words
}

fn sewn_place(app: &VectorcraftApp, id: u64) -> usize {
    order_of(app).blocks.iter().flat_map(|b| b.units.iter()).position(|u| u.id == id).expect("listed")
}

/// A row dragged over another says where its object would go before the button is let go, and the
/// release drops it there, as one undo step that selects it; dragged onto itself, or with its
/// object locked, it says why it can't, and the release changes nothing.
#[test]
fn a_dragged_row_says_where_it_lands_and_drops_on_release() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let last = order_of(&app).blocks[1].units[1].id;
    let undo = app.session.doc().unwrap().history.undo.len();
    let (from, to) = (at_row(&app, Row::Unit(1, 1), true), at_row(&app, Row::Unit(0, 0), true));
    let words = drag_to(&mut app, &ctx, 0.2, from, to);
    assert!(app.embroidery.order.drag.is_some(), "dragging");
    assert!(words.iter().any(|w| w == "Before <Rectangle>"), "the hint before the release: {words:?}");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo, "nothing moved yet");
    frame(&mut app, &ctx, 0.5, vec![button(to, false)]);
    assert!(app.embroidery.order.drag.is_none());
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    assert_eq!(app.session.doc().unwrap().selection.objects, [NodeId(last)]);
    for t in [0.6, 0.65, 0.7] {
        frame(&mut app, &ctx, t, vec![]);
    }
    assert_eq!(sewn_place(&app, last), 0, "sewn first");
    // Onto itself: the order is that already.
    let undo = app.session.doc().unwrap().history.undo.len();
    let (from, to) = (at_row(&app, Row::Unit(0, 0), false), at_row(&app, Row::Unit(0, 0), true));
    let words = drag_to(&mut app, &ctx, 1.0, from, to);
    assert!(words.iter().any(|w| w.contains("sewing order is that already")), "{words:?}");
    frame(&mut app, &ctx, 1.5, vec![button(to, false)]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo, "refused: nothing moved");
    // Locked: it can't move.
    let locked = order_of(&app).blocks.last().unwrap().units.last().unwrap().id;
    app.run("layer.setProps", json!({"id": locked, "locked": true})).unwrap();
    for t in [2.0, 2.05, 2.1] {
        frame(&mut app, &ctx, t, vec![]);
    }
    let undo = app.session.doc().unwrap().history.undo.len();
    let rows_now = rows(&order_of(&app), &HashSet::new()).0;
    let (b, u) = rows_now.iter().rev().find_map(|r| if let Row::Unit(b, u) = r { Some((*b, *u)) } else { None }).unwrap();
    let (from, to) = (at_row(&app, Row::Unit(b, u), true), at_row(&app, Row::Unit(0, 0), true));
    let words = drag_to(&mut app, &ctx, 2.2, from, to);
    assert!(words.iter().any(|w| w.contains("is locked")), "{words:?}");
    frame(&mut app, &ctx, 2.6, vec![button(to, false)]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo, "refused: nothing moved");
}

/// A lit row's ↑ and ↓ buttons sew its object a step earlier and later, without a drag.
#[test]
fn a_lit_rows_buttons_sew_it_earlier_and_later() {
    use vectorcraft_engine::cmd::embroidery::reorder::{Step, step_move};
    let mut app = app_with(6);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let mut time = 0.2;
    for (k, step) in [(0, Step::Earlier), (1, Step::Later)] {
        let order = order_of(&app);
        let id = order.blocks[1].units[1].id;
        app.run("select.set", json!({ "ids": [id] })).unwrap();
        for _ in 0..2 {
            time += 0.05;
            frame(&mut app, &ctx, time, vec![]);
        }
        let uid = app.session.active().unwrap().uid;
        let plan = app.embroidery.preview.shown(uid).unwrap().planned.plan.clone();
        assert!(step_move(&app.session.doc().unwrap().doc.clone(), &plan, &[NodeId(id)], step).is_ok(), "{step:?} moves it");
        let before = sewn_place(&app, id);
        let undo = app.session.doc().unwrap().history.undo.len();
        let r = app.embroidery.order.list_rect.unwrap();
        let (all, tops) = rows(&order, &HashSet::new());
        let i = all.iter().position(|x| *x == Row::Unit(1, 1)).unwrap();
        let at = egui::pos2(r.right() - STEP_BUTTONS_FROM_RIGHT + STEP_BUTTON_PITCH * k as f32, r.top() + tops[i] + UNIT_ROW * UNIT_SECOND_LINE);
        for events in [vec![moved(at)], vec![button(at, true)], vec![button(at, false)]] {
            time += 0.05;
            frame(&mut app, &ctx, time, events);
        }
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1, "{step:?}: one undo step");
        for _ in 0..3 {
            time += 0.05;
            frame(&mut app, &ctx, time, vec![]);
        }
        let after = sewn_place(&app, id);
        assert!(if step == Step::Earlier { after < before } else { after > before }, "{step:?}: from {before} to {after}");
    }
}

/// No drag starts while the list's plan is of an earlier state, in isolation mode, or while an
/// opacity mask or a pattern is edited.
#[test]
fn no_drag_on_a_stale_list_or_in_the_edit_modes() {
    use crate::embroidery::stitch_preview::{Held, StitchPreview};
    let held = Held::default();
    let mut app = app_with(4);
    app.embroidery.preview = StitchPreview::with_runner(Box::new(held.clone()));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, 0.0, vec![]);
    held.release();
    ready(&mut app, &ctx);
    let ids: Vec<u64> = order_of(&app).blocks.iter().flat_map(|b| b.units.iter().map(|u| u.id)).collect();
    let mut time = 1.0;
    let mut tried = |app: &mut VectorcraftApp, state: &str| {
        let undo = app.session.doc().unwrap().history.undo.len();
        for _ in 0..3 {
            time += 0.05;
            frame(app, &ctx, time, vec![]);
        }
        let (from, to) = (at_row(app, Row::Unit(1, 1), true), at_row(app, Row::Unit(0, 0), true));
        let words = drag_to(app, &ctx, time, from, to);
        assert!(words.iter().any(|w| w.starts_with("<Rectangle>")), "{state}: the list shows: {words:?}");
        assert!(app.embroidery.order.drag.is_none(), "{state}: no drag");
        time += 0.3;
        frame(app, &ctx, time, vec![button(to, false)]);
        assert_eq!(app.session.doc().unwrap().history.undo.len(), undo, "{state}: nothing moved");
    };
    // An edit whose plan is held: the list shown is the plan before's.
    app.run("select.set", json!({ "ids": [ids[0]] })).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    tried(&mut app, "stale");
    held.release();
    app.run("select.set", json!({ "ids": [] })).unwrap();
    app.run("object.isolate", json!({ "id": ids[0] })).unwrap();
    held.release();
    tried(&mut app, "isolation");
    app.run("object.exitIsolation", json!({})).unwrap();
    held.release();
    app.run("select.set", json!({ "ids": [ids[0], ids[1]] })).unwrap();
    app.run("transparency.makeOpacityMask", json!({})).unwrap();
    app.run("transparency.editOpacityMask", json!({})).unwrap();
    held.release();
    tried(&mut app, "mask edit");
    app.run("transparency.stopEditingOpacityMask", json!({})).unwrap();
    app.run("select.set", json!({ "ids": [ids[2]] })).unwrap();
    app.run("object.pattern.make", json!({})).unwrap();
    held.release();
    tried(&mut app, "pattern edit");
}

/// One headless frame with a screen reader on → the accessibility tree.
fn tree_frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) -> egui::accesskit::TreeUpdate {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 700.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        super::super::show(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    out.platform_output.accesskit_update.expect("an accessibility tree")
}

/// The keyboard reaches every row: ← folds a block's header and → unfolds it; ↓ moves to the next
/// row, bringing it into view, so a long list is walked to its end.
#[test]
fn the_keyboard_folds_blocks_and_walks_the_rows() {
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};
    let mut app = app_with(300);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ready(&mut app, &ctx);
    let key = |key: egui::Key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    let mut time = 0.2;
    let mut step = |app: &mut VectorcraftApp, events: Vec<egui::Event>| {
        time += 0.05;
        tree_frame(app, &ctx, time, events)
    };
    let tree = step(&mut app, vec![]);
    let (header, _) = tree.nodes.iter().find(|(_, n)| n.role() == Role::TreeItem && n.label().is_some_and(|l| l.starts_with("1 "))).unwrap();
    let focus =
        egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Focus, target_node: *header, target_tree: TreeId::ROOT, data: None });
    step(&mut app, vec![focus]);
    assert_eq!(step(&mut app, vec![]).focus, *header, "the header has the focus");
    let block = order_of(&app).blocks[0].block;
    step(&mut app, vec![key(egui::Key::ArrowLeft)]);
    let uid = app.session.active().unwrap().uid;
    assert!(app.embroidery.order.folded(uid).contains(&block), "← folds it");
    step(&mut app, vec![key(egui::Key::ArrowRight)]);
    assert!(!app.embroidery.order.folded(uid).contains(&block), "→ unfolds it");
    // Down the list, past what shows.
    let mut tree = step(&mut app, vec![]);
    for _ in 0..40 {
        step(&mut app, vec![key(egui::Key::ArrowDown)]);
        // A row out of view takes a pass or two to come into it.
        for _ in 0..3 {
            tree = step(&mut app, vec![]);
        }
    }
    let focused = tree.nodes.iter().find(|(id, _)| *id == tree.focus).map(|(_, n)| n.clone()).expect("a row has the focus");
    assert_eq!((focused.role(), focused.level()), (Role::TreeItem, Some(2)), "a unit row");
    assert!(app.embroidery.order.scrolled > 0.0, "brought into view");
}

/// A row's words keep to the room they have: a long name stops short of the stitch count, cut with
/// "…", and a lit row's second line short of its Sew Earlier and Sew Later buttons.
#[test]
fn a_rows_words_keep_to_their_room() {
    let mut app = app_with(2);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let first = order_of(&app).blocks[0].units[0].id;
    let long = "A very long name for one small square of red thread ".repeat(4);
    app.run("layer.setProps", json!({"id": first, "name": long.trim()})).unwrap();
    app.run("select.set", json!({"ids": [first]})).unwrap();
    let raw = egui::RawInput {
        time: Some(1.0),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 700.0))),
        ..Default::default()
    };
    let mut shapes = vec![];
    for t in [1.0, 1.05, 1.1] {
        let mut out = ctx.run_ui(egui::RawInput { time: Some(t), ..raw.clone() }, |ui| {
            super::super::show(&mut app, ui);
            stitch_preview::schedule(&mut app, ui.ctx());
        });
        out.textures_delta.clear();
        shapes = out.shapes.into_iter().map(|c| c.shape).collect::<Vec<_>>();
    }
    let mut texts = vec![];
    while let Some(s) = shapes.pop() {
        match s {
            Shape::Vec(v) => shapes.extend(v),
            Shape::Text(t) => texts.push(t),
            _ => {}
        }
    }
    let r = app.embroidery.order.list_rect.unwrap();
    let name = texts.iter().find(|t| t.galley.text().starts_with("A very long name")).expect("the name drawn");
    assert!(name.pos.x + name.galley.size().x < r.right() - ROW_END_PAD, "the name stops short: {:?}", name.galley.rect);
    assert!(name.galley.rows.len() == 1, "on one line");
    let how = texts.iter().find(|t| t.galley.text().starts_with("fill") && t.pos.y > name.pos.y).expect("the second line drawn");
    assert!(how.pos.x + how.galley.size().x < r.right() - STEP_BUTTONS_FROM_RIGHT - STEP_BUTTON / 2.0, "short of the buttons");
}

/// A list cut at its limit says how many rows it leaves out; a whole one says nothing.
#[test]
fn a_list_cut_short_says_how_many_rows_it_leaves_out() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let uid = app.session.active().unwrap().uid;
    let shown = app.embroidery.preview.shown(uid).unwrap();
    let (generation, plan) = (shown.planned.generation, shown.planned.plan.clone());
    let reply = (**shown.reply(&query()).unwrap().result.as_ref().unwrap()).clone();
    let words_of = |app: &mut VectorcraftApp, reply: &Value| {
        let mut out = ctx.run_ui(egui::RawInput { time: Some(1.0), ..Default::default() }, |ui| {
            let shown = Shown { generation, reply, of: generation, current: true, stats: None, stale: false, plan: plan.clone() };
            show(app, ui, shown);
        });
        out.textures_delta.clear();
        let mut words = vec![];
        let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
        while let Some(s) = pending.pop() {
            match s {
                Shape::Vec(v) => pending.extend(v),
                Shape::Text(t) => words.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        words
    };
    assert!(!words_of(&mut app, &reply).iter().any(|w| w.contains("more rows")), "a whole list");
    let mut cut = reply.clone();
    cut["truncated"] = json!(true);
    cut["unitCount"] = json!(reply["unitCount"].as_u64().unwrap() + 5);
    // The order is read once a plan: forgotten, so the cut reply is read.
    app.embroidery.order.read = None;
    let words = words_of(&mut app, &cut);
    assert!(words.iter().any(|w| w == "and 5 more rows aren't listed (the list shows the first 4)"), "{words:?}");
}

/// A list cut inside a block (the engine's own reply, at a small limit): the cut block shows no
/// thread change after the units listed, since more of its own follow; the blocks before keep
/// theirs.
#[test]
fn a_block_cut_by_the_limit_has_no_thread_change_row() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for k in 0..6 {
        app.run("select.set", json!({"ids": []})).unwrap();
        let color = ["#d01818", "#1830c8", "#18a030"][k % 3];
        app.run("paint.setFill", json!({ "color": color })).unwrap();
        app.run("shape.rectangle", json!({"x": 10 + 60 * k, "y": 10, "width": 25, "height": 25})).unwrap();
    }
    let reply = app.session.execute("embroidery.sequence", &json!({"limit": 3})).unwrap();
    let order: SewingOrder = serde_json::from_value(reply).unwrap();
    assert!(order.truncated);
    assert_eq!(order.blocks.iter().map(|b| (b.units.len(), b.unit_count)).collect::<Vec<_>>(), [(2, 2), (1, 2)], "cut inside the second block");
    let (all, _) = rows(&order, &HashSet::new());
    assert_eq!(all, [Row::Header(0), Row::Unit(0, 0), Row::Unit(0, 1), Row::Change(0), Row::Header(1), Row::Unit(1, 0)]);
}

#[test]
fn a_header_collapses() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    let block = order.blocks[0].block;
    // The rows are laid out once for an order and its folds, not every frame.
    let laid = |app: &VectorcraftApp| app.embroidery.order.laid_out.as_ref().map(|(_, _, r)| r.clone()).unwrap();
    let before = laid(&app);
    frame(&mut app, &ctx, 0.5, vec![]);
    assert!(Arc::ptr_eq(&before, &laid(&app)), "the same rows");
    app.embroidery.order.toggle_fold(app.session.active().unwrap().uid, block);
    let words = frame(&mut app, &ctx, 1.0, vec![]);
    assert_eq!(words.iter().filter(|w| w.starts_with("<Rectangle>")).count(), 2, "{words:?}");
    assert!(!Arc::ptr_eq(&before, &laid(&app)), "laid out again once folded");
}

#[test]
fn where_a_drop_on_a_row_lands() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    // Units 1 and 2 in the first block, 3 and 4 in the second.
    assert_eq!((gap_at(&order, Row::Unit(0, 1), true), gap_at(&order, Row::Unit(0, 1), false)), (Some(1), Some(2)));
    assert_eq!(gap_at(&order, Row::Header(1), true), Some(2), "before the block's first unit");
    assert_eq!(gap_at(&order, Row::Change(0), false), Some(2), "after the block's last");
}

/// A drop runs as one undo step and selects what it moved; once its plan comes, the note says
/// where it is sewn now (and how many thread changes there are, when that changed).
#[test]
fn a_drop_and_its_note() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    let uid = app.session.active().unwrap().uid;
    let shown = app.embroidery.preview.shown(uid).unwrap();
    let (plan, generation) = (shown.planned.plan.clone(), shown.planned.generation);
    let last = NodeId(order.blocks[1].units[1].id);
    let m = place(&app.session.doc().unwrap().doc.clone(), &plan, &[last], 0).unwrap();
    let undo = app.session.doc().unwrap().history.undo.len();
    drop_rows(&mut app, &plan, generation, m);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    assert_eq!(app.session.doc().unwrap().selection.objects, [last]);
    let mut words = vec![];
    for t in [1.0, 1.05, 1.1, 1.2] {
        words = frame(&mut app, &ctx, t, vec![]);
    }
    // The blue moved first: the other blue joins it, ahead of the reds; still one thread change.
    assert!(words.iter().any(|w| w == "<Rectangle> is sewn in block 1, joined with its thread"), "{words:?}");
    assert!(!words.iter().any(|w| w.starts_with("Thread changes:")), "{words:?}");
    // Another change drops the note.
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    for t in [2.0, 2.1, 2.2] {
        words = frame(&mut app, &ctx, t, vec![]);
    }
    assert!(!words.iter().any(|w| w.contains("is sewn in block")), "{words:?}");
}

/// The tab's state is each document's: a block folded stays folded when a reorder renumbers it,
/// and another thread's block taking its number doesn't fold; another document has its own folds,
/// and a drop waiting for its plan in one is never noted in another.
#[test]
fn each_document_keeps_its_own_order_state() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let a = app.session.active().unwrap().uid;
    let order = order_of(&app);
    // Fold the second thread's block, then sew one of its objects first: its block comes first.
    app.embroidery.order.toggle_fold(a, order.blocks[1].block);
    app.run("embroidery.sewFirst", json!({"ids": [order.blocks[1].units[0].id]})).unwrap();
    for t in [1.0, 1.05, 1.1, 1.2] {
        frame(&mut app, &ctx, t, vec![]);
    }
    let after = order_of(&app);
    assert_eq!(after.blocks[0].thread.color, order.blocks[1].thread.color, "renumbered");
    let folded = app.embroidery.order.folded(a);
    assert!(folded.contains(&after.blocks[0].block) && !folded.contains(&after.blocks[1].block), "{folded:?}");
    // A drop waiting for its plan, then another document.
    let shown = app.embroidery.preview.shown(a).unwrap();
    let (plan, generation) = (shown.planned.plan.clone(), shown.planned.generation);
    let last = NodeId(after.blocks[1].units[1].id);
    let m = place(&app.session.doc().unwrap().doc.clone(), &plan, &[last], 0).unwrap();
    drop_rows(&mut app, &plan, generation, m);
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 30, "height": 30})).unwrap();
    for t in [2.0, 2.05, 2.1, 2.2] {
        frame(&mut app, &ctx, t, vec![]);
    }
    let b = app.session.active().unwrap().uid;
    assert_ne!(a, b);
    assert!(app.embroidery.order.folded(b).is_empty(), "nothing folded in the other");
    assert!(app.embroidery.order.docs.get(&b).is_none_or(|d| d.note.is_none() && d.dropped.is_none()), "no note in the other");
    assert!(app.embroidery.order.docs.get(&a).is_some_and(|d| d.dropped.is_some()), "the drop still waits for its own plan");
}

#[test]
fn a_rows_units() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    assert_eq!(units_of(&order, Row::Unit(1, 0)), [2]);
    assert_eq!(units_of(&order, Row::Header(0)), [0, 1]);
    assert!(units_of(&order, Row::Change(0)).is_empty());
}

/// The plan comes a frame before its replies, as on the planning thread: until the new plan's
/// order comes, the list shows the plan before's, dimmed, with no drag, step or hover; then the
/// new plan's, read anew (never the plan before's kept under the new plan's generation).
#[test]
fn the_list_never_takes_the_plan_befores_order_for_the_new_ones() {
    use crate::embroidery::stitch_preview::{OneStepAPoll, StitchPreview};
    let mut app = app_with(4);
    app.embroidery.preview = StitchPreview::with_runner(Box::new(OneStepAPoll::default()));
    let ctx = egui::Context::default();
    let mut t = 0.0;
    let mut settle = |app: &mut VectorcraftApp| {
        for _ in 0..8 {
            t += 0.05;
            frame(app, &ctx, t, vec![]);
        }
    };
    settle(&mut app);
    let first = order_of(&app);
    let last = first.blocks[1].units[1].id;
    // The last square moves first in the stack: the new plan's order starts with it.
    let uid = app.session.active().unwrap().uid;
    let plan = app.embroidery.preview.shown(uid).unwrap().planned.plan.clone();
    let m = place(&app.session.doc().unwrap().doc.clone(), &plan, &[NodeId(last)], 0).unwrap();
    app.run("command.batch", m.batch()).unwrap();
    // Frame by frame: once the new plan is shown and its order not yet come, the order read is the
    // plan before's, and the list is stale.
    let mut seen_stale = false;
    for _ in 0..12 {
        t += 0.05;
        frame(&mut app, &ctx, t, vec![]);
        let shown = app.embroidery.preview.shown(uid).unwrap();
        let reply = shown.reply(&query()).unwrap();
        if !reply.current {
            seen_stale = true;
            assert_eq!(order_of(&app).blocks[0].units[0].id, first.blocks[0].units[0].id, "the plan before's order, read as such");
        }
    }
    assert!(seen_stale, "the runner hands the replies over after the plan");
    let now = order_of(&app);
    assert_eq!(
        now.blocks[0].units[0].id,
        last,
        "the new plan's order, read anew: {:?}",
        now.blocks[0].units.iter().map(|u| u.id).collect::<Vec<_>>()
    );
}

/// Sew Earlier, Later, First and Last from the panel are worked out on the plan the list shows, as
/// a drop is: one undo step, the moved object selected, nothing sent to plan; off while that plan
/// is out of date (even before a frame has drawn the change), and so is the ≡ menu's.
#[test]
fn the_panels_steps_move_on_the_plan_shown() {
    use vectorcraft_engine::cmd::embroidery::reorder::Step;
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    let last = NodeId(order.blocks[1].units[1].id);
    let sent = app.embroidery.preview.log.sent.len();
    let undo = app.session.doc().unwrap().history.undo.len();
    step_selection(&mut app, Step::First, &[last]);
    assert_eq!(app.embroidery.preview.log.sent.len(), sent, "nothing sent to plan");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    assert_eq!(app.session.doc().unwrap().selection.objects, [last]);
    for t in [1.0, 1.05, 1.1, 1.2] {
        frame(&mut app, &ctx, t, vec![]);
    }
    assert_eq!(order_of(&app).blocks[0].units[0].id, last.0);
    // A change: off until its plan comes.
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    assert!(steppable(&app).is_none());
    let undo = app.session.doc().unwrap().history.undo.len();
    step_selection(&mut app, Step::Last, &[last]);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo, "nothing moved on an out-of-date plan");
}
