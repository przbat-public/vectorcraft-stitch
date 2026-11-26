//! Husqvarna Viking HUS: read, never written (the project owner's choice, 2026-11-18). The
//! machines that need it are Husqvarna Viking's of the 1990s; their successors read VP3, which is
//! written ([`super::vp3`]).
//!
//! The file is a 42-byte header, a colour list, then three compressed streams of one byte per
//! record: the records' kinds, their x moves and their y moves. The header, little-endian: the
//! magic (four bytes, [`MAGICS`]), the record count (u32), the colour count (u32), how far the
//! design reaches right, up, left and down from where it starts (four i16, the last two negative),
//! where each stream starts (three u32; the last runs to the end of the file), eight bytes the
//! sources call a name or leave unexplained, and two bytes that are 0; then one u16 per colour.
//! Each stream expands ([`super::lzh`]) to as many bytes as there are records. A move is a signed
//! byte in tenths of a millimetre, y counting upwards.
//!
//! The kinds of record: 0x80 a stitch, 0x81 a jump, 0x84 a colour change (the next colour of the
//! list), 0x90 the last stitch (Jason Weiler's page). Two more appear in the real files below, which
//! no source names, both with no move: 0x88, before a jump or a design's first jump, is read as a
//! cut (pyembroidery, run as an outside reader of those files, reads it as one too), and 0x82,
//! between stitches and found in the appliqué and cutwork designs, as a stop (the machine stops,
//! the thread kept, as an appliqué piece is laid down): a stop read where the file had none costs
//! a pause at most, while one missed would sew an appliqué's next step with nothing laid down.
//! 0x90 also ends a part of a design that a colour change and more records follow: it is no needle
//! point, and the records after it are read (pyembroidery stops at it, and at 0x82). A record of
//! another kind is a move with the needle up, so that what follows stays where the file puts it.
//! Where a cut, a stop, a colour change or an end moves, it is made first and the move is a jump.
//! Every colour change moves on one colour of the list, whether it moves the needle or anything is
//! sewn after it, so two in a row pass over two colours (the project owner's choice, 2026-11-26:
//! every real file below lists one more colour than its colour changes; `threads`).
//!
//! The colours are numbers 0 to 29 of a chart no source publishes. Weiler names them, by
//! experiment, all but number 16 ("???"); the colours shown for the names are estimates made here
//! ([`COLOURS`]), the project owner's choice (2026-11-18, kept on 2026-11-26 knowing that a VP3
//! export writes them). They stand in as the colours of a design opened from a format without
//! colours do: an export to PES, PEC or JEF matches them to that palette's threads by the plan's
//! rule, and one to VP3 writes them as its threads' colours, as it writes every block's art colour.
//! A number with no name reads as a grey stand-in named after it.
//!
//! Checked on 6,944 real HUS files the project owner holds (used on their computer only, never
//! committed): every one with a stitch opens (four files of alphabets hold only moves); the
//! header's record and colour counts, extents and stream offsets are as above in all of them (the
//! colour count is one more than the colour changes); and the y axis counts upwards: read so, the
//! stitches of 2,576 of the 2,585 designs the files also hold as JEF, and of all 58 they hold as
//! PES, overlap those copies' more than read the other way up (on a 48 × 48 grid over each design;
//! the 9 others are nearly symmetrical top to bottom). Weiler's header notes give HUS's +Y extent
//! as DST's −Y and its −Y as DST's +Y, which would put y counting down; the files settle it the
//! other way, as DST and JEF count it. Two files end before their last records: they read as far
//! as they go, saying that they end early ([`losses`]).
//!
//! References: EduTechWiki, *Embroidery format HUS* (edutechwiki.unige.ch/en/
//! Embroidery_format_HUS) and *Embroidery format* (…/Embroidery_format); KDE Community Wiki,
//! *Husqvarna HUS* (community.kde.org/Projects/Liberty/File_Formats/Husqvarna_HUS); Jason Weiler,
//! *HUS and VIP Embroidery File Formats* (jasonweiler.com/HUSandVIPFileFormatInfo.html; facts only,
//! its text is all rights reserved); The National Archives, PRONOM fmt/2000 (the magic).
//! pyembroidery 1.5.1 (MIT) served only as an outside reader; none of its code was read.

use super::{Loss, MAX_POINTS, Reader, add, finish, lzh, mm};
use crate::{Command, Error, Pattern, Result, Stitch, Thread};

/// The first four bytes of a HUS file: 0x5B or 0x5D, then 0xAF, 0xFF or 0xFC, then 0xC8 0x00
/// (PRONOM fmt/2000; the KDE wiki gives 5D FC and 5B AF, which every real file above starts with).
pub(crate) const MAGICS: [&[u8]; 6] = [
    &[0x5B, 0xAF, 0xC8, 0x00],
    &[0x5B, 0xFF, 0xC8, 0x00],
    &[0x5B, 0xFC, 0xC8, 0x00],
    &[0x5D, 0xAF, 0xC8, 0x00],
    &[0x5D, 0xFF, 0xC8, 0x00],
    &[0x5D, 0xFC, 0xC8, 0x00],
];
/// The format's name in errors.
const WHAT: &str = "HUS";
/// The largest move one record holds, tenths of a millimetre: a signed byte's.
pub(crate) const MAX_DELTA: i32 = 127;

/// Where the header gives the record count and the colour count (u32 each).
const RECORDS_AT: usize = 4;
/// Where the header gives where the three streams start (u32 each).
const STREAMS_AT: usize = 20;
/// The header's eight bytes after the stream offsets: a name of ASCII digits in the real files
/// ("001"), padded with NULs; read as the design's name.
const NAME_AT: usize = 32;
const NAME_LEN: usize = 8;
/// Where the colour list starts, after the header: one u16 per colour.
const COLOURS_AT: usize = 42;
const COLOUR_BYTES: usize = 2;

/// The kinds of record.
const STITCH: u8 = 0x80;
const JUMP: u8 = 0x81;
const STOP: u8 = 0x82;
const COLOR_CHANGE: u8 = 0x84;
const TRIM: u8 = 0x88;

/// The colours of HUS's chart, by number, with the names Weiler gives them (number 16 has none):
/// each colour an sRGB shade chosen for its name, an estimate, not a measurement. The plain colours
/// are bright thread shades; the "Light" ones lie toward white and the "Dark" ones toward black,
/// most about half way, while Dark Yellow, Dark Orange, Dark Pink and Light Brown stay nearer their
/// plain colours, as gold, rust, rose and tan threads do.
pub(crate) const COLOURS: [Option<([u8; 3], &str)>; 30] = [
    Some(([0x00, 0x00, 0x00], "Black")),
    Some(([0x10, 0x40, 0xC0], "Blue")),
    Some(([0x10, 0x90, 0x30], "Green")),
    Some(([0xD0, 0x18, 0x18], "Red")),
    Some(([0xB0, 0x28, 0xA0], "Magenta")),
    Some(([0xF8, 0xD8, 0x00], "Yellow")),
    Some(([0x80, 0x80, 0x80], "Gray")),
    Some(([0x80, 0xB0, 0xE8], "Light Blue")),
    Some(([0x88, 0xD0, 0x88], "Light Green")),
    Some(([0xF8, 0x88, 0x10], "Orange")),
    Some(([0xF0, 0x88, 0xB0], "Pink")),
    Some(([0x88, 0x50, 0x20], "Brown")),
    Some(([0xFF, 0xFF, 0xFF], "White")),
    Some(([0x08, 0x20, 0x70], "Dark Blue")),
    Some(([0x08, 0x50, 0x20], "Dark Green")),
    Some(([0x80, 0x08, 0x08], "Dark Red")),
    None,
    Some(([0xF0, 0x80, 0x80], "Light Red")),
    Some(([0x58, 0x18, 0x70], "Dark Purple")),
    Some(([0xC8, 0x98, 0xE0], "Light Purple")),
    Some(([0xB8, 0x98, 0x00], "Dark Yellow")),
    Some(([0xFF, 0xF0, 0x98], "Light Yellow")),
    Some(([0x50, 0x50, 0x50], "Dark Grey")),
    Some(([0xC0, 0xC0, 0xC0], "Light Grey")),
    Some(([0xC0, 0x58, 0x00], "Dark Orange")),
    Some(([0xFF, 0xB8, 0x70], "Light Orange")),
    Some(([0xC0, 0x38, 0x78], "Dark Pink")),
    Some(([0xFF, 0xC0, 0xD8], "Light Pink")),
    Some(([0x50, 0x2C, 0x10], "Dark Brown")),
    Some(([0xB8, 0x88, 0x50], "Light Brown")),
];
/// The colour a number with no name in [`COLOURS`] reads in: a grey of its own, so that it isn't
/// taken for a colour of the chart.
const STAND_IN: [u8; 3] = [0xA0, 0xA0, 0xA0];

/// The thread colour number `number` of the chart names: its name and estimated colour, or the
/// grey stand-in named after the number ("Unknown thread 16").
fn thread(number: u16) -> Thread {
    match COLOURS.get(usize::from(number)).copied().flatten() {
        Some((color, name)) => Thread::named(color, name),
        None => Thread::named(STAND_IN, &format!("Unknown thread {number}")),
    }
}

/// The error for a header whose counts or offsets point past the file.
fn damaged_header(len: usize) -> Error {
    Error::Format(format!("the HUS header is cut short or points past the file's {len} bytes"))
}

/// A HUS file's header, as far as reading it takes, and its three streams expanded.
struct Streams {
    /// The colours the header claims.
    colours: usize,
    /// Where the first stream starts: the colour list ends there.
    kinds_at: usize,
    name: String,
    kinds: lzh::Expanded,
    xs: lzh::Expanded,
    ys: lzh::Expanded,
}

impl Streams {
    /// Whether every stream made the records the header counts: not in a file cut short, nor in
    /// one whose stream ends first.
    fn whole(&self) -> bool {
        self.kinds.whole && self.xs.whole && self.ys.whole
    }
}

/// The HUS file `bytes`'s header, checked, and its streams expanded.
fn streams(bytes: &[u8]) -> Result<Streams> {
    if !MAGICS.iter().any(|m| bytes.starts_with(m)) {
        return Err(Error::Format("not a HUS file: it does not start with a HUS signature".into()));
    }
    let mut r = Reader::new(bytes);
    r.seek(RECORDS_AT);
    let (Some(count), Some(colours)) = (r.u32(), r.u32()) else { return Err(damaged_header(bytes.len())) };
    r.seek(STREAMS_AT);
    let (Some(kinds_at), Some(xs_at), Some(ys_at)) = (r.u32(), r.u32(), r.u32()) else { return Err(damaged_header(bytes.len())) };
    let [kinds_at, xs_at, ys_at] = [kinds_at, xs_at, ys_at].map(|at| usize::try_from(at).unwrap_or(usize::MAX));
    if !(COLOURS_AT <= kinds_at && kinds_at <= xs_at && xs_at <= ys_at && ys_at <= bytes.len()) {
        return Err(damaged_header(bytes.len()));
    }
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    if count > MAX_POINTS {
        return Err(Error::TooLarge(format!("the {what} file holds more than {MAX_POINTS} stitches, more than a design may have", what = WHAT)));
    }
    let colours = usize::try_from(colours).unwrap_or(usize::MAX);
    let name: String = bytes
        .get(NAME_AT..NAME_AT + NAME_LEN)
        .unwrap_or_default()
        .iter()
        .take_while(|b| **b != 0)
        .map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' })
        .collect::<String>()
        .trim()
        .to_string();
    let stream = |from: usize, to: usize| {
        lzh::expand(bytes.get(from..to).unwrap_or_default(), count)
            .map_err(|_| Error::Format("the HUS file's compressed stitches are damaged".into()))
    };
    let (kinds, xs, ys) = (stream(kinds_at, xs_at)?, stream(xs_at, ys_at)?, stream(ys_at, bytes.len())?);
    Ok(Streams { colours, kinds_at, name, kinds, xs, ys })
}

/// What a HUS holds that reading it leaves out: nothing of its header (its counts, extents and
/// stream offsets the records give again; its name is read). But a file whose streams stop short
/// of the records its header counts, as a file cut short does, is said to end early, so that it
/// isn't taken for whole and writing over it asks first where writing is checked (the project
/// owner's choice, 2026-11-26).
/// The streams are expanded again for it: a file is opened once, and expanding is linear in its
/// records.
pub(crate) fn losses(bytes: &[u8]) -> Vec<Loss> {
    match streams(bytes) {
        Ok(s) if !s.whole() => vec![Loss::EndsEarly],
        _ => Vec::new(),
    }
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let Streams { colours, kinds_at, name, kinds, xs, ys } = streams(bytes)?;
    // A stream that ran out (a file cut short) ends the records there.
    let mut points: Vec<Stitch> = Vec::new();
    // Wide enough for any file: each record moves the needle at most 128 tenths.
    let (mut x, mut y) = (0i64, 0i64);
    for ((&kind, &dx), &dy) in kinds.bytes.iter().zip(&xs.bytes).zip(&ys.bytes) {
        let (dx, dy) = (i64::from(dx as i8), i64::from(dy as i8));
        let before = Stitch::new(mm(x), mm(y), Command::Stitch);
        x += dx;
        // y counts upwards in the file, as the real files above show (against Weiler's header
        // notes), and downwards here.
        y -= dy;
        let here = |command| Stitch::new(mm(x), mm(y), command);
        let moved = dx != 0 || dy != 0;
        match kind {
            STITCH => add(&mut points, here(Command::Stitch), WHAT)?,
            JUMP => add(&mut points, here(Command::Jump), WHAT)?,
            STOP | COLOR_CHANGE | TRIM => {
                let command = match kind {
                    STOP => Command::Stop,
                    COLOR_CHANGE => Command::ColorChange,
                    _ => Command::Trim,
                };
                add(&mut points, Stitch { command, ..before }, WHAT)?;
                if moved {
                    add(&mut points, here(Command::Jump), WHAT)?;
                }
            }
            // The end of the design or of a part of it, and kinds no source names: no needle point.
            _ => {
                if moved {
                    add(&mut points, here(Command::Jump), WHAT)?;
                }
            }
        }
    }
    if !points.iter().any(|s| s.command == Command::Stitch) {
        return Err(Error::Format(format!("the {what} file has no stitch records", what = WHAT)));
    }
    let threads = threads(bytes.get(COLOURS_AT..kinds_at).unwrap_or_default(), colours, &points);
    Ok(finish(name, points, &threads))
}

/// The threads of the blocks of `points`, in order, from the colour list `list` (a u16 per colour,
/// in the room before the first stream), of which the header claims `claimed`. Every colour change
/// moves on one colour of the list, whether it moves the needle or anything is sewn after it (the
/// project owner's choice, 2026-11-26: Weiler has a colour change move on to the list's next
/// colour, and real files list one more colour than their colour changes), so a block takes the
/// colour its colour changes come to, and colour changes in a row pass over a colour each.
/// [`finish`] makes a block of the points between two colour changes that hold any (joining jumps
/// in a row never leaves one empty), so a thread is made for each of those, up to the first the
/// list has no colour for: the blocks from there take the default threads, in their order. Made
/// from the list's room alone, a hostile count would make a thread, its name on the heap too, for
/// every two bytes of the file.
fn threads(list: &[u8], claimed: usize, points: &[Stitch]) -> Vec<Thread> {
    let (numbers, _) = list.as_chunks::<COLOUR_BYTES>();
    points
        .split(|s| s.command == Command::ColorChange)
        .enumerate()
        .filter(|(_, run)| !run.is_empty())
        .map_while(|(changes, _)| numbers.get(changes).filter(|_| changes < claimed))
        .map(|b| thread(u16::from_le_bytes(*b)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use vectorcraft_testkit::machine_files::{CUT_CASES_LIMIT, Lzh, hus, within};

    const NAME: &[u8; 8] = b"007\0\0\0\0\0";

    /// A design of two colours: a run of stitches with a cut and a jump inside, a stop between two
    /// stitches, a colour change, more stitches, the end.
    fn records() -> Vec<(u8, i8, i8)> {
        let mut r = vec![(STITCH, 0, 0)];
        r.extend((0..20).map(|i| (STITCH, 10, if i % 2 == 0 { 15 } else { -15 })));
        r.extend([(TRIM, 0, 0), (JUMP, 100, 0), (JUMP, 100, 20), (STITCH, 0, 0), (STITCH, 5, 5), (STOP, 0, 0), (STITCH, 5, 5)]);
        r.extend([(COLOR_CHANGE, 0, 0), (JUMP, -50, -50), (STITCH, 0, 0)]);
        r.extend((0..10).map(|_| (STITCH, -10, 7)));
        r.push((0x90, 0, 0));
        r
    }

    /// The design reads as written: its needle points where the moves put them (y upwards in the
    /// file, downwards here), its blocks in the colour list's threads, its cut and its stop, its
    /// name; with or without copies in the streams, in one block or many.
    #[test]
    fn a_design_reads_as_written() {
        for o in [Lzh::default(), Lzh { copies: false, block_tokens: 5 }] {
            let p = read(&hus(&records(), &[3, 7], NAME, &o)).expect("reads");
            assert_eq!(p.name, "007");
            let [a, b] = p.blocks.as_slice() else { panic!("two blocks: {:?}", p.blocks.len()) };
            assert_eq!((a.thread.name.as_str(), a.thread.color, b.thread.name.as_str()), ("Red", [0xD0, 0x18, 0x18], "Light Blue"));
            assert_eq!(p.sewn(), 1 + 20 + 3 + 1 + 10);
            assert_eq!(a.stitches.iter().filter(|s| s.command == Command::Trim).count(), 1);
            assert_eq!(a.stitches.iter().filter(|s| s.command == Command::Stop).count(), 1);
            // The first block's zigzag: up 1.5 mm in the file is up on the page too.
            let sewn: Vec<&Stitch> = a.stitches.iter().filter(|s| s.command == Command::Stitch).collect();
            assert!((sewn[1].y - sewn[0].y - -1.5).abs() < 1e-9 && (sewn[1].x - sewn[0].x - 1.0).abs() < 1e-9, "{:?} {:?}", sewn[0], sewn[1]);
        }
    }

    /// A HUS opens by its magic whatever its name, under each of the six PRONOM gives,
    /// (5B|5D)(AF|FF|FC) C8 00, built here from that pattern rather than taken from [`MAGICS`],
    /// which holds those six and no other; a VIP (90 01 in place of C8 00) and other bytes are
    /// refused, saying so.
    #[test]
    fn a_hus_is_told_by_its_magic() {
        let pronom: Vec<[u8; 4]> = [0x5B, 0x5D].into_iter().flat_map(|a| [0xAF, 0xFF, 0xFC].map(|b| [a, b, 0xC8, 0x00])).collect();
        assert_eq!(MAGICS.len(), pronom.len());
        assert!(pronom.iter().all(|p| MAGICS.contains(&p.as_slice())), "{MAGICS:02X?}");
        let bytes = hus(&records(), &[3, 7], NAME, &Lzh::default());
        for magic in &pronom {
            let mut b = bytes.clone();
            b[..4].copy_from_slice(magic);
            assert_eq!(Format::sniff(&b), Some(Format::Hus), "{magic:02X?}");
            assert!(read(&b).is_ok(), "{magic:02X?}");
        }
        let mut vip = bytes.clone();
        vip[..4].copy_from_slice(&[0x5D, 0xFC, 0x90, 0x01]);
        assert_eq!(Format::sniff(&vip), None);
        assert!(read(&vip).unwrap_err().to_string().contains("not a HUS file"));
    }

    /// Colour numbers with no name, and past the chart, read as a grey stand-in named after the
    /// number; a colour count past the list's room is the room (the blocks past the list take the
    /// default threads, not the streams' bytes); colour changes past the list take the default
    /// threads.
    #[test]
    fn unknown_colours_read_as_stand_ins() {
        let p = read(&hus(&records(), &[16, 300], NAME, &Lzh::default())).unwrap();
        let names: Vec<&str> = p.blocks.iter().map(|b| b.thread.name.as_str()).collect();
        assert_eq!(names, ["Unknown thread 16", "Unknown thread 300"]);
        assert!(p.blocks.iter().all(|b| b.thread.color == STAND_IN));
        let mut three = records();
        let end = three.pop();
        three.extend([(COLOR_CHANGE, 0, 0), (STITCH, 5, 5), (STITCH, 5, 5)]);
        three.extend(end);
        let mut claims_more = hus(&three, &[3, 7], NAME, &Lzh::default());
        claims_more[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        let read_more = read(&claims_more).unwrap();
        assert_eq!(read_more.blocks.len(), 3);
        assert_eq!(read_more.blocks[2].thread, crate::thread::default_thread(2));
        assert_eq!(read_more, read(&hus(&three, &[3, 7], NAME, &Lzh::default())).unwrap());
        let one = read(&hus(&records(), &[3], NAME, &Lzh::default())).unwrap();
        assert_eq!(one.blocks.len(), 2);
        assert_eq!(one.blocks[1].thread, crate::thread::default_thread(1));
    }

    /// However many colours the header claims and the list holds, the threads made are the ones
    /// the blocks take: one per block, and colour changes in a row, before the first point or
    /// after the last make none of their own.
    #[test]
    fn a_long_colour_list_makes_only_the_threads_used() {
        let list = vec![0u8; COLOUR_BYTES * 100_000];
        let (s, change) = (Stitch::new(0.0, 0.0, Command::Stitch), Stitch::new(0.0, 0.0, Command::ColorChange));
        let points = [s, change, Stitch::new(1.0, 0.0, Command::Stitch)];
        assert_eq!(threads(&list, usize::MAX, &points).len(), 2);
        assert_eq!(threads(&list, 1, &points).len(), 1);
        assert_eq!(threads(&list[..COLOUR_BYTES], usize::MAX, &points).len(), 1);
        for (points, blocks) in [(vec![s, change, change, change, s], 2), (vec![change, s, change], 1), (vec![s], 1)] {
            assert_eq!(threads(&list, usize::MAX, &points).len(), blocks, "{points:?}");
            assert_eq!(finish(String::new(), points, &[]).blocks.len(), blocks, "as finish makes them");
        }
    }

    /// Every colour change moves on one colour of the list, whether it moves the needle or not and
    /// whether anything is sewn after it: two in a row pass over two colours, one before the first
    /// stitch over the first; one after the last point takes nothing, and one at the end that moves
    /// the needle takes the next colour for its move, a block of that jump alone.
    #[test]
    fn every_colour_change_moves_on_one_colour() {
        let colours = [3, 7, 1];
        let last = |records: &[(u8, i8, i8)]| read(&hus(records, &colours, NAME, &Lzh::default())).unwrap().blocks.last().map(|b| b.thread.clone());
        let in_a_row_moving = [(STITCH, 0, 0), (COLOR_CHANGE, 5, 0), (COLOR_CHANGE, 0, 0), (STITCH, 0, 0)];
        let in_a_row_still = [(STITCH, 0, 0), (COLOR_CHANGE, 0, 0), (COLOR_CHANGE, 0, 0), (STITCH, 0, 0)];
        assert_eq!(last(&in_a_row_moving), Some(thread(1)));
        assert_eq!(last(&in_a_row_still), Some(thread(1)));
        let first_change = read(&hus(&[(COLOR_CHANGE, 0, 0), (STITCH, 0, 0), (STITCH, 5, 0)], &colours, NAME, &Lzh::default())).unwrap();
        assert_eq!(first_change.blocks.iter().map(|b| b.thread.clone()).collect::<Vec<_>>(), [thread(7)]);
        let change_last = read(&hus(&[(STITCH, 0, 0), (STITCH, 5, 0), (COLOR_CHANGE, 0, 0)], &colours, NAME, &Lzh::default())).unwrap();
        assert_eq!(change_last.blocks.iter().map(|b| b.thread.clone()).collect::<Vec<_>>(), [thread(3)]);
        let moving_last = read(&hus(&[(STITCH, 0, 0), (STITCH, 5, 0), (COLOR_CHANGE, 5, 0)], &colours, NAME, &Lzh::default())).unwrap();
        assert_eq!(moving_last.blocks.iter().map(|b| b.thread.clone()).collect::<Vec<_>>(), [thread(3), thread(7)]);
        assert_eq!(moving_last.blocks[1].stitches.iter().map(|s| s.command).collect::<Vec<_>>(), [Command::Jump]);
        // Past the list, the default threads, by block.
        let past = read(&hus(&in_a_row_still, &colours[..2], NAME, &Lzh::default())).unwrap();
        assert_eq!(past.blocks.iter().map(|b| b.thread.clone()).collect::<Vec<_>>(), [thread(3), crate::thread::default_thread(1)]);
    }

    /// A move on a cut, a stop, an end or a colour change is made after it, with the needle up (the
    /// next block starting with it after a colour change), an end that doesn't move adds nothing,
    /// and a jump is a point of its own: each point read where the records put it.
    #[test]
    fn moves_are_read_where_the_records_put_them() {
        let records = [
            (STITCH, 0, 0),
            (STITCH, 10, 0),
            (JUMP, 100, 0),
            (STITCH, 10, 0),
            (TRIM, 30, 0),
            (STITCH, 0, 0),
            (STOP, 0, 20),
            (STITCH, 0, 0),
            (0x90, 50, 0),
            (STITCH, 0, 0),
            (0x90, 0, 0),
            (STITCH, 10, 0),
            (COLOR_CHANGE, 0, -30),
            (STITCH, 0, 0),
        ];
        let p = read(&hus(&records, &[1, 2], NAME, &Lzh::default())).unwrap();
        assert_eq!(p.blocks.len(), 2);
        assert_eq!(p.blocks[1].stitches.first().map(|s| s.command), Some(Command::Jump), "the colour change's move starts the next block");
        let first = p.points().next().unwrap();
        let got: Vec<(Command, f64, f64)> = p.points().map(|s| (s.command, s.x - first.x, s.y - first.y)).collect();
        use Command::{Jump as J, Stitch as S, Stop as P, Trim as T};
        let want = [
            (S, 0.0, 0.0),
            (S, 1.0, 0.0),
            (J, 11.0, 0.0),
            (S, 12.0, 0.0),
            (T, 12.0, 0.0),
            (J, 15.0, 0.0),
            (S, 15.0, 0.0),
            (P, 15.0, 0.0),
            (J, 15.0, -2.0),
            (S, 15.0, -2.0),
            (J, 20.0, -2.0),
            (S, 20.0, -2.0),
            (S, 21.0, -2.0),
            (J, 21.0, 1.0),
            (S, 21.0, 1.0),
        ];
        assert_eq!(got.len(), want.len(), "{got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(g.0 == w.0 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9, "{got:?}");
        }
    }

    /// A file cut anywhere in its last stream, inside a block's tables or a copy too, reads its
    /// records up to the cut, never fewer for a later cut and none past it (or, cut before its
    /// first stitch, says it has none), and a cut short of the whole reads part of the design,
    /// saying that the file ends early, as one whose header counts more records than its streams
    /// hold does, and a whole file doesn't; a header that points past the file, a count past what
    /// a design may have and damaged streams are refused. The cuts run under a time limit, so a
    /// reader that goes round on the zero bits read past a cut fails here.
    #[test]
    fn cut_and_damaged_files() {
        within(CUT_CASES_LIMIT, cut_and_damaged_cases);
    }

    fn cut_and_damaged_cases() {
        for o in [Lzh::default(), Lzh { copies: false, block_tokens: 5 }, Lzh { copies: true, block_tokens: 3 }] {
            let whole = hus(&records(), &[3, 7], NAME, &o);
            let full = read(&whole).unwrap();
            assert_eq!(losses(&whole), [], "{o:?}: a whole file");
            let ys_at = u32::from_le_bytes([whole[28], whole[29], whole[30], whole[31]]) as usize;
            let (mut before, mut part) = (0, false);
            for cut in ys_at..=whole.len() {
                let sewn = match read(&whole[..cut]) {
                    Ok(p) => p.sewn(),
                    Err(e) => {
                        assert!(e.to_string().contains("no stitch records"), "{o:?}: cut at {cut}: {e}");
                        0
                    }
                };
                assert!(sewn >= before && sewn <= full.sewn(), "{o:?}: cut at {cut}: {sewn} needle points, {before} before");
                if sewn > 0 && sewn < full.sewn() {
                    part = true;
                    assert_eq!(losses(&whole[..cut]), [Loss::EndsEarly], "{o:?}: cut at {cut}, part of the design read");
                }
                before = sewn;
            }
            assert_eq!(before, full.sewn(), "{o:?}");
            assert!(part, "{o:?}: a file cut short reads part of the design");
            let mut counts_more = whole.clone();
            let count = u32::from_le_bytes([whole[4], whole[5], whole[6], whole[7]]);
            counts_more[4..8].copy_from_slice(&(count + 1).to_le_bytes());
            assert_eq!(read(&counts_more).unwrap(), full, "{o:?}: the records its streams hold");
            assert_eq!(losses(&counts_more), [Loss::EndsEarly], "{o:?}: streams that end before the records counted");
        }
        let whole = hus(&records(), &[3, 7], NAME, &Lzh::default());
        let mut past = whole.clone();
        past[28..32].copy_from_slice(&(whole.len() as u32 + 1).to_le_bytes());
        assert!(read(&past).unwrap_err().to_string().contains("points past the file"));
        // The first stream starting inside the header, and the last two the other way round.
        let mut inside = whole.clone();
        inside[STREAMS_AT..STREAMS_AT + 4].copy_from_slice(&(COLOURS_AT as u32 - 1).to_le_bytes());
        assert!(read(&inside).unwrap_err().to_string().contains("points past the file"));
        let mut swapped = whole.clone();
        swapped[STREAMS_AT + 4..STREAMS_AT + 8].copy_from_slice(&whole[STREAMS_AT + 8..STREAMS_AT + 12]);
        swapped[STREAMS_AT + 8..STREAMS_AT + 12].copy_from_slice(&whole[STREAMS_AT + 4..STREAMS_AT + 8]);
        assert!(read(&swapped).unwrap_err().to_string().contains("points past the file"));
        let mut many = whole.clone();
        many[4..8].copy_from_slice(&(MAX_POINTS as u32 + 1).to_le_bytes());
        assert!(matches!(read(&many), Err(Error::TooLarge(_))));
        let mut damaged = whole.clone();
        let at = u32::from_le_bytes([whole[20], whole[21], whole[22], whole[23]]) as usize;
        // The first block's 19-code table says it has 31 codes.
        damaged[at + 2] = 0xFF;
        assert!(read(&damaged).is_err());
        assert!(read(&hus(&[(JUMP, 5, 5)], &[1], NAME, &Lzh::default())).unwrap_err().to_string().contains("no stitch records"));
    }

    /// The kinds of record are the numbers Weiler gives, written here as numbers, not as this
    /// module's names for them: 0x80 a stitch, 0x81 a jump (a point of its own, even where it
    /// doesn't move the needle), 0x84 a colour change, 0x90 the end, which is no point.
    #[test]
    fn the_kinds_are_weilers_numbers() {
        let records = [(0x80, 0, 0), (0x80, 10, 0), (0x81, 0, 0), (0x80, 10, 0), (0x84, 0, 0), (0x80, 10, 0), (0x90, 0, 0)];
        let p = read(&hus(&records, &[3, 7], NAME, &Lzh::default())).unwrap();
        let commands: Vec<Vec<Command>> = p.blocks.iter().map(|b| b.stitches.iter().map(|s| s.command).collect()).collect();
        use Command::{Jump as J, Stitch as S};
        assert_eq!(commands, [vec![S, S, J, S], vec![S]]);
    }

    /// The header's colour count caps the list as reading takes it: of a list of three numbers
    /// the header claims one, so the second block takes a default thread, not the list's second.
    #[test]
    fn the_headers_colour_count_caps_the_list() {
        /// Where the header gives the colour count: after the record count, a u32 each.
        const COLOUR_COUNT_AT: usize = RECORDS_AT + 4;
        let records = [(STITCH, 0, 0), (STITCH, 5, 0), (COLOR_CHANGE, 0, 0), (STITCH, 5, 0)];
        let mut bytes = hus(&records, &[3, 7, 1], NAME, &Lzh::default());
        bytes[COLOUR_COUNT_AT..COLOUR_COUNT_AT + 4].copy_from_slice(&1u32.to_le_bytes());
        let threads: Vec<Thread> = read(&bytes).unwrap().blocks.iter().map(|b| b.thread.clone()).collect();
        assert_eq!(threads, [thread(3), crate::thread::default_thread(1)]);
    }

    /// The chart's names by number as Weiler's page gives them, its two columns (0x00 to 0x0F
    /// beside 0x10 to 0x1D) read again here number by number, apart from [`COLOURS`], so that a
    /// row taken from the other column fails; his "Magenta (Purple)" is shortened, and his 0x10,
    /// "???", reads as a stand-in.
    #[test]
    fn the_colour_names_are_weilers_by_number() {
        let weiler = [
            (0x00, "Black"),
            (0x01, "Blue"),
            (0x02, "Green"),
            (0x03, "Red"),
            (0x04, "Magenta"),
            (0x05, "Yellow"),
            (0x06, "Gray"),
            (0x07, "Light Blue"),
            (0x08, "Light Green"),
            (0x09, "Orange"),
            (0x0A, "Pink"),
            (0x0B, "Brown"),
            (0x0C, "White"),
            (0x0D, "Dark Blue"),
            (0x0E, "Dark Green"),
            (0x0F, "Dark Red"),
            (0x11, "Light Red"),
            (0x12, "Dark Purple"),
            (0x13, "Light Purple"),
            (0x14, "Dark Yellow"),
            (0x15, "Light Yellow"),
            (0x16, "Dark Grey"),
            (0x17, "Light Grey"),
            (0x18, "Dark Orange"),
            (0x19, "Light Orange"),
            (0x1A, "Dark Pink"),
            (0x1B, "Light Pink"),
            (0x1C, "Dark Brown"),
            (0x1D, "Light Brown"),
        ];
        for (number, name) in weiler {
            assert_eq!(thread(number).name, name, "{number:#04x}");
        }
        assert_eq!(thread(0x10).name, "Unknown thread 16");
        assert_eq!(COLOURS.iter().flatten().count(), weiler.len());
    }

    /// The header's name keeps printable ASCII and spaces, any other byte read as `_`, up to the
    /// first NUL, its spaces at the ends trimmed.
    #[test]
    fn the_name_keeps_printable_ascii() {
        let name = [b'A', 0x0A, 0xE9, 0x1B, b' ', b' ', 0, 0];
        assert_eq!(read(&hus(&records(), &[3], &name, &Lzh::default())).unwrap().name, "A___");
    }
}
