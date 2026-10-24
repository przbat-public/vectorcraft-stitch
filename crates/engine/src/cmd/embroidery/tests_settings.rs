//! The embroidery settings: what each one does to the stitches, and the angle's direction.

use serde_json::{Value, json};
use vectorcraft_stitch::Command;

use super::digitize::digitize;
use super::test_support::{PT_PER_MM, blank, run};
use super::*;

/// The direction most of `plan`'s needle-down stitches longer than `longer` (millimetres) run in,
/// degrees counter-clockwise on the page, from 0 up to a half turn (a line's two directions are
/// one): the median of their directions.
fn direction(plan: &plan::DesignPlan, longer: f64) -> f64 {
    let mut angles: Vec<f64> = Vec::new();
    for b in &plan.blocks {
        for w in b.stitches.windows(2) {
            let [a, z] = w else { continue };
            if a.command != Command::Stitch || z.command != Command::Stitch || a.distance(*z) <= longer {
                continue;
            }
            // The page's y axis points up, the document's down.
            angles.push((-(z.y - a.y)).atan2(z.x - a.x).to_degrees().rem_euclid(180.0));
        }
    }
    assert!(!angles.is_empty(), "no stitch longer than {longer} mm");
    angles.sort_by(f64::total_cmp);
    angles[angles.len() / 2]
}

/// A 60 mm square filled red, without underlay (so that its stitches are its rows), planned with
/// the embroidery options `opts`.
fn square(opts: &Value) -> plan::DesignPlan {
    let mut s = blank();
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    let side = 60.0 * PT_PER_MM;
    run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": side, "height": side}));
    let mut opts = opts.clone();
    opts["underlay"] = json!(false);
    let opts = Options::from_json(&opts).unwrap();
    digitize(&s.doc().unwrap().doc, &opts, None).unwrap()
}

/// A fill's angle turns counter-clockwise on the page, as every other angle in VectorCraft: rows
/// at 30° run up to the right, at 120° up to the left; left out, they run at 135°, from the top left
/// to the bottom right, as they always have.
#[test]
fn a_fill_angle_turns_counter_clockwise_on_the_page() {
    /// How far the rows' direction may lie from the angle asked for, degrees: a needle point's
    /// last bits and the rows' stagger move a 4 mm stitch's direction by far less.
    const WITHIN: f64 = 0.5;
    /// Longer than a row's remainder at the edges, shorter than its 4 mm stitches.
    const ROW_STITCH: f64 = 3.0;
    for (opts, want) in [(json!({"angle": 30}), 30.0), (json!({"stitchAngle": 120}), 120.0), (json!({}), 135.0), (json!({"angle": -45}), 135.0)] {
        let rows = direction(&square(&opts), ROW_STITCH);
        assert!((rows - want).abs() <= WITHIN, "{opts}: rows at {rows}°, not {want}°");
    }
    assert_eq!(options::planner_angle(vectorcraft_stitch::plan::FILL_ANGLE), 135.0);
}
