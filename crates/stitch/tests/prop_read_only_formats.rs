//! The formats read but never written (VectorCraft Stitch), SEW and HUS: for any records the test
//! kit's files hold (its HUS compressor, written for these tests, and its SEW builder), the reader
//! finds the needle points where the records put them, run by run, and a whole file leaves nothing
//! out; for damaged, cut and random HUS bytes it gives a design or an error, never a panic, and
//! quickly, and a file cut short says it ends early.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_stitch::format::{Format, Loss};
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
use vectorcraft_testkit::machine_files::{Lzh, hus, sew_from_jef};

/// HUS's kinds of record (the reader's module): a stitch, a jump, a stop, a colour change, a cut,
/// the end; and a kind no source names.
const KINDS: [u8; 7] = [0x80, 0x81, 0x82, 0x84, 0x88, 0x90, 0x07];

/// Records of any kind, moves of any size a byte holds, stitches the most of them; each move 0
/// one time in four, so that records which don't move (an end, a kind no source names) come up,
/// one in sixteen.
fn arb_records() -> impl Strategy<Value = Vec<(u8, i8, i8)>> {
    let kind = prop_oneof![6 => Just(0x80u8), 4 => prop::sample::select(KINDS.to_vec())];
    let step = || prop_oneof![3 => any::<i8>(), 1 => Just(0i8)];
    prop::collection::vec((kind, step(), step()), 1..400)
}

/// How the test kit compresses: with or without copies, in blocks of 1 to 63 tokens or one for
/// the whole stream.
fn arb_lzh() -> impl Strategy<Value = Lzh> {
    (any::<bool>(), prop_oneof![Just(0usize), 1usize..64]).prop_map(|(copies, block_tokens)| Lzh { copies, block_tokens })
}

/// The needle points `records` put the needle down at, in runs sewn without lifting the needle (a
/// run ends at a jump, a stop, a colour change, a cut, and a record of another kind that moves
/// the needle), in tenths of a millimetre with y downwards, from the first needle point: what any
/// reading of them must find.
fn runs_of_records(records: &[(u8, i8, i8)]) -> Vec<Vec<(i64, i64)>> {
    let (mut x, mut y, mut runs, mut run) = (0i64, 0i64, Vec::new(), Vec::new());
    for &(kind, dx, dy) in records {
        x += i64::from(dx);
        y -= i64::from(dy);
        match kind {
            0x80 => run.push((x, y)),
            0x81 | 0x82 | 0x84 | 0x88 => runs.push(std::mem::take(&mut run)),
            _ if dx != 0 || dy != 0 => runs.push(std::mem::take(&mut run)),
            _ => {}
        }
    }
    runs.push(run);
    relative_runs(runs)
}

/// A design's needle points in runs: a block's stitches split at every point that isn't a stitch,
/// as opening a file draws them, in tenths of a millimetre, from the first needle point.
fn runs_read(p: &Pattern) -> Vec<Vec<(i64, i64)>> {
    let mut runs = Vec::new();
    for block in &p.blocks {
        let mut run = Vec::new();
        for s in &block.stitches {
            if s.command == Command::Stitch {
                run.push(((s.x * 10.0).round() as i64, (s.y * 10.0).round() as i64));
            } else {
                runs.push(std::mem::take(&mut run));
            }
        }
        runs.push(run);
    }
    relative_runs(runs)
}

/// `runs` without the empty ones, from the first needle point.
fn relative_runs(runs: Vec<Vec<(i64, i64)>>) -> Vec<Vec<(i64, i64)>> {
    let runs: Vec<Vec<(i64, i64)>> = runs.into_iter().filter(|r| !r.is_empty()).collect();
    let first = runs.first().and_then(|r| r.first()).copied().unwrap_or((0, 0));
    runs.into_iter().map(|r| r.into_iter().map(|(x, y)| (x - first.0, y - first.1)).collect()).collect()
}

/// The needle points `records` put the needle down at (tenths of a millimetre, y downwards), from
/// the first.
fn needle_points(records: &[(u8, i8, i8)]) -> Vec<(i64, i64)> {
    let (mut x, mut y, mut out) = (0i64, 0i64, Vec::new());
    for &(kind, dx, dy) in records {
        x += i64::from(dx);
        y -= i64::from(dy);
        if kind == 0x80 {
            out.push((x, y));
        }
    }
    relative(out)
}

fn relative(points: Vec<(i64, i64)>) -> Vec<(i64, i64)> {
    let first = points.first().copied().unwrap_or((0, 0));
    points.into_iter().map(|(x, y)| (x - first.0, y - first.1)).collect()
}

fn read_needle_points(p: &Pattern) -> Vec<(i64, i64)> {
    relative(p.points().filter(|s| s.command == Command::Stitch).map(|s| ((s.x * 10.0).round() as i64, (s.y * 10.0).round() as i64)).collect())
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(128))]

    /// Any records, compressed as the test kit compresses (with or without copies, in blocks of 1
    /// to 63 tokens or one), read as their needle points run by run, a stop for each stop and no
    /// more cuts than they hold (two cuts in a row read as one), the file leaving nothing out; with
    /// no stitch, the file is refused.
    #[test]
    fn a_hus_reads_as_its_records(records in arb_records(), colours in prop::collection::vec(0u16..40, 0..6), o in arb_lzh()) {
        let bytes = hus(&records, &colours, b"prop\0\0\0\0", &o);
        let want = needle_points(&records);
        match Format::Hus.read(&bytes) {
            Ok(p) => {
                prop_assert!(!want.is_empty());
                prop_assert_eq!(read_needle_points(&p), want);
                prop_assert_eq!(runs_read(&p), runs_of_records(&records));
                let count = |c: Command| p.points().filter(|s| s.command == c).count();
                prop_assert_eq!(count(Command::Stop), records.iter().filter(|r| r.0 == 0x82).count());
                prop_assert!(count(Command::Trim) <= records.iter().filter(|r| r.0 == 0x88).count());
                prop_assert_eq!(Format::Hus.losses(&bytes), []);
            }
            Err(e) => prop_assert!(want.is_empty(), "{e}"),
        }
    }

    /// A HUS with its streams cut, bytes changed anywhere, or random bytes behind its header gives
    /// a design or an error, quickly; cut, it finds only needle points its records hold, the first
    /// ones, and missing some, says that it ends early.
    #[test]
    fn a_damaged_hus_never_panics(records in arb_records(), o in arb_lzh(), cut in 0usize..5000, edits in prop::collection::vec((0usize..5000, any::<u8>()), 0..12), body in prop::collection::vec(any::<u8>(), 0..600)) {
        let bytes = hus(&records, &[1, 2], b"prop\0\0\0\0", &o);
        let started = std::time::Instant::now();
        let mut damaged = bytes.clone();
        for &(at, b) in &edits {
            let n = damaged.len();
            damaged[at % n] = b;
        }
        let _ = (Format::Hus.read(&damaged), Format::Hus.losses(&damaged));
        let short = &bytes[..cut.min(bytes.len())];
        if let Ok(p) = Format::Hus.read(short) {
            let (got, all) = (read_needle_points(&p), needle_points(&records));
            prop_assert!(all.starts_with(&got) || got.is_empty(), "a cut file finds only the needle points before the cut");
            if got.len() < all.len() {
                prop_assert_eq!(Format::Hus.losses(short), [Loss::EndsEarly]);
            }
        }
        // Random streams: the header kept, its three streams pointing into random bytes.
        let start = 42 + 2 * 2;
        let mut random = bytes[..start].to_vec();
        random.extend_from_slice(&body);
        let third = body.len() / 3;
        for (k, at) in [start, start + third, start + 2 * third].into_iter().enumerate() {
            random[20 + 4 * k..24 + 4 * k].copy_from_slice(&(at as u32).to_le_bytes());
        }
        let _ = (Format::Hus.read(&random), Format::Hus.losses(&random));
        prop_assert!(started.elapsed().as_secs() < 10);
    }

    /// Any design written as JEF and held in a SEW (its records after SEW's header and pictures)
    /// reads as the JEF does: the same blocks, threads and stitches, the file leaving nothing out;
    /// its last byte cut off, it says that it ends early.
    #[test]
    fn a_sew_reads_as_the_jef_it_holds(blocks in prop::collection::vec((any::<[u8; 3]>(), prop::collection::vec((-60i32..60, -60i32..60, 0u8..10), 1..40)), 1..12)) {
        let mut p = Pattern::default();
        for (colour, points) in blocks {
            let mut b = Block::new(Thread::rgb(colour));
            for (x, y, k) in points {
                let command = match k { 0 => Command::Jump, 1 => Command::Trim, _ => Command::Stitch };
                b.stitches.push(Stitch::new(f64::from(x) / 2.0, f64::from(y) / 2.0, command));
            }
            p.blocks.push(b);
        }
        let Ok(jef) = Format::Jef.write(&p) else { return Ok(()) };
        let sew = sew_from_jef(&jef).unwrap();
        prop_assert_eq!(Format::Sew.read(&sew).unwrap(), Format::Jef.read(&jef).unwrap());
        prop_assert_eq!(Format::Sew.losses(&sew), []);
        prop_assert_eq!(Format::Sew.losses(&sew[..sew.len() - 1]), [Loss::EndsEarly]);
    }
}
