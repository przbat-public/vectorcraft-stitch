//! Pictures of the embroidery interface for the PR and the docs: the whole window (the app bar,
//! the tools, the dock with the Embroidery panel, the canvas and the status bar) drawn offscreen on
//! the GPU at 2 pixels a point (egui_kittest on wgpu: no window is presented, so a locked screen
//! doesn't stop them), of art the test draws itself (the corpus's designs are never committed, so
//! no picture of them is either). Run by hand:
//!
//! `VECTORCRAFT_STITCH_SHOTS=<folder> cargo test --release -p vectorcraft-ui-egui --lib -- --ignored embroidery_shots`

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use vectorcraft_engine::Session;

use crate::{Services, VectorcraftApp};

/// The window, points, and its pixels a point: a laptop's display.
const WINDOW: [f32; 2] = [1440.0, 900.0];
/// A small window, points: Document Setup's Embroidery tab in one column.
const NARROW_WINDOW: [f32; 2] = [800.0, 760.0];
const PIXELS_PER_POINT: f32 = 2.0;
/// Frames each picture is given to settle (fonts, the plan, the canvas's tiles), and the pause
/// between them, milliseconds: the background work they wait for.
const FRAMES: usize = 40;
const FRAME_PAUSE_MS: u64 = 25;

/// A badge drawn for the pictures: a green field, a red disc with a dark ring three millimetres
/// wide, and a small black seed → the app and the disc's id.
fn badge() -> (VectorcraftApp, u64) {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    let run = |app: &mut VectorcraftApp, cmd: &str, params: serde_json::Value| app.run(cmd, params).unwrap();
    run(&mut app, "file.new", json!({"width": 300, "height": 220, "artboards": 1}));
    run(&mut app, "paint.setStroke", json!({"none": true}));
    run(&mut app, "paint.setFill", json!({"color": "#2a6c31"}));
    run(&mut app, "shape.rectangle", json!({"x": 20, "y": 20, "width": 260, "height": 180}));
    run(&mut app, "select.set", json!({"ids": []}));
    run(&mut app, "paint.setFill", json!({"color": "#c40006"}));
    run(&mut app, "paint.setStroke", json!({"color": "#3a1a10"}));
    let disc = run(&mut app, "shape.ellipse", json!({"x": 70, "y": 35, "width": 160, "height": 150}))["id"].as_u64().unwrap();
    run(&mut app, "stroke.set", json!({"weight": 8.5, "ids": [disc]}));
    run(&mut app, "select.set", json!({"ids": []}));
    run(&mut app, "paint.setStroke", json!({"none": true}));
    run(&mut app, "paint.setFill", json!({"color": "#000000"}));
    run(&mut app, "shape.ellipse", json!({"x": 140, "y": 95, "width": 18, "height": 28}));
    run(&mut app, "select.set", json!({"ids": []}));
    (app, disc)
}

/// The window drawn after `prepare`, saved as `name`.png in `dir`.
fn shot(dir: &Path, name: &str, prepare: impl FnOnce(&mut VectorcraftApp, u64)) {
    shot_sized(dir, name, WINDOW, prepare);
}

/// [`shot`] in a window of `size` points.
fn shot_sized(dir: &Path, name: &str, size: [f32; 2], prepare: impl FnOnce(&mut VectorcraftApp, u64)) {
    let (mut app, disc) = badge();
    prepare(&mut app, disc);
    let mut harness =
        egui_kittest::Harness::builder().with_size(vec2(size[0], size[1])).with_pixels_per_point(PIXELS_PER_POINT).wgpu().build_ui_state(
            |ui, app: &mut VectorcraftApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
    harness.input_mut().max_texture_side = Some(8192);
    for _ in 0..FRAMES {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(FRAME_PAUSE_MS));
    }
    harness.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore = "pictures, made by hand"]
fn embroidery_shots() {
    let Some(dir) = std::env::var_os("VECTORCRAFT_STITCH_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    // The red disc, with an angle and a pull of its own: the Object tab.
    let disc = |app: &mut VectorcraftApp, id: u64| {
        app.run("select.set", json!({ "ids": [id] })).unwrap();
        app.run("embroidery.set", json!({"part": "fill", "angle": 30, "pull": 0.35})).unwrap();
        app.run("ui.embroideryPanel", json!({"tab": "object"})).unwrap();
    };
    shot(&dir, "object-tab", disc);
    shot(&dir, "object-tab-stitches", |app, id| {
        disc(app, id);
        app.run("view.stitchPreview", json!({"on": true})).unwrap();
    });
    shot(&dir, "object-tab-stroke", |app, id| {
        disc(app, id);
        app.run("ui.embroideryPanel", json!({"part": "stroke"})).unwrap();
    });
    // Every object selected: values they differ in are blank, counted.
    shot(&dir, "object-tab-several", |app, id| {
        disc(app, id);
        app.run("select.all", json!({})).unwrap();
    });
    shot(&dir, "object-tab-polish", |app, id| {
        // The interface's language, for this app alone (as the language tests set it).
        app.session.prefs.interface_language = "pl".into();
        disc(app, id);
    });
    // Document Setup's Embroidery tab: the knit chosen, its stitch length changed, the disc's own
    // pull counted in the bar.
    let setup = |app: &mut VectorcraftApp, id: u64| {
        app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
        app.run("embroidery.set", json!({"ids": [id], "part": "fill", "pull": 0.35})).unwrap();
        app.run("file.documentSetup", json!({})).unwrap();
        let d = app.ui.dialog.as_mut().unwrap();
        d.fields.insert("tab".into(), json!(crate::embroidery::setup_tab::TAB));
        crate::embroidery::setup_tab::choose_value(app, "fabric", json!("stretchKnit"));
        crate::embroidery::setup_tab::choose_value(app, "tatami.length", json!(3.5));
    };
    shot(&dir, "document-setup-embroidery", setup);
    shot_sized(&dir, "document-setup-embroidery-narrow", NARROW_WINDOW, setup);
    shot(&dir, "document-setup-embroidery-polish", |app, id| {
        app.session.prefs.interface_language = "pl".into();
        setup(app, id);
    });
}
