//! The working copy of the image: decoded, cropped and resampled to a resolution in pixels a
//! millimetre chosen for the design's size, so that a threshold in millimetres means the same
//! pixels whatever the picture's own resolution.
//!
//! The resolution is the smallest of three: [`WORKING_PX_PER_MM`], [`MAX_WORKING_SIDE_PX`] over the
//! design's longest side, and the image's own pixels a millimetre at the design's size. An image is
//! never enlarged: the trace reads anti-aliasing to a fraction of a pixel, so an enlarged copy would
//! add only blends. A smaller one is averaged down, each working pixel the mean of the image pixels
//! it covers (premultiplied by their opacity, so that transparent pixels tint nothing).

use vectorcraft_doc::ImageObject;
use vectorcraft_geom::{Affine, Point, Vec2};
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use super::settings::{C, SIZE_RANGE_MM, Settings, Size};
use crate::Result;
use crate::cmd::bad;

/// The working resolution digitizing aims for (pixels a millimetre): ten, the machine formats'
/// grid of a tenth of a millimetre (`vectorcraft_stitch::UNITS_PER_MM`), finer than a needle point
/// is placed.
pub(super) const WORKING_PX_PER_MM: f64 = 10.0;

/// The longest side of the working copy (pixels): upstream Image Trace traces a larger image "but
/// more slowly", the public documentation's warning
/// (<https://helpx.adobe.com/illustrator/using/image-trace-results-optimization.html>, as
/// `docs/image-trace-adjustments.md` cites it), which upstream names `LARGE_TRACE_SIDE` in a module
/// the fork can't reach. A design larger than 150 mm is worked at less than ten pixels a
/// millimetre: five at 300 mm.
pub(super) const MAX_WORKING_SIDE_PX: f64 = 1500.0;

/// Below this resolution (pixels a millimetre), a millimetre's detail spans fewer than four pixels,
/// so a line's width is known only to about a quarter: the image is digitized, with a warning. An
/// estimate.
pub(super) const COARSE_PX_PER_MM: f64 = 4.0;

/// How near two resolutions must be to count as one (a share of the larger): an image placed at
/// exactly ten pixels a millimetre comes through its transform's floating point a hair under or
/// over, and is still the grid's resolution, not its own.
const SAME_RESOLUTION: f64 = 1e-9;

/// What held the working resolution down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Limit {
    /// [`WORKING_PX_PER_MM`] itself.
    Grid,
    /// [`MAX_WORKING_SIDE_PX`]: the design is large.
    Side,
    /// The image's own pixels: it has fewer a millimetre at this size.
    Image,
}

impl Limit {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Side => "side",
            Self::Image => "image",
        }
    }
}

/// The working copy of an image, and how it maps onto the document.
#[derive(Clone, Debug)]
pub(super) struct Working {
    /// The crop, resampled to [`Working::px_per_mm`].
    pub(super) raster: tr::Raster,
    /// Its resolution (pixels a millimetre, the same both ways).
    pub(super) px_per_mm: f64,
    /// What held the resolution down.
    pub(super) limit: Limit,
    /// The design's size (millimetres): the crop at the size asked.
    pub(super) size_mm: (f64, f64),
    /// How much larger the design is than the crop as placed (1: as placed).
    pub(super) scale: f64,
    /// A working pixel's coordinates to the document's (points): through the image's own pixels
    /// and its transform, then [`Working::scaled`].
    pub(super) to_doc: Affine,
    /// The design's scaling on the page: about the crop's centre by [`Working::scale`]. The image
    /// moved under the design is scaled by it too.
    pub(super) scaled: Affine,
    /// A working pixel's coordinates to the image's own pixels: where a point keys an area.
    pub(super) to_image: Affine,
    /// The crop, in the image's own pixels.
    pub(super) crop: [u32; 4],
    /// The image's own size in pixels, and the size it was decoded at (smaller only past
    /// [`tr::MAX_PIXELS`]).
    pub(super) image_px: (u32, u32),
    pub(super) decoded_px: (u32, u32),
}

/// The length of `v`, the size a unit step along an image axis comes to on the page. By the square
/// root, which every system rounds alike, not `hypot`, which each system's maths library rounds its
/// own way in the last bit.
fn length(v: Vec2) -> f64 {
    (v.x * v.x + v.y * v.y).sqrt()
}

/// The size of the design `settings` ask for from a crop placed `placed` millimetres, and how much
/// larger that is than the crop as placed.
fn design_size(placed: (f64, f64), size: Size) -> Result<((f64, f64), f64)> {
    let k = match size {
        Size::Placed => 1.0,
        Size::Width(w) => w / placed.0,
        Size::Height(h) => h / placed.1,
        Size::Fit(w, h) => (w / placed.0).min(h / placed.1),
    };
    let size = (placed.0 * k, placed.1 * k);
    let (lo, hi) = SIZE_RANGE_MM;
    if !(k.is_finite() && k > 0.0 && [size.0, size.1].iter().all(|v| v.is_finite() && (lo..=hi).contains(v))) {
        return Err(bad(
            C,
            format!(
                "the design would be {:.1} × {:.1} mm, more than {hi} mm a side or nothing at all: give `width`, `height` or `fit` in millimetres",
                size.0, size.1
            ),
        ));
    }
    Ok((size, k))
}

/// The working copy of `image`, whose file is `bytes`, as `settings` ask: decoded (a picture of
/// more than [`tr::MAX_PIXELS`] scaled down to it, as Image Trace takes it), cropped, and resampled
/// to the working resolution. Refused, with why, when the image is empty, its transform maps it to
/// nothing or past every number, its file doesn't decode, or the crop lies outside it.
pub(super) fn prepare(image: &ImageObject, bytes: &[u8], settings: &Settings) -> Result<Working> {
    let (iw, ih) = (image.width, image.height);
    if iw == 0 || ih == 0 {
        return Err(bad(C, "the image has no pixels"));
    }
    let crop = settings.crop.unwrap_or([0, 0, iw, ih]);
    let [x0, y0, x1, y1] = crop;
    if x1 > iw || y1 > ih {
        return Err(bad(C, format!("`crop` {crop:?} lies outside the image's {iw} × {ih} pixels")));
    }
    let xf = image.xf;
    let [a, b, c, d, e, f] = xf.as_coeffs();
    if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) || xf.determinant() == 0.0 {
        return Err(bad(C, "the image is placed with a transform that has no size: it can't be measured"));
    }
    // A pixel's step along each of the image's axes, on the page (points).
    let (ux, uy) = (length(Vec2::new(a, b)), length(Vec2::new(c, d)));
    let placed = (f64::from(x1 - x0) * ux * MM_PER_PT, f64::from(y1 - y0) * uy * MM_PER_PT);
    let (size_mm, scale) = design_size(placed, settings.size)?;
    let decoded = tr::Raster::decode_to_fit(bytes).map_err(|e| bad(C, format!("the image can't be digitized: {e}")))?;
    let (dw, dh) = (decoded.width, decoded.height);
    // The crop in the decoded pixels: the same pixels unless the image was decoded smaller.
    let (sx, sy) = (f64::from(dw) / f64::from(iw), f64::from(dh) / f64::from(ih));
    let span = |lo: u32, hi: u32, k: f64, max: u32| {
        let a = ((f64::from(lo) * k).floor() as u32).min(max.saturating_sub(1));
        let b = ((f64::from(hi) * k).ceil() as u32).clamp(a + 1, max);
        (a, b)
    };
    let (dx0, dx1) = span(x0, x1, sx, dw);
    let (dy0, dy1) = span(y0, y1, sy, dh);
    let (cw, ch) = (dx1 - dx0, dy1 - dy0);
    // The resolution: the smallest of the grid's, the side's and the image's own.
    let longest = size_mm.0.max(size_mm.1);
    let own = (f64::from(cw) / size_mm.0).min(f64::from(ch) / size_mm.1);
    let candidates = [(WORKING_PX_PER_MM, Limit::Grid), (MAX_WORKING_SIDE_PX / longest, Limit::Side), (own, Limit::Image)];
    // The smallest; of those within a hair of each other, the first listed.
    let (mut px_per_mm, limit) =
        candidates.into_iter().fold((f64::INFINITY, Limit::Grid), |best, c| if c.0 < best.0 * (1.0 - SAME_RESOLUTION) { c } else { best });
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return Err(bad(C, "the image can't be measured at this size"));
    }
    // Never more pixels than the crop has: the image is not enlarged.
    let side = |mm: f64, max: u32| ((mm * px_per_mm).round().max(1.0) as u32).min(max);
    let (ww, wh) = (side(size_mm.0, cw), side(size_mm.1, ch));
    if limit == Limit::Image {
        // Exactly the image's pixels: the resolution they give.
        px_per_mm = (f64::from(ww) / size_mm.0).min(f64::from(wh) / size_mm.1);
    }
    let raster = resampled(&decoded, [dx0, dy0, dx1, dy1], ww, wh);
    // A working pixel to the image's own pixels (the decoded crop mapped back), to the page, then
    // scaled about the crop's centre on the page.
    let to_image =
        Affine::new([f64::from(cw) / f64::from(ww) / sx, 0.0, 0.0, f64::from(ch) / f64::from(wh) / sy, f64::from(dx0) / sx, f64::from(dy0) / sy]);
    let centre = xf * Point::new(f64::from(x0 + x1) / 2.0, f64::from(y0 + y1) / 2.0);
    let scaled = Affine::translate(centre.to_vec2()) * Affine::scale(scale) * Affine::translate(-centre.to_vec2());
    Ok(Working {
        raster,
        px_per_mm,
        limit,
        size_mm,
        scale,
        to_doc: scaled * xf * to_image,
        scaled,
        to_image,
        crop,
        image_px: (iw, ih),
        decoded_px: (dw, dh),
    })
}

impl Working {
    /// Where the design lies on the page: the box round the working copy's corners in the document
    /// (millimetres from the document's origin, y down, `[left, top, right, bottom]`).
    pub(super) fn bounds_mm(&self) -> [f64; 4] {
        let (w, h) = (f64::from(self.raster.width), f64::from(self.raster.height));
        let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| self.to_doc * Point::new(x, y));
        let (mut l, mut t, mut r, mut b) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in corners {
            (l, t, r, b) = (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y));
        }
        [l, t, r, b].map(|v| v * MM_PER_PT)
    }

    /// The warnings a reply gives about the working copy: an image too coarse for its size, or one
    /// decoded smaller than it is.
    pub(super) fn warnings(&self) -> Vec<String> {
        let mut out = vec![];
        if self.px_per_mm < COARSE_PX_PER_MM {
            out.push(format!(
                "the image has {:.1} pixels a millimetre at this size, fewer than {COARSE_PX_PER_MM}: details under a millimetre can't be measured; a larger copy of the image digitizes better",
                self.px_per_mm
            ));
        }
        if self.decoded_px != self.image_px {
            out.push(format!(
                "the image was read at {} × {} pixels, scaled down from {} × {} ({} megapixels at most)",
                self.decoded_px.0,
                self.decoded_px.1,
                self.image_px.0,
                self.image_px.1,
                tr::MAX_PIXELS >> 20
            ));
        }
        out
    }
}

/// Premultiplied colour and opacity, summed with weights.
#[derive(Clone, Copy, Default)]
struct Sum {
    rgb: [f64; 3],
    alpha: f64,
}

impl Sum {
    fn add(&mut self, p: [u8; 4], weight: f64) {
        let a = f64::from(p[3]) * weight;
        for (s, &c) in self.rgb.iter_mut().zip(&p[..3]) {
            *s += f64::from(c) * a;
        }
        self.alpha += a;
    }

    fn add_sum(&mut self, o: &Sum, weight: f64) {
        for (s, c) in self.rgb.iter_mut().zip(o.rgb) {
            *s += c * weight;
        }
        self.alpha += o.alpha * weight;
    }

    /// The pixel these sums average to, over a box of `area` source pixels.
    fn pixel(&self, area: f64) -> [u8; 4] {
        let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        if self.alpha <= 0.0 {
            return [0; 4];
        }
        let [r, g, b] = self.rgb.map(|c| byte(c / self.alpha));
        [r, g, b, byte(self.alpha / area)]
    }
}

/// For each of `out` pixels along an axis of `n` source pixels, the source pixels it covers and how
/// much of each (a box filter: each output pixel is `n / out` source pixels wide).
fn spans(n: u32, out: u32) -> Vec<Vec<(u32, f64)>> {
    let k = f64::from(n) / f64::from(out.max(1));
    (0..out)
        .map(|o| {
            let (a, b) = (f64::from(o) * k, f64::from(o + 1) * k);
            let first = a.floor() as u32;
            let last = (b.ceil() as u32).min(n).max(first + 1);
            (first..last)
                .filter_map(|s| {
                    let w = (b.min(f64::from(s + 1)) - a.max(f64::from(s))).max(0.0);
                    (w > 0.0).then_some((s, w))
                })
                .collect()
        })
        .collect()
}

/// The pixels `rect` (`[x0, y0, x1, y1]`, within `src`) of `src`, averaged down to `w × h` (at most
/// the rectangle's own size): a box filter, each source pixel's colour weighted by its opacity.
/// One output row is built at a time from the source rows it covers, so the memory used is a row's.
pub(super) fn resampled(src: &tr::Raster, rect: [u32; 4], w: u32, h: u32) -> tr::Raster {
    let [x0, y0, x1, y1] = rect;
    let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let (w, h) = (w.clamp(1, cw.max(1)), h.clamp(1, ch.max(1)));
    if (w, h) == (cw, ch) {
        // The crop as it is.
        return tr::Raster::from_fn(w, h, |x, y| src.pixel(x0 + x, y0 + y));
    }
    let (across, down) = (spans(cw, w), spans(ch, h));
    let area = (f64::from(cw) / f64::from(w)) * (f64::from(ch) / f64::from(h));
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for rows in &down {
        let mut acc = vec![Sum::default(); w as usize];
        for &(sy, wy) in rows {
            for (out, cols) in acc.iter_mut().zip(&across) {
                let mut row = Sum::default();
                for &(sx, wx) in cols {
                    row.add(src.pixel(x0 + sx, y0 + sy), wx);
                }
                out.add_sum(&row, wy);
            }
        }
        for s in &acc {
            rgba.extend_from_slice(&s.pixel(area));
        }
    }
    // As many bytes as `w × h` pixels take, by construction.
    tr::Raster::new(w, h, rgba).unwrap_or_else(|_| tr::Raster::from_fn(w, h, |_, _| [0; 4]))
}
