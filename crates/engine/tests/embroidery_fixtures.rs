//! The embroidery fixtures the stitch preview's tests share (`vectorcraft_testkit::embroidery`):
//! each one plans, and its plan holds together: stitches numbered from 1 without a gap, each
//! block's units covering its stitches one after another, each unit's runs inside its stitches,
//! and each unit naming an object of the document planned.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use vectorcraft_engine::cmd::embroidery::plan::{DesignPlan, FIRST_NUMBER};
use vectorcraft_engine::{EmbroideryOptions, Session};
use vectorcraft_testkit::embroidery as fx;

/// A runner for the fixtures over `s`, panicking with the command and its params on an error.
fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The active document's plan, and the document it plans, checked to hold together.
fn checked_plan(s: &mut Session) -> DesignPlan {
    let planner = s.embroidery_planner().unwrap();
    let plan = planner.plan(&EmbroideryOptions::default()).unwrap();
    let doc = planner.document();
    let mut number = FIRST_NUMBER;
    for (k, b) in plan.blocks.iter().enumerate() {
        assert_eq!(b.first_number, number, "block {k} numbered on from the one before");
        number += b.stitches.len();
        let mut next = 0;
        for i in b.units.clone() {
            let u = &plan.units[i];
            assert_eq!(u.block, k, "unit {i} in its block");
            assert_eq!(u.stitches.start, next, "unit {i} follows the one before");
            next = u.stitches.end;
            for r in &u.runs {
                assert!(u.stitches.start <= r.start && r.end <= u.stitches.end, "unit {i}'s run {r:?} is its own");
                assert!(b.stitches[r.clone()].iter().all(|s| s.command.needle_down()), "unit {i}'s run {r:?} is needle points");
            }
            assert!(doc.node(u.origin.selected()).is_some(), "unit {i} names an object of the document");
        }
        assert_eq!(next, b.stitches.len(), "block {k}'s units cover its stitches");
    }
    (*plan).clone()
}

#[test]
fn three_threads_join_apart_and_split_on_top() {
    for (overlap, blocks) in [(false, 2), (true, 3)] {
        let mut s = Session::new();
        let mut run = runner(&mut s);
        fx::blank(&mut run);
        let ids = fx::three_threads(&mut run, overlap);
        drop(run);
        let plan = checked_plan(&mut s);
        assert_eq!(plan.blocks.len(), blocks, "overlap {overlap}");
        let sewn: Vec<u64> = plan.units.iter().map(|u| u.origin.selected().0).collect();
        // Apart, both reds join one block sewn first; on top of each other, stacking order.
        let order = if overlap { ids.to_vec() } else { vec![ids[0], ids[2], ids[1]] };
        assert_eq!(sewn, order, "overlap {overlap}");
    }
}

#[test]
fn the_structures_plan_and_map_back_to_their_objects() {
    let mut s = Session::new();
    let mut run = runner(&mut s);
    let st = fx::structures(&mut run);
    drop(run);
    let plan = checked_plan(&mut s);
    let sewn: Vec<u64> = plan.units.iter().map(|u| u.origin.selected().0).collect();
    for id in st.grouped.iter().chain(&st.instances).chain([&st.filled_and_stroked, &st.locked]) {
        assert!(sewn.contains(id), "{id} is sewn: {sewn:?}");
    }
    assert!(!sewn.contains(&st.hidden) && !sewn.contains(&st.text), "hidden art and type aren't sewn");
    // Both instances of the symbol are sewn, each its own unit placed by it.
    for i in st.instances {
        assert_eq!(sewn.iter().filter(|&&id| id == i).count(), 1, "instance {i}");
    }
    // The object filled and stroked is sewn twice: its fill and its stroke.
    assert_eq!(sewn.iter().filter(|&&id| id == st.filled_and_stroked).count(), 2);
    assert!(!plan.warnings.is_empty(), "type left out says so");
}

#[test]
fn an_opened_design_plans_as_its_literal_runs() {
    for format in ["pes", "dst"] {
        let mut s = Session::new();
        let mut run = runner(&mut s);
        fx::opened_design(&mut run, format);
        drop(run);
        let plan = checked_plan(&mut s);
        assert!(!plan.units.is_empty(), "{format}");
        assert!(plan.units.iter().all(|u| u.params.is_none()), "{format}: every unit is a literal run");
    }
}

#[test]
fn the_satin_and_running_weights_are_sewn_so() {
    use vectorcraft_engine::cmd::embroidery::plan::Choice;
    let mut s = Session::new();
    let mut run = runner(&mut s);
    fx::blank(&mut run);
    fx::stroke(&mut run, fx::RED, fx::SATIN_WEIGHT, &[(20.0, 20.0), (200.0, 40.0)]);
    fx::stroke(&mut run, fx::BLUE, fx::RUNNING_WEIGHT, &[(20.0, 100.0), (200.0, 120.0)]);
    fx::letter_t(&mut run, fx::GREEN, 20.0, 200.0);
    drop(run);
    let plan = checked_plan(&mut s);
    let why: Vec<Choice> = plan.units.iter().map(|u| u.why).collect();
    assert_eq!(why, [Choice::WideStroke, Choice::NarrowStroke, Choice::Fill]);
    assert!(plan.units[2].columns.is_some(), "the T's stem is a satin column");
}

/// The points fixture plans, every object that takes points holds the ones set on it (read back
/// by `embroidery.info`, document points), and what takes none is what it is: the instance is
/// placed, the run is literal stitches, the hidden object and the type aren't sewn.
#[test]
fn the_points_fixture_plans_and_holds_its_points() {
    let mut s = Session::new();
    let mut run = runner(&mut s);
    let p = fx::points(&mut run);
    drop(run);
    let plan = checked_plan(&mut s);
    let sewn: Vec<u64> = plan.units.iter().map(|u| u.origin.selected().0).collect();
    for id in [p.fill, p.stroke, p.both, p.compound, p.grouped[0], p.grouped[1], p.clipped, p.instance, p.run, p.locked] {
        assert!(sewn.contains(&id), "{id} is sewn: {sewn:?} ({p:?})");
    }
    assert!(!sewn.contains(&p.hidden) && !sewn.contains(&p.text), "hidden art and type aren't sewn");
    let literal = plan.units.iter().filter(|u| u.origin.selected().0 == p.run).all(|u| u.params.is_none());
    assert!(literal, "the pasted run is sewn as its stitches are");
    let info = |s: &mut Session, id: u64, part: &str, key: &str| -> Value {
        let reply = s.execute("embroidery.info", &serde_json::json!({"ids": [id]})).unwrap();
        reply[part][key]["value"].clone()
    };
    assert_eq!(info(&mut s, p.fill, "fill", "entry"), serde_json::json!(p.fill_entry));
    assert_eq!(info(&mut s, p.fill, "fill", "exit"), serde_json::json!(p.fill_exit));
    assert_eq!(info(&mut s, p.stroke, "stroke", "entry"), serde_json::json!(p.stroke_entry));
    assert_eq!(info(&mut s, p.stroke, "stroke", "exit"), serde_json::json!(p.stroke_exit));
    assert_eq!(info(&mut s, p.both, "fill", "entry"), serde_json::json!(p.both_fill_entry));
    assert_eq!(info(&mut s, p.both, "stroke", "exit"), serde_json::json!(p.both_stroke_exit));
    assert_eq!(info(&mut s, p.both, "fill", "exit"), Value::Null, "only the points given are set");
    assert_eq!(info(&mut s, p.compound, "fill", "entry"), serde_json::json!(p.compound_entry));
}
