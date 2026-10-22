//! Happy TAP and Wilcom T01: Tajima DST's 3-byte records without DST's 512-byte header
//! (EduTechWiki: each "contains DST stitches with no header", and a DST with its first 512 bytes
//! deleted is one). The two are the same bytes under two names.
//!
//! The records are DST's ([`super::dst`]): x and y deltas of up to 121 tenths of a millimetre as
//! balanced-ternary bits, a jump, a colour change and the end in the third byte, y counting
//! upwards, a trim written as three small jumps that go nowhere. With no header there is no name,
//! no count and no size: a reader takes the records as they come, and a TAP or a T01 is recognised
//! by its extension. A file whose records aren't DST's (the third byte of every DST record has its
//! two lowest bits set) is refused, so that another file with the extension, a tape image say,
//! isn't read as stitches.
//!
//! References: EduTechWiki, *Embroidery format* (edutechwiki.unige.ch/en/Embroidery_format) and
//! *Embroidery format DST* (…/Embroidery_format_DST). pyembroidery 1.5.1 (MIT) served only as an
//! outside reader of the files written here; none of its code was read.

use super::dst;
use crate::{Pattern, Result};

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    Ok(dst::records(p)?.bytes)
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    read_named(bytes, "TAP")
}

/// A T01 file, read as a TAP is ([`read`]); its errors name T01.
pub fn read_t01(bytes: &[u8]) -> Result<Pattern> {
    read_named(bytes, "T01")
}

/// DST's records alone, read as a design; `what` names the format in the errors.
fn read_named(bytes: &[u8], what: &str) -> Result<Pattern> {
    dst::check_records(bytes, what)?;
    dst::read_records(bytes, String::new(), what)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::{Block, Command, Stitch, Thread};
    use vectorcraft_geom::Point;

    fn design() -> Pattern {
        let mut p = Pattern { name: "Tap".into(), ..Pattern::default() };
        let mut a = Block::new(Thread::rgb([200, 0, 0]));
        for i in 0..10 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(Point::new(t * 3.0, (t * 1.7) % 5.0)));
        }
        let here = a.stitches[4];
        a.stitches.insert(5, Stitch::new(here.x, here.y, Command::Trim));
        // Near enough that no stitch around it is longer than a record holds (DST splits those).
        a.stitches.insert(6, Stitch::new(here.x + 2.0, here.y, Command::Jump));
        a.stitches.insert(7, Stitch::new(here.x + 2.0, here.y, Command::Stitch));
        let mut b = Block::new(Thread::rgb([0, 0, 200]));
        b.stitches.push(Stitch::sew(Point::new(5.0, 20.0)));
        b.stitches.push(Stitch::sew(Point::new(9.0, 22.0)));
        p.blocks.extend([a, b]);
        p
    }

    /// A TAP is a DST without its header: the same records, read back the same, apart from the
    /// name only the header holds.
    #[test]
    fn a_tap_is_a_dst_without_its_header() {
        let p = design();
        let (tap, dst) = (write(&p).expect("writes"), Format::Dst.write(&p).expect("writes"));
        assert_eq!(tap.as_slice(), &dst[512..]);
        let back = read(&tap).expect("reads");
        let from_dst = Format::Dst.read(&dst).expect("reads");
        assert_eq!(back.blocks, from_dst.blocks);
        assert_eq!((back.name.as_str(), from_dst.name.as_str()), ("", "Tap"));
        assert_eq!(back.points().filter(|s| s.command == Command::Trim).count(), 1);
        assert_eq!(back.sewn(), p.sewn());
    }

    /// A T01 is a TAP under another name: the same bytes, read back the same.
    #[test]
    fn a_t01_is_a_tap() {
        let p = design();
        let (t01, tap) = (Format::T01.write(&p).expect("writes"), write(&p).expect("writes"));
        assert_eq!(t01, tap);
        assert_eq!(read_t01(&t01).expect("reads"), read(&tap).expect("reads"));
        assert!(read_t01(&[]).expect_err("nothing").to_string().contains("T01"));
    }

    /// Bytes that aren't DST records are refused, whatever follows: a tape image's first block
    /// (its length, then a flag byte of 0) has no record's set bits.
    #[test]
    fn bytes_that_are_not_dst_records_are_refused() {
        let tape = [0x13, 0x00, 0x00, 0x03, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46];
        let e = read(&tape).expect_err("not records").to_string();
        assert!(e.contains("not a TAP file"), "{e}");
        assert!(read_t01(&tape).expect_err("not records").to_string().contains("not a T01 file"));
        // Records after the end are not checked.
        let mut tap = write(&design()).expect("writes");
        tap.extend_from_slice(&[0x13, 0x00, 0x00]);
        assert!(read(&tap).is_ok());
    }

    /// Bytes that are no whole record are left over, not read; no records at all is an error.
    #[test]
    fn a_partial_record_is_left_over_and_nothing_is_an_error() {
        let mut tap = write(&design()).expect("writes");
        let whole = read(&tap).expect("reads");
        tap.push(0x1A);
        assert_eq!(read(&tap).expect("reads"), whole);
        assert!(read(&[]).is_err());
        assert!(read(&[0, 0]).is_err());
    }
}
