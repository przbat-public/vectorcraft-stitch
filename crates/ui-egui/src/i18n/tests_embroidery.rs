//! The fork's messages (machine embroidery) in every language: the export's warnings and errors,
//! and what opening a machine file says it left out, whose text the message scan in `tests` doesn't
//! read (they are constants, or joined as they are said).

use super::*;

/// The embroidery export's warnings, and its error when nothing is left to stitch, whose text the
/// scan above doesn't read (they are constants, or joined as the export runs), are translated whole
/// in every language.
#[test]
fn embroidery_export_messages_are_translated() {
    use serde_json::json;
    use vectorcraft_doc::embroidery::{EmbroiderySetup, ThreadPalette};
    use vectorcraft_engine::{EngineError, Session};
    let export = |s: &mut Session, expand: f64| s.execute("document.export", &json!({"format": "dst", "expand": expand}));
    // Nothing to stitch is the document's error, not the parameters'.
    let error = |e: EngineError| match e {
        EngineError::Other(msg) => msg,
        e => panic!("{e}"),
    };
    let rectangle = |s: &mut Session, x: u64, y: u64, width: u64, height: u64| {
        s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": width, "height": height})).unwrap()["id"].clone()
    };
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let mut messages = vec![error(export(&mut s, 0.0).unwrap_err())];
    s.execute("text.create", &json!({"x": 10, "y": 300, "text": "Stitch"})).unwrap();
    messages.push(error(export(&mut s, 0.0).unwrap_err()));
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#cc2222"})).unwrap();
    rectangle(&mut s, 20, 20, 100, 60);
    // A sliver that a negative expand leaves nothing of, a picture, a blend and a gradient.
    rectangle(&mut s, 20, 100, 100, 2);
    let picture = rectangle(&mut s, 150, 20, 30, 30);
    s.execute("select.set", &json!({"ids": [picture]})).unwrap();
    s.execute("object.rasterize", &json!({})).unwrap();
    let keys = [rectangle(&mut s, 20, 150, 10, 10), rectangle(&mut s, 120, 150, 10, 10)];
    s.execute("object.blend.make", &json!({"ids": keys, "steps": 1})).unwrap();
    let gradient = rectangle(&mut s, 150, 100, 30, 30);
    s.execute("select.set", &json!({"ids": [gradient]})).unwrap();
    s.execute("paint.setFill", &json!({"gradient": {}})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    // An intertwine.
    let pair = [rectangle(&mut s, 200, 150, 20, 20), rectangle(&mut s, 210, 155, 20, 20)];
    s.execute("object.intertwine.make", &json!({"ids": pair})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    // A stroke too wide for a satin column (160 pt is 56 mm).
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#2233cc"})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 400, "x2": 280, "y2": 400})).unwrap();
    s.execute("stroke.set", &json!({"weight": 160})).unwrap();
    // A thread palette this version doesn't know.
    s.edit("Embroidery Setup", |d, _| {
        let palette = Some(ThreadPalette::Unknown(json!("madeira")));
        d.embroidery = Some(Box::new(EmbroiderySetup { palette, ..EmbroiderySetup::default() }));
        Ok(())
    })
    .unwrap();
    let r = export(&mut s, -1.0).unwrap();
    let warnings: Vec<String> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_string)).collect();
    assert_eq!(warnings.len(), 8, "{warnings:#?}");
    messages.extend(warnings);
    let english = ["stitch", "sewn", "painted", "this version", "expand them", "trace them", "outline it"];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for m in &messages {
            let shown = message(lang, m);
            let left = english.iter().find(|w| shown.replace("stitch fill", "").contains(**w));
            if shown == *m || left.is_some() {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated embroidery messages:\n{}", untranslated.join("\n"));
}

/// What opening a machine file says it left out (which makes writing over the file ask first) is
/// translated whole in every language.
#[test]
fn embroidery_open_losses_are_translated() {
    // Phrases of the English notes no other language shares.
    let english = ["kept", "written as", "are read", "doesn't read", "this document", "nearest thread", "stitches"];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for note in vectorcraft_engine::EMBROIDERY_LOSS_NOTES {
            let shown = message(lang, note);
            if shown == note || english.iter().any(|w| shown.contains(*w)) {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated notes:\n{}", untranslated.join("\n"));
}
