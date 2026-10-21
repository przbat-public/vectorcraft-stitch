//! The planning service: one per open document ([`Services`]), it plans the document's art into
//! its stitch plan and keeps each sewing unit's stitches between plans ([`UnitCache`]), so that a
//! plan after an edit plans again only the units the edit changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vectorcraft_doc::Document;

use super::Options;
use super::cache::UnitCache;
use super::digitize::digitize_with;
use super::plan::DesignPlan;

/// One document's planning: its units' stitches kept between plans.
#[derive(Default)]
pub(crate) struct PlanService {
    units: Mutex<UnitCache>,
}

impl PlanService {
    /// `doc`'s plan with `opts`, each unit's stitches taken from those kept where nothing it is
    /// planned from has changed, and kept for the next plan. The same as a fresh plan
    /// ([`super::digitize::digitize`]).
    pub(crate) fn plan(&self, doc: &Document, opts: &Options) -> Result<DesignPlan, String> {
        digitize_with(doc, opts, Some(&self.units))
    }

    /// How many units are kept.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.units.lock().map_or(0, |c| c.units())
    }
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
