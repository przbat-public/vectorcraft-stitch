//! Brother PES: a PES header (the design app's data) followed by the PEC section, which is what
//! the machines read; and Brother PEC, the PEC section on its own after the magic `#PEC0001`. The
//! PEC section is the 512-byte PEC header (the label, the icon size and the palette indices), then
//! the stitch block (its own header, then the stitches), then the preview pictures. This module
//! writes the smallest valid PES — a version 1 header that only points at the PEC section — and
//! the whole PEC section, the same in a PEC file, with a 20-byte stitch-block header (the two origin words
//! real Brother files carry; the 16-byte form without them is read too) and blank previews. Every
//! reader except PE-Design itself reads only the PEC section, so a PES written here sews as it
//! should; PE-Design shows it as a blank document (it is the PES part, the editable shapes, that we
//! do not write yet).
//!
//! The PEC header: `LA:` and the label, spaces, `0xFF 0x00`, the icon's size (6 × 38 pixels),
//! spaces, then one byte of colour changes (`0xFF` for none) and one Brother palette index per
//! colour, padded with spaces to 512 bytes. A block's index is the thread chosen for it when it is
//! Brother's, else the thread nearest its colour ([`Palette::numbers`]).
//!
//! The stitch block: two zero bytes, its length as 24 bits (to the end of the stitches, where the
//! previews begin), `0x31 0xFF 0xF0`, the design's width and height in tenths of a millimetre,
//! `0x01E0`, `0x01B0`, in Brother's files the two origin words, then the stitches.
//!
//! A stitch is one or two bytes for x and then for y: a byte below `0x80` is a 7-bit signed delta;
//! otherwise the byte is `1`, three command bits and the top 4 bits of a 12-bit signed delta,
//! whose low 8 bits follow. A jump's command bits are `001`, a trim's `010`. `0xFE 0xB0` followed
//! by `0x02` and `0x01` alternately changes colour, and `0xFF` ends the design. Unlike DST, EXP
//! and JEF, PEC counts y downwards.
//!
//! A PEC file "often begins" with `#PEC0001` (EduTechWiki); one without it is refused rather than
//! guessed at: a PEC header starts with `LA:`, as a Tajima DST header does too, so a file that
//! starts that way may be anything.
//!
//! References: EduTechWiki, *Embroidery format PES* and *Embroidery format PEC*
//! (edutechwiki.unige.ch/en/Embroidery_format_PES, …/Embroidery_format_PEC).

use super::{Frame, LABEL_LEN, PAD, Reader, finish, label, mm, steps};
use crate::thread::{self, Palette};
use crate::{Command, Error, Pattern, Result, Stitch};

/// The PEC header's length: the start of the PEC section, before the stitch block.
const PEC_HEADER: usize = 512;
/// The preview pictures' size: 6 bytes of pixels a line (48 pixels, one bit each), 38 lines.
const ICON_LINE: usize = 6;
const ICON_LINES: usize = 38;
/// One preview picture's bytes.
const PREVIEW: usize = ICON_LINE * ICON_LINES;
/// The largest delta one value holds, as a 12-bit signed number.
pub(crate) const MAX_DELTA: i32 = 2047;
/// The most stitches one stitch block may hold (a stream that lost step stops rather than growing).
const MAX_STITCHES: usize = 1_048_576;
/// The most colours a PEC header holds: it stores one fewer than their count in a byte, and
/// `0xFF` there means none ([`NO_COLORS`]).
pub(crate) const MAX_COLORS: usize = 255;
/// The colour count byte of a design with no colours.
const NO_COLORS: u8 = 0xFF;
/// The PES version 1 header: the magic `#PES0001`, where the PEC section starts (right after this
/// header: byte 22), and the rest zeroed.
const PES_HEADER: [u8; 22] = [b'#', b'P', b'E', b'S', b'0', b'0', b'0', b'1', 22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// Where a PES header gives the PEC section's position (a little-endian word).
const PEC_POSITION: std::ops::Range<usize> = 8..12;
/// A bare PEC file's magic: its PEC header starts right after it.
pub(crate) const PEC_MAGIC: &[u8; 8] = b"#PEC0001";

// The PEC header: `LA:` and the label at its start, the colour count at COLOR_COUNT_AT, then the
// palette indices.

/// Where the label starts, after `LA:`.
const LABEL_AT: usize = 3;
/// Spaces after the label line, and again after the icon size.
const HEADER_GAP: usize = 12;
/// Two bytes the header carries before the icon size, as the format's decodings give them.
const BEFORE_ICON: [u8; 2] = [0xFF, 0x00];
/// Where the colour count byte is.
const COLOR_COUNT_AT: usize = 48;

// The stitch block that follows the PEC header: two zero bytes, the stitch block's length (3
// bytes), a fixed marker, the design's width and height, two fixed values, and in Brother's files
// the design's origin twice; then the stitches. Offsets below count from the stitch block's start.

/// Where the stitch block's length is (3 bytes, little-endian): it counts from the stitch block's
/// start to the end of the stitches, where the preview pictures begin.
const BLOCK_LENGTH_AT: usize = 2;
/// The fixed marker after the stitch block's length, as the format's decodings give it.
const BLOCK_MARKER: [u8; 3] = [0x31, 0xFF, 0xF0];
/// The two fixed values after the design's size, as the format's decodings give them.
const BLOCK_VALUES: [u16; 2] = [0x01E0, 0x01B0];
/// The base the origin words count from (big-endian; ours are always at the origin).
const ORIGIN: u16 = 0x9000;
/// The stitch-block header's length, so where the stitches start: 16 bytes without the origin
/// words (as pyembroidery writes it), 20 with them (as Brother does, and we do).
const SHORT_BLOCK_HEADER: usize = 16;
const LONG_BLOCK_HEADER: usize = 20;

// The stitches: each coordinate a short value (one byte below LONG, a 7-bit signed delta) or a
// long one (two bytes: LONG, three command bits and a 12-bit signed delta).

/// A long value's command bits for a jump (the needle up) and for a trim (the thread cut).
const JUMP: u8 = 0x10;
const TRIM: u8 = 0x20;
/// The top bit of a value's first byte: set, the value is long (two bytes).
const LONG: u8 = 0x80;
/// A long value's command bits (in its first byte).
const FLAGS: u8 = 0x70;
/// A short value's bits, and the first that makes it negative.
const SHORT_BITS: u8 = 0x7F;
const SHORT_NEGATIVE: u8 = 0x40;
/// A long value's 12 bits, and the first that makes it negative.
const LONG_BITS: u32 = 0x0FFF;
const LONG_NEGATIVE: i32 = 0x0800;
/// The deltas written as short values. A short value holds -64 to 63 (7-bit two's complement);
/// 63 is written long all the same, as this writer always has (either form reads the same).
const SHORT_DELTAS: std::ops::Range<i32> = -64..63;
/// A colour change: these two bytes, then [`COLOR_ONE`] and [`COLOR_TWO`] alternately.
const COLOR_CHANGE: [u8; 2] = [0xFE, 0xB0];
const COLOR_ONE: u8 = 0x01;
const COLOR_TWO: u8 = 0x02;
/// The byte that ends the stitches.
const END: u8 = 0xFF;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    after(&PES_HEADER, p)
}

/// A PEC file: [`PEC_MAGIC`] and the PEC section, as a PES carries it.
pub fn write_pec(p: &Pattern) -> Result<Vec<u8>> {
    after(PEC_MAGIC, p)
}

/// `head` (a PES header or PEC's magic), then the PEC section of `p`.
fn after(head: &[u8], p: &Pattern) -> Result<Vec<u8>> {
    let pec = pec_section(p)?;
    let mut out: Vec<u8> = Vec::with_capacity(head.len() + pec.len());
    out.extend_from_slice(head);
    out.extend_from_slice(&pec);
    Ok(out)
}

/// The PEC section: the PEC header, the stitch block and the previews.
fn pec_section(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, false)?;
    let colors = p.blocks.iter().filter(|b| !b.stitches.is_empty()).count();
    if colors == 0 {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    if colors > MAX_COLORS {
        return Err(Error::TooLarge(format!("PEC carries at most {MAX_COLORS} colours, not {colors}")));
    }
    let (width, height) = frame.size();
    // The header, room for the start of the stitch block, and the previews (a guess for the
    // stitches: the buffer grows as needed).
    let mut out: Vec<u8> = Vec::with_capacity(PEC_HEADER + 64 + colors * PREVIEW);
    out.extend_from_slice(&header(p, colors)?);
    let mut block: Vec<u8> = Vec::with_capacity(1024);
    block.extend_from_slice(&[0, 0]);
    // The stitch block's length, filled in below.
    block.extend_from_slice(&[0, 0, 0]);
    block.extend_from_slice(&BLOCK_MARKER);
    block.extend_from_slice(&u16::try_from(width).unwrap_or(u16::MAX).to_le_bytes());
    block.extend_from_slice(&u16::try_from(height).unwrap_or(u16::MAX).to_le_bytes());
    for v in BLOCK_VALUES {
        block.extend_from_slice(&v.to_le_bytes());
    }
    // The design's origin, as a big-endian ORIGIN-based value (0: our designs are normalised to
    // the origin). Real Brother files carry these; pyembroidery's writer omits them, and readers
    // accept both (see `read`).
    block.extend_from_slice(&ORIGIN.to_be_bytes());
    block.extend_from_slice(&ORIGIN.to_be_bytes());
    write_stitches(&mut block, p, &frame)?;
    let length = u32::try_from(block.len()).unwrap_or(u32::MAX);
    if let [_, _, l0, l1, l2, ..] = block.as_mut_slice() {
        *l0 = length as u8;
        *l1 = (length >> 8) as u8;
        *l2 = (length >> 16) as u8;
    }
    out.extend_from_slice(&block);
    // The preview pictures: the whole design and one per colour, drawn blank.
    out.resize(out.len() + (colors + 1) * PREVIEW, 0);
    Ok(out)
}

/// The 512-byte PEC header.
fn header(p: &Pattern, colors: usize) -> Result<Vec<u8>> {
    let name = label(&p.name);
    let mut h: Vec<u8> = Vec::with_capacity(PEC_HEADER);
    h.extend_from_slice(format!("LA:{name:<LABEL_LEN$}\r").as_bytes());
    h.extend_from_slice(&[PAD; HEADER_GAP]);
    h.extend_from_slice(&BEFORE_ICON);
    h.push(ICON_LINE as u8);
    h.push(ICON_LINES as u8);
    h.extend_from_slice(&[PAD; HEADER_GAP]);
    h.push(u8::try_from(colors.saturating_sub(1)).unwrap_or(NO_COLORS));
    let threads = p.blocks.iter().filter(|b| !b.stitches.is_empty()).map(|b| &b.thread);
    for index in Palette::Pec.numbers(threads) {
        // A thread's number is at most the palette's length, which a byte holds.
        h.push(u8::try_from(index).map_err(|_| Error::Format(format!("thread number {index} doesn't fit a PEC header")))?);
    }
    h.resize(PEC_HEADER, PAD);
    Ok(h)
}

fn write_stitches(out: &mut Vec<u8>, p: &Pattern, frame: &Frame) -> Result<()> {
    let mut x = 0i32;
    let mut y = 0i32;
    let mut started: bool;
    let mut wrote_any = false;
    let mut color_two = true;
    for (i, block) in p.blocks.iter().enumerate() {
        if block.stitches.is_empty() {
            continue;
        }
        if i > 0 && wrote_any {
            out.extend_from_slice(&COLOR_CHANGE);
            out.push(if color_two { COLOR_TWO } else { COLOR_ONE });
            color_two = !color_two;
        }
        // Each block starts with the needle up: travel to its first stitch.
        started = false;
        for s in &block.stitches {
            match s.command {
                Command::Stitch => {
                    let (nx, ny) = frame.point(*s);
                    if !started {
                        move_values(out, &mut x, &mut y, nx, ny, JUMP | LONG);
                        started = true;
                    }
                    move_values(out, &mut x, &mut y, nx, ny, 0);
                    wrote_any = true;
                }
                Command::Jump => {
                    let (nx, ny) = frame.point(*s);
                    move_values(out, &mut x, &mut y, nx, ny, JUMP | LONG);
                    started = true;
                }
                Command::Trim => {
                    value(out, 0, TRIM | LONG);
                    value(out, 0, TRIM | LONG);
                }
                Command::ColorChange | Command::Stop => {
                    out.extend_from_slice(&COLOR_CHANGE);
                    out.push(if color_two { COLOR_TWO } else { COLOR_ONE });
                    color_two = !color_two;
                }
            }
        }
    }
    if !wrote_any {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    out.push(END);
    Ok(())
}

fn move_values(out: &mut Vec<u8>, x: &mut i32, y: &mut i32, tx: i32, ty: i32, flags: u8) {
    let (dx, dy) = (tx - *x, ty - *y);
    *x = tx;
    *y = ty;
    if dx == 0 && dy == 0 {
        if flags == 0 {
            out.extend_from_slice(&[0, 0]);
        } else {
            value(out, 0, flags);
            value(out, 0, flags);
        }
        return;
    }
    for (sx, sy) in steps(dx, dy, MAX_DELTA) {
        value(out, sx, flags);
        value(out, sy, flags);
    }
}

/// One PEC coordinate: a 7-bit byte in short form, two bytes in long form.
fn value(out: &mut Vec<u8>, v: i32, flags: u8) {
    if flags == 0 && SHORT_DELTAS.contains(&v) {
        out.push((v & i32::from(SHORT_BITS)) as u8);
        return;
    }
    let bits = (u32::from(flags & FLAGS) << 8) | (v as u32 & LONG_BITS) | (u32::from(LONG) << 8);
    out.push((bits >> 8) as u8);
    out.push(bits as u8);
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let start = pec_start(bytes)?;
    let mut r = Reader::new(bytes);
    r.seek(start + LABEL_AT);
    let name = r.take(LABEL_LEN).map(|b| String::from_utf8_lossy(b).trim().to_string()).unwrap_or_default();
    r.seek(start + COLOR_COUNT_AT);
    let count = r.u8().ok_or_else(|| Error::Format("the PEC header is cut short".into()))?;
    let colors = if count == NO_COLORS { 0 } else { usize::from(count) + 1 };
    let mut threads = Vec::with_capacity(colors);
    for block in 0..colors {
        // 0 and a number past the palette name no thread: a stand-in, never one of its threads.
        let index = r.u8().unwrap_or(0);
        threads.push(Palette::Pec.thread(usize::from(index)).unwrap_or_else(|| thread::unknown_thread(block, u64::from(index))));
    }
    // The stitches start after the stitch-block header: 20 bytes in Brother's files (two origin
    // words after BLOCK_VALUES), 16 in pyembroidery's, which omit them. The stitch block's length
    // says where the stream must end: where the preview pictures begin.
    let block = start.saturating_add(PEC_HEADER);
    // Where the stream must end, when the stitch block's length is there to say.
    let expected_end = {
        let mut r = Reader::new(bytes);
        r.seek(block.saturating_add(BLOCK_LENGTH_AT));
        r.u24().map(|v| block.saturating_add(usize::try_from(v).unwrap_or(usize::MAX)))
    };
    // Which header, guessed from the first byte after the short one: the origin words Brother
    // writes have a high byte of 0x9_ (ORIGIN plus the design's offset). A first jump written as a
    // long value starts with 0x9_ too (LONG and JUMP), so a 16-byte header can be taken for a
    // 20-byte one. When that jump is two long values (4 bytes, as this writer puts it) the stream read
    // from past them stays in step and only the leading jump is lost, which moves nothing: the
    // reader puts the design's corner at the origin anyway. When it is not, the stream loses step
    // and ends elsewhere, and the length sends the reader back to the other start.
    const HIGH_NIBBLE: u8 = 0xF0;
    let origin_high = ORIGIN.to_be_bytes()[0] & HIGH_NIBBLE;
    let long_header = bytes.get(block.saturating_add(SHORT_BLOCK_HEADER)).is_some_and(|b| b & HIGH_NIBBLE == origin_high);
    let candidates = if long_header { [LONG_BLOCK_HEADER, SHORT_BLOCK_HEADER] } else { [SHORT_BLOCK_HEADER, LONG_BLOCK_HEADER] };
    let mut first: Option<Vec<Stitch>> = None;
    let mut points: Option<Vec<Stitch>> = None;
    for begin in candidates {
        let from = block.saturating_add(begin);
        if from >= bytes.len() {
            continue;
        }
        let (parsed, end) = stream(bytes, from);
        if !parsed.is_empty() && expected_end == Some(end) {
            points = Some(parsed);
            break;
        }
        if first.is_none() {
            first = Some(parsed);
        }
    }
    let points = points.or(first).filter(|p| !p.is_empty()).ok_or_else(|| Error::Format("the PEC block has no stitches".into()))?;
    Ok(finish(name, points, &threads))
}

/// The stitches from `from`, and where the [`END`] that ends them lies (just past it).
fn stream(bytes: &[u8], from: usize) -> (Vec<Stitch>, usize) {
    let mut r = Reader::new(bytes);
    r.seek(from);
    let mut points: Vec<Stitch> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    loop {
        let at = r.pos();
        let Some(b0) = r.u8() else { return (points, at) };
        if b0 == END {
            return (points, r.pos());
        }
        if b0 == COLOR_CHANGE[0] {
            // A colour change: COLOR_CHANGE and one byte (COLOR_TWO or COLOR_ONE).
            let Some(next) = r.u8() else { return (points, at) };
            if next != COLOR_CHANGE[1] {
                return (points, at);
            }
            let _ = r.u8();
            points.push(Stitch::new(mm(x), mm(y), Command::ColorChange));
            continue;
        }
        let Some((dx, fx)) = pec_value(&mut r, b0) else { return (points, at) };
        let Some(b1) = r.u8() else { return (points, at) };
        let Some((dy, fy)) = pec_value(&mut r, b1) else { return (points, at) };
        let flags = fx | fy;
        if flags & TRIM != 0 {
            points.push(Stitch::new(mm(x), mm(y), Command::Trim));
            if dx != 0 || dy != 0 {
                x += dx;
                y += dy;
                points.push(Stitch::new(mm(x), mm(y), Command::Jump));
            }
            continue;
        }
        x += dx;
        y += dy;
        let command = if flags & JUMP != 0 { Command::Jump } else { Command::Stitch };
        points.push(Stitch::new(mm(x), mm(y), command));
        if points.len() > MAX_STITCHES {
            return (points, r.pos());
        }
    }
}

/// One PEC coordinate: its value and its command bits.
fn pec_value(r: &mut Reader<'_>, first: u8) -> Option<(i32, u8)> {
    if first & LONG == 0 {
        // Short form: a 7-bit two's complement number, so 64 to 127 are -64 to -1.
        let v = if first >= SHORT_NEGATIVE { i32::from(first) - (i32::from(SHORT_BITS) + 1) } else { i32::from(first) };
        return Some((v, 0));
    }
    let second = r.u8()?;
    let bits = (u32::from(first) << 8) | u32::from(second);
    let raw = (bits & LONG_BITS) as i32;
    let v = if raw >= LONG_NEGATIVE { raw - (LONG_BITS as i32 + 1) } else { raw };
    Some((v, first & FLAGS))
}

/// Where the PEC section starts: a PES header states it; a bare PEC starts after its magic.
fn pec_start(bytes: &[u8]) -> Result<usize> {
    if bytes.starts_with(b"#PEC") {
        return Ok(PEC_MAGIC.len());
    }
    if bytes.starts_with(b"#PES") {
        let Some(b) = bytes.get(PEC_POSITION) else {
            return Err(Error::Format("the PES header is cut short".into()));
        };
        let at = usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(usize::MAX);
        // Checked: on a 32-bit target (wasm32) a position near 4 GB plus the header overflows.
        if at < PEC_POSITION.end || at.checked_add(PEC_HEADER).is_none_or(|end| end > bytes.len()) {
            return Err(Error::Format(format!("the PES header points at byte {at}, past the file's {} bytes", bytes.len())));
        }
        return Ok(at);
    }
    Err(Error::Format("not a PES or PEC file: it does not start with #PES or #PEC".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, Thread};
    use vectorcraft_geom::Point;

    fn design() -> Pattern {
        let mut p = Pattern { name: "Test".into(), ..Pattern::default() };
        let mut a = Block::new(Thread::rgb([220, 30, 30]));
        for i in 0..12 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(Point::new(t * 2.5, t * 1.0)));
        }
        let mut b = Block::new(Thread::rgb([20, 30, 220]));
        b.stitches.push(Stitch::sew(Point::new(5.0, 30.0)));
        b.stitches.push(Stitch::sew(Point::new(25.0, 32.0)));
        p.blocks.extend([a, b]);
        p
    }

    #[test]
    fn a_pes_round_trips() {
        let p = design();
        let bytes = write(&p).expect("writes");
        assert!(bytes.starts_with(b"#PES0001"));
        let back = read(&bytes).expect("reads");
        assert_eq!(back.blocks.len(), 2);
        assert_eq!(back.sewn(), p.sewn());
        assert_eq!(back.name, "Test");
        let a = back.bounds().expect("bounds");
        let b = p.bounds().expect("bounds");
        assert!((a.width() - b.width()).abs() < 0.2, "{a:?} {b:?}");
        assert!((a.height() - b.height()).abs() < 0.2, "{a:?} {b:?}");
        // The stitch-block header is 20 bytes (the two origin words last), the stitch block's
        // length counts from its start, and the previews follow the stitches.
        let block = PES_HEADER.len() + PEC_HEADER;
        let at = block + BLOCK_LENGTH_AT;
        let length = u32::from(bytes[at]) | u32::from(bytes[at + 1]) << 8 | u32::from(bytes[at + 2]) << 16;
        assert_eq!(&bytes[block + SHORT_BLOCK_HEADER..block + LONG_BLOCK_HEADER], &[0x90, 0x00, 0x90, 0x00]);
        assert_eq!(bytes.len(), block + length as usize + 3 * PREVIEW);
    }

    /// A PEC file is the PEC section a PES carries, after its own magic, and reads back the same.
    /// Without the magic it is refused: a DST header starts with `LA:` as a PEC header does.
    #[test]
    fn a_pec_file_is_the_pec_section_of_a_pes() {
        let p = design();
        let (pes, pec) = (write(&p).expect("writes"), write_pec(&p).expect("writes"));
        assert!(pec.starts_with(PEC_MAGIC));
        assert_eq!(&pec[PEC_MAGIC.len()..], &pes[PES_HEADER.len()..]);
        let back = read(&pec).expect("reads");
        assert_eq!(back, read(&pes).expect("reads"));
        assert!(read(&pec[PEC_MAGIC.len()..]).is_err(), "no magic");
        assert!(read(&crate::format::dst::write(&p).expect("writes")).is_err(), "a DST");
    }

    /// Small negative deltas are one-byte short-form values (7-bit two's complement): a design
    /// that zig-zags must come back at the same points, not mirrored or stretched.
    #[test]
    fn negative_short_deltas_keep_their_sign() {
        let mut p = Pattern::default();
        let mut b = Block::new(Thread::rgb([10, 20, 30]));
        for i in 0..20 {
            let t = f64::from(i);
            let x = if i % 2 == 0 { 30.0 + t * 0.5 } else { 30.0 + t * 0.5 - 3.0 };
            b.stitches.push(Stitch::sew(Point::new(x, 10.0 + (t % 3.0))));
        }
        p.blocks.push(b);
        let bytes = write(&p).expect("writes");
        let back = read(&bytes).expect("reads");
        let a = back.blocks[0].stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (s.x, s.y)).collect::<Vec<_>>();
        let want = p.blocks[0].stitches.iter().map(|s| (s.x, s.y)).collect::<Vec<_>>();
        assert_eq!(a.len(), want.len());
        // The reader puts the design's top-left corner at the origin: compare from the first point.
        let (a0, w0) = (a[0], want[0]);
        for (got, want) in a.iter().zip(&want) {
            let (gx, gy) = (got.0 - a0.0, got.1 - a0.1);
            let (wx, wy) = (want.0 - w0.0, want.1 - w0.1);
            assert!((gx - wx).abs() < 0.06 && (gy - wy).abs() < 0.06, "{got:?} vs {want:?}");
        }
    }

    /// A 16-byte stitch-block header whose first jump is a long value looks like the 20-byte one
    /// (its first byte is 0x9_ like the origin words'), so the reader starts 4 bytes too late:
    /// - when the jump is two long values (4 bytes, as we write it), the stream stays in step and
    ///   only the leading jump is lost: the design reads the same;
    /// - when it is not (a long x and a short y: 3 bytes), the stream loses step, ends where the
    ///   stitch block's length says it can't, and the reader goes back to the other start.
    #[test]
    fn a_short_stitch_block_header_that_looks_long_still_reads() {
        let p = design();
        let ours = write(&p).expect("writes");
        let block = PES_HEADER.len() + PEC_HEADER;
        let stitches = block + LONG_BLOCK_HEADER;
        // Our first values: a jump to the first stitch, both coordinates long (0x90 0x00 twice).
        assert_eq!(&ours[stitches..stitches + 4], &[0x90, 0x00, 0x90, 0x00]);
        // `bytes` with the stitch block's length shortened by `cut` bytes.
        let shorten = |bytes: &mut Vec<u8>, cut: u32| {
            let at = block + BLOCK_LENGTH_AT;
            let length = u32::from(bytes[at]) | u32::from(bytes[at + 1]) << 8 | u32::from(bytes[at + 2]) << 16;
            bytes[at..at + 3].copy_from_slice(&(length - cut).to_le_bytes()[..3]);
        };
        let sewn = |pattern: &Pattern| -> Vec<Vec<(i32, i32)>> {
            let q = |v: f64| (v * crate::UNITS_PER_MM).round() as i32;
            pattern.blocks.iter().map(|b| b.stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (q(s.x), q(s.y))).collect()).collect()
        };
        let want = sewn(&read(&ours).expect("reads"));
        // Without the origin words: a 16-byte header, the jump still 4 bytes.
        let mut aligned = ours.clone();
        aligned.drain(stitches - 4..stitches);
        shorten(&mut aligned, 4);
        assert_eq!(aligned[block + SHORT_BLOCK_HEADER], 0x90, "looks like the origin words");
        assert_eq!(sewn(&read(&aligned).expect("reads")), want);
        // And the jump's y written as a short value: a 3-byte first record.
        let mut stepped = aligned.clone();
        stepped.remove(block + SHORT_BLOCK_HEADER + 2);
        shorten(&mut stepped, 1);
        let back = read(&stepped).expect("reads");
        assert_eq!(back.sewn(), p.sewn());
        assert_eq!(sewn(&back), want);
    }

    /// The PEC header names each block's chosen thread when it is Brother's, whatever the block's
    /// colour, and the thread nearest the colour otherwise; read back, the blocks have those
    /// threads.
    #[test]
    fn the_header_names_the_chosen_threads() {
        use crate::thread::PaletteThread;
        let mut p = design();
        p.blocks[0].thread.chosen = PaletteThread::new(Palette::Pec, 37);
        p.blocks[1].thread.chosen = PaletteThread::new(Palette::Jef, 37);
        let bytes = write(&p).expect("writes");
        let at = PES_HEADER.len() + COLOR_COUNT_AT;
        let nearest = Palette::Pec.nearest([20, 30, 220], &[37]).index;
        assert_eq!(&bytes[at..at + 3], &[1, 37, u8::try_from(nearest).unwrap()]);
        let back = read(&bytes).expect("reads");
        assert_eq!((&back.blocks[0].thread, &back.blocks[1].thread), (&Palette::Pec.thread(37).unwrap(), &Palette::Pec.thread(nearest).unwrap()));
    }

    /// A file that names PEC's appliqué entries, as other open-source tools write an orange and a
    /// cream yellow, reads in their colours; written again, its blocks get threads instead.
    #[test]
    fn appliqué_entries_read_in_their_colours_and_are_written_as_threads() {
        let mut bytes = write(&design()).expect("writes");
        let at = PES_HEADER.len() + COLOR_COUNT_AT;
        bytes[at + 1..at + 3].copy_from_slice(&[62, 63]);
        let opened = read(&bytes).expect("reads");
        assert_eq!((&opened.blocks[0].thread, &opened.blocks[1].thread), (&Palette::Pec.thread(62).unwrap(), &Palette::Pec.thread(63).unwrap()));
        let again = write(&opened).expect("writes");
        let threads = [again[at + 1], again[at + 2]].map(usize::from);
        assert!(threads.iter().all(|&t| Palette::Pec.is_thread(t)), "{threads:?}");
        assert_eq!(read(&again).expect("reads").sewn(), opened.sewn());
    }

    /// A header number that names no entry, 0 or past 64, reads as a stand-in thread named after it,
    /// in its block's default colour; 64, an entry, reads as itself.
    #[test]
    fn numbers_that_name_no_thread_read_as_stand_ins() {
        let mut p = design();
        let extra = p.blocks[1].clone();
        p.blocks.extend([extra.clone(), extra]);
        let mut bytes = write(&p).expect("writes");
        let at = PES_HEADER.len() + COLOR_COUNT_AT;
        bytes[at + 1..at + 5].copy_from_slice(&[0, 64, 65, 255]);
        let read = read(&bytes).expect("reads");
        let threads: Vec<&Thread> = read.blocks.iter().map(|b| &b.thread).collect();
        let want = [thread::unknown_thread(0, 0), Palette::Pec.thread(64).unwrap(), thread::unknown_thread(2, 65), thread::unknown_thread(3, 255)];
        assert_eq!(threads, want.iter().collect::<Vec<_>>());
    }

    #[test]
    fn junk_is_a_clear_error_not_a_panic() {
        assert!(read(b"").is_err());
        assert!(read(b"#PES0001").is_err());
        assert!(read(&[0u8; 900]).is_err());
        let mut bytes = write(&design()).expect("writes");
        bytes.truncate(300);
        assert!(read(&bytes).is_err());
        // A PEC section said to start at the largest position a header can give, or just before a
        // header's length from it: past the file, never an overflow.
        for at in [u32::MAX, u32::MAX - 100] {
            let mut bytes = write(&design()).expect("writes");
            bytes[PEC_POSITION].copy_from_slice(&at.to_le_bytes());
            let e = read(&bytes).expect_err("past the file").to_string();
            assert!(e.contains("past the file"), "{e}");
        }
    }
}
