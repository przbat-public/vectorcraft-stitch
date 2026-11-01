//! The document as it prints, for the walk to sew (the project owner's choice, 2026-10-22): an
//! object whose appearance is more than one fill and one stroke (fills and strokes of its own
//! opacity or blend, several of them, live effects, brushes, a group's own fills and strokes) is
//! expanded as `effect.expandAppearance` expands it, on a copy of the document: each fill and
//! stroke an object of its own (strokes outlined, brushed ones their brush art), geometry effects
//! baked. Symbols' art is expanded the same way. What still can't be sewn is said: raster effects
//! (shadows, glows, blurs) are left off, the art under them sewn; opacity masks are not sewn, the
//! masked art is, whole.
//!
//! A run of literal stitches (manual, or a machine file's) is sewn as its points whatever its
//! appearance: neither it nor a group holding one (or a symbol instance placing one) is expanded,
//! and what of its appearance isn't sewn is said ([`RUN_LOOK_LEFT_OUT`]), naming the objects.
//!
//! Dashed, aligned, profiled and arrowed strokes of an otherwise basic appearance are sewn as the
//! area they paint by the walk itself (`Choice::StrokeOutlined`), and a stained symbol instance's
//! art takes its stain there too.
//!
//! A stroke of an expanded object, or of an object in an expanded group, that nothing reshapes (no
//! brush, no effect of its own or of the object's) is not outlined: it becomes an object of its own, the object's path with that stroke
//! alone and the object's stroke settings ([`split_strokes`]), so that the walk sews it as it sews
//! a basic object's stroke: a satin column or a running line along its path, by its width or as
//! its settings ask, the area it paints for dashes, alignment, a width profile or arrowheads (the
//! project owner's choice, 2026-10-24: a stroke set to running sews one line along its path, not
//! two round its outline). A stroke a brush or an effect reshapes is outlined as before, and when a
//! running or satin stitch is set on it, or given to the export, the plan says that the stitch
//! can't follow its path.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind};
use vectorcraft_doc::{Appearance, AppearanceItem, Document, Node, NodeId, NodeKind};
use vectorcraft_stitch::plan::Style;

use super::resolve::Stitch;
use crate::cmd::effectcmd::expand_node;
use crate::cmd::rasterfx::strip_raster;

pub(super) const RASTER_LEFT_OUT: &str = "shadows, glows, blurs and other raster effects are not stitched: the art under them is";
pub(super) const MASKS_LEFT_OUT: &str = "opacity masks are not stitched: the masked art is sewn whole";
pub(super) const RUN_LOOK_LEFT_OUT: &str = "effects, brushes and extra fills or strokes on manual stitches or stitches from a file (or on a group holding them) are not stitched: the stitches are sewn as they are";
pub(super) const STROKE_LOSES_ITS_PATH: &str = "strokes drawn with a brush or reshaped by an effect are sewn as the area they paint: a running or satin stitch set on them can't follow their path";

/// The document as the walk sews it, the objects expanded in it (the person selects each of them,
/// whatever pieces it became), and what was left off or approximated, with the objects a warning
/// is about where it is about some (`about`).
pub(super) struct Sewable<'a> {
    pub(super) doc: Cow<'a, Document>,
    pub(super) expanded: HashSet<NodeId>,
    pub(super) warnings: Vec<&'static str>,
    pub(super) about: Vec<(&'static str, NodeId)>,
}

/// Whether `n` has an appearance the walk can't sew as it is: more than one fill and one stroke,
/// or any of their own opacity, blend or effects, live effects, a brush; for a group or layer, any
/// fill, stroke or effect of its own; for a symbol instance, effects. Type and images are left to
/// the walk (type is not sewn; an image is traced first). A run of literal stitches, or a group
/// holding one, is never expanded: it is sewn as its points ([`holds_run`]).
pub(super) fn needs_expanding(n: &Node) -> bool {
    shows_more_than_sewn(n) && !holds_run(n, &[])
}

/// Whether `n`'s appearance shows more than the walk sews as it is ([`needs_expanding`], runs
/// aside).
fn shows_more_than_sewn(n: &Node) -> bool {
    match &n.kind {
        NodeKind::Path { clipping: true, .. } | NodeKind::Path { guide: true, .. } => false,
        NodeKind::Path { .. } | NodeKind::Compound { .. } => !n.appearance.is_basic() || vectorcraft_brush::has_brush(n),
        NodeKind::Group { .. } | NodeKind::Layer { .. } => !n.appearance.items.is_empty() || !n.appearance.effects.is_empty(),
        NodeKind::SymbolInstance { .. } => !n.appearance.effects.is_empty(),
        _ => false,
    }
}

/// Whether `n` is, or holds, a run of literal stitches (a symbol instance through its symbol's art,
/// among `symbols`, one level deep: a symbol placed in a symbol's art isn't followed, so that no
/// symbol holding itself loops): expanding it would make its stitches art planned again.
fn holds_run(n: &Node, symbols: &[vectorcraft_doc::Symbol]) -> bool {
    let mut found = false;
    n.walk(&mut |c| match &c.kind {
        NodeKind::SymbolInstance { symbol, .. } => found |= symbols.iter().any(|s| &s.name == symbol && holds_run(&s.art, &[])),
        _ => found |= c.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()),
    });
    found
}

/// Whether what `n` (a run or a group holding one) shows past what is sewn is more than raster
/// effects, which are said apart ([`RASTER_LEFT_OUT`]).
fn look_left_out(n: &Node) -> bool {
    let mut bare = n.clone();
    strip_raster(&mut bare, false);
    shows_more_than_sewn(&bare)
}

/// Whether `n` or anything in it has raster effects.
fn has_raster(n: &Node) -> bool {
    let raster = |e: &vectorcraft_doc::Effect| vectorcraft_render::effects::is_raster(&e.id);
    n.appearance.effects.iter().any(raster)
        || n.appearance.items.iter().any(|i| i.effects().iter().any(raster))
        || n.children().is_some_and(|c| c.iter().any(|c| has_raster(c)))
}

/// What the walk finds to expand under an object: the outermost objects to expand, the runs (and
/// groups holding them) whose appearance isn't sewn, and whether raster effects and opacity masks
/// are left out.
#[derive(Default)]
struct Found {
    expand: Vec<NodeId>,
    runs: Vec<NodeId>,
    raster: bool,
    masks: bool,
}

/// What `n` (itself included) holds to expand ([`Found`]), a symbol instance's art among `symbols`.
fn targets(n: &Node, symbols: &[vectorcraft_doc::Symbol], found: &mut Found) {
    if !n.visible {
        return;
    }
    found.masks |= n.extras().mask.is_some();
    found.raster |= has_raster(n);
    if shows_more_than_sewn(n) && !matches!(n.kind, NodeKind::Layer { .. }) {
        if !holds_run(n, symbols) {
            found.expand.push(n.id);
            return;
        }
        // Sewn as its points: what its appearance adds is said; the art beside a run in a group
        // is expanded as it needs.
        if look_left_out(n) {
            found.runs.push(n.id);
        }
    }
    for c in n.children().into_iter().flatten() {
        targets(c, symbols, found);
    }
}

/// Whether stroke `item` of an object of appearance `a` keeps its path when the object is expanded
/// for sewing: nothing it is drawn with reshapes it beyond what the walk draws itself (dashes,
/// alignment, a width profile, arrowheads: the area they paint, as on a basic object). A brush, or
/// an effect of its own or of the object's, reshapes it.
pub(super) fn keeps_its_path(a: &Appearance, item: &AppearanceItem) -> bool {
    a.effects.is_empty()
        && a.contents_index.is_none()
        && item.visible()
        && item.effects().is_empty()
        && matches!(item, AppearanceItem::Stroke(s) if s.brush.is_none())
}

/// Whether the stitch a stroke is sewn with follows its path: a running line or a satin column set
/// on the object's strokes, else given to the export (`forced`).
fn follows_its_path(n: &Node, forced: Option<Style>) -> bool {
    let set = n.extras().embroidery.as_deref().and_then(|e| e.part(PartKind::Stroke)).and_then(|p| p.stitch.as_deref()).and_then(Stitch::from_id);
    matches!(super::resolve::forcing(set, forced), Some((Style::Running | Style::Satin, _)))
}

/// `n` with its strokes that keep their path ([`keeps_its_path`]) made objects of their own: a
/// group in its place (its id, its cut after it), holding in appearance order a copy of `n` with
/// each such stroke alone and its stroke settings, and between them copies of `n` with the other
/// items, each run of them in one, which still expand if they need to. `None` when no stroke of
/// `n` keeps its path, or `n` is no path: it expands whole, as `effect.expandAppearance` expands
/// it.
fn split_strokes(d: &mut Document, n: &Node) -> Option<Node> {
    if !matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) || !n.appearance.items.iter().any(|i| keeps_its_path(&n.appearance, i)) {
        return None;
    }
    let e = n.extras().embroidery.as_deref();
    // The cut after the object goes after its last piece: on the group. The stroke's points go with
    // the strokes that keep their path alone.
    let rest = e
        .map(|e| {
            let mut rest = Embroidery { trim_after: false, stop_after: false, ..e.clone() };
            rest.edit_part(PartKind::Stroke, |part| {
                part.set(PartKey::Entry, None);
                part.set(PartKey::Exit, None);
            });
            rest
        })
        .filter(|e| !e.is_empty())
        .map(Box::new);
    let stroke = e.and_then(|e| e.stroke.clone()).map(|stroke| Box::new(Embroidery { stroke: Some(stroke), ..Embroidery::default() }));
    let (mut children, mut run): (Vec<(Node, bool)>, Vec<AppearanceItem>) = (Vec::new(), Vec::new());
    let flush = |d: &mut Document, run: &mut Vec<AppearanceItem>, children: &mut Vec<(Node, bool)>| {
        if !run.is_empty() {
            children.push((copy_with(d, n, std::mem::take(run), rest.clone()), false));
        }
    };
    for item in &n.appearance.items {
        if keeps_its_path(&n.appearance, item) {
            flush(d, &mut run, &mut children);
            children.push((copy_with(d, n, vec![item.clone()], stroke.clone()), true));
        } else {
            run.push(item.clone());
        }
    }
    flush(d, &mut run, &mut children);
    // Each part's points on one of the copies that hold them (they lie on one another: the first
    // the entry, the last the exit).
    for (strokes, kinds) in [(true, &[PartKind::Stroke][..]), (false, &[PartKind::Fill, PartKind::Stroke][..])] {
        let mut copies: Vec<&mut Node> = children.iter_mut().filter(|(_, s)| *s == strokes).map(|(c, _)| c).collect();
        for &kind in kinds {
            super::derive::points_on_nearest(&mut copies, kind);
        }
    }
    let children: Vec<Arc<Node>> = children.into_iter().map(|(c, _)| Arc::new(c)).collect();
    let mut group = n.clone();
    group.kind = NodeKind::Group { children, clip: false };
    *group.appearance = Appearance::default();
    group.extras_mut().embroidery = e.and_then(super::derive::whole);
    Some(group)
}

/// `n` with [`split_strokes`] done to it and to every path in it (a group's members) that the walk
/// expands, `None` when none of them changed. Sets `lost` when one of them has a stroke a brush or
/// an effect of its own reshapes whose stitch would follow its path (`forced`: the export's style).
/// A group's own effects reshape its members' paths, and their strokes follow the reshaped paths,
/// as a basic member's do.
fn split_inside(d: &mut Document, n: &Node, forced: Option<Style>, lost: &mut bool) -> Option<Node> {
    match &n.kind {
        NodeKind::Path { .. } | NodeKind::Compound { .. } if needs_expanding(n) => {
            let reshaped =
                n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Stroke(_)) && i.visible() && !keeps_its_path(&n.appearance, i));
            *lost |= reshaped && follows_its_path(n, forced);
            split_strokes(d, n)
        }
        NodeKind::Group { children, .. } | NodeKind::Layer { children, .. } => {
            let mut split = None;
            for (k, c) in children.iter().enumerate() {
                if let Some(c) = split_inside(d, c, forced, lost) {
                    let m: &mut Node = split.get_or_insert_with(|| n.clone());
                    if let Some(slot) = m.children_mut().and_then(|ch| ch.get_mut(k)) {
                        *slot = Arc::new(c);
                    }
                }
            }
            split
        }
        _ => None,
    }
}

/// A copy of `n` with appearance items `items` and embroidery settings `embroidery`, its ids (its
/// members' too) fresh in `d`.
fn copy_with(d: &mut Document, n: &Node, items: Vec<AppearanceItem>, embroidery: Option<Box<Embroidery>>) -> Node {
    fn fresh(d: &mut Document, n: &mut Node) {
        n.id = d.alloc_id();
        for c in n.children_mut().into_iter().flatten() {
            fresh(d, Arc::make_mut(c));
        }
    }
    let mut c = n.clone();
    c.appearance.items = items;
    c.extras_mut().embroidery = embroidery;
    fresh(d, &mut c);
    c
}

/// Expand object `id` of `d` for sewing: the strokes that keep their path of every path in it made
/// objects of their own ([`split_inside`]) and the rest expanded as `effect.expandAppearance`
/// expands it. A stroke reshaped by a brush or an effect whose stitch would follow its path
/// (`forced`: the export's style) is said, about `object` (the object a person selects, when it is
/// in the document).
fn expand_for_sewing(
    d: &mut Document,
    id: NodeId,
    object: Option<NodeId>,
    forced: Option<Style>,
    brushes: &[vectorcraft_brush::Brush],
    about: &mut Vec<(&'static str, NodeId)>,
    warnings: &mut Vec<&'static str>,
) {
    let Some(n) = d.node(id).cloned() else { return };
    let mut lost = false;
    if let Some(split) = split_inside(d, &n, forced, &mut lost)
        && let Some(slot) = d.node_mut(id)
    {
        *slot = split;
    }
    if lost {
        if !warnings.contains(&STROKE_LOSES_ITS_PATH) {
            warnings.push(STROKE_LOSES_ITS_PATH);
        }
        if let Some(object) = object {
            about.push((STROKE_LOSES_ITS_PATH, object));
        }
    }
    expand_node(d, id, brushes, &mut Vec::new());
}

/// `doc` as the walk sews it ([`Sewable`]), with `forced` the style the export forces: borrowed as
/// it is when nothing needs expanding.
pub(super) fn sewable(doc: &Document, forced: Option<Style>) -> Sewable<'_> {
    let mut in_document = Found::default();
    for layer in &doc.layers {
        targets(layer, &doc.symbols, &mut in_document);
    }
    // Symbols' art, by symbol: a run in it is said, naming none (its instances select it).
    let (mut raster, mut masks, mut runs) = (in_document.raster, in_document.masks, !in_document.runs.is_empty());
    let mut in_symbols: Vec<(usize, Vec<NodeId>)> = Vec::new();
    for (k, sym) in doc.symbols.iter().enumerate() {
        let mut art = Found::default();
        targets(&sym.art, &doc.symbols, &mut art);
        (raster, masks, runs) = (raster || art.raster, masks || art.masks, runs || !art.runs.is_empty());
        if !art.expand.is_empty() {
            in_symbols.push((k, art.expand));
        }
    }
    let found = in_document.expand;
    let mut warnings = Vec::new();
    if raster {
        warnings.push(RASTER_LEFT_OUT);
    }
    if masks {
        warnings.push(MASKS_LEFT_OUT);
    }
    let mut about: Vec<(&'static str, NodeId)> = Vec::new();
    if runs {
        warnings.push(RUN_LOOK_LEFT_OUT);
        about.extend(in_document.runs.iter().map(|id| (RUN_LOOK_LEFT_OUT, *id)));
    }
    if found.is_empty() && in_symbols.is_empty() && !raster {
        return Sewable { doc: Cow::Borrowed(doc), expanded: HashSet::new(), warnings, about };
    }
    let mut d = doc.clone();
    let brushes = vectorcraft_brush::library(&d);
    // Raster effects are left off everywhere: rendering them into images would only be traced.
    for layer in &mut d.layers {
        strip_raster(Arc::make_mut(layer), true);
    }
    for sym in &mut d.symbols {
        strip_raster(Arc::make_mut(&mut sym.art), true);
    }
    for id in &found {
        // Stripped of raster effects, it may be basic now.
        if d.node(*id).is_some_and(needs_expanding) {
            expand_for_sewing(&mut d, *id, Some(*id), forced, &brushes, &mut about, &mut warnings);
        }
    }
    let mut expanded: HashSet<NodeId> = found.into_iter().collect();
    // A symbol's art is expanded in a layer of its own, then given back to the symbol.
    for (k, ids) in in_symbols {
        let Some(art) = d.symbols.get(k).map(|s| s.art.clone()) else { continue };
        let scratch = d.add_layer(Some("Symbol art"));
        let placed = d.insert(Some(scratch), 0, (*art).clone()).is_ok();
        if placed {
            for id in &ids {
                if d.node(*id).is_some_and(needs_expanding) {
                    // Art inside a symbol is selected by its instances: the warning names none.
                    expand_for_sewing(&mut d, *id, None, forced, &brushes, &mut about, &mut warnings);
                }
            }
        }
        let expanded_art = d.node(art.id).cloned();
        // The scratch layer must go, or its art would be sewn as the document's: failing that,
        // the document is sewn as it is.
        if d.remove(scratch).is_err() {
            return Sewable { doc: Cow::Borrowed(doc), expanded: HashSet::new(), warnings, about };
        }
        if let (true, Some(expanded_art), Some(sym)) = (placed, expanded_art, d.symbols.get_mut(k)) {
            sym.art = Arc::new(expanded_art);
            expanded.extend(ids);
        }
    }
    Sewable { doc: Cow::Owned(d), expanded, warnings, about }
}
