//! The Check tab: its rules in order, their words from ids and numbers (in English and Polish),
//! the format chosen, its states, and a real check end to end.

use std::sync::Arc;

use egui::Shape;
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;
use crate::panels::embroidery::{Tab, show as panel};

/// Three overlapping squares in three colours, and a word of type: layers (a warning), stitches
/// shorter than the needle (a note) and the type left out (a warning).
fn app_with_findings() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for (x, color) in [(20, "#cc2222"), (40, "#2233cc"), (60, "#22aa33")] {
        app.run("select.set", json!({"ids": []})).unwrap();
        app.run("paint.setFill", json!({ "color": color })).unwrap();
        app.run("shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40})).unwrap();
    }
    app.run("text.create", json!({"x": 10, "y": 150, "text": "Haft"})).unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    app.ui.embroidery_panel.tab = Tab::Check;
    app
}

/// One headless frame of the panel at `time` → the words drawn.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64) -> Vec<String> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 1400.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        panel(app, ui);
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut words = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) => words.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    words
}

fn ready(app: &mut VectorcraftApp, ctx: &egui::Context) -> Vec<String> {
    for t in [0.0, 0.05] {
        frame(app, ctx, t);
    }
    frame(app, ctx, 0.1)
}

fn has(words: &[String], what: &str) -> bool {
    words.iter().any(|w| w.contains(what))
}

fn check_reply(app: &mut VectorcraftApp, params: Value) -> Value {
    app.session.execute("embroidery.check", &params).unwrap()
}

/// Every rule the engine checks has the interface's own words.
#[test]
fn every_rule_the_engine_lists_is_worded() {
    let mut app = app_with_findings();
    let reply = check_reply(&mut app, json!({}));
    let ids: Vec<&str> = reply["rules"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, WORDED);
    for rule in reply["rules"].as_array().unwrap() {
        let words = title(rule, "Janome JEF", Lang::EN);
        assert_ne!(words, rule["title"].as_str().unwrap(), "{}", rule["id"]);
        assert!(!words.contains('{'), "{words}");
        assert!(!found(rule, Lang::EN).contains('{'));
    }
    // A rule this version doesn't know shows the engine's title.
    let unknown = json!({"id": "newRule", "title": "A newer check", "severity": "warning", "hits": 0});
    assert_eq!(title(&unknown, "", Lang::EN), "A newer check");
}

/// Problems come first: the warnings, then the note, then what passed.
#[test]
fn problems_come_first() {
    let mut app = app_with_findings();
    let reply = check_reply(&mut app, json!({}));
    let order: Vec<&str> = ordered(&reply).iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(&order[..3], ["layers", "notSewn", "stitchBelowNeedle"], "{order:?}");
    assert!(order[3..].iter().all(|id| check_reply(&mut app, json!({"rules": [id]}))["rules"][0]["hits"] == json!(0)));
}

/// The thresholds are the format's own where the format sets them, written as the language writes
/// numbers; a format with no colour limit says so.
#[test]
fn titles_carry_the_formats_threshold_in_the_languages_numbers() {
    let mut app = app_with_findings();
    let rule = |reply: &Value, id: &str| reply["rules"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap().clone();
    let dst = check_reply(&mut app, json!({"format": "dst"}));
    assert_eq!(title(&rule(&dst, "formatStep"), format_label("dst"), Lang::EN), "Stitches longer than one Tajima DST record (12.1 mm)");
    let jef = check_reply(&mut app, json!({"format": "jef"}));
    assert_eq!(title(&rule(&jef, "formatStep"), format_label("jef"), Lang::EN), "Stitches longer than one Janome JEF record (12.7 mm)");
    assert_eq!(title(&rule(&jef, "formatColors"), format_label("jef"), Lang::EN), "Janome JEF holds any number of colour blocks");
    let pes = check_reply(&mut app, json!({"format": "pes"}));
    assert_eq!(title(&rule(&pes, "formatColors"), format_label("pes"), Lang::EN), "More colour blocks than Brother PES holds (255)");
    let pl = Lang::from_code("pl").unwrap();
    crate::i18n::set_current(pl);
    let below = title(&rule(&jef, "stitchBelowNeedle"), "", pl);
    crate::i18n::set_current(Lang::EN);
    assert!(below.contains("0,75") && !below.contains("0.75"), "{below}");
}

/// What each rule found, or the extreme it measured when it found nothing.
#[test]
fn findings_and_what_passed_rules_measured() {
    let mut app = app_with_findings();
    let reply = check_reply(&mut app, json!({}));
    let line = |id: &str| found(reply["rules"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap(), Lang::EN);
    assert!(line("layers").ends_with("spots · 1 object"), "the third square on the first two: {}", line("layers"));
    assert!(line("stitchBelowNeedle").contains("stitches · 3 objects"), "{}", line("stitchBelowNeedle"));
    assert_eq!(line("notSewn"), "1 object");
    assert!(line("stitchTooLong").starts_with("Longest: ") && line("stitchTooLong").ends_with(" mm"), "{}", line("stitchTooLong"));
    assert!(line("fillStitchShort").starts_with("Shortest: "), "{}", line("fillStitchShort"));
    assert_eq!(line("runStitchShort"), "No findings", "no stroke");
    assert_eq!(line("satinTooWide"), "No findings", "no satin column");
    assert_eq!(line("formatColors"), "3 thread blocks");
    assert_eq!(line("machineColorChanges"), "2 thread changes");
}

/// The tab drawn: the summary, the rules' words, the type left out as the plan says it, what isn't
/// checked; an opened rule lists its source and first hits.
#[test]
fn the_tab_drawn() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let words = ready(&mut app, &ctx);
    assert!(has(&words, "2 warnings · 1 note"), "{words:?}");
    assert!(has(&words, "3 or more layers of stitches on one spot"), "{words:?}");
    assert!(has(&words, "Art that isn't sewn"), "{words:?}");
    assert!(has(&words, "type is not stitched"), "{words:?}");
    assert!(has(&words, "Stitches shorter than the needle is thick (0.75 mm)"), "{words:?}");
    assert!(has(&words, "Not checked: stitch density, needle penetrations in an area, tie stitches"), "{words:?}");
    assert!(has(&words, "Janome JEF"), "the palette's format: {words:?}");
    assert!(!has(&words, "Source:"), "closed rules: {words:?}");
    app.embroidery.check.open.insert("stitchBelowNeedle".into());
    let words = frame(&mut app, &ctx, 1.0);
    assert!(has(&words, "Source: Melco"), "{words:?}");
    assert!(words.iter().any(|w| w.starts_with("Record ") && w.ends_with(" mm")), "{words:?}");
    assert!(words.iter().any(|w| w.starts_with("and ") && w.ends_with(" more")), "{words:?}");
    // A source with no address (the plan's own warnings) is no citation.
    app.embroidery.check.open.insert("notSewn".into());
    let words = frame(&mut app, &ctx, 2.0);
    assert!(!has(&words, "docs/embroidery.md"), "{words:?}");
}

/// A format chosen re-checks for it, without planning again; the choice is the document's.
#[test]
fn a_format_chosen_checks_for_it() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    ready(&mut app, &ctx);
    let uid = app.session.active().unwrap().uid;
    let generation = app.embroidery.preview.shown(uid).unwrap().planned.generation;
    app.embroidery.check.format.insert(uid, "dst");
    let mut words = vec![];
    // Frames past the quiet time, in case the plan shown went out of date meanwhile (the fonts'
    // generation is the process's, which other tests change).
    for t in [1.0, 1.05, 1.1, 1.2, 1.4, 1.6] {
        words = frame(&mut app, &ctx, t);
    }
    assert!(has(&words, "Tajima DST"), "{words:?}");
    assert_eq!(app.embroidery.preview.shown(uid).unwrap().planned.generation, generation, "not planned again");
    assert_eq!(query(&app.embroidery.check, uid).params["format"], json!("dst"));
    assert!(query(&app.embroidery.check, uid + 1).params.get("format").is_none(), "another document keeps the palette's");
}

/// Nothing found: no problems, and everything is sewn.
#[test]
fn nothing_found() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 20, "height": 20})).unwrap();
    let reply = check_reply(&mut app, json!({"rules": ["notSewn", "layers"]}));
    assert_eq!(summary(&reply), "No problems found");
    assert_eq!(title(&reply["rules"][0], "", Lang::EN), "Everything is sewn");
}

/// The tab in Polish: its words from the catalogue, its numbers with a decimal comma.
#[test]
fn the_tab_in_polish() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    crate::i18n::set_current(Lang::from_code("pl").unwrap());
    let words = ready(&mut app, &ctx);
    crate::i18n::set_current(Lang::EN);
    assert!(!has(&words, "Art that isn't sewn") && !has(&words, "Not checked"), "{words:?}");
    assert!(has(&words, "0,75"), "{words:?}");
}

/// `view.zoomTo` (what Show zooms with) centres a box and fits it in the canvas less the margin; it
/// refuses a box that isn't one.
#[test]
fn zoom_to_fits_a_box() {
    let mut app = app_with_findings();
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
    let zoomed = app.run("view.zoomTo", json!({"rect": [100, 100, 200, 150], "margin": 50})).unwrap();
    let v = app.view().unwrap();
    assert_eq!((v.center.x, v.center.y), (150.0, 125.0));
    // 700 points of width for 100 of box, 500 of height for 50: the width decides.
    assert!((v.zoom - 7.0).abs() < 1e-9, "{}", v.zoom);
    assert!((zoomed["zoom"].as_f64().unwrap() - 700.0).abs() < 1e-6);
    // A point is fitted as a box a point wide, as the fit commands fit: 540 of 600 points high.
    app.run("view.zoomTo", json!({"rect": [10, 10, 10, 10]})).unwrap();
    assert_eq!(app.view().unwrap().zoom, 540.0);
    let view = |app: &VectorcraftApp| app.view().map(|v| (v.center.x, v.center.y, v.zoom)).unwrap();
    let before = view(&app);
    for bad in [
        json!({"rect": [200, 100, 100, 150]}),
        json!({"rect": [0, 0, 1]}),
        json!({}),
        json!({"rect": [0, 0, 1, 1], "margin": -1}),
        // Exactly four numbers: nothing else is skipped over.
        json!({"rect": [0, "x", 10, 10, 20]}),
        json!({"rect": [0, null, 10, 10, 20]}),
        // Within the document's range: a far centre would stop the rulers' ticks advancing.
        json!({"rect": [1e15, 1e15, 1e15, 1e15]}),
        json!({"rect": [1e308, 0, 1.7e308, 0]}),
        json!({"rect": [0, 0, vectorcraft_doc::MAX_COORD * 2.0, 10]}),
    ] {
        let err = app.run("view.zoomTo", bad.clone()).unwrap_err();
        assert_eq!(view(&app), before, "{bad}: the view stays");
        if err.starts_with("rect:") {
            let pl = crate::i18n::message(crate::i18n::Lang::from_code("pl").unwrap(), &err);
            assert!(pl != err && pl.contains("4000000"), "{bad}: {err} in Polish: {pl}");
        }
    }
    // The document's whole range: centred halfway.
    let m = vectorcraft_doc::MAX_COORD;
    app.run("view.zoomTo", json!({"rect": [-m, -m, m, m]})).unwrap();
    assert_eq!((app.view().unwrap().center.x, app.view().unwrap().center.y), (0.0, 0.0));
    app.canvas_rect = None;
    assert!(app.run("view.zoomTo", json!({"rect": [0, 0, 1, 1]})).is_err(), "no canvas");
}

/// The check as the tab shows it, ready.
fn shown_check(app: &mut VectorcraftApp, ctx: &egui::Context) -> (u64, Value) {
    app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));
    ready(app, ctx);
    let uid = app.session.active().unwrap().uid;
    let q = query(&app.embroidery.check, uid);
    let r = app.embroidery.preview.shown(uid).unwrap().reply(&q).unwrap();
    let (result, current) = (r.result, r.current);
    assert!(current);
    (uid, (**result.as_ref().unwrap()).clone())
}

fn selection(app: &VectorcraftApp) -> Vec<u64> {
    app.session.doc().unwrap().selection.objects.iter().map(|id| id.0).collect()
}

/// `embroidery.showFinding` is Show as a command: a rule's objects, or one hit's, from the check
/// the tab shows; then Next Finding steps from it. Refused with no check of the document as it is,
/// for a rule or hit the check hasn't, or junk.
#[test]
fn show_finding_is_show_as_a_command() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    assert!(app.run("embroidery.showFinding", json!({"rule": "layers"})).is_err(), "no check yet");
    let (_, reply) = shown_check(&mut app, &ctx);
    let rule = rule_of(&reply, "stitchBelowNeedle").unwrap().clone();
    let shown = app.run("embroidery.showFinding", json!({"rule": "stitchBelowNeedle"})).unwrap();
    assert_eq!(shown, json!({"rule": "stitchBelowNeedle", "hit": null}));
    let objects: Vec<u64> = rule["objects"].as_array().unwrap().iter().map(|o| o.as_u64().unwrap()).collect();
    assert_eq!(selection(&app), objects);
    assert_eq!(app.run("embroidery.nextHit", json!({})).unwrap()["hit"], 0, "Next Finding goes on from it");
    assert_eq!(app.run("embroidery.showFinding", json!({"rule": "stitchBelowNeedle", "hit": 1})).unwrap()["hit"], 1);
    for bad in [
        json!({}),
        json!({"rule": "nothing"}),
        json!({"rule": "stitchBelowNeedle", "hit": 999}),
        json!({"rule": "layers", "hit": -1}),
        json!({"rule": "layers", "hits": 1}),
    ] {
        assert!(app.run("embroidery.showFinding", bad.clone()).is_err(), "{bad}");
    }
}

/// Show selects a rule's objects and zooms to its hits; its stitches stand out for a moment.
#[test]
fn show_selects_the_rules_objects_and_zooms_to_its_hits() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    let rule = rule_of(&reply, "stitchBelowNeedle").unwrap().clone();
    show_findings(&mut app, uid, &reply, "stitchBelowNeedle", None).unwrap();
    let objects: Vec<u64> = rule["objects"].as_array().unwrap().iter().map(|o| o.as_u64().unwrap()).collect();
    assert_eq!(selection(&app), objects);
    // The view is centred inside the hits' box.
    let points: Vec<Point> = listed(&rule).iter().flat_map(hit_points).map(|p| Point::new(mm_to_pt(p.x), mm_to_pt(p.y))).collect();
    let (x0, x1) = points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
    let c = app.view().unwrap().center;
    assert!(c.x >= x0 && c.x <= x1, "{c:?} in {x0}..{x1}");
    // The next frame stands the objects' stitches out, and the row is the focus.
    frame(&mut app, &ctx, 1.0);
    assert!(app.embroidery.layer.emphasis().is_some_and(|u| !u.is_empty()));
    assert_eq!(app.embroidery.check.focus.as_ref().map(|f| f.rule.as_str()), Some("stitchBelowNeedle"));
    // Its units are found once for the plan, not on every frame.
    let units = |app: &VectorcraftApp| app.embroidery.check.focus.as_ref().and_then(|f| f.units.clone()).unwrap().1;
    let found = units(&app);
    frame(&mut app, &ctx, 1.05);
    assert!(Arc::ptr_eq(&found, &units(&app)));
    assert_eq!(app.embroidery.layer.emphasis(), Some(&found[..]));
    // Only for a moment.
    app.embroidery.layer.emphasize(vec![]);
    frame(&mut app, &ctx, 1.0 + SHOW_EMPHASIS_SECS + 0.1);
    assert_eq!(app.embroidery.layer.emphasis(), Some(&[][..]), "not asked again");
}

/// A hit shown selects its own object and zooms to it; art left out is shown by its objects.
#[test]
fn a_hit_and_art_left_out_shown() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    let hit = listed(rule_of(&reply, "stitchBelowNeedle").unwrap())[2].clone();
    show_findings(&mut app, uid, &reply, "stitchBelowNeedle", Some(2)).unwrap();
    assert_eq!(selection(&app), [hit["id"].as_u64().unwrap()]);
    let wide = app.view().unwrap().zoom;
    assert!(wide > 1.0, "zoomed in on one stitch: {wide}");
    // The type left out: no point to zoom to, so its object's box.
    show_findings(&mut app, uid, &reply, "notSewn", None).unwrap();
    let text = app.session.doc().unwrap().doc.clone();
    let id = selection(&app);
    assert_eq!(id.len(), 1);
    let bounds = text.node(NodeId(id[0])).unwrap().geometric_bounds().unwrap();
    let c = app.view().unwrap().center;
    assert!((c.x - bounds.center().x).abs() < 1e-6 && (c.y - bounds.center().y).abs() < 1e-6, "{c:?} {bounds:?}");
}

/// Objects deleted since the check are left out of what Show selects.
#[test]
fn show_skips_objects_deleted_since_the_check() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    let objects: Vec<u64> =
        rule_of(&reply, "stitchBelowNeedle").unwrap()["objects"].as_array().unwrap().iter().map(|o| o.as_u64().unwrap()).collect();
    app.run("select.set", json!({"ids": [objects[0]]})).unwrap();
    app.run("edit.clear", json!({})).unwrap();
    show_findings(&mut app, uid, &reply, "stitchBelowNeedle", None).unwrap();
    assert_eq!(selection(&app), objects[1..]);
}

/// Next and Previous Finding step through the listed hits of the rule shown, round from the end;
/// they are off while the check is of an older plan.
#[test]
fn next_and_previous_finding() {
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    assert!(!crate::menus::enabled(&app, "embroidery.nextHit"), "nothing shown yet");
    assert!(app.run("embroidery.nextHit", json!({})).is_err());
    show_findings(&mut app, uid, &reply, "layers", None).unwrap();
    assert!(crate::menus::enabled(&app, "embroidery.nextHit"));
    let n = listed(rule_of(&reply, "layers").unwrap()).len();
    assert_eq!(app.run("embroidery.nextHit", json!({})).unwrap()["hit"], json!(0));
    assert_eq!(app.run("embroidery.nextHit", json!({})).unwrap()["hit"], json!(1));
    assert_eq!(app.run("embroidery.previousHit", json!({})).unwrap()["hit"], json!(0));
    assert_eq!(app.run("embroidery.previousHit", json!({})).unwrap()["hit"], json!(n - 1), "round to the last");
    // A change: off until the new plan's check comes.
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    assert!(!crate::menus::enabled(&app, "embroidery.nextHit"));
    assert!(app.run("embroidery.nextHit", json!({})).is_err());
    for t in [2.0, 2.05, 2.1, 2.2, 2.3] {
        frame(&mut app, &ctx, t);
    }
    assert!(crate::menus::enabled(&app, "embroidery.nextHit"));
}

/// The canvas's marks: the warnings' hits while the tab shows; a note's only once shown; the
/// focus's first, its hit marked; none on a check of an older plan, none once the tab is gone, none
/// when turned off; at most the cap.
#[test]
fn the_canvas_marks_the_findings_while_the_tab_shows() {
    use crate::embroidery::check_marks::{MAX_DRAWN_MARKS, marks};
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    let pass = ctx.cumulative_pass_nr();
    let all = marks(&app, pass);
    let layers = listed(rule_of(&reply, "layers").unwrap()).len();
    assert_eq!(all.len(), layers, "the layers warning's hits; the note's aren't marked");
    assert!(all.iter().all(|m| m.rule == "layers" && m.mark == Mark::Warning && m.half == 0.0));
    // A note shown: its hits first, the one shown marked.
    show_findings(&mut app, uid, &reply, "stitchBelowNeedle", Some(1)).unwrap();
    let all = marks(&app, pass);
    let first = all.first().unwrap();
    assert_eq!((first.rule.as_str(), first.mark), ("stitchBelowNeedle", Mark::Info));
    assert!(first.half > 0.0, "a stitch is ringed round its length");
    assert!(all.iter().filter(|m| m.focused).map(|m| (m.rule.as_str(), m.hit)).eq([("stitchBelowNeedle", 1)]));
    assert!(all.len() <= MAX_DRAWN_MARKS);
    // Turned off, the tab gone, or the check of an older plan: none.
    assert_eq!(app.run("view.checkMarks", json!({"on": false})).unwrap(), json!({"on": false}));
    assert!(marks(&app, pass).is_empty());
    assert!(!crate::menus::checked(&app, "view.checkMarks", &json!({})).unwrap());
    for bad in [json!({"show": false}), json!({"on": 1})] {
        assert!(app.run("view.checkMarks", bad.clone()).is_err(), "{bad}");
    }
    assert!(marks(&app, pass).is_empty(), "a refused call changes nothing");
    assert_eq!(app.run("view.checkMarks", json!({})).unwrap(), json!({"on": true}));
    assert!(!marks(&app, pass).is_empty());
    assert!(marks(&app, pass + 2).is_empty(), "the tab no longer drawn");
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    assert!(marks(&app, pass).is_empty(), "the document changed since the check");
}

/// The marks drawn: rings and badges on the canvas, and a hovered mark's words.
#[test]
fn the_marks_drawn_and_hovered() {
    use crate::embroidery::check_marks::{draw, marks};
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    show_findings(&mut app, uid, &reply, "layers", Some(0)).unwrap();
    let pass = ctx.cumulative_pass_nr();
    let first = marks(&app, pass).first().cloned().unwrap();
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let view = *app.view().unwrap();
    let xf = crate::canvas::Xf::new(canvas, &view);
    let at = xf.to_screen(first.centre);
    let (mut circles, mut words) = (0, vec![]);
    // The tab drawn in these passes too, so that its marks show; a tooltip shows from its second.
    for time in [5.0, 5.1] {
        let raw = egui::RawInput { time: Some(time), screen_rect: Some(canvas), events: vec![egui::Event::PointerMoved(at)], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            super::super::show(&mut app, ui);
            draw(&app, ui.ctx(), &ui.painter_at(canvas), &xf);
        });
        out.textures_delta.clear();
        (circles, words) = (0, vec![]);
        let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
        while let Some(s) = pending.pop() {
            match s {
                Shape::Vec(v) => pending.extend(v),
                Shape::Circle(_) => circles += 1,
                Shape::Text(t) => words.push(t.galley.text().to_string()),
                _ => {}
            }
        }
    }
    assert!(circles >= 3 * marks(&app, pass).len(), "{circles}");
    assert!(words.iter().any(|w| w.starts_with("Warning: 3 or more layers") && w.contains("\nRecord ")), "{words:?}");
}

/// A stats reply with these counts in its 27 bins (half a millimetre each up to 12.5, then 12.7,
/// then open), and median `median`.
fn lengths_reply(counts: &[u64], median: Option<f64>) -> Value {
    let edge = |k: usize| match k {
        k if k <= 25 => Some(k as f64 * 0.5),
        26 => Some(12.7),
        _ => None,
    };
    let bins: Vec<Value> = (0..27).map(|k| json!({"from": edge(k), "to": edge(k + 1), "count": counts.get(k).copied().unwrap_or(0)})).collect();
    json!({"lengths": {"bins": bins, "median": median}})
}

/// The chart drawn alone → its filled rectangles' colours and heights, and its words.
fn chart(stats: &Value, too_long: Option<f64>) -> (Vec<(Color32, f32)>, Vec<String>) {
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| super::super::lengths::show(ui, stats, too_long, false));
    out.textures_delta.clear();
    let (mut rects, mut words) = (vec![], vec![]);
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Rect(r) => rects.push((r.fill, r.rect.height())),
            Shape::Text(t) => words.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    (rects, words)
}

/// A bar per bin with stitches, as tall as its count against the tallest (never thinner than the
/// least a bar is); the band from 12.7 mm, its bar in the warning colour; the median in the
/// header; an empty design draws no chart.
#[test]
fn the_stitch_length_chart() {
    use super::super::lengths::{CHART_PT, MIN_BAR_PT};
    let t = Tokens::get(&egui::Context::default());
    let mut counts = vec![0u64; 27];
    counts[6] = 1000;
    counts[7] = 1;
    counts[26] = 4;
    let (rects, words) = chart(&lengths_reply(&counts, Some(3.25)), Some(12.7));
    let accent: Vec<f32> = rects.iter().filter(|(c, _)| *c == t.accent).map(|(_, h)| *h).collect();
    assert_eq!(accent.len(), 2, "{rects:?}");
    assert!(accent.contains(&CHART_PT) && accent.contains(&MIN_BAR_PT), "{accent:?}");
    let warning = crate::embroidery::marks::colour(&t, Mark::Warning);
    assert_eq!(rects.iter().filter(|(c, _)| *c == warning).count(), 1, "the band's bar");
    assert!(words.iter().any(|w| w == "Too long"), "{words:?}");
    assert!(words.iter().any(|w| w == "· Median: 3.3 mm" || w == "· Median: 3.2 mm"), "{words:?}");
    for axis in ["0", "5", "10", "12.7"] {
        assert!(words.iter().any(|w| w == axis), "{axis}: {words:?}");
    }
    // No band without the check's threshold.
    let (rects, words) = chart(&lengths_reply(&counts, Some(3.25)), None);
    assert!(!rects.iter().any(|(c, _)| *c == warning) && !words.iter().any(|w| w == "Too long"));
    // Nothing to chart.
    assert!(chart(&lengths_reply(&[], None), Some(12.7)).1.is_empty());
}

#[test]
fn a_bins_words() {
    use super::super::lengths::{Bin, bin_words};
    assert_eq!(bin_words(&Bin { from: 0.5, to: Some(1.0), count: 3 }), "0.5–1 mm: 3 stitches");
    assert_eq!(bin_words(&Bin { from: 12.7, to: None, count: 1 }), "Over 12.7 mm: 1 stitch");
    // The real design's figures read back.
    let mut app = app_with_findings();
    let stats = app.session.execute("embroidery.stats", &json!({})).unwrap();
    let (bins, median) = super::super::lengths::bins(&stats).unwrap();
    assert_eq!((bins.len(), bins.last().unwrap().to), (27, None));
    assert!(median.is_some_and(|m| m > 0.0));
}

/// Rings that would stack are left out: at a small zoom, the hits half a millimetre apart along an
/// edge keep a ring each only where no ring kept already covers them; the focus's always.
#[test]
fn stacked_rings_are_thinned() {
    use crate::embroidery::check_marks::{marks, thinned};
    let mut app = app_with_findings();
    let ctx = egui::Context::default();
    let (uid, reply) = shown_check(&mut app, &ctx);
    show_findings(&mut app, uid, &reply, "layers", Some(5)).unwrap();
    let all = marks(&app, ctx.cumulative_pass_nr());
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut view = *app.view().unwrap();
    view.zoom = 0.5;
    let kept = thinned(&all, &crate::canvas::Xf::new(canvas, &view));
    assert!(kept.len() < all.len() / 2, "{} of {}", kept.len(), all.len());
    assert!(kept.first().is_some_and(|(m, ..)| m.focused), "the focus's ring first");
    for (k, (_, a, _)) in kept.iter().enumerate() {
        for (_, b, rb) in &kept[..k] {
            assert!(a.distance(*b) >= *rb, "a ring's centre inside one kept before it");
        }
    }
    // Zoomed in, they stand apart: every one is kept.
    view.zoom = 40.0;
    assert_eq!(thinned(&all, &crate::canvas::Xf::new(canvas, &view)).len(), all.len());
}

/// The cap comes after the thinning: one rule's hits stacked on a spot never take the room of
/// another rule's, which stand apart; and never more than the cap are drawn.
#[test]
fn the_cap_comes_after_the_thinning() {
    use crate::embroidery::check_marks::{CanvasMark, MAX_DRAWN_MARKS, thinned};
    let mark = |rule: &str, k: usize, x: f64| CanvasMark {
        rule: rule.into(),
        hit: k,
        mark: Mark::Warning,
        centre: Point::new(x, 0.0),
        half: 0.0,
        focused: false,
    };
    // A first rule's many hits on one spot, then a second's apart.
    let mut all: Vec<CanvasMark> = (0..MAX_DRAWN_MARKS * 2).map(|k| mark("layers", k, 0.0)).collect();
    all.extend((0..10).map(|k| mark("stitchTooLong", k, 100.0 * (k as f64 + 1.0))));
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let view = crate::state::View { zoom: 1.0, center: Point::new(500.0, 0.0), ..Default::default() };
    let kept = thinned(&all, &crate::canvas::Xf::new(canvas, &view));
    assert_eq!(kept.iter().filter(|(m, ..)| m.rule == "stitchTooLong").count(), 10, "the second rule's rings all drawn");
    assert_eq!(kept.iter().filter(|(m, ..)| m.rule == "layers").count(), 1, "the stack, once");
    // Spread out, more than the cap: the cap.
    let spread: Vec<CanvasMark> = (0..MAX_DRAWN_MARKS * 2).map(|k| mark("layers", k, 50.0 * k as f64)).collect();
    assert_eq!(thinned(&spread, &crate::canvas::Xf::new(canvas, &view)).len(), MAX_DRAWN_MARKS);
}
