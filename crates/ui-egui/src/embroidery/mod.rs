//! The machine-embroidery interface of VectorCraft Stitch (this fork): what the app shows of a
//! design's stitches. Fork-owned, so that upstream files carry only one-line hooks into it.
//!
//! - [`numbers`]: numbers as the interface language writes them, in the embroidery interface's
//!   read-only text.
//! - [`plan_worker`]: the stitch preview's planning thread.
//! - [`stitch_preview`]: when the preview plans, and what it shows meanwhile.
//! - [`text`]: the embroidery interface's words, in the catalogues' `embroidery` context
//!   ([`crate::et!`], [`crate::etn!`]).
//! - [`render_stitches`]: the control channel's picture of the stitches.

pub mod numbers;
pub mod plan_worker;
pub mod stitch_preview;
pub mod text;

use vectorcraft_engine::cmd::embroidery::render::{Region, picture};
use vectorcraft_engine::cmd::embroidery::scene::FABRIC;

use crate::VectorcraftApp;

/// The fork's own state in the app.
#[derive(Default)]
pub struct Embroidery {
    /// The stitch preview's scheduler and the plans it shows.
    pub preview: stitch_preview::StitchPreview,
}

/// The control channel's `ui.render {stitchPreview: true}`: artboard `artboard` of the active
/// document drawn as its stitches at `scale` pixels a point, on the fabric, jumps and cuts marked,
/// with the options the stitch preview plans with, as it draws them (`embroidery.render`'s own
/// drawing).
pub fn render_stitches(app: &mut VectorcraftApp, artboard: usize, scale: f64) -> Result<vectorcraft_render::Rendered, String> {
    let uid = app.session.active().ok_or("no document")?.uid;
    let (opts, _) = app.session.stitch_preview_options(uid);
    picture(&mut app.session, Region::Artboard(artboard), scale, true, Some(FABRIC), &opts).map(|(img, _, _)| img).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
