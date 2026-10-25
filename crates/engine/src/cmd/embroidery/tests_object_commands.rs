//! The settings commands on objects: `embroidery.set` storing a part's values as the file keeps
//! them (one undo step, none for no change), refusing what it doesn't take before anything
//! changes, standing a group for its objects and listing what it skips; `embroidery.reset` keeping
//! the points and the cut; `embroidery.setEntryExit` on one object; `embroidery.copyFrom` copying
//! set and unset values.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::{Embroidery, PartKind};

use super::test_support::{blank, run};
use super::*;

/// A rectangle filled red with a blue stroke at `x` → its id.
fn shape(s: &mut Session, x: f64) -> u64 {
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    run(s, "paint.setStroke", json!({"color": "#2233cc"}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    id
}

/// Object `id`'s embroidery as the document keeps it, saved as the file saves it (`null`: none).
fn saved(s: &Session, id: u64) -> Value {
    s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().embroidery.as_deref().cloned().map_or(Value::Null, Value::from)
}

fn undo_steps(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

/// `embroidery.set` stores each value as the file keeps it (one number for both sides as two,
/// `fill` as `tatami`), on the part asked for, in one undo step; the same values again change
/// nothing and take no step; `null` unsets, and a part left with nothing is dropped.
#[test]
fn set_stores_a_parts_values_in_one_step() {
    let mut s = blank();
    let id = shape(&mut s, 20.0);
    let before = undo_steps(&s);
    let r = run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 30, "pull": 0.35, "stitch": "fill", "pullPercent": [5, 10]}));
    assert_eq!(
        r,
        json!({"part": "fill", "settings": {"angle": 30.0, "pull": [0.35, 0.35], "stitch": "tatami", "pullPercent": [5.0, 10.0]},
               "changedCount": 1, "unchangedCount": 0, "skippedCount": 0, "skipped": []})
    );
    assert_eq!(saved(&s, id), json!({"fill": {"stitch": "tatami", "angle": 30.0, "pull": [0.35, 0.35], "pull_percent": [5.0, 10.0]}}));
    assert_eq!(undo_steps(&s), before + 1);
    let again = run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 30}));
    assert_eq!((&again["changedCount"], &again["unchangedCount"]), (&json!(0), &json!(1)));
    assert_eq!(undo_steps(&s), before + 1, "no step for no change");
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "push": 0.5, "splitLength": 8}));
    assert_eq!(saved(&s, id)["stroke"], json!({"push": [0.5, 0.5], "split_length": 8.0}));
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": null, "pull": null, "stitch": null, "pullPercent": null}));
    assert_eq!(saved(&s, id), json!({"stroke": {"push": [0.5, 0.5], "split_length": 8.0}}), "the fill's part dropped");
    // Undo brings the values back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(saved(&s, id)["fill"]["angle"], json!(30.0));
}

/// What `embroidery.set` doesn't take is refused before anything changes, saying what to use.
#[test]
fn set_refuses_what_it_doesnt_take() {
    let mut s = blank();
    let id = shape(&mut s, 20.0);
    // Keys the command doesn't take: the boundary refuses them, naming the keys it takes; called
    // in-process (past the boundary), the command says what to use instead.
    for (params, says) in [
        (json!({"part": "fill", "density": 0.4}), "earlier name: embroidery.set takes `spacing`"),
        (json!({"part": "fill", "trimLength": 12}), "design setting"),
        (json!({"part": "fill", "entry": [1, 2]}), "embroidery.setEntryExit"),
        (json!({"part": "fill", "zigzag": 1}), "unknown setting"),
    ] {
        let mut p = params.clone();
        p["ids"] = json!([id]);
        assert!(s.execute("embroidery.set", &p).unwrap_err().to_string().contains("unknown parameter"), "{params}");
        let e = super::objects::set(&mut s, &p).unwrap_err().to_string();
        assert!(e.contains(says), "{params}: {e}");
    }
    for (params, says) in [
        (json!({"part": "fill", "spacing": 0.01}), "0.05 to 5"),
        (json!({"part": "both", "angle": 30}), "fill or stroke"),
        (json!({"angle": 30}), "fill or stroke"),
        (json!({"part": "fill"}), "give a setting"),
        (json!({"part": "fill", "stitch": "zigzag"}), "one of auto"),
        (json!({"part": "fill", "pull": [0.1, 0.2, 0.3]}), "pull"),
        (json!({"part": "fill", "underlay": "triple"}), "underlay"),
    ] {
        let mut p = params.clone();
        p["ids"] = json!([id]);
        let e = s.execute("embroidery.set", &p).unwrap_err().to_string();
        assert!(e.contains(says), "{params}: {e}");
    }
    assert_eq!(saved(&s, id), Value::Null);
    assert!(matches!(s.execute("embroidery.set", &json!({"ids": [id, 999_999], "part": "fill", "angle": 3})), Err(EngineError::NoNode(_))));
}

/// A group stands for the objects in it, a compound path's member for its compound path; type, a
/// symbol instance and a run of stitches opened from a file are skipped and listed, and so is an
/// object without the part; nothing taking the part is refused.
#[test]
fn set_takes_a_groups_objects_and_lists_what_it_skips() {
    let mut s = blank();
    let (a, b) = (shape(&mut s, 20.0), shape(&mut s, 100.0));
    let group = run(&mut s, "object.group", json!({"ids": [a, b]}))["id"].as_u64().unwrap();
    let text = run(&mut s, "text.create", json!({"x": 10, "y": 300, "text": "Stitch"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "embroidery.set", json!({"ids": [group, text], "part": "fill", "angle": 10}));
    assert_eq!((&r["changedCount"], &r["skipped"]), (&json!(2), &json!([{"id": text, "reason": "type"}])));
    assert!(saved(&s, a)["fill"]["angle"] == json!(10.0) && saved(&s, b)["fill"]["angle"] == json!(10.0));
    assert_eq!(saved(&s, group), Value::Null, "nothing on the group itself");
    // A symbol instance takes its art's: skipped.
    run(&mut s, "select.set", json!({"ids": [a]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Badge"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "embroidery.set", json!({"ids": [instance, b], "part": "fill", "angle": 20}));
    assert_eq!(r["skipped"], json!([{"id": instance, "reason": "symbolInstance"}]));
    // Nothing given takes settings: refused, saying why for each.
    let e = s.execute("embroidery.set", &json!({"ids": [text], "part": "stroke", "angle": 3})).unwrap_err().to_string();
    assert!(e.contains("none of these objects takes settings for its `stroke`") && e.contains(&format!("{text} (type)")), "{e}");
    let e = s.execute("embroidery.copyFrom", &json!({"source": b, "ids": [b]})).unwrap_err().to_string();
    assert!(e.contains("the source alone"), "{e}");
    let e = s.execute("embroidery.settings", &json!({"palete": "pec"})).unwrap_err().to_string();
    assert!(e.contains("takes no parameters"), "{e}");
    let e = s.execute("embroidery.applyFabric", &json!({"key": ["pull"]})).unwrap_err().to_string();
    assert!(e.contains("did you mean `keys`"), "{e}");
}

/// `embroidery.reset` unsets the objects' values on both parts, or one, or the keys named; the
/// points and the cut after the object stay unless named.
#[test]
fn reset_keeps_the_points_and_the_cut() {
    let mut s = blank();
    let id = shape(&mut s, 20.0);
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 30, "pull": 0.3}));
    run(&mut s, "embroidery.set", json!({"ids": [id], "part": "stroke", "length": 2}));
    run(&mut s, "embroidery.setEntryExit", json!({"id": id, "part": "fill", "entry": [20, 20]}));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [id], "on": true}));
    let r = run(&mut s, "embroidery.reset", json!({"ids": [id], "part": "stroke"}));
    assert_eq!((&r["part"], &r["changedCount"]), (&json!("stroke"), &json!(1)));
    assert_eq!(saved(&s, id)["stroke"], Value::Null);
    run(&mut s, "embroidery.reset", json!({"ids": [id]}));
    assert_eq!(saved(&s, id), json!({"trim_after": true, "fill": {"points": {"entry": [20.0, 20.0]}}}));
    run(&mut s, "embroidery.reset", json!({"ids": [id], "keys": ["entry"]}));
    assert_eq!(saved(&s, id), json!({"trim_after": true}));
    let e = s.execute("embroidery.reset", &json!({"ids": [id], "keys": ["density"]})).unwrap_err();
    assert!(e.to_string().contains("earlier name"), "{e}");
}

/// `embroidery.setEntryExit` sets one object's points, given or selected, and reads them back;
/// `null` unsets one, a point left out stays; a group and an object with no such part are refused.
#[test]
fn set_entry_exit_sets_one_objects_points() {
    let mut s = blank();
    let id = shape(&mut s, 20.0);
    let r = run(&mut s, "embroidery.setEntryExit", json!({"id": id, "part": "fill", "entry": [20, 20], "exit": [80, 60]}));
    assert_eq!(r, json!({"id": id, "part": "fill", "entry": [20.0, 20.0], "exit": [80.0, 60.0], "changedCount": 1}));
    run(&mut s, "select.set", json!({"ids": [id]}));
    let r = run(&mut s, "embroidery.setEntryExit", json!({"part": "fill", "exit": null}));
    assert_eq!((&r["entry"], &r["exit"]), (&json!([20.0, 20.0]), &Value::Null));
    let other = shape(&mut s, 100.0);
    let group = run(&mut s, "object.group", json!({"ids": [id, other]}))["id"].as_u64().unwrap();
    for (params, says) in [
        (json!({"id": group, "part": "fill", "entry": [0, 0]}), "group"),
        (json!({"id": id, "part": "fill"}), "give entry"),
        (json!({"id": id, "part": "fill", "entry": [0, 1e9]}), "within"),
        (json!({"id": id, "part": "fill", "entry": [0]}), "[x, y]"),
        (json!({"id": id, "part": "fill", "entry": [0, 0], "ids": [id]}), "unknown parameter"),
    ] {
        let e = s.execute("embroidery.setEntryExit", &params).unwrap_err().to_string();
        assert!(e.contains(says), "{params}: {e}");
    }
}

/// `embroidery.copyFrom` gives the objects the source's values of the keys, set or unset, on the
/// parts they have; the source itself is passed by.
#[test]
fn copy_from_copies_set_and_unset_values() {
    let mut s = blank();
    let (a, b) = (shape(&mut s, 20.0), shape(&mut s, 100.0));
    run(&mut s, "embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "pull": 0.3}));
    run(&mut s, "embroidery.set", json!({"ids": [b], "part": "fill", "length": 2, "angle": 5}));
    run(&mut s, "embroidery.setEntryExit", json!({"id": b, "part": "fill", "entry": [100, 20]}));
    let r = run(&mut s, "embroidery.copyFrom", json!({"source": a, "ids": [a, b]}));
    assert_eq!((&r["changedCount"], &r["unchangedCount"], &r["source"]), (&json!(1), &json!(0), &json!(a)));
    let fill: Embroidery = serde_json::from_value(saved(&s, b)).unwrap();
    let f = fill.part(PartKind::Fill).unwrap();
    assert_eq!((f.angle, f.pull, f.length), (Some(30.0), Some([0.3; 2]), None), "the length the source doesn't set is unset");
    assert!(f.points.as_ref().is_some_and(|p| p.entry.is_some()), "the points stay unless named");
}

/// `copyFrom` copies only what `set` would take: a newer version's stitch on the source, or a
/// value out of range, is passed over (the objects keep their own) and listed.
#[test]
fn copy_from_passes_over_what_set_would_refuse() {
    let mut s = blank();
    let (a, b) = (shape(&mut s, 20.0), shape(&mut s, 120.0));
    run(&mut s, "embroidery.set", json!({"ids": [b], "part": "fill", "stitch": "satin", "pull": 0.1}));
    s.edit("Newer Settings", |d, _| {
        let e = serde_json::from_value(json!({"fill": {"stitch": "motif", "pull": [9.0, 9.0], "angle": 30.0}}))
            .map_err(|e| EngineError::Other(e.to_string()))?;
        d.node_mut(NodeId(a)).unwrap().extras_mut().embroidery = Some(Box::new(e));
        Ok(())
    })
    .unwrap();
    let r = run(&mut s, "embroidery.copyFrom", json!({"source": a, "ids": [b], "part": "fill"}));
    assert_eq!(r["passedOver"], json!(["fill.stitch", "fill.pull"]));
    let fill = s.doc().unwrap().doc.node(NodeId(b)).unwrap().extras().embroidery.as_deref().unwrap().part(PartKind::Fill).cloned().unwrap();
    assert_eq!((fill.stitch.as_deref(), fill.pull, fill.angle), (Some("satin"), Some([0.1, 0.1]), Some(30.0)), "its own kept, the angle copied");
}

/// A command given more ids than it takes is refused on the ids, saying how many it takes, before
/// it looks at them: `set`, `copyFrom`, `info`, `applyFabric` and `reset` alike.
#[test]
fn too_many_ids_are_refused() {
    let mut s = blank();
    let id = shape(&mut s, 20.0);
    let ids: Vec<u64> = (0..=super::objects::MAX_IDS as u64).collect();
    for (command, mut params) in [
        ("embroidery.set", json!({"part": "fill", "angle": 10})),
        ("embroidery.copyFrom", json!({"source": id})),
        ("embroidery.info", json!({})),
        ("embroidery.applyFabric", json!({})),
        ("embroidery.reset", json!({})),
    ] {
        params["ids"] = json!(ids);
        let e = s.execute(command, &params).unwrap_err().to_string();
        assert!(e.contains(&format!("at most {}", super::objects::MAX_IDS)), "{command}: {e}");
    }
}
