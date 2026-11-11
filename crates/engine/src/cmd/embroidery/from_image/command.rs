//! `embroidery.digitizeImage`: an image (or an Image Trace object's source image) digitized with
//! the settings given, and made into embroidery objects (`super::create`), or answered without
//! changing anything (`dryRun`).
//!
//! Its answer gives what a window shows: the working copy the image was measured at, the colours
//! found with their threads, the areas to sew in their order, the design's figures as
//! `embroidery.stats` gives them, the warnings, the settings used with their defaults, and how long
//! each stage took. It is prepared on the document, run and applied in one go ([`DigitizeJob`]);
//! the image window runs the same stages through a pipeline that keeps them between runs
//! (`super::pipeline`), and its Create makes the objects the command makes (`super::window`).

use std::sync::Arc;

use serde_json::{Map, Value, json};
use vectorcraft_doc::{Document, ImageObject, NodeId, NodeKind};
use vectorcraft_geom::Point;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::{Match, MatchReason, Palette, mismatched, shown_delta_e};

use super::background::Background;
use super::colours::Colours;
use super::create::{self, Made};
use super::rules;
use super::settings::{BackgroundSetting, C, Settings, table};
use super::stages::{self, Digitized, clock, since};
use super::trace;
use super::warnings::Warning;
use crate::cmd::bad;
use crate::cmd::buildcmds::TRACE_NAME;
use crate::{EngineError, Result, Session};

/// What digitizing reports to a progress sink as it goes ([`vectorcraft_doc::cancel::progress`]):
/// the stages done of [`DIGITIZE_STAGES`]; the stitches' plan reports its own progress as a plan's.
pub(super) const DIGITIZE_PROGRESS: &str = "digitizeImage";

/// The stages a run goes through, as its progress counts them: the working copy, the trace, the
/// colours and areas, the shapes, the objects and their stitches.
pub(super) const DIGITIZE_STAGES: u64 = 5;

/// The undo step Create makes, as the command is named: its label, which `cmd!` takes only as a
/// literal (a test checks the two agree).
pub(super) const LABEL: &str = "Digitize Image";

/// The most areas a design is made of: past them, digitizing is refused with advice (an untrusted
/// image of noise makes hundreds of thousands). An estimate: the real designs' artwork has at most
/// 18 regions a design, a busy logo some hundreds.
pub(super) const MAX_IMAGE_AREAS: usize = 10_000;

// The areas a reply lists unless `limit` says otherwise, and the most it lists: as
// `embroidery.sequence` lists its units.
use super::super::stats::{LIST_LIMIT as AREAS_LISTED, MAX_LIST_LIMIT as MAX_AREAS_LISTED};

/// Hundredths: millimetres, square millimetres and shares are given to two decimals.
const HUNDREDTHS: f64 = 100.0;

/// A share is given as a percentage.
pub(super) const PERCENT: f64 = 100.0;

/// `v` to two decimals, as the replies give millimetres and shares.
pub(super) fn hundredths(v: f64) -> f64 {
    (v * HUNDREDTHS).round() / HUNDREDTHS
}

/// Whether flag `key` of `o` is on (left out or null: off).
fn flag(o: &Map<String, Value>, key: &str) -> Result<bool> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("`{key}` {v}: true or false"))),
    }
}

/// A whole number `o` gives `key`, within `0..=max`, or `default`.
fn count(o: &Map<String, Value>, key: &str, default: usize, max: usize) -> Result<usize> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= max)
            .ok_or_else(|| bad(C, format!("`{key}` {v}: a whole number from 0 to {max}"))),
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

/// Refused unless image `id` may be replaced by a design: Global Edit off, and neither the image
/// nor a group or layer it lies in locked, a template layer aside (locked so that a picture to
/// work over stays still, as File › Place makes one), as Convert to Manual Stitches and Nearest
/// Connection leave locked objects alone.
fn editable(st: &crate::DocState, id: NodeId) -> Result<()> {
    if st.global_edit.is_some() {
        return Err(bad(C, super::super::GLOBAL_EDIT_ON));
    }
    let ancestry = st.doc.ancestry(id).ok_or(EngineError::NoNode(id))?;
    let locked = |n: &vectorcraft_doc::Node| n.locked && !matches!(n.kind, NodeKind::Layer { template: true, .. });
    if ancestry.iter().any(|&a| st.doc.node(a).is_some_and(locked)) {
        return Err(bad(C, "the image is locked, or lies in a locked group or layer: unlock it first"));
    }
    Ok(())
}

/// Digitize `image`, whose file is `bytes`, with `settings` in `palette`'s threads: the working
/// copy, its background, the trace, the colours and their threads, the areas after the rules, and
/// the order they are sewn in, each stage in turn ([`stages`]). A cancel stops it within each
/// stage; `steps` is asked as the trace goes, from the threads that trace, and stops it when it
/// answers `false`.
pub(super) fn digitized(image: &ImageObject, bytes: &[u8], settings: &Settings, palette: Palette, steps: &trace::Steps<'_>) -> Result<Digitized> {
    let image_out = Arc::new(stages::image_stage(image, bytes, settings)?);
    let trace_out = Arc::new(stages::trace_stage(&image_out, settings, steps)?);
    let regions_out = Arc::new(stages::regions_stage(&image_out, &trace_out, settings)?);
    let colours_out = Arc::new(stages::colours_stage(&image_out, &trace_out, &regions_out, settings, palette)?);
    let lines_out = Arc::new(stages::lines_stage(&image_out, &colours_out)?);
    let shapes_out = Arc::new(stages::shapes_stage(&image_out, &trace_out, &colours_out, &lines_out, settings)?);
    vectorcraft_doc::cancel::progress(stages::MAKING, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
    Ok(Digitized { image_out, trace_out, regions_out, colours_out, lines_out, shapes_out })
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

/// The colour list's rows as the reply gives them: the background's, and each colour with areas.
fn colors_json(colours: &Colours, palette: Palette, px_per_mm2: f64, frame_px: f64) -> Vec<Value> {
    let sewn_px: usize = colours.rows.iter().filter(|r| r.sewn()).map(|r| r.pixels).sum();
    colours
        .rows
        .iter()
        .filter(|r| r.background || r.areas > 0)
        .map(|r| {
            let merged = r.merged_into.and_then(|j| colours.rows.get(j)).map(|t| super::super::hex(t.color));
            // Merged by sharing its only near thread, or because a person asked.
            let reason = match (&r.thread, &merged) {
                (Some(m), Some(_)) if matches!(m.reason, MatchReason::Shared { .. }) => Some("shared"),
                (_, Some(_)) => Some("merged"),
                (Some(m), None) => Some(m.reason.id()),
                (None, None) => None,
            };
            let by = r.taken_by.and_then(|j| colours.rows.get(j)).map(|t| super::super::hex(t.color));
            json!({
                "color": super::super::hex(r.color),
                "background": r.background,
                "sewn": r.sewn(),
                "job": r.job.id(),
                "thread": r.thread.as_ref().map(|m| thread_json(palette, m.thread)),
                "reason": reason,
                "by": by,
                "alternatives": r.thread.as_ref().map_or(vec![], |m| m.alternatives.iter().map(|&a| thread_json(palette, a)).collect()),
                "area": hundredths(r.pixels as f64 / px_per_mm2),
                "areaCount": r.areas,
                "share": hundredths(r.pixels as f64 / frame_px.max(1.0) * PERCENT),
                "sewnShare": r.sewn().then(|| hundredths(r.pixels as f64 / sewn_px.max(1) as f64 * PERCENT)),
                "mergedInto": merged,
                "choice": r.choice,
            })
        })
        .collect()
}

/// How many threads the colours sewn are sewn in: one thread once, however many colours it sews
/// (two colours a person chose one thread for are one colour in the design).
fn thread_count(colours: &Colours) -> usize {
    let mut threads: Vec<usize> =
        colours.rows.iter().filter(|r| r.sewn() && r.areas > 0).filter_map(|r| r.thread.as_ref()).map(|m| m.thread.index).collect();
    threads.sort_unstable();
    threads.dedup();
    threads.len()
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

/// The most path elements (moves, lines, curves, closes) of outlines a reply gives: past them the
/// areas listed end, `truncated` saying more follow, so that a reply stays about a megabyte
/// whatever the picture (each element some fifty characters). An estimate: the real designs'
/// artwork and the test logos are drawn in at most a few thousand.
pub(super) const MAX_OUTLINE_ELEMENTS: usize = 20_000;

/// Area `a`'s shape as sewn (a line's centre line), as an SVG path in millimetres from the
/// document's origin, each coordinate to hundredths as the reply gives millimetres; and how many
/// path elements it has.
fn outline_of(d: &Digitized, a: usize) -> Option<(String, usize)> {
    use vectorcraft_geom::kurbo::{BezPath, PathEl};
    let to_mm = vectorcraft_geom::Affine::scale(MM_PER_PT) * d.working().to_doc;
    let path = match d.lines().get(a).and_then(Option::as_ref) {
        Some(line) => to_mm * line.path(),
        None => d.shapes().shapes.get(a)?.transformed(to_mm).to_bezpath(),
    };
    let r = |p: Point| Point::new(hundredths(p.x), hundredths(p.y));
    let rounded: BezPath = path
        .elements()
        .iter()
        .map(|&el| match el {
            PathEl::MoveTo(p) => PathEl::MoveTo(r(p)),
            PathEl::LineTo(p) => PathEl::LineTo(r(p)),
            PathEl::QuadTo(p, q) => PathEl::QuadTo(r(p), r(q)),
            PathEl::CurveTo(p, q, e) => PathEl::CurveTo(r(p), r(q), r(e)),
            PathEl::ClosePath => PathEl::ClosePath,
        })
        .collect();
    Some((rounded.to_svg(), rounded.elements().len()))
}

/// The areas `offset` to `offset + limit` in sewing order, as the reply lists them, with their
/// outlines when `outlines` asks: then at most [`MAX_OUTLINE_ELEMENTS`] path elements of them, the
/// list ending at the area that would pass it (the first one listed whatever it has).
fn areas_json(d: &Digitized, palette: Palette, offset: usize, limit: usize, outlines: bool) -> Vec<Value> {
    let w = d.working();
    let px = w.px_per_mm;
    let number = |a: usize| d.order().placed.get(a).and_then(|p| p.index).map(|i| i + 1);
    let mm = |p: Point| [hundredths(p.x * MM_PER_PT), hundredths(p.y * MM_PER_PT)];
    let mut elements = 0usize;
    d.order()
        .sewn
        .iter()
        .skip(offset)
        .take(limit)
        .map(|&a| (a, outlines.then(|| outline_of(d, a)).flatten()))
        .take_while(|(_, outline)| {
            let n = outline.as_ref().map_or(0, |(_, n)| *n);
            let first = elements == 0;
            elements = elements.saturating_add(n);
            first || elements <= MAX_OUTLINE_ELEMENTS
        })
        .filter_map(|(a, outline)| {
            let area = d.areas().areas.get(a)?;
            let stats = d.areas().regions.areas.get(a)?;
            let placed = d.order().placed.get(a)?;
            let own = placed.index? + 1;
            let row = d.colours().rows.get(area.row)?;
            let (x, y) = stats.deepest;
            let at = w.to_image * Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let [x0, y0, x1, y1] = stats.bbox.map(f64::from);
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| w.to_doc * Point::new(x, y));
            let (l, t) = corners.iter().fold((f64::INFINITY, f64::INFINITY), |(l, t), p| (l.min(p.x), t.min(p.y)));
            let (r, b) = corners.iter().fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |(r, b), p| (r.max(p.x), b.max(p.y)));
            let bounds: Vec<f64> = [mm(Point::new(l, t)), mm(Point::new(r, b))].concat();
            let touches: Vec<Value> = d
                .areas()
                .regions
                .neighbours(a as u32)
                .iter()
                .filter_map(|&(o, edge)| {
                    let o = o as usize;
                    Some(json!({"area": number(o)?, "edge": hundredths(f64::from(edge) / px), "under": placed.under.contains(&o)}))
                })
                .collect();
            Some(json!({
                "area": own,
                "at": [hundredths(at.x), hundredths(at.y)],
                "color": super::super::hex(row.color),
                "thread": row.thread.as_ref().map(|m| thread_json(palette, m.thread)),
                "kind": area.kind.id(),
                "sewn": row.sewn(),
                "size": {
                    "area": hundredths(area.pixels as f64 / (px * px)),
                    "width": hundredths((x1 - x0) / px),
                    "height": hundredths((y1 - y0) / px),
                },
                "widest": hundredths(stats.widest_px / px),
                "line": d.lines().get(a).and_then(Option::as_ref).map(|l| json!({
                    "width": hundredths(l.width_px / px),
                    "length": hundredths(l.length_px / px),
                    "pieces": l.pieces.len(),
                    "ends": l.ends,
                    "junctions": l.junctions,
                })),
                "box": bounds,
                "inside": area.inside.and_then(number),
                "sewnOver": area.sewn_over,
                "joined": area.paths.len().saturating_sub(1),
                "touches": touches,
                "grownUnder": d.shapes().grown_under.get(a).map_or(vec![], |g| g.iter().filter_map(|&b| number(b)).collect::<Vec<_>>()),
                "outline": outline.map(|(svg, _)| svg),
                "order": {
                    "taken": placed.taken.id(),
                    "after": placed.after.iter().filter_map(|&(o, why)| Some(json!({"area": number(o)?, "because": why.id()}))).collect::<Vec<_>>(),
                },
            }))
        })
        .collect()
}

/// The region map as a reply gives it: each working pixel's area by its number in sewing order (0
/// for none), two bytes little-endian a pixel, in base64.
fn regions_json(d: &Digitized) -> Value {
    let regions = &d.areas().regions;
    let mut bytes = Vec::with_capacity(regions.labels.len() * 2);
    for &l in &regions.labels {
        let n = d.order().placed.get(l as usize).and_then(|p| p.index).and_then(|i| u16::try_from(i + 1).ok()).unwrap_or(0);
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    json!({"width": regions.w, "height": regions.h, "base64": vectorcraft_format::base64_encode(&bytes)})
}

/// The point `o` gives as `at` (`[x, y]` in the image's own pixels), if any.
fn point(o: &Map<String, Value>) -> Result<Option<Point>> {
    match o.get("at") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_array()
            .filter(|a| a.len() == 2)
            .and_then(|a| Some(Point::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .map(Some)
            .ok_or_else(|| bad(C, format!("`at` {v}: [x, y] in the image's own pixels"))),
    }
}

/// The number in sewing order of the area under `at` (the image's own pixels), if one lies there.
fn area_at(d: &Digitized, at: Point) -> Option<usize> {
    let p = d.working().to_image.inverse() * at;
    if !(p.x >= 0.0 && p.y >= 0.0) {
        return None;
    }
    let a = d.areas().regions.at(p.x.floor() as u32, p.y.floor() as u32)?;
    d.order().placed.get(a as usize).and_then(|placed| placed.index).map(|i| i + 1)
}

/// What a reply lists besides the design: the stretch of areas, whether the region map and the
/// outlines are given, and the point whose area is asked.
#[derive(Clone, Copy, Debug)]
pub(super) struct Listed {
    pub(super) offset: usize,
    pub(super) limit: usize,
    pub(super) regions: bool,
    pub(super) outlines: bool,
    pub(super) at: Option<Point>,
}

/// Digitizing asked for ([`DigitizeJob::run`]): the document as it was (its uid and snapshot), the
/// image (`target`, an image or an Image Trace object) and its file, the settings in the document's
/// palette, what the reply lists, and whether it only answers.
struct DigitizeJob {
    uid: u64,
    original: Arc<Document>,
    target: NodeId,
    image: ImageObject,
    bytes: Arc<Vec<u8>>,
    settings: Settings,
    palette: Palette,
    listed: Listed,
    dry_run: bool,
}

/// What a run made ([`DigitizeJob::run`]), to apply to the document it was made of ([`apply`]):
/// the document with the objects made and what was made (`None`: nothing to make, which only a dry
/// run answers), and the reply.
struct DigitizeFound {
    uid: u64,
    original: Arc<Document>,
    made: Option<(Document, Made)>,
    reply: Value,
    dry_run: bool,
}

/// `embroidery.stats`, whose reply the dry run gives, planning the selection alone
/// ([`SELECTED_ONLY`]).
const STATS: &str = "embroidery.stats";
use super::super::SELECTED_ONLY;

/// What `embroidery.stats {selectedOnly: true}` answers for `doc` with `group` selected: the plan
/// of the group alone, in its layers, made afresh by the same code (the export's plan, which a
/// planning service gives too).
fn stats_of(doc: &Document, group: NodeId) -> Result<Value> {
    let ask = super::super::read_params(STATS, &json!({ SELECTED_ONLY: true }))?;
    let mut st = crate::DocState::new(doc.clone(), None);
    st.selection.set([group]);
    let picked = super::super::to_plan(&st, ask.selected_only)?;
    let plan = super::super::service::plan_afresh(&picked, &ask.opts).map_err(super::super::PlanError::error)?;
    super::super::reply(ask.read, &plan, &picked, &ask.opts)
}

/// The job `p` asks for on the active document: the params checked, the image and its file taken.
fn prepare(s: &Session, p: &Value) -> Result<DigitizeJob> {
    let o = super::super::object(p, C)?;
    let st = s.doc()?;
    let palette = super::super::thread_palette(&st.doc);
    let settings = Settings::from_params(&o, palette)?;
    let dry_run = flag(&o, "dryRun")?;
    let listed = Listed {
        offset: count(&o, "offset", 0, usize::MAX)?,
        limit: count(&o, "limit", AREAS_LISTED, MAX_AREAS_LISTED)?,
        regions: flag(&o, "regions")?,
        outlines: flag(&o, "outlines")?,
        at: point(&o)?,
    };
    let (target, image, bytes) = image_of(s, &o)?;
    Ok(DigitizeJob { uid: st.uid, original: st.doc.clone(), target, image, bytes, settings, palette, listed, dry_run })
}

/// The image `o` names (or the one selected), with its file: refused as the command refuses it (no
/// image, a locked one, Global Edit on, a missing linked file).
pub(super) fn image_of(s: &Session, o: &Map<String, Value>) -> Result<(NodeId, ImageObject, Arc<Vec<u8>>)> {
    let st = s.doc()?;
    let (target, image) = target(s, o)?;
    editable(st, target)?;
    let blob = st.doc.images.get(&image.key).ok_or_else(|| bad(C, format!("the image's data `{}` is missing", image.key)))?;
    // While a linked file can't be read, the document holds only its preview, a few hundred pixels
    // a side: a design is never made of that (the project owner's decision, 2026-11-03).
    if blob.is_proxy() {
        let file = image.link.as_ref().map_or_else(String::new, |l| format!(" ({})", l.path));
        return Err(bad(C, format!("the image's linked file is missing{file}: relink it (links.relink) to digitize it, not its preview")));
    }
    Ok((target, image, Arc::clone(&blob.bytes)))
}

impl DigitizeJob {
    /// Digitizing, on the document as it was: the image digitized, its objects made on a copy and
    /// planned for their figures. Inside a [`vectorcraft_doc::cancel::scope`], a cancel stops it
    /// within each stage (an error then, nothing made), and the stages' progress goes to the
    /// scope's sink ([`DIGITIZE_PROGRESS`]); `steps` is asked as the trace goes, from the threads
    /// that trace it, which a scope's cancel doesn't reach, as Image Trace's own run asks it, and
    /// stops the run when it answers `false`.
    fn run(&self, steps: &(dyn Fn(u64, u64) -> bool + Sync)) -> Result<DigitizeFound> {
        let Self { uid, original, target, image, bytes, settings, palette, listed, dry_run } = self;
        let d = digitized(image, bytes, settings, *palette, steps)?;
        let started = clock();
        let mut doc = Document::clone(original);
        let mut warnings = d.warnings();
        let made = match create::made(&mut doc, *target, &d, *palette)? {
            Some(m) => Some((doc, m)),
            None if *dry_run => {
                warnings.push(Warning::new("nothingToMake", json!({}), NOTHING_TO_MAKE));
                None
            }
            None => return Err(bad(C, NOTHING_TO_MAKE)),
        };
        let objects_ms = since(started);
        let started = clock();
        let stats = match &made {
            Some((doc, m)) => stats_of(doc, m.group)?,
            None => Value::Null,
        };
        let stitches_ms = since(started);
        vectorcraft_doc::cancel::progress(DIGITIZE_STAGES, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
        let of = ReplyOf {
            id: *target,
            settings,
            palette: *palette,
            listed: *listed,
            dry_run: *dry_run,
            stats,
            warnings,
            made_ms: [objects_ms, stitches_ms],
        };
        let reply = reply_json(&d, of);
        Ok(DigitizeFound { uid: *uid, original: original.clone(), made, reply, dry_run: *dry_run })
    }
}

/// Said when every area of the image is its background.
pub(super) const NOTHING_TO_MAKE: &str =
    "the image makes nothing to sew: every area of it is its background (give `background: \"none\"` to sew it all)";

/// What a reply is made of besides what digitizing made.
pub(super) struct ReplyOf<'a> {
    pub(super) id: NodeId,
    pub(super) settings: &'a Settings,
    pub(super) palette: Palette,
    pub(super) listed: Listed,
    pub(super) dry_run: bool,
    pub(super) stats: Value,
    pub(super) warnings: Vec<Warning>,
    /// How long making the objects took, and planning their stitches (milliseconds).
    pub(super) made_ms: [Option<f64>; 2],
}

/// The command's reply for what `d` made (`created` null: [`apply`] fills it in).
pub(super) fn reply_json(d: &Digitized, of: ReplyOf) -> Value {
    let timings = d.timings();
    let ReplyOf { id, settings, palette, listed, dry_run, stats, mut warnings, made_ms: [objects_ms, stitches_ms] } = of;
    let w = d.working();
    let px_per_mm2 = w.px_per_mm * w.px_per_mm;
    let frame_px = f64::from(w.raster.width) * f64::from(w.raster.height);
    let unused: Vec<Value> = d.colours().unused.iter().map(|&(i, why)| json!({"choice": i, "reason": why})).collect();
    let areas = areas_json(d, palette, listed.offset, listed.limit, listed.outlines);
    let area_count = d.order().sewn.len();
    let specks = d.areas().joined.saturating_sub(d.areas().bands);
    if specks > 0 {
        warnings.push(Warning::new(
            "specksJoined",
            json!({"count": specks, "minArea": settings.min_area}),
            format!("{specks} areas smaller than {} mm² joined their neighbours", settings.min_area),
        ));
    }
    if d.areas().bands > 0 {
        warnings.push(Warning::new(
            "bandsJoined",
            json!({"count": d.areas().bands, "maxWidth": rules::SLIVER_MAX_MM}),
            format!(
                "{} bands at most {} mm wide, in a colour between their neighbours', joined the neighbour nearest their colour",
                d.areas().bands,
                rules::SLIVER_MAX_MM
            ),
        ));
    }
    if d.areas().dropped > 0 {
        warnings.push(Warning::new(
            "specksLeftOut",
            json!({"count": d.areas().dropped, "minArea": settings.min_area}),
            format!("{} areas smaller than {} mm² had nothing to join and were left out", d.areas().dropped, settings.min_area),
        ));
    }
    json!({
        "dryRun": dry_run,
        "created": Value::Null,
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
            "background": background_json(d.found(), settings.background, d.background()),
        },
        "colorCount": thread_count(d.colours()),
        "colors": colors_json(d.colours(), palette, px_per_mm2, frame_px),
        "unusedChoices": unused,
        "areaCount": area_count,
        "offset": listed.offset,
        "truncated": listed.offset.saturating_add(areas.len()) < area_count,
        "areas": areas,
        "areaAt": listed.at.and_then(|p| area_at(d, p)),
        "traced": {"paths": d.traced().paths.len(), "colors": d.traced().palette.len()},
        "doubledArea": hundredths(d.shapes().doubled_px / px_per_mm2),
        "joined": d.areas().joined,
        "leftOut": d.areas().dropped,
        "regions": if listed.regions { regions_json(d) } else { Value::Null },
        "stats": stats,
        "warnings": warnings.iter().map(Warning::json).collect::<Vec<_>>(),
        "settings": settings.echo(),
        "table": table(palette),
        "timings": {"image": timings[0], "trace": timings[1], "areas": timings[2], "shapes": timings[3], "objects": objects_ms, "stitches": stitches_ms},
    })
}

/// What `found` made applied to its document as one undo step (the new group selected), unless it
/// is a dry run, and the reply: refused when the document changed since the job took it (it was
/// made of an earlier one, so nothing is applied).
fn apply(s: &mut Session, found: DigitizeFound) -> Result<Value> {
    let DigitizeFound { uid, original, made, mut reply, dry_run } = found;
    if dry_run {
        return Ok(reply);
    }
    let Some((doc, made)) = made else { return Err(bad(C, NOTHING_TO_MAKE)) };
    put_in_place(s, uid, &original, doc, made)?;
    reply["created"] = created_json(made);
    Ok(reply)
}

/// The reply's `created`: what Create made.
pub(super) fn created_json(made: Made) -> Value {
    json!({"group": made.group.0, "layer": made.layer.0, "objects": made.objects, "blocks": made.blocks})
}

/// `doc`, with the objects `made`, put in place of document `uid` as one undo step "Digitize Image"
/// (the new group selected): refused when the document isn't `original` any more (it was made of
/// an earlier one, so nothing is applied), or while Global Edit is on.
pub(super) fn put_in_place(s: &mut Session, uid: u64, original: &Arc<Document>, doc: Document, made: Made) -> Result<()> {
    let st = s.doc()?;
    if st.uid != uid || !Arc::ptr_eq(&st.doc, original) {
        return Err(bad(C, "the document changed while the image was digitized: nothing was made"));
    }
    if st.global_edit.is_some() {
        return Err(bad(C, super::super::GLOBAL_EDIT_ON));
    }
    s.edit(LABEL, |d, sel| {
        *d = doc;
        sel.set([made.group]);
        Ok(())
    })?;
    Ok(())
}

/// `embroidery.digitizeImage`: prepared, run and applied in one go ([`DigitizeJob`]).
pub(in crate::cmd::embroidery) fn digitize_image(s: &mut Session, p: &Value) -> Result<Value> {
    let job = prepare(s, p)?;
    // On this thread, where a cancel reaches every stage but the trace's colour layers.
    let found = job.run(&|_, _| true)?;
    apply(s, found)
}
