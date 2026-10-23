//! The scheduler, frame by frame on a set clock, with jobs the tests finish when they say: nothing
//! planned while nothing asks, the first plan at once, light designs planned on every change and
//! heavy ones once the document is quiet, a cancelled job never shown, a failure kept until the
//! document changes, an undo back to the plan shown fresh at once, never another document's plan,
//! the replies equal to the commands', and the notice after its delay.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_testkit::embroidery as fx;

use super::*;
use crate::embroidery::plan_worker::run;

/// A plan the tests count as light, and one as heavy, milliseconds.
const LIGHT_MS: f64 = LIVE_PLAN_MS / 10.0;
const HEAVY_MS: f64 = LIVE_PLAN_MS * 4.0;

/// A little more than the quiet time, seconds (a frame exactly at its end may fall a rounding short).
const AFTER_QUIET: f64 = QUIET_SECS + 0.01;

#[derive(Default)]
struct Jobs {
    waiting: Vec<Job>,
    done: Vec<Done>,
    /// How long each step says it took.
    ms: f64,
}

/// Jobs kept until the test finishes them, as the planning thread would.
#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Jobs>>);

impl Runner for Fake {
    fn submit(&mut self, job: Job) {
        self.0.borrow_mut().waiting.push(job);
    }

    fn poll(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.0.borrow_mut().done)
    }
}

impl Fake {
    /// The tickets of the jobs sent and not finished.
    fn waiting(&self) -> Vec<u64> {
        self.0.borrow().waiting.iter().map(|j| j.ticket).collect()
    }

    /// Run the oldest job sent, its steps saying they took `ms` each → its ticket.
    fn finish(&self, ms: f64) -> u64 {
        let job = self.0.borrow_mut().waiting.remove(0);
        let mut done = vec![];
        run(&job, &mut |d| done.push(Done { ms, ..d }));
        self.0.borrow_mut().done.extend(done);
        job.ticket
    }

    fn finish_all(&self) {
        let ms = self.0.borrow().ms;
        while !self.0.borrow().waiting.is_empty() {
            self.finish(ms);
        }
    }
}

fn preview() -> (StitchPreview, Fake) {
    let fake = Fake::default();
    (StitchPreview::with_runner(Box::new(fake.clone())), fake)
}

fn exec(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A session with a document of three squares → its uid.
fn document(s: &mut Session) -> u64 {
    let mut run = |c: &str, p: Value| exec(s, c, p);
    fx::blank(&mut run);
    fx::three_threads(&mut run, true);
    s.active().unwrap().uid
}

/// An edit: everything moves a little (a new snapshot).
fn edit(s: &mut Session) {
    exec(s, "select.all", json!({}));
    exec(s, "object.move", json!({"dx": 1, "dy": 0}));
}

/// One frame at `now`: what finished is taken in, the canvas asks for stitches when `ask`, and the
/// frame's end schedules, as the app does → the repaint asked for.
fn frame(p: &mut StitchPreview, s: &mut Session, now: f64, busy: bool, ask: bool) -> Option<f64> {
    p.take_in();
    if ask {
        p.want_plan();
    }
    let wait = p.schedule_at(s, now, busy);
    p.take_in();
    wait
}

fn generation(p: &StitchPreview, uid: u64) -> Option<u64> {
    p.shown(uid).map(|sh| sh.planned.generation)
}

#[test]
fn nothing_is_planned_while_nothing_asks() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    for k in 0..3 {
        assert_eq!(frame(&mut p, &mut s, f64::from(k), false, false), None);
    }
    assert!(fake.waiting().is_empty());
    assert_eq!(p.freshness(uid, 3.0), Freshness::Off);
    assert_eq!(p.inspect(uid, 3.0)["state"], "off");
    // Asked, then not: the job is cancelled and the plan shown is kept for the next time.
    frame(&mut p, &mut s, 4.0, false, true);
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 4.1, false, true);
    edit(&mut s);
    frame(&mut p, &mut s, 4.2, false, true);
    let progress = p.in_flight.as_ref().unwrap().progress.clone();
    frame(&mut p, &mut s, 4.3, false, false);
    assert!(progress.cancelled() && p.in_flight.is_none());
    assert!(p.shown(uid).is_some());
}

#[test]
fn the_first_plan_starts_at_once_and_a_fresh_preview_rests() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    assert_eq!(fake.waiting(), [1], "at once");
    assert_eq!(p.freshness(uid, 0.0), Freshness::Pending { progress: None, notice: false });
    assert!(p.busy(uid, 0.0));
    fake.finish(LIGHT_MS);
    assert_eq!(frame(&mut p, &mut s, 0.01, false, true), None);
    assert_eq!(p.freshness(uid, 0.01), Freshness::Fresh);
    assert!(!p.busy(uid, 0.01));
    let shown = p.shown(uid).unwrap();
    assert_eq!(*shown.planned.plan, *s.embroidery_plan(&EmbroideryOptions::default()).unwrap(), "the document's plan");
    assert!(shown.planned.built_scene().is_some());
    // Idle and fresh: nothing sent, no repaint asked for.
    for k in 1..4 {
        assert_eq!(frame(&mut p, &mut s, f64::from(k), false, true), None);
    }
    assert!(fake.waiting().is_empty());
    let inspect = p.inspect(uid, 4.0);
    assert_eq!((&inspect["state"], &inspect["planning"], &inspect["planMs"]), (&json!("fresh"), &json!(false), &json!(LIGHT_MS)));
    assert_eq!(inspect["generation"], json!(generation(&p, uid)));
}

#[test]
fn a_light_design_is_planned_on_every_change_also_during_a_drag() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.01, false, true);
    let first = generation(&p, uid);
    edit(&mut s);
    frame(&mut p, &mut s, 0.02, true, true);
    assert_eq!(fake.waiting(), [2], "during a drag, at once");
    let under_way = p.in_flight.as_ref().unwrap().progress.clone();
    edit(&mut s);
    frame(&mut p, &mut s, 0.03, true, true);
    assert_eq!(fake.waiting(), [2], "the plan under way is let finish");
    assert!(!under_way.cancelled());
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.04, true, true);
    assert_ne!(generation(&p, uid), first, "shown, still stale");
    assert!(matches!(p.freshness(uid, 0.04), Freshness::Stale { .. }));
    assert_eq!(fake.waiting(), [3], "the newest state next");
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.05, true, true);
    assert_eq!(p.freshness(uid, 0.05), Freshness::Fresh);
}

#[test]
fn a_heavy_design_waits_for_the_document_to_stay_unchanged() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 0.5, false, true);
    let first = generation(&p, uid);
    // Changes at 1, 1.05 and 1.09 s: planned only once 1.09 s is a quiet time behind.
    for t in [1.0, 1.05, 1.09] {
        edit(&mut s);
        let wait = frame(&mut p, &mut s, t, false, true).unwrap();
        assert!((wait - QUIET_SECS).abs() < 1e-9, "{t}: draw again when it will have passed, {wait}");
    }
    let wait = frame(&mut p, &mut s, 1.15, false, true).unwrap();
    assert!((wait - (1.09 + QUIET_SECS - 1.15)).abs() < 1e-9, "{wait}");
    assert!(fake.waiting().is_empty());
    frame(&mut p, &mut s, 1.09 + AFTER_QUIET, false, true);
    assert_eq!(fake.waiting(), [2]);
    // A new change cancels the plan under way; nothing is planned during a drag.
    let under_way = p.in_flight.as_ref().unwrap().progress.clone();
    edit(&mut s);
    frame(&mut p, &mut s, 1.3, true, true);
    assert!(under_way.cancelled());
    fake.finish(HEAVY_MS);
    for t in [1.4, 2.0, 5.0] {
        frame(&mut p, &mut s, t, true, true);
    }
    assert!(fake.waiting().is_empty(), "nothing during the drag");
    assert_eq!(generation(&p, uid), first, "the cancelled plan was never shown");
    // The drag ends: the document has long been quiet.
    frame(&mut p, &mut s, 5.1, false, true);
    assert_eq!(fake.waiting(), [3]);
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 5.2, false, true);
    assert_eq!(p.freshness(uid, 5.2), Freshness::Fresh);
}

/// A plan that finished just before its job was cancelled is dropped, and so is a step of a job
/// that isn't the one under way; an answer "cancelled" changes nothing.
#[test]
fn a_cancelled_job_is_never_shown() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    // It finishes, and before its result is taken in, an edit cancels it.
    fake.finish(HEAVY_MS);
    edit(&mut s);
    p.want_plan();
    p.schedule_at(&mut s, 0.01, false);
    assert_eq!(fake.waiting(), [2], "a first plan starts again at once");
    p.take_in();
    assert!(p.shown(uid).is_none(), "the cancelled job's plan is dropped");
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 0.02, false, true);
    assert_eq!(p.freshness(uid, 0.02), Freshness::Fresh);
    let shown = generation(&p, uid);
    // An answer "cancelled" for the job under way ends it, and nothing else changes.
    edit(&mut s);
    frame(&mut p, &mut s, 1.0, false, true);
    frame(&mut p, &mut s, 1.0 + AFTER_QUIET, false, true);
    let ticket = p.in_flight.as_ref().unwrap().ticket;
    fake.0.borrow_mut().waiting.clear();
    fake.0.borrow_mut().done.push(Done { ticket, outcome: Outcome::Cancelled, ms: 0.0, last: true });
    p.take_in();
    assert!(p.in_flight.is_none());
    assert_eq!(generation(&p, uid), shown);
    assert!(p.docs[&uid].failed.is_none());
    // A stray step of another ticket is dropped.
    fake.0.borrow_mut().done.push(Done { ticket: 99, outcome: Outcome::Failed("stray".into()), ms: 0.0, last: true });
    p.take_in();
    assert!(p.docs[&uid].failed.is_none());
}

#[test]
fn a_design_that_fails_is_planned_again_only_once_it_changes() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.1, false, true);
    let good = generation(&p, uid);
    // Three metres a side: more stitches than one shape may have.
    let mut run = |c: &str, v: Value| exec(&mut s, c, v);
    fx::rect(&mut run, fx::RED, 0.0, 0.0, 8500.0, 8500.0);
    frame(&mut p, &mut s, 1.0, false, true);
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 1.1, false, true);
    let Freshness::Failed { error } = p.freshness(uid, 1.1) else { panic!("{:?}", p.freshness(uid, 1.1)) };
    assert_eq!(error, s.embroidery_plan(&EmbroideryOptions::default()).unwrap_err().to_string());
    assert_eq!(generation(&p, uid), good, "the last good stitches stay shown");
    assert_eq!(p.inspect(uid, 1.1)["error"], json!(error));
    for t in [2.0, 3.0, 4.0] {
        assert_eq!(frame(&mut p, &mut s, t, false, true), None);
    }
    assert!(fake.waiting().is_empty(), "not planned again while unchanged");
    edit(&mut s);
    frame(&mut p, &mut s, 5.0, false, true);
    frame(&mut p, &mut s, 5.0 + AFTER_QUIET, false, true);
    assert_eq!(fake.waiting().len(), 1, "an edit plans again");
}

#[test]
fn an_undo_back_to_the_plan_shown_is_fresh_at_once() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 0.1, false, true);
    let shown = generation(&p, uid);
    let steps = s.doc().unwrap().history.undo.len();
    edit(&mut s);
    frame(&mut p, &mut s, 1.0, false, true);
    frame(&mut p, &mut s, 1.0 + AFTER_QUIET, false, true);
    let under_way = p.in_flight.as_ref().unwrap().progress.clone();
    while s.doc().unwrap().history.undo.len() > steps {
        exec(&mut s, "edit.undo", json!({}));
    }
    frame(&mut p, &mut s, 1.2, false, true);
    assert!(under_way.cancelled(), "the plan of the state undone isn't wanted");
    assert_eq!(p.freshness(uid, 1.2), Freshness::Fresh);
    assert_eq!(generation(&p, uid), shown);
}

/// Each document keeps its own plan: switching shows the other's last plan at once, never this
/// one's; closing a document drops its plan and cancels its job.
#[test]
fn each_document_shows_its_own_plan() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let a = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.1, false, true);
    let shown_a = generation(&p, a);
    let b = document(&mut s);
    frame(&mut p, &mut s, 0.2, false, true);
    assert!(p.shown(b).is_none(), "never another document's plan");
    assert!(matches!(p.freshness(b, 0.2), Freshness::Pending { .. }));
    let under_way = p.in_flight.as_ref().unwrap().progress.clone();
    s.set_active(0);
    frame(&mut p, &mut s, 0.3, false, true);
    assert!(under_way.cancelled(), "the other document's job");
    assert_eq!(p.freshness(a, 0.3), Freshness::Fresh);
    assert_eq!(generation(&p, a), shown_a);
    s.set_active(1);
    frame(&mut p, &mut s, 0.4, false, true);
    fake.0.borrow_mut().waiting.retain(|j| !j.progress.cancelled());
    fake.finish(LIGHT_MS);
    frame(&mut p, &mut s, 0.5, false, true);
    assert!(p.shown(b).is_some());
    assert_ne!(generation(&p, b), shown_a);
    // B closes with a job under way: dropped, and the job cancelled.
    edit(&mut s);
    frame(&mut p, &mut s, 0.6, false, true);
    let under_way = p.in_flight.as_ref().unwrap().progress.clone();
    exec(&mut s, "file.close", json!({}));
    frame(&mut p, &mut s, 0.7, false, true);
    assert!(under_way.cancelled());
    assert!(!p.docs.contains_key(&b));
    assert!(p.docs.contains_key(&a));
}

/// The replies are the commands' own, of the plan shown; a reply asked for later (another format
/// for the check) is made from the plan remembered, which isn't planned again.
#[test]
fn the_replies_are_the_commands_and_a_new_one_reuses_the_plan() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    let stats = Query { cmd: "embroidery.stats", params: json!({}) };
    let check = |format: &str| Query { cmd: "embroidery.check", params: json!({"format": format}) };
    let ask = |p: &mut StitchPreview, format: &str| {
        p.want(stats.clone());
        p.want(check(format));
    };
    p.take_in();
    ask(&mut p, "dst");
    p.schedule_at(&mut s, 0.0, false);
    assert_eq!(p.freshness(uid, 0.0), Freshness::Pending { progress: None, notice: false }, "a reply asks for the plan");
    fake.finish(LIGHT_MS);
    p.take_in();
    let shown = p.shown(uid).unwrap();
    for (q, params) in [(stats.clone(), json!({})), (check("dst"), json!({"format": "dst"}))] {
        let r = shown.reply(&q).unwrap();
        let (reply, current) = (r.result, r.current);
        assert!(current);
        assert_eq!(**reply.as_ref().unwrap(), exec(&mut s, q.cmd, params), "{}", q.cmd);
    }
    let plan = shown.planned.plan.clone();
    ask(&mut p, "pes");
    p.schedule_at(&mut s, 0.1, false);
    assert_eq!(fake.waiting().len(), 1, "a reply nobody had");
    fake.finish(HEAVY_MS * 10.0);
    p.take_in();
    let shown = p.shown(uid).unwrap();
    assert!(Arc::ptr_eq(&shown.planned.plan, &plan), "the plan remembered");
    assert_eq!(p.docs[&uid].plan_ms, Some(LIGHT_MS), "a job that only made replies says nothing of the design's weight");
    assert_eq!(**shown.reply(&check("pes")).unwrap().result.as_ref().unwrap(), exec(&mut s, "embroidery.check", json!({"format": "pes"})));
    assert!(shown.reply(&check("dst")).is_some_and(|r| r.current), "the plan's own replies are kept");
    // Asked again as it is: nothing to do; nor for the other format checked on this plan.
    ask(&mut p, "pes");
    p.schedule_at(&mut s, 0.2, false);
    assert!(fake.waiting().is_empty());
    ask(&mut p, "dst");
    p.schedule_at(&mut s, 0.3, false);
    assert!(fake.waiting().is_empty(), "a tab switched back answers at once");
}

/// The preview plans with its options, the last embroidery export's, and the replies are of them.
#[test]
fn the_preview_plans_with_its_options() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    exec(&mut s, "document.export", json!({"format": "dst", "spacing": 0.5}));
    let opts = EmbroideryOptions::from_json(&json!({"spacing": 0.5})).unwrap();
    p.want(Query { cmd: "embroidery.stats", params: json!({}) });
    p.schedule_at(&mut s, 0.0, false);
    fake.finish(LIGHT_MS);
    p.take_in();
    let shown = p.shown(uid).unwrap();
    assert_eq!(shown.key, s.embroidery_plan_key(&opts).unwrap());
    assert_eq!(*shown.planned.plan, *s.embroidery_plan(&opts).unwrap());
    let reply = shown.reply(&Query { cmd: "embroidery.stats", params: json!({}) }).unwrap().result;
    assert_eq!(**reply.as_ref().unwrap(), exec(&mut s, "embroidery.stats", json!({"spacing": 0.5})));
}

#[test]
fn the_notice_shows_once_the_stitches_have_been_out_of_date_a_while() {
    let (mut p, fake) = preview();
    let mut s = Session::new();
    let uid = document(&mut s);
    frame(&mut p, &mut s, 0.0, false, true);
    fake.finish(HEAVY_MS);
    frame(&mut p, &mut s, 0.1, false, true);
    edit(&mut s);
    frame(&mut p, &mut s, 1.0, false, true);
    assert_eq!(p.freshness(uid, 1.1), Freshness::Stale { progress: None, notice: false });
    let wait = frame(&mut p, &mut s, 1.0 + AFTER_QUIET, false, true);
    assert_eq!(fake.waiting().len(), 1);
    assert!(wait.is_some_and(|w| (w - (STALE_NOTICE_SECS - AFTER_QUIET)).abs() < 1e-9), "drawn again when the notice is due: {wait:?}");
    assert!(matches!(p.freshness(uid, 1.0 + STALE_NOTICE_SECS + 0.01), Freshness::Stale { notice: true, .. }));
    let wait = frame(&mut p, &mut s, 1.5, false, true);
    assert_eq!(wait, Some(PROGRESS_REPAINT_SECS), "and while it shows, for the progress");
    fake.finish_all();
    frame(&mut p, &mut s, 1.6, false, true);
    assert_eq!(p.freshness(uid, 1.6), Freshness::Fresh);
}

/// Replies are merged, not replaced: a new reply replaces its query's; the plan's own replies to
/// other queries stay (at most the cap); an earlier plan's stays only while this job is to answer
/// its query.
#[test]
fn replies_are_merged_per_plan() {
    let q = |cmd: &'static str, n: u64| Query { cmd, params: json!({ "n": n }) };
    let reply = |query: Query, generation: u64| Reply { query, generation, result: Ok(Arc::new(json!(generation))) };
    let mut kept = vec![reply(q("a", 0), 1), reply(q("b", 0), 1), reply(q("c", 0), 2)];
    // Plan 2's job answers a: the earlier plan's b, not asked, goes; c, the plan's own, stays.
    merge_replies(&mut kept, vec![reply(q("a", 0), 2)], 2, &[q("a", 0)]);
    let has = |k: &[Reply], query: &Query, g: u64| k.iter().any(|r| r.query == *query && r.generation == g);
    assert!(
        has(&kept, &q("a", 0), 2) && has(&kept, &q("c", 0), 2) && kept.len() == 2,
        "{:?}",
        kept.iter().map(|r| (r.query.cmd, r.generation)).collect::<Vec<_>>()
    );
    // An earlier plan's reply to a query being answered stays until its new one comes.
    let mut kept = vec![reply(q("a", 0), 1), reply(q("b", 0), 1)];
    merge_replies(&mut kept, vec![reply(q("a", 0), 2)], 2, &[q("a", 0), q("b", 0)]);
    assert!(has(&kept, &q("b", 0), 1));
    // The plan's own, the newest kept, at most the cap in all.
    let mut kept = vec![];
    for n in 0..10 {
        merge_replies(&mut kept, vec![reply(q("d", n), 3)], 3, &[q("d", n)]);
    }
    assert_eq!(kept.len(), MAX_KEPT_REPLIES);
    assert!(has(&kept, &q("d", 9), 3) && !has(&kept, &q("d", 0), 3));
}
