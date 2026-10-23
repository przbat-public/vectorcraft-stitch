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

/// A selected object's ends: a group covers its members' units, an instance its own, an object
/// filled and stroked both its units; entry and exit are its first unit's entry and its last
/// unit's exit; type, which isn't sewn, has none; an id not in the document has none either.
#[test]
fn a_selected_objects_ends_are_its_units_first_and_last_needle_points() {
    let mut s = Session::new();
    let st = fx::structures(&mut runner(&mut s));
    let plan = plan_of(&mut s);
    let sc = scene::scene(&plan);
    let doc = s.doc().unwrap().doc.clone();
    let selected: Vec<NodeId> = [st.group, st.instances[1], st.filled_and_stroked, st.text, 999_999].map(NodeId).to_vec();
    let ends = sc.ends_of(&doc, &selected);
    assert_eq!(ends.len(), selected.len());
    let units_of = |ids: &[u64]| -> Vec<u32> {
        sc.units().iter().enumerate().filter(|(_, u)| ids.contains(&u.object.0)).map(|(i, _)| u32::try_from(i).unwrap()).collect()
    };
    assert_eq!(ends[0].units, units_of(&st.grouped), "the group's members");
    assert_eq!(ends[1].units, units_of(&[st.instances[1]]), "the instance, not its twin");
    assert_eq!(ends[2].units.len(), 2, "filled and stroked");
    for e in &ends[..3] {
        let (first, last) = (e.units[0] as usize, e.units[e.units.len() - 1] as usize);
        assert_eq!((e.entry, e.exit), (sc.units()[first].entry, sc.units()[last].exit), "{e:?}");
        assert!(e.units.windows(2).all(|w| w[0] < w[1]), "in sewing order");
    }
    for e in &ends[3..] {
        assert!(e.units.is_empty() && e.entry.is_none() && e.exit.is_none(), "{e:?}");
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

/// `embroidery.render`'s picture as an image, and its reply.
fn rendered(s: &mut Session, params: Value) -> (image::RgbaImage, Value) {
    let r = s.execute("embroidery.render", &params).unwrap();
    let png = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    (image::load_from_memory(&png).unwrap().to_rgba8(), r)
}

/// A filled square drawn as its stitches: the middle in its thread's colour (the rows just cover),
/// on the white fabric outside; the reply says the region drawn and the plan's generation.
#[test]
fn the_stitches_render_in_their_threads_colour_on_the_fabric() {
    let mut s = Session::new();
    let mut run = runner(&mut s);
    fx::blank(&mut run);
    fx::rect(&mut run, fx::RED, 50.0, 50.0, 100.0, 100.0);
    drop(run);
    let thread = scene::scene(&plan_of(&mut s)).thread_colors()[0];
    let (img, r) = rendered(&mut s, json!({"scale": 2}));
    let region: Vec<f64> = r["region"].as_array().unwrap().iter().filter_map(Value::as_f64).collect();
    let margin = vectorcraft_stitch::mm_to_pt(super::render::DESIGN_MARGIN_MM);
    assert!((region[0] - (50.0 - margin)).abs() < 2.0 && (region[2] - (150.0 + margin)).abs() < 2.0, "{region:?}");
    assert_eq!((img.width(), img.height()), (r["width"].as_u64().unwrap() as u32, r["height"].as_u64().unwrap() as u32));
    let middle = img.get_pixel(img.width() / 2, img.height() / 2).0;
    assert_eq!([middle[0], middle[1], middle[2]], thread, "the thread's colour");
    assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255, 255], "the fabric");
    assert!(r["generation"].as_u64().is_some());
    // Transparent when asked; an artboard; a box.
    let (clear, _) = rendered(&mut s, json!({"background": null}));
    assert_eq!(clear.get_pixel(1, 1).0[3], 0);
    let (board, r) = rendered(&mut s, json!({"artboard": 0}));
    assert_eq!((board.width(), board.height(), &r["region"]), (400, 400, &json!([0.0, 0.0, 400.0, 400.0])));
    let (bx, _) = rendered(&mut s, json!({"rect": [90, 90, 110, 110], "scale": 4}));
    assert_eq!((bx.width(), bx.height()), (80, 80));
}

/// What can't be drawn is an error, never a panic: no document, nothing sewn, a region or scale out
/// of range, two regions, an unknown key.
#[test]
fn the_picture_refuses_what_it_cant_draw() {
    let mut s = Session::new();
    assert!(s.execute("embroidery.render", &json!({})).is_err(), "no document");
    fx::blank(&mut runner(&mut s));
    let e = s.execute("embroidery.render", &json!({})).unwrap_err().to_string();
    assert!(e.contains("nothing to draw"), "{e}");
    fx::rect(&mut runner(&mut s), fx::RED, 50.0, 50.0, 100.0, 100.0);
    for bad in [
        json!({"scale": 0}),
        json!({"scale": 1000}),
        json!({"rect": [10, 10, 5, 5]}),
        json!({"rect": [0, 0, 10]}),
        json!({"artboard": 3}),
        json!({"artboard": 0, "rect": [0, 0, 10, 10]}),
        json!({"region": "page"}),
        json!({"background": "red"}),
        json!({"jumps": 1}),
        json!({"zoom": 2}),
        json!({"rect": [0, 0, 4e6, 4e6], "scale": 64}),
    ] {
        assert!(s.execute("embroidery.render", &bad).is_err(), "{bad}");
    }
}

/// An opened design drawn as stitches covers what its art (the same lines, 0.4 mm wide, round)
/// covers: the two views of an opened file agree, colours aside (the preview draws palette
/// threads).
#[test]
fn an_opened_design_draws_as_its_art_does() {
    let mut s = Session::new();
    fx::opened_design(&mut runner(&mut s), "dst");
    let (img, r) = rendered(&mut s, json!({"scale": 3, "jumps": false}));
    let region: Vec<f64> = r["region"].as_array().unwrap().iter().filter_map(Value::as_f64).collect();
    let rect = vectorcraft_geom::Rect::new(region[0], region[1], region[2], region[3]);
    let doc = s.doc().unwrap().doc.clone();
    let art = vectorcraft_render::Renderer::new().render_region(&doc, rect, 3.0, true);
    assert_eq!((art.width, art.height), (img.width(), img.height()));
    let inked = |p: [u8; 4]| p[3] > 0 && p[..3].iter().any(|c| *c < 200);
    let (mut both, mut differ) = (0usize, 0usize);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let (a, b) = (inked(img.get_pixel(x, y).0), inked(art.pixel(x, y)));
            both += usize::from(a || b);
            differ += usize::from(a != b);
        }
    }
    assert!(both > 1000, "something drawn");
    assert!((differ as f64) < 0.02 * both as f64, "{differ} of {both} pixels inked in one view only");
}
