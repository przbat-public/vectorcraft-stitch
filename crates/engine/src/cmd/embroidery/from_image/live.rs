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
use vectorcraft_doc::{AppearanceItem, Document, Node, NodeId, NodeKind, TraceView};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;

use vectorcraft_stitch::thread::Palette;

use super::create::Made;
use super::settings::{SETTINGS, Settings, most_colors};
use super::warnings::Warning;
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

/// The live object `id` of `doc` is, or holds as its image (its first child), when it is one or
/// does: what Image Trace refuses either way.
pub(crate) fn live_object_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    if doc.node(id).is_some_and(|n| live_digitizing(n).is_some()) {
        return Some(id);
    }
    let parent = doc.parent_of(id)?;
    let n = doc.node(parent)?;
    (live_digitizing(n).is_some() && n.children()?.first().is_some_and(|c| c.id == id)).then_some(parent)
}

/// The source image of live object `n` (its first child) and that image's transform.
fn image_of(n: &Node) -> Option<(&Node, Affine)> {
    let image = n.children()?.first()?;
    match &image.kind {
        NodeKind::Image(i) => Some((image, i.xf)),
        _ => None,
    }
}

/// The slant between a picture's sides (the sine of the angle between them) when they are square to
/// each other, as a placed picture's are.
const SQUARE_SLANT: f64 = 1.0;

/// Whether live object `n`'s objects were edited by hand since it was made: their fingerprint now
/// against the one kept, their strokes' weights scaled with it or kept as made; none kept: taken as
/// edited. `None` when `n` is no live object. Strokes kept as made are measured at the scale it was
/// made at: its picture's sides' lengths then, from the size kept, and the slant between them as it
/// is now (a move, a turn or a scale since keeps it) or square, as a placed picture's are (a turn
/// and then a stretch along the page slants them). Read as an edit, the safe side: a picture slanted
/// when it was made whose slant and size both changed since, its strokes kept; one scaled once with
/// Scale Strokes & Effects on and once off, its strokes neither. It walks all of its objects'
/// points, up to three times: ask it once, not every frame.
pub fn edited(n: &Node) -> Option<bool> {
    let d = live_digitizing(n)?;
    let (_, xf) = image_of(n)?;
    let Some(kept) = d.fingerprint else { return Some(true) };
    let matches = |weight_px: f64| fingerprint(n, xf, weight_px) == Some(kept);
    // Its strokes' weights scaled with it (Scale Strokes & Effects on): no edit.
    if matches(xf.determinant().abs().sqrt()) {
        return Some(false);
    }
    // Kept as they were made (off, the default): no edit either.
    let Some([a, b]) = made_points_per_px(n) else { return Some(true) };
    let [m0, m1, m2, m3, _, _] = xf.as_coeffs();
    let sides = m0.hypot(m1) * m2.hypot(m3);
    let slant = if sides > f64::EPSILON { (xf.determinant().abs() / sides).min(SQUARE_SLANT) } else { SQUARE_SLANT };
    Some(!(matches((a * b * slant).sqrt()) || (slant < SQUARE_SLANT && matches((a * b * SQUARE_SLANT).sqrt()))))
}

/// A fingerprint of live object `n`'s objects (all but its image) in the image's frame `xf`: each
/// object's kind and the flags that change what is sewn (a guide, a fill rule, a clipping path or
/// clip group, a live shape), its visibility, opacity and blending, its own look (its fills and
/// strokes, their effects, the object's effects) and embroidery settings, a compound path's and a
/// group's included, and its paths' anchors with their handles and whether each is closed, each
/// point and setting taken back through `xf` and rounded, a set angle read as the direction it
/// gives; a stroke's weight divided by `weight_px` (points a pixel: the factor Scale Strokes &
/// Effects scales it by is the square root of the transform's area factor). So moving, turning or
/// scaling the object changes nothing and an edit does. FNV-1a over lengths of 64 bits, whose value
/// is the same on every version and platform (as the manual stitches' fingerprint).
fn fingerprint(n: &Node, xf: Affine, weight_px: f64) -> Option<u64> {
    use super::super::manual::{FINGERPRINT_STEP, FNV_OFFSET, FNV_PRIME};
    let det = xf.determinant();
    if !(det.is_finite() && det.abs() > f64::EPSILON && weight_px.is_finite() && weight_px > 0.0) {
        return None;
    }
    let back = xf.inverse();
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX).to_le_bytes();
    let mut h = FNV_OFFSET;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(FNV_PRIME);
        }
    };
    let rounded = |v: f64| (v / FINGERPRINT_STEP).round();
    let step = |v: f64| (rounded(v) as i64).to_le_bytes();
    let mut stack: Vec<&Node> = n.children()?.iter().skip(1).map(|c| c.as_ref()).collect();
    stack.reverse();
    while let Some(c) = stack.pop() {
        // What it is, and the flags that change what is sewn of it.
        match &c.kind {
            NodeKind::Path { rule, clipping, guide, live, .. } => {
                eat(b"path");
                eat(&[u8::from(*clipping), u8::from(*guide), u8::from(live.is_some())]);
                eat(&serde_json::to_vec(rule).unwrap_or_default());
            }
            NodeKind::Compound { rule, .. } => {
                eat(b"compound");
                eat(&serde_json::to_vec(rule).unwrap_or_default());
            }
            NodeKind::Group { clip, .. } => {
                eat(b"group");
                eat(&[u8::from(*clip)]);
            }
            _ => eat(b"other"),
        }
        eat(&[u8::from(c.visible)]);
        eat(&c.opacity.to_bits().to_le_bytes());
        eat(&serde_json::to_vec(&c.blend).unwrap_or_default());
        // Every object's own look and stitch, before what it holds: Create paints an area with
        // holes, and sets its stitch, on its compound path, whose members carry none, and the
        // paint and settings commands write there too.
        let mut items = c.appearance.items.clone();
        for item in &mut items {
            if let AppearanceItem::Stroke(stroke) = item {
                stroke.width = rounded(stroke.width / weight_px) * FINGERPRINT_STEP;
            }
        }
        eat(&serde_json::to_vec(&items).unwrap_or_default());
        eat(&serde_json::to_vec(&c.appearance.effects).unwrap_or_default());
        // Its embroidery settings, every one the Object tab and the sequence set (a stitch, an
        // angle, entry and exit, Trim After…), taken back into the image's frame as its paths are
        // (a transform and its inverse give them back), each number rounded.
        let settings = c.extras().embroidery.as_deref().map(|e| {
            let mut e = e.clone();
            e.transform(back);
            // A set angle as the direction it gives: 270° and −90°, 180° and −180° are one.
            for part in [&mut e.fill, &mut e.stroke].into_iter().flatten() {
                if let Some(a) = part.angle.as_mut() {
                    *a = a.rem_euclid(FULL_TURN_DEG);
                }
            }
            let mut v = serde_json::to_value(&e).unwrap_or_default();
            rounded_numbers(&mut v, 0);
            v
        });
        eat(&serde_json::to_vec(&settings).unwrap_or_default());
        if let Some(children) = c.children() {
            eat(&count(children.len()));
            stack.extend(children.iter().rev().map(|c| c.as_ref()));
            continue;
        }
        if let Some(path) = c.path_data() {
            eat(&count(path.subpaths.len()));
            for sp in &path.subpaths {
                eat(&[u8::from(sp.closed)]);
                eat(&count(sp.anchors.len()));
                for a in &sp.anchors {
                    for q in [a.p, a.h_in, a.h_out] {
                        let p = back * q;
                        eat(&step(p.x));
                        eat(&step(p.y));
                    }
                }
            }
        }
    }
    Some(h)
}

/// A full turn (degrees): what a set angle is read modulo in a fingerprint.
const FULL_TURN_DEG: f64 = 360.0;

/// How deep an object's settings are walked for the numbers a fingerprint rounds: far deeper than
/// any real nesting (the JSON reader stops at 128 levels), a bound on the walk; what lies deeper is
/// left as it is. An estimate.
const MAX_SETTINGS_DEPTH: usize = 16;

/// Each number in `v` rounded to [`FINGERPRINT_STEP`](super::super::manual::FINGERPRINT_STEP)
/// steps, down to [`MAX_SETTINGS_DEPTH`] levels below `depth`, so that the last bits a transform and
/// its inverse leave in an angle or a point change nothing.
fn rounded_numbers(v: &mut Value, depth: usize) {
    if depth > MAX_SETTINGS_DEPTH {
        return;
    }
    match v {
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                // `as` saturates: a number past i64's range is held at its end, never a panic.
                *v = json!((f / super::super::manual::FINGERPRINT_STEP).round() as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|x| rounded_numbers(x, depth + 1)),
        Value::Object(map) => map.values_mut().for_each(|x| rounded_numbers(x, depth + 1)),
        _ => {}
    }
}

/// The size an image of `width` × `height` pixels placed by `xf` has on the page, millimetres:
/// each side along its own direction, so that a turn changes nothing.
fn image_size_mm(width: u32, height: u32, xf: Affine) -> [f64; 2] {
    let side = |v: u32, x: f64, y: f64| f64::from(v) * x.hypot(y) * MM_PER_PT;
    let [m0, m1, m2, m3, _, _] = xf.as_coeffs();
    [side(width, m0, m1), side(height, m2, m3)]
}

/// The points a pixel of live object `n`'s image measured along each of its sides when it was
/// digitized, rebuilt from the size kept (its sides square to each other, as a placed image's are);
/// `None` when it keeps no size or a measure isn't a finite number above 0.
fn made_points_per_px(n: &Node) -> Option<[f64; 2]> {
    let made = live_digitizing(n)?.size?;
    let NodeKind::Image(img) = &n.children()?.first()?.kind else { return None };
    let then = [made[0] / (f64::from(img.width) * MM_PER_PT), made[1] / (f64::from(img.height) * MM_PER_PT)];
    then.iter().all(|s| s.is_finite() && *s > 0.0).then_some(then)
}

/// How much live object `n` was scaled since it was digitized: the least and the most it stretches
/// a length by, in any direction (`scale_factors`, as stitches sewn as they are are judged), its
/// image's transform now against the one it had then. That one is rebuilt from the size kept, the
/// lengths of its image's sides: a placed image's sides are square to each other, so the turn it
/// also had changes nothing (a picture skewed when it was digitized is judged as if it weren't).
/// `None` when `n` is no live object, keeps no size, or a factor isn't a finite number.
pub(crate) fn live_scale_since(n: &Node) -> Option<[f64; 2]> {
    let then = made_points_per_px(n)?;
    let NodeKind::Image(img) = &n.children()?.first()?.kind else { return None };
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

/// The settings `embroidery.digitizeImage` digitizes a live object with, as the window opens on it:
/// those it keeps (`kept`, [`live_settings`]), the request's `o` over them (a setting `o` gives
/// wins; one it gives as `null` is the default, not the kept one), the project owner's choice of
/// 2026-11-17, „Brać zapisane (polecam)”. Kept settings the engine refuses, a crop lying outside the
/// image's `pixels` (relinked since at another size) among them: their threads let go and the
/// number of colours held to `palette`'s (made in another thread palette), else the request's own
/// settings `given` with the defaults; and the warning that says which, if any, with what still
/// blocks them.
pub(super) fn with_kept(
    o: &Map<String, Value>,
    kept: Map<String, Value>,
    palette: Palette,
    pixels: (u32, u32),
    given: Settings,
) -> (Settings, Option<Warning>) {
    let over = |kept: &Map<String, Value>| -> std::result::Result<Settings, String> {
        let mut all = kept.clone();
        for (k, v) in o {
            if v.is_null() {
                all.remove(k);
            } else {
                all.insert(k.clone(), v.clone());
            }
        }
        // Said as what is wrong with them, not as the request's parameters.
        let settings = Settings::from_params(&all, palette).map_err(|e| match e {
            EngineError::BadParams { msg, .. } => msg,
            e => e.to_string(),
        })?;
        let (iw, ih) = pixels;
        match settings.crop {
            Some(crop) if crop[2] > iw || crop[3] > ih => Err(format!("`crop` {crop:?} lies outside the image's {iw} × {ih} pixels")),
            _ => Ok(settings),
        }
    };
    if let Ok(settings) = over(&kept) {
        return (settings, None);
    }
    let why = match over(&without_threads(kept, palette)) {
        Ok(settings) => {
            let message =
                "the live object was made in another thread palette: its threads were let go and its colours matched to the document's palette again";
            return (settings, Some(Warning::new("keptThreadsLetGo", json!({}), message)));
        }
        Err(why) => why,
    };
    let message = format!("the settings the live object keeps couldn't be used ({why}): the request's settings and the defaults were used");
    (given, Some(Warning::new("keptSettingsRefused", json!({"why": why}), message)))
}

/// The keys of the settings live object `n` keeps that no setting of this version has (a newer
/// version's): left out of what it is digitized with, and of what it keeps once made again.
pub(super) fn unknown_kept(n: &Node) -> Vec<String> {
    let Some(d) = live_digitizing(n) else { return Vec::new() };
    d.settings.keys().filter(|k| !SETTINGS.iter().any(|s| s.key == k.as_str())).cloned().collect()
}

/// Kept settings `kept` with their threads let go (each colour choice's thread, the outlines'
/// thread) and the number of colours held to `palette`'s most: as the window takes a live object
/// made in another thread palette (the project owner's choice, 2026-11-15).
fn without_threads(mut kept: Map<String, Value>, palette: Palette) -> Map<String, Value> {
    kept.remove("outlineThread");
    if let Some(Value::Array(choices)) = kept.get_mut("colorChoices") {
        for c in choices.iter_mut() {
            if let Some(c) = c.as_object_mut() {
                c.remove("thread");
            }
        }
        // A choice that only named a thread chooses nothing now.
        choices.retain(|c| c.as_object().is_some_and(|c| c.keys().any(|k| k != "color")));
    }
    let most = u64::from(most_colors(palette));
    if kept.get("colors").and_then(Value::as_u64).is_some_and(|n| n > most) {
        kept.insert("colors".into(), json!(most));
    }
    kept
}

/// Create's plain design `made` in `doc` made a live object made with `settings`: in the design's
/// place, the image first and hidden (with its id; a copy of one an upstream Image Trace object
/// keeps, placed as the design is scaled), the design's blocks after it; `was`, the live object
/// digitized again (as it was before Create replaced it), keeps its id and what was set on it. The
/// template layer Create added goes.
pub(super) fn into_live(doc: &mut Document, made: &Made, was: Option<Node>, settings: &Settings) -> Result<Made> {
    // The image Create left inside an upstream Image Trace object (its place isn't sewn) stays that
    // object's source, so that its View, Expand and Release still work: the live object takes a copy.
    let in_trace = doc.parent_of(made.image).and_then(|p| doc.node(p)).is_some_and(|g| {
        matches!(g.kind, NodeKind::Group { .. })
            && g.name.as_deref() == Some(TRACE_NAME)
            && g.children().and_then(|c| c.first()).is_some_and(|c| c.id == made.image)
    });
    let mut image = if in_trace {
        let mut copy = doc.node(made.image).cloned().ok_or(EngineError::NoNode(made.image))?;
        copy.id = doc.alloc_id();
        // Placed as the design is scaled, as Create would have placed the image itself.
        if let NodeKind::Image(img) = &mut copy.kind {
            img.xf = made.image_xf;
        }
        copy
    } else {
        (*doc.remove(made.image)?).clone()
    };
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
    // Create replaced the old live object (in its place, or, where its place isn't sewn, by its
    // image); should it still be in the document even so, it goes: the live object takes the
    // design's place, and no second design is left behind.
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
    digitizing.fingerprint = fingerprint(&live, xf, xf.determinant().abs().sqrt());
    live.extras_mut().embroidery.get_or_insert_with(Default::default).digitizing = Some(digitizing);
    let id = live.id;
    let (parent, index, _) = doc.position(made.group).ok_or(EngineError::NoNode(made.group))?;
    doc.remove(made.group)?;
    doc.insert(parent, index, live)?;
    let layer = doc.layer_of(id).ok_or(EngineError::NoNode(id))?;
    Ok(Made { group: id, layer, added_layer: false, live: true, ..made.clone() })
}
