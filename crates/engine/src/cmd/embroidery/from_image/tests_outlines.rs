//! Digitizing from an image: the outlines a design may be given, a running line along each colour
//! edge once and round the design, and a satin border round it (`super::outlines`); Create's
//! objects for them, and what the reply says of them.

use std::sync::Arc;

use kurbo::{ParamCurveArclen, Shape};
use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace as tr;

use super::command::digitized;
use super::outlines::{MEET_MM, outlined, parts};
use super::settings::Settings;
use super::stages::Digitized;
use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const GREEN: [u8; 4] = [30, 150, 60, 255];

/// A tenth of a millimetre a pixel: the working copy is the picture, pixel for pixel.
const MM_PER_PX: f64 = 0.1;

/// How near a length of outline must come to the one worked out from the picture (working
/// pixels): two pixels, for the corners the trace rounds a little.
const NEAR_PX: f64 = 2.0;

/// How exactly a test measures an outline's length (working pixels).
const ACCURACY_PX: f64 = 1e-3;

fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

/// What digitizing makes of `raster`, placed at a tenth of a millimetre a pixel, with `params`.
fn digitize(raster: &tr::Raster, params: &Value) -> Digitized {
    let png = raster.encode_png();
    let xf = Affine::scale(MM_PER_PX / MM_PER_PT);
    let image = ImageObject {
        key: "picture".into(),
        width: raster.width,
        height: raster.height,
        xf,
        link: None,
        placement: Default::default(),
        pixels: None,
    };
    let settings = Settings::from_params(params.as_object().unwrap(), Palette::Jef).unwrap();
    digitized(&image, &png, &settings, Palette::Jef, &|_, _| true).unwrap()
}

/// An orange square and a navy one against its right side, on white.
fn two_squares() -> tr::Raster {
    tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 160, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) {
            NAVY
        } else {
            WHITE
        }
    })
}

/// Whether `chain` closes on itself.
fn closed(chain: &super::outlines::Chain) -> bool {
    chain.path.elements().last() == Some(&kurbo::PathEl::ClosePath)
}

/// How long `chains` are together (working pixels).
fn length(chains: &[super::outlines::Chain]) -> f64 {
    chains.iter().map(|c| c.length_px).sum()
}

/// The lengths of `chains`, shortest first (working pixels).
fn lengths(chains: &[super::outlines::Chain]) -> Vec<f64> {
    let mut l: Vec<f64> = chains.iter().map(|c| c.length_px).collect();
    l.sort_by(f64::total_cmp);
    l
}

/// Two squares side by side sew the edge they share once: 100 pixels of it, the orange's 600 and
/// the navy's 400 round them, and the 800 of the two together round the design.
#[test]
fn two_squares_sew_their_shared_edge_once() {
    let d = digitize(&two_squares(), &json!({"colors": 2}));
    let edges = outlined(&d, false).unwrap();
    assert!((edges.colour_px() - 100.0).abs() < NEAR_PX, "the shared edge once: {}", edges.colour_px());
    assert!((length(&edges.silhouette) - 800.0).abs() < NEAR_PX, "round the design: {}", length(&edges.silhouette));
    assert_eq!(edges.silhouette.len(), 1, "one loop");
    assert!(edges.silhouette.iter().all(closed));
    let all = outlined(&d, true).unwrap();
    assert!((all.colour_px() - 900.0).abs() < NEAR_PX, "and round the design: {}", all.colour_px());
    assert_eq!(parts(&all.colour, MEET_MM / MM_PER_PX), 1, "one line, no jump");
}

/// The colour outlines are as long as half of every area's outline and the silhouette together:
/// each edge between two areas is sewn once of the two areas' outlines that run along it, each
/// edge of the silhouette once of one.
fn length_identity_holds(d: &Digitized) -> Result<(), String> {
    let all = outlined(d, true).map_err(|e| e.to_string())?;
    let silhouette = length(&outlined(d, false).map_err(|e| e.to_string())?.silhouette);
    let own = &d.shapes().own;
    let sewn = |a: usize| d.areas().areas.get(a).and_then(|x| d.colours().rows.get(x.row)).is_some_and(super::colours::Colour::sewn);
    let perimeters: f64 =
        own.iter().enumerate().filter(|(a, _)| sewn(*a)).map(|(_, p)| p.to_bezpath().segments().map(|s| s.arclen(ACCURACY_PX)).sum::<f64>()).sum();
    let want = (perimeters + silhouette) / 2.0;
    // A pixel's worth of each area's corners, at most, where the trace rounds what lies across.
    let near = NEAR_PX * (1.0 + d.order().sewn.len() as f64);
    if (all.colour_px() - want).abs() <= near {
        Ok(())
    } else {
        Err(format!("colour outlines {} px, half the outlines and the silhouette {want} px", all.colour_px()))
    }
}

/// A picture of up to four rectangles in three colours on white, from `rects` (each its corner,
/// size and colour), 200 × 150 pixels.
fn rectangles(rects: &[(u32, u32, u32, u32, usize)]) -> tr::Raster {
    let colours = [ORANGE, NAVY, GREEN];
    tr::Raster::from_fn(200, 150, |x, y| {
        let mut c = WHITE;
        for &(x0, y0, w, h, k) in rects {
            if inside(x, y, [x0, y0, x0 + w, y0 + h]) {
                c = colours.get(k).copied().unwrap_or(WHITE);
            }
        }
        c
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// The length identity holds whatever rectangles the picture holds.
    #[test]
    fn the_colour_outlines_are_half_the_areas_outlines_and_the_silhouette(
        rects in prop::collection::vec((10u32..120, 10u32..90, 30u32..80, 30u32..60, 0usize..3), 1..5)
    ) {
        let d = digitize(&rectangles(&rects), &json!({"colors": 4, "minArea": 2.0}));
        prop_assert!(length_identity_holds(&d).is_ok(), "{:?}", length_identity_holds(&d));
    }
}

/// A ring's hole left unsewn is a loop of the silhouette, and the border runs round it too. (The
/// white in it touches no edge of the picture, so it is sewn unless it is left out, as the white
/// inside a letter is.)
#[test]
fn a_rings_hole_gets_a_border_loop() {
    let ring =
        tr::Raster::from_fn(200, 200, |x, y| if inside(x, y, [40, 40, 160, 160]) && !inside(x, y, [80, 80, 120, 120]) { ORANGE } else { WHITE });
    let d = digitize(&ring, &json!({"colors": 2, "colorChoices": [{"color": "#ffffff", "job": "none"}]}));
    let o = outlined(&d, false).unwrap();
    assert_eq!(o.silhouette.len(), 2, "round the ring and round its hole");
    assert!(o.silhouette.iter().all(closed));
    let l = lengths(&o.silhouette);
    assert!((l[0] - 160.0).abs() < NEAR_PX && (l[1] - 480.0).abs() < NEAR_PX, "{l:?}");
    assert!(o.colour.is_empty(), "no colour meets another");
}

/// Three areas meeting at a point give chained lines: the edge running straight through the
/// junction is one line, and the third edge goes on from it, with no jump between them.
#[test]
fn three_areas_at_a_junction_give_chained_lines() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| match (x < 150, y < 100) {
        (true, _) => ORANGE,
        (false, true) => NAVY,
        (false, false) => GREEN,
    });
    let d = digitize(&picture, &json!({"colors": 3, "background": "none"}));
    let o = outlined(&d, false).unwrap();
    let l = lengths(&o.colour);
    assert_eq!(l.len(), 2, "{l:?}");
    assert!((l[0] - 150.0).abs() < NEAR_PX && (l[1] - 200.0).abs() < NEAR_PX, "the edge through the junction whole: {l:?}");
    assert_eq!(parts(&o.colour, MEET_MM / MM_PER_PX), 1, "they meet: no jump");
}

/// A session holding `raster` placed at a tenth of a millimetre a pixel at the origin, selected,
/// and the image's id.
fn placed(raster: &tr::Raster) -> (Session, NodeId) {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    let mut id = NodeId(0);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(MM_PER_PX / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    (s, id)
}

/// The design's colour blocks in sewing order, each its objects.
fn blocks(s: &Session, group: u64) -> Vec<Vec<Arc<Node>>> {
    let doc = &s.doc().unwrap().doc;
    let design = doc.node(NodeId(group)).unwrap();
    design.children().unwrap().iter().map(|b| b.children().unwrap().to_vec()).collect()
}

/// A stroke object's path, its stroke's colour and its width (points).
fn stroke_of(n: &Node) -> (kurbo::BezPath, [u8; 3], f64) {
    let NodeKind::Path { path, .. } = &n.kind else { panic!("{:?} isn't a path", n.name) };
    let stroke = n.appearance.stroke().expect("a stroke");
    let vectorcraft_color::Paint::Solid { color, .. } = &stroke.paint else { panic!("a solid stroke") };
    let [r, g, b, _] = color.to_rgba8(1.0);
    (path.to_bezpath(), [r, g, b], stroke.width)
}

/// The colour outlines and the border come last, one stroke object in a block each, in the darkest
/// thread the colours are sewn in (the navy's), the outlines drawn 0.4 mm wide and the border
/// 3 mm by default; the reply says what they came to, as its dry run did.
#[test]
fn outlines_are_sewn_last_one_block_each_in_the_darkest_thread() {
    let (mut s, _) = placed(&two_squares());
    let params = json!({"colors": 2, "colorOutlines": true, "border": true, "limit": 0});
    let mut dry = params.clone();
    dry["dryRun"] = json!(true);
    let said = s.execute("embroidery.digitizeImage", &dry).unwrap();
    let r = s.execute("embroidery.digitizeImage", &params).unwrap();
    assert_eq!(r["outlined"], said["outlined"], "the dry run said so");
    let navy = r["colors"].as_array().unwrap().iter().find(|c| c["color"] == "#142878").unwrap()["thread"].clone();
    let thread = &r["outlined"]["thread"];
    assert_eq!((thread["palette"].clone(), thread["index"].clone()), (json!("jef"), navy["index"].clone()), "{}", r["outlined"]);
    assert_eq!((thread["name"].clone(), thread["color"].clone()), (navy["name"].clone(), navy["color"].clone()), "named as the colour list names it");
    let b = blocks(&s, r["created"]["group"].as_u64().unwrap());
    assert_eq!(r["created"]["blocks"].as_u64(), Some(b.len() as u64));
    let [.., outlines, border] = b.as_slice() else { panic!("{} blocks", b.len()) };
    assert_eq!((outlines.len(), border.len()), (1, 1), "one object each");
    assert_eq!((outlines[0].name.as_deref(), border[0].name.as_deref()), (Some("Color outlines"), Some("Border")));
    let (line, colour, width) = stroke_of(&outlines[0]);
    let navy_rgb = navy["color"].as_str().and_then(super::settings::hex_colour).unwrap();
    assert_eq!(colour, navy_rgb, "the darkest thread");
    assert!((width * MM_PER_PT - 0.4).abs() < 1e-9, "drawn 0.4 mm: {width} pt");
    // The border takes the silhouette: the outlines sew the shared edge alone, 10 mm.
    let length_mm: f64 = line.segments().map(|s| s.arclen(ACCURACY_PX)).sum::<f64>() * MM_PER_PT;
    assert!((length_mm - 10.0).abs() < NEAR_PX * MM_PER_PX, "{length_mm} mm");
    assert!((r["outlined"]["colorOutlines"]["length"].as_f64().unwrap() - 10.0).abs() < NEAR_PX * MM_PER_PX, "{}", r["outlined"]);
    let (round, colour, width) = stroke_of(&border[0]);
    assert_eq!(colour, navy_rgb);
    assert!((width * MM_PER_PT - 3.0).abs() < 1e-9, "3 mm: {width} pt");
    assert!((round.perimeter(ACCURACY_PX) * MM_PER_PT - 80.0).abs() < NEAR_PX * MM_PER_PX, "round the design");
    assert_eq!(r["outlined"]["border"]["lines"], json!(1));
}

/// Without outlines asked, there are none: no block of them, `outlined` null.
#[test]
fn no_outlines_unless_asked() {
    let (mut s, _) = placed(&two_squares());
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "limit": 0})).unwrap();
    assert_eq!(r["outlined"], Value::Null);
    assert_eq!(r["created"]["blocks"], json!(2));
}

/// The border's width and the outlines' thread are settings: a thread given is taken, one of
/// another palette refused, and so is a width past a satin's range.
#[test]
fn the_borders_width_and_the_outlines_thread_are_settings() {
    let (mut s, _) = placed(&two_squares());
    let r = s
        .execute(
            "embroidery.digitizeImage",
            &json!({"colors": 2, "border": true, "borderWidth": 5.0, "outlineThread": {"palette": "jef", "index": 2}, "limit": 0}),
        )
        .unwrap();
    let b = blocks(&s, r["created"]["group"].as_u64().unwrap());
    let (_, colour, width) = stroke_of(&b.last().unwrap()[0]);
    assert!((width * MM_PER_PT - 5.0).abs() < 1e-9, "{width} pt");
    let (rgb, _) = Palette::Jef.get(2).unwrap();
    assert_eq!(colour, rgb, "the thread given");
    assert_eq!(r["settings"]["outlineThread"], json!({"palette": "jef", "index": 2}));
    for refused in [json!({"borderWidth": 0.5}), json!({"borderWidth": 12}), json!({"outlineThread": {"palette": "pec", "index": 2}})] {
        let mut p = refused.clone();
        p["colors"] = json!(2);
        assert!(s.execute("embroidery.digitizeImage", &p).is_err(), "{refused}");
    }
}

/// Colour outlines along the image's own lines (thin navy lines across an orange square, sewn
/// along their middle) sew them twice there, and the reply warns of it; two squares don't.
#[test]
fn outlines_beside_the_images_lines_are_warned_of() {
    let lines = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 280, 180]) && ((95..101).contains(&x) || (195..201).contains(&x) || (97..103).contains(&y)) {
            NAVY
        } else if inside(x, y, [20, 20, 280, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let warned = |r: &Value| r["warnings"].as_array().unwrap().iter().any(|w| w["id"] == "outlinesBesideLines");
    let (mut s, _) = placed(&lines);
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "colorOutlines": true, "dryRun": true, "limit": 0})).unwrap();
    assert!(r["areas"].is_array() && r["areaCount"].as_u64() > Some(1));
    assert!(warned(&r), "{}", r["warnings"]);
    let (mut s, _) = placed(&two_squares());
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "colorOutlines": true, "dryRun": true, "limit": 0})).unwrap();
    assert!(!warned(&r), "{}", r["warnings"]);
}
