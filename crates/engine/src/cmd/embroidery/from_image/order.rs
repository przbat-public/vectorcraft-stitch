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

use std::collections::BTreeSet;

use super::colours::Colours;
use super::regions::Regions;
use super::rules::{Area, AreaKind};

/// How many times larger a neighbour must be for an area to be sewn after it: twice. An estimate.
pub(super) const MUCH_LARGER: usize = 2;

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
pub(super) fn ordered(areas: &[Area], regions: &Regions, colours: &Colours) -> Order {
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
        for (b, _) in regions.neighbours(a as u32) {
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
    let mut done: BTreeSet<usize> = (0..n).filter(|&a| !sewn(a)).collect();
    let mut current: Option<usize> = None;
    let mut last: Option<(u32, u32)> = None;
    while done.len() < n {
        let ready: Vec<usize> = (0..n).filter(|a| !done.contains(a) && after[*a].iter().all(|(b, _)| done.contains(b))).collect();
        // The keys agree with every rule kept, so one area is always ready.
        let Some(&first) = ready.iter().min_by_key(|&&a| key(a)) else { break };
        let distance = |a: usize| {
            let (x, y) = regions.areas.get(a).map_or((0, 0), |s| s.deepest);
            last.map_or(0u64, |(lx, ly)| {
                let (dx, dy) = (i64::from(x) - i64::from(lx), i64::from(y) - i64::from(ly));
                (dx * dx + dy * dy).unsigned_abs()
            })
        };
        let same: Vec<usize> = ready.iter().copied().filter(|&a| current.is_some() && thread(a) == current).collect();
        let (pick, how) = match same.iter().copied().min_by_key(|&a| (distance(a), a)) {
            Some(a) => (a, Taken::SameThread),
            None => (first, Taken::NewThread),
        };
        current = thread(pick);
        last = regions.areas.get(pick).map(|s| s.deepest);
        taken[pick] = Some(how);
        done.insert(pick);
        order.push(pick);
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
                regions.neighbours(a as u32).into_iter().map(|(b, _)| b as usize).filter(|&b| sewn(b) && sewn(a) && index[b] < index[a]).collect();
            under.sort_by_key(|&b| index[b]);
            Placed { index: index[a], taken: taken[a].unwrap_or(Taken::NewThread), after: after[a].clone(), under }
        })
        .collect();
    Order { sewn: order, placed }
}
