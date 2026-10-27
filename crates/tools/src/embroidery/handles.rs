//! The embroidery tool's handles ([`handles`]): one list the tool catches and the canvas draws, so
//! that what is drawn is what is caught. Each handle is named by its kind, its owner and an index,
//! never kept by position: positions come from the document and the plan on every call, so a new
//! plan, an undo or a transform never leaves a handle behind.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::takers::{first_takers, has_part};
use vectorcraft_doc::embroidery::{PartKey, PartKind, PartValue};
use vectorcraft_geom::{Point, Vec2};

use crate::ToolContext;

/// The most objects whose points show at once (the stitch preview's markers' bound, an estimate:
/// as many as a person can still tell apart).
pub const HANDLED_OBJECTS_MAX: usize = 32;

thread_local! {
    /// How many times the handles were found on this thread ([`handles`]), by the tool and the
    /// canvas alike: what the tests count, so that a frame that changes nothing finds none.
    static WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times the handles were found on this thread ([`WALKS`]).
pub fn handle_walks() -> usize {
    WALKS.with(std::cell::Cell::get)
}

/// The entry handle's size on screen, pixels (the stitch preview's entry marker's).
pub const ENTRY_MARKER_PX: f64 = 12.0;

/// The exit handle's size on screen, pixels (the stitch preview's exit marker's).
pub const EXIT_MARKER_PX: f64 = 14.0;

/// How much farther than half its size a handle is caught, screen pixels: as an anchor is, 2 px
/// past its drawn edge ([`ToolContext::point_tol`]).
pub const CATCH_PAST_PX: f64 = 2.0;

/// How far apart on screen an exit lying under its entry is drawn and caught, pixels: the exit
/// marker's size, so both can be caught.
pub const APART_PX: f64 = EXIT_MARKER_PX;

/// The angle arrow's head, its size on screen, pixels (an estimate: the entry marker's, as easy to
/// catch as the points beside it).
pub const ARROW_HEAD_PX: f64 = 12.0;

/// How far from the fill's centre the angle arrow's head is drawn and caught, screen pixels; the
/// arrow runs as far the other way (an estimate: far enough out to aim the rows by a degree, near
/// enough to stay over most fills at the zoom a design is digitized at).
pub const ARROW_REACH_PX: f64 = 40.0;

/// A kind of handle. Closed on purpose: every place kinds differ matches on it, so a kind added
/// later can't leave out its handles, its drag's command or its cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Where the needle goes in to sew a part.
    Entry,
    /// Where it comes out after it.
    Exit,
    /// The direction a fill's tatami rows run: the head of an arrow through the fill's centre.
    Angle,
}

impl Kind {
    /// Its id, as the tool's options and `embroidery.setEntryExit`'s keys name it.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Entry => "entry",
            Kind::Exit => "exit",
            Kind::Angle => "angle",
        }
    }

    /// The part key its value is kept under.
    fn key(self) -> PartKey {
        match self {
            Kind::Entry => PartKey::Entry,
            Kind::Exit => PartKey::Exit,
            Kind::Angle => PartKey::Angle,
        }
    }

    /// The handle's size on screen, pixels.
    pub fn size_px(self) -> f64 {
        match self {
            Kind::Entry => ENTRY_MARKER_PX,
            Kind::Exit => EXIT_MARKER_PX,
            Kind::Angle => ARROW_HEAD_PX,
        }
    }
}

/// What a handle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// A part of an object: its fill or its stroke.
    Part { object: NodeId, part: PartKind },
}

impl Owner {
    /// The object and its part.
    pub fn part(self) -> (NodeId, PartKind) {
        match self {
            Owner::Part { object, part } => (object, part),
        }
    }
}

/// What a handle's value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Set on the object.
    Set,
    /// Auto: a point where the plan shown sews it, the plan of the object as it is; an angle the
    /// export's or the default rows, turned with the object.
    Auto,
    /// Auto, from a plan of the object as it was: drawn dim until the plan catches up.
    Stale,
}

impl State {
    pub fn id(self) -> &'static str {
        match self {
            State::Set => "set",
            State::Auto => "auto",
            State::Stale => "stale",
        }
    }
}

/// What names a handle from one call to the next: its kind, its owner, and which of the owner's
/// handles of that kind (0 for an entry, an exit or an angle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HandleId {
    pub kind: Kind,
    pub owner: Owner,
    pub index: usize,
}

impl HandleId {
    /// As the tool's options give it: `{kind, id, part, index}`.
    pub fn json(self) -> Value {
        let (object, part) = self.owner.part();
        json!({"kind": self.kind.id(), "id": object.0, "part": part.id(), "index": self.index})
    }

    /// The handle `v` names as [`HandleId::json`] gives it (`index` 0 when left out); `None` for
    /// anything else.
    pub fn from_json(v: &Value) -> Option<Self> {
        let kind = [Kind::Entry, Kind::Exit, Kind::Angle].into_iter().find(|k| v["kind"].as_str() == Some(k.id()))?;
        let part = v["part"].as_str().and_then(PartKind::from_id)?;
        let object = NodeId(v["id"].as_u64()?);
        let index = match &v["index"] {
            Value::Null => 0,
            i => usize::try_from(i.as_u64()?).ok()?,
        };
        Some(Self { kind, owner: Owner::Part { object, part }, index })
    }
}

/// One handle as the tool catches it and the canvas draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub id: HandleId,
    /// Where it is drawn and caught, document points: its point, but an exit lying under its entry
    /// is moved apart on screen ([`APART_PX`]).
    pub at: Point,
    /// The point it stands for: the one set, or where the plan sews it.
    pub point: Point,
    pub state: State,
    /// Where the plan shown starts or ends the part, when it says (for a point set, where the
    /// stitches really start or end: the canvas joins the two when they lie apart).
    pub planned: Option<Point>,
    /// Where the needle comes from before an entry, or goes to after an exit, along the thread: the
    /// jump a drag shows. And the trim length a jump longer than is cut (document points).
    pub neighbour: Option<Point>,
    pub trim: Option<f64>,
    /// An angle handle's arrow (its head is the handle's point); `None` for a point.
    pub arrow: Option<Arrow>,
}

/// An angle arrow: the fill's centre it turns about, document points, and the angle it points at,
/// degrees counter-clockwise on the page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arrow {
    pub centre: Point,
    pub degrees: f64,
}

/// The direction `degrees` counter-clockwise on the page points, a unit vector in document
/// coordinates (whose y axis points down the page).
pub fn direction(degrees: f64) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(c, -s)
}

/// The part of `n` whose points show: `part` (else its fill) when `n` has it, else its other part,
/// as the Object tab shows the other part of objects that don't paint the one chosen; `None` when it
/// has neither.
fn shown_part(n: &vectorcraft_doc::Node, part: Option<PartKind>) -> Option<PartKind> {
    let first = part.unwrap_or(PartKind::Fill);
    let other = match first {
        PartKind::Fill => PartKind::Stroke,
        PartKind::Stroke => PartKind::Fill,
    };
    [first, other].into_iter().find(|k| has_part(n, *k))
}

/// The selection's handles: of the objects it stands for that take settings (groups expanded, a
/// compound path's member standing for its compound path; type, symbol instances and stitches from
/// files take none), at most [`HANDLED_OBJECTS_MAX`], each object's entry, its exit and, for a fill
/// sewn as tatami, its rows' angle, of the part `part` (one part at a time, as the Object tab shows
/// it: an object without that part shows its other; `None`: the fill). A point set stands where it
/// is; one left to the planner where the plan shown sews it (stale when that plan is of the object
/// as it was); without either, no handle. The angle arrow runs through the fill's centre (its
/// bounds'), its head [`ARROW_REACH_PX`] out on screen.
pub fn handles(cx: &ToolContext, part: Option<PartKind>) -> Vec<Handle> {
    WALKS.with(|w| w.set(w.get().saturating_add(1)));
    let take = first_takers(cx.doc, &cx.selection.objects, HANDLED_OBJECTS_MAX);
    // One screen pixel to the right, in document points, whatever way the view is turned.
    let right = cx.screen.map_or(Vec2::new(cx.tol(1.0), 0.0), |s| s.right);
    let mut out = Vec::new();
    for object in take {
        let Some(n) = cx.doc.node(object) else { continue };
        let Some(kind) = shown_part(n, part) else { continue };
        let held = n.extras().embroidery.as_deref().and_then(|e| e.part(kind));
        let stored = |key: PartKey| match held.and_then(|p| p.get(key)) {
            Some(PartValue::Point(p)) if p.x.is_finite() && p.y.is_finite() => Some(p),
            _ => None,
        };
        let planned = cx.embroidery.and_then(|f| f.ends(object, kind));
        let mut pair: Vec<Handle> = Vec::with_capacity(2);
        for k in [Kind::Entry, Kind::Exit] {
            let sewn = planned.map(|e| if k == Kind::Entry { e.first } else { e.last });
            let (point, state) = match (stored(k.key()), planned, sewn) {
                (Some(p), _, _) => (p, State::Set),
                (None, Some(e), Some(at)) => (at, if e.current { State::Auto } else { State::Stale }),
                _ => continue,
            };
            let id = HandleId { kind: k, owner: Owner::Part { object, part: kind }, index: 0 };
            let neighbour = planned.and_then(|e| if k == Kind::Entry { e.before } else { e.after });
            pair.push(Handle { id, at: point, point, state, planned: sewn, neighbour, trim: planned.and_then(|e| e.trim), arrow: None });
        }
        if let [entry, exit] = pair.as_mut_slice()
            && entry.at.distance(exit.at) < cx.tol(APART_PX)
        {
            exit.at = entry.at + right * APART_PX;
        }
        out.extend(pair);
        if kind == PartKind::Fill
            && let Some(rows) = cx.embroidery.and_then(|f| f.fill_angle(object))
            && let Some(centre) = cx.doc.bounds_of(&[object], false).map(|r| r.center())
        {
            let head = centre + direction(rows.degrees) * cx.tol(ARROW_REACH_PX);
            let id = HandleId { kind: Kind::Angle, owner: Owner::Part { object, part: kind }, index: 0 };
            let state = if rows.set { State::Set } else { State::Auto };
            let arrow = Some(Arrow { centre, degrees: rows.degrees });
            out.push(Handle { id, at: head, point: head, state, planned: None, neighbour: None, trim: None, arrow });
        }
    }
    out
}

/// The handle of `all` under `p` (document points): the nearest within half its size and
/// [`CATCH_PAST_PX`] on screen, or the selection tolerance where that is wider; of equally near
/// ones, the first (an entry before its exit).
pub fn hit(cx: &ToolContext, all: &[Handle], p: Point) -> Option<Handle> {
    hit_after(cx, all, p, None)
}

/// [`hit`], but where handles lie on one another (adjacent objects' Auto points, concentric fills'
/// arrows), the one after `after` among them when that is one of them: pressing there again goes
/// through them in turn.
pub fn hit_after(cx: &ToolContext, all: &[Handle], p: Point, after: Option<HandleId>) -> Option<Handle> {
    let mut best: Option<(f64, Handle)> = None;
    let mut near: Vec<(f64, Handle)> = Vec::new();
    for h in all {
        let reach = cx.tol(cx.selection_tolerance.max(h.id.kind.size_px() / 2.0 + CATCH_PAST_PX));
        let d = h.at.distance(p);
        if d <= reach {
            near.push((d, *h));
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, *h));
            }
        }
    }
    let (nearest, first) = best?;
    // The ones as near as the nearest (on one another, to a hair), in the list's order.
    let tied: Vec<Handle> = near
        .into_iter()
        .filter(|(d, h)| (d - nearest).abs() <= cx.tol(SAME_SPOT_PX) && h.at.distance(first.at) <= cx.tol(SAME_SPOT_PX))
        .map(|(_, h)| h)
        .collect();
    let next = after.and_then(|a| tied.iter().position(|h| h.id == a)).and_then(|k| tied.get((k + 1) % tied.len()));
    Some(next.copied().unwrap_or(first))
}

/// How far apart on screen two handles may lie and still lie on one another, pixels: a hair.
const SAME_SPOT_PX: f64 = 0.5;
