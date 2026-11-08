//! Digitizing from an image: centre lines. A thin line drawn in code (anti-aliased, as a picture's
//! edges are) is made a stroke along its middle and sewn in one go through its junctions; the
//! colours beside it reach its middle under it; a colour set to Fill keeps its lines filled, and one
//! not sewn keeps its line unsewn; a line's width doesn't depend on the picture's resolution; a line
//! its crossing widens to a millimetre is still sewn as a running line. A colour set to Lines is
//! sewn along its middle whatever its width, the colours round it reaching its middle; a job that
//! isn't one is refused.

use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point};
use vectorcraft_stitch::{Command, MM_PER_PT};
use vectorcraft_trace as tr;

use super::super::Options;
use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const YELLOW: [u8; 4] = [250, 210, 40, 255];
const RED: [u8; 4] = [200, 30, 40, 255];

/// The resolution the scenes are drawn at, unless a test says otherwise (pixels a millimetre).
const PX_PER_MM: f64 = 10.0;

/// The samples each pixel is the mean of, along each axis: edges anti-aliased as a picture's are.
const SAMPLES: u32 = 4;

/// The width of the lines drawn (millimetres): under a millimetre, a centre line.
const LINE_MM: f64 = 0.6;

/// How far a needle point may lie from the line drawn (millimetres): a working pixel and a half.
const ON_LINE_MM: f64 = 0.15;

/// A picture `w` × `h` millimetres at `px_per_mm`, each pixel the mean of [`SAMPLES`]² samples of
/// `paint` at points in millimetres.
fn picture(w: f64, h: f64, px_per_mm: f64, paint: impl Fn(Point) -> [u8; 4]) -> tr::Raster {
    let (pw, ph) = ((w * px_per_mm).round() as u32, (h * px_per_mm).round() as u32);
    tr::Raster::from_fn(pw, ph, |x, y| {
        let mut sum = [0u32; 4];
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let q = Point::new(
                    (f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SAMPLES)) / px_per_mm,
                    (f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SAMPLES)) / px_per_mm,
                );
                for (s, c) in sum.iter_mut().zip(paint(q)) {
                    *s += u32::from(c);
                }
            }
        }
        sum.map(|s| (s / (SAMPLES * SAMPLES)) as u8)
    })
}

/// How far `q` lies from segment `a`–`b` (millimetres).
fn to_segment(q: Point, (a, b): (Point, Point)) -> f64 {
    let ab = b - a;
    let t = if ab.hypot2() > 0.0 { ((q - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
    q.distance(a + ab * t)
}

/// A paint of `segments` drawn `width` millimetres wide (round at their ends) in `ink` over
/// `ground`.
fn lines_of(width: f64, segments: Vec<(Point, Point)>, ink: [u8; 4], ground: [u8; 4]) -> impl Fn(Point) -> [u8; 4] {
    move |q| if segments.iter().any(|&s| to_segment(q, s) <= width / 2.0) { ink } else { ground }
}

/// A paint of `segments` drawn [`LINE_MM`] wide (round at their ends) in `ink` over `ground`.
fn lines(segments: Vec<(Point, Point)>, ink: [u8; 4], ground: [u8; 4]) -> impl Fn(Point) -> [u8; 4] {
    lines_of(LINE_MM, segments, ink, ground)
}

/// A session holding `raster` placed at `px_per_mm`, selected.
fn placed(raster: &tr::Raster, px_per_mm: f64) -> Session {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(1.0 / px_per_mm / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    s
}

fn digitize(s: &mut Session, params: Value, dry_run: bool) -> Value {
    let mut p = params;
    p["dryRun"] = json!(dry_run);
    s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn hex(c: [u8; 4]) -> Value {
    json!(super::super::hex([c[0], c[1], c[2]]))
}

/// The areas of the reply whose colour is the image's nearest `colour` (the trace's colour is the
/// mean of its pixels, anti-aliasing among them: near the colour drawn, not always it).
fn areas_of(r: &Value, colour: [u8; 4]) -> Vec<Value> {
    let rgb = |v: &Value| v.as_str().and_then(super::settings::hex_colour);
    let drawn = [colour[0], colour[1], colour[2]];
    let colours: Vec<[u8; 3]> = r["colors"].as_array().unwrap().iter().filter_map(|c| rgb(&c["color"])).collect();
    let nearest = colours
        .iter()
        .copied()
        .min_by(|a, b| vectorcraft_stitch::color::delta_e_rgb(*a, drawn).total_cmp(&vectorcraft_stitch::color::delta_e_rgb(*b, drawn)));
    r["areas"].as_array().unwrap().iter().filter(|a| nearest.is_some() && rgb(&a["color"]) == nearest).cloned().collect()
}

/// The needle points of `s`'s design (millimetres).
fn needle_points(s: &Session) -> Vec<Point> {
    let plan = super::super::digitize::digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    plan.to_pattern().blocks.iter().flat_map(|b| &b.stitches).filter(|q| q.command == Command::Stitch).map(|q| q.point()).collect()
}

/// The objects Create made, in its group's groups.
fn made(s: &Session, created: &Value) -> Vec<Node> {
    let d = &s.doc().unwrap().doc;
    let group = d.node(NodeId(created["group"].as_u64().unwrap())).unwrap();
    group.children().unwrap().iter().flat_map(|g| g.children().unwrap().iter().map(|n| (**n).clone())).collect()
}

/// A letter T of thin navy lines on white: its bar from (5, 5) to (25, 5) mm, its stem down from
/// (15, 5) to (15, 20).
fn t_segments() -> Vec<(Point, Point)> {
    vec![(Point::new(5.0, 5.0), Point::new(25.0, 5.0)), (Point::new(15.0, 5.0), Point::new(15.0, 20.0))]
}

/// The T's middle through its ink: its segments carried on at its three free ends by half the
/// line's width, into the round ends the ink has there.
fn t_middle() -> Vec<(Point, Point)> {
    let half = LINE_MM / 2.0;
    vec![(Point::new(5.0 - half, 5.0), Point::new(25.0 + half, 5.0)), (Point::new(15.0, 5.0), Point::new(15.0, 20.0 + half))]
}

/// A thin T is one line with one junction and three ends; Create makes it a stroke along its
/// middle, as wide as the line drawn and round at its ends, which the planner sews in one go: no
/// jump, every needle point on the line drawn, reaching its ends.
#[test]
fn a_thin_t_is_one_line_sewn_in_one_go() {
    let mut s = placed(&picture(30.0, 25.0, PX_PER_MM, lines(t_segments(), NAVY, WHITE)), PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2}), true);
    let navy = areas_of(&r, NAVY);
    assert_eq!(navy.len(), 1, "{}", r["areas"]);
    let line = &navy[0]["line"];
    assert_eq!((&navy[0]["kind"], &line["junctions"], &line["ends"]), (&json!("line"), &json!(1), &json!(3)), "{}", navy[0]);
    let width = line["width"].as_f64().unwrap();
    assert!((width - LINE_MM).abs() <= WIDTH_MM, "{width} mm wide");
    let created = digitize(&mut s, json!({"colors": 2}), false)["created"].clone();
    let objects = made(&s, &created);
    assert_eq!(objects.len(), 1);
    let look = objects[0].appearance.items.clone();
    let stroke = look.iter().find_map(|i| match i {
        vectorcraft_doc::AppearanceItem::Stroke(st) => Some(st.clone()),
        vectorcraft_doc::AppearanceItem::Fill(_) => None,
    });
    let stroke = stroke.expect("a stroke");
    assert!(look.iter().all(|i| !matches!(i, vectorcraft_doc::AppearanceItem::Fill(f) if f.paint != vectorcraft_color::Paint::None)), "a fill");
    assert!((stroke.width * MM_PER_PT - width).abs() < 0.01 && stroke.cap == vectorcraft_doc::LineCap::Round, "{stroke:?}");
    let stats = s.execute("embroidery.stats", &json!({})).unwrap();
    assert_eq!((&stats["jumps"], &stats["trims"]), (&json!(0), &json!(0)), "{stats}");
    let pts = needle_points(&s);
    assert!(pts.iter().all(|&q| t_middle().iter().any(|&seg| to_segment(q, seg) <= ON_LINE_MM)), "a needle point off the line");
    let (lo, hi) = (pts.iter().map(|q| q.x).fold(f64::INFINITY, f64::min), pts.iter().map(|q| q.x).fold(f64::NEG_INFINITY, f64::max));
    let bottom = pts.iter().map(|q| q.y).fold(f64::NEG_INFINITY, f64::max);
    assert!(lo <= 5.0 + 0.3 && hi >= 25.0 - 0.3 && bottom >= 20.0 - 0.3, "the line's ends: {lo}, {hi}, {bottom}");
}

/// An X is one junction and four ends; a ring is a loop with neither; both are sewn with no jump.
#[test]
fn an_x_and_a_ring_are_sewn_in_one_go() {
    let x = vec![(Point::new(5.0, 5.0), Point::new(20.0, 20.0)), (Point::new(20.0, 5.0), Point::new(5.0, 20.0))];
    let mut s = placed(&picture(25.0, 25.0, PX_PER_MM, lines(x, NAVY, WHITE)), PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2}), true);
    let navy = areas_of(&r, NAVY);
    assert_eq!((navy.len(), &navy[0]["line"]["junctions"], &navy[0]["line"]["ends"]), (1, &json!(1), &json!(4)), "{}", r["areas"]);
    digitize(&mut s, json!({"colors": 2}), false);
    assert_eq!(s.execute("embroidery.stats", &json!({})).unwrap()["jumps"], json!(0));
    let ring = move |q: Point| if (q.distance(Point::new(12.0, 12.0)) - 6.0).abs() <= LINE_MM / 2.0 { NAVY } else { WHITE };
    let mut s = placed(&picture(25.0, 25.0, PX_PER_MM, ring), PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2}), true);
    let line = &areas_of(&r, NAVY)[0]["line"];
    assert_eq!((&line["pieces"], &line["junctions"], &line["ends"]), (&json!(1), &json!(0), &json!(0)), "{}", r["areas"]);
}

/// The colours beside a line reach its middle under it, whatever the overlap: a black line between
/// a yellow field and a red one leaves no fabric under it, the two fields together as large as the
/// picture, at an overlap of none and of 0.4 mm.
#[test]
fn the_colours_beside_a_line_reach_its_middle() {
    let paint = |q: Point| {
        if (q.x - 10.0).abs() <= LINE_MM / 2.0 {
            [10, 10, 10, 255]
        } else if q.x < 10.0 {
            YELLOW
        } else {
            RED
        }
    };
    let raster = picture(20.0, 10.0, PX_PER_MM, paint);
    let picture_mm2 = 20.0 * 10.0;
    for overlap in [0.0, 0.4] {
        let mut s = placed(&raster, PX_PER_MM);
        let r = digitize(&mut s, json!({"colors": 3, "background": "none", "overlap": overlap, "outlines": true}), true);
        let outlined = |c: [u8; 4]| -> f64 {
            areas_of(&r, c)
                .iter()
                .filter_map(|a| a["outline"].as_str())
                .map(|d| vectorcraft_geom::kurbo::Shape::area(&vectorcraft_geom::BezPath::from_svg(d).unwrap()).abs())
                .sum()
        };
        let (yellow, red) = (outlined(YELLOW), outlined(RED));
        assert!(yellow + red >= picture_mm2 - 0.2, "overlap {overlap}: the fields cover {} of {picture_mm2} mm²", yellow + red);
    }
}

/// A thin line lying inside one area alone is sewn over it: the orange field round a navy T is
/// solid under it, as large as the picture.
#[test]
fn a_line_inside_one_area_is_sewn_over_it() {
    let mut s = placed(&picture(30.0, 25.0, PX_PER_MM, lines(t_segments(), NAVY, ORANGE)), PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2, "background": "none", "outlines": true}), true);
    let orange: f64 = areas_of(&r, ORANGE)
        .iter()
        .filter_map(|a| a["outline"].as_str())
        .map(|d| vectorcraft_geom::kurbo::Shape::area(&vectorcraft_geom::BezPath::from_svg(d).unwrap()).abs())
        .sum();
    assert!(orange >= 30.0 * 25.0 - 0.2, "the field is {orange} mm², with a hole under the T");
    assert_eq!(areas_of(&r, NAVY)[0]["sewnOver"], json!(true));
}

/// A colour set to Fill keeps its thin lines filled (no centre line); one set not to be sewn keeps
/// its centre line, as a stroke set not to be sewn.
#[test]
fn a_colours_job_decides_whether_its_lines_are_centre_lines() {
    let raster = picture(30.0, 25.0, PX_PER_MM, lines(t_segments(), NAVY, WHITE));
    let mut s = placed(&raster, PX_PER_MM);
    let fill = json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "fill"}]});
    let r = digitize(&mut s, fill.clone(), true);
    // The navy found is the image's own, near the colour drawn: the choice takes colours within
    // the mismatch line of it.
    assert!(areas_of(&r, NAVY).is_empty() || areas_of(&r, NAVY).iter().all(|a| a["line"].is_null()), "{}", r["areas"]);
    let line_of = |r: &Value| r["areas"].as_array().unwrap().iter().find(|a| a["kind"] == json!("line")).cloned();
    assert!(line_of(&r).is_some_and(|a| a["line"].is_null()), "a Fill colour's line is no centre line: {}", r["areas"]);
    let created = digitize(&mut s, fill, false)["created"].clone();
    let filled = made(&s, &created);
    assert!(filled[0].appearance.items.iter().any(|i| matches!(i, vectorcraft_doc::AppearanceItem::Fill(_))));
    let mut s = placed(&raster, PX_PER_MM);
    let none = json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "none"}]});
    let r = digitize(&mut s, none.clone(), true);
    assert!(line_of(&r).is_some_and(|a| !a["line"].is_null()), "{}", r["areas"]);
    let created = digitize(&mut s, none, false)["created"].clone();
    let unsewn = made(&s, &created);
    let set = unsewn[0].extras().embroidery.as_deref().and_then(|e| e.stroke.as_deref()).and_then(|p| p.stitch.clone());
    assert_eq!(set.as_deref(), Some("none"));
    assert_eq!(s.execute("embroidery.stats", &json!({})).unwrap()["stitches"], json!(0));
}

/// How far a line's width may lie from the width drawn (millimetres): the trace gives the partly
/// covered pixels at each of its edges to the colour beside it, so a line measures up to a working
/// pixel narrower than drawn, an eighth of a millimetre at 8 pixels a millimetre.
const WIDTH_MM: f64 = 0.15;

/// A line's width is the same at any resolution the picture is drawn at, within a working pixel at
/// the coarsest: the T drawn at 8, 10 and 12 pixels a millimetre.
#[test]
fn a_lines_width_is_the_same_at_any_resolution() {
    let mut widths = Vec::new();
    for px_per_mm in [8.0, 10.0, 12.0] {
        let mut s = placed(&picture(30.0, 25.0, px_per_mm, lines(t_segments(), NAVY, WHITE)), px_per_mm);
        let r = digitize(&mut s, json!({"colors": 2}), true);
        let line = &areas_of(&r, NAVY)[0]["line"];
        widths.push(line["width"].as_f64().unwrap());
        assert_eq!(line["junctions"], json!(1), "{px_per_mm} px/mm: {line}");
    }
    let (lo, hi) = (widths.iter().copied().fold(f64::INFINITY, f64::min), widths.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!(hi - lo <= 1.0 / 8.0 && (lo - LINE_MM).abs() <= WIDTH_MM && (hi - LINE_MM).abs() <= WIDTH_MM, "{widths:?}");
}

/// How wide the small cross's lines are drawn and how far each of its arms reaches from its middle
/// (millimetres): lines under a millimetre, their arms short enough that the crossing widens the
/// line (its pixels over its length) to a millimetre or more.
const CROSS_LINE_MM: f64 = 0.9;
const CROSS_ARM_MM: f64 = 1.0;

/// A thin line that its crossing widens to a satin column's width is still sewn as a running line:
/// a small cross of lines under a millimetre, whose stroke is a millimetre wide or more, is set to a
/// running line and sewn as one.
#[test]
fn a_line_its_junction_widens_is_still_a_running_line() {
    let (c, along, down) = (Point::new(4.0, 4.0), vectorcraft_geom::Vec2::new(CROSS_ARM_MM, 0.0), vectorcraft_geom::Vec2::new(0.0, CROSS_ARM_MM));
    let arms = vec![(c - along, c + along), (c - down, c + down)];
    let mut s = placed(&picture(8.0, 8.0, PX_PER_MM, lines_of(CROSS_LINE_MM, arms, NAVY, WHITE)), PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2}), true);
    let navy = areas_of(&r, NAVY);
    let width = navy[0]["line"]["width"].as_f64().unwrap_or(0.0);
    assert!(width >= vectorcraft_stitch::plan::SATIN_MIN_WIDTH, "the cross is {width} mm wide: {}", r["areas"]);
    let created = digitize(&mut s, json!({"colors": 2}), false)["created"].clone();
    let objects = made(&s, &created);
    let set = objects[0].extras().embroidery.as_deref().and_then(|e| e.stroke.as_deref()).and_then(|p| p.stitch.clone());
    assert_eq!(set.as_deref(), Some("running"));
    let plan = super::super::digitize::digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let sewn: Vec<_> = plan.units.iter().filter_map(|u| u.sewn()).collect();
    assert_eq!(sewn, [vectorcraft_stitch::plan::Style::Running]);
}

/// How wide the letter set to Lines is drawn (millimetres): twice a satin column's least width,
/// which the rules alone would fill.
const WIDE_MM: f64 = 2.0;

/// A colour set to Lines is sewn along its middle whatever its width: a navy T drawn 2 mm wide,
/// filled by the rules, is one line with its junction and three ends when its colour's job is
/// Lines, made a stroke set to a running line (as wide as a satin column) and sewn in one go.
#[test]
fn a_colour_set_to_lines_is_sewn_along_its_middle() {
    let raster = picture(30.0, 25.0, PX_PER_MM, lines_of(WIDE_MM, t_segments(), NAVY, WHITE));
    let mut s = placed(&raster, PX_PER_MM);
    let r = digitize(&mut s, json!({"colors": 2}), true);
    let navy = areas_of(&r, NAVY);
    assert!(navy.iter().all(|a| a["kind"] == json!("fill") && a["line"].is_null()), "{}", r["areas"]);
    let as_lines = json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "lines"}]});
    let r = digitize(&mut s, as_lines.clone(), true);
    let navy = areas_of(&r, NAVY);
    assert_eq!(navy.len(), 1, "{}", r["areas"]);
    let line = &navy[0]["line"];
    assert_eq!((&navy[0]["kind"], &line["junctions"], &line["ends"]), (&json!("line"), &json!(1), &json!(3)), "{}", navy[0]);
    assert!((line["width"].as_f64().unwrap() - WIDE_MM).abs() <= 2.0 * WIDTH_MM, "{line}");
    let colour = r["colors"].as_array().unwrap().iter().find(|c| c["job"] == json!("lines")).cloned();
    assert!(colour.is_some_and(|c| c["sewn"] == json!(true)), "{}", r["colors"]);
    let created = digitize(&mut s, as_lines, false)["created"].clone();
    let objects = made(&s, &created);
    assert_eq!(objects.len(), 1);
    let set = objects[0].extras().embroidery.as_deref().and_then(|e| e.stroke.as_deref()).and_then(|p| p.stitch.clone());
    assert_eq!(set.as_deref(), Some("running"));
    let plan = super::super::digitize::digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let sewn: Vec<_> = plan.units.iter().filter_map(|u| u.sewn()).collect();
    assert_eq!(sewn, [vectorcraft_stitch::plan::Style::Running]);
    let stats = s.execute("embroidery.stats", &json!({})).unwrap();
    assert_eq!((&stats["jumps"], &stats["trims"]), (&json!(0), &json!(0)), "{stats}");
}

/// The colours round a colour set to Lines reach its middle under it from each side, whatever the
/// overlap (the project owner's choice, 2026-11-07, „Sąsiedzi wchodzą pod linię (polecam)”): no
/// fabric shows round the line. A band 2 mm wide between a yellow field and a red one, and a ring
/// 2 mm wide on an orange field (too wide, with what it holds, to be sewn over it): the fields
/// together as large as the picture, the line sewn after them.
#[test]
fn the_colours_round_a_colour_set_to_lines_reach_its_middle() {
    let band = |q: Point| {
        if (q.x - 10.0).abs() <= WIDE_MM / 2.0 {
            NAVY
        } else if q.x < 10.0 {
            YELLOW
        } else {
            RED
        }
    };
    let ring = |q: Point| if (q.distance(Point::new(15.0, 12.5)) - 6.0).abs() <= WIDE_MM / 2.0 { NAVY } else { ORANGE };
    let scenes: [(tr::Raster, f64, usize, Vec<[u8; 4]>); 2] = [
        (picture(20.0, 10.0, PX_PER_MM, band), 20.0 * 10.0, 3, vec![YELLOW, RED]),
        (picture(30.0, 25.0, PX_PER_MM, ring), 30.0 * 25.0, 2, vec![ORANGE]),
    ];
    for (raster, picture_mm2, colors, fields) in scenes {
        for overlap in [0.0, 0.4] {
            let mut s = placed(&raster, PX_PER_MM);
            let p = json!({"colors": colors, "background": "none", "overlap": overlap, "outlines": true,
                "colorChoices": [{"color": hex(NAVY), "job": "lines"}]});
            let r = digitize(&mut s, p, true);
            let outlined: f64 = fields
                .iter()
                .flat_map(|&c| areas_of(&r, c))
                .filter_map(|a| a["outline"].as_str().map(str::to_string))
                .map(|d| vectorcraft_geom::kurbo::Shape::area(&vectorcraft_geom::BezPath::from_svg(&d).unwrap()).abs())
                .sum();
            assert!(outlined >= picture_mm2 - 0.2, "overlap {overlap}: the fields cover {outlined} of {picture_mm2} mm²: {}", r["areas"]);
            let navy = areas_of(&r, NAVY);
            assert!(navy.len() == 1 && !navy[0]["line"].is_null(), "{}", r["areas"]);
            let order = |a: &Value| a["area"].as_u64().unwrap();
            let after = fields.iter().flat_map(|&c| areas_of(&r, c)).all(|f| order(&f) < order(&navy[0]));
            assert!(after, "the line after the fields: {}", r["areas"]);
        }
    }
}

/// The area (mm²) the outlines of the areas of `colour` cover.
fn outlined(r: &Value, colour: [u8; 4]) -> f64 {
    areas_of(r, colour)
        .iter()
        .filter_map(|a| a["outline"].as_str())
        .map(|d| vectorcraft_geom::kurbo::Shape::area(&vectorcraft_geom::BezPath::from_svg(d).unwrap()).abs())
        .sum()
}

/// The colours either side of a line reach its middle wherever it is narrower than at its widest
/// (the project owner's choices, 2026-11-07, „Sąsiedzi wchodzą pod linię (polecam)” and „Do
/// środka, najwyżej 2 mm (polecam)”), at no overlap: each field covers its own side, half the line
/// and half a pixel past its middle. A navy wedge set to Lines widening from 1 to 4 mm between a
/// yellow field and a red one (each field 375 mm² and half the wedge's 50); a band 1 mm wide for
/// half its length and 2 mm for the rest (each field 100 mm², half a square millimetre past the
/// middle).
#[test]
fn the_colours_beside_a_line_reach_its_middle_wherever_it_lies() {
    let half = |y: f64| (1.0 + 3.0 * y / 20.0) / 2.0;
    let wedge = move |q: Point| {
        if (q.x - 20.0).abs() <= half(q.y) {
            NAVY
        } else if q.x < 20.0 {
            YELLOW
        } else {
            RED
        }
    };
    let band = |q: Point| {
        let half = if q.y < 5.0 { WIDE_MM / 4.0 } else { WIDE_MM / 2.0 };
        if (q.x - 10.0).abs() <= half {
            NAVY
        } else if q.x < 10.0 {
            YELLOW
        } else {
            RED
        }
    };
    for (raster, each, near) in [(picture(40.0, 20.0, PX_PER_MM, wedge), 400.0, 2.0), (picture(20.0, 10.0, PX_PER_MM, band), 100.0, 1.0)] {
        let mut s = placed(&raster, PX_PER_MM);
        let p = json!({"colors": 3, "background": "none", "overlap": 0.0, "outlines": true,
            "colorChoices": [{"color": hex(NAVY), "job": "lines"}]});
        let r = digitize(&mut s, p, true);
        let (yellow, red) = (outlined(&r, YELLOW), outlined(&r, RED));
        assert!((yellow - each).abs() <= near && (red - each).abs() <= near, "yellow {yellow} mm², red {red} mm²: {}", r["areas"]);
    }
}

/// A colour reaches no more than 2 mm under a colour set to Lines however wide that is (the
/// project owner's choice, 2026-11-07, „Do środka, najwyżej 2 mm (polecam)”): a red dot 3 mm across
/// in a white field 60 × 30 mm set to Lines grows under the field by 2 mm and half a pixel, its top
/// at 11.45 mm, not to the field's middle or its edge.
#[test]
fn a_colour_grows_no_more_than_two_millimetres_under_a_field_set_to_lines() {
    let paint = |q: Point| if q.distance(Point::new(15.0, 15.0)) <= 1.5 { RED } else { WHITE };
    let mut s = placed(&picture(60.0, 30.0, PX_PER_MM, paint), PX_PER_MM);
    let p = json!({"colors": 2, "background": "none", "outlines": true,
        "colorChoices": [{"color": hex(WHITE), "job": "lines"}]});
    let r = digitize(&mut s, p, true);
    let red = areas_of(&r, RED);
    let outline = vectorcraft_geom::BezPath::from_svg(red[0]["outline"].as_str().unwrap()).unwrap();
    let top = vectorcraft_geom::kurbo::Shape::bounding_box(&outline).y0;
    let grown = 13.5 - super::settings::OVERLAP_RANGE_MM.1 - 0.05;
    assert!((top - grown).abs() <= 0.15, "the red dot's top at {top} mm, not {grown}: {}", red[0]);
}

/// A job that isn't one is refused, naming the four.
#[test]
fn a_job_that_isnt_one_is_refused() {
    let mut s = placed(&picture(30.0, 25.0, PX_PER_MM, lines(t_segments(), NAVY, WHITE)), PX_PER_MM);
    for job in [json!("line"), json!("Lines"), json!(1), json!(["lines"])] {
        let p = json!({"dryRun": true, "colors": 2, "colorChoices": [{"color": hex(NAVY), "job": job}]});
        let e = s.execute("embroidery.digitizeImage", &p).unwrap_err().to_string();
        assert!(e.contains(r#"`job` is "auto", "fill", "lines" or "none""#), "{job}: {e}");
    }
}

/// A dot of a colour set to Lines has no centre line to sew along, and is sewn as the shape it is
/// (the project owner's choice, 2026-11-07, „Szyć jako kształt (polecam)”), the reply counting it:
/// a navy dot 6 × 5.8 mm on white, whose middle thins to a piece about 0.4 mm long, is a fill, sewn
/// across most of its width, and the warning `notLines` names its colour and one area.
#[test]
fn a_dot_set_to_lines_is_sewn_as_the_shape_it_is() {
    let dot = |q: Point| if ((q.x - 10.0) / 3.0).powi(2) + ((q.y - 10.0) / 2.9).powi(2) <= 1.0 { NAVY } else { WHITE };
    let mut s = placed(&picture(20.0, 20.0, PX_PER_MM, dot), PX_PER_MM);
    let p = json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "lines"}]});
    let r = digitize(&mut s, p.clone(), true);
    let navy = areas_of(&r, NAVY);
    assert!(navy.len() == 1 && navy[0]["kind"] == json!("fill") && navy[0]["line"].is_null(), "{}", r["areas"]);
    let warned = r["warnings"].as_array().unwrap().iter().find(|w| w["id"] == json!("notLines")).cloned();
    assert!(warned.is_some_and(|w| w["values"]["count"] == json!(1) && w["values"]["color"] == navy[0]["color"]), "{}", r["warnings"]);
    digitize(&mut s, p, false);
    let pts = needle_points(&s);
    let lo = pts.iter().map(|q| q.x).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|q| q.x).fold(f64::NEG_INFINITY, f64::max);
    assert!(pts.len() >= 2 && hi - lo >= 3.0, "a 6 mm dot sewn as {} needle points over {} mm", pts.len(), hi - lo);
}

/// The dots of an "i" and a "j" in lettering set to Lines are sewn wherever they lie among the
/// pixels: drawn a tenth of a pixel to half a pixel apart from one picture to the next, each dot,
/// 1.4 mm across, is a fill with needle points inside it, and each letter's stroke a line.
#[test]
fn the_dots_of_lettering_set_to_lines_are_sewn_wherever_they_lie() {
    for shift in [0.0, 0.01, 0.03, 0.05, 0.07] {
        let (i, j) = (Point::new(8.0 + shift, 6.0 + shift), Point::new(16.0 + shift, 6.0));
        let strokes = [
            (Point::new(i.x, 9.0), Point::new(i.x, 18.0)),
            (Point::new(j.x, 9.0), Point::new(j.x, 20.0)),
            (Point::new(j.x, 20.0), Point::new(j.x - 3.0, 22.0)),
        ];
        let paint = move |q: Point| {
            if q.distance(i) <= 0.7 || q.distance(j) <= 0.7 || strokes.iter().any(|&s| to_segment(q, s) <= LINE_MM / 2.0) { NAVY } else { WHITE }
        };
        let mut s = placed(&picture(24.0, 26.0, PX_PER_MM, paint), PX_PER_MM);
        let p = json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "lines"}]});
        let r = digitize(&mut s, p.clone(), true);
        let navy = areas_of(&r, NAVY);
        let kinds: Vec<&Value> = navy.iter().map(|a| &a["kind"]).collect();
        assert_eq!(navy.iter().filter(|a| a["kind"] == json!("fill")).count(), 2, "shift {shift}: {kinds:?}: {}", r["areas"]);
        assert_eq!(navy.iter().filter(|a| !a["line"].is_null()).count(), 2, "shift {shift}: {}", r["areas"]);
        digitize(&mut s, p, false);
        let pts = needle_points(&s);
        for dot in [i, j] {
            assert!(pts.iter().any(|q| q.distance(dot) <= 0.5), "shift {shift}: the dot at {dot:?} isn't sewn");
        }
    }
}
