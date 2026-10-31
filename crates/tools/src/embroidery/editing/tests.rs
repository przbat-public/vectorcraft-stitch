//! Stitch editing with made-up needle points: selecting (a click, Shift, a stitch, a run, the
//! marquee, `[` `]`), moving (a drag on the grid, Shift's 45°, a preview at most once a tick, many
//! points only once rested, the arrows), inserting with Alt, deleting, Escape's order, entering and
//! leaving by a double-click, the cursors, and handles caught before needle points.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{Embroidery, StitchRun};
use vectorcraft_doc::{Appearance, Document, Node, NodeId, Selection};
use vectorcraft_geom::{PathData, Point, SubPath};

use super::super::*;
use crate::testutil::cx;
use crate::{Action, Cursor, Mods, PaintDefaults, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

/// Made-up paths of stitches, each a list of runs of needle points (document points).
struct Needles {
    object: NodeId,
    paths: Vec<(NodeId, Vec<Vec<Point>>)>,
}

impl Needles {
    fn flat(&self, id: NodeId) -> Vec<Point> {
        self.paths.iter().find(|(p, _)| *p == id).map(|(_, runs)| runs.concat()).unwrap_or_default()
    }

    /// Each stitch: the needle point it leaves and its two ends.
    fn stitches(&self) -> Vec<(NeedleRef, Point, Point)> {
        let mut out = Vec::new();
        for (id, runs) in &self.paths {
            let mut first = 0;
            for r in runs {
                for (k, w) in r.windows(2).enumerate() {
                    out.push((NeedleRef { id: *id, index: first + k }, w[0], w[1]));
                }
                first += r.len();
            }
        }
        out
    }
}

impl NeedlePoints for Needles {
    fn scope(&self) -> NeedleScope {
        NeedleScope {
            object: self.object,
            paths: self.paths.len(),
            needles: self.paths.iter().map(|(_, r)| r.concat().len()).sum(),
            bounds: None,
            refused: 0,
        }
    }
    fn holds(&self, id: NodeId) -> bool {
        self.paths.iter().any(|(p, _)| *p == id)
    }
    fn needle_at(&self, at: Point, tol: f64) -> Option<NeedleHit> {
        let mut best: Option<(f64, NeedleHit)> = None;
        for (id, _) in &self.paths {
            for (k, p) in self.flat(*id).into_iter().enumerate() {
                let d = p.distance(at);
                if d <= tol && best.is_none_or(|(e, _)| d <= e) {
                    best = Some((d, NeedleHit { needle: NeedleRef { id: *id, index: k }, point: p }));
                }
            }
        }
        best.map(|(_, h)| h)
    }
    fn thread_at(&self, at: Point, tol: f64) -> Option<ThreadHit> {
        let mut best: Option<(f64, ThreadHit)> = None;
        for (n, a, b) in self.stitches() {
            let ab = b - a;
            let t = ((at - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
            let q = a + ab * t;
            let d = q.distance(at);
            if d <= tol && best.is_none_or(|(e, _)| d <= e) {
                best = Some((d, ThreadHit { from: n, ends: [a, b], point: q }));
            }
        }
        best.map(|(_, h)| h)
    }
    fn point(&self, n: NeedleRef) -> Option<Point> {
        self.flat(n.id).get(n.index).copied()
    }
    fn step(&self, n: NeedleRef, forward: bool) -> Option<NeedleRef> {
        let all: Vec<NeedleRef> =
            self.paths.iter().flat_map(|(id, r)| (0..r.concat().len()).map(move |index| NeedleRef { id: *id, index })).collect();
        let k = all.iter().position(|m| *m == n)?;
        let j = if forward { (k + 1) % all.len() } else { (k + all.len() - 1) % all.len() };
        all.get(j).copied()
    }
    fn first(&self) -> Option<NeedleRef> {
        self.paths.first().map(|(id, _)| NeedleRef { id: *id, index: 0 })
    }
    fn run(&self, n: NeedleRef) -> Option<(usize, usize)> {
        let (_, runs) = self.paths.iter().find(|(p, _)| *p == n.id)?;
        let mut first = 0;
        for r in runs {
            if n.index < first + r.len() {
                return Some((first, first + r.len() - 1));
            }
            first += r.len();
        }
        None
    }
    fn anchors(&self, n: NeedleRef) -> Vec<(usize, usize)> {
        let Some((first, _)) = self.run(n) else { return vec![] };
        let (_, runs) = self.paths.iter().find(|(p, _)| *p == n.id).unwrap();
        let mut start = 0;
        for (s, r) in runs.iter().enumerate() {
            if start == first {
                return vec![(s, n.index - first)];
            }
            start += r.len();
        }
        vec![]
    }
    fn of_anchor(&self, id: NodeId, (s, a): (usize, usize)) -> Option<usize> {
        let (_, runs) = self.paths.iter().find(|(p, _)| *p == id)?;
        let first: usize = runs.iter().take(s).map(Vec::len).sum();
        (a < runs.get(s)?.len()).then_some(first + a)
    }
    fn count(&self, id: NodeId) -> usize {
        self.flat(id).len()
    }
    fn order(&self, id: NodeId) -> Option<usize> {
        self.paths.iter().position(|(p, _)| *p == id)
    }
    fn number(&self, _n: NeedleRef) -> Option<usize> {
        None
    }
}

/// The plan's answers while editing: the needle points; nothing else (no handles).
struct Editing {
    needles: Option<Needles>,
    under: Option<(NodeId, Option<vectorcraft_doc::embroidery::PartKind>)>,
}

impl PlanFacts for Editing {
    fn generation(&self) -> u64 {
        1
    }
    fn ends(&self, _object: NodeId, _part: vectorcraft_doc::embroidery::PartKind) -> Option<PlannedEnds> {
        None
    }
    fn part_at(&self, _at: Point, _tol: f64) -> Option<(NodeId, Option<vectorcraft_doc::embroidery::PartKind>)> {
        self.under
    }
    fn needle_points(&self) -> Option<&dyn NeedlePoints> {
        self.needles.as_ref().map(|n| n as &dyn NeedlePoints)
    }
}

/// A document holding a converted object: a group with two paths of stitches, the first a run of
/// three needle points along y = 100 (x 100, 200, 300) then a run of two along y = 200 (x 100,
/// 300), the second one run along y = 400 (x 100 to 400, every 100) → the document, the group and
/// the paths.
fn doc() -> (Document, NodeId, [NodeId; 2]) {
    let mut d = Document::new(600.0, 600.0);
    let l = d.layers[0].id;
    let path = |d: &mut Document, runs: &[&[(f64, f64)]]| {
        let id = d.alloc_id();
        let subpaths = runs.iter().map(|r| SubPath::polyline(&r.iter().map(|(x, y)| Point::new(*x, *y)).collect::<Vec<_>>(), false)).collect();
        let mut n = Node::path(id, PathData::new(subpaths), Appearance::default_art());
        n.extras_mut().embroidery =
            Some(Box::new(Embroidery { stitches: Some(StitchRun { manual: true, ..StitchRun::default() }), ..Embroidery::default() }));
        (id, n)
    };
    let (a, na) = path(&mut d, &[&[(100.0, 100.0), (200.0, 100.0), (300.0, 100.0)], &[(100.0, 200.0), (300.0, 200.0)]]);
    let (b, nb) = path(&mut d, &[&[(100.0, 400.0), (200.0, 400.0), (300.0, 400.0), (400.0, 400.0)]]);
    let g = d.alloc_id();
    d.insert(Some(l), 0, Node::group(g, vec![std::sync::Arc::new(na), std::sync::Arc::new(nb)])).unwrap();
    (d, g, [a, b])
}

fn needles(g: NodeId, [a, b]: [NodeId; 2]) -> Needles {
    let p = |x: f64, y: f64| Point::new(x, y);
    Needles {
        object: g,
        paths: vec![
            (a, vec![vec![p(100.0, 100.0), p(200.0, 100.0), p(300.0, 100.0)], vec![p(100.0, 200.0), p(300.0, 200.0)]]),
            (b, vec![vec![p(100.0, 400.0), p(200.0, 400.0), p(300.0, 400.0), p(400.0, 400.0)]]),
        ],
    }
}

fn ev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent::new(kind, x, y)
}

fn with(kind: PointerKind, x: f64, y: f64, mods: Mods) -> PointerEvent {
    PointerEvent { mods, ..PointerEvent::new(kind, x, y) }
}

const SHIFT: Mods = Mods { shift: true, alt: false, cmd: false, ctrl: false, space: false };
const ALT: Mods = Mods { shift: false, alt: true, cmd: false, ctrl: false, space: false };

/// The selection with needle points `anchors` of path `id` selected.
fn selected(id: NodeId, anchors: &[(usize, usize)]) -> Selection {
    let mut s = Selection { objects: vec![id], ..Default::default() };
    s.anchors.insert(id, anchors.iter().copied().collect());
    s
}

/// The isolated context for `d`, `s` and `f`.
fn editing<'a>(d: &'a Document, s: &'a Selection, p: &'a PaintDefaults, f: &'a Editing, g: NodeId) -> ToolContext<'a> {
    ToolContext { embroidery: Some(f), isolation: Some(g), ..cx(d, s, p) }
}

fn exec(cmd: &str, params: Value) -> Action {
    Action::Exec(cmd.into(), params)
}

/// A click on a needle point selects it alone; Shift-click adds it or takes it away; a click on a
/// stitch selects its two needle points; Alt-click on a needle point selects it as a click does.
#[test]
fn a_click_selects_a_needle_point_or_a_stitchs_two() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (Selection::default(), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    let one = |k: usize, mode: &str| exec("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": a.0, "index": k}], "mode": mode}));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 201.0, 101.0)), vec![one(1, "set")]);
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 201.0, 101.0)), vec![]);
    assert_eq!(t.pointer(&c, &with(PointerKind::Down, 300.0, 100.0, SHIFT)), vec![one(2, "toggle")]);
    t.pointer(&c, &with(PointerKind::Up, 300.0, 100.0, SHIFT));
    assert_eq!(t.pointer(&c, &with(PointerKind::Down, 100.0, 100.0, ALT)), vec![one(0, "set")], "Alt on a point: a click");
    t.pointer(&c, &with(PointerKind::Up, 100.0, 100.0, ALT));
    // Between needle points 0 and 1 of the first run.
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Down, 150.0, 101.0)),
        vec![exec("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": a.0, "index": 0}, {"id": a.0, "index": 1}], "mode": "set"}))]
    );
}

/// A press on a needle point selected already keeps the selection (a drag moves them all); let go
/// without dragging, it is selected alone.
#[test]
fn a_click_on_a_selected_needle_point_selects_it_alone_on_release() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 0), (0, 1)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0)), vec![]);
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 100.0, 100.0)),
        vec![exec("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": a.0, "index": 0}], "mode": "set"}))]
    );
}

/// A drag of selected needle points: Begin past the threshold, a preview at the tick (on the 0.1 mm
/// grid, millimetres), none at the next tick while the move on the grid is the same, the last move
/// previewed at the release and kept: one undo step.
#[test]
fn a_drag_moves_the_selected_needle_points_on_the_grid() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 0)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    assert!(t.wants_ticks());
    assert_eq!(t.pointer(&c, &ev(PointerKind::Drag, 101.0, 100.0)), vec![], "under the threshold");
    assert_eq!(t.pointer(&c, &ev(PointerKind::Drag, 110.0, 105.0)), vec![Action::Begin("Move Needle Points".into())]);
    assert!(t.busy());
    // 10 pt is 3.5278 mm, on the grid 3.5; 5 pt 1.7639, on the grid 1.8.
    assert_eq!(t.tick(&c, 1.0 / 60.0), vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 3.5, "dy": 1.8}))]);
    t.pointer(&c, &ev(PointerKind::Drag, 110.05, 105.05));
    assert_eq!(t.tick(&c, 1.0 / 60.0), vec![], "the same move on the grid: no preview");
    t.pointer(&c, &ev(PointerKind::Drag, 120.0, 105.0));
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 120.0, 105.0)),
        vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 7.1, "dy": 1.8})), Action::Commit]
    );
    assert!(!t.busy() && !t.wants_ticks());
}

/// Shift keeps the drag at 45° steps; a drag that comes back to where it started keeps nothing.
#[test]
fn shift_keeps_45_degrees_and_a_drag_back_to_the_start_keeps_nothing() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 0)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &with(PointerKind::Drag, 128.35, 101.0, SHIFT));
    assert_eq!(t.tick(&c, 0.01), vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 10.0, "dy": 0.0}))], "horizontal");
    t.pointer(&c, &ev(PointerKind::Drag, 100.0, 100.0));
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 100.0, 100.0)),
        vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 0.0, "dy": 0.0})), Action::Commit],
        "back to the start: the preview undone, the move of nothing kept"
    );
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 110.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 100.0, 100.0));
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 100.0, 100.0)),
        vec![Action::Begin("Move Needle Points".into()), Action::Cancel].split_off(1),
        "never previewed: cancelled"
    );
}

/// Past [`LIVE_DRAG_POINTS_MAX`] needle points selected, a drag previews only once it has rested
/// [`PAUSE_PREVIEW_SECS`] (the project owner's choice), and at the release.
#[test]
fn a_drag_of_many_needle_points_previews_when_it_rests() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let many: Vec<(usize, usize)> = (0..=LIVE_DRAG_POINTS_MAX).map(|k| (0, k)).collect();
    let (s, p, f) = (selected(a, &many), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 110.0, 100.0));
    assert_eq!(t.tick(&c, PAUSE_PREVIEW_SECS / 2.0), vec![], "moving: not yet");
    t.pointer(&c, &ev(PointerKind::Drag, 120.0, 100.0));
    assert_eq!(t.tick(&c, PAUSE_PREVIEW_SECS / 2.0), vec![], "moved again: the pause starts over");
    assert_eq!(t.tick(&c, PAUSE_PREVIEW_SECS / 2.0), vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 7.1, "dy": 0.0}))]);
    t.pointer(&c, &ev(PointerKind::Drag, 130.0, 100.0));
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 130.0, 100.0)),
        vec![Action::Preview("embroidery.moveNeedlePoints".into(), json!({"dx": 10.6, "dy": 0.0})), Action::Commit]
    );
}

/// Escape cancels a drag; then, with needle points selected, deselects them; then isn't claimed,
/// so isolation's own Escape ends editing (the project owner's order).
#[test]
fn escape_cancels_a_drag_then_deselects_then_leaves() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 1)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 200.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 220.0, 100.0));
    assert!(t.tick(&c, 0.01).iter().any(|a| matches!(a, Action::Preview(..))), "the drag moved them before Escape");
    assert!(t.claims_key(&c, ToolKey::Escape));
    assert_eq!(t.key(&c, ToolKey::Escape, Mods::default()), vec![Action::Cancel]);
    assert!(!t.busy());
    assert!(t.claims_key(&c, ToolKey::Escape), "needle points selected");
    assert_eq!(t.key(&c, ToolKey::Escape, Mods::default()), vec![exec("select.none", json!({}))]);
    let none = Selection::default();
    let c = editing(&d, &none, &p, &f, g);
    assert!(!t.claims_key(&c, ToolKey::Escape), "nothing selected: isolation's Escape ends editing");
}

/// A drag from where no needle point lies draws a marquee; at the release its needle points are
/// selected by one command carrying the rectangle in millimetres (Shift adds); a click there selects
/// nothing.
#[test]
fn the_marquee_selects_the_needle_points_inside_it() {
    let (d, g, ids) = doc();
    let (s, p, f) = (Selection::default(), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 50.0, 50.0));
    t.pointer(&c, &ev(PointerKind::Drag, 250.0, 150.0));
    assert!(t.busy());
    assert_eq!(t.options()["stitches"]["marquee"], json!([50.0, 50.0, 250.0, 150.0]));
    let mm = |v: f64| v / (72.0 / 25.4);
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::Up, 250.0, 150.0)),
        vec![exec("embroidery.selectNeedlePoints", json!({"rect": [mm(50.0), mm(50.0), mm(250.0), mm(150.0)], "mode": "set"}))]
    );
    t.pointer(&c, &with(PointerKind::Down, 250.0, 150.0, SHIFT));
    t.pointer(&c, &with(PointerKind::Drag, 50.0, 50.0, SHIFT));
    assert_eq!(
        t.pointer(&c, &with(PointerKind::Up, 50.0, 50.0, SHIFT)),
        vec![exec("embroidery.selectNeedlePoints", json!({"rect": [mm(50.0), mm(50.0), mm(250.0), mm(150.0)], "mode": "add"}))]
    );
    t.pointer(&c, &ev(PointerKind::Down, 50.0, 50.0));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 50.0, 50.0)), vec![exec("select.none", json!({}))], "a click away from them");
}

/// Alt-click on a stitch inserts a needle point where it is pressed, before the needle point the
/// stitch arrives at.
#[test]
fn alt_click_on_a_stitch_inserts_a_needle_point_there() {
    let (d, g, ids) = doc();
    let [_, b] = ids;
    let (s, p, f) = (Selection::default(), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    let mm = |v: f64| v / (72.0 / 25.4);
    assert_eq!(
        t.pointer(&c, &with(PointerKind::Down, 250.0, 401.0, ALT)),
        vec![exec("embroidery.insertNeedlePoints", json!({"id": b.0, "index": 2, "at": [[mm(250.0), mm(400.0)]]}))]
    );
    assert_eq!(t.cursor(&c, Point::new(250.0, 401.0), ALT), Cursor::PenAdd);
}

/// The arrows move the selected needle points a tenth of a millimetre, Shift a millimetre; Delete
/// and Backspace delete them; with nothing selected none of them is claimed.
#[test]
fn the_arrows_move_and_delete_deletes() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 1)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    let moved = |dx: f64, dy: f64| exec("embroidery.moveNeedlePoints", json!({"dx": dx, "dy": dy}));
    for (key, mods, want) in [
        (ToolKey::Left, Mods::default(), moved(-0.1, 0.0)),
        (ToolKey::Right, SHIFT, moved(1.0, 0.0)),
        (ToolKey::Up, Mods::default(), moved(0.0, -0.1)),
        (ToolKey::Down, SHIFT, moved(0.0, 1.0)),
        (ToolKey::Delete, Mods::default(), exec("embroidery.deleteNeedlePoints", json!({}))),
        (ToolKey::Backspace, Mods::default(), exec("embroidery.deleteNeedlePoints", json!({}))),
    ] {
        assert!(t.claims_key(&c, key), "{key:?}");
        assert_eq!(t.key(&c, key, mods), vec![want], "{key:?}");
    }
    let none = Selection::default();
    let c = editing(&d, &none, &p, &f, g);
    for key in [ToolKey::Left, ToolKey::Delete, ToolKey::Escape] {
        assert!(!t.claims_key(&c, key), "{key:?} with nothing selected");
    }
}

/// `[` and `]` step to the needle point before or after along the thread, from the one last pressed
/// (else the first selected, else the first), alone selected; Shift adds it; round from the last
/// path's last to the first.
#[test]
fn brackets_step_along_the_thread() {
    let (d, g, ids) = doc();
    let [a, b] = ids;
    let (s, p, f) = (Selection::default(), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    let one =
        |id: NodeId, k: usize, mode: &str| exec("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": id.0, "index": k}], "mode": mode}));
    assert!(t.claims_key(&c, ToolKey::BracketRight));
    assert_eq!(t.key(&c, ToolKey::BracketRight, Mods::default()), vec![one(a, 0, "set")], "nothing current: the first");
    assert_eq!(t.key(&c, ToolKey::BracketRight, SHIFT), vec![one(a, 1, "add")]);
    t.pointer(&c, &ev(PointerKind::Down, 300.0, 200.0));
    t.pointer(&c, &ev(PointerKind::Up, 300.0, 200.0));
    assert_eq!(t.key(&c, ToolKey::BracketRight, Mods::default()), vec![one(b, 0, "set")], "on to the next path");
    assert_eq!(t.key(&c, ToolKey::BracketLeft, Mods::default()), vec![one(a, 4, "set")]);
    let sel = selected(b, &[(0, 3)]);
    let c = editing(&d, &sel, &p, &f, g);
    let mut t = EmbroideryTool::default();
    assert_eq!(t.key(&c, ToolKey::BracketRight, Mods::default()), vec![one(a, 0, "set")], "from the first selected, round to the first");
}

/// A double-click on a stitch selects its run (Shift adds it); one away from the stitches being
/// edited ends editing.
#[test]
fn a_double_click_selects_a_run_or_leaves() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (Selection::default(), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::DoubleClick, 200.0, 201.0)),
        vec![exec("embroidery.selectNeedlePoints", json!({"ranges": [{"id": a.0, "from": 3, "to": 4}], "mode": "set"}))]
    );
    assert_eq!(t.pointer(&c, &ev(PointerKind::DoubleClick, 550.0, 550.0)), vec![exec("object.exitIsolation", json!({}))]);
}

/// Outside editing, a double-click on a stitch object (found by its stitches) isolates it and shows
/// the stitch view; on art that holds no stitches it does nothing; a press on it selects it (its
/// cursor isn't Not Allowed).
#[test]
fn a_double_click_on_a_stitch_object_starts_editing() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p) = (Selection::default(), PaintDefaults::default());
    let f = Editing { needles: None, under: Some((a, None)) };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    assert_eq!(
        t.pointer(&c, &ev(PointerKind::DoubleClick, 150.0, 100.0)),
        vec![exec("object.isolate", json!({"id": g.0})), exec("view.stitchPreview", json!({"on": true}))]
    );
    assert_eq!(t.cursor(&c, Point::new(150.0, 100.0), Mods::default()), Cursor::Embroidery, "not Not Allowed: it can be edited");
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 150.0, 100.0)), vec![exec("select.set", json!({"ids": [a.0]}))]);
    let f = Editing { needles: None, under: None };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    assert_eq!(t.pointer(&c, &ev(PointerKind::DoubleClick, 550.0, 550.0)), vec![]);
}

/// The cursors while editing: the point cursor over a needle point or a stitch and while dragging,
/// the Pen's add cursor with Alt over a stitch, the tool's own elsewhere; the options say what is
/// under the pointer.
#[test]
fn the_cursors_and_options_while_editing() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 0)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    assert_eq!(t.cursor(&c, Point::new(100.0, 100.0), Mods::default()), Cursor::EmbroideryPoint);
    assert_eq!(t.cursor(&c, Point::new(150.0, 100.0), Mods::default()), Cursor::EmbroideryPoint);
    assert_eq!(t.cursor(&c, Point::new(150.0, 100.0), ALT), Cursor::PenAdd);
    assert_eq!(t.cursor(&c, Point::new(100.0, 100.0), ALT), Cursor::EmbroideryPoint, "Alt on a point: a click");
    assert_eq!(t.cursor(&c, Point::new(550.0, 550.0), Mods::default()), Cursor::Embroidery);
    t.pointer(&c, &ev(PointerKind::Move, 100.0, 100.0));
    assert_eq!(t.options()["stitches"]["hover"], json!({"needlePoint": {"id": a.0, "index": 0}}));
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 130.0, 100.0));
    assert_eq!(t.cursor(&c, Point::new(550.0, 550.0), Mods::default()), Cursor::EmbroideryPoint, "dragging");
    assert_eq!(t.options()["stitches"]["moved"], json!([10.6, 0.0]));
}

/// Put away mid-drag, the drag is kept, as a release keeps it; an outside command ending it lets
/// it go.
#[test]
fn put_away_mid_drag_keeps_the_drag() {
    let (d, g, ids) = doc();
    let [a, _] = ids;
    let (s, p, f) = (selected(a, &[(0, 0)]), PaintDefaults::default(), Editing { needles: Some(needles(g, ids)), under: None });
    let c = editing(&d, &s, &p, &f, g);
    let mut t = EmbroideryTool::default();
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 130.0, 100.0));
    t.tick(&c, 0.01);
    assert_eq!(t.deactivate(&c), vec![Action::Commit]);
    t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0));
    t.pointer(&c, &ev(PointerKind::Drag, 130.0, 100.0));
    t.after_command(&c);
    assert!(!t.busy() && !t.wants_ticks(), "let go");
    assert_eq!(t.pointer(&c, &ev(PointerKind::Up, 130.0, 100.0)), vec![]);
}

/// Outside isolation the tool is as before: no needle point is caught or stepped to, and a press
/// on a path of stitches selects it, as on any object a press selects.
#[test]
fn outside_editing_nothing_changes() {
    let (d, _, [a, _]) = doc();
    let (s, p) = (Selection::default(), PaintDefaults::default());
    let f = Editing { needles: None, under: None };
    let c = ToolContext { embroidery: Some(&f), ..cx(&d, &s, &p) };
    let mut t = EmbroideryTool::default();
    assert!(!t.claims_key(&c, ToolKey::BracketRight) && !t.claims_key(&c, ToolKey::Left));
    assert_eq!(t.pointer(&c, &ev(PointerKind::Down, 100.0, 100.0)), vec![exec("select.set", json!({"ids": [a.0]}))]);
}
