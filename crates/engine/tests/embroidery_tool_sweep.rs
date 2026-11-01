//! The embroidery tool swept: on the fixture of every kind of object it meets (`points`: fills,
//! strokes, both, a compound path, a group, a clip group, a symbol instance, a run opened from a
//! file, a locked and a hidden object, type), planned, with any selection, any part shown, and any
//! run of pointer events (on its points, on its angle arrows' heads and off them, hostile
//! coordinates among them), keys, whole drags of a handle ended by the release, Escape or the
//! selection deleted, at any zoom, in the outline view too, with points as hostile as a file can
//! hold on some objects: nothing panics, the session holds together, a drag adds at most one undo
//! step, Escape mid-drag puts the document back as it was at the press, and once the button is let
//! go and Escape pressed no drag is left open.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::Embroidery;
use vectorcraft_engine::{Session, ViewInfo};
use vectorcraft_testkit::embroidery as fx;
use vectorcraft_testkit::invariants::check_session;
use vectorcraft_tools::embroidery::Kind;
use vectorcraft_tools::{Mods, PointerEvent, PointerKind, ToolKey};

/// The panics the engine caught since the sweep began. A tool's Preview that fails is only logged
/// (the drag goes on), so a panic in the command it previews never reaches the caller: the sweep
/// counts every panic through the panic hook, which still runs when the engine catches one.
static CAUGHT: AtomicUsize = AtomicUsize::new(0);

/// Count panics in [`CAUGHT`] from now on (once for the whole run), still printing each as before.
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

/// One step of the sweep.
#[derive(Clone, Debug)]
enum Step {
    /// A pointer event at a point, near one of the fixture's points (by index) or anywhere.
    Pointer(PointerKind, Spot, Mods),
    Key(ToolKey, Mods),
    /// The part shown.
    Part(Option<&'static str>),
    /// The selection deleted (`edit.clear`), mid-drag or not.
    Delete,
    /// A drag of one of the handles shown (by index): pressed on it, moved by each offset (screen
    /// pixels) in turn, and ended so.
    Drag(usize, Vec<(f64, f64)>, End),
}

/// How a [`Step::Drag`] ends.
#[derive(Clone, Copy, Debug)]
enum End {
    Release,
    Escape,
    /// The selection deleted mid-drag, then the release.
    Delete,
}

/// The zooms the sweep views the fixture at: far out, as drawn, close in, the closest.
const ZOOMS: [f64; 4] = [0.05, 1.0, 16.0, 640.0];

/// A point as hostile as a file can hold one: at JSON's edges, of the wrong shape or type.
fn hostile_point() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![
        json!([1.7e308, -1.7e308]),
        json!([4e6, -4e6]),
        json!([1e-300, 0.0]),
        json!([1.0, 2.0, 3.0]),
        json!(["x", 1.0]),
        json!("nearest"),
        json!({"x": 1}),
        Value::Null,
    ])
}

/// An object's settings with hostile points on both parts.
fn hostile_points() -> impl Strategy<Value = Value> {
    (hostile_point(), hostile_point(), hostile_point(), hostile_point())
        .prop_map(|(a, b, c, d)| json!({"fill": {"points": {"entry": a, "exit": b}}, "stroke": {"points": {"entry": c, "exit": d}}}))
}

/// Where a pointer event lands.
#[derive(Clone, Copy, Debug)]
enum Spot {
    /// Within a few points of one of the fixture's set points.
    Near(usize, f64, f64),
    /// Within a few points of the head of one of the angle arrows shown (by index, among them).
    Head(usize, f64, f64),
    At(f64, f64),
}

/// Shift and Alt (Cmd lends the selection tool for the gesture: the session's, not this tool's).
fn arb_mods() -> impl Strategy<Value = Mods> {
    (any::<bool>(), prop::bool::weighted(0.15)).prop_map(|(shift, alt)| Mods { shift, alt, ..Mods::default() })
}

fn arb_spot() -> impl Strategy<Value = Spot> {
    let coord = prop_oneof![8 => -50.0..450.0f64, 1 => Just(f64::NAN), 1 => Just(1e300), 1 => Just(-1e300)];
    prop_oneof![
        3 => (0usize..8, -6.0..6.0f64, -6.0..6.0f64).prop_map(|(i, dx, dy)| Spot::Near(i, dx, dy)),
        2 => (0usize..8, -6.0..6.0f64, -6.0..6.0f64).prop_map(|(i, dx, dy)| Spot::Head(i, dx, dy)),
        2 => (coord.clone(), coord).prop_map(|(x, y)| Spot::At(x, y)),
    ]
}

fn arb_step() -> impl Strategy<Value = Step> {
    let kind =
        prop_oneof![Just(PointerKind::Move), Just(PointerKind::Down), Just(PointerKind::Drag), Just(PointerKind::Up), Just(PointerKind::DoubleClick)];
    let key = prop_oneof![
        Just(ToolKey::Enter),
        Just(ToolKey::Escape),
        Just(ToolKey::Delete),
        Just(ToolKey::Left),
        Just(ToolKey::Up),
        Just(ToolKey::BracketLeft),
        Just(ToolKey::BracketRight),
    ];
    prop_oneof![
        8 => (kind, arb_spot(), arb_mods()).prop_map(|(k, s, m)| Step::Pointer(k, s, m)),
        2 => (key, arb_mods()).prop_map(|(k, m)| Step::Key(k, m)),
        1 => prop::sample::select(vec![None, Some("fill"), Some("stroke"), Some("rail")]).prop_map(Step::Part),
        1 => Just(Step::Delete),
        4 => (0usize..16, prop::collection::vec((-60.0..60.0f64, -60.0..60.0f64), 1..5), prop::sample::select(vec![End::Release, End::Escape, End::Delete]))
            .prop_map(|(i, moves, end)| Step::Drag(i, moves, end)),
    ]
}

/// The fixture, planned once (the tool reads the plan the service remembers).
fn fixture() -> (Session, fx::Points, Vec<u64>) {
    let mut s = Session::new();
    let points = fx::points(&mut |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}")));
    s.execute("embroidery.sequence", &json!({})).unwrap();
    let ids: Vec<u64> = s.doc().unwrap().doc.layers.iter().filter_map(|l| l.children()).flatten().map(|n| n.id.0).collect();
    (s, points, ids)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn the_embroidery_tool_never_breaks_a_session(
        picked in prop::collection::vec(any::<bool>(), 16),
        hostile in prop::collection::vec(prop::option::weighted(0.2, hostile_points()), 16),
        zoom in prop::sample::select(ZOOMS.to_vec()),
        outline in prop::bool::weighted(0.2),
        steps in prop::collection::vec(arb_step(), 1..40),
    ) {
        count_panics();
        let caught = CAUGHT.load(Ordering::SeqCst);
        let (mut s, p, ids) = fixture();
        // Hostile points on some objects, written as a file would hold them (no command takes them).
        s.edit("Hostile Points", |d, _| {
            for (id, e) in ids.iter().zip(&hostile) {
                if let (Some(e), Some(n)) = (e, d.node_mut(NodeId(*id))) {
                    n.extras_mut().embroidery = Some(Box::new(Embroidery::from(e.clone())));
                }
            }
            Ok(())
        })
        .unwrap();
        no_panic(s.execute("embroidery.sequence", &json!({})))?;
        let selected: Vec<u64> = ids.iter().zip(picked.iter().cycle()).filter(|(_, on)| **on).map(|(id, _)| *id).collect();
        s.execute("select.set", &json!({"ids": selected})).unwrap();
        let view = ViewInfo { zoom, outline, ..ViewInfo::default() };
        s.select_tool("embroidery", view).unwrap();
        let undo = |s: &Session| s.doc().map_or(0, |d| d.history.undo.len());
        let spots = [p.fill_entry, p.fill_exit, p.stroke_entry, p.stroke_exit, p.both_fill_entry, p.both_stroke_exit, [200.0, 200.0], [10.0, 10.0]];
        for step in &steps {
            match *step {
                Step::Pointer(kind, spot, mods) => {
                    let (x, y) = match spot {
                        Spot::Near(i, dx, dy) => {
                            let [x, y] = spots[i % spots.len()];
                            (x + dx, y + dy)
                        }
                        Spot::Head(i, dx, dy) => {
                            let (handles, _) = s.embroidery_handles(view);
                            let heads: Vec<_> = handles.iter().filter(|h| h.id.kind == Kind::Angle).map(|h| h.point).collect();
                            heads.get(i % heads.len().max(1)).map_or((dx, dy), |p| (p.x + dx, p.y + dy))
                        }
                        Spot::At(x, y) => (x, y),
                    };
                    no_panic(s.pointer(&PointerEvent::new(kind, x, y).with_mods(mods), view))?;
                }
                Step::Key(key, mods) => {
                    no_panic(s.tool_key(key, mods, view))?;
                }
                Step::Part(part) => s.set_tool_option("part", &part.map_or(Value::Null, |p| json!(p))),
                Step::Delete => {
                    no_panic(s.execute("edit.clear", &json!({})))?;
                }
                Step::Drag(i, ref moves, end) => {
                    // A gesture of its own: whatever the steps before left open ends first.
                    no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, 0.0, 0.0), view))?;
                    no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
                    let (handles, _) = s.embroidery_handles(view);
                    let Some(at) = handles.get(i % handles.len().max(1)).map(|h| h.at) else { continue };
                    let (doc, steps_before) = (s.doc().map(|d| d.doc.clone()).ok(), undo(&s));
                    no_panic(s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), view))?;
                    let mut to = at;
                    for (dx, dy) in moves {
                        to = vectorcraft_geom::Point::new(at.x + dx / zoom, at.y + dy / zoom);
                        no_panic(s.pointer(&PointerEvent::new(PointerKind::Drag, to.x, to.y), view))?;
                    }
                    match end {
                        End::Release => {
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.x, to.y), view))?;
                            prop_assert!(undo(&s) <= steps_before + 1, "a drag made {} undo steps: {steps:?}", undo(&s) - steps_before);
                        }
                        End::Escape => {
                            let dragging = s.in_interaction();
                            no_panic(s.tool_key(ToolKey::Escape, Mods::default(), view))?;
                            if dragging {
                                prop_assert!(s.doc().ok().map(|d| d.doc.clone()) == doc, "Escape mid-drag left the document changed: {steps:?}");
                                prop_assert_eq!(undo(&s), steps_before, "Escape mid-drag left an undo step");
                            }
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.x, to.y), view))?;
                        }
                        End::Delete => {
                            no_panic(s.execute("edit.clear", &json!({})))?;
                            no_panic(s.pointer(&PointerEvent::new(PointerKind::Up, to.x, to.y), view))?;
                        }
                    }
                }
            }
            // Mid-drag an interaction is open, as it should be; between drags the session holds.
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

/// The objects that take no points get no handles, whichever part the tool shows: a symbol instance
/// (its art takes them), type, and a run of stitches read from a file, sewn as it is; a square
/// selected with them does. The instance has planned ends, so only the rule of which objects take
/// settings keeps its handles away.
#[test]
fn objects_that_take_no_points_have_no_handles() {
    let (mut s, p, _) = fixture();
    s.execute("select.set", &json!({"ids": [p.instance, p.text, p.run, p.fill]})).unwrap();
    let selection = s.doc().unwrap().selection.clone();
    assert!([p.instance, p.text, p.run, p.fill].iter().all(|id| selection.contains(NodeId(*id))), "all selected: {selection:?}");
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    for part in [Value::Null, json!("fill"), json!("stroke")] {
        s.set_tool_option("part", &part);
        let owners: Vec<u64> = s.embroidery_handles(view).0.iter().map(|h| h.id.owner.part().0.0).collect();
        for (name, id) in [("the instance", p.instance), ("the type", p.text), ("the run from a file", p.run)] {
            assert!(!owners.contains(&id), "{name} has handles, the {part} shown: {owners:?}");
        }
        if part != json!("stroke") {
            assert!(owners.contains(&p.fill), "the square has its handles, the {part} shown: {owners:?}");
        }
    }
}

/// Over an object a press wouldn't select (type) the tool shows NotAllowed; over a run of stitches
/// read from a file, which a press selects and a double-click edits needle point by needle point
/// (the project owner's choice, 2026-10-27), over a square that takes points, and over nothing, its
/// own cursor.
#[test]
fn the_cursor_says_where_a_press_selects_nothing() {
    use vectorcraft_geom::Point;
    use vectorcraft_tools::Cursor;
    let (mut s, p, _) = fixture();
    let view = ViewInfo::default();
    s.select_tool("embroidery", view).unwrap();
    // The run's first stitch, the corner of its bounds; the type's middle.
    let run = s.doc().unwrap().doc.node(NodeId(p.run)).and_then(|n| n.geometric_bounds()).unwrap();
    let text = s.doc().unwrap().doc.bounds_of(&[NodeId(p.text)], false).unwrap().center();
    let [x, y] = p.fill_entry;
    for (at, cursor, what) in [
        (text, Cursor::NotAllowed, "the type"),
        (Point::new(run.x0, run.y0), Cursor::Embroidery, "the run from a file"),
        (Point::new(x + 10.0, y), Cursor::Embroidery, "inside the square"),
        (Point::new(10.0, 10.0), Cursor::Embroidery, "nothing"),
    ] {
        assert_eq!(s.cursor(at, Mods::default(), view), cursor, "over {what}");
    }
}
