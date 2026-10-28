//! VectorCraft Stitch: [`VisualBounds`] measures again only the objects an edit replaced, answers as
//! the full walk does ([`Document::art_bounds`], [`Document::bounds_of`]) on random documents and
//! edits, and holds one entry an object of the document last measured.

use std::sync::Arc;

use proptest::prelude::*;
use vectorcraft_color::{Color, Paint};
use vectorcraft_geom::{Affine, BezPath, PathData, Rect, shapes};

use crate::pattern::RepeatSpec;
use crate::visual_bounds::VisualBounds;
use crate::{Appearance, Document, LAYER_COLORS, LayerColor, LineCap, LineJoin, Node, NodeId, NodeKind, Symbol};

/// The bounds as bits, so that two answers compare exactly (the sign of a zero, a NaN).
fn bits(r: Option<Rect>) -> Option<[u64; 4]> {
    r.map(|r| [r.x0, r.y0, r.x1, r.y1].map(f64::to_bits))
}

/// A zig-zag of `points` points from `(x, y)`, `step` apart, stroked `width` pt wide with mitred
/// corners: its bounds measure each corner's spike.
fn zig_zag(d: &mut Document, x: f64, y: f64, points: usize, step: f64, width: f64) -> Node {
    let mut bp = BezPath::new();
    for k in 0..points {
        let at = (x + step * k as f64, y + if k % 2 == 0 { 0.0 } else { step / 2.0 });
        if k == 0 { bp.move_to(at) } else { bp.line_to(at) }
    }
    Node::path(d.alloc_id(), PathData::from_bezpath(&bp), Appearance::basic(Paint::None, Paint::solid(Color::BLACK), width))
}

/// The design the counting tests edit, shaped as a machine file of 2 million needle points opens
/// (a group a thread, a path a run of stitches), smaller: one layer holding `groups` groups of
/// `paths` zig-zag paths each. Returns the paths' ids by group.
fn design(groups: usize, paths: usize) -> (Document, Vec<Vec<NodeId>>) {
    let mut d = Document::new(1000.0, 1000.0);
    let layer = d.layers.first().map(|l| l.id);
    let mut ids = vec![];
    for g in 0..groups {
        let runs: Vec<Node> = (0..paths).map(|p| zig_zag(&mut d, 10.0, (g * paths + p) as f64, 40, 1.0, 0.4)).collect();
        ids.push(runs.iter().map(|n| n.id).collect());
        let group = Node::group(d.alloc_id(), runs.into_iter().map(Arc::new).collect());
        d.insert(layer, usize::MAX, group).unwrap();
    }
    (d, ids)
}

#[test]
fn an_edit_measures_again_only_the_objects_on_its_way_up() {
    let (mut doc, ids) = design(20, 100);
    let mut cache = VisualBounds::default();
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    // The layer, its 20 groups and their 2,000 paths.
    assert_eq!(cache.measured(), 2021);
    assert_eq!(cache.entries(), 2021);
    // Asked again, unchanged: nothing measured.
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    assert_eq!(cache.measured(), 2021);
    // One path moved, the version before kept (as the undo history keeps it): the path, its group
    // and the layer are measured again, the other 1,999 paths and 19 groups are not.
    let history = doc.clone();
    doc.node_mut(ids[7][42]).unwrap().transform(Affine::translate((900.0, -3.0)), false);
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    assert_ne!(doc.art_bounds(), history.art_bounds(), "the edit moved the art's bounds");
    assert_eq!(cache.measured(), 2021 + 3);
    // The selection's bounds come from the entries: nothing more measured.
    let selected = &ids[7][40..50];
    assert_eq!(bits(cache.bounds_of(&doc, selected)), bits(doc.bounds_of(selected, true)));
    assert_eq!(cache.measured(), 2021 + 3);
    // Undo: the version before is measured again where it differs, the same three objects.
    assert_eq!(bits(cache.art_bounds(&history)), bits(history.art_bounds()));
    assert_eq!(cache.measured(), 2021 + 6);
    // With no version before kept, the document is the only owner of its objects: the edit moves
    // them to new addresses all the same (the cache holds them weakly), and costs the same.
    drop(history);
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    let before = cache.measured();
    doc.node_mut(ids[3][0]).unwrap().transform(Affine::translate((-20.0, 0.0)), false);
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    assert_eq!(cache.measured(), before + 3);
    assert_eq!(cache.entries(), 2021, "one entry an object");
}

#[test]
fn adding_removing_and_restacking_measure_only_the_containers_and_whats_new() {
    let (mut doc, ids) = design(20, 100);
    let mut cache = VisualBounds::default();
    cache.art_bounds(&doc);
    let group = |doc: &Document, g: usize| doc.parent_of(ids[g][0]);
    let mut measured = cache.measured();
    let mut expect = |cache: &mut VisualBounds, doc: &Document, more: u64, what: &str| {
        assert_eq!(bits(cache.art_bounds(doc)), bits(doc.art_bounds()), "{what}");
        measured += more;
        assert_eq!(cache.measured(), measured, "{what}");
    };
    // A path added in the middle of a group: it, its group and the layer.
    let added = zig_zag(&mut doc, 5.0, 5.0, 8, 2.0, 1.0);
    doc.insert(group(&doc, 3), 50, added).unwrap();
    expect(&mut cache, &doc, 3, "added");
    // A path removed: its group and the layer.
    doc.remove(ids[5][10]).unwrap();
    expect(&mut cache, &doc, 2, "removed");
    // A path brought to the front of its group keeps its entry: the group and the layer.
    doc.move_node(ids[6][0], group(&doc, 6), usize::MAX).unwrap();
    expect(&mut cache, &doc, 2, "restacked");
    // A path moved to another group is measured again there: it, both groups and the layer.
    doc.move_node(ids[8][0], group(&doc, 9), 30).unwrap();
    expect(&mut cache, &doc, 4, "moved to another group");
    // A whole group removed: the layer, and the entries go with the group.
    doc.remove(group(&doc, 12).unwrap()).unwrap();
    expect(&mut cache, &doc, 1, "group removed");
    assert_eq!(cache.entries(), 2021 + 1 - 1 - 101);
    // Another set of symbols: everything is measured again (instances measure by their art).
    let art = zig_zag(&mut doc, 0.0, 0.0, 3, 1.0, 1.0);
    doc.symbols.push(Symbol { name: "A".into(), art: Arc::new(art) });
    expect(&mut cache, &doc, 1920, "symbols changed");
}

#[test]
fn objects_inside_one_measured_whole_are_measured_on_their_own() {
    let mut doc = Document::new(100.0, 100.0);
    let layer = doc.layers.first().map(|l| l.id);
    let clip = Node::path(doc.alloc_id(), shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)), Appearance::default());
    let inside = zig_zag(&mut doc, 2.0, 2.0, 5, 3.0, 2.0);
    let inside_id = inside.id;
    let mut clipping = Node::group(doc.alloc_id(), vec![Arc::new(clip), Arc::new(inside)]);
    if let NodeKind::Group { clip, .. } = &mut clipping.kind {
        *clip = true;
    }
    let clipping_id = doc.insert(layer, usize::MAX, clipping).unwrap();
    let mut cache = VisualBounds::default();
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    // The layer and the clipping group, measured whole.
    assert_eq!(cache.measured(), 2);
    assert_eq!(bits(cache.bounds_of(&doc, &[clipping_id])), bits(doc.bounds_of(&[clipping_id], true)));
    assert_eq!(cache.measured(), 2);
    assert_eq!(bits(cache.bounds_of(&doc, &[inside_id])), bits(doc.bounds_of(&[inside_id], true)));
    assert_eq!(cache.measured(), 3);
}

/// A small random number generator (SplitMix64), so that one proptest seed makes a document and a
/// session of edits.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// A number from 0 to `n` - 1 (0 when `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    /// A number from `lo` to `hi`.
    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Symbol names the random instances show: two in the document, one missing.
const SYMBOL_NAMES: [&str; 3] = ["A", "B", "missing"];

/// A random stroked path: a rectangle or a zig-zag, its stroke 0 to 4 pt wide, mitred or round,
/// butt or square capped.
fn random_path(r: &mut Rng, d: &mut Document) -> Node {
    let (x, y) = (r.between(-50.0, 150.0), r.between(-50.0, 150.0));
    let mut n = if r.below(2) == 0 {
        let rect = Rect::new(x, y, x + r.between(0.0, 40.0), y + r.between(0.0, 40.0));
        Node::path(d.alloc_id(), shapes::rectangle(rect), Appearance::basic(Paint::solid(Color::WHITE), Paint::solid(Color::BLACK), 1.0))
    } else {
        let points = 2 + r.below(6);
        zig_zag(d, x, y, points, r.between(0.5, 10.0), 1.0)
    };
    let width = [0.0, 0.5, 2.0, 4.0][r.below(4)];
    let (join, cap) = ([LineJoin::Miter, LineJoin::Round][r.below(2)], [LineCap::Butt, LineCap::Square][r.below(2)]);
    if let Some(s) = n.appearance.stroke_mut() {
        (s.width, s.join, s.cap) = (width, join, cap);
    }
    n
}

/// A random object, containers nested at most `depth` more deep: paths, symbol instances, groups
/// (some stroked, some clipping), Intertwine groups, compound paths, sublayers and Repeats, some
/// hidden.
fn random_object(r: &mut Rng, d: &mut Document, depth: usize) -> Node {
    let members = |r: &mut Rng, d: &mut Document, k: usize| -> Vec<Arc<Node>> { (0..k).map(|_| Arc::new(random_object(r, d, depth - 1))).collect() };
    let mut n = match if depth == 0 { r.below(3) } else { r.below(10) } {
        0 | 1 => random_path(r, d),
        2 => {
            let symbol = SYMBOL_NAMES[r.below(SYMBOL_NAMES.len())].to_string();
            let xf = Affine::translate((r.between(-20.0, 120.0), r.between(-20.0, 120.0))) * Affine::scale(r.between(0.5, 2.0));
            Node::new(d.alloc_id(), NodeKind::SymbolInstance { symbol, xf })
        }
        3 | 4 => {
            let k = r.below(4);
            let mut g = Node::group(d.alloc_id(), members(r, d, k));
            if r.below(3) == 0 {
                g.appearance = Appearance::basic(Paint::None, Paint::solid(Color::BLACK), r.between(0.0, 6.0)).into();
            }
            g
        }
        5 => {
            let mut children = vec![Arc::new(random_path(r, d))];
            let k = r.below(3);
            children.extend(members(r, d, k));
            Node::new(d.alloc_id(), NodeKind::Group { children, clip: true })
        }
        6 => {
            let k = 1 + r.below(3);
            Node::new(d.alloc_id(), NodeKind::Intertwine { children: members(r, d, k) })
        }
        7 => {
            let children = vec![Arc::new(random_path(r, d)), Arc::new(random_path(r, d))];
            let mut c = Node::new(d.alloc_id(), NodeKind::Compound { children, rule: Default::default() });
            c.appearance = Appearance::basic(Paint::solid(Color::WHITE), Paint::solid(Color::BLACK), r.between(0.0, 3.0)).into();
            c
        }
        8 => {
            let mut l = Node::layer(d.alloc_id(), "sublayer", LayerColor::Preset(1));
            let k = r.below(4);
            if let Some(c) = l.children_mut() {
                c.extend(members(r, d, k));
            }
            l
        }
        _ => {
            let source = vec![Arc::new(random_path(r, d))];
            Node::new(d.alloc_id(), NodeKind::Repeat(RepeatSpec::radial(source, 1 + r.below(5) as u32, None)))
        }
    };
    n.visible = r.below(6) != 0;
    n
}

/// Symbol `name`'s art: a random object (B's may hold an instance of A).
fn random_symbol(r: &mut Rng, d: &mut Document, name: &str) -> Symbol {
    Symbol { name: name.into(), art: Arc::new(random_object(r, d, 1)) }
}

/// A random document: 1 to 3 layers of up to 6 random objects each, and symbols A and B.
fn random_document(r: &mut Rng) -> Document {
    let mut d = Document::new(100.0, 100.0);
    for _ in 0..r.below(3) {
        let id = d.alloc_id();
        d.layers.push(Arc::new(Node::layer(id, "layer", LayerColor::Custom(LAYER_COLORS[2].1))));
    }
    for l in d.layers.iter().map(|l| l.id).collect::<Vec<_>>() {
        for _ in 0..r.below(7) {
            let n = random_object(r, &mut d, 3);
            d.insert(Some(l), usize::MAX, n).unwrap();
        }
    }
    let (a, b) = (random_symbol(r, &mut d, "A"), random_symbol(r, &mut d, "B"));
    d.symbols.extend([a, b]);
    d
}

/// Every object of `doc` in tree order, and the containers objects can be added to.
fn objects(doc: &Document) -> (Vec<NodeId>, Vec<NodeId>) {
    let (mut all, mut containers) = (vec![], vec![]);
    for l in &doc.layers {
        l.walk(&mut |n| {
            all.push(n.id);
            if matches!(n.kind, NodeKind::Layer { .. } | NodeKind::Group { .. } | NodeKind::Intertwine { .. }) {
                containers.push(n.id);
            }
        });
    }
    (all, containers)
}

/// One random edit of `doc`: an object moved, restroked, hidden or shown, added, removed or
/// restacked, or a symbol's art replaced.
fn random_edit(r: &mut Rng, doc: &mut Document) {
    let (all, containers) = objects(doc);
    let pick = all.get(r.below(all.len())).copied();
    let into = containers.get(r.below(containers.len())).copied();
    match r.below(7) {
        0 => {
            if let Some(n) = pick.and_then(|id| doc.node_mut(id)) {
                n.transform(Affine::translate((r.between(-30.0, 30.0), r.between(-30.0, 30.0))) * Affine::scale(r.between(0.8, 1.2)), true);
            }
        }
        1 => {
            let width = r.between(0.0, 8.0);
            if let Some(s) = pick.and_then(|id| doc.node_mut(id)).and_then(|n| n.appearance.stroke_mut()) {
                s.width = width;
            }
        }
        2 => {
            if let Some(n) = pick.and_then(|id| doc.node_mut(id)) {
                n.visible = !n.visible;
            }
        }
        3 => {
            let n = random_object(r, doc, 2);
            let at = r.below(4);
            if let Some(c) = into {
                doc.insert(Some(c), at, n).unwrap();
            }
        }
        4 => {
            if let Some(id) = pick.filter(|id| doc.layers.iter().all(|l| l.id != *id)) {
                doc.remove(id).unwrap();
            }
        }
        5 => {
            let at = r.below(4);
            if let (Some(id), Some(c)) = (pick, into)
                && doc.layers.iter().all(|l| l.id != id)
            {
                // Refused when it would move an object into itself.
                let _ = doc.move_node(id, Some(c), at);
            }
        }
        _ => {
            let name = ["A", "B"][r.below(2)];
            let s = random_symbol(r, doc, name);
            if let Some(old) = doc.symbols.iter_mut().find(|s| s.name == name) {
                *old = s;
            }
        }
    }
}

/// How many objects `doc` holds.
fn object_count(doc: &Document) -> usize {
    objects(doc).0.len()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    /// On a random document through a random session of edits, each answer is the full walk's, bit
    /// for bit: the art's bounds, every object's on its own and all of them at once. Some edits keep
    /// the version before (as the undo history does), some don't, and some go back to it (undo).
    /// The entries never outnumber the document's objects.
    #[test]
    fn bounds_measured_from_what_changed_are_the_full_walks(seed in any::<u64>(), steps in 0usize..24) {
        let mut r = Rng(seed);
        let mut doc = random_document(&mut r);
        let mut cache = VisualBounds::default();
        let mut history: Vec<Document> = vec![];
        for step in 0..=steps {
            prop_assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()), "art, step {}", step);
            let (all, _) = objects(&doc);
            for id in &all {
                prop_assert_eq!(bits(cache.bounds_of(&doc, &[*id])), bits(doc.bounds_of(&[*id], true)), "object {}, step {}", id, step);
            }
            prop_assert_eq!(bits(cache.bounds_of(&doc, &all)), bits(doc.bounds_of(&all, true)), "all, step {}", step);
            prop_assert!(cache.entries() <= object_count(&doc), "{} entries for {} objects", cache.entries(), object_count(&doc));
            match r.below(4) {
                0 => history.clear(),
                1 if !history.is_empty() => {
                    if let Some(before) = history.pop() {
                        doc = before;
                    }
                    continue;
                }
                _ => history.push(doc.clone()),
            }
            random_edit(&mut r, &mut doc);
        }
    }
}
