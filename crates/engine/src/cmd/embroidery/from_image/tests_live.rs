//! Digitizing from an image: the live object (`live: true`, Keep as Live Object), digitized again
//! keeping its id, its objects' edits found, its change angles kept to the art, made plain, and
//! upstream's Image Trace refusing it while its View, Expand and Release still work.

use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use super::live::{edited, live_digitizing, live_settings};
use crate::Session;
use crate::cmd::buildcmds::TRACE_NAME;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];

/// A tenth of a millimetre a pixel.
const MM_PER_PX: f64 = 0.1;

fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

/// An orange square and a navy one apart, on white.
fn picture() -> tr::Raster {
    tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 140, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) {
            NAVY
        } else {
            WHITE
        }
    })
}

fn hex(c: [u8; 4]) -> String {
    super::super::hex([c[0], c[1], c[2]])
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

fn node(s: &Session, id: NodeId) -> Node {
    s.doc().unwrap().doc.node(id).cloned().expect("the node")
}

/// Node `id` transformed by `a`, its strokes scaled with it, as one undo step.
fn transformed(s: &mut Session, id: NodeId, a: Affine) {
    s.edit("transform", |d, _| {
        d.node_mut(id).ok_or(crate::EngineError::NoNode(id))?.transform(a, true);
        Ok(())
    })
    .unwrap();
}

/// How many nodes of the document are named `name`.
fn named(s: &Session, name: &str) -> usize {
    fn walk(n: &Node, name: &str) -> usize {
        usize::from(n.name.as_deref() == Some(name)) + n.children().map_or(0, |c| c.iter().map(|c| walk(c, name)).sum())
    }
    s.doc().unwrap().doc.layers.iter().map(|l| walk(l, name)).sum()
}

/// The live object made of the picture with `params` (and `live: true`), and its id.
fn live_made(params: Value) -> (Session, NodeId, NodeId) {
    let (mut s, image) = placed(&picture());
    let mut p = params;
    p["live"] = json!(true);
    let r = s.execute("embroidery.digitizeImage", &p).unwrap();
    assert_eq!(r["created"]["live"], json!(true));
    (s, NodeId(r["created"]["group"].as_u64().unwrap()), image)
}

#[test]
fn keep_as_live_object_makes_an_image_trace_object_holding_the_image_first() {
    let (s, live, image) = live_made(json!({"colors": 3}));
    let n = node(&s, live);
    assert_eq!(n.name.as_deref(), Some(TRACE_NAME), "named as Image Trace names its objects");
    assert!(n.extras().trace.as_deref().is_some(), "a trace value");
    let first = n.children().and_then(|c| c.first()).expect("children");
    assert!(first.id == image && matches!(first.kind, NodeKind::Image(_)) && !first.visible, "the image, with its id, hidden first");
    assert!(n.children().is_some_and(|c| c.len() > 1), "the objects after it");
    let d = live_digitizing(&n).expect("its digitizing");
    assert!(d.fingerprint.is_some() && d.size.is_some_and(|[w, h]| (w - 30.0).abs() < 0.01 && (h - 20.0).abs() < 0.01), "{:?}", d.size);
    assert_eq!(d.settings.get("colors"), Some(&json!(3)), "by file keys");
    assert!(d.settings.get("width").is_none(), "the size its image's");
    assert!(s.doc().unwrap().doc.layers.iter().all(|l| l.name.as_deref() != Some("Template Image")), "no template layer");
    assert_eq!(edited(&n), Some(false));
}

#[test]
fn a_live_object_digitized_again_keeps_its_id_and_takes_the_new_settings() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    let before = node(&s, live);
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colors": 2})).unwrap();
    assert_eq!(r["created"]["group"], json!(live.0), "the same id");
    let after = node(&s, live);
    assert_eq!(live_digitizing(&after).and_then(|d| d.settings.get("colors").cloned()), Some(json!(2)));
    let ids = |n: &Node| n.children().map(|c| c.iter().skip(1).map(|c| c.id).collect::<Vec<_>>()).unwrap_or_default();
    assert!(ids(&after).iter().all(|id| !ids(&before).contains(id)), "new objects");
    assert_eq!(named(&s, TRACE_NAME), 1, "one live object, not two");
    // Digitized again as it is: the same objects, so none reads as edited.
    let made = live_digitizing(&after).and_then(|d| d.fingerprint);
    s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colors": 2})).unwrap();
    let again = node(&s, live);
    assert_eq!(live_digitizing(&again).and_then(|d| d.fingerprint), made, "the same objects");
    assert_eq!(edited(&again), Some(false));
}

#[test]
fn a_live_objects_hand_edits_are_found_and_moving_it_edits_nothing() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    let dry = |s: &mut Session| s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap()["edited"].clone();
    assert_eq!(dry(&mut s), json!(false));
    // Moved, turned and scaled: no edit.
    transformed(&mut s, live, Affine::translate((40.0, -10.0)) * Affine::rotate(0.7) * Affine::scale(1.3));
    assert_eq!(dry(&mut s), json!(false), "moved, turned, scaled");
    // One object recoloured by hand: an edit.
    let object = node(&s, live).children().and_then(|c| c.get(1)).and_then(|b| b.children().and_then(|c| c.first()).map(|o| o.id)).unwrap();
    s.edit("recolour", |d, _| {
        let o = d.node_mut(object).ok_or(crate::EngineError::NoNode(object))?;
        o.appearance = vectorcraft_doc::Appearance::basic(
            vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(0, 255, 0)),
            vectorcraft_color::Paint::None,
            0.0,
        )
        .into();
        Ok(())
    })
    .unwrap();
    assert_eq!(dry(&mut s), json!(true), "recoloured");
    // One object moved alone, its shape edited against the image: an edit.
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    let dry = |s: &mut Session| s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap()["edited"].clone();
    let object = node(&s, live).children().and_then(|c| c.get(1)).and_then(|b| b.children().and_then(|c| c.first()).map(|o| o.id)).unwrap();
    transformed(&mut s, object, Affine::translate((1.0, 0.0)));
    assert_eq!(dry(&mut s), json!(true), "an object moved alone");
    let (mut plain, _) = placed(&picture());
    assert_eq!(plain.execute("embroidery.digitizeImage", &json!({"dryRun": true})).unwrap()["edited"], Value::Null, "an image: no live object");
}

#[test]
fn a_turned_live_object_keeps_each_changes_angle_to_the_art() {
    let changes = json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "angle": 30.0}]);
    let (mut s, live, _) = live_made(json!({"colors": 3, "areaChanges": changes}));
    transformed(&mut s, live, Affine::rotate(std::f64::consts::FRAC_PI_2));
    let fields = live_settings(&node(&s, live)).expect("its settings");
    let angle = fields["areaChanges"][0]["angle"].as_f64().unwrap();
    assert!((angle - 120.0).rem_euclid(180.0) < 0.01 || (angle - 120.0).rem_euclid(180.0) > 179.99, "turned with the art: {angle}");
    assert!(fields.get("width").is_none(), "the size its image's as placed");
    // Mirrored too, then digitized again with the settings as the window shows them: the angle kept
    // is still the art's.
    transformed(&mut s, live, Affine::scale_non_uniform(-1.0, 1.0));
    let mut again = Value::Object(live_settings(&node(&s, live)).expect("its settings"));
    again["id"] = json!(live.0);
    again["live"] = json!(true);
    s.execute("embroidery.digitizeImage", &again).unwrap();
    let kept = live_digitizing(&node(&s, live)).and_then(|d| d.settings.get("area_changes").cloned()).expect("kept changes");
    let kept = kept[0]["angle"].as_f64().expect("an angle");
    assert!((kept - 30.0).rem_euclid(180.0) < 0.01 || (kept - 30.0).rem_euclid(180.0) > 179.99, "the art's angle: {kept}");
}

#[test]
fn create_on_a_live_object_makes_plain_objects() {
    let (mut s, live, image) = live_made(json!({"colors": 3}));
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "colors": 3})).unwrap();
    assert_eq!(r["created"]["live"], json!(false));
    let d = &s.doc().unwrap().doc;
    assert!(d.node(live).is_none(), "the live object gone");
    let template = d.layers.first().unwrap();
    assert!(template.children().is_some_and(|c| c.iter().any(|c| c.id == image)), "its image on the template layer");
}

#[test]
fn upstreams_view_expand_and_release_work_on_a_live_object() {
    let (mut s, live, image) = live_made(json!({"colors": 3}));
    s.execute("imageTrace.setView", &json!({"ids": [live.0], "view": "sourceImage"})).unwrap();
    let (mut released, _, _) = live_made(json!({"colors": 3}));
    s.execute("imageTrace.expand", &json!({"ids": [live.0]})).unwrap();
    let expanded = node(&s, live);
    assert!(expanded.name.is_none() && expanded.children().is_some_and(|c| c.iter().all(|c| !matches!(c.kind, NodeKind::Image(_)))), "plain objects");
    assert!(live_digitizing(&expanded).is_none(), "no live object any more");
    released.execute("imageTrace.release", &json!({"ids": [live.0]})).unwrap();
    assert!(released.doc().unwrap().doc.node(image).is_some_and(|n| n.visible), "its image back");
}

#[test]
fn image_trace_refuses_a_live_embroidery_object() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    for cmd in ["imageTrace.make", "imageTrace.makeAndExpand"] {
        let e = s.execute(cmd, &json!({"id": live.0})).unwrap_err().to_string();
        assert!(e.contains("Digitize Image"), "{cmd}: {e}");
    }
    let e = crate::cmd::buildcmds::TraceRun::prepare(&s, "imageTrace.make", &json!({"id": live.0})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("Digitize Image"), "the panel's run: {e}");
}
