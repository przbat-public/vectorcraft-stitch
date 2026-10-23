//! The planning service: one per open document ([`Services`]), it plans the document's art into
//! its stitch plan and keeps each sewing unit's stitches between plans ([`UnitCache`]), so that a
//! plan after an edit plans again only the units the edit changed.
//!
//! It also remembers its last plan, of which snapshot of the document and with which options: a
//! document snapshot (`Arc<Document>`, shared with the undo history) never changes, so the same
//! snapshot with the same options has the same plan, and the commands that read a design one
//! after another (`embroidery.threads`, and the statistics, sewing order and checks) and the app's
//! planner plan it once. The document planned may be cut from the snapshot (the selected objects
//! alone, or without an opacity mask's editing layer): the plan is remembered by the snapshot and
//! the selection it was cut to ([`Of`]), not by the copy, which is made anew each time.
//!
//! A plan can be cancelled ([`vectorcraft_doc::cancel`]): it stops before the next unit (or within
//! a fill's search for its narrow parts) with [`PlanError::Cancelled`] and leaves no plan, while
//! the units it finished stay kept for the next. Each unit is reported as a step of its progress.
//! The service is `Send` and `Sync`, so the app can plan on a worker thread while commands plan
//! on theirs; commands and the web build plan synchronously.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use vectorcraft_doc::{Document, NodeId};

use super::Options;
use super::cache::UnitCache;
use super::digitize::digitize;
use super::plan::DesignPlan;
use super::unit::PlanError;

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

/// The last plan a service made: of which snapshot (held weakly: the plan doesn't keep a closed
/// or undone document alive) cut to which selection, with which options, and with which fonts: a
/// text clipping path is outlined with the fonts installed, so a font added since can change the
/// plan of the same snapshot ([`vectorcraft_text::FontDb::generation`], as the renderer's caches
/// key on it). A design too large to plan is remembered too, so that the commands that read it one
/// after another are refused at once rather than each planning up to the limit again; a cancelled
/// plan never is (the next one plans).
struct Memo {
    snapshot: Weak<Document>,
    selection: Vec<NodeId>,
    opts: Options,
    fonts: u64,
    plan: Result<Arc<DesignPlan>, PlanError>,
}

/// One document's planning: its units' stitches kept between plans, and its last plan.
#[derive(Default)]
pub(crate) struct PlanService {
    units: Mutex<UnitCache>,
    last: Mutex<Option<Memo>>,
    /// How many times it walked a document (the tests count plans made).
    #[cfg(test)]
    walks: std::sync::atomic::AtomicUsize,
}

impl PlanService {
    /// The plan of `doc`, the document planned of `of`, with `opts`: the last plan when it was of
    /// this snapshot cut the same way with these options; else planned, each unit's stitches taken
    /// from those kept where nothing it is planned from has changed, and kept for the next plan.
    /// The same as a fresh plan ([`super::digitize::digitize`]).
    pub(crate) fn plan(&self, of: Of<'_>, doc: &Document, opts: &Options) -> Result<Arc<DesignPlan>, PlanError> {
        let fonts = vectorcraft_text::FontDb::global().generation();
        if let Some(plan) = self.remembered(of, opts, fonts) {
            return plan;
        }
        #[cfg(test)]
        self.walks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let plan = digitize(doc, opts, Some(&self.units)).map(Arc::new);
        // A memo that a panic elsewhere left locked is passed by: the next plan walks again.
        if plan.as_ref().err() != Some(&PlanError::Cancelled)
            && let Ok(mut last) = self.last.lock()
        {
            let snapshot = Arc::downgrade(of.snapshot);
            *last = Some(Memo { snapshot, selection: of.selection.to_vec(), opts: opts.clone(), fonts, plan: plan.clone() });
        }
        plan
    }

    /// The last plan (or why there is none), when it was of snapshot `doc` with `opts` and the
    /// fonts of generation `fonts`. A snapshot held only weakly can't be mistaken for another made
    /// since: its memory isn't reused while a weak reference is held.
    pub(super) fn remembered(&self, of: Of<'_>, opts: &Options, fonts: u64) -> Option<Result<Arc<DesignPlan>, PlanError>> {
        let last = self.last.lock().ok()?;
        let memo = last.as_ref()?;
        let same = std::ptr::eq(memo.snapshot.as_ptr(), Arc::as_ptr(of.snapshot)) && memo.selection == of.selection;
        (same && memo.opts == *opts && memo.fonts == fonts).then(|| memo.plan.clone())
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

/// The planning service of each open document, by [`crate::DocState::uid`].
#[derive(Default)]
pub(crate) struct Services(HashMap<u64, Arc<PlanService>>);

impl Services {
    /// Document `uid`'s service, made when it is first asked for.
    pub(crate) fn of(&mut self, uid: u64) -> Arc<PlanService> {
        self.0.entry(uid).or_default().clone()
    }

    /// Drop document `uid`'s service (the document closed), and what it kept.
    pub(crate) fn forget(&mut self, uid: u64) {
        self.0.remove(&uid);
    }

    /// The documents that have a service.
    #[cfg(test)]
    pub(crate) fn uids(&self) -> Vec<u64> {
        let mut uids: Vec<u64> = self.0.keys().copied().collect();
        uids.sort_unstable();
        uids
    }
}
