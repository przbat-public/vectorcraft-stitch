//! The Check tab: its rules in order, their words from ids and numbers (in English and Polish),
//! the format chosen, its states, and a real check end to end.

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
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 900.0))),
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
    assert!(words.iter().any(|w| w.starts_with("Stitch ") && w.ends_with(" mm")), "{words:?}");
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
    for t in [1.0, 1.05, 1.1, 1.2] {
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
    for bad in [json!({"rect": [200, 100, 100, 150]}), json!({"rect": [0, 0, 1]}), json!({}), json!({"rect": [0, 0, 1, 1], "margin": -1})] {
        assert!(app.run("view.zoomTo", bad.clone()).is_err(), "{bad}");
    }
    app.canvas_rect = None;
    assert!(app.run("view.zoomTo", json!({"rect": [0, 0, 1, 1]})).is_err(), "no canvas");
}
