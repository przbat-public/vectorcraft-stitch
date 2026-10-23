//! Where a drop in the sewing order goes: just above the object sewn before the gap, in its parent;
//! each refusal; every drop on the fixtures either runs as one undo step that changes when, never
//! what, is sewn, or is refused; and the gaps the Sew Earlier, Later, First and Last moves try.

use serde_json::{Value, json};
use vectorcraft_testkit::embroidery as fx;

use super::super::Options;
use super::super::plan::DesignPlan;
use super::*;
use crate::Session;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn plan_of(s: &mut Session) -> DesignPlan {
    (*s.embroidery_plan(&Options::default()).unwrap()).clone()
}

fn doc_of(s: &Session) -> std::sync::Arc<Document> {
    s.doc().unwrap().doc.clone()
}

/// The movers in sewing order, once each.
fn sewn(plan: &DesignPlan) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for u in &plan.units {
        let m = mover(u).0;
        if out.last() != Some(&m) {
            out.push(m);
        }
    }
    out
}

#[test]
fn a_drop_goes_just_above_the_object_sewn_before_the_gap() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    // Red, blue apart, red: the reds join, blue last.
    let [red, blue, again] = fx::three_threads(&mut runner(&mut s), false);
    let plan = plan_of(&mut s);
    assert_eq!(sewn(&plan), [red, again, blue]);
    // Blue dropped before everything: just below the first red.
    let m = place(&doc_of(&s), &plan, &[NodeId(blue)], 0).unwrap();
    assert_eq!(m, RowMove { ids: vec![NodeId(blue)], target: NodeId(red), place: Place::Below });
    let undo = s.doc().unwrap().history.undo.len();
    s.execute("command.batch", &m.batch()).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1, "one undo step");
    assert_eq!(sewn(&plan_of(&mut s)), [blue, red, again]);
    // Back after the first red: just above it.
    let plan = plan_of(&mut s);
    let m = place(&doc_of(&s), &plan, &[NodeId(blue)], 2).unwrap();
    assert_eq!((m.target, m.place), (NodeId(red), Place::Above));
    s.execute("command.batch", &m.batch()).unwrap();
    let doc = doc_of(&s);
    let layer: Vec<u64> = doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    assert_eq!(layer, [red, blue, again], "just above the first red in the stack");
    // Blue lies apart from both reds, so they still join ahead of it: the drop changes no stitch.
    assert_eq!(sewn(&plan_of(&mut s)), [red, again, blue]);
    // Where it is already: no change.
    let plan = plan_of(&mut s);
    assert_eq!(place(&doc_of(&s), &plan, &[NodeId(blue)], 1), Err(Refusal::NoChange));
}

#[test]
fn each_refusal() {
    let mut s = Session::new();
    let st = fx::structures(&mut runner(&mut s));
    let plan = plan_of(&mut s);
    let doc = doc_of(&s);
    let at = |id: u64| plan.units.iter().position(|u| mover(u).0 == id).unwrap();
    assert_eq!(place(&doc, &plan, &[], 0), Err(Refusal::Nothing));
    assert_eq!(place(&doc, &plan, &[NodeId(999_999)], 0), Err(Refusal::Gone(NodeId(999_999))));
    // The locked layer's square.
    assert_eq!(place(&doc, &plan, &[NodeId(st.locked)], 0), Err(Refusal::Locked(NodeId(st.locked))));

    // Out of the clip group: its clipped square moved beside the group's square.
    let clip = doc.node(NodeId(st.clip_group)).unwrap().children().unwrap().iter().map(|n| n.id).collect::<Vec<_>>();
    let (path, clipped) = (clip[0], clip[1]);
    let group_square = at(st.grouped[0]);
    assert_eq!(place(&doc, &plan, &[clipped], group_square + 1), Err(Refusal::ChangesClipping(clipped)));
    assert_eq!(place(&doc, &plan, &[path], group_square + 1), Err(Refusal::ClippingPath(path)));
    // Into the locked layer: right after its square.
    let locked = at(st.locked);
    assert_eq!(place(&doc, &plan, &[NodeId(st.grouped[0])], locked + 1), Err(Refusal::TargetLocked(NodeId(st.locked_layer))));
    // Between an object's fill and its stroke, both red, sewn one after the other.
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let both = fx::rect(&mut runner(&mut s), fx::RED, 20.0, 20.0, 60.0, 60.0);
    s.execute("paint.setStroke", &json!({"color": fx::RED, "ids": [both]})).unwrap();
    s.execute("stroke.set", &json!({"weight": fx::SATIN_WEIGHT, "ids": [both]})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    let other = fx::rect(&mut runner(&mut s), fx::BLUE, 200.0, 20.0, 60.0, 60.0);
    let plan = plan_of(&mut s);
    assert_eq!(sewn(&plan)[0], both);
    assert_eq!(mover(&plan.units[1]).0, both, "its stroke right after its fill");
    assert_eq!(place(&doc_of(&s), &plan, &[NodeId(other)], 1), Err(Refusal::InsideObject));
    // A run of an opened machine file.
    let mut s = Session::new();
    fx::opened_design(&mut runner(&mut s), "dst");
    let plan = plan_of(&mut s);
    let run = mover(&plan.units[0]);
    assert_eq!(place(&doc_of(&s), &plan, &[run], plan.units.len()), Err(Refusal::FromFile(run)));
}

/// Every single object dropped at every gap of the fixtures: a drop allowed runs as one undo step,
/// moves the object, and leaves what clips it as it was; a refused one names a reason that holds.
#[test]
fn every_drop_runs_or_is_refused_for_a_reason() {
    let mut s = Session::new();
    fx::structures(&mut runner(&mut s));
    let plan = plan_of(&mut s);
    let movers: Vec<NodeId> = plan.units.iter().map(mover).fold(Vec::new(), |mut v, m| {
        if !v.contains(&m) {
            v.push(m);
        }
        v
    });
    let mut moved = 0;
    for m in &movers {
        for gap in 0..=plan.units.len() {
            let doc = doc_of(&s);
            let before = clip_context(&doc, doc.parent_of(*m));
            match place(&doc, &plan, &[*m], gap) {
                Ok(row) => {
                    let undo = s.doc().unwrap().history.undo.len();
                    s.execute("command.batch", &row.batch()).unwrap_or_else(|e| panic!("{m:?} at {gap}: {e}"));
                    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
                    let now = doc_of(&s);
                    assert_ne!(now.layers, doc.layers, "{m:?} at {gap}: something moved");
                    assert_eq!(clip_context(&now, now.parent_of(*m)), before, "{m:?} at {gap}: what clips it");
                    assert_eq!(now.parent_of(*m), now.parent_of(row.target), "beside the target");
                    s.execute("edit.undo", &json!({})).unwrap();
                    moved += 1;
                }
                Err(Refusal::Locked(id)) => assert!(locked_within(&doc, id)),
                Err(Refusal::ClippingPath(id)) => assert!(clipping_path(&doc, id)),
                Err(Refusal::NoChange | Refusal::InsideObject | Refusal::ChangesClipping(_) | Refusal::TargetLocked(_) | Refusal::NotAccepted(_)) => {
                }
                Err(other) => panic!("{m:?} at {gap}: {other:?}"),
            }
        }
    }
    assert!(moved > 20, "{moved} drops made");
}

#[test]
fn the_gaps_sew_earlier_later_first_and_last_try() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let mut ids = vec![];
    for k in 0..4 {
        let color = [fx::RED, fx::BLUE, fx::GREEN, fx::RED][k];
        s.execute("select.none", &json!({})).unwrap();
        ids.push(fx::rect(&mut runner(&mut s), color, 20.0 + 80.0 * k as f64, 20.0, 50.0, 50.0));
    }
    let plan = plan_of(&mut s);
    let doc = doc_of(&s);
    let order = sewn(&plan);
    // The second object sewn moved by each step.
    let m = [NodeId(order[1])];
    assert_eq!(gap_for(&doc, &plan, &m, Step::First), Some(0));
    assert_eq!(gap_for(&doc, &plan, &m, Step::Last), Some(plan.units.len()));
    assert_eq!(gap_for(&doc, &plan, &m, Step::Earlier), Some(0), "before the first");
    assert_eq!(gap_for(&doc, &plan, &m, Step::Later), Some(3), "after the third");
    let earlier = place(&doc, &plan, &m, 0).unwrap();
    s.execute("command.batch", &earlier.batch()).unwrap();
    assert_eq!(sewn(&plan_of(&mut s))[0], order[1], "sewn first now");
    // The first can't go earlier.
    let plan = plan_of(&mut s);
    assert_eq!(gap_for(&doc_of(&s), &plan, &[NodeId(order[1])], Step::Earlier), None);
    // A selected group moves as one.
    assert_eq!(movers_of(&doc_of(&s), &plan, &[NodeId(order[2]), NodeId(order[2])]), [NodeId(order[2])]);
}
