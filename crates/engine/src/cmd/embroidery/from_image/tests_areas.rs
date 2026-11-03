//! Digitizing from an image: the areas, the rules that keep or join them, and their order.

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

/// A session holding `raster` placed at a tenth of a millimetre a pixel (10 pixels a millimetre),
/// selected.
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
    s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn hex(c: [u8; 4]) -> Value {
    json!(super::super::hex([c[0], c[1], c[2]]))
}

/// The areas of `colour` in the reply.
fn areas_of(r: &Value, colour: [u8; 4]) -> Vec<Value> {
    r["areas"].as_array().unwrap().iter().filter(|a| a["color"] == hex(colour)).cloned().collect()
}

/// Whether `(x, y)` lies in the rectangle `[x0, y0, x1, y1)`.
fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

/// Specks join the area round them, smallest first, while they are under the smallest area (1 mm²
/// by default, 100 pixels here): of navy squares of 0.49, 0.81, 1.21 and 2.25 mm² in an orange
/// field, the two larger stay. On transparency, with nothing to join, a speck is left out.
#[test]
fn specks_join_the_area_round_them() {
    let squares = [[40, 40, 47, 47], [100, 40, 109, 49], [160, 40, 171, 51], [220, 40, 235, 55]];
    let picture = tr::Raster::from_fn(300, 100, |x, y| if squares.iter().any(|&r| inside(x, y, r)) { NAVY } else { ORANGE });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2, "background": "none"}));
    let navy = areas_of(&r, NAVY);
    let sizes: Vec<f64> = navy.iter().map(|a| a["size"]["area"].as_f64().unwrap()).collect();
    assert_eq!(navy.len(), 2, "{sizes:?}");
    assert!(sizes.iter().all(|&a| a > 1.0), "{sizes:?}");
    assert_eq!(r["joined"], json!(2));
    assert_eq!(areas_of(&r, ORANGE)[0]["joined"], json!(2), "both joined the orange");
    // Raising the smallest area joins the rest too, and never splits what was joined.
    let r = dry(&mut s, json!({"colors": 2, "background": "none", "minArea": 2.5}));
    assert_eq!(areas_of(&r, NAVY).len(), 0);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("every area of")), "{}", r["warnings"]);
    // A speck alone on transparency: nothing to join.
    let alone = tr::Raster::from_fn(200, 100, |x, y| {
        if inside(x, y, [20, 20, 80, 80]) {
            ORANGE
        } else if inside(x, y, [150, 40, 156, 46]) {
            NAVY
        } else {
            CLEAR
        }
    });
    let mut s = placed(&alone);
    let r = dry(&mut s, json!({"colors": 2}));
    assert_eq!((r["leftOut"].as_u64(), areas_of(&r, NAVY).len()), (Some(1), 0), "{}", r["areas"]);
}

/// A long thin line stays below the smallest area, as a line: 0.4 mm wide and 3 mm long, 1.2 mm²,
/// it is kept when the smallest area is 1.5 mm², while a stub 0.5 mm wide and 1.5 mm long joins.
/// A line of an odd width is as wide as it is: 0.3 mm wide and 2.4 mm long, 0.72 mm², it is long
/// enough to stay.
#[test]
fn long_thin_lines_stay() {
    let picture =
        tr::Raster::from_fn(200, 100, |x, y| if inside(x, y, [30, 40, 60, 44]) || inside(x, y, [120, 40, 135, 45]) { NAVY } else { ORANGE });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2, "background": "none", "minArea": 1.5}));
    let navy = areas_of(&r, NAVY);
    assert_eq!(navy.len(), 1, "{navy:?}");
    assert_eq!(navy[0]["kind"], json!("line"));
    assert!(navy[0]["widest"].as_f64().unwrap() < 1.0, "{}", navy[0]["widest"]);
    assert!(navy[0]["size"]["width"].as_f64().unwrap() > 2.5);
    let odd = tr::Raster::from_fn(100, 60, |x, y| if inside(x, y, [30, 30, 54, 33]) { NAVY } else { ORANGE });
    let mut s = placed(&odd);
    let r = dry(&mut s, json!({"colors": 2, "background": "none"}));
    let navy = areas_of(&r, NAVY);
    assert_eq!(navy.len(), 1, "{}", r["areas"]);
    assert_eq!((&navy[0]["kind"], &navy[0]["widest"]), (&json!("line"), &json!(0.3)));
    // A stem just narrower than 1 mm is a line; one 1 mm wide is a fill.
    for (wide, kind) in [(9, "line"), (10, "fill")] {
        let stem = tr::Raster::from_fn(400, 60, |x, y| if inside(x, y, [40, 20, 340, 20 + wide]) { NAVY } else { ORANGE });
        let mut s = placed(&stem);
        let r = dry(&mut s, json!({"colors": 2, "background": "none"}));
        assert_eq!(areas_of(&r, NAVY)[0]["kind"], json!(kind), "{wide} px wide");
    }
}

/// An island just narrower than 4 mm is sewn over the area round it, however long; one 4 mm wide
/// has its hole cut.
#[test]
fn islands_narrower_than_four_millimetres_are_sewn_over() {
    for (side, over) in [(39, true), (40, false)] {
        let picture = tr::Raster::from_fn(200, 200, |x, y| {
            if inside(x, y, [80, 60, 80 + side, 130]) {
                NAVY
            } else if inside(x, y, [20, 20, 180, 180]) {
                ORANGE
            } else {
                WHITE
            }
        });
        let mut s = placed(&picture);
        let r = dry(&mut s, json!({"colors": 2}));
        assert_eq!(areas_of(&r, NAVY)[0]["sewnOver"], json!(over), "{side} px");
    }
}

/// A band's widest width is its own, odd or even, lying or standing: the pixel an even width adds
/// is counted across the band, never along it. A square is as wide as its side.
#[test]
fn a_band_measures_its_own_width() {
    let (w, h) = (80u32, 80u32);
    let measured = |inside: &dyn Fn(u32, u32) -> bool| {
        let labels: Vec<u32> = (0..w * h).map(|i| u32::from(inside(i % w, i / w))).collect();
        super::regions::Regions::from_labels(labels, w, h, 2, None).areas[1].widest_px
    };
    for width in [1u32, 2, 3, 4, 5, 9, 10] {
        let lying = measured(&|x, y| (10..70).contains(&x) && (10..10 + width).contains(&y));
        let standing = measured(&|x, y| (10..10 + width).contains(&x) && (10..70).contains(&y));
        let square = measured(&|x, y| (10..10 + width).contains(&x) && (10..10 + width).contains(&y));
        assert_eq!((lying, standing, square), (f64::from(width), f64::from(width), f64::from(width)), "{width} px");
    }
}

/// An area inside another alone is sewn after it; narrower than 4 mm it is sewn over it, no hole
/// cut, wider it is cut out. Each is keyed by its deepest point, in the image's own pixels, and
/// the region map and `at` find it there.
#[test]
fn islands_are_sewn_after_the_area_round_them() {
    let (dot, disc) = ([60, 40, 80, 60], [130, 20, 190, 80]);
    let picture = tr::Raster::from_fn(220, 100, |x, y| {
        if inside(x, y, dot) {
            NAVY
        } else if inside(x, y, disc) {
            GREEN
        } else if inside(x, y, [10, 10, 210, 90]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3, "regions": true, "at": [160, 50]}));
    let orange = &areas_of(&r, ORANGE)[0];
    let (small, large) = (&areas_of(&r, NAVY)[0], &areas_of(&r, GREEN)[0]);
    for (island, over) in [(small, true), (large, false)] {
        assert_eq!(island["inside"], orange["area"], "{island}");
        assert_eq!(island["sewnOver"], json!(over), "{island}");
        assert!(island["area"].as_u64() > orange["area"].as_u64(), "sewn after: {island} {orange}");
        assert!(island["order"]["after"].as_array().unwrap().iter().any(|a| a["area"] == orange["area"] && a["because"] == json!("inside")));
        assert!(island["touches"].as_array().unwrap().iter().any(|t| t["area"] == orange["area"] && t["under"] == json!(true)));
    }
    // Its four middle pixels are as deep: the first in scan order, at its centre.
    assert_eq!(small["at"], json!([69.5, 49.5]), "the dot's deepest point, in the image's pixels");
    assert_eq!(r["areaAt"], large["area"], "the area under [160, 50]");
    // The region map: two bytes a pixel, each its area's number.
    let map = &r["regions"];
    let bytes = vectorcraft_format::base64_decode(map["base64"].as_str().unwrap()).unwrap();
    let (w, h) = (map["width"].as_u64().unwrap() as usize, map["height"].as_u64().unwrap() as usize);
    assert_eq!(bytes.len(), w * h * 2);
    let at = |x: usize, y: usize| u16::from_le_bytes([bytes[(y * w + x) * 2], bytes[(y * w + x) * 2 + 1]]);
    assert_eq!(json!(at(70, 50)), small["area"]);
    assert_eq!(json!(at(160, 50)), large["area"]);
    assert_eq!(at(2, 2), 0, "the background is no area sewn");
}

/// A line is sewn after the fills it touches; a large fill before the small ones beside it; one
/// thread is kept while its areas can be sewn.
#[test]
fn details_are_sewn_last() {
    // Two orange squares with a navy band across both, and a small green square beside one.
    let picture = tr::Raster::from_fn(300, 120, |x, y| {
        if inside(x, y, [20, 58, 280, 62]) {
            NAVY
        } else if inside(x, y, [20, 20, 130, 100]) || inside(x, y, [170, 20, 280, 100]) {
            ORANGE
        } else if inside(x, y, [130, 50, 150, 70]) {
            GREEN
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3, "minArea": 2}));
    let number = |c: [u8; 4]| areas_of(&r, c).iter().map(|a| a["area"].as_u64().unwrap()).collect::<Vec<_>>();
    let (orange, navy, green) = (number(ORANGE), number(NAVY), number(GREEN));
    assert!(!navy.is_empty() && !orange.is_empty() && !green.is_empty(), "{}", r["areas"]);
    assert!(navy.iter().all(|n| orange.iter().all(|o| n > o)), "the line after the fills: {orange:?} {navy:?}");
    assert!(green.iter().all(|g| orange.iter().any(|o| g > o)), "the small square after a larger neighbour: {orange:?} {green:?}");
    // The orange areas, one thread, follow each other.
    let mut sorted = orange.clone();
    sorted.sort_unstable();
    assert_eq!(sorted.windows(2).filter(|w| w[1] != w[0] + 1).count(), 0, "{orange:?}");
    // Listed a page at a time.
    let page = dry(&mut s, json!({"colors": 3, "minArea": 2, "offset": 1, "limit": 1}));
    assert_eq!((page["offset"].as_u64(), page["areas"].as_array().unwrap().len(), &page["truncated"]), (Some(1), 1, &json!(true)));
    assert_eq!(page["areas"][0]["area"], json!(2));
}

/// A thin band along an edge in nearly its neighbour's colour is what a picture leaves round a
/// sharp edge, not a line: a darker red ring 0.2 mm wide round a red square joins the red, whatever
/// its size, while a navy line 0.5 mm wide stays a line.
#[test]
fn slivers_join_the_colour_they_are_nearest() {
    let (red, dark) = ([220, 30, 40, 255], [195, 26, 38, 255]);
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [50, 50, 150, 150]) {
            red
        } else if inside(x, y, [48, 48, 152, 152]) {
            dark
        } else if inside(x, y, [170, 90, 280, 95]) {
            NAVY
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3}));
    assert!(super::rules::is_mix([128, 128, 128], [0, 0, 0], [255, 255, 255]));
    assert!(!super::rules::is_mix([250, 250, 250], [0, 0, 0], [255, 255, 255]), "too near an end");
    assert_eq!(areas_of(&r, dark).len(), 0, "{}", r["areas"]);
    let reds = areas_of(&r, red);
    assert_eq!(reds.len(), 1);
    assert!(reds[0]["size"]["area"].as_f64().unwrap() > 104.0, "the ring joined the red: {}", reds[0]["size"]);
    let navy = areas_of(&r, NAVY);
    assert_eq!((navy.len(), &navy[0]["kind"]), (1, &json!("line")));
}
