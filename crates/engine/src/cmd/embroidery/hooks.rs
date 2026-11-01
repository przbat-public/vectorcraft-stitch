//! The marked upstream hooks of the stitch commands (FORK.md's hook list): Delete with needle points
//! selected, Join of runs of stitches, and a run cut into pieces or in place by the cutting tools.
//! Each acts as a stitch edit on runs of stitches and leaves everything else to upstream; `mod.rs`
//! wraps them for the upstream files.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;

use super::needle_edits::{self, ONE_RUN, RUNS_WITH_ART};
use super::needles::{self, Stitches};
use crate::{EngineError, Result, Session};

/// Delete with needle points selected (the marked hook in `path::delete_anchors`): when every object the
/// selection holds is a path of stitches and it selects needle points of them (no segment), the
/// needle points are deleted as stitches (`embroidery.deleteNeedlePoints`: the neighbours merge,
/// the paths leave the selection); `None` otherwise, for upstream's Delete.
pub(crate) fn delete_selected(s: &mut Session) -> Option<Result<Value>> {
    let st = s.doc().ok()?;
    let sel = &st.selection;
    let stitches = |id: &NodeId| st.doc.node(*id).is_some_and(needles::is_stitches);
    let all = !sel.anchors.is_empty() && sel.segments.is_empty() && sel.objects.iter().all(stitches) && sel.anchors.keys().all(stitches);
    all.then(|| needle_edits::delete(s, &json!({})))
}

/// Join (the marked hook in `path.join`) of paths `ids` when any is a path of stitches: all of them
/// joined as `embroidery.joinRuns {ids}` joins them, in the order they are sewn (their stacking
/// order in their parent), never reversed; a path of stitches with art is refused. `None` when none
/// is a path of stitches, for upstream's Join.
pub(crate) fn join_paths(s: &mut Session, ids: &[NodeId]) -> Option<Result<Value>> {
    let st = s.doc().ok()?;
    let doc = st.doc.clone();
    let stitches = |id: &NodeId| doc.node(*id).is_some_and(needles::is_stitches);
    if !ids.iter().any(stitches) {
        return None;
    }
    if !ids.iter().all(stitches) {
        return Some(Err(EngineError::Other(RUNS_WITH_ART.to_string())));
    }
    if ids.len() < 2 {
        return Some(Err(EngineError::Other(ONE_RUN.to_string())));
    }
    // Each one's place among its parent's children, each parent's read once.
    let mut place: std::collections::HashMap<NodeId, usize> = std::collections::HashMap::new();
    let mut parents = BTreeSet::new();
    for &id in ids {
        let Some(parent) = doc.parent_of(id).filter(|p| parents.insert(*p)).and_then(|p| doc.node(p)) else { continue };
        place.extend(parent.children().into_iter().flatten().enumerate().map(|(i, c)| (c.id, i)));
    }
    let mut sorted = ids.to_vec();
    sorted.sort_by_key(|id| place.get(id).copied().unwrap_or(usize::MAX));
    Some(needle_edits::join_runs(s, &json!({ "ids": sorted.iter().map(|i| i.0).collect::<Vec<_>>() })))
}

/// A path of stitches `id` of `d` as it is before it is cut (the marked hooks in
/// `draw2::replace_with_pieces`, `draw2::split` and `path::cut_at_anchors`), for [`run_cut`] and
/// [`cut_in_place`]; `None` for anything else.
pub(crate) fn before_cut(d: &Document, id: NodeId) -> Option<Stitches> {
    d.node(id).and_then(|n| needles::read(n).ok())
}

/// Where each needle point of a run lay before it was cut, by its coordinates' bits: its place, its
/// marks and whether a cut came before it, each taken once, in sewing order.
struct Lay {
    at: std::collections::HashMap<(u64, u64), std::collections::VecDeque<(needles::Needle, bool)>>,
}

impl Lay {
    fn of(run: &Stitches) -> Self {
        let mut at: std::collections::HashMap<(u64, u64), std::collections::VecDeque<(needles::Needle, bool)>> = Default::default();
        for r in &run.runs {
            for (i, n) in r.needles.iter().enumerate() {
                at.entry((n.p.x.to_bits(), n.p.y.to_bits())).or_default().push_back((*n, i == 0 && r.cut_before));
            }
        }
        Self { at }
    }

    /// The next needle point of the run before that lay at `p` and isn't taken yet.
    fn take(&mut self, p: Point) -> Option<(needles::Needle, bool)> {
        self.at.get_mut(&(p.x.to_bits(), p.y.to_bits()))?.pop_front()
    }
}

/// `pieces` (in sewing order) cut from the path of stitches `run` was: each piece's needle points
/// take the stops and exact millimetres of the needle points of `run` they lie on, each taken once
/// (a stop on the needle point a cut made twice stays on the first piece), each of its runs the cut
/// before it that `run` had there; the first piece keeps the cut before the path, the last the cut
/// and the stop after it (the object's own too), as they were once. A needle point made by the cut
/// has none. A file's run's later piece starts after the needle point the cut made twice (the walk
/// jumps onto a file's run's first needle point and would sew it again); a manual run's keeps it
/// (art goes on from the hole the needle is in without sewing it twice).
pub(crate) fn run_cut(run: Option<&Stitches>, pieces: &mut [&mut vectorcraft_doc::Node]) {
    let Some(run) = run else { return };
    let mut lay = Lay::of(run);
    let count = pieces.len();
    let mut before: Option<Point> = None;
    for (p, piece) in pieces.iter_mut().enumerate() {
        let Ok(mut st) = needles::read(piece) else { continue };
        let more_than_one = st.len() > 1;
        if p > 0
            && !run.marks.manual
            && more_than_one
            && let Some(first) = st.runs.first_mut()
            && first.needles.first().zip(before).is_some_and(|(n, b)| needles::same(n.p, b))
        {
            first.needles.remove(0);
        }
        for r in &mut st.runs {
            r.cut_before = false;
            for (i, n) in r.needles.iter_mut().enumerate() {
                match lay.take(n.p) {
                    Some((was, cut)) => {
                        (n.stop, n.exact) = (was.stop, was.exact);
                        if i == 0 && cut {
                            r.cut_before = true;
                        }
                    }
                    None => (n.stop, n.exact) = (false, [None, None]),
                }
            }
        }
        before = st.runs.iter().rev().find_map(|r| r.needles.last()).map(|n| n.p);
        let (first, final_piece) = (p == 0, p + 1 == count);
        st.marks.trim_before = first && run.marks.trim_before;
        st.marks.trim_after = final_piece && run.marks.trim_after;
        st.marks.stop_after = final_piece && run.marks.stop_after;
        st.object_after = if final_piece { run.object_after } else { Default::default() };
        needles::write(piece, st);
    }
}

/// Path of stitches `id` of `d` (as `run` before) cut where it lies by a tool that keeps the pieces
/// as its subpaths: a needle point the cut made twice, ending a run and starting the next at one
/// spot where no run ended and started before, is kept once (the next run starts after it), as the
/// walk would sew it twice; the marks follow their needle points as they always do.
pub(crate) fn cut_in_place(d: &mut Document, id: NodeId, run: Option<&Stitches>) {
    let Some(run) = run else { return };
    let Some(n) = d.node_mut(id) else { return };
    let Ok(mut st) = needles::read(n) else { return };
    // Where runs met at one spot before, each spot as often as it was.
    let mut met: std::collections::HashMap<(u64, u64), usize> = Default::default();
    for w in run.runs.windows(2) {
        if let [a, b] = w
            && let (Some(end), Some(start)) = (a.needles.last(), b.needles.first())
            && needles::same(end.p, start.p)
        {
            *met.entry((start.p.x.to_bits(), start.p.y.to_bits())).or_default() += 1;
        }
    }
    let mut changed = false;
    let mut end: Option<Point> = None;
    for r in &mut st.runs {
        if let (Some(e), Some(first)) = (end, r.needles.first().copied())
            && needles::same(e, first.p)
            && r.needles.len() > 1
        {
            match met.get_mut(&(first.p.x.to_bits(), first.p.y.to_bits())).filter(|c| **c > 0) {
                Some(c) => *c -= 1,
                None => {
                    r.needles.remove(0);
                    changed = true;
                }
            }
        }
        if let Some(last) = r.needles.last() {
            end = Some(last.p);
        }
    }
    if changed {
        needles::write(n, st);
    }
}
