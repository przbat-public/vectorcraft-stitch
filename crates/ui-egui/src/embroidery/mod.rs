//! The machine-embroidery interface of VectorCraft Stitch (this fork): what the app shows of a
//! design's stitches. Fork-owned, so that upstream files carry only one-line hooks into it.
//!
//! - [`marks`]: severity marks, drawn in code: error, warning, note, passed.
//! - [`check_marks`]: the Check tab's findings ringed on the canvas.
//! - [`counters`]: the status bar's stitches, colours, thread changes and time.
//! - [`numbers`]: numbers as the interface language writes them, in the embroidery interface's
//!   read-only text.
//! - [`plan_worker`]: the stitch preview's planning thread.
//! - [`stitch_preview`]: when the preview plans, and what it shows meanwhile.
//! - [`stitch_layer`]: the canvas in the stitch view.
//! - [`menu`]: View › Stitch Preview, its options, and the document tab's mode.
//! - [`switch`]: the canvas's Art | Stitches switch.
//! - [`notice`]: the canvas's notice while the stitches are planned, or can't be.
//! - [`overlay`]: the selection in the stitch view: entry and exit markers, changed objects.
//! - [`text`]: the embroidery interface's words, in the catalogues' `embroidery` context
//!   ([`crate::et!`], [`crate::etn!`]).
//! - [`zoom`]: View › Zoom to Box (`view.zoomTo`).
//! - [`render_stitches`]: the control channel's picture of the stitches.

pub mod check_marks;
pub mod counters;
pub mod marks;
pub mod menu;
pub mod notice;
pub mod numbers;
pub mod overlay;
pub mod plan_worker;
pub mod stitch_layer;
pub mod stitch_preview;
pub mod switch;
pub mod text;
pub mod zoom;

use vectorcraft_engine::cmd::embroidery::render::{Look, Region, picture};
use vectorcraft_engine::cmd::embroidery::scene::FABRIC;

use crate::VectorcraftApp;

/// The fork's own state in the app.
#[derive(Default)]
pub struct Embroidery {
    /// The stitch preview's scheduler and the plans it shows.
    pub preview: stitch_preview::StitchPreview,
    /// The canvas's stitch layer.
    pub layer: stitch_layer::StitchLayer,
    /// The selected objects' ends in the plan shown ([`overlay`]).
    pub(crate) ends: overlay::Ends,
    /// The Embroidery panel's Sewing order tab.
    pub(crate) order: crate::panels::embroidery::order::OrderState,
    /// The Embroidery panel's Check tab.
    pub(crate) check: crate::panels::embroidery::check::CheckState,
    /// The status bar's counters.
    pub(crate) counters: counters::Counters,
}

/// The height of the isolation mode's breadcrumb bar, which upstream draws over the canvas's top
/// after the fork's controls (`chrome::isolation_bar`), points: as it draws it.
const ISOLATION_BAR: f32 = 24.0;

/// What the fork draws over the canvas, after it: the selection's markers in the stitch view, the
/// check's marks, the Art | Stitches switch, and the notice under it (both below the isolation
/// mode's bar while it shows).
pub fn canvas_controls(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    let Some(canvas) = app.canvas_rect.filter(|_| !crate::menus::home_showing(app)) else { return };
    if let Some(view) = app.view().copied() {
        let xf = crate::canvas::Xf::new(canvas, &view);
        overlay::draw(app, &ui.painter_at(canvas), &xf);
        check_marks::draw(app, ui.ctx(), &ui.painter_at(canvas), &xf);
    }
    let isolated = app.ui.screen_mode < 3 && app.session.active().is_some_and(|st| st.isolation.is_some());
    let below = if isolated { egui::Rect::from_min_max(canvas.min + egui::vec2(0.0, ISOLATION_BAR), canvas.max) } else { canvas };
    switch::show(app, ui, below);
    notice::show(app, ui, below);
}

/// The fork's work at the frame's end: a click on the status bar's counters acted on, then what
/// the frame asked of the stitch preview planned.
pub fn end_frame(app: &mut VectorcraftApp, ctx: &egui::Context) {
    counters::act(app);
    stitch_preview::schedule(app, ctx);
}

/// The longest a window capture (`ui.screenshot`) waits for the stitch view to be drawn for the
/// document as it is, milliseconds: far longer than a real design plans and draws, so it only ends
/// the wait for a design that is slow to plan. An estimate.
pub const MAX_SCREENSHOT_WAIT_MS: f64 = 5000.0;

/// Whether a window capture should wait: the active document's stitches are being planned, or
/// drawn on the stitch layer's thread, for the document as it is.
pub fn holds_screenshots(app: &VectorcraftApp, now: f64) -> bool {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return false };
    app.embroidery.preview.busy(uid, now) || app.embroidery.layer.waiting()
}

/// `ui.inspect`'s `stitchPreview`: the active document's stitch view, how its plan stands
/// (`state`: off, fresh, pending, stale or failed; `planning`, `progress`, `error`, `planMs`,
/// `generation`), whether the canvas draws it (`drawn`), and the notice shown (`notice`). Null with
/// no document.
pub fn inspect(app: &VectorcraftApp, now: f64) -> serde_json::Value {
    let Some(st) = app.session.active() else { return serde_json::Value::Null };
    let mut v = app.embroidery.preview.inspect(st.uid, now);
    if let Some(o) = v.as_object_mut() {
        o.insert("on".into(), app.session.stitch_view_of(st.uid).on.into());
        o.insert("drawn".into(), app.embroidery.layer.drawn().into());
        o.insert("notice".into(), notice::text(app, now).map(|(t, _)| t).into());
        o.insert("counters".into(), app.embroidery.counters.inspect());
    }
    v
}

/// The control channel's `ui.render {stitchPreview: true}`: artboard `artboard` of the active
/// document drawn as its stitches at `scale` pixels a point, on the fabric, marked as its stitch
/// view marks them (Show Jumps and Cuts, Show Needle Points), with the options the stitch preview
/// plans with, as it draws them (`embroidery.render`'s own drawing).
pub fn render_stitches(app: &mut VectorcraftApp, artboard: usize, scale: f64) -> Result<vectorcraft_render::Rendered, String> {
    let uid = app.session.active().ok_or("no document")?.uid;
    let (opts, _) = app.session.stitch_preview_options(uid);
    let view = app.session.stitch_view_of(uid);
    let look = Look { jumps: view.jumps, needle_points: view.needle_points };
    picture(&mut app.session, Region::Artboard(artboard), scale, look, Some(FABRIC), &opts).map(|(img, _, _)| img).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_languages;
#[cfg(test)]
mod tests_preview;
