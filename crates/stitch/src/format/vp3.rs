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
//! call the long form a move, a jump stitch or a jump, and say that Pfaff's software writes a trim
//! and a jump as one; Pfaff machines cut jump stitches themselves, pulling the threads to the back
//! (Pfaff's "Cut Jump Stitches" function). None says whether the needle goes down where a long
//! form ends; pyembroidery reads one as a stitch there.
//!
//! What this module does, so that both readings sew the same:
//!
//! - It reads designs and colour blocks by their tags, one after another: the file's and a
//!   design's lengths are written but not relied on (EduTechWiki gives the file packet's length
//!   field two widths), and a colour block's and a stitch run's bound what is read in them. A file
//!   cut short, or damaged after some colour blocks, reads as the blocks before the damage.
//! - It reads a long form as a jump after which the needle goes down where it lands: a
//!   [`Command::Jump`] and a stitch at the same point. Read this way, the Emoji Embroidery
//!   Project's designs made with Ink/Stitch have as many needle points in each colour block in VP3
//!   as in PES. A zero-length stitch right after a long form is that same needle point, read once.
//! - It writes every jump, and each block's way to its first stitch, as a long form to the stitch
//!   the needle goes down at next (with the needle up, the way it travels doesn't matter), then a
//!   zero-length stitch there: the needle goes down there whichever way a machine reads the long
//!   form.
//! - A trim has no record: the jump after it is a long form, which the machine cuts. A stop or a
//!   colour change inside a block starts another colour block in the same thread (the machine
//!   stops at each block for its thread). The last block ends with `80 03`.
//! - The needle starts at the design's centre, as in the real files. The design's extent (and so
//!   its centre, and the hoop values) is where the needle goes down: the points of jumps, which
//!   the needle never reaches here, are left out.
//! - The thread record: the block's colour ([`Thread::color`], the art's), material rayon, 40 wt.
//!   The art's colour rather than the stitch plan's palette thread is the project owner's choice
//!   (2026-10-22): an opened VP3 written again keeps its colours. Its description is the thread's name only for a design read from a file ([`Thread::chosen`]
//!   unset): a name a stitch plan gives is a palette thread's, matched to the colour, not the
//!   colour written. Reading takes each block's first colour and its description (else its
//!   catalogue number) as the thread's name.
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
/// The most colour blocks a file holds: the summary and the design count them in a big-endian
/// u16.
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
        for s in stitches {
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
fn segments(p: &Pattern) -> Vec<(&Thread, Vec<Stitch>)> {
    let mut out: Vec<(&Thread, Vec<Stitch>)> = Vec::new();
    for block in &p.blocks {
        let mut piece: Vec<Stitch> = Vec::new();
        for s in &block.stitches {
            match s.command {
                Command::ColorChange | Command::Stop => {
                    push_piece(&mut out, &block.thread, std::mem::take(&mut piece));
                }
                _ => piece.push(*s),
            }
        }
        push_piece(&mut out, &block.thread, piece);
    }
    out
}

/// `piece` as a colour block in `thread`, when it sews anything.
fn push_piece<'a>(out: &mut Vec<(&'a Thread, Vec<Stitch>)>, thread: &'a Thread, piece: Vec<Stitch>) {
    if piece.iter().any(|s| s.command == Command::Stitch) {
        out.push((thread, piece));
    }
}

/// One colour block's stitch bytes, written from where the needle is.
struct Run {
    bytes: Vec<u8>,
    /// Where the needle is, in tenths of a millimetre from the design's centre (y down).
    x: i32,
    y: i32,
    /// Whether the needle is up: at the block's start and after a jump or a trim, so the next
    /// stitch is reached with a long form.
    up: bool,
    /// The stitches and long forms written: the summary's estimate of the sewing time.
    needle_points: usize,
}

impl Run {
    fn new((x, y): (i32, i32)) -> Self {
        Self { bytes: Vec::new(), x, y, up: true, needle_points: 0 }
    }

    fn add(&mut self, s: Stitch, frame: &super::Frame) {
        match s.command {
            Command::Stitch => {
                let (tx, ty) = frame.point(s);
                if std::mem::take(&mut self.up) {
                    self.travel(tx, ty);
                } else {
                    self.sew(tx, ty);
                }
            }
            // The needle lifts: the next stitch is reached with a long form, which the machine
            // cuts after a trim (VP3 has no trim of its own).
            Command::Jump | Command::Trim => self.up = true,
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
    /// stitch (see the module doc). Already there: just the stitch.
    fn travel(&mut self, tx: i32, ty: i32) {
        let (dx, dy) = (tx - self.x, ty - self.y);
        if dx != 0 || dy != 0 {
            for (sx, sy) in super::steps(dx, dy, MAX_LONG) {
                self.bytes.extend_from_slice(&[CONTROL, LONG_START]);
                self.bytes.extend_from_slice(&(sx as i16).to_be_bytes());
                self.bytes.extend_from_slice(&(sy as i16).to_be_bytes());
                self.bytes.extend_from_slice(&[CONTROL, LONG_END]);
                self.needle_points += 1;
            }
        }
        self.bytes.extend_from_slice(&[0, 0]);
        self.needle_points += 1;
        (self.x, self.y) = (tx, ty);
    }
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
    out.push(RAYON);
    out.push(WEIGHT);
    let description = if t.chosen.is_none() { t.name.as_str() } else { "" };
    for s in ["", description, ""] {
        utf8(out, s)?;
    }
    Ok(())
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
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
    Ok(super::finish(String::new(), d.points, &d.threads))
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
        let thread = read_thread(&mut b, d.threads.len())?;
        // How far the block's stitches move the needle: the stitches say so themselves.
        b.take(POSITION).ok_or_else(|| short(len))?;
        let run_end = open(&mut b, STITCHES_TAG, block_end, "stitch run")?;
        b.take(SCALE.len()).ok_or_else(|| short(len))?;
        if first {
            // A design's first block starts where it says (from the design's centre); every other
            // picks up where the last left off.
            d.x = tenths(cx + sx);
            d.y = tenths(-(cy + sy));
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
fn read_thread(r: &mut Reader<'_>, block: usize) -> Result<Thread> {
    let len = r.data_len();
    let colors = usize::from(r.u8().ok_or_else(|| short(len))?);
    // Six bytes a colour (a byte, then the RGB colour), the byte after them, the material and the
    // weight.
    let table = r.take(COLOR_ENTRY * colors + 1 + 2).ok_or_else(|| short(len))?;
    let rgb = match table.get(1..4) {
        Some(&[red, green, blue]) if colors > 0 => Some([red, green, blue]),
        _ => None,
    };
    let catalogue = read_utf8(r)?;
    let description = read_utf8(r)?;
    let _brand = read_utf8(r)?;
    // Whitespace and NULs (a writer that ends its strings with one) are no part of a name.
    let clean = |s: &str| s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string();
    let (catalogue, description) = (clean(&catalogue), clean(&description));
    let name = if description.is_empty() { catalogue } else { description };
    Ok(match rgb {
        Some(color) => Thread::named(color, &name),
        None => crate::thread::default_thread(block),
    })
}

/// The stitch records up to `end`; refused past [`super::MAX_POINTS`] points.
fn read_stitches(r: &mut Reader<'_>, d: &mut Decoded, end: usize) -> Result<()> {
    // Whether the last record was a long form, whose needle point a zero-length stitch repeats.
    let mut after_long = false;
    while r.pos() + 2 <= end {
        let (Some(b0), Some(b1)) = (r.u8(), r.u8()) else { return Ok(()) };
        if b0 != CONTROL {
            let (dx, dy) = (i64::from(b0 as i8), i64::from(b1 as i8));
            if !(std::mem::take(&mut after_long) && dx == 0 && dy == 0) {
                d.x += dx;
                d.y += dy;
                d.push(Command::Stitch)?;
            }
            continue;
        }
        match b1 {
            LONG_START => {
                if r.pos() + 4 > end {
                    return Ok(());
                }
                let (Some(dx), Some(dy)) = (r.i16_be(), r.i16_be()) else { return Ok(()) };
                d.x += i64::from(dx);
                d.y += i64::from(dy);
                d.push(Command::Jump)?;
                d.push(Command::Stitch)?;
                if r.pos() + 2 <= end && r.data().get(r.pos()..r.pos() + 2) == Some(&[CONTROL, LONG_END][..]) {
                    r.seek(r.pos() + 2);
                }
                after_long = true;
            }
            // Empty: a long form's end on its own, an empty command, and the end of the last block.
            NOTHING | LONG_END | LAST => {}
            // An unknown command: stop rather than guess where the needle goes next.
            _ => return Ok(()),
        }
    }
    Ok(())
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
    use crate::{Block, quantize};
    use vectorcraft_geom::Point;

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
        let threads: Vec<&Thread> = back.blocks.iter().map(|b| &b.thread).collect();
        assert_eq!(threads, [&Thread::named([230, 20, 20], "Poppy"), &Thread::named([20, 40, 210], "Royal")]);
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

    /// A jump is a long form to the next needle point and a zero-length stitch there, whatever its
    /// length; a trim has no record of its own; read back, a jump and the needle point.
    #[test]
    fn a_jump_is_a_long_form_and_a_zero_stitch_and_a_trim_has_no_record() {
        let mut p = design();
        let here = p.blocks[0].stitches[3];
        p.blocks[0].stitches.insert(4, Stitch::new(here.x, here.y, Command::Trim));
        p.blocks[0].stitches.insert(5, Stitch::new(here.x + 2.0, here.y, Command::Jump));
        p.blocks[0].stitches.insert(6, Stitch::new(here.x + 2.0, here.y, Command::Stitch));
        let bytes = write(&p).expect("writes");
        // The long form: 2 mm right, then its end and a zero-length stitch.
        assert!(bytes.windows(10).any(|w| w == [CONTROL, LONG_START, 0, 20, 0, 0, CONTROL, LONG_END, 0, 0]), "a long form for the 2 mm jump");
        // Each block's first stitch is reached the same way, and the last block ends with 80 03.
        let longs = bytes.windows(2).filter(|w| *w == [CONTROL, LONG_START]).count();
        assert_eq!(longs, 3);
        let block_end = bytes.len() - 1;
        assert_eq!(&bytes[block_end - 2..], &[CONTROL, LAST, BLOCK_END]);
        let back = read(&bytes).expect("reads");
        assert_eq!(needle_points(&back), needle_points(&p));
        assert_eq!(back.points().filter(|s| s.command == Command::Trim).count(), 0);
        let commands: Vec<Command> = back.blocks[0].stitches.iter().map(|s| s.command).collect();
        assert_eq!(commands[..2], [Command::Jump, Command::Stitch], "the block's first stitch, reached with the needle up");
        assert_eq!(commands.iter().filter(|c| **c == Command::Jump).count(), 2);
    }

    /// A long form followed by a stitch that moves on (as other writers write it): the needle goes
    /// down where the long form lands, then at the stitch.
    #[test]
    fn a_long_form_puts_the_needle_down_where_it_lands() {
        let mut p = design();
        p.blocks.truncate(1);
        let mut bytes = write(&p).expect("writes");
        // The block's first move: a long form and a zero-length stitch. Make the stitch move on.
        let at = bytes.windows(10).position(|w| w[..2] == [CONTROL, LONG_START] && w[6..8] == [CONTROL, LONG_END]).unwrap();
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

    /// A design planned from art carries its palette thread's name, which is not the colour's: the
    /// description is left empty. A design read from a file keeps its own.
    #[test]
    fn a_planned_threads_palette_name_is_not_written() {
        let mut p = design();
        p.blocks[0].thread.chosen = PaletteThread::new(Palette::Jef, 12);
        let back = read(&write(&p).expect("writes")).expect("reads");
        assert_eq!(back.blocks[0].thread, Thread::named([230, 20, 20], ""));
        assert_eq!(back.blocks[1].thread, Thread::named([20, 40, 210], "Royal"));
    }

    /// Stitches longer than a byte holds are sewn as several; jumps longer than a long form holds
    /// as several long forms.
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
        let threads: Vec<&Thread> = p.blocks.iter().map(|b| &b.thread).collect();
        assert_eq!(threads, [&Thread::named([200, 10, 10], "Twist"), &Thread::named([1, 2, 3], "After")]);
        assert_eq!(needle_points(&p), [vec![(0, 0), (10, 0)], vec![(10, 10), (10, 20)]]);
    }

    /// A second design is read after the first, its first block starting at its own centre.
    #[test]
    fn a_second_design_starts_at_its_own_centre() {
        let first = ((0, 0), vec![(thread([1, 1, 1], "A"), vec![10, 0, 10, 0])]);
        // 50 mm right of the first design's centre (micrometres).
        let second = ((50_000, 0), vec![(thread([2, 2, 2], "B"), vec![0, 10])]);
        let p = read(&assemble(&[first, second])).expect("reads");
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
        assert_eq!(p.blocks[0].thread, Thread::named([9, 8, 7], "Meadow Red"));
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
        let mut no_colour = bytes;
        let block = no_colour.windows(3).position(|w| w == BLOCK_TAG).unwrap();
        // The thread's colour count 0: the record is six bytes shorter, which misreads the rest of
        // the block; still a design or an error.
        no_colour[block + 15] = 0;
        let _ = read(&no_colour);
    }
}
