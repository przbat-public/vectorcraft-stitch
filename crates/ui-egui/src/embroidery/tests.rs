//! The embroidery interface's own tests that need the app: the control channel's picture of the
//! stitches.

use serde_json::{Value, json};
use vectorcraft_engine::Session;

use crate::control::ControlRequest;
use crate::{Services, VectorcraftApp};

/// An app answering control requests, and a way to send it one.
fn controlled() -> (VectorcraftApp, std::sync::mpsc::Sender<ControlRequest>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (VectorcraftApp::new(Session::new(), Services::default()).with_control(rx), tx)
}

fn request(app: &mut VectorcraftApp, tx: &std::sync::mpsc::Sender<ControlRequest>, method: &str, params: Value) -> Value {
    let (req, reply) = ControlRequest::new(method, params);
    tx.send(req).unwrap();
    app.drain_control(&egui::Context::default());
    reply.try_recv().unwrap()
}

/// The PNG of a `ui.render` reply, decoded.
fn image(r: &Value) -> image::RgbaImage {
    let png = vectorcraft_format::base64_decode(r["result"]["pngBase64"].as_str().unwrap_or_else(|| panic!("no picture: {r}"))).unwrap();
    image::load_from_memory(&png).unwrap().to_rgba8()
}

/// `ui.render {stitchPreview: true}` draws the artboard's stitches, the art's square in its
/// thread's colour as `embroidery.render` draws it; without it, the art; with no document, an error.
#[test]
fn ui_render_draws_the_stitches_when_asked() {
    let (mut app, tx) = controlled();
    assert_eq!(request(&mut app, &tx, "ui.render", json!({"stitchPreview": true}))["ok"], false, "no document");
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("paint.setFill", json!({"color": "#2a6fdb"})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 60})).unwrap();
    let stitches = image(&request(&mut app, &tx, "ui.render", json!({"stitchPreview": true, "scale": 2})));
    let art = image(&request(&mut app, &tx, "ui.render", json!({"scale": 2})));
    assert_eq!((stitches.width(), stitches.height()), (200, 200));
    let r = app.run("embroidery.render", json!({"artboard": 0, "scale": 2})).unwrap();
    let png = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    let engine = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(stitches, engine, "the same drawing as embroidery.render");
    assert_eq!(art.get_pixel(100, 100).0, [0x2a, 0x6f, 0xdb, 255]);
    assert_ne!(stitches.get_pixel(100, 100).0, art.get_pixel(100, 100).0, "the palette thread, not the art's colour");
    assert_eq!(stitches.get_pixel(2, 2).0, [255, 255, 255, 255], "the fabric");
}

/// An embroidery export from the app (File › Export) gives the stitch preview its options.
#[test]
fn an_export_from_the_app_gives_the_preview_its_options() {
    use vectorcraft_engine::EmbroideryOptions;
    use vectorcraft_engine::cmd::embroidery::view::OptionsSource;
    let services = Services { write: Some(Box::new(|_: &str, _: &[u8]| Ok(()))), ..Default::default() };
    let mut app = VectorcraftApp::new(Session::new(), services);
    app.session.execute("prefs.set", &json!({"values": {"backgroundExport": false}})).unwrap();
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 60})).unwrap();
    let uid = app.session.active().unwrap().uid;
    assert_eq!(app.session.stitch_preview_options(uid).1, OptionsSource::Defaults);
    crate::io::export(&mut app, Some("dst"), Some("/out/a.dst".into()), &json!({"spacing": 0.5})).unwrap();
    let ours = EmbroideryOptions::from_json(&json!({"spacing": 0.5})).unwrap();
    assert_eq!(app.session.stitch_preview_options(uid), (ours, OptionsSource::Document));
}

/// View › Stitch Preview follows Pixel Preview, with its options in a submenu; their checkmarks
/// follow the active document's stitch view; ⌘E toggles it; the tab says "Stitches" while the
/// canvas shows them; `ui.set {stitchPreview}` sets it.
#[test]
fn the_view_menu_the_tab_and_the_control_channel_reach_the_stitch_view() {
    use crate::menus::{Item, checked, menu_tree};
    let tree = menu_tree();
    let view = &tree.iter().find(|(name, _)| *name == "View").unwrap().1;
    let ids: Vec<&str> = view.iter().filter_map(|i| if let Item::Cmd(_, id, _) = i { Some(*id) } else { None }).collect();
    let at = ids.iter().position(|id| *id == "view.pixelPreview").unwrap();
    assert_eq!(ids.get(at + 1), Some(&"view.stitchPreview"), "{ids:?}");
    let options = view
        .iter()
        .find_map(|i| match i {
            Item::Sub("Stitch Preview Options", items) => Some(items),
            _ => None,
        })
        .unwrap();
    let options: Vec<&str> = options.iter().filter_map(|i| if let Item::Cmd(_, id, _) = i { Some(*id) } else { None }).collect();
    assert_eq!(options, ["view.stitchPreview.jumps", "view.stitchPreview.needlePoints", "view.stitchPreview.artwork", "view.checkMarks"]);
    let sc = crate::shortcuts::all_shortcuts().into_iter().find(|(_, id, _)| *id == "view.stitchPreview").map(|(sc, ..)| sc);
    assert_eq!(sc, crate::shortcuts::parse("Cmd+E"));

    let (mut app, tx) = controlled();
    app.run("file.new", json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 60})).unwrap();
    let none = Value::Null;
    assert_eq!(checked(&app, "view.stitchPreview", &none), Some(false));
    assert_eq!(checked(&app, "view.stitchPreview.jumps", &none), Some(true));
    let title = |app: &VectorcraftApp| {
        let d = app.session.active().unwrap();
        crate::chrome::tab_title(d, 1.0, app.ui.view.outline, crate::embroidery::menu::shows_stitches(app, d))
    };
    assert!(title(&app).ends_with("(RGB/Preview)"), "{}", title(&app));
    assert_eq!(request(&mut app, &tx, "ui.set", json!({"stitchPreview": true}))["ok"], true);
    assert_eq!(checked(&app, "view.stitchPreview", &none), Some(true));
    assert!(title(&app).ends_with("(RGB/Stitches)"), "{}", title(&app));
    app.run("view.stitchPreview.artwork", json!({})).unwrap();
    assert_eq!(checked(&app, "view.stitchPreview.artwork", &none), Some(true));
    // With Outline on too, the stitches are still what the canvas shows.
    app.run("view.outline", json!({})).unwrap();
    assert!(title(&app).ends_with("(RGB/Stitches)"), "{}", title(&app));
    assert_eq!(request(&mut app, &tx, "ui.set", json!({"stitchPreview": false}))["ok"], true);
    assert!(title(&app).ends_with("(RGB/Outline)"), "{}", title(&app));
}

/// One headless frame at `time`, as the app runs one: the logic, the canvas and what is drawn over
/// it, and the preview's scheduling at its end.
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
        crate::embroidery::stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
}

/// `ui.inspect {stitchPreview}` says how the active document's stitch view stands; a window
/// capture waits while its plan is waiting or under way; the canvas's notice says it is planning
/// once the stitches shown have been out of date a while, and why a design can't be planned.
#[test]
fn inspect_screenshots_and_the_notice_follow_the_plan() {
    let (mut app, tx) = controlled();
    let ctx = egui::Context::default();
    let inspect = |app: &mut VectorcraftApp| request(app, &tx, "ui.inspect", json!({}))["result"]["stitchPreview"].clone();
    assert_eq!(inspect(&mut app), Value::Null, "no document");
    app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 100, "height": 100})).unwrap();
    frame(&mut app, &ctx, 0.0);
    let v = inspect(&mut app);
    assert_eq!((&v["state"], &v["on"], &v["drawn"]), (&json!("off"), &json!(false), &json!(false)), "{v}");
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    for t in [0.1, 0.2] {
        frame(&mut app, &ctx, t);
    }
    let v = inspect(&mut app);
    assert_eq!((&v["state"], &v["drawn"], &v["notice"]), (&json!("fresh"), &json!(true), &Value::Null), "{v}");
    assert!(v["generation"].as_u64().is_some() && v["planMs"].as_f64().is_some(), "{v}");
    let uid = app.session.active().unwrap().uid;
    assert!(!crate::embroidery::holds_screenshots(&app, 0.2));
    // A heavy design after an edit: stale while it waits for the quiet time, the capture waits.
    app.embroidery.preview.set_plan_ms(uid, 1000.0);
    app.run("object.move", json!({"dx": 2, "dy": 0})).unwrap();
    frame(&mut app, &ctx, 1.0);
    assert!(crate::embroidery::holds_screenshots(&app, 1.0));
    assert_eq!(inspect(&mut app)["state"], "stale");
    assert_eq!(crate::embroidery::notice::text(&app, 1.0), None, "no notice at once");
    let notice = crate::embroidery::notice::text(&app, 1.5).map(|(t, _)| t);
    assert_eq!(notice.as_deref(), Some("Planning stitches…"));
    frame(&mut app, &ctx, 1.2);
    frame(&mut app, &ctx, 1.3);
    assert!(!crate::embroidery::holds_screenshots(&app, 1.3), "planned once quiet");
    assert_eq!(inspect(&mut app)["state"], "fresh");
    // Three metres a side: more stitches than one shape may have.
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    for t in [2.0, 2.2, 2.4] {
        frame(&mut app, &ctx, t);
    }
    let v = inspect(&mut app);
    assert_eq!(v["state"], "failed", "{v}");
    assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(v["notice"].as_str().is_some_and(|n| n.starts_with("Can't plan the stitches: ")), "{v}");
    assert!(!crate::embroidery::holds_screenshots(&app, 2.4), "a failure doesn't hold a capture");
}

/// The fork's UI commands and control keys answer every junk parameter, with and without a
/// document, without a panic, as upstream's own UI commands are swept; the app draws a frame after.
#[test]
fn the_embroidery_ui_commands_and_control_keys_answer_junk_without_a_panic() {
    use vectorcraft_testkit::strategies::{junk_params, junk_values};
    let ids = ["ui.embroideryPanel", "view.zoomTo", "view.checkMarks", "embroidery.nextHit", "embroidery.previousHit"];
    for with_document in [false, true] {
        let (mut app, tx) = controlled();
        if with_document {
            app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
            app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40})).unwrap();
            app.run("view.stitchPreview", json!({"on": true})).unwrap();
        }
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
        for id in ids {
            let doc = crate::menus::UI_COMMANDS.iter().find(|c| c.0 == id).unwrap_or_else(|| panic!("{id} is a UI command")).3;
            let params = junk_params(doc, "/nonexistent");
            assert!(params.len() >= 5, "{id}");
            for p in params.into_iter().chain(junk_values()) {
                let _ = app.run(id, p);
            }
        }
        for method in ["ui.set", "ui.render"] {
            for v in junk_values() {
                let r = request(&mut app, &tx, method, json!({ "stitchPreview": v }));
                assert!(r.get("ok").is_some(), "{method} {v}: {r}");
            }
        }
        // The app's fonts: with no document the canvas draws its empty state's heading.
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        frame(&mut app, &ctx, 1.0);
    }
}
