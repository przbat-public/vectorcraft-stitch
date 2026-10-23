//! Each sewing unit's stitches kept between plans ([`UnitCache`]), by exactly what they were
//! planned from ([`UnitKey`]), so that a plan after an edit plans again only the units the edit
//! changed; and the areas the path operations made for units (a fill's `expand`, the clip groups
//! around it, a stroke's outline: [`Shaped`]), by what they were made from, so that a plan doesn't
//! run those operations again on objects that haven't changed.
//!
//! A unit's stitches depend on where the needle comes from (each object starts near where the one
//! before it ended), so that is part of the key, to the bit: an edit that moves where an object
//! ends plans again the object after it in its block, and the change runs on only as far as the
//! ends move. Nothing is rounded, so a kept unit is always the unit a fresh plan would make.
//!
//! The memory is bounded ([`MAX_CACHED_STITCHES`], [`MAX_KEPT_SHAPE_WORDS`]): past it, what was
//! used least recently is dropped first, but never something the plan under way used for something
//! else it uses ([`UnitCache::begin`]): a design whose units don't all fit keeps those it planned
//! first, rather than each new unit dropping the one the next plan asks for next (which, plan after
//! plan, would keep none).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use vectorcraft_geom::{BezPath, FillRule, PathEl, Point};
use vectorcraft_stitch::plan::{Params, Style, Underlay};

use super::unit::{UnitRequest, UnitStitches};

/// The most stitches the units kept for one document may hold, a key's own length counted as
/// stitches of the same size (a stitch is three 64-bit words: two coordinates and a command):
/// about 50 MB. As many as the largest design may have (`digitize`'s 2 million), so a design near
/// that limit, whose keys count too, keeps most of its units, not all. A bound on what the units
/// keep: the service's last plan holds its own copy of the stitches besides. An estimate.
pub(crate) const MAX_CACHED_STITCHES: usize = 2_000_000;

/// The 64-bit words a stitch takes ([`MAX_CACHED_STITCHES`] counts a key's words in stitches).
const WORDS_PER_STITCH: usize = 3;

/// The most 64-bit words the kept areas ([`Shaped`]) and their keys may hold for one document:
/// about 32 MB, the size of the areas of a design of a few thousand objects. An estimate.
pub(crate) const MAX_KEPT_SHAPE_WORDS: usize = 4_000_000;
/// The words a kept area's path takes per element, at most (a curve's tag and three points).
const WORDS_PER_ELEMENT: usize = 7;
/// The most shared key parts ([`Shared`]) one document keeps: past it, they are forgotten (keys
/// that hold one keep it) and shared again from the next. Many more than a design's clip groups.
const MAX_SHARED: usize = 1024;

/// A part many keys share (a clip group's clipping path, in the key of every object inside it):
/// its words held once and their hash worked out once, so that building, hashing and comparing a
/// key doesn't go over them again. Two are equal when their words are (the same one at once).
#[derive(Debug)]
pub(crate) struct Shared {
    words: Box<[u64]>,
    hash: u64,
}

impl Shared {
    pub(crate) fn of(words: Vec<u64>) -> Arc<Self> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        words.hash(&mut h);
        Arc::new(Self { words: words.into_boxed_slice(), hash: h.finish() })
    }
}

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other) || (self.hash == other.hash && self.words == other.words)
    }
}

impl Eq for Shared {}

impl Hash for Shared {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

/// What the cache keeps something by: every value it depends on, in order, each as its exact bit
/// pattern, with a length or a tag before every part whose size varies, so that two keys are equal
/// exactly when they ask for the same thing; and a part shared with other keys ([`Shared`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct UnitKey {
    own: Box<[u64]>,
    shared: Option<Arc<Shared>>,
}

impl UnitKey {
    /// A key of `own` words and the `shared` part.
    pub(crate) fn new(own: Vec<u64>, shared: Option<Arc<Shared>>) -> Self {
        Self { own: own.into_boxed_slice(), shared }
    }

    /// A unit request's key ([`UnitRequest`]): its stitches are the same exactly when its key is.
    /// A clipping path that cuts its stitches is the shared part (`clip`, its words as
    /// [`clip_words`] gives them; worked out here when not given).
    pub(crate) fn of(req: &UnitRequest<'_>, clip: Option<&Arc<Shared>>) -> Self {
        let UnitRequest { shape, rule, params, columns, from, clip: region } = *req;
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
        let shared = region.map(|(region, rule)| clip.cloned().unwrap_or_else(|| Shared::of(clip_words(region, *rule))));
        k.push(u64::from(shared.is_some()));
        Self::new(k, shared)
    }

    /// Its own length in 64-bit words (a shared part is held once, by every key that shares it).
    fn words(&self) -> usize {
        self.own.len()
    }
}

/// A clipping path's words in a key: the path, then its fill rule.
pub(crate) fn clip_words(region: &BezPath, rule: FillRule) -> Vec<u64> {
    let mut k = Vec::new();
    path(&mut k, region);
    k.push(rule_word(rule));
    k
}

pub(crate) fn rule_word(rule: FillRule) -> u64 {
    match rule {
        FillRule::NonZero => 0,
        FillRule::EvenOdd => 1,
    }
}

pub(crate) fn point(k: &mut Vec<u64>, p: Point) {
    k.extend([p.x.to_bits(), p.y.to_bits()]);
}

/// A path: its element count, then each element's kind and points.
pub(crate) fn path(k: &mut Vec<u64>, bp: &BezPath) {
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

/// One kept value: what it costs ([`Lru`]'s measure), when it was last used ([`Lru::tick`]), and
/// by which plan ([`Lru::walk`]).
struct Kept<V> {
    value: V,
    cost: usize,
    used: u64,
    walk: u64,
}

/// Values by their keys, at most `limit` of them by their costs, those used least recently dropped
/// first, but never one the plan under way used ([`Lru::begin`]) for another it uses.
struct Lru<V> {
    kept: HashMap<Arc<UnitKey>, Kept<V>>,
    /// The keys by when they were last used, oldest first.
    recency: BTreeMap<u64, Arc<UnitKey>>,
    /// Counts the lookups and additions: each takes the next value as its time of use.
    tick: u64,
    /// Counts the plans: what the one under way uses is not dropped for each other.
    walk: u64,
    /// What the kept values cost in all.
    held: usize,
    /// The most they may cost.
    limit: usize,
}

impl<V: Clone> Lru<V> {
    fn new(limit: usize) -> Self {
        Self { kept: HashMap::new(), recency: BTreeMap::new(), tick: 0, walk: 0, held: 0, limit }
    }

    fn begin(&mut self) {
        self.walk += 1;
    }

    /// The value kept for `key`, now the most recently used.
    fn get(&mut self, key: &UnitKey) -> Option<V> {
        let kept = self.kept.get_mut(key)?;
        self.tick += 1;
        if let Some(shared) = self.recency.remove(&kept.used) {
            self.recency.insert(self.tick, shared);
        }
        kept.used = self.tick;
        kept.walk = self.walk;
        Some(kept.value.clone())
    }

    /// Keep `value`, costing `cost`, for `key`, dropping what was used least recently while more
    /// than the limit is held; when that is something the plan under way used, `value` is not kept
    /// instead. A value costing more than the whole limit is not kept.
    fn insert(&mut self, key: UnitKey, value: V, cost: usize) {
        if cost > self.limit {
            return;
        }
        self.tick += 1;
        let key = Arc::new(key);
        if let Some(old) = self.kept.remove(&key) {
            self.recency.remove(&old.used);
            self.held = self.held.saturating_sub(old.cost);
        }
        self.recency.insert(self.tick, key.clone());
        self.kept.insert(key.clone(), Kept { value, cost, used: self.tick, walk: self.walk });
        self.held = self.held.saturating_add(cost);
        while self.held > self.limit {
            let Some((&used, oldest)) = self.recency.first_key_value() else { break };
            // What the plan under way used stays: the value just added goes instead.
            let (gone, used) = match self.kept.get(oldest) {
                Some(k) if k.walk == self.walk => (key.clone(), self.tick),
                _ => (oldest.clone(), used),
            };
            self.recency.remove(&used);
            if let Some(dropped) = self.kept.remove(&gone) {
                self.held = self.held.saturating_sub(dropped.cost);
            }
            if Arc::ptr_eq(&gone, &key) {
                break;
            }
        }
    }
}

/// An area the path operations made for a unit to fill (millimetres): its path and fill rule,
/// and whether a negative `expand` left nothing of it (which the plan says).
#[derive(Debug)]
pub(crate) struct Shaped {
    pub(crate) area: BezPath,
    pub(crate) rule: FillRule,
    pub(crate) expanded_away: bool,
}

/// What one document's plans keep: each unit's stitches (at most [`MAX_CACHED_STITCHES`]), the
/// areas the path operations made (at most [`MAX_KEPT_SHAPE_WORDS`]), and the key parts shared
/// between keys ([`Shared`]).
pub(crate) struct UnitCache {
    units: Lru<Arc<UnitStitches>>,
    shapes: Lru<Arc<Shaped>>,
    shared: HashSet<Arc<Shared>>,
}

impl Default for UnitCache {
    fn default() -> Self {
        Self::with_limit(MAX_CACHED_STITCHES)
    }
}

impl UnitCache {
    /// A cache whose units hold at most `limit` stitches.
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self { units: Lru::new(limit), shapes: Lru::new(MAX_KEPT_SHAPE_WORDS), shared: HashSet::new() }
    }

    /// A plan begins: the units and areas it uses and adds are its own until the next begins.
    pub(crate) fn begin(&mut self) {
        self.units.begin();
        self.shapes.begin();
    }

    /// The stitches kept for `key`, now the most recently used.
    pub(crate) fn get(&mut self, key: &UnitKey) -> Option<Arc<UnitStitches>> {
        self.units.get(key)
    }

    /// Keep `planned` for `key` (see [`Lru::insert`]): its stitches and its key's words count, as
    /// stitches.
    pub(crate) fn insert(&mut self, key: UnitKey, planned: Arc<UnitStitches>) {
        let cost = planned.stitches.len().saturating_add(key.words().div_ceil(WORDS_PER_STITCH));
        self.units.insert(key, planned, cost);
    }

    /// The area kept for `key`, now the most recently used.
    pub(crate) fn shape(&mut self, key: &UnitKey) -> Option<Arc<Shaped>> {
        self.shapes.get(key)
    }

    /// Keep `shaped` for `key`: its path's words and its key's count.
    pub(crate) fn keep_shape(&mut self, key: UnitKey, shaped: Arc<Shaped>) {
        let cost = shaped.area.elements().len().saturating_mul(WORDS_PER_ELEMENT).saturating_add(key.words());
        self.shapes.insert(key, shaped, cost);
    }

    /// `words` as a part keys share: the one kept already when one is equal, so that keys holding
    /// it compare at once.
    pub(crate) fn shared(&mut self, words: Vec<u64>) -> Arc<Shared> {
        let part = Shared::of(words);
        if let Some(kept) = self.shared.get(&part) {
            return kept.clone();
        }
        if self.shared.len() >= MAX_SHARED {
            self.shared.clear();
        }
        self.shared.insert(part.clone());
        part
    }

    /// How many units are kept.
    #[cfg(test)]
    pub(crate) fn units(&self) -> usize {
        self.units.kept.len()
    }

    /// What the kept units hold, counted as [`MAX_CACHED_STITCHES`] counts it.
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.units.held
    }

    /// How many areas are kept.
    #[cfg(test)]
    pub(crate) fn shapes(&self) -> usize {
        self.shapes.kept.len()
    }
}
