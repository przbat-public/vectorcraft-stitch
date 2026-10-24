//! The embroidery thread libraries: listed and opened beside the built-in swatch libraries, and
//! used as any Document Library palette is (Image Trace).

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::Color;
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::thread::Palette;

use super::test_support::run;
use super::*;

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
    s
}

/// The embroidery thread libraries are the palettes PES and JEF files number their threads from:
/// every thread in order (PEC's appliqué entries, which are not threads, left out), named by its
/// palette, number and name; listed under their own category, found by id or name, added to the
/// document whole, and built in (never copied to the user folder).
#[test]
fn thread_libraries_are_the_machine_formats_palettes() {
    let mut s = session();
    let libs = run(&mut s, "swatch.library.list", json!({}));
    let threads: Vec<(&str, &str, u64)> = libs["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["category"] == "threads")
        .map(|l| (l["id"].as_str().unwrap(), l["name"].as_str().unwrap(), l["count"].as_u64().unwrap()))
        .collect();
    assert_eq!(threads, [("brother-threads-pec", "Brother Threads (PEC)", 61), ("janome-threads-jef", "Janome Threads (JEF)", 78)]);
    for (id, palette, code) in [("brother-threads-pec", Palette::Pec, "PEC"), ("janome-threads-jef", Palette::Jef, "JEF")] {
        let g = run(&mut s, "swatch.library.get", json!({"library": id}));
        let swatches = g["swatches"].as_array().unwrap();
        let entries: Vec<(usize, &([u8; 3], &str))> = palette.threads().iter().enumerate().filter(|(i, _)| palette.is_thread(i + 1)).collect();
        assert_eq!(swatches.len(), entries.len(), "{id}");
        for (w, (i, ([r, gr, b], name))) in swatches.iter().zip(entries) {
            assert_eq!(w["name"], format!("{code} {} {name}", i + 1));
            assert_eq!(w["hex"], format!("#{r:02x}{gr:02x}{b:02x}"));
            assert_eq!((&w["kind"], &w["global"], &w["group"]), (&json!("color"), &json!(false), &Value::Null));
        }
    }
    assert_eq!(run(&mut s, "swatch.library.get", json!({"library": "janome threads (jef)"}))["id"], "janome-threads-jef");
    let r = run(&mut s, "swatch.library.add", json!({"library": "brother-threads-pec"}));
    assert_eq!(r["added"].as_array().unwrap().len(), 61);
    let doc = &s.doc().unwrap().doc;
    assert!(["PEC 12 Orange", "PEC 42 Linen", "PEC 57 Flesh Pink", "PEC 61 Fresh Green"].iter().all(|n| doc.swatch(n).is_some()));
    assert!(["PEC 62 Orange", "PEC 63 Cream Yellow", "PEC 64 Applique"].iter().all(|n| doc.swatch(n).is_none()), "not threads");
    assert_eq!(Palette::from_id("jef"), Some(Palette::Jef));
    // One library for each palette, in the palettes' order.
    assert_eq!(threadlib::THREAD_LIBRARIES.map(|t| t.palette), Palette::ALL);
    assert!(s.execute("swatch.library.copyToUser", &json!({"library": "brother-threads-pec"})).is_err());
}

/// An image whose four quarters are four colours, selected.
fn quarters(s: &mut Session) {
    let r = vectorcraft_trace::Raster::from_fn(60, 60, |x, y| match (x < 30, y < 30) {
        (true, true) => [230, 20, 20, 255],
        (false, true) => [20, 200, 30, 255],
        (true, false) => [20, 30, 220, 255],
        (false, false) => [250, 230, 10, 255],
    });
    let (png, (w, h)) = (r.encode_png(), (r.width, r.height));
    s.edit("img", |d, sel| {
        d.images.insert("img1".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let l = d.default_layer();
        let image =
            ImageObject { key: "img1".into(), width: w, height: h, xf: Affine::IDENTITY, link: None, placement: Default::default(), pixels: None };
        d.insert(l, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
}

/// An embroidery thread library is a Document Library palette like any other: every traced area
/// comes out in one of Brother's (or Janome's) threads, as it is, so the design sews in exactly
/// those threads.
#[test]
fn image_trace_with_a_thread_library_paints_in_its_threads() {
    let mut s = session();
    quarters(&mut s);
    let mut id: Option<Value> = None;
    for (library, palette) in [("brother-threads-pec", Palette::Pec), ("Janome Threads (JEF)", Palette::Jef)] {
        let mut p = json!({"preset": "6 Colors", "params": {"palette": "documentLibrary", "library": library, "colors": 4}});
        if let Some(id) = &id {
            p["id"] = id.clone();
        }
        let r = s.execute("imageTrace.make", &p).unwrap();
        let traced = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(r["id"].as_u64().unwrap())).cloned().unwrap();
        let paths: Vec<Arc<Node>> = traced.children().unwrap().iter().filter(|c| !matches!(c.kind, NodeKind::Image(_))).cloned().collect();
        let fills: Vec<Color> = paths.iter().filter_map(|n| n.appearance.fill_paint().color()).collect();
        let threads: Vec<[u8; 3]> = palette.threads().iter().map(|(rgb, _)| *rgb).collect();
        assert_eq!(fills.len(), 4, "{library}: {fills:?}");
        for c in &fills {
            let [r, g, b, _] = c.to_rgba8(1.0);
            assert!(threads.contains(&[r, g, b]), "{library}: {c:?} is none of its threads");
        }
        id = Some(r["id"].clone());
    }
}
