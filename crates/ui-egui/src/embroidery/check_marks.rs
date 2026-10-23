//! The check's marks on the canvas: a ring round each listed hit the Embroidery panel's Check tab
//! has found, with its severity's mark on the ring, in both the art and the stitch view.
//!
//! - **When**: while the Check tab shows, for the rules that found errors or warnings, and a
//!   note's rule only while it is the tab's focus (notes are many: a fill's row ends are often
//!   shorter than the needle is thick); never on a check of an older plan than the document's
//!   (a mark would point where the stitches no longer are), never in Presentation Mode, with
//!   the edges hidden or while a tool transforms the selection. View › Stitch Preview Options ›
//!   Show Check Marks turns them off for the session.
//! - **Where**: a hit on a stitch is ringed round the stitch; a hit on a spot (layers), round its
//!   point. Rules on objects' settings, art left out and threads point at no stitch: Show selects
//!   their objects instead.
//! - **How**: a black ring on a white one, so that it reads on any fabric, thread or art; the
//!   mark on a white disc at its upper right; the focus's hit ringed again in the accent colour.
//!   At most [`MAX_DRAWN_MARKS`], the focus's first (no clustering: a hard cap).
//! - Hovering a mark tells its rule and hit, as the tab words them.

use egui::{Color32, Pos2, Stroke, vec2};
use vectorcraft_engine::cmd::embroidery::mm_to_pt;
use vectorcraft_geom::Point;

use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::embroidery::marks::{self, Mark};
use crate::panels::embroidery::check;
use crate::theme::Tokens;

/// The most marks drawn at once: a cap on the drawing however many hits are listed (up to 100 for
/// each of 15 rules). An estimate: more would hide the design under rings.
pub const MAX_DRAWN_MARKS: usize = 300;
/// The smallest ring's radius, and the room a ring leaves round its stitch, screen points.
/// Estimates: a ring stays visible round a stitch a few pixels long.
pub const RING_RADIUS_PT: f32 = 9.0;
const RING_ROOM_PT: f32 = 4.0;
/// The white ring's width under the black one's, screen points: the two read on any background.
const WHITE_EDGE_PT: f32 = 4.0;
const BLACK_EDGE_PT: f32 = 1.5;
/// The accent ring round the focus's hit, screen points.
const FOCUS_EDGE_PT: f32 = 2.5;
/// The severity mark on a ring, and the white disc round it, screen points.
pub const BADGE_PT: f32 = 12.0;
const BADGE_HALO_PT: f32 = 2.0;

/// A mark: hit `hit` (its place among its rule's listed hits) of rule `rule`, round `centre`
/// (document points), `half` the length of its stitch (0 for a spot).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CanvasMark {
    pub(crate) rule: String,
    pub(crate) hit: usize,
    pub(crate) mark: Mark,
    pub(crate) centre: Point,
    pub(crate) half: f64,
    pub(crate) focused: bool,
}

/// The marks to draw in pass `pass` (as the module says when and where), the focus's first.
pub(crate) fn marks(app: &VectorcraftApp, pass: u64) -> Vec<CanvasMark> {
    let state = &app.embroidery.check;
    // Not while the tab isn't drawn: in this pass or the last.
    if state.hide_marks || state.drawn_pass.is_none_or(|p| pass > p.saturating_add(1)) {
        return Vec::new();
    }
    let Some((uid, reply)) = check::current_check(app) else { return Vec::new() };
    let focus = state.focus.as_ref().filter(|f| f.uid == uid);
    let mut rules: Vec<&serde_json::Value> = check::ordered(reply)
        .into_iter()
        .filter(|r| {
            let shown = focus.is_some_and(|f| r["id"] == f.rule.as_str());
            match check::mark_of(r) {
                Mark::Error | Mark::Warning => true,
                Mark::Info => shown,
                Mark::Passed => false,
            }
        })
        .collect();
    rules.sort_by_key(|r| !focus.is_some_and(|f| r["id"] == f.rule.as_str()));
    let mut out = Vec::new();
    for rule in rules {
        let id = rule["id"].as_str().unwrap_or_default();
        let mark = check::mark_of(rule);
        for (k, hit) in check::listed(rule).iter().enumerate() {
            if out.len() >= MAX_DRAWN_MARKS {
                return out;
            }
            let points: Vec<Point> = check::hit_points(hit).into_iter().map(|p| Point::new(mm_to_pt(p.x), mm_to_pt(p.y))).collect();
            let (centre, half) = match points.as_slice() {
                [from, to] => (from.midpoint(*to), from.distance(*to) / 2.0),
                [at] => (*at, 0.0),
                _ => continue,
            };
            let focused = focus.is_some_and(|f| f.rule == id && f.hit == Some(k));
            out.push(CanvasMark { rule: id.to_string(), hit: k, mark, centre, half, focused });
        }
    }
    out
}

/// A mark's ring on screen: its centre and radius.
fn ring(m: &CanvasMark, xf: &Xf) -> (Pos2, f32) {
    let r = (m.half * xf.zoom) as f32 + RING_ROOM_PT;
    (xf.to_screen(m.centre), if r.is_finite() { r.max(RING_RADIUS_PT) } else { RING_RADIUS_PT })
}

/// Draw the marks over the canvas, and the hovered one's words.
pub fn draw(app: &VectorcraftApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf) {
    if !app.ui.view.edges || app.ui.screen_mode >= 3 || app.session.tool_transforming() {
        return;
    }
    let all = marks(app, ctx.cumulative_pass_nr());
    if all.is_empty() {
        return;
    }
    let t = Tokens::get(ctx);
    let pointer = ctx.pointer_hover_pos().filter(|p| painter.clip_rect().contains(*p));
    let mut hovered: Option<(f32, &CanvasMark)> = None;
    // The focus's marks come first in the list: drawn last, on top.
    for m in all.iter().rev() {
        let (c, r) = ring(m, xf);
        painter.circle_stroke(c, r, Stroke::new(WHITE_EDGE_PT, Color32::WHITE));
        painter.circle_stroke(c, r, Stroke::new(BLACK_EDGE_PT, Color32::BLACK));
        if m.focused {
            painter.circle_stroke(c, r + WHITE_EDGE_PT, Stroke::new(FOCUS_EDGE_PT, t.accent));
        }
        let badge = c + vec2(r, -r) * std::f32::consts::FRAC_1_SQRT_2;
        painter.circle_filled(badge, BADGE_PT / 2.0 + BADGE_HALO_PT, Color32::WHITE);
        painter.circle_stroke(badge, BADGE_PT / 2.0 + BADGE_HALO_PT, Stroke::new(1.0, Color32::BLACK));
        marks::paint_on(painter, badge, BADGE_PT, m.mark, &t, Color32::WHITE);
        if let Some(p) = pointer {
            let d = p.distance(c);
            if d <= r + WHITE_EDGE_PT && hovered.is_none_or(|(best, _)| d < best) {
                hovered = Some((d, m));
            }
        }
    }
    if let Some((_, m)) = hovered
        && let Some(words) = words(app, m)
    {
        let id = egui::Id::new("embroidery-check-mark");
        egui::Tooltip::always_open(ctx.clone(), painter.layer_id(), id, egui::PopupAnchor::Pointer).show(|ui| ui.label(words));
    }
}

/// What a mark's hover says: its severity and rule, then its hit, as the Check tab words them.
fn words(app: &VectorcraftApp, m: &CanvasMark) -> Option<String> {
    let (_, reply) = check::current_check(app)?;
    let rule = check::rule_of(reply, &m.rule)?;
    let lang = crate::i18n::current();
    let format = check::format_label(reply["format"].as_str().unwrap_or_default());
    let doc = app.session.doc().ok().map(|st| st.doc.clone());
    let hit = check::listed(rule).get(m.hit)?;
    let line = check::hit_line(app, doc.as_deref(), &m.rule, hit, lang).unwrap_or_default();
    Some(format!("{}: {}\n{line}", check::severity_name(m.mark), check::title(rule, format, lang)))
}
