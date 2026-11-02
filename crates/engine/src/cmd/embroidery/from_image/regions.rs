//! The region map: the traced areas drawn back into the working copy's pixels, each pixel holding
//! the number of the area it lies in. Abutting areas tile the image, so each pixel lies in one of
//! them (upstream's own test of the trace, `shared_boundaries.rs`, holds it to a thousandth of a
//! pixel), and the map answers what the window, the rules and the overlap ask of the areas:
//! - which areas touch which, and along how much of their edge;
//! - how wide each is at its widest, from an exact distance transform (P. F. Felzenszwalb and D. P.
//!   Huttenlocher, "Distance transforms of sampled functions", *Theory of Computing* 8, 2012);
//! - whether it touches the image's edge or transparency;
//! - the point deepest inside it, which keys it;
//! - the area under any point.
//!
//! Each area is drawn at the pixels' centres, by scanlines across its outline flattened to within
//! [`FLATTEN_TOLERANCE_PX`]. Where two flattened outlines of one shared edge disagree by a hair, a
//! pixel centre lying between them is taken by the area drawn first, or, taken by none, by the
//! neighbouring pixel's area.

use std::collections::BTreeMap;

use vectorcraft_geom::{BezPath, PathEl, Point};
use vectorcraft_trace as tr;

/// The label of a pixel in no area: transparent, never traced.
pub(super) const NONE: u32 = u32::MAX;

/// How closely a traced outline's curves are followed as straight pieces when it is drawn into the
/// map (pixels): a hundredth of a pixel, far finer than a pixel's centre can tell.
const FLATTEN_TOLERANCE_PX: f64 = 0.01;

/// The opacity below which a pixel is transparent, never traced (upstream Image Trace's threshold).
const TRANSPARENT_ALPHA: u8 = 128;

/// A distance larger than any in a working copy (pixels squared), for the distance transform's
/// "no feature here": finite, so that its arithmetic never meets infinity minus infinity.
const FAR: f64 = 1.0e20;

/// What the map knows of one area.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct AreaStats {
    /// Its pixels.
    pub(super) pixels: usize,
    /// The box of its pixels, `[x0, y0, x1, y1]` (x1 and y1 past the last).
    pub(super) bbox: [u32; 4],
    /// Whether a pixel of it lies on the working copy's edge.
    pub(super) touches_edge: bool,
    /// Whether it touches transparency (a pixel in no area beside one of its own).
    pub(super) touches_clear: bool,
    /// Its widest inscribed width (pixels): twice the distance from its deepest pixel to its edge,
    /// and a pixel more when its middle lies between two pixels as deep; to about a pixel on a slant.
    pub(super) widest_px: f64,
    /// Its deepest pixel: the farthest from its edge, the first in scan order of those as deep.
    pub(super) deepest: (u32, u32),
}

/// The region map of a trace.
#[derive(Clone, Debug)]
pub(super) struct Regions {
    pub(super) w: u32,
    pub(super) h: u32,
    /// Each pixel's area (its traced path's place in the trace), or [`NONE`].
    pub(super) labels: Vec<u32>,
    /// Each area's facts, by its place in the trace.
    pub(super) areas: Vec<AreaStats>,
    /// The edge two areas share (pixel edges), each pair once, smaller number first.
    pub(super) borders: BTreeMap<(u32, u32), u32>,
    /// Each area's neighbours by number, with the edge each shares: [`Regions::borders`] by area.
    adjacent: Vec<Vec<(u32, u32)>>,
}

/// The straight pieces of `path`'s outline, each `(from, to)`, flattened.
fn edges(path: &BezPath) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    let (mut start, mut last) = (Point::ZERO, Point::ZERO);
    vectorcraft_geom::kurbo::flatten(path, FLATTEN_TOLERANCE_PX, |el| match el {
        PathEl::MoveTo(p) => {
            if last != start {
                out.push((last, start));
            }
            start = p;
            last = p;
        }
        PathEl::LineTo(p) => {
            out.push((last, p));
            last = p;
        }
        PathEl::ClosePath => {
            if last != start {
                out.push((last, start));
            }
            last = start;
        }
        _ => {}
    });
    if last != start {
        out.push((last, start));
    }
    out
}

/// Draw area `label`, whose outline is `edges`, into `labels` (`w × h`) at the pixels' centres by
/// the non-zero rule, leaving pixels already drawn as they are.
fn draw(labels: &mut [u32], w: u32, h: u32, edges: &[(Point, Point)], label: u32) {
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for (a, b) in edges {
        y0 = y0.min(a.y.min(b.y));
        y1 = y1.max(a.y.max(b.y));
    }
    if !(y0.is_finite() && y1.is_finite()) {
        return;
    }
    let first = (y0 - 0.5).ceil().max(0.0) as u32;
    let last = ((y1 - 0.5).floor().min(f64::from(h) - 1.0)).max(-1.0);
    if last < 0.0 {
        return;
    }
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for y in first..=(last as u32) {
        let cy = f64::from(y) + 0.5;
        crossings.clear();
        for (a, b) in edges {
            // Half open, so a vertex on the scanline counts once.
            let (lo, hi, dir) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };
            if lo.y <= cy && cy < hi.y {
                let t = (cy - lo.y) / (hi.y - lo.y);
                crossings.push((lo.x + t * (hi.x - lo.x), dir));
            }
        }
        crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut winding = 0;
        for pair in crossings.windows(2) {
            winding += pair[0].1;
            if winding == 0 {
                continue;
            }
            // The pixels whose centres lie between the two crossings.
            let from = (pair[0].0 - 0.5).ceil().max(0.0);
            let to = (pair[1].0 - 0.5).ceil().min(f64::from(w));
            // Not `from >= to`: a crossing that isn't a number fills nothing either.
            if from.partial_cmp(&to) != Some(std::cmp::Ordering::Less) {
                continue;
            }
            let row = y as usize * w as usize;
            for x in from as usize..to as usize {
                if let Some(l) = labels.get_mut(row + x).filter(|l| **l == NONE) {
                    *l = label;
                }
            }
        }
    }
}

/// The squared distance from each of `n` samples to the nearest feature, where `f` holds 0 at a
/// feature and [`FAR`] elsewhere (or a squared distance already found): the lower envelope of
/// parabolas, one dimension at a time (the distance transform's paper, §2).
fn transform_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let sq = |q: usize| (q as f64) * (q as f64);
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -FAR;
    z[1] = FAR;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + sq(q)) - (f[p] + sq(p))) / (2.0 * q as f64 - 2.0 * p as f64);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // k is 0: the new parabola is lower everywhere.
            v[0] = q;
            z[0] = -FAR;
            z[1] = FAR;
            continue;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = FAR;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        *out = (q as f64 - p as f64) * (q as f64 - p as f64) + f[p];
    }
}

/// The squared distance (pixels) from each pixel of a `w × h` image to the nearest of `feature`.
fn distances(feature: &[bool], w: usize, h: usize) -> Vec<f64> {
    let mut grid: Vec<f64> = feature.iter().map(|&f| if f { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        transform_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..y * w + w]);
        transform_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        grid[y * w..y * w + w].copy_from_slice(&d[..w]);
    }
    grid
}

impl Regions {
    /// The region map of `traced`'s filled paths over `raster`, its working copy: each path drawn
    /// at the pixels' centres in its order, then each opaque pixel no path took given its
    /// neighbour's area.
    pub(super) fn of(traced: &tr::TraceResult, raster: &tr::Raster) -> Self {
        let (w, h) = (raster.width, raster.height);
        let (wu, hu) = (w as usize, h as usize);
        let mut labels = vec![NONE; wu * hu];
        for (i, p) in traced.paths.iter().enumerate() {
            if p.stroke.is_some() {
                continue;
            }
            let label = u32::try_from(i).unwrap_or(NONE);
            draw(&mut labels, w, h, &edges(&p.path.to_bezpath()), label);
        }
        // Opaque pixels between two outlines that disagree by a hair: their neighbour's area,
        // spreading from the pixels taken.
        let opaque = |i: usize| raster.rgba.get(i * 4 + 3).is_some_and(|&a| a >= TRANSPARENT_ALPHA);
        let mut frontier: Vec<usize> = (0..labels.len()).filter(|&i| labels[i] != NONE).collect();
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for i in frontier {
                let (x, y) = (i % wu, i / wu);
                let around = [(x > 0).then(|| i - 1), (x + 1 < wu).then(|| i + 1), (y > 0).then(|| i - wu), (y + 1 < hu).then(|| i + wu)];
                for j in around.into_iter().flatten() {
                    if labels[j] == NONE && opaque(j) {
                        labels[j] = labels[i];
                        next.push(j);
                    }
                }
            }
            frontier = next;
        }
        Self::from_labels(labels, w, h, traced.paths.len())
    }

    /// The map of `labels` (`w × h`, each pixel's area of `count`, or [`NONE`]), its facts worked
    /// out.
    pub(super) fn from_labels(labels: Vec<u32>, w: u32, h: u32, count: usize) -> Self {
        let (wu, hu) = (w as usize, h as usize);
        let mut areas =
            vec![
                AreaStats { pixels: 0, bbox: [u32::MAX, u32::MAX, 0, 0], touches_edge: false, touches_clear: false, widest_px: 0.0, deepest: (0, 0) };
                count
            ];
        let mut borders: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        // A pixel on its area's edge: beside another area, transparency or the image's edge.
        let mut edge = vec![false; labels.len()];
        for y in 0..hu {
            for x in 0..wu {
                let i = y * wu + x;
                let l = labels[i];
                if l == NONE {
                    continue;
                }
                let Some(a) = areas.get_mut(l as usize) else { continue };
                a.pixels += 1;
                let (xu, yu) = (x as u32, y as u32);
                a.bbox = [a.bbox[0].min(xu), a.bbox[1].min(yu), a.bbox[2].max(xu + 1), a.bbox[3].max(yu + 1)];
                if x == 0 || y == 0 || x + 1 == wu || y + 1 == hu {
                    a.touches_edge = true;
                    edge[i] = true;
                }
                let around = [(x > 0).then(|| i - 1), (x + 1 < wu).then(|| i + 1), (y > 0).then(|| i - wu), (y + 1 < hu).then(|| i + wu)];
                for j in around.into_iter().flatten() {
                    let o = labels[j];
                    if o == l {
                        continue;
                    }
                    edge[i] = true;
                    if o == NONE {
                        a.touches_clear = true;
                    } else if j > i {
                        // Each pair of pixels once: from the one before.
                        *borders.entry((l.min(o), l.max(o))).or_insert(0) += 1;
                    }
                }
            }
        }
        // Each area's widest width: twice its deepest pixel's distance to its edge, plus the half
        // pixel from an edge pixel's centre to the edge itself.
        let d2 = distances(&edge, wu, hu);
        let mut deepest: Vec<(f64, usize)> = vec![(-1.0, 0); count];
        for (i, &l) in labels.iter().enumerate() {
            if let Some(best) = deepest.get_mut(l as usize)
                && d2[i] > best.0
            {
                *best = (d2[i], i);
            }
        }
        for (l, (a, (d, i))) in areas.iter_mut().zip(deepest).enumerate() {
            if d < 0.0 {
                continue;
            }
            let (x, y) = (i % wu, i / wu);
            // Its middle between two pixels as deep (an even width): a pixel wider than either's
            // own distance says.
            let beside = [(x + 1 < wu).then(|| i + 1), (y + 1 < hu).then(|| i + wu)];
            let even = beside.into_iter().flatten().any(|j| labels.get(j).is_some_and(|&o| o as usize == l) && d2.get(j) == Some(&d));
            a.widest_px = 2.0 * (d.sqrt() + 0.5) + if even { 1.0 } else { 0.0 };
            a.deepest = (x as u32, y as u32);
        }
        let mut adjacent: Vec<Vec<(u32, u32)>> = vec![Vec::new(); count];
        for (&(a, b), &n) in &borders {
            if let Some(v) = adjacent.get_mut(a as usize) {
                v.push((b, n));
            }
            if let Some(v) = adjacent.get_mut(b as usize) {
                v.push((a, n));
            }
        }
        for v in &mut adjacent {
            v.sort_unstable();
        }
        Self { w, h, labels, areas, borders, adjacent }
    }

    /// The area under pixel `(x, y)`, if any.
    pub(super) fn at(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.w || y >= self.h {
            return None;
        }
        self.labels.get(y as usize * self.w as usize + x as usize).copied().filter(|&l| l != NONE)
    }

    /// The areas area `a` touches, by number, each with the edge they share (pixel edges).
    pub(super) fn neighbours(&self, a: u32) -> &[(u32, u32)] {
        self.adjacent.get(a as usize).map_or(&[], Vec::as_slice)
    }
}
