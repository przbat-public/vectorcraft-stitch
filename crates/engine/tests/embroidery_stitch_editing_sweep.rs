//! Stitch editing swept: the embroidery tool working inside isolation of each kind of stitch object
//! the manual fixture holds (converted fills, strokes, compound paths and groups, a clip group's
//! member, an opened design's thread blocks, the fixture's own runs of one needle point, a tack, cuts
//! and stops), in the stitch view, at any zoom, with any run of pointer events (on needle points,
//! on stitches, off them, at hostile coordinates), keys (Escape, the arrows, Delete, `[` `]`),
//! ticks, whole drags of needle points ended by the release, Escape or the selection deleted, and
//! marquees: nothing panics (the sweep FAILS on any panic the engine caught, a Preview's included),
//! the session holds together, a drag adds at most one undo step, Escape mid-drag puts the document
//! back as it was at the press, and once the button is let go and Escape pressed no drag is left
//! open.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::Cell;
use std::sync::Once;

use proptest::prelude::*;
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::{Session, ViewInfo};
use vectorcraft_geom::Point;
use vectorcraft_testkit::embroidery as fx;
use vectorcraft_testkit::invariants::check_session;
use vectorcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};

thread_local! {
    /// The panics the engine caught on this thread since the sweep began: a tool's Preview that
    /// fails is only logged, so the sweep counts every panic through the panic hook, which still runs
    /// when the engine catches one, on the thread that panicked (the engine's work runs on the
    /// caller's), so a test running beside the sweep counts none of its own here.
    static CAUGHT: Cell<usize> = const { Cell::new(0) };
}

fn count_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let printed = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            CAUGHT.with(|c| c.set(c.get() + 1));
            printed(info);
        }));
    });
}

/// `r`, failing the case when it is a panic the engine caught (`EngineError::Internal`).
fn no_panic<T>(r: vectorcraft_engine::Result<T>) -> Result<(), TestCaseError> {
    match r {
        Err(vectorcraft_engine::EngineError::Internal { cmd, msg }) => Err(TestCaseError::fail(format!("{cmd} panicked: {msg}"))),
        _ => Ok(()),
    }
}

/// Where a pointer event lands: near a needle point of the object edited (by index among them), on
/// the middle of a stitch, or anywhere.
#[derive(Clone, Copy, Debug)]
enum Spot {
    Needle(usize, f64, f64),
    Stitch(usize),
    At(f64, f64),
}

#[derive(Clone, Debug)]
enum Step {
    Pointer(PointerKind, Spot, Mods),
    Key(ToolKey, Mods),
    /// Time held, seconds.
    Tick(f64),
    /// The selection deleted (`edit.clear`).
    Delete,
    /// Undo, and Redo.
    Undo,
    Redo,
    /// The other document made active, and this one again.
    SwitchDocument,
    /// A drag from a spot, by each offset (screen pixels) in turn, ticked after each, ended so.
    Drag(Spot, Vec<(f64, f64)>, End),
}

#[derive(Clone, Copy, Debug)]
enum End {
    Release,
    Escape,
    Delete,
    /// Undo while the drag goes on, then the release.
    Undo,
}

/// The zooms the sweep views the design at: far out, as drawn, the mockup's 3,100 %, the closest.
const ZOOMS: [f64; 4] = [0.05, 1.0, 31.0, 640.0];

fn arb_mods() -> impl Strategy<Value = Mods> {
    (any::<bool>(), prop::bool::weighted(0.2)).prop_map(|(shift, alt)| Mods { shift, alt, ..Mods::default() })
}

fn arb_spot() -> impl Strategy<Value = Spot> {
    let coord = prop_oneof![8 => -50.0..1500.0f64, 1 => Just(f64::NAN), 1 => Just(1e300), 1 => Just(-1e300)];
    prop_oneof![
        4 => (0usize..400, -3.0..3.0f64, -3.0..3.0f64).prop_map(|(i, dx, dy)| Spot::Needle(i, dx, dy)),
        2 => (0usize..400).prop_map(Spot::Stitch),
        2 => (coord.clone(), coord).prop_map(|(x, y)| Spot::At(x, y)),
    ]
}

fn arb_step() -> impl Strategy<Value = Step> {
    let kind =
        prop_oneof![Just(PointerKind::Move), Just(PointerKind::Down), Just(PointerKind::Drag), Just(PointerKind::Up), Just(PointerKind::DoubleClick)];
    let key = prop_oneof![
        Just(ToolKey::Escape),
        Just(ToolKey::Delete),
        Just(ToolKey::Backspace),
        Just(ToolKey::Left),
        Just(ToolKey::Up),
        Just(ToolKey::BracketLeft),
        Just(ToolKey::BracketRight),
        Just(ToolKey::Enter),
    ];
    prop_oneof![
        8 => (kind, arb_spot(), arb_mods()).prop_map(|(k, s, m)| Step::Pointer(k, s, m)),
        3 => (key, arb_mods()).prop_map(|(k, m)| Step::Key(k, m)),
        1 => (0.0..1.0f64).prop_map(Step::Tick),
        1 => Just(Step::Delete),
        1 => Just(Step::Undo),
        1 => Just(Step::Redo),
        1 => Just(Step::SwitchDocument),
        4 => (
            arb_spot(),
            prop::collection::vec((-80.0..80.0f64, -80.0..80.0f64), 1..5),
            prop::sample::select(vec![End::Release, End::Escape, End::Delete, End::Undo])
        )
            .prop_map(|(s, moves, end)| Step::Drag(s, moves, end)),
    ]
}

/// The manual fixture, planned once, and the stitch objects it holds (each isolated in turn).
fn fixture() -> (Session, Vec<u64>) {
    let mut s = Session::new();
    let m = fx::manual(&mut |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}")));
    let c = m.converted;
    let objects =
        vec![c.fill, c.stroke, c.both, c.compound, c.group, c.clip_member, c.expanded, m.opened[0].top, m.opened[1].top, m.one_point, m.stopped];
    (s, objects)
}

/// The needle points of the paths of stitches inside `id` (document points).
fn needle_points(s: &Session, id: u64) -> Vec<Point> {
    let mut out = Vec::new();
    if let Some(n) = s.doc().ok().and_then(|st| st.doc.node(NodeId(id)).cloned()) {
        n.walk(&mut |c| {
            if c.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some())
                && let Some(p) = c.path_data()
            {
                out.extend(p.subpaths.iter().flat_map(|sp| sp.anchors.iter().map(|a| a.p)));
            }
        });
    }
    out
}

/// Where `spot` is, on `points` (the needle points of the object edited).
fn place(spot: Spot, points: &[Point]) -> (f64, f64) {
    match spot {
        Spot::Needle(i, dx, dy) => points.get(i % points.len().max(1)).map_or((dx, dy), |p| (p.x + dx, p.y + dy)),
        Spot::Stitch(i) => {
            let k = i % points.len().max(1);
            match (points.get(k), points.get(k + 1)) {
                (Some(a), Some(b)) => ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
                (Some(a), None) => (a.x, a.y),
                _ => (0.0, 0.0),
            }
        }
        Spot::At(x, y) => (x, y),
    }
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(40))]

    #[test]
    fn stitch_editing_never_breaks_a_session(
        object in 0usize..11,
        zoom in prop::sample::select(ZOOMS.to_vec()),
        steps in prop::collection::vec(arb_step(), 1..40),
    ) {
        count_panics();
        let caught = CAUGHT.with(Cell::get);
        let (mut s, objects) = fixture();
        // A second document, to switch to and back.
        no_panic(s.execute("file.new", &json!({"width": 100, "height": 100, "artboards": 1})))?;
        prop_assert!(s.set_active(0));
        let id = objects[object % objects.len()];
        no_panic(s.execute("object.isolate", &json!({"id": id})))?;
        no_panic(s.execute("view.stitchPreview", &json!({"on": true})))?;
        no_panic(s.execute("embroidery.sequence", &json!({})))?;
        let view = ViewInfo { zoom, ..ViewInfo::default() };
        s.select_tool("embroidery", view).unwrap();
        let editing = s.stitch_editing(view);
        prop_assert!(editing.is_some(), "isolating {id} starts editing its needle points");
        let undo = |s: &Session| s.doc().map_or(0, |d| d.history.undo.len());
        for step in &steps {
            let points = needle_points(&s, id);
            match step {
                Step::Pointer(kind, spot, mods) => {
                    let (x, y) = place(*spot, &points);
                    no_panic(s.pointer(&PointerEvent::new(*kind, x, y).with_mods(*mods), view))?;
                }
                Step::Key(key, mods) => {
                    no_panic(s.tool_key(*key, *mods, view))?;
                }
                Step::Tick(dt) => {
                    no_panic(s.tool_tick(*dt, view))?;
                }
                Step::Delete => {
                    no_panic(s.execute("edit.clear", &json!({})))?;
                }
                Step::Undo => {
                    no_panic(s.execute("edit.undo", &json!({})))?;
                }
                Step::Redo => {
                    no_panic(s.execute("edit.redo", &json!({})))?;
                }
                Step::SwitchDocument => {
                    prop_assert!(s.set_active(1) && s.stitch_editing(view).is_none(), "the other document edits nothing");
                    prop_assert!(s.set_active(0));
                }
                Step::Drag(spot, moves, end) => {
                    // A gesture of its own: whatever the steps before left open ends first.
                    no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, 0.0, 0.0), view))?;
                    no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
                    let (x, y) = place(*spot, &points);
                    let steps_before = undo(&s);
                    no_panic(s.pointer(&PointerEvent::new(PointerKind::Down, x, y), view))?;
                    let pressed = (s.doc().map(|d| d.doc.clone()).ok(), undo(&s));
                    let mut to = (x, y);
                    for (dx, dy) in moves {
                        to = (x + dx / zoom, y + dy / zoom);
                        no_panic(s.pointer(&PointerEvent::new(PointerKind::Drag, to.0, to.1), view))?;
                        no_panic(s.tool_tick(1.0 / 60.0, view))?;
                    }
                    match end {
                        End::Release => {
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.0, to.1), view))?;
                            // The press may select (no undo step), the drag at most one.
                            prop_assert!(undo(&s) <= steps_before + 1, "a drag made {} undo steps: {steps:?}", undo(&s) - steps_before);
                        }
                        End::Escape => {
                            let dragging = s.in_interaction();
                            no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
                            if dragging {
                                prop_assert!(s.doc().ok().map(|d| d.doc.clone()) == pressed.0, "Escape mid-drag left the document changed: {steps:?}");
                                prop_assert_eq!(undo(&s), pressed.1, "Escape mid-drag left an undo step");
                            }
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.0, to.1), view))?;
                        }
                        End::Delete => {
                            no_panic(s.execute("edit.clear", &json!({})))?;
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.0, to.1), view))?;
                        }
                        End::Undo => {
                            no_panic(s.execute("edit.undo", &json!({})))?;
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.0, to.1), view))?;
                            prop_assert!(!s.in_interaction(), "an undo mid-drag left the drag open: {steps:?}");
                        }
                    }
                }
            }
            if !s.in_interaction() {
                prop_assert!(check_session(&s).is_ok(), "{:?} after {step:?}", check_session(&s));
            }
            // What is being edited, asked after every step as the interface asks it after every
            // change: never more selected than the object holds, record numbers only of a
            // selection.
            if let Some(e) = s.stitch_editing(view) {
                prop_assert!(e.selected <= e.scope.needles, "{} selected of {} after {step:?}", e.selected, e.scope.needles);
                prop_assert!(e.ends.is_none() || e.selected > 0, "the ends of nothing selected after {step:?}");
            }
        }
        no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, 0.0, 0.0), view))?;
        no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
        prop_assert_eq!(CAUGHT.with(Cell::get), caught, "a panic was caught (a Preview's, say) after {:?}", steps);
        prop_assert!(!s.in_interaction(), "a drag left open after {steps:?}");
        prop_assert!(check_session(&s).is_ok());
    }
}

/// A drag of needle points with the pointer, through the session, makes the document a
/// `embroidery.moveNeedlePoints` of the same move makes (what an agent's `ui.pointer` drag and its
/// command do alike), in one undo step named as the command is.
#[test]
fn a_drag_is_the_move_command() {
    let (mut s, objects) = fixture();
    let id = objects[0];
    s.execute("object.isolate", &json!({"id": id})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("embroidery.sequence", &json!({})).unwrap();
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    let points = needle_points(&s, id);
    let p = points[points.len() / 2];
    let undo = s.doc().unwrap().history.undo.len();
    s.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), view).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, p.x, p.y), view).unwrap();
    let selected = s.doc().unwrap().selection.clone();
    assert_eq!(selected.anchors.values().map(std::collections::BTreeSet::len).sum::<usize>(), 1, "one needle point selected: {selected:?}");
    // The same document, the same needle point selected by the same press, moved by the command.
    let (mut by_command, _) = fixture();
    by_command.execute("object.isolate", &json!({"id": id})).unwrap();
    by_command.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    by_command.execute("embroidery.sequence", &json!({})).unwrap();
    by_command.select_tool("embroidery", view).unwrap();
    by_command.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), view).unwrap();
    by_command.pointer(&PointerEvent::new(PointerKind::Up, p.x, p.y), view).unwrap();
    assert_eq!(by_command.doc().unwrap().selection, selected);
    // 20 pt right, 10 pt down: 7.1 mm and 3.5 mm on the 0.1 mm grid.
    s.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), view).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, p.x + 10.0, p.y + 5.0), view).unwrap();
    s.tool_tick(1.0 / 60.0, view).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Drag, p.x + 20.0, p.y + 10.0), view).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, p.x + 20.0, p.y + 10.0), view).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    assert_eq!(s.doc().unwrap().history.undo.last().map(|e| e.label.clone()), Some("Move Needle Points".into()));
    by_command.execute("embroidery.moveNeedlePoints", &json!({"dx": 7.1, "dy": 3.5})).unwrap();
    // Made a moment apart, the two documents may say they were created a second apart.
    let (mut dragged, mut commanded) = ((*s.doc().unwrap().doc).clone(), (*by_command.doc().unwrap().doc).clone());
    (dragged.metadata.created, commanded.metadata.created) = (None, None);
    assert_eq!(dragged, commanded, "the drag is the command");
}

/// The lasso of stitch editing is the Lasso tool (the project owner's choice, 2026-10-27): in
/// isolation of a converted object it selects the needle points inside its loop, which stitch
/// editing counts while the Lasso is still the tool (its look kept, the owner's choice of
/// 2026-10-31) but not out of the stitch view; back with the Embroidery Tool they are being edited
/// (a drag moves them).
#[test]
fn the_lasso_tool_selects_needle_points_while_editing() {
    let (mut s, objects) = fixture();
    let id = objects[0];
    s.execute("object.isolate", &json!({"id": id})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("embroidery.sequence", &json!({})).unwrap();
    let view = ViewInfo::default();
    let points = needle_points(&s, id);
    let (lo, hi) = points.iter().fold((Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN)), |(lo, hi), p| {
        (Point::new(lo.x.min(p.x), lo.y.min(p.y)), Point::new(hi.x.max(p.x), hi.y.max(p.y)))
    });
    s.select_tool("lasso", view).unwrap();
    // A loop round the object's left half, its right edge between two columns of needle points.
    let mut xs: Vec<f64> = points.iter().map(|p| p.x).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let at = xs.len() / 2;
    let mid = (xs[at - 1] + xs[at]) / 2.0;
    let path = [(lo.x - 5.0, lo.y - 5.0), (mid, lo.y - 5.0), (mid, hi.y + 5.0), (lo.x - 5.0, hi.y + 5.0), (lo.x - 5.0, lo.y - 5.0)];
    s.pointer(&PointerEvent::new(PointerKind::Down, path[0].0, path[0].1), view).unwrap();
    for (x, y) in &path[1..] {
        s.pointer(&PointerEvent::new(PointerKind::Drag, *x, *y), view).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, path[0].0, path[0].1), view).unwrap();
    let inside = points.iter().filter(|p| p.x < mid).count();
    let selected: usize = s.doc().unwrap().selection.anchors.values().map(std::collections::BTreeSet::len).sum();
    assert!(inside > 0 && selected == inside, "the lasso selected {selected} of the {inside} anchors inside it");
    let lassoed = s.stitch_editing(view).expect("the Lasso keeps stitch editing's answers");
    assert_eq!(lassoed.selected, inside, "counted while the Lasso is the tool: {lassoed:?}");
    s.execute("view.stitchPreview", &json!({"on": false})).unwrap();
    assert!(s.stitch_editing(view).is_none(), "out of the stitch view the Lasso is upstream's");
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.select_tool("embroidery", view).unwrap();
    let editing = s.stitch_editing(view).expect("editing");
    assert_eq!(editing.selected, inside, "back with the Embroidery Tool, the lasso's needle points are selected: {editing:?}");
    // And a drag moves them, those outside the loop left where they were.
    let grip = *points.iter().filter(|p| p.x < mid).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Down, grip.x, grip.y), view).unwrap();
    for step in 1..=4 {
        s.pointer(&PointerEvent::new(PointerKind::Drag, grip.x + 2.5 * f64::from(step), grip.y), view).unwrap();
        s.tool_tick(1.0, view).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, grip.x + 10.0, grip.y), view).unwrap();
    let after = needle_points(&s, id);
    let moved = points.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert_eq!(moved, inside, "the drag moved the lasso's needle points and no other");
    assert!(points.iter().zip(&after).filter(|(a, _)| a.x < mid).all(|(a, b)| b.x > a.x + 1.0), "to the right");
}

/// Select All in isolation of a converted group of two objects selects the two converted objects
/// whole: their needle points are what stitch editing counts as selected, by the rule every reader
/// of the selection keeps; the tool claims the arrows and moves those needle points (one undo step
/// named as the command is, not upstream's nudge of the objects), claims Delete and deletes them as
/// stitches, and claims Escape, which deselects before isolation is left.
#[test]
fn objects_selected_whole_stand_for_their_needle_points() {
    let (mut s, objects) = fixture();
    let group = objects[4];
    s.execute("object.isolate", &json!({"id": group})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("embroidery.sequence", &json!({})).unwrap();
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    let members: Vec<NodeId> = s.doc().unwrap().doc.node(NodeId(group)).unwrap().children().unwrap().iter().map(|c| c.id).collect();
    assert_eq!(members.len(), 2, "two converted objects");
    assert_eq!(s.doc().unwrap().selection.objects, members, "selected whole");
    let all = needle_points(&s, group);
    let editing = s.stitch_editing(view).expect("editing");
    assert_eq!(editing.selected, all.len(), "every needle point of both, selected");
    assert_eq!(editing.selected, editing.scope.needles);
    // → moves the needle points 0.1 mm: the command's undo step.
    let undo = s.doc().unwrap().history.undo.len();
    assert!(s.tool_claims_key(ToolKey::Right, view), "the arrow is the tool's");
    s.tool_key(ToolKey::Right, Mods::default(), view).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    assert_eq!(s.doc().unwrap().history.undo.last().map(|e| e.label.clone()), Some("Move Needle Points".into()));
    let moved = needle_points(&s, group);
    let step = vectorcraft_stitch::mm_to_pt(0.1);
    assert!(all.iter().zip(&moved).all(|(a, b)| (b.x - a.x - step).abs() < 1e-6 && (b.y - a.y).abs() < 1e-6), "every needle point 0.1 mm right");
    // Delete deletes them as stitches.
    assert!(s.tool_claims_key(ToolKey::Delete, view));
    s.tool_key(ToolKey::Delete, Mods::default(), view).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.last().map(|e| e.label.clone()), Some("Delete Needle Points".into()));
    s.execute("edit.undo", &json!({})).unwrap();
    // Escape deselects first, isolation stays.
    assert!(s.tool_claims_key(ToolKey::Escape, view));
    s.tool_key(ToolKey::Escape, Mods::default(), view).unwrap();
    assert!(s.doc().unwrap().selection.objects.is_empty(), "deselected");
    assert_eq!(s.doc().unwrap().isolation, Some(NodeId(group)), "still isolated");
}

/// What stitch editing keeps follows what is edited: the needle point `[` and `]` step from goes
/// when editing ends by an agent's `object.exitIsolation`, and isn't carried into another object's
/// editing; pressed again, it comes back.
#[test]
fn the_needle_point_stepped_from_follows_the_object_edited() {
    let (mut s, objects) = fixture();
    let view = ViewInfo::default();
    let (a, b) = (objects[0], objects[1]);
    let current = |s: &Session| s.tool_options()["stitches"]["current"].clone();
    s.execute("object.isolate", &json!({"id": a})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("embroidery.sequence", &json!({})).unwrap();
    s.select_tool("embroidery", view).unwrap();
    let p = needle_points(&s, a)[5];
    for kind in [PointerKind::Down, PointerKind::Up] {
        s.pointer(&PointerEvent { kind, pos: p, mods: Mods::default(), backquote: false, pressure: 1.0, pen: None }, view).unwrap();
    }
    assert!(current(&s).is_object(), "pressed: {}", current(&s));
    s.execute("object.exitIsolation", &json!({})).unwrap();
    assert!(current(&s).is_null(), "editing ended: {}", current(&s));
    s.execute("object.isolate", &json!({"id": b})).unwrap();
    assert!(current(&s).is_null(), "another object: {}", current(&s));
    let q = needle_points(&s, b)[2];
    for kind in [PointerKind::Down, PointerKind::Up] {
        s.pointer(&PointerEvent { kind, pos: q, mods: Mods::default(), backquote: false, pressure: 1.0, pen: None }, view).unwrap();
    }
    assert!(current(&s).is_object(), "pressed in the other object: {}", current(&s));
}

/// Record numbers run through the whole design: after an edit of a path sewn earlier, a later
/// path's needle points have no number until the design is planned again, though their own path
/// is as it was.
#[test]
fn an_edit_of_a_path_sewn_before_takes_the_numbers_away() {
    let (mut s, objects) = fixture();
    let group = objects[4];
    s.execute("object.isolate", &json!({"id": group})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("embroidery.sequence", &json!({})).unwrap();
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let first_run = |member: &std::sync::Arc<vectorcraft_doc::Node>| member.children().unwrap()[0].id.0;
    let members = doc.node(NodeId(group)).unwrap().children().unwrap().clone();
    let (p1, p2) = (first_run(&members[0]), first_run(&members[1]));
    s.execute("embroidery.selectNeedlePoints", &json!({"needlePoints": [{"id": p2, "index": 3}]})).unwrap();
    let numbered = |s: &mut Session| s.stitch_editing(view).and_then(|e| e.ends).map(|[(_, a), _]| a);
    let before = numbered(&mut s).expect("ends");
    assert!(before.is_some(), "numbered by the plan");
    s.execute("embroidery.selectNeedlePoints", &json!({"needlePoints": [{"id": p1, "index": 3}]})).unwrap();
    let earlier = numbered(&mut s).flatten().expect("numbered");
    assert!(earlier < before.unwrap(), "the first member's run is sewn first");
    s.execute("embroidery.selectNeedlePoints", &json!({"needlePoints": [{"id": p2, "index": 3}]})).unwrap();
    s.execute("embroidery.deleteNeedlePoints", &json!({"needlePoints": [{"id": p1, "index": 2}]})).unwrap();
    assert_eq!(numbered(&mut s), Some(None), "no number until planned again");
}

/// After an Undo the needle points are found where they are now: a needle point moved, the move
/// undone, a press where it was before the move selects it.
#[test]
fn after_an_undo_a_press_finds_the_needle_point_where_it_is() {
    let (mut s, objects) = fixture();
    let id = objects[0];
    s.execute("object.isolate", &json!({"id": id})).unwrap();
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let mut run = None;
    doc.node(NodeId(id)).unwrap().walk(&mut |c| {
        if run.is_none() && c.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) {
            run = Some(c.id);
        }
    });
    let run = run.expect("a run");
    let at = doc.node(run).unwrap().path_data().unwrap().subpaths[0].anchors[3].p;
    s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": run.0, "index": 3}], "dx": 5.0})).unwrap();
    s.stitch_editing(view);
    s.execute("edit.undo", &json!({})).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), view).unwrap();
    s.pointer(&PointerEvent::new(PointerKind::Up, at.x, at.y), view).unwrap();
    let selected = s.doc().unwrap().selection.anchors.get(&run).cloned().unwrap_or_default();
    assert!(selected.contains(&(0, 3)), "the needle point back where it was is selected: {selected:?}");
}

/// Editing ends however it is left mid-drag, with nothing left open: an agent's exit from
/// isolation, the convert undone (the object is art again), another tool.
#[test]
fn editing_left_mid_drag_by_other_means_leaves_nothing_open() {
    let view = ViewInfo::default();
    for leave in ["exit", "undo the convert", "another tool"] {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
        s.execute("paint.setStroke", &json!({"none": true})).unwrap();
        s.execute("paint.setFill", &json!({"color": "#c81e1e"})).unwrap();
        let id = s.execute("shape.rectangle", &json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
        s.execute("select.set", &json!({"ids": [id]})).unwrap();
        s.execute("embroidery.convertToManual", &json!({})).unwrap();
        s.execute("object.isolate", &json!({"id": id})).unwrap();
        s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
        s.select_tool("embroidery", view).unwrap();
        assert!(s.stitch_editing(view).is_some(), "{leave}: editing");
        let points = needle_points(&s, id);
        let p = points[4];
        s.pointer(&PointerEvent::new(PointerKind::Down, p.x, p.y), view).unwrap();
        for k in 1..=4 {
            s.pointer(&PointerEvent::new(PointerKind::Drag, p.x + 3.0 * f64::from(k), p.y), view).unwrap();
            s.tool_tick(1.0 / 60.0, view).unwrap();
        }
        match leave {
            "exit" => {
                s.execute("object.exitIsolation", &json!({})).unwrap();
            }
            "undo the convert" => {
                // The drag's own step, then the convert's.
                s.execute("edit.undo", &json!({})).unwrap();
                s.execute("edit.undo", &json!({})).unwrap();
            }
            _ => s.select_tool("selection", view).unwrap(),
        }
        s.pointer(&PointerEvent::new(PointerKind::Up, p.x + 12.0, p.y), view).unwrap();
        assert!(!s.in_interaction(), "{leave}: no drag left open");
        assert!(s.stitch_editing(view).is_none(), "{leave}: editing ended");
        assert!(check_session(&s).is_ok(), "{leave}: {:?}", check_session(&s));
    }
}
