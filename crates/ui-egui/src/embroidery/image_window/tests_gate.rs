//! The image window's measurement gate, run by hand on an idle machine (left out of the usual test
//! run: it times frames and runs):
//!
//! `cargo test --release -p vectorcraft-ui-egui --lib -- --ignored --nocapture image_window_gate`
//!
//! On each design of the embroidery corpus drawn as a picture at 10 pixels a millimetre, and on a
//! logo 150 mm wide drawn here, the picture placed and selected, with the window's thread running as
//! in the app and frames at the display's pace in a window of 1440 × 900 points at 2 pixels a point:
//!
//! - Digitize Image… opened to its first whole result (colour mode): within 500 ms;
//! - a step of the number of colours (one fewer) to its result: within 250 ms;
//! - the smallest area's slider dragged along its range, a value a frame: the 95th percentile of the
//!   frames within a 60 Hz frame;
//! - logo mode to its result: within 1,000 ms;
//! - Compare's split dragged for 90 frames: nothing uploaded.
//!
//! The budgets are the plan's; the table of what was measured is printed.

use std::time::{Duration, Instant};

use egui::{Event, Pos2};
use serde_json::{Value, json};
use vectorcraft_engine::Session;

use super::worker::{Driver, Worker};
use super::{STEP, VIEW, params_of, preview};
use crate::embroidery::tests_gate::{FRAME_BUDGET_MS, corpus, percentile};
use crate::{Services, VectorcraftApp};

/// The budgets, milliseconds.
const OPEN_BUDGET_MS: f64 = 500.0;
const COLOURS_STEP_BUDGET_MS: f64 = 250.0;
const LOGO_MODE_BUDGET_MS: f64 = 1000.0;
/// The window, points, and its pixels a point: a laptop's display.
const WINDOW: [f32; 2] = [1440.0, 900.0];
const PIXELS_PER_POINT: f32 = 2.0;
/// The pictures' resolution, pixels a millimetre (and in pixels an inch, as the export takes it).
const PX_PER_MM: f64 = 10.0;
const MM_PER_INCH: f64 = 25.4;
const PT_PER_MM: f64 = 72.0 / MM_PER_INCH;
/// Room round a design's art in its picture, millimetres.
const MARGIN_MM: f64 = 5.0;
/// The drawn logo's width, millimetres.
const LOGO_MM: f64 = 150.0;
/// Frames the slider is dragged for, and the split.
const DRAG_FRAMES: usize = 90;
/// How long a run may take before the gate gives up on it, seconds: far past every budget.
const GIVE_UP_SECS: u64 = 60;
/// The smallest area's slider's ends, square millimetres: the engine's range, past 0 (the slider is
/// logarithmic).
const MIN_AREA_FROM: f64 = 0.1;
const MIN_AREA_TO: f64 = 50.0;

/// The design at `path` drawn as a PNG at [`PX_PER_MM`] over its art and a margin → the PNG and
/// its box, points.
fn picture_of(path: &std::path::Path) -> (Vec<u8>, [f64; 4]) {
    let mut s = Session::new();
    s.execute("document.open", &json!({"path": path.to_string_lossy()})).unwrap();
    picture_of_session(&mut s)
}

/// The art of session `s`'s document drawn as a PNG at [`PX_PER_MM`] over its art and a margin.
fn picture_of_session(s: &mut Session) -> (Vec<u8>, [f64; 4]) {
    s.execute("select.all", &json!({})).unwrap();
    let b = s.execute("document.exportSelection", &json!({"format": "svg"})).unwrap()["bounds"].clone();
    s.execute("select.none", &json!({})).unwrap();
    let pt = |i: usize| b[i].as_f64().unwrap();
    let m = MARGIN_MM * PT_PER_MM;
    let rect = [pt(0) - m, pt(1) - m, pt(2) + 2.0 * m, pt(3) + 2.0 * m];
    let png = s.execute("document.export", &json!({"format": "png", "ppi": PX_PER_MM * MM_PER_INCH, "rect": rect})).unwrap();
    (vectorcraft_format::base64_decode(png["dataBase64"].as_str().unwrap()).unwrap(), rect)
}

/// A logo [`LOGO_MM`] wide drawn here: a ring, a disc, bars and dots in six colours.
fn logo() -> (Vec<u8>, [f64; 4]) {
    let mut s = Session::new();
    let side = (LOGO_MM + 2.0 * MARGIN_MM) * PT_PER_MM;
    s.execute("file.new", &json!({"width": side, "height": side, "artboards": 1})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    let mm = |v: f64| (MARGIN_MM + v) * PT_PER_MM;
    let shapes = [
        ("shape.ellipse", [0.0, 0.0, 150.0, 150.0], "#1d3557"),
        ("shape.ellipse", [12.0, 12.0, 126.0, 126.0], "#f1faee"),
        ("shape.ellipse", [40.0, 40.0, 70.0, 70.0], "#e63946"),
        ("shape.rectangle", [20.0, 70.0, 110.0, 10.0], "#457b9d"),
        ("shape.rectangle", [70.0, 20.0, 10.0, 110.0], "#a8dadc"),
        ("shape.ellipse", [30.0, 30.0, 12.0, 12.0], "#ffb703"),
        ("shape.ellipse", [108.0, 30.0, 12.0, 12.0], "#ffb703"),
        ("shape.ellipse", [30.0, 108.0, 12.0, 12.0], "#ffb703"),
        ("shape.ellipse", [108.0, 108.0, 12.0, 12.0], "#ffb703"),
    ];
    for (cmd, [x, y, w, h], color) in shapes {
        s.execute("select.none", &json!({})).unwrap();
        s.execute("paint.setFill", &json!({"color": color})).unwrap();
        s.execute(cmd, &json!({"x": mm(x), "y": mm(y), "width": w * PT_PER_MM, "height": h * PT_PER_MM})).unwrap();
    }
    picture_of_session(&mut s)
}

/// An app holding PNG `png` (named `name`) placed over `rect`, selected, its window's thread
/// running as in the app once the window opens.
fn app_with(name: &str, png: &[u8], rect: [f64; 4]) -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    let side = rect[0].max(0.0) + rect[2] + rect[1].max(0.0) + rect[3];
    app.run("file.new", json!({"width": side, "height": side, "artboards": 1})).unwrap();
    app.run("file.place", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(png), "rect": rect})).unwrap();
    app
}

/// One frame at `time` with `events`, as the app runs the window over it, tessellated → its wall
/// time, ms, and the bytes it uploads as textures.
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
        crate::dialogs::show(app, ui.ctx());
    });
    let shapes = std::mem::take(&mut out.shapes);
    drop(ctx.tessellate(shapes, out.pixels_per_point));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let bytes = out.textures_delta.set.values().flatten().map(|d| (d.image.width() * d.image.height() * d.image.bytes_per_pixel()) as u64).sum();
    out.textures_delta.clear();
    (ms, bytes)
}

/// Frames at the display's pace: each waits out the rest of its 60 Hz slot (the pace of a
/// measurement, not a wait for a result).
struct Pace {
    start: Instant,
    next: Instant,
}

impl Pace {
    fn new() -> Self {
        let now = Instant::now();
        Self { start: now, next: now }
    }

    fn tick(&mut self, app: &mut VectorcraftApp, ctx: &egui::Context) -> (f64, u64) {
        self.next += Duration::from_secs_f64(FRAME_BUDGET_MS / 1000.0);
        let time = self.start.elapsed().as_secs_f64();
        let out = frame(app, ctx, time, vec![]);
        std::thread::sleep(self.next.saturating_duration_since(Instant::now()));
        out
    }
}

/// Whether the window shows a whole result of its settings as they are.
fn fresh(app: &VectorcraftApp) -> bool {
    let (Some(live), Some(d)) = (app.embroidery.image_window.live.as_ref(), app.ui.dialog.as_ref()) else { return false };
    live.schedule.current(&params_of(&d.fields)).is_some()
}

/// Frames until the window is fresh → milliseconds since `since`.
fn until_fresh(app: &mut VectorcraftApp, ctx: &egui::Context, pace: &mut Pace, since: Instant, what: &str) -> f64 {
    loop {
        pace.tick(app, ctx);
        if fresh(app) {
            return since.elapsed().as_secs_f64() * 1000.0;
        }
        assert!(since.elapsed().as_secs() < GIVE_UP_SECS, "{what}: never fresh");
    }
}

/// Set field `key` of the open window to `value`.
fn set(app: &mut VectorcraftApp, key: &str, value: Value) {
    app.ui.dialog.as_mut().expect("the window is open").fields.insert(key.into(), value);
}

/// What the gate measured on one picture, milliseconds (the uploads in bytes).
struct Measured {
    name: String,
    open: f64,
    colours: f64,
    slider_p95: f64,
    logo: f64,
    compare_bytes: u64,
}

/// The gate on picture `png` named `name` over `rect`.
fn measure(name: &str, png: &[u8], rect: [f64; 4]) -> Measured {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut app = app_with(name, png, rect);
    let mut pace = Pace::new();
    // Opened: the window's thread put in before its first run, as the app starts it.
    let t0 = Instant::now();
    crate::menus::invoke(&mut app, "embroidery.digitizeImage", json!({}));
    pace.tick(&mut app, &ctx);
    if let Some(live) = app.embroidery.image_window.live.as_mut() {
        let pipeline = app.session.digitize_pipeline();
        let worker = Worker::start(&ctx, pipeline).unwrap_or_else(|_| panic!("the window's thread starts"));
        live.runner = Box::new(Driver::Thread(worker));
        live.schedule = super::schedule::Schedule::default();
    }
    let open = until_fresh(&mut app, &ctx, &mut pace, t0, name);
    // A step of the number of colours.
    let colours_now =
        app.embroidery.image_window.live.as_mut().and_then(|l| l.reply().cloned()).and_then(|r| r["settings"]["colors"].as_u64()).unwrap_or(6);
    set(&mut app, "colors", json!(colours_now.saturating_sub(1).max(2)));
    let colours = until_fresh(&mut app, &ctx, &mut pace, Instant::now(), name);
    // The smallest area's slider dragged along its range, a value a frame.
    let mut frames = Vec::with_capacity(DRAG_FRAMES);
    for k in 0..DRAG_FRAMES {
        let along = k as f64 / (DRAG_FRAMES - 1) as f64;
        set(&mut app, "minArea", json!(MIN_AREA_FROM * (MIN_AREA_TO / MIN_AREA_FROM).powf(along)));
        frames.push(pace.tick(&mut app, &ctx).0);
    }
    let slider_p95 = percentile(frames, 0.95);
    until_fresh(&mut app, &ctx, &mut pace, Instant::now(), name);
    // Logo mode.
    set(&mut app, "mode", json!("logo"));
    let logo = until_fresh(&mut app, &ctx, &mut pace, Instant::now(), name);
    // Compare at the stitches, settled, then its split dragged.
    set(&mut app, VIEW, json!("compare"));
    set(&mut app, STEP, json!(4));
    for _ in 0..10 {
        pace.tick(&mut app, &ctx);
    }
    let mut compare_bytes = 0;
    for k in 0..DRAG_FRAMES {
        set(&mut app, preview::SPLIT, json!(k as f64 / DRAG_FRAMES as f64));
        compare_bytes += pace.tick(&mut app, &ctx).1;
    }
    Measured { name: name.to_string(), open, colours, slider_p95, logo, compare_bytes }
}

#[test]
#[ignore = "a measurement, run by hand on an idle machine"]
fn image_window_gate() {
    let dir = corpus();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "svg")).collect())
        .unwrap_or_default();
    files.sort();
    assert!(!files.is_empty(), "no corpus at {}", dir.display());
    let mut pictures: Vec<(String, Vec<u8>, [f64; 4])> = files
        .iter()
        .map(|p| {
            let (png, rect) = picture_of(p);
            (p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), png, rect)
        })
        .collect();
    let (png, rect) = logo();
    pictures.push((format!("drawn logo, {LOGO_MM} mm"), png, rect));
    println!("{:<44} {:>9} {:>11} {:>12} {:>9} {:>14}", "picture (10 px/mm)", "open ms", "colours ms", "slider p95", "logo ms", "compare bytes");
    let mut failed = Vec::new();
    for (name, png, rect) in &pictures {
        let m = measure(name, png, *rect);
        println!("{:<44} {:>9.0} {:>11.0} {:>12.1} {:>9.0} {:>14}", m.name, m.open, m.colours, m.slider_p95, m.logo, m.compare_bytes);
        let over = [
            (m.open > OPEN_BUDGET_MS, "open"),
            (m.colours > COLOURS_STEP_BUDGET_MS, "colours step"),
            (m.slider_p95 > FRAME_BUDGET_MS, "slider frames"),
            (m.logo > LOGO_MODE_BUDGET_MS, "logo mode"),
            (m.compare_bytes > 0, "compare uploads"),
        ];
        failed.extend(over.into_iter().filter(|(o, _)| *o).map(|(_, what)| format!("{}: {what}", m.name)));
    }
    assert!(failed.is_empty(), "over budget: {failed:?}");
}
