//! The order a design's blocks are sewn in: stacking order, with blocks of one thread joined
//! where nothing lies between them, and literal stitches kept as their file had them.
//!
//! Sewing units are added one at a time, in document order, so that each can be planned knowing
//! where the needle will be when it starts ([`Sequence::needle`]): an object starts near the end of
//! the one sewn before it, which saves jumps and trims. Each block keeps the units it holds and
//! where their stitches start, and [`Sequence::plan`] makes the design's plan of them, each block's
//! art colour matched to a thread of the palette.

use vectorcraft_doc::NodeId;
use vectorcraft_geom::{Point, Rect};
use vectorcraft_stitch::plan::Params;
use vectorcraft_stitch::thread::{Palette, PaletteThread};
use vectorcraft_stitch::{Block, Command, SAME_POINT, Stitch};

use super::plan::{BlockThread, Choice, DesignPlan, FIRST_NUMBER, Origin, ThreadBlock, Unit, runs};

/// How far apart two blocks' stitches must be not to touch: a thread's width and a satin's pull.
/// An estimate.
pub(super) const CLEARANCE: f64 = 1.0;

/// What a sewing unit is and how it was planned: the facts the plan's [`Unit`] carries, from the
/// walk that planned it to the plan.
#[derive(Clone, Copy, Debug)]
pub(super) struct Facts {
    /// What it sews.
    pub(super) origin: Origin,
    /// The parameters it was planned with (`None`: a literal run, sewn as its points are), and
    /// those of a fill's narrow parts sewn as satin columns (`None`: it had none).
    pub(super) params: Option<Params>,
    pub(super) columns: Option<Params>,
    /// The narrowest and widest satin columns it sews (`Unit::satin_widths`).
    pub(super) satin_widths: Option<[f64; 2]>,
    /// Why it is sewn the way it is.
    pub(super) why: Choice,
}

/// One sewing unit's stitches as planned, in document order.
pub(super) struct Planned {
    pub(super) facts: Facts,
    pub(super) block: Block,
    /// A literal run read from a machine file: the group it belongs to (that file's colour block)
    /// and whether the thread was cut before it. `None`: stitches planned from art.
    pub(super) literal: Option<(NodeId, bool)>,
}

/// A unit in a block: what it is and was planned with, the box its stitches cover as planned, and
/// where its stitches start in the block (they run up to the next unit's first stitch). It is sewn
/// in its block's thread.
struct Member {
    facts: Facts,
    bounds: Option<Rect>,
    start: usize,
}

/// The thread cut at the end of `stitches`, unless it already ends in a cut.
fn cut_after(stitches: &mut Vec<Stitch>) {
    if let Some(&last) = stitches.last()
        && last.command != Command::Trim
    {
        stitches.push(Stitch::new(last.x, last.y, Command::Trim));
    }
}

/// A block sewn so far: its stitches, the box they cover grown by [`CLEARANCE`], the group of the
/// literal runs it holds, and its units in sewing order.
struct Sewn {
    block: Block,
    bounds: Option<Rect>,
    literal: Option<NodeId>,
    members: Vec<Member>,
}

/// The blocks sewn so far, each with its bounds (grown by [`CLEARANCE`]), the group of the literal
/// runs it holds, and its units.
///
/// Blocks of one thread are sewn in one pass where the stacking allows it: an object joins an
/// earlier block of its thread only when it overlaps none of the blocks sewn between the two, so
/// art that lies on top is still sewn last (thread sewn later covers thread sewn earlier, as paint
/// does). The machine changes colour as few times as that allows, not once per object.
///
/// Literal runs (a design read from a file) are left as the file had them: the runs of one group
/// make one block, joined by the jumps (and trims) between them, and never merge with anything
/// else.
pub(super) struct Sequence {
    blocks: Vec<Sewn>,
    /// The jump and trim lengths between objects.
    connect: Params,
    /// The block each unit added went into, in the order they were added.
    adds: Vec<usize>,
}

impl Sequence {
    pub(super) fn new(connect: Params) -> Self {
        Self { blocks: Vec::new(), connect: connect.sane(), adds: Vec::new() }
    }

    /// The block an object of `color` that covers `bounds` (grown by [`CLEARANCE`]) joins: the
    /// last of its thread, when none sewn after that one overlaps it. `None`: it starts a block of
    /// its own.
    pub(super) fn target(&self, color: [u8; 3], bounds: Option<Rect>) -> Option<usize> {
        let j = self.blocks.iter().rposition(|m| m.literal.is_none() && m.block.thread.color == color)?;
        self.blocks
            .iter()
            .skip(j + 1)
            .all(|other| match (bounds, other.bounds) {
                (Some(a), Some(o)) => !overlaps(a, o),
                _ => false,
            })
            .then_some(j)
    }

    /// Where the needle is when an object that joins `target` (or, `None`, starts a block of its
    /// own after the last) starts: the end of that block's stitches.
    pub(super) fn needle(&self, target: Option<usize>) -> Option<Point> {
        let sewn = match target {
            Some(j) => self.blocks.get(j),
            None => self.blocks.last(),
        }?;
        sewn.block.stitches.iter().rev().find(|s| s.command != Command::Trim).map(|s| s.point())
    }

    /// Add one unit's stitches after the others.
    pub(super) fn add(&mut self, planned: Planned) {
        let Planned { facts, block, literal } = planned;
        let Some(&first) = block.stitches.first() else { return };
        let planned_bounds = block.bounds();
        let member = |start: usize| Member { facts, bounds: planned_bounds, start };
        match literal {
            Some((owner, trim_before)) => self.add_literal(block, first, member, owner, trim_before),
            None => self.add_art(block, first, member),
        }
    }

    /// A literal run's stitches (`b`, starting at `first`): into the block of its group's runs
    /// before it (its thread's), or a block of its own, with the cut before it the file had.
    fn add_literal(&mut self, mut b: Block, first: Stitch, member: impl Fn(usize) -> Member, owner: NodeId, trim_before: bool) {
        let bounds = b.bounds().map(|r| r.inflate(CLEARANCE, CLEARANCE));
        match self.blocks.last_mut() {
            Some(sewn) if sewn.literal == Some(owner) && sewn.block.thread.color == b.thread.color => {
                let start = sewn.block.stitches.len();
                let merged = &mut sewn.block;
                if trim_before {
                    cut_after(&mut merged.stitches);
                }
                merged.stitches.push(Stitch::jump(first.point()));
                merged.stitches.extend(b.stitches.iter().copied());
                sewn.bounds = union(sewn.bounds, bounds);
                sewn.members.push(member(start));
            }
            _ => {
                // A cut at the very start of the block, as the file had it.
                if trim_before {
                    b.stitches.insert(0, Stitch::new(first.x, first.y, Command::Trim));
                }
                let members = vec![member(0)];
                self.blocks.push(Sewn { block: b, bounds, literal: Some(owner), members });
            }
        }
        self.adds.push(self.blocks.len().saturating_sub(1));
    }

    /// Art's stitches (`b`, starting at `first`): into the last block of its colour when nothing
    /// sewn since overlaps it ([`Sequence::target`]), else a block of its own.
    fn add_art(&mut self, b: Block, first: Stitch, member: impl Fn(usize) -> Member) {
        let bounds = b.bounds().map(|r| r.inflate(CLEARANCE, CLEARANCE));
        let target = self.target(b.thread.color, bounds);
        self.adds.push(target.unwrap_or(self.blocks.len()));
        match target.and_then(|j| self.blocks.get_mut(j)) {
            Some(sewn) => {
                let start = sewn.block.stitches.len();
                let merged = &mut sewn.block;
                // To the next object: a jump, with the thread cut first when the move is longer
                // than `trimLength` (each object ties itself off and in), or when the object
                // before asked for it. An object that starts where the needle is goes on from
                // there, its first needle point left out: the needle is already in that hole.
                let mut skip = 0;
                if let Some(last) = merged.stitches.last().copied() {
                    let at = merged.stitches.iter().rev().find(|s| s.command != Command::Trim).copied().unwrap_or(last);
                    if last.command != Command::Trim && at.distance(first) > self.connect.trim {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    if at.distance(first) > SAME_POINT || last.command == Command::Trim {
                        merged.stitches.push(Stitch::jump(first.point()));
                    } else if first.command == Command::Stitch {
                        skip = 1;
                    }
                }
                merged.stitches.extend(b.stitches.iter().skip(skip).copied());
                sewn.bounds = union(sewn.bounds, bounds);
                sewn.members.push(member(start));
            }
            None => {
                let members = vec![member(0)];
                self.blocks.push(Sewn { block: b, bounds, literal: None, members });
            }
        }
    }

    /// How many units with stitches were added so far.
    pub(super) fn added(&self) -> usize {
        self.adds.len()
    }

    /// Cut the thread after the units added since `since` (an [`Sequence::added`] count: those of
    /// an object that asks for it, or of a group that does), however near the next one starts:
    /// after the one sewn last, which ends the latest block any of them went into (a group's
    /// units may join earlier blocks of their colours, so the last one added isn't always the
    /// last one sewn).
    pub(super) fn trim_after_since(&mut self, since: usize) {
        let latest = self.adds.get(since..).and_then(|blocks| blocks.iter().max().copied());
        if let Some(sewn) = latest.and_then(|j| self.blocks.get_mut(j)) {
            cut_after(&mut sewn.block.stitches);
        }
    }

    /// The design's plan, named `name`, with the `warnings` of what was left out: the blocks in
    /// sewing order, each in the thread of `palette` matched to its art's colour (the nearest, not
    /// a neighbouring block's: a colour change changes thread; [`Palette::assign`]), each unit with
    /// its block's thread, its stitches (from its first in its block up to the next unit's first,
    /// the last unit's to the block's end) and its runs, the stitches numbered from
    /// [`FIRST_NUMBER`].
    pub(super) fn plan(self, name: String, warnings: Vec<String>, palette: Palette) -> DesignPlan {
        let mut plan = DesignPlan { name, warnings, ..DesignPlan::default() };
        let mut number = FIRST_NUMBER;
        let colors: Vec<([u8; 3], Option<PaletteThread>)> = self.blocks.iter().map(|s| (s.block.thread.color, None)).collect();
        let matched = palette.assign(&colors);
        for (k, (Sewn { block: Block { thread, stitches }, members, .. }, matched)) in self.blocks.into_iter().zip(matched).enumerate() {
            let thread = BlockThread { record: thread.record, ..BlockThread::new(thread.color, palette, matched) };
            let first_unit = plan.units.len();
            let ends: Vec<usize> = members.iter().skip(1).map(|m| m.start).chain([stitches.len()]).collect();
            for (m, end) in members.into_iter().zip(ends) {
                let range = m.start..end;
                let runs = runs(&stitches, range.clone());
                let Member { facts: Facts { origin, params, columns, satin_widths, why }, bounds, .. } = m;
                plan.units.push(Unit { origin, thread: thread.clone(), params, columns, satin_widths, why, bounds, block: k, stitches: range, runs });
            }
            let first_number = number;
            number = number.saturating_add(stitches.len());
            plan.blocks.push(ThreadBlock { thread, stitches, units: first_unit..plan.units.len(), first_number });
        }
        plan
    }
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.union(y)),
        (x, y) => x.or(y),
    }
}

/// Whether two boxes share any area (touching edges do not count).
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

#[cfg(test)]
mod tests {
    use vectorcraft_stitch::Thread;
    use vectorcraft_stitch::plan::Style;
    use vectorcraft_stitch::thread::MISMATCH_DELTA_E;

    use super::super::plan::Part;
    use super::*;

    /// A block of one thread sewn along `x0`..`x1` on the x axis, a needle point every 2 mm.
    fn block(x0: f64, x1: f64) -> Planned {
        let mut b = Block::new(Thread::rgb([200, 30, 30]));
        let n = ((x1 - x0) / 2.0).round() as usize;
        b.stitches = (0..=n).map(|k| Stitch::sew(Point::new(x0 + 2.0 * k as f64, 0.0))).collect();
        let origin = Origin { node: NodeId(0), part: Part::Fill(0), instance: None };
        let facts = Facts { origin, params: Some(Params::for_style(Style::Fill)), columns: None, satin_widths: None, why: Choice::Fill };
        Planned { facts, block: b, literal: None }
    }

    /// The trims and jumps between two objects of one thread, the second starting `gap` mm from
    /// where the first ends.
    fn between(gap: f64) -> (usize, usize) {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        seq.add(block(0.0, 10.0));
        seq.add(block(10.0 + gap, 20.0 + gap));
        let plan = seq.plan(String::new(), Vec::new(), Palette::Pec);
        assert_eq!(plan.blocks.len(), 1, "one thread, one block");
        let count = |c: Command| plan.blocks[0].stitches.iter().filter(|s| s.command == c).count();
        (count(Command::Trim), count(Command::Jump))
    }

    /// A unit that joins a block is sewn in the block's thread, whatever its own thread is called:
    /// the palette's thread nearest the art's colour.
    #[test]
    fn a_unit_joining_a_block_has_the_blocks_thread() {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        let (mut first, mut second) = (block(0.0, 10.0), block(12.0, 20.0));
        first.block.thread.name = "first".into();
        second.block.thread.name = "second".into();
        seq.add(first);
        seq.add(second);
        let plan = seq.plan(String::new(), Vec::new(), Palette::Jef);
        assert_eq!(plan.blocks.len(), 1, "one colour, one block");
        assert!(plan.units.iter().all(|u| u.thread == plan.blocks[0].thread), "{:?}", plan.units);
        let t = &plan.blocks[0].thread;
        assert_eq!((t.art, t.palette(), t.index()), ([200, 30, 30], Palette::Jef, Palette::Jef.nearest([200, 30, 30], &[]).index));
        assert_ne!(t.name(), "first");
    }

    /// The plan of two blocks of `first` and `second` on top of each other, in `palette`: each
    /// block's thread and whether it is the art's own colour.
    fn two_blocks(palette: Palette, first: [u8; 3], second: [u8; 3]) -> Vec<(usize, bool)> {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        let (mut a, mut b) = (block(0.0, 10.0), block(0.0, 10.0));
        a.block.thread = Thread::rgb(first);
        b.block.thread = Thread::rgb(second);
        seq.add(a);
        seq.add(b);
        let plan = seq.plan(String::new(), Vec::new(), palette);
        assert_eq!(plan.blocks.len(), 2, "two colours on top of each other");
        plan.blocks.iter().map(|b| (b.thread.index(), b.thread.delta_e == 0.0)).collect()
    }

    /// Blocks are matched in sewing order. A block whose nearest thread is the block before's takes
    /// another when one is still similar, so that the colour change changes thread (two oranges,
    /// with three Brother oranges near); when none is, it keeps its nearest (two reds, whose next
    /// thread is far).
    #[test]
    fn a_block_takes_another_thread_than_the_block_before_only_for_a_similar_one() {
        let (orange, other_orange) = ([254, 180, 58], [253, 181, 58]);
        let nearest = Palette::Pec.nearest(other_orange, &[]).index;
        assert_eq!(Palette::Pec.nearest(orange, &[]).index, nearest, "both oranges' nearest");
        let next = Palette::Pec.nearest(other_orange, &[nearest]);
        assert!(next.delta_e <= MISMATCH_DELTA_E, "{next:?}");
        assert_eq!(two_blocks(Palette::Pec, orange, other_orange), [(nearest, false), (next.index, false)]);
        let (red, other_red) = ([200, 30, 30], [201, 30, 30]);
        let nearest = Palette::Pec.nearest(other_red, &[]).index;
        assert!(Palette::Pec.nearest(other_red, &[nearest]).delta_e > MISMATCH_DELTA_E);
        assert_eq!(two_blocks(Palette::Pec, red, other_red), [(nearest, false), (nearest, false)]);
    }

    /// A block painted with a thread's own colour (a swatch of its thread library) keeps that
    /// thread exactly; the block before it, whose nearest thread that is, takes another when one is
    /// still similar.
    #[test]
    fn a_threads_own_colour_keeps_its_thread() {
        let orange = [254, 180, 58];
        let first = Palette::Pec.nearest(orange, &[]).index;
        let own = Palette::Pec.get(first).map(|(rgb, _)| rgb).unwrap();
        let next = Palette::Pec.nearest(orange, &[first]).index;
        assert_eq!(two_blocks(Palette::Pec, orange, own), [(next, false), (first, true)]);
    }

    /// The move to the next object is cut only when it is longer than `trimLength`: one of exactly
    /// that length keeps the thread on.
    #[test]
    fn a_move_of_exactly_the_trim_length_is_not_cut() {
        let trim = Params::for_style(Style::Fill).sane().trim;
        assert_eq!(between(trim - 1.0), (0, 1));
        assert_eq!(between(trim), (0, 1));
        assert_eq!(between(trim + 0.01), (1, 1));
        // Starting where the last one ended: no move at all.
        assert_eq!(between(0.0), (0, 0));
    }
}
