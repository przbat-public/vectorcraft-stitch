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

/// Sew Earlier, Later, First and Last move the selection (or `ids`) by a step, one undo step named
/// "Change Sewing Order", and say why they can't.
#[test]
fn the_sew_commands() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let mut ids = vec![];
    // Four threads, so that no two join.
    for k in 0..4 {
        let color = [fx::RED, fx::BLUE, fx::GREEN, "#e0a000"][k];
        s.execute("select.none", &json!({})).unwrap();
        ids.push(fx::rect(&mut runner(&mut s), color, 20.0 + 80.0 * k as f64, 20.0, 50.0, 50.0));
    }
    let order = sewn(&plan_of(&mut s));
    assert_eq!(order, ids);
    s.execute("select.set", &json!({"ids": [order[1]]})).unwrap();
    let r = s.execute("embroidery.sewEarlier", &json!({})).unwrap();
    assert_eq!((&r["ids"], &r["place"]), (&json!([order[1]]), &json!("below")));
    assert_eq!(sewn(&plan_of(&mut s))[0], order[1]);
    assert_eq!(s.doc().unwrap().history.undo.last().map(|e| e.label.as_str()), Some(LABEL));
    let e = s.execute("embroidery.sewEarlier", &json!({})).unwrap_err().to_string();
    assert!(e.contains("the selection is sewn first already"), "{e}");
    s.execute("embroidery.sewLast", &json!({"ids": [order[1]]})).unwrap();
    assert_eq!(sewn(&plan_of(&mut s)).last(), Some(&order[1]));
    let e = s.execute("embroidery.sewLater", &json!({})).unwrap_err().to_string();
    assert!(e.contains("sewn last already"), "{e}");
    s.execute("embroidery.sewFirst", &json!({})).unwrap();
    assert_eq!(sewn(&plan_of(&mut s))[0], order[1]);
    s.execute("select.none", &json!({})).unwrap();
    let e = s.execute("embroidery.sewLater", &json!({})).unwrap_err().to_string();
    assert!(e.contains("nothing selected is sewn"), "{e}");
    // A locked object is refused, saying why.
    s.execute("select.set", &json!({"ids": [order[2]]})).unwrap();
    s.execute("object.lock", &json!({})).unwrap();
    let e = s.execute("embroidery.sewFirst", &json!({"ids": [order[2]]})).unwrap_err().to_string();
    assert!(e.contains("is locked"), "{e}");
    assert!(s.execute("embroidery.sewFirst", &json!({"idz": [1]})).is_err(), "a key it doesn't take");
}

/// What a reorder did: nothing, when the objects moved lie apart from what they passed and still
/// join their thread; otherwise the blocks and thread changes after.
#[test]
fn explaining_a_reorder() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let [_, blue, _] = fx::three_threads(&mut runner(&mut s), false);
    let before = plan_of(&mut s);
    // Blue just above the first red: the reds still join ahead of it.
    let row = place(&doc_of(&s), &before, &[NodeId(blue)], 2).unwrap();
    s.execute("command.batch", &row.batch()).unwrap();
    let after = plan_of(&mut s);
    let e = explain(&before, &after, &[NodeId(blue)]);
    assert!(e.unchanged, "{e:?}");
    assert_eq!(e.changes, (1, 1));
    // Blue sewn first: the reds still join, after it.
    s.execute("edit.undo", &json!({})).unwrap();
    let before = plan_of(&mut s);
    let row = place(&doc_of(&s), &before, &[NodeId(blue)], 0).unwrap();
    s.execute("command.batch", &row.batch()).unwrap();
    let after = plan_of(&mut s);
    let e = explain(&before, &after, &[NodeId(blue)]);
    assert!(!e.unchanged);
    assert_eq!(e.moved, [MovedObject { id: NodeId(blue), block: 1, joined: false }]);
    assert_eq!(e.changes, (1, 1));
    // Overlapping: the top red moved first joins the bottom red's block.
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let [_, _, top] = fx::three_threads(&mut runner(&mut s), true);
    let before = plan_of(&mut s);
    assert_eq!(before.blocks.len(), 3, "red, blue on it, red on that");
    let row = place(&doc_of(&s), &before, &[NodeId(top)], 0).unwrap();
    s.execute("command.batch", &row.batch()).unwrap();
    let after = plan_of(&mut s);
    let e = explain(&before, &after, &[NodeId(top)]);
    assert_eq!(e.changes.0, 2);
    assert_eq!(e.moved.first().map(|m| (m.block, m.joined)), Some((1, true)), "joined the first red: {e:?}");
}

/// Without options of their own, the sew commands plan with the stitch preview's (the document's
/// last embroidery export's), so that they move on the plan the Sewing order list shows: after the
/// preview's plan, the command plans nothing more.
#[test]
fn the_sew_commands_plan_with_the_previews_options() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    for k in 0..3 {
        s.execute("select.none", &json!({})).unwrap();
        fx::rect(&mut runner(&mut s), [fx::RED, fx::BLUE, fx::GREEN][k], 20.0 + 80.0 * k as f64, 20.0, 50.0, 50.0);
    }
    super::super::view::note_export(&mut s, "dst", &json!({"spacing": 0.5, "expand": 0.2}));
    let uid = s.doc().unwrap().uid;
    let (opts, _) = s.stitch_preview_options(uid);
    assert_ne!(opts, Options::default());
    let preview = s.embroidery_plan(&opts).unwrap();
    let service = s.embroidery_plans.of(uid);
    let walks = service.walks();
    let second = preview.units.iter().map(mover).nth(1).unwrap();
    s.execute("select.set", &json!({"ids": [second.0]})).unwrap();
    s.execute("embroidery.sewFirst", &json!({})).unwrap();
    assert_eq!(service.walks(), walks, "the preview's plan, not another");
    // Options given are the command's own: one plan of the moved design, with them.
    s.execute("embroidery.sewLast", &json!({"stitch": "running"})).unwrap();
    assert_eq!(service.walks(), walks + 1);
}

/// The ids a sew command is given are checked before anything is planned: an array of objects of
/// the document, at most as many as a move takes; nothing else falls back to the selection.
#[test]
fn the_sew_commands_check_their_ids() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let a = fx::rect(&mut runner(&mut s), fx::RED, 20.0, 20.0, 50.0, 50.0);
    s.execute("select.none", &json!({})).unwrap();
    fx::rect(&mut runner(&mut s), fx::BLUE, 120.0, 20.0, 50.0, 50.0);
    s.execute("select.set", &json!({"ids": [a]})).unwrap();
    let undo = s.doc().unwrap().history.undo.len();
    for bad in
        [json!({"ids": 42}), json!({"ids": "42"}), json!({"ids": [a, "x"]}), json!({"ids": [999_999]}), json!({"ids": [-1]}), json!({"ids": null})]
    {
        let e = s.execute("embroidery.sewLast", &bad).unwrap_err().to_string();
        assert!(!e.is_empty(), "{bad}");
    }
    let many: Vec<u64> = (0..=100_000).collect();
    let e = s.execute("embroidery.sewLast", &json!({ "ids": many })).unwrap_err().to_string();
    assert!(e.contains("too many objects"), "{e}");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo, "nothing moved");
    s.execute("embroidery.sewLast", &json!({"ids": [a]})).unwrap();
}
