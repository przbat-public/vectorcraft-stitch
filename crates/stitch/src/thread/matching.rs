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
//! The assignment is the shortest augmenting path method, written from its description in R.
//! Jonker and A. Volgenant, "A shortest augmenting path algorithm for dense and sparse linear
//! assignment problems", *Computing* 38, 1987 (its augmentation phase; the initialising phases
//! before it are left out): the colours are added one at a time, each by the path of least reduced
//! cost from it to a thread no colour holds yet, found as E. W. Dijkstra's method finds a shortest
//! path ("A note on two problems in connexion with graphs", *Numerische Mathematik* 1, 1959), and a
//! value for each colour and each thread keeps every reduced cost at least 0 and those of the pairs
//! assigned 0: the duality on which H. W. Kuhn's Hungarian method rests ("The Hungarian method for
//! the assignment problem", *Naval Research Logistics Quarterly* 2, 1955), which makes the total
//! the least.

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
    /// The thread a person chose for it, which it is sewn in whatever `threads` allows (a person
    /// may choose a coloured thread for a picture in greys). One of another palette is no choice
    /// here (the caller matches its colour instead).
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
        // The threads `threads` allows, and every thread a person chose: a chosen thread is a
        // candidate for its own colour only, so that no other colour is given it.
        let allowed = self.candidates(threads);
        let mut candidates = allowed.clone();
        for t in colours.iter().filter_map(|c| c.chosen).filter(|t| t.palette() == self && self.is_thread(t.index())) {
            if let Err(at) = candidates.binary_search(&t.index()) {
                candidates.insert(at, t.index());
            }
        }
        let labs = self.labs();
        // Each colour's difference from each candidate, in the candidates' order: none (infinite)
        // from a thread it may not be given.
        let lab_of = |i: usize| i.checked_sub(1).and_then(|k| labs.get(k)).copied();
        let differences: Vec<Vec<f64>> = colours
            .iter()
            .map(|c| {
                let lab = color::lab(c.color);
                let own_choice = |t: usize| c.chosen.is_some_and(|ch| ch.palette() == self && ch.index() == t);
                candidates
                    .iter()
                    .map(|&t| {
                        let may = allowed.binary_search(&t).is_ok() || own_choice(t);
                        lab_of(t).filter(|_| may).map_or(f64::INFINITY, |l| color::delta_e(lab, l))
                    })
                    .collect()
            })
            .collect();
        // The order the assignment is made in: the largest first, then by sRGB, then as given.
        let mut order: Vec<usize> = (0..colours.len()).collect();
        order.sort_by(|&a, &b| {
            let (x, y) = (&colours[a], &colours[b]);
            y.area.total_cmp(&x.area).then(x.color.cmp(&y.color)).then(a.cmp(&b))
        });
        let own = |c: usize| {
            candidates
                .iter()
                .enumerate()
                .position(|(k, &t)| differences[c][k].is_finite() && self.get(t).is_some_and(|(rgb, _)| rgb == colours[c].color))
        };
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

    /// The threads a colour may be matched to: the palette's (its appliqué entries left out), or
    /// its neutral ones.
    fn candidates(self, threads: Threads) -> Vec<usize> {
        (1..=self.threads().len()).filter(|&i| self.is_thread(i) && (threads == Threads::All || self.is_neutral(i))).collect()
    }

    /// Colour `color` sewn in thread `index` (from 1) with `reason`: its own difference from that
    /// thread, and the threads of `threads` nearest it after that one, as
    /// [`Palette::match_colours`] gives a colour's (the thread among them whatever `threads` says).
    /// For a colour sewn in a thread another colour holds, merged into it, so that it shows its own
    /// difference, flag and alternatives. `None` for a number that names no thread.
    pub fn match_to(self, color: [u8; 3], index: usize, reason: MatchReason, threads: Threads) -> Option<ColourMatch> {
        if !self.is_thread(index) {
            return None;
        }
        let mut candidates = self.candidates(threads);
        if !candidates.contains(&index) {
            candidates.push(index);
            candidates.sort_unstable();
        }
        let lab = color::lab(color);
        let labs = self.labs();
        let differences: Vec<f64> =
            candidates.iter().map(|&t| t.checked_sub(1).and_then(|k| labs.get(k)).map_or(f64::INFINITY, |l| color::delta_e(lab, *l))).collect();
        let k = candidates.iter().position(|&t| t == index)?;
        Some(self.matched(&candidates, &differences, k, reason))
    }

    /// A colour's match: candidate `k` with `reason`, and the nearest candidates after it.
    fn matched(self, candidates: &[usize], differences: &[f64], k: usize, reason: MatchReason) -> ColourMatch {
        let at = |i: usize| Match { index: candidates.get(i).copied().unwrap_or(1), delta_e: differences.get(i).copied().unwrap_or(f64::INFINITY) };
        let mut rest: Vec<usize> = (0..candidates.len()).filter(|&i| i != k && differences.get(i).is_some_and(|d| d.is_finite())).collect();
        rest.sort_by(|&a, &b| differences[a].total_cmp(&differences[b]).then(a.cmp(&b)));
        ColourMatch { thread: at(k), reason, alternatives: rest.into_iter().take(ALTERNATIVES_SHOWN).map(at).collect() }
    }
}

/// The column of `cost` each row is assigned to, a column to at most one row, at the least total
/// cost (one row a colour, `cols` columns, at least as many as rows; `cost[r][c]` finite, a missing
/// entry [`TOO_FAR`]): by shortest augmenting paths (this module's sources). Each row is added by
/// the path of least reduced cost from it to a column no row holds, each step along a column
/// another row holds to that row and on, found as Dijkstra's method finds a shortest path; the
/// duals then move so that every reduced cost stays at least 0 and the path's are 0, and the path's
/// columns pass along it. Empty for no rows, or for fewer columns than rows.
fn assignment(cost: &[Vec<f64>], cols: usize) -> Vec<usize> {
    let rows = cost.len();
    if rows == 0 || cols < rows {
        return vec![];
    }
    let at = |r: usize, c: usize| cost.get(r).and_then(|row| row.get(c)).copied().unwrap_or(TOO_FAR);
    // A value for each row and column; a pair's reduced cost is its cost less both. Each row's
    // starts at its least cost, so that no reduced cost starts below 0.
    let mut row_value: Vec<f64> = (0..rows).map(|r| (0..cols).map(|c| at(r, c)).fold(f64::INFINITY, f64::min)).collect();
    let mut col_value = vec![0.0f64; cols];
    let mut holder: Vec<Option<usize>> = vec![None; cols];
    let mut column_of: Vec<Option<usize>> = vec![None; rows];
    for start in 0..rows {
        let reduced = |r: usize, c: usize, row_value: &[f64], col_value: &[f64]| {
            at(r, c) - row_value.get(r).copied().unwrap_or(0.0) - col_value.get(c).copied().unwrap_or(0.0)
        };
        // Each column's least reduced cost from `start` so far, the row it is reached from, and
        // whether it is settled.
        let mut dist: Vec<f64> = (0..cols).map(|c| reduced(start, c, &row_value, &col_value)).collect();
        let mut from: Vec<usize> = vec![start; cols];
        let mut settled = vec![false; cols];
        let end = loop {
            // The nearest column not settled, the first of those as near.
            let next = (0..cols).filter(|&c| !settled[c]).min_by(|&a, &b| dist[a].total_cmp(&dist[b]).then(a.cmp(&b)));
            let Some(j) = next else { break None };
            settled[j] = true;
            let Some(i) = holder[j] else { break Some(j) };
            // On through the row holding it, whose pair with it costs nothing reduced.
            for c in 0..cols {
                if !settled[c] {
                    let through = dist[j] + reduced(i, c, &row_value, &col_value);
                    if through < dist[c] {
                        dist[c] = through;
                        from[c] = i;
                    }
                }
            }
        };
        // At least as many columns as rows: a free one is always reached.
        let Some(end) = end else { return vec![] };
        // The values: each settled column lowered by what it lacked of the path's length, the row
        // holding it raised by as much, and the start raised by the whole length.
        let length = dist[end];
        row_value[start] += length;
        for c in 0..cols {
            if settled[c] {
                let lack = length - dist[c];
                col_value[c] -= lack;
                if let Some(i) = holder[c] {
                    row_value[i] += lack;
                }
            }
        }
        // Along the path back from its end: each column to the row it was reached from, that row's
        // old column to the row before it.
        let mut c = end;
        loop {
            let i = from[c];
            let before = column_of[i];
            holder[c] = Some(i);
            column_of[i] = Some(c);
            match before {
                Some(b) if i != start => c = b,
                _ => break,
            }
        }
    }
    column_of.into_iter().map(|c| c.unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread a person chose is the colour's in greys too, though it isn't grey; the other colours
    /// still get grey threads.
    #[test]
    fn a_chosen_thread_is_used_in_greys() {
        let p = Palette::Jef;
        let pink = p.threads().iter().position(|(_, n)| *n == "Pink").map(|i| i + 1).unwrap();
        assert!(!p.is_neutral(pink));
        let chosen = PaletteThread::new(p, pink);
        let colours = [ColourToMatch { color: [128, 128, 128], area: 2.0, chosen }, ColourToMatch { color: [40, 40, 40], area: 1.0, chosen: None }];
        let got = p.match_colours(&colours, Threads::Neutral).unwrap();
        assert_eq!((got[0].thread.index, got[0].reason), (pink, MatchReason::Chosen));
        assert!(p.is_neutral(got[1].thread.index), "{:?}", got[1]);
        // A colour that is pink itself, not choosing it, gets a grey all the same, and lists no pink.
        let (pink_rgb, _) = p.get(pink).unwrap();
        let colours = [colours[0], ColourToMatch { color: pink_rgb, area: 1.0, chosen: None }];
        let got = p.match_colours(&colours, Threads::Neutral).unwrap();
        assert!(p.is_neutral(got[1].thread.index), "{:?}", got[1]);
        assert!(got[1].alternatives.iter().all(|a| p.is_neutral(a.index)), "{:?}", got[1].alternatives);
    }

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

    /// Ties and far pairs don't fool it: matrices of a few costs, many assignments equally good,
    /// some pairs at [`TOO_FAR`], and as many more columns as rows, each still a least one against
    /// brute force.
    #[test]
    fn the_assignment_costs_the_least_through_ties() {
        let mut seed = 0x7135_e1d5u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for rows in 1..=5 {
            for cols in rows..=(2 * rows).min(7) {
                for _ in 0..30 {
                    let cost: Vec<Vec<f64>> =
                        (0..rows).map(|_| (0..cols).map(|_| [0.0, 1.0, 1.0, 2.0, TOO_FAR][(next() % 5) as usize]).collect()).collect();
                    let got = assignment(&cost, cols);
                    let mut seen = got.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(seen.len(), rows, "distinct columns: {got:?}");
                    let total: f64 = got.iter().enumerate().map(|(r, &c)| cost[r][c]).sum();
                    assert_eq!(total, least_total(&cost, cols), "{rows} × {cols}: {cost:?}");
                }
            }
        }
        // Every cost the same: any one column each.
        let flat = vec![vec![3.0; 4]; 4];
        assert_eq!(assignment(&flat, 4).iter().map(|&c| flat[0][c]).sum::<f64>(), 12.0);
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

    /// A colour sewn in a given thread gets its own difference from it and its own nearest threads
    /// after it, as a colour matched alone does; a number that names no thread is none.
    #[test]
    fn a_colour_sewn_in_a_given_thread_keeps_its_own_difference() {
        let grey = [0x29, 0x2f, 0x33];
        let black = Palette::Jef.threads().iter().position(|(rgb, _)| *rgb == [0, 0, 0]).map(|i| i + 1).unwrap();
        let m = Palette::Jef.match_to(grey, black, MatchReason::Shared { with: 0 }, Threads::All).unwrap();
        assert_eq!(m.thread.index, black);
        assert!((m.thread.delta_e - color::delta_e_rgb(grey, [0, 0, 0])).abs() < 1e-9, "{m:?}");
        let alone = Palette::Jef.match_colours(&[colour(grey, 1.0)], Threads::All).unwrap().remove(0);
        let mut expected: Vec<Match> = std::iter::once(alone.thread).chain(alone.alternatives).filter(|a| a.index != black).collect();
        expected.truncate(ALTERNATIVES_SHOWN);
        assert_eq!(m.alternatives, expected);
        assert!(Palette::Jef.match_to(grey, 0, MatchReason::Nearest, Threads::All).is_none());
    }
}
