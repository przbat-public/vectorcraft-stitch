//! The stitch preview's scheduler: when the active document's design is planned, and what the
//! canvas, the Embroidery panel and the status bar show meanwhile.
//!
//! - **Only on demand.** Something drawn in the frame asks ([`StitchPreview::want_plan`] for the
//!   canvas showing stitches, [`StitchPreview::want`] for a read command's reply): an app that never
//!   shows stitches never plans.
//! - **Tagged by what it is the plan of** ([`EmbroideryPlanKey`]: the snapshot, the options, the
//!   fonts), never by the document's revision, which also moves on selection and view changes.
//! - **Light and heavy designs.** A design whose last plan took at most [`LIVE_PLAN_MS`] is planned
//!   on every change, also during a drag, and the plan under way is let finish, so the stitches
//!   follow the drag. Any other is planned once the document has stayed unchanged for
//!   [`QUIET_SECS`] with no drag, and a new change cancels the plan under way. The first plan of a
//!   document starts at once.
//! - **The last plan stays shown** while a newer one is made ([`Freshness::Stale`]); after
//!   [`STALE_NOTICE_SECS`] the preview says it is planning. A design that can't be planned keeps
//!   its last good stitches with the reason ([`Freshness::Failed`]), and isn't planned again until
//!   it changes.
//! - **A cancelled or replaced job's result is never shown**: a step is taken in only when it is of
//!   the job under way and that job wasn't cancelled.
//! - **Per document**: each open document keeps its last plan, so switching back shows it at once;
//!   never another document's. A document's state goes when it closes (also when a failed batch
//!   closes the document it opened).
//!
//! The plan is made on the planning thread ([`super::plan_worker`]), or in line where there is
//! none. The options are the preview's ([`Session::stitch_preview_options`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_engine::cmd::fileio::progress::Progress;
use vectorcraft_engine::doc::Document;
use vectorcraft_engine::{EmbroideryOptions, EmbroideryPlanKey, EmbroideryPlanned, Session};

use super::plan_worker::{Done, Driver, Job, Outcome, Query};

/// How long the document must stay unchanged before a design that is slow to plan is planned
/// again, seconds: the panels' settle time.
pub const QUIET_SECS: f64 = crate::thumb_worker::SETTLE_SECS;

/// A design whose last plan took at most this long, milliseconds, is planned on every change,
/// also during a drag: half the 100 ms re-plan budget of `vectorcraft-cli perf`'s embroidery rows,
/// so a change still shows within that budget when it waits for the plan before it. An estimate.
pub const LIVE_PLAN_MS: f64 = 50.0;

/// How long the stitches shown may be out of date before the preview says it is planning,
/// seconds: as long as an export runs before its progress shows.
pub const STALE_NOTICE_SECS: f64 = crate::export_progress::DELAY;

/// How often the preview is drawn again while it says it is planning, seconds (for the progress):
/// as often as the status bar's background jobs.
pub const PROGRESS_REPAINT_SECS: f64 = 0.1;

/// Where the preview's jobs run: the planning thread or in line ([`Driver`]), or the tests' own.
pub trait Runner {
    /// Send `job`, or run it.
    fn submit(&mut self, job: Job);
    /// The steps finished since the last call, oldest first.
    fn poll(&mut self) -> Vec<Done>;
}

impl Runner for Driver {
    fn submit(&mut self, job: Job) {
        Driver::submit(self, job);
    }

    fn poll(&mut self) -> Vec<Done> {
        Driver::poll(self)
    }
}

/// A plan shown: what it is of, the plan with its scene, and the replies made from it.
pub struct Shown {
    pub key: EmbroideryPlanKey,
    pub planned: EmbroideryPlanned,
    /// The document it was planned from: an object whose node differs now changed since.
    pub document: Arc<Document>,
    replies: Vec<Reply>,
}

/// A read command's reply, with the generation of the plan it was made from.
struct Reply {
    query: Query,
    generation: u64,
    result: Result<Value, String>,
}

impl Shown {
    /// The reply to `query`, and whether it is of the plan shown (a reply of the plan before stays
    /// until the new one's comes).
    pub fn reply(&self, query: &Query) -> Option<(&Result<Value, String>, bool)> {
        self.replies.iter().find(|r| r.query == *query).map(|r| (&r.result, r.generation == self.planned.generation))
    }

    /// Whether `query` has a reply of the plan shown.
    fn answers(&self, query: &Query) -> bool {
        self.reply(query).is_some_and(|(_, current)| current)
    }
}

/// How the stitches shown stand to the document as it is.
#[derive(Clone, Debug, PartialEq)]
pub enum Freshness {
    /// Nothing asked for stitches in the last frame.
    Off,
    /// The plan shown is of the document as it is.
    Fresh,
    /// No plan of this document yet: one is waiting or under way. `progress` (0 to 1) when it can
    /// say; `notice` once the preview should say it is planning.
    Pending { progress: Option<f64>, notice: bool },
    /// A plan of an earlier state is shown while a newer one waits or is made.
    Stale { progress: Option<f64>, notice: bool },
    /// The document as it is can't be planned: why. The last plan, if any, stays shown.
    Failed { error: String },
}

impl Freshness {
    /// The id `ui.inspect` gives it.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Fresh => "fresh",
            Self::Pending { .. } => "pending",
            Self::Stale { .. } => "stale",
            Self::Failed { .. } => "failed",
        }
    }
}

/// One open document's preview.
#[derive(Default)]
struct DocPreview {
    shown: Option<Shown>,
    /// The key that failed, and why: not planned again until the document changes.
    failed: Option<(EmbroideryPlanKey, String)>,
    /// How long the last job that really planned took, milliseconds.
    plan_ms: Option<f64>,
    /// The key wanted at the last frame, and since when (egui time, seconds).
    wanted: Option<(EmbroideryPlanKey, f64)>,
    /// Since when the plan shown hasn't been of the document as it is.
    stale_since: Option<f64>,
}

/// The job under way: the active document's.
struct InFlight {
    uid: u64,
    ticket: u64,
    key: EmbroideryPlanKey,
    /// The queries as asked (the job's carry the options).
    queries: Vec<Query>,
    progress: Arc<Progress>,
    /// Its key wasn't the one shown when it was sent: it plans (its time says how heavy the design
    /// is), rather than only making replies.
    plans: bool,
    /// The document it plans.
    document: Arc<Document>,
}

/// What this frame's consumers asked for.
#[derive(Default)]
struct Interest {
    plan: bool,
    queries: Vec<Query>,
}

/// The stitch preview's scheduler and what it shows ([`self`]).
#[derive(Default)]
pub struct StitchPreview {
    docs: HashMap<u64, DocPreview>,
    in_flight: Option<InFlight>,
    /// What this frame asks for, and whether the last frame asked anything.
    interest: Interest,
    asked: bool,
    runner: Option<Box<dyn Runner>>,
    next_ticket: u64,
    /// What the tests check the scheduler against: each job sent, and each plan taken in.
    #[cfg(test)]
    pub(crate) log: Log,
}

/// The scheduler's record for the tests: the jobs sent (their key, and whether they plan), and the
/// plans taken in (their ticket, and whether its job was cancelled then).
#[cfg(test)]
#[derive(Default)]
pub(crate) struct Log {
    pub(crate) sent: Vec<(EmbroideryPlanKey, bool)>,
    pub(crate) taken: Vec<(u64, bool)>,
}

impl StitchPreview {
    /// The canvas shows stitches this frame: the active document's design is wanted.
    pub fn want_plan(&mut self) {
        self.interest.plan = true;
    }

    /// The reply to `query` (a read command and its own params, the options left out: the
    /// preview's are added) is wanted this frame, with the plan.
    pub fn want(&mut self, query: Query) {
        if !self.interest.queries.contains(&query) {
            self.interest.queries.push(query);
        }
    }

    /// Document `uid`'s plan shown (never another document's).
    pub fn shown(&self, uid: u64) -> Option<&Shown> {
        self.docs.get(&uid)?.shown.as_ref()
    }

    /// Whether document `uid`'s stitches shown are those of the document as it was at the end of
    /// the last frame.
    pub fn is_fresh(&self, uid: u64) -> bool {
        self.docs.get(&uid).is_some_and(|d| matches!((&d.shown, &d.wanted), (Some(s), Some((key, _))) if s.key == *key))
    }

    /// How document `uid`'s stitches shown stand to it as it was at the end of the last frame.
    pub fn freshness(&self, uid: u64, now: f64) -> Freshness {
        if !self.asked {
            return Freshness::Off;
        }
        let Some(doc) = self.docs.get(&uid) else { return Freshness::Pending { progress: None, notice: false } };
        let Some((key, _)) = &doc.wanted else { return Freshness::Pending { progress: None, notice: false } };
        if doc.shown.as_ref().is_some_and(|s| s.key == *key) {
            return Freshness::Fresh;
        }
        if let Some((failed, error)) = &doc.failed
            && failed == key
        {
            return Freshness::Failed { error: error.clone() };
        }
        let progress = self.in_flight.as_ref().filter(|f| f.key == *key && f.plans).and_then(|f| f.progress.fraction());
        let notice = doc.stale_since.is_some_and(|since| !(0.0..STALE_NOTICE_SECS).contains(&(now - since)));
        match doc.shown {
            None => Freshness::Pending { progress, notice },
            Some(_) => Freshness::Stale { progress, notice },
        }
    }

    /// For how long, seconds, document `uid`'s stitches shown have not been of it as it is (a newer
    /// plan waits or is made): `None` while they are, or it can't be planned.
    pub fn out_of_date_for(&self, uid: u64, now: f64) -> Option<f64> {
        self.docs.get(&uid)?.stale_since.map(|since| now - since)
    }

    /// Whether document `uid` as it was at the end of the last frame can't be planned.
    pub fn failed(&self, uid: u64) -> bool {
        self.docs.get(&uid).is_some_and(|d| matches!((&d.failed, &d.wanted), (Some((f, _)), Some((w, _))) if f == w))
    }

    /// Whether the preview of document `uid` is waiting for a plan or making one, while asked:
    /// `ui.screenshot` waits for it.
    pub fn busy(&self, uid: u64, now: f64) -> bool {
        matches!(self.freshness(uid, now), Freshness::Pending { .. } | Freshness::Stale { .. })
    }

    /// What `ui.inspect` reports of document `uid`'s preview.
    pub fn inspect(&self, uid: u64, now: f64) -> Value {
        let freshness = self.freshness(uid, now);
        let doc = self.docs.get(&uid);
        let (progress, error) = match &freshness {
            Freshness::Pending { progress, .. } | Freshness::Stale { progress, .. } => (*progress, None),
            Freshness::Failed { error } => (None, Some(error.clone())),
            Freshness::Off | Freshness::Fresh => (None, None),
        };
        json!({
            "state": freshness.id(),
            "planning": self.in_flight.as_ref().is_some_and(|f| f.uid == uid),
            "progress": progress,
            "error": error,
            "planMs": doc.and_then(|d| d.plan_ms),
            "generation": doc.and_then(|d| d.shown.as_ref()).map(|s| s.planned.generation),
        })
    }

    /// Say document `uid`'s last plan took `ms` (the tests make a design heavy).
    #[cfg(test)]
    pub(crate) fn set_plan_ms(&mut self, uid: u64, ms: f64) {
        if let Some(d) = self.docs.get_mut(&uid) {
            d.plan_ms = Some(ms);
        }
    }

    /// Use `runner` for the jobs from now on (the tests' own).
    #[cfg(test)]
    pub(crate) fn with_runner(runner: Box<dyn Runner>) -> Self {
        Self { runner: Some(runner), ..Self::default() }
    }

    /// Cancel the job under way: its steps are dropped from now on.
    fn cancel(&mut self) {
        if let Some(f) = self.in_flight.take() {
            f.progress.cancel();
        }
    }

    /// Take in the steps the jobs finished.
    fn take_in(&mut self) {
        let Some(runner) = self.runner.as_mut() else { return };
        for d in runner.poll() {
            let Some(f) = self.in_flight.as_ref().filter(|f| f.ticket == d.ticket) else { continue };
            if !f.progress.cancelled()
                && let Some(doc) = self.docs.get_mut(&f.uid)
            {
                match d.outcome {
                    Outcome::Planned(planned) => {
                        #[cfg(test)]
                        self.log.taken.push((f.ticket, f.progress.cancelled()));
                        let replies = doc.shown.take().map(|s| s.replies).unwrap_or_default();
                        doc.shown = Some(Shown { key: f.key.clone(), planned, document: f.document.clone(), replies });
                        doc.failed = None;
                        if f.plans {
                            doc.plan_ms = Some(d.ms);
                        }
                    }
                    Outcome::Replies(replies) => {
                        if let Some(shown) = doc.shown.as_mut().filter(|s| s.key == f.key) {
                            let generation = shown.planned.generation;
                            shown.replies =
                                f.queries.iter().cloned().zip(replies).map(|(query, (_, result))| Reply { query, generation, result }).collect();
                        }
                    }
                    Outcome::Failed(error) => {
                        doc.failed = Some((f.key.clone(), error));
                        if f.plans {
                            doc.plan_ms = Some(d.ms);
                        }
                    }
                    Outcome::Cancelled => {}
                }
            }
            if d.last {
                self.in_flight = None;
            }
        }
    }

    /// Decide, at the end of a frame at time `now` (seconds), what to plan for what the frame
    /// asked: `busy` while a drag, a tool's gesture or a field's scrubbing goes on. → in how many
    /// seconds to draw again when nothing else would (a quiet time ending, the notice showing).
    fn schedule_at(&mut self, s: &mut Session, now: f64, busy: bool) -> Option<f64> {
        let interest = std::mem::take(&mut self.interest);
        self.asked = interest.plan || !interest.queries.is_empty();
        let open: HashSet<u64> = s.documents().iter().map(|d| d.uid).collect();
        self.docs.retain(|uid, _| open.contains(uid));
        let active = s.active().map(|st| st.uid);
        if self.in_flight.as_ref().is_some_and(|f| Some(f.uid) != active) {
            self.cancel();
        }
        let Some(uid) = active.filter(|_| self.asked) else {
            self.cancel();
            return None;
        };
        let (opts, _) = s.stitch_preview_options(uid);
        let key = s.embroidery_plan_key(&opts)?;
        let doc = self.docs.entry(uid).or_default();
        if doc.wanted.as_ref().is_none_or(|(k, _)| *k != key) {
            doc.wanted = Some((key.clone(), now));
        }
        let since = doc.wanted.as_ref().map_or(now, |(_, t)| *t);
        let fresh = doc.shown.as_ref().is_some_and(|sh| sh.key == key);
        let failed = doc.failed.as_ref().is_some_and(|(k, _)| *k == key);
        if fresh || failed {
            doc.stale_since = None;
        } else if doc.stale_since.is_none() {
            doc.stale_since = Some(now);
        }
        let stale_since = doc.stale_since;
        let light = doc.plan_ms.is_some_and(|ms| ms <= LIVE_PLAN_MS);
        let first = doc.shown.is_none();
        let flying = self.in_flight.as_ref().map(|f| f.key == key);
        if fresh {
            let missing = doc.shown.as_ref().is_some_and(|sh| interest.queries.iter().any(|q| !sh.answers(q)));
            match flying {
                // An undo back to the plan shown: the job for another state isn't wanted.
                Some(false) => self.cancel(),
                Some(true) => return None,
                None => {}
            }
            if missing {
                self.start(s, uid, key, opts, interest.queries, false);
            }
            return None;
        }
        if failed {
            if flying == Some(false) {
                self.cancel();
            }
            return None;
        }
        let notice = stale_since.map(|t| (STALE_NOTICE_SECS - (now - t)).max(0.0));
        match flying {
            Some(true) => return notice.map(|wait| if wait > 0.0 { wait } else { PROGRESS_REPAINT_SECS }),
            // A light design's plan of an earlier state is let finish: shown, still stale, and the
            // next starts after it.
            Some(false) if light => return notice,
            Some(false) => self.cancel(),
            None => {}
        }
        let waited = now - since;
        // The clock went back (a new egui context): don't wait for it to catch up.
        let quiet = !waited.is_nan() && !(0.0..QUIET_SECS).contains(&waited);
        if light || (!busy && (first || quiet)) {
            self.start(s, uid, key, opts, interest.queries, true);
            return notice;
        }
        if busy {
            // The drag's end draws a frame anyway.
            return notice;
        }
        let rest = (QUIET_SECS - waited).max(0.0);
        Some(notice.map_or(rest, |n| n.min(rest)))
    }

    /// Start a job for document `uid` at `key` with `opts`, making `queries`' replies after the
    /// plan; `plans` when its key isn't the one shown.
    fn start(&mut self, s: &mut Session, uid: u64, key: EmbroideryPlanKey, opts: EmbroideryOptions, queries: Vec<Query>, plans: bool) {
        let Ok(planner) = s.embroidery_planner() else { return };
        let Some(runner) = self.runner.as_mut() else { return };
        self.next_ticket += 1;
        let ticket = self.next_ticket;
        let progress = Progress::new();
        let sent = queries.iter().map(|q| with_options(q, &opts)).collect();
        #[cfg(test)]
        self.log.sent.push((key.clone(), plans));
        let document = planner.document().clone();
        self.in_flight = Some(InFlight { uid, ticket, key, queries, progress: progress.clone(), plans, document });
        runner.submit(Job::new(ticket, planner, opts, sent, progress));
    }
}

/// `q` with the embroidery options `opts` among its params (its own keys win).
fn with_options(q: &Query, opts: &EmbroideryOptions) -> Query {
    let mut params = opts.to_json();
    if let (Some(all), Some(own)) = (params.as_object_mut(), q.params.as_object()) {
        all.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    Query { cmd: q.cmd, params }
}

/// Start of a frame: take in what the planning finished.
pub fn poll(app: &mut crate::VectorcraftApp) {
    app.embroidery.preview.take_in();
}

/// End of a frame: plan what the frame asked for, on the planning thread (started on first use)
/// or in line.
pub fn schedule(app: &mut crate::VectorcraftApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let busy = app.session.active().is_some_and(|st| st.interaction.is_some()) || app.session.tool_busy() || app.scrub_group;
    let preview = &mut app.embroidery.preview;
    if preview.runner.is_none() && (preview.interest.plan || !preview.interest.queries.is_empty()) {
        preview.runner = Some(Box::new(Driver::new(ctx)));
    }
    let wait = preview.schedule_at(&mut app.session, now, busy);
    // Planned in line: shown from the next frame.
    preview.take_in();
    if let Some(wait) = wait {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
}

#[cfg(test)]
mod tests;
