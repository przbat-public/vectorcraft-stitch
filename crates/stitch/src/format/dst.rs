//! Tajima DST: a 512-byte text header, then 3-byte records.
//!
//! The header is ASCII lines of a two-letter key, a colon and a value, each ending in a carriage
//! return: `LA` the label, `ST` the record count, `CO` the colour changes, the four extents in
//! tenths of a millimetre, `AX`/`AY`/`MX`/`MY` (position data machines no longer need) and `PD`.
//! It is padded with spaces to 512 bytes; readers skip it and recompute everything from the
//! records.
//!
//! A record is three bytes. Bytes one and two carry the x and y deltas as sums of ±1, ±3, ±9,
//! ±27 and ±81 (the largest move is 121 tenths of a millimetre, 12.1 mm, per record, so longer
//! moves are written as several records). Byte three's two top bits are the command: `00` a
//! stitch, `10` a jump, `11` a colour change and `1111` (with both of its lower bits set) the
//! end of the design. Its two bottom bits are always set.
//!
//! DST has no colours and no trim: a thread is cut by three small jumps that go nowhere, which a
//! machine reads as "cut", and a reader infers a trim from a run of three or more jumps between
//! stitches (one or two are a move with the needle up). Y counts
//! upwards.
//!
//! Reference: EduTechWiki, *Embroidery format DST* (edutechwiki.unige.ch/en/Embroidery_format_DST),
//! which transcribes the public decodings by Achatina and Embroidermodder.

use super::{Frame, collapse_jumps, finish, steps};
use crate::{Block, Command, Error, Pattern, Result, Stitch};

/// The header's length; records follow it.
const HEADER: usize = 512;
/// The largest delta one record holds, in tenths of a millimetre.
const MAX_DELTA: i32 = 121;

/// The record's command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Stitch,
    Jump,
    ColorChange,
    End,
}

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, true)?;
    let mut body: Body = Body::default();
    let mut colors = 0usize;
    for (i, block) in p.blocks.iter().enumerate() {
        if block.stitches.is_empty() {
            continue;
        }
        if i > 0 && !body.out.is_empty() {
            body.push(0, 0, Kind::ColorChange);
            colors += 1;
        }
        // Each block starts with the needle up: travel to its first stitch.
        body.started = false;
        write_block(&mut body, block, &frame)?;
    }
    if body.out.is_empty() {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    let records = body.records;
    let (width, height) = extent(p, &frame);
    let mut out = header(p, records, colors, width, height);
    body.push(0, 0, Kind::End);
    out.extend_from_slice(&body.out);
    Ok(out)
}

fn write_block(body: &mut Body, block: &Block, frame: &Frame) -> Result<()> {
    for s in &block.stitches {
        match s.command {
            Command::Stitch => {
                let (x, y) = frame.point(*s);
                if !body.started {
                    // The design starts at the origin: travel to its first stitch with the
                    // needle up so no thread runs from there.
                    body.move_to(x, y, Kind::Jump);
                    body.started = true;
                }
                body.move_to(x, y, Kind::Stitch);
            }
            Command::Jump => {
                let (x, y) = frame.point(*s);
                body.move_to(x, y, Kind::Jump);
                body.started = true;
            }
            Command::Trim => {
                // Three small jumps that go nowhere: a machine trims after a run of jumps.
                for (dx, dy) in [(2, 2), (-4, -4), (2, 2)] {
                    body.push(dx, dy, Kind::Jump);
                }
            }
            Command::ColorChange | Command::Stop => body.push(0, 0, Kind::ColorChange),
            Command::End => break,
        }
    }
    Ok(())
}

/// The header's text lines, its end marker and the padding to [`HEADER`].
fn header(p: &Pattern, records: usize, colors: usize, width: i32, height: i32) -> Vec<u8> {
    let name: String = p.name.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' }).take(16).collect();
    let mut s = String::with_capacity(128);
    s.push_str(&format!("LA:{name:<16}\r"));
    s.push_str(&format!("ST:{records:7}\r"));
    s.push_str(&format!("CO:{colors:3}\r"));
    s.push_str(&format!("+X:{width:5}\r"));
    s.push_str(&format!("-X:{:5}\r", 0));
    s.push_str(&format!("+Y:{height:5}\r"));
    s.push_str(&format!("-Y:{:5}\r", 0));
    s.push_str(&format!("AX:+{:5}\r", 0));
    s.push_str(&format!("AY:+{:5}\r", 0));
    s.push_str(&format!("MX:+{:5}\r", 0));
    s.push_str(&format!("MY:+{:5}\r", 0));
    s.push_str(&format!("PD:{:6}\r", "******"));
    let mut out = s.into_bytes();
    out.push(0x1A);
    out.resize(HEADER, 0x20);
    out
}

/// The design's size in tenths of a millimetre.
fn extent(p: &Pattern, frame: &Frame) -> (i32, i32) {
    match p.bounds() {
        Some(b) => frame.size(b),
        None => (0, 0),
    }
}

/// The records of a design being written.
#[derive(Default)]
struct Body {
    out: Vec<u8>,
    /// Where the frame is, in tenths of a millimetre (the file's own x and y).
    x: i32,
    y: i32,
    records: usize,
    started: bool,
}

impl Body {
    fn push(&mut self, dx: i32, dy: i32, kind: Kind) {
        let [b0, b1, b2] = encode(dx, dy, kind);
        self.out.extend_from_slice(&[b0, b1, b2]);
        self.x += dx;
        self.y += dy;
        self.records += 1;
    }

    fn move_to(&mut self, x: i32, y: i32, kind: Kind) {
        let (dx, dy) = (x - self.x, y - self.y);
        if dx == 0 && dy == 0 {
            if kind == Kind::Stitch {
                self.push(0, 0, Kind::Stitch);
            }
            return;
        }
        for (sx, sy) in steps(dx, dy, MAX_DELTA) {
            self.push(sx, sy, kind);
        }
    }
}

/// The three bytes of one record.
fn encode(dx: i32, dy: i32, kind: Kind) -> [u8; 3] {
    match kind {
        Kind::ColorChange => return [0x00, 0x00, 0xC3],
        Kind::End => return [0x00, 0x00, 0xF3],
        Kind::Stitch | Kind::Jump => {}
    }
    let x = balanced(dx);
    let y = balanced(dy);
    let mut b0 = 0u8;
    let mut b1 = 0u8;
    let mut b2 = 0b0000_0011u8;
    set(&mut b0, x[0], 0x01, 0x02);
    set(&mut b1, x[1], 0x01, 0x02);
    set(&mut b0, x[2], 0x04, 0x08);
    set(&mut b1, x[3], 0x04, 0x08);
    set(&mut b2, x[4], 0x04, 0x08);
    set(&mut b0, y[0], 0x80, 0x40);
    set(&mut b1, y[1], 0x80, 0x40);
    set(&mut b0, y[2], 0x20, 0x10);
    set(&mut b1, y[3], 0x20, 0x10);
    set(&mut b2, y[4], 0x20, 0x10);
    if kind == Kind::Jump {
        b2 |= 0x80;
    }
    [b0, b1, b2]
}

fn set(byte: &mut u8, digit: i32, plus: u8, minus: u8) {
    match digit {
        1 => *byte |= plus,
        -1 => *byte |= minus,
        _ => {}
    }
}

/// `v` in balanced ternary, digits for ±1, ±3, ±9, ±27 and ±81 (the largest is 121).
fn balanced(mut v: i32) -> [i32; 5] {
    let mut d = [0i32; 5];
    for slot in &mut d {
        let r = v.rem_euclid(3);
        let r = if r == 2 { -1 } else { r };
        *slot = r;
        v = (v - r) / 3;
    }
    d
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    if bytes.len() <= HEADER {
        return Err(Error::Format(format!(
            "a DST file has a {HEADER}-byte header and at least one 3-byte record, but this file is {} bytes",
            bytes.len()
        )));
    }
    let name = header_name(&bytes[..HEADER]);
    let mut x = 0i32;
    let mut y = 0i32;
    let mut points: Vec<Stitch> = Vec::new();
    for record in bytes[HEADER..].chunks_exact(3) {
        let (b0, b1, b2) = (record[0], record[1], record[2]);
        let (dx, dy) = (decode_x(b0, b1, b2), decode_y(b0, b1, b2));
        if b2 & 0b1111_0011 == 0b1111_0011 {
            break;
        }
        if b2 & 0b1100_0011 == 0b1100_0011 {
            points.push(Stitch::new(f64::from(x) / 10.0, f64::from(-y) / 10.0, Command::ColorChange));
            // Some writers move the frame in the colour-change record itself: the move is a jump
            // to where the next thread starts.
            if dx != 0 || dy != 0 {
                x += dx;
                y += dy;
                points.push(Stitch::new(f64::from(x) / 10.0, f64::from(-y) / 10.0, Command::Jump));
            }
            continue;
        }
        x += dx;
        y += dy;
        let command = if b2 & 0b1000_0011 == 0b1000_0011 { Command::Jump } else { Command::Stitch };
        points.push(Stitch::new(f64::from(x) / 10.0, f64::from(-y) / 10.0, command));
    }
    if points.is_empty() {
        return Err(Error::Format("the DST file has no stitch records".into()));
    }
    // DST has no trim records: a run of jumps between stitches is a cut.
    insert_trims(&mut points);
    Ok(finish(name, points, &[]))
}

/// The label from the header's first line (`LA:`), `""` when it has none.
fn header_name(header: &[u8]) -> String {
    let Some(at) = header.windows(3).position(|w| w == b"LA:") else { return String::new() };
    let rest = &header[at + 3..];
    let end = rest.iter().position(|b| *b == b'\r' || *b == 0x1A).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).trim().to_string()
}

fn decode_x(b0: u8, b1: u8, b2: u8) -> i32 {
    let bit = |b: u8, n: u8| i32::from((b >> n) & 1);
    bit(b2, 2) * 81 - bit(b2, 3) * 81 + bit(b1, 2) * 27 - bit(b1, 3) * 27 + bit(b0, 2) * 9 - bit(b0, 3) * 9 + bit(b1, 0) * 3 - bit(b1, 1) * 3
        + bit(b0, 0)
        - bit(b0, 1)
}

fn decode_y(b0: u8, b1: u8, b2: u8) -> i32 {
    let bit = |b: u8, n: u8| i32::from((b >> n) & 1);
    bit(b2, 5) * 81 - bit(b2, 4) * 81 + bit(b1, 5) * 27 - bit(b1, 4) * 27 + bit(b0, 5) * 9 - bit(b0, 4) * 9 + bit(b1, 7) * 3 - bit(b1, 6) * 3
        + bit(b0, 7)
        - bit(b0, 6)
}

/// The fewest jumps in a row that a machine reads as a cut: the three that go nowhere writers
/// put down for one (a travel of one or two records is just a move with the needle up).
const TRIM_JUMPS: usize = 3;

/// A trim before every run of at least [`TRIM_JUMPS`] jumps that sits between stitches, and every
/// run collapsed to the point the thread is taken up at.
fn insert_trims(points: &mut Vec<Stitch>) {
    let mut out: Vec<Stitch> = Vec::with_capacity(points.len() + 4);
    let mut i = 0;
    while i < points.len() {
        let is_jump = points.get(i).is_some_and(|s| s.command == Command::Jump);
        if is_jump {
            let mut end = i;
            while points.get(end).is_some_and(|s| s.command == Command::Jump) {
                end += 1;
            }
            let after_stitch = out.last().is_some_and(|s| s.command == Command::Stitch);
            let before_stitch = points.get(end).is_some_and(|s| s.command == Command::Stitch);
            if after_stitch
                && before_stitch
                && end - i >= TRIM_JUMPS
                && let Some(last) = out.last().copied()
            {
                out.push(Stitch::new(last.x, last.y, Command::Trim));
            }
            if let Some(last) = points.get(end.saturating_sub(1)).copied() {
                out.push(last);
            }
            i = end;
            continue;
        }
        if let Some(s) = points.get(i).copied() {
            out.push(s);
        }
        i += 1;
    }
    collapse_jumps(&mut out);
    *points = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_ternary_covers_the_range() {
        for v in -121..=121 {
            let d = balanced(v);
            let sum = d[0] + 3 * d[1] + 9 * d[2] + 27 * d[3] + 81 * d[4];
            assert_eq!(sum, v, "{v} -> {d:?}");
        }
    }

    #[test]
    fn a_record_round_trips_every_delta() {
        for dx in [-121, -100, -27, -9, -3, -1, 0, 1, 3, 9, 27, 100, 121] {
            for dy in [-121, -83, -12, -1, 0, 1, 12, 83, 121] {
                let [b0, b1, b2] = encode(dx, dy, Kind::Stitch);
                assert_eq!(decode_x(b0, b1, b2), dx, "{dx},{dy}");
                assert_eq!(decode_y(b0, b1, b2), dy, "{dx},{dy}");
            }
        }
    }

    /// A file of `records` behind a blank header.
    fn file(records: &[[u8; 3]]) -> Vec<u8> {
        let mut b = vec![0x20u8; HEADER];
        for r in records {
            b.extend_from_slice(r);
        }
        b.extend_from_slice(&[0, 0, 0xF3]);
        b
    }

    /// One or two jumps are a move with the needle up; three or more between stitches are a cut.
    #[test]
    fn only_a_run_of_jumps_is_a_trim() {
        let stitch = encode(10, 0, Kind::Stitch);
        let jump = encode(20, 0, Kind::Jump);
        let trims = |jumps: usize| {
            let mut r = vec![stitch, stitch];
            r.extend(std::iter::repeat_n(jump, jumps));
            r.extend([stitch, stitch]);
            read(&file(&r)).expect("reads").points().filter(|s| s.command == Command::Trim).count()
        };
        assert_eq!((trims(1), trims(2), trims(3), trims(5)), (0, 0, 1, 1));
    }

    /// A colour change that also moves the frame: the next thread starts where it moved to.
    #[test]
    fn a_colour_change_can_carry_a_move() {
        let stitch = encode(10, 0, Kind::Stitch);
        let [b0, b1, _] = encode(30, 0, Kind::Stitch);
        let p = read(&file(&[stitch, stitch, [b0, b1, 0xC3], stitch, stitch])).expect("reads");
        assert_eq!(p.blocks.len(), 2);
        let first = p.blocks[1].stitches.iter().find(|s| s.command == Command::Stitch).expect("a stitch");
        // From the first stitch: 1 mm, the colour change's 3 mm, then 1 mm.
        assert!((first.x - p.blocks[0].stitches[0].x - 5.0).abs() < 1e-9, "{first:?}");
    }

    #[test]
    fn commands_are_their_own_bytes() {
        assert_eq!(encode(0, 0, Kind::ColorChange), [0, 0, 0xC3]);
        assert_eq!(encode(0, 0, Kind::End), [0, 0, 0xF3]);
        let [_, _, b2] = encode(0, 0, Kind::Jump);
        assert_eq!(b2, 0x83);
        let [_, _, b2] = encode(0, 0, Kind::Stitch);
        assert_eq!(b2, 0x03);
    }
}
