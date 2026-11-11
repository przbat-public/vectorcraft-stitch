//! The image window's thread: it runs digitizing's stages kept between runs
//! ([`DigitizePipeline`]) off the interface thread, so a slow picture never stalls a frame, and hands
//! each stage over as soon as it is done (the colours show before the stitches are planned).
//!
//! One job at a time, the newest: a job sent while another waits replaces it (the one replaced is
//! answered [`Outcome::Cancelled`]). Each job runs inside its own [`Progress`], which reports how
//! far it has got and cancels it, also inside upstream's trace (asked as it goes); a panic is caught
//! and answered as a failure, and the thread goes on serving. The pipeline lives on the thread, so
//! what it keeps between runs is never shared.
//!
//! [`Worker::spawn`] gives `None` on wasm (no threads), in unit tests and with
//! `VECTORCRAFT_SYNC_RENDER` set: the window then runs each job on the interface thread, in one go,
//! in the frame after the one that asked for it ([`Driver::InLine`]); so it does if the thread ever
//! ends.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use vectorcraft_engine::EngineError;
use vectorcraft_engine::cmd::embroidery::{DigitizeOutputs, DigitizePipeline, DigitizeRequest, DigitizeStage};
use vectorcraft_engine::cmd::fileio::progress::{self, Progress};

/// The Colours view's raster, painted on the thread as the colours are done: of which colours
/// output (its number), and its size and pixels (RGBA, row by row).
pub(crate) struct Painted {
    pub(crate) number: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// What a job's step hands over.
pub(crate) enum Outcome {
    /// A stage done: the outputs so far, whether it was made now (rather than found kept), and,
    /// for colours made now, the Colours view's raster.
    Stage { stage: DigitizeStage, outputs: DigitizeOutputs, made: bool, painted: Option<Arc<Painted>> },
    /// Every stage done.
    Finished(DigitizeOutputs),
    /// Refused or failed, with the reason in words (the engine's, or a bug's).
    Failed(String),
    /// Cancelled, or replaced by a newer job before it started.
    Cancelled,
}

/// One step of a job, finished.
pub(crate) struct Done {
    pub(crate) ticket: u64,
    pub(crate) outcome: Outcome,
    /// The job sends nothing more.
    pub(crate) last: bool,
}

/// A run asked for: which (`ticket`), of what, and the progress that reports on it and cancels it.
pub(crate) struct Job {
    pub(crate) ticket: u64,
    pub(crate) request: Arc<DigitizeRequest>,
    pub(crate) progress: Arc<Progress>,
    /// Run inside the job's progress scope before the pipeline (the tests block or panic there).
    #[cfg(test)]
    pub(crate) hook: Option<Hook>,
}

/// What a test runs inside a job before the pipeline.
#[cfg(test)]
pub(crate) type Hook = Box<dyn Fn(&Job) + Send>;

impl Job {
    pub(crate) fn new(ticket: u64, request: Arc<DigitizeRequest>, progress: Arc<Progress>) -> Self {
        Self {
            ticket,
            request,
            progress,
            #[cfg(test)]
            hook: None,
        }
    }

    fn done(&self, outcome: Outcome, last: bool) -> Done {
        Done { ticket: self.ticket, outcome, last }
    }
}

/// The text of a panic caught while digitizing, as the background jobs give it.
fn internal(msg: &str) -> String {
    format!("internal error: {msg} (please report this bug)")
}

/// Run `job` on `pipeline`: each stage handed to `send` as it is done (the Colours view painted
/// for colours made now, the stitch scene built so the interface finds it ready), then the outputs.
/// Everything runs inside the job's progress scope with panics caught; a cancel shows as
/// [`Outcome::Cancelled`]. The thread and the interface thread both run it.
pub(crate) fn run(pipeline: &mut DigitizePipeline, job: &Job, send: &mut dyn FnMut(Done)) {
    let ran = progress::scope(&job.progress, || {
        vectorcraft_engine::guard::catch_panic(|| {
            #[cfg(test)]
            if let Some(hook) = &job.hook {
                hook(job);
            }
            let progress = &job.progress;
            let asked = |_, _| !progress.cancelled();
            pipeline.run(&job.request, &asked, &mut |stage, outputs, made| {
                let painted = if stage == DigitizeStage::Colours && made { painted(outputs) } else { None };
                if stage == DigitizeStage::Plan {
                    // Built here, so the interface thread finds it ready.
                    drop(outputs.planned().map(|p| p.scene()));
                }
                send(job.done(Outcome::Stage { stage, outputs: outputs.clone(), made, painted }, false));
            })
        })
    });
    let outcome = match ran {
        Ok(Ok(outputs)) => Outcome::Finished(outputs),
        Ok(Err(EngineError::Cancelled)) => Outcome::Cancelled,
        Ok(Err(e)) => Outcome::Failed(crate::i18n::msg(&e.to_string())),
        Err(msg) => Outcome::Failed(internal(&msg)),
    };
    send(job.done(outcome, true));
}

/// The Colours view's raster of `outputs`' colours.
pub(crate) fn painted(outputs: &DigitizeOutputs) -> Option<Arc<Painted>> {
    let number = outputs.number(DigitizeStage::Colours)?;
    let (width, height, rgba) = outputs.painted_rgba()?;
    Some(Arc::new(Painted { number, width, height, rgba }))
}

/// The window's thread. Dropping it cancels the jobs it hasn't answered in full and ends the
/// thread once the one under way stops.
pub(crate) struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
    /// The jobs sent and not finished: cancelled on drop.
    pending: Vec<(u64, Arc<Progress>)>,
    /// The thread has ended.
    gone: bool,
}

impl Worker {
    /// Start the thread with `pipeline`, `None` where digitizing stays on the interface thread: on
    /// wasm, in unit tests, with `VECTORCRAFT_SYNC_RENDER` set; the pipeline comes back then.
    pub(crate) fn spawn(ctx: &egui::Context, pipeline: DigitizePipeline) -> Result<Self, Box<DigitizePipeline>> {
        if cfg!(target_arch = "wasm32") || cfg!(test) || std::env::var_os("VECTORCRAFT_SYNC_RENDER").is_some() {
            return Err(Box::new(pipeline));
        }
        Self::start(ctx, pipeline)
    }

    /// Start the thread with `pipeline`; the pipeline back if it can't start.
    pub(crate) fn start(ctx: &egui::Context, pipeline: DigitizePipeline) -> Result<Self, Box<DigitizePipeline>> {
        let (tx, jobs) = channel::<Job>();
        let (done_tx, rx) = channel::<Done>();
        let ctx = ctx.clone();
        // The pipeline goes to the thread once it has started, so that a thread that can't start
        // leaves it here.
        let (give, take) = channel::<DigitizePipeline>();
        let started = std::thread::Builder::new().name("vectorcraft-digitize".into()).spawn(move || {
            let Ok(mut pipeline) = take.recv() else { return };
            // The window is gone (its sender dropped): end the thread.
            while let Ok(mut job) = jobs.recv() {
                let mut open = true;
                let mut send = |d: Done| {
                    open &= done_tx.send(d).is_ok();
                    ctx.request_repaint();
                };
                // The newest job only: those it replaces are answered as cancelled.
                while let Ok(newer) = jobs.try_recv() {
                    send(std::mem::replace(&mut job, newer).done(Outcome::Cancelled, true));
                }
                if job.progress.cancelled() {
                    send(job.done(Outcome::Cancelled, true));
                } else {
                    run(&mut pipeline, &job, &mut send);
                }
                if !open {
                    break;
                }
            }
        });
        if started.is_err() {
            return Err(Box::new(pipeline));
        }
        give.send(pipeline).map_err(|e| Box::new(e.0))?;
        Ok(Self { tx, rx, pending: Vec::new(), gone: false })
    }

    /// Send `job` → `None`, or the job back when the thread is gone.
    #[must_use]
    pub(crate) fn submit(&mut self, job: Job) -> Option<Job> {
        let (ticket, progress) = (job.ticket, job.progress.clone());
        match self.tx.send(job) {
            Ok(()) => {
                self.pending.push((ticket, progress));
                None
            }
            Err(std::sync::mpsc::SendError(job)) => Some(job),
        }
    }

    /// The steps finished since the last call, oldest first. Once the thread is gone the jobs it
    /// had are answered as cancelled.
    pub(crate) fn poll(&mut self) -> Vec<Done> {
        let mut done = vec![];
        loop {
            match self.rx.try_recv() {
                Ok(d) => {
                    if d.last {
                        self.pending.retain(|(t, _)| *t != d.ticket);
                    }
                    done.push(d);
                }
                Err(TryRecvError::Empty) => return done,
                Err(TryRecvError::Disconnected) => {
                    done.extend(self.pending.drain(..).map(|(ticket, _)| Done { ticket, outcome: Outcome::Cancelled, last: true }));
                    self.gone = true;
                    return done;
                }
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        for (_, p) in &self.pending {
            p.cancel();
        }
    }
}

/// Where the window's jobs run: on its thread, or on the interface thread where there is none
/// (wasm, unit tests, `VECTORCRAFT_SYNC_RENDER`, or once the thread has ended): there a job runs in
/// the frame after the one that asked for it, so the window says it is recomputing first. A thread
/// that ended took its pipeline with it: the window gives a fresh one ([`Driver::needs_pipeline`]).
pub(crate) enum Driver {
    Thread(Worker),
    InLine { pipeline: Option<Box<DigitizePipeline>>, queued: Option<Job>, done: Vec<Done> },
}

impl Driver {
    /// The window's thread where there can be one, else digitizing in line.
    pub(crate) fn new(ctx: &egui::Context, pipeline: DigitizePipeline) -> Self {
        match Worker::spawn(ctx, pipeline) {
            Ok(w) => Self::Thread(w),
            Err(pipeline) => Self::InLine { pipeline: Some(pipeline), queued: None, done: Vec::new() },
        }
    }

    /// Whether it runs in line with no pipeline (its thread ended, taking it): give it one.
    pub(crate) fn needs_pipeline(&self) -> bool {
        matches!(self, Self::InLine { pipeline: None, .. })
    }

    /// Give it `pipeline` to run in line with, when it has none.
    pub(crate) fn give_pipeline(&mut self, pipeline: DigitizePipeline) {
        if let Self::InLine { pipeline: p @ None, .. } = self {
            *p = Some(Box::new(pipeline));
        }
    }

    /// Send `job` to the thread, or queue it for the next [`Driver::poll`] (a job queued before is
    /// replaced, answered as cancelled).
    pub(crate) fn submit(&mut self, job: Job) {
        let job = match self {
            Self::Thread(w) => match w.submit(job) {
                None => return,
                Some(job) => {
                    // The thread has gone: from now on in line.
                    let done = w.poll();
                    *self = Self::InLine { pipeline: None, queued: None, done };
                    job
                }
            },
            Self::InLine { .. } => job,
        };
        if let Self::InLine { queued, done, .. } = self
            && let Some(old) = queued.replace(job)
        {
            done.push(old.done(Outcome::Cancelled, true));
        }
    }

    /// The steps finished since the last call, oldest first; in line, the job queued is run now
    /// (once there is a pipeline).
    pub(crate) fn poll(&mut self) -> Vec<Done> {
        match self {
            Self::Thread(w) => {
                let done = w.poll();
                if w.gone {
                    *self = Self::InLine { pipeline: None, queued: None, done: Vec::new() };
                }
                done
            }
            Self::InLine { pipeline: Some(pipeline), queued, done } => {
                if let Some(job) = queued.take() {
                    if job.progress.cancelled() {
                        done.push(job.done(Outcome::Cancelled, true));
                    } else {
                        run(pipeline, &job, &mut |d| done.push(d));
                    }
                }
                std::mem::take(done)
            }
            Self::InLine { pipeline: None, done, .. } => std::mem::take(done),
        }
    }

    /// Whether jobs run in the frame (no thread): a change then waits for the slider to be let go.
    pub(crate) fn in_line(&self) -> bool {
        matches!(self, Self::InLine { .. })
    }
}

#[cfg(test)]
pub(crate) mod tests;
