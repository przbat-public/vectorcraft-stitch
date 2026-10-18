//! How a path becomes stitches: running stitch, satin column and tatami fill.
//!
//! A *run* is a stretch the needle sews without lifting: a set of needle points in order. Runs
//! are joined into a block: a short move between two runs stays down (it is just the next stitch),
//! a long one becomes a [`Command::Trim`] and a [`Command::Jump`].
//!
//! Parameters are millimetres and degrees. Everything is clamped to a sane range before use, so
//! out-of-range or non-finite input gives a usable stitch plan rather than a loop that never
//! ends: no function here panics.
//!
//! The three styles:
//!
//! - [`Style::Running`] follows a path: a single line of stitches, [`Params::stitch_length`]
//!   apart, with a lock stitch at each end ([`Params::lock`]).
//! - [`Style::Satin`] sews a column: the path is its centreline,[`Params::width`] its width, and
//!   the needle crosses from rail to rail every [`Params::density`] millimetres, so the threads
//!   lie side by side. [`Params::pull`] widens the column (pull compensation: the fabric draws
//!   the stitches together). A centre run ([`Params::underlay`]) holds the column up.
//! - [`Style::Fill`] fills a shape with rows of stitches [`Params::density`] apart at
//!   [`Params::angle`] degrees, alternating their direction so the needle stays near the last
//!   stitch, with an edge run underneath ([`Params::underlay`]). [`Params::stitch_length`] is
//!   the longest stitch a row is split into.

use std::f64::consts::TAU;

use vectorcraft_geom::{BezPath, FillRule, PathEl, Point, Vec2, kurbo};

use crate::{Command, Error, Stitch};

/// How far curves are flattened before stitching, millimetres: far below the 0.1 mm a machine
/// counts in.
const FLATTEN: f64 = 0.05;
/// A trim and a jump are needed when the thread would otherwise run further than this, unless
/// [`Params::jump`] says otherwise (millimetres).
const JUMP: f64 = 5.0;
/// The longest satin stitch before it is split (millimetres): longer stitches snag and sag. An
/// estimate (professional software makes it a setting, bounded by the machine format: 12.1 mm
/// per record in DST).
const MAX_SATIN: f64 = 10.0;
/// Satin underlay by column width (millimetres): from this width an edge run instead of a centre
/// run, from this one an edge run and a zigzag; and the zigzag's spacing. Estimates.
const SATIN_EDGE_UNDERLAY: f64 = 2.0;
const SATIN_ZIGZAG_UNDERLAY: f64 = 4.5;
const ZIGZAG_UNDERLAY_SPACING: f64 = 2.0;
/// A large fill's second underlay, sparse tatami rows across the top ones: the spacing of its rows
/// and its stitch length (millimetres), how far inside the edge it stays, and the smallest fill
/// (both ways, millimetres) that gets it. Estimates.
const TATAMI_UNDERLAY_SPACING: f64 = 2.0;
const TATAMI_UNDERLAY_STITCH: f64 = 3.0;
const TATAMI_UNDERLAY_INSET: f64 = 0.6;
const TATAMI_UNDERLAY_MIN: f64 = 10.0;
/// A travel run's stitch length (millimetres): short, so it follows curves and hides under the
/// fill.
const TRAVEL_STITCH: f64 = 2.0;
/// How far inside the shape a travel along its edge runs (millimetres), so it doesn't show at
/// the edge.
const TRAVEL_MARGIN: f64 = 0.5;
/// The longest travel run inside a fill (millimetres): a longer way trims and jumps instead. An
/// estimate.
const MAX_TRAVEL: f64 = 25.0;
/// The underlay's own stitch length (millimetres): short, to hold the fabric still.
const UNDERLAY_STITCH: f64 = 2.0;
/// The lock stitch's reach, millimetres.
const LOCK: f64 = 1.0;
/// The most runs, rows and stitches one shape may produce (a guard against absurd parameters).
const MAX_ROWS: usize = 100_000;
const MAX_RUNS: usize = 1_000_000;
/// The most stitches one shape may plan: [`try_stitches`] refuses a shape that would need more.
pub const MAX_STITCHES: usize = 4_000_000;

/// How a shape is stitched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// One line of stitches along the path.
    #[default]
    Running,
    /// A column across [`Params::width`].
    Satin,
    /// A shape filled with rows (tatami).
    Fill,
}

impl Style {
    pub fn id(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Satin => "satin",
            Self::Fill => "fill",
        }
    }

    /// The style `id` names (`running`, `satin`, `fill`).
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "running" => Self::Running,
            "satin" => Self::Satin,
            "fill" => Self::Fill,
            _ => return None,
        })
    }

    /// All styles, as menus list them.
    pub const ALL: [Self; 3] = [Self::Running, Self::Satin, Self::Fill];
}

/// What one shape is stitched as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub style: Style,
    /// The longest stitch along a running line or a fill row, millimetres.
    pub stitch_length: f64,
    /// The distance between neighbouring threads: rail to rail for satin, row to row for fill,
    /// millimetres.
    pub density: f64,
    /// The direction the fill's rows run, degrees (0: left to right).
    pub angle: f64,
    /// Pull compensation: how much wider the stitching is made so the fabric's pull does not
    /// leave a gap of background at an edge, millimetres (satin only for now).
    pub pull: f64,
    /// Sew an underlay first: an edge run under a fill, a centre run under a satin column.
    pub underlay: bool,
    /// A move longer than this trims and jumps instead of running the thread across,
    /// millimetres.
    pub jump: f64,
    /// Lock the thread so it does not pull out: a running line gets a lock stitch (a short back
    /// stitch) at each end of each run; satin and fill a tie-in after every cut and a tie-off
    /// before it.
    pub lock: bool,
    /// Shorter stitches are left out (a row's remainder, a sliver of a shape), millimetres.
    pub min_stitch: f64,
    /// The satin column's width, millimetres (unused by the other styles).
    pub width: f64,
    /// The longest satin stitch, millimetres: a crossing longer than this is split into shorter
    /// stitches (auto split), its penetrations spread so they don't line up into a seam.
    pub max_satin: f64,
    /// Satin only: pick [`Params::density`] from the column's width ([`satin_spacing`]) instead of
    /// using it as given. On by default; an explicit density turns it off.
    pub auto_spacing: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self::for_style(Style::Running)
    }
}

impl Params {
    /// The defaults of one style.
    pub fn for_style(style: Style) -> Self {
        match style {
            Style::Running => Self {
                style,
                stitch_length: 2.5,
                density: 0.5,
                angle: 0.0,
                pull: 0.0,
                underlay: false,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 0.0,
                max_satin: MAX_SATIN,
                auto_spacing: false,
            },
            Style::Satin => Self {
                style,
                stitch_length: 2.5,
                density: 0.4,
                angle: 0.0,
                pull: 0.2,
                underlay: true,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 3.0,
                max_satin: MAX_SATIN,
                auto_spacing: true,
            },
            Style::Fill => Self {
                style,
                stitch_length: 4.0,
                density: 0.4,
                angle: 45.0,
                pull: 0.0,
                underlay: true,
                jump: JUMP,
                lock: true,
                min_stitch: 0.4,
                width: 0.0,
                max_satin: MAX_SATIN,
                auto_spacing: false,
            },
        }
    }

    /// The same parameters with every value clamped to a range that produces a stitch plan:
    /// not finite falls back to the style's default, and each distance is bounded so that no
    /// loop can run away.
    pub fn sane(self) -> Self {
        let d = Self::for_style(self.style);
        let fin = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        let angle = fin(self.angle, d.angle) % 360.0;
        Self {
            style: self.style,
            stitch_length: fin(self.stitch_length, d.stitch_length).clamp(0.2, 12.0),
            density: fin(self.density, d.density).clamp(0.05, 5.0),
            angle: if angle < 0.0 { angle + 360.0 } else { angle },
            pull: fin(self.pull, d.pull).clamp(0.0, 2.0),
            underlay: self.underlay,
            jump: fin(self.jump, d.jump).clamp(0.5, 20.0),
            lock: self.lock,
            min_stitch: fin(self.min_stitch, d.min_stitch).clamp(0.05, 3.0),
            width: fin(self.width, d.width).clamp(0.0, 50.0),
            max_satin: fin(self.max_satin, d.max_satin).clamp(2.0, 12.0),
            auto_spacing: self.auto_spacing,
        }
    }
}

/// The stitches of `path` in style `p`: one block, in needle order.
///
/// `rule` decides what a fill covers when its subpaths overlap (a hole wound against its shape
/// is a hole under the non-zero rule; under the even-odd rule any two crossings change side).
pub fn stitches(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    match p.style {
        Style::Running => running(path, &p),
        Style::Satin if p.width > 0.0 => satin(path, &p),
        // A satin column needs a width: without one the shape is filled instead.
        Style::Satin | Style::Fill => fill(path, rule, &p),
    }
}

/// [`stitches`], refused with [`Error::TooLarge`] when the shape would need more than
/// [`MAX_STITCHES`] (a huge shape, or a tiny stitch length or spacing): estimated from the
/// geometry before any stitch is planned, so an absurd request costs no memory.
pub fn try_stitches(path: &BezPath, rule: FillRule, p: &Params) -> crate::Result<Vec<Stitch>> {
    let p = p.sane();
    let n = estimate(&subpaths(path), &p);
    if !n.is_finite() || n > MAX_STITCHES as f64 {
        return Err(Error::TooLarge(format!(
            "a shape would need about {n:.0} stitches, more than the {MAX_STITCHES} one shape may have: use a longer stitch, wider spacing or a smaller design"
        )));
    }
    Ok(stitches(path, rule, &p))
}

/// About how many points `subs` plan into in style `p` (an upper estimate; not finite when the
/// geometry is not).
fn estimate(subs: &[SubPath], p: &Params) -> f64 {
    let total: f64 = subs.iter().map(|s| length(&s.points(), s.closed)).sum();
    let ends = 6.0 * subs.len() as f64;
    match p.style {
        Style::Running => total / p.stitch_length + ends,
        Style::Satin if p.width > 0.0 => total / UNDERLAY_STITCH + total / p.density + ends,
        Style::Satin | Style::Fill => {
            let (along, across) = axes(p.angle);
            let extent = |v: Vec2| {
                let (lo, hi) = subs.iter().flat_map(|s| s.pts.iter()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| {
                    let t = dot(*q, v);
                    (lo.min(t), hi.max(t))
                });
                if hi >= lo { hi - lo } else { 0.0 }
            };
            let rows = extent(across) / p.density + 1.0;
            total / UNDERLAY_STITCH + rows * (extent(along) / p.stitch_length + 2.0) + ends
        }
    }
}

/// A running line of stitches along `path` ([`Style::Running`]).
pub fn running(path: &BezPath, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    let mut runs: Vec<Vec<Point>> = Vec::new();
    for sub in subpaths(path) {
        let pts = sub.points();
        if length(&pts, sub.closed) < p.min_stitch {
            continue;
        }
        let mut run = resample(&pts, p.stitch_length, sub.closed);
        if p.lock && !sub.closed {
            run = locked(&run);
        }
        runs.push(run);
    }
    join(runs, &p)
}

/// A satin column sewn along `path`'s centreline, `params.width` wide ([`Style::Satin`]). The
/// width comes from [`Params::width`] unless `width` is positive.
pub fn satin(path: &BezPath, p: &Params) -> Vec<Stitch> {
    let mut p = p.sane();
    p.style = Style::Satin;
    if p.width <= 0.0 {
        return Vec::new();
    }
    let half = (p.width + 2.0 * p.pull) / 2.0;
    let mut runs: Vec<Vec<Point>> = Vec::new();
    // The first run of each column is reached as any move is; the column's later runs (underlay
    // to top stitching) start inside the column, so the needle sews there, never trims.
    let mut travels: Vec<Travel> = Vec::new();
    for sub in subpaths(path) {
        let column = runs.len();
        let pts = sub.points();
        if length(&pts, sub.closed) < p.density * 2.0 {
            continue;
        }
        // The underlay, by the column's width; it may end at the column's far end, and then the
        // top stitching starts there and comes back.
        let mut back = false;
        if p.underlay {
            for (under, ends_far) in satin_underlay(&pts, sub.closed, p.width) {
                if under.len() >= 2 {
                    travels.push(if runs.len() == column { Travel::Auto } else { Travel::Sew });
                    runs.push(under);
                }
                back = ends_far;
            }
        }
        let spacing = if p.auto_spacing { satin_spacing(p.width) } else { p.density };
        let mut samples = resample(&pts, spacing, sub.closed);
        if samples.len() < 2 {
            continue;
        }
        if back && !sub.closed {
            samples.reverse();
        }
        let mut zigzag = Vec::with_capacity(samples.len() + 1);
        for i in 0..samples.len() {
            let Some(q) = samples.get(i).copied() else { continue };
            let t = tangent(&samples, i, sub.closed);
            let n = Vec2::new(-t.y, t.x);
            let side = if i % 2 == 0 { 1.0 } else { -1.0 };
            zigzag.push(q + n * half * side);
        }
        // A closed column ends where it started: the last crossing takes the remaining side.
        if sub.closed
            && let (Some(first), Some(last)) = (zigzag.first().copied(), zigzag.last().copied())
            && first.distance(last) > p.min_stitch
        {
            zigzag.push(first);
        }
        travels.push(if runs.len() == column { Travel::Auto } else { Travel::Sew });
        runs.push(split_long(&zigzag, p.max_satin));
        if runs.len() >= MAX_RUNS {
            break;
        }
    }
    join_cut(runs.into_iter().zip(travels).collect(), &p)
}

/// A tatami fill of `path` ([`Style::Fill`]): rows of stitches at [`Params::angle`] degrees,
/// [`Params::density`] apart, each split into stitches at most [`Params::stitch_length`] long,
/// with an edge run underneath when [`Params::underlay`] is on. Open subpaths are ignored by the
/// fill (there is no inside); they are sewn as running lines underneath instead.
pub fn fill(path: &BezPath, rule: FillRule, p: &Params) -> Vec<Stitch> {
    let p = p.sane();
    let subs = subpaths(path);
    let mut runs: Vec<Vec<Point>> = Vec::new();
    // An edge run under the fill holds the fabric; open lines are sewn as they are.
    if p.underlay {
        for sub in subs.iter().filter(|s| s.closed) {
            let under = resample(&sub.points(), UNDERLAY_STITCH, true);
            if under.len() >= 2 {
                runs.push(under);
            }
        }
    }
    let mut open: Vec<Vec<Point>> = subs.iter().filter(|s| !s.closed).map(|s| s.points()).collect();
    for line in open.drain(..) {
        if length(&line, false) >= p.min_stitch {
            runs.push(resample(&line, p.stitch_length, false));
        }
    }
    let closed: Vec<&SubPath> = subs.iter().filter(|s| s.closed && s.pts.len() >= 3).collect();
    if closed.is_empty() {
        return join(runs, &p);
    }
    let mut travels: Vec<Travel> = vec![Travel::Auto; runs.len()];
    // A large fill gets a second underlay under its top stitching: sparse tatami rows across the
    // top rows' direction, kept a little inside the edge so they never peek out.
    if p.underlay {
        let under = Params { angle: p.angle + 90.0, density: TATAMI_UNDERLAY_SPACING, stitch_length: TATAMI_UNDERLAY_STITCH, ..p }.sane();
        let (along, across) = axes(under.angle);
        if let (Some(w), Some(h)) = (extent(&closed, along), extent(&closed, across))
            && w.1 - w.0 >= TATAMI_UNDERLAY_MIN
            && h.1 - h.0 >= TATAMI_UNDERLAY_MIN
        {
            let rows = (((h.1 - h.0) / under.density).floor().max(1.0) as usize).min(MAX_ROWS);
            let regions = regions(&closed, rule, along, across, h.0, under.density, rows, TATAMI_UNDERLAY_INSET);
            sew_regions(&regions, &closed, rule, along, across, &under, &mut runs, &mut travels);
        }
    }
    let (along, across) = axes(p.angle);
    // The rows run along `along`; each sits at one value of `across`.
    let Some((lo, hi)) = extent(&closed, across) else { return join_cut(runs.into_iter().zip(travels).collect(), &p) };
    if hi - lo < p.density * 0.5 {
        return join_cut(runs.into_iter().zip(travels).collect(), &p);
    }
    let spacing = p.density;
    // Rows sit half a spacing inside the shape's extent: no row lies on a tangent, and the
    // thread's own width covers to the edge.
    let rows = (((hi - lo) / spacing).floor().max(1.0) as usize).min(MAX_ROWS);
    let regions = regions(&closed, rule, along, across, lo, spacing, rows, 0.0);
    sew_regions(&regions, &closed, rule, along, across, &p, &mut runs, &mut travels);
    join_cut(runs.into_iter().zip(travels).collect(), &p)
}

/// Where the polygons' points fall along `v`: the lowest and highest, `None` when none is finite.
fn extent(polys: &[&SubPath], v: Vec2) -> Option<(f64, f64)> {
    let (lo, hi) = polys
        .iter()
        .flat_map(|s| s.pts.iter())
        .map(|q| dot(*q, v))
        .filter(|t| t.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
    (lo <= hi).then_some((lo, hi))
}

/// The regions of one layer of fill sewn after `runs`: each region whole, row after row; the next
/// is the one that starts nearest where the last one ended, sewn from whichever of its ends and
/// sides is closer.
#[allow(clippy::too_many_arguments)]
fn sew_regions(
    regions: &[Region],
    closed: &[&SubPath],
    rule: FillRule,
    along: Vec2,
    across: Vec2,
    p: &Params,
    runs: &mut Vec<Vec<Point>>,
    travels: &mut Vec<Travel>,
) {
    let mut pending: Vec<&Region> = regions.iter().collect();
    let mut here: Option<Point> = runs.last().and_then(|r| r.last().copied());
    while !pending.is_empty() {
        let mut best: Option<(f64, usize, bool, bool)> = None;
        for (i, region) in pending.iter().enumerate() {
            for descending in [false, true] {
                for forward in [true, false] {
                    let Some(start) = region.start(descending, forward, along, across) else { continue };
                    let d = here.map_or(0.0, |h| h.distance(start));
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, i, descending, forward));
                    }
                }
            }
        }
        let Some((_, i, descending, forward)) = best else { break };
        let region = pending.swap_remove(i);
        let sewn = region.runs(descending, forward, along, across, p);
        let Some(first) = sewn.first().and_then(|r| r.first().copied()) else { continue };
        // Into the region: a short move is just the next stitch. A longer one, or one that would
        // cross a hole or leave the shape, travels inside the shape when it can (a run of travel
        // stitches, straight or along an edge), and is cut when it can't: the thread must never
        // float over the fabric.
        let into = match here {
            Some(h) if h.distance(first) > p.jump || !segment_inside(closed, rule, h, first) => match travel(closed, rule, h, first, p) {
                Some(route) => {
                    travels.push(Travel::Sew);
                    runs.push(route);
                    Travel::Sew
                }
                None => Travel::Cut,
            },
            _ => Travel::Auto,
        };
        for (k, run) in sewn.into_iter().enumerate() {
            let travel = match (k, here, run.first()) {
                (0, ..) => into,
                // A turn into the region's next row is sewn, however long, while it stays inside
                // the shape. Along a concave edge (the flat top of a round hole) it would cut
                // across the hole: then it travels round the edge, or is cut, unless it only
                // shaves a corner.
                (_, Some(h), Some(&f)) if h.distance(f) > 1.0 && !segment_inside(closed, rule, h, f) => match travel(closed, rule, h, f, p) {
                    Some(route) => {
                        travels.push(Travel::Sew);
                        runs.push(route);
                        Travel::Sew
                    }
                    None => Travel::Cut,
                },
                _ => Travel::Sew,
            };
            here = run.last().copied().or(here);
            travels.push(travel);
            runs.push(run);
        }
        if runs.len() >= MAX_RUNS {
            break;
        }
    }
}

/// How the needle gets to a run from the end of the one before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Travel {
    /// Sewn when it is no longer than [`Params::jump`], else trimmed and jumped.
    Auto,
    /// Always trimmed and jumped (it would cross a hole or leave the shape).
    Cut,
    /// Always sewn (a row turn inside the region being filled).
    Sew,
}

/// A stretch of fill rows that follow one another without the shape branching: each row has one
/// span, and it overlaps the previous row's.
#[derive(Default)]
struct Region {
    /// (row number, the row's position across, the span's ends along).
    rows: Vec<(usize, f64, f64, f64)>,
}

impl Region {
    /// Where sewing the region starts: its first row (its last when `descending`), from its start
    /// when `forward`, else from its end.
    fn start(&self, descending: bool, forward: bool, along: Vec2, across: Vec2) -> Option<Point> {
        let &(_, t, a, b) = if descending { self.rows.last()? } else { self.rows.first()? };
        Some(at(if forward { a } else { b }, t, along, across))
    }

    /// The region's rows as runs: alternating direction, the first `forward`, last row first when
    /// `descending`.
    fn runs(&self, descending: bool, forward: bool, along: Vec2, across: Vec2, p: &Params) -> Vec<Vec<Point>> {
        let mut out: Vec<Vec<Point>> = Vec::with_capacity(self.rows.len());
        let mut forward = forward;
        let order: Vec<&(usize, f64, f64, f64)> = if descending { self.rows.iter().rev().collect() } else { self.rows.iter().collect() };
        for &&(row, t, a, b) in &order {
            // The stagger walks a quarter stitch at a time through a cycle of four rows: no two
            // rows line their penetrations up into a groove.
            let phase = 0.25 * p.stitch_length * (row % 4) as f64;
            let ss = row_points(a, b, phase, p);
            if ss.len() < 2 {
                continue;
            }
            let mut pts: Vec<Point> = ss.iter().map(|s| at(*s, t, along, across)).collect();
            if !forward {
                pts.reverse();
            }
            // Skip the last stitch of the row: the needle turns diagonally into the next row
            // instead of crawling one row spacing along the edge — when the stitch it leaves out
            // is short, so the turn stays a short stitch too.
            if let [.., prev, last] = pts.as_slice()
                && pts.len() > 2
                && prev.distance(*last) <= 0.5 * p.stitch_length
            {
                pts.pop();
            }
            out.push(pts);
            forward = !forward;
        }
        out
    }
}

/// The fill's rows grouped into regions: a span joins the region of the previous row's span it
/// overlaps when that is the only one it overlaps and it is the only span overlapping it; where
/// the shape branches or merges (round a hole, into the arms of a letter) new regions start.
#[allow(clippy::too_many_arguments)]
fn regions(polys: &[&SubPath], rule: FillRule, along: Vec2, across: Vec2, lo: f64, spacing: f64, rows: usize, inset: f64) -> Vec<Region> {
    let overlap = |x: (f64, f64), y: (f64, f64)| x.0 < y.1 && y.0 < x.1;
    let mut out: Vec<Region> = Vec::new();
    // The regions the previous row's spans went to, with those spans.
    let mut open: Vec<(usize, (f64, f64))> = Vec::new();
    // Every row's position and spans, for the overlaps below.
    let mut all: Vec<(f64, Vec<(f64, f64)>)> = Vec::with_capacity(rows);
    for row in 0..rows {
        let t = lo + spacing * (row as f64 + 0.5);
        // `inset` keeps the rows that far from the edge where they meet it.
        let spans: Vec<(f64, f64)> =
            spans_at(polys, rule, along, across, t).into_iter().map(|(a, b)| (a + inset, b - inset)).filter(|(a, b)| b > a).collect();
        all.push((t, spans.clone()));
        let mut next: Vec<(usize, (f64, f64))> = Vec::with_capacity(spans.len());
        for &span in &spans {
            let mut touching = open.iter().filter(|(_, prev)| overlap(*prev, span));
            let joined = match (touching.next(), touching.next()) {
                (Some(&(r, prev)), None) if spans.iter().filter(|s| overlap(prev, **s)).count() == 1 => Some(r),
                _ => None,
            };
            let r = joined.unwrap_or_else(|| {
                out.push(Region::default());
                out.len() - 1
            });
            if let Some(region) = out.get_mut(r) {
                region.rows.push((row, t, span.0, span.1));
            }
            next.push((r, span));
        }
        open = next;
    }
    // Where a region meets another (round a hole, at a branch), it takes one more row of its
    // neighbour's, cut to the stretch the two share: the fabric's pull would otherwise open a gap
    // along the seam.
    for region in &mut out {
        let ends = [(region.rows.first().copied(), false), (region.rows.last().copied(), true)];
        for (end, after) in ends {
            let Some((row, _, a, b)) = end else { continue };
            let Some(next) = (if after { row.checked_add(1) } else { row.checked_sub(1) }) else { continue };
            let Some((t, spans)) = all.get(next) else { continue };
            // The neighbouring span that shares the most with this end.
            let shared =
                spans.iter().map(|s| (s.0.max(a), s.1.min(b))).filter(|(x, y)| y - x > 1e-9).max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)));
            if let Some((x, y)) = shared {
                if after {
                    region.rows.push((next, *t, x, y));
                } else {
                    region.rows.insert(0, (next, *t, x, y));
                }
            }
        }
    }
    out
}

/// A travel run from `a` to `b` inside the filled shape, as stitches [`TRAVEL_STITCH`] long:
/// straight when that stays inside, else along the edge of one of the shape's outlines (the outer
/// edge or a hole's), [`TRAVEL_MARGIN`] inside it, the shorter way round. `None` when neither
/// stays inside or the way is longer than [`MAX_TRAVEL`]: then the thread is cut instead.
fn travel(polys: &[&SubPath], rule: FillRule, a: Point, b: Point, p: &Params) -> Option<Vec<Point>> {
    let straight = a.distance(b);
    if !straight.is_finite() {
        return None;
    }
    if segment_inside(polys, rule, a, b) {
        return (straight <= MAX_TRAVEL).then(|| resample(&[a, b], TRAVEL_STITCH, false));
    }
    // The ends of rows lie on the outline, or within a stitch of it.
    let reach = p.stitch_length + 2.0 * p.density;
    let mut best: Option<(f64, Vec<Point>)> = None;
    for sub in polys {
        let pts = &sub.pts;
        let n = pts.len();
        if n < 3 {
            continue;
        }
        let (Some((ia, pa, da)), Some((ib, pb, db))) = (nearest_on(pts, a), nearest_on(pts, b)) else { continue };
        if da > reach || db > reach {
            continue;
        }
        // The outline's corners between the two feet, either way round: forward from the corner
        // after `a`'s edge to the corner of `b`'s edge, or backward.
        let forward: Vec<usize> = (1..=n).map(|k| (ia + k) % n).take_while(|&i| i != (ib + 1) % n).collect();
        let backward: Vec<usize> = (0..n).map(|k| (ia + n - k) % n).take_while(|&i| i != ib).collect();
        for corners in [forward, backward] {
            if corners.len() >= n {
                continue;
            }
            let mut route: Vec<Point> = vec![a, inset(polys, rule, pts, None, pa)];
            for &i in &corners {
                if let Some(&q) = pts.get(i) {
                    route.push(inset(polys, rule, pts, Some(i), q));
                }
            }
            route.push(inset(polys, rule, pts, None, pb));
            route.push(b);
            let len = route.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>();
            if len > MAX_TRAVEL || best.as_ref().is_some_and(|(l, _)| *l <= len) {
                continue;
            }
            if route.windows(2).all(|w| w[0].distance(w[1]) < 1e-9 || segment_inside(polys, rule, w[0], w[1])) {
                best = Some((len, route));
            }
        }
    }
    best.map(|(_, route)| resample(&route, TRAVEL_STITCH, false))
}

/// The point of the closed polyline `pts` nearest `q`: the index of the edge it lies on (from
/// point `i` to the next), the point, and its distance.
fn nearest_on(pts: &[Point], q: Point) -> Option<(usize, Point, f64)> {
    let n = pts.len();
    let mut best: Option<(usize, Point, f64)> = None;
    for i in 0..n {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        let ab = b - a;
        let len2 = ab.hypot2();
        let t = if len2 > 1e-18 { ((q - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let foot = a + ab * t;
        let d = foot.distance(q);
        if d.is_finite() && best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((i, foot, d));
        }
    }
    best
}

/// Point `q` of outline `pts` (its corner `corner`, or a point on an edge) moved [`TRAVEL_MARGIN`]
/// into the shape, so that a travel along the outline runs inside it; left where it is when
/// neither side is inside (a sliver).
fn inset(polys: &[&SubPath], rule: FillRule, pts: &[Point], corner: Option<usize>, q: Point) -> Point {
    let n = pts.len();
    // The direction across the outline at `q`: at a corner the bisector of its two edges, on an
    // edge the edge's normal.
    let across = match corner {
        Some(i) => {
            let (prev, next) = (pts.get((i + n - 1) % n), pts.get((i + 1) % n));
            match (prev, next) {
                (Some(&a), Some(&b)) => {
                    let (u, v) = (unit(q - a), unit(b - q));
                    match (u, v) {
                        (Some(u), Some(v)) => unit(Vec2::new(-(u.y + v.y), u.x + v.x)),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        None => nearest_on(pts, q).and_then(|(i, ..)| match (pts.get(i), pts.get((i + 1) % n)) {
            (Some(&a), Some(&b)) => unit(b - a).map(|d| Vec2::new(-d.y, d.x)),
            _ => None,
        }),
    };
    let Some(across) = across else { return q };
    for side in [across, -across] {
        let moved = q + side * TRAVEL_MARGIN;
        if inside(polys, rule, moved) {
            return moved;
        }
    }
    q
}

/// Whether the straight line from `a` to `b` stays inside the filled shape (sampled every half
/// millimetre, the ends excluded: they lie on its edge).
fn segment_inside(polys: &[&SubPath], rule: FillRule, a: Point, b: Point) -> bool {
    let n = (a.distance(b) / 0.5).ceil();
    if !n.is_finite() {
        return false;
    }
    let n = (n as usize).clamp(2, 400);
    (1..n).all(|k| inside(polys, rule, a.lerp(b, k as f64 / n as f64)))
}

/// Whether `q` is inside the closed polygons `polys` under `rule`.
fn inside(polys: &[&SubPath], rule: FillRule, q: Point) -> bool {
    let mut w = 0i32;
    for sub in polys {
        let n = sub.pts.len();
        for i in 0..n {
            let (Some(&a), Some(&b)) = (sub.pts.get(i), sub.pts.get(if i + 1 == n { 0 } else { i + 1 })) else { continue };
            let side = (b.x - a.x) * (q.y - a.y) - (q.x - a.x) * (b.y - a.y);
            if a.y <= q.y {
                if b.y > q.y && side > 0.0 {
                    w += 1;
                }
            } else if b.y <= q.y && side < 0.0 {
                w -= 1;
            }
        }
    }
    match rule {
        FillRule::NonZero => w != 0,
        FillRule::EvenOdd => w % 2 != 0,
    }
}

/// Where the row at `t` crosses the shape: the spans of `along` inside it, by `rule`.
fn spans_at(polys: &[&SubPath], rule: FillRule, along: Vec2, across: Vec2, t: f64) -> Vec<(f64, f64)> {
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for sub in polys {
        let n = sub.pts.len();
        for i in 0..n {
            let Some(&a) = sub.pts.get(i) else { continue };
            let Some(&b) = sub.pts.get(if i + 1 == n { 0 } else { i + 1 }) else { continue };
            let (ta, tb) = (dot(a, across), dot(b, across));
            // Half-open: a vertex exactly on the row counts once.
            if (ta > t) == (tb > t) {
                continue;
            }
            let f = (t - ta) / (tb - ta);
            if !f.is_finite() {
                continue;
            }
            let s = dot(a, along) + f * (dot(b, along) - dot(a, along));
            crossings.push((s, if tb > ta { 1 } else { -1 }));
        }
    }
    crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spans: Vec<(f64, f64)> = Vec::new();
    match rule {
        FillRule::EvenOdd => {
            for pair in crossings.chunks(2) {
                if let [a, b] = pair
                    && b.0 - a.0 > 1e-9
                {
                    spans.push((a.0, b.0));
                }
            }
        }
        FillRule::NonZero => {
            let mut winding = 0;
            let mut start = 0.0;
            for (s, dir) in crossings {
                let before = winding;
                winding += dir;
                if before == 0 && winding != 0 {
                    start = s;
                } else if before != 0 && winding == 0 && s - start > 1e-9 {
                    spans.push((start, s));
                }
            }
        }
    }
    spans
}

/// The positions along a row that get a stitch: `ceil(len / stitch_length)` even steps (so no
/// stitch is longer than asked and the last one is not a stub), with the interior points moved by
/// `phase` (the stagger that keeps rows from lining up). Both ends are included.
fn row_points(a: f64, b: f64, phase: f64, p: &Params) -> Vec<f64> {
    let len = b - a;
    // A row shorter than two row spacings (or 0.8 mm) has no room for a fill: leave it out.
    let shortest = (2.0 * p.density).max(0.8);
    if !len.is_finite() || len < shortest || len < p.min_stitch {
        return Vec::new();
    }
    let count = ((len / p.stitch_length).ceil() as usize).clamp(1, MAX_STITCHES);
    let step = len / count as f64;
    let mut out: Vec<f64> = (0..=count).map(|k| a + k as f64 * step).collect();
    if count >= 3 && step.is_finite() && step > 0.0 {
        // At most half a step (so no stitch grows past one and a half stitch lengths) and at
        // most a step less the shortest stitch (so the row's last stitch is not a stub).
        let hi = (step * 0.5).min((step - p.min_stitch).max(0.0));
        let shift = phase.rem_euclid(step).min(hi);
        for v in out.iter_mut().skip(1).take(count - 1) {
            *v = (*v + shift).min(b);
        }
    }
    out
}

/// Runs joined into one block: travelling far enough to trim does.
fn join(runs: Vec<Vec<Point>>, p: &Params) -> Vec<Stitch> {
    join_cut(runs.into_iter().map(|r| (r, Travel::Auto)).collect(), p)
}

/// [`join`], each run reached as its [`Travel`] says.
fn join_cut(runs: Vec<(Vec<Point>, Travel)>, p: &Params) -> Vec<Stitch> {
    // Satin and fill tie the thread in after every cut (and at the start) and off before every cut
    // (and at the end); a running line locks each of its own runs ([`locked`]).
    let tie = p.lock && p.style != Style::Running;
    let mut out: Vec<Stitch> = Vec::new();
    let mut fresh = true;
    for (mut run, travel) in runs {
        dedupe(&mut run);
        let Some(&first) = run.first() else { continue };
        if let Some(last) = out.last().copied()
            && let d = last.distance(Stitch::sew(first))
            && d > 1e-9
            && match travel {
                Travel::Auto => d > p.jump,
                Travel::Cut => true,
                Travel::Sew => false,
            }
        {
            if tie {
                tie_off(&mut out);
            }
            let last = out.last().copied().unwrap_or(last);
            out.push(Stitch::new(last.x, last.y, Command::Trim));
            out.push(Stitch::jump(first));
            fresh = true;
        }
        if tie && fresh {
            run = tie_in(&run);
        }
        fresh = false;
        for q in run {
            if out.len() >= MAX_STITCHES {
                return out;
            }
            // A repeated stitch is noise; the stitch where a jump lands is not (the needle goes
            // down there, holding the thread).
            if out.last().is_none_or(|last| last.command != Command::Stitch || last.point().distance(q) > 1e-9) {
                out.push(Stitch::sew(q));
            }
        }
    }
    if tie {
        tie_off(&mut out);
    }
    out
}

/// `run` started with a tie-in: a short stitch forward along it and back, so the cut thread's
/// tail is held.
fn tie_in(run: &[Point]) -> Vec<Point> {
    let (Some(&first), Some(&next)) = (run.first(), run.get(1)) else { return run.to_vec() };
    let Some(d) = unit(next - first) else { return run.to_vec() };
    let mut out = Vec::with_capacity(run.len() + 2);
    out.push(first);
    out.push(first + d * LOCK.min(first.distance(next)));
    out.extend_from_slice(run);
    out
}

/// A tie-off after the last stitch: a short stitch back along it and forward again, before the
/// thread is cut.
fn tie_off(out: &mut Vec<Stitch>) {
    let [.., a, b] = out.as_slice() else { return };
    let (a, b) = (*a, *b);
    if a.command != Command::Stitch || b.command != Command::Stitch {
        return;
    }
    let Some(d) = unit(a.point() - b.point()) else { return };
    out.push(Stitch::sew(b.point() + d * LOCK.min(a.distance(b))));
    out.push(b);
}

/// The spacing of a satin column's crossings for its `width` (auto spacing): more open in a narrow
/// column, where the thread bunches at the rails and covers anyway, denser in a wide one so it
/// covers. A stepped table, as professional software keeps one; the values are estimates.
pub fn satin_spacing(width: f64) -> f64 {
    const TABLE: [(f64, f64); 3] = [(2.0, 0.5), (4.0, 0.45), (6.0, 0.4)];
    TABLE.iter().find(|(w, _)| width < *w).map_or(0.37, |(_, spacing)| *spacing)
}

/// The underlay of a satin column along `pts` (its centreline), `width` wide, by width: a centre
/// run for a narrow column, an edge run (along one side and back along the other) for a medium one,
/// and an edge run then a zigzag between its sides for a wide one. Each run comes with whether it
/// ends at the column's far end. The thresholds are estimates from the usual practice.
fn satin_underlay(pts: &[Point], closed: bool, width: f64) -> Vec<(Vec<Point>, bool)> {
    let centre = resample(pts, UNDERLAY_STITCH, closed);
    if width < SATIN_EDGE_UNDERLAY {
        return vec![(centre, !closed)];
    }
    // The sides, kept inside the column so the underlay doesn't show at its edges.
    let side = (width / 2.0 - (width / 4.0).min(0.5)).max(0.0);
    let offset = |samples: &[Point], i: usize, d: f64| -> Option<Point> {
        let q = *samples.get(i)?;
        let t = tangent(samples, i, closed);
        Some(q + Vec2::new(-t.y, t.x) * d)
    };
    let mut edge: Vec<Point> = (0..centre.len()).filter_map(|i| offset(&centre, i, side)).collect();
    edge.extend((0..centre.len()).rev().filter_map(|i| offset(&centre, i, -side)));
    let mut out = vec![(edge, false)];
    if width >= SATIN_ZIGZAG_UNDERLAY {
        let steps = resample(pts, ZIGZAG_UNDERLAY_SPACING, closed);
        let zigzag: Vec<Point> = (0..steps.len()).filter_map(|i| offset(&steps, i, if i % 2 == 0 { side } else { -side })).collect();
        out.push((zigzag, !closed));
    }
    out
}

/// `zigzag`'s crossings longer than `max` split into shorter stitches (auto split). Each long
/// crossing is cut into equal parts shifted by a fraction that changes from one crossing to the
/// next (the golden ratio's sequence), so the splits of neighbouring crossings never line up into
/// a seam down the column; no part is longer than `max`.
fn split_long(zigzag: &[Point], max: f64) -> Vec<Point> {
    /// How far the cuts move along a crossing, as a share of one part (either way).
    const JITTER: f64 = 0.25;
    let mut out: Vec<Point> = Vec::with_capacity(zigzag.len());
    let mut crossing = 0u32;
    for (i, &q) in zigzag.iter().enumerate() {
        if let Some(&p) = i.checked_sub(1).and_then(|j| zigzag.get(j)) {
            let len = p.distance(q);
            if len.is_finite() && len > max {
                // The longest part is (1 + 2·JITTER)/n of the crossing: enough parts that it fits.
                let n = ((len * (1.0 + 2.0 * JITTER)) / max).ceil().clamp(2.0, 64.0) as u32;
                crossing += 1;
                let shift = ((f64::from(crossing) * 0.618_033_988_75).fract() - 0.5) * 2.0 * JITTER;
                for k in 1..n {
                    let t = (f64::from(k) + shift) / f64::from(n);
                    out.push(p.lerp(q, t));
                }
            }
        }
        out.push(q);
    }
    out
}

/// A run with a lock stitch at each end: a short stitch back along the run and forward again, so
/// the thread stays put. Both stay on the run: forward from its start, back from its end (never
/// past it).
fn locked(pts: &[Point]) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return pts.to_vec() };
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out: Vec<Point> = Vec::with_capacity(pts.len() + 6);
    if let Some(&next) = pts.get(1)
        && let Some(d) = unit(next - first)
    {
        let reach = LOCK.min(first.distance(next));
        out.push(first);
        out.push(first + d * reach);
    }
    out.extend_from_slice(pts);
    if let Some(&prev) = pts.len().checked_sub(2).and_then(|i| pts.get(i))
        && let Some(d) = unit(prev - last)
    {
        let reach = LOCK.min(last.distance(prev));
        out.push(last + d * reach);
        out.push(last);
    }
    out
}

/// A polyline: its points and whether it closes.
struct SubPath {
    pts: Vec<Point>,
    closed: bool,
}

impl SubPath {
    /// The points, with a closed polyline's first point repeated at the end.
    fn points(&self) -> Vec<Point> {
        let mut v = self.pts.clone();
        if self.closed
            && let (Some(&a), Some(&b)) = (v.first(), v.last())
            && a.distance(b) > 1e-9
        {
            v.push(a);
        }
        v
    }
}

/// `path`'s curves as polylines, within [`FLATTEN`] of them.
fn subpaths(path: &BezPath) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut cur: Option<SubPath> = None;
    kurbo::flatten(path.iter(), FLATTEN, |el| match el {
        PathEl::MoveTo(p) => {
            if let Some(s) = cur.take()
                && s.pts.len() >= 2
            {
                out.push(s);
            }
            cur = Some(SubPath { pts: vec![p], closed: false });
        }
        PathEl::LineTo(p) => match &mut cur {
            Some(s) => s.pts.push(p),
            None => cur = Some(SubPath { pts: vec![p], closed: false }),
        },
        PathEl::ClosePath => {
            if let Some(s) = &mut cur {
                s.closed = true;
            }
        }
        _ => {}
    });
    if let Some(s) = cur
        && s.pts.len() >= 2
    {
        out.push(s);
    }
    out
}

/// A turn sharper than this (radians, about 30°) is a corner the stitches keep.
const CORNER: f64 = 0.52;

/// Points along a polyline every `step` millimetres, both ends included. A closed polyline ends
/// where it started. Sharp corners get a point of their own (the steps start again from them),
/// so stitches follow the outline instead of cutting across a corner — into a notch or a gap
/// beside it.
fn resample(pts: &[Point], step: f64, closed: bool) -> Vec<Point> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return Vec::new() };
    let step = step.max(0.01);
    let mut out: Vec<Point> = vec![first];
    let mut travelled = 0.0;
    let mut next = step;
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let at = |i: usize| pts.get(if i >= n { i - n } else { i }).copied();
    for i in 0..segs {
        let (Some(a), Some(b)) = (at(i), at(i + 1)) else { continue };
        let seg = a.distance(b);
        if seg <= 1e-12 {
            continue;
        }
        while next <= travelled + seg {
            let f = (next - travelled) / seg;
            out.push(a + (b - a) * f);
            next += step;
            if out.len() >= MAX_STITCHES {
                return out;
            }
        }
        travelled += seg;
        // A sharp corner at `b` (not the polyline's end): stitch into it, moving the last point
        // there when it is close (no stub stitch), and count the next step from it.
        let has_next = i + 1 < segs;
        if has_next
            && let Some(c) = at(i + 2)
            && let (Some(u), Some(v)) = (unit(b - a), unit(c - b))
            && u.dot(v).clamp(-1.0, 1.0).acos() > CORNER
        {
            let movable = out.len() > 1;
            match out.last_mut() {
                Some(q) if movable && q.distance(b) < 0.3 * step => *q = b,
                Some(q) if q.distance(b) <= 1e-9 => {}
                _ => out.push(b),
            }
            next = travelled + step;
        }
    }
    if closed {
        if out.last().is_some_and(|q| q.distance(first) > 1e-9) {
            out.push(first);
        }
    } else if out.last().is_none_or(|q| q.distance(last) > 1e-9) {
        out.push(last);
    }
    out
}

/// The total length of a polyline, millimetres.
fn length(pts: &[Point], closed: bool) -> f64 {
    let n = pts.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut total = 0.0;
    for i in 0..segs {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get(if i + 1 == n { 0 } else { i + 1 })) else { continue };
        let d = a.distance(b);
        if d.is_finite() {
            total += d;
        }
    }
    total
}

/// The unit tangent at point `i` of `pts`: the direction the line runs through it.
fn tangent(pts: &[Point], i: usize, closed: bool) -> Vec2 {
    let n = pts.len();
    let prev = if i > 0 {
        Some(i - 1)
    } else if closed && n > 1 {
        Some(n - 1)
    } else {
        None
    };
    let next = if i + 1 < n {
        Some(i + 1)
    } else if closed && n > 1 {
        Some(0)
    } else {
        None
    };
    let mut t = Vec2::ZERO;
    if let (Some(p), Some(q)) = (prev.and_then(|j| pts.get(j)), next.and_then(|j| pts.get(j))) {
        t = *q - *p;
    }
    if t.hypot() < 1e-9
        && let Some(q) = next.and_then(|j| pts.get(j))
        && let Some(p) = pts.get(i)
    {
        t = *q - *p;
    }
    unit(t).unwrap_or(Vec2::new(1.0, 0.0))
}

/// `v` scaled to length one, `None` when it has no length.
fn unit(v: Vec2) -> Option<Vec2> {
    let len = v.hypot();
    if len.is_finite() && len > 1e-12 { Some(v / len) } else { None }
}

/// Remove repeated points from a run (a back stitch repeats its point on purpose, but a
/// duplicated sample is noise).
fn dedupe(pts: &mut Vec<Point>) {
    let mut i = 0;
    while i + 1 < pts.len() {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get(i + 1))
            && a.distance(*b) <= 1e-9
        {
            pts.remove(i + 1);
            continue;
        }
        i += 1;
    }
}

/// The two unit vectors a fill's rows are laid along: `along` (the row direction at
/// [`Params::angle`]) and `across` (the direction the rows step in).
fn axes(angle_deg: f64) -> (Vec2, Vec2) {
    let a = angle_deg * TAU / 360.0;
    let (s, c) = a.sin_cos();
    let along = Vec2::new(c, s);
    let across = Vec2::new(-s, c);
    if along.hypot().is_finite() && across.hypot().is_finite() { (along, across) } else { (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)) }
}

/// Where `p` falls along `v`.
fn dot(p: Point, v: Vec2) -> f64 {
    p.x * v.x + p.y * v.y
}

/// The point `s` along and `t` across the fill's axes.
fn at(s: f64, t: f64, along: Vec2, across: Vec2) -> Point {
    Point::new(s * along.x + t * across.x, s * along.y + t * across.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to((x, y));
        p.line_to((x + w, y));
        p.line_to((x + w, y + h));
        p.line_to((x, y + h));
        p.close_path();
        p
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> BezPath {
        let mut p = BezPath::new();
        p.move_to(a);
        p.line_to(b);
        p
    }

    fn points(v: &[Stitch]) -> Vec<Point> {
        v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
    }

    #[test]
    fn a_running_line_steps_at_the_stitch_length() {
        let p = Params::for_style(Style::Running);
        let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
        let q = points(&out);
        assert!(q.len() >= 8, "{q:?}");
        assert!((q[0].x - 0.0).abs() < 1e-6 && (q[0].y).abs() < 1e-6);
        assert!((q.last().expect("last").x - 20.0).abs() < 1e-6, "{:?}", q.last());
        for w in q.windows(2) {
            let d = w[0].distance(w[1]);
            assert!(d <= p.stitch_length + 1e-6, "{d}");
        }
        // The lock stitches stay on the line, never past either end.
        assert!(q.iter().all(|q| q.y.abs() < 1e-9));
        assert!(q.iter().all(|q| q.x >= -1e-9 && q.x <= 20.0 + 1e-9), "{q:?}");
        // …and each is a back-and-forth at its end.
        assert!((q[1].x - LOCK).abs() < 1e-9 && (q[q.len() - 2].x - (20.0 - LOCK)).abs() < 1e-9, "{q:?}");
    }

    #[test]
    fn a_fill_covers_the_rectangle_and_no_stitch_is_longer_than_asked() {
        let p = Params::for_style(Style::Fill);
        let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
        let q = points(&out);
        assert!(q.len() > 100, "{}", q.len());
        for a in &q {
            assert!(a.x >= -1e-6 && a.x <= 20.0 + 1e-6 && a.y >= -1e-6 && a.y <= 10.0 + 1e-6, "{a:?}");
        }
        for w in out.windows(2) {
            // Neighbouring penetrations of one row (same y, needle down twice in a row) are at
            // most the stitch length plus half a stagger apart; row turns and travels are longer,
            // and a jump between two stitches is not a stitch at all.
            let (a, b) = (w[0], w[1]);
            if a.command == Command::Stitch && b.command == Command::Stitch && (a.y - b.y).abs() < 1e-6 {
                assert!(a.distance(b) <= p.stitch_length * 1.5 + 1e-6, "{:?} .. {:?}", a.point(), b.point());
            }
        }
        // About the area divided by the thread's coverage.
        let area = 20.0 * 10.0;
        let expected = area / (p.density * p.stitch_length);
        assert!((q.len() as f64) > expected * 0.5, "{} vs {expected}", q.len());
    }

    #[test]
    fn a_hole_is_left_unstitched() {
        // A ring: the outer square wound one way, the inner square the other.
        let mut p = rect(0.0, 0.0, 30.0, 30.0);
        let mut hole = BezPath::new();
        hole.move_to((10.0, 10.0));
        hole.line_to((10.0, 20.0));
        hole.line_to((20.0, 20.0));
        hole.line_to((20.0, 10.0));
        hole.close_path();
        p.extend(hole);
        let params = Params::for_style(Style::Fill);
        let out = stitches(&p, FillRule::NonZero, &params);
        let q = points(&out);
        assert!(q.len() > 50);
        for a in &q {
            let in_hole = a.x > 10.0 + 0.2 && a.x < 20.0 - 0.2 && a.y > 10.0 + 0.2 && a.y < 20.0 - 0.2;
            assert!(!in_hole, "{a:?}");
        }
    }

    /// Sewn stitches (needle down at both ends) whose middle falls inside `hole`, deeper than
    /// half a thread (a row turn may shave a hole's corner by less, as any tatami does).
    fn over(out: &[Stitch], hole: (f64, f64, f64, f64)) -> usize {
        const DEPTH: f64 = 0.25;
        let (x0, y0, x1, y1) = hole;
        out.windows(2)
            .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
            .filter(|w| {
                let (x, y) = ((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
                x > x0 + DEPTH && x < x1 - DEPTH && y > y0 + DEPTH && y < y1 - DEPTH
            })
            .count()
    }

    /// A ring is sewn region by region (above the hole, its two sides, below it), not a row at
    /// a time: a handful of trims, not one per row, and no thread across the hole.
    #[test]
    fn a_ring_is_sewn_in_regions() {
        for hole in [2.0, 4.0, 10.0] {
            let a = 15.0 - hole / 2.0;
            let mut p = rect(0.0, 0.0, 30.0, 30.0);
            let mut h = BezPath::new();
            h.move_to((a, a));
            h.line_to((a, a + hole));
            h.line_to((a + hole, a + hole));
            h.line_to((a + hole, a));
            h.close_path();
            p.extend(h);
            for angle in [0.0, 45.0, 90.0] {
                let mut params = Params::for_style(Style::Fill);
                params.angle = angle;
                let out = stitches(&p, FillRule::NonZero, &params);
                let trims = out.iter().filter(|s| s.command == Command::Trim).count();
                // Four regions, and the underlay's move from the outer edge to the hole's.
                assert!(trims <= 6, "hole {hole}, angle {angle}: {trims} trims");
                assert_eq!(over(&out, (a, a, a + hole, a + hole)), 0, "hole {hole}, angle {angle}");
            }
        }
    }

    /// A running line round a square keeps its corners: every corner is a needle point.
    #[test]
    fn running_stitches_keep_sharp_corners() {
        let mut p = Params::for_style(Style::Running);
        p.stitch_length = 3.0;
        let out = stitches(&rect(0.0, 0.0, 10.0, 7.0), FillRule::NonZero, &p);
        for corner in [(10.0, 0.0), (10.0, 7.0), (0.0, 7.0)] {
            assert!(out.iter().any(|s| (s.x - corner.0).abs() < 1e-9 && (s.y - corner.1).abs() < 1e-9), "{corner:?}");
        }
        // No stub stitches at the corners.
        for w in out.windows(2) {
            assert!(w[0].distance(w[1]) > 0.3 * 3.0 - 1e-9 || w[0].distance(w[1]) < 1e-9, "{:?} {:?}", w[0], w[1]);
        }
    }

    /// A U whose arms are closer than the jump length: the thread never runs across the gap.
    #[test]
    fn a_narrow_gap_is_not_bridged() {
        let mut u = BezPath::new();
        for (x, y) in [(0.0, 0.0), (8.0, 0.0), (8.0, 20.0), (11.0, 20.0), (11.0, 0.0), (19.0, 0.0), (19.0, 30.0), (0.0, 30.0)] {
            if u.elements().is_empty() {
                u.move_to((x, y));
            } else {
                u.line_to((x, y));
            }
        }
        u.close_path();
        let mut params = Params::for_style(Style::Fill);
        params.angle = 0.0;
        let out = stitches(&u, FillRule::NonZero, &params);
        assert!(out.len() > 100);
        assert_eq!(over(&out, (8.0, -1.0, 11.0, 20.0)), 0, "a stitch crosses the gap");
    }

    /// Fill and satin tie the thread in where it starts and off before every cut and at the end:
    /// a stitch back and forth on the spot.
    #[test]
    fn fills_and_satins_tie_in_and_off() {
        let mut two = rect(0.0, 0.0, 10.0, 10.0);
        two.extend(rect(60.0, 0.0, 10.0, 10.0));
        let mut satin = Params::for_style(Style::Satin);
        satin.width = 3.0;
        for (shape, p) in [(&two, Params::for_style(Style::Fill)), (&line((0.0, 0.0), (20.0, 0.0)), satin)] {
            let out = stitches(shape, FillRule::NonZero, &p);
            // Back and forth: the third stitch returns to the first.
            let returns = |w: &[Stitch]| w[0].command == Command::Stitch && w[2].command == Command::Stitch && w[0].distance(w[2]) < 1e-9;
            assert!(returns(&out[..3]), "tie-in: {:?}", &out[..3]);
            assert!(returns(&out[out.len() - 3..]), "tie-off: {:?}", &out[out.len() - 3..]);
            for (i, _) in out.iter().enumerate().filter(|(_, s)| s.command == Command::Trim) {
                assert!(i >= 3 && returns(&out[i - 3..i]), "tie-off before the trim at {i}");
                assert!(returns(&out[i + 2..i + 5]), "tie-in after the trim at {i}");
            }
            let mut loose = p;
            loose.lock = false;
            assert!(stitches(shape, FillRule::NonZero, &loose).len() < out.len());
        }
    }

    /// A satin column wider than the longest satin stitch is split: no stitch longer than that,
    /// and the cuts of neighbouring crossings don't line up into a seam.
    #[test]
    fn wide_satins_are_split_without_a_seam() {
        let mut p = Params::for_style(Style::Satin);
        p.width = 20.0;
        p.pull = 0.0;
        p.underlay = false;
        p.lock = false;
        let out = stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &p);
        for w in out.windows(2) {
            assert!(w[0].distance(w[1]) <= p.max_satin + 1e-9, "{:?} {:?}", w[0], w[1]);
        }
        // The cuts inside the column (not on its rails) sit at many different heights.
        let mut inner: Vec<i64> = out.iter().filter(|s| s.y.abs() < 9.9).map(|s| (s.y * 10.0).round() as i64).collect();
        inner.sort_unstable();
        inner.dedup();
        assert!(inner.len() > 10, "{inner:?}");
        // A narrow column is not split at all.
        let mut narrow = p;
        narrow.width = 4.0;
        let out = stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &narrow);
        assert!(out.iter().all(|s| s.y.abs() > 1.9), "every penetration on a rail");
    }

    /// A ring's regions are joined by travel stitches inside it, not by trims, and each region
    /// overlaps its neighbours by a row so no gap opens along the seams.
    #[test]
    fn regions_travel_inside_and_overlap() {
        let mut ring = rect(0.0, 0.0, 30.0, 30.0);
        let mut h = BezPath::new();
        h.move_to((10.0, 10.0));
        h.line_to((10.0, 20.0));
        h.line_to((20.0, 20.0));
        h.line_to((20.0, 10.0));
        h.close_path();
        ring.extend(h);
        for angle in [0.0, 30.0, 90.0] {
            let mut p = Params::for_style(Style::Fill);
            p.angle = angle;
            p.underlay = false;
            let out = stitches(&ring, FillRule::NonZero, &p);
            let trims = out.iter().filter(|s| s.command == Command::Trim).count();
            assert!(trims <= 1, "angle {angle}: {trims} trims");
            assert_eq!(over(&out, (10.0, 10.0, 20.0, 20.0)), 0, "angle {angle}");
        }
        // Rows sewn twice where regions meet: at 0° the rows just above and below the hole.
        let mut p = Params::for_style(Style::Fill);
        p.angle = 0.0;
        p.underlay = false;
        let out = stitches(&ring, FillRule::NonZero, &p);
        let plain = stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p);
        let count = |v: &[Stitch], y: f64| v.iter().filter(|s| s.command == Command::Stitch && (s.y - y).abs() < 1e-6).count();
        // The last row above the hole (rows sit at 0.2, 0.6, 1.0 … mm).
        let above = (0..100).map(|k| 0.2 + 0.4 * f64::from(k)).take_while(|y| *y < 10.0).last().unwrap_or(0.0);
        assert!(count(&out, above) > count(&plain, above), "the row above the hole is sewn by two regions");
    }

    /// A satin's underlay follows its width — centre run, edge run, edge run and zigzag — and the
    /// top stitching starts where the underlay ended: no trim and no long move along the column.
    #[test]
    fn satin_underlay_follows_the_width() {
        for (width, want_centre, want_zigzag) in [(1.5, true, false), (3.0, false, false), (6.0, false, true)] {
            let mut p = Params::for_style(Style::Satin);
            p.width = width;
            p.pull = 0.0;
            let out = stitches(&line((0.0, 0.0), (40.0, 0.0)), FillRule::NonZero, &p);
            assert!(out.iter().all(|s| s.command == Command::Stitch), "{width} mm: no trim or jump");
            for w in out.windows(2) {
                assert!(w[0].distance(w[1]) <= p.max_satin.max(UNDERLAY_STITCH) + 1e-6, "{width} mm: {:?} {:?}", w[0], w[1]);
            }
            let side = width / 2.0 - (width / 4.0).min(0.5);
            let on = |y: f64| out.iter().any(|s| (s.y - y).abs() < 1e-6 && s.x > 5.0 && s.x < 35.0);
            assert_eq!(on(0.0), want_centre, "{width} mm: a centre run");
            assert_eq!(on(side) && on(-side), !want_centre, "{width} mm: an edge run");
            // A zigzag underlay: stitches from one side to the other, 2 mm along.
            let crossings =
                out.windows(2).filter(|w| (w[0].y - side).abs() < 1e-6 && (w[1].y + side).abs() < 1e-6 && (w[1].x - w[0].x).abs() > 1.0).count();
            assert_eq!(crossings > 3, want_zigzag, "{width} mm: a zigzag");
        }
    }

    /// A large fill gets a second, sparse underlay across its rows; a small one doesn't.
    #[test]
    fn large_fills_get_a_tatami_underlay() {
        let mut p = Params::for_style(Style::Fill);
        p.angle = 0.0;
        // Rows of the underlay run at 90°: vertical stitches, 3 mm long, inside the edge.
        // (Away from the shape's left and right edges, where the edge run is vertical too.)
        let vertical = |out: &[Stitch], width: f64| {
            out.windows(2)
                .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
                .filter(|w| (w[0].x - w[1].x).abs() < 1e-6 && (w[0].y - w[1].y).abs() > 1.0 && w[0].x > 0.1 && w[0].x < width - 0.1)
                .count()
        };
        assert!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0) > 20);
        assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 8.0, 30.0), FillRule::NonZero, &p), 8.0), 0);
        p.underlay = false;
        assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0), 0);
    }

    /// Satin spacing follows the column's width unless a density is given.
    #[test]
    fn satin_spacing_follows_the_width() {
        // The distance between two crossings on the same rail is twice the spacing.
        let rail_gap = |width: f64, auto: bool| {
            let mut p = Params::for_style(Style::Satin);
            p.width = width;
            p.pull = 0.0;
            p.underlay = false;
            p.lock = false;
            p.auto_spacing = auto;
            p.density = 0.3;
            let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
            let top: Vec<f64> = out.iter().filter(|s| (s.y - width / 2.0).abs() < 1e-6).map(|s| s.x).collect();
            (top[2] - top[1]) / 2.0
        };
        assert!((rail_gap(1.5, true) - 0.5).abs() < 0.02);
        assert!((rail_gap(5.0, true) - 0.4).abs() < 0.02);
        assert!(rail_gap(1.5, true) > rail_gap(8.0, true));
        assert!((rail_gap(5.0, false) - 0.3).abs() < 0.02, "a given density is kept");
    }

    #[test]
    fn the_angle_turns_the_rows() {
        let mut p = Params::for_style(Style::Fill);
        p.angle = 90.0;
        let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
        let q = points(&out);
        // Rows run vertically: each row keeps one x, rows step by the density, and a row spans
        // the shape's height.
        let mut xs: Vec<f64> = q.iter().map(|a| a.x).collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        assert!(xs.len() >= 10, "{xs:?}");
        assert!((xs[1] - xs[0]).abs() - p.density < 0.2, "{:?}", &xs[..2]);
        assert!(q.iter().any(|a| a.y > 9.0), "a vertical row spans the height");
    }

    #[test]
    fn a_satin_column_crosses_its_width() {
        let p = Params::for_style(Style::Satin);
        let mut params = p;
        params.width = 4.0;
        params.pull = 0.0;
        let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &params);
        let q = points(&out);
        assert!(q.len() > 20, "{}", q.len());
        assert!(q.iter().all(|a| a.y.abs() <= 2.0 + 1e-6), "{q:?}");
        // Neighbouring stitches are on opposite rails.
        let rails: Vec<f64> = q.iter().skip(1).map(|a| a.y).collect();
        assert!(rails.iter().any(|y| *y > 1.5) && rails.iter().any(|y| *y < -1.5));
        let mut wider = params;
        wider.pull = 0.5;
        let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &wider);
        let w = points(&out);
        assert!(w.iter().any(|a| a.y.abs() > 2.2), "pull compensation widens the column");
    }

    #[test]
    fn shapes_far_apart_are_trimmed() {
        let mut p = rect(0.0, 0.0, 10.0, 10.0);
        p.extend(rect(60.0, 0.0, 10.0, 10.0));
        let params = Params::for_style(Style::Fill);
        let out = stitches(&p, FillRule::NonZero, &params);
        assert!(out.iter().any(|s| s.command == Command::Trim), "a long travel cuts the thread");
    }

    /// A shape that would need millions of stitches is refused before any is planned.
    #[test]
    fn an_absurd_plan_is_refused_up_front() {
        let mut p = Params::for_style(Style::Fill);
        p.density = 0.05;
        p.stitch_length = 0.2;
        // Five metres square.
        let huge = rect(0.0, 0.0, 5000.0, 5000.0);
        assert!(matches!(try_stitches(&huge, FillRule::NonZero, &p), Err(Error::TooLarge(_))));
        assert!(try_stitches(&rect(0.0, 0.0, 50.0, 50.0), FillRule::NonZero, &p).is_ok_and(|s| !s.is_empty()));
        let mut run = Params::for_style(Style::Running);
        run.stitch_length = 0.2;
        assert!(matches!(try_stitches(&line((0.0, 0.0), (1e7, 0.0)), FillRule::NonZero, &run), Err(Error::TooLarge(_))));
    }

    #[test]
    fn nothing_panics_on_odd_input() {
        let empty = BezPath::new();
        assert!(stitches(&empty, FillRule::NonZero, &Params::default()).is_empty());
        let mut p = Params::for_style(Style::Fill);
        p.density = f64::NAN;
        p.stitch_length = 0.0;
        p.angle = f64::INFINITY;
        p.pull = -5.0;
        p.jump = f64::NAN;
        let out = stitches(&rect(0.0, 0.0, 10.0, 10.0), FillRule::NonZero, &p);
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
        // A zero-length path has no stitches, and a single-point path neither.
        assert!(stitches(&line((1.0, 1.0), (1.0, 1.0)), FillRule::NonZero, &Params::default()).is_empty());
    }
}
