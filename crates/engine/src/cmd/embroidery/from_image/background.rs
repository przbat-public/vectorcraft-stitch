//! The image's background: what lies round the art and is never sewn, the fabric showing there
//! instead.
//!
//! It is found at the image's edge: transparency, when the corners are transparent, or else the
//! colour three of the four corners agree on. That colour is traced like any other (one colour more
//! is asked of the trace for it), so specks inside it join it first; then only its areas that touch
//! the image's edge are left unsewn (the project owner's choice, 2026-11-01). Areas of that colour
//! inside the design, a white eye or the zebra's light stripes, are sewn as a colour of their own.

use vectorcraft_trace as tr;

/// How large each corner's patch is, as a share of the image's shorter side: 2 % (19 pixels on a
/// photo of a fox 941 pixels high). An estimate.
const CORNER_PATCH_SHARE: f64 = 0.02;

/// The fewest pixels a corner's patch is on a side, however small the image. An estimate.
const MIN_CORNER_PATCH_PX: u32 = 3;

/// How far apart (CIEDE2000) two corners' colours may be and still be one background: under a
/// difference seen side by side. An estimate.
pub(super) const CORNERS_AGREE_DELTA_E: f64 = 5.0;

/// How many of the four corners must agree on a colour, or be transparent, for it to be the
/// background: three, so that one corner may hold art. An estimate.
const CORNERS_NEEDED: usize = 3;

/// The opacity below which a pixel is transparent: Image Trace never traces it (upstream's
/// threshold, half of 255).
const TRANSPARENT_ALPHA: u8 = 128;

/// What lies round the art.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Background {
    /// Nothing: every area is sewn (`background: "none"`, or corners that don't agree).
    None,
    /// Transparency, which is never traced.
    Transparent,
    /// A colour (sRGB): the one found at the corners, or given.
    Colour([u8; 3]),
}

/// The median of `values`, which aren't empty (else 0).
fn median(mut values: Vec<u8>) -> u8 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

/// The background of `raster` as its corners show it.
pub(super) fn found(raster: &tr::Raster) -> Background {
    let (w, h) = (raster.width, raster.height);
    if w == 0 || h == 0 {
        return Background::None;
    }
    let side = ((f64::from(w.min(h)) * CORNER_PATCH_SHARE).round() as u32).max(MIN_CORNER_PATCH_PX).min(w).min(h);
    let corners = [(0, 0), (w - side, 0), (0, h - side), (w - side, h - side)];
    // Each corner's median colour, or `None` when most of its patch is transparent.
    let patches: Vec<Option<[u8; 3]>> = corners
        .iter()
        .map(|&(x0, y0)| {
            let pixels: Vec<[u8; 4]> = (y0..y0 + side).flat_map(|y| (x0..x0 + side).map(move |x| (x, y))).map(|(x, y)| raster.pixel(x, y)).collect();
            let opaque: Vec<[u8; 4]> = pixels.iter().copied().filter(|p| p[3] >= TRANSPARENT_ALPHA).collect();
            (opaque.len() * 2 > pixels.len()).then(|| [0, 1, 2].map(|k| median(opaque.iter().map(|p| p[k]).collect())))
        })
        .collect();
    if patches.iter().filter(|p| p.is_none()).count() >= CORNERS_NEEDED {
        return Background::Transparent;
    }
    // The corner most others agree with, and those that do.
    let colours: Vec<[u8; 3]> = patches.into_iter().flatten().collect();
    let agreeing = |c: [u8; 3]| -> Vec<[u8; 3]> {
        colours.iter().copied().filter(|&o| vectorcraft_stitch::color::delta_e_rgb(c, o) <= CORNERS_AGREE_DELTA_E).collect()
    };
    let best = colours.iter().map(|&c| agreeing(c)).max_by_key(Vec::len).unwrap_or_default();
    if best.len() < CORNERS_NEEDED {
        return Background::None;
    }
    Background::Colour([0, 1, 2].map(|k| median(best.iter().map(|c| c[k]).collect())))
}
