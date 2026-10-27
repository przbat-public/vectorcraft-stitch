//! The embroidery tool (VectorCraft Stitch): one tool with kinds of handles. Today the kinds are a
//! part's entry and exit points and a fill's rows' angle ([`Kind`]); later kinds (angle lines,
//! satin rails, needle points) plug into the same gesture core.
//!
//! - **Handles** come from [`handles`] on every call: the selection's objects that take settings,
//!   one part at a time, each point set where it is, else where the plan shown sews it, and a
//!   tatami fill's angle as an arrow through its centre (the engine hands both in, [`PlanFacts`]).
//! - **A drag** on a handle, past [`DRAG_START_PX`], previews its kind's command, one undo step on
//!   release; Escape cancels it. A point previews `embroidery.setEntryExit` with `snap` (taken to
//!   where the stitches can start or end); the arrow's head turns it about the fill's centre,
//!   previewing `embroidery.set {angle}`, in [`SHIFT_ANGLE_STEP`]s from Constrain Angle with Shift.
//!   A press elsewhere selects the object under the pointer.
//! - **Keys** act on the handle last pressed (or stepped to with `[` and `]`): Delete puts it back
//!   to Auto (as an Alt-click does); the arrows nudge a point by the keyboard increment, and turn an
//!   angle by [`ANGLE_KEY_STEP`] (→ and ↑ counter-clockwise), Shift ten times that; Enter asks for a
//!   point's X and Y (as a double-click on it does, [`POINT_DIALOG`]); Escape lets it go.

mod facts;
mod handles;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};
use vectorcraft_doc::MAX_COORD;
use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_doc::embroidery::takers::{compound_of, takers};
use vectorcraft_doc::hit::{HitKind, hit_test};
use vectorcraft_geom::{Point, Vec2, constrain_angle_from, normalize_deg};

pub use facts::{FillAngle, PlanFacts, PlannedEnds};
pub use handles::{
    APART_PX, ARROW_HEAD_PX, ARROW_REACH_PX, Arrow, CATCH_MIN_PX, CATCH_PAST_PX, ENTRY_MARKER_PX, EXIT_MARKER_PX, HANDLED_OBJECTS_MAX, Handle,
    HandleId, Kind, Owner, State, direction, handle_walks, handles, hit, hit_after,
};

use crate::{Action, Cursor, Mods, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};

/// The tool's id.
pub const ID: &str = "embroidery";

/// The command a point's drag, nudge or reset runs.
const SET_ENTRY_EXIT: &str = "embroidery.setEntryExit";

/// The undo step a point's drag makes: the command's own label, as the History panel names it.
const LABEL: &str = "Embroidery Entry and Exit";

/// The command an angle's drag or turn runs, and the undo step its drag makes (its own label).
const SET_SETTINGS: &str = "embroidery.set";
const SETTINGS_LABEL: &str = "Embroidery Object Settings";

/// The command an angle's reset runs.
const RESET_SETTINGS: &str = "embroidery.reset";

/// The steps Shift turns an angle in, degrees from Constrain Angle: as every Shift-constrained drag
/// in the app.
pub const SHIFT_ANGLE_STEP: f64 = 45.0;

/// How far an arrow key turns an angle, degrees (Shift ten times that): the Object tab's angle
/// field's step.
pub const ANGLE_KEY_STEP: f64 = 1.0;

/// A turned angle's places, a tenth of a degree: what the Object tab's angle field shows.
const ANGLE_PLACES: f64 = 10.0;

/// The dialog Enter or a double-click on a handle asks for: its point's X and Y, or Auto. Its params:
/// `{kind, id, part, at: [x, y], set}` (document points; whether the point is set).
pub const POINT_DIALOG: &str = "embroideryPoint";

/// How far the pointer moves after a press on a handle before it drags, screen pixels (the
/// Gradient tool's).
pub const DRAG_START_PX: f64 = 3.0;

/// How many keyboard increments an arrow with Shift nudges a point.
const SHIFT_NUDGE: f64 = 10.0;

/// A handle held by a press: which, where the press began and where its point was then, whether it
/// is dragging, the params last previewed and the point they store (taken where the stitches can
/// start or end), where the handle now is (document points) and, for an angle, its arrow now.
#[derive(Clone, Debug)]
struct Grab {
    id: HandleId,
    from: Point,
    start: Point,
    begun: bool,
    previewed: Option<Value>,
    stored: Option<Point>,
    at: Point,
    arrow: Option<Arrow>,
}

/// The embroidery tool.
#[derive(Debug, Default)]
pub struct EmbroideryTool {
    /// The part whose points show (the Object tab's), `None`: each object's fill, else its stroke.
    part: Option<PartKind>,
    /// The handle under the pointer.
    hover: Option<HandleId>,
    /// Away from every handle, the object a press would select (the canvas outlines it).
    hover_object: Option<vectorcraft_doc::NodeId>,
    /// The handle keys act on: the one last pressed or stepped to.
    focus: Option<HandleId>,
    /// Whether the keys acted on it last (a step, a nudge): the canvas then shows its jump, as a
    /// drag does.
    keyed: bool,
    grab: Option<Grab>,
    /// The handles as last found, and what for ([`MemoKey`]): each pointer move and cursor query
    /// asks for them, and they are found again only when that changed.
    memo: std::cell::RefCell<Option<(MemoKey, Vec<Handle>)>>,
}

/// What the tool's handles are found for: the document's uid and revision (which moves with every
/// change of it and of the selection), the document and the selection as given, the selection's
/// size, the generation of the plan answered from, the view's zoom and its turn (an arrow's head and
/// an exit moved apart are placed in screen pixels), and the part shown.
type MemoKey = ((u64, u64), usize, usize, usize, u64, u64, [u64; 2], Option<PartKind>);

/// [`MemoKey`] of `cx` and `part`.
fn memo_key(cx: &ToolContext, part: Option<PartKind>) -> MemoKey {
    let right = cx.screen.map_or([0, 0], |s| [s.right.x.to_bits(), s.right.y.to_bits()]);
    (
        cx.revision,
        std::ptr::from_ref(cx.doc) as usize,
        std::ptr::from_ref(cx.selection) as usize,
        cx.selection.objects.len(),
        cx.embroidery.map_or(0, |f| f.generation()),
        cx.zoom.to_bits(),
        right,
        part,
    )
}

/// The command a drag of a handle of `kind` previews, and the undo step it makes.
fn command_of(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::Entry | Kind::Exit => (SET_ENTRY_EXIT, LABEL),
        Kind::Angle => (SET_SETTINGS, SETTINGS_LABEL),
    }
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

/// `embroidery.set`'s params setting angle handle `id`'s rows to `degrees`.
fn set_angle(id: HandleId, degrees: f64) -> Value {
    let (object, part) = id.owner.part();
    json!({"ids": [object.0], "part": part.id(), "angle": degrees})
}

/// `degrees` as an angle is set: to a tenth of a degree, in (−180, 180].
fn angle_of(degrees: f64) -> Option<f64> {
    let a = normalize_deg((degrees * ANGLE_PLACES).round() / ANGLE_PLACES) + 0.0;
    a.is_finite().then_some(a)
}

/// The angle the arrow about `centre` points at `p` with (degrees counter-clockwise on the page);
/// with `shift`, in [`SHIFT_ANGLE_STEP`]s from Constrain Angle `base`. `None` at the centre.
fn aimed(centre: Point, p: Point, shift: bool, base: f64) -> Option<f64> {
    let v = p - centre;
    if v.hypot() <= vectorcraft_geom::EPS {
        return None;
    }
    let v = if shift { constrain_angle_from(v, SHIFT_ANGLE_STEP, base) } else { v };
    angle_of(-v.y.atan2(v.x).to_degrees())
}

impl EmbroideryTool {
    /// The handles now (the selection's, of the part shown), found again only when what they are
    /// found for changed ([`MemoKey`]).
    fn now(&self, cx: &ToolContext) -> Vec<Handle> {
        let key = memo_key(cx, self.part);
        if let Ok(memo) = self.memo.try_borrow()
            && let Some((k, found)) = memo.as_ref()
            && *k == key
        {
            return found.clone();
        }
        let found = handles(cx, self.part);
        if let Ok(mut memo) = self.memo.try_borrow_mut() {
            *memo = Some((key, found.clone()));
        }
        found
    }

    /// The focused handle, while it still shows.
    fn focused(&self, cx: &ToolContext) -> Option<Handle> {
        let id = self.focus?;
        self.now(cx).into_iter().find(|h| h.id == id)
    }

    /// The object a press at `p` would select, and the part pressed: the single object under it,
    /// even inside a group, by its stitches where the plan answers for them (in the stitch view; an
    /// object the art's hit test would pass over, [`pickable`], isn't picked by them), else by its
    /// art (its fill or its stroke; an outline or a bounding box says no part); only one that takes
    /// settings.
    fn under(cx: &ToolContext, p: Point) -> Option<(vectorcraft_doc::NodeId, Option<PartKind>)> {
        let by_stitch = cx.embroidery.and_then(|f| f.part_at(p, cx.pick_tol())).filter(|(id, _)| pickable(cx, *id));
        let by_art = || {
            hit_test(cx.doc, p, cx.hit_options()).map(|h| {
                let part = match h.kind {
                    HitKind::Fill => Some(PartKind::Fill),
                    HitKind::Stroke => Some(PartKind::Stroke),
                    HitKind::Outline | HitKind::Bounds => None,
                };
                (compound_of(cx.doc, h.leaf), part)
            })
        };
        // A symbol instance takes no points, but its Trim After: selected, it shows no handle.
        let instance =
            |id: &vectorcraft_doc::NodeId| matches!(cx.doc.node(*id).map(|n| &n.kind), Some(vectorcraft_doc::NodeKind::SymbolInstance { .. }));
        by_stitch.or_else(by_art).filter(|(id, _)| takers(cx.doc, &[*id]).0 == [*id] || instance(id))
    }

    /// A press away from every handle: the object under the pointer selected ([`Self::under`]) and
    /// the part pressed shown (the project owner's choice, 2026-10-25: a part is picked by pressing
    /// it on the canvas, and the Object tab follows), added or taken away with Shift; nothing under
    /// it, the selection emptied (unless Shift is held).
    fn select_at(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        self.focus = None;
        let under = Self::under(cx, ev.pos);
        match (under, ev.mods.shift) {
            (Some((id, part)), false) => {
                if part.is_some() {
                    self.part = part;
                }
                vec![Action::Exec("select.set".into(), json!({"ids": [id.0]}))]
            }
            (Some((id, _)), true) => vec![Action::Exec("select.toggle".into(), json!({"ids": [id.0]}))],
            (None, false) if !cx.selection.objects.is_empty() => vec![Action::Exec("select.set".into(), json!({"ids": []}))],
            (None, _) => vec![],
        }
    }

    /// The drag under way ended, kept (as a release keeps it); nothing when none has begun.
    fn finish_drag(&mut self) -> Vec<Action> {
        match self.grab.take() {
            Some(Grab { begun: true, previewed: Some(_), .. }) => vec![Action::Commit],
            Some(Grab { begun: true, previewed: None, .. }) => vec![Action::Cancel],
            _ => vec![],
        }
    }

    /// The point dialog for handle `h` ([`POINT_DIALOG`]).
    fn dialog(&mut self, h: &Handle) -> Vec<Action> {
        self.focus = Some(h.id);
        let mut p = h.id.json();
        p["at"] = json!([h.point.x, h.point.y]);
        p["set"] = json!(h.state == State::Set);
        vec![Action::Dialog(POINT_DIALOG.into(), p)]
    }

    /// Handle `id` back to Auto: its point or its angle unset.
    fn back_to_auto(&mut self, id: HandleId) -> Vec<Action> {
        self.focus = Some(id);
        match id.kind {
            Kind::Entry | Kind::Exit => vec![Action::Exec(SET_ENTRY_EXIT.into(), set_point(id, None))],
            Kind::Angle => {
                let (object, part) = id.owner.part();
                vec![Action::Exec(RESET_SETTINGS.into(), json!({"ids": [object.0], "part": part.id(), "keys": ["angle"]}))]
            }
        }
    }
}

/// Whether object `id`, found by its stitches, may be picked, as the art's hit test picks: visible
/// and unlocked with every group and layer it is in, off the locked artboards, on no template layer,
/// and inside the group or object isolated (isolation mode). The stitches drawn can be of a plan
/// made before the object was hidden or locked.
fn pickable(cx: &ToolContext, id: vectorcraft_doc::NodeId) -> bool {
    let Some(ancestry) = cx.doc.ancestry(id) else { return false };
    cx.doc.is_editable(id)
        && !ancestry.iter().any(|a| cx.doc.node(*a).is_some_and(vectorcraft_doc::Node::is_template))
        && cx.isolation.is_none_or(|scope| ancestry.contains(&scope))
}

impl Tool for EmbroideryTool {
    fn id(&self) -> &'static str {
        ID
    }

    fn pointer(&mut self, cx: &ToolContext, ev: &PointerEvent) -> Vec<Action> {
        match ev.kind {
            PointerKind::Move => {
                self.hover = hit(cx, &self.now(cx), ev.pos).map(|h| h.id);
                self.hover_object = if self.hover.is_some() { None } else { Self::under(cx, ev.pos).map(|(id, _)| id) };
                vec![]
            }
            PointerKind::Down => {
                self.keyed = false;
                // A press with a drag still under way (no release came between): that drag ends first.
                let mut out = self.finish_drag();
                match hit_after(cx, &self.now(cx), ev.pos, self.focus) {
                    None => out.extend(self.select_at(cx, ev)),
                    Some(h) if ev.mods.alt => out.extend(self.back_to_auto(h.id)),
                    Some(h) => {
                        self.focus = Some(h.id);
                        self.grab = Some(Grab {
                            id: h.id,
                            from: ev.pos,
                            start: h.point,
                            begun: false,
                            previewed: None,
                            stored: None,
                            at: h.point,
                            arrow: h.arrow,
                        });
                    }
                }
                out
            }
            PointerKind::Drag => {
                let Some(grab) = self.grab.as_mut() else { return vec![] };
                let Some(at) = within(ev.pos) else { return vec![] };
                let mut out = Vec::new();
                let (command, label) = command_of(grab.id.kind);
                if !grab.begun {
                    if ev.pos.distance(grab.from) <= cx.tol(DRAG_START_PX) {
                        return out;
                    }
                    grab.begun = true;
                    out.push(Action::Begin(label.into()));
                }
                let params = match grab.arrow {
                    // The head turns about the centre, staying as far out.
                    Some(arrow) => {
                        let Some(degrees) = aimed(arrow.centre, at, ev.mods.shift, cx.constrain_angle) else { return out };
                        grab.at = arrow.centre + direction(degrees) * cx.tol(ARROW_REACH_PX);
                        grab.arrow = Some(Arrow { degrees, ..arrow });
                        set_angle(grab.id, degrees)
                    }
                    None => {
                        // Moved by the drag, not set at the pointer: a handle grabbed off its point
                        // (an exit drawn apart from its entry, a press near the edge of its reach)
                        // doesn't jump.
                        let Some(at) = within(grab.start + (ev.pos - grab.from)) else { return out };
                        grab.at = at;
                        // No preview where the point stored wouldn't change (an open line's point
                        // stays at its nearer end while the pointer moves along it).
                        let (object, part) = grab.id.owner.part();
                        let stored = cx.embroidery.and_then(|f| f.snap(object, part, at));
                        if stored.is_some() && stored == grab.stored {
                            return out;
                        }
                        grab.stored = stored;
                        set_point(grab.id, Some(at))
                    }
                };
                // No preview where nothing would change.
                if grab.previewed.as_ref() != Some(&params) {
                    grab.previewed = Some(params.clone());
                    out.push(Action::Preview(command.into(), params));
                }
                out
            }
            PointerKind::Up => self.finish_drag(),
            // Ignored during a drag: the drag is the gesture under way, ended by its release.
            PointerKind::DoubleClick if self.busy() => vec![],
            PointerKind::DoubleClick => match hit(cx, &self.now(cx), ev.pos) {
                // An angle is typed in the Object tab's angle row.
                Some(h) if h.arrow.is_some() => vec![],
                Some(h) => {
                    self.grab = None;
                    self.dialog(&h)
                }
                None => vec![],
            },
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
        self.keyed = true;
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
        if let Some(arrow) = h.arrow {
            let step = ANGLE_KEY_STEP * if mods.shift { SHIFT_NUDGE } else { 1.0 };
            let by = match key {
                ToolKey::Delete | ToolKey::Backspace => return self.back_to_auto(h.id),
                ToolKey::Right | ToolKey::Up => step,
                ToolKey::Left | ToolKey::Down => -step,
                _ => return vec![],
            };
            return angle_of(arrow.degrees + by).map_or_else(Vec::new, |a| vec![Action::Exec(SET_SETTINGS.into(), set_angle(h.id, a))]);
        }
        let step = cx.keyboard_increment * if mods.shift { SHIFT_NUDGE } else { 1.0 };
        let by = match key {
            ToolKey::Delete | ToolKey::Backspace => return self.back_to_auto(h.id),
            ToolKey::Enter => return self.dialog(&h),
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
        let held = self.grab.as_ref().map(|g| g.id.kind).or_else(|| hit(cx, &self.now(cx), p).map(|h| h.id.kind));
        match held {
            Some(Kind::Entry | Kind::Exit) => Cursor::EmbroideryPoint,
            Some(Kind::Angle) => Cursor::Rotate,
            None => Cursor::Embroidery,
        }
    }

    fn options(&self) -> Value {
        let part = self.part.map_or(Value::Null, |k| json!(k.id()));
        let drag = self.grab.as_ref().filter(|g| g.begun).map_or(Value::Null, |g| {
            let mut v = g.id.json();
            v["at"] = json!([g.at.x, g.at.y]);
            if let Some(arrow) = g.arrow {
                v["angle"] = json!(arrow.degrees);
            }
            v
        });
        let id = |h: Option<HandleId>| h.map_or(Value::Null, HandleId::json);
        let hover_object = self.hover_object.map_or(Value::Null, |o| json!(o.0));
        json!({"part": part, "hover": id(self.hover), "hoverObject": hover_object, "focus": id(self.focus), "keyed": self.keyed && self.focus.is_some(), "drag": drag})
    }

    fn set_option(&mut self, key: &str, value: &Value) {
        match key {
            "part" => self.part = value.as_str().and_then(PartKind::from_id),
            // The handle the keys act on (Set on Canvas gives them the part's entry); `null` lets go.
            "focus" => self.focus = HandleId::from_json(value),
            _ => {}
        }
    }

    fn busy(&self) -> bool {
        self.grab.as_ref().is_some_and(|g| g.begun)
    }

    fn claims_key(&self, cx: &ToolContext, key: ToolKey) -> bool {
        match key {
            ToolKey::Escape => self.grab.is_some() || self.focus.is_some(),
            ToolKey::BracketLeft | ToolKey::BracketRight => !self.now(cx).is_empty(),
            // Enter asks for a point's X and Y: an angle has none.
            ToolKey::Enter => self.focused(cx).is_some_and(|h| h.arrow.is_none()),
            ToolKey::Delete | ToolKey::Backspace | ToolKey::Left | ToolKey::Right | ToolKey::Up | ToolKey::Down => self.focused(cx).is_some(),
            _ => false,
        }
    }

    /// Asked only when a command from outside ended the drag under way (Undo settles the
    /// interaction and takes it back): the handle is let go, nothing more to commit or cancel.
    fn after_command(&mut self, _cx: &ToolContext) -> Vec<Action> {
        if self.grab.as_ref().is_some_and(|g| g.begun) {
            self.grab = None;
        }
        vec![]
    }

    fn hover_ended(&mut self, _cx: &ToolContext) {
        self.hover = None;
        self.hover_object = None;
    }

    fn deactivate(&mut self, _cx: &ToolContext) -> Vec<Action> {
        self.hover = None;
        self.hover_object = None;
        self.focus = None;
        // Put away, it keeps no handles (the session drops the plan's answers it kept likewise).
        self.memo.get_mut().take();
        self.finish_drag()
    }
}
