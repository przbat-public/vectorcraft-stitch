//! The embroidery tool with a fake plan: its handles (where they stand, which objects and parts
//! have them), catching them at any zoom, dragging one (the threshold, the previews, one undo step,
//! Escape), the keys, the cursor, its options, and selecting by a press elsewhere.

use std::collections::HashMap;

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartValue};
use vectorcraft_doc::{Appearance, Document, Node, NodeId, NodeKind, Selection};
use vectorcraft_geom::{PathData, Point, SubPath};

use super::*;
use crate::testutil::cx;
use crate::{Action, Cursor, Mods, PaintDefaults, PointerEvent, PointerKind, Tool, ToolKey};

/// A plan the tests make up: each part's first and last needle points, and whether it is of the
/// object as it is.
#[derive(Default)]
struct Fake {
    ends: HashMap<(NodeId, PartKind), PlannedEnds>,
    under: Option<NodeId>,
}

impl PlanFacts for Fake {
    fn generation(&self) -> u64 {
        1
    }
    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds> {
        self.ends.get(&(object, part)).copied()
    }
    fn object_at(&self, _at: Point, _tol: f64) -> Option<NodeId> {
        self.under
    }
}

/// A document with a filled square (100 pt, at 100, 100), a stroked line (200, 300 to 400, 300)
/// and a guide (which takes no settings) → the document and the three ids.
fn doc() -> (Document, [NodeId; 3]) {
    let mut d = Document::new(600.0, 600.0);
    let l = d.layers[0].id;
    let square = d.alloc_id();
    let path = PathData::single(SubPath::polyline(
        &[Point::new(100.0, 100.0), Point::new(200.0, 100.0), Point::new(200.0, 200.0), Point::new(100.0, 200.0)],
        true,
    ));
    d.insert(Some(l), 0, Node::path(square, path, Appearance::default_art())).unwrap();
    let line = d.alloc_id();
    let path = PathData::single(SubPath::polyline(&[Point::new(200.0, 300.0), Point::new(400.0, 300.0)], false));
    let mut ap = Appearance::default_art();
    ap.items.retain(|i| matches!(i, vectorcraft_doc::AppearanceItem::Stroke(_)));
    d.insert(Some(l), 1, Node::path(line, path, ap)).unwrap();
    let guide = d.alloc_id();
    let path = PathData::single(SubPath::polyline(&[Point::new(500.0, 0.0), Point::new(500.0, 600.0)], false));
    let mut node = Node::path(guide, path, Appearance::default_art());
    if let NodeKind::Path { guide, .. } = &mut node.kind {
        *guide = true;
    }
    d.insert(Some(l), 2, node).unwrap();
    (d, [square, line, guide])
}

/// The plan of [`doc`]: the square's fill sewn from (100, 100) to (200, 200), its stroke from
/// (100, 100) round to it, the line from (200, 300) to (400, 300).
fn plan([square, line, _]: [NodeId; 3]) -> Fake {
    let e = |a: (f64, f64), b: (f64, f64)| PlannedEnds {
        first: Point::new(a.0, a.1),
        last: Point::new(b.0, b.1),
        current: true,
        before: None,
        after: None,
        trim: None,
    };
    let mut f = Fake::default();
    f.ends.insert((square, PartKind::Fill), e((100.0, 100.0), (200.0, 200.0)));
    f.ends.insert((square, PartKind::Stroke), e((100.0, 100.0), (100.0, 100.0)));
    f.ends.insert((line, PartKind::Stroke), e((200.0, 300.0), (400.0, 300.0)));
    f
}

fn select(ids: &[NodeId]) -> Selection {
    Selection { objects: ids.to_vec(), ..Default::default() }
}

fn set_point(d: &mut Document, id: NodeId, part: PartKind, key: PartKey, at: (f64, f64)) {
    let n = d.node_mut(id).unwrap();
    let mut e = n.extras().embroidery.as_deref().cloned().unwrap_or_else(Embroidery::default);
    e.edit_part(part, |p| {
        p.set(key, Some(PartValue::Point(Point::new(at.0, at.1))));
    });
    n.extras_mut().embroidery = Some(Box::new(e));
}

fn ev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent::new(kind, x, y)
}

fn at(h: &Handle) -> (f64, f64) {
    (h.point.x, h.point.y)
}

/// A point set stands where it is; one left to the planner where the plan sews it; without a plan,
/// an unset point has no handle; a plan of the object as it was shows stale.
#[test]
fn handles_stand_at_the_point_set_else_where_the_plan_sews_it() {
    let (mut d, ids) = doc();
    let [square, ..] = ids;
    set_point(&mut d, square, PartKind::Fill, PartKey::Entry, (150.0, 100.0));
    let (s, p, mut f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let hs = handles(&c, None);
    assert_eq!(
        hs.iter().map(|h| (h.id.kind, h.state, at(h))).collect::<Vec<_>>(),
        vec![(Kind::Entry, State::Set, (150.0, 100.0)), (Kind::Exit, State::Auto, (200.0, 200.0))]
    );
    let bare = cx(&d, &s, &p);
    assert_eq!(handles(&bare, None).iter().map(|h| h.id.kind).collect::<Vec<_>>(), vec![Kind::Entry], "no plan: the point set alone");
    f.ends.get_mut(&(square, PartKind::Fill)).unwrap().current = false;
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(handles(&c, None)[1].state, State::Stale);
}

/// One part at a time: the one asked for (else the fill), an object without it showing its other
/// part, as the Object tab does; a guide has none.
#[test]
fn one_part_at_a_time() {
    let (d, ids) = doc();
    let [square, line, text] = ids;
    let (s, p, f) = (select(&[square, line, text]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let parts = |part| handles(&c, part).iter().map(|h| h.id.owner.part()).collect::<Vec<_>>();
    assert_eq!(parts(None), vec![(square, PartKind::Fill), (square, PartKind::Fill), (line, PartKind::Stroke), (line, PartKind::Stroke)]);
    assert_eq!(parts(Some(PartKind::Fill)), parts(None));
    assert_eq!(
        parts(Some(PartKind::Stroke)),
        vec![(square, PartKind::Stroke), (square, PartKind::Stroke), (line, PartKind::Stroke), (line, PartKind::Stroke)]
    );
}

/// A group stands for its objects, a compound path's member for the compound path, and at most
/// HANDLED_OBJECTS_MAX objects show their points.
#[test]
fn groups_compounds_and_the_cap() {
    let mut d = Document::new(2000.0, 2000.0);
    let l = d.layers[0].id;
    let mut f = Fake::default();
    let mut squares = Vec::new();
    for k in 0..40 {
        let id = d.alloc_id();
        let x = 10.0 + 40.0 * k as f64;
        let path = PathData::single(SubPath::polyline(&[Point::new(x, 10.0), Point::new(x + 20.0, 10.0), Point::new(x + 20.0, 30.0)], true));
        squares.push(std::sync::Arc::new(Node::path(id, path, Appearance::default_art())));
        let ends = PlannedEnds { first: Point::new(x, 10.0), last: Point::new(x + 20.0, 30.0), current: true, before: None, after: None, trim: None };
        f.ends.insert((id, PartKind::Fill), ends);
    }
    let group = d.alloc_id();
    d.insert(Some(l), 0, Node::group(group, squares)).unwrap();
    let (s, p) = (select(&[group]), PaintDefaults::default());
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let objects: std::collections::BTreeSet<NodeId> = handles(&c, None).iter().map(|h| h.id.owner.part().0).collect();
    assert_eq!(objects.len(), HANDLED_OBJECTS_MAX);
    // A compound path's member stands for the compound path.
    let (member, compound) = (d.alloc_id(), d.alloc_id());
    let path = PathData::single(SubPath::polyline(&[Point::new(10.0, 100.0), Point::new(30.0, 100.0), Point::new(30.0, 120.0)], true));
    let kind =
        NodeKind::Compound { children: vec![std::sync::Arc::new(Node::path(member, path, Appearance::default_art()))], rule: Default::default() };
    let mut node = Node::new(compound, kind);
    node.appearance = Appearance::default_art().into();
    d.insert(Some(l), 1, node).unwrap();
    let ends = PlannedEnds { first: Point::new(10.0, 100.0), last: Point::new(30.0, 120.0), current: true, before: None, after: None, trim: None };
    f.ends.insert((compound, PartKind::Fill), ends);
    let s = select(&[member]);
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(handles(&c, None).iter().map(|h| h.id.owner.part().0).collect::<Vec<_>>(), vec![compound, compound]);
}

/// In a view turned a quarter, an exit under its entry is moved apart along the screen's right,
/// and a handle is caught by screen pixels all the same.
#[test]
fn a_turned_view() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    // One screen pixel right is a point down the page; one down, a point to the left.
    let screen = crate::ScreenFrame {
        origin: Point::ZERO,
        right: vectorcraft_geom::Vec2::new(0.0, 0.5),
        down: vectorcraft_geom::Vec2::new(-0.5, 0.0),
        size: (800.0, 600.0),
    };
    let c = ToolContext { embroidery: Some(&f), zoom: 2.0, screen: Some(screen), ..cx(&d, &s, &p) };
    let hs = handles(&c, Some(PartKind::Stroke));
    assert!((hs[1].at - Point::new(100.0, 100.0 + APART_PX * 0.5)).hypot() < 1e-9, "{:?}", hs[1].at);
    assert_eq!(hit(&c, &hs, hs[1].at).map(|h| h.id.kind), Some(Kind::Exit));
}

/// A handle is caught within half its size and 2 px on screen, whatever the zoom; an exit lying
/// under its entry is moved apart, and both are caught.
#[test]
fn handles_are_caught_in_screen_pixels_and_apart_when_coincident() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    for zoom in [0.25, 1.0, 8.0] {
        let c = ToolContext { embroidery: Some(&f), zoom, ..cx(&d, &s, &p) };
        let hs = handles(&c, Some(PartKind::Fill));
        let reach = (ENTRY_MARKER_PX / 2.0 + CATCH_PAST_PX - 0.5) / zoom;
        assert_eq!(hit(&c, &hs, Point::new(100.0 + reach, 100.0)).map(|h| h.id.kind), Some(Kind::Entry), "{zoom}×");
        assert_eq!(hit(&c, &hs, Point::new(100.0 + 3.0 * reach, 100.0)), None, "{zoom}×");
        // The stroke's ends lie on one another: its exit is moved apart.
        let hs = handles(&c, Some(PartKind::Stroke));
        let (entry, exit) = (hs[0], hs[1]);
        assert_eq!((entry.point, exit.point), (Point::new(100.0, 100.0), Point::new(100.0, 100.0)));
        assert!((exit.at.x - 100.0 - APART_PX / zoom).abs() < 1e-9, "{zoom}×: {:?}", exit.at);
        assert_eq!(hit(&c, &hs, entry.at).map(|h| h.id.kind), Some(Kind::Entry));
        assert_eq!(hit(&c, &hs, exit.at).map(|h| h.id.kind), Some(Kind::Exit));
    }
}

/// A press on a handle and a drag past 3 px begin one undo step; each move previews the point
/// (snapped by the engine), none where it wouldn't change; the release keeps it.
#[test]
fn a_drag_previews_the_point_and_the_release_keeps_it() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    assert!(t.pointer(&c, &ev(PointerKind::Down, 101.0, 101.0)).is_empty());
    assert!(t.pointer(&c, &ev(PointerKind::Drag, 102.0, 102.0)).is_empty(), "within 3 px");
    let acts = t.pointer(&c, &ev(PointerKind::Drag, 150.0, 104.0));
    assert_eq!(
        acts,
        vec![
            Action::Begin("Embroidery Entry and Exit".into()),
            Action::Preview("embroidery.setEntryExit".into(), json!({"id": square.0, "part": "fill", "entry": [150.0, 104.0], "snap": true}))
        ]
    );
    assert!(t.busy());
    assert!(t.pointer(&c, &ev(PointerKind::Drag, 150.0, 104.0)).is_empty(), "no preview where it wouldn't change");
    assert_eq!(t.options()["drag"], json!({"kind": "entry", "id": square.0, "part": "fill", "index": 0, "at": [150.0, 104.0]}));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 150.0, 104.0)), vec![Action::Commit]);
    assert!(!t.busy());
}

/// A press without a drag changes nothing (the handle is focused for the keys); Escape mid-drag
/// cancels it, and Escape again lets the handle go; switching tools mid-drag keeps the drag.
#[test]
fn a_click_escape_and_switching_away() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 200.0, 200.0));
    assert!(t.pointer(&c, &ev(PointerKind::Up, 200.0, 200.0)).is_empty());
    assert_eq!(t.options()["focus"]["kind"], json!("exit"));
    t.pointer(&c, &ev(PointerKind::Down, 200.0, 200.0));
    t.pointer(&c, &ev(PointerKind::Drag, 220.0, 220.0));
    assert!(t.claims_key(&c, ToolKey::Escape));
    assert_eq!(t.key(&c, ToolKey::Escape, Mods::default()), vec![Action::Cancel]);
    assert!(t.claims_key(&c, ToolKey::Escape), "the handle still focused");
    t.key(&c, ToolKey::Escape, Mods::default());
    assert!(!t.claims_key(&c, ToolKey::Escape));
    t.pointer(&c, &ev(PointerKind::Down, 200.0, 200.0));
    t.pointer(&c, &ev(PointerKind::Drag, 220.0, 220.0));
    assert_eq!(t.deactivate(&c), vec![Action::Commit], "switching away keeps the drag");
}

/// Alt-click or Delete puts a point back to Auto; the arrows nudge the focused one by the
/// keyboard increment (Shift ten times); `[` and `]` step between the handles.
#[test]
fn the_keys() {
    let (d, ids) = doc();
    let [square, line, _] = ids;
    let (s, p, f) = (select(&[square, line]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), keyboard_increment: 2.0, ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    let alt = Mods { alt: true, ..Mods::default() };
    let acts = t.pointer(&c, &ev(PointerKind::Down, 400.0, 300.0).with_mods(alt));
    assert_eq!(acts, vec![Action::Exec("embroidery.setEntryExit".into(), json!({"id": line.0, "part": "stroke", "exit": null, "snap": true}))]);
    assert_eq!(t.key(&c, ToolKey::Delete, Mods::default()), acts, "Delete as the Alt-click");
    let right = t.key(&c, ToolKey::Right, Mods::default());
    assert_eq!(
        right,
        vec![Action::Exec("embroidery.setEntryExit".into(), json!({"id": line.0, "part": "stroke", "exit": [402.0, 300.0], "snap": true}))]
    );
    let up = t.key(&c, ToolKey::Up, Mods { shift: true, ..Mods::default() });
    assert_eq!(
        up,
        vec![Action::Exec("embroidery.setEntryExit".into(), json!({"id": line.0, "part": "stroke", "exit": [400.0, 280.0], "snap": true}))]
    );
    // `]` from the line's exit, the last, wraps to the square's entry; `[` goes back.
    t.key(&c, ToolKey::BracketRight, Mods::default());
    assert_eq!(t.options()["focus"], json!({"kind": "entry", "id": square.0, "part": "fill", "index": 0}));
    t.key(&c, ToolKey::BracketLeft, Mods::default());
    assert_eq!(t.options()["focus"]["id"], json!(line.0));
    assert!(t.claims_key(&c, ToolKey::Left) && t.claims_key(&c, ToolKey::BracketRight) && t.claims_key(&c, ToolKey::Enter));
    // Enter asks for the focused point's X and Y; so does a double-click on a point.
    let dialog = |kind: &str, id: NodeId, part: &str, at: [f64; 2], set: bool| {
        vec![Action::Dialog("embroideryPoint".into(), json!({"kind": kind, "id": id.0, "part": part, "index": 0, "at": at, "set": set}))]
    };
    assert_eq!(t.key(&c, ToolKey::Enter, Mods::default()), dialog("exit", line, "stroke", [400.0, 300.0], false));
    assert_eq!(t.pointer(&c, &ev(PointerKind::DoubleClick, 100.0, 100.0)), dialog("entry", square, "fill", [100.0, 100.0], false));
    assert!(t.pointer(&c, &ev(PointerKind::DoubleClick, 300.0, 200.0)).is_empty(), "away from the points");
    t.key(&c, ToolKey::Escape, Mods::default());
    assert!(!t.claims_key(&c, ToolKey::Enter), "no point focused");
}

/// The cursor: over a handle and while dragging, the point's; elsewhere, the tool's.
#[test]
fn the_cursor() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    assert_eq!(t.cursor(&c, Point::new(100.0, 100.0), Mods::default()), Cursor::EmbroideryPoint);
    assert_eq!(t.cursor(&c, Point::new(300.0, 100.0), Mods::default()), Cursor::Embroidery);
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 300.0, 100.0));
    assert_eq!(t.cursor(&c, Point::new(300.0, 100.0), Mods::default()), Cursor::EmbroideryPoint, "while dragging");
}

/// A press away from the handles selects the object under it (the plan's answer first, the art
/// otherwise), Shift adds it or takes it away, and a press on nothing empties the selection; what
/// takes no settings isn't picked.
#[test]
fn a_press_elsewhere_selects() {
    let (d, ids) = doc();
    let [square, line, guide] = ids;
    let (s, p) = (select(&[]), PaintDefaults::default());
    let c = cx(&d, &s, &p);
    let mut t = EmbroideryTool::default();
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 150.0, 150.0)), vec![Action::Exec("select.set".into(), json!({"ids": [square.0]}))]);
    let shift = Mods { shift: true, ..Mods::default() };
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 300.0, 300.0).with_mods(shift)),
        vec![Action::Exec("select.toggle".into(), json!({"ids": [line.0]}))]
    );
    let s = select(&[square]);
    let c = cx(&d, &s, &p);
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)), vec![Action::Exec("select.set".into(), json!({"ids": []}))]);
    let f = Fake { under: Some(line), ..Fake::default() };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    t.pointer(&c, &ev(PointerKind::Move, 550.0, 50.0));
    assert_eq!(t.options()["hoverObject"], json!(line.0), "hovering its stitches");
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)),
        vec![Action::Exec("select.set".into(), json!({"ids": [line.0]}))],
        "by its stitches"
    );
    let f = Fake { under: Some(guide), ..Fake::default() };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)),
        vec![Action::Exec("select.set".into(), json!({"ids": []}))],
        "a guide takes no points"
    );
}

/// A point dragged past the coordinates a document holds is kept within them.
#[test]
fn a_point_past_the_document_is_kept_within_it() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    let acts = t.pointer(&c, &ev(PointerKind::Drag, 1e12, -1e12));
    let Some(Action::Preview(_, v)) = acts.last() else { panic!("{acts:?}") };
    assert_eq!(v["entry"], json!([vectorcraft_doc::MAX_COORD, -vectorcraft_doc::MAX_COORD]));
    assert!(t.pointer(&c, &ev(PointerKind::Drag, f64::NAN, 3.0)).is_empty(), "a point that isn't a number moves nothing");
}

/// The part option: fill, stroke, anything else back to each object's own; its options say which.
#[test]
fn the_part_option() {
    let mut t = EmbroideryTool::default();
    assert_eq!(t.options()["part"], Value::Null);
    t.set_option("part", &json!("stroke"));
    assert_eq!(t.options()["part"], json!("stroke"));
    t.set_option("part", &json!(5));
    assert_eq!(t.options()["part"], Value::Null);
    assert_eq!(super::create(ID).map(|t| t.id()), Some(ID));
}

/// Hostile values for any option, then events at hostile points (not numbers, past any document,
/// on a handle), never panic.
#[test]
fn hostile_options_and_points_never_panic() {
    let (d, ids) = doc();
    let (sel, p, f) = (select(&ids), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &sel, &p) };
    let numbers = [0.0, -0.0, 1e308, -1e308, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE, 100.0];
    let values: Vec<Value> = numbers
        .iter()
        .flat_map(|n| [json!(n), json!([n, n]), json!({"kind": "entry", "id": n, "part": "fill"})])
        .chain([json!("fill"), json!("x".repeat(1000)), json!(null), json!([]), json!({})])
        .collect();
    let mut t = EmbroideryTool::default();
    for key in ["part", "place", "", "drag"] {
        for v in &values {
            t.set_option(key, v);
        }
    }
    for &x in &numbers {
        for kind in [PointerKind::Move, PointerKind::Down, PointerKind::Drag, PointerKind::Drag, PointerKind::Up, PointerKind::DoubleClick] {
            t.pointer(&c, &ev(kind, x, 100.0));
            t.pointer(&c, &ev(kind, 100.0, x));
        }
        for key in [ToolKey::Left, ToolKey::Delete, ToolKey::BracketRight, ToolKey::Up, ToolKey::Escape] {
            t.key(&c, key, Mods { shift: true, ..Mods::default() });
        }
        let _ = (t.options(), t.cursor(&c, Point::new(x, x), Mods::default()));
    }
}
