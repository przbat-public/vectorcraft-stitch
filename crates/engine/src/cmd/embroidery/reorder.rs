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
//!
//! Sew Earlier, Later, First and Last ([`step_move`]) go where a drop at the gap past their
//! neighbour goes, except Sew Earlier, which goes just below the object sewn just before them, in
//! its parent: beside it, so an object in a group stays in it.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};

use super::plan::{DesignPlan, Part, Unit};
use super::*;
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

/// Objects to move, with what they cover worked out once: every node inside each of them (a unit is
/// dragged when its mover is one of those), so that a drag, which asks where a drop goes at every
/// frame, and a step cost a walk of the movers' subtrees and a pass over the units, never a search
/// of the document per unit and mover.
pub struct Moving {
    movers: Vec<NodeId>,
    inside: HashSet<NodeId>,
}

impl Moving {
    /// `movers` (objects of `doc`) and what they cover.
    pub fn new(doc: &Document, movers: Vec<NodeId>) -> Self {
        let mut inside = HashSet::new();
        for m in &movers {
            inside.insert(*m);
            if let Some(n) = doc.node(*m) {
                n.walk(&mut |c| {
                    inside.insert(c.id);
                });
            }
        }
        Self { movers, inside }
    }

    /// The objects moved.
    pub fn movers(&self) -> &[NodeId] {
        &self.movers
    }

    /// Whether unit `u` moves with them.
    fn drags(&self, u: &Unit) -> bool {
        self.inside.contains(&mover(u))
    }
}

/// Where a drop of `movers` at `gap` in `plan`'s sewing order goes in `doc` ([`place_moving`]).
pub fn place(doc: &Document, plan: &DesignPlan, movers: &[NodeId], gap: usize) -> std::result::Result<RowMove, Refusal> {
    if movers.len() > MAX_ROWS {
        return Err(Refusal::TooMany);
    }
    place_moving(doc, plan, &Moving::new(doc, movers.to_vec()), gap)
}

/// Where a drop of `moving` at `gap` in `plan`'s sewing order goes in `doc`: the gap lies before
/// unit `gap` (`plan.units.len()`: after the last). The objects go just above the object of the last
/// unit before the gap that isn't dragged, in its parent; when there is none, just below the object
/// of the first unit after it.
pub fn place_moving(doc: &Document, plan: &DesignPlan, moving: &Moving, gap: usize) -> std::result::Result<RowMove, Refusal> {
    let movers = moving.movers();
    if movers.is_empty() {
        return Err(Refusal::Nothing);
    }
    if movers.len() > MAX_ROWS {
        return Err(Refusal::TooMany);
    }
    if let Some(m) = movers.iter().find(|m| doc.node(**m).is_none()) {
        return Err(Refusal::Gone(*m));
    }
    let named: HashSet<NodeId> = movers.iter().copied().collect();
    if let Some(u) = plan.units.iter().find(|u| u.origin.part == Part::Literal && named.contains(&mover(u))) {
        return Err(Refusal::FromFile(mover(u)));
    }
    let dragged = |u: &Unit| moving.drags(u);
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
    beside(doc, moving, target, place)
}

/// The move of `moving` just above or below `target`, in its parent, or why it can't be made: the
/// refusals every move shares (a lock, a clipping path, a layer into a group, into or out of a clip
/// group), and none when they are there already.
fn beside(doc: &Document, moving: &Moving, target: NodeId, place: Place) -> std::result::Result<RowMove, Refusal> {
    let movers = moving.movers();
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

/// The movers of `ids` (objects of `doc`): the ids that cover a unit's mover, or lie inside one,
/// once each, in sewing order — what a selection moves (a selected group moves as one). One walk of
/// the ids' subtrees and ancestries, then one pass over the units.
pub fn movers_of(doc: &Document, plan: &DesignPlan, ids: &[NodeId]) -> Vec<NodeId> {
    // The given id each node belongs to: inside it, or above it (the first given wins).
    let mut of: HashMap<NodeId, NodeId> = HashMap::new();
    for id in ids {
        of.entry(*id).or_insert(*id);
        if let Some(n) = doc.node(*id) {
            n.walk(&mut |c| {
                of.entry(c.id).or_insert(*id);
            });
        }
        for above in doc.ancestry(*id).unwrap_or_default() {
            of.entry(above).or_insert(*id);
        }
    }
    let mut out: Vec<NodeId> = Vec::new();
    let mut seen: HashSet<NodeId> = HashSet::new();
    for u in &plan.units {
        if let Some(id) = of.get(&mover(u))
            && seen.insert(*id)
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
    gap_for_moving(plan, &Moving::new(doc, movers.to_vec()), step)
}

/// The object sewn just before the first unit of `moving`, when one is.
fn sewn_before(plan: &DesignPlan, moving: &Moving) -> Option<NodeId> {
    let first = plan.units.iter().position(|u| moving.drags(u))?;
    plan.units.get(..first)?.iter().rev().find(|u| !moving.drags(u)).map(mover)
}

/// [`gap_for`] of movers whose cover is worked out.
fn gap_for_moving(plan: &DesignPlan, moving: &Moving, step: Step) -> Option<usize> {
    let dragged = |u: &Unit| moving.drags(u);
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

/// What a change of the sewing order did, plan to plan ([`explain`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Explained {
    /// No stitch is sewn in another order: objects of one thread still join as before (the
    /// objects moved lie apart from what they passed).
    pub unchanged: bool,
    /// The thread changes before and after.
    pub changes: (usize, usize),
    /// Each object moved: the block it is sewn in now (from 1; its first unit's), and whether
    /// that block holds other objects too (it joined them).
    pub moved: Vec<MovedObject>,
}

/// An object moved, as [`Explained`] says it.
#[derive(Clone, Debug, PartialEq)]
pub struct MovedObject {
    pub id: NodeId,
    pub block: usize,
    pub joined: bool,
}

/// Each unit's mover and thread, in sewing order: what tells two sewing orders apart.
fn order_of(plan: &DesignPlan) -> Vec<(NodeId, Part, [u8; 3])> {
    plan.units.iter().map(|u| (mover(u), u.origin.part, u.thread.art)).collect()
}

/// What moving `moved` did, from the plan `before` to the plan `after`.
pub fn explain(before: &DesignPlan, after: &DesignPlan, moved: &[NodeId]) -> Explained {
    let changes = |p: &DesignPlan| p.blocks.len().saturating_sub(1);
    let moved = moved
        .iter()
        .filter_map(|id| {
            let unit = after.units.iter().find(|u| mover(u) == *id)?;
            let block = after.blocks.get(unit.block)?;
            let joined = after.units.get(block.units.clone()).unwrap_or_default().iter().any(|u| mover(u) != *id);
            Some(MovedObject { id: *id, block: unit.block + 1, joined })
        })
        .collect();
    Explained { unchanged: order_of(before) == order_of(after), changes: (changes(before), changes(after)), moved }
}

/// A move of the selection by the keys or the commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Earlier,
    Later,
    First,
    Last,
}

impl Step {
    fn id(self) -> &'static str {
        match self {
            Step::Earlier => "embroidery.sewEarlier",
            Step::Later => "embroidery.sewLater",
            Step::First => "embroidery.sewFirst",
            Step::Last => "embroidery.sewLast",
        }
    }
}

/// Why a move can't be made, as a message (the commands' error, the Sewing order list's hint,
/// translated by the interface's message catalogue).
pub fn reason(r: Refusal) -> &'static str {
    match r {
        Refusal::Nothing => "nothing selected is sewn",
        Refusal::Gone(_) => "can't change the sewing order: an object is no longer in the document",
        Refusal::Locked(_) => "can't change the sewing order: an object, or a layer or group it is in, is locked",
        Refusal::ClippingPath(_) => "can't change the sewing order: a clipping path stays at the bottom of its clip group",
        Refusal::ChangesClipping(_) => "can't change the sewing order: it would put an object into a clip group or take it out of one",
        Refusal::FromFile(_) => "can't change the sewing order: a run of an opened machine file moves with its block, so move its group",
        Refusal::NotAccepted(_) => "can't change the sewing order: a layer can't go into a group",
        Refusal::TargetLocked(_) => "can't change the sewing order: the layer or group it would go into is locked",
        Refusal::InsideObject => "can't change the sewing order: it would go between two parts of one object",
        Refusal::NoChange => "the sewing order is that already",
        Refusal::Nowhere => "can't change the sewing order: there is nothing to sew it beside",
        Refusal::TooMany => "can't change the sewing order: too many objects at once",
    }
}

/// A Sew Earlier, Later, First or Last move of the objects `ids` (each a selected group moving as
/// one) in `plan`'s sewing order: where they go, as a drop at the gap [`gap_for`] gives would go,
/// or why they can't, as the message the commands and the panel give. Pure: the panel works it out
/// on the plan its list shows, the commands on the plan they ask for.
pub fn step_move(doc: &Document, plan: &DesignPlan, ids: &[NodeId], step: Step) -> std::result::Result<RowMove, &'static str> {
    let moving = Moving::new(doc, movers_of(doc, plan, ids));
    let edge = match step {
        Step::Earlier | Step::First => "the selection is sewn first already",
        Step::Later | Step::Last => "the selection is sewn last already",
    };
    let gap = match gap_for_moving(plan, &moving, step) {
        Some(g) => g,
        None if moving.movers().is_empty() => return Err(reason(Refusal::Nothing)),
        None => return Err(edge),
    };
    let made = match (step, sewn_before(plan, &moving)) {
        // Just below the object sewn just before them, in its parent: beside it, in the group
        // they share with it (a drop at that gap would go above the object before that one, in
        // its parent, and could take them out of their group).
        (Step::Earlier, Some(object)) => beside(doc, &moving, object, Place::Below),
        _ => place_moving(doc, plan, &moving, gap),
    };
    made.map_err(|r| match r {
        Refusal::NoChange => edge,
        r => reason(r),
    })
}

/// Sew Earlier, Later, First or Last ([`step_move`]): the objects `ids` names (default: the
/// selection), planned with the embroidery options in `p`, or, when it gives none, the stitch
/// preview's (so that the move is made on the plan the Sewing order list shows, which the preview
/// has made already) → `{ids, target, place}` (`layer.move`'s params), one undo step named
/// [`LABEL`].
fn sew(s: &mut Session, p: &Value, step: Step) -> Result<Value> {
    let cmd = step.id();
    let o = super::object(p, cmd)?;
    let opts = if o.keys().any(|k| k != "ids") {
        super::options_in(&o, &["ids"], cmd)?
    } else {
        let uid = s.doc()?.uid;
        s.stitch_preview_options(uid).0
    };
    // Every id given checked (an array of objects of the document, at most as many as a move
    // takes), before anything is planned or walked; none given: the selection.
    if p.get("ids").and_then(Value::as_array).is_some_and(|a| a.len() > MAX_ROWS) {
        return Err(crate::EngineError::Other(reason(Refusal::TooMany).into()));
    }
    let ids = crate::cmd::checked_ids_or_selection(s, p, cmd)?;
    let (plan, doc) = super::planned(s, &opts, false, cmd)?;
    let row = step_move(&doc, &plan, &ids, step).map_err(|e| crate::EngineError::Other(e.into()))?;
    s.execute("command.batch", &row.batch())?;
    Ok(row.params())
}

/// The commands that change the sewing order by a step.
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "embroidery.sewEarlier",
            "Sew Earlier",
            [],
            None,
            "{ids?: [id…] (objects of the document, at most 100,000; default: the selection), …the embroidery options (default: the stitch preview's, those of the document's last embroidery export: the plan the Embroidery panel's Sewing order list shows)} move the objects (a selected group as one) in the stacking order so that they are sewn before the object sewn just before them: just below it, in its parent, one undo step \"Change Sewing Order\"; refused, saying why, where a drop in the Embroidery panel's Sewing order list would be (into or out of a clip group, a lock, a clipping path, a run of an opened machine file alone, between two parts of one object) → {ids, target, place} (layer.move's params). Objects of one thread still join a block sewn earlier when nothing between lies under them, so a move may change no stitch",
            has_doc,
            |s, p| sew(s, p, Step::Earlier)
        ),
        cmd!(
            "embroidery.sewLater",
            "Sew Later",
            [],
            None,
            "{ids?: [id…] (default: the selection), …the embroidery options} as embroidery.sewEarlier, after the object sewn just after them: just above it, in its parent → {ids, target, place}",
            has_doc,
            |s, p| sew(s, p, Step::Later)
        ),
        cmd!(
            "embroidery.sewFirst",
            "Sew First",
            [],
            None,
            "{ids?: [id…] (default: the selection), …the embroidery options} as embroidery.sewEarlier, before everything: just below the object sewn first, in its parent → {ids, target, place}",
            has_doc,
            |s, p| sew(s, p, Step::First)
        ),
        cmd!(
            "embroidery.sewLast",
            "Sew Last",
            [],
            None,
            "{ids?: [id…] (default: the selection), …the embroidery options} as embroidery.sewEarlier, after everything: just above the object sewn last, in its parent → {ids, target, place}",
            has_doc,
            |s, p| sew(s, p, Step::Last)
        ),
    ]
}

#[cfg(test)]
mod tests;
