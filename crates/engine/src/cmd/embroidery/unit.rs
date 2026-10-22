//! One sewing unit's stitches from what it is planned from ([`UnitRequest`]): the planning step
//! the document walk (`digitize`) takes for each fill and stroke, with nothing of the document or
//! of the units around it but where the needle comes from.
//!
//! The step is pure: the same request always plans the same stitches. That is what lets a unit's
//! stitches be kept between plans and used again when nothing it is planned from has changed.

use vectorcraft_doc::cancel;
use vectorcraft_geom::{BezPath, FillRule, PathData, Point, Shape};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, outline_stroke};
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
    /// The error command `cmd` reports: its parameters' error for a design too large, a cancel as
    /// a cancel.
    pub(crate) fn error(self, cmd: &str) -> crate::EngineError {
        match self {
            Self::TooLarge(why) => crate::cmd::bad(cmd, why),
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
    let widths = found.iter().map(plan::Column::width).fold(None, |range: Option<[f64; 2]>, w| match range {
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
    let mut cut: Option<PathData> = None;
    for c in columns {
        cancel::check().map_err(|_| PlanError::Cancelled)?;
        let outline = PathData::from_bezpath(&c.outline());
        let band = outline_stroke(&outline, 2.0 * COLUMN_GROWTH, Cap::Round, Join::Round, ROUND_JOINS);
        let grown = boolean(&outline, FillRule::NonZero, &band, FillRule::NonZero, BoolOp::Union);
        cut = Some(match cut {
            Some(all) => boolean(&all, FillRule::NonZero, &grown, FillRule::NonZero, BoolOp::Union),
            None => grown,
        });
    }
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
    let inside = |s: &Stitch| {
        let w = path.winding(s.point());
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
