//! Husqvarna Viking and Pfaff VP3: nested packets, each a 3-byte tag and a 4-byte length, and a
//! thread record (colour, catalogue number, description, brand) in every colour block.
//!
//! The file is the magic `%vsm%` and a NUL, a producer string, then the file packet (tag
//! `00 02 00`): a summary, then one design (`00 03 00`; the format allows several, which are
//! rarely seen) holding colour blocks (`00 05 00`), each with its thread and one stitch run
//! (`00 01 00`). A colour change is a new colour block: there is no colour-change command. Every
//! length is a big-endian u32 counted from the end of its own field: to the end of the file (the
//! file packet), of the design, of the colour block (its trailing zero byte included) and of the
//! stitches (that zero not included). Strings are a big-endian u16 byte count and the bytes:
//! UTF-16BE for the producer and the notes, UTF-8 for a thread's catalogue number, description
//! and brand.
//!
//! Two units. The packets' positions (the design's extents and centre, where a colour block
//! starts and how far its stitches take the needle) are signed 32-bit micrometres, a tenth of a
//! millimetre times 100, with y counting upwards. The stitches are signed bytes in tenths of a
//! millimetre with y counting downwards (the run's scale bytes say 10 and −10).
//!
//! - The summary: a notes string, the design's right, top, left and bottom edges, an estimate of
//!   the sewing time in stitches, the colour count (u16), a byte `0x0C`, the design count (u16).
//! - A design: its centre, three zero bytes, its left, right, bottom and top edges from the
//!   centre and its width and height, a notes string, `0x64 0x64`, the words 4096, 0, 0 and 4096,
//!   `xxPP`, `01 00`, the producer string again, and its colour block count (u16).
//! - A colour block: where its needle starts (from the design's centre), its thread, how far its
//!   stitches move the needle, its stitch run (the scale bytes `0A F6 00`, then the stitches) and
//!   a zero byte. A block picks up where the one before left off.
//! - A thread: how many colours it twists together (1 for a plain thread), six bytes per colour (a
//!   zero, the RGB colour, two zeros), a zero, a material code (5 is rayon) and the weight (40),
//!   then its catalogue number, description and brand.
//!
//! The stitches. Two signed bytes, x then y, are a stitch (at most 12.7 mm each way). `0x80`
//! starts a command: `80 01`, x and y as big-endian i16, then `80 02`, is the long form, the only
//! move longer than one byte holds. `80 03` is found at the end of the last block; `80 00` and,
//! elsewhere, `80 03` are empty. VP3 has no jump and no trim command of its own. The descriptions
//! call the long form a move, a jump stitch or a jump. EduTechWiki's VP3 page says to take a
//! long-form value of 255 or more (25.5 mm, in tenths) as a trim, that shorter ones are often
//! sewn, and that jumping away and back is a way to force one; its format overview says the long
//! form may mean a trim and a jump, as Pfaff's software seems to write it. Pfaff's creative 3.0
//! cuts jump stitches itself ("Cut Jump Stitches"); whether other machines do isn't documented.
//! None says whether the needle goes down where a long form ends; pyembroidery reads one as a
//! stitch there.
//!
//! What this module does, so that both readings sew the same:
//!
//! - It reads designs and colour blocks by their tags, one after another: the file's and a
//!   design's lengths are written but not relied on (EduTechWiki gives the file packet's length
//!   field two widths), and a colour block's and a stitch run's bound what is read in them. A file
//!   cut short, or damaged after some colour blocks, reads as the blocks before the damage.
//! - It reads long forms one after another as one move with the needle up, after which the
//!   needle goes down where the last lands: a [`Command::Jump`] and a stitch at the same point.
//!   Read this way, the Emoji Embroidery Project's designs made with Ink/Stitch have as many
//!   needle points in each colour block in VP3 as in PES, and each long form is a move their PES
//!   makes with the needle up. A zero-length stitch right after long forms is that same needle
//!   point, read once.
//! - It reads a move as a cut when one of its long forms is longer than the planner's trim
//!   length ([`crate::plan::TRIM`], 10 mm): EduTechWiki's trims (25.5 mm) and the long forms
//!   other writers use for every move too long for a stitch record (the machine cutting jump
//!   stitches) alike, so a design read from VP3 keeps its cuts in the formats that write trims. A
//!   move before a block's first stitch follows a thread change and cuts nothing.
//! - It writes every move with the needle up, and each block's way to its first stitch, as long
//!   forms to the stitch the needle goes down at next, then a zero-length stitch there: the needle
//!   goes down there whichever way a machine reads a long form. A move that keeps the thread on is
//!   long forms of at most 9.8 mm, which neither EduTechWiki's rule nor this reader takes for a
//!   trim. A trim is a long form reaching 25.5 mm along x: the move itself when it is that long,
//!   else out and back, the long form laid across the design's centre when the needle is near it
//!   (so the needle stays within 12.8 mm of the centre or within the design).
//! - A trim with no stitch after it in its block (before a colour change, where the machine
//!   stops for the thread) writes nothing. A stop or a colour change inside a block starts another
//!   colour block in the same thread (the machine stops at each block for its thread). The last
//!   block ends with `80 03`.
//! - A block picks up where the last left off (Weiler); its stated start (the KDE wiki's "move
//!   needle") is written and not read. On the real files the two agree to the rounding of their
//!   moves.
//! - The needle starts at the design's centre, as in the real files, also for a design read from a
//!   file that started elsewhere (see [`super::Frame::centered`]). The design's extent (and so
//!   its centre, and the hoop values) is where the needle goes down: the points of jumps, which
//!   the needle never reaches here, are left out.
//! - The thread record: the block's colour ([`Thread::color`], the art's). The art's colour rather
//!   than the stitch plan's palette thread is the project owner's choice (2026-10-22): an opened
//!   VP3 written again keeps its colours. A thread read from a file keeps its record
//!   ([`Thread::record`]: catalogue number, description, brand, material and weight), written
//!   back as it was. Otherwise: rayon, 40 wt, and as its description the thread's name only when
//!   no palette thread was chosen for it ([`Thread::chosen`] unset): a name a stitch plan gives is
//!   a palette thread's, matched to the colour, not the colour written. Reading takes each block's
//!   first colour and its description (else its catalogue number) as the thread's name.
//!
//! References: EduTechWiki, *Embroidery format VP3* and *Embroidery format*
//! (edutechwiki.unige.ch/en/Embroidery_format_VP3, …/Embroidery_format); KDE Community Wiki,
//! *Viking/Husqvarna Pfaff/Singer 3 VP3* (community.kde.org/Projects/Liberty/File_Formats/
//! Viking_Pfaff); Jason Weiler, *VP3 File Format* (jasonweiler.com/VP3FileFormatInfo.html); The
//! National Archives, PRONOM fmt/2002 (the magic); Pfaff's creative 3.0 brochure ("Cut Jump
//! Stitches"). pyembroidery 1.5.1 (MIT) served only as an outside reader of the files written
//! here; none of its code was read.

use super::Reader;
use crate::{Command, Error, Pattern, Result, Stitch, Thread, UNITS_PER_MM};

/// The file's first bytes: `%vsm%` and a NUL.
pub(crate) const MAGIC: &[u8; 6] = b"%vsm%\0";
/// The producer string the file and its design carry. The maker's software writes its own name
/// there; ours says what made the file.
const PRODUCER: &str = "Produced by VectorCraft";

/// The packets' tags: the file (summary), a design, a colour block, a stitch run.
const FILE_TAG: [u8; 3] = [0x00, 0x02, 0x00];
const DESIGN_TAG: [u8; 3] = [0x00, 0x03, 0x00];
const BLOCK_TAG: [u8; 3] = [0x00, 0x05, 0x00];
const STITCHES_TAG: [u8; 3] = [0x00, 0x01, 0x00];

/// Micrometres in a tenth of a millimetre: the packets count positions in micrometres, the
/// stitches in tenths.
const MICRONS_PER_TENTH: i64 = 100;
/// A packet word's length (a big-endian u32 or i32), and a position's (x and y).
const WORD: usize = 4;
const POSITION: usize = 2 * WORD;
/// The summary's fields after its notes: the four edges, the sewing time, the colour count (u16),
/// [`SUMMARY_MARK`] and the design count (u16).
const SUMMARY: usize = 4 * WORD + WORD + 2 + 1 + 2;
/// A design's edges from its centre and its size: left, right, bottom, top, width and height.
const DESIGN_EDGES: usize = 6;
/// A colour block's count: a big-endian u16.
const BLOCK_COUNT: usize = 2;

/// The summary's byte after the colour count, 12 in every file described.
const SUMMARY_MARK: u8 = 0x0C;
/// The designs a file written here holds.
const DESIGNS: u16 = 1;
/// The design packet's three zero bytes after its centre.
const DESIGN_GAP: [u8; 3] = [0; 3];
/// The design packet's fixed bytes after its notes: `0x64 0x64`, then the words 4096, 0, 0 and
/// 4096 (in every file described; they read like an identity transform in 4096ths), then `xxPP`,
/// 1 and 0.
const DESIGN_MARK: [u8; 2] = [0x64, 0x64];
const DESIGN_WORDS: [i32; 4] = [4096, 0, 0, 4096];
const DESIGN_MAGIC: [u8; 6] = [b'x', b'x', b'P', b'P', 0x01, 0x00];
/// A stitch run's scale bytes: x in tenths (10), y in tenths counted the other way (−10), and a
/// zero.
const SCALE: [u8; 3] = [0x0A, 0xF6, 0x00];
/// The zero byte that ends a colour block, after its stitch run.
const BLOCK_END: u8 = 0x00;

/// A thread record's fixed parts as written here: one colour; the byte before its RGB colour and
/// the two after it (zero for a plain thread); the byte after the colours (zero: not a gradient
/// of colours).
const COLORS_IN_THREAD: u8 = 1;
const BEFORE_RGB: u8 = 0x00;
const AFTER_RGB: [u8; 2] = [0x00, 0x00];
const AFTER_COLORS: u8 = 0x00;
/// A thread record's bytes per colour: a byte, the RGB colour, two bytes.
const COLOR_ENTRY: usize = 6;
/// The material written: 5, rayon, the code every description gives for the common thread.
const RAYON: u8 = 5;
/// The weight written: 40 wt (kilometres per kilogram), standard embroidery thread.
const WEIGHT: u8 = 40;

/// The command prefix: `0x80` is not a delta a record uses.
const CONTROL: u8 = 0x80;
/// The commands' second bytes: an empty command, the long form's start and end, and the command
/// found at the end of the last block (empty elsewhere).
const NOTHING: u8 = 0x00;
const LONG_START: u8 = 0x01;
const LONG_END: u8 = 0x02;
const LAST: u8 = 0x03;
/// The largest delta one stitch byte holds, in tenths of a millimetre (−128 is the command
/// prefix).
pub(crate) const MAX_SHORT: i32 = 127;
/// The largest delta a long form holds, in tenths of a millimetre (a big-endian i16).
const MAX_LONG: i32 = i16::MAX as i32;
/// A long form that reaches this far along an axis is a trim, in tenths of a millimetre (25.5 mm):
/// EduTechWiki's VP3 page says to take a long-form value of 255 or more as a trim, and that
/// shorter ones are often sewn.
const TRIM_REACH: i32 = 255;
/// The longest long form written for a move that keeps the thread on, in tenths of a millimetre
/// (9.8 mm): short of the trim reach, and short of [`crate::plan::TRIM`] (10 mm), past which this
/// reader takes a long form for a cut, with a part of a tenth each way to spare for the rounding
/// of the parts.
const KEPT_REACH: i32 = 98;
/// The most colour blocks a file holds: the summary and the design count them in a big-endian
/// u16, as the KDE Community Wiki and Jason Weiler give the field (EduTechWiki gives the summary's
/// count as a byte, and the design's as 16 bits; a reader of that kind would miscount a design of
/// 256 blocks or more).
pub(crate) const MAX_COLORS: usize = u16::MAX as usize;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let segments = segments(p);
    // The design is where the needle goes down: every jump is written as a move to the next
    // stitch (see the module doc), so a jump's own point is never reached and doesn't count in the
    // extents, the hoop or the centre.
    let needle = segments.iter().flat_map(|(_, s)| s.iter().copied()).filter(|s| s.command == Command::Stitch);
    let Some(b) = crate::bounds_of(needle) else {
        return Err(Error::Format("the design has no stitches to write".into()));
    };
    // Positions from the design's centre, snapped to the 0.1 mm grid; y downwards, as the stitches
    // count it (the packets' positions negate it).
    let frame = super::Frame::centered_on(b, false);
    let (x0, y0, x1, y1) = (frame.x(b.x0), frame.y(b.y0), frame.x(b.x1), frame.y(b.y1));
    let colors = u16::try_from(segments.len()).map_err(|_| Error::TooLarge(format!("VP3 holds at most {MAX_COLORS} colour blocks")))?;
    // Written in one buffer, in file order: each packet's length is filled in when it ends.
    let mut out: Vec<u8> = Vec::with_capacity(p.points().count() * 2 + 1024);
    out.extend_from_slice(MAGIC);
    utf16(&mut out, PRODUCER)?;
    let file = begin(&mut out, FILE_TAG);
    utf16(&mut out, "")?;
    // Right, top, left and bottom (y upwards).
    for v in [x1, -y0, x0, -y1] {
        microns(&mut out, v)?;
    }
    // The sewing-time estimate, filled in once the stitches are written.
    let estimate_at = out.len();
    out.extend_from_slice(&[0; WORD]);
    out.extend_from_slice(&colors.to_be_bytes());
    out.push(SUMMARY_MARK);
    out.extend_from_slice(&DESIGNS.to_be_bytes());
    let design = begin(&mut out, DESIGN_TAG);
    // The design's centre is the frame's origin.
    position(&mut out, (0, 0))?;
    out.extend_from_slice(&DESIGN_GAP);
    // Left, right, bottom and top from the centre (y upwards), then the width and height.
    for v in [x0, x1, -y1, -y0, x1 - x0, y1 - y0] {
        microns(&mut out, v)?;
    }
    utf16(&mut out, "")?;
    out.extend_from_slice(&DESIGN_MARK);
    for w in DESIGN_WORDS {
        out.extend_from_slice(&w.to_be_bytes());
    }
    out.extend_from_slice(&DESIGN_MAGIC);
    utf16(&mut out, PRODUCER)?;
    out.extend_from_slice(&colors.to_be_bytes());
    // The colour blocks: each starts where the last ended, at the design's centre for the first.
    let mut at = (0i32, 0i32);
    let mut needle_points = 0usize;
    for (k, (thread, stitches)) in segments.iter().enumerate() {
        let start = at;
        let mut run = Run::new(start);
        for s in stitches.iter() {
            run.add(*s, &frame);
            if out.len() + run.bytes.len() > super::MAX_FILE_BYTES {
                return Err(super::file_too_large("VP3"));
            }
        }
        if k + 1 == segments.len() {
            run.bytes.extend_from_slice(&[CONTROL, LAST]);
        }
        needle_points += run.needle_points;
        at = (run.x, run.y);
        let block = begin(&mut out, BLOCK_TAG);
        position(&mut out, start)?;
        thread_record(&mut out, thread)?;
        position(&mut out, (at.0 - start.0, at.1 - start.1))?;
        let stitch_run = begin(&mut out, STITCHES_TAG);
        out.extend_from_slice(&SCALE);
        out.extend_from_slice(&run.bytes);
        end(&mut out, stitch_run)?;
        out.push(BLOCK_END);
        end(&mut out, block)?;
    }
    end(&mut out, design)?;
    end(&mut out, file)?;
    let estimate = u32::try_from(needle_points).map_err(|_| Error::TooLarge("too many stitches for a VP3 file".into()))?;
    fill(&mut out, estimate_at, estimate);
    Ok(out)
}

/// The design's colour blocks as VP3 writes them: a block's stitches, cut where it stops or changes
/// colour (each piece a colour block in its thread); empty pieces left out.
fn segments(p: &Pattern) -> Vec<(&Thread, &[Stitch])> {
    let mut out: Vec<(&Thread, &[Stitch])> = Vec::new();
    for block in &p.blocks {
        // The stretches between its colour changes and stops (which aren't written), each a
        // colour block when it sews anything; borrowed, not copied.
        let pieces = block.stitches.split(|s| matches!(s.command, Command::ColorChange | Command::Stop));
        out.extend(pieces.filter(|piece| piece.iter().any(|s| s.command == Command::Stitch)).map(|piece| (&block.thread, piece)));
    }
    out
}

/// One colour block's stitch bytes, written from where the needle is.
struct Run {
    bytes: Vec<u8>,
    /// Where the needle is, in tenths of a millimetre from the design's centre (y down).
    x: i32,
    y: i32,
    /// How the needle reaches the next stitch: up at the block's start.
    needle: Needle,
    /// The stitches and long forms written: the summary's estimate of the sewing time.
    needle_points: usize,
}

/// How a [`Run`]'s next stitch is reached.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Needle {
    /// Sewn: a stitch from the last.
    Down,
    /// With the needle up and the thread kept (after a jump, and at a block's start).
    Up,
    /// With the thread cut first (after a trim).
    Cut,
}

impl Run {
    fn new((x, y): (i32, i32)) -> Self {
        Self { bytes: Vec::new(), x, y, needle: Needle::Up, needle_points: 0 }
    }

    fn add(&mut self, s: Stitch, frame: &super::Frame) {
        match s.command {
            Command::Stitch => {
                let (tx, ty) = frame.point(s);
                match std::mem::replace(&mut self.needle, Needle::Down) {
                    Needle::Down => self.sew(tx, ty),
                    Needle::Up => self.travel(tx, ty, false),
                    Needle::Cut => self.travel(tx, ty, true),
                }
            }
            // The needle lifts: the next stitch is reached with long forms.
            Command::Jump => {
                if self.needle == Needle::Down {
                    self.needle = Needle::Up;
                }
            }
            // A trim: one of those long forms reaches far enough to be taken as one.
            Command::Trim => self.needle = Needle::Cut,
            Command::ColorChange | Command::Stop => {}
        }
    }

    /// A stitch to (tx, ty): records of at most [`MAX_SHORT`] each way, the needle going down at
    /// the end of each (a longer stitch is sewn as several, as the other formats sew it).
    fn sew(&mut self, tx: i32, ty: i32) {
        let (dx, dy) = (tx - self.x, ty - self.y);
        for (sx, sy) in super::steps(dx, dy, MAX_SHORT) {
            self.bytes.extend_from_slice(&[(sx as i8) as u8, (sy as i8) as u8]);
            self.needle_points += 1;
        }
        (self.x, self.y) = (tx, ty);
    }

    /// The needle travels up to (tx, ty) and goes down there: long forms, then a zero-length
    /// stitch (see the module doc). Cut: one long form reaches [`TRIM_REACH`] along an axis (out
    /// and back when the move is shorter). Kept: long forms no longer than [`KEPT_REACH`], which
    /// no reading takes for a trim. Already there and kept: just the stitch.
    fn travel(&mut self, tx: i32, ty: i32, cut: bool) {
        let (dx, dy) = (tx - self.x, ty - self.y);
        if cut && dx.abs().max(dy.abs()) < TRIM_REACH {
            // Out along x to a trim's reach and back. The trimming long form is laid across the
            // design's centre when the needle is near it, so that the needle stays within half a
            // trim's reach of the centre or within the design (a small hoop has room for it).
            let half = TRIM_REACH / 2;
            let from = if self.x.abs() >= half { self.x } else { -half };
            let out = if from > 0 { -TRIM_REACH } else { TRIM_REACH };
            for (lx, ly) in [(from - self.x, 0), (out, 0), (tx - (from + out), dy)] {
                if lx != 0 || ly != 0 {
                    self.long(lx, ly);
                }
            }
        } else if cut {
            for (sx, sy) in super::steps(dx, dy, MAX_LONG) {
                self.long(sx, sy);
            }
        } else if dx != 0 || dy != 0 {
            for (sx, sy) in kept_steps(dx, dy) {
                self.long(sx, sy);
            }
        }
        self.bytes.extend_from_slice(&[0, 0]);
        self.needle_points += 1;
        (self.x, self.y) = (tx, ty);
    }

    /// One long form, (dx, dy) tenths of a millimetre.
    fn long(&mut self, dx: i32, dy: i32) {
        self.bytes.extend_from_slice(&[CONTROL, LONG_START]);
        self.bytes.extend_from_slice(&(dx as i16).to_be_bytes());
        self.bytes.extend_from_slice(&(dy as i16).to_be_bytes());
        self.bytes.extend_from_slice(&[CONTROL, LONG_END]);
        self.needle_points += 1;
    }
}

/// A move with the thread kept, as long forms no longer than [`KEPT_REACH`] each (equal parts).
fn kept_steps(dx: i32, dy: i32) -> impl Iterator<Item = (i32, i32)> {
    let parts = (f64::from(dx).hypot(f64::from(dy)) / f64::from(KEPT_REACH)).ceil().max(1.0);
    let span = f64::from(dx.unsigned_abs().max(dy.unsigned_abs()));
    // The parts' reach along the longer axis; at least 1, so that the parts are finitely many.
    let max = (span / parts).ceil().max(1.0) as i32;
    super::steps(dx, dy, max)
}

/// Start a packet: `tag` and room for its length (filled in by [`end`]), whose position it
/// answers.
fn begin(out: &mut Vec<u8>, tag: [u8; 3]) -> usize {
    out.extend_from_slice(&tag);
    let at = out.len();
    out.extend_from_slice(&[0; WORD]);
    at
}

/// End the packet whose length field is at `at`: its length counts from the end of that field to
/// here.
fn end(out: &mut [u8], at: usize) -> Result<()> {
    let len = out.len().saturating_sub(at + WORD);
    let len = u32::try_from(len).map_err(|_| Error::TooLarge("the design is too large for a VP3 file".into()))?;
    fill(out, at, len);
    Ok(())
}

/// Write `v` (a big-endian u32) into the field at `at`.
fn fill(out: &mut [u8], at: usize, v: u32) {
    if let Some(field) = out.get_mut(at..at + WORD) {
        field.copy_from_slice(&v.to_be_bytes());
    }
}

/// A position in tenths of a millimetre (y down) as the packets write it: x and −y in micrometres.
fn position(out: &mut Vec<u8>, (x, y): (i32, i32)) -> Result<()> {
    microns(out, x)?;
    microns(out, -y)
}

/// `tenths` of a millimetre as a big-endian i32 of micrometres.
fn microns(out: &mut Vec<u8>, tenths: i32) -> Result<()> {
    let v = i32::try_from(i64::from(tenths) * MICRONS_PER_TENTH).map_err(|_| Error::TooLarge("the design is too large for a VP3 file".into()))?;
    out.extend_from_slice(&v.to_be_bytes());
    Ok(())
}

/// A UTF-16BE string with its byte count (a big-endian u16), cut to what the count holds.
fn utf16(out: &mut Vec<u8>, s: &str) -> Result<()> {
    let units: Vec<u16> = s.encode_utf16().take(usize::from(u16::MAX) / 2).collect();
    let len = u16::try_from(units.len() * 2).map_err(|_| Error::Format("a VP3 string is too long".into()))?;
    out.extend_from_slice(&len.to_be_bytes());
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    Ok(())
}

/// A UTF-8 string with its byte count (a big-endian u16), cut at a character to what the count
/// holds.
fn utf8(out: &mut Vec<u8>, s: &str) -> Result<()> {
    let mut end = s.len().min(usize::from(u16::MAX));
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let bytes = s.get(..end).unwrap_or_default().as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| Error::Format("a VP3 string is too long".into()))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// A colour block's thread record: one colour, rayon, 40 wt, no catalogue number or brand, and
/// the description the module doc gives.
fn thread_record(out: &mut Vec<u8>, t: &Thread) -> Result<()> {
    out.push(COLORS_IN_THREAD);
    out.push(BEFORE_RGB);
    out.extend_from_slice(&t.color);
    out.extend_from_slice(&AFTER_RGB);
    out.push(AFTER_COLORS);
    match &t.record {
        // The file's record, as it was.
        Some(r) => {
            out.extend_from_slice(&[r.material, r.weight]);
            for s in [&r.catalogue, &r.description, &r.brand] {
                utf8(out, s)?;
            }
        }
        None => {
            out.extend_from_slice(&[RAYON, WEIGHT]);
            let description = if t.chosen.is_none() { t.name.as_str() } else { "" };
            for s in ["", description, ""] {
                utf8(out, s)?;
            }
        }
    }
    Ok(())
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let d = decode(bytes)?;
    // The needle starts where the first block says (the real files' design centre).
    let mm = |v: i64| v as f64 / UNITS_PER_MM;
    let start = d.start.map_or(vectorcraft_geom::Point::ZERO, |(x, y)| vectorcraft_geom::Point::new(mm(x), mm(y)));
    Ok(super::finish_from(String::new(), d.points, &d.threads, start))
}

/// What a VP3 holds that reading it leaves out: threads that twist several colours (read in their
/// first), and designs past the first (read into one).
pub(crate) fn losses(bytes: &[u8]) -> Vec<super::Loss> {
    let Ok(d) = decode(bytes) else { return Vec::new() };
    let twisted = d.twisted.then_some(super::Loss::TwistedThreads);
    let designs = (d.designs > 1).then_some(super::Loss::Designs);
    twisted.into_iter().chain(designs).collect()
}

/// The file's designs and colour blocks, read in order.
fn decode(bytes: &[u8]) -> Result<Decoded> {
    if !bytes.starts_with(MAGIC) {
        return Err(Error::Format("not a VP3 file: it does not start with %vsm%".into()));
    }
    let mut r = Reader::new(bytes);
    r.seek(MAGIC.len());
    skip_string(&mut r)?;
    // The file packet's length isn't relied on (the descriptions disagree on its field): its
    // designs are found by their tags.
    open(&mut r, FILE_TAG, bytes.len(), "file")?;
    skip_string(&mut r)?;
    // The edges, the sewing time and the counts: the colour blocks are counted as they are read
    // instead.
    r.take(SUMMARY).ok_or_else(|| short(bytes.len()))?;
    let mut d = Decoded::default();
    // A file cut short, or damaged after some colour blocks, keeps the blocks read before the cut.
    let mut stopped: Option<Error> = None;
    while at_tag(bytes, r.pos(), DESIGN_TAG) {
        d.designs += 1;
        let read = open(&mut r, DESIGN_TAG, bytes.len(), "design").and_then(|end| read_design(&mut r, &mut d).map(|()| end));
        match read {
            // Past the design's blocks, or as far as its length says when that is farther (a
            // packet this reader doesn't know between designs).
            Ok(end) => r.seek(end.max(r.pos())),
            // More stitches than a design may have: refused, not cut short.
            Err(e @ Error::TooLarge(_)) => return Err(e),
            Err(e) => {
                stopped = Some(e);
                break;
            }
        }
    }
    if d.points.is_empty() {
        return Err(stopped.unwrap_or_else(|| Error::Format("the VP3 file has no stitches".into())));
    }
    Ok(d)
}

/// What a reader decoded so far: the needle points in order and each colour block's thread.
#[derive(Default)]
struct Decoded {
    points: Vec<Stitch>,
    threads: Vec<Thread>,
    /// Where the needle is, in tenths of a millimetre, y down (wide enough that no file overflows
    /// it: each record moves it by at most a long form's reach).
    x: i64,
    y: i64,
    /// Where the needle starts: where the file's first block says.
    start: Option<(i64, i64)>,
    /// Whether a thread twists several colours, and how many designs were found.
    twisted: bool,
    designs: usize,
}

impl Decoded {
    /// A point where the needle is; refused past [`super::MAX_POINTS`].
    fn push(&mut self, command: Command) -> Result<()> {
        let mm = |v: i64| v as f64 / UNITS_PER_MM;
        super::add(&mut self.points, Stitch::new(mm(self.x), mm(self.y), command), "VP3")
    }
}

/// A design's colour blocks, found by their tags (a design's own length, like the file's, may
/// claim less than its blocks take).
fn read_design(r: &mut Reader<'_>, d: &mut Decoded) -> Result<()> {
    let len = r.data_len();
    let (cx, cy) = (i64::from(r.i32_be().ok_or_else(|| short(len))?), i64::from(r.i32_be().ok_or_else(|| short(len))?));
    // Three zero bytes; the edges from the centre and the size.
    r.take(DESIGN_GAP.len() + DESIGN_EDGES * WORD).ok_or_else(|| short(len))?;
    skip_string(r)?;
    r.take(DESIGN_MARK.len() + DESIGN_WORDS.len() * WORD + DESIGN_MAGIC.len()).ok_or_else(|| short(len))?;
    skip_string(r)?;
    // The colour block count: they are counted as they are read instead.
    r.take(BLOCK_COUNT).ok_or_else(|| short(len))?;
    let mut first = true;
    while at_tag(r.data(), r.pos(), BLOCK_TAG) {
        let block_end = open(r, BLOCK_TAG, len, "colour block")?;
        // The block's fields are read within its declared length, so that a block claiming less
        // than it holds can't send the reader back over what it read (each block would be read
        // again from inside the last).
        let mut b = r.up_to(block_end);
        let (sx, sy) = (i64::from(b.i32_be().ok_or_else(|| short(len))?), i64::from(b.i32_be().ok_or_else(|| short(len))?));
        let (thread, colors) = read_thread(&mut b, d.threads.len())?;
        d.twisted |= colors > 1;
        // How far the block's stitches move the needle: the stitches say so themselves.
        b.take(POSITION).ok_or_else(|| short(len))?;
        let run_end = open(&mut b, STITCHES_TAG, block_end, "stitch run")?;
        b.take(SCALE.len()).ok_or_else(|| short(len))?;
        if first {
            // A design's first block starts where it says (from the design's centre); every other
            // picks up where the last left off.
            d.x = tenths(cx + sx);
            d.y = tenths(-(cy + sy));
            d.start.get_or_insert((d.x, d.y));
            first = false;
        }
        let mark = d.points.len();
        if !d.threads.is_empty() {
            d.push(Command::ColorChange)?;
        }
        d.threads.push(thread);
        read_stitches(&mut b, d, run_end)?;
        // A block that sews nothing is no block: the next one keeps its own thread.
        if !d.points.get(mark..).unwrap_or_default().iter().any(|s| s.command == Command::Stitch) {
            d.points.truncate(mark);
            d.threads.pop();
        }
        // Forward only: past the block's header at least.
        r.seek(block_end.max(b.pos()).max(r.pos()));
    }
    Ok(())
}

/// A thread record: its first colour, and its description (else its catalogue number) as its
/// name.
fn read_thread(r: &mut Reader<'_>, block: usize) -> Result<(Thread, usize)> {
    let len = r.data_len();
    let colors = usize::from(r.u8().ok_or_else(|| short(len))?);
    // Six bytes a colour (a byte, then the RGB colour), the byte after them, the material and the
    // weight.
    let table = r.take(COLOR_ENTRY * colors + 1 + 2).ok_or_else(|| short(len))?;
    let rgb = match table.get(1..4) {
        Some(&[red, green, blue]) if colors > 0 => Some([red, green, blue]),
        _ => None,
    };
    let (material, weight) = match table {
        [.., material, weight] => (*material, *weight),
        _ => (RAYON, WEIGHT),
    };
    let record = crate::ThreadRecord { catalogue: read_utf8(r)?, description: read_utf8(r)?, brand: read_utf8(r)?, material, weight };
    // Whitespace and NULs (a writer that ends its strings with one) are no part of a name.
    let clean = |s: &str| s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string();
    let (catalogue, description) = (clean(&record.catalogue), clean(&record.description));
    let name = if description.is_empty() { catalogue } else { description };
    let thread = match rgb {
        Some(color) => Thread { record: Some(record), ..Thread::named(color, &name) },
        None => crate::thread::default_thread(block),
    };
    Ok((thread, colors))
}

/// The stitch records up to `end`; refused past [`super::MAX_POINTS`] points.
fn read_stitches(r: &mut Reader<'_>, d: &mut Decoded, end: usize) -> Result<()> {
    // Long forms read since the needle last went down: one move, the needle going down where the
    // last of them lands.
    let mut lift: Option<Lift> = None;
    // Whether the block has sewn: a move before its first stitch follows a thread change, with
    // nothing to cut.
    let mut sewn = false;
    while r.pos() + 2 <= end {
        let (Some(b0), Some(b1)) = (r.u8(), r.u8()) else { break };
        if b0 != CONTROL {
            let (dx, dy) = (i64::from(b0 as i8), i64::from(b1 as i8));
            // A zero-length stitch right after long forms is the needle point they reach, read once.
            if let Some(l) = lift.take() {
                land(d, l)?;
                sewn = true;
                if dx == 0 && dy == 0 {
                    continue;
                }
            }
            d.x += dx;
            d.y += dy;
            d.push(Command::Stitch)?;
            sewn = true;
            continue;
        }
        match b1 {
            LONG_START => {
                if r.pos() + 4 > end {
                    break;
                }
                let (Some(dx), Some(dy)) = (r.i16_be(), r.i16_be()) else { break };
                let l = lift.get_or_insert(Lift { from: (d.x, d.y), cut: false });
                l.cut |= sewn && cuts(dx, dy);
                d.x += i64::from(dx);
                d.y += i64::from(dy);
                if r.pos() + 2 <= end && r.data().get(r.pos()..r.pos() + 2) == Some(&[CONTROL, LONG_END][..]) {
                    r.seek(r.pos() + 2);
                }
            }
            // Empty: a long form's end on its own, an empty command, and the end of the last block.
            NOTHING | LONG_END | LAST => {}
            // An unknown command: stop rather than guess where the needle goes next.
            _ => break,
        }
    }
    // The run ends after long forms: the needle goes down where they land.
    match lift {
        Some(l) => land(d, l),
        None => Ok(()),
    }
}

/// Long forms read one after another: where the needle was when they began, and whether the
/// thread is cut.
struct Lift {
    from: (i64, i64),
    cut: bool,
}

/// The needle goes down where long forms took it: a cut where it was (when one of them is), the
/// move, and the needle point.
fn land(d: &mut Decoded, l: Lift) -> Result<()> {
    if l.cut {
        let here = (d.x, d.y);
        (d.x, d.y) = l.from;
        d.push(Command::Trim)?;
        (d.x, d.y) = here;
    }
    d.push(Command::Jump)?;
    d.push(Command::Stitch)
}

/// Whether a long form of (dx, dy) tenths of a millimetre cuts the thread: when it is longer than
/// [`crate::plan::TRIM`], the move past which the stitch planner cuts. That takes in EduTechWiki's
/// rule (a long form reaching 25.5 mm along an axis is a trim) and the long forms other writers
/// use for any move too long for a stitch record, which their machines cut (Pfaff's "Cut Jump
/// Stitches"): a design read from VP3 keeps its cuts in the formats that write trims.
fn cuts(dx: i16, dy: i16) -> bool {
    f64::from(dx).hypot(f64::from(dy)) / UNITS_PER_MM > crate::plan::TRIM
}

/// Whether `tag` is at `at`.
fn at_tag(bytes: &[u8], at: usize, tag: [u8; 3]) -> bool {
    bytes.get(at..at.saturating_add(3)) == Some(&tag[..])
}

/// Read the packet header `tag` and its length at the reader's position: where the packet ends,
/// no farther than `limit`.
fn open(r: &mut Reader<'_>, tag: [u8; 3], limit: usize, what: &str) -> Result<usize> {
    let at = r.pos();
    let found = r.take(3).ok_or_else(|| short(r.data_len()))?;
    if found != tag {
        return Err(Error::Format(format!("the VP3 file has no {what} packet at byte {at}")));
    }
    let len = r.u32_be().ok_or_else(|| short(r.data_len()))?;
    let len = usize::try_from(len).unwrap_or(usize::MAX);
    // A length past what holds it is the file's claim: what it holds is read.
    Ok(r.pos().saturating_add(len).min(limit))
}

/// A string's bytes: its byte count (a big-endian u16), then the bytes.
fn string<'a>(r: &mut Reader<'a>) -> Result<&'a [u8]> {
    let len = r.data_len();
    let n = r.u16_be().ok_or_else(|| short(len))?;
    r.take(usize::from(n)).ok_or_else(|| short(len))
}

/// Skip a string ([`string`]).
fn skip_string(r: &mut Reader<'_>) -> Result<()> {
    string(r).map(|_| ())
}

/// A UTF-8 string ([`string`]; anything not UTF-8 replaced).
fn read_utf8(r: &mut Reader<'_>) -> Result<String> {
    string(r).map(|b| String::from_utf8_lossy(b).into_owned())
}

/// `microns` as tenths of a millimetre, rounded.
fn tenths(microns: i64) -> i64 {
    (microns + MICRONS_PER_TENTH / 2).div_euclid(MICRONS_PER_TENTH)
}

fn short(len: usize) -> Error {
    Error::Format(format!("the VP3 file is cut short: {len} bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::{Palette, PaletteThread};
    use crate::{Block, ThreadRecord, quantize};
    use vectorcraft_geom::Point;

    /// The summary counts what the file holds: as many colour blocks as the design writes, one
    /// design, and a sewing-time estimate of its needle points; the design's own block count too.
    #[test]
    fn the_summary_counts_what_the_file_holds() {
        let mut p = Pattern::default();
        for k in 0..3u8 {
            let mut b = Block::new(Thread::rgb([200, 60 * k, 30]));
            for i in 0..5 {
                b.stitches.push(Stitch::sew(Point::new(f64::from(i) * 2.0, f64::from(k) * 10.0)));
            }
            p.blocks.push(b);
        }
        let bytes = write(&p).unwrap();
        let u16_at = |at: usize| usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
        let u32_at = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        // The magic and the producer string, the file packet's tag and length, its notes string,
        // then the four edges.
        let file = 8 + u16_at(6);
        let edges = file + 9 + u16_at(file + 7);
        let (estimate, colors, mark, designs) = (u32_at(edges + 16), u16_at(edges + 20), bytes[edges + 22], u16_at(edges + 23));
        assert_eq!((colors, mark, designs), (3, SUMMARY_MARK, 1));
        // The estimate counts what the machine sews through: the needle points and the long forms
        // that reach them.
        let long_forms = bytes.windows(2).filter(|w| w == &[CONTROL, LONG_START]).count();
        assert_eq!(estimate, read(&bytes).unwrap().sewn() + long_forms);
        // The design's own count, after the producer string again.
        let producer = PRODUCER.encode_utf16().count() * 2;
        let at = bytes.windows(DESIGN_MAGIC.len()).position(|w| w == DESIGN_MAGIC).unwrap() + DESIGN_MAGIC.len();
        assert_eq!(u16_at(at + 2 + producer), 3);
    }

    fn design() -> Pattern {
        let mut p = Pattern::default();
        let mut a = Block::new(Thread::named([230, 20, 20], "Poppy"));
        for i in 0..8 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(Point::new(t * 3.0, t * 1.5)));
        }
        let mut b = Block::new(Thread::named([20, 40, 210], "Royal"));
        b.stitches.push(Stitch::sew(Point::new(10.0, 20.0)));
        b.stitches.push(Stitch::sew(Point::new(18.0, 23.0)));
        p.blocks.extend([a, b]);
        p
    }

    /// A block's thread as a machine shows it: its colour and name.
    fn shown(b: &Block) -> ([u8; 3], &str) {
        (b.thread.color, b.thread.name.as_str())
    }

    /// Each block's needle points on the 0.1 mm grid, the design's top-left corner at the origin.
    fn needle_points(p: &Pattern) -> Vec<Vec<(i32, i32)>> {
        p.blocks
            .iter()
            .map(|b| b.stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (quantize(s.x), quantize(s.y))).collect())
            .collect()
    }

    #[test]
    fn a_design_round_trips_with_its_threads() {
        let p = design();
        let bytes = write(&p).expect("writes");
        assert!(bytes.starts_with(MAGIC));
        let back = read(&bytes).expect("reads");
        assert_eq!(needle_points(&back), needle_points(&p));
        let threads: Vec<([u8; 3], &str)> = back.blocks.iter().map(shown).collect();
        assert_eq!(threads, [([230, 20, 20], "Poppy"), ([20, 40, 210], "Royal")]);
        // Written again, the same bytes.
        assert_eq!(write(&back).expect("writes"), bytes);
    }

    /// Every length counts from the end of its own field: the file packet's to the end of the file,
    /// the design's too (one design), a colour block's to the end of its zero byte, a stitch run's
    /// to the end of its stitches.
    #[test]
    fn packet_lengths_end_where_the_format_says() {
        let bytes = write(&design()).expect("writes");
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let find = |tag: [u8; 3], from: usize| from + bytes[from..].windows(3).position(|w| w == tag).unwrap();
        let file = find(FILE_TAG, MAGIC.len());
        assert_eq!(file + 7 + word(file + 3), bytes.len());
        let design = find(DESIGN_TAG, file + 7);
        assert_eq!(design + 7 + word(design + 3), bytes.len());
        let block = find(BLOCK_TAG, design + 7);
        let block_end = block + 7 + word(block + 3);
        assert_eq!(&bytes[block_end..block_end + 3], &BLOCK_TAG, "the next block follows");
        // The run follows the block's start, its thread (ten bytes, then the three strings: "",
        // "Poppy", "") and its shift; a search for its tag could stop inside those.
        let run = block + 7 + POSITION + 10 + 2 + (2 + "Poppy".len()) + 2 + POSITION;
        assert_eq!(&bytes[run..run + 3], &STITCHES_TAG);
        let run_end = run + 7 + word(run + 3);
        assert_eq!(run_end + 1, block_end, "the block's zero byte follows its stitches");
        assert_eq!(bytes[run_end], BLOCK_END);
        assert_eq!(&bytes[run + 7..run + 10], &SCALE);
    }

    /// The packets' positions are micrometres with y upwards, the stitches tenths with y downwards:
    /// a block starts where the last ended, and its shift is where its stitches take the needle.
    #[test]
    fn positions_are_micrometres_and_blocks_pick_up_where_the_last_ended() {
        let p = design();
        let bytes = write(&p).expect("writes");
        let int = |at: usize| i32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        let blocks: Vec<usize> = bytes.windows(3).enumerate().filter(|(_, w)| *w == BLOCK_TAG).map(|(i, _)| i).collect();
        assert_eq!(blocks.len(), 2);
        // The first starts at the design's centre.
        assert_eq!((int(blocks[0] + 7), int(blocks[0] + 11)), (0, 0));
        // Its thread record: one colour, rayon, 40 wt, "Poppy" as its description.
        let thread = blocks[0] + 15;
        assert_eq!(&bytes[thread..thread + 10], &[1, 0, 230, 20, 20, 0, 0, 0, RAYON, WEIGHT]);
        let shift = thread + 10 + 2 + 2 + 5 + 2;
        let (sx, sy) = (int(shift), int(shift + 4));
        // The second starts where the first's shift took the needle.
        assert_eq!((int(blocks[1] + 7), int(blocks[1] + 11)), (sx, sy));
        // The design is 21 × 10.5 mm, centred: its first stitch is 10.5 mm left of the centre, and
        // the first block's shift takes the needle from the centre to its last stitch (y counted
        // upwards).
        let frame = super::super::Frame::centered(&p, false).unwrap();
        let (fx, _) = frame.point(p.blocks[0].stitches[0]);
        let (lx, ly) = frame.point(*p.blocks[0].stitches.last().unwrap());
        assert_eq!((sx, sy), (lx * 100, -ly * 100));
        assert_eq!(fx, -105);
        // The summary's extents: right, top, left and bottom in micrometres.
        let file = MAGIC.len() + 2 + 2 * PRODUCER.len();
        let extents = file + 7 + 2;
        let b = p.bounds().unwrap();
        assert_eq!(int(extents) - int(extents + 8), quantize(b.width()) * 100);
        assert_eq!(int(extents + 8), fx * 100);
        assert_eq!(int(extents + 4) - int(extents + 12), quantize(b.height()) * 100);
    }

    /// A jump is a long form to the next needle point and a zero-length stitch there; a trim is a
    /// long form reaching a trim's reach (out and back, for a short move). Read back: the jump, and
    /// the trim where the thread was cut.
    #[test]
    fn a_jump_is_a_long_form_and_a_zero_stitch_and_a_trim_reaches_far() {
        let mut p = design();
        let here = p.blocks[0].stitches[3];
        p.blocks[0].stitches.insert(4, Stitch::new(here.x + 2.0, here.y, Command::Jump));
        p.blocks[0].stitches.insert(5, Stitch::new(here.x + 2.0, here.y, Command::Stitch));
        p.blocks[0].stitches.insert(6, Stitch::new(here.x + 2.0, here.y, Command::Trim));
        p.blocks[0].stitches.insert(7, Stitch::new(here.x + 4.0, here.y, Command::Jump));
        p.blocks[0].stitches.insert(8, Stitch::new(here.x + 4.0, here.y, Command::Stitch));
        let bytes = write(&p).expect("writes");
        // The jump: a long form 2 mm right, then its end and a zero-length stitch.
        assert!(bytes.windows(10).any(|w| w == [CONTROL, LONG_START, 0, 20, 0, 0, CONTROL, LONG_END, 0, 0]), "a long form for the 2 mm jump");
        // The trim: one long form 25.5 mm along x.
        let forms = long_forms(&bytes);
        assert_eq!(forms.iter().filter(|(x, y)| x.abs().max(y.abs()) >= TRIM_REACH).count(), 1, "{forms:?}");
        let block_end = bytes.len() - 1;
        assert_eq!(&bytes[block_end - 2..], &[CONTROL, LAST, BLOCK_END]);
        let back = read(&bytes).expect("reads");
        assert_eq!(needle_points(&back), needle_points(&p));
        let trims: Vec<Stitch> = back.points().filter(|s| s.command == Command::Trim).collect();
        assert_eq!(trims.len(), 1);
        let first = back.blocks[0].stitches.iter().find(|s| s.command == Command::Stitch).unwrap();
        let (tx, ty) = (quantize(trims[0].x - first.x), quantize(trims[0].y - first.y));
        assert_eq!((tx, ty), (quantize(here.x + 2.0), quantize(here.y)), "cut where the thread was");
        let commands: Vec<Command> = back.blocks[0].stitches.iter().map(|s| s.command).collect();
        assert_eq!(commands[..2], [Command::Jump, Command::Stitch], "the block's first stitch, reached with the needle up");
        assert_eq!(commands.iter().filter(|c| **c == Command::Jump).count(), 3);
        // Written again, the same bytes.
        assert_eq!(write(&back).expect("writes"), bytes);
    }

    /// Every long form's (x, y) in written bytes.
    fn long_forms(bytes: &[u8]) -> Vec<(i32, i32)> {
        let i16_at = |at: usize| i32::from(i16::from_be_bytes([bytes[at], bytes[at + 1]]));
        bytes
            .windows(8)
            .enumerate()
            .filter(|(_, w)| w[..2] == [CONTROL, LONG_START] && w[6..8] == [CONTROL, LONG_END])
            .map(|(i, _)| (i16_at(i + 2), i16_at(i + 4)))
            .collect()
    }

    /// A trimmed move of any length, from anywhere, has a long form that reaches a trim's reach,
    /// and keeps the needle within the design or within half that reach of its centre (a small
    /// hoop has room for it); read back, it is one cut and the needle point it reaches.
    #[test]
    fn a_trimmed_move_reaches_a_trims_reach_and_stays_near_the_design() {
        for width in [10.0, 40.0, 100.0] {
            for along in [0.0, 0.3, 0.5, 0.7, 1.0] {
                for by in [0.0, 1.0, -7.5, 12.0, 26.0, -40.0] {
                    let from: f64 = width * along;
                    let to = (from + by).clamp(0.0, width);
                    let mut b = Block::new(Thread::rgb([5, 5, 5]));
                    let steps = (width / 5.0) as u32;
                    b.stitches.extend((0..=steps).map(|i| Stitch::sew(Point::new(f64::from(i) * 5.0, 0.0))));
                    b.stitches.extend([
                        Stitch::new(from, 1.0, Command::Jump),
                        Stitch::sew(Point::new(from, 1.0)),
                        Stitch::sew(Point::new(from, 2.0)),
                    ]);
                    b.stitches.extend([Stitch::new(from, 2.0, Command::Trim), Stitch::new(to, 3.0, Command::Jump), Stitch::sew(Point::new(to, 3.0))]);
                    let p = Pattern { blocks: vec![b], ..Pattern::default() };
                    let bytes = write(&p).expect("writes");
                    let case = format!("{width} mm wide, from {from} to {to}");
                    // The needle through the stitch run, from the design's centre.
                    let run = bytes.windows(10).position(|w| w[..3] == STITCHES_TAG && w[7..] == SCALE).unwrap() + 7 + SCALE.len();
                    let (mut x, mut at, mut reached) = (0i32, run, false);
                    let room = quantize(width / 2.0).max(TRIM_REACH / 2) + 1;
                    while bytes[at..at + 2] != [CONTROL, LAST] {
                        if bytes[at] == CONTROL {
                            let dx = i32::from(i16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
                            let dy = i32::from(i16::from_be_bytes([bytes[at + 4], bytes[at + 5]]));
                            reached |= dx.abs().max(dy.abs()) >= TRIM_REACH;
                            x += dx;
                            at += 8;
                        } else {
                            x += i32::from(bytes[at] as i8);
                            at += 2;
                        }
                        assert!(x.abs() <= room, "{case}: the needle {x} tenths from the centre");
                    }
                    assert!(reached, "{case}: no long form reaches a trim's reach");
                    let back = read(&bytes).expect("reads");
                    assert_eq!(needle_points(&back), needle_points(&p), "{case}");
                    assert_eq!(back.points().filter(|s| s.command == Command::Trim).count(), 1, "{case}");
                }
            }
        }
    }

    /// A move that keeps the thread on is written as long forms none of which is taken for a cut,
    /// adding up to the move.
    #[test]
    fn a_kept_move_is_never_taken_for_a_cut() {
        for dx in (-700..=700).step_by(7) {
            for dy in (-700..=700).step_by(11) {
                let parts: Vec<(i32, i32)> = kept_steps(dx, dy).collect();
                assert_eq!(parts.iter().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)), (dx, dy));
                for (x, y) in parts {
                    let (x, y) = (i16::try_from(x).unwrap(), i16::try_from(y).unwrap());
                    assert!(!cuts(x, y), "{dx},{dy}: a part {x},{y}");
                }
            }
        }
    }

    /// Read: a long form longer than the planner's trim length is a cut where the needle was, long
    /// forms read one after another are one move, and a move before a block's first stitch (after
    /// its thread change) cuts nothing.
    #[test]
    fn long_moves_are_read_as_cuts() {
        let long = |dx: i16, dy: i16| {
            let mut v = vec![CONTROL, LONG_START];
            v.extend_from_slice(&dx.to_be_bytes());
            v.extend_from_slice(&dy.to_be_bytes());
            v.extend_from_slice(&[CONTROL, LONG_END]);
            v
        };
        // A 12 mm move from the block's start, a stitch, a 12 mm long form and a stitch on; then
        // 9 mm, three times over, a 10 mm one, and a zero-length stitch.
        let mut run = long(120, 0);
        run.extend_from_slice(&[0, 0, 10, 0]);
        run.extend(long(0, 120));
        run.extend_from_slice(&[10, 0]);
        for _ in 0..3 {
            run.extend(long(-90, 0));
        }
        run.extend(long(0, 100));
        run.extend_from_slice(&[0, 0]);
        let p = read(&assemble(&[((0, 0), vec![(thread([1, 2, 3], "A"), run)])])).expect("reads");
        let points: Vec<(Command, (i32, i32))> = p.blocks[0].stitches.iter().map(|s| (s.command, (quantize(s.x), quantize(s.y)))).collect();
        // Normalised: the design's left edge is x 0 (the last needle point, 27 mm left of the
        // first long form's end).
        let at = |x: i32, y: i32| (x + 260 - 130, y);
        assert_eq!(
            points,
            [
                (Command::Jump, at(120, 0)),
                (Command::Stitch, at(120, 0)),
                (Command::Stitch, at(130, 0)),
                (Command::Trim, at(130, 0)),
                (Command::Jump, at(130, 120)),
                (Command::Stitch, at(130, 120)),
                (Command::Stitch, at(140, 120)),
                (Command::Jump, at(-130, 220)),
                (Command::Stitch, at(-130, 220)),
            ]
        );
    }

    /// A long form followed by a stitch that moves on (as other writers write it): the needle goes
    /// down where the long form lands, then at the stitch.
    #[test]
    fn a_long_form_puts_the_needle_down_where_it_lands() {
        let mut p = design();
        p.blocks.truncate(1);
        let mut bytes = write(&p).expect("writes");
        // The block's first move: long forms and a zero-length stitch. Make the stitch move on.
        let at = bytes.windows(10).position(|w| w[..2] == [CONTROL, LONG_START] && w[6..10] == [CONTROL, LONG_END, 0, 0]).unwrap();
        bytes[at + 8] = 5;
        let back = read(&bytes).expect("reads");
        let first: Vec<Command> = back.blocks[0].stitches.iter().take(3).map(|s| s.command).collect();
        assert_eq!(first, [Command::Jump, Command::Stitch, Command::Stitch]);
        assert_eq!(back.sewn(), p.sewn() + 1);
    }

    /// A stop or a colour change inside a block becomes another colour block in the same thread.
    #[test]
    fn a_stop_starts_another_block_in_the_same_thread() {
        let mut p = design();
        let here = p.blocks[0].stitches[3];
        p.blocks[0].stitches.insert(4, Stitch::new(here.x, here.y, Command::Stop));
        let back = read(&write(&p).expect("writes")).expect("reads");
        assert_eq!(back.blocks.len(), 3);
        assert_eq!(back.blocks[0].thread, back.blocks[1].thread);
        assert_eq!(back.sewn(), p.sewn());
    }

    /// A thread read from a file keeps its record (catalogue number, description, brand, material
    /// and weight), written back as it was; its name is the description, else the catalogue number.
    #[test]
    fn a_files_thread_record_is_written_back_as_it_was() {
        let record = |catalogue: &str, description: &str, brand: &str, material: u8, weight: u8| {
            let mut t = vec![1, 0x00, 200, 30, 90, 0x00, 0x00, 0x00, material, weight];
            for s in [catalogue, description, brand] {
                utf8(&mut t, s).unwrap();
            }
            t
        };
        let first = record("2296", "Pink", "Robison-Anton", 5, 40);
        let second = record("1801", "", "Madeira Polyneon", 7, 60);
        let bytes = assemble(&[((0, 0), vec![(first.clone(), vec![10, 0, 10, 0]), (second.clone(), vec![0, 10, 0, 10])])]);
        let p = read(&bytes).expect("reads");
        let records: Vec<Option<ThreadRecord>> = p.blocks.iter().map(|b| b.thread.record.clone()).collect();
        let want = |catalogue: &str, description: &str, brand: &str, material, weight| {
            Some(ThreadRecord { catalogue: catalogue.into(), description: description.into(), brand: brand.into(), material, weight })
        };
        assert_eq!(records, [want("2296", "Pink", "Robison-Anton", 5, 40), want("1801", "", "Madeira Polyneon", 7, 60)]);
        let names: Vec<&str> = p.blocks.iter().map(|b| b.thread.name.as_str()).collect();
        assert_eq!(names, ["Pink", "1801"]);
        let again = write(&p).expect("writes");
        assert!(again.windows(first.len()).any(|w| w == first) && again.windows(second.len()).any(|w| w == second));
        let threads = |p: &Pattern| p.blocks.iter().map(|b| b.thread.clone()).collect::<Vec<_>>();
        assert_eq!(threads(&read(&again).expect("reads")), threads(&p));
        // Even with a palette thread chosen for it (an opened design, planned again).
        let mut planned = p.clone();
        planned.blocks[0].thread.chosen = PaletteThread::new(Palette::Jef, 12);
        assert!(write(&planned).expect("writes").windows(first.len()).any(|w| w == first));
    }

    /// A design planned from art carries its palette thread's name, which is not the colour's: the
    /// description is left empty. A design read from a file keeps its own.
    #[test]
    fn a_planned_threads_palette_name_is_not_written() {
        let mut p = design();
        p.blocks[0].thread.chosen = PaletteThread::new(Palette::Jef, 12);
        let back = read(&write(&p).expect("writes")).expect("reads");
        assert_eq!((shown(&back.blocks[0]), shown(&back.blocks[1])), (([230, 20, 20], ""), ([20, 40, 210], "Royal")));
    }

    /// Stitches longer than a byte holds are sewn as several; a long jump that keeps the thread on
    /// as several long forms, read back as one move.
    #[test]
    fn long_stitches_and_jumps_are_split() {
        let mut p = Pattern::default();
        let mut b = Block::new(Thread::rgb([1, 2, 3]));
        b.stitches.push(Stitch::sew(Point::new(0.0, 0.0)));
        b.stitches.push(Stitch::sew(Point::new(30.0, 0.0)));
        b.stitches.push(Stitch::new(4000.0, 5.0, Command::Jump));
        b.stitches.push(Stitch::sew(Point::new(4000.0, 5.0)));
        p.blocks.push(b);
        let back = read(&write(&p).expect("writes")).expect("reads");
        let pts = needle_points(&back);
        let first = &pts[0];
        assert_eq!(first.first(), Some(&(0, 0)));
        assert!(first.contains(&(300, 0)));
        assert_eq!(first.last(), Some(&(40000, 50)));
        // 30 mm in steps of at most 12.7 mm: three stitches.
        assert_eq!(first.iter().filter(|(x, y)| *y == 0 && (1..=300).contains(x)).count(), 3);
    }

    /// Strings longer than their count holds are cut, at a character.
    #[test]
    fn long_names_are_cut_at_a_character() {
        let mut p = design();
        p.blocks[0].thread.name = "é".repeat(40_000);
        let back = read(&write(&p).expect("writes")).expect("reads");
        let name = &back.blocks[0].thread.name;
        assert!(name.len() <= usize::from(u16::MAX) && name.chars().all(|c| c == 'é'), "{}", name.len());
    }

    /// A colour block put together by hand: its thread record's bytes and its stitch bytes.
    type RawBlock = (Vec<u8>, Vec<u8>);
    /// A design put together by hand: its centre (micrometres, y up) and its colour blocks.
    type RawDesign = ((i32, i32), Vec<RawBlock>);

    /// A VP3 put together by hand, its blocks each starting at their design's centre.
    fn assemble(designs: &[RawDesign]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        utf16(&mut out, PRODUCER).unwrap();
        let file = begin(&mut out, FILE_TAG);
        utf16(&mut out, "").unwrap();
        // The edges, the sewing time and the counts: the reader counts for itself.
        out.extend_from_slice(&[0; SUMMARY]);
        for (centre, blocks) in designs {
            let design = begin(&mut out, DESIGN_TAG);
            out.extend_from_slice(&centre.0.to_be_bytes());
            out.extend_from_slice(&centre.1.to_be_bytes());
            out.extend_from_slice(&[0; DESIGN_GAP.len() + DESIGN_EDGES * WORD]);
            utf16(&mut out, "").unwrap();
            out.extend_from_slice(&DESIGN_MARK);
            for w in DESIGN_WORDS {
                out.extend_from_slice(&w.to_be_bytes());
            }
            out.extend_from_slice(&DESIGN_MAGIC);
            utf16(&mut out, PRODUCER).unwrap();
            out.extend_from_slice(&(blocks.len() as u16).to_be_bytes());
            for (thread, stitches) in blocks {
                let block = begin(&mut out, BLOCK_TAG);
                position(&mut out, (0, 0)).unwrap();
                out.extend_from_slice(thread);
                out.extend_from_slice(&[0; POSITION]);
                let run = begin(&mut out, STITCHES_TAG);
                out.extend_from_slice(&SCALE);
                out.extend_from_slice(stitches);
                end(&mut out, run).unwrap();
                out.push(BLOCK_END);
                end(&mut out, block).unwrap();
            }
            end(&mut out, design).unwrap();
        }
        end(&mut out, file).unwrap();
        out
    }

    /// A one-colour thread record, as the writer writes it.
    fn thread(color: [u8; 3], name: &str) -> Vec<u8> {
        let mut t = Vec::new();
        thread_record(&mut t, &Thread::named(color, name)).unwrap();
        t
    }

    /// A thread that twists two colours (the KDE wiki's example: each colour's six bytes, then the
    /// byte after them) reads in its first colour, and the block after it stays in step.
    #[test]
    fn a_thread_of_two_colours_reads_in_its_first() {
        let mut twisted = vec![2, 0x00, 200, 10, 10, 0x10, 0x00, 0x20, 10, 10, 200, 0x10, 0x00, 0x10, RAYON, WEIGHT];
        for s in ["", "Twist", ""] {
            utf8(&mut twisted, s).unwrap();
        }
        let bytes = assemble(&[((0, 0), vec![(twisted, vec![10, 0, 10, 0]), (thread([1, 2, 3], "After"), vec![0, 10, 0, 10])])]);
        let p = read(&bytes).expect("reads");
        let threads: Vec<([u8; 3], &str)> = p.blocks.iter().map(shown).collect();
        assert_eq!(threads, [([200, 10, 10], "Twist"), ([1, 2, 3], "After")]);
        assert_eq!(losses(&bytes), [super::super::Loss::TwistedThreads]);
        assert_eq!(needle_points(&p), [vec![(0, 0), (10, 0)], vec![(10, 10), (10, 20)]]);
    }

    /// A second design is read after the first, its first block starting at its own centre.
    #[test]
    fn a_second_design_starts_at_its_own_centre() {
        let first = ((0, 0), vec![(thread([1, 1, 1], "A"), vec![10, 0, 10, 0])]);
        // 50 mm right of the first design's centre (micrometres).
        let second = ((50_000, 0), vec![(thread([2, 2, 2], "B"), vec![0, 10])]);
        let bytes = assemble(&[first, second]);
        let p = read(&bytes).expect("reads");
        assert_eq!(losses(&bytes), [super::super::Loss::Designs]);
        assert_eq!(p.blocks.len(), 2);
        assert_eq!(needle_points(&p), [vec![(0, 0), (10, 0)], vec![(490, 10)]]);
    }

    /// The file's and a design's lengths aren't relied on: claiming too little hides no block.
    #[test]
    fn packet_lengths_that_claim_too_little_hide_nothing() {
        let p = design();
        let mut bytes = write(&p).expect("writes");
        for tag in [FILE_TAG, DESIGN_TAG] {
            let at = bytes.windows(3).position(|w| w == tag).unwrap();
            bytes[at + 3..at + 7].copy_from_slice(&0u32.to_be_bytes());
        }
        assert_eq!(needle_points(&read(&bytes).expect("reads")), needle_points(&p));
    }

    /// A file cut inside a colour block's header reads as the blocks before it; cut before any,
    /// it is an error.
    #[test]
    fn a_file_cut_inside_a_block_keeps_the_blocks_before() {
        let p = design();
        let bytes = write(&p).expect("writes");
        let second = bytes.windows(3).enumerate().filter(|(_, w)| *w == BLOCK_TAG).nth(1).map(|(i, _)| i).unwrap();
        // Inside the second block's thread record.
        let cut = read(&bytes[..second + 7 + POSITION + 4]).expect("reads the first block");
        assert_eq!(needle_points(&cut), needle_points(&p)[..1]);
        let first = bytes.windows(3).position(|w| w == BLOCK_TAG).unwrap();
        assert!(read(&bytes[..first + 7 + POSITION + 4]).is_err());
    }

    /// The extents, the hoop and the centre are where the needle goes down: a jump's own point,
    /// never reached (the jump is written as a move to the next stitch), is left out.
    #[test]
    fn the_extents_leave_out_jump_points_the_needle_never_reaches() {
        let mut p = design();
        let last = *p.blocks[1].stitches.last().unwrap();
        // A jump 100 mm to the right that goes nowhere after it.
        p.blocks[1].stitches.push(Stitch::new(last.x + 100.0, last.y, Command::Jump));
        let (with, without) = (write(&p).expect("writes"), write(&design()).expect("writes"));
        assert_eq!(with, without);
    }

    /// NUL bytes and spaces round a thread's name are no part of it.
    #[test]
    fn nuls_round_a_name_are_trimmed() {
        let mut t = vec![1, 0x00, 9, 8, 7, 0x00, 0x00, 0x00, RAYON, WEIGHT];
        for s in ["", " Meadow Red\0", ""] {
            utf8(&mut t, s).unwrap();
        }
        let p = read(&assemble(&[((0, 0), vec![(t, vec![10, 0])])])).expect("reads");
        assert_eq!(shown(&p.blocks[0]), ([9, 8, 7], "Meadow Red"));
        // The record keeps the file's string as it was.
        assert_eq!(p.blocks[0].thread.record.as_ref().map(|r| r.description.as_str()), Some(" Meadow Red\0"));
    }

    /// A file that names no colours in a thread reads in a default colour; one cut short or with a
    /// wrong tag is an error, never a panic; a packet whose length claims more than the file holds
    /// reads what it holds.
    #[test]
    fn damaged_files_are_errors_or_designs() {
        let bytes = write(&design()).expect("writes");
        assert!(read(b"%vsm%").is_err());
        assert!(read(b"%vsn%\0").is_err());
        for n in 0..bytes.len() {
            let _ = read(&bytes[..n]);
        }
        let mut wrong = bytes.clone();
        let tag = wrong.windows(3).position(|w| w == DESIGN_TAG).unwrap();
        wrong[tag + 1] = 9;
        assert!(read(&wrong).is_err());
        let mut long = bytes.clone();
        let file = long.windows(3).position(|w| w == FILE_TAG).unwrap();
        long[file + 3..file + 7].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(read(&long).expect("reads").sewn(), design().sewn());
        // The first thread's colour count 0, its record six bytes shorter as it would be, and the
        // packets round it as much shorter: it reads in the default colour of its block.
        let mut no_colour = bytes.clone();
        let block = no_colour.windows(3).position(|w| w == BLOCK_TAG).unwrap();
        no_colour[block + 15] = 0;
        no_colour.drain(block + 16..block + 16 + COLOR_ENTRY);
        for tag in [FILE_TAG, DESIGN_TAG, BLOCK_TAG] {
            let at = no_colour.windows(3).position(|w| w == tag).unwrap() + 3;
            let len = u32::from_be_bytes(no_colour[at..at + 4].try_into().unwrap()) - COLOR_ENTRY as u32;
            no_colour[at..at + 4].copy_from_slice(&len.to_be_bytes());
        }
        let p = read(&no_colour).expect("reads");
        assert_eq!(p.blocks[0].thread.color, crate::thread::default_thread(0).color);
        assert_eq!(p.sewn(), design().sewn(), "the stitches read as before");
        // Its colour count 0 with the colour's bytes left in: the rest of the block misreads; still
        // a design or an error.
        let mut misread = bytes;
        misread[block + 15] = 0;
        let _ = read(&misread);
    }
}
