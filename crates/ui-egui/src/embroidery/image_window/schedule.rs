//! When the image window digitizes again, by the stitch preview's rules (the project owner's
//! choice, 2026-11-11):
//!
//! - **What a change recomputes** the engine says before it runs, by the keys it keeps its stages
//!   by (`DigitizeRequest::first_stage_changed`): from the first stage that reads something
//!   different of the request run last, the stages before it found kept. So the image's size,
//!   crop or background recompute from the image on; the mode, the colours asked, or a smallest
//!   area under a thread's footprint from the trace on; a colour's choice, a larger smallest area
//!   or the palette from the colours on; an edit to the document under the window from the
//!   objects on.
//! - **Light and heavy.** A change whose stages took at most [`LIVE_PLAN_MS`] when they were last
//!   made (where there is no thread, at most [`INLINE_PLAN_MS`]) is run at once, also while a
//!   slider moves, the run under way let finish; any other once the settings have stayed unchanged
//!   for [`QUIET_SECS`], a newer change cancelling the run under way. A stage never timed counts as
//!   heavy; the first run starts at once.
//! - **The last result stays shown** while a newer one is made: after [`STALE_NOTICE_SECS`] the
//!   window says it is recomputing, with its progress, which only goes forward: each stage handed
//!   over is a share of it, and the stitches' plan, the last, fills its share with the planner's
//!   own units. A run that fails keeps the last good result with the reason.
//! - **A replaced or cancelled run's stages are never taken in**: a step is taken only from the run
//!   under way.

use std::sync::Arc;

use serde_json::Value;
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::embroidery::{DigitizeOutputs, DigitizePipeline, DigitizeRequest, DigitizeStage};
use vectorcraft_engine::cmd::fileio::progress::Progress;
use vectorcraft_engine::doc::NodeId;

use super::worker::{Checked, Done, Driver, Job, Outcome, Painted};
pub(crate) use crate::embroidery::stitch_preview::{INLINE_PLAN_MS, LIVE_PLAN_MS, PROGRESS_REPAINT_SECS, QUIET_SECS, STALE_NOTICE_SECS};

/// Milliseconds in a second: the time a change took to show is said in milliseconds.
const MS_PER_SEC: f64 = 1000.0;

/// Where the window's jobs run: its thread or in line ([`Driver`]), or the tests' own.
pub(crate) trait Runner {
    /// Send `job`, or queue it.
    fn submit(&mut self, job: Job);
    /// The steps finished since the last call, oldest first.
    fn poll(&mut self) -> Vec<Done>;
    /// Whether jobs run in the frame (no thread).
    fn in_line(&self) -> bool {
        false
    }
    /// Whether it needs a pipeline (its thread ended, taking its own).
    fn needs_pipeline(&self) -> bool {
        false
    }
    /// Give it a pipeline.
    fn give_pipeline(&mut self, _pipeline: DigitizePipeline) {}
}

impl Runner for Driver {
    fn submit(&mut self, job: Job) {
        Driver::submit(self, job);
    }

    fn poll(&mut self) -> Vec<Done> {
        Driver::poll(self)
    }

    fn in_line(&self) -> bool {
        Driver::in_line(self)
    }

    fn needs_pipeline(&self) -> bool {
        Driver::needs_pipeline(self)
    }

    fn give_pipeline(&mut self, pipeline: DigitizePipeline) {
        Driver::give_pipeline(self, pipeline);
    }
}

/// The request settings `params` (with the window's palette in them) make for image `image`, as the
/// window asks the engine for a run.
fn request_of(session: &Session, image: NodeId, params: &Value) -> vectorcraft_engine::Result<DigitizeRequest> {
    let palette = params.get(super::PALETTE).and_then(Value::as_str);
    session.digitize_window_request(image, &super::settings_of(params), palette)
}

/// How a result shown stands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Freshness {
    /// It is of the settings as they are.
    Fresh,
    /// A newer one is on its way: the window says so once `notice`, with the run's `progress`
    /// (0 to 1) when it reports one.
    Pending { notice: bool, progress: Option<f64> },
    /// The settings as they are can't be digitized: the reason, the last good result kept.
    Failed(String),
}

/// A change not run yet: its settings, the first stage it makes again (`None`: none, every stage
/// found kept), and since when it waits.
struct Waiting {
    params: Value,
    from: Option<DigitizeStage>,
    since: f64,
}

/// The run under way: which, of what settings and request, its progress, and how many of its
/// stages it has handed over.
struct Running {
    ticket: u64,
    /// The first stage the change it runs makes again.
    from: Option<DigitizeStage>,
    request: Arc<DigitizeRequest>,
    progress: Arc<Progress>,
    handed: usize,
}

impl Running {
    /// How far it has got, 0 to 1: each stage an equal share once handed over, and the stitches'
    /// plan, the last, filled by the planner's own units as it reports them (the window's thread
    /// clears what the stages reported before it begins, `worker::run`).
    fn fraction(&self) -> f64 {
        let stages = DigitizeStage::ALL.len();
        let planning = self.handed + 1 == stages;
        let within = if planning { self.progress.fraction().unwrap_or(0.0).clamp(0.0, 1.0) } else { 0.0 };
        (self.handed.min(stages) as f64 + within) / stages as f64
    }
}

/// What a run handed over that the window shows: the outputs so far, of which request.
#[derive(Clone)]
pub(crate) struct Shown {
    pub(crate) request: Arc<DigitizeRequest>,
    pub(crate) outputs: DigitizeOutputs,
    /// Whether every stage is done.
    pub(crate) whole: bool,
    /// The Colours view's raster painted on the thread, the latest.
    pub(crate) painted: Option<Arc<Painted>>,
    /// The check on the latest plan's objects, made on the thread.
    pub(crate) checked: Option<Checked>,
}

/// When the window digitizes again, and what it shows meanwhile.
#[derive(Default)]
pub(crate) struct Schedule {
    next_ticket: u64,
    /// The settings run last (or running): a change is measured against them.
    asked: Option<Value>,
    /// The request run last (or running), refused ones passed by: what the stages kept are of, so
    /// what a change makes again is measured against it.
    asked_request: Option<Arc<DigitizeRequest>>,
    waiting: Option<Waiting>,
    running: Option<Running>,
    /// Each stage's time when it was last made (milliseconds), by its place in
    /// [`DigitizeStage::ALL`].
    stage_ms: [Option<f64>; DigitizeStage::ALL.len()],
    /// When the result shown went out of date (the first change since it was fresh).
    changed_at: Option<f64>,
    failure: Option<String>,
    /// How long the last change took to show whole (milliseconds), and which stages it made.
    pub(crate) updated: Option<(f64, Vec<DigitizeStage>)>,
    /// The stages the run under way made so far.
    made: Vec<DigitizeStage>,
    pub(crate) shown: Option<Shown>,
    /// The document the last run asked for was made of: another one active (an edit since) is a
    /// change from the image on, so that Create never makes objects of an earlier document.
    made_of: Option<Arc<vectorcraft_engine::doc::Document>>,
}

fn place(stage: DigitizeStage) -> usize {
    DigitizeStage::ALL.iter().position(|&s| s == stage).unwrap_or(0)
}

impl Schedule {
    /// The time the stages from `from` on took when they were last made, `None` when one was never
    /// timed (it counts as heavy); nothing when no stage is made again.
    fn predicted(&self, from: Option<DigitizeStage>) -> Option<f64> {
        let Some(from) = from else { return Some(0.0) };
        self.stage_ms.get(place(from)..)?.iter().copied().sum()
    }

    /// The first stage a run of settings `params` makes again, as the engine says it by its keys:
    /// the first that reads something different of the request run last (`None`: none, every
    /// stage found kept). Every stage before a first run, or when the settings are refused (the run
    /// then says why).
    fn expected_from(&self, session: &Session, image: NodeId, params: &Value) -> Option<DigitizeStage> {
        let (Some(last), Ok(new)) = (&self.asked_request, request_of(session, image, params)) else { return Some(DigitizeStage::Image) };
        last.first_stage_changed(&new)
    }

    /// Whether settings `params` are what the result shown, or the run under way, is of.
    pub(crate) fn asked_for(&self, params: &Value) -> bool {
        self.asked.as_ref() == Some(params)
    }

    /// One frame: the settings now are `params` (with the window's palette in them); the runner's
    /// finished steps taken in; a change queued, and run as the rules say, its request made from
    /// `session` for image `image`. `now` in seconds.
    pub(crate) fn frame(&mut self, session: &Session, image: NodeId, params: &Value, now: f64, runner: &mut dyn Runner) {
        let base = self.waiting.as_ref().map(|w| &w.params).or(self.asked.as_ref());
        let edited = match (&self.made_of, session.doc()) {
            (Some(of), Ok(st)) => !Arc::ptr_eq(of, &st.doc) && self.waiting.is_none(),
            _ => false,
        };
        if base.is_none_or(|old| old != params) || edited {
            // Measured against the request run last, which the stages kept are of.
            let from = self.expected_from(session, image, params);
            self.waiting = Some(Waiting { params: params.clone(), from, since: now });
            self.changed_at.get_or_insert(now);
        }
        if runner.needs_pipeline() {
            runner.give_pipeline(session.digitize_pipeline());
        }
        // What finished is taken in first, so a change waiting for the run under way runs in the
        // frame that run ends.
        for done in runner.poll() {
            self.take(done, now);
        }
        if let Some(w) = &self.waiting {
            let limit = if runner.in_line() { INLINE_PLAN_MS } else { LIVE_PLAN_MS };
            let first = self.asked.is_none();
            let light = self.predicted(w.from).is_some_and(|ms| ms <= limit);
            let rested = now - w.since >= QUIET_SECS;
            if first || (light && self.running.is_none()) || (!light && rested) {
                self.submit(session, image, runner);
            }
        }
    }

    /// Run the change waiting: its request made (a refusal said, nothing run), the run under way
    /// cancelled.
    fn submit(&mut self, session: &Session, image: NodeId, runner: &mut dyn Runner) {
        let Some(w) = self.waiting.take() else { return };
        self.asked = Some(w.params.clone());
        self.made_of = session.doc().ok().map(|st| st.doc.clone());
        if let Some(r) = self.running.take() {
            r.progress.cancel();
        }
        match request_of(session, image, &w.params) {
            Ok(request) => {
                self.next_ticket += 1;
                let request = Arc::new(request);
                let progress = Progress::new();
                runner.submit(Job::new(self.next_ticket, request.clone(), progress.clone()));
                self.asked_request = Some(request.clone());
                self.running = Some(Running { ticket: self.next_ticket, from: w.from, request, progress, handed: 0 });
                self.made.clear();
                self.failure = None;
            }
            Err(e) => self.failure = Some(crate::i18n::msg(&e.to_string())),
        }
    }

    /// Take in a finished step of the run under way (any other run's is passed by).
    fn take(&mut self, done: Done, now: f64) {
        let Some(r) = self.running.as_ref().filter(|r| r.ticket == done.ticket) else { return };
        let request = r.request.clone();
        match done.outcome {
            Outcome::Stage { stage, outputs, made, painted, checked } => {
                if let Some(r) = self.running.as_mut() {
                    r.handed += 1;
                }
                if made {
                    self.stage_ms[place(stage)] = outputs.ms(stage).or(Some(0.0));
                    self.made.push(stage);
                }
                let painted = painted.or_else(|| self.shown.as_ref().and_then(|s| s.painted.clone()));
                let checked = checked.or_else(|| self.shown.as_ref().and_then(|s| s.checked.clone()));
                self.shown = Some(Shown { request, outputs, whole: false, painted, checked });
            }
            Outcome::Finished(outputs) => {
                let painted = self.shown.as_ref().and_then(|s| s.painted.clone());
                let checked = self.shown.as_ref().and_then(|s| s.checked.clone());
                self.shown = Some(Shown { request, outputs, whole: true, painted, checked });
                self.running = None;
                if self.waiting.is_none() {
                    let made = std::mem::take(&mut self.made);
                    if let Some(t) = self.changed_at.take() {
                        self.updated = Some(((now - t) * MS_PER_SEC, made));
                    }
                }
            }
            Outcome::Failed(reason) => {
                self.failure = Some(reason);
                self.running = None;
                self.changed_at = None;
            }
            Outcome::Cancelled => self.running = None,
        }
    }

    /// How the result shown stands at `now` (seconds).
    pub(crate) fn freshness(&self, now: f64) -> Freshness {
        let busy = self.waiting.is_some() || self.running.is_some();
        match (&self.failure, busy) {
            (Some(reason), false) => Freshness::Failed(reason.clone()),
            (_, true) => Freshness::Pending {
                notice: self.changed_at.is_some_and(|t| now - t >= STALE_NOTICE_SECS),
                progress: self.running.as_ref().map(Running::fraction),
            },
            (None, false) => Freshness::Fresh,
        }
    }

    /// The first stage a change waiting or under way makes again: the steps from its on are
    /// recomputed.
    pub(crate) fn pending_from(&self) -> Option<DigitizeStage> {
        let waiting = self.waiting.as_ref().and_then(|w| w.from);
        let running = self.running.as_ref().and_then(|r| r.from);
        [waiting, running].into_iter().flatten().min_by_key(|&s| place(s))
    }

    /// Whether it is waiting for a run or running one (the window is drawn again meanwhile).
    pub(crate) fn busy(&self) -> bool {
        self.waiting.is_some() || self.running.is_some()
    }

    /// The outputs shown when they are whole, of settings `params`, and those settings weren't
    /// refused (what Create applies): never the last good result kept for settings it isn't of.
    pub(crate) fn current(&self, params: &Value) -> Option<&Shown> {
        self.shown.as_ref().filter(|s| s.whole && !self.busy() && self.failure.is_none() && self.asked_for(params))
    }

    /// Stop the run under way and forget the change waiting (the window closes).
    pub(crate) fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.progress.cancel();
        }
        self.waiting = None;
    }
}

#[cfg(test)]
impl Schedule {
    /// Every stage timed at `ms` milliseconds, as a test's light or heavy runs.
    pub(crate) fn time_stages(&mut self, ms: f64) {
        self.stage_ms = [Some(ms); DigitizeStage::ALL.len()];
    }
}

#[cfg(test)]
mod tests;
