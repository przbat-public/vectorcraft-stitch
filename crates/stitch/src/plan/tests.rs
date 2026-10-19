//! The stitch planner's tests: each style's shape, lengths, ties and underlay, fills with holes
//! and gaps, and odd input.

use vectorcraft_geom::Point;

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

#[test]
fn a_fill_covers_the_rectangle_and_no_stitch_is_longer_than_asked() {
    let p = Params::for_style(Style::Fill);
    let out = stitches(&rect(0.0, 0.0, 20.0, 10.0), FillRule::NonZero, &p);
    let q = points(&out);
    assert!(q.len() > 100, "{}", q.len());
    for a in &q {
        assert!(a.x >= -1e-6 && a.x <= 20.0 + 1e-6 && a.y >= -1e-6 && a.y <= 10.0 + 1e-6, "{a:?}");
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
    p.pull = 0.0;
    p.underlay = false;
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
        p.underlay = false;
        let out = stitches(&ring, FillRule::NonZero, &p);
        let trims = out.iter().filter(|s| s.command == Command::Trim).count();
        assert!(trims <= 1, "angle {angle}: {trims} trims");
        assert_eq!(over(&out, (10.0, 10.0, 20.0, 20.0)), 0, "angle {angle}");
    }
    // Rows sewn twice where regions meet: at 0° the rows just above and below the hole.
    let mut p = Params::for_style(Style::Fill);
    p.angle = 0.0;
    p.underlay = false;
    let out = stitches(&ring, FillRule::NonZero, &p);
    let plain = stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p);
    let count = |v: &[Stitch], y: f64| v.iter().filter(|s| s.command == Command::Stitch && (s.y - y).abs() < 1e-6).count();
    // The last row above the hole (rows sit at 0.2, 0.6, 1.0 … mm).
    let above = (0..100).map(|k| 0.2 + 0.4 * f64::from(k)).take_while(|y| *y < 10.0).last().unwrap_or(0.0);
    assert!(count(&out, above) > count(&plain, above), "the row above the hole is sewn by two regions");
}

/// A satin's underlay follows its width — centre run, edge run, edge run and zigzag — and the
/// top stitching starts where the underlay ended: no trim and no long move along the column.
#[test]
fn satin_underlay_follows_the_width() {
    for (width, want_centre, want_zigzag) in [(1.5, true, false), (3.0, false, false), (6.0, false, true)] {
        let mut p = Params::for_style(Style::Satin);
        p.width = width;
        p.pull = 0.0;
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
    p.pull = 0.0;
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
    // (Away from the shape's left and right edges, where the edge run is vertical too.)
    let vertical = |out: &[Stitch], width: f64| {
        out.windows(2)
            .filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch)
            .filter(|w| (w[0].x - w[1].x).abs() < 1e-6 && (w[0].y - w[1].y).abs() > 1.0 && w[0].x > 0.1 && w[0].x < width - 0.1)
            .count()
    };
    assert!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0) > 20);
    assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 8.0, 30.0), FillRule::NonZero, &p), 8.0), 0);
    p.underlay = false;
    assert_eq!(vertical(&stitches(&rect(0.0, 0.0, 30.0, 30.0), FillRule::NonZero, &p), 30.0), 0);
}

/// Satin spacing follows the column's width unless a density is given.
#[test]
fn satin_spacing_follows_the_width() {
    // The distance between two crossings on the same rail is twice the spacing.
    let rail_gap = |width: f64, auto: bool| {
        let mut p = Params::for_style(Style::Satin);
        p.width = width;
        p.pull = 0.0;
        p.underlay = false;
        p.lock = false;
        p.auto_spacing = auto;
        p.density = 0.3;
        let out = stitches(&line((0.0, 0.0), (20.0, 0.0)), FillRule::NonZero, &p);
        let top: Vec<f64> = out.iter().filter(|s| (s.y - width / 2.0).abs() < 1e-6).map(|s| s.x).collect();
        (top[2] - top[1]) / 2.0
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
    p.underlay = false;
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
    for underlay in [false, true] {
        for angle in (0..360).step_by(5).map(f64::from) {
            let mut p = Params::for_style(Style::Fill);
            p.angle = angle;
            p.underlay = underlay;
            let out = stitches(&e, FillRule::NonZero, &p);
            assert_eq!(
                over(&out, (10.0, 8.0, 40.0, 16.0)) + over(&out, (10.0, 24.0, 40.0, 32.0)),
                0,
                "angle {angle}, underlay {underlay}: across a gap"
            );
            let lifts = out.iter().filter(|s| s.command != Command::Stitch).count();
            assert_eq!(lifts, 0, "angle {angle}, underlay {underlay}: {lifts} jumps and trims");
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
    params.pull = 0.0;
    let out = stitches(&line((0.0, 0.0), (10.0, 0.0)), FillRule::NonZero, &params);
    let q = points(&out);
    assert!(q.len() > 20, "{}", q.len());
    assert!(q.iter().all(|a| a.y.abs() <= 2.0 + 1e-6), "{q:?}");
    // Neighbouring stitches are on opposite rails.
    let rails: Vec<f64> = q.iter().skip(1).map(|a| a.y).collect();
    assert!(rails.iter().any(|y| *y > 1.5) && rails.iter().any(|y| *y < -1.5));
    let mut wider = params;
    wider.pull = 0.5;
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
    assert!(matches!(try_stitches(&line((0.0, 0.0), (1e7, 0.0)), FillRule::NonZero, &run), Err(Error::TooLarge(_))));
}

#[test]
fn nothing_panics_on_odd_input() {
    let empty = BezPath::new();
    assert!(stitches(&empty, FillRule::NonZero, &Params::default()).is_empty());
    let mut p = Params::for_style(Style::Fill);
    p.density = f64::NAN;
    p.stitch_length = 0.0;
    p.angle = f64::INFINITY;
    p.pull = -5.0;
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

/// The grid's memory stays near the number of edges, however long they are: thousands of edges
/// from corner to corner of a shape (as traced art can have) would fill every cell of their
/// boxes.
#[test]
fn the_edge_grid_stays_small_for_long_edges() {
    use super::area::Grid;
    use super::geometry::subpaths;
    for n in [2_000usize, 20_000] {
        let shape = polygon((0..n).map(|k| {
            let t = k as f64 / n as f64;
            if k % 2 == 0 { (t, 0.0) } else { (100.0 - t, 100.0) }
        }));
        let subs = subpaths(&shape);
        let outlines: Vec<&super::geometry::SubPath> = subs.iter().collect();
        if let Some(grid) = Grid::new(&outlines) {
            assert!(grid.entries() <= n * 32 + 256 * 256, "{n} edges: {} entries", grid.entries());
        }
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
