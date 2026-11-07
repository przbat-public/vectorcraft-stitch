//! Digitizing from an image (`embroidery.digitizeImage`) on images as hostile as a file can make
//! them, each written here by the tests' own code (the command sweep's fixtures hold no image): a
//! PNG claiming more pixels than any machine holds, files cut short, noise that makes more areas
//! than a design has, a photo's gradients, one pixel, a line one pixel high, a picture larger than
//! a metre as placed, nothing but transparency. Placed as a person places a file (`file.place`),
//! each is answered by a dry run and by Create with a reply or with a refusal that says what to do,
//! never a panic; what Create makes is what the dry run answered, and saves and reads back as it
//! was.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_testkit::invariants::check_native_roundtrip;
use vectorcraft_trace::Raster;

const PT_PER_MM: f64 = 72.0 / 25.4;
const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// `r`, after failing the test if the command panicked: the engine's guard reports a panic as
/// `EngineError::Internal`.
fn no_panic(r: Result<Value, EngineError>) -> Result<Value, EngineError> {
    if let Err(EngineError::Internal { cmd, msg }) = &r {
        panic!("{cmd} panicked: {msg}");
    }
    r
}

/// A session with file `name` (its `bytes`) placed as File › Place places it, and selected;
/// `None` when placing refuses it.
fn placed(name: &str, bytes: &[u8]) -> Option<Session> {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    no_panic(s.execute("file.place", &json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes)}))).ok()?;
    Some(s)
}

/// A PNG chunk: its length, type, data and CRC.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

/// A PNG whose header claims `width` × `height` pixels of 8-bit RGBA, with no pixel data to
/// back them.
fn png_claiming(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits a sample, RGBA, deflate, adaptive filtering, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    // An empty zlib stream: no pixels at all.
    chunk(&mut out, b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A JPEG of `r` (opaque), as the `image` crate writes one.
fn jpeg(r: &Raster) -> Vec<u8> {
    let rgb: Vec<u8> = r.rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode(&rgb, r.width, r.height, image::ExtendedColorType::Rgb8).unwrap();
    out
}

/// Noise: each pixel one of 16 colours, by a fixed hash of its place.
fn noise(width: u32, height: u32) -> Raster {
    Raster::from_fn(width, height, |x, y| {
        let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761) >> 28;
        let v = (h * 17) as u8;
        [v, 255 - v, v / 2, 255]
    })
}

/// An orange square with a navy bar across it, on white.
fn logo(width: u32, height: u32) -> Raster {
    Raster::from_fn(width, height, |x, y| {
        let (fx, fy) = (f64::from(x) / f64::from(width), f64::from(y) / f64::from(height));
        if (0.1..0.9).contains(&fx) && (0.45..0.55).contains(&fy) {
            NAVY
        } else if (0.1..0.9).contains(&fx) && (0.1..0.9).contains(&fy) {
            ORANGE
        } else {
            WHITE
        }
    })
}

/// The command on `s` with `params`, as a dry run and then as Create: each a reply or a refusal
/// with a reason, never a panic. When both answer, Create made what the dry run answered, and the
/// document saves and reads back as it was. The dry run's reply, or its refusal.
fn digitized(s: &mut Session, params: Value) -> Result<Value, String> {
    let mut dry = params.clone();
    dry["dryRun"] = json!(true);
    let answered = no_panic(s.execute("embroidery.digitizeImage", &dry)).map_err(|e| e.to_string());
    let made = no_panic(s.execute("embroidery.digitizeImage", &params)).map_err(|e| e.to_string());
    for r in [&answered, &made] {
        if let Err(why) = r {
            assert!(why.len() > "embroidery.digitizeImage: ".len(), "a refusal says why: {why}");
        }
    }
    if let (Ok(a), Ok(m)) = (&answered, &made) {
        assert_eq!(a["stats"], m["stats"], "Create made what the dry run answered");
        check_native_roundtrip(&s.doc().unwrap().doc).unwrap();
    }
    answered
}

/// A document holding `bytes` as an image claiming `width` × `height` pixels, a tenth of a
/// millimetre each, selected: what another program may write where File › Place would refuse the
/// file. `None` when the document refuses to hold it.
fn holding(bytes: &[u8], width: u32, height: u32) -> Option<Session> {
    held(bytes, width, height, false)
}

/// [`holding`], the image linked to a file that is missing (`missing`): the document keeps only
/// its preview, `bytes`.
fn held(bytes: &[u8], width: u32, height: u32, missing: bool) -> Option<Session> {
    use vectorcraft_doc::{ImageBlob, ImageObject, LinkInfo, Node, NodeKind};
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    let bytes = std::sync::Arc::new(bytes.to_vec());
    let placed = s.edit("place", |d, sel| {
        let proxy = missing.then(|| bytes.clone());
        d.images.insert("picture".into(), ImageBlob { mime: "image/png".into(), bytes: bytes.clone(), proxy });
        let id = d.alloc_id();
        let xf = vectorcraft_geom::Affine::scale(0.1 * PT_PER_MM);
        let link = missing.then(|| LinkInfo::new("/nowhere/picture.png"));
        let image = ImageObject { key: "picture".into(), width, height, xf, link, placement: Default::default(), pixels: None };
        d.insert(d.default_layer(), 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    });
    placed.ok().map(|_| s)
}

/// Files that lie, each with its own answer. File › Place takes a PNG claiming a million pixels a
/// side, which the command refuses as more than Image Trace takes, and a JPEG cut short, digitized
/// as far as it decodes. What File › Place refuses a document may still hold (another program
/// wrote it): one claiming no pixels, and a PNG cut short, the command refuses with why; one
/// claiming another size than its pixels is answered; one claiming more pixels than a document
/// holds, the document refuses.
#[test]
fn files_that_lie_are_answered_each_with_its_reason() {
    let mut s = placed("claims.png", &png_claiming(1 << 20, 1 << 20)).expect("File › Place takes it");
    let why = digitized(&mut s, json!({"width": 100})).unwrap_err();
    assert!(why.contains("more than Image Trace takes"), "{why}");
    let jpg = jpeg(&logo(200, 100));
    let mut s = placed("cut.jpg", &jpg[..jpg.len() / 2]).expect("File › Place takes it");
    digitized(&mut s, json!({})).unwrap();
    let png = logo(200, 100).encode_png();
    for (name, bytes, w, h, said) in [
        ("empty.png", png_claiming(0, 10), 0, 10, "the image has no pixels"),
        ("cut.png", png[..png.len() / 2].to_vec(), 200, 100, "could not decode image"),
        ("cut.png", png[..40].to_vec(), 200, 100, "could not decode image"),
    ] {
        assert!(placed(name, &bytes).is_none(), "{name}: File › Place refuses it");
        let why = digitized(&mut holding(&bytes, w, h).unwrap(), json!({})).unwrap_err();
        assert!(why.contains(said), "{name}: {why}");
    }
    digitized(&mut holding(&png, 5, 5).unwrap(), json!({})).unwrap();
    for (w, h) in [(u32::MAX, u32::MAX), (1, u32::MAX)] {
        assert!(placed("claims.png", &png_claiming(w, h)).is_none() && holding(&png_claiming(w, h), w, h).is_none(), "{w} × {h}");
    }
}

/// Each cap is reached through the command with its own reason and what changes it: noise of 800
/// pixels a side traces as more shapes than the trace may make (250,000), of 1,200 a side as more
/// anchor points (2,000,000); more areas than a design is made of is the next test's.
#[test]
fn each_cap_is_reached_with_its_own_reason() {
    for (side, why, advice) in
        [(800u32, "shapes, more than 250000", "lower `colors`"), (1200, "more than 2000000 anchor points", "raise `smoothing`")]
    {
        let mut s = placed("noise.png", &noise(side, side).encode_png()).unwrap();
        let said = digitized(&mut s, json!({"width": f64::from(side) / 10.0, "colors": 16, "minArea": 0, "smoothing": 0})).unwrap_err();
        assert!(said.contains(why) && said.contains(advice), "{side} pixels a side: {said}");
    }
}

/// Junk parameters on broken images, as the command sweep's image tests give them: on a JPEG cut
/// short, a PNG claiming a million pixels a side, a PNG cut short, one claiming no pixels and a
/// linked file that is missing, each answered or refused with why, never a panic.
#[test]
fn junk_params_on_broken_images_are_answered() {
    let jpg = jpeg(&logo(200, 100));
    let png = logo(200, 100).encode_png();
    let params = [
        json!({}),
        json!({"id": u64::MAX, "dryRun": true}),
        json!({"dryRun": 1, "width": 1e308, "crop": [0, 0, u32::MAX, u32::MAX]}),
        json!({"fit": [f64::NAN, -1], "colors": 1e308, "minArea": -1, "smoothing": "smooth"}),
        json!({"colorChoices": [{"color": "#zzzzzz", "thread": {"palette": "pec", "index": u64::MAX}}, null, 5]}),
        json!({"crop": [10, 10, 5, 5], "at": [1e308, f64::NAN], "overlap": 1e308, "mode": "photo", "background": [1]}),
        json!({"offset": u64::MAX, "limit": u64::MAX, "regions": "yes", "outlines": [true], "height": -1e-300}),
        json!({"dryRun": true, "width": 1e-9, "overlap": 2, "minArea": 50, "colors": 2}),
        json!({"dryRun": true, "crop": [0, 0, 1, 1], "at": [0, 0], "regions": true, "outlines": true, "limit": 0}),
    ];
    for p in &params {
        let sessions = [
            ("cut.jpg", placed("cut.jpg", &jpg[..jpg.len() / 2])),
            ("claims.png", placed("claims.png", &png_claiming(1 << 20, 1 << 20))),
            ("cut.png", holding(&png[..png.len() / 2], 200, 100)),
            ("empty.png", holding(&png_claiming(0, 10), 0, 10)),
            ("missing.png", held(&png, 200, 100, true)),
        ];
        for (name, s) in sessions {
            let mut s = s.unwrap_or_else(|| panic!("{name}"));
            if let Err(e) = no_panic(s.execute("embroidery.digitizeImage", p)) {
                assert!(e.to_string().len() > "embroidery.digitizeImage: ".len(), "{name} {p}: a refusal says why: {e}");
            }
        }
    }
}

/// Many islands, many slivers and many holes are answered, Create making what the dry run said:
/// 400 dots in a field, 160 bands anti-aliasing paints between stripes, and a field round 400 holes
/// of transparency (white inside it is sewn: the background is only what touches the image's
/// edge).
#[test]
fn many_islands_slivers_and_holes_are_answered() {
    let dots = Raster::from_fn(600, 600, |x, y| if (x % 30) >= 10 && (x % 30) < 22 && (y % 30) >= 10 && (y % 30) < 22 { ORANGE } else { NAVY });
    let r = digitized(&mut placed("dots.png", &dots.encode_png()).unwrap(), json!({"colors": 2, "background": "none"})).unwrap();
    assert!(r["areaCount"].as_u64().unwrap() > 400, "{}", r["areaCount"]);
    let stripes = Raster::from_fn(800, 300, |x, _| match x % 10 {
        0 => [130, 85, 70, 255],
        1..5 => ORANGE,
        5 => [130, 85, 70, 255],
        _ => NAVY,
    });
    let r = digitized(&mut placed("stripes.png", &stripes.encode_png()).unwrap(), json!({"colors": 3, "background": "none"})).unwrap();
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w["id"] == "bandsJoined"), "{}", r["warnings"]);
    let holes = Raster::from_fn(600, 600, |x, y| {
        let edge = !(20..580).contains(&x) || !(20..580).contains(&y);
        if edge || ((x % 28) >= 10 && (x % 28) < 20 && (y % 28) >= 10 && (y % 28) < 20) { CLEAR } else { NAVY }
    });
    let r = digitized(&mut placed("holes.png", &holes.encode_png()).unwrap(), json!({"colors": 2})).unwrap();
    assert_eq!(r["areaCount"], json!(1), "one field, its holes transparent: {}", r["areaCount"]);
}

/// Line art at the largest working size (1,500 pixels a side at 10 a millimetre) is answered, its
/// lines sewn along their middle, Create making what the dry run said: 75 hatch lines 0.6 mm wide
/// and 150 mm long, and a mesh of such lines every 2 mm inside a transparent margin of 1 mm (one
/// line with about 74 × 74 junctions, its holes transparent; with no margin, three of its corners would
/// be navy, the background).
#[test]
fn line_art_at_the_largest_size_is_answered() {
    let hatch = Raster::from_fn(1500, 1500, |_, y| if y % 20 < 6 { NAVY } else { CLEAR });
    let inner = |v: u32| (10..1490).contains(&v);
    let mesh = Raster::from_fn(1500, 1500, |x, y| if inner(x) && inner(y) && (x % 20 < 6 || y % 20 < 6) { NAVY } else { CLEAR });
    for (name, picture, lines, junctions) in [("hatch.png", hatch, 75, 0), ("mesh.png", mesh, 1, 74 * 74)] {
        let mut s = placed(name, &picture.encode_png()).unwrap();
        let r = digitized(&mut s, json!({"colors": 2, "width": 150})).unwrap();
        let areas = r["areas"].as_array().unwrap();
        let found: Vec<&Value> = areas.iter().filter(|a| !a["line"].is_null()).collect();
        assert_eq!(found.len(), lines, "{name}: {} areas", areas.len());
        let at_junctions: u64 = found.iter().map(|a| a["line"]["junctions"].as_u64().unwrap()).sum();
        assert!(at_junctions.abs_diff(junctions) <= junctions / 50, "{name}: {at_junctions} junctions");
    }
}

/// Noise makes more areas than a design is made of: refused by that cap, with what changes it.
/// With the smallest area raised it is digitized, the colours asked at most.
#[test]
fn noise_is_refused_with_advice() {
    let mut s = placed("noise.png", &noise(400, 400).encode_png()).unwrap();
    let why = digitized(&mut s, json!({"colors": 16, "minArea": 0, "smoothing": 0})).unwrap_err();
    assert!(why.contains("areas, more than 10000") && why.contains("raise `minArea`"), "{why}");
    let r = digitized(&mut s, json!({"colors": 4, "minArea": 20})).unwrap();
    assert!(r["colorCount"].as_u64().unwrap() <= 4, "{}", r["colors"]);
}

/// Whether reply `r` warns that the picture doesn't look like flat art.
fn warns_of_gradients(r: &Value) -> bool {
    r["warnings"].as_array().unwrap().iter().any(|w| w["id"] == "notFlatArt" && w["message"].as_str().unwrap().contains("doesn't look like flat art"))
}

/// A photo's gradients are digitized with a warning in every mode, never refused; flat art, as a
/// PNG and as a JPEG, is digitized with none.
#[test]
fn a_photo_is_warned_about_in_every_mode() {
    let gradient = Raster::from_fn(240, 160, |x, y| [(x % 256) as u8, (y % 256) as u8, 128, 255]);
    for mode in ["color", "grayscale", "logo"] {
        let mut s = placed("photo.jpg", &jpeg(&gradient)).unwrap();
        let r = digitized(&mut s, json!({"mode": mode})).unwrap();
        assert!(warns_of_gradients(&r), "{mode}: {}", r["warnings"]);
    }
    let flat = logo(400, 200);
    for (name, bytes) in [("logo.png", flat.encode_png()), ("logo.jpg", jpeg(&flat))] {
        for mode in ["color", "grayscale", "logo"] {
            let mut s = placed(name, &bytes).unwrap();
            let r = digitized(&mut s, json!({"mode": mode})).unwrap();
            assert!(!warns_of_gradients(&r), "{name} in {mode}: {}", r["warnings"]);
        }
    }
}

/// One pixel, a line one pixel high and nothing but transparency are answered (the last with
/// nothing to sew).
#[test]
fn odd_pictures_are_answered() {
    for (name, picture) in
        [("dot.png", Raster::from_fn(1, 1, |_, _| NAVY)), ("line.png", Raster::from_fn(3000, 1, |x, _| if x % 2 == 0 { NAVY } else { ORANGE }))]
    {
        let mut s = placed(name, &picture.encode_png()).unwrap();
        let _ = digitized(&mut s, json!({}));
    }
    let mut s = placed("clear.png", &Raster::from_fn(50, 50, |_, _| CLEAR).encode_png()).unwrap();
    let r = digitized(&mut s, json!({})).unwrap();
    assert_eq!(r["areaCount"], json!(0));
}

/// A picture larger than a metre as placed (3,000 pixels at 72 a inch) is refused unless a size is
/// given; made 200 mm wide it is worked at 1,500 pixels on its long side.
#[test]
fn a_picture_past_a_metre_needs_a_size() {
    let mut s = placed("large.png", &logo(3000, 2000).encode_png()).unwrap();
    let why = digitized(&mut s, json!({})).unwrap_err();
    assert!(why.contains("width"), "{why}");
    let r = digitized(&mut s, json!({"width": 200})).unwrap();
    assert_eq!((&r["image"]["worked"], &r["image"]["limitedBy"]), (&json!([1500, 1000]), &json!("side")));
}

/// `r` written as `format` by the `image` crate, its pixels as they are (`kind` RGBA, 16 bits a
/// channel, or grey with alpha).
fn written(r: &Raster, format: image::ImageFormat, kind: &str) -> Vec<u8> {
    let rgba = image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(r.width, r.height, r.rgba.clone()).unwrap());
    let img = match kind {
        "rgba16" => image::DynamicImage::ImageRgba16(rgba.to_rgba16()),
        "grey" => image::DynamicImage::ImageLumaA8(rgba.to_luma_alpha8()),
        _ => rgba,
    };
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), format).unwrap();
    out
}

/// What a dry run found, for comparing two files of one picture: the colours and the areas' boxes.
fn found(r: &Value) -> (Value, Vec<Value>) {
    let colours = json!(r["colors"].as_array().unwrap().iter().map(|c| c["color"].clone()).collect::<Vec<_>>());
    (colours, r["areas"].as_array().unwrap().iter().map(|a| a["box"].clone()).collect())
}

/// The same picture as a PNG of 16 bits a channel, a lossless WebP, a TIFF and a BMP is digitized
/// exactly as its PNG is; as a GIF (its colours chosen by the GIF writer) and in greys with alpha
/// it is digitized too.
#[test]
fn other_formats_are_digitized_as_the_png_is() {
    let picture = logo(200, 100);
    let params = json!({"colors": 3});
    let mut s = placed("logo.png", &picture.encode_png()).unwrap();
    let png = digitized(&mut s, params.clone()).unwrap();
    for (name, format, kind) in [
        ("logo16.png", image::ImageFormat::Png, "rgba16"),
        ("logo.webp", image::ImageFormat::WebP, "rgba"),
        ("logo.tif", image::ImageFormat::Tiff, "rgba"),
        ("logo.bmp", image::ImageFormat::Bmp, "rgba"),
    ] {
        let mut s = placed(name, &written(&picture, format, kind)).unwrap_or_else(|| panic!("{name} isn't placed"));
        let r = digitized(&mut s, params.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(found(&r), found(&png), "{name}");
    }
    for (name, format, kind) in [("logo.gif", image::ImageFormat::Gif, "rgba"), ("logo-grey.png", image::ImageFormat::Png, "grey")] {
        let mut s = placed(name, &written(&picture, format, kind)).unwrap_or_else(|| panic!("{name} isn't placed"));
        let r = digitized(&mut s, params.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r["areaCount"].as_u64().unwrap() >= 2, "{name}: {}", r["areas"]);
    }
}

/// `jpeg` with an EXIF segment (APP1) right after its start of image, saying its orientation is
/// `orientation` (tag 0x0112, a SHORT in IFD0, as the EXIF specification lays them out, in a
/// big-endian TIFF structure).
fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
    // The TIFF header (big-endian, IFD0 at offset 8), one entry, no next IFD.
    let mut exif = b"Exif\0\0MM\0\x2a\0\0\0\x08".to_vec();
    exif.extend_from_slice(&1u16.to_be_bytes());
    exif.extend_from_slice(&0x0112u16.to_be_bytes());
    exif.extend_from_slice(&3u16.to_be_bytes());
    exif.extend_from_slice(&1u32.to_be_bytes());
    exif.extend_from_slice(&orientation.to_be_bytes());
    exif.extend_from_slice(&[0, 0]);
    exif.extend_from_slice(&0u32.to_be_bytes());
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    out.extend(exif);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A photo whose EXIF orientation turns it a quarter clockwise (orientation 6, a camera held
/// upright) is digitized as it is seen, where the canvas shows it: stored 100 pixels wide and 200
/// tall, orange above navy, it is seen 200 wide, the orange on the right.
#[test]
fn a_photo_turned_by_its_exif_orientation_is_digitized_as_seen() {
    let stored = Raster::from_fn(100, 200, |_, y| if y < 100 { ORANGE } else { NAVY });
    let mut s = placed("turned.jpg", &with_orientation(&jpeg(&stored), 6)).unwrap();
    let r = digitized(&mut s, json!({"colors": 2, "background": "none"})).unwrap();
    assert_eq!(r["image"]["pixels"], json!([200, 100]));
    let bounds: Vec<f64> = r["image"]["bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let middle = (bounds[0] + bounds[2]) / 2.0;
    // The area nearest orange in colour, and its box.
    let rgb = |hex: &str| (1..7).step_by(2).map(|k| i64::from_str_radix(&hex[k..k + 2], 16).unwrap()).collect::<Vec<_>>();
    let far = |a: &Value| rgb(a["color"].as_str().unwrap()).iter().zip(ORANGE).map(|(c, o)| (c - i64::from(o)).pow(2)).sum::<i64>();
    let orange = r["areas"].as_array().unwrap().iter().min_by_key(|a| far(a)).unwrap();
    let left = orange["box"][0].as_f64().unwrap();
    assert!((left - middle).abs() < 1.0, "the orange on the right half: {} in {bounds:?}", orange["box"]);
}

/// The top-level layer object `id` lies in, and whether it is sewn: a template, shown, locked,
/// printed.
fn layer_of(s: &Session, id: u64) -> (u64, [bool; 4]) {
    let st = s.doc().unwrap();
    let layer = st.doc.layer_of(vectorcraft_doc::NodeId(id)).unwrap();
    let n = st.doc.node(layer).unwrap();
    let vectorcraft_doc::NodeKind::Layer { template, printable, .. } = n.kind else { panic!("{layer:?} isn't a layer") };
    (layer.0, [template, n.visible, n.locked, printable])
}

/// The flags of a layer whose objects are sewn: no template, shown, unlocked, printed.
const SEWN_LAYER: [bool; 4] = [false, true, false, true];

/// How many top-level layers the document has.
fn layer_count(s: &Session) -> usize {
    s.doc().unwrap().doc.children(None).unwrap().len()
}

/// Create on `s` with `params` after its dry run: the two agree, and the design is sewn, on a layer
/// that is. The design's group id and Create's reply.
fn created_sewn(s: &mut Session, params: Value) -> (u64, Value) {
    let mut dry = params.clone();
    dry["dryRun"] = json!(true);
    let answered = s.execute("embroidery.digitizeImage", &dry).unwrap();
    let made = s.execute("embroidery.digitizeImage", &params).unwrap();
    assert_eq!(answered["stats"], made["stats"], "Create made what the dry run answered");
    assert!(made["stats"]["stitches"].as_u64().unwrap() > 0, "{}", made["stats"]);
    let group = made["created"]["group"].as_u64().unwrap();
    assert_eq!(layer_of(s, group).1, SEWN_LAYER, "the design's layer");
    (group, made)
}

/// A picture placed as a template (on a locked template layer of its own, as File › Place › Template
/// puts it) makes a design that is sewn, on the layer above; the picture stays on its template layer
/// and no other is made.
#[test]
fn a_picture_on_a_template_layer_makes_a_design_that_is_sewn() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 600, "height": 600, "artboards": 1})).unwrap();
    let data = vectorcraft_format::base64_encode(&logo(200, 100).encode_png());
    let placed = s.execute("file.place", &json!({"name": "logo.png", "dataBase64": data, "template": true})).unwrap();
    let image = placed["ids"][0].as_u64().unwrap();
    let (template, flags) = layer_of(&s, image);
    assert!(flags[0] && flags[2], "placed on a locked template layer: {flags:?}");
    let layers = layer_count(&s);
    let (_, made) = created_sewn(&mut s, json!({"id": image, "colors": 3}));
    assert_eq!(layer_of(&s, image).0, template, "the picture stays on its template layer");
    assert_eq!(made["created"]["layer"], json!(template));
    assert_eq!(layer_count(&s), layers, "no layer made");
}

/// The image Create moved to its template layer, digitized again, makes a design that is sewn too;
/// the image stays on that template layer.
#[test]
fn the_image_create_moved_digitizes_again_into_a_design_that_is_sewn() {
    let mut s = placed("logo.png", &logo(200, 100).encode_png()).unwrap();
    let (_, first) = created_sewn(&mut s, json!({"colors": 3}));
    let image = first["image"]["id"].as_u64().unwrap();
    let template = first["created"]["layer"].as_u64().unwrap();
    let layers = layer_count(&s);
    let (_, again) = created_sewn(&mut s, json!({"id": image, "colors": 4}));
    assert_eq!(layer_of(&s, image).0, template);
    assert_eq!(again["created"]["layer"], json!(template));
    assert_eq!(layer_count(&s), layers, "no second template layer");
}

/// A picture on a layer that doesn't print, or is hidden, makes its design on the nearest layer
/// above that is sewn, or on a new layer when none is; the picture stays where it is.
#[test]
fn a_picture_on_a_layer_not_sewn_makes_its_design_on_one_that_is() {
    for props in [json!({"printable": false}), json!({"visible": false})] {
        for above in [true, false] {
            let mut s = placed("logo.png", &logo(200, 100).encode_png()).unwrap();
            let image = s.doc().unwrap().selection.objects[0].0;
            let (own, _) = layer_of(&s, image);
            let upper = above.then(|| s.execute("layer.new", &json!({"top": true})).unwrap()["id"].as_u64().unwrap());
            let mut set = props.clone();
            set["id"] = json!(own);
            s.execute("layer.setProps", &set).unwrap();
            let layers = layer_count(&s);
            let (group, made) = created_sewn(&mut s, json!({"id": image, "colors": 3}));
            assert_eq!(layer_of(&s, image).0, own, "{props}: the picture stays");
            assert_eq!(made["created"]["layer"], json!(own));
            match upper {
                Some(upper) => assert_eq!((layer_of(&s, group).0, layer_count(&s)), (upper, layers), "{props}: on the layer above"),
                None => assert_eq!(layer_count(&s), layers + 1, "{props}: on a new layer"),
            }
        }
    }
}

/// A locked image, an image on a locked layer and a run while Global Edit is on are refused,
/// nothing changed.
#[test]
fn a_locked_image_and_global_edit_are_refused() {
    let lock_image = |s: &mut Session, _: u64| {
        s.execute("object.lock", &json!({})).unwrap();
    };
    let lock_layer = |s: &mut Session, image: u64| {
        let (layer, _) = layer_of(s, image);
        s.execute("layer.setProps", &json!({"id": layer, "locked": true})).unwrap();
    };
    for lock in [&lock_image as &dyn Fn(&mut Session, u64), &lock_layer] {
        let mut s = placed("logo.png", &logo(200, 100).encode_png()).unwrap();
        let image = s.doc().unwrap().selection.objects[0].0;
        lock(&mut s, image);
        let steps = s.doc().unwrap().history.undo.len();
        for dry in [true, false] {
            let why = no_panic(s.execute("embroidery.digitizeImage", &json!({"id": image, "dryRun": dry}))).unwrap_err().to_string();
            assert!(why.contains("unlock it first"), "{why}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), steps, "nothing changed");
    }
    // Global Edit works on paths alike: started on one of two squares, the image named by its id.
    let mut s = placed("logo.png", &logo(200, 100).encode_png()).unwrap();
    let image = s.doc().unwrap().selection.objects[0].0;
    let square = s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 20, "height": 20})).unwrap()["id"].clone();
    s.execute("shape.rectangle", &json!({"x": 40, "y": 10, "width": 20, "height": 20})).unwrap();
    s.execute("select.set", &json!({"ids": [square]})).unwrap();
    s.execute("select.globalEdit", &json!({"artboards": "all", "includeCanvas": true})).unwrap();
    assert!(s.doc().unwrap().global_edit.is_some());
    match s.execute("embroidery.digitizeImage", &json!({"id": image})) {
        Err(EngineError::Disabled(_, why)) => assert!(why.contains("Global Edit"), "{why}"),
        r => panic!("{r:?}"),
    }
}
