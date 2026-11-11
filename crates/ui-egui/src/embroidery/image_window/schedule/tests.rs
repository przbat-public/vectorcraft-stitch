//! When the image window digitizes again: with runs held until a test lets them finish, and the
//! stages' times pinned, so that what is light and what is heavy doesn't depend on the machine.

use std::collections::VecDeque;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_engine::cmd::embroidery::{DigitizePipeline, DigitizeStage};
use vectorcraft_engine::doc::NodeId;

use super::super::worker::tests::picture_session;
use super::super::worker::{self, Done, Job};
use super::*;

/// A runner that holds every job until [`Held::finish`], as a picture slow to digitize: a newer job
/// replaces the one held (answered as cancelled), as the window's thread does.
struct Held {
    pipeline: DigitizePipeline,
    held: Option<Job>,
    done: VecDeque<Done>,
    submitted: usize,
}

impl Held {
    fn new(s: &Session) -> Self {
        Self { pipeline: s.digitize_pipeline(), held: None, done: VecDeque::new(), submitted: 0 }
    }

    /// Run the job held, its steps handed over at the next poll.
    fn finish(&mut self) {
        if let Some(job) = self.held.take() {
            let mut steps = Vec::new();
            worker::run(&mut self.pipeline, &job, &mut |d| steps.push(d));
            self.done.extend(steps);
        }
    }

    /// Run the job held but hand over only its first `n` steps now (a run part way through).
    fn finish_first(&mut self, n: usize) -> Vec<Done> {
        let Some(job) = self.held.take() else { return vec![] };
        let mut steps = Vec::new();
        worker::run(&mut self.pipeline, &job, &mut |d| steps.push(d));
        let rest = steps.split_off(n.min(steps.len()));
        self.done.extend(steps);
        rest
    }
}

impl Runner for Held {
    fn submit(&mut self, job: Job) {
        self.submitted += 1;
        if let Some(old) = self.held.replace(job) {
            self.done.push_back(Done { ticket: old.ticket, outcome: worker::Outcome::Cancelled, last: true });
        }
    }

    fn poll(&mut self) -> Vec<Done> {
        self.done.drain(..).collect()
    }
}

/// The settings the window holds: `colors` colours in Janome's palette.
fn params(colors: u64) -> Value {
    json!({"colors": colors, "palette": "jef"})
}

/// A moment past a time the rules wait for (seconds), so that a test's sums of times land past it
/// whatever their rounding.
const PAST: f64 = 0.001;

/// Every stage timed at `ms` milliseconds.
fn timed(s: &mut Schedule, ms: f64) {
    s.stage_ms = [Some(ms); 8];
}

fn setup() -> (Session, NodeId, Schedule) {
    let (s, id) = picture_session();
    (s, id, Schedule::default())
}

/// What a setting change recomputes from: the image's settings from the image, the trace's from
/// the trace, the rest (a colour's choice, the smallest area, the palette) from the colours.
#[test]
fn first_changed_names_the_first_stage_a_setting_reads() {
    let base = json!({"colors": 6, "minArea": 1.0, "palette": "jef"});
    let with = |k: &str, v: Value| {
        let mut p = base.clone();
        p[k] = v;
        p
    };
    assert_eq!(first_changed(&base, &base), None);
    assert_eq!(first_changed(&base, &with("width", json!(30))), Some(DigitizeStage::Image));
    assert_eq!(first_changed(&base, &with("background", json!("none"))), Some(DigitizeStage::Image));
    assert_eq!(first_changed(&base, &with("colors", json!(5))), Some(DigitizeStage::Trace));
    assert_eq!(first_changed(&base, &with("mode", json!("logo"))), Some(DigitizeStage::Trace));
    assert_eq!(first_changed(&base, &with("minArea", json!(2.0))), Some(DigitizeStage::Colours));
    assert_eq!(first_changed(&base, &with("palette", json!("pec"))), Some(DigitizeStage::Colours));
}

/// The first run starts at once, whatever it costs; its stages are taken in as they come, and the
/// result is fresh once the run finishes.
#[test]
fn the_first_run_starts_at_once() {
    let (s, id, mut sch) = setup();
    let mut held = Held::new(&s);
    sch.frame(&s, id, &params(3), 0.0, &mut held);
    assert_eq!(held.submitted, 1);
    assert!(matches!(sch.freshness(0.0), Freshness::Pending { notice: false, .. }));
    held.finish();
    sch.frame(&s, id, &params(3), 0.05, &mut held);
    assert_eq!(sch.freshness(0.05), Freshness::Fresh);
    assert!(sch.current(&params(3)).is_some_and(|shown| shown.outputs.made().is_some()));
    assert!(sch.updated.as_ref().is_some_and(|(_, stages)| stages == &DigitizeStage::ALL.to_vec()), "every stage made");
}

/// A change whose stages were last made within the live limit runs at once, also while a slider
/// moves; the run under way is let finish first.
#[test]
fn light_changes_follow_the_slider() {
    let (s, id, mut sch) = setup();
    let mut held = Held::new(&s);
    sch.frame(&s, id, &params(3), 0.0, &mut held);
    held.finish();
    sch.frame(&s, id, &params(3), 0.01, &mut held);
    timed(&mut sch, 1.0);
    sch.frame(&s, id, &params(4), 0.02, &mut held);
    assert_eq!(held.submitted, 2, "light: run at once");
    sch.frame(&s, id, &params(5), 0.03, &mut held);
    assert_eq!(held.submitted, 2, "the run under way is let finish");
    held.finish();
    sch.frame(&s, id, &params(5), 0.04, &mut held);
    assert_eq!(held.submitted, 3, "then the newest change runs");
}

/// A change whose stages took longer than the live limit waits until the settings have rested
/// for the quiet time; a newer change cancels the run under way.
#[test]
fn heavy_changes_wait_for_the_rest() {
    let (s, id, mut sch) = setup();
    let mut held = Held::new(&s);
    sch.frame(&s, id, &params(3), 0.0, &mut held);
    held.finish();
    sch.frame(&s, id, &params(3), 0.01, &mut held);
    timed(&mut sch, LIVE_PLAN_MS);
    sch.frame(&s, id, &params(4), 1.0, &mut held);
    assert_eq!(held.submitted, 1, "heavy: waits");
    sch.frame(&s, id, &params(5), 1.05, &mut held);
    sch.frame(&s, id, &params(5), 1.05 + QUIET_SECS * 0.5, &mut held);
    assert_eq!(held.submitted, 1, "still moving");
    sch.frame(&s, id, &params(5), 1.05 + QUIET_SECS + PAST, &mut held);
    assert_eq!(held.submitted, 2, "rested: run");
    sch.frame(&s, id, &params(6), 2.0, &mut held);
    sch.frame(&s, id, &params(6), 2.0 + QUIET_SECS + PAST, &mut held);
    assert_eq!(held.submitted, 3, "a newer change replaced the run under way");
}

/// While a newer result is on its way the last one stays shown; after the notice's time the window
/// says it is recomputing; a run that fails keeps the last good result with the reason.
#[test]
fn a_slow_run_keeps_the_last_result_and_says_so_after_the_notice() {
    let (s, id, mut sch) = setup();
    let mut held = Held::new(&s);
    sch.frame(&s, id, &params(3), 0.0, &mut held);
    held.finish();
    sch.frame(&s, id, &params(3), 0.01, &mut held);
    let first = sch.shown.as_ref().map(|shown| shown.outputs.number(DigitizeStage::Plan));
    timed(&mut sch, 1.0);
    sch.frame(&s, id, &params(4), 1.0, &mut held);
    assert!(matches!(sch.freshness(1.0 + STALE_NOTICE_SECS * 0.5), Freshness::Pending { notice: false, .. }));
    assert!(matches!(sch.freshness(1.0 + STALE_NOTICE_SECS + PAST), Freshness::Pending { notice: true, .. }));
    assert_eq!(sch.shown.as_ref().map(|shown| shown.outputs.number(DigitizeStage::Plan)), first, "the last result stays");
    held.finish();
    sch.frame(&s, id, &params(4), 1.9, &mut held);
    // Refused: more colours than the palette has threads.
    sch.frame(&s, id, &params(500), 2.0, &mut held);
    sch.frame(&s, id, &params(500), 2.0 + QUIET_SECS + PAST, &mut held);
    assert!(matches!(sch.freshness(3.0), Freshness::Failed(_)), "{:?}", sch.freshness(3.0));
    assert!(sch.shown.is_some(), "the last good result kept");
}

/// A replaced run's stages are never taken in: steps of the run held, handed over after a newer
/// run took its place, are passed by.
#[test]
fn a_replaced_runs_stages_are_never_taken_in() {
    let (s, id, mut sch) = setup();
    let mut held = Held::new(&s);
    sch.frame(&s, id, &params(3), 0.0, &mut held);
    held.finish();
    sch.frame(&s, id, &params(3), 0.01, &mut held);
    let shown = sch.shown.as_ref().and_then(|shown| shown.outputs.number(DigitizeStage::Trace));
    timed(&mut sch, 1.0);
    sch.frame(&s, id, &params(4), 1.0, &mut held);
    // The run of four colours gets two steps out, then a change replaces it.
    let late = held.finish_first(2);
    sch.frame(&s, id, &params(4), 1.01, &mut held);
    let four = sch.shown.as_ref().and_then(|shown| shown.outputs.number(DigitizeStage::Trace));
    assert_ne!(four, shown, "the run under way's stages are taken in");
    // A heavy change after the rest replaces it; its last steps come late.
    timed(&mut sch, LIVE_PLAN_MS);
    sch.frame(&s, id, &params(5), 1.02, &mut held);
    sch.frame(&s, id, &params(5), 1.02 + QUIET_SECS + PAST, &mut held);
    assert_eq!(held.submitted, 3);
    held.done.extend(late);
    sch.frame(&s, id, &params(5), 1.5, &mut held);
    let now = sch.shown.as_ref().map(|shown| (shown.whole, shown.outputs.number(DigitizeStage::Trace)));
    assert_eq!(now, Some((false, four)), "the replaced run's later steps were passed by");
}
