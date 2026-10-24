//! The document as it prints, for the walk to sew (the project owner's choice, 2026-10-22): an
//! object whose appearance is more than one fill and one stroke (fills and strokes of its own
//! opacity or blend, several of them, live effects, brushes, a group's own fills and strokes) is
//! expanded as `effect.expandAppearance` expands it, on a copy of the document: each fill and
//! stroke an object of its own (strokes outlined, brushed ones their brush art), geometry effects
//! baked. Symbols' art is expanded the same way. What still can't be sewn is said: raster effects
//! (shadows, glows, blurs) are left off, the art under them sewn; opacity masks are not sewn, the
//! masked art is, whole.
//!
//! Dashed, aligned, profiled and arrowed strokes of an otherwise basic appearance are sewn as the
//! area they paint by the walk itself (`Choice::StrokeOutlined`), and a stained symbol instance's
//! art takes its stain there too.
//!
//! A stroke of an expanded object that nothing reshapes (no brush, no effect of its own or of the
//! object's) is not outlined: it becomes an object of its own, the object's path with that stroke
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

use vectorcraft_doc::embroidery::{Embroidery, PartKind};
use vectorcraft_doc::{Appearance, AppearanceItem, Document, Node, NodeId, NodeKind};
use vectorcraft_stitch::plan::Style;

use super::resolve::Stitch;
use crate::cmd::effectcmd::expand_node;
use crate::cmd::rasterfx::strip_raster;

pub(super) const RASTER_LEFT_OUT: &str = "shadows, glows, blurs and other raster effects are not stitched: the art under them is";
pub(super) const MASKS_LEFT_OUT: &str = "opacity masks are not stitched: the masked art is sewn whole";
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
/// the walk (type is not sewn; an image is traced first).
pub(super) fn needs_expanding(n: &Node) -> bool {
    match &n.kind {
        NodeKind::Path { clipping: true, .. } | NodeKind::Path { guide: true, .. } => false,
        NodeKind::Path { .. } | NodeKind::Compound { .. } => !n.appearance.is_basic() || vectorcraft_brush::has_brush(n),
        NodeKind::Group { .. } | NodeKind::Layer { .. } => !n.appearance.items.is_empty() || !n.appearance.effects.is_empty(),
        NodeKind::SymbolInstance { .. } => !n.appearance.effects.is_empty(),
        _ => false,
    }
}

/// Whether `n` or anything in it has raster effects.
fn has_raster(n: &Node) -> bool {
    let raster = |e: &vectorcraft_doc::Effect| vectorcraft_render::effects::is_raster(&e.id);
    n.appearance.effects.iter().any(raster)
        || n.appearance.items.iter().any(|i| i.effects().iter().any(raster))
        || n.children().is_some_and(|c| c.iter().any(|c| has_raster(c)))
}

/// The ids of the outermost objects under `n` (itself included) to expand, its raster effects
/// noted.
fn targets(n: &Node, out: &mut Vec<NodeId>, raster: &mut bool, masks: &mut bool) {
    if !n.visible {
        return;
    }
    *masks |= n.extras().mask.is_some();
    *raster |= has_raster(n);
    if needs_expanding(n) && !matches!(n.kind, NodeKind::Layer { .. }) {
        out.push(n.id);
        return;
    }
    for c in n.children().into_iter().flatten() {
        targets(c, out, raster, masks);
    }
}

/// Whether stroke `item` of an object of appearance `a` keeps its path when the object is expanded
/// for sewing: nothing it is drawn with reshapes it beyond what the walk draws itself (dashes,
/// alignment, a width profile, arrowheads: the area they paint, as on a basic object). A brush, or
/// an effect of its own or of the object's, reshapes it.
fn keeps_its_path(a: &Appearance, item: &AppearanceItem) -> bool {
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
    let style = match set {
        Some(Stitch::Style(style)) => Some(style),
        Some(Stitch::Auto | Stitch::None) => None,
        None => forced,
    };
    matches!(style, Some(Style::Running | Style::Satin))
}

/// `n` with its strokes that keep their path ([`keeps_its_path`]) made objects of their own: a
/// group in its place (its id, its cut after it), holding in appearance order a copy of `n` with
/// each such stroke alone and its stroke settings, and between them copies of `n` with the other
/// items, each run of them in one, which still expand if they need to (their ids, the second of the
/// pair). `None` when no stroke of `n` keeps its path, or `n` is no path: it expands whole, as
/// `effect.expandAppearance` expands it.
fn split_strokes(d: &mut Document, n: &Node) -> Option<(Node, Vec<NodeId>)> {
    if !matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) || !n.appearance.items.iter().any(|i| keeps_its_path(&n.appearance, i)) {
        return None;
    }
    let e = n.extras().embroidery.as_deref();
    // The cut after the object goes after its last piece: on the group.
    let rest = e.map(|e| Embroidery { trim_after: false, ..e.clone() }).filter(|e| !e.is_empty()).map(Box::new);
    let stroke = e.and_then(|e| e.stroke.clone()).map(|stroke| Box::new(Embroidery { stroke: Some(stroke), ..Embroidery::default() }));
    let (mut children, mut expand, mut run) = (Vec::new(), Vec::new(), Vec::new());
    let mut flush = |d: &mut Document, run: &mut Vec<AppearanceItem>, children: &mut Vec<Arc<Node>>| {
        if run.is_empty() {
            return;
        }
        let c = copy_with(d, n, std::mem::take(run), rest.clone());
        if needs_expanding(&c) {
            expand.push(c.id);
        }
        children.push(Arc::new(c));
    };
    for item in &n.appearance.items {
        if keeps_its_path(&n.appearance, item) {
            flush(d, &mut run, &mut children);
            children.push(Arc::new(copy_with(d, n, vec![item.clone()], stroke.clone())));
        } else {
            run.push(item.clone());
        }
    }
    flush(d, &mut run, &mut children);
    let mut group = n.clone();
    group.kind = NodeKind::Group { children, clip: false };
    *group.appearance = Appearance::default();
    group.extras_mut().embroidery = e.and_then(super::derive::whole);
    Some((group, expand))
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

/// Expand object `id` of `d` for sewing: its strokes that keep their path made objects of their own
/// ([`split_strokes`]) and the rest expanded as `effect.expandAppearance` expands it. A stroke
/// reshaped by a brush or an effect whose stitch would follow its path (`forced`: the export's
/// style) is said, about `object` (the object a person selects, when it is in the document).
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
    let reshaped = n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Stroke(_)) && i.visible() && !keeps_its_path(&n.appearance, i));
    if reshaped && matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) && follows_its_path(&n, forced) {
        if !warnings.contains(&STROKE_LOSES_ITS_PATH) {
            warnings.push(STROKE_LOSES_ITS_PATH);
        }
        if let Some(object) = object {
            about.push((STROKE_LOSES_ITS_PATH, object));
        }
    }
    let mut out = Vec::new();
    match split_strokes(d, &n) {
        Some((group, expand)) => {
            if let Some(slot) = d.node_mut(id) {
                *slot = group;
            }
            for c in expand {
                expand_node(d, c, brushes, &mut out);
            }
        }
        None => expand_node(d, id, brushes, &mut out),
    }
}

/// `doc` as the walk sews it ([`Sewable`]), with `forced` the style the export forces: borrowed as
/// it is when nothing needs expanding.
pub(super) fn sewable(doc: &Document, forced: Option<Style>) -> Sewable<'_> {
    let (mut found, mut raster, mut masks) = (Vec::new(), false, false);
    for layer in &doc.layers {
        targets(layer, &mut found, &mut raster, &mut masks);
    }
    // Symbols' art, by symbol.
    let mut in_symbols: Vec<(usize, Vec<NodeId>)> = Vec::new();
    for (k, sym) in doc.symbols.iter().enumerate() {
        let mut ids = Vec::new();
        targets(&sym.art, &mut ids, &mut raster, &mut masks);
        if !ids.is_empty() {
            in_symbols.push((k, ids));
        }
    }
    let mut warnings = Vec::new();
    if raster {
        warnings.push(RASTER_LEFT_OUT);
    }
    if masks {
        warnings.push(MASKS_LEFT_OUT);
    }
    if found.is_empty() && in_symbols.is_empty() && !raster {
        return Sewable { doc: Cow::Borrowed(doc), expanded: HashSet::new(), warnings, about: Vec::new() };
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
    let mut about = Vec::new();
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
