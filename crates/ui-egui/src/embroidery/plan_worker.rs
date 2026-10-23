//! The stitch preview's planning thread: it plans a document's design off the interface thread,
//! so a design that takes long to plan never stalls a frame, and hands the plan over in two steps:
//! first the plan with its stitch scene (what the canvas draws), then the read commands' replies
//! the panel and the status bar show ([`Query`]), made by the commands' own code
//! ([`EmbroideryPlanner::query`]). The canvas never waits for the panel's data.
//!
//! One job at a time, the newest: a job sent while another waits replaces it (the one replaced is
//! answered [`Outcome::Cancelled`]). Each job runs inside its own [`Progress`], which reports how
//! far the plan has got and cancels it; a panic is caught and answered as a failure, and the
//! thread goes on serving.
//!
//! [`PlanWorker::spawn`] gives `None` on wasm (no threads), in unit tests (they look at the
//! preview right after the frame that asks) and with `VECTORCRAFT_SYNC_RENDER` set: the preview
//! then plans on the interface thread with the same [`run`].

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use serde_json::Value;
use vectorcraft_engine::cmd::fileio::progress::{self, Progress};
use vectorcraft_engine::{EmbroideryOptions, EmbroideryPlanned, EmbroideryPlanner, EngineError};

/// A read command's reply a job makes after its plan: `cmd` (`embroidery.stats`, `sequence`,
/// `check` or `threads`) with `params`, which hold the job's options so the reply is of its plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub cmd: &'static str,
    pub params: Value,
}

/// What a job's step hands over.
#[derive(Debug)]
pub enum Outcome {
    /// The plan and its stitch scene, built: sent first.
    Planned(EmbroideryPlanned),
    /// The replies to the job's queries, in the order asked, each a reply or its error.
    Replies(Vec<(Query, Result<Value, String>)>),
    /// The design can't be planned as it is (too many stitches), or planning hit a bug.
    Failed(String),
    /// The job was cancelled, or replaced by a newer one before it started.
    Cancelled,
}

/// One step of a job, finished.
#[derive(Debug)]
pub struct Done {
    pub ticket: u64,
    pub outcome: Outcome,
    /// How long the step took, milliseconds.
    pub ms: f64,
    /// The job sends nothing more.
    pub last: bool,
}

/// A design to plan: the planner of one snapshot, the options, the replies wanted after the plan,
/// and the progress that reports on it and cancels it. `ticket` says which job a [`Done`] is of.
pub struct Job {
    pub ticket: u64,
    pub planner: EmbroideryPlanner,
    pub opts: EmbroideryOptions,
    pub queries: Vec<Query>,
    pub progress: Arc<Progress>,
    /// Run inside the job's progress scope before it plans (the tests block, cancel or panic there).
    #[cfg(test)]
    pub(crate) hook: Option<Hook>,
}

/// What a test runs inside a job before it plans.
#[cfg(test)]
pub(crate) type Hook = Box<dyn Fn(&Job) + Send>;

impl Job {
    pub fn new(ticket: u64, planner: EmbroideryPlanner, opts: EmbroideryOptions, queries: Vec<Query>, progress: Arc<Progress>) -> Self {
        Self {
            ticket,
            planner,
            opts,
            queries,
            progress,
            #[cfg(test)]
            hook: None,
        }
    }

    fn done(&self, outcome: Outcome, since: f64, last: bool) -> Done {
        Done { ticket: self.ticket, outcome, ms: crate::now_ms() - since, last }
    }
}

/// The text of a panic caught while planning, as the background jobs give it.
fn internal(msg: &str) -> String {
    format!("internal error: {msg} (please report this bug)")
}

/// Run `job`: plan (or take the plan remembered) and build its scene, hand them to `send`, then
/// make the replies and hand those over. Everything runs inside the job's progress scope with
/// panics caught; a cancel shows as [`Outcome::Cancelled`] and ends the job. The thread and the
/// interface thread both run it.
pub fn run(job: &Job, send: &mut dyn FnMut(Done)) {
    let t0 = crate::now_ms();
    let planned = progress::scope(&job.progress, || {
        vectorcraft_engine::guard::catch_panic(|| {
            #[cfg(test)]
            if let Some(hook) = &job.hook {
                hook(job);
            }
            let planned = job.planner.planned(&job.opts)?;
            // Built here, so the interface thread finds it ready.
            planned.scene();
            Ok::<_, EngineError>(planned)
        })
    });
    let planned = match planned {
        Ok(Ok(p)) => p,
        Ok(Err(EngineError::Cancelled)) => return send(job.done(Outcome::Cancelled, t0, true)),
        Ok(Err(e)) => return send(job.done(Outcome::Failed(e.to_string()), t0, true)),
        Err(msg) => return send(job.done(Outcome::Failed(internal(&msg)), t0, true)),
    };
    let last = job.queries.is_empty();
    send(job.done(Outcome::Planned(planned), t0, last));
    if last {
        return;
    }
    let t1 = crate::now_ms();
    let mut replies = Vec::with_capacity(job.queries.len());
    for q in &job.queries {
        if job.progress.cancelled() {
            return send(job.done(Outcome::Cancelled, t1, true));
        }
        let reply = match progress::scope(&job.progress, || vectorcraft_engine::guard::catch_panic(|| job.planner.query(q.cmd, &q.params))) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(EngineError::Cancelled)) => return send(job.done(Outcome::Cancelled, t1, true)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(msg) => Err(internal(&msg)),
        };
        replies.push((q.clone(), reply));
    }
    send(job.done(Outcome::Replies(replies), t1, true));
}

/// The planning thread. Dropping it cancels the jobs it hasn't answered in full and ends the
/// thread once the one under way stops.
pub struct PlanWorker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
    /// The jobs sent and not finished: cancelled on drop.
    pending: Vec<(u64, Arc<Progress>)>,
    /// The thread, so the tests can see it end.
    #[cfg(test)]
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PlanWorker {
    /// Start the thread, `None` where planning stays on the interface thread: on wasm, in unit
    /// tests, with `VECTORCRAFT_SYNC_RENDER` set, or if the thread can't start.
    pub fn spawn(ctx: &egui::Context) -> Option<Self> {
        if cfg!(target_arch = "wasm32") || cfg!(test) || std::env::var_os("VECTORCRAFT_SYNC_RENDER").is_some() {
            return None;
        }
        Self::start(ctx)
    }

    /// Start the thread.
    fn start(ctx: &egui::Context) -> Option<Self> {
        let (tx, jobs) = channel::<Job>();
        let (done_tx, rx) = channel::<Done>();
        let ctx = ctx.clone();
        let thread = std::thread::Builder::new()
            .name("vectorcraft-plan".into())
            .spawn(move || {
                // The preview is gone (its sender dropped): end the thread.
                while let Ok(mut job) = jobs.recv() {
                    let mut open = true;
                    let mut send = |d: Done| {
                        open &= done_tx.send(d).is_ok();
                        ctx.request_repaint();
                    };
                    // The newest job only: those it replaces are answered as cancelled.
                    while let Ok(newer) = jobs.try_recv() {
                        let since = crate::now_ms();
                        send(std::mem::replace(&mut job, newer).done(Outcome::Cancelled, since, true));
                    }
                    if job.progress.cancelled() {
                        send(job.done(Outcome::Cancelled, crate::now_ms(), true));
                    } else {
                        run(&job, &mut send);
                    }
                    if !open {
                        break;
                    }
                }
            })
            .ok()?;
        #[cfg(not(test))]
        drop(thread);
        Some(Self {
            tx,
            rx,
            pending: Vec::new(),
            #[cfg(test)]
            thread: Some(thread),
        })
    }

    /// Send `job`. False when the thread is gone: the caller plans on the interface thread from
    /// now on.
    #[must_use]
    pub fn submit(&mut self, job: Job) -> bool {
        let (ticket, progress) = (job.ticket, job.progress.clone());
        let sent = self.tx.send(job).is_ok();
        if sent {
            self.pending.push((ticket, progress));
        }
        sent
    }

    /// The steps finished since the last call, oldest first; `None` when the thread is gone (the
    /// jobs it had will never finish).
    pub fn poll(&mut self) -> Option<Vec<Done>> {
        let mut done = vec![];
        loop {
            match self.rx.try_recv() {
                Ok(d) => {
                    if d.last {
                        self.pending.retain(|(t, _)| *t != d.ticket);
                    }
                    done.push(d);
                }
                Err(TryRecvError::Empty) => return Some(done),
                Err(TryRecvError::Disconnected) => return None,
            }
        }
    }
}

impl Drop for PlanWorker {
    fn drop(&mut self) {
        for (_, p) in &self.pending {
            p.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
