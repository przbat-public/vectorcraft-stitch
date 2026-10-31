//! An object's embroidery settings moved and turned with the object ([`Embroidery::transform`],
//! called from [`crate::Node::transform`], the one path every transform of an object takes:
//! Transform, Move, Rotate, Reflect, Scale, Shear, Transform Each, nudges, Align, paste and
//! duplicate offsets, a symbol's art placed by its instance, Break Link).
//!
//! The rules (`docs/embroidery.md` › Settings and transforms), each one a group action, so that
//! two transforms one after the other do what their product does and a transform and its inverse
//! give the settings back:
//! - points (entry, exit) map by the whole transform;
//! - a set angle goes where the transform takes its direction, turned half round when the transform
//!   mirrors; per-side values (`pull`, `pull_percent`) then swap, so that a mirrored object sews the
//!   mirror image (a satin's rails, a fill row's two ends alike); a column's ends (`push`), counted
//!   as its path is drawn, never swap;
//! - no length scales: spacing, stitch length, compensation and the rest describe thread and
//!   fabric, which don't change with the art's size (the satin's width is the stroke's weight,
//!   scaled with it as before);
//! - unset values stay unset.
//!
//! What a newer version keeps that this version doesn't read moves by two conventions, which later
//! versions keep to: every `[x, y]` inside a `points` object is a position, moved by the transform
//! (this version's entry and exit are two of them); and an object holding a `frame` (six numbers,
//! its own coordinates to the document's, in `Affine::new`'s order) has its frame composed with
//! the transform, and nothing inside it moves otherwise.

use serde_json::{Map, Value};
use vectorcraft_geom::{Affine, Point, Vec2, normalize_deg};

use super::part::POINTS;
use super::{Embroidery, PartSettings};

/// How near an angle turned by a transform must come to where it was to be kept as it was,
/// degrees: far below anything a person sets, so that a move, a uniform scale or a turn by a full
/// circle keeps the angle's bits (the planning cache keys on them). An estimate.
const SAME_ANGLE: f64 = 1e-9;
/// The steps a degree is divided into for a turned angle: an angle a transform turns is rounded to
/// a ten-billionth of a degree, far below what is shown (tenths) or sewn, and coarse enough that
/// the last bits of the platform's sine and cosine never reach the file (a flipped 30° is −30°,
/// not −29.999999999999996). An estimate.
const ANGLE_STEPS: f64 = 1e10;
/// A direction a transform shrinks below this length has no direction left: the angle stays as it
/// was. The bounding box's angle uses the same bound (`orient::transformed_angle`).
const NO_DIRECTION: f64 = 1e-12;
/// How deep the kept JSON is walked for positions and frames: far deeper than any real nesting, a
/// bound on the walk (the JSON reader stops at 128 levels). An estimate.
const MAX_KEPT_DEPTH: usize = 16;
/// The key a kept object's own coordinates are given under.
const FRAME: &str = "frame";

impl Embroidery {
    /// Move and turn the settings as the object they belong to is transformed by `a` (see the
    /// module's rules). The identity changes nothing.
    pub fn transform(&mut self, a: Affine) {
        if a == Affine::IDENTITY {
            return;
        }
        let Self { stitches, trim_after: _, stop_after: _, fill, stroke, original, extra, unread: _ } = self;
        for part in [fill, stroke].into_iter().flatten() {
            part.transform(a);
        }
        // The original goes where its stitches go: its frame composed with the transform, as the
        // convention for kept data composes any frame (so older versions move it the same). An
        // object holding a frame has nothing else in it moved: what a newer version keeps beside the
        // frame is in the original's own coordinates, which the frame places.
        if let Some(o) = original {
            let composed = a * o.frame;
            if composed.as_coeffs().iter().all(|c| c.is_finite()) {
                o.frame = composed;
            }
        }
        move_shared(extra, a);
        if let Some(run) = stitches {
            // The exact millimetres kept for its needle points stay: each applies only while its
            // needle point lies where it was kept for (`digitize::exact`), so a run moved and moved
            // back sews as it did, as a needle point moved and moved back does.
            // Each cut and stop is kept where its needle point lies: it moves with it (by the same
            // arithmetic, so the two still agree to the bit), or, when that leaves the numbers, is
            // found by its place alone.
            for m in run.trims.iter_mut().chain(run.stops.iter_mut()) {
                if let Some(at) = m.at {
                    let moved = a * at;
                    m.at = (moved.x.is_finite() && moved.y.is_finite()).then_some(moved);
                }
            }
            // How its stitches were turned and scaled since they were made: a move alone records
            // nothing (so moving a run leaves its saved form as it was), anything more starts the
            // record, and every transform after is composed with it.
            let [m0, m1, m2, m3, _, _] = a.as_coeffs();
            if run.since_made.is_some() || [m0, m1, m2, m3] != [1.0, 0.0, 0.0, 1.0] {
                let composed = a * run.since_made.unwrap_or(Affine::IDENTITY);
                if composed.as_coeffs().iter().all(|c| c.is_finite()) {
                    run.since_made = Some(composed);
                }
            }
            move_shared(&mut run.extra, a);
        }
    }
}

/// The least and the most transform `a` stretches a length by, in any direction: its linear
/// part's singular values, smallest first (a turn or a mirror gives `[1, 1]`; a scale by 1.2 along
/// one axis, `[1, 1.2]`). `None` when they aren't finite numbers.
pub fn scale_factors(a: Affine) -> Option<[f64; 2]> {
    let [m0, m1, m2, m3, _, _] = a.as_coeffs();
    // The closed form for a 2 × 2 matrix [[m0, m2], [m1, m3]]: its rotation-like and
    // reflection-like halves' sizes, q and r, give σ = q + r and |q − r|; `hypot` keeps a large
    // coefficient from overflowing on the way.
    let q = ((m0 + m3) / 2.0).hypot((m1 - m2) / 2.0);
    let r = ((m0 - m3) / 2.0).hypot((m1 + m2) / 2.0);
    let (most, least) = (q + r, (q - r).abs());
    (most.is_finite() && least.is_finite()).then_some([least, most])
}

/// [`move_kept_map`] of kept data that may be shared: copied only when there is any.
fn move_shared(kept: &mut super::Kept, a: Affine) {
    if !kept.is_empty() {
        move_kept_map(std::sync::Arc::make_mut(kept), a, 0);
    }
}

impl PartSettings {
    /// [`Embroidery::transform`] of one part.
    fn transform(&mut self, a: Affine) {
        let mirrors = a.determinant() < 0.0;
        if let Some(angle) = &mut self.angle {
            *angle = turned_angle(*angle, a);
        }
        if mirrors {
            for pair in [&mut self.pull, &mut self.pull_percent].into_iter().flatten() {
                pair.swap(0, 1);
            }
        }
        if let Some(points) = &mut self.points {
            // A point the transform sends off to no finite place stays where it was, as a kept
            // position does: nothing set is lost to a degenerate transform.
            for p in [&mut points.entry, &mut points.exit].into_iter().flatten() {
                let q = a * *p;
                if q.x.is_finite() && q.y.is_finite() {
                    *p = q;
                }
            }
            if !points.extra.is_empty() {
                move_positions_in_map(std::sync::Arc::make_mut(&mut points.extra), a, 0);
            }
        }
        move_shared(&mut self.extra, a);
    }
}

/// Where transform `a` takes a set fill angle `angle` (degrees counter-clockwise on the page): the
/// direction `a`'s linear part takes the angle's direction to, turned half round when `a` mirrors,
/// normalised to (−180, 180] and rounded to [`ANGLE_STEPS`]. Kept as it was within [`SAME_ANGLE`],
/// and when `a` leaves no direction ([`NO_DIRECTION`]) or isn't finite.
pub fn turned_angle(angle: f64, a: Affine) -> f64 {
    let [m0, m1, m2, m3, _, _] = a.as_coeffs();
    // The direction on the page, in the document's coordinates (whose y axis points down).
    let (s, c) = (-angle.to_radians()).sin_cos();
    let mut v = Vec2::new(m0 * c + m2 * s, m1 * c + m3 * s);
    let len = v.hypot();
    if !len.is_finite() || len <= NO_DIRECTION {
        return angle;
    }
    if a.determinant() < 0.0 {
        v = -v;
    }
    let turned = normalize_deg(-v.y.atan2(v.x).to_degrees());
    if !turned.is_finite() || normalize_deg(turned - angle).abs() <= SAME_ANGLE {
        return angle;
    }
    // +0 rather than −0.
    (turned * ANGLE_STEPS).round() / ANGLE_STEPS + 0.0
}

/// Move what `map` keeps by `a`: each value walked as [`move_kept`] walks it.
fn move_kept_map(map: &mut Map<String, Value>, a: Affine, depth: usize) {
    for (key, v) in map.iter_mut() {
        if key == POINTS {
            move_positions(v, a, depth + 1);
        } else {
            move_kept(v, a, depth + 1);
        }
    }
}

/// Move a kept value by `a`: an object holding a frame has it composed with `a` and nothing else in
/// it moves; a `points` member's positions move; everything else is walked into, to
/// [`MAX_KEPT_DEPTH`].
fn move_kept(v: &mut Value, a: Affine, depth: usize) {
    if depth > MAX_KEPT_DEPTH {
        return;
    }
    match v {
        Value::Object(map) => {
            if let Some(frame) = map.get_mut(FRAME).filter(|f| frame(f).is_some()) {
                compose_frame(frame, a);
                return;
            }
            move_kept_map(map, a, depth);
        }
        Value::Array(items) => {
            for item in items {
                move_kept(item, a, depth + 1);
            }
        }
        _ => {}
    }
}

/// Move every `[x, y]` (an array of two numbers) in `map`'s values by `a`.
fn move_positions_in_map(map: &mut Map<String, Value>, a: Affine, depth: usize) {
    for v in map.values_mut() {
        move_positions(v, a, depth + 1);
    }
}

/// Move every `[x, y]` (an array of two numbers) in `v` by `a`, to [`MAX_KEPT_DEPTH`]; one that
/// would leave the finite numbers is left as it was.
fn move_positions(v: &mut Value, a: Affine, depth: usize) {
    if depth > MAX_KEPT_DEPTH {
        return;
    }
    match v {
        Value::Array(items) => {
            if let [x, y] = items.as_slice()
                && let (Some(x), Some(y)) = (x.as_f64(), y.as_f64())
            {
                let q = a * Point::new(x, y);
                if q.x.is_finite() && q.y.is_finite() {
                    *v = serde_json::json!([q.x, q.y]);
                }
                return;
            }
            for item in items {
                move_positions(item, a, depth + 1);
            }
        }
        Value::Object(map) => move_positions_in_map(map, a, depth),
        _ => {}
    }
}

/// A frame (six finite numbers, `Affine::new`'s order).
fn frame(v: &Value) -> Option<Affine> {
    let items = v.as_array()?;
    let coeffs: Vec<f64> = items.iter().map(Value::as_f64).collect::<Option<_>>()?;
    let coeffs: [f64; 6] = coeffs.try_into().ok()?;
    coeffs.iter().all(|c| c.is_finite()).then(|| Affine::new(coeffs))
}

/// `frame := a · frame`, unless that leaves the finite numbers.
fn compose_frame(v: &mut Value, a: Affine) {
    let Some(f) = frame(v) else { return };
    let composed = (a * f).as_coeffs();
    if composed.iter().all(|c| c.is_finite()) {
        *v = serde_json::json!(composed);
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;
    use crate::embroidery::{PartKey, PartKind, PartValue};

    /// A turn by `degrees` counter-clockwise on the page, about the origin (the document's y axis
    /// points down, so in its coordinates the turn is clockwise).
    fn turn(degrees: f64) -> Affine {
        Affine::rotate(-degrees.to_radians())
    }

    /// The fill angle `angle` turned by `a`, normalised to (−180, 180].
    fn after(angle: f64, a: Affine) -> f64 {
        normalize_deg(turned_angle(angle, a))
    }

    /// The worked values: where a set angle goes under a turn, the two flips and an uneven scale
    /// (a flip turns it half round as well, so that the sides swap with it).
    #[test]
    fn a_set_angle_turns_with_the_object() {
        let flip_x = Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        let flip_y = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);
        let cases = [
            (turn(90.0), [(0.0, 90.0), (30.0, 120.0), (45.0, 135.0), (90.0, 180.0)]),
            (flip_x, [(0.0, 0.0), (30.0, -30.0), (45.0, -45.0), (90.0, -90.0)]),
            (flip_y, [(0.0, 180.0), (30.0, 150.0), (45.0, 135.0), (90.0, 90.0)]),
            (Affine::scale_non_uniform(2.0, 1.0), [(0.0, 0.0), (30.0, 16.102), (45.0, 26.565), (90.0, 90.0)]),
        ];
        for (a, values) in cases {
            for (angle, want) in values {
                let got = after(angle, a);
                assert!((normalize_deg(got - want)).abs() < 1e-3, "{angle}° under {a:?}: {got}°, not {want}°");
            }
        }
    }

    /// A move, a uniform scale and a full turn keep an angle's bits; a transform that leaves no
    /// direction, or that isn't finite, keeps it as it was.
    #[test]
    fn moves_keep_an_angles_bits() {
        for angle in [30.0, -170.25, 135.0, 0.1] {
            for a in [Affine::translate((12.5, -3.0)), Affine::scale(2.5), turn(360.0), Affine::IDENTITY] {
                assert_eq!(turned_angle(angle, a).to_bits(), f64::to_bits(angle), "{angle} under {a:?}");
            }
            for a in [Affine::scale(0.0), Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]), Affine::scale(f64::INFINITY)] {
                assert_eq!(turned_angle(angle, a).to_bits(), f64::to_bits(angle), "{angle} under {a:?}");
            }
        }
    }

    /// An object's embroidery with a fill and a stroke set, and what a newer version kept.
    fn settings() -> Embroidery {
        serde_json::from_value(json!({
            "fill": {"angle": 30.0, "pull": [0.3, 0.1], "pull_percent": [5.0, 0.0], "push": [0.4, 0.2], "spacing": 0.4,
                     "points": {"entry": [10.0, 20.0], "exit": [30.0, 40.0], "rails": [[[0.0, 0.0], [5.0, 5.0]]]},
                     "motif": {"points": {"at": [1.0, 2.0]}, "size": [3.0, 4.0]}},
            "stroke": {"length": 2.0},
            "areas": [{"points": {"at": [100.0, 50.0]}, "color": [7.0, 8.0]}],
            "original": {"frame": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "node": {"points": [[1.0, 1.0], [2.0, 2.0]]}},
            "trim_after": true
        }))
        .unwrap()
    }

    /// A move shifts every position by exactly its offset, those a newer version kept under a
    /// `points` object included, composes a kept frame with it, and leaves everything else, angles
    /// and per-side values included, as it was.
    #[test]
    fn a_move_moves_the_points_and_nothing_else() {
        let mut e = settings();
        e.transform(Affine::translate((5.0, -2.0)));
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({
                "fill": {"angle": 30.0, "pull": [0.3, 0.1], "pull_percent": [5.0, 0.0], "push": [0.4, 0.2], "spacing": 0.4,
                         "points": {"entry": [15.0, 18.0], "exit": [35.0, 38.0], "rails": [[[5.0, -2.0], [10.0, 3.0]]]},
                         "motif": {"points": {"at": [6.0, 0.0]}, "size": [3.0, 4.0]}},
                "stroke": {"length": 2.0},
                "areas": [{"points": {"at": [105.0, 48.0]}, "color": [7.0, 8.0]}],
                "original": {"frame": [1.0, 0.0, 0.0, 1.0, 5.0, -2.0], "node": {"points": [[1.0, 1.0], [2.0, 2.0]]}},
                "trim_after": true
            })
        );
        let mut same = settings();
        same.transform(Affine::IDENTITY);
        assert_eq!(same, settings(), "the identity changes nothing");
    }

    /// A converted object's kept original goes where its stitches go: its frame composed with each
    /// transform (moved, turned, mirrored, scaled), its node, and what a newer version keeps beside
    /// the frame, left as they were until it is restored (the frame places them). A transform that
    /// would leave the finite numbers leaves the frame as it was.
    #[test]
    fn an_original_goes_where_its_stitches_go() {
        let mut e = settings();
        let hint = json!({"points": [[3.0, 4.0]], "anchor": {"position": [1.0, 2.0]}});
        if let Some(o) = e.original.as_mut() {
            std::sync::Arc::make_mut(&mut o.extra).insert("hint".into(), hint.clone());
        }
        let a = Affine::translate((5.0, -2.0)) * turn(90.0) * Affine::scale_non_uniform(1.2, -0.5);
        e.transform(a);
        let o = e.original.as_ref().unwrap();
        assert_eq!(o.frame, a);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&o.node).unwrap(), json!({"points": [[1.0, 1.0], [2.0, 2.0]]}), "the node isn't moved");
        assert_eq!(o.extra.get("hint"), Some(&hint), "nor what it keeps beside its frame");
        e.transform(Affine::scale(f64::MAX));
        assert_eq!(e.original.as_ref().unwrap().frame, a, "a degenerate transform leaves the frame");
    }

    /// A run records how it was turned and scaled since its stitches were made: a move alone
    /// records nothing, the first transform that does more starts the record, every one after is
    /// composed with it (moves too), and one that would leave the finite numbers changes nothing.
    #[test]
    fn a_run_records_its_transforms_since_it_was_made() {
        let mut e = Embroidery { stitches: Some(super::super::StitchRun::default()), ..Embroidery::default() };
        let recorded = |e: &Embroidery| e.stitches.as_ref().and_then(|r| r.since_made);
        e.transform(Affine::translate((5.0, -2.0)));
        assert_eq!(recorded(&e), None, "a move records nothing");
        let scale = Affine::scale_non_uniform(1.15, 1.0);
        e.transform(scale);
        assert_eq!(recorded(&e), Some(scale));
        let then = Affine::translate((1.0, 1.0)) * turn(90.0);
        e.transform(then);
        assert_eq!(recorded(&e), Some(then * scale), "composed with every transform after");
        e.transform(Affine::scale(f64::MAX));
        assert_eq!(recorded(&e), Some(then * scale), "a degenerate transform leaves the record");
        let mut mirrored = Embroidery { stitches: Some(super::super::StitchRun::default()), ..Embroidery::default() };
        mirrored.transform(Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        assert!(recorded(&mirrored).is_some(), "a mirror is more than a move");
    }

    /// The record saves as an object holding a frame and reads back; so an older version, which
    /// keeps it unread, composes it with each transform as this one does (the convention for kept
    /// data); one holding anything more is kept unread, and composed all the same.
    #[test]
    fn a_runs_record_is_a_kept_frame() {
        let scale = Affine::scale(1.2);
        let run = super::super::StitchRun { since_made: Some(scale), ..Default::default() };
        let saved = serde_json::Value::from(run.clone());
        assert_eq!(saved, json!({"transformed": {"frame": [1.2, 0.0, 0.0, 1.2, 0.0, 0.0]}}));
        let back = super::super::StitchRun::from(saved.as_object().unwrap().clone());
        assert_eq!(back, run);
        // As an older version holds it: unread, among the run's kept keys.
        let mut older = Embroidery {
            stitches: Some(super::super::StitchRun { extra: std::sync::Arc::new(saved.as_object().unwrap().clone()), ..Default::default() }),
            ..Embroidery::default()
        };
        older.transform(Affine::scale(0.5));
        assert_eq!(older.stitches.as_ref().unwrap().extra["transformed"], json!({"frame": [0.6, 0.0, 0.0, 0.6, 0.0, 0.0]}));
        let newer = json!({"transformed": {"frame": [1.2, 0.0, 0.0, 1.2, 0.0, 0.0], "by": "a newer version"}});
        let read = super::super::StitchRun::from(newer.as_object().unwrap().clone());
        assert_eq!((read.since_made, serde_json::Value::from(read)), (None, newer), "kept as it was");
        for odd in [json!({"frame": [1.0, 0.0]}), json!({"frame": "x"}), json!([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])] {
            let held = json!({ "transformed": odd });
            assert_eq!(super::super::StitchRun::from(held.as_object().unwrap().clone()).since_made, None, "{held}");
        }
    }

    /// The least and most a transform stretches a length by: a turn and a mirror keep lengths, a
    /// scale gives its factors (smallest first), a shear stretches one way and shrinks the other,
    /// and one past the finite numbers gives none.
    #[test]
    fn scale_factors_are_the_stretches() {
        let near = |got: Option<[f64; 2]>, want: [f64; 2]| got.is_some_and(|g| g.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-12));
        let flip = Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        for (a, want) in [
            (Affine::IDENTITY, [1.0, 1.0]),
            (Affine::translate((40.0, -3.0)), [1.0, 1.0]),
            (turn(33.0), [1.0, 1.0]),
            (flip, [1.0, 1.0]),
            (Affine::scale_non_uniform(1.15, 0.9), [0.9, 1.15]),
            (turn(90.0) * Affine::scale_non_uniform(-2.0, 0.5) * turn(-20.0), [0.5, 2.0]),
            // A shear by 1: its stretches are the golden ratio and its inverse.
            (Affine::new([1.0, 0.0, 1.0, 1.0, 0.0, 0.0]), [(5f64.sqrt() - 1.0) / 2.0, (5f64.sqrt() + 1.0) / 2.0]),
            (Affine::scale(0.0), [0.0, 0.0]),
        ] {
            assert!(near(scale_factors(a), want), "{a:?}: {:?}, not {want:?}", scale_factors(a));
        }
        for a in [Affine::scale(f64::MAX), Affine::new([f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0])] {
            assert_eq!(scale_factors(a), None, "{a:?}");
        }
    }

    /// A mirror turns a set angle half round beyond its image and swaps the per-side values, never
    /// a column's ends; unset values stay unset.
    #[test]
    fn a_mirror_swaps_the_sides() {
        let mut e = settings();
        e.transform(Affine::new([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        let fill = e.part(PartKind::Fill).unwrap();
        assert_eq!((fill.angle, fill.pull, fill.pull_percent, fill.push), (Some(-30.0), Some([0.1, 0.3]), Some([0.0, 5.0]), Some([0.4, 0.2])));
        assert_eq!(fill.get(PartKey::Entry), Some(PartValue::Point(Point::new(-10.0, 20.0))));
        let stroke = e.part(PartKind::Stroke).unwrap();
        assert_eq!((stroke.angle, stroke.pull, stroke.length), (None, None, Some(2.0)));
    }

    /// A transform that sends a point off to no finite place leaves it where it was, as a kept
    /// position is left; nothing a transform writes is anything but finite.
    #[test]
    fn a_degenerate_transform_writes_nothing_infinite() {
        for a in [Affine::scale(f64::MAX), Affine::new([f64::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0])] {
            let mut e = settings();
            let entry = e.part(PartKind::Fill).and_then(|p| p.get(PartKey::Entry));
            e.transform(a);
            let saved = serde_json::to_string(&e).unwrap();
            assert!(!saved.contains("null") && !saved.contains("inf"), "{a:?}: {saved}");
            assert_eq!(e.part(PartKind::Fill).and_then(|p| p.get(PartKey::Entry)), entry, "{a:?}: the entry where it was");
        }
        let mut collapsed = settings();
        collapsed.transform(Affine::scale(0.0));
        assert_eq!(collapsed.part(PartKind::Fill).and_then(|p| p.angle), Some(30.0), "no direction left: kept");
    }

    /// The most an affine of [`arb_affine`] stretches one direction more than another (the ratio of
    /// its linear part's singular values): an uneven scale of ten to one, or a shear of about 70°,
    /// beyond any transform a person makes of art to be sewn. A rounded angle's error grows by up to
    /// its square under the transform.
    const MOST_STRETCH: f64 = 10.0;

    /// An affine of four linear coefficients within ±10 and an offset within ±1000, whose
    /// determinant lies between 0.01 and 100 in size (either sign), stretching no direction more
    /// than [`MOST_STRETCH`] times another: invertible, and far from degenerate.
    fn arb_affine() -> impl Strategy<Value = Affine> {
        (-10.0..10.0f64, -10.0..10.0f64, -10.0..10.0f64, -10.0..10.0f64, -1000.0..1000.0f64, -1000.0..1000.0f64)
            .prop_map(|(a, b, c, d, e, f)| Affine::new([a, b, c, d, e, f]))
            .prop_filter("near degenerate or too large a scale", |a| (0.01..100.0).contains(&a.determinant().abs()))
            .prop_filter("stretched too unevenly", |a| {
                // The singular values' ratio r has r + 1/r = (the squared coefficients' sum) / |det|.
                let [m0, m1, m2, m3, _, _] = a.as_coeffs();
                (m0 * m0 + m1 * m1 + m2 * m2 + m3 * m3) / a.determinant().abs() <= MOST_STRETCH + 1.0 / MOST_STRETCH
            })
    }

    /// Settings with a set angle, per-side values and points, from the strategy's numbers.
    fn arb_settings() -> impl Strategy<Value = Embroidery> {
        (-180.0..180.0f64, 0.0..2.0f64, 0.0..2.0f64, -1000.0..1000.0f64, -1000.0..1000.0f64).prop_map(|(angle, l, r, x, y)| {
            let mut e = Embroidery::default();
            e.edit_part(PartKind::Fill, |p| {
                p.set(PartKey::Angle, Some(PartValue::Number(angle)));
                p.set(PartKey::Pull, Some(PartValue::Sides([l, r])));
                p.set(PartKey::Entry, Some(PartValue::Point(Point::new(x, y))));
            });
            // A kept original, where it was converted (its frame the identity, as a convert leaves it).
            let node: std::sync::Arc<str> = json!({"id": 7}).to_string().into();
            e.original = Some(super::super::Original { frame: Affine::IDENTITY, node, fingerprint: None, extra: Default::default() });
            // A run already transformed once since it was made, so that its record is composed.
            e.stitches = Some(super::super::StitchRun { since_made: Some(Affine::IDENTITY), ..Default::default() });
            e
        })
    }

    /// How far apart two angles may lie after transforms that should give the same, degrees: the
    /// rounding of each turned angle ([`ANGLE_STEPS`]) grown by the stretch, far below what moves a
    /// stitch (1e-7° moves the end of a 100 mm row by 2e-7 mm).
    const ANGLE_TOLERANCE: f64 = 1e-7;
    /// How far apart two points may lie after transforms that should give the same, points.
    const POINT_TOLERANCE: f64 = 1e-6;

    /// Two settings are the same within what floating point leaves: angles within
    /// [`ANGLE_TOLERANCE`] (modulo a full turn), points within [`POINT_TOLERANCE`], everything else
    /// exactly.
    fn close(a: &Embroidery, b: &Embroidery) -> bool {
        let (Some(p), Some(q)) = (a.part(PartKind::Fill), b.part(PartKind::Fill)) else { return false };
        let angle = p.angle.zip(q.angle).is_some_and(|(x, y)| normalize_deg(x - y).abs() < ANGLE_TOLERANCE);
        let entry = p
            .points
            .as_ref()
            .and_then(|v| v.entry)
            .zip(q.points.as_ref().and_then(|v| v.entry))
            .is_some_and(|(x, y)| x.distance(y) < POINT_TOLERANCE);
        let near = |x: Affine, y: Affine| x.as_coeffs().iter().zip(y.as_coeffs()).all(|(u, v)| (u - v).abs() < POINT_TOLERANCE);
        let frames = match (&a.original, &b.original) {
            (Some(x), Some(y)) => near(x.frame, y.frame),
            (x, y) => x == y,
        };
        let records = match (a.stitches.as_ref().and_then(|r| r.since_made), b.stitches.as_ref().and_then(|r| r.since_made)) {
            (Some(x), Some(y)) => near(x, y),
            (x, y) => x == y,
        };
        angle && entry && p.pull == q.pull && frames && records
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

        /// A transform followed by its inverse gives the settings back.
        #[test]
        fn a_transform_and_its_inverse_give_the_settings_back(e in arb_settings(), a in arb_affine()) {
            let mut there = e.clone();
            there.transform(a);
            there.transform(a.inverse());
            prop_assert!(close(&there, &e), "{e:?} → {there:?} under {a:?}");
        }

        /// Two transforms one after the other do what their product does.
        #[test]
        fn two_transforms_are_their_product(e in arb_settings(), a in arb_affine(), b in arb_affine()) {
            let mut stepwise = e.clone();
            stepwise.transform(a);
            stepwise.transform(b);
            let mut at_once = e.clone();
            at_once.transform(b * a);
            prop_assert!(close(&stepwise, &at_once), "{stepwise:?} against {at_once:?}");
        }
    }
}
