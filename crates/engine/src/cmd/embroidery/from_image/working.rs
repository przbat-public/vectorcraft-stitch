//! The working copy of the image: decoded, cropped and resampled to a resolution in pixels a
//! millimetre chosen for the design's size, so that a threshold in millimetres means the same
//! pixels whatever the picture's own resolution.
//!
//! The resolution is the smallest of three: [`WORKING_PX_PER_MM`], [`MAX_WORKING_SIDE_PX`] over the
//! design's longest side, and the image's own pixels a millimetre at the design's size. An image is
//! never enlarged: the trace reads anti-aliasing to a fraction of a pixel, so an enlarged copy would
//! add only blends. A smaller one is averaged down, each working pixel the mean of the image pixels
//! it covers (premultiplied by their opacity, so that transparent pixels tint nothing).

use std::sync::Arc;

use serde_json::json;

use vectorcraft_doc::ImageObject;
use vectorcraft_geom::{Affine, Point, Vec2};
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_trace as tr;

use super::command::hundredths;
use super::settings::{C, SIZE_RANGE_MM, Settings, Size};
use super::warnings::Warning;
use crate::Result;
use crate::cmd::bad;

/// The working resolution digitizing aims for (pixels a millimetre): ten, the machine formats'
/// grid of a tenth of a millimetre (`vectorcraft_stitch::UNITS_PER_MM`), finer than a needle point
/// is placed.
pub const WORKING_PX_PER_MM: f64 = 10.0;

/// The longest side of the working copy (pixels): upstream Image Trace traces a larger image "but
/// more slowly", as upstream's own message words the public documentation's warning
/// (<https://helpx.adobe.com/illustrator/using/image-trace-results-optimization.html>, as
/// `docs/image-trace-adjustments.md` cites it), which upstream names `LARGE_TRACE_SIDE` in a module
/// the fork can't reach. A design larger than 150 mm is worked at less than ten pixels a
/// millimetre: five at 300 mm.
pub(super) const MAX_WORKING_SIDE_PX: f64 = 1500.0;

/// A count of pixels shifted this far right is in megapixels, as upstream writes its decoder's limit
/// (2²⁰ pixels).
const MEGAPIXEL_SHIFT: u32 = 20;

/// Below this resolution (pixels a millimetre), a millimetre's detail spans fewer than four pixels,
/// so a line's width is known only to about a quarter: the image is digitized, with a warning. An
/// estimate.
pub(super) const COARSE_PX_PER_MM: f64 = 4.0;

/// How near two resolutions must be to count as one (a share of the larger): an image placed at
/// exactly ten pixels a millimetre comes through its transform's floating point a hair under or
/// over, and is still the grid's resolution, not its own.
const SAME_RESOLUTION: f64 = 1e-9;

/// How far an image's axes may lean off square on the page and still be measured along them: its
/// y axis's step along its x axis against its step across it. A transform's floating point leaves
/// square axes a hair off; past this the image is slanted, and its working copy is made square on
/// the page.
const SLANT_TOLERANCE: f64 = 1e-9;

/// The most samples a working pixel of a slanted image takes along each side: enough for an image
/// eight times finer than its working copy to count every pixel. An estimate.
const MAX_SAMPLES_A_SIDE: f64 = 8.0;

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
    /// Of a slanted image, the working pixels outside it (their centres): the corners its slant
    /// leaves round it, the image's edge beyond its pixels; and the crop upright, along its own
    /// axes, whose corners show its background. `None` for an image square on the page, whose
    /// working copy is the crop.
    pub(super) outside: Option<Arc<[bool]>>,
    pub(super) upright: Option<tr::Raster>,
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
    // A pixel's step along each of the image's axes, on the page (points), and the y step split
    // into its part along the x axis (a slant) and its part square across it.
    let (ux, uy) = (length(Vec2::new(a, b)), length(Vec2::new(c, d)));
    let along = (a * c + b * d) / ux;
    let across = xf.determinant().abs() / ux;
    let square = b == 0.0 && c == 0.0;
    let slanted = along.abs() > SLANT_TOLERANCE * across;
    let (crop_w, crop_h) = (f64::from(x1 - x0), f64::from(y1 - y0));
    // The crop as placed, as it lies on the page (millimetres): what `width`, `height` and `fit`
    // measure (the project owner's decision, 2026-11-03). Along the image's axes when they are the
    // page's; turned or slanted, the box round it.
    let placed = if square {
        (crop_w * ux * MM_PER_PT, crop_h * uy * MM_PER_PT)
    } else {
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| xf * Point::new(f64::from(x), f64::from(y)));
        let (l, t, r, bottom) = corners.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |(l, t, r, b), p| {
            (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y))
        });
        ((r - l) * MM_PER_PT, (bottom - t) * MM_PER_PT)
    };
    let (size_mm, scale) = design_size(placed, settings.size)?;
    // The working copy's own extent (millimetres at the design's size): the crop along its axes, or,
    // slanted, along its x axis and square across it, wide enough to hold the slant. A turned image
    // is measured along its own axes, square on the page as they are.
    let frame = if square {
        size_mm
    } else if slanted {
        ((crop_w * ux + crop_h * along.abs()) * MM_PER_PT * scale, crop_h * across * MM_PER_PT * scale)
    } else {
        (crop_w * ux * MM_PER_PT * scale, crop_h * uy * MM_PER_PT * scale)
    };
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
    // The resolution: the smallest of the grid's, the side's and the image's own (along its
    // coarser axis).
    let longest = frame.0.max(frame.1);
    let own = if slanted {
        (sx / (ux * MM_PER_PT * scale)).min(sy / (uy * MM_PER_PT * scale))
    } else {
        (f64::from(cw) / frame.0).min(f64::from(ch) / frame.1)
    };
    let candidates = [(WORKING_PX_PER_MM, Limit::Grid), (MAX_WORKING_SIDE_PX / longest, Limit::Side), (own, Limit::Image)];
    // The smallest; of those within a hair of each other, the first listed.
    let (mut px_per_mm, limit) =
        candidates.into_iter().fold((f64::INFINITY, Limit::Grid), |best, c| if c.0 < best.0 * (1.0 - SAME_RESOLUTION) { c } else { best });
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return Err(bad(C, "the image can't be measured at this size"));
    }
    // Never more pixels than the crop has: the image is not enlarged.
    let side = |mm: f64, max: u32| ((mm * px_per_mm).round().max(1.0) as u32).min(max);
    let (raster, to_image, outside, upright) = if slanted {
        // Square on the page: each working pixel the mean of samples of the image under it, the
        // working copy as wide as the slant, the image's own resolution set by its coarser axis.
        let (ww, wh) = (side(frame.0, u32::MAX), side(frame.1, u32::MAX));
        // A working pixel's side (points, as placed), and where the slant leaves the frame's start.
        let t = 1.0 / (px_per_mm * scale) / MM_PER_PT;
        let x_min = (crop_h * along).min(0.0);
        let to_image = Affine::new([t / ux, 0.0, -t * along / (across * ux), t / across, f64::from(x0) + x_min / ux, f64::from(y0)]);
        let footprint = (sx * (t / ux + (t * along / (across * ux)).abs())).max(sy * t / across);
        let samples = footprint.ceil().clamp(1.0, MAX_SAMPLES_A_SIDE) as u32;
        let (raster, outside) = resampled_slanted(&decoded, to_image, (sx, sy), crop, (ww, wh), samples)?;
        let upright =
            resampled(&decoded, [dx0, dy0, dx1, dy1], side(crop_w * ux * MM_PER_PT * scale, cw), side(crop_h * uy * MM_PER_PT * scale, ch))?;
        (raster, to_image, Some(Arc::from(outside)), Some(upright))
    } else {
        let (ww, wh) = (side(frame.0, cw), side(frame.1, ch));
        if limit == Limit::Image {
            // Exactly the image's pixels: the resolution they give.
            px_per_mm = (f64::from(ww) / frame.0).min(f64::from(wh) / frame.1);
        }
        // A working pixel to the image's own pixels (the decoded crop mapped back).
        let to_image =
            Affine::new([f64::from(cw) / f64::from(ww) / sx, 0.0, 0.0, f64::from(ch) / f64::from(wh) / sy, f64::from(dx0) / sx, f64::from(dy0) / sy]);
        (resampled(&decoded, [dx0, dy0, dx1, dy1], ww, wh)?, to_image, None, None)
    };
    // To the page, then scaled about the crop's centre on the page.
    let centre = xf * Point::new((f64::from(x0) + f64::from(x1)) / 2.0, (f64::from(y0) + f64::from(y1)) / 2.0);
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
        outside,
        upright,
        crop,
        image_px: (iw, ih),
        decoded_px: (dw, dh),
    })
}

impl Working {
    /// The picture whose corners show its background: the working copy, or a slanted image's crop
    /// upright.
    pub(super) fn upright(&self) -> &tr::Raster {
        self.upright.as_ref().unwrap_or(&self.raster)
    }

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
    pub(super) fn warnings(&self) -> Vec<Warning> {
        let mut out = vec![];
        if self.px_per_mm < COARSE_PX_PER_MM {
            out.push(Warning::new(
                "coarse",
                json!({"pxPerMm": hundredths(self.px_per_mm), "fewerThan": COARSE_PX_PER_MM}),
                format!(
                    "the image has {:.1} pixels a millimetre at this size, fewer than {COARSE_PX_PER_MM}: details under a millimetre can't be measured; a larger copy of the image digitizes better",
                    self.px_per_mm
                ),
            ));
        }
        if self.decoded_px != self.image_px {
            let most = tr::MAX_PIXELS >> MEGAPIXEL_SHIFT;
            out.push(Warning::new(
                "decodedSmaller",
                json!({"decoded": [self.decoded_px.0, self.decoded_px.1], "pixels": [self.image_px.0, self.image_px.1], "maxMegapixels": most}),
                format!(
                    "the image was read at {} × {} pixels, scaled down from {} × {} ({most} megapixels at most)",
                    self.decoded_px.0, self.decoded_px.1, self.image_px.0, self.image_px.1
                ),
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
        let byte = |v: f64| v.round().clamp(0.0, f64::from(u8::MAX)) as u8;
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
/// A cancel stops it between rows.
pub(super) fn resampled(src: &tr::Raster, rect: [u32; 4], w: u32, h: u32) -> Result<tr::Raster> {
    let [x0, y0, x1, y1] = rect;
    let (cw, ch) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    let (w, h) = (w.clamp(1, cw.max(1)), h.clamp(1, ch.max(1)));
    if (w, h) == (cw, ch) {
        // The crop as it is.
        return Ok(tr::Raster::from_fn(w, h, |x, y| src.pixel(x0 + x, y0 + y)));
    }
    let (across, down) = (spans(cw, w), spans(ch, h));
    let area = (f64::from(cw) / f64::from(w)) * (f64::from(ch) / f64::from(h));
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for rows in &down {
        vectorcraft_doc::cancel::check()?;
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
    Ok(tr::Raster::new(w, h, rgba).unwrap_or_else(|_| tr::Raster::from_fn(w, h, |_, _| [0; 4])))
}

/// A slanted image's working copy, square on the page: `size` pixels, each `(x, y)` mapped to the
/// image's own pixels by `to_image`, the mean of `samples × samples` points spread evenly over it
/// (read from `src`, the image decoded at `scale` times its own pixels), each weighted by its
/// opacity, a point outside `crop` (the image's own pixels) transparent. And which pixels' centres
/// lie outside the crop. A cancel stops it between rows.
fn resampled_slanted(
    src: &tr::Raster,
    to_image: Affine,
    scale: (f64, f64),
    crop: [u32; 4],
    size: (u32, u32),
    samples: u32,
) -> Result<(tr::Raster, Vec<bool>)> {
    let ([x0, y0, x1, y1], (sx, sy), (w, h)) = (crop.map(f64::from), scale, size);
    let inside = |p: Point| p.x >= x0 && p.x < x1 && p.y >= y0 && p.y < y1;
    let (last_x, last_y) = (f64::from(src.width.saturating_sub(1)), f64::from(src.height.saturating_sub(1)));
    let read = |p: Point| -> [u8; 4] {
        if !inside(p) {
            return [0; 4];
        }
        src.pixel((p.x * sx).floor().clamp(0.0, last_x) as u32, (p.y * sy).floor().clamp(0.0, last_y) as u32)
    };
    let n = f64::from(samples);
    let (mut rgba, mut outside) = (Vec::with_capacity(w as usize * h as usize * 4), Vec::with_capacity(w as usize * h as usize));
    for y in 0..h {
        vectorcraft_doc::cancel::check()?;
        for x in 0..w {
            let mut sum = Sum::default();
            for j in 0..samples {
                for i in 0..samples {
                    let at = Point::new(f64::from(x) + (f64::from(i) + 0.5) / n, f64::from(y) + (f64::from(j) + 0.5) / n);
                    sum.add(read(to_image * at), 1.0);
                }
            }
            rgba.extend_from_slice(&sum.pixel(n * n));
            outside.push(!inside(to_image * Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5)));
        }
    }
    // As many bytes as `w × h` pixels take, by construction.
    let raster = tr::Raster::new(w, h, rgba).unwrap_or_else(|_| tr::Raster::from_fn(w, h, |_, _| [0; 4]));
    Ok((raster, outside))
}
