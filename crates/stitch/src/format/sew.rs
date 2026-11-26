//! Janome SEW: read, never written (the project owner's choice, 2026-11-18). The format of
//! Janome's older machines (Memory Craft 5700, 8000 and 9000, Elna and Kenmore ones), which JEF
//! replaced ([`super::jef`], written).
//!
//! The KDE Community Wiki (*Janome Embroidery Format*) describes SEW from the files it saw: little
//! endian, a header, preview pictures, then JEF's stitch records. The header is the colour count
//! (u16, at most 12), then twelve u16 thread numbers in Janome's JEF chart (the unused ones 0),
//! twelve u16 values whose meaning nobody gives (8, 0 or 255 in every file seen), and twelve u16
//! stitch counts per colour, "minus a few". The pictures follow at fixed places: a 48 × 32 one, one
//! of 24 × 22 per colour, a 64 × 48 one, one of 64 × 48 per colour, and an outline of the design of
//! any size, each its width in bytes, its height and its bits. The stitch records start at the
//! fixed offset 0x1D78 and are JEF's: two signed bytes, x and y in tenths of a millimetre with y
//! upwards, `0x80` starting a command (a colour change, a move with the needle up, the end).
//! EduTechWiki's format overview says the same in a line: SEW names its threads by number, and
//! its stitches are signed x, y pairs with `0x80` starting a command.
//!
//! What reading takes: the colour count, the thread numbers and the records, read as JEF's are
//! ([`super::jef::read_records`]: a move that goes nowhere is a cut). Threads go to blocks as in
//! JEF: each block of records between colour changes takes the list's next thread, so colour
//! changes in a row take one thread and one before the first record takes none; a block of moves
//! alone, with no stitch, takes its thread too, and opening a design leaves such a block out, so
//! its thread isn't seen (the project owner's choice, 2026-11-26; HUS's rule that every colour
//! change moves on one colour is HUS's alone). The pictures and the two tables are left, and
//! opening a file whose pictures or tables aren't blank says so ([`losses`]). Writing another
//! format over any opened SEW asks first, since a design opened from SEW is written in another
//! format, never as SEW. A colour count of 0 or above 12 is no SEW's, and refused. A thread number
//! past Janome's chart reads as a stand-in named after it, as in JEF.
//!
//! No SEW file has been read here: the description rests on the files the KDE wiki's author saw,
//! and the tests on files built from it. pyembroidery 1.5.1 (MIT), run as an outside reader, reads
//! such files with the same needle points; none of its code was read.
//!
//! References: KDE Community Wiki, *Janome Embroidery Format* (community.kde.org/Projects/Liberty/
//! File_Formats/Janome_Embroidery_Format, its SEW section); EduTechWiki, *Embroidery format*
//! (edutechwiki.unige.ch/en/Embroidery_format) and *Embroidery format JEF* (…/Embroidery_format_JEF).

use super::{Loss, Reader, finish, jef};
use crate::thread::{self, Palette};
use crate::{Error, Pattern, Result, Thread};

/// The format's name in errors.
const WHAT: &str = "SEW";
/// The most colours a SEW holds: its header lists twelve thread numbers.
pub(crate) const MAX_COLORS: usize = 12;
/// The header's numbers, the colour count and the thread numbers, are u16, little endian.
const NUMBER_BYTES: usize = 2;
/// Where the thread numbers start, after the colour count.
const THREADS_AT: usize = NUMBER_BYTES;
/// Where the header's two other tables start, after the twelve thread numbers: then the pictures,
/// up to the stitch records. Reading leaves all of it out.
const TABLES_AT: usize = THREADS_AT + MAX_COLORS * NUMBER_BYTES;
/// Where the stitch records start, after the header and the pictures (the KDE wiki: 0x1D78).
pub(crate) const STITCHES_AT: usize = 0x1D78;

/// What a SEW holds that reading it leaves out: its pictures and two tables, when any byte of them
/// isn't 0 (the test kit's files leave them blank; real files draw their pictures). And a file
/// whose records run out before their end record, as a file cut short inside them does, is said
/// to end early, so that it isn't taken for whole and writing over it asks first (the project
/// owner's choice, 2026-11-26; a JEF, whose records are the same, says nothing of it).
pub(crate) fn losses(bytes: &[u8]) -> Vec<Loss> {
    let drawn = bytes.get(TABLES_AT..STITCHES_AT).is_some_and(|b| b.iter().any(|v| *v != 0));
    let cut = bytes.len() > STITCHES_AT && jef::records_cut(bytes, STITCHES_AT);
    [(drawn, Loss::SewPictures), (cut, Loss::EndsEarly)].into_iter().filter_map(|(has, loss)| has.then_some(loss)).collect()
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let mut r = Reader::new(bytes);
    let (Some(count), true) = (r.take(NUMBER_BYTES).map(|b| u16::from_le_bytes([b[0], b[1]])), bytes.len() > STITCHES_AT) else {
        return Err(Error::Format(format!("the {what} file has no stitch records", what = WHAT)));
    };
    let colors = usize::from(count);
    if !(1..=MAX_COLORS).contains(&colors) {
        return Err(Error::Format(format!("not a SEW file: it names {colors} colours, and a SEW file names 1 to {MAX_COLORS}")));
    }
    r.seek(THREADS_AT);
    let mut threads: Vec<Thread> = Vec::with_capacity(colors);
    for block in 0..colors {
        let Some(b) = r.take(NUMBER_BYTES) else { break };
        let number = u16::from_le_bytes([b[0], b[1]]);
        // Janome's chart numbers its threads from 1: 0 and a number past it name none.
        let named = Palette::Jef.thread(usize::from(number));
        threads.push(named.unwrap_or_else(|| thread::unknown_thread(block, u64::from(number))));
    }
    let points = jef::read_records(bytes, STITCHES_AT, WHAT)?;
    if points.is_empty() {
        return Err(Error::Format(format!("the {what} file has no stitch records", what = WHAT)));
    }
    Ok(finish(String::new(), points, &threads))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::{Block, Command, Stitch};
    use vectorcraft_geom::Point;
    use vectorcraft_testkit::machine_files::{SEW_STITCHES_AT, SEW_TABLES_AT, sew, sew_from_jef};

    fn design() -> Pattern {
        let mut p = Pattern::default();
        let mut a = Block::new(Thread::rgb([230, 20, 20]));
        for i in 0..8 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(Point::new(t * 3.0, t * 1.5)));
        }
        let here = a.stitches[7];
        a.stitches.push(Stitch::new(here.x, here.y, Command::Trim));
        a.stitches.push(Stitch::new(here.x + 8.0, here.y, Command::Jump));
        a.stitches.push(Stitch::sew(Point::new(here.x + 8.0, here.y)));
        let mut b = Block::new(Thread::rgb([20, 40, 210]));
        b.stitches.push(Stitch::sew(Point::new(10.0, 20.0)));
        b.stitches.push(Stitch::sew(Point::new(18.0, 23.0)));
        p.blocks.extend([a, b]);
        p
    }

    /// A SEW holding a JEF's thread numbers and records reads as that JEF does: the same blocks,
    /// threads, needle points, cuts and jumps.
    #[test]
    fn a_sew_reads_as_the_jef_it_holds() {
        let jef_bytes = Format::Jef.write(&design()).unwrap();
        let sew_bytes = sew_from_jef(&jef_bytes).unwrap();
        assert_eq!(SEW_STITCHES_AT, STITCHES_AT);
        assert_eq!(SEW_TABLES_AT, TABLES_AT, "the tables start where the KDE wiki puts them");
        let (from_sew, from_jef) = (read(&sew_bytes).unwrap(), Format::Jef.read(&jef_bytes).unwrap());
        assert_eq!(from_sew.blocks, from_jef.blocks);
        assert_eq!(from_sew.points().filter(|s| s.command == Command::Trim).count(), 1);
        assert_eq!(Format::Sew.read(&sew_bytes).unwrap(), from_sew);
    }

    /// Thread numbers name Janome's threads; 0 and numbers past the chart read as stand-ins named
    /// after them; colour changes past the list take the default threads.
    #[test]
    fn thread_numbers_are_janomes() {
        let records = [0x0A, 0x05, 0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x80, 0x10];
        let p = read(&sew(&[2, 0], &records)).unwrap();
        let names: Vec<&str> = p.blocks.iter().map(|b| b.thread.name.as_str()).collect();
        assert_eq!(names.len(), 3);
        assert_eq!(Some(&p.blocks[0].thread), Palette::Jef.thread(2).as_ref());
        assert_eq!(names[1], "Unknown thread 0");
        assert_eq!(p.blocks[2].thread, thread::default_thread(2));
        assert!(read(&sew(&[200], &records)).unwrap().blocks[0].thread.name.contains("200"));
        // Twelve colours, as many as the header lists, are read: the twelfth block in the twelfth
        // number's thread.
        let twelve: Vec<u16> = (1..=12).collect();
        let mut changes = vec![0x0A, 0x05];
        for _ in 1..twelve.len() {
            changes.extend([0x80, 0x01, 0x00, 0x00, 0x0A, 0x05]);
        }
        changes.extend([0x80, 0x10]);
        let p = read(&sew(&twelve, &changes)).unwrap();
        assert_eq!(p.blocks.len(), 12);
        assert_eq!(Some(&p.blocks[11].thread), Palette::Jef.thread(12).as_ref());
    }

    /// Threads go to blocks, as in JEF: colour changes in a row take one thread, so the stitches
    /// after two of them are in the second; a colour change before the first record takes none,
    /// and one after a block of moves alone takes the next (the moves have the first).
    #[test]
    fn colour_changes_in_a_row_take_one_thread() {
        let in_a_row = [0x0A, 0x05, 0x80, 0x01, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x80, 0x10];
        let p = read(&sew(&[1, 2, 3], &in_a_row)).unwrap();
        let threads: Vec<Option<Thread>> = p.blocks.iter().map(|b| Some(b.thread.clone())).collect();
        assert_eq!(threads, [Palette::Jef.thread(1), Palette::Jef.thread(2)]);
        let first = [0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x0A, 0x05, 0x80, 0x10];
        let p = read(&sew(&[1, 2, 3], &first)).unwrap();
        let threads: Vec<Option<Thread>> = p.blocks.iter().map(|b| Some(b.thread.clone())).collect();
        assert_eq!(threads, [Palette::Jef.thread(1)]);
        // A move with the needle up, then a colour change: the move is a block of the first thread.
        let moved = [0x80, 0x02, 0x05, 0x05, 0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x0A, 0x05, 0x80, 0x10];
        let p = read(&sew(&[1, 2, 3], &moved)).unwrap();
        let blocks: Vec<(Vec<Command>, Option<Thread>)> =
            p.blocks.iter().map(|b| (b.stitches.iter().map(|s| s.command).collect(), Some(b.thread.clone()))).collect();
        assert_eq!(blocks, [(vec![Command::Jump], Palette::Jef.thread(1)), (vec![Command::Stitch; 2], Palette::Jef.thread(2))]);
    }

    /// A colour count no SEW has, a file that ends before its records and records that hold no
    /// stitch are refused, saying so; a file cut inside its records reads as far as it goes and
    /// says it ends early, wherever in a record the cut falls, and a file whose records reach
    /// their end, or stop at a command no source gives, doesn't.
    #[test]
    fn what_is_no_sew_is_refused() {
        let records = [0x0A, 0x05, 0x0A, 0x05, 0x80, 0x10];
        for colours in [0usize, 13, 40_000] {
            let mut b = sew(&[1], &records);
            b[..2].copy_from_slice(&(colours as u16).to_le_bytes());
            assert!(read(&b).unwrap_err().to_string().contains("not a SEW file"), "{colours}");
        }
        assert!(read(&sew(&[1], &records)[..SEW_STITCHES_AT]).unwrap_err().to_string().contains("no stitch records"));
        assert!(read(&[]).is_err());
        assert!(read(&sew(&[1], &[0x80, 0x10])).unwrap_err().to_string().contains("no stitch records"));
        let long: Vec<u8> = [0x0Au8, 0x05].repeat(50);
        let ended = [long.as_slice(), &[0x80, 0x10]].concat();
        let whole = read(&sew(&[1], &ended)).unwrap().sewn();
        assert_eq!(losses(&sew(&[1], &ended)), []);
        assert_eq!(read(&sew(&[1], &long[..40])).unwrap().sewn(), whole - 30);
        // Cut between records, inside one, inside a colour change's bytes and a jump's move, and
        // with no end record at all.
        let change = [0x0A, 0x05, 0x80, 0x01, 0x00, 0x00, 0x0A, 0x05, 0x80, 0x02, 0x05, 0x05, 0x0A, 0x05, 0x80, 0x10];
        for cut in [&long[..40], &long[..41], &change[..5], &change[..11], long.as_slice()] {
            assert_eq!(losses(&sew(&[1, 2], cut)), [Loss::EndsEarly], "{cut:02X?}");
        }
        assert_eq!(losses(&sew(&[1, 2], &change)), []);
        assert_eq!(losses(&sew(&[1], &[0x0A, 0x05, 0x80, 0x7F, 0x0A])), [], "a command no source gives, where reading stops");
    }
}
