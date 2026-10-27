//! Manual stitches: what the plan records of each unit to freeze it (its own stitches, the needle
//! point it shares with the unit before, its place in walk order).

use serde_json::json;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::plan::Part;
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

/// A path of literal stitches in the document's first layer (or in group `into`), stroked in
/// `color`, through `points` (millimetres), marked as `run` says → its id.
fn literal_path(s: &mut Session, into: Option<u64>, color: [u8; 3], points: &[(f64, f64)], run: vectorcraft_doc::embroidery::StitchRun) -> u64 {
    use vectorcraft_doc::embroidery::Embroidery;
    let mut bp = vectorcraft_geom::BezPath::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        let p = vectorcraft_geom::Point::new(x * PT_PER_MM, y * PT_PER_MM);
        if i == 0 { bp.move_to(p) } else { bp.line_to(p) }
    }
    let mut id = 0;
    s.edit("Add Run", |d, _| {
        let parent = into.map(vectorcraft_doc::NodeId).or_else(|| d.layers.first().map(|l| l.id));
        let node_id = d.alloc_id();
        let paint = vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(color[0], color[1], color[2]));
        let look = vectorcraft_doc::Appearance::basic(vectorcraft_color::Paint::None, paint, 1.0);
        let mut node = vectorcraft_doc::Node::path(node_id, vectorcraft_geom::PathData::from_bezpath(&bp), look);
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(run.clone()), ..Embroidery::default() }));
        d.insert(parent, usize::MAX, node).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        id = node_id.0;
        Ok(())
    })
    .unwrap();
    id
}

/// A red square of `side` millimetres at `x, y` (millimetres), filled, left unselected → its id.
fn red_square(s: &mut Session, x: f64, y: f64, side: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#c81e1e"}));
    let id = run(s, "shape.rectangle", json!({"x": x * PT_PER_MM, "y": y * PT_PER_MM, "width": side * PT_PER_MM, "height": side * PT_PER_MM}))["id"]
        .as_u64()
        .unwrap();
    run(s, "select.none", json!({}));
    id
}

/// A run of manual stitches is placed in the sewing order as art is: it joins the block of a square
/// of its colour sewn before it, the move into it cut when long. A machine file's run never joins
/// art: it keeps a block of its own.
#[test]
fn a_manual_run_joins_a_block_of_its_colour_as_art_does() {
    use vectorcraft_doc::embroidery::StitchRun;
    for (manual, blocks) in [(true, 1), (false, 2)] {
        let mut s = blank();
        let square = red_square(&mut s, 5.0, 5.0, 10.0);
        let line = literal_path(&mut s, None, [200, 30, 30], &[(40.0, 5.0), (42.0, 5.0), (44.0, 5.0)], StitchRun { manual, ..StitchRun::default() });
        let p = s.embroidery_plan(&Options::default()).unwrap();
        assert_eq!(p.blocks.len(), blocks, "manual {manual}");
        let parts: Vec<(u64, Part)> = p.units.iter().map(|u| (u.origin.selected().0, u.origin.part)).collect();
        let part = if manual { Part::Manual } else { Part::Literal };
        assert_eq!(parts, [(square, Part::Fill(0)), (line, part)]);
        if manual {
            let reached = &p.blocks[0].stitches[p.units[1].stitches.clone()];
            assert_eq!(reached.first().map(|s| s.command), Some(Command::Trim), "a move longer than the trim length is cut");
        }
    }
}

/// A run of manual stitches inside a clip group is sewn whole: its stitches were cut when they were
/// planned, and a fill's edge lies on the cut, which a second cut would chop. A file's run is cut to
/// the clipping path, as its line is drawn.
#[test]
fn a_manual_run_in_a_clip_group_is_not_cut_again() {
    use vectorcraft_doc::embroidery::StitchRun;
    for (manual, all) in [(true, true), (false, false)] {
        let mut s = blank();
        let points: Vec<(f64, f64)> = (0..20).map(|i| (5.0 + f64::from(i) * 2.0, 20.0)).collect();
        let line = literal_path(&mut s, None, [200, 30, 30], &points, StitchRun { manual, ..StitchRun::default() });
        run(&mut s, "paint.setFill", json!({"none": true}));
        run(&mut s, "paint.setStroke", json!({"none": true}));
        let clip = run(&mut s, "shape.rectangle", json!({"x": 0, "y": 0, "width": 20.0 * PT_PER_MM, "height": 40.0 * PT_PER_MM}))["id"].clone();
        run(&mut s, "select.set", json!({"ids": [line, clip]}));
        run(&mut s, "object.clippingMask.make", json!({}));
        let p = s.embroidery_plan(&Options::default()).unwrap();
        let needle: usize = p.blocks.iter().map(|b| b.needle_points()).sum();
        assert_eq!(needle == points.len(), all, "manual {manual}: {needle} needle points sewn");
    }
}

/// The exact millimetres a manual run keeps are sewn where its document points can't give them
/// back, while the needle point lies where it was stored; moved with its object, the run drops them.
#[test]
fn a_manual_runs_exact_millimetres_hold_while_its_points_stay() {
    use vectorcraft_doc::embroidery::{Exact, StitchRun};
    use vectorcraft_stitch::{MM_PER_PT, mm_to_pt};
    // An odd quarter of a millimetre (exactly halfway between two tenths) its document point can't
    // give back.
    let lost = (1..4000).map(|i| f64::from(2 * i + 1) * 0.25).find(|&m| mm_to_pt(m) * MM_PER_PT != m).unwrap();
    let mut s = blank();
    let exact = vec![Exact { subpath: 0, anchor: 0, x: Some(lost), y: None }];
    let id = literal_path(&mut s, None, [200, 30, 30], &[(lost, 5.0), (lost + 2.0, 5.0)], StitchRun { manual: true, exact, ..StitchRun::default() });
    // The path stores the document point the convert would: the millimetres divided.
    s.edit("Store", |d, _| {
        if let Some(n) = d.node_mut(vectorcraft_doc::NodeId(id))
            && let Some(path) = n.path_data_mut()
        {
            path.subpaths[0].anchors[0] = vectorcraft_geom::Anchor::corner(vectorcraft_geom::Point::new(mm_to_pt(lost), mm_to_pt(5.0)));
        }
        Ok(())
    })
    .unwrap();
    let first = |s: &mut Session| s.embroidery_plan(&Options::default()).unwrap().blocks[0].stitches[0].x;
    assert_eq!(first(&mut s), lost, "the exact millimetres");
    run(&mut s, "object.move", json!({"ids": [id], "dx": 10, "dy": 0}));
    let moved = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).and_then(|n| n.extras().embroidery.as_deref()?.stitches.clone()).unwrap();
    assert!(moved.exact.is_empty(), "a transform drops them");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(first(&mut s), lost, "undone: back");
}
