//! Documents digitized from images, against hashes of their files' bytes: scenes drawn here pixel by
//! pixel (each pixel the mean of a grid of samples, so that edges are anti-aliased as a picture's
//! are, and every system holds the same pixels), each placed at 10 pixels a millimetre, digitized
//! by Create, and exported in all eight formats; the dry run's reply is hashed too, its timings left
//! out. A change that moves a stitch of a design digitized from an image fails here, by its line.
//!
//! The scenes: a badge of four colours on white, and the same badge as a JPEG of quality 55; a
//! disc's two halves touching, on transparency; a letter O, its counter sewn white; specks of one
//! to four pixels on a field; a sharp edge between navy and yellow, whose anti-aliasing is no third
//! colour; two squares side by side at an overlap of 0, 0.2 and 0.4 mm; a logo on a cream scan
//! lit unevenly. The badge again in logo mode, in greys, with a colour sewn as Fill and one not
//! sewn, cropped to its middle, and made 25 mm wide. Line art (a T, an X, a ring and hatch lines)
//! drawn 0.6 mm wide, sewn along its middle by the rules, and drawn 2 mm wide with its colour set
//! to Lines.
//!
//! - Every scene digitized twice gives the same bytes.
//! - Every scene exports as recorded.
//!
//! Computed with one maths library on every system (`one_maths`, the `libm` crate in place of the
//! system's `sin`, `pow`…), so that the hashes are the same on macOS, Linux and FreeBSD: the
//! traced curves and the stitches planned from them otherwise differ in the last bit between the
//! systems' libraries.
//!
//! To record the hashes again (only for a change meant to move the stitches of designs digitized
//! from images, said in its commit):
//! `VECTORCRAFT_IMAGE_LISTING_RECORD=1 cargo test -p vectorcraft-engine --test embroidery_from_image_listing`.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_trace::Raster;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
mod one_maths;

/// The file the hashes are kept in, beside this one.
const LISTING: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/embroidery_from_image_listing.txt");

/// The formats every scene is exported in.
const FORMATS: [&str; 8] = ["dst", "tap", "t01", "exp", "pes", "pec", "jef", "vp3"];

/// The scenes' resolution (pixels a millimetre): the working resolution digitizing aims for.
const PX_PER_MM: f64 = 10.0;
const PT_PER_MM: f64 = 72.0 / 25.4;

/// The samples a pixel is the mean of, along each axis.
const SAMPLES: u32 = 4;

/// How wide the line art's lines are drawn (pixels): 0.6 mm, a centre line by the rules; and 2 mm,
/// sewn along their middle only as a colour set to Lines.
const THIN_LINE_PX: f64 = 6.0;
const WIDE_LINE_PX: f64 = 20.0;

/// The JPEG quality the badge is also written at: a heavily compressed picture.
const JPEG_QUALITY: u8 = 55;

/// FNV-1a 64's offset basis and prime, from its published definition.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const NAVY: [u8; 4] = [29, 42, 107, 255];
const GOLD: [u8; 4] = [232, 177, 0, 255];
const RED: [u8; 4] = [210, 32, 48, 255];
const ORANGE: [u8; 4] = [240, 122, 26, 255];
const YELLOW: [u8; 4] = [250, 220, 40, 255];
const CREAM: [u8; 4] = [245, 238, 220, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

/// A picture `width` × `height` pixels of what `at` paints at each point (pixels), each pixel the
/// mean of [`SAMPLES`]² samples, weighed by their opacity.
fn drawn(width: u32, height: u32, at: impl Fn(f64, f64) -> [u8; 4]) -> Raster {
    Raster::from_fn(width, height, |x, y| {
        let mut sum = [0u32; 4];
        for j in 0..SAMPLES {
            for i in 0..SAMPLES {
                let p = at(f64::from(x) + (f64::from(i) + 0.5) / f64::from(SAMPLES), f64::from(y) + (f64::from(j) + 0.5) / f64::from(SAMPLES));
                let a = u32::from(p[3]);
                for (s, c) in sum.iter_mut().zip(&p[..3]) {
                    *s += u32::from(*c) * a;
                }
                sum[3] += a;
            }
        }
        let n = SAMPLES * SAMPLES;
        match sum[3] {
            0 => CLEAR,
            a => [((sum[0] + a / 2) / a) as u8, ((sum[1] + a / 2) / a) as u8, ((sum[2] + a / 2) / a) as u8, ((a + n / 2) / n) as u8],
        }
    })
}

/// Whether `(x, y)` lies inside the polygon `points` (even-odd).
fn in_polygon(points: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    for (k, &(ax, ay)) in points.iter().enumerate() {
        let (bx, by) = points[(k + 1) % points.len()];
        if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
            inside = !inside;
        }
    }
    inside
}

/// A star of five points round `(cx, cy)`, its points `outer` and its notches `inner` from it.
fn star(cx: f64, cy: f64, outer: f64, inner: f64) -> Vec<(f64, f64)> {
    (0..10)
        .map(|k| {
            let r = if k % 2 == 0 { outer } else { inner };
            let a = std::f64::consts::PI * f64::from(k) / 5.0 - std::f64::consts::FRAC_PI_2;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// The badge: a navy disc ringed in gold, a red star, an orange bar, on white.
fn badge() -> Raster {
    let points = star(200.0, 140.0, 70.0, 30.0);
    drawn(400, 300, move |x, y| {
        let r = ((x - 200.0).powi(2) + (y - 150.0).powi(2)).sqrt();
        if (240.0..262.0).contains(&y) && (120.0..280.0).contains(&x) {
            ORANGE
        } else if in_polygon(&points, x, y) {
            RED
        } else if r < 100.0 {
            NAVY
        } else if r < 118.0 {
            GOLD
        } else if r < 130.0 {
            NAVY
        } else {
            WHITE
        }
    })
}

/// How far `(x, y)` lies from the segment `a`–`b` (pixels).
fn to_segment((x, y): (f64, f64), (a, b): ((f64, f64), (f64, f64))) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2)).sqrt()
}

/// Line art in navy on white, its lines `width` pixels wide, round at their ends: a letter T, an X,
/// a ring and three hatch lines.
fn line_art(width: f64) -> Raster {
    let segments = [
        ((30.0, 30.0), (130.0, 30.0)),
        ((80.0, 30.0), (80.0, 120.0)),
        ((160.0, 30.0), (250.0, 120.0)),
        ((250.0, 30.0), (160.0, 120.0)),
        ((30.0, 160.0), (130.0, 160.0)),
        ((30.0, 180.0), (130.0, 180.0)),
        ((30.0, 200.0), (130.0, 200.0)),
    ];
    drawn(300, 230, move |x, y| {
        let ring = (((x - 205.0).powi(2) + (y - 180.0).powi(2)).sqrt() - 35.0).abs() <= width / 2.0;
        if ring || segments.iter().any(|&s| to_segment((x, y), s) <= width / 2.0) { NAVY } else { WHITE }
    })
}

/// A JPEG of `r` (opaque), as the `image` crate writes one.
fn jpeg(r: &Raster) -> Vec<u8> {
    let rgb: Vec<u8> = r.rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(&rgb, r.width, r.height, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// Each scene: its name, its file and the params it is digitized with.
fn scenes() -> Vec<(&'static str, &'static str, Vec<u8>, Value)> {
    let halves = drawn(300, 300, |x, y| match ((x - 150.0).powi(2) + (y - 150.0).powi(2)).sqrt() < 120.0 {
        true if x < 150.0 => ORANGE,
        true => NAVY,
        false => CLEAR,
    });
    let letter = drawn(300, 300, |x, y| if (60.0..100.0).contains(&((x - 150.0).powi(2) + (y - 150.0).powi(2)).sqrt()) { NAVY } else { WHITE });
    let specks = drawn(300, 200, |x, y| {
        let (px, py) = (x as u32, y as u32);
        let h = (px / 25).wrapping_mul(73_856_093) ^ (py / 25).wrapping_mul(19_349_663);
        let side = 1 + h % 4;
        let (ox, oy) = (px % 25, py % 25);
        if !(10.0..290.0).contains(&x) || !(10.0..190.0).contains(&y) {
            CLEAR
        } else if ox >= 10 && ox < 10 + side && oy >= 10 && oy < 10 + side {
            NAVY
        } else {
            ORANGE
        }
    });
    // An edge 5° off upright.
    let blend = drawn(300, 200, |x, y| if x < 150.0 + (y - 100.0) * 0.0875 { NAVY } else { YELLOW });
    let squares = drawn(300, 200, |x, y| {
        if (20.0..160.0).contains(&x) && (30.0..170.0).contains(&y) {
            ORANGE
        } else if (160.0..270.0).contains(&x) && (50.0..150.0).contains(&y) {
            NAVY
        } else {
            WHITE
        }
    });
    // Light falling off by six levels from the left.
    let scan = drawn(300, 200, |x, y| {
        let shade = (x / 300.0 * 6.0) as u8;
        let logo = (60.0..240.0).contains(&x) && (50.0..150.0).contains(&y) && !((90.0..210.0).contains(&x) && (80.0..120.0).contains(&y));
        if logo { NAVY } else { [CREAM[0] - shade, CREAM[1] - shade, CREAM[2] - shade, 255] }
    });
    let badge = badge();
    let hex = |c: [u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    let jobs = json!([{"color": hex(GOLD), "job": "fill"}, {"color": hex(RED), "job": "none"}]);
    vec![
        ("badge", "badge.png", badge.encode_png(), json!({"colors": 4})),
        ("badge-jpeg", "badge.jpg", jpeg(&badge), json!({"colors": 4})),
        ("halves", "halves.png", halves.encode_png(), json!({"colors": 2})),
        ("letter", "letter.png", letter.encode_png(), json!({"colors": 2, "background": "corners"})),
        ("specks", "specks.png", specks.encode_png(), json!({"colors": 2})),
        ("blend", "blend.png", blend.encode_png(), json!({"colors": 3, "background": "none"})),
        ("squares-0", "squares.png", squares.encode_png(), json!({"colors": 3, "overlap": 0})),
        ("squares-0.2", "squares.png", squares.encode_png(), json!({"colors": 3, "overlap": 0.2})),
        ("squares-0.4", "squares.png", squares.encode_png(), json!({"colors": 3, "overlap": 0.4})),
        ("scan", "scan.png", scan.encode_png(), json!({"colors": 2})),
        ("badge-logo", "badge.png", badge.encode_png(), json!({"colors": 4, "mode": "logo"})),
        ("badge-greys", "badge.png", badge.encode_png(), json!({"colors": 3, "mode": "grayscale"})),
        ("badge-jobs", "badge.png", badge.encode_png(), json!({"colors": 4, "colorChoices": jobs})),
        ("badge-crop", "badge.png", badge.encode_png(), json!({"colors": 4, "crop": [100, 50, 300, 250]})),
        ("badge-resized", "badge.png", badge.encode_png(), json!({"colors": 4, "width": 25})),
        ("line-art", "lines.png", line_art(THIN_LINE_PX).encode_png(), json!({"colors": 2})),
        (
            "line-art-wide-as-lines",
            "lines.png",
            line_art(WIDE_LINE_PX).encode_png(),
            json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "lines"}]}),
        ),
    ]
}

/// The lines of scene `name`: picture `bytes` (a file named `file`) placed at [`PX_PER_MM`],
/// digitized with `params`: its dry run's reply, then each format's file.
fn lines(name: &str, file: &str, bytes: &[u8], params: &Value) -> Vec<String> {
    let image = image::load_from_memory(bytes).unwrap();
    let (w, h) = (f64::from(image.width()) / PX_PER_MM * PT_PER_MM, f64::from(image.height()) / PX_PER_MM * PT_PER_MM);
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": w, "height": h, "artboards": 1})).unwrap();
    let place = json!({"name": file, "dataBase64": vectorcraft_format::base64_encode(bytes), "rect": [0, 0, w, h]});
    s.execute("file.place", &place).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    let mut dry = params.clone();
    dry["dryRun"] = json!(true);
    let mut answered = s.execute("embroidery.digitizeImage", &dry).unwrap_or_else(|e| panic!("{name}: {e}"));
    answered.as_object_mut().unwrap().remove("timings");
    let mut out = vec![format!("{name} reply {:016x}", fnv(answered.to_string().as_bytes()))];
    s.execute("embroidery.digitizeImage", params).unwrap_or_else(|e| panic!("{name}: {e}"));
    for format in FORMATS {
        let r = s.execute("document.export", &json!({ "format": format })).unwrap_or_else(|e| panic!("{name}, {format}: {e}"));
        let bytes = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
        out.push(format!("{name} {format} {:016x} {}", fnv(&bytes), bytes.len()));
    }
    out
}

#[test]
fn images_digitized_export_as_recorded() {
    let got: Vec<String> = scenes().iter().flat_map(|(name, file, bytes, params)| lines(name, file, bytes, params)).collect();
    let again: Vec<String> = scenes().iter().flat_map(|(name, file, bytes, params)| lines(name, file, bytes, params)).collect();
    assert_eq!(got, again, "the same scenes digitized twice");
    if std::env::var_os("VECTORCRAFT_IMAGE_LISTING_RECORD").is_some() {
        std::fs::write(LISTING, got.join("\n") + "\n").unwrap();
        return;
    }
    let want: Vec<String> = std::fs::read_to_string(LISTING).expect("the recorded hashes").lines().map(str::to_string).collect();
    let differ: Vec<String> = got.iter().zip(&want).filter(|(g, w)| g != w).take(8).map(|(g, w)| format!("{g}  (recorded: {w})")).collect();
    assert!(differ.is_empty() && got.len() == want.len(), "{} lines against {}, first differences:\n{}", got.len(), want.len(), differ.join("\n"));
}
