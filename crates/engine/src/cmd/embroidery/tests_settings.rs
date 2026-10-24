//! The embroidery settings: what each one does to the stitches, and the angle's direction.

use serde_json::{Value, json};
use vectorcraft_stitch::Command;

use super::digitize::digitize;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// The direction most of `plan`'s needle-down stitches longer than `longer` (millimetres) run in,
/// degrees counter-clockwise on the page, from 0 up to a half turn (a line's two directions are
/// one): the median of their directions.
fn direction(plan: &plan::DesignPlan, longer: f64) -> f64 {
    let mut angles: Vec<f64> = Vec::new();
    for b in &plan.blocks {
        for w in b.stitches.windows(2) {
            let [a, z] = w else { continue };
            if a.command != Command::Stitch || z.command != Command::Stitch || a.distance(*z) <= longer {
                continue;
            }
            // The page's y axis points up, the document's down.
            angles.push((-(z.y - a.y)).atan2(z.x - a.x).to_degrees().rem_euclid(180.0));
        }
    }
    assert!(!angles.is_empty(), "no stitch longer than {longer} mm");
    angles.sort_by(f64::total_cmp);
    angles[angles.len() / 2]
}

/// A 60 mm square filled red, without underlay (so that its stitches are its rows), planned with
/// the embroidery options `opts`.
fn square(opts: &Value) -> plan::DesignPlan {
    let mut s = blank();
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    let side = 60.0 * PT_PER_MM;
    run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": side, "height": side}));
    let mut opts = opts.clone();
    opts["underlay"] = json!(false);
    let opts = Options::from_json(&opts).unwrap();
    digitize(&s.doc().unwrap().doc, &opts, None).unwrap()
}

/// A fill's angle turns counter-clockwise on the page, as every other angle in VectorCraft: rows
/// at 30° run up to the right, at 120° up to the left; left out, they run at 135°, from the top left
/// to the bottom right, as they always have.
#[test]
fn a_fill_angle_turns_counter_clockwise_on_the_page() {
    /// How far the rows' direction may lie from the angle asked for, degrees: a needle point's
    /// last bits and the rows' stagger move a 4 mm stitch's direction by far less.
    const WITHIN: f64 = 0.5;
    /// Longer than a row's remainder at the edges, shorter than its 4 mm stitches.
    const ROW_STITCH: f64 = 3.0;
    // The earlier name keeps its earlier meaning, clockwise: `stitchAngle` 120 is rows at 60°.
    for (opts, want) in [(json!({"angle": 30}), 30.0), (json!({"stitchAngle": 120}), 60.0), (json!({}), 135.0), (json!({"angle": -45}), 135.0)] {
        let rows = direction(&square(&opts), ROW_STITCH);
        assert!((rows - want).abs() <= WITHIN, "{opts}: rows at {rows}°, not {want}°");
    }
    assert_eq!(options::planner_angle(vectorcraft_stitch::plan::FILL_ANGLE), 135.0);
}

/// A document holding a filled square (no part of it narrow), a satin stroke 3 mm wide and a
/// running stroke 0.5 mm wide, with the embroidery settings `setup` (as `embroidery.setup` keeps
/// them).
fn sewn(setup: vectorcraft_doc::embroidery::EmbroiderySetup) -> Session {
    let mut s = blank();
    // Paint commands repaint what is selected too: nothing is, when each is given.
    let mut run_ = |c: &str, p: Value| {
        let r = run(&mut s, c, p);
        run(&mut s, "select.none", json!({}));
        r
    };
    run_("paint.setStroke", json!({"none": true}));
    run_("paint.setFill", json!({"color": "#cc2222"}));
    run_("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 60}));
    run_("paint.setFill", json!({"none": true}));
    run_("paint.setStroke", json!({"color": "#2233cc"}));
    let satin = run_("path.create", json!({"d": "M 20 150 L 200 150"}))["id"].clone();
    run_("stroke.set", json!({"weight": 3.0 * PT_PER_MM, "ids": [satin]}));
    run_("paint.setStroke", json!({"color": "#22aa44"}));
    let line = run_("path.create", json!({"d": "M 20 200 L 200 230"}))["id"].clone();
    run_("stroke.set", json!({"weight": 0.5 * PT_PER_MM, "ids": [line]}));
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = Some(Box::new(setup));
        Ok(())
    })
    .unwrap();
    s
}

/// The units of `s`'s plan with `opts`, by the style each is sewn in: (fill, satin, running).
fn units_by_style(s: &Session, opts: &Value) -> (plan::Unit, plan::Unit, plan::Unit) {
    let opts = Options::from_json(opts).unwrap();
    let plan = digitize(&s.doc().unwrap().doc, &opts, None).unwrap();
    let of = |style: vectorcraft_stitch::plan::Style| plan.units.iter().find(|u| u.sewn() == Some(style)).cloned().unwrap();
    use vectorcraft_stitch::plan::Style;
    (of(Style::Fill), of(Style::Satin), of(Style::Running))
}

/// A fabric with a value of its own for every kind of stitch.
fn fabric() -> vectorcraft_doc::embroidery::EmbroiderySetup {
    serde_json::from_value(json!({"fabric": {
        "tatami": {"pull": 0.35, "spacing": 0.33, "length": 3.5, "underlay": "edgeTatami"},
        "satin": {"pull": 0.4, "spacing_percent": 90.0, "underlay": "center"}
    }}))
    .unwrap()
}

/// The fabric's values reach each unit by the kind of stitch it is sewn in: a fill its tatami
/// values, a satin column its satin values, a running line none of them.
#[test]
fn the_fabric_reaches_each_unit_by_its_stitch() {
    use vectorcraft_stitch::plan::Underlay;
    let mut doc = sewn(fabric());
    let (fill, satin, running) = units_by_style(&doc, &json!({}));
    let f = fill.params.unwrap();
    assert_eq!((f.pull, f.density, f.stitch_length, f.underlay), ([0.35; 2], 0.33, 3.5, Underlay::EdgeTatami));
    // A fill's narrow parts are satin columns: the fabric's satin values. A filled T, its stem
    // 10 pt (3.5 mm) wide.
    run(&mut doc, "paint.setStroke", json!({"none": true}));
    run(&mut doc, "paint.setFill", json!({"color": "#cc2222"}));
    run(&mut doc, "path.create", json!({"d": "M 20 300 L 80 300 L 80 360 L 55 360 L 55 440 L 45 440 L 45 360 L 20 360 Z"}));
    let plan = digitize(&doc.doc().unwrap().doc, &Options::default(), None).unwrap();
    let c = plan.units.iter().find_map(|u| u.columns).unwrap();
    assert_eq!((c.pull, c.spacing_scale, c.underlay), ([0.4; 2], 0.9, Underlay::Center));
    let s = satin.params.unwrap();
    assert_eq!((s.pull, s.spacing_scale, s.underlay, s.auto_spacing), ([0.4; 2], 0.9, Underlay::Center, true));
    let r = running.params.unwrap();
    let default = vectorcraft_stitch::plan::Params::for_style(vectorcraft_stitch::plan::Style::Running);
    assert_eq!((r.pull, r.stitch_length, r.underlay), (default.pull, default.stitch_length, default.underlay), "a running line takes none");
}

/// An option given to one export beats the document's value for that export; the document's
/// values reach whatever it leaves out.
#[test]
fn an_export_option_beats_the_document() {
    let (fill, satin, _) = units_by_style(&sewn(fabric()), &json!({"pull": 0.1, "length": 2.0}));
    let (f, s) = (fill.params.unwrap(), satin.params.unwrap());
    assert_eq!((f.pull, f.stitch_length, f.density), ([0.1; 2], 2.0, 0.33));
    assert_eq!((s.pull, s.spacing_scale), ([0.1; 2], 0.9));
}

/// The fabric's row spacing is for 40 wt thread: the document's thread weight scales it as it
/// scales the built-in spacing, while a spacing given to an export is used as it is.
#[test]
fn the_fabrics_spacing_follows_the_thread() {
    use vectorcraft_stitch::plan::thread_scale;
    let mut setup = fabric();
    setup.set_thread_weight(Some(30.0));
    let s = sewn(setup);
    let (fill, satin, _) = units_by_style(&s, &json!({}));
    assert_eq!(fill.params.unwrap().density, 0.33 * thread_scale(30.0));
    assert_eq!(satin.params.unwrap().thread_weight, 30.0);
    let (given, _, _) = units_by_style(&s, &json!({"spacing": 0.5}));
    assert_eq!(given.params.unwrap().density, 0.5);
    // The export's thread weight beats the document's, for the fabric's spacing too.
    let (fine, _, _) = units_by_style(&s, &json!({"threadWeight": 60}));
    assert_eq!(fine.params.unwrap().density, 0.33 * thread_scale(60.0));
}

/// The machine's values reach every unit and the moves between them: its jump and trim lengths,
/// its shortest stitch, and its satin split, which no satin stitch is longer than.
#[test]
fn the_machine_reaches_every_unit() {
    let setup: vectorcraft_doc::embroidery::EmbroiderySetup =
        serde_json::from_value(json!({"machine": {"jump_length": 3.0, "trim_length": 6.0, "min_stitch": 0.3, "split_length": 7.0}})).unwrap();
    let mut s = sewn(setup);
    // A satin 9 mm wide, wider than the split.
    run(&mut s, "paint.setStroke", json!({"color": "#8822aa"}));
    let wide = run(&mut s, "path.create", json!({"d": "M 20 300 L 200 300"}))["id"].clone();
    run(&mut s, "select.none", json!({}));
    run(&mut s, "stroke.set", json!({"weight": 9.0 * PT_PER_MM, "ids": [wide]}));
    let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    for u in &plan.units {
        let p = u.params.unwrap();
        assert_eq!((p.jump, p.trim, p.min_stitch, p.max_satin), (3.0, 6.0, 0.3, 7.0));
    }
    let wide_unit = plan.units.iter().position(|u| u.params.is_some_and(|p| p.width > 8.0)).unwrap();
    let longest = plan
        .unit_stitches(wide_unit)
        .windows(2)
        .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
        .map(|w| w[0].distance(w[1]))
        .fold(0.0, f64::max);
    assert!(longest <= 7.0 + 1e-6, "a {longest} mm satin stitch");
}

/// Joining off sews objects of one colour in one block only when one follows the other: red, blue
/// apart, red make two blocks with joining on (the reds together) and three with it off.
#[test]
fn joining_off_keeps_the_stacking_order() {
    for (join, blocks) in [(None, 2), (Some(true), 2), (Some(false), 3)] {
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        for (color, x) in [("#cc2222", 20), ("#2233cc", 120), ("#cc2222", 220)] {
            run(&mut s, "paint.setFill", json!({"color": color}));
            run(&mut s, "shape.rectangle", json!({"x": x, "y": 20, "width": 40, "height": 40}));
            run(&mut s, "select.none", json!({}));
        }
        s.edit("Embroidery Setup", |d, _| {
            let mut e = vectorcraft_doc::embroidery::EmbroiderySetup::default();
            e.set_join_same_color(join);
            d.embroidery = (!e.is_empty()).then(|| Box::new(e));
            Ok(())
        })
        .unwrap();
        let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
        assert_eq!(plan.blocks.len(), blocks, "joining {join:?}");
    }
}

/// What a newer version wrote in the document's settings is sewn as this version can: a value out
/// of its range at the nearest it takes, an underlay kind it doesn't know at the default, and keys
/// it doesn't read without; the plan says which happened, once each.
#[test]
fn a_newer_versions_document_settings_are_sewn_as_this_version_can() {
    let setup: vectorcraft_doc::embroidery::EmbroiderySetup = serde_json::from_value(json!({
        "fabric": {"tatami": {"pull": 5.0, "underlay": "doubleZigzag"}, "grain": 45},
        "machine": {"trim_length": 2.0, "model": "x"}
    }))
    .unwrap();
    let s = sewn(setup);
    let plan = digitize(&s.doc().unwrap().doc, &Options::default(), None).unwrap();
    let fill = plan.units.iter().find(|u| u.sewn() == Some(vectorcraft_stitch::plan::Style::Fill)).unwrap().params.unwrap();
    let pull_most = vectorcraft_stitch::plan::PULL_RANGE.1;
    assert_eq!((fill.pull, fill.underlay), ([pull_most; 2], vectorcraft_stitch::plan::Underlay::Auto));
    assert_eq!(fill.trim, fill.jump, "a trim below the jump is raised to it");
    assert_eq!(
        plan.notes,
        [super::resolve::DOCUMENT_SETTINGS_UNREAD, super::resolve::DOCUMENT_SETTINGS_OUT_OF_RANGE, super::resolve::TRIM_BELOW_JUMP].map(String::from)
    );
}

/// [`Options::over`] keeps every value of the upper layer and fills every gap from the lower: each
/// option set alone, above and below.
#[test]
fn options_lie_over_one_another_option_by_option() {
    let all = Options::from_json(&json!({
        "stitch": "running", "spacing": 0.5, "length": 3, "angle": 10, "pull": 0.3, "pullPercent": 5, "push": 0.2, "expand": 0.1,
        "narrowAsSatin": 4, "threadWeight": 60, "underlay": "edge", "jumpLength": 3, "trimLength": 6, "minStitch": 0.3, "splitLength": 8
    }))
    .unwrap();
    let all = Options { spacing_percent: Some(90.0), ..all };
    assert_eq!(all.over(&Options::default()), all);
    assert_eq!(Options::default().over(&all), all);
    let other = Options::from_json(&json!({"spacing": 0.4, "jumpLength": 4})).unwrap();
    let merged = other.over(&all);
    assert_eq!((merged.spacing, merged.jump_length, merged.length, merged.split_length), (Some(0.4), Some(4.0), Some(3.0), Some(8.0)));
}

/// `embroidery.stats` times the design at the document's machine speed and counts the thread
/// through its fabric's thickness when not asked for others; asked values beat them, and a value a
/// newer version wrote out of range is taken at the nearest it may be, said in the warnings.
#[test]
fn the_figures_take_the_documents_machine_and_fabric() {
    let setup = serde_json::from_value(json!({"machine": {"speed": 800.0}, "fabric": {"thickness": 1.5}})).unwrap();
    let mut s = sewn(setup);
    let v = run(&mut s, "embroidery.stats", json!({}));
    assert_eq!((&v["time"]["speed"], &v["threadUsage"]["fabricThickness"]), (&json!(800), &json!(1.5)));
    let asked = run(&mut s, "embroidery.stats", json!({"speed": 500, "fabricThickness": 0.3}));
    assert_eq!((&asked["time"]["speed"], &asked["threadUsage"]["fabricThickness"]), (&json!(500), &json!(0.3)));
    let setup = serde_json::from_value(json!({"machine": {"speed": 5000.0}})).unwrap();
    let mut s = sewn(setup);
    let v = run(&mut s, "embroidery.stats", json!({}));
    assert_eq!(v["time"]["speed"].as_f64(), Some(super::stats::SPEED_RANGE.1));
    assert_eq!(v["warnings"], json!([super::resolve::DOCUMENT_SETTINGS_OUT_OF_RANGE]));
}

/// The stitches are drawn on the fabric's colour (the document's embroidery settings), white when
/// it has none.
#[test]
fn the_stitches_are_drawn_on_the_fabrics_colour() {
    let navy = [20, 45, 90];
    let mut s = sewn(serde_json::from_value(json!({"fabric": {"color": "#142d5a"}})).unwrap());
    let r = run(&mut s, "embroidery.render", json!({}));
    let png = vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(1, 1).0, [navy[0], navy[1], navy[2], 255]);
    assert_eq!(super::scene::fabric_color(&s.doc().unwrap().doc), navy);
    assert_eq!(super::scene::fabric_color(&blank().doc().unwrap().doc), super::scene::FABRIC);
}
