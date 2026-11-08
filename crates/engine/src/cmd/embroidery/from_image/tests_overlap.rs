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

/// The strip grows from the edge two areas share alone: orange and green, kept apart by a navy
/// band 0.3 mm wide and touching below it, are not joined by a sliver of the green across the band,
/// 7 mm from where they meet.
#[test]
fn the_strip_stays_along_the_shared_edge() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [140, 20, 143, 150]) {
            NAVY
        } else if inside(x, y, [20, 20, 140, 180]) || inside(x, y, [140, 150, 143, 180]) {
            ORANGE
        } else if inside(x, y, [143, 20, 260, 180]) {
            GREEN
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let (orange, ..) = shape_of(&r, ORANGE);
    let outline = kurbo::BezPath::from_svg(orange["outline"].as_str().unwrap()).unwrap();
    assert_eq!(outline.winding(kurbo::Point::new(14.35, 8.0)), 0, "no sliver of the green across the band");
    assert_ne!(outline.winding(kurbo::Point::new(14.35, 16.5)), 0, "under the green where they meet");
}

/// Two strips never stack under a narrow neck: orange and navy, both sewn before the green, face
/// each other across a neck of it 0.5 mm wide; each reaches 0.4 mm under the green, so the one sewn
/// later gives way where they would meet, and no point of the neck lies under both.
#[test]
fn two_strips_never_stack_under_a_neck() {
    let picture = tr::Raster::from_fn(220, 200, |x, y| {
        if inside(x, y, [20, 20, 100, 160]) {
            ORANGE
        } else if inside(x, y, [105, 20, 185, 160]) {
            NAVY
        } else if inside(x, y, [100, 20, 105, 160]) || inside(x, y, [20, 160, 185, 175]) {
            GREEN
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    let (green, ..) = shape_of(&r, GREEN);
    let (orange, ..) = shape_of(&r, ORANGE);
    let (navy, ..) = shape_of(&r, NAVY);
    assert!(orange["area"].as_u64() < green["area"].as_u64() && navy["area"].as_u64() < green["area"].as_u64(), "both sewn first");
    let covers = |a: &Value, x: f64| kurbo::BezPath::from_svg(a["outline"].as_str().unwrap()).unwrap().winding(kurbo::Point::new(x, 9.0)) != 0;
    for x in [10.05, 10.15, 10.25, 10.35, 10.45] {
        assert!(!(covers(&orange, x) && covers(&navy, x)), "both under the neck at {x} mm");
        assert!(covers(&orange, x) || covers(&navy, x), "the neck covered at {x} mm");
    }
}

/// The overlap is made however many edges the areas meet along: a checkerboard of 2 mm squares in
/// two colours, 2,500 squares meeting along some 4,900 edges, has each square of the colour sewn
/// first grown under its neighbours of the other, with nothing left out; with no overlap asked,
/// none is.
#[test]
fn the_overlap_is_made_however_many_edges_the_areas_meet_along() {
    let picture = tr::Raster::from_fn(1000, 1000, |x, y| if (x / 20 + y / 20) % 2 == 0 { ORANGE } else { NAVY });
    let mut s = placed(&picture);
    let run = |s: &mut Session, overlap: f64| {
        let p = json!({"dryRun": true, "colors": 2, "background": "none", "overlap": overlap, "limit": MAX_LISTED});
        s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let grown = |r: &Value| r["areas"].as_array().unwrap().iter().filter(|a| a["grownUnder"] != json!([])).count();
    let asked = run(&mut s, 0.4);
    assert_eq!(asked["areaCount"], json!(2500));
    assert_eq!(asked["warnings"], json!([]));
    assert_eq!(grown(&asked), 1250, "every square of the colour sewn first");
    assert!(asked["doubledArea"].as_f64().unwrap() > 0.0, "{}", asked["doubledArea"]);
    let none = run(&mut s, 0.0);
    assert_eq!((grown(&none), &none["doubledArea"]), (0, &json!(0.0)));
}

/// An outline straightened to cut strips from loses the points along its straight runs, and only
/// those: a side split at each of a hundred specks comes back as one line, as does a side with a
/// point off it by under half the path operations' precision (4 thousandths of a pixel), while a
/// point off by 2 hundredths, a curve and a line turning back stay.
#[test]
fn an_outline_to_cut_from_is_straightened_and_no_more() {
    use kurbo::{BezPath, PathEl, Point};
    let mut p = BezPath::new();
    p.move_to((0.0, 0.0));
    for x in 1..=100 {
        p.line_to((f64::from(x), 0.0));
    }
    for point in [(100.0, 50.0), (100.0, 100.0), (60.0, 100.0), (50.0, 100.004), (0.0, 100.0), (0.0, 60.0), (0.02, 50.0), (0.0, 40.0)] {
        p.line_to(point);
    }
    p.curve_to((-5.0, 30.0), (-5.0, 10.0), (0.0, 0.0));
    p.close_path();
    let line = |x: f64, y: f64| PathEl::LineTo(Point::new(x, y));
    let kept = [
        PathEl::MoveTo(Point::ZERO),
        line(100.0, 0.0),
        line(100.0, 100.0),
        line(0.0, 100.0),
        line(0.0, 60.0),
        line(0.02, 50.0),
        line(0.0, 40.0),
        PathEl::CurveTo(Point::new(-5.0, 30.0), Point::new(-5.0, 10.0), Point::ZERO),
        PathEl::ClosePath,
    ];
    assert_eq!(super::overlap::straightened(&p).elements(), kept);
    let mut back = BezPath::new();
    back.move_to((0.0, 0.0));
    back.line_to((10.0, 0.0));
    back.line_to((5.0, 0.0));
    assert_eq!(super::overlap::straightened(&back).elements(), back.elements(), "a turn back is no straight run");
}

/// The most areas a reply lists.
const MAX_LISTED: usize = 10_000;

/// The numbers of SVG path `d`.
fn numbers(d: &str) -> Vec<&str> {
    d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty()).collect()
}

/// The outlines are given in millimetres to hundredths, as the rest of the reply is, and the areas
/// listed with them end where their path elements would pass the most a reply gives, `truncated`
/// saying more follow: a checkerboard of 4,900 squares of 1.2 mm, its outlines far past it.
#[test]
fn outlines_are_rounded_and_capped() {
    let picture = tr::Raster::from_fn(840, 840, |x, y| if (x / 12 + y / 12) % 2 == 0 { ORANGE } else { NAVY });
    let mut s = placed(&picture);
    let p = json!({"dryRun": true, "colors": 2, "background": "none", "overlap": 0, "outlines": true, "limit": MAX_LISTED});
    let r = s.execute("embroidery.digitizeImage", &p).unwrap();
    let areas = r["areas"].as_array().unwrap();
    assert_eq!(r["areaCount"], json!(4900));
    assert!(areas.len() < 4900 && r["truncated"] == json!(true), "{} listed", areas.len());
    let elements: usize = areas.iter().map(|a| kurbo::BezPath::from_svg(a["outline"].as_str().unwrap()).unwrap().elements().len()).sum();
    assert!(elements <= super::command::MAX_OUTLINE_ELEMENTS, "{elements} elements");
    for a in areas {
        for n in numbers(a["outline"].as_str().unwrap()) {
            assert!(n.split('.').nth(1).is_none_or(|d| d.len() <= 2), "{n} in {}", a["outline"]);
        }
    }
    // Paged on from where it ended, the next areas come with their outlines.
    let mut next = p.clone();
    next["offset"] = json!(areas.len());
    let more = s.execute("embroidery.digitizeImage", &next).unwrap();
    assert_eq!(more["areas"][0]["area"], json!(areas.len() + 1));
}

/// How far from a centre line's middle the colours either side of it meet (millimetres): each
/// reaches half a working pixel past it (`overlap::PAST_MIDDLE_PX`), the one sewn later giving way
/// where the two would overlap.
const MIDDLE_MM: f64 = 0.05 + NEAR_MM;

/// The overlap never reaches the background or an area left unsewn, and under a thin line the
/// colours either side of it meet at its middle: two fills either side of a navy band 0.6 mm wide
/// meet within half a pixel of its middle, with no fabric between them.
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
    assert!((ob.x1 - 14.3).abs() < MIDDLE_MM && (gb.x0 - 14.3).abs() < MIDDLE_MM, "to the band's middle: {ob:?} {gb:?}");
    assert!(ob.x1 >= gb.x0 - NEAR_MM, "fabric between them: {ob:?} {gb:?}");
    assert!((ob.x0 - 2.0).abs() < NEAR_MM && (ob.y0 - 2.0).abs() < NEAR_MM, "never into the background: {ob:?}");
    // A band of an odd width, 0.3 mm: the two strips meet at its middle.
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
    assert!((ob.x1 - 14.15).abs() < MIDDLE_MM && (gb.x0 - 14.15).abs() < MIDDLE_MM && ob.x1 >= gb.x0 - NEAR_MM, "to its middle: {ob:?} {gb:?}");
    // The green left unsewn: nothing reaches under it.
    let r = dry(&mut s, json!({"colors": 3, "colorChoices": [{"color": hex(GREEN), "job": "none"}]}));
    let (green, _, _) = shape_of(&r, GREEN);
    assert_eq!(green["sewn"], json!(false));
    let (navy, nb, _) = shape_of(&r, NAVY);
    assert!(navy["grownUnder"].as_array().unwrap().is_empty(), "{navy}");
    // The band, a line, is sewn along its middle, which the reply's outline gives.
    assert!(navy["line"].is_object() && nb.x0 >= 14.0 && nb.x1 <= 14.6, "{nb:?}");
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

/// Two islands side by side in one hole of the area round them, neither filling it alone, are
/// sewn over it all the same: the area made solid by joining them, as one island's hole is closed.
#[test]
fn two_islands_in_one_hole_are_sewn_over() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [90, 90, 100, 110]) {
            NAVY
        } else if inside(x, y, [100, 90, 110, 110]) {
            GREEN
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    for colour in [NAVY, GREEN] {
        assert_eq!(shape_of(&r, colour).0["sewnOver"], json!(true), "{colour:?}");
    }
    let (_, _, area) = shape_of(&r, ORANGE);
    assert!((area - 256.0).abs() < 0.5, "16 × 16 mm, solid: {area}");
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

/// A dot inside a thin outline is sewn before the outline (a line, sewn after every fill, along its
/// middle), so the outline covers nothing of it and the dot grows under it to the outline's
/// middle: the outline 0.6 mm wide round a 3 mm dot is a loop round the band's middle, 3.6 mm a
/// side (12.96 mm² within it), not the solid 4.2 mm square that would cover the dot.
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
    assert_eq!((&ring["line"]["pieces"], &ring["line"]["ends"]), (&json!(1), &json!(0)), "{ring}");
    assert!((ring_mm2 - 3.6 * 3.6).abs() < 0.6, "the loop round the ring's middle: {ring_mm2}");
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

/// A small letter O and its counter each follow the rule for islands: the O, 3.4 × 4 mm with its
/// counter filled in and its sides 1.2 mm wide (a fill, not a line), is sewn over the field round
/// it, and its counter over it.
#[test]
fn a_letter_and_its_counter_are_islands() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [92, 92, 102, 108]) {
            WHITE
        } else if inside(x, y, [80, 80, 114, 120]) {
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
    assert!((field.2 - 256.0).abs() < 0.5 && (letter.2 - 13.6).abs() < 0.3, "both solid: {} {}", field.2, letter.2);
}
