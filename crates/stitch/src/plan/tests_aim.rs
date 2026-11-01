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

/// Entered where no way inside leads on to its first row (on the edge of a second outline the
/// non-zero rule fills too, which travels don't cross), a fill with no underlay still goes in at its
/// entry, and sews a stitch there to tie on to before it jumps: two needle points before the cut,
/// not one alone, at any shortest stitch. The stitch is at least the shortest stitch long, so the
/// joining sews it and ties the thread off on it before the cut: 1 mm long, at a shortest stitch
/// over 1 mm it was left out, and the thread was cut after one needle point with no tie.
#[test]
fn a_lead_that_must_jump_is_tied_first() {
    let mut overlapping = rect(0.0, 0.0, 60.0, 40.0);
    overlapping.extend(rect(20.0, 15.0, 20.0, 10.0));
    for min_stitch in [Params::for_style(Style::Fill).min_stitch, 1.5, 2.5] {
        let p = Params { underlay: Underlay::None, angle: 0.0, min_stitch, ..Params::for_style(Style::Fill) }.sane();
        let out = entered(&overlapping, &p, (30.0, 15.0));
        let before: Vec<Point> = out.iter().take_while(|s| s.command == Command::Stitch).map(|s| s.point()).collect();
        // Tied off: the last needle point sewn again after a stitch back at least the shortest
        // stitch long.
        let [.., back, step, last] = before.as_slice() else { panic!("shortest {min_stitch} mm: {} needle points before the cut", before.len()) };
        assert!(back.distance(*last) <= crate::SAME_POINT, "shortest {min_stitch} mm: not tied off before the cut: {before:?}");
        assert!(step.distance(*last) + crate::SAME_POINT >= min_stitch, "shortest {min_stitch} mm: a tie stitch {} mm long", step.distance(*last));
        assert!(first_point(&out).distance(Point::new(30.0, 15.0)) <= inset_reach(&p), "shortest {min_stitch} mm: goes in at its entry");
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

fn last_point(v: &[Stitch]) -> Point {
    v.iter().rev().find(|s| s.command == Command::Stitch).map(|s| s.point()).expect("a needle point")
}

fn aimed(path: &BezPath, p: &Params, start: Start, exit: Option<(f64, f64)>) -> Vec<Stitch> {
    stitches_aimed(path, vectorcraft_geom::FillRule::NonZero, p, Aim { start, exit: exit.map(|(x, y)| Point::new(x, y)) })
}

/// An open running line whose start is free ends at its end nearest its exit, even when the needle
/// comes from the other side (a set exit beats where the needle happens to be), as long as turning
/// it adds no lift or cut on the move in from the needle (the guard counts that move); entered at a
/// set point it ends where its stitches take it (a set entry beats a set exit: a running line is
/// never sewn back over itself); an exit as near both ends changes nothing.
#[test]
fn an_open_line_ends_at_its_end_nearest_its_exit_unless_its_entry_is_set() {
    let p = Params::for_style(Style::Running).sane();
    let open = line((0.0, 0.0), (60.0, 0.0));
    // 25.5 mm from the right end, 35.4 from the left: both moves in are cut, so turning adds none.
    let between = Start::Needle(Point::new(35.0, 5.0));
    assert!(last_point(&aimed(&open, &p, between, None)).x < 1.0, "from the needle's nearer end, the right one, to the left");
    assert!(last_point(&aimed(&open, &p, between, Some((56.0, 2.0)))).x > 59.0, "turned to end at the exit's end");
    // 3 mm from the left end: turning it would make the move in 63 mm, a cut more.
    let needle = Start::Needle(Point::new(-3.0, 0.0));
    assert!(last_point(&aimed(&open, &p, needle, None)).x > 59.0, "from the needle's end to the far one");
    assert_eq!(aimed(&open, &p, needle, Some((4.0, 2.0))), aimed(&open, &p, needle, None), "not turned at the cost of a cut");
    let entered = Start::Entry(Point::new(60.0, 0.0));
    assert!(last_point(&aimed(&open, &p, entered, Some((55.0, 1.0)))).x < 1.0, "the entry's end starts it: it ends at the other");
    assert_eq!(aimed(&open, &p, needle, Some((30.0, 5.0))), aimed(&open, &p, needle, None), "an exit as near both ends");
}

/// A closed running line goes on past its start to its needle point nearest the exit, the shorter
/// way, in its own holes: every point after the lap is one of the lap's, at most half of them; with
/// the exit at its start it is sewn as without it.
#[test]
fn a_closed_line_goes_on_round_to_its_exit_in_its_own_holes() {
    let p = Params { lock: false, ..Params::for_style(Style::Running) }.sane();
    let square = rect(0.0, 0.0, 40.0, 40.0);
    let entered = Start::Entry(Point::new(0.0, 0.0));
    let plain = points_of(&aimed(&square, &p, entered, None));
    for exit in [(40.0, 20.0), (5.0, 40.0), (0.0, 12.0)] {
        let out = points_of(&aimed(&square, &p, entered, Some(exit)));
        let end = *out.last().unwrap();
        assert!(end.distance(Point::new(exit.0, exit.1)) <= 0.65 * p.stitch_length, "{exit:?}: ends at {end:?}");
        let lap = plain.len();
        assert!(out.len() > lap && out.len() - lap <= lap / 2 + 1, "{exit:?}: {} points after a lap of {lap}", out.len() - lap);
        for q in &out[lap..] {
            assert!(plain.iter().any(|h| h.distance(*q) < 1e-9), "{exit:?}: {q:?} is a hole of the lap");
        }
    }
    assert_eq!(aimed(&square, &p, entered, Some((0.0, 0.0))), aimed(&square, &p, entered, None), "an exit at its start");
}

fn points_of(v: &[Stitch]) -> Vec<Point> {
    v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
}

/// Of a line's subpaths, the last is chosen and turned to end nearest the exit, the first staying
/// the one nearest the needle; an order that would need a jump, a cut or a longer connecting stitch
/// to reach the exit is not taken: the line is sewn as without the exit.
#[test]
fn a_lines_last_subpath_ends_nearest_its_exit_without_a_jump_more() {
    let p = Params::for_style(Style::Running).sane();
    // The second and third lines start as near the first one's end (√5 mm), 2 mm apart.
    let mut three = line((0.0, 0.0), (10.0, 0.0));
    three.extend(line((12.0, 1.0), (22.0, 1.0)));
    three.extend(line((12.0, -1.0), (22.0, -1.0)));
    let needle = Start::Needle(Point::new(0.0, 0.0));
    let plain = aimed(&three, &p, needle, None);
    assert!(last_point(&plain).distance(Point::new(12.0, -1.0)) < 1e-9, "nearest next ends on the third line's left end");
    let toward = aimed(&three, &p, needle, Some((12.0, 1.0)));
    assert!(last_point(&toward).distance(Point::new(12.0, 1.0)) < 1e-9, "the second line sewn last, ending at the exit");
    assert!(points_of(&toward).first().unwrap().distance(Point::new(0.0, 0.0)) < 1e-9, "the first line stays first");
    let jumps = |v: &[Stitch]| v.iter().filter(|s| s.command != Command::Stitch).count();
    assert_eq!(jumps(&toward), jumps(&plain));
    // An exit 0.6 mm from where the second line would end and 1.4 mm from the plain end: a gain
    // under the miss the Check tab lets pass, so the line is sewn as without it.
    assert_eq!(aimed(&three, &p, needle, Some((12.0, 0.4))), plain, "a gain under a millimetre");
    // The second and third lines 0 and 3 mm off the first's line: sewing the second last takes
    // √13 + 3 mm of connecting stitches against 2 + 3 plain, so it is not taken.
    let mut offset = line((0.0, 0.0), (10.0, 0.0));
    offset.extend(line((12.0, 0.0), (22.0, 0.0)));
    offset.extend(line((12.0, 3.0), (22.0, 3.0)));
    assert_eq!(aimed(&offset, &p, needle, Some((12.0, 0.0))), aimed(&offset, &p, needle, None), "more connecting stitches");
    // Two lines 10 mm apart: the second can end at its left end only after a jump past the trim.
    let mut two = line((0.0, 0.0), (20.0, 0.0));
    two.extend(line((30.0, 0.0), (50.0, 0.0)));
    let needle = Start::Needle(Point::new(-5.0, 0.0));
    assert_eq!(aimed(&two, &p, needle, Some((30.0, 5.0))), aimed(&two, &p, needle, None));
}

/// An open satin column entered at one end ends at its end nearest its exit with every underlay,
/// none included: where the underlay would leave its top stitching ending at the other end, one
/// pass along its middle, before any top crossing, turns it round. Started free (no entry), the
/// needle as far from both ends, it is started at the end that ends it there, with no pass. An exit
/// at its own end changes nothing.
#[test]
fn a_satin_column_ends_at_its_end_nearest_its_exit() {
    let column = line((0.0, 0.0), (60.0, 0.0));
    for underlay in [Underlay::None, Underlay::Center, Underlay::Edge, Underlay::EdgeZigzag] {
        let p = Params { width: 3.0, underlay, ..Params::for_style(Style::Satin) }.sane();
        let [left, right] = p.compensation(p.width);
        let reach = p.width / 2.0 + left.max(right) + 0.01;
        let entered = Start::Entry(Point::new(0.0, 0.0));
        let plain = aimed(&column, &p, entered, None);
        for end in [0.0, 60.0] {
            let out = aimed(&column, &p, entered, Some((end, 2.0)));
            let last = last_point(&out);
            assert!((last.x - end).abs() <= p.column_spacing(p.width) && last.y.abs() <= reach, "{underlay:?}, exit at {end}: ends at {last:?}");
            // Its own end already: the same stitches; else at most a travel stitch every 2 mm more.
            let extra = points_of(&out).len().saturating_sub(points_of(&plain).len());
            assert!(extra as f64 <= 60.0 * 1.3 / 2.0 + 3.0, "{underlay:?}, exit at {end}: {extra} points more");
            if (last_point(&plain).x - end).abs() <= p.column_spacing(p.width) {
                assert_eq!(out, plain, "{underlay:?}: an exit at its own end");
            }
            let free = aimed(&column, &p, Start::Needle(Point::new(30.0, -10.0)), Some((end, 2.0)));
            assert!((last_point(&free).x - end).abs() <= p.column_spacing(p.width), "{underlay:?}, free, exit at {end}");
            assert!(points_of(&free).len() <= points_of(&plain).len(), "{underlay:?}, free: no pass");
        }
    }
}

/// A free open satin column (no entry) whose exit lies at the end it doesn't end at: turned round,
/// its move in from the needle would jump and cut, so it starts nearest the needle and a pass along
/// its middle ends it at the exit (the project owner's choice, 2026-10-26: the cheaper of the two by
/// the planner's weights, turned only when that adds no jump or cut), with every underlay.
#[test]
fn a_free_satin_is_turned_round_only_when_that_adds_no_jump_or_cut() {
    let column = line((0.0, 0.0), (60.0, 0.0));
    let needle = Start::Needle(Point::new(-2.0, 0.0));
    for underlay in [Underlay::None, Underlay::Center, Underlay::Edge, Underlay::EdgeZigzag] {
        let p = Params { width: 3.0, underlay, ..Params::for_style(Style::Satin) }.sane();
        let plain = aimed(&column, &p, needle, None);
        let exit = if last_point(&plain).x < 30.0 { 60.0 } else { 0.0 };
        let out = aimed(&column, &p, needle, Some((exit, 2.0)));
        let first = points_of(&out)[0];
        assert!(first.x < 30.0, "{underlay:?}: started nearest the needle, at {first:?}");
        assert!((last_point(&out).x - exit).abs() <= p.column_spacing(p.width), "{underlay:?}: ends at its exit, {:?}", last_point(&out));
        assert!(points_of(&out).len() > points_of(&plain).len(), "{underlay:?}: with a pass");
    }
}

/// A satin ring ends at its exit: its underlay from its entry, then a travel along its middle the
/// shorter way, its top stitching from there round to there.
#[test]
fn a_satin_ring_ends_at_its_exit() {
    let p = Params { width: 3.0, ..Params::for_style(Style::Satin) }.sane();
    let [left, right] = p.compensation(p.width);
    let reach = p.width / 2.0 + left.max(right) + p.column_spacing(p.width);
    let ring = circle((50.0, 50.0), 20.0);
    let entered = Start::Entry(Point::new(70.0, 50.0));
    for exit in [(30.0, 50.0), (50.0, 30.0), (64.0, 64.0)] {
        let last = last_point(&aimed(&ring, &p, entered, Some(exit)));
        let on_ring = Point::new(50.0, 50.0) + (Point::new(exit.0, exit.1) - Point::new(50.0, 50.0)).normalize() * 20.0;
        assert!(last.distance(on_ring) <= reach, "exit {exit:?}: ends at {last:?}, its point on the ring {on_ring:?}");
    }
}

fn polygon(pts: &[(f64, f64)]) -> BezPath {
    let mut g = BezPath::new();
    for (k, q) in pts.iter().enumerate() {
        if k == 0 { g.move_to(*q) } else { g.line_to(*q) }
    }
    g.close_path();
    g
}

/// How far a fill's last needle point may lie from an exit on a side its rows cross, millimetres:
/// two row spacings (the row ends along the side lie a spacing or so apart, and the row a seam
/// takes of its neighbour's may end a little inside) and the row's pull compensation past the edge.
fn fill_reach(p: &Params) -> f64 {
    let [a, b] = p.compensation(100.0);
    2.0 * p.density + a.max(b)
}

fn jumps(v: &[Stitch]) -> usize {
    v.iter().filter(|s| s.command != Command::Stitch).count()
}

/// A rectangle with the default rows (45°) ends at an exit anywhere on its outline (its corners and
/// the middle of its sides), with and without underlay: cut along the row nearest the exit, the far
/// strip sewn first. It lifts and cuts no more than without the exit and starts where it did.
/// Mutation caught: without the cut, an exit in the middle of a side is missed by tens of
/// millimetres (only the extreme rows' ends can end a single region).
#[test]
fn a_fill_ends_at_its_exit_on_any_side_its_rows_cross() {
    let shape = rect(0.0, 0.0, 80.0, 60.0);
    let needle = Start::Needle(Point::new(-5.0, -5.0));
    for underlay in [Underlay::None, Underlay::Auto] {
        let p = Params { underlay, ..Params::for_style(Style::Fill) }.sane();
        let plain = aimed(&shape, &p, needle, None);
        for exit in [(0.0, 0.0), (80.0, 0.0), (80.0, 60.0), (0.0, 60.0), (40.0, 0.0), (80.0, 30.0), (40.0, 60.0), (0.0, 30.0)] {
            let out = aimed(&shape, &p, needle, Some(exit));
            let miss = last_point(&out).distance(Point::new(exit.0, exit.1));
            assert!(miss <= fill_reach(&p), "underlay {underlay:?}, exit {exit:?}: missed by {miss:.2} mm");
            assert!(jumps(&out) <= jumps(&plain), "underlay {underlay:?}, exit {exit:?}: {} moves against {}", jumps(&out), jumps(&plain));
            assert!(first_point(&out).distance(first_point(&plain)) < 1e-9, "underlay {underlay:?}, exit {exit:?}: started elsewhere");
        }
    }
}

/// An exit on a side the rows run along can only be reached at that side's ends: the fill ends at
/// the corner nearer it.
#[test]
fn an_exit_on_a_side_the_rows_run_along_ends_at_its_nearer_corner() {
    let p = Params { angle: 0.0, underlay: Underlay::None, ..Params::for_style(Style::Fill) }.sane();
    let out = aimed(&rect(0.0, 0.0, 80.0, 60.0), &p, Start::Needle(Point::new(40.0, -5.0)), Some((20.0, 60.0)));
    let end = last_point(&out);
    assert!(end.distance(Point::new(0.0, 60.0)) <= fill_reach(&p), "ends at {end:?}");
}

/// Entered and left at one end of a bar (its rows across it), a fill goes to the far end under its
/// rows still to come and sews back toward the exit: it ends there, starts at the entry, and never
/// lifts the needle.
#[test]
fn a_fill_entered_and_left_at_one_end_ends_there_without_a_jump() {
    for underlay in [Underlay::None, Underlay::Auto] {
        let p = Params { angle: 90.0, underlay, ..Params::for_style(Style::Fill) }.sane();
        let bar = rect(0.0, 0.0, 100.0, 20.0);
        // The bar's end runs along its rows: an exit there is reached at its corner.
        let out = aimed(&bar, &p, Start::Entry(Point::new(100.0, 10.0)), Some((100.0, 20.0)));
        assert_eq!(jumps(&out), 0, "underlay {underlay:?}");
        assert!(last_point(&out).distance(Point::new(100.0, 20.0)) <= fill_reach(&p), "underlay {underlay:?}: ends at {:?}", last_point(&out));
        assert!(first_point(&out).distance(Point::new(100.0, 10.0)) <= p.density, "underlay {underlay:?}: starts at its entry");
    }
}

/// Shapes of several regions (a U, a ring) end at an exit at the tip of an arm or by the hole, with
/// no more moves than without the exit.
#[test]
fn a_fill_of_several_regions_ends_at_its_exit() {
    let u = polygon(&[(0.0, 0.0), (14.0, 0.0), (14.0, 30.0), (9.0, 30.0), (9.0, 8.0), (5.0, 8.0), (5.0, 30.0), (0.0, 30.0)]);
    let mut ring = rect(0.0, 0.0, 40.0, 30.0);
    ring.extend(polygon(&[(13.0, 13.0), (13.0, 17.0), (17.0, 17.0), (17.0, 13.0)]));
    for (name, shape, exits) in [("U", u, vec![(2.5, 30.0), (11.5, 30.0), (7.0, 0.0)]), ("ring", ring, vec![(15.0, 13.0), (40.0, 15.0)])] {
        for angle in [0.0, 45.0, 90.0] {
            let p = Params { angle, ..Params::for_style(Style::Fill) }.sane();
            let needle = Start::Needle(Point::new(-3.0, -3.0));
            let plain = aimed(&shape, &p, needle, None);
            for exit in &exits {
                let out = aimed(&shape, &p, needle, Some(*exit));
                let miss = last_point(&out).distance(Point::new(exit.0, exit.1));
                let plain_miss = last_point(&plain).distance(Point::new(exit.0, exit.1));
                assert!(miss <= plain_miss + 1e-9, "{name} at {angle}°, exit {exit:?}: worse than without it");
                assert!(jumps(&out) <= jumps(&plain), "{name} at {angle}°, exit {exit:?}");
            }
        }
    }
}

/// An exit the fill could reach only by lifting the needle once more (in the first of two pieces,
/// the needle beside it) leaves the fill as it is without the exit; so does an exit where the fill
/// ends anyway, and one that isn't a number. One a kilometre away gives finite stitches.
#[test]
fn an_exit_out_of_reach_or_already_reached_changes_nothing() {
    let p = Params { underlay: Underlay::None, ..Params::for_style(Style::Fill) }.sane();
    let mut two = rect(0.0, 0.0, 20.0, 20.0);
    two.extend(rect(50.0, 0.0, 20.0, 20.0));
    let needle = Start::Needle(Point::new(-2.0, 10.0));
    let plain = aimed(&two, &p, needle, None);
    assert_eq!(aimed(&two, &p, needle, Some((20.0, 20.0))), plain, "reached only by a second lift");
    let end = last_point(&plain);
    assert_eq!(aimed(&two, &p, needle, Some((end.x, end.y))), plain, "where it ends anyway");
    assert_eq!(aimed(&two, &p, needle, Some((f64::NAN, 3.0))), plain, "an exit that isn't a number");
    let far = aimed(&two, &p, needle, Some((1.0e6, 0.0)));
    assert!(!far.is_empty() && far.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
}

/// A T 3 mm wide sewn as satin columns (its stem and its bar's two arms) round the square of tatami
/// where they meet: an exit at the stem's foot or at the far arm's end ends it there, the part
/// order and the last column's direction (a pass along its middle where its underlay would end it at
/// the other end) chosen for it, with no more moves than without the exit. An exit on the arm the
/// needle lies beside is out of reach, never worse: the move to any other part first is cut.
#[test]
fn a_fill_with_narrow_parts_ends_at_its_exit() {
    use vectorcraft_geom::FillRule;
    /// The widest part sewn as satin, millimetres: the default `narrowAsSatin`.
    const NARROW: f64 = 7.0;
    let t = polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&t, FillRule::NonZero, NARROW);
    let rest = polygon(&[(13.5, 0.0), (16.5, 0.0), (16.5, 3.0), (13.5, 3.0)]);
    let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
    let sewn = |exit: Option<(f64, f64)>| {
        let aim = Aim { start: Start::Needle(Point::new(-5.0, 1.5)), exit: exit.map(|(x, y)| Point::new(x, y)) };
        try_fill_with_columns_aimed(&t, FillRule::NonZero, &rest, &found, &fill, &satin, aim).unwrap()
    };
    let plain = sewn(None);
    // Half the column's width and its pull, and a crossing's pitch along it.
    let reach = 1.5 + satin.pull[0].max(satin.pull[1]) + satin.column_spacing(3.0);
    for exit in [(15.0, 23.0), (30.0, 1.5)] {
        let out = sewn(Some(exit));
        let miss = last_point(&out).distance(Point::new(exit.0, exit.1));
        assert!(miss <= reach, "exit {exit:?}: missed by {miss:.2} mm");
        assert!(jumps(&out) <= jumps(&plain), "exit {exit:?}");
    }
    // On the arm the needle lies beside: the move to any other part is cut (past the 10 mm trim
    // length), so the arm stays first and the exit out of reach, never worse.
    let exit = Point::new(0.0, 1.5);
    let out = sewn(Some((exit.x, exit.y)));
    assert!(last_point(&out).distance(exit) <= last_point(&plain).distance(exit) + 1e-9);
    assert!(points_of(&out)[0].x < 10.0, "the arm beside the needle first: {:?}", points_of(&out)[0]);
}

/// An entry set by a fill's narrow part, sewn as a satin column, is where the needle goes in: it
/// travels from there inside to the column's nearer end, hidden under the column's stitches still
/// to come (a column starts only at an end), rather than going in at that end. The T's left arm,
/// entered on its lower side 8 mm from its tip.
#[test]
fn an_entry_by_a_narrow_part_is_where_the_needle_goes_in() {
    use vectorcraft_geom::FillRule;
    /// The widest part sewn as satin, millimetres: the default `narrowAsSatin`.
    const NARROW: f64 = 7.0;
    let t = polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&t, FillRule::NonZero, NARROW);
    let rest = polygon(&[(13.5, 0.0), (16.5, 0.0), (16.5, 3.0), (13.5, 3.0)]);
    let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
    let entry = Point::new(8.0, 0.0);
    let out =
        try_fill_with_columns_aimed(&t, FillRule::NonZero, &rest, &found, &fill, &satin, Aim { start: Start::Entry(entry), exit: None }).unwrap();
    let first = points_of(&out)[0];
    assert!(first.distance(entry) < 1e-9, "goes in at {first:?}");
    assert!(jumps(&out) == 0, "travels on inside, with no jump");
}

/// A fill's rows aimed at an exit may travel over rows already sewn up to the miss the Check tab
/// lets pass (1 mm), and no more (the project owner's choice, 2026-10-26): a 60 × 20 mm bar entered
/// and aimed at the middle of its right side turns round a row's end there (0.50 mm over sewn rows)
/// and ends 0.20 mm from the exit, not 9.88 mm; an 80 × 25 mm fill at 45° aimed near its bottom right
/// corner would show 1.28 mm, so it ends where it would without the exit (a 2 mm allowance let it
/// through).
#[test]
fn a_fill_shows_at_most_a_millimetre_more_travel_toward_its_exit() {
    let bar = rect(0.0, 0.0, 60.0, 20.0);
    let p = Params { angle: 0.0, ..Params::for_style(Style::Fill) }.sane();
    let entered = Start::Entry(Point::new(60.0, 10.0));
    let plain = aimed(&bar, &p, entered, None);
    assert!(last_point(&plain).distance(Point::new(60.0, 10.0)) > 9.0, "the plain rows end at a corner");
    let toward = aimed(&bar, &p, entered, Some((60.0, 10.0)));
    assert!(last_point(&toward).distance(Point::new(60.0, 10.0)) < 0.3, "half a millimetre over sewn rows: kept");
    let wide = rect(0.0, 0.0, 80.0, 25.0);
    let p = Params { angle: 45.0, ..Params::for_style(Style::Fill) }.sane();
    let entered = Start::Entry(Point::new(0.0, 0.0));
    assert_eq!(aimed(&wide, &p, entered, Some((78.75, 0.0))), aimed(&wide, &p, entered, None), "1.28 mm over sewn rows: refused");
}
