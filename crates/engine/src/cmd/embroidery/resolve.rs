//! Where each value a sewing unit is planned with comes from (the project owner's choice,
//! 2026-10-20): the object's own setting for its part, then the option given to this one export or
//! read command, then the document's embroidery settings, then the built-in default. *Auto* (the
//! stitch chosen by the shape, the underlay by the width, a satin's spacing by its width) is a
//! rule the planner applies, never a value written in.
//!
//! Each layer is a set of the embroidery options ([`Options`]), one value or none for each, laid
//! over the next ([`Options::over`]) and turned into the planner's parameters by the one
//! conversion every unit takes ([`Options::params`]): with nothing set but the export's options,
//! a unit is planned exactly as before, by construction.
//!
//! The document's layer ([`DocLayer`]) is read by the kind of stitch a unit is sewn in, as a
//! fabric is described: a tatami fill (a fill, a stroke sewn as the area it paints) takes the
//! fabric's tatami values, its row spacing for 40 wt thread scaled to the thread as the built-in
//! spacing is; a satin column (a fill's narrow parts among them) takes the fabric's satin values;
//! a running line takes none of the fabric's. The machine's values and the thread's weight are
//! every unit's.
//!
//! What belongs to the design rather than to a unit (the jump and trim lengths between objects,
//! the cuts across a literal run's clipping, whether objects of one colour join) is resolved once a
//! plan ([`DesignSettings`]).

use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::{EmbroiderySetup, MachineSettings, StitchValues};
use vectorcraft_stitch::plan::{
    DENSITY_RANGE, JUMP_RANGE, MAX_SATIN_STITCH_RANGE, MAX_TRIM, MIN_STITCH_RANGE, PULL_RANGE, Params, SPACING_PERCENT_RANGE, STITCH_LENGTH_RANGE,
    Style, THREAD_WEIGHT, THREAD_WEIGHT_RANGE, Underlay, thread_scale,
};

use super::Options;

/// The column width given for what is not a satin column: fills and running lines have none.
const NO_WIDTH: f64 = 0.0;

/// Said once in a plan whose document's embroidery settings hold some this version doesn't read.
pub(crate) const DOCUMENT_SETTINGS_UNREAD: &str = "the document's embroidery settings hold some this version doesn't read (from a newer version): they are kept, and the design is sewn without them";
/// Said once in a plan whose document's embroidery settings hold values outside what this version
/// sews.
pub(crate) const DOCUMENT_SETTINGS_OUT_OF_RANGE: &str =
    "some of the document's embroidery settings lie outside what this version sews: each is sewn at the nearest value it takes";
/// Said once in a plan whose trim length is shorter than its jump length (the two from different
/// places: the document's machine and an export's options).
pub(crate) const TRIM_BELOW_JUMP: &str = "the trim length is shorter than the jump length, so jumps are cut from the jump length on";

/// One sewing unit's settings: the call's options (the export's, a read command's), then the
/// document's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitSettings<'a> {
    export: &'a Options,
    doc: &'a DocLayer,
}

impl<'a> UnitSettings<'a> {
    /// The settings of a unit planned with the call's options `export` over the document's `doc`.
    pub(crate) fn new(export: &'a Options, doc: &'a DocLayer) -> Self {
        Self { export, doc }
    }

    /// Every layer that doesn't depend on the stitch, the highest first, as one set of options.
    fn above_the_document(&self) -> Options {
        *self.export
    }

    /// The style the settings force, `None` for the rule (Auto).
    pub(crate) fn forced(&self) -> Option<Style> {
        self.above_the_document().forced()
    }

    /// How far a filled area grows all round before it is stitched (millimetres).
    pub(crate) fn expand(&self) -> f64 {
        self.above_the_document().expand()
    }

    /// The planner's parameters for a unit sewn in `style`, a satin column `width` wide: every
    /// layer laid over the next, the document's read for `style` with the thread's weight they
    /// resolve to.
    pub(crate) fn params(&self, style: Style, width: f64) -> Params {
        let above = self.above_the_document();
        let weight = above.thread_weight.or(self.doc.every.thread_weight).unwrap_or(THREAD_WEIGHT);
        above.over(&self.doc.for_style(style, weight)).params(style, width)
    }
}

/// What belongs to the design rather than to a unit, resolved once a plan: the jump and trim
/// lengths between objects and across a literal run's cuts, the shortest stitch, as the
/// parameters of a running line carry them, and whether objects of one colour join.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DesignSettings {
    /// A running line's parameters with the design's jump, trim and shortest stitch.
    pub(crate) connect: Params,
    /// Objects of one colour are sewn in one block where nothing lies between them; off, only
    /// one after another.
    pub(crate) join: bool,
}

impl DesignSettings {
    /// The design's settings with the call's options `export` over the document's `doc`, and the
    /// note to say when they put the trim length below the jump length.
    pub(crate) fn new(export: &Options, doc: &DocLayer) -> (Self, Option<&'static str>) {
        let merged = export.over(&doc.every);
        let d = Params::for_style(Style::Running);
        let (jump, trim) = (merged.jump_length.unwrap_or(d.jump), merged.trim_length.unwrap_or(d.trim));
        let note = (trim < jump).then_some(TRIM_BELOW_JUMP);
        (Self { connect: merged.params(Style::Running, NO_WIDTH), join: doc.join }, note)
    }
}

/// The document's embroidery settings as a plan reads them, once: by the kind of stitch they
/// apply to, and those every unit takes. A value a newer version gave outside what this version
/// sews is brought into range, one it can't read is left out (the built-in default then applies),
/// and either is said in the plan ([`DocLayer::notes`]).
#[derive(Clone, Debug)]
pub(crate) struct DocLayer {
    /// The fabric's tatami values: pull, row spacing for 40 wt thread, stitch length, underlay.
    tatami: Options,
    /// The fabric's satin values: pull, spacing percent, underlay.
    satin: Options,
    /// Every unit's: the thread's weight, the machine's jump, trim, shortest stitch, satin split.
    every: Options,
    /// Objects of one colour join one block where nothing lies between them.
    join: bool,
    /// What the plan says about settings it couldn't take as they are.
    pub(crate) notes: Vec<&'static str>,
}

impl Default for DocLayer {
    fn default() -> Self {
        Self { tatami: Options::default(), satin: Options::default(), every: Options::default(), join: true, notes: Vec::new() }
    }
}

/// A value read from a file, brought within `range` (one already within it stays as it is, bit for
/// bit); `clamped` set when it wasn't.
fn within(v: f64, (lo, hi): (f64, f64), clamped: &mut bool) -> f64 {
    if v.is_finite() && (lo..=hi).contains(&v) {
        return v;
    }
    *clamped = true;
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}

impl DocLayer {
    /// The settings of `d` (none: the built-in defaults).
    pub(crate) fn of(d: &Document) -> Self {
        d.embroidery.as_deref().map_or_else(Self::default, Self::read)
    }

    fn read(e: &EmbroiderySetup) -> Self {
        let (mut clamped, mut unread) = (false, !e.extra.is_empty() || e.unread.is_some());
        let mut stitch = |v: Option<&StitchValues>, tatami: bool| {
            let mut o = Options::default();
            let Some(v) = v else { return o };
            unread |= !v.extra.is_empty();
            o.pull = v.pull.map(|p| [within(p, PULL_RANGE, &mut clamped); 2]);
            if tatami {
                o.spacing = v.spacing.map(|s| within(s, DENSITY_RANGE, &mut clamped));
                o.length = v.length.map(|l| within(l, STITCH_LENGTH_RANGE, &mut clamped));
                // A satin's values that are a tatami's, and the other way round, are not read.
                unread |= v.spacing_percent.is_some();
            } else {
                o.spacing_percent = v.spacing_percent.map(|p| within(p, SPACING_PERCENT_RANGE, &mut clamped));
                unread |= v.spacing.is_some() || v.length.is_some();
            }
            o.underlay = v.underlay.as_deref().and_then(|id| {
                let kind = Underlay::from_id(id);
                unread |= kind.is_none();
                kind
            });
            o
        };
        let fabric = e.fabric.as_deref();
        let tatami = stitch(fabric.and_then(|f| f.values.tatami.as_ref()), true);
        let satin = stitch(fabric.and_then(|f| f.values.satin.as_ref()), false);
        if let Some(f) = fabric {
            unread |= !f.extra.is_empty() || !f.values.extra.is_empty();
        }
        let mut every = Options { thread_weight: e.thread_weight.map(|w| within(w, THREAD_WEIGHT_RANGE, &mut clamped)), ..Options::default() };
        if let Some(MachineSettings { jump_length, trim_length, min_stitch, split_length, speed: _, extra }) = e.machine.as_deref() {
            unread |= !extra.is_empty();
            every.jump_length = jump_length.map(|j| within(j, JUMP_RANGE, &mut clamped));
            every.trim_length = trim_length.map(|t| within(t, (JUMP_RANGE.0, MAX_TRIM), &mut clamped));
            every.min_stitch = min_stitch.map(|m| within(m, MIN_STITCH_RANGE, &mut clamped));
            every.split_length = split_length.map(|s| within(s, MAX_SATIN_STITCH_RANGE, &mut clamped));
        }
        let notes = [(unread, DOCUMENT_SETTINGS_UNREAD), (clamped, DOCUMENT_SETTINGS_OUT_OF_RANGE)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
            .collect();
        Self { tatami, satin, every, join: e.join_same_color.unwrap_or(true), notes }
    }

    /// The document's layer for a unit sewn in `style` with thread of `weight` (wt): the fabric's
    /// values for that kind of stitch (a tatami spacing, given for 40 wt thread, scaled to the
    /// thread as the built-in spacing is), over every unit's.
    fn for_style(&self, style: Style, weight: f64) -> Options {
        let mut fabric = match style {
            Style::Fill => self.tatami,
            Style::Satin => self.satin,
            Style::Running => Options::default(),
        };
        fabric.spacing = fabric.spacing.map(|s| s * thread_scale(weight));
        fabric.over(&self.every)
    }
}
