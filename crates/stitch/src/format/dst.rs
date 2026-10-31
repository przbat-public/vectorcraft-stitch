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
//! Happy's TAP is these records alone, without the header ([`super::tap`]): [`records`] and
//! [`read_records`] serve both.
//!
//! Reference: EduTechWiki, *Embroidery format DST* (edutechwiki.unige.ch/en/Embroidery_format_DST),
//! which transcribes the public decodings by Achatina and Embroidermodder.

use super::{Emitter, Frame, LABEL_LEN, PAD, add, finish, label, mm, steps, walk};
use crate::{Command, Error, Pattern, Result, Stitch};

/// The header's length; records follow it.
pub(crate) const HEADER: usize = 512;
/// The byte that ends the header's text, before the padding (ASCII SUB, the old end-of-file mark).
const HEADER_END: u8 = 0x1A;
/// The length of a record.
const RECORD: usize = 3;
/// How many balanced-ternary digits a record holds per axis: worth 1, 3, 9, 27 and 81.
const DIGITS: usize = 5;
/// The largest delta one record holds, in tenths of a millimetre: 1 + 3 + 9 + 27 + 81 = 121, all
/// [`DIGITS`] digits at +1.
pub(crate) const MAX_DELTA: i32 = (3i32.pow(DIGITS as u32) - 1) / 2;
/// The largest extent the header's five-digit `+X`, `-X`, `+Y` and `-Y` fields state, tenths of a
/// millimetre (9,999.9 mm).
pub(crate) const MAX_EXTENT: i32 = 99_999;
/// The most records the header's seven-digit `ST` field counts.
const MAX_RECORDS: usize = 9_999_999;

// A record's third byte: its two bottom bits are always set, and its top bits say what the record
// is. A stitch sets only the bottom two.

/// The bits every record's third byte has set.
const SET_BITS: u8 = 0b0000_0011;
/// A jump's third byte (the top bit and the two bottom ones).
const JUMP_BITS: u8 = 0b1000_0011;
/// A colour change's third byte (the two top bits and the two bottom ones).
const COLOR_CHANGE_BITS: u8 = 0b1100_0011;
/// The end of the design's third byte (the four top bits and the two bottom ones).
const END_BITS: u8 = 0b1111_0011;

/// Where a delta's balanced-ternary digits go, for the digits worth 1, 3, 9, 27 and 81 in turn:
/// the byte of the record (0, 1 or 2), the bit set for a digit of +1 and the bit set for −1. The
/// writer and the reader both work from these tables.
const X_DIGITS: [(usize, u8, u8); DIGITS] = [(0, 0x01, 0x02), (1, 0x01, 0x02), (0, 0x04, 0x08), (1, 0x04, 0x08), (2, 0x04, 0x08)];
const Y_DIGITS: [(usize, u8, u8); DIGITS] = [(0, 0x80, 0x40), (1, 0x80, 0x40), (0, 0x20, 0x10), (1, 0x20, 0x10), (2, 0x20, 0x10)];

/// A trim: three small jumps that go nowhere (0.2 mm up and right, 0.4 mm back, 0.2 mm on), which
/// a machine reads as "cut the thread" ([`TRIM_JUMPS`] is the same convention, read back).
const TRIM_RECORDS: [(i32, i32); 3] = [(2, 2), (-4, -4), (2, 2)];

/// The record's command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Stitch,
    Jump,
    ColorChange,
    End,
}

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let r = records(p)?;
    let mut out = header(p, r.count, r.colors, r.extents);
    out.extend_from_slice(&r.bytes);
    Ok(out)
}

/// A design's records, as the header counts and sizes them.
pub(crate) struct Records {
    /// Every record, the end record last.
    pub(crate) bytes: Vec<u8>,
    /// How many records there are before the end record (the header's `ST`).
    count: usize,
    /// The colour change records (the header's `CO`).
    colors: usize,
    /// How far the design reaches from where it starts, tenths of a millimetre: right, left, up
    /// and down (the header's `+X`, `-X`, `+Y` and `-Y`).
    extents: (i32, i32, i32, i32),
}

/// The frame DST, TAP and T01 count `p`'s points in: from where it starts (or its corner), y
/// counting upwards.
pub(crate) fn frame(p: &Pattern) -> Result<Frame> {
    Frame::of(p, true)
}

/// The records `p` is written as: stitches, jumps, trims as three small jumps, a colour change
/// between blocks, and the end record.
pub(crate) fn records(p: &Pattern) -> Result<Records> {
    let frame = frame(p)?;
    let mut body: Body = Body::default();
    walk(p, &frame, &mut body, "DST")?;
    if body.out.is_empty() {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    if body.records > MAX_RECORDS {
        return Err(Error::TooLarge(format!("a DST header counts at most {MAX_RECORDS} records, and the design takes {}", body.records)));
    }
    let count = body.records;
    let extents = frame.extents();
    body.push(0, 0, Kind::End);
    Ok(Records { bytes: body.out, count, colors: body.colors, extents })
}

/// The header's text lines, its end marker and the padding to [`HEADER`].
fn header(p: &Pattern, records: usize, colors: usize, (right, left, up, down): (i32, i32, i32, i32)) -> Vec<u8> {
    let name = label(&p.name);
    let mut s = String::with_capacity(HEADER);
    // Fixed-width fields: the label, the record count, the colour changes, the four extents and
    // the position data, as the format's decodings give them.
    s.push_str(&format!("LA:{name:<LABEL_LEN$}\r"));
    s.push_str(&format!("ST:{records:7}\r"));
    s.push_str(&format!("CO:{colors:3}\r"));
    s.push_str(&format!("+X:{right:5}\r"));
    s.push_str(&format!("-X:{left:5}\r"));
    s.push_str(&format!("+Y:{up:5}\r"));
    s.push_str(&format!("-Y:{down:5}\r"));
    s.push_str(&format!("AX:+{:5}\r", 0));
    s.push_str(&format!("AY:+{:5}\r", 0));
    s.push_str(&format!("MX:+{:5}\r", 0));
    s.push_str(&format!("MY:+{:5}\r", 0));
    s.push_str(&format!("PD:{:6}\r", "******"));
    let mut out = s.into_bytes();
    out.push(HEADER_END);
    out.resize(HEADER, PAD);
    out
}

/// The records of a design being written.
#[derive(Default)]
struct Body {
    out: Vec<u8>,
    /// Where the frame is, in tenths of a millimetre (the file's own x and y).
    x: i32,
    y: i32,
    records: usize,
    /// The colour change records written (the header's `CO`): between blocks, and a stop or a
    /// change inside one.
    colors: usize,
}

impl Emitter for Body {
    fn stitch(&mut self, x: i32, y: i32) {
        self.move_to(x, y, Kind::Stitch);
    }

    fn jump(&mut self, x: i32, y: i32) {
        self.move_to(x, y, Kind::Jump);
    }

    fn trim(&mut self) {
        // Three small jumps that go nowhere: a machine trims after a run of jumps.
        for (dx, dy) in TRIM_RECORDS {
            self.push(dx, dy, Kind::Jump);
        }
    }

    /// A colour change record, between blocks or a stop inside one: the header's `CO` counts
    /// every one.
    fn color_change(&mut self) {
        self.push(0, 0, Kind::ColorChange);
        self.colors += 1;
    }

    fn next_block(&mut self) {
        self.color_change();
    }

    /// Any record: a trim or a stop too.
    fn wrote(&self) -> bool {
        !self.out.is_empty()
    }

    fn size(&self) -> usize {
        self.out.len()
    }
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
fn encode(dx: i32, dy: i32, kind: Kind) -> [u8; RECORD] {
    let mut record = [0, 0, SET_BITS];
    match kind {
        Kind::ColorChange => return [0, 0, COLOR_CHANGE_BITS],
        Kind::End => return [0, 0, END_BITS],
        Kind::Jump => record[2] = JUMP_BITS,
        Kind::Stitch => {}
    }
    for (digits, layout) in [(balanced(dx), &X_DIGITS), (balanced(dy), &Y_DIGITS)] {
        for (&digit, &(byte, plus, minus)) in digits.iter().zip(layout) {
            if let Some(b) = record.get_mut(byte) {
                set(b, digit, plus, minus);
            }
        }
    }
    record
}

fn set(byte: &mut u8, digit: i32, plus: u8, minus: u8) {
    match digit {
        1 => *byte |= plus,
        -1 => *byte |= minus,
        _ => {}
    }
}

/// `v` in balanced ternary, digits for ±1, ±3, ±9, ±27 and ±81 (the largest is 121).
fn balanced(mut v: i32) -> [i32; DIGITS] {
    let mut d = [0i32; DIGITS];
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
    read_records(bytes.get(HEADER..).unwrap_or_default(), header_name(bytes.get(..HEADER).unwrap_or_default()), "DST")
}

/// The header fields a DST reader leaves out, and the writer writes as zeros: the needle's end
/// position (`AX`, `AY`) and the multi-volume fields (`MX`, `MY`, `PD`).
const END_FIELDS: [&str; 4] = ["AX:", "AY:", "MX:", "MY:"];
const MULTI_VOLUME: &str = "PD:";
/// What `PD` holds in a design that is not one of several volumes.
const NO_VOLUME: char = '*';

/// What a DST's header holds that reading it leaves out: an end position or multi-volume fields
/// that aren't zero (or, for `PD`, asterisks).
pub(crate) fn losses(bytes: &[u8]) -> Vec<super::Loss> {
    let header = String::from_utf8_lossy(bytes.get(..HEADER).unwrap_or_default()).to_string();
    let field = |name: &str| header.split('\r').find_map(|line| line.trim_start_matches('\n').strip_prefix(name).map(str::trim));
    let set = END_FIELDS.iter().filter_map(|f| field(f)).any(|v| v.trim_start_matches(['+', '-']).trim().parse::<i64>().is_ok_and(|n| n != 0))
        || field(MULTI_VOLUME).is_some_and(|v| !v.chars().all(|c| c == NO_VOLUME || c == ' ' || c == '0'));
    if set { vec![super::Loss::DstFields] } else { Vec::new() }
}

/// Whether `records` are DST records: every one up to the end record has [`SET_BITS`] in its
/// third byte, as every record does. A TAP or T01, with no header, is told from another file by
/// this alone; `what` names the format in the error.
pub(crate) fn check_records(records: &[u8], what: &str) -> Result<()> {
    for (k, record) in records.as_chunks::<RECORD>().0.iter().enumerate() {
        let b2 = record.get(2).copied().unwrap_or(0);
        if b2 & SET_BITS != SET_BITS {
            return Err(Error::Format(format!("not a {what} file: its record {} is not a DST record", k + 1)));
        }
        if b2 & END_BITS == END_BITS {
            break;
        }
    }
    Ok(())
}

/// The design `records` hold (a DST's after its header, or a whole TAP file), named `name`; `what`
/// names the format in an error.
pub(crate) fn read_records(records: &[u8], name: String, what: &str) -> Result<Pattern> {
    // Wide enough that no file overflows it: each record moves the needle at most 121 tenths.
    let (mut x, mut y) = (0i64, 0i64);
    let mut points: Vec<Stitch> = Vec::new();
    for record in records.as_chunks::<RECORD>().0 {
        let b2 = record[2];
        let (dx, dy) = (i64::from(decode(record, &X_DIGITS)), i64::from(decode(record, &Y_DIGITS)));
        if b2 & END_BITS == END_BITS {
            break;
        }
        if b2 & COLOR_CHANGE_BITS == COLOR_CHANGE_BITS {
            add(&mut points, Stitch::new(mm(x), mm(-y), Command::ColorChange), what)?;
            // Some writers move the frame in the colour-change record itself: the move is a jump
            // to where the next thread starts.
            if dx != 0 || dy != 0 {
                x += dx;
                y += dy;
                add(&mut points, Stitch::new(mm(x), mm(-y), Command::Jump), what)?;
            }
            continue;
        }
        x += dx;
        y += dy;
        let command = if b2 & JUMP_BITS == JUMP_BITS { Command::Jump } else { Command::Stitch };
        add(&mut points, Stitch::new(mm(x), mm(-y), command), what)?;
    }
    if points.is_empty() {
        return Err(Error::Format(format!("the {what} file has no stitch records")));
    }
    // DST has no trim records: a run of jumps between stitches is a cut.
    insert_trims(&mut points);
    Ok(finish(name, points, &[]))
}

/// The label from the header's first line (`LA:`), `""` when it has none.
fn header_name(header: &[u8]) -> String {
    let Some(at) = header.windows(3).position(|w| w == b"LA:") else { return String::new() };
    let rest = &header[at + 3..];
    let end = rest.iter().position(|b| *b == b'\r' || *b == HEADER_END).unwrap_or(rest.len());
    String::from_utf8_lossy(&rest[..end]).trim().to_string()
}

/// The delta a record's bytes carry along one axis, by that axis's digit layout ([`X_DIGITS`] or
/// [`Y_DIGITS`]).
fn decode(record: &[u8], layout: &[(usize, u8, u8); DIGITS]) -> i32 {
    let mut v = 0;
    let mut weight = 1;
    for &(byte, plus, minus) in layout {
        let b = record.get(byte).copied().unwrap_or(0);
        if b & plus != 0 {
            v += weight;
        }
        if b & minus != 0 {
            v -= weight;
        }
        weight *= 3;
    }
    v
}

/// The fewest jumps in a row that a machine reads as a cut: the three that go nowhere writers
/// put down for one, as [`TRIM_RECORDS`] writes it (a travel of one or two records is just a move
/// with the needle up).
const TRIM_JUMPS: usize = TRIM_RECORDS.len();

/// A trim before every run of at least [`TRIM_JUMPS`] jumps that sits between stitches, and every
/// run collapsed to the point the thread is taken up at (its last jump: runs are maximal, so no two
/// jumps end up side by side).
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
    *points = out;
}

#[cfg(test)]
mod tests {
    use vectorcraft_geom::Point;

    use super::*;
    use crate::{Block, Thread};

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
                assert_eq!(decode(&[b0, b1, b2], &X_DIGITS), dx, "{dx},{dy}");
                assert_eq!(decode(&[b0, b1, b2], &Y_DIGITS), dy, "{dx},{dy}");
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

    /// The header's `ST`, `CO` and extents are the records': as many records before the end
    /// record, as many colour change records (between blocks, and a stop inside one), and the
    /// farthest the records take the needle from where it starts, each way.
    #[test]
    fn the_header_counts_and_measures_the_records() {
        let mut a = Block::new(Thread::rgb([200, 0, 0]));
        for (x, y) in [(0.0, 0.0), (5.0, 2.0), (10.0, -3.0)] {
            a.stitches.push(Stitch::sew(Point::new(x, y)));
        }
        // A stop inside the block, a trim and a 30 mm jump.
        a.stitches.push(Stitch::new(10.0, -3.0, Command::Stop));
        a.stitches.push(Stitch::sew(Point::new(12.0, 4.0)));
        a.stitches.push(Stitch::new(12.0, 4.0, Command::Trim));
        a.stitches.push(Stitch::new(42.0, 4.0, Command::Jump));
        a.stitches.push(Stitch::sew(Point::new(42.0, 4.0)));
        let mut b = Block::new(Thread::rgb([0, 0, 200]));
        b.stitches.push(Stitch::sew(Point::new(40.0, 20.0)));
        b.stitches.push(Stitch::sew(Point::new(30.0, 25.0)));
        let p = Pattern { blocks: vec![a, b], ..Pattern::default() };
        let bytes = write(&p).unwrap();
        let header = String::from_utf8_lossy(&bytes[..HEADER]).to_string();
        let field = |name: &str| -> i64 {
            let at = header.find(name).unwrap() + name.len();
            header[at..].split('\r').next().unwrap().trim().trim_start_matches('+').trim().parse().unwrap()
        };
        let (mut records, mut changes) = (0i64, 0i64);
        let (mut x, mut y) = (0i64, 0i64);
        let (mut x0, mut x1, mut y0, mut y1) = (0i64, 0i64, 0i64, 0i64);
        for r in bytes[HEADER..].as_chunks::<RECORD>().0 {
            if r[2] & END_BITS == END_BITS {
                break;
            }
            records += 1;
            if r[2] & COLOR_CHANGE_BITS == COLOR_CHANGE_BITS {
                changes += 1;
                continue;
            }
            x += i64::from(decode(r, &X_DIGITS));
            y += i64::from(decode(r, &Y_DIGITS));
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        assert_eq!((field("ST:"), field("CO:")), (records, changes));
        assert_eq!(changes, 2, "the block change and the stop");
        assert_eq!((field("+X:"), field("-X:"), field("+Y:"), field("-Y:")), (x1, -x0, y1, -y0));
    }
}
