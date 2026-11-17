//! Digitizing's stages kept between runs, for a window that runs them again at every change
//! ([`Pipeline`]): each stage's output is kept with the key it was made from (the numbers of the
//! outputs it read and exactly the settings it takes), so a run makes only the stages whose key
//! changed and finds the others kept. A request says by the same keys which stage a run of another
//! would make first ([`WindowRequest::first_stage_changed`]), so that a window knows what a change
//! makes again before it runs it. After the shapes come two stages of the window's own: the
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
use vectorcraft_doc::embroidery::{EmbroiderySetup, ThreadPalette};
use vectorcraft_doc::{Document, ImageObject, NodeId};
use vectorcraft_render::lines::{LinePicture, LinePictureBuilder};
use vectorcraft_stitch::thread::Palette;

use super::super::{DetachedService, Options, Planned, Planner};
use super::changes::Found;
use super::command::{Listed, NOTHING_TO_MAKE, ReplyOf, reply_json};
use super::create::{self, Made};
use super::rules::{Area, AreaKind};
use super::settings::{AreaChange, AreaStitch, BackgroundSetting, ColourChoice, Mode, OutlineSettings, Settings, Size};
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

/// How closely the Shapes view's lines follow the shapes' curves (document points): a twentieth of
/// a millimetre, under a pixel at any zoom the window offers (400 %). An estimate.
const OUTLINE_FLATTEN_PT: f64 = 0.05 / vectorcraft_stitch::MM_PER_PT;

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

/// What the trace reads of a request: the mode, the colours, the smoothing, the corners, whether
/// nearly straight curves become lines, and the noise the smallest area makes, kept as that area
/// itself up to a thread's footprint (a smallest area above it traces the same).
#[derive(PartialEq)]
struct TraceSettings {
    mode: Mode,
    colors: u32,
    smoothing: f64,
    corners: f64,
    straight_lines: bool,
    noise_mm2: f64,
}

/// What the trace reads: the working copy, and the request's [`TraceSettings`].
#[derive(PartialEq)]
struct TraceKey {
    image: u64,
    settings: TraceSettings,
}

/// What the region map reads of a request: the background setting and the mode (for its
/// warnings).
#[derive(PartialEq)]
struct RegionsSettings {
    background: BackgroundSetting,
    mode: Mode,
}

/// What the region map reads: the working copy and its background, the trace, and the request's
/// [`RegionsSettings`].
#[derive(PartialEq)]
struct RegionsKey {
    image: u64,
    trace: u64,
    settings: RegionsSettings,
}

/// What the colours read of a request: the colour choices, the smallest area, the mode, the palette,
/// and how thin an area is a line.
#[derive(PartialEq)]
struct ColoursSettings {
    choices: Vec<ColourChoice>,
    min_area: f64,
    mode: Mode,
    palette: Palette,
    thin_line: f64,
}

/// What the colours read: the working copy, the trace, the region map, and the request's
/// [`ColoursSettings`].
#[derive(PartialEq)]
struct ColoursKey {
    image: u64,
    trace: u64,
    regions: u64,
    settings: ColoursSettings,
}

/// What the centre lines read of a request: how thin an area is a line, held to its width away
/// from its junctions.
#[derive(PartialEq)]
struct LinesSettings {
    thin_line: f64,
}

/// A change to a single area setting its stitch, as the shapes read it: what finds its area again,
/// and whether it leaves that area out.
#[derive(PartialEq)]
struct StitchSet {
    at: vectorcraft_geom::Point,
    color: [u8; 3],
    line: bool,
    leaves_out: bool,
}

/// What the centre lines and the order read: the working copy, the colours and the request's
/// [`LinesSettings`].
#[derive(PartialEq)]
struct LinesKey {
    image: u64,
    colours: u64,
    settings: LinesSettings,
}

/// What the shapes read of a request: the overlap; and the changes to single areas that may leave
/// one unsewn (Don't Sew), so that what lies inside it keeps its hole: those setting a stitch from
/// the first Don't Sew on (one before it leaves nothing out, and a later change to its area
/// overrules it).
#[derive(PartialEq)]
struct ShapesSettings {
    overlap: f64,
    stitches: Vec<StitchSet>,
}

/// What the shapes read: the working copy, the trace, the colours, the lines and the request's
/// [`ShapesSettings`].
#[derive(PartialEq)]
struct ShapesKey {
    image: u64,
    trace: u64,
    colours: u64,
    lines: u64,
    settings: ShapesSettings,
}

/// What the objects read of a request: the document they are made on (the same snapshot, held)
/// with the image's place in it, the palette, the document's embroidery settings with the
/// window's changes, which their stitches are planned with, the outlines, and the changes to
/// single areas.
struct ObjectsSettings {
    document: Arc<Document>,
    target: NodeId,
    palette: Palette,
    setup: Option<Box<EmbroiderySetup>>,
    outlines: OutlineSettings,
    changes: Vec<AreaChange>,
}

impl PartialEq for ObjectsSettings {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.document, &o.document)
            && self.target == o.target
            && self.palette == o.palette
            && self.setup == o.setup
            && self.outlines == o.outlines
            && self.changes == o.changes
    }
}

/// What the objects read: every stage before them, and the request's [`ObjectsSettings`].
#[derive(PartialEq)]
struct ObjectsKey {
    stages: [u64; 6],
    settings: ObjectsSettings,
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
/// (`target`, an image or an Image Trace object) and its file, the settings, the palette the window
/// matches threads to (the document's unless the window chose another, which Create then makes the
/// document's), and the document's embroidery settings with the window's changes, which Create
/// makes the document's too.
pub struct WindowRequest {
    pub(super) uid: u64,
    pub(super) original: Arc<Document>,
    pub(super) target: NodeId,
    pub(super) image: ImageObject,
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) settings: Settings,
    pub(super) palette: Palette,
    pub(super) setup: Option<Box<EmbroiderySetup>>,
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

    /// Its changes to single areas as the reply echoes them, in their order: that of
    /// [`Outputs::change_states`]' states.
    pub fn changes(&self) -> Vec<Value> {
        self.settings.changes.iter().map(super::settings::change_json).collect()
    }

    /// The first stage a run of `new` makes after a run of this request: the first stage that
    /// reads something different of the two, by the keys the pipeline keeps its stages by (the
    /// stages after it read its output, so they are made again too); `None` when every stage reads
    /// the same of both, nothing made again.
    pub fn first_stage_changed(&self, new: &Self) -> Option<StageId> {
        StageId::ALL.into_iter().find(|&stage| !self.reads_alike(new, stage))
    }

    /// Whether `stage` reads the same of this request as of `o` (the outputs before it aside).
    fn reads_alike(&self, o: &Self, stage: StageId) -> bool {
        match stage {
            StageId::Image => self.image_key() == o.image_key(),
            StageId::Trace => self.trace_settings() == o.trace_settings(),
            StageId::Regions => self.regions_settings() == o.regions_settings(),
            StageId::Colours => self.colours_settings() == o.colours_settings(),
            StageId::Lines => self.lines_settings() == o.lines_settings(),
            // The plan reads only the objects.
            StageId::Plan => true,
            StageId::Shapes => self.shapes_settings() == o.shapes_settings(),
            StageId::Objects => self.objects_settings() == o.objects_settings(),
        }
    }

    /// What the image stage reads of it: the whole of the image stage's key.
    fn image_key(&self) -> ImageKey {
        let s = &self.settings;
        ImageKey { bytes: self.bytes.clone(), image: self.image.clone(), size: s.size, crop: s.crop, background: s.background }
    }

    fn trace_settings(&self) -> TraceSettings {
        let s = &self.settings;
        TraceSettings {
            mode: s.mode,
            colors: s.colors,
            smoothing: s.smoothing,
            corners: s.corners,
            straight_lines: s.straight_lines,
            noise_mm2: s.min_area.min(trace::TRACE_AREA_MM2),
        }
    }

    fn regions_settings(&self) -> RegionsSettings {
        RegionsSettings { background: self.settings.background, mode: self.settings.mode }
    }

    fn colours_settings(&self) -> ColoursSettings {
        let s = &self.settings;
        ColoursSettings { choices: s.choices.clone(), min_area: s.min_area, mode: s.mode, palette: self.palette, thin_line: s.thin_line }
    }

    fn lines_settings(&self) -> LinesSettings {
        LinesSettings { thin_line: self.settings.thin_line }
    }

    fn shapes_settings(&self) -> ShapesSettings {
        let set = |c: &AreaChange| c.stitch.map(|s| StitchSet { at: c.at, color: c.color, line: c.line, leaves_out: s == AreaStitch::None });
        let stitches = self.settings.changes.iter().filter_map(set).skip_while(|s| !s.leaves_out).collect();
        ShapesSettings { overlap: self.settings.overlap, stitches }
    }

    fn objects_settings(&self) -> ObjectsSettings {
        ObjectsSettings {
            document: self.original.clone(),
            target: self.target,
            palette: self.palette,
            setup: self.setup.clone(),
            outlines: self.settings.outlines,
            changes: self.settings.changes.clone(),
        }
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
    /// Its number in the sewing order among the areas sewn, from 1 (`None`: not sewn).
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
    /// Whether it is sewn (the background, a colour set not to be sewn and, once the lines are
    /// found, an area a change leaves out (Don't Sew) aren't).
    pub sewn: bool,
    /// Whether it is the background.
    pub background: bool,
    /// Where it lies in the design's box.
    pub place: Place,
    /// Its widest part (millimetres): twice the distance from its deepest point to its edge, measured
    /// on the shape it is made as once the shapes are done (the islands it is made solid under and
    /// its shares of the centre lines beside it in it), on its own pixels until then.
    pub widest_mm: f64,
    /// How wide the line sewn along its middle is (millimetres; `None`: a filled area, or the
    /// lines aren't found yet).
    pub line_width_mm: Option<f64>,
    /// Its box's width and height (millimetres).
    pub size_mm: (f64, f64),
}

/// The widest a filled area may be for Satin to be offered on it (millimetres): the most narrow
/// parts as satin takes (`plan::NARROW_RANGE`), past which satin can't sew it whole (the project
/// owner's choice, 2026-11-16, „Do 12 mm szerokości (polecam)”).
pub const SATIN_AREA_MOST_MM: f64 = vectorcraft_stitch::plan::NARROW_RANGE.1;

/// The narrowest a line may be for Satin to be offered on it (millimetres): the narrowest satin
/// column the planner sews (`plan::SATIN_MIN_WIDTH`).
pub const SATIN_LINE_LEAST_MM: f64 = vectorcraft_stitch::plan::SATIN_MIN_WIDTH;

impl AreaFacts {
    /// Whether Satin is offered on it: a filled area at most [`SATIN_AREA_MOST_MM`] wide at its
    /// widest, a line at least [`SATIN_LINE_LEAST_MM`] wide (a line not found yet: not).
    pub fn satin_fits(&self) -> bool {
        if self.line { self.line_width_mm.is_some_and(|w| w >= SATIN_LINE_LEAST_MM) } else { self.widest_mm <= SATIN_AREA_MOST_MM }
    }
}

/// What finds an area again after the settings change: a point inside it (its deepest, in the
/// image's own pixels), its colour (the image's, before threads) and its kind; the key a change to
/// it is kept by (`areaChanges`).
#[derive(Clone, Debug, PartialEq)]
pub struct AreaKey {
    pub at: [f64; 2],
    pub color: [u8; 3],
    /// A line sewn along its middle, not a filled area.
    pub line: bool,
}

/// Where an area is sewn: its number in the sewing order (from 1), how many areas there are, and
/// the areas sewn just before and just after it, each with its key.
#[derive(Clone, Debug, PartialEq)]
pub struct SewingPlace {
    pub number: usize,
    pub of: usize,
    pub before: Option<(AreaFacts, AreaKey)>,
    pub after: Option<(AreaFacts, AreaKey)>,
}

/// What became of a change to an area in a run: the area it applies to, or why it applies to none
/// (`otherColor`, `noArea`, `otherKind`, `notSewn`, as the reply's `areaChanges` says).
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeState {
    Applies(AreaFacts),
    Kept(&'static str),
}

/// Two areas are alike for Apply to Similar Areas when they are of the same colour and kind and
/// their sizes lie within this share of the larger: an estimate, checked before the inspector was
/// built on the person's real designs and the test logos (2026-11-16), to group what a person calls
/// alike (a fox's ears and eyes, a watermelon's seeds, a logo's letters).
const SIMILAR_SHARE: f64 = 0.25;

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
            made_ms: [objects.as_ref().and_then(|o| o.ms), plan.and_then(|p| p.ms)],
            outlined: objects.as_ref().and_then(|o| o.made.as_ref()).and_then(|(_, m)| m.outlined),
            edited: req.original.node(req.target).and_then(super::live::edited),
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

    /// The art's bounds in the image's own pixels, `[x0, y0, x1, y1]` (x1 and y1 past the last),
    /// once the colours are done: the box of every area that isn't the background, rounded out to
    /// whole pixels and kept inside the image; what Crop to the Art crops to. `None` when every
    /// area is the background.
    pub fn art_bounds(&self) -> Option<[u32; 4]> {
        let c = &self.colours.as_ref()?.value;
        let w = &self.image.as_ref()?.value.working;
        let mut b: Option<[u32; 4]> = None;
        for (a, area) in c.areas.areas.iter().enumerate() {
            if c.colours.rows.get(area.row).is_none_or(|r| r.background) {
                continue;
            }
            let Some(s) = c.areas.regions.areas.get(a) else { continue };
            b = Some(b.map_or(s.bbox, |[x0, y0, x1, y1]| [x0.min(s.bbox[0]), y0.min(s.bbox[1]), x1.max(s.bbox[2]), y1.max(s.bbox[3])]));
        }
        let [x0, y0, x1, y1] = b?.map(f64::from);
        let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| w.to_image * vectorcraft_geom::Point::new(x, y));
        let (mut l, mut t, mut r, mut bt) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in corners {
            (l, t, r, bt) = (l.min(p.x), t.min(p.y), r.max(p.x), bt.max(p.y));
        }
        let (iw, ih) = (f64::from(w.image_px.0), f64::from(w.image_px.1));
        let clamp = |v: f64, hi: f64| v.clamp(0.0, hi) as u32;
        let out = [clamp(l.floor(), iw), clamp(t.floor(), ih), clamp(r.ceil(), iw), clamp(bt.ceil(), ih)];
        (out[0] < out[2] && out[1] < out[3]).then_some(out)
    }

    /// The image's own size in pixels, once the image stage is done.
    pub fn image_px(&self) -> Option<(u32, u32)> {
        Some(self.image.as_ref()?.value.working.image_px)
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
        let areas = self.lines.as_ref().map_or(&c.areas, |l| &l.value.areas);
        self.facts(areas.regions.at(x, y)? as usize)
    }

    /// Area `a` (its place in the areas) named and described, once the colours are done (its
    /// numbers, its place and its line's width once the order is too).
    fn facts(&self, a: usize) -> Option<AreaFacts> {
        let c = &self.colours.as_ref()?.value;
        let palette = self.palette?;
        let px_per_mm = self.px_per_mm()?;
        let lines = self.lines.as_ref().map(|l| &l.value);
        // The areas as they are sewn once the shapes are done (those a change leaves out marked).
        let areas = self.shapes.as_ref().map(|s| &s.value.areas).or_else(|| lines.map(|l| &l.areas)).unwrap_or(&c.areas);
        let area = areas.areas.get(a)?;
        let row = c.colours.rows.get(area.row)?;
        let thread =
            row.thread.as_ref().filter(|_| !row.background).and_then(|m| palette.get(m.thread.index).map(|(rgb, name)| (m.thread.index, rgb, name)));
        // Sewn: by its colour, and once the lines are found, no change to it alone leaving it out.
        let sewn = !row.background && area.sewn(&c.colours);
        // Numbered in sewing order, and among the areas of its thread.
        let thread_of = |b: usize| areas.areas.get(b).and_then(|x| c.colours.rows.get(x.row)).and_then(|r| r.thread.as_ref()).map(|m| m.thread.index);
        let (mut number, mut own_number, mut box_of) = (None, None, None::<[u32; 4]>);
        if let Some(l) = lines {
            // Numbered among the areas sewn (those of a colour not sewn come first in the order, and
            // aren't; those a change leaves out aren't either).
            let of_sewn = |b: usize| areas.areas.get(b).is_some_and(|x| x.sewn(&c.colours));
            let mut own = 0;
            for (i, &b) in l.order.sewn.iter().filter(|&&b| of_sewn(b)).enumerate() {
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
        let [bx0, by0, bx1, by1] = stats.bbox.map(f64::from);
        let line_width_mm = lines.and_then(|l| l.lines.get(a)).and_then(Option::as_ref).map(|line| line.width_px / px_per_mm);
        Some(AreaFacts {
            number: number.filter(|_| sewn),
            color: row.color,
            thread_name: thread.map(|t| t.2.to_string()),
            own_number: own_number.filter(|_| sewn),
            thread_color: thread.map(|t| t.1),
            area_mm2: area.pixels as f64 / (px_per_mm * px_per_mm),
            // A line: sewn along its middle as Create makes it, once the centre lines are found.
            line: lines.map_or(area.kind == AreaKind::Line, |l| super::changes::made_line(&l.lines, a)),
            sewn,
            background: row.background,
            place: Place::of(share(cx, x0, x1), share(cy, y0, y1)),
            // Satin's measure: the shape it is made as, once the shapes are done.
            widest_mm: self.shapes.as_ref().and_then(|s| s.value.shapes.widest_px.get(a).copied()).unwrap_or(stats.widest_px) / px_per_mm,
            line_width_mm,
            size_mm: ((bx1 - bx0) / px_per_mm, (by1 - by0) / px_per_mm),
        })
    }

    /// The key of the area under working pixel `(x, y)` (a point inside it, its colour and its
    /// kind), once the image is digitized; `None` on the background and on an area of a colour not
    /// sewn, which a change never finds.
    pub fn area_key(&self, x: u32, y: u32) -> Option<AreaKey> {
        let d = self.digitized()?;
        key_of(&d, d.areas().regions.at(x, y)? as usize)
    }

    /// The working pixel of the area `key` finds now (its deepest), by the rule a change to it is
    /// found again by (`super::changes`), once the image is digitized; `None` when a change would
    /// be kept, applying to nothing.
    pub fn find(&self, key: &AreaKey) -> Option<(u32, u32)> {
        let d = self.digitized()?;
        let probe = AreaChange {
            at: vectorcraft_geom::Point::new(key.at[0], key.at[1]),
            color: key.color,
            line: key.line,
            stitch: Some(AreaStitch::None),
            angle: None,
        };
        match super::changes::found(std::slice::from_ref(&probe), &d).first()? {
            Found::Applies(a) => d.areas().regions.areas.get(*a).map(|s| s.deepest),
            Found::Kept(_) => None,
        }
    }

    /// The object the area under working pixel `(x, y)` was made into, in the document
    /// [`Outputs::made`] holds, once the objects are made.
    pub fn area_object(&self, x: u32, y: u32) -> Option<NodeId> {
        let a = self.lines.as_ref()?.value.areas.regions.at(x, y)? as usize;
        let (_, made) = self.objects.as_ref()?.value.made.as_ref()?;
        made.areas.iter().find(|(b, _)| *b == a).map(|(_, id)| *id)
    }

    /// The keys of the areas alike to the one under working pixel `(x, y)`, in sewing order: of its
    /// colour and kind, their sizes within [`SIMILAR_SHARE`] of the larger; none before the image
    /// is digitized.
    pub fn similar(&self, x: u32, y: u32) -> Vec<AreaKey> {
        let Some(d) = self.digitized() else { return vec![] };
        let areas = d.areas();
        let Some(a) = areas.regions.at(x, y).map(|l| l as usize) else { return vec![] };
        let Some(this) = areas.areas.get(a) else { return vec![] };
        let line = |b: usize| super::changes::made_line(d.lines(), b);
        let alike = |b: usize, x: &Area| {
            let (p, q) = (x.pixels as f64, this.pixels as f64);
            x.row == this.row && line(b) == line(a) && (p - q).abs() <= SIMILAR_SHARE * p.max(q)
        };
        d.order().sewn.iter().filter(|&&b| b != a && areas.areas.get(b).is_some_and(|x| alike(b, x))).filter_map(|&b| key_of(&d, b)).collect()
    }

    /// Where the area under working pixel `(x, y)` is sewn among the areas sewn, once the order is
    /// found (`None` for an area not sewn: of a colour not sewn, or one a change leaves out).
    pub fn sewing_place(&self, x: u32, y: u32) -> Option<SewingPlace> {
        let d = self.digitized()?;
        let a = d.areas().regions.at(x, y)? as usize;
        let sewn: Vec<usize> = d.order().sewn.iter().copied().filter(|&b| d.areas().areas.get(b).is_some_and(|x| x.sewn(d.colours()))).collect();
        let i = sewn.iter().position(|&b| b == a)?;
        let facts = |j: Option<usize>| j.and_then(|j| sewn.get(j)).and_then(|&b| Some((self.facts(b)?, key_of(&d, b)?)));
        Some(SewingPlace { number: i + 1, of: sewn.len(), before: facts(i.checked_sub(1)), after: facts(i.checked_add(1)) })
    }

    /// The key of the area sewn first, once the order is found: where the keyboard starts in the
    /// preview (an area a change leaves out is passed by, as one of a colour not sewn).
    pub fn first_key(&self) -> Option<AreaKey> {
        let d = self.digitized()?;
        d.order().sewn.iter().filter(|&&a| d.areas().areas.get(a).is_some_and(|x| x.sewn(d.colours()))).find_map(|&a| key_of(&d, a))
    }

    /// Which working pixels belong to an area sewn after the one under working pixel `(x, y)`: the
    /// map's width and height and a byte a pixel, 1 for those, 0 for the rest (the background, and
    /// the areas sewn before it or it); what Show in Sewing Order dims.
    pub fn sewn_after(&self, x: u32, y: u32) -> Option<(u32, u32, Vec<u8>)> {
        let d = self.digitized()?;
        let regions = &d.areas().regions;
        let a = regions.at(x, y)? as usize;
        let placed = &d.order().placed;
        // Sewn: an area a change leaves out is sewn after nothing, nor is anything sewn after it.
        let sewn = |l: usize| d.areas().areas.get(l).is_some_and(|x| x.sewn(d.colours()));
        let index = |l: usize| placed.get(l).and_then(|p| p.index).filter(|_| sewn(l));
        let own = index(a)?;
        let mask = regions.labels.iter().map(|&l| u8::from(index(l as usize).is_some_and(|i| i > own))).collect();
        Some((regions.w, regions.h, mask))
    }

    /// What became of each of `req`'s changes to single areas in this run, in their order, once the
    /// image is digitized.
    pub fn change_states(&self, req: &WindowRequest) -> Option<Vec<ChangeState>> {
        let d = self.digitized()?;
        let found = super::changes::found(&req.settings.changes, &d);
        Some(
            found
                .into_iter()
                .map(|f| match f {
                    Found::Applies(a) => self.facts(a).map_or(ChangeState::Kept(super::changes::KeptWhy::NoArea.id()), ChangeState::Applies),
                    Found::Kept(why) => ChangeState::Kept(why.id()),
                })
                .collect(),
        )
    }

    /// The Shapes view's lines, once the shapes are done: each area's shape as it is sewn (its
    /// outline and holes, the overlap in it; a line's centre line) in document points, in the slot
    /// of its colour, and each slot's colour (its thread's, else the image's).
    pub fn outlines(&self) -> Option<(LinePicture, Vec<[u8; 3]>)> {
        let d = self.digitized()?;
        let palette = self.palette?;
        let to_doc = d.working().to_doc;
        let colors: Vec<[u8; 3]> =
            d.colours().rows.iter().map(|r| r.thread.as_ref().and_then(|m| palette.get(m.thread.index)).map_or(r.color, |(rgb, _)| rgb)).collect();
        let origin = to_doc * vectorcraft_geom::Point::ZERO;
        let mut b = LinePictureBuilder::new(origin, 1.0);
        let mut number = 0u32;
        for &a in &d.order().sewn {
            let Some(area) = d.areas().areas.get(a) else { continue };
            let path = match d.lines().get(a).and_then(Option::as_ref) {
                Some(line) => to_doc * line.path(),
                None => match d.shapes().shapes.get(a) {
                    Some(shape) => shape.transformed(to_doc).to_bezpath(),
                    None => continue,
                },
            };
            let slot = u32::try_from(area.row).unwrap_or(u32::MAX);
            let tag = u32::try_from(a).unwrap_or(u32::MAX);
            let mut start = None;
            vectorcraft_geom::kurbo::flatten(&path, OUTLINE_FLATTEN_PT, |el| match el {
                vectorcraft_geom::kurbo::PathEl::MoveTo(p) => {
                    b.begin_line(slot, tag, 0.0);
                    number = number.wrapping_add(1);
                    b.point(p, number);
                    start = Some(p);
                }
                vectorcraft_geom::kurbo::PathEl::LineTo(p) => {
                    number = number.wrapping_add(1);
                    b.point(p, number);
                }
                vectorcraft_geom::kurbo::PathEl::ClosePath => {
                    if let Some(p) = start {
                        number = number.wrapping_add(1);
                        b.point(p, number);
                    }
                    b.end_line();
                }
                _ => {}
            });
            b.end_line();
        }
        Some((b.build(), colors))
    }

    /// The shape, in document points, of the area under working pixel `(x, y)` as it is sewn (a
    /// line's centre line), once the shapes are done.
    pub fn outline_at(&self, x: u32, y: u32) -> Option<vectorcraft_geom::BezPath> {
        let d = self.digitized()?;
        let a = d.areas().regions.at(x, y)? as usize;
        let to_doc = d.working().to_doc;
        match d.lines().get(a).and_then(Option::as_ref) {
            Some(line) => Some(to_doc * line.path()),
            None => d.shapes().shapes.get(a).map(|s| s.transformed(to_doc).to_bezpath()).filter(|p| !p.elements().is_empty()),
        }
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
        let (image, made) = stage(&mut self.image, &mut self.next, req.image_key(), || stages::image_stage(&req.image, &req.bytes, s))?;
        out.image = Some(image.clone());
        done(StageId::Image, &out, made);
        let key = TraceKey { image: image.number, settings: req.trace_settings() };
        let (traced, made) = stage(&mut self.trace, &mut self.next, key, || stages::trace_stage(&image.value, s, steps))?;
        out.trace = Some(traced.clone());
        done(StageId::Trace, &out, made);
        let key = RegionsKey { image: image.number, trace: traced.number, settings: req.regions_settings() };
        let (regions, made) = stage(&mut self.regions, &mut self.next, key, || stages::regions_stage(&image.value, &traced.value, s))?;
        out.regions = Some(regions.clone());
        done(StageId::Regions, &out, made);
        let key = ColoursKey { image: image.number, trace: traced.number, regions: regions.number, settings: req.colours_settings() };
        let (colours, made) =
            stage(&mut self.colours, &mut self.next, key, || stages::colours_stage(&image.value, &traced.value, &regions.value, s, req.palette))?;
        out.colours = Some(colours.clone());
        done(StageId::Colours, &out, made);
        let key = LinesKey { image: image.number, colours: colours.number, settings: req.lines_settings() };
        let (lines, made) = stage(&mut self.lines, &mut self.next, key, || stages::lines_stage(&image.value, &colours.value, s))?;
        out.lines = Some(lines.clone());
        done(StageId::Lines, &out, made);
        let key =
            ShapesKey { image: image.number, trace: traced.number, colours: colours.number, lines: lines.number, settings: req.shapes_settings() };
        let (shapes, made) =
            stage(&mut self.shapes, &mut self.next, key, || stages::shapes_stage(&image.value, &traced.value, &colours.value, &lines.value, s))?;
        out.shapes = Some(shapes.clone());
        done(StageId::Shapes, &out, made);
        let numbers = [image.number, traced.number, regions.number, colours.number, lines.number, shapes.number];
        let key = ObjectsKey { stages: numbers, settings: req.objects_settings() };
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

/// The objects Create would make of `d` on a copy of `req`'s document, its palette and embroidery
/// settings the window's.
fn objects_of(req: &WindowRequest, d: &Digitized) -> Result<ObjectsOut> {
    let started = clock();
    let mut doc = Document::clone(&req.original);
    doc.embroidery = req.setup.clone();
    with_palette(&mut doc, req.palette);
    let made = create::made(&mut doc, req.target, d, req.palette, &req.settings)?.map(|m| (Arc::new(doc), m));
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

/// The key of area `a` of `d` (its place in the areas): its deepest point in the image's own
/// pixels, its colour and its kind; `None` for the background.
fn key_of(d: &Digitized, a: usize) -> Option<AreaKey> {
    let area = d.areas().areas.get(a)?;
    // An area of a colour not sewn has no key: a change to it is kept, never found
    // (`KeptWhy::NotSewn`). One a change leaves out has: it can be changed back.
    d.colours().rows.get(area.row).filter(|r| r.sewn())?;
    let (x, y) = d.areas().regions.areas.get(a)?.deepest;
    let at = d.working().to_image * vectorcraft_geom::Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
    let color = d.colours().rows.get(area.row)?.color;
    Some(AreaKey { at: [at.x, at.y], color, line: super::changes::made_line(d.lines(), a) })
}

impl Outputs {
    /// What the run's objects were made on, the document with them, and what was made (`None`:
    /// nothing to make, or the objects aren't made yet).
    pub(super) fn made_for_create(&self) -> Option<(&Arc<Document>, Arc<Document>, Made)> {
        let o = &self.objects.as_ref()?.value;
        o.made.as_ref().map(|(d, m)| (&o.of, d.clone(), m.clone()))
    }
}
