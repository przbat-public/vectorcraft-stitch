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
    let req = Arc::new(s.digitize_window_request(image, params, palette).unwrap());
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
/// mode: the same colours, threads, warnings, settings and figures.
#[test]
fn a_pipeline_run_is_the_commands_dry_run() {
    for mode in ["color", "logo"] {
        let (mut s, image) = placed(&picture());
        let params = json!({"mode": mode, "colors": 3});
        let want = steady(s.execute("embroidery.digitizeImage", &with(&params, &json!({"dryRun": true, "limit": 0}))).unwrap());
        let req = Arc::new(s.digitize_window_request(image, &params, None).unwrap());
        let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
        assert_eq!(steady(out.reply(&req).unwrap()), want, "{mode}");
    }
}

/// A change runs again its own stage and the stages after it, and keeps those before it: the size
/// from the image, the colours and the smoothing from the trace (the smallest area too, below the
/// trace's own noise of a thread's footprint, 0.16 mm²), a colour's choice and the smallest area
/// above it from the colours, the overlap from the shapes, the window's palette from the colours;
/// nothing changed, nothing runs again.
#[test]
fn a_change_reruns_its_stage_and_those_after() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let base = json!({"colors": 3, "minArea": 1.0, "overlap": 0.4});
    assert_eq!(made(&s, &mut pipe, image, &base, None), StageId::ALL.to_vec(), "the first run makes every stage");
    assert_eq!(made(&s, &mut pipe, image, &base, None), Vec::<StageId>::new(), "nothing changed");
    let cases = [
        (json!({"width": 25.0}), StageId::Image),
        (json!({"background": "none"}), StageId::Image),
        (json!({"colors": 4}), StageId::Trace),
        (json!({"smoothing": 30}), StageId::Trace),
        (json!({"minArea": 0.1}), StageId::Trace),
        (json!({"colorChoices": [{"color": hex(ORANGE), "job": "fill"}]}), StageId::Colours),
        (json!({"minArea": 2.0}), StageId::Colours),
        (json!({"overlap": 0.6}), StageId::Shapes),
    ];
    for (change, first) in cases {
        made(&s, &mut pipe, image, &base, None);
        assert_eq!(made(&s, &mut pipe, image, &with(&base, &change), None), from(first), "{change}");
    }
    made(&s, &mut pipe, image, &base, None);
    assert_eq!(made(&s, &mut pipe, image, &base, Some("pec")), from(StageId::Colours), "the window's palette");
}

/// A run cancelled after its colours keeps the stages it finished: the next run makes only those
/// after them.
#[test]
fn a_cancelled_run_keeps_the_stages_it_finished() {
    let (s, image) = placed(&picture());
    let mut pipe = s.digitize_pipeline();
    let params = json!({"colors": 3});
    let req = Arc::new(s.digitize_window_request(image, &params, None).unwrap());
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
    let req = Arc::new(s.digitize_window_request(image, &params, None).unwrap());
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
    let req = Arc::new(window.digitize_window_request(image, &params, None).unwrap());
    let out = window.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let steps = window.doc().unwrap().history.undo.len();
    let from_window = window.apply_digitize_window(&req, &out).unwrap();
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
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    s.execute("shape.rectangle", &json!({"x": 200, "y": 200, "width": 20, "height": 20})).unwrap();
    let before = doc(&s);
    let e = s.apply_digitize_window(&req, &out).unwrap_err().to_string();
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
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), Some("pec")).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    let reply = out.reply(&req).unwrap();
    assert!(reply["colors"].as_array().unwrap().iter().filter(|c| !c["thread"].is_null()).all(|c| c["thread"]["palette"] == "pec"), "{reply}");
    let steps = s.doc().unwrap().history.undo.len();
    s.apply_digitize_window(&req, &out).unwrap();
    assert_eq!(super::super::thread_palette(&doc(&s)), Palette::Pec);
    assert_eq!(s.doc().unwrap().history.undo.len(), steps + 1, "one undo step");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(super::super::thread_palette(&doc(&s)), super::super::DEFAULT_PALETTE);
}

/// Each area is named by its thread and its number among that thread's areas in sewing order, and
/// placed roughly in the design's box (in thirds): two orange squares apart, a navy one between.
#[test]
fn areas_are_named_by_thread_and_number() {
    let (s, image) = placed(&three_squares());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None).unwrap());
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

/// The Colours view's raster paints each area in its thread's own colour, the background clear.
#[test]
fn the_painted_raster_uses_the_threads_colours() {
    let (s, image) = placed(&picture());
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None).unwrap());
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
    (0..4usize, 0..3usize).prop_map(|(key, k)| {
        let pick = |values: [Value; 3]| values.into_iter().nth(k).unwrap_or(Value::Null);
        match key {
            0 => json!({"colors": pick([json!(2), json!(3), json!(4)])}),
            1 => json!({"minArea": pick([json!(0.1), json!(1.0), json!(3.0)])}),
            2 => json!({"overlap": pick([json!(0.0), json!(0.4), json!(0.8)])}),
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
    /// pipeline, its last run answers and makes what a fresh pipeline's run of the same settings
    /// does.
    #[test]
    fn kept_or_fresh_the_outputs_are_the_same(changes in proptest::collection::vec(change(), 1..5)) {
        let (s, image) = placed(&picture());
        let mut pipe = s.digitize_pipeline();
        let mut params = json!({"colors": 3});
        let mut last = None;
        for c in &changes {
            params = with(&params, c);
            let req = Arc::new(s.digitize_window_request(image, &params, None).unwrap());
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
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3}), None).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {}).unwrap();
    assert_eq!(out.art_bounds(), Some([20, 20, 230, 180]));
    assert_eq!(out.image_px(), Some((300, 200)));
    // Cropped to a corner of white alone, nothing is art.
    let req = Arc::new(s.digitize_window_request(image, &json!({"colors": 3, "crop": [0, 0, 15, 15]}), None).unwrap());
    let out = s.digitize_pipeline().run(&req, &|_, _| true, &mut |_, _, _| {});
    assert!(out.map_or(true, |o| o.art_bounds().is_none()));
}
