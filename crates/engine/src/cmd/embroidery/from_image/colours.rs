//! The image's colours as a design's threads: each colour the trace found matched to a thread of the
//! document's palette by the thread module ([`Palette::match_colours`]: CIEDE2000, the threads chosen
//! together), with its area and its share, whether it is the background, and what it is sewn as.
//!
//! The trace keeps the image's own colours, so the difference between each colour and its thread is
//! known and shown, and a colour no thread comes near is flagged (past `thread::MISMATCH_DELTA_E`).
//! Two colours whose only thread within that line is the same one are merged: sewn in it, as one
//! (the project owner's choice, 2026-11-01). A person's choices for a colour (its thread, a merge
//! into another, what it is sewn as) are found again by colour: each applies to the colours within
//! the line of it, the nearest.
//!
//! The list is made in two steps around the rules for areas (`super::rules`): first each colour's
//! traced areas and what it is sewn as ([`rows`]), then, once the smallest areas have joined their
//! neighbours, each colour's threads by the areas it still has ([`matched`]), so that a colour whose
//! every area was a speck takes no thread from another.

use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::thread::{ColourMatch, ColourToMatch, MISMATCH_DELTA_E, MatchReason, Palette, Threads};
use vectorcraft_trace as tr;

use super::background::Background;
use super::settings::{C, ColorChoice, Job, Mode};
use crate::Result;
use crate::cmd::bad;

/// One row of the colour list: a colour of the image and what becomes of it.
#[derive(Clone, Debug)]
pub(super) struct Colour {
    /// The image's own colour (the trace's).
    pub(super) color: [u8; 3],
    /// Whether these are the background's areas: the colour's areas touching the image's edge,
    /// never sewn.
    pub(super) background: bool,
    /// The traced paths of this row (their places in the trace).
    pub(super) paths: Vec<usize>,
    /// Its pixels and its areas, once the smallest areas have joined their neighbours ([`matched`];
    /// before that, the traced paths').
    pub(super) pixels: usize,
    pub(super) areas: usize,
    /// The thread it is sewn in, why, and the threads nearest it after that one; `None` for the
    /// background, and for a colour left without areas.
    pub(super) thread: Option<ColourMatch>,
    /// The row it is merged into: its areas sewn as that row's, in its thread.
    pub(super) merged_into: Option<usize>,
    /// What it is sewn as.
    pub(super) job: Job,
    /// The person's choice that applies to it (its place in `colorChoices`).
    pub(super) choice: Option<usize>,
}

impl Colour {
    /// Whether its areas are sewn: neither the background nor a colour left unsewn.
    pub(super) fn sewn(&self) -> bool {
        !self.background && self.job != Job::None
    }
}

/// The colour list of a trace: which colours there are, and what becomes of each.
pub(super) struct Colours {
    pub(super) rows: Vec<Colour>,
    /// The choices no colour took (their places in `colorChoices`).
    pub(super) unused: Vec<usize>,
}

impl Colours {
    /// Each traced path's row, by the path's place in the trace (`0` for a path in no row, which
    /// only a stroke is).
    pub(super) fn row_of(&self, paths: usize) -> Vec<usize> {
        let mut out = vec![0; paths];
        for (r, row) in self.rows.iter().enumerate() {
            for &p in &row.paths {
                if let Some(slot) = out.get_mut(p) {
                    *slot = r;
                }
            }
        }
        out
    }
}

/// The traced colour that is the background `found`: the one nearest it, within the line
/// (`MISMATCH_DELTA_E`); none when no traced colour is that near.
pub(super) fn background_colour(palette: &[[u8; 3]], found: Background) -> Option<[u8; 3]> {
    let Background::Colour(bg) = found else { return None };
    palette
        .iter()
        .map(|&c| (c, delta_e_rgb(c, bg)))
        .filter(|&(_, d)| d <= MISMATCH_DELTA_E)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(c, _)| c)
}

/// The choice of `choices` nearest `color`, within the line.
fn nearest_choice(choices: &[ColorChoice], color: [u8; 3]) -> Option<usize> {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| (i, delta_e_rgb(color, c.color)))
        .filter(|&(_, d)| d <= MISMATCH_DELTA_E)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|(i, _)| i)
}

/// The colour list's rows of `traced`, whose background is the traced colour `background` (`None`:
/// none): one row for each colour's areas and one for the background's (the areas of its colour
/// that `touches_edge` says touch the image's edge), the background first, then the largest; each
/// with the person's choice nearest it, and what that choice sews it as.
pub(super) fn rows(traced: &tr::TraceResult, touches_edge: &dyn Fn(usize) -> bool, background: Option<[u8; 3]>, choices: &[ColorChoice]) -> Colours {
    let mut rows: Vec<Colour> = Vec::new();
    for (i, p) in traced.paths.iter().enumerate() {
        let is_background = background == Some(p.color) && touches_edge(i);
        let found = rows.iter().position(|r| r.color == p.color && r.background == is_background);
        let r = match found {
            Some(r) => r,
            None => {
                rows.push(Colour {
                    color: p.color,
                    background: is_background,
                    paths: vec![],
                    pixels: 0,
                    areas: 0,
                    thread: None,
                    merged_into: None,
                    job: Job::Auto,
                    choice: None,
                });
                rows.len() - 1
            }
        };
        if let Some(row) = rows.get_mut(r) {
            row.paths.push(i);
            row.pixels += p.pixels;
            row.areas += 1;
        }
    }
    rows.sort_by(|a, b| b.background.cmp(&a.background).then(b.pixels.cmp(&a.pixels)).then(a.color.cmp(&b.color)));
    let mut used = vec![false; choices.len()];
    for row in rows.iter_mut().filter(|r| !r.background) {
        row.choice = nearest_choice(choices, row.color);
        if let Some(u) = row.choice.and_then(|i| used.get_mut(i)) {
            *u = true;
        }
        if let Some(job) = row.choice.and_then(|i| choices.get(i)).and_then(|c| c.job) {
            row.job = job;
        }
    }
    let unused = used.iter().enumerate().filter(|(_, u)| !**u).map(|(i, _)| i).collect();
    Colours { rows, unused }
}

/// The colour list once its areas are known: `areas` holds, for each area left, its row and its
/// pixels. Each row is counted again by them (a colour left without areas is kept, with none, and
/// takes no thread); the merges a person asked for are made; then the threads are chosen together
/// for the rows that keep their own colour and are sewn (neutral threads only in grayscale mode),
/// a colour left unsewn given its nearest; two colours sharing their only thread within the line
/// are merged.
pub(super) fn matched(mut colours: Colours, areas: &[(usize, usize)], mode: Mode, palette: Palette, choices: &[ColorChoice]) -> Result<Colours> {
    for row in colours.rows.iter_mut() {
        (row.pixels, row.areas) = (0, 0);
    }
    for &(r, pixels) in areas {
        if let Some(row) = colours.rows.get_mut(r) {
            row.pixels += pixels;
            row.areas += 1;
        }
    }
    let rows = &mut colours.rows;
    let choice_of = |row: &Colour| row.choice.and_then(|i| choices.get(i));
    // Merges asked for: into the row with areas nearest the colour named, within the line.
    let targets: Vec<Option<usize>> = rows
        .iter()
        .map(|row| {
            let into = choice_of(row).and_then(|c| c.merge_into)?;
            rows.iter()
                .enumerate()
                .filter(|(_, r)| !r.background && r.areas > 0 && r.color != row.color)
                .map(|(j, r)| (j, delta_e_rgb(r.color, into)))
                .filter(|&(_, d)| d <= MISMATCH_DELTA_E)
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map(|(j, _)| j)
        })
        .collect();
    // Each merge followed to the row it ends in (a loop ends at the last row before it repeats).
    let end = |mut i: usize| {
        let mut seen = vec![i];
        while let Some(next) = targets.get(i).copied().flatten() {
            if seen.contains(&next) {
                break;
            }
            seen.push(next);
            i = next;
        }
        i
    };
    for i in 0..rows.len() {
        let e = end(i);
        if let Some(row) = rows.get_mut(i) {
            row.merged_into = (e != i).then_some(e);
        }
    }
    // A merged row is sewn as the row it ends in.
    for i in 0..rows.len() {
        let job = rows.get(i).and_then(|r| r.merged_into).and_then(|j| rows.get(j)).map(|t| t.job);
        if let (Some(row), Some(job)) = (rows.get_mut(i), job) {
            row.job = job;
        }
    }
    // Threads for the sewn rows with areas that keep their own colour, chosen together.
    let threads = if mode == Mode::Grayscale { Threads::Neutral } else { Threads::All };
    let keeps = |r: &Colour| !r.background && r.areas > 0 && r.merged_into.is_none();
    let together: Vec<usize> = (0..rows.len()).filter(|&i| rows.get(i).is_some_and(|r| keeps(r) && r.sewn())).collect();
    let asked: Vec<ColourToMatch> = together
        .iter()
        .filter_map(|&i| rows.get(i))
        .map(|r| ColourToMatch {
            color: r.color,
            area: r.pixels as f64,
            chosen: choice_of(r).and_then(|c| c.thread).filter(|t| t.palette() == palette),
        })
        .collect();
    let got = palette.match_colours(&asked, threads).map_err(|e| bad(C, format!("the image has {e}: lower `colors`")))?;
    for (&i, m) in together.iter().zip(got) {
        // A shared thread merges the colour into the one that holds it.
        if let MatchReason::Shared { with } = m.reason
            && let Some(&into) = together.get(with)
            && let Some(row) = rows.get_mut(i)
        {
            row.merged_into = Some(into);
        }
        if let Some(row) = rows.get_mut(i) {
            row.thread = Some(m);
        }
    }
    // A colour left unsewn keeps its own thread, its nearest (or the one chosen for it), for when
    // it is sewn later: it takes none from the colours sewn.
    for i in 0..rows.len() {
        let Some(row) = rows.get(i).filter(|r| keeps(r) && !r.sewn()) else { continue };
        let chosen = choice_of(row).and_then(|c| c.thread).filter(|t| t.palette() == palette);
        let ask = [ColourToMatch { color: row.color, area: row.pixels as f64, chosen }];
        let m = palette.match_colours(&ask, threads).ok().and_then(|mut v| v.pop());
        if let Some(row) = rows.get_mut(i) {
            row.thread = m;
        }
    }
    // A merged row is sewn in the thread of the row it ends in.
    for i in 0..rows.len() {
        let thread = rows.get(i).and_then(|r| r.merged_into).and_then(|j| rows.get(j)).and_then(|r| r.thread.clone());
        if let (Some(row), Some(t)) = (rows.get_mut(i), thread) {
            row.thread = Some(t);
        }
    }
    Ok(colours)
}
