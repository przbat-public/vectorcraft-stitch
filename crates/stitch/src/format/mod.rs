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
//! - no colour names in DST and EXP: a design read from them gets [`crate::thread::DEFAULT`]'s
//!   colours, one per block; PES and JEF number their threads in a maker's palette, and VP3 gives
//!   each block's colour and name;
//! - no explicit trim in DST, JEF or VP3: three small jumps that go nowhere in DST, which a
//!   machine reads as "cut", and three jumps of zero length in JEF (one is already a trim when
//!   read); VP3 machines cut jump stitches themselves;
//! - jumps between stitches carry no thread: consecutive jumps collapse to the point the thread
//!   is taken up from.

pub mod dst;
pub mod exp;
pub mod jef;
pub mod pes;
pub mod tap;
pub mod vp3;

use crate::thread;
use crate::{Block, Command, Error, Pattern, Result, Stitch, Thread, quantize};

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
pub(crate) fn mm(units: i32) -> f64 {
    f64::from(units) / crate::UNITS_PER_MM
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

impl Format {
    /// Every format, as menus list them.
    pub const ALL: [Self; 8] = [Self::Dst, Self::Tap, Self::T01, Self::Exp, Self::Pes, Self::Pec, Self::Jef, Self::Vp3];

    /// The `format` parameter value.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dst => "dst",
            Self::Tap => "tap",
            Self::T01 => "t01",
            Self::Exp => "exp",
            Self::Pes => "pes",
            Self::Pec => "pec",
            Self::Jef => "jef",
            Self::Vp3 => "vp3",
        }
    }

    /// The format's name in menus and dialogs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dst => "Tajima DST",
            Self::Exp => "Melco EXP",
            Self::Tap => "Happy TAP",
            Self::T01 => "Wilcom T01",
            Self::Pes => "Brother PES",
            Self::Pec => "Brother PEC",
            Self::Jef => "Janome JEF",
            Self::Vp3 => "Husqvarna Viking / Pfaff VP3",
        }
    }

    /// The file extension (without the dot).
    pub fn extension(self) -> &'static str {
        self.id()
    }

    /// The thread palette the format numbers its threads in: Brother's for PES (its PEC part) and
    /// PEC, Janome's for JEF; `None` for DST, TAP, T01 and EXP, which name no colours, and for VP3,
    /// which writes each thread's own colour.
    pub fn palette(self) -> Option<thread::Palette> {
        match self {
            Self::Dst | Self::Exp | Self::Tap | Self::T01 | Self::Vp3 => None,
            Self::Pes | Self::Pec => Some(thread::Palette::Pec),
            Self::Jef => Some(thread::Palette::Jef),
        }
    }

    /// The longest move one stitch record holds along x or along y, millimetres: 12.1 in DST, TAP
    /// and T01, 12.7 in EXP, JEF and VP3, 204.7 in PES and PEC (their long form). A writer writes a
    /// longer stitch as several stitches of equal steps, so more needle points than planned, and a
    /// longer jump as several jumps.
    pub fn max_step(self) -> f64 {
        mm(match self {
            Self::Dst | Self::Tap | Self::T01 => dst::MAX_DELTA,
            Self::Exp => exp::MAX_DELTA,
            Self::Pes | Self::Pec => pes::MAX_DELTA,
            Self::Jef => jef::MAX_DELTA,
            Self::Vp3 => vp3::MAX_SHORT,
        })
    }

    /// The most colour blocks the format holds, `None` where its layout sets no limit: the PEC
    /// header (in PES and in PEC) counts them in a byte (255), VP3 in a 16-bit word (65,535); DST,
    /// TAP, T01, EXP and JEF have no such field (DST's header counts colour changes in text a reader
    /// recomputes, TAP and T01 have no header, JEF counts them in a 32-bit word).
    pub fn max_colors(self) -> Option<usize> {
        match self {
            Self::Pes | Self::Pec => Some(pes::MAX_COLORS),
            Self::Vp3 => Some(vp3::MAX_COLORS),
            Self::Dst | Self::Tap | Self::T01 | Self::Exp | Self::Jef => None,
        }
    }

    /// The format `id` names (`dst`, `tap`, `t01`, `exp`, `pes`, `pec`, `jef`, `vp3`).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The format a file named `extension` has, in any case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        let e = extension.trim_start_matches('.').to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.extension() == e)
    }

    /// The format a file's first bytes announce, when they announce one: Brother's and VP3 files
    /// carry a magic string; DST, TAP, T01, EXP and JEF are recognised by their extension.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"#PES") {
            Some(Self::Pes)
        } else if bytes.starts_with(b"#PEC") {
            Some(Self::Pec)
        } else if bytes.starts_with(vp3::MAGIC) {
            Some(Self::Vp3)
        } else {
            None
        }
    }

    /// Read a design from `bytes`.
    pub fn read(self, bytes: &[u8]) -> Result<Pattern> {
        match self {
            Self::Dst => dst::read(bytes),
            Self::Exp => exp::read(bytes),
            Self::Tap => tap::read(bytes),
            Self::T01 => tap::read_t01(bytes),
            Self::Pes | Self::Pec => pes::read(bytes),
            Self::Jef => jef::read(bytes),
            Self::Vp3 => vp3::read(bytes),
        }
    }

    /// Write `pattern` in this format.
    pub fn write(self, pattern: &Pattern) -> Result<Vec<u8>> {
        if pattern.is_empty() {
            return Err(Error::Format("the design has no stitches to write".into()));
        }
        match self {
            Self::Dst => dst::write(pattern),
            Self::Exp => exp::write(pattern),
            Self::Tap => tap::write(pattern),
            // The same bytes as a TAP.
            Self::T01 => tap::write(pattern),
            Self::Pes => pes::write(pattern),
            Self::Pec => pes::write_pec(pattern),
            Self::Jef => jef::write(pattern),
            Self::Vp3 => vp3::write(pattern),
        }
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
    /// A frame whose origin is the design's corner: its top-left corner, or its bottom-left one
    /// when `flip` (y counting upwards from it).
    pub(crate) fn of(p: &Pattern, flip: bool) -> Result<Self> {
        let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
        Ok(Self { left: b.x0, top: if flip { b.y1 } else { b.y0 }, flip, bounds: b })
    }

    /// A frame whose origin is the design's centre, for formats that place the design around the
    /// hoop's centre (JEF), y flipped when `flip`.
    pub(crate) fn centered(p: &Pattern, flip: bool) -> Result<Self> {
        let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
        Ok(Self::centered_on(b, flip))
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

    /// The design's size in tenths of a millimetre, from a frame whose origin is its corner
    /// ([`Frame::of`]): its width, then its height (the top and the bottom swap when the format
    /// counts y upwards).
    pub(crate) fn size(&self) -> (i32, i32) {
        let b = self.bounds;
        let width = self.x(b.x1).max(0);
        let height = if self.flip { self.y(b.y0) } else { self.y(b.y1) };
        (width, height.max(0))
    }
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
}

/// Walk `p`'s blocks in sewing order, telling `w` what to write: a thread change before each block
/// that follows one that wrote something ([`Emitter::wrote`]), each block's first stitch reached
/// with the needle up (a jump to it, then the stitch), then its stitches, jumps, trims and stops as
/// they come. Empty blocks are passed by.
pub(crate) fn walk(p: &Pattern, frame: &Frame, w: &mut impl Emitter) {
    for (i, block) in p.blocks.iter().enumerate() {
        if block.stitches.is_empty() {
            continue;
        }
        if i > 0 && w.wrote() {
            w.next_block();
        }
        // Each block starts with the needle up: travel to its first stitch, so that no thread runs
        // from where the needle was.
        let mut up = true;
        for s in &block.stitches {
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
                Command::ColorChange | Command::Stop => w.color_change(),
            }
        }
    }
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
/// (dx, dy), each rounded to whole tenths.
pub(crate) fn steps(dx: i32, dy: i32, max: i32) -> Vec<(i32, i32)> {
    let max = max.max(1);
    let span = i64::from(dx.abs().max(dy.abs()));
    let n = i32::try_from((span + i64::from(max) - 1) / i64::from(max)).unwrap_or(1).max(1);
    if n <= 1 {
        return vec![(dx, dy)];
    }
    let mut out = Vec::with_capacity(n as usize);
    let (mut px, mut py) = (0i32, 0i32);
    for i in 1..=n {
        let tx = (i64::from(i) * i64::from(dx) / i64::from(n)) as i32;
        let ty = (i64::from(i) * i64::from(dy) / i64::from(n)) as i32;
        out.push((tx - px, ty - py));
        px = tx;
        py = ty;
    }
    out
}

/// A design built from what a reader decoded, one block per colour change.
///
/// `threads` are the block threads the format names (PEC's and JEF's palette indices); a format
/// that names none passes none, and the blocks get [`thread::default_thread`].
pub(crate) fn finish(name: String, points: Vec<Stitch>, threads: &[Thread]) -> Pattern {
    let mut points = points;
    collapse_jumps(&mut points);
    let mut pattern = Pattern { name, blocks: Vec::new() };
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
                let pattern = Pattern { name: String::new(), blocks: vec![block] };
                let read = format.read(&format.write(&pattern).unwrap()).unwrap();
                assert_eq!(read.sewn(), needle, "{format:?}: a stitch of {dx}, {dy}");
            }
        }
        let many = |n: usize| Pattern {
            name: String::new(),
            blocks: (0..n)
                .map(|k| Block { thread: Thread::rgb([(k % 256) as u8, 0, 0]), stitches: vec![Stitch::new((k % 256) as f64, 0.0, Command::Stitch)] })
                .collect(),
        };
        for format in Format::ALL {
            let Some(most) = format.max_colors() else { continue };
            assert!(format.write(&many(most)).is_ok(), "{format:?}");
            assert!(format.write(&many(most + 1)).is_err(), "{format:?}");
        }
        assert_eq!(Format::ALL.map(Format::max_colors), [None, None, None, None, Some(255), Some(255), None, Some(65_535)]);
    }

    #[test]
    fn steps_sum_to_the_move() {
        for (dx, dy) in [(0, 0), (5, -3), (121, 121), (-400, 17), (1000, 0), (0, -999)] {
            let s = steps(dx, dy, 121);
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
