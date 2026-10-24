//! Objects made from others keep the embroidery settings of the objects they come from (the
//! project owner's choice, 2026-10-23: losing a person's settings silently is a kind of lost work).
//! Upstream's commands call these through one-line hooks (FORK.md › Keeping up with upstream):
//!
//! - one result from one source takes the settings of the object it takes its paint from, less
//!   its literal stitches (they are its path's own points): Pathfinder, Offset Path, Shape Builder,
//!   Divide Objects Below and Split Into Grid (through Pathfinder's style, [`carried`]), and Make
//!   Compound Path (from the backmost path, [`compound`]); so two touching shapes of one colour
//!   whose settings differ stay apart under Merge (their stroke settings left out with their
//!   strokes, [`strokes_removed`]); the cut after an object cut into several pieces
//!   goes to its last ([`cut_once`]);
//! - one object becoming several gives each the settings of the part it came from and the cut after
//!   the object to what stands for the object: Release Compound Path ([`released`]), Create
//!   Outlines ([`outlined_type`]), Outline Stroke and Expand Appearance ([`Expanding`], through
//!   [`expand_leaf`] for every path it expands, also what the walk sews an expanded appearance's
//!   pieces with; [`outlined`] for what it expands through its outlines), Ungroup ([`ungrouped`]:
//!   the group's cut to its last member the walk sews, so the cut stays after the same stitches)
//!   and Break Link ([`unlinked`]).

use std::sync::Arc;

use vectorcraft_doc::embroidery::{Embroidery, PartKind, PartSettings};
use vectorcraft_doc::{AppearanceItem, Document, Node, NodeKind};
use vectorcraft_render::effects::{self, StrokeArt};

/// The embroidery settings a new object made from `n` takes: `n`'s, less its literal stitches.
/// `None` when nothing is left.
pub(crate) fn carried(n: &Node) -> Option<Box<Embroidery>> {
    let mut e = n.extras().embroidery.as_deref()?.clone();
    e.stitches = None;
    (!e.is_empty()).then(|| Box::new(e))
}

/// Pieces cut from objects (Pathfinder's Divide, Trim, Merge, Crop, Outline and mixes, Split Into
/// Grid's cells, Divide Objects Below's pieces), in sewing order, each with its source's settings
/// ([`carried`]): of the pieces with the same settings (one object's, or several objects' that were
/// the same) only the last keeps the cut after the object, so that the thread is cut once, after
/// the last of them, as it was after the object.
pub(crate) fn cut_once<'a>(pieces: impl DoubleEndedIterator<Item = &'a mut Node>) {
    let mut cut: Vec<Embroidery> = Vec::new();
    for p in pieces.rev() {
        let Some(e) = p.extras().embroidery.as_deref().filter(|e| e.trim_after) else { continue };
        if !cut.contains(e) {
            cut.push(e.clone());
            continue;
        }
        let mut e = e.clone();
        e.trim_after = false;
        p.extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
    }
}

/// Trim, Merge and Crop of a shape whose settings are `e`: they remove its strokes, and its stroke
/// settings go with them, so that shapes of one colour whose settings differ only there merge.
pub(crate) fn strokes_removed(e: &mut Option<Box<Embroidery>>) {
    if let Some(b) = e {
        b.stroke = None;
        if b.is_empty() {
            *e = None;
        }
    }
}

/// Make Compound Path of `sources` (back to front): the settings of the path whose paint the
/// compound path takes, the backmost (inside a group, its backmost path).
pub(crate) fn compound<'a>(sources: impl IntoIterator<Item = &'a Node>) -> Option<Box<Embroidery>> {
    fn backmost(n: &Node) -> Option<&Node> {
        match &n.kind {
            NodeKind::Path { .. } | NodeKind::Compound { .. } => Some(n),
            _ => n.children()?.iter().find_map(|c| backmost(c)),
        }
    }
    sources.into_iter().find_map(backmost).and_then(carried)
}

/// The object-level settings of `e`, for what stands for an object made into several: the cut
/// after it, and what a newer version kept. `None` when nothing is left.
pub(crate) fn whole(e: &Embroidery) -> Option<Box<Embroidery>> {
    let stand_in = Embroidery { trim_after: e.trim_after, extra: e.extra.clone(), ..Embroidery::default() };
    (!stand_in.is_empty()).then(|| Box::new(stand_in))
}

/// The settings a piece made of part `part` of an object takes: that part's, as a fill (every
/// piece is filled art: a stroke's outline, a fill's copy). `None` when the part has none.
fn piece(part: Option<&PartSettings>) -> Option<Box<Embroidery>> {
    let part = part?.clone();
    Some(Box::new(Embroidery { fill: Some(Box::new(part)), ..Embroidery::default() }))
}

/// Release Compound Path of `n`: its parts (in stacking order) take its settings, each its own
/// copy, the last (topmost, sewn last) its cut too.
pub(crate) fn released(n: &Node, parts: &mut [Node]) {
    let Some(e) = carried(n) else { return };
    let last = parts.len().saturating_sub(1);
    for (k, p) in parts.iter_mut().enumerate() {
        let mut own = (*e).clone();
        own.trim_after = e.trim_after && k == last;
        p.extras_mut().embroidery = (!own.is_empty()).then(|| Box::new(own));
    }
}

/// Ungroup of `group`: the cut after the group goes to its last member the walk can sew (shown, a
/// path, compound path, group or symbol instance: not type, an image, a guide or a clipping path),
/// so that the cut stays after the same stitches (its other settings are about its own fills and
/// strokes, which ungrouping drops).
pub(crate) fn ungrouped(group: &Node, members: &mut [Node]) {
    let cut = group.extras().embroidery.as_deref().is_some_and(|e| e.trim_after);
    let sewn = |n: &Node| {
        n.visible
            && match &n.kind {
                NodeKind::Path { guide, clipping, .. } => !guide && !clipping,
                NodeKind::Compound { .. } | NodeKind::Group { .. } | NodeKind::SymbolInstance { .. } => true,
                _ => false,
            }
    };
    if let (true, Some(last)) = (cut, members.iter_mut().rev().find(|m| sewn(m))) {
        let mut e = last.extras().embroidery.as_deref().cloned().unwrap_or_default();
        e.set_trim_after(true);
        last.extras_mut().embroidery = Some(Box::new(e));
    }
}

/// Create Outlines of `text` into `group`: each glyph path takes the type's part settings (type
/// takes none from the commands until lettering sews it; a file may hold them), the group the cut.
pub(crate) fn outlined_type(text: &Node, group: &mut Node) {
    let Some(e) = text.extras().embroidery.as_deref() else { return };
    let parts = Embroidery { fill: e.fill.clone(), stroke: e.stroke.clone(), ..Embroidery::default() };
    if !parts.is_empty() {
        for c in group.children_mut().into_iter().flatten() {
            if matches!(c.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) {
                Arc::make_mut(c).extras_mut().embroidery = Some(Box::new(parts.clone()));
            }
        }
    }
    group.extras_mut().embroidery = whole(e);
}

/// Break Link of `instance` into `art`: the instance's cut goes to the art standing for it (the
/// art's own settings, set inside the symbol and turned by the instance, stay).
pub(crate) fn unlinked(instance: &Node, art: &mut Node) {
    if instance.extras().embroidery.as_deref().is_some_and(|e| e.trim_after) {
        let mut e = art.extras().embroidery.as_deref().cloned().unwrap_or_default();
        e.set_trim_after(true);
        art.extras_mut().embroidery = Some(Box::new(e));
    }
}

/// [`effects::expand_leaf`] of path or compound path `n`, its pieces sewn with the settings of the
/// parts they come from ([`Expanding`]). Every path and compound path Expand Appearance expands goes
/// through here, at any depth (a group's members, a symbol's art, a path's Transform copies), and
/// so does the walk's copy of the document as it prints.
pub(crate) fn expand_leaf(d: &mut Document, n: &Node, stroke_art: StrokeArt) -> Option<Node> {
    let expanding = Expanding::of(n);
    let mut m = effects::expand_leaf(d, n, stroke_art)?;
    expanding.apply(&mut m);
    Some(m)
}

/// [`effects::expand_art`] of `n` (art made for Expand Appearance: a group's own fills and strokes,
/// outlined type, a symbol's art), each path and compound path in it through [`expand_leaf`].
pub(crate) fn expand_art(d: &mut Document, n: &mut Node, stroke_art: StrokeArt) {
    if matches!(n.kind, NodeKind::Path { clipping: false, guide: false, .. } | NodeKind::Compound { .. }) {
        if let Some(m) = expand_leaf(d, n, stroke_art) {
            *n = m;
        }
        return;
    }
    for c in n.children_mut().into_iter().flatten() {
        expand_art(d, Arc::make_mut(c), stroke_art);
    }
}

/// What Expand Appearance made of `n` through its outlines (a symbol instance with an effect, type,
/// a live object, a path whose Transform effect copies it): `result` stands for `n` and takes the
/// cut after it (and what a newer version kept), so that the thread is cut once, after all of it.
/// A path's copies each held its settings, the cut too: it goes from them to `result`. A symbol's
/// art keeps the settings it has in the symbol.
pub(crate) fn outlined(n: &Node, result: &mut Node) {
    fn uncut(n: &mut Node) {
        if let Some(e) = n.extras().embroidery.as_deref().filter(|e| e.trim_after) {
            let mut e = e.clone();
            e.trim_after = false;
            n.extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
        }
        for c in n.children_mut().into_iter().flatten() {
            uncut(Arc::make_mut(c));
        }
    }
    let Some(e) = n.extras().embroidery.as_deref() else { return };
    if matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) {
        uncut(result);
    }
    let Some(stand_in) = whole(e) else { return };
    let mut own = result.extras().embroidery.as_deref().cloned().unwrap_or_default();
    own.trim_after |= stand_in.trim_after;
    for (k, v) in stand_in.extra {
        own.extra.entry(k).or_insert(v);
    }
    result.extras_mut().embroidery = Some(Box::new(own));
}

/// An object about to be expanded into pieces (Outline Stroke, Expand Appearance, the walk's copy
/// of the document as it prints), and what its pieces are sewn with afterwards
/// ([`Expanding::apply`]).
pub(crate) struct Expanding {
    /// Its embroidery; `None`: nothing to give its pieces.
    settings: Option<Box<Embroidery>>,
    /// Whether it is a path or compound path, whose pieces are its fills and strokes one by one.
    leaf: bool,
    /// Whether one of its fills paints (a fill's piece is always made, a stroke's may not be).
    fills: bool,
    /// Its bounding box's angle, which its pieces take: their unset fill angles turn as its did.
    angle: f64,
}

impl Expanding {
    /// Object `n`, before it is expanded.
    pub(crate) fn of(n: &Node) -> Self {
        let fills = n.appearance.items.iter().any(|i| matches!(i, AppearanceItem::Fill(f) if f.visible && !f.paint.is_none()));
        let settings = n.extras().embroidery.clone().filter(|e| e.stitches.is_none());
        Self { settings, leaf: matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }), fills, angle: n.bbox_angle }
    }

    /// `result`, what the object became, with each piece sewn with the settings of the part it
    /// came from and the cut after the object on what stands for it, so that the object is cut
    /// once, after its last piece. A fill's piece is a copy of the object (it holds the object's
    /// settings as they were); every other piece is a stroke's (its outline, or its brush's art,
    /// whose paths take the settings: the walk reads them from the paths it sews). A single piece
    /// in the object's place is a fill's when the object has a fill that paints. Every piece takes
    /// the object's bounding box's angle, so that a stroke's outline (a new path, upright) turns
    /// its unset rows as the object did.
    pub(crate) fn apply(&self, result: &mut Node) {
        if !self.leaf {
            return;
        }
        let e = self.settings.as_deref();
        let of = |fill: bool| e.and_then(|e| piece(e.part(if fill { PartKind::Fill } else { PartKind::Stroke })));
        result.bbox_angle = self.angle;
        if matches!(result.kind, NodeKind::Group { .. }) {
            for c in result.children_mut().into_iter().flatten() {
                let came_from_fill = e.is_some() && c.extras().embroidery.as_deref() == e;
                let settings = e.map(|_| of(came_from_fill));
                on_paths(Arc::make_mut(c), settings.as_ref(), self.angle);
            }
            if let Some(e) = e {
                result.extras_mut().embroidery = whole(e);
            }
        } else if let Some(e) = e {
            let mut own = of(self.fills).map(|b| *b).unwrap_or_default();
            own.trim_after = e.trim_after;
            own.extra = e.extra.clone();
            result.extras_mut().embroidery = (!own.is_empty()).then(|| Box::new(own));
        }
    }
}

/// `n` and everything in it turned to `angle` (the bounding box's), and `settings`, when given, on
/// every path and compound path in it (itself, when it is one), none on the groups between them.
fn on_paths(n: &mut Node, settings: Option<&Option<Box<Embroidery>>>, angle: f64) {
    n.bbox_angle = angle;
    if matches!(n.kind, NodeKind::Compound { .. }) || n.children().is_none() {
        if let Some(settings) = settings {
            n.extras_mut().embroidery = settings.clone();
        }
        return;
    }
    if settings.is_some() {
        n.extras_mut().embroidery = None;
    }
    for c in n.children_mut().into_iter().flatten() {
        on_paths(Arc::make_mut(c), settings, angle);
    }
}
