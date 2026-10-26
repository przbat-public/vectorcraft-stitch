//! What only the engine knows about the stitch plan, handed to the embroidery tool through
//! [`crate::ToolContext::embroidery`]: the tools crate can't reach the plan (the engine's), and no
//! view plans stitches of its own, so the engine answers from the plan the canvas shows, and never
//! plans to answer. A new question is a new method with a default body.

use vectorcraft_doc::NodeId;
use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_geom::Point;

/// Where the plan sews a part: the first needle point of its first unit and the last of its last,
/// document points, and whether the plan is of the object as it is now (else what it shows is
/// drawn stale until the next plan); where the needle comes from before it and goes to after it,
/// along the thread (none after a colour change or a cut, or before the design's first part); and
/// the design's trim length (document points: a jump longer is cut first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlannedEnds {
    pub first: Point,
    pub last: Point,
    pub current: bool,
    pub before: Option<Point>,
    pub after: Option<Point>,
    pub trim: Option<f64>,
}

/// What the embroidery tool asks of the plan the canvas shows.
pub trait PlanFacts {
    /// The plan's generation: what the answers are of.
    fn generation(&self) -> u64;
    /// Where part `part` of object `object` (the object a person selects) is sewn; `None` when the
    /// plan sews it nowhere.
    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds>;
    /// The object a person selects whose stitches lie under `at` (document points) within `tol`,
    /// the one sewn on top; `None` when no stitch lies there, or nothing answers.
    fn object_at(&self, _at: Point, _tol: f64) -> Option<NodeId> {
        None
    }
}
