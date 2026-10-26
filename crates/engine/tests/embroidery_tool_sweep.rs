//! The embroidery tool swept: on the fixture of every kind of object it meets (`points`: fills,
//! strokes, both, a compound path, a group, a clip group, a symbol instance, a run opened from a
//! file, a locked and a hidden object, type), planned, with any selection, any part shown, and any
//! run of pointer events (on its points, on its angle arrows' heads and off them, hostile
//! coordinates among them) and keys:
//! nothing panics, the session holds together, and once the button is let go and Escape pressed no
//! drag is left open.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;
use serde_json::{Value, json};
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
        steps in prop::collection::vec(arb_step(), 1..40),
    ) {
        count_panics();
        let caught = CAUGHT.load(Ordering::SeqCst);
        let (mut s, p, ids) = fixture();
        let selected: Vec<u64> = ids.iter().zip(picked.iter().cycle()).filter(|(_, on)| **on).map(|(id, _)| *id).collect();
        s.execute("select.set", &json!({"ids": selected})).unwrap();
        let view = ViewInfo::default();
        s.select_tool("embroidery", view).unwrap();
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
