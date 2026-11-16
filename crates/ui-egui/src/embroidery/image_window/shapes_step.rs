//! The image window's third step, the Shapes: how the areas' outlines are traced (Smoothness and
//! Corners, upstream Image Trace's Paths and Corners; nearly straight curves straightened into
//! lines), how far the colour sewn first reaches under the next where they meet, and below what
//! width an area is sewn as one line along its middle (the project owner's choice, 2026-11-12:
//! both new controls, by default as digitizing was before them).
//!
//! As in the Colors step, a slider's value is taken while it moves (on the web once it is let go)
//! and a typed value out of its range is refused, said in the status bar, the field keeping its
//! value.

use egui::Ui;
use serde_json::{Value, json};

use super::colours_step::{FIELD_WIDTH, Scale, heading, live_slider, note, take};
use super::image_step::range_of;
use crate::VectorcraftApp;
use crate::embroidery::fields::{self, Look};
use crate::embroidery::rows;
use crate::state::Dialog;
use crate::theme::Tokens;

/// The settings this step sets (the engine's names).
pub(crate) const SMOOTHING: &str = "smoothing";
pub(crate) const CORNERS: &str = "corners";
pub(crate) const STRAIGHT_LINES: &str = "straightLines";
pub(crate) const OVERLAP: &str = "overlap";
pub(crate) const THIN_LINE: &str = "thinLine";

/// The decimals smoothing and corners are typed to (whole: upstream's sliders step by one), and a
/// width in millimetres (tenths: the machine formats' 0.1 mm).
const PERCENT_DECIMALS: usize = 0;
const MM_DECIMALS: usize = 1;

/// The room between the step's sections (points): as the Colors step's.
const GAP: f32 = 6.0;

/// The step's controls, for dialog `d`.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let Some(reply) = live.reply().cloned() else { return };
    let smoothness = Row {
        key: SMOOTHING,
        name: crate::et!("Smoothness"),
        note: crate::et!("Higher: smoother outlines with fewer points. Lower: closer to the picture's edges."),
    };
    slid_section(app, ui, d, &reply, &smoothness);
    ui.add_space(GAP);
    let corners =
        Row { key: CORNERS, name: crate::et!("Corners"), note: crate::et!("Higher: more turns kept as sharp corners. Lower: more of them rounded.") };
    slid_section(app, ui, d, &reply, &corners);
    ui.add_space(GAP);
    straighten_section(ui, d, &reply);
    ui.add_space(GAP);
    let overlap = Row {
        key: OVERLAP,
        name: crate::et!("Colors overlap by"),
        note: crate::et!("The color sewn first reaches under the next, so no fabric shows between them."),
    };
    mm_section(app, ui, d, &reply, &overlap);
    ui.add_space(GAP);
    let thin = Row {
        key: THIN_LINE,
        name: crate::et!("Thin lines sewn along their middle below"),
        note: crate::et!("A narrower part is sewn as a running line along its middle. 0: none."),
    };
    mm_section(app, ui, d, &reply, &thin);
}

/// A setting of the step: the engine's name, its own (in the interface language), and what it does.
struct Row {
    key: &'static str,
    name: &'static str,
    note: &'static str,
}

/// The value of setting `key`: the field's, else the result's.
fn value_of(d: &Dialog, reply: &Value, key: &str) -> Option<f64> {
    d.fields.get(key).and_then(Value::as_f64).or(reply["settings"][key].as_f64())
}

/// Setting `s`, of 0 to 100: its heading, a slider and a field over the engine's range, and its note.
fn slid_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, s: &Row) {
    let lang = crate::i18n::current();
    let Some((lo, hi)) = range_of(reply, s.key) else { return };
    let value = value_of(d, reply, s.key).unwrap_or(lo);
    let format = fields::format(lang, "", PERCENT_DECIMALS);
    heading(ui, s.name);
    let mut taken = None;
    ui.horizontal(|ui| {
        taken = live_slider(ui, &format!("digitize-{}-slider", s.key), s.name, value, lo..=hi, Scale::Whole);
        let look = Look { name: Some(s.name), ..Look::default() };
        if let Some(v) = fields::number_field(ui, ("digitize", s.key), Some(value), &format, &look, FIELD_WIDTH, &(lo..=hi)) {
            taken = Some(v);
        }
    });
    if let Some(words) = fields::take_unreadable(ui) {
        super::refuse(app, words);
    }
    if let Some(v) = taken {
        take(app, d, s.key, v.round(), (lo, hi), &format);
    }
    note(ui, s.note);
}

/// Straightening nearly straight curves into lines: a checkbox.
fn straighten_section(ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let t = Tokens::get(ui.ctx());
    let on = d.fields.get(STRAIGHT_LINES).and_then(Value::as_bool).or(reply["settings"][STRAIGHT_LINES].as_bool()).unwrap_or(false);
    let words = crate::et!("Straighten nearly straight lines");
    let tip = crate::et!("Slightly curved edges become straight lines, parts of circles true arcs, and nearly level or upright lines exactly so.");
    if rows::check_with_tip(ui, words, Some(on), t.panel, tip) {
        d.fields.insert(STRAIGHT_LINES.into(), json!(!on));
    }
}

/// Setting `s`, a width: its name, a field in millimetres over the engine's range, and its note.
fn mm_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, s: &Row) {
    let lang = crate::i18n::current();
    let Some((lo, hi)) = range_of(reply, s.key) else { return };
    let value = value_of(d, reply, s.key).unwrap_or(lo);
    let format = fields::format(lang, "mm", MM_DECIMALS);
    let mut taken = None;
    ui.horizontal(|ui| {
        crate::widgets::field_label(ui, s.name);
        taken = fields::number_field(ui, ("digitize", s.key), Some(value), &format, &Look::default(), FIELD_WIDTH, &(lo..=hi));
    });
    if let Some(words) = fields::take_unreadable(ui) {
        super::refuse(app, words);
    }
    if let Some(v) = taken {
        take(app, d, s.key, v, (lo, hi), &format);
    }
    note(ui, s.note);
}
