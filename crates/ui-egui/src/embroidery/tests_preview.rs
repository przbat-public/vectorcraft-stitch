//! The stitch preview end to end, frame by frame through the app: after random edits at random
//! times the plan shown is the final document's, on the planning thread and in line; no plan taken
//! in was of a cancelled job; no state is planned twice in a row, nor more than once per edit;
//! nothing is planned while nothing shows the stitches.

use serde_json::{Value, json};
use vectorcraft_engine::{EmbroideryOptions, Session};

use super::plan_worker::{Driver, PlanWorker};
use super::stitch_preview::{Freshness, StitchPreview};
use crate::{Services, VectorcraftApp};

/// How long a test waits for the planning thread, wall clock: far longer than these plans take.
const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Edits made in a run.
const EDITS: usize = 40;

/// A small deterministic generator (the tests need no randomness crate): Knuth's MMIX constants.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n.max(1)
    }
}

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for (color, x) in [("#2a6fdb", 20), ("#d01818", 110), ("#20a030", 200)] {
        app.run("paint.setFill", json!({"color": color})).unwrap();
        app.run("shape.rectangle", json!({"x": x, "y": 40, "width": 60, "height": 60})).unwrap();
    }
    app
}

/// One headless frame at `time`, as the app runs one.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        super::stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
}

/// One random edit: a move, a new square, a delete, an undo, a new colour.
fn edit(app: &mut VectorcraftApp, rng: &mut Lcg) {
    let ids: Vec<u64> = app.session.doc().unwrap().doc.layers[0].children().map(|c| c.iter().map(|n| n.id.0).collect()).unwrap_or_default();
    let pick = ids.get(rng.next(ids.len() as u64) as usize).copied();
    let _: Option<Value> = match (rng.next(5), pick) {
        (0, Some(id)) => {
            app.run("select.set", json!({"ids": [id]})).ok();
            app.run("object.move", json!({"dx": rng.next(9) as f64 - 4.0, "dy": rng.next(9) as f64 - 4.0})).ok()
        }
        (1, _) => {
            let (x, y) = (rng.next(240), rng.next(240));
            app.run("shape.rectangle", json!({"x": x, "y": y, "width": 20 + rng.next(30), "height": 20 + rng.next(30)})).ok()
        }
        (2, Some(id)) if ids.len() > 1 => {
            app.run("select.set", json!({"ids": [id]})).ok();
            app.run("edit.clear", json!({})).ok()
        }
        (3, _) => app.run("edit.undo", json!({})).ok(),
        (_, Some(id)) => {
            app.run("select.set", json!({"ids": [id]})).ok();
            let color = ["#2a6fdb", "#d01818", "#f0e020"][rng.next(3) as usize];
            app.run("paint.setFill", json!({ "color": color })).ok()
        }
        _ => None,
    };
}

/// A design planned on every change, and one that waits for the document to rest, whatever the
/// machine measures (milliseconds a plan takes, kept in place of the time measured).
const LIGHT_MS: f64 = 1.0;
const HEAVY_MS: f64 = 1000.0;

/// Random edits at random times with the stitch view on, its design pinned light or heavy
/// (`plan_ms`), then frames until the plan shown is fresh: it is the final document's, and equals
/// a fresh plan of it.
fn random_edits(threaded: bool, seed: u64, plan_ms: f64) {
    let ctx = egui::Context::default();
    let mut app = app();
    app.embroidery.preview.log.pinned_ms = Some(plan_ms);
    if threaded {
        let Some(worker) = PlanWorker::start(&ctx) else { return };
        app.embroidery.preview = StitchPreview::with_runner(Box::new(Driver::Thread(worker)));
    }
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let mut rng = Lcg(seed);
    let mut time = 0.0;
    for _ in 0..EDITS {
        edit(&mut app, &mut rng);
        time += 0.01 + rng.next(200) as f64 / 1000.0;
        frame(&mut app, &ctx, time);
    }
    let uid = app.session.active().unwrap().uid;
    let t0 = std::time::Instant::now();
    while app.embroidery.preview.freshness(uid, time) != Freshness::Fresh {
        assert!(t0.elapsed() < WAIT, "never fresh: {:?}", app.embroidery.preview.freshness(uid, time));
        time += 0.05;
        frame(&mut app, &ctx, time);
        std::thread::yield_now();
    }
    let shown = app.embroidery.preview.shown(uid).unwrap();
    assert_eq!(Some(shown.key.clone()), app.session.embroidery_plan_key(&EmbroideryOptions::default()), "the final document's");
    // A new session plans the document from scratch.
    let mut fresh = Session::new();
    fresh.add_document(app.session.doc().unwrap().doc.as_ref().clone(), None);
    assert_eq!(*shown.planned.plan, *fresh.embroidery_plan(&EmbroideryOptions::default()).unwrap(), "equal to a fresh plan of it");
    let log = &app.embroidery.preview.log;
    // Every plan taken in was the job's under way when it came, and of no job cancelled before.
    let cancelled_before = |k: usize, ticket: u64| log.cancelled.iter().any(|(t, at)| *t == ticket && *at <= k);
    let wrong: Vec<&(u64, u64, bool)> = log
        .taken
        .iter()
        .enumerate()
        .filter(|(k, (done, flying, cancelled))| done != flying || *cancelled || cancelled_before(*k, *done))
        .map(|(_, t)| t)
        .collect();
    assert!(wrong.is_empty(), "plans taken in for another job, or a cancelled one: {wrong:?}");
    let planned: Vec<(u64, &vectorcraft_engine::EmbroideryPlanKey)> =
        log.sent.iter().filter(|(_, _, plans)| *plans).map(|(t, k, _)| (*t, k)).collect();
    // A state is planned again only after another (an undo back to it: the plan remembered), or
    // after its own job was cancelled.
    assert!(planned.windows(2).all(|w| w[0].1 != w[1].1 || log.cancelled.iter().any(|(t, _)| *t == w[0].0)), "one state planned twice in a row");
    assert!(planned.len() <= EDITS + 1, "at most one plan per edit: {}", planned.len());
}

#[test]
fn after_random_edits_the_plan_shown_is_the_final_documents_in_line() {
    for plan_ms in [LIGHT_MS, HEAVY_MS] {
        for seed in [1, 7, 42] {
            random_edits(false, seed, plan_ms);
        }
    }
}

#[test]
fn after_random_edits_the_plan_shown_is_the_final_documents_on_the_thread() {
    for plan_ms in [LIGHT_MS, HEAVY_MS] {
        for seed in [3, 11] {
            random_edits(true, seed, plan_ms);
        }
    }
}

/// With the stitch view off, edits plan nothing.
#[test]
fn nothing_is_planned_while_nothing_shows_the_stitches() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut rng = Lcg(5);
    for k in 0..10 {
        edit(&mut app, &mut rng);
        frame(&mut app, &ctx, f64::from(k) * 0.2);
    }
    assert!(app.embroidery.preview.log.sent.is_empty());
    let uid = app.session.active().unwrap().uid;
    assert!(app.embroidery.preview.shown(uid).is_none());
}
