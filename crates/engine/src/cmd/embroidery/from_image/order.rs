//! The order the areas are sewn in, which is the order the objects are stacked in (the stitch plan
//! sews in stacking order).
//!
//! Three rules hold, each because what is sewn later lies on top and pulls on what lies under it:
//! - a line is sewn after the fills it touches (details last);
//! - an area after the area it lies inside;
//! - an area after a neighbour more than [`MUCH_LARGER`] times its size, so a large fill sewn later
//!   doesn't pull a small detail out of shape.
//!
//! A rule is kept only where it agrees with one order of every area (lines after fills, then the
//! more deeply nested, then the smaller): so the rules never ask for a circle. Within them, the
//! areas are taken greedily: the current thread is kept while one of its areas may be sewn (the
//! nearest to the last area sewn), else the thread of the area that is most a background (a fill,
//! the least nested, the largest). Each area keeps why it is where it is, and the areas it touches
//! sewn before it, which the overlap grows under it.

use std::collections::{BTreeMap, BTreeSet};

use super::colours::Colours;
use super::regions::Regions;
use super::rules::{Area, AreaKind};

/// How many times larger a neighbour must be for an area to be sewn after it: twice. An estimate.
pub(super) const MUCH_LARGER: usize = 2;

/// The side of the cells the areas ready to sew are kept in, by where their key points lie
/// (working pixels): the nearest one to the last area sewn is looked for ring by ring of cells
/// round it, not among every area of the thread. An estimate: a few cells cover a small design,
/// some fifty a side the largest working copy.
const CELL_PX: u32 = 32;

/// The areas ready to sew of each thread, by the cell their key point lies in ([`CELL_PX`]).
#[derive(Default)]
struct ReadyCells {
    cells: BTreeMap<(Option<usize>, u32, u32), BTreeSet<usize>>,
    /// The farthest cell any area lies in, along either axis.
    reach: u32,
}

impl ReadyCells {
    fn cell((x, y): (u32, u32)) -> (u32, u32) {
        (x / CELL_PX, y / CELL_PX)
    }

    fn insert(&mut self, thread: Option<usize>, at: (u32, u32), a: usize) {
        let (cx, cy) = Self::cell(at);
        self.reach = self.reach.max(cx).max(cy);
        self.cells.entry((thread, cx, cy)).or_default().insert(a);
    }

    fn remove(&mut self, thread: Option<usize>, at: (u32, u32), a: usize) {
        let (cx, cy) = Self::cell(at);
        if let Some(set) = self.cells.get_mut(&(thread, cx, cy)) {
            set.remove(&a);
            if set.is_empty() {
                self.cells.remove(&(thread, cx, cy));
            }
        }
    }

    /// Of `thread`'s areas, the one whose key point (`at` of each) lies nearest `from`, the first by
    /// number of those as near: ring by ring of cells round `from`'s, until no ring farther can
    /// hold one nearer.
    fn nearest(&self, thread: Option<usize>, from: (u32, u32), at: impl Fn(usize) -> (u32, u32)) -> Option<usize> {
        let (cx, cy) = Self::cell(from);
        let distance = |a: usize| {
            let (x, y) = at(a);
            let (dx, dy) = (i64::from(x) - i64::from(from.0), i64::from(y) - i64::from(from.1));
            (dx * dx + dy * dy).unsigned_abs()
        };
        let mut best: Option<(u64, usize)> = None;
        for ring in 0..=self.reach.saturating_add(1) {
            // Every cell of this ring and beyond lies at least `ring - 1` cells from `from`'s cell.
            let near = u64::from(ring.saturating_sub(1)) * u64::from(CELL_PX);
            if best.is_some_and(|(d, _)| near * near > d) {
                break;
            }
            let (x0, x1) = (cx.saturating_sub(ring), cx.saturating_add(ring));
            let (y0, y1) = (cy.saturating_sub(ring), cy.saturating_add(ring));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    // The ring's own cells: those `ring` cells away along either axis.
                    if x.abs_diff(cx).max(y.abs_diff(cy)) != ring {
                        continue;
                    }
                    for &a in self.cells.get(&(thread, x, y)).into_iter().flatten() {
                        let d = distance(a);
                        if best.is_none_or(|(bd, ba)| (d, a) < (bd, ba)) {
                            best = Some((d, a));
                        }
                    }
                }
            }
        }
        best.map(|(_, a)| a)
    }
}

/// Why an area must come after another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum After {
    /// It is a line, and the other a fill it touches.
    Detail,
    /// It lies inside the other.
    Inside,
    /// The other, which it touches, is more than [`MUCH_LARGER`] times its size.
    Larger,
}

impl After {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Detail => "detail",
            Self::Inside => "inside",
            Self::Larger => "larger",
        }
    }
}

/// How an area came to be taken when it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Taken {
    /// Not sewn: first, under everything.
    NotSewn,
    /// Its thread was being sewn: the nearest of its areas that could be.
    SameThread,
    /// A new thread: its area is the most a background of those that could be sewn.
    NewThread,
}

impl Taken {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::NotSewn => "notSewn",
            Self::SameThread => "sameThread",
            Self::NewThread => "newThread",
        }
    }
}

/// Where an area is sewn, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Placed {
    /// Its place in the sewing order, from 0; `None` for the background's areas, which are never
    /// made.
    pub(super) index: Option<usize>,
    pub(super) taken: Taken,
    /// The areas it must come after, and why.
    pub(super) after: Vec<(usize, After)>,
    /// The sewn areas it touches that are sewn before it.
    pub(super) under: Vec<usize>,
}

/// The sewing order of `areas`.
pub(super) struct Order {
    /// The areas made, in the order sewn (those not sewn first): their indices. The background's
    /// are not among them.
    pub(super) sewn: Vec<usize>,
    /// Each area's place and reasons, by its index.
    pub(super) placed: Vec<Placed>,
}

/// The sewing order of `areas` (their region map `regions`, their colours `colours`): the rules,
/// then the greedy choice (this module's).
pub(super) fn ordered(areas: &[Area], regions: &Regions, colours: &Colours) -> crate::Result<Order> {
    let n = areas.len();
    let sewn = |a: usize| areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(super::colours::Colour::sewn);
    let background = |a: usize| areas.get(a).and_then(|x| colours.rows.get(x.row)).is_some_and(|r| r.background);
    let thread = |a: usize| areas.get(a).and_then(|x| colours.rows.get(x.row)).and_then(|r| r.thread.as_ref()).map(|m| m.thread.index);
    let pixels = |a: usize| areas.get(a).map_or(0, |x| x.pixels);
    let depth = |mut a: usize| {
        let mut d = 0usize;
        while let Some(o) = areas.get(a).and_then(|x| x.inside) {
            d += 1;
            if d > n {
                break;
            }
            a = o;
        }
        d
    };
    let line = |a: usize| areas.get(a).is_some_and(|x| x.kind == AreaKind::Line);
    // The one order every rule must agree with: fills first, the least nested first, the largest
    // first, then by index.
    let key = |a: usize| (u8::from(line(a)), depth(a), std::cmp::Reverse(pixels(a)), a);
    let mut after: Vec<Vec<(usize, After)>> = vec![Vec::new(); n];
    for (a, rules) in after.iter_mut().enumerate() {
        if !sewn(a) {
            continue;
        }
        let mut add = |b: usize, why: After| {
            if sewn(b) && key(b) < key(a) && !rules.iter().any(|&(x, _)| x == b) {
                rules.push((b, why));
            }
        };
        if let Some(b) = areas.get(a).and_then(|x| x.inside) {
            add(b, After::Inside);
        }
        for &(b, _) in regions.neighbours(a as u32) {
            let b = b as usize;
            if line(a) && !line(b) {
                add(b, After::Detail);
            } else if pixels(b) > MUCH_LARGER * pixels(a) {
                add(b, After::Larger);
            }
        }
    }
    // Those not sewn first, in the trace's order; then the sewn ones, greedily. The background's
    // aren't made at all.
    let mut order: Vec<usize> = (0..n).filter(|&a| !sewn(a) && !background(a)).collect();
    let mut taken: Vec<Option<Taken>> = vec![None; n];
    for &a in &order {
        taken[a] = Some(Taken::NotSewn);
    }
    // How many of its rules each area still waits on, and the areas waiting on each.
    let mut waiting: Vec<usize> = after.iter().map(Vec::len).collect();
    let mut waited_on: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (a, rules) in after.iter().enumerate() {
        for &(b, _) in rules {
            if let Some(w) = waited_on.get_mut(b) {
                w.push(a);
            }
        }
    }
    // The areas ready to be sewn, by key and by thread.
    let at = |a: usize| regions.areas.get(a).map_or((0, 0), |s| s.deepest);
    let mut ready = BTreeSet::new();
    let mut cells = ReadyCells::default();
    for a in (0..n).filter(|&a| sewn(a) && waiting[a] == 0) {
        ready.insert(key(a));
        cells.insert(thread(a), at(a), a);
    }
    let mut current: Option<usize> = None;
    let mut last: Option<(u32, u32)> = None;
    // The keys agree with every rule kept, so an area is ready until every one is sewn.
    while let Some(&(.., first)) = ready.first() {
        vectorcraft_doc::cancel::check()?;
        // The current thread's nearest area to the last one sewn (none before the first).
        let same = current.and(last).and_then(|from| cells.nearest(current, from, at));
        let (pick, how) = match same {
            Some(a) => (a, Taken::SameThread),
            None => (first, Taken::NewThread),
        };
        ready.remove(&key(pick));
        cells.remove(thread(pick), at(pick), pick);
        current = thread(pick);
        last = Some(at(pick));
        taken[pick] = Some(how);
        order.push(pick);
        for &a in waited_on.get(pick).map_or(&[][..], Vec::as_slice) {
            if let Some(w) = waiting.get_mut(a) {
                *w = w.saturating_sub(1);
                if *w == 0 {
                    ready.insert(key(a));
                    cells.insert(thread(a), at(a), a);
                }
            }
        }
    }
    let mut index: Vec<Option<usize>> = vec![None; n];
    for (i, &a) in order.iter().enumerate() {
        if let Some(slot) = index.get_mut(a) {
            *slot = Some(i);
        }
    }
    let placed = (0..n)
        .map(|a| {
            let mut under: Vec<usize> =
                regions.neighbours(a as u32).iter().map(|&(b, _)| b as usize).filter(|&b| sewn(b) && sewn(a) && index[b] < index[a]).collect();
            under.sort_by_key(|&b| index[b]);
            Placed { index: index[a], taken: taken[a].unwrap_or(Taken::NewThread), after: after[a].clone(), under }
        })
        .collect();
    Ok(Order { sewn: order, placed })
}

#[cfg(test)]
mod tests {
    use super::ReadyCells;

    /// The cells find the area every scan of the ready areas finds: of the thread's, the nearest
    /// to the point asked, the first by number of those as near; random points (many on the same
    /// spot, ties), two threads, areas taken out as they are sewn.
    #[test]
    fn the_cells_find_the_nearest_as_a_scan_does() {
        let mut seed = 0x0dde_c311u64;
        let mut next = |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % u64::from(n)) as u32
        };
        let points: Vec<(u32, u32)> = (0..400).map(|_| (next(300) / 7 * 7, next(200) / 7 * 7)).collect();
        let threads: Vec<Option<usize>> = (0..400).map(|_| Some((next(2)) as usize)).collect();
        let mut cells = ReadyCells::default();
        let mut ready: Vec<usize> = (0..400).collect();
        for &a in &ready {
            cells.insert(threads[a], points[a], a);
        }
        let at = |a: usize| points[a];
        while !ready.is_empty() {
            let (from, thread) = ((next(320), next(220)), Some(next(2) as usize));
            let scan = ready.iter().copied().filter(|&a| threads[a] == thread).min_by_key(|&a| {
                let (dx, dy) = (i64::from(points[a].0) - i64::from(from.0), i64::from(points[a].1) - i64::from(from.1));
                ((dx * dx + dy * dy).unsigned_abs(), a)
            });
            assert_eq!(cells.nearest(thread, from, at), scan, "from {from:?}");
            let taken = scan.unwrap_or_else(|| ready[0]);
            cells.remove(threads[taken], points[taken], taken);
            ready.retain(|&a| a != taken);
        }
    }
}
