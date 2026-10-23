//! The planning service: a plan made with the units' stitches (and the areas the path operations
//! made) kept from the plans before it is the plan made afresh, for random documents edited at
//! random, with a cache small enough to drop what it keeps or not; what is kept stays within its
//! bound; one snapshot of a document with the same options is planned once.

use std::sync::Arc;

use proptest::prelude::*;
use serde_json::{Value, json};

use super::cache::{UnitCache, UnitKey};
use super::digitize::digitize;
use super::service::{Of, PlanService};
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

/// One random edit, of the object at an index of the objects still there, of the document or of
/// how it is planned.
#[derive(Clone, Debug)]
enum Edit {
    Move(usize, f64, f64),
    Fill(usize, usize),
    StrokeWeight(usize, f64),
    Delete(usize),
    ToFront(usize),
    Undo,
    Redo,
    Hide(usize),
    ShowAll,
    TrimAfter(usize, bool),
    /// The thread palette: Brother's, or Janome's.
    Palette(bool),
    /// A clip group of two objects (the upper one clips).
    Clip(usize, usize),
    /// An object made a symbol, and the symbol placed once more.
    Symbol(usize, f64, f64),
    /// Planned with the options at an index of [`OPTIONS`] from here on.
    Options(usize),
    /// The document exported to PES and opened: its runs literal stitches.
    Open,
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
        1 => Just(Edit::Redo),
        1 => (0usize..16).prop_map(Edit::Hide),
        1 => Just(Edit::ShowAll),
        1 => (0usize..16, any::<bool>()).prop_map(|(i, on)| Edit::TrimAfter(i, on)),
        1 => any::<bool>().prop_map(Edit::Palette),
        1 => (0usize..16, 0usize..16).prop_map(|(a, b)| Edit::Clip(a, b)),
        1 => (0usize..16, 0.0..220.0f64, 0.0..220.0f64).prop_map(|(i, x, y)| Edit::Symbol(i, x, y)),
        1 => (0..OPTIONS.len()).prop_map(Edit::Options),
        1 => Just(Edit::Open),
    ]
}

/// The export options a random document is planned with: the defaults, ones that change how every
/// unit is sewn, and `expand` both ways (fills grown or shrunk by the path operations).
const OPTIONS: [&str; 5] =
    [r#"{}"#, r#"{"stitchStyle": "satin"}"#, r#"{"density": 0.5, "jumpLength": 3, "trimLength": 4}"#, r#"{"expand": 0.3}"#, r#"{"expand": -0.4}"#];

/// The options at index `k` of [`OPTIONS`].
fn options_at(k: usize) -> Options {
    let v: Value = serde_json::from_str(OPTIONS[k % OPTIONS.len()]).unwrap();
    Options::from_params(&v, "test").unwrap()
}

/// How many stitches the service's cache may hold: the default, or few enough that it drops units
/// within a plan and between plans.
fn cache_limit() -> impl Strategy<Value = Option<usize>> {
    prop_oneof![2 => Just(None), 1 => Just(Some(400)), 1 => Just(Some(4000))]
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

/// Apply `e` to the objects `ids` (those still there, in no particular order), planned with
/// `opts`.
fn apply(s: &mut Session, ids: &mut Vec<u64>, opts: &mut Options, e: &Edit) {
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
        // An undo (or a redo) may bring back an object deleted: the objects to edit are the
        // document's again, restored ones included.
        Edit::Undo => {
            let _ = s.execute("edit.undo", &json!({}));
            *ids = objects(s);
        }
        Edit::Redo => {
            let _ = s.execute("edit.redo", &json!({}));
            *ids = objects(s);
        }
        Edit::Hide(i) => {
            if let Some(id) = pick(i) {
                run(s, "object.hide", json!({"ids": [id]}));
            }
        }
        Edit::ShowAll => {
            let _ = s.execute("object.showAll", &json!({}));
        }
        Edit::TrimAfter(i, on) => {
            if let Some(id) = pick(i) {
                run(s, "embroidery.setTrimAfter", json!({"on": on, "ids": [id]}));
            }
        }
        Edit::Palette(pec) => {
            run(s, "embroidery.setup", json!({"palette": if pec { "pec" } else { "jef" }}));
        }
        // Either may fail (one object, or objects in different groups): the document is then as
        // it was.
        Edit::Clip(a, b) => {
            if let (Some(a), Some(b)) = (pick(a), pick(b))
                && a != b
            {
                run(s, "select.set", json!({"ids": [a, b]}));
                let _ = s.execute("object.clippingMask.make", &json!({}));
                *ids = objects(s);
            }
        }
        Edit::Symbol(i, x, y) => {
            if let Some(id) = pick(i) {
                let name = format!("Symbol {}", s.doc().unwrap().doc.symbols.len());
                run(s, "select.set", json!({"ids": [id]}));
                if s.execute("symbol.new", &json!({"name": name})).is_ok() {
                    let _ = s.execute("symbol.place", &json!({"name": name, "x": x, "y": y}));
                }
                *ids = objects(s);
            }
        }
        Edit::Options(k) => *opts = options_at(k),
        Edit::Open => {
            if let Ok(r) = s.execute("document.export", &json!({"format": "pes"})) {
                let data = r["dataBase64"].as_str().unwrap().to_string();
                run(s, "document.open", json!({"name": "design.pes", "dataBase64": data}));
                *ids = objects(s);
            }
        }
    }
    run(s, "select.none", json!({}));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// A plan made with the stitches and areas kept from the plans before it, after every edit, is
    /// the plan made afresh, whatever the cache had room for.
    #[test]
    fn a_plan_with_kept_units_is_a_fresh_plan(
        arts in proptest::collection::vec(art(), 1..8),
        edits in proptest::collection::vec(edit(), 1..10),
        first in 0..OPTIONS.len(),
        limit in cache_limit(),
    ) {
        let mut opts = options_at(first);
        let (mut s, mut ids) = build(&arts);
        let service = limit.map_or_else(PlanService::default, PlanService::with_limit);
        for e in std::iter::once(None).chain(edits.iter().map(Some)) {
            if let Some(e) = e {
                apply(&mut s, &mut ids, &mut opts, e);
            }
            let doc = s.doc().unwrap().doc.clone();
            let kept = service.plan(Of::whole(&doc), &doc, &opts).map(|p| (*p).clone());
            let fresh = digitize(&doc, &opts, None);
            prop_assert_eq!(kept, fresh, "after {:?}", e);
        }
    }
}

/// A key holds every value a unit's stitches depend on: changing any one of them, the needle's
/// start point by the smallest step a float takes, changes the key.
#[test]
fn a_key_changes_with_every_value_a_unit_depends_on() {
    use vectorcraft_geom::{BezPath, FillRule, Point, Rect, Shape};
    use vectorcraft_stitch::plan::{Params, Style, Underlay};
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
    let key = UnitKey::of(&base, None);
    assert_eq!(key, UnitKey::of(&base.clone(), None), "the same request, the same key");
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
        |p| p.underlay = if p.underlay == Underlay::None { Underlay::Auto } else { Underlay::None },
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
        assert_ne!(UnitKey::of(req, None), key, "change {k}");
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
        UnitKey::of(
            &UnitRequest { shape, rule: FillRule::NonZero, params: Params::for_style(Style::Running), columns: None, from: None, clip: None },
            None,
        )
    };
    // Each unit 100 stitches and a key of about 40 words: 4 such units don't fit a limit of 320.
    let unit = || Arc::new(UnitStitches { stitches: vec![Stitch::sew(Point::ORIGIN); 100], columns: None, widths: None });
    // Each step a plan of its own (one plan's units are never dropped for each other).
    let mut cache = UnitCache::with_limit(320);
    cache.begin();
    cache.insert(key(&shapes[0]), unit());
    cache.begin();
    cache.insert(key(&shapes[1]), unit());
    cache.begin();
    assert!(cache.get(&key(&shapes[0])).is_some(), "the first is used again");
    cache.begin();
    cache.insert(key(&shapes[2]), unit());
    cache.begin();
    cache.insert(key(&shapes[3]), unit());
    assert!(cache.held() <= 320, "{}", cache.held());
    cache.begin();
    assert!(cache.get(&key(&shapes[1])).is_none(), "the one used least recently went first");
    assert!(cache.get(&key(&shapes[3])).is_some(), "the last added stays");
    // A unit larger than the whole limit is not kept, and drops nothing.
    let before = cache.units();
    cache.insert(key(&shapes[1]), Arc::new(UnitStitches { stitches: vec![Stitch::sew(Point::ORIGIN); 400], columns: None, widths: None }));
    assert_eq!(cache.units(), before);
    assert!(cache.get(&key(&shapes[1])).is_none());
}

/// A plan whose units don't all fit keeps those it planned first, never dropping one of its own for
/// another: the next plan of the same units finds those, where dropping the least recently used
/// would have dropped, unit by unit, the very one asked for next, and kept none.
#[test]
fn a_plan_keeps_its_first_units_when_not_all_fit() {
    use vectorcraft_geom::{BezPath, FillRule, Point};
    use vectorcraft_stitch::Stitch;
    use vectorcraft_stitch::plan::{Params, Style};
    let shapes: Vec<BezPath> = (0..3)
        .map(|k| {
            let mut p = BezPath::new();
            p.move_to(Point::new(f64::from(k), 0.0));
            p.line_to(Point::new(f64::from(k), 10.0));
            p
        })
        .collect();
    let key = |shape: &BezPath| {
        UnitKey::of(
            &UnitRequest { shape, rule: FillRule::NonZero, params: Params::for_style(Style::Running), columns: None, from: None, clip: None },
            None,
        )
    };
    let unit = || Arc::new(UnitStitches { stitches: vec![Stitch::sew(Point::ORIGIN); 100], columns: None, widths: None });
    // Two units of 100 stitches and their keys fit 320, three don't.
    let mut cache = UnitCache::with_limit(320);
    for _ in 0..3 {
        cache.begin();
        let found: Vec<bool> = shapes
            .iter()
            .map(|shape| {
                let kept = cache.get(&key(shape)).is_some();
                if !kept {
                    cache.insert(key(shape), unit());
                }
                kept
            })
            .collect();
        assert!(cache.held() <= 320, "{}", cache.held());
        assert_eq!(cache.units(), 2);
        if found.iter().any(|k| *k) {
            assert_eq!(found, [true, true, false], "the first two kept, plan after plan");
        }
    }
}

/// A design too large to plan is refused again at once, without planning up to the limit again;
/// a plan of it with other options is planned.
#[test]
fn a_design_too_large_is_refused_once_per_snapshot() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    // Three metres a side: more stitches than one shape may have.
    run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500}));
    let doc = s.doc().unwrap().doc.clone();
    let service = PlanService::default();
    let opts = Options::default();
    let first = service.plan(Of::whole(&doc), &doc, &opts);
    assert!(matches!(first, Err(PlanError::TooLarge(_))), "{first:?}");
    assert_eq!(service.walks(), 1);
    assert_eq!(service.plan(Of::whole(&doc), &doc, &opts), first, "the same refusal");
    assert_eq!(service.walks(), 1, "not planned again");
    let other = options_at(2);
    assert!(service.plan(Of::whole(&doc), &doc, &other).is_err());
    assert_eq!(service.walks(), 2, "other options, planned");
}

/// The areas the path operations make are kept: a plan after one object moves makes that
/// object's area again, and takes every other one's from what was kept.
#[test]
fn a_plan_after_a_nudge_makes_only_the_moved_objects_area() {
    let arts: Vec<Art> =
        (0..3).map(|k| Art { kind: 0, x: 20.0 + 70.0 * f64::from(k), y: 40.0, w: 50.0, h: 40.0, fill: Some(k as usize), stroke: None }).collect();
    let (mut s, ids) = build(&arts);
    let opts = options_at(3);
    let service = PlanService::default();
    let doc = s.doc().unwrap().doc.clone();
    service.plan(Of::whole(&doc), &doc, &opts).unwrap();
    assert_eq!(service.shapes(), 3, "each fill's grown area");
    run(&mut s, "object.move", json!({"ids": [ids[1]], "dx": 2, "dy": 0}));
    let doc = s.doc().unwrap().doc.clone();
    let kept = service.plan(Of::whole(&doc), &doc, &opts).unwrap();
    assert_eq!(service.shapes(), 4, "only the moved one's made again");
    assert_eq!(*kept, digitize(&doc, &opts, None).unwrap());
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
    let first = service.plan(Of::whole(&doc), &doc, &opts).unwrap();
    assert!(Arc::ptr_eq(&first, &service.plan(Of::whole(&doc), &doc, &opts).unwrap()), "the same snapshot, the same plan");
    assert_eq!(service.walks(), 1);
    // Fonts added since (a text clipping path is outlined with them): not the plan remembered.
    let fonts = vectorcraft_text::FontDb::global().generation();
    assert!(service.remembered(Of::whole(&doc), &opts, fonts).is_some());
    assert!(service.remembered(Of::whole(&doc), &opts, fonts.wrapping_add(1)).is_none());
    // The same art in another snapshot: walked again, every unit from what was kept.
    let kept = service.kept();
    let copy = Arc::new((*doc).clone());
    let again = service.plan(Of::whole(&copy), &copy, &opts).unwrap();
    assert!(!Arc::ptr_eq(&first, &again));
    assert_eq!((*first).clone(), (*again).clone());
    assert_eq!((service.walks(), service.kept()), (2, kept), "no unit planned anew");
    // Other options: another plan.
    let satin = Options::from_params(&json!({"stitchStyle": "satin"}), "test").unwrap();
    assert_ne!(*service.plan(Of::whole(&doc), &doc, &satin).unwrap(), *first);
    // Through the commands: three in a row walk the document once; an edit walks it again, and
    // its undo (the earlier snapshot, whose plan is still remembered) doesn't.
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
    assert_eq!(service.walks(), 2);
}

/// A service remembers its last few plans: the most recently used are kept, the least recently
/// used beyond `MAX_MEMOS` go, and so do those of snapshots no longer held.
#[test]
fn a_service_remembers_its_last_few_plans() {
    use super::service::MAX_MEMOS;
    let (s, _) = build(&[Art { kind: 0, x: 10.0, y: 10.0, w: 40.0, h: 30.0, fill: Some(0), stroke: None }]);
    let doc = s.doc().unwrap().doc.clone();
    let service = PlanService::default();
    let options: Vec<Options> = (0..=MAX_MEMOS).map(options_at).collect();
    for o in &options {
        service.plan(Of::whole(&doc), &doc, o).unwrap();
    }
    assert_eq!((service.walks(), service.memos()), (MAX_MEMOS + 1, MAX_MEMOS));
    // The newest are still remembered, the oldest is planned again.
    service.plan(Of::whole(&doc), &doc, &options[MAX_MEMOS]).unwrap();
    service.plan(Of::whole(&doc), &doc, &options[1]).unwrap();
    assert_eq!(service.walks(), MAX_MEMOS + 1);
    service.plan(Of::whole(&doc), &doc, &options[0]).unwrap();
    assert_eq!(service.walks(), MAX_MEMOS + 2);
    // A snapshot no longer held: its plans go when the next is remembered.
    let copy = Arc::new((*doc).clone());
    service.plan(Of::whole(&copy), &copy, &options[0]).unwrap();
    drop(copy);
    let other = Arc::new((*doc).clone());
    service.plan(Of::whole(&other), &other, &options[0]).unwrap();
    assert!(service.memos() <= MAX_MEMOS);
}

/// Each plan made has a generation of its own; reading a remembered plan gives the same one, and
/// its scene is built once and shared.
#[test]
fn each_plan_has_a_generation_and_its_scene_is_built_once() {
    let (s, _) = build(&[Art { kind: 0, x: 10.0, y: 10.0, w: 40.0, h: 30.0, fill: Some(0), stroke: Some((1, 4.0)) }]);
    let doc = s.doc().unwrap().doc.clone();
    let service = PlanService::default();
    let opts = Options::default();
    let first = service.planned(Of::whole(&doc), &doc, &opts).unwrap();
    assert!(first.built_scene().is_none(), "built when asked for");
    let again = service.planned(Of::whole(&doc), &doc, &opts).unwrap();
    assert_eq!(first.generation, again.generation);
    assert!(Arc::ptr_eq(&first.scene(), &again.scene()), "one scene for one plan");
    assert!(again.built_scene().is_some());
    let copy = Arc::new((*doc).clone());
    let other = service.planned(Of::whole(&copy), &copy, &opts).unwrap();
    assert!(other.generation > first.generation, "another snapshot: another plan, a later generation");
    assert!(!Arc::ptr_eq(&first.scene(), &other.scene()));
    assert!(first.plan.byte_size() > 0);
}

/// The planner's latest plan never plans; a plan key tells whether a plan is of the document as it
/// is: a selection doesn't change it, an edit and other options do.
#[test]
fn the_latest_plan_never_plans_and_a_key_follows_the_document() {
    let (mut s, ids) = build(&[Art { kind: 0, x: 10.0, y: 10.0, w: 40.0, h: 30.0, fill: Some(0), stroke: None }]);
    let opts = Options::default();
    let planner = s.embroidery_planner().unwrap();
    let uid = s.doc().unwrap().uid;
    assert!(planner.latest(&opts).is_none());
    assert_eq!(s.embroidery_plans.of(uid).walks(), 0);
    let planned = planner.planned(&opts).unwrap();
    assert_eq!(planner.latest(&opts).unwrap().unwrap().generation, planned.generation);
    let key = s.embroidery_plan_key(&opts).unwrap();
    assert_eq!(key, planner.key(&opts));
    assert!(key.is_of(&s.doc().unwrap().doc) && key.uid() == uid);
    run(&mut s, "select.set", json!({"ids": [ids[0]]}));
    assert_eq!(s.embroidery_plan_key(&opts).unwrap(), key, "a selection plans nothing new");
    assert_ne!(s.embroidery_plan_key(&options_at(2)).unwrap(), key, "other options");
    run(&mut s, "object.move", json!({"ids": [ids[0]], "dx": 5, "dy": 0}));
    assert_ne!(s.embroidery_plan_key(&opts).unwrap(), key, "an edit");
    assert_eq!(s.embroidery_plans.of(uid).walks(), 1);
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
    let fresh = digitize(&doc, &opts, None).unwrap();
    assert_eq!(fresh.units.len(), 7);
    let service = PlanService::default();
    // Cancelled before it starts: nothing planned, nothing kept.
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(cancel::scope(&token, None, || service.plan(Of::whole(&doc), &doc, &opts)), Err(PlanError::Cancelled));
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
    assert_eq!(cancel::scope(&token, Some(sink), || service.plan(Of::whole(&doc), &doc, &opts)), Err(PlanError::Cancelled));
    assert_eq!(*steps.borrow(), [(1, 7), (2, 7), (3, 7)]);
    assert_eq!(service.kept(), 2, "the units finished before the cancel");
    // Not cancelled: the fresh plan, with only the other five planned.
    let walks = service.walks();
    assert_eq!(*service.plan(Of::whole(&doc), &doc, &opts).unwrap(), fresh);
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
        std::thread::spawn(move || service.plan(Of::whole(&doc), &doc, &opts).map(|p| (*p).clone())).join().unwrap()
    };
    assert_eq!(there, digitize(&doc, &opts, None));
    let kept = service.kept();
    let copy = Arc::new((*doc).clone());
    let here = service.plan(Of::whole(&copy), &copy, &opts).unwrap();
    assert_eq!(Ok((*here).clone()), there);
    assert_eq!((service.walks(), service.kept()), (2, kept), "every unit taken from what was kept there");
}

/// The planner a session hands out plans on another thread what the commands plan, through the
/// same service: a plan made on the worker is the one the next command reads, without planning
/// again. Its options come from the commands' params, checked as they are.
#[test]
fn a_planner_plans_on_another_thread_what_the_commands_plan() {
    let (mut s, _) = build(&[
        Art { kind: 0, x: 20.0, y: 20.0, w: 60.0, h: 40.0, fill: Some(0), stroke: Some((1, 6.0)) },
        Art { kind: 1, x: 120.0, y: 40.0, w: 50.0, h: 50.0, fill: Some(2), stroke: None },
    ]);
    let opts = crate::EmbroideryOptions::from_json(&json!({"spacing": 0.5, "stitch": "tatami"})).unwrap();
    assert!(crate::EmbroideryOptions::from_json(&json!({"spacing": 0.01})).is_err());
    assert!(crate::EmbroideryOptions::from_json(&json!({"format": "pes"})).is_err());
    let planner = s.embroidery_planner().unwrap();
    let on_worker = std::thread::spawn(move || planner.plan(&opts).map(|p| (p.len(), p))).join().unwrap().unwrap();
    let opts = crate::EmbroideryOptions::from_json(&json!({"spacing": 0.5, "stitch": "tatami"})).unwrap();
    let here = s.embroidery_plan(&opts).unwrap();
    assert!(Arc::ptr_eq(&on_worker.1, &here), "the worker's plan is the one remembered");
    let uid = s.doc().unwrap().uid;
    assert_eq!(s.embroidery_plans.of(uid).walks(), 1);
    assert_eq!(run(&mut s, "embroidery.stats", json!({"spacing": 0.5, "stitch": "tatami"}))["records"], json!(on_worker.0));
    assert_eq!(s.embroidery_plans.of(uid).walks(), 1, "the command reads the same plan");
}

/// While an opacity mask is edited, the planner plans what the read commands and the export plan,
/// without the mask's editing layer, and shares the plan they remember: it is remembered by the
/// snapshot, not by the document cut from it, which is made anew each time.
#[test]
fn the_planner_plans_what_the_commands_plan_while_a_mask_is_edited() {
    let (mut s, ids) = build(&[
        Art { kind: 0, x: 20.0, y: 20.0, w: 60.0, h: 40.0, fill: Some(0), stroke: None },
        Art { kind: 0, x: 40.0, y: 30.0, w: 60.0, h: 40.0, fill: Some(2), stroke: None },
    ]);
    let opts = Options::default();
    run(&mut s, "select.all", json!({}));
    run(&mut s, "transparency.makeOpacityMask", json!({}));
    let masked = s.embroidery_plan(&opts).unwrap();
    run(&mut s, "transparency.editOpacityMask", json!({"id": ids[0]}));
    assert!(s.doc().unwrap().doc.mask_edit.is_some());
    let planner = s.embroidery_planner().unwrap();
    assert!(planner.document().mask_edit.is_none(), "the editing layer left out");
    let editing = planner.plan(&opts).unwrap();
    assert_eq!(*editing, *masked);
    let uid = s.doc().unwrap().uid;
    let walks = s.embroidery_plans.of(uid).walks();
    assert_eq!(run(&mut s, "embroidery.stats", json!({}))["records"], json!(editing.len()));
    assert!(Arc::ptr_eq(&editing, &s.embroidery_planner().unwrap().plan(&opts).unwrap()));
    assert_eq!(s.embroidery_plans.of(uid).walks(), walks, "the command and the planner read the plan remembered");
}

/// A document a batch opened and planned is gone when the batch fails, and so is its planning
/// service, with what it kept.
#[test]
fn a_document_a_failed_batch_opened_leaves_no_planning_behind() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    run(&mut s, "embroidery.stats", json!({}));
    let before: Vec<u64> = s.docs.iter().map(|d| d.uid).collect();
    assert_eq!(s.embroidery_plans.uids(), before);
    let r = s.execute(
        "command.batch",
        &json!({"commands": [
            {"command": "file.new", "params": {"width": 200, "height": 200, "artboards": 1}},
            {"command": "shape.rectangle", "params": {"x": 10, "y": 10, "width": 50, "height": 40}},
            {"command": "embroidery.stats", "params": {}},
            {"command": "no.such.command", "params": {}},
        ]}),
    );
    assert!(r.is_err());
    let after: Vec<u64> = s.docs.iter().map(|d| d.uid).collect();
    assert_eq!(after, before);
    assert_eq!(s.embroidery_plans.uids(), before, "the opened document's service is gone");
}

/// A plan cancelled from another thread, as the app cancels its planning worker, stops at the
/// next unit: a stroke being planned when the cancel comes (a satin has no check of its own) is
/// finished and kept, and the next plan is the fresh one. An export cancelled part way writes
/// nothing.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_plan_cancelled_from_another_thread_stops_at_the_next_unit() {
    use std::rc::Rc;
    use std::sync::mpsc;

    use vectorcraft_doc::cancel::{self, CancelToken, ProgressSink};

    let (s, _) = five();
    let doc = s.doc().unwrap().doc.clone();
    let opts = Options::default();
    let service = Arc::new(PlanService::default());
    let token = CancelToken::new();
    let (reached, at_second) = mpsc::channel::<()>();
    let (go, cancelled) = mpsc::channel::<()>();
    let worker = {
        let (service, doc, opts, token) = (service.clone(), doc.clone(), opts.clone(), token.clone());
        std::thread::spawn(move || {
            // As the second unit (the first object's stroke) is reported, the worker waits for the
            // cancel, then plans it.
            let sink: ProgressSink = Rc::new(move |done, _, _| {
                if done == 2 {
                    let _ = reached.send(());
                    let _ = cancelled.recv();
                }
            });
            cancel::scope(&token, Some(sink), || service.plan(Of::whole(&doc), &doc, &opts).map(|p| (*p).clone()))
        })
    };
    at_second.recv().unwrap();
    token.cancel();
    go.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), Err(PlanError::Cancelled));
    assert_eq!(service.kept(), 2, "the fill before, and the stroke under way when the cancel came");
    assert_eq!(*service.plan(Of::whole(&doc), &doc, &opts).unwrap(), digitize(&doc, &opts, None).unwrap());

    let (mut s, _) = five();
    let token = CancelToken::new();
    let sink: ProgressSink = {
        let token = token.clone();
        Rc::new(move |done, _, _| {
            if done == 3 {
                token.cancel();
            }
        })
    };
    let r = cancel::scope(&token, Some(sink), || s.execute("document.export", &json!({"format": "dst"})));
    assert!(matches!(r, Err(crate::EngineError::Cancelled)), "{r:?}");
}

/// A plan after one object moves plans again only that object's units, and the unit after them in
/// their block, which starts where they end: every other unit is taken from what was kept.
#[test]
fn an_edit_plans_again_only_what_it_changed() {
    let (mut s, ids) = five();
    let opts = Options::default();
    let service = PlanService::default();
    let doc = s.doc().unwrap().doc.clone();
    service.plan(Of::whole(&doc), &doc, &opts).unwrap();
    assert_eq!(service.kept(), 7);
    run(&mut s, "object.move", json!({"ids": [ids[1]], "dx": 3, "dy": 2}));
    let doc = s.doc().unwrap().doc.clone();
    let after = service.plan(Of::whole(&doc), &doc, &opts).unwrap();
    let moved: Vec<usize> = after.units.iter().enumerate().filter(|(_, u)| u.origin.selected().0 == ids[1]).map(|(k, _)| k).collect();
    let last = *moved.last().unwrap();
    let next = after.units.get(last + 1).is_some_and(|u| u.block == after.units[last].block);
    assert!(next, "the moved object isn't its block's last: the test needs the unit after it");
    assert_eq!(service.kept(), 7 + moved.len() + 1, "{} moved", moved.len());
    assert_eq!(*after, digitize(&doc, &opts, None).unwrap());
}

/// Two callers asking for one plan at once walk the document once and get the same plan.
#[test]
fn two_callers_at_once_plan_once() {
    let (s, _) = build(&[
        Art { kind: 0, x: 10.0, y: 10.0, w: 80.0, h: 60.0, fill: Some(0), stroke: Some((1, 4.0)) },
        Art { kind: 1, x: 120.0, y: 20.0, w: 60.0, h: 60.0, fill: Some(2), stroke: None },
    ]);
    let doc = s.doc().unwrap().doc.clone();
    let service = Arc::new(PlanService::default());
    let start = Arc::new(std::sync::Barrier::new(2));
    let callers: Vec<_> = (0..2)
        .map(|_| {
            let (service, doc, start) = (service.clone(), doc.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                service.planned(Of::whole(&doc), &doc, &Options::default()).unwrap().generation
            })
        })
        .collect();
    let generations: Vec<u64> = callers.into_iter().map(|c| c.join().unwrap()).collect();
    assert_eq!(generations[0], generations[1], "the same plan");
    assert_eq!((service.walks(), service.flights()), (1, 0));
}

/// A caller waiting for a flight that lands without a plan (a cancelled one) plans itself; a
/// cancelled waiter stops waiting; a wait past its bound plans; a flight ends when its plan panics.
#[test]
fn a_wait_for_another_callers_plan_always_ends() {
    let (s, _) = build(&[Art { kind: 0, x: 10.0, y: 10.0, w: 80.0, h: 60.0, fill: Some(0), stroke: None }]);
    let doc = s.doc().unwrap().doc.clone();
    let opts = Options::default();
    let service = Arc::new(PlanService::default());
    // Cancelled while it waits: no plan, no walk.
    service.in_flight(Of::whole(&doc), &opts, || {
        let (service, doc) = (service.clone(), doc.clone());
        let waiter = std::thread::spawn(move || {
            let token = vectorcraft_doc::cancel::CancelToken::new();
            token.cancel();
            vectorcraft_doc::cancel::scope(&token, None, || service.planned(Of::whole(&doc), &doc, &Options::default()).err())
        });
        assert_eq!(waiter.join().unwrap(), Some(PlanError::Cancelled));
    });
    assert_eq!((service.walks(), service.flights()), (0, 0));
    // Past its bound: it plans itself.
    service.in_flight(Of::whole(&doc), &opts, || {
        let planned = service.planned_waiting(Of::whole(&doc), &doc, &opts, std::time::Duration::from_millis(1));
        assert!(planned.is_ok());
    });
    assert_eq!(service.walks(), 1);
    // The flight lands without a plan (as a cancelled one does): the waiter plans.
    let other = Options::from_params(&json!({"stitchStyle": "satin"}), "test").unwrap();
    let waiter = service.in_flight(Of::whole(&doc), &other, || {
        let (service, doc, other) = (service.clone(), doc.clone(), other.clone());
        std::thread::spawn(move || service.planned(Of::whole(&doc), &doc, &other).is_ok())
    });
    assert!(waiter.join().unwrap());
    assert_eq!((service.walks(), service.flights()), (2, 0));
    // A plan that panics ends its flight.
    let panicked =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| service.in_flight(Of::whole(&doc), &opts, || panic!("a fault in planning"))));
    assert!(panicked.is_err());
    assert_eq!(service.flights(), 0);
}
