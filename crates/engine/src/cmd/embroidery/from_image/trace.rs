//! Upstream's Image Trace on the working copy, with the settings digitizing fixes:
//! - the image's own colours: Color mode with the Limited palette (the `colors` that best represent
//!   the image), with the Automatic palette in logo mode, or Grayscale;
//! - abutting shapes: each colour's own area, so that two neighbours share one curve along the
//!   edge between them (upstream's Overlapping lays every lower colour whole under every upper one:
//!   up to 2.3 times the stitches on the real designs' artwork, measured on 2026-11-02);
//! - filled areas only, and no gradients, shapes or grouping, which embroidery doesn't sew.
//!
//! Noise, the one threshold of the trace that is an area, keeps what a thread's footprint covers
//! ([`TRACE_AREA_MM2`], turned into the working copy's pixels by [`area_px`]), or the smallest area
//! when that is smaller: the smallest area itself is applied to the traced areas afterwards
//! (`super::rules`), where a long thin line below it can be kept, which the trace's noise removal
//! would merge away before any line is looked for.

use vectorcraft_trace as tr;

use super::settings::{C, Mode, PERCENT_RANGE, Settings};
use super::working::Working;
use crate::cmd::bad;
use crate::{EngineError, Result};

/// The most anchor points a trace may make before it is refused: upstream Image Trace's limit on
/// the web (`MAX_TRACE_ANCHORS`, 2,000,000, where memory is tighter; 8,000,000 elsewhere), on every
/// system here, so that an image is digitized on the desktop exactly when it is on the web
/// (upstream keeps it in a module the fork can't reach). The real designs' artwork and the test
/// logos trace in at most 2,476; noise of 1,500 pixels a side in 16 colours, in 6.7 million, the
/// trace giving up a third of the way.
pub(super) const MAX_ANCHORS: usize = 2_000_000;

/// The most shapes a trace may make before it is refused, ahead of the region map and the rules,
/// whose work grows with them. An estimate: the real designs' artwork and the test logos trace in
/// at most 114 shapes, the specks of a JPEG among them; noise of 1,500 pixels a side in 16
/// colours, in 1.3 million.
pub(super) const MAX_TRACED_SHAPES: usize = 250_000;

/// The area the trace keeps at least (square millimetres): a thread's footprint, a square as wide
/// as the thread lies (0.4 mm for 40 wt, `plan::THREAD_WIDTH`): anything smaller can't be sewn as
/// anything, a line or a dot.
pub(super) const TRACE_AREA_MM2: f64 = vectorcraft_stitch::plan::THREAD_WIDTH * vectorcraft_stitch::plan::THREAD_WIDTH;

/// The whole pixels `area_mm2` square millimetres cover at `px_per_mm`, rounded up: ⌈A × r²⌉,
/// as Image Trace's noise counts them (its threshold is strict: a region of exactly that many
/// pixels stays). 0 for nothing, and at most `u32::MAX`.
pub(super) fn area_px(area_mm2: f64, px_per_mm: f64) -> u32 {
    let px = (area_mm2 * px_per_mm * px_per_mm).ceil();
    if px.is_finite() && px > 0.0 { px.min(f64::from(u32::MAX)) as u32 } else { 0 }
}

/// How a trace finds the image's colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Palette {
    /// As the mode asks: Limited for colours and greys, Automatic for logo mode.
    Mode,
    /// Limited, whatever the mode: logo mode's trace again, when the Automatic palette found more
    /// colours than asked (a photo, or gradients).
    Limited,
}

/// Image Trace's parameters for `settings` at `px_per_mm`, reducing the image to at most `colors`
/// (the colours asked, and one more for a background colour), its palette found as `palette` says.
pub(super) fn params(settings: &Settings, px_per_mm: f64, colors: u32, palette: Palette) -> tr::TraceParams {
    let (mode, palette) = match (settings.mode, palette) {
        (Mode::Color, _) | (Mode::Logo, Palette::Limited) => (tr::Mode::Color, tr::Palette::Limited),
        (Mode::Logo, Palette::Mode) => (tr::Mode::Color, tr::Palette::Automatic),
        (Mode::Grayscale, _) => (tr::Mode::Grayscale, tr::Palette::Limited),
    };
    tr::TraceParams {
        mode,
        palette,
        colors,
        // Smoothing is upstream's Paths turned round: the higher, the looser the fit.
        paths: PERCENT_RANGE.1 - settings.smoothing,
        corners: settings.corners,
        noise: area_px(settings.min_area.min(TRACE_AREA_MM2), px_per_mm),
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

/// The working copy traced with `settings` in at most `colors` colours found as `palette` says: its paths in the working copy's pixels, bottom-most
/// first, each with its colour and pixel count, and the colours used. A cancel stops it between
/// stages (with [`EngineError::Cancelled`], nothing made); an image that would make more than
/// [`MAX_ANCHORS`] anchor points is refused with advice.
pub(super) fn traced(working: &Working, settings: &Settings, colors: u32, palette: Palette) -> Result<tr::TraceResult> {
    let p = params(settings, working.px_per_mm, colors, palette);
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
