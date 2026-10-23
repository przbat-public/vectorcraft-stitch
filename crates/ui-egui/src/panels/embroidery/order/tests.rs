//! The Sewing order tab: its rows and their heights, collapsing, the rows laid out only in view,
//! selection both ways, and the figures under the list.

use egui::Shape;
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;

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
    app.embroidery.order.read.as_ref().map(|(_, o)| o.clone()).expect("the order read")
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
    let order = order_of(&app);
    let (list, _) = rows(&order, &HashSet::new());
    let first = order.blocks[0].units[0].id;
    select(&mut app, &order, &list, Row::Unit(0, 0), egui::Modifiers::NONE);
    assert_eq!(app.session.doc().unwrap().selection.objects, [NodeId(first)]);
    let last = order.blocks[1].units[1].id;
    select(&mut app, &order, &list, Row::Unit(1, 1), egui::Modifiers::SHIFT);
    assert_eq!(app.session.doc().unwrap().selection.objects.len(), 4, "the range");
    assert!(app.session.doc().unwrap().selection.objects.contains(&NodeId(last)));
    select(&mut app, &order, &list, Row::Header(1), egui::Modifiers::NONE);
    let block: Vec<NodeId> = order.blocks[1].units.iter().map(|u| NodeId(u.id)).collect();
    assert_eq!(app.session.doc().unwrap().selection.objects, block);
    select(&mut app, &order, &list, Row::Unit(0, 0), egui::Modifiers::COMMAND);
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
