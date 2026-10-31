//! The Embroidery panel's Object tab: how the selected objects are sewn, and their embroidery
//! settings, part by part (all their fills, all their strokes).
//!
//! - **It never needs a plan.** Its settings come from `embroidery.info` read without planning
//!   (`Session::embroidery_info`), once per document snapshot and selection, so the fields change
//!   at once. Only the stitch count in its header comes from a plan, the one the stitch preview
//!   shows (counted, never planned, on the interface thread): "…" before the first, dimmed while a
//!   newer one is made, the last count kept when the design can't be planned. So a part set not to
//!   be sewn, or a design too large to plan, can always be set back.
//! - **States**: nothing selected; objects that take no settings (type, an image, a symbol
//!   instance, stitches from a file…, each saying why); one object or several, their values merged
//!   (those they share; else blank, and how many set it).
//! - **Rows**: the settings a person sets on an object most (the product plan's subset: the stitch,
//!   its angle, spacing and length, narrow parts as satin; the underlay; the pull compensation and
//!   the expansion), each where the stitch its part is sewn in uses it; another setting an object
//!   holds (set by a command) shows too, so that nothing set is hidden. Then the entry and exit
//!   points, Trim After and Stop After, Convert to Manual Stitches, and a footer with the
//!   document's fabric and Restore from Fabric.
//! - **Manual stitches** (objects converted to them, or a run of one): what they are in words,
//!   their runs and needle points, how much they were scaled since converting, and Restore Object
//!   (asking first only when their stitches were edited since converting, the project owner's
//!   choice), then Trim After and Stop After.
//! - **Edits are commands**, one undo step each (a drag on a label one for the whole drag, the
//!   app's scrub): a value typed or chosen is `embroidery.set` (a choice equal to what the objects
//!   would inherit unsets theirs), ↺ `embroidery.reset`, Trim After `embroidery.setTrimAfter`,
//!   Stop After `embroidery.setStopAfter`, Convert `embroidery.convertToManual`, Restore
//!   `embroidery.restoreObject`. They go to the objects the fields were drawn for; converting and
//!   restoring say in the status bar what they did.

use std::sync::{Arc, Weak};

use egui::{Color32, CornerRadius, FontId, Sense, Ui, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId, NodeKind};
use vectorcraft_engine::cmd::embroidery::POINT_MISS_MM;
use vectorcraft_engine::cmd::embroidery::info::{KeyInfo, ObjectSettings, PartInfo, SelectionSewing, selection_sewing};
use vectorcraft_engine::cmd::embroidery::order::EntryExit;
use vectorcraft_engine::cmd::embroidery::setup::{FabricChoice, fabric_and_thread};
use vectorcraft_engine::cmd::embroidery::{SettingField, distinct_underlays, part_field};

use super::SHOW_BUSY_AFTER_SECS;
use crate::VectorcraftApp;
use crate::embroidery::rows::{self, Columns, Edit, Owned, OwnedField, Row, WORDS_DECIMALS, restore_words, text_width};
use crate::embroidery::stitch_preview::Freshness;
use crate::embroidery::{fields, numbers, text, vocabulary};
use crate::i18n::Lang;
use crate::theme::Tokens;

/// The words of the header's title and the tab's states, points: the panel's sizes.
const TITLE_TEXT: f32 = 13.0;
const SMALL_TEXT: f32 = 11.0;
/// The stitch count's pill: its words, its padding and corners, points.
const PILL_TEXT: f32 = 11.0;
const PILL_PAD: egui::Vec2 = vec2(7.0, 2.0);
const PILL_RADIUS: u8 = 9;
/// The part switch's height and the size of its words, points: as the tab strip's.
const SWITCH_HEIGHT: f32 = 22.0;
const SWITCH_TEXT: f32 = 12.0;
/// Room between the tab's blocks, points.
const GAP: f32 = 8.0;
/// The footer's padding and corners inside its card, and between its buttons, points.
const FOOTER_PAD: f32 = 8.0;
const FOOTER_RADIUS: u8 = 6;
/// The footer's first line, points: a point over the lines under it.
const FOOTER_LINE_TEXT: f32 = 12.0;
/// The least room the rows keep above the footer, points: two rows.
const MIN_ROWS: f32 = 56.0;
/// Degrees in a half turn: a fill's rows are lines, shown at an angle from 0 to 180°.
const HALF_TURN: f64 = 180.0;
/// How close to a half turn an angle counts as one, degrees: a millionth of a degree, far finer
/// than a field shows (the same rows).
const SAME_ANGLE: f64 = 1e-6;
/// Decimals of the thread's weight in the footer: whole weights, as thread is sold.
const WEIGHT_DECIMALS: usize = 0;
/// The places a point's miss is written to: a tenth of a millimetre, the formats' grid.
const MISS_DECIMALS: usize = 1;
/// What the entry and exit rows ask for when pressed: the embroidery tool's point dialog on the
/// point (`embroidery/point_dialog.rs`), opened after the tab is drawn as its commands run.
const POINT_DIALOG: &str = vectorcraft_tools::embroidery::POINT_DIALOG;
/// The command the footer's Embroidery Settings… runs: Document Setup at its Embroidery tab.
const SETTINGS: &str = "embroidery.documentSetup";
/// The stitch count before the first plan: an ellipsis, the same in every language.
const PENDING: &str = "…";

/// The part of the selected objects the tab shows: the document's kind of part (its id stable:
/// `ui.embroideryPanel`, the saved interface state).
pub use vectorcraft_doc::embroidery::PartKind as Part;

/// What the tab reads of a part of the selection.
trait PartOf {
    /// The selection's part of this kind, when one of the objects has it.
    fn of(self, info: &ObjectSettings) -> Option<&PartInfo>;
    /// Whether some of the objects paint this part: a path keeps a fill and a stroke in its
    /// appearance even when one paints nothing ("Stroke: none"), and settings kept for it take
    /// effect once it paints.
    fn painted(self, info: &ObjectSettings) -> bool;
}

impl PartOf for Part {
    fn of(self, info: &ObjectSettings) -> Option<&PartInfo> {
        match self {
            Part::Fill => info.fill.as_ref(),
            Part::Stroke => info.stroke.as_ref(),
        }
    }

    fn painted(self, info: &ObjectSettings) -> bool {
        self.of(info).is_some_and(|p| p.reason.as_deref() != Some("unpainted"))
    }
}

/// The part shown: `chosen` when the objects paint it, else the other when they paint that, else
/// whichever they have.
fn shown(chosen: Part, info: &ObjectSettings) -> Option<Part> {
    let other = if chosen == Part::Fill { Part::Stroke } else { Part::Fill };
    let order = [chosen, other];
    order.into_iter().find(|p| p.painted(info)).or_else(|| order.into_iter().find(|p| p.of(info).is_some()))
}

/// The tab's state between frames.
#[derive(Default)]
pub(crate) struct ObjectState {
    /// The selection's settings, read for one document snapshot and selection.
    memo: Option<Memo>,
    /// The selection's stitches in a plan the preview showed.
    sewn: Option<Sewn>,
    /// What Copy took (a snapshot, pasted as it was then, into any document).
    copied: Option<Copied>,
    /// The button that takes the keyboard when it is drawn, for the objects it was asked for: the
    /// one that replaces Convert or Restore Object once they have done their work, when the
    /// keyboard pressed them (else nothing would have it, and the next Tab would hide the panels).
    focus_next: Option<(FocusNext, Vec<NodeId>)>,
    /// How many times the selection's settings were read (the memo missed).
    #[cfg(test)]
    reads: usize,
}

/// Which of the tab's buttons takes the keyboard next ([`ObjectState::focus_next`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusNext {
    Convert,
    Restore,
}

/// What Copy Embroidery Settings took: each part's settings of the object copied, as they were
/// then, by key (null: unset), as `embroidery.set` takes them; and the values this version
/// doesn't take (a newer version's), left out (`part.key`).
#[derive(Clone, Debug, PartialEq)]
struct Copied {
    parts: Vec<(Part, serde_json::Map<String, Value>)>,
    left_out: Vec<String>,
}

/// The selection's settings as read, shared from frame to frame, or why they can't be read.
type Read = Result<Arc<ObjectSettings>, String>;

/// The settings of objects `ids` of document `uid`'s snapshot `snapshot` (held weakly: a memo never
/// keeps a document alive, and its memory can't be reused for another snapshot while it is held).
struct Memo {
    uid: u64,
    snapshot: Weak<Document>,
    ids: Vec<NodeId>,
    info: Read,
}

/// What a plan of document `uid` (its `generation`) sews of objects `ids`.
struct Sewn {
    uid: u64,
    generation: u64,
    ids: Vec<NodeId>,
    sewing: SelectionSewing,
}

/// The settings of the active document's selection: the memo's, or read now. While a gesture is
/// under way (a drag on the canvas, a tool's preview: each frame a new snapshot whose settings it
/// doesn't change), the memo of the same objects is kept, and read again once it is committed or
/// cancelled; the tab's own scrub, an undo group, isn't one, so its fields follow each step.
fn settings_of(app: &mut VectorcraftApp) -> Option<(Vec<NodeId>, Read)> {
    let st = app.session.active()?;
    let (uid, snapshot, ids) = (st.uid, Arc::downgrade(&st.doc), st.selection.objects.clone());
    let gesture = st.interaction.is_some();
    let memo = &app.embroidery.object.memo;
    if let Some(m) = memo.as_ref().filter(|m| m.uid == uid && m.ids == ids && (gesture || Weak::ptr_eq(&m.snapshot, &snapshot))) {
        return Some((ids, m.info.clone()));
    }
    #[cfg(test)]
    {
        app.embroidery.object.reads += 1;
    }
    let info = app.session.embroidery_merged(&ids).map(Arc::new).map_err(|e| e.to_string());
    app.embroidery.object.memo = Some(Memo { uid, snapshot, ids: ids.clone(), info: info.clone() });
    Some((ids, info))
}

/// How the stitch count stands: the count of the plan shown (`None` before any), whether that plan
/// is of the document as it is, and whether the preview has said for a while that it plans.
struct Count {
    stitches: Option<usize>,
    fresh: bool,
    updating: bool,
    failed: Option<String>,
}

/// The selection's stitches in the plan the preview shows, counted once per plan and selection.
fn count(app: &mut VectorcraftApp, uid: u64, ids: &[NodeId], now: f64) -> Count {
    let preview = &app.embroidery.preview;
    let freshness = preview.freshness(uid, now);
    let updating = preview.out_of_date_for(uid, now).is_some_and(|s| s >= SHOW_BUSY_AFTER_SECS);
    let fresh = matches!(freshness, Freshness::Fresh);
    let failed = match freshness {
        Freshness::Failed { error } => Some(error),
        _ => None,
    };
    let Some(shown) = preview.shown(uid) else { return Count { stitches: None, fresh, updating, failed } };
    let generation = shown.planned.generation;
    let state = &mut app.embroidery.object;
    if !state.sewn.as_ref().is_some_and(|s| s.uid == uid && s.generation == generation && s.ids == ids) {
        let sewing = selection_sewing(&shown.document, &shown.planned.plan, ids);
        state.sewn = Some(Sewn { uid, generation, ids: ids.to_vec(), sewing });
    }
    Count { stitches: state.sewn.as_ref().map(|s| s.sewing.stitches()), fresh, updating, failed }
}

/// A command the tab runs after it is drawn.
type Command = (&'static str, Value);

/// The Object tab.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some((ids, info)) = settings_of(app) else { return };
    let now = ui.input(|i| i.time);
    let count = count(app, uid, &ids, now);
    let lang = crate::i18n::current();
    let mut commands: Vec<Command> = Vec::new();
    header(app, ui, &ids, info.as_ref().ok().map(|i| i.as_ref()), &count, lang);
    ui.add_space(GAP);
    let info = match info {
        Ok(info) => info,
        Err(e) => {
            ui.label(egui::RichText::new(crate::i18n::msg(&e)).size(SMALL_TEXT).color(Tokens::get(ui.ctx()).text));
            return;
        }
    };
    // Read from the document alone, on every frame: the fabric's name and the thread's weight.
    let (fabric, weight) = app.session.active().map(|st| fabric_and_thread(&st.doc)).unwrap_or((FabricChoice::BuiltIn, 0.0));
    let footer_words = footer_words(&fabric, weight, lang);
    let buttons = [super::command_label(app, SETTINGS), crate::et!("Restore from Fabric").to_string()];
    let footer_room = footer_height(ui, &footer_words, &buttons);
    if ids.is_empty() {
        state_words(ui, crate::et!("Select an object to set how it is sewn."));
    } else if info.object_count == 0 {
        takes_none(ui, &info);
        manual(ui, &info, &ids, lang, &mut commands, &mut app.embroidery.object.focus_next);
        trim_after(ui, &info, &ids, &mut commands);
        stop_after(ui, &info, &ids, &mut commands);
    } else {
        manual(ui, &info, &ids, lang, &mut commands, &mut app.embroidery.object.focus_next);
        let chosen = app.ui.embroidery_panel.part;
        let shown = shown(chosen, &info);
        if let Some(part) = part_switch(ui, &info, shown) {
            app.ui.embroidery_panel.part = part;
        }
        if let Some(part) = shown.and_then(|p| p.of(&info).map(|i| (p, i))) {
            // How far the stitches miss the part's entry and exit, from the plan shown when it is of
            // the document as it is.
            let sewn = app.embroidery.object.sewn.as_ref().filter(|s| count.fresh && s.uid == uid && s.ids == ids);
            let missed = sewn.map(|s| match part.0 {
                Part::Fill => s.sewing.fill.missed_by,
                Part::Stroke => s.sewing.stroke.missed_by,
            });
            // Where the plan shown starts and ends the one object's part: the point dialog opens there
            // on a point left to the planner.
            let planned = planned_ends(app, uid, &ids, part.0);
            let room = (ui.available_height() - footer_room).max(MIN_ROWS);
            // The panel's width, measured outside the scrolled rows: what they are fitted to, so that
            // nothing wider in them can widen the next frame's.
            let width = ui.available_width();
            let (label, busy) = (super::command_label(app, CONVERT), crate::embroidery::convert_job::running(app));
            let focus = &mut app.embroidery.object.focus_next;
            egui::ScrollArea::vertical().id_salt("embroidery-object-rows").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_max_width(width);
                part_rows(ui, part.0, part.1, &ids, width, lang, missed, planned, &mut commands);
                trim_after(ui, &info, &ids, &mut commands);
                stop_after(ui, &info, &ids, &mut commands);
                convert(ui, label, busy, &ids, &mut commands, focus);
            });
        }
    }
    footer(ui, &info, &footer_words, &buttons, &ids, &mut commands);
    if let Some(words) = fields::take_unreadable(ui) {
        app.status(words);
    }
    let selected = app.session.active().map(|st| st.selection.objects.clone()).unwrap_or_default();
    for (cmd, mut params) in commands {
        if cmd == POINT_DIALOG {
            crate::embroidery::point_dialog::open(app, &params);
            continue;
        }
        if cmd == CONVERT {
            // Off the interface thread: it plans the whole design afresh.
            for_selection(&mut params, &ids, &selected);
            crate::embroidery::convert_job::start(app, &params);
            continue;
        }
        if cmd == RESTORE {
            // Asking first when their stitches were edited since converting.
            let ids = params.get_mut("ids").map(Value::take).unwrap_or_default();
            crate::embroidery::menu::restore_asking(app, ids);
            continue;
        }
        for_selection(&mut params, &ids, &selected);
        if let Err(e) = app.run(cmd, params) {
            app.status(e);
        }
    }
}

/// The header: the selection's name and kind, and its stitches in the plan shown.
fn header(app: &VectorcraftApp, ui: &mut Ui, ids: &[NodeId], info: Option<&ObjectSettings>, count: &Count, lang: Lang) {
    let t = Tokens::get(ui.ctx());
    // Borrowed: a traced path's anchors would be copied on every repaint.
    let one = ids.first().filter(|_| ids.len() == 1).and_then(|id| app.session.active()?.doc.node(*id));
    let title = match (one, ids.len()) {
        (_, 0) => crate::et!("Nothing selected").to_string(),
        (Some(n), _) => crate::panels::layers::shown_name(app, n, lang),
        (None, n) => crate::etn!(n as u64, "{count} object", "{count} objects"),
    };
    let subtitle = match one {
        Some(n) => Some(kind_words(n, info)),
        None => info
            .filter(|i| i.skipped_count > 0 && i.object_count > 0)
            // Taking none isn't the same as not being sewn: a symbol's instance and stitches from a
            // file are sewn.
            .map(|i| crate::etn!(i.skipped_count as u64, "{count} selected object takes no settings", "{count} selected objects take no settings")),
    };
    ui.horizontal(|ui| {
        let pill = (!ids.is_empty()).then(|| pill_words(count));
        let room = pill.as_ref().map_or(0.0, |w| text_width(ui, w, PILL_TEXT) + 2.0 * PILL_PAD.x + GAP);
        ui.scope(|ui| {
            ui.set_max_width((ui.available_width() - room).max(0.0));
            ui.add(egui::Label::new(egui::RichText::new(&title).size(TITLE_TEXT).strong().color(t.text_strong)).truncate());
        });
        if let Some(words) = pill {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                pill_shape(ui, &words, !count.fresh);
            });
        }
    });
    // Dim words, moved as far as they need to read by 4.5:1 on the panel.
    let dim = crate::embroidery::marks::readable_text(t.text_dim, &[t.panel], t.dark);
    if let Some(words) = subtitle {
        ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL_TEXT).color(dim)).wrap());
    }
    if count.updating && !count.fresh {
        ui.label(egui::RichText::new(crate::et!("Updating…")).size(SMALL_TEXT).color(dim))
            .on_hover_text(crate::et!("These are the stitches before the last change; the new ones are being planned."));
    }
    if let Some(error) = &count.failed {
        super::failed(ui, error);
    }
}

/// The pill's words: the stitches, or [`PENDING`] before the first plan.
fn pill_words(count: &Count) -> String {
    match count.stitches {
        Some(n) => crate::etn!(n as u64, "{count} stitch", "{count} stitches"),
        None => PENDING.to_string(),
    }
}

/// The stitch count's pill, dimmed while the plan it counts is of an earlier state.
fn pill_shape(ui: &mut Ui, words: &str, stale: bool) {
    let t = Tokens::get(ui.ctx());
    let colour = if stale { t.text_dim } else { t.text };
    let colour = crate::embroidery::marks::readable_text(colour, &[t.divider], t.dark);
    let galley = ui.painter().layout_no_wrap(words.to_string(), FontId::proportional(PILL_TEXT), colour);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * PILL_PAD, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(PILL_RADIUS), t.divider);
    ui.painter().galley(rect.min + PILL_PAD, galley, colour);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, words));
}

/// What a selected object is, in words: its kind, and for a path its parts ("Path · fill, no stroke").
fn kind_words(n: &vectorcraft_doc::Node, info: Option<&ObjectSettings>) -> String {
    let e = n.extras().embroidery.as_deref();
    let literal = e.is_some_and(|e| e.stitches.is_some());
    // An object converted to manual stitches, or one of its runs.
    let manual = e.is_some_and(|e| e.original.is_some() || e.stitches.as_ref().is_some_and(|r| r.manual));
    if manual {
        return crate::et!("Manual stitches").to_string();
    }
    let kind = match &n.kind {
        NodeKind::Path { guide: true, .. } => crate::et!("Guide"),
        NodeKind::Path { .. } if literal => crate::et!("Stitches from a file"),
        NodeKind::Path { .. } => crate::et!("Path"),
        NodeKind::Compound { .. } => crate::et!("Compound path"),
        NodeKind::Group { .. } => crate::et!("Group"),
        NodeKind::Layer { .. } => crate::et!("Layer"),
        NodeKind::Text(_) => crate::et!("Text object"),
        NodeKind::Image(_) => crate::et!("Image"),
        NodeKind::SymbolInstance { .. } => crate::et!("Symbol instance"),
        NodeKind::PlacedDocument(_) => crate::et!("Placed file"),
        NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) | NodeKind::Repeat(_) | NodeKind::Intertwine { .. } => {
            crate::et!("Live object")
        }
    };
    let parts = match (&n.kind, info) {
        (NodeKind::Path { guide: false, .. } | NodeKind::Compound { .. }, Some(i)) if !literal => {
            Some(match (Part::Fill.painted(i), Part::Stroke.painted(i)) {
                (true, true) => crate::et!("fill and stroke"),
                (true, false) => crate::et!("fill, no stroke"),
                (false, true) => crate::et!("stroke, no fill"),
                (false, false) => crate::et!("no fill or stroke"),
            })
        }
        (NodeKind::Group { .. } | NodeKind::Layer { .. }, Some(i)) => {
            return format!("{kind} · {}", crate::etn!(i.object_count as u64, "{count} object takes settings", "{count} objects take settings"));
        }
        _ => None,
    };
    match parts {
        Some(p) => format!("{kind} · {p}"),
        None => kind.to_string(),
    }
}

/// A state's words, under the header.
fn state_words(ui: &mut Ui, words: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL_TEXT).color(t.text)).wrap());
}

/// Why the selected objects take no settings: each reason once (manual stitches said by
/// [`manual`] instead, when the selection holds converted objects).
fn takes_none(ui: &mut Ui, info: &ObjectSettings) {
    let mut said: Vec<&str> = Vec::new();
    for s in info.skipped.iter().filter(|s| info.manual.is_none() || s.reason != "manual") {
        let words = vocabulary::takes_none(&s.reason);
        if !said.contains(&words) {
            said.push(words);
            state_words(ui, words);
        }
    }
}

/// The part switch: the selection's fill and stroke as the panel's [`super::segments`], a part
/// none of them paints greyed ("Stroke: none") → the part chosen this frame.
fn part_switch(ui: &mut Ui, info: &ObjectSettings, shown: Option<Part>) -> Option<Part> {
    let labels = Part::ALL.map(|part| {
        let has = part.painted(info) || shown == Some(part);
        let label = match (part, has) {
            (Part::Fill, true) => crate::et!("Fill"),
            (Part::Fill, false) => crate::et!("Fill: none"),
            (Part::Stroke, true) => crate::et!("Stroke"),
            (Part::Stroke, false) => crate::et!("Stroke: none"),
        };
        (part, has, label)
    });
    let items: Vec<super::Segment<'_>> = labels
        .iter()
        .map(|(part, has, label)| super::Segment {
            id: ui.id().with(("embroidery-part", part.id())),
            label,
            selected: shown == Some(*part),
            enabled: *has,
        })
        .collect();
    let chosen = super::segments(ui, SWITCH_HEIGHT, SWITCH_TEXT, &items).and_then(|i| labels.get(i)).map(|(part, ..)| *part);
    ui.add_space(GAP);
    chosen
}

/// The settings the tab offers, by section: the product plan's subset (what a person sets on an
/// object most), in the order the mockups draw them. Any other part setting one of the objects sets
/// shows after its section's ([`MORE`]).
const SECTIONS: [(Section, &[&str]); 3] = [
    (Section::Stitch, &["stitch", "angle", "spacing", "length", "narrowAsSatin"]),
    (Section::Underlay, &["underlay"]),
    (Section::Compensation, &["pull", "expand"]),
];

/// The part settings outside the tab's subset, and the section each shows in when an object sets it.
const MORE: [(&str, Section); 3] = [("splitLength", Section::Stitch), ("pullPercent", Section::Compensation), ("push", Section::Compensation)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Stitch,
    Underlay,
    Compensation,
}

impl Section {
    fn heading(self) -> &'static str {
        match self {
            Section::Stitch => crate::et!("Stitch"),
            Section::Underlay => crate::et!("Underlay"),
            Section::Compensation => crate::et!("Compensation"),
        }
    }
}

/// The rows of part `part` (`p`) of the selection, fitted to `width` points, by section, then its
/// entry and exit, with how far the stitches miss them (`missed`, millimetres, from the plan shown).
#[allow(clippy::too_many_arguments)]
fn part_rows(
    ui: &mut Ui,
    part: Part,
    p: &PartInfo,
    ids: &[NodeId],
    width: f32,
    lang: Lang,
    missed: Option<EntryExit<f64>>,
    planned: Option<[vectorcraft_geom::Point; 2]>,
    commands: &mut Vec<Command>,
) {
    if p.sewn.as_deref() == Some("none") {
        let t = Tokens::get(ui.ctx());
        let words = vocabulary::not_sewn(p.reason.as_deref());
        let dim = crate::embroidery::marks::readable_text(t.text_dim, &[t.panel], t.dark);
        ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL_TEXT).color(dim)).wrap());
    }
    let sections: Vec<(Section, Vec<Owned>)> = shown_keys(p)
        .into_iter()
        .map(|(section, keys)| (section, keys.into_iter().filter_map(|k| key_row(p, part, k, lang)).collect::<Vec<_>>()))
        .filter(|(_, rows)| !rows.is_empty())
        .collect();
    // One object: its rows open the point dialog (the command it sets them with takes one).
    let one = ids.first().copied().filter(|_| ids.len() == 1);
    let points: Vec<Owned> = ["entry", "exit"]
        .into_iter()
        .map(|k| point_row(p, k, missed.and_then(|m| if k == "entry" { m.entry } else { m.exit }), lang, one.is_some()))
        .collect();
    let all: Vec<Row<'_>> = sections.iter().flat_map(|(_, r)| r.iter()).chain(&points).map(|o| o.row(lang)).collect();
    let cols = Columns::fit(ui, width, &all);
    for (section, owned) in &sections {
        rows::heading(ui, section.heading());
        for o in owned {
            if let Some(edit) = rows::show(ui, &cols, &o.row(lang)) {
                commands.extend(edit_command(p, part, o.key, edit, ids));
            }
        }
    }
    // Set on Canvas: the embroidery tool, at this part (the tool shows the part the tab does).
    let tip = crate::et!("Drag this part's entry and exit points on the canvas with the embroidery tool.");
    if rows::heading_with_button(ui, crate::et!("Entry and exit"), crate::et!("Set on Canvas"), tip) {
        commands.push(("tool.select", json!({"tool": vectorcraft_tools::embroidery::ID})));
        // The keys act at once on the first object's entry: the arrows nudge it, Enter types it.
        if let Some(id) = ids.first() {
            let focus = json!({"kind": "entry", "id": id.0, "part": part.id(), "index": 0});
            commands.push(("tool.setOption", json!({"key": "focus", "value": focus})));
        }
    }
    for o in &points {
        match rows::show(ui, &cols, &o.row(lang)) {
            Some(Edit::Restore) => commands.push(("embroidery.reset", json!({"ids": ids_json(ids), "part": part.id(), "keys": [o.key]}))),
            Some(Edit::Open) => {
                let point = if o.key == "entry" { &p.entry } else { &p.exit };
                let stored = point.value.filter(|_| point.set_count > 0).map(|[x, y]| vectorcraft_geom::Point::new(x, y));
                let at = stored.or_else(|| planned.map(|e| if o.key == "entry" { e[0] } else { e[1] }));
                if let (Some(id), Some(at)) = (one, at) {
                    let p = json!({"kind": o.key, "id": id.0, "part": part.id(), "at": [at.x, at.y], "set": stored.is_some()});
                    commands.push((POINT_DIALOG, p));
                }
            }
            _ => {}
        }
    }
}

/// The keys of part `p` that have rows, by section: the tab's subset, then any other key of the
/// section one of the objects sets ([`MORE`]).
fn shown_keys(p: &PartInfo) -> Vec<(Section, Vec<&'static str>)> {
    SECTIONS
        .into_iter()
        .map(|(section, keys)| {
            let more = MORE.iter().filter(|(_, s)| *s == section).map(|(k, _)| *k);
            (section, keys.iter().copied().filter(|k| shows(p, k, true)).chain(more.filter(|k| shows(p, k, false))).collect())
        })
        .collect()
}

/// Whether part `p`'s key `key` has a row: a key of the tab's subset where the stitch its objects
/// are sewn in uses it (any where they differ), another where one of the objects sets it.
fn shows(p: &PartInfo, key: &str, offered: bool) -> bool {
    let Some(k) = p.settings.get(key) else { return false };
    if key == "stitch" {
        return true;
    }
    (offered && k.applies != Some(false)) || k.set_count > 0
}

/// The row of key `key` of part `p` (`part`).
fn key_row(p: &PartInfo, part: Part, key: &'static str, lang: Lang) -> Option<Owned> {
    let k = p.settings.get(key)?;
    let label = vocabulary::label(key, p.sewn.as_deref());
    let every = k.set_count >= p.object_count && k.set_count > 0 && !k.mixed;
    let some = k.set_count > 0 && !every;
    let field = match part_field(key)? {
        SettingField::Number { min, max, unit, decimals } => {
            let (value, hint) = number_of(&k.value, key, unit, decimals, lang, k.rule.as_ref());
            OwnedField::Number { value, unit, decimals, bounds: (min, max), hint }
        }
        SettingField::Choice(_) => choice_of(p, part, key, k),
    };
    // What the value would be unset: with its unit for screen readers, without it after ↺ (the
    // field beside it shows the unit), "fabric 0,20".
    let inherited = |unit: bool| {
        k.inherited.as_ref().map(|i| vocabulary::inherited(&i.source, i.section.as_deref(), &value_words(&i.value, key, lang, part, unit)))
    };
    let source_words = match (k.value.as_str(), k.source.as_deref()) {
        (Some("auto"), _) => Some(crate::et!("auto").to_string()),
        (_, Some(s)) => Some(vocabulary::source(s, k.section.as_deref())),
        (_, None) => None,
    };
    let counted = text::fill(
        crate::et!("{count} of {total} set"),
        &[("count", &numbers::count(lang, k.set_count as u64)), ("total", &numbers::count(lang, p.object_count as u64))],
    );
    let name = name_of(key, label);
    let marks = if every {
        let without =
            text::fill(crate::et!("Set on the selected objects; without it: {value}"), &[("value", inherited(true).as_deref().unwrap_or_default())]);
        rows::Marks::own(&name, inherited(false), without)
    } else if some {
        rows::Marks::some(&name, counted)
    } else {
        rows::Marks::inherited(source_words, None)
    };
    let mut owned = Owned::new(key, label, name, vocabulary::tip(key), field, marks);
    if k.mixed {
        // On a line of its own: a sentence joined with ". " reads wrong in Japanese and Chinese.
        owned.description = format!("{}\n{}", owned.description, crate::et!("The selected objects differ."));
    }
    Some(owned)
}

/// What screen readers call key `key`'s field: its label, its section's with it where two labels
/// read alike ("Underlay: Type").
fn name_of(key: &str, label: &str) -> String {
    let section = match key {
        "stitch" => Section::Stitch,
        "underlay" => Section::Underlay,
        _ => return label.to_string(),
    };
    text::fill(crate::et!("{section}: {setting}"), &[("section", section.heading()), ("setting", label)])
}

/// A number field's value from `info`'s `value` of key `key` (`None`: blank) and the words shown
/// in it when blank: a rule's (satin spacing by width), or the two sides of a pull that differ.
fn number_of(value: &Value, key: &str, unit: &str, decimals: usize, lang: Lang, rule: Option<&Value>) -> (Option<f64>, Option<String>) {
    let format = fields::format(lang, unit, decimals);
    match value {
        Value::Number(_) => (value.as_f64().map(|v| if key == "angle" { shown_angle(v) } else { v }), None),
        Value::Array(sides) => match sides.as_slice() {
            [a, b] if a == b => (a.as_f64(), None),
            [a, b] => {
                let words = |v: &Value| v.as_f64().map(|x| format.show(x)).unwrap_or_default();
                (None, Some(format!("{} / {}", words(a), words(b))))
            }
            _ => (None, None),
        },
        Value::String(s) if s == "auto" => {
            let chose = rule.and_then(|r| r.get("chose")).and_then(Value::as_f64);
            let hint = match chose {
                Some(v) => text::fill(crate::et!("Auto · {value}"), &[("value", &format.show(v))]),
                None => crate::et!("Auto").to_string(),
            };
            (None, Some(hint))
        }
        _ => (None, None),
    }
}

/// An angle as the tab shows it: from 0 to 180° (a fill's rows are lines, the same either way).
fn shown_angle(a: f64) -> f64 {
    let a = a.rem_euclid(HALF_TURN);
    // Within a millionth of 180 is 0: the same rows.
    if HALF_TURN - a < SAME_ANGLE { 0.0 } else { a }
}

/// The stitches a fill isn't offered, of those `embroidery.set` takes: satin (a fill has no width
/// to sew across, the project owner's choice) and a running line.
const NOT_FOR_A_FILL: [&str; 2] = ["satin", "running"];

/// A choice row of key `key` (`stitch`, `underlay`) of part `p`: the stitches as `embroidery.set`
/// lists them (a fill's without [`NOT_FOR_A_FILL`]), the underlays the planner sews apart.
fn choice_of(p: &PartInfo, part: Part, key: &str, k: &KeyInfo) -> OwnedField {
    let stroke = part == Part::Stroke;
    let mut ids: Vec<&str> = match (key, part_field(key)) {
        ("stitch", Some(SettingField::Choice(ids))) => ids.into_iter().filter(|id| stroke || !NOT_FOR_A_FILL.contains(id)).collect(),
        ("stitch", _) => vec!["auto"],
        _ => match p.sewn.as_deref() {
            Some(sewn @ ("tatami" | "satin")) => distinct_underlays(sewn),
            _ => distinct_underlays(""),
        },
    };
    let current = k.value.as_str();
    if let Some(c) = current.filter(|c| !ids.contains(c)) {
        ids.push(c);
    }
    let words = |id: &str| if key == "stitch" { vocabulary::stitch(id, stroke) } else { vocabulary::underlay(id) };
    let options = ids.iter().map(|id| (id.to_string(), words(id))).collect();
    let shown = match current {
        None => crate::et!("Mixed").to_string(),
        Some("auto") if key == "stitch" => match p.sewn.as_deref() {
            Some(sewn) if sewn != "none" => vocabulary::auto(&vocabulary::sewn_as(sewn, p.rule.as_deref())),
            _ => words("auto"),
        },
        Some("auto") => match k.rule.as_ref() {
            Some(rule) => vocabulary::auto(&vocabulary::underlay_chosen(rule.get("chose").and_then(Value::as_str))),
            None => words("auto"),
        },
        Some(id) => words(id),
    };
    OwnedField::Choice { shown, options, current: current.map(str::to_string) }
}

/// A value in words, as a hint or a description says it: a number as its field writes it (its unit
/// with it when `unit`), a choice by its name.
fn value_words(v: &Value, key: &str, lang: Lang, part: Part, unit: bool) -> String {
    match (part_field(key), v) {
        (_, Value::String(id)) if key == "stitch" => vocabulary::stitch(id, part == Part::Stroke),
        (_, Value::String(id)) if key == "underlay" => vocabulary::underlay(id),
        (_, Value::String(id)) if id == "auto" => crate::et!("auto").to_string(),
        (Some(SettingField::Number { unit: name, decimals, .. }), _) => {
            let format = fields::format(lang, if unit { name } else { "" }, decimals);
            match v {
                Value::Array(sides) => match sides.as_slice() {
                    [a, b] if a == b => a.as_f64().map(|x| format.show(x)).unwrap_or_default(),
                    _ => sides.iter().filter_map(Value::as_f64).map(|x| format.show(x)).collect::<Vec<_>>().join(" / "),
                },
                _ => v.as_f64().map(|x| format.show(if key == "angle" { shown_angle(x) } else { x })).unwrap_or_default(),
            }
        }
        _ => numbers::decimal(lang, v.as_f64().unwrap_or_default(), WORDS_DECIMALS),
    }
}

/// The entry or exit row of part `p`: "Auto" (nearest where the needle is) or set, with how far its
/// stitches miss it when that is more than the Check tab lets pass (`missed`, millimetres: the most
/// of the selected objects'). It `opens` the point's dialog for one object; for several it is read
/// only (the embroidery tool places their points one at a time).
fn point_row(p: &PartInfo, key: &'static str, missed: Option<f64>, lang: Lang, opens: bool) -> Owned {
    let point = if key == "entry" { &p.entry } else { &p.exit };
    let label = vocabulary::label(key, None);
    let every = point.set_count >= p.object_count && point.set_count > 0 && !point.mixed;
    let (words, own, aside) = if point.mixed {
        (crate::et!("Mixed"), false, None)
    } else if every {
        let note = missed
            .filter(|m| *m > POINT_MISS_MM)
            .map(|m| text::fill(crate::et!("missed by {value} mm"), &[("value", &numbers::decimal(lang, m, MISS_DECIMALS))]));
        (crate::et!("Set"), true, note)
    } else {
        (crate::et!("Auto"), false, Some(crate::et!("nearest").to_string()))
    };
    let restore = (point.set_count > 0).then(|| restore_words(label));
    let description = if every { crate::et!("Set on the selected objects.") } else { crate::et!("Nearest to where the needle is.") };
    Owned {
        key,
        label,
        name: label.to_string(),
        tip: vocabulary::tip(key),
        // One object: a button opening the point's dialog; several: read only.
        field: if opens { OwnedField::Button(words.to_string()) } else { OwnedField::Words(words.to_string()) },
        own,
        restore,
        aside,
        hint: None,
        description: description.to_string(),
        problem: None,
    }
}

/// Where the plan shown starts and ends `part` of the one object `ids` holds (document points): its
/// first unit's first needle point and its last unit's last; `None` for several objects, or no plan.
fn planned_ends(app: &VectorcraftApp, uid: u64, ids: &[NodeId], part: Part) -> Option<[vectorcraft_geom::Point; 2]> {
    let [id] = ids else { return None };
    let scene = app.embroidery.preview.shown(uid)?.planned.built_scene()?;
    use vectorcraft_engine::cmd::embroidery::plan::Part as Planned;
    let of_part = |k: &Planned| matches!((part, k), (Part::Fill, Planned::Fill(_)) | (Part::Stroke, Planned::Stroke(_)));
    let units: Vec<_> = scene.units().iter().filter(|u| u.object == *id && of_part(&u.key.part)).collect();
    Some([units.first()?.entry?, units.last()?.exit?])
}

/// The command for `edit` of key `key` of part `p` (`part`) on objects `ids`: a value set, or
/// unset when it is what they would inherit; ↺ unsets it.
fn edit_command(p: &PartInfo, part: Part, key: &str, edit: Edit, ids: &[NodeId]) -> Option<Command> {
    let ids = ids_json(ids);
    let set = |v: Value| Some(("embroidery.set", json!({"ids": ids, "part": part.id(), key: v})));
    match edit {
        Edit::Number(v) => set(json!(v)),
        Edit::Choice(id) => {
            let k = p.settings.get(key)?;
            let inherited = k.inherited.as_ref().map(|i| &i.value);
            if k.set_count == 0 && k.value.as_str() == Some(id.as_str()) {
                // Already what they inherit.
                return None;
            }
            if inherited.and_then(Value::as_str) == Some(id.as_str()) { set(Value::Null) } else { set(json!(id)) }
        }
        Edit::Restore => Some(("embroidery.reset", json!({"ids": ids, "part": part.id(), "keys": [key]}))),
        // Only the entry and exit rows open anything (`part_rows`).
        Edit::Open => None,
    }
}

/// Command params for objects `ids` made the selection's when `ids` are the objects `selected`:
/// their `ids` left out, as each of the tab's commands defaults to the selection, so that a
/// selection larger than the commands' list of ids takes the edits too.
fn for_selection(params: &mut Value, ids: &[NodeId], selected: &[NodeId]) {
    if ids == selected
        && let Some(o) = params.as_object_mut()
    {
        o.remove("ids");
    }
}

/// Ids as the commands take them.
fn ids_json(ids: &[NodeId]) -> Value {
    ids.iter().map(|id| id.0).collect()
}

/// Trim After: whether the thread is cut after the objects, a dash where they differ.
fn trim_after(ui: &mut Ui, info: &ObjectSettings, ids: &[NodeId], commands: &mut Vec<Command>) {
    if ids.is_empty() || !trims(info) {
        return;
    }
    ui.add_space(GAP);
    let value = if info.trim_after.mixed { None } else { info.trim_after.value };
    let words = crate::etn!(ids.len() as u64, "Trim the thread after the object", "Trim the thread after the objects");
    if rows::check(ui, &words, value, Tokens::get(ui.ctx()).panel) {
        commands.push(("embroidery.setTrimAfter", json!({"ids": ids_json(ids), "on": value != Some(true)})));
    }
}

/// Whether the cut after the objects means something: objects that are sewn (those that take
/// settings, stitches from a file or manual ones, a symbol's instances, groups of them).
fn trims(info: &ObjectSettings) -> bool {
    info.object_count > 0
        || info.manual.is_some()
        || info.skipped.iter().any(|s| matches!(s.reason.as_str(), "literal" | "manual" | "symbolInstance"))
}

/// Stop After: whether the machine stops after the objects, a dash where they differ, beside Trim
/// After.
fn stop_after(ui: &mut Ui, info: &ObjectSettings, ids: &[NodeId], commands: &mut Vec<Command>) {
    if ids.is_empty() || !trims(info) {
        return;
    }
    let value = if info.stop_after.mixed { None } else { info.stop_after.value };
    let words = crate::etn!(ids.len() as u64, "Stop the machine after the object", "Stop the machine after the objects");
    let tip = crate::et!(
        "The machine stops after the object's stitches, as for appliqué. Where a thread change or the design's end comes next, the machine stops there anyway, so no extra stop is written."
    );
    if rows::check_with_tip(ui, &words, value, Tokens::get(ui.ctx()).panel, tip) {
        commands.push(("embroidery.setStopAfter", json!({"ids": ids_json(ids), "on": value != Some(true)})));
    }
}

/// The commands the tab runs that it says more of, and the Restore question's own.
const CONVERT: &str = "embroidery.convertToManual";
const RESTORE: &str = "embroidery.restoreObject";
/// How far from 100 % a scale since converting must be for the tab to say it, percent: what its
/// words round to (a whole percent), so that a scale undone by rounding isn't said.
const SCALE_SAID: f64 = 0.5;

/// Convert to Manual Stitches, the command's label (`label`), for the objects that take settings;
/// waiting while a convert is `busy`. It takes the keyboard when `focus` says so (Restore Object,
/// pressed by the keyboard, put these objects back), and pressed by the keyboard, it hands the
/// keyboard to Restore Object once the objects are converted.
fn convert(ui: &mut Ui, label: String, busy: bool, ids: &[NodeId], commands: &mut Vec<Command>, focus: &mut Option<(FocusNext, Vec<NodeId>)>) {
    ui.add_space(GAP);
    let tip = crate::et!("Freezes the stitches planned now, so each needle point can be moved; Restore Object brings the object back.");
    let resp = ui.add_enabled(!busy, egui::Button::new(label));
    take_focus(&resp, FocusNext::Convert, ids, focus);
    rows::focus_ring(ui, &resp, resp.rect.expand(rows::FOCUS_GAP), Tokens::get(ui.ctx()).panel);
    rows::keyboard_tip(ui, resp.id, tip);
    rows::described(ui, resp.id, tip);
    let resp = resp.on_hover_text(tip);
    if resp.clicked() {
        hand_focus(&resp, FocusNext::Restore, ids, focus);
        commands.push((CONVERT, json!({"ids": ids_json(ids)})));
    }
}

/// `resp` takes the keyboard when `focus` names its button (`which`) for objects `ids`.
fn take_focus(resp: &egui::Response, which: FocusNext, ids: &[NodeId], focus: &mut Option<(FocusNext, Vec<NodeId>)>) {
    if focus.as_ref().is_some_and(|(f, at)| *f == which && at.as_slice() == ids) {
        resp.request_focus();
        *focus = None;
    }
}

/// Pressed while it had the keyboard, `resp`'s button hands it to the button `next` that replaces
/// it for objects `ids` once its work is done.
fn hand_focus(resp: &egui::Response, next: FocusNext, ids: &[NodeId], focus: &mut Option<(FocusNext, Vec<NodeId>)>) {
    if resp.has_focus() {
        *focus = Some((next, ids.to_vec()));
    }
}

/// The status bar's words after converting, from the command's reply.
pub(crate) fn converted(reply: &Value) -> String {
    let count = reply["convertedCount"].as_u64().unwrap_or(0);
    crate::etn!(
        count,
        "{count} object converted to manual stitches: Restore Object brings it back",
        "{count} objects converted to manual stitches: Restore Object brings them back"
    )
}

/// The manual stitches among the selection ([`ObjectSettings::manual`]): what they are, their runs
/// and needle points, the scale since converting when there is one, and Restore Object (asking first
/// when their stitches were edited since converting).
fn manual(ui: &mut Ui, info: &ObjectSettings, ids: &[NodeId], lang: Lang, commands: &mut Vec<Command>, focus: &mut Option<(FocusNext, Vec<NodeId>)>) {
    let Some(m) = &info.manual else { return };
    state_words(ui, crate::et!("Sewn as they are placed: they no longer change with the fabric or the object's settings."));
    let t = Tokens::get(ui.ctx());
    let dim = crate::embroidery::marks::readable_text(t.text_dim, &[t.panel], t.dark);
    let runs = crate::etn!(m.runs as u64, "{count} run · {needle} needle points", "{count} runs · {needle} needle points");
    let counts = text::fill(&runs, &[("needle", &numbers::count(lang, m.needle_points as u64))]);
    ui.add(egui::Label::new(egui::RichText::new(counts).size(SMALL_TEXT).color(dim)).wrap());
    // The axis scaled most, as a percent of the size converted at.
    if let Some([least, most]) = m.scale {
        let farthest = if most - 1.0 >= 1.0 - least { most } else { least };
        let percent = farthest * 100.0;
        if (percent - 100.0).abs() >= SCALE_SAID && percent.is_finite() {
            let words = text::fill(crate::et!("Scaled to {value}% since converting"), &[("value", &numbers::decimal(lang, percent, 0))]);
            ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL_TEXT).color(dim)).wrap());
        }
    }
    ui.add_space(GAP / 2.0);
    let tip = crate::et!("Puts the object back in place of its manual stitches, planned again at its size now.");
    // Dimmed only as Restore refuses: every one locked, which it says.
    let locked = crate::et!("Locked objects can't be restored: unlock them first.");
    let resp = ui.add_enabled(m.restorable > 0, egui::Button::new(crate::et!("Restore Object…")));
    take_focus(&resp, FocusNext::Restore, ids, focus);
    rows::focus_ring(ui, &resp, resp.rect.expand(rows::FOCUS_GAP), t.panel);
    rows::keyboard_tip(ui, resp.id, tip);
    rows::described(ui, resp.id, if m.restorable > 0 { tip } else { locked });
    let resp = resp.on_hover_text(tip).on_disabled_hover_text(locked);
    if resp.clicked() {
        hand_focus(&resp, FocusNext::Convert, ids, focus);
        // Restored at once, or asked first when their stitches were edited since converting: the
        // question takes the keyboard, so the button lets it go (else each Enter would press it
        // again behind the question).
        resp.surrender_focus();
        commands.push((RESTORE, json!({"ids": ids_json(ids)})));
    }
}

/// The footer's words: the document's fabric (`fabric`) and thread (`weight`), then what the rows'
/// looks mean.
fn footer_words(fabric: &FabricChoice, weight: f64, lang: Lang) -> [String; 3] {
    let fabric = match fabric {
        FabricChoice::BuiltIn => crate::et!("built-in values").to_string(),
        FabricChoice::Own => crate::et!("Custom").to_string(),
        FabricChoice::Preset(id) => vocabulary::fabric(id),
    };
    let line = text::fill(
        crate::et!("Fabric: {fabric} · thread {weight} wt"),
        &[("fabric", &fabric), ("weight", &numbers::decimal(lang, weight, WEIGHT_DECIMALS))],
    );
    [
        line,
        crate::et!("Dim values come from the fabric and automatic rules.").to_string(),
        crate::et!("A bar marks a value set on these objects.").to_string(),
    ]
}

/// The footer's height: its words wrapped in the panel's width as they will be drawn, its button,
/// and the room around them.
fn footer_height(ui: &Ui, words: &[String; 3], buttons: &[String; 2]) -> f32 {
    let wrap = (ui.available_width() - 2.0 * FOOTER_PAD).max(1.0);
    let height = |w: &str, size: f32| ui.painter().layout(w.to_string(), FontId::proportional(size), Color32::WHITE, wrap).size().y;
    let [line, dim @ ..] = words;
    let text = height(line, FOOTER_LINE_TEXT) + dim.iter().map(|w| height(w, SMALL_TEXT)).sum::<f32>();
    let s = ui.spacing();
    let button = s.interact_size.y.max(ui.text_style_height(&egui::TextStyle::Button) + 2.0 * s.button_padding.y);
    // The buttons side by side where they fit, else one under the other.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let widths: f32 =
        buttons.iter().map(|b| ui.painter().layout_no_wrap(b.clone(), font.clone(), Color32::WHITE).size().x + 2.0 * s.button_padding.x).sum();
    let lines = if widths + s.item_spacing.x <= wrap { 1.0 } else { 2.0 };
    // The room above the card, its padding, the half gap over its buttons, and the spaces between
    // the rows above it, its lines of words and its buttons' lines.
    GAP + 2.0 * FOOTER_PAD + text + GAP / 2.0 + lines * button + (words.len() as f32 + lines) * s.item_spacing.y
}

/// The footer: `words` (the document's fabric and thread, what the rows' looks mean), and Restore
/// from Fabric (the selection's own values unset, both parts, their points and Trim After kept).
fn footer(ui: &mut Ui, info: &ObjectSettings, words: &[String; 3], buttons: &[String; 2], ids: &[NodeId], commands: &mut Vec<Command>) {
    let t = Tokens::get(ui.ctx());
    let [line, dim @ ..] = words;
    let any_set = [&info.fill, &info.stroke].into_iter().flatten().any(|p| p.settings.values().any(|k| k.set_count > 0));
    ui.add_space(GAP);
    egui::Frame::NONE.fill(t.panel_darker).corner_radius(CornerRadius::same(FOOTER_RADIUS)).inner_margin(egui::Margin::same(FOOTER_PAD as i8)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(line).size(FOOTER_LINE_TEXT).color(t.text_strong)).wrap());
            let colour = crate::embroidery::marks::readable_text(t.text_dim, &[t.panel_darker], t.dark);
            for words in dim {
                ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL_TEXT).color(colour)).wrap());
            }
            ui.add_space(GAP / 2.0);
            let [settings, restore] = buttons;
            ui.horizontal_wrapped(|ui| {
                let open = ui
                    .button(settings.as_str())
                    .on_hover_text(crate::et!("The document's fabric, thread and machine: Document Setup's Embroidery tab."));
                if open.clicked() {
                    commands.push((SETTINGS, json!({})));
                }
                let tip = crate::et!("Unsets the selected objects' own values, so they follow the fabric again; entry, exit and Trim After stay.");
                let restore = ui.add_enabled(any_set && !ids.is_empty(), egui::Button::new(restore.as_str()));
                if restore.on_hover_text(tip).clicked() {
                    commands.push(("embroidery.reset", json!({"ids": ids_json(ids)})));
                }
            });
        },
    );
}

/// The one object selected when it takes settings itself (a path or a compound path, not a group
/// of them): what Copy takes.
fn copyable(app: &VectorcraftApp) -> Option<NodeId> {
    let st = app.session.active()?;
    let [id] = st.selection.objects.as_slice() else { return None };
    let n = st.doc.node(*id)?;
    let literal = n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some());
    matches!(n.kind, NodeKind::Path { guide: false, .. } | NodeKind::Compound { .. }).then_some(*id).filter(|_| !literal)
}

/// Whether Copy Embroidery Settings can run: one object that takes settings selected.
pub(crate) fn can_copy(app: &VectorcraftApp) -> bool {
    copyable(app).is_some()
}

/// Whether Paste Embroidery Settings can run: something copied, and objects selected.
pub(crate) fn can_paste(app: &VectorcraftApp) -> bool {
    app.embroidery.object.copied.is_some() && app.session.active().is_some_and(|st| !st.selection.objects.is_empty())
}

/// Whether `embroidery.set` takes `value` for part setting `key`: a number within its range (a
/// pull's two sides each), `auto`, or one of its choices. A value a newer version wrote fails it.
fn takes(key: &str, value: &Value) -> bool {
    match (part_field(key), value) {
        (_, Value::Null) => true,
        (_, Value::String(s)) if s == "auto" => true,
        (Some(SettingField::Number { min, max, .. }), Value::Number(_)) => value.as_f64().is_some_and(|v| (min..=max).contains(&v)),
        (Some(SettingField::Number { min, max, .. }), Value::Array(sides)) => {
            !sides.is_empty() && sides.iter().all(|v| v.as_f64().is_some_and(|v| (min..=max).contains(&v)))
        }
        (Some(SettingField::Choice(ids)), Value::String(id)) => ids.contains(&id.as_str()),
        // Trim After and the like aren't copied; anything else as the engine would judge it.
        (Some(_), _) => false,
        (None, _) => false,
    }
}

/// `embroidery.copySettings {}`: take the selected object's settings, both parts, as they are now
/// (its entry and exit points and Trim After stay with it, as `embroidery.copyFrom` copies) →
/// `{source, passedOver}`.
pub(crate) fn copy(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    super::only_keys(p, &[])?;
    let id = copyable(app).ok_or("select one object that takes embroidery settings: a path or a compound path")?;
    let info = app.session.embroidery_merged(&[id]).map_err(|e| e.to_string())?;
    let mut copied = Copied { parts: Vec::new(), left_out: Vec::new() };
    for part in Part::ALL {
        let Some(p) = part.of(&info) else { continue };
        let mut values = serde_json::Map::new();
        for (key, k) in &p.settings {
            let value = if k.set_count > 0 { k.value.clone() } else { Value::Null };
            if takes(key, &value) {
                values.insert(key.clone(), value);
            } else {
                copied.left_out.push(format!("{}.{key}", part.id()));
            }
        }
        copied.parts.push((part, values));
    }
    let left_out = copied.left_out.clone();
    app.embroidery.object.copied = Some(copied);
    Ok(json!({ "source": id.0, "passedOver": left_out }))
}

/// `embroidery.pasteSettings {part?}`: what Copy took, given to the selected objects (in any
/// document) as one `embroidery.set` a part in one `command.batch`, one undo step; a part none of
/// them has left out. What it passed over (values this version doesn't take, objects that take no
/// settings) is said in the status bar → `{passedOver, skippedCount, skipped}`.
pub(crate) fn paste(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    super::only_keys(p, &["part"])?;
    let copied = app.embroidery.object.copied.clone().ok_or("copy an object's embroidery settings first")?;
    let only = match p.get("part").filter(|v| !v.is_null()) {
        Some(part) => Some(part.as_str().and_then(Part::from_id).ok_or_else(|| format!("part {part}: fill or stroke"))?),
        None => None,
    };
    let ids = app.session.active().map(|st| st.selection.objects.clone()).ok_or("no document")?;
    if ids.is_empty() {
        return Err("select the objects to paste the embroidery settings onto".into());
    }
    // The parts some selected object has: `embroidery.set` refuses a part none of them has.
    let info = app.session.embroidery_merged(&ids).map_err(|e| e.to_string())?;
    let commands: Vec<Value> = copied
        .parts
        .iter()
        .filter(|(part, _)| only.is_none_or(|o| o == *part) && part.of(&info).is_some())
        .map(|(part, values)| {
            let mut params = values.clone();
            params.insert("part".into(), json!(part.id()));
            json!({"command": "embroidery.set", "params": params})
        })
        .collect();
    if commands.is_empty() {
        return Err("the selected objects take none of the embroidery settings copied".into());
    }
    app.run("command.batch", json!({"label": crate::i18n::t("Paste Embroidery Settings"), "commands": commands}))?;
    // The selected objects that take no settings (type, images…), passed over.
    let skipped: Vec<Value> = info.skipped.iter().map(|k| json!({"id": k.id, "reason": k.reason})).collect();
    let mut said: Vec<String> = Vec::new();
    if !copied.left_out.is_empty() {
        said.push(text::fill(crate::et!("Not pasted, as this version doesn't take them: {keys}"), &[("keys", &copied.left_out.join(", "))]));
    }
    if info.skipped_count > 0 {
        said.push(crate::etn!(
            info.skipped_count as u64,
            "{count} selected object takes no settings and was passed over",
            "{count} selected objects take no settings and were passed over"
        ));
    }
    if !said.is_empty() {
        app.status(said.join(" · "));
    }
    Ok(json!({ "passedOver": copied.left_out, "skippedCount": info.skipped_count, "skipped": skipped }))
}

/// The part the tab shows of the active document's selection: the one chosen when the objects
/// paint it, else the other ([`shown`]); the one chosen with nothing to read.
pub(crate) fn shown_part(app: &mut VectorcraftApp) -> Part {
    let chosen = app.ui.embroidery_panel.part;
    match settings_of(app) {
        Some((_, Ok(info))) => shown(chosen, &info).unwrap_or(chosen),
        _ => chosen,
    }
}

/// What `ui.inspect` reports of the tab (`embroideryPanel.object`): the objects its fields are of,
/// the part shown, and each row of it: its key, value (as `embroidery.info` gives it), source,
/// whether it is set on every object (as the tab marks it: not where they differ), and whether the
/// objects differ. Null with nothing read yet, or when what it read isn't of the active document,
/// its snapshot and its selection as they are (the tab hasn't been drawn since).
pub(crate) fn inspect(app: &VectorcraftApp) -> Value {
    let Some(memo) = app.embroidery.object.memo.as_ref() else { return Value::Null };
    let Some(st) = app.session.active() else { return Value::Null };
    if memo.uid != st.uid || memo.ids != st.selection.objects || !Weak::ptr_eq(&memo.snapshot, &Arc::downgrade(&st.doc)) {
        return Value::Null;
    }
    let Ok(info) = &memo.info else { return Value::Null };
    let part = shown(app.ui.embroidery_panel.part, info);
    let rows: Vec<Value> = part
        .and_then(|p| p.of(info))
        .map(|p| {
            let keys = shown_keys(p).into_iter().flat_map(|(_, keys)| keys);
            keys.filter_map(|key| p.settings.get(key).map(|k| (key, k)))
                .map(|(key, k)| {
                    let set = k.set_count >= p.object_count && k.set_count > 0 && !k.mixed;
                    json!({"key": key, "value": k.value, "source": k.source, "set": set, "mixed": k.mixed})
                })
                .collect()
        })
        .unwrap_or_default();
    json!({"ids": ids_json(&memo.ids), "part": part.map(Part::id), "rows": rows})
}

#[cfg(test)]
mod tests;
