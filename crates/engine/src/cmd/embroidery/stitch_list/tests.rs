//! The Stitches tab's read side against `embroidery.stitches`, which lists the same records with
//! the commands' own code: each row's kind, place and length, the filters' picks and counts, a
//! scope's records, and the needle point each record is edited by.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_testkit::embroidery as fx;

use super::super::check::StitchLimits;
use super::super::test_support::run;
use super::{Filter, Flags, edited_by, leading, numbering, ordinals, row, scope};
use crate::Session;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The manual fixture, planned, and its plan.
fn planned() -> (Session, fx::Manual, std::sync::Arc<super::super::plan::DesignPlan>) {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    run(&mut s, "embroidery.sequence", json!({}));
    let uid = s.doc().unwrap().uid;
    let facts = s.embroidery_plans.tool_facts(uid, &super::super::Options::default()).unwrap();
    let plan = facts.plan();
    (s, m, plan)
}

/// Every record's row equals what `embroidery.stitches` lists for it: its kind, place, length,
/// object and path; and the needle point it is edited by, where the list names one.
#[test]
fn rows_are_what_the_stitch_list_lists() {
    let (mut s, _, plan) = planned();
    let doc = s.doc().unwrap().doc.clone();
    let listed = run(&mut s, "embroidery.stitches", json!({"limit": 10000}));
    let records = listed["records"].as_array().unwrap();
    assert!(records.len() > 100);
    let mut numbered: std::collections::HashMap<usize, Option<(NodeId, usize)>> = std::collections::HashMap::new();
    for r in records {
        let n = r["record"].as_u64().unwrap() as usize;
        let row = row(&plan, n).unwrap();
        assert_eq!(row.kind.id(), r["kind"].as_str().unwrap(), "record {n}");
        let mm = |v: &Value| v.as_f64().unwrap();
        assert!((row.at.x - mm(&r["at"][0])).abs() < 1e-9 && (row.at.y - mm(&r["at"][1])).abs() < 1e-9, "record {n}");
        match (row.length, r["length"].as_f64()) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "record {n}: {a} {b}"),
            (a, b) => assert_eq!(a, b, "record {n}"),
        }
        // The needle point it is edited by, from the unit's numbering and ordinals; before the
        // unit's own records (an opened design's run), from the needle points they lead from, and
        // a cut there is the path's whose needle point it follows.
        let unit = row.unit;
        let edited = *numbered.entry(unit).or_insert_with(|| numbering(&plan, &doc, unit));
        let lead = edited.map(|(_, first)| leading(&plan, &doc, unit, first)).unwrap_or_default();
        let address = match lead.iter().find(|(at, _)| *at == n) {
            Some((_, a)) => Some(*a),
            None => edited.and_then(|(node, first)| edited_by(&plan, unit, first, &ordinals(&plan, unit), n, row.kind).map(|k| (node, k))),
        };
        let node = address.map_or(row.node, |(node, _)| node);
        assert_eq!((row.object.0, node.0), (r["id"].as_u64().unwrap(), r["node"].as_u64().unwrap()), "record {n}");
        assert_eq!(address.map(|(_, k)| k as u64), r["index"].as_u64(), "record {n}: {r}");
    }
    assert_eq!(row(&plan, 0), None);
    assert_eq!(row(&plan, usize::MAX), None);
}

/// The filters pick what `embroidery.stitches` picks for each kind; the short and long ones, the
/// stitches the limits flag.
#[test]
fn the_filters_pick_what_the_list_picks() {
    let (mut s, _, plan) = planned();
    let doc = s.doc().unwrap().doc.clone();
    let limits = StitchLimits::of(&doc, None);
    let flags = Flags::of(&plan, &limits);
    let total = run(&mut s, "embroidery.stitches", json!({"limit": 0}))["recordCount"].as_u64().unwrap() as usize;
    assert_eq!(flags.len(), total);
    for (filter, kinds) in [(Filter::Jumps, "jump"), (Filter::Trims, "trim"), (Filter::Stops, "stop")] {
        let want = run(&mut s, "embroidery.stitches", json!({"kinds": [kinds], "limit": 10000}))["matchedCount"].as_u64().unwrap() as usize;
        let got = (1..=total).filter(|n| flags.matches(*n, filter)).count();
        assert_eq!(got, want, "{filter:?}");
    }
    let short = run(&mut s, "embroidery.stitches", json!({"shorterThan": limits.shortest, "kinds": ["stitch"], "limit": 0}))["matchedCount"]
        .as_u64()
        .unwrap() as usize;
    assert_eq!((1..=total).filter(|n| flags.matches(*n, Filter::Short)).count(), short);
    assert_eq!((1..=total).filter(|n| flags.matches(*n, Filter::All)).count(), total);
    assert!(!flags.matches(0, Filter::All) && !flags.matches(total + 1, Filter::All));
}

/// A scope is the records of the units sewing the objects given and what they hold: a converted
/// object's runs' records, a file's run's, a group's members', as `embroidery.stitches {ids}`
/// lists them for the objects themselves.
#[test]
fn a_scope_is_its_objects_records() {
    let (mut s, m, plan) = planned();
    let doc = s.doc().unwrap().doc.clone();
    let members: Vec<u64> = doc.node(NodeId(m.converted.group)).unwrap().children().unwrap().iter().map(|c| c.id.0).collect();
    for (given, ids) in
        [(vec![m.converted.fill], vec![m.converted.fill]), (vec![m.trim_after], vec![m.trim_after]), (vec![m.converted.group], members)]
    {
        let given: Vec<NodeId> = given.into_iter().map(NodeId).collect();
        let ranges = scope(&plan, &doc, &given);
        let got: Vec<usize> = ranges.iter().flat_map(|r| r.clone()).collect();
        let id = given[0].0;
        let listed = run(&mut s, "embroidery.stitches", json!({"ids": ids, "limit": 10000}));
        let want: Vec<usize> = listed["records"].as_array().unwrap().iter().map(|r| r["record"].as_u64().unwrap() as usize).collect();
        assert_eq!(got, want, "object {id}");
    }
    assert!(scope(&plan, &doc, &[]).is_empty());
}
