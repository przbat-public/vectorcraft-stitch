//! The canvas in the stitch view, frame by frame: the stitches drawn from their own texture, which a
//! selection change doesn't upload again, over the fabric; the art back fresh when the view goes
//! back to it; a document switch never drawn in line where the layer's thread runs, a slow frame's
//! next sent to it, and its being gone; the art's passes; isolation fading the rest; a design that
//! fails faded.

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::embroidery::scene::FABRIC;

use super::*;
use crate::embroidery::stitch_preview;
use crate::{Services, canvas};

/// How long a test waits for the layer's thread: far longer than a frame takes.
const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    document(&mut app);
    app
}

/// A document with two squares in two colours.
fn document(app: &mut VectorcraftApp) {
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for (color, x) in [("#2a6fdb", 20), ("#d01818", 110)] {
        app.run("paint.setFill", json!({"color": color})).unwrap();
        app.run("shape.rectangle", json!({"x": x, "y": 40, "width": 70, "height": 70})).unwrap();
    }
}

/// One headless frame at `time`, as the app runs one: the logic (the plans taken in), the canvas,
/// and the preview's scheduling at its end → the shapes drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) -> Vec<Shape> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        canvas::show(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut all = vec![];
    flatten(out.shapes.into_iter().map(|c| c.shape).collect(), &mut all);
    all
}

fn flatten(shapes: Vec<Shape>, out: &mut Vec<Shape>) {
    for s in shapes {
        match s {
            Shape::Vec(v) => flatten(v, out),
            s => out.push(s),
        }
    }
}

/// The meshes drawn with texture `tex`: their vertices' tint.
fn drawn_with(shapes: &[Shape], tex: Option<egui::TextureId>) -> Vec<Color32> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Mesh(m) if Some(m.texture_id) == tex => m.vertices.first().map(|v| v.color),
            _ => None,
        })
        .collect()
}

fn stitch_texture(app: &VectorcraftApp) -> Option<egui::TextureId> {
    app.embroidery.layer.texture.as_ref().map(egui::TextureHandle::id)
}

fn emphasis_texture(app: &VectorcraftApp) -> Option<egui::TextureId> {
    app.embroidery.layer.emphasis_texture.as_ref().map(egui::TextureHandle::id)
}

fn art_texture(app: &VectorcraftApp) -> Option<egui::TextureId> {
    app.canvas.texture.as_ref().map(egui::TextureHandle::id)
}

/// Frames until the stitches show (the first plan made in line, then drawn) → the time reached.
fn until_stitches(app: &mut VectorcraftApp, ctx: &egui::Context, mut time: f64) -> f64 {
    for _ in 0..3 {
        time += 0.02;
        frame(app, ctx, time);
    }
    assert!(app.embroidery.layer.shown, "the stitch view is drawn");
    time
}

#[test]
fn the_stitches_are_drawn_from_their_own_texture_over_the_fabric() {
    let mut app = app();
    let ctx = egui::Context::default();
    let shapes = frame(&mut app, &ctx, 0.0);
    assert!(!drawn_with(&shapes, art_texture(&app)).is_empty(), "the art");
    assert!(app.embroidery.preview.shown(app.session.active().unwrap().uid).is_none(), "nothing planned with the view off");
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    // The first frame asks for the plan and still shows the art.
    let shapes = frame(&mut app, &ctx, 0.1);
    assert!(!drawn_with(&shapes, art_texture(&app)).is_empty(), "the art until the first plan comes");
    let shapes = frame(&mut app, &ctx, 0.2);
    assert_eq!(app.embroidery.layer.uploads, 1);
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [Color32::WHITE], "the stitches, once");
    assert!(drawn_with(&shapes, art_texture(&app)).is_empty(), "the art is hidden under the stitches by default");
    // The stitches are the plan's, as `embroidery.render` draws them.
    let uid = app.session.active().unwrap().uid;
    let key = app.embroidery.layer.key.clone().unwrap();
    assert_eq!(key.generation, app.embroidery.preview.shown(uid).unwrap().planned.generation);
    // Uploaded once: four bytes a pixel of the canvas.
    assert_eq!(app.embroidery.layer.uploaded_bytes, u64::from(key.w) * u64::from(key.h) * 4);
    // A selection change, a hover: the texture is drawn again, nothing uploaded, no art rendered.
    let art_key = app.canvas.key.clone();
    app.run("select.all", json!({})).unwrap();
    let shapes = frame(&mut app, &ctx, 0.3);
    frame(&mut app, &ctx, 0.4);
    assert_eq!(app.embroidery.layer.uploads, 1, "no upload for a selection change");
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [Color32::WHITE]);
    assert_eq!(app.canvas.key, art_key, "the art isn't rendered while it is hidden");
    // A zoom draws them again.
    app.run("view.zoomIn", json!({})).unwrap();
    frame(&mut app, &ctx, 0.5);
    assert_eq!(app.embroidery.layer.uploads, 2);
    // So does Show Needle Points, and its dots are drawn.
    app.run("view.stitchPreview.needlePoints", json!({"on": true})).unwrap();
    frame(&mut app, &ctx, 0.6);
    assert_eq!(app.embroidery.layer.uploads, 3);
    assert!(app.embroidery.layer.key.as_ref().is_some_and(|k| k.needle_points));
}

/// The stitch view lays the fabric in place of the paper: on the artboard, and under the stitches'
/// box where they reach past it; the art view lays nothing past the artboard.
#[test]
fn the_fabric_is_laid_on_the_artboard_and_under_the_stitches() {
    let mut app = app();
    // A bar reaching past the artboard's right edge.
    app.run("shape.rectangle", json!({"x": 150, "y": 150, "width": 120, "height": 30})).unwrap();
    let ctx = egui::Context::default();
    let fabric = Color32::from_rgb(FABRIC[0], FABRIC[1], FABRIC[2]);
    let laid = |shapes: &[Shape]| -> Vec<egui::Rect> {
        shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) if p.closed && p.fill == fabric => Some(egui::Rect::from_points(&p.points)),
                _ => None,
            })
            .collect()
    };
    let art = laid(&frame(&mut app, &ctx, 0.0));
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    let stitches = laid(&frame(&mut app, &ctx, time + 0.1));
    let [artboard, design] = stitches[..] else { panic!("the artboard's fabric and the stitches': {stitches:?}") };
    assert!(design.right() > artboard.right() + 10.0, "under the stitches past the artboard: {design:?} {artboard:?}");
    assert!(artboard.contains(design.left_top() + egui::vec2(1.0, 1.0)), "the design starts on the artboard");
    assert!(art.iter().all(|r| r.right() <= artboard.right() + 0.5), "the art view lays nothing past it: {art:?}");
}

/// The fabric is laid in the colour the document's embroidery settings give it, and the stitches
/// are drawn for it (the edges of threads close to it follow it).
#[test]
fn the_fabric_has_the_documents_colour() {
    let mut app = app();
    let navy = [20, 30, 90];
    app.session
        .edit("Embroidery Setup", |d, _| {
            let fabric = vectorcraft_doc::embroidery::FabricSettings { color: Some(navy), ..Default::default() };
            d.embroidery = Some(Box::new(vectorcraft_doc::embroidery::EmbroiderySetup { fabric: Some(Box::new(fabric)), ..Default::default() }));
            Ok(())
        })
        .unwrap();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    let shapes = frame(&mut app, &ctx, time + 0.1);
    let fabric = Color32::from_rgb(navy[0], navy[1], navy[2]);
    assert!(shapes.iter().any(|s| matches!(s, Shape::Path(p) if p.closed && p.fill == fabric)), "laid in the fabric's colour");
    assert_eq!(app.embroidery.layer.key.as_ref().map(|k| k.fabric), Some(navy));
}

/// Back in the art view after an edit made in the stitch view, the art is rendered before it shows:
/// never the art as it was before the edit.
#[test]
fn back_to_the_art_never_shows_stale_art() {
    let mut app = app();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, 0.0);
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 5, "dy": 0})).unwrap();
    frame(&mut app, &ctx, time + 0.2);
    app.run("view.stitchPreview", json!({"on": false})).unwrap();
    let shapes = frame(&mut app, &ctx, time + 0.4);
    let revision = app.session.active().unwrap().revision;
    assert_eq!(app.canvas.key.as_ref().map(|k| k.revision), Some(revision), "rendered for the document as it is");
    assert!(app.canvas.key.as_ref().is_some_and(|k| k.hidden.is_empty() && k.templates));
    assert!(!drawn_with(&shapes, art_texture(&app)).is_empty());
    assert!(drawn_with(&shapes, stitch_texture(&app)).is_empty());
}

/// Where the layer's thread runs, switching to another document draws its stitches there: no frame
/// is drawn in line, and the other document's stitches never show meanwhile.
#[test]
fn a_document_switch_is_drawn_on_the_layers_thread() {
    let ctx = egui::Context::default();
    let Some(worker) = LineWorker::start(&ctx) else { panic!("the layer's thread starts") };
    let mut app = app();
    document(&mut app);
    for index in [0, 1] {
        app.session.set_active(index);
        app.run("view.stitchPreview", json!({"on": true})).unwrap();
    }
    let mut time = until_stitches(&mut app, &ctx, 0.0);
    let second = app.session.active().unwrap().uid;
    app.embroidery.layer.worker = Some(worker);
    app.embroidery.layer.worker_started = true;
    app.session.set_active(0);
    let first = app.session.active().unwrap().uid;
    // Its first plan comes first (the art meanwhile), then its stitches go to the thread.
    let uploads = app.embroidery.layer.uploads;
    let t0 = std::time::Instant::now();
    let mut waited = false;
    loop {
        time += 0.02;
        let shapes = frame(&mut app, &ctx, time);
        let key = app.embroidery.layer.key.as_ref().map(|k| k.uid);
        // Planned, and its stitches asked of the thread: none drawn meanwhile.
        waited |= app.embroidery.preview.shown(first).is_some() && key.is_none() && drawn_with(&shapes, stitch_texture(&app)).is_empty();
        let drawn = !drawn_with(&shapes, stitch_texture(&app)).is_empty();
        assert!(!drawn || key != Some(second), "never the other document's stitches");
        if key == Some(first) {
            assert!(drawn);
            break;
        }
        assert!(t0.elapsed() < WAIT, "the thread never drew them");
        std::thread::yield_now();
    }
    assert!(waited, "not drawn in line");
    assert_eq!(app.embroidery.layer.uploads, uploads + 1, "drawn on the thread, uploaded once");
}

/// The art's passes: hidden under the stitches by default (nothing rendered), its templates alone
/// when there are some, faint when asked, its outlines over the stitches with Outline.
#[test]
fn the_art_shows_as_the_view_says() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    until_stitches(&mut app, &ctx, 0.0);
    let pass = art_pass(&mut app);
    assert_eq!(pass, ArtPass { stitches: true, render: false, hidden: vec![], hide_templates: false });
    // A template layer: drawn alone.
    let layer = app.run("layer.new", json!({"name": "Sketch"})).unwrap();
    let layer = layer["id"].as_u64().unwrap();
    app.run("layer.setProps", json!({"id": layer, "template": true})).unwrap();
    let pass = art_pass(&mut app);
    let ids: Vec<u64> = app.session.doc().unwrap().doc.layers.iter().map(|l| l.id.0).filter(|id| *id != layer).collect();
    assert_eq!((pass.render, pass.hidden), (true, ids), "everything but the template");
    // Worked out once for the snapshot.
    let doc = app.session.doc().unwrap().doc.clone();
    assert!(app.embroidery.layer.templates_only.as_ref().is_some_and(|(of, _)| std::ptr::eq(of.as_ptr(), Arc::as_ptr(&doc))));
    app.run("view.stitchPreview.artwork", json!({"on": true})).unwrap();
    assert_eq!(art_pass(&mut app), ArtPass { stitches: true, render: true, hidden: vec![], hide_templates: false });
    let shapes = frame(&mut app, &ctx, 1.0);
    assert_eq!(drawn_with(&shapes, art_texture(&app)), [faded()], "the art faint");
    // A selection change with the art shown, once the plan of the document as it is shows: the
    // stitches aren't drawn again.
    let uid = app.session.active().unwrap().uid;
    for t in [1.2, 1.4] {
        frame(&mut app, &ctx, t);
    }
    assert_eq!(app.embroidery.preview.freshness(uid, 1.4), stitch_preview::Freshness::Fresh);
    let uploads = app.embroidery.layer.uploads;
    app.run("select.all", json!({})).unwrap();
    frame(&mut app, &ctx, 1.45);
    assert_eq!(app.embroidery.layer.uploads, uploads, "no stitches uploaded for a selection change");
    app.run("view.outline", json!({})).unwrap();
    assert_eq!(art_pass(&mut app), ArtPass { stitches: true, render: true, hidden: vec![], hide_templates: true });
    // The outlines, once the canvas has them (it may render them on its thread).
    let (mut time, t0) = (1.5, std::time::Instant::now());
    let shapes = loop {
        let shapes = frame(&mut app, &ctx, time);
        if app.canvas.key.as_ref().is_some_and(|k| k.outline) {
            break shapes;
        }
        assert!(t0.elapsed() < WAIT, "the outlines never came");
        time += 0.02;
        std::thread::yield_now();
    };
    let order: Vec<Option<egui::TextureId>> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Mesh(m) if [stitch_texture(&app), art_texture(&app)].contains(&Some(m.texture_id)) => Some(Some(m.texture_id)),
            _ => None,
        })
        .collect();
    assert_eq!(order, [stitch_texture(&app), art_texture(&app)], "the outlines over the stitches");
    app.run("view.stitchPreview", json!({"on": false})).unwrap();
    assert_eq!(art_pass(&mut app), ArtPass::art());
}

/// In isolation mode the stitches of the objects outside the group are faded.
#[test]
fn isolation_fades_the_rest() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    until_stitches(&mut app, &ctx, 0.0);
    let uid = app.session.active().unwrap().uid;
    let scene = app.embroidery.preview.shown(uid).unwrap().planned.scene();
    let doc = app.session.doc().unwrap().doc.clone();
    let objects: Vec<NodeId> = scene.units().iter().map(|u| u.object).collect();
    assert_eq!(objects.len(), 2, "{objects:?}");
    let faded = outside(&scene, &doc, objects[0]);
    assert!(!faded.contains(0) && faded.contains(1));
    app.run("object.isolate", json!({"id": objects[0].0})).unwrap();
    let isolated = app.session.active().unwrap().isolation;
    assert_eq!(isolated, Some(objects[0]));
    frame(&mut app, &ctx, 1.0);
    frame(&mut app, &ctx, 1.1);
    assert!(app.embroidery.layer.key.as_ref().is_some_and(|k| k.isolated == Some(objects[0].0)), "drawn for the isolation");
}

/// A design that fails keeps its last good stitches, faded.
#[test]
fn a_design_that_fails_shows_its_last_stitches_faded() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    // Three metres a side: more stitches than one shape may have.
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    let mut shapes = vec![];
    for k in 1..4 {
        shapes = frame(&mut app, &ctx, time + 0.2 * f64::from(k));
    }
    let uid = app.session.active().unwrap().uid;
    assert!(app.embroidery.preview.failed(uid));
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [faded()]);
    let _: Value = app.run("edit.undo", json!({})).unwrap();
}

/// Zoomed in on one square, the other square's lines are left out of the frame, not drawn outside
/// it.
#[test]
fn lines_outside_the_view_are_left_out() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    let whole = app.embroidery.layer.renderer.stats;
    assert_eq!(whole.culled, 0, "everything in view at first: {whole:?}");
    if let Some(v) = app.view_mut() {
        v.zoom *= 4.0;
        v.center = vectorcraft_geom::Point::new(55.0, 75.0);
    }
    frame(&mut app, &ctx, time + 0.1);
    let zoomed = app.embroidery.layer.renderer.stats;
    assert!(zoomed.culled > 0 && zoomed.drawn < whole.drawn, "{zoomed:?} against {whole:?}");
}

/// Emphasis for a moment: the frame already drawn is tinted faded and the emphasised units drawn
/// over it from a raster of their own; the stitches' own texture isn't drawn again, and the next
/// frame without emphasis shows them as before.
#[test]
fn emphasis_fades_the_rest_without_drawing_the_stitches_again() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    let uploads = app.embroidery.layer.uploads;
    app.embroidery.layer.emphasize(vec![0]);
    let shapes = frame(&mut app, &ctx, time + 0.1);
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [faded()], "the rest faded");
    let emphasized = emphasis_texture(&app);
    assert_eq!(drawn_with(&shapes, emphasized), [Color32::WHITE], "the unit over them");
    assert_eq!(app.embroidery.layer.uploads, uploads, "the stitches aren't drawn again");
    // Another unit: drawn into the same texture.
    app.embroidery.layer.emphasize(vec![1]);
    let shapes = frame(&mut app, &ctx, time + 0.15);
    assert_eq!(drawn_with(&shapes, emphasized), [Color32::WHITE]);
    assert_eq!(emphasis_texture(&app), emphasized, "one texture");
    let shapes = frame(&mut app, &ctx, time + 0.2);
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [Color32::WHITE], "a moment only");
    assert!(drawn_with(&shapes, emphasized).is_empty());
    assert!(emphasis_texture(&app).is_none(), "its texture goes when it ends");
}

/// A design slow to draw has its emphasis drawn on the layer's thread, as its frames are: the rest
/// faded meanwhile, the units over them once they come; one asked is not asked again.
#[test]
fn a_slow_designs_emphasis_is_drawn_on_the_layers_thread() {
    let ctx = egui::Context::default();
    let Some(worker) = LineWorker::start(&ctx) else { panic!("the layer's thread starts") };
    let mut app = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let mut time = until_stitches(&mut app, &ctx, 0.0);
    app.embroidery.layer.use_worker(worker);
    app.embroidery.layer.last_ms = 1000.0;
    let drawn = app.embroidery.layer.renderer.stats;
    app.embroidery.layer.emphasize(vec![0]);
    time += 0.1;
    let shapes = frame(&mut app, &ctx, time);
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [faded()], "the rest faded at once");
    assert!(app.embroidery.layer.emphasis_asked.is_some(), "asked of the thread");
    assert_eq!(app.embroidery.layer.renderer.stats, drawn, "nothing drawn in line");
    let t0 = std::time::Instant::now();
    loop {
        app.embroidery.layer.emphasize(vec![0]);
        time += 0.02;
        let shapes = frame(&mut app, &ctx, time);
        if !drawn_with(&shapes, emphasis_texture(&app)).is_empty() {
            break;
        }
        assert!(t0.elapsed() < WAIT, "the thread never drew it");
        std::thread::yield_now();
    }
    assert!(app.embroidery.layer.emphasis_asked.is_none());
    assert_eq!(app.embroidery.layer.renderer.stats, drawn, "still nothing drawn in line");
}

/// A frame that took longer than [`INLINE_DRAW_MS`] sends the next to the layer's thread, the frame
/// shown staying meanwhile; one within it has the next drawn in line.
#[test]
fn a_slow_frame_sends_the_next_to_the_layers_thread() {
    let ctx = egui::Context::default();
    let Some(worker) = LineWorker::start(&ctx) else { panic!("the layer's thread starts") };
    let mut app = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let mut time = until_stitches(&mut app, &ctx, 0.0);
    app.embroidery.layer.use_worker(worker);
    app.embroidery.layer.last_ms = INLINE_DRAW_MS;
    let uploads = app.embroidery.layer.uploads;
    app.run("view.zoomIn", json!({})).unwrap();
    time += 0.1;
    frame(&mut app, &ctx, time);
    assert!(!app.embroidery.layer.waiting(), "within a frame's time: in line");
    assert_eq!(app.embroidery.layer.uploads, uploads + 1);
    app.embroidery.layer.last_ms = INLINE_DRAW_MS + 1.0;
    let drawn = app.embroidery.layer.renderer.stats;
    app.run("view.zoomIn", json!({})).unwrap();
    time += 0.1;
    let shapes = frame(&mut app, &ctx, time);
    assert!(app.embroidery.layer.waiting(), "past it: asked of the thread");
    assert_eq!(app.embroidery.layer.renderer.stats, drawn, "nothing drawn in line");
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [Color32::WHITE], "the frame before shows meanwhile");
    let t0 = std::time::Instant::now();
    while app.embroidery.layer.waiting() {
        assert!(t0.elapsed() < WAIT, "the thread never drew it");
        std::thread::yield_now();
        time += 0.02;
        frame(&mut app, &ctx, time);
    }
    assert_eq!(app.embroidery.layer.uploads, uploads + 2, "the thread's frame shown");
}

/// With the layer's thread gone (its channels closed), the stitches are drawn in line from then
/// on, also a frame that would have gone to it.
#[test]
fn with_the_layers_thread_gone_the_stitches_are_drawn_in_line() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    let (tx, jobs) = channel::<LayerJob>();
    let (done, rx) = channel::<LayerDone>();
    drop((jobs, done));
    app.embroidery.layer.use_worker(LineWorker { tx, rx });
    app.embroidery.layer.last_ms = 1000.0;
    let uploads = app.embroidery.layer.uploads;
    app.run("view.zoomIn", json!({})).unwrap();
    let shapes = frame(&mut app, &ctx, time + 0.1);
    assert!(app.embroidery.layer.worker.is_none(), "found gone");
    assert!(!app.embroidery.layer.waiting());
    assert_eq!(app.embroidery.layer.uploads, uploads + 1, "drawn in line");
    assert_eq!(drawn_with(&shapes, stitch_texture(&app)), [Color32::WHITE]);
}

/// A frame asked of the layer's thread is forgotten when the stitch view stops being drawn: a
/// window capture never waits for it (it would wait its whole bound, 5 s, every time).
#[test]
fn a_frame_asked_of_the_thread_is_forgotten_when_the_stitches_go() {
    let ctx = egui::Context::default();
    let Some(worker) = LineWorker::start(&ctx) else { panic!("the layer's thread starts") };
    let mut app = app();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    let time = until_stitches(&mut app, &ctx, 0.0);
    app.embroidery.layer.use_worker(worker);
    // A design slow to draw: the next frame goes to the thread.
    app.embroidery.layer.last_ms = 1000.0;
    app.run("view.zoomIn", json!({})).unwrap();
    frame(&mut app, &ctx, time + 0.1);
    assert!(app.embroidery.layer.waiting(), "asked of the thread");
    app.run("view.stitchPreview", json!({"on": false})).unwrap();
    frame(&mut app, &ctx, time + 0.2);
    assert!(!app.embroidery.layer.waiting());
    assert!(!crate::embroidery::holds_screenshots(&app, time + 0.2));
}

/// The art over or under the stitches is only ever the art of the pass shown: on a document whose
/// art the canvas renders on its thread, Outline turned on in the stitch view shows no art until the
/// outlines come, never the art in colour over the stitches.
#[test]
fn the_art_of_another_pass_never_shows_with_the_stitches() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("view.stitchPreview.artwork", json!({"on": true})).unwrap();
    let mut time = until_stitches(&mut app, &ctx, 0.0);
    time += 0.05;
    let shapes = frame(&mut app, &ctx, time);
    assert_eq!(drawn_with(&shapes, art_texture(&app)), [faded()], "the art, faint under the stitches");
    // A document slow to render: the canvas's next frame goes to its thread, the texture kept.
    app.canvas.last_ms = 1000.0;
    app.run("view.outline", json!({})).unwrap();
    time += 0.05;
    let shapes = frame(&mut app, &ctx, time);
    if app.canvas.key.as_ref().is_some_and(|k| !k.outline) {
        assert!(drawn_with(&shapes, art_texture(&app)).is_empty(), "not the art in colour while the outlines are made");
    }
    let t0 = std::time::Instant::now();
    loop {
        time += 0.02;
        let shapes = frame(&mut app, &ctx, time);
        if app.canvas.key.as_ref().is_some_and(|k| k.outline) {
            assert_eq!(drawn_with(&shapes, art_texture(&app)), [Color32::WHITE], "the outlines over the stitches");
            break;
        }
        assert!(t0.elapsed() < WAIT, "the outlines never came");
        std::thread::yield_now();
    }
}
