//! Window › Embroidery (VectorCraft Stitch): how the active document's design is sewn, in two
//! tabs, Sewing Order and Check (Object and Stitches come with the PRs that build them).
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
//! - **States**: no document; not planned yet (blank for [`SHOW_BUSY_AFTER_SECS`]); planning;
//!   nothing to sew; can't be planned (the reason; the last good result stays when there is one);
//!   ready; updating (the last result, and "Updating…" once the newer one takes a while).
//! - The open tab is kept with the interface's state, read leniently (an unknown tab reads as the
//!   first), and `ui.embroideryPanel {tab?}` shows the panel at a tab.

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::reorder::Step;

use crate::VectorcraftApp;
use crate::embroidery::plan_worker::Query;

pub mod check;
pub mod lengths;
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
/// Room under the strip and between the header's lines, points.
const GAP: f32 = 8.0;

/// The panel's tabs, in the order they show. Their ids are stable (`ui.embroideryPanel`, the saved
/// interface state).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tab {
    #[default]
    Order,
    Check,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Order, Tab::Check];

    pub fn id(self) -> &'static str {
        match self {
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

/// The panel's settings, kept with the interface's state (`ui.inspect` → `ui.embroidery_panel`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub tab: Tab,
}

/// Settings as saved, read as leniently as the tab: anything but an object (null, a word, a number:
/// a hand edit, a damaged file, another version's shape) reads as the defaults, so that it never
/// stops the interface's state, and with it every other preference, from loading.
impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(Self { tab: v.get("tab").and_then(Value::as_str).and_then(Tab::from_id).unwrap_or_default() })
    }
}

/// What a tab asks the scheduler for besides the plan, for document `uid`: the figures, always,
/// and its own reply.
fn queries(app: &VectorcraftApp, uid: u64, tab: Tab) -> Vec<Query> {
    let stats = crate::embroidery::stitch_preview::figures_query();
    match tab {
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
        _ => None,
    }
}

/// `ui.embroideryPanel {tab?: "order"|"check", format?: a format's id, or null}`: show the panel,
/// at `tab`; `format` the one the active document's check is for (null: its thread palette's, as
/// the Check tab's "Checked for" chooses) → `{tab, format}` (`format` null when none is chosen).
fn panel_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    only_keys(p, &["tab", "format"])?;
    if let Some(tab) = p.get("tab").filter(|v| !v.is_null()) {
        let ids: Vec<&str> = Tab::ALL.iter().map(|t| t.id()).collect();
        app.ui.embroidery_panel.tab = tab.as_str().and_then(Tab::from_id).ok_or_else(|| format!("tab {tab}: one of {}", ids.join(", ")))?;
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
    Ok(json!({ "tab": app.ui.embroidery_panel.tab.id(), "format": format }))
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
    match anchor {
        Some(anchor) => app.run("window.panel.move", json!({ "panel": ID, "anchor": anchor })),
        None => app.run("window.panel.layout", json!({ "action": { "Open": { "panel": ID, "anchor": null } } })),
    }
}

/// The panel's ≡ menu: the selection sewn earlier, later, first or last; the Sewing order's blocks
/// collapsed or expanded; the next or previous finding of the check.
pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    // Worked out on the plan the list shows, as a drop is; off while that plan is out of date.
    let selection = app.session.active().map(|st| st.selection.objects.clone()).unwrap_or_default();
    let enabled = !selection.is_empty() && order::steppable(app).is_some();
    for (label, step) in [
        (crate::et!("Sew Earlier"), Step::Earlier),
        (crate::et!("Sew Later"), Step::Later),
        (crate::et!("Sew First"), Step::First),
        (crate::et!("Sew Last"), Step::Last),
    ] {
        if crate::widgets::menu_item_name(ui, label, enabled, false) {
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
    if crate::widgets::menu_item_name(ui, crate::et!("Next Finding"), steps, false) {
        crate::menus::invoke(app, "embroidery.nextHit", json!({}));
    }
    if crate::widgets::menu_item_name(ui, crate::et!("Previous Finding"), steps, false) {
        crate::menus::invoke(app, "embroidery.previousHit", json!({}));
    }
}

/// The panel.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(uid) = app.session.active().map(|st| st.uid) else {
        empty_state(ui, crate::et!("No document"), crate::et!("Open or draw a design to see how it will be sewn."));
        return;
    };
    strip(app, ui);
    ui.add_space(GAP);
    let tab = app.ui.embroidery_panel.tab;
    let asked = queries(app, uid, tab);
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
        if let Some(stats) = &stats {
            let blocks = stats["blockCount"].as_u64().unwrap_or(0);
            let stitches = stats["stitches"].as_u64().unwrap_or(0);
            let title = format!(
                "{} · {}",
                crate::etn!(blocks, "{count} thread block", "{count} thread blocks"),
                crate::etn!(stitches, "{count} stitch", "{count} stitches")
            );
            // Wrapped in the panel's width: a long line would widen the panel's content.
            ui.add(egui::Label::new(egui::RichText::new(title).strong().color(if stale { t.text_dim } else { t.text_strong })).wrap());
        }
        if stale && busy {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(crate::et!("Updating…")).size(11.0).color(t.text_dim))
                    .on_hover_text(crate::et!("These are the stitches before the last change; the new ones are being planned."));
                ui.add(egui::Spinner::new().size(11.0).color(t.accent));
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
        (Tab::Order, _) => {}
    }
}

/// The tab strip: a segment per tab, accessible as tabs.
fn strip(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP_HEIGHT), Sense::hover());
    let radius = CornerRadius::same(t.radius);
    ui.painter().rect(rect, radius, t.input, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let width = rect.width() / Tab::ALL.len() as f32;
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let segment = Rect::from_min_size(rect.min + vec2(width * i as f32, 0.0), vec2(width, STRIP_HEIGHT));
        let selected = app.ui.embroidery_panel.tab == tab;
        let label = tab.label();
        let resp = craft_ui::tabs::Tab::new(ui.id().with(("embroidery-tab", tab.id())), label, selected).show_at(ui, segment, |ui, resp| {
            let fill = if selected {
                t.button
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(segment.shrink(2.0), radius, fill);
            let color = if selected { t.text_strong } else { t.text_dim };
            ui.painter().text(segment.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), color);
        });
        if resp.clicked() {
            app.ui.embroidery_panel.tab = tab;
        }
    }
}

/// A state with no content: a title and a line under it.
fn empty_state(ui: &mut Ui, title: &str, body: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        crate::icons::icon(ui, "dc-embroidery", 28.0, t.text_disabled);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(title).size(12.5).color(t.text));
        ui.label(egui::RichText::new(body).size(11.5).color(t.text_dim));
    });
}

/// Planning, with the plan's progress when it can say.
fn planning(ui: &mut Ui, progress: Option<f64>) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        ui.add(egui::Spinner::new().size(18.0).color(t.accent));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(crate::et!("Planning stitches…")).size(12.0).color(t.text));
        if let Some(done) = progress.filter(|p| p.is_finite()) {
            ui.add(egui::ProgressBar::new(done.clamp(0.0, 1.0) as f32).desired_width(160.0));
        }
    });
}

/// The design can't be planned: why, and what makes it plan again.
fn failed(ui: &mut Ui, error: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(
        egui::RichText::new(crate::et!("Can't plan the stitches:"))
            .color(crate::embroidery::marks::text_colour(&t, crate::embroidery::marks::Mark::Error)),
    );
    ui.label(egui::RichText::new(crate::i18n::msg(error)).size(11.5).color(t.text));
    ui.label(egui::RichText::new(crate::et!("Change the design and the stitches are planned again.")).size(11.5).color(t.text_dim));
}

/// A plan that sews nothing: the plan's own warnings, and what is sewn.
fn nothing_to_sew(ui: &mut Ui, warnings: &[String]) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(crate::et!("Nothing to sew")).strong().color(t.text));
    for w in warnings {
        ui.label(egui::RichText::new(crate::i18n::msg(w)).size(11.5).color(t.text));
    }
    ui.label(
        egui::RichText::new(crate::et!("Paths with a fill or a stroke are sewn; hidden objects, guides and non-printing layers aren't."))
            .size(11.5)
            .color(t.text_dim),
    );
}

#[cfg(test)]
mod tests;
