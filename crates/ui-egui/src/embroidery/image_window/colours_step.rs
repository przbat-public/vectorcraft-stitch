//! The image window's second step, the Colors: how the image's colours are found (Logo, Colors,
//! Grays, each saying what it does in its tooltip), how many, the threads they are matched to (the
//! palette Create makes the document's, the window saying so), the colour list, and the smallest
//! area kept (smaller specks join the colour around them, counted).
//!
//! A slider's value is taken while it moves, the scheduler deciding when to recompute (the stitch
//! preview's rules); on the web, once it is let go (the project owner's choice, 2026-11-11). A
//! typed value out of its range is refused, said in the status bar, the field keeping its value.

use egui::{FontId, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::DIGITIZE_WORKING_PX_PER_MM;

use super::image_step::range_of;
use crate::VectorcraftApp;
use crate::embroidery::fields::{self, Look};
use crate::embroidery::text::fill;
use crate::embroidery::{rows, vocabulary};
use crate::state::Dialog;
use crate::theme::Tokens;

/// The settings this step sets (the engine's names).
pub(crate) const MODE: &str = "mode";
pub(crate) const COLORS: &str = "colors";
pub(crate) const MIN_AREA: &str = "minArea";

/// The modes in the order the step offers them (the engine's ids), and the engine's default.
const MODES: [&str; 3] = ["logo", "color", "grayscale"];
const DEFAULT_MODE: &str = "color";

/// The warning that counts the specks the smallest area joined.
const SPECKS_JOINED: &str = "specksJoined";

/// A field's width, the size of the step's notes, a radio button's circle and the room after it,
/// and a row of choices' height (points): estimates from the mockups' column.
const FIELD_WIDTH: f32 = 64.0;
const CHOICE_WIDTH: f32 = 230.0;
const NOTE_TEXT: f32 = 11.5;
const LABEL_TEXT: f32 = 12.5;
const RADIO: f32 = 13.0;
const RADIO_GAP: f32 = 5.0;
const RADIO_ROW: f32 = 20.0;
const GAP: f32 = 6.0;
/// The chosen radio button's dot, a share of its circle's radius: half, as egui's own draws it.
const RADIO_DOT: f32 = 0.5;

/// The decimals a smallest area is typed to (hundredths of a square millimetre: the smallest the
/// engine tells apart is about a pixel at the working resolution), and a count's (whole).
const AREA_DECIMALS: usize = 2;
const COUNT_DECIMALS: usize = 0;

/// Whether this is the web, where a slider's value is taken once it is let go.
const WEB: bool = cfg!(target_arch = "wasm32");

/// One working pixel's area at the resolution digitizing aims for (square millimetres: 0.01 at ten
/// pixels a millimetre), where the smallest area's slider starts, as nothing smaller changes what
/// is digitized (the project owner's choice, 2026-11-11); its field still takes nothing (0).
const PIXEL_MM2: f64 = 1.0 / (DIGITIZE_WORKING_PX_PER_MM * DIGITIZE_WORKING_PX_PER_MM);

/// How a slider's track maps to its values.
#[derive(Clone, Copy)]
enum Scale {
    /// Whole numbers, evenly.
    Whole,
    /// Logarithmic, from nothing at its left end, then from `smallest` up.
    Logarithmic { smallest: f64 },
}

/// Mode `id`'s name and what it does.
fn mode_words(id: &str) -> (&'static str, &'static str) {
    match id {
        "logo" => (crate::et!("Logo"), crate::et!("Flat art: exactly its own colors, rare but distinct ones kept, at most the number of colors.")),
        "grayscale" => (crate::et!("Grays"), crate::et!("Shades of gray, as many as the number of colors.")),
        _ => (crate::et!("Colors"), crate::et!("The colors that best represent the picture, at most the number of colors.")),
    }
}

/// The step's controls, for dialog `d`.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let Some(reply) = live.reply().cloned() else { return };
    mode_section(ui, d);
    ui.add_space(GAP);
    count_section(app, ui, d, &reply);
    ui.add_space(GAP);
    palette_section(app, ui, d);
    ui.add_space(GAP);
    super::colour_list::show(app, ui, d, &reply);
    ui.add_space(GAP);
    min_area_section(app, ui, d, &reply);
}

/// A section's heading.
fn heading(ui: &mut Ui, words: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(words).color(t.text_dim));
}

/// A note under a section's controls.
fn note(ui: &mut Ui, words: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(words).size(NOTE_TEXT).color(t.text_dim));
}

/// The mode: Logo, Colors or Grays.
fn mode_section(ui: &mut Ui, d: &mut Dialog) {
    let mode = d.fields.get(MODE).and_then(Value::as_str).unwrap_or(DEFAULT_MODE).to_string();
    heading(ui, crate::et!("Mode"));
    ui.horizontal(|ui| {
        for id in MODES {
            let (words, tip) = mode_words(id);
            if radio(ui, words, mode == id, tip) {
                if id == DEFAULT_MODE {
                    d.fields.remove(MODE);
                } else {
                    d.fields.insert(MODE.into(), json!(id));
                }
            }
        }
    });
}

/// A radio button of `words` (already in the interface language), its tooltip `tip` (also said on
/// the keyboard's focus and to screen readers) → whether it was clicked.
fn radio(ui: &mut Ui, words: &str, selected: bool, tip: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(words.to_string(), FontId::proportional(LABEL_TEXT), t.text);
    let size = vec2(RADIO + RADIO_GAP + galley.size().x, galley.size().y.max(RADIO_ROW));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let centre = pos2(rect.left() + RADIO / 2.0, rect.center().y);
    ui.painter().circle_filled(centre, RADIO / 2.0, t.input);
    ui.painter().circle_stroke(centre, RADIO / 2.0, Stroke::new(1.0, if resp.hovered() { t.text } else { t.button_border }));
    if selected {
        ui.painter().circle_filled(centre, RADIO / 2.0 * RADIO_DOT, t.accent_strong);
    }
    ui.painter().galley(pos2(rect.left() + RADIO + RADIO_GAP, rect.center().y - galley.size().y / 2.0), galley, t.text);
    rows::focus_ring(ui, &resp, rect.expand(rows::FOCUS_GAP), t.panel);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, words));
    rows::keyboard_tip(ui, resp.id, tip);
    rows::described(ui, resp.id, tip);
    resp.on_hover_text(tip).clicked()
}

/// A slider `name` (for screen readers) over `range` on `scale` showing `value` → the value to
/// take: while it moves, or on the web once it is let go (its place kept meanwhile).
fn live_slider(ui: &mut Ui, salt: &str, name: &str, value: f64, range: std::ops::RangeInclusive<f64>, scale: Scale) -> Option<f64> {
    let id = ui.make_persistent_id(salt);
    let mut v = ui.data(|m| m.get_temp::<f64>(id)).unwrap_or(value);
    let slider = egui::Slider::new(&mut v, range).show_value(false);
    let slider = match scale {
        Scale::Whole => slider.step_by(1.0),
        Scale::Logarithmic { smallest } => slider.logarithmic(true).smallest_positive(smallest),
    };
    let resp = crate::widgets::slider(ui, slider);
    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name));
    if WEB && resp.dragged() {
        ui.data_mut(|m| m.insert_temp(id, v));
        return None;
    }
    ui.data_mut(|m| m.remove::<f64>(id));
    ((resp.changed() || resp.drag_stopped()) && v != value).then_some(v)
}

/// `value` taken into setting `key` when it lies in `range`, else refused in the status bar with
/// the range said in `format`.
fn take(app: &mut VectorcraftApp, d: &mut Dialog, key: &str, value: f64, range: (f64, f64), format: &fields::NumberFormat<'_>) {
    if value.is_finite() && value >= range.0 && value <= range.1 {
        d.fields.insert(key.into(), json!(value));
        return;
    }
    let words = fill(
        crate::et!("{value} is out of range: {low} to {high}."),
        &[("value", &format.show(value)), ("low", &format.show(range.0)), ("high", &format.show(range.1))],
    );
    app.status(words);
}

/// The number of colours: a slider and a field.
fn count_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let lang = crate::i18n::current();
    let Some((lo, hi)) = range_of(reply, COLORS) else { return };
    let asked = d.fields.get(COLORS).and_then(Value::as_f64).or(reply["settings"][COLORS].as_f64()).unwrap_or(lo);
    let format = fields::format(lang, "", COUNT_DECIMALS);
    let name = crate::et!("Number of colors");
    heading(ui, name);
    let mut taken = None;
    ui.horizontal(|ui| {
        taken = live_slider(ui, "digitize-colors-slider", name, asked, lo..=hi, Scale::Whole);
        let look = Look { name: Some(name), ..Look::default() };
        if let Some(v) = fields::number_field(ui, "digitize-colors", Some(asked), &format, &look, FIELD_WIDTH, &(lo..=hi)) {
            taken = Some(v);
        }
    });
    if let Some(words) = fields::take_unreadable(ui) {
        app.status(words);
    }
    if let Some(v) = taken {
        take(app, d, COLORS, v.round(), (lo, hi), &format);
    }
    if let Some(words) = inside_note(reply) {
        note(ui, &words);
    }
    let joined = joined_colours(reply);
    if joined > 0 {
        note(ui, &crate::etn!(joined, "{count} color was too small and joined others", "{count} colors were too small and joined others"));
    }
}

/// The traced colours of `reply` that the smallest area joined to others whole, so that no row is
/// left of them.
pub(crate) fn joined_colours(reply: &Value) -> u64 {
    let traced = reply["traced"]["colors"].as_u64().unwrap_or(0);
    traced.saturating_sub(distinct(super::colour_list::rows_of(reply).iter().map(|r| r.color.as_str())))
}

/// What the number of colours counts, said when `reply` sews the background's colour where the
/// design encloses it: the colours besides the background, and that colour too.
pub(crate) fn inside_note(reply: &Value) -> Option<String> {
    let rows = super::colour_list::rows_of(reply);
    if !rows.iter().any(|r| r.inside && r.sewn) {
        return None;
    }
    // Counted first: the catalogue's check reads a plural's words as the call's first strings.
    let besides = distinct(rows.iter().filter(|r| !r.background && !r.inside).map(|r| r.color.as_str()));
    Some(crate::etn!(
        besides,
        "{count} color besides the background, and the background's color sewn where the design encloses it.",
        "{count} colors besides the background, and the background's color sewn where the design encloses it."
    ))
}

/// How many colours `colours` are, each counted once ("#FFFFFF" and "#ffffff" are one).
fn distinct<'a>(colours: impl Iterator<Item = &'a str>) -> u64 {
    let mut seen: Vec<String> = colours.map(str::to_ascii_lowercase).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len() as u64
}

/// The threads the colours are matched to: the palette, said when Create makes it the document's.
fn palette_section(app: &VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let current = d.str(super::PALETTE);
    let palettes = vectorcraft_engine::cmd::embroidery::thread_palettes();
    let words = |id: &str| {
        let count = palettes.iter().find(|(p, _)| *p == id).map_or(0, |(_, n)| *n as u64);
        format!("{} · {}", vocabulary::palette(id), crate::etn!(count, "{count} thread", "{count} threads"))
    };
    let name = crate::et!("Match to threads");
    heading(ui, name);
    let look = Look { name: Some(name), ..Look::default() };
    let chosen = fields::choice(ui, "digitize-palette", &words(&current), &look, CHOICE_WIDTH, |ui| {
        let mut chosen = None;
        for (id, _) in &palettes {
            if ui.add(egui::Button::selectable(*id == current, words(id))).clicked() {
                chosen = Some(*id);
            }
        }
        chosen
    });
    if let Some(id) = chosen.filter(|id| *id != current) {
        d.fields.insert(super::PALETTE.into(), json!(id));
        super::colour_list::drop_threads(&mut d.fields);
    }
    let document = app.session.active().map(|st| vectorcraft_engine::cmd::embroidery::thread_palette_id(&st.doc));
    let now = d.str(super::PALETTE);
    if document.is_some_and(|p| p != now) {
        note(ui, &fill(crate::et!("Create sets the document's thread palette to {palette}."), &[("palette", &vocabulary::palette(&now))]));
    }
}

/// The smallest area: a slider on a logarithmic scale and a field, in square millimetres.
fn min_area_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let lang = crate::i18n::current();
    let Some((lo, hi)) = range_of(reply, MIN_AREA) else { return };
    let value = d.fields.get(MIN_AREA).and_then(Value::as_f64).or(reply["settings"][MIN_AREA].as_f64()).unwrap_or(lo);
    let format = fields::format(lang, "", AREA_DECIMALS);
    let name = crate::et!("Smallest area");
    heading(ui, name);
    let mut taken = None;
    ui.horizontal(|ui| {
        taken = live_slider(ui, "digitize-min-area-slider", name, value, lo..=hi, Scale::Logarithmic { smallest: PIXEL_MM2 });
        let look = Look { name: Some(name), ..Look::default() };
        if let Some(v) = fields::number_field(ui, "digitize-min-area", Some(value), &format, &look, FIELD_WIDTH, &(lo..=hi)) {
            taken = Some(v);
        }
        // The unit as the language writes it ("мм²"): the row of a value in square millimetres.
        let unit = fill(crate::et!("{value} mm²"), &[("value", "")]);
        ui.label(unit.trim());
    });
    if let Some(words) = fields::take_unreadable(ui) {
        app.status(words);
    }
    if let Some(v) = taken {
        take(app, d, MIN_AREA, v, (lo, hi), &format);
    }
    note(ui, crate::et!("Smaller specks, such as JPEG noise, join the color around them."));
    let specks = reply["warnings"].as_array().and_then(|w| w.iter().find(|w| w["id"] == SPECKS_JOINED)).and_then(|w| w["values"]["count"].as_u64());
    if let Some(n) = specks {
        note(ui, &crate::etn!(n, "{count} speck joined", "{count} specks joined"));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// A reply asking `asked` colours, `traced` of them traced, with rows `(colour, background,
    /// sewn)`.
    fn reply(asked: u64, traced: u64, rows: &[(&str, bool, bool)]) -> Value {
        let colors: Vec<Value> =
            rows.iter().map(|&(c, background, sewn)| json!({"color": c, "background": background, "sewn": sewn, "areaCount": 1})).collect();
        json!({"settings": {"colors": asked}, "traced": {"colors": traced}, "colors": colors})
    }

    /// The background's colour sewn where the design encloses it: the count says what it counts,
    /// the colours besides the background, and that colour sewn too (a thread more); not when
    /// nothing is inside, nor when the colour inside is left out.
    #[test]
    fn the_count_says_the_backgrounds_colour_inside_is_sewn_too() {
        let with = |inside: Option<bool>| {
            let mut rows = vec![("#ffffff", true, false), ("#ff0000", false, true), ("#00ff00", false, true), ("#0000ff", false, false)];
            if let Some(sewn) = inside {
                rows.push(("#FFFFFF", false, sewn));
            }
            inside_note(&reply(6, 4, &rows))
        };
        let said = "3 colors besides the background, and the background's color sewn where the design encloses it.";
        assert_eq!(with(Some(true)).as_deref(), Some(said));
        assert_eq!(with(None), None, "nothing inside: nothing to say");
        assert_eq!(with(Some(false)), None, "left out: no thread more");
    }

    /// The smallest area's slider starts at one working pixel's area, not at egui's millionth: a
    /// press at the middle of its track takes the area halfway between a pixel's and the top on its
    /// logarithmic scale, and its default, 1 mm², sits about the middle.
    #[test]
    fn the_smallest_areas_slider_starts_at_a_working_pixel() {
        /// The engine's range of the smallest area and its default (square millimetres).
        const RANGE: (f64, f64) = (0.0, 50.0);
        const DEFAULT: f64 = 1.0;
        /// How near the area taken lies to the one halfway: a share of it, for a press rounded to
        /// the point.
        const NEAR: f64 = 0.05;
        /// How far from the track's middle the default may sit, a share of the track.
        const ABOUT_THE_MIDDLE: f64 = 0.1;
        // One working pixel's area at the engine's working resolution: where the slider starts.
        let pixel = 1.0 / (DIGITIZE_WORKING_PX_PER_MM * DIGITIZE_WORKING_PX_PER_MM);
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let name = "Smallest area";
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0));
        let frame = |events: Vec<egui::Event>, taken: &mut Option<f64>| {
            let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let scale = Scale::Logarithmic { smallest: PIXEL_MM2 };
            let mut out = ctx.run_ui(raw, |ui| {
                if let Some(v) = live_slider(ui, "test-min-area", name, DEFAULT, RANGE.0..=RANGE.1, scale) {
                    *taken = Some(v);
                }
            });
            out.textures_delta.clear();
            out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default()
        };
        let mut taken = None;
        let nodes = frame(vec![], &mut taken);
        let slider = nodes.iter().find(|(_, n)| n.role() == egui::accesskit::Role::Slider && n.label() == Some(name));
        let b = slider.and_then(|(_, n)| n.bounds()).expect("the slider");
        let middle = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        let press =
            |pressed| egui::Event::PointerButton { pos: middle, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        // Pressed in one frame and let go in the next, as a slider takes a press.
        frame(vec![egui::Event::PointerMoved(middle), press(true)], &mut taken);
        frame(vec![press(false)], &mut taken);
        let halfway = (pixel * RANGE.1).sqrt();
        let v = taken.expect("the press took a value");
        assert!((v - halfway).abs() < NEAR * halfway, "took {v} mm², halfway is {halfway} mm²");
        let share = (DEFAULT.log10() - pixel.log10()) / (RANGE.1.log10() - pixel.log10());
        assert!((share - 0.5).abs() < ABOUT_THE_MIDDLE, "the default at {share} of the track");
    }

    /// Colours the smallest area joined to others whole are counted by colour, not by row: the
    /// background's colour inside the design is a second row of one colour.
    #[test]
    fn colours_joined_are_counted_by_colour() {
        let rows = [("#ffffff", true, false), ("#ff0000", false, true), ("#FFFFFF", false, true)];
        assert_eq!(joined_colours(&reply(6, 3, &rows)), 1, "three traced, two among the rows");
        assert_eq!(joined_colours(&reply(6, 2, &rows)), 0);
    }
}
