//! Stitch planner property tests: for any closed shape (rectangles, ellipses, star-shaped polygons)
//! and any sane parameters, a fill or running line stays on the art, sews no stitch longer than it
//! allows, and cuts the thread exactly on the jumps longer than the trim length.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_geom::{BezPath, FillRule, Shape};
use vectorcraft_stitch::plan::{Params, Style, Underlay, fill_with_columns, narrow_columns, stitches};
use vectorcraft_stitch::{Command, Stitch};
use vectorcraft_testkit::strategies::arb_closed_shape;

/// Fill or running parameters across their useful range: stitch length, row spacing, angle,
/// underlay (every kind), jump length, how much longer the trim length is, and pull compensation at each end of
/// a row (fixed and by the row's length).
fn arb_params() -> impl Strategy<Value = Params> {
    (
        prop_oneof![Just(Style::Fill), Just(Style::Running)],
        1.0..6.0f64,
        0.25..1.5f64,
        0.0..360.0f64,
        prop::sample::select(&Underlay::ALL[..]),
        1.0..10.0f64,
        0.0..20.0f64,
        [0.0..1.0f64, 0.0..1.0f64],
        [0.0..10.0f64, 0.0..10.0f64],
    )
        .prop_map(|(style, stitch_length, density, angle, underlay, jump, longer, pull, pull_percent)| {
            let d = Params::for_style(style);
            // A running line has no compensation.
            let (pull, pull_percent) = if style == Style::Fill { (pull, pull_percent) } else { (d.pull, d.pull_percent) };
            Params { stitch_length, density, angle, underlay, jump, trim: jump + longer, pull, pull_percent, ..d }.sane()
        })
}

/// Each move between two needle points in a row: how far, whether the needle lifts on the way and
/// whether the thread is cut.
fn moves(out: &[Stitch]) -> Vec<(f64, bool, bool)> {
    let mut v = Vec::new();
    let mut last: Option<Stitch> = None;
    let (mut up, mut cut) = (false, false);
    for s in out {
        match s.command {
            Command::Stitch => {
                if let Some(l) = last {
                    v.push((l.distance(*s), up, cut));
                }
                (last, up, cut) = (Some(*s), false, false);
            }
            Command::Trim => cut = true,
            _ => up = true,
        }
    }
    v
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Every needle point lies within the art's bounds (a fill's rows reaching past them by their
    /// pull compensation at most: for a row as long as the art's diagonal), and is a point.
    #[test]
    fn stitches_stay_on_the_art(shape in arb_closed_shape(), p in arb_params()) {
        let path: BezPath = shape.to_bezpath();
        let bounds = path.bounding_box();
        let [start, end] = p.compensation(bounds.width().hypot(bounds.height()));
        let reach = start.max(end) + 1e-6;
        let b = bounds.inflate(reach, reach);
        for s in stitches(&path, FillRule::NonZero, &p) {
            prop_assert!(s.x.is_finite() && s.y.is_finite(), "{s:?}");
            prop_assert!(b.contains(s.point()), "{:?} outside {b:?} ({:?})", s.point(), p.style);
        }
    }

    /// No stitch is longer than the parameters allow: a row's stitch length (an end stitch up to
    /// the shortest stitch more), a line's, an edge run's or a travel's (up to 0.3 of a step more
    /// next to a corner or an end, where a stub moves onto it), the tatami underlay's 3 mm, or a
    /// move short enough to sew; and a stitch shorter than the shortest is left out, which
    /// lengthens the next by at most that much. Without pull compensation, so that the bound is
    /// as tight as the stitching's.
    #[test]
    fn no_stitch_is_too_long(shape in arb_closed_shape(), p in arb_params()) {
        let p = Params { pull: [0.0; 2], pull_percent: [0.0; 2], ..p };
        let out = stitches(&shape.to_bezpath(), FillRule::NonZero, &p);
        let longest = (p.stitch_length + p.min_stitch).max(1.3 * p.stitch_length).max(3.0 + p.min_stitch).max(2.6).max(p.jump) + p.min_stitch + 1e-6;
        for (d, up, _) in moves(&out) {
            prop_assert!(up || d <= longest, "a {d} mm stitch, longer than {longest} ({p:?})");
        }
    }

    /// With pull compensation, a stitch is longer by its reach at most: a row's end stitch, or a
    /// row with no stitch between its ends, by both ends' (the row as long as the art's diagonal).
    #[test]
    fn compensation_lengthens_stitches_by_its_reach_at_most(shape in arb_closed_shape(), p in arb_params()) {
        let path = shape.to_bezpath();
        let out = stitches(&path, FillRule::NonZero, &p);
        let bounds = path.bounding_box();
        let [start, end] = p.compensation(bounds.width().hypot(bounds.height()));
        let longest = (p.stitch_length + p.min_stitch + start + end).max(1.3 * p.stitch_length).max(3.0 + p.min_stitch).max(2.6).max(p.jump)
            + p.min_stitch
            + 1e-6;
        for (d, up, _) in moves(&out) {
            prop_assert!(up || d <= longest, "a {d} mm stitch, longer than {longest} ({p:?})");
        }
    }

    /// The needle lifts for a move only when it is not sewn, and the thread is cut exactly on the
    /// moves longer than the trim length.
    #[test]
    fn only_jumps_longer_than_the_trim_length_are_cut(shape in arb_closed_shape(), p in arb_params()) {
        for (d, up, cut) in moves(&stitches(&shape.to_bezpath(), FillRule::NonZero, &p)) {
            prop_assert_eq!(cut, up && d > p.trim, "a {} mm move, needle up {}, cut {} (trim {}, {:?})", d, up, cut, p.trim, p.style);
            prop_assert!(!up || d > p.min_stitch - 1e-9, "the needle lifts for a {} mm move", d);
        }
    }

    /// The narrow parts found in any shape lie inside it and are typically no wider than asked
    /// (half as wide again where one runs on); sewn with
    /// the shape (here its rest is the whole shape: the path operations that cut them out run in
    /// the engine), every needle point stays on the art, or past it by the compensation at most.
    #[test]
    fn narrow_columns_stay_on_the_art(shape in arb_closed_shape(), widest in 1.0..10.0f64) {
        let path: BezPath = shape.to_bezpath();
        let columns = narrow_columns(&path, FillRule::NonZero, widest);
        let bounds = path.bounding_box();
        let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
        for c in &columns {
            prop_assert!(c.width() <= widest + 1e-6, "a column {} wide", c.width());
            for (a, b) in &c.stations {
                prop_assert!(a.distance(*b) <= 1.5 * widest + 1e-6, "a station {} wide", a.distance(*b));
                prop_assert!(bounds.inflate(1e-6, 1e-6).contains(a.midpoint(*b)), "{:?} outside {bounds:?}", a.midpoint(*b));
            }
        }
        let reach = fill.compensation(bounds.width().hypot(bounds.height()))[0].max(satin.compensation(1.5 * widest)[0]) + 1e-6;
        let b = bounds.inflate(reach, reach);
        for s in fill_with_columns(&path, FillRule::NonZero, &path, &columns, &fill, &satin, None) {
            prop_assert!(s.x.is_finite() && s.y.is_finite() && b.contains(s.point()), "{:?} outside {b:?}", s.point());
        }
    }
}
