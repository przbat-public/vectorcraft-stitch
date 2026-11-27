//! Digitizing from an image: the live object (`live: true`, Keep as Live Object), digitized again
//! keeping its id, its objects' edits found, its change angles kept to the art, made plain, and
//! upstream's Image Trace refusing it while its View, Expand and Release still work.

use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use super::live::{edited, live_digitizing, live_settings};
use super::settings::MAX_AREA_CHANGES;
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

/// The live object made of the picture with `params` (and `live: true`), its id and its image's.
fn live_made(params: Value) -> (Session, NodeId, NodeId) {
    live_made_of(&picture(), params)
}

/// The live object made of `raster` with `params` (and `live: true`), its id and its image's.
fn live_made_of(raster: &tr::Raster, params: Value) -> (Session, NodeId, NodeId) {
    let (mut s, image) = placed(raster);
    let mut p = params;
    p["live"] = json!(true);
    let r = s.execute("embroidery.digitizeImage", &p).unwrap();
    assert_eq!(r["created"]["live"], json!(true));
    (s, NodeId(r["created"]["group"].as_u64().unwrap()), image)
}

#[test]
fn keep_as_live_object_makes_an_image_trace_object_holding_the_image_first() {
    // A width given (the picture's own, 30 mm): kept by no live object, its size its image's.
    let (s, live, image) = live_made(json!({"colors": 3, "width": 30.0}));
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

/// An orange square holding a navy one too wide to be sewn over it (the orange made a compound path
/// with a hole), and a navy line 0.6 mm wide (a stroke along its middle), on white.
fn hole_and_line() -> tr::Raster {
    tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [70, 70, 130, 130]) || inside(x, y, [220, 20, 226, 180]) {
            NAVY
        } else if inside(x, y, [20, 20, 180, 180]) {
            ORANGE
        } else {
            WHITE
        }
    })
}

/// The first of live object `live`'s objects (its image aside, groups and compound paths
/// included, in drawing order) that `wanted` takes.
fn object_where(s: &Session, live: NodeId, wanted: impl Fn(&Node) -> bool) -> Option<NodeId> {
    let n = node(s, live);
    let mut stack: Vec<&Node> = n.children()?.iter().skip(1).rev().map(|c| c.as_ref()).collect();
    while let Some(c) = stack.pop() {
        if wanted(c) {
            return Some(c.id);
        }
        stack.extend(c.children().into_iter().flatten().rev().map(|c| c.as_ref()));
    }
    None
}

/// Whether live object `live` reads as edited by hand, as its dry run says.
fn edited_now(s: &mut Session, live: NodeId) -> Value {
    s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap()["edited"].clone()
}

/// A compound path's own look and settings are its object's (Create paints an area with a hole,
/// and sets its stitch, on its compound path): recolouring it, or setting its rows' angle, is an
/// edit. The orange square is made with an angle of its own, so that the hand edit changes a
/// setting it already has.
#[test]
fn a_compound_paths_look_and_settings_are_read_for_hand_edits() {
    let changes = json!([{"at": [50.5, 50.5], "color": hex(ORANGE), "kind": "fill", "angle": 30.0}]);
    for edit in ["recolour", "angle"] {
        let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3, "areaChanges": changes.clone()}));
        let compound = object_where(&s, live, |n| matches!(n.kind, NodeKind::Compound { .. })).expect("the orange square with its hole");
        let angle = node(&s, compound).extras().embroidery.as_deref().and_then(|e| e.fill.as_deref()).and_then(|p| p.angle);
        assert_eq!(angle, Some(30.0), "{edit}: made with its own angle");
        assert_eq!(edited_now(&mut s, live), json!(false), "{edit}: as made");
        if edit == "recolour" {
            s.edit("recolour", |d, _| {
                let o = d.node_mut(compound).ok_or(crate::EngineError::NoNode(compound))?;
                let green = vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(0, 255, 0));
                o.appearance = vectorcraft_doc::Appearance::basic(green, vectorcraft_color::Paint::None, 0.0).into();
                Ok(())
            })
            .unwrap();
        } else {
            s.execute("embroidery.set", &json!({"ids": [compound.0], "part": "fill", "angle": 33})).unwrap();
        }
        assert_eq!(edited_now(&mut s, live), json!(true), "{edit}");
    }
}

/// A path's handles are its shape: one drawn out of its anchor is an edit, though the anchor stays.
#[test]
fn a_handle_drawn_out_is_a_hand_edit() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    let path = object_where(&s, live, |n| n.path_data().is_some_and(|p| !p.subpaths.is_empty())).expect("a path");
    s.edit("handle", |d, _| {
        let data = d.node_mut(path).and_then(Node::path_data_mut).ok_or(crate::EngineError::NoNode(path))?;
        if let Some(a) = data.subpaths.first_mut().and_then(|sp| sp.anchors.first_mut()) {
            a.h_out = a.p + vectorcraft_geom::Vec2::new(2.0, 1.0);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true));
}

/// Scaled with its strokes (Scale Strokes & Effects) and turned, a live object's line keeps its
/// weight in the image's frame, and its set angles turn with it: no edit. Its line's weight set by
/// hand is one.
#[test]
fn a_live_object_scaled_with_its_strokes_edits_nothing() {
    let changes = json!([{"at": [50.5, 50.5], "color": hex(ORANGE), "kind": "fill", "angle": 30.0}]);
    let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3, "areaChanges": changes}));
    let stroked = |n: &Node| n.appearance.items.iter().any(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_)));
    let line = object_where(&s, live, stroked).expect("the line, a stroke");
    transformed(&mut s, live, Affine::rotate(0.3) * Affine::scale(1.5));
    assert_eq!(edited_now(&mut s, live), json!(false), "scaled with its strokes, turned");
    s.edit("weight", |d, _| {
        let n = d.node_mut(line).ok_or(crate::EngineError::NoNode(line))?;
        n.appearance.scale_strokes(2.0);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true), "its weight doubled");
}

/// Scaled with its strokes kept as they were made (Scale Strokes & Effects off, the default), a live
/// object's line keeps its weight as made: no edit either.
#[test]
fn a_live_object_scaled_with_its_strokes_kept_edits_nothing() {
    let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3}));
    let stroked = |n: &Node| n.appearance.items.iter().any(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_)));
    assert!(object_where(&s, live, stroked).is_some(), "the line, a stroke");
    s.edit("scale", |d, _| {
        d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?.transform(Affine::scale(1.5), false);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(false), "scaled, its strokes kept");
}

/// A slanted picture's live object, scaled with its strokes kept as they were made, edits nothing:
/// its scale as made takes the slant between its sides.
#[test]
fn a_slanted_live_object_scaled_with_its_strokes_kept_edits_nothing() {
    let (mut s, image) = placed(&hole_and_line());
    transformed(&mut s, image, Affine::skew(0.25, 0.0));
    let r = s.execute("embroidery.digitizeImage", &json!({"id": image.0, "live": true, "colors": 3})).unwrap();
    let live = NodeId(r["created"]["group"].as_u64().unwrap());
    let stroked = |n: &Node| n.appearance.items.iter().any(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_)));
    assert!(object_where(&s, live, stroked).is_some(), "the line, a stroke");
    assert_eq!(edited_now(&mut s, live), json!(false), "as made");
    s.edit("scale", |d, _| {
        d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?.transform(Affine::scale(1.5), false);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(false), "scaled, its strokes kept");
}

/// A live object turned and then stretched along the page with its strokes kept as they were made
/// edits nothing, though the stretch slants its picture's sides, square when it was made; its line's
/// weight set by hand then is an edit.
#[test]
fn a_turned_live_object_stretched_with_its_strokes_kept_edits_nothing() {
    let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3}));
    let stroked = |n: &Node| n.appearance.items.iter().any(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_)));
    let line = object_where(&s, live, stroked).expect("the line, a stroke");
    s.edit("turn and stretch", |d, _| {
        let n = d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?;
        n.transform(Affine::rotate(std::f64::consts::FRAC_PI_4), false);
        n.transform(Affine::scale_non_uniform(2.0, 1.0), false);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(false), "turned, stretched, its strokes kept");
    s.edit("weight", |d, _| {
        let n = d.node_mut(line).ok_or(crate::EngineError::NoNode(line))?;
        n.appearance.scale_strokes(2.0);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true), "its weight doubled");
}

/// A set angle is read as the direction it gives: a live object made with angles of 270° and 180°
/// (the same rows as −90° and −180°) and turned edits nothing.
#[test]
fn a_live_objects_angles_are_read_as_directions() {
    for angle in [270.0, 180.0, -180.0] {
        let changes = json!([{"at": [50.5, 50.5], "color": hex(ORANGE), "kind": "fill", "angle": angle}]);
        let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3, "areaChanges": changes}));
        transformed(&mut s, live, Affine::rotate(std::f64::consts::FRAC_PI_2));
        assert_eq!(edited_now(&mut s, live), json!(false), "{angle}°, turned a quarter");
    }
}

/// What changes what is sewn of an object, or how it shows, is a hand edit: one of its lines made a
/// guide or a clipping path, an object's opacity set to 0 or its blending to Multiply, a compound
/// path's fill rule set to Even-Odd, a colour block made a clip group.
#[test]
fn a_guide_a_fill_rule_a_clip_an_opacity_or_a_blend_set_is_a_hand_edit() {
    for edit in ["guide", "clipping", "opacity", "blend", "rule", "clip"] {
        let (mut s, live, _) = live_made_of(&hole_and_line(), json!({"colors": 3}));
        let target = match edit {
            "rule" => object_where(&s, live, |n| matches!(n.kind, NodeKind::Compound { .. })),
            "clip" => node(&s, live).children().and_then(|c| c.get(1)).map(|b| b.id),
            _ => object_where(&s, live, |n| matches!(n.kind, NodeKind::Path { .. })),
        }
        .expect("an object to edit");
        s.edit(edit, |d, _| {
            let n = d.node_mut(target).ok_or(crate::EngineError::NoNode(target))?;
            match (edit, &mut n.kind) {
                ("guide", NodeKind::Path { guide, .. }) => *guide = true,
                ("clipping", NodeKind::Path { clipping, .. }) => *clipping = true,
                ("rule", NodeKind::Compound { rule, .. }) => *rule = vectorcraft_geom::FillRule::EvenOdd,
                ("clip", NodeKind::Group { clip, .. }) => *clip = true,
                ("opacity", _) => n.opacity = 0.0,
                ("blend", _) => n.blend = vectorcraft_color::BlendMode::Multiply,
                _ => panic!("{edit}: not that kind of object"),
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(edited_now(&mut s, live), json!(true), "{edit}");
    }
}

/// The image a live object holds stands for it: digitizing it digitizes the live object again (its
/// id kept, no second live object made inside it), and its dry run says whether it was edited.
#[test]
fn a_live_objects_image_stands_for_it() {
    let (mut s, live, image) = live_made(json!({"colors": 3}));
    let dry = s.execute("embroidery.digitizeImage", &json!({"id": image.0, "dryRun": true})).unwrap();
    assert_eq!(dry["edited"], json!(false), "the live object's: {}", dry["edited"]);
    let r = s.execute("embroidery.digitizeImage", &json!({"id": image.0, "live": true, "colors": 2})).unwrap();
    assert_eq!(r["created"]["group"], json!(live.0), "the live object digitized again");
    assert_eq!(named(&s, TRACE_NAME), 1, "one live object");
}

#[test]
fn a_turned_live_object_keeps_each_changes_angle_to_the_art() {
    let changes = json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "angle": 30.0}]);
    let (mut s, live, _) = live_made(json!({"colors": 3, "width": 30.0, "areaChanges": changes}));
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

/// Create on a live object whose place isn't sewn (its layer hidden) makes the plain design on a
/// layer above, and the live object becomes its image there, shown: no second design is left
/// inside it to be sewn once the layer is shown.
#[test]
fn create_on_a_live_object_whose_place_isnt_sewn_leaves_its_image_there() {
    let (mut s, live, image) = live_made(json!({"colors": 3}));
    let layer = s.doc().unwrap().doc.layer_of(live).expect("its layer");
    s.edit("hide", |d, _| {
        d.node_mut(layer).ok_or(crate::EngineError::NoNode(layer))?.visible = false;
        Ok(())
    })
    .unwrap();
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "colors": 3})).unwrap();
    assert_eq!(r["created"]["live"], json!(false));
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    assert_eq!(named(&s, TRACE_NAME), 0, "no live object left");
    let d = &s.doc().unwrap().doc;
    let left = d.node(image).expect("its image");
    assert!(matches!(left.kind, NodeKind::Image(_)) && left.visible && d.layer_of(image) == Some(layer), "its image in its place, shown");
    assert!(d.layer_of(group).is_some_and(|l| l != layer), "the design on another layer");
}

/// Keep as Live Object on an upstream Image Trace object whose place isn't sewn leaves that object
/// whole, its source image first in it and placed as it was (its View, Expand and Release still
/// work), though the design is made at another size; the live object holds a copy of the image,
/// placed as the design is.
#[test]
fn keep_as_live_object_on_a_trace_whose_place_isnt_sewn_leaves_the_trace_whole() {
    let (mut s, image) = placed(&picture());
    let trace = NodeId(s.execute("imageTrace.make", &json!({"id": image.0})).unwrap()["id"].as_u64().unwrap());
    let placed_xf = |s: &Session, id: NodeId| match &node(s, id).kind {
        NodeKind::Image(i) => Some(i.xf),
        _ => None,
    };
    let before = placed_xf(&s, image);
    let layer = s.doc().unwrap().doc.layer_of(trace).expect("its layer");
    s.edit("hide", |d, _| {
        d.node_mut(layer).ok_or(crate::EngineError::NoNode(layer))?.visible = false;
        Ok(())
    })
    .unwrap();
    // Twice the width the picture is placed at (30 mm).
    let r = s.execute("embroidery.digitizeImage", &json!({"id": trace.0, "live": true, "colors": 3, "width": 60.0})).unwrap();
    let live = NodeId(r["created"]["group"].as_u64().unwrap());
    let first = |id: NodeId| node(&s, id).children().and_then(|c| c.first()).map(|c| (c.id, matches!(c.kind, NodeKind::Image(_))));
    assert_eq!(first(trace), Some((image, true)), "the trace object keeps its image first");
    assert_eq!(placed_xf(&s, image), before, "placed as it was");
    let copy = first(live).filter(|(id, is_image)| *is_image && *id != image).map(|(id, _)| id).expect("the live object a copy");
    let size = live_digitizing(&node(&s, live)).and_then(|d| d.size).expect("its size");
    assert!((size[0] - 60.0).abs() < 0.01, "the copy placed as the design is: {size:?}");
    assert!(placed_xf(&s, copy) != before, "the copy scaled with the design");
    s.execute("imageTrace.release", &json!({"ids": [trace.0]})).unwrap();
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
fn image_trace_refuses_a_live_object_in_its_own_words() {
    let refused = crate::cmd::embroidery::LIVE_EMBROIDERY_REFUSED;
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    for cmd in ["imageTrace.make", "imageTrace.makeAndExpand"] {
        let e = s.execute(cmd, &json!({"id": live.0})).unwrap_err().to_string();
        assert_eq!(e, refused, "{cmd}: the refusal as itself, not as invalid parameters");
    }
    let e = crate::cmd::buildcmds::TraceRun::prepare(&s, "imageTrace.make", &json!({"id": live.0})).map(|_| ()).unwrap_err().to_string();
    assert_eq!(e, refused, "the panel's run");
    // Its image, which it holds first: the same.
    let image = node(&s, live).children().and_then(|c| c.first()).map(|c| c.id).expect("its image");
    let e = s.execute("imageTrace.make", &json!({"id": image.0})).unwrap_err().to_string();
    assert_eq!(e, refused, "its image");
}

/// The Check tab warns of a live object scaled more than ±10 % since it was digitized, by its
/// image's transform now against the one rebuilt from the size kept, along the direction it changed
/// most; turning it, or scaling it less, finds nothing, and digitizing it again at its new size
/// settles it (the project owner's choice, 2026-11-16).
#[test]
fn the_check_warns_of_a_live_object_scaled_since_it_was_digitized() {
    let check = |s: &mut Session| {
        let v = s.execute("embroidery.check", &json!({"rules": ["liveObjectScaled"]})).unwrap();
        v["rules"].as_array().and_then(|r| r.first()).cloned().expect("the rule")
    };
    let measured = |rule: &Value| rule["measured"].as_f64().expect("measured");
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    transformed(&mut s, live, Affine::rotate(0.5));
    let rule = check(&mut s);
    assert!(rule["hits"] == json!(0) && measured(&rule) < 0.01, "turned only: {rule}");
    transformed(&mut s, live, Affine::rotate(-0.5) * Affine::scale(1.05));
    let rule = check(&mut s);
    assert!(rule["hits"] == json!(0) && (measured(&rule) - 5.0).abs() < 0.01, "within 10 %: {rule}");
    transformed(&mut s, live, Affine::scale_non_uniform(1.0, 1.2));
    let rule = check(&mut s);
    assert_eq!((rule["hits"].as_u64(), rule["severity"].as_str()), (Some(1), Some("warning")), "{rule}");
    assert_eq!(rule["objects"], json!([live.0]));
    let hit = &rule["examples"][0];
    assert_eq!((hit["objects"].clone(), hit["note"].as_str()), (json!([live.0]), Some("larger")), "{hit}");
    assert!((hit["value"].as_f64().unwrap() - 26.0).abs() < 0.01, "1.05 × 1.2 along its height: {hit}");
    // Digitized again at its size now: nothing to warn of, and still judged.
    s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colors": 3})).unwrap();
    let rule = check(&mut s);
    assert!(rule["hits"] == json!(0) && measured(&rule) < 0.01, "settled: {rule}");
    // Scaled by exactly 10 %: it passes, as stitches scaled by exactly 10 % do.
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    transformed(&mut s, live, Affine::scale(1.1));
    let rule = check(&mut s);
    assert!(rule["hits"] == json!(0) && (measured(&rule) - 10.0).abs() < 0.01, "exactly 10 %: {rule}");
    // Turned 45° and stretched 18 % along the page: 18 % along the direction it changed most, though
    // each of its image's sides grew by less than 10 %.
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    transformed(&mut s, live, Affine::rotate(std::f64::consts::FRAC_PI_4));
    transformed(&mut s, live, Affine::scale_non_uniform(1.18, 1.0));
    let hit = check(&mut s)["examples"][0].clone();
    assert!(hit["note"] == json!("larger") && (hit["value"].as_f64().unwrap() - 18.0).abs() < 0.01, "a stretch across its sides: {hit}");
    // Every object of it given two fills, sewn as pieces expanded from each: judged all the same.
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    s.edit("two fills", |d, _| {
        let objects: Vec<NodeId> = d
            .node(live)
            .and_then(|n| n.children())
            .map(|c| c.iter().skip(1).flat_map(|b| b.children().into_iter().flatten().map(|o| o.id)).collect())
            .unwrap_or_default();
        for id in objects {
            let n = d.node_mut(id).ok_or(crate::EngineError::NoNode(id))?;
            if let Some(first) = n.appearance.items.first().cloned() {
                n.appearance.items.push(first);
            }
        }
        Ok(())
    })
    .unwrap();
    transformed(&mut s, live, Affine::scale(1.5));
    let rule = check(&mut s);
    assert_eq!(rule["objects"], json!([live.0]), "sewn as expanded pieces: {rule}");
    // Reduced to 80 % along one side: smaller.
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    transformed(&mut s, live, Affine::scale_non_uniform(1.0, 0.8));
    let hit = check(&mut s)["examples"][0].clone();
    assert!(hit["note"] == json!("smaller") && (hit["value"].as_f64().unwrap() - 20.0).abs() < 0.01, "reduced: {hit}");
}

/// Two of a live object's blocks swapped by hand (the sewing order changed), or one of its objects
/// deleted, is a hand edit.
#[test]
fn reordering_or_deleting_a_live_objects_objects_is_a_hand_edit() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    assert_eq!(edited_now(&mut s, live), json!(false));
    s.edit("swap", |d, _| {
        let n = d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?;
        let c = n.children_mut().ok_or(crate::EngineError::NoNode(live))?;
        assert!(c.len() >= 3, "the image and two blocks at least: {}", c.len());
        c.swap(1, 2);
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true), "two blocks swapped");
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    let object = node(&s, live).children().and_then(|c| c.get(1)).and_then(|b| b.children().and_then(|c| c.first()).map(|o| o.id)).unwrap();
    s.edit("delete", |d, _| {
        d.remove(object)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true), "an object deleted");
}

/// The command on a live object, given one setting, keeps the others it was made with, as the
/// window does when it opens on it (the project owner's choice, 2026-11-17).
#[test]
fn the_command_on_a_live_object_keeps_the_settings_not_given() {
    let (mut s, live, _) = live_made(json!({"colors": 3, "colorOutlines": true}));
    let kept = |s: &Session| live_settings(&node(s, live)).and_then(|k| k.get("colorOutlines").cloned());
    assert_eq!(kept(&s), Some(json!(true)), "made with outlines");
    s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colors": 3})).unwrap();
    assert_eq!(kept(&s), Some(json!(true)), "digitized again by the command, the outlines not given: kept");
}

/// A setting the request gives as `null` is the default, not the one the live object keeps; the
/// others it keeps stay.
#[test]
fn a_setting_given_as_null_is_the_default_not_the_kept_one() {
    let (mut s, live, _) = live_made(json!({"colors": 3, "colorOutlines": true, "smoothing": 30}));
    s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colorOutlines": null})).unwrap();
    let kept = live_settings(&node(&s, live)).expect("kept settings");
    let read = |k: &str| kept.get(k).and_then(Value::as_f64);
    assert_eq!(kept.get("colorOutlines"), Some(&json!(false)), "the default: {kept:?}");
    assert_eq!((read("smoothing"), read("colors")), (Some(30.0), Some(3.0)), "the others kept: {kept:?}");
}

/// A change the live object keeps survives a run of the command given only the number of colours.
#[test]
fn a_kept_area_change_survives_a_run_given_only_the_colours() {
    let changes = json!([{"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "angle": 45.0}]);
    let (mut s, live, _) = live_made(json!({"colors": 3, "areaChanges": changes}));
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "colors": 3})).unwrap();
    assert_eq!(r["areaChanges"][0]["applies"], json!(true), "{}", r["areaChanges"]);
    let kept = live_settings(&node(&s, live)).and_then(|k| k.get("areaChanges").cloned()).expect("kept changes");
    assert_eq!(kept.as_array().map(Vec::len), Some(1), "{kept}");
}

/// A dry run on a live object, or on the image it holds, echoes the settings it keeps, so that an
/// agent can read them, and warns of nothing about them.
#[test]
fn a_dry_run_on_a_live_object_echoes_the_settings_it_keeps() {
    let (mut s, live, image) = live_made(json!({"colors": 3, "colorOutlines": true}));
    for id in [live, image] {
        let r = s.execute("embroidery.digitizeImage", &json!({"id": id.0, "dryRun": true})).unwrap();
        assert_eq!((r["settings"]["colors"].clone(), r["settings"]["colorOutlines"].clone()), (json!(3), json!(true)), "{}", r["settings"]);
        let warned = r["warnings"].as_array().into_iter().flatten().filter_map(|w| w["id"].as_str()).any(|w| w.starts_with("kept"));
        assert!(!warned, "{}", r["warnings"]);
    }
}

/// A live object made in another thread palette than the document's now: the command lets its
/// threads go (a colour's, the outlines'), holds its number of colours to the palette's threads,
/// keeps the rest of its settings, and says so.
#[test]
fn kept_threads_of_another_palette_are_let_go_and_said() {
    let choices = json!([{"color": hex(ORANGE), "thread": {"palette": "jef", "index": 1}}]);
    let changes = json!([{"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "angle": 45.0}]);
    let made = json!({"colors": 70, "colorChoices": choices, "colorOutlines": true, "outlineThread": {"palette": "jef", "index": 2}, "areaChanges": changes});
    let (mut s, live, _) = live_made(made);
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap();
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w["id"] == "keptThreadsLetGo"), "{}", r["warnings"]);
    let pec = crate::cmd::embroidery::thread_palettes().into_iter().find(|(p, _)| *p == "pec").map(|(_, n)| n as u64);
    let settings = &r["settings"];
    assert_eq!((settings["outlineThread"].clone(), settings["colors"].as_u64()), (Value::Null, pec), "{settings}");
    assert!(settings["colorChoices"].as_array().is_none_or(|c| c.iter().all(|c| c["thread"].is_null())), "{}", settings["colorChoices"]);
    assert_eq!(
        (settings["colorOutlines"].clone(), settings["areaChanges"].as_array().map(Vec::len)),
        (json!(true), Some(1)),
        "the rest kept: {settings}"
    );
}

/// A live object whose kept settings a file holds as junk (changes of no shape, a colour count
/// that is a word) digitizes again by the command with the settings given and the defaults, says
/// why, and never panics.
#[test]
fn a_live_object_with_junk_kept_settings_is_said_and_never_panics() {
    // Changes of no shape read one by one, and a thousand and one of them refused by their count.
    for count in [3, MAX_AREA_CHANGES + 1] {
        let (mut s, live, _) = live_made(json!({"colors": 3}));
        let junk: Vec<Value> = (0..count).map(|i| json!({"at": [i, i], "color": "#zzzzzz", "kind": "both"})).collect();
        keeping(&mut s, live, |m| {
            m.insert("area_changes".into(), Value::Array(junk.clone()));
            m.insert("colors".into(), json!("lots"));
        });
        let n = node(&s, live);
        let _ = live_settings(&n);
        let _ = edited(&n);
        let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "live": true, "dryRun": true, "colors": 4})).unwrap();
        let why = r["warnings"].as_array().unwrap().iter().find(|w| w["id"] == "keptSettingsRefused").and_then(|w| w["values"]["why"].as_str());
        assert!(why.is_some_and(|w| w.contains("areaChanges") && !w.contains("invalid parameters")), "{count}: {}", r["warnings"]);
        assert_eq!(r["settings"]["colors"], json!(4), "the request's");
        assert!(r["settings"]["areaChanges"].as_array().is_none_or(Vec::is_empty), "the default: none");
    }
}

/// The settings live object `live` keeps, changed by `change` (as a file edited by hand, or one of
/// a newer version, would hold them).
fn keeping(s: &mut Session, live: NodeId, change: impl Fn(&mut serde_json::Map<String, Value>)) {
    s.edit("kept", |d, _| {
        let n = d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?;
        let mut extras = n.extras_mut();
        let dz = extras.embroidery.as_mut().and_then(|e| e.digitizing.as_mut()).ok_or(crate::EngineError::NoNode(live))?;
        let mut m = (*dz.settings).clone();
        change(&mut m);
        dz.settings = m.into();
        Ok(())
    })
    .unwrap();
}

/// Create without `live` on a live object makes its plain objects from the settings it keeps too.
#[test]
fn create_without_live_on_a_live_object_starts_from_its_kept_settings() {
    let changes = json!([{"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "angle": 45.0}]);
    let (mut s, live, _) = live_made(json!({"colors": 3, "colorOutlines": true, "areaChanges": changes}));
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "colors": 3})).unwrap();
    assert_eq!(r["created"]["live"], json!(false));
    assert_eq!((r["settings"]["colorOutlines"].clone(), r["areaChanges"][0]["applies"].clone()), (json!(true), json!(true)), "{}", r["settings"]);
}

/// A crop the live object keeps that lies outside its image (relinked since to a smaller picture)
/// can't be used: said, the settings given and the defaults used; given `crop: null`, the rest of
/// what it keeps is used.
#[test]
fn a_kept_crop_outside_its_image_is_said_and_null_takes_the_rest() {
    let (mut s, live, _) = live_made(json!({"colors": 3, "colorOutlines": true}));
    keeping(&mut s, live, |m| {
        m.insert("crop".into(), json!([0, 0, 999, 999]));
    });
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap();
    let why = r["warnings"].as_array().unwrap().iter().find(|w| w["id"] == "keptSettingsRefused").and_then(|w| w["values"]["why"].as_str());
    assert!(why.is_some_and(|w| w.contains("`crop`")), "{}", r["warnings"]);
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true, "crop": null})).unwrap();
    assert!(r["warnings"].as_array().unwrap().iter().all(|w| w["id"] != "keptSettingsRefused"), "{}", r["warnings"]);
    assert_eq!(r["settings"]["colorOutlines"], json!(true), "the rest kept");
}

/// Settings a live object keeps that this version has no setting for (a newer version's) are left
/// out, and said.
#[test]
fn kept_settings_this_version_doesnt_know_are_said() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    keeping(&mut s, live, |m| {
        m.insert("fill_pattern".into(), json!("waves"));
    });
    let r = s.execute("embroidery.digitizeImage", &json!({"id": live.0, "dryRun": true})).unwrap();
    let said = r["warnings"].as_array().unwrap().iter().find(|w| w["id"] == "keptSettingsUnknown").map(|w| w["values"]["keys"].clone());
    assert_eq!(said, Some(json!(["fill_pattern"])), "{}", r["warnings"]);
    assert_eq!(r["settings"]["colors"], json!(3), "the rest kept");
}

/// A live object keeping no fingerprint (a file of an older version, or one edited by hand) is
/// taken as edited, the safe side, so that the window asks before replacing its objects.
#[test]
fn a_live_object_keeping_no_fingerprint_reads_as_edited() {
    let (mut s, live, _) = live_made(json!({"colors": 3}));
    assert_eq!(edited_now(&mut s, live), json!(false));
    s.edit("no fingerprint", |d, _| {
        let n = d.node_mut(live).ok_or(crate::EngineError::NoNode(live))?;
        let mut extras = n.extras_mut();
        let dz = extras.embroidery.as_mut().and_then(|e| e.digitizing.as_mut()).ok_or(crate::EngineError::NoNode(live))?;
        dz.fingerprint = None;
        Ok(())
    })
    .unwrap();
    assert_eq!(edited_now(&mut s, live), json!(true));
}
