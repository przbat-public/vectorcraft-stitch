//! VectorCraft Stitch: [`VisualBounds`] measures again only the objects an edit replaced, answers as
//! the full walk does ([`Document::art_bounds`], [`Document::bounds_of`]) on random documents and
//! edits, and holds one entry an object of the document last measured.

use std::sync::Arc;

use proptest::prelude::*;
use vectorcraft_color::{Color, Paint};
use vectorcraft_geom::{Affine, BezPath, PathData, Rect, shapes};

use crate::embroidery::seeded::seeded;
use crate::pattern::RepeatSpec;
use crate::visual_bounds::VisualBounds;
use crate::{Appearance, Document, ImageObject, LAYER_COLORS, LayerColor, LineCap, LineJoin, Node, NodeId, NodeKind, Symbol};

/// The bounds as bits, so that two answers compare exactly: equal coordinates as `==` has them
/// (`f64::min` and `max` may give either zero of two that compare equal), every NaN as one.
fn bits(r: Option<Rect>) -> Option<[u64; 4]> {
    let exact = |v: f64| if v.is_nan() { f64::NAN.to_bits() } else { (v + 0.0).to_bits() };
    r.map(|r| [r.x0, r.y0, r.x1, r.y1].map(exact))
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
    // Another set of symbols, with no instance and no Repeat in the design: nothing reads them.
    let art = zig_zag(&mut doc, 0.0, 0.0, 3, 1.0, 1.0);
    doc.symbols.push(Symbol { name: "A".into(), art: Arc::new(art), dynamic: false });
    expect(&mut cache, &doc, 0, "a symbol added");
    // An instance of it added to a group: the instance, its group and the layer.
    let instance = Node::new(doc.alloc_id(), NodeKind::SymbolInstance { symbol: "A".into(), xf: Affine::translate((300.0, 0.0)) });
    doc.insert(group(&doc, 4), 7, instance).unwrap();
    expect(&mut cache, &doc, 3, "an instance added");
    // The symbol's art replaced: what reads the symbols, the instance and the containers on its
    // way up, measured again; the 1,918 other objects keep their bounds.
    let art = zig_zag(&mut doc, 0.0, 0.0, 5, 40.0, 3.0);
    if let Some(s) = doc.symbols.first_mut() {
        s.art = Arc::new(art);
    }
    expect(&mut cache, &doc, 3, "the symbol's art replaced");
    assert_eq!(cache.entries(), 1921);
    // Renamed: its instance no longer shows it (the 20 × 20 square stands in), measured again.
    if let Some(s) = doc.symbols.first_mut() {
        s.name = "B".into();
    }
    expect(&mut cache, &doc, 3, "the symbol renamed");
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

/// A group whose first child its bounds leave out (a Shaper group's source, as `Node::editing_children`
/// gives it): its members are answered from their entries, the child left out is measured on its own.
#[test]
fn a_shaper_groups_members_are_answered_from_their_entries() {
    let (mut doc, ids) = design(1, 4);
    let group = doc.parent_of(ids[0][0]).unwrap();
    doc.node_mut(group).unwrap().extras_mut().shaper = Some(Box::default());
    let mut cache = VisualBounds::default();
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    // The layer, the group and the three paths after the source.
    assert_eq!((cache.measured(), cache.entries()), (5, 5));
    for id in &ids[0][1..] {
        assert_eq!(bits(cache.bounds_of(&doc, &[*id])), bits(doc.bounds_of(&[*id], true)));
    }
    assert_eq!(cache.measured(), 5, "the members came from their entries");
    assert_eq!(bits(cache.bounds_of(&doc, &[ids[0][0]])), bits(doc.bounds_of(&[ids[0][0]], true)));
    assert_eq!(cache.measured(), 6, "the source, left out of the group's bounds, measured on its own");
}

/// A group whose clipping mask is turned on is measured whole from then on: its members' entries
/// go, and come back when the mask is turned off.
#[test]
fn a_clipping_mask_turned_on_and_off_drops_and_keeps_member_entries() {
    let (mut doc, ids) = design(2, 5);
    let group = doc.parent_of(ids[1][0]).unwrap();
    let mut cache = VisualBounds::default();
    cache.art_bounds(&doc);
    assert_eq!(cache.entries(), 13);
    let set_clip = |doc: &mut Document, on: bool| {
        if let NodeKind::Group { clip, .. } = &mut doc.node_mut(group).unwrap().kind {
            *clip = on;
        }
    };
    set_clip(&mut doc, true);
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    assert_eq!(cache.entries(), 8, "the clipping group measured whole, its five paths' entries gone");
    set_clip(&mut doc, false);
    assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
    assert_eq!(cache.entries(), 13);
}

/// Measuring another document with the same cache lets the last one go: none of its objects is
/// held, even weakly, once the next is measured.
#[test]
fn measuring_another_document_lets_the_last_one_go() {
    let (first, ids) = design(3, 10);
    let (second, _) = design(2, 4);
    let mut cache = VisualBounds::default();
    cache.art_bounds(&first);
    let path = first.node_arc(ids[2][9]).unwrap().clone();
    let (layer, held) = (first.layers.first().unwrap().clone(), Arc::weak_count(&path));
    assert_eq!((Arc::weak_count(&layer), held), (1, 1), "the cache holds the first document's objects weakly");
    assert_eq!(bits(cache.art_bounds(&second)), bits(second.art_bounds()));
    assert_eq!((Arc::weak_count(&layer), Arc::weak_count(&path)), (0, 0), "and none once the second is measured");
    assert_eq!(cache.entries(), 1 + 2 + 2 * 4);
}

/// The minimal failure the property suite found when the selection's ids were looked up in one
/// walk: an id that appears twice (a damaged file keeps its ids) while the document's id index
/// still holds the place it had when the id was unique. `Document::index_path` then finds the copy
/// at that place, a walk finds the first in tree order; the cache looks ids up as
/// `Document::bounds_of` does, so both measure the same copy.
#[test]
fn a_repeated_id_means_the_object_document_bounds_of_finds() {
    let mut doc = Document::new(100.0, 100.0);
    let layer = doc.layers.first().map(|l| l.id);
    let first = Node::group(doc.alloc_id(), vec![]);
    let first_group = doc.insert(layer, usize::MAX, first).unwrap();
    let path = zig_zag(&mut doc, 10.0, 10.0, 4, 2.0, 1.0);
    let (id, original) = (path.id, path.clone());
    let second = Node::group(doc.alloc_id(), vec![Arc::new(path)]);
    doc.insert(layer, usize::MAX, second).unwrap();
    // The index made afresh while the id is unique, then a copy, moved elsewhere, put first in tree
    // order without moving the original.
    doc.drop_lookup_cache();
    assert_eq!(doc.index_path(id), Some(vec![0, 1, 0]));
    assert!(doc.has_lookup_cache());
    let mut copy = original;
    copy.transform(Affine::translate((60.0, 40.0)), false);
    doc.children_mut(Some(first_group)).unwrap().push(Arc::new(copy));
    assert_eq!(doc.index_path(id), Some(vec![0, 1, 0]), "the index still finds the original");
    assert_eq!(doc.index_paths([id]).get(&id), Some(&vec![0, 0, 0]), "a walk finds the copy");
    let mut cache = VisualBounds::default();
    assert_eq!(bits(cache.bounds_of(&doc, &[id])), bits(doc.bounds_of(&[id], true)));
}

/// A damaged or hand-made native file keeps its ids (the reader only moves the id counter), so
/// siblings may share one: through an edit, a restack and a removal every answer is the full walk's
/// and the entries are exactly the objects the cache keeps one for.
#[test]
fn siblings_sharing_an_id_answer_as_the_full_walk() {
    let (mut doc, ids) = design(3, 6);
    let shared = ids[1][0];
    for id in &ids[1][1..] {
        doc.node_mut(*id).unwrap().id = shared;
    }
    let mut cache = VisualBounds::default();
    let check = |cache: &mut VisualBounds, doc: &Document, what: &str| {
        assert_eq!(bits(cache.art_bounds(doc)), bits(doc.art_bounds()), "{what}");
        let (mine, walked) = both_bounds_of(cache, doc, &[shared]);
        assert_eq!(mine, walked, "{what}");
        assert_eq!(cache.entries(), doc.layers.iter().map(|l| mirrored(l)).sum::<usize>(), "{what}");
    };
    check(&mut cache, &doc, "made");
    doc.node_mut(shared).unwrap().transform(Affine::translate((50.0, 0.0)), false);
    check(&mut cache, &doc, "the first moved");
    // All six still share the id: the addresses alone tell them apart.
    let group = doc.parent_of(shared);
    doc.move_node(shared, group, usize::MAX).unwrap();
    check(&mut cache, &doc, "the first restacked to the front");
    doc.remove(shared).unwrap();
    check(&mut cache, &doc, "one removed");
}

/// The cache's walk on a document as deep as one may be, on a thread with the 1 MB stack of a
/// Windows main thread (the web build's stack is 1 MB too).
#[test]
fn a_document_as_deep_as_allowed_is_measured_on_a_1_mb_stack() {
    /// The stack of the thread the test measures on, bytes: a Windows main thread's, and the web
    /// build's.
    const STACK_BYTES: usize = 1 << 20;
    std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(|| {
            let mut doc = Document::new(100.0, 100.0);
            let mut n = zig_zag(&mut doc, 0.0, 0.0, 3, 1.0, 1.0);
            // A path in groups nested as deep as a document may nest them, with its layer.
            for _ in 0..crate::MAX_DEPTH - 2 {
                n = Node::group(doc.alloc_id(), vec![Arc::new(n)]);
            }
            let layer = doc.layers.first().map(|l| l.id);
            doc.insert(layer, usize::MAX, n).unwrap();
            assert!(!doc.too_deep(None));
            let mut cache = VisualBounds::default();
            assert_eq!(bits(cache.art_bounds(&doc)), bits(doc.art_bounds()));
            assert_eq!(cache.entries(), crate::MAX_DEPTH);
        })
        .unwrap()
        .join()
        .unwrap();
}

/// A 10 × 10 square at the origin, filled, unstroked: its bounds are its own box.
fn square(d: &mut Document) -> Arc<Node> {
    Arc::new(Node::path(
        d.alloc_id(),
        shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)),
        Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0),
    ))
}

/// A 2 pt stroke with round joins and butt caps: it reaches 1 pt past its path on every side.
fn round_stroke() -> Appearance {
    let mut a = Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 2.0);
    if let Some(s) = a.stroke_mut() {
        s.join = LineJoin::Round;
    }
    a
}

/// Boxes worked out by hand for the containers whose bounds the cache unions: an oracle that shares
/// no code with the walk or the cache. A 10 × 10 square in a group, in a sublayer and in a
/// top-level layer, each with a 2 pt stroke of its own reaching 1 pt, grown by it; in an Intertwine
/// group with that stroke, not grown; under a clipping layer, its mask's box; under a clipping group
/// whose mask is stroked, the stroked mask's box.
#[test]
fn containers_measure_as_worked_out_by_hand() {
    let mut doc = Document::new(100.0, 100.0);
    let layer = doc.layers.first().map(|l| l.id);
    let mask = |d: &mut Document, a: Appearance| Arc::new(Node::path(d.alloc_id(), shapes::rectangle(Rect::new(20.0, 20.0, 30.0, 30.0)), a));
    let mut group = Node::group(doc.alloc_id(), vec![square(&mut doc)]);
    group.appearance = round_stroke().into();
    let mut sublayer = Node::layer(doc.alloc_id(), "stroked", LayerColor::Preset(1));
    if let Some(c) = sublayer.children_mut() {
        c.push(square(&mut doc));
    }
    sublayer.appearance = round_stroke().into();
    let mut intertwine = Node::new(doc.alloc_id(), NodeKind::Intertwine { children: vec![square(&mut doc)] });
    intertwine.appearance = round_stroke().into();
    let mut clipping_layer = Node::layer(doc.alloc_id(), "clipping", LayerColor::Preset(2));
    let unstroked = Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0);
    if let NodeKind::Layer { children, clip, .. } = &mut clipping_layer.kind {
        (*children, *clip) = (vec![mask(&mut doc, unstroked), square(&mut doc)], true);
    }
    let clipping_group = Node::new(doc.alloc_id(), NodeKind::Group { children: vec![mask(&mut doc, round_stroke()), square(&mut doc)], clip: true });
    let cases = [
        (group, Rect::new(-1.0, -1.0, 11.0, 11.0), "a stroked group"),
        (sublayer, Rect::new(-1.0, -1.0, 11.0, 11.0), "a stroked sublayer"),
        (intertwine, Rect::new(0.0, 0.0, 10.0, 10.0), "a stroked Intertwine group"),
        (clipping_layer, Rect::new(20.0, 20.0, 30.0, 30.0), "a clipping layer"),
        (clipping_group, Rect::new(19.0, 19.0, 31.0, 31.0), "a clipping group with a stroked mask"),
    ];
    let mut cache = VisualBounds::default();
    for (n, want, what) in cases {
        let id = doc.insert(layer, usize::MAX, n).unwrap();
        assert_eq!(doc.bounds_of(&[id], true), Some(want), "the walk, {what}");
        assert_eq!(cache.bounds_of(&doc, &[id]), Some(want), "the cache, {what}");
    }
    // A top-level layer with that stroke: the art's bounds.
    let mut alone = Document::new(100.0, 100.0);
    let only = square(&mut alone);
    if let Some(l) = alone.layers.first_mut() {
        let l = Arc::make_mut(l);
        l.appearance = round_stroke().into();
        if let Some(c) = l.children_mut() {
            c.push(only);
        }
    }
    assert_eq!(alone.art_bounds(), Some(Rect::new(-1.0, -1.0, 11.0, 11.0)), "the walk, a stroked top-level layer");
    assert_eq!(VisualBounds::default().art_bounds(&alone), Some(Rect::new(-1.0, -1.0, 11.0, 11.0)), "the cache, a stroked top-level layer");
}

/// Random documents measured with every field but their layers and symbols taken from
/// [`donor_document`].
const DOCUMENTS_SWAPPED: u64 = 32;

/// A document that differs from a new one in the fields the walk could read besides its layers and
/// symbols: its artboards, units, rulers' origin, colour mode, guides, graphic styles, unknown
/// keys (a perspective grid), embroidery settings, page and raster settings.
fn donor_document() -> Document {
    let mut d = Document::new(500.0, 300.0);
    d.title = "donor".into();
    d.units = crate::Unit::Millimeters;
    d.ruler_origin = vectorcraft_geom::Point::new(17.0, 23.0);
    d.color_mode = crate::ColorMode::Cmyk;
    if let Some(mut board) = d.artboards.first().cloned() {
        (board.id, board.rect) = (2, Rect::new(600.0, 0.0, 900.0, 200.0));
        d.artboards.push(board);
    }
    d.guides.push(crate::Guide::new(true, 40.0));
    d.graphic_styles = vec![crate::GraphicStyle::new("Thick", Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 12.0))].into();
    let mut unknown = serde_json::Map::new();
    unknown.insert("perspectiveGrid".into(), serde_json::json!({ "horizon": 120.0, "left": [0.0, 300.0], "right": [500.0, 300.0] }));
    d.unknown = unknown.into();
    d.embroidery = Some(Box::default());
    (d.raster_effects_ppi, d.page_isolate, d.page_knockout, d.spot_use_lab, d.transparency_grid) = (300.0, true, true, false, true);
    d
}

/// The cache keeps boxes by address, which is right only while visual bounds read nothing of a
/// document but its layers and its symbols. With every other field taken from another document,
/// the full walk's answers, and the cache's, stay as they are.
#[test]
fn visual_bounds_read_only_the_layers_and_the_symbols() {
    let donor = serde_json::to_value(donor_document()).unwrap();
    for seed in 0..DOCUMENTS_SWAPPED {
        let own = serde_json::to_value(random_document(&mut Rng(seed))).unwrap();
        let base: Document = serde_json::from_value(own.clone()).unwrap();
        let mut mixed = donor.clone();
        for key in ["layers", "symbols"] {
            match own.get(key) {
                Some(v) => mixed[key] = v.clone(),
                None => {
                    mixed.as_object_mut().unwrap().remove(key);
                }
            }
        }
        let swapped: Document = serde_json::from_value(mixed).unwrap();
        assert_ne!(swapped.artboards, base.artboards, "seed {seed}: the other fields come from the donor");
        assert_eq!(bits(swapped.art_bounds()), bits(base.art_bounds()), "seed {seed}");
        assert_eq!(bits(VisualBounds::default().art_bounds(&swapped)), bits(base.art_bounds()), "seed {seed}, the cache");
        for id in objects(&base).0 {
            assert_eq!(bits(swapped.bounds_of(&[id], true)), bits(base.bounds_of(&[id], true)), "seed {seed}, object {id}");
        }
    }
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

/// Symbol names the random instances show: two in the document, one missing (its instances drawn
/// as the 20 × 20 square until a symbol is renamed to it).
const SYMBOL_NAMES: [&str; 3] = ["A", "B", "missing"];

/// The Image Trace views a random live trace shows (its source image left out of its bounds
/// unless the view shows it).
const TRACE_VIEWS: [&str; 4] = ["result", "outlines", "outlinesWithSource", "source"];

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
/// (some stroked, some clipping), Intertwine groups, compound paths, sublayers (some stroked, some
/// clipping), Repeats, Shaper groups and live traces (whose first child is left out of their
/// bounds), some hidden.
fn random_object(r: &mut Rng, d: &mut Document, depth: usize) -> Node {
    let members = |r: &mut Rng, d: &mut Document, k: usize| -> Vec<Arc<Node>> { (0..k).map(|_| Arc::new(random_object(r, d, depth - 1))).collect() };
    let mut n = match if depth == 0 { r.below(3) } else { r.below(12) } {
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
            let clipping = r.below(3) == 0;
            let mut kids = if clipping { vec![Arc::new(random_path(r, d))] } else { vec![] };
            let k = r.below(4);
            kids.extend(members(r, d, k));
            if let NodeKind::Layer { children, clip, .. } = &mut l.kind {
                (*children, *clip) = (kids, clipping);
            }
            if r.below(3) == 0 {
                l.appearance = Appearance::basic(Paint::None, Paint::solid(Color::BLACK), r.between(0.0, 6.0)).into();
            }
            l
        }
        9 => {
            let k = 1 + r.below(3);
            let mut g = Node::group(d.alloc_id(), members(r, d, k));
            g.extras_mut().shaper = Some(Box::default());
            g
        }
        10 => {
            // A live trace: its hidden source image first, then the traced art.
            let mut source = Node::new(
                d.alloc_id(),
                NodeKind::Image(Box::new(ImageObject {
                    key: "traced".into(),
                    width: 1 + r.below(300) as u32,
                    height: 1 + r.below(300) as u32,
                    xf: Affine::translate((r.between(-50.0, 50.0), r.between(-50.0, 50.0))),
                    link: None,
                    placement: Default::default(),
                    pixels: None,
                })),
            );
            source.visible = false;
            let mut children = vec![Arc::new(source)];
            let k = r.below(3);
            children.extend(members(r, d, k));
            let mut g = Node::group(d.alloc_id(), children);
            let view = TRACE_VIEWS[r.below(TRACE_VIEWS.len())];
            g.extras_mut().trace = Some(Box::new(serde_json::json!({ "view": view })));
            g
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
    Symbol { name: name.into(), art: Arc::new(random_object(r, d, 1)), dynamic: false }
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

/// One random edit of `doc`, named for the failure message: an object moved, restroked, hidden or
/// shown, added, removed or restacked, a group's or layer's clipping mask turned on or off, a copy
/// of an object that keeps its id added, an object put at a second place, a top-level layer added,
/// removed or restacked, or the symbols changed (an art replaced, one renamed, deleted or added,
/// or two swapped).
fn random_edit(r: &mut Rng, doc: &mut Document) -> String {
    let (all, containers) = objects(doc);
    let pick = all.get(r.below(all.len())).copied();
    let into = containers.get(r.below(containers.len())).copied();
    let top = |doc: &Document, id: NodeId| doc.layers.iter().any(|l| l.id == id);
    // Not into itself or an object inside it.
    let fits = |doc: &Document, id: NodeId, c: NodeId| doc.ancestry(c).is_some_and(|a| !a.contains(&id));
    match r.below(15) {
        0 => {
            if let Some(n) = pick.and_then(|id| doc.node_mut(id)) {
                n.transform(Affine::translate((r.between(-30.0, 30.0), r.between(-30.0, 30.0))) * Affine::scale(r.between(0.8, 1.2)), true);
            }
            format!("{pick:?} moved")
        }
        1 => {
            let width = r.between(0.0, 8.0);
            if let Some(s) = pick.and_then(|id| doc.node_mut(id)).and_then(|n| n.appearance.stroke_mut()) {
                s.width = width;
            }
            format!("{pick:?} restroked {width}")
        }
        2 => {
            if let Some(n) = pick.and_then(|id| doc.node_mut(id)) {
                n.visible = !n.visible;
            }
            format!("{pick:?} hidden or shown")
        }
        3 => {
            let n = random_object(r, doc, 2);
            let at = r.below(4);
            if let Some(c) = into {
                // Refused when a repeated id's places disagree: an edit made, or none.
                let _ = doc.insert(Some(c), at, n);
            }
            format!("an object added to {into:?} at {at}")
        }
        4 => {
            if let Some(id) = pick.filter(|id| !top(doc, *id)) {
                // Refused when a repeated id's places disagree: an edit made, or none.
                let _ = doc.remove(id);
            }
            format!("{pick:?} removed")
        }
        5 => {
            let at = r.below(4);
            if let (Some(id), Some(c)) = (pick, into)
                && !top(doc, id)
            {
                // Refused when it would move an object into itself.
                let _ = doc.move_node(id, Some(c), at);
            }
            format!("{pick:?} moved into {into:?} at {at}")
        }
        6 => {
            if let Some(n) = into.and_then(|id| doc.node_mut(id))
                && let NodeKind::Group { clip, .. } | NodeKind::Layer { clip, .. } = &mut n.kind
            {
                *clip = !*clip;
            }
            format!("{into:?}'s clipping mask turned on or off")
        }
        7 => {
            let at = r.below(4);
            if let (Some(id), Some(c)) = (pick, into)
                && !top(doc, id)
                && fits(doc, id, c)
                && let Some(copy) = doc.node(id).cloned()
            {
                // Refused when a repeated id's places disagree: an edit made, or none.
                let _ = doc.insert(Some(c), at, copy);
            }
            format!("a copy of {pick:?}, its id kept, added to {into:?} at {at}")
        }
        8 => {
            if let (Some(id), Some(c)) = (pick, into)
                && !top(doc, id)
                && fits(doc, id, c)
                && let Some(shared) = doc.node_arc(id).cloned()
                && let Ok(children) = doc.children_mut(Some(c))
            {
                children.push(shared);
            }
            format!("{pick:?} put at a second place, in {into:?}")
        }
        9 => {
            let id = doc.alloc_id();
            let mut l = Node::layer(id, "layer", LayerColor::Preset(3));
            let k = r.below(3);
            if let Some(c) = l.children_mut() {
                for _ in 0..k {
                    c.push(Arc::new(random_object(r, doc, 2)));
                }
            }
            let at = r.below(doc.layers.len() + 1);
            doc.layers.insert(at, Arc::new(l));
            format!("a top-level layer added at {at}")
        }
        10 => {
            let at = r.below(doc.layers.len());
            if doc.layers.len() > 1 {
                doc.layers.remove(at);
            }
            format!("top-level layer {at} removed")
        }
        11 => {
            let (a, b) = (r.below(doc.layers.len()), r.below(doc.layers.len()));
            if a < doc.layers.len() && b < doc.layers.len() {
                doc.layers.swap(a, b);
            }
            format!("top-level layers {a} and {b} swapped")
        }
        12 => {
            let name = SYMBOL_NAMES[r.below(SYMBOL_NAMES.len())];
            let to = SYMBOL_NAMES[r.below(SYMBOL_NAMES.len())];
            if let Some(s) = doc.symbols.iter_mut().find(|s| s.name == name) {
                s.name = to.into();
            }
            format!("symbol {name} renamed {to}")
        }
        13 => {
            let at = r.below(doc.symbols.len() + 1);
            if at < doc.symbols.len() {
                doc.symbols.remove(at);
            } else {
                let name = SYMBOL_NAMES[r.below(SYMBOL_NAMES.len())];
                let s = random_symbol(r, doc, name);
                doc.symbols.push(s);
            }
            format!("symbol {at} deleted, or one added")
        }
        _ => {
            if r.below(2) == 0 && doc.symbols.len() > 1 {
                let (a, b) = (r.below(doc.symbols.len()), r.below(doc.symbols.len()));
                doc.symbols.swap(a, b);
                return format!("symbols {a} and {b} swapped");
            }
            let name = ["A", "B"][r.below(2)];
            let s = random_symbol(r, doc, name);
            if let Some(old) = doc.symbols.iter_mut().find(|s| s.name == name) {
                *old = s;
            }
            format!("symbol {name}'s art replaced")
        }
    }
}

/// The objects the cache keeps an entry for in `n`, counted again by the test: `n`, and the
/// members of a layer or group without a clipping mask (all but a first child it leaves out) and of
/// an Intertwine group, each counted the same way. The rule `Node::visual` measures containers by,
/// stated again: should upstream change it, this count says so.
fn mirrored(n: &Node) -> usize {
    1 + match &n.kind {
        NodeKind::Layer { clip: false, .. } | NodeKind::Group { clip: false, .. } => n.editing_children().iter().map(|c| mirrored(c)).sum(),
        NodeKind::Intertwine { children } => children.iter().map(|c| mirrored(c)).sum::<usize>(),
        _ => 0,
    }
}

/// The property suite's cases, and the most edits a case makes: a random document then up to
/// [`MOST_EDITS`] edits, every answer checked before the first edit and after each one. One step
/// in [`ANOTHER_DOCUMENT_EVERY`] also measures another random document with the same cache, as the
/// canvas measures each document it shows. Estimates: the suite runs in a second or two.
const CASES: u32 = 96;
const MOST_EDITS: usize = 24;
const ANOTHER_DOCUMENT_EVERY: usize = 6;

proptest! {
    #![proptest_config(seeded(CASES))]
    /// On a random document through a random session of edits, each answer is the full walk's: the
    /// art's bounds, every object's on its own and all of them at once. Some edits keep the version
    /// before (as the undo history does), some don't, and some go back to it (undo); now and then
    /// another document is measured in between. The entries are exactly the objects the cache
    /// keeps one for.
    #[test]
    fn bounds_measured_from_what_changed_are_the_full_walks(seed in any::<u64>(), steps in 0usize..=MOST_EDITS) {
        let mut r = Rng(seed);
        let mut doc = random_document(&mut r);
        let mut cache = VisualBounds::default();
        let mut history: Vec<Document> = vec![];
        check(&mut cache, &doc, 0, "the document as made")?;
        for step in 1..=steps {
            let what = match r.below(4) {
                1 if !history.is_empty() => {
                    if let Some(before) = history.pop() {
                        doc = before;
                    }
                    "undo".to_string()
                }
                0 => {
                    history.clear();
                    random_edit(&mut r, &mut doc)
                }
                _ => {
                    history.push(doc.clone());
                    random_edit(&mut r, &mut doc)
                }
            };
            if r.below(ANOTHER_DOCUMENT_EVERY) == 0 {
                let other = random_document(&mut r);
                check(&mut cache, &other, step, "another document")?;
            }
            check(&mut cache, &doc, step, &what)?;
        }
    }
}

/// `cache`'s and the full walk's bounds of `ids` in `doc`, each looked up from the same state of
/// the document's id index. Which copy of a repeated id `Document::index_path` finds depends on that
/// state (a place it still holds from when the id was unique, or the first in tree order), and a
/// lookup can rebuild it, so two calls in a row may look a repeated id up differently: each side
/// looks its ids up on a clone of `doc`, which shares the index as it is (and shares every object,
/// so the cache's entries still hold).
fn both_bounds_of(cache: &mut VisualBounds, doc: &Document, ids: &[NodeId]) -> (Option<[u64; 4]>, Option<[u64; 4]>) {
    let (mine, walked) = (doc.clone(), doc.clone());
    (bits(cache.bounds_of(&mine, ids)), bits(walked.bounds_of(ids, true)))
}

/// Every answer of `cache` on `doc` after `step` edits, the last `what`: the art's bounds, every
/// object's on its own and all of them at once, as the full walk gives them, and one entry for
/// each object the cache keeps one for.
fn check(cache: &mut VisualBounds, doc: &Document, step: usize, what: &str) -> Result<(), TestCaseError> {
    prop_assert_eq!(bits(cache.art_bounds(doc)), bits(doc.art_bounds()), "art, step {} ({})", step, what);
    let (all, _) = objects(doc);
    for id in &all {
        let (mine, walked) = both_bounds_of(cache, doc, &[*id]);
        prop_assert_eq!(mine, walked, "object {}, step {} ({})", id, step, what);
    }
    let (mine, walked) = both_bounds_of(cache, doc, &all);
    prop_assert_eq!(mine, walked, "all, step {} ({})", step, what);
    let want: usize = doc.layers.iter().map(|l| mirrored(l)).sum();
    prop_assert_eq!(cache.entries(), want, "entries, step {} ({})", step, what);
    Ok(())
}
