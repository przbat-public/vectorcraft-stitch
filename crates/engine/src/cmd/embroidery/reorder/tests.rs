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

/// `id`'s siblings in stacking order (bottom first) and its place among them.
fn place_in_stack(doc: &Document, id: NodeId) -> (Vec<NodeId>, usize) {
    let siblings: Vec<NodeId> = match doc.parent_of(id).and_then(|p| doc.node(p)) {
        Some(parent) => parent.children().map(|c| c.iter().map(|n| n.id).collect()).unwrap_or_default(),
        None => doc.layers.iter().map(|l| l.id).collect(),
    };
    let at = siblings.iter().position(|n| *n == id).unwrap_or(usize::MAX);
    (siblings, at)
}

/// What `plan` sews, whatever the order: each unit's object, part and thread, sorted.
fn what_is_sewn(plan: &DesignPlan) -> Vec<(u64, String, [u8; 3])> {
    let mut out: Vec<(u64, String, [u8; 3])> = plan
        .units
        .iter()
        .map(|u| (mover(u).0, format!("{:?}", u.origin.part), plan.blocks.get(u.block).map_or([0; 3], |b| b.thread.color())))
        .collect();
    out.sort();
    out
}

/// Every single object dropped at every gap of the fixtures: a drop allowed runs as one undo step,
/// moves the object just above or below the object it goes beside, in its parent, leaves what
/// clips it as it was and changes when, never what, is sewn; a refused one names a reason that
/// holds.
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
    let sewn_before = what_is_sewn(&plan);
    let mut moved = 0;
    for m in &movers {
        // The mover's own units, and the units on either side of a gap that aren't its.
        let own: Vec<usize> = plan.units.iter().enumerate().filter(|(_, u)| mover(u) == *m).map(|(k, _)| k).collect();
        let (first, last) = (own.first().copied().unwrap_or(0), own.last().copied().unwrap_or(0));
        for gap in 0..=plan.units.len() {
            let doc = doc_of(&s);
            let before = clip_context(&doc, doc.parent_of(*m));
            let case = format!("{m:?} at {gap}");
            let prev = plan.units.get(..gap).unwrap_or_default().iter().rev().map(mover).find(|o| o != m);
            let next = plan.units.get(gap..).unwrap_or_default().iter().map(mover).find(|o| o != m);
            match place(&doc, &plan, &[*m], gap) {
                Ok(row) => {
                    let undo = s.doc().unwrap().history.undo.len();
                    s.execute("command.batch", &row.batch()).unwrap_or_else(|e| panic!("{case}: {e}"));
                    assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
                    let now = doc_of(&s);
                    assert_ne!(now.layers, doc.layers, "{case}: something moved");
                    assert_eq!(clip_context(&now, now.parent_of(*m)), before, "{case}: what clips it");
                    assert_eq!(now.parent_of(*m), now.parent_of(row.target), "{case}: beside the target");
                    // Just above or just below it.
                    let ((_, at), (_, target)) = (place_in_stack(&now, *m), place_in_stack(&now, row.target));
                    let beside = match row.place {
                        Place::Above => target + 1,
                        Place::Below => target.wrapping_sub(1),
                    };
                    assert_eq!(at, beside, "{case}: {:?} its target", row.place);
                    assert_eq!(what_is_sewn(&plan_of(&mut s)), sewn_before, "{case}: what is sewn");
                    s.execute("edit.undo", &json!({})).unwrap();
                    moved += 1;
                }
                Err(Refusal::Locked(id)) => assert!(locked_within(&doc, id), "{case}"),
                Err(Refusal::ClippingPath(id)) => assert!(clipping_path(&doc, id), "{case}"),
                Err(Refusal::TargetLocked(id)) => assert!(locked_within(&doc, id), "{case}: {id:?} locked"),
                Err(Refusal::NotAccepted(id)) => {
                    assert!(
                        doc.node(id).is_some_and(|n| matches!(n.kind, vectorcraft_doc::NodeKind::Layer { .. })),
                        "{case}: only a layer is turned away"
                    )
                }
                Err(Refusal::ChangesClipping(_)) => {
                    let clips = |o: Option<NodeId>| o.map(|o| clip_context(&doc, doc.parent_of(o)));
                    assert!(
                        clips(prev).is_some_and(|c| c != before) || clips(next).is_some_and(|c| c != before),
                        "{case}: what clips it would change"
                    );
                }
                Err(Refusal::InsideObject) => {
                    let between = |k: usize| plan.units.get(k.wrapping_sub(1)).zip(plan.units.get(k)).is_some_and(|(a, b)| mover(a) == mover(b));
                    assert!(between(gap), "{case}: between two parts of one object");
                }
                Err(Refusal::NoChange) => {
                    // At its own place, or beside an object it already sits beside in the stack.
                    let (siblings, at) = place_in_stack(&doc, *m);
                    let next_to = |o: Option<NodeId>| o.is_some_and(|o| [at.wrapping_sub(1), at + 1].iter().any(|k| siblings.get(*k) == Some(&o)));
                    assert!((first..=last + 1).contains(&gap) || next_to(prev) || next_to(next), "{case}: it is there already");
                }
                Err(other) => panic!("{case}: {other:?}"),
            }
        }
    }
    // The fixtures' drops that go through (the number they make: a change in where a drop may go
    // shows here).
    assert_eq!(moved, DROPS_MADE, "drops made");
}

/// How many single-object drops at every gap of `fx::structures` go through.
const DROPS_MADE: usize = 27;

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
    // Sewn first, then a step later: second, just above the object that was sewn next.
    let next = sewn(&plan_of(&mut s))[1];
    let r = s.execute("embroidery.sewLater", &json!({})).unwrap();
    assert_eq!((&r["ids"], &r["target"], &r["place"]), (&json!([order[1]]), &json!(next), &json!("above")));
    assert_eq!(sewn(&plan_of(&mut s))[..2], [next, order[1]]);
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

/// Sew Earlier goes just below the object sewn just before, in its parent, as documented: an object
/// in a group stays in it, beside its neighbour, though the object sewn before that one lies
/// outside the group (where a drop at that gap would go).
#[test]
fn sew_earlier_stays_beside_its_neighbour() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let rect = |s: &mut Session, color: &str, x: f64| {
        s.execute("select.none", &json!({})).unwrap();
        fx::rect(&mut runner(s), color, x, 20.0, 50.0, 50.0)
    };
    let w = rect(&mut s, fx::RED, 20.0);
    let x = rect(&mut s, fx::BLUE, 120.0);
    let m = rect(&mut s, fx::GREEN, 220.0);
    s.execute("select.set", &json!({"ids": [x, m]})).unwrap();
    let group = s.execute("object.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(sewn(&plan_of(&mut s)), [w, x, m]);
    s.execute("select.set", &json!({"ids": [m]})).unwrap();
    let r = s.execute("embroidery.sewEarlier", &json!({})).unwrap();
    assert_eq!((&r["target"], &r["place"]), (&json!(x), &json!("below")));
    assert_eq!(s.doc().unwrap().doc.parent_of(NodeId(m)), Some(NodeId(group)), "still in its group");
    assert_eq!(sewn(&plan_of(&mut s)), [w, m, x]);
}
