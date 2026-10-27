//! Line pictures drawn: probes at pixel centres on pictures built by hand (no stored images, no
//! planner output), so nothing here moves when the planner changes.

use super::*;
use proptest::prelude::*;

const RED: [u8; 3] = [200, 20, 20];
const BLUE: [u8; 3] = [20, 40, 200];
const WHITE: [u8; 3] = [255, 255, 255];

/// A point of a line made by hand: x, y and its number.
type At = (f64, f64, u32);
/// A line made by hand: its colour slot, its tag, its width and its points.
type HandLine<'a> = (u32, u32, f32, &'a [At]);

/// A picture of `lines`, in picture units equal to document points (origin 0, one point a unit).
fn picture(lines: &[HandLine]) -> LinePicture {
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0);
    for (group, tag, width, points) in lines {
        b.begin_line(*group, *tag, *width);
        for (x, y, n) in *points {
            b.point(Point::new(*x, *y), *n);
        }
        b.end_line();
    }
    b.build()
}

fn style(colors: &[[u8; 3]]) -> LineStyle {
    LineStyle { colors: colors.to_vec(), ..LineStyle::default() }
}

/// `picture` drawn `w`×`h`, one document point a pixel, on one thread.
fn draw(picture: &LinePicture, w: u32, h: u32, style: &LineStyle) -> Rendered {
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    r.render(picture, w, h, Affine::IDENTITY, style)
}

/// The straight colour at pixel (x, y), alpha last.
fn at(img: &Rendered, x: u32, y: u32) -> [u8; 4] {
    img.pixel(x, y)
}

fn rgb(px: [u8; 4]) -> [u8; 3] {
    [px[0], px[1], px[2]]
}

#[test]
fn a_line_draws_its_colour_along_it_and_nothing_beside_it() {
    let p = picture(&[(0, 0, 4.0, &[(10.0, 10.5, 1), (90.0, 10.5, 2)])]);
    let img = draw(&p, 100, 20, &style(&[RED]));
    assert_eq!(at(&img, 50, 10), [200, 20, 20, 255]);
    assert_eq!(at(&img, 50, 16)[3], 0, "beyond its half width");
    // Round ends: past the end by less than the half width there is ink, beyond it none.
    assert!(at(&img, 91, 10)[3] > 0);
    assert_eq!(at(&img, 95, 10)[3], 0);
}

#[test]
fn a_lone_point_draws_a_dot() {
    let p = picture(&[(0, 0, 6.0, &[(20.5, 20.5, 1)])]);
    let img = draw(&p, 40, 40, &style(&[BLUE]));
    assert_eq!(rgb(at(&img, 20, 20)), BLUE);
    assert_eq!(at(&img, 20, 26)[3], 0);
}

#[test]
fn a_later_line_covers_an_earlier_one() {
    let p = picture(&[(0, 0, 4.0, &[(10.5, 20.5, 1), (30.5, 20.5, 2)]), (1, 1, 4.0, &[(20.5, 10.5, 3), (20.5, 30.5, 4)])]);
    let img = draw(&p, 40, 40, &style(&[RED, BLUE]));
    assert_eq!(rgb(at(&img, 20, 20)), BLUE);
    assert_eq!(rgb(at(&img, 12, 20)), RED);
}

#[test]
fn hairlines_are_a_pixel_and_laid_lines_never_thinner_than_the_minimum() {
    let p = picture(&[(0, 0, 10.0, &[(5.0, 20.5, 1), (95.0, 20.5, 2)])]);
    let hair = draw(&p, 100, 40, &LineStyle { widths: Widths::Hairline, ..style(&[RED]) });
    assert!(at(&hair, 50, 20)[3] > 200, "{:?}", at(&hair, 50, 20));
    assert_eq!(at(&hair, 50, 23)[3], 0);
    let thin = picture(&[(0, 0, 0.001, &[(5.0, 20.5, 1), (95.0, 20.5, 2)])]);
    let img = draw(&thin, 100, 40, &style(&[RED]));
    assert!(at(&img, 50, 20)[3] > 0, "a minimum of {MIN_LINE_PX} px");
}

#[test]
fn hidden_tags_are_left_out_and_faded_ones_are_mixed_toward_the_backdrop() {
    let p = picture(&[(0, 7, 4.0, &[(10.5, 10.5, 1), (30.5, 10.5, 2)]), (0, 8, 4.0, &[(10.5, 30.5, 3), (30.5, 30.5, 4)])]);
    let hidden = draw(&p, 40, 40, &LineStyle { hidden: Some(TagSet::of([7])), ..style(&[RED]) });
    assert_eq!(at(&hidden, 20, 10)[3], 0);
    assert_eq!(rgb(at(&hidden, 20, 30)), RED);
    let faded = draw(&p, 40, 40, &LineStyle { faded: Some(TagSet::of([8])), ..style(&[RED]) });
    assert_eq!(rgb(at(&faded, 20, 10)), RED);
    // Halfway from the thread to the white backdrop, worked out by hand: (200, 20, 20) and white.
    assert_eq!(FADE, 0.5);
    assert_eq!(rgb(at(&faded, 20, 30)), [228, 138, 138]);
}

#[test]
fn a_range_of_numbers_draws_only_those_even_within_a_line() {
    let line: &[At] = &[(10.0, 10.5, 1), (30.0, 10.5, 2), (50.0, 10.5, 3), (70.0, 10.5, 4), (90.0, 10.5, 5)];
    let p = picture(&[(0, 0, 4.0, line)]);
    // Segments are numbered by their end: 2 (10–30) and 3 (30–50).
    let img = draw(&p, 100, 20, &LineStyle { numbers: Some(2..4), ..style(&[RED]) });
    assert_eq!(rgb(at(&img, 20, 10)), RED);
    assert_eq!(rgb(at(&img, 40, 10)), RED);
    assert_eq!(at(&img, 60, 10)[3], 0);
    assert_eq!(at(&img, 80, 10)[3], 0);
}

#[test]
fn lines_outside_the_view_are_culled_and_a_culled_frame_is_the_whole_frames_cut_out() {
    let lines: Vec<(u32, u32, f32, Vec<At>)> = (0..20u32)
        .map(|k| (k % 3, k, 3.0, vec![(f64::from(k) * 10.0 + 2.0, 5.0, k * 2 + 1), (f64::from(k) * 10.0 + 7.0, 95.0, k * 2 + 2)]))
        .collect();
    let refs: Vec<HandLine> = lines.iter().map(|(g, t, w, p)| (*g, *t, *w, p.as_slice())).collect();
    let p = picture(&refs);
    let s = style(&[RED, BLUE, [30, 160, 40]]);
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    let whole = r.render(&p, 200, 100, Affine::IDENTITY, &s);
    assert_eq!(r.stats.culled, 0);
    // The right half alone: the left lines are culled, the pixels the same.
    let half = r.render(&p, 100, 100, Affine::translate((-100.0, 0.0)), &s);
    assert!(r.stats.culled >= 8, "{:?}", r.stats);
    for y in 0..100 {
        for x in 0..100 {
            let (a, b) = (at(&whole, x + 100, y), at(&half, x, y));
            assert!(a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= 1), "({x}, {y}): {a:?} {b:?}");
        }
    }
}

#[test]
fn pixels_do_not_depend_on_the_thread_count() {
    let pts: Vec<At> = (0..300u32).map(|i| (f64::from(i % 17) * 11.0 + 3.0, f64::from(i) * 0.6 + 2.0, i + 1)).collect();
    let p = picture(&[(0, 0, 2.5, &pts), (1, 1, 1.2, &pts[100..])]);
    let s = style(&[RED, BLUE]);
    let mut one = LineRenderer { threads: 0, ..LineRenderer::new() };
    let mut four = LineRenderer { threads: 4, ..LineRenderer::new() };
    let a = one.render(&p, 200, 200, Affine::IDENTITY, &s);
    let b = four.render(&p, 200, 200, Affine::IDENTITY, &s);
    assert!(a.pixels == b.pixels, "pixels differ between 0 and 4 threads");
}

#[test]
fn numbers_that_arent_draw_nothing_and_never_panic() {
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0);
    b.begin_line(0, 0, 2.0);
    for (x, y) in [(f64::NAN, 1.0), (1.0, f64::INFINITY), (1e300, 1e300), (5.0, 5.0), (f64::NEG_INFINITY, 0.0), (9.0, 9.0)] {
        b.point(Point::new(x, y), 1);
    }
    b.link(Point::new(f64::NAN, 0.0), Point::new(1.0, 1.0), LinkKind::Cut, 0, 0, 1);
    b.mark(Point::new(1e300, 0.0), MarkKind::Cross, 0, 1);
    let p = b.build();
    // Only the finite points that fit an offset are kept.
    assert!(p.lines().iter().all(|l| p.line_points(l).all(|q| q.x.is_finite() && q.y.is_finite())));
    for view in [Affine::IDENTITY, Affine::scale(1e9), Affine::scale(1e-9), Affine::translate((1e12, -1e12))] {
        let _ = draw_view(&p, view);
    }
    // A degenerate view draws nothing.
    let img = draw_view(&p, Affine::scale(0.0));
    assert!(img.pixels.iter().all(|v| *v == 0));
}

fn draw_view(p: &LinePicture, view: Affine) -> Rendered {
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    r.render(p, 64, 64, view, &style(&[RED]))
}

#[test]
fn zoomed_far_out_and_far_in_it_draws_without_a_panic() {
    let pts: Vec<At> = (0..64u32).map(|i| (f64::from(i), f64::from(i % 2), i + 1)).collect();
    let p = picture(&[(0, 0, 0.4, &pts)]);
    for zoom in [0.0313, 640.0] {
        let _ = draw_view(&p, Affine::scale(zoom));
    }
}

#[test]
fn a_colour_close_to_the_backdrop_gets_an_edge_that_shows() {
    assert_eq!(edge_against(RED, WHITE), None, "red stands out on white");
    let edge = edge_against(WHITE, WHITE).unwrap();
    assert!(contrast(edge, WHITE) >= EDGE_CONTRAST, "{edge:?}");
    let black = edge_against([5, 5, 5], [0, 0, 0]).unwrap();
    assert!(contrast(black, [0, 0, 0]) >= EDGE_CONTRAST, "{black:?}");
    // White thread on white: the edge shows beside the line, the line keeps its colour.
    let p = picture(&[(0, 0, 6.0, &[(10.0, 20.5, 1), (90.0, 20.5, 2)])]);
    let s = LineStyle { edges: vec![Some(edge)], background: Some([255, 255, 255, 255]), ..style(&[WHITE]) };
    let img = draw(&p, 100, 40, &s);
    assert_eq!(rgb(at(&img, 50, 20)), WHITE);
    let rim = at(&img, 50, 23);
    assert!(rim[0] < 230, "{rim:?}");
}

#[test]
fn an_edge_never_shows_between_two_lines_of_one_colour() {
    // Two pieces of one white run overlapping: no edge colour where they overlap.
    let p = picture(&[(0, 0, 6.0, &[(10.0, 20.5, 1), (60.0, 20.5, 2)]), (0, 1, 6.0, &[(50.0, 20.5, 2), (90.0, 20.5, 3)])]);
    let edge = edge_against(WHITE, WHITE);
    let img = draw(&p, 100, 40, &LineStyle { edges: vec![edge], background: Some([255, 255, 255, 255]), ..style(&[WHITE]) });
    for x in 40..70 {
        assert_eq!(rgb(at(&img, x, 20)), WHITE, "x {x}");
    }
}

#[test]
fn links_and_marks_show_in_two_tones_on_top_and_only_when_asked() {
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0);
    b.begin_line(0, 0, 4.0);
    b.point(Point::new(5.0, 50.5), 1);
    b.point(Point::new(195.0, 50.5), 2);
    b.end_line();
    b.link(Point::new(5.0, 20.5), Point::new(195.0, 20.5), LinkKind::Cut, 0, 0, 3);
    b.link(Point::new(5.0, 80.5), Point::new(195.0, 80.5), LinkKind::Thread, 0, 0, 4);
    b.mark(Point::new(100.0, 50.0), MarkKind::Cross, 0, 2);
    let p = b.build();
    let s = style(&[RED]);
    let img = draw(&p, 200, 100, &s);
    let along: Vec<[u8; 3]> = (10..190).map(|x| rgb(at(&img, x, 20))).collect();
    assert!(along.contains(&s.dark) && along.contains(&s.light), "both tones along a cut");
    assert_eq!(rgb(at(&img, 100, 80)), RED, "a thread link in its thread's colour");
    // The cross over the line: its dark stroke at the centre.
    assert_eq!(rgb(at(&img, 100, 50)), s.dark);
    let bare = draw(&p, 200, 100, &LineStyle { links: false, marks: false, ..s.clone() });
    assert_eq!(at(&bare, 100, 20)[3], 0);
    assert_eq!(at(&bare, 100, 80)[3], 0);
    assert_eq!(rgb(at(&bare, 100, 50)), RED);
    // Short on screen: left out.
    let mut tiny = LinePictureBuilder::new(Point::ZERO, 1.0);
    tiny.link(Point::new(10.0, 10.0), Point::new(11.0, 10.0), LinkKind::Cut, 0, 0, 1);
    let img = draw(&tiny.build(), 20, 20, &s);
    assert!(img.pixels.iter().all(|v| *v == 0));
}

#[test]
fn long_runs_are_cut_into_pieces_that_share_their_joins() {
    let pts: Vec<At> = (0..200u32).map(|i| (f64::from(i), 0.0, i + 1)).collect();
    let p = picture(&[(0, 0, 1.0, &pts)]);
    assert!(p.lines().len() >= 4);
    assert!(p.lines().iter().all(|l| (l.points.end - l.points.start) as usize <= MAX_LINE_POINTS));
    let segments: usize = p.lines().iter().map(|l| (l.points.end - l.points.start) as usize - 1).sum();
    assert_eq!(segments, 199, "every segment once");
    for pair in p.lines().windows(2) {
        let (a, b) = (p.line_points(&pair[0]).last().unwrap(), p.line_points(&pair[1]).next().unwrap());
        assert_eq!(a, b, "consecutive pieces share their join");
    }
}

#[test]
fn points_a_line_passes_straight_through_change_nothing_drawn() {
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0).straight_within(0.01);
    b.begin_line(0, 0, 3.0);
    // A row: straight through 30 and 50, then a turn at 70.
    for (x, y, n) in [(10.0, 10.5, 1), (30.0, 10.5, 2), (50.0, 10.5, 3), (70.0, 10.5, 4), (70.0, 30.5, 5)] {
        b.point(Point::new(x, y), n);
    }
    let p = b.build();
    let straight: Vec<bool> = (0..p.point_count()).map(|i| p.is_straight(i)).collect();
    assert_eq!(straight, [false, true, true, false, false]);
    let s = style(&[RED]);
    let whole = draw(&p, 100, 40, &s);
    // Drawn with every point (a range covering them all), the pixels are the same.
    let every = draw(&p, 100, 40, &LineStyle { numbers: Some(0..100), ..s.clone() });
    assert!(whole.pixels.iter().zip(&every.pixels).all(|(a, b)| a.abs_diff(*b) <= 1));
    // Without a tolerance, none is.
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0);
    b.begin_line(0, 0, 3.0);
    for (x, n) in [(10.0, 1), (30.0, 2), (50.0, 3)] {
        b.point(Point::new(x, 10.0), n);
    }
    let p = b.build();
    assert!((0..p.point_count()).all(|i| !p.is_straight(i)));
}

/// On a gentle curve sewn in short stitches, every point is within the tolerance of its own
/// neighbours' chord, yet the points left out never add up to a chord that strays: each is within
/// the tolerance of the line drawn in its place, and the drawing matches one of every point.
#[test]
fn points_left_out_on_a_curve_stay_within_the_tolerance_of_the_line_drawn() {
    // Millimetres: 0.5 mm stitches round a circle 64 mm across, with a 2 µm tolerance (the
    // stitch preview's), where each point is about 0.5 µm off its neighbours' chord.
    let (radius, step, tolerance) = (64.0_f64, 0.5_f64, 0.002_f64);
    let mut b = LinePictureBuilder::new(Point::new(0.0, 0.0), 1.0).straight_within(tolerance);
    b.begin_line(0, 0, 0.2);
    let n = 400;
    let pts: Vec<Point> = (0..n)
        .map(|k| {
            let a = f64::from(k) * step / radius;
            Point::new(radius * a.sin(), radius * (1.0 - a.cos()))
        })
        .collect();
    for (k, q) in pts.iter().enumerate() {
        b.point(*q, k as u32 + 1);
    }
    let p = b.build();
    let line_of = |i: usize| p.lines().iter().find(|l| (l.points.start as usize..l.points.end as usize).contains(&i)).unwrap().clone();
    let mut left_out = 0;
    for i in 0..p.point_count() {
        if !p.is_straight(i) {
            continue;
        }
        left_out += 1;
        let line = line_of(i);
        let (s, e) = (line.points.start as usize, line.points.end as usize);
        let before = (s..i).rev().find(|k| !p.is_straight(*k)).unwrap();
        let after = (i + 1..e).find(|k| !p.is_straight(*k)).unwrap();
        let (a, m, c) = (p.point(before).unwrap().0, p.point(i).unwrap().0, p.point(after).unwrap().0);
        let (dx, dy) = (c.x - a.x, c.y - a.y);
        let off = ((m.x - a.x) * dy - (m.y - a.y) * dx).abs() / (dx * dx + dy * dy).sqrt();
        // f32 offsets: a hair of rounding over the tolerance.
        assert!(off <= tolerance * 1.01, "point {i} lies {off} mm off the line drawn");
    }
    assert!(left_out > n as usize / 4, "points are left out: {left_out} of {n}");
    // Drawn at 40 pixels a millimetre (the tolerance a tenth of a pixel), the same pixels.
    let s = style(&[RED]);
    let view = Affine::scale(40.0) * Affine::translate((0.0, -1.0));
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    let whole = r.render(&p, 400, 240, view, &s);
    let every = r.render(&p, 400, 240, view, &LineStyle { numbers: Some(0..u32::MAX), ..s.clone() });
    assert!(whole.pixels.iter().zip(&every.pixels).all(|(a, b)| a.abs_diff(*b) <= 1), "the same pixels");
}

/// Zoomed in so close that the tolerance would show (over a tenth of a pixel), every point is
/// drawn; further out, the straight ones are left out.
#[test]
fn zoomed_in_close_every_point_is_drawn() {
    let mut b = LinePictureBuilder::new(Point::ZERO, 1.0).straight_within(0.01);
    b.begin_line(0, 0, 3.0);
    for (k, x) in (0..10).map(|k| (k, f64::from(k) * 10.0)) {
        b.point(Point::new(x, 10.0), k + 1);
    }
    let p = b.build();
    let segments = |scale: f64| {
        let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
        // The row's height (10 points) a few pixels down, whatever the scale.
        r.render(&p, 200, 100, Affine::translate((0.0, 20.0)) * Affine::scale(scale) * Affine::translate((0.0, -10.0)), &style(&[RED]));
        r.stats.segments
    };
    assert_eq!(segments(1.0), 1, "a straight row: one segment, its ends");
    let close = STRAIGHT_SKIP_PX / 0.01 * 2.0;
    assert_eq!(segments(close), 9, "every point");
}

#[test]
fn a_picture_is_placed_by_its_origin_and_scale() {
    // Picture units of a tenth of a point, offsets from (1000, 1000).
    let mut b = LinePictureBuilder::new(Point::new(1000.0, 1000.0), 0.1);
    b.begin_line(0, 0, 20.0);
    b.point(Point::new(1100.0, 1105.0), 1);
    b.point(Point::new(1900.0, 1105.0), 2);
    // A move below the line and a cut at its middle, placed the same way.
    b.link(Point::new(1100.0, 1255.0), Point::new(1900.0, 1255.0), LinkKind::Thread, 0, 0, 3);
    b.mark(Point::new(1500.0, 1405.0), MarkKind::Square, 0, 4);
    let p = b.build();
    // (110, 110.5) to (190, 110.5) in document points, 2 pt wide.
    assert_eq!(p.bounds().map(|r| (r.x0.round(), r.x1.round())), Some((110.0, 190.0)));
    assert_eq!(p.line_points(&p.lines()[0]).next(), Some(Point::new(1100.0, 1105.0)));
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    let img = r.render(&p, 100, 50, Affine::translate((-100.0, -100.0)), &style(&[BLUE]));
    assert_eq!(rgb(at(&img, 50, 10)), BLUE);
    assert_eq!(at(&img, 5, 10)[3], 0);
    assert_eq!(rgb(at(&img, 50, 25)), BLUE, "the link at (125.5) pt");
    assert!(at(&img, 50 + (MARK_PT as u32), 40)[3] > 0, "the square round (150, 140.5) pt");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// Any picture, any view, any style: drawn without a panic.
    #[test]
    fn any_picture_draws(
        points in prop::collection::vec((any::<f64>(), any::<f64>()), 0..80),
        breaks in prop::collection::vec(any::<u8>(), 0..8),
        width in any::<f32>(),
        scale in prop::num::f64::ANY,
        tx in -1e6f64..1e6,
        range in prop::option::of((0u32..100, 0u32..100)),
    ) {
        let mut b = LinePictureBuilder::new(Point::new(tx, -tx), 0.5);
        b.begin_line(0, 0, width);
        for (i, (x, y)) in points.iter().enumerate() {
            if breaks.contains(&(i as u8)) {
                b.begin_line((i % 3) as u32, i as u32, width);
                b.link(Point::new(*x, *y), Point::new(*y, *x), if i % 2 == 0 { LinkKind::Cut } else { LinkKind::Thread }, 1, i as u32, i as u32);
                b.mark(Point::new(*y, *x), MarkKind::Square, i as u32, i as u32);
            }
            b.point(Point::new(*x, *y), i as u32);
        }
        let p = b.build();
        let s = LineStyle {
            edges: vec![edge_against(WHITE, WHITE)],
            numbers: range.map(|(a, b)| a.min(b)..a.max(b)),
            faded: Some(TagSet::of([1, 2, 3])),
            ..style(&[WHITE, RED])
        };
        let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
        let _ = r.render(&p, 48, 32, Affine::scale(scale) * Affine::translate((tx, tx)), &s);
    }
}

/// Needle points: a dot at each point of a line in the tone that contrasts with it, once points
/// stand at least the gap apart on screen; none while they are closer, and none unless asked.
#[test]
fn needle_points_stand_out_once_they_stand_apart() {
    // Points 20 pixels apart along a thin light line: a dark dot on each.
    let light = [240, 240, 120];
    let p = picture(&[(0, 0, 1.0, &[(10.5, 10.5, 1), (30.5, 10.5, 2), (50.5, 10.5, 3), (70.5, 10.5, 4)])]);
    let on = LineStyle { needle_points: true, background: Some([255, 255, 255, 255]), ..style(&[light]) };
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    let img = r.render(&p, 80, 20, Affine::IDENTITY, &on);
    assert_eq!(r.stats.needle_points, 4);
    let dark = LineStyle::default().dark;
    assert_eq!(rgb(at(&img, 30, 10)), dark, "a dark dot on the light thread");
    assert_eq!(rgb(at(&img, 20, 10)), light, "the thread between them");
    // Not asked: no dot.
    let img = draw(&p, 80, 20, &LineStyle { needle_points: false, ..on.clone() });
    assert_eq!(rgb(at(&img, 30, 10)), light);
    // Points 2 pixels apart: too close for dots.
    let close: Vec<At> = (0..30).map(|k| (10.0 + 2.0 * f64::from(k), 10.5, k)).collect();
    let p = picture(&[(0, 0, 1.0, &close)]);
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    r.render(&p, 80, 20, Affine::IDENTITY, &on);
    assert_eq!(r.stats.needle_points, 0);
    // Zoomed in four times, they stand 8 apart: dots again.
    r.render(&p, 320, 80, Affine::scale(4.0), &on);
    assert!(r.stats.needle_points > 0);
}

/// A press picks the line drawn on top under it, within the tolerance past its half width; a box
/// that holds the point but no segment near it passes it by; in a picture scaled to document
/// points, the tolerance is in document points.
#[test]
fn a_hit_picks_the_line_drawn_on_top() {
    let p = picture(&[
        (0, 7, 4.0, &[(0.0, 50.0, 0), (100.0, 50.0, 1)]),
        (1, 9, 2.0, &[(50.0, 0.0, 2), (50.0, 100.0, 3)]),
        (0, 11, 1.0, &[(0.0, 0.0, 4), (100.0, 100.0, 5)]),
    ]);
    assert_eq!(p.hit(Point::new(50.0, 50.0), 0.0), Some(11), "the last drawn, on top");
    assert_eq!(p.hit(Point::new(20.0, 51.5), 0.0), Some(7), "within half its width");
    assert_eq!(p.hit(Point::new(20.0, 53.0), 0.0), None);
    assert_eq!(p.hit(Point::new(20.0, 53.0), 1.5), Some(7), "within the tolerance past it");
    assert_eq!(p.hit(Point::new(80.0, 20.0), 0.5), None, "inside the diagonal's box, far from it");
    assert_eq!(p.hit(Point::new(f64::NAN, 1.0), 1.0), None);
    let mut b = LinePictureBuilder::new(Point::ZERO, 2.0);
    b.begin_line(0, 3, 1.0);
    b.point(Point::new(0.0, 0.0), 0);
    b.point(Point::new(10.0, 0.0), 1);
    b.end_line();
    let scaled = b.build();
    assert_eq!(scaled.hit(Point::new(10.0, 2.5), 1.0), None, "1 pt past a half unit is a pixel short");
    assert_eq!(scaled.hit(Point::new(10.0, 2.0), 1.0), Some(3), "two document points a unit");
}

/// The edit look of a tag: its lines drawn [`EDIT_LINE_PT`] wide over a later line that crosses
/// them, its needle points as circles (a light face in a dark edge) each once, none past the cap;
/// without the look the later line covers it.
#[test]
fn the_edit_look_draws_its_units_thin_on_top_with_circles() {
    let wide = 9.0;
    // Tag 0 across, tag 1 down over it later.
    let p = picture(&[(0, 0, wide, &[(10.5, 30.5, 1), (40.5, 30.5, 2), (70.5, 30.5, 3)]), (1, 1, wide, &[(55.5, 5.5, 4), (55.5, 55.5, 5)])]);
    let plain = LineStyle { background: Some([255, 255, 255, 255]), ..style(&[RED, BLUE]) };
    let img = draw(&p, 80, 60, &plain);
    assert_eq!(rgb(at(&img, 55, 30)), BLUE, "the later line on top");
    assert_eq!(rgb(at(&img, 25, 33)), RED, "laid wide");
    let edit = EditLook { tags: TagSet::of([0]), circles: Some(100), shortest: 0.0, longest: f64::INFINITY, step: None };
    // Four pixels a point: the line 4 px wide, a circle 10 px across its radius in a 4 px edge.
    let look = LineStyle { edit: Some(edit.clone()), px_per_pt: 4.0, ..plain.clone() };
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    let img = r.render(&p, 80, 60, Affine::IDENTITY, &look);
    assert_eq!(rgb(at(&img, 57, 30)), RED, "the edited line over the later one");
    assert_eq!(rgb(at(&img, 25, 33)), [255, 255, 255], "thin: nothing three points off it");
    assert_eq!(r.stats.circles, 3, "a circle at each needle point of the edited line");
    assert_eq!(rgb(at(&img, 40, 30)), LineStyle::default().light, "a light face");
    assert_eq!(rgb(at(&img, 40, 20)), LineStyle::default().dark, "inside a dark edge");
    let few = LineStyle { edit: Some(EditLook { circles: Some(2), ..edit.clone() }), ..look.clone() };
    r.render(&p, 80, 60, Affine::IDENTITY, &few);
    assert_eq!((r.stats.circles, r.stats.circles_capped), (0, true), "more than may show: none");
    let none = LineStyle { edit: Some(EditLook { circles: None, ..edit }), ..plain };
    r.render(&p, 80, 60, Affine::IDENTITY, &none);
    assert_eq!(r.stats.circles, 0, "circles not asked for");
}

/// A long run cut into pieces: each needle point gets one circle, the joins shared by two pieces
/// once.
#[test]
fn a_run_cut_into_pieces_gets_a_circle_a_needle_point() {
    let points: Vec<At> = (0..150u32).map(|k| (5.0 + 3.0 * f64::from(k % 30), 5.0 + 6.0 * f64::from(k / 30), k + 1)).collect();
    let p = picture(&[(0, 0, 1.0, &points)]);
    assert!(p.lines().len() > 2, "cut into pieces");
    let look = LineStyle {
        edit: Some(EditLook { tags: TagSet::of([0]), circles: Some(10_000), shortest: 0.0, longest: f64::INFINITY, step: None }),
        ..style(&[RED])
    };
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    r.render(&p, 120, 50, Affine::IDENTITY, &look);
    assert_eq!(r.stats.circles, 150);
}

/// The stitches of an edited line that break a rule are marked, those that don't aren't: shorter
/// than the shortest, longer than the longest, longer along x or y than the step; another tag's
/// never.
#[test]
fn the_edit_look_marks_the_stitches_that_break_a_rule() {
    // Stitches 10 long, 2 (short), 10, 30 (long), and one 14 across and 14 down (19.8 long: under
    // the longest, over the step along each axis).
    let p = picture(&[
        (0, 0, 1.0, &[(5.0, 5.0, 1), (15.0, 5.0, 2), (17.0, 5.0, 3), (27.0, 5.0, 4), (57.0, 5.0, 5), (71.0, 19.0, 6)]),
        (0, 1, 1.0, &[(5.0, 40.0, 7), (6.0, 40.0, 8)]),
    ]);
    let edit = EditLook { tags: TagSet::of([0]), circles: None, shortest: 5.0, longest: 25.0, step: None };
    let mut r = LineRenderer { threads: 0, ..LineRenderer::new() };
    r.render(&p, 80, 50, Affine::IDENTITY, &LineStyle { edit: Some(edit.clone()), ..style(&[RED]) });
    assert_eq!(r.stats.flagged, 2, "the short and the long one");
    r.render(&p, 80, 50, Affine::IDENTITY, &LineStyle { edit: Some(EditLook { step: Some(12.0), ..edit.clone() }), ..style(&[RED]) });
    assert_eq!(r.stats.flagged, 3, "and the one longer than the step along x and y");
    assert!(edit.breaks(Point::new(0.0, 0.0), Point::new(1.0, 0.0)) && !edit.breaks(Point::new(0.0, 0.0), Point::new(10.0, 0.0)));
}

/// The default style has no edit look: every picture draws as before it existed.
#[test]
fn without_the_edit_look_nothing_changes() {
    assert_eq!(LineStyle::default().edit, None);
    let p = picture(&[(0, 0, 4.0, &[(10.5, 10.5, 1), (60.5, 10.5, 2)])]);
    let plain = style(&[RED]);
    let empty = LineStyle {
        edit: Some(EditLook { tags: TagSet::default(), circles: Some(10), shortest: 0.0, longest: f64::INFINITY, step: None }),
        ..plain.clone()
    };
    assert_eq!(draw(&p, 80, 20, &plain).pixels, draw(&p, 80, 20, &empty).pixels, "an edit look of no tags draws the same pixels");
}
