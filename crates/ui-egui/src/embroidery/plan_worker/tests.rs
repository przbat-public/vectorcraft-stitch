//! The planning thread: a job's plan and replies are the planner's and the commands' own, only
//! the newest of a burst is planned, a cancel or a panic ends a job and the thread goes on, and
//! dropping the worker ends the thread. In line (no thread), a job runs at once in one go.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_testkit::embroidery as fx;

use super::*;

/// How long a test waits for the thread: far longer than these plans take.
const WAIT: Duration = Duration::from_secs(30);

/// A document of three squares in two threads, open in a session.
fn session() -> Session {
    let mut s = Session::new();
    let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"));
    fx::blank(&mut run);
    fx::three_threads(&mut run, true);
    s
}

fn stats() -> Query {
    Query { cmd: "embroidery.stats", params: json!({}) }
}

fn job(s: &mut Session, ticket: u64, queries: Vec<Query>) -> Job {
    Job::new(ticket, s.embroidery_planner().unwrap(), EmbroideryOptions::default(), queries, Progress::new())
}

/// The steps the worker hands over until `n` jobs have finished.
fn finished(w: &mut PlanWorker, n: usize) -> Vec<Done> {
    let t0 = Instant::now();
    let mut got = vec![];
    while got.iter().filter(|d: &&Done| d.last).count() < n {
        assert!(t0.elapsed() < WAIT, "the thread answered {got:?}");
        got.extend(w.poll());
        assert!(!w.gone());
        std::thread::yield_now();
    }
    got
}

#[test]
fn a_job_hands_over_the_plan_then_the_commands_replies() {
    let mut s = session();
    let Some(mut w) = PlanWorker::start(&egui::Context::default()) else { return };
    let check = Query { cmd: "embroidery.check", params: json!({"format": "dst"}) };
    assert!(w.submit(job(&mut s, 1, vec![stats(), check.clone()])).is_none());
    let got = finished(&mut w, 1);
    let [
        Done { ticket: 1, outcome: Outcome::Planned(planned), last: false, .. },
        Done { ticket: 1, outcome: Outcome::Replies(replies), last: true, .. },
    ] = &got[..]
    else {
        panic!("{got:?}")
    };
    assert!(planned.built_scene().is_some(), "the scene is built on the thread");
    assert_eq!(*planned.plan, *s.embroidery_plan(&EmbroideryOptions::default()).unwrap(), "the planner's plan");
    assert_eq!(replies.len(), 2);
    for (q, reply) in replies {
        assert_eq!(reply.as_ref().unwrap(), &s.execute(q.cmd, &q.params).unwrap(), "{}: the command's own reply", q.cmd);
    }
    assert_eq!(replies[1].0, check);
    // A reply's own error stays with it.
    let bad = Query { cmd: "embroidery.check", params: json!({"format": "svg"}) };
    assert!(w.submit(job(&mut s, 2, vec![bad, stats()])).is_none());
    let got = finished(&mut w, 1);
    let Some(Done { outcome: Outcome::Replies(replies), .. }) = got.last() else { panic!("{got:?}") };
    assert!(replies[0].1.is_err() && replies[1].1.is_ok(), "{replies:?}");
    // With no query the plan is the job's last step.
    assert!(w.submit(job(&mut s, 3, vec![])).is_none());
    let got = finished(&mut w, 1);
    assert!(matches!(&got[..], [Done { ticket: 3, outcome: Outcome::Planned(_), last: true, .. }]), "{got:?}");
}

#[test]
fn only_the_newest_of_a_burst_is_planned() {
    let mut s = session();
    let Some(mut w) = PlanWorker::start(&egui::Context::default()) else { return };
    // The first job, once started, waits on the thread until the others are sent.
    let (started, has_started) = mpsc::channel::<()>();
    let (go, wait) = mpsc::channel::<()>();
    let wait = std::sync::Mutex::new(wait);
    let mut first = job(&mut s, 1, vec![]);
    first.hook = Some(Box::new(move |_| {
        started.send(()).unwrap();
        let _ = wait.lock().unwrap().recv_timeout(WAIT);
    }));
    assert!(w.submit(first).is_none());
    has_started.recv_timeout(WAIT).unwrap();
    for ticket in 2..=4 {
        assert!(w.submit(job(&mut s, ticket, vec![])).is_none());
    }
    go.send(()).unwrap();
    let got = finished(&mut w, 4);
    let tickets: Vec<(u64, bool)> = got.iter().map(|d| (d.ticket, matches!(d.outcome, Outcome::Planned(_)))).collect();
    assert_eq!(tickets, [(1, true), (2, false), (3, false), (4, true)], "{got:?}");
    assert!(got[1..3].iter().all(|d| matches!(d.outcome, Outcome::Cancelled)));
}

#[test]
fn a_cancel_or_a_panic_ends_the_job_and_the_thread_goes_on() {
    let mut s = session();
    let Some(mut w) = PlanWorker::start(&egui::Context::default()) else { return };
    // Cancelled once it has started: the plan stops at its next check.
    let mut cancelled = job(&mut s, 1, vec![stats()]);
    cancelled.hook = Some(Box::new(|job| job.progress.cancel()));
    assert!(w.submit(cancelled).is_none());
    let got = finished(&mut w, 1);
    assert!(matches!(&got[..], [Done { ticket: 1, outcome: Outcome::Cancelled, last: true, .. }]), "{got:?}");
    // Cancelled before it started.
    let ahead = job(&mut s, 2, vec![]);
    ahead.progress.cancel();
    assert!(w.submit(ahead).is_none());
    let got = finished(&mut w, 1);
    assert!(matches!(&got[..], [Done { ticket: 2, outcome: Outcome::Cancelled, .. }]), "{got:?}");
    let mut panics = job(&mut s, 3, vec![stats()]);
    panics.hook = Some(Box::new(|_| panic!("a planning bug")));
    assert!(w.submit(panics).is_none());
    let got = finished(&mut w, 1);
    let [Done { ticket: 3, outcome: Outcome::Failed(why), last: true, .. }] = &got[..] else { panic!("{got:?}") };
    assert_eq!(why, "internal error: a planning bug (please report this bug)");
    assert!(w.submit(job(&mut s, 4, vec![])).is_none());
    let got = finished(&mut w, 1);
    assert!(matches!(&got[..], [Done { ticket: 4, outcome: Outcome::Planned(_), .. }]), "the thread goes on: {got:?}");
}

#[test]
fn a_design_too_large_fails_with_the_reason() {
    let mut s = Session::new();
    let mut run = |c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"));
    fx::blank(&mut run);
    // Three metres a side: more stitches than one shape may have.
    fx::rect(&mut run, fx::RED, 0.0, 0.0, 8500.0, 8500.0);
    let Some(mut w) = PlanWorker::start(&egui::Context::default()) else { return };
    let expected = s.embroidery_plan(&EmbroideryOptions::default()).unwrap_err().to_string();
    assert!(w.submit(job(&mut s, 1, vec![stats()])).is_none());
    let got = finished(&mut w, 1);
    let [Done { outcome: Outcome::Failed(why), last: true, .. }] = &got[..] else { panic!("{got:?}") };
    assert_eq!(*why, expected);
}

#[test]
fn dropping_the_worker_ends_the_thread() {
    let mut s = session();
    let Some(mut w) = PlanWorker::start(&egui::Context::default()) else { return };
    assert!(w.submit(job(&mut s, 1, vec![stats()])).is_none());
    let thread = w.thread.take().unwrap();
    drop(w);
    let t0 = Instant::now();
    while !thread.is_finished() {
        assert!(t0.elapsed() < WAIT, "the thread is still running");
        std::thread::yield_now();
    }
}

#[test]
fn in_line_a_job_runs_at_once_in_one_go() {
    let mut s = session();
    let mut d = Driver::new(&egui::Context::default());
    assert!(!d.threaded(), "unit tests plan in line");
    assert!(d.poll().is_empty());
    d.submit(job(&mut s, 1, vec![stats()]));
    let got = d.poll();
    let [
        Done { ticket: 1, outcome: Outcome::Planned(planned), last: false, .. },
        Done { ticket: 1, outcome: Outcome::Replies(replies), last: true, .. },
    ] = &got[..]
    else {
        panic!("{got:?}")
    };
    assert_eq!(*planned.plan, *s.embroidery_plan(&EmbroideryOptions::default()).unwrap());
    assert_eq!(replies[0].1.as_ref().unwrap(), &s.execute("embroidery.stats", &json!({})).unwrap());
    assert!(d.poll().is_empty(), "each step handed over once");
    let cancelled = job(&mut s, 2, vec![stats()]);
    cancelled.progress.cancel();
    d.submit(cancelled);
    assert!(matches!(&d.poll()[..], [Done { ticket: 2, outcome: Outcome::Cancelled, last: true, .. }]));
}

/// A worker whose thread has ended: the jobs it had are answered as cancelled, and from then on
/// jobs run in line.
#[test]
fn once_the_thread_is_gone_jobs_run_in_line() {
    let mut s = session();
    let ended = || {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (done_tx, rx) = mpsc::channel::<Done>();
        drop((jobs, done_tx));
        PlanWorker { tx, rx, pending: vec![(7, Progress::new())], gone: false, thread: None }
    };
    let mut d = Driver::Thread(ended());
    d.submit(job(&mut s, 8, vec![]));
    assert!(!d.threaded());
    let got = d.poll();
    assert!(
        matches!(&got[..], [Done { ticket: 7, outcome: Outcome::Cancelled, .. }, Done { ticket: 8, outcome: Outcome::Planned(_), last: true, .. }]),
        "{got:?}"
    );
    let mut d = Driver::Thread(ended());
    assert!(matches!(&d.poll()[..], [Done { ticket: 7, outcome: Outcome::Cancelled, .. }]));
    assert!(!d.threaded());
}
