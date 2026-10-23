//! The embroidery interface drawn in every language: the Embroidery panel's tabs (a check's rule
//! opened and shown, its chart) and the status bar's counters draw no English the language's
//! catalogue doesn't give, and the panel's words stay inside it at 256 and 300 points; and what
//! screen readers are told.

use egui::accesskit::{Node, Role};
use egui::{Pos2, Rect, Shape, vec2};
use serde_json::json;
use vectorcraft_engine::Session;

use crate::i18n::{LANGUAGES, Lang};
use crate::panels::embroidery::{Tab, check};
use crate::theme;
use crate::{Services, VectorcraftApp};

/// The window's width, points: room for the status bar's whole counters.
const WINDOW: f32 = 1600.0;
const HEIGHT: f32 = 1400.0;

/// Three overlapping squares in three colours and a word of type: a warning, a note and art left
/// out, so that every kind of row draws.
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
    app
}

/// Frames of the status bar and the Embroidery panel at `tab`, `width` points wide on the
/// window's right, until the plan and its replies are in → the last frame's texts with their boxes,
/// and its screen-reader tree.
fn frames(app: &mut VectorcraftApp, ctx: &egui::Context, tab: Tab, width: f32) -> (Vec<(String, Rect)>, Vec<Node>) {
    app.ui.embroidery_panel.tab = tab;
    let mut texts = vec![];
    let mut nodes = vec![];
    for time in [0.0, 0.05, 0.1, 0.2, 0.3] {
        let raw = egui::RawInput { time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW, HEIGHT))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            crate::chrome::status_bar(app, ui);
            egui::Panel::right("embroidery-test").exact_size(width).resizable(false).show(ui, |ui| crate::panels::embroidery::show(app, ui));
            crate::embroidery::stitch_preview::schedule(app, ui.ctx());
        });
        out.textures_delta.clear();
        texts.clear();
        let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
        while let Some(s) = pending.pop() {
            match s {
                Shape::Vec(v) => pending.extend(v),
                Shape::Text(t) => texts.push((t.galley.text().to_string(), t.visual_bounding_rect())),
                _ => {}
            }
        }
        nodes = out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
    }
    (texts, nodes)
}

/// The Check tab with its layers rule shown (selected, zoomed to, focused) and opened.
fn check_tab(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> (Vec<(String, Rect)>, Vec<Node>) {
    frames(app, ctx, Tab::Check, width);
    let uid = app.session.active().unwrap().uid;
    let q = check::query(&app.embroidery.check, uid);
    let reply = app.embroidery.preview.shown(uid).and_then(|s| s.reply(&q)).and_then(|r| r.result.clone().ok()).expect("the check");
    app.canvas_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
    check::show_findings(app, uid, &reply, "layers", Some(0)).unwrap();
    app.embroidery.check.open.insert("layers".into());
    frames(app, ctx, Tab::Check, width)
}

/// A drawn text that may read as its English: a name (a format's, a thread's, a source's), a
/// number with its unit, or what the language's catalogue `catalogue` gives (a row translated to
/// the same words, as German's "Design").
fn may_be_english(catalogue: &str, text: &str, names: &[String]) -> bool {
    let letters = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let units = text.split_whitespace().all(|w| ["mm", "min", "fps", "ms"].contains(&w) || !w.chars().any(|c| c.is_ascii_alphabetic()));
    if text.contains(" · ") {
        return text.split(" · ").all(|part| may_be_english(catalogue, part, names));
    }
    let text = text.trim_start_matches("· ");
    let translated = || catalogue.lines().filter_map(|l| l.rsplit('\t').next()).flat_map(|t| t.split('|')).any(|t| fills(t, text));
    letters < 3 || units || names.iter().any(|n| text.contains(n.as_str())) || translated()
}

/// Whether `text` is template `template` with its `{placeholders}` filled (any text for each).
fn fills(template: &str, text: &str) -> bool {
    let mut pieces = vec![];
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        pieces.push(&rest[..open]);
        rest = &rest[open + close + 1..];
    }
    pieces.push(rest);
    let Some((first, others)) = pieces.split_first() else { return false };
    let Some(mut left) = text.strip_prefix(first) else { return false };
    let Some((last, middle)) = others.split_last() else { return left.is_empty() };
    for piece in middle {
        let Some(at) = left.find(piece) else { return false };
        left = &left[at + piece.len()..];
    }
    left.ends_with(last)
}

/// The names the interface draws as they are: the formats', the threads', the sources', and the
/// objects' as the Layers panel names them in `lang`.
fn names(app: &VectorcraftApp, lang: Lang) -> Vec<String> {
    let mut names: Vec<String> = vectorcraft_engine::cmd::embroidery::check_formats().into_iter().map(|(_, label)| label.to_string()).collect();
    let uid = app.session.active().map(|st| st.uid).unwrap_or_default();
    if let Some(shown) = app.embroidery.preview.shown(uid) {
        names.extend(shown.planned.plan.blocks.iter().map(|b| b.thread.name().to_string()));
        let doc = &shown.document;
        names.extend(
            shown.planned.plan.units.iter().filter_map(|u| doc.node(u.origin.selected())).map(|n| crate::panels::layers::shown_name(app, n, lang)),
        );
        let q = check::query(&app.embroidery.check, uid);
        if let Some(Ok(reply)) = shown.reply(&q).map(|r| r.result) {
            for rule in reply["rules"].as_array().into_iter().flatten() {
                names.extend(rule["sources"].as_array().into_iter().flatten().filter_map(|s| s["name"].as_str().map(str::to_string)));
            }
        }
    }
    names.retain(|n| !n.is_empty());
    names
}

/// In every complete language, the panel's two tabs and the counters draw no English that the
/// language's catalogue doesn't give; the panel's words stay inside it at 256 and 300 points.
#[test]
fn the_embroidery_interface_in_every_language() {
    let english: Vec<String> = {
        crate::i18n::set_current(Lang::EN);
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut app = app_with_findings();
        let (mut texts, _) = frames(&mut app, &ctx, Tab::Order, 300.0);
        texts.extend(check_tab(&mut app, &ctx, 300.0).0);
        texts.into_iter().map(|(t, _)| t).collect()
    };
    for info in LANGUAGES.iter().filter(|l| l.complete_menus) {
        let lang = Lang::from_code(info.code).unwrap();
        let catalogue = info.source;
        for width in [256.0, 300.0] {
            crate::i18n::set_current(lang);
            let ctx = egui::Context::default();
            theme::install_fonts(&ctx);
            let mut app = app_with_findings();
            let order = frames(&mut app, &ctx, Tab::Order, width).0;
            let checked = check_tab(&mut app, &ctx, width).0;
            let names = names(&app, lang);
            crate::i18n::set_current(Lang::EN);
            let panel = Rect::from_min_max(Pos2::new(WINDOW - width, 0.0), Pos2::new(WINDOW, HEIGHT - 24.0));
            let counters = app.embroidery.counters.inspect()["shown"].as_str().unwrap_or_default().to_string();
            for texts in [order, checked] {
                // The panel's words overlap nothing (the icons, unloaded in the tests, draw a
                // placeholder).
                let panel_texts: Vec<&(String, Rect)> = texts.iter().filter(|(t, r)| panel.intersects(*r) && t != "⚠").collect();
                for (k, (a, ra)) in panel_texts.iter().enumerate() {
                    for (b, rb) in &panel_texts[k + 1..] {
                        assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{} at {width}: {a:?} overlaps {b:?}", info.code);
                    }
                }
                // The panel's words and the counters (the rest of the status bar is upstream's).
                for (text, rect) in texts.iter().filter(|(t, r)| panel.intersects(*r) || *t == counters) {
                    if english.contains(text) {
                        assert!(may_be_english(catalogue, text, &names), "{}: {text:?} is drawn in English", info.code);
                    }
                    if panel.intersects(*rect) {
                        assert!(
                            rect.left() >= panel.left() - 0.5 && rect.right() <= panel.right() + 0.5,
                            "{} at {width}: {text:?} {rect:?} leaves the panel",
                            info.code
                        );
                    }
                }
            }
        }
    }
}

/// What screen readers are told: the tabs, each rule with its severity, Show, the steps, the
/// chart, and the counters, a button that opens the panel.
#[test]
fn screen_readers_hear_the_embroidery_interface() {
    crate::i18n::set_current(Lang::EN);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    theme::install_fonts(&ctx);
    let mut app = app_with_findings();
    let (_, nodes) = check_tab(&mut app, &ctx, 300.0);
    let named =
        |start: &str| nodes.iter().find(|n| n.label().is_some_and(|l| l.starts_with(start))).unwrap_or_else(|| panic!("nothing is named {start:?}"));
    assert_eq!(named("Check").role(), Role::Tab);
    assert_eq!(named("Sewing Order").role(), Role::Tab);
    let rule = named("Warning: 3 or more layers of stitches on one spot. ");
    assert!(rule.label().unwrap().contains("spots · 1 object"));
    assert!(named("Note: Stitches shorter than the needle is thick").label().is_some());
    assert!(named("Passed: Stitches longer than 12.7 mm").label().unwrap().contains("Longest: "));
    assert_eq!(named("Show: 3 or more layers").role(), Role::Button);
    assert_eq!(named("Stitch lengths. Median: ").role(), Role::Unknown);
    let counters = named("Embroidery: ");
    assert_eq!(counters.role(), Role::Button);
    assert!(counters.label().unwrap().ends_with("Click to open the Embroidery panel."));
    let (_, nodes) = frames(&mut app, &ctx, Tab::Order, 300.0);
    // The Sewing order's rows are a tree: a block's header with its thread, a unit with its object.
    let item = |start: &str| nodes.iter().any(|n| n.role() == Role::TreeItem && n.label().is_some_and(|l| l.starts_with(start)));
    assert!(item("1 ") && item("<Rectangle> fill · tatami") && item("Thread change"));
}
