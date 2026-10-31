//! The embroidery interface drawn in every language: the Embroidery panel's tabs (the Object tab
//! with a value set on an object and with objects that differ, a check's rule opened and shown, its
//! chart), its states (planning, failed over the last stitches, checking, a check that failed) and
//! the status bar's counters draw no English the language's catalogue doesn't give, and the panel's
//! words stay inside it at 256 and 300 points; the Stitches tab likewise in its states, at 220 too;
//! Document Setup's Embroidery tab likewise, in its two layouts; and what screen readers are told.

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
    let mut last = (vec![], vec![]);
    for time in [0.0, 0.05, 0.1, 0.2, 0.3] {
        last = frame_at(app, ctx, time, width, &mut |app, ui| crate::panels::embroidery::show(app, ui));
    }
    last
}

/// One frame at `time`: the status bar, `panel` drawn `width` points wide on the window's right,
/// and the preview's scheduling → the texts with their boxes, and the screen-reader tree.
fn frame_at(
    app: &mut VectorcraftApp,
    ctx: &egui::Context,
    time: f64,
    width: f32,
    panel: &mut dyn FnMut(&mut VectorcraftApp, &mut egui::Ui),
) -> (Vec<(String, Rect)>, Vec<Node>) {
    let raw = egui::RawInput { time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW, HEIGHT))), ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| {
        crate::chrome::status_bar(app, ui);
        egui::Panel::right("embroidery-test").exact_size(width).resizable(false).show(ui, |ui| panel(app, ui));
        crate::embroidery::stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let mut texts = vec![];
    // A text its clip hides whole isn't drawn (a list's row past its end, laid out for scrolling);
    // one it cuts keeps its whole box, so words leaving the panel still show.
    let mut pending: Vec<(Rect, Shape)> = out.shapes.into_iter().map(|c| (c.clip_rect, c.shape)).collect();
    while let Some((clip, s)) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v.into_iter().map(|s| (clip, s))),
            Shape::Text(t) if clip.intersects(t.visual_bounding_rect()) => texts.push((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => {}
        }
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
    (texts, nodes)
}

/// The states the panel and the status bar pass through, at `width`: a design that takes long to
/// plan (the panel's "Planning stitches…" with its progress, and the bar's), an edit of it
/// planned (its last stitches, "Updating…"), one that can't be planned over its last stitches, and
/// the Check tab with its check asked and not answered, then
/// failed (with the plan's reason) → each one's texts, with the counters' words then.
fn states(ctx: &egui::Context, width: f32) -> Vec<(Vec<(String, Rect)>, String)> {
    use crate::embroidery::stitch_preview::{Freshness, Held, STALE_NOTICE_SECS, StitchPreview};
    let mut app = app_with_findings();
    let held = Held::default();
    app.embroidery.preview = StitchPreview::with_runner(Box::new(held.clone()));
    // Each edit's plan sent in the edit's first frame, before the release that runs it.
    app.embroidery.preview.pin_light();
    app.ui.embroidery_panel.tab = Tab::Order;
    let panel = &mut |app: &mut VectorcraftApp, ui: &mut egui::Ui| crate::panels::embroidery::show(app, ui);
    let counters = |app: &VectorcraftApp| app.embroidery.counters.inspect()["shown"].as_str().unwrap_or_default().to_string();
    let mut out = vec![];
    // Each state from a second of its own, its frames at `later` (the notices show) and after.
    let later = STALE_NOTICE_SECS + 0.1;
    frame_at(&mut app, ctx, 0.0, width, panel);
    held.progress(2, 5);
    frame_at(&mut app, ctx, later, width, panel);
    let (texts, _) = frame_at(&mut app, ctx, later + 0.05, width, panel);
    assert!(texts.iter().any(|(t, _)| *t == counters(&app)) && !counters(&app).is_empty(), "the bar says it plans: {texts:?}");
    out.push((texts, counters(&app)));
    held.release();
    frame_at(&mut app, ctx, later + 0.1, width, panel);
    // An edit whose plan takes long: the last stitches, "Updating…" beside their figures.
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    frame_at(&mut app, ctx, 1.0, width, panel);
    let (texts, _) = frame_at(&mut app, ctx, 1.0 + later, width, panel);
    assert!(texts.iter().any(|(t, _)| t == crate::et!("Updating…")), "{texts:?}");
    out.push((texts, counters(&app)));
    held.release();
    frame_at(&mut app, ctx, 1.1 + later, width, panel);
    // Three metres a side: it can't be planned.
    app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 8500, "height": 8500})).unwrap();
    frame_at(&mut app, ctx, 2.0, width, panel);
    held.release();
    frame_at(&mut app, ctx, 2.1, width, panel);
    let (texts, _) = frame_at(&mut app, ctx, 2.2, width, panel);
    out.push((texts, counters(&app)));
    let uid = app.session.active().unwrap().uid;
    let Freshness::Failed { error } = app.embroidery.preview.freshness(uid, 2.2) else { panic!("the big square can't be planned") };
    let failed: Result<std::sync::Arc<serde_json::Value>, String> = Err(error);
    let busy = crate::panels::embroidery::SHOW_BUSY_AFTER_SECS;
    for (time, reply) in [(3.0, None), (3.0 + busy, None), (4.0, Some((&failed, true)))] {
        let (texts, _) =
            frame_at(&mut app, ctx, time, width, &mut |app, ui| check::show(app, ui, check::Shown { uid, reply, stats: None, stale: false }));
        out.push((texts, String::new()));
    }
    out
}

/// The Object tab: the first square, with an angle and a pull of its own, then all three squares,
/// one of them setting its pull (their values differ) → each one's texts.
fn object_tab(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Vec<Vec<(String, Rect)>> {
    let order = app.run("embroidery.sequence", json!({})).unwrap();
    let ids: Vec<u64> =
        order["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).filter_map(|u| u["id"].as_u64()).collect();
    let first = ids.first().copied().unwrap();
    app.run("embroidery.set", json!({"ids": [first], "part": "fill", "angle": 30, "pull": 0.35})).unwrap();
    app.run("select.set", json!({ "ids": [first] })).unwrap();
    let one = frames(app, ctx, Tab::Object, width).0;
    app.run("select.set", json!({ "ids": ids })).unwrap();
    let several = frames(app, ctx, Tab::Object, width).0;
    // Manual stitches: one converted object alone, and beside the others.
    let last = ids.last().copied().unwrap();
    app.run("embroidery.convertToManual", json!({"ids": [last]})).unwrap();
    app.run("select.set", json!({ "ids": [last] })).unwrap();
    let manual = frames(app, ctx, Tab::Object, width).0;
    app.run("select.set", json!({ "ids": ids })).unwrap();
    let mixed = frames(app, ctx, Tab::Object, width).0;
    app.run("edit.undo", json!({})).unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    vec![one, several, manual, mixed]
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

/// A drawn text that may read as its English: a name (a format's, a thread's, a source's) with no
/// other English word round it, a number with its unit, or what the language's catalogue
/// `catalogue` gives for the embroidery interface (one of its own rows, its plurals or messages,
/// translated to the same words, as German's "Design"), never a template whose fixed words have no
/// letters (`{path}: {e}` would pass anything with a colon).
fn may_be_english(catalogue: &str, text: &str, names: &[String]) -> bool {
    let ascii_letters = |t: &str| t.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let units = text.split_whitespace().all(|w| ["fps", "ms"].contains(&w) || !w.chars().any(|c| c.is_ascii_alphabetic()));
    if text.contains(" · ") {
        return text.split(" · ").all(|part| may_be_english(catalogue, part, names));
    }
    // A word followed by its count (the Stitches tab's filters, "Stops 3"): the word.
    if let Some((word, count)) = text.rsplit_once(' ')
        && !count.is_empty()
        && count.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '\u{a0}' | '\u{202f}'))
        && may_be_english(catalogue, word, names)
    {
        return true;
    }
    let text = text.trim_start_matches("· ");
    let without_names = names.iter().fold(text.to_string(), |t, n| t.replace(n.as_str(), ""));
    let worded = |t: &str| {
        let mut fixed = String::new();
        let mut rest = t;
        while let Some(open) = rest.find('{') {
            fixed.push_str(&rest[..open]);
            rest = rest[open..].find('}').map_or("", |close| &rest[open + close + 1..]);
        }
        fixed.push_str(rest);
        fixed.chars().filter(|c| c.is_alphabetic()).count() >= 3
    };
    let ours = |l: &&str| ["embroidery\t", "@plural\t", "@msg\t"].iter().any(|c| l.starts_with(c));
    let translated = || {
        catalogue.lines().filter(ours).filter_map(|l| l.rsplit('\t').next()).flat_map(|t| t.split('|')).filter(|t| worded(t)).any(|t| fills(t, text))
    };
    ascii_letters(text) < 3 || units || ascii_letters(&without_names) < 3 || translated()
}

/// `text` with its numbers (digits and the marks between them) blanked: an English text and its
/// translation drawn with the language's own decimal mark compare equal when it isn't translated.
fn numbers_blanked(text: &str) -> String {
    let mut out = String::new();
    let mut in_number = false;
    for c in text.chars() {
        let part = c.is_ascii_digit() || (in_number && matches!(c, '.' | ',' | '\u{a0}' | '\u{202f}'));
        if part && !in_number {
            out.push('#');
        }
        if !part {
            out.push(c);
        }
        in_number = part;
    }
    out
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
        // The palettes' ids, as a block's header writes them ("JEF 45").
        names.extend(shown.planned.plan.blocks.iter().map(|b| b.thread.palette().id().to_uppercase()));
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
        texts.extend(object_tab(&mut app, &ctx, 300.0).into_iter().flatten());
        texts.extend(states(&ctx, 300.0).into_iter().flat_map(|(t, _)| t));
        texts.into_iter().map(|(t, _)| numbers_blanked(&t)).collect()
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
            let object = object_tab(&mut app, &ctx, width);
            let names = names(&app, lang);
            let counters = app.embroidery.counters.inspect()["shown"].as_str().unwrap_or_default().to_string();
            let mut drawn = vec![(order, counters.clone()), (checked, counters.clone())];
            drawn.extend(object.into_iter().map(|texts| (texts, counters.clone())));
            drawn.extend(states(&ctx, width));
            crate::i18n::set_current(Lang::EN);
            let panel = Rect::from_min_max(Pos2::new(WINDOW - width, 0.0), Pos2::new(WINDOW, HEIGHT - 24.0));
            for (texts, counters) in drawn {
                // The panel's words overlap nothing (the icons, unloaded in the tests, draw a
                // placeholder).
                let panel_texts: Vec<&(String, Rect)> = texts.iter().filter(|(t, r)| panel.intersects(*r) && t != "⚠").collect();
                for (k, (a, ra)) in panel_texts.iter().enumerate() {
                    for (b, rb) in &panel_texts[k + 1..] {
                        // A field's value in its look's colour, painted over the app's own words
                        // in their place (`fields::number_field`), is one text.
                        if a == b && ra == rb {
                            continue;
                        }
                        assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{} at {width}: {a:?} overlaps {b:?}", info.code);
                    }
                }
                // The panel's words and the counters (the rest of the status bar is upstream's).
                for (text, rect) in texts.iter().filter(|(t, r)| panel.intersects(*r) || (*t == counters && !t.is_empty())) {
                    if english.contains(&numbers_blanked(text)) {
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

/// [`frames`] at the Stitches tab, then frames until two in a row draw the same (the rows listed
/// and the foot's room, laid out from the frame before, settled; at most [`SETTLE_FRAMES`] more).
fn stitches_frames(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Vec<(String, Rect)> {
    // A frame's words and their boxes to a tenth of a point (an empty text's box isn't a number).
    let seen = |texts: &[(String, Rect)]| -> Vec<(String, Option<[i64; 4]>)> {
        let tenth = |v: f32| v.is_finite().then(|| (v * 10.0).round() as i64);
        texts.iter().map(|(t, r)| (t.clone(), (|| Some([tenth(r.min.x)?, tenth(r.min.y)?, tenth(r.max.x)?, tenth(r.max.y)?]))())).collect()
    };
    let mut last = frames(app, ctx, Tab::Stitches, width).0;
    for k in 0..SETTLE_FRAMES {
        let now = frame_at(app, ctx, 1.0 + k as f64 * 0.05, width, &mut |app, ui| crate::panels::embroidery::show(app, ui)).0;
        let settled = seen(&now) == seen(&last);
        last = now;
        if settled {
            break;
        }
    }
    last
}

/// The most frames [`stitches_frames`] waits for the Stitches tab to settle.
const SETTLE_FRAMES: usize = 20;

/// The Stitches tab at `width`: nothing selected, the whole design listed, a planned square
/// selected (its foot: planned, Convert to Manual Stitches), then that square converted and its
/// stitches edited with a needle point selected (its foot: the count, Move by, the actions) → each
/// state's texts.
fn stitches_tab(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Vec<Vec<(String, Rect)>> {
    use crate::panels::embroidery::stitches::Scope;
    let panel = &mut app.ui.embroidery_panel;
    (panel.stitches_scope, panel.stitches_filter) = (Scope::Selection, vectorcraft_engine::cmd::embroidery::stitch_list::Filter::All);
    app.run("select.set", json!({"ids": []})).unwrap();
    let nothing = stitches_frames(app, ctx, width);
    app.ui.embroidery_panel.stitches_scope = Scope::Design;
    let design = stitches_frames(app, ctx, width);
    app.ui.embroidery_panel.stitches_scope = Scope::Selection;
    let order = app.run("embroidery.sequence", json!({})).unwrap();
    let first = order["blocks"][0]["units"][0]["id"].as_u64().unwrap();
    app.run("select.set", json!({ "ids": [first] })).unwrap();
    let planned = stitches_frames(app, ctx, width);
    app.run("embroidery.convertToManual", json!({})).unwrap();
    app.run("object.isolate", json!({ "id": first })).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    let run = app.session.active().unwrap().doc.node(vectorcraft_doc::NodeId(first)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 1}]})).unwrap();
    let editing = stitches_frames(app, ctx, width);
    assert!(crate::embroidery::editing::editing(app).is_some_and(|e| e.selected == 1), "editing, one needle point selected");
    vec![nothing, design, planned, editing]
}

/// In every complete language, the Stitches tab, in each of its states, draws no English its
/// catalogue doesn't give, no words over others, and nothing outside the panel, at the shared
/// dock's narrowest (220 points) and at 256 and 300.
#[test]
fn the_stitches_tab_in_every_language() {
    const WIDTHS: [f32; 3] = [220.0, 256.0, 300.0];
    let english: Vec<String> = {
        crate::i18n::set_current(Lang::EN);
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut app = app_with_findings();
        stitches_tab(&mut app, &ctx, 300.0).into_iter().flatten().map(|(t, _)| numbers_blanked(&t)).collect()
    };
    for info in LANGUAGES.iter().filter(|l| l.complete_menus) {
        let lang = Lang::from_code(info.code).unwrap();
        for width in WIDTHS {
            crate::i18n::set_current(lang);
            let ctx = egui::Context::default();
            theme::install_fonts(&ctx);
            let mut app = app_with_findings();
            let drawn = stitches_tab(&mut app, &ctx, width);
            let names = names(&app, lang);
            crate::i18n::set_current(Lang::EN);
            let panel = Rect::from_min_max(Pos2::new(WINDOW - width, 0.0), Pos2::new(WINDOW, HEIGHT - 24.0));
            for (state, texts) in drawn.iter().enumerate() {
                let inside: Vec<&(String, Rect)> = texts.iter().filter(|(_, r)| panel.intersects(*r)).collect();
                for (k, (a, ra)) in inside.iter().enumerate() {
                    for (b, rb) in &inside[k + 1..] {
                        // A field's value painted over the app's own words in their place: one text.
                        if a == b && ra == rb {
                            continue;
                        }
                        assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{} at {width}, state {state}: {a:?} overlaps {b:?}", info.code);
                    }
                }
                for (text, rect) in inside {
                    if english.contains(&numbers_blanked(text)) {
                        assert!(may_be_english(info.source, text, &names), "{}, state {state}: {text:?} is drawn in English", info.code);
                    }
                    assert!(
                        rect.left() >= panel.left() - 0.5 && rect.right() <= panel.right() + 0.5,
                        "{} at {width}, state {state}: {text:?} {rect:?} leaves the panel",
                        info.code
                    );
                }
            }
        }
    }
}

/// Document Setup's Embroidery tab, its fabric changed with objects of their own values (the
/// bar amber), drawn in a window `window` points wide → its texts with their boxes, and the left
/// of the tab and the width it takes (two columns in a wide window, one in a narrow one).
fn setup_tab(lang: Lang, window: f32) -> (Vec<(String, Rect)>, f32, f32) {
    crate::i18n::set_current(lang);
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut app = app_with_findings();
    app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
    app.run("select.all", json!({})).unwrap();
    app.run("embroidery.set", json!({"part": "fill", "pull": 0.3})).unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    app.run("file.documentSetup", json!({})).unwrap();
    crate::embroidery::setup_tab::choose_value(&mut app, "fabric", json!("stretchKnit"));
    crate::embroidery::setup_tab::choose_value(&mut app, "tatami.length", json!(3.5));
    let (mut texts, mut left) = (vec![], 0.0);
    for _ in 0..3 {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(window, HEIGHT))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                left = ui.max_rect().left();
                let Some(mut d) = app.ui.dialog.clone() else { return };
                crate::embroidery::setup_tab::body(&mut app, ui, &mut d);
                app.ui.dialog = Some(d);
            });
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
    }
    crate::i18n::set_current(Lang::EN);
    let width = crate::embroidery::setup_tab::width_for(window);
    (texts, left, width)
}

/// In every complete language, Document Setup's Embroidery tab, in two columns and in one, draws
/// no English its catalogue doesn't give, no words over others, and nothing wider than the tab.
#[test]
fn document_setups_embroidery_tab_in_every_language() {
    /// A wide window (two columns) and a narrow one (one), points.
    const WINDOWS: [f32; 2] = [1440.0, 800.0];
    // The makers the presets' values come from, and a unit, are the same in every language.
    let names: Vec<String> = ["Wilcom", "Melco", "Sulky", "Gunold", "/min"].map(str::to_string).to_vec();
    let english: Vec<String> = WINDOWS.iter().flat_map(|w| setup_tab(Lang::EN, *w).0).map(|(t, _)| numbers_blanked(&t)).collect();
    for info in LANGUAGES.iter().filter(|l| l.complete_menus) {
        let lang = Lang::from_code(info.code).unwrap();
        for window in WINDOWS {
            let (texts, left, width) = setup_tab(lang, window);
            for (k, (a, ra)) in texts.iter().enumerate() {
                for (b, rb) in &texts[k + 1..] {
                    // A field's value painted over the app's own words in their place: one text.
                    if a == b && ra == rb {
                        continue;
                    }
                    assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{} at {window}: {a:?} overlaps {b:?}", info.code);
                }
            }
            for (text, rect) in &texts {
                if english.contains(&numbers_blanked(text)) {
                    assert!(may_be_english(info.source, text, &names), "{}: {text:?} is drawn in English", info.code);
                }
                assert!(rect.right() <= left + width + 0.5, "{} at {window}: {text:?} {rect:?} leaves the tab ({width} wide)", info.code);
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
    // The chart is a figure, its bins in words (a pointer reads one at a time).
    let chart = named("Stitch lengths. Median: ");
    assert_eq!(chart.role(), Role::Figure);
    assert!(chart.description().is_some_and(|d| d.contains(" mm: ") && d.contains("stitches")), "{:?}", chart.description());
    let counters = named("Embroidery: ");
    assert_eq!(counters.role(), Role::Button);
    assert!(counters.label().unwrap().ends_with("Click to open the Embroidery panel."));
    let (_, nodes) = frames(&mut app, &ctx, Tab::Order, 300.0);
    // The Sewing order's rows are a tree: a block's header with its thread, a unit with its object.
    let item = |start: &str| nodes.iter().find(|n| n.role() == Role::TreeItem && n.label().is_some_and(|l| l.starts_with(start)));
    let (header, unit, change) = (item("1 ").unwrap(), item("<Rectangle> fill · tatami").unwrap(), item("Thread change").unwrap());
    // In a tree: a block's header unfolded at the first level, its units at the second.
    assert_eq!((header.level(), header.is_expanded()), (Some(1), Some(true)));
    assert_eq!((unit.level(), unit.is_expanded()), (Some(2), None));
    assert_eq!(change.level(), Some(1));
    let tree = nodes.iter().find(|n| n.role() == Role::Tree).expect("the list is a tree");
    assert_eq!(tree.label(), Some("Sewing Order"));
    let items = nodes.iter().filter(|n| n.role() == Role::TreeItem).count();
    assert!(tree.children().len() >= items, "{} children for {items} rows", tree.children().len());
}
