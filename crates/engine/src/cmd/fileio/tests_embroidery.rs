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
    // At least a run of stitches per shape (each is sewn without a cut now, travelling inside).
    assert!(leaves >= 2, "{leaves} paths");
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

/// Each block's needle points (where the needle goes down) and the trims, as a reader decodes
/// them: what a machine sews.
fn sewn(p: &vectorcraft_stitch::Pattern) -> Vec<(Vec<(i32, i32)>, usize)> {
    use vectorcraft_stitch::{Command, quantize};
    p.blocks
        .iter()
        .map(|b| {
            let points = b.stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (quantize(s.x), quantize(s.y))).collect();
            (points, b.stitches.iter().filter(|s| s.command == Command::Trim).count())
        })
        .collect()
}

/// A design opened from a machine file and exported again is the same design: every needle
/// point, trim and colour block where the file had it, in every format.
#[test]
fn an_opened_design_is_written_back_unchanged() {
    for format in Format::ALL {
        let mut s = art();
        // Two shapes of each thread, far apart, so the blocks hold trims and jumps.
        rect(&mut s, "#cc2222", 20.0, 120.0, 30.0, 20.0);
        let first = export(&mut s, format.id(), json!({}));
        open(&mut s, &format!("design.{}", format.id()), &first).expect("opens");
        let again = export(&mut s, format.id(), json!({}));
        let (a, b) = (format.read(&first).expect("reads"), format.read(&again).expect("reads"));
        assert_eq!(sewn(&a), sewn(&b), "{}", format.id());
        // Opened again and written again: still the same.
        open(&mut s, &format!("again.{}", format.id()), &again).expect("opens");
        assert_eq!(sewn(&format.read(&export(&mut s, format.id(), json!({}))).expect("reads")), sewn(&a), "{}", format.id());
    }
}

/// An opened design written to another format keeps its needle points too (as far as the target
/// format can say the same thing).
#[test]
fn an_opened_design_converts_between_formats() {
    let mut s = art();
    let pes = export(&mut s, "pes", json!({}));
    open(&mut s, "design.pes", &pes).expect("opens");
    let jef = export(&mut s, "jef", json!({}));
    let points = |p: &vectorcraft_stitch::Pattern| sewn(p).into_iter().map(|(pts, _)| pts).collect::<Vec<_>>();
    assert_eq!(points(&Format::Pes.read(&pes).expect("reads")), points(&Format::Jef.read(&jef).expect("reads")));
}

/// Moving an opened design moves its stitches; its style options don't re-stitch it.
#[test]
fn an_opened_design_is_not_restitched_by_the_options() {
    let mut s = art();
    let dst = export(&mut s, "dst", json!({}));
    open(&mut s, "design.dst", &dst).expect("opens");
    let plain = plan(&export(&mut s, "dst", json!({})));
    let forced = plan(&export(&mut s, "dst", json!({"stitchStyle": "satin", "density": 1.0, "stitchLength": 8.0})));
    assert_eq!(sewn(&plain), sewn(&forced));
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

/// A filled rectangle in `color`, left unselected.
fn rect(s: &mut Session, color: &str, x: f64, y: f64, w: f64, h: f64) {
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": color})).unwrap();
    s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": w, "height": h})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
}

fn blank() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    s
}

/// Thread sewn later lies on top: a red object above a blue one is sewn after it, even though
/// another red object under the blue one could take it along in the same pass.
#[test]
fn stacking_order_survives_grouping_by_thread() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 60.0);
    rect(&mut s, "#2233cc", 50.0, 50.0, 60.0, 60.0);
    rect(&mut s, "#cc2222", 90.0, 90.0, 30.0, 30.0);
    let p = plan(&export(&mut s, "dst", json!({})));
    assert_eq!(p.blocks.len(), 3, "red, blue, then the red on top");
    // The last block is the top red square (30 pt ≈ 10.6 mm), not the big one.
    let top = p.blocks[2].bounds().expect("bounds");
    assert!(top.width() < 12.0 && top.height() < 12.0, "{top:?}");

    // Apart, the two reds share a pass: one colour change instead of two.
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 40.0);
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 40.0);
    rect(&mut s, "#cc2222", 20.0, 120.0, 40.0, 40.0);
    assert_eq!(plan(&export(&mut s, "dst", json!({}))).blocks.len(), 2);
}

/// A clip group sews what it shows: the clipped art only inside the clipping path, which itself
/// paints nothing here.
#[test]
fn a_clipping_mask_cuts_the_stitches() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 0.0, 0.0, 200.0, 200.0);
    // The clipping path, on top: an unpainted 50 pt square.
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 50, "y": 50, "width": 50, "height": 50})).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    s.execute("object.clippingMask.make", &json!({})).unwrap();
    let p = plan(&export(&mut s, "dst", json!({})));
    assert_eq!(p.blocks.len(), 1);
    let b = p.bounds().expect("bounds");
    // 50 pt is 17.6 mm: the 70 mm square is cut to it.
    assert!(b.width() < 18.5 && b.height() < 18.5, "{b:?}");
    assert!(b.width() > 16.0 && b.height() > 16.0, "{b:?}");
}

/// Template layers are references to trace over, not art: they are not sewn.
#[test]
fn template_layers_are_not_stitched() {
    let mut s = blank();
    let template = s.doc().unwrap().doc.layers[0].id.0;
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 40.0);
    s.execute("layer.new", &json!({})).unwrap();
    rect(&mut s, "#2233cc", 100.0, 20.0, 40.0, 40.0);
    s.execute("layer.setProps", &json!({"id": template, "template": true})).unwrap();
    let p = plan(&export(&mut s, "dst", json!({})));
    assert_eq!(p.blocks.len(), 1, "only the blue square");
}

/// A stroke that runs out of the clipping path is sewn only where it shows, as a running line or
/// a satin column alike.
#[test]
fn a_clipping_mask_cuts_strokes() {
    for weight in [1.0, 6.0] {
        let mut s = blank();
        s.execute("paint.setFill", &json!({"none": true})).unwrap();
        s.execute("paint.setStroke", &json!({"color": "#cc2222", "width": weight})).unwrap();
        s.execute("shape.line", &json!({"x1": 0, "y1": 75, "x2": 250, "y2": 75})).unwrap();
        s.execute("select.none", &json!({})).unwrap();
        s.execute("paint.setStroke", &json!({"none": true})).unwrap();
        s.execute("shape.rectangle", &json!({"x": 50, "y": 50, "width": 50, "height": 50})).unwrap();
        s.execute("select.all", &json!({})).unwrap();
        s.execute("object.clippingMask.make", &json!({})).unwrap();
        let p = plan(&export(&mut s, "dst", json!({})));
        let b = p.bounds().expect("bounds");
        // The 88 mm line is cut to the 17.6 mm square, to the stitch: a running line keeps the
        // needle points inside, so it can stop up to a stitch (3.5 mm) short at each edge.
        assert!(b.width() < 18.0, "{weight} pt: {b:?}");
        assert!(b.width() > 17.6 - 2.0 * 3.5, "{weight} pt: {b:?}");
    }
}

/// Options that would make millions of stitches are refused with a message, not a huge file or an
/// exhausted memory.
#[test]
fn an_absurd_density_is_refused() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 14000, "height": 14000, "artboards": 1})).unwrap();
    rect(&mut s, "#cc2222", 0.0, 0.0, 14000.0, 14000.0);
    let e = s.execute("document.export", &params("dst", json!({"density": 0.05, "stitchLength": 0.2}))).unwrap_err().to_string();
    assert!(e.contains("stitches"), "{e}");
}

/// `stitchStyle: fill` on a stroke fills the area the stroke paints (a ring for a stroked
/// circle), not the inside of the path it follows.
#[test]
fn a_forced_fill_fills_the_stroke_not_the_path() {
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222", "width": 6})).unwrap();
    s.execute("shape.ellipse", &json!({"x": 50, "y": 50, "width": 100, "height": 100})).unwrap();
    let p = plan(&export(&mut s, "dst", json!({"stitchStyle": "fill"})));
    // The ring is 6 pt (2.1 mm) wide round a 35.3 mm circle: no needle point near the centre.
    let b = p.bounds().expect("bounds");
    let (cx, cy) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
    let r = 35.3 / 2.0;
    for st in p.points().filter(|s| s.command == vectorcraft_stitch::Command::Stitch) {
        let d = ((st.x - cx).powi(2) + (st.y - cy).powi(2)).sqrt();
        assert!((d - r).abs() < 1.4, "a stitch {d:.1} mm from the centre");
    }
}

/// An opened design saved as a VectorCraft document and opened again still writes back the file's
/// stitches: the native format keeps which lines are literal stitches.
#[test]
fn literal_stitches_survive_the_native_format() {
    let mut s = art();
    let dst = export(&mut s, "dst", json!({}));
    open(&mut s, "design.dst", &dst).expect("opens");
    let saved = s.execute("document.serialize", &json!({})).expect("serializes");
    let native = match saved["dataBase64"].as_str() {
        Some(b) => vectorcraft_format::base64_decode(b).expect("base64"),
        None => saved["text"].as_str().expect("text").as_bytes().to_vec(),
    };
    open(&mut s, "design.vectorcraft", &native).expect("opens the native file");
    assert_eq!(sewn(&plan(&export(&mut s, "dst", json!({})))), sewn(&plan(&dst)));
}

/// A file that cuts the thread at the end of a colour block (before the change) and at the start
/// of the next, as Ink/Stitch writes them, is written back with those cuts.
#[test]
fn cuts_at_a_blocks_ends_are_written_back() {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    let mut p = Pattern::default();
    let mut a = Block::new(Thread::rgb([200, 0, 0]));
    for i in 0..10 {
        a.stitches.push(Stitch::new(f64::from(i) * 2.0, 0.0, Command::Stitch));
    }
    a.stitches.push(Stitch::new(18.0, 0.0, Command::Trim));
    let mut b = Block::new(Thread::rgb([0, 0, 200]));
    b.stitches.push(Stitch::new(0.0, 10.0, Command::Trim));
    for i in 0..10 {
        b.stitches.push(Stitch::new(f64::from(i) * 2.0, 10.0, Command::Stitch));
    }
    p.blocks.extend([a, b]);
    for format in [Format::Pes, Format::Jef, Format::Exp] {
        let file = format.write(&p).unwrap();
        let mut s = Session::new();
        open(&mut s, &format!("cuts.{}", format.id()), &file).expect("opens");
        let again = export(&mut s, format.id(), json!({}));
        assert_eq!(sewn(&format.read(&again).unwrap()), sewn(&format.read(&file).unwrap()), "{}", format.id());
    }
}
