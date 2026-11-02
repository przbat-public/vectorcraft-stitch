//! The digitizing rows of `vectorcraft-cli perf --embroidery`: how long `embroidery.digitizeImage`
//! takes, stage by stage as its reply times them (`timings`), and in all, on pictures made here.
//!
//! The pictures, none from the corpus:
//! - a badge of 98 × 88 mm in six colours on white (an oval, a ring, a star, a band, bars and
//!   dots), drawn as art and exported by the engine as a JPEG at 10 pixels a millimetre over its
//!   box and a 5 mm margin ([`BADGE_PX`]);
//! - 2,500 specks of 0.09 mm² strewn over an orange field 100 mm square;
//! - grids of squares in five colours, none beside another of its own: 300 of 4 mm and 1,000 of
//!   3 mm;
//! - 1,500 × 1,500 pixels of noise in 16 colours, which makes more areas than a design may have.

use std::time::Instant;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_render::Rendered;

use super::{PT_PER_MM, RUNS, err, middle};

/// The badge's art (millimetres), the margin round it, and the picture's size at
/// [`PX_PER_MM`] (pixels).
const BADGE_MM: (f64, f64) = (98.0, 88.0);
const MARGIN_MM: f64 = 5.0;
const BADGE_PX: (u32, u32) = (1080, 980);

/// The pictures' resolution: the working resolution digitizing aims for, so that each is worked
/// pixel for pixel (`ppi` as the export takes it: 254 a inch).
const PX_PER_MM: f64 = 10.0;
const MM_PER_INCH: f64 = 25.4;

/// The JPEG quality the badge is exported at: a photo editor's usual.
const JPEG_QUALITY: u8 = 80;

/// The badge's colours: its own six, as the command is asked for.
const BADGE_COLORS: u32 = 6;

/// The specks: a grid of 50 by 50, each 3 pixels square (0.09 mm²), 20 pixels apart, on a field
/// of 1,000 pixels a side.
const SPECKS: u32 = 50;
const SPECK_PX: u32 = 3;
const SPECK_STEP_PX: u32 = 20;
const FIELD_PX: u32 = 1000;

/// The grids' colours, none of which is beside another of its own (each square's is its column
/// plus twice its row, of five).
const GRID_COLOURS: [[u8; 3]; 5] = [[29, 42, 107], [232, 177, 0], [210, 32, 48], [31, 138, 60], [126, 200, 232]];

/// The noise: 1,500 pixels a side, the working copy's largest, in 16 colours.
const NOISE_PX: u32 = 1500;
const NOISE_COLORS: u32 = 16;

/// The rows' names, in order (at most 42 characters, the report's column).
pub(super) const ROWS: [&str; 10] = [
    "image: prepare a 1,080 × 980 JPEG",
    "image: trace and colours, 6 threads",
    "image: logo mode, 1,080 × 980 JPEG",
    "image: areas merged, 2,500 specks",
    "image: overlap 0.4 mm, 300 areas",
    "image: objects made, 300 areas",
    "image: stitches, 98 × 88 mm badge",
    "image: whole dry run, 98 × 88 mm badge",
    "image: 1,500 × 1,500 noise, refused",
    "image: dry run reply, 1,000 areas",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: estimates, about 2.5 times what each
/// measured on 2026-11-02 (release, 12-core Apple Silicon, load under 2, the median of three), as
/// the other rows', so that a regression of that size shows: the JPEG prepared in 1.9 ms, traced
/// with its colours in 105 ms (178 in logo mode), 2,500 specks merged in 29 ms, 300 areas grown
/// under each other in 16 ms and made into objects in 0.06 ms (1 ms, the least the report's
/// whole-millisecond column shows), the badge's stitches planned in 32 ms and its whole dry run in
/// 194 ms, noise refused in 1.1 s, a reply of 1,000 areas built in 4.9 ms.
pub(super) const BUDGETS: [f64; 10] = [5.0, 265.0, 450.0, 75.0, 40.0, 1.0, 80.0, 490.0, 2750.0, 13.0];

/// A picture of `width` × `height` opaque pixels, each `colour(x, y)`, as PNG.
fn png(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> Result<Vec<u8>, String> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = colour(x, y);
            pixels.extend_from_slice(&[r, g, b, u8::MAX]);
        }
    }
    Rendered { width, height, pixels }.to_png()
}

/// A rectangle `[x, y, width, height]` in points from millimetres.
fn rect_pt(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!([x * PT_PER_MM, y * PT_PER_MM, w * PT_PER_MM, h * PT_PER_MM])
}

/// A session holding picture `bytes` (a file named `name`) placed at [`PX_PER_MM`], selected.
fn placed(name: &str, bytes: &[u8], width_px: u32, height_px: u32) -> Result<Session, String> {
    let (w, h) = (f64::from(width_px) / PX_PER_MM, f64::from(height_px) / PX_PER_MM);
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": w * PT_PER_MM, "height": h * PT_PER_MM, "artboards": 1})).map_err(err)?;
    let data = vectorcraft_format::base64_encode(bytes);
    s.execute("file.place", &json!({"name": name, "dataBase64": data, "rect": rect_pt(0.0, 0.0, w, h)})).map_err(err)?;
    s.execute("select.all", &json!({})).map_err(err)?;
    Ok(s)
}

/// The badge, drawn as art and exported by the engine as a JPEG.
fn badge() -> Result<Vec<u8>, String> {
    let (w, h) = (BADGE_MM.0 + 2.0 * MARGIN_MM, BADGE_MM.1 + 2.0 * MARGIN_MM);
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": w * PT_PER_MM, "height": h * PT_PER_MM, "artboards": 1})).map_err(err)?;
    s.execute("paint.setStroke", &json!({"none": true})).map_err(err)?;
    let at = |v: f64| (v + MARGIN_MM) * PT_PER_MM;
    let size = |v: f64| v * PT_PER_MM;
    let mut shapes: Vec<(&str, &str, Value)> = vec![
        ("#1d2a6b", "shape.ellipse", json!({"x": at(0.0), "y": at(0.0), "width": size(98.0), "height": size(88.0)})),
        ("#e8b100", "shape.ellipse", json!({"x": at(9.0), "y": at(9.0), "width": size(80.0), "height": size(70.0)})),
        ("#1d2a6b", "shape.ellipse", json!({"x": at(13.0), "y": at(13.0), "width": size(72.0), "height": size(62.0)})),
        ("#d22030", "shape.star", json!({"cx": at(49.0), "cy": at(42.0), "radius1": size(22.0), "radius2": size(9.0), "points": 5})),
        ("#1f8a3c", "shape.rectangle", json!({"x": at(19.0), "y": at(62.0), "width": size(60.0), "height": size(7.0)})),
    ];
    for k in 0..5 {
        let x = 27.0 + 10.0 * f64::from(k);
        shapes.push(("#f07a1a", "shape.rectangle", json!({"x": at(x), "y": at(16.0), "width": size(4.0), "height": size(8.0)})));
    }
    for (x, y) in [(20.0, 40.0), (75.0, 40.0), (30.0, 52.0), (65.0, 52.0), (40.0, 74.0), (55.0, 74.0)] {
        shapes.push(("#7ec8e8", "shape.ellipse", json!({"x": at(x), "y": at(y), "width": size(5.0), "height": size(5.0)})));
    }
    for (colour, command, params) in shapes {
        s.execute("paint.setFill", &json!({"color": colour})).map_err(err)?;
        s.execute(command, &params).map_err(err)?;
    }
    let export = json!({"format": "jpg", "ppi": PX_PER_MM * MM_PER_INCH, "quality": JPEG_QUALITY, "rect": rect_pt(0.0, 0.0, w, h)});
    let r = s.execute("document.export", &export).map_err(err)?;
    let data = r["dataBase64"].as_str().ok_or("perf: the badge's export gave no data")?;
    vectorcraft_format::base64_decode(data).ok_or_else(|| "perf: the badge's export isn't base64".into())
}

/// Squares `side` pixels a side, `cols` by `rows`, in [`GRID_COLOURS`], as PNG.
fn grid(cols: u32, rows: u32, side: u32) -> Result<Vec<u8>, String> {
    let colours = GRID_COLOURS.len() as u32;
    png(cols * side, rows * side, |x, y| GRID_COLOURS.get(((x / side + 2 * (y / side)) % colours) as usize).copied().unwrap_or_default())
}

/// A dry run of `params` on `s`, `RUNS` times: each run's reply and its milliseconds in all.
fn dry_runs(s: &mut Session, params: Value) -> Result<Vec<(Value, f64)>, String> {
    let mut p = params;
    p["dryRun"] = json!(true);
    let mut out = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        let r = s.execute("embroidery.digitizeImage", &p).map_err(err)?;
        out.push((r, t.elapsed().as_secs_f64() * 1000.0));
    }
    Ok(out)
}

/// The median of the stages `stages` of `runs`' timings, added up.
fn staged(runs: &[(Value, f64)], stages: &[&str]) -> f64 {
    middle(runs.iter().map(|(r, _)| stages.iter().filter_map(|k| r["timings"][*k].as_f64()).sum()).collect())
}

/// The digitizing rows' milliseconds, in the order of [`ROWS`].
pub(super) fn times() -> Result<Vec<f64>, String> {
    let mut ms = Vec::with_capacity(ROWS.len());
    let jpeg = badge()?;
    let mut s = placed("badge.jpg", &jpeg, BADGE_PX.0, BADGE_PX.1)?;
    let runs = dry_runs(&mut s, json!({"colors": BADGE_COLORS}))?;
    ms.push(staged(&runs, &["image"]));
    ms.push(staged(&runs, &["trace", "areas"]));
    let logo = dry_runs(&mut s, json!({"colors": BADGE_COLORS, "mode": "logo"}))?;
    ms.push(staged(&logo, &["trace", "areas"]));
    let specks = png(FIELD_PX, FIELD_PX, |x, y| {
        let speck = |v: u32| v % SPECK_STEP_PX < SPECK_PX && v / SPECK_STEP_PX < SPECKS;
        if speck(x) && speck(y) { [29, 42, 107] } else { [240, 130, 20] }
    })?;
    let mut field = placed("specks.png", &specks, FIELD_PX, FIELD_PX)?;
    ms.push(staged(&dry_runs(&mut field, json!({"colors": 2, "background": "none"}))?, &["areas"]));
    let (cols, rows, side) = (20, 15, 40);
    let mut squares = placed("squares.png", &grid(cols, rows, side)?, cols * side, rows * side)?;
    let squares_runs = dry_runs(&mut squares, json!({"colors": GRID_COLOURS.len()}))?;
    ms.push(staged(&squares_runs, &["shapes"]));
    ms.push(staged(&squares_runs, &["objects"]));
    ms.push(staged(&runs, &["stitches"]));
    ms.push(middle(runs.iter().map(|(_, t)| *t).collect()));
    let noise = png(NOISE_PX, NOISE_PX, |x, y| {
        let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761) >> 28;
        let v = (h * (255 / (NOISE_COLORS - 1))) as u8;
        [v, 255 - v, v / 2]
    })?;
    let mut noisy = placed("noise.png", &noise, NOISE_PX, NOISE_PX)?;
    let refusal = json!({"dryRun": true, "colors": NOISE_COLORS, "minArea": 0, "smoothing": 0});
    let mut refused = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        if noisy.execute("embroidery.digitizeImage", &refusal).is_ok() {
            return Err("perf: noise was digitized, not refused as more areas than a design may have".into());
        }
        refused.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.push(middle(refused));
    let (cols, rows, side) = (40, 25, 30);
    let mut many = placed("many.png", &grid(cols, rows, side)?, cols * side, rows * side)?;
    let replies = dry_runs(&mut many, json!({"colors": GRID_COLOURS.len()}))?;
    let stages = ["image", "trace", "areas", "shapes", "objects", "stitches"];
    ms.push(middle(replies.iter().map(|(r, t)| t - stages.iter().filter_map(|k| r["timings"][*k].as_f64()).sum::<f64>()).collect()));
    Ok(ms)
}
