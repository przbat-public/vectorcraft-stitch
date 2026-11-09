//! Digitizing from an image: Create, the embroidery objects made in place of the image, and the
//! job the command runs as.

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::Paint;
use vectorcraft_doc::appearance::AppearanceItem;
use vectorcraft_doc::{Document, ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point};
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const GREEN: [u8; 4] = [30, 150, 60, 255];

/// A tenth of a millimetre a pixel.
const MM_PER_PX: f64 = 0.1;

/// How near (millimetres) a box must lie to where it is asked: well under a working pixel.
const NEAR_MM: f64 = 0.01;

/// A session holding `raster` placed at a tenth of a millimetre a pixel, its corner at `at`
/// (millimetres), selected, and the image's id.
fn placed_at(raster: &tr::Raster, at: (f64, f64)) -> (Session, NodeId) {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    let mut id = NodeId(0);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::translate((at.0 / MM_PER_PT, at.1 / MM_PER_PT)) * Affine::scale(MM_PER_PX / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    (s, id)
}

fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

/// An orange square with a navy dot sewn over it, and a green square against it, on white.
fn picture() -> tr::Raster {
    tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [70, 90, 90, 110]) {
            NAVY
        } else if inside(x, y, [20, 20, 140, 180]) {
            ORANGE
        } else if inside(x, y, [140, 60, 230, 140]) {
            GREEN
        } else {
            WHITE
        }
    })
}

fn doc(s: &Session) -> Arc<Document> {
    s.doc().unwrap().doc.clone()
}

/// The colour a node's fill is painted, `#rrggbb`.
fn fill_of(n: &Node) -> Option<String> {
    n.appearance.items.iter().find_map(|i| match i {
        AppearanceItem::Fill(f) => match &f.paint {
            Paint::Solid { color, .. } => Some(color.to_hex()),
            _ => None,
        },
        _ => None,
    })
}

/// The reply without what changes from run to run (the timings).
fn steady(mut r: Value) -> Value {
    r.as_object_mut().unwrap().remove("timings");
    r
}

/// Create puts one group where the image was, a group a colour block in sewing order named after
/// its thread, each area one object painted with its thread's own colour; the image moves, with
/// its id, to a hidden, locked template layer below every layer; one undo step puts it all back.
#[test]
fn create_makes_the_objects_in_place_of_the_image() {
    let (mut s, image) = placed_at(&picture(), (0.0, 0.0));
    let layer = doc(&s).layer_of(image).unwrap();
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 3})).unwrap();
    assert_eq!(r["dryRun"], json!(false));
    let created = &r["created"];
    let group = NodeId(created["group"].as_u64().unwrap());
    let d = doc(&s);
    assert_eq!(s.doc().unwrap().selection.objects, vec![group], "the new group selected");
    assert_eq!(d.position(group).map(|(p, i, _)| (p, i)), Some((Some(layer), 0)), "in the image's place");
    // The template layer, below every layer, holding the image with its id.
    let template = d.layers.first().unwrap();
    assert_eq!(json!(template.id.0), created["layer"]);
    assert_eq!(template.name.as_deref(), Some("Template Image"));
    assert!(template.locked && !template.visible && matches!(template.kind, NodeKind::Layer { template: true, .. }));
    assert_eq!(template.children().unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), vec![image]);
    // A block a thread, in sewing order (bottom first), each object painted with its thread.
    let blocks = d.node(group).unwrap().children().unwrap().clone();
    assert_eq!(json!(blocks.len()), created["blocks"]);
    let areas = r["areas"].as_array().unwrap();
    let mut listed = areas.iter();
    for block in &blocks {
        let objects = block.children().unwrap();
        for object in objects {
            let area = listed.next().unwrap();
            let thread = &area["thread"];
            let name =
                format!("{} {} {}", thread["palette"].as_str().unwrap().to_ascii_uppercase(), thread["index"], thread["name"].as_str().unwrap());
            assert_eq!(block.name.as_deref(), Some(name.as_str()), "{area}");
            assert_eq!(fill_of(object), thread["color"].as_str().map(str::to_string), "{area}");
            assert!(object.extras().embroidery.is_none(), "Auto: nothing set");
        }
    }
    assert!(listed.next().is_none());
    assert_eq!(json!(areas.len()), created["objects"]);
    // One undo step.
    s.execute("edit.undo", &json!({})).unwrap();
    let d = doc(&s);
    assert!(d.node(group).is_none());
    assert_eq!(d.layer_of(image), Some(layer), "the image back where it was");
    assert!(d.layers.iter().all(|l| l.name.as_deref() != Some("Template Image")));
}

/// A dry run changes nothing and answers the figures Create's objects have: its `stats` are
/// `embroidery.stats {selectedOnly: true}` after Create, the new group selected, and each block is
/// sewn in exactly its thread. The same picture gives the same answer.
#[test]
fn a_dry_run_answers_what_create_makes() {
    let (mut s, _) = placed_at(&picture(), (5.0, 5.0));
    let before = doc(&s);
    let dry = s.execute("embroidery.digitizeImage", &json!({"colors": 3, "dryRun": true})).unwrap();
    assert!(Arc::ptr_eq(&before, &doc(&s)), "a dry run changes nothing");
    assert_eq!((&dry["dryRun"], &dry["created"]), (&json!(true), &Value::Null));
    let again = s.execute("embroidery.digitizeImage", &json!({"colors": 3, "dryRun": true})).unwrap();
    assert_eq!(steady(dry.clone()), steady(again), "the same answer");
    let made = s.execute("embroidery.digitizeImage", &json!({"colors": 3})).unwrap();
    let stats = s.execute("embroidery.stats", &json!({"selectedOnly": true})).unwrap();
    assert_eq!(dry["stats"], stats);
    assert_eq!(made["stats"], stats);
    assert!(stats["stitches"].as_u64().unwrap() > 0);
    assert_eq!(stats["blockCount"], made["created"]["blocks"]);
    for block in stats["blocks"].as_array().unwrap() {
        assert_eq!(block["thread"]["deltaE"], json!(0.0), "{block}");
    }
    // Create's reply is the dry run's, with what it made.
    let mut made = steady(made);
    made["created"] = Value::Null;
    made["dryRun"] = json!(true);
    assert_eq!(made, steady(dry));
}

/// A dry run's figures are what Create's objects sew, checked apart from the command: beside other
/// sewn art (a rectangle), Create made, the document saved and opened again, and the new group alone
/// measured by `embroidery.stats`; the whole document sews more.
#[test]
fn a_dry_run_answers_what_create_makes_beside_other_art() {
    let (mut s, image) = placed_at(&picture(), (5.0, 5.0));
    s.execute("shape.rectangle", &json!({"x": 200, "y": 200, "width": 60, "height": 40})).unwrap();
    let p = json!({"id": image.0, "colors": 3});
    let mut dry_p = p.clone();
    dry_p["dryRun"] = json!(true);
    let dry = s.execute("embroidery.digitizeImage", &dry_p).unwrap();
    let made = s.execute("embroidery.digitizeImage", &p).unwrap();
    let group = made["created"]["group"].as_u64().unwrap();
    let saved = vectorcraft_format::save(&doc(&s), false);
    let mut t = Session::new();
    t.execute("document.open", &json!({"name": "saved.vectorcraft", "dataBase64": vectorcraft_format::base64_encode(&saved)})).unwrap();
    t.execute("select.set", &json!({"ids": [group]})).unwrap();
    let stats = t.execute("embroidery.stats", &json!({"selectedOnly": true})).unwrap();
    assert_eq!(dry["stats"], stats);
    let all = t.execute("embroidery.stats", &json!({"selectedOnly": false})).unwrap();
    let stitches = |r: &Value| r["stitches"].as_u64().unwrap();
    assert!(stitches(&all) > stitches(&stats), "the rectangle sews too: {} against {}", all["stitches"], stats["stitches"]);
}

/// Create is one undo step, and it redoes: undone, the document is the one it was; redone, the one
/// Create left.
#[test]
fn create_undoes_and_redoes() {
    let (mut s, _) = placed_at(&picture(), (5.0, 5.0));
    let before = vectorcraft_format::save(&doc(&s), false);
    s.execute("embroidery.digitizeImage", &json!({"colors": 3})).unwrap();
    let made = vectorcraft_format::save(&doc(&s), false);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(vectorcraft_format::save(&doc(&s), false) == before, "undone");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(vectorcraft_format::save(&doc(&s), false) == made, "redone");
}

/// Create's undo step is named as the command is: the two names, one a literal in `cmd!`, agree.
#[test]
fn the_undo_step_is_named_as_the_command() {
    let spec = crate::cmd::command_specs().iter().find(|c| c.id == "embroidery.digitizeImage").unwrap();
    assert_eq!(spec.label, super::command::LABEL);
}

/// A colour's job sets only what it changes: Fill sets its fill's narrow parts not to be satin, Don't
/// sew sets its fill not to be sewn (its objects made, under the rest, and left out of the figures),
/// Auto sets nothing.
#[test]
fn a_colours_job_sets_only_what_it_changes() {
    use super::super::resolve::Stitch;
    assert_eq!(Stitch::from_id(super::create::NOT_SEWN), Some(Stitch::None));
    let (mut s, _) = placed_at(&picture(), (0.0, 0.0));
    let hex = |c: [u8; 4]| super::super::hex([c[0], c[1], c[2]]);
    let choices = json!([{"color": hex(ORANGE), "job": "fill"}, {"color": hex(GREEN), "job": "none"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 3, "colorChoices": choices})).unwrap();
    let d = doc(&s);
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let blocks = d.node(group).unwrap().children().unwrap().clone();
    let colour_of = |c: [u8; 4]| r["colors"].as_array().unwrap().iter().find(|x| x["color"] == json!(hex(c))).unwrap()["thread"]["color"].clone();
    let (orange, navy, green) = (colour_of(ORANGE), colour_of(NAVY), colour_of(GREEN));
    let first = blocks.first().unwrap().children().unwrap();
    assert!(first.iter().all(|o| json!(fill_of(o)) == green), "the unsewn first, under everything");
    for object in blocks.iter().flat_map(|b| b.children().unwrap().iter()) {
        let fill = json!(fill_of(object));
        let part = object.extras().embroidery.as_ref().and_then(|e| e.fill.as_deref());
        if fill == orange {
            assert_eq!(part.map(|p| (p.narrow_as_satin, p.stitch.is_none())), Some((Some(0.0), true)));
        } else if fill == green {
            assert_eq!(part.map(|p| (p.stitch.as_deref(), p.narrow_as_satin)), Some((Some("none"), None)));
        } else {
            assert_eq!(fill, navy);
            assert!(object.extras().embroidery.is_none());
        }
    }
    assert_eq!(r["stats"]["blockCount"], json!(2), "the green isn't sewn: {}", r["stats"]["blocks"]);
    assert_eq!(r["created"]["blocks"], json!(3));
}

/// Made smaller and from a crop, the design keeps the crop's centre, and the image moved under it
/// is scaled with it: the crop's corners land on the design's corners.
#[test]
fn the_image_lies_under_the_design_it_made() {
    let (mut s, image) = placed_at(&picture(), (10.0, 20.0));
    // The crop: 200 × 160 pixels, 20 × 16 mm as placed, made 10 mm wide.
    let crop = [10, 20, 210, 180];
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 3, "crop": crop, "width": 10})).unwrap();
    let bounds: Vec<f64> = r["image"]["bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    // The crop's centre, 21 by 30 mm, is kept.
    assert!(((bounds[0] + bounds[2]) / 2.0 - 21.0).abs() < NEAR_MM && ((bounds[1] + bounds[3]) / 2.0 - 30.0).abs() < NEAR_MM, "{bounds:?}");
    assert!((bounds[2] - bounds[0] - 10.0).abs() < NEAR_MM && (bounds[3] - bounds[1] - 8.0).abs() < NEAR_MM, "{bounds:?}");
    let d = doc(&s);
    let Some(NodeKind::Image(img)) = d.node(image).map(|n| &n.kind) else { panic!("the image") };
    let corner = |x: u32, y: u32| img.xf * Point::new(f64::from(x), f64::from(y));
    let (a, b) = (corner(crop[0], crop[1]), corner(crop[2], crop[3]));
    for (got, want) in [a.x, a.y, b.x, b.y].into_iter().zip(&bounds) {
        assert!((got * MM_PER_PT - want).abs() < NEAR_MM, "{a:?} {b:?} {bounds:?}");
    }
}

/// An Image Trace object is digitized from its source image, and replaced whole: the image, its
/// first child, moves to the template layer, shown there.
#[test]
fn an_image_trace_object_is_replaced() {
    let (mut s, image) = placed_at(&picture(), (0.0, 0.0));
    let traced = s.execute("imageTrace.make", &json!({})).unwrap();
    let object = NodeId(traced["id"].as_u64().unwrap());
    assert!(doc(&s).node(object).unwrap().children().unwrap().iter().any(|c| c.id == image));
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 3})).unwrap();
    assert_eq!(r["image"]["id"], json!(object.0));
    let d = doc(&s);
    assert!(d.node(object).is_none(), "the Image Trace object replaced");
    let moved = d.node(image).unwrap();
    assert!(moved.visible, "shown in its layer, which is hidden");
    assert_eq!(d.layer_of(image).map(|l| json!(l.0)), Some(r["created"]["layer"].clone()));
}

/// The job: prepared on the document, run, and applied to that document only. A document changed
/// meanwhile refuses it, and nothing is made; an image that is all background makes nothing, which
/// a dry run says and Create refuses.
#[test]
fn the_job_applies_to_the_document_it_was_made_of() {
    let (mut s, image) = placed_at(&picture(), (0.0, 0.0));
    let job = s.digitize_image_job(&json!({"colors": 3})).unwrap();
    let found = job.run(&|_, _| true).unwrap();
    let (preview, group) = found.document().unwrap();
    assert!(preview.node(group).is_some() && preview.node(image).is_some());
    assert!(s.digitize_image_current(&found));
    s.execute("shape.rectangle", &json!({"x": 200, "y": 200, "width": 20, "height": 20})).unwrap();
    assert!(!s.digitize_image_current(&found));
    let before = doc(&s);
    let e = s.apply_digitize_image(found).unwrap_err().to_string();
    assert!(e.contains("changed"), "{e}");
    assert!(Arc::ptr_eq(&before, &doc(&s)), "nothing made");
    // All background: nothing to make.
    let (mut s, _) = placed_at(&tr::Raster::from_fn(100, 100, |_, _| WHITE), (0.0, 0.0));
    let r = s.execute("embroidery.digitizeImage", &json!({"dryRun": true})).unwrap();
    assert_eq!(r["stats"], Value::Null);
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w["id"] == "nothingToMake" && w["values"] == json!({})), "{}", r["warnings"]);
    let e = s.execute("embroidery.digitizeImage", &json!({})).unwrap_err().to_string();
    assert!(e.contains("nothing to sew"), "{e}");
}

/// Off the interface thread: the job, and what it made, go to another thread; there it reports its
/// five stages to the sink of the scope it runs in, from the first to the last, and a cancel stops
/// it, nothing made: a scope cancelled before it starts, a cancel in the middle of it (from the
/// sink, when its third stage begins), and a poll answering `false` as the trace asks it from the
/// threads that trace.
#[test]
fn the_job_runs_on_another_thread_and_stops_when_cancelled() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use vectorcraft_doc::cancel::{self, CancelToken};
    fn sendable<T: Send>(_: &T) {}
    let (s, _) = placed_at(&picture(), (0.0, 0.0));
    let job = s.digitize_image_job(&json!({"colors": 3})).unwrap();
    sendable(&job);
    let (stages, found, job) = std::thread::spawn(move || {
        let seen: Rc<RefCell<Vec<(u64, u64)>>> = Rc::default();
        let into = seen.clone();
        let sink: cancel::ProgressSink = Rc::new(move |done, total, what| {
            if what == super::DIGITIZE_PROGRESS {
                into.borrow_mut().push((done, total));
            }
        });
        let found = cancel::scope(&CancelToken::new(), Some(sink), || job.run(&|_, _| true));
        let stages = seen.borrow().clone();
        (stages, found, job)
    })
    .join()
    .unwrap();
    let found = found.unwrap();
    sendable(&found);
    assert!(stages.iter().all(|&(_, total)| total == super::DIGITIZE_STAGES), "{stages:?}");
    let done: Vec<u64> = stages.iter().map(|&(d, _)| d).collect();
    assert_eq!(done, (0..=super::DIGITIZE_STAGES).collect::<Vec<_>>(), "each stage once, in order");
    // Cancelled before it starts, in its middle, and through the trace's poll.
    let cancelled = CancelToken::new();
    cancelled.cancel();
    assert!(matches!(cancel::scope(&cancelled, None, || job.run(&|_, _| true)), Err(crate::EngineError::Cancelled)));
    let token = CancelToken::new();
    let midway = token.clone();
    let sink: cancel::ProgressSink = Rc::new(move |done, _, what| {
        if what == super::DIGITIZE_PROGRESS && done == 2 {
            midway.cancel();
        }
    });
    assert!(matches!(cancel::scope(&token, Some(sink), || job.run(&|_, _| true)), Err(crate::EngineError::Cancelled)));
    assert!(matches!(job.run(&|_, _| false), Err(crate::EngineError::Cancelled)));
}

/// The reply, pinned by shape as the read commands' are (`tests_contract`): every key and the type
/// of its value, so that a change is a deliberate one. Its `stats` are `embroidery.stats`' own,
/// pinned there.
#[test]
fn the_reply_keeps_its_shape() {
    let (mut s, _) = placed_at(&picture(), (0.0, 0.0));
    let hex = |c: [u8; 4]| super::super::hex([c[0], c[1], c[2]]);
    let choices = json!([{"color": hex(GREEN), "job": "fill"}, {"color": "#ff00ff"}]);
    let params = json!({"colors": 3, "colorChoices": choices, "regions": true, "outlines": true, "at": [80, 100]});
    let mut r = s.execute("embroidery.digitizeImage", &params).unwrap();
    let stats = r.as_object_mut().unwrap().remove("stats").unwrap();
    assert_eq!(stats, s.execute("embroidery.stats", &json!({"selectedOnly": true})).unwrap());
    let pinned = "{areaAt: int, areaCount: int, areas: [{area: int, at: [float], box: [float], color: string, grownUnder: [], inside: int, joined: int, kind: string, line: null, order: {after: [{area: int, because: string}], taken: string}, outline: string, sewn: bool, sewnOver: bool, size: {area: float, height: float, width: float}, thread: {color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, touches: [{area: int, edge: float, under: bool}], widest: float} | {area: int, at: [float], box: [float], color: string, grownUnder: [], inside: null, joined: int, kind: string, line: null, order: {after: [{area: int, because: string}], taken: string}, outline: string, sewn: bool, sewnOver: bool, size: {area: float, height: float, width: float}, thread: {color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, touches: [{area: int, edge: float, under: bool}], widest: float} | {area: int, at: [float], box: [float], color: string, grownUnder: [int], inside: null, joined: int, kind: string, line: null, order: {after: [], taken: string}, outline: string, sewn: bool, sewnOver: bool, size: {area: float, height: float, width: float}, thread: {color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, touches: [{area: int, edge: float, under: bool}], widest: float}], colorCount: int, colors: [{alternatives: [], area: float, areaCount: int, background: bool, by: null, choice: null, color: string, job: string, mergedInto: null, reason: null, sewn: bool, sewnShare: null, share: float, thread: null} | {alternatives: [{color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}], area: float, areaCount: int, background: bool, by: null, choice: int, color: string, job: string, mergedInto: null, reason: string, sewn: bool, sewnShare: float, share: float, thread: {color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}} | {alternatives: [{color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}], area: float, areaCount: int, background: bool, by: null, choice: null, color: string, job: string, mergedInto: null, reason: string, sewn: bool, sewnShare: float, share: float, thread: {color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}}], created: {blocks: int, group: int, layer: int, objects: int}, doubledArea: float, dryRun: bool, image: {background: {color: string, from: string}, bounds: [float], crop: [int], decoded: [int], id: int, limitedBy: string, pixels: [int], pxPerMm: float, scale: float, size: [float], worked: [int]}, joined: int, leftOut: int, offset: int, regions: {base64: string, height: int, width: int}, settings: {background: string, colorChoices: [{color: string, job: null, mergeInto: null, thread: null} | {color: string, job: string, mergeInto: null, thread: null}], colors: int, corners: float, crop: null, fit: null, height: null, minArea: float, mode: string, overlap: float, smoothing: float, width: null}, table: [{default: [], key: string, name: string, range: [int], source: string, step: string, unit: string} | {default: float, key: string, name: string, range: [float], source: string, step: string, unit: string} | {default: int, key: string, name: string, range: [int], source: string, step: string, unit: string} | {default: null, key: string, name: string, range: [float], source: string, step: string, unit: string} | {default: null, key: string, name: string, range: null, source: string, step: string, unit: string} | {default: string, key: string, name: string, range: [string], source: string, step: string, unit: string}], timings: {areas: float, image: float, objects: float, shapes: float, stitches: float, trace: float}, traced: {colors: int, paths: int}, truncated: bool, unusedChoices: [{choice: int, reason: string}], warnings: []}";
    assert_eq!(super::super::tests_contract::shape(&r), pinned, "the reply changed shape; if on purpose, pin it");
}
