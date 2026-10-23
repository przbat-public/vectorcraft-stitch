//! One sewing unit's stitches from what it is planned from ([`UnitRequest`]): the planning step
//! the document walk (`digitize`) takes for each fill and stroke, with nothing of the document or
//! of the units around it but where the needle comes from.
//!
//! The step is pure: the same request always plans the same stitches. That is what lets a unit's
//! stitches be kept between plans and used again when nothing it is planned from has changed.

use vectorcraft_doc::cancel;
use vectorcraft_geom::kurbo::ParamCurveExtrema;
use vectorcraft_geom::{BezPath, FillRule, PathData, PathSeg, Point, Rect, Shape};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, outline_stroke, unite_all};
use vectorcraft_stitch::plan::{self, Params};
use vectorcraft_stitch::{Command, Stitch};

/// The miter limit given for a band grown along an outline (a fill's `expand`, a narrow column's
/// [`COLUMN_GROWTH`]): its joins are round, which have none; this is a stroke's usual default.
pub(super) const ROUND_JOINS: f64 = 4.0;

/// How much each narrow column's outline grows before it is cut out of its shape (millimetres): a
/// hair, so that it covers its own sides, which lie on the shape's outline, and leaves no sliver of
/// the shape there. Far below a thread's width.
const COLUMN_GROWTH: f64 = 0.05;

/// Why a design, or a unit of it, could not be planned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlanError {
    /// It would need too many stitches: why, and what to change.
    TooLarge(String),
    /// The plan was cancelled ([`vectorcraft_doc::cancel`]) before it was done.
    Cancelled,
}

impl PlanError {
    /// The error a command reports: a design too large is the design's limit, not a wrong
    /// parameter (a caller that fixes its parameters on `BadParams` would only loop); a cancel is
    /// a cancel.
    pub(crate) fn error(self) -> crate::EngineError {
        match self {
            Self::TooLarge(why) => crate::EngineError::Other(why),
            Self::Cancelled => crate::EngineError::Cancelled,
        }
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(why) => f.write_str(why),
            Self::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl From<vectorcraft_stitch::Error> for PlanError {
    fn from(e: vectorcraft_stitch::Error) -> Self {
        Self::TooLarge(e.to_string())
    }
}

/// Everything one sewing unit's stitches depend on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitRequest<'a> {
    /// The shape in millimetres, as it is sewn: transformed into place, and for a fill grown or
    /// shrunk by `expand` and cut to the clip groups around it.
    pub(crate) shape: &'a BezPath,
    pub(crate) rule: FillRule,
    /// The parameters it is planned with; their style says how it is sewn.
    pub(crate) params: Params,
    /// A fill whose narrow parts may be sewn as satin columns: the parameters those columns are
    /// sewn with. `None` for everything else (strokes, and fills the `stitch` option forces to tatami).
    pub(crate) columns: Option<Params>,
    /// Where the needle is when the unit starts (`None`: nowhere yet, the design's first unit), so
    /// that it starts as near there as it can.
    pub(crate) from: Option<Point>,
    /// For a stroke inside clip groups: the region they leave visible (millimetres), to which its
    /// stitches are cut. `None` for fills, whose shape is cut before it is planned.
    pub(crate) clip: Option<&'a (BezPath, FillRule)>,
}

/// A unit's stitches as planned, the parameters its narrow parts were sewn with as satin columns
/// (`None`: it had none), and the typical widths of the narrowest and the widest of those columns
/// (millimetres).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UnitStitches {
    pub(crate) stitches: Vec<Stitch>,
    pub(crate) columns: Option<Params>,
    pub(crate) widths: Option<[f64; 2]>,
}

/// The stitches `req` asks for: the shape planned in its style from where the needle is, a fill's
/// narrow parts sewn as satin columns that follow the shape, a stroke inside clip groups cut to
/// them; an error when the shape would need too many stitches, or when the plan is cancelled
/// while its narrow parts are looked for or cut out.
pub(crate) fn plan_unit(req: &UnitRequest<'_>) -> Result<UnitStitches, PlanError> {
    let UnitRequest { shape, rule, params, columns, from, clip } = *req;
    let found = match columns {
        Some(_) => plan::narrow_columns_until(shape, rule, params.narrow, &cancel::is_cancelled).ok_or(PlanError::Cancelled)?,
        None => Vec::new(),
    };
    // The widths of the columns sewn as satin (a narrower one is a running line).
    let widths = found.iter().filter(|c| c.is_satin()).map(plan::Column::width).fold(None, |range: Option<[f64; 2]>, w| match range {
        Some([narrowest, widest]) => Some([narrowest.min(w), widest.max(w)]),
        None => Some([w, w]),
    });
    let (planned, columns) = match columns.filter(|_| !found.is_empty()) {
        None => (plan::try_stitches_from(shape, rule, &params, from), None),
        Some(satin) => {
            let (rest, whole) = rest_of(shape, rule, &found)?;
            (plan::try_fill_with_columns(&whole, FillRule::NonZero, &rest, &found, &params, &satin, from), Some(satin))
        }
    };
    let mut stitches = planned?;
    // A stroke inside a clip group: only the stitches inside the clipping path are sewn. (A
    // filled area was cut to it exactly; its edge run lies on the cut, which a point test would
    // chop.)
    if let Some(clip) = clip {
        stitches = cut_to(stitches, clip, &params);
    }
    Ok(UnitStitches { stitches, columns, widths })
}

/// `area` without its narrow `columns` ([`plan::narrow_columns`]): what is left to fill, each
/// column's outline grown by [`COLUMN_GROWTH`] and cut out; and the rest and the grown columns
/// together, the area the needle travels in. (The rest's edges along the outline come from the
/// path operations, a hair off the outline as the planner flattens it: travelling in the rest
/// and the columns as the same operations give them, every row's end lies on or inside it.) An
/// error when the plan is cancelled between two columns.
fn rest_of(area: &BezPath, rule: FillRule, columns: &[plan::Column]) -> Result<(BezPath, BezPath), PlanError> {
    // Every column's outline and the band along it, united in one sweep (uniting them one at a
    // time swept everything so far again for each column).
    let mut parts: Vec<PathData> = Vec::with_capacity(2 * columns.len());
    for c in columns {
        cancel::check().map_err(|_| PlanError::Cancelled)?;
        let outline = PathData::from_bezpath(&c.outline());
        parts.push(outline_stroke(&outline, 2.0 * COLUMN_GROWTH, Cap::Round, Join::Round, ROUND_JOINS));
        parts.push(outline);
    }
    let cut = (!parts.is_empty()).then(|| unite_all(&parts.iter().map(|p| (p, FillRule::NonZero)).collect::<Vec<_>>()));
    match cut {
        Some(cut) => {
            let rest = boolean(&PathData::from_bezpath(area), rule, &cut, FillRule::NonZero, BoolOp::Difference);
            let whole = boolean(&rest, FillRule::NonZero, &cut, FillRule::NonZero, BoolOp::Union);
            Ok((rest.to_bezpath(), whole.to_bezpath()))
        }
        None => Ok((area.clone(), area.clone())),
    }
}

/// The stitches inside `region`: a run that leaves it is cut there and taken up again where it
/// comes back in, the needle lifted between as the planner lifts it (`p`'s jump and trim lengths,
/// the thread tied off and in around a long jump).
pub(super) fn cut_to(stitches: Vec<Stitch>, region: &(BezPath, FillRule), p: &Params) -> Vec<Stitch> {
    let (path, rule) = region;
    let banded = Banded::new(path);
    let inside = |s: &Stitch| {
        let w = banded.winding(s.point());
        match rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    // The stretches inside, each with the plan's own jumps and trims within it.
    let mut pieces: Vec<Vec<Stitch>> = vec![Vec::new()];
    for s in stitches {
        if s.command == Command::Stitch && !inside(&s) {
            if pieces.last().is_some_and(|p| !p.is_empty()) {
                pieces.push(Vec::new());
            }
            continue;
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(s);
        }
    }
    // A piece that ends on the plan's own jump or trim drops it: the joining lifts the needle.
    for piece in &mut pieces {
        while piece.last().is_some_and(|s| s.command != Command::Stitch) {
            piece.pop();
        }
    }
    plan::join_cut(pieces, p)
}

/// How far past a segment's box it is still put in a band (millimetres): the box is worked out
/// from the curve as the winding test works the curve's pieces out, to within rounding.
const BOX_MARGIN: f64 = 1e-7;
/// The most horizontal bands [`Banded`] sorts segments into, and the most bands a segment is in
/// on average: past that (segments as tall as the path, for their number) the whole path is
/// tested. Bounds on the work and memory.
const MAX_BANDS: usize = 4096;
const ENTRIES_PER_SEGMENT: usize = 32;

/// A path's segments sorted into horizontal bands, so that whether a point is inside it is worked
/// out from the segments near the point's height alone: a stroke in a clip group tests every
/// stitch against the clipping path, which may have thousands of segments. The winding is
/// kurbo's (`BezPath::winding`) exactly: that sums, segment by segment, the crossings of a ray to
/// the left of the point, and a segment whose box lies above or below the point adds nothing. So
/// each band is a path of the segments whose boxes reach into it, each a subpath of its own (a
/// move to its start, then the segment), and a point's winding is its band's.
struct Banded {
    whole: BezPath,
    y0: f64,
    height: f64,
    /// Each band's segments; none when the whole path is tested.
    bands: Vec<BezPath>,
}

impl Banded {
    fn new(path: &BezPath) -> Self {
        let segments: Vec<(PathSeg, Rect)> =
            path.segments().map(|s| (s, ParamCurveExtrema::bounding_box(&s).inflate(BOX_MARGIN, BOX_MARGIN))).collect();
        let (y0, y1) = segments.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, b)| (lo.min(b.y0), hi.max(b.y1)));
        let whole = Self { whole: path.clone(), y0: 0.0, height: 0.0, bands: Vec::new() };
        let n = (segments.len() / 2).clamp(1, MAX_BANDS);
        let height = (y1 - y0) / n as f64;
        if !(height.is_finite() && height > 0.0) {
            return whole;
        }
        let band = |y: f64| (((y - y0) / height).floor().max(0.0) as usize).min(n - 1);
        let mut bands: Vec<BezPath> = vec![BezPath::new(); n];
        let budget = segments.len().saturating_mul(ENTRIES_PER_SEGMENT);
        let mut entries = 0usize;
        for (seg, b) in &segments {
            let (first, last) = (band(b.y0), band(b.y1));
            entries = entries.saturating_add(last - first + 1);
            if entries > budget {
                return whole;
            }
            for bp in bands.get_mut(first..=last).into_iter().flatten() {
                match *seg {
                    PathSeg::Line(l) => {
                        bp.move_to(l.p0);
                        bp.line_to(l.p1);
                    }
                    PathSeg::Quad(q) => {
                        bp.move_to(q.p0);
                        bp.quad_to(q.p1, q.p2);
                    }
                    PathSeg::Cubic(c) => {
                        bp.move_to(c.p0);
                        bp.curve_to(c.p1, c.p2, c.p3);
                    }
                }
            }
        }
        Self { bands, y0, height, ..whole }
    }

    /// The path's winding number about `q`, as `BezPath::winding` gives it.
    fn winding(&self, q: Point) -> i32 {
        if self.bands.is_empty() {
            return self.whole.winding(q);
        }
        let k = (q.y - self.y0) / self.height;
        if !(k >= 0.0 && k < self.bands.len() as f64 + 1.0) {
            // Above or below every segment's box (or not a number): no segment adds to it.
            return 0;
        }
        self.bands.get((k.floor() as usize).min(self.bands.len() - 1)).map_or(0, |band| band.winding(q))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The banded winding is kurbo's: on a grid of points all over a wavy ring of curves with a
    /// wavy hole (and on every one of its points), and with every segment tested (one band).
    #[test]
    fn the_banded_winding_is_kurbos() {
        let mut path = BezPath::new();
        for (radius, turn) in [(40.0, 1.0), (15.0, -1.0)] {
            let at = |k: f64| {
                let a = turn * std::f64::consts::TAU * k / 48.0;
                let r = radius + 3.0 * (6.0 * a).sin();
                Point::new(50.0 + r * a.cos(), 50.0 + r * a.sin())
            };
            path.move_to(at(0.0));
            for k in 0..48 {
                let k = k as f64;
                path.curve_to(at(k + 0.3) + (0.0, 2.0), at(k + 0.7) - (2.0, 0.0), at(k + 1.0));
            }
            path.close_path();
        }
        let banded = Banded::new(&path);
        assert!(!banded.bands.is_empty(), "banded");
        let one = Banded { bands: Vec::new(), ..Banded::new(&path) };
        assert_eq!(one.whole, path);
        let mut points: Vec<Point> = (0..=120).flat_map(|i| (0..=120).map(move |j| Point::new(i as f64 * 0.85, j as f64 * 0.85))).collect();
        points.extend(path.elements().iter().filter_map(|el| el.end_point()));
        for q in points {
            assert_eq!(banded.winding(q), path.winding(q), "at {q:?}");
            assert_eq!(one.winding(q), path.winding(q), "at {q:?}, every segment");
        }
    }
}
