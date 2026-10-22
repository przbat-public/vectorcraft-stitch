//! Each sewing unit's stitches kept between plans ([`UnitCache`]), by exactly what they were
//! planned from ([`UnitKey`]), so that a plan after an edit plans again only the units the edit
//! changed.
//!
//! A unit's stitches depend on where the needle comes from (each object starts near where the one
//! before it ended), so that is part of the key, to the bit: an edit that moves where an object
//! ends plans again the object after it in its block, and the change runs on only as far as the
//! ends move. Nothing is rounded, so a kept unit is always the unit a fresh plan would make.
//!
//! The memory is bounded ([`MAX_CACHED_STITCHES`]): past it, the units used least recently are
//! dropped first.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use vectorcraft_geom::{BezPath, FillRule, PathEl, Point};
use vectorcraft_stitch::plan::{Params, Style, Underlay};

use super::unit::{UnitRequest, UnitStitches};

/// The most stitches the units kept for one document may hold, a key's own length counted as
/// stitches of the same size (a stitch is three 64-bit words: two coordinates and a command).
/// As many as the largest design may have (`digitize`'s 2 million): about 50 MB, enough to keep
/// every unit of any design the export takes, and a bound on what an open document keeps. An
/// estimate.
pub(crate) const MAX_CACHED_STITCHES: usize = 2_000_000;

/// The 64-bit words a stitch takes ([`MAX_CACHED_STITCHES`] counts a key's words in stitches).
const WORDS_PER_STITCH: usize = 3;

/// A unit request ([`UnitRequest`]) as the cache keys it: every value its stitches depend on, in
/// order, each as its exact bit pattern, with a length or a tag before every part whose size
/// varies, so that two requests have the same key exactly when they ask for the same stitches.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct UnitKey(Box<[u64]>);

impl UnitKey {
    pub(crate) fn of(req: &UnitRequest<'_>) -> Self {
        let UnitRequest { shape, rule, params, columns, from, clip } = *req;
        let mut k: Vec<u64> = Vec::new();
        path(&mut k, shape);
        k.push(rule_word(rule));
        params_words(&mut k, &params);
        match columns {
            None => k.push(0),
            Some(p) => {
                k.push(1);
                params_words(&mut k, &p);
            }
        }
        match from {
            None => k.push(0),
            Some(p) => {
                k.push(1);
                point(&mut k, p);
            }
        }
        match clip {
            None => k.push(0),
            Some((region, rule)) => {
                k.push(1);
                path(&mut k, region);
                k.push(rule_word(*rule));
            }
        }
        Self(k.into_boxed_slice())
    }

    /// Its length in 64-bit words.
    fn words(&self) -> usize {
        self.0.len()
    }
}

fn rule_word(rule: FillRule) -> u64 {
    match rule {
        FillRule::NonZero => 0,
        FillRule::EvenOdd => 1,
    }
}

fn point(k: &mut Vec<u64>, p: Point) {
    k.extend([p.x.to_bits(), p.y.to_bits()]);
}

/// A path: its element count, then each element's kind and points.
fn path(k: &mut Vec<u64>, bp: &BezPath) {
    let elements = bp.elements();
    k.push(elements.len() as u64);
    for el in elements {
        match *el {
            PathEl::MoveTo(p) => {
                k.push(0);
                point(k, p);
            }
            PathEl::LineTo(p) => {
                k.push(1);
                point(k, p);
            }
            PathEl::QuadTo(a, p) => {
                k.push(2);
                point(k, a);
                point(k, p);
            }
            PathEl::CurveTo(a, b, p) => {
                k.push(3);
                point(k, a);
                point(k, b);
                point(k, p);
            }
            PathEl::ClosePath => k.push(4),
        }
    }
}

/// Every one of a unit's parameters. Taken apart field by field, so that a parameter added to
/// [`Params`] fails to compile here until the key holds it too.
fn params_words(k: &mut Vec<u64>, p: &Params) {
    let Params {
        style,
        stitch_length,
        density,
        angle,
        pull,
        pull_percent,
        push,
        thread_weight,
        underlay,
        jump,
        trim,
        lock,
        min_stitch,
        width,
        max_satin,
        auto_spacing,
        narrow,
    } = *p;
    k.push(match style {
        Style::Running => 0,
        Style::Satin => 1,
        Style::Fill => 2,
    });
    let [pull0, pull1] = pull;
    let [percent0, percent1] = pull_percent;
    let [push0, push1] = push;
    for v in [
        stitch_length,
        density,
        angle,
        pull0,
        pull1,
        percent0,
        percent1,
        push0,
        push1,
        thread_weight,
        jump,
        trim,
        min_stitch,
        width,
        max_satin,
        narrow,
    ] {
        k.push(v.to_bits());
    }
    k.push(match underlay {
        Underlay::None => 0,
        Underlay::Auto => 1,
        Underlay::Center => 2,
        Underlay::Edge => 3,
        Underlay::EdgeZigzag => 4,
        Underlay::EdgeTatami => 5,
    });
    for flag in [lock, auto_spacing] {
        k.push(u64::from(flag));
    }
}

/// One kept unit: its stitches, and when it was last used ([`UnitCache::tick`]).
struct Kept {
    planned: Arc<UnitStitches>,
    used: u64,
}

/// The units of one document's plans, by their keys, at most [`MAX_CACHED_STITCHES`] of them,
/// those used least recently dropped first.
pub(crate) struct UnitCache {
    kept: HashMap<Arc<UnitKey>, Kept>,
    /// The keys by when they were last used, oldest first.
    recency: BTreeMap<u64, Arc<UnitKey>>,
    /// Counts the lookups and additions: each takes the next value as its time of use.
    tick: u64,
    /// What the kept units hold, counted as [`MAX_CACHED_STITCHES`] counts it.
    held: usize,
    /// The most they may hold.
    limit: usize,
}

impl Default for UnitCache {
    fn default() -> Self {
        Self::with_limit(MAX_CACHED_STITCHES)
    }
}

impl UnitCache {
    /// A cache that holds at most `limit` stitches.
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self { kept: HashMap::new(), recency: BTreeMap::new(), tick: 0, held: 0, limit }
    }

    /// The stitches kept for `key`, now the most recently used.
    pub(crate) fn get(&mut self, key: &UnitKey) -> Option<Arc<UnitStitches>> {
        let kept = self.kept.get_mut(key)?;
        self.tick += 1;
        if let Some(shared) = self.recency.remove(&kept.used) {
            self.recency.insert(self.tick, shared);
        }
        kept.used = self.tick;
        Some(kept.planned.clone())
    }

    /// Keep `planned` for `key`, dropping the units used least recently while the cache holds more
    /// than its limit. A unit larger than the whole limit is not kept.
    pub(crate) fn insert(&mut self, key: UnitKey, planned: Arc<UnitStitches>) {
        let size = cost(&key, &planned);
        if size > self.limit {
            return;
        }
        self.tick += 1;
        let key = Arc::new(key);
        if let Some(old) = self.kept.remove(&key) {
            self.recency.remove(&old.used);
            self.held = self.held.saturating_sub(cost(&key, &old.planned));
        }
        self.recency.insert(self.tick, key.clone());
        self.kept.insert(key, Kept { planned, used: self.tick });
        self.held = self.held.saturating_add(size);
        while self.held > self.limit {
            let Some((_, oldest)) = self.recency.pop_first() else { break };
            if let Some(gone) = self.kept.remove(&oldest) {
                self.held = self.held.saturating_sub(cost(&oldest, &gone.planned));
            }
        }
    }

    /// How many units are kept.
    #[cfg(test)]
    pub(crate) fn units(&self) -> usize {
        self.kept.len()
    }

    /// What the kept units hold, counted as [`MAX_CACHED_STITCHES`] counts it.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.held
    }
}

/// What keeping `planned` under `key` costs, in stitches: its stitches, and its key's words as
/// stitches.
fn cost(key: &UnitKey, planned: &UnitStitches) -> usize {
    planned.stitches.len().saturating_add(key.words().div_ceil(WORDS_PER_STITCH))
}
