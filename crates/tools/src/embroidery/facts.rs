//! What only the engine knows about the stitch plan and the settings, handed to the embroidery tool
//! through [`crate::ToolContext::embroidery`]: the tools crate can't reach the plan or the settings'
//! layers (the engine's), and no view plans stitches of its own, so the engine answers from the
//! plan the canvas shows, and from the document, and never plans to answer. A new question is a
//! new method with a default body.

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

/// The angle of a fill's tatami rows: degrees counter-clockwise on the page (as the commands take
/// it), and whether the object sets it (else Auto: the export's or the default rows, turned with
/// the object).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillAngle {
    pub degrees: f64,
    pub set: bool,
}

/// What the embroidery tool asks of the plan the canvas shows.
pub trait PlanFacts {
    /// The plan's generation: what the answers are of.
    fn generation(&self) -> u64;
    /// Where part `part` of object `object` (the object a person selects) is sewn; `None` when the
    /// plan sews it nowhere.
    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds>;
    /// The object a person selects whose stitches lie under `at` (document points) within `tol`,
    /// the one sewn on top, and the part they are of (`None`: stitches read from a machine file);
    /// `None` when no stitch lies there, or nothing answers.
    fn part_at(&self, _at: Point, _tol: f64) -> Option<(NodeId, Option<PartKind>)> {
        None
    }
    /// Where a point given at `at` (document points) for part `part` of object `object` is stored:
    /// taken to where its stitches can start or end, as `embroidery.setEntryExit {snap: true}`
    /// takes it; `None` when nothing answers.
    fn snap(&self, _object: NodeId, _part: PartKind, _at: Point) -> Option<Point> {
        None
    }
    /// The angle of object `object`'s fill rows, when its fill is sewn as tatami, from the document
    /// as it is (no plan needed); `None` for a fill sewn otherwise or not at all.
    fn fill_angle(&self, _object: NodeId) -> Option<FillAngle> {
        None
    }
    /// The needle points stitch editing works on: while the embroidery tool works inside isolation
    /// of an object holding paths of stitches (the project owner's choice, 2026-10-27: stitch
    /// editing is the tool in isolation of a stitch object); `None` otherwise.
    fn needle_points(&self) -> Option<&dyn NeedlePoints> {
        None
    }
}

/// A needle point of a path of stitches (opened from a machine file, or converted to manual
/// stitches), as the stitch commands address it: the path and the needle point's place in it,
/// from 0 across its runs (`{id, index}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NeedleRef {
    pub id: NodeId,
    pub index: usize,
}

/// A needle point found under the pointer, and where it is (document points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NeedleHit {
    pub needle: NeedleRef,
    pub point: Point,
}

/// A place on a thread found under the pointer: the stitch from needle point `from` to the next one
/// in its run, its two ends, and the point on it nearest the pointer (document points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreadHit {
    pub from: NeedleRef,
    pub ends: [Point; 2],
    pub point: Point,
}

/// What stitch editing edits: the object isolated (a stitch object: a converted object, an opened
/// design's thread block, a run), the paths of stitches in it whose needle points can be edited and
/// how many needle points they hold, the box those fall in (document points), and the paths of
/// stitches in it that are shown but can't be edited (locked or hidden, cut by a clip group, with
/// anchors that aren't numbers, or more needle points than a design may have).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NeedleScope {
    pub object: NodeId,
    pub paths: usize,
    pub needles: usize,
    pub bounds: Option<vectorcraft_geom::Rect>,
    pub refused: usize,
}

/// The needle points stitch editing works on, answered by the engine from the document as it is
/// (so a press right after an edit never lands where a needle point was), with record numbers
/// from the plan the canvas shows where it is of the path as it is. Bounded to the paths being
/// edited; positions are document points.
pub trait NeedlePoints {
    /// What is being edited.
    fn scope(&self) -> NeedleScope;
    /// Whether path `id` is one being edited.
    fn holds(&self, id: NodeId) -> bool;
    /// The needle point nearest `at` within `tol`; of equally near ones, the one sewn later (drawn
    /// on top).
    fn needle_at(&self, at: Point, tol: f64) -> Option<NeedleHit>;
    /// The stitch nearest `at` within `tol`, and the place on it nearest `at`.
    fn thread_at(&self, at: Point, tol: f64) -> Option<ThreadHit>;
    /// Where needle point `n` is.
    fn point(&self, n: NeedleRef) -> Option<Point>;
    /// The needle point after `n` along the thread (`forward`), or before it: within its path,
    /// across its runs, then on to the next path or back to the one before, round from the last to
    /// the first.
    fn step(&self, n: NeedleRef, forward: bool) -> Option<NeedleRef>;
    /// The first needle point of the first path being edited.
    fn first(&self) -> Option<NeedleRef>;
    /// The first and the last needle point (their places in its path) of the run `n` is in.
    fn run(&self, n: NeedleRef) -> Option<(usize, usize)>;
    /// The anchors needle point `n` is in its path, `(subpath, anchor)`: one, or both of a run of
    /// one needle point (a line to its own start).
    fn anchors(&self, n: NeedleRef) -> Vec<(usize, usize)>;
    /// The needle point anchor `anchor` of path `id` is.
    fn of_anchor(&self, id: NodeId, anchor: (usize, usize)) -> Option<usize>;
    /// How many needle points path `id` holds (0: not one being edited).
    fn count(&self, id: NodeId) -> usize;
    /// Path `id`'s place among the paths being edited, in the order they are sewn.
    fn order(&self, id: NodeId) -> Option<usize>;
    /// The record number the plan the canvas shows gives needle point `n`, when that plan is of
    /// its path as it is now; `None` while the plan lags behind an edit of it.
    fn number(&self, n: NeedleRef) -> Option<usize>;
}
