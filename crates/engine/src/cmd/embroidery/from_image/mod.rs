//! Digitizing from an image (`embroidery.digitizeImage`): an image's pixels made into embroidery
//! objects, every size threshold given in millimetres.
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
//! - [`regions`]: the traced areas drawn back into the working copy's pixels, an area for each
//!   pixel, with which areas touch which, how wide each is, and the point that keys it;
//! - [`flat`]: whether the picture is flat art or has gradients (a photo, shading), which are
//!   sewn as bands of flat colour and warned about;
//! - [`colours`]: each colour matched to a thread of the document's palette, merged, or left
//!   unsewn;
//! - [`rules`]: the smallest area (long thin lines kept below it), and the slivers a picture
//!   leaves along its edges;
//! - [`inside`]: what lies inside what, and which islands are sewn over the area round them;
//! - [`order`]: the order the areas are sewn in;
//! - [`centre`]: a thin line's middle, the path a running stitch follows through its junctions;
//! - [`overlap`]: the shapes they are sewn as, each grown under the neighbours sewn after it (and
//!   under a centre line to its middle);
//! - [`create`]: the embroidery objects made of them, in place of the image, which moves to a
//!   template layer;
//! - [`command`]: `embroidery.digitizeImage` and its answer, run as a job ([`DigitizeJob`]);
//! - [`warnings`]: what the answer warns of, each by an id a window words, with its values.
//!
//! No upstream file is hooked: the trace crate is called as any caller would, and the command is
//! one of the fork's own.

mod background;
mod centre;
mod colours;
mod command;
mod create;
mod flat;
mod inside;
mod order;
mod overlap;
mod regions;
mod rules;
mod settings;
mod trace;
mod warnings;
mod working;

pub(super) use command::digitize_image;
pub use command::{DIGITIZE_PROGRESS, DIGITIZE_STAGES, DigitizeFound, DigitizeJob};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_areas;
#[cfg(test)]
mod tests_colours;
#[cfg(test)]
mod tests_create;
#[cfg(test)]
mod tests_lines;
#[cfg(test)]
mod tests_overlap;
