//! Janome JEF: a fixed 116-byte header (plus two 4-byte arrays per thread colour) and 2-byte
//! records.
//!
//! The header gives where the records start (`0x74 + 8 × colours`), a date, the version letter,
//! the colour count, the record count, the hoop, the design's size and distances from the hoop
//! edges, then one palette index per colour and a `0x0D` word per colour.
//!
//! A record is two signed bytes, the x and y deltas in tenths of a millimetre (at most 127). The
//! byte `0x80` starts a command: `0x80 0x02 dx dy` jumps; `0x80 0x01 dx dy` changes colour;
//! `0x80 0x10` ends the design. Y counts upwards. Colours are indices into Janome's 78-thread
//! chart: a block's is the thread chosen for it when it is Janome's, else the thread nearest its
//! colour ([`Palette::numbers`]).
//!
//! JEF has no trim record of its own. EduTechWiki calls a jump of zero length the trim command,
//! which Janome machines set to cut at a "cutting command" obey; set to cut automatically, they
//! also cut before any jump longer than 3 mm. We write a trim as three zero-length jumps and read
//! each zero-length jump as a trim (consecutive trims are one cut, [`super::finish`]): one is the
//! wiki's trim command, and three is what pyembroidery 1.5.1, run as an outside reader of our
//! files, takes as a trim with its trims setting on (a single one it drops as a jump that goes
//! nowhere).
//!
//! References: EduTechWiki, *Embroidery format JEF* (edutechwiki.unige.ch/en/Embroidery_format_JEF).
//! pyembroidery 1.5.1 (MIT) served only as an outside reader of the files written here
//! (`docs/embroidery.md` › Validation); none of its code was read.

use super::{Emitter, Frame, Reader, TWO_BYTE_COMMAND, add, finish, mm, two_byte_move, walk};
use crate::thread::{self, Palette};
use crate::{Command, Error, Pattern, Result, Stitch};

/// The header's fixed length: everything before the per-colour arrays.
const HEADER: usize = 0x74;
/// The bytes of one record: a command takes two records.
const RECORD_BYTES: usize = 2;
/// The largest delta one record holds, in tenths of a millimetre.
pub(crate) const MAX_DELTA: i32 = 127;
/// The command prefix: `0x80` is not a delta a record uses.
const CONTROL: u8 = TWO_BYTE_COMMAND;
/// The commands' second bytes: the end of the design, a jump (then the deltas), a colour change
/// (then two bytes that don't move the frame).
const END: u8 = 0x10;
const JUMP: u8 = 0x02;
const COLOR_CHANGE: u8 = 0x01;
/// A trim is written as three jumps that go nowhere: one is the wiki's trim command, three is what
/// pyembroidery, run as an outside reader, reads as a trim (see the module doc).
const TRIM_JUMPS: usize = 3;
/// The length of a header word (little-endian).
const WORD: usize = 4;
/// The header bytes each colour takes after [`HEADER`]: its palette index and its `0x0D` word.
const BYTES_PER_COLOR: usize = 2 * WORD;
/// The header's second word, which writers set to this.
const SECOND_WORD: u32 = 0x14;
/// The date field: fourteen ASCII digits (year to second), left as zeros.
const NO_DATE: &[u8; 14] = b"00000000000000";
/// Where the header gives the colour count.
const COLOR_COUNT_AT: usize = 24;
/// The word written per colour after the palette indices.
const COLOR_WORD: u32 = 0x0D;
/// The four hoops the header gives the design's margins in, as half sizes in tenths of a
/// millimetre: 110×110, 50×50, 140×200 and the custom hoop, which writers set to 140×200 too.
const HOOP_HALVES: [(i32, i32); 4] = [(550, 550), (250, 250), (700, 1000), (700, 1000)];
/// The margins written for a hoop the design doesn't fit.
const NO_FIT: i32 = -1;
/// The hoops Janome numbers, smallest first: a design narrower and shorter than the first two
/// values (tenths of a millimetre) gets the third as the header's hoop. 50×50, 126×110, 140×200
/// and 200×200; anything larger gets [`DEFAULT_HOOP`].
const HOOPS: [(u32, u32, u32); 4] = [(500, 500, 1), (1260, 1100, 3), (1400, 2000, 2), (2000, 2000, 4)];
/// The hoop number for a design that fits none of [`HOOPS`]: the 110×110 default.
const DEFAULT_HOOP: u32 = 0;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    // Janome places a design around the hoop's centre: the stitches start there, and the header's
    // extents are distances from it.
    let frame = Frame::centered(p, true)?;
    let b = frame.bounds;
    let colors = p.blocks.iter().filter(|b| !b.stitches.is_empty()).count();
    if colors == 0 {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    // Left, top, right and bottom of the design, in tenths from the centre (y counts upwards).
    let extents = [(-frame.x(b.x0)).max(0), frame.y(b.y0).max(0), frame.x(b.x1).max(0), (-frame.y(b.y1)).max(0)];
    let [left, top, right, bottom] = extents;
    let (width, height) = (left + right, top + bottom);
    // The body first: the header counts its records (two bytes each, the end record included), as
    // Janome's and Ink/Stitch's files do: (file length − offset) / 2.
    let mut body: Vec<u8> = Vec::new();
    write_body(&mut body, p, &frame)?;
    let records = body.len() / RECORD_BYTES;
    let offset = HEADER + BYTES_PER_COLOR * colors;
    let mut out: Vec<u8> = Vec::with_capacity(offset + body.len());
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&SECOND_WORD.to_le_bytes());
    out.extend_from_slice(NO_DATE);
    // The version letter and a byte the wiki lists as unknown (0x20), both left zero, as Janome's
    // software sometimes leaves them.
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(colors as u32).to_le_bytes());
    out.extend_from_slice(&(records as u32).to_le_bytes());
    out.extend_from_slice(&hoop(width, height).to_le_bytes());
    for v in extents {
        out.extend_from_slice(&(v as u32).to_le_bytes());
    }
    // The design's distance from the edges of each of the header's four hoops.
    for (hx, hy) in HOOP_HALVES {
        let margins = [hx - left, hy - top, hx - right, hy - bottom];
        let fit = margins.iter().all(|m| *m >= 0);
        for v in if fit { margins } else { [NO_FIT; 4] } {
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
    }
    // Janome's palette indices, one per block: the thread chosen for it when it is Janome's, else
    // the nearest to its colour, never the block before's (a thread change needs another thread).
    let threads = p.blocks.iter().filter(|b| !b.stitches.is_empty()).map(|b| &b.thread);
    for index in Palette::Jef.numbers(threads) {
        // A thread's number is at most the palette's length, which a word holds.
        let word = u32::try_from(index).map_err(|_| Error::Format(format!("thread number {index} doesn't fit a JEF header")))?;
        out.extend_from_slice(&word.to_le_bytes());
    }
    for _ in 0..colors {
        out.extend_from_slice(&COLOR_WORD.to_le_bytes());
    }
    out.extend_from_slice(&body);
    Ok(out)
}

fn write_body(out: &mut Vec<u8>, p: &Pattern, frame: &Frame) -> Result<()> {
    walk(p, frame, &mut Jef { out, at: (0, 0), wrote: false }, "JEF")?;
    out.extend_from_slice(&[CONTROL, END]);
    Ok(())
}

/// A JEF body being written: its records, where the needle is, and whether a stitch was written
/// yet.
struct Jef<'a> {
    out: &'a mut Vec<u8>,
    at: (i32, i32),
    wrote: bool,
}

impl Emitter for Jef<'_> {
    fn stitch(&mut self, x: i32, y: i32) {
        two_byte_move(self.out, &mut self.at, (x, y), None, MAX_DELTA);
        self.wrote = true;
    }

    fn jump(&mut self, x: i32, y: i32) {
        two_byte_move(self.out, &mut self.at, (x, y), Some(JUMP), MAX_DELTA);
    }

    fn trim(&mut self) {
        for _ in 0..TRIM_JUMPS {
            self.out.extend_from_slice(&[CONTROL, JUMP, 0, 0]);
        }
    }

    fn color_change(&mut self) {
        self.out.extend_from_slice(&[CONTROL, COLOR_CHANGE, 0, 0]);
    }

    fn wrote(&self) -> bool {
        self.wrote
    }

    fn size(&self) -> usize {
        self.out.len()
    }
}

/// The hoop the design fits: 50×50, 126×110, 140×200 or 200×200 (in tenths of a millimetre),
/// else the 110×110 default, as Janome numbers them.
fn hoop(width: i32, height: i32) -> u32 {
    let (w, h) = (u32::try_from(width).unwrap_or(u32::MAX), u32::try_from(height).unwrap_or(u32::MAX));
    HOOPS.iter().find(|(hw, hh, _)| w < *hw && h < *hh).map_or(DEFAULT_HOOP, |(_, _, n)| *n)
}

/// Where the header gives the date, the version letter (and a byte after it), the hoop and the
/// design's extents (left, top, right and bottom from its centre, a word each).
const DATE_AT: usize = 8;
const VERSION_AT: usize = DATE_AT + NO_DATE.len();
const VERSION: usize = 2;
const HOOP_AT: usize = 32;
const EXTENTS_AT: usize = 36;

/// What a JEF's header holds that reading it leaves out: a date or version the writer doesn't
/// write, a hoop other than the smallest the design fits, and thread numbers that name no Janome
/// thread.
pub(crate) fn losses(bytes: &[u8]) -> Vec<super::Loss> {
    let mut out = Vec::new();
    let word = |at: usize| bytes.get(at..at + WORD).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]));
    let dated = bytes.get(DATE_AT..VERSION_AT).is_some_and(|d| d != NO_DATE && d.iter().any(|b| !matches!(b, 0 | b' ')));
    let versioned = bytes.get(VERSION_AT..VERSION_AT + VERSION).is_some_and(|v| v.iter().any(|b| *b != 0));
    let extents: Option<Vec<i32>> = (0..4).map(|k| word(EXTENTS_AT + k * WORD).map(|w| w as i32)).collect();
    let rehooped = match (word(HOOP_AT), extents.as_deref()) {
        (Some(code), Some(&[left, top, right, bottom])) => code != hoop(left.saturating_add(right), top.saturating_add(bottom)),
        _ => false,
    };
    if dated || versioned || rehooped {
        out.push(super::Loss::JefHeader);
    }
    let offset = word(0).map_or(0, |o| o as usize);
    let colors = word(COLOR_COUNT_AT).map_or(0, |c| c as usize).min(offset.saturating_sub(HEADER) / BYTES_PER_COLOR);
    if (0..colors).filter_map(|k| word(HEADER + k * WORD)).any(|n| !usize::try_from(n).is_ok_and(|n| Palette::Jef.is_thread(n))) {
        out.push(super::Loss::NotThreads);
    }
    out
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let mut r = Reader::new(bytes);
    let offset = r.u32().ok_or_else(|| short(bytes.len()))? as usize;
    r.seek(COLOR_COUNT_AT);
    let colors = r.u32().ok_or_else(|| short(bytes.len()))? as usize;
    if offset < HEADER || offset >= bytes.len() {
        return Err(Error::Format(format!("the JEF header points at byte {offset}, past the file's {} bytes", bytes.len())));
    }
    r.seek(HEADER);
    // The count is the file's claim: each colour takes two words of the header before the records
    // start (`offset`), so no more fit than that (an absurd count must not size an allocation).
    let colors = colors.min(offset.saturating_sub(HEADER) / BYTES_PER_COLOR);
    let mut threads = Vec::with_capacity(colors);
    for block in 0..colors {
        // Janome's chart numbers its threads from 1 to 78: 0 and a number past it name none, and
        // read as a stand-in thread named after the number, never one of the chart's (no number
        // wraps round).
        let number = r.u32().ok_or_else(|| short(bytes.len()))?;
        let named = usize::try_from(number).ok().and_then(|index| Palette::Jef.thread(index));
        threads.push(named.unwrap_or_else(|| thread::unknown_thread(block, u64::from(number))));
    }
    r.seek(offset);
    let mut points: Vec<Stitch> = Vec::new();
    // Wide enough that no file overflows it: each record moves the needle at most 127 tenths.
    let (mut x, mut y) = (0i64, 0i64);
    while let Some(b0) = r.u8() {
        let Some(b1) = r.u8() else { break };
        if b0 != CONTROL {
            x += i64::from(b0 as i8);
            y -= i64::from(b1 as i8);
            add(&mut points, Stitch::new(mm(x), mm(y), Command::Stitch), "JEF")?;
            continue;
        }
        match b1 {
            END => break,
            COLOR_CHANGE => {
                // The colour change's two delta bytes, which don't move the frame.
                let _ = r.take(2);
                add(&mut points, Stitch::new(mm(x), mm(y), Command::ColorChange), "JEF")?;
            }
            JUMP => {
                let (Some(dx), Some(dy)) = (r.u8(), r.u8()) else { break };
                let (dx, dy) = (i64::from(dx as i8), i64::from(dy as i8));
                let here = Stitch::new(mm(x), mm(y), Command::Trim);
                if dx == 0 && dy == 0 {
                    add(&mut points, here, "JEF")?;
                } else {
                    x += dx;
                    y -= dy;
                    add(&mut points, Stitch::new(mm(x), mm(y), Command::Jump), "JEF")?;
                }
            }
            // An unknown command: stop rather than guess where the needle goes next.
            _ => break,
        }
    }
    if points.is_empty() {
        return Err(Error::Format("the JEF file has no stitch records".into()));
    }
    Ok(finish(String::new(), points, &threads))
}

fn short(len: usize) -> Error {
    Error::Format(format!("the JEF file is cut short: {len} bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, Thread};
    use vectorcraft_geom::Point;

    fn design() -> Pattern {
        let mut p = Pattern::default();
        let mut a = Block::new(Thread::rgb([230, 20, 20]));
        for i in 0..8 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(Point::new(t * 3.0, t * 1.5)));
        }
        let mut b = Block::new(Thread::rgb([20, 40, 210]));
        b.stitches.push(Stitch::sew(Point::new(10.0, 20.0)));
        // Short enough for one record: a longer stitch is written as two and sewn as two.
        b.stitches.push(Stitch::sew(Point::new(18.0, 23.0)));
        p.blocks.extend([a, b]);
        p
    }

    /// The header's record count is the records the body holds, the end record included, as
    /// Janome's and Ink/Stitch's files have it ((file length − offset) / 2): for designs of one to
    /// three blocks with a jump, a trim and a 30 mm stitch (written as three records).
    #[test]
    fn the_header_counts_the_records_the_body_holds() {
        let int = |bytes: &[u8], at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        for blocks in 1..=3 {
            let mut p = Pattern::default();
            for k in 0..blocks {
                let y = f64::from(k) * 10.0;
                let mut b = Block::new(Thread::rgb([200, 30 * k as u8, 30]));
                b.stitches.push(Stitch::sew(Point::new(0.0, y)));
                b.stitches.push(Stitch::sew(Point::new(3.0, y)));
                b.stitches.push(Stitch::new(20.0, y, Command::Jump));
                b.stitches.push(Stitch::sew(Point::new(20.0, y)));
                b.stitches.push(Stitch::new(20.0, y, Command::Trim));
                b.stitches.push(Stitch::new(40.0, y, Command::Jump));
                b.stitches.push(Stitch::sew(Point::new(40.0, y)));
                // 30 mm: three records of at most 12.7 mm.
                b.stitches.push(Stitch::sew(Point::new(70.0, y)));
                p.blocks.push(b);
            }
            let bytes = write(&p).unwrap();
            let offset = int(&bytes, 0);
            assert_eq!(int(&bytes, 28), (bytes.len() - offset) / RECORD_BYTES, "{blocks} blocks");
            // Walking them from the offset to the end record counts the same.
            let (mut at, mut walked) = (offset, 0);
            loop {
                let (b0, b1) = (bytes[at], bytes[at + 1]);
                let records = if b0 == CONTROL { if b1 == END { 1 } else { 2 } } else { 1 };
                walked += records;
                at += records * RECORD_BYTES;
                if b0 == CONTROL && b1 == END {
                    break;
                }
            }
            assert_eq!(int(&bytes, 28), walked, "{blocks} blocks");
        }
    }

    /// The header's extents are distances from the hoop's centre, where the needle starts: they
    /// must be where the records actually take the needle, or the machine sews off centre.
    #[test]
    fn the_header_extents_are_where_the_stitches_go() {
        let mut p = Pattern::default();
        let mut b = Block::new(Thread::rgb([200, 0, 0]));
        for i in 0..=50 {
            let t = f64::from(i);
            b.stitches.push(Stitch::sew(Point::new(10.0 + t, 5.0 + (t * 0.7) % 35.0)));
        }
        p.blocks.push(b);
        let bytes = write(&p).expect("writes");
        let int = |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let header = [int(36), int(40), int(44), int(48)];
        // Walk the records from the origin, y upwards as the file counts it.
        let (mut x, mut y) = (0i32, 0i32);
        let (mut x0, mut x1, mut y0, mut y1) = (0, 0, 0, 0);
        let mut at = int(0) as usize;
        while at + 1 < bytes.len() {
            let (b0, b1) = (bytes[at], bytes[at + 1]);
            if b0 == CONTROL && b1 == END {
                break;
            }
            let (dx, dy, len) = if b0 == CONTROL { (bytes[at + 2], bytes[at + 3], 4) } else { (b0, b1, 2) };
            x += i32::from(dx as i8);
            y += i32::from(dy as i8);
            at += len;
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        let walked = [-x0, y1, x1, -y0];
        for (h, w) in header.iter().zip(walked) {
            assert!((h - w).abs() <= 1, "header {header:?}, records {walked:?}");
        }
        // Centred: the extents are even on both sides.
        assert!((header[0] - header[2]).abs() <= 1 && (header[1] - header[3]).abs() <= 1, "{header:?}");
        // A 50 × 35 mm design fits the 110 × 110 hoop with 30 mm to spare left and right.
        assert_eq!(int(52), 300);
        // …is exactly as wide as the 50 × 50 one, and the margins above count from its top.
        assert_eq!((int(68), int(72)), (0, 250 - header[1]));
        assert_eq!((int(84), int(88)), (700 - header[0], 1000 - header[1]));
    }

    /// The colour count is the file's claim: a huge one is not believed (on wasm32 sizing a vector
    /// by it would abort).
    #[test]
    fn an_absurd_colour_count_is_not_believed() {
        let mut bytes = vec![0u8; 200];
        bytes[0..4].copy_from_slice(&(HEADER as u32 + 8).to_le_bytes());
        bytes[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        // Records: one stitch, then the end.
        bytes[HEADER + 8..HEADER + 12].copy_from_slice(&[10, 10, CONTROL, END]);
        let p = read(&bytes).expect("reads");
        assert_eq!(p.sewn(), 1);
    }

    /// A trim is three zero-length jumps in the file, and they read back as one trim.
    #[test]
    fn a_trim_is_three_jumps_that_go_nowhere_and_reads_back_as_one() {
        let mut p = design();
        let at = p.blocks[0].stitches.len() - 2;
        let here = p.blocks[0].stitches[at];
        p.blocks[0].stitches.insert(at + 1, Stitch::new(here.x, here.y, Command::Trim));
        let bytes = write(&p).expect("writes");
        let trim = [CONTROL, JUMP, 0, 0];
        let zero_jumps = bytes.windows(4).filter(|w| *w == trim).count();
        assert_eq!(zero_jumps, 3);
        let run = trim.repeat(3);
        assert!(bytes.windows(run.len()).any(|w| w == run.as_slice()), "the jumps are one run");
        let back = read(&bytes).expect("reads");
        assert_eq!(back.points().filter(|s| s.command == Command::Trim).count(), 1);
        assert_eq!(back.sewn(), p.sewn());
    }

    /// One zero-length jump, the wiki's trim command as other writers may use it, is a trim too.
    #[test]
    fn a_single_jump_that_goes_nowhere_is_a_trim() {
        let mut bytes = vec![0u8; HEADER + 8];
        bytes[0..4].copy_from_slice(&(HEADER as u32 + 8).to_le_bytes());
        bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&[10, 0, 10, 0, CONTROL, JUMP, 0, 0, CONTROL, JUMP, 100, 0, 10, 0, 10, 0, CONTROL, END]);
        let p = read(&bytes).expect("reads");
        let commands: Vec<Command> = p.points().map(|s| s.command).collect();
        assert_eq!(commands.iter().filter(|c| **c == Command::Trim).count(), 1, "{commands:?}");
        assert_eq!(p.sewn(), 4);
    }

    /// The header names each block's chosen thread when it is Janome's, and the thread nearest the
    /// block's colour otherwise.
    #[test]
    fn the_header_names_the_chosen_threads() {
        use crate::thread::PaletteThread;
        let mut p = design();
        p.blocks[0].thread.chosen = PaletteThread::new(Palette::Jef, 61);
        p.blocks[1].thread.chosen = PaletteThread::new(Palette::Pec, 61);
        let bytes = write(&p).expect("writes");
        let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        assert_eq!(word(HEADER), 61);
        assert_eq!(word(HEADER + WORD), Palette::Jef.nearest([20, 40, 210], &[61]).index);
        let back = read(&bytes).expect("reads");
        assert_eq!(back.blocks[0].thread, Palette::Jef.thread(61).unwrap());
    }

    /// Header numbers that name no thread, 0 and past 78, read as stand-in threads named after
    /// them, in their blocks' default colours: none wraps round to one of Janome's threads.
    #[test]
    fn numbers_that_name_no_thread_read_as_stand_ins() {
        let mut p = design();
        let extra = p.blocks[1].clone();
        p.blocks.extend([extra.clone(), extra.clone(), extra]);
        let mut bytes = write(&p).expect("writes");
        let numbers = [0, 1, 78, 79, u32::MAX];
        for (k, n) in numbers.iter().enumerate() {
            bytes[HEADER + k * WORD..HEADER + (k + 1) * WORD].copy_from_slice(&n.to_le_bytes());
        }
        let read = read(&bytes).expect("reads");
        let threads: Vec<&Thread> = read.blocks.iter().map(|b| &b.thread).collect();
        let want = [
            thread::unknown_thread(0, 0),
            Palette::Jef.thread(1).unwrap(),
            Palette::Jef.thread(78).unwrap(),
            thread::unknown_thread(3, 79),
            thread::unknown_thread(4, u64::from(u32::MAX)),
        ];
        assert_eq!(threads, want.iter().collect::<Vec<_>>());
    }

    #[test]
    fn a_design_round_trips_through_the_header_and_records() {
        let p = design();
        let bytes = write(&p).expect("writes");
        let back = read(&bytes).expect("reads");
        assert_eq!(back.blocks.len(), 2);
        assert_eq!(back.sewn(), p.sewn());
        let a = back.bounds().expect("bounds");
        let b = p.bounds().expect("bounds");
        assert!((a.width() - b.width()).abs() < 0.2, "{a:?} {b:?}");
        assert!((a.height() - b.height()).abs() < 0.2, "{a:?} {b:?}");
    }
}
