//! The stitch view's overlays for the selection (the project owner's choice: a frame and markers,
//! no dimming):
//!
//! - with the Selection tool the canvas draws only the selection's frame and the key object's
//!   outline ([`frame_only`]), no path outlines or anchors over the stitches;
//! - each selected object gets an entry ◆ where the needle enters it and an exit ⊕ where it leaves
//!   it, at most [`MARKED_OBJECTS_MAX`] of them, labelled when one object is selected; they come from
//!   the scene's ends ([`StitchScene::ends_of`]), as the Sewing order list's do;
//! - a selected object the stitches shown aren't of yet (it changed since the plan shown) is outlined
//!   dashed in its layer's colour until the new plan comes;
//! - [`not_sewn`] counts the selected objects that sew nothing, for the canvas's notice.

use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Shape, Stroke, StrokeKind, pos2, vec2};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::scene::ObjectEnds;

use super::stitch_preview::Freshness;
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// The most selected objects marked: more markers would cover the stitches they mark. An estimate.
pub const MARKED_OBJECTS_MAX: usize = 32;
/// The entry marker's diagonal and the exit marker's diameter, points (the mockups' sizes).
pub const ENTRY_MARKER_PT: f32 = 12.0;
pub const EXIT_MARKER_PT: f32 = 14.0;
/// The labels' size, their room either side, and how far they sit from their marker, points.
const LABEL_SIZE: f32 = 11.0;
const LABEL_PAD: f32 = 5.0;
const LABEL_OFFSET: f32 = 10.0;
/// The dashes of a pending object's outline, points.
const DASH: f32 = 4.0;
const GAP: f32 = 3.0;

/// The selected objects' ends for (document, plan generation, revision): the revision moves with
/// the selection, so a selection change finds them again and nothing else does.
#[derive(Default)]
pub struct Ends {
    of: Option<(EndsKey, Arc<Vec<ObjectEnds>>)>,
}

/// What the ends are found for: the document's uid, the plan's generation, the revision.
type EndsKey = (u64, u64, u64);

/// Whether the canvas draws only the selection's frame and the key object's outline: the Selection
/// tool in the stitch view.
pub fn frame_only(app: &VectorcraftApp) -> bool {
    app.embroidery.layer.drawn() && app.session.tool_id() == "selection"
}

/// The active document's selected objects' ends in the plan shown, found again only when the
/// selection or the plan changes.
fn ends(app: &mut VectorcraftApp) -> Option<Arc<Vec<ObjectEnds>>> {
    let st = app.session.active()?;
    let shown = app.embroidery.preview.shown(st.uid)?;
    let key = (st.uid, shown.planned.generation, st.revision);
    if let Some((k, ends)) = &app.embroidery.ends.of
        && *k == key
    {
        return Some(ends.clone());
    }
    let selected: Vec<NodeId> = st.selection.objects.clone();
    let ends = Arc::new(shown.planned.scene().ends_of(&st.doc, &selected));
    app.embroidery.ends.of = Some((key, ends.clone()));
    Some(ends)
}

/// How many selected objects the plan shown sews nothing of, when it is of the document as it is
/// (else it can't say yet), as this frame's overlays found them.
pub fn not_sewn(app: &VectorcraftApp, now: f64) -> usize {
    let Some(st) = app.session.active() else { return 0 };
    if !app.embroidery.layer.drawn() || app.embroidery.preview.freshness(st.uid, now) != Freshness::Fresh {
        return 0;
    }
    let Some(generation) = app.embroidery.preview.shown(st.uid).map(|s| s.planned.generation) else { return 0 };
    match &app.embroidery.ends.of {
        Some((k, ends)) if *k == (st.uid, generation, st.revision) => ends.iter().filter(|e| e.units.is_empty()).count(),
        _ => 0,
    }
}

/// The selected objects the stitches shown aren't of yet: changed since the plan shown.
#[cfg(test)]
pub(crate) fn pending(app: &VectorcraftApp) -> Vec<NodeId> {
    let Some(st) = app.session.active() else { return Vec::new() };
    let Some(shown) = app.embroidery.preview.shown(st.uid) else { return Vec::new() };
    st.selection.objects.iter().copied().filter(|id| changed(&st.doc, &shown.document, *id)).collect()
}

/// Whether `id` differs in `now` from `planned`: the stitches shown aren't of it yet.
fn changed(now: &Document, planned: &Document, id: NodeId) -> bool {
    match (now.node(id), planned.node(id)) {
        (Some(a), Some(b)) => !std::ptr::eq(a, b),
        _ => true,
    }
}

/// Draw the overlays over the stitch view: dashed outlines for the selected objects the stitches
/// shown aren't of yet, then the markers.
pub fn draw(app: &mut VectorcraftApp, painter: &egui::Painter, xf: &Xf) {
    if !app.embroidery.layer.drawn() || !app.ui.view.edges || app.session.tool_transforming() || app.ui.screen_mode >= 3 {
        return;
    }
    let Some(ends) = ends(app) else { return };
    let Some(st) = app.session.active() else { return };
    let Some(shown) = app.embroidery.preview.shown(st.uid) else { return };
    let t = Tokens::get(painter.ctx());
    let symbols = st.doc.symbol_boxes();
    for e in ends.iter().filter(|e| changed(&st.doc, &shown.document, e.object)) {
        let Some(n) = st.doc.node(e.object) else { continue };
        let color = layer_color(&st.doc, e.object);
        for line in crate::canvas::to_screen_path(&crate::canvas::node_outline(n, &symbols), xf) {
            painter.extend(Shape::dashed_line(&line, Stroke::new(1.0, color), DASH, GAP));
        }
    }
    let marked: Vec<&ObjectEnds> = ends.iter().filter(|e| !e.units.is_empty()).take(MARKED_OBJECTS_MAX).collect();
    let labelled = ends.len() == 1;
    for e in &marked {
        let color = layer_color(&st.doc, e.object);
        if let Some(exit) = e.exit {
            let at = xf.to_screen(exit);
            exit_marker(painter, at, color);
            if labelled {
                label(painter, at + vec2(-LABEL_OFFSET, LABEL_OFFSET), Align2::RIGHT_TOP, crate::et!("Exit"), color);
            }
        }
        if let Some(entry) = e.entry {
            let at = xf.to_screen(entry);
            entry_marker(painter, at, t.accent);
            if labelled {
                label(painter, at + vec2(LABEL_OFFSET, -LABEL_OFFSET), Align2::LEFT_BOTTOM, crate::et!("Entry"), t.accent);
            }
        }
    }
}

fn layer_color(doc: &Document, id: NodeId) -> Color32 {
    let [r, g, b] = doc.layer_color(id);
    Color32::from_rgb(r, g, b)
}

/// The entry ◆: a diamond in `color` with a white edge.
fn entry_marker(p: &egui::Painter, at: Pos2, color: Color32) {
    let r = ENTRY_MARKER_PT / 2.0;
    let points = vec![pos2(at.x, at.y - r), pos2(at.x + r, at.y), pos2(at.x, at.y + r), pos2(at.x - r, at.y)];
    p.add(Shape::convex_polygon(points, color, Stroke::new(1.5, Color32::WHITE)));
}

/// The exit ⊕: a white face with a ring and a plus in `color`.
fn exit_marker(p: &egui::Painter, at: Pos2, color: Color32) {
    let r = EXIT_MARKER_PT / 2.0;
    p.circle(at, r, Color32::WHITE, Stroke::new(1.5, color));
    let arm = r * 0.6;
    let stroke = Stroke::new(1.5, color);
    p.line_segment([at - vec2(arm, 0.0), at + vec2(arm, 0.0)], stroke);
    p.line_segment([at - vec2(0.0, arm), at + vec2(0.0, arm)], stroke);
}

/// A pill reading `text` in white on `color`, anchored at `at` by `align`.
fn label(p: &egui::Painter, at: Pos2, align: Align2, text: &str, color: Color32) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(LABEL_SIZE), Color32::WHITE);
    let size = galley.size() + vec2(2.0 * LABEL_PAD, LABEL_PAD);
    let rect = align.anchor_size(at, size);
    p.rect(rect, CornerRadius::same(u8::MAX), color, Stroke::NONE, StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests;
