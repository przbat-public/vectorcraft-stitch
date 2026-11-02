//! The image's colours as a design's threads: each colour the trace found matched to a thread of the
//! document's palette by the thread module ([`Palette::match_colours`]: CIEDE2000, the threads chosen
//! together), with its area and its share, whether it is the background, and what it is sewn as.
//!
//! The trace keeps the image's own colours, so the difference between each colour and its thread is
//! known and shown, and a colour no thread comes near is flagged (past `thread::MISMATCH_DELTA_E`).
//! Two colours whose only thread within that line is the same one are merged: sewn in it, as one
//! (the project owner's choice, 2026-11-01). A person's choices for a colour (its thread, a merge
//! into another, not sewing it) are found again by colour: each applies to the colours within the
//! line of it, the nearest.

use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::thread::{ColourMatch, ColourToMatch, MISMATCH_DELTA_E, MatchReason, Palette, Threads};
use vectorcraft_trace as tr;

use super::background::Background;
use super::settings::{C, ColorChoice, Job, Mode};
use crate::Result;
use crate::cmd::bad;

/// How near a traced path's box may come to the working copy's edge (pixels) and touch it: half a
/// pixel, less than the sub-pixel shift a traced outline is moved by.
const EDGE_TOLERANCE_PX: f64 = 0.5;

/// One row of the colour list: a colour of the image and what becomes of it.
#[derive(Clone, Debug)]
pub(super) struct Colour {
    /// The image's own colour (the trace's).
    pub(super) color: [u8; 3],
    /// Whether these are the background's areas: the colour's areas touching the image's edge,
    /// never sewn.
    pub(super) background: bool,
    /// The traced paths of this row (indices into the trace's), and their pixels.
    pub(super) paths: Vec<usize>,
    pub(super) pixels: usize,
    /// The thread it is sewn in, why, and the threads nearest it after that one; `None` for the
    /// background.
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

/// Whether traced path `p` touches the edge of a `w × h` pixel working copy.
fn touches_edge(p: &tr::TracedPath, w: u32, h: u32) -> bool {
    p.path.bounds().is_some_and(|b| {
        b.x0 <= EDGE_TOLERANCE_PX || b.y0 <= EDGE_TOLERANCE_PX || b.x1 >= f64::from(w) - EDGE_TOLERANCE_PX || b.y1 >= f64::from(h) - EDGE_TOLERANCE_PX
    })
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

/// The colour list of `traced`, a `w × h` working copy whose background is the traced colour
/// `background` (`None`: none), in `palette`'s threads, with the person's `choices`: one row for
/// each colour's sewn areas (largest first) and one for the background's, each colour matched to a
/// thread (neutral threads only in grayscale mode), merges and jobs applied.
pub(super) fn listed(
    traced: &tr::TraceResult,
    (w, h): (u32, u32),
    background: Option<[u8; 3]>,
    mode: Mode,
    palette: Palette,
    choices: &[ColorChoice],
) -> Result<Colours> {
    // Each colour's areas, the background's apart.
    let mut rows: Vec<Colour> = Vec::new();
    for (i, p) in traced.paths.iter().enumerate() {
        let is_background = background == Some(p.color) && touches_edge(p, w, h);
        let row = match rows.iter_mut().find(|r| r.color == p.color && r.background == is_background) {
            Some(r) => r,
            None => {
                rows.push(Colour {
                    color: p.color,
                    background: is_background,
                    paths: vec![],
                    pixels: 0,
                    thread: None,
                    merged_into: None,
                    job: Job::Auto,
                    choice: None,
                });
                let Some(r) = rows.last_mut() else { continue };
                r
            }
        };
        row.paths.push(i);
        row.pixels += p.pixels;
    }
    // The background first, then the largest colours.
    rows.sort_by(|a, b| b.background.cmp(&a.background).then(b.pixels.cmp(&a.pixels)).then(a.color.cmp(&b.color)));
    // Each sewn row's choice: the nearest within the line.
    let mut used = vec![false; choices.len()];
    for row in rows.iter_mut().filter(|r| !r.background) {
        let nearest = choices
            .iter()
            .enumerate()
            .map(|(i, c)| (i, delta_e_rgb(row.color, c.color)))
            .filter(|&(_, d)| d <= MISMATCH_DELTA_E)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if let Some((i, _)) = nearest {
            row.choice = Some(i);
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
    }
    let choice_of = |row: &Colour| row.choice.and_then(|i| choices.get(i));
    for row in rows.iter_mut() {
        if let Some(job) = choice_of(row).and_then(|c| c.job) {
            row.job = job;
        }
    }
    // Merges asked for: into the sewn row nearest the colour named, within the line.
    let targets: Vec<Option<usize>> = rows
        .iter()
        .map(|row| {
            let into = choice_of(row).and_then(|c| c.merge_into)?;
            rows.iter()
                .enumerate()
                .filter(|(_, r)| !r.background && r.color != row.color)
                .map(|(j, r)| (j, delta_e_rgb(r.color, into)))
                .filter(|&(_, d)| d <= MISMATCH_DELTA_E)
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                .map(|(j, _)| j)
        })
        .collect();
    for (row, target) in rows.iter_mut().zip(&targets) {
        row.merged_into = *target;
    }
    // Follow merges to the row each ends in (a chain or a loop ends at the last row before it
    // would repeat).
    let end = |mut i: usize, rows: &[Colour]| {
        let mut seen = vec![i];
        while let Some(next) = rows.get(i).and_then(|r| r.merged_into) {
            if seen.contains(&next) {
                break;
            }
            seen.push(next);
            i = next;
        }
        i
    };
    let ends: Vec<usize> = (0..rows.len()).map(|i| end(i, &rows)).collect();
    for (i, row) in rows.iter_mut().enumerate() {
        row.merged_into = ends.get(i).copied().filter(|&e| e != i);
    }
    // Threads for the sewn rows that keep their own colour, chosen together.
    let threads = if mode == Mode::Grayscale { Threads::Neutral } else { Threads::All };
    let matched: Vec<usize> = (0..rows.len()).filter(|&i| rows.get(i).is_some_and(|r| !r.background && r.merged_into.is_none())).collect();
    let asked: Vec<ColourToMatch> = matched
        .iter()
        .filter_map(|&i| rows.get(i))
        .map(|r| ColourToMatch {
            color: r.color,
            area: r.pixels as f64,
            chosen: choice_of(r).and_then(|c| c.thread).filter(|t| t.palette() == palette),
        })
        .collect();
    let got = palette.match_colours(&asked, threads).map_err(|e| bad(C, format!("the image has {e}: lower `colors`")))?;
    for (&i, m) in matched.iter().zip(got) {
        // A shared thread merges the colour into the one that holds it.
        if let MatchReason::Shared { with } = m.reason
            && let (Some(row), Some(&into)) = (rows.get_mut(i), matched.get(with))
        {
            row.merged_into = Some(into);
        }
        if let Some(row) = rows.get_mut(i) {
            row.thread = Some(m);
        }
    }
    // A merged row is sewn in the thread of the row it ends in.
    for i in 0..rows.len() {
        let into = rows.get(i).and_then(|r| r.merged_into);
        let thread = into.and_then(|j| rows.get(j)).and_then(|r| r.thread.clone());
        if let (Some(row), Some(t)) = (rows.get_mut(i), thread) {
            row.thread = Some(t);
        }
    }
    let unused = used.iter().enumerate().filter(|(_, u)| !**u).map(|(i, _)| i).collect();
    Ok(Colours { rows, unused })
}
