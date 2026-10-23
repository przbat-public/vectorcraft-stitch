//! The machine-embroidery interface of VectorCraft Stitch (this fork): what the app shows of a
//! design's stitches. Fork-owned, so that upstream files carry only one-line hooks into it.
//!
//! - [`numbers`]: numbers as the interface language writes them, in the embroidery interface's
//!   read-only text.
//! - [`text`]: the embroidery interface's words, in the catalogues' `embroidery` context
//!   ([`crate::et!`], [`crate::etn!`]).
//! - [`render_stitches`]: the control channel's picture of the stitches.

pub mod numbers;
pub mod text;

use vectorcraft_engine::EmbroideryOptions;
use vectorcraft_engine::cmd::embroidery::render::{Region, picture};
use vectorcraft_engine::cmd::embroidery::scene::FABRIC;

use crate::VectorcraftApp;

/// The control channel's `ui.render {stitchPreview: true}`: artboard `artboard` of the active
/// document drawn as its stitches at `scale` pixels a point, on the fabric, jumps and cuts marked,
/// as the stitch preview draws it (`embroidery.render`'s own drawing).
pub fn render_stitches(app: &mut VectorcraftApp, artboard: usize, scale: f64) -> Result<vectorcraft_render::Rendered, String> {
    let opts = EmbroideryOptions::default();
    picture(&mut app.session, Region::Artboard(artboard), scale, true, Some(FABRIC), &opts).map(|(img, _, _)| img).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
