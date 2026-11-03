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

/// A PNG claiming more pixels than any machine holds, and files cut short, are refused or never
/// placed: never a panic.
#[test]
fn files_that_lie_are_refused() {
    for (w, h) in [(u32::MAX, u32::MAX), (1 << 20, 1 << 20), (1, u32::MAX), (0, 10)] {
        if let Some(mut s) = placed("claims.png", &png_claiming(w, h)) {
            assert!(digitized(&mut s, json!({})).is_err(), "{w} × {h}");
        }
    }
    let png = logo(200, 100).encode_png();
    let jpg = jpeg(&logo(200, 100));
    for (name, bytes) in [("cut.png", &png[..png.len() / 2]), ("cut.jpg", &jpg[..jpg.len() / 2]), ("cut.png", &png[..40])] {
        if let Some(mut s) = placed(name, bytes) {
            let _ = digitized(&mut s, json!({}));
        }
    }
}

/// Noise makes more areas than a design is made of: refused with what to change. With the
/// smallest area raised it is digitized, the colours asked at most.
#[test]
fn noise_is_refused_with_advice() {
    let mut s = placed("noise.png", &noise(400, 400).encode_png()).unwrap();
    let why = digitized(&mut s, json!({"colors": 16, "minArea": 0, "smoothing": 0})).unwrap_err();
    assert!(why.contains("raise `minArea`"), "{why}");
    let r = digitized(&mut s, json!({"colors": 4, "minArea": 20})).unwrap();
    assert!(r["colorCount"].as_u64().unwrap() <= 4, "{}", r["colors"]);
}

/// Whether reply `r` warns that the picture doesn't look like flat art.
fn warns_of_gradients(r: &Value) -> bool {
    r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("doesn't look like flat art"))
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
