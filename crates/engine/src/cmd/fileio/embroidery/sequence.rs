//! The order a design's blocks are sewn in: stacking order, with blocks of one thread joined
//! where nothing lies between them, and literal stitches kept as their file had them.

use vectorcraft_doc::NodeId;
use vectorcraft_geom::Rect;
use vectorcraft_stitch::{Block, Command, Stitch};

/// One object's stitches, in document order.
pub(super) struct Planned {
    pub(super) block: Block,
    /// A literal run read from a machine file: the group it belongs to (that file's colour block)
    /// and whether the thread was cut before it. `None`: stitches planned from art.
    pub(super) literal: Option<(NodeId, bool)>,
}

/// Blocks of one thread sewn in one pass where the stacking allows it: a block joins an earlier
/// block of its thread only when it overlaps none of the blocks sewn between the two, so art that
/// lies on top is still sewn last (thread sewn later covers thread sewn earlier, as paint does).
/// The machine changes colour as few times as that allows, not once per object.
///
/// Literal runs (a design read from a file) are left as the file had them: the runs of one group
/// make one block, joined by the jumps (and trims) between them, and never merge with anything
/// else.
pub(super) fn group(planned: Vec<Planned>, jump: f64) -> Vec<Block> {
    /// How far apart two blocks' stitches must be not to touch: a thread's width and a satin's pull.
    const CLEARANCE: f64 = 1.0;
    // Each block so far, its bounds, and the group of the literal runs it holds.
    let mut out: Vec<(Block, Option<Rect>, Option<NodeId>)> = Vec::new();
    for Planned { block: b, literal } in planned {
        let Some(&first) = b.stitches.first() else { continue };
        let bounds = b.bounds().map(|r| r.inflate(CLEARANCE, CLEARANCE));
        if let Some((owner, trim_before)) = literal {
            match out.last_mut() {
                Some((merged, mb, Some(o))) if *o == owner && merged.thread.color == b.thread.color => {
                    if let Some(last) = merged.stitches.last().copied()
                        && trim_before
                    {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    merged.stitches.push(Stitch::jump(first.point()));
                    merged.stitches.extend(b.stitches.iter().copied());
                    *mb = union(*mb, bounds);
                }
                _ => {
                    // A cut at the very start of the block, as the file had it.
                    let mut b = b;
                    if trim_before {
                        b.stitches.insert(0, Stitch::new(first.x, first.y, Command::Trim));
                    }
                    out.push((b, bounds, Some(owner)));
                }
            }
            continue;
        }
        let target = out.iter().rposition(|(m, _, lit)| lit.is_none() && m.thread.color == b.thread.color).filter(|&j| {
            out.iter().skip(j + 1).all(|(_, other, _)| match (bounds, other) {
                (Some(a), Some(o)) => !overlaps(a, *o),
                _ => false,
            })
        });
        match target.and_then(|j| out.get_mut(j)) {
            Some((merged, mb, _)) => {
                // To the next object: a jump, with the thread cut first when the move is longer
                // than `jumpLength` (each object is tied off and in by its own plan).
                if let Some(last) = merged.stitches.last().copied() {
                    if last.distance(first) > jump {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    merged.stitches.push(Stitch::jump(first.point()));
                }
                merged.stitches.extend(b.stitches.iter().copied());
                *mb = union(*mb, bounds);
            }
            None => out.push((b, bounds, None)),
        }
    }
    out.into_iter().map(|(b, ..)| b).collect()
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
