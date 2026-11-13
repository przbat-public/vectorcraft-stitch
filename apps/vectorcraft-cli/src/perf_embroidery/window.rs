//! The image window's rows of `vectorcraft-cli perf --embroidery`: digitizing's stages kept between
//! runs (`DigitizePipeline`), as the window runs them at every change. Each sample starts a pipeline
//! of its own, so that what a row times is made, never found kept from the sample before:
//! - the first run of the badge (the digitizing rows' 98 × 88 mm JPEG in six colours), every stage
//!   through its plan;
//! - a thread chosen for one of its colours, and the colours asked changed from six to five, each
//!   after a first run: only the stages after the change are made;
//! - a run with nothing changed: every stage found kept;
//! - the badge drawn 1,500 pixels wide (the working copy's largest) in logo mode, its first run;
//! - that badge's working copy painted in its threads' colours, as the Colours view shows it;
//! - a fabric chosen in the Stitches step after a first run: the objects and their stitches made;
//! - the colour outlines and the border turned on after a first run: the same, with the outlines.

use std::time::Instant;

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

use super::digitizing::{BADGE_COLORS, badge_at, placed_badge};
use super::{RUNS, err, middle};

/// The rows' names, in order (at most 42 characters, the report's column).
pub(super) const ROWS: [&str; 8] = [
    "window: first run, badge, to its plan",
    "window: a thread chosen, trace kept",
    "window: six colours to five",
    "window: nothing changed, all kept",
    "window: logo mode, 1,500 px badge",
    "window: painted raster, 1,500 px badge",
    "window: a fabric chosen, objects on",
    "window: outlines and border, objects on",
];

/// Each row's budget, milliseconds, in the order of [`ROWS`]: [`BUDGET_TIMES`] what each measured,
/// as the other rows', so that a regression of that size shows; the kept run's is 1 ms, the least
/// the report's whole-millisecond column shows (it measured well under a hundredth of that).
pub(super) const BUDGETS: [f64; 8] = [
    MEASURED_MS[0] * BUDGET_TIMES,
    MEASURED_MS[1] * BUDGET_TIMES,
    MEASURED_MS[2] * BUDGET_TIMES,
    KEPT_BUDGET_MS,
    MEASURED_MS[4] * BUDGET_TIMES,
    MEASURED_MS[5] * BUDGET_TIMES,
    MEASURED_MS[6] * BUDGET_TIMES,
    MEASURED_MS[7] * BUDGET_TIMES,
];

/// How many times its measure a row's budget is: the embroidery rows' rule.
const BUDGET_TIMES: f64 = 2.5;

/// The budget of a run that finds every stage kept (milliseconds).
const KEPT_BUDGET_MS: f64 = 1.0;

/// What the rows measured when they were added (release, 12-core Apple Silicon, the median of
/// three; milliseconds): on 2026-11-11 at a load of about 4, the badge's first run 228, a thread
/// chosen 93.4, six colours to five 222, nothing changed 0.00 (not budgeted from it), the
/// 1,500-pixel badge in logo mode 564, its painted raster 0.91; on 2026-11-13 at a load of about
/// 2.6, a fabric chosen 8.43 (the objects and their stitches made again) and the outlines and the
/// border turned on 1.13 (the planning service the window shares already holds the areas' plans, so
/// only the outline objects are planned, an object a line).
const MEASURED_MS: [f64; 8] = [228.0, 93.4, 222.0, 0.0, 564.0, 0.91, 8.43, 1.13];

/// The large badge's resolution (pixels a millimetre): 1,500 pixels over its 108 mm with its
/// margins, the working copy's largest side.
const LARGE_PX_PER_MM: f64 = 1500.0 / 108.0;

/// One of the badge's colours, given a thread of its own in the thread row: its navy.
const NAVY: &str = "#1d2a6b";

/// A request for the image `id` of `s` digitized with `params` (the document settings the window
/// changes under `document`, as `embroidery.setup` takes them; none without it), the document's
/// palette.
fn request(s: &Session, id: NodeId, params: &Value) -> Result<vectorcraft_engine::cmd::embroidery::DigitizeRequest, String> {
    let mut settings = params.clone();
    let document = settings.as_object_mut().and_then(|o| o.remove(DOCUMENT)).unwrap_or(Value::Null);
    s.digitize_window_request(id, &settings, None, &document).map_err(err)
}

/// The key the rows give the window's document settings under, beside digitizing's own.
const DOCUMENT: &str = "document";

/// The image selected in `s`.
fn image(s: &Session) -> Result<NodeId, String> {
    s.doc().map_err(err)?.selection.objects.first().copied().ok_or_else(|| "perf: no image selected".to_string())
}

/// A fresh pipeline's first run of `first` (untimed when `then` is given), then, timed, the run of
/// `then`: milliseconds, the median of [`RUNS`].
fn timed(s: &Session, first: &Value, then: Option<&Value>) -> Result<f64, String> {
    let id = image(s)?;
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let mut pipe = s.digitize_pipeline();
        let req = request(s, id, first)?;
        let timed = match then {
            None => req,
            Some(p) => {
                pipe.run(&req, &|_, _| true, &mut |_, _, _| {}).map_err(err)?;
                request(s, id, p)?
            }
        };
        let t = Instant::now();
        pipe.run(&timed, &|_, _| true, &mut |_, _, _| {}).map_err(err)?;
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(middle(samples))
}

/// The window rows' milliseconds, in the order of [`ROWS`].
pub(super) fn times() -> Result<Vec<f64>, String> {
    let mut ms = Vec::with_capacity(ROWS.len());
    let s = placed_badge(&badge_at(super::digitizing::PX_PER_MM)?, super::digitizing::PX_PER_MM)?;
    let base = json!({"colors": BADGE_COLORS});
    ms.push(timed(&s, &base, None)?);
    let chosen = json!({"colors": BADGE_COLORS, "colorChoices": [{"color": NAVY, "thread": {"palette": "jef", "index": 1}}]});
    ms.push(timed(&s, &base, Some(&chosen))?);
    ms.push(timed(&s, &base, Some(&json!({"colors": BADGE_COLORS - 1})))?);
    // Nothing changed: the same request run again on the same pipeline.
    let id = image(&s)?;
    let mut pipe = s.digitize_pipeline();
    let req = request(&s, id, &base)?;
    pipe.run(&req, &|_, _| true, &mut |_, _, _| {}).map_err(err)?;
    let mut kept = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        pipe.run(&req, &|_, _| true, &mut |_, _, _| {}).map_err(err)?;
        kept.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.push(middle(kept));
    let large = placed_badge(&badge_at(LARGE_PX_PER_MM)?, LARGE_PX_PER_MM)?;
    ms.push(timed(&large, &json!({"colors": BADGE_COLORS, "mode": "logo"}), None)?);
    let id = image(&large)?;
    let out = large.digitize_pipeline().run(&request(&large, id, &base)?, &|_, _| true, &mut |_, _, _| {}).map_err(err)?;
    let mut painted = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let t = Instant::now();
        let (w, h, rgba) = out.painted_rgba().ok_or("perf: the large badge has no colours")?;
        if rgba.len() != w as usize * h as usize * 4 {
            return Err("perf: the painted raster isn't the working copy's size".into());
        }
        painted.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    ms.push(middle(painted));
    ms.push(timed(&s, &base, Some(&json!({"colors": BADGE_COLORS, DOCUMENT: {"fabric": "denim"}})))?);
    ms.push(timed(&s, &base, Some(&json!({"colors": BADGE_COLORS, "colorOutlines": true, "border": true})))?);
    Ok(ms)
}
