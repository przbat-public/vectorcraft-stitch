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
//! upwards, `0x80` starting a command (a colour change to the list's next thread, a move with the
//! needle up, the end). EduTechWiki's format overview says the same in a line: SEW names its
//! threads by number, and its stitches are signed x, y pairs with `0x80` starting a command.
//!
//! What reading takes: the colour count, the thread numbers and the records, read as JEF's are
//! ([`super::jef::read_records`]: a move that goes nowhere is a cut). The pictures and the counts
//! are left: a design opened from SEW is written in another format, never as SEW. A colour count
//! of 0 or above 12 is no SEW's, and refused. A thread number past Janome's chart reads as a
//! stand-in named after it, as in JEF.
//!
//! No SEW file has been read here: the description rests on the files the KDE wiki's author saw,
//! and the tests on files built from it. pyembroidery 1.5.1 (MIT), run as an outside reader, reads
//! such files with the same needle points; none of its code was read.
//!
//! References: KDE Community Wiki, *Janome Embroidery Format* (community.kde.org/Projects/Liberty/
//! File_Formats/Janome_Embroidery_Format, its SEW section); EduTechWiki, *Embroidery format*
//! (edutechwiki.unige.ch/en/Embroidery_format) and *Embroidery format JEF* (…/Embroidery_format_JEF).

use super::{Reader, finish, jef};
use crate::thread::{self, Palette};
use crate::{Error, Pattern, Result, Thread};

/// The format's name in errors.
const WHAT: &str = "SEW";
/// The most colours a SEW holds: its header lists twelve thread numbers.
pub(crate) const MAX_COLORS: usize = 12;
/// Where the thread numbers start, after the colour count (u16 each).
const THREADS_AT: usize = 2;
/// Where the stitch records start, after the header and the pictures (the KDE wiki: 0x1D78).
pub(crate) const STITCHES_AT: usize = 0x1D78;

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let mut r = Reader::new(bytes);
    let (Some(count), true) = (r.take(2).map(|b| u16::from_le_bytes([b[0], b[1]])), bytes.len() > STITCHES_AT) else {
        return Err(Error::Format(format!("the {what} file has no stitch records", what = WHAT)));
    };
    let colors = usize::from(count);
    if !(1..=MAX_COLORS).contains(&colors) {
        return Err(Error::Format(format!("not a SEW file: it names {colors} colours, and a SEW file names 1 to {MAX_COLORS}")));
    }
    r.seek(THREADS_AT);
    let mut threads: Vec<Thread> = Vec::with_capacity(colors);
    for block in 0..colors {
        let Some(b) = r.take(2) else { break };
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
    use vectorcraft_testkit::machine_files::{SEW_STITCHES_AT, sew, sew_from_jef};

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
    }

    /// A colour count no SEW has, a file that ends before its records and records that hold no
    /// stitch are refused, saying so; a file cut inside its records reads as far as it goes.
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
        let whole = read(&sew(&[1], &long)).unwrap().sewn();
        assert_eq!(read(&sew(&[1], &long[..40])).unwrap().sewn(), whole - 30);
    }
}
