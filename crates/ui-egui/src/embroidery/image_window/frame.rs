//! The image window's frame: its size (most of the app's window, never smaller than a 1280 × 800
//! screen holds at 100 %), its regions, and what is drawn in each: the title band, the preview on
//! the left, the steps' settings on the right, and the Live bar along the bottom with the buttons.
//! Below [`COMPACT_BELOW`] points wide the settings column narrows.

use egui::{Align, Layout, Rect, RichText, Sense, Ui, UiBuilder, Vec2, vec2};

use super::words;
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

/// The room the window keeps above the app's bottom bars, points: as the app's menus keep above
/// them (`widgets::menu_room`'s gap).
pub(crate) const ABOVE_BARS: f32 = 12.0;

/// The window's size in content area `content` and how far above the area's centre its centre
/// sits, so that it ends [`ABOVE_BARS`] over `floor` (the top of the app's bottom bars, the hint
/// line and the status bar, which it would otherwise cut through), its top where it would be; never
/// shorter than [`MIN_SIZE`] allows.
pub(crate) fn placed(content: Rect, floor: f32) -> (Vec2, f32) {
    let s = size(content);
    let bottom = content.center().y + s.y / 2.0;
    let over = (bottom + ABOVE_BARS - floor).clamp(0.0, (s.y - MIN_SIZE.y).max(0.0));
    (vec2(s.x, s.y - over), over / 2.0)
}

/// Whether a window `width` points wide is drawn compact.
pub(crate) fn compact(width: f32) -> bool {
    width < COMPACT_BELOW
}

/// The regions of a window's inside `inner`: the title band, the step strip, the preview, the
/// settings column and the Live bar.
pub(crate) struct Regions {
    pub(crate) title: Rect,
    pub(crate) strip: Rect,
    pub(crate) preview: Rect,
    pub(crate) settings: Rect,
    pub(crate) bar: Rect,
}

pub(crate) fn regions(inner: Rect) -> Regions {
    let column = if compact(inner.width()) { COMPACT_SETTINGS_WIDTH } else { SETTINGS_WIDTH };
    let title = Rect::from_min_size(inner.min, vec2(inner.width(), TITLE_HEIGHT));
    let bar = Rect::from_min_max(egui::pos2(inner.left(), inner.bottom() - BAR_HEIGHT), inner.max);
    let strip = Rect::from_min_size(egui::pos2(inner.left(), title.bottom() + GAP), vec2(inner.width(), super::steps::CARD_HEIGHT));
    let middle = Rect::from_min_max(egui::pos2(inner.left(), strip.bottom() + GAP), egui::pos2(inner.right(), bar.top() - GAP));
    let settings = Rect::from_min_max(egui::pos2(middle.right() - column, middle.top()), middle.max);
    let preview = Rect::from_min_max(middle.min, egui::pos2(settings.left() - GAP, middle.bottom()));
    Regions { title, strip, preview, settings, bar }
}

/// Draw the window for dialog `d` (its fields changed in place), the run state `live` shows, at
/// `now` (seconds) → what was asked.
pub(crate) fn draw(app: &mut VectorcraftApp, ctx: &egui::Context, d: &mut Dialog, now: f64) -> Option<Asked> {
    let (size, lift) = placed(ctx.content_rect(), crate::widgets::menu_floor(ctx));
    let title = crate::et!("Digitize Image");
    let id = egui::Id::unique("embroidery-digitize-image");
    let mut asked = None;
    crate::dialogs::modal::show_at(ctx, title, id, vec2(0.0, -lift), MARGIN, |ui| {
        let (inner, _) = ui.allocate_exact_size(size - Vec2::splat(2.0 * f32::from(MARGIN)), Sense::hover());
        let r = regions(inner);
        ui.scope_builder(UiBuilder::new().max_rect(r.title), |ui| {
            if title_band(app, ui, d) {
                asked = Some(Asked::Cancel);
            }
        });
        ui.scope_builder(UiBuilder::new().max_rect(r.strip), |ui| super::steps::strip(app, ui, d));
        ui.scope_builder(UiBuilder::new().max_rect(r.preview), |ui| super::preview::show(app, ui, d, now));
        ui.scope_builder(UiBuilder::new().max_rect(r.settings), |ui| super::steps::accordion(app, ui, d));
        ui.scope_builder(UiBuilder::new().max_rect(r.bar), |ui| {
            if let Some(a) = super::live_bar::show(app, ui, now) {
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
