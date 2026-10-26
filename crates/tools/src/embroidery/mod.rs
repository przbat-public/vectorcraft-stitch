//! The embroidery tool (VectorCraft Stitch): one tool with kinds of handles. Today the kinds are a
//! part's entry and exit points ([`Kind`]); later kinds (angle lines, satin rails, needle points)
//! plug into the same gesture core.
//!
//! - **Handles** come from [`handles`] on every call: the selection's objects that take settings,
//!   one part at a time, each point set where it is, else where the plan shown sews it (the engine
//!   hands that in, [`PlanFacts`]).
//! - **A drag** on a handle, past [`DRAG_START_PX`], previews `embroidery.setEntryExit` with
//!   `snap` (the point taken to where the stitches can start or end), one undo step on release;
//!   Escape cancels it. A press elsewhere selects the object under the pointer.
//! - **Keys** act on the handle last pressed (or stepped to with `[` and `]`): Delete puts it back
//!   to Auto (as an Alt-click does), the arrows nudge it by the keyboard increment (Shift ten times
//!   that), Escape lets it go.

mod facts;
mod handles;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};
use vectorcraft_doc::MAX_COORD;
use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_doc::embroidery::takers::{compound_of, takers};
use vectorcraft_doc::hit::hit_test;
use vectorcraft_geom::{Point, Vec2};

pub use facts::{PlanFacts, PlannedEnds};
pub use handles::{
    APART_PX, CATCH_PAST_PX, ENTRY_MARKER_PX, EXIT_MARKER_PX, HANDLED_OBJECTS_MAX, Handle, HandleId, Kind, Owner, State, handles, hit,
};

use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

/// The tool's id.
pub const ID: &str = "embroidery";

/// The command a handle's drag, nudge or reset runs.
const SET_ENTRY_EXIT: &str = "embroidery.setEntryExit";

/// The undo step a drag makes: the command's own label, as the History panel names it.
const LABEL: &str = "Embroidery Entry and Exit";

/// How far the pointer moves after a press on a handle before it drags, screen pixels (the
/// Gradient tool's).
pub const DRAG_START_PX: f64 = 3.0;

/// How many keyboard increments an arrow with Shift nudges a point.
const SHIFT_NUDGE: f64 = 10.0;

/// A handle held by a press: which, where the press began, whether it is dragging, the last point
/// previewed and where the handle now is (document points).
#[derive(Clone, Copy, Debug)]
struct Grab {
    id: HandleId,
    from: Point,
    begun: bool,
    previewed: Option<Point>,
    at: Point,
}

/// The embroidery tool.
#[derive(Debug, Default)]
pub struct EmbroideryTool {
    /// The part whose points show (the Object tab's), `None`: each object's fill, else its stroke.
    part: Option<PartKind>,
    /// The handle under the pointer.
    hover: Option<HandleId>,
    /// The handle keys act on: the one last pressed or stepped to.
    focus: Option<HandleId>,
    grab: Option<Grab>,
}

/// The tool for `id`, when it is this one.
pub fn create(id: &str) -> Option<Box<dyn Tool>> {
    (id == ID).then(|| Box::new(EmbroideryTool::default()) as Box<dyn Tool>)
}

/// `p` kept within the coordinates a document holds; `None` when it isn't a number.
fn within(p: Point) -> Option<Point> {
    (p.x.is_finite() && p.y.is_finite()).then(|| Point::new(p.x.clamp(-MAX_COORD, MAX_COORD), p.y.clamp(-MAX_COORD, MAX_COORD)))
}

/// `embroidery.setEntryExit`'s params setting handle `id`'s point to `at` (`None`: back to Auto).
fn set_point(id: HandleId, at: Option<Point>) -> Value {
    let (object, part) = id.owner.part();
    let mut p = json!({"id": object.0, "part": part.id(), "snap": true});
    p[id.kind.id()] = at.map_or(Value::Null, |q| json!([q.x, q.y]));
    p
}

impl EmbroideryTool {
    /// The handles now (the selection's, of the part shown).
    fn now(&self, cx: &ToolContext) -> Vec<Handle> {
        handles(cx, self.part)
    }

    /// The focused handle, while it still shows.
    fn focused(&self, cx: &ToolContext) -> Option<Handle> {
        let id = self.focus?;
        self.now(cx).into_iter().find(|h| h.id == id)
    }

    /// A press away from every handle: the object under the pointer selected (the single object,
    /// even inside a group; by its stitches where the plan answers for them), added or taken away
    /// with Shift; nothing under it, the selection emptied (unless Shift is held).
    fn select_at(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        self.focus = None;
        let by_stitch = cx.embroidery.and_then(|f| f.object_at(ev.pos, cx.pick_tol()));
        let by_art = || hit_test(cx.doc, ev.pos, cx.hit_options()).map(|h| compound_of(cx.doc, h.leaf));
        let under = by_stitch.or_else(by_art).filter(|id| takers(cx.doc, &[*id]).0 == [*id]);
        match (under, ev.mods.shift) {
            (Some(id), false) => vec![Action::Exec("select.set".into(), json!({"ids": [id.0]}))],
            (Some(id), true) => vec![Action::Exec("select.toggle".into(), json!({"ids": [id.0]}))],
            (None, false) if !cx.selection.objects.is_empty() => vec![Action::Exec("select.set".into(), json!({"ids": []}))],
            (None, _) => vec![],
        }
    }

    /// Handle `id` back to Auto: its point unset.
    fn back_to_auto(&mut self, id: HandleId) -> Vec<Action> {
        self.focus = Some(id);
        vec![Action::Exec(SET_ENTRY_EXIT.into(), set_point(id, None))]
    }
}

impl Tool for EmbroideryTool {
    fn id(&self) -> &'static str {
        ID
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Move => {
                self.hover = hit(cx, &self.now(cx), ev.pos).map(|h| h.id);
                vec![]
            }
            PointerKind::Down => {
                let Some(h) = hit(cx, &self.now(cx), ev.pos) else { return self.select_at(cx, ev) };
                if ev.mods.alt {
                    return self.back_to_auto(h.id);
                }
                self.focus = Some(h.id);
                self.grab = Some(Grab { id: h.id, from: ev.pos, begun: false, previewed: None, at: h.point });
                vec![]
            }
            PointerKind::Drag => {
                let Some(grab) = self.grab.as_mut() else { return vec![] };
                let Some(at) = within(ev.pos) else { return vec![] };
                let mut out = Vec::new();
                if !grab.begun {
                    if ev.pos.distance(grab.from) <= cx.tol(DRAG_START_PX) {
                        return out;
                    }
                    grab.begun = true;
                    out.push(Action::Begin(LABEL.into()));
                }
                grab.at = at;
                // No preview where the point wouldn't change.
                if grab.previewed != Some(at) {
                    grab.previewed = Some(at);
                    out.push(Action::Preview(SET_ENTRY_EXIT.into(), set_point(grab.id, Some(at))));
                }
                out
            }
            PointerKind::Up => match self.grab.take() {
                Some(Grab { begun: true, previewed: Some(_), .. }) => vec![Action::Commit],
                Some(Grab { begun: true, previewed: None, .. }) => vec![Action::Cancel],
                _ => vec![],
            },
            PointerKind::DoubleClick => vec![],
        }
    }

    fn key(&mut self, cx: &ToolContext, key: ToolKey, mods: Mods) -> Vec<Action> {
        if key == ToolKey::Escape {
            return match self.grab.take() {
                Some(Grab { begun: true, .. }) => vec![Action::Cancel],
                Some(_) => vec![],
                None => {
                    self.focus = None;
                    vec![]
                }
            };
        }
        if self.grab.is_some() {
            return vec![];
        }
        let all = self.now(cx);
        if matches!(key, ToolKey::BracketLeft | ToolKey::BracketRight) {
            let at = self.focus.and_then(|id| all.iter().position(|h| h.id == id));
            let n = all.len();
            let next = match (at, key) {
                (_, _) if n == 0 => None,
                (None, ToolKey::BracketRight) => Some(0),
                (None, _) => Some(n - 1),
                (Some(i), ToolKey::BracketRight) => Some((i + 1) % n),
                (Some(i), _) => Some((i + n - 1) % n),
            };
            self.focus = next.and_then(|i| all.get(i)).map(|h| h.id);
            return vec![];
        }
        let Some(h) = self.focus.and_then(|id| all.into_iter().find(|h| h.id == id)) else { return vec![] };
        let step = cx.keyboard_increment * if mods.shift { SHIFT_NUDGE } else { 1.0 };
        let by = match key {
            ToolKey::Delete | ToolKey::Backspace => return self.back_to_auto(h.id),
            ToolKey::Left => Vec2::new(-step, 0.0),
            ToolKey::Right => Vec2::new(step, 0.0),
            ToolKey::Up => Vec2::new(0.0, -step),
            ToolKey::Down => Vec2::new(0.0, step),
            _ => return vec![],
        };
        match within(h.point + by) {
            Some(at) => vec![Action::Exec(SET_ENTRY_EXIT.into(), set_point(h.id, Some(at)))],
            None => vec![],
        }
    }

    fn cursor(&self, cx: &ToolContext, p: Point, _mods: Mods) -> Cursor {
        if self.grab.is_some() || hit(cx, &self.now(cx), p).is_some() { Cursor::EmbroideryPoint } else { Cursor::Embroidery }
    }

    fn options(&self) -> Value {
        let part = self.part.map_or(Value::Null, |k| json!(k.id()));
        let drag = self.grab.filter(|g| g.begun).map_or(Value::Null, |g| {
            let mut v = g.id.json();
            v["at"] = json!([g.at.x, g.at.y]);
            v
        });
        let id = |h: Option<HandleId>| h.map_or(Value::Null, HandleId::json);
        json!({"part": part, "hover": id(self.hover), "focus": id(self.focus), "drag": drag})
    }

    fn set_option(&mut self, key: &str, value: &Value) {
        if key == "part" {
            self.part = value.as_str().and_then(PartKind::from_id);
        }
    }

    fn busy(&self) -> bool {
        self.grab.is_some_and(|g| g.begun)
    }

    fn claims_key(&self, cx: &ToolContext, key: ToolKey) -> bool {
        match key {
            ToolKey::Escape => self.grab.is_some() || self.focus.is_some(),
            ToolKey::BracketLeft | ToolKey::BracketRight => !self.now(cx).is_empty(),
            ToolKey::Delete | ToolKey::Backspace | ToolKey::Left | ToolKey::Right | ToolKey::Up | ToolKey::Down => self.focused(cx).is_some(),
            _ => false,
        }
    }

    fn hover_ended(&mut self, _cx: &ToolContext) {
        self.hover = None;
    }

    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.hover = None;
        self.focus = None;
        match self.grab.take() {
            Some(Grab { begun: true, previewed: Some(_), .. }) => vec![Action::Commit],
            Some(Grab { begun: true, previewed: None, .. }) => vec![Action::Cancel],
            _ => vec![],
        }
    }
}
