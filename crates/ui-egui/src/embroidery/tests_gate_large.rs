//! The canvas while one point of a design of 2 million is dragged (VectorCraft Stitch), a
//! measurement run by hand on an idle machine (left out of the usual test run: it times frames):
//!
//! `cargo test --release -p vectorcraft-ui-egui --lib -- --ignored --nocapture large_design_drag_gate`
//!
//! On the stitch-editing gate's design of 2 million needle points
//! ([`two_million_needle_points`]), drawn as art, with the planning threads running and frames
//! coming at 60 Hz as in the other gates (the status bar, the Embroidery panel and the canvas drawn
//! by [`frame`], 1440 × 900 points at 2 pixels a point):
//! - With Direct Selection, the anchor nearest the middle of the middle path clicked, then dragged
//!   a pointer move a frame for [`DRAG_FRAMES`] frames, each frame timed (the app's logic, the
//!   panel, the canvas and the controls over it, egui's tessellation; not the GPU's work): the 95th
//!   percentile within a 60 Hz frame. Every frame of the drag is a revision of the document, which
//!   the canvas measures for its scroll bars and the selection's frame: each frame must make one,
//!   and measure no more than the objects on the way up to the anchor's path
//!   ([`OBJECTS_ON_THE_WAY_UP`]), a count that holds on any machine.
//! - Then everything selected (Select All) and dragged with the Selection tool, the case the
//!   cache doesn't help: every object changes each frame. Its frames are timed and printed, with
//!   the objects measured a frame, and given no budget.

use std::sync::Arc;
use std::time::Instant;

use egui::{Event, PointerButton, Pos2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::plan_worker::{Driver, PlanWorker};
use super::stitch_layer::LineWorker;
use super::stitch_preview::StitchPreview;
use super::tests_gate::{
    BIG_RUN_POINTS, BIG_RUNS, BIG_THREADS, DRAG_FRAMES, DRAG_STEP, FRAME_BUDGET_MS, frame, percentile, two_million_needle_points,
};
use crate::{Services, VectorcraftApp};

/// What one anchor's edit replaces in the design, and so the most objects a drag frame measures: the
/// path, its thread's group and the layer.
const OBJECTS_ON_THE_WAY_UP: u64 = 3;
/// The longest the gate waits for the design to be drawn and planned before it drags, seconds:
/// far past what the design takes. An estimate.
const SETTLE_SECS: u64 = 60;
/// How far a Select All drag moves the design a frame, points.
const SELECT_ALL_STEP: f32 = 1.0;
/// The most pointer moves before a press becomes a drag: egui takes a press for a drag only once
/// it has moved past its click distance (6 points; a move here goes 2.2). An estimate, with room.
const DRAG_START_MOVES: usize = 10;

/// One frame of the gate with its events → its wall time, ms, and the time it was drawn at.
type Tick<'a> = dyn FnMut(&mut VectorcraftApp, Vec<Event>) -> (f64, f64) + 'a;

#[test]
#[ignore = "a measurement, run by hand on an idle machine"]
fn large_design_drag_gate() {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut session = Session::new();
    session.add_document(two_million_needle_points(), None);
    let mut app = VectorcraftApp::new(session, Services::default());
    let plans = PlanWorker::start(&ctx).expect("the planning thread starts");
    let lines = LineWorker::start(&ctx).expect("the stitch layer's thread starts");
    app.embroidery.preview = StitchPreview::with_runner(Box::new(Driver::Thread(plans)));
    app.embroidery.layer.use_worker(lines);
    app.run("tool.select", json!({"tool": "directSelection"})).unwrap();
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    let mut time = 0.0;
    // Frames come at the display's pace, as in the app: each waits out the rest of its 60 Hz slot.
    let mut next = Instant::now();
    let mut tick = |app: &mut VectorcraftApp, events: Vec<Event>| {
        time += FRAME_BUDGET_MS / 1000.0;
        next += std::time::Duration::from_secs_f64(FRAME_BUDGET_MS / 1000.0);
        let (ms, _) = frame(app, &ctx, time, events);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        (ms, time)
    };
    // Settle: the art drawn for the document as it is, and the status bar's figures planned.
    let settle = |app: &mut VectorcraftApp, tick: &mut Tick<'_>| {
        let t0 = Instant::now();
        loop {
            let (_, now) = tick(app, vec![]);
            let st = app.session.active().unwrap();
            let drawn = app.canvas.shown.as_ref().is_some_and(|(d, _)| Arc::ptr_eq(d, &st.doc));
            if drawn && !app.embroidery.preview.busy(st.uid, now) {
                return;
            }
            assert!(t0.elapsed().as_secs() < SETTLE_SECS, "never drawn and planned");
        }
    };
    settle(&mut app, &mut tick);
    let uid = app.session.active().unwrap().uid;
    // A click on the anchor nearest the middle of the middle path of the middle group. At fit the
    // rows lie under a point apart: it selects the anchor nearest it, of whichever path that is.
    let doc = app.session.active().unwrap().doc.clone();
    let group = doc.layers[0].children().unwrap()[BIG_THREADS / 2].clone();
    let middle = group.children().unwrap()[BIG_RUNS / BIG_THREADS / 2].path_data().unwrap().subpaths[0].anchors[BIG_RUN_POINTS / 2].p;
    let xf = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let at = xf.to_screen(middle);
    tick(&mut app, vec![Event::PointerMoved(at), button(at, true)]);
    tick(&mut app, vec![button(at, false)]);
    settle(&mut app, &mut tick);
    let selected = &app.session.active().unwrap().selection;
    let picked: Vec<_> = selected.anchors.iter().flat_map(|(id, a)| a.iter().map(move |k| (*id, *k))).collect();
    let [(path, (subpath, k))] = picked[..] else { panic!("the click selected {} anchors, not one", picked.len()) };
    let anchor_of = |app: &VectorcraftApp| app.session.active().unwrap().doc.node(path).unwrap().path_data().unwrap().subpaths[subpath].anchors[k].p;
    let anchor = anchor_of(&app);
    tick(&mut app, vec![button(at, true)]);
    // The moves until the drag's first revision start it, untimed (egui's click distance).
    let (start, mut to) = (app.session.active().unwrap().revision, at);
    for _ in 0..DRAG_START_MOVES {
        to += egui::vec2(DRAG_STEP, DRAG_STEP / 2.0);
        tick(&mut app, vec![Event::PointerMoved(to)]);
        if app.session.active().unwrap().revision > start {
            break;
        }
    }
    assert!(app.session.active().unwrap().revision > start, "the press never became a drag");
    let mut drag = vec![];
    for k in 0..DRAG_FRAMES {
        let (revision, measured) = (app.session.active().unwrap().revision, app.embroidery.bounds.measured(uid));
        to += egui::vec2(DRAG_STEP, DRAG_STEP / 2.0);
        drag.push(tick(&mut app, vec![Event::PointerMoved(to)]).0);
        assert!(app.session.active().unwrap().revision > revision, "frame {k} of the drag made no revision");
        let more = app.embroidery.bounds.measured(uid) - measured;
        assert!(more <= OBJECTS_ON_THE_WAY_UP, "frame {k} of the drag measured {more} objects");
    }
    tick(&mut app, vec![button(to, false)]);
    settle(&mut app, &mut tick);
    assert_ne!(anchor_of(&app), anchor, "the drag moved the anchor");
    let (p50, p95, max) = (percentile(drag.clone(), 0.5), percentile(drag.clone(), 0.95), percentile(drag, 1.0));
    println!(
        "{} anchors, one dragged with Direct Selection: a revision each of {DRAG_FRAMES} frames, at most {OBJECTS_ON_THE_WAY_UP} objects measured each; p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms (budget {FRAME_BUDGET_MS:.1})",
        BIG_RUNS * BIG_RUN_POINTS
    );
    // Everything selected and dragged: every object changes each frame. At fit the strokes of
    // neighbouring rows touch, so a press where the anchor was lands on art.
    let corner = |app: &VectorcraftApp| {
        app.session.active().unwrap().doc.layers[0].children().unwrap()[0].children().unwrap()[0].path_data().unwrap().subpaths[0].anchors[0].p
    };
    let before = corner(&app);
    app.run("select.all", json!({})).unwrap();
    app.run("tool.select", json!({"tool": "selection"})).unwrap();
    settle(&mut app, &mut tick);
    let (mut all, mut to) = (vec![], at);
    tick(&mut app, vec![Event::PointerMoved(at), button(at, true)]);
    let start = app.session.active().unwrap().revision;
    for _ in 0..DRAG_START_MOVES {
        to += egui::vec2(SELECT_ALL_STEP, 0.0);
        tick(&mut app, vec![Event::PointerMoved(to)]);
        if app.session.active().unwrap().revision > start {
            break;
        }
    }
    let measured = app.embroidery.bounds.measured(uid);
    for _ in 0..DRAG_FRAMES {
        to += egui::vec2(SELECT_ALL_STEP, 0.0);
        all.push(tick(&mut app, vec![Event::PointerMoved(to)]).0);
    }
    let per_frame = (app.embroidery.bounds.measured(uid) - measured) / DRAG_FRAMES as u64;
    tick(&mut app, vec![button(to, false)]);
    settle(&mut app, &mut tick);
    assert_ne!(corner(&app), before, "the Select All drag moved the design");
    println!(
        "everything selected and dragged with the Selection tool: about {per_frame} objects measured a frame; p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        percentile(all.clone(), 0.5),
        percentile(all.clone(), 0.95),
        percentile(all, 1.0)
    );
    assert!(p95 <= FRAME_BUDGET_MS, "one anchor's drag: a frame p95 {p95:.2} ms");
}
