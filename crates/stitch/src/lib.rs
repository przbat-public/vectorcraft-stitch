//! Embroidery for VectorCraft: stitches from vector art, and the machine formats that carry them.
//!
//! A design is a [`Pattern`]: colour blocks ([`Block`]) of [`Stitch`] points in needle order, one
//! thread each. Coordinates are millimetres with y pointing down, like the document's own points
//! (1/72 in; [`pt_to_mm`] and [`mm_to_pt`] convert). The machine formats count in 0.1 mm with y
//! pointing up: every writer flips y as it writes and every reader flips it back, so a file keeps
//! its shape when it is read and written again.
//!
//! - [`plan`]: how a path becomes stitches — running, satin and tatami fill, underlay and trims.
//! - [`format`]: the machine formats (DST, TAP, T01, EXP, PES, PEC, JEF, VP3), read and written.
//! - [`thread`]: the thread palettes the formats name their colours from.
//! - [`color`]: colours in CIE L*a*b* and the CIEDE2000 difference, which matching a colour to a
//!   palette's thread uses.
//!
//! Nothing here panics on bad input: a malformed file gives an [`Error`], and stitch parameters
//! that are missing or out of range fall back to the style's defaults.

#![forbid(unsafe_code)]

pub mod color;
pub mod format;
pub mod plan;
pub mod thread;

use vectorcraft_geom::{Point, Rect};

/// Millimetres in a document point (1/72 in).
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// The units machine formats count in: tenths of a millimetre.
pub const UNITS_PER_MM: f64 = 10.0;
/// Two points closer than this (millimetres) are the same point: far below the 0.1 mm the machine
/// formats count in, so only rounding separates them. It also decides when something has no
/// length, measured as the distance between its two ends: a span along a row, the step between
/// two neighbours.
pub const SAME_POINT: f64 = 1e-9;

/// `v` points as millimetres.
pub fn pt_to_mm(v: f64) -> f64 {
    v * MM_PER_PT
}

/// `v` millimetres as points.
pub fn mm_to_pt(v: f64) -> f64 {
    v / MM_PER_PT
}

/// The farthest a coordinate is written from a file's origin, tenths of a millimetre (±200 m): well
/// within every format's 32-bit fields. [`quantize`] clamps to it; the writers refuse a design that
/// reaches past it, or past what a format's header can state, instead of writing it clamped
/// ([`format::Format::write`]).
pub const MAX_TENTHS: i32 = 2_000_000;

/// `mm` snapped to the 0.1 mm machine formats count in (0 for anything not finite, so a bad
/// coordinate writes as the origin instead of a wrapped integer), within ±[`MAX_TENTHS`].
pub fn quantize(mm: f64) -> i32 {
    if !mm.is_finite() {
        return 0;
    }
    let limit = f64::from(MAX_TENTHS);
    (mm * UNITS_PER_MM).round().clamp(-limit, limit) as i32
}

/// What the machine does at a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// The needle goes down: a stitch from the previous needle point to this one.
    Stitch,
    /// The point is reached with the needle up, without stitching.
    Jump,
    /// The thread is cut here (the needle stays up).
    Trim,
    /// The machine stops for the thread to be changed to the block's thread.
    ColorChange,
    /// The machine stops and waits (for an appliqué piece, say), the needle up.
    Stop,
}

impl Command {
    /// Does the needle go down at this point?
    pub fn needle_down(self) -> bool {
        matches!(self, Self::Stitch)
    }
}

/// One point of a design in needle order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stitch {
    /// Millimetres, y down.
    pub x: f64,
    pub y: f64,
    pub command: Command,
}

impl Stitch {
    pub fn new(x: f64, y: f64, command: Command) -> Self {
        Self { x, y, command }
    }

    /// A stitch at `p` (the needle goes down there).
    pub fn sew(p: Point) -> Self {
        Self::new(p.x, p.y, Command::Stitch)
    }

    /// A jump to `p`.
    pub fn jump(p: Point) -> Self {
        Self::new(p.x, p.y, Command::Jump)
    }

    pub fn point(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// The distance from `self` to `other`, millimetres.
    pub fn distance(self, other: Self) -> f64 {
        self.point().distance(other.point())
    }
}

/// A block's thread: a colour (the thread's own in a design read from a file, the art's in a
/// design planned from art), the name a palette gives the thread, and the palette thread chosen
/// for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Thread {
    /// sRGB: as a palette lists the thread (a design read from a file), or as the art sewn in it
    /// is painted (a design planned from art).
    pub color: [u8; 3],
    /// The palette's name for the thread (`""` when the format names none).
    pub name: String,
    /// The palette thread it is sewn in, when one was chosen for it (the stitch plan's): a format
    /// that numbers its threads from that palette writes that one, and any other matches
    /// [`Thread::color`] to its own ([`thread::Palette::numbers`]). `None`: every format matches
    /// the colour.
    pub chosen: Option<thread::PaletteThread>,
    /// The thread record the file gave beside the colour (VP3's), written back as it was by a
    /// format that has one. `None`: a planned design's thread, or a format that records none.
    pub record: Option<ThreadRecord>,
}

impl Thread {
    pub fn rgb(color: [u8; 3]) -> Self {
        Self { color, name: String::new(), chosen: None, record: None }
    }

    pub fn named(color: [u8; 3], name: &str) -> Self {
        Self { color, name: name.to_string(), chosen: None, record: None }
    }
}

/// A thread as a machine file records it beside its colour (a VP3 colour block's thread record):
/// kept so that a design read from a file is written back with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadRecord {
    /// The maker's catalogue number, as the file has it.
    pub catalogue: String,
    /// The thread's description.
    pub description: String,
    /// The maker.
    pub brand: String,
    /// The material's code (5 is rayon).
    pub material: u8,
    /// The thread's weight (40 wt is standard embroidery thread).
    pub weight: u8,
}

/// One thread's stitches: a colour block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub thread: Thread,
    /// Needle order. The block's first point is stitched, never jumped to.
    pub stitches: Vec<Stitch>,
}

impl Block {
    pub fn new(thread: Thread) -> Self {
        Self { thread, stitches: Vec::new() }
    }

    /// The stitches the needle sews (jumps, trims and stops leave no thread down).
    pub fn sewn(&self) -> usize {
        self.stitches.iter().filter(|s| s.command.needle_down()).count()
    }

    /// The box the block's points fall in (millimetres), `None` when it has none.
    pub fn bounds(&self) -> Option<Rect> {
        bounds_of(self.stitches.iter().copied())
    }
}

/// The box finite points fall in.
fn bounds_of(points: impl Iterator<Item = Stitch>) -> Option<Rect> {
    let mut b: Option<Rect> = None;
    for p in points {
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        let r = Rect::new(p.x, p.y, p.x, p.y);
        b = Some(b.map_or(r, |b| b.union(r)));
    }
    b
}

/// A whole design: its colour blocks in sewing order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pattern {
    /// The design's name as the file's header or title gives it (`""` when it has none).
    pub name: String,
    pub blocks: Vec<Block>,
    /// Where the machine starts the needle (millimetres, in the design's frame): for a design read
    /// from a file, the file's own start point (its origin); `None` for one planned from art. The
    /// formats that start at a corner ([`format::Start::Corner`]) start here when it is set; JEF and
    /// VP3, whose machines put the start at the hoop's centre, always start at the design's centre.
    pub start: Option<Point>,
}

impl Pattern {
    /// The stitches the needle sews in every block.
    pub fn sewn(&self) -> usize {
        self.blocks.iter().map(Block::sewn).sum()
    }

    /// Every point of the design, in reading order.
    pub fn points(&self) -> impl Iterator<Item = Stitch> + '_ {
        self.blocks.iter().flat_map(|b| b.stitches.iter().copied())
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.stitches.is_empty())
    }

    /// The box the design's needle points fall in (millimetres), `None` when it has none.
    pub fn bounds(&self) -> Option<Rect> {
        bounds_of(self.points())
    }

    /// Move every point, and the start, by (`dx`, `dy`) millimetres.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        if !(dx.is_finite() && dy.is_finite()) {
            return;
        }
        for s in self.blocks.iter_mut().flat_map(|b| b.stitches.iter_mut()) {
            s.x += dx;
            s.y += dy;
        }
        if let Some(start) = &mut self.start {
            *start = Point::new(start.x + dx, start.y + dy);
        }
    }

    /// Move the design so its top-left needle point is the origin, as the machine formats record
    /// their designs; answers whether it moved.
    pub fn normalize(&mut self) -> bool {
        let Some(b) = self.bounds() else { return false };
        if b.x0.abs() < SAME_POINT && b.y0.abs() < SAME_POINT {
            return false;
        }
        self.translate(-b.x0, -b.y0);
        true
    }
}

/// What went wrong reading or writing a design.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The file is too short, claims more than it holds, or its bytes are not what the format
    /// requires.
    #[error("{0}")]
    Format(String),
    /// The format can't hold the design: too many stitches, colours or colours the palette lacks.
    #[error("{0}")]
    TooLarge(String),
}

pub type Result<T> = std::result::Result<T, Error>;
