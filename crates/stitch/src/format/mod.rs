//! The machine formats: DST, EXP, PES (its PEC part) and JEF — read and written here.
//!
//! Files count coordinates in tenths of a millimetre; this crate counts millimetres. The y axis
//! points up in DST, EXP and JEF (a design's top is its largest y there) and down in PEC, as
//! Brother writes it: the readers and writers flip y accordingly. Every reader finishes with the
//! design's top-left corner at the origin, and every writer places it there, so a design read and
//! written again keeps its shape and its place.
//!
//! The formats' own documentation (there is no specification; these were reverse-engineered
//! publicly) is summarised in each module. What they share, and what this module holds:
//!
//! - no colour names in DST and EXP: a design read from them gets [`crate::thread::DEFAULT`]'s
//!   colours, one per block;
//! - no explicit trim in DST (three small jumps that go nowhere, which a machine reads as "cut")
//!   and in JEF a trim is a jump that goes nowhere;
//! - jumps between stitches carry no thread: consecutive jumps collapse to the point the thread
//!   is taken up from.

pub mod dst;
pub mod exp;
pub mod jef;
pub mod pes;

use crate::thread;
use crate::{Block, Command, Error, Pattern, Result, Stitch, Thread, quantize};

/// A machine format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// Tajima DST: the closest thing to a standard, no colours, 3-byte records.
    Dst,
    /// Melco EXP: stitch records only, no header at all.
    Exp,
    /// Brother PES: a PES part (for the design app) and the PEC part machines read.
    Pes,
    /// Janome JEF: a fixed header and 2-byte records.
    Jef,
}

impl Format {
    /// Every format, as menus list them.
    pub const ALL: [Self; 4] = [Self::Dst, Self::Exp, Self::Pes, Self::Jef];

    /// The `format` parameter value.
    pub fn id(self) -> &'static str {
        match self {
            Self::Dst => "dst",
            Self::Exp => "exp",
            Self::Pes => "pes",
            Self::Jef => "jef",
        }
    }

    /// The format's name in menus and dialogs.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dst => "Tajima DST",
            Self::Exp => "Melco EXP",
            Self::Pes => "Brother PES",
            Self::Jef => "Janome JEF",
        }
    }

    /// The file extension (without the dot).
    pub fn extension(self) -> &'static str {
        self.id()
    }

    /// The format `id` names (`dst`, `exp`, `pes`, `jef`).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The format a file named `extension` has, in any case.
    pub fn from_extension(extension: &str) -> Option<Self> {
        let e = extension.trim_start_matches('.').to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.extension() == e)
    }

    /// The format a file's first bytes announce, when they announce one: only Brother's files
    /// carry a magic string; DST, EXP and JEF are recognised by their extension.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"#PES") || bytes.starts_with(b"#PEC") { Some(Self::Pes) } else { None }
    }

    /// Read a design from `bytes`.
    pub fn read(self, bytes: &[u8]) -> Result<Pattern> {
        match self {
            Self::Dst => dst::read(bytes),
            Self::Exp => exp::read(bytes),
            Self::Pes => pes::read(bytes),
            Self::Jef => jef::read(bytes),
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
            Self::Pes => pes::write(pattern),
            Self::Jef => jef::write(pattern),
        }
    }
}

/// The design's points in the file's frame: tenths of a millimetre, its top-left corner at the
/// origin, y flipped when the format counts y upwards.
pub(crate) struct Frame {
    left: f64,
    top: f64,
    flip: bool,
}

impl Frame {
    pub(crate) fn of(p: &Pattern, flip: bool) -> Result<Self> {
        let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
        Ok(Self { left: b.x0, top: if flip { b.y1 } else { b.y0 }, flip })
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

    /// The design's size in tenths of a millimetre: its width, then its height (the top and the
    /// bottom swap when the format counts y upwards).
    pub(crate) fn size(&self, b: vectorcraft_geom::Rect) -> (i32, i32) {
        let width = self.x(b.x1).max(0);
        let height = if self.flip { self.y(b.y0) } else { self.y(b.y1) };
        (width, height.max(0))
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
        let magics: [&[u8]; 4] = [b"#PES0001", b"#PEC0001", b"LA:", b"        "];
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
