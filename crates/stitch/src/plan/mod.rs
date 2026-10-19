//! How a path becomes stitches: running stitch, satin column and tatami fill.
//!
//! A *run* is a stretch the needle sews without lifting: a set of needle points in order. Runs
//! are joined into a block: a short move between two runs stays down (it is just the next stitch),
//! a long one becomes a [`Command::Jump`], tied off and in, with a [`Command::Trim`] first when it
//! is longer than [`Params::trim`].
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
//!   the needle crosses from rail to rail, so the threads lie side by side. [`Params::pull`]
//!   widens the column (pull compensation: the fabric draws the stitches together). The underlay
//!   ([`Params::underlay`]) follows the column's width.
//! - [`Style::Fill`] fills a shape with rows of stitches [`Params::density`] apart at
//!   [`Params::angle`] degrees, region by region, over an edge run and, for a large shape, a
//!   tatami underlay ([`Params::underlay`]). [`Params::stitch_length`] is the longest stitch a row
//!   is split into.
//!
//! The parts: [`geometry`] (polylines, spacing points along them, the rows' axes), [`area`] (what
//! is inside a filled shape, where rows cross it, travelling inside it), [`runs`] (runs joined into
//! stitches: jumps, trims, tie-ins and tie-offs), and one module per style ([`running`],
//! [`satin`], [`fill`]).

mod area;
mod fill;
mod geometry;
mod running;
mod runs;
mod satin;
#[cfg(test)]
mod tests;

use vectorcraft_geom::{BezPath, FillRule, Point};

pub use fill::fill;
pub use running::running;
pub use runs::join_cut;
pub use satin::{satin, satin_spacing};

use crate::{Error, Stitch};
use geometry::{Axes, SubPath, length, subpaths};

/// A move longer than this lifts the needle (a jump) instead of being sewn, unless
/// [`Params::jump`] says otherwise (millimetres).
const JUMP: f64 = 5.0;
/// A jump longer than this cuts the thread first, unless [`Params::trim`] says otherwise
/// (millimetres). Shorter jumps (hops between letters or seeds) are left for the machine or a
/// person to cut: machines with jump trimming cut those longer than a length set on the machine
/// (Brother's settings run from 5 to 50 mm), and Ink/Stitch adds no trims unless an object asks
/// for one. Longer ones are cut in the file, so they don't lie loose on a machine that can't cut
/// jumps itself. A choice within those bounds, not a published value.
pub const TRIM: f64 = 10.0;
/// The longest satin stitch before it is split (millimetres): longer stitches snag and sag. An
/// estimate (professional software makes it a setting, bounded by the machine format: 12.1 mm
/// per record in DST).
const MAX_SATIN_STITCH: f64 = 10.0;
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
const FILL_SPACING: f64 = 0.38;
/// The longest stitch of a fill row (millimetres). Wilcom's tatami default (same page as
/// [`FILL_SPACING`]).
const FILL_STITCH: f64 = 4.0;
/// The longest stitch of a running line (millimetres): short enough to follow curves and corners
/// closely, long enough that the thread doesn't pile up. An estimate within the 2–3 mm usual for
/// outlines.
const RUNNING_STITCH: f64 = 2.5;
/// Shorter stitches are left out (millimetres): a tiny stitch can break the thread or cut the
/// fabric, and a repeated point is noise. An estimate.
const MIN_STITCH: f64 = 0.4;
/// The direction of a fill's rows, degrees (0: left to right). Diagonal, so that rows don't run
/// along a shape's horizontal and vertical edges, where they would show each step of the edge.
const FILL_ANGLE: f64 = 45.0;
/// Pull compensation on a satin column (millimetres added on each side): the fabric draws the
/// thread in, so a column sewn exactly as wide as drawn comes out narrower. 0.2 mm is a usual
/// starting value for stable woven fabric; stretchy fabrics need more (fabric presets).
const SATIN_PULL: f64 = 0.2;
/// A satin column's width when none is given (millimetres). The export always gives one, the
/// stroke's width; this is for callers of the planner that don't.
const SATIN_WIDTH: f64 = 3.0;
/// The underlay's own stitch length (millimetres): short, to hold the fabric still.
const UNDERLAY_STITCH: f64 = 2.0;
/// The most runs one shape may produce (a guard against absurd parameters).
const MAX_RUNS: usize = 1_000_000;
/// The most stitches one shape may plan: [`try_stitches`] refuses a shape that would need more.
pub const MAX_STITCHES: usize = 4_000_000;

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

    /// All styles, as menus list them.
    pub const ALL: [Self; 3] = [Self::Running, Self::Satin, Self::Fill];
}

/// What one shape is stitched as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub style: Style,
    /// The longest stitch along a running line or a fill row, millimetres.
    pub stitch_length: f64,
    /// The distance between neighbouring threads: rail to rail for satin, row to row for fill,
    /// millimetres.
    pub density: f64,
    /// The direction the fill's rows run, degrees (0: left to right).
    pub angle: f64,
    /// Pull compensation: how much wider the stitching is made so the fabric's pull does not
    /// leave a gap of background at an edge, millimetres (satin only for now).
    pub pull: f64,
    /// Sew an underlay first: an edge run under a fill, a centre run under a satin column.
    pub underlay: bool,
    /// A move longer than this lifts the needle (a jump) instead of running the thread across,
    /// millimetres. A jump this long or longer is tied off before and in after (when [`Params::lock`]
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
}

// The ranges [`Params::sane`] clamps each value to (millimetres, lowest and highest). They are
// not settings: they keep the planning finite and the stitches within what a machine sews, and
// anything outside them is a mistake or hostile input.

/// A stitch length: from 0.2 mm (two of the 0.1 mm steps the machine formats count in) to 12 mm,
/// about the longest move one record of the machine formats holds (12.1 mm in DST).
const STITCH_LENGTH_RANGE: (f64, f64) = (0.2, 12.0);
/// The distance between threads: from 0.05 mm (twenty threads a millimetre, far denser than any
/// fabric takes) to 5 mm (rows that no longer cover).
const DENSITY_RANGE: (f64, f64) = (0.05, 5.0);
/// Pull compensation: none to 2 mm, far more than the most stretchy fabric needs.
const PULL_RANGE: (f64, f64) = (0.0, 2.0);
/// The longest move sewn rather than jumped: from 0.5 mm to 20 mm, past which a sewn move would be
/// a loose thread across the design.
const JUMP_RANGE: (f64, f64) = (0.5, 20.0);
/// The longest jump left uncut: from the jump length itself (every jump cut) to a metre (none).
const MAX_TRIM: f64 = 1000.0;
/// The shortest stitch kept: from 0.05 mm to 3 mm.
const MIN_STITCH_RANGE: (f64, f64) = (0.05, 3.0);
/// The longest satin stitch before it is split: from 2 mm to the 12 mm of [`STITCH_LENGTH_RANGE`].
const MAX_SATIN_STITCH_RANGE: (f64, f64) = (2.0, 12.0);

impl Default for Params {
    fn default() -> Self {
        Self::for_style(Style::Running)
    }
}

impl Params {
    /// The defaults of one style.
    pub fn for_style(style: Style) -> Self {
        match style {
            // A running line uses no density, angle, pull, underlay or width.
            Style::Running => Self {
                style,
                stitch_length: RUNNING_STITCH,
                density: FILL_SPACING,
                angle: 0.0,
                pull: 0.0,
                underlay: false,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: 0.0,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: false,
            },
            // A satin column's stitches cross from rail to rail: it uses no stitch length or angle
            // (a satin without a width is filled instead, and then they apply).
            Style::Satin => Self {
                style,
                stitch_length: RUNNING_STITCH,
                density: FILL_SPACING,
                angle: 0.0,
                pull: SATIN_PULL,
                underlay: true,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: SATIN_WIDTH,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: true,
            },
            Style::Fill => Self {
                style,
                stitch_length: FILL_STITCH,
                density: FILL_SPACING,
                angle: FILL_ANGLE,
                pull: 0.0,
                underlay: true,
                jump: JUMP,
                trim: TRIM,
                lock: true,
                min_stitch: MIN_STITCH,
                width: 0.0,
                max_satin: MAX_SATIN_STITCH,
                auto_spacing: false,
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
        let angle = fin(self.angle, d.angle) % 360.0;
        let jump = clamp(self.jump, d.jump, JUMP_RANGE);
        Self {
            style: self.style,
            stitch_length: clamp(self.stitch_length, d.stitch_length, STITCH_LENGTH_RANGE),
            density: clamp(self.density, d.density, DENSITY_RANGE),
            angle: if angle < 0.0 { angle + 360.0 } else { angle },
            pull: clamp(self.pull, d.pull, PULL_RANGE),
            underlay: self.underlay,
            jump,
            trim: clamp(self.trim, d.trim, (jump, MAX_TRIM)),
            lock: self.lock,
            min_stitch: clamp(self.min_stitch, d.min_stitch, MIN_STITCH_RANGE),
            width: clamp(self.width, d.width, (0.0, MAX_SATIN_WIDTH)),
            max_satin: clamp(self.max_satin, d.max_satin, MAX_SATIN_STITCH_RANGE),
            auto_spacing: self.auto_spacing,
        }
    }
}

/// The stitches of `path` in style `p`: one block, in needle order.
///
/// `rule` decides what a fill covers when its subpaths overlap (a hole wound against its shape
/// is a hole under the non-zero rule; under the even-odd rule any two crossings change side).
pub fn stitches(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    stitches_from(path, rule, p, None)
}

/// [`stitches`], the needle at `from` before the shape starts (where the object sewn before it
/// ended): the shape starts as near it as it can, which saves jumps and trims. `None`: the shape
/// starts where it is drawn to.
pub fn stitches_from(path: &BezPath, rule: FillRule, p: &Params, from: Option<Point>) -> Vec<Stitch> {
    let p = p.sane();
    match p.style {
        Style::Running => running::running_from(path, &p, from),
        Style::Satin if p.width > 0.0 => satin::satin_from(path, &p, from),
        // A satin column needs a width: without one the shape is filled instead.
        Style::Satin | Style::Fill => fill::fill_from(path, rule, &p, from),
    }
}

/// [`stitches`], refused with [`Error::TooLarge`] when the shape would need more than
/// [`MAX_STITCHES`] (a huge shape, or a tiny stitch length or spacing): estimated from the
/// geometry before any stitch is planned, so an absurd request costs no memory.
pub fn try_stitches(path: &BezPath, rule: FillRule, p: &Params) -> crate::Result<Vec<Stitch>> {
    try_stitches_from(path, rule, p, None)
}

/// [`try_stitches`], the needle at `from` before the shape starts ([`stitches_from`]).
pub fn try_stitches_from(path: &BezPath, rule: FillRule, p: &Params, from: Option<Point>) -> crate::Result<Vec<Stitch>> {
    let p = p.sane();
    let n = estimate(&subpaths(path), &p);
    if !n.is_finite() || n > MAX_STITCHES as f64 {
        return Err(Error::TooLarge(format!(
            "a shape would need about {n:.0} stitches, more than the {MAX_STITCHES} one shape may have: use a longer stitch, wider spacing or a smaller design"
        )));
    }
    Ok(stitches_from(path, rule, &p, from))
}

/// About how many points `subs` plan into in style `p` (an upper estimate; not finite when the
/// geometry is not).
fn estimate(subs: &[SubPath], p: &Params) -> f64 {
    /// More than the points a subpath adds beyond its stitches: the lock stitch, tie-in or tie-off
    /// at each of its two ends.
    const POINTS_PER_SUBPATH: f64 = 6.0;
    /// More than the points each fill row adds beyond its stitches: its two ends.
    const POINTS_PER_ROW: f64 = 2.0;
    let total: f64 = subs.iter().map(|s| length(&s.points(), s.closed)).sum();
    let ends = POINTS_PER_SUBPATH * subs.len() as f64;
    match p.style {
        Style::Running => total / p.stitch_length + ends,
        Style::Satin if p.width > 0.0 => total / UNDERLAY_STITCH + total / p.density + ends,
        Style::Satin | Style::Fill => {
            let axes = Axes::new(p.angle);
            let extent = |f: &dyn Fn(vectorcraft_geom::Point) -> f64| {
                let (lo, hi) = subs
                    .iter()
                    .flat_map(|s| s.pts.iter())
                    .map(|q| f(*q))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
                if hi >= lo { hi - lo } else { 0.0 }
            };
            // One row more than fit in the extent: a part row at the edge.
            let rows = extent(&|q| axes.across(q)) / p.density + 1.0;
            total / UNDERLAY_STITCH + rows * (extent(&|q| axes.along(q)) / p.stitch_length + POINTS_PER_ROW) + ends
        }
    }
}
