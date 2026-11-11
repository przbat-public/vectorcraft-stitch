//! Digitize Image…: the large window over the document that digitizes the selected image live
//! (FORK.md's work item 9; the project owner's decisions of 2026-11-11, `docs/embroidery.md` ›
//! Digitizing from an image).
//!
//! - **One of upstream's dialogs** (a row of the dialog registry that draws its own window): agents
//!   reach it with `ui.inspect`, `ui.dialog.set` and `ui.dialog.confirm` (Create), Escape closes it
//!   (a popup open in it first), and Document Setup can open over it. Its fields are small JSON:
//!   the engine's settings ([`SETTING_FIELDS`]), the palette its threads are matched to, and what
//!   it shows; `__state` says how its result stands.
//! - **Opened** by Digitize Image… (Object › Image Trace, the command palette, the Embroidery
//!   panel's Object tab) on the image selected, or an Image Trace object, with its defaults ready:
//!   Create is one click. A refusal (no image, a locked one, a missing linked file) is said in the
//!   status bar and opens nothing.
//! - **Live**: digitizing's stages run off the interface thread ([`worker`]) when a setting
//!   changes, by the stitch preview's rules ([`schedule`]); each stage shows as soon as it is
//!   done.
//! - **Remembered for the session** (the owner's choice): Cancel, Escape or another document made
//!   active close it keeping that image's settings; Digitize Image… on the same image continues;
//!   Start Over goes back to the defaults. Create forgets them.
//! - **Create** applies the run shown when it is of the settings as they are (at once), else when
//!   it is (the window says so); then the window closes, what was made selected. Enter only commits
//!   the field typed in; Cmd/Ctrl+Enter is Create.

pub(crate) mod frame;
pub(crate) mod schedule;
mod words;
pub(crate) mod worker;

use std::collections::HashMap;

use egui::{Key, Modifiers};
use serde_json::{Map, Value, json};
use vectorcraft_engine::doc::NodeId;

use self::schedule::{Freshness, PROGRESS_REPAINT_SECS, Runner, Schedule};
use self::worker::Driver;
use crate::VectorcraftApp;
use crate::dialogs::DialogResult;
use crate::state::Dialog;

/// The dialog's kind: the command it runs.
pub const KIND: &str = "embroidery.digitizeImage";

/// The fields that are the engine's settings (`embroidery.digitizeImage`'s), in its names: the
/// image's size, crop and background; the colours' mode, count, choices and smallest area. The
/// shapes' and stitches' settings keep their defaults in this version of the window.
pub(crate) const SETTING_FIELDS: [&str; 9] = ["width", "height", "fit", "crop", "background", "mode", "colors", "minArea", "colorChoices"];

/// The field holding the palette the window matches threads to (`pec` or `jef`): the document's
/// when it opens; Create makes it the document's.
pub(crate) const PALETTE: &str = "palette";

/// The fields naming the document and the image the window is of.
const UID: &str = "uid";
const IMAGE: &str = "id";

/// The fields of what it shows: the step open (1 to 4) and the preview's view.
pub(crate) const STEP: &str = "step";
pub(crate) const VIEW: &str = "view";

/// The step and the view it opens at: the colours, compared with the original.
const FIRST_STEP: u64 = 2;
const FIRST_VIEW: &str = "compare";

/// The read-only field saying how the result stands, for agents (`ui.inspect`).
const STATE: &str = "__state";

/// The image window's state in the app: each image's settings as its window was left, and the
/// window open.
#[derive(Default)]
pub struct State {
    /// By document and image: the fields of a window closed without Create, for the session.
    remembered: HashMap<(u64, NodeId), Map<String, Value>>,
    pub(crate) live: Option<Live>,
}

/// The window open: what it is of, where its runs go and when, and what it was left with.
pub(crate) struct Live {
    uid: u64,
    image: NodeId,
    pub(crate) runner: Box<dyn Runner>,
    pub(crate) schedule: Schedule,
    /// The fields as last drawn: what is remembered when it closes without Create.
    fields: Map<String, Value>,
    /// Create was asked for before the result was of the settings as they are: it applies once it
    /// is.
    pub(crate) create_waiting: bool,
    /// The window closed by Create: nothing to remember.
    created: bool,
}

/// Whether `kind` names this window.
pub fn opens(kind: &str) -> bool {
    kind == KIND
}

/// The fields of a window first opened on image `id` of document `uid`: the engine's defaults,
/// the document's palette.
fn defaults(app: &VectorcraftApp, uid: u64, id: NodeId) -> Map<String, Value> {
    let palette = app.session.active().map_or("jef", |st| vectorcraft_engine::cmd::embroidery::thread_palette_id(&st.doc));
    let fields = json!({UID: uid, IMAGE: id.0, PALETTE: palette, STEP: FIRST_STEP, VIEW: FIRST_VIEW});
    fields.as_object().cloned().unwrap_or_default()
}

/// Open the window on image `id` (`None`: the one object selected, an image or an Image Trace
/// object), continuing where that image's window was left in this session. A refusal is said in
/// the status bar.
pub fn open(app: &mut VectorcraftApp, id: Option<NodeId>) {
    let Some(st) = app.session.active() else { return };
    let uid = st.uid;
    let picked = match (id, st.selection.objects.as_slice()) {
        (Some(id), _) => id,
        (None, [one]) => *one,
        _ => return app.status(crate::et!("Select one image to digitize it.")),
    };
    // Refused as the command refuses (not an image, locked, a missing linked file).
    if let Err(e) = app.session.digitize_window_request(picked, &json!({}), None) {
        return app.status(crate::i18n::msg(&e.to_string()));
    }
    let fields = app.embroidery.image_window.remembered.get(&(uid, picked)).cloned().unwrap_or_else(|| defaults(app, uid, picked));
    app.embroidery.image_window.live = None;
    app.ui.dialog = Some(Dialog::new(KIND, Value::Object(fields)));
    app.status(crate::et!("Digitize Image: live preview"));
}

/// Start Over: the open window's settings back to the defaults, its image's remembered ones
/// forgotten.
pub(crate) fn start_over(app: &mut VectorcraftApp) {
    let Some((uid, image)) = app.ui.dialog.as_ref().filter(|d| d.kind == KIND).and_then(of) else { return };
    let fields = defaults(app, uid, image);
    app.embroidery.image_window.remembered.remove(&(uid, image));
    if let Some(d) = app.ui.dialog.as_mut() {
        d.fields = fields;
    }
}

/// The engine's settings and the palette the fields hold: what a change is measured on.
pub(crate) fn params_of(fields: &Map<String, Value>) -> Value {
    let mut p = Map::new();
    for k in SETTING_FIELDS.iter().chain([&PALETTE]) {
        if let Some(v) = fields.get(*k).filter(|v| !v.is_null()) {
            p.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(p)
}

/// `params` without the palette: the settings `embroidery.digitizeImage` takes.
pub(crate) fn settings_of(params: &Value) -> Value {
    let mut p = params.clone();
    if let Some(o) = p.as_object_mut() {
        o.remove(PALETTE);
    }
    p
}

/// The document and the image `d` is of.
fn of(d: &Dialog) -> Option<(u64, NodeId)> {
    Some((d.fields.get(UID)?.as_u64()?, NodeId(d.fields.get(IMAGE)?.as_u64()?)))
}

/// The window's run state for agents (`__state`): how its result stands, its progress, and the
/// last change's time.
fn state_json(live: &Live, now: f64) -> Value {
    let (state, progress, reason) = match live.schedule.freshness(now) {
        Freshness::Fresh => ("fresh", None, None),
        Freshness::Pending { progress, .. } => ("pending", progress, None),
        Freshness::Failed(reason) => ("failed", None, Some(reason)),
    };
    let updated = live.schedule.updated.as_ref().map(|(ms, stages)| json!({"ms": ms, "stages": stages.iter().map(|s| s.id()).collect::<Vec<_>>()}));
    json!({"state": state, "progress": progress, "error": reason, "createWaiting": live.create_waiting, "updated": updated})
}

/// The open window, each frame: closed as Cancel when its document is no longer the active one;
/// its runs scheduled and taken in; drawn; Create when it is asked for and ready.
pub fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.dialog.clone().filter(|d| d.kind == KIND) else { return };
    let Some((uid, image)) = of(&d) else { return crate::dialogs::cancel(app) };
    if app.session.active().map(|st| st.uid) != Some(uid) {
        return crate::dialogs::cancel(app);
    }
    let now = ctx.input(|i| i.time);
    if app.embroidery.image_window.live.as_ref().is_none_or(|l| (l.uid, l.image) != (uid, image)) {
        let runner: Box<dyn Runner> = Box::new(Driver::new(ctx, app.session.digitize_pipeline()));
        let live = Live { uid, image, runner, schedule: Schedule::default(), fields: d.fields.clone(), create_waiting: false, created: false };
        app.embroidery.image_window.live = Some(live);
    }
    let params = params_of(&d.fields);
    if let Some(live) = app.embroidery.image_window.live.as_mut() {
        live.schedule.frame(&app.session, image, &params, now, live.runner.as_mut());
    }
    let create_now = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    let asked = frame::draw(app, ctx, &mut d, now);
    match asked {
        Some(frame::Asked::Cancel) => return crate::dialogs::cancel(app),
        Some(frame::Asked::StartOver) => {
            app.ui.dialog = Some(d.clone());
            start_over(app);
            if let Some(reset) = app.ui.dialog.clone() {
                d = reset;
            }
        }
        Some(frame::Asked::Create) => ask_create(app),
        None if create_now => ask_create(app),
        None => {}
    }
    if let Some(live) = app.embroidery.image_window.live.as_mut()
        && live.create_waiting
        && live.schedule.current(&params).is_some()
    {
        live.create_waiting = false;
        match create(app) {
            Ok(_) => return,
            Err(e) => app.status(e),
        }
    }
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    d.fields.insert(STATE.into(), state_json(live, now));
    live.fields = d.fields.clone();
    if live.schedule.busy() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(PROGRESS_REPAINT_SECS));
    }
    if app.ui.dialog.as_ref().is_some_and(|open| open.kind == KIND) {
        app.ui.dialog = Some(d);
    }
}

/// Create asked for (the button, Cmd/Ctrl+Enter): at once when the result shown is of the settings
/// as they are, else once it is.
fn ask_create(app: &mut VectorcraftApp) {
    let ready = app.embroidery.image_window.live.as_ref().is_some_and(|l| l.schedule.current(&params_of(&l.fields)).is_some());
    if ready {
        if let Err(e) = create(app) {
            app.status(e);
        }
    } else if let Some(live) = app.embroidery.image_window.live.as_mut() {
        live.create_waiting = true;
    }
}

/// Create: the run shown applied as one undo step, the window closed, what was made selected and
/// said in the status bar.
fn create(app: &mut VectorcraftApp) -> DialogResult {
    let Some(live) = app.embroidery.image_window.live.as_ref() else { return Err("the image window isn't open".into()) };
    let params = params_of(&live.fields);
    let Some(shown) = live.schedule.current(&params).cloned() else { return Err(crate::et!("The preview isn't ready yet.").to_string()) };
    let key = (live.uid, live.image);
    let reply = app.session.apply_digitize_window(&shown.request, &shown.outputs).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    closed_by_create(app, key, &reply);
    Ok(reply)
}

/// The window closed by Create: its settings forgotten, what was made said.
fn closed_by_create(app: &mut VectorcraftApp, key: (u64, NodeId), reply: &Value) {
    if let Some(live) = app.embroidery.image_window.live.as_mut() {
        live.created = true;
        live.schedule.stop();
    }
    app.embroidery.image_window.remembered.remove(&key);
    app.embroidery.image_window.live = None;
    app.ui.dialog = None;
    let made = reply["created"]["objects"].as_u64().unwrap_or(0);
    app.status(crate::etn!(made, "Created {count} object: Undo removes it.", "Created {count} objects: Undo removes them."));
}

/// `ui.dialog.confirm`: Create, in one go: the run shown when it is of the fields as they are, else
/// a run made now on the interface thread, as an agent asks for it.
pub(crate) fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> DialogResult {
    let Some((uid, image)) = of(d) else { return Err("the image window names no image".into()) };
    if app.embroidery.image_window.live.as_ref().is_some_and(|l| l.schedule.current(&params_of(&d.fields)).is_some()) {
        return create(app);
    }
    let params = params_of(&d.fields);
    let palette = params.get(PALETTE).and_then(Value::as_str).map(str::to_string);
    let request =
        app.session.digitize_window_request(image, &settings_of(&params), palette.as_deref()).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    let outputs = app.session.digitize_pipeline().run(&request, &|_, _| true, &mut |_, _, _| {}).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    let reply = app.session.apply_digitize_window(&request, &outputs).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    closed_by_create(app, (uid, image), &reply);
    Ok(reply)
}

/// Each frame, before the window is drawn: a window that closed without Create (Cancel, Escape,
/// another dialog, its document no longer active) leaves its fields remembered for its image, and
/// its runs stop.
pub(crate) fn poll(app: &mut VectorcraftApp) {
    let open = app.ui.dialog.as_ref().is_some_and(|d| d.kind == KIND);
    if open {
        return;
    }
    let Some(mut live) = app.embroidery.image_window.live.take() else { return };
    live.schedule.stop();
    if !live.created {
        live.fields.remove(STATE);
        app.embroidery.image_window.remembered.insert((live.uid, live.image), live.fields);
        if app.ui.status == crate::et!("Digitize Image: live preview") {
            app.status(String::new());
        }
    }
}

/// Whether a window capture should wait: the window is computing.
pub(crate) fn busy(app: &VectorcraftApp) -> bool {
    app.embroidery.image_window.live.as_ref().is_some_and(|l| l.schedule.busy())
}

#[cfg(test)]
mod tests;
