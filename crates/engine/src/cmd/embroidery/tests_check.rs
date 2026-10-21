//! Design checks (`embroidery.check`): every rule fires on a design built to trigger it and stays
//! quiet on one just short of it; the command checks its params and bounds what it lists.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_geom::Point;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::plan::{Params, Style};
use vectorcraft_stitch::thread::Palette;
use vectorcraft_stitch::{Block, Stitch, Thread};

use super::check::{self, Ask, RULES};
use super::plan::{Choice, DesignPlan, Origin, Part};
use super::sequence::{Planned, Sequence};
use super::*;

/// A unit of `style` in `color` sewing needle points at `points` (millimetres), whose widest satin
/// column is `satin_width`.
fn unit(node: u64, style: Style, color: [u8; 3], points: &[(f64, f64)], satin_width: Option<f64>) -> Planned {
    let mut block = Block::new(Thread::rgb(color));
    block.stitches = points.iter().map(|&(x, y)| Stitch::sew(Point::new(x, y))).collect();
    let origin = Origin { node: NodeId(node), part: Part::Fill(0), instance: None };
    Planned { origin, params: Some(Params::for_style(style)), columns: None, satin_width, why: Choice::Forced, block, literal: None }
}

fn plan(units: Vec<Planned>) -> DesignPlan {
    let mut seq = Sequence::new(Params::for_style(Style::Fill));
    for u in units {
        seq.add(u);
    }
    seq.plan("test".into(), Vec::new(), Palette::Jef)
}

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
    let cases: [(&str, Style, f64, f64); 7] = [
        ("stitchTooLong", Style::Running, 12.8, 12.7),
        ("fillStitchShort", Style::Fill, 2.49, 2.5),
        ("satinStitchShort", Style::Satin, 0.99, 1.0),
        ("runStitchShort", Style::Running, 1.49, 1.5),
        ("stitchBelowNeedle", Style::Running, 0.74, 0.75),
        ("stitchTiny", Style::Running, 0.3, 0.31),
        ("formatStep", Style::Running, 12.2, 12.1),
    ];
    for (id, style, past, at) in cases {
        // Stitches of 3 mm either side, so that no other short stitch is near.
        assert_eq!(hits(&one(style, &[3.0, past, 3.0]), id, Format::Dst), 1, "{id}: {past}");
        assert_eq!(hits(&one(style, &[3.0, at, 3.0]), id, Format::Dst), 0, "{id}: {at}");
    }
    // A style's rule leaves the other styles alone.
    assert_eq!(hits(&one(Style::Satin, &[3.0, 2.0, 3.0]), "fillStitchShort", Format::Dst), 0);
    assert_eq!(hits(&one(Style::Fill, &[3.0, 0.9, 3.0]), "satinStitchShort", Format::Dst), 0);
    // The format's step is along x or y: PES's is far longer than DST's.
    assert_eq!(hits(&one(Style::Running, &[12.2]), "formatStep", Format::Pes), 0);
    assert_eq!(hits(&one(Style::Running, &[12.6]), "formatStep", Format::Jef), 0);
    assert_eq!(hits(&one(Style::Running, &[12.8]), "formatStep", Format::Jef), 1);
}

/// More than five short stitches in a row fire, five don't; a long stitch, a jump or a cut ends a
/// row.
#[test]
fn more_than_five_short_stitches_in_a_row_fire() {
    let row = |n: usize| [vec![3.0], vec![0.9; n], vec![3.0]].concat();
    assert_eq!(hits(&one(Style::Satin, &row(6)), "shortStitchesInARow", Format::Dst), 1);
    assert_eq!(hits(&one(Style::Satin, &row(5)), "shortStitchesInARow", Format::Dst), 0);
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

/// A satin column wider than 10 mm fires, one 10 mm wide doesn't; so does a fill's narrow part.
#[test]
fn a_satin_wider_than_ten_millimetres_fires() {
    let satin = |width: f64, style: Style| plan(vec![unit(1, style, RED, &line(&[3.0, 3.0]), Some(width))]);
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

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
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
        assert!(r["first"].as_array().unwrap().len() <= check::LIMIT);
    }
    // A fill's short row ends and underlay fire its rule, on the rectangle.
    let fill = v["rules"].as_array().unwrap().iter().find(|r| r["id"] == json!("fillStitchShort")).unwrap();
    assert!(fill["hits"].as_u64().unwrap() > 0);
    assert_eq!(fill["objects"], json!([id]));
    assert_eq!(fill["first"][0]["id"], json!(id));
    // Sources that disagree are both listed, the one checked first.
    let wide = &v["rules"][0];
    assert_eq!((&wide["id"], &wide["threshold"], wide["sources"].as_array().unwrap().len()), (&json!("satinTooWide"), &json!(10.0), 2));
    // Asked for: some rules, a format, a limit.
    let v = run(&mut s, "embroidery.check", json!({"rules": ["fillStitchShort", "layers", "fillStitchShort"], "format": "dst", "limit": 2}));
    assert_eq!((v["format"].clone(), v["rules"].as_array().unwrap().len()), (json!("dst"), 2));
    assert_eq!(v["rules"][0]["first"].as_array().unwrap().len(), 2);
    run(&mut s, "embroidery.setup", json!({"palette": "pec"}));
    assert_eq!(run(&mut s, "embroidery.check", json!({"rules": []}))["format"], json!("pes"));
    for (bad, why) in [
        (json!({"format": "svg"}), "format"),
        (json!({"rules": ["noSuchRule"]}), "noSuchRule"),
        (json!({"rules": "layers"}), "a list of rule ids"),
        (json!({"limit": 1001}), "limit"),
        (json!({"limit": -1}), "limit"),
        (json!({"density": "x"}), "embroidery options"),
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
