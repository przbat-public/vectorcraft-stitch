//! The planner with no exit point sews what it sewed before entry and exit points were planned
//! toward (VectorCraft Stitch, 2026-10-26): a seeded corpus of random paths (open and closed
//! subpaths, curves, crossings, closed shapes) in every style, with every underlay, from a needle
//! point or from nowhere, and fills with narrow parts, each planned and its stitches hashed, each
//! point to a hundredth of a millimetre ([`QUANTUM_MM`]). The hashes in `golden_no_exit.txt` were
//! recorded by this file run against the planner before that work (commit be4c60d1), so they are an
//! oracle independent of the new code: a change that moves a stitch with no exit set fails here, on
//! shapes the pinned exports don't hold.
//!
//! To record them again (only for a change meant to move stitches with no exit, said in its
//! commit): `VECTORCRAFT_STITCH_GOLDEN_RECORD=1 cargo test -p vectorcraft-stitch --test golden_no_exit`.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use vectorcraft_geom::{BezPath, FillRule, Point};
use vectorcraft_stitch::plan::{Params, Style, Underlay, narrow_columns, try_fill_with_columns, try_stitches_from};
use vectorcraft_stitch::{Command, Stitch};
use vectorcraft_testkit::strategies::{arb_closed_shape, arb_path_data, arb_point};

/// How many paths in every style, and how many fills with narrow parts, the corpus holds.
const PATHS: usize = 160;
const FILLS_WITH_COLUMNS: usize = 60;

/// The corpus's seed: any fixed one.
const SEED: [u8; 32] = *b"vectorcraft-stitch golden corpus";

/// The grid each point is hashed on, millimetres: a tenth of the formats' 0.1 mm, finer than any
/// machine file holds, and far coarser than the last bit of a coordinate, where two platforms'
/// maths libraries (`hypot`, `atan2`, `acos`) may differ: hashed by its bits, one case in 220 sewed
/// otherwise on Linux than on macOS, with the same stitches.
const QUANTUM_MM: f64 = 0.01;

/// The file the hashes are kept in, beside this one.
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden_no_exit.txt");

/// One case: how it is planned.
#[derive(Debug)]
enum Case {
    /// A path in a style (and its satin width), an underlay, from a needle point or from nowhere.
    Path { path: BezPath, style: Style, width: f64, underlay: Underlay, from: Option<Point> },
    /// A closed shape filled with its parts narrower than `widest` sewn as satin columns.
    Columns { path: BezPath, widest: f64, from: Option<Point> },
}

fn arb_path_case() -> impl Strategy<Value = Case> {
    (
        arb_path_data(),
        prop_oneof![Just(Style::Fill), Just(Style::Running), Just(Style::Satin)],
        1.0..12.0f64,
        prop::sample::select(&Underlay::ALL[..]),
        proptest::option::of(arb_point(-20.0, 220.0)),
    )
        .prop_map(|(path, style, width, underlay, from)| Case::Path { path: path.to_bezpath(), style, width, underlay, from })
}

fn arb_columns_case() -> impl Strategy<Value = Case> {
    (arb_closed_shape(), 1.0..10.0f64, proptest::option::of(arb_point(-20.0, 220.0))).prop_map(|(shape, widest, from)| Case::Columns {
        path: shape.to_bezpath(),
        widest,
        from,
    })
}

/// The corpus, the same on every run and every target.
fn corpus() -> Vec<Case> {
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &SEED));
    let mut out = Vec::new();
    for _ in 0..PATHS {
        out.push(arb_path_case().new_tree(&mut runner).unwrap().current());
    }
    for _ in 0..FILLS_WITH_COLUMNS {
        out.push(arb_columns_case().new_tree(&mut runner).unwrap().current());
    }
    out
}

/// A case's stitches, or `None` when the planner refuses it.
fn sew(case: &Case) -> Option<Vec<Stitch>> {
    match case {
        Case::Path { path, style, width, underlay, from } => {
            let p = Params { width: *width, underlay: *underlay, ..Params::for_style(*style) };
            try_stitches_from(path, FillRule::NonZero, &p, *from).ok()
        }
        Case::Columns { path, widest, from } => {
            let columns = narrow_columns(path, FillRule::NonZero, *widest);
            let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
            try_fill_with_columns(path, FillRule::NonZero, path, &columns, &fill, &satin, *from).ok()
        }
    }
}

/// FNV-1a over the stitches (each point's x and y on the [`QUANTUM_MM`] grid, and its command), or
/// over "refused": a fingerprint that changes with any stitch moved past the grid.
fn fingerprint(stitches: Option<&[Stitch]>) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(PRIME);
        }
    };
    match stitches {
        None => eat(b"refused"),
        Some(out) => {
            for s in out {
                // Rounded to the grid; a point beyond i64's range is no stitch the planner makes.
                eat(&((s.x / QUANTUM_MM).round() as i64).to_le_bytes());
                eat(&((s.y / QUANTUM_MM).round() as i64).to_le_bytes());
                eat(&[match s.command {
                    Command::Stitch => 0,
                    Command::Jump => 1,
                    Command::Trim => 2,
                    Command::ColorChange => 3,
                    Command::Stop => 4,
                }]);
            }
        }
    }
    format!("{h:016x} {}", stitches.map_or(0, <[Stitch]>::len))
}

#[test]
fn with_no_exit_the_planner_sews_what_it_sewed_before() {
    let got: Vec<String> = corpus().iter().map(|c| fingerprint(sew(c).as_deref())).collect();
    if std::env::var_os("VECTORCRAFT_STITCH_GOLDEN_RECORD").is_some() {
        std::fs::write(GOLDEN, got.join("\n") + "\n").unwrap();
        return;
    }
    let want: Vec<String> = std::fs::read_to_string(GOLDEN).unwrap().lines().map(str::to_string).collect();
    assert_eq!(want.len(), got.len(), "the corpus and its recorded hashes differ in size");
    let corpus = corpus();
    let moved: Vec<String> = want
        .iter()
        .zip(&got)
        .enumerate()
        .filter(|(_, (w, g))| w != g)
        .map(|(k, (w, g))| format!("case {k}: {w} → {g}: {:?}", corpus.get(k)))
        .collect();
    assert!(moved.is_empty(), "{} of {} cases sew otherwise with no exit:\n{}", moved.len(), got.len(), moved.join("\n"));
}
