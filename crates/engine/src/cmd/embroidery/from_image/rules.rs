//! The rules that turn traced areas into areas to sew:
//! - **The smallest area.** An area smaller than it joins the area it shares the longest edge with,
//!   smallest first, as Image Trace's own noise removal does (a speck's true colour is most likely
//!   its surroundings'); one with nothing to join (only transparency round it) is left out, since
//!   a lone speck would cost a jump, its ties and a cut. The trace removes most specks itself; this
//!   catches what it keeps (the Automatic palette keeps a rare colour's regions of a few pixels
//!   whatever its noise setting is) and what the trace is told to keep for the next rule.
//! - **Long thin lines stay**, whatever their area (the project owner's choice, 2026-11-01): a
//!   hairline weighs less than a crumb, but it is drawn. A line is narrower than [`THIN_LINE_MM`]
//!   along its middle (at most [`JUNCTION_ALLOWANCE`] times wider where its lines meet) and,
//!   measured along it, at least [`LINE_KEEP_LENGTH_MM`] long (the owner's, 2026-11-03), so that a
//!   stroke tapering to a point counts its whole length. It is sewn along its centre line
//!   (`super::centre`).
//! - **Bands a thread wide join, whatever their size.** A band no wider than a thread
//!   ([`SLIVER_MAX_MM`], the project owner's choice, 2026-11-03) whose colour is a neighbour's,
//!   near enough ([`SLIVER_DELTA_E`]), or a mix of two neighbours' ([`is_mix`]), is no drawn line
//!   but what a picture leaves along an edge: the darker or lighter band a JPEG rings round a sharp
//!   edge, the blend anti-aliasing paints between two colours, a pixel or three wide. It joins the
//!   neighbour nearest its colour. A wider thin area in an in-between colour (a grey outline round
//!   black on white) is drawn on purpose: the line rule keeps it when it is long.
//!
//! The background is never sewn, and areas join it as they join any area: a speck in it vanishes.
//! What lies inside what, and which islands are sewn over the area round them, is decided once
//! the colours have their threads and the areas their order (`super::inside`).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::thread::MISMATCH_DELTA_E;

use super::colours::Colours;
use super::regions::Regions;

/// The narrowest a long area may be and still be no line (millimetres): the narrowest satin column
/// the planner sews (`plan::SATIN_MIN_WIDTH`, from Melco's design checker); narrower, the planner
/// sews it as a running line along its middle.
pub(super) const THIN_LINE_MM: f64 = vectorcraft_stitch::plan::SATIN_MIN_WIDTH;

/// How much wider than its lines a drawing of lines may be where they meet and still be lines: √2,
/// a right-angle crossing's largest disc as wide as its lines' diagonal (the research's geometry:
/// a T's is 1.26 times its lines, a Y's 1.16, a crossing's 1.41). A line is narrower than
/// [`THIN_LINE_MM`] along its middle (`AreaStats::ridge_px`), and at most this much wider at its
/// widest; the session's choice, for the owner to confirm.
pub(super) const JUNCTION_ALLOWANCE: f64 = std::f64::consts::SQRT_2;

/// How long a thin area must be, measured along it, to stay below the smallest area (millimetres):
/// a line, not a speck. The project owner's choice (2026-11-03), taken from the research's estimate
/// of two stitches of a running line at their shortest.
pub(super) const LINE_KEEP_LENGTH_MM: f64 = 2.0;

/// The widest a band may be and join a neighbour for its colour (millimetres): a thread's width
/// (0.4 mm for 40 wt, `plan::THREAD_WIDTH`), the project owner's choice (2026-11-03). Anti-aliasing
/// and a JPEG's rings are a pixel or three wide, under it at the working resolution; a band wider
/// is drawn on purpose.
pub(super) const SLIVER_MAX_MM: f64 = vectorcraft_stitch::plan::THREAD_WIDTH;

/// How near a band's colour may lie to a neighbour's (CIEDE2000) and be part of it: within the
/// line past which a thread looks unlike its colour (`thread::MISMATCH_DELTA_E`). A JPEG rings
/// round a sharp edge with a darker or lighter copy of the colour beside it, nearer it than that.
pub(super) const SLIVER_DELTA_E: f64 = MISMATCH_DELTA_E;

/// How far a colour may lie from the line between two others (sRGB levels) and be their mix, and
/// the shares of the way along it it must lie between: upstream Image Trace's own test of the
/// blends anti-aliasing leaves between two colours (`quantize/rare.rs`: within 12 levels of the
/// line, at least 0.12 of the way from each end, about an eighth).
const MIX_OFF_LEVELS: f64 = 12.0;
const MIX_ALONG: (f64, f64) = (0.12, 0.88);

/// Whether colour `c` lies between `a` and `b` as anti-aliasing mixes them ([`MIX_OFF_LEVELS`],
/// [`MIX_ALONG`]).
pub(super) fn is_mix(c: [u8; 3], a: [u8; 3], b: [u8; 3]) -> bool {
    let f = |v: [u8; 3]| v.map(f64::from);
    let (c, a, b) = (f(c), f(a), f(b));
    let d = [0, 1, 2].map(|k| b[k] - a[k]);
    let len2: f64 = d.iter().map(|v| v * v).sum();
    if len2 <= 0.0 {
        return false;
    }
    let t = (0..3).map(|k| (c[k] - a[k]) * d[k]).sum::<f64>() / len2;
    let off2: f64 = (0..3).map(|k| (c[k] - a[k] - t * d[k]).powi(2)).sum();
    (MIX_ALONG.0..=MIX_ALONG.1).contains(&t) && off2 < MIX_OFF_LEVELS * MIX_OFF_LEVELS
}

/// What an area is sewn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AreaKind {
    /// A filled area.
    Fill,
    /// A thin line kept below the smallest area: sewn along its middle.
    Line,
}

impl AreaKind {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Line => "line",
        }
    }
}

/// An area to sew (or to leave unsewn): one traced path, or several joined.
#[derive(Clone, Debug)]
pub(super) struct Area {
    /// Its traced paths (their places in the trace): its own first, then those that joined it.
    pub(super) paths: Vec<usize>,
    /// Its row in the colour list.
    pub(super) row: usize,
    /// Its pixels, those that joined it included.
    pub(super) pixels: usize,
    pub(super) kind: AreaKind,
    /// The area it lies inside alone (its place in the areas: `super::inside`), whether it is
    /// narrow enough to be sewn over it, with what it holds filled in, and whether it is.
    pub(super) inside: Option<usize>,
    pub(super) narrow: bool,
    pub(super) sewn_over: bool,
}

/// The areas of a trace after the rules, and its region map with each pixel's area.
pub(super) struct Areas {
    pub(super) areas: Vec<Area>,
    /// The region map, its labels now the areas' places in [`Areas::areas`].
    pub(super) regions: Regions,
    /// How many specks and bands joined another area (the bands among them counted again in
    /// [`Areas::bands`]), and how many specks were left out with nothing to join.
    pub(super) joined: usize,
    pub(super) bands: usize,
    pub(super) dropped: usize,
}

/// The group a path ended in, following joins.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let grand = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = grand;
        }
        i = p;
    }
    i
}

/// The rules applied to the areas of `regions` (one a traced path), whose colours `colours` lists
/// (`row_of` each path's row), at `px_per_mm`, keeping `min_area_px` pixels at least. A cancel
/// stops it.
pub(super) fn applied(regions: Regions, colours: &Colours, row_of: &[usize], px_per_mm: f64, min_area_px: usize) -> crate::Result<Areas> {
    let n = regions.areas.len();
    let background = |i: usize| row_of.get(i).and_then(|&r| colours.rows.get(r)).is_some_and(|r| r.background);
    let thin_px = THIN_LINE_MM * px_per_mm;
    let long_px = LINE_KEEP_LENGTH_MM * px_per_mm;
    let band_px = SLIVER_MAX_MM * px_per_mm;
    // A line: narrower than a satin column along its middle (a little wider where its lines meet),
    // and long enough along it not to be a speck.
    let thin = |i: usize| regions.areas.get(i).is_some_and(|a| a.pixels > 0 && a.ridge_px < thin_px && a.widest_px < thin_px * JUNCTION_ALLOWANCE);
    let lengths = regions.lengths(thin)?;
    let is_line = |i: usize| thin(i) && lengths.get(i).is_some_and(|&l| l >= long_px);
    let colour = |i: usize| row_of.get(i).and_then(|&r| colours.rows.get(r)).map(|r| r.color);
    // Each area's neighbours, as traced.
    let mut around: Vec<Vec<(usize, u32)>> = vec![Vec::new(); n];
    for (&(a, b), &len) in &regions.borders {
        if let Some(v) = around.get_mut(a as usize) {
            v.push((b as usize, len));
        }
        if let Some(v) = around.get_mut(b as usize) {
            v.push((a as usize, len));
        }
    }
    // A sliver: a band no wider than a thread, its colour a neighbour's or a mix of two.
    let is_sliver = |i: usize| {
        let (Some(stats), Some(c), Some(next)) = (regions.areas.get(i), colour(i), around.get(i)) else { return false };
        if stats.widest_px > band_px {
            return false;
        }
        // Its neighbours' colours, each once: no more than the trace's colours, however many
        // neighbours it has (a thin line through a field of dots has thousands).
        let mut near: Vec<[u8; 3]> = next.iter().filter_map(|&(j, _)| colour(j)).collect();
        near.sort_unstable();
        near.dedup();
        near.iter().any(|&o| o != c && delta_e_rgb(c, o) <= SLIVER_DELTA_E)
            || near.iter().enumerate().any(|(k, &x)| near.iter().skip(k + 1).any(|&y| is_mix(c, x, y)))
    };
    let mut sliver: Vec<bool> = Vec::with_capacity(n);
    for i in 0..n {
        vectorcraft_doc::cancel::check()?;
        sliver.push(!background(i) && is_sliver(i));
    }
    let mut parent: Vec<usize> = (0..n).collect();
    let mut pixels: Vec<usize> = regions.areas.iter().map(|a| a.pixels).collect();
    let mut dropped = vec![false; n];
    // Each group's edges with the others, kept as groups join.
    let mut edges: Vec<BTreeMap<usize, u32>> = vec![BTreeMap::new(); n];
    for (&(a, b), &len) in &regions.borders {
        let (a, b) = (a as usize, b as usize);
        if let Some(e) = edges.get_mut(a) {
            *e.entry(b).or_insert(0) += len;
        }
        if let Some(e) = edges.get_mut(b) {
            *e.entry(a).or_insert(0) += len;
        }
    }
    // Slivers, then specks, smallest first; ties by place, so the joins are the same every time.
    let joins = |i: usize, size: usize| !background(i) && (sliver[i] || (size < min_area_px && !is_line(i)));
    let mut heap: BinaryHeap<Reverse<(usize, usize)>> =
        (0..n).filter(|&i| pixels[i] > 0 && joins(i, pixels[i])).map(|i| Reverse((pixels[i], i))).collect();
    let (mut joined, mut bands, mut left_out) = (0usize, 0usize, 0usize);
    while let Some(Reverse((size, i))) = heap.pop() {
        vectorcraft_doc::cancel::check()?;
        // A stale entry: the group grew, or joined another.
        if root(&mut parent, i) != i || pixels[i] != size || dropped[i] {
            continue;
        }
        // A sliver joins the neighbour nearest its colour, a speck the one it shares the longest
        // edge with (ties: the longer edge, then the first).
        let open: Vec<(usize, u32)> = edges[i].iter().filter(|(j, _)| !dropped[**j]).map(|(&j, &len)| (j, len)).collect();
        let target = if sliver[i] {
            let de = |j: usize| colour(i).zip(colour(j)).map_or(f64::INFINITY, |(a, b)| delta_e_rgb(a, b));
            open.iter().min_by(|a, b| de(a.0).total_cmp(&de(b.0)).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0))).map(|&(j, _)| j)
        } else {
            open.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|&(j, _)| j)
        };
        let Some(j) = target else {
            dropped[i] = true;
            left_out += 1;
            continue;
        };
        // Group i joins group j: its pixels and its edges with the others.
        joined += 1;
        if sliver[i] {
            bands += 1;
        }
        parent[i] = j;
        pixels[j] += pixels[i];
        let moved = std::mem::take(&mut edges[i]);
        for (k, len) in moved {
            if k == j {
                continue;
            }
            *edges[j].entry(k).or_insert(0) += len;
            let e = &mut edges[k];
            if let Some(old) = e.remove(&i) {
                *e.entry(j).or_insert(0) += old;
            }
        }
        edges[j].remove(&i);
        if joins(j, pixels[j]) {
            heap.push(Reverse((pixels[j], j)));
        }
    }
    // The groups left, in the trace's order, each an area.
    let mut place: Vec<Option<usize>> = vec![None; n];
    let mut areas: Vec<Area> = Vec::new();
    for i in 0..n {
        if root(&mut parent, i) == i && !dropped[i] && pixels[i] > 0 {
            place[i] = Some(areas.len());
            let kind = if is_line(i) && !sliver[i] { AreaKind::Line } else { AreaKind::Fill };
            areas.push(Area {
                paths: vec![i],
                row: row_of.get(i).copied().unwrap_or(0),
                pixels: pixels[i],
                kind,
                inside: None,
                narrow: false,
                sewn_over: false,
            });
        }
    }
    for i in 0..n {
        let r = root(&mut parent, i);
        if r != i
            && let Some(a) = place.get(r).copied().flatten().and_then(|p| areas.get_mut(p))
        {
            a.paths.push(i);
        }
    }
    // The region map, relabelled by area.
    let mut labels = regions.labels;
    for l in labels.iter_mut() {
        if *l == super::regions::NONE {
            continue;
        }
        let r = root(&mut parent, *l as usize);
        *l = place.get(r).copied().flatten().and_then(|p| u32::try_from(p).ok()).unwrap_or(super::regions::NONE);
    }
    let regions = Regions::from_labels(labels, regions.w, regions.h, areas.len(), regions.outside.clone());
    Ok(Areas { areas, regions, joined, bands, dropped: left_out })
}

/// The areas once the colours have their threads (`colours`): touching areas sewn in one thread
/// joined into one, so that one colour of the design is one area, with no seam through it and no
/// strip sewn twice in one thread. Colours merge into one thread (two sharing their only near
/// thread, the project owner's choice of 2026-11-01, or a merge a person asks for), and a speck
/// joined to one area of a colour can make it touch another of that colour (as in the trace's own
/// noise removal, where a speck merged into a colour joins the regions of it that it touches).
/// Areas left unsewn join only areas of their own colour, and the background's none. A group keeps the paths of every area in it, the row of the
/// colour the others are merged into, and is a line only when every area in it is one. A cancel
/// stops it.
pub(super) fn joined_by_thread(areas: Areas, colours: &Colours) -> crate::Result<Areas> {
    vectorcraft_doc::cancel::check()?;
    let Areas { areas: list, regions, joined, bands, dropped } = areas;
    let n = list.len();
    let rows = &colours.rows;
    // The row a merged row ends in.
    let end = |mut r: usize| {
        let mut hops = 0usize;
        while let Some(next) = rows.get(r).and_then(|x| x.merged_into) {
            hops += 1;
            if hops > rows.len() {
                break;
            }
            r = next;
        }
        r
    };
    // What two areas must share to be one: a sewn area's thread, an unsewn one's colour.
    let key = |a: usize| -> Option<(bool, usize)> {
        let row_index = list.get(a)?.row;
        let row = rows.get(row_index)?;
        if row.background {
            None
        } else if row.sewn() {
            row.thread.as_ref().map(|m| (true, m.thread.index))
        } else {
            Some((false, end(row_index)))
        }
    };
    let mut parent: Vec<usize> = (0..n).collect();
    let mut any = false;
    for &(a, b) in regions.borders.keys() {
        let (a, b) = (a as usize, b as usize);
        if a < n && b < n && key(a).is_some() && key(a) == key(b) {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            if ra != rb
                && let Some(slot) = parent.get_mut(ra.max(rb))
            {
                *slot = ra.min(rb);
                any = true;
            }
        }
    }
    if !any {
        return Ok(Areas { areas: list, regions, joined, bands, dropped });
    }
    // Each group an area, in the order of its first member; the row of the colour the others are
    // merged into (unmerged first, then the largest).
    let mut place: Vec<Option<usize>> = vec![None; n];
    let mut out: Vec<Area> = Vec::new();
    for (a, area) in list.iter().enumerate() {
        if root(&mut parent, a) == a
            && let Some(slot) = place.get_mut(a)
        {
            *slot = Some(out.len());
            out.push(Area { paths: Vec::new(), pixels: 0, inside: None, narrow: false, sewn_over: false, ..area.clone() });
        }
    }
    let mut best: Vec<Option<(bool, usize)>> = vec![None; out.len()];
    for (a, area) in list.iter().enumerate() {
        let r = root(&mut parent, a);
        let Some(g) = place.get(r).copied().flatten() else { continue };
        let (Some(group), Some(pick)) = (out.get_mut(g), best.get_mut(g)) else { continue };
        group.paths.extend(area.paths.iter().copied());
        group.pixels += area.pixels;
        if area.kind == AreaKind::Fill {
            group.kind = AreaKind::Fill;
        }
        let rank = (rows.get(area.row).is_some_and(|x| x.merged_into.is_none()), area.pixels);
        if pick.is_none_or(|p| rank > p) {
            *pick = Some(rank);
            group.row = area.row;
        }
    }
    let mut labels = regions.labels;
    for l in labels.iter_mut() {
        if *l == super::regions::NONE {
            continue;
        }
        let r = root(&mut parent, *l as usize);
        *l = place.get(r).copied().flatten().and_then(|p| u32::try_from(p).ok()).unwrap_or(super::regions::NONE);
    }
    let regions = Regions::from_labels(labels, regions.w, regions.h, out.len(), regions.outside.clone());
    Ok(Areas { areas: out, regions, joined, bands, dropped })
}
