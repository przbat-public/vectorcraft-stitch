//! The image window's frame: its size (most of the app's window, never smaller than a 1280 × 800
//! screen holds at 100 %), its regions, and what is drawn in each: the title band, the preview on
//! the left, the steps' settings on the right, and the Live bar along the bottom with the buttons.
//! Below [`COMPACT_BELOW`] points wide the settings column narrows.

use egui::{Align, Layout, Rect, RichText, Sense, Ui, UiBuilder, Vec2, vec2};

use super::schedule::Freshness;
use super::{Live, words};
use crate::VectorcraftApp;
use crate::state::Dialog;
use crate::theme::Tokens;
use crate::widgets;

/// The window's share of the app's content area, each way: an estimate from the mockups, 1349 ×
/// 786 points of 1440 × 874.
pub(crate) const WIDTH_SHARE: f32 = 0.94;
pub(crate) const HEIGHT_SHARE: f32 = 0.92;

/// The smallest window, points: a 1280 × 800 screen at 100 % holds it with the app's bars round it.
/// An estimate.
pub(crate) const MIN_SIZE: Vec2 = vec2(960.0, 600.0);

/// Below this width (points) the settings column narrows. An estimate between the smallest window
/// and the mockups' 1349.
pub(crate) const COMPACT_BELOW: f32 = 1180.0;

/// The settings column's width, points: the mockups' 390, and 320 when compact (an estimate).
pub(crate) const SETTINGS_WIDTH: f32 = 390.0;
pub(crate) const COMPACT_SETTINGS_WIDTH: f32 = 320.0;

/// The heights of the title band and the Live bar, points: a heading's line with its gap, and two
/// lines of figures beside the buttons. Estimates from the mockups.
pub(crate) const TITLE_HEIGHT: f32 = 34.0;
pub(crate) const BAR_HEIGHT: f32 = 58.0;

/// The gap between the regions, and the window's own margin, points.
pub(crate) const GAP: f32 = 12.0;
const MARGIN: i8 = 16;

/// The × button's size, points: as the app's dialogs draw an icon button.
const CLOSE_SIZE: f32 = 22.0;

/// The size of the words under a heading (the title's facts, the Live bar's line), points.
pub(crate) const SMALL_TEXT: f32 = 12.0;

/// What the person asked of the window this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    Cancel,
    StartOver,
    Create,
}

/// The window's size in an app whose content area is `content`: its share of it, at least
/// [`MIN_SIZE`] while that fits, never more than the content area.
pub(crate) fn size(content: Rect) -> Vec2 {
    let wanted = vec2(content.width() * WIDTH_SHARE, content.height() * HEIGHT_SHARE).max(MIN_SIZE);
    wanted.min(content.size() - Vec2::splat(f32::from(MARGIN)))
}

/// Whether a window `width` points wide is drawn compact.
pub(crate) fn compact(width: f32) -> bool {
    width < COMPACT_BELOW
}

/// The regions of a window's inside `inner`: the title band, the preview, the settings column and
/// the Live bar.
pub(crate) struct Regions {
    pub(crate) title: Rect,
    pub(crate) preview: Rect,
    pub(crate) settings: Rect,
    pub(crate) bar: Rect,
}

pub(crate) fn regions(inner: Rect) -> Regions {
    let column = if compact(inner.width()) { COMPACT_SETTINGS_WIDTH } else { SETTINGS_WIDTH };
    let title = Rect::from_min_size(inner.min, vec2(inner.width(), TITLE_HEIGHT));
    let bar = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - BAR_HEIGHT), inner.max);
    let middle = Rect::from_min_max(egui::pos2(inner.left(), title.bottom() + GAP), egui::pos2(inner.right(), bar.top() - GAP));
    let settings = Rect::from_min_max(egui::pos2(middle.right() - column, middle.top()), middle.max);
    let preview = Rect::from_min_max(middle.min, egui::pos2(settings.left() - GAP, middle.bottom()));
    Regions { title, preview, settings, bar }
}

/// Draw the window for dialog `d` (its fields changed in place), the run state `live` shows, at
/// `now` (seconds) → what was asked.
pub(crate) fn draw(app: &mut VectorcraftApp, ctx: &egui::Context, d: &mut Dialog, now: f64) -> Option<Asked> {
    let size = size(ctx.content_rect());
    let title = crate::et!("Digitize Image");
    let id = egui::Id::unique("embroidery-digitize-image");
    let mut asked = None;
    crate::dialogs::modal::show_at(ctx, title, id, Vec2::ZERO, MARGIN, |ui| {
        let (inner, _) = ui.allocate_exact_size(size - Vec2::splat(2.0 * f32::from(MARGIN)), Sense::hover());
        let r = regions(inner);
        ui.scope_builder(UiBuilder::new().max_rect(r.title), |ui| {
            if title_band(app, ui, d) {
                asked = Some(Asked::Cancel);
            }
        });
        ui.scope_builder(UiBuilder::new().max_rect(r.preview), |ui| preview_region(app, ui, d));
        ui.scope_builder(UiBuilder::new().max_rect(r.settings), |ui| settings_region(app, ui, d));
        ui.scope_builder(UiBuilder::new().max_rect(r.bar), |ui| {
            if let Some(a) = live_bar(app, ui, now) {
                asked = Some(a);
            }
        });
    });
    asked
}

/// The title band: the window's name, what it digitizes, and × (Cancel) → whether × was pressed.
fn title_band(app: &VectorcraftApp, ui: &mut Ui, d: &Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut close = false;
    ui.horizontal(|ui| {
        crate::dialogs::modal::heading(ui, crate::et!("Digitize Image"));
        ui.add_space(GAP);
        if let Some(facts) = words::title_facts(app, d) {
            ui.label(RichText::new(facts).size(SMALL_TEXT).color(t.text_dim));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::icon_button(ui, "x", crate::et!("Cancel"), false, CLOSE_SIZE).clicked();
        });
    });
    close
}

/// The preview's region: what the window shows of the image (drawn by the preview's views).
fn preview_region(app: &VectorcraftApp, ui: &mut Ui, _d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, f32::from(t.radius), t.panel_darker);
    let Some(live) = app.embroidery.image_window.live.as_ref() else { return };
    if live.schedule.shown.is_none() {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, crate::et!("Digitizing…"), egui::FontId::proportional(SMALL_TEXT), t.text_dim);
    }
}

/// The settings column: the four steps (drawn by the steps' sections).
fn settings_region(_app: &VectorcraftApp, ui: &mut Ui, _d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    for name in words::step_names() {
        ui.label(RichText::new(name).color(t.text));
    }
}

/// The Live bar: how the result stands, and Start Over, Cancel and Create Embroidery Objects →
/// what was asked.
fn live_bar(app: &VectorcraftApp, ui: &mut Ui, now: f64) -> Option<Asked> {
    let t = Tokens::get(ui.ctx());
    let live = app.embroidery.image_window.live.as_ref();
    let mut asked = None;
    ui.horizontal(|ui| {
        if let Some(live) = live {
            ui.label(RichText::new(state_words(live, now)).size(SMALL_TEXT).color(t.text_dim));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ready = live.is_some_and(|l| l.schedule.shown.as_ref().is_some_and(|s| s.outputs.made().is_some()));
            if widgets::primary_button_enabled(ui, crate::et!("Create Embroidery Objects"), ready).clicked() {
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

/// How the result stands, in words.
fn state_words(live: &Live, now: f64) -> String {
    if live.create_waiting {
        return crate::et!("Create waits for the preview to be ready.").to_string();
    }
    match live.schedule.freshness(now) {
        Freshness::Fresh => String::new(),
        Freshness::Pending { notice: false, .. } => String::new(),
        Freshness::Pending { notice: true, progress } => words::recomputing(progress),
        Freshness::Failed(reason) => crate::embroidery::text::fill(crate::et!("Can't digitize it: {reason}"), &[("reason", &reason)]),
    }
}
