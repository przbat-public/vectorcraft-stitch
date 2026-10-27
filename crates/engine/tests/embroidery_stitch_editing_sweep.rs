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

use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::{Session, ViewInfo};
use vectorcraft_geom::Point;
use vectorcraft_testkit::embroidery as fx;
use vectorcraft_testkit::invariants::check_session;
use vectorcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};

/// The panics the engine caught since the sweep began: a tool's Preview that fails is only logged,
/// so the sweep counts every panic through the panic hook, which still runs when the engine catches
/// one.
static CAUGHT: AtomicUsize = AtomicUsize::new(0);

fn count_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let printed = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            CAUGHT.fetch_add(1, Ordering::SeqCst);
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
    /// A drag from a spot, by each offset (screen pixels) in turn, ticked after each, ended so.
    Drag(Spot, Vec<(f64, f64)>, End),
}

#[derive(Clone, Copy, Debug)]
enum End {
    Release,
    Escape,
    Delete,
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
        4 => (arb_spot(), prop::collection::vec((-80.0..80.0f64, -80.0..80.0f64), 1..5), prop::sample::select(vec![End::Release, End::Escape, End::Delete]))
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
    #![proptest_config(ProptestConfig { cases: 40, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn stitch_editing_never_breaks_a_session(
        object in 0usize..11,
        zoom in prop::sample::select(ZOOMS.to_vec()),
        steps in prop::collection::vec(arb_step(), 1..40),
    ) {
        count_panics();
        let caught = CAUGHT.load(Ordering::SeqCst);
        let (mut s, objects) = fixture();
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
                    }
                }
            }
            if !s.in_interaction() {
                prop_assert!(check_session(&s).is_ok(), "{:?} after {step:?}", check_session(&s));
            }
        }
        no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, 0.0, 0.0), view))?;
        no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
        prop_assert_eq!(CAUGHT.load(Ordering::SeqCst), caught, "a panic was caught (a Preview's, say) after {:?}", steps);
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
    assert_eq!(s.doc().unwrap().doc, by_command.doc().unwrap().doc, "the drag is the command");
}

/// The lasso of stitch editing is the Lasso tool (the project owner's choice, 2026-10-27): in
/// isolation of a converted object it selects the needle points inside its loop, and back with the
/// Embroidery Tool they are being edited (a drag moves them).
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
    // A loop round the object's left half.
    let mid = (lo.x + hi.x) / 2.0;
    let path = [(lo.x - 5.0, lo.y - 5.0), (mid, lo.y - 5.0), (mid, hi.y + 5.0), (lo.x - 5.0, hi.y + 5.0), (lo.x - 5.0, lo.y - 5.0)];
    s.pointer(&PointerEvent::new(PointerKind::Down, path[0].0, path[0].1), view).unwrap();
    for (x, y) in &path[1..] {
        s.pointer(&PointerEvent::new(PointerKind::Drag, *x, *y), view).unwrap();
    }
    s.pointer(&PointerEvent::new(PointerKind::Up, path[0].0, path[0].1), view).unwrap();
    let inside = points.iter().filter(|p| p.x < mid).count();
    let selected: usize = s.doc().unwrap().selection.anchors.values().map(std::collections::BTreeSet::len).sum();
    assert!(selected > 0 && selected <= inside, "the lasso selected {selected} of the {inside} anchors inside it");
    s.select_tool("embroidery", view).unwrap();
    let editing = s.stitch_editing(view).expect("editing");
    assert!(editing.selected > 0, "back with the Embroidery Tool, the lasso's needle points are selected: {editing:?}");
}
