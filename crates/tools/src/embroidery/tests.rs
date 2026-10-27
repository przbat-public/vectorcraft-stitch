//! The embroidery tool with a fake plan: its handles (where they stand, which objects and parts
//! have them), catching them at any zoom, dragging one (the threshold, the previews, one undo step,
//! Escape), the keys, the cursor, its options, selecting by a press elsewhere (and picking the part
//! pressed), and a tatami fill's
//! angle arrow (where it points, turning it, Shift's steps, resetting it, its keys).

use std::collections::HashMap;

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartValue};
use vectorcraft_doc::{Appearance, Document, Node, NodeId, NodeKind, Selection};
use vectorcraft_geom::{PathData, Point, SubPath};

use super::*;
use crate::testutil::cx;
use crate::{Action, Cursor, Mods, PaintDefaults, PointerEvent, PointerKind, Tool, ToolKey};

/// A plan the tests make up: each part's first and last needle points, and whether it is of the
/// object as it is; and the fills sewn as tatami, with their rows' angles. Each made up is a plan of
/// its own generation, as a new plan is (the tool keeps its handles by it).
struct Fake {
    ends: HashMap<(NodeId, PartKind), PlannedEnds>,
    under: Option<(NodeId, Option<PartKind>)>,
    rows: HashMap<NodeId, FillAngle>,
    generation: u64,
    /// Where every point given is stored (an open line's nearer end), when set; else as given.
    stored: Option<Point>,
}

impl Default for Fake {
    fn default() -> Self {
        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            ends: HashMap::new(),
            under: None,
            rows: HashMap::new(),
            generation: MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            stored: None,
        }
    }
}

impl PlanFacts for Fake {
    fn generation(&self) -> u64 {
        self.generation
    }
    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds> {
        self.ends.get(&(object, part)).copied()
    }
    fn part_at(&self, _at: Point, _tol: f64) -> Option<(NodeId, Option<PartKind>)> {
        self.under
    }
    fn fill_angle(&self, object: NodeId) -> Option<FillAngle> {
        self.rows.get(&object).copied()
    }
    fn snap(&self, _object: NodeId, _part: PartKind, at: Point) -> Option<Point> {
        Some(self.stored.unwrap_or(at))
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

/// A handle is caught within half its size and 2 px on screen, and at least 12 px from its centre
/// (a 24 px target), whatever the zoom; an exit lying under its entry is moved apart, and both are
/// caught.
#[test]
fn handles_are_caught_in_screen_pixels_and_apart_when_coincident() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    for zoom in [0.25, 1.0, 8.0] {
        let c = ToolContext { embroidery: Some(&f), zoom, ..cx(&d, &s, &p) };
        let hs = handles(&c, Some(PartKind::Fill));
        let reach = ((ENTRY_MARKER_PX / 2.0 + CATCH_PAST_PX).max(CATCH_MIN_PX) - 0.5) / zoom;
        assert!(reach * zoom >= CATCH_MIN_PX - 0.5, "a 24 px target at least");
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

/// A drag previews only where the point stored would change: along an open line it stays at the
/// line's nearer end, so moves that keep it there send nothing more, and the release keeps the one
/// previewed.
#[test]
fn a_drag_previews_only_where_the_point_stored_moves() {
    let (d, ids) = doc();
    let [_, line, _] = ids;
    let (s, p) = (select(&[line]), PaintDefaults::default());
    let f = Fake { stored: Some(Point::new(200.0, 300.0)), ..plan(ids) };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    assert!(t.pointer(&c, &ev(PointerKind::Down, 200.0, 300.0)).is_empty());
    let first = t.pointer(&c, &ev(PointerKind::Drag, 230.0, 300.0));
    assert!(matches!(first.as_slice(), [Action::Begin(_), Action::Preview(..)]), "{first:?}");
    for x in [260.0, 290.0, 320.0] {
        assert!(t.pointer(&c, &ev(PointerKind::Drag, x, 300.0)).is_empty(), "stored where it was: {x}");
    }
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 320.0, 300.0)), vec![Action::Commit]);
}

/// A press on a handle and a drag past 3 px begin one undo step; each move previews the point
/// (snapped by the engine), moved by the drag from where it was (pressed a pixel off it, the point
/// follows the pointer that pixel off), none where it wouldn't change; the release keeps it.
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
            Action::Preview("embroidery.setEntryExit".into(), json!({"id": square.0, "part": "fill", "entry": [149.0, 103.0], "snap": true}))
        ]
    );
    assert!(t.busy());
    assert!(t.pointer(&c, &ev(PointerKind::Drag, 150.0, 104.0)).is_empty(), "no preview where it wouldn't change");
    assert_eq!(t.options()["drag"], json!({"kind": "entry", "id": square.0, "part": "fill", "index": 0, "at": [149.0, 103.0]}));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 150.0, 104.0)), vec![Action::Commit]);
    assert!(!t.busy());
}

/// An exit drawn apart from the entry it lies under (a closed stroke's) is grabbed where it is
/// drawn: a small drag moves its point by that much, not to the pointer, so it doesn't jump by the
/// gap.
#[test]
fn an_exit_drawn_apart_moves_by_the_drag() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool { part: Some(PartKind::Stroke), ..EmbroideryTool::default() };
    let exit = handles(&c, Some(PartKind::Stroke)).into_iter().find(|h| h.id.kind == Kind::Exit).unwrap();
    assert!(exit.at != exit.point, "drawn apart");
    t.pointer(&c, &ev(PointerKind::Down, exit.at.x, exit.at.y));
    let acts = t.pointer(&c, &ev(PointerKind::Drag, exit.at.x + 5.0, exit.at.y));
    let point = acts.iter().find_map(|a| match a {
        Action::Preview(_, p) => Some((p["exit"][0].as_f64().unwrap(), p["exit"][1].as_f64().unwrap())),
        _ => None,
    });
    assert_eq!(point, Some((exit.point.x + 5.0, exit.point.y)), "its point moved by the drag");
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
    // A press with a drag under way (no release between, as a script may send) ends that drag
    // first; a double-click during a drag changes nothing.
    t.pointer(&c, &ev(PointerKind::Down, 200.0, 200.0));
    t.pointer(&c, &ev(PointerKind::Drag, 220.0, 220.0));
    assert!(t.pointer(&c, &ev(PointerKind::DoubleClick, 220.0, 220.0)).is_empty() && t.busy());
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0)), vec![Action::Commit]);
    assert!(!t.busy());
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
/// otherwise) and shows the part pressed, its fill or its stroke; Shift adds it or takes it away,
/// the part left as it is, and a press on nothing empties the selection; what takes no settings
/// isn't picked.
#[test]
fn a_press_elsewhere_selects() {
    let (d, ids) = doc();
    let [square, line, guide] = ids;
    let (s, p) = (select(&[]), PaintDefaults::default());
    let c = cx(&d, &s, &p);
    let mut t = EmbroideryTool::default();
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 150.0, 150.0)), vec![Action::Exec("select.set".into(), json!({"ids": [square.0]}))]);
    assert_eq!(t.options()["part"], json!("fill"), "its fill pressed, its fill shown");
    let shift = Mods { shift: true, ..Mods::default() };
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 300.0, 300.0).with_mods(shift)),
        vec![Action::Exec("select.toggle".into(), json!({"ids": [line.0]}))]
    );
    assert_eq!(t.options()["part"], json!("fill"), "Shift adds the object, the part stays");
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 300.0, 300.0)), vec![Action::Exec("select.set".into(), json!({"ids": [line.0]}))]);
    assert_eq!(t.options()["part"], json!("stroke"), "its stroke pressed, its stroke shown");
    let s = select(&[square]);
    let c = cx(&d, &s, &p);
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)), vec![Action::Exec("select.set".into(), json!({"ids": []}))]);
    let f = Fake { under: Some((line, Some(PartKind::Stroke))), ..Fake::default() };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    t.pointer(&c, &ev(PointerKind::Move, 550.0, 50.0));
    assert_eq!(t.options()["hoverObject"], json!(line.0), "hovering its stitches");
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)),
        vec![Action::Exec("select.set".into(), json!({"ids": [line.0]}))],
        "by its stitches"
    );
    let f = Fake { under: Some((square, Some(PartKind::Fill))), ..Fake::default() };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0));
    assert_eq!(t.options()["part"], json!("fill"), "the part whose stitches are pressed shown");
    let f = Fake { under: Some((guide, Some(PartKind::Fill))), ..Fake::default() };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 550.0, 50.0)),
        vec![Action::Exec("select.set".into(), json!({"ids": []}))],
        "a guide takes no points"
    );
}

/// A press on stitches picks only what the art's hit test would pick: not an object locked or hidden
/// since the plan drawn was made, nor one outside the object isolated (the stitches drawn can be of
/// a plan made before); nothing is under the press then, and the selection empties.
#[test]
fn a_press_on_stitches_picks_only_what_the_art_would() {
    let (d, ids) = doc();
    let [square, line, _] = ids;
    let (s, p) = (select(&[square]), PaintDefaults::default());
    let f = Fake { under: Some((line, Some(PartKind::Stroke))), ..Fake::default() };
    let press = |d: &Document, isolation: Option<NodeId>| {
        let c = ToolContext { embroidery: Some(&f), isolation, ..cx(d, &s, &p) };
        EmbroideryTool::default().pointer(&c, &ev(PointerKind::Down, 550.0, 50.0))
    };
    let picked = vec![Action::Exec("select.set".into(), json!({"ids": [line.0]}))];
    let nothing = vec![Action::Exec("select.set".into(), json!({"ids": []}))];
    assert_eq!(press(&d, None), picked, "as it is");
    assert_eq!(press(&d, Some(square)), nothing, "outside the object isolated");
    for edit in [|n: &mut Node| n.locked = true, |n: &mut Node| n.visible = false] {
        let mut d = d.clone();
        edit(d.node_mut(line).unwrap());
        assert_eq!(press(&d, None), nothing, "locked or hidden since the plan was made");
    }
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

/// [`plan`] with the square's fill sewn as tatami, its rows at `degrees`, set or Auto.
fn with_rows(ids: [NodeId; 3], degrees: f64, set: bool) -> Fake {
    let mut f = plan(ids);
    f.rows.insert(ids[0], FillAngle { degrees, set });
    f
}

/// The square's angle handle.
fn arrow_of(c: &ToolContext) -> Handle {
    handles(c, None).into_iter().find(|h| h.id.kind == Kind::Angle).expect("the square's arrow")
}

/// A tatami fill's rows show as an arrow through its centre (150, 150), its head 40 px out on
/// screen at its angle, counter-clockwise on the page (135°: up and to the left), at any zoom; set
/// or Auto as the engine says; after the fill's entry and exit; none for its stroke, nor for a
/// fill sewn otherwise.
#[test]
fn a_tatami_fills_rows_are_an_arrow_through_its_centre() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p) = (select(&[square]), PaintDefaults::default());
    let f = with_rows(ids, 135.0, false);
    for zoom in [1.0, 2.0] {
        let c = ToolContext { embroidery: Some(&f), zoom, ..cx(&d, &s, &p) };
        let hs = handles(&c, None);
        assert_eq!(hs.iter().map(|h| h.id.kind).collect::<Vec<_>>(), vec![Kind::Entry, Kind::Exit, Kind::Angle]);
        let a = arrow_of(&c);
        let out = ARROW_REACH_PX / zoom / std::f64::consts::SQRT_2;
        assert!(a.point.distance(Point::new(150.0 - out, 150.0 - out)) < 1e-9, "{zoom}: {:?}", a.point);
        assert_eq!((a.state, a.arrow.map(|r| (r.centre, r.degrees))), (State::Auto, Some((Point::new(150.0, 150.0), 135.0))));
    }
    let f = with_rows(ids, 30.0, true);
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(arrow_of(&c).state, State::Set);
    assert!(handles(&c, Some(PartKind::Stroke)).iter().all(|h| h.id.kind != Kind::Angle), "the stroke has no rows");
    let plain = plan(ids);
    let c = ToolContext { embroidery: Some(&plain), ..cx(&d, &s, &p) };
    assert!(handles(&c, None).iter().all(|h| h.id.kind != Kind::Angle), "a fill sewn otherwise");
}

/// Dragging the head turns the rows about the centre, counter-clockwise: straight up is 90°, to
/// the right 0°, each move previewing `embroidery.set {ids, part, angle}` under the command's own
/// undo label, the head kept 40 px out, the angle in the options; Shift steps 45° from the
/// Constrain Angle; the release keeps it.
#[test]
fn dragging_the_arrows_head_turns_the_rows() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), with_rows(ids, 135.0, false));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let head = arrow_of(&c).point;
    let mut t = EmbroideryTool::default();
    assert_eq!(t.cursor(&c, head, Mods::default()), Cursor::Rotate, "over the head");
    t.pointer(&c, &ev(PointerKind::Down, head.x, head.y));
    let set = |angle: f64| Action::Preview("embroidery.set".into(), json!({"ids": [square.0], "part": "fill", "angle": angle}));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Drag, 150.0, 60.0)), vec![Action::Begin("Embroidery Object Settings".into()), set(90.0)]);
    assert!((t.options()["drag"]["at"][1].as_f64().unwrap() - (150.0 - ARROW_REACH_PX)).abs() < 1e-9, "the head 40 px out");
    assert_eq!(t.options()["drag"]["angle"], json!(90.0));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Drag, 250.0, 150.0)), vec![set(0.0)]);
    let shift = |x: f64, y: f64| ev(PointerKind::Drag, x, y).with_mods(Mods { shift: true, ..Mods::default() });
    assert_eq!(t.pointer(&c, &shift(200.0, 110.0)), vec![set(45.0)], "Shift: 45° steps");
    let turned = ToolContext { embroidery: Some(&f), constrain_angle: 30.0, ..cx(&d, &s, &p) };
    assert_eq!(t.pointer(&turned, &shift(250.0, 130.0)), vec![set(30.0)], "from the Constrain Angle");
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 250.0, 130.0)), vec![Action::Commit]);
}

/// The arrow's keys: Alt-click or Delete puts the rows back to Auto (`embroidery.reset {keys:
/// [angle]}`); → and ↑ turn it a degree counter-clockwise, ← and ↓ clockwise, Shift ten; Enter
/// and a double-click ask for no dialog (an angle is typed in the Object tab).
#[test]
fn the_arrows_keys_and_its_reset() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), with_rows(ids, 135.0, true));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let head = arrow_of(&c).point;
    let reset = Action::Exec("embroidery.reset".into(), json!({"ids": [square.0], "part": "fill", "keys": ["angle"]}));
    let mut t = EmbroideryTool::default();
    let alt = ev(PointerKind::Down, head.x, head.y).with_mods(Mods { alt: true, ..Mods::default() });
    assert_eq!(t.pointer(&c, &alt), vec![reset.clone()]);
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, head.x, head.y)), vec![]);
    let set = |angle: f64| vec![Action::Exec("embroidery.set".into(), json!({"ids": [square.0], "part": "fill", "angle": angle}))];
    let shift = Mods { shift: true, ..Mods::default() };
    assert_eq!(t.key(&c, ToolKey::Right, Mods::default()), set(136.0));
    assert_eq!(t.key(&c, ToolKey::Up, Mods::default()), set(136.0));
    assert_eq!(t.key(&c, ToolKey::Down, shift), set(125.0));
    assert_eq!(t.key(&c, ToolKey::Left, Mods::default()), set(134.0));
    assert_eq!(t.key(&c, ToolKey::Delete, Mods::default()), vec![reset]);
    assert!(!t.claims_key(&c, ToolKey::Enter), "Enter is left to the window");
    assert_eq!(t.key(&c, ToolKey::Enter, Mods::default()), vec![]);
    assert_eq!(t.pointer(&c, &ev(PointerKind::DoubleClick, head.x, head.y)), vec![]);
    // Across the half turn: 180° and −180° are the same rows, written (−180, 180].
    let f = with_rows(ids, 179.5, true);
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(t.key(&c, ToolKey::Right, Mods::default()), set(-179.5));
}

/// Handles lying on one another (two objects' Auto entries at one point) are taken in turn: each
/// press there focuses the one after the focused, round to the first.
#[test]
fn coincident_handles_are_taken_in_turn() {
    let (d, ids) = doc();
    let [square, line, _] = ids;
    let (s, p) = (select(&[square, line]), PaintDefaults::default());
    let mut f = plan(ids);
    let on_square =
        PlannedEnds { first: Point::new(100.0, 100.0), last: Point::new(400.0, 300.0), current: true, before: None, after: None, trim: None };
    f.ends.insert((line, PartKind::Stroke), on_square);
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    let mut focused = Vec::new();
    for _ in 0..3 {
        t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
        t.pointer(&c, &ev(PointerKind::Up, 100.0, 100.0));
        focused.push(t.options()["focus"]["id"].clone());
    }
    assert_eq!(focused, vec![json!(square.0), json!(line.0), json!(square.0)]);
}

/// The keys report that they acted on the focused point (the canvas then shows its jump, as a
/// drag does); a press hands it back to the pointer. `tool.setOption {focus}` gives the keys a
/// point, as Set on Canvas does.
#[test]
fn the_keys_say_when_they_acted_on_the_point() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    t.set_option("focus", &json!({"kind": "entry", "id": square.0, "part": "fill"}));
    assert_eq!(t.options()["focus"], json!({"kind": "entry", "id": square.0, "part": "fill", "index": 0}));
    assert_eq!(t.options()["keyed"], json!(false));
    t.key(&c, ToolKey::Right, Mods::default());
    assert_eq!(t.options()["keyed"], json!(true), "a nudge");
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    assert_eq!(t.options()["keyed"], json!(false), "a press");
}

/// Put away, the tool keeps none of the handles it found: taken up again, it finds them afresh.
#[test]
fn put_away_it_keeps_no_handles() {
    let (d, ids) = doc();
    let [square, ..] = ids;
    let (s, p, f) = (select(&[square]), PaintDefaults::default(), plan(ids));
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Move, 10.0, 10.0));
    let walks = handle_walks();
    t.pointer(&c, &ev(PointerKind::Move, 12.0, 10.0));
    assert_eq!(handle_walks(), walks, "kept while it is in use");
    t.deactivate(&c);
    t.pointer(&c, &ev(PointerKind::Move, 14.0, 10.0));
    assert_eq!(handle_walks(), walks + 1, "found afresh");
}
