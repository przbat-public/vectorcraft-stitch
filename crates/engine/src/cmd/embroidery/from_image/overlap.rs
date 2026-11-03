//! The shapes the areas are sewn as: each area's own outline, joined with the specks that joined it,
//! made solid under the islands sewn over it, and grown under the neighbours sewn after it.
//!
//! The overlap: where two colours meet, the one sewn first reaches under the one sewn after by
//! `overlap` millimetres (0.4 by default), only along the edge they share, so that no fabric shows
//! between them when the stitches pull the cloth (the project owner's choice, 2026-11-01: the lower
//! colour slides under the upper, as roof tiles lie over each other by a strip). It is the geometry
//! of print trapping, a strip of one area within a width of another's edge, here of the edge the two
//! share alone: the region map says which stretches of an area's outline border which neighbour,
//! and the strip is those stretches widened, kept inside the neighbour. So it never reaches across
//! a third area to a neighbour beyond it, nor round a corner past where the two meet, nor into the
//! background, an area left unsewn, the image's edge or an area sewn earlier. Each strip is at most
//! half as wide as the area it grows into, so that two neighbours growing under a thin area from
//! both sides meet in its middle; where two still meet under a narrow neck of one, the one sewn
//! later gives way, so that no third layer stacks there. Upstream's Overlapping method (every lower
//! colour whole under every upper one) and `expand` (growth all round, details and the outline too)
//! are not used: the visible edges stay where the picture has them.

use std::collections::BTreeMap;

use vectorcraft_geom::kurbo::{BezPath, ParamCurve, ParamCurveDeriv, PathSeg};
use vectorcraft_geom::{FillRule, PathData, Point};
use vectorcraft_pathops::{BoolOp, Cap, Join, outline_stroke, try_boolean, try_unite_all_with};

use super::colours::Colours;
use super::order::Order;
use super::regions::{NONE, Regions};
use super::rules::Areas;
use crate::Result;

/// How closely the joined and grown outlines are fitted (working pixels): a hundredth of a pixel, a
/// thousandth of a millimetre at ten pixels a millimetre, the path operations' own default.
const PRECISION_PX: f64 = vectorcraft_pathops::DEFAULT_PRECISION;

/// How far beyond an area's outline the region map is asked which area lies there (working
/// pixels): half a pixel, the centre of the pixel beside the edge.
const BEYOND_PX: f64 = 0.5;

/// The miter limit the strip's widened edge is drawn with: none is drawn, its joins and ends being
/// round, so that the strip reaches no farther than its width from the shared edge anywhere.
const NO_MITER: f64 = 1.0;

/// The most edges between two sewn areas the overlap grows along: past them it is left out, with a
/// warning. An estimate: the real designs' artwork and the test logos have at most 43 (a dry run of
/// them takes at most half a second in all), a busy logo of some hundreds of areas a few hundred,
/// while an untrusted image of noise has tens of thousands, a path operation a few for each.
pub(super) const MAX_OVERLAP_EDGES: usize = 2_000;

/// The shapes of a digitized image's areas.
pub(super) struct Shapes {
    /// Each area's shape (working pixels), by its index; empty for the background's.
    pub(super) shapes: Vec<PathData>,
    /// The areas each area was grown under, by the indices of the areas sewn after it: none where
    /// nothing was grown (no overlap asked, the overlap left out, a strip that came to nothing, a
    /// shape the path operations couldn't grow).
    pub(super) grown_under: Vec<Vec<usize>>,
    /// How much the overlap added (square working pixels): the area sewn twice, of the strips
    /// joined to the shapes.
    pub(super) doubled_px: f64,
    /// Whether an overlap asked for was left out (past [`MAX_OVERLAP_EDGES`]), and how many shapes
    /// the path operations couldn't join or grow (each left as it was).
    pub(super) skipped: bool,
    pub(super) failed: usize,
}

/// The union of `paths`, refit to [`PRECISION_PX`]; `None` when the path operations refuse it.
fn united(paths: &[&PathData]) -> Option<PathData> {
    let with_rules: Vec<(&PathData, FillRule)> = paths.iter().map(|p| (*p, FillRule::NonZero)).collect();
    try_unite_all_with(&with_rules, PRECISION_PX).ok()
}

/// The union of areas' shapes `paths` when the path operations refuse it: their outlines one after
/// another, which under the non-zero rule is their union exactly, the areas abutting (none
/// overlaps another) and each wound with its holes against its outlines.
fn side_by_side(paths: &[&PathData]) -> PathData {
    PathData::new(paths.iter().flat_map(|p| p.subpaths.iter().cloned()).collect())
}

/// The area of `p` (square units), its outlines' signed areas added: the trace's outer outlines run
/// one way and its holes the other.
fn area(p: &PathData) -> f64 {
    p.subpaths.iter().map(|sp| sp.signed_area()).sum::<f64>().abs()
}

/// Where along a curve piece of an outline the region map is asked what lies beside it: a quarter,
/// half and three quarters of the way, the piece taken for the neighbour two of them find.
const SAMPLES_ALONG: [f64; 3] = [0.25, 0.5, 0.75];

/// The stretches of area `a`'s outline (`base`, working pixels) that border each other area, as
/// open paths of its own curves, by that area's number: each curve piece of the outline whose
/// sides, half a pixel out, lie in `a` and in that area, as the region map says, at two of
/// [`SAMPLES_ALONG`]; consecutive pieces of one neighbour make one path.
fn shared_edges(base: &PathData, a: usize, regions: &Regions) -> BTreeMap<usize, Vec<BezPath>> {
    let label = |p: Point| {
        if p.x >= 0.0 && p.y >= 0.0 && p.x.is_finite() && p.y.is_finite() { regions.at(p.x as u32, p.y as u32).unwrap_or(NONE) } else { NONE }
    };
    let own = u32::try_from(a).unwrap_or(NONE);
    // The neighbour beside a curve piece at `t`, if one side there is `a`'s and the other another
    // area's.
    let beside = |seg: &PathSeg, t: f64| -> Option<u32> {
        let tangent = match seg {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
        };
        let len = (tangent.x * tangent.x + tangent.y * tangent.y).sqrt();
        if len.is_nan() || len <= 0.0 {
            return None;
        }
        let n = vectorcraft_geom::Vec2::new(-tangent.y, tangent.x) / len * BEYOND_PX;
        let m = seg.eval(t);
        match (label(m + n), label(m - n)) {
            (l, o) | (o, l) if l == own && o != own && o != NONE => Some(o),
            _ => None,
        }
    };
    let mut out: BTreeMap<usize, Vec<BezPath>> = BTreeMap::new();
    for sp in &base.subpaths {
        let bez = PathData::new(vec![sp.clone()]).to_bezpath();
        let mut current: Option<(usize, BezPath)> = None;
        for seg in bez.segments() {
            let found: Vec<u32> = SAMPLES_ALONG.iter().filter_map(|&t| beside(&seg, t)).collect();
            let by = found.iter().copied().find(|&o| found.iter().filter(|&&x| x == o).count() * 2 > SAMPLES_ALONG.len()).map(|o| o as usize);
            match (current.as_mut(), by) {
                (Some((b, path)), Some(o)) if *b == o => path.push(seg.as_path_el()),
                (_, next) => {
                    if let Some((b, path)) = current.take() {
                        out.entry(b).or_default().push(path);
                    }
                    current = next.map(|o| {
                        let mut path = BezPath::new();
                        path.move_to(seg.start());
                        path.push(seg.as_path_el());
                        (o, path)
                    });
                }
            }
        }
        if let Some((b, path)) = current.take() {
            out.entry(b).or_default().push(path);
        }
    }
    out
}

/// `edges` (open paths) as one path.
fn joined_edges(edges: &[BezPath]) -> PathData {
    let mut all = BezPath::new();
    for e in edges {
        all.extend(e.iter());
    }
    PathData::from_bezpath(&all)
}

/// The shapes of `areas` (traced as `traced`), sewn in `order`, with `overlap_px` working pixels of
/// overlap. A cancel stops it between areas.
pub(super) fn shaped(traced: &[vectorcraft_trace::TracedPath], areas: &Areas, colours: &Colours, order: &Order, overlap_px: f64) -> Result<Shapes> {
    let n = areas.areas.len();
    let sewn = |a: usize| areas.areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(super::colours::Colour::sewn);
    let background = |a: usize| areas.areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(|r| r.background);
    let mut failed = 0usize;
    // Each area's own shape: its traced path, joined with those that joined it.
    let mut own: Vec<PathData> = Vec::with_capacity(n);
    for (a, area) in areas.areas.iter().enumerate() {
        vectorcraft_doc::cancel::check()?;
        let paths: Vec<&PathData> = area.paths.iter().filter_map(|&p| traced.get(p)).map(|t| &t.path).collect();
        let shape = match paths.as_slice() {
            _ if background(a) => PathData::default(),
            [one] => (*one).clone(),
            _ => united(&paths).unwrap_or_else(|| {
                failed += 1;
                side_by_side(&paths)
            }),
        };
        own.push(shape);
    }
    // Solid under the islands sewn over it: each area joined, in one union, with every island
    // sewn over it and every island sewn over those in turn (an eye with its pupil), so that only
    // what keeps its hole stays a hole.
    let mut over: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, area) in areas.areas.iter().enumerate() {
        if let (Some(b), true) = (area.inside, area.sewn_over)
            && let Some(o) = over.get_mut(b)
        {
            o.push(a);
        }
    }
    let mut shapes = own.clone();
    for b in 0..n {
        if over.get(b).is_none_or(Vec::is_empty) {
            continue;
        }
        vectorcraft_doc::cancel::check()?;
        let mut solid = vec![b];
        let mut next = 0;
        while let Some(&x) = solid.get(next) {
            next += 1;
            if solid.len() <= n {
                solid.extend(over.get(x).into_iter().flatten().copied());
            }
        }
        let paths: Vec<&PathData> = solid.iter().filter_map(|&x| own.get(x)).collect();
        let joined = united(&paths).unwrap_or_else(|| {
            failed += 1;
            side_by_side(&paths)
        });
        if let Some(slot) = shapes.get_mut(b) {
            *slot = joined;
        }
    }
    // The overlap: each sewn area under the sewn neighbours sewn after it.
    let mut under: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, placed) in order.placed.iter().enumerate() {
        if !sewn(b) {
            continue;
        }
        for &a in &placed.under {
            let sewn_over = areas.areas.get(b).is_some_and(|x| x.inside == Some(a) && x.sewn_over);
            if sewn(a)
                && !sewn_over
                && let Some(g) = under.get_mut(a)
            {
                g.push(b);
            }
        }
    }
    let edges: usize = under.iter().map(Vec::len).sum();
    let skipped = overlap_px > 0.0 && edges > MAX_OVERLAP_EDGES;
    let mut grown_under: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut doubled_px = 0.0;
    // What strips already lie under each area, from the areas sewn before: a later strip there gives
    // way to them.
    let mut taken: Vec<Option<PathData>> = vec![None; n];
    if overlap_px > 0.0 && !skipped {
        for &a in &order.sewn {
            vectorcraft_doc::cancel::check()?;
            let Some(later) = under.get(a).filter(|l| !l.is_empty()) else { continue };
            let Some(base) = own.get(a) else { continue };
            let edges = shared_edges(base, a, &areas.regions);
            let (mut strips, mut reached, mut strip_px): (Vec<PathData>, Vec<usize>, f64) = (Vec::new(), Vec::new(), 0.0);
            for &b in later {
                let (Some(into), Some(stats), Some(lines)) = (own.get(b), areas.regions.areas.get(b), edges.get(&b)) else { continue };
                // As far as asked, or half as wide as a thin neighbour.
                let width = overlap_px.min(stats.widest_px / 2.0);
                let band = outline_stroke(&joined_edges(lines), 2.0 * width, Cap::Round, Join::Round, NO_MITER);
                let Ok(mut strip) = try_boolean(&band, FillRule::NonZero, into, FillRule::NonZero, BoolOp::Intersect, PRECISION_PX) else {
                    failed += 1;
                    continue;
                };
                if let Some(Some(before)) = taken.get(b) {
                    match try_boolean(&strip, FillRule::NonZero, before, FillRule::NonZero, BoolOp::Difference, PRECISION_PX) {
                        Ok(rest) => strip = rest,
                        Err(_) => {
                            failed += 1;
                            continue;
                        }
                    }
                }
                if strip.is_empty() {
                    continue;
                }
                if let Some(slot) = taken.get_mut(b) {
                    *slot = Some(match slot.take() {
                        Some(before) => united(&[&before, &strip]).unwrap_or_else(|| side_by_side(&[&before, &strip])),
                        None => strip.clone(),
                    });
                }
                strip_px += area(&strip);
                strips.push(strip);
                reached.push(b);
            }
            if strips.is_empty() {
                continue;
            }
            let Some(shape) = shapes.get(a) else { continue };
            let mut all: Vec<&PathData> = vec![shape];
            all.extend(strips.iter());
            // Counted only once joined: a shape the path operations can't grow is sewn as it was.
            match (united(&all), shapes.get_mut(a), grown_under.get_mut(a)) {
                (Some(grown), Some(slot), Some(g)) => {
                    *slot = grown;
                    reached.sort_unstable();
                    *g = reached;
                    doubled_px += strip_px;
                }
                _ => failed += 1,
            }
        }
    }
    Ok(Shapes { shapes, grown_under, doubled_px, skipped, failed })
}
