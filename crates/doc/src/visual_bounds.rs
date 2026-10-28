//! VectorCraft Stitch: visual bounds kept from one measure to the next ([`VisualBounds`]), so that
//! measuring a document again after an edit costs what the edit changed, not the size of the
//! document. The canvas measures the art (its scroll bars' span) and the selection (its frame) at
//! every revision, and a drag makes a revision a frame: on a design of 2 million needle points the
//! walk over every object took about 40 ms of each frame.
//!
//! An edit replaces the objects it changes and the containers on the way up to their layer
//! (`Arc::make_mut`, copy on write), and keeps every other object where it was. So an object found
//! at the address it was measured at is the object as measured, and its bounds stand; any other
//! object is measured again, a layer or group as the union of its members' bounds, most of them
//! kept. The entries hold their objects weakly: an object's allocation outlives it while its entry
//! does, so no other object can take its address, and `Arc::make_mut` moves an object it changes
//! to a new allocation while weak references to it remain (the crate has no unsafe code that could
//! change one in place). The bounds of symbol instances and Repeats read the document's symbols:
//! another set of symbols (by name and art) starts the measures over.
//!
//! The entries mirror the last document measured, down to the objects measured whole (all but
//! layers, groups without a clipping mask and Intertwine groups, [`Node::visual_members`]): one
//! entry an object, and nothing of a version of it once the next version is measured.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use vectorcraft_geom::{Rect, union_opt};

use crate::node::grown;
use crate::{Document, Node, NodeId, SymbolBoxes};

/// The visual bounds of a document's objects as last measured, which [`VisualBounds::art_bounds`]
/// and [`VisualBounds::bounds_of`] measure again only where the document changed. They answer as
/// [`Document::art_bounds`] and [`Document::bounds_of`] (with `visual`) do, bit for bit.
#[derive(Default)]
pub struct VisualBounds {
    /// The entries of the layers of the document last measured, in order.
    layers: Vec<Entry>,
    /// The symbols the entries were measured with: each one's name and art, the art held weakly.
    symbols: Vec<(String, Weak<Node>)>,
    /// Objects measured since this was made, each container once for its members' union.
    measured: u64,
}

impl VisualBounds {
    /// [`Document::art_bounds`]: the visual bounds of all of `doc`'s art.
    pub fn art_bounds(&mut self, doc: &Document) -> Option<Rect> {
        let symbols = doc.symbol_boxes();
        self.update(doc, &symbols);
        self.layers.iter().fold(None, |acc, e| union_opt(acc, e.bounds))
    }

    /// [`Document::bounds_of`] `ids` with `visual`: the union of their visual bounds. An object
    /// inside one measured whole (the members of a clipping group or a compound path) is measured
    /// on its own each time.
    pub fn bounds_of(&mut self, doc: &Document, ids: &[NodeId]) -> Option<Rect> {
        let symbols = doc.symbol_boxes();
        self.update(doc, &symbols);
        let mut bounds = None;
        for id in ids {
            let Some(path) = doc.index_path(*id) else { continue };
            let b = match self.entry_at(doc, &path) {
                Some(e) => e.bounds,
                None => {
                    let Some(n) = doc.node_at(&path) else { continue };
                    self.measured += 1;
                    n.visual(Some(&symbols))
                }
            };
            bounds = union_opt(bounds, b);
        }
        bounds
    }

    /// How many objects this has measured since it was made, each container once for the union of
    /// its members: what the edits between two measures cost.
    pub fn measured(&self) -> u64 {
        self.measured
    }

    /// The entries it holds: one for each object of the document last measured, down to the
    /// objects measured whole.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> usize {
        fn count(entries: &[Entry]) -> usize {
            entries.iter().map(|e| 1 + count(&e.members)).sum()
        }
        count(&self.layers)
    }

    /// Brings the entries up to `doc`: those of the objects still at their address stand, the
    /// others are measured again, all of them when the symbols changed.
    fn update(&mut self, doc: &Document, symbols: &SymbolBoxes) {
        let same = self.symbols.len() == doc.symbols.len()
            && self.symbols.iter().zip(&doc.symbols).all(|((name, art), s)| *name == s.name && std::ptr::eq(art.as_ptr(), Arc::as_ptr(&s.art)));
        if !same {
            self.layers.clear();
            self.symbols = doc.symbols.iter().map(|s| (s.name.clone(), Arc::downgrade(&s.art))).collect();
        }
        update(&mut self.layers, &doc.layers, symbols, &mut self.measured);
    }

    /// The entry of the object at index path `path` in `doc` ([`Document::index_path`]) when it is
    /// that object's: `None` for an object inside one measured whole.
    fn entry_at(&self, doc: &Document, path: &[usize]) -> Option<&Entry> {
        let (first, rest) = path.split_first()?;
        let (mut node, mut entry): (&Node, &Entry) = (doc.layers.get(*first)?, self.layers.get(*first)?);
        for &i in rest {
            let (members, _) = node.visual_members()?;
            let children = node.children()?;
            // The members are the children after those a container leaves out
            // ([`Node::editing_children`]).
            let left_out = children.len().saturating_sub(members.len());
            node = children.get(i)?;
            entry = entry.members.get(i.checked_sub(left_out)?)?;
        }
        entry.holds(node).then_some(entry)
    }
}

/// One object's visual bounds as measured, with its members' when it is measured as they are.
struct Entry {
    /// The object measured, held weakly (see the module's notes); `None` until it is measured.
    node: Option<Weak<Node>>,
    /// Its id: an object's next version takes the entry of its id, so that the members it kept
    /// keep theirs.
    id: NodeId,
    bounds: Option<Rect>,
    /// The entries of its members ([`Node::visual_members`]), in order: none for an object measured
    /// whole.
    members: Vec<Entry>,
}

impl Entry {
    /// An entry for object `n`, not measured yet.
    fn blank(n: &Node) -> Self {
        Self { node: None, id: n.id, bounds: None, members: Vec::new() }
    }

    /// Whether this is the entry of `n` as it is: measured at its address.
    fn holds(&self, n: &Node) -> bool {
        self.node.as_ref().is_some_and(|w| std::ptr::eq(w.as_ptr(), n))
    }

    /// Measures `n` into this entry: a container as the union of its members' bounds, each
    /// measured again only when it changed; any other object whole.
    fn measure(&mut self, n: &Arc<Node>, symbols: &SymbolBoxes, measured: &mut u64) {
        *measured += 1;
        self.bounds = match n.visual_members() {
            Some((members, grow)) => {
                update(&mut self.members, members, symbols, measured);
                grown(self.members.iter().fold(None, |acc, e| union_opt(acc, e.bounds)), grow)
            }
            None => {
                self.members = Vec::new();
                n.visual(Some(symbols))
            }
        };
        (self.node, self.id) = (Some(Arc::downgrade(n)), n.id);
    }
}

/// Brings `entries` up to `nodes` (a container's members, or the layers): an object at its entry's
/// address keeps the entry as it is, any other object is measured again.
fn update(entries: &mut Vec<Entry>, nodes: &[Arc<Node>], symbols: &SymbolBoxes, measured: &mut u64) {
    if entries.len() != nodes.len() || entries.iter().zip(nodes).any(|(e, n)| e.id != n.id) {
        restack(entries, nodes);
    }
    for (e, n) in entries.iter_mut().zip(nodes) {
        if !e.holds(n) {
            e.measure(n, symbols, measured);
        }
    }
}

/// Lines `entries` up with `nodes` after objects were added, removed or moved among them: the
/// objects in the same order at the start and at the end keep their entries where they are; each
/// object between takes the entry of the object of its id among those between, if any, else a
/// blank one. An object added or removed costs a look at each of the others.
fn restack(entries: &mut Vec<Entry>, nodes: &[Arc<Node>]) {
    let head = entries.iter().zip(nodes).take_while(|(e, n)| e.id == n.id).count();
    let tail = entries.iter().skip(head).rev().zip(nodes.iter().skip(head).rev()).take_while(|(e, n)| e.id == n.id).count();
    let mut kept = std::mem::take(entries);
    let end = kept.split_off(kept.len().saturating_sub(tail));
    let mut between: HashMap<NodeId, Entry> = kept.split_off(head.min(kept.len())).into_iter().map(|e| (e.id, e)).collect();
    let middle = nodes.len().saturating_sub(head + tail);
    kept.extend(nodes.iter().skip(head).take(middle).map(|n| between.remove(&n.id).unwrap_or_else(|| Entry::blank(n))));
    kept.extend(end);
    *entries = kept;
}
