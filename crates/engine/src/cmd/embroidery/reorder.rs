//! Changing the sewing order: where a drop in the Sewing order list goes, as `layer.move`'s params,
//! or why it can't go there.
//!
//! The sewing order follows the stacking order, so a drop moves objects in the stack: the objects
//! dragged go **just above the object sewn before the gap, in that object's parent** (the project
//! owner's rule; just below the object sewn after it when the gap is the first). One `layer.move`
//! makes it one undo step, labelled "Change Sewing Order" by the batch that runs it.
//!
//! A drag changes **when** something is sewn, never **what**: a move that would put an object into a
//! clip group or take it out of one (decision 11), move a clip group's clipping path, move a locked
//! object or into a locked layer or group, or split an object's own stitches is refused, typed
//! ([`Refusal`]), so the panel can say why before the drop. A run of an opened machine file moves
//! with its block (its group), never alone.

use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};

use super::plan::{DesignPlan, Part, Unit};
use crate::cmd::layer::{MAX_ROWS, accepts, locked_within};

/// The undo step's name for every change of the sewing order (drags, keys, commands).
pub const LABEL: &str = "Change Sewing Order";

/// Which side of the target the objects go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Just above it (sewn after it).
    Above,
    /// Just below it (sewn before it).
    Below,
}

/// A drop that can be made: `layer.move {ids, target, place}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowMove {
    /// The objects moved, in paint order (kept in that order).
    pub ids: Vec<NodeId>,
    pub target: NodeId,
    pub place: Place,
}

impl RowMove {
    /// `layer.move`'s params.
    pub fn params(&self) -> Value {
        let place = match self.place {
            Place::Above => "above",
            Place::Below => "below",
        };
        json!({ "ids": self.ids.iter().map(|i| i.0).collect::<Vec<_>>(), "target": self.target.0, "place": place })
    }

    /// The batch that makes it, one undo step named [`LABEL`].
    pub fn batch(&self) -> Value {
        json!({ "label": LABEL, "commands": [{ "command": "layer.move", "params": self.params() }] })
    }
}

/// Why a drop can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing is dragged.
    Nothing,
    /// The object is no longer in the document (the list is older than the document).
    Gone(NodeId),
    /// The object, or a layer or group it is in, is locked.
    Locked(NodeId),
    /// The object is a clip group's clipping path: it stays at the bottom of its group.
    ClippingPath(NodeId),
    /// The move would put the object into a clip group, or take it out of one: what is sewn would
    /// change, not only when.
    ChangesClipping(NodeId),
    /// The object is a run of an opened machine file: it moves with its block.
    FromFile(NodeId),
    /// The object can't go where the gap is (a layer into a group).
    NotAccepted(NodeId),
    /// The layer or group the objects would go into is locked.
    TargetLocked(NodeId),
    /// The gap lies between two rows of one object (its fill and its stroke, a symbol instance's
    /// parts).
    InsideObject,
    /// The objects are there already.
    NoChange,
    /// There is nothing to place them beside.
    Nowhere,
    /// More objects than one move takes.
    TooMany,
}

impl Refusal {
    /// The id replies give it.
    pub fn id(self) -> &'static str {
        match self {
            Refusal::Nothing => "nothing",
            Refusal::Gone(_) => "gone",
            Refusal::Locked(_) => "locked",
            Refusal::ClippingPath(_) => "clippingPath",
            Refusal::ChangesClipping(_) => "changesClipping",
            Refusal::FromFile(_) => "fromFile",
            Refusal::NotAccepted(_) => "notAccepted",
            Refusal::TargetLocked(_) => "targetLocked",
            Refusal::InsideObject => "insideObject",
            Refusal::NoChange => "noChange",
            Refusal::Nowhere => "nowhere",
            Refusal::TooMany => "tooMany",
        }
    }

    /// The object it is about, if one.
    pub fn object(self) -> Option<NodeId> {
        match self {
            Refusal::Gone(id)
            | Refusal::Locked(id)
            | Refusal::ClippingPath(id)
            | Refusal::ChangesClipping(id)
            | Refusal::FromFile(id)
            | Refusal::NotAccepted(id)
            | Refusal::TargetLocked(id) => Some(id),
            _ => None,
        }
    }
}

/// What a person moves for unit `u`: the outermost symbol instance it is drawn through, else its
/// object ([`super::plan::Origin::selected`]).
pub fn mover(u: &Unit) -> NodeId {
    u.origin.selected()
}

/// The clip groups and clipping layers `id` and the containers above it are in, innermost first:
/// what clips an object put there.
fn clip_context(doc: &Document, id: Option<NodeId>) -> Vec<NodeId> {
    let Some(id) = id else { return Vec::new() };
    let mut out: Vec<NodeId> = doc.ancestry(id).unwrap_or_default().into_iter().filter(|a| doc.node(*a).is_some_and(|n| n.clips())).collect();
    out.reverse();
    out
}

/// Whether `id` is a clip group's (or clipping layer's) clipping path.
fn clipping_path(doc: &Document, id: NodeId) -> bool {
    doc.parent_of(id).and_then(|p| doc.node(p)).is_some_and(|p| p.clips() && p.children().and_then(|c| c.first()).is_some_and(|first| first.id == id))
}

/// Whether `inner` is `outer` or lies inside it.
fn within(doc: &Document, inner: NodeId, outer: NodeId) -> bool {
    inner == outer || doc.ancestry(inner).is_some_and(|a| a.contains(&outer))
}

/// Where a drop of `movers` at `gap` in `plan`'s sewing order goes in `doc`: the gap lies before
/// unit `gap` (`plan.units.len()`: after the last). The objects go just above the object of the last
/// unit before the gap that isn't dragged, in its parent; when there is none, just below the object
/// of the first unit after it.
pub fn place(doc: &Document, plan: &DesignPlan, movers: &[NodeId], gap: usize) -> Result<RowMove, Refusal> {
    if movers.is_empty() {
        return Err(Refusal::Nothing);
    }
    if movers.len() > MAX_ROWS {
        return Err(Refusal::TooMany);
    }
    for m in movers {
        if doc.node(*m).is_none() {
            return Err(Refusal::Gone(*m));
        }
        if plan.units.iter().any(|u| u.origin.part == Part::Literal && mover(u) == *m) {
            return Err(Refusal::FromFile(*m));
        }
    }
    let dragged = |u: &Unit| movers.iter().any(|m| within(doc, mover(u), *m));
    let gap = gap.min(plan.units.len());
    let prev = plan.units.get(..gap).unwrap_or_default().iter().rev().find(|u| !dragged(u));
    let next = plan.units.get(gap..).unwrap_or_default().iter().find(|u| !dragged(u));
    if let (Some(p), Some(n)) = (prev, next)
        && mover(p) == mover(n)
    {
        return Err(Refusal::InsideObject);
    }
    let (target, place) = match (prev, next) {
        (Some(p), _) => (mover(p), Place::Above),
        (None, Some(n)) => (mover(n), Place::Below),
        (None, None) => return Err(Refusal::Nowhere),
    };
    let parent = doc.parent_of(target);
    let context = clip_context(doc, parent);
    for m in movers {
        let n = doc.node(*m).ok_or(Refusal::Gone(*m))?;
        if locked_within(doc, *m) {
            return Err(Refusal::Locked(*m));
        }
        if clipping_path(doc, *m) {
            return Err(Refusal::ClippingPath(*m));
        }
        if !accepts(doc, parent, n) {
            return Err(Refusal::NotAccepted(*m));
        }
        if clip_context(doc, doc.parent_of(*m)) != context {
            return Err(Refusal::ChangesClipping(*m));
        }
    }
    if let Some(p) = parent
        && locked_within(doc, p)
    {
        return Err(Refusal::TargetLocked(p));
    }
    // The movers, outermost only, in paint order.
    let ids = crate::cmd::layer::row_roots(doc, movers);
    // Already there: each just above the one before it, from just above (or below) the target.
    let siblings: Vec<NodeId> = doc.children(parent).map(|c| c.iter().map(|n| n.id).collect()).unwrap_or_default();
    if let Some(t) = siblings.iter().position(|s| *s == target) {
        let run = match place {
            Place::Above => siblings.get(t + 1..t + 1 + ids.len()),
            Place::Below => t.checked_sub(ids.len()).and_then(|start| siblings.get(start..t)),
        };
        if run == Some(ids.as_slice()) {
            return Err(Refusal::NoChange);
        }
    }
    Ok(RowMove { ids, target, place })
}

/// The movers of `ids` (objects of `doc`): the units' movers each covers, once, in sewing order —
/// what a selection moves (a selected group moves as one).
pub fn movers_of(doc: &Document, plan: &DesignPlan, ids: &[NodeId]) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = Vec::new();
    for u in &plan.units {
        let m = mover(u);
        if let Some(id) = ids.iter().find(|id| within(doc, m, **id) || within(doc, **id, m))
            && !out.contains(id)
        {
            out.push(*id);
        }
    }
    out
}

/// The gap a Sew Earlier, Later, First or Last move of `movers` tries first: before the object
/// sewn just before them, after the one sewn just after them, before everything, after
/// everything. `None` when there is nothing to pass.
pub fn gap_for(doc: &Document, plan: &DesignPlan, movers: &[NodeId], step: Step) -> Option<usize> {
    let dragged = |u: &Unit| movers.iter().any(|m| within(doc, mover(u), *m));
    let first = plan.units.iter().position(dragged)?;
    let last = plan.units.iter().rposition(dragged)?;
    match step {
        Step::First => Some(0),
        Step::Last => Some(plan.units.len()),
        Step::Earlier => {
            // The object sewn just before the first dragged unit, and where its run of units starts.
            let before = plan.units.get(..first)?.iter().rposition(|u| !dragged(u))?;
            let object = plan.units.get(before).map(mover)?;
            let start = plan.units.get(..=before)?.iter().rposition(|u| mover(u) != object && !dragged(u)).map_or(0, |i| i + 1);
            Some(start)
        }
        Step::Later => {
            let after = last + 1 + plan.units.get(last + 1..)?.iter().position(|u| !dragged(u))?;
            let object = plan.units.get(after).map(mover)?;
            let end = after + plan.units.get(after..)?.iter().position(|u| mover(u) != object && !dragged(u)).unwrap_or(plan.units.len() - after);
            Some(end)
        }
    }
}

/// A move of the selection by the keys or the commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Earlier,
    Later,
    First,
    Last,
}

#[cfg(test)]
mod tests;
