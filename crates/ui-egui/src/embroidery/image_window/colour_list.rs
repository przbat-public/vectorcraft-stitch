//! The image window's colour list: each colour of the image, as the reply gives it, with the thread
//! it is sewn in (note 09 §3.5).
//!
//! - **A row:** the image's colour as a square → the thread as a circle (the shape tells which is
//!   which), the thread's name and number, its share of the frame and its areas, and a ΔE badge on
//!   every row. A thread the reply flags as unlike the colour (never a line of the window's own)
//!   gets the warning triangle, amber words "unlike the image's color" and, when it is the nearest
//!   thread, "no closer thread in {palette}": the colour is the last of four cues.
//! - **The background's row:** its share of the frame, not sewn (step 1 chooses it).
//! - **Selection:** a click selects a row, Shift a range, Cmd/Ctrl adds or takes one away; ↑/↓ move
//!   it (with Shift, extend it), Space adds or takes away the row with the keyboard's focus.
//! - **Actions** on the selection: Merge (two or more: sewn as one, in the largest's thread), Don't
//!   Sew and Sew, and Choose Thread… (one: a list of the palette's threads, nearest first, searched
//!   by name or number); each disabled with its reason. ↺ on a changed row gives it back to Auto.
//! - Every choice is a colour choice of the engine's (`colorChoices`): the window recomputes from
//!   the colours on. A screen reader hears a row whole.

use std::collections::BTreeSet;

use egui::scroll_area::ScrollBarVisibility;
use egui::{Align2, Color32, FontId, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Map, Value, json};
use vectorcraft_engine::cmd::embroidery::NearThread;

use crate::VectorcraftApp;
use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::embroidery::{rows, vocabulary};
use crate::i18n::Lang;
use crate::state::Dialog;
use crate::theme::Tokens;

#[cfg(test)]
mod tests;

/// The field of the colours' choices (the engine's `colorChoices`).
pub(crate) const CHOICES: &str = "colorChoices";

/// What a choice sets besides its colour: its thread, the colour it is merged into, and what it is
/// sewn as (`none`: not sewn).
const THREAD: &str = "thread";
const MERGE_INTO: &str = "mergeInto";
const JOB: &str = "job";
const NOT_SEWN: &str = "none";

/// The reasons a thread was given that make it the nearest the palette has.
const NEAREST_REASONS: [&str; 3] = ["own", "nearest", "shared"];

/// A row's height, its colour square's and thread circle's size, the gap between its parts, its
/// badge's padding and corners, and the most room the list takes before it scrolls (points):
/// estimates from the mockups' list.
const ROW_HEIGHT: f32 = 38.0;
const SWATCH: f32 = 14.0;
const GAP: f32 = 6.0;
const PAD: f32 = 6.0;
const BADGE_PAD: egui::Vec2 = vec2(3.0, 1.0);
const BADGE_RADIUS: f32 = 3.0;
const LIST_MAX_HEIGHT: f32 = 260.0;
const MARK_SIZE: f32 = 12.0;

/// The size of a row's name and of its other words (points).
const NAME_TEXT: f32 = 12.5;
const SMALL_TEXT: f32 = 11.5;

/// The picker's width, a thread's row in its list, and its list's most height (points).
const PICKER_WIDTH: f32 = 280.0;
const PICKER_ROW_HEIGHT: f32 = ROW_HEIGHT * 0.7;
const PICKER_LIST_HEIGHT: f32 = 300.0;

/// ΔE shown to one decimal, two where one would round onto the line the reply flags past (10), so
/// that a badge never reads as the line itself.
const DELTA_E_LINE: f64 = 10.0;
const DELTA_E_NEAR_LINE: f64 = 0.05;

/// A row of the list, as the reply gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    /// The image's colour, "#rrggbb": what a choice names.
    pub(crate) color: String,
    pub(crate) background: bool,
    /// The background's colour where the design encloses it: a row of its own, apart from the
    /// background's (the engine keeps an area of that colour touching the image's edge as the
    /// background, and sews one it doesn't).
    pub(crate) inside: bool,
    pub(crate) sewn: bool,
    pub(crate) thread: Option<Thread>,
    /// Why it has its thread ("nearest", "chosen", "merged"…).
    pub(crate) reason: Option<String>,
    /// A person changed it (a choice applies to it).
    pub(crate) changed: bool,
    pub(crate) merged: bool,
    /// Its share of the frame, percent, and its areas.
    pub(crate) share: f64,
    pub(crate) areas: u64,
}

/// A row's thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Thread {
    pub(crate) palette: String,
    pub(crate) index: u64,
    pub(crate) name: String,
    pub(crate) color: String,
    pub(crate) delta_e: f64,
    pub(crate) mismatch: bool,
}

impl Row {
    /// Whether its thread is the nearest the palette has, so that none is closer.
    fn nearest(&self) -> bool {
        self.reason.as_deref().is_some_and(|r| NEAREST_REASONS.contains(&r))
    }
}

/// The list's rows in `reply` (`colors`), the background's first, the background's colour where
/// the design encloses it marked.
pub(crate) fn rows_of(reply: &Value) -> Vec<Row> {
    let mut rows: Vec<Row> = reply["colors"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Row {
                    color: c["color"].as_str().unwrap_or_default().to_string(),
                    background: c["background"].as_bool().unwrap_or(false),
                    inside: false,
                    sewn: c["sewn"].as_bool().unwrap_or(false),
                    thread: c[THREAD].is_object().then(|| Thread {
                        palette: c[THREAD]["palette"].as_str().unwrap_or_default().to_string(),
                        index: c[THREAD]["index"].as_u64().unwrap_or(0),
                        name: c[THREAD]["name"].as_str().unwrap_or_default().to_string(),
                        color: c[THREAD]["color"].as_str().unwrap_or_default().to_string(),
                        delta_e: c[THREAD]["deltaE"].as_f64().unwrap_or(0.0),
                        mismatch: c[THREAD]["mismatch"].as_bool().unwrap_or(false),
                    }),
                    reason: c["reason"].as_str().map(str::to_string),
                    changed: !c["choice"].is_null(),
                    merged: !c["mergedInto"].is_null(),
                    share: c["share"].as_f64().unwrap_or(0.0),
                    areas: c["areaCount"].as_u64().unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    let background: Vec<String> = rows.iter().filter(|r| r.background).map(|r| r.color.to_ascii_lowercase()).collect();
    for r in rows.iter_mut().filter(|r| !r.background) {
        r.inside = background.contains(&r.color.to_ascii_lowercase());
    }
    rows.sort_by_key(|r| !r.background);
    rows
}

/// The list's own state: the colours selected, where a range starts, and the thread choice open.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) selected: BTreeSet<String>,
    anchor: Option<String>,
    /// Choose Thread… while its list is open.
    pub(crate) picker: Option<Picker>,
    /// The row to take the keyboard's focus next frame.
    focus: Option<String>,
}

/// Choose Thread…'s list while it is open: the colour whose thread is chosen, the search typed, and
/// whether it opened this frame (its search field takes the keyboard's focus then).
#[derive(Clone, Debug, Default)]
pub(crate) struct Picker {
    pub(crate) color: String,
    pub(crate) search: String,
    opening: bool,
}

impl State {
    /// The colours selected that `rows` still list (not the background's).
    pub(crate) fn chosen<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        rows.iter().filter(|r| !r.background && self.selected.contains(&r.color)).collect()
    }
}

/// A click on row `k` of `rows` with `modifiers` held: it alone, a range from the last clicked
/// (Shift), or it added or taken away (Cmd/Ctrl).
pub(crate) fn click(state: &mut State, rows: &[Row], k: usize, modifiers: Modifiers) {
    let Some(row) = rows.get(k).filter(|r| !r.background) else { return };
    if modifiers.shift
        && let Some(a) = state.anchor.as_ref().and_then(|a| rows.iter().position(|r| &r.color == a))
    {
        let (lo, hi) = (a.min(k), a.max(k));
        if !modifiers.command {
            state.selected.clear();
        }
        state.selected.extend(rows.iter().skip(lo).take(hi - lo + 1).filter(|r| !r.background).map(|r| r.color.clone()));
        return;
    }
    if modifiers.command {
        if !state.selected.remove(&row.color) {
            state.selected.insert(row.color.clone());
        }
    } else {
        state.selected = BTreeSet::from([row.color.clone()]);
    }
    state.anchor = Some(row.color.clone());
}

/// ↑ (`down` false) or ↓ from row `from`: the next row selected alone, or, with Shift, the range
/// from the last clicked to it → the row moved to.
pub(crate) fn step(state: &mut State, rows: &[Row], from: usize, down: bool, shift: bool) -> Option<usize> {
    let selectable: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| !r.background).map(|(k, _)| k).collect();
    let at = selectable.iter().position(|&k| k == from)?;
    let next = if down { selectable.get(at + 1) } else { at.checked_sub(1).and_then(|a| selectable.get(a)) };
    let &next = next?;
    if shift {
        if state.anchor.is_none() {
            state.anchor = rows.get(from).map(|r| r.color.clone());
        }
        click(state, rows, next, Modifiers::SHIFT);
    } else {
        click(state, rows, next, Modifiers::NONE);
    }
    Some(next)
}

/// Space on row `k`: added to the selection or taken away.
pub(crate) fn toggle(state: &mut State, rows: &[Row], k: usize) {
    click(state, rows, k, Modifiers::COMMAND);
}

/// Merge with `chosen` selected: possible with two or more, else why not.
pub(crate) fn merge_refused(chosen: &[&Row]) -> Option<&'static str> {
    (chosen.len() < 2).then(|| crate::et!("Select two or more colors to merge them."))
}

/// Don't Sew and Sew with `chosen` selected: possible with one or more, else why not.
pub(crate) fn sew_refused(chosen: &[&Row]) -> Option<&'static str> {
    chosen.is_empty().then(|| crate::et!("Select one or more colors first."))
}

/// Choose Thread… with `chosen` selected: possible with one, else why not.
pub(crate) fn choose_refused(chosen: &[&Row]) -> Option<&'static str> {
    (chosen.len() != 1).then(|| crate::et!("Select one color to choose its thread."))
}

/// The choices in fields `f`.
fn choices(f: &Map<String, Value>) -> Vec<Map<String, Value>> {
    f.get(CHOICES).and_then(Value::as_array).map(|a| a.iter().filter_map(|c| c.as_object().cloned()).collect()).unwrap_or_default()
}

/// Colour `color`'s choice in fields `f` changed by `change`: added when it has none, and left out
/// when the change leaves it nothing but its colour.
fn change_choice(f: &mut Map<String, Value>, color: &str, change: impl FnOnce(&mut Map<String, Value>)) {
    let mut all = choices(f);
    let same = |c: &Map<String, Value>| c.get("color").and_then(Value::as_str).is_some_and(|c| c.eq_ignore_ascii_case(color));
    let mut choice = all.iter().find(|c| same(c)).cloned().unwrap_or_else(|| Map::from_iter([("color".to_string(), json!(color))]));
    all.retain(|c| !same(c));
    change(&mut choice);
    if choice.len() > 1 {
        all.push(choice);
    }
    set_choices(f, all);
}

fn set_choices(f: &mut Map<String, Value>, all: Vec<Map<String, Value>>) {
    if all.is_empty() {
        f.remove(CHOICES);
    } else {
        f.insert(CHOICES.into(), Value::Array(all.into_iter().map(Value::Object).collect()));
    }
}

/// Colour `color` sewn in thread `index` of palette `palette` (a merge into another undone).
pub(crate) fn choose_thread(f: &mut Map<String, Value>, color: &str, palette: &str, index: usize) {
    change_choice(f, color, |c| {
        c.remove(MERGE_INTO);
        c.insert(THREAD.into(), json!({"palette": palette, "index": index}));
    });
}

/// `chosen` merged: sewn as one, in the thread and as the job of the one with the most of the frame.
pub(crate) fn merge(f: &mut Map<String, Value>, chosen: &[&Row]) {
    let Some(into) = chosen.iter().max_by(|a, b| a.share.total_cmp(&b.share)).map(|r| r.color.clone()) else { return };
    for row in chosen.iter().filter(|r| r.color != into) {
        change_choice(f, &row.color, |c| {
            c.remove(THREAD);
            c.remove(JOB);
            c.insert(MERGE_INTO.into(), json!(into));
        });
    }
}

/// `colors` sewn (`sewn`), as Auto chooses, or not sewn; a merge undone, since a colour merged into
/// another is sewn as that one is.
pub(crate) fn set_sewn(f: &mut Map<String, Value>, colors: &[String], sewn: bool) {
    for color in colors {
        change_choice(f, color, |c| {
            c.remove(MERGE_INTO);
            if sewn {
                c.remove(JOB);
            } else {
                c.insert(JOB.into(), json!(NOT_SEWN));
            }
        });
    }
}

/// Colour `color` back to Auto: its choice left out.
pub(crate) fn reset(f: &mut Map<String, Value>, color: &str) {
    change_choice(f, color, Map::clear);
}

/// Every thread chosen left out, merges and jobs kept: the threads were of another palette.
pub(crate) fn drop_threads(f: &mut Map<String, Value>) {
    let all: Vec<Map<String, Value>> = choices(f)
        .into_iter()
        .map(|mut c| {
            c.remove(THREAD);
            c
        })
        .filter(|c| c.len() > 1)
        .collect();
    set_choices(f, all);
}

/// The threads Choose Thread… lists for colour `color` in palette `palette`: nearest first, those
/// whose name holds `search` (any case) or whose number starts with it.
pub(crate) fn picker_rows(palette: &str, color: &str, search: &str) -> Vec<NearThread> {
    let Some(rgb) = super::image_step::parse(color).map(|c| [c.r(), c.g(), c.b()]) else { return Vec::new() };
    let wanted = search.trim().to_lowercase();
    vectorcraft_engine::cmd::embroidery::threads_nearest(palette, rgb)
        .into_iter()
        .filter(|t| wanted.is_empty() || t.name.to_lowercase().contains(&wanted) || t.index.to_string().starts_with(&wanted))
        .collect()
}

/// A difference as a badge writes it: one decimal, two where one would round onto the line.
pub(crate) fn delta_e_words(lang: Lang, delta_e: f64) -> String {
    let places = if (delta_e - DELTA_E_LINE).abs() < DELTA_E_NEAR_LINE && delta_e != DELTA_E_LINE { 2 } else { 1 };
    format!("ΔE {}", numbers::fixed(lang, delta_e, places))
}

/// A share of the frame as a row writes it: whole percent.
fn share_words(lang: Lang, share: f64) -> String {
    numbers::percent(lang, share.max(0.0).round() as u64)
}

/// A thread's name and number as the rows write them ("Dark Fuchsia  PEC 7").
fn thread_name(t: &Thread) -> String {
    format!("{}  {} {}", t.name, t.palette.to_uppercase(), t.index)
}

/// What a row says after its thread: its share and areas, and what is set or found about it.
fn row_facts(row: &Row, lang: Lang) -> Vec<String> {
    let mut facts = vec![fill(crate::et!("{share} of the frame"), &[("share", &share_words(lang, row.share))])];
    facts.push(crate::etn!(row.areas, "{count} area", "{count} areas"));
    if !row.sewn {
        facts.push(crate::et!("not sewn").to_string());
    }
    if row.merged {
        facts.push(crate::et!("merged").to_string());
    }
    if row.inside {
        facts.push(crate::et!("the background's color inside the design").to_string());
    }
    facts
}

/// What a row's mismatch says, amber: unlike the colour, and that no thread is closer.
fn mismatch_words(row: &Row) -> Vec<String> {
    let Some(t) = row.thread.as_ref().filter(|t| t.mismatch) else { return Vec::new() };
    let mut words = vec![crate::et!("unlike the image's color").to_string()];
    if row.nearest() {
        words.push(fill(crate::et!("no closer thread in {palette}"), &[("palette", &vocabulary::palette(&t.palette))]));
    }
    words
}

/// A row whole, as a screen reader hears it.
pub(crate) fn row_words(row: &Row, lang: Lang) -> String {
    let mut parts = Vec::new();
    if row.background {
        parts.push(crate::et!("Background").to_string());
    } else if let Some(t) = &row.thread {
        parts.push(fill(crate::et!("{color} in {thread}"), &[("color", &row.color), ("thread", &thread_name(t))]));
        parts.push(delta_e_words(lang, t.delta_e));
    } else {
        parts.push(row.color.clone());
    }
    parts.extend(mismatch_words(row));
    parts.extend(row_facts(row, lang));
    parts.join(", ")
}

/// The list in `ui`, for dialog `d`, of the result's `reply`: its rows, then its actions.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let lang = crate::i18n::current();
    let rows = rows_of(reply);
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let state = &mut live.colours;
    state.selected.retain(|c| rows.iter().any(|r| &r.color == c && !r.background));
    let mut reset_one = None;
    scrolled(ui, "digitize-colour-list", rows.len(), ROW_HEIGHT, LIST_MAX_HEIGHT, |ui| {
        for (k, row) in rows.iter().enumerate() {
            let selected = state.selected.contains(&row.color);
            let drawn = draw_row(ui, row, selected, lang, state.focus.as_deref() == Some(row.color.as_str()));
            if drawn.restore {
                reset_one = Some(row.color.clone());
            }
            if drawn.focused_now {
                state.focus = None;
            }
            row_input(ui, state, &rows, k, &drawn.resp);
        }
    });
    if let Some(color) = reset_one {
        reset(&mut d.fields, &color);
    }
    actions(ui, state, &rows, d);
}

/// A scrolling list `id` of `rows` rows `row_height` points high (spaced as `ui` spaces them), at most
/// `max_height` points before it scrolls. Its bar is the Layers panel's: in a column of its own, so
/// it never covers what sits at the rows' right end (a badge, ↺, a ΔE), and shown whenever the rows
/// overflow, not only under the pointer, so that the list says it scrolls; decided before the rows
/// are laid out, so they never shift when it appears.
fn scrolled<R>(ui: &mut Ui, id: &str, rows: usize, row_height: f32, max_height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().scroll = crate::widgets::list_scroll_style();
        let height = rows as f32 * row_height + rows.saturating_sub(1) as f32 * ui.spacing().item_spacing.y;
        let bar = if height > max_height { ScrollBarVisibility::AlwaysVisible } else { ScrollBarVisibility::AlwaysHidden };
        egui::ScrollArea::vertical().id_salt(id).max_height(max_height).auto_shrink([false, true]).scroll_bar_visibility(bar).show(ui, add).inner
    })
    .inner
}

/// What drawing a row gave: its response, whether its ↺ was pressed, and whether it took the
/// keyboard's focus it was to take.
struct Drawn {
    resp: egui::Response,
    restore: bool,
    focused_now: bool,
}

/// Row `row` drawn, `selected` filled, taking the keyboard's focus when `focus`.
fn draw_row(ui: &mut Ui, row: &Row, selected: bool, lang: Lang, focus: bool) -> Drawn {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), if row.background { Sense::hover() } else { Sense::click() });
    let focused_now = focus && !row.background;
    if focused_now {
        resp.request_focus();
    }
    let p = ui.painter_at(rect);
    let back = if selected { t.row_selected } else { t.panel };
    if selected {
        p.rect_filled(rect, f32::from(t.radius), t.row_selected);
    } else if resp.hovered() && !row.background {
        p.rect_filled(rect, f32::from(t.radius), t.hover);
    }
    let ink = |c: Color32| if row.sewn || row.background { c } else { c.gamma_multiply(0.6) };
    let top = rect.top() + ROW_HEIGHT * 0.32;
    let bottom = rect.top() + ROW_HEIGHT * 0.72;
    let square = Rect::from_center_size(pos2(rect.left() + PAD + SWATCH / 2.0, top), vec2(SWATCH, SWATCH));
    let image = super::image_step::parse(&row.color).unwrap_or(t.text_dim);
    p.rect_filled(square, 2.0, image);
    p.rect_stroke(square, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let x = square.right() + GAP;
    let end = rect.right() - PAD - RESTORE_ROOM;
    let mut said = row_words(row, lang);
    let mut cut = false;
    if row.background {
        let words = fill(crate::et!("Background · {share} of the frame · not sewn"), &[("share", &share_words(lang, row.share))]);
        cut |= one_line(&p, &[(&words, t.text)], NAME_TEXT, pos2(x, top), end - x);
        said = words;
    } else if let Some(thread) = &row.thread {
        let arrow = p.text(pos2(x, top), Align2::LEFT_CENTER, "→", FontId::proportional(SMALL_TEXT), t.text_dim);
        let centre = pos2(arrow.right() + GAP + SWATCH / 2.0, top);
        p.circle_filled(centre, SWATCH / 2.0, super::image_step::parse(&thread.color).unwrap_or(t.text_dim));
        p.circle_stroke(centre, SWATCH / 2.0, Stroke::new(1.0, t.border));
        let badge_left = badge(&p, end, top, thread, lang, &t, back);
        let name_left = centre.x + SWATCH / 2.0 + GAP;
        cut |= one_line(&p, &[(&thread_name(thread), ink(t.text_strong))], NAME_TEXT, pos2(name_left, top), badge_left - GAP - name_left);
    }
    let mut restore = false;
    if !row.background {
        let facts = row_facts(row, lang).join(" · ");
        let amber = marks::text_colour(&t, Mark::Warning);
        let warned = mismatch_words(row).join(" · ");
        let mut lx = x;
        let line: Vec<(&str, Color32)> = if warned.is_empty() {
            vec![(&facts, ink(t.text_dim))]
        } else {
            marks::paint_on(&p, pos2(lx + MARK_SIZE / 2.0, bottom), MARK_SIZE, Mark::Warning, &t, back);
            lx += MARK_SIZE + GAP / 2.0;
            vec![(&warned, amber), (" · ", ink(t.text_dim)), (&facts, ink(t.text_dim))]
        };
        cut |= one_line(&p, &line, SMALL_TEXT, pos2(lx, bottom), end - lx);
        if row.changed {
            restore = restore_button(ui, rect, row);
        }
    }
    // Cut short on screen: the whole row in its tooltip (screen readers hear it whole anyway).
    let resp = if cut { resp.on_hover_text(&said) } else { resp };
    if resp.has_focus() {
        rows::focus_ring(ui, &resp, rect.shrink(1.0), back);
    }
    let said_now = said;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListBoxOption, !row.background, selected, said_now.clone()));
    Drawn { resp, restore, focused_now }
}

/// A row's ΔE badge, ending at `right`: amber, outlined, when the reply flags the thread → its left
/// edge.
fn badge(p: &egui::Painter, right: f32, mid: f32, thread: &Thread, lang: Lang, t: &Tokens, back: Color32) -> f32 {
    let words = delta_e_words(lang, thread.delta_e);
    let colour = if thread.mismatch { marks::text_colour(t, Mark::Warning) } else { marks::readable_text(t.text_dim, &[back], t.dark) };
    let g = p.text(pos2(right, mid), Align2::RIGHT_CENTER, &words, FontId::proportional(SMALL_TEXT), colour);
    if thread.mismatch {
        p.rect_stroke(g.expand2(BADGE_PAD), BADGE_RADIUS, Stroke::new(1.0, colour), StrokeKind::Outside);
    }
    g.left() - BADGE_PAD.x
}

/// `sections` (words and their colours) on one line from `at` (its left middle), at most `width`
/// points wide, cut short with "…" past it → whether they were cut.
fn one_line(p: &egui::Painter, sections: &[(&str, Color32)], size: f32, at: egui::Pos2, width: f32) -> bool {
    let mut job = egui::text::LayoutJob::default();
    for (words, colour) in sections {
        job.append(words, 0.0, egui::TextFormat { font_id: FontId::proportional(size), color: *colour, ..Default::default() });
    }
    job.wrap = egui::text::TextWrapping { max_width: width.max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = p.layout_job(job);
    let cut = galley.elided;
    p.galley(pos2(at.x, at.y - galley.size().y / 2.0), galley, Color32::PLACEHOLDER);
    cut
}

/// The room a row keeps at its right end for ↺ (points).
const RESTORE_ROOM: f32 = 22.0;

/// ↺ at the right end of a changed row → whether it was pressed. Placed over the row, leaving the
/// list's rows where they are (`Ui::place`): a scope there would move the list's cursor up to the
/// button's bottom, and the next row would start over this one.
fn restore_button(ui: &mut Ui, rect: Rect, row: &Row) -> bool {
    let at = Rect::from_center_size(pos2(rect.right() - RESTORE_ROOM / 2.0, rect.center().y), vec2(RESTORE_ROOM, RESTORE_ROOM));
    let name = fill(crate::et!("Back to Auto: {color}"), &[("color", &row.color)]);
    ui.place(at, |ui: &mut Ui| rows::restore(ui, &name, crate::et!("Back to Auto"))).clicked()
}

/// A row's clicks and keys: selection, and the keyboard's focus moved with it.
fn row_input(ui: &Ui, state: &mut State, rows: &[Row], k: usize, resp: &egui::Response) {
    // Space on the row in focus is its toggle alone: egui counts it as the row's click too, which
    // would first select the row alone. Enter stays a click.
    let space = resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space));
    if resp.clicked() && !space {
        let modifiers = ui.input(|i| i.modifiers);
        click(state, rows, k, modifiers);
    }
    if !resp.has_focus() {
        return;
    }
    let (up, down, shift) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.modifiers.shift));
    if space {
        toggle(state, rows, k);
    }
    if (up || down)
        && let Some(next) = step(state, rows, k, down, shift)
    {
        state.focus = rows.get(next).map(|r| r.color.clone());
    }
}

/// The actions under the list: Merge, Don't Sew or Sew, and Choose Thread… with its list.
fn actions(ui: &mut Ui, state: &mut State, rows: &[Row], d: &mut Dialog) {
    let chosen = state.chosen(rows);
    let colors: Vec<String> = chosen.iter().map(|r| r.color.clone()).collect();
    let all_off = !chosen.is_empty() && chosen.iter().all(|r| !r.sewn);
    let palette = d.str(super::PALETTE);
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        let refused = merge_refused(&chosen);
        if action(ui, crate::et!("Merge"), refused, crate::et!("Sew these colors as one.")).clicked() {
            merge(&mut d.fields, &chosen);
        }
        let (sew_words, sew_tip) = if all_off {
            (crate::et!("Sew"), crate::et!("Sew these colors again, as Auto chooses."))
        } else {
            (crate::et!("Don't Sew"), crate::et!("Leave these colors out: their areas aren't sewn."))
        };
        if action(ui, sew_words, sew_refused(&chosen), sew_tip).clicked() {
            set_sewn(&mut d.fields, &colors, all_off);
        }
        let refused = choose_refused(&chosen);
        let button = action(ui, crate::et!("Choose Thread…"), refused, crate::et!("Choose the thread this color is sewn in."));
        if button.clicked()
            && let Some(row) = chosen.first()
        {
            state.picker = Some(Picker { color: row.color.clone(), search: String::new(), opening: true });
        }
        picked = picker(ui, state, &button, &palette);
    });
    if let Some((color, index)) = picked {
        choose_thread(&mut d.fields, &color, &palette, index);
    }
}

/// An action button with `words`, disabled with `refused` said as its tooltip, else `tip`.
fn action(ui: &mut Ui, words: &str, refused: Option<&str>, tip: &str) -> egui::Response {
    let resp = ui.add_enabled(refused.is_none(), egui::Button::new(words));
    match refused {
        Some(why) => resp.on_disabled_hover_text(why),
        None => resp.on_hover_text(tip),
    }
}

/// Choose Thread…'s list, below its button while it is open: a search field and the palette's
/// threads, nearest first → the colour and the thread picked.
fn picker(ui: &mut Ui, state: &mut State, button: &egui::Response, palette: &str) -> Option<(String, usize)> {
    let Picker { color, mut search, opening } = state.picker.clone()?;
    let lang = crate::i18n::current();
    let t = Tokens::get(ui.ctx());
    let mut open = true;
    let mut picked = None;
    egui::Popup::from_response(button)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .accessible_name(crate::et!("Choose Thread…"))
        .show(|ui| {
            ui.set_width(PICKER_WIDTH);
            let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text(crate::et!("Name or number")).desired_width(PICKER_WIDTH));
            if opening {
                field.request_focus();
            }
            let threads = picker_rows(palette, &color, &search);
            scrolled(ui, "digitize-thread-choice", threads.len(), PICKER_ROW_HEIGHT, PICKER_LIST_HEIGHT, |ui| {
                for (k, thread) in threads.iter().enumerate() {
                    let nearest = k == 0 && search.trim().is_empty();
                    if picker_row(ui, thread, nearest, palette, lang, &t).clicked() {
                        picked = Some((color.clone(), thread.index));
                    }
                }
            });
        });
    state.picker = (open && picked.is_none()).then_some(Picker { color, search, opening: false });
    picked
}

/// A thread in Choose Thread…'s list: its colour, name and number, ΔE, and "nearest" on the first.
fn picker_row(ui: &mut Ui, thread: &NearThread, nearest: bool, palette: &str, lang: Lang, t: &Tokens) -> egui::Response {
    // As wide as the list leaves it beside its bar.
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), PICKER_ROW_HEIGHT), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() || resp.has_focus() {
        p.rect_filled(rect, f32::from(t.radius), t.hover);
    }
    let mid = rect.center().y;
    let [r, g, b] = thread.color;
    let centre = pos2(rect.left() + PAD + SWATCH / 2.0, mid);
    p.circle_filled(centre, SWATCH / 2.0, Color32::from_rgb(r, g, b));
    p.circle_stroke(centre, SWATCH / 2.0, Stroke::new(1.0, t.border));
    let name = format!("{}  {} {}", thread.name, palette.to_uppercase(), thread.index);
    p.text(pos2(centre.x + SWATCH / 2.0 + GAP, mid), Align2::LEFT_CENTER, &name, FontId::proportional(NAME_TEXT), t.text_strong);
    let mut right = delta_e_words(lang, thread.delta_e);
    if nearest {
        right = fill(crate::et!("{value} · nearest"), &[("value", &right)]);
    }
    let colour = if thread.mismatch { marks::text_colour(t, Mark::Warning) } else { t.text_dim };
    p.text(pos2(rect.right() - PAD, mid), Align2::RIGHT_CENTER, &right, FontId::proportional(SMALL_TEXT), colour);
    let said = format!("{name}, {right}");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, said.clone()));
    resp
}
