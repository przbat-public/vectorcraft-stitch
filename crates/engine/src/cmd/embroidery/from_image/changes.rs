//! Changes to single areas found again (`areaChanges`): each by its point and its colour, so that it
//! follows its area through any setting that changes the areas (research note 07, §3.4).
//!
//! 1. Its point, in the image's own pixels, is looked up in the run's region map: the area there
//!    takes it when that area's colour (the image's, before threads) is within ΔE 10 of the
//!    change's.
//! 2. Else the nearest pixel within [`REACH_MM`] whose area's colour is: an edge that moved a
//!    little.
//! 3. Then the area must be of the kind the change was set on, as Create makes it (a filled shape,
//!    or a line sewn along its middle: one with a centre line), and its colour sewn.
//!
//! Otherwise the change is kept, applying to nothing, with why: another colour lies at its point,
//! nothing sewn does (the background, or past the crop), the area there is of the other kind, or its
//! colour isn't sewn. It applies again once a setting brings its area back: the same settings give
//! the same areas.

use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::thread::MISMATCH_DELTA_E;

use super::centre::CentreLine;
use super::colours::Colours;
use super::rules::Areas;
use super::settings::{AreaChange, AreaStitch};
use super::stages::Digitized;
use super::working::Working;

/// How far round its point a change looks for an area of its colour (millimetres): the narrowest
/// satin the planner sews (`plan::SATIN_MIN_WIDTH`), so that an edge moved a little, or a thin line
/// clicked beside its middle, is still found. Research note 07's estimate; measured before the area
/// inspector was built, it rescues a change rarely, the point being an area's deepest.
pub(super) const REACH_MM: f64 = vectorcraft_stitch::plan::SATIN_MIN_WIDTH;

/// Why a change applies to no area now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeptWhy {
    /// An area of another colour lies at its point, and none of its colour near it.
    OtherColor,
    /// Nothing sewn lies at its point (the background, or past the crop), nor near it.
    NoArea,
    /// Its area is of the other kind now (a line where a filled area was, or the other way round).
    OtherKind,
    /// Its area's colour isn't sewn.
    NotSewn,
}

impl KeptWhy {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::OtherColor => "otherColor",
            Self::NoArea => "noArea",
            Self::OtherKind => "otherKind",
            Self::NotSewn => "notSewn",
        }
    }
}

/// What became of a change in a run: the area it applies to (its place in the areas), or why it
/// applies to none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Found {
    Applies(usize),
    Kept(KeptWhy),
}

/// What the changes found to one area set of it together: how it is sewn, and a filled area's
/// angle (degrees, in the picture as the window shows it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Applied {
    pub(super) stitch: Option<AreaStitch>,
    pub(super) angle: Option<f64>,
}

/// What a change is found again on: a run's working copy, its areas, their colours and the centre
/// lines of those sewn along their middle (from the lines stage on).
pub(super) struct Parts<'a> {
    pub(super) working: &'a Working,
    pub(super) areas: &'a Areas,
    pub(super) colours: &'a Colours,
    pub(super) lines: &'a [Option<CentreLine>],
}

impl<'a> Parts<'a> {
    /// The parts of run `d`.
    pub(super) fn of(d: &'a Digitized) -> Self {
        Self { working: d.working(), areas: d.areas(), colours: d.colours(), lines: d.lines() }
    }
}

/// Each of `changes` found again in run `d`, in their order.
pub(super) fn found(changes: &[AreaChange], d: &Digitized) -> Vec<Found> {
    found_in(changes, &Parts::of(d))
}

/// Each of `changes` found again on `parts`, in their order.
pub(super) fn found_in(changes: &[AreaChange], parts: &Parts) -> Vec<Found> {
    changes.iter().map(|c| found_one(c, parts)).collect()
}

/// Which of `parts`' areas `changes` leave unsewn (by their place): those whose stitch the changes
/// found to them set to Don't Sew, the last that sets one winning, as Create takes them.
pub(super) fn left_out(changes: &[AreaChange], parts: &Parts) -> Vec<bool> {
    let found = found_in(changes, parts);
    per_area(changes, &found, parts.areas.areas.len()).into_iter().map(|a| a.is_some_and(|a| a.stitch == Some(AreaStitch::None))).collect()
}

/// Whether area `a` is made a line sewn along its middle (a stroke along its centre line in
/// `lines`), as Create makes it; a filled shape otherwise (a thin area of a colour set to Fill, or
/// one with no centre line to sew along, included).
pub(super) fn made_line(lines: &[Option<CentreLine>], a: usize) -> bool {
    lines.get(a).is_some_and(Option::is_some)
}

/// What the changes found set each of a run's `areas` areas to (a later change wins where two set
/// the same of one area), by what [`found`] said of each.
pub(super) fn per_area(changes: &[AreaChange], found: &[Found], areas: usize) -> Vec<Option<Applied>> {
    let mut out = vec![None; areas];
    for (c, f) in changes.iter().zip(found) {
        let Found::Applies(a) = *f else { continue };
        let Some(slot) = out.get_mut(a) else { continue };
        let applied: &mut Applied = slot.get_or_insert_with(Applied::default);
        if c.stitch.is_some() {
            applied.stitch = c.stitch;
        }
        if c.angle.is_some() {
            applied.angle = c.angle;
        }
    }
    out
}

/// Change `c` found again on `parts`.
fn found_one(c: &AreaChange, parts: &Parts) -> Found {
    let areas = parts.areas;
    let rows = &parts.colours.rows;
    // The area `a`'s colour row, when it is no background.
    let row_of = |a: usize| areas.areas.get(a).and_then(|area| rows.get(area.row)).filter(|r| !r.background);
    let alike = |a: usize| row_of(a).is_some_and(|r| delta_e_rgb(r.color, c.color) <= MISMATCH_DELTA_E);
    let area_at = |x: i64, y: i64| -> Option<usize> {
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        areas.regions.at(x, y).map(|l| l as usize).filter(|&a| row_of(a).is_some())
    };
    let w = parts.working;
    let p = w.to_image.inverse() * c.at;
    if !(p.x.is_finite() && p.y.is_finite()) {
        return Found::Kept(KeptWhy::NoArea);
    }
    // Saturating casts: a point far past the image lies past the region map, where nothing is.
    let (x, y) = (p.x.floor() as i64, p.y.floor() as i64);
    let under = area_at(x, y);
    let area = under.filter(|&a| alike(a)).or_else(|| {
        let reach = (REACH_MM * w.px_per_mm).ceil() as i64;
        let mut nearest: Option<(i64, usize)> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let d2 = dx * dx + dy * dy;
                if d2 > reach * reach || nearest.is_some_and(|(best, _)| best <= d2) {
                    continue;
                }
                if let Some(a) = area_at(x.saturating_add(dx), y.saturating_add(dy)).filter(|&a| alike(a)) {
                    nearest = Some((d2, a));
                }
            }
        }
        nearest.map(|(_, a)| a)
    });
    let Some(a) = area else {
        return Found::Kept(if under.is_some() { KeptWhy::OtherColor } else { KeptWhy::NoArea });
    };
    let Some(row) = row_of(a) else { return Found::Kept(KeptWhy::NoArea) };
    if made_line(parts.lines, a) != c.line {
        return Found::Kept(KeptWhy::OtherKind);
    }
    if !row.sewn() {
        return Found::Kept(KeptWhy::NotSewn);
    }
    Found::Applies(a)
}
