//! The inspector's satin answers ([`DigitizeOutputs::satin_parts`], [`DigitizeOutputs::satin_offer`]),
//! worked out off the interface thread: each is the planner's parting of an area's object, which on
//! a large area with many narrow parts, or on the many alike areas Apply to Similar Areas sets with
//! Satin, takes a noticeable fraction of a second or more. The inspector asks for the areas it shows
//! and reads only the answers worked out ([`DigitizeOutputs::satin_parts_kept`]); the thread keeps
//! them in the result's outputs, shared by their clones, and has the window drawn again once they
//! are in.
//!
//! One ask at a time, the newest: one sent cancels the one before, unless that one holds all its
//! areas already. [`Answers::new`] works them out in the frame that asks instead on wasm (no
//! threads), in unit tests and with `VECTORCRAFT_SYNC_RENDER` set, as the window's runs are
//! ([`super::worker`]); so it does if the thread ever ends.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use vectorcraft_engine::cmd::embroidery::{DigitizeOutputs, DigitizeStage};
use vectorcraft_engine::cmd::fileio::progress::{self, Progress};

/// The areas, by working pixel, whose satin answers are asked of a result.
pub(crate) struct Ask {
    outputs: DigitizeOutputs,
    at: Vec<(u32, u32)>,
    /// Cancelled once a newer ask replaces it.
    progress: Arc<Progress>,
}

#[cfg(test)]
impl Ask {
    /// The working pixels of the areas asked about.
    pub(crate) fn areas(&self) -> &[(u32, u32)] {
        &self.at
    }
}

/// The last ask sent: of which result (its plan's number), its areas, and its progress.
type Sent = (Option<u64>, HashSet<(u32, u32)>, Arc<Progress>);

/// Where the inspector's satin answers are worked out.
pub(crate) enum Answers {
    /// On a thread of their own: the last ask sent, and how many asks the thread has answered (what
    /// the window read of them is read again once that changes).
    Thread { tx: Sender<Ask>, last: Option<Sent>, answered: Arc<AtomicU64> },
    /// In the frame that asks.
    InLine,
}

impl Answers {
    /// A thread of their own where there can be one: not on wasm, in unit tests, or with
    /// `VECTORCRAFT_SYNC_RENDER` set; there, or when it can't start, in the frame.
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        if cfg!(target_arch = "wasm32") || cfg!(test) || std::env::var_os("VECTORCRAFT_SYNC_RENDER").is_some() {
            return Self::InLine;
        }
        Self::start(ctx)
    }

    /// Start the thread, which has the window drawn again (`ctx`) as its answers come in; in the
    /// frame when it can't start.
    pub(crate) fn start(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel::<Ask>();
        let answered = Arc::new(AtomicU64::new(0));
        let (count, ctx) = (Arc::clone(&answered), ctx.clone());
        match std::thread::Builder::new().name("vectorcraft-satin-answers".into()).spawn(move || serve(&rx, &count, &ctx)) {
            Ok(_) => Self::Thread { tx, last: None, answered },
            Err(_) => Self::InLine,
        }
    }

    /// Ask for the satin answers of the areas under working pixels `at` (the inspector's own first)
    /// of `outputs`: in the frame, worked out now; else sent to the thread, unless the last ask of
    /// the same result holds them all, or the result's stitches aren't planned yet (nothing to work
    /// out: asked again once they are, a result of its own).
    pub(crate) fn ask(&mut self, outputs: &DigitizeOutputs, at: &[(u32, u32)]) {
        let Self::Thread { tx, last, .. } = self else { return work_out(outputs, at, &|| {}) };
        let plan = outputs.number(DigitizeStage::Plan);
        if plan.is_none() || last.as_ref().is_some_and(|(p, asked, _)| *p == plan && at.iter().all(|q| asked.contains(q))) {
            return;
        }
        if let Some((_, _, before)) = last.take() {
            before.cancel();
        }
        let progress = Progress::new();
        match tx.send(Ask { outputs: outputs.clone(), at: at.to_vec(), progress: Arc::clone(&progress) }) {
            Ok(()) => *last = Some((plan, at.iter().copied().collect(), progress)),
            // The thread has ended: from now on in the frame.
            Err(_) => {
                *self = Self::InLine;
                work_out(outputs, at, &|| {});
            }
        }
    }

    /// How many asks the thread has answered (0 in the frame, where each is answered as it is asked).
    pub(crate) fn answered(&self) -> u64 {
        match self {
            Self::Thread { answered, .. } => answered.load(Ordering::Relaxed),
            Self::InLine => 0,
        }
    }

    /// Sent to a thread a test holds (whose asks it answers with [`answer`] when it chooses), and the
    /// count of asks answered it bumps.
    #[cfg(test)]
    pub(crate) fn held() -> (Self, Receiver<Ask>, Arc<AtomicU64>) {
        let (tx, rx) = channel();
        let answered = Arc::new(AtomicU64::new(0));
        (Self::Thread { tx, last: None, answered: Arc::clone(&answered) }, rx, answered)
    }
}

impl Drop for Answers {
    /// The window closed: the ask under way stops.
    fn drop(&mut self) {
        if let Self::Thread { last: Some((_, _, progress)), .. } = self {
            progress.cancel();
        }
    }
}

/// The thread: each newest ask worked out, counted in `answered`, the window drawn again (`ctx`);
/// ends once the window is gone (its sender dropped).
fn serve(rx: &Receiver<Ask>, answered: &AtomicU64, ctx: &egui::Context) {
    while let Ok(mut ask) = rx.recv() {
        // The newest only: those it replaced were cancelled as it was sent.
        while let Ok(newer) = rx.try_recv() {
            ask = newer;
        }
        answer(&ask, answered, &|| ctx.request_repaint());
        ctx.request_repaint();
    }
}

/// Work `ask` out (as the thread does), counted in `answered` once it is, `shown` asked to draw the
/// window again once its first area's answers are in.
pub(crate) fn answer(ask: &Ask, answered: &AtomicU64, shown: &dyn Fn()) {
    progress::scope(&ask.progress, || work_out(&ask.outputs, &ask.at, shown));
    answered.fetch_add(1, Ordering::Relaxed);
}

/// The satin answers of the areas under working pixels `at` of `outputs`, worked out and kept there:
/// as planned, and whether Satin is offered (at the width Create would give it); `shown` asked to
/// draw the window again once the first area's are in. Stops once cancelled (the scope's progress);
/// a panic is caught (a bug), the answers it would have given left unkept.
fn work_out(outputs: &DigitizeOutputs, at: &[(u32, u32)], shown: &dyn Fn()) {
    // The panic is logged by the guard; the answers are read from the outputs, not returned.
    let _ = vectorcraft_engine::guard::catch_panic(|| {
        for (i, &(x, y)) in at.iter().enumerate() {
            if vectorcraft_engine::doc::cancel::is_cancelled() {
                return;
            }
            // Kept in the outputs, where the inspector reads them.
            let _ = (outputs.satin_parts(x, y, None), outputs.satin_offer(x, y));
            if i == 0 {
                shown();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::super::worker::tests::picture_session;
    use super::*;

    /// The picture's result, its stitches planned: and the working pixel inside its orange square.
    fn outputs() -> (DigitizeOutputs, (u32, u32)) {
        let (s, id) = picture_session();
        let request = s.digitize_window_request(id, &json!({"colors": 2}), None, &Value::Null).unwrap();
        (s.digitize_pipeline().run(&request, &|_, _| true, &mut |_, _, _| {}).unwrap(), (60, 40))
    }

    /// Asked with a thread, nothing is worked out on the asking thread: the ask is sent, and asked
    /// again for areas it holds, nothing more is; one for another area cancels it and is sent; once
    /// answered (as the thread answers), the answers are kept in the result and counted.
    #[test]
    fn an_ask_is_sent_and_nothing_worked_out_on_the_asking_thread() {
        let (out, at) = outputs();
        let (mut answers, rx, answered) = Answers::held();
        answers.ask(&out, &[at]);
        assert_eq!(out.satin_kept_count(), 0, "nothing worked out here");
        let first = rx.try_recv().expect("sent");
        answers.ask(&out, &[at]);
        assert!(rx.try_recv().is_err(), "held by the ask before: not sent again");
        answers.ask(&out, &[at, (5, 5)]);
        let second = rx.try_recv().expect("another area: sent");
        assert!(first.progress.cancelled() && !second.progress.cancelled(), "the one before cancelled");
        answer(&second, &answered, &|| {});
        assert!(out.satin_known(at.0, at.1), "kept in the result");
        assert_eq!(answers.answered(), 1);
    }

    /// Before the stitches are planned there is nothing to work out: nothing is sent.
    #[test]
    fn nothing_is_asked_before_the_stitches_are_planned() {
        let (s, id) = picture_session();
        let request = s.digitize_window_request(id, &json!({"colors": 2}), None, &Value::Null).unwrap();
        let mut early = None;
        s.digitize_pipeline()
            .run(&request, &|_, _| true, &mut |stage, o, _| {
                if stage == DigitizeStage::Objects {
                    early = Some(o.clone());
                }
            })
            .unwrap();
        let (mut answers, rx, _) = Answers::held();
        answers.ask(&early.expect("the objects' stage"), &[(60, 40)]);
        assert!(rx.try_recv().is_err());
    }

    /// The thread works an ask out off the asking thread: its answers come to be kept in the result.
    #[test]
    fn the_thread_works_the_answers_out() {
        let (out, at) = outputs();
        let mut answers = Answers::start(&egui::Context::default());
        assert!(matches!(answers, Answers::Thread { .. }), "the thread started");
        answers.ask(&out, &[at]);
        let deadline = Instant::now() + Duration::from_secs(120);
        while !(out.satin_known(at.0, at.1) && answers.answered() == 1) {
            assert!(Instant::now() < deadline, "answered in time");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// In the frame, an ask is worked out at once.
    #[test]
    fn in_the_frame_an_ask_is_worked_out_at_once() {
        let (out, at) = outputs();
        Answers::InLine.ask(&out, &[at]);
        assert!(out.satin_known(at.0, at.1));
    }
}
