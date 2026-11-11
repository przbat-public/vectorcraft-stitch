//! Digitizing's stages kept between runs, for a window that runs them again at every change
//! ([`Pipeline`]): each stage's output is kept with the key it was made from (the numbers of the
//! outputs it read and exactly the settings it takes), so a run makes only the stages whose key
//! changed and finds the others kept. After the shapes come two stages of the window's own: the
//! objects Create would make, on a copy of the document, and their stitches, planned by a planning
//! service of the pipeline's own ([`super::super::DetachedService`]), which keeps the units of the
//! areas a change left as they were.
//!
//! A run calls back as each stage is done, with the outputs so far and whether it made the stage
//! or found it kept, so that a window shows each stage's result as soon as it has it. A cancel
//! (`vectorcraft_doc::cancel`) stops a run between stages and within each; the stages it finished
//! stay kept. Timing is the stages' own (none on the web, which has no clock).

use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::ThreadPalette;
use vectorcraft_doc::{Document, ImageObject, NodeId};
use vectorcraft_stitch::thread::Palette;

use super::super::{DetachedService, Options, Planned, Planner};
use super::command::{Listed, NOTHING_TO_MAKE, ReplyOf, reply_json};
use super::create::{self, Made};
use super::rules::AreaKind;
use super::settings::{BackgroundSetting, ColourChoice, Mode, Settings, Size};
use super::stages::{self, ColoursOut, Digitized, ImageOut, LinesOut, RegionsOut, ShapesOut, TraceOut, clock, since};
use super::trace;
use super::warnings::Warning;
use crate::{EngineError, Result};

/// How many outputs each stage keeps: the newest and the one before on the desktop, so that a value
/// changed and changed back is found kept; one on the web, whose memory is tighter. An estimate.
#[cfg(not(target_arch = "wasm32"))]
pub(super) const KEPT_PER_STAGE: usize = 2;
#[cfg(target_arch = "wasm32")]
pub(super) const KEPT_PER_STAGE: usize = 1;

/// A stage of digitizing, as a window shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageId {
    /// The working copy and its background.
    Image,
    /// Upstream's Image Trace.
    Trace,
    /// The region map.
    Regions,
    /// The colours, their threads and the areas.
    Colours,
    /// The centre lines and the sewing order.
    Lines,
    /// The shapes, grown under each other.
    Shapes,
    /// The objects Create would make.
    Objects,
    /// Their stitches.
    Plan,
}

impl StageId {
    /// Every stage, in the order a run goes through them.
    pub const ALL: [Self; 8] = [Self::Image, Self::Trace, Self::Regions, Self::Colours, Self::Lines, Self::Shapes, Self::Objects, Self::Plan];

    /// Its id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Trace => "trace",
            Self::Regions => "regions",
            Self::Colours => "colours",
            Self::Lines => "lines",
            Self::Shapes => "shapes",
            Self::Objects => "objects",
            Self::Plan => "plan",
        }
    }

    /// The window's step it belongs to, from 1: the image; the colours (the trace, the region map
    /// and the colours); the shapes (the centre lines, the order and the shapes); the stitches (the
    /// objects and their plan).
    pub fn step(self) -> u8 {
        match self {
            Self::Image => 1,
            Self::Trace | Self::Regions | Self::Colours => 2,
            Self::Lines | Self::Shapes => 3,
            Self::Objects | Self::Plan => 4,
        }
    }
}

/// An output with the number it was given when it was made: the stages after it are keyed by it.
pub(super) struct Numbered<T> {
    pub(super) number: u64,
    pub(super) value: Arc<T>,
}

impl<T> Clone for Numbered<T> {
    fn clone(&self) -> Self {
        Self { number: self.number, value: self.value.clone() }
    }
}

/// A stage's outputs kept, each with its key, the most recently used last, at most
/// [`KEPT_PER_STAGE`].
struct Kept<K, T> {
    entries: Vec<(K, Numbered<T>)>,
}

impl<K, T> Default for Kept<K, T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<K: PartialEq, T> Kept<K, T> {
    /// The output kept for `key`, made the most recently used.
    fn find(&mut self, key: &K) -> Option<Numbered<T>> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        let entry = self.entries.remove(at);
        let found = entry.1.clone();
        self.entries.push(entry);
        Some(found)
    }

    /// `out` kept for `key`, the least recently used let go past [`KEPT_PER_STAGE`].
    fn keep(&mut self, key: K, out: Numbered<T>) {
        self.entries.push((key, out));
        while self.entries.len() > KEPT_PER_STAGE {
            self.entries.remove(0);
        }
    }
}

/// What the image stage reads: the image's file (its bytes, the same while the document holds
/// them), the image object, and the size, crop and background settings.
struct ImageKey {
    bytes: Arc<Vec<u8>>,
    image: ImageObject,
    size: Size,
    crop: Option<[u32; 4]>,
    background: BackgroundSetting,
}

impl PartialEq for ImageKey {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.bytes, &o.bytes) && self.image == o.image && self.size == o.size && self.crop == o.crop && self.background == o.background
    }
}

/// What the trace reads: the working copy, and the mode, colours, smoothing, corners and the noise
/// the smallest area makes (a thread's footprint at most, so a smallest area above it traces the
/// same).
#[derive(PartialEq)]
struct TraceKey {
    image: u64,
    mode: Mode,
    colors: u32,
    smoothing: f64,
    corners: f64,
    noise: u32,
}

/// What the region map reads: the working copy and its background, the trace, and the background
/// setting and the mode (for its warnings).
#[derive(PartialEq)]
struct RegionsKey {
    image: u64,
    trace: u64,
    background: BackgroundSetting,
    mode: Mode,
}

/// What the colours read: the working copy, the trace, the region map, the colour choices, the
/// smallest area, the mode and the palette.
#[derive(PartialEq)]
struct ColoursKey {
    image: u64,
    trace: u64,
    regions: u64,
    choices: Vec<ColourChoice>,
    min_area: f64,
    mode: Mode,
    palette: Palette,
}

/// What the centre lines and the order read: the working copy and the colours.
#[derive(PartialEq)]
struct LinesKey {
    image: u64,
    colours: u64,
}

/// What the shapes read: the working copy, the trace, the colours, the lines and the overlap.
#[derive(PartialEq)]
struct ShapesKey {
    image: u64,
    trace: u64,
    colours: u64,
    lines: u64,
    overlap: f64,
}

/// What the objects read: every stage before them, the document they are made on (the same
/// snapshot, held) with the image's place in it, and the palette.
struct ObjectsKey {
    stages: [u64; 6],
    document: Arc<Document>,
    target: NodeId,
    palette: Palette,
}

impl PartialEq for ObjectsKey {
    fn eq(&self, o: &Self) -> bool {
        self.stages == o.stages && Arc::ptr_eq(&self.document, &o.document) && self.target == o.target && self.palette == o.palette
    }
}

/// The objects stage's output: the document with the objects Create would make, and what it made
/// (`None`: nothing to make, every area the background).
pub(super) struct ObjectsOut {
    /// The document they were made on (the request's snapshot).
    pub(super) of: Arc<Document>,
    pub(super) made: Option<(Arc<Document>, Made)>,
    pub(super) ms: Option<f64>,
}

/// The plan stage's output: the objects' plan and its figures (`embroidery.stats` on them alone),
/// or nothing when nothing is made.
pub(super) struct PlanOut {
    pub(super) planned: Option<(Planner, Planned)>,
    pub(super) stats: Value,
    pub(super) ms: Option<f64>,
}

/// What a window digitizes: the document as it was asked (its number and snapshot), the image
/// (`target`, an image or an Image Trace object) and its file, the settings, and the palette the
/// window matches threads to (the document's unless the window chose another, which Create then
/// makes the document's).
pub struct WindowRequest {
    pub(super) uid: u64,
    pub(super) original: Arc<Document>,
    pub(super) target: NodeId,
    pub(super) image: ImageObject,
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) settings: Settings,
    pub(super) palette: Palette,
}

impl WindowRequest {
    /// The open document it is of ([`crate::DocState::uid`]).
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// The image (or Image Trace object) it digitizes.
    pub fn target(&self) -> NodeId {
        self.target
    }

    /// The palette its threads are matched to.
    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// The settings as the command's reply echoes them, defaults filled in.
    pub fn settings(&self) -> Value {
        self.settings.echo()
    }
}

/// Where an area lies in the design's box, in thirds each way: for screen readers, which say an
/// area's name with it ("top left").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    TopLeft,
    Top,
    TopRight,
    Left,
    Middle,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Place {
    /// The place of a point at `(fx, fy)`, each its share of the box from its left or top (0 to 1).
    fn of(fx: f64, fy: f64) -> Self {
        let third = |f: f64| {
            if f < 1.0 / 3.0 {
                0
            } else if f < 2.0 / 3.0 {
                1
            } else {
                2
            }
        };
        match (third(fx), third(fy)) {
            (0, 0) => Self::TopLeft,
            (1, 0) => Self::Top,
            (_, 0) => Self::TopRight,
            (0, 1) => Self::Left,
            (1, 1) => Self::Middle,
            (_, 1) => Self::Right,
            (0, _) => Self::BottomLeft,
            (1, _) => Self::Bottom,
            _ => Self::BottomRight,
        }
    }
}

/// An area as the window names and describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaFacts {
    /// Its number in the sewing order, from 1 (`None`: not sewn).
    pub number: Option<usize>,
    /// The image's colour it is of.
    pub color: [u8; 3],
    /// The thread it is sewn in: its name in the palette, its number among the areas of that thread
    /// in sewing order (from 1), and its colour (`None` for the background).
    pub thread_name: Option<String>,
    pub own_number: Option<usize>,
    pub thread_color: Option<[u8; 3]>,
    /// Square millimetres.
    pub area_mm2: f64,
    /// A line sewn along its middle, or a fill.
    pub line: bool,
    /// Whether it is sewn (the background and a colour set not to be sewn aren't).
    pub sewn: bool,
    /// Whether it is the background.
    pub background: bool,
    /// Where it lies in the design's box.
    pub place: Place,
}

/// What a run has made so far, stage by stage: shared outputs, cheap to clone and to hand to
/// another thread.
#[derive(Clone, Default)]
pub struct Outputs {
    pub(super) image: Option<Numbered<ImageOut>>,
    pub(super) trace: Option<Numbered<TraceOut>>,
    pub(super) regions: Option<Numbered<RegionsOut>>,
    pub(super) colours: Option<Numbered<ColoursOut>>,
    pub(super) lines: Option<Numbered<LinesOut>>,
    pub(super) shapes: Option<Numbered<ShapesOut>>,
    pub(super) objects: Option<Numbered<ObjectsOut>>,
    pub(super) plan: Option<Numbered<PlanOut>>,
    /// The palette the run matched threads to.
    palette: Option<Palette>,
}

/// The outputs are handed to the window's thread and back.
const _: () = {
    const fn sent<T: Send + Sync>() {}
    sent::<Outputs>();
    sent::<WindowRequest>();
};

impl Outputs {
    /// The number of `stage`'s output, once it is done.
    pub fn number(&self, stage: StageId) -> Option<u64> {
        match stage {
            StageId::Image => self.image.as_ref().map(|n| n.number),
            StageId::Trace => self.trace.as_ref().map(|n| n.number),
            StageId::Regions => self.regions.as_ref().map(|n| n.number),
            StageId::Colours => self.colours.as_ref().map(|n| n.number),
            StageId::Lines => self.lines.as_ref().map(|n| n.number),
            StageId::Shapes => self.shapes.as_ref().map(|n| n.number),
            StageId::Objects => self.objects.as_ref().map(|n| n.number),
            StageId::Plan => self.plan.as_ref().map(|n| n.number),
        }
    }

    /// How long `stage` took when it was made (milliseconds; a kept output keeps the time of the run
    /// that made it), none before it is done or on the web.
    pub fn ms(&self, stage: StageId) -> Option<f64> {
        match stage {
            StageId::Image => self.image.as_ref()?.value.ms,
            StageId::Trace => self.trace.as_ref()?.value.ms,
            StageId::Regions => self.regions.as_ref()?.value.ms,
            StageId::Colours => self.colours.as_ref()?.value.ms,
            StageId::Lines => {
                let l = &self.lines.as_ref()?.value;
                stages::added(l.centre_ms, l.order_ms)
            }
            StageId::Shapes => self.shapes.as_ref()?.value.ms,
            StageId::Objects => self.objects.as_ref()?.value.ms,
            StageId::Plan => self.plan.as_ref()?.value.ms,
        }
    }

    /// The six digitizing stages, once they are all done.
    fn digitized(&self) -> Option<Digitized> {
        Some(Digitized {
            image_out: self.image.as_ref()?.value.clone(),
            trace_out: self.trace.as_ref()?.value.clone(),
            regions_out: self.regions.as_ref()?.value.clone(),
            colours_out: self.colours.as_ref()?.value.clone(),
            lines_out: self.lines.as_ref()?.value.clone(),
            shapes_out: self.shapes.as_ref()?.value.clone(),
        })
    }

    /// The reply the command's dry run gives for `req` (its areas listed none), once the shapes are
    /// done: its figures (`stats`) null until the stitches are planned, and the objects' and the
    /// stitches' timings with them.
    pub fn reply(&self, req: &WindowRequest) -> Option<Value> {
        let d = self.digitized()?;
        let mut warnings = d.warnings();
        let objects = self.objects.as_ref().map(|o| o.value.clone());
        if objects.as_ref().is_some_and(|o| o.made.is_none()) {
            warnings.push(Warning::new("nothingToMake", json!({}), NOTHING_TO_MAKE));
        }
        let plan = self.plan.as_ref().map(|p| p.value.clone());
        let of = ReplyOf {
            id: req.target,
            settings: &req.settings,
            palette: req.palette,
            listed: Listed { offset: 0, limit: 0, regions: false, outlines: false, at: None },
            dry_run: true,
            stats: plan.as_ref().map_or(Value::Null, |p| p.stats.clone()),
            warnings,
            made_ms: [objects.and_then(|o| o.ms), plan.and_then(|p| p.ms)],
        };
        Some(reply_json(&d, of))
    }

    /// The working copy, its width and height in pixels and its pixels (RGBA, row by row), once the
    /// image stage is done.
    pub fn working_rgba(&self) -> Option<(u32, u32, &[u8])> {
        let r = &self.image.as_ref()?.value.working.raster;
        Some((r.width, r.height, &r.rgba))
    }

    /// The working copy's pixels to the document's points, once the image stage is done.
    pub fn working_to_doc(&self) -> Option<vectorcraft_geom::Affine> {
        Some(self.image.as_ref()?.value.working.to_doc)
    }

    /// The design's size, width and height in millimetres, once the image stage is done.
    pub fn size_mm(&self) -> Option<(f64, f64)> {
        Some(self.image.as_ref()?.value.working.size_mm)
    }

    /// The working copy's resolution (pixels a millimetre), once the image stage is done.
    pub fn px_per_mm(&self) -> Option<f64> {
        Some(self.image.as_ref()?.value.working.px_per_mm)
    }

    /// The working copy painted as the Colours view shows it, once the colours are done: each pixel
    /// its area's thread's own colour, the background and the colours not sewn clear; its width,
    /// height and pixels (RGBA, row by row).
    pub fn painted_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let c = &self.colours.as_ref()?.value;
        let palette = self.palette?;
        let colour: Vec<[u8; 4]> = c
            .areas
            .areas
            .iter()
            .map(|a| {
                let row = c.colours.rows.get(a.row).filter(|r| r.sewn() && !r.background);
                row.and_then(|r| r.thread.as_ref()).and_then(|m| palette.get(m.thread.index)).map_or([0; 4], |([r, g, b], _)| [r, g, b, u8::MAX])
            })
            .collect();
        let map = &c.areas.regions;
        let mut rgba = Vec::with_capacity(map.labels.len().saturating_mul(4));
        for &l in &map.labels {
            rgba.extend_from_slice(colour.get(l as usize).unwrap_or(&[0; 4]));
        }
        Some((map.w, map.h, rgba))
    }

    /// The area under working pixel `(x, y)`, named and described, once the colours are done (its
    /// numbers and its place once the order is too).
    pub fn area_at(&self, x: u32, y: u32) -> Option<AreaFacts> {
        let c = &self.colours.as_ref()?.value;
        let palette = self.palette?;
        let px_per_mm = self.px_per_mm()?;
        let lines = self.lines.as_ref().map(|l| &l.value);
        let areas = lines.map_or(&c.areas, |l| &l.areas);
        let a = areas.regions.at(x, y)? as usize;
        let area = areas.areas.get(a)?;
        let row = c.colours.rows.get(area.row)?;
        let thread =
            row.thread.as_ref().filter(|_| !row.background).and_then(|m| palette.get(m.thread.index).map(|(rgb, name)| (m.thread.index, rgb, name)));
        let sewn = row.sewn() && !row.background;
        // Numbered in sewing order, and among the areas of its thread.
        let thread_of = |b: usize| areas.areas.get(b).and_then(|x| c.colours.rows.get(x.row)).and_then(|r| r.thread.as_ref()).map(|m| m.thread.index);
        let (mut number, mut own_number, mut box_of) = (None, None, None::<[u32; 4]>);
        if let Some(l) = lines {
            let mut own = 0;
            for (i, &b) in l.order.sewn.iter().enumerate() {
                if thread_of(b) == thread.map(|t| t.0) {
                    own += 1;
                }
                if b == a {
                    number = Some(i + 1);
                    own_number = Some(own);
                }
                let Some(s) = areas.regions.areas.get(b) else { continue };
                box_of = Some(box_of.map_or(s.bbox, |[x0, y0, x1, y1]| [x0.min(s.bbox[0]), y0.min(s.bbox[1]), x1.max(s.bbox[2]), y1.max(s.bbox[3])]));
            }
        }
        let stats = areas.regions.areas.get(a)?;
        let [x0, y0, x1, y1] = box_of.unwrap_or([0, 0, areas.regions.w, areas.regions.h]).map(f64::from);
        let (cx, cy) = (f64::from(stats.bbox[0] + stats.bbox[2]) / 2.0, f64::from(stats.bbox[1] + stats.bbox[3]) / 2.0);
        let share = |v: f64, lo: f64, hi: f64| if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
        Some(AreaFacts {
            number: number.filter(|_| sewn),
            color: row.color,
            thread_name: thread.map(|t| t.2.to_string()),
            own_number: own_number.filter(|_| sewn),
            thread_color: thread.map(|t| t.1),
            area_mm2: area.pixels as f64 / (px_per_mm * px_per_mm),
            line: area.kind == AreaKind::Line,
            sewn,
            background: row.background,
            place: Place::of(share(cx, x0, x1), share(cy, y0, y1)),
        })
    }

    /// The document with the objects Create would make, and their group, once the objects are done
    /// (`None`: nothing to make).
    pub fn made(&self) -> Option<(&Arc<Document>, NodeId)> {
        self.objects.as_ref()?.value.made.as_ref().map(|(d, m)| (d, m.group))
    }

    /// The objects' plan, with its stitch scene, once the stitches are planned.
    pub fn planned(&self) -> Option<&Planned> {
        self.plan.as_ref()?.value.planned.as_ref().map(|(_, p)| p)
    }

    /// A read command's reply on the objects alone (`embroidery.check`, `threads`…), made by the
    /// command's own code from their plan, once the stitches are planned.
    pub fn query(&self, cmd: &str, params: &Value) -> Option<Result<Value>> {
        self.plan.as_ref()?.value.planned.as_ref().map(|(planner, _)| planner.query(cmd, params))
    }
}

/// Digitizing's stages kept between runs ([`Pipeline::run`]), with a planning service of its own.
pub struct Pipeline {
    image: Kept<ImageKey, ImageOut>,
    trace: Kept<TraceKey, TraceOut>,
    regions: Kept<RegionsKey, RegionsOut>,
    colours: Kept<ColoursKey, ColoursOut>,
    lines: Kept<LinesKey, LinesOut>,
    shapes: Kept<ShapesKey, ShapesOut>,
    objects: Kept<ObjectsKey, ObjectsOut>,
    plan: Kept<u64, PlanOut>,
    planning: DetachedService,
    /// The number the next output made takes.
    next: u64,
}

/// The pipeline goes to the window's thread.
const _: () = {
    const fn sent<T: Send>() {}
    sent::<Pipeline>();
};

/// The output `kept` holds for `key`, or one made now by `make` and kept: with whether it was made
/// now. A cancel is looked for before a stage is made.
fn stage<K: PartialEq, T>(kept: &mut Kept<K, T>, next: &mut u64, key: K, make: impl FnOnce() -> Result<T>) -> Result<(Numbered<T>, bool)> {
    if let Some(found) = kept.find(&key) {
        return Ok((found, false));
    }
    vectorcraft_doc::cancel::check()?;
    let made = Numbered { number: *next, value: Arc::new(make()?) };
    *next += 1;
    kept.keep(key, made.clone());
    Ok((made, true))
}

impl Pipeline {
    /// A pipeline planning with `planning`.
    pub(super) fn new(planning: DetachedService) -> Self {
        Self {
            image: Kept::default(),
            trace: Kept::default(),
            regions: Kept::default(),
            colours: Kept::default(),
            lines: Kept::default(),
            shapes: Kept::default(),
            objects: Kept::default(),
            plan: Kept::default(),
            planning,
            next: 1,
        }
    }

    /// Digitize as `req` asks, making only the stages whose key changed since they were kept, and
    /// calling `done` as each is done, with the outputs so far and whether it was made now. `steps`
    /// is asked as the trace goes, from the threads that trace, and stops the run when it answers
    /// `false`. Refused as the command refuses; a design too large to plan fails at its stitches,
    /// after the stages before them were handed over.
    pub fn run(
        &mut self,
        req: &WindowRequest,
        steps: &(dyn Fn(u64, u64) -> bool + Sync),
        done: &mut dyn FnMut(StageId, &Outputs, bool),
    ) -> Result<Outputs> {
        let s = &req.settings;
        let mut out = Outputs { palette: Some(req.palette), ..Outputs::default() };
        let key = ImageKey { bytes: req.bytes.clone(), image: req.image.clone(), size: s.size, crop: s.crop, background: s.background };
        let (image, made) = stage(&mut self.image, &mut self.next, key, || stages::image_stage(&req.image, &req.bytes, s))?;
        out.image = Some(image.clone());
        done(StageId::Image, &out, made);
        let noise = trace::area_px(s.min_area.min(trace::TRACE_AREA_MM2), image.value.working.px_per_mm);
        let key = TraceKey { image: image.number, mode: s.mode, colors: s.colors, smoothing: s.smoothing, corners: s.corners, noise };
        let (traced, made) = stage(&mut self.trace, &mut self.next, key, || stages::trace_stage(&image.value, s, steps))?;
        out.trace = Some(traced.clone());
        done(StageId::Trace, &out, made);
        let key = RegionsKey { image: image.number, trace: traced.number, background: s.background, mode: s.mode };
        let (regions, made) = stage(&mut self.regions, &mut self.next, key, || stages::regions_stage(&image.value, &traced.value, s))?;
        out.regions = Some(regions.clone());
        done(StageId::Regions, &out, made);
        let key = ColoursKey {
            image: image.number,
            trace: traced.number,
            regions: regions.number,
            choices: s.choices.clone(),
            min_area: s.min_area,
            mode: s.mode,
            palette: req.palette,
        };
        let (colours, made) =
            stage(&mut self.colours, &mut self.next, key, || stages::colours_stage(&image.value, &traced.value, &regions.value, s, req.palette))?;
        out.colours = Some(colours.clone());
        done(StageId::Colours, &out, made);
        let key = LinesKey { image: image.number, colours: colours.number };
        let (lines, made) = stage(&mut self.lines, &mut self.next, key, || stages::lines_stage(&image.value, &colours.value))?;
        out.lines = Some(lines.clone());
        done(StageId::Lines, &out, made);
        let key = ShapesKey { image: image.number, trace: traced.number, colours: colours.number, lines: lines.number, overlap: s.overlap };
        let (shapes, made) =
            stage(&mut self.shapes, &mut self.next, key, || stages::shapes_stage(&image.value, &traced.value, &colours.value, &lines.value, s))?;
        out.shapes = Some(shapes.clone());
        done(StageId::Shapes, &out, made);
        let numbers = [image.number, traced.number, regions.number, colours.number, lines.number, shapes.number];
        let key = ObjectsKey { stages: numbers, document: req.original.clone(), target: req.target, palette: req.palette };
        let d = out.digitized().ok_or_else(|| EngineError::Internal { cmd: super::settings::C.into(), msg: "a stage is missing".into() })?;
        let (objects, made) = stage(&mut self.objects, &mut self.next, key, || objects_of(req, &d))?;
        out.objects = Some(objects.clone());
        done(StageId::Objects, &out, made);
        let planning = &self.planning;
        let (plan, made) = stage(&mut self.plan, &mut self.next, objects.number, || plan_of(planning, &objects.value))?;
        out.plan = Some(plan);
        done(StageId::Plan, &out, made);
        Ok(out)
    }
}

/// `document` with `palette` as its thread palette, unless it is the one it matches to already.
pub(super) fn with_palette(document: &mut Document, palette: Palette) {
    if super::super::thread_palette(document) == palette {
        return;
    }
    let mut setup = document.embroidery.as_deref().cloned().unwrap_or_default();
    setup.set_palette(ThreadPalette::from_id(palette.id()));
    document.embroidery = Some(Box::new(setup));
}

/// The objects Create would make of `d` on a copy of `req`'s document, its palette the window's.
fn objects_of(req: &WindowRequest, d: &Digitized) -> Result<ObjectsOut> {
    let started = clock();
    let mut doc = Document::clone(&req.original);
    with_palette(&mut doc, req.palette);
    let made = create::made(&mut doc, req.target, d, req.palette)?.map(|m| (Arc::new(doc), m));
    Ok(ObjectsOut { of: req.original.clone(), made, ms: since(started) })
}

/// The stitches of what `objects` made, planned by `planning` on the objects alone, and their
/// figures as `embroidery.stats` gives them.
fn plan_of(planning: &DetachedService, objects: &ObjectsOut) -> Result<PlanOut> {
    let started = clock();
    let Some((doc, made)) = &objects.made else { return Ok(PlanOut { planned: None, stats: Value::Null, ms: since(started) }) };
    let planner = planning.planner(doc.clone(), vec![made.group])?;
    let planned = planner.planned(&Options::default())?;
    let stats = planner.query("embroidery.stats", &json!({}))?;
    Ok(PlanOut { planned: Some((planner, planned)), stats, ms: since(started) })
}

impl Outputs {
    /// What the run's objects were made on, the document with them, and what was made (`None`:
    /// nothing to make, or the objects aren't made yet).
    pub(super) fn made_for_create(&self) -> Option<(&Arc<Document>, Arc<Document>, Made)> {
        let o = &self.objects.as_ref()?.value;
        o.made.as_ref().map(|(d, m)| (&o.of, d.clone(), *m))
    }
}
