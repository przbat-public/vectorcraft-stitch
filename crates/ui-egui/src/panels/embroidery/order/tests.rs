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
    assert!(app.embroidery.order.collapsed.contains(&block), "← folds it");
    step(&mut app, vec![key(egui::Key::ArrowRight)]);
    assert!(!app.embroidery.order.collapsed.contains(&block), "→ unfolds it");
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

#[test]
fn a_header_collapses() {
    let mut app = app_with(4);
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let order = order_of(&app);
    let block = order.blocks[0].block;
    app.embroidery.order.collapsed.insert(block);
    let words = frame(&mut app, &ctx, 1.0, vec![]);
    assert_eq!(words.iter().filter(|w| w.starts_with("<Rectangle>")).count(), 2, "{words:?}");
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
