//! The formats read but never written (VectorCraft Stitch), SEW: for any design the test kit's SEW
//! builder holds, the reader finds the stitches its JEF records hold.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
use vectorcraft_testkit::machine_files::sew_from_jef;

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(128))]

    /// Any design written as JEF and held in a SEW (its records after SEW's header and pictures)
    /// reads as the JEF does: the same blocks, threads and stitches.
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
    }
}
