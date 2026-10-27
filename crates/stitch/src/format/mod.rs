//! The machine formats: DST, TAP and T01 (DST's records alone), EXP, PES (its PEC part), PEC on
//! its own, JEF and VP3 — read and written here.
//!
//! Files count coordinates in tenths of a millimetre; this crate counts millimetres. The y axis
//! points up in DST, TAP, T01, EXP and JEF (a design's top is its largest y there) and down in PEC
//! and in VP3's stitches, as Brother and Husqvarna write them: the readers and writers flip y
//! accordingly. Every reader finishes with the design's top-left corner at the origin. Writers put
//! the origin where the format expects the needle to start: a corner of the design in DST, TAP,
//! T01, EXP and PEC, its centre in JEF and VP3 (Janome sews around the hoop's centre, and VP3 files
//! start there). A design read and written again keeps its shape.
//!
//! The formats' own documentation (there is no specification; these were reverse-engineered
//! publicly) is summarised in each module. What they share, and what this module holds:
//!
//! - no colour names in DST, TAP, T01 and EXP: a design read from them gets [`crate::thread::DEFAULT`]'s
//!   colours, one per block; PES and JEF number their threads in a maker's palette, and VP3 gives
//!   each block's colour and name;
//! - no explicit trim in DST, TAP, T01, JEF or VP3: three small jumps that go nowhere in DST (and
//!   in TAP and T01, which are its records), which a
//!   machine reads as "cut", and three jumps of zero length in JEF (one is already a trim when
//!   read); in VP3, a long form reaching 25.5 mm (see `vp3`);
//! - jumps between stitches carry no thread: consecutive jumps collapse to the point the thread
//!   is taken up from.

pub(crate) mod dst;
pub(crate) mod exp;
pub(crate) mod jef;
pub(crate) mod pes;
pub(crate) mod tap;
pub(crate) mod vp3;

use crate::thread;
use crate::{Block, Command, Error, MAX_TENTHS, Pattern, Result, Stitch, Thread, quantize};

/// How many characters of the design's name the DST and PEC headers hold, after `LA:`.
pub(crate) const LABEL_LEN: usize = 16;
/// The byte the DST and PEC headers are padded with: an ASCII space.
pub(crate) const PAD: u8 = b' ';

/// The design's name as a DST or PEC label: printable ASCII and spaces (anything else becomes
/// `_`), at most [`LABEL_LEN`] characters.
pub(crate) fn label(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' }).take(LABEL_LEN).collect()
}

/// A coordinate in the file's tenths of a millimetre, as millimetres.
pub(crate) fn mm(units: impl Into<i64>) -> f64 {
    units.into() as f64 / crate::UNITS_PER_MM
}

/// The most points a reader takes from a file (needle points, jumps, cuts and colour changes): as
/// many as the largest design the export writes (2 million stitches). A file that holds more is
/// refused: a damaged stream or a hostile file would otherwise grow without bound (a DST of
/// alternating stitches and jumps takes 24 bytes of memory for every 3 of the file's, and opens
/// as one object per run).
pub const MAX_POINTS: usize = 2_000_000;

/// `s` added to the points a reader of `what` decoded so far; refused past [`MAX_POINTS`].
pub(crate) fn add(points: &mut Vec<Stitch>, s: Stitch, what: &str) -> Result<()> {
    if points.len() >= MAX_POINTS {
        return Err(Error::TooLarge(format!("the {what} file holds more than {MAX_POINTS} stitches, more than a design may have")));
    }
    points.push(s);
    Ok(())
}

/// A machine format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// Tajima DST: the closest thing to a standard, no colours, 3-byte records.
    Dst,
    /// Happy TAP: DST's records without its header.
    Tap,
    /// Wilcom T01: the same bytes as a TAP, under Wilcom's name.
    T01,
    /// Melco EXP: stitch records only, no header at all.
    Exp,
    /// Brother PES: a PES part (for the design app) and the PEC part machines read.
    Pes,
    /// Brother PEC: the PEC part of a PES on its own, after the magic `#PEC0001`.
    Pec,
    /// Janome JEF: a fixed header and 2-byte records.
    Jef,
    /// Husqvarna Viking and Pfaff VP3: nested packets, a thread record (colour, name, brand) in
    /// each colour block.
    Vp3,
}

/// Where a format's files put the machine's origin, the needle's start: a corner of the design
/// (or, for a design read from a file, where the file started: [`crate::Pattern::start`]), or its
/// centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// A corner of the design (DST, TAP, T01, EXP, PES and PEC, where their readers put the start).
    Corner,
    /// The design's centre (JEF: Janome sews around the hoop's centre; VP3: its files start there).
    Centre,
}

/// What the code knows about one format, in one place ([`Format::spec`]): adding a format is one
/// of these, its module's reader and writer, and the arm that names it in [`Format::spec`], which
/// fails to compile until it is there.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// The `format` parameter value.
    pub id: &'static str,
    /// Its name in menus and dialogs.
    pub label: &'static str,
    /// Its file extensions, without the dot, the one written first.
    pub extensions: &'static [&'static str],
    /// The MIME type its files are offered as.
    pub mime: &'static str,
    /// The thread palette it numbers its threads in, if any ([`Format::palette`]).
    pub palette: Option<thread::Palette>,
    /// The longest move one stitch record holds along x or along y, tenths of a millimetre
    /// ([`Format::max_step`]).
    pub max_step: i32,
    /// The widest and tallest design the format's fields can state, tenths of a millimetre: what
    /// its header holds (DST's five-digit extents, PEC's 16-bit size), else [`MAX_TENTHS`].
    pub max_size: i32,
    /// The most colour blocks it holds, if its layout sets a limit ([`Format::max_colors`]).
    pub max_colors: Option<usize>,
    /// Where its files start the needle.
    pub start: Start,
    /// The bytes its files start with, if they announce themselves ([`Format::sniff`]).
    pub magic: Option<&'static [u8]>,
    /// Its reader and writer.
    pub read: fn(&[u8]) -> Result<Pattern>,
    pub write: fn(&Pattern) -> Result<Vec<u8>>,
    /// What a file of it holds that reading it leaves out ([`Format::losses`]).
    pub losses: fn(&[u8]) -> Vec<Loss>,
}

/// What a machine file holds that reading it leaves out, so that a file written from the design
/// read lacks it ([`Format::losses`]): said once each, when the file has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Loss {
    /// A PES's own section before its PEC part: what PE-Design edits (its objects, and their
    /// stitches as it keeps them). A PES written from the design holds the PEC part alone.
    PesSection,
    /// The preview pictures after a PEC part's stitches: a PES or PEC written from the design has
    /// blank ones.
    Previews,
    /// Thread numbers that name no thread of the format's palette (PEC's 62 to 64, which mark
    /// appliqué; 0, which names no colour; numbers past the palette's end): written as a thread
    /// that is.
    NotThreads,
    /// A JEF header's date and version, and its hoop when it names another than the smallest the
    /// design fits: written anew.
    JefHeader,
    /// A DST header's end position and multi-volume fields (`AX`, `AY`, `MX`, `MY`, `PD`): written
    /// as zeros.
    DstFields,
    /// VP3 threads that twist several colours: read, and so written, in their first.
    TwistedThreads,
    /// A VP3 file's several designs: read, and so written, as one.
    Designs,
}

/// No losses: a format whose files hold nothing a reader leaves out (TAP, T01 and EXP have no
/// header).
fn no_losses(_: &[u8]) -> Vec<Loss> {
    Vec::new()
}

/// Tajima DST: no colours, 3-byte records ([`dst`]).
const DST: Spec = Spec {
    id: "dst",
    label: "Tajima DST",
    extensions: &["dst"],
    mime: "application/x-dst",
    palette: None,
    max_step: dst::MAX_DELTA,
    max_size: dst::MAX_EXTENT,
    max_colors: None,
    start: Start::Corner,
    magic: None,
    read: dst::read,
    write: dst::write,
    losses: dst::losses,
};
/// Happy TAP: DST's records alone ([`tap`]).
const TAP: Spec = Spec {
    id: "tap",
    label: "Happy TAP",
    extensions: &["tap"],
    mime: "application/x-tap",
    read: tap::read,
    write: tap::write,
    losses: no_losses,
    // No header: only the coordinates' range bounds the design.
    max_size: MAX_TENTHS,
    ..DST
};
/// Wilcom T01: the same bytes as a TAP ([`tap`]).
const T01: Spec = Spec { id: "t01", label: "Wilcom T01", extensions: &["t01"], mime: "application/x-t01", read: tap::read_t01, ..TAP };
/// Melco EXP: 2-byte records, no header ([`exp`]).
const EXP: Spec = Spec {
    id: "exp",
    label: "Melco EXP",
    extensions: &["exp"],
    mime: "application/x-exp",
    palette: None,
    max_step: exp::MAX_DELTA,
    max_size: MAX_TENTHS,
    max_colors: None,
    start: Start::Corner,
    magic: None,
    read: exp::read,
    write: exp::write,
    losses: no_losses,
};
/// Brother PES: a PES header and the PEC section ([`pes`]).
const PES: Spec = Spec {
    id: "pes",
    label: "Brother PES",
    extensions: &["pes"],
    mime: "application/x-pes",
    palette: Some(thread::Palette::Pec),
    max_step: pes::MAX_DELTA,
    max_size: pes::MAX_SIZE,
    max_colors: Some(pes::MAX_COLORS),
    start: Start::Corner,
    magic: Some(b"#PES"),
    read: pes::read,
    write: pes::write,
    losses: pes::losses,
};
/// Brother PEC: the PEC section on its own ([`pes`]).
const PEC: Spec =
    Spec { id: "pec", label: "Brother PEC", extensions: &["pec"], mime: "application/x-pec", magic: Some(b"#PEC"), write: pes::write_pec, ..PES };
/// Janome JEF: a header and 2-byte records ([`jef`]).
const JEF: Spec = Spec {
    id: "jef",
    label: "Janome JEF",
    extensions: &["jef"],
    mime: "application/x-jef",
    palette: Some(thread::Palette::Jef),
    max_step: jef::MAX_DELTA,
    max_size: MAX_TENTHS,
    max_colors: None,
    start: Start::Centre,
    magic: None,
    read: jef::read,
    write: jef::write,
    losses: jef::losses,
};
/// Husqvarna Viking and Pfaff VP3: nested packets ([`vp3`]).
const VP3: Spec = Spec {
    id: "vp3",
    label: "Husqvarna Viking / Pfaff VP3",
    extensions: &["vp3"],
    mime: "application/x-vp3",
    palette: None,
    max_step: vp3::MAX_SHORT,
    max_size: MAX_TENTHS,
    max_colors: Some(vp3::MAX_COLORS),
    start: Start::Centre,
    magic: Some(vp3::MAGIC),
    read: vp3::read,
    write: vp3::write,
    losses: vp3::losses,
};

impl Format {
    /// Every format, as menus list them.
    pub const ALL: [Self; 8] = [Self::Dst, Self::Tap, Self::T01, Self::Exp, Self::Pes, Self::Pec, Self::Jef, Self::Vp3];

    /// What the code knows about the format ([`Spec`]).
    pub const fn spec(self) -> &'static Spec {
        match self {
            Self::Dst => &DST,
            Self::Tap => &TAP,
            Self::T01 => &T01,
            Self::Exp => &EXP,
            Self::Pes => &PES,
            Self::Pec => &PEC,
            Self::Jef => &JEF,
            Self::Vp3 => &VP3,
        }
    }

    /// The `format` parameter value.
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// The format's name in menus and dialogs.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// The file extension written (without the dot).
    pub fn extension(self) -> &'static str {
        self.spec().extensions.first().copied().unwrap_or(self.spec().id)
    }

    /// The thread palette the format numbers its threads in: Brother's for PES (its PEC part) and
    /// PEC, Janome's for JEF; `None` for DST, TAP, T01 and EXP, which name no colours, and for VP3,
    /// which writes each thread's own colour.
    pub fn palette(self) -> Option<thread::Palette> {
        self.spec().palette
    }

    /// The longest move one stitch record holds along x or along y, millimetres: 12.1 in DST, TAP
    /// and T01, 12.7 in EXP, JEF and VP3, 204.7 in PES and PEC (their long form). A writer writes a
    /// longer stitch as several stitches of equal steps, so more needle points than planned, and a
    /// longer jump as several jumps.
    pub fn max_step(self) -> f64 {
        mm(self.spec().max_step)
    }

    /// The most colour blocks the format holds, `None` where its layout sets no limit: the PEC
    /// header (in PES and in PEC) counts them in a byte (255), VP3 in a 16-bit word (65,535); DST,
    /// TAP, T01, EXP and JEF have no such field (DST's header counts colour changes in text a reader
    /// recomputes, TAP and T01 have no header, JEF counts them in a 32-bit word).
    pub fn max_colors(self) -> Option<usize> {
        self.spec().max_colors
    }

    /// Where the format's files start the needle: a corner of the design or its centre.
    pub fn start(self) -> Start {
        self.spec().start
    }

    /// The format `id` names (`dst`, `tap`, `t01`, `exp`, `pes`, `pec`, `jef`, `vp3`).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The format a file named `extension` has, in any case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        let e = extension.trim_start_matches('.').to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.spec().extensions.contains(&e.as_str()))
    }

    /// The format a file's first bytes announce, when they announce one: Brother's and VP3 files
    /// carry a magic string; DST, TAP, T01, EXP and JEF are recognised by their extension.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.spec().magic.is_some_and(|m| bytes.starts_with(m)))
    }

    /// Read a design from `bytes`.
    pub fn read(self, bytes: &[u8]) -> Result<Pattern> {
        (self.spec().read)(bytes)
    }

    /// What `bytes`, a file of this format, holds that [`Format::read`] leaves out, each once: what a
    /// file written from the design read lacks. None for a file that doesn't read.
    pub fn losses(self, bytes: &[u8]) -> Vec<Loss> {
        (self.spec().losses)(bytes)
    }

    /// Write `pattern` in this format: refused with [`Error::TooLarge`] when the design is wider or
    /// taller than the format can state ([`Spec::max_size`]), or its file would be larger than
    /// [`MAX_FILE_BYTES`] (moves split into many records).
    pub fn write(self, pattern: &Pattern) -> Result<Vec<u8>> {
        if pattern.is_empty() {
            return Err(Error::Format("the design has no stitches to write".into()));
        }
        if let Some(b) = pattern.bounds() {
            let most = self.spec().max_size;
            let (w, h) = (b.width() * crate::UNITS_PER_MM, b.height() * crate::UNITS_PER_MM);
            if !(w.is_finite() && h.is_finite()) || w > f64::from(most) || h > f64::from(most) {
                return Err(Error::TooLarge(format!(
                    "the design is {:.0} × {:.0} mm, larger than {} can hold ({} mm a side)",
                    b.width(),
                    b.height(),
                    self.label(),
                    mm(most)
                )));
            }
        }
        (self.spec().write)(pattern)
    }
}

/// The design's points in the file's frame: tenths of a millimetre from the frame's origin (the
/// design's top-left corner, or its centre), y flipped when the format counts y upwards; and the
/// box the design's points fall in, which the frame was made from.
pub(crate) struct Frame {
    left: f64,
    top: f64,
    flip: bool,
    /// The box the design's points fall in (millimetres).
    pub(crate) bounds: vectorcraft_geom::Rect,
}

impl Frame {
    /// A frame whose origin is where the design starts ([`Pattern::start`]) or, when it sets none,
    /// its corner: its top-left corner, or its bottom-left one when `flip` (y counting upwards from
    /// it).
    pub(crate) fn of(p: &Pattern, flip: bool) -> Result<Self> {
        let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
        Ok(match p.start.filter(|s| s.x.is_finite() && s.y.is_finite()) {
            Some(s) => Self::at(s, b, flip),
            None => Self { left: b.x0, top: if flip { b.y1 } else { b.y0 }, flip, bounds: b },
        })
    }

    /// A frame whose origin is the design's centre, for formats that place the design around the
    /// hoop's centre (JEF), y flipped when `flip`. Where a design read from a file started is not
    /// used: these machines put the start at the hoop's centre, and a start at a corner (as DST, TAP,
    /// T01, EXP, PES and PEC files written here have it) would put the design beside it.
    pub(crate) fn centered(p: &Pattern, flip: bool) -> Result<Self> {
        let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
        Ok(Self::centered_on(b, flip))
    }

    /// A frame whose origin is `start`, on the 0.1 mm grid the file counts in, for a design whose
    /// points fall in `b`.
    fn at(start: vectorcraft_geom::Point, b: vectorcraft_geom::Rect, flip: bool) -> Self {
        let snap = |v: f64| f64::from(quantize(v)) / crate::UNITS_PER_MM;
        Self { left: snap(start.x), top: snap(start.y), flip, bounds: b }
    }

    /// A frame whose origin is the centre of `b` (the box the design's points fall in), y flipped
    /// when `flip`.
    pub(crate) fn centered_on(b: vectorcraft_geom::Rect, flip: bool) -> Self {
        // On the 0.1 mm grid the file counts in: a centre between two grid points would round
        // the points on either side of it in opposite directions, moving some by a tenth.
        let c = b.center();
        let snap = |v: f64| f64::from(quantize(v)) / crate::UNITS_PER_MM;
        Self { left: snap(c.x), top: snap(c.y), flip, bounds: b }
    }

    /// The file's x for a design x, in tenths.
    pub(crate) fn x(&self, x: f64) -> i32 {
        quantize(x - self.left)
    }

    /// The file's y for a design y, in tenths (flipped when the format counts y upwards).
    pub(crate) fn y(&self, y: f64) -> i32 {
        if self.flip { quantize(self.top - y) } else { quantize(y - self.top) }
    }

    pub(crate) fn point(&self, s: Stitch) -> (i32, i32) {
        (self.x(s.x), self.y(s.y))
    }

    /// The design's size in tenths of a millimetre, as its points are written: its width, then its
    /// height.
    pub(crate) fn size(&self) -> (i32, i32) {
        let b = self.bounds;
        (self.x(b.x1) - self.x(b.x0), (self.y(b.y1) - self.y(b.y0)).abs())
    }

    /// How far the design reaches from the frame's origin in tenths of a millimetre, each at least
    /// 0: right, left, then up and down when the format counts y upwards (else down and up).
    pub(crate) fn extents(&self) -> (i32, i32, i32, i32) {
        let b = self.bounds;
        let (y0, y1) = (self.y(b.y0), self.y(b.y1));
        (self.x(b.x1).max(0), (-self.x(b.x0)).max(0), y0.max(y1).max(0), (-y0.min(y1)).max(0))
    }
}

/// The largest machine file a writer writes, bytes (64 MiB): far beyond any design a hoop holds
/// (2 million stitches, the most a design may have, take about 6 MB in DST), so only a design
/// whose moves split into a great many records (objects hundreds of metres apart) reaches it, and
/// is refused rather than written into memory without bound.
pub const MAX_FILE_BYTES: usize = 64 << 20;

/// The error a writer gives when the file would be larger than [`MAX_FILE_BYTES`].
pub(crate) fn file_too_large(format: &str) -> Error {
    Error::TooLarge(format!(
        "the {format} file would be larger than {} MiB: the design's moves are too long (objects far apart)",
        MAX_FILE_BYTES >> 20
    ))
}

/// What a record-based writer (DST, TAP and T01, EXP, JEF, PEC) writes for a design's points, in
/// the file's tenths of a millimetre; [`walk`] calls it in needle order.
pub(crate) trait Emitter {
    /// A stitch to (`x`, `y`): the needle goes down there.
    fn stitch(&mut self, x: i32, y: i32);
    /// A move to (`x`, `y`) with the needle up.
    fn jump(&mut self, x: i32, y: i32);
    /// The thread cut where the needle is.
    fn trim(&mut self);
    /// A thread change or a stop inside a block.
    fn color_change(&mut self);
    /// The thread change before the next block (a colour change unless the format counts them).
    fn next_block(&mut self) {
        self.color_change();
    }
    /// Whether what is written so far calls for a thread change before the next block.
    fn wrote(&self) -> bool;
    /// How many bytes are written so far.
    fn size(&self) -> usize;
}

/// The stretches a block's stitches are written as: cut at each stop (or thread change) that has a
/// needle point of the block before it, since the last cut, and one after it. No format has a stop
/// of its own (EduTechWiki gives DST's, EXP's and JEF's stop the bytes of their colour change, and
/// PEC and VP3 document none), so a stop is written as a change to the same thread: each stretch is
/// written as a block of the block's thread. A stop or thread change anywhere else (before the
/// block's first needle point, after its last, right after another) is written as nothing: the
/// thread change before the next block, or the design's end, already stops a machine there.
///
/// Each stretch is a range of `stitches`, holding a needle point, without the stops it was cut at;
/// one written as nothing stays inside a stretch, which writes it as nothing. A block without a
/// needle point is one stretch, as it is (empty: none).
pub(crate) fn stretches(stitches: &[Stitch]) -> Vec<std::ops::Range<usize>> {
    let Some(last) = stitches.iter().rposition(|s| s.command == Command::Stitch) else {
        return std::iter::once(0..stitches.len()).filter(|r| !r.is_empty()).collect();
    };
    let mut out = Vec::new();
    let (mut start, mut sewn) = (0, false);
    for (i, s) in stitches.iter().enumerate() {
        match s.command {
            Command::Stitch => sewn = true,
            Command::Stop | Command::ColorChange if sewn && i < last => {
                out.push(start..i);
                (start, sewn) = (i + 1, false);
            }
            _ => {}
        }
    }
    out.push(start..stitches.len());
    out
}

/// How many stops a block of `stitches` is written with: each a change to the same thread
/// ([`stretches`]), which a machine counts as a colour change.
pub fn stops_written(stitches: &[Stitch]) -> usize {
    stretches(stitches).len().saturating_sub(1)
}

/// The stops (and thread changes) of a block of `stitches` that are written ([`stretches`]), by
/// their index, in order: a stitch plan keeps these and leaves out the rest, so that it lists what
/// the machine gets.
pub fn written_stops(stitches: &[Stitch]) -> Vec<usize> {
    let ranges = stretches(stitches);
    let cut = ranges.len().saturating_sub(1);
    ranges.into_iter().take(cut).map(|r| r.end).collect()
}

/// `p`'s blocks as they are written, each stretch of a block ([`stretches`]) with its block's
/// thread: in sewing order, empty blocks left out.
pub(crate) fn written_blocks(p: &Pattern) -> impl Iterator<Item = (&Thread, &[Stitch])> {
    p.blocks.iter().flat_map(|b| stretches(&b.stitches).into_iter().map(move |r| (&b.thread, b.stitches.get(r).unwrap_or_default())))
}

/// The threads `palette` numbers a design's written blocks with ([`written_blocks`]): each block's
/// number ([`thread::Palette::numbers`], matched block by block), repeated for each stretch it is
/// written as, so that a stop is a change to the same thread and never matched again (as a block of
/// its own, it would take another thread than the block before's).
pub(crate) fn written_numbers(p: &Pattern, palette: thread::Palette) -> Vec<usize> {
    let blocks: Vec<&Block> = p.blocks.iter().filter(|b| !b.stitches.is_empty()).collect();
    let numbers = palette.numbers(blocks.iter().map(|b| &b.thread));
    blocks.iter().zip(numbers).flat_map(|(b, n)| std::iter::repeat_n(n, stretches(&b.stitches).len())).collect()
}

/// Walk `p`'s blocks in sewing order, telling `w` what to write: each block as the stretches it is
/// written as ([`written_blocks`]: a stop is a change to the same thread), a thread change before
/// each that follows one that wrote something ([`Emitter::wrote`]), each one's first stitch reached
/// with the needle up (a jump to it, then the stitch), then its stitches, jumps and trims as they
/// come. Refused with [`file_too_large`] (naming `format`) once more than [`MAX_FILE_BYTES`] are
/// written.
pub(crate) fn walk(p: &Pattern, frame: &Frame, w: &mut impl Emitter, format: &str) -> Result<()> {
    for (i, (_, stitches)) in written_blocks(p).enumerate() {
        if i > 0 && w.wrote() {
            w.next_block();
        }
        // Each block starts with the needle up: travel to its first stitch, so that no thread runs
        // from where the needle was.
        let mut up = true;
        for s in stitches {
            match s.command {
                Command::Stitch => {
                    let (x, y) = frame.point(*s);
                    if std::mem::take(&mut up) {
                        w.jump(x, y);
                    }
                    w.stitch(x, y);
                }
                Command::Jump => {
                    let (x, y) = frame.point(*s);
                    w.jump(x, y);
                    up = false;
                }
                Command::Trim => w.trim(),
                // Written as nothing ([`stretches`]): a stop between needle points ends a stretch.
                Command::ColorChange | Command::Stop => {}
            }
            if w.size() > MAX_FILE_BYTES {
                return Err(file_too_large(format));
            }
        }
    }
    Ok(())
}

/// The prefix of a command record in the two-byte formats (EXP and JEF): `0x80`, which no delta
/// they write uses.
pub(crate) const TWO_BYTE_COMMAND: u8 = 0x80;

/// A move from `at` to `to` (tenths of a millimetre) in a two-byte format (EXP and JEF), and `at`
/// moved there: stitches of at most `max` each way, or, when `command` names one, that command's
/// four-byte records (the prefix, the command, the deltas). A stitch that goes nowhere is written
/// (the needle goes down where it is); a jump that goes nowhere is not.
pub(crate) fn two_byte_move(out: &mut Vec<u8>, at: &mut (i32, i32), to: (i32, i32), command: Option<u8>, max: i32) {
    let (dx, dy) = (to.0 - at.0, to.1 - at.1);
    *at = to;
    if dx == 0 && dy == 0 {
        if command.is_none() {
            out.extend_from_slice(&[0, 0]);
        }
        return;
    }
    for (sx, sy) in steps(dx, dy, max) {
        let (bx, by) = ((sx as i8) as u8, (sy as i8) as u8);
        match command {
            Some(c) => out.extend_from_slice(&[TWO_BYTE_COMMAND, c, bx, by]),
            None => out.extend_from_slice(&[bx, by]),
        }
    }
}

/// A file's bytes, read in order.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// A reader of the bytes up to `end` only, at this one's position: what it reads can't run
    /// past `end` (a packet's declared end).
    pub(crate) fn up_to(&self, end: usize) -> Reader<'a> {
        let data = self.data.get(..end.min(self.data.len())).unwrap_or_default();
        Reader { data, pos: self.pos.min(data.len()) }
    }

    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// All the file's bytes.
    pub(crate) fn data(&self) -> &'a [u8] {
        self.data
    }

    /// The file's length.
    pub(crate) fn data_len(&self) -> usize {
        self.data.len()
    }

    pub(crate) fn seek(&mut self, at: usize) {
        self.pos = at.min(self.data.len());
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub(crate) fn u24(&mut self) -> Option<u32> {
        let b = self.take(3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A big-endian u16 (VP3 counts and string lengths).
    pub(crate) fn u16_be(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }

    /// A big-endian i16 (VP3's long moves).
    pub(crate) fn i16_be(&mut self) -> Option<i16> {
        let b = self.take(2)?;
        Some(i16::from_be_bytes([b[0], b[1]]))
    }

    /// A big-endian u32 (VP3's packet lengths).
    pub(crate) fn u32_be(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A big-endian i32 (VP3's positions).
    pub(crate) fn i32_be(&mut self) -> Option<i32> {
        let b = self.take(4)?;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// The delta steps of at most `max` tenths a move is written as: `n` steps whose sum is exactly
/// (dx, dy), each rounded to whole tenths. Worked out as they are taken (a design's moves are
/// written by the million, most of them one step). Public so that a stitch split before it is
/// written (`embroidery.splitLongStitches {format}`) is cut where the writer would cut it, and the
/// file is the same.
pub fn steps(dx: i32, dy: i32, max: i32) -> impl Iterator<Item = (i32, i32)> {
    let max = i64::from(max.max(1));
    let span = i64::from(dx.unsigned_abs().max(dy.unsigned_abs()));
    let n = i64::from(i32::try_from((span + max - 1) / max).unwrap_or(1).max(1));
    // Where the needle is after `i` of the `n` steps, along one axis of the move `d`.
    let at = move |d: i32, i: i64| (i * i64::from(d) / n) as i32;
    (1..=n).map(move |i| (at(dx, i) - at(dx, i - 1), at(dy, i) - at(dy, i - 1)))
}

/// A design built from what a reader decoded, one block per colour change.
///
/// `threads` are the block threads the format names (PEC's and JEF's palette indices); a format
/// that names none passes none, and the blocks get [`thread::default_thread`].
pub(crate) fn finish(name: String, points: Vec<Stitch>, threads: &[Thread]) -> Pattern {
    finish_from(name, points, threads, vectorcraft_geom::Point::ZERO)
}

/// [`finish`] for a design whose needle starts at `start` (millimetres, where its points are) rather
/// than at the file's origin.
pub(crate) fn finish_from(name: String, points: Vec<Stitch>, threads: &[Thread], start: vectorcraft_geom::Point) -> Pattern {
    let mut points = points;
    collapse_jumps(&mut points);
    let mut pattern = Pattern { name, blocks: Vec::new(), start: Some(start) };
    let mut block = Block::new(threads.first().cloned().unwrap_or_else(|| thread::default_thread(0)));
    let mut index = 0;
    for s in points {
        if s.command == Command::ColorChange {
            if !block.stitches.is_empty() {
                pattern.blocks.push(std::mem::take(&mut block));
                index += 1;
            }
            block = Block::new(threads.get(index).cloned().unwrap_or_else(|| thread::default_thread(index)));
            continue;
        }
        if s.command == Command::Trim && block.stitches.last().is_some_and(|l| l.command == Command::Trim) {
            continue;
        }
        block.stitches.push(s);
    }
    if !block.stitches.is_empty() {
        pattern.blocks.push(block);
    }
    pattern.normalize();
    pattern
}

/// Jumps carry no thread: a run of them is one move to where the thread is taken up again.
fn collapse_jumps(points: &mut Vec<Stitch>) {
    let mut out: Vec<Stitch> = Vec::with_capacity(points.len());
    let mut i = 0;
    while i < points.len() {
        if points.get(i).is_some_and(|s| s.command == Command::Jump) {
            let mut last = None;
            while let Some(s) = points.get(i).filter(|s| s.command == Command::Jump) {
                last = Some(*s);
                i += 1;
            }
            if let Some(last) = last {
                out.push(last);
            }
            continue;
        }
        if let Some(s) = points.get(i) {
            out.push(*s);
        }
        i += 1;
    }
    *points = out;
}

#[cfg(test)]
mod tests_fixtures;
#[cfg(test)]
mod tests_stops;

#[cfg(test)]
mod tests {
    use super::*;

    /// PES and PEC number Brother's threads and JEF Janome's, as their readers name them; DST, TAP,
    /// T01, EXP and VP3 number none.
    #[test]
    fn each_format_numbers_its_palettes_threads() {
        let (pec, jef) = (Some(thread::Palette::Pec), Some(thread::Palette::Jef));
        assert_eq!(Format::ALL.map(Format::palette), [None, None, None, None, pec, pec, jef, None]);
        let mut pattern = Pattern::default();
        let stitches = [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)].map(|(x, y)| Stitch::new(x, y, Command::Stitch)).to_vec();
        pattern.blocks.push(Block { thread: Thread::rgb([0x12, 0x34, 0x56]), stitches });
        for format in [Format::Pes, Format::Pec, Format::Jef] {
            let read = format.read(&format.write(&pattern).unwrap()).unwrap();
            let (Some(palette), [block]) = (format.palette(), read.blocks.as_slice()) else { panic!("{format:?}: one block") };
            let index = palette.nearest([0x12, 0x34, 0x56], &[]).index;
            assert_eq!(Some(&block.thread), palette.thread(index).as_ref(), "{format:?}");
        }
    }

    /// A stitch of exactly the format's longest step is one record; a tenth longer, along x or y,
    /// is written as two stitches; more colours than the format holds are refused.
    #[test]
    fn the_longest_step_is_one_record() {
        for format in Format::ALL {
            let step = format.max_step();
            for (dx, dy, needle) in [(step, 0.0, 2), (step + 0.1, 0.0, 3), (0.0, -(step + 0.1), 3), (step, step, 2)] {
                let mut block = Block::new(Thread::rgb([200, 30, 30]));
                block.stitches = vec![Stitch::new(0.0, 0.0, Command::Stitch), Stitch::new(dx, dy, Command::Stitch)];
                let pattern = Pattern { blocks: vec![block], ..Pattern::default() };
                let read = format.read(&format.write(&pattern).unwrap()).unwrap();
                assert_eq!(read.sewn(), needle, "{format:?}: a stitch of {dx}, {dy}");
            }
        }
        let many = |n: usize| Pattern {
            blocks: (0..n)
                .map(|k| Block { thread: Thread::rgb([(k % 256) as u8, 0, 0]), stitches: vec![Stitch::new((k % 256) as f64, 0.0, Command::Stitch)] })
                .collect(),
            ..Pattern::default()
        };
        for format in Format::ALL {
            let Some(most) = format.max_colors() else { continue };
            assert!(format.write(&many(most)).is_ok(), "{format:?}");
            assert!(format.write(&many(most + 1)).is_err(), "{format:?}");
        }
        assert_eq!(Format::ALL.map(Format::max_colors), [None, None, None, None, Some(255), Some(255), None, Some(65_535)]);
    }

    /// Each format's spec is its own (its id names it back, its extension finds it), every format is
    /// in `ALL` once, and the starts are the ones the writers use: a corner, the centre in JEF and VP3.
    #[test]
    fn every_format_has_a_spec_of_its_own() {
        for f in Format::ALL {
            assert_eq!(Format::from_id(f.id()), Some(f));
            assert_eq!(Format::from_extension(f.extension()), Some(f));
            assert_eq!(Format::ALL.iter().filter(|g| **g == f).count(), 1, "{f:?}");
        }
        let centred: Vec<Format> = Format::ALL.into_iter().filter(|f| f.start() == Start::Centre).collect();
        assert_eq!(centred, [Format::Jef, Format::Vp3]);
        assert_eq!(Format::sniff(b"#PES0001"), Some(Format::Pes));
        assert_eq!(Format::sniff(b"#PEC0001"), Some(Format::Pec));
        assert_eq!(Format::sniff(vp3::MAGIC), Some(Format::Vp3));
        assert_eq!(Format::sniff(b"LA:"), None);
    }

    /// A design whose moves split into a great many records (stitches 150 m apart) is refused by
    /// every format, with an error that says so, rather than written into memory without bound.
    #[test]
    fn a_file_too_large_is_refused() {
        let mut far = Pattern::default();
        let mut b = Block::new(Thread::rgb([200, 30, 30]));
        b.stitches = (0..10_000).map(|k| Stitch::new(if k % 2 == 0 { 0.0 } else { 150_000.0 }, 0.0, Command::Stitch)).collect();
        far.blocks.push(b);
        for format in Format::ALL {
            assert!(matches!(format.write(&far), Err(Error::TooLarge(_))), "{format:?}");
        }
    }

    /// A file of more records than a design may have (a stream of stitch records, or PEC's trims,
    /// each a few bytes of the file) is refused by every reader, quickly, instead of growing without
    /// bound.
    #[test]
    fn readers_refuse_more_points_than_a_design_may_have() {
        let n = MAX_POINTS + 10;
        let refused = |format: Format, bytes: &[u8]| {
            let started = std::time::Instant::now();
            assert!(matches!(format.read(bytes), Err(Error::TooLarge(_))), "{format:?}");
            assert!(started.elapsed().as_secs() < 20, "{format:?}: refused quickly");
        };
        // DST and TAP: a stitch record of +1 x.
        let records = [0x01u8, 0x00, 0x03].repeat(n);
        let mut dst = vec![b' '; 512];
        dst.extend_from_slice(&records);
        refused(Format::Dst, &dst);
        refused(Format::Tap, &records);
        // EXP: a stitch of +1 x.
        refused(Format::Exp, &[0x01u8, 0x00].repeat(n));
        // JEF: a header of no colours, then stitches.
        let mut jef = vec![0u8; 0x74];
        jef[0..4].copy_from_slice(&0x74u32.to_le_bytes());
        jef.extend_from_slice(&[0x01u8, 0x00].repeat(n));
        refused(Format::Jef, &jef);
        // PEC: a header of one colour, then trims (a long value with the trim bit, twice).
        let mut pec = PEC_HEADER_OF_ONE.to_vec();
        pec.extend_from_slice(&[0xA0u8, 0x00, 0xA0, 0x00].repeat(n));
        refused(Format::Pec, &pec);
        // VP3: a design written here, its one block's stitches made millions long.
        let mut design = Pattern::default();
        let mut b = Block::new(Thread::rgb([1, 2, 3]));
        b.stitches = vec![Stitch::new(0.0, 0.0, Command::Stitch), Stitch::new(1.0, 0.0, Command::Stitch)];
        design.blocks.push(b);
        let written = Format::Vp3.write(&design).unwrap();
        let run = written.windows(3).rposition(|w| w == [0x00, 0x01, 0x00]).unwrap();
        let mut vp3 = written[..run + 3 + 4 + 3].to_vec();
        let stitches = [0x01u8, 0x00].repeat(n);
        vp3.extend_from_slice(&stitches);
        vp3[run + 3..run + 7].copy_from_slice(&((stitches.len() + 3) as u32).to_be_bytes());
        // The colour block's length covers its run.
        let block = written.windows(3).rposition(|w| w == [0x00, 0x05, 0x00]).unwrap();
        let block_len = (vp3.len() - (block + 7)) as u32;
        vp3[block + 3..block + 7].copy_from_slice(&block_len.to_be_bytes());
        refused(Format::Vp3, &vp3);
    }

    /// A PEC file's first 520 bytes: the magic, a PEC header naming one colour, and a stitch
    /// block's 16-byte header, so that what follows is read as its stitches.
    const PEC_HEADER_OF_ONE: [u8; 8 + 512 + 16] = {
        let mut b = [b' '; 8 + 512 + 16];
        let magic = *pes::PEC_MAGIC;
        let mut i = 0;
        while i < magic.len() {
            b[i] = magic[i];
            i += 1;
        }
        // One colour (the count less one), thread 1.
        b[8 + 48] = 0;
        b[8 + 49] = 1;
        // The stitch block's header: zeros where the origin words would make it look longer.
        let mut k = 8 + 512;
        while k < 8 + 512 + 16 {
            b[k] = 0;
            k += 1;
        }
        b
    };

    /// A VP3 whose colour blocks claim fewer bytes than their thread records hold: each block is
    /// read within its own length, so the reader never goes back over what it read, and ends at
    /// once, with a design or an error.
    #[test]
    fn a_vp3_block_claiming_less_than_it_holds_ends_quickly() {
        let mut design = Pattern::default();
        let mut b = Block::new(Thread::named([1, 2, 3], &"x".repeat(60_000)));
        b.stitches = vec![Stitch::new(0.0, 0.0, Command::Stitch), Stitch::new(1.0, 0.0, Command::Stitch)];
        design.blocks.extend([b.clone(), b.clone(), b]);
        let mut bytes = Format::Vp3.write(&design).unwrap();
        let blocks: Vec<usize> = bytes.windows(3).enumerate().filter(|(_, w)| *w == [0x00, 0x05, 0x00]).map(|(i, _)| i).collect();
        for at in blocks {
            bytes[at + 3..at + 7].copy_from_slice(&7u32.to_be_bytes());
        }
        let started = std::time::Instant::now();
        let _ = Format::Vp3.read(&bytes);
        assert!(started.elapsed().as_millis() < 1000);
    }

    #[test]
    fn steps_sum_to_the_move() {
        for (dx, dy) in [(0, 0), (5, -3), (121, 121), (-400, 17), (1000, 0), (0, -999)] {
            let s: Vec<(i32, i32)> = steps(dx, dy, 121).collect();
            let sum = s.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            assert_eq!(sum, (dx, dy), "{dx},{dy}");
            for (sx, sy) in s {
                assert!(sx.abs() <= 121 && sy.abs() <= 121, "{sx},{sy}");
            }
        }
    }

    /// A small design with two threads, for the reader tests.
    fn design() -> Pattern {
        let mut p = Pattern { name: "Reader".into(), ..Pattern::default() };
        let mut a = Block::new(crate::Thread::rgb([200, 30, 30]));
        for i in 0..24 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(vectorcraft_geom::Point::new(t * 2.0, (t % 5.0) * 1.5)));
        }
        let mut b = Block::new(crate::Thread::rgb([30, 60, 200]));
        b.stitches.push(Stitch::new(10.0, 20.0, Command::Jump));
        b.stitches.push(Stitch::sew(vectorcraft_geom::Point::new(12.0, 21.0)));
        p.blocks.extend([a, b]);
        p
    }

    /// A design read from a file starts where the file started: the formats that start at a corner
    /// start a design that says where it starts there (a DST's header measures it from there), and
    /// read it back from there, written again unchanged; JEF and VP3 start every design at its
    /// centre. A design that says nothing starts where its format's frame puts the start: its corner
    /// or its centre.
    #[test]
    fn a_design_starts_where_it_says_in_the_formats_that_start_at_a_corner() {
        let (plain, mut started) = (design(), design());
        let b = plain.bounds().unwrap();
        // A tenth of a millimetre off the grid, as a planned design may be.
        let start = vectorcraft_geom::Point::new(b.x0 + 9.03, b.y0 + 4.0);
        started.start = Some(start);
        let near = |a: vectorcraft_geom::Point, b: vectorcraft_geom::Point| (a.x - b.x).abs() < 0.051 && (a.y - b.y).abs() < 0.051;
        for format in Format::ALL {
            let (from_plain, from_start) = (format.write(&plain).unwrap(), format.write(&started).unwrap());
            let (read_plain, read) = (format.read(&from_plain).unwrap(), format.read(&from_start).unwrap());
            assert_eq!(read.bounds(), read_plain.bounds(), "{format:?}: the same design");
            let (r, at) = (read.bounds().unwrap(), read.start.unwrap());
            let at = vectorcraft_geom::Point::new(at.x - r.x0 + b.x0, at.y - r.y0 + b.y0);
            match format.start() {
                Start::Corner => {
                    assert!(near(at, start), "{format:?}: starts at {at:?}, not {start:?}");
                    assert_eq!(
                        format.write(&read).unwrap(),
                        format.write(&format.read(&format.write(&read).unwrap()).unwrap()).unwrap(),
                        "{format:?}"
                    );
                    let corner = read_plain.start.unwrap();
                    assert!(corner.x.abs() < 1e-9 && (corner.y.abs() < 1e-9 || (corner.y - r.height()).abs() < 1e-9), "{format:?}: {corner:?}");
                }
                Start::Centre => {
                    assert_eq!(from_start, from_plain, "{format:?}");
                    assert!(near(at, b.center()), "{format:?}: starts at {at:?}");
                }
            }
        }
        // DST's header: the design's reach right, left, up and down from where it starts.
        let header = String::from_utf8_lossy(&Format::Dst.write(&started).unwrap()[..dst::HEADER]).to_string();
        let field = |name: &str| -> i32 {
            let at = header.find(name).unwrap() + name.len();
            header[at..at + 5].trim().parse().unwrap()
        };
        assert_eq!((field("+X:"), field("-X:")), (quantize(b.x1 - 9.03 - b.x0), 90));
        assert_eq!((field("+Y:"), field("-Y:")), (40, quantize(b.height() - 4.0)));
    }

    /// The files written here hold nothing a reader leaves out; a file that holds what the readers
    /// leave out says so, each once.
    #[test]
    fn a_file_says_what_reading_it_leaves_out() {
        let p = design();
        for format in Format::ALL {
            assert_eq!(format.losses(&format.write(&p).unwrap()), [], "{format:?}");
        }
        assert_eq!(Format::Dst.losses(b"not a DST"), []);
        // DST: an end position, or a design that is one volume of several.
        let dst = Format::Dst.write(&p).unwrap();
        let with = |bytes: &[u8], from: &str, to: &str| {
            let at = bytes.windows(from.len()).position(|w| w == from.as_bytes()).unwrap();
            let mut out = bytes.to_vec();
            out[at..at + to.len()].copy_from_slice(to.as_bytes());
            out
        };
        assert_eq!(Format::Dst.losses(&with(&dst, "AX:+    0", "AX:-  123")), [Loss::DstFields]);
        assert_eq!(Format::Dst.losses(&with(&dst, "PD:******", "PD:000002")), [Loss::DstFields]);
        // PES: a section of its own before the PEC part (the header points past it).
        let pes = Format::Pes.write(&p).unwrap();
        let mut sectioned = pes[..8].to_vec();
        let section = b"CEmbOne and its stitches, as PE-Design keeps them";
        sectioned.extend_from_slice(&u32::try_from(8 + 4 + section.len()).unwrap().to_le_bytes());
        sectioned.extend_from_slice(section);
        sectioned.extend_from_slice(&pes[22..]);
        assert_eq!(Format::Pes.read(&sectioned).unwrap(), Format::Pes.read(&pes).unwrap());
        assert_eq!(Format::Pes.losses(&sectioned), [Loss::PesSection]);
        // PEC: entry 62 (appliqué), and preview pictures drawn.
        let pec = Format::Pec.write(&p).unwrap();
        let mut applique = pec.clone();
        applique[8 + 48 + 1] = 62;
        assert_eq!(Format::Pec.losses(&applique), [Loss::NotThreads]);
        let mut drawn = pec.clone();
        *drawn.last_mut().unwrap() = 0x18;
        assert_eq!(Format::Pec.losses(&drawn), [Loss::Previews]);
        // JEF: a date, another hoop, a number that names no thread.
        let jef = Format::Jef.write(&p).unwrap();
        assert_eq!(Format::Jef.losses(&with(&jef, "00000000000000", "20251228124602")), [Loss::JefHeader]);
        let mut big_hoop = jef.clone();
        big_hoop[32..36].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(Format::Jef.losses(&big_hoop), [Loss::JefHeader]);
        let mut no_thread = jef.clone();
        no_thread[0x74..0x78].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(Format::Jef.losses(&no_thread), [Loss::NotThreads]);
    }

    /// A deterministic pseudo-random stream (no dependency on a random crate).
    fn noise(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    /// Files are untrusted input: every reader must answer a `Result` for random, truncated and
    /// damaged bytes, and never panic.
    #[test]
    fn no_reader_panics_on_hostile_input() {
        let designs: Vec<Vec<u8>> = Format::ALL.iter().map(|f| f.write(&design()).expect("writes")).collect();
        let magics: [&[u8]; 5] = [b"#PES0001", b"#PEC0001", b"LA:", b"        ", vp3::MAGIC];
        for len in [0usize, 1, 2, 3, 8, 12, 22, 100, 511, 512, 513, 528, 532, 1000, 4096] {
            for seed in 0..24u64 {
                let bytes = noise(seed.wrapping_mul(0x9E37_79B9) + len as u64, len);
                for f in Format::ALL {
                    let _ = f.read(&bytes);
                }
                // The same bytes behind each format's magic, which takes the deeper paths.
                for magic in magics {
                    let mut with_magic = magic.to_vec();
                    with_magic.extend_from_slice(&bytes);
                    for f in Format::ALL {
                        let _ = f.read(&with_magic);
                    }
                }
            }
        }
        // Every prefix of a valid file, and single bytes flipped: an error or a design, not a panic.
        for (i, f) in Format::ALL.iter().enumerate() {
            let bytes = designs.get(i).expect("a design per format");
            for n in (0..bytes.len()).step_by(7) {
                let _ = f.read(&bytes[..n]);
            }
            for seed in 0..32u64 {
                let mut damaged = bytes.clone();
                let noise = noise(seed + 7, 24);
                for (k, b) in noise.iter().enumerate() {
                    let at = (usize::from(*b) * 31 + k * 97) % damaged.len();
                    damaged[at] = b.wrapping_mul(37);
                }
                let _ = f.read(&damaged);
            }
        }
    }

    #[test]
    fn jumps_collapse_to_where_the_thread_is_taken_up() {
        let mut v = vec![
            Stitch::new(0.0, 0.0, Command::Stitch),
            Stitch::new(10.0, 0.0, Command::Jump),
            Stitch::new(20.0, 0.0, Command::Jump),
            Stitch::new(20.0, 0.0, Command::Stitch),
        ];
        collapse_jumps(&mut v);
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].command, Command::Jump);
        assert_eq!(v[1].x, 20.0);
    }
}
