//! Digitizing from an image: centre lines. A thin line drawn in code (anti-aliased, as a picture's
//! edges are) is made a stroke along its middle and sewn in one go through its junctions; the
//! colours beside it reach its middle under it; a colour set to Fill keeps its lines filled, and one
//! not sewn keeps its line unsewn; a line's width doesn't depend on the picture's resolution; a line
//! its crossing widens to a millimetre is still sewn as a running line.

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
