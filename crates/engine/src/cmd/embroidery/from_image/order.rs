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
    let mut ready = BTreeSet::new();
    let mut ready_by_thread: BTreeMap<Option<usize>, BTreeSet<usize>> = BTreeMap::new();
    for a in (0..n).filter(|&a| sewn(a) && waiting[a] == 0) {
        ready.insert(key(a));
        ready_by_thread.entry(thread(a)).or_default().insert(a);
    }
    let mut current: Option<usize> = None;
    let mut last: Option<(u32, u32)> = None;
    // The keys agree with every rule kept, so an area is ready until every one is sewn.
    while let Some(&(.., first)) = ready.first() {
        vectorcraft_doc::cancel::check()?;
        let distance = |a: usize| {
            let (x, y) = regions.areas.get(a).map_or((0, 0), |s| s.deepest);
            last.map_or(0u64, |(lx, ly)| {
                let (dx, dy) = (i64::from(x) - i64::from(lx), i64::from(y) - i64::from(ly));
                (dx * dx + dy * dy).unsigned_abs()
            })
        };
        let same = current.and_then(|_| ready_by_thread.get(&current)).and_then(|r| r.iter().copied().min_by_key(|&a| (distance(a), a)));
        let (pick, how) = match same {
            Some(a) => (a, Taken::SameThread),
            None => (first, Taken::NewThread),
        };
        ready.remove(&key(pick));
        if let Some(r) = ready_by_thread.get_mut(&thread(pick)) {
            r.remove(&pick);
        }
        current = thread(pick);
        last = regions.areas.get(pick).map(|s| s.deepest);
        taken[pick] = Some(how);
        order.push(pick);
        for &a in waited_on.get(pick).map_or(&[][..], Vec::as_slice) {
            if let Some(w) = waiting.get_mut(a) {
                *w = w.saturating_sub(1);
                if *w == 0 {
                    ready.insert(key(a));
                    ready_by_thread.entry(thread(a)).or_default().insert(a);
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
