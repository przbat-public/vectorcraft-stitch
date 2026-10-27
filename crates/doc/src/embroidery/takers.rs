//! Which objects take embroidery settings, and why one doesn't ([`takers`]): the settings commands'
//! rule, and the embroidery tool's (it shows points only on what takes them). A group or a layer
//! stands for the paths and compound paths in it, a compound path's member for its compound path;
//! guides, stitches read from a file, type, images, symbol instances, placed files and live objects
//! take none.

use std::collections::HashSet;

use crate::{AppearanceItem, Document, Node, NodeId, NodeKind};

use super::PartKind;

/// Why an object given takes no part settings, as the replies name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// A run of stitches read from a machine file: sewn as its points are.
    Literal,
    /// A guide: never sewn.
    Guide,
    /// Type: not sewn until lettering sews it (the project owner's choice, 2026-10-23).
    Type,
    Image,
    /// A symbol's settings are set on its art (an instance takes only the cut after it).
    SymbolInstance,
    /// A blend, an envelope, a mesh or a repeat: not sewn as it is.
    Live,
    Placed,
    Intertwine,
    /// It has no part of the kind asked for (no fill, or no stroke).
    NoPart,
}

impl Skip {
    /// Every reason, as `embroidery.info` and the settings commands' replies give them.
    pub const ALL: [Skip; 9] =
        [Skip::Literal, Skip::Guide, Skip::Type, Skip::Image, Skip::SymbolInstance, Skip::Live, Skip::Placed, Skip::Intertwine, Skip::NoPart];

    /// Its id in replies.
    pub fn id(self) -> &'static str {
        match self {
            Skip::Literal => "literal",
            Skip::Guide => "guide",
            Skip::Type => "type",
            Skip::Image => "image",
            Skip::SymbolInstance => "symbolInstance",
            Skip::Live => "live",
            Skip::Placed => "placed",
            Skip::Intertwine => "intertwine",
            Skip::NoPart => "noPart",
        }
    }
}

/// The objects a settings command takes, and those it skips with why.
pub type Takers = (Vec<NodeId>, Vec<(NodeId, Skip)>);

/// The objects `ids` stand for, as the settings commands take them, each once in the order met: a
/// path or a compound path itself; a compound path's member its compound path; the paths and
/// compound paths in a group or a layer, at any depth. What takes no settings is skipped, with why.
pub fn takers(d: &Document, ids: &[NodeId]) -> Takers {
    let (mut take, mut skip, mut seen) = (Vec::new(), Vec::new(), HashSet::new());
    for id in ids {
        let Some(n) = d.node(compound_of(d, *id)) else { continue };
        visit_takers(n, &mut take, &mut skip, &mut seen);
    }
    (take.into_iter().map(|n| n.id).collect(), skip)
}

/// The first `max` objects of [`takers`]' list, in its order, found without walking the rest of
/// the selection (the embroidery tool shows a few objects' handles of however many selected).
pub fn first_takers(d: &Document, ids: &[NodeId], max: usize) -> Vec<NodeId> {
    let (mut take, mut skip, mut seen) = (Vec::new(), Vec::new(), HashSet::new());
    for id in ids {
        if take.len() >= max {
            break;
        }
        let Some(n) = d.node(compound_of(d, *id)) else { continue };
        visit_takers_until(n, &mut take, &mut skip, &mut seen, max);
    }
    take.into_iter().take(max).map(|n| n.id).collect()
}

/// [`takers`] of `n`, which need not be in a document (a symbol's art): the objects that take
/// settings into `take`, what is skipped into `skip`, each node once (`seen`).
pub fn visit_takers<'a>(n: &'a Node, take: &mut Vec<&'a Node>, skip: &mut Vec<(NodeId, Skip)>, seen: &mut HashSet<NodeId>) {
    visit_takers_until(n, take, skip, seen, usize::MAX);
}

/// [`visit_takers`], stopping once `take` holds `max`.
fn visit_takers_until<'a>(n: &'a Node, take: &mut Vec<&'a Node>, skip: &mut Vec<(NodeId, Skip)>, seen: &mut HashSet<NodeId>, max: usize) {
    if take.len() >= max || !seen.insert(n.id) {
        return;
    }
    let why = match &n.kind {
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
            for c in children {
                if take.len() >= max {
                    break;
                }
                visit_takers_until(c, take, skip, seen, max);
            }
            return;
        }
        NodeKind::Path { guide: true, .. } => Skip::Guide,
        NodeKind::Path { .. } if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) => Skip::Literal,
        NodeKind::Path { .. } | NodeKind::Compound { .. } => {
            take.push(n);
            return;
        }
        NodeKind::Text(_) => Skip::Type,
        NodeKind::Image(_) => Skip::Image,
        NodeKind::SymbolInstance { .. } => Skip::SymbolInstance,
        NodeKind::PlacedDocument(_) => Skip::Placed,
        NodeKind::Intertwine { .. } => Skip::Intertwine,
        NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) | NodeKind::Repeat(_) => Skip::Live,
    };
    skip.push((n.id, why));
}

/// The object `id` stands for: its compound path when it is one's member, else itself.
pub fn compound_of(d: &Document, id: NodeId) -> NodeId {
    d.parent_of(id).filter(|p| matches!(d.node(*p).map(|n| &n.kind), Some(NodeKind::Compound { .. }))).unwrap_or(id)
}

/// Whether `n` has a part of kind `kind`: a fill, or a stroke, in its appearance, whether it paints
/// or not (settings are kept for it either way, and take effect once it paints).
pub fn has_part(n: &Node, kind: PartKind) -> bool {
    n.appearance.items.iter().any(|i| part_kind(i) == kind)
}

/// The kind of part item `item` is of.
pub fn part_kind(item: &AppearanceItem) -> PartKind {
    match item {
        AppearanceItem::Fill(_) => PartKind::Fill,
        AppearanceItem::Stroke(_) => PartKind::Stroke,
    }
}

#[cfg(test)]
mod tests {
    use vectorcraft_geom::{Rect, shapes};

    use super::*;
    use crate::Appearance;

    /// The first objects of the list are the same however they are found: a group of five paths
    /// and a sixth on its own, the first three of them found without walking the rest.
    #[test]
    fn the_first_takers_are_the_lists_first() {
        let mut d = Document::new(200.0, 200.0);
        let l = d.layers[0].id;
        let path = |d: &mut Document, k: f64| {
            let id = d.alloc_id();
            Node::path(id, shapes::rectangle(Rect::new(k, 0.0, k + 5.0, 5.0)), Appearance::default_art())
        };
        let members: Vec<std::sync::Arc<Node>> = (0..5).map(|k| std::sync::Arc::new(path(&mut d, f64::from(k) * 10.0))).collect();
        let group = d.alloc_id();
        d.insert(Some(l), 0, Node::group(group, members)).unwrap();
        let alone = path(&mut d, 100.0);
        let alone_id = alone.id;
        d.insert(Some(l), 1, alone).unwrap();
        let all = takers(&d, &[group, alone_id]).0;
        assert_eq!(all.len(), 6);
        for max in [0, 3, 5, 6, 10] {
            assert_eq!(first_takers(&d, &[group, alone_id], max), all.iter().copied().take(max).collect::<Vec<_>>(), "{max}");
        }
    }
}
