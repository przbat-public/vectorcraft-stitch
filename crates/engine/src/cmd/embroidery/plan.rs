//! A design's stitch plan ([`DesignPlan`]): the document's art as sewing units, the thread blocks
//! they are sewn in, and every stitch numbered in sewing order.
//!
//! A *sewing unit* ([`Unit`]) is what is planned in one go: one fill or one stroke of an object,
//! or a run of literal stitches read from a machine file. It keeps what it sews ([`Origin`]), the
//! parameters it was planned with, its thread, the box its stitches cover, and where its stitches
//! are: a range of its block's, cut into runs where the needle lifts. Units of one thread are
//! joined into one block wherever the stacking allows (`sequence`), so a block lists the units it
//! holds. Each block is sewn in a thread of the document's palette, matched to the colour its art
//! is painted with ([`BlockThread`]), and the PES and JEF writers write that thread.
//!
//! The plan is the one way from a document to stitches. The export writes it
//! ([`DesignPlan::to_pattern`]), and the other views of a design's stitches are to read the same
//! plan (a preview on the canvas, stitch counts, the sewing order, design checks, manual
//! stitches), so that each shows what will be sewn. `docs/embroidery.md` › Architecture records
//! the decisions that rest on it.

use std::collections::BTreeMap;
use std::ops::Range;

use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_geom::{Point, Rect};
use vectorcraft_stitch::plan::{self, Params, Style};
use vectorcraft_stitch::thread::{Match, Palette, PaletteThread};
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread, ThreadRecord};

/// The number of a design's first record (needle point, jump or cut): records are numbered from 1,
/// as a stitch list shows them. The commands call these record numbers (`firstRecord`, `record`),
/// apart from `stitches`, which counts needle points only.
pub const FIRST_NUMBER: usize = 1;

/// The part of an object a sewing unit sews.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// One of its fills: which one, counting the object's fills in appearance order from 0 (hidden
    /// ones too, so that showing or hiding one renumbers none). Adding or removing a fill renumbers
    /// the ones after it, so the number tells one plan's units apart and is no key for anything
    /// stored with the object.
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
    /// art places another symbol), or the object an expanded appearance's pieces come from: the
    /// object a person selects. `None` otherwise. Units that share an origin, or its selected object
    /// and part, are told apart by [`Unit::occurrence`].
    pub instance: Option<NodeId>,
}

impl Origin {
    /// The object a person selects for the unit: the symbol instance that places it (or the object
    /// whose expanded appearance it is a piece of), else the object itself. A selection maps to
    /// units through this id, or an object that holds it (its group, its layer), never through
    /// [`Origin::node`], which a symbol's instances share and an expanded object's pieces get anew
    /// at every plan.
    pub fn selected(&self) -> NodeId {
        self.instance.unwrap_or(self.node)
    }
}

/// What tells a sewing unit apart, within a plan and from one plan to the next: the object a
/// person selects ([`Origin::selected`]), the part it sews, and which of that object's units of
/// that part it is ([`Unit::occurrence`]). It holds while the object's own art (a symbol's paths,
/// the pieces its appearance expands to) keeps its number and order. A unit's index, its block and
/// its stitch numbers are positions in one plan only, and [`Origin::node`] is no key: a symbol's
/// instances share it, and an expanded object's pieces get it anew at every plan. What the
/// interface keeps between plans (a row of the sewing order, a unit just moved) is kept by this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnitKey {
    pub object: NodeId,
    pub part: Part,
    pub occurrence: usize,
}

impl Part {
    /// The kind of part it is of an object: its fill or its stroke (`None`: stitches read from a
    /// machine file).
    pub fn kind(self) -> Option<PartKind> {
        match self {
            Part::Fill(_) => Some(PartKind::Fill),
            Part::Stroke(_) => Some(PartKind::Stroke),
            Part::Literal => None,
        }
    }
}

/// Which setting forced a unit's style ([`Choice::Forced`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForcedBy {
    /// The object's own `stitch` (`embroidery.set`).
    Object,
    /// The `stitch` option given to the call (an export's, a read command's).
    Export,
}

impl ForcedBy {
    /// Its id in replies: the layer it is (`embroidery.info`'s sources).
    pub fn id(self) -> &'static str {
        match self {
            ForcedBy::Object => "object",
            ForcedBy::Export => "export",
        }
    }
}

/// Why a sewing unit is sewn the way it is: the rule that chose its stitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A fill: tatami, its parts narrower than `narrowAsSatin` sewn as satin columns.
    Fill,
    /// A stroke at least as wide as a satin column needs (`digitize::SATIN_MIN_WIDTH`): a satin
    /// column across its width.
    WideStroke,
    /// A narrower stroke: a running line along it.
    NarrowStroke,
    /// `stitch: fill` on a stroke: the area the stroke paints, filled.
    StrokeFilled,
    /// A `stitch` setting forced the style: the object's own, or the option given to the call.
    Forced(ForcedBy),
    /// A stroke that paints an area its path doesn't run down the middle of (dashed, aligned,
    /// of varying width, with arrowheads): that area, as on the canvas, filled.
    StrokeOutlined,
    /// `stitch: satin` on a fill (the object's own, or the call's): a satin needs a width a fill
    /// doesn't have, so the fill is filled as a fill is without the option (its narrow parts as
    /// satin columns).
    SatinFilled(ForcedBy),
    /// Stitches read from a machine file, sewn as they are.
    Literal,
}

pub use vectorcraft_stitch::thread::DELTA_E_HUNDREDTHS;

/// The thread a block is sewn in: a thread of the document's palette, matched to the colour the
/// block's art is painted with ([`Palette::assign`], in sewing order).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct BlockThread {
    /// The colour the block's art is painted with (sRGB): its units' fill or stroke, a literal
    /// run's stroke.
    pub art: [u8; 3],
    /// The palette thread: one of the palette's threads by construction, of the document's
    /// palette (`embroidery.setup`, Janome's JEF when it sets none).
    pub thread: PaletteThread,
    /// The CIEDE2000 colour difference between the art's colour and the thread's
    /// (`vectorcraft_stitch::color`): 0 when the art is painted with the thread's own colour, a
    /// swatch of the palette's thread library.
    pub delta_e: f64,
    /// The thread record the machine file the block was read from gave (a VP3's: catalogue
    /// number, description, brand, material and weight), which a format that has one writes back.
    /// `None` for art, and for a file that records none.
    pub record: Option<ThreadRecord>,
}

impl BlockThread {
    /// `palette`'s thread `matched` to art colour `art`.
    pub(super) fn new(art: [u8; 3], palette: Palette, matched: Match) -> Self {
        Self { art, thread: PaletteThread::matched(palette, matched), delta_e: matched.delta_e, record: None }
    }

    /// The palette the thread is from.
    pub fn palette(&self) -> Palette {
        self.thread.palette()
    }

    /// The thread's number in the palette, from 1 as the formats count.
    pub fn index(&self) -> usize {
        self.thread.index()
    }

    /// Its name as the palette lists it.
    pub fn name(&self) -> &'static str {
        self.thread.entry().1
    }

    /// Its colour as the palette lists it (sRGB).
    pub fn color(&self) -> [u8; 3] {
        self.thread.entry().0
    }

    /// The colour difference as a list of threads shows it (`embroidery.threads`): to
    /// [`DELTA_E_HUNDREDTHS`].
    pub fn shown_delta_e(&self) -> f64 {
        vectorcraft_stitch::thread::shown_delta_e(self.delta_e)
    }

    /// Whether the thread looks like another colour than the art: its difference as shown is past
    /// `vectorcraft_stitch::thread::MISMATCH_DELTA_E`; the same line, read the same way, decides
    /// when a block takes another thread than a neighbour's. As shown (to two decimals), so that a
    /// list never flags a thread whose difference it shows as the threshold itself.
    pub fn mismatch(&self) -> bool {
        vectorcraft_stitch::thread::mismatched(self.delta_e)
    }

    /// The thread as the machine formats take it ([`DesignPlan::to_pattern`]): a format that
    /// numbers its threads from this palette writes this one, and any other matches the art's
    /// colour to its own palette.
    pub fn to_thread(&self) -> Thread {
        Thread { color: self.art, name: self.name().to_string(), chosen: Some(self.thread), record: self.record.clone() }
    }
}

impl DesignPlan {
    /// Its blocks' threads as a format of `palette` writes them: the plan's own when they are
    /// that palette's; else each block's art matched to `palette` in sewing order
    /// ([`Palette::assign`]), as the writer of such a format matches them ([`Palette::numbers`]).
    pub fn threads_in(&self, palette: Palette) -> Vec<BlockThread> {
        if self.blocks.iter().all(|b| b.thread.palette() == palette) {
            return self.blocks.iter().map(|b| b.thread.clone()).collect();
        }
        let blocks: Vec<([u8; 3], Option<PaletteThread>)> = self.blocks.iter().map(|b| (b.thread.art, Some(b.thread.thread))).collect();
        self.blocks
            .iter()
            .zip(palette.assign(&blocks))
            .map(|(b, m)| BlockThread { record: b.thread.record.clone(), ..BlockThread::new(b.thread.art, palette, m) })
            .collect()
    }
}

/// One sewing unit, and where its stitches are in the plan.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Unit {
    pub origin: Origin,
    /// Which of the plan's units of the same selected object and part ([`Origin::selected`],
    /// [`Origin::part`]) it is, from 1 in walk order: the art a symbol instance places, and an
    /// expanded object's pieces, share their object and often their part. With them it tells a
    /// unit apart within the plan, and from one plan to the next as long as the object's own art
    /// (its symbol's paths, the pieces its appearance expands to) keeps its number and order.
    /// [`Origin::node`] is no such key: an expanded object's pieces get new ids at every plan, and
    /// a symbol's paths are the same nodes in every instance.
    pub occurrence: usize,
    /// The thread it is sewn in: its block's.
    pub thread: BlockThread,
    /// The parameters its shape was planned with (their style says how it is sewn); `None` for a
    /// literal run, whose points are sewn as they are.
    pub params: Option<Params>,
    /// The parameters a fill's narrow parts were sewn with as satin columns; `None` when it had
    /// none.
    pub columns: Option<Params>,
    /// The narrowest and the widest satin column it sews, millimetres: a satin's width (both), the
    /// typical widths of a fill's narrowest and widest narrow parts; `None` when it sews none.
    pub satin_widths: Option<[f64; 2]>,
    /// Why it is sewn the way it is.
    pub why: Choice,
    /// The box its stitches fall in as planned, millimetres (`None`: no finite point).
    pub bounds: Option<Rect>,
    /// The block it is sewn in: an index into [`DesignPlan::blocks`] (whose [`ThreadBlock::units`]
    /// lists it).
    pub block: usize,
    /// Its stitches, a range of its block's in needle order: the move that reaches it (a jump,
    /// after a cut when the move is longer than the trim length, or where the file a literal run
    /// comes from had one), its own stitches, and the cut after them when it, or a group it ends,
    /// asks for one. A block's units' ranges follow one another without a gap from its first
    /// stitch to its last. A unit that starts where the needle already is leaves that needle point
    /// to the one before, so a unit of that one needle point alone has an empty range.
    pub stitches: Range<usize>,
    /// Its runs: each stretch of needle-down stitches one after another, sewn without lifting the
    /// needle, as a range of its block's stitches (within [`Unit::stitches`]).
    pub runs: Vec<Range<usize>>,
    /// The entry and exit points it was planned toward: its part's, when its object sets them.
    pub asked: Asked,
}

/// The entry and exit points a unit was planned toward (millimetres from the document's origin),
/// each `None` when its object sets none: how far its stitches miss them is
/// [`DesignPlan::missed`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Asked {
    pub entry: Option<Point>,
    pub exit: Option<Point>,
    /// How far from each its first and last needle points can lie at best, millimetres, as its
    /// planning found them: a satin column's half width and pull compensation (a fill's narrow part
    /// where the needle point lies in it, at its width there), an open column's push compensation.
    pub reach: [f64; 2],
}

impl Unit {
    /// What tells it apart from the plan's other units, in this plan and the next.
    pub fn key(&self) -> UnitKey {
        UnitKey { object: self.origin.selected(), part: self.origin.part, occurrence: self.occurrence }
    }

    /// The style it is sewn in, as the planner sews its parameters ([`plan::sewn_style`]: a satin
    /// without a width, a fill whose `stitch` is satin, is filled); `None` for a literal run,
    /// sewn as its points are.
    pub fn sewn(&self) -> Option<Style> {
        self.params.map(|p| plan::sewn_style(&p))
    }
}

/// One thread's stitches as the machine sews them, and the units they come from.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ThreadBlock {
    pub thread: BlockThread,
    /// Its stitches in needle order.
    pub stitches: Vec<Stitch>,
    /// The units sewn in it, in sewing order: a range of [`DesignPlan::units`].
    pub units: Range<usize>,
    /// The number of its first stitch in the design ([`DesignPlan::number`]).
    pub first_number: usize,
}

impl ThreadBlock {
    /// The number of its last stitch in the design (its first's, for a block of none).
    pub fn last_number(&self) -> usize {
        self.first_number + self.stitches.len().saturating_sub(1)
    }

    /// How many of its stitches are needle points (needle-down).
    pub fn needle_points(&self) -> usize {
        self.stitches.iter().filter(|s| s.command.needle_down()).count()
    }
}

/// The most objects a plan names for one warning ([`DesignPlan::warned`]): as many as a check lists
/// for a rule, whatever the design.
pub const MAX_WARNED_OBJECTS: usize = 1000;

/// A design's stitch plan: its sewing units and thread blocks in sewing order, and what was left
/// out or approximated.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct DesignPlan {
    /// The design's name (the document's title), which machine files carry as a label.
    pub name: String,
    /// The sewing units in sewing order: block by block, each block's in the order they are sewn.
    pub units: Vec<Unit>,
    /// The thread blocks in sewing order.
    pub blocks: Vec<ThreadBlock>,
    /// What was left out or approximated, once each (type, images, gradients…): the art the
    /// `notSewn` check counts.
    pub warnings: Vec<String>,
    /// The objects each warning is about, by its words: those a person selects (a symbol's
    /// instance for art inside the symbol), in the order the walk met them, at most
    /// [`MAX_WARNED_OBJECTS`] each. A warning about no object in particular (what the document's
    /// appearances leave out) has none.
    pub warned: BTreeMap<String, Vec<NodeId>>,
    /// What else the export says that isn't about the art (a thread palette this version doesn't
    /// know): said with the warnings ([`DesignPlan::messages`]), not counted as art left out.
    pub notes: Vec<String>,
    /// Where the machine starts the needle (millimetres from the document's origin, y down): the
    /// document's start point ([`vectorcraft_doc::embroidery::EmbroiderySetup::start`]), `None`
    /// when it sets none.
    pub start: Option<Point>,
}

impl DesignPlan {
    /// How many stitches the design has: every needle point, jump and cut of every block (a thread
    /// change between two blocks is not a stitch of its own).
    pub fn len(&self) -> usize {
        self.blocks.iter().map(|b| b.stitches.len()).sum()
    }

    /// About how many bytes it holds: its units (their runs included) and its blocks' stitches, as
    /// allocated (the planning service bounds what it remembers by this).
    pub fn byte_size(&self) -> usize {
        let units: usize = self.units.iter().map(|u| std::mem::size_of::<Unit>() + u.runs.capacity() * std::mem::size_of::<Range<usize>>()).sum();
        let blocks: usize =
            self.blocks.iter().map(|b| std::mem::size_of::<ThreadBlock>() + b.stitches.capacity() * std::mem::size_of::<Stitch>()).sum();
        let words: usize = self.warnings.iter().chain(&self.notes).map(String::capacity).sum();
        let warned: usize = self.warned.iter().map(|(w, ids)| w.capacity() + ids.capacity() * std::mem::size_of::<NodeId>()).sum();
        let words = words + warned;
        std::mem::size_of::<Self>() + self.units.capacity().saturating_sub(self.units.len()) * std::mem::size_of::<Unit>() + units + blocks + words
    }

    /// Everything the export and the read commands say about the plan: its notes, then its
    /// warnings.
    pub fn messages(&self) -> Vec<String> {
        self.notes.iter().chain(&self.warnings).cloned().collect()
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

    /// The unit whose key is `key` ([`Unit::key`]): an index into [`DesignPlan::units`].
    pub fn find(&self, key: &UnitKey) -> Option<usize> {
        self.units.iter().position(|u| u.key() == *key)
    }

    /// Unit `unit`'s stitches ([`Unit::stitches`]): empty for a unit that isn't there.
    pub fn unit_stitches(&self, unit: usize) -> &[Stitch] {
        self.units.get(unit).and_then(|u| self.blocks.get(u.block)?.stitches.get(u.stitches.clone())).unwrap_or_default()
    }

    /// Unit `unit`'s first and last needle points (millimetres), `None` when it has none of its
    /// own. A unit that starts exactly where the needle already is leaves that needle point to the
    /// unit before ([`Unit::stitches`]): its range then starts with a needle point (its tie-in's
    /// step) rather than the move that reaches it, and it starts at the needle point before its
    /// range, not at the first of its own.
    pub fn needle_ends(&self, unit: usize) -> Option<(Point, Point)> {
        let u = self.units.get(unit)?;
        let block = self.blocks.get(u.block)?;
        let own = block.stitches.get(u.stitches.clone())?;
        let mut needle = own.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point());
        let first = needle.next()?;
        let last = needle.next_back().unwrap_or(first);
        let met = own.first().is_some_and(|s| s.command == Command::Stitch);
        let before = u.stitches.start.checked_sub(1).and_then(|k| block.stitches.get(k)).filter(|s| met && s.command == Command::Stitch);
        Some((before.map_or(first, |s| s.point()), last))
    }

    /// How far unit `unit`'s first and last needle points ([`DesignPlan::needle_ends`]) lie from the
    /// entry and the exit point it was planned toward ([`Unit::asked`]), beyond the reach of its
    /// satin columns there ([`Asked::reach`]), millimetres: each `None` when it asked for none, or
    /// has no needle point of its own.
    pub fn missed(&self, unit: usize) -> [Option<f64>; 2] {
        let Some(u) = self.units.get(unit) else { return [None, None] };
        let ends = self.needle_ends(unit);
        let (first, last) = (ends.map(|e| e.0), ends.map(|e| e.1));
        let miss = |a: Point, q: Point, reach: f64| (a.distance(q) - reach).max(0.0);
        let [entry_reach, exit_reach] = u.asked.reach;
        [u.asked.entry.zip(first).map(|(a, q)| miss(a, q, entry_reach)), u.asked.exit.zip(last).map(|(a, q)| miss(a, q, exit_reach))]
    }

    /// The numbers of unit `unit`'s stitches ([`Unit::stitches`], numbered as
    /// [`DesignPlan::number`] numbers them).
    pub fn numbers(&self, unit: usize) -> Option<Range<usize>> {
        let u = self.units.get(unit)?;
        let first = self.blocks.get(u.block)?.first_number;
        Some(first + u.stitches.start..first + u.stitches.end)
    }

    /// The design as the machine formats write it: its blocks in sewing order, each with its
    /// thread ([`BlockThread::to_thread`]), under its name.
    pub fn to_pattern(&self) -> Pattern {
        let blocks = self.blocks.iter().map(|b| Block { thread: b.thread.to_thread(), stitches: b.stitches.clone() }).collect();
        Pattern { name: self.name.clone(), blocks, start: self.start }
    }

    /// [`DesignPlan::to_pattern`], the plan's stitches moved into it rather than copied (the export,
    /// which writes the plan and drops it).
    pub fn into_pattern(self) -> Pattern {
        let blocks = self.blocks.into_iter().map(|b| Block { thread: b.thread.to_thread(), stitches: b.stitches }).collect();
        Pattern { name: self.name, blocks, start: self.start }
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
