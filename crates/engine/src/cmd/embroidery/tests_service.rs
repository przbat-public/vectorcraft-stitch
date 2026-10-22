//! The planning service: a plan made with the units' stitches kept from the plans before it is
//! the plan made afresh, for random documents edited at random; what is kept stays within its
//! bound; one snapshot of a document with the same options is planned once.

use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::cache::{UnitCache, UnitKey};
use super::digitize::digitize;
use super::service::PlanService;
use super::test_support::run;
use super::unit::{PlanError, UnitRequest, UnitStitches};
use super::*;

/// The colours the random art is painted in: few, so that objects of one thread share blocks and
/// the needle's way from one to the next matters.
const COLOURS: [&str; 3] = ["#cc2222", "#2233cc", "#22aa44"];

/// One object of a random document: its kind, box (points), fill and stroke.
#[derive(Clone, Debug)]
struct Art {
    kind: u8,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: Option<usize>,
    stroke: Option<(usize, f64)>,
}

/// One random edit, of the object at an index of the objects still there.
#[derive(Clone, Debug)]
enum Edit {
    Move(usize, f64, f64),
    Fill(usize, usize),
    StrokeWeight(usize, f64),
    Delete(usize),
    ToFront(usize),
    Undo,
}

fn art() -> impl Strategy<Value = Art> {
    (
        0u8..3,
        (0.0..220.0f64, 0.0..220.0f64, 12.0..70.0f64, 12.0..70.0f64),
        proptest::option::weighted(0.8, 0..COLOURS.len()),
        proptest::option::weighted(0.4, (0..COLOURS.len(), 0.5..14.0f64)),
    )
        .prop_map(|(kind, (x, y, w, h), fill, stroke)| Art { kind, x, y, w, h, fill, stroke })
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        3 => (0usize..16, -12.0..12.0f64, -12.0..12.0f64).prop_map(|(i, dx, dy)| Edit::Move(i, dx, dy)),
        2 => (0usize..16, 0..COLOURS.len()).prop_map(|(i, c)| Edit::Fill(i, c)),
        2 => (0usize..16, 0.5..14.0f64).prop_map(|(i, w)| Edit::StrokeWeight(i, w)),
        1 => (0usize..16).prop_map(Edit::Delete),
        1 => (0usize..16).prop_map(Edit::ToFront),
        1 => Just(Edit::Undo),
    ]
}

/// The export options a random document is planned with: the defaults, or ones that change how
/// every unit is sewn.
fn options() -> impl Strategy<Value = Value> {
    prop_oneof![Just(json!({})), Just(json!({"stitchStyle": "satin"})), Just(json!({"density": 0.5, "jumpLength": 3, "trimLength": 4}))]
}

/// A session holding `arts` → it and their ids, in stacking order.
fn build(arts: &[Art]) -> (Session, Vec<u64>) {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    let mut ids = Vec::new();
    for a in arts {
        let fill = a.fill.map_or(json!({"none": true}), |c| json!({"color": COLOURS[c]}));
        run(&mut s, "paint.setFill", fill);
        run(&mut s, "paint.setStroke", json!({"none": true}));
        let created = match a.kind {
            0 => run(&mut s, "shape.rectangle", json!({"x": a.x, "y": a.y, "width": a.w, "height": a.h})),
            1 => run(&mut s, "shape.ellipse", json!({"x": a.x, "y": a.y, "width": a.w, "height": a.h})),
            _ => run(&mut s, "shape.star", json!({"cx": a.x + a.w / 2.0, "cy": a.y + a.h / 2.0, "radius1": a.w / 2.0, "radius2": a.w / 4.0})),
        };
        let id = created["id"].as_u64().unwrap();
        if let Some((c, weight)) = a.stroke {
            run(&mut s, "paint.setStroke", json!({"color": COLOURS[c], "ids": [id]}));
            run(&mut s, "stroke.set", json!({"weight": weight, "ids": [id]}));
        }
        run(&mut s, "select.none", json!({}));
        ids.push(id);
    }
    (s, ids)
}

/// The objects on the document's layers, in stacking order.
fn objects(s: &Session) -> Vec<u64> {
    s.doc().unwrap().doc.layers.iter().filter_map(|l| l.children()).flatten().map(|c| c.id.0).collect()
}

/// Apply `e` to the objects `ids` (those still there, in no particular order).
fn apply(s: &mut Session, ids: &mut Vec<u64>, e: &Edit) {
    let pick = |i: usize| ids.get(i % ids.len().max(1)).copied();
    match *e {
        Edit::Move(i, dx, dy) => {
            if let Some(id) = pick(i) {
                run(s, "object.move", json!({"ids": [id], "dx": dx, "dy": dy}));
            }
        }
        Edit::Fill(i, c) => {
            if let Some(id) = pick(i) {
                run(s, "paint.setFill", json!({"color": COLOURS[c], "ids": [id]}));
            }
        }
        Edit::StrokeWeight(i, w) => {
            if let Some(id) = pick(i) {
                run(s, "stroke.set", json!({"weight": w, "ids": [id]}));
            }
        }
        Edit::Delete(i) => {
            if let Some(id) = pick(i) {
                run(s, "edit.clear", json!({"ids": [id]}));
                ids.retain(|x| *x != id);
            }
        }
        Edit::ToFront(i) => {
            if let Some(id) = pick(i) {
                run(s, "object.arrange.bringToFront", json!({"ids": [id]}));
            }
        }
        // An undo may bring back an object deleted: the objects to edit are the document's again,
        // restored ones included.
        Edit::Undo => {
            let _ = s.execute("edit.undo", &json!({}));
            *ids = objects(s);
        }
    }
    run(s, "select.none", json!({}));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// A plan made with the stitches kept from the plans before it, after every edit, is the plan
    /// made afresh.
    #[test]
    fn a_plan_with_kept_units_is_a_fresh_plan(arts in proptest::collection::vec(art(), 1..8), edits in proptest::collection::vec(edit(), 1..8), opts in options()) {
        let opts = Options::from_params(&opts, "test").unwrap();
        let (mut s, mut ids) = build(&arts);
        let service = PlanService::default();
        for e in std::iter::once(None).chain(edits.iter().map(Some)) {
            if let Some(e) = e {
                apply(&mut s, &mut ids, e);
            }
            let doc = s.doc().unwrap().doc.clone();
            let kept = service.plan(&doc, &opts).map(|p| (*p).clone());
            let fresh = digitize(&doc, &opts);
            prop_assert_eq!(kept, fresh, "after {:?}", e);
        }
    }
}

/// A key holds every value a unit's stitches depend on: changing any one of them, the needle's
/// start point by the smallest step a float takes, changes the key.
#[test]
fn a_key_changes_with_every_value_a_unit_depends_on() {
    use vectorcraft_geom::{BezPath, FillRule, Point, Rect, Shape};
    use vectorcraft_stitch::plan::{Params, Style};
    let shape = Rect::new(0.0, 0.0, 20.0, 10.0).to_path(0.1);
    let clip = (Rect::new(0.0, 0.0, 15.0, 15.0).to_path(0.1), FillRule::NonZero);
    let base = UnitRequest {
        shape: &shape,
        rule: FillRule::NonZero,
        params: Params::for_style(Style::Fill),
        columns: Some(Params::for_style(Style::Satin)),
        from: Some(Point::new(3.0, 4.0)),
        clip: Some(&clip),
    };
    let key = UnitKey::of(&base);
    assert_eq!(key, UnitKey::of(&base.clone()), "the same request, the same key");
    let moved: BezPath = {
        let mut p = shape.clone();
        p.apply_affine(vectorcraft_geom::Affine::translate((0.1, 0.0)));
        p
    };
    let other_clip = (Rect::new(0.0, 0.0, 15.0, 16.0).to_path(0.1), FillRule::NonZero);
    let next = |v: f64| f64::from_bits(v.to_bits() + 1);
    let mut changed: Vec<UnitRequest<'_>> = vec![
        UnitRequest { shape: &moved, ..base },
        UnitRequest { rule: FillRule::EvenOdd, ..base },
        UnitRequest { columns: None, ..base },
        UnitRequest { from: None, ..base },
        UnitRequest { from: Some(Point::new(next(3.0), 4.0)), ..base },
        UnitRequest { from: Some(Point::new(3.0, next(4.0))), ..base },
        UnitRequest { clip: None, ..base },
        UnitRequest { clip: Some(&other_clip), ..base },
    ];
    // Every parameter of the unit, and of its columns.
    let tweaks: Vec<fn(&mut Params)> = vec![
        |p| p.style = if p.style == Style::Satin { Style::Fill } else { Style::Satin },
        |p| p.stitch_length += 0.1,
        |p| p.density += 0.01,
        |p| p.angle += 1.0,
        |p| p.pull[0] += 0.1,
        |p| p.pull[1] += 0.1,
        |p| p.pull_percent[0] += 1.0,
        |p| p.pull_percent[1] += 1.0,
        |p| p.push[0] += 0.1,
        |p| p.push[1] += 0.1,
        |p| p.thread_weight += 1.0,
        |p| p.underlay = !p.underlay,
        |p| p.jump += 0.5,
        |p| p.trim += 0.5,
        |p| p.lock = !p.lock,
        |p| p.min_stitch += 0.1,
        |p| p.width += 0.1,
        |p| p.max_satin += 0.1,
        |p| p.auto_spacing = !p.auto_spacing,
        |p| p.narrow += 0.1,
    ];
    for tweak in &tweaks {
        let mut params = base.params;
        tweak(&mut params);
        changed.push(UnitRequest { params, ..base });
        let mut columns = base.columns.unwrap();
        tweak(&mut columns);
        changed.push(UnitRequest { columns: Some(columns), ..base });
    }
    for (k, req) in changed.iter().enumerate() {
        assert_ne!(UnitKey::of(req), key, "change {k}");
    }
}

/// The cache holds at most its limit, dropping the units used least recently: one looked up is
/// kept over one added before it and not used since.
#[test]
fn the_units_used_least_recently_are_dropped_first() {
    use vectorcraft_geom::{BezPath, FillRule, Point};
    use vectorcraft_stitch::Stitch;
    use vectorcraft_stitch::plan::{Params, Style};
    let shapes: Vec<BezPath> = (0..4)
        .map(|k| {
            let mut p = BezPath::new();
            p.move_to(Point::new(f64::from(k), 0.0));
            p.line_to(Point::new(f64::from(k), 10.0));
            p
        })
        .collect();
    let key = |shape: &BezPath| {
        UnitKey::of(&UnitRequest { shape, rule: FillRule::NonZero, params: Params::for_style(Style::Running), columns: None, from: None, clip: None })
    };
    // Each unit 100 stitches and a key of about 40 words: 4 such units don't fit a limit of 320.
    let unit = || Arc::new(UnitStitches { stitches: vec![Stitch::sew(Point::ORIGIN); 100], columns: None, widths: None });
    let mut cache = UnitCache::with_limit(320);
    cache.insert(key(&shapes[0]), unit());
    cache.insert(key(&shapes[1]), unit());
    assert!(cache.get(&key(&shapes[0])).is_some(), "the first is used again");
    cache.insert(key(&shapes[2]), unit());
    cache.insert(key(&shapes[3]), unit());
    assert!(cache.held() <= 320, "{}", cache.held());
    assert!(cache.get(&key(&shapes[1])).is_none(), "the one used least recently went first");
    assert!(cache.get(&key(&shapes[3])).is_some(), "the last added stays");
    // A unit larger than the whole limit is not kept, and drops nothing.
    let before = cache.units();
    cache.insert(key(&shapes[1]), Arc::new(UnitStitches { stitches: vec![Stitch::sew(Point::ORIGIN); 400], columns: None, widths: None }));
    assert_eq!(cache.units(), before);
    assert!(cache.get(&key(&shapes[1])).is_none());
}

/// Each document has a service of its own, dropped when it closes; a second plan of a document
/// takes every unit from what the first kept.
#[test]
fn each_open_document_keeps_its_own_units() {
    let (mut s, _) = build(&[
        Art { kind: 0, x: 10.0, y: 10.0, w: 40.0, h: 30.0, fill: Some(0), stroke: Some((1, 4.0)) },
        Art { kind: 1, x: 80.0, y: 20.0, w: 30.0, h: 30.0, fill: Some(2), stroke: None },
    ]);
    let first = s.doc().unwrap().uid;
    run(&mut s, "embroidery.threads", json!({}));
    let service = s.embroidery_plans.of(first);
    let kept = service.kept();
    assert_eq!(kept, 3, "two fills and a stroke");
    run(&mut s, "file.new", json!({"width": 100, "height": 100, "artboards": 1}));
    let second = s.doc().unwrap().uid;
    assert_eq!(s.embroidery_plans.of(second).kept(), 0, "a document of its own");
    assert!(Arc::ptr_eq(&s.embroidery_plans.of(first), &service));
    let index = s.documents().iter().position(|d| d.uid == first).unwrap();
    s.close_document(index);
    assert_eq!(s.embroidery_plans.of(first).kept(), 0, "dropped with its document");
}

/// One snapshot of a document with the same options is planned once: the commands that read a
/// design one after another share its plan. Another snapshot, or other options, plan again, each
/// unit from what was kept.
#[test]
fn a_snapshot_with_the_same_options_is_planned_once() {
    let (mut s, ids) = build(&[
        Art { kind: 0, x: 10.0, y: 10.0, w: 40.0, h: 30.0, fill: Some(0), stroke: Some((1, 4.0)) },
        Art { kind: 1, x: 80.0, y: 20.0, w: 30.0, h: 30.0, fill: Some(0), stroke: None },
    ]);
    let doc = s.doc().unwrap().doc.clone();
    let service = PlanService::default();
    let opts = Options::default();
    let first = service.plan(&doc, &opts).unwrap();
    assert!(Arc::ptr_eq(&first, &service.plan(&doc, &opts).unwrap()), "the same snapshot, the same plan");
    assert_eq!(service.walks(), 1);
    // The same art in another snapshot: walked again, every unit from what was kept.
    let kept = service.kept();
    let copy = Arc::new((*doc).clone());
    let again = service.plan(&copy, &opts).unwrap();
    assert!(!Arc::ptr_eq(&first, &again));
    assert_eq!((*first).clone(), (*again).clone());
    assert_eq!((service.walks(), service.kept()), (2, kept), "no unit planned anew");
    // Other options: another plan.
    let satin = Options::from_params(&json!({"stitchStyle": "satin"}), "test").unwrap();
    assert_ne!(*service.plan(&doc, &satin).unwrap(), *first);
    // Through the commands: three in a row walk the document once; an edit, and its undo (an
    // earlier snapshot), walk it again.
    let uid = s.doc().unwrap().uid;
    for _ in 0..3 {
        run(&mut s, "embroidery.threads", json!({}));
    }
    let service = s.embroidery_plans.of(uid);
    assert_eq!(service.walks(), 1);
    run(&mut s, "object.move", json!({"ids": [ids[1]], "dx": 5, "dy": 0}));
    run(&mut s, "embroidery.threads", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "embroidery.threads", json!({}));
    run(&mut s, "embroidery.threads", json!({}));
    assert_eq!(service.walks(), 3);
}

/// Five objects of two colours, side by side: seven units (each fill, and two strokes).
fn five() -> (Session, Vec<u64>) {
    build(&[
        Art { kind: 0, x: 10.0, y: 10.0, w: 30.0, h: 20.0, fill: Some(0), stroke: Some((1, 4.0)) },
        Art { kind: 1, x: 60.0, y: 10.0, w: 30.0, h: 30.0, fill: Some(1), stroke: None },
        Art { kind: 0, x: 110.0, y: 10.0, w: 30.0, h: 20.0, fill: Some(0), stroke: None },
        Art { kind: 2, x: 160.0, y: 10.0, w: 30.0, h: 30.0, fill: Some(1), stroke: Some((0, 1.0)) },
        Art { kind: 0, x: 210.0, y: 10.0, w: 30.0, h: 20.0, fill: Some(0), stroke: None },
    ])
}

/// A plan cancelled part way returns the cancel and no plan, its finished units stay kept, and the
/// next plan is the fresh plan, planning only the units not yet kept. Each unit is a step of the
/// plan's progress, of how many the document has.
#[test]
fn a_cancelled_plan_leaves_no_plan_and_keeps_its_finished_units() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use vectorcraft_doc::cancel::{self, CancelToken, ProgressSink};

    let (s, _) = five();
    let doc = s.doc().unwrap().doc.clone();
    let opts = Options::default();
    let fresh = digitize(&doc, &opts).unwrap();
    assert_eq!(fresh.units.len(), 7);
    let service = PlanService::default();
    // Cancelled before it starts: nothing planned, nothing kept.
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(cancel::scope(&token, None, || service.plan(&doc, &opts)), Err(PlanError::Cancelled));
    assert_eq!(service.kept(), 0);
    // Cancelled as the third unit is reported: two units finished and kept, the third (a fill)
    // stopped while its narrow parts were looked for.
    let token = CancelToken::new();
    let steps = Rc::new(RefCell::new(Vec::new()));
    let sink: ProgressSink = {
        let (token, steps) = (token.clone(), steps.clone());
        Rc::new(move |done, total, _| {
            steps.borrow_mut().push((done, total));
            if done == 3 {
                token.cancel();
            }
        })
    };
    assert_eq!(cancel::scope(&token, Some(sink), || service.plan(&doc, &opts)), Err(PlanError::Cancelled));
    assert_eq!(*steps.borrow(), [(1, 7), (2, 7), (3, 7)]);
    assert_eq!(service.kept(), 2, "the units finished before the cancel");
    // Not cancelled: the fresh plan, with only the other five planned.
    let walks = service.walks();
    assert_eq!(*service.plan(&doc, &opts).unwrap(), fresh);
    assert_eq!((service.walks(), service.kept()), (walks + 1, 7));
}

/// Commands and the export stop on a cancel with the engine's cancel, change nothing and write
/// nothing; outside the cancelled scope they work.
#[test]
fn a_cancelled_command_or_export_reports_the_cancel() {
    use vectorcraft_doc::cancel::{self, CancelToken};

    let (mut s, _) = five();
    let token = CancelToken::new();
    token.cancel();
    let revision = s.doc().unwrap().revision;
    let r = cancel::scope(&token, None, || s.execute("embroidery.threads", &json!({})));
    assert!(matches!(r, Err(crate::EngineError::Cancelled)), "{r:?}");
    let r = cancel::scope(&token, None, || s.execute("document.export", &json!({"format": "dst"})));
    assert!(matches!(r, Err(crate::EngineError::Cancelled)), "{r:?}");
    assert_eq!(s.doc().unwrap().revision, revision);
    run(&mut s, "embroidery.threads", json!({}));
    run(&mut s, "document.export", json!({"format": "dst"}));
}

/// The service plans on another thread: the plan made there is the fresh plan, and what it kept
/// there is used here.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_service_plans_on_another_thread() {
    let (s, _) = five();
    let doc = s.doc().unwrap().doc.clone();
    let opts = Options::default();
    let service = Arc::new(PlanService::default());
    let there = {
        let (service, doc, opts) = (service.clone(), doc.clone(), opts.clone());
        std::thread::spawn(move || service.plan(&doc, &opts).map(|p| (*p).clone())).join().unwrap()
    };
    assert_eq!(there, digitize(&doc, &opts));
    let kept = service.kept();
    let here = service.plan(&Arc::new((*doc).clone()), &opts).unwrap();
    assert_eq!(Ok((*here).clone()), there);
    assert_eq!((service.walks(), service.kept()), (2, kept), "every unit taken from what was kept there");
}
