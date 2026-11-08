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
//!
//! Under a centre line (`super::centre`), sewn on top of what lies beside it, each pixel of the line
//! goes to the colour nearest it outside the lines (an exact distance transform that keeps each
//! pixel's nearest, `super::regions::nearest`), so that each neighbour reaches the line's middle on
//! its side, wherever the line is wider or narrower, and half a pixel past it, so that the two
//! sides meet; never more than [`LINE_REACH_MM`] from its own edge, and none where the pixel lies
//! nearer the background, an area left unsewn or an area sewn after the line (the project owner's
//! choices, 2026-11-07, „Sąsiedzi wchodzą pod linię (polecam)” and „Do środka, najwyżej 2 mm
//! (polecam)”).

use std::collections::BTreeMap;

use vectorcraft_geom::kurbo::{BezPath, ParamCurve, ParamCurveDeriv, PathEl, PathSeg};
use vectorcraft_geom::{FillRule, PathData, Point, Rect, SubPath};
use vectorcraft_pathops::{BoolOp, Cap, Join, outline_stroke, try_boolean, try_unite_all_with};

use super::centre::CentreLine;
use super::colours::Colours;
use super::order::Order;
use super::regions::{NONE, Regions, nearest};
use super::rules::Areas;
use crate::Result;

/// How closely the joined and grown outlines are fitted (working pixels): a hundredth of a pixel, a
/// thousandth of a millimetre at ten pixels a millimetre, the path operations' own default.
const PRECISION_PX: f64 = vectorcraft_pathops::DEFAULT_PRECISION;

/// How far beyond an area's outline the region map is asked which area lies there (working
/// pixels): half a pixel, the centre of the pixel beside the edge.
const BEYOND_PX: f64 = 0.5;

/// How much of a thin neighbour's widest inscribed width a strip may take: half, so that two
/// neighbours growing under it from both sides meet in its middle. The session's choice
/// (2026-11-02), for the owner to confirm.
const THIN_SHARE: f64 = 0.5;

/// How far past its share of a centre line a neighbour reaches (working pixels): half a pixel, so
/// that the two sides of a line meet under its middle rather than at it, and the share, made of
/// whole pixels, covers the line's traced outline, which the trace's anti-aliased edges move by up
/// to half a pixel from the pixels' (measured 2026-11-07: a ring 2 mm wide set to Lines left
/// 1.9 mm² of fabric under its middle when each side reached its middle alone).
const PAST_MIDDLE_PX: f64 = 0.5;

/// The farthest a neighbour reaches under a centre line from its own edge (millimetres): the
/// largest overlap a person may ask for (`super::settings::OVERLAP_RANGE_MM`, `expand`'s 2 mm), so
/// that a colour set to Lines however wide (a field) leaves the colours in it as they are drawn,
/// grown by no more than an overlap grows them (the project owner's choice, 2026-11-07, „Do środka,
/// najwyżej 2 mm (polecam)”).
pub(super) const LINE_REACH_MM: f64 = super::settings::OVERLAP_RANGE_MM.1;

/// The miter limit the strip's widened edge is drawn with: none is drawn, its joins and ends being
/// round, so that the strip reaches no farther than its width from the shared edge anywhere.
const NO_MITER: f64 = 1.0;

/// The shapes of a digitized image's areas.
pub(super) struct Shapes {
    /// Each area's shape (working pixels), by its index; empty for the background's.
    pub(super) shapes: Vec<PathData>,
    /// The areas each area was grown under, by the indices of the areas sewn after it: none where
    /// nothing was grown (no overlap asked, a strip that came to nothing, a shape the path
    /// operations couldn't grow).
    pub(super) grown_under: Vec<Vec<usize>>,
    /// How much the overlap added (square working pixels): the area sewn twice, of the strips
    /// joined to the shapes.
    pub(super) doubled_px: f64,
    /// How many shapes the path operations couldn't join or grow (each left as it was).
    pub(super) failed: usize,
}

/// The union of `paths`, refit to [`PRECISION_PX`]; `None` when the path operations refuse it.
fn united(paths: &[&PathData]) -> Option<PathData> {
    let with_rules: Vec<(&PathData, FillRule)> = paths.iter().map(|p| (*p, FillRule::NonZero)).collect();
    try_unite_all_with(&with_rules, PRECISION_PX).ok()
}

/// The union of `paths` with no path operation: their outlines one after another, which under the
/// non-zero rule is their union exactly when each is wound with its holes against its outlines and
/// all the same way round, as the trace's shapes are (abutting areas' shapes, when the path
/// operations refuse their union) and the path operations' outputs (the strips under one area).
fn side_by_side(paths: &[&PathData]) -> PathData {
    PathData::new(paths.iter().flat_map(|p| p.subpaths.iter().cloned()).collect())
}

/// The area of `p` (square units), its outlines' signed areas added: the trace's outer outlines run
/// one way and its holes the other.
fn area(p: &PathData) -> f64 {
    p.subpaths.iter().map(|sp| sp.signed_area()).sum::<f64>().abs()
}

/// Whether shapes in boxes `a` and `b` may overlap by more than [`PRECISION_PX`]: the boxes overlap
/// by more than it both ways. The strips round neighbouring dots, whose boxes only touch, don't.
fn may_overlap(a: Rect, b: Rect) -> bool {
    a.x1.min(b.x1) - a.x0.max(b.x0) > PRECISION_PX && a.y1.min(b.y1) - a.y0.max(b.y0) > PRECISION_PX
}

/// `path` with the points along each straight run dropped: a line on from the run's start in the
/// direction of its first line, every point within half of [`PRECISION_PX`] of it, so that the
/// run's one line lies within the precision of every point it drops. The trace splits an outline
/// wherever what lies beside it changes, so the edge along a row of specks that joined one area
/// keeps a point at each of them (a serpentine line between 111,452 specks keeps 222,608): a strip
/// is cut from the outline straightened, the same shape to the precision, a few lines instead of
/// thousands.
pub(super) fn straightened(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    // The run of lines not yet written: its start, its direction and its end.
    let mut run: Option<(Point, vectorcraft_geom::Vec2, Point)> = None;
    // Where the pen is, and where its outline started (where a close takes it back to).
    let (mut at, mut start) = (Point::ZERO, Point::ZERO);
    for el in path.elements() {
        if let PathEl::LineTo(p) = *el {
            let on = |(s, d, e): (Point, vectorcraft_geom::Vec2, Point)| {
                let len = d.hypot();
                len > 0.0 && (d.cross(p - s) / len).abs() <= PRECISION_PX / 2.0 && (p - e).dot(d) > 0.0
            };
            match run {
                Some((s, d, e)) if on((s, d, e)) => run = Some((s, d, p)),
                _ => {
                    if let Some((_, _, e)) = run.take() {
                        out.line_to(e);
                    }
                    run = Some((at, p - at, p));
                }
            }
            at = p;
            continue;
        }
        if let Some((_, _, e)) = run.take() {
            out.line_to(e);
        }
        out.push(*el);
        at = match *el {
            PathEl::MoveTo(p) => {
                start = p;
                p
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => p,
            PathEl::ClosePath => start,
        };
    }
    if let Some((_, _, e)) = run {
        out.line_to(e);
    }
    out
}

/// `p`'s outlines straightened ([`straightened`]), each with its box.
fn straightened_outlines(p: &PathData) -> Vec<(Rect, SubPath)> {
    let straight = |sp: &SubPath| {
        let mut bez = BezPath::new();
        sp.to_bezpath_into(&mut bez);
        PathData::from_bezpath(&straightened(&bez)).subpaths.into_iter().next()
    };
    p.subpaths.iter().filter_map(|sp| straight(sp).and_then(|s| Some((outline_box(&s)?, s)))).collect()
}

/// The box of an outline's points and handles, which holds all of it.
fn outline_box(sp: &SubPath) -> Option<Rect> {
    let mut points = sp.anchors.iter().flat_map(|a| [a.p, a.h_in, a.h_out]);
    let first = points.next()?;
    Some(points.fold(Rect::from_points(first, first), |r, p| r.union_pt(p)))
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

/// Each centre line's pixels by the area they go to (the module's notes), for each area of
/// `regions` along whose middle a line runs (`line`): the area nearest each pixel outside the
/// lines, when `takes` says it may grow under a line and the pixel lies within `reach_px` of it.
/// A cancel stops it.
fn line_shares(
    regions: &Regions,
    line: &dyn Fn(usize) -> bool,
    takes: &dyn Fn(usize) -> bool,
    reach_px: f64,
) -> Result<Vec<BTreeMap<usize, Vec<usize>>>> {
    let (w, h) = (regions.w as usize, regions.h as usize);
    let labels = &regions.labels;
    let in_line = |l: u32| l != NONE && line(l as usize);
    let outside: Vec<bool> = labels.iter().map(|&l| !in_line(l)).collect();
    let (d2, from) = nearest(&outside, w, h);
    vectorcraft_doc::cancel::check()?;
    let mut shares: Vec<BTreeMap<usize, Vec<usize>>> = vec![BTreeMap::new(); regions.areas.len()];
    for (i, &l) in labels.iter().enumerate() {
        if !in_line(l) || d2.get(i).is_none_or(|&d| d > reach_px * reach_px) {
            continue;
        }
        let Some(a) = from.get(i).copied().flatten().and_then(|j| labels.get(j)).copied().filter(|&a| a != NONE).map(|a| a as usize) else {
            continue;
        };
        if takes(a)
            && let Some(by) = shares.get_mut(l as usize)
        {
            by.entry(a).or_default().push(i);
        }
    }
    Ok(shares)
}

/// The band a neighbour grows under a centre line by: its share of the line's pixels (`pixels`, of
/// a map `w` wide) as their own outline, along the pixels' edges, widened by [`PAST_MIDDLE_PX`];
/// `None` when it has no pixel or the path operations refuse it.
fn share_band(pixels: &[usize], w: usize) -> Option<PathData> {
    let w = w.max(1);
    let (x0, y0) = pixels.iter().fold((usize::MAX, usize::MAX), |(x, y), &i| (x.min(i % w), y.min(i / w)));
    let (x1, y1) = pixels.iter().fold((0, 0), |(x, y), &i| (x.max(i % w + 1), y.max(i / w + 1)));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut mask = vec![false; bw * bh];
    for &i in pixels {
        if let Some(m) = mask.get_mut((i / w - y0) * bw + (i % w - x0)) {
            *m = true;
        }
    }
    let mut outline = BezPath::new();
    for c in vectorcraft_trace::trace_mask(&mask, bw, bh) {
        for l in std::iter::once(&c.outer).chain(&c.holes) {
            let mut pts = l.pts.iter().map(|&(x, y)| Point::new(f64::from(x) + x0 as f64, f64::from(y) + y0 as f64));
            let Some(first) = pts.next() else { continue };
            outline.move_to(first);
            for p in pts {
                outline.line_to(p);
            }
            outline.close_path();
        }
    }
    let share = PathData::from_bezpath(&straightened(&outline));
    let rim = outline_stroke(&share, 2.0 * PAST_MIDDLE_PX, Cap::Round, Join::Round, NO_MITER);
    united(&[&share, &rim])
}

/// The shapes of `areas` (traced as `traced`), sewn in `order`, with `overlap_px` working pixels of
/// overlap. An area sewn along its centre line (`lines`) grows under nothing, and its neighbours
/// grow under it to its middle on their side, whatever the overlap, at most `line_reach_px` (the
/// module's notes): a line is sewn on top of what lies beside it. A cancel stops it between areas.
pub(super) fn shaped(
    traced: &[vectorcraft_trace::TracedPath],
    areas: &Areas,
    colours: &Colours,
    order: &Order,
    overlap_px: f64,
    lines: &[Option<CentreLine>],
    line_reach_px: f64,
) -> Result<Shapes> {
    let n = areas.areas.len();
    let line = |a: usize| lines.get(a).is_some_and(Option::is_some);
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
                && !line(a)
                && !sewn_over
                && let Some(g) = under.get_mut(a)
            {
                g.push(b);
            }
        }
    }
    let mut grown_under: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut doubled_px = 0.0;
    // Each area's outlines straightened ([`straightened`]), with their boxes, made the first time it
    // is grown under: a band is cut from the outlines its box meets alone, an outline beyond the
    // box winding nothing inside it, so that a strip costs its own stretch of edge however many
    // outlines the area has (a field round thousands of dots).
    let mut boxed: Vec<Option<Vec<(Rect, SubPath)>>> = vec![None; n];
    // What strips already lie under each area, from the areas sewn before, each with its box: a
    // later strip there gives way to those it may overlap.
    let mut taken: Vec<Vec<(Rect, PathData)>> = vec![Vec::new(); n];
    let any_line = lines.iter().any(Option::is_some);
    if overlap_px > 0.0 || any_line {
        let shares = if any_line { line_shares(&areas.regions, &line, &|a| sewn(a) && !line(a), line_reach_px)? } else { Vec::new() };
        let map_w = areas.regions.w as usize;
        for &a in &order.sewn {
            vectorcraft_doc::cancel::check()?;
            let Some(later) = under.get(a).filter(|l| !l.is_empty()) else { continue };
            let Some(base) = own.get(a) else { continue };
            // The edges `a` shares with the others, found once when a band is cut along them.
            let mut edges: Option<BTreeMap<usize, Vec<BezPath>>> = None;
            let (mut strips, mut reached, mut strip_px): (Vec<PathData>, Vec<usize>, f64) = (Vec::new(), Vec::new(), 0.0);
            for &b in later {
                let (Some(into), Some(stats), Some(outlines)) = (own.get(b), areas.regions.areas.get(b), boxed.get_mut(b)) else {
                    continue;
                };
                let band = if line(b) {
                    // Under a centre line, `a`'s share of it.
                    let Some(pixels) = shares.get(b).and_then(|s| s.get(&a)) else { continue };
                    let Some(band) = share_band(pixels, map_w) else {
                        failed += 1;
                        continue;
                    };
                    band
                } else {
                    // As far as asked, or half as wide as a thin neighbour, along the edge they share.
                    let width = overlap_px.min(stats.widest_px * THIN_SHARE);
                    let Some(shared) = edges.get_or_insert_with(|| shared_edges(base, a, &areas.regions)).get(&b).filter(|_| width > 0.0) else {
                        continue;
                    };
                    outline_stroke(&joined_edges(shared), 2.0 * width, Cap::Round, Join::Round, NO_MITER)
                };
                let Some(reach) = band.control_bounds() else { continue };
                let outlines = outlines.get_or_insert_with(|| straightened_outlines(into));
                let near = PathData::new(outlines.iter().filter(|(r, _)| r.overlaps(reach)).map(|(_, sp)| sp.clone()).collect());
                let Ok(mut strip) = try_boolean(&band, FillRule::NonZero, &near, FillRule::NonZero, BoolOp::Intersect, PRECISION_PX) else {
                    failed += 1;
                    continue;
                };
                let Some(found) = strip.bounds() else { continue };
                let met: Vec<&PathData> = taken.get(b).into_iter().flatten().filter(|(r, _)| may_overlap(*r, found)).map(|(_, p)| p).collect();
                if !met.is_empty() {
                    match try_boolean(&strip, FillRule::NonZero, &side_by_side(&met), FillRule::NonZero, BoolOp::Difference, PRECISION_PX) {
                        Ok(rest) => strip = rest,
                        Err(_) => {
                            failed += 1;
                            continue;
                        }
                    }
                }
                let Some(strip_box) = strip.bounds() else { continue };
                if let Some(slot) = taken.get_mut(b) {
                    slot.push((strip_box, strip.clone()));
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
    Ok(Shapes { shapes, grown_under, doubled_px, failed })
}
