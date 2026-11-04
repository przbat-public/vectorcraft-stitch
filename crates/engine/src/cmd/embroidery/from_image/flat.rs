//! Whether a picture is flat art or has gradients (a photo, shading). Digitizing sews flat colours,
//! so a gradient is sewn as bands of flat colour, each band's edge one the picture doesn't have. The
//! project owner's decision (2026-11-01) is that a photo or gradients are warned about, never
//! refused for it. Logo mode has a test of its own, the Automatic palette's (`super::command`);
//! the colours and greys modes use this one.
//!
//! The measure looks across the trace's own edges. Where flat art has an edge, its colour changes
//! there at once: a pixel a little way inside each area is that area's colour, so the two differ
//! about as much as the colours they were traced in. Where a gradient was cut into bands, the
//! picture changes little across the cut, and the two pixels differ far less than their bands do.
//! Noise (a JPEG's ringing, anti-aliasing that reaches past the inset) makes a few such crossings
//! along any edge, never most of one; a band's edge is gradual along its whole length. So a picture
//! has gradients when enough of the crossings lie on borders that are mostly gradual.

use std::collections::BTreeMap;

use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_trace as tr;

use super::regions::{NONE, Regions};

/// How far inside each area the two pixels compared across an edge lie (pixels): past an
/// anti-aliased edge's blend, a pixel or two wide, and most of a JPEG's ringing.
const INSET_PX: usize = 3;

/// A crossing is gradual when the picture changes across it by less than this share of what the
/// colours it was traced in do (CIEDE2000 both): half. Flat art changes across its edges about as
/// much as its colours do: the median share on the real designs' artwork in 6 colours, as PNG and
/// as JPEG of quality 55, is 0.99 to 1.04 (measured 2026-11-03).
const GRADUAL_STEP: f64 = 0.5;

/// The fewest crossings a border between two areas is judged on: fewer can't tell an edge from
/// noise. An estimate.
const MIN_BORDER_CROSSINGS: usize = 8;

/// The share of the crossings lying on mostly gradual borders from which a picture is taken to
/// have gradients: 2 %. An estimate, from measures of 2026-11-03 on pictures digitized in 6 colours,
/// in 3 and in greys: the real designs' artwork (11 designs, as PNG and as JPEG of quality 55) has
/// none; the test logos' flat art at most 0.8 % (at 4 pixels a millimetre); the test logo with
/// gradients on its fabric, petals and hoop 4.7 to 10.2 %; smooth gradients and gradients under
/// noise of up to ±48 levels 10 to 100 % at 10 pixels a millimetre.
pub(super) const GRADIENT_SHARE: f64 = 0.02;

/// The most rows, and the most columns, of the working copy looked along for crossings, evenly
/// spaced: enough to cross every border a design can sew many times over.
const LINES: usize = 256;

/// The most crossings measured: past it, every so many is, the same ones on every run.
const MAX_CROSSINGS: usize = 65_536;

/// A pixel's grey as the trace's Grayscale mode takes it, Rec. 601 luma rounded to a whole level:
/// (299 R + 587 G + 114 B) / 1000, as upstream's `quantize::luma` computes it.
fn grey(p: [u8; 3]) -> u8 {
    const WEIGHTS: [u32; 3] = [299, 587, 114];
    const WHOLE: u32 = 1000;
    let sum: u32 = p.iter().zip(WEIGHTS).map(|(&c, w)| u32::from(c) * w).sum();
    u8::try_from((sum + WHOLE / 2) / WHOLE).unwrap_or(u8::MAX)
}

/// What is known of one border: its crossings measured, the gradual ones among them, and the
/// difference between the colours either side was traced in.
#[derive(Default)]
struct Border {
    crossings: usize,
    gradual: usize,
    traced_step: Option<f64>,
}

/// The share of the trace's edges, crossed along evenly spaced rows and columns of `regions`, that
/// lie on borders where `raster` changes gradually (this module's measure): 0 when nothing is
/// crossed. In greys mode (`greys`) each pixel is its grey, as the trace takes it.
pub(super) fn gradual_share(raster: &tr::Raster, regions: &Regions, traced: &tr::TraceResult, greys: bool) -> f64 {
    let (w, h) = (regions.w as usize, regions.h as usize);
    let label = |i: usize| regions.labels.get(i).copied().unwrap_or(NONE);
    // Each crossing of an edge between areas `a` and `b`, with the pixels `INSET_PX` inside each,
    // when both areas reach that far: along every so many rows, then every so many columns.
    let crossings = |visit: &mut dyn FnMut(u32, u32, usize, usize)| {
        for (along, lines, len, step) in [(1, h, w, w), (w, w, h, 1)] {
            let spacing = lines.div_ceil(LINES).max(1);
            for line in (0..lines).step_by(spacing) {
                let at = |t: usize| line * step + t * along;
                for t in INSET_PX..len.saturating_sub(INSET_PX) {
                    let (a, b) = (label(at(t - 1)), label(at(t)));
                    if a == b || a == NONE || b == NONE {
                        continue;
                    }
                    let inside = |from: usize, to: usize, l: u32| (from..to).all(|u| label(at(u)) == l);
                    if inside(t - INSET_PX, t, a) && inside(t, t + INSET_PX, b) {
                        visit(a, b, at(t - INSET_PX), at(t + INSET_PX - 1));
                    }
                }
            }
        }
    };
    let mut total = 0usize;
    crossings(&mut |_, _, _, _| total += 1);
    if total == 0 {
        return 0.0;
    }
    let every = total.div_ceil(MAX_CROSSINGS).max(1);
    let colour = |i: usize| -> Option<[u8; 3]> {
        let p = raster.rgba.get(i * 4..i * 4 + 3)?;
        let p = [*p.first()?, *p.get(1)?, *p.get(2)?];
        Some(if greys { [grey(p); 3] } else { p })
    };
    let traced_colour = |l: u32| traced.paths.get(l as usize).map(|p| p.color);
    let mut borders: BTreeMap<(u32, u32), Border> = BTreeMap::new();
    let (mut seen, mut measured) = (0usize, 0usize);
    crossings(&mut |a, b, inside_a, inside_b| {
        seen += 1;
        if (seen - 1) % every != 0 {
            return;
        }
        let border = borders.entry((a.min(b), a.max(b))).or_default();
        let traced_step = *border.traced_step.get_or_insert_with(|| match (traced_colour(a), traced_colour(b)) {
            (Some(x), Some(y)) => delta_e_rgb(x, y),
            _ => 0.0,
        });
        let (Some(x), Some(y)) = (colour(inside_a), colour(inside_b)) else { return };
        // Two areas traced in one colour have no edge to judge.
        if traced_step.is_nan() || traced_step <= 0.0 {
            return;
        }
        measured += 1;
        border.crossings += 1;
        if delta_e_rgb(x, y) < GRADUAL_STEP * traced_step {
            border.gradual += 1;
        }
    });
    let on_gradual: usize =
        borders.values().filter(|b| b.crossings >= MIN_BORDER_CROSSINGS && 2 * b.gradual > b.crossings).map(|b| b.crossings).sum();
    if measured == 0 { 0.0 } else { on_gradual as f64 / measured as f64 }
}
