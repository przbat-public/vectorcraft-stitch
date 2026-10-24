//! The canvas's Art | Stitches switch: two segments at the canvas's top right corner that show the
//! active document as its art or as its stitches (View › Stitch Preview, ⌘E, by its command). For
//! screen readers it is a group of two radio buttons. Hidden in Presentation Mode and with no
//! document; while an opacity mask or a pattern is edited it shows Art, disabled (the art shows
//! then).

use egui::{Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_engine::cmd::embroidery::view::paused;

use crate::VectorcraftApp;
use crate::theme::Tokens;

/// Room between the switch and the canvas's top and right edges, points.
const INSET: f32 = 10.0;
/// A segment's height, and the room either side of its word, points.
const HEIGHT: f32 = 24.0;
const PAD: f32 = 12.0;
/// The size of its words, points: the interface's small text.
const TEXT_SIZE: f32 = 12.0;

/// The switch's id; its segments' are `.with(stitches)`.
pub(crate) fn group_id() -> Id {
    Id::new("embroidery-view-switch")
}

/// The switch's segments, left to right: the word, and whether it shows the stitches.
fn segments() -> [(&'static str, bool); 2] {
    [(crate::et!("Art"), false), (crate::et!("Stitches"), true)]
}

/// Draw the switch over the canvas `canvas` and act on a click → whether one changed the view.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, canvas: Rect) -> bool {
    if app.ui.screen_mode >= 3 {
        return false;
    }
    let Some(st) = app.session.active() else { return false };
    let enabled = paused(&st.doc).is_none();
    let on = enabled && app.session.stitch_view_of(st.uid).on;
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(TEXT_SIZE);
    let widths: Vec<f32> =
        segments().iter().map(|(word, _)| ui.painter().layout_no_wrap((*word).to_string(), font.clone(), t.text).size().x + 2.0 * PAD).collect();
    let total: f32 = widths.iter().sum();
    let right = canvas.right() - INSET;
    let frame = Rect::from_min_size(pos2(right - total, canvas.top() + INSET), vec2(total, HEIGHT));
    if !canvas.contains_rect(frame) {
        return false;
    }
    let group = group_id();
    // The radio group: a child of the canvas's, whose node the two radios are parented under.
    let segments_ui = ui.new_child(egui::UiBuilder::new().id_salt(group).max_rect(frame));
    ui.ctx().accesskit_node_builder(segments_ui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::RadioGroup);
        node.set_label(crate::et!("View"));
    });
    let radius = CornerRadius::same(t.radius);
    ui.painter().rect(frame, radius, t.panel, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut x = frame.left();
    let mut clicked = None;
    for ((word, stitches), width) in segments().into_iter().zip(widths) {
        let rect = Rect::from_min_size(pos2(x, frame.top()), vec2(width, HEIGHT));
        x += width;
        let selected = stitches == on;
        let id = group.with(stitches);
        // A drag too, so that the canvas under it never takes a press there (it drags), also while
        // the switch is disabled.
        let resp = segments_ui.interact(rect, id, Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, word));
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::RadioButton);
            node.set_toggled(if selected { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
        });
        let fill = if selected {
            t.tool_active
        } else if resp.hovered() && enabled {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect.shrink(2.0), radius, fill);
        let color = if !enabled {
            t.text_disabled
        } else if selected {
            t.text_strong
        } else {
            t.text_dim
        };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, word, font.clone(), color);
        if resp.has_focus() {
            ui.painter().rect_stroke(rect.shrink(1.0), radius, Stroke::new(1.0, t.accent), StrokeKind::Inside);
        }
        if resp.clicked() && !selected && enabled {
            clicked = Some(stitches);
        }
    }
    let Some(stitches) = clicked else { return false };
    if let Err(e) = app.run("view.stitchPreview", json!({"on": stitches})) {
        app.status(e);
    }
    true
}

#[cfg(test)]
mod tests;
