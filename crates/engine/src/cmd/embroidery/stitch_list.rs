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

use super::check::{LimitsKey, StitchLimits};
use super::plan::DesignPlan;

pub use super::records::RecordKind;

/// A record's kind in its flags byte's low bits ([`KIND_BITS`]), and back.
fn kind_bits(kind: RecordKind) -> u8 {
    match kind {
        RecordKind::Stitch => 0,
        RecordKind::Jump => 1,
        RecordKind::Trim => 2,
        RecordKind::Stop => 3,
    }
}

fn kind_of_bits(bits: u8) -> RecordKind {
    match bits & KIND_BITS {
        1 => RecordKind::Jump,
        2 => RecordKind::Trim,
        3 => RecordKind::Stop,
        _ => RecordKind::Stitch,
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
/// was built with; and each filter's count of the records before every [`COUNT_CHUNK`]th one, so
/// that a scope's counts take a few chunks and a filter's rows skip the chunks it matches nothing
/// in (the interface thread asks for both).
#[derive(Clone, Debug, PartialEq)]
pub struct Flags {
    bits: Vec<u8>,
    limits: LimitsKey,
    counted: Vec<[u32; Filter::ALL.len()]>,
}

/// How many records a chunk of [`Flags`]' counts covers: a scope's counts walk at most two chunks'
/// records at its ends; 2 million records keep 489 chunks. An estimate.
pub const COUNT_CHUNK: usize = 4096;

/// Each filter's count of the records before every [`COUNT_CHUNK`]th one of `bits`, and of them
/// all at the end: one walk, with the flags, on the planning thread.
fn counted(bits: &[u8]) -> Vec<[u32; Filter::ALL.len()]> {
    let mut out = Vec::with_capacity(bits.len() / COUNT_CHUNK + 2);
    let mut sum = [0u32; Filter::ALL.len()];
    for (slot, b) in bits.iter().enumerate() {
        if slot % COUNT_CHUNK == 0 {
            out.push(sum);
        }
        for (s, f) in sum.iter_mut().zip(Filter::ALL) {
            *s = s.saturating_add(u32::from(bit_matches(*b, f)));
        }
    }
    out.push(sum);
    out
}

/// Whether a record of flag byte `b` is listed under `filter`.
fn bit_matches(b: u8, filter: Filter) -> bool {
    match filter {
        Filter::All => true,
        Filter::Jumps => kind_of_bits(b) == RecordKind::Jump,
        Filter::Trims => kind_of_bits(b) == RecordKind::Trim,
        Filter::Stops => kind_of_bits(b) == RecordKind::Stop,
        Filter::Short => b & SHORT_BIT != 0,
        Filter::Long => b & LONG_BIT != 0,
    }
}

/// How a record's stitch stands to the limits ([`Flags::judged`]): too short, too long.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Judgement {
    pub short: bool,
    pub long: bool,
}

impl Flags {
    /// The flags of `plan`'s records judged against `limits`: one walk, stopped early (and then
    /// incomplete) when the work is cancelled.
    pub fn of(plan: &DesignPlan, limits: &StitchLimits) -> Self {
        let mut bits = Vec::with_capacity(plan.blocks.iter().map(|b| b.stitches.len()).sum());
        super::measure::each_until_cancelled(plan, |r| {
            let mut b = kind_bits(RecordKind::of(r.stitch.command));
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
        let counted = counted(&bits);
        Self { bits, limits: limits.key(), counted }
    }

    /// How many of records `range` (numbers, from 1) each filter lists, in [`Filter::ALL`]'s order:
    /// whole chunks by their counts, the records at the ends one by one.
    pub fn counts(&self, range: Range<usize>) -> [usize; Filter::ALL.len()] {
        let (start, end) = (range.start.max(1), range.end.min(self.bits.len() + 1));
        let mut out = [0usize; Filter::ALL.len()];
        if start >= end {
            return out;
        }
        // Slots (number - 1): the whole chunks between the ends, from the counts before them.
        let (a, b) = (start - 1, end - 1);
        let (first, last) = (a.div_ceil(COUNT_CHUNK), b / COUNT_CHUNK);
        let mut add = |slots: Range<usize>| {
            for slot in slots {
                for (o, f) in out.iter_mut().zip(Filter::ALL) {
                    *o += usize::from(self.matches(slot + 1, f));
                }
            }
        };
        if first >= last {
            add(a..b);
            return out;
        }
        add(a..first * COUNT_CHUNK);
        add(last * COUNT_CHUNK..b);
        if let (Some(from), Some(to)) = (self.counted.get(first), self.counted.get(last)) {
            for ((o, x), y) in out.iter_mut().zip(from).zip(to) {
                *o += (y - x) as usize;
            }
        }
        out
    }

    /// Each record of `range` (numbers, from 1) `filter` lists, in order, skipping the chunks it
    /// lists none of.
    pub fn each_listed(&self, range: Range<usize>, filter: Filter, mut f: impl FnMut(usize)) {
        let (start, end) = (range.start.max(1), range.end.min(self.bits.len() + 1));
        let k = Filter::ALL.iter().position(|x| *x == filter).unwrap_or(0);
        let mut n = start;
        while n < end {
            let chunk = (n - 1) / COUNT_CHUNK;
            let chunk_end = ((chunk + 1) * COUNT_CHUNK + 1).min(end);
            let none = match (self.counted.get(chunk), self.counted.get(chunk + 1)) {
                (Some(x), Some(y)) => x.get(k) == y.get(k),
                _ => false,
            };
            if !none {
                for m in n..chunk_end {
                    if self.matches(m, filter) {
                        f(m);
                    }
                }
            }
            n = chunk_end;
        }
    }

    /// What the limits it was built with are kept for.
    pub fn limits(&self) -> LimitsKey {
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
        self.bits.get(number.checked_sub(1)?).map(|b| kind_of_bits(*b))
    }

    /// Whether record `number` is listed under `filter`.
    pub fn matches(&self, number: usize, filter: Filter) -> bool {
        number.checked_sub(1).and_then(|i| self.bits.get(i)).is_some_and(|b| bit_matches(*b, filter))
    }

    /// Whether record `number`'s stitch breaks the limits.
    pub fn judged(&self, number: usize) -> Judgement {
        let b = number.checked_sub(1).and_then(|i| self.bits.get(i)).copied().unwrap_or(0);
        Judgement { short: b & SHORT_BIT != 0, long: b & LONG_BIT != 0 }
    }

    /// About how many bytes it holds.
    pub fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.bits.capacity() + self.counted.capacity() * std::mem::size_of::<[u32; Filter::ALL.len()]>()
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
    let length = super::measure::Record { number, unit, stitch, before, same_block: index > 0 }.length();
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

/// The path unit `unit` sews where its records number the path's needle points, and the needle
/// point its first own needle-down record is: `embroidery.stitches`' rule
/// ([`super::records::numbering`]).
pub use super::records::numbering;

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
    kind.edited_needle(first, *ordinals.get(at.checked_sub(u.stitches.start)?)? as usize)
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
        // A needle-down record that isn't numbered: nothing after it follows a needle point.
        if s.command == Command::Stitch {
            last = None;
        } else if let Some(at) = super::records::leading_needle(s.command, (u.origin.node, first), last) {
            out.push((number, at));
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

/// The rows a scope and a filter list (the Stitches tab's): record numbers in sewing order, as ranges
/// (every record of them) or picked one by one; with each filter's count in the scope.
pub struct Listed {
    rows: Rows,
    pub counts: [usize; Filter::ALL.len()],
    /// The scope's first and last record numbers.
    pub span: Option<(usize, usize)>,
}

impl Default for Listed {
    /// No rows: before any plan's flags are built.
    fn default() -> Self {
        Self { rows: Rows::Picked(Vec::new()), counts: [0; Filter::ALL.len()], span: None }
    }
}

enum Rows {
    /// Every record of the ranges, and how many come before each range.
    Ranges(Vec<Range<usize>>, Vec<usize>),
    Picked(Vec<u32>),
}

impl Listed {
    /// How many rows.
    pub fn len(&self) -> usize {
        match &self.rows {
            Rows::Ranges(r, before) => before.last().copied().unwrap_or(0) + r.last().map_or(0, |l| l.len()),
            Rows::Picked(p) => p.len(),
        }
    }

    /// Whether it lists no row.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Row `i`'s record number.
    pub fn record(&self, i: usize) -> Option<usize> {
        match &self.rows {
            Rows::Ranges(ranges, before) => {
                let k = before.partition_point(|b| *b <= i).checked_sub(1)?;
                let r = ranges.get(k)?;
                let n = r.start + (i - before.get(k)?);
                (n < r.end).then_some(n)
            }
            Rows::Picked(p) => p.get(i).map(|n| *n as usize),
        }
    }

    /// The row of record `number`, when it is listed.
    pub fn row_of(&self, number: usize) -> Option<usize> {
        match &self.rows {
            Rows::Ranges(ranges, before) => {
                let k = ranges.partition_point(|r| r.end <= number);
                let r = ranges.get(k).filter(|r| r.contains(&number))?;
                Some(before.get(k)? + (number - r.start))
            }
            Rows::Picked(p) => {
                let n = u32::try_from(number).ok()?;
                p.binary_search(&n).ok()
            }
        }
    }
}

/// The rows of `ranges` (record numbers; `None`: every record of `flags`) under `filter`, and every
/// filter's count, in one pass over the scope's records.
pub fn listed(flags: &Flags, ranges: Option<&[Range<usize>]>, filter: Filter) -> Listed {
    let whole = 1..flags.len() + 1;
    let ranges: &[Range<usize>] = ranges.unwrap_or(std::slice::from_ref(&whole));
    // Each filter's count from the flags' counts by chunk (built on the planning thread), and the
    // filter's rows from the chunks it lists any of: no walk of every record here.
    let mut counts = [0usize; Filter::ALL.len()];
    let mut picked: Vec<u32> = Vec::new();
    for r in ranges {
        for (slot, n) in counts.iter_mut().zip(flags.counts(r.clone())) {
            *slot += n;
        }
        if filter != Filter::All {
            flags.each_listed(r.clone(), filter, |n| {
                if let Ok(n) = u32::try_from(n) {
                    picked.push(n);
                }
            });
        }
    }
    let span = ranges.first().zip(ranges.last()).and_then(|(a, b)| (b.end > a.start).then(|| (a.start, b.end - 1)));
    let rows = if filter == Filter::All {
        let mut before = Vec::with_capacity(ranges.len());
        let mut sum = 0;
        for r in ranges {
            before.push(sum);
            sum += r.len();
        }
        Rows::Ranges(ranges.to_vec(), before)
    } else {
        Rows::Picked(picked)
    };
    Listed { rows, counts, span }
}

#[cfg(test)]
mod tests;
