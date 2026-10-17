//! Janome JEF: a fixed 116-byte header (plus two 4-byte arrays per thread colour) and 2-byte
//! records.
//!
//! The header gives where the records start (`0x74 + 8 × colours`), a date, the version letter,
//! the colour count, the record count, the hoop, the design's size and distances from the hoop
//! edges, then one palette index per colour and a `0x0D` word per colour.
//!
//! A record is two signed bytes, the x and y deltas in tenths of a millimetre (at most 127). The
//! byte `0x80` starts a command: `0x80 0x02 dx dy` jumps, and a jump of zero length is a trim
//! (Janome's own way of cutting); `0x80 0x01 dx dy` changes colour; `0x80 0x10` ends the
//! design. Y counts upwards. Colours are indices into Janome's 78-thread chart
//! ([`crate::thread::JEF`]).
//!
//! Reference: EduTechWiki, *Embroidery format JEF* (edutechwiki.unige.ch/en/Embroidery_format_JEF).

use super::{Frame, Reader, finish, steps};
use crate::thread;
use crate::{Command, Error, Pattern, Result, Stitch};

/// The header's fixed length: everything before the per-colour arrays.
const HEADER: usize = 0x74;
/// The largest delta one record holds, in tenths of a millimetre.
const MAX_DELTA: i32 = 127;
const CONTROL: u8 = 0x80;
const END: u8 = 0x10;
const JUMP: u8 = 0x02;
const COLOR_CHANGE: u8 = 0x01;
/// A trim is three jumps that go nowhere: Janome's machines cut after a run of them.
const TRIM_JUMPS: usize = 3;

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, true)?;
    let b = p.bounds().ok_or_else(|| Error::Format("the design has no points to write".into()))?;
    let colors = p.blocks.iter().filter(|b| !b.stitches.is_empty()).count();
    if colors == 0 {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    let width = frame.x(b.x1).max(0);
    let height = frame.y(b.y0).max(0);
    let records = record_count(p);
    let offset = HEADER + 8 * colors;
    let mut out: Vec<u8> = Vec::with_capacity(offset + records * 2);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&0x14u32.to_le_bytes());
    out.extend_from_slice(b"00000000000000");
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(colors as u32).to_le_bytes());
    out.extend_from_slice(&(records as u32).to_le_bytes());
    out.extend_from_slice(&hoop(width, height).to_le_bytes());
    let (hw, hh) = (width / 2, height / 2);
    for v in [hw, hh, hw, hh] {
        out.extend_from_slice(&(v.max(0) as u32).to_le_bytes());
    }
    // The distance from each of the four hoop edges (a negative margin: -1, it does not fit).
    for (x, y) in [(550, 550), (250, 250), (700, 1000), (700, 1000)].map(|(x, y): (i32, i32)| (x - hw, y - hh)) {
        let fit = x >= 0 && y >= 0;
        for v in if fit { [x, y, x, y] } else { [-1, -1, -1, -1] } {
            out.extend_from_slice(&(v as u32).to_le_bytes());
        }
    }
    // Janome's palette indices, one per block (a thread change needs a different index).
    let mut last: Option<usize> = None;
    for block in p.blocks.iter().filter(|b| !b.stitches.is_empty()) {
        let index = thread::nearest(&thread::JEF, block.thread.color, last);
        last = Some(index);
        out.extend_from_slice(&(index as u32).to_le_bytes());
    }
    for _ in 0..colors {
        out.extend_from_slice(&0x0Du32.to_le_bytes());
    }
    write_body(&mut out, p, &frame);
    Ok(out)
}

/// The number of 2-byte records the body holds (the header's `points length / 2`).
fn record_count(p: &Pattern) -> usize {
    let mut n = 1; // the end record
    for s in p.points() {
        n += match s.command {
            Command::Stitch => 1,
            Command::Jump => 2,
            Command::ColorChange | Command::Stop => 2,
            Command::Trim => 2 * TRIM_JUMPS,
            Command::End => break,
        };
    }
    n
}

fn write_body(out: &mut Vec<u8>, p: &Pattern, frame: &Frame) {
    let mut x = 0i32;
    let mut y = 0i32;
    let mut started: bool;
    let mut wrote_any = false;
    for (i, block) in p.blocks.iter().enumerate() {
        if block.stitches.is_empty() {
            continue;
        }
        if i > 0 && wrote_any {
            out.extend_from_slice(&[CONTROL, COLOR_CHANGE, 0, 0]);
        }
        // Each block starts with the needle up: travel to its first stitch.
        started = false;
        for s in &block.stitches {
            match s.command {
                Command::Stitch => {
                    let (nx, ny) = frame.point(*s);
                    if !started {
                        write_move(out, &mut x, &mut y, nx, ny, Some(JUMP));
                        started = true;
                    }
                    write_move(out, &mut x, &mut y, nx, ny, None);
                    wrote_any = true;
                }
                Command::Jump => {
                    let (nx, ny) = frame.point(*s);
                    write_move(out, &mut x, &mut y, nx, ny, Some(JUMP));
                    started = true;
                }
                Command::Trim => {
                    for _ in 0..TRIM_JUMPS {
                        out.extend_from_slice(&[CONTROL, JUMP, 0, 0]);
                    }
                }
                Command::ColorChange | Command::Stop => out.extend_from_slice(&[CONTROL, COLOR_CHANGE, 0, 0]),
                Command::End => break,
            }
        }
    }
    out.extend_from_slice(&[CONTROL, END]);
}

fn write_move(out: &mut Vec<u8>, x: &mut i32, y: &mut i32, tx: i32, ty: i32, control: Option<u8>) {
    let (dx, dy) = (tx - *x, ty - *y);
    *x = tx;
    *y = ty;
    if dx == 0 && dy == 0 {
        if control.is_none() {
            out.extend_from_slice(&[0, 0]);
        }
        return;
    }
    for (sx, sy) in steps(dx, dy, MAX_DELTA) {
        let (bx, by) = ((sx as i8) as u8, (sy as i8) as u8);
        match control {
            Some(c) => out.extend_from_slice(&[CONTROL, c, bx, by]),
            None => out.extend_from_slice(&[bx, by]),
        }
    }
}

/// The hoop the design fits: 50×50, 126×110, 140×200 or 200×200 (in tenths of a millimetre),
/// else the 110×110 default, as Janome numbers them.
fn hoop(width: i32, height: i32) -> u32 {
    let (w, h) = (u32::try_from(width).unwrap_or(u32::MAX), u32::try_from(height).unwrap_or(u32::MAX));
    if w < 500 && h < 500 {
        1
    } else if w < 1260 && h < 1100 {
        3
    } else if w < 1400 && h < 2000 {
        2
    } else if w < 2000 && h < 2000 {
        4
    } else {
        0
    }
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let mut r = Reader::new(bytes);
    let offset = r.u32().ok_or_else(|| short(bytes.len()))? as usize;
    r.seek(24);
    let colors = r.u32().ok_or_else(|| short(bytes.len()))? as usize;
    if offset < HEADER || offset >= bytes.len() {
        return Err(Error::Format(format!("the JEF header points at byte {offset}, past the file's {} bytes", bytes.len())));
    }
    r.seek(HEADER);
    let mut threads = Vec::with_capacity(colors);
    for _ in 0..colors {
        let index = r.u32().ok_or_else(|| short(bytes.len()))? as usize % 79;
        threads.push(thread::jef_thread(index));
    }
    r.seek(offset);
    let mut points: Vec<Stitch> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    while let Some(b0) = r.u8() {
        let Some(b1) = r.u8() else { break };
        if b0 != CONTROL {
            x += i32::from(b0 as i8);
            y -= i32::from(b1 as i8);
            points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::Stitch));
            continue;
        }
        match b1 {
            END => break,
            COLOR_CHANGE => {
                let _ = r.take(2);
                points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::ColorChange));
            }
            JUMP => {
                let (Some(dx), Some(dy)) = (r.u8(), r.u8()) else { break };
                let (dx, dy) = (i32::from(dx as i8), i32::from(dy as i8));
                let here = Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::Trim);
                if dx == 0 && dy == 0 {
                    points.push(here);
                } else {
                    x += dx;
                    y -= dy;
                    points.push(Stitch::new(f64::from(x) / 10.0, f64::from(y) / 10.0, Command::Jump));
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
