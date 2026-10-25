//! The stitch preview's third measurement gate, run by hand on an idle machine (left out of the
//! usual test run: it times frames):
//!
//! `cargo test --release -p vectorcraft-ui-egui --lib -- --ignored --nocapture stitch_preview_gate`
//!
//! On each of the corpus's SVG designs, with the planning thread and the stitch layer's thread
//! running as in the app and frames coming at 60 Hz, in a window of 1440 × 900 points at 2 pixels a
//! point (a laptop's display) with the status bar and the Embroidery panel drawn (once with each
//! tab), it drags one object across the canvas with the Selection tool, a pointer move a frame, and
//! times each frame (the app's logic, the status bar, the panel, the canvas and the controls over
//! it, the scheduling and egui's tessellation; not the GPU's work): the 95th percentile must stay
//! within a 60 Hz frame, and the stitches must follow the drag. It reports the bytes the drag's
//! frames upload as textures. Then it nudges an object and times until the stitches drawn are of
//! the document as it is ("edit to fresh"), within 100 ms. Both budgets are the plan's.

use std::time::Instant;

use egui::{Event, PointerButton, Pos2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::plan_worker::{Driver, PlanWorker};
use super::stitch_layer::LineWorker;
use super::stitch_preview::{Freshness, StitchPreview};
use crate::{Services, VectorcraftApp};

/// A frame at 60 Hz, milliseconds: the 95th percentile of the frames while dragging stays within it.
const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;
/// From an edit to the stitches of the document as it is drawn, milliseconds.
const EDIT_TO_FRESH_BUDGET_MS: f64 = 100.0;
/// Pointer moves in a drag, and how far each goes, points.
const DRAG_FRAMES: usize = 90;
const DRAG_STEP: f32 = 2.0;
/// Bytes in a megabyte, for the uploads' report.
const BYTES_PER_MB: f64 = 1e6;
/// Edits timed per design.
const EDITS: usize = 10;
/// The window, points, and its pixels a point: a laptop's display.
const WINDOW: [f32; 2] = [1440.0, 900.0];
const PIXELS_PER_POINT: f32 = 2.0;
/// The Embroidery panel's width, points: the dock's usual width.
const PANEL_WIDTH: f32 = 300.0;

#[path = "../../../../xtask/src/sha256.rs"]
mod sha256;

/// The corpus's folder, every file checked against its manifest, as the engine's corpus tests check
/// it.
fn corpus() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/embroidery");
    for line in include_str!("../../../../xtask/embroidery-corpus.sha256").lines().filter(|l| !l.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("malformed embroidery corpus manifest");
        let bytes =
            std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("missing corpus file {name}: {e}; run `cargo xtask corpus --embroidery`"));
        assert_eq!(sha256::hex(&bytes), digest, "{name}: corpus file changed; run `cargo xtask corpus --embroidery`");
    }
    dir
}

/// One frame at `time` with `events`, as the app runs one (the status bar, the Embroidery panel on
/// the right, the canvas in the rest), tessellated → its wall time, ms, and the bytes it uploads as
/// textures.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> (f64, u64) {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(WINDOW[0], WINDOW[1]))),
        viewports: std::iter::once((
            egui::ViewportId::ROOT,
            egui::ViewportInfo { native_pixels_per_point: Some(PIXELS_PER_POINT), ..Default::default() },
        ))
        .collect(),
        events,
        ..Default::default()
    };
    let t0 = Instant::now();
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        crate::chrome::status_bar(app, ui);
        egui::Panel::right("embroidery-gate").exact_size(PANEL_WIDTH).resizable(false).show(ui, |ui| crate::panels::embroidery::show(app, ui));
        crate::canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
        super::stitch_preview::schedule(app, ui.ctx());
    });
    let shapes = std::mem::take(&mut out.shapes);
    drop(ctx.tessellate(shapes, out.pixels_per_point));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let bytes = out.textures_delta.set.values().flatten().map(|d| (d.image.width() * d.image.height() * d.image.bytes_per_pixel()) as u64).sum();
    out.textures_delta.clear();
    (ms, bytes)
}

/// The value below which `p` of `samples` lie.
fn percentile(mut samples: Vec<f64>, p: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    let i = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples.get(i).copied().unwrap_or(f64::NAN)
}

/// Whether the stitches drawn are of the active document as it is.
fn drawn_fresh(app: &VectorcraftApp, time: f64) -> bool {
    let uid = app.session.active().map(|st| st.uid).unwrap_or_default();
    let preview = &app.embroidery.preview;
    preview.freshness(uid, time) == Freshness::Fresh
        && !app.embroidery.layer.waiting()
        && preview.shown(uid).map(|s| s.planned.generation) == app.embroidery.layer.generation()
}

#[test]
#[ignore = "a measurement, run by hand on an idle machine"]
fn stitch_preview_gate() {
    let dir = corpus();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "svg")).collect())
        .unwrap_or_default();
    files.sort();
    assert!(!files.is_empty(), "no corpus at {}", dir.display());
    let (mut worst_p95, mut worst_fresh) = (0.0_f64, 0.0_f64);
    println!("{:<52} {:<6} {:>9} {:>9} {:>12} {:>11} {:>11}", "design", "tab", "drag p50", "drag p95", "upload/frame", "fresh p50", "fresh max");
    for (path, tab) in files.iter().flat_map(|p| crate::panels::embroidery::Tab::ALL.into_iter().map(move |t| (p, t))) {
        let ctx = egui::Context::default();
        // The app's fonts: the status bar's readouts draw in its monospaced family.
        crate::theme::install_fonts(&ctx);
        let mut session = Session::new();
        session.execute("document.open", &json!({"path": path.to_string_lossy()})).unwrap();
        let mut app = VectorcraftApp::new(session, Services::default());
        app.ui.embroidery_panel.tab = tab;
        let plans = PlanWorker::start(&ctx).expect("the planning thread starts");
        let lines = LineWorker::start(&ctx).expect("the stitch layer's thread starts");
        app.embroidery.preview = StitchPreview::with_runner(Box::new(Driver::Thread(plans)));
        app.embroidery.layer.use_worker(lines);
        app.run("view.stitchPreview", json!({"on": true})).unwrap();
        let mut time = 0.0;
        // Frames come at the display's pace, as in the app (a frame drawn sooner would leave the
        // threads no time to answer): each waits out the rest of its 60 Hz slot. The pace of a
        // measurement, not a wait for a result.
        let mut next = Instant::now();
        let mut tick = |app: &mut VectorcraftApp, events: Vec<Event>| {
            time += 1.0 / 60.0;
            next += std::time::Duration::from_secs_f64(FRAME_BUDGET_MS / 1000.0);
            let (ms, bytes) = frame(app, &ctx, time, events);
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
            (ms, bytes, time)
        };
        // Settle: the first plan and its frame.
        let t0 = Instant::now();
        loop {
            let (_, _, now) = tick(&mut app, vec![]);
            if drawn_fresh(&app, now) {
                break;
            }
            assert!(t0.elapsed().as_secs() < 30, "{}: never drawn", path.display());
        }
        // Drag the first object sewn from its middle, a pointer move a frame.
        let uid = app.session.active().unwrap().uid;
        let unit = app.embroidery.preview.shown(uid).and_then(|s| s.planned.scene().units().first().and_then(|u| u.bounds));
        let (Some(b), Some(canvas)) = (unit, app.canvas_rect) else { panic!("{}: no stitches to drag, or no canvas", path.display()) };
        let view = *app.view().unwrap();
        let xf = crate::canvas::Xf::new(canvas, &view);
        let mut at = xf.to_screen(b.center());
        let (mut drag, mut uploaded) = (vec![], 0);
        let (revision, taken) = (app.session.active().unwrap().revision, app.embroidery.preview.log.taken.len());
        let mut drawn = std::collections::HashSet::new();
        tick(&mut app, vec![Event::PointerMoved(at)]);
        tick(&mut app, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        for _ in 0..DRAG_FRAMES {
            at += egui::vec2(DRAG_STEP, DRAG_STEP / 2.0);
            let (ms, bytes, _) = tick(&mut app, vec![Event::PointerMoved(at)]);
            drag.push(ms);
            uploaded += bytes;
            drawn.extend(app.embroidery.layer.generation());
        }
        // The drag moved the art, and the stitches followed it, plan after plan.
        assert!(app.session.active().unwrap().revision > revision, "{}: nothing was dragged", path.display());
        let (plans, frames) = (app.embroidery.preview.log.taken.len() - taken, drawn.len());
        assert!(plans > 1 && frames > 1, "{}: the stitches didn't follow the drag ({plans} plans, {frames} frames)", path.display());
        tick(&mut app, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        // Edit to fresh: a nudge, then frames as fast as they come until the stitches drawn are of
        // the document as it is.
        let mut fresh = vec![];
        for k in 0..EDITS {
            let t0 = Instant::now();
            loop {
                let (_, _, now) = tick(&mut app, vec![]);
                if drawn_fresh(&app, now) {
                    break;
                }
                assert!(t0.elapsed().as_secs() < 30, "{}: never fresh", path.display());
            }
            let dx = if k % 2 == 0 { 1.0 } else { -1.0 };
            app.run("object.move", json!({"dx": dx, "dy": 0})).unwrap();
            let t0 = Instant::now();
            loop {
                let (_, _, now) = tick(&mut app, vec![]);
                if drawn_fresh(&app, now) {
                    break;
                }
                assert!(t0.elapsed().as_secs() < 30, "{}: never fresh", path.display());
            }
            fresh.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let (p50, p95) = (percentile(drag.clone(), 0.5), percentile(drag, 0.95));
        let (f50, fmax) = (percentile(fresh.clone(), 0.5), percentile(fresh, 1.0));
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let per_frame = uploaded as f64 / DRAG_FRAMES as f64 / BYTES_PER_MB;
        println!(
            "{name:<52} {:<6} {p50:>6.2} ms {p95:>6.2} ms {per_frame:>9.1} MB {f50:>8.2} ms {fmax:>8.2} ms   {plans} plans, {frames} frames drawn while dragging",
            tab.id()
        );
        worst_p95 = worst_p95.max(p95);
        worst_fresh = worst_fresh.max(fmax);
    }
    println!("worst: drag p95 {worst_p95:.2} ms (budget {FRAME_BUDGET_MS:.1}), edit to fresh {worst_fresh:.2} ms (budget {EDIT_TO_FRESH_BUDGET_MS})");
    assert!(worst_p95 <= FRAME_BUDGET_MS && worst_fresh <= EDIT_TO_FRESH_BUDGET_MS);
}
