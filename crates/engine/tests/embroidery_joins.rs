//! `embroidery.nearestConnection` on random designs of running lines, loops, satin lines and filled
//! rectangles, in one or two colours, some cut after: the moves between parts never cost more after
//! it than before (by the planner's weights: a millimetre of move, a jump 20, a cut 50 more), what
//! it answers is what the design then sews, and run again it changes nothing.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_engine::Session;

/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// The planner's weights of a jump and of a cut, in millimetres of move (`plan::runs`).
const JUMP: f64 = 20.0;
const CUT: f64 = 50.0;

/// One object: what it is, where (millimetres), its colour (of two), and whether its thread is cut
/// after it.
#[derive(Clone, Debug)]
enum Kind {
    Line,
    Loop,
    Satin,
    Square,
}

/// An object: its kind, where it starts, how long it is and how far it rises (millimetres), whether
/// it is in the second colour, and whether its thread is cut after it.
type Object = (Kind, (f64, f64), (f64, f64), bool, bool);

fn arb_object() -> impl Strategy<Value = Object> {
    (
        prop_oneof![Just(Kind::Line), Just(Kind::Loop), Just(Kind::Satin), Just(Kind::Square)],
        (5.0..120.0f64, 5.0..120.0f64),
        (8.0..30.0f64, -20.0..20.0f64),
        any::<bool>(),
        prop::bool::weighted(0.2),
    )
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn design(objects: &[Object]) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 500, "height": 500, "artboards": 1}));
    for (kind, (x, y), (len, rise), second, trim) in objects {
        let color = if *second { "#22aa44" } else { "#2233cc" };
        let pt = |v: f64| v * PT_PER_MM;
        let id = match kind {
            Kind::Square => {
                run(&mut s, "paint.setStroke", json!({"none": true}));
                run(&mut s, "paint.setFill", json!({"color": color}));
                run(&mut s, "shape.rectangle", json!({"x": pt(*x), "y": pt(*y), "width": pt(*len), "height": pt(len / 2.0)}))
            }
            Kind::Loop => {
                run(&mut s, "paint.setFill", json!({"none": true}));
                run(&mut s, "paint.setStroke", json!({"color": color}));
                let id = run(&mut s, "shape.ellipse", json!({"x": pt(*x), "y": pt(*y), "width": pt(*len), "height": pt(len / 2.0)}));
                run(&mut s, "stroke.set", json!({"weight": 1.0, "ids": [id["id"]]}));
                id
            }
            Kind::Line | Kind::Satin => {
                run(&mut s, "paint.setFill", json!({"none": true}));
                run(&mut s, "paint.setStroke", json!({"color": color}));
                let d = format!("M {} {} L {} {}", pt(*x), pt(*y), pt(x + len), pt(y + rise));
                let id = run(&mut s, "path.create", json!({ "d": d }));
                run(&mut s, "stroke.set", json!({"weight": if matches!(kind, Kind::Satin) { 6.0 } else { 1.0 }, "ids": [id["id"]]}));
                id
            }
        };
        if *trim {
            run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id["id"]], "on": true}));
        }
        run(&mut s, "select.none", json!({}));
    }
    s
}

/// What moves of `m` (`before` or `after`) cost by the planner's weights.
fn cost(m: &Value) -> f64 {
    m["length"].as_f64().unwrap() + JUMP * m["jumps"].as_f64().unwrap() + CUT * m["trims"].as_f64().unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn connections_never_get_worse(objects in prop::collection::vec(arb_object(), 2..7)) {
        let mut s = design(&objects);
        let r = run(&mut s, "embroidery.nearestConnection", json!({}));
        // Lengths are given to a hundredth of a millimetre.
        prop_assert!(cost(&r["after"]) <= cost(&r["before"]) + 0.05, "{r}");
        let again = run(&mut s, "embroidery.nearestConnection", json!({"dryRun": true}));
        prop_assert_eq!(&again["before"], &r["after"], "what it answers is what the design sews");
        let second = run(&mut s, "embroidery.nearestConnection", json!({}));
        prop_assert_eq!(second["changedCount"].clone(), json!(0), "run again: {}", second);
    }
}
