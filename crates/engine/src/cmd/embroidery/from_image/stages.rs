//! Digitizing's stages, each a function of what it reads: the outputs of the stages before it and
//! the settings it takes, never more. [`super::command::digitized`] runs them in turn; a window can
//! keep each stage's output and run again only the stages after a change.
//!
//! - [`image_stage`]: the working copy and what lies round the art;
//! - [`trace_stage`]: upstream's Image Trace of the working copy;
//! - [`regions_stage`]: the traced shapes drawn back into the working copy's pixels, and the
//!   traced colour that is the background;
//! - [`colours_stage`]: the colours and their threads, the areas after the rules;
//! - [`lines_stage`]: the centre lines, what lies inside what, and the sewing order;
//! - [`shapes_stage`]: the shapes, each grown under the neighbours sewn after it.
//!
//! [`Digitized`] holds the six outputs and reads them as one result.

use std::sync::Arc;

use serde_json::json;
use vectorcraft_doc::ImageObject;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace as tr;

use super::background::{self, Background};
use super::centre::{self, CentreLine};
use super::colours::{self, Colours};
use super::command::{DIGITIZE_PROGRESS, DIGITIZE_STAGES, MAX_IMAGE_AREAS, PERCENT};
use super::order::{self, Order};
use super::overlap::{self, Shapes};
use super::regions::Regions;
use super::rules::{self, AreaKind, Areas};
use super::settings::{BackgroundSetting, C, Job, Mode, Settings};
use super::warnings::Warning;
use super::working::Working;
use super::{flat, inside, trace, working};
use crate::Result;
use crate::cmd::bad;

/// The stages done when each stage of [`DIGITIZE_STAGES`] begins: the working copy, the trace, the
/// colours and areas, the shapes, and the objects with their stitches.
pub(super) const PREPARING: u64 = 0;
pub(super) const TRACING: u64 = 1;
pub(super) const COLOURING: u64 = 2;
pub(super) const SHAPING: u64 = 3;
pub(super) const MAKING: u64 = 4;

/// Milliseconds in a second, for the stages' timings.
#[cfg(not(target_arch = "wasm32"))]
const MS_PER_SEC: f64 = 1000.0;

/// The clock a stage's time is read from; none on the web.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn clock() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn clock() {}

/// The time since `started` (milliseconds); none on the web, which has no clock to read.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn since(started: std::time::Instant) -> Option<f64> {
    Some(super::command::hundredths(started.elapsed().as_secs_f64() * MS_PER_SEC))
}

#[cfg(target_arch = "wasm32")]
pub(super) fn since(_: ()) -> Option<f64> {
    None
}

/// Two stretches' times added (milliseconds, to hundredths); none on the web.
pub(super) fn added(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    Some(super::command::hundredths(a? + b?))
}

/// The image stage's output: the working copy, and what lies round the art.
pub(super) struct ImageOut {
    pub(super) working: Working,
    pub(super) found: Background,
    /// What the working copy warns of (a coarse picture, one decoded smaller).
    pub(super) warnings: Vec<Warning>,
    /// How long it took (milliseconds), none on the web.
    pub(super) ms: Option<f64>,
}

/// The working copy of `image`, whose file is `bytes`, at the size and crop `settings` ask, and its
/// background as `settings` say where to find it.
pub(super) fn image_stage(image: &ImageObject, bytes: &[u8], settings: &Settings) -> Result<ImageOut> {
    vectorcraft_doc::cancel::progress(PREPARING, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let working = working::prepare(image, bytes, settings)?;
    let found = match settings.background {
        BackgroundSetting::Corners => background::found(working.upright()),
        BackgroundSetting::None => Background::None,
        BackgroundSetting::Colour(c) => Background::Colour(c),
    };
    let ms = since(started);
    let warnings = working.warnings();
    Ok(ImageOut { working, found, warnings, ms })
}

/// The trace stage's output.
pub(super) struct TraceOut {
    pub(super) traced: tr::TraceResult,
    /// What the trace warns of (fewer colours than asked, the anchors' limit).
    pub(super) warnings: Vec<Warning>,
    pub(super) ms: Option<f64>,
}

/// The working copy traced as `settings` ask (their mode, colours, smoothing, corners and smallest
/// area): `steps` is asked as the trace goes, from the threads that trace, and stops it when it
/// answers `false`.
pub(super) fn trace_stage(image: &ImageOut, settings: &Settings, steps: &trace::Steps<'_>) -> Result<TraceOut> {
    vectorcraft_doc::cancel::check()?;
    vectorcraft_doc::cancel::progress(TRACING, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    // A background colour takes one of the trace's colours: the design keeps the colours asked.
    let colors = settings.colors + u32::from(matches!(image.found, Background::Colour(_)));
    let mut warnings = Vec::new();
    let traced = trace::in_mode(&image.working, settings, colors, trace::MAX_ANCHORS, steps, &mut warnings)?;
    if traced.paths.len() > trace::MAX_TRACED_SHAPES {
        return Err(bad(
            C,
            format!(
                "the image traces as {} shapes, more than {}: lower `colors`, give a smaller size (`width`, `height` or `fit`), or crop the image",
                traced.paths.len(),
                trace::MAX_TRACED_SHAPES
            ),
        ));
    }
    Ok(TraceOut { traced, warnings, ms: since(started) })
}

/// The regions stage's output.
pub(super) struct RegionsOut {
    pub(super) regions: Regions,
    /// The traced colour that is the background, if one is.
    pub(super) background: Option<[u8; 3]>,
    /// What it warns of (no traced colour is the background's, the corners disagree, a photo).
    pub(super) warnings: Vec<Warning>,
    pub(super) ms: Option<f64>,
}

/// The traced shapes drawn back into the working copy's pixels, and the traced colour that is the
/// background (as `settings` say where to find it, in their mode).
pub(super) fn regions_stage(image: &ImageOut, trace: &TraceOut, settings: &Settings) -> Result<RegionsOut> {
    vectorcraft_doc::cancel::check()?;
    vectorcraft_doc::cancel::progress(COLOURING, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let (w, traced) = (&image.working, &trace.traced);
    let mut warnings = Vec::new();
    let background = colours::background_colour(&traced.palette, image.found);
    if matches!(image.found, Background::Colour(_)) && background.is_none() {
        warnings.push(Warning::new("noBackgroundColor", json!({}), "no traced colour is the background's: every area is sewn"));
    }
    if settings.background == BackgroundSetting::Corners && image.found == Background::None {
        warnings.push(Warning::new(
            "cornersDisagree",
            json!({}),
            "the image's corners don't agree on a background: every area is sewn (give `background` to name one)",
        ));
    }
    let regions = Regions::of(traced, &w.raster, w.outside.clone())?;
    if settings.mode != Mode::Logo {
        let gradual = flat::gradual_share(&w.raster, &regions, traced, settings.mode == Mode::Grayscale);
        if gradual >= flat::GRADIENT_SHARE {
            let percent = (gradual * PERCENT).round();
            warnings.push(Warning::new(
                "notFlatArt",
                json!({"by": "gradual", "percent": percent}),
                format!(
                    "the image doesn't look like flat art (a photo or gradients?): {percent}% of the edges traced between its colours lie where it changes gradually, so its gradients are sewn as bands of flat colour"
                ),
            ));
        }
    }
    Ok(RegionsOut { regions, background, warnings, ms: since(started) })
}

/// The colours stage's output.
pub(super) struct ColoursOut {
    pub(super) colours: Colours,
    /// The areas after the rules, those of one thread joined, those of a colour sewn as Lines made
    /// lines.
    pub(super) areas: Areas,
    /// What it warns of (a chosen thread that isn't grey, a colour whose every area joined others).
    pub(super) warnings: Vec<Warning>,
    pub(super) ms: Option<f64>,
}

/// Each colour matched to a thread of `palette` (with the person's choices in `settings`), the
/// smallest area applied (`settings`' in square millimetres), and the areas of one thread joined.
pub(super) fn colours_stage(image: &ImageOut, trace: &TraceOut, regions: &RegionsOut, settings: &Settings, palette: Palette) -> Result<ColoursOut> {
    let started = clock();
    let (w, traced, map) = (&image.working, &trace.traced, &regions.regions);
    let mut warnings = Vec::new();
    let touches = |i: usize| map.areas.get(i).is_some_and(|a| a.touches_edge);
    let rows = colours::rows(traced, &touches, regions.background, &settings.choices);
    let row_of = rows.row_of(traced.paths.len());
    let min_px = trace::area_px(settings.min_area, w.px_per_mm) as usize;
    let areas = rules::applied(map.clone(), &rows, &row_of, w.px_per_mm, min_px)?;
    if areas.areas.len() > MAX_IMAGE_AREAS {
        return Err(bad(
            C,
            format!(
                "the image makes {} areas, more than {MAX_IMAGE_AREAS}: raise `minArea`, lower `colors`, give a smaller size (`width`, `height` or `fit`), or crop the image",
                areas.areas.len()
            ),
        ));
    }
    let area_rows: Vec<(usize, usize)> = areas.areas.iter().map(|a| (a.row, a.pixels)).collect();
    vectorcraft_doc::cancel::check()?;
    let colours = colours::matched(rows, &area_rows, settings.mode, palette, &settings.choices)?;
    // In greys mode a thread a person chose is sewn even when it isn't grey (the project owner's
    // decision, 2026-11-03): the reply says so.
    if settings.mode == Mode::Grayscale {
        for row in colours.rows.iter().filter(|r| r.areas > 0) {
            let chosen = row.choice.and_then(|i| settings.choices.get(i)).and_then(|c| c.thread);
            let Some(t) = chosen.filter(|t| !palette.is_neutral(t.index())) else { continue };
            let name = palette.get(t.index()).map_or("", |(_, n)| n);
            let color = super::super::hex(row.color);
            warnings.push(Warning::new(
                "choiceNotGray",
                json!({"color": color, "thread": {"palette": palette.id(), "index": t.index(), "name": name}}),
                format!(
                    "the thread chosen for {color}, {}, isn't grey: it is sewn as chosen",
                    super::super::threadlib::swatch_name(palette, t.index(), name)
                ),
            ));
        }
    }
    for row in colours.rows.iter().filter(|r| !r.background && r.areas == 0) {
        let color = super::super::hex(row.color);
        warnings.push(Warning::new(
            "colorJoined",
            json!({"color": color, "minArea": settings.min_area}),
            format!("every area of {color} was smaller than {} mm², so it joined its neighbours", settings.min_area),
        ));
    }
    let mut areas = rules::joined_by_thread(areas, &colours)?;
    rules::lines_by_job(&mut areas, &colours);
    Ok(ColoursOut { colours, areas, warnings, ms: since(started) })
}

/// The lines stage's output.
pub(super) struct LinesOut {
    /// The areas with what lies inside what and which islands are sewn over.
    pub(super) areas: Areas,
    pub(super) order: Order,
    /// Each area's centre line, where it is sewn along its middle ([`centre`]), by its index.
    pub(super) lines: Vec<Option<CentreLine>>,
    /// What it warns of (areas of a colour set to Lines with no centre line).
    pub(super) warnings: Vec<Warning>,
    /// How long the centre lines took, and what lies inside what with the order (milliseconds).
    pub(super) centre_ms: Option<f64>,
    pub(super) order_ms: Option<f64>,
}

/// The centre lines of the areas sewn along their middle, then what lies inside what and the order
/// the areas are sewn in.
pub(super) fn lines_stage(image: &ImageOut, colours: &ColoursOut) -> Result<LinesOut> {
    // A line along its middle: a thin one of a colour sewn by the rules (or kept unsewn, to be sewn
    // later), and every area of a colour sewn as Lines; not a colour sewn as Fill. Found before the
    // order, which sews a line after the fills it touches: an area with no centre line to sew along
    // is sewn as the shape it is, in its turn as a fill.
    vectorcraft_doc::cancel::progress(SHAPING, DIGITIZE_STAGES, DIGITIZE_PROGRESS);
    let started = clock();
    let px_per_mm = image.working.px_per_mm;
    let rows = &colours.colours;
    let mut areas = colours.areas.clone();
    let mut warnings = Vec::new();
    // A line by the rules is held to its width away from where its lines meet.
    let thin_px = rules::THIN_LINE_MM * px_per_mm + rules::MEASURE_SLACK_PX;
    let wanted: Vec<centre::Wanted> = areas
        .areas
        .iter()
        .map(|x| match rows.rows.get(x.row) {
            Some(r) if x.kind == AreaKind::Line && !r.background => match r.job {
                Job::Lines => centre::Wanted::AnyWidth,
                Job::Auto | Job::None => centre::Wanted::NoWiderThan(thin_px),
                Job::Fill => centre::Wanted::No,
            },
            _ => centre::Wanted::No,
        })
        .collect();
    let lines = centre::lines(&areas.regions, &|a| wanted.get(a).copied().unwrap_or(centre::Wanted::No), px_per_mm)?;
    let along: Vec<bool> = wanted.iter().map(|&x| x != centre::Wanted::No).collect();
    for (row, count) in rules::fills_without_centre_lines(&mut areas, rows, &along, &lines) {
        let color = rows.rows.get(row).map(|r| super::super::hex(r.color)).unwrap_or_default();
        warnings.push(Warning::new(
            "notLines",
            json!({"color": color, "count": count}),
            format!("areas of {color} with no centre line to sew along (a dot, or a blob shorter than it is wide) are sewn as the shapes they are: {count}"),
        ));
    }
    let centre_ms = since(started);
    let started = clock();
    inside::containers(&mut areas, rows, px_per_mm)?;
    let order = order::ordered(&areas.areas, &areas.regions, rows)?;
    inside::sewn_over(&mut areas, rows, &order);
    Ok(LinesOut { areas, order, lines, warnings, centre_ms, order_ms: since(started) })
}

/// The shapes stage's output.
pub(super) struct ShapesOut {
    pub(super) shapes: Shapes,
    /// What it warns of (areas path operations couldn't join or grow).
    pub(super) warnings: Vec<Warning>,
    pub(super) ms: Option<f64>,
}

/// Each area's shape, grown under the neighbours sewn after it by `settings`' overlap (and under a
/// centre line to its middle).
pub(super) fn shapes_stage(image: &ImageOut, trace: &TraceOut, colours: &ColoursOut, lines: &LinesOut, settings: &Settings) -> Result<ShapesOut> {
    let started = clock();
    let px_per_mm = image.working.px_per_mm;
    let shapes = overlap::shaped(
        &trace.traced.paths,
        &lines.areas,
        &colours.colours,
        &lines.order,
        settings.overlap * px_per_mm,
        &lines.lines,
        overlap::LINE_REACH_MM * px_per_mm,
    )?;
    let mut warnings = Vec::new();
    if shapes.failed > 0 {
        warnings.push(Warning::new(
            "shapesNotJoined",
            json!({"count": shapes.failed}),
            format!("{} areas couldn't be joined or grown by path operations and are sewn as traced", shapes.failed),
        ));
    }
    Ok(ShapesOut { shapes, warnings, ms: since(started) })
}

/// What digitizing made of an image, stage by stage: each stage's output, shared.
#[derive(Clone)]
pub(super) struct Digitized {
    pub(super) image_out: Arc<ImageOut>,
    pub(super) trace_out: Arc<TraceOut>,
    pub(super) regions_out: Arc<RegionsOut>,
    pub(super) colours_out: Arc<ColoursOut>,
    pub(super) lines_out: Arc<LinesOut>,
    pub(super) shapes_out: Arc<ShapesOut>,
}

impl Digitized {
    /// The working copy.
    pub(super) fn working(&self) -> &Working {
        &self.image_out.working
    }

    /// What lies round the art, as found.
    pub(super) fn found(&self) -> Background {
        self.image_out.found
    }

    /// What the trace made.
    pub(super) fn traced(&self) -> &tr::TraceResult {
        &self.trace_out.traced
    }

    /// The traced colour that is the background, if one is.
    pub(super) fn background(&self) -> Option<[u8; 3]> {
        self.regions_out.background
    }

    /// The colours and their threads.
    pub(super) fn colours(&self) -> &Colours {
        &self.colours_out.colours
    }

    /// The areas, with what lies inside what.
    pub(super) fn areas(&self) -> &Areas {
        &self.lines_out.areas
    }

    /// The order the areas are sewn in.
    pub(super) fn order(&self) -> &Order {
        &self.lines_out.order
    }

    /// Each area's centre line, where it is sewn along its middle, by its index.
    pub(super) fn lines(&self) -> &[Option<CentreLine>] {
        &self.lines_out.lines
    }

    /// The shapes the areas are sewn as.
    pub(super) fn shapes(&self) -> &Shapes {
        &self.shapes_out.shapes
    }

    /// What the stages warn of, in the order they warned; and that nothing was traced, last.
    pub(super) fn warnings(&self) -> Vec<Warning> {
        let mut out: Vec<Warning> = [
            &self.image_out.warnings,
            &self.trace_out.warnings,
            &self.regions_out.warnings,
            &self.colours_out.warnings,
            &self.lines_out.warnings,
            &self.shapes_out.warnings,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        if self.traced().paths.is_empty() {
            out.push(Warning::new("nothingTraced", json!({}), "nothing in the image was traced: it is empty or transparent"));
        }
        out
    }

    /// The time of each stage as the reply gives it (milliseconds), none on the web: the working
    /// copy, the trace, the areas (the region map, the colours, what lies inside what and the
    /// order) and the shapes (the centre lines, which they are made into, and the overlap).
    pub(super) fn timings(&self) -> [Option<f64>; 4] {
        let areas = added(added(self.regions_out.ms, self.colours_out.ms), self.lines_out.order_ms);
        let shapes = added(self.shapes_out.ms, self.lines_out.centre_ms);
        [self.image_out.ms, self.trace_out.ms, areas, shapes]
    }
}
