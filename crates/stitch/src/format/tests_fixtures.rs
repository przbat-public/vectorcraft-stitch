//! Byte fixtures built by hand from the formats' public tables (EduTechWiki's pages on DST and
//! EXP), read by our readers: an oracle that doesn't share the readers' and writers' own tables.
//!
//! DST (and TAP and T01, its records without the header): three bytes a record; the x deltas
//! ±1 and ±9 in the first byte's bits 0x01/0x02 and 0x04/0x08, ±3 and ±27 in the second's, ±81 in
//! the third's 0x04/0x08; y's ±1 in the first byte's 0x80/0x40 (y up); the third byte's 0x03 always
//! set, 0x80 a jump, 0xC0 a colour change, 0xF3 the end.
//!
//! EXP: two signed bytes a record (x, then y up), and four for a command after `0x80`: `0x04` a
//! jump, `0x02` the needle put down, `0x01` a colour change, `0x80` a trim.

use super::Format;
use crate::{Command, Pattern};

/// A design's points as (command, x, y) in tenths of a millimetre from its first point, rounded,
/// without the colour changes between blocks (a reader moves a design to its corner).
fn points(p: &Pattern) -> Vec<(Command, i64, i64)> {
    let first = p.points().next().map_or((0.0, 0.0), |s| (s.x, s.y));
    p.points()
        .filter(|s| s.command != Command::ColorChange)
        .map(|s| (s.command, ((s.x - first.0) * 10.0).round() as i64, ((s.y - first.1) * 10.0).round() as i64))
        .collect()
}

/// The records: x+1, x+3, x+81, y+1 (up), a jump of x+1, a stitch where it lands, the end.
const DST_RECORDS: [[u8; 3]; 7] =
    [[0x01, 0x00, 0x03], [0x00, 0x01, 0x03], [0x00, 0x00, 0x07], [0x80, 0x00, 0x03], [0x01, 0x00, 0x83], [0x00, 0x00, 0x03], [0x00, 0x00, 0xF3]];

/// What they hold, in tenths from the first point, y down as the design counts it.
const DST_POINTS: [(Command, i64, i64); 6] = [
    (Command::Stitch, 0, 0),
    (Command::Stitch, 3, 0),
    (Command::Stitch, 84, 0),
    (Command::Stitch, 84, -1),
    (Command::Jump, 85, -1),
    (Command::Stitch, 85, -1),
];

#[test]
fn dst_tap_and_t01_records_read_as_the_tables_say() {
    let records: Vec<u8> = DST_RECORDS.concat();
    let mut dst = b"LA:fixture        \r".to_vec();
    dst.push(0x1A);
    dst.resize(512, 0x20);
    dst.extend_from_slice(&records);
    assert_eq!(points(&Format::Dst.read(&dst).unwrap()), DST_POINTS);
    for f in [Format::Tap, Format::T01] {
        assert_eq!(points(&f.read(&records).unwrap()), DST_POINTS, "{f:?}");
    }
}

#[test]
fn exp_records_and_commands_read_as_the_table_says() {
    let bytes: Vec<u8> = [
        &[0x0A, 0x00][..],             // a stitch x+10
        &[0x80, 0x04, 0x14, 0x00][..], // a jump x+20
        &[0x80, 0x02, 0x0A, 0x00][..], // the needle down x+10
        &[0x00, 0x0A][..],             // a stitch y+10 (up)
        &[0x80, 0x01, 0x05, 0x00][..], // a colour change that moves x+5
        &[0x05, 0x00][..],             // a stitch x+5
        &[0x80, 0x7F, 0x00, 0x00][..], // a command no table lists: the reader stops
        &[0x05, 0x00][..],
    ]
    .concat();
    let p = Format::Exp.read(&bytes).unwrap();
    assert_eq!(p.blocks.len(), 2, "the colour change starts a second block");
    assert_eq!(
        points(&p),
        [
            (Command::Stitch, 0, 0),
            (Command::Jump, 20, 0),
            (Command::Stitch, 30, 0),
            (Command::Stitch, 30, -10),
            (Command::Jump, 35, -10),
            (Command::Stitch, 40, -10),
        ]
    );
}
