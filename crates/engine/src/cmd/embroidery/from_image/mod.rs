//! Digitizing from an image (`embroidery.digitizeImage`): an image's pixels made into embroidery
//! objects, every threshold given in millimetres.
//!
//! Image Trace works only in pixels: its noise is a pixel count, its stroke width pixels, and an
//! image traces at whatever size it happens to be. So the fork chooses a working copy of the image
//! itself, at a resolution in pixels a millimetre ([`working`]), and turns each threshold given in
//! millimetres or square millimetres into that copy's pixels ([`trace`]). Everything after the trace
//! is in millimetres again.
//!
//! The stages, each a module of its own:
//! - [`settings`]: the command's settings, each with its window step, unit, range and default;
//! - [`working`]: the image decoded, cropped and resampled to the working resolution;
//! - [`background`]: what lies round the art, found at the image's corners;
//! - [`trace`]: upstream's Image Trace (`vectorcraft_trace`, through its public API) with the fixed
//!   settings digitizing needs: the image's own colours, abutting shapes;
//! - [`colours`]: each colour matched to a thread of the document's palette, merged, or left
//!   unsewn;
//! - [`command`]: `embroidery.digitizeImage` and its answer.
//!
//! No upstream file is hooked: the trace crate is called as any caller would, and the command is
//! one of the fork's own.

mod background;
mod colours;
mod command;
mod settings;
mod trace;
mod working;

pub(super) use command::digitize_image;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_colours;
