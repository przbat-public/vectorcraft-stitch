//! `embroidery.digitizeImage`: an image (or an Image Trace object's source image) digitized with
//! the settings given, answered without changing anything (`dryRun`).
//!
//! Its answer gives what a window shows: the working copy the image was measured at, the colours
//! found with their areas, the warnings, the settings used with their defaults, and how long each
//! stage took.

use serde_json::{Map, Value, json};
use vectorcraft_doc::{ImageObject, NodeId, NodeKind};

use super::settings::{C, Settings, table};
use super::{trace, working};
use crate::cmd::bad;
use crate::cmd::buildcmds::TRACE_NAME;
use crate::{EngineError, Result, Session};

/// What digitizing reports to a progress sink as it goes ([`vectorcraft_doc::cancel::progress`]):
/// the stages done of [`STAGES`].
pub(crate) const DIGITIZE_PROGRESS: &str = "digitizeImage";

/// The stages a run goes through, as its progress counts them: the working copy, the trace.
const STAGES: u64 = 2;

/// Hundredths: millimetres, square millimetres and shares are given to two decimals.
const HUNDREDTHS: f64 = 100.0;

/// A share is given as a percentage.
const PERCENT: f64 = 100.0;

/// Milliseconds in a second, for the stages' timings.
#[cfg(not(target_arch = "wasm32"))]
const MS_PER_SEC: f64 = 1000.0;

/// `v` to two decimals, as the replies give millimetres and shares.
fn hundredths(v: f64) -> f64 {
    (v * HUNDREDTHS).round() / HUNDREDTHS
}

/// Whether flag `key` of `o` is on (left out or null: off).
fn flag(o: &Map<String, Value>, key: &str) -> Result<bool> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("`{key}` {v}: true or false"))),
    }
}

/// The image `o` names (`id`), else the one object selected: an image, or an Image Trace object,
/// whose first child is the image it was traced from.
fn target(s: &Session, o: &Map<String, Value>) -> Result<(NodeId, ImageObject)> {
    let st = s.doc()?;
    let id = match o.get("id") {
        None | Some(Value::Null) => match st.selection.objects.as_slice() {
            [one] => *one,
            [] => return Err(bad(C, "select an image, or give its `id`")),
            _ => return Err(bad(C, "select one image, or give its `id`")),
        },
        Some(v) => NodeId(v.as_u64().ok_or_else(|| bad(C, format!("`id` {v}: an object's id")))?),
    };
    let n = st.doc.node(id).ok_or(EngineError::NoNode(id))?;
    let image = match &n.kind {
        NodeKind::Image(i) => Some((**i).clone()),
        NodeKind::Group { .. } if n.name.as_deref() == Some(TRACE_NAME) => n.children().and_then(|c| c.first()).and_then(|c| match &c.kind {
            NodeKind::Image(i) => Some((**i).clone()),
            _ => None,
        }),
        _ => None,
    };
    let image = image.ok_or_else(|| bad(C, "not an image: select an image, or an Image Trace object"))?;
    Ok((id, image))
}

/// The time since `started` (milliseconds); none on the web, which has no clock to read.
#[cfg(not(target_arch = "wasm32"))]
fn since(started: std::time::Instant) -> Option<f64> {
    Some(hundredths(started.elapsed().as_secs_f64() * MS_PER_SEC))
}

/// The clock a stage's time is read from; none on the web.
#[cfg(not(target_arch = "wasm32"))]
fn clock() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg(target_arch = "wasm32")]
fn clock() {}

#[cfg(target_arch = "wasm32")]
fn since(_: ()) -> Option<f64> {
    None
}

/// `embroidery.digitizeImage`.
pub(in crate::cmd::embroidery) fn digitize_image(s: &mut Session, p: &Value) -> Result<Value> {
    let o = super::super::object(p, C)?;
    let palette = super::super::thread_palette(&s.doc()?.doc);
    let settings = Settings::from_params(&o, palette)?;
    if !flag(&o, "dryRun")? {
        return Err(bad(C, "give `dryRun: true`: digitizing answers what it would make, and creates nothing yet"));
    }
    let (id, image) = target(s, &o)?;
    let bytes = s
        .doc()?
        .doc
        .images
        .get(&image.key)
        .map(|b| std::sync::Arc::clone(&b.bytes))
        .ok_or_else(|| bad(C, format!("the image's data `{}` is missing", image.key)))?;
    vectorcraft_doc::cancel::progress(0, STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let w = working::prepare(&image, &bytes, &settings)?;
    let image_ms = since(started);
    vectorcraft_doc::cancel::check()?;
    vectorcraft_doc::cancel::progress(1, STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let traced = trace::traced(&w, &settings)?;
    let trace_ms = since(started);
    vectorcraft_doc::cancel::progress(STAGES, STAGES, DIGITIZE_PROGRESS);
    // Each colour's area: its paths' pixels at the working resolution.
    let px_per_mm2 = w.px_per_mm * w.px_per_mm;
    let mut colors: Vec<([u8; 3], usize, usize)> = traced.palette.iter().map(|&c| (c, 0, 0)).collect();
    for path in &traced.paths {
        if let Some(entry) = colors.iter_mut().find(|e| e.0 == path.color) {
            entry.1 += path.pixels;
            entry.2 += 1;
        }
    }
    let total: usize = colors.iter().map(|e| e.1).sum();
    let colors: Vec<Value> = colors
        .iter()
        .map(|&(c, pixels, count)| {
            json!({
                "color": super::super::hex(c),
                "area": hundredths(pixels as f64 / px_per_mm2),
                "areaCount": count,
                "share": hundredths(pixels as f64 / total.max(1) as f64 * PERCENT),
            })
        })
        .collect();
    let mut warnings = w.warnings();
    if traced.paths.is_empty() {
        warnings.push("nothing in the image was traced: it is empty or transparent".into());
    }
    Ok(json!({
        "dryRun": true,
        "image": {
            "id": id.0,
            "pixels": [w.image_px.0, w.image_px.1],
            "decoded": [w.decoded_px.0, w.decoded_px.1],
            "crop": w.crop,
            "worked": [w.raster.width, w.raster.height],
            "pxPerMm": hundredths(w.px_per_mm),
            "limitedBy": w.limit.id(),
            "size": [hundredths(w.size_mm.0), hundredths(w.size_mm.1)],
            "scale": w.scale,
            "bounds": w.bounds_mm().map(hundredths),
        },
        "colorCount": colors.len(),
        "colors": colors,
        "areaCount": traced.paths.len(),
        "warnings": warnings,
        "settings": settings.echo(),
        "table": table(),
        "timings": {"image": image_ms, "trace": trace_ms},
    }))
}
