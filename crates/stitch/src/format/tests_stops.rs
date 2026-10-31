//! A stop in every format: written as a change to the same thread, only between two needle points
//! of one block ([`super::stretches`]), with the headers that list one thread a colour (PES, PEC
//! and JEF) repeating the block's.

use super::{Format, stretches, written_stops};
use crate::thread::Palette;
use crate::{Block, Command, Pattern, Stitch, Thread};

/// A block of `color` sewn along y = `y`, a needle point every 2 mm from x = `x0`, `n` of them.
fn row(color: [u8; 3], x0: f64, y: f64, n: usize) -> Block {
    let mut b = Block::new(Thread::rgb(color));
    b.stitches = (0..n).map(|k| Stitch::new(x0 + 2.0 * k as f64, y, Command::Stitch)).collect();
    b
}

/// The art colours: a red and a blue far apart, each with its nearest thread in both palettes.
const RED: [u8; 3] = [200, 30, 30];
const BLUE: [u8; 3] = [30, 30, 200];

/// `block` with a stop after its needle point `after` (from 0), where that needle point is.
fn stopped(mut block: Block, after: usize) -> Block {
    let at = block.stitches[after];
    block.stitches.insert(after + 1, Stitch::new(at.x, at.y, Command::Stop));
    block
}

/// A red block with a stop after its fifth needle point, then a blue block.
fn with_a_stop() -> Pattern {
    Pattern { name: "stop".into(), blocks: vec![stopped(row(RED, 0.0, 0.0, 10), 4), row(BLUE, 0.0, 6.0, 5)], ..Pattern::default() }
}

/// Each block's needle points, read back.
fn needle_points(p: &Pattern) -> Vec<usize> {
    p.blocks.iter().map(|b| b.stitches.iter().filter(|s| s.command == Command::Stitch).count()).collect()
}

/// A stop between two needle points of a block is a change to the same thread in every format: read
/// back, one block more, the needle points as they were; in PES, PEC, JEF and VP3 the block after
/// the stop is in the same thread (the header repeats the block's number), while DST, TAP, T01 and
/// EXP, which carry no colours, read each block in a default colour of its own.
#[test]
fn a_stop_is_a_change_to_the_same_thread_in_every_format() {
    let p = with_a_stop();
    for format in Format::ALL {
        let bytes = format.write(&p).unwrap();
        let back = format.read(&bytes).unwrap();
        assert_eq!(needle_points(&back), [5, 5, 5], "{format:?}");
        let same = back.blocks[0].thread == back.blocks[1].thread;
        let colours = matches!(format, Format::Pes | Format::Pec | Format::Jef | Format::Vp3);
        assert_eq!(same, colours, "{format:?}: {:?}", back.blocks.iter().map(|b| &b.thread).collect::<Vec<_>>());
        assert_ne!(back.blocks[1].thread, back.blocks[2].thread, "{format:?}: the blue block");
    }
    // The headers list the stop's thread: three colours, the first twice.
    for (format, palette) in [(Format::Pes, Palette::Pec), (Format::Pec, Palette::Pec), (Format::Jef, Palette::Jef)] {
        let back = format.read(&format.write(&p).unwrap()).unwrap();
        let (red, blue) = (palette.thread(palette.nearest(RED, &[]).index), palette.thread(palette.nearest(BLUE, &[]).index));
        let threads: Vec<Option<Thread>> = back.blocks.iter().map(|b| Some(b.thread.clone())).collect();
        assert_eq!(threads, [red.clone(), red, blue], "{format:?}");
    }
}

/// The header's colours are the colours the body sews, with a stop: PES's and PEC's colour count
/// (one less than the colours, at byte 48 of the PEC header) and JEF's (a word at byte 24).
#[test]
fn the_headers_count_a_stop_as_a_colour() {
    let p = with_a_stop();
    let pes = Format::Pes.write(&p).unwrap();
    // The PEC section starts where the PES header says (a word at byte 8).
    let pec_at = u32::from_le_bytes([pes[8], pes[9], pes[10], pes[11]]) as usize;
    assert_eq!(pes[pec_at + 48], 2, "PES: three colours");
    let pec = Format::Pec.write(&p).unwrap();
    assert_eq!(pec[super::pes::PEC_MAGIC.len() + 48], 2, "PEC: three colours");
    let jef = Format::Jef.write(&p).unwrap();
    assert_eq!(u32::from_le_bytes([jef[24], jef[25], jef[26], jef[27]]), 3, "JEF: three colours");
}

/// A stop where no needle point of its block lies on both sides is written as nothing: at the
/// block's start or end (the thread change, or the design's end, stops a machine there), right
/// after another stop. The file is the one written without them.
#[test]
fn a_stop_not_between_two_needle_points_is_written_as_nothing() {
    let plain = with_a_stop();
    let mut extra = plain.clone();
    {
        let red = &mut extra.blocks[0].stitches;
        let first = red[0];
        red.insert(0, Stitch::new(first.x, first.y, Command::Stop));
        // After the stop that is written: a second one at the same needle point.
        let at = red.iter().position(|s| s.command == Command::Stop && s.x > 0.0).unwrap();
        red.insert(at + 1, red[at]);
        let last = *red.last().unwrap();
        red.push(Stitch::new(last.x, last.y, Command::Stop));
    }
    // The design's end.
    let last = *extra.blocks[1].stitches.last().unwrap();
    extra.blocks[1].stitches.push(Stitch::new(last.x, last.y, Command::Stop));
    for format in Format::ALL {
        assert_eq!(format.write(&extra).unwrap(), format.write(&plain).unwrap(), "{format:?}");
    }
    assert_eq!(extra.blocks.iter().map(|b| written_stops(&b.stitches).len()).collect::<Vec<_>>(), [1, 0]);
}

/// A cut beside a stop is kept on its side: before it, at the stretch's end; after it, at the next
/// stretch's start. A cut alone between two stops is kept too, with the stretch after it.
#[test]
fn a_cut_beside_a_stop_is_kept() {
    let mut block = row(RED, 0.0, 0.0, 6);
    let at = block.stitches[2];
    let cut = Stitch::new(at.x, at.y, Command::Trim);
    let stop = Stitch::new(at.x, at.y, Command::Stop);
    // Needle point 2, a cut, a stop, a cut, a stop, then the rest.
    block.stitches.splice(3..3, [cut, stop, cut, stop]);
    let ranges = stretches(&block.stitches);
    assert_eq!(ranges, [0..4, 5..10], "cut at the first stop, the second written as nothing");
    let p = Pattern { blocks: vec![block], ..Pattern::default() };
    for format in Format::ALL {
        let back = format.read(&format.write(&p).unwrap()).unwrap();
        assert_eq!(needle_points(&back), [3, 3], "{format:?}");
    }
}

/// Stops and the threads after them survive their own round trip: a file read and written again
/// is written again the same (from the second time on for the formats that start at a corner,
/// which keep the start the first reading found).
#[test]
fn a_stop_survives_its_own_round_trip() {
    let mut p = with_a_stop();
    // Two stops in one block, and one in the second block.
    p.blocks[0] = stopped(p.blocks[0].clone(), 8);
    p.blocks[1] = stopped(p.blocks[1].clone(), 1);
    for format in Format::ALL {
        let once = format.write(&p).unwrap();
        let twice = format.write(&format.read(&once).unwrap()).unwrap();
        let thrice = format.write(&format.read(&twice).unwrap()).unwrap();
        assert_eq!(thrice, twice, "{format:?}");
        assert_eq!(needle_points(&format.read(&once).unwrap()), [5, 3, 2, 2, 3], "{format:?}");
    }
}

/// More colours than PES and PEC hold, stops counted, are refused: 254 blocks and two stops.
#[test]
fn stops_count_toward_the_colours_a_format_holds() {
    let mut blocks: Vec<Block> = (0..254).map(|k| row([(k % 200) as u8, 30, 30], 0.0, k as f64, 3)).collect();
    blocks[0] = stopped(blocks[0].clone(), 0);
    let p = Pattern { blocks: blocks.clone(), ..Pattern::default() };
    assert!(Format::Pes.write(&p).is_ok(), "255 colours");
    blocks[1] = stopped(blocks[1].clone(), 1);
    let p = Pattern { blocks, ..Pattern::default() };
    for format in [Format::Pes, Format::Pec] {
        assert!(format.write(&p).is_err(), "{format:?}: 256 colours");
    }
}
