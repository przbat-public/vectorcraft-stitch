//! The stitch planner's tests: each style's shape, lengths, ties and underlay, fills with holes
//! and gaps, and odd input.

use vectorcraft_geom::{Point, Vec2};

use super::geometry::Axes;
use super::runs::LOCK;
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

fn points(v: &[Stitch]) -> Vec<Point> {
    v.iter().filter(|s| s.command == Command::Stitch).map(|s| s.point()).collect()
}

/// A satin without a width is sewn as a fill, and `sewn_style` says so: the stitches are the
/// fill's, and every other style is sewn as it is.
#[test]
fn a_satin_without_a_width_is_sewn_as_a_fill() {
    let shape = rect(0.0, 0.0, 20.0, 10.0);
    let unwidened = Params { width: 0.0, ..Params::for_style(Style::Satin) };
    assert_eq!(sewn_style(&unwidened), Style::Fill);
    assert_eq!(stitches(&shape, FillRule::NonZero, &unwidened), stitches(&shape, FillRule::NonZero, &Params { style: Style::Fill, ..unwidened }));
    assert_eq!(sewn_style(&Params { width: 3.0, ..unwidened }), Style::Satin);
    for style in [Style::Running, Style::Fill] {
        assert_eq!(sewn_style(&Params::for_style(style)), style);
    }
}

#[test]
fn a_running_line_steps_at_the_stitch_length() {
    let p = Params::for_style(Style::Running);
    let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
    let q = points(&out);
    assert!(q.len() >= 8, "{q:?}");
    assert!((q[0].x - 0.0).abs() < 1e-6 && (q[0].y).abs() < 1e-6);
    assert!((q.last().expect("last").x - 20.0).abs() < 1e-6, "{:?}", q.last());
    for w in q.windows(2) {
        let d = w[0].distance(w[1]);
        assert!(d <= p.stitch_length + 1e-6, "{d}");
    }
    // The lock stitches stay on the line, never past either end.
    assert!(q.iter().all(|q| q.y.abs() < 1e-9));
    assert!(q.iter().all(|q| q.x >= -1e-9 && q.x <= 20.0 + 1e-9), "{q:?}");
    // …and each is a back-and-forth at its end.
    assert!((q[1].x - LOCK).abs() < 1e-9 && (q[q.len() - 2].x - (20.0 - LOCK)).abs() < 1e-9, "{q:?}");
}

/// A closed running line is locked where it starts and ends too, so that a cut after it can't pull
/// it out: a square starts with a step forward and back, and ends with a step back and forward.
#[test]
fn a_closed_running_line_is_locked() {
    let mut square = BezPath::new();
    square.move_to((0.0, 0.0));
    square.line_to((20.0, 0.0));
    square.line_to((20.0, 20.0));
    square.line_to((0.0, 20.0));
    square.close_path();
    let q = points(&stitches(&square, FillRule::NonZero, &Params::for_style(Style::Running)));
    let (first, last) = (q[0], q[q.len() - 1]);
    assert!(first.distance(last) < 1e-9, "it ends where it starts: {q:?}");
    // Forward a lock step and back to the start; at the end back a lock step and forward again.
    assert!((q[1].distance(first) - LOCK).abs() < 1e-9 && q[2].distance(first) < 1e-9, "{q:?}");
    let n = q.len();
    assert!((q[n - 2].distance(last) - LOCK).abs() < 1e-9 && q[n - 3].distance(last) < 1e-9, "{q:?}");
    // Without lock, none.
    let loose = points(&stitches(&square, FillRule::NonZero, &Params { lock: false, ..Params::for_style(Style::Running) }));
    assert_eq!(loose.len(), n - 4);
}

#[test]
fn a_fill_covers_the_rectangle_and_no_stitch_is_longer_than_asked() {
    let p = Params::for_style(Style::Fill);
    let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
    let q = points(&out);
    assert!(q.len() > 100, "{}", q.len());
    // Inside the rectangle, rows reaching past its edges by the pull compensation at most.
    let reach = p.pull[0].max(p.pull[1]) + 1e-6;
    for a in &q {
        assert!(a.x >= -reach && a.x <= 20.0 + reach && a.y >= -reach && a.y <= 10.0 + reach, "{a:?}");
    }
    for w in out.windows(2) {
        // Neighbouring penetrations of one row (same y, needle down twice in a row) are at
        // most a stitch length apart (an end stitch at most the shortest stitch more); row
        // turns and travels are other stitches, and a jump is not a stitch at all.
        let (a, b) = (w[0], w[1]);
        if a.command == Command::Stitch && b.command == Command::Stitch && (a.y - b.y).abs() < 1e-6 {
            assert!(a.distance(b) <= p.stitch_length + p.min_stitch + 1e-6, "{:?} .. {:?}", a.point(), b.point());
        }
    }
    // About the area divided by the thread's coverage.
    let area = 20.0 * 10.0;
    let expected = area / (p.density * p.stitch_length);
    assert!((q.len() as f64) > expected * 0.5, "{} vs {expected}", q.len());
}

#[test]
fn a_hole_is_left_unstitched() {
    // A ring: the outer square wound one way, the inner square the other.
    let mut p = rect(0.0, 0.0, 30.0, 30.0);
    let mut hole = BezPath::new();
    hole.move_to((10.0, 10.0));
    hole.line_to((10.0, 20.0));
    hole.line_to((20.0, 20.0));
    hole.line_to((20.0, 10.0));
    hole.close_path();
    p.extend(hole);
    let params = Params::for_style(Style::Fill);
    let out = stitches(&p, FillRule::NonZero, &params);
    let q = points(&out);
    assert!(q.len() > 50);
    for a in &q {
        let in_hole = a.x > 10.0 + 0.2 && a.x < 20.0 - 0.2 && a.y > 10.0 + 0.2 && a.y < 20.0 - 0.2;
        assert!(!in_hole, "{a:?}");
    }
}

/// Sewn stitches (needle down at both ends) whose middle falls inside `hole`, deeper than
/// half a thread (a row turn may shave a hole's corner by less, as any tatami does).
fn over(out: &[Stitch], hole: (f64, f64, f64, f64)) -> usize {
    const DEPTH: f64 = 0.25;
    let (x0, y0, x1, y1) = hole;
    out.windows(2)
        .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
        .filter(|w| {
            let (x, y) = ((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            x > x0 + DEPTH && x < x1 - DEPTH && y > y0 + DEPTH && y < y1 - DEPTH
        })
        .count()
}

/// A ring is sewn region by region (above the hole, its two sides, below it), not a row at
/// a time: a handful of trims, not one per row, and no thread across the hole.
#[test]
fn a_ring_is_sewn_in_regions() {
    for hole in [2.0, 4.0, 10.0] {
        let a = 15.0 - hole / 2.0;
        let mut p = rect(0.0, 0.0, 30.0, 30.0);
        let mut h = BezPath::new();
        h.move_to((a, a));
        h.line_to((a, a + hole));
        h.line_to((a + hole, a + hole));
        h.line_to((a + hole, a));
        h.close_path();
        p.extend(h);
        for angle in [0.0, 45.0, 90.0] {
            let mut params = Params::for_style(Style::Fill);
            params.angle = angle;
            let out = stitches(&p, FillRule::NonZero, &params);
            let trims = out.iter().filter(|s| s.command == Command::Trim).count();
            // Four regions, and the underlay's move from the outer edge to the hole's.
            assert!(trims <= 6, "hole {hole}, angle {angle}: {trims} trims");
            assert_eq!(over(&out, (a, a, a + hole, a + hole)), 0, "hole {hole}, angle {angle}");
        }
    }
}

/// A running line round a square keeps its corners: every corner is a needle point.
#[test]
fn running_stitches_keep_sharp_corners() {
    let mut p = Params::for_style(Style::Running);
    p.stitch_length = 3.0;
    let out = stitches(&rect(0.0, 0.0, 10.0, 7.0), FillRule::NonZero, &p);
    for corner in [(10.0, 0.0), (10.0, 7.0), (0.0, 7.0)] {
        assert!(out.iter().any(|s| (s.x - corner.0).abs() < 1e-9 && (s.y - corner.1).abs() < 1e-9), "{corner:?}");
    }
    // No stub stitches at the corners.
    for w in out.windows(2) {
        assert!(w[0].distance(w[1]) > 0.3 * 3.0 - 1e-9 || w[0].distance(w[1]) < 1e-9, "{:?} {:?}", w[0], w[1]);
    }
}

/// A U whose arms are closer than the jump length: the thread never runs across the gap.
#[test]
fn a_narrow_gap_is_not_bridged() {
    let mut u = BezPath::new();
    for (x, y) in [(0.0, 0.0), (8.0, 0.0), (8.0, 20.0), (11.0, 20.0), (11.0, 0.0), (19.0, 0.0), (19.0, 30.0), (0.0, 30.0)] {
        if u.elements().is_empty() {
            u.move_to((x, y));
        } else {
            u.line_to((x, y));
        }
    }
    u.close_path();
    let mut params = Params::for_style(Style::Fill);
    params.angle = 0.0;
    let out = stitches(&u, FillRule::NonZero, &params);
    assert!(out.len() > 100);
    assert_eq!(over(&out, (8.0, -1.0, 11.0, 20.0)), 0, "a stitch crosses the gap");
}

/// Fill and satin tie the thread in where it starts and off before every cut and at the end:
/// a stitch back and forth on the spot.
#[test]
fn fills_and_satins_tie_in_and_off() {
    let mut two = rect(0.0, 0.0, 10.0, 10.0);
    two.extend(rect(60.0, 0.0, 10.0, 10.0));
    let mut satin = Params::for_style(Style::Satin);
    satin.width = 3.0;
    for (shape, p) in [(&two, Params::for_style(Style::Fill)), (&line((0.0, 0.0), (20.0, 0.0)), satin)] {
        let out = stitches(shape, FillRule::NonZero, &p);
        // Back and forth: the third stitch returns to the first.
        let returns = |w: &[Stitch]| w[0].command == Command::Stitch && w[2].command == Command::Stitch && w[0].distance(w[2]) < 1e-9;
        assert!(returns(&out[..3]), "tie-in: {:?}", &out[..3]);
        assert!(returns(&out[out.len() - 3..]), "tie-off: {:?}", &out[out.len() - 3..]);
        for (i, _) in out.iter().enumerate().filter(|(_, s)| s.command == Command::Trim) {
            assert!(i >= 3 && returns(&out[i - 3..i]), "tie-off before the trim at {i}");
            assert!(returns(&out[i + 2..i + 5]), "tie-in after the trim at {i}");
        }
        let mut loose = p;
        loose.lock = false;
        assert!(stitches(shape, FillRule::NonZero, &loose).len() < out.len());
    }
}

/// A satin column wider than the longest satin stitch is split: no stitch longer than that,
/// and the cuts of neighbouring crossings don't line up into a seam.
#[test]
fn wide_satins_are_split_without_a_seam() {
    let mut p = Params::for_style(Style::Satin);
    p.width = 20.0;
    p.pull = [0.0; 2];
    p.underlay = Underlay::None;
    p.lock = false;
    let out = stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &p);
    for w in out.windows(2) {
        assert!(w[0].distance(w[1]) <= p.max_satin + 1e-9, "{:?} {:?}", w[0], w[1]);
    }
    // The cuts inside the column (not on its rails) sit at many different heights.
    let mut inner: Vec<i64> = out.iter().filter(|s| s.y.abs() < 9.9).map(|s| (s.y * 10.0).round() as i64).collect();
    inner.sort_unstable();
    inner.dedup();
    assert!(inner.len() > 10, "{inner:?}");
    // A narrow column is not split at all.
    let mut narrow = p;
    narrow.width = 4.0;
    let out = stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &narrow);
    assert!(out.iter().all(|s| s.y.abs() > 1.9), "every penetration on a rail");
}

/// A ring's regions are joined by travel stitches inside it, not by trims, and each region
/// overlaps its neighbours by a row so no gap opens along the seams.
#[test]
fn regions_travel_inside_and_overlap() {
    let mut ring = rect(0.0, 0.0, 30.0, 30.0);
    let mut h = BezPath::new();
    h.move_to((10.0, 10.0));
    h.line_to((10.0, 20.0));
    h.line_to((20.0, 20.0));
    h.line_to((20.0, 10.0));
    h.close_path();
    ring.extend(h);
    for angle in [0.0, 30.0, 90.0] {
        let mut p = Params::for_style(Style::Fill);
        p.angle = angle;
        p.underlay = Underlay::None;
        let out = stitches(&ring, FillRule::NonZero, &p);
        let trims = out.iter().filter(|s| s.command == Command::Trim).count();
        assert!(trims <= 1, "angle {angle}: {trims} trims");
        assert_eq!(over(&out, (10.0, 10.0, 20.0, 20.0)), 0, "angle {angle}");
    }
    // Rows sewn twice where regions meet: at 0° the rows just above and below the hole.
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    p.underlay = Underlay::None;
    let out = stitches(&ring, FillRule::NonZero, &p);
    let plain = stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p);
    let count = |v: &[Stitch], y: f64| v.iter().filter(|s| s.command == Command::Stitch && (s.y - y).abs() < 1e-6).count();
    // The last row above the hole (rows centred across the ring, a spacing apart).
    let first = first_row(0.0, 30.0, p.density);
    let above = (0..1000).map(|k| first + p.density * f64::from(k)).take_while(|y| *y < 10.0).last().unwrap_or(0.0);
    assert!(count(&out, above) > count(&plain, above), "the row above the hole is sewn by two regions");
}

/// A satin's underlay follows its width — centre run, edge run, edge run and zigzag — and the
/// top stitching starts where the underlay ended: no trim and no long move along the column.
#[test]
fn satin_underlay_follows_the_width() {
    for (width, want_centre, want_zigzag) in [(1.5, true, false), (3.0, false, false), (6.0, false, true)] {
        let mut p = Params::for_style(Style::Satin);
        p.width = width;
        p.pull = [0.0; 2];
        let out = stitches(&line((0.0, 0.0), (40.0, 0.0)), FillRule::NonZero, &p);
        assert!(out.iter().all(|s| s.command == Command::Stitch), "{width} mm: no trim or jump");
        for w in out.windows(2) {
            assert!(w[0].distance(w[1]) <= p.max_satin.max(UNDERLAY_STITCH) + 1e-6, "{width} mm: {:?} {:?}", w[0], w[1]);
        }
        let side = width / 2.0 - (width / 4.0).min(0.5);
        let on = |y: f64| out.iter().any(|s| (s.y - y).abs() < 1e-6 && s.x > 5.0 && s.x < 35.0);
        assert_eq!(on(0.0), want_centre, "{width} mm: a centre run");
        assert_eq!(on(side) && on(-side), !want_centre, "{width} mm: an edge run");
        // A zigzag underlay: stitches from one side to the other, 2 mm along.
        let crossings =
            out.windows(2).filter(|w| (w[0].y - side).abs() < 1e-6 && (w[1].y + side).abs() < 1e-6 && (w[1].x - w[0].x).abs() > 1.0).count();
        assert_eq!(crossings > 3, want_zigzag, "{width} mm: a zigzag");
    }
}

/// A wide column's zigzag underlay runs at an angle, longer than the column is wide: it is split
/// like the top stitching, so no underlay stitch is longer than the longest satin stitch.
#[test]
fn a_wide_zigzag_underlay_is_split() {
    let mut p = Params::for_style(Style::Satin);
    p.width = 14.0;
    p.pull = [0.0; 2];
    p.lock = false;
    let out = stitches(&line((0.0, 0.0), (40.0, 0.0)), FillRule::NonZero, &p);
    let longest = out.windows(2).map(|w| w[0].distance(w[1])).fold(0.0, f64::max);
    assert!(longest <= p.max_satin + 1e-9, "a {longest:.1} mm stitch");
    // The zigzag is still there: penetrations inside the column, between its sides.
    let side = 14.0 / 2.0 - 0.5;
    assert!(out.iter().any(|s| s.y.abs() < side - 0.5 && s.x > 5.0 && s.x < 35.0));
}

/// A large fill gets a second, sparse underlay across its rows; a small one doesn't.
#[test]
fn large_fills_get_a_tatami_underlay() {
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    // Rows of the underlay run at 90°: vertical stitches, 3 mm long, inside the edge.
    // (Away from the shape's left and right edges, where the edge run, half a spacing in, is
    // vertical too.)
    let vertical = |out: &[Stitch], width: f64| {
        out.windows(2)
            .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
            .filter(|w| (w[0].x - w[1].x).abs() < 1e-6 && (w[0].y - w[1].y).abs() > 1.0 && w[0].x > 0.5 && w[0].x < width - 0.5)
            .count()
    };
    assert!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0) > 20);
    assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 8.0, 30.0), FillRule::NonZero, &p), 8.0), 0);
    p.underlay = Underlay::None;
    assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0), 0);
}

/// On a tight curve the penetrations on a satin's inner side don't pile up: stitches there are
/// shortened (a 5 mm column round a 3 mm radius put 16 of its 18 inner penetrations within
/// 0.15 mm of the one before), while the outer side and a straight column keep their full width.
#[test]
fn tight_satin_curves_get_short_stitches() {
    let (r, width) = (3.0, 5.0);
    let mut p = Params::for_style(Style::Satin);
    p.width = width;
    p.underlay = Underlay::None;
    p.lock = false;
    let half = width / 2.0 + p.pull[0];
    let arc = vectorcraft_geom::kurbo::Arc::new((0.0, 0.0), (r, r), 0.0, 1.5 * std::f64::consts::PI, 0.0);
    let path: BezPath = vectorcraft_geom::Shape::path_elements(&arc, 0.01).collect();
    let q = points(&stitches(&path, FillRule::NonZero, &p));
    let radius = |a: &Point| a.to_vec2().hypot();
    let inner: Vec<Point> = q.iter().copied().filter(|a| radius(a) < r).collect();
    let outer: Vec<Point> = q.iter().copied().filter(|a| radius(a) >= r).collect();
    let bunched = (1..inner.len()).filter(|&i| inner[i.saturating_sub(2)..i].iter().any(|b| b.distance(inner[i]) < 0.15)).count();
    eprintln!("{bunched} of {} inner penetrations within 0.15 mm of one of the two before", inner.len());
    assert!(inner.len() >= 15, "{}", inner.len());
    assert!(bunched * 4 <= inner.len(), "{bunched} of {} inner penetrations bunched", inner.len());
    // Shortened stitches stay on the inner half; the outer rail keeps its width (within the arc's
    // flattening).
    assert!(inner.iter().all(|a| radius(a) >= r - half - 0.05), "{inner:?}");
    assert!(outer.iter().all(|a| (radius(a) - (r + half)).abs() < 0.05), "{outer:?}");
    // Along a straight column nothing is shortened.
    let straight = points(&stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p));
    assert!(straight.iter().all(|a| (a.y.abs() - half).abs() < 1e-6), "{straight:?}");
}

/// Satin spacing follows the column's width unless a density is given.
#[test]
fn satin_spacing_follows_the_width() {
    // The spacing is the distance between two penetrations on the same rail (peak to peak).
    let rail_gap = |width: f64, auto: bool| {
        let mut p = Params::for_style(Style::Satin);
        p.width = width;
        p.pull = [0.0; 2];
        p.underlay = Underlay::None;
        p.lock = false;
        p.auto_spacing = auto;
        p.density = 0.3;
        let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
        let top: Vec<f64> = out.iter().filter(|s| (s.y - width / 2.0).abs() < 1e-6).map(|s| s.x).collect();
        top[2] - top[1]
    };
    assert!((rail_gap(1.5, true) - 0.5).abs() < 0.02);
    assert!((rail_gap(5.0, true) - 0.4).abs() < 0.02);
    assert!(rail_gap(1.5, true) > rail_gap(8.0, true));
    assert!((rail_gap(5.0, false) - 0.3).abs() < 0.02, "a given density is kept");
}

/// A shape drawn without a final close (common in SVG from drawing apps) is still filled, as
/// paint fills it; a line with no area is sewn as a line.
#[test]
fn an_unclosed_shape_is_filled() {
    let mut open = BezPath::new();
    open.move_to((0.0, 0.0));
    open.line_to((20.0, 0.0));
    open.line_to((20.0, 10.0));
    open.line_to((0.0, 10.0));
    let p = Params::for_style(Style::Fill);
    let filled = stitches(&open, FillRule::NonZero, &p);
    let closed = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
    assert_eq!(filled.len(), closed.len());
    let line = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
    assert!(!line.is_empty() && line.len() < 20, "{}", line.len());
}

/// No sewn stitch is shorter than the shortest stitch asked for, in any style: tiny stitches
/// break the thread or cut the fabric, and professional software removes them.
#[test]
fn no_stitch_is_shorter_than_the_shortest() {
    let mut ring = rect(0.0, 0.0, 30.0, 20.0);
    ring.extend(rect(10.0, 5.0, 4.0, 4.0));
    let mut satin = Params::for_style(Style::Satin);
    satin.width = 6.0;
    for (shape, p) in [(&ring, Params::for_style(Style::Fill)), (&ring, satin), (&ring, Params::for_style(Style::Running))] {
        let out = stitches(shape, FillRule::EvenOdd, &p);
        for w in out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch) {
            let d = w[0].distance(w[1]);
            assert!(d >= p.min_stitch - 1e-9 || d < 1e-9, "{:?}: a {d:.2} mm stitch", p.style);
        }
    }
}

/// Separate pieces of one shape (seeds, letters): the needle jumps between pieces closer than the
/// jump length with the thread on; a longer jump is tied off before and in after, so that the
/// machine or a person can cut it, and the thread is cut only before a jump longer than the trim
/// length. It never sews across.
#[test]
fn separate_pieces_jump_and_trim_only_when_far() {
    let mut p = Params::for_style(Style::Fill);
    p.underlay = Underlay::None;
    // Vertical rows: the first square ends at its right edge, right by the second.
    p.angle = 90.0;
    let count = |gap: f64| {
        let mut two = rect(0.0, 0.0, 10.0, 10.0);
        two.extend(rect(10.0 + gap, 0.0, 10.0, 10.0));
        let out = stitches(&two, FillRule::NonZero, &p);
        assert_eq!(over(&out, (10.0, -1.0, 10.0 + gap, 11.0)), 0, "{gap} mm: a stitch across the gap");
        let trims = out.iter().filter(|s| s.command == Command::Trim).count();
        let jump = out.iter().position(|s| s.command == Command::Jump).expect("a jump");
        // Back and forth on the spot before the jump (and its trim) and after it.
        let before = out[..jump].iter().rposition(|s| s.command == Command::Stitch).unwrap();
        let tied = |w: &[Stitch]| w.iter().all(|s| s.command == Command::Stitch) && w[0].distance(w[2]) < 1e-9;
        (trims, tied(&out[before - 2..=before]) && tied(&out[jump + 1..jump + 4]))
    };
    assert_eq!(count(2.0), (0, false), "a 2 mm gap: a jump with the thread on");
    assert_eq!(count(p.trim - 3.0), (0, true), "a gap shorter than the trim length: a tied jump, not cut");
    assert_eq!(count(p.trim + 5.0), (1, true), "a gap longer than the trim length is cut");
}

/// A concave shape (a letter E): between its arms and spine the needle travels inside the shape,
/// never sewing across the gaps, at any angle (every 5° tried), underlay or not; and it is sewn in
/// one go, without a jump or a cut: the regions are ordered so that travels pass under stitching
/// still to come (with vertical rows, the arms first, each towards the spine, and the spine last).
#[test]
fn a_concave_shape_is_sewn_in_one_go_never_across_its_gaps() {
    let mut e = BezPath::new();
    let pts = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 8.0),
        (10.0, 8.0),
        (10.0, 16.0),
        (30.0, 16.0),
        (30.0, 24.0),
        (10.0, 24.0),
        (10.0, 32.0),
        (40.0, 32.0),
        (40.0, 40.0),
        (0.0, 40.0),
    ];
    for (i, q) in pts.iter().enumerate() {
        if i == 0 {
            e.move_to(*q);
        } else {
            e.line_to(*q);
        }
    }
    e.close_path();
    for underlay in [Underlay::None, Underlay::Auto] {
        for angle in (0..360).step_by(5).map(f64::from) {
            let mut p = Params::for_style(Style::Fill);
            p.angle = angle;
            p.underlay = underlay;
            let out = stitches(&e, FillRule::NonZero, &p);
            assert_eq!(
                over(&out, (10.0, 8.0, 40.0, 16.0)) + over(&out, (10.0, 24.0, 40.0, 32.0)),
                0,
                "angle {angle}, underlay {underlay:?}: across a gap"
            );
            let lifts = out.iter().filter(|s| s.command != Command::Stitch).count();
            assert_eq!(lifts, 0, "angle {angle}, underlay {underlay:?}: {lifts} jumps and trims");
        }
    }
}

#[test]
fn the_angle_turns_the_rows() {
    let mut p = Params::for_style(Style::Fill);
    p.angle = 90.0;
    let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
    let q = points(&out);
    // Rows run vertically: each row keeps one x, rows step by the density, and a row spans
    // the shape's height.
    let mut xs: Vec<f64> = q.iter().map(|a| a.x).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert!(xs.len() >= 10, "{xs:?}");
    assert!((xs[1] - xs[0]).abs() - p.density < 0.2, "{:?}", &xs[..2]);
    assert!(q.iter().any(|a| a.y > 9.0), "a vertical row spans the height");
}

#[test]
fn a_satin_column_crosses_its_width() {
    let p = Params::for_style(Style::Satin);
    let mut params = p;
    params.width = 4.0;
    params.pull = [0.0; 2];
    let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &params);
    let q = points(&out);
    assert!(q.len() > 20, "{}", q.len());
    assert!(q.iter().all(|a| a.y.abs() <= 2.0 + 1e-6), "{q:?}");
    // Neighbouring stitches are on opposite rails.
    let rails: Vec<f64> = q.iter().skip(1).map(|a| a.y).collect();
    assert!(rails.iter().any(|y| *y > 1.5) && rails.iter().any(|y| *y < -1.5));
    let mut wider = params;
    wider.pull = [0.5; 2];
    let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &wider);
    let w = points(&out);
    assert!(w.iter().any(|a| a.y.abs() > 2.2), "pull compensation widens the column");
}

#[test]
fn shapes_far_apart_are_trimmed() {
    let mut p = rect(0.0, 0.0, 10.0, 10.0);
    p.extend(rect(60.0, 0.0, 10.0, 10.0));
    let params = Params::for_style(Style::Fill);
    let out = stitches(&p, FillRule::NonZero, &params);
    assert!(out.iter().any(|s| s.command == Command::Trim), "a long travel cuts the thread");
}

/// A shape that would need millions of stitches is refused before any is planned.
#[test]
fn an_absurd_plan_is_refused_up_front() {
    let mut p = Params::for_style(Style::Fill);
    p.density = 0.05;
    p.stitch_length = 0.2;
    // Five metres square.
    let huge = rect(0.0, 0.0, 5000.0, 5000.0);
    assert!(matches!(try_stitches(&huge, FillRule::NonZero, &p), Err(Error::TooLarge(_))));
    assert!(try_stitches(&rect(0.0, 0.0, 50.0, 50.0), FillRule::NonZero, &p).is_ok_and(|s| !s.is_empty()));
    let mut run = Params::for_style(Style::Running);
    run.stitch_length = 0.2;
    assert!(matches!(try_stitches(&line((0.0, 0.0), (1e6, 0.0)), FillRule::NonZero, &run), Err(Error::TooLarge(_))));
}

/// The estimate counts what a shape really costs, so that it refuses before planning: a comb of
/// thousands of teeth whose every row crosses every tooth, a satin wide enough that each crossing
/// is split, and a fill of more rows than a layer may have, each refused (quickly, from the edges
/// alone), while a comb of a few teeth is planned.
#[test]
fn the_estimate_is_an_upper_bound() {
    /// A comb of `teeth` teeth 0.81 mm wide and 100 mm long, 0.05 mm apart, on a 2 mm back.
    fn comb(teeth: usize) -> BezPath {
        let mut pts = vec![(0.0, 102.0)];
        for k in 0..teeth {
            let x = k as f64 * 0.86;
            pts.extend([(x, 0.0), (x + 0.81, 0.0), (x + 0.81, 100.0), (x + 0.86, 100.0)]);
        }
        pts.push((teeth as f64 * 0.86, 102.0));
        polygon(pts)
    }
    let mut fill = Params::for_style(Style::Fill);
    (fill.density, fill.stitch_length, fill.angle) = (0.05, 12.0, 0.0);
    let started = std::time::Instant::now();
    assert!(matches!(try_stitches(&comb(15_900), FillRule::NonZero, &fill), Err(Error::TooLarge(_))));
    assert!(try_stitches(&comb(5), FillRule::NonZero, &Params::for_style(Style::Fill)).is_ok_and(|s| !s.is_empty()));
    // Six metres tall: 120,000 rows at 0.05 mm, more than a layer may have.
    assert!(matches!(try_stitches(&rect(0.0, 0.0, 3.0, 6000.0), FillRule::NonZero, &fill), Err(Error::TooLarge(_))));
    let mut satin = Params::for_style(Style::Satin);
    (satin.width, satin.density, satin.auto_spacing) = (50.0, 0.05, false);
    assert!(matches!(try_stitches(&line((0.0, 0.0), (99_000.0, 0.0)), FillRule::NonZero, &satin), Err(Error::TooLarge(_))));
    assert!(started.elapsed().as_secs() < 5, "refused from the edges alone");
}

/// A shape whose control points lie far beyond the document's range, or aren't numbers, is
/// refused before it is flattened (a curve's flattened points grow with its size, without bound),
/// and has no narrow parts to look for.
#[test]
fn a_shape_out_of_range_is_refused_before_it_is_flattened() {
    let mut far = BezPath::new();
    far.move_to((0.0, 0.0));
    far.curve_to((1e16, 0.0), (1e16, 1e16), (0.0, 10.0));
    far.close_path();
    let mut nan = rect(0.0, 0.0, 10.0, 10.0);
    nan.line_to((f64::NAN, 5.0));
    for style in [Style::Fill, Style::Satin, Style::Running] {
        for shape in [&far, &nan] {
            assert!(matches!(try_stitches(shape, FillRule::NonZero, &Params::for_style(style)), Err(Error::TooLarge(_))), "{style:?}");
        }
    }
    assert_eq!(narrow_columns(&far, FillRule::NonZero, NARROW_SATIN), Vec::new());
    assert!(try_stitches(&rect(MAX_COORD_MM - 20.0, 0.0, 10.0, 10.0), FillRule::NonZero, &Params::for_style(Style::Fill)).is_ok());
}

/// A row far from the origin, where a step no longer moves a coordinate, ends: its loop counts.
#[test]
fn a_row_far_away_ends() {
    let p = Params::for_style(Style::Fill).sane();
    for at in [3e15, 1e16, 7.2e16] {
        let points = super::fill::row_points(at, at + 50.0, 0.0, &p);
        assert!(points.len() <= 50, "{at}: {} points", points.len());
    }
}

#[test]
fn nothing_panics_on_odd_input() {
    let empty = BezPath::new();
    assert!(stitches(&empty, FillRule::NonZero, &Params::default()).is_empty());
    let mut p = Params::for_style(Style::Fill);
    p.density = f64::NAN;
    p.stitch_length = 0.0;
    p.angle = f64::INFINITY;
    p.pull = [-5.0; 2];
    p.jump = f64::NAN;
    let out = stitches(&rect(0.0, 0.0, 10.0, 10.0), FillRule::NonZero, &p);
    assert!(!out.is_empty());
    assert!(out.iter().all(|s| s.x.is_finite() && s.y.is_finite()));
    // A zero-length path has no stitches, and a single-point path neither.
    assert!(stitches(&line((1.0, 1.0), (1.0, 1.0)), FillRule::NonZero, &Params::default()).is_empty());
}

/// A closed polygon through `pts`.
fn polygon(pts: impl IntoIterator<Item = (f64, f64)>) -> BezPath {
    let mut g = BezPath::new();
    for (k, q) in pts.into_iter().enumerate() {
        if k == 0 { g.move_to(q) } else { g.line_to(q) }
    }
    g.close_path();
    g
}

/// The outlines' grid answers what testing every edge answers: whether a point is inside and
/// whether a segment stays inside, for a jagged shape with holes, a star of long spikes and a
/// zigzag whose edges cross the whole shape.
#[test]
fn the_edge_grid_agrees_with_every_edge() {
    use super::area::{Area, Grid};
    use super::geometry::subpaths;
    let tau = std::f64::consts::TAU;
    let mut jagged = polygon((0..120).map(|k| {
        let a = k as f64 / 120.0 * tau;
        let r = if (k / 2) % 2 == 0 { 30.0 } else { 26.0 };
        (40.0 + r * a.cos(), 40.0 + r * a.sin())
    }));
    jagged.extend(rect(30.0, 30.0, 8.0, 20.0));
    let star = polygon((0..48).map(|k| {
        let a = k as f64 / 48.0 * tau;
        let r = if k % 2 == 0 { 40.0 } else { 3.0 };
        (40.0 + r * a.cos(), 40.0 + r * a.sin())
    }));
    let zigzag = polygon((0..40).map(|k| if k % 2 == 0 { (k as f64, 0.0) } else { (80.0 - k as f64, 80.0) }));
    for shape in [jagged, star, zigzag] {
        let subs = subpaths(&shape);
        let outlines: Vec<&super::geometry::SubPath> = subs.iter().collect();
        let grid = Grid::new(&outlines).expect("a grid");
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let plain = Area { outlines: &outlines, rule, grid: None };
            let fast = Area { grid: Some(&grid), ..plain };
            // A fixed scatter of points (a linear congruential sequence), some exactly on corners.
            let mut seed = 12345u64;
            let mut next = || {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (seed >> 11) as f64 / (1u64 << 53) as f64 * 90.0 - 5.0
            };
            let mut pts: Vec<Point> = (0..400).map(|_| Point::new(next(), next())).collect();
            pts.extend(subs.iter().flat_map(|s| s.pts.iter().copied()));
            for (i, &a) in pts.iter().enumerate() {
                assert_eq!(plain.contains(a), fast.contains(a), "{a:?}");
                let b = pts[(i * 7 + 3) % pts.len()];
                assert_eq!(plain.contains_segment(a, b), fast.contains_segment(a, b), "{a:?} to {b:?}");
            }
        }
    }
}

/// The grid's memory stays near the number of edges, however long they are: edges from corner to
/// corner of a shape (as traced art can have) would fill every cell of their boxes. A few such
/// edges are listed only in the cells they pass; thousands would still take too many, so the shape
/// keeps no grid and tests every edge.
#[test]
fn the_edge_grid_stays_small_for_long_edges() {
    use super::area::Grid;
    use super::geometry::subpaths;
    let zigzag = |n: usize| {
        polygon((0..n).map(|k| {
            let t = k as f64 / n as f64;
            if k % 2 == 0 { (t, 0.0) } else { (100.0 - t, 100.0) }
        }))
    };
    let few = zigzag(200);
    let subs = subpaths(&few);
    let outlines: Vec<&super::geometry::SubPath> = subs.iter().collect();
    let grid = Grid::new(&outlines).expect("a grid");
    // An 11 × 11 grid: each edge's box is every cell, the cells it passes about two a row.
    assert!(grid.entries() <= 200 * 32, "{} entries", grid.entries());
    for n in [2_000usize, 20_000] {
        let many = zigzag(n);
        let subs = subpaths(&many);
        let outlines: Vec<&super::geometry::SubPath> = subs.iter().collect();
        assert!(Grid::new(&outlines).is_none(), "{n} edges from corner to corner keep a grid");
    }
    // A shape of short edges keeps its grid, each edge in a cell or two.
    let circle = polygon((0..2_000).map(|k| {
        let a = k as f64 / 2_000.0 * std::f64::consts::TAU;
        (50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin())
    }));
    let subs = subpaths(&circle);
    let outlines: Vec<&super::geometry::SubPath> = subs.iter().collect();
    let grid = Grid::new(&outlines).expect("a grid");
    assert!(grid.entries() <= 2_000 * 4, "{} entries", grid.entries());
}

/// With nothing sewn before, a line's first subpath is sewn as drawn and the next one from its end
/// nearer the needle: a second line drawn away from the first is sewn towards it, not jumped to
/// its far end.
#[test]
fn with_no_needle_the_first_line_is_sewn_as_drawn_and_the_next_from_near_it() {
    let mut path = line((0.0, 0.0), (10.0, 0.0));
    path.extend(line((100.0, 0.0), (90.0, 0.0)));
    let q = points(&stitches_from(&path, FillRule::NonZero, &Params::for_style(Style::Running), None));
    assert!(q[0].x.abs() < 1e-6, "{:?}", q[0]);
    let second = q.iter().find(|p| p.x > 50.0).expect("the second line");
    assert!((second.x - 90.0).abs() < 1e-6, "{second:?}");
}

/// Where the needle comes from decides where a shape starts: a satin column or a running line
/// from its nearer end, a fill from its corner nearest the needle.
#[test]
fn a_shape_starts_nearest_the_needle() {
    let from = Point::new(30.0, 1.0);
    let mut satin = Params::for_style(Style::Satin);
    satin.width = 3.0;
    for p in [satin, Params::for_style(Style::Running)] {
        let out = stitches_from(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &p, Some(from));
        let first = points(&out)[0];
        assert!(first.distance(from) < 3.0, "{:?}: starts at {first:?}", p.style);
        let drawn = points(&stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &p))[0];
        assert!(drawn.x < 3.0, "{:?}: without a needle, from where it is drawn: {drawn:?}", p.style);
    }
    let out = stitches_from(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &Params::for_style(Style::Fill), Some(Point::new(25.0, 12.0)));
    let first = points(&out)[0];
    assert!(first.distance(Point::new(20.0, 10.0)) < 1.5, "a fill starts at {first:?}");
    // A needle point that is not a point is ignored.
    assert_eq!(
        stitches_from(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &Params::for_style(Style::Fill), Some(Point::new(f64::NAN, 0.0))),
        stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &Params::for_style(Style::Fill))
    );
}

/// A jagged shape of many regions (a gear of 90 teeth with 24 holes) still travels inside: its
/// travel graph keeps only the reflex corners, small enough to search, so the teeth and the
/// stretches between the holes are joined by travels, not jumps.
#[test]
fn a_jagged_shape_travels_inside() {
    let mut g = BezPath::new();
    for k in 0..360 {
        let a = k as f64 / 360.0 * std::f64::consts::TAU;
        let r = if (k / 2) % 2 == 0 { 30.0 } else { 26.0 };
        let q = (40.0 + r * a.cos(), 40.0 + r * a.sin());
        if k == 0 { g.move_to(q) } else { g.line_to(q) }
    }
    g.close_path();
    for h in 0..24 {
        let a = h as f64 / 24.0 * std::f64::consts::TAU;
        let (cx, cy) = (40.0 + 18.0 * a.cos(), 40.0 + 18.0 * a.sin());
        for k in 0..16 {
            let b = -(k as f64) / 16.0 * std::f64::consts::TAU;
            let q = (cx + 1.8 * b.cos(), cy + 1.8 * b.sin());
            if k == 0 { g.move_to(q) } else { g.line_to(q) }
        }
        g.close_path();
    }
    for angle in [0.0, 45.0, 90.0] {
        let mut p = Params::for_style(Style::Fill);
        p.angle = angle;
        let out = stitches(&g, FillRule::NonZero, &p);
        let lifts = out.iter().filter(|s| s.command == Command::Jump).count();
        assert!(lifts <= 6, "angle {angle}: {lifts} jumps");
    }
}

/// Moves between the runs of a running line, at the exact limits: a gap of exactly
/// [`Params::jump`] is still sewn as one stitch and a hair more lifts the needle; a jump of exactly
/// [`Params::trim`] keeps the thread on and a hair more cuts it.
#[test]
fn moves_at_exactly_the_jump_and_trim_lengths() {
    let p = Params::for_style(Style::Running);
    let moves = |gap: f64| {
        let mut path = line((0.0, 0.0), (10.0, 0.0));
        path.extend(line((10.0 + gap, 0.0), (20.0 + gap, 0.0)));
        let out = stitches(&path, FillRule::NonZero, &p);
        let count = |c: Command| out.iter().filter(|s| s.command == c).count();
        (count(Command::Jump), count(Command::Trim))
    };
    assert_eq!(moves(p.jump), (0, 0), "a gap of exactly the jump length is sewn");
    assert_eq!(moves(p.jump + 0.01), (1, 0), "a longer one jumps");
    assert_eq!(moves(p.trim), (1, 0), "a jump of exactly the trim length is not cut");
    assert_eq!(moves(p.trim + 0.01), (1, 1), "a longer one is");
}

/// Inside a fill, a straight move of exactly [`Params::jump`] is the next stitch; a longer one
/// travels in short stitches.
#[test]
fn a_move_inside_of_exactly_the_jump_length_is_one_stitch() {
    use super::area::{Area, Travels};
    use super::geometry::{SubPath, subpaths};
    use super::runs::{Runs, Travel};
    let subs = subpaths(&rect(0.0, 0.0, 30.0, 10.0));
    let outlines: Vec<&SubPath> = subs.iter().collect();
    let mut travels = Travels::new(Area { outlines: &outlines, rule: FillRule::NonZero, grid: None });
    let p = Params::for_style(Style::Fill).sane();
    let mut runs = Runs::from(None);
    runs.push(vec![Point::new(1.0, 5.0)], Travel::Sew);
    let here = Point::new(1.0, 5.0);
    let to = here + Vec2::new(p.jump, 0.0);
    assert_eq!(runs.approach(&mut travels, to, to, f64::INFINITY, &|_| 0.0, &p), Travel::Auto);
    assert_eq!(runs.here(), Some(here), "no travel run added");
    let to = here + Vec2::new(p.jump + 0.01, 0.0);
    assert_eq!(runs.approach(&mut travels, to, to, f64::INFINITY, &|_| 0.0, &p), Travel::Sew);
    assert_eq!(runs.here(), Some(to), "a travel run to it");
}

/// A travel round a corner passes it half a millimetre inside the shape ([`area`]'s margin): off
/// the edge, where the fill covers it, and no farther from the shortest way than that.
#[test]
fn a_travel_passes_a_corner_just_inside() {
    use super::area::{Area, Travels};
    use super::geometry::{SubPath, subpaths};
    // An L: its reflex corner at (8, 8).
    let subs = subpaths(&polygon([(0.0, 0.0), (20.0, 0.0), (20.0, 8.0), (8.0, 8.0), (8.0, 20.0), (0.0, 20.0)]));
    let outlines: Vec<&SubPath> = subs.iter().collect();
    let mut travels = Travels::new(Area { outlines: &outlines, rule: FillRule::NonZero, grid: None });
    let (a, b) = (Point::new(18.0, 4.0), Point::new(4.0, 18.0));
    let way = travels.way(a, b).expect("a way inside");
    assert_eq!(way.len(), 3, "round the one corner: {way:?}");
    let (corner, turn) = (Point::new(8.0, 8.0), way[1]);
    assert!(turn.x < 8.0 && turn.y < 8.0, "inside the shape: {turn:?}");
    let d = turn.distance(corner);
    assert!((0.4..=0.6).contains(&d), "{d} mm from the corner");
}

/// Fill rows are staggered: each row's penetrations sit a quarter stitch along from the row
/// before's, through a cycle of four rows, so that no two neighbouring rows line their
/// penetrations up into a groove.
#[test]
fn fill_rows_stagger_through_a_cycle_of_four() {
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    p.underlay = Underlay::None;
    let out = stitches(&rect(0.3, 0.0, 40.0, 10.0), FillRule::NonZero, &p);
    // Each row's penetrations away from its ends (where the row starts and stops, ties
    // included), by row.
    let mut rows: std::collections::BTreeMap<i64, Vec<f64>> = Default::default();
    let first = first_row(0.0, 10.0, p.density);
    for q in points(&out) {
        let k = ((q.y - first) / p.density).round();
        assert!((q.y - first - p.density * k).abs() < 1e-6, "{q:?} is on no row");
        rows.entry(k as i64).or_default().push(q.x);
    }
    let phases: Vec<f64> = rows
        .values()
        .map(|xs| {
            let (lo, hi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
            let inner: Vec<f64> = xs.iter().copied().filter(|x| *x > lo + 1.5 && *x < hi - 1.5).collect();
            assert!(inner.len() >= 5, "{xs:?}");
            let phase = (inner[0] / p.stitch_length).rem_euclid(1.0);
            for x in &inner {
                let off = ((x / p.stitch_length).rem_euclid(1.0) - phase).abs();
                assert!(off < 1e-6 || (1.0 - off) < 1e-6, "a row's penetrations off its grid: {inner:?}");
            }
            phase
        })
        .collect();
    assert_eq!(phases.len(), (10.0 / p.density).floor() as usize);
    for w in phases.windows(2) {
        let step = (w[1] - w[0]).rem_euclid(1.0);
        assert!((step - 0.25).abs() < 1e-6, "rows {:?}: a step of {step} stitch", w);
    }
}

/// The longest travel run in `out` and the length of all of them, millimetres: runs of three or
/// more stitches in a row of a travel's length (rows' stitches are longer, row turns shorter).
fn travels(out: &[Stitch]) -> (f64, f64) {
    let (mut longest, mut total) = (0.0f64, 0.0);
    let (mut run, mut n) = (0.0, 0);
    for w in out.windows(2) {
        let d = w[0].distance(w[1]);
        if w[0].command == Command::Stitch && w[1].command == Command::Stitch && (0.9..=2.0 + 1e-9).contains(&d) {
            run += d;
            n += 1;
            continue;
        }
        if n >= 3 {
            longest = longest.max(run);
            total += run;
        }
        (run, n) = (0.0, 0);
    }
    if n >= 3 {
        longest = longest.max(run);
        total += run;
    }
    (longest, total)
}

/// Of the orders that sew a shape in one go, the one that travels least is kept: a U whose arms
/// are 30 mm apart is sewn arm, bar, arm, each joined to the bar near where they meet it, not
/// both arms first with a travel from one to the other across the whole bar.
#[test]
fn the_order_that_travels_least_wins() {
    let u = polygon([(0.0, 0.0), (42.0, 0.0), (42.0, 40.0), (36.0, 40.0), (36.0, 8.0), (6.0, 8.0), (6.0, 40.0), (0.0, 40.0)]);
    for angle in [45.0, 60.0, 135.0] {
        let mut p = Params::for_style(Style::Fill);
        p.angle = angle;
        p.underlay = Underlay::None;
        let out = stitches(&u, FillRule::NonZero, &p);
        assert_eq!(out.iter().filter(|s| s.command != Command::Stitch).count(), 0, "angle {angle}: sewn in one go");
        let (longest, total) = travels(&out);
        assert!(longest < 15.0, "angle {angle}: a {longest:.1} mm travel ({total:.1} mm in all)");
    }
}

/// A comb of `n` teeth `long` mm long, `wide` mm wide and `gap` mm apart, on a bar `bar` mm high:
/// with horizontal rows, the bar and each tooth are regions of their own.
fn comb(n: usize, long: f64, wide: f64, gap: f64, bar: f64) -> BezPath {
    let w = n as f64 * wide + (n as f64 - 1.0) * gap;
    let mut pts = vec![(0.0, 0.0), (w, 0.0)];
    for k in (0..n).rev() {
        let x0 = k as f64 * (wide + gap);
        pts.push((x0 + wide, bar + long));
        pts.push((x0, bar + long));
        if k > 0 {
            pts.push((x0, bar));
            pts.push((x0 - gap, bar));
        }
    }
    polygon(pts)
}

/// Shapes whose regions branch (a U, combs, rings, an H) are sewn in one go at any angle, underlay
/// or not: of the ways to order their regions, one that travels under stitching still to come (or
/// shows a little) is kept over one that jumps, and a jump that would need a cut costs more still.
#[test]
fn branching_shapes_are_sewn_in_one_go() {
    let u = polygon([(0.0, 0.0), (14.0, 0.0), (14.0, 30.0), (9.0, 30.0), (9.0, 8.0), (5.0, 8.0), (5.0, 30.0), (0.0, 30.0)]);
    let ring = |hole: f64| {
        let a = 15.0 - hole / 2.0;
        let mut p = rect(0.0, 0.0, 40.0, 30.0);
        p.extend(polygon([(a, a), (a, a + hole), (a + hole, a + hole), (a + hole, a)]));
        p
    };
    let h = polygon([
        (0.0, 0.0),
        (8.0, 0.0),
        (8.0, 16.0),
        (22.0, 16.0),
        (22.0, 0.0),
        (30.0, 0.0),
        (30.0, 40.0),
        (22.0, 40.0),
        (22.0, 24.0),
        (8.0, 24.0),
        (8.0, 40.0),
        (0.0, 40.0),
    ]);
    let shapes = [("U", u), ("3 teeth", comb(3, 30.0, 4.0, 4.0, 6.0)), ("5 teeth", comb(5, 35.0, 4.0, 4.0, 6.0)), ("ring", ring(4.0)), ("H", h)];
    for (name, shape) in &shapes {
        for underlay in [Underlay::None, Underlay::Auto] {
            for angle in (0..180).step_by(15).map(f64::from) {
                let mut p = Params::for_style(Style::Fill);
                p.angle = angle;
                p.underlay = underlay;
                let out = stitches(shape, FillRule::NonZero, &p);
                let lifts = out.iter().filter(|s| s.command != Command::Stitch).count();
                assert_eq!(lifts, 0, "{name}, angle {angle}, underlay {underlay:?}: {lifts} jumps and trims");
            }
        }
    }
}

/// A turn between two rows that end on one straight edge runs along it: the outline belongs to
/// the shape. (Rows nearly parallel to a U's arms end on the arms' outer edges a few spacings
/// apart; the turn along the edge was taken for a way outside, and the needle lifted or travelled
/// round the U, at 0.37–0.39 mm spacing.)
#[test]
fn a_turn_along_a_straight_edge_is_sewn() {
    let u = polygon([(0.0, 0.0), (14.0, 0.0), (14.0, 30.0), (9.0, 30.0), (9.0, 8.0), (5.0, 8.0), (5.0, 30.0), (0.0, 30.0)]);
    let wide = polygon([(0.0, 0.0), (42.0, 0.0), (42.0, 40.0), (36.0, 40.0), (36.0, 8.0), (6.0, 8.0), (6.0, 40.0), (0.0, 40.0)]);
    for density in [0.3, 0.34, 0.36, 0.37, 0.38, 0.39, 0.4, 0.42, 0.45] {
        for angle in [45.0, 60.0, 75.0, 105.0, 135.0] {
            let mut p = Params::for_style(Style::Fill);
            p.density = density;
            p.angle = angle;
            p.underlay = Underlay::None;
            for (name, shape) in [("U", &u), ("wide U", &wide)] {
                let out = stitches(shape, FillRule::NonZero, &p);
                let lifts = out.iter().filter(|s| s.command != Command::Stitch).count();
                assert_eq!(lifts, 0, "{name}, {density} mm, {angle}°: {lifts} jumps and trims");
            }
            // The wide U is sewn arm, bar, arm, with no travel across the bar (30 mm): a travel
            // down an arm's side to the bar is at most its 14 mm, and the arm's last row's end
            // stitch before it counts with it.
            let (longest, total) = travels(&stitches(&wide, FillRule::NonZero, &p));
            assert!(longest < 20.0, "wide U, {density} mm, {angle}°: a {longest:.1} mm travel ({total:.1} mm in all)");
        }
    }
}

/// A travel may show at most 25 mm over stitching already sewn; one that would show more lifts the
/// needle. A comb of 200 teeth has more regions than orders are compared for, so it is sewn
/// nearest region next: tooth after tooth, each reached back along the one just sewn. Teeth 20 mm
/// long travel; 30 mm long, the needle jumps instead.
#[test]
fn a_travel_that_would_show_too_much_lifts_the_needle() {
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    p.underlay = Underlay::None;
    let jumps = |long: f64| stitches(&comb(200, long, 2.0, 1.5, 4.0), FillRule::NonZero, &p).iter().filter(|s| s.command == Command::Jump).count();
    assert_eq!(jumps(20.0), 0);
    assert!(jumps(30.0) >= 50, "{}", jumps(30.0));
}

/// The lines of a running path are sewn nearest first, not as drawn: three lines drawn left,
/// right, middle are sewn left to right, and from a needle at the right, right to left.
#[test]
fn lines_are_sewn_nearest_first() {
    let mut path = line((0.0, 0.0), (10.0, 0.0));
    path.extend(line((40.0, 0.0), (50.0, 0.0)));
    path.extend(line((20.0, 0.0), (30.0, 0.0)));
    // The x of each line's first needle point, in sewing order.
    let starts = |from: Option<Point>| {
        let mut v: Vec<f64> = Vec::new();
        for q in points(&stitches_from(&path, FillRule::NonZero, &Params::for_style(Style::Running), from)) {
            if v.last().is_none_or(|x| (q.x - x).abs() > 12.0) {
                v.push(q.x);
            }
        }
        v
    };
    assert_eq!(starts(None), [0.0, 20.0, 40.0]);
    assert_eq!(starts(Some(Point::new(60.0, 0.0))), [50.0, 30.0, 10.0]);
}

/// Pieces of a cut block ([`join_cut`]) are joined as runs are: a gap up to [`Params::jump`] by a
/// jump with the thread on and no ties, a longer one tied off and in around the jump, and the
/// thread cut only past [`Params::trim`]. The needle points of each piece stay as they were.
#[test]
fn cut_pieces_are_joined_by_their_gap() {
    let piece = |x: f64| (0..3).map(|k| Stitch::sew(Point::new(x + 2.0 * f64::from(k), 0.0))).collect::<Vec<_>>();
    let join = |gap: f64, lock: bool| {
        let p = Params { lock, ..Params::for_style(Style::Running) };
        // The second piece starts with a jump to it, as a piece cut out of a block may: it is
        // dropped.
        let mut second = vec![Stitch::jump(Point::new(4.0 + gap, 0.0))];
        second.extend(piece(4.0 + gap));
        join_cut(vec![piece(0.0), second], &p)
    };
    let commands = |v: &[Stitch]| v.iter().map(|s| s.command).collect::<Vec<_>>();
    use Command::{Jump as J, Stitch as S, Trim as T};
    let p = Params::for_style(Style::Running);
    for gap in [2.0, p.jump] {
        let out = join(gap, true);
        assert_eq!(commands(&out), [S, S, S, J, S, S, S], "a {gap} mm gap");
        assert_eq!(out[3].point(), Point::new(4.0 + gap, 0.0));
    }
    for gap in [p.jump + 0.5, p.trim] {
        let out = join(gap, true);
        assert_eq!(commands(&out), [S, S, S, S, S, J, S, S, S, S, S], "a {gap} mm gap: tied off, jumped, tied in");
        // Back and forth on the spot on each side.
        assert_eq!(out[2].point(), out[4].point());
        assert_eq!(out[6].point(), out[8].point());
        assert_eq!(out[6].point(), Point::new(4.0 + gap, 0.0));
    }
    assert_eq!(commands(&join(p.trim + 0.5, true)), [S, S, S, S, S, T, J, S, S, S, S, S], "cut past the trim length");
    assert_eq!(commands(&join(p.jump + 0.5, false)), [S, S, S, J, S, S, S], "no ties without lock");
    assert_eq!(commands(&join(p.trim + 0.5, false)), [S, S, S, T, J, S, S, S]);
}

/// Along a fine zigzag (sharp corners closer together than 0.3 of a stitch), a running line keeps
/// its stitch length: it passes the corners it can't stitch into without a stub, rather than one
/// needle point moving from corner to corner into a single stitch the length of the line. An edge
/// run under a fill with a zigzag edge keeps its length the same way.
#[test]
fn a_fine_zigzag_keeps_its_stitch_length() {
    let zigzag = |amp: f64, period: f64| (0..=(100.0 / period) as usize).map(move |k| (k as f64 * period / 2.0, if k % 2 == 1 { amp } else { 0.0 }));
    let longest = |out: &[Stitch]| {
        out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch).map(|w| w[0].distance(w[1])).fold(0.0, f64::max)
    };
    for (amp, period) in [(0.3, 0.6), (0.2, 1.0), (0.5, 0.8)] {
        let mut path = BezPath::new();
        for (k, q) in zigzag(amp, period).enumerate() {
            if k == 0 { path.move_to(q) } else { path.line_to(q) }
        }
        let p = Params::for_style(Style::Running);
        let out = stitches(&path, FillRule::NonZero, &p);
        assert!(longest(&out) <= 1.3 * p.stitch_length + 1e-9, "{amp} × {period}: a {:.2} mm stitch", longest(&out));
        assert!(points(&out).len() >= 20, "{amp} × {period}: {} needle points along 50 mm", points(&out).len());
        assert!(points(&out).iter().all(|q| q.y >= -1e-9 && q.y <= amp + 1e-9), "{amp} × {period}: off the line");

        let mut edge: Vec<(f64, f64)> = zigzag(amp, period).map(|(x, y)| (x, 20.0 - y)).collect();
        edge.extend([(50.0, 0.0), (0.0, 0.0)]);
        let mut p = Params::for_style(Style::Fill);
        p.angle = 90.0;
        let out = stitches(&polygon(edge), FillRule::NonZero, &p);
        assert!(longest(&out) <= p.stitch_length + 2.0 * p.min_stitch + 1e-9, "{amp} × {period}: a {:.2} mm fill stitch", longest(&out));
    }
}

/// The needle points of a satin column along `path` with only the given compensation: no underlay
/// or locks, so every point is a crossing's.
fn bare_satin(path: &BezPath, width: f64, pull: [f64; 2], pull_percent: [f64; 2], push: [f64; 2], from: Option<Point>) -> Vec<Point> {
    let mut p = Params::for_style(Style::Satin);
    p.width = width;
    (p.pull, p.pull_percent, p.push) = (pull, pull_percent, push);
    p.underlay = Underlay::None;
    p.lock = false;
    points(&stitches_from(path, FillRule::NonZero, &p, from))
}

/// Pull compensation can differ between a satin's rails: the first value widens the rail on the
/// left as the path is drawn (y runs down the page), the second the one on the right, whichever
/// end the column is sewn from.
#[test]
fn satin_pull_compensation_is_per_side() {
    let span = |q: &[Point]| (q.iter().map(|a| a.y).fold(f64::INFINITY, f64::min), q.iter().map(|a| a.y).fold(f64::NEG_INFINITY, f64::max));
    let rightwards = line((0.0, 0.0), (20.0, 0.0));
    for from in [None, Some(Point::new(21.0, 0.0))] {
        let (lo, hi) = span(&bare_satin(&rightwards, 4.0, [0.5, 0.0], [0.0; 2], [0.0; 2], from));
        assert!((lo + 2.5).abs() < 1e-6 && (hi - 2.0).abs() < 1e-6, "from {from:?}: rails at {lo} and {hi}");
    }
    // Drawn the other way, its left is the other side.
    let (lo, hi) = span(&bare_satin(&line((20.0, 0.0), (0.0, 0.0)), 4.0, [0.5, 0.0], [0.0; 2], [0.0; 2], None));
    assert!((lo + 2.0).abs() < 1e-6 && (hi - 2.5).abs() < 1e-6, "rails at {lo} and {hi}");
}

/// Pull compensation can grow with the column: a share of its width added at each rail, on top of
/// the fixed value, at most 2 mm a side.
#[test]
fn satin_pull_compensation_grows_with_the_width() {
    let widest = |width: f64, pull: f64, percent: f64| {
        bare_satin(&line((0.0, 0.0), (20.0, 0.0)), width, [pull; 2], [percent; 2], [0.0; 2], None).iter().map(|a| a.y.abs()).fold(0.0, f64::max)
    };
    assert!((widest(4.0, 0.0, 10.0) - 2.4).abs() < 1e-6, "{}", widest(4.0, 0.0, 10.0));
    assert!((widest(8.0, 0.0, 10.0) - 4.8).abs() < 1e-6, "{}", widest(8.0, 0.0, 10.0));
    assert!((widest(4.0, 0.2, 10.0) - 2.6).abs() < 1e-6, "{}", widest(4.0, 0.2, 10.0));
    assert!((widest(20.0, 0.5, 50.0) - 12.0).abs() < 1e-6, "at most 2 mm a side: {}", widest(20.0, 0.5, 50.0));
}

/// Push compensation sews an open satin column shorter at its start and its end as drawn, whichever
/// end it is sewn from, the underlay with it; a column keeps at least half its length; a closed one
/// has no ends to shorten.
#[test]
fn satin_push_compensation_shortens_the_ends() {
    let extent = |q: &[Point]| (q.iter().map(|a| a.x).fold(f64::INFINITY, f64::min), q.iter().map(|a| a.x).fold(f64::NEG_INFINITY, f64::max));
    let column = line((0.0, 0.0), (20.0, 0.0));
    for from in [None, Some(Point::new(21.0, 0.0))] {
        let (lo, hi) = extent(&bare_satin(&column, 4.0, [0.0; 2], [0.0; 2], [1.0, 2.0], from));
        assert!((lo - 1.0).abs() < 1e-6 && (hi - 18.0).abs() < 1e-6, "from {from:?}: {lo} to {hi}");
    }
    let mut p = Params::for_style(Style::Satin);
    p.width = 4.0;
    p.push = [1.0, 2.0];
    let (lo, hi) = extent(&points(&stitches(&column, FillRule::NonZero, &p)));
    assert!(lo >= 1.0 - 1e-6 && hi <= 18.0 + 1e-6, "with underlay and ties: {lo} to {hi}");
    // Asked for more than half a 4 mm column: both ends give in proportion, half of it is left.
    let (lo, hi) = extent(&bare_satin(&line((0.0, 0.0), (4.0, 0.0)), 1.0, [0.0; 2], [0.0; 2], [3.0, 3.0], None));
    assert!((lo - 1.0).abs() < 1e-6 && (hi - 3.0).abs() < 1e-6, "{lo} to {hi}");
    let ring: BezPath = vectorcraft_geom::Shape::path_elements(&vectorcraft_geom::kurbo::Circle::new((0.0, 0.0), 10.0), 0.01).collect();
    assert_eq!(
        bare_satin(&ring, 3.0, [0.0; 2], [0.0; 2], [2.0, 2.0], None),
        bare_satin(&ring, 3.0, [0.0; 2], [0.0; 2], [0.0; 2], None),
        "a closed column is not shortened"
    );
}

/// A fill's rows reach past the shape's edge at each end by its pull compensation (the first value
/// at the rows' start, the second at their end), fixed or by the row's length; across the rows the
/// shape keeps its size.
#[test]
fn fill_rows_reach_past_the_edge_by_the_pull_compensation() {
    let reach = |pull: [f64; 2], pull_percent: [f64; 2]| {
        let mut p = Params::for_style(Style::Fill);
        p.angle = 0.0;
        p.underlay = Underlay::None;
        (p.pull, p.pull_percent) = (pull, pull_percent);
        let q = points(&stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p));
        assert!(q.iter().all(|a| a.y >= -1e-6 && a.y <= 10.0 + 1e-6), "{pull:?} {pull_percent:?}: a row off the shape across");
        (-q.iter().map(|a| a.x).fold(f64::INFINITY, f64::min), q.iter().map(|a| a.x).fold(f64::NEG_INFINITY, f64::max) - 20.0)
    };
    let (start, end) = reach([0.3, 0.1], [0.0; 2]);
    assert!((start - 0.3).abs() < 1e-6 && (end - 0.1).abs() < 1e-6, "{start} and {end}");
    let (start, end) = reach([0.0; 2], [1.0, 1.0]);
    assert!((start - 0.2).abs() < 1e-6 && (end - 0.2).abs() < 1e-6, "1 % of 20 mm rows: {start} and {end}");
    let (start, end) = reach([0.0; 2], [0.0; 2]);
    assert!(start.abs() < 1e-6 && end.abs() < 1e-6, "none: {start} and {end}");
}

/// Rows either side of a narrow gap reach at most half way into it, so they never meet across it:
/// two squares 0.4 mm apart with 0.5 mm of pull compensation.
#[test]
fn compensated_rows_never_cross_a_gap() {
    let mut shape = rect(0.0, 0.0, 10.0, 10.0);
    shape.extend(rect(10.4, 0.0, 10.0, 10.0));
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    p.pull = [0.5; 2];
    let out = stitches(&shape, FillRule::NonZero, &p);
    let middle = 10.2;
    for w in out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch) {
        let (a, b) = (w[0].x.min(w[1].x), w[0].x.max(w[1].x));
        assert!(!(a < middle - 1e-6 && b > middle + 1e-6), "a stitch across the gap: {:?} to {:?}", w[0].point(), w[1].point());
    }
    let q = points(&out);
    assert!(q.iter().any(|a| (a.x - middle).abs() < 1e-6), "the rows reach the middle of the gap");
}

/// Where a fill puts its first row across an extent from `lo` to `hi`: the rows the spacing fits,
/// rounded, centred, `spacing` apart.
fn first_row(lo: f64, hi: f64, spacing: f64) -> f64 {
    let rows = ((hi - lo) / spacing).round().max(1.0);
    lo + ((hi - lo) - spacing * (rows - 1.0)) / 2.0
}

/// The rows `out` sews in the order it first reaches them, by number: a fill of `shape` at `angle`
/// degrees has its rows `spacing` apart, centred across its extent ([`first_row`]), and every
/// needle point on one of them counts for it (travels between rows fall between them). Coming back
/// to a row (a tie-off along the last stitch, a region's overlap with its neighbour) counts nothing.
fn row_order(out: &[Stitch], shape: &BezPath, angle: f64, spacing: f64) -> Vec<i64> {
    let axes = Axes::new(angle);
    let corners: Vec<f64> = shape
        .elements()
        .iter()
        .filter_map(|el| match el {
            vectorcraft_geom::PathEl::MoveTo(q) | vectorcraft_geom::PathEl::LineTo(q) => Some(axes.across(*q)),
            _ => None,
        })
        .collect();
    let lo = corners.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let first = first_row(lo, hi, spacing);
    let mut order: Vec<i64> = Vec::new();
    for q in points(out) {
        let k = (axes.across(q) - first) / spacing;
        if (k - k.round()).abs() < 1e-6 && !order.contains(&(k.round() as i64)) {
            order.push(k.round() as i64);
        }
    }
    order
}

/// Pull compensation changes no sewing order: a fill's regions are ordered by moves judged inside
/// the shape, so with and without it the same shapes are sewn row after row in the same order, with
/// the same jumps and trims.
#[test]
fn pull_compensation_changes_no_sewing_order() {
    let wide_u = polygon([(0.0, 0.0), (42.0, 0.0), (42.0, 40.0), (36.0, 40.0), (36.0, 8.0), (6.0, 8.0), (6.0, 40.0), (0.0, 40.0)]);
    let mut ring = rect(0.0, 0.0, 30.0, 30.0);
    ring.extend(polygon([(10.0, 10.0), (10.0, 20.0), (20.0, 20.0), (20.0, 10.0)]));
    for (name, shape) in [("wide U", &wide_u), ("ring", &ring), ("comb", &comb(5, 35.0, 4.0, 4.0, 6.0))] {
        for angle in [0.0, 45.0, 90.0, 135.0] {
            for density in [0.34, 0.38] {
                let sew = |pull: [f64; 2]| {
                    let mut p = Params::for_style(Style::Fill);
                    (p.angle, p.density, p.pull) = (angle, density, pull);
                    let out = stitches(shape, FillRule::NonZero, &p);
                    (out.iter().filter(|s| s.command != Command::Stitch).count(), row_order(&out, shape, angle, density))
                };
                let ((lifts, rows), (lifts_0, rows_0)) = (sew([0.3; 2]), sew([0.0; 2]));
                assert_eq!(lifts, lifts_0, "{name}, {angle}°, {density} mm: jumps and trims");
                assert!(rows.len() > 50, "{name}, {angle}°, {density} mm: {} rows", rows.len());
                assert_eq!(rows, rows_0, "{name}, {angle}°, {density} mm: the rows' order");
            }
        }
    }
}

/// The spacing follows the thread's weight: the defaults are for 40 wt, a finer thread (a larger
/// weight) sews rows and satin crossings closer together, a thicker one farther apart; a weight out
/// of range is clamped.
#[test]
fn spacing_follows_the_thread_weight() {
    assert!((thread_scale(THREAD_WEIGHT) - 1.0).abs() < 1e-12);
    assert_eq!(Params::for_thread(Style::Fill, THREAD_WEIGHT), Params::for_style(Style::Fill));
    let fill = |w: f64| Params::for_thread(Style::Fill, w).density;
    assert!((fill(60.0) - 0.310).abs() < 1e-3 && (fill(30.0) - 0.439).abs() < 1e-3, "{} {}", fill(60.0), fill(30.0));
    assert!(fill(f64::NAN) == fill(40.0) && fill(1.0) == fill(10.0) && fill(1e9) == fill(150.0));
    // A satin's auto spacing: the distance between two penetrations on one rail (peak to peak).
    let rail_gap = |weight: f64| {
        let mut p = Params::for_thread(Style::Satin, weight);
        p.width = 5.0;
        p.pull = [0.0; 2];
        p.underlay = Underlay::None;
        p.lock = false;
        let top: Vec<f64> = points(&stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p))
            .iter()
            .filter(|a| (a.y - 2.5).abs() < 1e-6)
            .map(|a| a.x)
            .collect();
        top[2] - top[1]
    };
    assert!((rail_gap(40.0) - 0.4).abs() < 0.01 && (rail_gap(60.0) - 0.4 * thread_scale(60.0)).abs() < 0.01, "{} {}", rail_gap(40.0), rail_gap(60.0));
    let mut odd = Params::for_style(Style::Satin);
    odd.thread_weight = f64::INFINITY;
    assert_eq!(odd.sane().thread_weight, THREAD_WEIGHT);
}

/// Every crossing of an open satin column goes across it, the last one too, whichever way the
/// column runs (the last point's direction was taken as left to right, so a column running any
/// other way ended with a crossing along it, or on the rail the one before used).
#[test]
fn the_last_crossing_goes_across() {
    for (a, b) in [((0.0, 0.0), (0.0, 20.0)), ((20.0, 0.0), (0.0, 0.0)), ((0.0, 0.0), (14.0, 14.0))] {
        let q = bare_satin(&line(a, b), 4.0, [0.0; 2], [0.0; 2], [0.0; 2], None);
        let (a, b) = (Point::new(a.0, a.1), Point::new(b.0, b.1));
        let normal = Vec2::new(-(b - a).y, (b - a).x) / a.distance(b);
        let sides: Vec<f64> = q.iter().map(|p| (*p - a).dot(normal)).collect();
        assert!(sides.iter().all(|d| (d.abs() - 2.0).abs() < 1e-6), "{a:?} to {b:?}: {sides:?}");
        assert!(sides.windows(2).all(|w| w[0] * w[1] < 0.0), "{a:?} to {b:?}: rails alternate: {sides:?}");
    }
}

/// A region's last row keeps its last stitch, so it reaches past the edge by the pull
/// compensation: a fill of one row (thinner than two spacings) is sewn edge to edge, not stopped
/// up to half a stitch short of its end.
#[test]
fn a_regions_last_row_reaches_its_edge() {
    for (w, h) in [(5.0, 0.5), (10.0, 0.5), (10.0, 3.0), (17.0, 2.0)] {
        let mut p = Params::for_style(Style::Fill);
        p.angle = 0.0;
        p.underlay = Underlay::None;
        let out = stitches(&rect(0.0, 0.0, w, h), FillRule::NonZero, &p);
        let q = points(&out);
        // The last row ends past one edge (its last needle point, before the tie-off's step back).
        let Some(end) = q.iter().rev().nth(2) else { panic!("{w} × {h}: no stitches") };
        assert!((end.x + p.pull[0]).abs() < 1e-6 || (end.x - w - p.pull[1]).abs() < 1e-6, "{w} × {h}: the last row ends at {end:?}");
        // A fill of one row reaches past both.
        if h < 2.0 * p.density {
            let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| (lo.min(a.x), hi.max(a.x)));
            assert!((lo + p.pull[0]).abs() < 1e-6 && (hi - w - p.pull[1]).abs() < 1e-6, "{w} × {h}: the row runs {lo} to {hi}");
        }
    }
}

/// Push compensation never takes away a column that would be sewn without it: a 1.3 mm column
/// pushed 0.3 mm at each end is sewn, shortened to no less than the shortest column sewn.
#[test]
fn push_compensation_keeps_a_short_column() {
    let mut p = Params::for_style(Style::Satin);
    p.width = 2.0;
    p.push = [0.3; 2];
    let q = points(&stitches(&line((0.0, 0.0), (1.3, 0.0)), FillRule::NonZero, &p));
    assert!(!q.is_empty(), "the column is sewn");
    let (lo, hi) = q.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), a| (lo.min(a.x), hi.max(a.x)));
    assert!(hi - lo >= p.density * 2.0 - 1e-6 && hi - lo < 1.3, "{lo} to {hi}");
}

/// A line cut on one of its points keeps that point once.
#[test]
fn a_cut_on_a_point_of_the_line_is_one_point() {
    let pts = [Point::new(0.0, 0.0), Point::new(1.0, 0.0), Point::new(3.0, 0.0)];
    let cut = super::geometry::trimmed(&pts, 0.0, 2.0);
    assert_eq!(cut, vec![Point::new(0.0, 0.0), Point::new(1.0, 0.0)]);
    let cut = super::geometry::trimmed(&pts, 0.5, 0.0);
    assert_eq!(cut, vec![Point::new(0.5, 0.0), Point::new(1.0, 0.0), Point::new(3.0, 0.0)]);
}

/// A satin column sews as densely as Madeira's stitch density chart for 40 wt: 51 satin stitches a
/// centimetre at its 0.40 mm (measured peak to peak): a 5 mm column (auto spacing 0.40 mm) puts 50
/// crossings in each centimetre.
#[test]
fn a_satin_sews_as_densely_as_the_thread_chart() {
    let mut p = Params::for_style(Style::Satin);
    p.width = 5.0;
    p.underlay = Underlay::None;
    p.lock = false;
    let q = points(&stitches(&line((0.0, 0.0), (30.0, 0.0)), FillRule::NonZero, &p));
    let in_a_cm = q.iter().filter(|a| a.x >= 10.0 && a.x < 20.0).count();
    assert!((49..=51).contains(&in_a_cm), "{in_a_cm} crossings in a centimetre");
}

/// Round a bend a stroke's satin keeps its spacing on the outer rail, evenly, and its inner rail
/// takes the penetrations closer by the rails' radii, as a fill's columns do: a 5 mm stroke along
/// a half turn of radius 6 mm, and a letter S of two such half turns, have their outer rails the
/// auto spacing for 5 mm (0.40 mm) apart and their inner rails that times the inner rail's radius
/// over the outer's. (Spaced along the centre line, with each crossing square to its flattened
/// segment, the outer rail's penetrations lay 0.40 mm apart along a segment and up to 0.97 mm where
/// two met.)
#[test]
fn a_satin_stroke_keeps_its_spacing_on_the_outer_rail() {
    use super::tests_narrow::{quantile, rail_steps};
    use std::f64::consts::{FRAC_PI_2, PI};
    let (r, width) = (6.0, 5.0);
    let mut p = Params::for_style(Style::Satin);
    p.width = width;
    p.underlay = Underlay::None;
    p.lock = false;
    let spacing = satin_spacing(width);
    let rails = [r + width / 2.0 + p.pull[0], r - width / 2.0 - p.pull[1]];
    let closer = spacing * rails[1] / rails[0];
    let check = |name: &str, q: &[Point], centre: (f64, f64), toward: (f64, f64)| {
        // Away from the ends and from where the S's two turns meet.
        let window = 1.2;
        let [outer, inner] = rail_steps(q, Point::new(centre.0, centre.1), Vec2::new(toward.0, toward.1), window, r, rails);
        let (lo, mid, hi) = (quantile(&outer, 0.1), quantile(&outer, 0.5), quantile(&outer, 0.9));
        let inner = quantile(&inner, 0.5);
        eprintln!("{name}: outer rail {lo:.3} / {mid:.3} / {hi:.3} mm (spacing {spacing}), inner {inner:.3} mm ({closer:.3})");
        assert!((mid / spacing - 1.0).abs() < 0.1, "{name}: the outer rail's penetrations {mid:.3} mm apart, not {spacing}");
        assert!(lo > spacing * 0.9 && hi < spacing * 1.1, "{name}: the outer rail's penetrations {lo:.3} to {hi:.3} mm apart");
        assert!((inner / closer - 1.0).abs() < 0.1, "{name}: the inner rail's penetrations {inner:.3} mm apart, not {closer:.3}");
    };
    // A half turn round (0, 0) through (-6, 0), then one the other way round (0, -12) through
    // (6, -12).
    let first = vectorcraft_geom::kurbo::Arc::new((0.0, 0.0), (r, r), FRAC_PI_2, PI, 0.0);
    let second = vectorcraft_geom::kurbo::Arc::new((0.0, -12.0), (r, r), FRAC_PI_2, -PI, 0.0);
    let mut arc = BezPath::new();
    arc.move_to((0.0, r));
    arc.extend(first.append_iter(0.01));
    check("the half turn", &points(&stitches(&arc, FillRule::NonZero, &p)), (0.0, 0.0), (-1.0, 0.0));
    let mut s = arc.clone();
    s.extend(second.append_iter(0.01));
    let q = points(&stitches(&s, FillRule::NonZero, &p));
    check("the S's upper turn", &q, (0.0, 0.0), (-1.0, 0.0));
    check("the S's lower turn", &q, (0.0, -12.0), (1.0, 0.0));
}

/// Fill rows are centred across the shape: the margin at both sides is the same, a quarter to three
/// quarters of a spacing (a band 10.9 spacings tall had 0.19 mm at one side and 0.57 at the
/// other).
#[test]
fn fill_rows_are_centred_across_the_shape() {
    let mut p = Params::for_style(Style::Fill);
    (p.angle, p.underlay) = (0.0, Underlay::None);
    for tall in [10.9, 10.5, 11.0, 3.2] {
        let h = tall * p.density;
        let out = stitches(&rect(0.0, 0.0, 30.0, h), FillRule::NonZero, &p);
        let ys: Vec<f64> = points(&out).iter().map(|q| q.y).collect();
        let (top, bottom) = ys.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
        assert!((top - (h - bottom)).abs() < 1e-6, "{tall} spacings: margins {top} and {}", h - bottom);
        assert!(top >= 0.25 * p.density - 1e-9 && top <= 0.75 * p.density + 1e-9, "{tall} spacings: margin {top}");
    }
}

/// A fill's edge-run underlay runs half a spacing inside its outline, about where the outermost
/// rows lie, so the top stitching covers it.
#[test]
fn a_fills_edge_run_is_inset() {
    let p = Params::for_style(Style::Fill);
    let (w, h) = (20.0, 12.0);
    let out = stitches(&rect(0.0, 0.0, w, h), FillRule::NonZero, &p);
    // The edge run comes first: needle points up to the first top row's long stitches. Its
    // corners, the points nearest the rectangle's.
    let inset = p.density * 0.5;
    for corner in [Point::new(0.0, 0.0), Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)] {
        let nearest = points(&out).iter().map(|q| q.distance(corner)).fold(f64::INFINITY, f64::min);
        // A corner moves along its bisector: √2 times the inset from the outline's corner.
        assert!((nearest - inset * std::f64::consts::SQRT_2).abs() < 0.05, "{corner:?}: {nearest}");
    }
}

/// With a shortest stitch above the lock's 1 mm, locks, tie-ins and tie-offs step that far, so the
/// planner's filter of short stitches keeps them: a running line keeps its locks, and a fill
/// started after a long jump is tied in.
#[test]
fn locks_and_ties_are_no_shorter_than_the_shortest_stitch() {
    let min_stitch = 1.5;
    let line = line((0.0, 0.0), (20.0, 0.0));
    let p = Params { min_stitch, ..Params::for_style(Style::Running) };
    let q = points(&stitches(&line, FillRule::NonZero, &p));
    assert!((q[1].x - min_stitch).abs() < 1e-9 && q[2].x.abs() < 1e-9, "locked at the start: {q:?}");
    let n = q.len();
    assert!((q[n - 2].x - (20.0 - min_stitch)).abs() < 1e-9, "locked at the end: {q:?}");
    // Two squares far apart in one fill: the second is reached by a jump, tied off before and in
    // after, each a step of the shortest stitch.
    let mut two = rect(0.0, 0.0, 10.0, 10.0);
    two.extend(rect(40.0, 0.0, 50.0, 10.0));
    let p = Params { min_stitch, ..Params::for_style(Style::Fill) };
    let out = stitches(&two, FillRule::NonZero, &p);
    let jump = out.iter().position(|s| s.command == Command::Jump).expect("a jump to the second square");
    let landing = out[jump].point();
    let after: Vec<Point> = out[jump + 1..].iter().take(3).map(|s| s.point()).collect();
    assert!((after[0].distance(landing)) < 1e-9, "down where the jump lands: {after:?}");
    assert!((after[1].distance(landing) - min_stitch).abs() < 1e-6 && after[2].distance(landing) < 1e-9, "tied in: {after:?}");
    // No needle-down stitch is shorter than asked, the ties included.
    for w in out.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch) {
        let d = w[0].point().distance(w[1].point());
        assert!(d < crate::SAME_POINT || d + 1e-9 >= min_stitch, "a {d} mm stitch");
    }
}
