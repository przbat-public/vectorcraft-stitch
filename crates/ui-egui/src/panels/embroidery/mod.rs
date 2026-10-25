//! Window › Embroidery (VectorCraft Stitch): how the active document's design is sewn, in three
//! tabs: Object (the selected objects' embroidery settings), Sewing Order and Check (Stitches comes
//! with the PR that builds it).
//!
//! - **It never plans.** Drawing it asks the stitch preview's scheduler for the plan and for its
//!   tab's read command replies ([`crate::embroidery::stitch_preview`]), which the planning thread
//!   makes with the commands' own code; the panel, the canvas and the status bar read the same plan.
//! - **Where it goes.** The first Window › Embroidery docks it as the last tab of the group holding
//!   Properties, Layers and Libraries, as the mockups draw it (the project owner's choice), turning
//!   the dock into the shared layout once; after that it shows where the person left it. It is no
//!   `DockTab`, so the preferences keep upstream's shape; they live in the fork's own folder, since
//!   upstream resets a layout holding a panel it doesn't know (`apps/vectorcraft/src/stitch_profile.rs`).
//!   It has no icon in the collapsed icon column (its icon shows on its collapsed tab).
//! - **States** of Sewing Order and Check: no document; not planned yet (blank for
//!   [`SHOW_BUSY_AFTER_SECS`]); planning; nothing to sew; can't be planned (the reason; the last
//!   good result stays when there is one); ready; updating (the last result, and "Updating…" once
//!   the newer one takes a while). The Object tab draws in every one of them ([`object`]): only its
//!   stitch count waits for a plan.
//! - **The tab strip** fits every language: each tab as wide as its name, the widest shrunk first
//!   when they don't fit (`craft_ui::tabs::fit`), a shrunk name elided with the whole of it in its
//!   tooltip and for screen readers.
//! - The open tab and the Object tab's part are kept with the interface's state, read leniently (an
//!   unknown tab reads as the first), and `ui.embroideryPanel {tab?, part?}` shows the panel at a
//!   tab.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::reorder::Step;

use crate::VectorcraftApp;
use crate::embroidery::plan_worker::Query;

pub mod check;
pub mod lengths;
pub mod object;
pub mod order;
use crate::embroidery::stitch_preview::Freshness;
use crate::theme::Tokens;

/// The panel id (Window › Embroidery, `window.panel`).
pub const ID: &str = "embroidery";

/// How long a plan may take before the panel says it is planning, seconds: under a tenth of a
/// second a response reads as instant, so a busy mark would only flicker (Nielsen, "Response Times:
/// The 3 Important Limits", <https://www.nngroup.com/articles/response-times-3-important-limits/>).
pub const SHOW_BUSY_AFTER_SECS: f64 = 0.1;

/// The tab strip's height, points (measured on the mockups at 2 px a point: an estimate).
const STRIP_HEIGHT: f32 = 22.0;
/// The room round a tab's name in the strip, each side, and the narrowest a tab is shrunk to before
/// the strip would hide one, points: estimates (a tab of three letters stays readable).
pub(super) const TAB_PAD: f32 = 8.0;
const TAB_MIN: f32 = 36.0;
/// Room under the strip and between the header's lines, points.
const GAP: f32 = 8.0;
/// How much of a stale result's colour the panel keeps: its words and marks dimmed while a newer
/// plan is made. An estimate.
pub(crate) const STALE_INK: f32 = 0.6;
/// The words of the panel's states and of the strip, points: the strip's tabs as the interface's
/// text, the states' lines a size under it, their titles a size over the states' lines.
const STRIP_TEXT: f32 = 12.0;
const STATE_TEXT: f32 = 11.5;
const STATE_TITLE: f32 = 12.5;
/// "Updating…" and its spinner, points: the header's small text.
const UPDATING_TEXT: f32 = 11.0;
/// The empty state's icon, the planning state's spinner and its progress bar, points (the
/// mockups' sizes: estimates).
const STATE_ICON: f32 = 28.0;
const STATE_SPINNER: f32 = 18.0;
const STATE_PROGRESS: f32 = 160.0;
/// The room over a state, and between its icon and its words, points.
const STATE_TOP: f32 = 12.0;
const STATE_GAP: f32 = 4.0;
/// How far a tab's highlight sits inside its segment, points.
pub(super) const SEGMENT_INSET: f32 = 2.0;

/// The panel's tabs, in the order they show. Their ids are stable (`ui.embroideryPanel`, the saved
/// interface state).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
    #[default]
    Object,
    Order,
    Check,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Object, Tab::Order, Tab::Check];

    pub fn id(self) -> &'static str {
        match self {
            Tab::Object => "object",
            Tab::Order => "order",
            Tab::Check => "check",
        }
    }

    pub fn from_id(id: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The tab's name in the interface language.
    pub fn label(self) -> &'static str {
        match self {
            Tab::Object => crate::et!("Object"),
            Tab::Order => crate::et!("Sewing Order"),
            Tab::Check => crate::et!("Check"),
        }
    }
}

/// A tab as saved: a tab this version doesn't show (a newer version's, or anything else) reads as
/// the first, so that it never stops the interface's state from loading.
impl<'de> Deserialize<'de> for Tab {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(v.as_str().and_then(Tab::from_id).unwrap_or_default())
    }
}

/// The panel's settings, kept with the interface's state (`ui.inspect` → `ui.embroidery_panel`):
/// the open tab, and the part the Object tab shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub tab: Tab,
    pub part: object::Part,
}

/// Settings as saved, read as leniently as the tab: anything but an object (null, a word, a number:
/// a hand edit, a damaged file, another version's shape) reads as the defaults, so that it never
/// stops the interface's state, and with it every other preference, from loading.
impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let read = |key: &str| v.get(key).and_then(Value::as_str);
        Ok(Self {
            tab: read("tab").and_then(Tab::from_id).unwrap_or_default(),
            part: read("part").and_then(object::Part::from_id).unwrap_or_default(),
        })
    }
}

/// What a tab asks the scheduler for besides the plan, for document `uid`: the figures, always,
/// and its own reply.
fn queries(app: &VectorcraftApp, uid: u64, tab: Tab) -> Vec<Query> {
    let stats = crate::embroidery::stitch_preview::figures_query();
    match tab {
        // Its count is read from the plan itself.
        Tab::Object => vec![stats],
        Tab::Order => vec![stats, order::query()],
        Tab::Check => vec![stats, check::query(&app.embroidery.check, uid)],
    }
}

/// Window › Embroidery, `ui.embroideryPanel {tab?, format?}`, Show (`embroidery.showFinding`) and
/// Next and Previous Finding: the panel's own handling of its commands, before the dock's (`None`
/// for any other command).
pub(crate) fn command(app: &mut VectorcraftApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    match id {
        "window.panel" if p.get("panel").and_then(Value::as_str).and_then(crate::menus::normalize_panel) == Some(ID) => Some(show_panel(app)),
        "ui.embroideryPanel" => Some(panel_command(app, p)),
        "embroidery.nextHit" => Some(check::step(app, true)),
        "embroidery.showFinding" => Some(check::show_finding_command(app, p)),
        "view.checkMarks" => Some(check_marks(app, p)),
        "embroidery.previousHit" => Some(check::step(app, false)),
        "embroidery.copySettings" => Some(object::copy(app, p)),
        "embroidery.pasteSettings" => Some(object::paste(app, p)),
        "embroidery.documentSetup" => Some(crate::embroidery::setup_tab::open_at_tab(app, p)),
        _ => None,
    }
}

/// `view.checkMarks {on?}`: the Check tab's findings marked on the canvas or not, for the session
/// (no `on`: the other way) → `{on}`, as the other view switches answer.
fn check_marks(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    only_key(p, "on")?;
    let on = crate::menus::opt_bool(p, "on")?.unwrap_or(app.embroidery.check.hide_marks);
    app.embroidery.check.hide_marks = !on;
    Ok(json!({ "on": on }))
}

/// Refuse a key of `p` other than `keys`, those a panel command reads.
pub(crate) fn only_keys(p: &Value, keys: &[&str]) -> Result<(), String> {
    match p.as_object().and_then(|o| o.keys().find(|k| !keys.contains(&k.as_str()))) {
        Some(key) => {
            let keys = keys.join(", ");
            Err(format!("unknown parameter `{key}`; the command takes: {keys}"))
        }
        None => Ok(()),
    }
}

/// Refuse a key of `p` other than `only`, the one key a panel command reads.
fn only_key(p: &Value, only: &str) -> Result<(), String> {
    match p.as_object().and_then(|o| o.keys().find(|k| *k != only)) {
        Some(k) => Err(format!("unknown key `{k}`: the only one is {only}")),
        None => Ok(()),
    }
}

/// Whether the panel's own commands can run now (`None` for any other command): Next and Previous
/// Finding need a finding shown, on a check of the plan shown.
pub(crate) fn enabled(app: &VectorcraftApp, id: &str) -> Option<bool> {
    match id {
        "embroidery.nextHit" | "embroidery.previousHit" => Some(check::can_step(app)),
        "embroidery.copySettings" => Some(object::can_copy(app)),
        "embroidery.pasteSettings" => Some(object::can_paste(app)),
        "embroidery.documentSetup" => Some(app.session.active().is_some()),
        _ => None,
    }
}

/// `ui.embroideryPanel {tab?: "object"|"order"|"check", part?: "fill"|"stroke", format?: a
/// format's id, or null}`: show the panel, at `tab`; `part` the one the Object tab shows; `format`
/// the one the active document's check is for (null: its thread palette's, as the Check tab's
/// "Checked for" chooses) → `{tab, part, format}` (`format` null when none is chosen).
fn panel_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    only_keys(p, &["tab", "part", "format"])?;
    // Both checked before either changes.
    let tab = match p.get("tab").filter(|v| !v.is_null()) {
        Some(tab) => {
            let ids: Vec<&str> = Tab::ALL.iter().map(|t| t.id()).collect();
            Some(tab.as_str().and_then(Tab::from_id).ok_or_else(|| format!("tab {tab}: one of {}", ids.join(", ")))?)
        }
        None => None,
    };
    let part = match p.get("part").filter(|v| !v.is_null()) {
        Some(part) => Some(part.as_str().and_then(object::Part::from_id).ok_or_else(|| format!("part {part}: fill or stroke"))?),
        None => None,
    };
    if let Some(tab) = tab {
        app.ui.embroidery_panel.tab = tab;
    }
    if let Some(part) = part {
        app.ui.embroidery_panel.part = part;
    }
    if let Some(format) = p.get("format") {
        let uid = app.session.active().map(|st| st.uid).ok_or("no document")?;
        match format.as_str() {
            None if format.is_null() => {
                app.embroidery.check.format.remove(&uid);
            }
            given => {
                let ids: Vec<&'static str> = vectorcraft_engine::cmd::embroidery::check_formats().into_iter().map(|(id, _)| id).collect();
                let key = "format";
                let id = given.and_then(|g| ids.iter().find(|id| **id == g)).ok_or_else(|| format!("`{key}` must be one of {}", ids.join(", ")))?;
                app.embroidery.check.format.insert(uid, id);
            }
        }
    }
    show_panel(app)?;
    let format = app.session.active().and_then(|st| app.embroidery.check.format.get(&st.uid).copied());
    // The part the Object tab shows: the one chosen, or the other where the objects don't paint it.
    let part = object::shown_part(app);
    Ok(json!({ "tab": app.ui.embroidery_panel.tab.id(), "part": part.id(), "format": format }))
}

/// `ui.inspect`'s `embroideryPanel` (VectorCraft Stitch): what the panel's Object tab shows
/// (`object`, [`object::inspect`]). Its open tab and the part chosen are in `ui.embroidery_panel`.
pub fn inspect(app: &VectorcraftApp) -> Value {
    json!({ "object": object::inspect(app) })
}

/// Show the panel: brought forward where it is; else where it was closed from, by the dock's own
/// rule; else, the first time, as the last tab of the group holding Libraries, Layers or
/// Properties (the dock turns into the shared layout then).
fn show_panel(app: &mut VectorcraftApp) -> Result<Value, String> {
    let docked = app.ui.docking.as_ref().is_some_and(|layout| layout.contains(&ID.to_string()));
    let floating = crate::floating::group_of(&app.ui, ID).is_some();
    if docked || floating {
        return app.run("window.panel.activate", json!({ "panel": ID }));
    }
    let shown = |app: &VectorcraftApp, id: &str| match &app.ui.docking {
        Some(layout) => layout.contains(&id.to_string()),
        None => crate::floating::docked_tabs(&app.ui).any(|t| t.info().0 == id),
    };
    // Closed from the Window menu: back where it was, floating or beside the panel it was with.
    if let Some(location) = app.ui.docking_hidden.remove(ID) {
        if let Some([x, y, width, height]) = location.floating {
            return app.run("window.panel.float", json!({ "panel": ID, "x": x, "y": y, "width": width, "height": height }));
        }
        if let Some(anchor) = location.anchor.filter(|a| shown(app, a)) {
            let placement = serde_json::to_value(&location.placement).map_err(|e| e.to_string())?;
            return app.run("window.panel.layout", json!({ "action": { "OpenAt": { "panel": ID, "anchor": anchor, "placement": placement } } }));
        }
    }
    let anchor = ["libraries", "layers", "properties"].into_iter().find(|id| shown(app, id));
    let r = match anchor {
        Some(anchor) => app.run("window.panel.move", json!({ "panel": ID, "anchor": anchor })),
        None => app.run("window.panel.layout", json!({ "action": { "Open": { "panel": ID, "anchor": null } } })),
    };
    // Its tab joins the others': room for their labels is made once they are drawn.
    if r.is_ok() {
        app.embroidery.fit_dock_frames = FIT_DOCK_FRAMES;
    }
    r
}

/// How many frames after Window › Embroidery the dock may be widened for its tabs' labels: the
/// dock draws the panel's new tab within a frame or two. A dock not drawn by then (collapsed, or
/// the panel floating) isn't widened later, at a moment that has nothing to do with the command.
/// An estimate.
const FIT_DOCK_FRAMES: u8 = 3;

/// The id of the shared dock's tree (upstream's `panel_docking`, its `DockArea`): its tabs' ids
/// come from it (craft-ui's `("tab", panel)` under it). Both are upstream's internals, listed in
/// FORK.md: if they change, the fit finds no tabs and does nothing.
const DOCK_AREA: &str = "vectorcraft-panel-docking";
/// The room a tab's label keeps inside its tab, points: a little on each side.
const LABEL_ROOM: f32 = 8.0;
/// The most of the window the dock may take to show its tabs' labels whole. An estimate: the
/// canvas keeps the rest.
const DOCK_SHARE: f32 = 0.4;

/// Once Window › Embroidery has docked the panel beside other tabs: the dock widened, where their
/// labels don't fit, until each shows whole. The shared dock's strip (craft-ui's) squeezes the
/// widest tabs first and clips their words, so four tabs at the dock's usual width cut some of
/// them ("Embroidery", "Properties"). Measured on the tabs as drawn: each one's width against its
/// label's, the squeezed ones (as wide as the widest) widened together by what the shortest of
/// room lacks. Within [`DOCK_SHARE`] of the window; the person may narrow the dock again.
fn fit_dock_tabs(app: &mut VectorcraftApp, ui: &Ui) {
    if app.embroidery.fit_dock_frames == 0 {
        return;
    }
    let group = app.ui.docking.as_ref().map(|layout| tab_group(&layout.root, ID)).unwrap_or_default();
    // Not in the dock's tabs (floating, or no dock): nothing to fit.
    if group.is_empty() {
        app.embroidery.fit_dock_frames = 0;
        return;
    }
    let area = egui::Id::new(DOCK_AREA);
    let font = craft_ui::docking::DockStyle::from_ui(ui).font;
    let tabs: Vec<(f32, f32)> = group
        .iter()
        .filter_map(|panel| {
            let tab = ui.ctx().read_response(area.with(("tab", panel)))?.rect.width();
            // The label the dock draws: the panel's name as upstream translates it (a plain row).
            let label = crate::state::all_panels().find(|p| p.0 == panel.as_str()).map_or(panel.as_str(), |p| crate::i18n::t(p.1));
            let words = ui.painter().layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE).size().x;
            Some((tab, words))
        })
        .collect();
    // Not drawn yet: the next frame, while the command's frames last ([`widen_dock`] counts them).
    if tabs.len() != group.len() {
        return;
    }
    app.embroidery.fit_dock_frames = 0;
    let lacking = tabs.iter().map(|(tab, words)| words + LABEL_ROOM - tab).fold(0.0, f32::max);
    if lacking <= 0.0 {
        return;
    }
    let widest = tabs.iter().map(|(tab, _)| *tab).fold(0.0, f32::max);
    let squeezed = tabs.iter().filter(|(tab, _)| *tab >= widest - 0.5).count() as f32;
    let window = ui.ctx().content_rect().width();
    let now = crate::dock::shared_dock_default_width(&app.ui);
    let wanted = (now + squeezed * lacking).min((window * DOCK_SHARE).max(now));
    // Given to the dock at the frame's end: the dock notes the width it was drawn at after the
    // panels draw, over anything set before.
    app.embroidery.dock_width = Some(crate::dock::clamp_shared_dock_width(wanted));
}

/// End of a frame: the dock's width [`fit_dock_tabs`] asked for, laid out from the next frame; one
/// of the frames it may ask in ([`FIT_DOCK_FRAMES`]) gone.
pub(crate) fn widen_dock(app: &mut VectorcraftApp) {
    app.embroidery.fit_dock_frames = app.embroidery.fit_dock_frames.saturating_sub(1);
    if let Some(width) = app.embroidery.dock_width.take() {
        app.ui.dock_width = Some(width);
        app.ui.dock_width_pending = true;
    }
}

/// The panels of the tabbed group holding `panel` in the dock's tree.
fn tab_group(root: &Option<craft_ui::docking::Node<String>>, panel: &str) -> Vec<String> {
    let mut visit: Vec<&craft_ui::docking::Node<String>> = root.iter().collect();
    while let Some(node) = visit.pop() {
        match node {
            craft_ui::docking::Node::Tabs { panels, .. } if panels.iter().any(|p| p == panel) => return panels.clone(),
            craft_ui::docking::Node::Split { first, second, .. } => {
                visit.push(second);
                visit.push(first);
            }
            _ => {}
        }
    }
    Vec::new()
}

/// The panel's ≡ menu: the selected object's embroidery settings copied and pasted; the selection
/// sewn earlier, later, first or last; the Sewing order's blocks collapsed or expanded; the next or
/// previous finding of the check.
pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    for id in ["embroidery.copySettings", "embroidery.pasteSettings"] {
        if crate::widgets::menu_item_name(ui, &command_label(app, id), crate::menus::enabled(app, id), false) {
            crate::menus::invoke(app, id, json!({}));
        }
    }
    ui.separator();
    // Worked out on the plan the list shows, as a drop is; off while that plan is out of date.
    let selection = app.session.active().map(|st| st.selection.objects.clone()).unwrap_or_default();
    let enabled = !selection.is_empty() && order::steppable(app).is_some();
    for step in [Step::Earlier, Step::Later, Step::First, Step::Last] {
        if crate::widgets::menu_item_name(ui, &command_label(app, step.id()), enabled, false) {
            order::step_selection(app, step, &selection);
        }
    }
    ui.separator();
    let uid = app.session.active().map(|st| st.uid);
    let has_blocks = app.embroidery.order.has_blocks();
    for (label, fold, enabled) in [(crate::et!("Collapse All"), true, has_blocks), (crate::et!("Expand All"), false, true)] {
        if crate::widgets::menu_item_name(ui, label, enabled && uid.is_some(), false)
            && let Some(uid) = uid
        {
            app.embroidery.order.fold_all(uid, fold);
        }
    }
    ui.separator();
    let steps = check::can_step(app);
    for id in ["embroidery.nextHit", "embroidery.previousHit"] {
        if crate::widgets::menu_item_name(ui, &command_label(app, id), steps, false) {
            crate::menus::invoke(app, id, json!({}));
        }
    }
}

/// Command `id`'s label as the menus show it, in the interface language: the panel's items and
/// buttons for commands are named as the commands are, from the commands' own rows.
pub(crate) fn command_label(app: &VectorcraftApp, id: &str) -> String {
    let label = vectorcraft_engine::cmd::command_specs()
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.label)
        .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| c.1))
        .unwrap_or(id);
    crate::menus::display_label(app, id, label)
}

/// The panel.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    fit_dock_tabs(app, ui);
    let t = Tokens::get(ui.ctx());
    let Some(uid) = app.session.active().map(|st| st.uid) else {
        empty_state(ui, crate::et!("No document"), crate::et!("Open or draw a design to see how it will be sewn."));
        return;
    };
    strip(app, ui);
    ui.add_space(GAP);
    let tab = app.ui.embroidery_panel.tab;
    let asked = queries(app, uid, tab);
    // The Object tab draws whatever the plan's state: only its count needs one.
    if tab == Tab::Object {
        let preview = &mut app.embroidery.preview;
        preview.want_plan();
        for q in asked {
            preview.want(q);
        }
        object::show(app, ui);
        return;
    }
    let preview = &mut app.embroidery.preview;
    preview.want_plan();
    for q in &asked {
        preview.want(q.clone());
    }
    let now = ui.input(|i| i.time);
    let freshness = preview.freshness(uid, now);
    let busy = preview.out_of_date_for(uid, now).is_some_and(|s| s >= SHOW_BUSY_AFTER_SECS);
    let Some(shown) = preview.shown(uid) else {
        match freshness {
            Freshness::Failed { error } => failed(ui, &error),
            Freshness::Pending { progress, .. } if busy => planning(ui, progress),
            // Not planned yet: blank, so a quick plan never flashes a busy mark.
            _ => {}
        }
        return;
    };
    if let Freshness::Failed { error } = &freshness {
        failed(ui, error);
        ui.add_space(GAP);
    }
    if shown.planned.plan.is_empty() {
        let warnings = shown.planned.plan.messages();
        nothing_to_sew(ui, &warnings);
        return;
    }
    // The figures, and the tab's own reply (with the generation of the plan it was made from:
    // the plan before's stays until the new one's comes).
    let stats = asked.first().and_then(|q| shown.reply(q)).and_then(|r| r.result.as_ref().ok()).cloned();
    let own = asked.get(1).and_then(|q| shown.reply(q)).map(|r| (r.result.clone(), r.generation, r.current));
    let generation = shown.planned.generation;
    let stale = !matches!(freshness, Freshness::Fresh);
    ui.horizontal(|ui| {
        // "Updating…" and its spinner at the right, in the room the figures leave them.
        let updating = (stale && busy).then(|| crate::et!("Updating…"));
        let room = updating.map_or(0.0, |words| {
            let font = egui::FontId::proportional(UPDATING_TEXT);
            ui.painter().layout_no_wrap(words.to_string(), font, t.text_dim).size().x + UPDATING_TEXT + 2.0 * ui.spacing().item_spacing.x
        });
        if let Some(stats) = &stats {
            let blocks = stats["blockCount"].as_u64().unwrap_or(0);
            let stitches = stats["stitches"].as_u64().unwrap_or(0);
            let title = format!(
                "{} · {}",
                crate::etn!(blocks, "{count} thread block", "{count} thread blocks"),
                crate::etn!(stitches, "{count} stitch", "{count} stitches")
            );
            // Wrapped in the panel's width (a long line would widen the panel's content), short of
            // "Updating…".
            ui.scope(|ui| {
                ui.set_max_width((ui.available_width() - room).max(0.0));
                ui.add(egui::Label::new(egui::RichText::new(title).strong().color(if stale { t.text_dim } else { t.text_strong })).wrap());
            });
        }
        if let Some(words) = updating {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(words).size(UPDATING_TEXT).color(t.text_dim))
                    .on_hover_text(crate::et!("These are the stitches before the last change; the new ones are being planned."));
                ui.add(egui::Spinner::new().size(UPDATING_TEXT).color(t.accent));
            });
        }
    });
    let plan = shown.planned.plan.clone();
    match (tab, &own) {
        (Tab::Order, Some((Ok(reply), of, current))) => {
            order::show(app, ui, order::Shown { generation, reply, of: *of, current: *current, stats: stats.as_deref(), stale, plan })
        }
        (Tab::Check, _) => {
            let reply = own.as_ref().map(|(r, _, current)| (r, *current));
            check::show(app, ui, check::Shown { uid, reply, stats: stats.as_deref(), stale });
        }
        // Drawn above, before the plan's states.
        (Tab::Order | Tab::Object, _) => {}
    }
}

/// The tab strip: the panel's tabs as [`segments`].
fn strip(app: &mut VectorcraftApp, ui: &mut Ui) {
    let items: Vec<Segment<'_>> = Tab::ALL
        .iter()
        .map(|tab| Segment {
            id: ui.id().with(("embroidery-tab", tab.id())),
            label: tab.label(),
            selected: app.ui.embroidery_panel.tab == *tab,
            enabled: true,
        })
        .collect();
    if let Some(tab) = segments(ui, STRIP_HEIGHT, STRIP_TEXT, &items).and_then(|i| Tab::ALL.get(i)) {
        app.ui.embroidery_panel.tab = *tab;
    }
}

/// How far a segment may be narrower than its words before they count as elided, points: the
/// layout's rounding.
const ELIDED_SLACK: f32 = 0.5;

/// One segment of a strip ([`segments`]): its id, its words, whether it is the one selected, and
/// whether it can be chosen.
pub(super) struct Segment<'a> {
    pub(super) id: egui::Id,
    pub(super) label: &'a str,
    pub(super) selected: bool,
    pub(super) enabled: bool,
}

/// A strip of segments (the panel's tabs, the Object tab's parts), `height` points tall, its words
/// `size` points, accessible as tabs: each as wide as its words and the strip's room shared out
/// ([`strip_widths`]); words a segment can't hold are elided, whole in its tooltip and for screen
/// readers; one that can't be chosen is greyed → the segment clicked.
pub(super) fn segments(ui: &mut Ui, height: f32, size: f32, items: &[Segment<'_>]) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let radius = CornerRadius::same(t.radius);
    ui.painter().rect(rect, radius, t.input, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let font = egui::FontId::proportional(size);
    let natural: Vec<f32> =
        items.iter().map(|s| ui.painter().layout_no_wrap(s.label.to_string(), font.clone(), t.text).size().x + 2.0 * TAB_PAD).collect();
    let selected = items.iter().position(|s| s.selected).unwrap_or(0);
    // An unselected segment's words, read by 4.5:1 on the strip.
    let dim = crate::embroidery::marks::readable_text(t.text_dim, &[t.input], t.dark);
    let mut left = rect.left();
    let mut clicked = None;
    for (i, width) in strip_widths(&natural, selected, rect.width()) {
        let Some(s) = items.get(i) else { continue };
        let segment = Rect::from_min_size(egui::pos2(left, rect.top()), vec2(width, height));
        left += width;
        let resp = ui.add_enabled_ui(s.enabled, |ui| {
            craft_ui::tabs::Tab::new(s.id, s.label, s.selected).show_at(ui, segment, |ui, resp| {
                let fill = if s.selected {
                    t.button
                } else if resp.hovered() {
                    t.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(segment.shrink(SEGMENT_INSET), radius, fill);
                let color = match (s.selected, s.enabled) {
                    (true, _) => t.text_strong,
                    (false, true) => dim,
                    (false, false) => t.text_disabled,
                };
                let room = (segment.width() - 2.0 * SEGMENT_INSET).max(1.0);
                let galley = egui::WidgetText::from(egui::RichText::new(s.label).font(font.clone()).color(color)).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    room,
                    egui::TextStyle::Body,
                );
                ui.painter().galley(egui::Align2::CENTER_CENTER.align_size_within_rect(galley.size(), segment).min, galley, color);
            })
        });
        let elided = natural.get(i).is_some_and(|n| *n > width + ELIDED_SLACK);
        let resp = if elided { resp.inner.on_hover_text(s.label) } else { resp.inner };
        if resp.clicked() && s.enabled {
            clicked = Some(i);
        }
    }
    clicked
}

/// Each tab's segment in a strip `room` points wide, its tabs' names `natural` points wide with
/// their room, `selected` the open one → (tab, width), in order: each as wide as its name when they
/// fit, the widest shrunk first when they don't (`craft_ui::tabs::fit`), and the room left shared
/// out evenly, so the strip is filled.
pub(super) fn strip_widths(natural: &[f32], selected: usize, room: f32) -> Vec<(usize, f32)> {
    let fit = craft_ui::tabs::fit(natural, selected, room, TAB_MIN, 0.0);
    let used: f32 = fit.shown.iter().map(|(_, w)| w).sum();
    let extra = if fit.shown.is_empty() { 0.0 } else { (room - used).max(0.0) / fit.shown.len() as f32 };
    fit.shown.into_iter().map(|(i, w)| (i, w + extra)).collect()
}

/// A state with no content: a title and a line under it.
fn empty_state(ui: &mut Ui, title: &str, body: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(STATE_TOP);
    ui.vertical_centered(|ui| {
        crate::icons::icon(ui, "dc-embroidery", STATE_ICON, t.text_disabled);
        ui.add_space(STATE_GAP);
        ui.label(egui::RichText::new(title).size(STATE_TITLE).color(t.text));
        ui.label(egui::RichText::new(body).size(STATE_TEXT).color(t.text_dim));
    });
}

/// Planning, with the plan's progress when it can say.
fn planning(ui: &mut Ui, progress: Option<f64>) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(STATE_TOP);
    ui.vertical_centered(|ui| {
        ui.add(egui::Spinner::new().size(STATE_SPINNER).color(t.accent));
        ui.add_space(STATE_GAP);
        ui.label(egui::RichText::new(crate::et!("Planning stitches…")).size(STATE_TITLE).color(t.text));
        if let Some(done) = progress.filter(|p| p.is_finite()) {
            ui.add(egui::ProgressBar::new(done.clamp(0.0, 1.0) as f32).desired_width(STATE_PROGRESS));
        }
    });
}

/// The design can't be planned: why, and what makes it plan again.
pub(super) fn failed(ui: &mut Ui, error: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(
        egui::RichText::new(crate::et!("Can't plan the stitches:"))
            .color(crate::embroidery::marks::text_colour(&t, crate::embroidery::marks::Mark::Error)),
    );
    ui.label(egui::RichText::new(crate::i18n::msg(error)).size(STATE_TEXT).color(t.text));
    ui.label(egui::RichText::new(crate::et!("Change the design and the stitches are planned again.")).size(STATE_TEXT).color(t.text_dim));
}

/// A plan that sews nothing: the plan's own warnings, and what is sewn.
fn nothing_to_sew(ui: &mut Ui, warnings: &[String]) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(crate::et!("Nothing to sew")).strong().color(t.text));
    for w in warnings {
        ui.label(egui::RichText::new(crate::i18n::msg(w)).size(STATE_TEXT).color(t.text));
    }
    ui.label(
        egui::RichText::new(crate::et!("Paths with a fill or a stroke are sewn; hidden objects, guides and non-printing layers aren't."))
            .size(STATE_TEXT)
            .color(t.text_dim),
    );
}

#[cfg(test)]
mod tests;
