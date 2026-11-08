//! A satin column's caps: an open column's top stitching reaches past the path's ends by half its
//! width, round or square as the stroke paints them; a ring, a column whose ends meet, and a butt
//! cap are sewn as before.

use vectorcraft_geom::{BezPath, FillRule, Point};

use super::*;
use crate::Command;

/// The width of the columns tried (millimetres), and the half of it a cap reaches past each end.
const WIDTH: f64 = 3.0;
const HALF: f64 = WIDTH / 2.0;

/// The length of the straight column tried (millimetres).
const LENGTH: f64 = 20.0;

/// How near the path's own end plus a cap's reach the column's last crossing lies at most
/// (millimetres): a crossing's pitch on this width (half the auto spacing of 0.5 mm) and the share
/// of a round cap's tip a stitch of the shortest length leaves out.
const TIP: f64 = 0.3;

/// A satin column `WIDTH` wide along `path` with `cap`, no underlay, ties or compensation: its
/// needle points.
fn column(path: &BezPath, cap: Cap) -> Vec<Point> {
    let p = Params { width: WIDTH, cap, underlay: Underlay::None, lock: false, pull: [0.0; 2], ..Params::for_style(Style::Satin) };
    points(&stitches(path, FillRule::NonZero, &p))
}

fn points(v: &[Stitch]) -> Vec<Point> {
    v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
}

fn straight() -> BezPath {
    let mut p = BezPath::new();
    p.move_to((0.0, 0.0));
    p.line_to((LENGTH, 0.0));
    p
}

/// A circle of `radius` drawn as one open path that comes back to its start, without closing.
fn open_circle(radius: f64) -> BezPath {
    let ring: BezPath = vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Circle::new((0.0, 0.0), radius), 0.01).collect();
    ring.elements().iter().filter(|el| !matches!(el, vectorcraft_geom::PathEl::ClosePath)).copied().collect()
}

fn extent(q: &[Point]) -> (f64, f64) {
    (q.iter().map(|a| a.x).fold(f64::INFINITY, f64::min), q.iter().map(|a| a.x).fold(f64::NEG_INFINITY, f64::max))
}

/// A round cap: the crossings go on past each end, narrowing round a half disc as wide as the
/// column, and every needle point lies within the stroke as it is painted.
#[test]
fn a_round_cap_is_sewn_round_past_each_end() {
    let q = column(&straight(), Cap::Round);
    let (lo, hi) = extent(&q);
    assert!((-HALF - 1e-9..=-HALF + TIP).contains(&lo), "the start reaches {lo}");
    assert!((LENGTH + HALF - TIP..=LENGTH + HALF + 1e-9).contains(&hi), "the end reaches {hi}");
    for a in &q {
        let inside = if a.x < 0.0 {
            a.distance(Point::new(0.0, 0.0)) <= HALF + 1e-9
        } else if a.x > LENGTH {
            a.distance(Point::new(LENGTH, 0.0)) <= HALF + 1e-9
        } else {
            a.y.abs() <= HALF + 1e-9
        };
        assert!(inside, "{a:?} lies outside the round-capped stroke");
    }
    // Near the tip the crossings are short: none past a millimetre beyond the end reaches the rails.
    assert!(q.iter().filter(|a| a.x < -1.0 || a.x > LENGTH + 1.0).all(|a| a.y.abs() < 1.2), "a crossing at a round tip spans the full width");
    assert!(q.iter().any(|a| a.x < -1.0) && q.iter().any(|a| a.x > LENGTH + 1.0), "no crossing near the tips");
}

/// A square cap: the column goes on past each end by half its width, at its full width.
#[test]
fn a_square_cap_is_sewn_at_full_width_past_each_end() {
    let q = column(&straight(), Cap::Square);
    let (lo, hi) = extent(&q);
    assert!((-HALF - 1e-9..=-HALF + TIP).contains(&lo), "the start reaches {lo}");
    assert!((LENGTH + HALF - TIP..=LENGTH + HALF + 1e-9).contains(&hi), "the end reaches {hi}");
    assert!(q.iter().all(|a| a.y.abs() <= HALF + 1e-9), "past the column's width");
    assert!(q.iter().any(|a| a.x < -1.0 && a.y.abs() > HALF - 1e-6), "the square cap's corners are not reached");
}

/// A butt cap sews the column at the path's ends, as every column was sewn before caps; a ring has
/// no ends, and a column whose ends meet (a circle drawn without closing it) has none to cap: each
/// is sewn the same whatever its cap.
#[test]
fn a_butt_cap_a_ring_and_ends_that_meet_are_sewn_as_before() {
    let butt = column(&straight(), Cap::Butt);
    let (lo, hi) = extent(&butt);
    assert!(lo >= -1e-9 && hi <= LENGTH + 1e-9, "a butt column reaches {lo} to {hi}");
    assert_ne!(column(&straight(), Cap::Round), butt, "a round cap changes nothing");
    let ring: BezPath = vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Circle::new((0.0, 0.0), 10.0), 0.01).collect();
    let open = open_circle(10.0);
    for cap in [Cap::Round, Cap::Square] {
        assert_eq!(column(&ring, cap), column(&ring, Cap::Butt), "a ring with a {} cap", cap.id());
        assert_eq!(column(&open, cap), column(&open, Cap::Butt), "an open circle with a {} cap", cap.id());
    }
}

/// No stitch at a round tip is shorter than the shortest the planner sews: the cap stops where a
/// crossing of its half disc would be.
#[test]
fn no_stitch_at_a_round_tip_is_shorter_than_the_shortest() {
    for width in [1.0, 1.5, 3.0, 6.0, 9.0] {
        let p = Params { width, cap: Cap::Round, underlay: Underlay::None, lock: false, ..Params::for_style(Style::Satin) };
        let q = points(&stitches(&straight(), FillRule::NonZero, &p));
        let shortest = q.windows(2).map(|w| w[0].distance(w[1])).fold(f64::INFINITY, f64::min);
        assert!(shortest >= p.min_stitch - 1e-9, "{width} mm: a stitch of {shortest}");
    }
}

/// Push compensation shortens a capped column from the cap's end: the end it is sewn to is the
/// painted end less the push.
#[test]
fn push_compensation_shortens_a_capped_column_from_its_caps() {
    let p = Params { width: WIDTH, cap: Cap::Square, underlay: Underlay::None, lock: false, push: [1.0, 2.0], ..Params::for_style(Style::Satin) };
    let (lo, hi) = extent(&points(&stitches(&straight(), FillRule::NonZero, &p)));
    assert!((lo - (-HALF + 1.0)).abs() < TIP && lo >= -HALF + 1.0 - 1e-9, "the start at {lo}");
    assert!((hi - (LENGTH + HALF - 2.0)).abs() < TIP && hi <= LENGTH + HALF - 2.0 + 1e-9, "the end at {hi}");
}

/// The underlay holds up the caps too (the project owner's choice, 2026-11-07, „Podszycie w
/// zakończeniach (polecam)”), inside the stroke as it is painted: a centre run along a narrow
/// round-capped column goes on into each cap, stopping short of its tip by the underlay's inset; an
/// edge run and a zigzag along a wide one keep inside the round-capped stroke, the zigzag reaching
/// into the caps, its sides narrowing with them.
#[test]
fn the_underlay_reaches_into_the_caps_inside_the_stroke() {
    let p = Params { width: 1.5, cap: Cap::Round, underlay: Underlay::Center, lock: false, pull: [0.0; 2], ..Params::for_style(Style::Satin) };
    let q = points(&stitches(&straight(), FillRule::NonZero, &p));
    let on_the_middle: Vec<&Point> = q.iter().filter(|a| a.y.abs() <= 1e-9).collect();
    let (lo, hi) = extent(&on_the_middle.iter().map(|a| **a).collect::<Vec<_>>());
    let (tip_lo, tip_hi) = extent(&q);
    assert!(lo < -0.25 && hi > LENGTH + 0.25, "the centre run stops at the path's ends: {lo}..{hi}");
    assert!(lo > tip_lo && hi < tip_hi, "the centre run reaches the tips {tip_lo}..{tip_hi}: {lo}..{hi}");
    let wide = 6.0;
    let p = Params { width: wide, cap: Cap::Round, underlay: Underlay::EdgeZigzag, lock: false, pull: [0.0; 2], ..Params::for_style(Style::Satin) };
    let q = points(&stitches(&straight(), FillRule::NonZero, &p));
    let inside = |a: &Point| {
        let x = a.x.clamp(0.0, LENGTH);
        a.distance(Point::new(x, 0.0)) <= wide / 2.0 + 1e-9
    };
    assert!(q.iter().all(inside), "a needle point outside the round-capped stroke");
    assert!(q.iter().any(|a| a.x > LENGTH + 1.0 && a.y.abs() > 0.5 && a.y.abs() < wide / 2.0 - 0.3), "no zigzag in the end's cap");
}

/// A capped column's ends ([`ends`]): it ends past the path's by its cap, where its top stitching
/// ends; a square-capped one starts there too, with an underlay or without, the underlay going
/// into the cap. Sewn from each start, its first needle point lies within the column's half width
/// and pull compensation of it, on a rail.
#[test]
fn a_capped_column_starts_and_ends_where_its_stitches_do() {
    let near = |a: Point, b: Point| a.distance(b) < 1e-9;
    let capped = [Point::new(-HALF, 0.0), Point::new(LENGTH + HALF, 0.0)];
    let path_ends = [Point::new(0.0, 0.0), Point::new(LENGTH, 0.0)];
    for (underlay, starts) in [(Underlay::None, capped), (Underlay::Auto, capped)] {
        let p = Params { width: WIDTH, cap: Cap::Square, underlay, ..Params::for_style(Style::Satin) };
        let e = ends(&straight(), &p, false).expect("an open column has ends");
        assert!(capped.iter().all(|&c| e.ends.iter().any(|&q| near(q, c))), "{:?}: ends {:?}", underlay, e.ends);
        assert!(starts.iter().all(|&c| e.starts.iter().any(|&q| near(q, c))), "{:?}: starts {:?}", underlay, e.starts);
        // The rails lie half the width out, and the pull compensation beyond.
        let [left, right] = p.compensation(WIDTH);
        for &entry in &e.starts {
            let sewn = try_stitches_aimed(&straight(), FillRule::NonZero, &p, Aim { start: Start::Entry(entry), exit: None }).expect("sewn");
            let first = points(&sewn).first().copied().expect("a needle point");
            assert!(
                first.distance(entry) <= HALF + left.max(right) + 1e-9,
                "{underlay:?}: starts {first:?}, {} from {entry:?}",
                first.distance(entry)
            );
        }
    }
    let butt = ends(&straight(), &Params { width: WIDTH, ..Params::for_style(Style::Satin) }, false).expect("ends");
    assert!(path_ends.iter().all(|&c| butt.starts.iter().any(|&q| near(q, c)) && butt.ends.iter().any(|&q| near(q, c))), "{butt:?}");
}

/// Ends within a thread's width meet (the project owner's choice, 2026-11-07, „Jak zamknięte
/// (polecam)”): a ring drawn open with its ends 0.3 mm apart, or 0.39 mm, sews no cap over its
/// column, as a ring whose ends meet exactly; its ends 0.5 mm apart, they take their caps.
#[test]
fn ends_within_a_threads_width_meet() {
    let arc = |gap: f64| -> BezPath {
        let a = vectorcraft_geom::kurbo::Arc::new((0.0, 0.0), (10.0, 10.0), 0.0, std::f64::consts::TAU - gap / 10.0, 0.0);
        vectorcraft_geom::Shape::path_elements(&a, 0.01).collect()
    };
    for cap in [Cap::Round, Cap::Square] {
        for gap in [0.3, 0.39] {
            assert_eq!(column(&arc(gap), cap), column(&arc(gap), Cap::Butt), "{gap} mm apart, a {} cap", cap.id());
        }
        assert_ne!(column(&arc(0.5), cap), column(&arc(0.5), Cap::Butt), "0.5 mm apart, no {} cap", cap.id());
    }
}

/// No cap is sewn where pieces of one satin meet (the project owner's choice, 2026-11-07, „Jak
/// zamknięte (polecam)”): a T drawn as two pieces of one path, its stem's end on its bar, sews no
/// cap there, no crossing of the stem's lying past the bar's middle, but one at the stem's foot; a
/// P drawn as one open path, its end on its own stem, sews none there, no crossing of its bowl past
/// the stem's middle.
#[test]
fn no_cap_is_sewn_where_pieces_of_one_satin_meet() {
    // Crossings running along x (`along_x`) or along y, lying where `at` says.
    let crossings = |q: &[Point], along_x: bool, at: &dyn Fn(Point) -> bool| {
        q.windows(2)
            .filter(|w| {
                let d = w[1] - w[0];
                let (run, off) = if along_x { (d.x.abs(), d.y.abs()) } else { (d.y.abs(), d.x.abs()) };
                run > 0.8 * WIDTH && off < 0.5 && at(w[0].midpoint(w[1]))
            })
            .count()
    };
    let mut t = BezPath::new();
    t.move_to((0.0, 0.0));
    t.line_to((LENGTH, 0.0));
    t.move_to((LENGTH / 2.0, 0.0));
    t.line_to((LENGTH / 2.0, 15.0));
    let q = column(&t, Cap::Square);
    assert_eq!(crossings(&q, true, &|m| m.y < -1e-9), 0, "a cap over the bar");
    assert!(crossings(&q, true, &|m| m.y > 15.0) > 0, "no cap at the stem's foot");
    let mut p = BezPath::new();
    p.move_to((0.0, 30.0));
    p.line_to((0.0, 0.0));
    p.line_to((8.0, 0.0));
    p.line_to((8.0, 10.0));
    p.line_to((0.0, 10.0));
    let q = column(&p, Cap::Square);
    assert_eq!(crossings(&q, false, &|m| m.x < -1e-9), 0, "a cap over the stem");
    assert!(crossings(&q, true, &|m| m.y > 30.0) > 0, "no cap at the stem's foot");
}

/// A dot (the project owner's choice, 2026-11-07, „Szyć jako małą kropkę (polecam)”): a subpath of
/// no length, or nearly none, with a round or a square cap is painted as a disc or a square as wide
/// as the stroke (SVG 2 § 9.5.3; the renderer's `zero_length_subpaths_draw_dots_with_their_cap`):
/// its column sews it, a stroke's width across, where it sewed nothing.
#[test]
fn a_dot_is_sewn_as_its_cap_paints_it() {
    for cap in [Cap::Round, Cap::Square] {
        for len in [0.0, 0.05] {
            let mut dot = BezPath::new();
            dot.move_to((10.0, 10.0));
            dot.line_to((10.0 + len, 10.0));
            let q = column(&dot, cap);
            assert!(q.len() > 2, "a {} dot {len} mm long sews {} needle points", cap.id(), q.len());
            let (lo, hi) = extent(&q);
            assert!(hi - lo >= WIDTH - 2.0 * TIP, "a {} dot spans {lo}..{hi}", cap.id());
        }
    }
}

/// Pull compensation reaches as far past the painted half disc round a round cap as along the
/// column: its fixed 0.2 mm is kept whole at every crossing of the cap, where it was scaled with
/// the half chord, to almost nothing at the tip.
#[test]
fn pull_compensation_holds_round_a_round_cap() {
    let pull = 0.2;
    let p = Params { width: WIDTH, cap: Cap::Round, underlay: Underlay::None, lock: false, pull: [pull; 2], ..Params::for_style(Style::Satin) };
    let q = points(&stitches(&straight(), FillRule::NonZero, &p));
    for a in q.iter().filter(|a| a.x > LENGTH + 0.2 && a.x < LENGTH + 1.2) {
        let painted = (HALF * HALF - (a.x - LENGTH).powi(2)).sqrt();
        assert!(a.y.abs() >= painted + pull - 1e-6, "{a:?}: {} beside a half chord of {painted}", a.y.abs());
    }
}

/// Push compensation shortens a round cap and keeps it round (the project owner's choice,
/// 2026-11-07, „Okrągły koniec zostaje okrągły (polecam)”): its tip crossing is no wider than
/// without push, where a 0.5 mm push cut a 3 mm cap's tip to a crossing 2.26 mm wide.
#[test]
fn push_compensation_keeps_a_round_cap_round() {
    let tip = |push: f64| {
        let p = Params {
            width: WIDTH,
            cap: Cap::Round,
            underlay: Underlay::None,
            lock: false,
            pull: [0.0; 2],
            push: [push; 2],
            ..Params::for_style(Style::Satin)
        };
        let q = points(&stitches(&straight(), FillRule::NonZero, &p));
        let (lo, _) = extent(&q);
        q.iter().filter(|a| (a.x - lo).abs() < 1e-9).map(|a| a.y.abs()).fold(0.0, f64::max)
    };
    assert!(tip(0.5) <= tip(0.0) + 1e-6, "a 0.5 mm push cuts the tip {} mm off the middle", tip(0.5));
}

/// A round cap pressed by push compensation stays within the half disc it paints: with a 0.6 mm push
/// every needle point lies inside the round-capped stroke.
#[test]
fn a_round_cap_with_push_stays_on_its_half_disc() {
    let p = Params {
        width: WIDTH,
        cap: Cap::Round,
        underlay: Underlay::None,
        lock: false,
        pull: [0.0; 2],
        push: [0.6, 0.6],
        ..Params::for_style(Style::Satin)
    };
    for a in &points(&stitches(&straight(), FillRule::NonZero, &p)) {
        let inside = if a.x < 0.0 {
            a.distance(Point::new(0.0, 0.0)) <= HALF + 1e-9
        } else if a.x > LENGTH {
            a.distance(Point::new(LENGTH, 0.0)) <= HALF + 1e-9
        } else {
            a.y.abs() <= HALF + 1e-9
        };
        assert!(inside, "{a:?} lies outside the round-capped stroke");
    }
}

/// The underlay holds up a square cap too: a centre run reaches into it, where it stopped at the
/// path's ends.
#[test]
fn the_underlay_reaches_into_a_square_cap() {
    let p = Params { width: 1.5, cap: Cap::Square, underlay: Underlay::Center, lock: false, ..Params::for_style(Style::Satin) };
    let q = points(&stitches(&straight(), FillRule::NonZero, &p));
    let middle: Vec<&Point> = q.iter().filter(|a| a.y.abs() <= 1e-9).collect();
    assert!(middle.iter().any(|a| a.x < -0.25) && middle.iter().any(|a| a.x > LENGTH + 0.25), "the centre run stops at the path's ends: {middle:?}");
}
