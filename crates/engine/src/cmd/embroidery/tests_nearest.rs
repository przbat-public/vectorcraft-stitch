//! `embroidery.nearestConnection`: a line turned round for the part after it, a fill's exit chosen
//! for the next, one undo step that Undo takes back, a second run that changes nothing, the scope
//! (objects outside keep their points), what it can't join and why, a colour change breaking the
//! chain, strict params, and a dry run that answers as the run would.

use serde_json::{Value, json};

use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// The page's margin the drawings here start at, millimetres.
const AT: f64 = 30.0;

/// An unfilled running line in `color` (1 pt wide) from `a` to `b` (millimetres from [`AT`]) →
/// its id.
fn line(s: &mut Session, color: &str, a: (f64, f64), b: (f64, f64)) -> u64 {
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"color": color}));
    let pt = |v: f64| (AT + v) * PT_PER_MM;
    let d = format!("M {} {} L {} {}", pt(a.0), pt(a.1), pt(b.0), pt(b.1));
    let id = run(s, "path.create", json!({ "d": d }))["id"].as_u64().unwrap();
    run(s, "stroke.set", json!({"weight": 1.0, "ids": [id]}));
    run(s, "select.none", json!({}));
    id
}

/// A filled square `side` millimetres wide, its corner at `at` (millimetres from [`AT`]) → its id.
fn square(s: &mut Session, at: (f64, f64), side: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": "#cc2222"}));
    let pt = |v: f64| (AT + v) * PT_PER_MM;
    let id = run(s, "shape.rectangle", json!({"x": pt(at.0), "y": pt(at.1), "width": side * PT_PER_MM, "height": side * PT_PER_MM}))["id"]
        .as_u64()
        .unwrap();
    run(s, "select.none", json!({}));
    id
}

/// The three lines of the example: the first ends at the origin; the second runs from x = 1 to
/// x = −6 (1 mm from the needle at its right end); the third starts 6 mm right of the second's right
/// end. Started at its nearer end, the second ends 13 mm from the third, past the 10 mm trim length.
fn three_lines(s: &mut Session, third: &str) -> [u64; 3] {
    [line(s, "#2233cc", (-20.0, 10.0), (0.0, 0.0)), line(s, "#2233cc", (1.0, 0.0), (-6.0, 0.0)), line(s, third, (7.0, 0.0), (27.0, 0.0))]
}

fn units(s: &mut Session) -> Vec<Value> {
    let order = run(s, "embroidery.sequence", json!({}));
    order["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap().clone()).collect()
}

/// Unit `id`'s first needle point's x, millimetres from [`AT`].
fn first_x(s: &mut Session, id: u64) -> f64 {
    let u = units(s).into_iter().find(|u| u["id"] == json!(id)).unwrap();
    u["entry"][0].as_f64().unwrap() - AT
}

fn steps(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

/// The second line is turned round: started at its far end, it ends 6 mm from the third, so the
/// thread isn't cut; the reply says what it saved, every part gets its points, one undo step, and
/// Undo takes them all back.
#[test]
fn a_line_is_turned_round_for_the_next_part() {
    let mut s = blank();
    let [a, b, c] = three_lines(&mut s, "#2233cc");
    assert!((first_x(&mut s, b) - 1.0).abs() < 0.1, "Auto starts it at its nearer end");
    let before = steps(&s);
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!((r["applied"].clone(), r["changedCount"].clone(), r["skippedCount"].clone()), (json!(true), json!(3), json!(0)), "{r}");
    assert_eq!((r["before"]["trims"].clone(), r["after"]["trims"].clone(), r["saved"]["trims"].clone()), (json!(1), json!(0), json!(1)), "{r}");
    assert!(r["after"]["length"].as_f64().unwrap() < r["before"]["length"].as_f64().unwrap(), "{r}");
    assert!((first_x(&mut s, b) + 6.0).abs() < 0.1, "from its far end");
    assert_eq!(steps(&s), before + 1);
    let info = run(&mut s, "embroidery.info", json!({"ids": [a, b, c]}));
    for o in info["objects"].as_array().unwrap() {
        assert!(o["settings"]["stroke"]["entry"].is_array() && o["settings"]["stroke"]["exit"].is_array(), "{o}");
    }
    run(&mut s, "edit.undo", json!({}));
    let info = run(&mut s, "embroidery.info", json!({"ids": [a, b, c]}));
    assert!(info["objects"].as_array().unwrap().iter().all(|o| o["settings"]["stroke"]["entry"].is_null()), "{info}");
}

/// Run again on its own result, it changes nothing: no point moves, and no undo step.
#[test]
fn a_second_nearest_connection_changes_nothing() {
    let mut s = blank();
    three_lines(&mut s, "#2233cc");
    run(&mut s, "embroidery.nearestConnection", json!({}));
    let before = steps(&s);
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!((r["applied"].clone(), r["changedCount"].clone(), r["unchangedCount"].clone()), (json!(false), json!(0), json!(3)), "{r}");
    assert_eq!(steps(&s), before);
}

/// The selection alone: the objects outside keep the points they have (none here); the one
/// selected is turned round all the same, as its moves to both are weighed.
#[test]
fn the_selection_alone_is_joined() {
    let mut s = blank();
    let [a, b, c] = three_lines(&mut s, "#2233cc");
    run(&mut s, "select.set", json!({"ids": [b]}));
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(r["changedCount"], json!(1), "{r}");
    assert!((first_x(&mut s, b) + 6.0).abs() < 0.1);
    let info = run(&mut s, "embroidery.info", json!({"ids": [a, c]}));
    assert!(info["objects"].as_array().unwrap().iter().all(|o| o["settings"]["stroke"]["entry"].is_null()), "{info}");
    // `all` joins every object, whatever is selected.
    let r = run(&mut s, "embroidery.nearestConnection", json!({"all": true}));
    assert_eq!(r["changedCount"], json!(2), "{r}");
}

/// Across a colour change no thread lies: the second line is left as Auto sews it, started at its
/// end nearer the first.
#[test]
fn a_colour_change_breaks_the_chain() {
    let mut s = blank();
    let [_, b, _] = three_lines(&mut s, "#22aa44");
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(r["saved"]["trims"], json!(0), "{r}");
    assert!((first_x(&mut s, b) - 1.0).abs() < 0.1, "still from its nearer end");
}

/// Three squares 2 mm apart in a row: each ends where the next begins, so the moves between them
/// are short and none is cut; the moves never come out longer than Auto's.
#[test]
fn fills_end_where_the_next_begins() {
    let mut s = blank();
    for k in 0..3 {
        square(&mut s, (k as f64 * 22.0, 0.0), 20.0);
    }
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    let (before, after) = (r["before"]["length"].as_f64().unwrap(), r["after"]["length"].as_f64().unwrap());
    assert!(after <= before, "{r}");
    assert_eq!(r["after"]["trims"], json!(0), "{r}");
    assert!(after < 10.0, "two moves of a few millimetres: {r}");
}

/// What it can't join is listed with why: type, a symbol's instance, an object not sewn; the parts
/// beside them are joined all the same.
#[test]
fn what_it_cant_join_is_listed() {
    let mut s = blank();
    let [a, b, _] = three_lines(&mut s, "#2233cc");
    let text = run(&mut s, "text.create", json!({"x": 300, "y": 300, "text": "A"}))["id"].as_u64().unwrap();
    run(&mut s, "object.hide", json!({"ids": [a]}));
    run(&mut s, "select.set", json!({"ids": [a, b, text]}));
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    let reasons: Vec<(u64, String)> =
        r["skipped"].as_array().unwrap().iter().map(|k| (k["id"].as_u64().unwrap(), k["reason"].as_str().unwrap().to_string())).collect();
    assert!(reasons.contains(&(text, "type".into())) && reasons.contains(&(a, "notSewn".into())), "{r}");
    assert_eq!(r["changedCount"], json!(1), "{r}");
}

/// A dry run writes nothing and answers what the run would.
#[test]
fn a_dry_run_writes_nothing() {
    let mut s = blank();
    three_lines(&mut s, "#2233cc");
    let before = steps(&s);
    let dry = run(&mut s, "embroidery.nearestConnection", json!({"dryRun": true}));
    assert_eq!((dry["applied"].clone(), steps(&s)), (json!(false), before));
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(
        (dry["after"].clone(), dry["changedCount"].clone(), dry["parts"].clone()),
        (r["after"].clone(), r["changedCount"].clone(), r["parts"].clone())
    );
}

/// Params are strict: an unknown key, `ids` with `all`, a flag that isn't true or false.
#[test]
fn its_params_are_strict() {
    let mut s = blank();
    three_lines(&mut s, "#2233cc");
    for (p, says) in [
        (json!({"dryrun": true}), "dryrun"),
        (json!({"ids": [1], "all": true}), "not both"),
        (json!({"all": 1}), "true or false"),
        (json!({"dryRun": "yes"}), "true or false"),
    ] {
        let e = s.execute("embroidery.nearestConnection", &p).unwrap_err().to_string();
        assert!(e.contains(says), "{p}: {e}");
    }
}

/// Parts that meet exactly are no jump: a line ending where the next starts, and that one ending on
/// a closed line's corner, where Auto starts it. Nearest Connection counts no jump before or after,
/// the design's jumps stay none, and the closed line still starts on its corner. Before, the first
/// needle point of a part met exactly read as its tie-in's step, 1 mm in: a phantom 1 mm jump to
/// the command, and a pin there that made a real one.
#[test]
fn exact_meets_are_no_jump() {
    let mut s = blank();
    line(&mut s, "#2233cc", (0.0, 0.0), (20.0, 0.0));
    line(&mut s, "#2233cc", (20.0, 0.0), (40.0, 0.0));
    let pt = |v: f64| (AT + v) * PT_PER_MM;
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let d = format!("M {} {} L {} {} L {} {} L {} {} Z", pt(40.0), pt(0.0), pt(60.0), pt(0.0), pt(60.0), pt(20.0), pt(40.0), pt(20.0));
    let ring = run(&mut s, "path.create", json!({ "d": d }))["id"].as_u64().unwrap();
    run(&mut s, "stroke.set", json!({"weight": 1.0, "ids": [ring]}));
    run(&mut s, "select.none", json!({}));
    let jumps = |s: &mut Session| run(s, "embroidery.stats", json!({}))["jumps"].as_u64().unwrap();
    assert_eq!(jumps(&mut s), 0, "drawn end to end");
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!((r["before"]["jumps"].clone(), r["after"]["jumps"].clone()), (json!(0), json!(0)), "{r}");
    assert_eq!(jumps(&mut s), 0, "none added: {r}");
    assert!((first_x(&mut s, ring) - 40.0).abs() < 0.1, "the closed line still starts on its corner");
}

/// A closed line left where it starts, in a stretch Nearest Connection keeps (the second line
/// turned round before it), gets its own start written as its entry exactly, so it starts where it
/// did: a diamond whose corner pointing at the third line's end is where Auto starts it and the
/// nearest it can start, 23.1 mm along its 30.3 mm outline, between the points spread along it
/// 0.95 mm apart; a spread point written instead would move it 0.36 mm.
#[test]
fn a_closed_line_left_as_it_is_keeps_its_start() {
    let mut s = blank();
    three_lines(&mut s, "#2233cc");
    let pt = |v: f64| (AT + v) * PT_PER_MM;
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let d = format!("M {} {} L {} {} L {} {} L {} {} Z", pt(35.0), pt(-5.2), pt(41.0), pt(0.0), pt(35.0), pt(5.2), pt(30.0), pt(0.0));
    let ring = run(&mut s, "path.create", json!({ "d": d }))["id"].as_u64().unwrap();
    run(&mut s, "stroke.set", json!({"weight": 1.0, "ids": [ring]}));
    run(&mut s, "select.none", json!({}));
    let start = |s: &mut Session| {
        let u = units(s).into_iter().find(|u| u["id"] == json!(ring)).unwrap();
        (u["entry"][0].as_f64().unwrap() - AT, u["entry"][1].as_f64().unwrap() - AT)
    };
    let before = start(&mut s);
    assert!((before.0 - 30.0).abs() < 0.01 && before.1.abs() < 0.01, "Auto starts it at the corner nearest the third line's end: {before:?}");
    let r = run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(r["applied"], json!(true), "the second line turned round: {r}");
    let entry = run(&mut s, "embroidery.info", json!({"ids": [ring]}))["objects"][0]["settings"]["stroke"]["entry"].clone();
    let (x, y) = (entry[0].as_f64().unwrap(), entry[1].as_f64().unwrap());
    assert!((x - pt(30.0)).abs() < 1e-6 && (y - pt(0.0)).abs() < 1e-6, "its own start, exactly: {entry}");
    let after = start(&mut s);
    assert!((after.0 - before.0).abs() < 1e-6 && (after.1 - before.1).abs() < 1e-6, "{before:?} → {after:?}");
}

/// Nearest Connection plans its trials on a copy of the units kept: a dry run, and a run, add none
/// of their own to the document's, so they never push its own units out (a dry run on the largest
/// design would have filled the cache with units of a document never kept, and the next edit would
/// have planned the whole design cold).
#[test]
fn its_trials_keep_none_of_their_units() {
    let mut s = blank();
    three_lines(&mut s, "#2233cc");
    run(&mut s, "embroidery.sequence", json!({}));
    let uid = s.doc().unwrap().uid;
    let kept = |s: &mut Session| s.embroidery_plans.of(uid).kept();
    let before = kept(&mut s);
    assert!(before > 0, "the plan's units kept");
    run(&mut s, "embroidery.nearestConnection", json!({"dryRun": true}));
    assert_eq!(kept(&mut s), before, "a dry run");
    run(&mut s, "embroidery.nearestConnection", json!({}));
    assert_eq!(kept(&mut s), before, "a run");
}
