//! Manual stitches: what the plan records of each unit to freeze it (its own stitches, the needle
//! point it shares with the unit before, its place in walk order).

use serde_json::json;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::test_support::{PT_PER_MM, blank, plan, run, unit};
use super::*;

/// A unit's own stitches are what it sews itself: not the move that reaches it (the jump, and the
/// cut before a long one), not the cut its object asks for after it. A unit that starts where the
/// needle already is gets its first needle point back, which the plan left to the unit before.
#[test]
fn a_units_own_stitches_leave_out_the_moves_around_it() {
    let red = [200, 30, 30];
    // The second starts where the first ends; the third far away (a cut, then a jump).
    let p = plan(vec![
        unit(1, Style::Running, red, &[(0.0, 0.0), (2.0, 0.0)], None),
        unit(2, Style::Running, red, &[(2.0, 0.0), (4.0, 0.0)], None),
        unit(3, Style::Running, red, &[(40.0, 0.0), (42.0, 0.0)], None),
    ]);
    assert_eq!(p.blocks.len(), 1);
    let commands = |u: usize| p.own_stitches(u).iter().map(|s| (s.command, s.x)).collect::<Vec<_>>();
    assert_eq!(commands(0), [(Command::Stitch, 0.0), (Command::Stitch, 2.0)]);
    assert!(p.units[1].met, "the second starts where the needle is");
    assert_eq!(commands(1), [(Command::Stitch, 2.0), (Command::Stitch, 4.0)], "its first needle point put back");
    assert!(!p.units[2].met);
    assert_eq!(commands(2), [(Command::Stitch, 40.0), (Command::Stitch, 42.0)], "without the cut and the jump that reach it");
    let range = |u: usize| p.units[u].stitches.clone();
    assert_eq!((range(2).len(), p.units[2].own.len()), (4, 2), "its range holds the cut and the jump too");
}

/// The plan keeps the order the walk added the units in: a unit that joins an earlier block of its
/// colour is sewn before units walked ahead of it. And the cut its object asks for after it is not
/// its own.
#[test]
fn units_keep_their_place_in_walk_order() {
    let mut s = blank();
    let side = 20.0 * PT_PER_MM;
    run(&mut s, "paint.setStroke", json!({"none": true}));
    let mut ids = Vec::new();
    for (color, x, y) in [("#d01818", 20.0, 20.0), ("#1830c8", 20.0, 200.0), ("#d01818", 150.0, 20.0)] {
        run(&mut s, "paint.setFill", json!({ "color": color }));
        ids.push(run(&mut s, "shape.rectangle", json!({"x": x, "y": y, "width": side, "height": side}))["id"].as_u64().unwrap());
        run(&mut s, "select.none", json!({}));
    }
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [ids[0]]}));
    let p = s.embroidery_plan(&Options::default()).unwrap();
    let sewn: Vec<(u64, usize)> = p.units.iter().map(|u| (u.origin.selected().0, u.walked)).collect();
    assert_eq!(sewn, [(ids[0], 0), (ids[2], 2), (ids[1], 1)], "the reds joined, sewn before the blue walked between them");
    let first = &p.units[0];
    let block = &p.blocks[first.block];
    assert_eq!(block.stitches.get(first.own.end).map(|s| s.command), Some(Command::Trim), "the cut asked for after it follows its own stitches");
    assert!(p.own_stitches(0).iter().all(|s: &Stitch| s.command != Command::Trim || s.x != block.stitches[first.own.end].x));
}
