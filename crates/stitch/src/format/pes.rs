//! Brother PES: a PES header (the design app's data) followed by the PEC block, which is what
//! the machines read. This module writes the smallest valid PES — a version 1 header that only
//! points at the PEC block — and the whole PEC block: its 512-byte header (the label, the icon
//! size and the palette indices), a 16-byte block header, the stitches and blank preview
//! pictures. Every reader except PE-Design itself reads only the PEC block, so a PES written
//! here sews as it should; PE-Design shows it as a blank document (it is the PES part, the
//! editable shapes, that we do not write yet).
//!
//! The PEC header: `LA:` and the label, spaces, `0xFF 0x00`, the icon's size (6 × 38 pixels),
//! spaces, then one byte of colour changes (`0xFF` for none) and one Brother palette index per
//! colour ([`crate::thread::PEC`]), padded with spaces to 512 bytes.
//!
//! The block: two zero bytes, the block's length as 24 bits, `0x31 0xFF 0xF0`, the design's width
//! and height in tenths of a millimetre, `0x01E0`, `0x01B0`, then the stitches and the previews.
//!
//! A stitch is one or two bytes for x and then for y: a byte below `0x80` is a 7-bit signed delta;
//! otherwise the byte is `1`, three command bits and the top 4 bits of a 12-bit signed delta,
//! whose low 8 bits follow. A jump's command bits are `001`, a trim's `010`. `0xFE 0xB0` followed
//! by `0x02` and `0x01` alternately changes colour, and `0xFF` ends the design. Unlike DST, EXP
//! and JEF, PEC counts y downwards.
//!
//! References: EduTechWiki, *Embroidery format PES* and *Embroidery format PEC*
//! (edutechwiki.unige.ch/en/Embroidery_format_PES, …/Embroidery_format_PEC).

use super::{Frame, LABEL_LEN, PAD, Reader, finish, label, mm, steps};
use crate::thread;
use crate::{Command, Error, Pattern, Result, Stitch};

/// The PEC block's header length.
const PEC_HEADER: usize = 512;
/// The preview pictures' size: 6 bytes of pixels a line (48 pixels, one bit each), 38 lines.
const ICON_LINE: usize = 6;
const ICON_LINES: usize = 38;
/// One preview picture's bytes.
const PREVIEW: usize = ICON_LINE * ICON_LINES;
/// The largest delta one value holds, as a 12-bit signed number.
const MAX_DELTA: i32 = 2047;
/// The most stitches one block may hold (a desynchronised stream stops rather than growing).
const MAX_STITCHES: usize = 1_048_576;
/// The most colours a PEC header holds: it stores one fewer than their count in a byte, and
/// `0xFF` there means none ([`NO_COLORS`]).
const MAX_COLORS: usize = 255;
/// The colour count byte of a design with no colours.
const NO_COLORS: u8 = 0xFF;
/// The PES version 1 header: the magic `#PES0001`, where the PEC block starts (right after this
/// header: byte 22), and the rest zeroed.
const PES_HEADER: [u8; 22] = [b'#', b'P', b'E', b'S', b'0', b'0', b'0', b'1', 22, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// Where a PES header gives the PEC block's position (a little-endian word).
const PEC_POSITION: std::ops::Range<usize> = 8..12;
/// A bare PEC file's magic (`#PEC0001`): its block starts right after it.
const PEC_MAGIC_LEN: usize = 8;

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

// The PEC block that follows the header: two zero bytes, the block's length (3 bytes), a fixed
// marker, the design's width and height, two fixed values, and in Brother's files the design's
// origin twice; then the stitches.

/// Where the block's length is, from the PEC header's start (3 bytes, little-endian): it counts
/// from the block's start to the end of the stitches, where the preview pictures begin.
const BLOCK_LENGTH_AT: usize = PEC_HEADER + 2;
/// The fixed marker after the block's length, as the format's decodings give it.
const BLOCK_MARKER: [u8; 3] = [0x31, 0xFF, 0xF0];
/// The two fixed values after the design's size, as the format's decodings give them.
const BLOCK_VALUES: [u16; 2] = [0x01E0, 0x01B0];
/// The base the origin words count from (big-endian; ours are always at the origin).
const ORIGIN: u16 = 0x9000;
/// Where the stitches start, from the PEC header's start: after a 16-byte block header (without
/// the origin words, as pyembroidery writes it) or a 20-byte one (with them, as Brother does).
const SHORT_BLOCK_STITCHES: usize = PEC_HEADER + 16;
const LONG_BLOCK_STITCHES: usize = PEC_HEADER + 20;

// The stitches: each coordinate a short value (one byte below LONG, a 7-bit signed delta) or a
// long one (two bytes: LONG, three command bits and a 12-bit signed delta).

const JUMP: u8 = 0x10;
const TRIM: u8 = 0x20;
const LONG: u8 = 0x80;
/// A long value's command bits (in its first byte).
const FLAGS: u8 = 0x70;
/// A short value's bits, and the first that makes it negative.
const SHORT_BITS: u8 = 0x7F;
const SHORT_NEGATIVE: u8 = 0x40;
/// A long value's 12 bits, and the first that makes it negative.
const LONG_BITS: u32 = 0x0FFF;
const LONG_NEGATIVE: i32 = 0x0800;
/// The deltas written as short values (63 itself is written long; either form reads the same).
const SHORT_DELTAS: std::ops::Range<i32> = -64..63;
/// A colour change: these two bytes, then [`COLOR_ONE`] and [`COLOR_TWO`] alternately.
const COLOR_CHANGE: [u8; 2] = [0xFE, 0xB0];
const COLOR_ONE: u8 = 0x01;
const COLOR_TWO: u8 = 0x02;
/// The byte that ends the stitches.
const END: u8 = 0xFF;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, false)?;
    let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
    let colors = p.blocks.iter().filter(|b| !b.stitches.is_empty()).count();
    if colors == 0 {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    if colors > MAX_COLORS {
        return Err(Error::TooLarge(format!("PEC carries at most {MAX_COLORS} colours, not {colors}")));
    }
    let (width, height) = frame.size(b);
    // The headers, room for the start of the block, and the previews (a guess for the stitches:
    // the buffer grows as needed).
    let mut out: Vec<u8> = Vec::with_capacity(PES_HEADER.len() + PEC_HEADER + 64 + colors * PREVIEW);
    out.extend_from_slice(&PES_HEADER);
    out.extend_from_slice(&header(p, colors));
    let block_start = out.len();
    let mut block: Vec<u8> = Vec::with_capacity(1024);
    block.extend_from_slice(&[0, 0]);
    // The block's length, filled in below.
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
    debug_assert_eq!(out.len(), block_start);
    out.extend_from_slice(&block);
    // The preview pictures: the whole design and one per colour, drawn blank.
    out.resize(out.len() + (colors + 1) * PREVIEW, 0);
    Ok(out)
}

/// The 512-byte PEC header.
fn header(p: &Pattern, colors: usize) -> Vec<u8> {
    let name = label(&p.name);
    let mut h: Vec<u8> = Vec::with_capacity(PEC_HEADER);
    h.extend_from_slice(format!("LA:{name:<LABEL_LEN$}\r").as_bytes());
    h.extend_from_slice(&[PAD; HEADER_GAP]);
    h.extend_from_slice(&BEFORE_ICON);
    h.push(ICON_LINE as u8);
    h.push(ICON_LINES as u8);
    h.extend_from_slice(&[PAD; HEADER_GAP]);
    h.push(u8::try_from(colors.saturating_sub(1)).unwrap_or(NO_COLORS));
    let mut last: Option<usize> = None;
    for block in p.blocks.iter().filter(|b| !b.stitches.is_empty()) {
        let index = thread::nearest(&thread::PEC, block.thread.color, last);
        last = Some(index);
        h.push(u8::try_from(index).unwrap_or(1));
    }
    h.resize(PEC_HEADER, PAD);
    h
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
                Command::End => break,
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
    for _ in 0..colors {
        let index = r.u8().unwrap_or(0);
        threads.push(thread::pec_thread(usize::from(index)));
    }
    // The stitches start after the block header. Brother writes a 20-byte one (two origin words
    // after BLOCK_VALUES); pyembroidery writes 16 bytes and omits them. The block's length tells
    // which: the stream ends exactly where the block does, and the preview pictures begin.
    let (g, have_g) = {
        let mut g = Reader::new(bytes);
        g.seek(start + BLOCK_LENGTH_AT);
        match g.u24() {
            Some(v) => (usize::try_from(v).unwrap_or(usize::MAX), true),
            None => (0, false),
        }
    };
    let expected_end = start.saturating_add(PEC_HEADER).saturating_add(g);
    // Which block header, first guessed: the two origin words Brother writes have a high byte of
    // 0x90 (they are ORIGIN with the design's offset), and then the stitches start at
    // LONG_BLOCK_STITCHES; without them (as pyembroidery writes) at SHORT_BLOCK_STITCHES. A first
    // jump written long looks like them too; read from the wrong start the stream loses step and
    // ends elsewhere, so the block's length decides.
    let [origin_high, _] = ORIGIN.to_be_bytes();
    // The top four bits are the origin base's; the bottom ones carry the design's offset.
    let long_header = bytes.get(start + SHORT_BLOCK_STITCHES).is_some_and(|b| b & 0xF0 == origin_high);
    let candidates = if long_header { [LONG_BLOCK_STITCHES, SHORT_BLOCK_STITCHES] } else { [SHORT_BLOCK_STITCHES, LONG_BLOCK_STITCHES] };
    let mut first: Option<Vec<Stitch>> = None;
    let mut points: Option<Vec<Stitch>> = None;
    for begin in candidates {
        let from = start.saturating_add(begin);
        if from >= bytes.len() {
            continue;
        }
        let (parsed, end) = stream(bytes, from);
        if !parsed.is_empty() && have_g && end == expected_end {
            points = Some(parsed);
            break;
        }
        if first.is_none() {
            first = Some(parsed);
        }
    }
    let Some(points) = points.or(first) else {
        return Err(Error::Format("the PEC block has no stitches".into()));
    };
    if points.is_empty() {
        return Err(Error::Format("the PEC block has no stitches".into()));
    }
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

/// Where the PEC block starts: a PES header states it; a bare PEC starts after its magic.
fn pec_start(bytes: &[u8]) -> Result<usize> {
    if bytes.starts_with(b"#PEC") {
        return Ok(PEC_MAGIC_LEN);
    }
    if bytes.starts_with(b"#PES") {
        let Some(b) = bytes.get(PEC_POSITION) else {
            return Err(Error::Format("the PES header is cut short".into()));
        };
        let at = usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(usize::MAX);
        if at < PEC_POSITION.end || at + PEC_HEADER > bytes.len() {
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
        // The block header is 20 bytes (the two origin words included), the graphics offset
        // counts from PEC+0x200, and the previews follow the stitches.
        let pec = 22;
        let at = pec + 0x202;
        let g = u32::from(bytes[at]) | u32::from(bytes[at + 1]) << 8 | u32::from(bytes[at + 2]) << 16;
        assert_eq!(&bytes[pec + 0x210..pec + 0x214], &[0x90, 0x00, 0x90, 0x00]);
        assert_eq!(bytes.len(), pec + 0x200 + g as usize + 3 * PREVIEW);
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

    /// Which block header a file has (16 bytes, or 20 with the origin words) is first guessed from
    /// the byte where the stitches would start, and then checked: the stream must end where the
    /// block's length says. A 16-byte header whose first jump starts with a long value looks like
    /// the origin words; read from past them the stream loses step, and the check sends the reader
    /// back to the right start.
    #[test]
    fn the_block_length_decides_where_the_stitches_start() {
        let p = design();
        let mut bytes = write(&p).expect("writes");
        let pec = PES_HEADER.len();
        let stitches = pec + LONG_BLOCK_STITCHES;
        // Our first values: a jump to the first stitch, both coordinates long (0x90 0x00 twice).
        assert_eq!(&bytes[stitches..stitches + 4], &[0x90, 0x00, 0x90, 0x00]);
        // Drop the origin words (a 16-byte header), and write the jump's y as a short value.
        bytes.drain(stitches - 4..stitches);
        bytes.remove(pec + SHORT_BLOCK_STITCHES + 2);
        let block = pec + PEC_HEADER;
        let length = u32::from(bytes[block + 2]) | u32::from(bytes[block + 3]) << 8 | u32::from(bytes[block + 4]) << 16;
        let length = (length - 5).to_le_bytes();
        bytes[block + 2..block + 5].copy_from_slice(&length[..3]);
        assert_eq!(bytes[pec + SHORT_BLOCK_STITCHES], 0x90, "looks like the origin words");
        let back = read(&bytes).expect("reads");
        assert_eq!(back.sewn(), p.sewn());
        let (a, b) = (back.bounds().expect("bounds"), p.bounds().expect("bounds"));
        assert!((a.width() - b.width()).abs() < 0.2 && (a.height() - b.height()).abs() < 0.2, "{a:?} {b:?}");
    }

    #[test]
    fn junk_is_a_clear_error_not_a_panic() {
        assert!(read(b"").is_err());
        assert!(read(b"#PES0001").is_err());
        assert!(read(&[0u8; 900]).is_err());
        let mut bytes = write(&design()).expect("writes");
        bytes.truncate(300);
        assert!(read(&bytes).is_err());
    }
}
