//! What the stitch preview reads from a plan: each sewing unit's key, which tells it apart within
//! a plan and from one plan to the next, and the scene it draws, which holds exactly the thread the
//! plan sews.

use std::collections::HashSet;

use serde_json::{Value, json};
use vectorcraft_geom::Point;
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

/// What a scene draws, counted from its picture: the thread's length and stitches (each segment of
/// a line, millimetres), the needle points (each record number a line holds, once), the links and
/// the cuts.
struct Drawn {
    length: f64,
    segments: usize,
    needle: usize,
    links: usize,
    cuts: usize,
}

fn drawn(s: &scene::StitchScene) -> Drawn {
    use vectorcraft_render::lines::MarkKind;
    let p = s.picture();
    let mut numbers = std::collections::BTreeSet::new();
    let (mut length, mut segments) = (0.0, 0);
    for line in p.lines() {
        let pts: Vec<Point> = p.line_points(line).collect();
        for w in pts.windows(2) {
            length += w[0].distance(w[1]);
            segments += 1;
        }
        for i in line.points.clone() {
            numbers.insert(p.point(i as usize).map(|(_, n)| n));
        }
    }
    let cuts = p.marks().iter().filter(|m| m.kind == MarkKind::Cross).count();
    Drawn { length, segments, needle: numbers.len(), links: p.links().len(), cuts }
}

/// What the plan sews, counted from its records as the figures count them.
fn sewn(plan: &DesignPlan) -> Drawn {
    let (mut length, mut segments, mut needle, mut links, mut cuts) = (0.0, 0, 0, 0, 0);
    super::measure::each(plan, |r| {
        if let Some(l) = r.sewn() {
            length += l;
            segments += 1;
        }
        match r.stitch.command {
            vectorcraft_stitch::Command::Stitch => needle += 1,
            vectorcraft_stitch::Command::Jump => links += usize::from(r.same_block),
            vectorcraft_stitch::Command::Trim => cuts += 1,
            _ => {}
        }
    });
    Drawn { length, segments, needle, links, cuts }
}

/// The plans of the shared fixtures, and a design opened from each machine format that has jumps
/// and cuts.
fn fixture_plans() -> Vec<(String, DesignPlan)> {
    let mut out = Vec::new();
    for overlap in [false, true] {
        let mut s = Session::new();
        let mut run = runner(&mut s);
        fx::blank(&mut run);
        fx::three_threads(&mut run, overlap);
        drop(run);
        out.push((format!("three threads, overlap {overlap}"), plan_of(&mut s)));
    }
    let mut s = Session::new();
    fx::structures(&mut runner(&mut s));
    out.push(("structures".into(), plan_of(&mut s)));
    let mut s = Session::new();
    let mut run = runner(&mut s);
    fx::blank(&mut run);
    fx::stroke(&mut run, fx::RED, fx::SATIN_WEIGHT, &[(20.0, 20.0), (200.0, 40.0), (220.0, 160.0)]);
    fx::stroke(&mut run, fx::BLUE, fx::RUNNING_WEIGHT, &[(20.0, 100.0), (200.0, 120.0)]);
    fx::letter_t(&mut run, fx::GREEN, 20.0, 200.0);
    drop(run);
    out.push(("strokes and a T".into(), plan_of(&mut s)));
    for format in ["pes", "dst", "jef", "vp3"] {
        let mut s = Session::new();
        fx::opened_design(&mut runner(&mut s), format);
        out.push((format!("opened {format}"), plan_of(&mut s)));
    }
    out
}

/// The scene draws the plan's thread exactly: as long, in as many stitches, every needle point,
/// jump and cut once.
#[test]
fn the_scene_draws_the_thread_the_plan_sews() {
    for (name, plan) in fixture_plans() {
        let (d, w) = (drawn(&scene::scene(&plan)), sewn(&plan));
        assert!((d.length - w.length).abs() <= 1e-3 * w.length.max(1.0), "{name}: drawn {} mm, sewn {} mm", d.length, w.length);
        assert_eq!(d.segments, w.segments, "{name}: stitches");
        assert_eq!(d.needle, w.needle, "{name}: needle points");
        assert_eq!(d.links, w.links, "{name}: jumps");
        assert_eq!(d.cuts, w.cuts, "{name}: cuts");
        assert!(w.segments > 0, "{name} sews something");
    }
}

/// Each line is one unit's: its tag is the unit that sews each of its points (the first may be
/// the unit before's, a lead-in), and its colour slot that unit's block; each unit's object is the
/// one a person selects for it.
#[test]
fn each_line_is_one_units_and_each_unit_its_objects() {
    for (name, plan) in fixture_plans() {
        let sc = scene::scene(&plan);
        let p = sc.picture();
        for line in p.lines() {
            let unit = line.tag as usize;
            assert_eq!(line.group as usize, plan.units[unit].block, "{name}");
            let numbers: Vec<u32> = line.points.clone().filter_map(|i| p.point(i as usize).map(|(_, n)| n)).collect();
            for (k, n) in numbers.iter().enumerate() {
                let sewn_by = plan.unit_at(*n as usize);
                if k == 0 && sewn_by != Some(unit) {
                    // A lead-in: the next point is the unit's own stitch from it.
                    assert_eq!(numbers.get(1).and_then(|m| plan.unit_at(*m as usize)), Some(unit), "{name}: line {line:?}");
                } else {
                    assert_eq!(sewn_by, Some(unit), "{name}: point {n} of line {line:?}");
                }
            }
        }
        assert_eq!(sc.units().len(), plan.units.len());
        for (u, su) in plan.units.iter().zip(sc.units()) {
            assert_eq!((su.object, su.key, su.block), (u.origin.selected(), u.key(), u.block), "{name}");
        }
        assert_eq!(sc.blocks().len(), plan.blocks.len());
        assert_eq!(sc.thread_colors(), plan.blocks.iter().map(|b| b.thread.color()).collect::<Vec<_>>());
    }
}

/// A unit that starts where the needle already is (its first needle point left to the unit before)
/// still draws its first stitch: its line starts from the unit before's last needle point.
#[test]
fn a_unit_continuing_from_the_last_needle_point_draws_its_first_stitch() {
    use super::test_support::{plan, unit};
    use vectorcraft_stitch::plan::Style;
    let red = [200, 20, 20];
    // Two running units of one colour, the second starting at the first's end.
    let plan = plan(vec![
        unit(1, Style::Running, red, &[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)], None),
        unit(2, Style::Running, red, &[(10.0, 0.0), (15.0, 3.0), (20.0, 0.0)], None),
    ]);
    assert_eq!(plan.blocks.len(), 1, "one block");
    let sc = scene::scene(&plan);
    let (d, w) = (drawn(&sc), sewn(&plan));
    assert_eq!(d.segments, w.segments);
    assert!((d.length - w.length).abs() < 1e-9, "{} {}", d.length, w.length);
    let second: Vec<&vectorcraft_render::lines::Line> = sc.picture().lines().iter().filter(|l| l.tag == 1).collect();
    let first_point = second.first().and_then(|l| sc.picture().line_points(l).next()).map(|p| (p.x, p.y));
    assert_eq!(first_point, Some((10.0, 0.0)), "the second unit's line starts where the first ended");
}

/// The entry and exit of each unit are its first and last needle points, as
/// `embroidery.sequence` gives them (millimetres there, document points here).
#[test]
fn a_units_entry_and_exit_are_the_sewing_orders() {
    let mut s = Session::new();
    fx::structures(&mut runner(&mut s));
    let plan = plan_of(&mut s);
    let sc = scene::scene(&plan);
    let listed = s.execute("embroidery.sequence", &json!({})).unwrap();
    let units: Vec<&Value> = listed["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).collect();
    assert_eq!(units.len(), sc.units().len());
    let mm = |p: Point| [vectorcraft_stitch::pt_to_mm(p.x), vectorcraft_stitch::pt_to_mm(p.y)];
    for (u, su) in units.iter().zip(sc.units()) {
        for (key, at) in [("entry", su.entry), ("exit", su.exit)] {
            let want: Option<Vec<f64>> = u[key].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect());
            let got = at.map(mm);
            match (want, got) {
                (Some(w), Some(g)) => assert!((w[0] - g[0]).abs() < 0.006 && (w[1] - g[1]).abs() < 0.006, "{key}: {w:?} {g:?}"),
                (w, g) => assert!(w.is_none() && g.is_none(), "{key}: {w:?} {g:?}"),
            }
        }
    }
}

/// An opened design's scene lies on its lines: every point drawn is an anchor of the paths it was
/// opened as, in document points.
#[test]
fn an_opened_designs_scene_lies_on_its_lines() {
    for format in ["pes", "dst"] {
        let mut s = Session::new();
        fx::opened_design(&mut runner(&mut s), format);
        let plan = plan_of(&mut s);
        let sc = scene::scene(&plan);
        let mut anchors = Vec::new();
        s.doc().unwrap().doc.walk(|n| {
            if let vectorcraft_doc::NodeKind::Path { path, .. } = &n.kind {
                anchors.extend(path.subpaths.iter().flat_map(|sp| sp.anchors.iter().map(|a| a.p)));
            }
        });
        let to_doc = sc.picture().to_doc();
        let p = sc.picture();
        for line in p.lines() {
            for q in p.line_points(line) {
                let at = to_doc * q;
                assert!(anchors.iter().any(|a| a.distance(at) < 1e-3), "{format}: {at:?} on no line");
            }
        }
    }
}

/// An empty plan draws nothing.
#[test]
fn an_empty_plan_draws_nothing() {
    let sc = scene::scene(&DesignPlan::default());
    assert!(sc.picture().lines().is_empty() && sc.units().is_empty() && sc.bounds().is_none());
}
