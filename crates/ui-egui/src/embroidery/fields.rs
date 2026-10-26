//! The embroidery fields (the Embroidery panel's Object tab, Document Setup's Embroidery tab): a
//! number field and a choice, drawn as the app's own fields are (upstream's `widgets` functions,
//! called as they are), with what the embroidery rows add on top.
//!
//! - **Their numbers** write the interface language's decimal mark and read both marks (the
//!   project owner's choice, 2026-10-23): a comma language writes "0,38 mm" and reads "0,38" and
//!   "0.38"; a point language writes "0.38 mm" and reads the point alone, since "1,000" would
//!   otherwise read as one; text that holds no number is said in the status bar ("Not a number:
//!   0,35"). A whole-number field in a language that groups thousands with a point reads "1.000"
//!   as a thousand; the minus sign reads as a hyphen. Every other field of the app keeps its point.
//! - **Each unit steps in a size of its own** (↑/↓, the wheel, a drag on the field's label; Shift
//!   ten steps, Ctrl/Cmd a tenth): a tenth of a millimetre, a degree, a percent, a thread weight,
//!   ten stitches a minute. Upstream's stepping counts in whole numbers, so the field hands it its
//!   value counted in steps. Units are the engine's field table's (`setting_field`'s `unit`).
//! - **Their look** ([`Look`]): the value's colour (a value set on the objects strong, an inherited
//!   one dim), words in an empty field (what Auto chose), and what screen readers call the field
//!   and say of its value.

use std::hash::Hash;
use std::ops::RangeInclusive;

use egui::{Color32, FontId, Ui};

use crate::embroidery::numbers;
use crate::i18n::Lang;
use crate::theme::Tokens;
use crate::widgets;

/// A millimetre field's step, as a power of ten: a tenth of a millimetre (a hundredth with
/// Ctrl/Cmd, one with Shift). Embroidery lengths run from a twentieth of a millimetre to a dozen: a
/// step of one would leap across most of them. An estimate.
pub const MM_STEP: i32 = -1;
/// A speed field's step, as a power of ten: ten stitches a minute (a hundred with Shift). Machines
/// are set in tens. An estimate.
pub const SPEED_STEP: i32 = 1;
/// The step of a field in degrees, percent or thread weight, as a power of ten: one.
pub const UNIT_STEP: i32 = 0;

/// The units a field writes after its number, by their Latin names: each written as the
/// interface language's catalogue gives it in the embroidery context (Russian "мм", as the Check
/// tab writes it), the Latin name read too.
const MM: &str = "mm";
const WEIGHT: &str = "wt";
const SPEED: &str = "/min";
const DEGREES: &str = "°";
const PERCENT: &str = "%";
/// What stands between a number and its unit: a space before a unit's name, none before a degree
/// sign, and before a percent sign as the language spaces it ([`numbers::PERCENT_SPACES`]).
const SPACE: &str = " ";
const NO_BREAK_SPACE: &str = "\u{a0}";
const NARROW_NO_BREAK_SPACE: &str = "\u{202f}";

/// The size of a field's words, points: as upstream's `recessed_text` writes them.
const FIELD_TEXT: f32 = 12.5;

/// How a field writes its number and reads what is typed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NumberFormat<'a> {
    /// The unit after the number (`mm`, `°`, `%`) as the language writes it, what stands between
    /// them, and the unit's Latin name, read too.
    pub unit: &'a str,
    pub space: &'a str,
    pub latin: &'a str,
    /// The unit before the number, as Turkish writes a percent ("%100", [`numbers::PERCENT_FIRST`]).
    pub unit_first: bool,
    /// The most decimals written, trailing zeros left out.
    pub decimals: usize,
    /// The decimal mark written and read; a point is read too.
    pub mark: char,
    /// The mark the language groups thousands with: a point reads as it in a whole-number field
    /// ("1.000" is a thousand in German), else as a decimal mark.
    pub group: char,
    /// How far a plain step goes, as a power of ten ([`MM_STEP`], [`UNIT_STEP`], [`SPEED_STEP`]).
    pub step: i32,
}

impl NumberFormat<'_> {
    /// `value` as the field shows it: at most `decimals` places, without trailing zeros, in the
    /// decimal mark, then the unit; nothing for a number that isn't finite.
    pub(crate) fn show(&self, value: f64) -> String {
        if !value.is_finite() {
            return String::new();
        }
        let number = format!("{:.*}", self.decimals, value);
        let number = if number.contains('.') { number.trim_end_matches('0').trim_end_matches('.').to_string() } else { number };
        let number = if self.mark == '.' { number } else { number.replace('.', &self.mark.to_string()) };
        match (self.unit.is_empty(), self.unit_first) {
            (true, _) => number,
            (false, true) => format!("{}{}{number}", self.unit, self.space),
            (false, false) => format!("{number}{}{}", self.space, self.unit),
        }
    }

    /// The number `text` holds, its decimal mark read as a point; `None` when it holds none. The
    /// unit, as the language writes it or by its Latin name (and %, °), may follow any operand:
    /// `45*2°`, `50% / 2`; a no-break space (a language's, before its percent sign) reads as a space.
    pub(crate) fn read(&self, text: &str) -> Option<f64> {
        let mut bare = text.to_string();
        for unit in [self.unit, self.latin].into_iter().filter(|u| !u.is_empty()) {
            bare = bare.replace(unit, "");
        }
        // The minus sign (CLDR's for Swedish and Finnish) and an en dash read as a hyphen.
        let bare = bare.replace(['\u{2212}', '\u{2013}'], "-");
        let bare = if self.decimals == 0 && self.group == '.' && grouped(&bare) { bare.replace('.', "") } else { bare };
        let bare = if self.mark == '.' { bare } else { bare.replace(self.mark, ".") };
        vectorcraft_doc::parse_number(&bare.replace(['%', '°'], "").replace(['\u{a0}', '\u{202f}'], " "))
    }

    /// The size of a plain step.
    fn step_size(&self) -> f64 {
        10f64.powi(self.step)
    }
}

/// Whether `text` is a whole number grouped in thousands by points ("1.000", "-12.500").
fn grouped(text: &str) -> bool {
    let t = text.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    let mut groups = t.split('.');
    let first = groups.next().unwrap_or_default();
    let rest: Vec<&str> = groups.collect();
    let digits = |g: &str| g.chars().all(|c| c.is_ascii_digit());
    (1..=3).contains(&first.len()) && digits(first) && !rest.is_empty() && rest.iter().all(|g| g.len() == 3 && digits(g))
}

/// Under egui's memory: the words for the status bar when a field's typed text held no number
/// ([`take_unreadable`]).
fn unreadable_id() -> egui::Id {
    egui::Id::new("embroidery-field-unreadable")
}

/// The words a field left this frame for the status bar, when its typed text held no number: the
/// tab drawing it says them (`app.status`), so that the field doesn't go back silently.
pub(crate) fn take_unreadable(ui: &Ui) -> Option<String> {
    ui.data_mut(|d| d.remove_temp::<String>(unreadable_id()))
}

/// The most characters of typed text the status bar repeats. An estimate.
const MAX_REPEATED: usize = 32;

/// The format of an embroidery field in `unit` (the engine's: `mm`, `°`, `wt`, `/min`, `%`) to
/// `decimals` places, as `lang` writes numbers and the unit.
pub(crate) fn format(lang: Lang, unit: &str, decimals: usize) -> NumberFormat<'static> {
    let (latin, space, step) = match unit {
        "mm" => (MM, SPACE, MM_STEP),
        "°" => (DEGREES, "", UNIT_STEP),
        "wt" => (WEIGHT, SPACE, UNIT_STEP),
        "/min" => (SPEED, SPACE, SPEED_STEP),
        "%" => (PERCENT, percent_space(lang), UNIT_STEP),
        _ => ("", "", UNIT_STEP),
    };
    let unit = if latin.is_empty() { latin } else { unit_word(lang, latin) };
    let unit_first = latin == PERCENT && numbers::PERCENT_FIRST.contains(&lang.code());
    let symbols = numbers::symbols(lang);
    NumberFormat { unit, space: if unit_first { "" } else { space }, latin, unit_first, decimals, mark: symbols.decimal, group: symbols.group, step }
}

/// Unit `latin` as `lang` writes it after a number: its catalogue row in the embroidery context,
/// else as it is (a degree sign, a percent sign).
fn unit_word(lang: Lang, latin: &'static str) -> &'static str {
    crate::i18n::tr_ctx(lang, crate::embroidery::text::CONTEXT, latin)
}

/// What stands between a number and a percent sign, as `lang` spaces it.
fn percent_space(lang: Lang) -> &'static str {
    match numbers::PERCENT_SPACES.iter().find(|(code, _)| *code == lang.code()).map(|(_, c)| *c) {
        Some('\u{a0}') => NO_BREAK_SPACE,
        Some(_) => NARROW_NO_BREAK_SPACE,
        None => "",
    }
}

/// How an embroidery field draws its value, on top of the app's field: `None`s draw as the app
/// does.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Look<'a> {
    /// The value's colour while the field isn't being edited (while it is, the app's strong text);
    /// `None`: the app's.
    pub text: Option<Color32>,
    /// Words shown in the value's colour in an empty field (what Auto chose); `None`: none.
    pub hint: Option<&'a str>,
    /// What screen readers call the field, and what they say of its value (where it comes from);
    /// `None`: what the toolkit gives.
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
}

impl Look<'_> {
    /// Name the widget `id` for screen readers as the look says, its words in an empty field as its
    /// placeholder (what Auto chose, a pull's two sides: painted, so said too).
    fn name(&self, ui: &Ui, id: egui::Id) {
        if self.name.is_none() && self.description.is_none() && self.hint.is_none() {
            return;
        }
        ui.ctx().accesskit_node_builder(id, |node| {
            if let Some(name) = self.name {
                node.set_label(name);
            }
            if let Some(description) = self.description {
                node.set_description(description);
            }
            if let Some(hint) = self.hint {
                node.set_placeholder(hint);
            }
        });
    }
}

/// A number field showing `value` (blank when `None`: the objects' values differ, or a rule's) as
/// `format` writes it, drawn with `look`, whose ↑/↓, wheel and scrub step in the format's size and
/// stop at `bounds`: upstream's number field (`widgets::mixed_field`), its text box, stepping and
/// scrub called as they are. Returns the value typed (Enter, Shift+Enter or focus loss), stepped or
/// scrubbed.
pub(crate) fn number_field(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    value: Option<f64>,
    format: &NumberFormat<'_>,
    look: &Look<'_>,
    width: f32,
    bounds: &RangeInclusive<f64>,
) -> Option<f64> {
    let id = ui.id().with(id);
    let shown = value.map(|v| format.show(v)).unwrap_or_default();
    widgets::take_dialog_focus(ui, id, &shown);
    let (mut buf, commit, resp, rect) = widgets::recessed_text(ui, id, &shown, width, 1);
    paint_look(ui, &resp, rect, &buf, look);
    widgets::select_all_on_focus(ui, &resp, &buf);
    // Upstream steps whole numbers (a tenth with Ctrl/Cmd, ten with Shift): it is handed the value
    // counted in the format's steps, its bounds and its decimals with it.
    let size = format.step_size();
    let in_steps = |v: f64| v / size;
    let steps_bounds = in_steps(*bounds.start())..=in_steps(*bounds.end());
    let decimals = format.decimals.min(6) as i32;
    let read = |text: &str| format.read(text).map(in_steps);
    let show = |steps: f64| format.show(steps * size);
    let stepped = widgets::step_value(ui, &resp, &mut buf, &steps_bounds, read, show).map(|n| widgets::round_to(n * size, decimals));
    let scrubbed = crate::scrub::field(ui, id, rect, &buf, value.map(in_steps), (decimals + format.step).max(0), &steps_bounds)
        .map(|n| widgets::round_to(n * size, decimals));
    if commit && buf != shown {
        let read = format.read(&buf);
        if read.is_none() && !buf.trim().is_empty() {
            let text: String = buf.chars().take(MAX_REPEATED).collect();
            let words = crate::embroidery::text::fill(crate::et!("Not a number: {text}"), &[("text", &text)]);
            ui.data_mut(|d| d.insert_temp(unreadable_id(), words));
        }
        read
    } else {
        stepped.or(scrubbed).filter(|&v| Some(v) != value)
    }
}

/// The look over a field drawn by upstream's `recessed_text` (its text edit's response `resp`, its
/// box `rect`, its text `text`): while it isn't being edited, its value in the look's colour (over
/// the app's, in the text edit's own place) or, empty, the look's words; and its name for screen
/// readers.
fn paint_look(ui: &Ui, resp: &egui::Response, rect: egui::Rect, text: &str, look: &Look<'_>) {
    look.name(ui, resp.id);
    if ui.memory(|m| m.has_focus(resp.id)) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let colour = look.text.unwrap_or(t.text_strong);
    let words = match look.hint {
        Some(hint) if text.is_empty() => hint,
        _ if colour != t.text_strong && !text.is_empty() => text,
        _ => return,
    };
    // Inside the box's border; over the app's own words when there are any.
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0).intersect(ui.clip_rect()));
    if !text.is_empty() {
        painter.rect_filled(resp.rect.expand(1.0), 0.0, t.input);
    }
    painter.text(resp.rect.left_top(), egui::Align2::LEFT_TOP, words, FontId::proportional(FIELD_TEXT), colour);
}

/// A choice showing `current`, drawn with `look` (the value's colour, its name for screen readers):
/// upstream's combo box (`widgets::combo`), called as it is → the option `list` returns.
pub(crate) fn choice<R>(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug + Copy,
    current: &str,
    look: &Look<'_>,
    width: f32,
    list: impl FnOnce(&mut Ui) -> Option<R>,
) -> Option<R> {
    // The combo's words take the ui's text colour; its list, in a window of its own, doesn't.
    let before = ui.visuals().override_text_color;
    if let Some(colour) = look.text {
        ui.visuals_mut().override_text_color = Some(colour);
    }
    let chosen = widgets::combo(ui, id, current, width, false, list);
    ui.visuals_mut().override_text_color = before;
    look.name(ui, combo_button(ui, id));
    chosen
}

/// The id of the button `widgets::combo` draws for `id` in `ui`: egui's combo box's, in the frame
/// it is drawn in (egui gives a frame's contents the id of its ui with "child"), as `combo` itself
/// finds it to tell whether it has the keyboard.
pub(crate) fn combo_button(ui: &Ui, id: impl Hash + std::fmt::Debug) -> egui::Id {
    let frame = ui.id().with(egui::IdSalt::new("child"));
    frame.with(egui::IdSalt::new(frame.with(id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> Lang {
        Lang::from_code(code).unwrap()
    }

    /// A comma language writes its comma and reads both marks; a point language writes and reads
    /// the point alone, so "1,000" isn't read as one.
    #[test]
    fn fields_write_the_languages_mark_and_read_both() {
        let pl = format(lang("pl"), "mm", 2);
        assert_eq!(pl.show(0.38), "0,38 mm");
        assert_eq!((pl.read("0,35"), pl.read("0.35"), pl.read("0,35 mm"), pl.read("1,5*2")), (Some(0.35), Some(0.35), Some(0.35), Some(3.0)));
        let en = format(Lang::EN, "mm", 2);
        assert_eq!(en.show(0.38), "0.38 mm");
        assert_eq!((en.read("0.35"), en.read("1,000")), (Some(0.35), None));
        assert_eq!(format(lang("de"), "%", 0).show(100.0), "100\u{a0}%");
        assert_eq!(format(lang("fr"), "%", 0).show(100.0), "100\u{202f}%");
        assert_eq!(format(lang("de"), "%", 0).read("120 %"), Some(120.0));
        assert_eq!(format(Lang::EN, "°", 1).show(135.0), "135°");
        assert_eq!(format(lang("tr"), "%", 0).show(100.0), "%100", "Turkish writes the sign first");
        assert_eq!(format(lang("tr"), "%", 0).read("%120"), Some(120.0));
        assert_eq!(format(Lang::EN, "/min", 0).show(700.0), "700 /min");
    }

    /// What a field shows reads back as the value, in every language and unit.
    #[test]
    fn what_a_field_shows_reads_back() {
        for info in &crate::i18n::LANGUAGES {
            let Some(lang) = Lang::from_code(info.code) else { continue };
            for (unit, value) in [("mm", 0.38), ("°", 135.5), ("%", 120.0), ("wt", 40.0), ("/min", 650.0), ("mm", -1.25)] {
                let f = format(lang, unit, 2);
                assert_eq!(f.read(&f.show(value)), Some(value), "{} {unit}: {:?}", info.code, f.show(value));
            }
        }
    }

    /// A field writes its unit as the language's catalogue does (Russian "мм", as the Check tab
    /// does) and reads it typed either way.
    #[test]
    fn a_field_writes_the_languages_unit_and_reads_both() {
        let ru = format(lang("ru"), "mm", 2);
        assert_eq!(ru.show(0.38), "0,38 мм");
        assert_eq!((ru.read("0,4 мм"), ru.read("0,4 mm"), ru.read("0,4")), (Some(0.4), Some(0.4), Some(0.4)));
        assert_eq!(format(lang("uk"), "/min", 0).show(650.0), "650 /хв");
        assert_eq!(format(lang("ka"), "mm", 1).show(2.5), "2,5 მმ");
        assert_eq!(format(Lang::EN, "wt", 0).show(40.0), "40 wt");
    }

    /// A whole-number field in a language that groups thousands with a point reads "1.000" as a
    /// thousand; a field with decimals still reads the point as a decimal mark; the minus sign and
    /// an en dash read as a hyphen.
    #[test]
    fn grouped_thousands_and_minus_signs_read_as_written() {
        let de = |unit, decimals| format(lang("de"), unit, decimals);
        assert_eq!((de("/min", 0).read("1.000"), de("/min", 0).read("1.200 /min")), (Some(1000.0), Some(1200.0)));
        assert_eq!(de("mm", 2).read("1.000"), Some(1.0));
        assert_eq!(format(Lang::EN, "/min", 0).read("1.000"), Some(1.0), "a point language's point is its decimal mark");
        assert_eq!((format(lang("sv"), "mm", 2).read("\u{2212}1,5"), format(Lang::EN, "mm", 2).read("\u{2013}2")), (Some(-1.5), Some(-2.0)));
    }

    /// A number that isn't finite is written as nothing, never "inf" or "NaN".
    #[test]
    fn a_number_that_isnt_finite_is_written_as_nothing() {
        for v in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(format(Lang::EN, "mm", 2).show(v), "");
        }
    }

    /// A seeded sweep, as a property test would (this crate has no proptest): in every language and
    /// unit, what a field shows of a number reads back within half its last decimal, written again
    /// the same, and
    /// reading any text (multibyte marks, no-break spaces, operators, units, digits mixed) never
    /// panics.
    #[test]
    fn a_fields_format_reads_back_and_never_panics() {
        /// A small linear congruential generator: the same numbers every run.
        struct Seeded(u64);
        impl Seeded {
            fn next(&mut self) -> u64 {
                self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                self.0 >> 33
            }
        }
        const NUMBERS: usize = 300;
        const TEXTS: usize = 300;
        let pieces =
            ["0", "1", "9", ",", ".", "-", "−", "–", " ", "\u{a0}", "\u{202f}", "%", "°", "mm", "мм", "*", "/", "(", "e", "ż", "毫米", "1.000"];
        let mut rng = Seeded(7);
        for info in &crate::i18n::LANGUAGES {
            let Some(l) = Lang::from_code(info.code) else { continue };
            for unit in ["mm", "°", "%", "wt", "/min"] {
                for decimals in [0usize, 1, 2] {
                    let f = format(l, unit, decimals);
                    for _ in 0..NUMBERS {
                        let magnitude = 10f64.powi((rng.next() % 7) as i32 - 2);
                        let sign = if rng.next().is_multiple_of(2) { 1.0 } else { -1.0 };
                        let x = sign * (rng.next() % 10_000) as f64 / 10_000.0 * magnitude;
                        // Read back within half its last decimal, and written again the same.
                        let half = 0.5 * 10f64.powi(-(decimals as i32)) + 1e-9 * x.abs().max(1.0);
                        let shown = f.show(x);
                        let back = f.read(&shown);
                        assert!(
                            back.is_some_and(|b| (b - x).abs() <= half && f.show(b) == shown),
                            "{} {unit} {decimals}: {x} → {shown:?} → {back:?}",
                            info.code
                        );
                    }
                    for _ in 0..TEXTS {
                        let text: String = (0..rng.next() % 8).map(|_| pieces[(rng.next() as usize) % pieces.len()]).collect();
                        let _ = f.read(&text);
                    }
                }
            }
        }
    }

    /// Millimetres step a tenth, speeds ten, the rest one.
    #[test]
    fn each_unit_steps_in_its_own_size() {
        assert_eq!(format(Lang::EN, "mm", 2).step_size(), 0.1);
        assert_eq!(format(Lang::EN, "/min", 0).step_size(), 10.0);
        for unit in ["°", "%", "wt"] {
            assert_eq!(format(Lang::EN, unit, 0).step_size(), 1.0, "{unit}");
        }
    }
}
