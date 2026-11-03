//! Digitizing from an image: the shapes the areas are sewn as, and the overlap.

use kurbo::Shape;
use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const GREEN: [u8; 4] = [30, 150, 60, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// How near (millimetres) a shape's edge must lie to where it is asked: a fifth of a working
/// pixel, so that a pixel's error in a width shows, the fitting's own error far under it.
const NEAR_MM: f64 = 0.02;

/// A session holding `raster` placed at a tenth of a millimetre a pixel, its corner at the origin.
fn placed(raster: &tr::Raster) -> Session {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(0.1 / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    s
}

fn dry(s: &mut Session, params: Value) -> Value {
    let mut p = params;
    p["dryRun"] = json!(true);
    p["outlines"] = json!(true);
    s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn hex(c: [u8; 4]) -> Value {
    json!(super::super::hex([c[0], c[1], c[2]]))
}

/// The one area of `colour`, its outline's box (millimetres) and its outline's area (mm²).
fn shape_of(r: &Value, colour: [u8; 4]) -> (Value, kurbo::Rect, f64) {
    let area =
        r["areas"].as_array().unwrap().iter().find(|a| a["color"] == hex(colour)).unwrap_or_else(|| panic!("{colour:?}: {}", r["areas"])).clone();
    let path = kurbo::BezPath::from_svg(area["outline"].as_str().unwrap()).unwrap();
    (area, path.bounding_box(), path.area().abs())
}

fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

/// Two squares side by side: the larger, sewn first, reaches 0.4 mm under the smaller only along
/// their shared edge; its other edges stay where the picture has them, and the smaller is as drawn.
/// With no overlap, they meet edge to edge.
#[test]
fn the_colour_sewn_first_reaches_under_the_next() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 160, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) {
            NAVY
        } else {
            CLEAR
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2}));
    let (orange, ob, _) = shape_of(&r, ORANGE);
    let (navy, nb, _) = shape_of(&r, NAVY);
    assert!(orange["area"].as_u64() < navy["area"].as_u64(), "the larger first");
    assert_eq!(orange["grownUnder"], json!([navy["area"]]));
    assert!((ob.x1 - 16.4).abs() < NEAR_MM, "0.4 mm under the navy: {ob:?}");
    assert!((ob.x0 - 2.0).abs() < NEAR_MM && (ob.y0 - 2.0).abs() < NEAR_MM && (ob.y1 - 18.0).abs() < NEAR_MM, "its other edges as drawn: {ob:?}");
    assert!((nb.x0 - 16.0).abs() < NEAR_MM && (nb.x1 - 26.0).abs() < NEAR_MM, "the navy as drawn: {nb:?}");
    // The strip along 10 mm of shared edge, 0.4 mm deep.
    assert!((r["doubledArea"].as_f64().unwrap() - 4.0).abs() < 0.1, "{}", r["doubledArea"]);
    let r = dry(&mut s, json!({"colors": 2, "overlap": 0}));
    let (orange, ob, _) = shape_of(&r, ORANGE);
    assert!((ob.x1 - 16.0).abs() < NEAR_MM, "edge to edge: {ob:?}");
    assert_eq!((&orange["grownUnder"], &r["doubledArea"]), (&json!([]), &json!(0.0)), "nothing grown under");
}

/// Past the most edges the overlap grows along, an overlap asked for is left out with a warning,
/// and nothing is listed as grown under; with no overlap asked, there is nothing to leave out and no
/// warning. A checkerboard of 2 mm squares in two colours: 2,500 squares, the first colour's each
/// touching four of the second's.
#[test]
fn the_edge_warning_comes_only_with_an_overlap_to_leave_out() {
    let picture = tr::Raster::from_fn(1000, 1000, |x, y| if (x / 20 + y / 20) % 2 == 0 { ORANGE } else { NAVY });
    let mut s = placed(&picture);
    let run = |s: &mut Session, overlap: f64| {
        let p = json!({"dryRun": true, "colors": 2, "background": "none", "overlap": overlap, "limit": MAX_LISTED});
        s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let warned = |r: &Value| r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("edges"));
    let grown = |r: &Value| r["areas"].as_array().unwrap().iter().any(|a| a["grownUnder"] != json!([]));
    let asked = run(&mut s, 0.4);
    assert_eq!(asked["areaCount"], json!(2500));
    assert!(warned(&asked) && !grown(&asked), "{}", asked["warnings"]);
    assert_eq!(asked["doubledArea"], json!(0.0));
    let none = run(&mut s, 0.0);
    assert!(!warned(&none) && !grown(&none), "{}", none["warnings"]);
}

/// The most areas a reply lists.
const MAX_LISTED: usize = 10_000;

/// The overlap never reaches the background or an area left unsewn, and reaches at most half way
/// under a thin area: two fills on either side of a navy band 0.6 mm wide each reach 0.3 mm under
/// it, meeting in its middle.
#[test]
fn the_overlap_stops_at_what_isnt_sewn_and_halfway_under_thin_areas() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 140, 180]) {
            ORANGE
        } else if inside(x, y, [140, 20, 146, 180]) {
            NAVY
        } else if inside(x, y, [146, 20, 280, 180]) {
            GREEN
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let (_, ob, _) = shape_of(&r, ORANGE);
    let (_, gb, _) = shape_of(&r, GREEN);
    let (navy, _, _) = shape_of(&r, NAVY);
    assert!(navy["area"].as_u64() > shape_of(&r, ORANGE).0["area"].as_u64() && navy["area"].as_u64() > shape_of(&r, GREEN).0["area"].as_u64());
    assert!((ob.x1 - 14.3).abs() < NEAR_MM, "half the band: {ob:?}");
    assert!((gb.x0 - 14.3).abs() < NEAR_MM, "half the band: {gb:?}");
    assert!((ob.x0 - 2.0).abs() < NEAR_MM && (ob.y0 - 2.0).abs() < NEAR_MM, "never into the background: {ob:?}");
    // A band of an odd width, 0.3 mm: the two strips meet in its middle, neither crossing it.
    let odd = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 140, 180]) {
            ORANGE
        } else if inside(x, y, [140, 20, 143, 180]) {
            NAVY
        } else if inside(x, y, [143, 20, 280, 180]) {
            GREEN
        } else {
            WHITE
        }
    });
    let mut odd = placed(&odd);
    let r = dry(&mut odd, json!({"colors": 3}));
    let (_, ob, _) = shape_of(&r, ORANGE);
    let (_, gb, _) = shape_of(&r, GREEN);
    assert!((ob.x1 - 14.15).abs() < NEAR_MM && (gb.x0 - 14.15).abs() < NEAR_MM, "half of 0.3 mm each: {ob:?} {gb:?}");
    // The green left unsewn: nothing reaches under it.
    let r = dry(&mut s, json!({"colors": 3, "colorChoices": [{"color": hex(GREEN), "job": "none"}]}));
    let (green, _, _) = shape_of(&r, GREEN);
    assert_eq!(green["sewn"], json!(false));
    let (navy, nb, _) = shape_of(&r, NAVY);
    assert!(navy["grownUnder"].as_array().unwrap().is_empty(), "{navy}");
    assert!((nb.x1 - 14.6).abs() < NEAR_MM, "{nb:?}");
}

/// An island sewn over the area round it leaves no hole in it: the area is sewn solid, its outline
/// as large as the whole.
#[test]
fn an_island_sewn_over_leaves_no_hole() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [90, 90, 110, 110]) {
            NAVY
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2}));
    let (dot, _, _) = shape_of(&r, NAVY);
    assert_eq!(dot["sewnOver"], json!(true));
    let (orange, _, area) = shape_of(&r, ORANGE);
    assert!((area - 256.0).abs() < 0.5, "16 × 16 mm, solid: {area}");
    assert!(orange["grownUnder"].as_array().unwrap().is_empty(), "nothing to grow under: it lies under the dot whole");
}

/// An island whose colour is not sewn keeps its hole in the area round it, so that the fabric
/// shows there, however narrow: a 2 mm dot left unsewn in a 16 mm square leaves 252 mm² sewn.
#[test]
fn an_unsewn_island_keeps_its_hole() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [90, 90, 110, 110]) {
            NAVY
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "none"}]}));
    let (dot, _, _) = shape_of(&r, NAVY);
    assert_eq!((&dot["sewn"], &dot["sewnOver"]), (&json!(false), &json!(false)), "{dot}");
    let (_, _, area) = shape_of(&r, ORANGE);
    assert!((area - 252.0).abs() < 0.5, "16 × 16 mm less the 2 × 2 mm dot: {area}");
}

/// A dot inside a thin outline is sewn before the outline (a line, sewn after every fill), so the
/// outline keeps its hole for it and the dot grows under it: the outline 0.6 mm wide round a 3 mm
/// dot is 8.64 mm², not the solid 17.64 mm² square that would cover the dot.
#[test]
fn a_dot_inside_a_thin_outline_is_not_covered_by_it() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [86, 86, 116, 116]) {
            GREEN
        } else if inside(x, y, [80, 80, 122, 122]) {
            NAVY
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let (dot, _, _) = shape_of(&r, GREEN);
    let (ring, _, ring_mm2) = shape_of(&r, NAVY);
    assert_eq!((&ring["kind"], &dot["inside"]), (&json!("line"), &ring["area"]));
    assert!(dot["area"].as_u64() < ring["area"].as_u64(), "the line sewn after the dot");
    assert_eq!(dot["sewnOver"], json!(false));
    assert!((ring_mm2 - 8.64).abs() < 0.3, "the ring alone: {ring_mm2}");
    assert!(dot["grownUnder"].as_array().unwrap().contains(&ring["area"]), "{dot}");
}

/// An island holding an island is an island as a plain dot is (the project owner's choice): an eye
/// with its pupil inside a face is sewn over the face, measured with its pupil filled in, and the
/// pupil over the eye; the face and the eye are solid under them.
#[test]
fn an_eye_with_its_pupil_is_sewn_over_a_solid_face() {
    let disc = |x: u32, y: u32, (cx, cy, r): (i64, i64, i64)| (i64::from(x) - cx).pow(2) + (i64::from(y) - cy).pow(2) < r * r;
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if disc(x, y, (100, 100, 7)) {
            NAVY
        } else if disc(x, y, (100, 100, 18)) {
            GREEN
        } else if disc(x, y, (100, 100, 60)) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let ((face, _, face_mm2), (eye, _, eye_mm2), (pupil, _, _)) = (shape_of(&r, ORANGE), shape_of(&r, GREEN), shape_of(&r, NAVY));
    assert_eq!((&eye["inside"], &eye["sewnOver"]), (&face["area"], &json!(true)), "{eye}");
    assert_eq!((&pupil["inside"], &pupil["sewnOver"]), (&eye["area"], &json!(true)), "{pupil}");
    let mm2 = |r: f64| std::f64::consts::PI * r * r / 100.0;
    assert!((face_mm2 - mm2(60.0)).abs() < 1.5, "the face solid: {face_mm2}");
    assert!((eye_mm2 - mm2(18.0)).abs() < 0.5, "the eye solid: {eye_mm2}");
}

/// A small letter O and its counter each follow the rule for islands: the O, 3 × 3.6 mm with its
/// counter filled in, is sewn over the field round it, and its counter over it.
#[test]
fn a_letter_and_its_counter_are_islands() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [88, 88, 102, 108]) {
            WHITE
        } else if inside(x, y, [80, 80, 110, 116]) {
            NAVY
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let field = shape_of(&r, ORANGE);
    let letter = shape_of(&r, NAVY);
    let counter = r["areas"].as_array().unwrap().iter().find(|a| a["color"] == hex(WHITE)).unwrap().clone();
    assert_eq!((&letter.0["inside"], &letter.0["sewnOver"]), (&field.0["area"], &json!(true)), "{}", letter.0);
    assert_eq!((&counter["inside"], &counter["sewnOver"]), (&letter.0["area"], &json!(true)), "{counter}");
    assert!((field.2 - 256.0).abs() < 0.5 && (letter.2 - 10.8).abs() < 0.3, "both solid: {} {}", field.2, letter.2);
}
