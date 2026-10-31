//! Convert to Manual Stitches from the Object tab, off the interface thread: a convert plans the
//! whole design afresh (as the export does), over a second on a design of 22,000 paths, so the plan
//! made for the plan's sake waits for nobody, as Nearest Connection's search doesn't
//! ([`super::nearest`]). The status bar says how far it has got, and Escape stops it; when it
//! finishes, the objects are converted as one undo step, only if the document is as the convert took
//! it (edited meanwhile, nothing is converted, and the status bar says so). On the web, which has no
//! thread, it plans in one go in the frame after the one whose status bar says it has begun.
//! Agents' and scripts' `embroidery.convertToManual` runs in one go, as every command does.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Key, Modifiers};
use serde_json::Value;
use vectorcraft_doc::cancel::{self, CancelToken};
use vectorcraft_engine::cmd::embroidery::{ConvertJob, ConvertPlanned};

use crate::VectorcraftApp;
use crate::panels::embroidery::command_label;

/// The command it runs, by its id: its label is the button's.
const CONVERT: &str = "embroidery.convertToManual";

/// How often the status bar's progress is drawn again while it plans, seconds.
const PROGRESS_EVERY_SECS: f32 = 0.1;

/// Thousandths: how the plan's progress is kept between threads.
const PER_MILLE: u64 = 1000;

/// The most the progress says before the plan has finished, percent.
const MOST_BEFORE_DONE: u64 = 99;

/// A convert under way, if any.
#[derive(Default)]
pub struct Converting {
    running: Option<Running>,
}

/// A convert under way: of which document, how to stop it, how far its plan has got, and where its
/// plan comes from.
struct Running {
    uid: u64,
    stop: CancelToken,
    done: Arc<AtomicU64>,
    result: Pending,
}

/// Where a convert's plan comes from: its thread, or (on the web, or when no thread starts) the job
/// itself, planned in the next frame.
enum Pending {
    Thread(Receiver<vectorcraft_engine::Result<ConvertPlanned>>),
    InLine(Box<ConvertJob>),
}

/// Whether a convert is under way: the button waits for it.
pub fn running(app: &VectorcraftApp) -> bool {
    app.embroidery.converting.running.is_some()
}

/// Begin converting the objects of `params` (the command's) on the active document, unless a
/// convert is under way.
pub fn start(app: &mut VectorcraftApp, params: &Value) {
    if running(app) {
        return;
    }
    let job = match app.session.convert_to_manual_job(params) {
        Ok(job) => job,
        Err(e) => return app.status(crate::i18n::msg(&e.to_string())),
    };
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let (stop, done) = (CancelToken::new(), Arc::new(AtomicU64::new(0)));
    let result = if cfg!(target_arch = "wasm32") { Pending::InLine(Box::new(job)) } else { spawn(job, &stop, &done) };
    app.embroidery.converting.running = Some(Running { uid, stop, done, result });
    say_progress(app);
}

/// `job` planned on a thread of its own, reporting to `done` and stopped by `stop`; in line in the
/// next frame where no thread starts.
fn spawn(job: ConvertJob, stop: &CancelToken, done: &Arc<AtomicU64>) -> Pending {
    let (send, receive) = channel();
    let (stop, done) = (stop.clone(), done.clone());
    let job = Arc::new(job);
    let planned = job.clone();
    let spawned = std::thread::Builder::new().name("convert-to-manual".into()).spawn(move || {
        let sink: cancel::ProgressSink = Rc::new(move |part, total, _| {
            if let Some(per_mille) = part.saturating_mul(PER_MILLE).checked_div(total) {
                done.store(per_mille, Ordering::Relaxed);
            }
        });
        // The interface has gone when it can't be told: nothing to do then.
        let _ = send.send(cancel::scope(&stop, Some(sink), || planned.plan()));
    });
    match spawned {
        Ok(_) => Pending::Thread(receive),
        // No thread started: the job is ours again (the thread's closure is gone), planned in line.
        Err(_) => match Arc::try_unwrap(job) {
            Ok(job) => Pending::InLine(Box::new(job)),
            Err(_) => Pending::Thread(receive),
        },
    }
}

/// Each frame: Escape stops a convert under way; a finished one is applied (or says why not), one
/// still planning says how far it has got. One for another document than the active one stops.
pub fn poll(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(r) = app.embroidery.converting.running.as_ref() else { return };
    if app.session.active().map(|st| st.uid) != Some(r.uid) || ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        r.stop.cancel();
    }
    // Stopped is stopped, even when the plan had just finished.
    let stopped = r.stop.is_cancelled();
    let finished = match &r.result {
        Pending::Thread(receive) => match receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(vectorcraft_engine::EngineError::Internal { cmd: CONVERT.into(), msg: "the plan stopped".into() }))
            }
        },
        // The web's frame after the one that said it has begun: planned now, in one go, unless an
        // Escape or another document stopped it first.
        Pending::InLine(_) if stopped => Some(Err(vectorcraft_engine::EngineError::Cancelled)),
        Pending::InLine(job) => Some(job.plan()),
    };
    let Some(result) = finished else {
        say_progress(app);
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(PROGRESS_EVERY_SECS));
        return;
    };
    app.embroidery.converting.running = None;
    let said = match result {
        _ if stopped => format!("{} · {}", command_label(app, CONVERT), crate::et!("Stopped: nothing was changed.")),
        Ok(planned) if !app.session.convert_to_manual_current(&planned) => {
            format!("{} · {}", command_label(app, CONVERT), crate::et!("The document changed meanwhile: nothing was changed."))
        }
        Ok(planned) => match app.session.apply_convert_to_manual(planned) {
            Ok(reply) => crate::panels::embroidery::object::converted(&reply),
            Err(e) => crate::i18n::msg(&e.to_string()),
        },
        Err(e) => crate::i18n::msg(&e.to_string()),
    };
    app.status(said);
}

/// The status bar while it plans: how far it has got, and that Escape stops it.
fn say_progress(app: &mut VectorcraftApp) {
    let Some(r) = app.embroidery.converting.running.as_ref() else { return };
    let percent = (r.done.load(Ordering::Relaxed) * 100 / PER_MILLE).min(MOST_BEFORE_DONE);
    let label = command_label(app, CONVERT);
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

    /// An app with two red squares, the first selected.
    fn app() -> (VectorcraftApp, u64) {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
        app.run("paint.setStroke", json!({"none": true})).unwrap();
        app.run("paint.setFill", json!({"color": "#cc2222"})).unwrap();
        let a = app.run("shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40})).unwrap()["id"].as_u64().unwrap();
        app.run("shape.rectangle", json!({"x": 120, "y": 20, "width": 60, "height": 40})).unwrap();
        app.run("select.set", json!({"ids": [a]})).unwrap();
        (app, a)
    }

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
        panic!("still converting");
    }

    fn undo_steps(app: &VectorcraftApp) -> usize {
        app.session.doc().unwrap().history.undo.len()
    }

    /// Escape stops a convert under way: nothing is changed, and the status bar says so. A document
    /// edited while it planned is left as it is, and the status bar says why; one left alone is
    /// converted, as one undo step, the status bar saying how to go back.
    #[test]
    fn escape_stops_it_and_an_edited_document_is_left_alone() {
        let ctx = egui::Context::default();
        let (mut app, a) = app();
        let steps = undo_steps(&app);
        start(&mut app, &json!({"ids": [a]}));
        let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![escape]);
        until_done(&mut app, &ctx);
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (steps, "Convert to Manual Stitches · Stopped: nothing was changed."));
        start(&mut app, &json!({"ids": [a]}));
        app.run("object.move", json!({"ids": [a], "dx": 1, "dy": 0})).unwrap();
        let edited = undo_steps(&app);
        until_done(&mut app, &ctx);
        assert_eq!(
            (undo_steps(&app), app.ui.status.as_str()),
            (edited, "Convert to Manual Stitches · The document changed meanwhile: nothing was changed.")
        );
        start(&mut app, &json!({"ids": [a]}));
        until_done(&mut app, &ctx);
        assert_eq!(undo_steps(&app), edited + 1, "{}", app.ui.status);
        assert_eq!(app.ui.status, "1 object converted to manual stitches: Restore Object brings it back");
        assert!(app.run("embroidery.info", json!({"ids": [a]})).unwrap()["manual"].is_object());
    }

    /// Where no thread plans (the web), the convert plans in the frame after the one that said it
    /// has begun: an Escape in that frame stops it before it plans, nothing changed, and the status
    /// bar says so; with none, it converts in that frame.
    #[test]
    fn escape_stops_a_convert_planned_in_line() {
        let ctx = egui::Context::default();
        let (mut app, a) = app();
        let steps = undo_steps(&app);
        let in_line = |app: &mut VectorcraftApp| {
            let job = app.session.convert_to_manual_job(&json!({"ids": [a]})).unwrap();
            let uid = app.session.active().unwrap().uid;
            let (stop, done) = (CancelToken::new(), Arc::new(AtomicU64::new(0)));
            app.embroidery.converting.running = Some(Running { uid, stop, done, result: Pending::InLine(Box::new(job)) });
        };
        in_line(&mut app);
        let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![escape]);
        assert!(!running(&app));
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (steps, "Convert to Manual Stitches · Stopped: nothing was changed."));
        in_line(&mut app);
        frame(&mut app, &ctx, vec![]);
        assert_eq!(undo_steps(&app), steps + 1, "{}", app.ui.status);
    }
}
