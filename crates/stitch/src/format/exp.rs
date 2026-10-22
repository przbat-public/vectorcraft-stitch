//! Melco EXP: stitch data only, no header and no colours.
//!
//! Every record is two bytes: the x and y deltas in tenths of a millimetre, two's complement,
//! so a record moves at most 127 tenths (12.7 mm) — longer moves are written as several records.
//! The byte `0x80` cannot be a delta (-128 is not used), so it starts a four-byte command: a
//! second byte says what it is, then come the two deltas. `0x80 0x04` jumps, `0x80 0x01` changes
//! colour (the machine stops), `0x80 0x80` trims, and `0x80 0x02` puts the needle down without
//! moving (after a jump, so that the next stitch is at the jumped position). The file simply
//! ends; there is no end command. Y counts upwards.
//!
//! Reference: EduTechWiki, *Embroidery format EXP* (edutechwiki.unige.ch/en/Embroidery_format_EXP).

use super::{Emitter, Frame, Reader, TWO_BYTE_COMMAND, finish, mm, two_byte_move, walk};
use crate::{Command, Error, Pattern, Result, Stitch};

/// The largest delta one record holds, in tenths of a millimetre.
pub(crate) const MAX_DELTA: i32 = 127;
/// The command prefix: `0x80` is not a valid delta.
const CONTROL: u8 = TWO_BYTE_COMMAND;
/// The commands' second bytes: a jump (the needle up, then the deltas), the needle put down where
/// it is, a colour change (the machine stops), and a trim.
const JUMP: u8 = 0x04;
const STITCH_DOWN: u8 = 0x02;
const COLOR_CHANGE: u8 = 0x01;
const TRIM: u8 = 0x80;
/// The two bytes that follow a trim command, as the format's decodings give them: not a move.
const TRIM_PAYLOAD: [u8; 2] = [0x07, 0x00];

pub fn write(p: &Pattern) -> Result<Vec<u8>> {
    let frame = Frame::of(p, true)?;
    // Two bytes a point, and a little more for commands (a guess; the buffer grows as needed).
    let mut w = Exp { out: Vec::with_capacity(p.points().count() * 2 + 16), at: (0, 0), wrote: false };
    walk(p, &frame, &mut w, "EXP")?;
    if !w.wrote {
        return Err(Error::Format("the design has no stitches to write".into()));
    }
    Ok(w.out)
}

/// An EXP file being written: its records, where the needle is, and whether a stitch or a jump was
/// written yet.
struct Exp {
    out: Vec<u8>,
    at: (i32, i32),
    wrote: bool,
}

impl Emitter for Exp {
    fn stitch(&mut self, x: i32, y: i32) {
        two_byte_move(&mut self.out, &mut self.at, (x, y), None, MAX_DELTA);
        self.wrote = true;
    }

    fn jump(&mut self, x: i32, y: i32) {
        two_byte_move(&mut self.out, &mut self.at, (x, y), Some(JUMP), MAX_DELTA);
        self.wrote = true;
    }

    fn trim(&mut self) {
        self.out.extend_from_slice(&[CONTROL, TRIM, TRIM_PAYLOAD[0], TRIM_PAYLOAD[1]]);
    }

    fn color_change(&mut self) {
        self.out.extend_from_slice(&[CONTROL, COLOR_CHANGE, 0, 0]);
    }

    fn wrote(&self) -> bool {
        self.wrote
    }

    fn size(&self) -> usize {
        self.out.len()
    }
}

pub fn read(bytes: &[u8]) -> Result<Pattern> {
    let mut r = Reader::new(bytes);
    let mut points: Vec<Stitch> = Vec::new();
    let (mut x, mut y) = (0i32, 0i32);
    while let Some(b0) = r.u8() {
        let Some(b1) = r.u8() else { break };
        if b0 != CONTROL {
            x += i32::from(b0 as i8);
            y -= i32::from(b1 as i8);
            points.push(Stitch::new(mm(x), mm(y), Command::Stitch));
            continue;
        }
        let (Some(dx), Some(dy)) = (r.u8(), r.u8()) else { break };
        let (dx, dy) = (i32::from(dx as i8), i32::from(dy as i8));
        match b1 {
            JUMP => {
                x += dx;
                y -= dy;
                points.push(Stitch::new(mm(x), mm(y), Command::Jump));
            }
            STITCH_DOWN => {
                x += dx;
                y -= dy;
                points.push(Stitch::new(mm(x), mm(y), Command::Stitch));
            }
            COLOR_CHANGE => {
                points.push(Stitch::new(mm(x), mm(y), Command::ColorChange));
                if dx != 0 || dy != 0 {
                    x += dx;
                    y -= dy;
                    points.push(Stitch::new(mm(x), mm(y), Command::Jump));
                }
            }
            TRIM => points.push(Stitch::new(mm(x), mm(y), Command::Trim)),
            // An unknown command: stop rather than guess where the needle goes next.
            _ => break,
        }
        // A trim's payload is not a move.
    }
    if points.is_empty() {
        return Err(Error::Format("the EXP file has no stitch records".into()));
    }
    Ok(finish(String::new(), points, &[]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, Thread};

    #[test]
    fn a_design_round_trips() {
        let mut p = Pattern::default();
        let mut a = Block::new(Thread::rgb([200, 20, 20]));
        for i in 0..10 {
            let t = f64::from(i);
            a.stitches.push(Stitch::sew(vectorcraft_geom::Point::new(t * 2.0, t)));
        }
        let mut b = Block::new(Thread::rgb([20, 20, 200]));
        b.stitches.push(Stitch::new(30.0, 30.0, Command::Jump));
        b.stitches.push(Stitch::sew(vectorcraft_geom::Point::new(32.0, 31.0)));
        p.blocks.push(a);
        p.blocks.push(b);
        let bytes = write(&p).expect("writes");
        let back = read(&bytes).expect("reads");
        assert_eq!(back.blocks.len(), 2);
        let sewn = |q: &Pattern| q.blocks.iter().map(|b| b.sewn()).sum::<usize>();
        assert_eq!(sewn(&back), 11);
    }
}
