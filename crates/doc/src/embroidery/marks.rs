//! A run's cuts and stops, kept by their needle point's position ([`Mark`]), and where they fall
//! on its path as it is now ([`StitchRun::placed`]).
//!
//! A mark keeps two things: its needle point's place in the path when it was written (subpath and
//! anchor), and where that needle point lay. Upstream's path commands (Reverse Path Direction, Add
//! and Delete Anchor Point, Remove Anchor Points, Simplify, Join's merge of coincident anchors,
//! Close Path) add, remove and reorder anchors without knowing the marks, so a mark is found again
//! by its needle point rather than by its place alone (the project owner's choice, 2026-10-30): at
//! its place when its needle point still lies there; else at the needle point that lies where it
//! lay (the one nearest its place); else, when the path still has as many subpaths and anchors as
//! when the marks were written ([`StitchRun::shape`]), at its place (its needle point was moved, as
//! Direct Selection moves one); else nowhere, and the plan says a mark was left out. A stop follows
//! its needle point; a cut, kept at the first needle point of the subpath it comes before, follows
//! that subpath (Reverse Path Direction reverses each subpath in place, so that needle point may
//! now end it). A transform of the object moves where each lay with its needle point
//! ([`super::Embroidery::transform`]).

use std::collections::{HashMap, HashSet};

use vectorcraft_geom::{PathData, Point};

use super::StitchRun;

/// How near a needle point must lie to where a mark's lay to be its, points: far below anything
/// sewn (a tenth of a millimetre is 0.28 pt), far above the last bits two ways of computing one
/// position can differ by at the largest coordinate (4 million points, where one unit in the last
/// place is about a billionth of a point). An estimate.
pub const SAME_SPOT: f64 = 1e-6;

/// A needle point a cut or a stop is kept at ([`StitchRun::trims`], [`StitchRun::stops`]): its place
/// in the path when the mark was written (subpath, anchor, from 0), and where it lay (document
/// points). Saved as `[subpath, anchor, x, y]`; an entry without the point (`[subpath, anchor]`) is
/// found by its place alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    /// The subpath it was in, from 0.
    pub subpath: usize,
    /// Its anchor in that subpath, from 0.
    pub anchor: usize,
    /// Where it lay, document points; `None`: found by its place alone.
    pub at: Option<Point>,
}

impl Mark {
    /// The needle point at anchor `anchor` of subpath `subpath`, lying at `at`.
    pub fn at(subpath: usize, anchor: usize, at: Point) -> Self {
        Self { subpath, anchor, at: Some(at) }
    }

    /// The needle point at anchor `anchor` of subpath `subpath`, found by its place alone.
    pub fn place(subpath: usize, anchor: usize) -> Self {
        Self { subpath, anchor, at: None }
    }

    /// An entry a file holds, when it is one this version reads: two indices, and two finite
    /// numbers or none.
    pub(super) fn read(v: &serde_json::Value) -> Option<Self> {
        let index = |v: &serde_json::Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
        match v.as_array()?.as_slice() {
            [s, a] => Some(Self::place(index(s)?, index(a)?)),
            [s, a, x, y] => {
                let (x, y) = (x.as_f64().filter(|x| x.is_finite())?, y.as_f64().filter(|y| y.is_finite())?);
                Some(Self::at(index(s)?, index(a)?, Point::new(x, y)))
            }
            _ => None,
        }
    }

    /// As a file holds it.
    pub(super) fn json(self) -> serde_json::Value {
        match self.at {
            Some(p) => serde_json::json!([self.subpath, self.anchor, p.x, p.y]),
            None => serde_json::json!([self.subpath, self.anchor]),
        }
    }
}

/// How many subpaths and anchors `path` has: what [`StitchRun::shape`] keeps.
pub fn shape_of(path: &PathData) -> [usize; 2] {
    [path.subpaths.len(), path.subpaths.iter().map(|sp| sp.anchors.len()).sum()]
}

/// Where a run's cuts and stops fall on its path now ([`StitchRun::placed`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placed {
    /// The subpaths the needle reaches after a cut.
    pub cuts: HashSet<usize>,
    /// The anchors (subpath, anchor) the machine stops after.
    pub stops: HashSet<(usize, usize)>,
    /// How many marks fell nowhere: their needle point is gone, or a cut's is in the first subpath
    /// (no gap before it).
    pub lost: usize,
}

/// The anchors of a path by where they lie, found by grid cells of [`SAME_SPOT`]: each anchor in
/// the cell its position rounds to, a position looked up in the cells around it.
struct Spots {
    cells: HashMap<(i64, i64), Vec<(usize, usize)>>,
    /// Each subpath's first anchor's place among all the path's anchors.
    starts: Vec<usize>,
}

impl Spots {
    fn cell(p: Point) -> Option<(i64, i64)> {
        let (x, y) = ((p.x / SAME_SPOT).round(), (p.y / SAME_SPOT).round());
        // Within the document's coordinates a cell number is far inside `i64`; a point out of
        // range has no cell (and no needle point lies there).
        (x.is_finite() && y.is_finite() && x.abs() < 9e18 && y.abs() < 9e18).then_some((x as i64, y as i64))
    }

    fn of(path: &PathData) -> Self {
        let mut cells: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
        let mut starts = Vec::with_capacity(path.subpaths.len());
        let mut at = 0usize;
        for (s, sp) in path.subpaths.iter().enumerate() {
            starts.push(at);
            at += sp.anchors.len();
            for (a, anchor) in sp.anchors.iter().enumerate() {
                if let Some(c) = Self::cell(anchor.p) {
                    cells.entry(c).or_default().push((s, a));
                }
            }
        }
        Self { cells, starts }
    }

    /// The anchor of `path` lying where `at` is that is nearest place `(s, a)` along the path (the
    /// later of two as near).
    fn nearest(&self, path: &PathData, at: Point, (s, a): (usize, usize)) -> Option<(usize, usize)> {
        let (cx, cy) = Self::cell(at)?;
        let flat = |(s, a): (usize, usize)| self.starts.get(s).map_or(usize::MAX, |start| start.saturating_add(a));
        let hint = flat((s, a)).min(self.starts.last().copied().unwrap_or(0).saturating_add(a));
        let lies =
            |&(s, a): &(usize, usize)| path.subpaths.get(s).and_then(|sp| sp.anchors.get(a)).is_some_and(|anchor| anchor.p.distance(at) <= SAME_SPOT);
        let mut best: Option<((usize, usize), usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(found) = self.cells.get(&(cx.saturating_add(dx), cy.saturating_add(dy))) else { continue };
                for place in found.iter().filter(|p| lies(p)) {
                    let d = flat(*place).abs_diff(hint);
                    if best.is_none_or(|(b, bd)| d < bd || (d == bd && flat(*place) > flat(b))) {
                        best = Some((*place, d));
                    }
                }
            }
        }
        best.map(|(p, _)| p)
    }
}

impl StitchRun {
    /// Where the run's cuts and stops fall on `path` (its path as it is now): each mark at its needle
    /// point, found as the module says; a stop after it, a cut before the subpath holding it (when
    /// that isn't the first).
    pub fn placed(&self, path: &PathData) -> Placed {
        let mut out = Placed::default();
        if self.trims.is_empty() && self.stops.is_empty() {
            return out;
        }
        let same_shape = self.shape == Some(shape_of(path));
        let mut spots: Option<Spots> = None;
        let mut find = |m: &Mark| -> Option<(usize, usize)> {
            let here = path.subpaths.get(m.subpath).and_then(|sp| sp.anchors.get(m.anchor)).map(|a| a.p);
            let Some(at) = m.at else { return here.map(|_| (m.subpath, m.anchor)) };
            if here.is_some_and(|p| p.distance(at) <= SAME_SPOT) {
                return Some((m.subpath, m.anchor));
            }
            let spots = spots.get_or_insert_with(|| Spots::of(path));
            spots.nearest(path, at, (m.subpath, m.anchor)).or_else(|| (same_shape && here.is_some()).then_some((m.subpath, m.anchor)))
        };
        for m in &self.trims {
            match find(m).map(|(s, _)| s).filter(|&s| s > 0) {
                Some(s) => {
                    out.cuts.insert(s);
                }
                None => out.lost += 1,
            }
        }
        for m in &self.stops {
            match find(m) {
                Some(place) => {
                    out.stops.insert(place);
                }
                None => out.lost += 1,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use vectorcraft_geom::{Anchor, SubPath};

    use super::*;

    /// A path of open subpaths through `xs` (each subpath's x positions, y 0).
    fn path(xs: &[&[f64]]) -> PathData {
        PathData::new(
            xs.iter().map(|sub| SubPath { anchors: sub.iter().map(|&x| Anchor::corner(Point::new(x, 0.0))).collect(), closed: false }).collect(),
        )
    }

    /// A run of `p`'s marks: a cut before each subpath of `cuts` (at its first anchor) and a stop
    /// after each anchor of `stops`, where they lie.
    fn marked(p: &PathData, cuts: &[usize], stops: &[(usize, usize)]) -> StitchRun {
        let at = |s: usize, a: usize| Mark::at(s, a, p.subpaths[s].anchors[a].p);
        StitchRun {
            trims: cuts.iter().map(|&s| at(s, 0)).collect(),
            stops: stops.iter().map(|&(s, a)| at(s, a)).collect(),
            shape: Some(shape_of(p)),
            ..StitchRun::default()
        }
    }

    fn placed(run: &StitchRun, p: &PathData) -> (Vec<usize>, Vec<(usize, usize)>, usize) {
        let out = run.placed(p);
        let (mut cuts, mut stops): (Vec<_>, Vec<_>) = (out.cuts.into_iter().collect(), out.stops.into_iter().collect());
        cuts.sort_unstable();
        stops.sort_unstable();
        (cuts, stops, out.lost)
    }

    /// Each mark stays at its needle point through what upstream's path commands do to a path's
    /// anchors: added before it, deleted, moved in place, reversed in place; a mark whose needle
    /// point is gone is lost and counted.
    #[test]
    fn marks_follow_their_needle_points() {
        let before = path(&[&[0.0, 1.0, 2.0, 3.0], &[10.0, 11.0, 12.0]]);
        let run = marked(&before, &[1], &[(0, 2), (1, 1)]);
        assert_eq!(placed(&run, &before), (vec![1], vec![(0, 2), (1, 1)], 0), "as written");
        // An anchor added before them (Add Anchor Point, a Pen click).
        let added = path(&[&[0.0, 0.5, 1.0, 2.0, 3.0], &[10.0, 11.0, 12.0]]);
        assert_eq!(placed(&run, &added), (vec![1], vec![(0, 3), (1, 1)], 0));
        // The stop's needle point deleted (Delete Anchor Point): lost, the others found.
        let deleted = path(&[&[0.0, 1.0, 3.0], &[10.0, 11.0, 12.0]]);
        assert_eq!(placed(&run, &deleted), (vec![1], vec![(1, 1)], 1));
        // Moved in place, as many subpaths and anchors (Direct Selection): kept at its place.
        let moved = path(&[&[0.0, 1.0, 2.5, 3.0], &[10.0, 11.0, 12.0]]);
        assert_eq!(placed(&run, &moved), (vec![1], vec![(0, 2), (1, 1)], 0));
        // Moved, with an anchor deleted elsewhere: nothing tells where it went.
        let both = path(&[&[0.0, 1.0, 2.5], &[10.0, 11.0, 12.0]]);
        assert_eq!(placed(&run, &both), (vec![1], vec![(1, 1)], 1));
        // Reversed in place (Reverse Path Direction): each stop after its needle point; the cut
        // before the subpath holding its needle point, which now ends it.
        let reversed = path(&[&[3.0, 2.0, 1.0, 0.0], &[12.0, 11.0, 10.0]]);
        assert_eq!(placed(&run, &reversed), (vec![1], vec![(0, 1), (1, 1)], 0));
        // The cut's subpath now first (the one before it deleted): no gap before it, lost.
        let alone = path(&[&[10.0, 11.0, 12.0]]);
        assert_eq!(placed(&run, &alone), (vec![], vec![(0, 1)], 2));
    }

    /// Of two needle points where a mark's lay, the one nearest its place takes it (the later of
    /// two as near); a mark kept without its point is at its place while the path has it, else
    /// lost.
    #[test]
    fn the_nearest_needle_point_where_it_lay_takes_a_mark() {
        // A tie: 4 back to 3 and forward to 4 again; the stop after the second 4 (anchor 4).
        let tied = path(&[&[2.0, 3.0, 4.0, 3.0, 4.0, 5.0]]);
        let run = marked(&tied, &[], &[(0, 4)]);
        let shifted = path(&[&[1.0, 2.0, 3.0, 4.0, 3.0, 4.0, 5.0]]);
        assert_eq!(placed(&run, &shifted).1, [(0, 5)], "one added before: the second 4 still");
        let place = StitchRun { stops: vec![Mark::place(0, 3)], trims: vec![Mark::place(1, 0)], ..StitchRun::default() };
        let two = path(&[&[0.0, 1.0, 2.0, 3.0], &[10.0, 11.0]]);
        assert_eq!(placed(&place, &two), (vec![1], vec![(0, 3)], 0));
        assert_eq!(placed(&place, &path(&[&[0.0, 1.0]])), (vec![], vec![], 2));
    }
}
