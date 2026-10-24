//! The canvas's notice in the stitch view, under the Art | Stitches switch: "Planning stitches…"
//! with the plan's progress once the stitches shown have been out of date a while
//! ([`super::stitch_preview::STALE_NOTICE_SECS`]), or why the design can't be planned, while the
//! status bar's counters don't say it (the bar hidden, or too short of room); how many selected
//! objects aren't sewn.

use egui::{CornerRadius, FontId, Id, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use vectorcraft_engine::cmd::embroidery::view::{layers, paused};

use super::stitch_preview::Freshness;
use crate::VectorcraftApp;
use crate::theme::Tokens;

/// Room between the notice and the canvas's right edge, and below the switch, points.
const INSET: f32 = 10.0;
const BELOW_SWITCH: f32 = 42.0;
/// The notice's height, the room either side of its words, and its progress bar's height, points.
const HEIGHT: f32 = 26.0;
const PAD: f32 = 10.0;
const BAR: f32 = 2.0;
/// The size of its words, points: the interface's small text.
const TEXT_SIZE: f32 = 12.0;
/// The widest notice, points: a long reason wraps onto more lines within it.
const MAX_WIDTH: f32 = 420.0;

/// What the notice says, and the plan's progress (0 to 1) when it can say: `None` when there is
/// nothing to say.
pub fn text(app: &VectorcraftApp, now: f64) -> Option<(String, Option<f64>)> {
    let st = app.session.active()?;
    let view = app.session.stitch_view_of(st.uid);
    if !layers(view, paused(&st.doc), app.ui.view.outline).stitches {
        return None;
    }
    // The status bar's counters say when the stitches are planned or can't be: the notice says it
    // only while the bar doesn't.
    let bar = app.ui.status_bar && app.ui.screen_mode < 3 && app.embroidery.counters.says();
    match app.embroidery.preview.freshness(st.uid, now) {
        Freshness::Pending { .. } | Freshness::Stale { .. } | Freshness::Failed { .. } if bar => None,
        Freshness::Pending { progress, notice: true } | Freshness::Stale { progress, notice: true } => {
            Some((crate::et!("Planning stitches…").to_string(), progress))
        }
        Freshness::Failed { error } => Some((format!("{} {}", crate::et!("Can't plan the stitches:"), crate::i18n::msg(&error)), None)),
        Freshness::Fresh => match super::overlay::not_sewn(app, now) {
            0 => None,
            n => Some((crate::etn!(n as u64, "{count} selected object isn't sewn", "{count} selected objects aren't sewn"), None)),
        },
        _ => None,
    }
}

/// Draw the notice over the canvas `canvas`, when there is one.
pub fn show(app: &VectorcraftApp, ui: &Ui, canvas: Rect) {
    if app.ui.screen_mode >= 3 {
        return;
    }
    let now = ui.input(|i| i.time);
    let Some((words, progress)) = text(app, now) else { return };
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(TEXT_SIZE);
    let galley = ui.painter().layout(words.clone(), font, t.text, MAX_WIDTH - 2.0 * PAD);
    let size = vec2(galley.size().x + 2.0 * PAD, galley.size().y.max(HEIGHT - 2.0 * BAR) + BAR);
    let rect = Rect::from_min_size(pos2(canvas.right() - INSET - size.x, canvas.top() + BELOW_SWITCH), size);
    if !canvas.contains_rect(rect) {
        return;
    }
    let radius = CornerRadius::same(t.radius);
    ui.painter().rect(rect, radius, t.panel, Stroke::new(1.0, t.border), StrokeKind::Inside);
    ui.painter().galley(rect.left_center() + vec2(PAD, -galley.size().y / 2.0), galley, t.text);
    if let Some(done) = progress.filter(|p| p.is_finite()) {
        let bar =
            Rect::from_min_size(pos2(rect.left() + 1.0, rect.bottom() - BAR - 1.0), vec2((rect.width() - 2.0) * done.clamp(0.0, 1.0) as f32, BAR));
        ui.painter().rect_filled(bar, 0.0, t.accent);
    }
    let id = Id::new("embroidery-notice");
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_label(words);
    });
}
