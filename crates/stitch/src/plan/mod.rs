//! How a path becomes stitches: running stitch, satin column and tatami fill.
//!
//! A *run* is a stretch the needle sews without lifting: a set of needle points in order. Runs
//! are joined into a block: a short move between two runs stays down (it is just the next stitch),
//! a long one becomes a [`crate::Command::Jump`], tied off and in, with a [`crate::Command::Trim`]
//! first when it is longer than [`Params::trim`].
//!
//! Parameters are millimetres and degrees. Everything is clamped to a sane range before use, so
//! out-of-range or non-finite input gives a usable stitch plan rather than a loop that never
//! ends: no function here panics.
//!
//! The three styles:
//!
//! - [`Style::Running`] follows a path: a single line of stitches, [`Params::stitch_length`]
//!   apart, with a lock stitch at each end ([`Params::lock`]).
//! - [`Style::Satin`] sews a column: the path is its centreline, [`Params::width`] its width, and
//!   the needle crosses from rail to rail, so the threads lie side by side. [`Params::pull`] and
//!   [`Params::pull_percent`] widen the column at each rail (pull compensation: the fabric draws
//!   the stitches together), and [`Params::push`] shortens its ends (push compensation: sewing
//!   pushes them out). The underlay ([`Params::underlay`]) follows the column's width.
//! - [`Style::Fill`] fills a shape with rows of stitches [`Params::density`] apart at
//!   [`Params::angle`] degrees, region by region, over an edge run and, for a large shape, a
//!   tatami underlay ([`Params::underlay`]). [`Params::stitch_length`] is the longest stitch a row
//!   is split into; pull compensation stretches each row past the edge at its ends.
//!
//! The spacing defaults are for 40 wt thread; [`Params::for_thread`] gives them for another
//! weight ([`thread_scale`]).
//!
//! The parts: `geometry` (polylines, spacing points along them, the rows' axes), `area` (what is
//! inside a filled shape, where rows cross it, travelling inside it), `runs` (runs joined into
//! stitches: jumps, trims, tie-ins and tie-offs), one module per style (`running`, `satin`, `fill`),
//! and `narrow` (a fill's narrow parts, sewn as satin columns).

mod area;
mod fill;
mod geometry;
mod narrow;
mod running;
mod runs;
mod satin;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_narrow;

use vectorcraft_geom::{BezPath, FillRule, Point};

pub use fill::MIN_AREA;
pub use fill::TATAMI_UNDERLAY_MIN;
pub(crate) use fill::fill_with_columns;
pub use narrow::{Column, columns as narrow_columns, columns_until as narrow_columns_until};
pub use runs::join_cut;
pub use satin::{SATIN_EDGE_UNDERLAY, SATIN_ZIGZAG_UNDERLAY, satin_auto_underlay, satin_spacing};

use crate::{Error, Stitch};
pub use geometry::MAX_COORD_MM;
pub use geometry::in_range;
use geometry::{Axes, SubPath, length, subpaths};

/// A move longer than this lifts the needle (a jump) instead of being sewn, unless
/// [`Params::jump`] says otherwise (millimetres). An estimate: past 5 mm a sewn move lies loose on
/// the fabric; machines' jump trimming starts about there (Brother's settings run from 5 mm).
const JUMP: f64 = 5.0;
/// A jump longer than this cuts the thread first, unless [`Params::trim`] says otherwise
/// (millimetres). Shorter jumps (hops between letters or seeds) are left for the machine or a
/// person to cut: machines with jump trimming cut those longer than a length set on the machine
/// (Brother's settings run from 5 to 50 mm), and Ink/Stitch adds no trims unless an object asks
/// for one. Longer ones are cut in the file, so they don't lie loose on a machine that can't cut
/// jumps itself. A choice within those bounds, not a published value.
pub(crate) const TRIM: f64 = 10.0;
/// The longest satin stitch before it is split (millimetres): longer stitches snag and sag.
/// Professional software makes it a setting, bounded by the machine format (12.1 mm per record in
/// DST). Wilcom recommends an Auto Split of 7 mm and satin widths within 10 mm (the article cited
/// for [`DENSITY`] below); Melco's design checker lets satin stitches up to 10 mm pass. 10 mm, by
/// judgement: splitting at 7 would split every satin wider than that, and the narrow parts of fills
/// sewn as satin reach 10 mm.
const MAX_SATIN_STITCH: f64 = 10.0;
/// The narrowest satin column (millimetres): a stroke, or a fill's narrow part, narrower than this
/// is sewn as a running line along its middle, since below about a millimetre a satin's two rails
/// are too close for its crossings to lie side by side. Melco's design checker flags satin stitches
/// shorter than 1 mm (DesignShop v11 user manual, Design Checker Tab, p. 38,
/// <http://www.melco-service.com/docs/DSv11_Manual.pdf>), and its machine manual keeps satin
/// stitches 1 mm or longer (OS manual, pp. 301–302,
/// <https://www.melco-service.com/docs/X/melco-os-ai-en.pdf>).
pub const SATIN_MIN_WIDTH: f64 = 1.0;
/// The widest satin column (millimetres): a wider one is sewn this wide. Far beyond what a satin
/// can cover (professional practice tops out near 10–12 mm), a bound on the stitching.
pub const MAX_SATIN_WIDTH: f64 = 50.0;
/// The distance between a fill's rows (millimetres), also the density a satin column falls back to
/// when its spacing isn't picked by width: tight enough that 40 wt thread covers without gaps.
/// Wilcom lists 0.38 mm as its tatami default
/// (<https://wilcom.com/resources/blog/mastering-basic-stitch-types-satin-stitch-and-tatami-stitch>),
/// and David Morrish's *Digital Embroidery* (2025, ch. 2) gives 0.40 mm as the spacing where 40 wt
/// threads lie flush and 0.38 mm as the one that leaves no gaps. How much a fabric needs is for
/// fabric presets.
pub(crate) const FILL_SPACING: f64 = 0.38;
/// The longest stitch of a fill row (millimetres). Wilcom's tatami default (same page as
/// [`FILL_SPACING`]).
pub(crate) const FILL_STITCH: f64 = 4.0;
/// The longest stitch of a running line (millimetres): short enough to follow curves and corners
/// closely, long enough that the thread doesn't pile up. An estimate within the 2–3 mm usual for
/// outlines.
const RUNNING_STITCH: f64 = 2.5;
/// Shorter stitches are left out (millimetres): a tiny stitch can break the thread or cut the
/// fabric, and a repeated point is noise. An estimate.
const MIN_STITCH: f64 = 0.4;
/// The direction of a fill's rows, degrees clockwise as seen on the page (0: left to right; the
/// planner's y axis points down the page, see [`Params::angle`]): from the top left to the bottom
/// right. Diagonal, so that rows don't run along a shape's horizontal and vertical edges, where
/// they would show each step of the edge. By judgement.
pub const FILL_ANGLE: f64 = 45.0;
/// Pull compensation (millimetres added at each side of a stitch: each rail of a satin column, each
/// end of a fill row): the fabric draws the thread in, so stitching sewn exactly as drawn comes out
/// narrower and leaves a gap of background where two shapes meet. Wilcom's automatic pull
/// compensation is one overstitch allowance per fabric, 0.20 mm for drill and cotton (0.35 for
/// T-shirts and 0.40 for fleece: fabric presets)
/// (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/stabilizing/stabilizing-17.htm>).
/// Satin columns shrink most, and tatami fills less but noticeably over large areas
/// (<https://wilcom.com/resources/blog/push-and-pull-compensation>): both get the same value (an
/// estimate; fabric presets may part them).
pub(crate) const PULL: f64 = 0.2;
/// Pull compensation that grows with the stitch (percent of a satin column's width, or of a fill
/// row's length, added at each side): none by default. Ink/Stitch offers it on top of the fixed
/// value, so that wide columns get more than narrow ones
/// (<https://inkstitch.org/docs/stitches/satin-column/>).
const PULL_PERCENT: f64 = 0.0;
/// Push compensation (millimetres a satin column is sewn shorter at each end): the stitches pile
/// up at a column's open ends and push the fabric out, so a column comes out longer than drawn
/// (<https://wilcom.com/resources/blog/push-and-pull-compensation>; Ink/Stitch shortens the
/// column's start and end by a setting of its own). No starting value is published: none by
/// default, so a column is sewn as long as it is drawn.
const PUSH: f64 = 0.0;
/// The thread the default spacings are for (its weight: kilometres of thread per kilogram, so a
/// larger number is a thinner thread, <https://en.wikipedia.org/wiki/Thread_(textiles)>): 40 wt,
/// standard embroidery thread, the one the default fill spacing (rows 0.38 mm apart) covers
/// without gaps.
pub const THREAD_WEIGHT: f64 = 40.0;
/// How wide thread of [`THREAD_WEIGHT`] lies on the fabric, millimetres: the spacing at which its
/// rows lie flush, 0.40 mm in David Morrish's *Digital Embroidery* (2025, ch. 2; see
/// `FILL_SPACING`) and in Madeira's stitch density chart for 40 wt (see [`thread_scale`]). An
/// opened design's lines and the stitch preview draw thread this wide ([`thread_width`]), so the
/// default 0.38 mm rows just cover, as sewn thread does. An estimate of a laid thread, which
/// spreads by fabric and tension.
pub const THREAD_WIDTH: f64 = 0.4;
/// Filled parts narrower than this (millimetres) are sewn as satin columns rather than tatami
/// ([`Params::narrow`]), as auto-digitizers choose satin for narrow parts. One digitizing guide
/// keeps satin to parts narrower than 8–10 mm, past which its stitches lie loose
/// (<https://www.drawstitch.com/blog/knowledge-base/how-to-digitize-satin-stitches/>), and Wilcom
/// warns that a satin too wide sews loose and covers badly
/// (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Digitizing/stitches/stitches-8.htm>);
/// 7 mm stays below that, with room for pull compensation. An estimate.
pub(crate) const NARROW_SATIN: f64 = 7.0;
/// A satin column's width when none is given (millimetres). The export always gives one, the
/// stroke's width; this is for callers of the planner that don't.
const SATIN_WIDTH: f64 = 3.0;
/// The share of the spacing picked by a satin column's width it is sewn at when nothing says
/// another ([`Params::spacing_scale`]): all of it.
const SPACING_SCALE: f64 = 1.0;
/// Percent in a whole: a percentage over this is a share.
pub const PERCENT: f64 = 100.0;
/// The share of the spacing picked by a satin column's width that leaves it as picked, in percent:
/// all of it ([`SPACING_SCALE`]), what a satin spacing percent of no fabric is.
pub const SPACING_PERCENT_AS_PICKED: f64 = SPACING_SCALE * PERCENT;
/// The underlay's own stitch length (millimetres): short, to hold the fabric still. An estimate.
const UNDERLAY_STITCH: f64 = 2.0;
/// The most runs one shape may produce (a guard against absurd parameters).
const MAX_RUNS: usize = 1_000_000;
/// The most stitches one shape may plan: [`try_stitches`] refuses a shape that would need more.
pub(crate) const MAX_STITCHES: usize = 4_000_000;

/// How a shape is stitched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// One line of stitches along the path.
    #[default]
    Running,
    /// A column across [`Params::width`].
    Satin,
    /// A shape filled with rows (tatami).
    Fill,
}

impl Style {
    pub fn id(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Satin => "satin",
            Self::Fill => "fill",
        }
    }

    /// The style `id` names (`running`, `satin`, `fill`).
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "running" => Self::Running,
            "satin" => Self::Satin,
            "fill" => Self::Fill,
            _ => return None,
        })
    }
}

/// What is sewn under a shape before its top stitching, to hold the fabric still
/// ([`Params::underlay`]). The kinds are a satin column's and a fill's together, as the settings
/// name them: a kind of the other's is taken as its nearest (a fill takes `Center` as `Edge` and
/// `EdgeZigzag` as `EdgeTatami`; a satin takes `EdgeTatami` as `EdgeZigzag`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Underlay {
    /// None.
    None,
    /// Chosen by the shape: under a satin column by its width (a centre run, an edge run from 2 mm,
    /// an edge run and a zigzag from 4.5 mm), under a fill an edge run and, when it is at least
    /// 10 mm across both ways, a sparse tatami layer.
    #[default]
    Auto,
    /// A run along a satin column's centre.
    Center,
    /// A run along the edges.
    Edge,
    /// An edge run, then a zigzag across a satin column.
    EdgeZigzag,
    /// An edge run, then a sparse tatami layer under a fill whatever its size.
    EdgeTatami,
}

impl Underlay {
    /// Every kind, as the settings list them.
    pub const ALL: [Self; 6] = [Self::None, Self::Auto, Self::Center, Self::Edge, Self::EdgeZigzag, Self::EdgeTatami];

    /// Its name in parameters: `none`, `auto`, `center`, `edge`, `edgeZigzag`, `edgeTatami`.
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Auto => "auto",
            Self::Center => "center",
            Self::Edge => "edge",
            Self::EdgeZigzag => "edgeZigzag",
            Self::EdgeTatami => "edgeTatami",
        }
    }

    /// The kind `id` names ([`Underlay::id`]).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.id() == id)
    }
}

/// What one shape is stitched as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub style: Style,
    /// The longest stitch along a running line or a fill row, millimetres.
    pub stitch_length: f64,
    /// The distance between threads, millimetres: row to row for fill; for satin between two
    /// penetrations on the same rail (peak to peak), its crossings lying half that apart.
    pub density: f64,
    /// The direction the fill's rows run, degrees (0: left to right), clockwise as seen on the page:
    /// the planner's y axis points down the page, as the document's does. People and the engine's
    /// commands count angles counter-clockwise, as everywhere else in VectorCraft; the engine
    /// converts them in one place (`cmd::embroidery::options::planner_angle`).
    pub angle: f64,
    /// Pull compensation: how much farther the stitches reach at each side so the fabric's pull
    /// does not leave a gap of background at an edge, millimetres, at two sides: a satin
    /// column's rails (the left one as its path runs, then the right one), a fill row's ends (its
    /// start along the rows' direction, then its end). Running lines have none.
    pub pull: [f64; 2],
    /// Pull compensation that grows with the stitch, percent added at each side (as
    /// [`Params::pull`], on top of it): of a satin column's width, of a fill row's length.
    pub pull_percent: [f64; 2],
    /// Push compensation: how much shorter an open satin column is sewn at its start and its end
    /// (as its path is drawn), millimetres. Sewing pushes the fabric out at a column's ends.
    pub push: [f64; 2],
    /// The thread's weight (wt, kilometres per kilogram: 40 is standard embroidery thread). Auto
    /// spacing ([`Params::auto_spacing`]) and [`Params::for_thread`]'s spacing follow it
    /// ([`thread_scale`]).
    pub thread_weight: f64,
    /// What is sewn under the shape first ([`Underlay`]).
    pub underlay: Underlay,
    /// A move longer than this lifts the needle (a jump) instead of running the thread across,
    /// millimetres. A jump longer than this is tied off before and in after (when [`Params::lock`]
    /// is on), so that the machine or a person can cut it without the stitching coming undone.
    pub jump: f64,
    /// A jump longer than this cuts the thread first (a trim), millimetres. Never less than
    /// [`Params::jump`].
    pub trim: f64,
    /// Lock the thread so it does not pull out: a running line gets a lock stitch (a short back
    /// stitch) at each end of each run; satin and fill a tie-in after every cut and a tie-off
    /// before it.
    pub lock: bool,
    /// Shorter stitches are left out (a row's remainder, a sliver of a shape), millimetres.
    pub min_stitch: f64,
    /// The satin column's width, millimetres (unused by the other styles).
    pub width: f64,
    /// The longest satin stitch, millimetres: a crossing longer than this is split into shorter
    /// stitches (auto split), its penetrations spread so they don't line up into a seam.
    pub max_satin: f64,
    /// Satin only: pick [`Params::density`] from the column's width ([`satin_spacing`]) instead of
    /// using it as given. On by default; an explicit density turns it off.
    pub auto_spacing: bool,
    /// Satin only: the share of the spacing picked by the column's width ([`Params::auto_spacing`])
    /// it is sewn at, 1 for the spacing as picked (a fabric sets another: the document's satin
    /// spacing percent). An explicit density, used as given, ignores it.
    pub spacing_scale: f64,
    /// Fill only: the widest part of a filled shape sewn as a satin column instead of tatami,
    /// millimetres; 0 sews every part as tatami. The planner's [`narrow_columns`] finds such parts,
    /// and [`try_fill_with_columns`] sews them with the rest of the shape, which the caller cuts
    /// out (the planner has no path operations); [`try_stitches`] itself fills the whole shape.
    pub narrow: f64,
}

// The ranges [`Params::sane`] clamps each value to (millimetres, lowest and highest). They are
// not settings: they keep the planning finite and the stitches within what a machine sews, and
// anything outside them is a mistake or hostile input. The commands refuse a value outside them
// rather than let it be clamped (the engine's embroidery options read them from here).

/// A stitch length: from 0.2 mm (two of the 0.1 mm steps the machine formats count in) to 12 mm,
/// about the longest move one record of the machine formats holds (12.1 mm in DST).
pub const STITCH_LENGTH_RANGE: (f64, f64) = (0.2, 12.0);
/// The distance between threads: from 0.05 mm (twenty threads a millimetre, far denser than any
/// fabric takes) to 5 mm (rows that no longer cover).
pub const DENSITY_RANGE: (f64, f64) = (0.05, 5.0);
/// Pull compensation: none to 2 mm at a side, far more than the most stretchy fabric needs. Also
/// the most a side gets with [`Params::pull_percent`] added.
pub const PULL_RANGE: (f64, f64) = (0.0, 2.0);
/// Pull compensation by size: none to half the stitch again at each side (a column twice as wide).
pub const PULL_PERCENT_RANGE: (f64, f64) = (0.0, 50.0);
/// Push compensation: none to 5 mm at an end. A column keeps at least [`MIN_PUSHED`] of its length
/// whatever is asked.
pub const PUSH_RANGE: (f64, f64) = (0.0, 5.0);
/// The share of an open satin column's length that push compensation leaves, at least: both ends
/// are shortened in proportion when they would take more. Push compensation corrects a fraction of
/// a millimetre; more than half a column is a mistake.
const MIN_PUSHED: f64 = 0.5;
/// The widest part sewn as a satin column: none to 12 mm, the longest stitch a column's crossing
/// may be before it is split ([`MAX_SATIN_STITCH_RANGE`]).
pub const NARROW_RANGE: (f64, f64) = (0.0, 12.0);
/// The thread's weight: from 10 wt (a heavy decorative thread, thicker than any embroidery
/// thread) to 150 wt (finer than the finest bobbin and lettering threads).
pub const THREAD_WEIGHT_RANGE: (f64, f64) = (10.0, 150.0);
/// The tatami row spacings a fabric may give for 40 wt thread (millimetres): those that every weight
/// of [`THREAD_WEIGHT_RANGE`] scales ([`thread_scale`], 0.52 at 150 wt to 2.0 at 10 wt) to a
/// spacing within [`DENSITY_RANGE`], so that no plan has to clamp one silently (0.1 mm at 150 wt is
/// 0.052, 2.5 mm at 10 wt is 5.0). Bounds on what is asked.
pub const FABRIC_SPACING_RANGE: (f64, f64) = (0.1, 2.5);
/// The longest move sewn rather than jumped: from 0.5 mm to 20 mm, past which a sewn move would be
/// a loose thread across the design.
pub const JUMP_RANGE: (f64, f64) = (0.5, 20.0);
/// The longest jump left uncut: from the jump length itself (every jump cut) to a metre (none).
pub const MAX_TRIM: f64 = 1000.0;
/// The shortest stitch kept: from 0.05 mm to 3 mm.
pub const MIN_STITCH_RANGE: (f64, f64) = (0.05, 3.0);
/// The longest satin stitch before it is split: from 2 mm to the 12 mm of [`STITCH_LENGTH_RANGE`].
pub const MAX_SATIN_STITCH_RANGE: (f64, f64) = (2.0, 12.0);
/// A share of a satin column's spacing picked by its width, percent: from half (twice as dense) to
/// twice (half as dense), within which every width, at every thread weight, keeps a spacing in
/// [`DENSITY_RANGE`] (from 0.096 mm, half the narrowest picked spacing at 150 wt, to 2.0 mm). Bounds
/// on what is asked.
pub const SPACING_PERCENT_RANGE: (f64, f64) = (50.0, 200.0);

impl Default for Params {
    fn default() -> Self {
        Self::for_style(Style::Running)
    }
}

impl Params {
    /// The defaults of one style, for 40 wt thread ([`THREAD_WEIGHT`]).
    pub fn for_style(style: Style) -> Self {
        Self::for_thread(style, THREAD_WEIGHT)
    }

    /// The defaults of one style for thread of `weight` (wt): the spacing between threads follows
    /// the thread's thickness ([`thread_scale`]), a fill's rows and a satin column's crossings alike.
    pub fn for_thread(style: Style, weight: f64) -> Self {
        let weight = sane_weight(weight);
        let density = FILL_SPACING * thread_scale(weight);
        match style {
            // A running line uses no density, angle, compensation, underlay or width.
            Style::Running => Self {
                style,
                stitch_length: RUNNING_STITCH,
                density,
                angle: 0.0,
                pull: [0.0; 2],
                pull_percent: [0.0; 2],
                push: [0.0; 2],
                thread_weight: weight,
                underlay: Underlay::None,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: 0.0,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: false,
                spacing_scale: SPACING_SCALE,
                narrow: 0.0,
            },
            // A satin column's stitches cross from rail to rail: it uses no stitch length or angle
            // (a satin without a width is filled instead, and then they apply).
            Style::Satin => Self {
                style,
                stitch_length: RUNNING_STITCH,
                density,
                angle: 0.0,
                pull: [PULL; 2],
                pull_percent: [PULL_PERCENT; 2],
                push: [PUSH; 2],
                thread_weight: weight,
                underlay: Underlay::Auto,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: SATIN_WIDTH,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: true,
                spacing_scale: SPACING_SCALE,
                narrow: 0.0,
            },
            // A fill has no ends to push in: its rows run edge to edge.
            Style::Fill => Self {
                style,
                stitch_length: FILL_STITCH,
                density,
                angle: FILL_ANGLE,
                pull: [PULL; 2],
                pull_percent: [PULL_PERCENT; 2],
                push: [0.0; 2],
                thread_weight: weight,
                underlay: Underlay::Auto,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: 0.0,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: false,
                spacing_scale: SPACING_SCALE,
                narrow: NARROW_SATIN,
            },
        }
    }

    /// The same parameters with every value clamped to a range that produces a stitch plan:
    /// not finite falls back to the style's default, and each distance is bounded so that no
    /// loop can run away (the `*_RANGE` constants below).
    pub fn sane(self) -> Self {
        let d = Self::for_style(self.style);
        let fin = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        let clamp = |v: f64, fallback: f64, (lo, hi): (f64, f64)| fin(v, fallback).clamp(lo, hi);
        let sides = |v: [f64; 2], fallback: [f64; 2], range: (f64, f64)| [clamp(v[0], fallback[0], range), clamp(v[1], fallback[1], range)];
        let angle = fin(self.angle, d.angle) % 360.0;
        let jump = clamp(self.jump, d.jump, JUMP_RANGE);
        Self {
            style: self.style,
            stitch_length: clamp(self.stitch_length, d.stitch_length, STITCH_LENGTH_RANGE),
            density: clamp(self.density, d.density, DENSITY_RANGE),
            angle: if angle < 0.0 { angle + 360.0 } else { angle },
            pull: sides(self.pull, d.pull, PULL_RANGE),
            pull_percent: sides(self.pull_percent, d.pull_percent, PULL_PERCENT_RANGE),
            push: sides(self.push, d.push, PUSH_RANGE),
            thread_weight: clamp(self.thread_weight, d.thread_weight, THREAD_WEIGHT_RANGE),
            underlay: self.underlay,
            jump,
            trim: clamp(self.trim, d.trim, (jump, MAX_TRIM)),
            lock: self.lock,
            min_stitch: clamp(self.min_stitch, d.min_stitch, MIN_STITCH_RANGE),
            width: clamp(self.width, d.width, (0.0, MAX_SATIN_WIDTH)),
            max_satin: clamp(self.max_satin, d.max_satin, MAX_SATIN_STITCH_RANGE),
            auto_spacing: self.auto_spacing,
            spacing_scale: clamp(self.spacing_scale, d.spacing_scale, (SPACING_PERCENT_RANGE.0 / PERCENT, SPACING_PERCENT_RANGE.1 / PERCENT)),
            narrow: clamp(self.narrow, d.narrow, NARROW_RANGE),
        }
    }

    /// The spacing of a satin column `width` wide (millimetres, peak to peak on a rail): picked by
    /// its width and the thread with [`Params::auto_spacing`] ([`satin_spacing`], [`thread_scale`]),
    /// the share [`Params::spacing_scale`] of it, else [`Params::density`].
    pub fn column_spacing(&self, width: f64) -> f64 {
        if self.auto_spacing { satin_spacing(width) * thread_scale(self.thread_weight) * self.spacing_scale } else { self.density }
    }

    /// How much farther a stitch `length` long reaches at each side for pull compensation
    /// ([`Params::pull`] plus [`Params::pull_percent`] of `length`), millimetres, at most 2 mm at
    /// a side (the most [`Params::sane`] lets [`Params::pull`] be).
    pub fn compensation(&self, length: f64) -> [f64; 2] {
        let length = if length.is_finite() { length.max(0.0) } else { 0.0 };
        let side = |k: usize| {
            let (pull, percent) = (self.pull.get(k).copied().unwrap_or(0.0), self.pull_percent.get(k).copied().unwrap_or(0.0));
            let v = pull + percent / 100.0 * length;
            if v.is_finite() { v.clamp(0.0, PULL_RANGE.1) } else { 0.0 }
        };
        [side(0), side(1)]
    }
}

/// How a spacing for 40 wt thread ([`THREAD_WEIGHT`]) changes for thread of `weight`: in
/// proportion to the thread's thickness. Thread weight counts kilometres per kilogram, so a
/// thread's cross-section is inversely proportional to it and its thickness to its square root:
/// `(40 / weight)^½`. Madeira's stitch density chart, by thread, follows the same curve: 0.80,
/// 0.50, 0.40 and 0.35 mm for its Classic 12, 30, 40 and 60 wt, where this gives 0.73, 0.46, 0.40
/// and 0.33 from 0.40 (its 75 wt Polyneon, 0.20 mm, is finer than the curve)
/// (<https://www.madeirausa.com/_resources/common/userfiles/file/Resources/TH.Reads/Density.pdf>).
/// The chart is for satin; fills follow it too here, an estimate: no source scales fill spacing
/// by thread. (Wilcom's satin auto spacing shifts its spacing by thread class too, by fixed
/// offsets of 0.01, 0.03, −0.03 and −0.06 with no unit stated, read here as millimetres, an
/// assumption,
/// <https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Digitizing/stitches/stitches-10.htm>.)
pub fn thread_scale(weight: f64) -> f64 {
    /// The square root: the thread's thickness follows it.
    const SPACING_EXPONENT: f64 = 0.5;
    (THREAD_WEIGHT / sane_weight(weight)).powf(SPACING_EXPONENT)
}

/// How wide thread of `weight` (wt) lies, millimetres: [`THREAD_WIDTH`] for 40 wt, scaled by the
/// thread's thickness ([`thread_scale`]), as the planner scales its spacing.
pub fn thread_width(weight: f64) -> f64 {
    THREAD_WIDTH * thread_scale(weight)
}

/// A thread weight within [`THREAD_WEIGHT_RANGE`]; one that isn't finite is [`THREAD_WEIGHT`].
fn sane_weight(weight: f64) -> f64 {
    if weight.is_finite() { weight.clamp(THREAD_WEIGHT_RANGE.0, THREAD_WEIGHT_RANGE.1) } else { THREAD_WEIGHT }
}

/// [`try_stitches`] without its checks, for this crate's tests.
#[cfg(test)]
pub(crate) fn stitches(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    stitches_from(path, rule, p, None)
}

/// [`try_stitches_from`] without its checks: what the planners build. Unchecked, a shape far out
/// of range or a tiny stitch length builds without bound, so only the checked entry points are
/// public.
pub(crate) fn stitches_from(path: &BezPath, rule: FillRule, p: &Params, from: Option<Point>) -> Vec<Stitch> {
    let p = p.sane();
    match sewn_style(&p) {
        Style::Running => running::running_from(path, &p, from),
        Style::Satin => satin::satin_from(path, &p, from),
        Style::Fill => fill::fill_from(path, rule, &p, from),
    }
}

/// The style a shape with `p` is sewn in: its own, except that a satin column needs a width, and a
/// satin without one (a fill whose `stitch` option is satin) is filled instead.
pub fn sewn_style(p: &Params) -> Style {
    let p = p.sane();
    match p.style {
        Style::Satin if p.width > 0.0 => Style::Satin,
        Style::Satin | Style::Fill => Style::Fill,
        Style::Running => Style::Running,
    }
}

/// The stitches of `path` in style `p`: one block, in needle order; refused with
/// [`Error::TooLarge`] when the shape would need more than [`MAX_STITCHES`] (a huge shape, or a
/// tiny stitch length or spacing), estimated from the geometry before any stitch is planned, so an
/// absurd request costs no memory, and when it lies out of range ([`in_range`]).
///
/// `rule` decides what a fill covers when its subpaths overlap (a hole wound against its shape
/// is a hole under the non-zero rule; under the even-odd rule any two crossings change side).
pub fn try_stitches(path: &BezPath, rule: FillRule, p: &Params) -> crate::Result<Vec<Stitch>> {
    try_stitches_from(path, rule, p, None)
}

/// [`try_stitches`], the needle at `from` before the shape starts (where the object sewn before it
/// ended): the shape starts as near it as it can, which saves jumps and trims. `None`: the shape
/// starts where it is drawn to.
pub fn try_stitches_from(path: &BezPath, rule: FillRule, p: &Params, from: Option<Point>) -> crate::Result<Vec<Stitch>> {
    let p = p.sane();
    placed(path)?;
    within_limit(estimate(&subpaths(path), &p))?;
    Ok(stitches_from(path, rule, &p, from))
}

/// A filled shape sewn in parts: `rest`, the shape without its narrow `columns` (cut out by the
/// caller with path operations), as a tatami fill, and each column as a satin column whose width
/// follows the shape, one after another, whichever starts nearest the needle next, travelling
/// inside the whole `shape`; `fill` and `satin` are the two kinds of parts' parameters. Refused
/// with [`Error::TooLarge`] when it would need more than [`MAX_STITCHES`] stitches (the whole
/// `shape` filled, plus each column's crossings and underlay, estimated before any stitch is
/// planned), and when a shape lies out of range ([`in_range`]).
pub fn try_fill_with_columns(
    shape: &BezPath,
    rule: FillRule,
    rest: &BezPath,
    columns: &[Column],
    fill: &Params,
    satin: &Params,
    from: Option<Point>,
) -> crate::Result<Vec<Stitch>> {
    /// More than a column's points per millimetre of its middle beyond its crossings: its
    /// underlay's (up to three runs at [`UNDERLAY_STITCH`]) and its splits.
    const UNDERLAYS: f64 = 3.0;
    let (f, s) = (Params { style: Style::Fill, ..*fill }.sane(), Params { style: Style::Satin, ..*satin }.sane());
    placed(shape)?;
    placed(rest)?;
    let closest = s.density.min(satin_spacing(f64::INFINITY) * thread_scale(s.thread_weight) * s.spacing_scale);
    let column = |c: &Column| {
        // Its widest crossing, with the most pull compensation at both sides, split as satin's are.
        let widest = c.stations.iter().map(|(a, b)| a.distance(*b)).fold(0.0, f64::max);
        let split = satin::splits(widest + 2.0 * PULL_RANGE.1, s.max_satin);
        let crossings = c.length() * satin::CROSSINGS_PER_SPACING / closest * satin::MAX_RAIL_STRETCH;
        (crossings + 1.0) * split + c.length() * (UNDERLAYS / UNDERLAY_STITCH + split / satin::ZIGZAG_UNDERLAY_SPACING)
    };
    within_limit(estimate(&subpaths(shape), &f) + columns.iter().map(column).sum::<f64>())?;
    Ok(fill_with_columns(shape, rule, rest, columns, &f, &s, from))
}

/// Refused with [`Error::TooLarge`] when `path` lies beyond [`MAX_COORD_MM`] or has a coordinate
/// that isn't a number: flattening it could take without bound.
fn placed(path: &BezPath) -> crate::Result<()> {
    if in_range(path) {
        return Ok(());
    }
    Err(Error::TooLarge(format!(
        "a shape lies more than {MAX_COORD_MM} mm from the origin, or has a coordinate that isn't a number: it can't be stitched"
    )))
}

/// Refused with [`Error::TooLarge`] when a shape would need `n` stitches, more than
/// [`MAX_STITCHES`] (or a number that isn't finite).
fn within_limit(n: f64) -> crate::Result<()> {
    if !n.is_finite() || n > MAX_STITCHES as f64 {
        return Err(Error::TooLarge(format!(
            "a shape would need about {n:.0} stitches, more than the {MAX_STITCHES} one shape may have: use a longer stitch, wider spacing or a smaller design"
        )));
    }
    Ok(())
}

/// How many points `subs` plan into in style `p` at most: an upper bound, so that a shape refused
/// past [`MAX_STITCHES`] is refused before any of it is planned. Not finite when a point isn't, or
/// when a fill needs more rows than one layer may have ([`fill::MAX_ROWS`]).
fn estimate(subs: &[SubPath], p: &Params) -> f64 {
    /// More than the points a subpath adds beyond its stitches: the lock stitch, tie-in or tie-off
    /// at each of its two ends.
    const POINTS_PER_SUBPATH: f64 = 6.0;
    /// The runs a satin column's underlay sews along its length, at most: a centre run, or an edge
    /// run along both sides (its zigzag is counted apart).
    const SATIN_UNDERLAY_RUNS: f64 = 2.0;
    if subs.iter().flat_map(|s| s.pts.iter()).any(|q| !(q.x.is_finite() && q.y.is_finite())) {
        return f64::NAN;
    }
    let total: f64 = subs.iter().map(|s| length(&s.points(), s.closed)).sum();
    let ends = POINTS_PER_SUBPATH * subs.len() as f64;
    match sewn_style(p) {
        Style::Running => total / p.stitch_length + ends,
        Style::Satin => {
            // Each crossing split as `split_long` splits the longest a crossing can be (the column
            // and the most pull compensation at both rails); round a bend, the crossings come up to
            // MAX_RAIL_STRETCH times closer along the centre line.
            let split = satin::splits(p.width + 2.0 * PULL_RANGE.1, p.max_satin);
            let crossings = total * satin::CROSSINGS_PER_SPACING / p.column_spacing(p.width).min(p.density) * satin::MAX_RAIL_STRETCH;
            let underlay = total * SATIN_UNDERLAY_RUNS / UNDERLAY_STITCH + total / satin::ZIGZAG_UNDERLAY_SPACING * split;
            (crossings + 1.0) * split + underlay + ends
        }
        Style::Fill => {
            let top = layer_points(subs, p.angle, p.density, p.stitch_length);
            let underlay = if p.underlay != Underlay::None {
                total / UNDERLAY_STITCH
                    + layer_points(subs, p.angle + fill::UNDERLAY_TURN, fill::TATAMI_UNDERLAY_SPACING, fill::TATAMI_UNDERLAY_STITCH)
            } else {
                0.0
            };
            // A subpath that encloses nothing is sewn as a running line.
            top + underlay + total / p.stitch_length + ends
        }
    }
}

/// The most points one layer of rows at `angle`, `spacing` apart and split into stitches up to
/// `stitch` long, can plan across `subs`: every row across the shapes' extent split along all of
/// it, and each stretch of a row (two edge crossings) its ends, its stagger's part stitch and the
/// row a region takes of its neighbour's. An edge crosses as many rows as fit across it: counted
/// edge by edge, without finding a row's stretches. Infinite past [`fill::MAX_ROWS`] rows.
fn layer_points(subs: &[SubPath], angle: f64, spacing: f64, stitch: f64) -> f64 {
    /// More than the points each edge crossing of a row adds beyond the row's stitches.
    const POINTS_PER_CROSSING: f64 = 4.0;
    let axes = Axes::new(angle);
    let extent = |f: &dyn Fn(vectorcraft_geom::Point) -> f64| {
        let (lo, hi) =
            subs.iter().flat_map(|s| s.pts.iter()).map(|q| f(*q)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
        if hi >= lo { hi - lo } else { 0.0 }
    };
    let across = extent(&|q| axes.across(q));
    if across / spacing > fill::MAX_ROWS as f64 {
        return f64::INFINITY;
    }
    // One row more than fit in the extent: a part row at the edge.
    let rows = across / spacing + 1.0;
    let crossings: f64 = subs
        .iter()
        .map(|s| {
            let n = s.pts.len();
            (0..n)
                .filter_map(|i| Some((*s.pts.get(i)?, *s.pts.get((i + 1) % n)?)))
                .map(|(a, b)| (axes.across(b) - axes.across(a)).abs() / spacing + 1.0)
                .sum::<f64>()
        })
        .sum();
    rows * (extent(&|q| axes.along(q)) / stitch + 1.0) + crossings * POINTS_PER_CROSSING
}
