//! Pictures of the embroidery interface for the PR and the docs: the whole window (the app bar,
//! the tools, the dock with the Embroidery panel, the canvas and the status bar) drawn offscreen on
//! the GPU at 2 pixels a point (egui_kittest on wgpu: no window is presented, so a locked screen
//! doesn't stop them), of art the test draws itself (the corpus's designs are never committed, so
//! no picture of them is either). It makes the pictures `docs/images` holds, by their names there
//! (`embroidery-object-tab.png`, `embroidery-stitch-editing.png`…): copy them over the committed
//! ones. Run by hand:
//!
//! `VECTORCRAFT_STITCH_SHOTS=<folder> cargo test --release -p vectorcraft-ui-egui --lib -- --ignored embroidery_shots`

use std::path::{Path, PathBuf};

use egui::vec2;
use serde_json::json;
use vectorcraft_engine::Session;

use crate::{Services, VectorcraftApp};

/// The window, points, and its pixels a point: a laptop's display.
const WINDOW: [f32; 2] = [1440.0, 900.0];
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
    shot_then(dir, name, size, prepare, |_, _| {});
}

/// [`shot_sized`], with `then` run once the window has been drawn a few frames (the canvas laid
/// out, so a zoom to a box can fit it).
fn shot_then(dir: &Path, name: &str, size: [f32; 2], prepare: impl FnOnce(&mut VectorcraftApp, u64), then: impl FnOnce(&mut VectorcraftApp, u64)) {
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
    let mut then = Some(then);
    for k in 0..FRAMES {
        if k == FRAMES / 4
            && let Some(then) = then.take()
        {
            then(harness.state_mut(), disc);
        }
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(FRAME_PAUSE_MS));
    }
    harness.render().unwrap().save(dir.join(format!("{name}.png"))).unwrap();
}

/// The red disc converted to manual stitches and edited: isolated, in the stitch view, with the
/// Embroidery Tool; the Embroidery panel at its Stitches tab.
fn editing(app: &mut VectorcraftApp, id: u64) {
    app.run("select.set", json!({ "ids": [id] })).unwrap();
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("object.isolate", json!({ "id": id })).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    app.run("ui.embroideryPanel", json!({"tab": "stitches", "scope": "selection", "filter": "all"})).unwrap();
}

/// Zoomed in on the disc's left side (its needle points drawn as circles), the needle points in a
/// box there selected (the Contextual Task Bar under them, their rows lit in the Stitches tab).
fn zoomed_and_selected(app: &mut VectorcraftApp, _: u64) {
    app.run("view.zoomTo", json!({"rect": EDIT_VIEW, "margin": 0})).unwrap();
    let mm = |v: f64| v * 25.4 / 72.0;
    let [x0, y0, x1, y1] = EDIT_SELECTED;
    app.run("embroidery.selectNeedlePoints", json!({"rect": [mm(x0), mm(y0), mm(x1), mm(y1)]})).unwrap();
}

/// The box the editing pictures zoom to, and the box whose needle points they select, document
/// points: the disc's left edge, where its fill's rows meet its outline.
const EDIT_VIEW: [f64; 4] = [64.0, 78.0, 154.0, 138.0];
const EDIT_SELECTED: [f64; 4] = [96.0, 100.0, 108.0, 112.0];

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
    shot(&dir, "embroidery-object-tab", disc);
    // Every object selected: values they differ in are blank, counted.
    shot(&dir, "embroidery-object-tab-several", |app, id| {
        disc(app, id);
        app.run("select.all", json!({})).unwrap();
    });
    shot(&dir, "embroidery-object-tab-polish", |app, id| {
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
    shot(&dir, "embroidery-document-setup", setup);
    shot(&dir, "embroidery-document-setup-polish", |app, id| {
        app.session.prefs.interface_language = "pl".into();
        setup(app, id);
    });
    // The embroidery tool on the disc: its entry set on the outline, its exit left to the planner;
    // in the art view, then over the stitches.
    let tool = |app: &mut VectorcraftApp, id: u64| {
        app.run("select.set", json!({ "ids": [id] })).unwrap();
        app.run("embroidery.setEntryExit", json!({"id": id, "part": "fill", "entry": [70, 110], "snap": true})).unwrap();
        app.run("ui.embroideryPanel", json!({"tab": "object"})).unwrap();
        app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    };
    shot(&dir, "embroidery-tool-art", tool);
    shot(&dir, "embroidery-tool-stitches", |app, id| {
        tool(app, id);
        app.run("view.stitchPreview", json!({"on": true})).unwrap();
    });
    shot(&dir, "embroidery-tool-point-dialog", |app, id| {
        tool(app, id);
        let entry = app.run("embroidery.info", json!({"ids": [id]})).unwrap()["objects"][0]["settings"]["fill"]["entry"].clone();
        crate::embroidery::point_dialog::open(app, &json!({"kind": "entry", "id": id, "part": "fill", "at": entry, "set": true}));
    });
    shot(&dir, "embroidery-tool-polish", |app, id| {
        app.session.prefs.interface_language = "pl".into();
        tool(app, id);
        app.run("view.stitchPreview", json!({"on": true})).unwrap();
    });
    // Editing stitches: the disc's needle points, some selected, with the Stitches tab.
    shot_then(&dir, "embroidery-stitch-editing", WINDOW, editing, zoomed_and_selected);
    shot_then(
        &dir,
        "embroidery-stitch-editing-polish",
        WINDOW,
        |app, id| {
            app.session.prefs.interface_language = "pl".into();
            editing(app, id);
        },
        zoomed_and_selected,
    );
    // The Stitches tab outside editing: the whole design's records, one in the middle in focus,
    // marked on the canvas, zoomed to.
    shot_then(
        &dir,
        "embroidery-stitches-tab",
        WINDOW,
        |app, _| {
            app.run("view.stitchPreview", json!({"on": true})).unwrap();
            app.run("ui.embroideryPanel", json!({"tab": "stitches", "scope": "design", "filter": "all"})).unwrap();
        },
        |app, _| {
            let count = app.run("embroidery.stitches", json!({"limit": 0})).unwrap()["recordCount"].as_u64().unwrap();
            let middle = app.run("embroidery.stitches", json!({"from": count / 2, "to": count / 2})).unwrap();
            let record = &middle["records"][0];
            let pt = |mm: f64| mm * 72.0 / 25.4;
            let (x, y) = (pt(record["at"][0].as_f64().unwrap()), pt(record["at"][1].as_f64().unwrap()));
            let half = pt(SHOWN_MM / 2.0);
            app.run("view.zoomTo", json!({"rect": [x - half, y - half, x + half, y + half], "margin": 0})).unwrap();
            let number = usize::try_from(record["record"].as_u64().unwrap()).unwrap();
            crate::panels::embroidery::stitches::reveal(app, number);
        },
    );
}

/// How much of the design the Stitches tab's picture shows round the stitch shown, millimetres.
const SHOWN_MM: f64 = 60.0;
