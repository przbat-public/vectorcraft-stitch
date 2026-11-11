//! The image window's steps: a strip of four cards across its top (each with its view in small, its
//! name, what it made in words, how long it took, and whether it is being recomputed), and the
//! settings column's accordion, the current step's section open and the others closed under their
//! names and summaries. Choosing a step opens its section and shows its view (unless Compare is on:
//! then Compare shows the original against that step's view).

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::DigitizeStage;

use super::preview::{self, View};
use super::schedule::Shown;
use super::{STEP, VIEW, words};
use crate::VectorcraftApp;
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::state::Dialog;
use crate::theme::Tokens;

/// How many steps the window has.
pub(crate) const STEPS: u64 = 4;

/// A card's height, its view's size in it, and the gap between cards (points): estimates from the
/// mockups' strip.
pub(crate) const CARD_HEIGHT: f32 = 56.0;
const THUMB: f32 = 44.0;
const CARD_GAP: f32 = 8.0;
const CARD_PAD: f32 = 6.0;

/// The size of a card's name and of its other words (points).
const NAME_TEXT: f32 = 13.0;
const SMALL_TEXT: f32 = 11.5;

/// The stages a step shows the result of, in order.
fn stages_of(step: u64) -> &'static [DigitizeStage] {
    match step {
        1 => &[DigitizeStage::Image],
        2 => &[DigitizeStage::Trace, DigitizeStage::Regions, DigitizeStage::Colours],
        3 => &[DigitizeStage::Lines, DigitizeStage::Shapes],
        _ => &[DigitizeStage::Objects, DigitizeStage::Plan],
    }
}

/// The step `step` and its name.
fn name(step: u64) -> &'static str {
    words::step_names().get((step.max(1) - 1) as usize).copied().unwrap_or("")
}

/// How long step `step` took when its result shown was made (milliseconds): its stages' times.
pub(crate) fn time_ms(shown: &Shown, step: u64) -> Option<f64> {
    stages_of(step).iter().map(|&s| shown.outputs.ms(s)).sum()
}

/// What step `step` made, in words, from the reply `reply` of the result shown.
pub(crate) fn summary(step: u64, shown: &Shown, reply: &Value) -> String {
    let lang = crate::i18n::current();
    match step {
        1 => {
            let Some((w, h)) = shown.outputs.size_mm() else { return String::new() };
            let background =
                if reply["image"]["background"]["from"] == "none" { crate::et!("every color sewn") } else { crate::et!("background left out") };
            fill(
                crate::et!("{width} × {height} mm · {background}"),
                &[("width", &numbers::decimal(lang, w, 1)), ("height", &numbers::decimal(lang, h, 1)), ("background", background)],
            )
        }
        2 => {
            let threads = reply["colorCount"].as_u64().unwrap_or(0);
            if reply["image"]["background"]["from"] == "none" {
                crate::etn!(threads, "{count} thread", "{count} threads")
            } else {
                crate::etn!(threads, "{count} thread + background", "{count} threads + background")
            }
        }
        3 => {
            // Counted first: the catalogue's check reads a plural's words as the call's first strings.
            let areas = reply["areaCount"].as_u64().unwrap_or(0);
            crate::etn!(areas, "{count} area", "{count} areas")
        }
        _ => match reply["stats"]["stitches"].as_u64() {
            Some(n) => crate::etn!(n, "{count} stitch", "{count} stitches"),
            None => String::new(),
        },
    }
}

/// The view a step shows when it is chosen.
fn view_of(step: u64) -> View {
    match step {
        1 => View::Original,
        _ => View::of_step(step),
    }
}

/// Make step `step` the current one: its section open, its view shown (Compare kept).
pub(crate) fn choose(d: &mut Dialog, step: u64) {
    d.fields.insert(STEP.into(), json!(step.clamp(1, STEPS)));
    if preview::View::of(d.str(VIEW).as_str()) != View::Compare {
        let id = match view_of(step) {
            View::Original => "original",
            View::Colors => "colors",
            View::Shapes => "shapes",
            View::Stitches => "stitches",
            View::Compare => "compare",
        };
        d.fields.insert(VIEW.into(), json!(id));
    }
}

/// The strip of step cards in `ui`'s rectangle.
pub(crate) fn strip(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let rect = ui.max_rect();
    let current = d.fields.get(STEP).and_then(Value::as_u64).unwrap_or(2);
    let width = (rect.width() - CARD_GAP * (STEPS as f32 - 1.0)) / STEPS as f32;
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let shown = live.schedule.shown.clone();
    let reply = live.reply().cloned();
    let pending = live.schedule.pending_from().map(DigitizeStage::step).map(u64::from);
    for step in 1..=STEPS {
        let left = rect.left() + (step - 1) as f32 * (width + CARD_GAP);
        let card = Rect::from_min_size(pos2(left, rect.top()), vec2(width, CARD_HEIGHT.min(rect.height())));
        let id = ui.scope_id().with(("digitize-step", step));
        let resp = ui.interact(card, id, Sense::click());
        let selected = step == current;
        let fill_colour = if selected { t.row_selected } else { t.panel_darker };
        ui.painter().rect_filled(card, f32::from(t.radius), fill_colour);
        if resp.has_focus() {
            ui.painter().rect_stroke(card, f32::from(t.radius), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
        }
        let thumb = Rect::from_min_size(card.min + vec2(CARD_PAD, (card.height() - THUMB) / 2.0), vec2(THUMB, THUMB));
        if let Some(shown) = &shown {
            live.preview.thumbnail(ui.painter(), ui.ctx(), shown, view_of(step), thumb);
        }
        let text_left = thumb.right() + CARD_PAD;
        let heading = fill(crate::et!("{number} · {name}"), &[("number", &numbers::count(lang, step)), ("name", name(step))]);
        let weight = if selected { crate::theme::semibold(NAME_TEXT) } else { FontId::proportional(NAME_TEXT) };
        ui.painter().text(pos2(text_left, card.top() + CARD_PAD), Align2::LEFT_TOP, &heading, weight, t.text);
        let said = match (&shown, &reply) {
            (Some(shown), Some(reply)) => summary(step, shown, reply),
            _ => String::new(),
        };
        ui.painter().text(pos2(text_left, card.center().y + 2.0), Align2::LEFT_TOP, &said, FontId::proportional(SMALL_TEXT), t.text_dim);
        let busy = pending.is_some_and(|p| step >= p);
        let time = match (busy, &shown) {
            (true, _) => crate::et!("Computing…").to_string(),
            (false, Some(shown)) => {
                time_ms(shown, step).map_or_else(String::new, |ms| fill(crate::et!("{value} ms"), &[("value", &numbers::decimal(lang, ms, 0))]))
            }
            (false, None) => String::new(),
        };
        ui.painter().text(
            pos2(card.right() - CARD_PAD, card.bottom() - CARD_PAD),
            Align2::RIGHT_BOTTOM,
            &time,
            FontId::proportional(SMALL_TEXT),
            t.text_dim,
        );
        let description = fill(
            crate::et!("Step {number} of {count}: {name}. {summary}. {time}"),
            &[
                ("number", &numbers::count(lang, step)),
                ("count", &numbers::count(lang, STEPS)),
                ("name", name(step)),
                ("summary", &said),
                ("time", &time),
            ],
        );
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::Tab);
            node.set_label(name(step));
            node.set_description(description.clone());
            node.set_selected(selected);
            if busy {
                node.set_busy();
            }
        });
        if resp.clicked() {
            choose(d, step);
        }
    }
}

/// The settings column's accordion: each step's header, the current one open with its settings.
pub(crate) fn accordion(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let current = d.fields.get(STEP).and_then(Value::as_u64).unwrap_or(2);
    let (shown, reply) = match app.embroidery.image_window.live.as_mut() {
        Some(live) => (live.schedule.shown.clone(), live.reply().cloned()),
        None => (None, None),
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for step in 1..=STEPS {
            let open = step == current;
            let lang = crate::i18n::current();
            let said = match (&shown, &reply) {
                (Some(shown), Some(reply)) if !open => summary(step, shown, reply),
                _ => String::new(),
            };
            let heading = fill(crate::et!("{number} · {name}"), &[("number", &numbers::count(lang, step)), ("name", name(step))]);
            let header = crate::widgets::section_toggle_name(ui, &heading, open);
            if !said.is_empty() {
                ui.label(RichText::new(&said).size(SMALL_TEXT).color(t.text_dim));
            }
            if header.clicked() {
                choose(d, step);
            }
            if open {
                ui.add_space(4.0);
                match step {
                    1 => super::image_step::show(app, ui, d),
                    _ => defaults_note(ui, step, reply.as_ref()),
                }
            }
            ui.add_space(6.0);
        }
    });
}

/// What a step whose settings this version of the window keeps at their defaults uses, read only.
fn defaults_note(ui: &mut Ui, step: u64, reply: Option<&Value>) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let Some(r) = reply else { return };
    let s = &r["settings"];
    let lines: Vec<String> = match step {
        2 => vec![],
        3 => vec![
            fill(crate::et!("Smoothness: {value}"), &[("value", &numbers::decimal(lang, s["smoothing"].as_f64().unwrap_or(0.0), 0))]),
            fill(crate::et!("Corners: {value}"), &[("value", &numbers::decimal(lang, s["corners"].as_f64().unwrap_or(0.0), 0))]),
            fill(crate::et!("Colors overlap by {value} mm"), &[("value", &numbers::decimal(lang, s["overlap"].as_f64().unwrap_or(0.0), 1))]),
        ],
        _ => vec![crate::et!("The stitches follow the document's embroidery settings.").to_string()],
    };
    for line in lines {
        ui.label(RichText::new(line).size(SMALL_TEXT).color(t.text));
    }
}
