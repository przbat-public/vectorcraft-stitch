//! Digitize Image from the command palette or a shortcut: on the desktop it digitizes off the
//! interface thread ([`vectorcraft_engine::cmd::embroidery::DigitizeJob`]), as Nearest Connection
//! searches ([`super::nearest`]), the status bar saying how far it has got, and Escape stops it;
//! when it finishes, what it made is applied as one undo step, only if the document is as the job
//! took it (edited meanwhile, nothing is applied, and the status bar says so). On the web, which has
//! no thread, it runs in one go in the frame after the one whose status bar says it has begun.
//! Agents' and scripts' `embroidery.digitizeImage` runs in one go, as every command does.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Key, Modifiers};
use serde_json::json;
use vectorcraft_doc::cancel::{self, CancelToken};
use vectorcraft_engine::cmd::embroidery::{DIGITIZE_PROGRESS, DIGITIZE_STAGES, DigitizeFound, DigitizeJob};

use crate::VectorcraftApp;
use crate::panels::embroidery::command_label;

/// The command it runs, by its id: its label is the menu's.
const DIGITIZE: &str = "embroidery.digitizeImage";

/// How often the status bar's progress is drawn again while it runs, seconds.
const PROGRESS_EVERY_SECS: f32 = 0.1;

/// The most the progress says before it has finished, percent: its last stage ends with the reply.
const MOST_BEFORE_DONE: u64 = 99;

/// Digitize Image under way, if any.
#[derive(Default)]
pub struct Digitizing {
    running: Option<Running>,
}

/// A run under way: of which document, how to stop it, how far it has got, and where its result
/// comes from.
struct Running {
    uid: u64,
    stop: CancelToken,
    stages: Arc<AtomicU64>,
    result: Pending,
}

/// Where a run's result comes from: its thread, or (on the web, or when no thread starts) the job
/// itself, run in the next frame.
enum Pending {
    Thread(Receiver<vectorcraft_engine::Result<DigitizeFound>>),
    InLine(Box<DigitizeJob>),
}

/// Whether a run is under way: the palette's item waits for it.
pub fn running(app: &VectorcraftApp) -> bool {
    app.embroidery.digitizing.running.is_some()
}

/// Begin Digitize Image on the active document's selected image with the default settings, unless
/// one is under way.
pub fn start(app: &mut VectorcraftApp) {
    if running(app) {
        return;
    }
    let job = match app.session.digitize_image_job(&json!({})) {
        Ok(job) => job,
        Err(e) => return app.status(crate::i18n::msg(&e.to_string())),
    };
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let (stop, stages) = (CancelToken::new(), Arc::new(AtomicU64::new(0)));
    let result = if cfg!(target_arch = "wasm32") { Pending::InLine(Box::new(job)) } else { spawn(job, &stop, &stages) };
    app.embroidery.digitizing.running = Some(Running { uid, stop, stages, result });
    say_progress(app);
}

/// `job` run on a thread of its own, reporting its stages to `stages` and stopped by `stop`, which
/// the trace's own threads read too; in line in the next frame where no thread starts.
fn spawn(job: DigitizeJob, stop: &CancelToken, stages: &Arc<AtomicU64>) -> Pending {
    let (send, receive) = channel();
    let (stop, stages) = (stop.clone(), stages.clone());
    let job = Arc::new(job);
    let run = job.clone();
    let spawned = std::thread::Builder::new().name("digitize-image".into()).spawn(move || {
        let sink: cancel::ProgressSink = Rc::new(move |done, _, what| {
            if what == DIGITIZE_PROGRESS {
                stages.store(done, Ordering::Relaxed);
            }
        });
        let poll = stop.clone();
        // The interface has gone when it can't be told: nothing to do then.
        let _ = send.send(cancel::scope(&stop, Some(sink), || run.run(&|_, _| !poll.is_cancelled())));
    });
    match spawned {
        Ok(_) => Pending::Thread(receive),
        // No thread started: the job is ours again (the thread's closure is gone), run in line.
        Err(_) => match Arc::try_unwrap(job) {
            Ok(job) => Pending::InLine(Box::new(job)),
            Err(_) => Pending::Thread(receive),
        },
    }
}

/// Each frame: Escape stops a run under way; a finished one is applied (or says why not), one still
/// running says how far it has got. One for another document than the active one stops.
pub fn poll(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(r) = app.embroidery.digitizing.running.as_ref() else { return };
    // Escape is an open dialog's or a field's first (it closes or cancels that), then the run's.
    let escape = app.ui.dialog.is_none() && !ctx.egui_wants_keyboard_input() && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    if app.session.active().map(|st| st.uid) != Some(r.uid) || escape {
        r.stop.cancel();
    }
    // Stopped is stopped, even when the run had just finished.
    let stopped = r.stop.is_cancelled();
    let finished = match &r.result {
        Pending::Thread(receive) => match receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(vectorcraft_engine::EngineError::Internal { cmd: DIGITIZE.into(), msg: "digitizing stopped".into() }))
            }
        },
        // The web's frame after the one that said it has begun: run now, in one go, unless an
        // Escape or another document stopped it first.
        Pending::InLine(_) if stopped => Some(Err(vectorcraft_engine::EngineError::Cancelled)),
        Pending::InLine(job) => Some(job.run(&|_, _| true)),
    };
    let Some(result) = finished else {
        say_progress(app);
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(PROGRESS_EVERY_SECS));
        return;
    };
    app.embroidery.digitizing.running = None;
    let label = command_label(app, DIGITIZE);
    let said = match result {
        _ if stopped => crate::et!("Stopped: nothing was changed.").to_string(),
        Ok(found) if !app.session.digitize_image_current(&found) => crate::et!("The document changed meanwhile: nothing was changed.").to_string(),
        Ok(found) => match app.session.apply_digitize_image(found) {
            Ok(reply) => {
                // Counted first: the catalogues' scan takes a count's words from the call's first
                // strings.
                let made = reply["created"]["objects"].as_u64().unwrap_or(0);
                crate::etn!(made, "{count} object made", "{count} objects made")
            }
            Err(e) => crate::i18n::msg(&e.to_string()),
        },
        Err(e) => crate::i18n::msg(&e.to_string()),
    };
    app.status(format!("{label} · {said}"));
}

/// The status bar while it runs: how far it has got, and that Escape stops it.
fn say_progress(app: &mut VectorcraftApp) {
    let Some(r) = app.embroidery.digitizing.running.as_ref() else { return };
    let percent = (r.stages.load(Ordering::Relaxed) * 100 / DIGITIZE_STAGES.max(1)).min(MOST_BEFORE_DONE);
    let label = command_label(app, DIGITIZE);
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

    /// An app with a picture placed and selected: an orange square with a navy bar, on white.
    fn app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
        let picture = image::RgbaImage::from_fn(200, 100, |x, y| {
            image::Rgba(match ((20..180).contains(&x), (45..55).contains(&y), (10..90).contains(&y)) {
                (true, true, _) => [20, 40, 120, 255],
                (true, _, true) => [240, 130, 20, 255],
                _ => [255, 255, 255, 255],
            })
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(picture).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let data = vectorcraft_format::base64_encode(&png);
        app.run("file.place", json!({"name": "logo.png", "dataBase64": data})).unwrap();
        app
    }

    /// One frame of [`poll`] with `events`.
    fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<Event>) {
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| poll(app, ui.ctx()));
        out.textures_delta.clear();
    }

    /// Frames until the run is done, or a deadline generous for a busy machine has passed.
    fn until_done(app: &mut VectorcraftApp, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while running(app) {
            assert!(std::time::Instant::now() < deadline, "still running");
            frame(app, ctx, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn undo_steps(app: &VectorcraftApp) -> usize {
        app.session.doc().unwrap().history.undo.len()
    }

    /// The palette's Digitize Image runs off the interface thread: Escape stops it, nothing changed
    /// and the status bar saying so; a document edited while it ran is left as it is, and the status
    /// bar says why; one left alone gets what it made, as one undo step.
    #[test]
    fn the_palette_runs_it_off_the_interface_thread() {
        let ctx = egui::Context::default();
        let mut app = app();
        let image = app.session.doc().unwrap().selection.objects[0];
        let steps = undo_steps(&app);
        assert!(crate::embroidery::menu::invoked(&mut app, DIGITIZE));
        let escape = Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        frame(&mut app, &ctx, vec![escape]);
        until_done(&mut app, &ctx);
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (steps, "Digitize Image · Stopped: nothing was changed."));
        start(&mut app);
        app.run("shape.rectangle", json!({"x": 300, "y": 300, "width": 20, "height": 20})).unwrap();
        let edited = undo_steps(&app);
        app.run("select.set", json!({"ids": []})).unwrap();
        until_done(&mut app, &ctx);
        assert_eq!((undo_steps(&app), app.ui.status.as_str()), (edited, "Digitize Image · The document changed meanwhile: nothing was changed."));
        app.run("edit.undo", json!({})).unwrap();
        app.run("select.set", json!({"ids": [image.0]})).unwrap();
        let before = undo_steps(&app);
        start(&mut app);
        until_done(&mut app, &ctx);
        assert_eq!(undo_steps(&app), before + 1, "{}", app.ui.status);
        assert!(app.ui.status.starts_with("Digitize Image · ") && app.ui.status.ends_with("objects made"), "{}", app.ui.status);
    }
}
