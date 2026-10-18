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

use super::{Frame, Reader, finish, steps};
use crate::thread;
use crate::{Command, Error, Pattern, Result, Stitch};

/// The PEC block's header length.
const PEC_HEADER: usize = 512;
/// One preview picture: 6 bytes of pixels per line, 38 lines.
const PREVIEW: usize = 6 * 38;
/// The largest delta one value holds, as a 12-bit signed number.
const MAX_DELTA: i32 = 2047;
/// The most stitches one block may hold (a desynchronised stream stops rather than growing).
const MAX_STITCHES: usize = 1_048_576;
/// The PES version 1 header: the magic, where the PEC block starts, and the rest zeroed.
const PES_HEADER: [u8; 22] = [0x23, 0x50, 0x45, 0x53, 0x30, 0x30, 0x30, 0x31, 0x16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

const JUMP: u8 = 0x10;
const TRIM: u8 = 0x20;
const LONG: u8 = 0x80;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, false)?;
    let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
    let colors = p.blocks.iter().filter(|b| !b.stitches.is_empty()).count();
    if colors == 0 {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    if colors > 255 {
        return Err(Error::TooLarge(format!("PEC carries at most 255 colours, not {colors}")));
    }
    let (width, height) = frame.size(b);
    let mut out: Vec<u8> = Vec::with_capacity(PES_HEADER.len() + PEC_HEADER + 64 + colors * PREVIEW);
    out.extend_from_slice(&PES_HEADER);
    out.extend_from_slice(&header(p, colors));
    let block_start = out.len();
    let mut block: Vec<u8> = Vec::with_capacity(1024);
    block.extend_from_slice(&[0, 0]);
    block.extend_from_slice(&[0, 0, 0]);
    block.extend_from_slice(&[0x31, 0xFF, 0xF0]);
    block.extend_from_slice(&u16::try_from(width).unwrap_or(u16::MAX).to_le_bytes());
    block.extend_from_slice(&u16::try_from(height).unwrap_or(u16::MAX).to_le_bytes());
    block.extend_from_slice(&0x01E0u16.to_le_bytes());
    block.extend_from_slice(&0x01B0u16.to_le_bytes());
    // The design's origin, as a big-endian 0x9000-based value (0: our designs are normalised to
    // the origin). Real Brother files carry these; pyembroidery's writer omits them, and readers
    // accept both (see `read`).
    block.extend_from_slice(&0x9000u16.to_be_bytes());
    block.extend_from_slice(&0x9000u16.to_be_bytes());
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
    let name: String = p.name.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' }).take(16).collect();
    let mut h: Vec<u8> = Vec::with_capacity(PEC_HEADER);
    h.extend_from_slice(format!("LA:{name:<16}\r").as_bytes());
    h.extend_from_slice(&[0x20; 12]);
    h.extend_from_slice(&[0xFF, 0x00]);
    h.push(6);
    h.push(38);
    h.extend_from_slice(&[0x20; 12]);
    h.push(u8::try_from(colors.saturating_sub(1)).unwrap_or(0xFF));
    let mut last: Option<usize> = None;
    for block in p.blocks.iter().filter(|b| !b.stitches.is_empty()) {
        let index = thread::nearest(&thread::PEC, block.thread.color, last);
        last = Some(index);
        h.push(u8::try_from(index).unwrap_or(1));
    }
    h.resize(PEC_HEADER, 0x20);
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
            out.extend_from_slice(&[0xFE, 0xB0, if color_two { 0x02 } else { 0x01 }]);
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
                    out.extend_from_slice(&[0xFE, 0xB0, if color_two { 0x02 } else { 0x01 }]);
                    color_two = !color_two;
                }
                Command::End => break,
            }
        }
    }
    if !wrote_any {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    out.push(0xFF);
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
    if flags == 0 && (-64..63).contains(&v) {
        out.push((v & 0x7F) as u8);
        return;
    }
    let bits = (u32::from(flags & 0x70) << 8) | (v as u32 & 0x0FFF) | 0x8000;
    out.push((bits >> 8) as u8);
    out.push(bits as u8);
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let start = pec_start(bytes)?;
    let mut r = Reader::new(bytes);
    r.seek(start + 3);
    let name = r.take(16).map(|b| String::from_utf8_lossy(b).trim().to_string()).unwrap_or_default();
    r.seek(start + 48);
    let count = r.u8().ok_or_else(|| Error::Format("the PEC header is cut short".into()))?;
    let colors = if count == 0xFF { 0 } else { usize::from(count) + 1 };
    let mut threads = Vec::with_capacity(colors);
    for _ in 0..colors {
        let index = r.u8().unwrap_or(0);
        threads.push(thread::pec_thread(usize::from(index)));
    }
    // The stitches start after the block header. Brother writes a 20-byte one (two origin words
    // after 0x01E0/0x01B0); pyembroidery writes 16 bytes and omits them. The graphics offset at
    // PEC+0x202 tells which: the stream ends exactly where the preview pictures begin.
    let (g, have_g) = {
        let mut g = Reader::new(bytes);
        g.seek(start + 2);
        match g.u24() {
            Some(v) => (usize::try_from(v).unwrap_or(usize::MAX), true),
            None => (0, false),
        }
    };
    let expected_end = start.saturating_add(0x200).saturating_add(g);
    // Which block header: the two origin words Brother writes have a high byte of 0x90 (they are
    // 0x9000 with the design's offset), and then the stitches start at 532; without them (as
    // pyembroidery writes) they start at 528. The graphics offset confirms it: the stream ends
    // exactly where the preview pictures begin.
    let long_header = bytes.get(start + 0x210).is_some_and(|b| b & 0xF0 == 0x90);
    let candidates = if long_header { [0x214usize, 0x210] } else { [0x210, 0x214] };
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

/// The stitches from `from`, and where the `0xFF` that ends them lies (just past it).
fn stream(bytes: &[u8], from: usize) -> (Vec<Stitch>, usize) {
    let mut r = Reader::new(bytes);
    r.seek(from);
    let mut points: Vec<Stitch> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    loop {
        let at = r.pos();
        let Some(b0) = r.u8() else { return (points, at) };
        if b0 == 0xFF {
            return (points, r.pos());
        }
        if b0 == 0xFE {
            // A colour change: 0xFE 0xB0 and one byte (0x02 or 0x01).
            let Some(next) = r.u8() else { return (points, at) };
            if next != 0xB0 {
                return (points, at);
            }
            let _ = r.u8();
            points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::ColorChange));
            continue;
        }
        let Some((dx, fx)) = pec_value(&mut r, b0) else { return (points, at) };
        let Some(b1) = r.u8() else { return (points, at) };
        let Some((dy, fy)) = pec_value(&mut r, b1) else { return (points, at) };
        let flags = fx | fy;
        if flags & TRIM != 0 {
            points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::Trim));
            if dx != 0 || dy != 0 {
                x += dx;
                y += dy;
                points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::Jump));
            }
            continue;
        }
        x += dx;
        y += dy;
        let command = if flags & JUMP != 0 { Command::Jump } else { Command::Stitch };
        points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, command));
        if points.len() > MAX_STITCHES {
            return (points, r.pos());
        }
    }
}

/// One PEC coordinate: its value and its command bits.
fn pec_value(r: &mut Reader<'_>, first: u8) -> Option<(i32, u8)> {
    if first & LONG == 0 {
        // Short form: a 7-bit two's complement number, so 64 to 127 are -64 to -1.
        let v = if first > 0x3F { i32::from(first) - 0x80 } else { i32::from(first) };
        return Some((v, 0));
    }
    let second = r.u8()?;
    let bits = (u32::from(first) << 8) | u32::from(second);
    let raw = (bits & 0x0FFF) as i32;
    let v = if raw > 0x07FF { raw - 0x1000 } else { raw };
    Some((v, first & 0x70))
}

/// Where the PEC block starts: a PES header states it; a bare PEC starts after its magic.
fn pec_start(bytes: &[u8]) -> Result<usize> {
    if bytes.starts_with(b"#PEC") {
        return Ok(8);
    }
    if bytes.starts_with(b"#PES") {
        let Some(b) = bytes.get(8..12) else {
            return Err(Error::Format("the PES header is cut short".into()));
        };
        let at = usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(usize::MAX);
        if at < 12 || at + PEC_HEADER > bytes.len() {
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
