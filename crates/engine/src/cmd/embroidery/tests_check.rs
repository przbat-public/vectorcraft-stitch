//! Design checks (`embroidery.check`): every rule fires on a design built to trigger it and stays
//! quiet on one just short of it; the command checks its params and bounds what it lists.

use serde_json::{Value, json};
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::plan::{Params, Style};

use super::check::{self, Ask, RULES};
use super::plan::DesignPlan;
use super::test_support::{plan, planned, run, unit};
use super::*;

const RED: [u8; 3] = [200, 30, 30];

/// A line of needle points from the origin along x, `steps` apart.
fn line(steps: &[f64]) -> Vec<(f64, f64)> {
    let mut x = 0.0;
    let mut out = vec![(0.0, 0.0)];
    for s in steps {
        x += s;
        out.push((x, 0.0));
    }
    out
}

/// How many hits rule `id` finds in `plan`, checked for `format`.
fn hits(plan: &DesignPlan, id: &str, format: Format) -> u64 {
    let rules = vec![check::rule(id).unwrap()];
    let v = check::check(plan, &Ask { rules, format, limit: check::LIMIT });
    v["rules"][0]["hits"].as_u64().unwrap()
}

/// One unit of `style` sewing stitches of `steps` millimetres along x.
fn one(style: Style, steps: &[f64]) -> DesignPlan {
    plan(vec![unit(1, style, RED, &line(steps), None)])
}

/// The stitch-length rules: each fires on a stitch past its threshold and not on one at it (or
/// just inside it), and the style rules only on their style.
/// Every rule with a threshold says which extreme it reports as `measured`: a limit not to pass the
/// largest it judged, one not to fall under the smallest; a rule that measures nothing, none.
#[test]
fn every_rule_with_a_threshold_reports_an_extreme() {
    use super::check::Extreme;
    for rule in RULES {
        match rule.extreme {
            None => assert!(rule.unit.is_none(), "{} measures {:?} but reports no extreme", rule.id, rule.unit),
            Some(e) => {
                assert!(rule.unit.is_some(), "{}", rule.id);
                let short = rule.id.contains("Short") || rule.id.contains("Below") || rule.id.contains("Tiny");
                assert_eq!(e == Extreme::Smallest, short, "{}", rule.id);
            }
        }
    }
}

#[test]
fn each_stitch_length_rule_fires_past_its_threshold() {
    let cases: [(&str, Style, f64, f64); 4] = [
        ("stitchTooLong", Style::Running, 12.8, 12.7),
        ("stitchBelowNeedle", Style::Running, 0.75, 0.76),
        ("stitchTiny", Style::Running, 0.3, 0.31),
        ("formatStep", Style::Running, 12.2, 12.1),
    ];
    for (id, style, past, at) in cases {
        // Stitches of 3 mm either side, so that no other short stitch is near.
        assert_eq!(hits(&one(style, &[3.0, past, 3.0]), id, Format::Dst), 1, "{id}: {past}");
        assert_eq!(hits(&one(style, &[3.0, at, 3.0]), id, Format::Dst), 0, "{id}: {at}");
    }
    // How many stitches a split one is written as, and when the rounding to 0.1 mm decides.
    let hit = |steps: &[f64]| {
        let v = check::check(
            &one(Style::Running, steps),
            &Ask { rules: vec![check::rule("formatStep").unwrap()], format: Format::Dst, limit: check::LIMIT },
        );
        v["rules"][0]["examples"][0].clone()
    };
    let note = |steps: &[f64]| hit(steps)["note"].as_str().unwrap().to_string();
    assert_eq!((&hit(&[12.15])["pieces"], &hit(&[12.3])["pieces"]), (&json!([1, 2]), &json!([2, 2])));
    // The value is the measure compared with the step (the longer of x and y), the length apart:
    // the same for a stitch along x.
    assert_eq!((&hit(&[12.3])["value"], &hit(&[12.3])["length"]), (&json!(12.3), &json!(12.3)));
    assert!(note(&[12.15]).starts_with("written as 1 or 2 stitches"), "{}", note(&[12.15]));
    assert_eq!(note(&[12.3]), "written as 2 stitches");
    assert!(note(&[24.15]).starts_with("written as 2 or 3 stitches"), "{}", note(&[24.15]));
    // The format's step is along x or y: PES's is far longer than DST's.
    assert_eq!(hits(&one(Style::Running, &[12.2]), "formatStep", Format::Pes), 0);
    assert_eq!(hits(&one(Style::Running, &[12.6]), "formatStep", Format::Jef), 0);
    assert_eq!(hits(&one(Style::Running, &[12.8]), "formatStep", Format::Jef), 1);
}

/// Melco's fill, satin and walk-stitch limits look at each object's settings: a fill or running
/// stitch length set short of them, a satin column narrower than 1 mm (a satin stroke, or a fill's
/// narrowest narrow part), whatever its stitches measure; one hit per object, measuring the
/// setting. A run of stitches read from a file has no settings and never fires them.
#[test]
fn melcos_length_rules_look_at_each_objects_settings() {
    let set = |style: Style, length: f64| Params { stitch_length: length, ..Params::for_style(style) };
    let points = line(&[3.0, 3.0]);
    let one_unit = |params: Option<Params>, widths: Option<[f64; 2]>| plan(vec![planned(1, params, RED, &points, widths)]);
    for (id, style, past, at) in [("fillStitchShort", Style::Fill, 2.49, 2.5), ("runStitchShort", Style::Running, 1.49, 1.5)] {
        assert_eq!(hits(&one_unit(Some(set(style, past)), None), id, Format::Dst), 1, "{id}: {past}");
        assert_eq!(hits(&one_unit(Some(set(style, at)), None), id, Format::Dst), 0, "{id}: {at}");
    }
    let satin = Some(Params::for_style(Style::Satin));
    assert_eq!(hits(&one_unit(satin, Some([0.99, 0.99])), "satinStitchShort", Format::Dst), 1);
    assert_eq!(hits(&one_unit(satin, Some([1.0, 1.0])), "satinStitchShort", Format::Dst), 0);
    assert_eq!(hits(&one_unit(Some(Params::for_style(Style::Fill)), Some([1.2, 3.0])), "satinStitchShort", Format::Dst), 0);
    // Short stitches with sound settings: none; the setting is what is measured.
    let short_stitches = plan(vec![unit(1, Style::Fill, RED, &line(&[0.5, 0.5, 0.5]), None)]);
    assert_eq!(hits(&short_stitches, "fillStitchShort", Format::Dst), 0);
    let two = plan(vec![
        planned(1, Some(set(Style::Fill, 2.0)), RED, &points, None),
        planned(2, Some(set(Style::Fill, 1.0)), [30, 30, 200], &points, None),
    ]);
    let v = check::check(&two, &Ask { rules: vec![check::rule("fillStitchShort").unwrap()], format: Format::Dst, limit: check::LIMIT });
    assert_eq!((&v["rules"][0]["hits"], &v["rules"][0]["objects"]), (&json!(2), &json!([1, 2])));
    assert_eq!((&v["rules"][0]["examples"][0]["value"], &v["rules"][0]["examples"][1]["value"]), (&json!(2.0), &json!(1.0)));
    // A literal run: none of the three, however short its stitches.
    let literal = plan(vec![planned(1, None, RED, &line(&[0.5, 0.5, 0.5]), None)]);
    for id in ["fillStitchShort", "satinStitchShort", "runStitchShort"] {
        assert_eq!(hits(&literal, id, Format::Dst), 0, "{id}");
    }
}

/// More than five short stitches in a row fire, five don't; a long stitch, a jump or a cut ends a
/// row.
#[test]
fn more_than_five_short_stitches_in_a_row_fire() {
    let row = |n: usize| [vec![3.0], vec![0.9; n], vec![3.0]].concat();
    assert_eq!(hits(&one(Style::Satin, &row(6)), "shortStitchesInARow", Format::Dst), 1);
    assert_eq!(hits(&one(Style::Satin, &row(5)), "shortStitchesInARow", Format::Dst), 0);
    // Three at the end of one block and three at the start of the next: the thread is changed
    // between them, so two rows of three.
    let across = plan(vec![
        unit(1, Style::Satin, RED, &line(&[3.0, 0.9, 0.9, 0.9]), None),
        unit(2, Style::Satin, [30, 30, 200], &line(&[0.9, 0.9, 0.9, 3.0]), None),
    ]);
    assert_eq!(across.blocks.len(), 2);
    assert_eq!(hits(&across, "shortStitchesInARow", Format::Dst), 0);
    // Two rows of four, split by a long stitch: none.
    let split = [vec![0.9; 4], vec![3.0], vec![0.9; 4]].concat();
    assert_eq!(hits(&one(Style::Satin, &split), "shortStitchesInARow", Format::Dst), 0);
}

/// Three units sewn over one spot fire; two don't; one unit going over its own stitches is one
/// layer.
#[test]
fn three_layers_on_one_spot_fire() {
    let square = |node: u64, color: [u8; 3]| unit(node, Style::Fill, color, &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0), (0.0, 0.0)], None);
    let two = plan(vec![square(1, RED), square(2, [30, 30, 200])]);
    assert_eq!(hits(&two, "layers", Format::Dst), 0);
    let three = plan(vec![square(1, RED), square(2, [30, 30, 200]), square(3, [30, 200, 30])]);
    assert!(hits(&three, "layers", Format::Dst) > 0);
    let back_and_forth = one(Style::Running, &[3.0, -3.0, 3.0, -3.0, 3.0]);
    assert_eq!(hits(&back_and_forth, "layers", Format::Dst), 0);
}

/// Counting layers costs a bounded amount whatever a stitch's length and however far apart the
/// stitches: a run read from a file with stitches kilometres long is counted at a bounded number
/// of points a stitch, in a table of the cells stitched rather than a grid over its box; a design
/// of a hoop's size is counted on a grid.
#[test]
fn layers_are_counted_in_bounded_work_however_long_the_stitches() {
    let far = 1.0e6;
    let long = plan(vec![planned(1, None, RED, &[(0.0, 0.0), (far, 0.0), (0.0, far), (far, far)], None)]);
    assert_eq!(hits(&long, "layers", Format::Dst), 0);
    assert_eq!(check::layer_samples(far * std::f64::consts::SQRT_2), check::MAX_LAYER_SAMPLES);
    assert_eq!(check::layer_grid_cells(&long), None, "no grid over a kilometre square");
    let hoop = one(Style::Running, &[3.0, 3.0, 3.0]);
    assert!(check::layer_grid_cells(&hoop).is_some_and(|n| n < 100), "{:?}", check::layer_grid_cells(&hoop));
}

/// A satin column wider than 10 mm fires, one 10 mm wide doesn't; so does a fill's narrow part.
#[test]
fn a_satin_wider_than_ten_millimetres_fires() {
    let satin = |width: f64, style: Style| plan(vec![unit(1, style, RED, &line(&[3.0, 3.0]), Some([3.0_f64.min(width), width]))]);
    assert_eq!(hits(&satin(10.1, Style::Satin), "satinTooWide", Format::Dst), 1);
    assert_eq!(hits(&satin(10.0, Style::Satin), "satinTooWide", Format::Dst), 0);
    assert_eq!(hits(&satin(10.1, Style::Fill), "satinTooWide", Format::Dst), 1);
}

/// A thread past ΔE 10 as shown fires; one at 10 doesn't. What the plan leaves out fires once per
/// warning.
#[test]
fn a_mismatched_thread_and_art_left_out_fire() {
    let mut p = one(Style::Running, &[3.0]);
    p.blocks[0].thread.delta_e = 10.004;
    assert_eq!(hits(&p, "threadMismatch", Format::Dst), 0);
    p.blocks[0].thread.delta_e = 10.006;
    assert_eq!(hits(&p, "threadMismatch", Format::Dst), 1);
    // Every object sewn in the mismatched thread is named: two of one block.
    let mut two = plan(vec![unit(1, Style::Running, RED, &line(&[3.0]), None), unit(2, Style::Running, RED, &[(5.0, 0.0), (8.0, 0.0)], None)]);
    assert_eq!(two.blocks.len(), 1);
    two.blocks[0].thread.delta_e = 12.0;
    let v = check::check(&two, &Ask { rules: vec![check::rule("threadMismatch").unwrap()], format: Format::Dst, limit: check::LIMIT });
    assert_eq!((&v["rules"][0]["hits"], &v["rules"][0]["objects"]), (&json!(1), &json!([1, 2])));
    assert_eq!(hits(&p, "notSewn", Format::Dst), 0);
    p.warnings = vec!["type is not stitched".into(), "images are not stitched".into()];
    assert_eq!(hits(&p, "notSewn", Format::Dst), 2);
    // A note that isn't about the art (a palette this version doesn't know) is said, not counted.
    p.notes = vec!["the document's thread palette is not one this version knows".into()];
    assert_eq!(hits(&p, "notSewn", Format::Dst), 2);
    assert_eq!(p.messages().len(), 3);
}

/// A hit on a unit that adds no record (one needle point where the needle already is) has no
/// record number, rather than the next unit's.
#[test]
fn a_unit_that_adds_no_record_is_hit_without_a_number() {
    let satin = Some(Params::for_style(Style::Satin));
    let p = plan(vec![planned(1, None, RED, &line(&[3.0]), None), planned(2, satin, RED, &[(3.0, 0.0)], Some([0.5, 0.5]))]);
    assert!(p.numbers(1).unwrap().is_empty(), "the second unit adds no record");
    let v = check::check(&p, &Ask { rules: vec![check::rule("satinStitchShort").unwrap()], format: Format::Dst, limit: check::LIMIT });
    assert_eq!((&v["rules"][0]["hits"], &v["rules"][0]["examples"][0]["record"]), (&json!(1), &json!(null)), "{v}");
}

/// More colour blocks than PES holds is an error there and nothing in the other formats; more
/// stitches or colour changes than one machine shows are noted.
#[test]
fn the_format_and_machine_limits_fire_past_them() {
    let blocks = |n: usize| {
        plan((0..n).map(|k| unit(k as u64, Style::Running, [(k % 256) as u8, (k / 256) as u8, 7], &[(0.0, 0.0), (1.0, 0.0)], None)).collect())
    };
    let (most, past) = (blocks(255), blocks(256));
    assert_eq!((most.blocks.len(), past.blocks.len()), (255, 256));
    assert_eq!(hits(&most, "formatColors", Format::Pes), 0);
    assert_eq!(hits(&past, "formatColors", Format::Pes), 1);
    assert_eq!(hits(&past, "formatColors", Format::Jef), 0);
    assert_eq!(hits(&blocks(128), "machineColorChanges", Format::Pes), 0);
    assert_eq!(hits(&blocks(129), "machineColorChanges", Format::Pes), 1);
    let long =
        |n: usize| plan(vec![unit(1, Style::Running, RED, &(0..n).map(|k| ((k % 1000) as f64 * 0.5, (k / 1000) as f64)).collect::<Vec<_>>(), None)]);
    assert_eq!(hits(&long(500_000), "machineStitches", Format::Pes), 0);
    assert_eq!(hits(&long(500_001), "machineStitches", Format::Pes), 1);
}

/// The command: every rule listed with its threshold and sources, the objects hit, at most `limit`
/// hits each; the format of the document's palette by default; params checked.
#[test]
fn the_check_command_lists_every_rule_and_checks_its_params() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
    let v = run(&mut s, "embroidery.check", json!({}));
    let ids: Vec<&str> = v["rules"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, RULES.iter().map(|r| r.id).collect::<Vec<_>>());
    assert_eq!(v["format"], json!("jef"), "the default palette's format");
    for r in v["rules"].as_array().unwrap() {
        assert!(!r["sources"].as_array().unwrap().is_empty(), "{}", r["id"]);
        assert!(r["examples"].as_array().unwrap().len() <= check::LIMIT);
        assert_eq!(r["objectCount"], json!(r["objects"].as_array().unwrap().len()), "{}", r["id"]);
        assert_eq!(r["truncated"], json!(false), "{}", r["id"]);
        assert!(r["measures"].as_str().is_some_and(|m| !m.is_empty()), "{}", r["id"]);
    }
    // For each severity, the rules that found something and their hits.
    for severity in ["errors", "warnings", "info"] {
        let (rules, hits) = (v["summary"][severity]["rules"].as_u64().unwrap(), v["summary"][severity]["hits"].as_u64().unwrap());
        assert!(rules <= hits, "{severity}: {}", v["summary"]);
    }
    // The rectangle's fill stitch length, 4 mm by default, passes; set to 2 mm it fires, on the
    // rectangle.
    let fill = |v: &Value| v["rules"].as_array().unwrap().iter().find(|r| r["id"] == json!("fillStitchShort")).unwrap().clone();
    assert_eq!(fill(&v)["hits"], json!(0));
    let short = fill(&run(&mut s, "embroidery.check", json!({"stitchLength": 2})));
    assert_eq!(
        (&short["hits"], &short["objects"], &short["examples"][0]["id"], &short["examples"][0]["value"]),
        (&json!(1), &json!([id]), &json!(id), &json!(2.0))
    );
    // Satin forced on the rectangle's fill: it is filled (a fill has no width to sew a satin
    // across), so no satin column, and its fill stitch length is the one judged.
    let forced = run(&mut s, "embroidery.check", json!({"stitchStyle": "satin", "rules": ["satinStitchShort", "satinTooWide"]}));
    assert_eq!((&forced["rules"][0]["hits"], &forced["rules"][1]["hits"]), (&json!(0), &json!(0)), "{forced}");
    // The needle-blade rule is information until the planner spreads rows' remainders.
    let blade = v["rules"].as_array().unwrap().iter().find(|r| r["id"] == json!("stitchBelowNeedle")).unwrap();
    assert_eq!(blade["severity"], json!("info"));
    // Sources that disagree are both listed, the one checked first.
    let wide = &v["rules"][0];
    assert_eq!((&wide["id"], &wide["threshold"], wide["sources"].as_array().unwrap().len()), (&json!("satinTooWide"), &json!(10.0), 3));
    // Asked for: some rules, a format, a limit.
    let v = run(
        &mut s,
        "embroidery.check",
        json!({"rules": ["fillStitchShort", "layers", "fillStitchShort"], "format": "dst", "limit": 0, "stitchLength": 2}),
    );
    assert_eq!((v["format"].clone(), v["rules"].as_array().unwrap().len()), (json!("dst"), 2));
    assert_eq!((&v["rules"][0]["hits"], v["rules"][0]["examples"].as_array().unwrap().len()), (&json!(1), 0), "counted, none listed");
    run(&mut s, "embroidery.setup", json!({"palette": "pec"}));
    assert_eq!(run(&mut s, "embroidery.check", json!({"rules": []}))["format"], json!("pes"));
    for (bad, why) in [
        (json!({"format": "svg"}), "format"),
        (json!({"rules": ["noSuchRule"]}), "noSuchRule"),
        (json!({"rules": "layers"}), "a list of rule ids"),
        (json!({"limit": 1001}), "limit"),
        (json!({"limit": -1}), "limit"),
        (json!({"density": "x"}), "`density` \"x\": a number of millimetres"),
    ] {
        let e = s.execute("embroidery.check", &bad).unwrap_err().to_string();
        assert!(e.contains(why), "{bad}: {e}");
    }
    // The keys it takes: the export's embroidery options and its own four; the rules its
    // description lists are the rules there are.
    let spec = crate::cmd::find_command("embroidery.check").unwrap();
    for key in crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).chain(["format", "rules", "limit", "selectedOnly"]) {
        assert!(crate::cmd::params::takes_key(spec.params, key), "{key}");
    }
    for rule in RULES {
        assert!(spec.params.contains(&format!(" {} ", rule.id)) || spec.params.contains(&format!(" {})", rule.id)), "{}", rule.id);
    }
    let e = crate::check_params("embroidery.check", &json!({"speed": 800})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter `speed`"), "{e}");
}

/// `embroidery.check {format}` judges the threads that format writes: a PES of a document in
/// Janome's palette writes Brother's threads, matched to the art's colours, and those are judged;
/// the reply says which palette.
#[test]
fn the_check_judges_the_threads_its_format_writes() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    // Brother's Peacock Blue (PEC 38), in a document of Janome's palette.
    run(&mut s, "paint.setFill", json!({"color": "#134a46"}));
    run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40}));
    let check = |s: &mut Session, format: &str| run(s, "embroidery.check", json!({"format": format, "rules": ["threadMismatch"]}));
    let (jef, pes) = (check(&mut s, "jef"), check(&mut s, "pes"));
    assert_eq!((&jef["palette"], &pes["palette"]), (&json!("jef"), &json!("pec")));
    // Brother's own colour: no mismatch in a PES. In a JEF, what the document's palette gives.
    assert_eq!(pes["rules"][0]["hits"], json!(0));
    let threads = run(&mut s, "embroidery.threads", json!({}));
    let janome = threads["blocks"][0]["thread"]["mismatch"].as_bool().unwrap();
    assert_eq!(jef["rules"][0]["hits"], json!(u64::from(janome)));
    // A format that names no threads judges the document's.
    assert_eq!(check(&mut s, "dst")["palette"], json!("jef"));
}

/// A fill's narrow part narrower than 1 mm is sewn as a running line, not a satin column: the
/// satin rule has no satin stitch to flag in it.
#[test]
fn a_narrow_part_sewn_as_a_running_line_is_no_short_satin() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    // A 20 mm square with a 0.8 mm wide, 12 mm long strip off its right side, in points.
    let pt = |mm: f64| mm * 72.0 / 25.4;
    let d = format!(
        "M 20 20 L {r} 20 L {r} {t} L {e} {t} L {e} {b} L {r} {b} L {r} {bottom} L 20 {bottom} Z",
        r = 20.0 + pt(20.0),
        e = 20.0 + pt(32.0),
        t = 20.0 + pt(9.6),
        b = 20.0 + pt(10.4),
        bottom = 20.0 + pt(20.0)
    );
    run(&mut s, "path.create", json!({"d": d}));
    let order = run(&mut s, "embroidery.sequence", json!({}));
    assert_eq!(order["blocks"][0]["units"][0]["columns"], json!(true), "the strip is found as a narrow part: {order}");
    let check = run(&mut s, "embroidery.check", json!({"rules": ["satinStitchShort"]}));
    assert_eq!(check["rules"][0]["hits"], json!(0), "{check}");
}

/// The `formatStep` note says how many records the writer writes a long stitch as: a 30 mm stitch
/// along x in a DST (12.1 mm a record) is written as three.
#[test]
fn the_format_step_note_is_what_the_writer_writes() {
    let p = one(Style::Running, &[3.0, 30.0, 3.0]);
    let v = check::check(&p, &Ask { rules: vec![check::rule("formatStep").unwrap()], format: Format::Dst, limit: check::LIMIT });
    assert_eq!(v["rules"][0]["examples"][0]["note"], json!("written as 3 stitches"));
    let written = Format::Dst.read(&Format::Dst.write(&p.to_pattern()).unwrap()).unwrap();
    // Four needle points become six: the 30 mm stitch's two more on the way.
    let sewn = written.points().filter(|s| s.command == vectorcraft_stitch::Command::Stitch).count();
    assert_eq!(sewn, p.blocks[0].needle_points() + 2);
}

/// What the Check tab and its chart read besides the hits: each rule's extreme measured over
/// everything it judged (the longest stitch equals the figures' longest), a stitch hit's start, a
/// thread hit's block, the not-checked items' ids, and the stitch lengths in half-millimetre bins
/// with their median.
#[test]
fn the_checks_extremes_and_the_lengths_chart() {
    use vectorcraft_testkit::embroidery as fx;
    let mut s = Session::new();
    let mut runner = |c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"));
    fx::blank(&mut runner);
    fx::three_threads(&mut runner, true);
    fx::stroke(&mut runner, fx::GREEN, fx::RUNNING_WEIGHT, &[(20.0, 300.0), (300.0, 300.0)]);
    let check = s.execute("embroidery.check", &json!({"limit": 5})).unwrap();
    let stats = s.execute("embroidery.stats", &json!({})).unwrap();
    let rule = |id: &str| check["rules"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap().clone();
    assert_eq!(rule("stitchTooLong")["measured"], stats["longestStitch"], "the longest stitch judged");
    assert_eq!(rule("stitchTiny")["measured"].as_f64(), stats["shortestStitch"].as_f64());
    assert!(rule("threadMismatch")["measured"].as_f64().is_some());
    assert!(rule("notSewn")["measured"].is_null(), "a rule that measures nothing");
    // The design's own figures, against the format's and the machine's limits; the most layers.
    assert_eq!((&rule("formatColors")["measured"], &rule("machineStitches")["measured"]), (&stats["blockCount"], &stats["stitches"]));
    assert_eq!(rule("machineColorChanges")["measured"], stats["colorChanges"]);
    assert!(rule("layers")["measured"].as_u64().is_some_and(|n| n >= 1), "{}", rule("layers"));
    let ids: Vec<&str> = check["notChecked"].as_array().unwrap().iter().filter_map(|n| n["id"].as_str()).collect();
    assert_eq!(ids, ["density", "penetrations", "ties"]);
    let lengths = &stats["lengths"];
    let bins = lengths["bins"].as_array().unwrap();
    assert_eq!(bins.len(), super::stats::LENGTH_BINS);
    assert_eq!((&bins[0]["from"], &bins[1]["from"], &bins[25]["from"], &bins[25]["to"]), (&json!(0.0), &json!(0.5), &json!(12.5), &json!(12.7)));
    assert_eq!((&bins[26]["from"], &bins[26]["to"]), (&json!(12.7), &Value::Null));
    let counted: u64 = bins.iter().map(|b| b["count"].as_u64().unwrap()).sum();
    assert!(counted > 0 && counted <= stats["stitches"].as_u64().unwrap(), "{counted}");
    let median = lengths["median"].as_f64().unwrap();
    assert!(median > 0.0 && median <= stats["longestStitch"].as_f64().unwrap(), "{median}");
    // A stitch hit says where its stitch starts.
    let tiny = s.execute("embroidery.check", &json!({"rules": ["stitchBelowNeedle"], "limit": 1})).unwrap();
    if let Some(hit) = tiny["rules"][0]["examples"].get(0) {
        assert!(hit["from"].as_array().is_some_and(|a| a.len() == 2), "{hit}");
    }
}

/// A not-sewn hit names the objects whose art was left out (a symbol's instance for the art inside
/// it), and the rule's objects are theirs; what the appearances leave out names none. A thread hit
/// carries its block and thread.
#[test]
fn what_is_not_sewn_names_its_objects_and_a_thread_hit_its_thread() {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 400, "height": 400, "artboards": 1}));
    run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40}));
    let rect = run(&mut s, "effect.apply", json!({"effect": "stylize.dropShadow"}));
    assert!(rect.is_object(), "{rect}");
    let first = run(&mut s, "text.create", json!({"x": 10, "y": 150, "text": "Haft"}))["id"].as_u64().unwrap();
    let second = run(&mut s, "text.create", json!({"x": 10, "y": 250, "text": "Nić"}))["id"].as_u64().unwrap();
    run(&mut s, "select.set", json!({"ids": [second]}));
    let instance = run(&mut s, "symbol.new", json!({"name": "Word"}))["id"].as_u64().unwrap();
    let v = run(&mut s, "embroidery.check", json!({"rules": ["notSewn"]}));
    let rule = &v["rules"][0];
    let hits = rule["examples"].as_array().unwrap();
    let of = |start: &str| hits.iter().find(|h| h["note"].as_str().is_some_and(|n| n.starts_with(start))).unwrap_or_else(|| panic!("{start}: {v}"));
    assert_eq!(of("type is not stitched")["objects"], json!([first, instance]), "{v}");
    assert_eq!(of("shadows, glows, blurs")["objects"], json!([]), "{v}");
    assert_eq!(rule["objects"], json!([first, instance]));
    // A mismatched thread's hit: its block and its thread.
    let mut p = one(Style::Running, &[3.0]);
    p.blocks[0].thread.delta_e = 12.0;
    let v = check::check(&p, &Ask { rules: vec![check::rule("threadMismatch").unwrap()], format: Format::Dst, limit: check::LIMIT });
    let hit = &v["rules"][0]["examples"][0];
    assert_eq!(hit["block"], json!(1));
    assert_eq!(hit["thread"]["deltaE"], json!(12.0), "{hit}");
}

/// The chart's bins compare as the checks do: a stitch up to 12.7 mm is in the bin before it, one
/// past it in the last, which holds exactly the `stitchTooLong` hits; the median is the middle
/// length's tenth.
#[test]
fn the_lengths_bins_agree_with_the_longest_stitch_check() {
    use super::stats::{self, Machine, Page, Thread};
    let p = one(Style::Running, &[0.3, 3.04, 3.06, 3.33, 12.6, 12.71, 20.0]);
    let v = stats::stats(&p, Machine { speed: 650.0 }, Thread { fabric: 0.5 }, Page { offset: 0, limit: 0 });
    let bins = v["lengths"]["bins"].as_array().unwrap();
    let count = |k: usize| bins[k]["count"].as_u64().unwrap();
    assert_eq!((count(0), count(6), count(25), count(26)), (1, 3, 1, 2), "{bins:?}");
    assert_eq!(count(26), hits(&p, "stitchTooLong", Format::Dst));
    assert_eq!(bins.iter().map(|b| b["count"].as_u64().unwrap()).sum::<u64>(), 7);
    // Seven lengths: the fourth, 3.33 mm, is the middle, given as the middle of its tenth.
    assert_eq!(v["lengths"]["median"], json!(3.35));
    let empty = stats::stats(&plan(vec![]), Machine { speed: 650.0 }, Thread { fabric: 0.5 }, Page { offset: 0, limit: 0 });
    assert_eq!(empty["lengths"]["median"], Value::Null);
}
