//! The canvas in the stitch view, frame by frame: the stitches drawn from their own texture, which a
//! selection change doesn't upload again; the art back fresh when the view goes back to it; a
//! document switch never drawn in line where the layer's thread runs; the art's passes; isolation
//! fading the rest; a design that fails faded.

use serde_json::{Value, json};
use vectorcraft_engine::Session;

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
    let Some(worker) = LineWorker::start(&ctx) else { return };
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
    app.run("view.stitchPreview.artwork", json!({"on": true})).unwrap();
    assert_eq!(art_pass(&mut app), ArtPass { stitches: true, render: true, hidden: vec![], hide_templates: false });
    let shapes = frame(&mut app, &ctx, 1.0);
    assert_eq!(drawn_with(&shapes, art_texture(&app)), [faded()], "the art faint");
    app.run("view.outline", json!({})).unwrap();
    assert_eq!(art_pass(&mut app), ArtPass { stitches: true, render: true, hidden: vec![], hide_templates: true });
    let shapes = frame(&mut app, &ctx, 1.1);
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
