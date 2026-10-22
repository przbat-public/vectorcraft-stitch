//! A design's figures (`embroidery.stats`), exactly, on designs built stitch by stitch.

use serde_json::json;
use vectorcraft_geom::Point;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::plan::DesignPlan;
use super::sequence::Planned;
use super::stats::{self, Machine, Thread as Fabric};
use super::test_support::{self, plan};
use super::*;

/// How close two figures worked out two ways must be: rounding only.
const ROUNDING: f64 = 1e-9;

/// A unit of `style` in `color`, sewing `points` (millimetres) as needle points.
fn unit(node: u64, style: Style, color: [u8; 3], points: &[(f64, f64)]) -> Planned {
    test_support::unit(node, style, color, points, None)
}

const RED: [u8; 3] = [200, 30, 30];
const BLUE: [u8; 3] = [30, 30, 200];

/// A running line of three stitches (2, 2 and 3 mm) in red, then a satin of three (1, √2 and
/// 1 mm) in blue: two blocks, eight needle points, no jumps.
fn two_blocks() -> DesignPlan {
    plan(vec![
        unit(1, Style::Running, RED, &[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0), (4.0, 3.0)]),
        unit(2, Style::Satin, BLUE, &[(30.0, 0.0), (30.0, 1.0), (31.0, 0.0), (31.0, 1.0)]),
    ])
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < ROUNDING
}

/// Counts, lengths, size, start, end and extents, as laid out stitch by stitch.
#[test]
fn the_counts_and_lengths_are_the_designs() {
    let v = stats::stats(&two_blocks(), Machine { speed: 650.0 }, Fabric { fabric: 0.5 });
    assert_eq!(
        (&v["stitches"], &v["records"], &v["jumps"], &v["trims"], &v["colorChanges"], &v["threads"], &v["blocks"]),
        (&json!(8), &json!(8), &json!(0), &json!(0), &json!(1), &json!(2), &json!(2))
    );
    assert_eq!((&v["shortestStitch"], &v["longestStitch"], &v["longestJump"]), (&json!(1.0), &json!(3.0), &json!(null)));
    assert_eq!(v["size"], json!({"width": 31.0, "height": 3.0}));
    assert_eq!(v["bounds"], json!([0.0, 0.0, 31.0, 3.0]));
    assert_eq!((&v["start"], &v["end"]), (&json!([0.0, 0.0]), &json!([31.0, 1.0])));
    assert_eq!(v["extents"], json!({"left": 0.0, "right": 31.0, "up": 0.0, "down": 3.0}));
    let blocks = v["perBlock"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!((&blocks[0]["first"], &blocks[0]["last"], &blocks[1]["first"], &blocks[1]["last"]), (&json!(1), &json!(4), &json!(5), &json!(8)));
    assert_eq!(v["time"]["threadChanges"], json!(1));
}

/// The thread: on the face each stitch's length; on the back its length shared between the bobbin
/// (a third under a satin, scaled by Wilcom's factors for the other styles: 125 / 65 of that under
/// a running line) and the top thread pulled under; and twice the fabric's thickness of top thread
/// at every needle point.
#[test]
fn the_thread_follows_the_stitches_and_the_fabric() {
    let (blocks, _) = stats::tally(&two_blocks(), Machine { speed: 650.0 }, Fabric { fabric: 0.5 });
    let (running, satin) = (7.0, 2.0 + 2f64.sqrt());
    let (run_share, satin_share) = (1.0 / 3.0 * 1.25 / 0.65, 1.0 / 3.0);
    let through = 2.0 * 0.5;
    assert!(close(blocks[0].bobbin, running * run_share), "{}", blocks[0].bobbin);
    assert!(close(blocks[0].top, running * (2.0 - run_share) + 4.0 * through), "{}", blocks[0].top);
    assert!(close(blocks[1].bobbin, satin * satin_share), "{}", blocks[1].bobbin);
    assert!(close(blocks[1].top, satin * (2.0 - satin_share) + 4.0 * through), "{}", blocks[1].top);
    // A thicker fabric only adds to the top thread.
    let (thick, _) = stats::tally(&two_blocks(), Machine { speed: 650.0 }, Fabric { fabric: 1.5 });
    assert!(close(thick[0].top - blocks[0].top, 4.0 * 2.0 * 1.0));
    assert!(close(thick[0].bobbin, blocks[0].bobbin));
}

/// The time: each needle point and jump one turn of the machine at its speed, slower past 4 mm,
/// a trim 3.5 s, the thread change not timed.
#[test]
fn the_time_counts_turns_and_trims() {
    let turn = |spm: f64| 60.0 / spm;
    let (blocks, _) = stats::tally(&two_blocks(), Machine { speed: 650.0 }, Fabric { fabric: 0.5 });
    // The red block's four needle points, then the blue block's first after a move of 26 mm
    // (slow), and its three.
    assert!(close(blocks[0].seconds, 4.0 * turn(650.0)), "{}", blocks[0].seconds);
    assert!(close(blocks[1].seconds, turn(400.0) + 3.0 * turn(650.0)), "{}", blocks[1].seconds);
    // A red unit far from the first joins its block after a trim and a jump of 46 mm.
    let joined =
        plan(vec![unit(1, Style::Running, RED, &[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0)]), unit(2, Style::Running, RED, &[(50.0, 0.0), (52.0, 0.0)])]);
    let (blocks, _) = stats::tally(&joined, Machine { speed: 650.0 }, Fabric { fabric: 0.5 });
    assert_eq!(joined.blocks.len(), 1);
    assert_eq!((blocks[0].needle, blocks[0].jumps, blocks[0].trims, blocks[0].longest_jump), (5, 1, 1, Some(46.0)));
    // Five needle points (the one after the jump goes nowhere), the jump slow, the trim.
    assert!(close(blocks[0].seconds, 5.0 * turn(650.0) + turn(400.0) + 3.5), "{}", blocks[0].seconds);
    // A slower machine sews no faster than its own speed past 4 mm either.
    let (slow, _) = stats::tally(&joined, Machine { speed: 300.0 }, Fabric { fabric: 0.5 });
    assert!(close(slow[0].seconds, 6.0 * turn(300.0) + 3.5), "{}", slow[0].seconds);
}

/// The command: the export's options, a speed and a fabric thickness within their ranges; out of
/// range or not a number is refused, not clamped; no other key is taken.
#[test]
fn the_stats_command_checks_its_params() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#cc2222"})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 40, "height": 30})).unwrap();
    let v = s.execute("embroidery.stats", &json!({"speed": 800, "fabricThickness": 1, "density": 0.5})).unwrap();
    assert_eq!((&v["time"]["speed"], &v["thread"]["fabricThickness"]), (&json!(800.0), &json!(1.0)));
    assert!(v["stitches"].as_u64().unwrap() > 100);
    // Both figures say they are estimates, and what they rest on.
    for field in [&v["thread"]["estimate"], &v["time"]["estimate"]] {
        assert!(field.as_str().unwrap().starts_with("an estimate: "), "{field}");
    }
    let defaults = s.execute("embroidery.stats", &json!({})).unwrap();
    assert_eq!((&defaults["time"]["speed"], &defaults["thread"]["fabricThickness"]), (&json!(650.0), &json!(0.5)));
    for (bad, why) in [
        (json!({"speed": 5000}), "`speed`"),
        (json!({"speed": "fast"}), "`speed`"),
        (json!({"fabricThickness": -1}), "`fabricThickness`"),
        (json!({"density": "x"}), "embroidery options"),
    ] {
        let e = s.execute("embroidery.stats", &bad).unwrap_err().to_string();
        assert!(e.contains(why), "{bad}: {e}");
    }
    // The keys it takes: the export's embroidery options and its own two.
    let spec = crate::cmd::find_command("embroidery.stats").unwrap();
    let mut keys: Vec<&str> = crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).collect();
    keys.extend(["speed", "fabricThickness"]);
    for key in &keys {
        assert!(crate::cmd::params::takes_key(spec.params, key), "{key}");
    }
    let e = crate::check_params("embroidery.stats", &json!({"format": "pes"})).unwrap_err().to_string();
    assert!(e.contains("unknown parameter `format`"), "{e}");
}

/// What counts as a stitch: a jump or cut is none, nor the needle point a jump lands on; between
/// two blocks the needle doesn't sew either.
#[test]
fn only_needle_down_stitches_have_lengths() {
    let mut p = two_blocks();
    // A jump inside the red block, and the needle point it lands on.
    let red = &mut p.blocks[0].stitches;
    red.push(Stitch::new(14.0, 3.0, Command::Jump));
    red.push(Stitch::sew(Point::new(14.0, 3.0)));
    red.push(Stitch::sew(Point::new(14.5, 3.0)));
    p.units[0].stitches.end = red.len();
    let (blocks, _) = stats::tally(&p, Machine { speed: 650.0 }, Fabric { fabric: 0.5 });
    assert_eq!(
        (blocks[0].needle, blocks[0].jumps, blocks[0].shortest, blocks[0].longest, blocks[0].longest_jump),
        (6, 1, Some(0.5), Some(3.0), Some(10.0))
    );
}
