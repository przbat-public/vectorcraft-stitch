//! Stitch planner property tests: for any closed shape (rectangles, ellipses, star-shaped polygons)
//! and any sane parameters, a fill or running line stays on the art, sews no stitch longer than it
//! allows, and cuts the thread exactly on the jumps longer than the trim length; for any path (open
//! and closed subpaths, curves, crossings), rings and shapes apart, in every style from any start,
//! every point is a point and every stitch lies on the art; toward an exit point, any path in any
//! style from any start sews what it sews with none when the exit isn't a number, never lifts the
//! needle or cuts the thread more and never ends farther from the exit, and sews
//! nothing new when the exit lies where it ends anyway; so does a fill with narrow parts.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use vectorcraft_geom::{Affine, BezPath, FillRule, PathEl, Point, Shape};
use vectorcraft_stitch::plan::{
    Aim, Params, Start, Style, Underlay, narrow_columns, try_fill_with_columns, try_fill_with_columns_aimed, try_stitches, try_stitches_aimed,
    try_stitches_from,
};
use vectorcraft_stitch::{Command, Stitch};
use vectorcraft_testkit::strategies::{arb_closed_shape, arb_path_data, arb_point};

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
        for s in try_stitches(&path, FillRule::NonZero, &p).unwrap() {
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
        let out = try_stitches(&shape.to_bezpath(), FillRule::NonZero, &p).unwrap();
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
        let out = try_stitches(&path, FillRule::NonZero, &p).unwrap();
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
        for (d, up, cut) in moves(&try_stitches(&shape.to_bezpath(), FillRule::NonZero, &p).unwrap()) {
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
        for s in try_fill_with_columns(&path, FillRule::NonZero, &path, &columns, &fill, &satin, None).unwrap() {
            prop_assert!(s.x.is_finite() && s.y.is_finite() && b.contains(s.point()), "{:?} outside {b:?}", s.point());
        }
    }
}

/// How finely a path is flattened to measure distances to it (millimetres).
const FLATTEN: f64 = 0.05;
/// How far off the art a fill's stitch may lie past its rows' pull compensation (millimetres):
/// half the longest turn between two rows the planner sews straight across a concave corner
/// (`fill::SHAVED_CORNER`, 1 mm, measured before the rows' ends were stretched).
const SHAVED: f64 = 0.5;

/// The segments of `path` (each subpath closed, as a fill closes it), flattened.
fn segments(path: &BezPath) -> Vec<(Point, Point)> {
    let mut out = Vec::new();
    let (mut start, mut last): (Option<Point>, Option<Point>) = (None, None);
    let close = |out: &mut Vec<(Point, Point)>, start: Option<Point>, last: Option<Point>| {
        if let (Some(a), Some(b)) = (last, start) {
            out.push((a, b));
        }
    };
    vectorcraft_geom::kurbo::flatten(path.iter(), FLATTEN, |el| match el {
        PathEl::MoveTo(q) => {
            close(&mut out, start, last);
            (start, last) = (Some(q), Some(q));
        }
        PathEl::LineTo(q) => {
            if let Some(a) = last {
                out.push((a, q));
            }
            last = Some(q);
        }
        PathEl::ClosePath => {
            close(&mut out, start, last);
            last = start;
        }
        _ => {}
    });
    close(&mut out, start, last);
    out
}

/// How far `q` is from the nearest of `segments`.
fn distance(q: Point, segments: &[(Point, Point)]) -> f64 {
    segments
        .iter()
        .map(|&(a, b)| {
            let ab = b - a;
            let t = if ab.hypot2() > 0.0 { ((q - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
            q.distance(a + ab * t)
        })
        .fold(f64::INFINITY, f64::min)
}

/// A style, and a satin's width (millimetres).
fn arb_style() -> impl Strategy<Value = (Style, f64)> {
    (prop_oneof![Just(Style::Fill), Just(Style::Running), Just(Style::Satin)], 1.0..12.0f64)
}

/// `path` with every subpath closed, as a fill closes it (an open subpath's winding counts no
/// closing edge).
fn closed(path: &BezPath) -> BezPath {
    let mut out = BezPath::new();
    let mut open = false;
    for el in path.elements() {
        if matches!(el, PathEl::MoveTo(_)) && open {
            out.close_path();
        }
        out.push(*el);
        open = !matches!(el, PathEl::ClosePath);
    }
    if open {
        out.close_path();
    }
    out
}

/// Every needle-down stitch of `out` (a fill of `path`) lies on the art: its middle inside the
/// path, or within `reach` of its outline (a row's end stretched past it, a turn between two rows
/// shaving a corner, a line sewn along it).
fn on_the_art(path: &BezPath, out: &[Stitch], reach: f64) -> Result<(), String> {
    let segs = segments(path);
    let shape = closed(path);
    for w in out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch) {
        let mid = w[0].point().midpoint(w[1].point());
        if shape.winding(mid) == 0 && distance(mid, &segs) > reach {
            return Err(format!("a stitch from {:?} to {:?} off the art of {:?}", w[0].point(), w[1].point(), path.elements()));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// Any path, in any style, from any start: every point is a point; a fill's stitches lie on the
    /// art, a running line's needle points on its path, a satin's within its half width and pull
    /// compensation of its middle line.
    #[test]
    fn any_path_is_sewn_on_the_art(
        path in arb_path_data(),
        (style, width) in arb_style(),
        from in proptest::option::of(arb_point(-20.0, 220.0)),
        underlay in prop::sample::select(&Underlay::ALL[..]),
    ) {
        let bp = path.to_bezpath();
        let p = Params { width, underlay, ..Params::for_style(style) };
        let Ok(out) = try_stitches_from(&bp, FillRule::NonZero, &p, from) else { return Ok(()) };
        prop_assert!(out.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
        let segs = segments(&bp);
        let b = bp.bounding_box();
        match vectorcraft_stitch::plan::sewn_style(&p) {
            Style::Fill => {
                // A row reaches past the edge by its pull compensation (a row as long as the art's
                // diagonal), a turn or travel by a stitch's rounding.
                let [start, end] = p.compensation(b.width().hypot(b.height()));
                let reach = 2.0 * start.max(end) + SHAVED + FLATTEN;
                prop_assert!(on_the_art(&bp, &out, reach).is_ok(), "{:?}", on_the_art(&bp, &out, reach));
            }
            Style::Running => {
                // On the path, or (a lock stitch, a step along the first or last stitch) on a stitch
                // between needle points beside it: a chord, which a tight curve bows away from.
                let needle: Vec<Point> = out.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect();
                for (i, &q) in needle.iter().enumerate() {
                    let near = |j: usize, k: usize| j != i && k != i && distance(q, &[(needle[j], needle[k])]) <= FLATTEN + 0.1;
                    let around = i.saturating_sub(3)..(i + 4).min(needle.len());
                    let on_a_stitch = around.clone().any(|j| around.clone().any(|k| j < k && near(j, k)));
                    prop_assert!(distance(q, &segs) <= FLATTEN + 0.1 || on_a_stitch, "{:?} off the path {:?}", q, bp.elements());
                }
            }
            Style::Satin => {
                let [left, right] = p.compensation(p.width);
                let reach = p.width.min(vectorcraft_stitch::plan::MAX_SATIN_WIDTH) / 2.0 + left.max(right) + FLATTEN + 0.1;
                for s in out.iter().filter(|s| s.command == Command::Stitch) {
                    prop_assert!(distance(s.point(), &segs) <= reach, "{:?} farther than {reach} from the path {:?} ({p:?})", s.point(), bp.elements());
                }
            }
        }
    }

    /// A ring (a shape with a hole: its copy scaled down about its middle and wound the other way)
    /// and a shape with a copy far apart: a fill sews on the art, never across the hole or the gap.
    #[test]
    fn rings_and_shapes_apart_are_sewn_on_the_art(shape in arb_closed_shape(), angle in 0.0..180.0f64, apart in 30.0..80.0f64) {
        let outer = shape.to_bezpath();
        let middle = outer.bounding_box().center();
        let mut hole = outer.clone();
        hole.apply_affine(Affine::translate(middle.to_vec2()) * Affine::scale(0.4) * Affine::translate(-middle.to_vec2()));
        let hole = hole.reverse_subpaths();
        let mut ring = outer.clone();
        ring.extend(hole.iter());
        let mut two = outer.clone();
        let mut copy = outer.clone();
        copy.apply_affine(Affine::translate((outer.bounding_box().width() + apart, 0.0)));
        two.extend(copy.iter());
        let p = Params { angle, ..Params::for_style(Style::Fill) };
        let b = two.bounding_box();
        let [start, end] = p.compensation(b.width().hypot(b.height()));
        let reach = 2.0 * start.max(end) + SHAVED + FLATTEN;
        for art in [&ring, &two] {
            let out = try_stitches(art, FillRule::NonZero, &p).unwrap();
            prop_assert!(on_the_art(art, &out, reach).is_ok(), "{:?}", on_the_art(art, &out, reach));
        }
    }

    /// With the jump length no longer than the stitch length, no move short enough to sew is longer
    /// than a stitch may be: the bound no longer leans on the jump length.
    #[test]
    fn with_short_jumps_no_stitch_is_longer_than_a_stitch(shape in arb_closed_shape(), p in arb_params()) {
        let p = Params { pull: [0.0; 2], pull_percent: [0.0; 2], jump: p.stitch_length.min(p.jump), ..p };
        let out = try_stitches(&shape.to_bezpath(), FillRule::NonZero, &p).unwrap();
        let longest = (p.stitch_length + p.min_stitch).max(1.3 * p.stitch_length).max(3.0 + p.min_stitch).max(2.6) + p.min_stitch + 1e-6;
        for (d, up, _) in moves(&out) {
            prop_assert!(up || d <= longest, "a {d} mm stitch, longer than {longest} ({p:?})");
        }
    }
}

/// Where a shape starts from: nowhere yet, where the needle is, or an entry point, about the art.
fn arb_start() -> impl Strategy<Value = Start> {
    prop_oneof![Just(Start::Nowhere), arb_point(-20.0, 220.0).prop_map(Start::Needle), arb_point(-20.0, 220.0).prop_map(Start::Entry)]
}

/// What a plan toward an exit is judged by: where its needle ends, how often it lifts the needle
/// between two needle points, and how often it cuts the thread.
fn judged(out: &[Stitch]) -> (Option<Point>, usize, usize) {
    let m = moves(out);
    let end = out.iter().rev().find(|s| s.command == Command::Stitch).map(|s| s.point());
    (end, m.iter().filter(|(_, up, _)| *up).count(), m.iter().filter(|(_, _, cut)| *cut).count())
}

/// How much farther from its exit a plan toward it may end than the plain plan does
/// (millimetres): a float's rounding, nothing a needle could tell.
const NO_FARTHER: f64 = 1e-6;

/// `aimed` (planned toward `exit`) is no worse than `plain` (planned without it): no more lifts
/// or cuts, and an end no farther from the exit.
fn no_worse(plain: &[Stitch], aimed: &[Stitch], exit: Point) -> Result<(), String> {
    let ((plain_end, plain_lifts, plain_cuts), (aimed_end, aimed_lifts, aimed_cuts)) = (judged(plain), judged(aimed));
    if aimed_lifts > plain_lifts || aimed_cuts > plain_cuts {
        return Err(format!("{aimed_lifts} lifts and {aimed_cuts} cuts toward the exit, {plain_lifts} and {plain_cuts} without"));
    }
    match (plain_end, aimed_end) {
        (Some(a), Some(b)) if b.distance(exit) > a.distance(exit) + NO_FARTHER => {
            Err(format!("ends at {b:?}, {:.3} mm from the exit, {:.3} mm without it", b.distance(exit), a.distance(exit)))
        }
        (Some(_), None) => Err("sews nothing toward the exit".into()),
        _ => Ok(()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    /// An exit that isn't a number is no exit: a shape is sewn bit for bit as with none, from any
    /// start, and the entry points `try_stitches_from` and `try_stitches_aimed` agree. (That no
    /// exit sews what the planner sewed before exits were planned toward is `golden_no_exit.rs`'s
    /// check, against hashes recorded with the planner as it was.)
    #[test]
    fn an_exit_that_isnt_a_number_is_no_exit(
        path in arb_path_data(),
        (style, width) in arb_style(),
        start in arb_start(),
        underlay in prop::sample::select(&Underlay::ALL[..]),
    ) {
        let bp = path.to_bezpath();
        let p = Params { width, underlay, ..Params::for_style(style) };
        let plain = try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: None }).ok();
        if let Start::Needle(at) = start {
            prop_assert_eq!(&plain, &try_stitches_from(&bp, FillRule::NonZero, &p, Some(at)).ok());
        }
        for exit in [Point::new(f64::NAN, 40.0), Point::new(40.0, f64::INFINITY)] {
            let odd = try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: Some(exit) }).ok();
            prop_assert_eq!(&odd, &plain, "an exit at {:?}", exit);
        }
    }

    /// An exit anywhere never makes a plan worse: any path, in any style, from any start, lifts the
    /// needle and cuts the thread no more often, and ends no farther from the exit, than without it.
    #[test]
    fn an_exit_never_makes_a_plan_worse(
        path in arb_path_data(),
        (style, width) in arb_style(),
        start in arb_start(),
        exit in arb_point(-20.0, 220.0),
        underlay in prop::sample::select(&Underlay::ALL[..]),
    ) {
        let bp = path.to_bezpath();
        let p = Params { width, underlay, ..Params::for_style(style) };
        let (Ok(plain), Ok(aimed)) = (
            try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: None }),
            try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: Some(exit) }),
        ) else { return Ok(()) };
        prop_assert!(no_worse(&plain, &aimed, exit).is_ok(), "{:?} ({style:?}, {start:?}, exit {exit:?}, {:?})", no_worse(&plain, &aimed, exit), bp.elements());
    }

    /// An exit where the plan ends anyway changes nothing: the stitches are the plain plan's.
    #[test]
    fn an_exit_at_the_plans_own_end_changes_nothing(
        path in arb_path_data(),
        (style, width) in arb_style(),
        start in arb_start(),
        underlay in prop::sample::select(&Underlay::ALL[..]),
    ) {
        let bp = path.to_bezpath();
        let p = Params { width, underlay, ..Params::for_style(style) };
        let Ok(plain) = try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: None }) else { return Ok(()) };
        let Some(end) = judged(&plain).0 else { return Ok(()) };
        let aimed = try_stitches_aimed(&bp, FillRule::NonZero, &p, Aim { start, exit: Some(end) }).unwrap();
        prop_assert!(aimed == plain, "toward its own end {end:?} ({style:?}, {start:?}, {:?}): {} stitches, {} without", bp.elements(), aimed.len(), plain.len());
    }

    /// A fill with narrow parts sewn as satin, toward an exit anywhere: no worse than without it;
    /// toward its own end, the same stitches.
    #[test]
    fn a_fill_with_narrow_parts_is_no_worse_toward_an_exit(
        shape in arb_closed_shape(),
        widest in 1.0..10.0f64,
        start in arb_start(),
        exit in arb_point(-20.0, 220.0),
    ) {
        let path: BezPath = shape.to_bezpath();
        let columns = narrow_columns(&path, FillRule::NonZero, widest);
        let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
        let sew = |exit| try_fill_with_columns_aimed(&path, FillRule::NonZero, &path, &columns, &fill, &satin, Aim { start, exit }).unwrap();
        let plain = sew(None);
        let aimed = sew(Some(exit));
        prop_assert!(no_worse(&plain, &aimed, exit).is_ok(), "{:?} ({start:?}, exit {exit:?}, {} columns)", no_worse(&plain, &aimed, exit), columns.len());
        if let Some(end) = judged(&plain).0 {
            prop_assert!(sew(Some(end)) == plain, "toward its own end {end:?}");
        }
    }
}

/// A case the property above found: the last row of a region keeps its last stitch, and the way
/// on to the next region was judged from the stitch before it (2 mm back along the row) but sewn
/// from the row's end, so it ran up to a millimetre outside the copy's edge.
#[test]
fn the_way_on_from_a_regions_last_row_stays_on_the_art() {
    let corners = [
        (56.85166158337017, 95.10254807380309),
        (34.0100328904633, 80.4694424264317),
        (11.09327452415717, 89.49751773399532),
        (5.8612841733958945, 54.4861973869284),
        (35.50103571303082, 61.88913907865753),
        (23.223338743940822, 35.402036977685015),
        (39.41902244869237, 60.26538066676839),
        (42.345316438522374, 60.8156896921764),
        (45.0, 65.2315124697793),
    ];
    let (angle, apart) = (98.74448839765469, 35.48049976168461);
    let mut outer = BezPath::new();
    outer.move_to(corners[0]);
    for c in &corners[1..] {
        outer.line_to(*c);
    }
    outer.close_path();
    let mut two = outer.clone();
    let mut copy = outer.clone();
    copy.apply_affine(Affine::translate((outer.bounding_box().width() + apart, 0.0)));
    two.extend(copy.iter());
    let p = Params { angle, ..Params::for_style(Style::Fill) };
    let b = two.bounding_box();
    let [start, end] = p.compensation(b.width().hypot(b.height()));
    let reach = 2.0 * start.max(end) + SHAVED + FLATTEN;
    let out = try_stitches(&two, FillRule::NonZero, &p).unwrap();
    assert_eq!(on_the_art(&two, &out, reach), Ok(()));
}
