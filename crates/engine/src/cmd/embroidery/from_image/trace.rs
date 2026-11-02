//! Upstream's Image Trace on the working copy, with the settings digitizing fixes:
//! - the image's own colours: Color mode with the Limited palette (the `colors` that best represent
//!   the image), with the Automatic palette in logo mode, or Grayscale;
//! - abutting shapes: each colour's own area, so that two neighbours share one curve along the
//!   edge between them (upstream's Overlapping lays every lower colour whole under every upper one:
//!   up to 2.3 times the stitches on the real designs' artwork, measured on 2026-11-02);
//! - filled areas only, and no gradients, shapes or grouping, which embroidery doesn't sew.
//!
//! Noise, the one threshold of the trace that is an area, is the smallest area turned into the
//! working copy's pixels ([`area_px`]).

use vectorcraft_trace as tr;

use super::settings::{C, Mode, PERCENT_RANGE, Settings};
use super::working::Working;
use crate::cmd::bad;
use crate::{EngineError, Result};

/// The most anchor points a trace may make before it is refused (a photo traced finely makes
/// millions): upstream Image Trace's own limit (`MAX_TRACE_ANCHORS`, 8,000,000; 2,000,000 on the
/// web, where memory is tighter), repeated here because upstream keeps it in a module the fork
/// can't reach.
#[cfg(not(target_arch = "wasm32"))]
pub(super) const MAX_ANCHORS: usize = 8_000_000;
#[cfg(target_arch = "wasm32")]
pub(super) const MAX_ANCHORS: usize = 2_000_000;

/// The whole pixels `area_mm2` square millimetres cover at `px_per_mm`, rounded up: ⌈A × r²⌉,
/// as Image Trace's noise counts them (its threshold is strict: a region of exactly that many
/// pixels stays). 0 for nothing, and at most `u32::MAX`.
pub(super) fn area_px(area_mm2: f64, px_per_mm: f64) -> u32 {
    let px = (area_mm2 * px_per_mm * px_per_mm).ceil();
    if px.is_finite() && px > 0.0 { px.min(f64::from(u32::MAX)) as u32 } else { 0 }
}

/// Image Trace's parameters for `settings` at `px_per_mm`.
pub(super) fn params(settings: &Settings, px_per_mm: f64) -> tr::TraceParams {
    let (mode, palette) = match settings.mode {
        Mode::Color => (tr::Mode::Color, tr::Palette::Limited),
        Mode::Logo => (tr::Mode::Color, tr::Palette::Automatic),
        Mode::Grayscale => (tr::Mode::Grayscale, tr::Palette::Limited),
    };
    tr::TraceParams {
        mode,
        palette,
        colors: settings.colors,
        // Smoothing is upstream's Paths turned round: the higher, the looser the fit.
        paths: PERCENT_RANGE.1 - settings.smoothing,
        corners: settings.corners,
        noise: area_px(settings.min_area, px_per_mm),
        method: tr::Method::Abutting,
        ignore_white: false,
        ignore_color: None,
        snap_curves_to_lines: false,
        shapes: false,
        auto_grouping: false,
        gradients: false,
        fills: true,
        strokes: false,
        ..tr::TraceParams::default()
    }
}

/// The working copy traced with `settings`: its paths in the working copy's pixels, bottom-most
/// first, each with its colour and pixel count, and the colours used. A cancel stops it between
/// stages (with [`EngineError::Cancelled`], nothing made); an image that would make more than
/// [`MAX_ANCHORS`] anchor points is refused with advice.
pub(super) fn traced(working: &Working, settings: &Settings) -> Result<tr::TraceResult> {
    let p = params(settings, working.px_per_mm);
    // The stages before the colour layers report on this thread, where a cancel reaches them; the
    // layers report from the threads that trace them, where it doesn't.
    tr::trace_watched(&working.raster, &p, MAX_ANCHORS, &|_, _| !vectorcraft_doc::cancel::is_cancelled()).map_err(|e| match e {
        tr::TraceError::Cancelled => EngineError::Cancelled,
        tr::TraceError::TooComplex { max } => {
            bad(C, format!("the image would make more than {max} anchor points: raise `minArea` or `smoothing`, or lower `colors`"))
        }
        e => bad(C, format!("the image can't be traced: {e}")),
    })
}
