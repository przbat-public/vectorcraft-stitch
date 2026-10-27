//! The narrow parts of filled shapes found as satin columns ([`super::narrow`]) and sewn
//! ([`super::fill_with_columns`]).

use vectorcraft_geom::{BezPath, FillRule, Point, Shape, Vec2, kurbo};

use super::*;
use crate::Command;

/// The widest column the tests look for (millimetres).
const WIDEST: f64 = 7.0;

fn polygon(pts: impl IntoIterator<Item = (f64, f64)>) -> BezPath {
    let mut p = BezPath::new();
    for (k, q) in pts.into_iter().enumerate() {
        if k == 0 { p.move_to(q) } else { p.line_to(q) }
    }
    p.close_path();
    p
}

fn circle(c: (f64, f64), r: f64) -> BezPath {
    kurbo::Circle::new(c, r).path_elements(0.01).collect()
}

/// A circle wound the other way: a hole in one wound the usual way.
fn hole(c: (f64, f64), r: f64) -> BezPath {
    let mut p = circle(c, r);
    p = p.reverse_subpaths();
    p
}

fn points(v: &[Stitch]) -> Vec<Point> {
    v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
}

/// A stripe 30 mm long and 3 mm wide is one column: its stations pair the long sides, square
/// across, along all of it.
#[test]
fn a_stripe_is_one_column() {
    let stripe = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&stripe, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{found:?}");
    let c = &found[0];
    assert!(!c.closed);
    assert!((c.width() - 3.0).abs() < 1e-6, "{}", c.width());
    assert!(c.length() > 29.0 && c.length() <= 30.0 + 1e-6, "{}", c.length());
    for (a, b) in &c.stations {
        assert!((a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() > 2.9, "{a:?} {b:?}");
    }
}

/// A ring 2 mm wide is one column that goes round.
#[test]
fn a_ring_is_a_closed_column() {
    let mut ring = circle((0.0, 0.0), 10.0);
    ring.extend(hole((0.0, 0.0), 8.0));
    let found = narrow_columns(&ring, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{}", found.len());
    let c = &found[0];
    assert!(c.closed);
    assert!((c.width() - 2.0).abs() < 0.05, "{}", c.width());
    let round = std::f64::consts::TAU * 9.0;
    assert!((c.length() - round).abs() < 1.0, "{} vs {round}", c.length());
}

/// A compact or wide shape stays a fill: a square, a 10 mm wide rectangle, a dot.
#[test]
fn compact_and_wide_shapes_stay_fills() {
    for (name, shape) in [
        ("square", polygon([(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)])),
        ("wide", polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 10.0), (0.0, 10.0)])),
        ("dot", circle((0.0, 0.0), 2.0)),
    ] {
        assert!(narrow_columns(&shape, FillRule::NonZero, WIDEST).is_empty(), "{name}");
    }
}

/// A curved column (a band round a third of a circle) crosses square to its middle: each station
/// points at the centre.
#[test]
fn a_curved_column_crosses_square_to_its_middle() {
    let arc = |r: f64, from: f64, to: f64| {
        (0..=60).map(move |k| {
            let t = from + (to - from) * f64::from(k) / 60.0;
            (r * t.cos(), r * t.sin())
        })
    };
    let third = std::f64::consts::TAU / 3.0;
    let band = polygon(arc(13.0, 0.0, third).chain(arc(10.0, third, 0.0)));
    let found = narrow_columns(&band, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{}", found.len());
    for (a, b) in &found[0].stations {
        let m = a.midpoint(*b);
        let radial = m.to_vec2() / m.to_vec2().hypot();
        let across = (*a - *b) / a.distance(*b);
        assert!(radial.dot(across).abs() > 0.98, "{a:?} {b:?}");
    }
}

/// A pointed leaf narrows to its tips: the column follows it round both, its end stations
/// narrow and near the tips.
#[test]
fn a_pointed_leaf_narrows_to_its_tips() {
    // Two arcs from (0, 0) to (30, 0), 2 mm either side of the middle at their widest.
    let side = |sign: f64| {
        (0..=60).map(move |k| {
            let x = 30.0 * f64::from(k) / 60.0;
            (x, sign * 2.0 * (std::f64::consts::PI * x / 30.0).sin())
        })
    };
    let leaf = polygon(side(1.0).chain(side(-1.0).rev().skip(1)));
    let found = narrow_columns(&leaf, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{}", found.len());
    let c = &found[0];
    let ends = [c.stations.first().unwrap(), c.stations.last().unwrap()];
    for (a, b) in ends {
        let m = a.midpoint(*b);
        assert!(a.distance(*b) < 0.6, "an end {} wide", a.distance(*b));
        assert!(m.x < 1.0 || m.x > 29.0, "an end at {m:?}");
    }
}

/// A round end is carried round: a stripe with round ends is one column reaching into both.
#[test]
fn a_round_end_is_carried_round() {
    let mut stadium = BezPath::new();
    stadium.move_to((0.0, 0.0));
    stadium.line_to((20.0, 0.0));
    stadium.extend(kurbo::Arc::new((20.0, 1.5), (1.5, 1.5), -std::f64::consts::FRAC_PI_2, std::f64::consts::PI, 0.0).append_iter(0.01));
    stadium.line_to((0.0, 3.0));
    stadium.extend(kurbo::Arc::new((0.0, 1.5), (1.5, 1.5), std::f64::consts::FRAC_PI_2, std::f64::consts::PI, 0.0).append_iter(0.01));
    stadium.close_path();
    let found = narrow_columns(&stadium, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{}", found.len());
    let m = found[0].middles();
    let (lo, hi) = m.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    assert!(lo < -1.0 && hi > 21.0, "the column reaches {lo} to {hi} (the ends at -1.5 and 21.5)");
}

/// A letter T is three columns (the bar's two arms and the stem), its middle left to the fill.
#[test]
fn a_t_is_three_columns() {
    let t = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&t, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 3, "{found:?}");
    for c in &found {
        assert!((c.width() - 3.0).abs() < 0.05, "{}", c.width());
        // None crosses the middle square, where the three meet.
        let inside = c.middles().iter().filter(|m| m.x > 13.6 && m.x < 16.4 && m.y < 2.9).count();
        assert!(inside <= 1, "{inside} stations where the strokes meet");
    }
}

/// The search can be stopped: never asked to, it finds what [`narrow_columns`] finds; asked at its
/// first check or a later one, it gives nothing.
#[test]
fn the_search_for_narrow_parts_stops_when_asked() {
    let t = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)]);
    let all = narrow_columns(&t, FillRule::NonZero, WIDEST);
    let never = narrow_columns_until(&t, FillRule::NonZero, WIDEST, &|| false).unwrap();
    assert_eq!(never.len(), all.len());
    for (a, b) in never.iter().zip(&all) {
        assert_eq!(a.middles(), b.middles());
    }
    assert!(narrow_columns_until(&t, FillRule::NonZero, WIDEST, &|| true).is_none());
    // Asked once per ring and every so many samples and sides: stopped at the third ask.
    let asks = std::cell::Cell::new(0);
    let third = || {
        asks.set(asks.get() + 1);
        asks.get() >= 3
    };
    assert!(narrow_columns_until(&t, FillRule::NonZero, WIDEST, &third).is_none());
    assert_eq!(asks.get(), 3);
}

/// Nothing narrow is found in odd input, and nothing panics.
#[test]
fn odd_input_finds_nothing() {
    let stripe = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let found = narrow_columns(&stripe, FillRule::NonZero, w);
        assert!(w.is_infinite() || found.is_empty(), "{w}: {found:?}");
    }
    assert!(narrow_columns(&BezPath::new(), FillRule::NonZero, WIDEST).is_empty());
    let nan = polygon([(0.0, 0.0), (f64::NAN, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    let _ = narrow_columns(&nan, FillRule::EvenOdd, WIDEST);
    let line = polygon([(0.0, 0.0), (30.0, 0.0), (60.0, 0.0)]);
    assert!(narrow_columns(&line, FillRule::NonZero, WIDEST).is_empty());
}

/// The satin of `shape`'s columns alone (no rest), with `p` for the satin.
fn sew(shape: &BezPath, p: Params) -> Vec<Stitch> {
    let found = narrow_columns(shape, FillRule::NonZero, WIDEST);
    fill_with_columns(shape, FillRule::NonZero, &BezPath::new(), &found, &Params::for_style(Style::Fill), &p, Aim::from_needle(None))
}

/// A column is sewn as satin: crossings square across the stripe from side to side, widened by
/// the pull compensation, with no jump.
#[test]
fn a_column_is_sewn_across_it() {
    let stripe = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    let mut p = Params::for_style(Style::Satin);
    p.underlay = Underlay::None;
    let out = sew(&stripe, p);
    assert!(out.iter().all(|s| s.command == Command::Stitch), "no jumps");
    let q = points(&out);
    // 30 mm at 0.45 mm (auto spacing for 3 mm).
    assert!(q.len() > 60, "{}", q.len());
    let pull = p.pull[0];
    assert!(q.iter().all(|a| a.y >= -pull - 1e-6 && a.y <= 3.0 + pull + 1e-6 && a.x >= -1e-6 && a.x <= 30.0 + 1e-6), "{q:?}");
    let across = q.windows(2).filter(|w| (w[0].y - w[1].y).abs() > 3.0 && (w[0].x - w[1].x).abs() < 0.6).count();
    assert!(across * 10 > q.len() * 9, "{across} of {} stitches cross the stripe", q.len());
}

/// The spacing follows the column's width: in a column that widens from 1.5 to 6 mm, the
/// crossings at its narrow end are farther apart than at its wide end.
#[test]
fn the_spacing_follows_the_width() {
    let wedge = polygon([(0.0, 0.0), (40.0, -2.25), (40.0, 3.75), (0.0, 1.5)]);
    let mut p = Params::for_style(Style::Satin);
    p.underlay = Underlay::None;
    p.lock = false;
    let q = points(&sew(&wedge, p));
    // Two crossings apart is the spacing: the distance between two penetrations on one side.
    let gap = |near: f64| {
        let k = q.iter().position(|a| a.x >= near).unwrap();
        (q[k + 2].x - q[k].x).abs()
    };
    assert!(gap(2.0) > gap(36.0) + 0.05, "{} at the narrow end, {} at the wide end", gap(2.0), gap(36.0));
}

/// A column narrower than a millimetre is a running line along its middle.
#[test]
fn a_hairline_is_a_running_line() {
    let hair = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 0.6), (0.0, 0.6)]);
    let out = sew(&hair, Params::for_style(Style::Satin));
    let q = points(&out);
    assert!(q.len() >= 10, "{}", q.len());
    assert!(q.iter().all(|a| (a.y - 0.3).abs() < 0.05), "{q:?}");
}

/// Push compensation shortens a column at both ends, the mean of its two values each.
#[test]
fn push_compensation_shortens_a_column() {
    let stripe = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (0.0, 3.0)]);
    let mut p = Params::for_style(Style::Satin);
    p.push = [1.0, 3.0];
    let q = points(&sew(&stripe, p));
    let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| (lo.min(a.x), hi.max(a.x)));
    assert!(lo > 1.9 && hi < 28.1, "{lo} to {hi}");
}

/// A ring is sewn round in one go, and a T's three columns and the fill where they meet are sewn
/// without a jump, every needle point on the shape.
#[test]
fn columns_and_their_rest_are_sewn_in_one_go() {
    let mut ring = circle((0.0, 0.0), 10.0);
    ring.extend(hole((0.0, 0.0), 8.0));
    let out = sew(&ring, Params::for_style(Style::Satin));
    assert!(out.iter().all(|s| s.command == Command::Stitch), "the ring in one go");
    let t = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&t, FillRule::NonZero, WIDEST);
    // The rest: the square where the strokes meet (the caller cuts it out with path operations).
    let rest = polygon([(13.5, 0.0), (16.5, 0.0), (16.5, 3.0), (13.5, 3.0)]);
    let fill = Params::for_style(Style::Fill);
    let out = fill_with_columns(&t, FillRule::NonZero, &rest, &found, &fill, &Params::for_style(Style::Satin), Aim::from_needle(None));
    let lifts: Vec<(usize, Stitch)> = out.iter().copied().enumerate().filter(|(_, s)| s.command != Command::Stitch).collect();
    assert!(
        lifts.is_empty(),
        "the T in one go: {lifts:?} around {:?}",
        lifts.first().map(|(i, _)| &out[i.saturating_sub(3)..(*i + 3).min(out.len())])
    );
    let reach = fill.pull[0].max(Params::for_style(Style::Satin).pull[0]) + 1e-6;
    for a in points(&out) {
        let inside = (a.y >= -reach && a.y <= 3.0 + reach && a.x >= -reach && a.x <= 30.0 + reach)
            || (a.x >= 13.5 - reach && a.x <= 16.5 + reach && a.y >= 0.0 && a.y <= 23.0 + reach);
        assert!(inside, "{a:?} off the T");
    }
}

/// The columns found in a few odd shapes lie inside them and are typically no wider than asked
/// (half as wide again where one runs on).
#[test]
fn columns_lie_inside_their_shape() {
    let shapes = [
        polygon([(0.0, 0.0), (40.0, 5.0), (38.0, 9.0), (-2.0, 4.0)]),
        circle((5.0, 5.0), 3.0),
        polygon((0..10).map(|k| {
            let t = std::f64::consts::TAU * f64::from(k) / 10.0;
            let r = if k % 2 == 0 { 20.0 } else { 6.0 };
            (r * t.cos(), r * t.sin())
        })),
    ];
    for shape in &shapes {
        for c in narrow_columns(shape, FillRule::NonZero, WIDEST) {
            assert!(c.width() <= WIDEST + 1e-6, "{}", c.width());
            for (a, b) in &c.stations {
                assert!(a.distance(*b) <= 1.5 * WIDEST + 1e-6, "{a:?} {b:?}");
                let m = a.midpoint(*b);
                assert!(shape.winding(m) != 0 || shape.winding(m + Vec2::new(1e-3, 1e-3)) != 0, "{m:?} outside");
            }
        }
    }
}

/// The parts of a shape are sewn in the order that moves least: a square with an arm on each side,
/// the needle starting by one arm's tip, is sewn arm, square, arm, with no jump.
#[test]
fn parts_are_sewn_in_the_order_that_moves_least() {
    let shape = polygon([
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 8.5),
        (35.0, 8.5),
        (35.0, 11.5),
        (20.0, 11.5),
        (20.0, 20.0),
        (0.0, 20.0),
        (0.0, 11.5),
        (-15.0, 11.5),
        (-15.0, 8.5),
        (0.0, 8.5),
    ]);
    let found = narrow_columns(&shape, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 2, "{found:?}");
    let rest = polygon([(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]);
    for (from, underlay) in [((37.0, 10.0), Underlay::Auto), ((37.0, 10.0), Underlay::None), ((-17.0, 10.0), Underlay::Auto)] {
        let mut satin = Params::for_style(Style::Satin);
        satin.underlay = underlay;
        let out = fill_with_columns(
            &shape,
            FillRule::NonZero,
            &rest,
            &found,
            &Params::for_style(Style::Fill),
            &satin,
            Aim::from_needle(Some(Point::new(from.0, from.1))),
        );
        let lifts = out.iter().filter(|s| s.command != Command::Stitch).count();
        assert_eq!(lifts, 0, "from {from:?}, underlay {underlay:?}: {lifts} jumps and trims");
    }
}

/// A stroke whose width wavers about the limit is one column, not cut where it crosses it: a band
/// 6 to 8 mm wide (typically under 7) is satin all along; one 8 to 10 mm wide stays a fill.
#[test]
fn a_stroke_that_wavers_about_the_limit_is_sewn_whole() {
    let band = |lo: f64, hi: f64| polygon([(0.0, 0.0), (20.0, 0.0), (40.0, 0.0), (40.0, hi), (20.0, lo), (0.0, hi)]);
    let found = narrow_columns(&band(6.0, 8.0), FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{found:?}");
    let (lo, hi) = found[0].middles().iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), m| (lo.min(m.x), hi.max(m.x)));
    assert!(lo < 1.0 && hi > 39.0, "the column runs {lo} to {hi}");
    assert!(narrow_columns(&band(8.0, 10.0), FillRule::NonZero, WIDEST).is_empty());
}

/// The same-rail spacing round a bend centred on `centre`: of `q`'s penetrations within `window`
/// radians either side of the direction `toward` (from the centre), those farther out than
/// `middle` (the outer rail) and those nearer in (the inner one), each the steps between
/// consecutive ones, smallest first: the angle between them times its rail's radius (`rails`:
/// outer, inner). A shortened stitch moves in along its crossing, square to the rail, so it keeps
/// the rail's angle.
pub(super) fn rail_steps(q: &[Point], centre: Point, toward: Vec2, window: f64, middle: f64, rails: [f64; 2]) -> [Vec<f64>; 2] {
    use std::f64::consts::{PI, TAU};
    let mut angles: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    for a in q {
        let v = *a - centre;
        let angle = toward.cross(v).atan2(toward.dot(v));
        if angle.abs() <= window {
            angles[usize::from(v.hypot() < middle)].push(angle);
        }
    }
    let steps = |rail: &[f64], radius: f64| {
        let mut steps: Vec<f64> = rail.windows(2).map(|w| ((w[1] - w[0] + PI).rem_euclid(TAU) - PI).abs() * radius).collect();
        steps.sort_by(f64::total_cmp);
        steps
    };
    [steps(&angles[0], rails[0]), steps(&angles[1], rails[1])]
}

/// The share `at` of the way through `sorted` (0 the smallest, 0.5 the median).
pub(super) fn quantile(sorted: &[f64], at: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * at).round() as usize]
}

/// On a bend a column keeps its spacing on the outer rail, and the inner rail, which runs a
/// shorter way, takes its penetrations closer (shortened where they bunch), as digitizers hold a
/// satin's density on the outside of a turn: a ring 5 mm wide round a 3 mm hole and a letter S of
/// two bowls 5 mm wide round 6 mm have their outer rails the auto spacing for 5 mm (0.40 mm) apart
/// and their inner rails that times the inner rail's radius over the outer's. (With the crossings
/// spaced along the middle, the outer rails lay 0.60 mm apart.) A straight stripe keeps the
/// spacing on both rails.
#[test]
fn a_bend_keeps_its_spacing_on_the_outer_rail() {
    let mut p = Params::for_style(Style::Satin);
    p.underlay = Underlay::None;
    p.lock = false;
    let width = 5.0;
    let spacing = satin_spacing(width);
    // Pull compensation widens both rails by the mean of its two values.
    let [left, right] = p.compensation(width);
    let reach = width / 2.0 + (left + right) / 2.0;
    let check = |name: &str, q: &[Point], centre: (f64, f64), toward: (f64, f64), window: f64, middle: f64| {
        let rails = [middle + reach, middle - reach];
        let [outer, inner] =
            rail_steps(q, Point::new(centre.0, centre.1), Vec2::new(toward.0, toward.1), window, middle, rails).map(|v| quantile(&v, 0.5));
        let closer = spacing * rails[1] / rails[0];
        eprintln!("{name}: outer rail {outer:.3} mm (spacing {spacing}), inner {inner:.3} mm ({closer:.3})");
        assert!((outer / spacing - 1.0).abs() < 0.1, "{name}: the outer rail's penetrations {outer:.3} mm apart, not {spacing}");
        assert!((inner / closer - 1.0).abs() < 0.1, "{name}: the inner rail's penetrations {inner:.3} mm apart, not {closer:.3}");
    };

    let mut ring = circle((0.0, 0.0), 8.0);
    ring.extend(hole((0.0, 0.0), 3.0));
    check("ring", &points(&sew(&ring, p)), (0.0, 0.0), (1.0, 0.0), std::f64::consts::PI, 5.5);

    // The S's middle: a half turn round (0, 0) through (-6, 0), then one the other way round
    // (0, -12) through (6, -12); its sides 2.5 mm either side.
    let (r, steps) = (6.0, 180);
    let middle: Vec<(Point, Vec2)> = (0..=steps)
        .map(|k| {
            let t = std::f64::consts::PI * f64::from(k) / f64::from(steps);
            let a = std::f64::consts::FRAC_PI_2 + t;
            (Point::new(r * a.cos(), r * a.sin()), Vec2::new(a.cos(), a.sin()))
        })
        .chain((1..=steps).map(|k| {
            let t = std::f64::consts::PI * f64::from(k) / f64::from(steps);
            let a = std::f64::consts::FRAC_PI_2 - t;
            (Point::new(r * a.cos(), -12.0 + r * a.sin()), Vec2::new(-a.cos(), -a.sin()))
        }))
        .collect();
    let side = |sign: f64| middle.iter().map(move |(m, n)| (m.x + n.x * sign * width / 2.0, m.y + n.y * sign * width / 2.0));
    let s = polygon(side(1.0).chain(side(-1.0).rev()));
    let q = points(&sew(&s, p));
    // Each bowl away from its ends and from where the two meet.
    let bowl = 1.0;
    check("the S's upper bowl", &q, (0.0, 0.0), (-1.0, 0.0), bowl, r);
    check("the S's lower bowl", &q, (0.0, -12.0), (1.0, 0.0), bowl, r);

    // A straight stripe: both rails the spacing apart.
    let stripe = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, width), (0.0, width)]);
    let q = points(&sew(&stripe, p));
    for (name, top) in [("top", true), ("bottom", false)] {
        let rail: Vec<f64> = q.iter().filter(|a| if top { a.y < 0.0 } else { a.y > width }).map(|a| a.x).collect();
        let mut steps: Vec<f64> = rail.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        steps.sort_by(f64::total_cmp);
        let median = steps[steps.len() / 2];
        assert!((median - spacing).abs() < 1e-6, "the stripe's {name} rail {median} mm apart");
    }
}

/// A ring closes at any width a column may be, not only up to the width where flattening its
/// curves left a repeated point at each join that broke the side there (a ring of radius 10 used
/// to open from 3 mm wide, a quarter of it left to the tatami).
#[test]
fn rings_close_at_every_width() {
    for (r, w) in
        [(10.0, 2.0), (10.0, 3.0), (10.0, 4.0), (10.0, 5.0), (10.0, 6.0), (5.0, 2.0), (5.0, 3.0), (8.0, 5.0), (20.0, 6.0), (3.0, 2.0), (2.5, 1.5)]
    {
        let mut ring = circle((0.0, 0.0), r);
        ring.extend(hole((0.0, 0.0), r - w));
        let found = narrow_columns(&ring, FillRule::NonZero, WIDEST);
        assert_eq!(found.len(), 1, "r {r}, w {w}: {}", found.len());
        let round = std::f64::consts::TAU * (r - w / 2.0);
        assert!(found[0].closed && found[0].length() > 0.98 * round, "r {r}, w {w}: closed {}, {} of {round}", found[0].closed, found[0].length());
    }
}

/// An outline drawn with a point repeated (an anchor twice over) is one column all the same.
#[test]
fn a_repeated_point_does_not_break_a_column() {
    let mut pts: Vec<(f64, f64)> =
        (0..=60).map(|k| (std::f64::consts::TAU * k as f64 / 120.0).sin_cos()).map(|(s, c)| (20.0 * c, 20.0 * s)).collect();
    pts.extend((0..=60).rev().map(|k| (std::f64::consts::TAU * k as f64 / 120.0).sin_cos()).map(|(s, c)| (16.0 * c, 16.0 * s)));
    pts.insert(30, pts[30]);
    let found = narrow_columns(&polygon(pts), FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].length() > 55.0, "{}", found[0].length());
}

/// A narrow part whose side runs on along a wider block still finds its column: a 3 mm bar with
/// a block 7.5 to 10 mm tall at its end (wider than the limit, narrower than a column may run on
/// through), and a 3 mm pole holding a flag, are a 3 mm column each.
#[test]
fn a_narrow_part_beside_a_wider_block_is_a_column() {
    for b in [7.5, 8.0, 9.0, 10.0, 10.4] {
        let bar = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, b), (15.0, b), (15.0, 3.0), (0.0, 3.0)]);
        let found = narrow_columns(&bar, FillRule::NonZero, WIDEST);
        assert_eq!(found.len(), 1, "block {b}: {found:?}");
        let c = &found[0];
        assert!((c.width() - 3.0).abs() < 0.05 && c.length() > 13.0 && c.length() < 16.5, "block {b}: {} x {}", c.width(), c.length());
    }
    for flag in [
        polygon([(0.0, 0.0), (3.0, 0.0), (3.0, 12.0), (15.0, 12.0), (15.0, 20.0), (3.0, 20.0), (0.0, 20.0)]),
        polygon([(15.0, 0.0), (15.0, 20.0), (12.0, 20.0), (0.0, 20.0), (0.0, 12.0), (12.0, 12.0), (12.0, 0.0)]),
    ] {
        let found = narrow_columns(&flag, FillRule::NonZero, WIDEST);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!((found[0].width() - 3.0).abs() < 0.05, "{}", found[0].width());
    }
}

/// A column runs on through a waver about the limit, not across a wide part: two discs 10 mm
/// across joined by a 1 mm neck are a column along the neck, its stations within the limit,
/// the discs left to the tatami.
#[test]
fn a_column_does_not_fan_across_a_wide_part() {
    let a0 = (0.5f64 / 5.0).asin();
    let arc = |cx: f64, from: f64| {
        (0..=200).map(move |k| from - (std::f64::consts::TAU - 2.0 * a0) * k as f64 / 200.0).map(move |t| (cx + 5.0 * t.cos(), 5.0 * t.sin()))
    };
    let dumbbell = polygon(arc(9.0, std::f64::consts::PI - a0).chain(arc(-9.0, -a0)));
    let found = narrow_columns(&dumbbell, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{found:?}");
    let c = &found[0];
    let widest = c.stations.iter().map(|(a, b)| a.distance(*b)).fold(0.0, f64::max);
    assert!(widest <= WIDEST && (c.width() - 1.0).abs() < 0.05, "{} typically, {widest} at most", c.width());
    assert!(c.middles().iter().all(|m| m.x.abs() < 6.0), "{:?}", c.middles());
}

/// Push compensation shortens a column only at an end where the shape ends: a bar's free end is
/// sewn shorter, its end at the block it meets is not (the block's tatami is cut out up to it, so
/// a shorter column would leave a band sewn by neither).
#[test]
fn push_compensation_spares_the_end_that_meets_the_rest() {
    let bar = polygon([(0.0, 0.0), (30.0, 0.0), (30.0, 12.0), (15.0, 12.0), (15.0, 3.0), (0.0, 3.0)]);
    let found = narrow_columns(&bar, FillRule::NonZero, WIDEST);
    assert_eq!(found.len(), 1, "{found:?}");
    let c = &found[0];
    let at_free = c.middles().first().is_some_and(|m| m.x < 1.0);
    assert_eq!(c.tips, if at_free { [true, false] } else { [false, true] }, "{:?}", c.middles());
    let mut p = Params::for_style(Style::Satin);
    p.push = [1.0, 1.0];
    p.underlay = Underlay::None;
    // The column alone (the block's rest left out).
    let q = points(&fill_with_columns(&bar, FillRule::NonZero, &BezPath::new(), &found, &Params::for_style(Style::Fill), &p, Aim::from_needle(None)));
    let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| (lo.min(a.x), hi.max(a.x)));
    let end = c.middles().iter().fold(f64::NEG_INFINITY, |x, m| x.max(m.x));
    assert!(lo > 0.9 && hi > end - 0.05, "{lo} to {hi}, the column ending at {end}");
}
