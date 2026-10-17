//! Embroidery through the commands: `document.export` stitches the art into DST, EXP, PES and
//! JEF, `document.open` reads a design back as art, the options change the plan, and the format
//! table lists all four.

use serde_json::{Value, json};
use vectorcraft_stitch::format::Format;

use super::*;

fn b64(bytes: &[u8]) -> String {
    vectorcraft_format::base64_encode(bytes)
}

/// A red 40 × 30 rectangle and a blue 30 mm ellipse beside it.
fn art() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#cc2222"})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 40, "height": 30})).unwrap();
    // The new rectangle is selected: without this the next fill would recolour it.
    s.execute("select.none", &json!({})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#2233cc"})).unwrap();
    s.execute("shape.ellipse", &json!({"x": 90, "y": 30, "width": 30, "height": 30})).unwrap();
    s
}

fn export(s: &mut Session, format: &str, extra: Value) -> Vec<u8> {
    let r = s.execute("document.export", &params(format, extra)).expect("exports");
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().expect("dataBase64")).expect("base64")
}

fn params(format: &str, extra: Value) -> Value {
    let mut p = json!({"format": format});
    if let (Some(p), Value::Object(e)) = (p.as_object_mut(), extra) {
        p.extend(e);
    }
    p
}

fn open(s: &mut Session, name: &str, bytes: &[u8]) -> crate::Result<Value> {
    s.execute("document.open", &json!({"name": name, "dataBase64": b64(bytes)}))
}

fn warnings(r: &Value) -> Vec<String> {
    r["warnings"].as_array().map(|w| w.iter().filter_map(|w| w.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The pattern a written file holds.
fn plan(bytes: &[u8]) -> vectorcraft_stitch::Pattern {
    Format::Dst.read(bytes).expect("a DST we wrote reads")
}

#[test]
fn export_writes_a_dst_of_the_art() {
    let mut s = art();
    let bytes = export(&mut s, "dst", json!({}));
    assert!(bytes.len() > 512 + 200, "{} bytes", bytes.len());
    let p = plan(&bytes);
    assert_eq!(p.blocks.len(), 2, "one block per thread");
    assert!(p.sewn() > 200, "{} stitches", p.sewn());
    // Millimetres: the art spans 100 × 40 pt, which is 35.3 × 14.1 mm.
    let b = p.bounds().expect("bounds");
    assert!((b.width() - 35.3).abs() < 1.5, "{b:?}");
    assert!((b.height() - 14.1).abs() < 1.5, "{b:?}");
    // DST carries no colours: the blocks get the default palette's, one each, in order.
    assert_ne!(p.blocks[0].thread.color, p.blocks[1].thread.color);
    assert_eq!(p.blocks[0].thread.color, vectorcraft_stitch::thread::default_thread(0).color);
}

#[test]
fn every_format_writes_and_reads_back() {
    let mut s = art();
    for format in Format::ALL {
        let bytes = export(&mut s, format.id(), json!({}));
        let p = format.read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", format.id()));
        assert!(p.sewn() > 100, "{}: {} stitches", format.id(), p.sewn());
        assert_eq!(p.blocks.len(), 2, "{}: colour blocks", format.id());
        assert!(p.points().all(|s| s.x.is_finite() && s.y.is_finite()), "{}", format.id());
    }
}

#[test]
fn options_change_the_stitch_plan() {
    let mut s = art();
    let sparse = plan(&export(&mut s, "dst", json!({"density": 0.8})));
    let dense = plan(&export(&mut s, "dst", json!({"density": 0.4})));
    assert!(dense.sewn() > sparse.sewn() + 50, "dense {} sparse {}", dense.sewn(), sparse.sewn());

    let running = plan(&export(&mut s, "dst", json!({"stitchStyle": "running"})));
    assert!(running.sewn() < sparse.sewn(), "running {} sparse {}", running.sewn(), sparse.sewn());

    // Satin without a width (fills) falls back to filling.
    let satin = plan(&export(&mut s, "dst", json!({"stitchStyle": "satin"})));
    assert!(satin.sewn() > 50);

    let long = plan(&export(&mut s, "dst", json!({"stitchLength": 8.0})));
    assert!(long.sewn() < dense.sewn());
}

#[test]
fn a_written_pes_opens_as_art() {
    let mut s = art();
    let bytes = export(&mut s, "pes", json!({}));
    let r = open(&mut s, "design.pes", &bytes).expect("opens");
    assert_eq!(r["format"].as_str(), Some("pes"));
    let st = s.doc().unwrap();
    let groups: Vec<String> = st.doc.layers.iter().flat_map(|l| l.children().into_iter().flatten()).filter_map(|c| c.name.clone()).collect();
    assert_eq!(groups.len(), 2, "{groups:?}");
    let mut leaves = 0;
    st.doc.walk(|n| {
        if n.children().is_none() {
            leaves += 1;
        }
    });
    assert!(leaves > 2, "{leaves} paths");
    let board = st.doc.artboards[0].rect;
    assert!(board.width() > 0.0 && board.height() > 0.0, "{board:?}");
}

#[test]
fn a_pes_opens_by_its_magic_bytes() {
    let mut s = art();
    let bytes = export(&mut s, "pes", json!({}));
    let r = open(&mut s, "design.bin", &bytes).expect("opens by content");
    assert_eq!(r["format"].as_str(), Some("pes"));
}

#[test]
fn a_stitched_design_reads_back_and_sews_again() {
    let mut s = art();
    let first = plan(&export(&mut s, "dst", json!({})));
    let bytes = export(&mut s, "dst", json!({}));
    open(&mut s, "design.dst", &bytes).expect("opens");
    // The design is art now: thin strokes, sewn along their centre lines.
    let again = plan(&export(&mut s, "dst", json!({})));
    assert!(again.sewn() > 100, "{} stitches again", again.sewn());
    assert_eq!(again.blocks.len(), first.blocks.len());
    let a = first.bounds().expect("bounds");
    let b = again.bounds().expect("bounds");
    assert!((a.width() - b.width()).abs() < 3.0, "{a:?} {b:?}");
    assert!((a.height() - b.height()).abs() < 3.0, "{a:?} {b:?}");
}

#[test]
fn the_format_table_lists_embroidery() {
    let mut s = Session::new();
    let r = s.execute("document.formats", &json!({})).unwrap();
    let formats = r["formats"].as_array().expect("formats");
    for id in ["dst", "exp", "pes", "jef"] {
        let f = formats.iter().find(|f| f["id"] == id).unwrap_or_else(|| panic!("{id} is listed"));
        assert_eq!(f["read"].as_bool(), Some(true), "{id}");
        assert_eq!(f["write"].as_bool(), Some(true), "{id}");
    }
}

#[test]
fn nothing_to_stitch_is_an_error() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 100, "height": 100, "artboards": 1})).unwrap();
    let e = s.execute("document.export", &json!({"format": "dst"})).unwrap_err().to_string();
    assert!(e.contains("nothing to stitch"), "{e}");
}

#[test]
fn a_bad_style_is_refused() {
    let mut s = art();
    let e = s.execute("document.export", &json!({"format": "pes", "stitchStyle": "cross"})).unwrap_err().to_string();
    assert!(e.contains("stitchStyle"), "{e}");
}

#[test]
fn type_is_left_out_with_a_warning() {
    let mut s = art();
    s.execute("text.create", &json!({"x": 10, "y": 150, "text": "Hafciarstwo"})).unwrap();
    let r = s.execute("document.export", &params("dst", json!({}))).expect("exports");
    assert!(r["dataBase64"].is_string());
    let w = warnings(&r);
    assert!(w.iter().any(|w| w.contains("type")), "{w:?}");
}
