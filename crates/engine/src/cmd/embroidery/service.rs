//! The planning service: one per open document ([`Services`]), it plans the document's art into
//! its stitch plan and keeps each sewing unit's stitches between plans ([`UnitCache`]), so that a
//! plan after an edit plans again only the units the edit changed.
//!
//! It also remembers its last few plans ([`MAX_MEMOS`]; the plans of all open documents, and their
//! scenes, within [`MAX_MEMO_BYTES`] together, [`Memos`]), each of which snapshot of the document
//! and with which options: a document snapshot (`Arc<Document>`, shared
//! with the undo history) never changes, so the same snapshot with the same options has the same
//! plan, and the commands that read a design one after another (`embroidery.threads`, and the
//! statistics, sewing order and checks) and the app's planner plan it once. Several are kept so that
//! the stitch preview's plan, a command's with other options and an undo back to the last snapshot
//! don't push one another out. The document planned may be cut from the snapshot (the selected
//! objects alone, or without an opacity mask's editing layer): a plan is remembered by the snapshot
//! and the selection it was cut to ([`Of`]), not by the copy, which is made anew each time.
//!
//! Each plan made gets a generation, a number no other plan of the process has
//! ([`Planned::generation`]): what is drawn from it (its stitch scene, built once on first request
//! and remembered with it) is keyed by that number, never by the document's revision, which also
//! moves when the selection does.
//!
//! A plan can be cancelled ([`vectorcraft_doc::cancel`]): it stops before the next unit (or within
//! a fill's search for its narrow parts) with [`PlanError::Cancelled`] and leaves no plan, while
//! the units it finished stay kept for the next. Each unit is reported as a step of its progress.
//! The service is `Send` and `Sync`, so the app can plan on a worker thread while commands plan
//! on theirs; commands and the web build plan synchronously.
//!
//! Two callers asking for the same plan at once (the app's worker and a command, say) plan it once:
//! the second waits for the first's plan instead of walking the document too (one flight per plan).
//! The wait ends when the first lands (its plan remembered, or not when it was cancelled: the second
//! then plans), when the second is cancelled itself, or after [`MAX_FLIGHT_WAIT_MS`], past which it
//! plans for itself; a flight ends even when its plan panics. The web build, which has one thread,
//! has no flights.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use vectorcraft_doc::{Document, Node, NodeId, NodeKind};

use super::Options;
use super::cache::UnitCache;
use super::digitize::digitize;
use super::plan::DesignPlan;
use super::scene::{StitchScene, scene};
use super::unit::PlanError;

/// The most plans a service remembers: the stitch preview's, a read command's with other options
/// or of the selection alone, an agent's, and the snapshot before the last edit (an undo). An
/// estimate.
pub const MAX_MEMOS: usize = 4;

/// The most bytes the plans remembered for all open documents ([`Memos`]), and their scenes, may
/// hold together; the newest is kept whatever its size. At the 2-million-stitch limit a plan holds
/// about 50 to 90 MB (25 to 45 bytes a record, measured on the tests' designs) and its scene about
/// 26 MB ([`SCENE_BYTES_PER_RECORD`]), so two or three of those. An estimate. On the web, whose
/// memory is far smaller, about one.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_MEMO_BYTES: usize = 256 << 20;
#[cfg(target_arch = "wasm32")]
pub const MAX_MEMO_BYTES: usize = 96 << 20;

/// About how many bytes a plan's stitch scene holds for each record, counted for a remembered plan
/// before its scene is built: 8 for the point, 4 for its number, and its share of the lines. 12.9
/// to 13.1 measured on the tests' designs (2026-10-23), rounded up.
pub const SCENE_BYTES_PER_RECORD: usize = 14;

/// How many snapshots [`fonts_of`] remembers its answer for: the documents open and the snapshots
/// before their last edits. An estimate.
const FONTS_OF_KEPT: usize = 32;

/// The fonts' generation a plan of `snapshot` depends on: the generation of the fonts installed
/// ([`vectorcraft_text::FontDb::generation`]) when the snapshot may outline type for its stitches,
/// else 0, so that a font loaded for something else (a text shaped in another document, a font
/// menu's previews) never makes its plan out of date. Text itself is left out of the stitches; a
/// text clipping path is outlined with the fonts installed, so a snapshot that has type and a clip
/// group, in its layers or its symbols' art, may (an over-estimate on purpose: the type may be
/// elsewhere). Worked out once a snapshot, which never changes.
pub(crate) fn fonts_of(snapshot: &Arc<Document>) -> u64 {
    static SEEN: Mutex<Vec<(Weak<Document>, bool)>> = Mutex::new(Vec::new());
    let ptr = Arc::as_ptr(snapshot);
    let known = SEEN.lock().ok().and_then(|seen| seen.iter().find(|(w, _)| std::ptr::eq(w.as_ptr(), ptr)).map(|(_, f)| *f));
    let uses = known.unwrap_or_else(|| {
        let uses = may_outline_type(snapshot);
        // A snapshot held only weakly can't be mistaken for another made since: its memory isn't
        // reused while the weak reference is held.
        if let Ok(mut seen) = SEEN.lock() {
            seen.retain(|(w, _)| w.strong_count() > 0);
            seen.push((Arc::downgrade(snapshot), uses));
            if seen.len() > FONTS_OF_KEPT {
                seen.remove(0);
            }
        }
        uses
    });
    if uses { vectorcraft_text::FontDb::global().generation() } else { 0 }
}

/// Whether `d` has type and a clip group, in its layers or its symbols' art ([`fonts_of`]).
fn may_outline_type(d: &Document) -> bool {
    let (mut text, mut clip) = (false, false);
    let mut look = |n: &Node| match &n.kind {
        NodeKind::Text(_) => text = true,
        NodeKind::Layer { clip: true, .. } | NodeKind::Group { clip: true, .. } => clip = true,
        _ => {}
    };
    d.walk(&mut look);
    for s in &d.symbols {
        s.art.walk(&mut look);
    }
    text && clip
}

/// The generation the next plan made gets: one counter for the process, so no two plans share one.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// How long a caller waits for another that plans the same snapshot with the same options before
/// planning it itself, milliseconds: far longer than a plan of a real design takes (under 20 ms), so
/// that it ends only a wait for a caller that got stuck. An estimate.
#[cfg(not(target_arch = "wasm32"))]
pub const MAX_FLIGHT_WAIT_MS: u64 = 10_000;

/// How often a waiting caller looks whether it was cancelled, milliseconds. An estimate.
#[cfg(not(target_arch = "wasm32"))]
const FLIGHT_CHECK_MS: u64 = 10;

/// A plan under way: what it is of, and its number among the service's flights.
#[cfg(not(target_arch = "wasm32"))]
struct Flight {
    id: u64,
    snapshot: Weak<Document>,
    selection: Vec<NodeId>,
    opts: Options,
    fonts: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Flight {
    fn of(&self, of: Of<'_>, opts: &Options, fonts: u64) -> bool {
        std::ptr::eq(self.snapshot.as_ptr(), Arc::as_ptr(of.snapshot)) && self.selection == of.selection && self.opts == *opts && self.fonts == fonts
    }
}

/// The plans under way of one service, and what wakes those waiting for one.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Flights {
    under_way: Mutex<(u64, Vec<Flight>)>,
    landed: std::sync::Condvar,
}

/// Ends its flight when dropped, also when the plan unwinds, and wakes the callers waiting for it.
/// `None`: planning without a flight (the flights' lock poisoned, or a wait given up).
#[cfg(not(target_arch = "wasm32"))]
struct Landing<'a> {
    flights: &'a Flights,
    id: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Landing<'_> {
    fn drop(&mut self) {
        let Some(id) = self.id else { return };
        // A poisoned lock still holds the list: the flight is taken off it all the same.
        let mut under_way = self.flights.under_way.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        under_way.1.retain(|f| f.id != id);
        drop(under_way);
        self.flights.landed.notify_all();
    }
}

/// What a caller who wants a plan does: plan it (its flight landing when the plan is done), or look
/// again for the plan another landed with.
#[cfg(not(target_arch = "wasm32"))]
enum Turn<'a> {
    Plan(Landing<'a>),
    LookAgain,
}

/// What a plan is of: a document snapshot, cut to the selected objects when `selection` lists
/// them (empty: all of it). The document planned is made from these alone (and without an opacity
/// mask's editing layer, which no export writes), so the same snapshot cut the same way plans the
/// same.
#[derive(Clone, Copy)]
pub(crate) struct Of<'a> {
    pub(crate) snapshot: &'a Arc<Document>,
    pub(crate) selection: &'a [NodeId],
}

impl<'a> Of<'a> {
    /// The whole of `snapshot`.
    pub(crate) fn whole(snapshot: &'a Arc<Document>) -> Self {
        Self { snapshot, selection: &[] }
    }
}

/// The plans remembered by the services of one [`Services`] (in the app, every open document's), the
/// most recently used last, and the most bytes they may hold together: one bound for all, so that
/// several heavy documents open don't each keep their own share.
pub(crate) struct Memos {
    list: Mutex<Vec<Memo>>,
    budget: usize,
}

impl Default for Memos {
    fn default() -> Self {
        Self::with_budget(MAX_MEMO_BYTES)
    }
}

impl Memos {
    /// Memos within `budget` bytes (small, in tests that drop plans).
    pub(crate) fn with_budget(budget: usize) -> Self {
        Self { list: Mutex::new(Vec::new()), budget }
    }

    /// Drop the plans of document `owner` (it closed).
    fn forget(&self, owner: u64) {
        if let Ok(mut list) = self.list.lock() {
            list.retain(|m| m.owner != owner);
        }
    }

    /// The bytes the plans remembered hold, their scenes included (counted ahead until built).
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.list.lock().map_or(0, |l| l.iter().map(Memo::held).sum())
    }
}

/// The last plan a service made: of which snapshot (held weakly: the plan doesn't keep a closed
/// or undone document alive) cut to which selection, with which options, and with which fonts: a
/// text clipping path is outlined with the fonts installed, so a font added since can change the
/// plan of the same snapshot ([`fonts_of`]: the fonts' generation where the snapshot may outline
/// type, 0 elsewhere). A design too large to plan is remembered too, so that the commands that read it one
/// after another are refused at once rather than each planning up to the limit again; a cancelled
/// plan never is (the next one plans).
struct Memo {
    /// The service that made it ([`PlanService::owner`]).
    owner: u64,
    snapshot: Weak<Document>,
    selection: Vec<NodeId>,
    opts: Options,
    fonts: u64,
    plan: Result<Planned, PlanError>,
    /// About how many bytes the plan holds, and how many records it has (its scene is counted
    /// ahead from them until it is built).
    bytes: usize,
    records: usize,
}

impl Memo {
    fn of(&self, of: Of<'_>, opts: &Options, fonts: u64) -> bool {
        std::ptr::eq(self.snapshot.as_ptr(), Arc::as_ptr(of.snapshot)) && self.selection == of.selection && self.opts == *opts && self.fonts == fonts
    }

    /// The bytes it holds, its scene's included (as built, or as it will be) and its records'
    /// flags for the Stitches tab once built.
    fn held(&self) -> usize {
        let scene = match self.plan.as_ref().ok().map(|p| p.scene.get()) {
            Some(Some(built)) => built.byte_size(),
            Some(None) => self.records.saturating_mul(SCENE_BYTES_PER_RECORD),
            None => 0,
        };
        let flags = self.plan.as_ref().ok().and_then(|p| p.flags.lock().ok().and_then(|f| f.as_ref().map(|f| f.byte_size()))).unwrap_or(0);
        self.bytes.saturating_add(scene).saturating_add(flags)
    }
}

/// Milliseconds in a second: a walk's time is kept in milliseconds.
#[cfg(not(target_arch = "wasm32"))]
const MS_PER_SEC: f64 = 1000.0;

/// A plan as the service remembers it: the plan, its generation (a number no other plan of the
/// process has, which what is drawn from it is keyed by), how long the walk that made it took, and
/// its stitch scene, built once on first request ([`Planned::scene`]).
#[derive(Clone, Debug)]
pub struct Planned {
    pub plan: Arc<DesignPlan>,
    pub generation: u64,
    /// How long the walk that made the plan took, milliseconds, whoever asked for it first: a
    /// caller that takes it remembered (the stitch preview after a read command) knows how heavy
    /// the design is to plan. `None` where the walk isn't timed (the web, which has no clock here).
    pub walk_ms: Option<f64>,
    scene: Arc<OnceLock<Arc<StitchScene>>>,
    /// Its records' flags for the Stitches tab ([`Planned::stitch_flags`]), for the limits they
    /// were built with.
    flags: Arc<Mutex<Option<Arc<super::stitch_list::Flags>>>>,
}

impl Planned {
    fn new(plan: Arc<DesignPlan>) -> Self {
        Self { plan, generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed), walk_ms: None, scene: Arc::default(), flags: Arc::default() }
    }

    /// The plan's records' flags judged against `limits` (the Stitches tab's filters): built the
    /// first time they are asked for with these limits, on the asking thread (the planning thread,
    /// for the tab), and kept with the plan.
    pub fn stitch_flags(&self, limits: &super::check::StitchLimits) -> Arc<super::stitch_list::Flags> {
        if let Some(kept) = self.built_stitch_flags(limits) {
            return kept;
        }
        let made = Arc::new(super::stitch_list::Flags::of(&self.plan, limits));
        // A walk cut short by a cancel is incomplete: never kept.
        if !vectorcraft_doc::cancel::is_cancelled()
            && let Ok(mut kept) = self.flags.lock()
        {
            *kept = Some(made.clone());
        }
        made
    }

    /// The flags for `limits` when they have been built already.
    pub fn built_stitch_flags(&self, limits: &super::check::StitchLimits) -> Option<Arc<super::stitch_list::Flags>> {
        let key = super::stitch_list::limits_key(limits);
        self.flags.lock().ok()?.as_ref().filter(|f| f.limits() == key).cloned()
    }

    /// The plan's stitch scene, built the first time it is asked for (on the asking thread, outside
    /// the service's locks) and remembered with the plan.
    pub fn scene(&self) -> Arc<StitchScene> {
        self.scene.get_or_init(|| Arc::new(scene(&self.plan))).clone()
    }

    /// The scene when it has been built already.
    pub fn built_scene(&self) -> Option<Arc<StitchScene>> {
        self.scene.get().cloned()
    }
}

impl PartialEq for Planned {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation
    }
}

/// One document's planning: its units' stitches kept between plans, and its last plans.
#[derive(Default)]
pub(crate) struct PlanService {
    units: Mutex<UnitCache>,
    /// The plans remembered, with those of the other documents' services, and which are its own.
    memos: Arc<Memos>,
    owner: u64,
    #[cfg(not(target_arch = "wasm32"))]
    flights: Flights,
    /// How many times it walked a document (the tests count plans made).
    #[cfg(test)]
    walks: std::sync::atomic::AtomicUsize,
}

impl PlanService {
    /// The service of document `owner`, remembering its plans in `memos` with the other
    /// documents'.
    pub(crate) fn sharing(memos: Arc<Memos>, owner: u64) -> Self {
        Self { memos, owner, ..Self::default() }
    }

    /// The plan of `doc`, the document planned of `of`, with `opts`: the last plan when it was of
    /// this snapshot cut the same way with these options; else planned, each unit's stitches taken
    /// from those kept where nothing it is planned from has changed, and kept for the next plan.
    /// The same as a fresh plan ([`super::digitize::digitize`]).
    pub(crate) fn plan(&self, of: Of<'_>, doc: &Document, opts: &Options) -> Result<Arc<DesignPlan>, PlanError> {
        self.planned(of, doc, opts).map(|p| p.plan)
    }

    /// [`PlanService::plan`], with the plan's generation and scene.
    pub(crate) fn planned(&self, of: Of<'_>, doc: &Document, opts: &Options) -> Result<Planned, PlanError> {
        self.planned_and_walked(of, doc, opts).0
    }

    /// [`PlanService::planned`], and whether this call walked the document for it (`false`: the
    /// plan, or the refusal, was remembered, or another caller's was waited for).
    pub(crate) fn planned_and_walked(&self, of: Of<'_>, doc: &Document, opts: &Options) -> (Result<Planned, PlanError>, bool) {
        #[cfg(not(target_arch = "wasm32"))]
        return self.planned_waiting(of, doc, opts, std::time::Duration::from_millis(MAX_FLIGHT_WAIT_MS));
        #[cfg(target_arch = "wasm32")]
        {
            let fonts = fonts_of(of.snapshot);
            match self.remembered(of, opts, fonts) {
                Some(plan) => (plan, false),
                None => (self.walk(of, doc, opts, fonts), true),
            }
        }
    }

    /// [`PlanService::planned_and_walked`], waiting at most `max_wait` for another caller that
    /// plans the same.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn planned_waiting(
        &self,
        of: Of<'_>,
        doc: &Document,
        opts: &Options,
        max_wait: std::time::Duration,
    ) -> (Result<Planned, PlanError>, bool) {
        let fonts = fonts_of(of.snapshot);
        let asked = std::time::Instant::now();
        loop {
            if let Some(plan) = self.remembered(of, opts, fonts) {
                return (plan, false);
            }
            match self.take_off(of, opts, fonts, asked, max_wait) {
                Ok(Turn::Plan(landing)) => {
                    let plan = self.walk(of, doc, opts, fonts);
                    drop(landing);
                    return (plan, true);
                }
                Ok(Turn::LookAgain) => {}
                Err(e) => return (Err(e), false),
            }
        }
    }

    /// Start the flight of this plan, or wait for the one under way to land: then look again for
    /// the plan it remembered. `Cancelled` when this caller was cancelled while it waited.
    #[cfg(not(target_arch = "wasm32"))]
    fn take_off(
        &self,
        of: Of<'_>,
        opts: &Options,
        fonts: u64,
        asked: std::time::Instant,
        max_wait: std::time::Duration,
    ) -> Result<Turn<'_>, PlanError> {
        let flights = &self.flights;
        let alone = || Ok(Turn::Plan(Landing { flights, id: None }));
        let Ok(mut under_way) = flights.under_way.lock() else { return alone() };
        if !under_way.1.iter().any(|f| f.of(of, opts, fonts)) {
            // Another's flight may have landed since this caller looked: its plan is remembered
            // before its flight comes off the list, so look again rather than plan it twice. (The
            // memos are taken with the flights held, never the other way round.)
            if self.remembered(of, opts, fonts).is_some() {
                return Ok(Turn::LookAgain);
            }
            under_way.0 += 1;
            let id = under_way.0;
            let flight = Flight { id, snapshot: Arc::downgrade(of.snapshot), selection: of.selection.to_vec(), opts: *opts, fonts };
            under_way.1.push(flight);
            return Ok(Turn::Plan(Landing { flights, id: Some(id) }));
        }
        loop {
            if vectorcraft_doc::cancel::is_cancelled() {
                return Err(PlanError::Cancelled);
            }
            if asked.elapsed() >= max_wait {
                return alone();
            }
            let Ok((guard, _)) = flights.landed.wait_timeout(under_way, std::time::Duration::from_millis(FLIGHT_CHECK_MS)) else { return alone() };
            under_way = guard;
            if !under_way.1.iter().any(|f| f.of(of, opts, fonts)) {
                return Ok(Turn::LookAgain);
            }
        }
    }

    /// Walk `doc` (planned of `of`) with `opts` and the fonts of generation `fonts`, and remember
    /// the plan, or the refusal (never a cancel).
    fn walk(&self, of: Of<'_>, doc: &Document, opts: &Options, fonts: u64) -> Result<Planned, PlanError> {
        #[cfg(test)]
        self.walks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        let plan = digitize(doc, opts, Some(&self.units)).map(|p| Planned::new(Arc::new(p)));
        #[cfg(not(target_arch = "wasm32"))]
        let plan = plan.map(|p| Planned { walk_ms: Some(started.elapsed().as_secs_f64() * MS_PER_SEC), ..p });
        if plan.as_ref().err() != Some(&PlanError::Cancelled) {
            let bytes = plan.as_ref().map_or(0, |p| p.plan.byte_size());
            let records = plan.as_ref().map_or(0, |p| p.plan.blocks.iter().map(|b| b.stitches.len()).sum());
            let snapshot = Arc::downgrade(of.snapshot);
            let (selection, opts) = (of.selection.to_vec(), *opts);
            self.remember(Memo { owner: self.owner, snapshot, selection, opts, fonts, plan: plan.clone(), bytes, records });
        }
        plan
    }

    /// A copy of the units this service keeps, for planning documents never kept (a dry run, or a
    /// command's trial of a change): it plans with the units kept so far, and what it adds is its
    /// own, so the trials never push the real document's units out.
    pub(crate) fn scratch(&self) -> std::sync::Mutex<UnitCache> {
        std::sync::Mutex::new(self.units.lock().map(|u| u.clone()).unwrap_or_default())
    }

    /// Keep `memo` as the most recent, dropping the plans of snapshots gone (no plan of theirs can
    /// be asked for again), the service's own least recently used beyond [`MAX_MEMOS`], and the
    /// least recently used of any document while they hold more than the budget
    /// ([`MAX_MEMO_BYTES`]; never the newest). A memo list that a panic elsewhere left locked is
    /// passed by: the next plan walks again.
    fn remember(&self, memo: Memo) {
        let Ok(mut memos) = self.memos.list.lock() else { return };
        memos.retain(|m| {
            m.snapshot.strong_count() > 0 && !(m.snapshot.ptr_eq(&memo.snapshot) && m.selection == memo.selection && m.opts == memo.opts)
        });
        memos.push(memo);
        while memos.iter().filter(|m| m.owner == self.owner).count() > MAX_MEMOS {
            let Some(oldest) = memos.iter().position(|m| m.owner == self.owner) else { break };
            memos.remove(oldest);
        }
        while memos.len() > 1 && memos.iter().map(Memo::held).sum::<usize>() > self.memos.budget {
            memos.remove(0);
        }
    }

    /// The newest plan it remembers of a whole document with `opts` (of the newest snapshot it was
    /// asked for, used most recently), with that snapshot while it is still held: what the
    /// embroidery tool's handles read.
    pub(crate) fn newest(&self, opts: &Options) -> Option<(Arc<Document>, Planned)> {
        let memos = self.memos.list.lock().ok()?;
        memos
            .iter()
            .rev()
            .filter(|m| m.owner == self.owner && m.selection.is_empty() && m.opts == *opts)
            .find_map(|m| Some((m.snapshot.upgrade()?, m.plan.as_ref().ok()?.clone())))
    }

    /// The plan remembered (or why there is none) of snapshot `of` with `opts` and the fonts of
    /// generation `fonts`, made the most recently used. A snapshot held only weakly can't be
    /// mistaken for another made since: its memory isn't reused while a weak reference is held.
    pub(super) fn remembered(&self, of: Of<'_>, opts: &Options, fonts: u64) -> Option<Result<Planned, PlanError>> {
        let mut memos = self.memos.list.lock().ok()?;
        let at = memos.iter().position(|m| m.of(of, opts, fonts))?;
        let memo = memos.remove(at);
        let plan = memo.plan.clone();
        memos.push(memo);
        Some(plan)
    }

    /// How many of its plans are remembered.
    #[cfg(test)]
    pub(crate) fn memos(&self) -> usize {
        self.memos.list.lock().map_or(0, |m| m.iter().filter(|m| m.owner == self.owner).count())
    }

    /// How many plans are under way.
    #[cfg(test)]
    pub(crate) fn flights(&self) -> usize {
        self.flights.under_way.lock().map_or(0, |f| f.1.len())
    }

    /// Run `f` as if a flight of `of` with `opts` were under way, which ends when `f` returns or
    /// unwinds (the tests' stand-in for a plan another caller makes).
    #[cfg(test)]
    pub(crate) fn in_flight<T>(&self, of: Of<'_>, opts: &Options, f: impl FnOnce() -> T) -> T {
        let fonts = fonts_of(of.snapshot);
        let landing = match self.take_off(of, opts, fonts, std::time::Instant::now(), std::time::Duration::ZERO) {
            Ok(Turn::Plan(landing)) => landing,
            _ => panic!("a flight under way already"),
        };
        let out = f();
        drop(landing);
        out
    }

    /// A service whose cache holds at most `limit` stitches (small, in tests that drop units).
    #[cfg(test)]
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self { units: Mutex::new(UnitCache::with_limit(limit)), ..Self::default() }
    }

    /// How many areas the path operations made are kept.
    #[cfg(test)]
    pub(crate) fn shapes(&self) -> usize {
        self.units.lock().map_or(0, |c| c.shapes())
    }

    /// How many fills' narrow parts and rests are kept.
    #[cfg(test)]
    pub(crate) fn parted(&self) -> usize {
        self.units.lock().map_or(0, |c| c.parted())
    }

    /// How many times it walked a document.
    #[cfg(test)]
    pub(crate) fn walks(&self) -> usize {
        self.walks.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many units are kept.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.units.lock().map_or(0, |c| c.units())
    }
}

/// The service is shared between threads: the app plans on a worker while commands plan on theirs.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<PlanService>();
};

/// `doc`'s plan with `opts`, made afresh: for a caller with no open document's service at hand.
/// The export plans the document it writes this way, as edited for the file, so that what a file
/// holds never depends on what was kept.
pub(crate) fn plan_afresh(doc: &Document, opts: &Options) -> Result<DesignPlan, PlanError> {
    digitize(doc, opts, None)
}

/// The planning service of each open document, by [`crate::DocState::uid`], and its stitch
/// preview's view setting ([`super::view::StitchView`]).
#[derive(Default)]
pub(crate) struct Services {
    plans: HashMap<u64, Arc<PlanService>>,
    /// The plans the services remember, within one bound for all.
    memos: Arc<Memos>,
    views: HashMap<u64, super::view::StitchView>,
    /// The plan the interface draws of each document, with the snapshot it was planned from: what
    /// the embroidery tool's answers are of ([`Services::tool_facts`]).
    shown: HashMap<u64, (Arc<Document>, Planned)>,
    /// The embroidery tool's answers of each document's plan, built once a plan.
    facts: HashMap<u64, Arc<super::tool::ToolFacts>>,
    /// The needle points stitch editing works on in each document: the paths of stitches inside the
    /// object isolated, while the embroidery tool works in it ([`Services::needle_index`]), with the
    /// snapshot and the object they are of (`None`: it holds no path of stitches).
    needles: HashMap<u64, KeptNeedles>,
}

/// The needle points kept for a document ([`Services::needle_index`]): the snapshot and the object
/// isolated they are of, and the index (`None`: the object holds no path of stitches).
type KeptNeedles = (Arc<Document>, NodeId, Option<Arc<super::needle_index::NeedleIndex>>);

impl Services {
    /// Document `uid`'s service, made when it is first asked for.
    pub(crate) fn of(&mut self, uid: u64) -> Arc<PlanService> {
        let memos = &self.memos;
        self.plans.entry(uid).or_insert_with(|| Arc::new(PlanService::sharing(memos.clone(), uid))).clone()
    }

    /// Document `uid`'s stitch view (the default until one is set).
    pub(crate) fn view(&self, uid: u64) -> super::view::StitchView {
        self.views.get(&uid).copied().unwrap_or_default()
    }

    pub(crate) fn set_view(&mut self, uid: u64, view: super::view::StitchView) {
        self.views.insert(uid, view);
    }

    /// Drop document `uid`'s service (the document closed), what it kept and its stitch view.
    pub(crate) fn forget(&mut self, uid: u64) {
        self.plans.remove(&uid);
        self.memos.forget(uid);
        self.views.remove(&uid);
        self.shown.remove(&uid);
        self.facts.remove(&uid);
        self.needles.remove(&uid);
    }

    /// The interface draws `planned`, the plan of `snapshot`, for document `uid`.
    pub(crate) fn show(&mut self, uid: u64, snapshot: Arc<Document>, planned: Planned) {
        if self.shown.get(&uid).is_none_or(|(_, p)| p.generation != planned.generation) {
            self.shown.insert(uid, (snapshot, planned));
        }
    }

    /// The embroidery tool's answers kept, dropped (the tool put away): they hold a plan's ends and
    /// its scene, built again when the tool is chosen again.
    pub(crate) fn drop_facts(&mut self) {
        self.facts.clear();
        self.needles.clear();
    }

    /// The needle points stitch editing works on in document `st` (the embroidery tool working
    /// inside isolation of an object holding paths of stitches, the project owner's choice,
    /// 2026-10-27): the paths of stitches inside the object isolated, built from the document as it
    /// is and kept for its snapshot, a path's needle points built again only when an edit changed it
    /// ([`super::needle_index`]); `None` when nothing is isolated or the object isolated holds no
    /// path of stitches, and then what was kept goes.
    pub(crate) fn needle_index(&mut self, st: &crate::DocState) -> Option<Arc<super::needle_index::NeedleIndex>> {
        let Some(scope) = st.isolation else {
            self.needles.remove(&st.uid);
            return None;
        };
        if let Some((doc, kept_scope, index)) = self.needles.get(&st.uid)
            && Arc::ptr_eq(doc, &st.doc)
            && *kept_scope == scope
        {
            return index.clone();
        }
        let kept = self.needles.remove(&st.uid).and_then(|(_, s, i)| i.filter(|_| s == scope));
        let index = super::needle_index::NeedleIndex::of(&st.doc, &[scope], kept.as_deref()).map(Arc::new);
        self.needles.insert(st.uid, (st.doc.clone(), scope, index.clone()));
        index
    }

    /// How many documents' needle points stitch editing keeps.
    #[cfg(test)]
    pub(crate) fn kept_needles(&self) -> usize {
        self.needles.len()
    }

    /// How many documents' answers the embroidery tool keeps.
    #[cfg(test)]
    pub(crate) fn kept_facts(&self) -> usize {
        self.facts.len()
    }

    /// The embroidery tool's answers for document `uid`: of the plan the interface draws of it
    /// ([`Services::show`]: what is drawn is what is caught), else, with no interface (an agent, a
    /// test), of the newest plan its service remembers of the whole document with `opts` (the stitch
    /// preview's); built once a plan; `None` when there is neither. Never plans.
    pub(crate) fn tool_facts(&mut self, uid: u64, opts: &Options) -> Option<Arc<super::tool::ToolFacts>> {
        let (snapshot, planned) = match self.shown.get(&uid) {
            Some((snapshot, planned)) => (snapshot.clone(), planned.clone()),
            None => self.plans.get(&uid)?.newest(opts)?,
        };
        if let Some(kept) = self.facts.get(&uid).filter(|f| f.generation == planned.generation) {
            return Some(kept.clone());
        }
        let facts = Arc::new(super::tool::ToolFacts::of(snapshot, &planned));
        self.facts.insert(uid, facts.clone());
        Some(facts)
    }

    /// The bytes the plans remembered hold.
    #[cfg(test)]
    pub(crate) fn memos_held(&self) -> usize {
        self.memos.held()
    }

    /// The documents that have a service or a stitch view.
    #[cfg(test)]
    pub(crate) fn uids(&self) -> Vec<u64> {
        let mut uids: Vec<u64> = self.plans.keys().chain(self.views.keys()).copied().collect();
        uids.sort_unstable();
        uids.dedup();
        uids
    }
}
