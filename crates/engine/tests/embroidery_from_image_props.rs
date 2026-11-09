//! Digitizing from an image on random flat pictures (rectangles and discs in two to six colours, on
//! white or on transparency, 20 to 40 mm, at 4 to 10 pixels a millimetre), from the fork's fixed
//! seed:
//! - no area is smaller than the smallest area, unless it is a line kept below it;
//! - every object Create makes is painted with a thread of the document's palette, and the threads
//!   number no more than the colours asked;
//! - the dry run's figures are Create's, and digitizing twice answers the same;
//! - a larger smallest area never adds areas, either side of the trace's own noise cap;
//! - what the overlap sews twice, measured from the outlines, is what the reply says, and a larger
//!   overlap never sews less twice.
//!
//! And on random line art: navy lines 0.3 to 0.9 mm wide over bands of colour, at 10 pixels a
//! millimetre, their colour left to the rules or set to Lines, at no overlap or 0.4 mm:
//! - each line is an area sewn along its middle, every point of which lies within a pixel of the
//!   line drawn;
//! - the bands beside each line cover it under its whole width;
//! - each line, drawn as one stroke, is sewn with no jump but the move to it.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace::Raster;

const PT_PER_MM: f64 = 72.0 / 25.4;

/// The colours the shapes are drawn in: far apart, so that each is a colour of its own.
const COLOURS: [[u8; 4]; 6] =
    [[29, 42, 107, 255], [232, 177, 0, 255], [210, 32, 48, 255], [31, 138, 60, 255], [240, 122, 26, 255], [126, 200, 232, 255]];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// The smallest areas tried in pairs, the smaller and the larger (square millimetres): either side
/// of the trace's own noise cap, a thread's footprint (0.16 mm²), below which the trace keeps what
/// the rules then join.
const MIN_AREAS: [f64; 7] = [0.05, 0.1, 0.16, 0.25, 0.5, 1.0, 4.0];

/// How far a vertex of an outline may move when the reply rounds it to hundredths of a millimetre
/// (millimetres): half a hundredth either way, along both axes.
const ROUNDED_MM: f64 = 0.005 * std::f64::consts::SQRT_2;

/// The most areas a reply lists.
const LISTED: u64 = 10_000;

/// A shape of a picture: a rectangle or a disc, its box as shares of the picture, and its colour.
#[derive(Clone, Debug)]
struct Shape {
    disc: bool,
    box_: [f64; 4],
    colour: usize,
}

fn arb_shape() -> impl Strategy<Value = Shape> {
    (any::<bool>(), 0.0..0.7f64, 0.0..0.7f64, 0.15..0.6f64, 0.15..0.6f64, 0..COLOURS.len()).prop_map(|(disc, x, y, w, h, colour)| Shape {
        disc,
        box_: [x, y, (x + w).min(1.0), (y + h).min(1.0)],
        colour,
    })
}

/// A picture: its size (millimetres), its resolution (pixels a millimetre), whether it is on white,
/// and its shapes, the last on top.
#[derive(Clone, Debug)]
struct Picture {
    size_mm: (f64, f64),
    px_per_mm: f64,
    on_white: bool,
    shapes: Vec<Shape>,
}

/// A pair of smallest areas from [`MIN_AREAS`], the smaller first.
fn arb_min_areas() -> impl Strategy<Value = (f64, f64)> {
    (0..MIN_AREAS.len(), 0..MIN_AREAS.len()).prop_map(|(i, j)| (MIN_AREAS[i.min(j)], MIN_AREAS[i.max(j)]))
}

fn arb_picture() -> impl Strategy<Value = Picture> {
    (20.0..40.0f64, 20.0..40.0f64, prop_oneof![Just(4.0), Just(6.0), Just(10.0)], any::<bool>(), prop::collection::vec(arb_shape(), 2..7))
        .prop_map(|(w, h, px_per_mm, on_white, shapes)| Picture { size_mm: (w, h), px_per_mm, on_white, shapes })
}

impl Picture {
    fn raster(&self) -> Raster {
        let (w, h) = ((self.size_mm.0 * self.px_per_mm).round() as u32, (self.size_mm.1 * self.px_per_mm).round() as u32);
        Raster::from_fn(w, h, |x, y| {
            let (u, v) = ((f64::from(x) + 0.5) / f64::from(w), (f64::from(y) + 0.5) / f64::from(h));
            let inside = |s: &Shape| {
                let [x0, y0, x1, y1] = s.box_;
                if s.disc {
                    let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
                    ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) <= 1.0
                } else {
                    (x0..x1).contains(&u) && (y0..y1).contains(&v)
                }
            };
            match self.shapes.iter().rev().find(|s| inside(s)) {
                Some(s) => COLOURS[s.colour],
                None if self.on_white => WHITE,
                None => CLEAR,
            }
        })
    }

    /// A session holding the picture placed at its size, selected.
    fn placed(&self) -> Session {
        let (w, h) = (self.size_mm.0 * PT_PER_MM, self.size_mm.1 * PT_PER_MM);
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": w, "height": h, "artboards": 1})).unwrap();
        let png = vectorcraft_format::base64_encode(&self.raster().encode_png());
        s.execute("file.place", &json!({"name": "picture.png", "dataBase64": png, "rect": [0, 0, w, h]})).unwrap();
        s.execute("select.all", &json!({})).unwrap();
        s
    }

    /// How many colours it is drawn in.
    fn colours(&self) -> usize {
        let mut seen: Vec<usize> = self.shapes.iter().map(|s| s.colour).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    }
}

/// A dry run of `params` on `s`, its timings left out.
fn dry(s: &mut Session, params: &Value) -> Value {
    let mut p = params.clone();
    p["dryRun"] = json!(true);
    let mut r = s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    r.as_object_mut().unwrap().remove("timings");
    r
}

/// The outlines of reply `r`'s areas, measured: their areas added (square millimetres), and their
/// lengths added (millimetres).
fn outlines_measured(r: &Value) -> (f64, f64) {
    use vectorcraft_geom::kurbo::{BezPath, Shape};
    let outlines = r["areas"].as_array().unwrap().iter().filter_map(|a| a["outline"].as_str());
    outlines.map(|d| BezPath::from_svg(d).unwrap()).fold((0.0, 0.0), |(area, length), p| (area + p.area().abs(), length + p.perimeter(1e-3)))
}

/// The fill and stroke colours of every object in `s`'s document (a centre line is a stroke),
/// `#rrggbb`.
fn fills(s: &Session) -> Vec<String> {
    use vectorcraft_doc::appearance::AppearanceItem;
    let doc = s.doc().unwrap().doc.clone();
    let mut out = Vec::new();
    doc.walk(|n| {
        for item in &n.appearance.items {
            let paint = match item {
                AppearanceItem::Fill(f) => &f.paint,
                AppearanceItem::Stroke(st) => &st.paint,
            };
            if let vectorcraft_color::Paint::Solid { color, .. } = paint {
                out.push(color.to_hex());
            }
        }
    });
    out
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(24))]

    #[test]
    fn random_pictures_digitize_by_the_rules(picture in arb_picture(), (small, large) in arb_min_areas()) {
        let mut s = picture.placed();
        let colors = picture.colours().max(2) + 1;
        let params = json!({"colors": colors});
        let answered = dry(&mut s, &params);
        prop_assert_eq!(&answered, &dry(&mut s, &params), "digitizing twice");
        let min_area = answered["settings"]["minArea"].as_f64().unwrap();
        for a in answered["areas"].as_array().unwrap() {
            prop_assert!(a["size"]["area"].as_f64().unwrap() >= min_area || a["kind"] == json!("line"), "{}", a);
        }
        prop_assert!(answered["colorCount"].as_u64().unwrap() as usize <= colors);
        // A larger smallest area never adds areas.
        let mut count = |min: f64| dry(&mut s, &json!({"colors": colors, "minArea": min}))["areaCount"].as_u64().unwrap();
        let (more, fewer) = (count(small), count(large));
        prop_assert!(fewer <= more, "{} mm² makes {} areas, {} mm² {}", large, fewer, small, more);
        // What the overlap sews twice, from the outlines: the areas' own shapes tile the picture,
        // so what their outlines cover past that is what the strips added.
        let mut measured = |overlap: f64| {
            let r = dry(&mut s, &json!({"colors": colors, "overlap": overlap, "outlines": true, "limit": LISTED}));
            let (area, length) = outlines_measured(&r);
            (area, length, r["doubledArea"].as_f64().unwrap())
        };
        // With none asked, the areas beside a centre line still reach its middle under it (what the
        // reply says is sewn twice then): the overlap adds what it adds past that.
        let (none, none_length, none_said) = measured(0.0);
        let (thin, thin_length, thin_said) = measured(0.2);
        let (asked, asked_length, asked_said) = measured(0.4);
        // The outlines' rounding moves each area by at most its length times a vertex's move.
        let slack = |length: f64| (length + none_length) * ROUNDED_MM;
        prop_assert!(
            ((thin - none) - (thin_said - none_said)).abs() <= slack(thin_length),
            "0.2 mm sews {} mm² twice, the reply says {}",
            thin - none,
            thin_said - none_said
        );
        prop_assert!(
            ((asked - none) - (asked_said - none_said)).abs() <= slack(asked_length),
            "0.4 mm sews {} mm² twice, the reply says {}",
            asked - none,
            asked_said - none_said
        );
        prop_assert!(thin <= asked + slack(asked_length), "0.2 mm sews {} mm² twice, 0.4 mm {}", thin - none, asked - none);
        if answered["areaCount"] == json!(0) {
            return Ok(());
        }
        let made = s.execute("embroidery.digitizeImage", &params).unwrap();
        prop_assert_eq!(&made["stats"], &answered["stats"]);
        let threads: Vec<String> = Palette::Jef.threads().iter().map(|&([r, g, b], _)| vectorcraft_color::Color::rgb8(r, g, b).to_hex()).collect();
        for fill in fills(&s) {
            prop_assert!(threads.contains(&fill), "{} is no thread's colour", fill);
        }
    }
}

/// A line of line art: from `a` to `b` (shares of the picture's width and height), `width_mm` wide.
#[derive(Clone, Debug)]
struct Line {
    a: (f64, f64),
    b: (f64, f64),
    width_mm: f64,
}

/// Line art: navy lines over bands of colour, `size_mm` at [`LINE_ART_PX_PER_MM`]; the bands split
/// where `splits` say (shares of the width, in order), each line in a slot of the height of its
/// own so that no two touch; navy set to Lines (`as_lines`) or left to the rules; the overlap.
#[derive(Clone, Debug)]
struct LineArt {
    size_mm: (f64, f64),
    splits: Vec<f64>,
    lines: Vec<Line>,
    as_lines: bool,
    overlap: f64,
}

/// The line art's resolution (pixels a millimetre): the working resolution, so that the thinnest
/// line is three pixels wide.
const LINE_ART_PX_PER_MM: f64 = 10.0;

/// Where the bands may be split (shares of the width), the first ones taken, and how far each is
/// moved, the same way: bands a fifth of the width apart, at least 6 mm, wider than any line.
const SPLITS: [f64; 3] = [0.3, 0.5, 0.7];
const SPLIT_JITTER: f64 = 0.05;

/// The lines' colour, and the bands'.
const LINE_COLOUR: [u8; 4] = COLOURS[0];
const BANDS: [[u8; 4]; 3] = [COLOURS[1], COLOURS[2], COLOURS[3]];

/// How far apart along a line its points are looked at under the bands (millimetres): a quarter
/// of a millimetre, a few working pixels.
const SAMPLE_MM: f64 = 0.25;

fn arb_line_art() -> impl Strategy<Value = LineArt> {
    let line = (0.1..0.45f64, 0.55..0.9f64, -0.2..0.2f64, -0.2..0.2f64, 0.3..0.9f64);
    (
        30.0..50.0f64,
        30.0..50.0f64,
        0..=SPLITS.len(),
        -SPLIT_JITTER..SPLIT_JITTER,
        prop::collection::vec(line, 1..5),
        any::<bool>(),
        prop_oneof![Just(0.0), Just(0.4)],
    )
        .prop_map(|(w, h, bands, jitter, lines, as_lines, overlap)| {
            let splits = SPLITS.iter().take(bands).map(|s| s + jitter).collect();
            let slot = 1.0 / lines.len() as f64;
            let lines = lines
                .into_iter()
                .enumerate()
                .map(|(k, (x0, x1, dy0, dy1, width_mm))| {
                    // Within the middle half of its slot, so that the slots' lines stay apart.
                    let middle = slot * (k as f64 + 0.5);
                    Line { a: (x0, middle + dy0 * slot), b: (x1, middle + dy1 * slot), width_mm }
                })
                .collect();
            LineArt { size_mm: (w, h), splits, lines, as_lines, overlap }
        })
}

/// How far point `q` (millimetres) lies from line `l`'s middle, drawn on a picture `size` mm.
fn from_line(q: (f64, f64), l: &Line, size: (f64, f64)) -> f64 {
    let (a, b) = ((l.a.0 * size.0, l.a.1 * size.1), (l.b.0 * size.0, l.b.1 * size.1));
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((q.0 - a.0) * dx + (q.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((q.0 - a.0 - t * dx).powi(2) + (q.1 - a.1 - t * dy).powi(2)).sqrt()
}

impl LineArt {
    fn raster(&self) -> Raster {
        let (w, h) = ((self.size_mm.0 * LINE_ART_PX_PER_MM).round() as u32, (self.size_mm.1 * LINE_ART_PX_PER_MM).round() as u32);
        Raster::from_fn(w, h, |x, y| {
            let q = ((f64::from(x) + 0.5) / LINE_ART_PX_PER_MM, (f64::from(y) + 0.5) / LINE_ART_PX_PER_MM);
            if self.lines.iter().any(|l| from_line(q, l, self.size_mm) <= l.width_mm / 2.0) {
                return LINE_COLOUR;
            }
            let band = self.splits.iter().filter(|&&s| q.0 / self.size_mm.0 >= s).count();
            BANDS[band.min(BANDS.len() - 1)]
        })
    }

    /// A session holding the picture placed at its size, selected.
    fn placed(&self) -> Session {
        let (w, h) = (self.size_mm.0 * PT_PER_MM, self.size_mm.1 * PT_PER_MM);
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": w, "height": h, "artboards": 1})).unwrap();
        let png = vectorcraft_format::base64_encode(&self.raster().encode_png());
        s.execute("file.place", &json!({"name": "lines.png", "dataBase64": png, "rect": [0, 0, w, h]})).unwrap();
        s.execute("select.all", &json!({})).unwrap();
        s
    }
}

/// The ids of the objects in `s`'s document sewn along a line: those stroked and not filled (a
/// fill of no paint is none).
fn stroked_only(s: &Session) -> Vec<u64> {
    use vectorcraft_color::Paint;
    use vectorcraft_doc::appearance::AppearanceItem;
    let doc = s.doc().unwrap().doc.clone();
    let mut out = Vec::new();
    doc.walk(|n| {
        let stroked = n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Stroke(st) if !matches!(st.paint, Paint::None)));
        let filled = n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Fill(f) if !matches!(f.paint, Paint::None)));
        if stroked && !filled && matches!(n.kind, vectorcraft_doc::NodeKind::Path { .. }) {
            out.push(n.id.0);
        }
    });
    out
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(24))]

    #[test]
    fn random_line_art_is_sewn_along_its_lines(art in arb_line_art()) {
        use vectorcraft_geom::kurbo::{BezPath, ParamCurve, Shape};
        let mut s = art.placed();
        let navy = format!("#{:02x}{:02x}{:02x}", LINE_COLOUR[0], LINE_COLOUR[1], LINE_COLOUR[2]);
        let choices = if art.as_lines { json!([{"color": navy, "job": "lines"}]) } else { json!([]) };
        let params = json!({"colors": art.splits.len() + 2, "background": "none", "overlap": art.overlap, "colorChoices": choices});
        let mut asked = params.clone();
        asked["outlines"] = json!(true);
        asked["limit"] = json!(LISTED);
        let r = dry(&mut s, &asked);
        let areas = r["areas"].as_array().unwrap();
        let (lines, fields): (Vec<&Value>, Vec<&Value>) = areas.iter().partition(|a| !a["line"].is_null());
        prop_assert_eq!(lines.len(), art.lines.len(), "the lines sewn along their middle: {}", r["areas"]);
        // Every point of each centre line within a pixel of a line drawn.
        let pixel = 1.0 / LINE_ART_PX_PER_MM;
        for a in &lines {
            let path = BezPath::from_svg(a["outline"].as_str().unwrap()).unwrap();
            for seg in path.segments() {
                for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let q = seg.eval(t);
                    let off = art.lines.iter().map(|l| from_line((q.x, q.y), l, art.size_mm) - l.width_mm / 2.0).fold(f64::INFINITY, f64::min);
                    prop_assert!(off <= pixel + ROUNDED_MM, "a centre line {} mm past its line at {:?}", off, q);
                }
            }
        }
        // The bands beside each line cover it under its whole width: points along its middle and
        // half a pixel inside its edges lie in a band's outline.
        let bands: Vec<BezPath> = fields.iter().filter_map(|a| a["outline"].as_str()).map(|d| BezPath::from_svg(d).unwrap()).collect();
        for l in &art.lines {
            let (a, b) = ((l.a.0 * art.size_mm.0, l.a.1 * art.size_mm.1), (l.b.0 * art.size_mm.0, l.b.1 * art.size_mm.1));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let length = (dx * dx + dy * dy).sqrt();
            let (nx, ny) = (-dy / length, dx / length);
            let steps = (length / SAMPLE_MM).ceil() as usize;
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                for side in [-0.5, 0.0, 0.5] {
                    let off = side * (l.width_mm - pixel);
                    let q = vectorcraft_geom::Point::new(a.0 + t * dx + off * nx, a.1 + t * dy + off * ny);
                    prop_assert!(bands.iter().any(|band| band.contains(q)), "fabric under a line at {:?}", q);
                }
            }
        }
        // Each line, drawn as one stroke, sewn with no jump but the move to it (sewn after the
        // bands, it is never the design's first).
        s.execute("embroidery.digitizeImage", &params).unwrap();
        let made = stroked_only(&s);
        prop_assert_eq!(made.len(), art.lines.len(), "the lines made");
        for id in made {
            let info = s.execute("embroidery.info", &json!({"ids": [id], "plan": true})).unwrap();
            let jumps = info["stroke"]["sewing"]["jumps"].as_u64().unwrap();
            prop_assert!(jumps <= 1, "line {} jumps inside it: {}", id, info["stroke"]["sewing"]);
        }
    }
}
