//! Digitizing's stages kept between runs ([`super::pipeline::Pipeline`]), for the image window: a
//! run answers what the command answers, a change runs again only its stage and those after it,
//! kept or fresh the outputs are the same, a cancel keeps the stages finished, and Create from a run
//! is the command's Create, the window's palette set in the same undo step.

use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_doc::cancel::{self, CancelToken};
use vectorcraft_doc::{Document, ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace as tr;

use super::pipeline::{Pipeline, Place, StageId};
use crate::{EngineError, Session};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];
const GREEN: [u8; 4] = [30, 150, 60, 255];

/// A tenth of a millimetre a pixel: the working copy is the picture, pixel for pixel.
const MM_PER_PX: f64 = 0.1;

/// A session holding `raster` placed at a tenth of a millimetre a pixel at the origin, selected,
/// and the image's id.
fn placed(raster: &tr::Raster) -> (Session, NodeId) {
    placed_at(raster, MM_PER_PX)
}

/// [`placed`], a pixel `mm_per_px` millimetres.
fn placed_at(raster: &tr::Raster, mm_per_px: f64) -> (Session, NodeId) {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    let mut id = NodeId(0);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(mm_per_px / MM_PER_PT);
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

/// Two orange squares apart, a navy one between them, on white.
fn three_squares() -> tr::Raster {
    tr::Raster::from_fn(300, 100, |x, y| {
        if inside(x, y, [20, 20, 80, 80]) || inside(x, y, [220, 20, 280, 80]) {
            ORANGE
        } else if inside(x, y, [120, 20, 180, 80]) {
            NAVY
        } else {
            WHITE
        }
    })
}

fn hex(c: [u8; 4]) -> String {
    super::super::hex([c[0], c[1], c[2]])
}

fn doc(s: &Session) -> Arc<Document> {
    s.doc().unwrap().doc.clone()
}

/// The reply without what changes from run to run (the timings).
fn steady(mut r: Value) -> Value {
    r.as_object_mut().unwrap().remove("timings");
    r
}

/// `params` with `more`'s keys set.
fn with(params: &Value, more: &Value) -> Value {
    let mut p = params.clone();
    for (k, v) in more.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// The stages a run of `pipe` with `params` (and the window's `palette`) made rather than found
/// kept, in order.
fn made(s: &Session, pipe: &mut Pipeline, image: NodeId, params: &Value, palette: Option<&str>) -> Vec<StageId> {
    made_with(s, pipe, image, params, palette, &json!({}))
}

/// [`made`] with the window's document settings `document` (`embroidery.setup`'s).
fn made_with(s: &Session, pipe: &mut Pipeline, image: NodeId, params: &Value, palette: Option<&str>, document: &Value) -> Vec<StageId> {
    let req = Arc::new(s.digitize_window_request(image, params, palette, document).unwrap());
    let mut made = Vec::new();
    pipe.run(&req, &|_, _| true, &mut |stage, _, computed| {
        if computed {
            made.push(stage);
        }
    })
    .unwrap();
    made
}

/// The stages from `first` on.
fn from(first: StageId) -> Vec<StageId> {
    StageId::ALL.iter().copied().skip_while(|&s| s != first).collect()
}

/// A run answers what the command's dry run answers (its areas listed none), in colour and in logo
/// mode, and with outlines: the same colours, threads, warnings, settings, figures and outlines.
#[test]
fn a_pipeline_run_is_the_commands_dry_run() {
    let outlined = json!({"colors": 3, "colorOutlines": true, "border": true});
    for params in [json!({"mode": "color", "colors": 3}), json!({"mode": "logo", "colors": 3}), outlined] {
        let (mut s, image) = placed(&picture());
        let mode = params.clone();
        let want = steady(s.execute("embroidery.digitizeImage", &with(&params, &json!({"dryRun": true, "limit": 0}))).unwrap());
        let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
        let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
        assert_eq!(steady(out.reply(&req).unwrap()), want, "{mode}");
    }
}

/// A change runs again its own stage and the stages after it, and keeps those before it: the size,
/// the box it fits, the crop and the background from the image, the mode, the colours and the
/// smoothing from the trace (the smallest area too, below the trace's own noise of a thread's
/// footprint, 0.16 mm²), a colour's choice and the smallest area above it from the colours, the
/// overlap from the shapes, the window's palette from the colours, an edit to the document from the
/// objects; nothing changed, nothing runs again. The request says
/// so before the run: the first stage it reads differently of the request run before.
#[test]
fn a_change_reruns_its_stage_and_those_after() {
    let (mut s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let base = json!({"colors": 3, "minArea": 1.0, "overlap": 0.4});
    let request = |s: &Session, params: &Value, palette: Option<&str>| s.digitize_window_request(image, params, palette, &json!({})).unwrap();
    assert_eq!(made(&s, &mut pipe, image, &base, None), StageId::ALL.to_vec(), "the first run makes every stage");
    assert_eq!(made(&s, &mut pipe, image, &base, None), Vec::<StageId>::new(), "nothing changed");
    assert_eq!(request(&s, &base, None).first_stage_changed(&request(&s, &base, None)), None, "nothing changed");
    let cases = [
        (json!({"width": 25.0}), StageId::Image),
        (json!({"fit": [25.0, 15.0]}), StageId::Image),
        (json!({"crop": [10, 10, 290, 190]}), StageId::Image),
        (json!({"background": "none"}), StageId::Image),
        (json!({"background": hex(WHITE)}), StageId::Image),
        (json!({"mode": "logo"}), StageId::Trace),
        (json!({"colors": 4}), StageId::Trace),
        (json!({"smoothing": 30}), StageId::Trace),
        (json!({"straightLines": true}), StageId::Trace),
        (json!({"thinLine": 2.0}), StageId::Colours),
        (json!({"minArea": 0.1}), StageId::Trace),
        (json!({"colorChoices": [{"color": hex(ORANGE), "job": "fill"}]}), StageId::Colours),
        (json!({"minArea": 2.0}), StageId::Colours),
        (json!({"overlap": 0.6}), StageId::Shapes),
        (json!({"colorOutlines": true}), StageId::Objects),
        (json!({"border": true}), StageId::Objects),
        (json!({"borderWidth": 5.0}), StageId::Objects),
        (json!({"outlineThread": {"palette": "jef", "index": 2}}), StageId::Objects),
        (json!({"areaChanges": [{"at": [30.0, 30.0], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"}]}), StageId::Objects),
        // Don't Sew may leave a hole in what is round the area, and a stitch set after it may
        // overrule it: from the shapes on, the centre lines kept.
        (json!({"areaChanges": [{"at": [30.0, 30.0], "color": hex(ORANGE), "kind": "fill", "stitch": "none"}]}), StageId::Shapes),
        (
            json!({"areaChanges": [
                {"at": [30.0, 30.0], "color": hex(ORANGE), "kind": "fill", "stitch": "none"},
                {"at": [30.0, 30.0], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"},
            ]}),
            StageId::Shapes,
        ),
    ];
    for (change, first) in cases {
        made(&s, &mut pipe, image, &base, None);
        let changed = with(&base, &change);
        assert_eq!(request(&s, &base, None).first_stage_changed(&request(&s, &changed, None)), Some(first), "{change}: said before");
        assert_eq!(made(&s, &mut pipe, image, &changed, None), from(first), "{change}");
    }
    // From a Don't Sew to a stitch set after it on that area: no longer left out, from the shapes
    // on; and back. A stitch before the first Don't Sew changed: from the objects on.
    let change = |at: [f64; 2], c: [u8; 4], stitch: &str| json!({"at": at, "color": hex(c), "kind": "fill", "stitch": stitch});
    let (p, q) = ([30.0, 30.0], [185.0, 100.0]);
    let with_changes = |list: Value| with(&base, &json!({"areaChanges": list}));
    let transitions = [
        (json!([change(p, ORANGE, "none")]), json!([change(p, ORANGE, "none"), change(p, ORANGE, "tatami")]), StageId::Shapes),
        (json!([change(p, ORANGE, "none"), change(p, ORANGE, "tatami")]), json!([change(p, ORANGE, "none")]), StageId::Shapes),
        (
            json!([change(p, ORANGE, "none"), change(q, GREEN, "none")]),
            json!([change(p, ORANGE, "none"), change(q, GREEN, "tatami")]),
            StageId::Shapes,
        ),
        (
            json!([change(q, GREEN, "tatami"), change(p, ORANGE, "none")]),
            json!([change(q, GREEN, "satin"), change(p, ORANGE, "none")]),
            StageId::Objects,
        ),
    ];
    // Said by the keys (the stages may be found kept from the cases above, each keeping its two
    // newest outputs).
    for (from_changes, to_changes, first) in transitions {
        let (before, after) = (request(&s, &with_changes(from_changes.clone()), None), request(&s, &with_changes(to_changes.clone()), None));
        assert_eq!(before.first_stage_changed(&after), Some(first), "{from_changes} → {to_changes}");
    }
    made(&s, &mut pipe, image, &base, None);
    assert_eq!(request(&s, &base, None).first_stage_changed(&request(&s, &base, Some("pec"))), Some(StageId::Colours));
    assert_eq!(made(&s, &mut pipe, image, &base, Some("pec")), from(StageId::Colours), "the window's palette");
    // The window's document settings from the objects on: the stitches follow them.
    for document in [json!({"fabric": "denim"}), json!({"fillAngle": "acrossShape"}), json!({"joinSameColor": false})] {
        made(&s, &mut pipe, image, &base, None);
        let changed = s.digitize_window_request(image, &base, None, &document).unwrap();
        assert_eq!(request(&s, &base, None).first_stage_changed(&changed), Some(StageId::Objects), "{document}: said before");
        assert_eq!(made_with(&s, &mut pipe, image, &base, None, &document), from(StageId::Objects), "{document}");
    }
    made(&s, &mut pipe, image, &base, None);
    let before = request(&s, &base, None);
    s.execute("shape.rectangle", &json!({"x": 200, "y": 200, "width": 20, "height": 20})).unwrap();
    assert_eq!(before.first_stage_changed(&request(&s, &base, None)), Some(StageId::Objects));
    assert_eq!(made(&s, &mut pipe, image, &base, None), from(StageId::Objects), "an edit to the document");
}

/// Each stage keeps its two newest outputs: a value changed and changed back is found kept, and a
/// third value lets the oldest go, so that what the window keeps stays bounded.
#[test]
fn each_stage_keeps_its_two_newest_outputs() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let wide = |width: f64| json!({"colors": 3, "width": width});
    made(&s, &mut pipe, image, &wide(20.0), None);
    made(&s, &mut pipe, image, &wide(25.0), None);
    assert_eq!(made(&s, &mut pipe, image, &wide(20.0), None), Vec::<StageId>::new(), "changed back: found kept");
    made(&s, &mut pipe, image, &wide(30.0), None);
    assert_eq!(made(&s, &mut pipe, image, &wide(25.0), None), from(StageId::Image), "the oldest let go");
}

/// A run cancelled after its colours keeps the stages it finished: the next run makes only those
/// after them.
#[test]
fn a_cancelled_run_keeps_the_stages_it_finished() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let params = json!({"colors": 3});
    let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let token = CancelToken::new();
    let stop = token.clone();
    let cancelled = cancel::scope(&token, None, || {
        pipe.run(&req, &|_, _| true, &mut |stage, _, _| {
            if stage == StageId::Colours {
                stop.cancel();
            }
        })
    });
    assert!(matches!(cancelled, Err(EngineError::Cancelled)));
    assert_eq!(made(&s, &mut pipe, image, &params, None), from(StageId::Lines));
}

/// A poll answering no, as the trace asks it from the threads that trace, stops the run in its
/// trace: the run is cancelled, and the working copy it made stays kept.
#[test]
fn a_poll_answering_no_stops_the_run_in_its_trace() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let params = json!({"colors": 3});
    let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let stopped = pipe.run(&req, &|_, _| false, &mut |_, _, _| {});
    assert!(matches!(stopped, Err(EngineError::Cancelled)), "{:?}", stopped.err());
    assert_eq!(made(&s, &mut pipe, image, &params, None), from(StageId::Trace));
}

/// Create from a run puts the run's objects in place as the command's Create does, and answers as
/// it answers (its areas listed none): the same document, the same objects, one undo step, and a
/// reply saying what was made, no dry run.
#[test]
fn create_from_a_run_is_the_commands_create() {
    let params = json!({"colors": 3});
    let (mut window, image) = placed(&picture());
    let (mut command, _) = placed(&picture());
    let req = Arc::new(window.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let out = window.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let steps = window.doc().unwrap().history.undo.len();
    let from_window = window.apply_digitize_window(&req, &out, false).unwrap();
    let from_command = command.execute("embroidery.digitizeImage", &with(&params, &json!({"limit": 0}))).unwrap();
    assert_eq!(from_window["dryRun"], json!(false), "made, so no dry run");
    assert_eq!(steady(from_window), steady(from_command));
    assert_eq!(*doc(&window), *doc(&command));
    assert_eq!(window.doc().unwrap().history.undo.len(), steps + 1);
}

/// A run made of an earlier document is refused, nothing applied.
#[test]
fn create_refuses_a_run_made_of_an_earlier_document() {
    let (mut s, image) = placed(&picture());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    s.execute("shape.rectangle", &json!({"x": 200, "y": 200, "width": 20, "height": 20})).unwrap();
    let before = doc(&s);
    let e = s.apply_digitize_window(&req, &out, false).unwrap_err().to_string();
    assert!(e.contains("changed"), "{e}");
    assert!(Arc::ptr_eq(&before, &doc(&s)), "nothing made");
}

/// The palette chosen in the window is matched to and becomes the document's on Create, in the
/// same undo step; Undo takes both back.
#[test]
fn the_windows_palette_is_set_in_the_same_undo_step() {
    let (mut s, image) = placed(&picture());
    assert_eq!(super::super::thread_palette(&doc(&s)), super::super::DEFAULT_PALETTE);
    assert_ne!(super::super::DEFAULT_PALETTE, Palette::Pec, "the test picks the other palette");
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), Some("pec"), &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let reply = out.reply(&req).unwrap();
    assert!(reply["colors"].as_array().unwrap().iter().filter(|c| !c["thread"].is_null()).all(|c| c["thread"]["palette"] == "pec"), "{reply}");
    let steps = s.doc().unwrap().history.undo.len();
    s.apply_digitize_window(&req, &out, false).unwrap();
    assert_eq!(super::super::thread_palette(&doc(&s)), Palette::Pec);
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "one undo step");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(super::super::thread_palette(&doc(&s)), super::super::DEFAULT_PALETTE);
}

/// The document settings the window changes (its fabric, fill angle and joining one thread's
/// objects, as `embroidery.setup` takes them) are what its stitches are planned with, and Create
/// makes them the document's in the same undo step: the window's reply and document are the
/// command's after those settings were set, and Undo takes the settings back with the objects.
#[test]
fn the_windows_document_settings_are_planned_and_set_in_the_same_undo_step() {
    let document = json!({"fabric": "denim", "fillAngle": "acrossShape", "joinSameColor": false});
    let params = json!({"colors": 3});
    let (mut window, image) = placed(&picture());
    let (mut command, _) = placed(&picture());
    let before = window.doc().unwrap().doc.embroidery.clone();
    let req = Arc::new(window.digitize_window_request(image, &params, None, &document).unwrap());
    let out = window.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let planned = out.reply(&req).unwrap();
    command.execute("embroidery.setup", &document).unwrap();
    let dry = command.execute("embroidery.digitizeImage", &with(&params, &json!({"dryRun": true, "limit": 0}))).unwrap();
    assert_eq!(steady(planned), steady(dry), "planned with the window's settings");
    let steps = window.doc().unwrap().history.undo.len();
    window.apply_digitize_window(&req, &out, false).unwrap();
    command.execute("embroidery.digitizeImage", &with(&params, &json!({"limit": 0}))).unwrap();
    assert_eq!(*doc(&window), *doc(&command), "the settings and the objects, as the command makes them");
    assert_eq!(window.doc().unwrap().history.undo.len(), steps + 1, "one undo step");
    window.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(window.doc().unwrap().doc.embroidery, before, "Undo takes the settings back");
}

/// The window's request says the document's settings as its objects are planned with them, each
/// with where it comes from, its changes in place: the fabric chosen in it and its rows' spacing
/// from that fabric, the fill angle across the shape, and its palette.
#[test]
fn the_windows_request_says_the_settings_it_plans_with() {
    let (s, image) = placed(&picture());
    let plain = s.digitize_window_request(image, &json!({}), None, &json!({})).unwrap().document_settings();
    assert_eq!(plain.resolved.fabric.tatami.spacing.source, "default");
    assert!(plain.resolved.fabric.preset.is_none());
    let document = json!({"fabric": "denim", "fillAngle": "acrossShape"});
    let changed = s.digitize_window_request(image, &json!({}), Some("pec"), &document).unwrap().document_settings();
    assert_eq!(changed.resolved.fabric.preset.as_ref().map(|p| p.id.as_str()), Some("denim"));
    assert_eq!(changed.resolved.fabric.tatami.spacing.source, "document");
    assert_eq!(changed.resolved.fill_angle.value, json!("acrossShape"));
    assert_eq!(changed.resolved.palette.value, "pec");
    assert!(s.doc().unwrap().doc.embroidery.is_none(), "the document itself as it was");
}

/// The window's document settings are refused as `embroidery.setup` refuses them, and only the
/// three it changes are taken.
#[test]
fn the_windows_document_settings_are_refused_as_the_setup_refuses_them() {
    let (mut s, image) = placed(&picture());
    for refused in [json!({"fabric": "velvet"}), json!({"fillAngle": 999}), json!({"joinSameColor": "yes"})] {
        let window = s.digitize_window_request(image, &json!({}), None, &refused).map(|_| ()).unwrap_err().to_string();
        let setup = s.execute("embroidery.setup", &refused).map(|_| ()).unwrap_err().to_string();
        assert_eq!(window, setup, "{refused}");
    }
    for other in [json!({"palette": "pec"}), json!({"machine": {"speed": 600}}), json!(5)] {
        let e = s.digitize_window_request(image, &json!({}), None, &other).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("fabric, fillAngle and joinSameColor"), "{other}: {e}");
    }
}

/// Each area is named by its thread and its number among that thread's areas in sewing order, and
/// placed roughly in the design's box (in thirds): two orange squares apart, a navy one between.
#[test]
fn areas_are_named_by_thread_and_number() {
    let (s, image) = placed(&three_squares());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let left = out.area_at(50, 50).unwrap();
    let right = out.area_at(250, 50).unwrap();
    let middle = out.area_at(150, 50).unwrap();
    assert_eq!(left.thread_name, right.thread_name);
    assert_ne!(left.thread_name, middle.thread_name);
    let mut own = [left.own_number, right.own_number];
    own.sort_unstable();
    assert_eq!(own, [Some(1), Some(2)], "numbered within their thread");
    assert_eq!(middle.own_number, Some(1));
    assert_eq!((left.place, middle.place, right.place), (Place::Left, Place::Middle, Place::Right));
    assert!(left.sewn && (left.area_mm2 - 36.0).abs() < 1.0, "{left:?}");
    assert!(out.area_at(5, 5).is_none_or(|a| !a.sewn), "the background is no area sewn");
}

/// The command's reply gives an area the size and area the window's inspector shows, of its own
/// traced shape once the shapes are done, not of its pixels (the project owner's choice,
/// 2026-11-26, „Jak okno (polecam)”), and its box is the one that size measures: a dot placed at
/// 72 ppi, a pixel a point, as the app check's picture was, whose pixels (the traced shape drawn
/// back by their centres) read it smaller than its curve.
#[test]
fn the_replys_area_sizes_are_the_inspectors() {
    // Millimetres a pixel at 72 ppi (a pixel a point); the dot's middle and radius (pixels); and how
    // near two measures given to hundredths must come (millimetres, square millimetres; a box's
    // side is the difference of two).
    const MM_PER_PX_72: f64 = MM_PER_PT;
    const MIDDLE: (f64, f64) = (20.0, 20.0);
    const RADIUS_PX: f64 = 5.7;
    const HUNDREDTH: f64 = 0.01;
    let in_dot = |x: u32, y: u32| (f64::from(x) + 0.5 - MIDDLE.0).hypot(f64::from(y) + 0.5 - MIDDLE.1) <= RADIUS_PX;
    let picture = tr::Raster::from_fn(40, 40, |x, y| if in_dot(x, y) { NAVY } else { WHITE });
    let (mut s, image) = placed_at(&picture, MM_PER_PX_72);
    let params = json!({"colors": 2});
    let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let facts = out.area_at(20, 20).expect("the dot");
    let reply = s.execute("embroidery.digitizeImage", &with(&params, &json!({"dryRun": true}))).unwrap();
    let dot = reply["areas"].as_array().and_then(|a| a.iter().find(|x| x["color"] == json!(hex(NAVY)))).cloned().expect("the dot listed");
    let near = |v: &Value, want: f64, by: f64| v.as_f64().is_some_and(|v| (v - want).abs() <= by);
    let size = &dot["size"];
    assert!(near(&size["area"], facts.area_mm2, HUNDREDTH), "area {} against the inspector's {}", size["area"], facts.area_mm2);
    let (w, h) = facts.size_mm;
    assert!(near(&size["width"], w, HUNDREDTH) && near(&size["height"], h, HUNDREDTH), "{size} against the inspector's {w} × {h}");
    let side = |i: usize, j: usize| json!(dot["box"][j].as_f64().unwrap_or(0.0) - dot["box"][i].as_f64().unwrap_or(0.0));
    assert!(near(&side(0, 2), w, 2.0 * HUNDREDTH) && near(&side(1, 3), h, 2.0 * HUNDREDTH), "the box {} that size measures", dot["box"]);
    // What its pixels say, the reply's measure before: their count and their box, apart from the
    // curve's, so that this test tells the two measures apart.
    let areas = &out.lines.as_ref().expect("the lines").value.areas;
    let a = areas.regions.at(20, 20).expect("the dot's pixels") as usize;
    let by_pixels = areas.areas[a].pixels as f64 * MM_PER_PX_72 * MM_PER_PX_72;
    let [x0, _, x1, _] = areas.regions.areas[a].bbox;
    let width_by_pixels = f64::from(x1 - x0) * MM_PER_PX_72;
    assert!(
        (by_pixels - facts.area_mm2).abs() > HUNDREDTH,
        "the dot's area by its pixels apart from its curve's: {by_pixels} and {}",
        facts.area_mm2
    );
    assert!((width_by_pixels - w).abs() > 2.0 * HUNDREDTH, "the dot's width by its pixels apart from its curve's: {width_by_pixels} and {w}");
}

/// The Colours view's raster paints each area in its thread's own colour, the background clear.
#[test]
fn the_painted_raster_uses_the_threads_colours() {
    let (s, image) = placed(&picture());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let reply = out.reply(&req).unwrap();
    let thread_of = |c: [u8; 4]| {
        let row = reply["colors"].as_array().unwrap().iter().find(|r| r["color"] == hex(c)).unwrap();
        row["thread"]["color"].as_str().unwrap().to_string()
    };
    let (w, h, rgba) = out.painted_rgba().unwrap();
    assert_eq!((w, h), (300, 200));
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let [r, g, b, a] = at(40, 40);
    assert_eq!((super::super::hex([r, g, b]), a), (thread_of(ORANGE), 255));
    let [r, g, b, a] = at(200, 100);
    assert_eq!((super::super::hex([r, g, b]), a), (thread_of(GREEN), 255));
    assert_eq!(at(5, 5)[3], 0, "the background clear");
}

/// A setting's values a random change picks from.
fn change() -> impl Strategy<Value = Value> {
    (0..8usize, 0..3usize).prop_map(|(key, k)| {
        let pick = |values: [Value; 3]| values.into_iter().nth(k).unwrap_or(Value::Null);
        match key {
            0 => json!({"colors": pick([json!(2), json!(3), json!(4)])}),
            1 => json!({"minArea": pick([json!(0.1), json!(1.0), json!(3.0)])}),
            2 => json!({"overlap": pick([json!(0.0), json!(0.4), json!(0.8)])}),
            3 => json!({"crop": pick([Value::Null, json!([10, 10, 290, 190]), json!([0, 0, 150, 200])])}),
            4 => json!({"width": pick([Value::Null, json!(20.0), json!(30.0)])}),
            5 => json!({"mode": pick([json!("color"), json!("logo"), json!("grayscale")])}),
            6 => json!({"background": pick([Value::Null, json!("none"), json!(hex(WHITE))])}),
            _ => json!({"colorChoices": pick([
                json!([]),
                json!([{"color": hex(ORANGE), "job": "fill"}]),
                json!([{"color": hex(GREEN), "mergeInto": hex(ORANGE)}]),
            ])}),
        }
    })
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(8))]

    /// Kept or fresh, the outputs are the same: after random changes run one by one through one
    /// pipeline (the colours, the smallest area, the overlap, the choices, the crop, the size, the
    /// mode and the background), its last run answers and makes what a fresh pipeline's run of
    /// the same settings does.
    #[test]
    fn kept_or_fresh_the_outputs_are_the_same(changes in proptest::collection::vec(change(), 1..5)) {
        let (s, image) = placed(&picture());
        let mut pipe = s.digitize_pipeline();
        let mut params = json!({"colors": 3});
        let mut last = None;
        for c in &changes {
            params = with(&params, c);
            let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
            let out = pipe.run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
            last = Some((req, out));
        }
        let (req, kept) = last.unwrap();
        let fresh = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
        prop_assert_eq!(steady(kept.reply(&req).unwrap()), steady(fresh.reply(&req).unwrap()));
        prop_assert_eq!(kept.made().map(|(d, g)| (d.as_ref().clone(), g)), fresh.made().map(|(d, g)| (d.as_ref().clone(), g)));
    }
}

/// Crop to the Art crops to the box of every area that isn't the background, in the image's own
/// pixels: the orange and green squares (the navy dot inside the orange).
#[test]
fn art_bounds_are_the_box_of_what_isnt_background() {
    let (s, image) = placed(&picture());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    assert_eq!(out.art_bounds(), Some([20, 20, 230, 180]));
    assert_eq!(out.image_px(), Some((300, 200)));
    // Cropped to a corner of white alone, nothing is art.
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3, "crop": [0, 0, 15, 15]}), None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {});
    assert!(out.map_or(true, |o| o.art_bounds().is_none()));
}

/// What a run made of `params` for `raster` placed and selected: its request and its outputs.
fn run_of(raster: &tr::Raster, params: Value) -> (Session, Arc<super::pipeline::WindowRequest>, super::pipeline::Outputs) {
    let (s, image) = placed(raster);
    let req = Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    (s, req, out)
}

/// An area's key (its deepest point in the image's pixels, its colour, its kind) finds it again in
/// another run: with a colour more, the square under the same pixel is found, and the key the
/// background would have is none.
#[test]
fn an_areas_key_finds_it_again_after_a_recompute() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let request = |params: Value| Arc::new(s.digitize_window_request(image, &params, None, &json!({})).unwrap());
    let three = pipe.run(&request(json!({"colors": 3})), &|_, _| true, &mut |_, _, _| {}).unwrap();
    let key = three.area_key(180, 100).expect("the green square's key");
    let like = |c: [u8; 3], to: [u8; 4]| vectorcraft_stitch::color::delta_e_rgb(c, [to[0], to[1], to[2]]) < 1.0;
    assert!(like(key.color, GREEN) && !key.line, "{key:?}");
    assert!(three.area_key(5, 5).is_none(), "the background has no key");
    let four = pipe.run(&request(json!({"colors": 4})), &|_, _| true, &mut |_, _, _| {}).unwrap();
    let (x, y) = four.find(&key).expect("found again");
    assert_eq!(four.area_at(x, y).map(|f| f.color), four.area_at(180, 100).map(|f| f.color), "the same square");
    let navy = super::pipeline::AreaKey { color: [NAVY[0], NAVY[1], NAVY[2]], ..key };
    assert_eq!(four.find(&navy), None, "a key of another colour finds nothing there");
}

/// Similar areas are of the same colour and kind, sizes within a quarter of each other: the two
/// orange squares are each other's, the navy one between them has none.
#[test]
fn similar_areas_are_alike_in_colour_kind_and_size() {
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3}));
    let (left, right) = (out.area_key(50, 50).unwrap(), out.area_key(250, 50).unwrap());
    assert_eq!(out.similar(50, 50), vec![right]);
    assert_eq!(out.similar(250, 50), vec![left]);
    assert_eq!(out.similar(150, 50), vec![]);
}

/// Similar areas are of a similar shape too (the project owner's choice, 2026-11-18): three navy
/// bars, lying, upright and at 45°, are each other's whichever way they lie, but not the navy square
/// of about their size; a fourth, a little shorter, is like them still. The bar at 45° has a box
/// about square: a ratio of its box's sides would take it for the square.
#[test]
fn similar_areas_are_alike_in_shape_whichever_way_they_lie() {
    // Filled bars 6 to 1 (120 × 20 px, 12 × 2 mm: the window's test bar's 21 × 3.5 mm in its
    // proportions), one upright, one at 45° (from the middle of its end `AT`, along `(1, 1)`,
    // `HALF_WIDTH_PX` either side of that line), one 5.5 to 1, and a square of a bar's size
    // (49 × 49 px against 120 × 20).
    const AT: (f64, f64) = (215.0, 110.0);
    const LONG_PX: f64 = 120.0;
    const HALF_WIDTH_PX: f64 = 10.0;
    let slanted = |x: u32, y: u32| {
        let (dx, dy) = (f64::from(x) + 0.5 - AT.0, f64::from(y) + 0.5 - AT.1);
        let (along, across) = ((dx + dy) / 2f64.sqrt(), (dy - dx) / 2f64.sqrt());
        (0.0..LONG_PX).contains(&along) && across.abs() < HALF_WIDTH_PX
    };
    let picture = tr::Raster::from_fn(320, 260, |x, y| {
        let bar = inside(x, y, [20, 20, 140, 40]) || inside(x, y, [180, 20, 200, 140]) || inside(x, y, [20, 200, 130, 220]);
        if bar || slanted(x, y) || inside(x, y, [250, 20, 299, 69]) { NAVY } else { WHITE }
    });
    let (_, _, out) = run_of(&picture, json!({"colors": 2}));
    let key = |x, y| out.area_key(x, y).expect("an area");
    let (lying, upright, diagonal, shorter, square) = (key(80, 30), key(190, 80), key(257, 152), key(75, 210), key(274, 44));
    assert!(!lying.line && !diagonal.line && !square.line, "filled areas: {lying:?} {diagonal:?} {square:?}");
    let alike = out.similar(80, 30);
    assert!(alike.contains(&upright) && alike.contains(&diagonal) && alike.contains(&shorter), "the bars, upright, at 45° or shorter: {alike:?}");
    assert!(!alike.contains(&square), "not the square of a bar's size: {alike:?}");
    assert!(!out.similar(257, 152).contains(&square), "the bar at 45° unlike the square");
    let bars = [lying, upright, diagonal, shorter];
    assert!(!out.similar(274, 44).iter().any(|k| bars.contains(k)), "the square is like no bar");
}

/// Apply to Similar Areas compares the elongations the pipeline kept with the lines
/// (`Outputs::elongations`) and walks no map itself: given the navy square a bar's elongation, it
/// is taken for the bar's like, which its shape alone isn't (its size already is).
#[test]
fn similar_areas_compare_the_elongations_kept_with_the_lines() {
    let navy = |x: u32, y: u32| inside(x, y, [20, 20, 140, 40]) || inside(x, y, [200, 20, 249, 69]);
    let picture = tr::Raster::from_fn(320, 120, |x, y| if navy(x, y) { NAVY } else { WHITE });
    let (_, _, mut out) = run_of(&picture, json!({"colors": 2}));
    let square = out.area_key(224, 44).expect("the square");
    assert!(!out.similar(80, 30).contains(&square), "the square unlike the bar by its shape");
    let regions = &out.lines.as_ref().expect("the lines").value.areas.regions;
    let (bar, at) = (regions.at(80, 30).expect("the bar") as usize, regions.at(224, 44).expect("the square") as usize);
    let mut kept = out.elongations.as_deref().cloned().expect("elongations kept with the lines");
    kept[at] = kept[bar];
    out.elongations = Some(Arc::new(kept));
    assert!(out.similar(80, 30).contains(&square), "the square alike once it is kept as long as the bar");
}

/// Apply to Similar Areas' shapes follow the lines output they are asked of, not one kept from an
/// earlier output: one pipeline digitizes the picture cropped to a bar and a square, then the whole
/// of it, a second bar below them coming in (the image's stage and every stage after it made again).
/// The upper bar is like the lower one, not the square, with the elongations a fresh pipeline works
/// out; those kept from the crop are of its areas, the lower bar not among them.
#[test]
fn similar_areas_follow_the_lines_when_they_are_made_again() {
    // The crop (the image's pixels) leaves out the lower bar, and keeps the upper one and the square
    // whole.
    const CROP: [u32; 4] = [0, 0, 320, 80];
    let navy = |x: u32, y: u32| inside(x, y, [20, 20, 140, 40]) || inside(x, y, [200, 20, 249, 69]) || inside(x, y, [20, 90, 140, 110]);
    let picture = tr::Raster::from_fn(320, 120, |x, y| if navy(x, y) { NAVY } else { WHITE });
    let (s, image) = placed(&picture);
    let request = |params: &Value| Arc::new(s.digitize_window_request(image, params, None, &json!({})).unwrap());
    let whole = json!({"colors": 2});
    let mut pipe = s.digitize_pipeline();
    pipe.run(&request(&with(&whole, &json!({"crop": CROP}))), &|_, _| true, &mut |_, _, _| {}).unwrap();
    let again = pipe.run(&request(&whole), &|_, _| true, &mut |_, _, _| {}).unwrap();
    let fresh = s.digitize_pipeline().run(&request(&whole), &|_, _| true, &mut |_, _, _| {}).unwrap();
    let lower = again.area_key(80, 100).expect("the lower bar");
    assert_eq!(again.similar(80, 30), vec![lower], "the upper bar like the lower one, not the square");
    assert_eq!(again.elongations, fresh.elongations, "the elongations of the lines made again, as a fresh pipeline's");
}

/// How long Apply to Similar Areas' shape rule takes on the largest working map (1,500 px a side):
/// the pass over the region map that works out every area's elongation (`Regions::elongations`),
/// which the window's pipeline makes each time it moves to another lines output, and
/// `Outputs::similar` on an area of a
/// grid of squares, which the window's interface thread runs each time the inspector works out its
/// alike areas again. A measurement, run by hand on an idle machine:
/// `cargo test --release -p vectorcraft-engine --lib -- --ignored --nocapture the_shape_rule_on_the_largest_working_map`.
#[test]
#[ignore = "a measurement, run by hand on an idle machine"]
fn the_shape_rule_on_the_largest_working_map() {
    // The largest working map's side (pixels), a grid of squares on it, their side and pitch
    // (pixels: 625 squares of two colours), the times each is timed, and milliseconds a second.
    const SIDE: u32 = super::working::MAX_WORKING_SIDE_PX as u32;
    const SQUARE: u32 = 40;
    const PITCH: u32 = 60;
    const RUNS: usize = 20;
    const MS_PER_SEC: f64 = 1000.0;
    let picture = tr::Raster::from_fn(SIDE, SIDE, |x, y| match (x % PITCH < SQUARE && y % PITCH < SQUARE, (x / PITCH + y / PITCH) % 2) {
        (true, 0) => ORANGE,
        (true, _) => NAVY,
        _ => WHITE,
    });
    let (_, _, out) = run_of(&picture, json!({"colors": 3}));
    let regions = &out.shapes.as_ref().expect("the shapes").value.areas.regions;
    assert_eq!((regions.w, regions.h), (SIDE, SIDE), "the largest working map");
    let time = |f: &dyn Fn()| {
        let mut ms: Vec<f64> = (0..RUNS)
            .map(|_| {
                let started = std::time::Instant::now();
                f();
                started.elapsed().as_secs_f64() * MS_PER_SEC
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        (ms[RUNS / 2], ms[RUNS - 1])
    };
    let pass = time(&|| {
        std::hint::black_box(regions.elongations());
    });
    // The middle of the first square.
    let at = (SQUARE / 2, SQUARE / 2);
    let similar = time(&|| {
        std::hint::black_box(out.similar(at.0, at.1));
    });
    println!(
        "{} of {} areas alike on a {SIDE} px map: the pass p50 {:.2} ms, max {:.2} ms; similar p50 {:.2} ms, max {:.2} ms",
        out.similar(at.0, at.1).len(),
        regions.areas.len(),
        pass.0,
        pass.1,
        similar.0,
        similar.1
    );
}

/// An area's place in the sewing order names the areas sewn just before and after it.
#[test]
fn the_sewing_place_names_the_areas_either_side() {
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3}));
    let places: Vec<_> = [(50, 50), (150, 50), (250, 50)].into_iter().map(|(x, y)| out.sewing_place(x, y).unwrap()).collect();
    assert!(places.iter().all(|p| p.of == 3));
    let mut numbers: Vec<usize> = places.iter().map(|p| p.number).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2, 3]);
    let first = places.iter().find(|p| p.number == 1).unwrap();
    assert!(first.before.is_none() && first.after.as_ref().and_then(|(a, _)| a.number) == Some(2), "{first:?}");
    let last = places.iter().find(|p| p.number == 3).unwrap();
    assert!(last.after.is_none() && last.before.as_ref().and_then(|(a, _)| a.number) == Some(2), "{last:?}");
    // Each neighbour's key finds it.
    let (facts, key) = first.after.clone().unwrap();
    assert_eq!(out.find(&key).and_then(|(x, y)| out.area_at(x, y)).and_then(|f| f.number), facts.number);
}

/// The areas of a colour not sewn are left out of the keyboard's way and of the numbers: with the
/// navy square not sewn, the first key is an orange square's, the two are numbered 1 and 2 of 2,
/// the first with none before it, and the navy square has no place.
#[test]
fn navigation_skips_the_areas_of_colours_not_sewn() {
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3, "colorChoices": [{"color": hex(NAVY), "job": "none"}]}));
    let orange = |c: [u8; 3]| vectorcraft_stitch::color::delta_e_rgb(c, [ORANGE[0], ORANGE[1], ORANGE[2]]) < 1.0;
    assert!(out.first_key().is_some_and(|k| orange(k.color)), "{:?}", out.first_key());
    assert_eq!(out.sewing_place(150, 50), None, "the navy square isn't sewn");
    let places: Vec<_> = [(50, 50), (250, 50)].into_iter().map(|(x, y)| out.sewing_place(x, y).unwrap()).collect();
    assert!(places.iter().all(|p| p.of == 2), "{places:?}");
    let first = places.iter().find(|p| p.number == 1).expect("one numbered 1");
    assert!(first.before.is_none(), "{first:?}");
    let mut numbers: Vec<_> = [(50, 50), (250, 50)].into_iter().filter_map(|(x, y)| out.area_at(x, y).and_then(|f| f.number)).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2]);
}

/// Satin is offered on a filled area by the shape it is made as: a 13 mm square with a 3 mm dot sewn
/// over it is sewn solid, 13 mm wide, too wide for satin, though the ring round the dot is narrower.
#[test]
fn satin_is_offered_by_the_shape_an_area_is_made_as() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [85, 85, 115, 115]) {
            NAVY
        } else if inside(x, y, [35, 35, 165, 165]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let (_, _, out) = run_of(&picture, json!({"colors": 2}));
    let square = out.area_at(45, 45).expect("the square");
    assert!(square.widest_mm >= 12.9 && !square.satin_width_fits(), "{square:?}");
}

/// Satin is offered by the shape as made beside centre lines too: a 10 mm bar between two 3 mm
/// stripes sewn as lines grows under half of each, about 13 mm, too wide for satin.
#[test]
fn satin_is_offered_by_the_shape_made_under_the_lines_beside_it() {
    let picture = tr::Raster::from_fn(200, 230, |x, y| {
        if inside(x, y, [20, 20, 180, 50]) || inside(x, y, [20, 150, 180, 180]) {
            NAVY
        } else if inside(x, y, [20, 50, 180, 150]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let (_, _, out) = run_of(&picture, json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "lines"}]}));
    let bar = out.area_at(100, 100).expect("the bar");
    assert!(bar.widest_mm >= 12.9 && !bar.satin_width_fits(), "{bar:?}");
}

/// An island's own island that isn't sewn over it keeps its hole in the shape the area round them
/// is made as: a 13 mm square holding a thin navy ring (sewn over it) that holds a green dot (sewn
/// before the ring, so not over it) is made with the dot's hole, about 6 mm at its widest.
#[test]
fn an_islands_island_not_sewn_over_it_keeps_its_hole() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [88, 88, 112, 112]) {
            GREEN
        } else if inside(x, y, [82, 82, 118, 118]) {
            NAVY
        } else if inside(x, y, [35, 35, 165, 165]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let (_, _, out) = run_of(&picture, json!({"colors": 3}));
    let square = out.area_at(45, 45).expect("the square");
    assert!(square.widest_mm < 8.0 && square.satin_width_fits(), "{square:?}");
}

/// An area a change leaves out (Don't Sew) is sewn by no one: the window says so, gives it no
/// number or place, and the keyboard starts past it, though it was the first sewn; a click still
/// finds it.
#[test]
fn an_area_left_out_by_a_change_is_not_sewn_in_the_window() {
    let (_, _, plain) = run_of(&three_squares(), json!({"colors": 3}));
    let first = plain.first_key().expect("the area sewn first");
    let dont_sew = json!([{"at": first.at, "color": super::super::hex(first.color), "kind": "fill", "stitch": "none"}]);
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3, "areaChanges": dont_sew}));
    let (x, y) = out.find(&first).expect("found again");
    let left = out.area_at(x, y).expect("the area left out");
    assert!(!left.sewn && left.number.is_none(), "{left:?}");
    assert_eq!(out.sewing_place(x, y), None, "no place");
    let others: Vec<_> = [(50, 50), (150, 50), (250, 50)].into_iter().filter_map(|(x, y)| out.sewing_place(x, y)).collect();
    assert!(others.len() == 2 && others.iter().all(|p| p.of == 2), "{others:?}");
    let mut numbers: Vec<_> = [(50, 50), (150, 50), (250, 50)].into_iter().filter_map(|(x, y)| out.area_at(x, y).and_then(|f| f.number)).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2], "the others numbered 1 and 2");
    assert!(out.first_key().is_some_and(|k| k.at != first.at), "the keyboard starts past it");
    assert!(out.area_key(x, y).is_some(), "it keeps a key: a click finds it");
    // Show in Sewing Order: it is sewn after nothing, and nothing is veiled for it.
    assert!(out.sewn_after(x, y).is_none(), "no veil from it");
    let next = out.first_key().and_then(|k| out.find(&k)).expect("the first area sewn");
    let (w, _, mask) = out.sewn_after(next.0, next.1).expect("a veil from the first");
    assert_eq!(mask.get((y * w + x) as usize), Some(&0), "the area left out isn't veiled as sewn after");
}

/// An area left out by a change keeps its number among its thread's areas, and so do the others:
/// a name never moves to another area while the window shows it, and two areas of one thread left
/// out are still named apart; nor does any other area's rough place move.
#[test]
fn an_area_left_out_keeps_its_number_and_the_others_keep_theirs() {
    let (_, _, plain) = run_of(&three_squares(), json!({"colors": 3}));
    // The two orange squares' numbers among the orange areas.
    let oranges = |out: &super::pipeline::Outputs| -> Vec<Option<usize>> {
        [(50, 50), (250, 50)].into_iter().map(|(x, y)| out.area_at(x, y).and_then(|f| f.own_number)).collect()
    };
    let before = oranges(&plain);
    let mut numbers: Vec<usize> = before.iter().flatten().copied().collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2], "numbered 1 and 2 among the orange areas");
    let left_out = |at: &[(u32, u32)]| {
        let changes: Vec<Value> = at
            .iter()
            .filter_map(|&(x, y)| plain.area_key(x, y))
            .map(|k| json!({"at": k.at, "color": super::super::hex(k.color), "kind": "fill", "stitch": "none"}))
            .collect();
        run_of(&three_squares(), json!({"colors": 3, "areaChanges": changes})).2
    };
    // The one sewn first left out: it keeps its 1, the other its 2.
    let first = if before.first() == Some(&Some(1)) { (50, 50) } else { (250, 50) };
    let out = left_out(&[first]);
    assert_eq!(oranges(&out), before, "each keeps its number");
    let left = out.area_at(first.0, first.1).expect("the area left out");
    assert!(!left.sewn && left.number.is_none(), "not sewn, no place in the sewing order: {left:?}");
    // The navy square between them keeps its rough place: the design's box still holds the square
    // left out (without it, the navy one read at the box's left or right).
    assert_eq!(out.area_at(150, 50).map(|f| f.place), Some(Place::Middle), "the navy square in the middle");
    // Both left out: still named apart.
    assert_eq!(oranges(&left_out(&[(50, 50), (250, 50)])), before, "both keep theirs");
}

/// A thin area of a colour set to Fill is a fill in the window too: its key is a fill's, finds it
/// again, and its facts and its alike areas read it so.
#[test]
fn a_fill_colours_thin_area_is_keyed_as_a_fill() {
    let picture = tr::Raster::from_fn(300, 100, |x, y| if inside(x, y, [40, 20, 46, 80]) { NAVY } else { WHITE });
    let (_, _, out) = run_of(&picture, json!({"colors": 2, "colorChoices": [{"color": hex(NAVY), "job": "fill"}]}));
    let key = out.area_key(43, 50).expect("the line's key");
    assert!(!key.line, "{key:?}");
    let (x, y) = out.find(&key).expect("found again");
    assert!(out.area_at(x, y).is_some_and(|f| !f.line), "a fill");
}

/// The object an area was made into is in the made document, painted with the area's thread.
#[test]
fn the_area_object_is_the_one_made_of_it() {
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3}));
    let (doc, _) = out.made().unwrap();
    for (x, y) in [(50, 50), (150, 50), (250, 50)] {
        let id = out.area_object(x, y).expect("an object");
        let thread = out.area_at(x, y).and_then(|f| f.thread_color).expect("a thread");
        let node = doc.node(id).expect("in the made document");
        let painted = node.appearance.items.iter().find_map(|i| match i {
            vectorcraft_doc::appearance::AppearanceItem::Fill(f) => match &f.paint {
                vectorcraft_color::Paint::Solid { color, .. } => Some(color.to_hex()),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(painted, Some(super::super::hex(thread)), "({x}, {y})");
    }
    assert_eq!(out.area_object(5, 5), None, "the background made nothing");
}

/// What became of each change in a run: the area it applies to, described, or why not.
#[test]
fn a_run_says_what_became_of_each_change() {
    let hex = |c: [u8; 4]| super::super::hex([c[0], c[1], c[2]]);
    let changes = json!([
        {"at": [50.5, 50.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"},
        {"at": [150.5, 50.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"},
    ]);
    let (_, req, out) = run_of(&three_squares(), json!({"colors": 3, "areaChanges": changes}));
    let states = out.change_states(&req).unwrap();
    let orange = |c: [u8; 3]| vectorcraft_stitch::color::delta_e_rgb(c, [ORANGE[0], ORANGE[1], ORANGE[2]]) < 1.0;
    assert!(matches!(states.first(), Some(super::pipeline::ChangeState::Applies(f)) if orange(f.color)), "{states:?}");
    assert_eq!(states.get(1), Some(&super::pipeline::ChangeState::Kept("otherColor")));
}

/// Show in Sewing Order's mask covers the areas sewn after an area, and nothing else.
#[test]
fn the_sewn_after_mask_covers_the_areas_sewn_later() {
    let (_, _, out) = run_of(&three_squares(), json!({"colors": 3}));
    let squares = [(50, 50), (150, 50), (250, 50)];
    let number = |(x, y): (u32, u32)| out.area_at(x, y).and_then(|f| f.number).unwrap();
    for at in squares {
        let (w, _, mask) = out.sewn_after(at.0, at.1).unwrap();
        for other in squares {
            let flag = mask[(other.1 * w + other.0) as usize];
            assert_eq!(flag == 1, number(other) > number(at), "{at:?} against {other:?}");
        }
        assert_eq!(mask[(5 * w + 5) as usize], 0, "the background never");
    }
}

/// The picture of the shapes satin is judged on, at a tenth of a millimetre a pixel: a navy bar
/// 21 × 3.5 mm, a red dot 4 mm across, a green square 5 mm a side, a purple stick 3 mm wide with a
/// round head 15 mm across, and an orange square 15 mm a side, on white.
fn satin_shapes() -> tr::Raster {
    const RED: [u8; 4] = [200, 30, 30, 255];
    const PURPLE: [u8; 4] = [120, 40, 140, 255];
    let round = |x: u32, y: u32, (cx, cy): (f64, f64), r: f64| (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) <= r;
    tr::Raster::from_fn(620, 400, |x, y| {
        if inside(x, y, [20, 20, 230, 55]) {
            NAVY
        } else if round(x, y, (60.0, 150.0), 20.0) {
            RED
        } else if inside(x, y, [120, 130, 170, 180]) {
            GREEN
        } else if inside(x, y, [260, 135, 470, 165]) || round(x, y, (520.0, 150.0), 75.0) {
            PURPLE
        } else if inside(x, y, [20, 240, 170, 390]) {
            ORANGE
        } else {
            WHITE
        }
    })
}

/// What the planner sews of an area with narrow parts as satin, and whether Satin is offered on it,
/// come from the planner's own parting (a column at least twice as long as it is wide): a long bar
/// is all satin and takes Satin; a 4 mm dot and a 5 mm square have no column, so are tatami and too
/// short; a 3 mm stick with a 15 mm round head is satin and tatami; a 15 mm square is too wide.
#[test]
fn what_the_planner_sews_of_an_area_and_whether_satin_is_offered() {
    use super::pipeline::{SatinOffer, SatinParts};
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6}));
    let at = |x, y| (out.satin_parts(x, y, None), out.satin_offer(x, y));
    assert_eq!(at(120, 37), (Some(SatinParts::All), Some(SatinOffer::Fits)), "the bar");
    assert_eq!(at(60, 150), (Some(SatinParts::None), Some(SatinOffer::TooShort)), "the dot");
    assert_eq!(at(145, 155), (Some(SatinParts::None), Some(SatinOffer::TooShort)), "the square");
    assert_eq!(at(360, 150).0, Some(SatinParts::Some), "the stick with its head");
    let wide = at(95, 315).1;
    assert!(matches!(wide, Some(SatinOffer::TooWide { width_mm, overlap: false }) if (width_mm - 15.0).abs() < 0.3), "the wide square: {wide:?}");
}

/// Two stripes 40 mm long touching on white, at a tenth of a millimetre a pixel: a red one 6.8 mm
/// wide, the larger, so sewn first and grown by the overlap under a blue one 5 mm wide sewn after
/// it.
fn grown_stripe() -> tr::Raster {
    const RED: [u8; 4] = [200, 30, 30, 255];
    const BLUE: [u8; 4] = [30, 60, 200, 255];
    tr::Raster::from_fn(300, 480, |x, y| {
        if inside(x, y, [20, 40, 88, 440]) {
            RED
        } else if inside(x, y, [88, 40, 138, 440]) {
            BLUE
        } else {
            WHITE
        }
    })
}

/// At a tenth of a millimetre a pixel, a long orange band 11.6 mm wide between two blue ones 5 mm
/// wide: the largest, it is sewn first and grows by the overlap under both, past the 12 mm the
/// planner parts satin at most; and an orange square 11.6 mm a side apart on white.
fn capped_band() -> tr::Raster {
    const BLUE: [u8; 4] = [30, 60, 200, 255];
    tr::Raster::from_fn(520, 480, |x, y| {
        if inside(x, y, [20, 40, 70, 440]) || inside(x, y, [186, 40, 236, 440]) {
            BLUE
        } else if inside(x, y, [70, 40, 186, 440]) || inside(x, y, [300, 40, 416, 156]) {
            ORANGE
        } else {
            WHITE
        }
    })
}

/// An area grown by the overlap under the areas sewn after it is judged as it is made: a 6.8 mm
/// stripe grown past 7 mm has no satin column at 7 mm (tatami for its width, not for its length)
/// and has one at Create's width, so Satin is offered; a long band 11.6 mm wide grown past 12 mm on
/// either side is too wide for Satin, said with its overlap, where a square as wide on white is too
/// short.
#[test]
fn an_area_grown_by_the_overlap_is_judged_as_it_is_made() {
    use super::pipeline::{SatinOffer, SatinParts};
    let (_, _, out) = run_of(&grown_stripe(), json!({"colors": 3}));
    assert_eq!(out.satin_parts(54, 240, None), Some(SatinParts::None), "the red stripe as planned");
    assert_eq!(out.satin_offer(54, 240), Some(SatinOffer::Fits), "the red stripe");
    assert!(out.satin_created_kept(54, 240).is_some_and(|p| p != SatinParts::None), "a column at Create's width");
    let (_, _, out) = run_of(&capped_band(), json!({"colors": 3}));
    let band = out.satin_offer(128, 240);
    assert!(matches!(band, Some(SatinOffer::TooWide { width_mm, overlap: true }) if (width_mm - 12.4).abs() < 0.3), "the band: {band:?}");
    assert_eq!(out.satin_offer(358, 98), Some(SatinOffer::TooShort), "the square");
}

/// The satin answers can be read without working them out, as the window's inspector reads them
/// (its thread works them out): none before they are worked out, nor before the stitches are
/// planned (nothing kept then); once worked out, kept for the run, a filled area with narrow parts
/// as satin off keeping its none (not asked of the planner again); and an area as planned kept
/// under its object's width too, so that with Satin chosen on it the offer's parting is that one.
#[test]
fn satin_answers_are_kept_and_read_without_working_them_out() {
    use super::pipeline::{SatinOffer, SatinParts};
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6}));
    let mut early = out.clone();
    early.plan = None;
    assert_eq!((early.satin_parts(120, 37, None), early.satin_offer(120, 37)), (None, None), "before the stitches are planned");
    assert!(!early.satin_known(120, 37));
    assert_eq!(out.satin_kept_count(), 0, "nothing kept before the plan");
    assert_eq!((out.satin_parts_kept(120, 37, None), out.satin_offer_kept(120, 37)), (None, None), "not worked out yet");
    assert!(!out.satin_known(120, 37));
    assert_eq!(out.satin_parts(120, 37, None), Some(SatinParts::All));
    assert!(!out.satin_known(120, 37), "whether Satin is offered not worked out yet");
    assert_eq!(out.satin_offer(120, 37), Some(SatinOffer::Fits));
    assert!(out.satin_known(120, 37));
    assert_eq!((out.satin_parts_kept(120, 37, None), out.satin_offer_kept(120, 37)), (Some(SatinParts::All), Some(SatinOffer::Fits)));
    // The bar's colour sewn as a fill: narrow parts as satin off, its none kept.
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6, "colorChoices": [{"color": "#142878", "job": "fill"}]}));
    assert_eq!(out.satin_parts(120, 37, None), None);
    assert_eq!(out.satin_kept_count(), 1, "its none kept");
    assert_eq!(out.satin_parts(120, 37, None), None);
    assert_eq!(out.satin_kept_count(), 1, "worked out once");
    // Satin chosen on the bar: as planned is at Create's width, and the offer parts it no more.
    let satin = json!([{"at": [125.0, 37.5], "color": "#142878", "kind": "fill", "stitch": "satin"}]);
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6, "areaChanges": satin}));
    assert_eq!(out.satin_parts(120, 37, None), Some(SatinParts::All));
    assert_eq!(out.satin_kept_count(), 2, "as planned, and at its object's width");
    assert_eq!(out.satin_offer(120, 37), Some(SatinOffer::Fits));
    assert_eq!(out.satin_kept_count(), 2, "the offer's parting the one kept");
}

/// A satin answer, or an Auto width, whose query is cancelled (a newer ask, on the window's thread)
/// is not kept: it is worked out another time. Kept, it read as having none to give for the rest of
/// the run: a bar sewn all as satin read "Auto: tatami" with no reason, and Satin read as offered
/// even on an area too short for it (a dot); kept now, it would also leave open the angle field this
/// branch greys there.
#[test]
fn a_cancelled_satin_answer_is_not_kept() {
    use super::pipeline::SatinParts;
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6}));
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(cancel::scope(&token, None, || out.satin_parts(120, 37, None)), None, "not worked out");
    assert_eq!(cancel::scope(&token, None, || out.auto_narrow_mm(120, 37)), None, "its Auto width not read");
    assert_eq!((out.satin_kept_count(), out.auto_kept_count()), (0, 0), "nothing kept");
    assert_eq!(out.satin_parts(120, 37, None), Some(SatinParts::All), "worked out afterwards");
}

/// The width up to which Auto sews an area's narrow parts as satin is read of its object's settings
/// once and kept for the run: the window's inspector reads it every frame, and it asked the planner
/// each time (a copy of the outputs without the plan, which can ask nothing, still reads it).
#[test]
fn the_auto_width_is_read_once_and_kept() {
    let (_, _, out) = run_of(&satin_shapes(), json!({"colors": 6}));
    let mut early = out.clone();
    early.plan = None;
    assert_eq!(early.auto_narrow_mm(120, 37), None, "before the stitches are planned");
    assert_eq!(out.auto_kept_count(), 0, "nothing kept before the plan");
    let width = out.auto_narrow_mm(120, 37);
    assert!(width.is_some_and(|w| w > 0.0), "{width:?}");
    let _ = (out.satin_known(120, 37), out.satin_parts_auto_kept(120, 37), out.auto_narrow_mm(120, 37));
    assert_eq!(out.auto_kept_count(), 1, "kept once");
    assert_eq!(early.auto_narrow_mm(120, 37), width, "read of what is kept");
}
