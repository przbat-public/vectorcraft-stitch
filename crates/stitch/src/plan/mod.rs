//! How a path becomes stitches: running stitch, satin column and tatami fill.
//!
//! A *run* is a stretch the needle sews without lifting: a set of needle points in order. Runs
//! are joined into a block: a short move between two runs stays down (it is just the next stitch),
//! a long one becomes a [`Command::Trim`] and a [`Command::Jump`].
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

use vectorcraft_geom::{BezPath, FillRule};

pub use fill::fill;
pub use running::running;
pub use satin::{satin, satin_spacing};

use crate::{Error, Stitch};
use geometry::{Axes, SubPath, length, subpaths};

/// A trim and a jump are needed when the thread would otherwise run further than this, unless
/// [`Params::jump`] says otherwise (millimetres).
const JUMP: f64 = 5.0;
/// The longest satin stitch before it is split (millimetres): longer stitches snag and sag. An
/// estimate (professional software makes it a setting, bounded by the machine format: 12.1 mm
/// per record in DST).
const MAX_SATIN: f64 = 10.0;
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
    /// A move longer than this trims and jumps instead of running the thread across,
    /// millimetres.
    pub jump: f64,
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

impl Default for Params {
    fn default() -> Self {
        Self::for_style(Style::Running)
    }
}

impl Params {
    /// The defaults of one style.
    pub fn for_style(style: Style) -> Self {
        match style {
            Style::Running => Self {
                style,
                stitch_length: 2.5,
                density: 0.5,
                angle: 0.0,
                pull: 0.0,
                underlay: false,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 0.0,
                max_satin: MAX_SATIN,
                auto_spacing: false,
            },
            Style::Satin => Self {
                style,
                stitch_length: 2.5,
                density: 0.4,
                angle: 0.0,
                pull: 0.2,
                underlay: true,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 3.0,
                max_satin: MAX_SATIN,
                auto_spacing: true,
            },
            Style::Fill => Self {
                style,
                stitch_length: 4.0,
                density: 0.4,
                angle: 45.0,
                pull: 0.0,
                underlay: true,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 0.0,
                max_satin: MAX_SATIN,
                auto_spacing: false,
            },
        }
    }

    /// The same parameters with every value clamped to a range that produces a stitch plan:
    /// not finite falls back to the style's default, and each distance is bounded so that no
    /// loop can run away.
    pub fn sane(self) -> Self {
        let d = Self::for_style(self.style);
        let fin = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        let angle = fin(self.angle, d.angle) % 360.0;
        Self {
            style: self.style,
            stitch_length: fin(self.stitch_length, d.stitch_length).clamp(0.2, 12.0),
            density: fin(self.density, d.density).clamp(0.05, 5.0),
            angle: if angle < 0.0 { angle + 360.0 } else { angle },
            pull: fin(self.pull, d.pull).clamp(0.0, 2.0),
            underlay: self.underlay,
            jump: fin(self.jump, d.jump).clamp(0.5, 20.0),
            lock: self.lock,
            min_stitch: fin(self.min_stitch, d.min_stitch).clamp(0.05, 3.0),
            width: fin(self.width, d.width).clamp(0.0, 50.0),
            max_satin: fin(self.max_satin, d.max_satin).clamp(2.0, 12.0),
            auto_spacing: self.auto_spacing,
        }
    }
}

/// The stitches of `path` in style `p`: one block, in needle order.
///
/// `rule` decides what a fill covers when its subpaths overlap (a hole wound against its shape
/// is a hole under the non-zero rule; under the even-odd rule any two crossings change side).
pub fn stitches(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    match p.style {
        Style::Running => running(path, &p),
        Style::Satin if p.width > 0.0 => satin(path, &p),
        // A satin column needs a width: without one the shape is filled instead.
        Style::Satin | Style::Fill => fill(path, rule, &p),
    }
}

/// [`stitches`], refused with [`Error::TooLarge`] when the shape would need more than
/// [`MAX_STITCHES`] (a huge shape, or a tiny stitch length or spacing): estimated from the
/// geometry before any stitch is planned, so an absurd request costs no memory.
pub fn try_stitches(path: &BezPath, rule: FillRule, p: &Params) -> crate::Result<Vec<Stitch>> {
    let p = p.sane();
    let n = estimate(&subpaths(path), &p);
    if !n.is_finite() || n > MAX_STITCHES as f64 {
        return Err(Error::TooLarge(format!(
            "a shape would need about {n:.0} stitches, more than the {MAX_STITCHES} one shape may have: use a longer stitch, wider spacing or a smaller design"
        )));
    }
    Ok(stitches(path, rule, &p))
}

/// About how many points `subs` plan into in style `p` (an upper estimate; not finite when the
/// geometry is not).
fn estimate(subs: &[SubPath], p: &Params) -> f64 {
    let total: f64 = subs.iter().map(|s| length(&s.points(), s.closed)).sum();
    let ends = 6.0 * subs.len() as f64;
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
            let rows = extent(&|q| axes.across(q)) / p.density + 1.0;
            total / UNDERLAY_STITCH + rows * (extent(&|q| axes.along(q)) / p.stitch_length + 2.0) + ends
        }
    }
}
