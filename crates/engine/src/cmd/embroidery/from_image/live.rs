//! A live object of a digitized image (`live: true`, Keep as Live Object): an upstream Image Trace
//! object, so that its View, Expand and Release and the bounds rule keep working, holding the
//! source image hidden first and the objects Create makes after it, with the settings it was made
//! with in its embroidery's `digitizing` (research note 07, §3.3; the project owner's decisions of
//! 2026-11-16).
//!
//! - **Made** from Create's plain design: the image (which Create moved to a template layer, or left
//!   where it was) goes into the design's group, hidden; the group is named as Image Trace names
//!   its objects and given a trace value; the template layer Create added goes.
//! - **Digitized again** in place: the live object keeps its id and what was set on it (its
//!   visibility, lock, look, Trim After and Stop After); its objects are new.
//! - **What it keeps** is in the source image's frame: the settings by their file keys, the size
//!   aside (the object's size is its image's, as placed, so that a live object scaled on the canvas
//!   is digitized again at its size); a change's angle turned into the image as the page shows it
//!   then, and back. A fingerprint of its objects, taken in the image's frame, tells them edited by
//!   hand since (moving, turning or scaling the object edits nothing); its size then tells it
//!   scaled since.

use std::sync::Arc;

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{Digitizing, turned_angle};
use vectorcraft_doc::{Document, Node, NodeKind, TraceView};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;

use super::create::Made;
use super::settings::{SETTINGS, Settings};
use crate::cmd::buildcmds::TRACE_NAME;
use crate::{EngineError, Result};

/// The settings a live object doesn't keep: the size, which is its image's as placed.
const NOT_KEPT: [&str; 3] = ["width", "height", "fit"];

/// The digitizing a live object holds: `n` is a group named as Image Trace names its objects, an
/// image first in it, and it keeps a digitizing.
pub fn live_digitizing(n: &Node) -> Option<&Digitizing> {
    let image_first = n.children().and_then(|c| c.first()).is_some_and(|c| matches!(c.kind, NodeKind::Image(_)));
    let traced = matches!(n.kind, NodeKind::Group { clip: false, .. }) && n.name.as_deref() == Some(TRACE_NAME) && image_first;
    traced.then(|| n.extras().embroidery.as_deref()?.digitizing.as_ref()).flatten()
}

/// The source image of live object `n` (its first child) and that image's transform.
fn image_of(n: &Node) -> Option<(&Node, Affine)> {
    let image = n.children()?.first()?;
    match &image.kind {
        NodeKind::Image(i) => Some((image, i.xf)),
        _ => None,
    }
}

/// Whether live object `n`'s objects were edited by hand since it was made: their fingerprint now
/// against the one kept (none kept: taken as edited). `None` when `n` is no live object. It walks
/// all of its objects' points: ask it once, not every frame.
pub fn edited(n: &Node) -> Option<bool> {
    let d = live_digitizing(n)?;
    let (_, xf) = image_of(n)?;
    Some(d.fingerprint.is_none() || fingerprint(n, xf) != d.fingerprint)
}

/// A fingerprint of live object `n`'s objects (all but its image) in the image's frame `xf`: their
/// paths, colours, visibility and stitch, each point taken back through `xf` and rounded, so that
/// moving, turning or scaling the object changes nothing and an edit does. FNV-1a, whose value is
/// the same on every version and platform (as the manual stitches' fingerprint).
fn fingerprint(n: &Node, xf: Affine) -> Option<u64> {
    use super::super::manual::{FINGERPRINT_STEP, FNV_OFFSET, FNV_PRIME};
    if !(xf.determinant().is_finite() && xf.determinant().abs() > f64::EPSILON) {
        return None;
    }
    let back = xf.inverse();
    let mut h = FNV_OFFSET;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
        }
    };
    let step = |v: f64| ((v / FINGERPRINT_STEP).round() as i64).to_le_bytes();
    let mut stack: Vec<&Node> = n.children()?.iter().skip(1).map(|c| c.as_ref()).collect();
    stack.reverse();
    while let Some(c) = stack.pop() {
        eat(&[u8::from(c.visible)]);
        if let Some(children) = c.children() {
            eat(&children.len().to_le_bytes());
            stack.extend(children.iter().rev().map(|c| c.as_ref()));
            continue;
        }
        let look = serde_json::to_vec(&c.appearance.items).unwrap_or_default();
        eat(&look);
        let stitch = c.extras().embroidery.as_deref().map(|e| {
            let part = |p: &Option<Box<vectorcraft_doc::embroidery::PartSettings>>| {
                p.as_deref().map(|p| (p.stitch.clone(), p.narrow_as_satin.map(f64::to_bits))).unwrap_or_default()
            };
            (part(&e.fill), part(&e.stroke))
        });
        eat(format!("{stitch:?}").as_bytes());
        if let Some(path) = c.path_data() {
            eat(&path.subpaths.len().to_le_bytes());
            for sp in &path.subpaths {
                eat(&sp.anchors.len().to_le_bytes());
                for a in &sp.anchors {
                    let p = back * a.p;
                    eat(&step(p.x));
                    eat(&step(p.y));
                }
            }
        }
    }
    Some(h)
}

/// The size an image of `width` × `height` pixels placed by `xf` has on the page, millimetres:
/// each side along its own direction, so that a turn changes nothing.
fn image_size_mm(width: u32, height: u32, xf: Affine) -> [f64; 2] {
    let side = |v: u32, x: f64, y: f64| f64::from(v) * x.hypot(y) * MM_PER_PT;
    let [m0, m1, m2, m3, _, _] = xf.as_coeffs();
    [side(width, m0, m1), side(height, m2, m3)]
}

/// How much live object `n` was scaled since it was digitized: the least and the most it stretches
/// a length by, in any direction (`scale_factors`, as stitches sewn as they are are judged), its
/// image's transform now against the one it had then. That one is rebuilt from the size kept, the
/// lengths of its image's sides: a placed image's sides are square to each other, so the turn it
/// also had changes nothing (a picture skewed when it was digitized is judged as if it weren't).
/// `None` when `n` is no live object, keeps no size, or a factor isn't a finite number.
pub(crate) fn live_scale_since(n: &Node) -> Option<[f64; 2]> {
    let made = live_digitizing(n)?.size?;
    let NodeKind::Image(img) = &n.children()?.first()?.kind else { return None };
    // Points a pixel along each side then.
    let then = [made[0] / (f64::from(img.width) * MM_PER_PT), made[1] / (f64::from(img.height) * MM_PER_PT)];
    if !then.iter().all(|s| s.is_finite() && *s > 0.0) {
        return None;
    }
    let [m0, m1, m2, m3, _, _] = img.xf.as_coeffs();
    vectorcraft_doc::embroidery::scale_factors(Affine::new([m0 / then[0], m1 / then[0], m2 / then[1], m3 / then[1], 0.0, 0.0]))
}

/// The settings a live object keeps, by their file keys (`settings`' echo but the size), a change's
/// angle turned into the image as transform `xf` shows it (page degrees taken back through `xf`).
fn kept_settings(settings: &Settings, xf: Affine) -> Map<String, Value> {
    let back = xf.inverse();
    let echo = settings.echo();
    let mut out = Map::new();
    for s in SETTINGS.iter().filter(|s| !NOT_KEPT.contains(&s.name)) {
        let Some(v) = echo.get(s.name).filter(|v| !v.is_null()) else { continue };
        out.insert(s.key.to_string(), v.clone());
    }
    if let Some(Value::Array(changes)) = out.get_mut("area_changes") {
        for c in changes.iter_mut() {
            if let Some(a) = c.get("angle").and_then(Value::as_f64) {
                c["angle"] = json!(turned_angle(a, back));
            }
        }
    }
    out
}

/// The settings live object `n` keeps, as the window and the command take them (their names), a
/// change's angle turned back onto the page as the object lies now; `None` when `n` is no live
/// object. Keys no setting of this version has are left out.
pub fn live_settings(n: &Node) -> Option<Map<String, Value>> {
    let d = live_digitizing(n)?;
    let (_, xf) = image_of(n)?;
    let mut out = Map::new();
    for s in SETTINGS.iter().filter(|s| !NOT_KEPT.contains(&s.name)) {
        if let Some(v) = d.settings.get(s.key) {
            out.insert(s.name.to_string(), v.clone());
        }
    }
    if let Some(Value::Array(changes)) = out.get_mut("areaChanges") {
        for c in changes.iter_mut() {
            if let Some(a) = c.get("angle").and_then(Value::as_f64) {
                c["angle"] = json!(turned_angle(a, xf));
            }
        }
    }
    Some(out)
}

/// Create's plain design `made` in `doc` made a live object made with `settings`: in the design's
/// place, the image (with its id) first and hidden, the design's blocks after it; `was`, the live
/// object digitized again (as it was before Create replaced it), keeps its id and what was set on
/// it. The template layer Create added goes.
pub(super) fn into_live(doc: &mut Document, made: &Made, was: Option<Node>, settings: &Settings) -> Result<Made> {
    let mut image = (*doc.remove(made.image)?).clone();
    if made.added_layer {
        doc.remove(made.layer)?;
    }
    let NodeKind::Image(img) = &image.kind else { return Err(EngineError::NoNode(made.image)) };
    let xf = img.xf;
    // The size it is made at: its image's, as placed.
    let size = image_size_mm(img.width, img.height, xf);
    image.visible = false;
    let design = doc.node(made.group).cloned().ok_or(EngineError::NoNode(made.group))?;
    let blocks: Vec<Arc<Node>> = design.children().cloned().unwrap_or_default();
    // Where the old live object still is (Create leaves an image where the design isn't sewn), it
    // goes: the live object takes the design's place.
    if let Some(old) = was.as_ref().filter(|old| doc.node(old.id).is_some()) {
        doc.remove(old.id)?;
    }
    let mut live = match was {
        Some(old) => old,
        None => {
            let mut g = Node::group(made.group, vec![]);
            g.name = Some(TRACE_NAME.into());
            g.extras_mut().trace = Some(Box::new(json!({"preset": "Custom", "view": TraceView::Result.id()})));
            g
        }
    };
    if let Some(children) = live.children_mut() {
        *children = std::iter::once(Arc::new(image)).chain(blocks).collect();
    }
    let mut digitizing = Digitizing { settings: kept_settings(settings, xf).into(), fingerprint: None, size: Some(size), extra: Default::default() };
    digitizing.fingerprint = fingerprint(&live, xf);
    live.extras_mut().embroidery.get_or_insert_with(Default::default).digitizing = Some(digitizing);
    let id = live.id;
    let (parent, index, _) = doc.position(made.group).ok_or(EngineError::NoNode(made.group))?;
    doc.remove(made.group)?;
    doc.insert(parent, index, live)?;
    let layer = doc.layer_of(id).ok_or(EngineError::NoNode(id))?;
    Ok(Made { group: id, layer, added_layer: false, live: true, ..made.clone() })
}
