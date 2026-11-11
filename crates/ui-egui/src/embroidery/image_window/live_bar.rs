//! The image window's Live bar: how its result stands, the figures of what Create would make, its
//! notes, and Start Over, Cancel and Create Embroidery Objects.
//!
//! - **Figures** in the status bar counters' words ("8,105 stitches · 4 threads · 3 thread changes
//!   · about 12 min · 98 × 88.2 mm"): the image's objects sewn alone, with the document's fabric,
//!   thread and machine (the project owner's choice, 2026-11-11), as its tooltip and its description
//!   say. Under them, how long the last change took to show and which steps it ran.
//! - **States:** computing, said once a run has taken [`super::schedule::STALE_NOTICE_SECS`], with
//!   its progress; the figures; stale (the last figures dimmed while a newer run is on its way);
//!   nothing to sew; failed with the reason, the last figures kept.
//! - **Notes:** each of digitizing's warnings in words, and what the Check tab finds on the objects
//!   (errors and warnings), problems first: a chip with the first and "+2", a list of them all.
//! - **Screen readers** hear the bar as a polite status, said again only once a run settles.
//! - **Create** is disabled with its reason when nothing would be made, or the settings can't be
//!   digitized.

use egui::{Align, Color32, FontId, Layout, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::Value;
use vectorcraft_engine::cmd::embroidery::DigitizeStage;

use super::schedule::Freshness;
use super::{Live, frame::Asked, words};
use crate::VectorcraftApp;
use crate::embroidery::counters::{self, Figures};
use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::i18n::Lang;
use crate::panels::embroidery::check;
use crate::theme::Tokens;
use crate::widgets;

/// The size of the bar's figures and of its other words (points).
const FIGURES_TEXT: f32 = 12.5;
const SMALL_TEXT: f32 = 11.0;
/// A note's mark, the chip's padding, and the list's width (points): estimates from the mockups'
/// bar.
const MARK: f32 = 12.0;
const CHIP_PAD: f32 = 6.0;
const NOTES_WIDTH: f32 = 420.0;
/// The most room the chip's first note takes (points), cut short with "…" past it.
const CHIP_WORDS: f32 = 300.0;
/// How strongly stale figures are dimmed (the share of their colour kept): an estimate.
const STALE_ALPHA: f32 = 0.55;

/// A note: its mark (an error, a warning, or a note), its words, and the step it is about (its
/// card shows it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub(crate) mark: Mark,
    pub(crate) words: String,
    pub(crate) step: u8,
}

/// The step digitizing's warning `id` is about: the image's own (1), its colours (2), its shapes
/// (3); what Create would make (4).
fn step_of(id: &str) -> u8 {
    match id {
        "coarse" | "decodedSmaller" => 1,
        "notLines" | "shapesNotJoined" => 3,
        "nothingToMake" => 4,
        _ => 2,
    }
}

/// What the bar says of the result shown, settled: the figures, nothing to sew and why, or why it
/// can't be digitized (what screen readers are told).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Said {
    Figures(Figures),
    Nothing(Nothing),
    Failed(String),
}

/// Why nothing would be sewn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nothing {
    /// Every area is the background, left out: Create would make no object.
    Background,
    /// Every colour is left out: Create would make their objects, none of them sewn.
    LeftOut,
}

impl Nothing {
    /// The bar's words, and the note's: what isn't sewn, and for the background what to do.
    pub(crate) fn words(self) -> &'static str {
        match self {
            Nothing::Background => crate::et!("Nothing to sew: every area is the background. Sew the background, or crop less."),
            Nothing::LeftOut => crate::et!("Nothing to sew: every color is left out."),
        }
    }

    /// Create's refusal, in the status bar and on its disabled button.
    pub(crate) fn refusal(self) -> &'static str {
        match self {
            Nothing::Background => crate::et!("Nothing to create: every area is the background. Sew the background, or crop less."),
            Nothing::LeftOut => crate::et!("Nothing to create: every color is left out."),
        }
    }
}

impl Said {
    pub(crate) fn words(&self, lang: Lang) -> String {
        match self {
            Said::Figures(f) => figures_words(f, lang),
            Said::Nothing(why) => why.words().to_string(),
            Said::Failed(reason) => fill(crate::et!("Can't digitize it: {reason}"), &[("reason", reason)]),
        }
    }
}

/// The figures in words: the counters' ("stitches · threads · thread changes · time") and the
/// design's size.
pub(crate) fn figures_words(f: &Figures, lang: Lang) -> String {
    let stitches = crate::etn!(f.stitches, "{count} stitch", "{count} stitches");
    let threads = crate::etn!(f.colors, "{count} thread", "{count} threads");
    let changes = crate::etn!(f.thread_changes, "{count} thread change", "{count} thread changes");
    let mut parts = vec![stitches, threads, changes, counters::minutes_words(f.minutes)];
    if let Some((w, h)) = f.size {
        parts.push(fill(crate::et!("{width} × {height} mm"), &[("width", &numbers::decimal(lang, w, 1)), ("height", &numbers::decimal(lang, h, 1))]));
    }
    parts.join(" · ")
}

/// What the result shown says, settled, from `live`: the figures once planned; nothing to sew when
/// Create would make nothing (every area the background) or nothing sewn (every colour left out
/// still makes their objects, not sewn); `None` while nothing is shown.
pub(crate) fn said(live: &mut Live, now: f64) -> Option<Said> {
    if let Freshness::Failed(reason) = live.schedule.freshness(now) {
        return Some(Said::Failed(reason));
    }
    let shown = live.schedule.shown.as_ref().filter(|s| s.whole)?;
    if shown.outputs.made().is_none() {
        return Some(Said::Nothing(Nothing::Background));
    }
    let stats = live.reply()?["stats"].clone();
    match Figures::of(&stats)? {
        f if f.stitches == 0 => Some(Said::Nothing(Nothing::LeftOut)),
        f => Some(Said::Figures(f)),
    }
}

/// Why Create can't make anything now (`None`: it can, at once or once the result is ready).
pub(crate) fn create_refused(live: &mut Live, now: f64) -> Option<String> {
    match said(live, now)? {
        Said::Nothing(why) => Some(why.refusal().to_string()),
        Said::Failed(reason) => Some(fill(crate::et!("Can't digitize it: {reason}"), &[("reason", &reason)])),
        Said::Figures(_) => None,
    }
}

/// Digitizing's warning `w` in words; `None` for one this version doesn't know (the engine's prose
/// is never shown).
pub(crate) fn warning_words(w: &Value, lang: Lang) -> Option<Note> {
    let v = &w["values"];
    let count = v["count"].as_u64().unwrap_or(0);
    let color = v["color"].as_str().unwrap_or_default();
    let area = |key: &str| numbers::decimal(lang, v[key].as_f64().unwrap_or(0.0), 2);
    let id = w["id"].as_str()?;
    let note = |mark, words: String| Some(Note { mark, words, step: step_of(id) });
    match id {
        "specksJoined" => note(
            Mark::Info,
            fill(
                &crate::etn!(
                    count,
                    "{count} speck smaller than {value} mm² joined the color around it",
                    "{count} specks smaller than {value} mm² joined the colors around them"
                ),
                &[("value", &area("minArea"))],
            ),
        ),
        "specksLeftOut" => note(
            Mark::Warning,
            fill(
                &crate::etn!(
                    count,
                    "{count} speck smaller than {value} mm² had nothing to join and was left out",
                    "{count} specks smaller than {value} mm² had nothing to join and were left out"
                ),
                &[("value", &area("minArea"))],
            ),
        ),
        "bandsJoined" => note(
            Mark::Info,
            fill(
                &crate::etn!(
                    count,
                    "{count} band at most {value} mm wide joined the neighbor nearest its color",
                    "{count} bands at most {value} mm wide joined the neighbors nearest their colors"
                ),
                &[("value", &area("maxWidth"))],
            ),
        ),
        "colorJoined" => note(
            Mark::Info,
            fill(
                crate::et!("Every area of {color} was smaller than {value} mm²: it joined its neighbors."),
                &[("color", color), ("value", &area("minArea"))],
            ),
        ),
        "choiceNotGray" => {
            let thread = v["thread"]["name"].as_str().unwrap_or_default();
            note(
                Mark::Info,
                fill(
                    crate::et!("The thread chosen for {color}, {thread}, isn't gray: it is sewn as chosen."),
                    &[("color", color), ("thread", thread)],
                ),
            )
        }
        "coarse" => note(
            Mark::Warning,
            fill(
                crate::et!("The image has {value} pixels a millimeter at this size, fewer than {least}: a larger copy of it digitizes better."),
                &[("value", &area("pxPerMm")), ("least", &numbers::decimal(lang, v["fewerThan"].as_f64().unwrap_or(0.0), 0))],
            ),
        ),
        "decodedSmaller" => {
            let px = |k: &str, i: usize| numbers::count(lang, v[k][i].as_u64().unwrap_or(0));
            note(
                Mark::Info,
                fill(
                    crate::et!("The image was read at {width} × {height} pixels, smaller than its {fullWidth} × {fullHeight}."),
                    &[("width", &px("decoded", 0)), ("height", &px("decoded", 1)), ("fullWidth", &px("pixels", 0)), ("fullHeight", &px("pixels", 1))],
                ),
            )
        }
        "cornersDisagree" => note(Mark::Warning, crate::et!("The image's corners don't agree on a background: every color is sewn.").to_string()),
        "noBackgroundColor" => note(Mark::Warning, crate::et!("No color traced is the background's: every area is sewn.").to_string()),
        "notFlatArt" => note(
            Mark::Warning,
            crate::et!("The image doesn't look like flat art (a photo, or gradients?): its gradients are sewn as bands of flat color.").to_string(),
        ),
        "notLines" => note(
            Mark::Info,
            fill(
                &crate::etn!(
                    count,
                    "{count} area of {color} has no center line to sew along: it is sewn as its shape",
                    "{count} areas of {color} have no center line to sew along: they are sewn as their shapes"
                ),
                &[("color", color)],
            ),
        ),
        "shapesNotJoined" => note(
            Mark::Warning,
            crate::etn!(
                count,
                "{count} area couldn't be joined to its neighbors: it is sewn as traced",
                "{count} areas couldn't be joined to their neighbors: they are sewn as traced"
            ),
        ),
        "nothingTraced" => note(Mark::Warning, crate::et!("Nothing in the image was traced: it is empty or transparent.").to_string()),
        // Said only when every area is the background.
        "nothingToMake" => note(Mark::Warning, Nothing::Background.words().to_string()),
        _ => None,
    }
}

/// The notes on the result: the Check tab's errors and warnings on the objects (`check`, its reply),
/// then digitizing's warnings (`reply`), problems first, notes last.
pub(crate) fn notes(reply: &Value, check: Option<&Value>, lang: Lang) -> Vec<Note> {
    let mut all = Vec::new();
    if let Some(c) = check {
        let format = check::format_label(c["format"].as_str().unwrap_or_default());
        for rule in check::ordered(c) {
            let mark = check::mark_of(rule);
            if matches!(mark, Mark::Error | Mark::Warning) {
                all.push(Note { mark, words: format!("{} · {}", check::title(rule, format, lang), check::found(rule, lang)), step: 4 });
            }
        }
    }
    all.extend(reply["warnings"].as_array().into_iter().flatten().filter_map(|w| warning_words(w, lang)));
    all.sort_by_key(|n| n.mark.rank());
    all
}

/// The notes on the result shown: its reply's warnings and the check of its plan (not an earlier
/// one's).
pub(crate) fn current_notes(live: &mut Live, lang: Lang) -> Vec<Note> {
    let check = live
        .schedule
        .shown
        .as_ref()
        .and_then(|s| s.checked.as_ref().filter(|(n, _)| Some(*n) == s.outputs.number(DigitizeStage::Plan)).map(|(_, c)| c.clone()));
    live.reply().map(|r| notes(r, check.as_deref(), lang)).unwrap_or_default()
}

/// The bar in `ui`: the state, the figures and the notes at its left, the buttons at its right →
/// what was asked.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, now: f64) -> Option<Asked> {
    let lang = crate::i18n::current();
    let live = app.embroidery.image_window.live.as_mut()?;
    let said_now = said(live, now);
    let refused = create_refused(live, now);
    let pending = live.schedule.freshness(now);
    if let Some(s) = said_now.as_ref().filter(|_| !matches!(pending, Freshness::Pending { .. })) {
        live.announced = s.words(lang);
    }
    let notes = current_notes(live, lang);
    let updated = live.schedule.updated.clone();
    let shown_figures = live.last_figures(said_now.as_ref());
    let create_waiting = live.create_waiting;
    let announced = live.announced.clone();
    let mut asked = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                status_words(ui, &pending, said_now.as_ref(), shown_figures.as_ref(), create_waiting, &announced, lang);
                notes_chip(ui, &notes, lang);
            });
            if let Some((ms, stages)) = &updated {
                updated_words(ui, *ms, stages, lang);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let create = widgets::primary_button_enabled(ui, crate::et!("Create Embroidery Objects"), refused.is_none());
            let create = match &refused {
                Some(why) => create.on_disabled_hover_text(why),
                None => create,
            };
            if create.clicked() {
                asked = Some(Asked::Create);
            }
            if widgets::secondary_button(ui, crate::et!("Cancel")).clicked() {
                asked = Some(Asked::Cancel);
            }
            if widgets::secondary_button(ui, crate::et!("Start Over")).clicked() {
                asked = Some(Asked::StartOver);
            }
        });
    });
    asked
}

/// The bar's first words: how the result stands, and the figures (dimmed while stale); a polite
/// status for screen readers, which hear `announced`, said once a run settles.
fn status_words(ui: &mut Ui, pending: &Freshness, said: Option<&Said>, last: Option<&Figures>, create_waiting: bool, announced: &str, lang: Lang) {
    let t = Tokens::get(ui.ctx());
    let mut parts: Vec<(String, Color32)> = Vec::new();
    if create_waiting {
        parts.push((crate::et!("Create waits for the preview to be ready.").to_string(), t.text));
    }
    match pending {
        Freshness::Pending { notice: true, progress } => parts.push((words::recomputing(*progress), t.text)),
        Freshness::Failed(reason) => {
            parts.push((fill(crate::et!("Can't digitize it: {reason}"), &[("reason", reason)]), marks::text_colour(&t, Mark::Error)))
        }
        _ => {}
    }
    let stale = !matches!(pending, Freshness::Fresh);
    match said {
        Some(Said::Nothing(why)) => parts.push((why.words().to_string(), t.text)),
        _ => {
            if let Some(f) = last {
                let colour = if stale { t.text.gamma_multiply(STALE_ALPHA) } else { t.text_strong };
                parts.push((figures_words(f, lang), colour));
            }
        }
    }
    let tip = crate::et!("The image's objects alone, with the document's fabric, thread and machine.");
    let mut job = egui::text::LayoutJob::default();
    for (k, (words, colour)) in parts.iter().enumerate() {
        if k > 0 {
            job.append("  ", 0.0, egui::TextFormat::default());
        }
        job.append(words, 0.0, egui::TextFormat { font_id: FontId::proportional(FIGURES_TEXT), color: *colour, ..Default::default() });
    }
    let resp = ui.add(egui::Label::new(job).sense(Sense::hover())).on_hover_text(tip);
    let announced = announced.to_string();
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
        node.set_label(announced);
        node.set_description(tip);
    });
}

/// "Updated in 96 ms after the last change (Image → Colors → Shapes → Stitches)": the steps the
/// change ran.
fn updated_words(ui: &mut Ui, ms: f64, stages: &[DigitizeStage], lang: Lang) {
    let t = Tokens::get(ui.ctx());
    let names = words::step_names();
    let mut steps: Vec<u8> = stages.iter().map(|s| s.step()).collect();
    steps.dedup();
    let steps: Vec<&str> = steps.iter().filter_map(|&s| names.get(usize::from(s).saturating_sub(1)).copied()).collect();
    let words = fill(
        crate::et!("Updated in {value} ms after the last change ({steps})"),
        &[("value", &numbers::decimal(lang, ms, 0)), ("steps", &steps.join(" → "))],
    );
    ui.label(RichText::new(words).size(SMALL_TEXT).color(t.text_dim));
}

/// The notes' chip: the first note and "+2" for the others; a click lists them all.
fn notes_chip(ui: &mut Ui, notes: &[Note], lang: Lang) {
    let Some(first) = notes.first() else { return };
    let t = Tokens::get(ui.ctx());
    let more = notes.len() - 1;
    // The first note cut short past its room; how many more after it, never cut.
    let mut job = egui::text::LayoutJob::simple_singleline(first.words.clone(), FontId::proportional(SMALL_TEXT), marks::text_colour(&t, first.mark));
    job.wrap = egui::text::TextWrapping { max_width: CHIP_WORDS, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    let more_galley = (more > 0)
        .then(|| ui.painter().layout_no_wrap(format!("+{}", numbers::count(lang, more as u64)), FontId::proportional(SMALL_TEXT), t.text_dim));
    let more_width = more_galley.as_ref().map_or(0.0, |g| g.size().x + CHIP_PAD);
    let size = vec2(MARK + CHIP_PAD * 3.0 + galley.size().x + more_width, galley.size().y.max(MARK) + CHIP_PAD);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let back = if resp.hovered() { t.hover } else { t.panel_darker };
    ui.painter().rect_filled(rect, f32::from(t.radius), back);
    ui.painter().rect_stroke(rect, f32::from(t.radius), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    marks::paint_on(ui.painter(), pos2(rect.left() + CHIP_PAD + MARK / 2.0, rect.center().y), MARK, first.mark, &t, back);
    let words_left = rect.left() + CHIP_PAD * 2.0 + MARK;
    let words_width = galley.size().x;
    ui.painter().galley(pos2(words_left, rect.center().y - galley.size().y / 2.0), galley, t.text);
    if let Some(g) = more_galley {
        ui.painter().galley(pos2(words_left + words_width + CHIP_PAD, rect.center().y - g.size().y / 2.0), g, t.text_dim);
    }
    let name = crate::etn!(notes.len() as u64, "{count} note", "{count} notes");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .accessible_name(crate::et!("Notes"))
        .show(|ui| {
            ui.set_width(NOTES_WIDTH);
            for n in notes {
                ui.horizontal(|ui| {
                    let (mark_rect, _) = ui.allocate_exact_size(vec2(MARK, MARK), Sense::hover());
                    marks::paint(ui.painter(), mark_rect.center(), MARK, n.mark, &t);
                    ui.add(egui::Label::new(RichText::new(&n.words).size(SMALL_TEXT).color(marks::text_colour(&t, n.mark))).wrap());
                });
            }
        });
}

/// The status bar's words once Create made the objects: how many, in how many threads, and the
/// template layer the image moved to (`layer`, its name). The counts stand in parentheses, so that
/// every language can write them as it inflects a count.
pub(crate) fn created_words(objects: u64, threads: u64, layer: &str) -> String {
    fill(
        crate::et!("Created {objects} ({threads}). The image is on the hidden template layer “{layer}”. Undo removes them."),
        &[
            ("objects", &crate::etn!(objects, "{count} object", "{count} objects")),
            ("threads", &crate::etn!(threads, "{count} thread", "{count} threads")),
            ("layer", layer),
        ],
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The ids of digitizing's warnings, as the engine gives them.
    const IDS: [&str; 14] = [
        "bandsJoined",
        "choiceNotGray",
        "coarse",
        "colorJoined",
        "cornersDisagree",
        "decodedSmaller",
        "noBackgroundColor",
        "notFlatArt",
        "notLines",
        "nothingToMake",
        "nothingTraced",
        "shapesNotJoined",
        "specksJoined",
        "specksLeftOut",
    ];

    /// Every warning digitizing gives is said in words, its values in them; one this version
    /// doesn't know is left out rather than shown in the engine's prose.
    #[test]
    fn every_warning_is_said_in_words() {
        let values = json!({"count": 3, "color": "#e0115f", "minArea": 1.0, "maxWidth": 0.6, "pxPerMm": 3.5, "fewerThan": 5.0,
            "decoded": [4000, 3000], "pixels": [8000, 6000], "thread": {"name": "Dark Fuchsia"}});
        for id in IDS {
            let note = warning_words(&json!({"id": id, "values": values}), Lang::EN).unwrap_or_else(|| panic!("{id}: no words"));
            assert!(!note.words.is_empty() && !note.words.contains('{'), "{id}: {}", note.words);
        }
        assert_eq!(warning_words(&json!({"id": "somethingNew", "values": {}, "message": "prose"}), Lang::EN), None);
        let specks = warning_words(&json!({"id": "specksJoined", "values": {"count": 1, "minArea": 1.0}}), Lang::EN).map(|n| n.words);
        assert_eq!(specks.as_deref(), Some("1 speck smaller than 1 mm² joined the color around it"));
    }

    /// Notes come problems first: the check's errors, then warnings, then digitizing's notes.
    #[test]
    fn notes_put_problems_first() {
        let reply = json!({"warnings": [{"id": "specksJoined", "values": {"count": 2, "minArea": 1.0}}, {"id": "coarse", "values": {"pxPerMm": 3.0, "fewerThan": 5.0}}]});
        let check = json!({"format": "jef", "rules": [
            {"id": "stitchTooLong", "severity": "warning", "hits": 0, "threshold": 12.7},
            {"id": "satinTooWide", "severity": "error", "hits": 2, "objectCount": 1, "threshold": 12.0},
        ]});
        let marks: Vec<Mark> = notes(&reply, Some(&check), Lang::EN).iter().map(|n| n.mark).collect();
        assert_eq!(marks, [Mark::Error, Mark::Warning, Mark::Info], "a rule that passed is no note");
    }

    /// The notes' chip shows the first note, cut short past its room, and how many more, never cut;
    /// screen readers hear how many notes there are.
    #[test]
    fn the_notes_chip_keeps_how_many_more() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let long = "A note long enough to be cut short in the chip, ".repeat(4);
        let notes = [Note { mark: Mark::Warning, words: long, step: 2 }, Note { mark: Mark::Info, words: "Short".into(), step: 1 }];
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| notes_chip(ui, &notes, Lang::EN));
        out.textures_delta.clear();
        let mut texts = Vec::new();
        for c in &out.shapes {
            if let egui::Shape::Text(t) = &c.shape {
                texts.push((t.galley.text().to_string(), t.galley.elided));
            }
        }
        assert!(texts.iter().any(|(t, cut)| t.starts_with("A note long") && *cut), "{texts:?}");
        assert!(texts.iter().any(|(t, cut)| t == "+1" && !cut), "{texts:?}");
        let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
        assert!(nodes.iter().any(|(_, n)| n.label() == Some("2 notes")));
    }

    /// Create's words: how many objects, in how many threads, and the layer the image moved to.
    #[test]
    fn create_says_what_it_made() {
        assert_eq!(
            created_words(23, 7, "Template logo.png"),
            "Created 23 objects (7 threads). The image is on the hidden template layer “Template logo.png”. Undo removes them."
        );
    }
}
