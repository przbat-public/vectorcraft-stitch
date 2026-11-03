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
    let (_, ob, _) = shape_of(&r, ORANGE);
    assert!((ob.x1 - 16.0).abs() < NEAR_MM, "edge to edge: {ob:?}");
    assert_eq!(r["doubledArea"], json!(0.0));
}

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
