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
//!
//! A path whose anchors all move at once (Direct Selection) takes its settings along, as an object
//! transformed whole does ([`anchors_transformed`]).
//!
//! Of the pieces one object becomes, each entry and exit point stays only on the piece nearest it,
//! the others' points set back to Auto ([`points_on_nearest`], the project owner's choice,
//! 2026-10-25): pieces cut apart keep a point on the piece it lies on; the pieces an appearance is
//! expanded to lie on one another, so the first takes the entry and the last the exit.

use std::collections::BTreeSet;
use std::sync::Arc;

use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartSettings};
use vectorcraft_doc::selection::AnchorRef;
use vectorcraft_doc::{AppearanceItem, Document, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point};
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
/// the last of them, as it was after the object; and each entry and exit point only the piece
/// nearest it ([`points_on_nearest`]).
pub(crate) fn cut_once<'a>(pieces: impl DoubleEndedIterator<Item = &'a mut Node>) {
    let mut pieces: Vec<&mut Node> = pieces.collect();
    // The pieces with the same settings (cut from one object, or from several that were the same),
    // in sewing order.
    let mut groups: Vec<(Embroidery, Vec<usize>)> = Vec::new();
    for (k, p) in pieces.iter().enumerate() {
        let Some(e) = p.extras().embroidery.as_deref() else { continue };
        match groups.iter_mut().find(|(g, _)| g == e) {
            Some((_, members)) => members.push(k),
            None => groups.push((e.clone(), vec![k])),
        }
    }
    for (e, members) in &groups {
        // Only the last keeps the cut.
        if e.trim_after
            && let Some((_, before)) = members.split_last()
        {
            for &k in before {
                let Some(p) = pieces.get_mut(k) else { continue };
                let mut e = e.clone();
                e.trim_after = false;
                p.extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
            }
        }
        // Each point on the piece nearest it.
        let mut group: Vec<&mut Node> = pieces.iter_mut().enumerate().filter(|(k, _)| members.contains(k)).map(|(_, p)| &mut **p).collect();
        for kind in [PartKind::Fill, PartKind::Stroke] {
            points_on_nearest(&mut group, kind);
        }
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
/// copy, the last (topmost, sewn last) its cut too, each entry and exit point only the part nearest
/// it ([`points_on_nearest`]).
pub(crate) fn released(n: &Node, parts: &mut [Node]) {
    let Some(e) = carried(n) else { return };
    let last = parts.len().saturating_sub(1);
    for (k, p) in parts.iter_mut().enumerate() {
        let mut own = (*e).clone();
        own.trim_after = e.trim_after && k == last;
        p.extras_mut().embroidery = (!own.is_empty()).then(|| Box::new(own));
    }
    let mut all: Vec<&mut Node> = parts.iter_mut().collect();
    for kind in [PartKind::Fill, PartKind::Stroke] {
        points_on_nearest(&mut all, kind);
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
        let mut glyphs: Vec<&mut Node> = Vec::new();
        for c in group.children_mut().into_iter().flatten() {
            if matches!(c.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) {
                let glyph = Arc::make_mut(c);
                glyph.extras_mut().embroidery = Some(Box::new(parts.clone()));
                glyphs.push(glyph);
            }
        }
        for kind in [PartKind::Fill, PartKind::Stroke] {
            points_on_nearest(&mut glyphs, kind);
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
    if own.extra.is_empty() {
        own.extra = stand_in.extra;
    } else if stand_in.extra.keys().any(|k| !own.extra.contains_key(k)) {
        let kept = Arc::make_mut(&mut own.extra);
        for (k, v) in stand_in.extra.iter() {
            kept.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    result.extras_mut().embroidery = Some(Box::new(own));
}

/// Direct Selection's transform `xf` of anchors `set` of path `id`: when they are every anchor of
/// it, the object moved (or turned, or scaled) as a whole, and its embroidery settings (its entry
/// and exit points, a set angle) go with it, as they do when it is transformed as an object.
pub(crate) fn anchors_transformed(d: &mut Document, id: NodeId, set: &BTreeSet<AnchorRef>, xf: Affine) {
    let Some(n) = d.node(id) else { return };
    let (Some(_), Some(path)) = (n.extras().embroidery.as_deref(), n.path_data()) else { return };
    let every = path.subpaths.iter().enumerate().all(|(si, sp)| (0..sp.anchors.len()).all(|ai| set.contains(&(si, ai))));
    if every
        && let Some(n) = d.node_mut(id)
        && let Some(e) = n.extras_mut().embroidery.as_deref_mut()
    {
        e.transform(xf);
    }
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
            // The pieces of each part keep each of its points on one of them (they lie on one
            // another: the first its entry, the last its exit).
            if e.is_some() {
                for source in [of(true), of(false)].into_iter().flatten() {
                    let mut pieces: Vec<&mut Node> = Vec::new();
                    leaves(result, &mut pieces);
                    let mut of_it: Vec<&mut Node> = pieces.into_iter().filter(|n| n.extras().embroidery.as_deref() == Some(&*source)).collect();
                    points_on_nearest(&mut of_it, PartKind::Fill);
                }
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

/// The paths and compound paths in `n` (itself, when it is one), in stacking order.
fn leaves<'a>(n: &'a mut Node, out: &mut Vec<&'a mut Node>) {
    if matches!(n.kind, NodeKind::Compound { .. }) || n.children().is_none() {
        if matches!(n.kind, NodeKind::Path { .. } | NodeKind::Compound { .. }) {
            out.push(n);
        }
        return;
    }
    for c in n.children_mut().into_iter().flatten() {
        leaves(Arc::make_mut(c), out);
    }
}

/// Of `pieces` (in sewing order), those holding the same entry and exit points of part `kind` as
/// the first (copies of one part's settings) keep each point only on the piece whose outline lies
/// nearest it, the others' set back to Auto: of equally near pieces (pieces lying on one another,
/// as an appearance expands), the first keeps the entry and the last the exit, so that a part sewn
/// as several units starts at its entry and ends at its exit once.
pub(crate) fn points_on_nearest(pieces: &mut [&mut Node], kind: PartKind) {
    let points = |n: &Node| n.extras().embroidery.as_deref().and_then(|e| e.part(kind)).and_then(|p| p.points.as_ref()).map(|p| (p.entry, p.exit));
    let Some(held @ (entry, exit)) = pieces.first().and_then(|n| points(n)) else { return };
    if entry.is_none() && exit.is_none() {
        return;
    }
    let same: Vec<usize> = (0..pieces.len()).filter(|&k| pieces.get(k).and_then(|n| points(n)) == Some(held)).collect();
    if same.len() < 2 {
        return;
    }
    let outlines: Vec<Vec<Vec<Point>>> = same.iter().map(|&k| pieces.get(k).map(|n| polylines(n)).unwrap_or_default()).collect();
    // The piece nearest `q`, by its place among `same`: the first or (`last`) the last of equally
    // near ones.
    let nearest = |q: Point, last: bool| -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, lines) in outlines.iter().enumerate() {
            let d = lines.iter().map(|l| distance_to(l, q)).fold(f64::INFINITY, f64::min);
            if best.is_none_or(|(_, b)| d < b || (last && d <= b)) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    };
    let (keep_entry, keep_exit) = (entry.and_then(|q| nearest(q, false)), exit.and_then(|q| nearest(q, true)));
    for (i, &k) in same.iter().enumerate() {
        let Some(n) = pieces.get_mut(k) else { continue };
        let Some(mut e) = n.extras().embroidery.as_deref().cloned() else { continue };
        e.edit_part(kind, |part| {
            if keep_entry != Some(i) {
                part.set(PartKey::Entry, None);
            }
            if keep_exit != Some(i) {
                part.set(PartKey::Exit, None);
            }
        });
        n.extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
    }
}

/// How finely a piece's outline is flattened to measure how far a point lies from it, document
/// points: a hundredth of a point, far below a needle's width.
const NEAREST_FLATTEN: f64 = 0.01;

/// `n`'s outline (a path's, or a compound path's members'), flattened, document points.
fn polylines(n: &Node) -> Vec<Vec<Point>> {
    let mut path = vectorcraft_geom::BezPath::new();
    match &n.kind {
        NodeKind::Path { path: p, .. } => path.extend(p.to_bezpath()),
        NodeKind::Compound { children, .. } => {
            for c in children {
                if let Some(p) = c.path_data() {
                    path.extend(p.to_bezpath());
                }
            }
        }
        _ => {}
    }
    let mut out: Vec<Vec<Point>> = Vec::new();
    vectorcraft_geom::kurbo::flatten(path.iter(), NEAREST_FLATTEN, |el| match el {
        vectorcraft_geom::PathEl::MoveTo(p) => out.push(vec![p]),
        vectorcraft_geom::PathEl::LineTo(p) => {
            if let Some(line) = out.last_mut() {
                line.push(p);
            }
        }
        vectorcraft_geom::PathEl::ClosePath => {
            if let Some(first) = out.last().and_then(|l| l.first().copied())
                && let Some(line) = out.last_mut()
            {
                line.push(first);
            }
        }
        _ => {}
    });
    out
}

/// How far `q` lies from polyline `line` (infinitely far from none).
fn distance_to(line: &[Point], q: Point) -> f64 {
    match line {
        [] => f64::INFINITY,
        [only] => only.distance(q),
        _ => line
            .windows(2)
            .filter_map(|w| {
                let [a, b] = w else { return None };
                let ab = *b - *a;
                let len2 = ab.hypot2();
                let t = if len2 > 0.0 { ((q - *a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
                Some((*a + ab * t).distance(q))
            })
            .fold(f64::INFINITY, f64::min),
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
