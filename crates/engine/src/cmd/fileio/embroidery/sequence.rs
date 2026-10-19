//! The order a design's blocks are sewn in: stacking order, with blocks of one thread joined
//! where nothing lies between them, and literal stitches kept as their file had them.
//!
//! Objects are added one at a time, in document order, so that each can be planned knowing where
//! the needle will be when it starts ([`Sequence::needle`]): an object starts near the end of the
//! one sewn before it, which saves jumps and trims.

use vectorcraft_doc::NodeId;
use vectorcraft_geom::{Point, Rect};
use vectorcraft_stitch::plan::Params;
use vectorcraft_stitch::{Block, Command, Stitch};

/// How far apart two blocks' stitches must be not to touch: a thread's width and a satin's pull.
pub(super) const CLEARANCE: f64 = 1.0;

/// One object's stitches, in document order.
pub(super) struct Planned {
    pub(super) block: Block,
    /// A literal run read from a machine file: the group it belongs to (that file's colour block)
    /// and whether the thread was cut before it. `None`: stitches planned from art.
    pub(super) literal: Option<(NodeId, bool)>,
}

/// The blocks sewn so far, each with its bounds (grown by [`CLEARANCE`]) and the group of the
/// literal runs it holds.
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
    blocks: Vec<(Block, Option<Rect>, Option<NodeId>)>,
    /// The jump and trim lengths between objects.
    connect: Params,
    /// How many objects were added, and the block the last one went to.
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
        let j = self.blocks.iter().rposition(|(m, _, lit)| lit.is_none() && m.thread.color == color)?;
        self.blocks
            .iter()
            .skip(j + 1)
            .all(|(_, other, _)| match (bounds, other) {
                (Some(a), Some(o)) => !overlaps(a, *o),
                _ => false,
            })
            .then_some(j)
    }

    /// Where the needle is when an object that joins `target` (or, `None`, starts a block of its
    /// own after the last) starts: the end of that block's stitches.
    pub(super) fn needle(&self, target: Option<usize>) -> Option<Point> {
        let block = match target {
            Some(j) => self.blocks.get(j),
            None => self.blocks.last(),
        }?;
        block.0.stitches.iter().rev().find(|s| s.command != Command::Trim).map(|s| s.point())
    }

    /// Add one object's stitches after the others.
    pub(super) fn add(&mut self, planned: Planned) {
        let Planned { block: mut b, literal } = planned;
        let Some(&first) = b.stitches.first() else { return };
        self.added += 1;
        let bounds = b.bounds().map(|r| r.inflate(CLEARANCE, CLEARANCE));
        if let Some((owner, trim_before)) = literal {
            match self.blocks.last_mut() {
                Some((merged, mb, Some(o))) if *o == owner && merged.thread.color == b.thread.color => {
                    if let Some(last) = merged.stitches.last().copied()
                        && trim_before
                        && last.command != Command::Trim
                    {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    merged.stitches.push(Stitch::jump(first.point()));
                    merged.stitches.extend(b.stitches.iter().copied());
                    *mb = union(*mb, bounds);
                    self.last = self.blocks.len().checked_sub(1);
                }
                _ => {
                    // A cut at the very start of the block, as the file had it.
                    if trim_before {
                        b.stitches.insert(0, Stitch::new(first.x, first.y, Command::Trim));
                    }
                    self.blocks.push((b, bounds, Some(owner)));
                    self.last = self.blocks.len().checked_sub(1);
                }
            }
            return;
        }
        let target = self.target(b.thread.color, bounds);
        self.last = Some(target.unwrap_or(self.blocks.len()));
        match target.and_then(|j| self.blocks.get_mut(j)) {
            Some((merged, mb, _)) => {
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
                    if at.distance(first) > 1e-9 || last.command == Command::Trim {
                        merged.stitches.push(Stitch::jump(first.point()));
                    } else if first.command == Command::Stitch {
                        skip = 1;
                    }
                }
                merged.stitches.extend(b.stitches.iter().skip(skip).copied());
                *mb = union(*mb, bounds);
            }
            None => self.blocks.push((b, bounds, None)),
        }
    }

    /// How many objects with stitches were added so far.
    pub(super) fn added(&self) -> usize {
        self.added
    }

    /// Cut the thread after the last object added (one that asks for it), however near the next
    /// object starts.
    pub(super) fn trim_last(&mut self) {
        let Some((block, ..)) = self.last.and_then(|j| self.blocks.get_mut(j)) else { return };
        if let Some(&last) = block.stitches.last()
            && last.command != Command::Trim
        {
            block.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
        }
    }

    /// The blocks, in sewing order.
    pub(super) fn blocks(self) -> Vec<Block> {
        self.blocks.into_iter().map(|(b, ..)| b).collect()
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

    use super::*;

    /// A block of one thread sewn along `x0`..`x1` on the x axis, a needle point every 2 mm.
    fn block(x0: f64, x1: f64) -> Planned {
        let mut b = Block::new(Thread::rgb([200, 30, 30]));
        let n = ((x1 - x0) / 2.0).round() as usize;
        b.stitches = (0..=n).map(|k| Stitch::sew(Point::new(x0 + 2.0 * k as f64, 0.0))).collect();
        Planned { block: b, literal: None }
    }

    /// The trims and jumps between two objects of one thread, the second starting `gap` mm from
    /// where the first ends.
    fn between(gap: f64) -> (usize, usize) {
        let mut seq = Sequence::new(Params::for_style(Style::Fill));
        seq.add(block(0.0, 10.0));
        seq.add(block(10.0 + gap, 20.0 + gap));
        let blocks = seq.blocks();
        assert_eq!(blocks.len(), 1, "one thread, one block");
        let count = |c: Command| blocks[0].stitches.iter().filter(|s| s.command == c).count();
        (count(Command::Trim), count(Command::Jump))
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
