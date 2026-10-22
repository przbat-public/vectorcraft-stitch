//! Embroidery through the commands: `document.export` stitches the art into DST, TAP, T01, EXP,
//! PES, PEC, JEF and VP3, `document.open` reads a design back as art, the options change the plan, and the
//! format table lists them all.

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

/// A PEC file is a PES's PEC section after its own magic: it opens by that, in Brother's palette.
#[test]
fn a_pec_is_the_pes_machine_part_and_opens_by_its_magic_bytes() {
    let mut s = art();
    let (pes, pec) = (export(&mut s, "pes", json!({})), export(&mut s, "pec", json!({})));
    assert_eq!(&pec[8..], &pes[22..], "the PEC section, after #PEC0001 instead of the PES header");
    let r = open(&mut s, "design.bin", &pec).expect("opens by content");
    assert_eq!(r["format"].as_str(), Some("pec"));
    let palette = s.doc().unwrap().doc.embroidery.as_deref().and_then(|e| e.palette.as_ref()).and_then(|p| p.id());
    assert_eq!(palette, Some("pec"));
}

/// A T01 is a TAP under Wilcom's name: the same bytes, a DST's records without its header.
#[test]
fn a_t01_is_a_tap_and_a_dst_without_its_header() {
    let mut s = art();
    let (dst, tap, t01) = (export(&mut s, "dst", json!({})), export(&mut s, "tap", json!({})), export(&mut s, "t01", json!({})));
    assert_eq!(t01, tap);
    assert_eq!(&t01[..], &dst[512..]);
    let r = open(&mut s, "design.t01", &t01).expect("opens");
    assert_eq!(r["format"].as_str(), Some("t01"));
}

/// A VP3 says what it is in its first bytes too, and carries each block's own colour: the art's,
/// which it opens in again.
#[test]
fn a_vp3_keeps_the_arts_colours_and_opens_by_its_magic_bytes() {
    let mut s = art();
    let bytes = export(&mut s, "vp3", json!({}));
    let colors: Vec<[u8; 3]> = Format::Vp3.read(&bytes).expect("reads").blocks.iter().map(|b| b.thread.color).collect();
    assert_eq!(colors, [[0xcc, 0x22, 0x22], [0x22, 0x33, 0xcc]]);
    let r = open(&mut s, "design.bin", &bytes).expect("opens by content");
    assert_eq!(r["format"].as_str(), Some("vp3"));
    let mut strokes: Vec<String> = Vec::new();
    s.doc().unwrap().doc.walk(|n| {
        if n.children().is_none()
            && let Some(st) = n.appearance.stroke()
        {
            strokes.push(format!("{:?}", st.paint));
        }
    });
    let red = format!("{:?}", vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(0xcc, 0x22, 0x22)));
    assert!(strokes.contains(&red), "{strokes:?}");
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

/// Each block's thread as the file gives it: its colour, its name (a palette thread's for the
/// formats that number them) and its record (VP3's).
fn threads(p: &vectorcraft_stitch::Pattern) -> Vec<([u8; 3], String, Option<vectorcraft_stitch::ThreadRecord>)> {
    p.blocks.iter().map(|b| (b.thread.color, b.thread.name.clone(), b.thread.record.clone())).collect()
}

/// A design opened from a machine file and exported again is the same design: every needle
/// point, trim and colour block where the file had it, each block's thread (and VP3's thread
/// record), the design's name and where the needle starts, in every format.
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
        assert_eq!(start(&a), start(&b), "{}: the needle starts where it did", format.id());
        assert_eq!(threads(&a), threads(&b), "{}", format.id());
        assert_eq!(a.name, b.name, "{}: the design's name", format.id());
        // Opened again and written again: still the same.
        open(&mut s, &format!("again.{}", format.id()), &again).expect("opens");
        assert_eq!(sewn(&format.read(&export(&mut s, format.id(), json!({}))).expect("reads")), sewn(&a), "{}", format.id());
    }
}

/// A VP3 opened and exported again writes each block's thread record back as the file had it
/// (catalogue number, description, brand, material and weight); a DST's label is written again,
/// not the file's name, and `embroidery.setup {label}` changes it.
#[test]
fn an_opened_designs_thread_records_and_label_are_written_back() {
    let mut s = art();
    let mut p = Format::Vp3.read(&export(&mut s, "vp3", json!({}))).expect("reads");
    let records = [("2296", "Pink", "Robison-Anton", 5, 40), ("1801", "Royal", "Madeira", 7, 60)];
    for (b, (catalogue, description, brand, material, weight)) in p.blocks.iter_mut().zip(records) {
        b.thread.record = Some(vectorcraft_stitch::ThreadRecord {
            catalogue: catalogue.into(),
            description: description.into(),
            brand: brand.into(),
            material,
            weight,
        });
        // Read back, a thread is named by its description.
        b.thread.name = description.into();
    }
    let vp3 = Format::Vp3.write(&p).expect("writes");
    open(&mut s, "flower.vp3", &vp3).expect("opens");
    let back = Format::Vp3.read(&export(&mut s, "vp3", json!({}))).expect("reads");
    assert_eq!(threads(&back), threads(&p));
    p.name = "ACME_01".into();
    let dst = Format::Dst.write(&p).expect("writes");
    open(&mut s, "my-logo.dst", &dst).expect("opens");
    assert_eq!(s.execute("embroidery.settings", &json!({})).unwrap()["settings"]["label"], "ACME_01");
    for format in [Format::Dst, Format::Pes, Format::Pec] {
        assert_eq!(format.read(&export(&mut s, format.id(), json!({}))).expect("reads").name, "ACME_01", "{}", format.id());
    }
    s.execute("embroidery.setup", &json!({"label": "Logo 2"})).unwrap();
    assert_eq!(Format::Dst.read(&export(&mut s, "dst", json!({}))).expect("reads").name, "Logo 2");
    s.execute("embroidery.setup", &json!({"label": null})).unwrap();
    assert_eq!(Format::Dst.read(&export(&mut s, "dst", json!({}))).expect("reads").name, "my-logo.dst", "the document's title");
    assert!(s.execute("embroidery.setup", &json!({"label": 5})).is_err());
}

/// Opening a machine file says what reading it left out (a PES's own section, PE-Design's), and
/// writing over that file is then refused unless the loss is acknowledged; a file this app wrote
/// leaves nothing out and is written over without asking.
#[test]
fn writing_over_a_file_whose_opening_left_something_out_asks_first() {
    let d = std::env::temp_dir().join(format!("vc-embroidery-loss-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let mut s = art();
    let pes = export(&mut s, "pes", json!({}));
    // A PES section before the PEC part, as PE-Design's files have one: the header points past it.
    let mut sectioned = pes[..8].to_vec();
    let section = b"CEmbOne and its stitches, as PE-Design keeps them";
    sectioned.extend_from_slice(&u32::try_from(12 + section.len()).unwrap().to_le_bytes());
    sectioned.extend_from_slice(section);
    sectioned.extend_from_slice(&pes[22..]);
    for (name, file, lost) in [("ours.pes", &pes, false), ("theirs.pes", &sectioned, true)] {
        let path = d.join(name);
        std::fs::write(&path, file).unwrap();
        let path = path.to_string_lossy().to_string();
        let mut s = Session::new();
        let r = s.execute("document.open", &json!({"path": path})).unwrap();
        let said = warnings(&r).contains(&super::embroidery::LOSS_NOTES[0].to_string());
        assert_eq!(said, lost, "{name}: {r}");
        let over = s.execute("document.export", &json!({"path": path}));
        if lost {
            let e = over.unwrap_err().to_string();
            assert!(e.contains("left things out") && e.contains("PE-Design"), "{name}: {e}");
            assert_eq!(&std::fs::read(&path).unwrap(), file, "{name}: untouched");
            s.execute("document.export", &json!({"path": path, "acknowledgeLoss": true})).unwrap();
            assert_ne!(&std::fs::read(&path).unwrap(), file, "{name}");
        } else {
            over.unwrap();
        }
    }
    std::fs::remove_dir_all(d).ok();
}

/// Where the needle starts, on the 0.1 mm grid from the design's top-left corner.
fn start(p: &vectorcraft_stitch::Pattern) -> (i32, i32) {
    use vectorcraft_stitch::quantize;
    let (s, b) = (p.start.expect("read from a file"), p.bounds().expect("has stitches"));
    (quantize(s.x - b.x0), quantize(s.y - b.y0))
}

/// A design opened from a file that starts at its centre, as commercial DST files often do, is
/// written again from there: the DST, TAP, T01, EXP, PES and PEC exports start where the file
/// started (JEF and VP3 start every design at its centre). With the start unset
/// (`embroidery.setup {start: null}`), they start at the design's corner again.
#[test]
fn an_opened_design_starts_where_its_file_started() {
    use vectorcraft_stitch::quantize;
    let mut s = art();
    let mut p = Format::Dst.read(&export(&mut s, "dst", json!({}))).expect("reads");
    p.start = p.bounds().map(|b| b.center());
    let centred = Format::Dst.write(&p).expect("writes");
    open(&mut s, "centred.dst", &centred).expect("opens");
    assert!(s.execute("embroidery.settings", &json!({})).unwrap()["settings"]["start"].is_array());
    for format in Format::ALL {
        let back = format.read(&export(&mut s, format.id(), json!({}))).expect("reads");
        let (at, b) = (start(&back), back.bounds().expect("has stitches"));
        let centre = (quantize(b.width() / 2.0), quantize(b.height() / 2.0));
        assert!((at.0 - centre.0).abs() <= 1 && (at.1 - centre.1).abs() <= 1, "{}: starts at {at:?}, not {centre:?}", format.id());
    }
    s.execute("embroidery.setup", &json!({"start": null})).unwrap();
    let corner = Format::Dst.read(&export(&mut s, "dst", json!({}))).expect("reads");
    assert_eq!(start(&corner), (0, quantize(corner.bounds().expect("has stitches").height())), "the bottom-left corner");
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
    let vp3 = export(&mut s, "vp3", json!({}));
    assert_eq!(points(&Format::Pes.read(&pes).expect("reads")), points(&Format::Vp3.read(&vp3).expect("reads")));
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

/// Every embroidery format is listed wherever formats are, as the stitch crate describes it: in
/// `document.formats` (its label and extensions), the extensions opened, placed and left
/// unassociated, and the hand-written format lists of the export and check commands' params.
#[test]
fn the_format_table_lists_embroidery() {
    let mut s = Session::new();
    let r = s.execute("document.formats", &json!({})).unwrap();
    let formats = r["formats"].as_array().expect("formats");
    let doc = |id: &str| crate::cmd::command_specs().iter().find(|c| c.id == id).map_or("", |c| c.params);
    let listings = ["document.serialize", "document.export", "document.exportSelection"].map(doc);
    for format in Format::ALL {
        let (id, spec) = (format.id(), format.spec());
        let f = formats.iter().find(|f| f["id"] == id).unwrap_or_else(|| panic!("{id} is listed"));
        assert_eq!(f["read"].as_bool(), Some(true), "{id}");
        assert_eq!(f["write"].as_bool(), Some(true), "{id}");
        assert_eq!(f["label"].as_str(), Some(spec.label), "{id}");
        for ext in spec.extensions {
            for (list, name) in [(super::OPEN_EXTS, "opened"), (super::PLACE_EXTS, "placed"), (super::UNASSOCIATED_EXTS, "unassociated")] {
                assert!(list.contains(ext), "{ext} is {name}");
            }
        }
        for listing in &listings {
            assert!([format!("|{id}|"), format!("|{id} ")].iter().any(|word| listing.contains(word.as_str())), "{id} in {listing}");
        }
        assert!(doc("embroidery.check").contains(&format!("\"{id}\"")), "{id} in embroidery.check's formats");
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

/// A fill forced to a running line inside a clip group follows its own outline where it shows: its
/// stitches are cut to the clipping path, as a stroke's are, not its area, whose cut edge would be
/// sewn as a line the art doesn't have.
#[test]
fn a_running_fill_in_a_clip_group_follows_its_own_outline() {
    let mut s = blank();
    // A 100 pt square, its right half shown through a 100 × 200 pt clipping path.
    rect(&mut s, "#cc2222", 50.0, 50.0, 100.0, 100.0);
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 100, "y": 0, "width": 100, "height": 200})).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    s.execute("object.clippingMask.make", &json!({})).unwrap();
    let p = plan(&export(&mut s, "dst", json!({"stitch": "running"})));
    // Needle points lie on the square's outline (left 50, right 150, top 50, bottom 150 pt) within
    // the clip, never along the clip's edge at x = 100 pt inside the square.
    let mm = |pt: f64| pt * 25.4 / 72.0;
    let b = p.bounds().expect("bounds");
    let (dx, dy) = (b.x0, b.y0);
    let on_outline = |x: f64, y: f64| {
        let (x, y) = (x - dx + mm(100.0), y - dy + mm(50.0));
        let near = |a: f64, b: f64| (a - b).abs() < 0.3;
        near(y, mm(50.0)) || near(y, mm(150.0)) || near(x, mm(150.0)) || near(x, mm(100.0)) && (near(y, mm(50.0)) || near(y, mm(150.0)))
    };
    let inside_cut: Vec<(f64, f64)> = p
        .points()
        .filter(|st| st.command == vectorcraft_stitch::Command::Stitch)
        .map(|st| (st.x, st.y))
        .filter(|(x, y)| !on_outline(*x, *y))
        .collect();
    assert!(inside_cut.is_empty(), "needle points off the square's outline: {inside_cut:?}");
    assert!(b.width() < mm(50.0) + 0.5, "only the shown half: {b:?}");
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
        s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
        s.execute("shape.line", &json!({"x1": 0, "y1": 75, "x2": 250, "y2": 75})).unwrap();
        // 1 pt is a running line, 6 pt (2.1 mm) a satin column.
        s.execute("stroke.set", &json!({"weight": weight})).unwrap();
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
    let e = s.execute("document.export", &params("dst", json!({"density": 0.05, "stitchLength": 0.2}))).unwrap_err();
    assert!(e.to_string().contains("stitches"), "{e}");
    // The design's limit, not a wrong parameter: a caller that fixes its parameters on
    // `BadParams` would only loop. The read commands report it the same way.
    assert!(matches!(e, EngineError::Other(_)), "{e:?}");
    let e = s.execute("embroidery.stats", &json!({"spacing": 0.05, "length": 0.2})).unwrap_err();
    assert!(matches!(e, EngineError::Other(_)), "{e:?}");
    // So is a document with nothing to stitch.
    s.execute("select.all", &json!({})).unwrap();
    s.execute("edit.clear", &json!({})).unwrap();
    let e = s.execute("document.export", &params("dst", json!({}))).unwrap_err();
    assert!(matches!(&e, EngineError::Other(m) if m.starts_with("nothing to stitch")), "{e:?}");
}

/// A design a format can't hold (more colours than PES numbers) is the design's limit too.
#[test]
fn more_colours_than_a_format_holds_is_not_a_bad_parameter() {
    let mut s = blank();
    let most = Format::Pes.max_colors().unwrap();
    for k in 0..=most {
        // Colours far enough apart to keep their own blocks: one grey step each.
        let grey = u8::try_from(k % 256).unwrap();
        rect(&mut s, &format!("#{grey:02x}{:02x}{:02x}", 255 - grey, (k / 256) * 128), 2.0 * k as f64, 0.0, 1.5, 1.5);
    }
    let e = s.execute("document.export", &params("pes", json!({}))).unwrap_err();
    assert!(matches!(&e, EngineError::Other(m) if m.contains("colour")), "{e:?}");
}

/// `stitchStyle: fill` on a stroke fills the area the stroke paints (a ring for a stroked
/// circle), not the inside of the path it follows.
#[test]
fn a_forced_fill_fills_the_stroke_not_the_path() {
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
    s.execute("shape.ellipse", &json!({"x": 50, "y": 50, "width": 100, "height": 100})).unwrap();
    s.execute("stroke.set", &json!({"weight": 6})).unwrap();
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

/// A run of two needle points at one spot (a tack between two jumps) is written back as two, and a
/// run of one as one: their lines, from a point to the same point, look alike, and the run says
/// which it is.
#[test]
fn a_tack_and_a_single_stitch_round_trip() {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    let mut a = Block::new(Thread::rgb([200, 0, 0]));
    for i in 0..5 {
        a.stitches.push(Stitch::new(f64::from(i) * 2.0, 0.0, Command::Stitch));
    }
    // A tack: two penetrations in one hole, between two jumps.
    a.stitches.push(Stitch::new(14.0, 0.0, Command::Jump));
    a.stitches.push(Stitch::new(14.0, 0.0, Command::Stitch));
    a.stitches.push(Stitch::new(14.0, 0.0, Command::Stitch));
    // A single stitch, between two jumps.
    a.stitches.push(Stitch::new(20.0, 0.0, Command::Jump));
    a.stitches.push(Stitch::new(20.0, 0.0, Command::Stitch));
    a.stitches.push(Stitch::new(26.0, 0.0, Command::Jump));
    for i in 0..5 {
        a.stitches.push(Stitch::new(26.0 + f64::from(i) * 2.0, 0.0, Command::Stitch));
    }
    let p = Pattern { blocks: vec![a], ..Pattern::default() };
    let file = Format::Dst.write(&p).unwrap();
    let sewn = |p: &Pattern| p.points().filter(|s| s.command == Command::Stitch).count();
    let mut s = Session::new();
    open(&mut s, "tack.dst", &file).expect("opens");
    assert_eq!(sewn(&plan(&export(&mut s, "dst", json!({})))), sewn(&plan(&file)));
    // Saved and opened again, it still says so.
    let saved = export(&mut s, "vectorcraft", json!({}));
    let mut again = Session::new();
    open(&mut again, "tack.vectorcraft", &saved).unwrap();
    assert_eq!(sewn(&plan(&export(&mut again, "dst", json!({})))), sewn(&plan(&file)));
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

/// A stroke wider than the widest satin column is sewn that wide, and the export says so.
#[test]
fn a_satin_wider_than_the_widest_is_warned_about() {
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 150, "x2": 280, "y2": 150})).unwrap();
    // 160 pt is 56 mm.
    s.execute("stroke.set", &json!({"weight": 160})).unwrap();
    let r = s.execute("document.export", &params("dst", json!({}))).expect("exports");
    let w = warnings(&r);
    assert!(w.iter().any(|w| w.contains("50 mm")), "{w:?}");
    // The column is sewn 50 mm wide.
    let b = plan(&vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()).bounds().unwrap();
    assert!((b.height() - 50.0).abs() < 1.5, "{b:?}");
    // A narrower one isn't warned about.
    s.execute("stroke.set", &json!({"weight": 20})).unwrap();
    let r = s.execute("document.export", &params("dst", json!({}))).expect("exports");
    assert!(warnings(&r).is_empty(), "{:?}", warnings(&r));
}

/// The cuts and jumps of a PES the export wrote: every trim, and how far each move with the
/// needle up goes.
fn moves(bytes: &[u8]) -> (usize, Vec<f64>) {
    use vectorcraft_stitch::Command;
    let p = Format::Pes.read(bytes).expect("a PES we wrote reads");
    let trims = p.points().filter(|s| s.command == Command::Trim).count();
    let mut jumps = Vec::new();
    for b in &p.blocks {
        let mut from: Option<vectorcraft_stitch::Stitch> = None;
        let mut up = false;
        for s in &b.stitches {
            match s.command {
                Command::Stitch => {
                    if up && let Some(f) = from {
                        jumps.push(f.distance(*s));
                    }
                    from = Some(*s);
                    up = false;
                }
                _ => up = from.is_some(),
            }
        }
    }
    (trims, jumps)
}

/// Where a clipping path cuts a stroke's stitches, the needle lifts as the planner lifts it: a
/// short way across with the thread on, a longer one tied off before and in after so it can be
/// cut, and the thread cut only before a jump longer than `trimLength`.
#[test]
fn cuts_made_by_a_clipping_mask_are_tied() {
    use vectorcraft_stitch::Command;
    // 20 pt is 7 mm, 100 pt 35 mm.
    for (gap, trims) in [(20.0, 0), (100.0, 1)] {
        let mut s = blank();
        s.execute("paint.setFill", &json!({"none": true})).unwrap();
        s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
        s.execute("shape.line", &json!({"x1": 0, "y1": 75, "x2": 290, "y2": 75})).unwrap();
        s.execute("stroke.set", &json!({"weight": 1.0})).unwrap();
        s.execute("select.none", &json!({})).unwrap();
        s.execute("paint.setStroke", &json!({"none": true})).unwrap();
        let a = s.execute("shape.rectangle", &json!({"x": 20, "y": 50, "width": 50, "height": 50})).unwrap()["id"].clone();
        let b = s.execute("shape.rectangle", &json!({"x": 70.0 + gap, "y": 50, "width": 50, "height": 50})).unwrap()["id"].clone();
        s.execute("select.set", &json!({"ids": [a, b]})).unwrap();
        s.execute("object.compoundPath.make", &json!({})).unwrap();
        s.execute("select.all", &json!({})).unwrap();
        s.execute("object.clippingMask.make", &json!({})).unwrap();
        let bytes = export(&mut s, "pes", json!({}));
        let (cut, jumps) = moves(&bytes);
        assert_eq!(cut, trims, "a {gap} pt gap: {jumps:?}");
        assert_eq!(jumps.len(), 1, "a {gap} pt gap: {jumps:?}");
        // Back and forth on the spot on both sides of the jump.
        let p = Format::Pes.read(&bytes).unwrap();
        let st = &p.blocks[0].stitches;
        let start = st.iter().position(|s| s.command == Command::Stitch).unwrap();
        let up = start + st[start..].iter().position(|s| s.command != Command::Stitch).unwrap();
        let down = up + st[up..].iter().position(|s| s.command == Command::Stitch).unwrap();
        let tied = |w: &[vectorcraft_stitch::Stitch]| w.iter().all(|s| s.command == Command::Stitch) && w[0].distance(w[2]) < 0.15;
        assert!(tied(&st[up - 3..up]), "a {gap} pt gap: tie-off {:?}", &st[up - 3..up]);
        assert!(tied(&st[down..down + 3]), "a {gap} pt gap: tie-in {:?}", &st[down..down + 3]);
    }
}

/// Objects of one thread near each other are joined by a jump, not a trim, unless an object asks
/// for the thread to be cut after it (`embroidery.setTrimAfter`), which the document keeps.
#[test]
fn an_object_can_ask_for_a_trim_after_it() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 30.0, 30.0);
    let first = s.doc().unwrap().doc.layers[0].children().unwrap().last().unwrap().id.0;
    rect(&mut s, "#cc2222", 60.0, 20.0, 30.0, 30.0);
    assert_eq!(moves(&export(&mut s, "pes", json!({}))).0, 0);
    let r = s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [first]})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(moves(&export(&mut s, "pes", json!({}))).0, 1);
    // Saved and opened again, it still asks.
    let saved = export(&mut s, "vectorcraft", json!({}));
    let mut again = Session::new();
    open(&mut again, "trim.vectorcraft", &saved).unwrap();
    assert_eq!(moves(&export(&mut again, "pes", json!({}))).0, 1);
    let r = s.execute("embroidery.setTrimAfter", &json!({"on": false, "ids": [first]})).unwrap();
    assert_eq!(r["changed"], 1);
    assert_eq!(moves(&export(&mut s, "pes", json!({}))).0, 0);
    assert!(s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(first)).unwrap().extras().embroidery.is_none());
    // `on` left out toggles: on, unless every object is marked already (as the command palette
    // runs it, on the selection).
    s.execute("select.set", &json!({"ids": [first]})).unwrap();
    assert_eq!(s.execute("embroidery.setTrimAfter", &json!({})).unwrap(), json!({"on": true, "changed": 1}));
    assert_eq!(s.execute("embroidery.setTrimAfter", &json!({})).unwrap(), json!({"on": false, "changed": 1}));
    // Nothing selected and no ids: not available.
    s.execute("select.none", &json!({})).unwrap();
    let e = s.execute("embroidery.setTrimAfter", &json!({"on": true})).unwrap_err();
    assert!(matches!(e, EngineError::Disabled(..)), "{e:?}");
    // Its description names every format it applies to; its label says it is embroidery's.
    let spec = crate::cmd::find_command("embroidery.setTrimAfter").unwrap();
    for f in Format::ALL {
        assert!(spec.params.contains(&format!(" {}", f.id())), "{}", f.id());
    }
    assert!(spec.label.starts_with("Embroidery"), "{}", spec.label);
}

/// Off, the thread isn't cut after a run of stitches opened from a file either: the cut its file
/// had after it goes too.
#[test]
fn trim_after_off_clears_a_runs_cut_from_its_file() {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    // A block whose thread is cut at its end, before the change.
    let mut p = Pattern::default();
    let mut a = Block::new(Thread::rgb([200, 0, 0]));
    for i in 0..10 {
        a.stitches.push(Stitch::new(f64::from(i) * 2.0, 0.0, Command::Stitch));
    }
    a.stitches.push(Stitch::new(18.0, 0.0, Command::Trim));
    let mut b = Block::new(Thread::rgb([0, 0, 200]));
    for i in 0..10 {
        b.stitches.push(Stitch::new(f64::from(i) * 2.0, 10.0, Command::Stitch));
    }
    p.blocks.extend([a, b]);
    let mut opened = Session::new();
    open(&mut opened, "cut.pes", &Format::Pes.write(&p).unwrap()).unwrap();
    let runs: Vec<u64> = {
        let d = &opened.doc().unwrap().doc;
        let mut ids = Vec::new();
        d.walk(|n| {
            if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.as_ref().is_some_and(|r| r.trim_after)) {
                ids.push(n.id.0);
            }
        });
        ids
    };
    assert!(!runs.is_empty(), "the file cuts at the end of a block");
    let r = opened.execute("embroidery.setTrimAfter", &json!({"on": false, "ids": runs})).unwrap();
    assert_eq!(r["changed"], json!(runs.len()));
    let d = &opened.doc().unwrap().doc;
    for id in &runs {
        let e = d.node(vectorcraft_doc::NodeId(*id)).unwrap().extras().embroidery.as_deref().cloned().unwrap();
        assert!(!e.trim_after && !e.stitches.unwrap().trim_after, "{id}");
    }
}

/// `trimLength` decides which jumps cut the thread: a jump between two objects is cut only when it
/// is longer; a value below `jumpLength` is refused.
#[test]
fn trim_length_decides_which_jumps_are_cut() {
    let mut s = blank();
    // 30 pt squares 57 pt (20 mm) apart.
    rect(&mut s, "#cc2222", 20.0, 20.0, 30.0, 30.0);
    rect(&mut s, "#cc2222", 107.0, 20.0, 30.0, 30.0);
    let (trims, jumps) = moves(&export(&mut s, "pes", json!({})));
    assert_eq!(trims, 1, "{jumps:?}");
    let longest = jumps.iter().copied().fold(0.0, f64::max);
    assert!(longest > 15.0, "{jumps:?}");
    let (trims, jumps) = moves(&export(&mut s, "pes", json!({"trimLength": longest + 1.0})));
    assert_eq!(trims, 0, "{jumps:?}");
    // Only a jump is cut, so a trim length shorter than the jump length is refused, not raised.
    let e = s.execute("document.export", &params("pes", json!({"trimLength": 1.0, "jumpLength": 5.0}))).unwrap_err();
    assert!(e.to_string().contains("at least `jumpLength`"), "{e}");
}

/// Each object starts near where the one sewn before it ended: a satin column drawn from its far
/// end is sewn from its near one, a short jump instead of a long move cut by a trim.
#[test]
fn an_object_starts_near_the_one_before() {
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
    s.execute("stroke.set", &json!({"weight": 6.0})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 50, "x2": 120, "y2": 50})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    s.execute("shape.line", &json!({"x1": 230, "y1": 70, "x2": 130, "y2": 70})).unwrap();
    let (trims, jumps) = moves(&export(&mut s, "pes", json!({})));
    assert_eq!(trims, 0, "{jumps:?}");
    assert!(jumps.iter().all(|d| *d < 10.0), "{jumps:?}");
}

/// An object that starts exactly where the one before ended (a line drawn twice) goes on from
/// there: no jump, and no second needle point in the same hole.
#[test]
fn an_object_starting_where_the_last_ended_adds_no_second_needle_point() {
    use vectorcraft_stitch::Command;
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
    s.execute("stroke.set", &json!({"weight": 0.5})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 50, "x2": 120, "y2": 50})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 50, "x2": 120, "y2": 50})).unwrap();
    let p = Format::Pes.read(&export(&mut s, "pes", json!({}))).unwrap();
    for b in &p.blocks {
        for w in b.stitches.windows(2) {
            if w[0].command == Command::Stitch && w[1].command == Command::Stitch {
                assert!(w[0].distance(w[1]) > 1e-6, "two needle points at {:?}", w[1].point());
            }
        }
    }
}

/// Where the needle lands after each of a PES's trims, as a share of the design's width from its
/// left edge.
fn trims_landing(bytes: &[u8]) -> Vec<f64> {
    use vectorcraft_stitch::Command;
    let p = Format::Pes.read(bytes).expect("a PES we wrote reads");
    let b = p.bounds().expect("bounds");
    let all: Vec<vectorcraft_stitch::Stitch> = p.points().collect();
    all.iter()
        .enumerate()
        .filter(|(_, s)| s.command == Command::Trim)
        .filter_map(|(i, _)| all[i..].iter().find(|s| s.command == Command::Stitch))
        .map(|s| (s.x - b.x0) / b.width())
        .collect()
}

/// `embroidery.setTrimAfter` on a group, a compound path or a clip group cuts the thread after the
/// last stitch it holds, before the next object, and never between its own members.
#[test]
fn a_group_compound_or_clip_group_can_ask_for_a_trim_after_it() {
    let last = |s: &Session| s.doc().unwrap().doc.layers[0].children().unwrap().last().unwrap().id.0;
    for kind in ["group", "compound", "clip"] {
        let mut s = blank();
        // Three red 15 pt (5.3 mm) squares 5 pt apart, the first two made into one object: every
        // move between them is shorter than `trimLength`, a jump without a cut.
        rect(&mut s, "#cc2222", 20.0, 20.0, 15.0, 15.0);
        let a = last(&s);
        rect(&mut s, "#cc2222", 40.0, 20.0, 15.0, 15.0);
        let b = last(&s);
        let made = match kind {
            "group" => "object.group",
            "compound" => "object.compoundPath.make",
            _ => {
                // The clipping path, on top: an unpainted rectangle round both squares.
                s.execute("paint.setFill", &json!({"none": true})).unwrap();
                s.execute("shape.rectangle", &json!({"x": 15, "y": 15, "width": 43, "height": 25})).unwrap();
                "object.clippingMask.make"
            }
        };
        let mut ids = vec![a, b];
        if kind == "clip" {
            ids.push(last(&s));
        }
        s.execute("select.set", &json!({"ids": ids})).unwrap();
        s.execute(made, &json!({})).unwrap();
        let made = last(&s);
        s.execute("select.none", &json!({})).unwrap();
        rect(&mut s, "#cc2222", 60.0, 20.0, 15.0, 15.0);
        assert_eq!(trims_landing(&export(&mut s, "pes", json!({}))), Vec::<f64>::new(), "{kind}");
        let r = s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [made]})).unwrap();
        assert_eq!(r["changed"], 1, "{kind}");
        let landing = trims_landing(&export(&mut s, "pes", json!({})));
        // The third square spans the design's last 27 % (60 to 75 pt of 20 to 75).
        assert!(landing.len() == 1 && landing[0] > 0.65, "{kind}: a trim, the needle landing at {landing:?} of the width");
    }
}

/// `embroidery.setTrimAfter` with an id that names no object fails as a missing object, and
/// changes none of the others it was given.
#[test]
fn trim_after_refuses_a_missing_object() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 30.0, 30.0);
    let first = s.doc().unwrap().doc.layers[0].children().unwrap().last().unwrap().id.0;
    rect(&mut s, "#cc2222", 60.0, 20.0, 30.0, 30.0);
    let e = s.execute("embroidery.setTrimAfter", &json!({"on": true, "ids": [first, 999_999]})).unwrap_err();
    assert!(matches!(e, EngineError::NoNode(vectorcraft_doc::NodeId(999_999))), "{e:?}");
    assert!(s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(first)).unwrap().extras().embroidery.is_none());
    assert_eq!(moves(&export(&mut s, "pes", json!({}))).0, 0);
}

/// Points per millimetre: the document counts points, the designs millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A fill's rows reach past its edges by the pull compensation (by default too), and `expand` grows
/// the shape all round before it is filled (or shrinks it).
#[test]
fn fills_are_stretched_by_pull_compensation_and_grown_by_expand() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 50.0, 50.0, 20.0 * PT_PER_MM, 10.0 * PT_PER_MM);
    let size = |s: &mut Session, extra: Value| {
        let mut p = json!({"stitchAngle": 0, "underlay": false});
        if let (Some(p), Value::Object(e)) = (p.as_object_mut(), extra) {
            p.extend(e);
        }
        let b = plan(&export(s, "dst", p)).bounds().expect("bounds");
        (b.width(), b.height())
    };
    let (w0, h0) = size(&mut s, json!({"pullCompensation": 0}));
    assert!((w0 - 20.0).abs() < 0.15, "rows edge to edge: {w0}");
    let (w, h) = size(&mut s, json!({}));
    assert!((w - 20.4).abs() < 0.15 && (h - h0).abs() < 0.15, "0.2 mm past each end by default: {w} × {h}");
    let (w, _) = size(&mut s, json!({"pullCompensation": [0.5, 0.0]}));
    assert!((w - 20.5).abs() < 0.15, "one side only: {w}");
    // Across the rows, as many fit as the spacing allows, centred: within a spacing (0.38 mm).
    let across = 0.4;
    let (w, h) = size(&mut s, json!({"pullCompensation": 0, "expand": 1}));
    assert!((w - 22.0).abs() < 0.15 && (h - (h0 + 2.0)).abs() < across, "grown 1 mm all round: {w} × {h} (from {w0} × {h0})");
    let (w, h) = size(&mut s, json!({"pullCompensation": 0, "expand": -1}));
    assert!((w - 18.0).abs() < 0.15 && (h - (h0 - 2.0)).abs() < across, "shrunk 1 mm all round: {w} × {h}");
}

/// Two fills drawn 0.6 mm apart leave a gap of background between them; pull compensation or
/// `expand` closes it, each fill reaching half way across.
#[test]
fn neighbouring_fills_meet_without_a_gap() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 50.0, 50.0, 10.0 * PT_PER_MM, 10.0 * PT_PER_MM);
    rect(&mut s, "#2233cc", 50.0 + 10.6 * PT_PER_MM, 50.0, 10.0 * PT_PER_MM, 10.0 * PT_PER_MM);
    let gap = |s: &mut Session, extra: Value| {
        let mut p = json!({"stitchAngle": 0, "underlay": false});
        if let (Some(p), Value::Object(e)) = (p.as_object_mut(), extra) {
            p.extend(e);
        }
        let design = plan(&export(s, "dst", p));
        let edge = |b: &vectorcraft_stitch::Block, right: bool| {
            let xs = b.stitches.iter().filter(|s| s.command == vectorcraft_stitch::Command::Stitch).map(|s| s.x);
            if right { xs.fold(f64::NEG_INFINITY, f64::max) } else { xs.fold(f64::INFINITY, f64::min) }
        };
        assert_eq!(design.blocks.len(), 2);
        edge(&design.blocks[1], false) - edge(&design.blocks[0], true)
    };
    let drawn = gap(&mut s, json!({"pullCompensation": 0}));
    assert!((drawn - 0.6).abs() < 0.15, "as drawn: {drawn}");
    for extra in [json!({"pullCompensation": 0.3}), json!({"pullCompensation": 0, "expand": 0.3})] {
        let g = gap(&mut s, extra.clone());
        assert!(g.abs() < 0.15, "{extra}: a {g} mm gap");
    }
}

/// The spacing follows the thread's weight: a finer thread sews more stitches over the same art, a
/// thicker one fewer.
#[test]
fn the_thread_weight_sets_the_spacing() {
    let mut s = art();
    let count = |s: &mut Session, weight: f64| plan(&export(s, "dst", json!({"threadWeight": weight}))).sewn();
    let (thick, standard, fine) = (count(&mut s, 30.0), count(&mut s, 40.0), count(&mut s, 60.0));
    assert!(thick < standard && standard < fine, "30 wt {thick}, 40 wt {standard}, 60 wt {fine} stitches");
    assert_eq!(standard, plan(&export(&mut s, "dst", json!({}))).sewn(), "40 wt is the default");
}

/// Push compensation sews a satin column shorter at its ends: a 30 mm line 3 mm wide, 2 mm off its
/// start and 1 mm off its end.
#[test]
fn push_compensation_shortens_a_satin_column() {
    let mut s = blank();
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#cc2222"})).unwrap();
    s.execute("shape.line", &json!({"x1": 50.0, "y1": 100.0, "x2": 50.0 + 30.0 * PT_PER_MM, "y2": 100.0})).unwrap();
    s.execute("stroke.set", &json!({"weight": 3.0 * PT_PER_MM})).unwrap();
    let length = |s: &mut Session, extra: Value| plan(&export(s, "dst", extra)).bounds().expect("bounds").width();
    let (drawn, pushed) = (length(&mut s, json!({})), length(&mut s, json!({"pushCompensation": [2, 1]})));
    assert!((drawn - 30.0).abs() < 0.15 && (pushed - 27.0).abs() < 0.15, "{drawn} mm, pushed {pushed} mm");
}

/// A filled polygon in `color` from corners in millimetres, unselected.
fn filled_polygon(s: &mut Session, color: &str, corners: &[(f64, f64)]) {
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": color})).unwrap();
    let d: Vec<String> = corners
        .iter()
        .enumerate()
        .map(|(k, (x, y))| format!("{} {} {}", if k == 0 { "M" } else { "L" }, 50.0 + x * PT_PER_MM, 50.0 + y * PT_PER_MM))
        .collect();
    s.execute("path.create", &json!({"d": format!("{} Z", d.join(" "))})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
}

/// How many of a design's stitches cross a horizontal stripe `width` mm wide from side to side.
fn crossings(p: &vectorcraft_stitch::Pattern, width: f64) -> (usize, usize) {
    use vectorcraft_stitch::Command;
    let pts: Vec<vectorcraft_stitch::Stitch> = p.points().filter(|s| s.command == Command::Stitch).collect();
    let across = pts.windows(2).filter(|w| (w[0].y - w[1].y).abs() > width - 0.3 && (w[0].x - w[1].x).abs() < 0.6).count();
    (across, pts.len())
}

/// A narrow filled shape is sewn as a satin column across it; `stitchStyle: fill` or
/// `narrowAsSatin: 0` keeps it tatami.
#[test]
fn a_narrow_fill_is_sewn_as_satin() {
    let mut s = blank();
    filled_polygon(&mut s, "#cc2222", &[(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    let (across, all) = crossings(&plan(&export(&mut s, "dst", json!({"underlay": false}))), 3.0);
    assert!(across * 10 > all * 8, "{across} of {all} stitches cross the stripe");
    let tatami = plan(&export(&mut s, "dst", json!({"underlay": false, "stitchStyle": "fill"})));
    let (across, all) = crossings(&tatami, 3.0);
    assert!(across * 10 < all, "forced tatami: {across} of {all} cross");
    let off = plan(&export(&mut s, "dst", json!({"underlay": false, "narrowAsSatin": 0})));
    assert_eq!(sewn(&off), sewn(&tatami), "narrowAsSatin 0 sews tatami");
}

/// A letter T is sewn in one go: its three strokes as satin columns and the square where they
/// meet as tatami, with no jump and no trim.
#[test]
fn a_letter_t_is_sewn_in_one_go() {
    let mut s = blank();
    let t = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];
    filled_polygon(&mut s, "#cc2222", &t);
    let bytes = export(&mut s, "pes", json!({}));
    let (trims, jumps) = moves(&bytes);
    assert_eq!((trims, jumps.len()), (0, 0), "{trims} trims, jumps {jumps:?}");
    // Each stroke's satin is widened across it by the pull compensation (0.2 mm a side): the bar
    // reaches 0.2 mm above the T, and the column ends keep the T's length and the stem's.
    let p = Format::Pes.read(&bytes).unwrap();
    let b = p.bounds().unwrap();
    assert!((b.width() - 30.0).abs() < 0.15 && (b.height() - 23.2).abs() < 0.15, "{b:?}");
}

/// A filled path from SVG path data in millimetres (offset 50 pt into the page), in `color`.
fn filled_d(s: &mut Session, color: &str, subpaths: &[&[(f64, f64)]], close: &[bool]) {
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": color})).unwrap();
    let mut d = String::new();
    for (k, sub) in subpaths.iter().enumerate() {
        for (i, (x, y)) in sub.iter().enumerate() {
            d.push_str(&format!("{} {} {} ", if i == 0 { "M" } else { "L" }, 50.0 + x * PT_PER_MM, 50.0 + y * PT_PER_MM));
        }
        if close.get(k).copied().unwrap_or(true) {
            d.push_str("Z ");
        }
    }
    s.execute("path.create", &json!({"d": d})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
}

/// A negative `expand` shrinks the region a path covers, not each of its outlines: two squares
/// that overlap in one path shrink as one shape, with no groove along the outlines inside it.
#[test]
fn a_negative_expand_shrinks_the_region_not_its_outlines() {
    let mut s = blank();
    let square = |x0: f64| [(x0, 0.0), (x0 + 10.0, 0.0), (x0 + 10.0, 10.0), (x0, 10.0)];
    let (a, b) = (square(0.0), square(5.0));
    filled_d(&mut s, "#cc2222", &[&a, &b], &[true, true]);
    for e in [0.0, -1.0] {
        let p = plan(&export(&mut s, "dst", json!({"stitchAngle": 90, "underlay": false, "expand": e})));
        let b = p.bounds().unwrap();
        // Rows run up and down: no two neighbouring rows more than a spacing or two apart.
        let mut xs: Vec<f64> =
            p.points().filter(|s| s.command == vectorcraft_stitch::Command::Stitch).map(|s| ((s.x - b.x0) * 10.0).round() / 10.0).collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        let widest = xs.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        assert!(widest < 0.8, "expand {e}: a {widest} mm gap between rows");
        assert!((b.width() - (15.0 + 2.0 * e)).abs() < 0.8, "expand {e}: {} mm wide", b.width());
    }
}

/// A negative `expand` that leaves nothing of a shape says so: alongside other art as a warning,
/// alone in the error.
#[test]
fn a_shape_expanded_away_is_warned_about() {
    let mut s = blank();
    filled_d(&mut s, "#cc2222", &[&[(0.0, 0.0), (20.0, 0.0), (20.0, 1.0), (0.0, 1.0)]], &[true]);
    let e = s.execute("document.export", &params("dst", json!({"expand": -1}))).unwrap_err().to_string();
    assert!(e.contains("nothing to stitch") && e.contains("negative expand"), "{e}");
    rect(&mut s, "#2233cc", 50.0, 100.0, 20.0 * PT_PER_MM, 10.0 * PT_PER_MM);
    let r = s.execute("document.export", &params("dst", json!({"expand": -1}))).unwrap();
    assert!(warnings(&r).iter().any(|w| w.contains("negative expand")), "{:?}", warnings(&r));
}

/// `expand` grows a fill's areas only: an open line in the same path (no area: it is sewn as a
/// line) stays a line, wherever it lies on the page.
#[test]
fn expand_leaves_a_line_in_a_fill_a_line() {
    let mut s = blank();
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let line = [(0.0, 30.0), (40.0, 30.0)];
    filled_d(&mut s, "#cc2222", &[&square, &line], &[true, false]);
    let p = plan(&export(&mut s, "dst", json!({"expand": 1, "underlay": false})));
    let b = p.bounds().unwrap();
    // The line's stitches well past the square (15 mm and more from the left) lie on it: none beside it.
    let near_line: Vec<f64> = p.points().filter(|s| s.command == vectorcraft_stitch::Command::Stitch && s.x - b.x0 > 15.0).map(|s| s.y).collect();
    let spread = near_line.iter().fold(f64::NEG_INFINITY, |m, y| m.max(*y)) - near_line.iter().fold(f64::INFINITY, |m, y| m.min(*y));
    assert!(!near_line.is_empty() && spread < 0.15, "the line's stitches spread {spread} mm across it");
}

/// A DST of `records` behind a blank header (each record three bytes, the end record added).
fn dst_of(records: &[[u8; 3]]) -> Vec<u8> {
    let mut b = vec![b' '; 512];
    for r in records {
        b.extend_from_slice(r);
    }
    b.extend_from_slice(&[0, 0, 0xF3]);
    b
}

/// A design wider than a document holds (1.45 km) is refused when it opens, rather than opening
/// into a document whose every edit is then refused; so is one of more separate runs of stitches
/// than a document opens as objects.
#[test]
fn an_opened_design_too_large_for_a_document_is_refused() {
    // 120,000 stitches of +12.1 mm along x.
    let wide = dst_of(&vec![[0x05, 0x05, 0x07]; 120_000]);
    let mut s = Session::new();
    let e = open(&mut s, "wide.dst", &wide).expect_err("too wide").to_string();
    assert!(e.contains("larger than a document holds"), "{e}");
    // A stitch, then a jump, 100,001 times: as many runs.
    let stitch = [0x01, 0x00, 0x03];
    let jump = [0x02, 0x00, 0x83];
    let mut many = Vec::new();
    for _ in 0..100_001 {
        many.extend([stitch, jump]);
    }
    let e = open(&mut s, "many.dst", &dst_of(&many)).expect_err("too many runs").to_string();
    assert!(e.contains("separate runs"), "{e}");
}

/// Export Selection writes the selected objects alone: one of two rectangles, one block.
#[test]
fn export_selection_writes_the_selected_objects_alone() {
    let mut s = blank();
    let red = {
        rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
        s.doc().unwrap().doc.layers[0].children().unwrap().last().unwrap().id.0
    };
    rect(&mut s, "#2233cc", 120.0, 20.0, 40.0, 30.0);
    s.execute("select.set", &json!({"ids": [red]})).unwrap();
    let r = s.execute("document.exportSelection", &json!({"format": "pes"})).unwrap();
    let bytes = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    assert_eq!(Format::Pes.read(&bytes).unwrap().blocks.len(), 1);
}

/// A machine file placed beside art keeps its needle points (moved to where it is placed), and the
/// art beside it is planned: the export sews both.
#[test]
fn a_placed_design_keeps_its_needle_points_beside_planned_art() {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    let mut a = Block::new(Thread::rgb([0, 0, 200]));
    for i in 0..10 {
        a.stitches.push(Stitch::new(f64::from(i) * 2.0, f64::from(i % 2), Command::Stitch));
    }
    let design = Pattern { blocks: vec![a], ..Pattern::default() };
    let file = Format::Dst.write(&design).unwrap();
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    s.execute("file.place", &json!({"name": "run.dst", "dataBase64": vectorcraft_format::base64_encode(&file), "at": [200, 200]})).unwrap();
    let written = plan(&export(&mut s, "dst", json!({})));
    assert_eq!(written.blocks.len(), 2, "the rectangle and the placed run");
    // The placed run's needle points, as they were relative to one another.
    let placed = written.blocks.iter().find(|b| b.sewn() == 10).expect("a block of the run's ten needle points");
    let steps: Vec<(f64, f64)> = placed
        .stitches
        .windows(2)
        .filter(|w| w.iter().all(|s| s.command == Command::Stitch))
        .map(|w| ((w[1].x - w[0].x).round(), (w[1].y - w[0].y).abs().round()))
        .collect();
    assert!(steps.iter().all(|&(dx, dy)| dx == 2.0 && dy == 1.0), "{steps:?}");
}
