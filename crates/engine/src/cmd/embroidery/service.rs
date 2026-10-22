//! The planning service: one per open document ([`Services`]), it plans the document's art into
//! its stitch plan and keeps each sewing unit's stitches between plans ([`UnitCache`]), so that a
//! plan after an edit plans again only the units the edit changed.
//!
//! It also remembers its last plan, of which snapshot of the document and with which options: a
//! document snapshot (`Arc<Document>`, shared with the undo history) never changes, so the same
//! snapshot with the same options has the same plan, and the commands that read a design one
//! after another (`embroidery.threads`, and the statistics, sewing order and checks) plan it once.
//!
//! A plan can be cancelled ([`vectorcraft_doc::cancel`]): it stops before the next unit (or within
//! a fill's search for its narrow parts) with [`PlanError::Cancelled`] and leaves no plan, while
//! the units it finished stay kept for the next. Each unit is reported as a step of its progress.
//! The service is `Send` and `Sync`, so the app can plan on a worker thread while commands plan
//! on theirs; commands and the web build plan synchronously.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use vectorcraft_doc::Document;

use super::Options;
use super::cache::UnitCache;
use super::digitize::digitize;
use super::plan::DesignPlan;
use super::unit::PlanError;

/// The last plan a service made: of which snapshot (held weakly: the plan doesn't keep a closed
/// or undone document alive), with which options.
struct Memo {
    doc: Weak<Document>,
    opts: Options,
    plan: Arc<DesignPlan>,
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
    /// `doc`'s plan with `opts`: the last plan when it was of this snapshot with these options;
    /// else planned, each unit's stitches taken from those kept where nothing it is planned from
    /// has changed, and kept for the next plan. The same as a fresh plan
    /// ([`super::digitize::digitize`]).
    pub(crate) fn plan(&self, doc: &Arc<Document>, opts: &Options) -> Result<Arc<DesignPlan>, PlanError> {
        if let Some(plan) = self.remembered(doc, opts) {
            return Ok(plan);
        }
        #[cfg(test)]
        self.walks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let plan = Arc::new(digitize(doc, opts, Some(&self.units))?);
        // A memo that a panic elsewhere left locked is passed by: the next plan walks again.
        if let Ok(mut last) = self.last.lock() {
            *last = Some(Memo { doc: Arc::downgrade(doc), opts: opts.clone(), plan: plan.clone() });
        }
        Ok(plan)
    }

    /// The last plan, when it was of snapshot `doc` with `opts`. A snapshot held only weakly can't
    /// be mistaken for another made since: its memory isn't reused while a weak reference is held.
    fn remembered(&self, doc: &Arc<Document>, opts: &Options) -> Option<Arc<DesignPlan>> {
        let last = self.last.lock().ok()?;
        let memo = last.as_ref()?;
        (std::ptr::eq(memo.doc.as_ptr(), Arc::as_ptr(doc)) && memo.opts == *opts).then(|| memo.plan.clone())
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
}
