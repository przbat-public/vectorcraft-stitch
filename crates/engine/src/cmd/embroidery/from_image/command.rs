//! `embroidery.digitizeImage`: an image (or an Image Trace object's source image) digitized with
//! the settings given, answered without changing anything (`dryRun`).
//!
//! Its answer gives what a window shows: the working copy the image was measured at, the colours
//! found with their areas, the warnings, the settings used with their defaults, and how long each
//! stage took.

use serde_json::{Map, Value, json};
use vectorcraft_doc::{ImageObject, NodeId, NodeKind};
use vectorcraft_stitch::thread::{Match, Palette, mismatched, shown_delta_e};

use super::background::{self, Background};
use super::colours::{self, Colours};
use super::settings::{BackgroundSetting, C, Mode, Settings, table};
use super::{trace, working};
use crate::cmd::bad;
use crate::cmd::buildcmds::TRACE_NAME;
use crate::{EngineError, Result, Session};

/// What digitizing reports to a progress sink as it goes ([`vectorcraft_doc::cancel::progress`]):
/// the stages done of [`STAGES`].
pub(crate) const DIGITIZE_PROGRESS: &str = "digitizeImage";

/// The stages a run goes through, as its progress counts them: the working copy, the trace, the
/// colours.
const STAGES: u64 = 3;

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

/// A thread as a reply gives it: its palette, number, name, colour, and its difference from the
/// image's colour (shown to two decimals, flagged past the line).
fn thread_json(palette: Palette, m: Match) -> Value {
    let (color, name) = palette.get(m.index).unwrap_or(([0, 0, 0], ""));
    json!({
        "palette": palette.id(),
        "index": m.index,
        "name": name,
        "color": super::super::hex(color),
        "deltaE": shown_delta_e(m.delta_e),
        "mismatch": mismatched(m.delta_e),
    })
}

/// The colour list's rows as the reply gives them.
fn colors_json(colours: &Colours, palette: Palette, px_per_mm2: f64, frame_px: f64) -> Vec<Value> {
    let sewn_px: usize = colours.rows.iter().filter(|r| r.sewn()).map(|r| r.pixels).sum();
    colours
        .rows
        .iter()
        .map(|r| {
            let merged = r.merged_into.and_then(|j| colours.rows.get(j)).map(|t| super::super::hex(t.color));
            let reason = match (&r.thread, &merged) {
                (_, Some(_)) => Some("merged"),
                (Some(m), None) => Some(m.reason.id()),
                (None, None) => None,
            };
            json!({
                "color": super::super::hex(r.color),
                "background": r.background,
                "sewn": r.sewn(),
                "job": r.job.id(),
                "thread": r.thread.as_ref().map(|m| thread_json(palette, m.thread)),
                "reason": reason,
                "alternatives": r.thread.as_ref().map_or(vec![], |m| m.alternatives.iter().map(|&a| thread_json(palette, a)).collect()),
                "area": hundredths(r.pixels as f64 / px_per_mm2),
                "areaCount": r.paths.len(),
                "share": hundredths(r.pixels as f64 / frame_px.max(1.0) * PERCENT),
                "sewnShare": r.sewn().then(|| hundredths(r.pixels as f64 / sewn_px.max(1) as f64 * PERCENT)),
                "mergedInto": merged,
                "choice": r.choice,
            })
        })
        .collect()
}

/// The background as a reply gives it: where it came from, and its colour.
fn background_json(found: Background, setting: BackgroundSetting, traced: Option<[u8; 3]>) -> Value {
    let from = match (setting, found) {
        (BackgroundSetting::None, _) | (_, Background::None) => "none",
        (BackgroundSetting::Colour(_), _) => "given",
        (BackgroundSetting::Corners, Background::Transparent) => "transparency",
        (BackgroundSetting::Corners, Background::Colour(_)) => "corners",
    };
    json!({"from": from, "color": traced.map(super::super::hex)})
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
    let found = match settings.background {
        BackgroundSetting::Corners => background::found(&w.raster),
        BackgroundSetting::None => Background::None,
        BackgroundSetting::Colour(c) => Background::Colour(c),
    };
    let image_ms = since(started);
    vectorcraft_doc::cancel::check()?;
    vectorcraft_doc::cancel::progress(1, STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    // A background colour takes one of the trace's colours: the design keeps the colours asked.
    let colors = settings.colors + u32::from(matches!(found, Background::Colour(_)));
    let mut warnings = w.warnings();
    let mut traced = trace::traced(&w, &settings, colors, trace::Palette::Mode)?;
    if settings.mode == Mode::Logo && traced.palette.len() > colors as usize {
        // The Automatic palette traces an image that isn't flat art (a photo, gradients) in as
        // many colours as its tones need: traced again in the colours asked.
        warnings.push(format!(
            "the image doesn't look like flat art (a photo or gradients?): {} colours were found, so it was traced in the {} that represent it best",
            traced.palette.len(),
            settings.colors
        ));
        traced = trace::traced(&w, &settings, colors, trace::Palette::Limited)?;
    }
    let trace_ms = since(started);
    vectorcraft_doc::cancel::check()?;
    vectorcraft_doc::cancel::progress(2, STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let traced_background = colours::background_colour(&traced.palette, found);
    if matches!(found, Background::Colour(_)) && traced_background.is_none() {
        warnings.push("no traced colour is the background's: every area is sewn".into());
    }
    if settings.background == BackgroundSetting::Corners && found == Background::None {
        warnings.push("the image's corners don't agree on a background: every area is sewn (give `background` to name one)".into());
    }
    let colours = colours::listed(&traced, (w.raster.width, w.raster.height), traced_background, settings.mode, palette, &settings.choices)?;
    let colors_ms = since(started);
    vectorcraft_doc::cancel::progress(STAGES, STAGES, DIGITIZE_PROGRESS);
    if traced.paths.is_empty() {
        warnings.push("nothing in the image was traced: it is empty or transparent".into());
    }
    let px_per_mm2 = w.px_per_mm * w.px_per_mm;
    let frame_px = f64::from(w.raster.width) * f64::from(w.raster.height);
    let unused: Vec<Value> = colours.unused.iter().map(|&i| json!({"choice": i, "reason": "noSuchColor"})).collect();
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
            "background": background_json(found, settings.background, traced_background),
        },
        "colorCount": colours.rows.iter().filter(|r| r.sewn() && r.merged_into.is_none()).count(),
        "colors": colors_json(&colours, palette, px_per_mm2, frame_px),
        "unusedChoices": unused,
        "areaCount": traced.paths.len(),
        "warnings": warnings,
        "settings": settings.echo(),
        "table": table(),
        "timings": {"image": image_ms, "trace": trace_ms, "colors": colors_ms},
    }))
}
