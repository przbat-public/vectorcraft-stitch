//! Digitizing from an image: the working copy, the trace in millimetres, the settings.

use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace as tr;

use super::settings::{MIN_AREA_MM2, SETTINGS, Settings};
use super::trace::{self, Palette as TracePalette, area_px};
use super::working::{self, COARSE_PX_PER_MM, resampled};
use crate::Session;

/// Points a millimetre.
const PT_PER_MM: f64 = 1.0 / MM_PER_PT;

/// A session holding a new document.
fn blank() -> Session {
    super::super::test_support::blank()
}

/// `raster` placed as an image whose pixel `(x, y)` lies at `xf × (x, y)` points, selected; `None`
/// when the document refuses it (a transform past its range).
fn placed_with(s: &mut Session, raster: &tr::Raster, xf: Affine) -> Option<NodeId> {
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let layer = d.default_layer();
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(id)
    })
    .ok()
}

/// `raster` placed as an image whose pixel `(x, y)` lies at `xf × (x, y)` points, selected.
fn image_with(s: &mut Session, raster: &tr::Raster, xf: Affine) -> NodeId {
    placed_with(s, raster, xf).unwrap()
}

/// `raster` placed `mm_per_px` millimetres a pixel, its corner at the origin.
fn image(s: &mut Session, raster: &tr::Raster, mm_per_px: f64) -> NodeId {
    image_with(s, raster, Affine::scale(mm_per_px * PT_PER_MM))
}

/// A flat raster of one colour.
fn flat(w: u32, h: u32, c: [u8; 4]) -> tr::Raster {
    tr::Raster::from_fn(w, h, |_, _| c)
}

/// What `embroidery.digitizeImage` answers with `params` (a dry run).
fn dry(s: &mut Session, params: Value) -> Value {
    let mut p = params;
    p["dryRun"] = json!(true);
    s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// Why `embroidery.digitizeImage` refuses `params` (a dry run).
fn refused(s: &mut Session, params: Value) -> String {
    let mut p = params;
    p["dryRun"] = json!(true);
    match s.execute("embroidery.digitizeImage", &p) {
        Ok(v) => panic!("{p} answered {v}"),
        Err(e) => e.to_string(),
    }
}

/// The working copy's resolution is the smallest of 10 pixels a millimetre, 1,500 pixels over the
/// design's longest side and the image's own: a 1000 × 800 pixel image 100 mm wide works at its own
/// 10; 300 mm wide, at 5 (1,500 pixels); a 300 × 240 pixel one 100 mm wide at its own 3, with a
/// warning that it is coarse.
#[test]
fn the_working_resolution_is_the_smallest_of_three() {
    let cases = [
        (1000, 800, 0.1, 10.0, "grid", [1000, 800], [100.0, 80.0]),
        (3000, 2400, 0.1, 5.0, "side", [1500, 1200], [300.0, 240.0]),
        (300, 240, 100.0 / 300.0, 3.0, "image", [300, 240], [100.0, 80.0]),
        (2000, 1600, 0.05, 10.0, "grid", [1000, 800], [100.0, 80.0]),
    ];
    for (w, h, mm_per_px, px_per_mm, limit, worked, size) in cases {
        let mut s = blank();
        image(&mut s, &flat(w, h, [200, 40, 40, 255]), mm_per_px);
        let r = dry(&mut s, json!({}));
        let im = &r["image"];
        assert_eq!(im["pxPerMm"], json!(px_per_mm), "{w} × {h}: {im}");
        assert_eq!(im["limitedBy"], json!(limit), "{w} × {h}");
        assert_eq!(im["worked"], json!(worked), "{w} × {h}");
        assert_eq!(im["size"], json!(size), "{w} × {h}");
        let coarse = r["warnings"].as_array().unwrap().iter().find(|t| t["id"] == "coarse");
        assert_eq!(coarse.is_some(), px_per_mm < COARSE_PX_PER_MM, "{w} × {h}: {}", r["warnings"]);
        if let Some(c) = coarse {
            assert_eq!(c["values"], json!({"pxPerMm": px_per_mm, "fewerThan": COARSE_PX_PER_MM}), "{w} × {h}: {c}");
            assert!(c["message"].as_str().unwrap().contains("pixels a millimetre"), "{c}");
        }
    }
}

/// The design's size is the image's as placed, else the width, the height or the largest size
/// within a field asked for, the picture's proportions kept; more than one of them is refused.
#[test]
fn the_size_follows_the_picture() {
    let mut s = blank();
    image(&mut s, &flat(1000, 800, [30, 90, 200, 255]), 0.1);
    for (params, size, worked) in [
        (json!({"width": 50}), [50.0, 40.0], [500, 400]),
        (json!({"height": 20}), [25.0, 20.0], [250, 200]),
        (json!({"fit": [40, 40]}), [40.0, 32.0], [400, 320]),
        (json!({"fit": [400, 100]}), [125.0, 100.0], [1000, 800]),
    ] {
        let r = dry(&mut s, params.clone());
        assert_eq!(r["image"]["size"], json!(size), "{params}");
        assert_eq!(r["image"]["worked"], json!(worked), "{params}: never more pixels than the image has");
    }
    let r = dry(&mut s, json!({"fit": [400, 100]}));
    assert_eq!(r["image"]["limitedBy"], json!("image"), "125 mm from 1000 pixels: 8 a millimetre");
    assert!(refused(&mut s, json!({"width": 50, "height": 20})).contains("one of"));
    assert!(refused(&mut s, json!({"width": 0})).contains("above 0"));
    assert!(refused(&mut s, json!({"fit": [50]})).contains("[width, height]"));
    assert!(refused(&mut s, json!({"width": 5000})).contains("1000"));
}

/// A crop is in the image's own pixels: it sets the design's size and where the working copy
/// starts; one outside the image, empty or turned round is refused.
#[test]
fn a_crop_counts_the_images_pixels() {
    let mut s = blank();
    image(&mut s, &flat(1000, 800, [30, 90, 200, 255]), 0.1);
    let r = dry(&mut s, json!({"crop": [100, 200, 600, 600]}));
    assert_eq!(r["image"]["crop"], json!([100, 200, 600, 600]));
    assert_eq!(r["image"]["size"], json!([50.0, 40.0]));
    assert_eq!(r["image"]["worked"], json!([500, 400]));
    assert!(refused(&mut s, json!({"crop": [0, 0, 1001, 800]})).contains("outside"));
    assert!(refused(&mut s, json!({"crop": [10, 10, 10, 20]})).contains("x0 < x1"));
    assert!(refused(&mut s, json!({"crop": [-1, 0, 10, 20]})).contains("whole pixels"));
}

/// The design's size of a turned image is measured as it lies on the page (the project owner's
/// decision, 2026-11-03): turned a quarter, a 100 × 80 mm picture fitted to a 100 × 80 mm field
/// comes out 64 wide and 80 tall, within it; turned 30°, `width` is the width of the box round it.
/// Its working copy runs along its own axes, at the resolution it has upright.
#[test]
fn a_turned_image_is_measured_on_the_page() {
    let picture = flat(1000, 800, [30, 90, 200, 255]);
    let turned = |degrees: f64, params: Value| {
        let mut s = blank();
        image_with(&mut s, &picture, Affine::rotate(degrees.to_radians()) * Affine::scale(0.1 * PT_PER_MM));
        dry(&mut s, params)["image"].clone()
    };
    let bounds = |r: &Value| {
        let b: Vec<f64> = r["bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        (b[2] - b[0], b[3] - b[1])
    };
    let quarter = turned(90.0, json!({"fit": [100, 80]}));
    let (w, h) = bounds(&quarter);
    assert!((w - 64.0).abs() < 0.01 && (h - 80.0).abs() < 0.01, "within the field: {w} × {h}");
    assert_eq!(quarter["size"], json!([64.0, 80.0]));
    let thirty = turned(30.0, json!({"width": 50}));
    let (w, _) = bounds(&thirty);
    assert!((w - 50.0).abs() < 0.01, "the box round it 50 wide: {w}");
    let upright = {
        let mut s = blank();
        image(&mut s, &picture, 0.1);
        dry(&mut s, json!({}))["image"].clone()
    };
    let as_placed = turned(30.0, json!({}));
    for key in ["pxPerMm", "worked"] {
        assert_eq!(as_placed[key], upright[key], "{key}");
    }
}

/// A slanted image is measured as it lies on the page too: a 10 mm square slanted 30° covers
/// 100 mm² (its base and height kept), not the 115 its slanted side would give, and lies where its
/// picture does.
#[test]
fn a_slanted_image_is_measured_on_the_page() {
    let mut s = blank();
    let slant = Affine::new([1.0, 0.0, 30f64.to_radians().tan(), 1.0, 0.0, 0.0]);
    image_with(&mut s, &flat(100, 100, [30, 90, 200, 255]), slant * Affine::scale(0.1 * PT_PER_MM));
    let r = dry(&mut s, json!({"background": "none", "colors": 2}));
    let areas = r["areas"].as_array().unwrap();
    assert_eq!(areas.len(), 1, "{areas:?}");
    let mm2 = areas[0]["size"]["area"].as_f64().unwrap();
    assert!((mm2 - 100.0).abs() < 1.0, "the page's 100 mm²: {mm2}");
    let b: Vec<f64> = r["image"]["bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let width = 10.0 + 10.0 * 30f64.to_radians().tan();
    assert!((b[2] - b[0] - width).abs() < 0.1 && (b[3] - b[1] - 10.0).abs() < 0.1, "the picture's box: {b:?}");
}

/// A slanted picture's background is found at its own corners, and the areas of it that touch its
/// edge are left unsewn, as upright: a white margin round a navy square.
#[test]
fn a_slanted_pictures_background_is_its_own() {
    let mut s = blank();
    let slant = Affine::new([1.0, 0.0, 30f64.to_radians().tan(), 1.0, 0.0, 0.0]);
    let picture =
        tr::Raster::from_fn(
            200,
            200,
            |x, y| if (40..160).contains(&x) && (40..160).contains(&y) { [20, 40, 120, 255] } else { [255, 255, 255, 255] },
        );
    image_with(&mut s, &picture, slant * Affine::scale(0.1 * PT_PER_MM));
    let r = dry(&mut s, json!({"colors": 2}));
    assert_eq!(r["image"]["background"]["from"], json!("corners"), "{}", r["image"]["background"]);
    let white = r["colors"].as_array().unwrap().iter().find(|c| c["background"] == json!(true)).unwrap_or_else(|| panic!("{}", r["colors"]));
    assert_eq!(white["sewn"], json!(false));
    assert!(r["areas"].as_array().unwrap().iter().all(|a| a["color"] != white["color"]), "no white area sewn: {}", r["areas"]);
}

/// The working copy averages the pixels each of its own covers, weighted by their opacity: a 2 × 2
/// square of red, blue and two transparent pixels comes to a half-opaque purple, not a darker one.
#[test]
fn averaging_down_weighs_pixels_by_their_opacity() {
    let src = tr::Raster::from_fn(2, 2, |x, y| match (x, y) {
        (0, 0) => [255, 0, 0, 255],
        (1, 1) => [0, 0, 255, 255],
        _ => [0, 0, 0, 0],
    });
    let one = resampled(&src, [0, 0, 2, 2], 1, 1).unwrap();
    assert_eq!(one.pixel(0, 0), [128, 0, 128, 128], "transparent pixels tint nothing");
    // Never enlarged: asked for more pixels than the crop has, it is the crop as it is.
    let same = resampled(&src, [0, 0, 2, 2], 4, 4).unwrap();
    assert_eq!((same.width, same.height), (2, 2));
    assert_eq!(same, src);
    // A box of 1.5 pixels: the middle one counts half to each side.
    let row = tr::Raster::from_fn(3, 1, |x, _| [(x * 100) as u8, 0, 0, 255]);
    let two = resampled(&row, [0, 0, 3, 1], 2, 1).unwrap();
    assert_eq!(two.pixel(0, 0)[0], 33, "0 and half of 100, over 1.5");
    assert_eq!(two.pixel(1, 0)[0], 167, "half of 100 and 200, over 1.5");
}

/// An area turns into the working copy's pixels as ⌈A × r²⌉: 1 mm² is 100 pixels at 10 a
/// millimetre and 16 at 4; nothing, or a number that isn't one, is none.
#[test]
fn areas_turn_into_whole_pixels() {
    assert_eq!(area_px(MIN_AREA_MM2, 10.0), 100);
    assert_eq!(area_px(1.0, 4.0), 16);
    assert_eq!(area_px(1.41, 10.0), 141);
    assert_eq!(area_px(0.0, 10.0), 0);
    assert_eq!(area_px(f64::NAN, 10.0), 0);
    assert_eq!(area_px(1e30, 10.0), u32::MAX);
    // A product a hair over a whole number is that number: 0.4 mm squared at 10 a millimetre.
    assert_eq!(area_px(super::trace::TRACE_AREA_MM2, 10.0), 16);
    assert_eq!(area_px(0.3, 10.0), 30);
    assert_eq!(area_px(0.305, 10.0), 31);
}

/// The trace's noise keeps a thread's footprint whatever the smallest area above it, so a refusal
/// past the anchor points or the shapes never advises raising the smallest area, which changes
/// neither; only the areas' refusal does, which it changes.
#[test]
fn refusals_advise_only_what_changes_them() {
    let at = |a: f64| Settings::from_params(json!({ "minArea": a }).as_object().unwrap(), Palette::Jef).unwrap();
    let noise = |a: f64| super::trace::params(&at(a), 10.0, 6, TracePalette::Mode).noise;
    assert_eq!(noise(1.0), noise(50.0), "the smallest area past a thread's footprint traces alike");
    let anchors = super::trace::refusal(tr::TraceError::TooComplex { max: 10 }).to_string();
    assert!(!anchors.contains("minArea") && anchors.contains("colors") && anchors.contains("smoothing") && anchors.contains("crop"), "{anchors}");
}

/// The smallest area is in square millimetres: at 10 pixels a millimetre, a speck of 1.4 mm² (140
/// pixels) joins its neighbour when the smallest area is 1.5 mm², and one of 1.6 mm² stays; at 1.3
/// mm² both stay.
#[test]
fn the_smallest_area_is_in_square_millimetres() {
    let (orange, blue) = ([240, 130, 20, 255], [20, 60, 200, 255]);
    let picture = tr::Raster::from_fn(400, 400, |x, y| {
        let speck = |x0: u32, y0: u32, w: u32| (x0..x0 + w).contains(&x) && (y0..y0 + 10).contains(&y);
        if y >= 300 || speck(50, 50, 14) || speck(250, 50, 16) { blue } else { orange }
    });
    let mut s = blank();
    image(&mut s, &picture, 0.1);
    let blues = |r: &Value| r["colors"].as_array().unwrap().iter().find(|c| c["color"] == json!("#143cc8")).map(|c| c["areaCount"].clone());
    let r = dry(&mut s, json!({"colors": 2, "minArea": 1.5}));
    assert_eq!(r["image"]["pxPerMm"], json!(10.0));
    assert_eq!(blues(&r), Some(json!(2)), "the band and the 1.6 mm² speck: {}", r["colors"]);
    let r = dry(&mut s, json!({"colors": 2, "minArea": 1.3}));
    assert_eq!(blues(&r), Some(json!(3)), "both specks: {}", r["colors"]);
}

/// Each setting has a row (name, file key, step, what it takes, its default and its source), the
/// reply echoes every setting with its default filled in, and an unknown key or a value out of
/// range is refused, naming the nearest key or the range.
#[test]
fn settings_are_checked_against_their_rows() {
    let mut s = blank();
    image(&mut s, &flat(100, 80, [30, 90, 200, 255]), 0.1);
    let r = dry(&mut s, json!({}));
    let echoed: Vec<&str> = r["settings"].as_object().unwrap().keys().map(String::as_str).collect();
    for row in &SETTINGS {
        assert!(echoed.contains(&row.name), "{} echoed", row.name);
        assert!(!row.source.is_empty(), "{} has a source", row.name);
    }
    assert_eq!(r["settings"]["minArea"], json!(MIN_AREA_MM2));
    assert_eq!(r["settings"]["mode"], json!("color"));
    assert_eq!(r["table"].as_array().unwrap().len(), SETTINGS.len());
    assert!(refused(&mut s, json!({"minAre": 1})).contains("did you mean `minArea`"));
    assert!(refused(&mut s, json!({"minArea": 51})).contains("0 to 50"));
    assert!(refused(&mut s, json!({"minArea": "big"})).contains("0 to 50"));
    assert!(refused(&mut s, json!({"mode": "photo"})).contains("color, logo, grayscale"));
    assert!(refused(&mut s, json!({"smoothing": -1})).contains("0 to 100"));
    // A design's colours are its threads: Janome's palette has 78, Brother's 61.
    assert!(refused(&mut s, json!({"colors": 79})).contains("2 to 78"));
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    assert!(refused(&mut s, json!({"colors": 62})).contains("2 to 61"));
    assert_eq!(Settings::from_params(&serde_json::Map::new(), Palette::Pec).unwrap().colors, super::settings::COLORS);
}

/// Anything but an image, or no image chosen, is refused with what to do.
#[test]
fn only_an_image_is_digitized() {
    let mut s = blank();
    assert!(refused(&mut s, json!({})).contains("select an image"));
    let id = s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap()["id"].clone();
    assert!(refused(&mut s, json!({"id": id})).contains("not an image"));
    let err = s.execute("embroidery.digitizeImage", &json!({"id": id, "dryRun": "yes"})).unwrap_err().to_string();
    assert!(err.contains("true or false"), "{err}");
    let picture = image(&mut s, &flat(10, 10, [0, 0, 0, 255]), 0.1);
    // An Image Trace object digitizes its source image.
    s.execute("imageTrace.make", &json!({"id": picture.0, "preset": "6 Colors"})).unwrap();
    let traced = s.doc().unwrap().selection.objects[0];
    let r = dry(&mut s, json!({"id": traced.0}));
    assert_eq!(r["image"]["pixels"], json!([10, 10]));
}

/// Images that can't be measured or read are refused, never a crash: no pixels, a transform that
/// maps them to nothing or past every number, data that isn't an image, one larger than a metre.
#[test]
fn hostile_images_are_refused() {
    let picture = flat(20, 20, [10, 200, 10, 255]);
    for xf in [Affine::scale(0.0), Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]), Affine::scale(1e300), Affine::scale(1e-300)] {
        let mut s = blank();
        // The document itself refuses some of them.
        if placed_with(&mut s, &picture, xf).is_some() {
            let why = refused(&mut s, json!({}));
            assert!(!why.is_empty(), "{xf:?}");
        }
    }
    // Sizes the file doesn't have, and bytes cut short.
    let mut s = blank();
    let id = image(&mut s, &picture, 0.1);
    s.edit("lie", |d, _| {
        let png = d.images.get("picture").map(|b| b.bytes.to_vec()).unwrap_or_default();
        d.images.insert("picture".into(), ImageBlob::new("image/png", png[..png.len() / 2].to_vec()));
        Ok(())
    })
    .unwrap();
    assert!(refused(&mut s, json!({"id": id.0})).contains("can't be digitized"));
    // An image whose object claims far more pixels than its file has (the document may refuse it
    // first, as past its range).
    let mut s = blank();
    let id = image(&mut s, &picture, 0.1);
    let lied = s.edit("lie", |d, _| {
        if let Some(n) = d.node_mut(id)
            && let NodeKind::Image(i) = &mut n.kind
        {
            i.width = u32::MAX;
            i.height = u32::MAX;
        }
        Ok(())
    });
    if lied.is_ok() {
        let why = refused(&mut s, json!({"id": id.0}));
        assert!(why.contains("mm"), "{why}");
    }
    // Claiming a few more pixels than the file has is measured by the claim, the file read as it is.
    let mut s = blank();
    let id = image(&mut s, &picture, 0.1);
    s.edit("lie", |d, _| {
        if let Some(n) = d.node_mut(id)
            && let NodeKind::Image(i) = &mut n.kind
        {
            i.width = 40;
        }
        Ok(())
    })
    .unwrap();
    let r = dry(&mut s, json!({"id": id.0}));
    assert_eq!(r["image"]["pixels"], json!([40, 20]));
    assert_eq!(r["image"]["decoded"], json!([20, 20]));
}

/// The params description names every key the command takes, so that callers from outside the app
/// (MCP, the control channel), whose keys are checked against it, are refused only what the
/// command itself refuses.
#[test]
fn the_params_description_names_every_setting() {
    let mut all = serde_json::Map::new();
    for key in SETTINGS.iter().map(|row| row.name).chain(super::settings::OTHER_KEYS) {
        all.insert(key.into(), Value::Null);
    }
    crate::cmd::params::check_params("embroidery.digitizeImage", &Value::Object(all)).unwrap();
    assert!(crate::cmd::params::check_params("embroidery.digitizeImage", &json!({"minAreas": 1})).is_err());
}

/// Logo mode's own colours, past the anchor points a trace may make, are traced again in the
/// colours asked, with a warning, not refused; the colours mode is refused past them. The limit
/// lies between the two traces' anchor points: a gradient in its own colours makes more than in 6.
#[test]
fn logo_mode_past_the_anchor_limit_traces_in_the_colours_asked() {
    let gradient = tr::Raster::from_fn(200, 200, |x, y| [x as u8, y as u8, 128, 255]);
    let png = gradient.encode_png();
    let image = ImageObject {
        key: "picture".into(),
        width: 200,
        height: 200,
        xf: Affine::scale(0.1 * PT_PER_MM),
        link: None,
        placement: Default::default(),
        pixels: None,
    };
    let mode = |m: &str| Settings::from_params(json!({ "mode": m }).as_object().unwrap(), Palette::Pec).unwrap();
    let (logo, colours) = (mode("logo"), mode("color"));
    let w = working::prepare(&image, &png, &logo).unwrap();
    let anchors = |p| trace::traced(&w, &logo, logo.colors, p, usize::MAX, &|_, _| true).unwrap().anchor_count();
    let (own, asked) = (anchors(TracePalette::Mode), anchors(TracePalette::Limited));
    assert!(asked < own, "the gradient's own colours make {own} anchor points, 6 colours {asked}");
    let limit = asked + (own - asked) / 2;
    let mut warnings = Vec::new();
    let t = trace::in_mode(&w, &logo, logo.colors, limit, &|_, _| true, &mut warnings).unwrap();
    assert!(t.palette.len() <= logo.colors as usize, "{:?}", t.palette);
    let warned = warnings.iter().find(|w| w.id == "notFlatArt").unwrap_or_else(|| panic!("{warnings:?}"));
    assert_eq!(warned.values, json!({"by": "anchors", "maxAnchors": limit, "colors": logo.colors}));
    assert!(warned.message.contains(&format!("more than {limit} anchor points")), "{warned:?}");
    let why = trace::in_mode(&w, &colours, colours.colors, asked - 1, &|_, _| true, &mut Vec::new()).unwrap_err().to_string();
    assert!(why.contains("anchor points"), "{why}");
}

/// A crop at the far end of an image as wide as a u32 holds is measured from its own centre, no
/// sum of its corners overflowing: answered or refused with a reason, never a panic.
#[test]
fn a_crop_at_the_end_of_the_widest_image_is_answered() {
    let mut s = blank();
    let id = image_with(&mut s, &flat(4, 4, [20, 40, 120, 255]), Affine::scale(1e-4));
    s.edit("widen", |d, _| {
        if let Some(NodeKind::Image(img)) = d.node_mut(id).map(|n| &mut n.kind) {
            img.width = u32::MAX;
        }
        Ok(())
    })
    .unwrap();
    let r = s.execute("embroidery.digitizeImage", &json!({"dryRun": true, "crop": [u32::MAX - 2, 0, u32::MAX, 1]}));
    assert!(!matches!(r, Err(crate::EngineError::Internal { .. })), "{r:?}");
}

/// A small crop of a large image made much larger would carry the rest of the image past the
/// canvas when the image moves under the design: Create is refused, saying what to do, and the
/// dry run with it, so that it never answers what Create refuses.
#[test]
fn the_image_scaled_past_the_canvas_is_refused() {
    let mut s = blank();
    image(&mut s, &tr::Raster::from_fn(6000, 20, |x, _| if x < 3 { [20, 40, 120, 255] } else { [240, 130, 20, 255] }), 0.1);
    for dry in [true, false] {
        let p = json!({"dryRun": dry, "crop": [0, 0, 4, 4], "width": 1000, "background": "none"});
        let why = s.execute("embroidery.digitizeImage", &p).unwrap_err().to_string();
        assert!(why.contains("past the canvas"), "{why}");
    }
}

/// A linked picture whose file is missing is refused, asking for it to be relinked, rather than
/// digitized from the preview the document keeps of it.
#[test]
fn a_missing_linked_file_is_refused() {
    let mut s = blank();
    let preview = flat(256, 171, [20, 40, 120, 255]).encode_png();
    let preview = std::sync::Arc::new(preview);
    let id = s
        .edit("place", |d, sel| {
            d.images.insert("logo".into(), ImageBlob { mime: "image/png".into(), bytes: preview.clone(), proxy: Some(preview.clone()) });
            let id = d.alloc_id();
            let layer = d.default_layer();
            let link = Some(vectorcraft_doc::LinkInfo::new("/nowhere/logo.png"));
            let image = ImageObject {
                key: "logo".into(),
                width: 3000,
                height: 2000,
                xf: Affine::scale(0.1 * PT_PER_MM),
                link,
                placement: Default::default(),
                pixels: None,
            };
            d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
            sel.set([id]);
            Ok(id)
        })
        .unwrap();
    let why = refused(&mut s, json!({"id": id.0}));
    assert!(why.contains("linked file is missing") && why.contains("links.relink"), "{why}");
}
