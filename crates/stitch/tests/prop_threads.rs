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
use vectorcraft_stitch::thread::{ColourToMatch, MatchReason, Palette, PaletteThread, Threads, mismatched};

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
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(512))]

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
                // Its nearest, which a neighbour has only when no other thread is similar (as the
                // difference is shown, to two decimals: the rule's own line).
                prop_assert!(!neighbours.contains(&n) || mismatched(other.delta_e), "block {}: {:?} beside {:?}", k, other, neighbours);
            } else {
                // Another: the nearest neither neighbour has, similar, because a neighbour has the nearest.
                prop_assert!(neighbours.contains(&nearest.index), "block {}", k);
                prop_assert_eq!(n, other.index);
                prop_assert!(!neighbours.contains(&n) && !mismatched(delta_e_rgb(t.color, palette.get(n).unwrap().0)));
            }
        }
    }
}

/// A case the property found: a green beside a block of its nearest thread takes Brother's next
/// nearest, 10.0032 from it, shown as 10.00: similar by the rule, which reads the difference as
/// shown.
#[test]
fn a_thread_shown_at_the_threshold_is_similar() {
    let threads = vec![Thread::rgb([64, 154, 0]), Thread::rgb([57, 210, 106])];
    let numbers = Palette::Pec.numbers(&threads);
    let nearest = Palette::Pec.nearest([57, 210, 106], &[]);
    assert_eq!(nearest.index, numbers[0], "the block before has the green's nearest thread");
    let other = Palette::Pec.nearest([57, 210, 106], &[numbers[0]]);
    assert!(other.delta_e > 10.0 && !mismatched(other.delta_e), "{other:?}");
    assert_eq!(numbers[1], other.index);
}

/// A design's colour to match: any colour, or a thread's own, covering any area, with a thread
/// chosen for it from either palette now and then.
fn arb_colour() -> impl Strategy<Value = ColourToMatch> {
    let own = (arb_palette(), 1..=MOST_ENTRIES).prop_map(|(p, i)| p.get(i).map_or([0, 0, 0], |(rgb, _)| rgb));
    let color = prop_oneof![3 => any::<[u8; 3]>(), 1 => own];
    let chosen = prop_oneof![4 => Just(None), 1 => (arb_palette(), 1..=MOST_ENTRIES).prop_map(|(p, i)| PaletteThread::new(p, i))];
    (color, 1u32..1000, chosen).prop_map(|(color, area, chosen)| ColourToMatch { color, area: f64::from(area), chosen })
}

/// The least total difference of colours `free` (indices into `colours`), each given a distinct
/// thread of `near` within the line, or none (paying `unmatched`): by brute force.
fn least(colours: &[ColourToMatch], free: &[usize], near: &[usize], palette: Palette, taken: &[usize], unmatched: f64) -> f64 {
    let Some((&c, rest)) = free.split_first() else { return 0.0 };
    let mut best = unmatched + least(colours, rest, near, palette, taken, unmatched);
    for &t in near {
        if taken.contains(&t) {
            continue;
        }
        let d = delta_e_rgb(colours[c].color, palette.get(t).unwrap().0);
        if mismatched(d) {
            continue;
        }
        let mut more = taken.to_vec();
        more.push(t);
        best = best.min(d + least(colours, rest, near, palette, &more, unmatched));
    }
    best
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(256))]

    /// Threads for a design's colours keep the rules: each is a thread (and a neutral one for
    /// greys, but for a thread chosen for it), a choice from the palette is kept, each difference is the colour's from its
    /// thread, the order the colours come in changes nothing but the order of the answer, and
    /// within the line the fewest colours share a thread and then the least total difference is
    /// taken (against brute force over the threads near them).
    #[test]
    fn colours_get_threads_by_the_rules(palette in arb_palette(), colours in prop::collection::vec(arb_colour(), 0..4), neutral in any::<bool>(), turn in 0usize..4) {
        let threads = if neutral { Threads::Neutral } else { Threads::All };
        let got = palette.match_colours(&colours, threads).unwrap();
        prop_assert_eq!(got.len(), colours.len());
        for (c, m) in colours.iter().zip(&got) {
            prop_assert!(palette.is_thread(m.thread.index));
            let chosen = c.chosen.filter(|t| t.palette() == palette);
            prop_assert!(!neutral || palette.is_neutral(m.thread.index) || chosen.is_some_and(|t| t.index() == m.thread.index));
            prop_assert_eq!(m.thread.delta_e, delta_e_rgb(c.color, palette.get(m.thread.index).unwrap().0));
            if let Some(t) = chosen {
                prop_assert_eq!((m.thread.index, m.reason), (t.index(), MatchReason::Chosen));
            }
            prop_assert!(!m.alternatives.iter().any(|a| a.index == m.thread.index));
        }
        // Turned round: the same threads, each with its colour.
        let mut turned = colours.clone();
        turned.rotate_left(turn.min(colours.len()));
        let again = palette.match_colours(&turned, threads).unwrap();
        for (k, m) in again.iter().enumerate() {
            let from = (k + turn.min(colours.len())) % colours.len().max(1);
            prop_assert_eq!(m.thread, got[from].thread);
        }
        // The colours whose thread wasn't fixed (chosen, or their own): within the line as few
        // without one of their own as can be, then the least total.
        let fixed: Vec<usize> = (0..colours.len()).filter(|&c| matches!(got[c].reason, MatchReason::Chosen | MatchReason::Own)).collect();
        let taken: Vec<usize> = fixed.iter().map(|&c| got[c].thread.index).collect();
        let free: Vec<usize> = (0..colours.len()).filter(|c| !fixed.contains(c)).collect();
        let near: Vec<usize> = (1..=palette.threads().len())
            .filter(|&t| palette.is_thread(t) && (!neutral || palette.is_neutral(t)))
            .filter(|&t| free.iter().any(|&c| !mismatched(delta_e_rgb(colours[c].color, palette.get(t).unwrap().0))))
            .collect();
        let unmatched = 1.0e6;
        let ours: f64 = free
            .iter()
            .map(|&c| {
                let own = !matches!(got[c].reason, MatchReason::Shared { .. }) && !mismatched(got[c].thread.delta_e) && !taken.contains(&got[c].thread.index);
                if own { got[c].thread.delta_e } else { unmatched }
            })
            .sum();
        let best = least(&colours, &free, &near, palette, &taken, unmatched);
        prop_assert!((ours - best).abs() < 1e-6, "ours {} against the least {}: {:?}", ours, best, got);
    }
}
