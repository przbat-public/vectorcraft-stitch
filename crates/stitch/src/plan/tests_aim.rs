//! A shape sewn from and toward the points its object sets ([`Aim`]): an entry point met exactly
//! where the stitches can start anywhere (a fill on its inset outline, a closed line or ring at the
//! point of it nearest), at the nearer end of an open line or column; where the needle only is,
//! nothing changes.

use vectorcraft_geom::{BezPath, Point};

use super::*;
use crate::Command;

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

/// A circle of radius `r` about `c`, as four cubic arcs.
fn circle(c: (f64, f64), r: f64) -> BezPath {
    use vectorcraft_geom::Shape;
    vectorcraft_geom::kurbo::Circle::new(c, r).to_path(0.01)
}

fn first_point(v: &[Stitch]) -> Point {
    v.iter().find(|s| s.command == Command::Stitch).map(|s| s.point()).expect("a needle point")
}

fn entered(path: &BezPath, p: &Params, at: (f64, f64)) -> Vec<Stitch> {
    stitches_aimed(path, vectorcraft_geom::FillRule::NonZero, p, Aim { start: Start::Entry(Point::new(at.0, at.1)), exit: None })
}

/// The farthest a fill's first needle point lies from its entry on the outline: the edge run's
/// inset (half a row spacing), along the outline's normal, and a hair for the corner's mitre.
fn inset_reach(p: &Params) -> f64 {
    p.density * 0.5 * 1.01
}

/// A fill entered anywhere on its outline (the middle of a side, near a corner, on a hole's edge)
/// starts there, on its inset outline, with its edge run and without underlay alike, at any row
/// angle: not at the corner nearest (which the needle's start takes, and the entry took before).
/// Mutation caught: an entry treated as the needle starts at a corner, millimetres off.
#[test]
fn a_fill_starts_at_its_entry_on_its_inset_outline() {
    let mut holed = rect(0.0, 0.0, 60.0, 40.0);
    holed.extend(rect(20.0, 15.0, 20.0, 10.0));
    for underlay in [Underlay::Auto, Underlay::None, Underlay::EdgeTatami] {
        for angle in [0.0, 45.0, 100.0] {
            let p = Params { underlay, angle, ..Params::for_style(Style::Fill) }.sane();
            for at in [(30.0, 0.0), (60.0, 21.0), (2.0, 40.0), (0.0, 5.0), (30.0, 15.0), (40.0, 20.0)] {
                let out = entered(&holed, &p, at);
                let first = first_point(&out);
                let d = first.distance(Point::new(at.0, at.1));
                assert!(d <= inset_reach(&p), "underlay {underlay:?}, {angle}°, entered at {at:?}: starts at {first:?}, {d:.3} mm off");
                assert!(out.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
            }
        }
    }
}

/// Entered without underlay, a fill still sews its rows over the place it is entered: the needle
/// travels from there to the first row under the rows still to come, with no jump.
#[test]
fn a_fill_entered_without_underlay_travels_to_its_first_row() {
    let p = Params { underlay: Underlay::None, ..Params::for_style(Style::Fill) }.sane();
    let bar = rect(0.0, 0.0, 100.0, 20.0);
    for at in [(50.0, 0.0), (100.0, 10.0), (3.0, 20.0)] {
        let out = entered(&bar, &p, at);
        assert_eq!(out.iter().filter(|s| s.command == Command::Jump).count(), 0, "entered at {at:?}: no jump");
        assert!(first_point(&out).distance(Point::new(at.0, at.1)) <= inset_reach(&p), "entered at {at:?}");
    }
}

/// A closed running line, a satin ring and a running line round a hole start at the point of them
/// nearest their entry, between two of their vertices; an open line and an open column can start
/// only at an end, the one nearer the entry.
#[test]
fn a_closed_line_starts_exactly_at_its_entry_and_an_open_one_at_its_nearer_end() {
    let running = Params::for_style(Style::Running).sane();
    // Narrower than 2 mm: its underlay is a centre run, which starts on its middle.
    let satin = Params { width: 1.5, ..Params::for_style(Style::Satin) }.sane();
    let square = rect(0.0, 0.0, 40.0, 40.0);
    // On the square's side, half way between two corners: no vertex there.
    for at in [(20.0, 0.0), (40.0, 13.0)] {
        let first = first_point(&entered(&square, &running, at));
        assert!(first.distance(Point::new(at.0, at.1)) < 1e-6, "running square entered at {at:?}: {first:?}");
    }
    // A circle's flattened points lie a millimetre or two apart: its start lies on it, at the entry.
    let ring = circle((50.0, 50.0), 20.0);
    let at = (50.0 + 20.0 * 0.6, 50.0 + 20.0 * 0.8);
    let first = first_point(&entered(&ring, &running, at));
    assert!(first.distance(Point::new(at.0, at.1)) < 0.06, "running ring: {first:?}");
    // The satin ring's first stitches are its underlay's, along its middle, from there.
    let first = first_point(&entered(&ring, &satin, at));
    assert!(first.distance(Point::new(at.0, at.1)) < 0.06, "satin ring: {first:?}");
    // An open line and column: the nearer end, however far along the entry lies.
    let open = line((0.0, 0.0), (60.0, 0.0));
    for (style, p) in [("running", running), ("satin", satin)] {
        let first = first_point(&entered(&open, &p, (45.0, 3.0)));
        assert!(first.x > 55.0, "{style}: from its right end, {first:?}");
        let first = first_point(&entered(&open, &p, (14.0, -3.0)));
        assert!(first.x < 5.0, "{style}: from its left end, {first:?}");
    }
}

/// Of a line's subpaths, the one its entry lies nearest is sewn first, from there, and the others
/// follow nearest first: two squares, entered on the second.
#[test]
fn a_lines_entry_picks_its_first_subpath() {
    let mut two = rect(0.0, 0.0, 20.0, 20.0);
    two.extend(rect(50.0, 0.0, 20.0, 20.0));
    let out = entered(&two, &Params::for_style(Style::Running).sane(), (60.0, 20.0));
    assert!(first_point(&out).distance(Point::new(60.0, 20.0)) < 1e-6);
}

/// An entry point that isn't a number is no entry: the shape is sewn as from nowhere, whatever its
/// style.
#[test]
fn an_entry_that_isnt_a_number_is_none() {
    let shape = rect(0.0, 0.0, 30.0, 20.0);
    for style in [Style::Running, Style::Fill] {
        let p = Params::for_style(style).sane();
        let rule = vectorcraft_geom::FillRule::NonZero;
        let odd = stitches_aimed(&shape, rule, &p, Aim { start: Start::Entry(Point::new(f64::NAN, 3.0)), exit: None });
        assert_eq!(odd, stitches_aimed(&shape, rule, &p, Aim::from_needle(None)), "{style:?}");
    }
}
