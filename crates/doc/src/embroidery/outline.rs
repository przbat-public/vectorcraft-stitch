//! An object's outline as the embroidery code measures points against it: where a point snaps to
//! (`embroidery.setEntryExit {snap}`), which of an object's pieces lies nearest a point (objects
//! made from others), and what the embroidery tool outlines on the canvas. A path's own outline, or
//! a compound path's members'; nothing for anything else.

use vectorcraft_geom::{BezPath, PathEl, Point};

use crate::{Node, NodeKind};

/// How finely an outline is flattened to measure how far a point lies from it, document points: a
/// hundredth of a point, far below a needle's width.
pub const OUTLINE_FLATTEN: f64 = 0.01;

/// `n`'s outline (a path's, or a compound path's members'), document points; empty for an object
/// that is neither.
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
    path
}

/// `n`'s outline flattened ([`OUTLINE_FLATTEN`]), document points: each subpath's points, with
/// whether it is closed (its first point isn't repeated at its end).
pub fn subpaths(n: &Node) -> Vec<(Vec<Point>, bool)> {
    let mut out: Vec<(Vec<Point>, bool)> = Vec::new();
    vectorcraft_geom::kurbo::flatten(outline(n).iter(), OUTLINE_FLATTEN, |el| match el {
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
