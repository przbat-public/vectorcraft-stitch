//! The image window's words: what its title says it digitizes, its steps' names, and its run's
//! state, every one through the embroidery catalogue context, numbers as the interface language
//! writes them.

use serde_json::Value;

use crate::VectorcraftApp;
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::state::Dialog;

/// The decimals the window writes its numbers to: sizes in millimetres and areas in square
/// millimetres to a tenth, a smallest area and a resolution to hundredths, and whole the times, the
/// scale bar's lengths and the trace's settings.
pub(crate) const TENTHS: usize = 1;
pub(crate) const HUNDREDTHS: usize = 2;
pub(crate) const WHOLE: usize = 0;

/// A whole, in percent: a run's progress is said in percent.
const PERCENT: f64 = 100.0;

/// The four steps' names, in order.
pub(crate) fn step_names() -> [&'static str; 4] {
    [crate::et!("Image"), crate::et!("Colors"), crate::et!("Shapes"), crate::et!("Stitches")]
}

/// A file format's name from its media type ("image/jpeg" → "JPEG").
fn format_name(mime: &str) -> String {
    match mime {
        "image/jpeg" | "image/jpg" => "JPEG".into(),
        other => other.rsplit('/').next().unwrap_or(other).to_ascii_uppercase(),
    }
}

/// What the title says the window digitizes: the image's name, its format and pixels, and the
/// design's size once it is measured ("fox.jpg · JPEG 1044 × 941 px · embroidery 98 × 88 mm").
pub(crate) fn title_facts(app: &VectorcraftApp, d: &Dialog) -> Option<String> {
    let st = app.session.active()?;
    let id = vectorcraft_engine::doc::NodeId(d.fields.get(super::IMAGE).and_then(Value::as_u64)?);
    let node = st.doc.node(id)?;
    let image = match &node.kind {
        vectorcraft_engine::doc::NodeKind::Image(i) => Some(i.as_ref()),
        _ => node.children().and_then(|c| c.first()).and_then(|c| match &c.kind {
            vectorcraft_engine::doc::NodeKind::Image(i) => Some(i.as_ref()),
            _ => None,
        }),
    }?;
    let lang = crate::i18n::current();
    let name: String =
        node.name.clone().unwrap_or_else(|| crate::et!("Image").to_string()).chars().take(crate::embroidery::vocabulary::MAX_SHOWN).collect();
    let format = st.doc.images.get(&image.key).map_or_else(String::new, |b| format_name(&b.mime));
    let (w, h) = (numbers::count(lang, u64::from(image.width)), numbers::count(lang, u64::from(image.height)));
    let size = app.embroidery.image_window.live.as_ref().and_then(|l| l.schedule.shown.as_ref()).and_then(|s| s.outputs.size_mm());
    Some(match size {
        Some((mw, mh)) => fill(
            crate::et!("{name} · {format} {width} × {height} px · embroidery {w} × {h} mm"),
            &[
                ("name", &name),
                ("format", &format),
                ("width", &w),
                ("height", &h),
                ("w", &numbers::decimal(lang, mw, TENTHS)),
                ("h", &numbers::decimal(lang, mh, TENTHS)),
            ],
        ),
        None => fill(crate::et!("{name} · {format} {width} × {height} px"), &[("name", &name), ("format", &format), ("width", &w), ("height", &h)]),
    })
}

/// That the window is recomputing, with the run's progress when it reports one (0 to 1).
pub(crate) fn recomputing(progress: Option<f64>) -> String {
    match progress {
        Some(f) if f.is_finite() => {
            let percent = (f.clamp(0.0, 1.0) * PERCENT).round() as u64;
            fill(crate::et!("Recomputing… {percent}"), &[("percent", &numbers::percent(crate::i18n::current(), percent))])
        }
        _ => crate::et!("Recomputing…").to_string(),
    }
}
