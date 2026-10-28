//! The canvas while one point of a design of 2 million is dragged (VectorCraft Stitch), a
//! measurement run by hand on an idle machine (left out of the usual test run: it times frames):
//!
//! `cargo test --release -p vectorcraft-ui-egui --lib -- --ignored --nocapture large_design_drag_gate`
//!
//! On a design shaped as a machine file of 2 million needle points opens ([`two_million_points`]),
//! drawn as art, with Direct Selection: an anchor in the middle of a path clicked, then dragged a
//! pointer move a frame for [`DRAG_FRAMES`] frames, at 60 Hz in a window of 1440 × 900 points at 2
//! pixels a point (a laptop's display), each frame timed (the app's logic, the canvas and the
//! controls over it, egui's tessellation; not the GPU's work): the 95th percentile must stay within
//! a 60 Hz frame. Every frame of the drag is a revision of the document, which the canvas measures
//! for its scroll bars and the selection's frame; measuring every path's stroke again took about 46
//! ms a frame before those bounds were measured only where an edit changed the document
//! (`vectorcraft_doc::visual_bounds`).

use std::time::Instant;

use egui::{Event, PointerButton, Pos2};
use serde_json::json;
use vectorcraft_engine::Session;

use crate::{Services, VectorcraftApp};

/// A frame at 60 Hz, milliseconds: the 95th percentile of the frames while dragging stays within it.
const FRAME_BUDGET_MS: f64 = 1000.0 / 60.0;
/// Pointer moves in the drag, and how far each goes, points.
const DRAG_FRAMES: usize = 90;
const DRAG_STEP: f32 = 2.0;
/// Frames drawn before the drag, for the first render to come in.
const SETTLE_FRAMES: usize = 30;
/// The window, points, and its pixels a point: a laptop's display.
const WINDOW: [f32; 2] = [1440.0, 900.0];
const PIXELS_PER_POINT: f32 = 2.0;
/// The design ([`two_million_points`]): groups (a thread each), paths in each (a run of stitches
/// each), anchors in each path (needle points), and the stitches' and rows' spacing, mm: as the
/// stitch-editing gate's design of 2 million needle points.
const GROUPS: usize = 20;
const PATHS_PER_GROUP: usize = 100;
const ANCHORS_PER_PATH: usize = 1000;
const STITCH_MM: f64 = 0.4;
const ROW_MM: f64 = 0.2;
/// The paths' margin from the artboard's edges, and their stroke's width, mm.
const MARGIN_MM: f64 = 10.0;
const STROKE_MM: f64 = 0.4;

/// [`GROUPS`] groups of [`PATHS_PER_GROUP`] paths of [`ANCHORS_PER_PATH`] anchors (2 million in
/// all, as many needle points as a design may have), one layer: a zig-zag row a path, its stitches
/// [`STITCH_MM`] long, the rows [`ROW_MM`] apart, each stroked.
fn two_million_points() -> vectorcraft_doc::Document {
    use vectorcraft_color::{Color, Paint};
    use vectorcraft_doc::{Appearance, Document, Node};
    use vectorcraft_geom::{BezPath, PathData};
    let pt = vectorcraft_engine::cmd::embroidery::mm_to_pt;
    let rows = GROUPS * PATHS_PER_GROUP;
    let mut doc = Document::new(pt(STITCH_MM * ANCHORS_PER_PATH as f64 + 2.0 * MARGIN_MM), pt(ROW_MM * rows as f64 + 2.0 * MARGIN_MM));
    let layer = doc.layers.first().map(|l| l.id);
    for g in 0..GROUPS {
        let shade = u8::try_from(g * 255 / GROUPS).unwrap_or(u8::MAX);
        let paint = Paint::solid(Color::rgb8(shade, 40, 255 - shade));
        let mut paths = Vec::with_capacity(PATHS_PER_GROUP);
        for p in 0..PATHS_PER_GROUP {
            let row = (g * PATHS_PER_GROUP + p) as f64;
            let mut bp = BezPath::new();
            for k in 0..ANCHORS_PER_PATH {
                let zig = if k % 2 == 0 { 0.0 } else { ROW_MM / 2.0 };
                let at = (pt(MARGIN_MM + STITCH_MM * k as f64), pt(MARGIN_MM + ROW_MM * row + zig));
                if k == 0 { bp.move_to(at) } else { bp.line_to(at) }
            }
            let node = Node::path(doc.alloc_id(), PathData::from_bezpath(&bp), Appearance::basic(Paint::None, paint.clone(), pt(STROKE_MM)));
            paths.push(std::sync::Arc::new(node));
        }
        let group = Node::group(doc.alloc_id(), paths);
        doc.insert(layer, usize::MAX, group).expect("the group goes on the layer");
    }
    doc
}

/// One frame at `time` with `events`, as the app draws its canvas, tessellated → its wall time, ms.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, events: Vec<Event>) -> f64 {
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
        crate::canvas::show(app, ui);
        crate::embroidery::canvas_controls(app, ui);
    });
    let shapes = std::mem::take(&mut out.shapes);
    drop(ctx.tessellate(shapes, out.pixels_per_point));
    t0.elapsed().as_secs_f64() * 1000.0
}

/// The value below which `p` of `samples` lie.
fn percentile(mut samples: Vec<f64>, p: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    let i = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples.get(i).copied().unwrap_or(f64::NAN)
}

#[test]
#[ignore = "a measurement, run by hand on an idle machine"]
fn large_design_drag_gate() {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    let mut session = Session::new();
    session.add_document(two_million_points(), None);
    let mut app = VectorcraftApp::new(session, Services::default());
    app.run("tool.select", json!({"tool": "directSelection"})).unwrap();
    let button = |pos: Pos2, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    let mut time = 0.0;
    // Frames come at the display's pace, as in the app: each waits out the rest of its 60 Hz slot.
    let mut next = Instant::now();
    let mut tick = |app: &mut VectorcraftApp, events: Vec<Event>| {
        time += 1.0 / 60.0;
        next += std::time::Duration::from_secs_f64(FRAME_BUDGET_MS / 1000.0);
        let ms = frame(app, &ctx, time, events);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        ms
    };
    for _ in 0..SETTLE_FRAMES {
        tick(&mut app, vec![]);
    }
    // A click on the middle anchor of the middle path of the middle group.
    let doc = app.session.active().unwrap().doc.clone();
    let group = doc.layers[0].children().unwrap()[GROUPS / 2].clone();
    let middle = group.children().unwrap()[PATHS_PER_GROUP / 2].path_data().unwrap().subpaths[0].anchors[ANCHORS_PER_PATH / 2].p;
    let xf = crate::canvas::Xf::new(app.canvas_rect.unwrap(), app.view().unwrap());
    let at = xf.to_screen(middle);
    tick(&mut app, vec![Event::PointerMoved(at), button(at, true)]);
    tick(&mut app, vec![button(at, false)]);
    // At fit the rows lie under a point apart: the click selects the anchor nearest it, of
    // whichever path that is.
    let selected = &app.session.active().unwrap().selection;
    let picked: Vec<_> = selected.anchors.iter().flat_map(|(id, a)| a.iter().map(move |k| (*id, *k))).collect();
    let [(path, (subpath, k))] = picked[..] else { panic!("the click selected {} anchors, not one", picked.len()) };
    let anchor_of = |app: &VectorcraftApp| app.session.active().unwrap().doc.node(path).unwrap().path_data().unwrap().subpaths[subpath].anchors[k].p;
    let anchor = anchor_of(&app);
    let revision = app.session.active().unwrap().revision;
    tick(&mut app, vec![button(at, true)]);
    let mut to = at;
    let mut drag = vec![];
    for _ in 0..DRAG_FRAMES {
        to += egui::vec2(DRAG_STEP, DRAG_STEP / 2.0);
        drag.push(tick(&mut app, vec![Event::PointerMoved(to)]));
    }
    let revisions = app.session.active().unwrap().revision - revision;
    tick(&mut app, vec![button(to, false)]);
    assert_ne!(anchor_of(&app), anchor, "the drag moved the anchor");
    assert!(revisions as usize >= DRAG_FRAMES / 2, "the drag made a revision a frame ({revisions} in {DRAG_FRAMES} frames)");
    let (p50, p95, max) = (percentile(drag.clone(), 0.5), percentile(drag.clone(), 0.95), percentile(drag, 1.0));
    println!(
        "{} anchors, one dragged with Direct Selection: {revisions} revisions in {DRAG_FRAMES} frames, p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms (budget {FRAME_BUDGET_MS:.1})",
        GROUPS * PATHS_PER_GROUP * ANCHORS_PER_PATH
    );
    assert!(p95 <= FRAME_BUDGET_MS, "drag p95 {p95:.2} ms");
}
