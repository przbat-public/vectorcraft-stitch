//! The stitch view's overlays for the selection (the project owner's choice: a frame and markers,
//! no dimming):
//!
//! - with the Selection tool (and the embroidery tool) the canvas draws only the selection's frame
//!   and the key object's outline ([`frame_only`]), no path outlines or anchors over the stitches;
//! - each selected object gets an entry ◆ where the needle enters it and an exit ⊕ where it leaves
//!   it, at most [`MARKED_OBJECTS_MAX`] of them, labelled when one of them is marked; they come from
//!   the scene's ends ([`StitchScene::ends_of`]), as the Sewing order list's do; with the embroidery
//!   tool its handles take their place ([`super::handles`]);
//! - a selected object the stitches shown aren't of yet (it changed since the plan shown) is outlined
//!   dashed in its layer's colour until the new plan comes; past [`PENDING_OUTLINES_MAX`] objects or
//!   [`PENDING_ANCHORS_MAX`] anchors the box they fall in is dashed instead, and a frame draws at
//!   most [`PENDING_DASHES_MAX`] dashes, where they cross the canvas;
//! - [`not_sewn`] counts the selected objects that sew nothing, for the canvas's notice.

use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Shape, Stroke, StrokeKind, pos2, vec2};
use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_engine::cmd::embroidery::scene::ObjectEnds;
use vectorcraft_geom::{BezPath, Rect};

use super::stitch_preview::Freshness;
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// The most selected objects marked: more markers would cover the stitches they mark. An estimate.
pub const MARKED_OBJECTS_MAX: usize = 32;
/// The entry marker's diagonal and the exit marker's diameter, points (the mockups' sizes): the
/// embroidery tool's handles', which are drawn in their place.
pub const ENTRY_MARKER_PT: f32 = vectorcraft_tools::embroidery::ENTRY_MARKER_PX as f32;
pub const EXIT_MARKER_PT: f32 = vectorcraft_tools::embroidery::EXIT_MARKER_PX as f32;
/// The labels' size, their room either side, and how far they sit from their marker, points (the
/// tool's handles' labels' too).
const LABEL_SIZE: f32 = 11.0;
const LABEL_PAD: f32 = 5.0;
pub(super) const LABEL_OFFSET: f32 = 10.0;
/// The dashes of a pending object's outline, points.
const DASH: f32 = 4.0;
const GAP: f32 = 3.0;
/// The most pending objects outlined, and the most anchors their outlines may have in all: past
/// either, the box they fall in is dashed instead, as the canvas's own overlay leaves out outlines
/// past its cap (a mesh larger than the GPU takes in one buffer). Estimates: as many objects as are
/// marked, and a fifth of the canvas's cap, since each anchor is dashed many times over.
pub const PENDING_OUTLINES_MAX: usize = MARKED_OBJECTS_MAX;
pub const PENDING_ANCHORS_MAX: usize = 20_000;
/// The most dashes drawn in a frame, whatever the zoom. An estimate: a few outlines the size of the
/// window.
pub const PENDING_DASHES_MAX: usize = 20_000;

/// The selected objects' ends for (document, plan generation, revision): the revision moves with
/// the selection, so a selection change finds them again and nothing else does. And what is dashed
/// for those changed since the plan shown, found for the same.
#[derive(Default)]
pub struct Ends {
    of: Option<(EndsKey, Arc<Vec<ObjectEnds>>)>,
    pending: Option<(PendingKey, Arc<Pending>)>,
}

/// What is dashed for the selected objects the stitches shown aren't of yet.
enum Pending {
    /// Their outlines, document points, each in its layer's colour.
    Outlines(Vec<(Color32, BezPath)>),
    /// Too many, or too large to outline: the box they fall in, in the first one's layer colour.
    Bounds(Color32, Rect),
}

/// What the ends are found for: the document's uid, the plan's generation, and the objects selected
/// (not the revision, which moves with every frame of a drag: the plan shown is the same).
type EndsKey = (u64, u64, Vec<NodeId>);
/// What the dashed outlines are found for: the document's uid, the plan's generation, the revision
/// (they are of the objects as they are now).
type PendingKey = (u64, u64, u64);

/// Whether the canvas draws only the selection's frame and the key object's outline: the Selection
/// tool, or the embroidery tool (its handles over the stitches, no anchors), in the stitch view.
pub fn frame_only(app: &VectorcraftApp) -> bool {
    app.embroidery.layer.drawn() && (app.session.tool_id() == "selection" || super::handles::active(app))
}

/// The active document's selected objects' ends in the plan shown, found again only when the
/// selection or the plan changes.
fn ends(app: &mut VectorcraftApp) -> Option<Arc<Vec<ObjectEnds>>> {
    let st = app.session.active()?;
    let shown = app.embroidery.preview.shown(st.uid)?;
    if let Some(((uid, generation, selected), ends)) = &app.embroidery.ends.of
        && (*uid, *generation) == (st.uid, shown.planned.generation)
        && *selected == st.selection.objects
    {
        return Some(ends.clone());
    }
    let selected: Vec<NodeId> = st.selection.objects.clone();
    let ends = Arc::new(shown.planned.scene().ends_of(&st.doc, &selected));
    app.embroidery.ends.of = Some(((st.uid, shown.planned.generation, selected), ends.clone()));
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
        Some(((uid, g, selected), ends)) if (*uid, *g) == (st.uid, generation) && *selected == st.selection.objects => {
            ends.iter().filter(|e| e.units.is_empty()).count()
        }
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

/// What is dashed for the active document's selected objects (`ends`) that changed since the plan
/// shown, found again only when the selection or the plan changes.
fn pending_outlines(app: &mut VectorcraftApp, ends: &[ObjectEnds]) -> Option<Arc<Pending>> {
    let st = app.session.active()?;
    let shown = app.embroidery.preview.shown(st.uid)?;
    let key = (st.uid, shown.planned.generation, st.revision);
    if let Some((k, pending)) = &app.embroidery.ends.pending
        && *k == key
    {
        return Some(pending.clone());
    }
    let nodes: Vec<&Node> = ends.iter().filter(|e| changed(&st.doc, &shown.document, e.object)).filter_map(|e| st.doc.node(e.object)).collect();
    let pending = if nodes.len() <= PENDING_OUTLINES_MAX && anchors_within(&nodes, PENDING_ANCHORS_MAX) {
        let symbols = st.doc.symbol_boxes();
        Pending::Outlines(nodes.iter().map(|n| (layer_color(&st.doc, n.id), crate::canvas::node_outline(n, &symbols))).collect())
    } else {
        let ids: Vec<NodeId> = nodes.iter().map(|n| n.id).collect();
        match (ids.first(), st.doc.bounds_of(&ids, false)) {
            (Some(first), Some(b)) => Pending::Bounds(layer_color(&st.doc, *first), b),
            _ => Pending::Outlines(Vec::new()),
        }
    };
    let pending = Arc::new(pending);
    app.embroidery.ends.pending = Some((key, pending.clone()));
    Some(pending)
}

/// Whether `nodes` and what they hold have at most `max` anchors in all (a path's anchors, one for
/// anything else).
fn anchors_within(nodes: &[&Node], max: usize) -> bool {
    let mut left = max;
    let mut over = false;
    for n in nodes {
        n.walk(&mut |c| {
            let k = match &c.kind {
                NodeKind::Path { path, .. } => path.anchor_count(),
                _ => 1,
            };
            match left.checked_sub(k) {
                Some(l) => left = l,
                None => over = true,
            }
        });
        if over {
            return false;
        }
    }
    true
}

/// Dash `line` (screen points) with `stroke`, `pattern` its dashes' and gaps' lengths, where it
/// crosses `clip`, at most `budget` dashes, taken from it: a long line far outside the canvas costs
/// nothing.
pub(super) fn dash(p: &egui::Painter, line: &[Pos2], stroke: Stroke, pattern: (f32, f32), clip: egui::Rect, budget: &mut usize) {
    let (dash, period) = (f64::from(pattern.0), f64::from(pattern.0 + pattern.1));
    // How far along the line this segment starts, folded into one period (the dashes go on from one
    // segment into the next).
    let mut phase = 0.0_f64;
    for w in line.windows(2) {
        let [a, b] = w else { continue };
        let len = f64::from(a.distance(*b));
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        if let Some((t0, t1)) = clip_segment(*a, *b, clip) {
            let (x0, x1) = (t0 * len, t1 * len);
            // In f64: a segment from far off the canvas would lose its dashes' places in f32.
            let [ax, ay, bx, by] = [a.x, a.y, b.x, b.y].map(f64::from);
            let (dx, dy) = (bx - ax, by - ay);
            let at = |x: f64| pos2((ax + dx * x / len) as f32, (ay + dy * x / len) as f32);
            // The dashes over [x0, x1] of the segment, the first one starting at or before x0.
            let mut d0 = x0 - (x0 + phase).rem_euclid(period);
            while d0 < x1 {
                let (u0, u1) = (d0.max(x0), (d0 + dash).min(x1));
                if u1 > u0 {
                    let Some(left) = budget.checked_sub(1) else { return };
                    *budget = left;
                    p.line_segment([at(u0), at(u1)], stroke);
                }
                d0 += period;
            }
        }
        phase = (phase + len).rem_euclid(period);
    }
}

/// The part of segment `a`–`b` inside `clip` (Liang–Barsky) → where it enters and leaves, as
/// fractions of the segment; `None` when it misses.
fn clip_segment(a: Pos2, b: Pos2, clip: egui::Rect) -> Option<(f64, f64)> {
    let [ax, ay, bx, by] = [a.x, a.y, b.x, b.y].map(f64::from);
    let [x0, y0, x1, y1] = [clip.min.x, clip.min.y, clip.max.x, clip.max.y].map(f64::from);
    let (dx, dy) = (bx - ax, by - ay);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (q, r) in [(-dx, ax - x0), (dx, x1 - ax), (-dy, ay - y0), (dy, y1 - ay)] {
        if q == 0.0 {
            if r < 0.0 {
                return None;
            }
        } else {
            let t = r / q;
            if q < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 < t1).then_some((t0, t1))
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
    let pending = pending_outlines(app, &ends);
    let Some(st) = app.session.active() else { return };
    let t = Tokens::get(painter.ctx());
    // A stroke's width past the canvas, so that a dash on its edge isn't cut.
    let clip = painter.clip_rect().expand(1.0);
    let mut budget = PENDING_DASHES_MAX;
    match pending.as_deref() {
        Some(Pending::Outlines(outlines)) => {
            for (color, outline) in outlines {
                for line in crate::canvas::to_screen_path(outline, xf) {
                    dash(painter, &line, Stroke::new(1.0, *color), (DASH, GAP), clip, &mut budget);
                }
            }
        }
        Some(Pending::Bounds(color, b)) => {
            let mut corners = xf.quad(*b);
            corners.extend(corners.first().copied());
            dash(painter, &corners, Stroke::new(1.0, *color), (DASH, GAP), clip, &mut budget);
        }
        None => {}
    }
    // The embroidery tool draws its own handles in their place.
    if super::handles::active(app) {
        return;
    }
    let marked: Vec<&ObjectEnds> = ends.iter().filter(|e| !e.units.is_empty()).take(MARKED_OBJECTS_MAX).collect();
    // Labelled when one object is marked: an object selected with it that sews nothing (a text
    // left out, an empty path) takes nothing from it.
    let labelled = marked.len() == 1;
    for e in &marked {
        let color = layer_color(&st.doc, e.object);
        if let Some(exit) = e.exit {
            let at = xf.to_screen(exit);
            exit_marker(painter, at, color);
            if labelled {
                label(painter, at + vec2(-LABEL_OFFSET, LABEL_OFFSET), Align2::RIGHT_TOP, crate::et!("Exit"), color, Stroke::NONE);
            }
        }
        if let Some(entry) = e.entry {
            let at = xf.to_screen(entry);
            entry_marker(painter, at, t.accent);
            if labelled {
                label(painter, at + vec2(LABEL_OFFSET, -LABEL_OFFSET), Align2::LEFT_BOTTOM, crate::et!("Entry"), t.accent, Stroke::NONE);
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

/// The exit ⊕: a white face with a ring and a plus in `color`, darkened as far as it takes to stand
/// out from the white face (a light layer colour, yellow or cyan, would hardly show).
fn exit_marker(p: &egui::Painter, at: Pos2, color: Color32) {
    let color = super::marks::readable(color, &[Color32::WHITE], false);
    let r = EXIT_MARKER_PT / 2.0;
    p.circle(at, r, Color32::WHITE, Stroke::new(1.5, color));
    let arm = r * 0.6;
    let stroke = Stroke::new(1.5, color);
    p.line_segment([at - vec2(arm, 0.0), at + vec2(arm, 0.0)], stroke);
    p.line_segment([at - vec2(0.0, arm), at + vec2(0.0, arm)], stroke);
}

/// A pill reading `text` on `color`, anchored at `at` by `align`: in black or white, whichever reads
/// better on it.
pub(super) fn label(p: &egui::Painter, at: Pos2, align: Align2, text: &str, color: Color32, edge: Stroke) {
    let ink = super::marks::text_on(color);
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(LABEL_SIZE), ink);
    let size = galley.size() + vec2(2.0 * LABEL_PAD, LABEL_PAD);
    let rect = align.anchor_size(at, size);
    p.rect(rect, CornerRadius::same(u8::MAX), color, edge, StrokeKind::Outside);
    p.galley(rect.center() - galley.size() / 2.0, galley, ink);
}

#[cfg(test)]
mod tests;
