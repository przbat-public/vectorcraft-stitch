//! VectorCraft Stitch: visual bounds kept from one measure to the next ([`VisualBounds`]), so that
//! measuring a document again after an edit measures only the objects the edit replaced, not the
//! whole document; on the way up to each of them it compares each sibling's address with the one
//! kept. The canvas measures the art (its scroll bars' span) and the selection (its frame) at every
//! revision, and a drag makes a revision a frame: on a design of 2 million needle points the walk
//! over every object was nearly all of each frame (`docs/embroidery.md` › Editing a large design
//! has the figures).
//!
//! An edit replaces the objects it changes and the containers on the way up to their layer
//! (`Arc::make_mut`, copy on write), and keeps every other object where it was. So an object found
//! at the address it was measured at is the object as measured, and its bounds stand; any other
//! object is measured again. A container is measured by upstream's own arms, through
//! `Node::visual_by`, which asks for each member's bounds: the cache hands over those it kept and
//! measures again only the members that changed.
//!
//! The entries hold their objects weakly, and compare an object's address with the one kept only
//! while the weak reference still has a strong one, as std defines `Weak::as_ptr`. Then the
//! comparison rests on what std documents: a weak reference keeps its allocation, so no other
//! object can take that address while the entry lives; safe code reaches `&mut Node` behind an
//! `Arc` only through `Arc::get_mut`, which refuses while a weak reference exists, or
//! `Arc::make_mut`, which clones a shared object into a new allocation and otherwise dissociates
//! the weak references (they no longer have a strong one); and unsafe code is denied
//! workspace-wide. So an object at a live entry's address is the object as measured. The bounds of
//! symbol instances and Repeats (which count the instances nested in their source) read the
//! document's symbols: when the symbols differ (by name and art), the bounds of the objects that
//! show an instance or hold a Repeat, and of the containers holding them, are measured again.
//!
//! The entries mirror the last document measured, down to the objects measured whole (all but
//! layers and groups without a clipping mask, and Intertwine groups, which `Node::visual_by`
//! measures by their members): one entry an object, and nothing of a version of it once the next
//! version is measured.

use std::sync::{Arc, Weak};

use vectorcraft_geom::{Rect, union_opt};

use crate::id_index::IdMap;
use crate::{Document, Node, NodeId, NodeKind, SymbolBoxes};

#[cfg(test)]
mod tests;

/// The visual bounds of a document's objects as last measured, which [`VisualBounds::art_bounds`]
/// and [`VisualBounds::bounds_of`] measure again only where the document changed. They answer as
/// [`Document::art_bounds`] and [`Document::bounds_of`] (with `visual`) do: the same measures,
/// folded in the same order, so the answers are equal (`==`).
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
    /// its members: what the edits between two measures cost. A diagnostic count for tests and
    /// gates, with no meaning to rely on beyond that.
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
    /// others are measured again, and so are those that read the symbols when they changed.
    fn update(&mut self, doc: &Document, symbols: &SymbolBoxes) {
        let kept = self.symbols.iter().map(|(name, art)| (name.as_str(), live(art)));
        let same = kept.eq(doc.symbols.iter().map(|s| (s.name.as_str(), Arc::as_ptr(&s.art))));
        if !same {
            forget_symbol_readers(&mut self.layers);
            self.symbols = doc.symbols.iter().map(|s| (s.name.clone(), Arc::downgrade(&s.art))).collect();
        }
        let mut layers = LineUp::new(std::mem::take(&mut self.layers));
        for l in &doc.layers {
            layers.measure(l, symbols, &mut self.measured);
        }
        self.layers = layers.finish();
    }

    /// The entry of the object at index path `path` in `doc` ([`Document::index_path`]) when it is
    /// that object's: `None` for an object inside one measured whole.
    fn entry_at(&self, doc: &Document, path: &[usize]) -> Option<&Entry> {
        let (first, rest) = path.split_first()?;
        let (mut node, mut entry): (&Node, &Entry) = (doc.layers.get(*first)?, self.layers.get(*first)?);
        for &i in rest {
            let children = node.children()?;
            // A container's members are a tail of its children (`Node::visual_by`): those it leaves
            // out come first. An object measured whole has no member entries, so none is found.
            let left_out = children.len().saturating_sub(entry.members.len());
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
    /// The entries of the members `Node::visual_by` measured it by, in order: none for an object
    /// measured whole.
    members: Vec<Entry>,
}

impl Entry {
    /// An entry for object `n`, not measured yet.
    fn blank(n: &Node) -> Self {
        Self { node: None, id: n.id, bounds: None, members: Vec::new() }
    }

    /// Whether this is the entry of `n` as it is: measured at its address, which is still its.
    fn holds(&self, n: &Node) -> bool {
        self.node.as_ref().is_some_and(|w| std::ptr::eq(live(w), n))
    }

    /// Measures `n` into this entry by `Node::visual_by`: a container's members are handed the
    /// bounds kept for them, each measured again only when it changed.
    fn measure(&mut self, n: &Arc<Node>, symbols: &SymbolBoxes, measured: &mut u64) {
        *measured += 1;
        let mut members = LineUp::new(std::mem::take(&mut self.members));
        self.bounds = n.visual_by(Some(symbols), &mut |c| members.measure(c, symbols, measured));
        self.members = members.finish();
        self.node = Some(Arc::downgrade(n));
    }
}

/// Forgets the bounds of `entries` that read the symbols, so that they are measured again: an
/// object measured whole that shows a symbol instance or holds a Repeat anywhere in it, and a
/// container measured by its members when one of them reads them (a container's own part reads
/// none). Its other members keep their entries. Returns whether any of `entries` reads them.
fn forget_symbol_readers(entries: &mut [Entry]) -> bool {
    let mut any = false;
    for e in entries {
        let reads = if e.members.is_empty() {
            e.node.as_ref().and_then(Weak::upgrade).is_some_and(|n| shows_symbols(&n))
        } else {
            forget_symbol_readers(&mut e.members)
        };
        if reads {
            e.node = None;
            any = true;
        }
    }
    any
}

/// Whether `n` or anything in it is a symbol instance or a Repeat, whose bounds read the document's
/// symbols.
fn shows_symbols(n: &Node) -> bool {
    let mut found = false;
    n.walk(&mut |c| found |= matches!(c.kind, NodeKind::SymbolInstance { .. } | NodeKind::Repeat(_)));
    found
}

/// The address `w` refers to while it has a strong reference, else null, which no object's address
/// equals: std defines `Weak::as_ptr` only while a strong reference exists, and a weak reference
/// `Arc::make_mut` dissociated has none.
fn live(w: &Weak<Node>) -> *const Node {
    if w.strong_count() > 0 { w.as_ptr() } else { std::ptr::null() }
}

/// The entries of a list of objects as last measured (a container's members, or the layers), lined
/// up with the objects as they come, in their order. While the objects come in their old order,
/// each keeps its entry where it is: its address is the first thing compared, and its id only
/// when the address differs. From the first object out of that order (one added, removed or
/// restacked), the entries left are looked up: first the one after the last taken, so that the
/// objects after a moved one keep their order cheaply, then by id.
struct LineUp {
    /// The entries lined up so far; while no object has come out of the old order, followed by the
    /// old entries still to come.
    entries: Vec<Entry>,
    /// How many objects have come.
    at: usize,
    /// The old entries left, once an object came out of the old order.
    left: Option<Left>,
}

impl LineUp {
    fn new(entries: Vec<Entry>) -> Self {
        Self { entries, at: 0, left: None }
    }

    /// The entry for `c`, the next object of the list, measured again unless it holds `c`: its
    /// bounds.
    fn measure(&mut self, c: &Arc<Node>, symbols: &SymbolBoxes, measured: &mut u64) -> Option<Rect> {
        if self.left.is_none() {
            if let Some(e) = self.entries.get_mut(self.at)
                && (e.holds(c) || e.id == c.id)
            {
                self.at += 1;
                if !e.holds(c) {
                    e.measure(c, symbols, measured);
                }
                return e.bounds;
            }
            let rest = self.entries.split_off(self.at.min(self.entries.len()));
            self.left = Some(Left { old: rest.into_iter().map(Some).collect(), next: 0, by_id: None });
        }
        let mut e = self.left.as_mut().map_or_else(|| Entry::blank(c), |left| left.take(c));
        if !e.holds(c) {
            e.measure(c, symbols, measured);
        }
        let b = e.bounds;
        self.entries.push(e);
        self.at += 1;
        b
    }

    /// The entries lined up: those of objects no longer in the list are gone.
    fn finish(mut self) -> Vec<Entry> {
        self.entries.truncate(self.at);
        self.entries
    }
}

/// The old entries left once an object came out of its list's old order ([`LineUp`]).
struct Left {
    /// The old entries, each taken at most once.
    old: Vec<Option<Entry>>,
    /// Where the next object is looked for first: after the entry the last one took.
    next: usize,
    /// The old entries' places by id (the first, where ids repeat), made the first time an object
    /// isn't found where it is looked for first.
    by_id: Option<IdMap<usize>>,
}

impl Left {
    /// The old entry for `c` (its address compared first, then its id), or a blank one.
    fn take(&mut self, c: &Node) -> Entry {
        if let Some(slot) = self.old.get_mut(self.next)
            && slot.as_ref().is_some_and(|e| e.holds(c) || e.id == c.id)
            && let Some(e) = slot.take()
        {
            self.next += 1;
            return e;
        }
        let old = &self.old;
        let by_id = self.by_id.get_or_insert_with(|| {
            let mut places = IdMap::default();
            for (k, e) in old.iter().enumerate() {
                if let Some(e) = e {
                    places.entry(e.id).or_insert(k);
                }
            }
            places
        });
        let found = by_id.get(&c.id).copied().and_then(|k| Some((k, self.old.get_mut(k)?.take()?)));
        match found {
            Some((k, e)) => {
                self.next = k + 1;
                e
            }
            None => Entry::blank(c),
        }
    }
}
