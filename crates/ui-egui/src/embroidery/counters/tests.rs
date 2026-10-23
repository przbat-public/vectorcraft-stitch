//! The status bar's counters: when they show and what they say, their wordings in English and
//! Polish, the cadence of new figures, the room they take beside the bar's other items (a sweep of
//! widths, scales, themes, languages, the pointer and a message), the click, and `ui.inspect`.

use egui::{Shape, vec2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::Services;
use crate::embroidery::stitch_preview;
use crate::theme::{self, Brightness};

/// Three overlapping squares in three colours.
fn app_with_art() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    for (x, color) in [(20, "#cc2222"), (40, "#2233cc"), (60, "#22aa33")] {
        app.run("select.set", json!({"ids": []})).unwrap();
        app.run("paint.setFill", json!({ "color": color })).unwrap();
        app.run("shape.rectangle", json!({"x": x, "y": 20, "width": 60, "height": 40})).unwrap();
    }
    app.run("select.set", json!({"ids": []})).unwrap();
    app
}

/// A frame of `width` points at `scale`: the status bar, then (for the plan to be wanted) the
/// Embroidery panel when `panel`, then the preview's scheduling → the status bar's texts, with
/// their boxes.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, time: f64, width: f32, scale: f32, panel: bool) -> Vec<(String, egui::Rect)> {
    let raw = egui::RawInput {
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 640.0))),
        viewports: std::iter::once((egui::ViewportId::ROOT, egui::ViewportInfo { native_pixels_per_point: Some(scale), ..Default::default() }))
            .collect(),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        crate::chrome::status_bar(app, ui);
        if panel {
            crate::panels::embroidery::show(app, ui);
        }
        stitch_preview::schedule(app, ui.ctx());
    });
    out.textures_delta.clear();
    let bar_top = 640.0 - 24.0;
    let mut texts = vec![];
    let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            Shape::Text(t) if t.visual_bounding_rect().top() >= bar_top - 1.0 => texts.push((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => {}
        }
    }
    texts
}

fn ready(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Vec<(String, egui::Rect)> {
    for t in [0.0, 0.05, 0.1] {
        frame(app, ctx, t, width, 1.0, true);
    }
    frame(app, ctx, 0.2, width, 1.0, true)
}

fn said(texts: &[(String, egui::Rect)], what: &str) -> bool {
    texts.iter().any(|(t, _)| t.contains(what))
}

/// Hidden while nothing wants the stitches; the figures once the panel shows them, with the
/// bar's timings when there is room; the figures read from the plan's stats.
#[test]
fn the_figures_show_while_the_stitches_are_wanted() {
    let mut app = app_with_art();
    let ctx = egui::Context::default();
    let texts = frame(&mut app, &ctx, 0.0, 1600.0, 1.0, false);
    assert!(!said(&texts, "stitches"), "{texts:?}");
    assert_eq!(app.embroidery.counters.inspect()["state"], json!("hidden"));
    let texts = ready(&mut app, &ctx, 1600.0);
    let stats = app.session.execute("embroidery.stats", &json!({})).unwrap();
    let f = Figures::of(&stats).unwrap();
    assert_eq!((f.colors, f.thread_changes), (3, 2));
    let full = words(&f, Variant::Named);
    assert!(said(&texts, &full), "{full}: {texts:?}");
    assert!(full.starts_with("Embroidery: ") && full.contains(" · 3 colours · 2 thread changes · about "), "{full}");
    assert!(said(&texts, " fps"), "the timings kept: {texts:?}");
    let v = app.embroidery.counters.inspect();
    assert_eq!((&v["state"], &v["stitches"], &v["colors"], &v["threadChanges"]), (&json!("figures"), &json!(f.stitches), &json!(3), &json!(2)));
    assert_eq!(v["shown"], json!(full));
    // The panel closed: hidden again, and the bar alone asks for nothing: no plan, no reply.
    let sent = app.embroidery.preview.log.sent.len();
    let uid = app.session.active().unwrap().uid;
    frame(&mut app, &ctx, 1.0, 1600.0, 1.0, false);
    // An edit too: the bar plans nothing for it.
    app.run("select.all", json!({})).unwrap();
    app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
    let texts = frame(&mut app, &ctx, 1.1, 1600.0, 1.0, false);
    frame(&mut app, &ctx, 1.5, 1600.0, 1.0, false);
    assert!(!said(&texts, "stitches"), "{texts:?}");
    assert!(!app.embroidery.preview.plan_wanted());
    assert_eq!(app.embroidery.preview.log.sent.len(), sent, "nothing sent for the bar alone");
    assert_eq!(app.embroidery.preview.freshness(uid, 1.5), crate::embroidery::stitch_preview::Freshness::Off);
}

/// The wordings, longest first, and in Polish with the language's numbers.
#[test]
fn the_wordings() {
    let f = Figures { stitches: 4752, colors: 4, thread_changes: 3, trims: 2, jumps: 5, minutes: 6.2, speed: 650, size: Some((100.4, 70.3)) };
    assert_eq!(words(&f, Variant::All), "4,752 stitches · 4 colours · 3 thread changes · about 7 min");
    assert_eq!(words(&f, Variant::NoChanges), "4,752 stitches · 4 colours · about 7 min");
    assert_eq!(words(&f, Variant::Time), "4,752 stitches · about 7 min");
    assert_eq!(words(&f, Variant::Stitches), "4,752 stitches");
    crate::i18n::set_current(crate::i18n::Lang::from_code("pl").unwrap());
    let pl = words(&f, Variant::All);
    let named = words(&f, Variant::Named);
    crate::i18n::set_current(crate::i18n::Lang::EN);
    assert!(pl.starts_with("4752 ściegi · 4 kolory · 3 zmiany nici"), "{pl}");
    assert!(named.starts_with("Haft: "), "{named}");
}

/// New figures show at most every quarter second: the ones shown stay until then.
#[test]
fn new_figures_show_at_most_every_quarter_second() {
    let mut app = app_with_art();
    let ctx = egui::Context::default();
    ready(&mut app, &ctx, 1600.0);
    let stitches = |app: &VectorcraftApp| app.embroidery.counters.inspect()["stitches"].clone();
    let square = |app: &mut VectorcraftApp, x: u32| app.run("shape.rectangle", json!({"x": x, "y": 200, "width": 40, "height": 40})).unwrap();
    // A change long after the last figures: shown as soon as planned (in line, in the tests).
    square(&mut app, 20);
    for t in [2.0, 2.02] {
        frame(&mut app, &ctx, t, 1600.0, 1.0, true);
    }
    let first = stitches(&app);
    // Another right after: held back for a quarter second from the last shown, then shown.
    square(&mut app, 120);
    for t in [2.05, 2.1] {
        frame(&mut app, &ctx, t, 1600.0, 1.0, true);
    }
    assert_eq!(stitches(&app), first, "held back");
    for t in [2.3, 2.35] {
        frame(&mut app, &ctx, t, 1600.0, 1.0, true);
    }
    assert_ne!(stitches(&app), first, "then shown");
}

/// A click opens the Embroidery panel at its Sewing order; the tooltip tells the figures.
#[test]
fn a_click_opens_the_panel_and_the_tooltip_tells_the_figures() {
    let f = Figures { stitches: 1, colors: 1, thread_changes: 0, trims: 0, jumps: 0, minutes: 0.2, speed: 650, size: Some((10.0, 5.0)) };
    let tip = tooltip(&State::Figures(f.clone()));
    assert!(tip.contains("1 stitch · 0 jumps · 0 trims") && tip.contains("10 × 5 mm") && tip.contains("650 stitches a minute"), "{tip}");
    assert!(tooltip(&State::Stale(f)).contains("before the last change"));
    assert!(tooltip(&State::Failed("too many stitches".into())).starts_with("too many stitches"));
    let mut app = app_with_art();
    app.ui.embroidery_panel.tab = crate::panels::embroidery::Tab::Check;
    app.run("ui.embroideryPanel", json!({"tab": "order"})).unwrap();
    assert_eq!(app.ui.embroidery_panel.tab, crate::panels::embroidery::Tab::Order);
}

/// The states other than figures: nothing to sew, and a design that can't be planned (said by
/// the bar, so the canvas's notice leaves it).
#[test]
fn nothing_to_sew_and_cant_be_planned() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
    let ctx = egui::Context::default();
    let texts = ready(&mut app, &ctx, 1600.0);
    assert!(said(&texts, "Nothing to sew"), "{texts:?}");
    assert_eq!(app.embroidery.counters.inspect()["state"], json!("nothing"));
    let t = Tokens::for_brightness(Brightness::Dark);
    let failed = wordings(&State::Failed("x".into()), &t);
    assert_eq!(failed.len(), 1);
    assert!(failed[0].1.starts_with("⚠ "));
    let stale = wordings(&State::Stale(Figures::default()), &t);
    assert!(stale.iter().all(|(_, w, c)| w.starts_with("Updating… ") && *c == t.text_dim));
}

/// A design that takes long to plan: nothing in the bar at first (waiting), then "Planning
/// stitches…" with how far the plan got, never 100 % before it is in; then the figures.
#[test]
fn planning_says_how_far_it_got_never_all_of_it() {
    use crate::embroidery::stitch_preview::{Held, STALE_NOTICE_SECS, StitchPreview};
    let mut app = app_with_art();
    let held = Held::default();
    app.embroidery.preview = StitchPreview::with_runner(Box::new(held.clone()));
    let ctx = egui::Context::default();
    // The bar is drawn before the panel: it shows from the frame after the panel asked.
    frame(&mut app, &ctx, 0.0, 1600.0, 1.0, true);
    let texts = frame(&mut app, &ctx, 0.05, 1600.0, 1.0, true);
    assert_eq!(app.embroidery.counters.inspect()["state"], json!("waiting"));
    assert!(!said(&texts, "Planning") && !said(&texts, "stitches"), "{texts:?}");
    let later = STALE_NOTICE_SECS + 0.1;
    held.progress(2, 5);
    let texts = frame(&mut app, &ctx, later, 1600.0, 1.0, true);
    assert_eq!(app.embroidery.counters.inspect()["state"], json!("planning"));
    assert!(said(&texts, "Planning stitches… 40%"), "{texts:?}");
    for (done, all) in [(999, 1000), (1000, 1000)] {
        held.progress(done, all);
        let texts = frame(&mut app, &ctx, later + 0.1, 1600.0, 1.0, true);
        assert!(said(&texts, "Planning stitches… 99%"), "{done} of {all}: {texts:?}");
    }
    held.release();
    for t in [0.2, 0.3] {
        frame(&mut app, &ctx, later + t, 1600.0, 1.0, true);
    }
    assert_eq!(app.embroidery.counters.inspect()["state"], json!("figures"));
}

/// Across window widths, scales, themes, languages, with and without the pointer over the canvas
/// and a long message: the bar's texts never overlap and stay in the window; the counters show
/// whenever their shortest wording fits beside what the bar keeps, the full one in a wide window.
#[test]
fn the_counters_fit_the_bar() {
    let pl = crate::i18n::Lang::from_code("pl").unwrap();
    for lang in [crate::i18n::Lang::EN, pl] {
        for brightness in [Brightness::Dark, Brightness::Light] {
            for scale in [1.0, 2.0] {
                for (pointer, message) in [(false, false), (true, false), (true, true)] {
                    let ctx = egui::Context::default();
                    theme::install_fonts(&ctx);
                    theme::apply(&ctx, brightness);
                    crate::i18n::set_current(lang);
                    let mut app = app_with_art();
                    if message {
                        app.ui.status = format!("Saved /Users/example/Documents/{}/design.svg", "long-folder/".repeat(8));
                    }
                    for (k, width) in [640.0, 800.0, 1024.0, 1280.0, 1600.0].into_iter().enumerate() {
                        app.hover_doc = pointer.then(|| vectorcraft_geom::Point::new(123.45, 678.9));
                        let start = k as f64;
                        for t in [start, start + 0.05, start + 0.1] {
                            frame(&mut app, &ctx, t, width, scale, true);
                        }
                        let texts = frame(&mut app, &ctx, start + 0.2, width, scale, true);
                        let case = format!("{lang:?} {brightness:?} {scale} pointer {pointer} message {message} {width}");
                        let shown = app.embroidery.counters.inspect()["shown"].as_str().unwrap_or_default().to_string();
                        // The counters overlap nothing and stay in the window. (Upstream's X/Y
                        // readout can run into the tool's name in a narrow window, counters or not.)
                        if let Some((_, own)) = texts.iter().find(|(t, _)| *t == shown) {
                            assert!(own.left() >= -0.5 && own.right() <= width + 0.5, "{case}: {own:?}");
                            for (b, rb) in texts.iter().filter(|(t, _)| *t != shown) {
                                assert!(!own.intersects(*rb), "{case}: the counters overlap {b:?}");
                            }
                        }
                        if width >= 1600.0 && !message {
                            assert!(shown.contains(" · "), "{case}: {shown:?}");
                        }
                        if !shown.is_empty() {
                            assert!(texts.iter().any(|(t, _)| *t == shown), "{case}: {shown:?} drawn");
                        }
                        // Shown whenever the shortest wording fits the room the bar leaves.
                        if let Some((room, shortest)) = app.embroidery.counters.fit {
                            assert_eq!(!shown.is_empty(), shortest <= room, "{case}: {shortest} in {room}");
                        }
                    }
                    crate::i18n::set_current(crate::i18n::Lang::EN);
                }
            }
        }
    }
}
