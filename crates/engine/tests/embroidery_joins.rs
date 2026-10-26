//! `embroidery.nearestConnection` on random designs of running lines, loops, satin lines and filled
//! rectangles, in one or two colours, some cut after: the moves between parts never cost more after
//! it than before (by the planner's weights: a millimetre of move, a jump 20, a cut 50 more, a needle
//! point a half), what it answers is what the design then sews, and run again it changes nothing.
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
/// The weight of a needle point, in millimetres of move: a 2 mm travel stitch hidden under stitching
/// still to come, weighed a quarter (`plan::stitches_cost`).
const NEEDLE_POINT: f64 = 0.5;

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

/// What moves of `m` (`before` or `after`), and its needle points, cost by the planner's weights.
fn cost(m: &Value) -> f64 {
    m["length"].as_f64().unwrap()
        + JUMP * m["jumps"].as_f64().unwrap()
        + CUT * m["trims"].as_f64().unwrap()
        + NEEDLE_POINT * m["stitches"].as_f64().unwrap()
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

/// A square in a second colour starts where the needle is; given that point as its entry (in document
/// points, a float's width off once back in millimetres), its underlay's first row ties with
/// another and sews five needle points more. What the command answers is what the design sews after
/// it, those five included.
#[test]
fn it_answers_what_the_points_it_sets_sew() {
    let mut s = design(&[
        (Kind::Line, (114.33416647484525, 16.38172117414631), (8.0, 0.0), false, false),
        (Kind::Square, (97.85766699578129, 6.52305527944707), (17.7168502205124, 0.0), true, false),
    ]);
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    let again = run(&mut s, "embroidery.nearestConnection", json!({"dryRun": true}));
    assert_eq!(again["before"], r["after"], "what it answers is what the design sews");
}

/// A square sewn from the needle can't be given the exit that saves 6 mm of the move after it; once
/// it holds its own entry it can. The points are set first, so the run finds that join, and a second
/// run changes nothing.
#[test]
fn a_second_run_starts_where_the_first_ended() {
    let mut s = design(&[
        (Kind::Square, (11.776786608248061, 5.0), (18.98142599316573, 0.0), false, false),
        (Kind::Line, (5.0, 5.0), (8.0, -4.857692351538571), true, false),
        (Kind::Line, (113.64832025413213, 5.0), (8.0, 0.0), true, false),
        (Kind::Line, (20.896811924697563, 80.75385473297587), (17.47295709499661, -5.20580310791632), false, false),
    ]);
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert!(cost(&r["after"]) < cost(&r["before"]), "{r}");
    let second = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(second["changedCount"], json!(0), "run again: {second}");
}
