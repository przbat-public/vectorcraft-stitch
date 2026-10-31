//! What the Embroidery panel's Stitches tab reads of the plan the canvas shows, never by asking a
//! command per scroll position: the design's records as the Sewing order and the Check tab number
//! them (every needle point, jump, cut and stop, from 1).
//!
//! - **One byte a record** ([`Flags`]): its kind and whether its stitch breaks the limits (the one
//!   set of [`StitchLimits`]), what the tab's filters count and pick. Built in one walk of the plan,
//!   once a plan and limits, on the planning thread (measured at 15.7 ms on 2 million records,
//!   past what a frame draws in line), and kept with the plan ([`super::Planned::stitch_flags`]).
//! - **A row** ([`row`]): one record's kind, place, length, block, unit and object, found in the plan
//!   by its number (a binary search), only for the rows in view.
//! - **A scope** ([`scope`]): the record numbers of the units sewing some objects, for the tab's
//!   Selection.
//! - **The needle point a record is edited by** ([`numbering`], [`ordinals`]): as
//!   `embroidery.stitches` names it, worked out a unit at a time.

use std::collections::HashSet;
use std::ops::Range;

use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;
use vectorcraft_stitch::Command;

use super::check::StitchLimits;
use super::plan::{DesignPlan, Part};

/// A record's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    Stitch,
    Jump,
    Trim,
    Stop,
}

impl RecordKind {
    /// As `embroidery.stitches` names it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Stitch => "stitch",
            Self::Jump => "jump",
            Self::Trim => "trim",
            Self::Stop => "stop",
        }
    }

    fn of(command: Command) -> Self {
        match command {
            Command::Stitch => Self::Stitch,
            Command::Jump => Self::Jump,
            Command::Trim => Self::Trim,
            Command::Stop | Command::ColorChange => Self::Stop,
        }
    }

    fn bits(self) -> u8 {
        match self {
            Self::Stitch => 0,
            Self::Jump => 1,
            Self::Trim => 2,
            Self::Stop => 3,
        }
    }

    fn from_bits(bits: u8) -> Self {
        match bits & KIND_BITS {
            1 => Self::Jump,
            2 => Self::Trim,
            3 => Self::Stop,
            _ => Self::Stitch,
        }
    }
}

/// The Stitches tab's filters: every record, or one kind, or the stitches that break the limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Filter {
    #[default]
    All,
    Jumps,
    Trims,
    Stops,
    Short,
    Long,
}

impl Filter {
    pub const ALL: [Filter; 6] = [Filter::All, Filter::Jumps, Filter::Trims, Filter::Stops, Filter::Short, Filter::Long];

    /// Its id, as `ui.embroideryPanel {filter}` names it.
    pub fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Jumps => "jumps",
            Self::Trims => "trims",
            Self::Stops => "stops",
            Self::Short => "short",
            Self::Long => "long",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }
}

/// A record's kind in its byte's low bits, and whether its stitch is too short or too long.
const KIND_BITS: u8 = 0b11;
const SHORT_BIT: u8 = 0b100;
const LONG_BIT: u8 = 0b1000;

/// Each record's kind and judgement, one byte a record (record `n` at `n - 1`), for the limits it
/// was built with.
#[derive(Clone, Debug, PartialEq)]
pub struct Flags {
    bits: Vec<u8>,
    limits: [u64; 3],
}

/// `limits` as a query's params carry them to the planning thread: `{shortest, longest, step,
/// format}` (millimetres, the format's id).
pub fn limits_params(limits: &StitchLimits) -> serde_json::Value {
    serde_json::json!({"shortest": limits.shortest, "longest": limits.longest, "step": limits.step, "format": limits.format.id()})
}

/// The limits `params` carry ([`limits_params`]); `None` when they don't (a number missing or not
/// finite, a format no format has).
pub fn limits_from(params: &serde_json::Value) -> Option<StitchLimits> {
    let number = |k: &str| params.get(k).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite());
    let format = params.get("format").and_then(serde_json::Value::as_str).and_then(vectorcraft_stitch::format::Format::from_id)?;
    Some(StitchLimits { shortest: number("shortest")?, longest: number("longest")?, step: number("step")?, format })
}

/// The bits of `limits`, what flags are kept for.
pub fn limits_key(limits: &StitchLimits) -> [u64; 3] {
    [limits.shortest.to_bits(), limits.longest.to_bits(), limits.step.to_bits()]
}

impl Flags {
    /// The flags of `plan`'s records judged against `limits`: one walk, stopped early (and then
    /// incomplete) when the work is cancelled.
    pub fn of(plan: &DesignPlan, limits: &StitchLimits) -> Self {
        let mut bits = Vec::with_capacity(plan.blocks.iter().map(|b| b.stitches.len()).sum());
        super::measure::each_until_cancelled(plan, |r| {
            let mut b = RecordKind::of(r.stitch.command).bits();
            if let Some(length) = r.sewn() {
                if limits.short(length) {
                    b |= SHORT_BIT;
                }
                if let Some(before) = r.before
                    && limits.long(r.stitch.x - before.x, r.stitch.y - before.y)
                {
                    b |= LONG_BIT;
                }
            }
            // Records come in number order, from 1, without gaps.
            if let Some(slot) = r.number.checked_sub(1) {
                if slot >= bits.len() {
                    bits.resize(slot + 1, 0);
                }
                if let Some(x) = bits.get_mut(slot) {
                    *x = b;
                }
            }
        });
        Self { bits, limits: limits_key(limits) }
    }

    /// The limits' bits it was built with.
    pub fn limits(&self) -> [u64; 3] {
        self.limits
    }

    /// How many records it holds.
    pub fn len(&self) -> usize {
        self.bits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }

    /// Record `number`'s kind.
    pub fn kind(&self, number: usize) -> Option<RecordKind> {
        self.bits.get(number.checked_sub(1)?).map(|b| RecordKind::from_bits(*b))
    }

    /// Whether record `number` is listed under `filter`.
    pub fn matches(&self, number: usize, filter: Filter) -> bool {
        let Some(&b) = number.checked_sub(1).and_then(|i| self.bits.get(i)) else { return false };
        match filter {
            Filter::All => true,
            Filter::Jumps => RecordKind::from_bits(b) == RecordKind::Jump,
            Filter::Trims => RecordKind::from_bits(b) == RecordKind::Trim,
            Filter::Stops => RecordKind::from_bits(b) == RecordKind::Stop,
            Filter::Short => b & SHORT_BIT != 0,
            Filter::Long => b & LONG_BIT != 0,
        }
    }

    /// Whether record `number`'s stitch breaks the limits: too short, too long.
    pub fn judged(&self, number: usize) -> [bool; 2] {
        let b = number.checked_sub(1).and_then(|i| self.bits.get(i)).copied().unwrap_or(0);
        [b & SHORT_BIT != 0, b & LONG_BIT != 0]
    }

    /// About how many bytes it holds.
    pub fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.bits.capacity()
    }
}

/// One record as the tab lists it: its number and kind, where it is (millimetres from the
/// document's origin, y down), the thread its stitch lays (a needle point's from the one before
/// it, a jump's move; `None` for the needle point a jump or a cut lands on, a cut, a stop), its
/// block and unit (indices into the plan), and the object a person selects for it and the path
/// sewn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub record: usize,
    pub kind: RecordKind,
    pub at: Point,
    pub length: Option<f64>,
    pub block: usize,
    pub unit: usize,
    pub object: NodeId,
    pub node: NodeId,
}

/// Record `number` of `plan` as [`Row`] gives it, as `embroidery.stitches` measures it.
pub fn row(plan: &DesignPlan, number: usize) -> Option<Row> {
    let (k, index) = plan.locate(number)?;
    let block = plan.blocks.get(k)?;
    let stitch = *block.stitches.get(index)?;
    let unit = plan.unit_at(number)?;
    let u = plan.units.get(unit)?;
    // The record before it in the design: in its block, else the last of the block before.
    let before = match index.checked_sub(1) {
        Some(i) => block.stitches.get(i).copied(),
        None => k.checked_sub(1).and_then(|b| plan.blocks.get(b)).and_then(|b| b.stitches.last().copied()),
    };
    let length = match stitch.command {
        Command::Stitch => before.filter(|b| index > 0 && b.command == Command::Stitch).map(|b| b.distance(stitch)),
        Command::Jump => Some(before.map_or(0.0, |b| b.distance(stitch))),
        _ => None,
    };
    Some(Row {
        record: number,
        kind: RecordKind::of(stitch.command),
        at: stitch.point(),
        length,
        block: k,
        unit,
        object: u.origin.selected(),
        node: u.origin.node,
    })
}

/// The records of the units sewing the objects inside `ids` of `doc` (each object and what it
/// holds; the object a person selects, or the path sewn), as ranges of record numbers in sewing
/// order, those that follow one another joined.
pub fn scope(plan: &DesignPlan, doc: &Document, ids: &[NodeId]) -> Vec<Range<usize>> {
    let mut inside: HashSet<NodeId> = HashSet::new();
    for id in ids {
        if let Some(n) = doc.node(*id) {
            n.walk(&mut |c| {
                inside.insert(c.id);
            });
        }
    }
    let mut out: Vec<Range<usize>> = Vec::new();
    for b in &plan.blocks {
        for k in b.units.clone() {
            let Some(u) = plan.units.get(k) else { continue };
            if !(inside.contains(&u.origin.selected()) || inside.contains(&u.origin.node)) {
                continue;
            }
            let r = b.first_number + u.stitches.start..b.first_number + u.stitches.end;
            if r.is_empty() {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => out.push(r),
            }
        }
    }
    out
}

/// The path unit `unit` sews where its records number the path's needle points, as
/// `embroidery.stitches` names them, and the needle point its first own needle-down record is (1
/// when it starts where the needle already was); `None` for a unit that isn't one (planned art, a
/// symbol's art, a file's run cut by a clip group, a locked or hidden path, a path whose needle
/// points the records don't count one to one).
pub fn numbering(plan: &DesignPlan, doc: &Document, unit: usize) -> Option<(NodeId, usize)> {
    let u = plan.units.get(unit)?;
    let node = u.origin.node;
    let manual = matches!(u.origin.part, Part::Manual);
    if !(manual || matches!(u.origin.part, Part::Literal))
        || u.origin.instance.is_some_and(|i| i != node && !doc.node(i).is_some_and(super::digitize::is_converted))
    {
        return None;
    }
    let n = doc.node(node)?;
    let clipped = !manual && doc.ancestry(node).unwrap_or_default().iter().any(|a| doc.node(*a).is_some_and(|a| a.clips()));
    if !doc.is_editable(node) || clipped {
        return None;
    }
    let needles = super::needles::check(n).ok()?;
    let block = plan.blocks.get(u.block)?;
    let sewn = block.stitches.get(u.own.clone()).map_or(0, |s| s.iter().filter(|s| s.command == Command::Stitch).count());
    let first = usize::from(u.met);
    (needles == sewn + first).then_some((node, first))
}

/// For each record of unit `unit` (its range, [`DesignPlan::numbers`]), how many of the unit's own
/// needle-down records come before it: the needle point a record is edited by is the numbering's
/// first plus this (a needle point, the one a jump lands on), or one less (a cut or a stop: the
/// needle point before it).
pub fn ordinals(plan: &DesignPlan, unit: usize) -> Vec<u32> {
    let Some(u) = plan.units.get(unit) else { return Vec::new() };
    let Some(block) = plan.blocks.get(u.block) else { return Vec::new() };
    let mut seen = 0u32;
    u.stitches
        .clone()
        .map(|i| {
            let here = seen;
            if u.own.contains(&i) && block.stitches.get(i).is_some_and(|s| s.command == Command::Stitch) {
                seen = seen.saturating_add(1);
            }
            here
        })
        .collect()
}

/// The needle point record `number` (of kind `kind`) of unit `unit` is edited by, its numbering's
/// `first` and `ordinals` given ([`numbering`], [`ordinals`]): only a record of the unit's own
/// stitches (not the move that reaches it, not the cut or stop its object asks for after it); a
/// needle point itself, the one a jump lands on, the one before a cut or a stop.
pub fn edited_by(plan: &DesignPlan, unit: usize, first: usize, ordinals: &[u32], number: usize, kind: RecordKind) -> Option<usize> {
    let u = plan.units.get(unit)?;
    let at = number.checked_sub(plan.blocks.get(u.block)?.first_number)?;
    if !u.own.contains(&at) {
        return None;
    }
    let k = first + *ordinals.get(at.checked_sub(u.stitches.start)?)? as usize;
    match kind {
        RecordKind::Trim | RecordKind::Stop => k.checked_sub(1),
        RecordKind::Stitch | RecordKind::Jump => Some(k),
    }
}

/// The needle points unit `unit`'s records before its own are edited by, as `embroidery.stitches`
/// names them, for a unit numbered from needle point `first` ([`numbering`]): an opened design's
/// run, which the cut and the jump onto it the file had come before. The jump lands on the unit's
/// first needle point; a cut or a stop follows the needle point the last needle-down record before
/// it in its block is, where that record is numbered (a needle-down record that isn't ends it) →
/// (record number, the path and its needle point), in order, for those that have one. Walks back to
/// the needle-down record before the unit's records once: a cut follows its stitches at once.
pub fn leading(plan: &DesignPlan, doc: &Document, unit: usize, first: usize) -> Vec<(usize, (NodeId, usize))> {
    let Some(u) = plan.units.get(unit) else { return Vec::new() };
    let Some(block) = plan.blocks.get(u.block) else { return Vec::new() };
    if u.stitches.start >= u.own.start {
        return Vec::new();
    }
    let mut last = block.stitches.get(..u.stitches.start).and_then(|before| {
        let i = before.iter().rposition(|s| s.command == Command::Stitch)?;
        needle_of(plan, doc, block.first_number + i)
    });
    let mut out = Vec::new();
    for at in u.stitches.start..u.own.start {
        let Some(s) = block.stitches.get(at) else { break };
        let number = block.first_number + at;
        match s.command {
            Command::Jump => out.push((number, (u.origin.node, first))),
            Command::Trim | Command::Stop => out.extend(last.map(|l| (number, l))),
            // A needle-down record that isn't numbered: nothing after it follows a needle point.
            Command::Stitch => last = None,
            Command::ColorChange => {}
        }
    }
    out
}

/// The needle point needle-down record `number` is, where its unit numbers its path's needle
/// points: the path and the needle point.
fn needle_of(plan: &DesignPlan, doc: &Document, number: usize) -> Option<(NodeId, usize)> {
    let unit = plan.unit_at(number)?;
    let (node, first) = numbering(plan, doc, unit)?;
    let k = edited_by(plan, unit, first, &ordinals(plan, unit), number, RecordKind::Stitch)?;
    Some((node, k))
}

#[cfg(test)]
mod tests;
