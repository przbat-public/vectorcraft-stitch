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
use vectorcraft_stitch::thread::Palette;
use vectorcraft_stitch::{Block, Command, SAME_POINT, Stitch};

use super::plan::{BlockThread, DesignPlan, FIRST_NUMBER, Origin, ThreadBlock, Unit, runs};

/// How far apart two blocks' stitches must be not to touch: a thread's width and a satin's pull.
pub(super) const CLEARANCE: f64 = 1.0;

/// One sewing unit's stitches as planned, in document order.
pub(super) struct Planned {
    /// What it sews.
    pub(super) origin: Origin,
    /// The parameters it was planned with (`None`: a literal run, sewn as its points are), and
    /// those of a fill's narrow parts sewn as satin columns (`None`: it had none).
    pub(super) params: Option<Params>,
    pub(super) columns: Option<Params>,
    pub(super) block: Block,
    /// A literal run read from a machine file: the group it belongs to (that file's colour block)
    /// and whether the thread was cut before it. `None`: stitches planned from art.
    pub(super) literal: Option<(NodeId, bool)>,
}

/// A unit in a block: what it is and was planned with, the box its stitches cover as planned, and
/// where its stitches start in the block (they run up to the next unit's first stitch). It is sewn
/// in its block's thread.
struct Member {
    origin: Origin,
    params: Option<Params>,
    columns: Option<Params>,
    bounds: Option<Rect>,
    start: usize,
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
    /// How many units were added, and the block the last one went to.
    added: usize,
    last: Option<usize>,
}

impl Sequence {
    pub(super) fn new(connect: Params) -> Self {
        Self { blocks: Vec::new(), connect: connect.sane(), added: 0, last: None }
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
        let Planned { origin, params, columns, block: mut b, literal } = planned;
        let Some(&first) = b.stitches.first() else { return };
        self.added += 1;
        let planned_bounds = b.bounds();
        let bounds = planned_bounds.map(|r| r.inflate(CLEARANCE, CLEARANCE));
        let member = |start: usize| Member { origin, params, columns, bounds: planned_bounds, start };
        if let Some((owner, trim_before)) = literal {
            match self.blocks.last_mut() {
                Some(sewn) if sewn.literal == Some(owner) && sewn.block.thread.color == b.thread.color => {
                    let start = sewn.block.stitches.len();
                    let merged = &mut sewn.block;
                    if let Some(last) = merged.stitches.last().copied()
                        && trim_before
                        && last.command != Command::Trim
                    {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    merged.stitches.push(Stitch::jump(first.point()));
                    merged.stitches.extend(b.stitches.iter().copied());
                    sewn.bounds = union(sewn.bounds, bounds);
                    sewn.members.push(member(start));
                    self.last = self.blocks.len().checked_sub(1);
                }
                _ => {
                    // A cut at the very start of the block, as the file had it.
                    if trim_before {
                        b.stitches.insert(0, Stitch::new(first.x, first.y, Command::Trim));
                    }
                    let members = vec![member(0)];
                    self.blocks.push(Sewn { block: b, bounds, literal: Some(owner), members });
                    self.last = self.blocks.len().checked_sub(1);
                }
            }
            return;
        }
        let target = self.target(b.thread.color, bounds);
        self.last = Some(target.unwrap_or(self.blocks.len()));
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
        self.added
    }

    /// Cut the thread after the last unit added (an object that asks for it, or the last of a
    /// group that does), however near the next one starts: the cut is the last of that unit's
    /// stitches.
    pub(super) fn trim_last(&mut self) {
        let Some(sewn) = self.last.and_then(|j| self.blocks.get_mut(j)) else { return };
        let block = &mut sewn.block;
        if let Some(&last) = block.stitches.last()
            && last.command != Command::Trim
        {
            block.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
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
        let colors: Vec<([u8; 3], Option<usize>)> = self.blocks.iter().map(|s| (s.block.thread.color, None)).collect();
        let matched = palette.assign(&colors);
        for (k, (Sewn { block: Block { thread, stitches }, members, .. }, matched)) in self.blocks.into_iter().zip(matched).enumerate() {
            let thread = BlockThread::new(thread.color, palette, matched);
            let first_unit = plan.units.len();
            let ends: Vec<usize> = members.iter().skip(1).map(|m| m.start).chain([stitches.len()]).collect();
            for (m, end) in members.into_iter().zip(ends) {
                let range = m.start..end;
                let runs = runs(&stitches, range.clone());
                let Member { origin, params, columns, bounds, .. } = m;
                plan.units.push(Unit { origin, thread: thread.clone(), params, columns, bounds, block: k, stitches: range, runs });
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

    use super::super::plan::Part;
    use super::*;

    /// A block of one thread sewn along `x0`..`x1` on the x axis, a needle point every 2 mm.
    fn block(x0: f64, x1: f64) -> Planned {
        let mut b = Block::new(Thread::rgb([200, 30, 30]));
        let n = ((x1 - x0) / 2.0).round() as usize;
        b.stitches = (0..=n).map(|k| Stitch::sew(Point::new(x0 + 2.0 * k as f64, 0.0))).collect();
        let origin = Origin { node: NodeId(0), part: Part::Fill(0), instance: None };
        Planned { origin, params: Some(Params::for_style(Style::Fill)), columns: None, block: b, literal: None }
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
        assert_eq!((t.art, t.palette, t.index), ([200, 30, 30], Palette::Jef, Palette::Jef.nearest([200, 30, 30], &[]).index));
        assert_ne!(t.name, "first");
    }

    /// Blocks are matched in sewing order, and a block whose nearest thread is the block before's
    /// gets another, so that the colour change changes thread.
    #[test]
    fn a_block_is_not_sewn_in_the_thread_of_the_block_before() {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        let (mut a, mut b) = (block(0.0, 10.0), block(0.0, 10.0));
        a.block.thread = Thread::rgb([200, 30, 30]);
        b.block.thread = Thread::rgb([201, 30, 30]);
        seq.add(a);
        seq.add(b);
        let plan = seq.plan(String::new(), Vec::new(), Palette::Pec);
        assert_eq!(plan.blocks.len(), 2, "two colours on top of each other");
        let nearest = Palette::Pec.nearest([201, 30, 30], &[]).index;
        assert_eq!(plan.blocks[0].thread.index, nearest, "both colours' nearest");
        assert_ne!(plan.blocks[1].thread.index, nearest);
        assert_eq!(plan.blocks[1].thread.index, Palette::Pec.nearest([201, 30, 30], &[nearest]).index);
    }

    /// A block painted with a thread's own colour (a swatch of its thread library) keeps that
    /// thread exactly, and the block before it, whose nearest thread that is, takes another.
    #[test]
    fn a_threads_own_colour_keeps_its_thread() {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        let (mut near, mut own) = (block(0.0, 10.0), block(0.0, 10.0));
        let red = Palette::Pec.get(5).map(|(rgb, _)| rgb).unwrap();
        near.block.thread = Thread::rgb([red[0] - 3, red[1] + 2, red[2]]);
        own.block.thread = Thread::rgb(red);
        seq.add(near);
        seq.add(own);
        let plan = seq.plan(String::new(), Vec::new(), Palette::Pec);
        let threads: Vec<(usize, bool)> = plan.blocks.iter().map(|b| (b.thread.index, b.thread.delta_e == 0.0)).collect();
        let near_red = [red[0] - 3, red[1] + 2, red[2]];
        assert_eq!(Palette::Pec.nearest(near_red, &[]).index, 5, "Red is its nearest");
        assert_eq!(threads, [(Palette::Pec.nearest(near_red, &[5]).index, false), (5, true)]);
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
