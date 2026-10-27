//! Stitch editing: the embroidery tool working inside isolation of an object holding paths of
//! stitches (the project owner's choice, 2026-10-27: „Izolacja, reszta przyciemniona”), so the mode
//! is isolation itself, entered and left as isolation is, with no state of its own beyond the
//! gesture under way. The engine answers which needle points are being edited, from the document
//! as it is ([`super::NeedlePoints`]); every gesture is a stitch command, so agents and the pointer
//! edit alike.
//!
//! - **Selecting** (a selection, no undo step; `embroidery.selectNeedlePoints`): a click on a needle
//!   point selects it alone (on release, when it was selected already and no drag followed),
//!   Shift-click adds or takes it away; a click on a stitch selects its two needle points; a
//!   double-click on a stitch its run; a drag from where no needle point lies draws a marquee whose
//!   needle points are selected at the release (Shift adds); a click there selects nothing. `[` and
//!   `]` step to the needle point before or after along the thread, alone selected (Shift adds it).
//!   The lasso is the Lasso tool's, which selects needle points as it selects anchors in isolation.
//! - **Editing** (one undo step a gesture): a drag of selected needle points moves them in
//!   [`NEEDLE_STEP_MM`] steps (the formats' grid), Shift at 45° steps from Constrain Angle, a preview
//!   at most once a frame ([`crate::Tool::tick`]), and past [`LIVE_DRAG_POINTS_MAX`] selected only
//!   once the drag rests [`PAUSE_PREVIEW_SECS`] or at the release; the arrows move them a step (Shift
//!   ten); Alt-click on a stitch inserts a needle point there; Delete and Backspace delete the
//!   selected ones, their neighbours merging.
//! - **Escape** cancels a drag (or a marquee), then deselects, then (unclaimed) leaves isolation, as
//!   upstream's Escape leaves it (the owner's order).
//! - **Entering:** a double-click on a stitch object with the tool isolates it and shows the stitch
//!   view (`object.isolate`, `view.stitchPreview {on: true}`); a double-click away from the stitches
//!   being edited leaves it (`object.exitIsolation`).

use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId, Selection, Unit};
use vectorcraft_geom::{Point, Rect, Vec2, constrain_angle_from};

use super::{DRAG_START_PX, NeedlePoints, NeedleRef};
use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, ToolContext, ToolKey};

/// The command a drag, the arrows and the Stitches tab's Move by run, and the undo step it makes
/// (the command's own label).
pub const MOVE: &str = "embroidery.moveNeedlePoints";
const MOVE_LABEL: &str = "Move Needle Points";
/// The commands the other gestures run.
const SELECT: &str = "embroidery.selectNeedlePoints";
const INSERT: &str = "embroidery.insertNeedlePoints";
const DELETE: &str = "embroidery.deleteNeedlePoints";

/// The grid a needle-point drag and the arrows move on, millimetres: the formats' 0.1 mm (a DST,
/// PES or JEF record's unit), the project owner's choice (2026-10-27: the arrows by 0.1 mm).
pub const NEEDLE_STEP_MM: f64 = 0.1;
/// The steps in a millimetre ([`NEEDLE_STEP_MM`]'s reciprocal): a move is a whole number of steps
/// divided by it, so that it is the nearest double to a tenth.
const STEPS_PER_MM: f64 = 10.0;
/// How many steps an arrow with Shift moves: 1 mm (the project owner's choice).
pub const SHIFT_STEPS: f64 = 10.0;
/// The steps Shift constrains a drag to, degrees from Constrain Angle: as every Shift-constrained
/// drag in the app.
const SHIFT_ANGLE: f64 = 45.0;
/// Past this many needle points selected, a drag previews only once it rests or at the release
/// (the project owner's choice, 2026-10-27: above 20,000 selected points the stitches refresh on a
/// pause or release).
pub const LIVE_DRAG_POINTS_MAX: usize = 20_000;
/// How long a drag of many needle points rests before it previews, seconds: the stitch preview's
/// pause before it plans a design slow to plan (`PAUSE_TO_PLAN_SECS`, 0.3 s, an estimate).
pub const PAUSE_PREVIEW_SECS: f64 = 0.3;

/// What lies under the pointer while editing: a needle point, or a place on a stitch (the needle
/// point it leaves, and the place, document points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hover {
    Needle(NeedleRef),
    Thread(NeedleRef, Point),
}

/// A drag of needle points: where the press was, the needle point pressed (selected alone at the
/// release when it was selected already and no drag followed), whether it has begun, the move now
/// and the move last previewed (millimetres, on the grid), how long the move has rested since it
/// last changed (seconds), and whether so many needle points are moved that it previews only
/// once it rests.
#[derive(Clone, Debug)]
struct Drag {
    from: Point,
    alone: Option<NeedleRef>,
    begun: bool,
    delta: [f64; 2],
    previewed: Option<[f64; 2]>,
    rested: f64,
    many: bool,
}

/// A marquee: where it started and where it is now (document points), what it does at the release
/// (set or add), and whether it has been dragged.
#[derive(Clone, Copy, Debug)]
struct Marquee {
    from: Point,
    to: Point,
    mode: &'static str,
    dragged: bool,
}

/// The gesture under way while editing stitches, and what the pointer is over.
#[derive(Clone, Debug, Default)]
pub struct Editing {
    pub hover: Option<Hover>,
    /// The needle point `[` and `]` step from: the one last pressed or stepped to.
    pub current: Option<NeedleRef>,
    drag: Option<Drag>,
    marquee: Option<Marquee>,
}

/// Points in a millimetre.
fn pt_per_mm() -> f64 {
    Unit::Millimeters.points()
}

/// `v` points as millimetres on the grid.
fn on_grid(v: f64) -> f64 {
    let steps = (v / pt_per_mm() * STEPS_PER_MM).round();
    if steps.is_finite() { steps / STEPS_PER_MM + 0.0 } else { 0.0 }
}

/// `p` (document points) as the commands take it: millimetres from the document's origin.
fn mm(p: Point) -> [f64; 2] {
    [p.x / pt_per_mm(), p.y / pt_per_mm()]
}

/// `{id, index}`, as the stitch commands address a needle point.
fn address(n: NeedleRef) -> Value {
    json!({"id": n.id.0, "index": n.index})
}

/// Select `needles` (`mode`: set, add or toggle).
fn select(needles: &[NeedleRef], mode: &str) -> Action {
    Action::Exec(SELECT.into(), json!({"needlePoints": needles.iter().copied().map(address).collect::<Vec<_>>(), "mode": mode}))
}

/// Whether needle point `n` is selected: its anchor among the selection's anchors, or its path
/// selected whole.
pub fn is_selected(sel: &Selection, needles: &dyn NeedlePoints, n: NeedleRef) -> bool {
    match sel.anchors.get(&n.id) {
        Some(anchors) => needles.anchors(n).iter().any(|a| anchors.contains(a)),
        None => sel.objects.contains(&n.id),
    }
}

/// About how many needle points being edited are selected (a run of one needle point counted once
/// for each of its two anchors): what decides whether a drag previews live.
pub fn selected_count(sel: &Selection, needles: &dyn NeedlePoints) -> usize {
    sel.objects
        .iter()
        .filter(|id| needles.holds(**id))
        .map(|id| sel.anchors.get(id).map_or_else(|| needles.count(*id), std::collections::BTreeSet::len))
        .sum()
}

/// The first needle point being edited that is selected, in sewing order, when any is: where `[`
/// and `]` start without a current one. Reads the selection's first path only.
fn first_selected(sel: &Selection, needles: &dyn NeedlePoints) -> Option<NeedleRef> {
    let id = *sel.objects.iter().find(|id| needles.holds(**id))?;
    let index = match sel.anchors.get(&id) {
        Some(anchors) => anchors.iter().filter_map(|a| needles.of_anchor(id, *a)).min()?,
        None => 0,
    };
    Some(NeedleRef { id, index })
}

/// Whether `n`'s subtree holds a path of stitches: the object a double-click starts editing.
pub fn holds_stitches(n: &vectorcraft_doc::Node) -> bool {
    let mut found = false;
    n.walk(&mut |c| {
        found |= matches!(c.kind, vectorcraft_doc::NodeKind::Path { .. }) && c.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some());
    });
    found
}

/// The object a double-click on `id` isolates: the one just inside the isolation (or the layer)
/// that holds it, when that holds stitches (a converted object, an opened design's thread block).
pub fn stitch_object(doc: &Document, isolation: Option<NodeId>, id: NodeId) -> Option<NodeId> {
    let ancestry = doc.ancestry(id)?;
    let inside = match isolation {
        Some(scope) => ancestry.iter().position(|a| *a == scope)?.saturating_add(1),
        None => 1,
    };
    let top = *ancestry.get(inside).or(ancestry.last())?;
    doc.node(top).filter(|n| holds_stitches(n)).map(|_| top)
}

impl Editing {
    /// Whether a gesture is under way (a drag that has begun, or a marquee).
    pub fn busy(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.begun) || self.marquee.is_some_and(|m| m.dragged)
    }

    /// Whether ticks are wanted: a drag of needle points is held.
    pub fn wants_ticks(&self) -> bool {
        self.drag.is_some()
    }

    /// Whether a press is held: a drag of needle points or a marquee, begun or not.
    pub fn held(&self) -> bool {
        self.drag.is_some() || self.marquee.is_some()
    }

    /// Escape: a drag under way cancelled (the document as it was at the press), a marquee dropped.
    pub fn cancel(&mut self) -> Vec<Action> {
        self.marquee = None;
        match self.drag.take() {
            Some(d) if d.begun => vec![Action::Cancel],
            _ => vec![],
        }
    }

    /// The marquee's box, while one is drawn (document points).
    pub fn marquee(&self) -> Option<Rect> {
        self.marquee.filter(|m| m.dragged).map(|m| Rect::from_points(m.from, m.to))
    }

    /// The drag's move now, millimetres, while one has begun.
    pub fn moved(&self) -> Option<[f64; 2]> {
        self.drag.as_ref().filter(|d| d.begun).map(|d| d.delta)
    }

    /// Everything let go (the tool put away, an outside command ended the drag).
    pub fn let_go(&mut self) {
        self.drag = None;
        self.marquee = None;
        self.hover = None;
    }

    /// What lies under `p`: a needle point within the anchors' reach, else a stitch within the
    /// selection tolerance (as Direct Selection catches anchors before the segments through them).
    pub fn under(cx: &ToolContext, needles: &dyn NeedlePoints, p: Point) -> Option<Hover> {
        if let Some(h) = needles.needle_at(p, cx.point_tol()) {
            return Some(Hover::Needle(h.needle));
        }
        needles.thread_at(p, cx.pick_tol()).map(|t| Hover::Thread(t.from, t.point))
    }

    /// A pointer event while editing.
    pub fn pointer(&mut self, cx: &ToolContext, needles: &dyn NeedlePoints, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Move => {
                self.hover = Self::under(cx, needles, ev.pos);
                vec![]
            }
            PointerKind::Down => self.press(cx, needles, ev),
            PointerKind::Drag => self.drag_to(cx, ev),
            PointerKind::Up => self.release(),
            PointerKind::DoubleClick if self.busy() => vec![],
            PointerKind::DoubleClick => match Self::under(cx, needles, ev.pos) {
                // A stitch: its whole run.
                Some(Hover::Thread(n, _)) => match needles.run(n) {
                    Some((first, last)) => {
                        self.current = Some(NeedleRef { id: n.id, index: first });
                        vec![Action::Exec(
                            SELECT.into(),
                            json!({"ranges": [{"id": n.id.0, "from": first, "to": last}], "mode": if ev.mods.shift { "add" } else { "set" }}),
                        )]
                    }
                    None => vec![],
                },
                Some(Hover::Needle(_)) => vec![],
                // Away from the stitches being edited: editing ends, as a double-click outside
                // leaves isolation with the Selection tool.
                None => {
                    self.let_go();
                    vec![Action::Exec("object.exitIsolation".into(), json!({}))]
                }
            },
        }
    }

    /// A press: on a needle point or a stitch, select it (Shift: add it or take it away) and hold it
    /// for a drag; Alt on a stitch, a needle point inserted there; elsewhere, a marquee.
    fn press(&mut self, cx: &ToolContext, needles: &dyn NeedlePoints, ev: &PointerEvent) -> Vec<Action> {
        let mut out = self.release();
        self.marquee = None;
        let drag = |alone: Option<NeedleRef>, count: usize| Drag {
            from: ev.pos,
            alone,
            begun: false,
            delta: [0.0, 0.0],
            previewed: None,
            rested: 0.0,
            many: count > LIVE_DRAG_POINTS_MAX,
        };
        match Self::under(cx, needles, ev.pos) {
            Some(Hover::Thread(n, at)) if ev.mods.alt => {
                // A new needle point on the stitch, where it is pressed: before the needle point the
                // stitch arrives at, left selected.
                let next = NeedleRef { id: n.id, index: n.index + 1 };
                self.current = Some(next);
                out.push(Action::Exec(INSERT.into(), json!({"id": n.id.0, "index": next.index, "at": [mm(at)]})));
            }
            Some(Hover::Needle(n)) => {
                self.current = Some(n);
                let selected = is_selected(cx.selection, needles, n);
                if ev.mods.shift {
                    out.push(select(&[n], "toggle"));
                    if !selected {
                        self.drag = Some(drag(None, selected_count(cx.selection, needles) + 1));
                    }
                } else if selected {
                    self.drag = Some(drag(Some(n), selected_count(cx.selection, needles)));
                } else {
                    out.push(select(&[n], "set"));
                    self.drag = Some(drag(None, 1));
                }
            }
            Some(Hover::Thread(n, _)) => {
                let pair = [n, NeedleRef { id: n.id, index: n.index + 1 }];
                self.current = Some(n);
                let both = pair.iter().all(|m| is_selected(cx.selection, needles, *m));
                if ev.mods.shift {
                    out.push(select(&pair, "toggle"));
                } else if both {
                    self.drag = Some(drag(None, selected_count(cx.selection, needles)));
                } else {
                    out.push(select(&pair, "set"));
                    self.drag = Some(drag(None, pair.len()));
                }
            }
            None => {
                self.marquee = Some(Marquee { from: ev.pos, to: ev.pos, mode: if ev.mods.shift { "add" } else { "set" }, dragged: false });
            }
        }
        out
    }

    /// The pointer dragged: the move of the needle points held (on the grid; Shift at 45° steps),
    /// previewed at the next tick; or the marquee's corner.
    fn drag_to(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        if let Some(m) = self.marquee.as_mut() {
            m.to = ev.pos;
            m.dragged |= ev.pos.distance(m.from) > cx.tol(DRAG_START_PX);
            return vec![];
        }
        let Some(d) = self.drag.as_mut() else { return vec![] };
        let mut out = Vec::new();
        if !d.begun {
            if ev.pos.distance(d.from) <= cx.tol(DRAG_START_PX) {
                return out;
            }
            d.begun = true;
            out.push(Action::Begin(MOVE_LABEL.into()));
        }
        let by: Vec2 = ev.pos - d.from;
        let by = if ev.mods.shift { constrain_angle_from(by, SHIFT_ANGLE, cx.constrain_angle) } else { by };
        let delta = [on_grid(by.x), on_grid(by.y)];
        if delta != d.delta {
            d.delta = delta;
            d.rested = 0.0;
        }
        out
    }

    /// Time passes during a drag: the move previewed when it changed since the last preview (at
    /// most once a frame), for many needle points only once it has rested.
    pub fn tick(&mut self, dt: f64) -> Vec<Action> {
        let Some(d) = self.drag.as_mut().filter(|d| d.begun) else { return vec![] };
        if d.previewed == Some(d.delta) || (d.previewed.is_none() && d.delta == [0.0, 0.0]) {
            return vec![];
        }
        if d.many {
            d.rested += dt.max(0.0);
            if d.rested < PAUSE_PREVIEW_SECS {
                return vec![];
            }
        }
        d.previewed = Some(d.delta);
        vec![Action::Preview(MOVE.into(), json!({"dx": d.delta[0], "dy": d.delta[1]}))]
    }

    /// The release: a drag kept (its last move previewed first), a click on a selected needle point
    /// selecting it alone, a marquee's needle points selected (a click away from them selecting
    /// none).
    pub fn release(&mut self) -> Vec<Action> {
        if let Some(m) = self.marquee.take() {
            return match (m.dragged, m.mode) {
                (true, mode) => {
                    let r = Rect::from_points(m.from, m.to);
                    let (a, b) = (mm(Point::new(r.x0, r.y0)), mm(Point::new(r.x1, r.y1)));
                    vec![Action::Exec(SELECT.into(), json!({"rect": [a[0], a[1], b[0], b[1]], "mode": mode}))]
                }
                (false, "set") => vec![Action::Exec("select.none".into(), json!({}))],
                (false, _) => vec![],
            };
        }
        let Some(d) = self.drag.take() else { return vec![] };
        if !d.begun {
            return d.alone.map_or_else(Vec::new, |n| vec![select(&[n], "set")]);
        }
        let mut out = Vec::new();
        let mut previewed = d.previewed;
        if previewed != Some(d.delta) && !(previewed.is_none() && d.delta == [0.0, 0.0]) {
            out.push(Action::Preview(MOVE.into(), json!({"dx": d.delta[0], "dy": d.delta[1]})));
            previewed = Some(d.delta);
        }
        out.push(if previewed.is_some() { Action::Commit } else { Action::Cancel });
        out
    }

    /// Whether the editing claims `key` now: Escape while a gesture is under way or needle points are
    /// selected; the arrows, Delete and Backspace while needle points are selected; `[` and `]`
    /// while there are needle points to step through.
    pub fn claims_key(&self, cx: &ToolContext, needles: &dyn NeedlePoints, key: ToolKey) -> bool {
        let selected = || cx.selection.objects.iter().any(|id| needles.holds(*id));
        match key {
            ToolKey::Escape => self.drag.is_some() || self.marquee.is_some() || selected(),
            ToolKey::Left | ToolKey::Right | ToolKey::Up | ToolKey::Down | ToolKey::Delete | ToolKey::Backspace => self.drag.is_none() && selected(),
            ToolKey::BracketLeft | ToolKey::BracketRight => needles.first().is_some(),
            _ => false,
        }
    }

    /// A key while editing (one it claims).
    pub fn key(&mut self, cx: &ToolContext, needles: &dyn NeedlePoints, key: ToolKey, mods: Mods) -> Vec<Action> {
        if key == ToolKey::Escape {
            if self.held() {
                return self.cancel();
            }
            self.current = None;
            return vec![Action::Exec("select.none".into(), json!({}))];
        }
        if self.drag.is_some() {
            return vec![];
        }
        let steps = if mods.shift { SHIFT_STEPS } else { 1.0 };
        let by = steps / STEPS_PER_MM;
        let (dx, dy) = match key {
            ToolKey::Left => (-by, 0.0),
            ToolKey::Right => (by, 0.0),
            ToolKey::Up => (0.0, -by),
            ToolKey::Down => (0.0, by),
            ToolKey::Delete | ToolKey::Backspace => return vec![Action::Exec(DELETE.into(), json!({}))],
            ToolKey::BracketLeft | ToolKey::BracketRight => {
                let from = self.current.filter(|n| n.index < needles.count(n.id)).or_else(|| first_selected(cx.selection, needles));
                let next = match from {
                    Some(n) => needles.step(n, key == ToolKey::BracketRight),
                    None => needles.first(),
                };
                let Some(next) = next else { return vec![] };
                self.current = Some(next);
                return vec![select(&[next], if mods.shift { "add" } else { "set" })];
            }
            _ => return vec![],
        };
        vec![Action::Exec(MOVE.into(), json!({"dx": dx, "dy": dy}))]
    }

    /// The cursor while editing: the point cursor over a needle point or a stitch and while dragging
    /// them, the Pen's add cursor with Alt over a stitch (a click inserts there), the tool's own
    /// elsewhere (a drag draws a marquee).
    pub fn cursor(&self, cx: &ToolContext, needles: &dyn NeedlePoints, p: Point, mods: Mods) -> Cursor {
        if self.drag.is_some() {
            return Cursor::EmbroideryPoint;
        }
        if self.marquee.is_some() {
            return Cursor::Embroidery;
        }
        match Self::under(cx, needles, p) {
            Some(Hover::Thread(..)) if mods.alt => Cursor::PenAdd,
            Some(_) => Cursor::EmbroideryPoint,
            None => Cursor::Embroidery,
        }
    }

    /// The tool's options' `stitches`: what lies under the pointer, the needle point `[` `]` step
    /// from, the marquee (document points) and the drag's move (millimetres). What is being edited
    /// is the engine's to say (`Session::stitch_editing`): the tool keeps no copy of it.
    pub fn options(&self) -> Value {
        let hover = match self.hover {
            Some(Hover::Needle(h)) => json!({"needlePoint": address(h)}),
            Some(Hover::Thread(h, at)) => json!({"stitch": address(h), "at": [at.x, at.y]}),
            None => Value::Null,
        };
        json!({
            "hover": hover,
            "current": self.current.map_or(Value::Null, address),
            "marquee": self.marquee().map_or(Value::Null, |r| json!([r.x0, r.y0, r.x1, r.y1])),
            "moved": self.moved().map_or(Value::Null, |d| json!(d)),
        })
    }
}

#[cfg(test)]
mod tests;
