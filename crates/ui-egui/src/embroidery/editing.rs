//! Stitch editing on the canvas (VectorCraft Stitch's manual stitches): the chip under isolation
//! mode's bar that says what is being edited, and how editing starts from the interface.
//!
//! - **The mode is isolation** (the project owner's choice, 2026-10-27: „Izolacja, reszta
//!   przyciemniona”): the Embroidery Tool working inside isolation of an object holding stitches,
//!   in the stitch view. Upstream's isolation bar names the object and leaves it (its ‹, Escape, a
//!   double-click outside); the chip under it says "Editing stitches", how many needle points the
//!   object holds and how many are selected (with their record numbers, while the plan shown is of
//!   them as they are), and asks to zoom in where the needle points don't show yet. In the art view
//!   it offers the stitches instead: needle points are edited where they are drawn.
//! - **Read once a change** ([`Editing`]): what is being edited is asked of the engine again only
//!   when the document, the selection, the plan shown, the tool or the view changes, and held while
//!   a gesture is under way (a drag's previews move the revision every frame).
//! - **Starting:** a double-click on a stitch object with the tool, or Edit Stitches (the Object
//!   tab) ([`start`]): the object isolated, the tool taken up, the stitches shown.

use egui::{CornerRadius, FontId, Id, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::cmd::embroidery::StitchEditing;

use crate::VectorcraftApp;
use crate::theme::Tokens;

/// The zoom from which needle points show as circles (the project owner's choice, 2026-10-27:
/// „nakłucia jako dwutonowe kółka od 250 %”): where a 1 mm stitch shows its two circles apart.
pub const NEEDLE_CIRCLES_MIN_ZOOM: f64 = 2.5;

/// Room between the chip and the canvas's left edge, and under isolation mode's bar, points.
const INSET: f32 = 10.0;
/// The room either side of the chip's words, and between its lines, points.
const PAD: f32 = 10.0;
const LINE_GAP: f32 = 2.0;
/// The size of its words, points: the interface's small text, its title a size over it.
const TEXT_SIZE: f32 = 12.0;
const TITLE_SIZE: f32 = 12.5;
/// The widest the chip is drawn, points: longer words wrap within it.
const MAX_WIDTH: f32 = 360.0;
/// Its Show Stitches button's height, and the row it takes under the words, points: the panel's
/// buttons' size.
const BUTTON_HEIGHT: f32 = 22.0;
const BUTTON_ROW: f32 = BUTTON_HEIGHT + 4.0;

/// What the chip shows, read again only when what it is of changes ([`Editing::key`]).
#[derive(Default)]
pub struct Editing {
    key: Option<Key>,
    editing: Option<StitchEditing>,
    /// How many times the engine was asked (the tests count them: an idle frame asks nothing).
    pub(crate) reads: usize,
}

/// What [`Editing`] is of: the document and its revision (which moves with every change of it, of
/// the selection and of isolation), the generation of the plan shown, the tool, whether the stitch
/// view and the outline view are on.
type Key = (u64, u64, Option<u64>, &'static str, bool, bool);

/// What stitch editing edits now, asked of the engine only when its [`Key`] changed, and not while
/// a gesture is under way (its previews move the revision every frame): the last answer stands.
pub fn editing(app: &mut VectorcraftApp) -> Option<StitchEditing> {
    let st = app.session.active()?;
    let (uid, revision, gesture) = (st.uid, st.revision, st.interaction.is_some());
    let shown = app.embroidery.preview.shown(uid).map(|s| s.planned.generation);
    let key = (uid, revision, shown, app.session.tool_id(), app.session.stitch_view_of(uid).on, app.ui.view.outline);
    let kept = &app.embroidery.editing;
    if kept.key == Some(key) || (gesture && kept.key.is_some_and(|k| k.0 == uid)) {
        return kept.editing;
    }
    let view = app.view_info();
    let editing = app.session.stitch_editing(view);
    let kept = &mut app.embroidery.editing;
    kept.key = Some(key);
    kept.editing = editing;
    kept.reads += 1;
    editing
}

/// Whether the tool works in isolation of an object holding stitches while the art is shown: the
/// chip then offers the stitches.
fn in_art_view(app: &VectorcraftApp) -> Option<NodeId> {
    let st = app.session.active()?;
    let scope = st.isolation?;
    (app.session.tool_id() == vectorcraft_tools::embroidery::ID
        && !app.session.stitch_view_of(st.uid).on
        && st.doc.node(scope).is_some_and(vectorcraft_tools::embroidery::holds_stitches))
    .then_some(scope)
}

/// The chip's lines: its title and what it says under it.
pub fn lines(app: &mut VectorcraftApp) -> Option<(String, Vec<String>)> {
    let lang = crate::i18n::current();
    if let Some(e) = editing(app) {
        let st = app.session.active()?;
        let name = st.doc.node(e.scope.object).and_then(|n| n.name.clone()).filter(|n| !n.trim().is_empty());
        let title = match name {
            Some(name) => super::text::fill(crate::et!("Editing stitches: {name}"), &[("name", &name)]),
            None => crate::et!("Editing stitches").to_string(),
        };
        let mut said = vec![crate::etn!(e.scope.needles as u64, "{count} needle point", "{count} needle points")];
        if e.selected > 0 {
            let mut selected = crate::etn!(e.selected as u64, "{count} selected", "{count} selected");
            if let Some([(_, Some(a)), (_, Some(b))]) = e.ends {
                let range = if a == b {
                    format!("#{}", super::numbers::count(lang, a as u64))
                } else {
                    format!("#{}–{}", super::numbers::count(lang, a as u64), super::numbers::count(lang, b as u64))
                };
                selected = format!("{selected} · {range}");
            }
            said.push(selected);
        }
        let mut lines = vec![said.join(" · ")];
        if app.view().is_some_and(|v| v.zoom < NEEDLE_CIRCLES_MIN_ZOOM) {
            lines.push(crate::et!("Zoom in to see the needle points.").to_string());
        }
        if e.scope.refused > 0 {
            lines.push(crate::etn!(
                e.scope.refused as u64,
                "{count} run can't be edited: locked, hidden or cut by a clipping mask",
                "{count} runs can't be edited: locked, hidden or cut by a clipping mask"
            ));
        }
        return Some((title, lines));
    }
    in_art_view(app)?;
    Some((crate::et!("Editing stitches").to_string(), vec![crate::et!("Needle points are edited in the Stitches view.").to_string()]))
}

/// Draw the chip over the canvas `canvas` (below isolation mode's bar), when stitches are being
/// edited; in the art view with its Show Stitches button.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, canvas: Rect) {
    if app.ui.screen_mode >= 3 {
        return;
    }
    let Some((title, said)) = lines(app) else { return };
    let art = in_art_view(app).is_some();
    let t = Tokens::get(ui.ctx());
    let room = MAX_WIDTH - 2.0 * PAD;
    let title_galley = ui.painter().layout(title.clone(), crate::theme::semibold(TITLE_SIZE), t.text_strong, room);
    let galleys: Vec<_> = said.iter().map(|s| ui.painter().layout(s.clone(), FontId::proportional(TEXT_SIZE), t.text, room)).collect();
    let button = crate::et!("Show Stitches");
    let button_width = if art { super::rows::text_width(ui, button, TEXT_SIZE) + 2.0 * PAD } else { 0.0 };
    let width = galleys.iter().map(|g| g.size().x).fold(title_galley.size().x, f32::max).max(button_width) + 2.0 * PAD;
    let height =
        PAD + title_galley.size().y + galleys.iter().map(|g| g.size().y + LINE_GAP).sum::<f32>() + if art { BUTTON_ROW } else { 0.0 } + PAD / 2.0;
    let rect = Rect::from_min_size(pos2(canvas.left() + INSET, canvas.top() + INSET), vec2(width, height));
    if !canvas.contains_rect(rect) {
        return;
    }
    let radius = CornerRadius::same(t.radius);
    ui.painter().rect(rect, radius, t.panel, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut y = rect.top() + PAD / 2.0;
    ui.painter().galley(pos2(rect.left() + PAD, y), title_galley.clone(), t.text_strong);
    y += title_galley.size().y + LINE_GAP;
    for g in &galleys {
        ui.painter().galley(pos2(rect.left() + PAD, y), g.clone(), t.text);
        y += g.size().y + LINE_GAP;
    }
    let id = Id::new("embroidery-stitch-editing");
    let words = std::iter::once(title).chain(said).collect::<Vec<_>>().join(". ");
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_label(words);
    });
    if art {
        let at = Rect::from_min_size(pos2(rect.left() + PAD, y + (BUTTON_ROW - BUTTON_HEIGHT) / 2.0), vec2(button_width, BUTTON_HEIGHT));
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id.with("show")).max_rect(at));
        let resp = child.add(egui::Button::new(button));
        if resp.clicked()
            && let Err(e) = app.run("view.stitchPreview", json!({"on": true}))
        {
            app.status(e);
        }
    }
}

/// Edit Stitches: object `id` (holding stitches) isolated, the Embroidery Tool taken up and the
/// stitches shown, as a double-click on it with the tool does (the Object tab's button).
pub fn start(app: &mut VectorcraftApp, id: NodeId) {
    for (command, params) in [
        ("object.isolate", json!({"id": id.0})),
        ("tool.select", json!({"tool": vectorcraft_tools::embroidery::ID})),
        ("view.stitchPreview", json!({"on": true})),
    ] {
        if let Err(e) = app.run(command, params) {
            app.status(e);
            return;
        }
    }
}

#[cfg(test)]
mod tests;
