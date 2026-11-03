//! The shapes the areas are sewn as: each area's own outline, joined with the specks that joined it,
//! made solid under the islands sewn over it, and grown under the neighbours sewn after it.
//!
//! The overlap: where two colours meet, the one sewn first reaches under the one sewn after by
//! `overlap` millimetres (0.4 by default), only along the edge they share, so that no fabric shows
//! between them when the stitches pull the cloth (the project owner's choice, 2026-11-01: the lower
//! colour slides under the upper, as roof tiles lie over each other by a strip). It is the geometry
//! of print trapping, a strip of one area within a width of another's edge (upstream's
//! `pathops::trap_band`), and it never reaches the background, an area left unsewn, the image's
//! edge or an area sewn earlier. Each edge's strip is at most half as wide as the area it grows
//! into, so that two neighbours growing under a thin area from both sides meet in its middle rather
//! than stacking a third layer. Upstream's Overlapping method (every lower colour whole under every
//! upper one) and `expand` (growth all round, details and the outline too) are not used: the visible
//! edges stay where the picture has them.

use std::collections::BTreeMap;

use vectorcraft_geom::{FillRule, PathData};
use vectorcraft_pathops::{trap_band, try_unite_all_with};

use super::colours::Colours;
use super::order::Order;
use super::rules::Areas;
use crate::Result;

/// How closely the joined and grown outlines are fitted (working pixels): a hundredth of a pixel, a
/// thousandth of a millimetre at ten pixels a millimetre, the path operations' own default.
const PRECISION_PX: f64 = vectorcraft_pathops::DEFAULT_PRECISION;

/// The overlap's strip is as wide across horizontal edges as across vertical ones.
const SAME_BOTH_WAYS: f64 = 1.0;

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
    if overlap_px > 0.0 && !skipped {
        for a in 0..n {
            vectorcraft_doc::cancel::check()?;
            let Some(later) = under.get(a).filter(|l| !l.is_empty()) else { continue };
            let Some(base) = own.get(a) else { continue };
            // The neighbours it grows under, by how far it reaches: as far as asked, or half as wide
            // as a thin one. Areas never overlap, so one strip reaches into all those of one width
            // at once, a path operation a width rather than a neighbour.
            let mut by_width: BTreeMap<u64, (PathData, Vec<usize>)> = BTreeMap::new();
            for &b in later {
                let (Some(into), Some(stats)) = (own.get(b), areas.regions.areas.get(b)) else { continue };
                let width = overlap_px.min(stats.widest_px / 2.0);
                let (paths, of) = by_width.entry(width.to_bits()).or_default();
                paths.subpaths.extend(into.subpaths.iter().cloned());
                of.push(b);
            }
            let (mut strips, mut reached, mut strip_px): (Vec<PathData>, Vec<usize>, f64) = (Vec::new(), Vec::new(), 0.0);
            for (width, (into, of)) in &by_width {
                let strip = trap_band(base, FillRule::NonZero, into, FillRule::NonZero, f64::from_bits(*width), SAME_BOTH_WAYS, PRECISION_PX);
                if !strip.is_empty() {
                    strip_px += area(&strip);
                    strips.push(strip);
                    reached.extend(of);
                }
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
