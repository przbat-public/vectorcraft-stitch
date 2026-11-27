//! The image window's Live bar: how its result stands, the figures of what Create would make, its
//! notes, and Start Over, Cancel, Keep as Live Object and Create Embroidery Objects.
//!
//! - **Figures** in the status bar counters' words ("8,105 stitches · 4 threads · 3 thread changes
//!   · about 12 min · 98 × 88.2 mm"): the image's objects sewn alone, with the document's fabric,
//!   thread and machine (the project owner's choice, 2026-11-11), as its tooltip and its description
//!   say. Under them, how long the last change took to show and which steps it ran.
//! - **States:** computing, said once a run has taken [`super::schedule::STALE_NOTICE_SECS`], with
//!   its progress; the figures; stale (the last figures dimmed while a newer run is on its way);
//!   nothing to sew; failed with the reason, the last figures kept.
//! - **Notes:** each of digitizing's warnings in words, and what the Check tab finds on the objects
//!   (errors and warnings), problems first: a chip with the first, on up to two rows in the room the
//!   bar leaves beside its buttons (whole in its tooltip when cut even so), and "+2"; a click lists
//!   them all.
//! - **Screen readers** hear the bar as a polite status, said again only once a run settles.
//! - **Create** is disabled with its reason when nothing would be made, or the settings can't be
//!   digitized; so is **Keep as Live Object** beside it (the mockup's place, between Cancel and
//!   Create). On a live object opened again they are **Update Live Object** (coral) and **Convert
//!   to Plain Objects** (the project owner's choice, 2026-11-18).
//! - **The question** before a live object's objects changed by hand are replaced takes the
//!   buttons' place (one dialog at a time): its words, Don't Replace (the safe answer, coral, in the
//!   coral button's place, taking the keyboard) and Replace; they answer only once the double-click
//!   time has passed with no mouse button, Enter or Space held, and the keyboard goes back to the
//!   button that asked when it is let go. Its words wrap in rows of about one length.
//! - **The keyboard** shows on the bar's buttons with the panels' focus ring (upstream's buttons
//!   draw none).

use egui::{Align, Color32, FontId, Layout, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::Value;
use vectorcraft_engine::cmd::embroidery::DigitizeStage;

use super::schedule::Freshness;
use super::{Live, Making, frame::Asked, words};
use crate::VectorcraftApp;
use crate::embroidery::counters::{self, Figures};
use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::rows;
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
/// The most rows the chip's first note takes: the Live bar is two lines tall (`frame::BAR_HEIGHT`),
/// and two of the chip's small rows fit above its "Updated in…" line (an estimate).
const CHIP_ROWS: usize = 2;
/// The least room the chip's words take where the bar leaves less (points), an estimate: a few
/// words.
pub(super) const CHIP_LEAST_WORDS: f32 = 80.0;
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
        "nothingToMake" | "outlinesBesideLines" => 4,
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
        parts.push(fill(
            crate::et!("{width} × {height} mm"),
            &[("width", &numbers::decimal(lang, w, words::TENTHS)), ("height", &numbers::decimal(lang, h, words::TENTHS))],
        ));
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
    let area = |key: &str| numbers::decimal(lang, v[key].as_f64().unwrap_or(0.0), words::HUNDREDTHS);
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
                // The resolution always with both its decimals ("2.83", "3.00"): a fraction, so that
                // one form of the noun after it is right for every value (Polish „2,83 piksela”).
                &[
                    ("value", &numbers::fixed(lang, v["pxPerMm"].as_f64().unwrap_or(0.0), words::HUNDREDTHS)),
                    ("least", &numbers::decimal(lang, v["fewerThan"].as_f64().unwrap_or(0.0), words::WHOLE)),
                ],
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
        "outlinesBesideLines" => {
            let share = v["share"].as_f64().filter(|s| s.is_finite()).unwrap_or(0.0).clamp(0.0, 100.0).round() as u64;
            note(
                Mark::Warning,
                fill(
                    crate::et!(
                        "The color outlines run beside the picture's own lines along {share} of their length: those lines are sewn twice. Leave the lines' color out, or the outlines off."
                    ),
                    &[("share", &numbers::percent(lang, share))],
                ),
            )
        }
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
    let reopened = super::on_live_object(app);
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
    // The question's buttons answer only once it is armed (`Question::armed`), as of the frame
    // before: it arms once the double-click time has passed with no mouse button, Enter or Space
    // held (what presses a button).
    let held = ui.input(|i| i.pointer.any_down() || i.key_down(egui::Key::Enter) || i.key_down(egui::Key::Space));
    let double_click = ui.ctx().options(|o| o.input_options.max_double_click_delay);
    let question = live.question.as_mut().map(|q| {
        let asking = (q.armed, std::mem::take(&mut q.focus));
        q.armed |= now - q.shown >= double_click && !held;
        asking
    });
    let refocus = live.refocus.take();
    let mut asked = None;
    // The room the buttons take at the bar's right, so that the chip takes what is left beside them;
    // while the question holds their place, the chip takes its least, the question's words the room.
    let buttons = if question.is_some() {
        None
    } else {
        let [coral, grey] = makers(reopened);
        let labels = [coral.words, grey.words, crate::et!("Cancel"), crate::et!("Start Over")];
        Some(labels.iter().map(|l| widgets::button_width(ui, l)).sum::<f32>() + labels.len() as f32 * ui.spacing().item_spacing.x)
    };
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                status_words(ui, &pending, said_now.as_ref(), shown_figures.as_ref(), create_waiting, &announced, lang);
                let room = buttons.map_or(0.0, |b| ui.available_width() - b);
                notes_chip(ui, &notes, lang, room);
            });
            if let Some((ms, stages)) = &updated {
                updated_words(ui, *ms, stages, lang);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((armed, focus)) = question {
                let [coral, _] = makers(reopened);
                asked = replace_question(ui, armed, focus, widgets::button_width(ui, coral.words));
                return;
            }
            let [coral, grey] = makers(reopened);
            let first = widgets::primary_button_enabled(ui, coral.words, refused.is_none());
            let second = ui.add_enabled_ui(refused.is_none(), |ui| widgets::secondary_button(ui, grey.words)).inner;
            for (maker, resp) in [(coral, first), (grey, second)] {
                if refocus == Some(maker.making) {
                    resp.request_focus();
                }
                ringed(ui, &resp);
                let resp = match maker.tip {
                    Some(tip) => resp.on_hover_text(tip),
                    None => resp,
                };
                if with_refusal(resp, refused.as_deref()).clicked() {
                    asked = Some(match maker.making {
                        Making::Objects => Asked::Create,
                        Making::LiveObject => Asked::KeepLive,
                    });
                }
            }
            let cancel = widgets::secondary_button(ui, crate::et!("Cancel"));
            ringed(ui, &cancel);
            if cancel.clicked() {
                asked = Some(Asked::Cancel);
            }
            let start_over = widgets::secondary_button(ui, crate::et!("Start Over"));
            ringed(ui, &start_over);
            if start_over.clicked() {
                asked = Some(Asked::StartOver);
            }
        });
    });
    asked
}

/// One of the Live bar's two making buttons: what it makes, its words and its tooltip.
#[derive(Clone, Copy)]
pub(crate) struct Maker {
    pub(crate) making: Making,
    pub(crate) words: &'static str,
    tip: Option<&'static str>,
}

/// The Live bar's two making buttons, the coral one first: on a new image Create Embroidery
/// Objects and Keep as Live Object; on a live object opened again (`reopened`) Update Live Object,
/// digitizing it again and keeping it live (where its place isn't sewn, on the layer its design goes
/// to), and Convert to Plain Objects (the project owner's choice, 2026-11-18, „Aktualizacja jako
/// główny (polecam)”: opened to be adjusted, the coral Create turned it into plain objects without
/// saying so).
pub(crate) fn makers(reopened: bool) -> [Maker; 2] {
    let keep = crate::et!("Keeps the image with the objects made of it, so that Digitize Image… can change them again.");
    if reopened {
        [
            Maker {
                making: Making::LiveObject,
                words: crate::et!("Update Live Object"),
                tip: Some(crate::et!("Digitizes the live object again with these settings.")),
            },
            Maker {
                making: Making::Objects,
                words: crate::et!("Convert to Plain Objects"),
                tip: Some(crate::et!("Makes plain embroidery objects of the live object; Digitize Image… doesn't change them again.")),
            },
        ]
    } else {
        [
            Maker { making: Making::Objects, words: crate::et!("Create Embroidery Objects"), tip: None },
            Maker { making: Making::LiveObject, words: crate::et!("Keep as Live Object"), tip: Some(keep) },
        ]
    }
}

/// The keyboard's place on a button of the bar or its notes chip, ringed as the Object tab rings
/// its buttons (upstream's buttons draw none), on the window's fill.
fn ringed(ui: &Ui, resp: &egui::Response) {
    rows::focus_ring(ui, resp, resp.rect.expand(rows::FOCUS_GAP), Tokens::get(ui.ctx()).panel);
}

/// `button`, disabled with the reason Create is refused (`refused`) in its tooltip.
fn with_refusal(button: egui::Response, refused: Option<&str>) -> egui::Response {
    match refused {
        Some(why) => button.on_disabled_hover_text(why),
        None => button,
    }
}

/// The question's id in egui's memory, which holds one open popup at a time: the question holds it
/// while it shows, so that dialogs leave Escape to it.
pub(crate) fn question_id() -> egui::Id {
    egui::Id::unique("digitize-replace-question")
}

/// The question before objects changed by hand are replaced, in place of the bar's buttons (one
/// dialog at a time): its words, Don't Replace and Replace, each described by its words for screen
/// readers. They answer once it is `armed`; Don't Replace, the safe answer, is the coral one, in
/// the coral button's place and at least as wide, so that a double-click on the coral button lands
/// on it, and takes the keyboard when it first shows (`focus`), so that Enter lands on it whichever
/// button asked (a double-click on the grey one lands on Replace, held back by the arming alone);
/// both ringed while they have the keyboard → its answer.
fn replace_question(ui: &mut Ui, armed: bool, focus: bool, coral_width: f32) -> Option<Asked> {
    let words =
        crate::et!("The contents of this live object were changed by hand. Digitizing it again replaces those changes; Undo brings them back.");
    let described = |ui: &Ui, resp: &egui::Response| ui.ctx().accesskit_node_builder(resp.id, |node| node.set_description(words));
    let mut asked = None;
    // As wide as the coral button it stands for at least, so that it holds the whole of its place.
    let keep = widgets::primary_button_width(ui, crate::et!("Don't Replace"), coral_width);
    described(ui, &keep);
    if focus {
        keep.request_focus();
    }
    ringed(ui, &keep);
    if keep.clicked() && armed {
        asked = Some(Asked::KeepEdits);
    }
    let replace = widgets::secondary_button(ui, crate::et!("Replace"));
    described(ui, &replace);
    ringed(ui, &replace);
    if replace.clicked() && armed {
        asked = Some(Asked::Replace);
    }
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(even_rows(ui, words, &FontId::proportional(SMALL_TEXT), t.text_strong)));
    asked
}

/// How many times the question's width is halved toward the narrowest that keeps its rows: to
/// within a 256th of its room (about 2 points of 500), an estimate.
const EVEN_ROW_STEPS: u32 = 8;

/// `words` laid out in the room `ui` has left, in as many rows as they take there but at the
/// narrowest width that keeps that many, so that the rows come out about as long as one another
/// (egui fills each row before the next: words just past the room leave a last word alone on its
/// row), aligned to their left.
fn even_rows(ui: &Ui, words: &str, font: &FontId, colour: Color32) -> std::sync::Arc<egui::Galley> {
    let room = ui.available_width().max(0.0);
    let laid = |width: f32| ui.painter().layout(words.to_string(), font.clone(), colour, width);
    let full = laid(room);
    let rows = full.rows.len();
    if rows < 2 || !room.is_finite() {
        return full;
    }
    // Narrower than the room shared out by its rows, the words take more rows: the narrowest
    // width keeping them lies between.
    let (mut narrow, mut wide) = (room / rows as f32, room);
    for _ in 0..EVEN_ROW_STEPS {
        let middle = (narrow + wide) / 2.0;
        if laid(middle).rows.len() > rows {
            narrow = middle;
        } else {
            wide = middle;
        }
    }
    laid(wide)
}

/// The bar's first words: how the result stands, and the figures (dimmed while stale); a polite
/// status for screen readers, which hear `announced`, said once a run settles.
fn status_words(ui: &mut Ui, pending: &Freshness, said: Option<&Said>, last: Option<&Figures>, create_waiting: bool, announced: &str, lang: Lang) {
    let t = Tokens::get(ui.ctx());
    let mut parts: Vec<(String, Color32)> = Vec::new();
    if create_waiting {
        parts.push((crate::et!("The objects are made once the preview is ready.").to_string(), t.text));
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
        &[("value", &numbers::decimal(lang, ms, words::WHOLE)), ("steps", &steps.join(" → "))],
    );
    ui.label(RichText::new(words).size(SMALL_TEXT).color(t.text_dim));
}

/// The notes' chip, `room` points wide at most: the first note, on up to [`CHIP_ROWS`] rows, and
/// "+2" for the others; a click lists them all.
fn notes_chip(ui: &mut Ui, notes: &[Note], lang: Lang, room: f32) {
    let Some(first) = notes.first() else { return };
    let t = Tokens::get(ui.ctx());
    let more = notes.len() - 1;
    // How many more after the first, never cut; the first broken between words in the room left,
    // cut short with "…" past its rows.
    let more_galley = (more > 0)
        .then(|| ui.painter().layout_no_wrap(format!("+{}", numbers::count(lang, more as u64)), FontId::proportional(SMALL_TEXT), t.text_dim));
    let more_width = more_galley.as_ref().map_or(0.0, |g| g.size().x + CHIP_PAD);
    let words_room = (room - MARK - CHIP_PAD * 3.0 - more_width).max(CHIP_LEAST_WORDS);
    let mut job =
        egui::text::LayoutJob::simple(first.words.clone(), FontId::proportional(SMALL_TEXT), marks::text_colour(&t, first.mark), words_room);
    job.wrap = egui::text::TextWrapping { max_width: words_room, max_rows: CHIP_ROWS, break_anywhere: false, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    let cut = galley.elided;
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
    ringed(ui, &resp);
    let name = crate::etn!(notes.len() as u64, "{count} note", "{count} notes");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    // Cut even so: whole on hover.
    let resp = if cut { resp.on_hover_text(&first.words) } else { resp };
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
/// hidden template layer the image lies on afterwards (`layer`, its name: the one Create moved it
/// to, or its own where it stayed in its place, which isn't sewn; `None` where it lies on none).
/// The counts stand in parentheses, so that every language can write them as it inflects a count.
pub(crate) fn created_words(objects: u64, threads: u64, layer: Option<&str>) -> String {
    let (objects, threads) = (objects_words(objects), threads_words(threads));
    match layer {
        Some(layer) => fill(
            crate::et!("Created {objects} ({threads}). The image is on the hidden template layer “{layer}”. Undo removes them."),
            &[("objects", &objects), ("threads", &threads), ("layer", layer)],
        ),
        None => fill(crate::et!("Created {objects} ({threads}). Undo removes them."), &[("objects", &objects), ("threads", &threads)]),
    }
}

/// The status bar's words once the window made what it made (`reply`, Create's; `layer` the name
/// of the hidden template layer the image lies on afterwards, `None` where none): plain objects of
/// a new image ([`created_words`]), a new live object ([`kept_live_words`]), a live object updated
/// (digitized again, kept live), or one converted to plain objects; told apart by whether it made a
/// live object and whether it was made on one (the reply's `edited`, given for a live object only).
/// What happened comes first, as the status bar shows a message's start alone when it is long.
pub(crate) fn made_words(reply: &Value, layer: Option<&str>) -> String {
    let objects = reply["created"]["objects"].as_u64().unwrap_or(0);
    let threads = reply["stats"]["threadCount"].as_u64().unwrap_or(0);
    let on_live = !reply["edited"].is_null();
    match (reply["created"]["live"].as_bool() == Some(true), on_live) {
        (true, false) => kept_live_words(objects, threads),
        (true, true) => fill(
            crate::et!("Updated the live object: {objects} ({threads}). Undo brings back the earlier version."),
            &[("objects", &objects_words(objects)), ("threads", &threads_words(threads))],
        ),
        (false, true) => match layer {
            Some(layer) => fill(
                crate::et!(
                    "Converted to plain objects: {objects} ({threads}). The image is on the hidden template layer “{layer}”. Undo brings the live object back."
                ),
                &[("objects", &objects_words(objects)), ("threads", &threads_words(threads)), ("layer", layer)],
            ),
            None => fill(
                crate::et!("Converted to plain objects: {objects} ({threads}). Undo brings the live object back."),
                &[("objects", &objects_words(objects)), ("threads", &threads_words(threads))],
            ),
        },
        (false, false) => created_words(objects, threads, layer),
    }
}

/// `objects` in words ("23 objects"), as the status bar's words count them.
fn objects_words(objects: u64) -> String {
    crate::etn!(objects, "{count} object", "{count} objects")
}

/// `threads` in words ("7 threads").
fn threads_words(threads: u64) -> String {
    crate::etn!(threads, "{count} thread", "{count} threads")
}

/// The status bar's words once Keep as Live Object made the live object: of how many objects, in
/// how many threads, and how to change it again.
pub(crate) fn kept_live_words(objects: u64, threads: u64) -> String {
    fill(
        crate::et!("Created a live object: {objects} ({threads}). Digitize Image… changes it again; Undo removes it."),
        &[
            ("objects", &crate::etn!(objects, "{count} object", "{count} objects")),
            ("threads", &crate::etn!(threads, "{count} thread", "{count} threads")),
        ],
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The ids of digitizing's warnings, as the engine gives them.
    const IDS: [&str; 15] = [
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
        "outlinesBesideLines",
        "shapesNotJoined",
        "specksJoined",
        "specksLeftOut",
    ];

    /// Every warning digitizing gives is said in words, its values in them; one this version
    /// doesn't know is left out rather than shown in the engine's prose.
    #[test]
    fn every_warning_is_said_in_words() {
        let values = json!({"count": 3, "color": "#e0115f", "minArea": 1.0, "maxWidth": 0.6, "pxPerMm": 3.5, "fewerThan": 5.0,
            "decoded": [4000, 3000], "pixels": [8000, 6000], "thread": {"name": "Dark Fuchsia"}, "share": 57.0});
        for id in IDS {
            let note = warning_words(&json!({"id": id, "values": values}), Lang::EN).unwrap_or_else(|| panic!("{id}: no words"));
            assert!(!note.words.is_empty() && !note.words.contains('{'), "{id}: {}", note.words);
        }
        assert_eq!(warning_words(&json!({"id": "somethingNew", "values": {}, "message": "prose"}), Lang::EN), None);
        let specks = warning_words(&json!({"id": "specksJoined", "values": {"count": 1, "minArea": 1.0}}), Lang::EN).map(|n| n.words);
        assert_eq!(specks.as_deref(), Some("1 speck smaller than 1 mm² joined the color around it"));
    }

    /// The coarse picture's note writes its resolution with both decimals, a fraction in every
    /// language, so that one form of its noun is right for every value: Polish „2,83 piksela” and
    /// „3,00 piksela”, never „pikseli”, which is right for none of them.
    #[test]
    fn the_coarse_note_writes_its_resolution_as_a_fraction() {
        let note = |code: &str, px: f64| {
            let lang = Lang::from_code(code).expect("a language");
            crate::i18n::set_current(lang);
            let words = warning_words(&json!({"id": "coarse", "values": {"pxPerMm": px, "fewerThan": 4.0}}), lang).map(|n| n.words);
            crate::i18n::set_current(Lang::EN);
            words.unwrap_or_default()
        };
        assert_eq!(note("pl", 2.83), "Przy tym rozmiarze obraz ma 2,83 piksela na milimetr, mniej niż 4: większa kopia digitalizuje się lepiej.");
        assert!(note("pl", 3.0).contains("3,00 piksela"), "{}", note("pl", 3.0));
        assert!(note("en", 1.0).contains("1.00 pixels"), "{}", note("en", 1.0));
        for (code, words) in [("cs", "2,83 pixelu"), ("ru", "2,83 пикселя"), ("uk", "2,83 пікселя")] {
            assert!(note(code, 2.83).contains(words), "{code}: {}", note(code, 2.83));
        }
    }

    /// The notes' chip shows the keyboard with the panels' focus ring, and says its first note whole
    /// in its tooltip where the chip cuts it short.
    #[test]
    fn the_notes_chip_shows_the_keyboard_and_its_cut_note_whole() {
        use egui::accesskit::{Action, ActionRequest, Role, TreeId};
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let long = "A note long enough to be cut short in the chip, ".repeat(4);
        let notes = [Note { mark: Mark::Warning, words: long.clone(), step: 2 }];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0));
        let run = |time: f64, events: Vec<egui::Event>| {
            let raw = egui::RawInput { time: Some(time), screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| notes_chip(ui, &notes, Lang::EN, CHIP_LEAST_WORDS));
            out.textures_delta.clear();
            let mut shapes = Vec::new();
            out.shapes.into_iter().for_each(|c| shapes.push(c.shape));
            (shapes, out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default())
        };
        let (_, nodes) = run(0.0, vec![]);
        let (id, chip) = nodes.iter().find(|(_, n)| n.role() == Role::Button && n.label() == Some("1 note")).expect("the chip");
        let b = chip.bounds().expect("drawn");
        let rect = egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32));
        let focus =
            egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Focus, target_node: *id, target_tree: TreeId::ROOT, data: None });
        run(0.1, vec![focus]);
        let (shapes, _) = run(0.2, vec![]);
        let t = Tokens::get(&ctx);
        let ring = rows::focus_colour(&t, t.panel);
        let ringed =
            shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.stroke.color == ring && r.stroke.width > 0.0 && r.rect.contains_rect(rect)));
        assert!(ringed, "the keyboard's ring round the chip");
        // The pointer on the chip, still past the tooltip's wait; a frame more, as egui lays a new
        // tooltip out unseen first.
        let wait = f64::from(ctx.global_style().interaction.tooltip_delay);
        run(0.3, vec![egui::Event::PointerMoved(rect.center())]);
        run(0.3 + wait * 2.0, vec![]);
        let (shapes, _) = run(0.4 + wait * 2.0, vec![]);
        let text = |s: &egui::Shape| match s {
            egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.elided)),
            _ => None,
        };
        let texts: Vec<(String, bool)> = shapes.iter().filter_map(text).collect();
        assert!(texts.iter().any(|(w, cut)| *w == long && *cut), "cut short in the chip: {texts:?}");
        assert!(texts.iter().any(|(w, cut)| *w == long && !cut), "whole in its tooltip: {texts:?}");
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

    /// While Create, Update or Convert waits for the preview, the status says the objects are made
    /// once it is ready, naming no button: a live object opened again has no Create.
    #[test]
    fn the_status_says_the_objects_wait_for_the_preview() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| status_words(ui, &Freshness::Fresh, None, None, true, "", Lang::EN));
        out.textures_delta.clear();
        let texts: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t.contains("The objects are made once the preview is ready.")), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("Create")), "no button named: {texts:?}");
    }

    /// On a live object opened again, Update's and Convert's tooltips say what each does wherever
    /// the live object lies, with no "in its place" (where its place isn't sewn, its design goes to
    /// the layer above); Convert's says the plain objects it makes are what Digitize Image… doesn't
    /// change again, not the live object it removes.
    #[test]
    fn the_tooltips_of_update_and_convert_say_what_each_does() {
        let [update, convert] = makers(true);
        assert_eq!(update.tip, Some("Digitizes the live object again with these settings."));
        assert_eq!(convert.tip, Some("Makes plain embroidery objects of the live object; Digitize Image… doesn't change them again."));
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
        // The chip's room before it could take the bar's (points).
        const OLD_ROOM: f32 = 300.0;
        let mut out = ctx.run_ui(raw, |ui| notes_chip(ui, &notes, Lang::EN, OLD_ROOM));
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

    /// What the status bar says after the window made something, by what it made and what it was
    /// open on: a new image's plain objects or live object, a live object updated (Undo bringing
    /// back the one before, not removing it) or converted (Undo bringing it back); what happened
    /// first, and the hidden template layer wherever the image lies on one afterwards (the one
    /// Create moved it to, or its own).
    #[test]
    fn the_status_says_what_was_made_and_what_undo_does() {
        let reply = |live: bool, edited: Value| json!({"created": {"objects": 6, "live": live}, "stats": {"threadCount": 4}, "edited": edited});
        let cases = [
            (false, Value::Null, "Created 6 objects (4 threads). The image is on the hidden template layer “T”. Undo removes them."),
            (true, Value::Null, "Created a live object: 6 objects (4 threads). Digitize Image… changes it again; Undo removes it."),
            (true, json!(false), "Updated the live object: 6 objects (4 threads). Undo brings back the earlier version."),
            (true, json!(true), "Updated the live object: 6 objects (4 threads). Undo brings back the earlier version."),
            (
                false,
                json!(true),
                "Converted to plain objects: 6 objects (4 threads). The image is on the hidden template layer “T”. Undo brings the live object back.",
            ),
        ];
        for (live, edited, said) in cases {
            assert_eq!(made_words(&reply(live, edited.clone()), Some("T")), said, "live {live}, edited {edited}");
        }
        // The image on no hidden template layer afterwards (left on an ordinary layer whose place
        // isn't sewn, say): no layer named.
        assert_eq!(made_words(&reply(false, Value::Null), None), "Created 6 objects (4 threads). Undo removes them.");
        assert_eq!(
            made_words(&reply(false, json!(true)), None),
            "Converted to plain objects: 6 objects (4 threads). Undo brings the live object back."
        );
    }

    /// The question's words wrap in rows of about one length, at every room that takes two rows, in
    /// English and in Polish: egui fills each row before the next, which left a last word alone.
    #[test]
    fn the_questions_rows_are_about_one_length() {
        // The bar's widths tried (points): from the room two rows need to a whole row, a step apart;
        // and the least a row is of the longest, an estimate (a lone last word was a tenth).
        const WIDTHS: std::ops::Range<u32> = 300..1400;
        const STEP: usize = 3;
        const EVEN_SHARE: f32 = 0.5;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut two_rows = 0;
        for code in ["en", "pl"] {
            crate::i18n::set_current(Lang::from_code(code).expect("a language"));
            let words = crate::et!(
                "The contents of this live object were changed by hand. Digitizing it again replaces those changes; Undo brings them back."
            )
            .to_string();
            for width in WIDTHS.step_by(STEP) {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width as f32, 100.0));
                let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| replace_question(ui, false, false, 0.0));
                });
                out.textures_delta.clear();
                let galley = out.shapes.iter().find_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == words => Some(t.galley.clone()),
                    _ => None,
                });
                let Some(galley) = galley.filter(|g| g.rows.len() == 2) else { continue };
                two_rows += 1;
                let longest = galley.rows.iter().map(|r| r.rect().width()).fold(0.0_f32, f32::max);
                for row in &galley.rows {
                    assert!(
                        row.rect().width() >= EVEN_SHARE * longest,
                        "{code} at {width}: rows {:?}",
                        galley.rows.iter().map(|r| r.text()).collect::<Vec<_>>()
                    );
                }
            }
        }
        crate::i18n::set_current(Lang::EN);
        assert!(two_rows > 0, "some width gave two rows");
    }

    /// Create's words: how many objects, in how many threads, and the layer the image moved to.
    #[test]
    fn create_says_what_it_made() {
        assert_eq!(
            created_words(23, 7, Some("Template logo.png")),
            "Created 23 objects (7 threads). The image is on the hidden template layer “Template logo.png”. Undo removes them."
        );
    }
}
