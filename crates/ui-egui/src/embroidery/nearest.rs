//! Nearest Connection from the interface (the Embroidery panel's ≡ menu and the Embroidery Tool's
//! bar), the project owner's choice of 2026-10-26: on the desktop it searches off the interface
//! thread ([`vectorcraft_engine::cmd::embroidery::NearestJob`]), the status bar saying how far it
//! has got, and Escape stops it; when it finishes, what it found is applied as one undo step, only
//! if the document is as the search took it (edited meanwhile, nothing is applied, and the status
//! bar says so). On the web, which has no thread, it searches in one go in the frame after the one
//! whose status bar says it has begun, the page waiting meanwhile. Agents' and scripts'
//! `embroidery.nearestConnection` runs in one go, as every command does.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Key, Modifiers};
use serde_json::json;
use vectorcraft_doc::cancel::{self, CancelToken};
use vectorcraft_engine::cmd::embroidery::{NEAREST_PASSES, NEAREST_PROGRESS, NearestFound, NearestJob};

use crate::VectorcraftApp;
use crate::panels::embroidery::{command_label, nearest_words};

/// The command it runs, by its id: its label is the menu's.
const NEAREST: &str = "embroidery.nearestConnection";

/// How often the status bar's progress is drawn again while the search runs, seconds.
const PROGRESS_EVERY_SECS: f32 = 0.1;

/// Thousandths: how a planning pass's own progress is kept between threads.
const PER_MILLE: u64 = 1000;

/// The most the progress says before the search has finished, percent: the passes it counts are at
/// most what it makes, so it may finish sooner, never later.
const MOST_BEFORE_DONE: u64 = 99;

/// Nearest Connection under way, if any.
#[derive(Default)]
pub struct Nearest {
    running: Option<Running>,
}

/// A search under way: of which document, how to stop it, how far it has got, and where its result
/// comes from.
struct Running {
    uid: u64,
    stop: CancelToken,
    passes: Arc<AtomicU64>,
    pass: Arc<AtomicU64>,
    result: Pending,
}

/// Where a search's result comes from: its thread, or (on the web, or when no thread starts) the
/// job itself, searched in the next frame.
enum Pending {
    Thread(Receiver<vectorcraft_engine::Result<NearestFound>>),
    InLine(Box<NearestJob>),
}

/// Whether a search is under way: the menu's item and the tool's button wait for it.
pub fn running(app: &VectorcraftApp) -> bool {
    app.embroidery.nearest.running.is_some()
}

/// Begin Nearest Connection on the active document (the selection, else every object), unless one
/// is under way.
pub fn start(app: &mut VectorcraftApp) {
    if running(app) {
        return;
    }
    let job = match app.session.nearest_connection_job(&json!({})) {
        Ok(job) => job,
        Err(e) => return app.status(crate::i18n::msg(&e.to_string())),
    };
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let (stop, passes, pass) = (CancelToken::new(), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let result = if cfg!(target_arch = "wasm32") { Pending::InLine(Box::new(job)) } else { spawn(job, &stop, &passes, &pass) };
    app.embroidery.nearest.running = Some(Running { uid, stop, passes, pass, result });
    say_progress(app);
}

/// `job` searched on a thread of its own, reporting to `passes` and `pass` and stopped by `stop`;
/// in line in the next frame where no thread starts.
fn spawn(job: NearestJob, stop: &CancelToken, passes: &Arc<AtomicU64>, pass: &Arc<AtomicU64>) -> Pending {
    let (send, receive) = channel();
    let (stop, passes, pass) = (stop.clone(), passes.clone(), pass.clone());
    let job = Arc::new(job);
    let searched = job.clone();
    let spawned = std::thread::Builder::new().name("nearest-connection".into()).spawn(move || {
        let sink: cancel::ProgressSink = Rc::new(move |done, total, what| {
            if what == NEAREST_PROGRESS {
                passes.store(done, Ordering::Relaxed);
                pass.store(0, Ordering::Relaxed);
            } else if let Some(part) = done.saturating_mul(PER_MILLE).checked_div(total) {
                pass.store(part, Ordering::Relaxed);
            }
        });
        // The interface has gone when it can't be told: nothing to do then.
        let _ = send.send(cancel::scope(&stop, Some(sink), || searched.search()));
    });
    match spawned {
        Ok(_) => Pending::Thread(receive),
        // No thread started: the job is ours again (the thread's closure is gone), searched in line.
        Err(_) => match Arc::try_unwrap(job) {
            Ok(job) => Pending::InLine(Box::new(job)),
            Err(_) => Pending::Thread(receive),
        },
    }
}

/// Each frame: Escape stops a search under way; a finished one is applied (or says why not), one
/// still running says how far it has got. One for another document than the active one stops.
pub fn poll(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(r) = app.embroidery.nearest.running.as_ref() else { return };
    if app.session.active().map(|st| st.uid) != Some(r.uid) || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        r.stop.cancel();
    }
    // Stopped is stopped, even when the search had just finished.
    let stopped = r.stop.is_cancelled();
    let finished = match &r.result {
        Pending::Thread(receive) => match receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(vectorcraft_engine::EngineError::Internal { cmd: NEAREST.into(), msg: "the search stopped".into() }))
            }
        },
        // The web's frame after the one that said it has begun: searched now, in one go, unless an
        // Escape or another document stopped it first.
        Pending::InLine(_) if stopped => Some(Err(vectorcraft_engine::EngineError::Cancelled)),
        Pending::InLine(job) => Some(job.search()),
    };
    let Some(result) = finished else {
        say_progress(app);
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(PROGRESS_EVERY_SECS));
        return;
    };
    app.embroidery.nearest.running = None;
    let label = command_label(app, NEAREST);
    let said = match result {
        _ if stopped => crate::et!("Stopped: nothing was changed.").to_string(),
        Ok(found) if !app.session.nearest_connection_current(&found) => {
            crate::et!("The document changed meanwhile: nothing was changed.").to_string()
        }
        Ok(found) => match app.session.apply_nearest_connection(found) {
            Ok(reply) => nearest_words(&reply, crate::i18n::current()),
            Err(e) => crate::i18n::msg(&e.to_string()),
        },
        Err(e) => crate::i18n::msg(&e.to_string()),
    };
    app.status(format!("{label} · {said}"));
}

/// The status bar while a search runs: how far it has got, and that Escape stops it.
fn say_progress(app: &mut VectorcraftApp) {
    let Some(r) = app.embroidery.nearest.running.as_ref() else { return };
    let (passes, pass) = (r.passes.load(Ordering::Relaxed), r.pass.load(Ordering::Relaxed));
    let percent = (passes.saturating_mul(PER_MILLE).saturating_add(pass) * 100 / (NEAREST_PASSES * PER_MILLE).max(1)).min(MOST_BEFORE_DONE);
    let label = command_label(app, NEAREST);
    let lang = crate::i18n::current();
    let progress =
        crate::embroidery::text::fill(crate::et!("{percent}% · Esc stops it"), &[("percent", &crate::embroidery::numbers::count(lang, percent))]);
    app.status(format!("{label}… {progress}"));
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};
    use serde_json::json;
    use vectorcraft_engine::Session;

    use super::*;
    use crate::Services;

    /// An app with three running lines, the second started at its end nearer the first, so that
    /// Nearest Connection turns it round.
    fn app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
        app.run("paint.setFill", json!({"none": true})).unwrap();
        app.run("paint.setStroke", json!({"color": "#2233cc"})).unwrap();
        let pt = |v: f64| (30.0 + v) * 72.0 / 25.4;
        for ((ax, ay), (bx, by)) in [((-20.0, 10.0), (0.0, 0.0)), ((1.0, 0.0), (-6.0, 0.0)), ((7.0, 0.0), (27.0, 0.0))] {
            let d = format!("M {} {} L {} {}", pt(ax), pt(ay), pt(bx), pt(by));
            let id = app.run("path.create", json!({ "d": d })).unwrap()["id"].clone();
            app.run("stroke.set", json!({"weight": 1.0, "ids": [id]})).unwrap();
        }
        app.run("select.set", json!({"ids": []})).unwrap();
        app
    }

    /// One frame of [`poll`] with `events`.
    fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<Event>) {
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| poll(app, ui.ctx()));
        out.textures_delta.clear();
    }

    fn until_done(app: &mut VectorcraftApp, ctx: &egui::Context) {
        for _ in 0..1000 {
            if !running(app) {
                return;
            }
            frame(app, ctx, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("still running");
    }

    fn undo_steps(app: &VectorcraftApp) -> usize {
        app.session.doc().unwrap().history.undo.len()
    }

    /// Escape stops a search under way, whatever it found: nothing is changed, and the status bar
    /// says so. A document edited while it searched is left as it is, and the status bar says why;
    /// one left alone gets what it found, as one undo step.
    #[test]
    fn escape_stops_it_and_an_edited_document_is_left_alone() {
        let ctx = egui::Context::default();
        let mut app = app();
        let steps = undo_steps(&app);
        start(&mut app);
        let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![escape]);
        until_done(&mut app, &ctx);
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (steps, "Nearest Connection · Stopped: nothing was changed."));
        start(&mut app);
        app.run("select.all", json!({})).unwrap();
        app.run("object.move", json!({"dx": 1, "dy": 0})).unwrap();
        let edited = undo_steps(&app);
        until_done(&mut app, &ctx);
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (edited, "Nearest Connection · The document changed meanwhile: nothing was changed."));
        start(&mut app);
        until_done(&mut app, &ctx);
        assert_eq!(undo_steps(&app), edited + 1, "{}", app.ui.status);
    }

    /// Where no thread searches (the web), the search runs in the frame after the one that said it
    /// has begun: an Escape in that frame stops it before it searches, nothing changed, and the
    /// status bar says so; with none, what it found is applied in that frame.
    #[test]
    fn escape_stops_a_search_made_in_line() {
        let ctx = egui::Context::default();
        let mut app = app();
        let steps = undo_steps(&app);
        let in_line = |app: &mut VectorcraftApp| {
            let job = app.session.nearest_connection_job(&json!({})).unwrap();
            let uid = app.session.active().unwrap().uid;
            let (stop, passes, pass) = (CancelToken::new(), Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
            app.embroidery.nearest.running = Some(Running { uid, stop, passes, pass, result: Pending::InLine(Box::new(job)) });
        };
        in_line(&mut app);
        let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![escape]);
        assert!(!running(&app));
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (steps, "Nearest Connection · Stopped: nothing was changed."));
        in_line(&mut app);
        frame(&mut app, &ctx, vec![]);
        assert_eq!(undo_steps(&app), steps + 1, "{}", app.ui.status);
    }
}
