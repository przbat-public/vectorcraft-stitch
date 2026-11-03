//! What lies inside what, and which islands are sewn on top of the area round them.
//!
//! An area lies inside another alone when every way from outside to it passes through that one:
//! the other is its immediate dominator in the graph of which areas touch which, rooted outside
//! (the image's edge, transparency and the background, which is never sewn, are one node). So an
//! eye with a pupil lies inside the face round it, and the pupil inside the eye; a letter's
//! counter inside the letter, and the letter inside the field round it.
//!
//! An island narrower than [`ISLAND_SEWN_OVER_MM`] with what it holds filled in (an eye measured
//! with its pupil, a letter with its counter) is sewn on top of the area round it, with no hole
//! cut, as a plain dot is (the project owner's choices, 2026-11-01 and 2026-11-03), when both are
//! sewn and the island is sewn after it. An island not sewn keeps its hole, so that the fabric
//! shows there; one sewn before the area round it (a dot inside a thin outline, which as a line is
//! sewn after every fill) keeps its hole too, and grows under the outline instead.

use super::colours::Colours;
use super::order::Order;
use super::regions::widest_of;
use super::rules::Areas;
use crate::Result;

/// The widest an island may be and be sewn over the area round it, with no hole cut
/// (millimetres): the project owner's choice (2026-11-01, among the recommendations accepted
/// together: "islands under 4 mm are sewn on top, with no hole cut"), measured with what the
/// island holds filled in (2026-11-03). It is a fill's longest stitch (Wilcom's tatami default,
/// `plan::FILL_STITCH`), so that the fill round an island has no rows too short to sew; the
/// person's real designs sew details 2.5 to 5.4 mm wide over the colour beneath.
pub(super) const ISLAND_SEWN_OVER_MM: f64 = 4.0;

/// The most passes the dominators are worked out in: they settle within a few on any graph (the
/// method's own bound is the graph's loop nesting, a handful for areas that touch); past this many
/// the last pass's answer stands. An estimate, far above what a picture needs.
const MAX_DOMINATOR_PASSES: usize = 64;

/// The immediate dominator of each node of an undirected graph from `root` (`adjacent`, each
/// node's neighbours): the nearest node through which every path from the root to it passes, by
/// Cooper, Harvey and Kennedy's iterative method (K. D. Cooper, T. J. Harvey, K. Kennedy, "A
/// Simple, Fast Dominance Algorithm", Software Practice and Experience 4, 2001, its Figure 3):
/// nodes in reverse postorder of a depth-first walk from the root, each one's dominator the
/// nearest common dominator of its neighbours worked out so far, until a pass changes none. `None`
/// for the root and for a node the root doesn't reach.
fn dominators(adjacent: &[Vec<usize>], root: usize) -> Result<Vec<Option<usize>>> {
    let n = adjacent.len();
    const UNSET: usize = usize::MAX;
    let neighbours = |v: usize| adjacent.get(v).map_or(&[][..], Vec::as_slice);
    // The depth-first walk, without recursion: a picture of thousands of areas nests deeper than a
    // thread's stack goes.
    let mut post = vec![UNSET; n];
    let mut postorder: Vec<usize> = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    if let Some(s) = seen.get_mut(root) {
        *s = true;
        stack.push((root, 0));
    }
    while let Some(top) = stack.last_mut() {
        let (v, next) = *top;
        match neighbours(v).get(next) {
            Some(&w) => {
                top.1 += 1;
                if let Some(s) = seen.get_mut(w).filter(|s| !**s) {
                    *s = true;
                    stack.push((w, 0));
                }
            }
            None => {
                if let Some(p) = post.get_mut(v) {
                    *p = postorder.len();
                }
                postorder.push(v);
                stack.pop();
            }
        }
    }
    let mut idom = vec![UNSET; n];
    if let Some(r) = idom.get_mut(root) {
        *r = root;
    }
    let number = |v: usize| post.get(v).copied().unwrap_or(UNSET);
    // The nearest common dominator of two nodes already placed: each walks up from the one further
    // from the root in postorder (the root's number is the highest).
    let intersect = |idom: &[usize], mut a: usize, mut b: usize| {
        let mut steps = 0usize;
        while a != b && steps <= 2 * n {
            while number(a) < number(b) {
                a = idom.get(a).copied().unwrap_or(root);
                steps += 1;
            }
            while number(b) < number(a) {
                b = idom.get(b).copied().unwrap_or(root);
                steps += 1;
            }
        }
        a
    };
    for _ in 0..MAX_DOMINATOR_PASSES {
        vectorcraft_doc::cancel::check()?;
        let mut changed = false;
        for &v in postorder.iter().rev() {
            if v == root {
                continue;
            }
            let mut new = UNSET;
            for &p in neighbours(v) {
                if idom.get(p).is_none_or(|&d| d == UNSET) {
                    continue;
                }
                new = if new == UNSET { p } else { intersect(&idom, p, new) };
            }
            if let Some(d) = idom.get_mut(v)
                && new != UNSET
                && *d != new
            {
                *d = new;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok((0..n).map(|v| idom.get(v).copied().filter(|&d| v != root && d != UNSET)).collect())
}

/// Each area's container (the area it lies inside alone, its immediate dominator: this module's
/// rule) and whether it is narrow enough to be sewn over it, measured with what it holds filled
/// in; the background's areas lie inside nothing. A cancel stops it.
pub(super) fn containers(areas: &mut Areas, colours: &Colours, px_per_mm: f64) -> Result<()> {
    let n = areas.areas.len();
    let root = n;
    let background: Vec<bool> = areas.areas.iter().map(|x| colours.rows.get(x.row).is_some_and(|r| r.background)).collect();
    let node = |a: usize| if background.get(a).copied().unwrap_or(false) { root } else { a };
    let mut adjacent: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
    let mut link = |a: usize, b: usize| {
        if a != b {
            if let Some(v) = adjacent.get_mut(a) {
                v.push(b);
            }
            if let Some(v) = adjacent.get_mut(b) {
                v.push(a);
            }
        }
    };
    for a in 0..n {
        let Some(stats) = areas.regions.areas.get(a) else { continue };
        if stats.touches_edge || stats.touches_clear {
            link(node(a), root);
        }
        for &(b, _) in areas.regions.neighbours(a as u32) {
            if (b as usize) < n {
                link(node(a), node(b as usize));
            }
        }
    }
    for v in &mut adjacent {
        v.sort_unstable();
        v.dedup();
    }
    let idom = dominators(&adjacent, root)?;
    for (a, area) in areas.areas.iter_mut().enumerate() {
        area.inside = idom.get(a).copied().flatten().filter(|&b| b < n && b != a && !background.get(a).copied().unwrap_or(false));
    }
    // What each area holds: the areas whose containers lead to it.
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, area) in areas.areas.iter().enumerate() {
        if let Some(k) = area.inside.and_then(|b| kids.get_mut(b)) {
            k.push(a);
        }
    }
    let island_px = ISLAND_SEWN_OVER_MM * px_per_mm;
    let w = areas.regions.w as usize;
    for a in 0..n {
        vectorcraft_doc::cancel::check()?;
        let Some(stats) = areas.regions.areas.get(a) else { continue };
        let widest = if kids.get(a).is_none_or(Vec::is_empty) {
            stats.widest_px
        } else {
            // Filled in: the area and everything inside it, over its box (everything inside it lies
            // within its outline).
            let mut held = vec![false; n];
            let mut todo = vec![a];
            while let Some(x) = todo.pop() {
                if let Some(h) = held.get_mut(x).filter(|h| !**h) {
                    *h = true;
                    todo.extend(kids.get(x).into_iter().flatten().copied());
                }
            }
            let [x0, y0, x1, y1] = stats.bbox.map(|v| v as usize);
            let (bw, bh) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
            let mask: Vec<bool> = (0..bw * bh)
                .map(|i| {
                    let label = areas.regions.labels.get((y0 + i / bw) * w + x0 + i % bw).copied();
                    label.is_some_and(|l| held.get(l as usize).copied().unwrap_or(false))
                })
                .collect();
            widest_of(&mask, bw, bh)
        };
        if let Some(area) = areas.areas.get_mut(a) {
            area.narrow = widest < island_px;
        }
    }
    Ok(())
}

/// Which islands are sewn on top of their containers in sewing order `order` (this module's rule):
/// narrow, both sewn by their colours' final jobs, the island after its container.
pub(super) fn sewn_over(areas: &mut Areas, colours: &Colours, order: &Order) {
    let sewn = |a: usize, areas: &Areas| areas.areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(super::colours::Colour::sewn);
    let index = |a: usize| order.placed.get(a).and_then(|p| p.index);
    let over: Vec<bool> = (0..areas.areas.len())
        .map(|a| {
            let Some(area) = areas.areas.get(a) else { return false };
            let Some(b) = area.inside else { return false };
            let after = matches!((index(a), index(b)), (Some(i), Some(j)) if i > j);
            area.narrow && sewn(a, areas) && sewn(b, areas) && after
        })
        .collect();
    for (area, over) in areas.areas.iter_mut().zip(over) {
        area.sewn_over = over;
    }
}
