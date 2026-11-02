//! The rules that turn traced areas into areas to sew:
//! - **The smallest area.** An area smaller than it joins the area it shares the longest edge with,
//!   smallest first, as Image Trace's own noise removal does (a speck's true colour is most likely
//!   its surroundings'); one with nothing to join (only transparency round it) is left out, since
//!   a lone speck would cost a jump, its ties and a cut. The trace removes most specks itself; this
//!   catches what it keeps (the Automatic palette keeps a rare colour's regions of a few pixels
//!   whatever its noise setting is) and what the trace is told to keep for the next rule.
//! - **Long thin lines stay**, whatever their area, as long as they are lines (narrower than
//!   [`THIN_LINE_MM`]) at least [`LINE_KEEP_LENGTH_MM`] long: a hairline weighs less than a crumb,
//!   but it is drawn (the project owner's choice, 2026-11-01). The planner sews a part that narrow
//!   as a running line along its middle.
//! - **Slivers join, whatever their size.** A thin area whose colour is a neighbour's, near enough
//!   ([`SLIVER_DELTA_E`]), or a mix of two neighbours' ([`is_mix`]), is no drawn line but what a
//!   picture leaves along an edge: the darker or lighter band a JPEG rings round a sharp edge, the
//!   blend anti-aliasing paints between two colours. It joins the neighbour nearest its colour.
//! - **Islands.** An area lying inside one other area alone is sewn after it; one narrower than
//!   [`ISLAND_SEWN_OVER_MM`] is sewn on top of it, with no hole cut for it (the project owner's
//!   choice): a dot or an eye is sewn over the fill round it, as the person's real designs do.
//!
//! The background is never sewn, and areas join it as they join any area: a speck in it vanishes.

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

/// How long a thin area must be to stay below the smallest area (millimetres): a line, not a speck.
/// An estimate (two stitches of a running line at their shortest).
pub(super) const LINE_KEEP_LENGTH_MM: f64 = 2.0;

/// The widest an island may be and be sewn over the area round it, with no hole cut (millimetres):
/// a fill's longest stitch (Wilcom's tatami default, `plan::FILL_STITCH`), so that the fill round
/// an island has no rows too short to sew. The person's real designs sew details 2.5 to 5.4 mm wide
/// over the colour beneath. An estimate.
pub(super) const ISLAND_SEWN_OVER_MM: f64 = 4.0;

/// How near a thin area's colour may lie to a neighbour's (CIEDE2000) and be part of it: within the
/// line past which a thread looks unlike its colour (`thread::MISMATCH_DELTA_E`). A JPEG rings
/// round a sharp edge with a darker or lighter copy of the colour beside it, nearer it than that.
pub(super) const SLIVER_DELTA_E: f64 = MISMATCH_DELTA_E;

/// How far a colour may lie from the line between two others (sRGB levels) and be their mix, and
/// the shares of the way along it it must lie between: upstream Image Trace's own test of the
/// blends anti-aliasing leaves between two colours (`quantize/rare.rs`: within 12 levels of the
/// line, at least an eighth of the way from each end).
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
    /// The area it lies inside alone (its place in the areas), and whether it is sewn over it.
    pub(super) inside: Option<usize>,
    pub(super) sewn_over: bool,
}

/// The areas of a trace after the rules, and its region map with each pixel's area.
pub(super) struct Areas {
    pub(super) areas: Vec<Area>,
    /// The region map, its labels now the areas' places in [`Areas::areas`].
    pub(super) regions: Regions,
    /// How many specks joined another area, and how many were left out with nothing to join.
    pub(super) joined: usize,
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
/// (`row_of` each path's row), at `px_per_mm`, keeping `min_area_px` pixels at least.
pub(super) fn applied(regions: Regions, colours: &Colours, row_of: &[usize], px_per_mm: f64, min_area_px: usize) -> Areas {
    let n = regions.areas.len();
    let background = |i: usize| row_of.get(i).and_then(|&r| colours.rows.get(r)).is_some_and(|r| r.background);
    let thin_px = THIN_LINE_MM * px_per_mm;
    let long_px = LINE_KEEP_LENGTH_MM * px_per_mm;
    // A line: narrower than a satin column, long enough not to be a speck.
    let is_line =
        |i: usize| regions.areas.get(i).is_some_and(|a| a.pixels > 0 && a.widest_px < thin_px && a.pixels as f64 / a.widest_px.max(1.0) >= long_px);
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
    // A sliver: thin, and its colour a neighbour's or a mix of two.
    let is_sliver = |i: usize| {
        let (Some(stats), Some(c), Some(next)) = (regions.areas.get(i), colour(i), around.get(i)) else { return false };
        if stats.widest_px >= thin_px {
            return false;
        }
        let near: Vec<[u8; 3]> = next.iter().filter_map(|&(j, _)| colour(j)).collect();
        near.iter().any(|&o| o != c && delta_e_rgb(c, o) <= SLIVER_DELTA_E)
            || near.iter().enumerate().any(|(k, &x)| near.iter().skip(k + 1).any(|&y| is_mix(c, x, y)))
    };
    let sliver: Vec<bool> = (0..n).map(|i| !background(i) && is_sliver(i)).collect();
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
    let (mut joined, mut left_out) = (0usize, 0usize);
    while let Some(Reverse((size, i))) = heap.pop() {
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
            areas.push(Area { paths: vec![i], row: row_of.get(i).copied().unwrap_or(0), pixels: pixels[i], kind, inside: None, sewn_over: false });
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
    let regions = Regions::from_labels(labels, regions.w, regions.h, areas.len());
    // Islands: an area whose only neighbour is one other area, away from the image's edge and from
    // transparency.
    let island_px = ISLAND_SEWN_OVER_MM * px_per_mm;
    for a in 0..areas.len() {
        let Some(stats) = regions.areas.get(a) else { continue };
        if stats.touches_edge || stats.touches_clear {
            continue;
        }
        if let [(b, _)] = regions.neighbours(a as u32) {
            let b = *b as usize;
            let narrow = stats.widest_px < island_px;
            let sewn_under = areas.get(b).and_then(|o| colours.rows.get(o.row)).is_some_and(super::colours::Colour::sewn);
            if let Some(area) = areas.get_mut(a) {
                area.inside = Some(b);
                area.sewn_over = narrow && sewn_under;
            }
        }
    }
    Areas { areas, regions, joined, dropped: left_out }
}
