//! The fixed seed the fork's property suites in this crate run from (VectorCraft Stitch): the one
//! the fork's other suites run with (`vectorcraft_testkit::embroidery::proptest_config`, out of
//! reach here: the test kit depends on this crate). Both the embroidery settings' transforms
//! (`embroidery/transform.rs`) and the canvas's bounds (`visual_bounds/tests.rs`) take it.

use proptest::test_runner::{Config, RngSeed};

/// The seed the suites run with unless `PROPTEST_RNG_SEED` is set: the test kit's.
const PROPTEST_SEED: u64 = 0x5717_c4ed;

/// `cases` cases from [`PROPTEST_SEED`], with no failure files written, so that a CI run never
/// fails at random, as the test kit gives the fork's other suites: explore further locally with
/// `PROPTEST_CASES` and `PROPTEST_RNG_SEED`, and keep a minimal failure found that way as a test of
/// its own.
pub(crate) fn seeded(cases: u32) -> Config {
    let mut c = Config { failure_persistence: None, ..Config::default() };
    if std::env::var_os("PROPTEST_CASES").is_none() {
        c.cases = cases;
    }
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        c.rng_seed = RngSeed::Fixed(PROPTEST_SEED);
    }
    c
}
