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
