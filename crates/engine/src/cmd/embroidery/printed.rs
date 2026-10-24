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

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use vectorcraft_doc::{Document, Node, NodeId, NodeKind};

use crate::cmd::effectcmd::expand_node;
use crate::cmd::rasterfx::strip_raster;

pub(super) const RASTER_LEFT_OUT: &str = "shadows, glows, blurs and other raster effects are not stitched: the art under them is";
pub(super) const MASKS_LEFT_OUT: &str = "opacity masks are not stitched: the masked art is sewn whole";

/// The document as the walk sews it, the objects expanded in it (the person selects each of them,
/// whatever pieces it became), and what was left off.
pub(super) struct Sewable<'a> {
    pub(super) doc: Cow<'a, Document>,
    pub(super) expanded: HashSet<NodeId>,
    pub(super) warnings: Vec<&'static str>,
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

/// `doc` as the walk sews it ([`Sewable`]): borrowed as it is when nothing needs expanding.
pub(super) fn sewable(doc: &Document) -> Sewable<'_> {
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
        return Sewable { doc: Cow::Borrowed(doc), expanded: HashSet::new(), warnings };
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
    let mut out = Vec::new();
    for id in &found {
        // Stripped of raster effects, it may be basic now.
        if d.node(*id).is_some_and(needs_expanding) {
            expand_node(&mut d, *id, &brushes, &mut out);
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
                    expand_node(&mut d, *id, &brushes, &mut out);
                }
            }
        }
        let expanded_art = d.node(art.id).cloned();
        // The scratch layer must go, or its art would be sewn as the document's: failing that,
        // the document is sewn as it is.
        if d.remove(scratch).is_err() {
            return Sewable { doc: Cow::Borrowed(doc), expanded: HashSet::new(), warnings };
        }
        if let (true, Some(expanded_art), Some(sym)) = (placed, expanded_art, d.symbols.get_mut(k)) {
            sym.art = Arc::new(expanded_art);
            expanded.extend(ids);
        }
    }
    Sewable { doc: Cow::Owned(d), expanded, warnings }
}
