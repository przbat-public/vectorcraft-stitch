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
#[test]
fn each_stitch_length_rule_fires_past_its_threshold() {
    let cases: [(&str, Style, f64, f64); 4] = [
        ("stitchTooLong", Style::Running, 12.8, 12.7),
        ("stitchBelowNeedle", Style::Running, 0.74, 0.75),
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
    assert_eq!(hits(&one_unit(Some(Params::for_style(Style::Fill)), Some([0.8, 3.0])), "satinStitchShort", Format::Dst), 1);
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

/// Counting layers costs a bounded amount whatever a stitch's length: a run read from a file with
/// stitches kilometres long is checked at once.
#[test]
fn layers_are_counted_in_bounded_work_however_long_the_stitches() {
    let far = 1.0e6;
    let long = plan(vec![planned(1, None, RED, &[(0.0, 0.0), (far, 0.0), (0.0, far), (far, far)], None)]);
    let t = std::time::Instant::now();
    assert_eq!(hits(&long, "layers", Format::Dst), 0);
    // Far below what sampling every half millimetre would take (millions of cells a stitch).
    assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());
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
    assert_eq!((&wide["id"], &wide["threshold"], wide["sources"].as_array().unwrap().len()), (&json!("satinTooWide"), &json!(10.0), 2));
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
    // The keys it takes: the export's embroidery options and its own three; the rules its
    // description lists are the rules there are.
    let spec = crate::cmd::find_command("embroidery.check").unwrap();
    for key in crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).chain(["format", "rules", "limit"]) {
        assert!(crate::cmd::params::takes_key(spec.params, key), "{key}");
    }
    for rule in RULES {
        assert!(spec.params.contains(&format!(" {} ", rule.id)) || spec.params.contains(&format!(" {})", rule.id)), "{}", rule.id);
    }
    let e = crate::check_params("embroidery.check", &json!({"selectedOnly": true})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter `selectedOnly`"), "{e}");
}
