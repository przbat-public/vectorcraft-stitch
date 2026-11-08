//! A filled shape's long axis and the rows across it: a bar's axis at every turn, rows square to
//! it, round shapes left to the fixed angle, holes and either fill rule, shapes with no area or out
//! of range, and an axis that moves with its shape and turns with it; a shape's separate pieces,
//! each sewn across its own length, and the outlines that stay one piece.

use vectorcraft_geom::{Affine, BezPath, FillRule, Point, Vec2};

use super::axis::MAX_DIRECTIONS;
use super::*;
use crate::Command;

/// How near a measured axis lies to the true one (degrees): the bands' rounding across the shape.
const AXIS_TOLERANCE: f64 = 0.01;

/// A rectangle `w` × `h` (millimetres) centred on `c`, turned `turn` degrees clockwise as seen on
/// the page, wound clockwise or not.
fn rectangle(c: Point, w: f64, h: f64, turn: f64, clockwise: bool) -> BezPath {
    let corners = [(-w / 2.0, -h / 2.0), (w / 2.0, -h / 2.0), (w / 2.0, h / 2.0), (-w / 2.0, h / 2.0)];
    let xf = Affine::translate(c.to_vec2()) * Affine::rotate(turn.to_radians());
    let mut pts: Vec<Point> = corners.iter().map(|&(x, y)| xf * Point::new(x, y)).collect();
    if !clockwise {
        pts.reverse();
    }
    let mut p = BezPath::new();
    p.move_to(pts[0]);
    for &q in &pts[1..] {
        p.line_to(q);
    }
    p.close_path();
    p
}

/// The difference between two directions of lines (degrees), whichever way round each is said.
fn apart(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(180.0);
    d.min(180.0 - d)
}

/// A 10 × 30 mm bar turned every 15°: its long axis is its length's direction, 3 times as long as
/// wide, and its rows run square to it.
#[test]
fn a_bars_axis_is_its_length_at_every_turn() {
    for k in 0..12 {
        let turn = 15.0 * f64::from(k);
        let bar = rectangle(Point::new(50.0, 40.0), 30.0, 10.0, turn, true);
        let axis = shape_axis(&bar, FillRule::NonZero).expect("a bar has an axis");
        assert!(apart(axis.angle, turn) < AXIS_TOLERANCE, "{turn}°: the axis at {}", axis.angle);
        assert!((axis.elongation - 3.0).abs() < 1e-3, "{turn}°: {} times as long as wide", axis.elongation);
        let (rows, _) = rows_across(&bar, FillRule::NonZero).expect("a bar's rows follow it");
        assert!(apart(rows, turn + 90.0) <= SHAPE_ANGLE_STEP / 2.0 + 1e-9, "{turn}°: rows at {rows}");
        assert!((rows / SHAPE_ANGLE_STEP - (rows / SHAPE_ANGLE_STEP).round()).abs() < 1e-6, "{rows} isn't on the 0.1° step");
    }
}

/// A square, a disc and a ring are as wide every way: their rows keep the fixed angle; a 3 × 2
/// rectangle is just past the line.
#[test]
fn round_shapes_keep_the_fixed_angle() {
    let square = rectangle(Point::new(0.0, 0.0), 20.0, 20.0, 30.0, true);
    let disc: BezPath = vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Circle::new((5.0, 5.0), 12.0), 0.01).collect();
    let mut ring = disc.clone();
    let inner: BezPath = vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Circle::new((5.0, 5.0), 8.0), 0.01).collect();
    ring.extend(inner);
    for (name, shape) in [("square", &square), ("disc", &disc), ("ring", &ring)] {
        assert!(rows_across(shape, FillRule::EvenOdd).is_none(), "the {name}'s rows follow it");
    }
    let wide = rectangle(Point::new(0.0, 0.0), 3.0, 2.0, 0.0, true);
    let axis = shape_axis(&wide, FillRule::NonZero).expect("an axis");
    assert!((axis.elongation - 1.5).abs() < 1e-3, "{}", axis.elongation);
}

/// A hole is left out of the area as the fill rule leaves it: a square frame with a bar-shaped
/// hole is long across the hole, by either rule when the hole is wound against the frame; the
/// even-odd rule leaves a hole out whichever way it is wound.
#[test]
fn holes_are_measured_as_the_rows_leave_them() {
    let mut frame = rectangle(Point::new(0.0, 0.0), 40.0, 40.0, 0.0, true);
    frame.extend(rectangle(Point::new(0.0, 0.0), 36.0, 10.0, 0.0, false));
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let axis = shape_axis(&frame, rule).expect("an axis");
        // What is left is two bands above and below the hole: longer up and down than across.
        assert!(apart(axis.angle, 90.0) < AXIS_TOLERANCE, "{rule:?}: {}", axis.angle);
    }
    let mut same_way = rectangle(Point::new(0.0, 0.0), 40.0, 40.0, 0.0, true);
    same_way.extend(rectangle(Point::new(0.0, 0.0), 36.0, 10.0, 0.0, true));
    let axis = shape_axis(&same_way, FillRule::EvenOdd).expect("an axis");
    assert!(apart(axis.angle, 90.0) < AXIS_TOLERANCE, "{}", axis.angle);
    // Wound the same way under non-zero, the hole is filled: the square has no long axis.
    assert!(rows_across(&same_way, FillRule::NonZero).is_none());
}

/// A shape with no area, an empty one, and one out of range have no axis, and nothing panics.
#[test]
fn shapes_without_area_have_no_axis() {
    let mut line = BezPath::new();
    line.move_to((0.0, 0.0));
    line.line_to((30.0, 0.0));
    let far = rectangle(Point::new(1e9, 0.0), 30.0, 10.0, 0.0, true);
    let mut nan = BezPath::new();
    nan.move_to((f64::NAN, 0.0));
    nan.line_to((1.0, 1.0));
    nan.line_to((0.0, 1.0));
    nan.close_path();
    for (name, shape) in [("line", line), ("empty", BezPath::new()), ("far", far), ("not a number", nan)] {
        assert!(shape_axis(&shape, FillRule::NonZero).is_none(), "the {name} has an axis");
        assert!(rows_across(&shape, FillRule::NonZero).is_none(), "the {name} has rows");
    }
}

/// Random rectangles and ellipses (fixed seed): the axis doesn't move when the shape moves, and
/// turns with it, within the tolerance, wherever the shape is longer than [`SHAPE_AXIS_MIN_ELONGATION`].
#[test]
fn the_axis_moves_and_turns_with_its_shape() {
    let mut seed: u64 = 0x5eed_a815;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for case in 0..100 {
        let (w, h) = (5.0 + next() * 40.0, 2.0 + next() * 10.0);
        if w / h < SHAPE_AXIS_MIN_ELONGATION * 1.1 {
            continue;
        }
        let turn = next() * 360.0;
        let shape: BezPath = if case % 2 == 0 {
            rectangle(Point::new(0.0, 0.0), w, h, 0.0, true)
        } else {
            vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Ellipse::new((0.0, 0.0), (w / 2.0, h / 2.0), 0.0), 0.01).collect()
        };
        let base = shape_axis(&shape, FillRule::NonZero).expect("an axis");
        let moved = Affine::translate(Vec2::new(next() * 200.0 - 100.0, next() * 200.0 - 100.0)) * Affine::rotate(turn.to_radians());
        let there = shape_axis(&(moved * shape.clone()), FillRule::NonZero).expect("an axis");
        assert!(apart(there.angle, base.angle + turn) < AXIS_TOLERANCE * 10.0, "case {case}: {} turned {turn} is {}", base.angle, there.angle);
        assert!((there.elongation - base.elongation).abs() < 1e-2 * base.elongation, "case {case}");
    }
}

/// The direction (degrees as the planner counts them, from 0 up to a half turn) most of `out`'s
/// needle-down stitches longer than a millimetre with both ends where `inside` holds run in: the
/// median of their directions.
fn sewn_direction(out: &[Stitch], inside: impl Fn(Point) -> bool) -> f64 {
    let mut angles: Vec<f64> = out
        .windows(2)
        .filter_map(|w| {
            let [a, z] = w else { return None };
            let (pa, pz) = (Point::new(a.x, a.y), Point::new(z.x, z.y));
            let sewn = a.command == Command::Stitch && z.command == Command::Stitch && inside(pa) && inside(pz) && pa.distance(pz) > 1.0;
            sewn.then(|| (pz.y - pa.y).atan2(pz.x - pa.x).to_degrees().rem_euclid(180.0))
        })
        .collect();
    assert!(!angles.is_empty(), "no stitch there");
    angles.sort_by(f64::total_cmp);
    angles[angles.len() / 2]
}

/// The fill parameters without underlay (its stitches are its rows), with the rows across the
/// shape, `fixed` the angle a round piece keeps.
fn across_shape(fixed: f64) -> Params {
    Params { underlay: Underlay::None, rows: Rows::AcrossShape, angle: fixed, ..Params::for_style(Style::Fill) }
}

/// A tall bar and a wide bar apart in one path: each is measured alone and sewn across its own
/// length, the tall one's rows at 0°, the wide one's at 90°, and each is covered; two tall bars
/// have one direction and are sewn whole at it, exactly as fixed rows at that angle.
#[test]
fn separate_pieces_are_sewn_across_each_one() {
    let mut path = rectangle(Point::new(5.0, 20.0), 10.0, 40.0, 0.0, true);
    path.extend(rectangle(Point::new(50.0, 5.0), 40.0, 10.0, 0.0, true));
    let pieces = rows_by_piece(&path, FillRule::NonZero, 45.0);
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert!(pieces.iter().all(|p| p.across) && apart(pieces[0].angle, 0.0) < 1e-9 && apart(pieces[1].angle, 90.0) < 1e-9, "{pieces:?}");
    let out = stitches(&path, FillRule::NonZero, &across_shape(45.0));
    assert!(apart(sewn_direction(&out, |q| q.x < 10.5), 0.0) < 1.0, "the tall bar's rows");
    assert!(apart(sewn_direction(&out, |q| q.x > 29.5), 90.0) < 1.0, "the wide bar's rows");
    let mut two = rectangle(Point::new(5.0, 20.0), 10.0, 40.0, 0.0, true);
    two.extend(rectangle(Point::new(55.0, 20.0), 10.0, 40.0, 0.0, true));
    let fixed = Params { rows: Rows::Fixed, angle: 0.0, ..across_shape(45.0) };
    assert_eq!(stitches(&two, FillRule::NonZero, &across_shape(45.0)), stitches(&two, FillRule::NonZero, &fixed));
}

/// Outlines that touch, cross or lie one inside the other are one piece: two squares sharing an
/// edge are one rectangle twice as long as wide (rows across it), a plus of two crossing bars is as
/// wide every way (the fixed rows), and a frame keeps its hole, and an island in the hole, by
/// either fill rule.
#[test]
fn touching_crossing_and_nested_outlines_are_one_piece() {
    let mut side_by_side = rectangle(Point::new(5.0, 5.0), 10.0, 10.0, 0.0, true);
    side_by_side.extend(rectangle(Point::new(15.0, 5.0), 10.0, 10.0, 0.0, true));
    let pieces = rows_by_piece(&side_by_side, FillRule::NonZero, 45.0);
    assert!(pieces.len() == 1 && pieces[0].across && apart(pieces[0].angle, 90.0) < 1e-9, "{pieces:?}");
    let mut plus = rectangle(Point::new(0.0, 0.0), 30.0, 6.0, 0.0, true);
    plus.extend(rectangle(Point::new(0.0, 0.0), 6.0, 30.0, 0.0, true));
    let pieces = rows_by_piece(&plus, FillRule::NonZero, 45.0);
    assert!(pieces.len() == 1 && !pieces[0].across && pieces[0].angle == 45.0, "{pieces:?}");
    let mut frame = rectangle(Point::new(0.0, 0.0), 40.0, 40.0, 0.0, true);
    frame.extend(rectangle(Point::new(0.0, 0.0), 36.0, 10.0, 0.0, false));
    frame.extend(rectangle(Point::new(0.0, 0.0), 20.0, 4.0, 0.0, true));
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        assert_eq!(rows_by_piece(&frame, rule, 45.0).len(), 1, "{rule:?}");
    }
}

/// Past [`MAX_DIRECTIONS`] directions a shape's pieces are measured together: small bars each
/// turned 2.5° more than the one before are as many pieces as bars up to it, one piece past it.
#[test]
fn past_the_most_directions_the_pieces_are_measured_together() {
    let bars = |n: usize| {
        let mut p = BezPath::new();
        for k in 0..n {
            p.extend(rectangle(Point::new(10.0 * k as f64, 0.0), 6.0, 1.0, 2.5 * k as f64, true));
        }
        p
    };
    assert_eq!(rows_by_piece(&bars(MAX_DIRECTIONS), FillRule::NonZero, 45.0).len(), MAX_DIRECTIONS);
    assert_eq!(rows_by_piece(&bars(MAX_DIRECTIONS + 1), FillRule::NonZero, 45.0).len(), 1);
}
