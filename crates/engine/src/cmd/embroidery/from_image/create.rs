//! Create: what digitizing made of an image, made into embroidery objects.
//!
//! One group goes where the image was (its parent, its place), named as the image is (unnamed when
//! it isn't, as upstream's Expand leaves a trace). In it, a group for each colour block in sewing
//! order, named after its thread as the thread libraries name swatches ("PEC 12 Orange"): the plan
//! sews in stacking order and joins one thread's objects only where nothing lies between them, so a
//! thread that comes back later in the order gets a group of its own. In each, one object per area
//! in sewing order, its outline and holes (a compound path for an area with holes), painted with
//! its thread's own colour, so that the plan sews it in exactly that thread.
//!
//! An area along whose middle a centre line runs (`super::centre`) is made a stroke along it, as
//! wide as the line, round at its turns, with no fill: the planner sews a stroke that narrow as one
//! running line through its junctions. Its ends are round on a line narrower than a satin column
//! and butt on a wider one, whose centre line ends half a thread short of the ink.
//!
//! An area's embroidery settings are set only where its colour's job departs from what it would
//! inherit: `fill` sets its fill's narrowAsSatin to none (tatami, its narrow parts too), `none` sets
//! its fill (a centre line's stroke) not to be sewn, and a centre line's stroke as wide as a satin
//! column (a thin line widened where its lines meet, or any line of a colour sewn as `lines`) is
//! set to a running line. Everything else stays unset (Auto, following the document's fabric).
//!
//! The background's areas become no object. The source image moves, with its id, to a new template
//! layer below every layer, hidden and locked, scaled as the design is so that it lies under it.
//!
//! Where the image's place isn't sewn (the plan walks no template, non-printing or hidden layer,
//! nor a hidden group), the design goes on top of the nearest layer above that is (visible,
//! printing, unlocked), or of a new layer on top when none is, and the image stays where it is,
//! scaled as the design is: a picture on its template layer stays there (the project owner's
//! decision, 2026-11-03), and so does the image Create moved to its own template layer, digitized
//! again.

use std::sync::Arc;

use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::appearance::{AppearanceItem, FillLayer};
use vectorcraft_doc::embroidery::{Embroidery, PartSettings};
use vectorcraft_doc::{Appearance, Document, LineCap, LineJoin, MAX_COORD, Node, NodeId, NodeKind};
use vectorcraft_geom::{PathData, Point};
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::Palette;

use super::super::digitize::{printed, shows};
use super::command::Digitized;
use super::settings::{C, Job};
use crate::cmd::bad;
use crate::cmd::buildcmds::TRACE_NAME;
use crate::cmd::pathops::shape_node;
use crate::{EngineError, Result};

/// The stitch id of a part not sewn ([`super::super::resolve::Stitch::None`]).
pub(super) const NOT_SEWN: &str = "none";

/// The stitch id of a part sewn as a running line ([`super::super::resolve::Stitch::IDS`]).
pub(super) const RUNNING: &str = "running";

/// How wide a stroke the planner sews as a satin column by default (millimetres).
const SATIN_MIN_WIDTH: f64 = vectorcraft_stitch::plan::SATIN_MIN_WIDTH;

/// The widest part of a colour sewn as fill that is sewn as a satin column (millimetres): none, the
/// least the setting takes (`vectorcraft_stitch::plan::NARROW_RANGE`).
pub(super) const NO_SATIN_MM: f64 = vectorcraft_stitch::plan::NARROW_RANGE.0;

/// What Create made: the design's group, the layer holding the image (the new template layer, or
/// the one it stayed on), and how many objects and colour blocks the design is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Made {
    pub(super) group: NodeId,
    pub(super) layer: NodeId,
    pub(super) objects: usize,
    pub(super) blocks: usize,
}

/// The settings an area of a colour sewn as `job` is given: none for Auto, nor for an area of a
/// colour sewn as Lines that has no centre line (a dot, which thins to no piece: sewn as the area
/// it is).
fn settings_for(job: Job) -> Option<Embroidery> {
    let fill = match job {
        Job::Auto | Job::Lines => return None,
        Job::Fill => PartSettings { narrow_as_satin: Some(NO_SATIN_MM), ..Default::default() },
        Job::None => PartSettings { stitch: Some(NOT_SEWN.into()), ..Default::default() },
    };
    Some(Embroidery { fill: Some(Box::new(fill)), ..Default::default() })
}

/// The settings a centre line's stroke `width_pt` points wide, of a colour sewn as `job`, is given:
/// not sewn for `none`; a running line where the stroke is as wide as a satin column
/// (`plan::SATIN_MIN_WIDTH`, which the planner sews a stroke that wide as by default: a thin line
/// whose junctions widen it, or any of a colour sewn as Lines); none for a narrower one, which the
/// planner sews as a running line.
fn line_settings_for(job: Job, width_pt: f64) -> Option<Embroidery> {
    let stitch = match job {
        Job::None => NOT_SEWN,
        Job::Auto | Job::Lines if vectorcraft_stitch::pt_to_mm(width_pt) >= SATIN_MIN_WIDTH => RUNNING,
        Job::Auto | Job::Lines | Job::Fill => return None,
    };
    Some(Embroidery { stroke: Some(Box::new(PartSettings { stitch: Some(stitch.into()), ..Default::default() })), ..Default::default() })
}

/// Points in a millimetre.
const PT_PER_MM: f64 = 1.0 / MM_PER_PT;

/// The width of the stroke made of centre line `line` (points) in a design `px_per_mm` pixels a
/// millimetre: the line's.
fn line_width(line: &super::centre::CentreLine, px_per_mm: f64) -> f64 {
    line.width_px / px_per_mm * PT_PER_MM
}

/// The object made of centre line `line` (working pixels, placed by `to_doc`), in `color`: a stroke
/// along it `width` points wide ([`line_width`]), round at its turns, and no fill. Its ends are round
/// on a line the planner sews as a running line by its width (narrower than a satin column), as a
/// thin line's are drawn; butt on a wider one, set to a running line, whose ends lie half a thread
/// short of the ink's (`super::centre`), so that the stroke drawn ends with the line.
fn line_node(doc: &mut Document, line: &super::centre::CentreLine, to_doc: vectorcraft_geom::Affine, width: f64, color: Color) -> Node {
    let path = PathData::from_bezpath(&(to_doc * line.path()));
    let mut look = Appearance::basic(Paint::None, Paint::solid(color), width);
    // Its stroke wherever the basic appearance holds it (upstream's own accessor, not its place).
    if let Some(s) = look.stroke_mut() {
        s.cap = if vectorcraft_stitch::pt_to_mm(width) >= SATIN_MIN_WIDTH { LineCap::Butt } else { LineCap::Round };
        s.join = LineJoin::Round;
    }
    Node::path(doc.alloc_id(), path, look)
}

/// The name of the template layer an image moves to, as File › Place names one: "Template" and
/// the image's `name`, else the name of the file it links to (`link`), else "Image".
fn template_name(name: Option<&str>, link: Option<&vectorcraft_doc::LinkInfo>) -> String {
    let file = link.and_then(|l| std::path::Path::new(&l.path).file_name()).and_then(|f| f.to_str());
    format!("Template {}", name.or(file).unwrap_or("Image"))
}

/// Whether the place of `target` is sewn: every layer and group it lies in shown, printed and
/// unlocked, as the plan walks the document (`super::super::digitize::{shows, printed}`).
fn sewn_place(doc: &Document, target: NodeId) -> bool {
    let Some(ancestry) = doc.ancestry(target) else { return false };
    let (_, around) = ancestry.split_last().unwrap_or((&target, &[]));
    around.iter().all(|&a| doc.node(a).is_some_and(|n| shows(n) && printed(n) && !n.locked))
}

/// The nearest top-level layer above `below` that is sewn and may take objects (shown, printed,
/// unlocked): where a design goes when its image's place isn't sewn.
fn ordinary_layer_above(doc: &Document, below: NodeId) -> Option<NodeId> {
    let layers = doc.children(None)?;
    let i = layers.iter().position(|l| l.id == below)?;
    layers.iter().skip(i + 1).find(|l| l.is_layer() && shows(l) && printed(l) && !l.locked).map(|l| l.id)
}

/// The objects digitizing made of `target` (an image, or an Image Trace object) as `d` says, in
/// `palette`'s threads, made in `doc` (this module's rules); `None`, and `doc` to be dropped, when
/// there is nothing to make (every area is the background's).
pub(super) fn made(doc: &mut Document, target: NodeId, d: &Digitized, palette: Palette) -> Result<Option<Made>> {
    let src = doc.node(target).cloned().ok_or(EngineError::NoNode(target))?;
    let traced = matches!(src.kind, NodeKind::Group { .. }) && src.name.as_deref() == Some(TRACE_NAME);
    let mut image = if traced { src.children().and_then(|c| c.first()).map(|c| (**c).clone()).ok_or(EngineError::NoNode(target))? } else { src };
    let NodeKind::Image(img) = &mut image.kind else {
        return Err(bad(C, "not an image: select an image, or an Image Trace object"));
    };
    img.xf = d.working.scaled * img.xf;
    // The image moved under the design stays on the canvas, as the design must: a small crop of a
    // large image made much larger would carry the rest of the image past it.
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let on_canvas = |p: Point| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD;
    if ![(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].into_iter().all(|(x, y)| on_canvas(img.xf * Point::new(x, y))) {
        return Err(bad(
            C,
            "the image, scaled as the design is, would reach past the canvas: give a smaller size, or crop the image first (Object › Crop Image)",
        ));
    }
    let layer_name = template_name(image.name.as_deref(), img.link.as_ref());
    image.visible = true;
    // The areas in sewing order, by colour block: a block's thread, whether it is sewn, its objects.
    let mut blocks: Vec<(Option<usize>, bool, Vec<Arc<Node>>)> = Vec::new();
    let mut objects = 0usize;
    for &a in &d.order.sewn {
        vectorcraft_doc::cancel::check()?;
        let (Some(area), Some(shape)) = (d.areas.areas.get(a), d.shapes.shapes.get(a)) else { continue };
        let Some(row) = d.colours.rows.get(area.row) else { continue };
        if shape.subpaths.is_empty() {
            continue;
        }
        let thread = row.thread.as_ref().map(|m| m.thread.index);
        let [r, g, b] = thread.and_then(|i| palette.get(i)).map_or(row.color, |(rgb, _)| rgb);
        let n = match d.lines.get(a).and_then(Option::as_ref) {
            Some(line) => {
                let width = line_width(line, d.working.px_per_mm);
                let mut n = line_node(doc, line, d.working.to_doc, width, Color::rgb8(r, g, b));
                if let Some(e) = line_settings_for(row.job, width) {
                    n.extras_mut().embroidery = Some(Box::new(e));
                }
                n
            }
            None => {
                let mut n = shape_node(doc, shape.transformed(d.working.to_doc), None);
                n.appearance =
                    Appearance { items: vec![AppearanceItem::Fill(FillLayer::new(Paint::solid(Color::rgb8(r, g, b))))], ..Default::default() }.into();
                if let Some(e) = settings_for(row.job) {
                    n.extras_mut().embroidery = Some(Box::new(e));
                }
                n
            }
        };
        objects += 1;
        match blocks.last_mut() {
            Some((t, sewn, list)) if *t == thread && *sewn == row.sewn() => list.push(Arc::new(n)),
            _ => blocks.push((thread, row.sewn(), vec![Arc::new(n)])),
        }
    }
    if objects == 0 {
        return Ok(None);
    }
    let count = blocks.len();
    let groups: Vec<Arc<Node>> = blocks
        .into_iter()
        .map(|(thread, _, children)| {
            let mut g = Node::group(doc.alloc_id(), children);
            g.name = thread.and_then(|i| palette.get(i).map(|(_, name)| super::super::threadlib::swatch_name(palette, i, name)));
            Arc::new(g)
        })
        .collect();
    let mut group = Node::group(doc.alloc_id(), groups);
    group.name = image.name.clone();
    let id = group.id;
    if !sewn_place(doc, target) {
        let below = doc.layer_of(target).ok_or(EngineError::NoNode(target))?;
        let layer = ordinary_layer_above(doc, below).unwrap_or_else(|| doc.add_layer(None));
        doc.insert(Some(layer), usize::MAX, group)?;
        let NodeKind::Image(scaled) = &image.kind else { return Err(bad(C, "not an image: select an image, or an Image Trace object")) };
        let xf = scaled.xf;
        if let Some(NodeKind::Image(img)) = doc.node_mut(image.id).map(|n| &mut n.kind) {
            img.xf = xf;
        }
        return Ok(Some(Made { group: id, layer: below, objects, blocks: count }));
    }
    let (parent, index, _) = doc.position(target).ok_or(EngineError::NoNode(target))?;
    doc.remove(target)?;
    doc.insert(parent, index, group)?;
    // The template layer, below every layer: locked and a template, as File › Place makes one, and
    // hidden, so that the design shows alone.
    let layer = doc.add_layer(Some(&layer_name));
    doc.move_node(layer, None, 0)?;
    if let Some(n) = doc.node_mut(layer) {
        (n.locked, n.visible) = (true, false);
        if let NodeKind::Layer { template, .. } = &mut n.kind {
            *template = true;
        }
    }
    doc.insert(Some(layer), 0, image)?;
    Ok(Some(Made { group: id, layer, objects, blocks: count }))
}
