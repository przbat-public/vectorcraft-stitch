//! Threads for a design's colours, chosen together ([`Palette::match_colours`]): the colours an
//! image is reduced to, each given a thread of the palette as a person would pick them from a box
//! of threads.
//!
//! A colour gets its nearest thread unless another colour needs that thread more. Within
//! [`MISMATCH_DELTA_E`](super::MISMATCH_DELTA_E) every colour gets a thread of its own while the palette has one: two
//! colours sewn in one thread would be one colour in the design. So the threads are chosen as an
//! assignment, all at once: first the threads a person chose, then as few colours as can be left
//! without a thread of their own within the line, then the least total difference. A colour left
//! without one takes its nearest thread, shared when another colour holds it: two colours whose
//! only thread within the line is the same one are sewn in it, merged (the project owner's choice,
//! 2026-11-01). The colours are put in one order (largest first, then by their sRGB) before the
//! assignment, so the threads don't depend on the order the colours are given in.
//!
//! The assignment is the Hungarian method (H. W. Kuhn, "The Hungarian method for the assignment
//! problem", *Naval Research Logistics Quarterly* 2, 1955; J. Munkres, "Algorithms for the
//! assignment and transportation problems", *J. SIAM* 5, 1957), in its shortest-augmenting-path
//! form with potentials (R. Jonker and A. Volgenant, "A shortest augmenting path algorithm for
//! dense and sparse linear assignment problems", *Computing* 38, 1987), written from the papers.

use super::{Match, Palette, PaletteThread, mismatched};
use crate::color;

/// The most colours matched at once: as many as Image Trace reduces an image to at most (256
/// colours).
pub const MAX_COLOURS_MATCHED: usize = 256;

/// The threads given after a colour's own, nearest first, for a person to choose from: five. A
/// cream far from every Brother thread has five within 1.5 of each other (10.22 to 11.75 from it),
/// so close that CIEDE2000's order among them can't be trusted, and a person should see them all.
/// An estimate.
pub const ALTERNATIVES_SHOWN: usize = 5;

/// Below this chroma (C*ab, the distance from grey in CIE L*a*b*) a thread is neutral, a grey from
/// black to white. Computed from the tables: Brother's and Janome's neutral threads lie under 7.1,
/// and the least saturated of their other threads at 13.4 (Janome's Pale Sky) and 16.2 (Brother's
/// Dark Brown).
pub const NEUTRAL_CHROMA: f64 = 10.0;

/// What an assignment pays for a colour left without a thread of its own within
/// [`MISMATCH_DELTA_E`](super::MISMATCH_DELTA_E): more than the differences of every colour within the line added up
/// ([`MAX_COLOURS_MATCHED`] of them, each at most the line), so the assignment leaves as few
/// colours without one as it can, then takes the least total difference.
const UNMATCHED: f64 = 1.0e6;

/// What an assignment pays for a thread farther than [`MISMATCH_DELTA_E`](super::MISMATCH_DELTA_E) from a colour: more than
/// leaving the colour without a thread of its own, so it is never given one that way (it takes its
/// nearest thread afterwards, shared or not).
const TOO_FAR: f64 = 1.0e9;

/// The threads a colour may be matched to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threads {
    /// Every thread of the palette.
    All,
    /// Its neutral threads only ([`NEUTRAL_CHROMA`]): a picture in greys is sewn in grey threads.
    Neutral,
}

/// A colour of a design to be matched to a thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColourToMatch {
    /// The colour (sRGB).
    pub color: [u8; 3],
    /// How much of the design it covers, in any unit (the same for every colour): a larger colour
    /// comes first in the order the assignment is made in, and so keeps a thread in a tie.
    pub area: f64,
    /// The thread a person chose for it. One of another palette is no choice here (the caller
    /// matches its colour instead).
    pub chosen: Option<PaletteThread>,
}

/// Why a colour got its thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchReason {
    /// The colour is the thread's own.
    Own,
    /// A person chose it.
    Chosen,
    /// Its nearest thread, which no colour nearer needed.
    Nearest,
    /// Not its nearest thread, which colour `by` (its place in the colours given) holds, but the
    /// next free one within [`MISMATCH_DELTA_E`](super::MISMATCH_DELTA_E).
    NextFree { by: Option<usize> },
    /// Its nearest thread, which colour `with` holds too: no other thread within
    /// [`MISMATCH_DELTA_E`](super::MISMATCH_DELTA_E) was free, so the two are sewn in one thread, merged.
    Shared { with: usize },
}

impl MatchReason {
    /// Its id in replies.
    pub fn id(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Chosen => "chosen",
            Self::Nearest => "nearest",
            Self::NextFree { .. } => "nextFree",
            Self::Shared { .. } => "shared",
        }
    }
}

/// The thread a colour is matched to, why, and the threads nearest it after that one.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourMatch {
    pub thread: Match,
    pub reason: MatchReason,
    /// The nearest threads after [`ColourMatch::thread`], nearest first, at most
    /// [`ALTERNATIVES_SHOWN`].
    pub alternatives: Vec<Match>,
}

/// More colours than [`Palette::match_colours`] matches at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooManyColours {
    pub given: usize,
    pub max: usize,
}

impl std::fmt::Display for TooManyColours {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} colours, more than the {} matched to threads at once", self.given, self.max)
    }
}

impl std::error::Error for TooManyColours {}

impl Palette {
    /// Whether thread `index` (from 1) is neutral: its chroma under [`NEUTRAL_CHROMA`]. `false`
    /// for a number that names no thread.
    pub fn is_neutral(self, index: usize) -> bool {
        self.is_thread(index)
            && index.checked_sub(1).and_then(|i| self.labs().get(i)).is_some_and(|lab| (lab.a * lab.a + lab.b * lab.b).sqrt() < NEUTRAL_CHROMA)
    }

    /// The thread each of `colours` is sewn in, chosen together (this module's rules), with why and
    /// the threads nearest it after that one; in the order the colours are given, whatever that
    /// order is. Refused past [`MAX_COLOURS_MATCHED`] colours.
    pub fn match_colours(self, colours: &[ColourToMatch], threads: Threads) -> Result<Vec<ColourMatch>, TooManyColours> {
        if colours.len() > MAX_COLOURS_MATCHED {
            return Err(TooManyColours { given: colours.len(), max: MAX_COLOURS_MATCHED });
        }
        let candidates: Vec<usize> =
            (1..=self.threads().len()).filter(|&i| self.is_thread(i) && (threads == Threads::All || self.is_neutral(i))).collect();
        let labs = self.labs();
        // Each colour's difference from each candidate, in the candidates' order.
        let lab_of = |i: usize| i.checked_sub(1).and_then(|k| labs.get(k)).copied();
        let differences: Vec<Vec<f64>> = colours
            .iter()
            .map(|c| {
                let lab = color::lab(c.color);
                candidates.iter().map(|&t| lab_of(t).map_or(f64::INFINITY, |l| color::delta_e(lab, l))).collect()
            })
            .collect();
        // The order the assignment is made in: the largest first, then by sRGB, then as given.
        let mut order: Vec<usize> = (0..colours.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&colours[a], &colours[b]);
            y.area.total_cmp(&x.area).then(x.color.cmp(&y.color)).then(a.cmp(&b))
        });
        let own = |c: usize| candidates.iter().position(|&t| self.get(t).is_some_and(|(rgb, _)| rgb == colours[c].color));
        // A thread chosen from this palette, or a colour that is a thread's own, is that thread.
        let mut held: Vec<Option<(usize, MatchReason)>> = vec![None; colours.len()];
        for &c in &order {
            let chosen = colours[c].chosen.filter(|t| t.palette() == self).and_then(|t| candidates.iter().position(|&k| k == t.index()));
            held[c] = match (chosen, own(c)) {
                (Some(k), _) => Some((k, MatchReason::Chosen)),
                (None, Some(k)) => Some((k, MatchReason::Own)),
                (None, None) => None,
            };
        }
        let taken: Vec<usize> = held.iter().flatten().map(|(k, _)| *k).collect();
        // The rest are assigned among the threads not taken: one row a colour, one column a free
        // thread, and one column a colour for leaving it without a thread of its own.
        let rows: Vec<usize> = order.iter().copied().filter(|&c| held[c].is_none()).collect();
        let free: Vec<usize> = (0..candidates.len()).filter(|k| !taken.contains(k)).collect();
        let cost: Vec<Vec<f64>> = rows
            .iter()
            .map(|&c| {
                let mut row: Vec<f64> = free.iter().map(|&k| differences[c].get(k).copied().filter(|&d| !mismatched(d)).unwrap_or(TOO_FAR)).collect();
                row.extend(std::iter::repeat_n(UNMATCHED, rows.len()));
                row
            })
            .collect();
        let assigned = assignment(&cost, free.len() + rows.len());
        for (r, &c) in rows.iter().enumerate() {
            held[c] = assigned.get(r).and_then(|&col| free.get(col)).map(|&k| (k, MatchReason::Nearest));
        }
        // Who holds each thread, in the order of the assignment: the first holder of a shared one.
        let holder = |k: usize, held: &[Option<(usize, MatchReason)>]| order.iter().copied().find(|&c| held[c].is_some_and(|(h, _)| h == k));
        let nearest = |c: usize| (0..candidates.len()).min_by(|&a, &b| differences[c][a].total_cmp(&differences[c][b]).then(a.cmp(&b)));
        let mut out: Vec<Option<ColourMatch>> = vec![None; colours.len()];
        // Colours with a thread of their own: its nearest, or the next free one.
        let snapshot = held.clone();
        for &c in &order {
            let Some((k, reason)) = snapshot[c] else { continue };
            let reason = match (reason, nearest(c)) {
                (MatchReason::Nearest, Some(n)) if n != k && differences[c][n] < differences[c][k] => {
                    MatchReason::NextFree { by: holder(n, &snapshot).filter(|&h| h != c) }
                }
                (r, _) => r,
            };
            out[c] = Some(self.matched(&candidates, &differences[c], k, reason));
        }
        // Colours left without one: their nearest thread, shared with whoever holds it.
        for &c in &order {
            if out[c].is_some() {
                continue;
            }
            let Some(n) = nearest(c) else { continue };
            let reason = match holder(n, &held) {
                Some(h) if h != c => MatchReason::Shared { with: h },
                _ => MatchReason::Nearest,
            };
            held[c] = Some((n, reason));
            out[c] = Some(self.matched(&candidates, &differences[c], n, reason));
        }
        // A palette has threads, so every colour got one: the stand-in is never taken.
        Ok(out
            .into_iter()
            .map(|m| {
                m.unwrap_or(ColourMatch { thread: Match { index: 1, delta_e: f64::INFINITY }, reason: MatchReason::Nearest, alternatives: vec![] })
            })
            .collect())
    }

    /// A colour's match: candidate `k` with `reason`, and the nearest candidates after it.
    fn matched(self, candidates: &[usize], differences: &[f64], k: usize, reason: MatchReason) -> ColourMatch {
        let at = |i: usize| Match { index: candidates.get(i).copied().unwrap_or(1), delta_e: differences.get(i).copied().unwrap_or(f64::INFINITY) };
        let mut rest: Vec<usize> = (0..candidates.len()).filter(|&i| i != k).collect();
        rest.sort_by(|&a, &b| differences[a].total_cmp(&differences[b]).then(a.cmp(&b)));
        ColourMatch { thread: at(k), reason, alternatives: rest.into_iter().take(ALTERNATIVES_SHOWN).map(at).collect() }
    }
}

/// The columns of `cost` (one row a colour, `cols` columns, at least as many as rows) the rows are
/// assigned to, one each, at the least total cost: the Hungarian method's shortest-augmenting-path
/// form (this module's sources). `cost[r][c]` is finite; a missing entry counts as [`TOO_FAR`].
fn assignment(cost: &[Vec<f64>], cols: usize) -> Vec<usize> {
    let rows = cost.len();
    if rows == 0 || cols < rows {
        return vec![];
    }
    let at = |r: usize, c: usize| cost.get(r).and_then(|row| row.get(c)).copied().unwrap_or(TOO_FAR);
    // 1-based, with row and column 0 standing for none: the method's own bookkeeping.
    let (mut u, mut v) = (vec![0.0f64; rows + 1], vec![0.0f64; cols + 1]);
    let (mut row_of, mut way) = (vec![0usize; cols + 1], vec![0usize; cols + 1]);
    for r in 1..=rows {
        row_of[0] = r;
        let mut col = 0usize;
        let mut least = vec![f64::INFINITY; cols + 1];
        let mut used = vec![false; cols + 1];
        loop {
            used[col] = true;
            let r0 = row_of[col];
            let (mut delta, mut next) = (f64::INFINITY, 0usize);
            for c in 1..=cols {
                if used[c] {
                    continue;
                }
                let reduced = at(r0 - 1, c - 1) - u[r0] - v[c];
                if reduced < least[c] {
                    least[c] = reduced;
                    way[c] = col;
                }
                if least[c] < delta {
                    delta = least[c];
                    next = c;
                }
            }
            for c in 0..=cols {
                if used[c] {
                    u[row_of[c]] += delta;
                    v[c] -= delta;
                } else {
                    least[c] -= delta;
                }
            }
            col = next;
            if col == 0 || row_of[col] == 0 {
                break;
            }
        }
        // Along the augmenting path back to the start.
        while col != 0 {
            let prev = way[col];
            row_of[col] = row_of[prev];
            col = prev;
        }
    }
    let mut out = vec![0usize; rows];
    for (c, &r) in row_of.iter().enumerate().skip(1) {
        if let Some(slot) = r.checked_sub(1).and_then(|r| out.get_mut(r)) {
            *slot = c - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every assignment of `rows` rows to distinct columns of `cost`, its least total: brute force,
    /// for small sizes.
    fn least_total(cost: &[Vec<f64>], cols: usize) -> f64 {
        fn go(cost: &[Vec<f64>], r: usize, used: &mut Vec<bool>, sum: f64, best: &mut f64) {
            if r == cost.len() {
                *best = best.min(sum);
                return;
            }
            for c in 0..used.len() {
                if !used[c] {
                    used[c] = true;
                    go(cost, r + 1, used, sum + cost[r][c], best);
                    used[c] = false;
                }
            }
        }
        let mut best = f64::INFINITY;
        go(cost, 0, &mut vec![false; cols], 0.0, &mut best);
        best
    }

    /// The assignment is a least-cost one, each row to its own column: against brute force on
    /// small matrices of every shape the method takes.
    #[test]
    fn the_assignment_costs_the_least() {
        let mut seed = 0x5717_c4edu64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 1000) as f64 / 10.0
        };
        for rows in 1..=5 {
            for cols in rows..=6 {
                for _ in 0..20 {
                    let cost: Vec<Vec<f64>> = (0..rows).map(|_| (0..cols).map(|_| next()).collect()).collect();
                    let got = assignment(&cost, cols);
                    assert_eq!(got.len(), rows);
                    let mut seen = got.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(seen.len(), rows, "distinct columns: {got:?}");
                    let total: f64 = got.iter().enumerate().map(|(r, &c)| cost[r][c]).sum();
                    assert!((total - least_total(&cost, cols)).abs() < 1e-9, "{rows} × {cols}: {total} against the least");
                }
            }
        }
        assert!(assignment(&[], 3).is_empty());
        assert!(assignment(&vec![vec![1.0, 2.0]; 3], 2).is_empty(), "more rows than columns");
    }

    /// The number of `palette`'s thread called `name`.
    fn thread_named(palette: Palette, name: &str) -> usize {
        palette.threads().iter().position(|(_, n)| *n == name).map(|i| i + 1).unwrap()
    }

    fn colour(color: [u8; 3], area: f64) -> ColourToMatch {
        ColourToMatch { color, area, chosen: None }
    }

    /// The fox's colours in Brother's palette: its orange near Tangerine, its cream and its ears
    /// farther than the line from every thread (flagged), its black a thread's own; in Janome's, all
    /// within it.
    #[test]
    fn the_fox_in_both_palettes() {
        let fox = [colour([0xf4, 0x90, 0x10], 60.0), colour([0xff, 0xd9, 0x83], 20.0), colour([0xa0, 0x04, 0x1e], 10.0), colour([0, 0, 0], 5.0)];
        let pec = Palette::Pec.match_colours(&fox, Threads::All).unwrap();
        let name = |p: Palette, m: &ColourMatch| p.get(m.thread.index).map(|(_, n)| n).unwrap();
        assert_eq!(name(Palette::Pec, &pec[3]), "Black");
        assert_eq!(pec[3].reason, MatchReason::Own);
        assert_eq!(pec[3].thread.delta_e, 0.0);
        assert!(mismatched(pec[2].thread.delta_e), "the ears: {}", pec[2].thread.delta_e);
        assert!(mismatched(pec[1].thread.delta_e), "the cream: {}", pec[1].thread.delta_e);
        assert!(!mismatched(pec[0].thread.delta_e));
        assert_eq!(pec.iter().map(|m| m.alternatives.len()).collect::<Vec<_>>(), [ALTERNATIVES_SHOWN; 4]);
        let jef = Palette::Jef.match_colours(&fox, Threads::All).unwrap();
        assert!(jef.iter().all(|m| !mismatched(m.thread.delta_e)), "{jef:?}");
    }

    /// Two colours near one thread each get one of their own while the palette has another within
    /// the line; a person's choice is kept; the order the colours come in changes nothing.
    #[test]
    fn colours_near_one_thread_share_it_only_past_the_line() {
        let pec = Palette::Pec;
        // The watermelon's two reds, whose nearest Brother thread is the same one: both have
        // another within the line.
        let reds = [colour([0xdd, 0x2e, 0x44], 30.0), colour([0xc4, 0x00, 0x06], 20.0)];
        assert_eq!(pec.nearest(reds[0].color, &[]).index, pec.nearest(reds[1].color, &[]).index, "one nearest thread");
        let m = pec.match_colours(&reds, Threads::All).unwrap();
        assert_ne!(m[0].thread.index, m[1].thread.index, "{m:?}");
        assert!(
            matches!(m[1].reason, MatchReason::NextFree { by: Some(0) }) || matches!(m[0].reason, MatchReason::NextFree { by: Some(1) }),
            "{m:?}"
        );
        assert!(m.iter().all(|m| !mismatched(m.thread.delta_e)));
        let turned = pec.match_colours(&[reds[1], reds[0]], Threads::All).unwrap();
        assert_eq!((turned[1].thread, turned[0].thread), (m[0].thread, m[1].thread), "order-free");
        // A choice is kept, and the other colour takes another.
        let shared = pec.nearest(reds[0].color, &[]).index;
        let chosen = [ColourToMatch { chosen: PaletteThread::new(pec, shared), ..reds[1] }, reds[0]];
        let m = pec.match_colours(&chosen, Threads::All).unwrap();
        assert_eq!((m[0].thread.index, m[0].reason), (shared, MatchReason::Chosen));
        assert_ne!(m[1].thread.index, shared);
        assert!(thread_named(pec, "Red") > 0);
        // Janome's two near blacks: nothing but Black within the line of either, so they share it.
        let jef = Palette::Jef;
        let blacks = [colour([0, 0, 0], 30.0), colour([0x29, 0x2f, 0x33], 10.0)];
        let m = jef.match_colours(&blacks, Threads::All).unwrap();
        assert_eq!(m[0].thread.index, m[1].thread.index);
        assert_eq!(m[1].reason, MatchReason::Shared { with: 0 });
    }

    /// Greys are matched to neutral threads only: every one of their threads is under the chroma
    /// line, and the palettes have 7 (Brother) and 6 (Janome) of them.
    #[test]
    fn greys_take_neutral_threads() {
        let neutral = |p: Palette| (1..=p.threads().len()).filter(|&i| p.is_neutral(i)).count();
        assert_eq!((neutral(Palette::Pec), neutral(Palette::Jef)), (7, 6));
        let greys = [colour([40, 40, 40], 5.0), colour([128, 128, 128], 4.0), colour([200, 200, 200], 3.0), colour([120, 130, 160], 2.0)];
        for p in Palette::ALL {
            let m = p.match_colours(&greys, Threads::Neutral).unwrap();
            assert!(m.iter().all(|m| p.is_neutral(m.thread.index)), "{p:?}: {m:?}");
            assert!(m.iter().flat_map(|m| &m.alternatives).all(|a| p.is_neutral(a.index)));
        }
        let too_many = vec![colour([1, 2, 3], 1.0); MAX_COLOURS_MATCHED + 1];
        assert_eq!(Palette::Jef.match_colours(&too_many, Threads::All).unwrap_err().max, MAX_COLOURS_MATCHED);
        assert!(Palette::Jef.match_colours(&[], Threads::All).unwrap().is_empty());
    }
}
