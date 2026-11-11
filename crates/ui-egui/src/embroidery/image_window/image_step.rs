//! The image window's first step, the Image: the design's size in millimetres (the width or the
//! height typed, the other following the picture's proportions; or both, a box the design fits
//! within), the crop as four margins in the image's own pixels (Crop to the Art: the box of what
//! isn't the background; Whole Image), and the background: left out (the colour found at the
//! image's corners, the transparency round the art, or one of the image's colours chosen) or sewn.
//! A value out of its range is refused, said in the status bar, the field keeping its value.

use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use crate::VectorcraftApp;
use crate::embroidery::fields::{self, Look};
use crate::embroidery::rows;
use crate::embroidery::text::fill;
use crate::i18n::Lang;
use crate::state::Dialog;
use crate::theme::Tokens;

/// The settings this step sets (the engine's names).
pub(crate) const WIDTH: &str = "width";
pub(crate) const HEIGHT: &str = "height";
pub(crate) const FIT: &str = "fit";
pub(crate) const CROP: &str = "crop";
pub(crate) const BACKGROUND: &str = "background";

/// The background's setting that sews every colour, and the one that finds it at the image's
/// corners (the engine's default, what no setting means).
pub(crate) const NONE: &str = "none";
const AUTO: &str = "corners";

/// Where the reply says the background came from.
const FROM_CORNERS: &str = "corners";
const FROM_TRANSPARENCY: &str = "transparency";
const FROM_GIVEN: &str = "given";

/// A size field's and a margin field's width, and the size of the step's notes (points): estimates
/// from the mockups' column.
const FIELD_WIDTH: f32 = 84.0;
const MARGIN_WIDTH: f32 = 60.0;
const NOTE_TEXT: f32 = 11.5;
const GAP: f32 = 6.0;

/// The decimals a size is typed to (tenths of a millimetre, the formats' step), a margin (whole
/// pixels), and a size's top said in words (whole millimetres).
const MM_DECIMALS: usize = 1;
const PX_DECIMALS: usize = 0;
const WORDS_DECIMALS: usize = 0;

/// A side of the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Width,
    Height,
}

impl Side {
    fn key(self) -> &'static str {
        match self {
            Side::Width => WIDTH,
            Side::Height => HEIGHT,
        }
    }

    fn other(self) -> &'static str {
        match self {
            Side::Width => HEIGHT,
            Side::Height => WIDTH,
        }
    }
}

/// The range setting `name` takes, from the engine's table in `reply` (`table`): its two ends.
pub(crate) fn range_of(reply: &Value, name: &str) -> Option<(f64, f64)> {
    let row = reply["table"].as_array()?.iter().find(|r| r["name"] == name)?;
    Some((row["range"][0].as_f64()?, row["range"][1].as_f64()?))
}

/// The box fields `f` fit the design within, `(width, height)` millimetres.
pub(crate) fn fit_of(f: &Map<String, Value>) -> Option<(f64, f64)> {
    let a = f.get(FIT)?.as_array()?;
    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

/// The crop fields `f` hold, `[x0, y0, x1, y1]` in the image's pixels.
pub(crate) fn crop_of(f: &Map<String, Value>) -> Option<[u32; 4]> {
    let v: Vec<u32> = f.get(CROP)?.as_array()?.iter().filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok())).collect();
    <[u32; 4]>::try_from(v).ok()
}

/// The four margins (left, top, right, bottom) fields `f`'s crop leaves round an image `px` pixels.
pub(crate) fn margins(f: &Map<String, Value>, px: (u32, u32)) -> [u32; 4] {
    let (iw, ih) = px;
    let [x0, y0, x1, y1] = crop_of(f).unwrap_or([0, 0, iw, ih]);
    [x0, y0, iw.saturating_sub(x1), ih.saturating_sub(y1)]
}

/// Side `side` typed as `value` millimetres into fields `f`: that side alone (the other following
/// the picture, the design `size` now), or the box's side while the design fits within one. A size
/// of nothing, or a side past `top` (the engine's table), is refused: the words saying so, the
/// fields as they were.
pub(crate) fn type_side(
    f: &mut Map<String, Value>,
    side: Side,
    value: f64,
    size: Option<(f64, f64)>,
    top: Option<f64>,
    lang: Lang,
) -> Result<(), String> {
    let mm = fields::format(lang, "mm", MM_DECIMALS);
    let whole = fields::format(lang, "mm", WORDS_DECIMALS);
    if !(value.is_finite() && value > 0.0) {
        return Err(fill(crate::et!("A size is more than {low}."), &[("low", &whole.show(0.0))]));
    }
    let fit = fit_of(f);
    let wanted = match (fit, size) {
        (Some((w, h)), _) => Some(if side == Side::Width { (value, h) } else { (w, value) }),
        (None, Some((w, h))) if w > 0.0 && h > 0.0 => Some(if side == Side::Width { (value, value * h / w) } else { (value * w / h, value) }),
        _ => None,
    };
    if let (Some((w, h)), Some(top)) = (wanted, top)
        && (w > top || h > top)
    {
        let words = crate::et!("{width} × {height} is too large: a side is at most {high}.");
        return Err(fill(words, &[("width", &mm.show(w)), ("height", &mm.show(h)), ("high", &whole.show(top))]));
    }
    match (fit, wanted) {
        (Some(_), Some((w, h))) => {
            f.insert(FIT.into(), json!([w, h]));
        }
        _ => {
            f.insert(side.key().into(), json!(value));
            f.remove(side.other());
            f.remove(FIT);
        }
    }
    Ok(())
}

/// The design fitted within a box (`on`) the size it is now (`size`), or its width typed again:
/// the size kept either way.
pub(crate) fn set_fit(f: &mut Map<String, Value>, on: bool, size: (f64, f64)) {
    for key in [WIDTH, HEIGHT, FIT] {
        f.remove(key);
    }
    if on {
        f.insert(FIT.into(), json!([size.0, size.1]));
    } else {
        f.insert(WIDTH.into(), json!(size.0));
    }
}

/// Margin `k` (left, top, right, bottom) typed as `value` pixels into fields `f`, for an image `px`
/// pixels: the crop the margins leave (none when they leave the whole image). Less than none, or
/// margins that leave no picture, are refused.
pub(crate) fn type_margin(f: &mut Map<String, Value>, k: usize, value: f64, px: (u32, u32)) -> Result<(), String> {
    if !(value.is_finite() && value >= 0.0) {
        return Err(crate::et!("A margin is 0 pixels or more.").to_string());
    }
    let (iw, ih) = px;
    let mut m = margins(f, px);
    let Some(slot) = m.get_mut(k) else { return Ok(()) };
    // A float cast saturates: a margin past the image leaves no picture, said below.
    *slot = value.round() as u32;
    let [l, t, r, b] = m;
    let rect = [l, t, iw.saturating_sub(r), ih.saturating_sub(b)];
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return Err(crate::et!("The margins leave no picture: crop less.").to_string());
    }
    if rect == [0, 0, iw, ih] {
        f.remove(CROP);
    } else {
        f.insert(CROP.into(), json!(rect));
    }
    Ok(())
}

/// Fields `f` cropped to `art` (the box of what isn't the background), the background found in the
/// corners kept as the colour left out (`found`: the reply's background): the crop's own corners are
/// the art's.
pub(crate) fn crop_to_art(f: &mut Map<String, Value>, art: [u32; 4], found: &Value) {
    f.insert(CROP.into(), json!(art));
    if f.get(BACKGROUND).and_then(Value::as_str).is_none_or(|s| s == AUTO)
        && found["from"] == FROM_CORNERS
        && let Some(colour) = found["color"].as_str()
    {
        f.insert(BACKGROUND.into(), json!(colour));
    }
}

/// The step's controls, for dialog `d`.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let reply = live.reply().cloned();
    let Some(outputs) = live.schedule.shown.as_ref().map(|s| s.outputs.clone()) else { return };
    let (Some(reply), Some(size), Some(px)) = (reply, outputs.size_mm(), outputs.image_px()) else { return };
    size_section(app, ui, d, &reply, size);
    ui.add_space(GAP);
    crop_section(app, ui, d, &reply, px, outputs.art_bounds());
    ui.add_space(GAP);
    background_section(ui, d, &reply);
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

/// The size: the width and the height, linked, or the box the design fits within.
fn size_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, size: (f64, f64)) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let mm = fields::format(lang, "mm", MM_DECIMALS);
    let top = range_of(reply, WIDTH).map(|r| r.1);
    let fit = fit_of(&d.fields);
    let (width, height) = fit.unwrap_or(size);
    heading(ui, crate::et!("Size"));
    let mut typed = None;
    ui.horizontal(|ui| {
        crate::widgets::field_label(ui, crate::et!("Width"));
        if let Some(v) = fields::number_field(ui, "digitize-width", Some(width), &mm, &Look::default(), FIELD_WIDTH, &(0.0..=top.unwrap_or(f64::MAX)))
        {
            typed = Some((Side::Width, v));
        }
        crate::widgets::field_label(ui, crate::et!("Height"));
        if let Some(v) =
            fields::number_field(ui, "digitize-height", Some(height), &mm, &Look::default(), FIELD_WIDTH, &(0.0..=top.unwrap_or(f64::MAX)))
        {
            typed = Some((Side::Height, v));
        }
    });
    if let Some(words) = fields::take_unreadable(ui) {
        app.status(words);
    }
    if let Some((side, v)) = typed
        && let Err(words) = type_side(&mut d.fields, side, v, Some(size), top, lang)
    {
        app.status(words);
    }
    if rows::check(ui, crate::et!("Fit within the width and height"), Some(fit.is_some()), t.panel) {
        set_fit(&mut d.fields, fit.is_none(), size);
    }
    let said = match fit {
        Some(_) => {
            fill(crate::et!("The design is {width} × {height}: as large as fits."), &[("width", &mm.show(size.0)), ("height", &mm.show(size.1))])
        }
        None => crate::et!("Type the width or the height: the other follows the picture.").to_string(),
    };
    note(ui, &said);
}

/// The crop: four margins in the image's pixels, Crop to the Art and Whole Image.
fn crop_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, px: (u32, u32), art: Option<[u32; 4]>) {
    let lang = crate::i18n::current();
    let format = fields::format(lang, "", PX_DECIMALS);
    let m = margins(&d.fields, px);
    let names = [crate::et!("Left"), crate::et!("Top"), crate::et!("Right"), crate::et!("Bottom")];
    let limits = [px.0, px.1, px.0, px.1];
    heading(ui, crate::et!("Crop"));
    let mut typed = None;
    ui.horizontal_wrapped(|ui| {
        for (k, ((name, margin), limit)) in names.iter().zip(m).zip(limits).enumerate() {
            crate::widgets::field_label(ui, *name);
            let bounds = 0.0..=f64::from(limit);
            if let Some(v) = fields::number_field(ui, ("digitize-crop", k), Some(f64::from(margin)), &format, &Look::default(), MARGIN_WIDTH, &bounds)
            {
                typed = Some((k, v));
            }
        }
    });
    if let Some(words) = fields::take_unreadable(ui) {
        app.status(words);
    }
    if let Some((k, v)) = typed
        && let Err(words) = type_margin(&mut d.fields, k, v, px)
    {
        app.status(words);
    }
    let cropped = crop_of(&d.fields).is_some();
    ui.horizontal(|ui| {
        let to_art = art.filter(|&a| Some(a) != crop_of(&d.fields));
        if ui.add_enabled(to_art.is_some(), egui::Button::new(crate::et!("Crop to the Art"))).clicked()
            && let Some(a) = to_art
        {
            crop_to_art(&mut d.fields, a, &reply["image"]["background"]);
        }
        if ui.add_enabled(cropped, egui::Button::new(crate::et!("Whole Image"))).clicked() {
            d.fields.remove(CROP);
        }
    });
}

/// The background: what is left out, whether it is, and the image's colours to choose it from.
fn background_section(ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let t = Tokens::get(ui.ctx());
    let setting = d.fields.get(BACKGROUND).and_then(Value::as_str).map(str::to_string);
    let found = &reply["image"]["background"];
    let sewn = setting.as_deref() == Some(NONE);
    heading(ui, crate::et!("Background"));
    let (words, colour) = match found["from"].as_str() {
        _ if sewn => (crate::et!("Every color is sewn"), None),
        Some(FROM_TRANSPARENCY) => (crate::et!("Found: the transparency round the art"), None),
        Some(FROM_CORNERS) => (crate::et!("Found in the corners"), found["color"].as_str()),
        Some(FROM_GIVEN) => (crate::et!("Chosen from the picture's colors"), found["color"].as_str()),
        _ => (crate::et!("Not found: the corners disagree"), None),
    };
    ui.horizontal(|ui| {
        if let Some(c) = colour {
            swatch(ui, c, false);
        }
        ui.label(RichText::new(words).color(t.text));
    });
    if rows::check(ui, crate::et!("Leave the background out"), Some(!sewn), t.panel) {
        if sewn {
            d.fields.remove(BACKGROUND);
        } else {
            d.fields.insert(BACKGROUND.into(), json!(NONE));
        }
    }
    let colours: Vec<&str> = reply["colors"].as_array().map(|a| a.iter().filter_map(|c| c["color"].as_str()).collect()).unwrap_or_default();
    if colours.is_empty() {
        return;
    }
    note(ui, crate::et!("Or one of the picture's colors:"));
    ui.horizontal_wrapped(|ui| {
        for c in colours {
            let chosen = setting.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(c));
            if swatch(ui, c, chosen).clicked() {
                d.fields.insert(BACKGROUND.into(), json!(c));
            }
        }
    });
}

/// A swatch's side (points): at least the 24-point target WCAG 2.2 asks for.
const SWATCH: f32 = 24.0;

/// A colour's swatch, a button named by its colour for screen readers, ringed when chosen.
fn swatch(ui: &mut Ui, hex: &str, chosen: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let colour = parse(hex).unwrap_or(t.text_dim);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::click());
    ui.painter().rect_filled(rect, 2.0, colour);
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    if chosen {
        ui.painter().rect_stroke(rect.expand(2.0), 3.0, egui::Stroke::new(2.0, t.text), egui::StrokeKind::Outside);
    }
    rows::focus_ring(ui, &resp, rect.expand(2.0), t.panel);
    let name = fill(crate::et!("Color {color}"), &[("color", hex)]);
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(egui::accesskit::Role::Button);
        node.set_label(name.clone());
        if chosen {
            node.set_selected(true);
        }
    });
    resp.on_hover_text(hex)
}

/// A colour from "#rrggbb".
pub(crate) fn parse(hex: &str) -> Option<egui::Color32> {
    let h = hex.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6)?;
    let [_, r, g, b] = v.to_be_bytes();
    Some(egui::Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en() -> Lang {
        Lang::EN
    }

    /// A typed side is that side alone: the other leaves the fields, and so does a fit.
    #[test]
    fn a_side_typed_is_that_side_alone() {
        let mut f = Map::new();
        f.insert(HEIGHT.into(), json!(40.0));
        type_side(&mut f, Side::Width, 100.0, Some((80.0, 40.0)), Some(1000.0), en()).unwrap();
        assert_eq!((f.get(WIDTH), f.get(HEIGHT), f.get(FIT)), (Some(&json!(100.0)), None, None));
        type_side(&mut f, Side::Height, 30.0, Some((100.0, 50.0)), Some(1000.0), en()).unwrap();
        assert_eq!((f.get(WIDTH), f.get(HEIGHT)), (None, Some(&json!(30.0))));
    }

    /// Within a box, a side typed is the box's: the other side of the box stays.
    #[test]
    fn within_a_box_a_side_typed_is_the_boxs() {
        let mut f = Map::new();
        set_fit(&mut f, true, (80.0, 40.0));
        assert_eq!(fit_of(&f), Some((80.0, 40.0)));
        type_side(&mut f, Side::Height, 60.0, Some((80.0, 40.0)), Some(1000.0), en()).unwrap();
        assert_eq!((fit_of(&f), f.get(WIDTH), f.get(HEIGHT)), (Some((80.0, 60.0)), None, None));
        set_fit(&mut f, false, (80.0, 40.0));
        assert_eq!((fit_of(&f), f.get(WIDTH)), (None, Some(&json!(80.0))));
    }

    /// Nothing, less, or a side past the top (the side typed or the one following it) is refused,
    /// said in words, the fields as they were.
    #[test]
    fn a_size_out_of_range_is_refused_and_kept() {
        let mut f = Map::new();
        f.insert(WIDTH.into(), json!(100.0));
        let kept = f.clone();
        for v in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let words = type_side(&mut f, Side::Width, v, Some((100.0, 50.0)), Some(1000.0), en()).unwrap_err();
            assert_eq!(words, "A size is more than 0 mm.");
        }
        // 600 mm high makes the width 1200 mm; 1200 mm wide is past the top itself.
        let words = type_side(&mut f, Side::Height, 600.0, Some((100.0, 50.0)), Some(1000.0), en()).unwrap_err();
        assert_eq!(words, "1200 mm × 600 mm is too large: a side is at most 1000 mm.");
        let words = type_side(&mut f, Side::Width, 1200.4, Some((100.0, 50.0)), Some(1000.0), en()).unwrap_err();
        assert_eq!(words, "1200.4 mm × 600.2 mm is too large: a side is at most 1000 mm.");
        assert_eq!(f, kept);
    }

    /// A margin typed crops the image; margins that leave no picture, or less than none, are
    /// refused; margins of nothing are no crop.
    #[test]
    fn margins_crop_and_are_refused_when_they_leave_nothing() {
        let mut f = Map::new();
        type_margin(&mut f, 0, 20.0, (200, 100)).unwrap();
        type_margin(&mut f, 3, 10.0, (200, 100)).unwrap();
        assert_eq!(crop_of(&f), Some([20, 0, 200, 90]));
        assert_eq!(margins(&f, (200, 100)), [20, 0, 0, 10]);
        let kept = f.clone();
        assert_eq!(type_margin(&mut f, 2, 180.0, (200, 100)).unwrap_err(), "The margins leave no picture: crop less.");
        assert_eq!(type_margin(&mut f, 1, 1e30, (200, 100)).unwrap_err(), "The margins leave no picture: crop less.");
        assert_eq!(type_margin(&mut f, 1, -1.0, (200, 100)).unwrap_err(), "A margin is 0 pixels or more.");
        assert_eq!(f, kept);
        type_margin(&mut f, 0, 0.0, (200, 100)).unwrap();
        type_margin(&mut f, 3, 0.0, (200, 100)).unwrap();
        assert_eq!(f.get(CROP), None, "the whole image");
    }

    /// Cropping to the art keeps the colour the corners found as the one left out (the crop's own
    /// corners are the art's), and keeps a background a person chose.
    #[test]
    fn cropping_to_the_art_keeps_the_background_found() {
        let mut f = Map::new();
        crop_to_art(&mut f, [20, 10, 180, 90], &json!({"from": "corners", "color": "#ffffff"}));
        assert_eq!((crop_of(&f), f.get(BACKGROUND)), (Some([20, 10, 180, 90]), Some(&json!("#ffffff"))));
        let mut f = Map::new();
        f.insert(BACKGROUND.into(), json!(NONE));
        crop_to_art(&mut f, [20, 10, 180, 90], &json!({"from": "none", "color": null}));
        assert_eq!(f.get(BACKGROUND), Some(&json!(NONE)));
        let mut f = Map::new();
        f.insert(BACKGROUND.into(), json!(AUTO));
        crop_to_art(&mut f, [20, 10, 180, 90], &json!({"from": "corners", "color": "#ffffff"}));
        assert_eq!(f.get(BACKGROUND), Some(&json!("#ffffff")), "the corners named are no setting");
        let mut f = Map::new();
        crop_to_art(&mut f, [20, 10, 180, 90], &json!({"from": "transparency", "color": null}));
        assert_eq!(f.get(BACKGROUND), None, "transparency is the background by itself");
    }
}
