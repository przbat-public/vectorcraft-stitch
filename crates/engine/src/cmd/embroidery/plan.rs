//! A design's stitch plan ([`DesignPlan`]): the document's art as sewing units, the thread blocks
//! they are sewn in, and every stitch numbered in sewing order.
//!
//! A *sewing unit* ([`Unit`]) is what is planned in one go: one fill or one stroke of an object,
//! or a run of literal stitches read from a machine file. It keeps what it sews ([`Origin`]), the
//! parameters it was planned with, its thread, the box its stitches cover, and where its stitches
//! are: a range of its block's, cut into runs where the needle lifts. Units of one thread are
//! joined into one block wherever the stacking allows (`sequence`), so a block lists the units it
//! holds.
//!
//! The plan is the one way from a document to stitches. The export writes it
//! ([`DesignPlan::to_pattern`]), and the other views of a design's stitches are to read the same
//! plan (a preview on the canvas, stitch counts, the sewing order, design checks, manual
//! stitches), so that each shows what will be sewn. `docs/embroidery.md` › Architecture records
//! the decisions that rest on it.

use std::ops::Range;

use vectorcraft_doc::NodeId;
use vectorcraft_geom::Rect;
use vectorcraft_stitch::plan::Params;
use vectorcraft_stitch::{Block, Pattern, Stitch, Thread};

/// The number of a design's first stitch: stitches are numbered from 1, as a stitch list shows
/// them.
pub const FIRST_NUMBER: usize = 1;

/// The part of an object a sewing unit sews.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// One of its fills: which one, counting the object's fills in appearance order from 0 (hidden
    /// ones too, so that showing or hiding one renumbers none).
    Fill(usize),
    /// One of its strokes, counted as [`Part::Fill`] counts fills.
    Stroke(usize),
    /// Its points, sewn as they are: a run of stitches read from a machine file.
    Literal,
}

/// What a sewing unit sews: an object, and which part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    /// The object whose art is sewn; inside a symbol, the object in the symbol's art.
    pub node: NodeId,
    pub part: Part,
    /// The symbol instance that places the art in the document (the outermost, where a symbol's
    /// art places another symbol): the object a person selects. `None` outside symbols. Two units
    /// share an origin only where a symbol places another symbol more than once.
    pub instance: Option<NodeId>,
}

/// One sewing unit, and where its stitches are in the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub origin: Origin,
    /// The thread it is sewn in: its block's.
    pub thread: Thread,
    /// The parameters its shape was planned with (their style says how it is sewn); `None` for a
    /// literal run, whose points are sewn as they are.
    pub params: Option<Params>,
    /// The parameters a fill's narrow parts were sewn with as satin columns; `None` when it had
    /// none.
    pub columns: Option<Params>,
    /// The box its stitches fall in as planned, millimetres (`None`: no finite point).
    pub bounds: Option<Rect>,
    /// The block it is sewn in: an index into [`DesignPlan::blocks`].
    pub block: usize,
    /// Its stitches, a range of its block's in needle order: the move that reaches it (a jump,
    /// after a cut when the move is long or the object before asked for one), its own stitches,
    /// and the cut after them when it, or a group it ends, asks for one. A block's units' ranges
    /// follow one another without a gap from its first stitch to its last; a unit that starts
    /// where the needle already is leaves that needle point to the one before.
    pub stitches: Range<usize>,
    /// Its runs: each stretch of needle-down stitches one after another, sewn without lifting the
    /// needle, as a range of its block's stitches (within [`Unit::stitches`]).
    pub runs: Vec<Range<usize>>,
}

/// One thread's stitches as the machine sews them, and the units they come from.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadBlock {
    pub thread: Thread,
    /// Its stitches in needle order.
    pub stitches: Vec<Stitch>,
    /// The units sewn in it, in sewing order: a range of [`DesignPlan::units`].
    pub units: Range<usize>,
    /// The number of its first stitch in the design ([`DesignPlan::number`]).
    pub first_number: usize,
}

/// A design's stitch plan: its sewing units and thread blocks in sewing order, and what was left
/// out or approximated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignPlan {
    /// The design's name (the document's title), which machine files carry as a label.
    pub name: String,
    /// The sewing units in sewing order: block by block, each block's in the order they are sewn.
    pub units: Vec<Unit>,
    /// The thread blocks in sewing order.
    pub blocks: Vec<ThreadBlock>,
    /// What was left out or approximated, once each (type, images, gradients…): the export's
    /// warnings.
    pub warnings: Vec<String>,
}

impl DesignPlan {
    /// How many stitches the design has: every needle point, jump and cut of every block (a thread
    /// change between two blocks is not a stitch of its own).
    pub fn len(&self) -> usize {
        self.blocks.iter().map(|b| b.stitches.len()).sum()
    }

    /// Whether the design has no stitch at all.
    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.stitches.is_empty())
    }

    /// The number of stitch `index` of block `block` in the whole design: stitches are numbered
    /// from [`FIRST_NUMBER`] in sewing order, block after block, every needle point, jump and cut
    /// taking one. `None` when there is no such stitch.
    pub fn number(&self, block: usize, index: usize) -> Option<usize> {
        let b = self.blocks.get(block)?;
        (index < b.stitches.len()).then(|| b.first_number + index)
    }

    /// The block, and the index in it, of stitch `number` ([`DesignPlan::number`]).
    pub fn locate(&self, number: usize) -> Option<(usize, usize)> {
        // The last block numbered from `number` or before.
        let k = self.blocks.partition_point(|b| b.first_number <= number).checked_sub(1)?;
        let b = self.blocks.get(k)?;
        let index = number.checked_sub(b.first_number)?;
        (index < b.stitches.len()).then_some((k, index))
    }

    /// The unit that sews stitch `number` ([`DesignPlan::number`]): an index into
    /// [`DesignPlan::units`].
    pub fn unit_at(&self, number: usize) -> Option<usize> {
        let (k, index) = self.locate(number)?;
        let b = self.blocks.get(k)?;
        let units = self.units.get(b.units.clone())?;
        let i = units.partition_point(|u| u.stitches.end <= index);
        units.get(i).filter(|u| u.stitches.contains(&index)).map(|_| b.units.start + i)
    }

    /// The numbers of unit `unit`'s stitches ([`Unit::stitches`], numbered as
    /// [`DesignPlan::number`] numbers them).
    pub fn numbers(&self, unit: usize) -> Option<Range<usize>> {
        let u = self.units.get(unit)?;
        let first = self.blocks.get(u.block)?.first_number;
        Some(first + u.stitches.start..first + u.stitches.end)
    }

    /// The design as the machine formats write it: its blocks in sewing order, under its name.
    pub fn to_pattern(&self) -> Pattern {
        let blocks = self.blocks.iter().map(|b| Block { thread: b.thread.clone(), stitches: b.stitches.clone() }).collect();
        Pattern { name: self.name.clone(), blocks }
    }
}

/// The stretches of `range` of `stitches` sewn without lifting the needle: needle-down stitches
/// one after another.
pub(super) fn runs(stitches: &[Stitch], range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in range.clone() {
        let down = stitches.get(i).is_some_and(|s| s.command.needle_down());
        match (down, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(s..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(s..range.end);
    }
    out
}
