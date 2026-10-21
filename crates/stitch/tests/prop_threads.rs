//! Thread matching property tests: for any run of blocks (any colour, a palette entry's own colour,
//! appliqué entries' included, and a thread chosen from either palette or none), the threads a
//! format writes (`Palette::numbers`) keep the rules: a known thread is kept, every other block
//! gets its nearest thread unless a neighbour has it and a similar one is left, and no block gets
//! an entry that isn't a thread.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_stitch::Thread;
use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::thread::{MISMATCH_DELTA_E, Palette, PaletteThread};

/// The most entries a palette has (Janome's 78), and one past it for numbers that name none.
const MOST_ENTRIES: usize = 78;

fn arb_palette() -> impl Strategy<Value = Palette> {
    prop::sample::select(Palette::ALL.to_vec())
}

/// A block's thread: any colour or an entry's own, and a thread chosen from either palette, or none
/// (a number that names no thread chooses none).
fn arb_thread() -> impl Strategy<Value = Thread> {
    let own = (arb_palette(), 1..=MOST_ENTRIES).prop_map(|(p, i)| p.get(i).map_or([0, 0, 0], |(rgb, _)| rgb));
    let color = prop_oneof![any::<[u8; 3]>(), own];
    let chosen = prop::option::of((arb_palette(), 0..=MOST_ENTRIES + 1)).prop_map(|c| c.and_then(|(p, i)| PaletteThread::new(p, i)));
    (color, chosen).prop_map(|(color, chosen)| Thread { chosen, ..Thread::rgb(color) })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn each_block_gets_a_thread_by_the_rules(palette in arb_palette(), threads in prop::collection::vec(arb_thread(), 1..8)) {
        let numbers = palette.numbers(&threads);
        prop_assert_eq!(numbers.len(), threads.len());
        // A block's thread is known before matching when it is chosen from this palette or its
        // colour is a thread's own.
        let own = |c: [u8; 3]| (1..=MOST_ENTRIES).find(|&i| palette.is_thread(i) && palette.get(i).is_some_and(|(rgb, _)| rgb == c));
        let known: Vec<Option<usize>> =
            threads.iter().map(|t| t.chosen.filter(|c| c.palette() == palette).map(PaletteThread::index).or_else(|| own(t.color))).collect();
        for (k, (t, &n)) in threads.iter().zip(&numbers).enumerate() {
            prop_assert!(palette.is_thread(n), "block {} got {}, which is no thread", k, n);
            if let Some(want) = known[k] {
                prop_assert_eq!(n, want, "block {} keeps its thread", k);
                continue;
            }
            // The neighbours' threads: the block before's, and the block after's when known.
            let neighbours: Vec<usize> = k.checked_sub(1).map(|b| numbers[b]).into_iter().chain(known.get(k + 1).copied().flatten()).collect();
            let nearest = palette.nearest(t.color, &[]);
            let other = palette.nearest(t.color, &neighbours);
            if n == nearest.index {
                // Its nearest, which a neighbour has only when no other thread is similar.
                prop_assert!(!neighbours.contains(&n) || other.delta_e > MISMATCH_DELTA_E, "block {}: {:?} beside {:?}", k, other, neighbours);
            } else {
                // Another: the nearest neither neighbour has, similar, because a neighbour has the nearest.
                prop_assert!(neighbours.contains(&nearest.index), "block {}", k);
                prop_assert_eq!(n, other.index);
                prop_assert!(!neighbours.contains(&n) && delta_e_rgb(t.color, palette.get(n).unwrap().0) <= MISMATCH_DELTA_E);
            }
        }
    }
}
