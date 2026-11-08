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

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::sync::Arc;

use vectorcraft_geom::{BezPath, PathEl, Point};
use vectorcraft_trace as tr;

use super::background::TRANSPARENT_ALPHA;

/// The label of a pixel in no area: transparent, never traced.
pub(super) const NONE: u32 = u32::MAX;

/// How closely a traced outline's curves are followed as straight pieces when it is drawn into the
/// map (pixels): a hundredth of a pixel, far finer than a pixel's centre can tell.
const FLATTEN_TOLERANCE_PX: f64 = 0.01;

/// A distance larger than any in a working copy (pixels squared), for the distance transform's
/// "no feature here": finite, so that its arithmetic never meets infinity minus infinity.
const FAR: f64 = 1.0e20;

/// A pixel's eight neighbours, and its four side neighbours.
const NEIGHBOURS: [(i64, i64); 8] = [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];
const SIDES: [(i64, i64); 4] = [(1, 0), (0, -1), (-1, 0), (0, 1)];

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
    /// and a pixel more when its middle lies between two pixels as deep across it (an even width);
    /// exact for upright bands and squares, to about a pixel on a slant.
    pub(super) widest_px: f64,
    /// Its width along its middle (pixels): the median of the widths, measured as
    /// [`AreaStats::widest_px`] is, at its ridge (its pixels no neighbour of it is deeper than). A
    /// band's is its width; a drawing of thin lines that cross keeps its lines' width, where its
    /// widest is the disc at a crossing.
    pub(super) ridge_px: f64,
    /// Its key point: of its deepest pixels (the farthest from its edge), the one nearest their
    /// middle (their mean), the first in scan order of those as near; so a long area is keyed at
    /// the middle of its ridge, where a later run that moves its outline by a pixel still finds
    /// it, not at one end.
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
    /// The pixels outside the image (a slanted image's corners), beyond its edge; `None` when every
    /// pixel is the image's.
    pub(super) outside: Option<Arc<[bool]>>,
}

/// [`draw`], for the tests.
#[cfg(test)]
pub(super) fn draw_for_tests(labels: &mut [u32], w: u32, h: u32, edges: &[(Point, Point)], label: u32) {
    draw(labels, w, h, edges, label);
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
    let (first_row, last_row) = (first, last as u32);
    // A path crossing no row's pixel centres fills nothing.
    let Some(rows) = last_row.checked_sub(first_row) else { return };
    // The edges each row meets, by an edge table: each edge listed at the first row whose pixel
    // centre it crosses, and kept in the rows' active list until its last, so that a row meets its
    // own edges, not every edge of a long outline (a field holding thousands of dots has them all).
    let mut starts: Vec<Vec<usize>> = vec![Vec::new(); rows as usize + 1];
    let mut ends: Vec<u32> = vec![0; edges.len()];
    for (k, (a, b)) in edges.iter().enumerate() {
        let (lo, hi) = if a.y < b.y { (a, b) } else { (b, a) };
        if lo.y.is_nan() || hi.y.is_nan() || lo.y >= hi.y {
            continue;
        }
        let (r0, r1) = ((lo.y - 0.5).ceil().max(f64::from(first_row)), ((hi.y - 0.5).ceil() - 1.0).min(f64::from(last_row)));
        if r0 > r1 {
            continue;
        }
        if let (Some(list), Some(end)) = (starts.get_mut((r0 as u32 - first_row) as usize), ends.get_mut(k)) {
            list.push(k);
            *end = r1 as u32;
        }
    }
    let mut active: Vec<usize> = Vec::new();
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for y in first_row..=last_row {
        let cy = f64::from(y) + 0.5;
        if let Some(list) = starts.get((y - first_row) as usize) {
            active.extend_from_slice(list);
        }
        active.retain(|&k| ends.get(k).is_some_and(|&e| e >= y));
        crossings.clear();
        for (a, b) in active.iter().filter_map(|&k| edges.get(k)) {
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
/// parabolas, one dimension at a time (the distance transform's paper, §2). `from` takes, for each
/// sample, the sample whose parabola gives its distance.
fn transform_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64], from: &mut [usize]) {
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
    for (q, (out, at)) in d.iter_mut().zip(from.iter_mut()).enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        *out = (q as f64 - p as f64) * (q as f64 - p as f64) + f[p];
        *at = p;
    }
}

/// The squared distance (pixels) from each pixel of a `w × h` image to the nearest of `feature`.
pub(super) fn distances(feature: &[bool], w: usize, h: usize) -> Vec<f64> {
    nearest(feature, w, h).0
}

/// The squared distance (pixels) from each pixel of a `w × h` image to the nearest of `feature`,
/// and that feature pixel (`None` with no feature in the image): [`transform_1d`] down each column
/// and then along each row, each pixel's nearest feature the one in the column whose distance
/// along the row is least (the distance transform's paper, §2.2, the parabolas' feet kept).
pub(super) fn nearest(feature: &[bool], w: usize, h: usize) -> (Vec<f64>, Vec<Option<usize>>) {
    let mut grid: Vec<f64> = feature.iter().map(|&f| if f { 0.0 } else { FAR }).collect();
    let n = w.max(h);
    let (mut f, mut d, mut v, mut z, mut at) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1], vec![0usize; n]);
    // Each pixel's nearest feature's row in its column.
    let mut row = vec![0usize; grid.len()];
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        transform_1d(&f[..h], &mut d[..h], &mut v, &mut z, &mut at[..h]);
        for y in 0..h {
            grid[y * w + x] = d[y];
            row[y * w + x] = at[y];
        }
    }
    let mut from: Vec<Option<usize>> = vec![None; grid.len()];
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..y * w + w]);
        transform_1d(&f[..w], &mut d[..w], &mut v, &mut z, &mut at[..w]);
        grid[y * w..y * w + w].copy_from_slice(&d[..w]);
        for x in 0..w {
            // Beyond any feature the distance stays about FAR: no feature.
            if d[x] < FAR / 2.0
                && let Some(&r) = at.get(x).and_then(|&c| row.get(y * w + c))
            {
                from[y * w + x] = Some(r * w + at[x]);
            }
        }
    }
    (grid, from)
}

/// Each of `count` areas' ridge width (pixels; `labels` and `d2`, the squared distances to the
/// areas' edges, of a map `wu × hu`): the median, over the area's pixels no neighbour of its area is
/// deeper than (its ridge), of twice the pixel's distance to the edge and the half pixel beyond, a
/// pixel more where an as deep pixel beside it across the ridge makes it two pixels thick; `None`
/// for an area with no pixel.
fn ridge_widths(labels: &[u32], d2: &[f64], wu: usize, hu: usize, count: usize) -> Vec<Option<f64>> {
    let mut ridge: Vec<Vec<f64>> = vec![Vec::new(); count];
    for (i, &l) in labels.iter().enumerate() {
        let (Some(list), Some(&d)) = (ridge.get_mut(l as usize), d2.get(i)) else { continue };
        let (x, y) = ((i % wu) as i64, (i / wu) as i64);
        let at = |dx: i64, dy: i64| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && (nx as usize) < wu && (ny as usize) < hu).then(|| ny as usize * wu + nx as usize)
        };
        let same = |j: Option<usize>| j.and_then(|j| labels.get(j)) == Some(&l);
        let deeper = NEIGHBOURS.iter().any(|&(dx, dy)| same(at(dx, dy)) && at(dx, dy).and_then(|j| d2.get(j)).is_some_and(|&e| e > d));
        if deeper {
            continue;
        }
        list.push(width_at(labels, d2, wu, hu, i));
    }
    ridge
        .into_iter()
        .map(|mut widths| {
            let mid = widths.len() / 2;
            if widths.is_empty() {
                return None;
            }
            widths.select_nth_unstable_by(mid, f64::total_cmp);
            widths.get(mid).copied()
        })
        .collect()
}

/// The width (pixels) of the shape round pixel `i` of a `w × h` map (`labels`, and `d2`, the squared
/// distances to the areas' edges), measured as [`AreaStats::ridge_px`] measures it along a ridge:
/// twice the pixel's distance to the edge and the half pixel beyond, a pixel more where an as deep
/// pixel beside it across the ridge makes it two pixels thick (an even width).
pub(super) fn width_at(labels: &[u32], d2: &[f64], w: usize, h: usize, i: usize) -> f64 {
    let (Some(&l), Some(&d)) = (labels.get(i), d2.get(i)) else { return 0.0 };
    let (x, y) = ((i % w.max(1)) as i64, (i / w.max(1)) as i64);
    let at = |dx: i64, dy: i64| {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h).then(|| ny as usize * w + nx as usize)
    };
    let deep = |dx: i64, dy: i64| at(dx, dy).is_some_and(|j| labels.get(j) == Some(&l) && d2.get(j) == Some(&d));
    let even = SIDES.iter().any(|&(dx, dy)| deep(dx, dy) && !deep(2 * dx, 2 * dy));
    2.0 * (d.sqrt() + 0.5) + if even { 1.0 } else { 0.0 }
}

/// Whether the ridge through pixel `i` of a `w × h` map is two pixels thick across it (an even
/// width), `deep` telling which pixels are as deep as it in its shape: the pixel one step on is as
/// deep and the one beyond it is not (shallower, or not the shape's), rightwards or downwards.
/// Along a band's ridge every pixel is as deep, so that way never counts. The pixel before `i` is
/// shallower whatever the width when `i` is the first of the deepest in scan order.
fn two_pixels_thick(i: usize, w: usize, h: usize, deep: impl Fn(usize) -> bool) -> bool {
    let (x, y) = (i % w, i / w);
    let thick = |step: usize, room: usize| room >= 1 && deep(i + step) && !(room >= 2 && deep(i + 2 * step));
    thick(1, w - 1 - x) || thick(w, h - 1 - y)
}

/// The widest inscribed width (pixels) of the shape `mask` holds (a `w × h` map, `true` inside),
/// measured as [`AreaStats::widest_px`] measures an area: twice the distance from its deepest
/// pixel to its edge, a pixel more for an even width; 0 for an empty mask. Beyond the map is
/// outside the shape.
pub(super) fn widest_of(mask: &[bool], w: usize, h: usize) -> f64 {
    let inside = |x: usize, y: usize| mask.get(y * w + x).copied().unwrap_or(false);
    // A pixel of the shape beside one that isn't, or on the map's edge.
    let on_edge = |x: usize, y: usize| {
        x == 0 || y == 0 || x + 1 == w || y + 1 == h || !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1)
    };
    let edge: Vec<bool> = (0..mask.len()).map(|i| inside(i % w, i / w) && on_edge(i % w, i / w)).collect();
    let d2 = distances(&edge, w, h);
    let mut deepest: Option<(f64, usize)> = None;
    for (i, (&m, &d)) in mask.iter().zip(&d2).enumerate() {
        if m && deepest.is_none_or(|(best, _)| d > best) {
            deepest = Some((d, i));
        }
    }
    let Some((d, i)) = deepest else { return 0.0 };
    let deep = |j: usize| mask.get(j).copied().unwrap_or(false) && d2.get(j) == Some(&d);
    2.0 * (d.sqrt() + 0.5) + if two_pixels_thick(i, w, h, deep) { 1.0 } else { 0.0 }
}

impl Regions {
    /// The region map of `traced`'s filled paths over `raster`, its working copy: each path drawn
    /// at the pixels' centres in its order, then each opaque pixel no path took given its
    /// neighbour's area. `outside` marks the pixels outside the image, if any. A cancel stops it
    /// between paths.
    pub(super) fn of(traced: &tr::TraceResult, raster: &tr::Raster, outside: Option<Arc<[bool]>>) -> crate::Result<Self> {
        let (w, h) = (raster.width, raster.height);
        let (wu, hu) = (w as usize, h as usize);
        let mut labels = vec![NONE; wu * hu];
        for (i, p) in traced.paths.iter().enumerate() {
            if p.stroke.is_some() {
                continue;
            }
            vectorcraft_doc::cancel::check()?;
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
        Ok(Self::from_labels(labels, w, h, traced.paths.len(), outside))
    }

    /// The map of `labels` (`w × h`, each pixel's area of `count`, or [`NONE`]), its facts worked
    /// out; `outside` marks the pixels outside the image, if any: an area beside one touches the
    /// image's edge, not transparency.
    pub(super) fn from_labels(labels: Vec<u32>, w: u32, h: u32, count: usize, outside: Option<Arc<[bool]>>) -> Self {
        let beyond = |j: usize| outside.as_ref().is_some_and(|o| o.get(j).copied().unwrap_or(false));
        let (wu, hu) = (w as usize, h as usize);
        let mut areas = vec![
            AreaStats {
                pixels: 0,
                bbox: [u32::MAX, u32::MAX, 0, 0],
                touches_edge: false,
                touches_clear: false,
                widest_px: 0.0,
                ridge_px: 0.0,
                deepest: (0, 0)
            };
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
                    if o == NONE && beyond(j) {
                        a.touches_edge = true;
                    } else if o == NONE {
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
        for (l, (a, &(d, i))) in areas.iter_mut().zip(&deepest).enumerate() {
            if d < 0.0 {
                continue;
            }
            let (x, y) = (i % wu, i / wu);
            // Its middle between two pixels as deep across it (an even width): a pixel wider than
            // either's own distance says.
            let deep = |j: usize| labels.get(j).is_some_and(|&o| o as usize == l) && d2.get(j) == Some(&d);
            let even = two_pixels_thick(i, wu, hu, deep);
            a.widest_px = 2.0 * (d.sqrt() + 0.5) + if even { 1.0 } else { 0.0 };
            a.deepest = (x as u32, y as u32);
        }
        // Each area's width along its ridge, which a junction or a blob doesn't widen.
        for (a, ridge) in areas.iter_mut().zip(ridge_widths(&labels, &d2, wu, hu, count)) {
            if let Some(r) = ridge {
                a.ridge_px = r;
            }
        }
        // Each area's key: its deepest pixel nearest the mean of its deepest pixels. The sums are of
        // whole coordinates, exact in floating point.
        let deepest_of = |i: usize, l: u32| deepest.get(l as usize).filter(|&&(d, _)| d >= 0.0 && d2.get(i) == Some(&d)).is_some();
        let mut sums: Vec<(f64, f64, f64)> = vec![(0.0, 0.0, 0.0); count];
        for (i, &l) in labels.iter().enumerate() {
            if deepest_of(i, l)
                && let Some(s) = sums.get_mut(l as usize)
            {
                *s = (s.0 + (i % wu) as f64, s.1 + (i / wu) as f64, s.2 + 1.0);
            }
        }
        let mut nearest: Vec<Option<(f64, usize)>> = vec![None; count];
        for (i, &l) in labels.iter().enumerate() {
            let (Some(&(sx, sy, n)), Some(best)) = (sums.get(l as usize), nearest.get_mut(l as usize)) else { continue };
            if !deepest_of(i, l) || n <= 0.0 {
                continue;
            }
            let (dx, dy) = ((i % wu) as f64 - sx / n, (i / wu) as f64 - sy / n);
            let off = dx * dx + dy * dy;
            if best.is_none_or(|(b, _)| off < b) {
                *best = Some((off, i));
            }
        }
        for (a, key) in areas.iter_mut().zip(nearest) {
            if let Some((_, i)) = key {
                a.deepest = ((i % wu) as u32, (i / wu) as u32);
            }
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
        Self { w, h, labels, areas, borders, adjacent, outside }
    }

    /// The length of each area `of` picks, measured along it (pixels): the longest of the shortest
    /// ways between two of its pixels through its own, a diagonal step √2, a pixel added for the
    /// halves at its two ends; found by two sweeps, from its first pixel to the farthest, then from
    /// that one to the farthest (exact for a line without branches, a tapered one included). 0 for
    /// an area not picked. A cancel stops it between areas.
    pub(super) fn lengths(&self, of: impl Fn(usize) -> bool) -> crate::Result<Vec<f64>> {
        let (w, h) = (self.w as usize, self.h as usize);
        let n = self.areas.len();
        // The pixels of each area picked, in scan order.
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, &l) in self.labels.iter().enumerate() {
            if let Some(m) = members.get_mut(l as usize).filter(|_| of(l as usize)) {
                m.push(i);
            }
        }
        let mut lengths = vec![0.0; n];
        let mut dist: Vec<f64> = vec![f64::INFINITY; self.labels.len()];
        for (a, pixels) in members.iter().enumerate() {
            let Some(&first) = pixels.first() else { continue };
            vectorcraft_doc::cancel::check()?;
            let label = u32::try_from(a).unwrap_or(NONE);
            // The farthest pixel from `start` through the area's own, and how far.
            let mut sweep = |start: usize| {
                for &p in pixels {
                    if let Some(d) = dist.get_mut(p) {
                        *d = f64::INFINITY;
                    }
                }
                let mut heap = BinaryHeap::new();
                if let Some(d) = dist.get_mut(start) {
                    *d = 0.0;
                }
                heap.push(Reverse((0.0f64.to_bits(), start)));
                let mut far = (0.0f64, start);
                // A distance is never negative, so its bits order as it does.
                while let Some(Reverse((bits, p))) = heap.pop() {
                    let d = f64::from_bits(bits);
                    if dist.get(p).is_some_and(|&best| d > best) {
                        continue;
                    }
                    if d > far.0 || (d == far.0 && p < far.1) {
                        far = (d, p);
                    }
                    let (x, y) = (p % w, p / w);
                    for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                        let (Some(nx), Some(ny)) = (x.checked_add_signed(dx), y.checked_add_signed(dy)) else { continue };
                        if nx >= w || ny >= h {
                            continue;
                        }
                        let q = ny * w + nx;
                        if self.labels.get(q) != Some(&label) {
                            continue;
                        }
                        let step = if dx != 0 && dy != 0 { std::f64::consts::SQRT_2 } else { 1.0 };
                        let next = d + step;
                        if let Some(best) = dist.get_mut(q).filter(|best| next < **best) {
                            *best = next;
                            heap.push(Reverse((next.to_bits(), q)));
                        }
                    }
                }
                far
            };
            let (_, end) = sweep(first);
            let (longest, _) = sweep(end);
            if let Some(slot) = lengths.get_mut(a) {
                *slot = longest + 1.0;
            }
        }
        Ok(lengths)
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
