//! The embroidery fixtures the stitch preview's tests share (`vectorcraft_testkit::embroidery`):
//! each one plans, and its plan holds together: stitches numbered from 1 without a gap, each
//! block's units covering its stitches one after another, each unit's runs inside its stitches,
//! and each unit naming an object of the document planned.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use vectorcraft_engine::cmd::embroidery::plan::{Choice, DesignPlan, FIRST_NUMBER};
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

/// The manual fixture plans, the document holds together, every format writes it, and each of its
/// runs is what it says: the designs opened from DST, PES, JEF and VP3 files hold the file's runs
/// (three threads, sewn as their stitches are), the run of one needle point and the tack are two
/// anchors at one spot (only the tack marked so), the cuts and stops are marked, the curved run has
/// handles, the instance places a run, the locked run is on a locked layer and the hidden one isn't
/// sewn; each converted object is a group of manual runs keeping its original, sewn as manual
/// stitches (the clip member in its clip group).
#[test]
fn the_manual_fixture_holds_each_kind_of_run() {
    use vectorcraft_doc::embroidery::StitchRun;
    use vectorcraft_doc::{Node, NodeKind};
    let mut s = Session::new();
    let mut run = runner(&mut s);
    let m = fx::manual(&mut run);
    drop(run);
    assert_eq!(s.documents().len(), 1, "the fixture is the one document left open");
    vectorcraft_testkit::invariants::check_session(&s).unwrap();
    let plan = checked_plan(&mut s);
    let sewn: Vec<u64> = plan.units.iter().map(|u| u.origin.selected().0).collect();
    for o in &m.opened {
        assert!(!o.runs.is_empty(), "{}: runs pasted", o.format);
        for r in &o.runs {
            assert!(sewn.contains(r), "{}: run {r} is sewn", o.format);
        }
    }
    for id in [m.one_point, m.double_point, m.trim_before, m.trim_after, m.curved, m.instance, m.locked, m.stopped] {
        assert!(sewn.contains(&id), "{id} is sewn: {m:?}");
    }
    assert!(!sewn.contains(&m.hidden), "the hidden run isn't sewn");
    assert!(plan.units.iter().all(|u| u.params.is_none()), "every unit is a run sewn as its stitches are");
    let doc = s.doc().unwrap().doc.clone();
    let node = |id: u64| -> &Node { doc.node(vectorcraft_doc::NodeId(id)).unwrap() };
    let stitches = |id: u64| -> StitchRun { node(id).extras().embroidery.as_deref().and_then(|e| e.stitches.clone()).unwrap() };
    let anchors = |id: u64| node(id).path_data().unwrap().subpaths[0].anchors.clone();
    for id in [m.one_point, m.double_point] {
        let a = anchors(id);
        assert!(a.len() == 2 && a[0].p == a[1].p, "{id}: two anchors at one spot");
    }
    assert_eq!((stitches(m.one_point).double_point, stitches(m.double_point).double_point), (false, true));
    assert!(stitches(m.trim_before).trim_before && stitches(m.trim_after).trim_after);
    assert!(anchors(m.curved).iter().any(|a| a.h_in != a.p || a.h_out != a.p), "the curved run has handles");
    assert!(matches!(node(m.instance).kind, NodeKind::SymbolInstance { .. }));
    assert!(!node(m.hidden).visible);
    assert!(!doc.is_editable(vectorcraft_doc::NodeId(m.locked)), "the locked run can't be edited");
    let stopped = stitches(m.stopped);
    assert_eq!((stopped.stops, stopped.stop_after), (vec![(0, 1)], true));
    // Each converted object: a group keeping its id, holding runs marked manual, its original kept,
    // sewn as manual stitches; the group given whole holds its two members converted.
    let c = m.converted;
    let members: Vec<u64> = node(c.group).children().unwrap().iter().map(|n| n.id.0).collect();
    assert_eq!(members.len(), 2);
    for id in [c.fill, c.stroke, c.both, c.compound, c.clip_member, c.expanded].into_iter().chain(members) {
        let group = node(id);
        let runs = group.children().unwrap_or_else(|| panic!("{id}: a group"));
        assert!(!runs.is_empty() && runs.iter().all(|r| stitches(r.id.0).manual), "{id}: runs marked manual");
        assert!(group.extras().embroidery.as_deref().is_some_and(|e| e.original.is_some()), "{id}: its original kept");
        assert!(plan.units.iter().any(|u| u.origin.selected().0 == id && u.why == Choice::Manual), "{id}: sewn as manual stitches");
    }
    let parent = doc.parent_of(vectorcraft_doc::NodeId(c.clip_member)).and_then(|p| doc.node(p));
    assert!(parent.is_some_and(Node::clips), "the clip member converted in its clip group");
    for format in vectorcraft_stitch::format::Format::ALL {
        let reply = s.execute("document.export", &serde_json::json!({"format": format.id()}));
        assert!(reply.is_ok(), "{}: {reply:?}", format.id());
    }
}
