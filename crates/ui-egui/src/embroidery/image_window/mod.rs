//! Digitize Image…: the large window over the document that digitizes the selected image live
//! (FORK.md's work item 9; the project owner's decisions of 2026-11-11, `docs/embroidery.md` ›
//! Digitizing from an image).
//!
//! - **One of upstream's dialogs** (a row of the dialog registry that draws its own window): agents
//!   reach it with `ui.inspect`, `ui.dialog.set` and `ui.dialog.confirm` (Create), Escape closes it
//!   (a popup open in it first), and another dialog opened (Document Setup) takes its place: there
//!   is one dialog at a time, so the window closes, its settings remembered. Its fields are small
//!   JSON: the engine's settings ([`SETTING_FIELDS`]), the palette its threads are matched to, and
//!   what it shows; `__state` says how its result stands.
//! - **Opened** by Digitize Image… (Object › Image Trace, the command palette, the Embroidery
//!   panel's Object tab) on the image selected, or an Image Trace object, with its defaults ready:
//!   Create is one click. A refusal (no image, a locked one, a missing linked file) is said in the
//!   status bar and opens nothing.
//! - **Live**: digitizing's stages run off the interface thread ([`worker`]) when a setting
//!   changes, by the stitch preview's rules ([`schedule`]); each stage shows as soon as it is
//!   done.
//! - **Remembered for the session** (the owner's choice): Cancel, Escape or another document made
//!   active close it keeping that image's settings; Digitize Image… on the same image continues (the
//!   same picture: another given its id after an Undo starts from the defaults); Start Over goes
//!   back to the defaults. Create forgets them.
//! - **Create** applies the run shown when it is of the settings as they are (at once), else when
//!   it is (the window says so); then the window closes, what was made selected. Enter only commits
//!   the field typed in; Cmd/Ctrl+Enter is Create, the field being typed in committed first as
//!   Enter would, a value it refuses making nothing (the project owner's choice, 2026-11-11).
//! - **Keep as Live Object** makes the same as a live object, the image kept in it, which
//!   Digitize Image… opens again with the settings it was made with (the window's remembered ones
//!   first, in a session). On a live object whose objects were changed by hand, both ask first in
//!   the Live bar (the project owner's choice, 2026-11-16, „Zastąpić, pytając najpierw (polecam)”).
//!   Agents keep one with the field `keep: "live"` and `ui.dialog.confirm`, which never asks.

mod answers;
pub(crate) mod colour_list;
mod colours_step;
pub(crate) mod frame;
mod history;
mod image_step;
pub(crate) mod inspector;
pub(crate) mod live_bar;
pub(crate) mod preview;
pub(crate) mod schedule;
mod shapes_step;
pub(crate) mod steps;
mod stitches_step;
mod words;
pub(crate) mod worker;

use std::collections::HashMap;

use egui::{Key, Modifiers};
use serde_json::{Map, Value, json};
use vectorcraft_engine::doc::{Document, ImageObject, NodeId, NodeKind};
use vectorcraft_geom::Affine;

use self::schedule::{Freshness, PROGRESS_REPAINT_SECS, Runner, Schedule};
use self::worker::Driver;
use crate::VectorcraftApp;
use crate::dialogs::{DialogResult, DialogSpec};
use crate::state::Dialog;

/// The dialog's kind: the command it runs.
pub const KIND: &str = "embroidery.digitizeImage";

/// The fields that are the engine's settings (`embroidery.digitizeImage`'s), in its names: the
/// image's size, crop and background; the colours' mode, count, choices and smallest area; the
/// shapes' smoothness, corners, straightening, overlap and thin lines; the outlines, the border's
/// width and their thread; the changes to single areas (the inspector's). The document settings the
/// Stitches step changes are fields of their own
/// (`stitches_step::DOCUMENT_FIELDS`).
pub(crate) const SETTING_FIELDS: [&str; 19] = [
    "width",
    "height",
    "fit",
    "crop",
    "background",
    "mode",
    "colors",
    "minArea",
    "colorChoices",
    "smoothing",
    "corners",
    "straightLines",
    "overlap",
    "thinLine",
    "colorOutlines",
    "border",
    "borderWidth",
    "outlineThread",
    "areaChanges",
];

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
pub(crate) const FIRST_STEP: u64 = 2;
const FIRST_VIEW: &str = "compare";

/// The read-only field saying how the result stands, for agents (`ui.inspect`).
const STATE: &str = "__state";

/// The field agents set to keep a live object on `ui.dialog.confirm`, and its one value (note 09
/// › The window).
const KEEP: &str = "keep";
const KEEP_LIVE: &str = "live";

/// What `ui.dialog.set` takes though the window may not hold it yet
/// (`crate::dialogs::accepts_field`): `keep`, the palette, the engine's settings and the document
/// settings the Stitches step changes.
const INPUTS: [&str; 2 + SETTING_FIELDS.len() + stitches_step::DOCUMENT_FIELDS.len()] = {
    let mut all = [KEEP; 2 + SETTING_FIELDS.len() + stitches_step::DOCUMENT_FIELDS.len()];
    all[1] = PALETTE;
    let mut k = 0;
    while k < SETTING_FIELDS.len() {
        all[2 + k] = SETTING_FIELDS[k];
        k += 1;
    }
    let mut d = 0;
    while d < stitches_step::DOCUMENT_FIELDS.len() {
        all[2 + SETTING_FIELDS.len() + d] = stitches_step::DOCUMENT_FIELDS[d];
        d += 1;
    }
    all
};

/// The window's row of the dialog registry: a window the fork's code draws (as upstream's
/// `DialogSpec::window` makes one), whose fields `ui.dialog.set` takes besides those it holds
/// ([`INPUTS`]).
pub(crate) const SPEC: DialogSpec = DialogSpec { window: Some(show), confirm, inputs: |_| &INPUTS, ..DialogSpec::FORM };

/// The field a window remembered for the session keeps its image's transform in, as it was left
/// (six numbers; never a setting): its changes' angles, page degrees, turn with the image when it
/// is turned on the canvas before the window opens again.
const PLACED: &str = "__placed";

/// What Create makes: plain objects (Create Embroidery Objects), or a live object that Digitize
/// Image… changes again (Keep as Live Object).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Making {
    Objects,
    LiveObject,
}

/// The question the Live bar asks before a live object's objects changed by hand are replaced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Question {
    /// What its Replace makes.
    pub(crate) making: Making,
    /// When it showed (the frame's clock, seconds).
    pub(crate) shown: f64,
    /// Whether its buttons answer yet: only once the double-click time has passed with no mouse
    /// button, Enter or Space held (what presses a button), so that neither the second click of a
    /// double-click on Create nor a held Enter answers it.
    pub(crate) armed: bool,
    /// It has just shown: its safe answer, Don't Replace, takes the keyboard.
    pub(crate) focus: bool,
}

/// The settings a window closed without Create was left with, for the session, and the picture
/// they are of.
struct Remembered {
    fields: Map<String, Value>,
    /// Its image's pixels as its document held them: an object given the same id after an Undo, of
    /// another picture, doesn't take them. Held weakly, so that no other pixels take their place
    /// in memory while it is kept.
    pixels: std::sync::Weak<Vec<u8>>,
}

/// The image window's state in the app: each image's settings as its window was left, and the
/// window open.
#[derive(Default)]
pub struct State {
    /// By document and image: the settings of a window closed without Create, for the session.
    remembered: HashMap<(u64, NodeId), Remembered>,
    pub(crate) live: Option<Live>,
    /// The window stepped aside for Document Setup (Embroidery Settings…): its document and image,
    /// to come back to when no dialog is open any more.
    back_to: Option<(u64, NodeId)>,
}

/// The window open: what it is of, where its runs go and when, and what it was left with.
pub(crate) struct Live {
    uid: u64,
    image: NodeId,
    pub(crate) runner: Box<dyn Runner>,
    pub(crate) schedule: Schedule,
    /// The preview's textures, pan and the area under the pointer.
    pub(crate) preview: preview::Preview,
    /// The colour list's selection and its thread choice.
    pub(crate) colours: colour_list::State,
    /// The area the inspector is open on.
    pub(crate) inspector: Option<inspector::Inspected>,
    /// Where the inspector's satin answers are worked out: off the interface thread where there can
    /// be a thread.
    pub(crate) answers: answers::Answers,
    /// Undo inside the window.
    pub(crate) history: history::History,
    /// The figures last shown: kept, dimmed, while a newer result is on its way or the settings
    /// can't be digitized.
    figures: Option<crate::embroidery::counters::Figures>,
    /// What screen readers were last told of the result, once it settled.
    pub(crate) announced: String,
    /// The command's reply for the result shown, built once a result.
    reply: Option<(ReplyOf, Value)>,
    /// The document's embroidery settings as the result shown was planned with them (by the run
    /// that made its objects), built once a result.
    document: Option<(Option<u64>, vectorcraft_engine::cmd::embroidery::setup::DocumentSettings)>,
    /// The fabric presets' ids, listed once the Stitches step first shows them.
    fabrics: Option<Vec<String>>,
    /// The document's own embroidery settings, by the snapshot of the document they were read of:
    /// read again only when the document changes (reading them walks all of it).
    own: Option<(std::sync::Arc<vectorcraft_engine::doc::Document>, vectorcraft_engine::cmd::embroidery::setup::DocumentSettings)>,
    /// The fields as last drawn: what is remembered when it closes without Create.
    fields: Map<String, Value>,
    /// Create was asked for before the result was of the settings as they are: it applies once it
    /// is.
    pub(crate) create_waiting: bool,
    /// What the Create asked for makes.
    pub(crate) making: Making,
    /// The Live bar asks before objects changed by hand are replaced.
    pub(crate) question: Option<Question>,
    /// The question just let go: the keyboard goes back to the button that asked.
    pub(crate) refocus: Option<Making>,
    /// The window closed by Create: nothing to remember.
    created: bool,
    /// A value typed was refused in this frame (the status bar says why): a Create asked for in it
    /// makes nothing.
    refused: bool,
}

/// What a reply was built of: the numbers of its shapes and of its plan.
type ReplyOf = (Option<u64>, Option<u64>);

impl Live {
    /// The figures to show: `said`'s when it has some (kept for later), else the last shown.
    pub(crate) fn last_figures(&mut self, said: Option<&live_bar::Said>) -> Option<crate::embroidery::counters::Figures> {
        if let Some(live_bar::Said::Figures(f)) = said {
            self.figures = Some(f.clone());
        }
        self.figures.clone()
    }

    /// The command's reply for the result shown (its areas listed none), once its shapes are done:
    /// what the window's lists and figures read.
    pub(crate) fn reply(&mut self) -> Option<&Value> {
        use vectorcraft_engine::cmd::embroidery::DigitizeStage;
        let shown = self.schedule.shown.as_ref()?;
        let key = (shown.outputs.number(DigitizeStage::Shapes), shown.outputs.number(DigitizeStage::Plan));
        if self.reply.as_ref().is_none_or(|(k, _)| *k != key) {
            let reply = shown.outputs.reply(&shown.request)?;
            self.reply = Some((key, reply));
        }
        self.reply.as_ref().map(|(_, r)| r)
    }

    /// The document's embroidery settings as the result shown is planned with them (the window's
    /// palette and document settings in place), each with where it comes from.
    pub(crate) fn document_settings(&mut self) -> Option<&vectorcraft_engine::cmd::embroidery::setup::DocumentSettings> {
        use vectorcraft_engine::cmd::embroidery::DigitizeStage;
        let shown = self.schedule.shown.as_ref()?;
        let key = shown.outputs.number(DigitizeStage::Objects);
        if self.document.as_ref().is_none_or(|(k, _)| *k != key) {
            self.document = Some((key, shown.request.document_settings()));
        }
        self.document.as_ref().map(|(_, s)| s)
    }

    /// The active document's own embroidery settings, read again only when the document changed
    /// since they were last read.
    pub(crate) fn own_settings(
        &mut self,
        session: &vectorcraft_engine::Session,
    ) -> Option<&vectorcraft_engine::cmd::embroidery::setup::DocumentSettings> {
        let doc = session.active()?.doc.clone();
        if self.own.as_ref().is_none_or(|(d, _)| !std::sync::Arc::ptr_eq(d, &doc)) {
            let settings = session.embroidery_settings().ok()?;
            self.own = Some((doc, settings));
        }
        self.own.as_ref().map(|(_, s)| s)
    }

    /// The fabric presets' ids, as `embroidery.fabrics` lists them (none when it fails: the
    /// built-in values alone are offered then).
    pub(crate) fn fabric_presets(&mut self, session: &mut vectorcraft_engine::Session) -> Vec<String> {
        self.fabrics
            .get_or_insert_with(|| {
                let listed = session.execute("embroidery.fabrics", &json!({})).unwrap_or(Value::Null);
                listed["fabrics"].as_array().into_iter().flatten().filter_map(|f| f["id"].as_str().map(str::to_string)).collect()
            })
            .clone()
    }
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
/// object), continuing where that image's window was left in this session; a live object with the
/// settings it was made with. A refusal is said in the status bar.
pub fn open(app: &mut VectorcraftApp, id: Option<NodeId>) {
    let Some(st) = app.session.active() else { return };
    let uid = st.uid;
    let picked = match (id, st.selection.objects.as_slice()) {
        (Some(id), _) => id,
        (None, [one]) => *one,
        _ => return app.status(crate::et!("Select one image to digitize it.")),
    };
    // The image an Image Trace object (a live object too) holds stands for that object, as the
    // command takes it: the window is that object's, its settings and its question about hand edits.
    let picked = vectorcraft_engine::cmd::embroidery::digitizing_target(&st.doc, picked);
    // Refused as the command refuses (not an image, locked, a missing linked file).
    if let Err(e) = app.session.digitize_window_request(picked, &json!({}), None, &Value::Null) {
        return app.status(crate::i18n::msg(&e.to_string()));
    }
    // Its settings as its window was left, when they are of the picture it shows now.
    let pixels = pixels_of(app, uid, picked);
    let remembered = app.embroidery.image_window.remembered.get(&(uid, picked));
    let of_it = remembered.filter(|r| pixels.as_ref().is_some_and(|p| std::ptr::eq(r.pixels.as_ptr(), std::sync::Arc::as_ptr(p))));
    let (fields, said) = match of_it.map(|r| r.fields.clone()) {
        Some(fields) => (turned_since(fields, placed(app, uid, picked)), None),
        None => {
            app.embroidery.image_window.remembered.remove(&(uid, picked));
            first_fields(app, uid, picked)
        }
    };
    app.embroidery.image_window.live = None;
    app.ui.dialog = Some(Dialog::new(KIND, Value::Object(fields)));
    app.status(said.unwrap_or_else(|| crate::et!("Digitize Image: live preview").to_string()));
}

/// The fields of a window opened on `id` with nothing remembered: a live object's settings over
/// the defaults (its changes' angles as it lies now), and what the status bar says of them; an
/// image's defaults.
///
/// - Made in another thread palette than the document's now (Document Setup changed it since): its
///   threads are let go and its colours held to the palette's threads, the rest kept, as after
///   Document Setup's OK (the project owner's choice of 2026-11-15), and the status bar says so.
/// - Refused even so (a newer version's settings, or a file edited by hand): the defaults, and why.
fn first_fields(app: &VectorcraftApp, uid: u64, id: NodeId) -> (Map<String, Value>, Option<String>) {
    let mut fields = defaults(app, uid, id);
    let kept = app.session.active().and_then(|st| st.doc.node(id)).and_then(vectorcraft_engine::cmd::embroidery::live_settings);
    let Some(mut kept) = kept else { return (fields, None) };
    let readable = |k: &Map<String, Value>| app.session.digitize_window_request(id, &Value::Object(k.clone()), None, &Value::Null).is_ok();
    let mut said = None;
    if !readable(&kept) {
        colour_list::drop_threads(&mut kept);
        let palette = fields.get(PALETTE).and_then(Value::as_str).unwrap_or_default();
        let most = vectorcraft_engine::cmd::embroidery::thread_palettes().into_iter().find(|(p, _)| *p == palette).map(|(_, n)| n as u64);
        if let (Some(most), Some(n)) = (most, kept.get(COLORS).and_then(Value::as_u64))
            && n > most
        {
            kept.insert(COLORS.into(), json!(most));
        }
        if !readable(&kept) {
            return (fields, Some(crate::et!("This live object's settings couldn't be read: the window starts from the defaults.").to_string()));
        }
        said = Some(
            crate::et!("This live object was made in another thread palette: its colors are matched to the document's palette again.").to_string(),
        );
    }
    fields.extend(kept);
    (fields, said)
}

/// The number of colours' field.
const COLORS: &str = "colors";

/// The transform image `id` of document `uid` is placed with: an image's own, or that of the image
/// first in an Image Trace object or a live object. Read in that document, which needn't be the
/// active one (the window closes when another becomes active).
fn placed(app: &VectorcraftApp, uid: u64, id: NodeId) -> Option<Affine> {
    picture_of(app, uid, id).map(|(_, image)| image.xf)
}

/// The pixels of the image a window on `id` of document `uid` digitizes, as that document holds
/// them: what tells one picture from another given the same id.
fn pixels_of(app: &VectorcraftApp, uid: u64, id: NodeId) -> Option<std::sync::Arc<Vec<u8>>> {
    let (doc, image) = picture_of(app, uid, id)?;
    doc.image_blob(image).map(|blob| blob.bytes.clone())
}

/// The image a window on `id` of document `uid` digitizes (it, or the one an Image Trace object
/// holds first), and that document; `None` when either is gone.
fn picture_of(app: &VectorcraftApp, uid: u64, id: NodeId) -> Option<(&Document, &ImageObject)> {
    let doc: &Document = &app.session.documents().iter().find(|st| st.uid == uid)?.doc;
    let n = doc.node(id)?;
    let image = match &n.kind {
        NodeKind::Image(i) => i,
        _ => match &n.children()?.first()?.kind {
            NodeKind::Image(i) => i,
            _ => return None,
        },
    };
    Some((doc, image))
}

/// Remembered fields `fields`, their changes' angles turned with the image since the window was
/// left (`now`, its transform now; the one it was left with is their [`PLACED`]): a page angle set
/// then makes the same angle with the art now.
fn turned_since(mut fields: Map<String, Value>, now: Option<Affine>) -> Map<String, Value> {
    let then = fields.remove(PLACED).and_then(|v| {
        let c: Vec<f64> = v.as_array()?.iter().map(Value::as_f64).collect::<Option<_>>()?;
        Some(Affine::new(<[f64; 6]>::try_from(c).ok()?))
    });
    let (Some(then), Some(now)) = (then, now) else { return fields };
    let d = then.determinant();
    if then == now || !(d.is_finite() && d.abs() > f64::EPSILON) {
        return fields;
    }
    let since = now * then.inverse();
    if let Some(Value::Array(changes)) = fields.get_mut(inspector::CHANGES) {
        for c in changes.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(a) = c.get("angle").and_then(Value::as_f64) {
                c.insert("angle".into(), json!(vectorcraft_engine::doc::embroidery::turned_angle(a, since)));
            }
        }
    }
    fields
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

/// The engine's settings, the palette and the document settings the fields hold: what a change is
/// measured on.
pub(crate) fn params_of(fields: &Map<String, Value>) -> Value {
    let mut p = Map::new();
    for k in SETTING_FIELDS.iter().chain([&PALETTE]).chain(stitches_step::DOCUMENT_FIELDS.iter()) {
        if let Some(v) = fields.get(*k).filter(|v| !v.is_null()) {
            p.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(p)
}

/// `params` without the palette and the document settings: the settings `embroidery.digitizeImage`
/// takes.
pub(crate) fn settings_of(params: &Value) -> Value {
    let mut p = params.clone();
    if let Some(o) = p.as_object_mut() {
        o.remove(PALETTE);
        for k in stitches_step::DOCUMENT_FIELDS {
            o.remove(k);
        }
    }
    p
}

/// The document settings `params` change, as `embroidery.setup` takes them: what the window's
/// request carries besides the command's settings.
pub(crate) fn document_of(params: &Value) -> Value {
    stitches_step::document_of(params)
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
        let live = Live {
            uid,
            image,
            runner,
            schedule: Schedule::default(),
            preview: preview::Preview::default(),
            colours: colour_list::State::default(),
            inspector: None,
            answers: answers::Answers::new(ctx),
            history: history::History::default(),
            figures: None,
            announced: String::new(),
            reply: None,
            document: None,
            fabrics: None,
            own: None,
            fields: d.fields.clone(),
            create_waiting: false,
            making: Making::Objects,
            question: None,
            refocus: None,
            created: false,
            refused: false,
        };
        app.embroidery.image_window.live = Some(live);
    }
    // The question before replacing hand edits holds egui's one open popup, which dialogs leave
    // Escape to: Escape, or another popup opened since (an area's inspector), answers it as its
    // Cancel would.
    if let Some(live) = app.embroidery.image_window.live.as_mut()
        && live.question.is_some()
    {
        if !egui::Popup::is_id_open(ctx, live_bar::question_id()) || ctx.input(|i| i.key_pressed(Key::Escape)) {
            dismiss_question(ctx, live);
        } else {
            egui::Popup::open_id(ctx, live_bar::question_id());
        }
    }
    // Undo inside the window: Shift+Cmd/Ctrl+Z, then Cmd/Ctrl+Z (which egui matches with Shift
    // too), unless a field being typed in takes the keys for its own undo (a focused button or the
    // preview doesn't).
    if !ctx.text_edit_focused()
        && let Some(live) = app.embroidery.image_window.live.as_mut()
    {
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
            live.history.redo(&mut d.fields);
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
            live.history.undo(&mut d.fields);
        }
    }
    let params = params_of(&d.fields);
    if let Some(live) = app.embroidery.image_window.live.as_mut() {
        live.schedule.frame(&app.session, image, &params, now, live.runner.as_mut());
        live.refused = false;
    }
    let create_now = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    if create_now {
        // The field being typed in commits first, as Enter would: a field ends its editing when it
        // loses the focus, in this frame's drawing (upstream's `recessed_text`).
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
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
        Some(frame::Asked::Create) => asked_to_make(app, ctx, &d.fields, now, Making::Objects),
        Some(frame::Asked::KeepLive) => asked_to_make(app, ctx, &d.fields, now, Making::LiveObject),
        Some(frame::Asked::Replace) => {
            let making = app.embroidery.image_window.live.as_mut().and_then(|l| {
                let making = l.question.map(|q| q.making);
                dismiss_question(ctx, l);
                l.refocus = None;
                making
            });
            if let Some(making) = making {
                ask_create(app, &d.fields, now, making);
            }
        }
        Some(frame::Asked::KeepEdits) => {
            if let Some(live) = app.embroidery.image_window.live.as_mut() {
                dismiss_question(ctx, live);
            }
        }
        None if create_now => asked_to_make(app, ctx, &d.fields, now, Making::Objects),
        None => {}
    }
    // The settings as this frame drew them: a field may have committed a value in it.
    let params = params_of(&d.fields);
    // A Create waiting is settled once the run of the settings as they are has been: made, or
    // refused with the reason (settings the engine refuses make nothing, never the result of the
    // ones before).
    if let Some(live) = app.embroidery.image_window.live.as_mut()
        && live.create_waiting
        && live.schedule.asked_for(&params)
        && !live.schedule.busy()
    {
        live.create_waiting = false;
        let making = live.making;
        match live_bar::create_refused(live, now) {
            Some(why) => app.status(why),
            None => match create(app, &params, making) {
                Ok(_) => return,
                Err(e) => app.status(e),
            },
        }
    }
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    d.fields.insert(STATE.into(), state_json(live, now));
    live.history.note(&d.fields, ctx.input(|i| i.pointer.any_down()));
    live.fields = d.fields.clone();
    if live.schedule.busy() {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(PROGRESS_REPAINT_SECS));
    }
    if app.ui.dialog.as_ref().is_some_and(|open| open.kind == KIND) {
        app.ui.dialog = Some(d);
    }
}

/// Create or Keep as Live Object asked for (`making`; the buttons, Cmd/Ctrl+Enter) with the
/// window's fields as this frame drew them: on a live object whose objects were changed by hand,
/// the Live bar asks first (the project owner's choice, 2026-11-16, „Zastąpić, pytając najpierw
/// (polecam)”: Undo brings the edits back, as for Restore Object); else as [`ask_create`].
fn asked_to_make(app: &mut VectorcraftApp, ctx: &egui::Context, fields: &Map<String, Value>, now: f64, making: Making) {
    // A value typed refused in this frame makes nothing; a question showing is answered by its own
    // buttons (Cmd/Ctrl+Enter doesn't turn Keep as Live Object's into Create's).
    let Some(image) = app.embroidery.image_window.live.as_ref().filter(|l| !l.refused && l.question.is_none()).map(|l| l.image) else { return };
    // Asked once, at the click: it walks all of the live object's points.
    let edited = app.session.active().and_then(|st| st.doc.node(image)).and_then(vectorcraft_engine::cmd::embroidery::live_edited);
    if edited != Some(true) {
        return ask_create(app, fields, now, making);
    }
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    // Refused at once only by a result of the settings as they are: one of a change still running is
    // judged once it is done, after Replace.
    let params = params_of(fields);
    if !live.schedule.busy()
        && live.schedule.current(&params).is_some()
        && let Some(why) = live_bar::create_refused(live, now)
    {
        return app.status(why);
    }
    live.question = Some(Question { making, shown: now, armed: false, focus: true });
    live.create_waiting = false;
    egui::Popup::open_id(ctx, live_bar::question_id());
}

/// The question before replacing hand edits gone, nothing made: the keyboard goes back to the
/// button that asked.
fn dismiss_question(ctx: &egui::Context, live: &mut Live) {
    live.refocus = live.question.take().map(|q| q.making);
    egui::Popup::close_id(ctx, live_bar::question_id());
}

/// Create (`making`: plain objects or a live object) asked for with the window's fields as this
/// frame drew them, edits replaced if any: nothing when a value typed in it was refused (said
/// already); once their run is done when they haven't run yet or are running (a field committed by
/// this Create, a change waiting); else at once, or refused with the reason.
fn ask_create(app: &mut VectorcraftApp, fields: &Map<String, Value>, now: f64, making: Making) {
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    if live.refused {
        return;
    }
    live.making = making;
    let params = params_of(fields);
    if live.schedule.busy() || !live.schedule.asked_for(&params) {
        live.create_waiting = true;
        return;
    }
    if let Some(why) = live_bar::create_refused(live, now) {
        return app.status(why);
    }
    if live.schedule.current(&params).is_none() {
        live.create_waiting = true;
    } else if let Err(e) = create(app, &params, making) {
        app.status(e);
    }
}

/// A value typed refused: said in the status bar as `words`, and a Create asked for in the same
/// frame (Cmd/Ctrl+Enter committing the field) makes nothing.
pub(crate) fn refuse(app: &mut VectorcraftApp, words: String) {
    if let Some(live) = app.embroidery.image_window.live.as_mut() {
        live.refused = true;
    }
    app.status(words);
}

/// Create of the run shown for settings `params`, as plain objects or a live object (`making`):
/// applied as one undo step, the window closed, what was made selected and said in the status bar.
fn create(app: &mut VectorcraftApp, params: &Value, making: Making) -> DialogResult {
    let Some(live) = app.embroidery.image_window.live.as_ref() else { return Err("the image window isn't open".into()) };
    let Some(shown) = live.schedule.current(params).cloned() else { return Err(crate::et!("The preview isn't ready yet.").to_string()) };
    let key = (live.uid, live.image);
    let reply = app
        .session
        .apply_digitize_window(&shown.request, &shown.outputs, making == Making::LiveObject)
        .map_err(|e| crate::i18n::msg(&e.to_string()))?;
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
    let threads = reply["stats"]["threadCount"].as_u64().unwrap_or(0);
    if reply["created"]["live"].as_bool() == Some(true) {
        return app.status(live_bar::kept_live_words(made, threads));
    }
    let layer = reply["created"]["layer"].as_u64().map(NodeId);
    let name = app.session.active().and_then(|st| layer.and_then(|l| st.doc.node(l))).and_then(|n| n.name.clone()).unwrap_or_default();
    app.status(live_bar::created_words(made, threads, &name));
}

/// `ui.dialog.confirm`: Create, in one go (a live object with the field `keep: "live"`, never
/// asking about hand edits: an agent reads them in the command's dry run, `edited`): the run shown
/// when it is of the fields as they are, else a run made now on the interface thread, as an agent
/// asks for it.
pub(crate) fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> DialogResult {
    let Some((uid, image)) = of(d) else { return Err("the image window names no image".into()) };
    let making = match d.fields.get(KEEP) {
        None | Some(Value::Null) => Making::Objects,
        Some(v) if v.as_str() == Some(KEEP_LIVE) => Making::LiveObject,
        Some(v) => return Err(format!("{KEEP}: \"{KEEP_LIVE}\" or nothing, not {v}")),
    };
    let params = params_of(&d.fields);
    if app.embroidery.image_window.live.as_ref().is_some_and(|l| l.schedule.current(&params).is_some()) {
        return create(app, &params, making);
    }
    let palette = params.get(PALETTE).and_then(Value::as_str).map(str::to_string);
    let request = app
        .session
        .digitize_window_request(image, &settings_of(&params), palette.as_deref(), &document_of(&params))
        .map_err(|e| crate::i18n::msg(&e.to_string()))?;
    let outputs = app.session.digitize_pipeline().run(&request, &|_, _| true, &mut |_, _, _| {}).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    let reply = app.session.apply_digitize_window(&request, &outputs, making == Making::LiveObject).map_err(|e| crate::i18n::msg(&e.to_string()))?;
    closed_by_create(app, (uid, image), &reply);
    Ok(reply)
}

/// Embroidery Settings…: the window steps aside for Document Setup's Embroidery tab (there is one
/// dialog at a time), its settings and its kept stages waiting, and comes back when no dialog is
/// open any more, after OK or Cancel, recomputing only what the settings changed (the project
/// owner's choice, 2026-11-12, „Okno ustąpi i wróci (polecam)”). Document Setup opens showing what
/// the window plans with, its fields' (`fields`) own palette, fabric, fill angle and joining over
/// the document's (the project owner's choice, 2026-11-15, „Jedne wartości (polecam)”).
pub(crate) fn step_aside(app: &mut VectorcraftApp, fields: &Map<String, Value>) {
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    // A Create waiting for its run is let go: it was asked of the settings before the person went
    // to change them, and would otherwise make the design behind their back once they are back.
    live.create_waiting = false;
    let key = (live.uid, live.image);
    app.embroidery.image_window.back_to = Some(key);
    app.embroidery.setup_confirmed = false;
    let own: Map<String, Value> =
        [PALETTE].into_iter().chain(stitches_step::DOCUMENT_FIELDS).filter_map(|k| fields.get(k).map(|v| (k.to_string(), v.clone()))).collect();
    if let Err(e) = crate::embroidery::setup_tab::open_at_tab_with(app, &own) {
        app.embroidery.image_window.back_to = None;
        app.status(e);
    }
}

/// Each frame, before the window is drawn: a window that stepped aside for Document Setup waits
/// while a dialog is open and comes back once none is, as it was left; a window that closed without
/// Create (Cancel, Escape, another dialog, its document no longer active) leaves its fields
/// remembered for its image, and its runs stop.
pub(crate) fn poll(app: &mut VectorcraftApp) {
    let open = app.ui.dialog.as_ref().is_some_and(|d| d.kind == KIND);
    if open {
        return;
    }
    if let Some(back) = app.embroidery.image_window.back_to {
        let waiting = app.embroidery.image_window.live.as_ref().is_some_and(|l| (l.uid, l.image) == back)
            && app.session.active().map(|st| st.uid) == Some(back.0);
        if waiting && app.ui.dialog.is_some() {
            return;
        }
        app.embroidery.image_window.back_to = None;
        if let Some(live) = app.embroidery.image_window.live.as_ref().filter(|_| waiting) {
            // Back where it was, its fields as it left them; its runs kept, so the next frame
            // recomputes only from what the settings changed.
            let mut fields = live.fields.clone();
            fields.remove(STATE);
            // After Document Setup's OK what it held is the document's: the window lets go of its
            // own values of those settings and plans with the document's, matching its threads to
            // the document's palette (the project owner's choice, 2026-11-15, „Jedne wartości
            // (polecam)”). After Cancel they stay the window's.
            if std::mem::take(&mut app.embroidery.setup_confirmed) {
                for k in stitches_step::DOCUMENT_FIELDS {
                    fields.remove(k);
                }
                let palette = app.session.active().map(|st| vectorcraft_engine::cmd::embroidery::thread_palette_id(&st.doc));
                if let Some(palette) = palette.filter(|p| fields.get(PALETTE).and_then(Value::as_str) != Some(*p)) {
                    fields.insert(PALETTE.into(), json!(palette));
                    colour_list::drop_threads(&mut fields);
                }
            }
            app.ui.dialog = Some(Dialog::new(KIND, Value::Object(fields)));
            return;
        }
    }
    let Some(mut live) = app.embroidery.image_window.live.take() else { return };
    live.schedule.stop();
    if !live.created {
        live.fields.remove(STATE);
        // An agent's `keep` was for that Create alone.
        live.fields.remove(KEEP);
        live.fields.insert(PLACED.into(), json!(placed(app, live.uid, live.image).map(|xf| xf.as_coeffs())));
        let pixels = pixels_of(app, live.uid, live.image).map_or_else(std::sync::Weak::new, |p| std::sync::Arc::downgrade(&p));
        app.embroidery.image_window.remembered.insert((live.uid, live.image), Remembered { fields: live.fields, pixels });
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
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_inspector;
#[cfg(test)]
mod tests_live;
