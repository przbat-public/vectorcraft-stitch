//! A fill's turn from one row into the next where the edge lies square to the rows: the turn is the
//! rows' spacing, under the shortest stitch, and each row still starts at the edge (it starts a
//! little further in, [`super::fill`]'s `lengthen_turn`), so no thin triangle of fabric is left
//! beside the edge; a notch drawn here, one side square to the rows, is covered.

use vectorcraft_geom::{BezPath, FillRule, Point, Shape};

use super::geometry::Axes;
use super::*;
use crate::Command;

/// A closed polygon through `corners` (millimetres).
fn polygon(corners: &[(f64, f64)]) -> BezPath {
    let mut p = BezPath::new();
    for (k, &q) in corners.iter().enumerate() {
        if k == 0 {
            p.move_to(q);
        } else {
            p.line_to(q);
        }
    }
    p.close_path();
    p
}

/// A fill with no underlay, its rows at `angle` degrees: only its top rows are sewn.
fn rows_at(angle: f64) -> Params {
    Params { angle, underlay: Underlay::None, ..Params::for_style(Style::Fill) }
}

/// The rows of `out` as they are sewn (`axes` the fill's): each run of needle points on one line
/// across the rows, in sewing order.
fn sewn_rows(out: &[Stitch], axes: Axes) -> Vec<Vec<Point>> {
    let mut rows: Vec<Vec<Point>> = Vec::new();
    for q in out.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()) {
        match rows.last_mut() {
            Some(row) if row.last().is_some_and(|l| (axes.across(*l) - axes.across(q)).abs() < 1e-6) => row.push(q),
            _ => rows.push(vec![q]),
        }
    }
    rows
}

/// How far in a row starts where its turn from the row before is the rows' spacing: until the turn
/// is the shortest stitch, and a hair more (millimetres).
fn lengthening(p: &Params) -> f64 {
    let length = p.min_stitch + crate::SAME_POINT;
    (length * length - p.density * p.density).max(0.0).sqrt()
}

/// A rectangle whose rows run left to right turns at its left and right edges, square to the rows,
/// with the rows' spacing (0.38 mm, under the shortest stitch, 0.4): every row still starts at the
/// edge it turns at, past it by its pull compensation less how far in it starts so that the turn is
/// the shortest stitch (0.125 mm); where a row's first needle point was left out with the short
/// turn, the row started at its second, 0.4 mm or more inside the edge.
#[test]
fn each_row_starts_at_the_edge_it_turns_at() {
    let p = rows_at(0.0);
    assert!(p.density < p.min_stitch, "the turns are under the shortest stitch");
    let (width, height) = (20.0, 10.0);
    let out = stitches(&polygon(&[(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)]), FillRule::NonZero, &p);
    let reach = p.pull[0].max(p.pull[1]);
    let rows = sewn_rows(&out, Axes::new(p.angle));
    assert!(rows.len() > 20, "{} rows", rows.len());
    // Every turn into a neighbouring row is past the shortest stitch, by more than rounding: never
    // just under it, where a check with no tolerance would count it as short.
    let axes = Axes::new(p.angle);
    for pair in rows.windows(2) {
        if let (Some(&end), Some(&start)) = (pair[0].last(), pair[1].first())
            && ((axes.across(start) - axes.across(end)).abs() - p.density).abs() < 1e-6
        {
            let turn = end.distance(start);
            assert!(turn > p.min_stitch + crate::SAME_POINT / 2.0, "a turn of {turn:.12} mm at {start:?}");
        }
    }
    let mut started_in = 0;
    for row in &rows {
        let (Some(&first), Some(&second)) = (row.first(), row.get(1)) else { continue };
        // The edge it starts at, past it by the pull compensation: the left one going right.
        let edge = if second.x > first.x { -reach } else { width + reach };
        let inside = (first.x - edge).abs();
        assert!(inside <= lengthening(&p) + 1e-9, "a row starts {inside:.3} mm inside its edge, at {first:?}");
        started_in += usize::from(inside > 1e-9);
    }
    assert!(started_in > 0, "no turn was short");
}

/// The bare fabric beside a notch drawn here: a square with a notch cut into its left side, the
/// notch's upper side along the rows (45°) and its lower side square to them, where the rows turn
/// with their spacing. Of the square's points at least [`MARGIN`] inside its outline and within
/// [`NEAR`] of the notch's lower side, those under no stitch drawn a 40 wt thread wide cover at most
/// [`NOTCH_FABRIC_MM2`]; with each such turn's next row starting at its second needle point they
/// covered 2.37 mm² (2026-11-09).
#[test]
fn a_notch_square_to_the_rows_is_covered() {
    let p = rows_at(45.0);
    let shape = polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 16.0), (6.0, 10.0), (0.0, 4.0)]);
    let out = stitches(&shape, FillRule::NonZero, &p);
    let segments: Vec<(Point, Point)> =
        out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch).map(|w| (w[0].point(), w[1].point())).collect();
    let (side_a, side_b) = (Point::new(6.0, 10.0), Point::new(0.0, 16.0));
    let mut outline: Vec<(Point, Point)> = Vec::new();
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 16.0), (6.0, 10.0), (0.0, 4.0)];
    for k in 0..corners.len() {
        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
        outline.push((Point::new(a.0, a.1), Point::new(b.0, b.1)));
    }
    let to = |q: Point, (a, b): (Point, Point)| {
        let ab = b - a;
        let t = ((q - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0);
        q.distance(a + ab * t)
    };
    let mut bare = 0usize;
    let steps = (8.0 / PIXEL) as usize;
    for i in 0..steps {
        for j in 0..steps {
            let q = Point::new((i as f64 + 0.5) * PIXEL, 9.0 + (j as f64 + 0.5) * PIXEL);
            if shape.winding(q) == 0 || outline.iter().any(|&s| to(q, s) < MARGIN) || to(q, (side_a, side_b)) > NEAR {
                continue;
            }
            if !segments.iter().any(|&s| to(q, s) <= HALF_THREAD) {
                bare += 1;
            }
        }
    }
    let mm2 = bare as f64 * PIXEL * PIXEL;
    assert!(mm2 <= NOTCH_FABRIC_MM2, "{mm2:.3} mm² of fabric beside the notch");
}

/// The most bare fabric beside the notch (square millimetres): 0.115 measured on 2026-11-09, where a
/// row's short last stitch is left out on purpose and the needle turns into the next row along a
/// slant (`SHORT_LAST` in [`super::fill`]), with room for rounding.
const NOTCH_FABRIC_MM2: f64 = 0.2;

/// The grid the bare fabric is measured on (millimetres): a twentieth of a millimetre.
const PIXEL: f64 = 0.05;
/// Half the width a 40 wt thread covers (millimetres): the 0.4 mm at which such threads touch.
const HALF_THREAD: f64 = 0.2;
/// How far inside the outline the bare fabric is looked for, and how near the notch's square side
/// (millimetres): past the outline's own pull compensation, and as far in as a row's first stitch
/// reaches.
const MARGIN: f64 = 0.3;
const NEAR: f64 = 4.5;

/// The turn itself: a row whose turn from the row before is the rows' spacing starts further in
/// until the turn is the shortest stitch and a hair more; a row whose first stitch has no room for
/// that, or whose turn is already long enough, is left as it is.
#[test]
fn a_short_turn_is_lengthened_where_the_row_has_room() {
    use super::fill::lengthen_turn;
    let p = rows_at(0.0);
    let from = Point::new(0.0, 0.0);
    // The next row, a spacing on, sewn rightwards from the edge: its first stitch 3 mm long.
    let mut row = [Point::new(0.0, p.density), Point::new(3.0, p.density), Point::new(7.0, p.density)];
    lengthen_turn(&mut row, from, p.min_stitch);
    assert!((row[0].x - lengthening(&p)).abs() < 1e-12 && row[0].y == p.density, "{:?}", row[0]);
    assert!(row[0].distance(from) >= p.min_stitch, "a turn of {}", row[0].distance(from));
    assert_eq!(row[1..], [Point::new(3.0, p.density), Point::new(7.0, p.density)]);
    // No room: a first stitch only a hair longer than the shortest one.
    let tight = [Point::new(0.0, p.density), Point::new(p.min_stitch + 0.05, p.density)];
    let mut row = tight;
    lengthen_turn(&mut row, from, p.min_stitch);
    assert_eq!(row, tight, "a first stitch left shorter than the shortest");
    // Long enough already: turns along a slanted edge, the next row starting further on, or
    // further back (where starting further in would shorten the turn).
    for start in [0.5, -0.5] {
        let slanted = [Point::new(start, p.density), Point::new(4.0, p.density)];
        let mut row = slanted;
        lengthen_turn(&mut row, from, p.min_stitch);
        assert_eq!(row, slanted, "a long turn moved");
    }
}
