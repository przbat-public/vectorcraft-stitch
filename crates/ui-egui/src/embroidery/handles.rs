//! The embroidery tool's handles on the canvas, in the art view and the stitch view alike: the list
//! the tool catches ([`vectorcraft_engine::Session::embroidery_handles`]), drawn where it catches
//! them, so that what is drawn is what is caught.
//!
//! - **Look:** an entry ◆ in the theme's accent, an exit ⊕ in its layer's colour (each darkened as
//!   far as it takes to stand out from the light edge, [`entry_color`], [`exit_color`]); solid for a point
//!   set, hollow for one left to the planner, dimmed while the plan shown is of the object as it was;
//!   each with a two-tone edge (dark outside, light inside), which reads on any fabric. A tatami
//!   fill's rows' angle is a dashed two-tone arrow through its centre, its head ▲ in the accent
//!   (solid set, hollow Auto) with its degrees, as the interface language writes numbers.
//! - **Hover and focus:** the handle under the pointer is drawn larger ([`HOVER_SCALE`]) and
//!   labelled; the one the keys act on has a ring ([`FOCUS_RING_PT`]).
//! - **A point set where the stitches can't start** (a fill's entry on its outline, the stitches half a
//!   row spacing inside it) is joined by a hairline to a dot where they do, past [`LEADER_MIN_PT`].
//! - **While dragging,** the handle follows the pointer in the frame it moves (moved by the drag, not
//!   set at the pointer), a dot marks where its point is stored when they lie apart, and a dashed
//!   ghost joins that to where the needle comes from (an entry) or goes next (an exit), with the jump's length
//!   and whether the thread will be cut; on a design slow to plan, the status bar says when the
//!   stitches will catch up (on a pause, [`super::stitch_preview::PAUSE_TO_PLAN_SECS`], or the
//!   release).
//! - **One part at a time,** the tool's `part`, kept in step with the Embroidery panel's Object tab
//!   ([`sync_part`]): the tab's Fill | Stroke switch sets it, and so does a press on a part on the
//!   canvas or an agent's `tool.setOption {part}`, which the tab then follows.
//! - **A plan is asked for** while the tool is active ([`super::stitch_preview::StitchPreview::want_plan_alone`]),
//!   so Auto points show in the art view too, without turning the status bar's counters on.
//!
//! The handles are found again only when the document, the selection, the plan shown or the tool's
//! state changes ([`Cache`]), never on an idle frame.

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Shape, Stroke, StrokeKind, pos2, vec2};
use serde_json::Value;
use vectorcraft_engine::cmd::embroidery::scene::{MARK_DARK, MARK_LIGHT};
use vectorcraft_tools::embroidery::{HANDLED_OBJECTS_MAX, Handle, Kind, State};

use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::panels::embroidery::object::Part;
use crate::theme::Tokens;

/// How much larger the handle under the pointer is drawn.
pub const HOVER_SCALE: f32 = 1.25;
/// How far out from a focused handle its ring is drawn, points, and the ring's width.
pub const FOCUS_RING_PT: f32 = 4.0;
const FOCUS_RING_WIDTH: f32 = 1.5;
/// The two-tone edge: the dark outer line's width and the light inner one's, points.
const EDGE_DARK: f32 = 3.0;
const EDGE_LIGHT: f32 = 1.5;
/// How faint a handle from a plan of the object as it was is drawn.
const STALE_ALPHA: f32 = 0.45;
/// How far apart on screen a point set and where its stitches start must lie to be joined,
/// points: farther than a handle's half size, so the line shows past it.
pub const LEADER_MIN_PT: f32 = 8.0;
/// The dot where the stitches start, at the leader's end: its radius, points.
const PLANNED_DOT_PT: f32 = 2.5;
/// The outline of the object a press would select, points wide (the selection tools' hover outline).
const HOVER_OUTLINE_PT: f32 = 1.5;

/// The ghost's dashes and the gaps between them, points.
const GHOST_DASH: f32 = 5.0;
const GHOST_GAP: f32 = 3.0;
/// The decimals the ghost's jump is said with: a tenth of a millimetre, the formats' grid.
const JUMP_DECIMALS: usize = 1;
/// The decimals an angle is said with: a tenth of a degree, as the Object tab's angle field.
const ANGLE_DECIMALS: usize = 1;

/// The labels' size, their room either side, and how far they sit from their handle, points (the
/// passive markers' labels').
const LABEL_SIZE: f32 = 11.0;
const LABEL_PAD: f32 = 5.0;
const LABEL_OFFSET: f32 = 10.0;

/// What the handles are found for ([`Cache`]).
type Key = (u64, u64, Option<u64>, [u64; 3], String);

/// What the handles are found for, and what was found: the document's uid and revision, the
/// generation of the plan shown, the view's zoom and turn (an arrow's head and an exit moved apart
/// from its entry are placed in screen pixels), and the tool's options (its part, hover, focus and
/// drag).
#[derive(Default)]
pub struct Cache {
    key: Option<Key>,
    handles: Vec<Handle>,
    options: Value,
    /// How many times the handles were found (the tests count them).
    pub(crate) found: usize,
    /// The document and the generation of the plan last handed to the session as the one drawn
    /// ([`hand_shown`]); `None` while the tool is put away.
    handed: Option<(u64, u64)>,
    /// The tool's part and the Object tab's as last kept in step ([`sync_part`]); `None` while the
    /// tool is put away.
    parts: Option<(Option<Part>, Part)>,
    /// The document whose selection was last found to hold more objects than show their points
    /// ([`HANDLED_OBJECTS_MAX`]): the status bar said so once, until it holds no more.
    capped: Option<u64>,
}

/// The tool's part and the Object tab's kept in step (the project owner's choice, 2026-10-25: the
/// part is picked with the tab's Fill | Stroke switch or by pressing it on the canvas, and the two
/// stay in step): whichever changed since the frame before sets the other, the tab's when the
/// tool is taken up. The tool's `null` (an agent's: each object's fill, else its stroke) leaves the
/// tab as it is.
fn sync_part(app: &mut VectorcraftApp) {
    fn tool(app: &VectorcraftApp) -> Option<Part> {
        app.session.tool_options()["part"].as_str().and_then(Part::from_id)
    }
    let (now, tab) = (tool(app), app.ui.embroidery_panel.part);
    match app.embroidery.handles.parts {
        Some((was, _)) if now != was => {
            if let Some(part) = now {
                app.ui.embroidery_panel.part = part;
            }
        }
        Some((_, was)) if tab == was => {}
        _ => app.session.set_tool_option("part", &serde_json::json!(tab.id())),
    }
    app.embroidery.handles.parts = Some((tool(app), app.ui.embroidery_panel.part));
}

/// The plan the stitch preview shows of the active document handed to the session, when it is new:
/// the tool's Auto points and its picking read it, so what is drawn is what is caught.
fn hand_shown(app: &mut VectorcraftApp) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some(shown) = app.embroidery.preview.shown(uid) else { return };
    let now = (uid, shown.planned.generation);
    if app.embroidery.handles.handed != Some(now) {
        let (document, planned) = (shown.document.clone(), shown.planned.clone());
        app.session.show_embroidery_plan(uid, document, planned);
        app.embroidery.handles.handed = Some(now);
    }
}

/// The handles and the tool's options, found again only when what they are found for changed.
fn current(app: &mut VectorcraftApp) -> Option<(Vec<Handle>, Value)> {
    let st = app.session.active()?;
    let (uid, revision) = (st.uid, st.revision);
    let generation = app.embroidery.preview.shown(uid).map(|s| s.planned.generation);
    let view = app.view_info();
    let right = view.screen.map_or(vectorcraft_geom::Vec2::ZERO, |s| s.right);
    let frame = [view.zoom.to_bits(), right.x.to_bits(), right.y.to_bits()];
    let key = (uid, revision, generation, frame, app.session.tool_options().to_string());
    if app.embroidery.handles.key.as_ref() != Some(&key) {
        // More objects selected than show their points: said once, as the selection changes.
        let more =
            vectorcraft_doc::embroidery::takers::first_takers(&st.doc, &st.selection.objects, HANDLED_OBJECTS_MAX + 1).len() > HANDLED_OBJECTS_MAX;
        let (handles, options) = app.session.embroidery_handles(view);
        let cache = &mut app.embroidery.handles;
        (cache.key, cache.handles, cache.options) = (Some(key), handles, options);
        cache.found += 1;
        let said = cache.capped == Some(uid);
        cache.capped = more.then_some(uid);
        if more && !said {
            app.status(crate::etn!(
                HANDLED_OBJECTS_MAX as u64,
                "Entry and exit points show for the first {count} object selected",
                "Entry and exit points show for the first {count} objects selected"
            ));
        }
    }
    let cache = &app.embroidery.handles;
    Some((cache.handles.clone(), cache.options.clone()))
}

/// Whether the embroidery tool is the active tool: its handles show, and the passive markers give
/// way to them.
pub fn active(app: &VectorcraftApp) -> bool {
    app.session.tool_id() == vectorcraft_tools::embroidery::ID
}

/// Draw the tool's handles over the canvas (both views), and ask for a plan while it is active.
pub fn draw(app: &mut VectorcraftApp, painter: &egui::Painter, xf: &Xf) {
    if !active(app) {
        // The tool put away (the session drops the answers it kept): the plan drawn is handed again,
        // and the tab's part leads, when it is taken up.
        app.embroidery.handles.handed = None;
        app.embroidery.handles.parts = None;
        return;
    }
    hand_shown(app);
    // Auto points need a plan: asked for whatever the view, the counters left as they are.
    app.embroidery.preview.want_plan_alone();
    sync_part(app);
    if !app.ui.view.edges || app.ui.screen_mode >= 3 {
        return;
    }
    let Some((handles, options)) = current(app) else { return };
    // A drag on a design slow to plan: when the stitches will catch up.
    if options["drag"].is_object()
        && let Some(uid) = app.session.active().map(|st| st.uid)
        && !app.embroidery.preview.live(uid)
    {
        app.status(if cfg!(target_arch = "wasm32") {
            crate::et!("Stitches update when you let go.")
        } else {
            crate::et!("Stitches update when you pause or let go.")
        });
    }
    let Some(st) = app.session.active() else { return };
    let t = Tokens::get(painter.ctx());
    let same = |a: &Value, h: &Handle| {
        a["kind"].as_str() == Some(h.id.kind.id())
            && a["id"].as_u64() == Some(h.id.owner.part().0.0)
            && a["part"].as_str() == Some(h.id.owner.part().1.id())
    };
    // The object a press would select: outlined in its layer's colour.
    if let Some(object) = options["hoverObject"].as_u64().map(vectorcraft_doc::NodeId).filter(|_| !options["drag"].is_object())
        && let Some(outline) = outline_of(&st.doc, object)
    {
        let [r, g, b] = st.doc.layer_color(object);
        for line in crate::canvas::to_screen_path(&outline, xf) {
            painter.add(Shape::line(line, Stroke::new(HOVER_OUTLINE_PT, Color32::from_rgb(r, g, b))));
        }
    }
    let drag = &options["drag"];
    let dragged_at = drag["at"].as_array().and_then(|a| Some(vectorcraft_geom::Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
    let objects = handles.iter().map(|h| h.id.owner.part().0).collect::<std::collections::BTreeSet<_>>().len();
    // Arrows first, then exits, then entries: an entry over its exit, as the passive markers draw
    // them, and the points over an arrow.
    let mut order: Vec<&Handle> = handles.iter().collect();
    order.sort_by_key(|h| match h.id.kind {
        Kind::Angle => 0,
        Kind::Exit => 1,
        Kind::Entry => 2,
    });
    for h in order {
        let dragged = same(drag, h);
        let point = if dragged { dragged_at.unwrap_or(h.at) } else { h.at };
        let at = xf.to_screen(point);
        let hovered = same(&options["hover"], h) || dragged;
        let focused = same(&options["focus"], h);
        let alpha = if h.state == State::Stale { STALE_ALPHA } else { 1.0 };
        let color = match h.id.kind {
            Kind::Entry | Kind::Angle => entry_color(&t),
            Kind::Exit => exit_color(st.doc.layer_color(h.id.owner.part().0)),
        };
        // A point set where the stitches can't start: joined to where they do.
        if let (State::Set, Some(planned), false) = (h.state, h.planned, dragged) {
            let to = xf.to_screen(planned);
            if to.distance(at) > LEADER_MIN_PT {
                painter.line_segment([at, to], Stroke::new(1.0, color.gamma_multiply(alpha)));
                painter.circle(to, PLANNED_DOT_PT, color.gamma_multiply(alpha), Stroke::new(1.0, dark()));
            }
        }
        // An exit moved apart from the entry it lies under: a hairline back to its point.
        if h.at != h.point && !dragged {
            painter.line_segment([at, xf.to_screen(h.point)], Stroke::new(1.0, dark().gamma_multiply(alpha)));
        }
        // While dragging, where the point is stored (taken where the stitches can start or end, as
        // the last preview took it): a dot there, joined to the handle that follows the pointer when
        // they lie apart; and the jump to or from the neighbour along the thread to there, dashed,
        // and how long.
        let stored = xf.to_screen(h.point);
        if dragged && h.id.kind != Kind::Angle && stored.distance(at) > LEADER_MIN_PT {
            painter.line_segment([at, stored], Stroke::new(1.0, color.gamma_multiply(alpha)));
            painter.circle(stored, PLANNED_DOT_PT, color.gamma_multiply(alpha), Stroke::new(1.0, dark()));
        }
        // The point the keys last stepped to or nudged shows its jump too.
        let keyed = focused && options["keyed"].as_bool() == Some(true) && h.id.kind != Kind::Angle;
        if let (true, Some(neighbour)) = (dragged || keyed, h.neighbour) {
            let from = xf.to_screen(neighbour);
            painter.extend(Shape::dashed_line(&[from, stored], Stroke::new(1.5, dark()), GHOST_DASH, GHOST_GAP));
            painter.extend(Shape::dashed_line(&[from, stored], Stroke::new(0.75, light()), GHOST_DASH, GHOST_GAP));
            let mm = neighbour.distance(h.point) / vectorcraft_engine::cmd::embroidery::mm_to_pt(1.0);
            let value = super::numbers::decimal(crate::i18n::current(), mm, JUMP_DECIMALS);
            let cut = h.trim.is_some_and(|trim| neighbour.distance(h.point) > trim);
            let words = super::text::fill(if cut { crate::et!("jump {value} mm, cut") } else { crate::et!("jump {value} mm") }, &[("value", &value)]);
            label(painter, from + (stored - from) / 2.0, Align2::CENTER_BOTTOM, &words, if cut { t.warning } else { dark() });
        }
        let scale = if hovered { HOVER_SCALE } else { 1.0 };
        let size = h.id.kind.size_px() as f32 * scale;
        if focused {
            painter.circle_stroke(at, size / 2.0 + FOCUS_RING_PT, Stroke::new(FOCUS_RING_WIDTH, t.accent));
        }
        let solid = h.state == State::Set;
        match (h.id.kind, h.arrow) {
            (Kind::Entry, _) => entry(painter, at, size, color, solid, alpha),
            (Kind::Exit, _) => exit(painter, at, size, color, solid, alpha),
            (Kind::Angle, Some(arrow)) => {
                // While dragging, the head where the drag has turned it, the tail opposite.
                let centre = xf.to_screen(arrow.centre);
                head(painter, centre, at, size, color, solid);
            }
            (Kind::Angle, None) => {}
        }
        if hovered || focused || objects == 1 {
            let lang = crate::i18n::current();
            let degrees = drag["angle"].as_f64().filter(|_| dragged).or(h.arrow.map(|a| a.degrees));
            // As the Object tab's angle field writes it.
            let angle = degrees.map(|d| super::fields::format(lang, "°", ANGLE_DECIMALS).show(d));
            let words = match h.id.kind {
                Kind::Entry => crate::et!("Entry"),
                Kind::Exit => crate::et!("Exit"),
                Kind::Angle => angle.as_deref().unwrap_or_default(),
            };
            let (offset, align) = match h.id.kind {
                Kind::Entry => (vec2(LABEL_OFFSET, -LABEL_OFFSET), Align2::LEFT_BOTTOM),
                Kind::Exit => (vec2(-LABEL_OFFSET, LABEL_OFFSET), Align2::RIGHT_TOP),
                // Beyond the head, along the arrow.
                Kind::Angle => {
                    let out = h.arrow.map_or(vec2(1.0, 0.0), |a| (at - xf.to_screen(a.centre)).normalized());
                    (out * (LABEL_OFFSET + size / 2.0), if out.x < 0.0 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER })
                }
            };
            if !words.is_empty() {
                label(painter, at + offset * scale, align, words, color);
            }
        }
    }
}

/// `object`'s outline, document points: a path's, or a compound path's members'.
fn outline_of(doc: &vectorcraft_doc::Document, object: vectorcraft_doc::NodeId) -> Option<vectorcraft_geom::BezPath> {
    let path = vectorcraft_doc::embroidery::outline::outline(doc.node(object)?);
    (!path.elements().is_empty()).then_some(path)
}

/// [`outline_of`] flattened, for the hand-run gate's drag along an object's outline.
#[cfg(test)]
pub(crate) fn tests_outline(doc: &vectorcraft_doc::Document, object: vectorcraft_doc::NodeId) -> Vec<vectorcraft_geom::Point> {
    let mut pts = Vec::new();
    if let Some(bp) = outline_of(doc, object) {
        vectorcraft_geom::kurbo::flatten(bp.iter(), 0.5, |el| match el {
            vectorcraft_geom::PathEl::MoveTo(p) | vectorcraft_geom::PathEl::LineTo(p) => pts.push(p),
            _ => {}
        });
    }
    pts
}

/// An entry's and an angle arrow's colour: the theme's accent, darkened only as far as it takes to
/// stand out from the handles' light inner edge (a set point's face against an Auto one's).
pub(super) fn entry_color(t: &Tokens) -> Color32 {
    super::marks::readable(t.accent, &[light()], false)
}

/// An exit's colour: its layer's `rgb`, darkened only as far as it takes to stand out from the
/// handles' light inner edge.
pub(super) fn exit_color([r, g, b]: [u8; 3]) -> Color32 {
    super::marks::readable(Color32::from_rgb(r, g, b), &[light()], false)
}

pub(super) fn dark() -> Color32 {
    Color32::from_rgb(MARK_DARK[0], MARK_DARK[1], MARK_DARK[2])
}

pub(super) fn light() -> Color32 {
    Color32::from_rgb(MARK_LIGHT[0], MARK_LIGHT[1], MARK_LIGHT[2])
}

/// An entry ◆ `size` points across at `at`: filled with `color` when `solid`, else a light face with
/// a `color` outline; a two-tone edge either way.
fn entry(p: &egui::Painter, at: Pos2, size: f32, color: Color32, solid: bool, alpha: f32) {
    let diamond = |r: f32| vec![pos2(at.x, at.y - r), pos2(at.x + r, at.y), pos2(at.x, at.y + r), pos2(at.x - r, at.y)];
    let r = size / 2.0;
    p.add(Shape::convex_polygon(diamond(r), Color32::TRANSPARENT, Stroke::new(EDGE_DARK, dark().gamma_multiply(alpha))));
    let face = if solid { color } else { light() };
    p.add(Shape::convex_polygon(diamond(r), face.gamma_multiply(alpha), Stroke::new(EDGE_LIGHT, light().gamma_multiply(alpha))));
    if !solid {
        p.add(Shape::closed_line(diamond(r - EDGE_LIGHT * 1.5), Stroke::new(EDGE_LIGHT, color.gamma_multiply(alpha))));
    }
}

/// An exit ⊕ `size` points across at `at`: a `color` face with a light plus when `solid`, else a
/// light face with a `color` ring and plus; a two-tone edge either way.
fn exit(p: &egui::Painter, at: Pos2, size: f32, color: Color32, solid: bool, alpha: f32) {
    let r = size / 2.0;
    p.circle_stroke(at, r, Stroke::new(EDGE_DARK, dark().gamma_multiply(alpha)));
    let (face, ink) = if solid { (color, light()) } else { (light(), color) };
    p.circle(at, r, face.gamma_multiply(alpha), Stroke::new(EDGE_LIGHT, light().gamma_multiply(alpha)));
    if !solid {
        p.circle_stroke(at, r - EDGE_LIGHT * 1.5, Stroke::new(EDGE_LIGHT, color.gamma_multiply(alpha)));
    }
    let arm = r * 0.55;
    let stroke = Stroke::new(EDGE_LIGHT, ink.gamma_multiply(alpha));
    p.line_segment([at - vec2(arm, 0.0), at + vec2(arm, 0.0)], stroke);
    p.line_segment([at - vec2(0.0, arm), at + vec2(0.0, arm)], stroke);
}

/// An angle arrow through `centre` (screen), its head ▲ `size` points across at `at` pointing away
/// from the centre: the shaft dashed in the two tones, the tail as far the other way; the head
/// filled with `color` when `solid`, else a light face with a `color` outline.
fn head(p: &egui::Painter, centre: Pos2, at: Pos2, size: f32, color: Color32, solid: bool) {
    let out = at - centre;
    if out.length() <= f32::EPSILON {
        return;
    }
    let d = out.normalized();
    let tail = centre - out;
    let r = size / 2.0;
    let base = at - d * r;
    for (width, ink) in [(EDGE_DARK, dark()), (EDGE_LIGHT, light())] {
        p.extend(Shape::dashed_line(&[tail, base], Stroke::new(width, ink), GHOST_DASH, GHOST_GAP));
    }
    let across = vec2(-d.y, d.x) * r;
    let triangle = vec![at + d * r, base + across, base - across];
    p.add(Shape::convex_polygon(triangle.clone(), Color32::TRANSPARENT, Stroke::new(EDGE_DARK, dark())));
    let face = if solid { color } else { light() };
    p.add(Shape::convex_polygon(triangle.clone(), face, Stroke::new(EDGE_LIGHT, light())));
    if !solid {
        p.add(Shape::closed_line(triangle, Stroke::new(1.0, color)));
    }
}

/// A pill reading `text` on `color`, anchored at `at` by `align`, in black or white, whichever
/// reads better on it.
fn label(p: &egui::Painter, at: Pos2, align: Align2, text: &str, color: Color32) {
    let ink = super::marks::text_on(color);
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(LABEL_SIZE), ink);
    let size = galley.size() + vec2(2.0 * LABEL_PAD, LABEL_PAD);
    let rect = align.anchor_size(at, size);
    p.rect(rect, CornerRadius::same(u8::MAX), color, Stroke::new(1.0, dark()), StrokeKind::Outside);
    p.galley(rect.center() - galley.size() / 2.0, galley, ink);
}
