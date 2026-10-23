//! What the stitch preview reads from a plan: each sewing unit's key, which tells it apart within
//! a plan and from one plan to the next.

use std::collections::HashSet;

use serde_json::{Value, json};
use vectorcraft_testkit::embroidery as fx;

use super::plan::{DesignPlan, UnitKey};
use super::*;

/// A runner for the shared fixtures over `s`.
fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The active document's plan, as the read commands plan it.
fn plan_of(s: &mut Session) -> DesignPlan {
    (*s.embroidery_plan(&Options::default()).unwrap()).clone()
}

/// Every unit of the structures fixture (a group, a clip group, a symbol placed twice, an object
/// filled and stroked) has a key of its own, which finds it; after an object is added below them
/// all, every key still finds a unit sewing the same object and part.
#[test]
fn every_unit_has_a_key_of_its_own_that_holds_across_plans() {
    let mut s = Session::new();
    fx::structures(&mut runner(&mut s));
    let plan = plan_of(&mut s);
    let keys: Vec<UnitKey> = plan.units.iter().map(|u| u.key()).collect();
    assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len(), "{keys:?}");
    for (i, k) in keys.iter().enumerate() {
        assert_eq!(plan.find(k), Some(i));
    }
    // Something new at the bottom of the stack moves every unit's index, not its key.
    let mut run = runner(&mut s);
    let layer = first_layer(&mut run);
    run("layer.setCurrent", json!({"id": layer}));
    let id = fx::rect(&mut run, fx::BLUE, 300.0, 300.0, 30.0, 30.0);
    run("select.set", json!({"ids": [id]}));
    run("object.arrange.sendToBack", json!({}));
    drop(run);
    let after = plan_of(&mut s);
    for k in &keys {
        let i = after.find(k).unwrap_or_else(|| panic!("{k:?} lost"));
        assert_eq!(after.units[i].origin.selected(), k.object);
        assert_eq!(after.units[i].origin.part, k.part);
    }
    assert_eq!(after.units.first().map(|u| u.origin.selected().0), Some(id), "the new square is sewn first");
}

/// The first layer's id, through `document.inspect`'s layer list.
fn first_layer(run: fx::Run) -> u64 {
    let d = run("document.inspect", json!({}));
    d["layers"][0]["id"].as_u64().unwrap_or_else(|| panic!("no layer in {d}"))
}
