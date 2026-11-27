//! The area inspector: a click on an area in the preview opens a popover beside it with the area's
//! facts, how Auto sews it and why, and the stitch and angle of its own (FORK.md's work item 9; the
//! project owner's decisions of 2026-11-16; the mockup of the Stitches step).
//!
//! - **Opened** by a click on an area in any view but Original; a click on another area moves it
//!   there, a click on the background closes it, and so does Escape: it is held open as egui's one
//!   open popup, which dialogs leave Escape to (another popup opened closes it). It follows its area
//!   through every recompute by the area's key (its deepest point, colour and kind), and closes when
//!   the area is gone.
//! - **Placed** beside its area, to its right, else to its left, else (neither side with room) on
//!   the side with more, inside the preview, which it never leaves: over the design, never over the
//!   settings, and over its area only where neither side has room.
//! - **How Auto sews it** is what the planner sews of it, from the planner's own parting of its
//!   object: "Auto: satin" where every part is a satin column, the reason true of the area as it is
//!   made (grown by the overlap under the areas sewn after it); said under a stitch chosen too,
//!   dimmed, as Auto would make it (under Don't Sew, of the area as it is left out, which isn't
//!   grown), so that what the choice changes stays in sight. Worked out off
//!   the interface thread ([`super::answers`]): the line and its reason show once the answer is in.
//! - **Its choices** follow the area's kind: a filled area Auto, Tatami, Satin, Don't Sew; a line
//!   Auto, Running Stitch, Satin, Don't Sew. Satin is offered on a filled area only where the whole
//!   area is at most 12 mm wide (the project owner's choice, „Do 12 mm szerokości (polecam)”) and the
//!   planner sews a satin column in it at the width Create gives it (2026-11-17, „Tylko tam, gdzie
//!   zadziała (polecam)”), by the widths alone until that answer is in; and on a line from 1 mm wide.
//!   Elsewhere it is greyed out, the reason said under the choices, unless the area's choice is
//!   Satin already or the reason above says it in the same words.
//! - **The angle** of a filled area's rows: Auto (the document's), or a degree of its own, ↺ back;
//!   greyed out, saying why, where no tatami rows are sewn (every part a satin column, or the area
//!   left out).
//! - **A change** is the window's `areaChanges` entry for the area: one per area, under the key of
//!   the change that already applied to it if one did; Auto with no angle of its own removes it.
//! - **Apply to Similar Areas** sets the area's stitch and angle on the areas of its colour and kind
//!   about its size and proportions (the project owner's choice, 2026-11-18), counting those it
//!   would change, greyed out once every one is sewn as this one (outlined in the preview while
//!   the button is hovered); **Show in Sewing
//!   Order** dims what is sewn after the area and says where it is sewn, ◀ ▶ walking the order.

use egui::{Color32, Id, Pos2, Rect, RichText, Ui, vec2};
use serde_json::{Map, Value, json};
use vectorcraft_engine::cmd::embroidery::{
    AreaFacts, AreaKey, DigitizeOutputs, DigitizeStage, MAX_AREA_CHANGES, SATIN_AREA_MOST_MM, SATIN_LINE_LEAST_MM, SatinOffer, SatinParts,
};
use vectorcraft_geom::Point;

use super::answers::Answers;
use crate::embroidery::text::fill;
use crate::embroidery::{fields, numbers, vocabulary};
use crate::theme::Tokens;

/// The window's field holding the changes to single areas: the engine's `areaChanges`.
pub(crate) const CHANGES: &str = "areaChanges";

/// The popover's width (points): the mockup's popover, about a fifth of its window (an estimate).
const WIDTH: f32 = 320.0;

/// The area's colour dot (points), as the colour list draws its swatches small.
const DOT: f32 = 12.0;

/// The segmented choices' height and their words' size (points): the view tabs'.
const CHOICES_HEIGHT: f32 = 24.0;
const CHOICES_TEXT: f32 = 12.0;

/// The space between the popover's parts (points): half the window's gap.
const GAP: f32 = super::frame::GAP / 2.0;

/// Millimetres shown to a tenth, as the window's other figures in millimetres.
const MM_DECIMALS: usize = 1;

/// An angle shown to a tenth of a degree, as the Object tab's angle field.
const ANGLE_DECIMALS: usize = 1;

/// The stitch choices of a filled area and of a line, by their ids in `areaChanges` ("auto": none).
const FILL_CHOICES: [&str; 4] = ["auto", "tatami", "satin", "none"];
const LINE_CHOICES: [&str; 4] = ["auto", "running", "satin", "none"];

/// The area the inspector is open on, by the key it follows its area with; whether it shows the
/// area's place in the sewing order (the preview dimming what is sewn after it), whether Apply to
/// Similar Areas is hovered (the preview outlining them), the areas it would set, as last worked
/// out, and whether it was just opened on its area.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inspected {
    pub(crate) key: AreaKey,
    pub(crate) order: bool,
    pub(crate) similar_shown: bool,
    pub(crate) similar: Option<Similar>,
    /// Opened on its area by a click or the keyboard, not drawn yet: its body, when it scrolls,
    /// starts at its top, the area's name in sight (◀ ▶ keep where it is scrolled, so that they stay
    /// under the pointer).
    pub(crate) fresh: bool,
}

/// The areas alike to the inspected one (with Satin, those it is offered on), those of them Apply to
/// Similar Areas would change (not sewn as this area already; on Auto, every one), whether the
/// engine takes the changes that makes, and whether every area's answer is in (with Satin, whether
/// it is offered on each), with what they were worked out of: worked out again only when that
/// changes, not each frame (a design of a thousand alike dots would otherwise find every one of
/// them a frame).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Similar {
    of: SimilarOf,
    pub(crate) keys: Vec<AreaKey>,
    pub(crate) changing: Vec<AreaKey>,
    room: bool,
    complete: bool,
}

/// What [`Similar`] is worked out of: the result's shapes and plan (their numbers), how many asks
/// for satin answers were answered, the area (its working pixel), its stitch and angle (the angle's
/// bits), and the window's changes, as they were.
type SimilarOf = (Option<u64>, Option<u64>, u64, (u32, u32), Option<String>, Option<u64>, Option<Value>);

/// The popover's id in egui's memory, which holds one open popup at a time: another opened closes
/// this one.
pub(crate) fn popup_id() -> Id {
    Id::unique("digitize-area-inspector")
}

/// A click on the preview at working pixel `at` (`None`: past the design): the inspector opened on
/// the area there, or moved there, or closed on the background.
pub(crate) fn clicked(
    ctx: &egui::Context,
    inspector: &mut Option<Inspected>,
    fields: &Map<String, Value>,
    outputs: &DigitizeOutputs,
    at: Option<(u32, u32)>,
) {
    match at.and_then(|(x, y)| outputs.area_key(x, y)) {
        Some(key) => {
            // Under the key of a change already applying to it, so that its edits change that one.
            let key = applying(fields, outputs, &key).unwrap_or(key);
            *inspector = Some(Inspected { key, order: false, similar_shown: false, similar: None, fresh: true });
            egui::Popup::open_id(ctx, popup_id());
        }
        None => close(ctx, inspector),
    }
}

/// The inspector closed.
pub(crate) fn close(ctx: &egui::Context, inspector: &mut Option<Inspected>) {
    *inspector = None;
    egui::Popup::close_id(ctx, popup_id());
}

/// The inspector drawn beside its area inside the preview's `pane`, when it is open: `fields` are
/// the window's (a choice made changes them), `answers` where its satin answers are worked out,
/// `outputs` the result shown, `to_screen` where a document point is drawn. Closed on Escape, when
/// another popup took egui's one open place, or when its area is gone.
// The window's state each is borrowed from apart: the result shown is borrowed from it while the
// inspector and its answers change, so they can't be handed over as one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &Ui,
    pane: Rect,
    fields: &mut Map<String, Value>,
    inspector: &mut Option<Inspected>,
    answers: &mut Answers,
    outputs: &DigitizeOutputs,
    running: bool,
    to_screen: impl Fn(Point) -> Pos2,
) {
    let ctx = ui.ctx().clone();
    let Some(mut open) = inspector.clone() else { return };
    if !egui::Popup::is_id_open(&ctx, popup_id()) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return close(&ctx, inspector);
    }
    // A run under way (`running`) shows its stages as they come, the areas only once its shapes are
    // done: the inspector waits for them, drawn again then, rather than closing on its area not
    // found yet. A result left without shapes (a run that failed, or was refused) closes it.
    if outputs.number(DigitizeStage::Shapes).is_none() {
        if !running {
            return close(&ctx, inspector);
        }
        egui::Popup::open_id(&ctx, popup_id());
        return;
    }
    let Some(at) = outputs.find(&open.key) else { return close(&ctx, inspector) };
    let Some(facts) = outputs.area_at(at.0, at.1) else { return close(&ctx, inspector) };
    let Some(rect) = outputs.outline_at(at.0, at.1).map(|path| screen_box(&path, &to_screen)) else { return close(&ctx, inspector) };
    // Kept open in egui's memory for this frame, which drops a popup no one keeps.
    egui::Popup::open_id(&ctx, popup_id());
    // Beside the area, its frame clear of it ([`beside`]); the pane holds it. The popover is its
    // width and its frame's margins and stroke.
    let frame = egui::Frame::popup(&ctx.global_style());
    let outer = WIDTH + frame.total_margin().sum().x;
    let left = beside(rect, pane, outer);
    egui::Area::new(popup_id()).order(egui::Order::Foreground).fixed_pos(Pos2::new(left, rect.top())).constrain_to(pane).show(&ctx, |ui| {
        // Taller than the pane (the window at its smallest, words that wrap more in a language), its
        // body scrolls inside it rather than being cut off. Its height is the content's up to that
        // room, whatever height it had the frame before (egui lays an area out in that), so that it
        // grows when its content does: Show in Sewing Order's row.
        let room = (pane.height() - frame.total_margin().sum().y).max(0.0);
        let scroll = egui::ScrollArea::vertical().id_salt("digitize-area-inspector-body").max_height(room).min_scrolled_height(room);
        // One scroll place for every area: opened on another, it starts at its top.
        let scroll = if open.fresh { scroll.vertical_scroll_offset(0.0) } else { scroll };
        frame.show(ui, |ui| {
            ui.set_width(WIDTH);
            scroll.show(ui, |ui| {
                body(ui, fields, outputs, answers, &mut open, at, &facts);
            });
        });
    });
    open.fresh = false;
    *inspector = Some(open);
}

/// The box on screen of `path` (document points).
fn screen_box(path: &vectorcraft_geom::BezPath, to_screen: &impl Fn(Point) -> Pos2) -> Rect {
    use vectorcraft_geom::kurbo::Shape;
    let b = path.bounding_box();
    Rect::from_two_pos(to_screen(Point::new(b.x0, b.y0)), to_screen(Point::new(b.x1, b.y1)))
}

/// Where the popover's left edge goes beside `rect` (its area's box on screen) in `pane`, the
/// popover `outer` points wide with its frame: to the area's right where it fits, else to its left
/// where it fits; where neither side has room, on the side with more, so that the pane, holding it,
/// pushes it over as little of the area as it can.
pub(super) fn beside(rect: Rect, pane: Rect, outer: f32) -> f32 {
    let (right, left_of) = (rect.right() + GAP, rect.left() - GAP - outer);
    let roomier_right = pane.right() - rect.right() >= rect.left() - pane.left();
    if right + outer <= pane.right() || (left_of < pane.left() && roomier_right) { right } else { left_of }
}

/// The popover's contents for the area under working pixel `at`, whose facts are `facts`, inspected
/// by `key`; its satin answers asked of `answers`.
fn body(
    ui: &mut Ui,
    fields: &mut Map<String, Value>,
    outputs: &DigitizeOutputs,
    answers: &mut Answers,
    open: &mut Inspected,
    at: (u32, u32),
    facts: &AreaFacts,
) {
    let key = &open.key.clone();
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let mm = |v: f64| numbers::decimal(lang, v, MM_DECIMALS);
    // Its dot and name, its thread, size and box.
    ui.horizontal(|ui| {
        let [r, g, b] = facts.thread_color.unwrap_or(facts.color);
        let (rect, _) = ui.allocate_exact_size(vec2(DOT, DOT), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), DOT / 2.0, Color32::from_rgb(r, g, b));
        ui.label(RichText::new(super::preview::area_name(facts)).strong().color(t.text_strong));
    });
    let area = fill(crate::et!("{value} mm²"), &[("value", &numbers::decimal(lang, facts.area_mm2, super::words::TENTHS))]);
    let (w, h) = facts.size_mm;
    let thread = facts.thread_name.clone().unwrap_or_default();
    let line =
        fill(crate::et!("{thread} · {area} · {width} × {height} mm"), &[("thread", &thread), ("area", &area), ("width", &mm(w)), ("height", &mm(h))]);
    ui.label(RichText::new(line).small().color(t.text_dim));
    ui.add_space(GAP);
    // How it is sewn now, and why.
    let (stitch, angle) = current(fields, outputs, key);
    let info =
        outputs.area_object(at.0, at.1).and_then(|id| outputs.query("embroidery.info", &json!({"ids": [id.0], "plan": true}))).and_then(Result::ok);
    let part = info.as_ref().map(|i| i[if facts.line { "stroke" } else { "fill" }].clone()).unwrap_or(Value::Null);
    let sewn = part["sewn"].as_str().unwrap_or_default();
    // What the planner sews of it: a filled area whose every part the plan sews as a satin column
    // is sewn as satin, though its object's stitch is tatami (the project owner's choices,
    // 2026-11-17, „Pokazać satynę (polecam)” and „Z planu ściegów (polecam)”). Worked out off the
    // interface thread: the Auto line and its reason wait for it, Satin offered by the widths alone
    // until then.
    if !outputs.satin_known(at.0, at.1) {
        answers.ask(outputs, &[at]);
    }
    let known = outputs.satin_known(at.0, at.1);
    let parts = outputs.satin_parts_kept(at.0, at.1, None);
    let offer = outputs.satin_offer_kept(at.0, at.1).unwrap_or_else(|| facts.satin_offer_by_widths());
    let created = outputs.satin_created_kept(at.0, at.1);
    // How Auto sews it, and why: as planned while it is the area's; under a stitch chosen, as Auto
    // would make it (its own parting, worked out off the interface thread too), dimmed, so that what
    // the choice changes stays in sight. The object's own settings are the choice's then.
    let (auto, why) = if stitch.is_none() {
        let word =
            (!sewn.is_empty()).then(|| vocabulary::sewn_as(if parts == Some(SatinParts::All) { "satin" } else { sewn }, part["rule"].as_str()));
        (word, auto_reason(facts, narrow_mm(&part), &Planned { parts, created }, lang))
    } else {
        let parts = outputs.satin_parts_auto_kept(at.0, at.1);
        let word = if facts.line {
            "running"
        } else if parts == Some(SatinParts::All) {
            "satin"
        } else {
            "tatami"
        };
        (Some(vocabulary::sewn_as(word, None)), auto_reason(facts, outputs.auto_narrow_mm(at.0, at.1), &Planned { parts, created }, lang))
    };
    // With a Satin kept where it isn't offered, the reason under the choices says why it isn't sewn
    // as satin: the Auto line's reason, where that reason starts with it, isn't said twice ("Too
    // short for satin", then "Too short for satin: sewn as tatami").
    let kept = if stitch.as_deref() == Some("satin") { kept_satin_reason(facts, offer, lang) } else { None };
    let why = why.filter(|w| known && !kept.as_ref().is_some_and(|k| k.starts_with(w.as_str())));
    if let Some(auto) = auto.filter(|_| known) {
        let colour = if stitch.is_none() { t.text } else { t.text_dim };
        ui.label(RichText::new(vocabulary::auto(&auto)).color(colour));
    }
    if let Some(why) = &why {
        ui.label(RichText::new(why.as_str()).small().color(t.text_dim));
    }
    ui.add_space(GAP);
    // At the most changes the engine takes, a stitch or an angle for an area no change applies to
    // yet would be one more: greyed out, saying why.
    let held = fields.get(CHANGES).and_then(Value::as_array).map_or(0, Vec::len);
    let full = held >= MAX_AREA_CHANGES && applying_to(fields, outputs, key).is_empty();
    let most = numbers::count(lang, u64::try_from(MAX_AREA_CHANGES).unwrap_or(u64::MAX));
    let too_many = fill(crate::et!("That would be more than {count} area changes, the most a design takes"), &[("count", &most)]);
    // The choices by the area's kind: Satin only where it sews satin.
    let ids = if facts.line { LINE_CHOICES } else { FILL_CHOICES };
    let labels: Vec<String> = ids.iter().map(|id| vocabulary::stitch(id, facts.line)).collect();
    let shown = stitch.as_deref().unwrap_or("auto");
    let items: Vec<crate::panels::embroidery::Segment<'_>> = ids
        .iter()
        .zip(&labels)
        .map(|(id, label)| crate::panels::embroidery::Segment {
            id: ui.scope_id().with(("digitize-inspector-stitch", *id)),
            label,
            full: label,
            selected: *id == shown,
            enabled: *id == shown || (!full && (*id != "satin" || offer == SatinOffer::Fits)),
        })
        .collect();
    if let Some(k) = crate::panels::embroidery::segments(ui, CHOICES_HEIGHT, CHOICES_TEXT, &items) {
        let chosen = ids.get(k).copied().filter(|id| *id != "auto");
        set(fields, outputs, key, chosen, angle);
    }
    if full {
        ui.label(RichText::new(too_many.as_str()).small().color(t.text_dim));
    }
    // Why Satin isn't offered: with Satin kept where it isn't (a live object made before, or an
    // agent's), why it isn't sewn as satin there; else unless the reason above says it in the same
    // words (an area too short for satin, sewn by Auto).
    let not = if shown == "satin" { kept } else { satin_reason(facts, offer, lang).filter(|not| why.as_ref() != Some(not)) };
    if let Some(not) = not {
        ui.label(RichText::new(not).small().color(t.text_dim));
    }
    // A filled area's rows, their angle greyed out where no tatami rows are sewn: every part a
    // satin column (as planned), or the area left out; offered until that answer is in.
    if !facts.line {
        let rows = shown != "none" && parts != Some(SatinParts::All);
        ui.add_enabled_ui(!full, |ui| angle_row(ui, fields, outputs, key, stitch.as_deref(), angle, &part, rows, lang));
    }
    // Its stitches as planned.
    if let Some(n) = part["sewing"]["stitches"].as_u64() {
        ui.add_space(GAP);
        ui.label(RichText::new(crate::etn!(n, "about {count} stitch", "about {count} stitches")).small().color(t.text_dim));
    }
    // The areas like it, and its place in the sewing order.
    ui.add_space(GAP);
    // The stitch and angle as the choices above may have just set them (a typed angle is taken as
    // the pointer leaves its field, in this frame).
    let (stitch, angle) = current(fields, outputs, key);
    let of = (
        outputs.number(DigitizeStage::Shapes),
        outputs.number(DigitizeStage::Plan),
        answers.answered(),
        at,
        stitch.clone(),
        angle.map(f64::to_bits),
        fields.get(CHANGES).cloned(),
    );
    if open.similar.as_ref().is_none_or(|s| s.of != of) {
        open.similar = Some(similar_of(fields, outputs, answers, at, stitch.as_deref(), angle, of));
    }
    let similar = open.similar.clone();
    open.similar_shown = false;
    // Its row only where there are alike areas: an empty one would take a row's height.
    if let Some(similar) = similar.filter(|s| !s.keys.is_empty()) {
        ui.horizontal_wrapped(|ui| {
            let count = |keys: &[AreaKey]| u64::try_from(keys.len()).unwrap_or(u64::MAX);
            // Once every alike area is sewn as this one, greyed out where it was, saying so.
            let applied = similar.changing.is_empty();
            let words = if applied {
                crate::etn!(count(&similar.keys), "Applied to {count} Similar Area", "Applied to {count} Similar Areas")
            } else {
                crate::etn!(count(&similar.changing), "Apply to {count} Similar Area", "Apply to {count} Similar Areas")
            };
            let tip = crate::et!("Sets this area's stitch and angle on the areas of its color and kind about its size and proportions.");
            // No more changes than the engine takes, and with Satin, once whether it is offered on
            // each is known.
            let enabled = (stitch.is_some() || angle.is_some()) && similar.room && similar.complete && !applied;
            let resp = ui.add_enabled(enabled, egui::Button::new(words)).on_hover_text(tip);
            let resp = if similar.room { resp } else { resp.on_disabled_hover_text(too_many.as_str()) };
            // Outlined while the pointer is on it, greyed out too: which areas are alike.
            open.similar_shown = resp.contains_pointer();
            if resp.clicked() {
                set_many(fields, outputs, &similar.changing, stitch.as_deref(), angle);
            }
        });
    }
    // Show in Sewing Order: a checkbox on its own row, greyed out on an area left out, which has no
    // place in the order (its tick kept for when it is sewn again).
    let tip = if facts.sewn {
        crate::et!("Dims what is sewn after this area.")
    } else {
        crate::et!("This area isn't sewn, so it has no place in the sewing order")
    };
    let words = crate::et!("Show in Sewing Order");
    let order = ui.add_enabled_ui(facts.sewn, |ui| crate::embroidery::rows::check_response(ui, words, Some(open.order && facts.sewn), t.panel)).inner;
    crate::embroidery::rows::described(ui, order.id, tip);
    let order = if facts.sewn { order.on_hover_text(tip) } else { order.on_disabled_hover_text(tip) };
    if order.clicked() {
        open.order = !open.order;
    }
    // Greyed out, why said under it, as the inspector's other greyed controls say it.
    if !facts.sewn {
        ui.label(RichText::new(tip).small().color(t.text_dim));
    }
    if open.order && facts.sewn {
        place_row(ui, outputs, open, at, lang);
    }
}

/// Where the area is sewn, in words ("Sewn 2 of 5, after Blue Ink 1, before Orange 1"), with ◀ ▶
/// moving the inspector to the area sewn before or after it.
fn place_row(ui: &mut Ui, outputs: &DigitizeOutputs, open: &mut Inspected, at: (u32, u32), lang: crate::i18n::Lang) {
    let t = Tokens::get(ui.ctx());
    let Some(place) = outputs.sewing_place(at.0, at.1) else { return };
    let count = |n: usize| numbers::count(lang, u64::try_from(n).unwrap_or(u64::MAX));
    let (number, of) = (count(place.number), count(place.of));
    let before = place.before.as_ref().map(|(f, _)| super::preview::area_name(f));
    let after = place.after.as_ref().map(|(f, _)| super::preview::area_name(f));
    let words = match (&before, &after) {
        (Some(b), Some(a)) => fill(
            crate::et!("Sewn {number} of {of}, after {before}, before {after}"),
            &[("number", &number), ("of", &of), ("before", b), ("after", a)],
        ),
        (None, Some(a)) => fill(crate::et!("Sewn {number} of {of}, before {after}"), &[("number", &number), ("of", &of), ("after", a)]),
        (Some(b), None) => fill(crate::et!("Sewn {number} of {of}, after {before}"), &[("number", &number), ("of", &of), ("before", b)]),
        (None, None) => fill(crate::et!("Sewn {number} of {of}"), &[("number", &number), ("of", &of)]),
    };
    let mut to = None;
    ui.horizontal_wrapped(|ui| {
        for (glyph, name, next) in [
            ("◀", crate::et!("Previous area in sewing order"), place.before.as_ref().map(|(_, k)| k)),
            ("▶", crate::et!("Next area in sewing order"), place.after.as_ref().map(|(_, k)| k)),
        ] {
            let resp = ui.add_enabled(next.is_some(), egui::Button::new(glyph)).on_hover_text(name);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, next.is_some(), name));
            if resp.clicked() {
                to = next.cloned();
            }
        }
        ui.label(RichText::new(words).small().color(t.text_dim));
    });
    if let Some(k) = to {
        open.key = k;
    }
}

/// The angle of a filled area's rows: Auto's (the document's, shown with where it comes from) or
/// one of its own, typed, with ↺ back to Auto. Where no tatami rows are sewn (`rows` false: every
/// part a satin column, or the area left out) its field is greyed out, saying why, ↺ still taking
/// back an angle kept.
#[allow(clippy::too_many_arguments)]
fn angle_row(
    ui: &mut Ui,
    fields: &mut Map<String, Value>,
    outputs: &DigitizeOutputs,
    key: &AreaKey,
    stitch: Option<&str>,
    angle: Option<f64>,
    part: &Value,
    rows: bool,
    lang: crate::i18n::Lang,
) {
    let auto = part["settings"]["angle"]["value"].as_f64();
    let format = fields::format(lang, "°", ANGLE_DECIMALS);
    // The degrees an object's angle takes, as `embroidery.set` takes them.
    let range = match vectorcraft_engine::cmd::embroidery::setting_field("angle") {
        Some(vectorcraft_engine::cmd::embroidery::SettingField::Number { min, max, .. }) => (min, max),
        _ => return,
    };
    let mut taken = None;
    let mut back = false;
    ui.horizontal(|ui| {
        crate::widgets::field_label(ui, crate::et!("Angle"));
        taken = ui
            .add_enabled_ui(rows, |ui| {
                fields::number_field(
                    ui,
                    ("digitize-inspector", "angle"),
                    angle.or(auto),
                    &format,
                    &fields::Look::default(),
                    super::colours_step::FIELD_WIDTH,
                    &(range.0..=range.1),
                )
            })
            .inner;
        if angle.is_some() {
            back = crate::embroidery::rows::restore(ui, crate::et!("Back to Auto"), crate::et!("Back to Auto")).clicked();
        } else {
            ui.label(RichText::new(crate::et!("Auto")).small().color(Tokens::get(ui.ctx()).text_dim));
        }
    });
    if !rows {
        ui.label(RichText::new(crate::et!("The angle turns tatami rows: this area has none")).small().color(Tokens::get(ui.ctx()).text_dim));
    }
    if let Some(v) = taken.filter(|v| v.is_finite()) {
        set(fields, outputs, key, stitch, Some(v));
    } else if back {
        set(fields, outputs, key, stitch, None);
    }
}

/// What the planner sews of a filled area, as far as its answers are in: as planned (`parts`), and
/// at the width Create would give it with Satin chosen (`created`).
struct Planned {
    parts: Option<SatinParts>,
    created: Option<SatinParts>,
}

/// Why Auto sews the area as it does: a line's, or how the planner sews a filled area at `narrow`,
/// the width up to which Auto sews its parts as satin, from `planned` (as Auto would make it). Wider
/// or not is of the area as it is made: grown by the overlap under the areas sewn after it, an area
/// no wider as traced has no satin column at that width but one at Create's. `None` with narrow
/// parts as satin off, or every part a satin column though one is wider (a column runs on through a
/// short stretch wider than the width), rather than a reason that isn't true. Built from these, never
/// from the engine's words.
fn auto_reason(facts: &AreaFacts, narrow: Option<f64>, planned: &Planned, lang: crate::i18n::Lang) -> Option<String> {
    if facts.line {
        return Some(crate::et!("Sewn along its middle as one running line").to_string());
    }
    let narrow = narrow?;
    let width = numbers::decimal(lang, narrow, MM_DECIMALS);
    let wider = facts.widest_mm > narrow;
    let columns_created = planned.created.is_some_and(|c| c != SatinParts::None);
    match planned.parts? {
        SatinParts::All => (!wider).then(|| fill(crate::et!("No part wider than {width} mm"), &[("width", &width)])),
        SatinParts::Some if wider => Some(fill(crate::et!("Wider than {width} mm: tatami, its narrower parts as satin"), &[("width", &width)])),
        SatinParts::Some => Some(crate::et!("Its long parts as satin, the rest tatami").to_string()),
        SatinParts::None if wider || columns_created => Some(fill(crate::et!("Wider than {width} mm: tatami"), &[("width", &width)])),
        SatinParts::None => Some(crate::et!("Too short for satin").to_string()),
    }
}

/// The width up to which a filled area's parts are sewn as satin, as `embroidery.info` resolves it
/// for the area's object (`part`, its fill); `None` when narrow parts aren't sewn as satin.
fn narrow_mm(part: &Value) -> Option<f64> {
    part["settings"]["narrowAsSatin"]["value"].as_f64().filter(|n| *n > 0.0)
}

/// Why a Satin kept where Satin isn't offered (`offer`: a live object made before, or an agent's,
/// as the command takes it anywhere) isn't sewn as satin: too short, sewn as tatami; too wide; too
/// narrow a line. `None` where it is offered.
fn kept_satin_reason(facts: &AreaFacts, offer: SatinOffer, lang: crate::i18n::Lang) -> Option<String> {
    match offer {
        SatinOffer::TooShort if !facts.line => Some(crate::et!("Too short for satin: sewn as tatami").to_string()),
        SatinOffer::TooWide { .. } | SatinOffer::TooNarrow => satin_reason(facts, offer, lang),
        _ => None,
    }
}

/// Why Satin isn't offered on the area (`offer`): too wide a filled area, too narrow a line, or a
/// filled area too short for a satin column (a dot, a small square); `None` where it is offered. A
/// reason, said with no full stop, as the inspector's other reasons are.
fn satin_reason(facts: &AreaFacts, offer: SatinOffer, lang: crate::i18n::Lang) -> Option<String> {
    let mm = |v: f64| numbers::decimal(lang, v, MM_DECIMALS);
    Some(match offer {
        SatinOffer::Fits => return None,
        SatinOffer::TooNarrow => fill(
            crate::et!("Satin needs a line at least {least} mm wide: this one is {width} mm"),
            &[("least", &mm(SATIN_LINE_LEAST_MM)), ("width", &mm(facts.line_width_mm.unwrap_or_default()))],
        ),
        // The width it takes said, not a second width of the area's (its widest, inscribed, beside
        // its box's sides in the facts above): wider as it is, or with the part it grows by under
        // its neighbours.
        SatinOffer::TooWide { overlap: false, .. } => {
            fill(crate::et!("Satin sews parts up to {most} mm wide: this area is wider"), &[("most", &mm(SATIN_AREA_MOST_MM))])
        }
        SatinOffer::TooWide { overlap: true, .. } => fill(
            crate::et!("Satin sews parts up to {most} mm wide: with the part under its neighbors, this area is wider"),
            &[("most", &mm(SATIN_AREA_MOST_MM))],
        ),
        SatinOffer::TooShort => crate::et!("Too short for satin").to_string(),
    })
}

/// `rgb` written `#rrggbb`, as the engine takes a colour.
fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The colour `s` writes as `#rrggbb`.
fn rgb(s: &str) -> Option<[u8; 3]> {
    let digits = s.strip_prefix('#').filter(|d| d.len() == 6)?;
    let byte = |k: usize| digits.get(k..k + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The changes `fields` hold, each as the engine takes it.
fn changes(fields: &Map<String, Value>) -> Vec<Map<String, Value>> {
    fields.get(CHANGES).and_then(Value::as_array).map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect()).unwrap_or_default()
}

/// The key change `c` keeps its area by.
fn key_of(c: &Map<String, Value>) -> Option<AreaKey> {
    let at = c.get("at")?.as_array()?;
    let color = c.get("color").and_then(Value::as_str).and_then(rgb)?;
    let line = c.get("kind").and_then(Value::as_str)? == "line";
    Some(AreaKey { at: [at.first()?.as_f64()?, at.get(1)?.as_f64()?], color, line })
}

/// The changes of `fields` that apply to the area `key` finds in `outputs`, in their order.
fn applying_to(fields: &Map<String, Value>, outputs: &DigitizeOutputs, key: &AreaKey) -> Vec<Map<String, Value>> {
    let here = outputs.find(key);
    changes(fields).into_iter().filter(|c| key_of(c).is_some_and(|k| &k == key || (here.is_some() && outputs.find(&k) == here))).collect()
}

/// The key of the last change of `fields` that applies to the area `key` finds.
fn applying(fields: &Map<String, Value>, outputs: &DigitizeOutputs, key: &AreaKey) -> Option<AreaKey> {
    applying_to(fields, outputs, key).last().and_then(key_of)
}

/// The area's stitch (`None`: Auto) and its rows' angle (`None`: Auto), as the changes that apply
/// to it set them, a later one winning key by key.
fn current(fields: &Map<String, Value>, outputs: &DigitizeOutputs, key: &AreaKey) -> (Option<String>, Option<f64>) {
    applying_to(fields, outputs, key).iter().fold((None, None), |(stitch, angle), c| {
        (c.get("stitch").and_then(Value::as_str).map(str::to_string).or(stitch), c.get("angle").and_then(Value::as_f64).or(angle))
    })
}

/// The areas Apply to Similar Areas sets for the area under working pixel `at` with `stitch` and
/// `angle` (for Satin, only those Satin is offered on, as their own inspectors offer it), and
/// whether the engine takes the changes that makes, worked out of `of`.
fn similar_of(
    fields: &Map<String, Value>,
    outputs: &DigitizeOutputs,
    answers: &mut Answers,
    at: (u32, u32),
    stitch: Option<&str>,
    angle: Option<f64>,
    of: SimilarOf,
) -> Similar {
    let alike = outputs.similar(at.0, at.1);
    let mut complete = true;
    let keys = if stitch == Some("satin") {
        // With Satin, only the areas Satin is offered on: their answers asked for (the inspector's
        // own area first), and the list complete once every one is in.
        let places: Vec<Option<(u32, u32)>> = alike.iter().map(|k| outputs.find(k)).collect();
        let asked: Vec<(u32, u32)> = std::iter::once(at).chain(places.iter().flatten().copied()).collect();
        answers.ask(outputs, &asked);
        let mut offered = |p: &Option<(u32, u32)>| match p.map(|(x, y)| outputs.satin_offer_kept(x, y)) {
            Some(Some(offer)) => offer == SatinOffer::Fits,
            Some(None) => {
                complete = false;
                false
            }
            None => false,
        };
        alike.into_iter().zip(&places).filter(|(_, p)| offered(p)).map(|(k, _)| k).collect()
    } else {
        alike
    };
    // Those a press would change: not sewn as this area already, each as the changes applying to
    // it set it (worked out once for every change, not once a key); on Auto, every one.
    let changing = if stitch.is_none() && angle.is_none() {
        keys.clone()
    } else {
        let mut set: std::collections::HashMap<(u32, u32), (Option<String>, Option<f64>)> = std::collections::HashMap::new();
        for c in changes(fields) {
            let Some(place) = key_of(&c).and_then(|k| outputs.find(&k)) else { continue };
            let held = set.entry(place).or_default();
            if let Some(s) = c.get("stitch").and_then(Value::as_str) {
                held.0 = Some(s.to_string());
            }
            if let Some(a) = c.get("angle").and_then(Value::as_f64) {
                held.1 = Some(a);
            }
        }
        let wanted = (stitch.map(str::to_string), angle);
        keys.iter().filter(|k| outputs.find(k).and_then(|p| set.get(&p)).cloned().unwrap_or_default() != wanted).cloned().collect()
    };
    // Past the most by themselves: not counted change by change.
    let room = changing.len() <= MAX_AREA_CHANGES && set_many(&mut fields.clone(), outputs, &changing, stitch, angle) <= MAX_AREA_CHANGES;
    Similar { of, keys, changing, room, complete }
}

/// Each of `keys`' areas' stitch and angle set at once (Apply to Similar Areas): every change that
/// applies to any of them replaced by one under each key, or by none when it sets neither, each
/// area found once → how many changes `fields` then hold.
fn set_many(fields: &mut Map<String, Value>, outputs: &DigitizeOutputs, keys: &[AreaKey], stitch: Option<&str>, angle: Option<f64>) -> usize {
    let places: std::collections::HashSet<(u32, u32)> = keys.iter().filter_map(|k| outputs.find(k)).collect();
    let applies = |c: &Map<String, Value>| key_of(c).is_some_and(|k| keys.contains(&k) || outputs.find(&k).is_some_and(|p| places.contains(&p)));
    let mut all: Vec<Value> = changes(fields).into_iter().filter(|c| !applies(c)).map(Value::Object).collect();
    if stitch.is_some() || angle.is_some() {
        all.extend(keys.iter().map(|k| Value::Object(change(k, stitch, angle))));
    }
    let n = all.len();
    if all.is_empty() {
        fields.remove(CHANGES);
    } else {
        fields.insert(CHANGES.into(), Value::Array(all));
    }
    n
}

/// A change under `key` setting `stitch` and `angle`, as the engine takes it.
fn change(key: &AreaKey, stitch: Option<&str>, angle: Option<f64>) -> Map<String, Value> {
    let mut c = Map::new();
    c.insert("at".into(), json!(key.at));
    c.insert("color".into(), json!(hex(key.color)));
    c.insert("kind".into(), json!(if key.line { "line" } else { "fill" }));
    if let Some(s) = stitch {
        c.insert("stitch".into(), json!(s));
    }
    if let Some(a) = angle {
        c.insert("angle".into(), json!(a));
    }
    c
}

/// The area's stitch and angle set: every change that applies to it replaced by one under `key`, or
/// by none when it sets neither.
pub(crate) fn set(fields: &mut Map<String, Value>, outputs: &DigitizeOutputs, key: &AreaKey, stitch: Option<&str>, angle: Option<f64>) {
    let mine = applying_to(fields, outputs, key);
    let mut all: Vec<Value> = changes(fields).into_iter().filter(|c| !mine.contains(c)).map(Value::Object).collect();
    if stitch.is_some() || angle.is_some() {
        all.push(Value::Object(change(key, stitch, angle)));
    }
    if all.is_empty() {
        fields.remove(CHANGES);
    } else {
        fields.insert(CHANGES.into(), Value::Array(all));
    }
}
