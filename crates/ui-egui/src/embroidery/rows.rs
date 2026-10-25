//! A row of the embroidery settings, shared by the Embroidery panel's Object tab and Document
//! Setup's Embroidery tab: a label, a field (a number, a choice, or words), and what says where its
//! value comes from.
//!
//! - **A value of its own** (set on the objects; in Document Setup, changed from its fabric's
//!   preset) has a bar in the accent left of its label, its value in strong text, ↺ to restore it,
//!   and after ↺ what it would be otherwise ("fabric 0,20").
//! - **An inherited value** is dim, with a badge naming where it comes from (auto, fabric, machine,
//!   default).
//! - **The accent outline is the focus alone**: the field being edited (the project owner's
//!   choice, 2026-10-23).
//! - **Objects that differ**: a blank field (a choice says "Mixed"), ↺ unsetting all of them, and
//!   after it how many set it.
//! - **Colours** are the theme's tokens, each moved only as far as it needs to read: the bar by 3:1
//!   against the panel ([`marks::NON_TEXT_CONTRAST`]), dim words by 4.5:1 against what they lie
//!   on ([`marks::TEXT_CONTRAST`]).
//! - **Screen readers** hear each field by its name (its label, with its section's where two labels
//!   read alike), where its value comes from as its description; ↺ is a button; a badge is
//!   decoration, its words being in the description. Ids come from the settings' keys, never from
//!   translated words.
//! - **Room** ([`Columns::fit`]): one line a row, the label column scrubbing its field as the app's
//!   labels do, a field as wide as its words want. Where the panel is too narrow for all of it, the
//!   hint after ↺ gives way first (↺'s tooltip then says it), then long labels wrap beside their
//!   fields, then fields cut their words short; only then does each label go above its field. A
//!   badge, and the count after ↺, keep their room.

use std::ops::RangeInclusive;

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Ui, vec2};

use crate::embroidery::marks;
use crate::theme::Tokens;
use crate::widgets::{self, Look, NumberFormat};

/// The room left of the labels for the bar of a value of its own, points: the bar and a gap.
const GUTTER: f32 = 8.0;
/// The bar's width and height, points: the mockups' 2.5 × 15 (an estimate from the drawing).
const BAR: egui::Vec2 = vec2(2.5, 15.0);
/// Between a label, its field and what follows it, points.
pub(crate) const GAP: f32 = 6.0;
/// A row's height on one line, points: a field's 26 and a point above and below.
const ROW: f32 = 28.0;
/// The fewest and the most points a field takes on a row's line, and the most its words claim
/// before the labels wrap for them: estimates from the mockups' 104-point field.
const FIELD_MIN: f32 = 88.0;
const FIELD_MAX: f32 = 140.0;
const FIELD_WORDS: f32 = 112.0;
/// The share of the width a field takes under its label when the labels go above. An estimate.
const FIELD_UNDER: f32 = 0.6;
/// The words' sizes, points: a label as the panels' labels, a badge's and a hint's a size smaller.
const LABEL_TEXT: f32 = 12.5;
const SMALL_TEXT: f32 = 10.5;
/// A choice's words and the frame around its button, points: as [`widgets::combo_look`] draws them.
const CHOICE_TEXT: f32 = 12.0;
const CHOICE_FRAME: f32 = 4.0;
/// A badge's padding and corners, points.
const BADGE_PAD: egui::Vec2 = vec2(5.0, 1.5);
const BADGE_RADIUS: u8 = 3;
/// ↺'s side, and how far its icon sits inside it, points.
const RESTORE: f32 = 18.0;
const RESTORE_INSET: f32 = 3.0;
/// A checkbox's box, the room between it and its words, the least height of its row and its mark's
/// weight, points: as the app's checkboxes draw them.
const CHECK_BOX: f32 = 13.0;
const CHECK_GAP: f32 = 5.0;
const CHECK_ROW: f32 = 20.0;
const CHECK_MARK: f32 = 1.6;
/// A field of words (a read-only value): as tall as the app's fields, its words as far in.
const WORDS_HEIGHT: f32 = 26.0;
const WORDS_INSET: f32 = 7.0;
const WORDS_TEXT: f32 = 12.5;
const WORDS_RADIUS: u8 = 5;

/// Where a set of rows puts its labels and fields, for the width they are drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Columns {
    /// The label column's width, each row on one line; `None`: each label above its field.
    label: Option<f32>,
    /// A field's width.
    field: f32,
}

impl Columns {
    /// The columns of `rows` in `width` points, measured on their words (the module's **Room**):
    /// each part on one line where it fits; else, in turn, the hints after ↺ left out, the labels
    /// wrapped (down to their longest word), the fields narrowed (down to [`FIELD_MIN`]); else the
    /// labels above their fields.
    pub(crate) fn fit(ui: &Ui, width: f32, rows: &[Row<'_>]) -> Self {
        let room = (width - GUTTER).max(0.0);
        let most = |each: &dyn Fn(&Row<'_>) -> f32| rows.iter().map(each).fold(0.0, f32::max);
        let label = most(&|r| text_width(ui, r.label, LABEL_TEXT));
        let word = most(&|r| r.label.split_whitespace().map(|w| text_width(ui, w, LABEL_TEXT)).fold(0.0, f32::max));
        let field = most(&|r| field_words(ui, r)).clamp(FIELD_MIN, FIELD_WORDS);
        let aside = most(&|r| aside_width(ui, r, false));
        let hinted = most(&|r| aside_width(ui, r, true));
        let fits = |label: f32, field: f32, aside: f32| label + GAP + field + GAP + aside <= room;
        let (label, field) = if fits(label, field, hinted) {
            (Some(label), (room - label - 2.0 * GAP - hinted).min(FIELD_MAX))
        } else if fits(label, field, aside) {
            (Some(label), (room - label - 2.0 * GAP - aside).min(FIELD_MAX))
        } else if fits(word, field, aside) {
            (Some(room - field - 2.0 * GAP - aside), field)
        } else if fits(word, FIELD_MIN, aside) {
            (Some(word), room - word - 2.0 * GAP - aside)
        } else {
            (None, (room * FIELD_UNDER).max(FIELD_MIN.min(room)))
        };
        Self { label, field }
    }
}

/// The width row `r`'s field wants for its words: a choice's and read-only words whole; a number's
/// are short.
fn field_words(ui: &Ui, r: &Row<'_>) -> f32 {
    let s = ui.spacing();
    match &r.field {
        Field::Choice { shown, .. } => text_width(ui, shown, CHOICE_TEXT) + CHOICE_FRAME + 2.0 * s.button_padding.x + s.icon_spacing + s.icon_width,
        Field::Words(words) => text_width(ui, words, WORDS_TEXT) + 2.0 * WORDS_INSET,
        Field::Number { .. } => FIELD_MIN,
    }
}

/// The room what follows row `r`'s field takes: ↺ and the words after it (its hint too when
/// `hinted`), or its badge.
fn aside_width(ui: &Ui, r: &Row<'_>, hinted: bool) -> f32 {
    let words = |w: Option<&str>| w.filter(|w| !w.is_empty()).map_or(0.0, |w| text_width(ui, w, SMALL_TEXT));
    match r.restore {
        Some(_) => RESTORE + GAP / 2.0 + words(r.aside.as_deref()).max(if hinted { words(r.hint.as_deref()) } else { 0.0 }),
        None => r.aside.as_deref().filter(|w| !w.is_empty()).map_or(0.0, |w| text_width(ui, w, SMALL_TEXT) + 2.0 * BADGE_PAD.x),
    }
}

/// What a row edits.
pub(crate) enum Field<'a> {
    /// A number in `format`, kept within `bounds`; `None`: blank (objects that differ, a rule's
    /// value), `hint` shown in it then.
    Number { value: Option<f64>, format: NumberFormat<'a>, bounds: RangeInclusive<f64>, hint: Option<&'a str> },
    /// A choice showing `shown`, from `options` (id, words), `current` marked in the list.
    Choice { shown: &'a str, options: &'a [(String, String)], current: Option<&'a str> },
    /// Words, read only.
    Words(&'a str),
}

/// One row.
pub(crate) struct Row<'a> {
    /// The setting's key: the row's ids come from it, never from words.
    pub(crate) key: &'a str,
    pub(crate) label: &'a str,
    /// What screen readers call its field: the label, with its section where two labels read
    /// alike ("Underlay: Type").
    pub(crate) name: &'a str,
    /// What the setting does, shown over the label.
    pub(crate) tip: &'a str,
    pub(crate) field: Field<'a>,
    /// A value of its own: the bar and strong text.
    pub(crate) own: bool,
    /// ↺ shown, named `restore` for screen readers and in its tooltip: a value of its own, or some
    /// of several objects set it.
    pub(crate) restore: Option<&'a str>,
    /// The words after the field, which keep their room: after ↺, how many of the objects set it
    /// (or a note); with no ↺, the badge's (where the value comes from).
    pub(crate) aside: Option<String>,
    /// After ↺, what the value would be otherwise ("fabric 0,20"): shown where it fits whole, else
    /// ↺'s tooltip says it.
    pub(crate) hint: Option<String>,
    /// Where its value comes from, for screen readers.
    pub(crate) description: String,
}

/// What a row's person did.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Edit {
    Number(f64),
    Choice(String),
    Restore,
}

/// A section's heading over its rows.
pub(crate) fn heading(ui: &mut Ui, words: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(GAP);
    ui.label(egui::RichText::new(words).size(LABEL_TEXT).strong().color(t.text_strong));
}

/// Draw `row` in `cols` → what its person did this frame.
pub(crate) fn show(ui: &mut Ui, cols: &Columns, row: &Row<'_>) -> Option<Edit> {
    let mut edit = None;
    match cols.label {
        Some(label) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.set_min_height(ROW);
                bar(ui, row.own);
                label_cell(ui, row, label, true);
                ui.add_space(GAP);
                edit = field(ui, cols.field, row);
                ui.add_space(GAP);
                edit = aside(ui, row).or(edit.take());
            });
        }
        None => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                bar(ui, row.own);
                label_cell(ui, row, (ui.available_width()).max(0.0), false);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.set_min_height(ROW);
                ui.add_space(GUTTER);
                edit = field(ui, cols.field.min(ui.available_width()), row);
                ui.add_space(GAP);
                edit = aside(ui, row).or(edit.take());
            });
        }
    }
    edit
}

/// The gutter left of a label, with the bar in it for a value of its own.
fn bar(ui: &mut Ui, own: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(GUTTER, BAR.y), Sense::hover());
    if own {
        let t = Tokens::get(ui.ctx());
        let colour = marks::readable(t.accent, &[t.panel], t.dark);
        ui.painter().rect_filled(Rect::from_min_size(rect.left_top(), BAR), CornerRadius::same(1), colour);
    }
}

/// A label `width` points wide, its tooltip what the setting does; beside its field (`scrubs`) the
/// whole cell scrubs the field, as the app's labels do.
fn label_cell(ui: &mut Ui, row: &Row<'_>, width: f32, scrubs: bool) {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout(row.label.to_string(), FontId::proportional(LABEL_TEXT), t.text, width.max(1.0));
    let height = if scrubs { ROW.max(galley.size().y) } else { galley.size().y };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().galley(Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect).min, galley, t.text);
    if !row.tip.is_empty() {
        resp.on_hover_text(row.tip);
    }
    if scrubs {
        crate::scrub::note_label(ui, rect);
    }
}

/// The row's field, `width` points wide → what was typed, stepped, scrubbed or chosen.
fn field(ui: &mut Ui, width: f32, row: &Row<'_>) -> Option<Edit> {
    let t = Tokens::get(ui.ctx());
    let text = value_colour(&t, row.own);
    let look = Look { text: Some(text), border: None, hint: None, name: Some(row.name), description: Some(&row.description) };
    match &row.field {
        Field::Number { value, format, bounds, hint } => {
            widgets::number_field(ui, (row.key, "number"), *value, format, &Look { hint: *hint, ..look }, width, bounds).map(Edit::Number)
        }
        // In a cell of its own width, its words cut short there: a combo lays its words out in the
        // room its row has left, so a long one would push the rest of the row out.
        Field::Choice { shown, options, current } => ui
            .allocate_ui_with_layout(vec2(width, ROW), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_max_width(width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                widgets::combo_look(ui, (row.key, "choice"), shown, &look, width, false, |ui| {
                    let mut chosen = None;
                    for (id, words) in options.iter() {
                        if ui.add(egui::Button::selectable(*current == Some(id.as_str()), words.as_str())).clicked() {
                            chosen = Some(id.clone());
                        }
                    }
                    chosen
                })
            })
            .inner
            .map(Edit::Choice),
        Field::Words(words) => {
            words_field(ui, width, row, words, text);
            None
        }
    }
}

/// A value read only, drawn as a field holding it.
fn words_field(ui: &mut Ui, width: f32, row: &Row<'_>, words: &str, colour: Color32) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, WORDS_HEIGHT), Sense::hover());
    ui.painter().rect(rect, CornerRadius::same(WORDS_RADIUS), t.input, egui::Stroke::new(1.0, t.input_border), egui::StrokeKind::Inside);
    let room = (rect.width() - 2.0 * WORDS_INSET).max(1.0);
    let galley = egui::WidgetText::from(egui::RichText::new(words).size(WORDS_TEXT).color(colour)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        room,
        egui::TextStyle::Body,
    );
    let at = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect.shrink2(vec2(WORDS_INSET, 0.0))).min;
    ui.painter().galley(at, galley, colour);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{}: {words}", row.name)));
    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_description(row.description.clone()));
}

/// After the field: ↺ and the words after it (the hint only where it fits whole, else in ↺'s
/// tooltip), or the badge → a click on ↺.
fn aside(ui: &mut Ui, row: &Row<'_>) -> Option<Edit> {
    let t = Tokens::get(ui.ctx());
    let mut edit = None;
    let mut after = row.aside.as_deref().filter(|w| !w.is_empty());
    if let Some(name) = row.restore {
        let room = ui.available_width() - RESTORE - GAP / 2.0;
        let hint = row.hint.as_deref().filter(|h| !h.is_empty());
        let shown = hint.filter(|h| after.is_none() && text_width(ui, h, SMALL_TEXT) <= room);
        let tip = match hint.filter(|_| shown.is_none()) {
            Some(h) => format!("{name} · {h}"),
            None => name.to_string(),
        };
        if restore(ui, name, &tip).clicked() {
            edit = Some(Edit::Restore);
        }
        ui.add_space(GAP / 2.0);
        after = after.or(shown);
    }
    let Some(words) = after else { return edit };
    let room = ui.available_width().max(1.0);
    if row.restore.is_some() {
        let colour = marks::readable_text(t.text_dim, &[t.panel], t.dark);
        let galley = egui::WidgetText::from(egui::RichText::new(words).size(SMALL_TEXT).color(colour)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            room,
            egui::TextStyle::Small,
        );
        let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
        ui.painter().galley(rect.min, galley, colour);
        resp.on_hover_text(words);
    } else {
        badge(ui, words, room);
    }
    edit
}

/// A badge holding `words`, at most `room` points wide.
fn badge(ui: &mut Ui, words: &str, room: f32) {
    let t = Tokens::get(ui.ctx());
    let colour = marks::readable_text(t.text, &[t.divider], t.dark);
    let galley = egui::WidgetText::from(egui::RichText::new(words).size(SMALL_TEXT).color(colour)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (room - 2.0 * BADGE_PAD.x).max(1.0),
        egui::TextStyle::Small,
    );
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * BADGE_PAD, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(BADGE_RADIUS), t.divider);
    ui.painter().galley(rect.min + BADGE_PAD, galley, colour);
    resp.on_hover_text(words);
}

/// ↺, a button screen readers call `name`, its tooltip `tip`.
fn restore(ui: &mut Ui, name: &str, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(RESTORE, RESTORE), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(BADGE_RADIUS), t.hover);
    }
    crate::icons::paint(ui, "rotate-ccw", rect.shrink(RESTORE_INSET), t.icon);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    resp.on_hover_text(tip)
}

/// A checkbox of `words` (already in the interface language), `None` a dash where the objects
/// differ, drawn as the app's are but its words wrapping in the room they have → whether it was
/// clicked.
pub(crate) fn check(ui: &mut Ui, words: &str, value: Option<bool>) -> bool {
    let t = Tokens::get(ui.ctx());
    let room = (ui.available_width() - CHECK_BOX - CHECK_GAP).max(1.0);
    let galley = ui.painter().layout(words.to_string(), FontId::proportional(LABEL_TEXT), t.text, room);
    let size = vec2(CHECK_BOX + CHECK_GAP + galley.size().x, galley.size().y.max(CHECK_ROW));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let first_line = galley.rows.first().map_or(galley.size().y, |r| r.rect().height());
    let bx = Rect::from_min_size(egui::pos2(rect.left(), rect.top() + (first_line - CHECK_BOX) / 2.0), vec2(CHECK_BOX, CHECK_BOX));
    ui.painter().galley(egui::pos2(bx.right() + CHECK_GAP, rect.top()), galley, t.text);
    let checked = value == Some(true);
    ui.painter().rect_filled(bx, CornerRadius::same(2), if checked { t.accent_strong } else { t.input });
    let border = if resp.hovered() { t.text } else { t.button_border };
    ui.painter().rect_stroke(bx, CornerRadius::same(2), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let mark = egui::Stroke::new(CHECK_MARK, if checked { Color32::WHITE } else { t.text });
    match value {
        Some(true) => {
            let (a, b, c) = (bx.left_center() + vec2(3.0, 0.0), bx.center_bottom() + vec2(-1.0, -3.5), bx.right_top() + vec2(-3.0, 3.0));
            ui.painter().line_segment([a, b], mark);
            ui.painter().line_segment([b, c], mark);
        }
        None => {
            ui.painter().line_segment([bx.left_center() + vec2(3.0, 0.0), bx.right_center() - vec2(3.0, 0.0)], mark);
        }
        Some(false) => {}
    }
    resp.widget_info(|| match value {
        Some(on) => egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, words),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, ui.is_enabled(), words),
    });
    resp.clicked()
}

/// The colour of a field's value: strong for a value of its own, else dim, read by 4.5:1 on the
/// field.
pub(crate) fn value_colour(t: &Tokens, own: bool) -> Color32 {
    if own { t.text_strong } else { marks::readable_text(t.text_dim, &[t.input], t.dark) }
}

/// How wide `words` are at `size` points.
fn text_width(ui: &Ui, words: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(words.to_string(), FontId::proportional(size), Color32::WHITE).size().x
}
