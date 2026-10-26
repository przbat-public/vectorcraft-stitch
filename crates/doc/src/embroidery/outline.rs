//! An object's outline as the embroidery code measures points against it: where a point snaps to
//! (`embroidery.setEntryExit {snap}`), which of an object's pieces lies nearest a point (objects
//! made from others), and what the embroidery tool outlines on the canvas. A path's own outline, or
//! a compound path's members'; nothing for anything else, or for a path a file holds out of range,
//! which would flatten to a number of points without bound.

use vectorcraft_geom::kurbo::Shape;
use vectorcraft_geom::{BezPath, PathEl, Point};

use crate::{MAX_COORD, Node, NodeKind};

/// How finely an outline is flattened to measure how far a point lies from it, document points: a
/// hundredth of a point, far below a needle's width.
pub const OUTLINE_FLATTEN: f64 = 0.01;

/// The most steps an outline's size is flattened in: an outline up to 1,000 pt across (35 cm) is
/// flattened at [`OUTLINE_FLATTEN`], a larger one more coarsely, so that no outline flattens to
/// more than a few hundred points a segment however large it is (an estimate: art is sewn in hoops
/// of at most about 40 cm).
pub const OUTLINE_SPAN_STEPS: f64 = 1e5;

/// `n`'s outline (a path's, or a compound path's members'), document points; empty for an object
/// that is neither, or one with a point that isn't a number within [`MAX_COORD`] (a hostile
/// file's: the commands keep art within it).
pub fn outline(n: &Node) -> BezPath {
    let mut path = BezPath::new();
    match &n.kind {
        NodeKind::Path { path: p, .. } => path.extend(p.to_bezpath()),
        NodeKind::Compound { children, .. } => {
            for c in children {
                if let Some(p) = c.path_data() {
                    path.extend(p.to_bezpath());
                }
            }
        }
        _ => {}
    }
    if path.elements().iter().all(in_range) { path } else { BezPath::new() }
}

/// Whether each point of `el` is a number within [`MAX_COORD`].
fn in_range(el: &PathEl) -> bool {
    let ok = |p: &Point| p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD;
    match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => ok(p),
        PathEl::QuadTo(a, b) => ok(a) && ok(b),
        PathEl::CurveTo(a, b, c) => ok(a) && ok(b) && ok(c),
        PathEl::ClosePath => true,
    }
}

/// `n`'s outline flattened ([`OUTLINE_FLATTEN`], or a step of its size over
/// [`OUTLINE_SPAN_STEPS`] where that is coarser), document points: each subpath's points, with
/// whether it is closed (its first point isn't repeated at its end).
pub fn subpaths(n: &Node) -> Vec<(Vec<Point>, bool)> {
    let path = outline(n);
    let bounds = path.bounding_box();
    let size = bounds.width().max(bounds.height());
    let tolerance = if size.is_finite() { OUTLINE_FLATTEN.max(size / OUTLINE_SPAN_STEPS) } else { OUTLINE_FLATTEN };
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    vectorcraft_geom::kurbo::flatten(path.iter(), tolerance, |el| match el {
        PathEl::MoveTo(p) => out.push((vec![p], false)),
        PathEl::LineTo(p) => {
            if let Some((line, _)) = out.last_mut() {
                line.push(p);
            }
        }
        PathEl::ClosePath => {
            if let Some((_, closed)) = out.last_mut() {
                *closed = true;
            }
        }
        _ => {}
    });
    out
}

/// The point of polyline `line` nearest `q`, `None` for none.
pub fn nearest_on(line: &[Point], q: Point) -> Option<Point> {
    match line {
        [] => None,
        [only] => Some(*only),
        _ => line
            .windows(2)
            .filter_map(|w| {
                let [a, b] = w else { return None };
                let ab = *b - *a;
                let len2 = ab.hypot2();
                let t = if len2 > 0.0 { ((q - *a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let at = *a + ab * t;
                Some((at.distance(q), at))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, at)| at),
    }
}

/// How far `q` lies from polyline `line` (infinitely far from none).
pub fn distance_to(line: &[Point], q: Point) -> f64 {
    nearest_on(line, q).map_or(f64::INFINITY, |p| p.distance(q))
}
