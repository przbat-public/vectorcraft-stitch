//! The read commands' replies, and the settings commands', pinned by shape: every key and the type
//! of its value, so that a change is a deliberate one. Replies change only by addition (`docs/mcp.md` › Embroidery): a test
//! failing here for a removed or retyped key is a break for every client that reads it.

use serde_json::{Value, json};

use super::test_support::{blank, run};
use super::*;

/// `v`'s shape: an object's keys (sorted) each with its value's shape, an array's items' shapes
/// (each different one once, sorted, split by `|`), a scalar's type (`int`, `float`, `string`,
/// `bool`, `null`).
fn shape(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(_) => "bool".into(),
        Value::Number(n) if n.is_f64() => "float".into(),
        Value::Number(_) => "int".into(),
        Value::String(_) => "string".into(),
        Value::Array(a) => {
            let mut items: Vec<String> = a.iter().map(shape).collect();
            items.sort_unstable();
            items.dedup();
            format!("[{}]", items.join(" | "))
        }
        Value::Object(o) => {
            let mut keys: Vec<(&String, &Value)> = o.iter().collect();
            keys.sort_by(|a, b| a.0.cmp(b.0));
            format!("{{{}}}", keys.iter().map(|(k, v)| format!("{k}: {}", shape(v))).collect::<Vec<_>>().join(", "))
        }
    }
}

/// A document whose replies have a value in every key that can have one: a filled rectangle
/// framed by a wide stroke, a far one of the same colour (a cut), a thin stroked one, and a long
/// stitch for the format checks.
fn document() -> Session {
    let mut s = blank();
    run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    let framed = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40}))["id"].clone();
    run(&mut s, "stroke.set", json!({"weight": 6, "ids": [framed]}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "shape.rectangle", json!({"x": 200, "y": 200, "width": 40, "height": 30}));
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#22aa44"}));
    let thin = run(&mut s, "shape.rectangle", json!({"x": 100, "y": 100, "width": 60, "height": 40}))["id"].clone();
    run(&mut s, "stroke.set", json!({"weight": 0.5, "ids": [thin]}));
    run(&mut s, "select.none", json!({}));
    s
}

/// The shapes, command by command: what the replies hold today.
const SHAPES: [(&str, &str); 4] = [
    (
        "embroidery.threads",
        "{blockCount: int, blocks: [{block: int, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, unitCount: int}], offset: int, options: {stitch: string}, palette: string, threshold: float, truncated: bool, warnings: []}",
    ),
    (
        "embroidery.stats",
        "{blockCount: int, blocks: [{block: int, bobbinThreadMetres: float, firstRecord: int, jumps: int, lastRecord: int, records: int, seconds: int, stitches: int, stops: int, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, topThreadMetres: float, trims: int}], bounds: [float], colorChanges: int, end: [float], extents: {down: float, left: float, right: float, up: float}, jumps: int, lengths: {bins: [{count: int, from: float, to: float} | {count: int, from: float, to: null}], median: float}, longestJump: float, longestStitch: float, offset: int, options: {stitch: string}, records: int, shortestStitch: float, size: {height: float, width: float}, start: [float], stitches: int, stops: int, threadCount: int, threadUsage: {bobbinMetres: float, estimate: string, fabricThickness: float, topMetres: float}, time: {estimate: string, includes: string, minutes: float, seconds: int, slowAbove: int, slowSpeed: int, speed: int, trimSeconds: float}, trims: int, truncated: bool, warnings: []}",
    ),
    (
        "embroidery.sequence",
        "{blockCount: int, blocks: [{block: int, firstRecord: int, jumps: int, lastRecord: int, records: int, stitches: int, stops: int, then: null, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, trims: int, unitCount: int, units: [{asked: {entry: null, exit: null}, bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, missedBy: {entry: null, exit: null}, node: int, occurrence: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {satinFrom: float, widest: float, width: float}, sewn: string, stitches: int, stops: int, trims: int, unit: int, why: string} | {asked: {entry: null, exit: null}, bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, missedBy: {entry: null, exit: null}, node: int, occurrence: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {satinFrom: float}, sewn: string, stitches: int, stops: int, trims: int, unit: int, why: string}]} | {block: int, firstRecord: int, jumps: int, lastRecord: int, records: int, stitches: int, stops: int, then: {colorChange: bool, trim: bool}, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, trims: int, unitCount: int, units: [{asked: {entry: null, exit: null}, bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, missedBy: {entry: null, exit: null}, node: int, occurrence: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {narrowAsSatin: float}, sewn: string, stitches: int, stops: int, trims: int, unit: int, why: string}]}], offset: int, options: {stitch: string}, split: [], splitCount: int, truncated: bool, unitCount: int, warnings: []}",
    ),
    (
        "embroidery.check",
        "{format: string, notChecked: [{id: string, what: string, why: string}], options: {length: float, stitch: string}, palette: string, rules: [{examples: [], hits: int, id: string, measured: float, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measured: int, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: int, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measured: int, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: null, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measured: null, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measured: null, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: null, title: string, truncated: bool, unit: null} | {examples: [{at: [float], block: null, from: [float], id: int, length: float, note: string, objects: null, pieces: [int], record: int, thread: null, value: float}], hits: int, id: string, measured: float, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [{at: [float], block: null, from: [float], id: int, length: null, note: null, objects: null, pieces: null, record: int, thread: null, value: float}], hits: int, id: string, measured: float, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [{at: null, block: int, from: null, id: int, length: null, note: string, objects: null, pieces: null, record: int, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, value: float}], hits: int, id: string, measured: float, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string}], summary: {errors: {hits: int, rules: int}, info: {hits: int, rules: int}, warnings: {hits: int, rules: int}}}",
    ),
];

#[test]
fn the_read_commands_replies_keep_their_shape() {
    let mut s = document();
    let mut failed = Vec::new();
    for (command, pinned) in SHAPES {
        let params = if command == "embroidery.check" { json!({"length": 12, "format": "dst"}) } else { json!({}) };
        let now = shape(&run(&mut s, command, params));
        if now != pinned {
            failed.push(format!("(\"{command}\", \"{now}\"),"));
        }
    }
    assert!(failed.is_empty(), "replies changed shape; if on purpose, pin:\n{}", failed.join("\n"));
}

/// The settings commands' replies on objects, pinned as the read commands' are, after these calls
/// on [`document`]'s objects, in this order.
const SETTINGS_SHAPES: [(&str, &str); 9] = [
    (
        "embroidery.fabrics",
        "{caveat: string, default: string, fabrics: [{estimates: [], id: string, label: string, sources: [{family: string, how: null, key: string, kind: string, name: string, url: string} | {family: string, how: string, key: string, kind: string, name: string, url: string}], values: {needlePoint: string, satin: {pull: null, spacingPercent: null, underlay: null}, stabilizer: string, tatami: {length: null, pull: null, spacing: null, underlay: null}, thickness: null, topping: string}} | {estimates: [], id: string, label: string, sources: [{family: string, how: string, key: string, kind: string, name: string, url: string}], values: {needlePoint: string, satin: {pull: null, spacingPercent: null, underlay: null}, stabilizer: string, tatami: {length: null, pull: null, spacing: null, underlay: null}, thickness: float, topping: string}} | {estimates: [string], id: string, label: string, sources: [{family: null, how: string, key: string, kind: string, name: null, url: null} | {family: string, how: null, key: string, kind: string, name: string, url: string} | {family: string, how: string, key: string, kind: string, name: string, url: string}], values: {needlePoint: string, satin: {pull: float, spacingPercent: float, underlay: string}, stabilizer: string, tatami: {length: float, pull: float, spacing: float, underlay: string}, thickness: float, topping: string}} | {estimates: [string], id: string, label: string, sources: [{family: null, how: string, key: string, kind: string, name: null, url: null} | {family: string, how: null, key: string, kind: string, name: string, url: string} | {family: string, how: string, key: string, kind: string, name: string, url: string}], values: {needlePoint: string, satin: {pull: float, spacingPercent: float, underlay: string}, stabilizer: string, tatami: {length: float, pull: float, spacing: float, underlay: string}, thickness: null, topping: string}}]}",
    ),
    (
        "embroidery.settings",
        "{overrides: {length: int, objectCount: int, pull: int, spacing: int, underlay: int}, resolved: {fabric: {color: {source: string, value: string}, needlePoint: {changed: bool, preset: null, source: string, value: null}, preset: null, satin: {pull: {changed: bool, preset: null, source: string, value: float}, spacingPercent: {changed: bool, preset: null, source: string, value: float}, underlay: {changed: bool, preset: null, source: string, value: string}}, stabilizer: {changed: bool, preset: null, source: string, value: null}, tatami: {length: {changed: bool, preset: null, source: string, value: float}, pull: {changed: bool, preset: null, source: string, value: float}, spacing: {changed: bool, preset: null, source: string, value: float}, underlay: {changed: bool, preset: null, source: string, value: string}}, thickness: {changed: bool, preset: null, source: string, value: float}, topping: {changed: bool, preset: null, source: string, value: null}}, joinSameColor: {source: string, value: bool}, machine: {jumpLength: {source: string, value: float}, minStitch: {source: string, value: float}, speed: {source: string, value: float}, splitLength: {source: string, value: float}, trimLength: {source: string, value: float}}, palette: {source: string, value: string}, threadWeight: {source: string, value: float}}, settings: {fabric: null, joinSameColor: null, label: null, machine: null, palette: null, start: null, threadWeight: null}}",
    ),
    ("embroidery.applyFabric", "{changedCount: int, keys: [string], skipped: [], skippedCount: int, unchangedCount: int}"),
    (
        "embroidery.info",
        "{fill: {entry: {mixed: bool, setCount: int, value: null}, exit: {mixed: bool, setCount: int, value: null}, itemCount: int, objectCount: int, reason: null, rule: null, settings: {angle: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, expand: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, length: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, narrowAsSatin: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, pull: {applies: null, inherited: {section: null, source: string, value: [float]}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: [float]}, pullPercent: {applies: null, inherited: {section: null, source: string, value: [float]}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: [float]}, push: {applies: null, inherited: {section: null, source: string, value: [float]}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: [float]}, spacing: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, splitLength: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, stitch: {applies: bool, inherited: {section: null, source: string, value: string}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: string}, underlay: {applies: null, inherited: {section: null, source: string, value: string}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: string}}, sewing: {jumps: int, missedBy: {entry: null, exit: null}, records: int, stitches: int, trims: int, unitCount: int}, sewn: null}, objectCount: int, objects: [{expanded: bool, fills: int, id: int, kept: [], kind: string, settings: {fill: null, stroke: null}, strokes: int}], offset: int, options: {stitch: string}, planned: bool, skipped: [], skippedCount: int, stroke: {entry: {mixed: bool, setCount: int, value: null}, exit: {mixed: bool, setCount: int, value: null}, itemCount: int, objectCount: int, reason: null, rule: null, settings: {angle: {applies: bool, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, expand: {applies: bool, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, length: {applies: null, inherited: null, mixed: bool, rule: null, section: null, setCount: int, source: string, value: null}, narrowAsSatin: {applies: bool, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, pull: {applies: null, inherited: null, mixed: bool, rule: null, section: null, setCount: int, source: string, value: null}, pullPercent: {applies: null, inherited: {section: null, source: string, value: [float]}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: [float]}, push: {applies: null, inherited: {section: null, source: string, value: [float]}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: [float]}, spacing: {applies: null, inherited: null, mixed: bool, rule: null, section: null, setCount: int, source: string, value: null}, splitLength: {applies: null, inherited: {section: null, source: string, value: float}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: float}, stitch: {applies: bool, inherited: {section: null, source: string, value: string}, mixed: bool, rule: null, section: null, setCount: int, source: string, value: string}, underlay: {applies: null, inherited: null, mixed: bool, rule: null, section: null, setCount: int, source: string, value: null}}, sewing: {jumps: int, missedBy: {entry: null, exit: null}, records: int, stitches: int, trims: int, unitCount: int}, sewn: null}, trimAfter: {mixed: bool, setCount: int, value: bool}, truncated: bool, warnings: []}",
    ),
    (
        "embroidery.set",
        "{changedCount: int, part: string, settings: {angle: float, pull: [float]}, skipped: [], skippedCount: int, unchangedCount: int}",
    ),
    ("embroidery.setEntryExit", "{changedCount: int, entry: [float], exit: null, id: int, part: string}"),
    (
        "embroidery.nearestConnection",
        "{after: {connections: int, jumps: int, length: float, longest: float, stitches: int, trims: int}, applied: bool, before: {connections: int, jumps: int, length: float, longest: float, stitches: int, trims: int}, changedCount: int, parts: [{entry: [float], exit: [float], id: int, part: string}], restoredCount: int, saved: {jumps: int, length: float, stitches: int, trims: int}, skipped: [], skippedCount: int, unchangedCount: int}",
    ),
    (
        "embroidery.copyFrom",
        "{changedCount: int, keys: [string], part: null, passedOver: [], skipped: [], skippedCount: int, source: int, unchangedCount: int}",
    ),
    ("embroidery.reset", "{changedCount: int, keys: [string], part: string, skipped: [], skippedCount: int, unchangedCount: int}"),
];

#[test]
fn the_settings_commands_replies_keep_their_shape() {
    let mut s = document();
    let ids: Vec<u64> = s.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|c| c.id.0).collect();
    let params = |command: &str| match command {
        "embroidery.set" => json!({"ids": ids, "part": "fill", "angle": 30, "pull": [0.3, 0.1]}),
        "embroidery.setEntryExit" => json!({"id": ids[0], "part": "fill", "entry": [20, 20]}),
        "embroidery.copyFrom" => json!({"source": ids[0], "ids": ids}),
        "embroidery.info" => json!({"ids": ids, "plan": true}),
        "embroidery.fabrics" | "embroidery.settings" | "embroidery.nearestConnection" => json!({}),
        "embroidery.applyFabric" => json!({"ids": ids}),
        _ => json!({"ids": ids, "part": "fill"}),
    };
    let mut failed = Vec::new();
    for (command, pinned) in SETTINGS_SHAPES {
        let now = shape(&run(&mut s, command, params(command)));
        if now != pinned {
            failed.push(format!("(\"{command}\", \"{now}\"),"));
        }
    }
    assert!(failed.is_empty(), "replies changed shape; if on purpose, pin:\n{}", failed.join("\n"));
}

/// The manual-stitch commands' replies, pinned by shape as the others are.
#[test]
fn the_manual_stitch_commands_replies_keep_their_shape() {
    let mut s = document();
    let ids: Vec<u64> = s.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|c| c.id.0).collect();
    let converted = shape(&run(&mut s, "embroidery.convertToManual", json!({ "ids": ids })));
    let pinned =
        "{converted: [{id: int, needlePoints: int, runs: int, units: int}], convertedCount: int, needlePoints: int, skipped: [], skippedCount: int}";
    assert_eq!(converted, pinned, "replies changed shape; if on purpose, pin it");
    let a_run = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(ids[0])).unwrap().children().unwrap()[0].id.0;
    let moved = shape(&run(&mut s, "embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": a_run, "index": 1}], "dx": 0.5})));
    let pinned = "{changedCount: int, lengths: {aboveMax: int, belowMin: int, count: int, longest: float, max: float, min: float, shortest: float}, passedOver: [], pathCount: int, paths: [{id: int, needlePoints: int}]}";
    assert_eq!(moved, pinned, "replies changed shape; if on purpose, pin it");
    let inserted = shape(&run(&mut s, "embroidery.insertNeedlePoints", json!({"id": a_run, "index": 1})));
    let pinned = "{changedCount: int, inserted: [{id: int, index: int}], lengths: {aboveMax: int, belowMin: int, count: int, longest: float, max: float, min: float, shortest: float}, passedOver: [], pathCount: int, paths: [{id: int, needlePoints: int}]}";
    assert_eq!(inserted, pinned, "replies changed shape; if on purpose, pin it");
    let deleted = shape(&run(&mut s, "embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": a_run, "index": 1}]})));
    let pinned = "{changedCount: int, dropped: [], lengths: {aboveMax: int, belowMin: int, count: int, longest: float, max: float, min: float, shortest: float}, moved: [], passedOver: [], pathCount: int, paths: [{id: int, needlePoints: int}], removed: [], removedCount: int}";
    assert_eq!(deleted, pinned, "replies changed shape; if on purpose, pin it");
    let split = shape(&run(&mut s, "embroidery.splitRun", json!({"needlePoints": [{"id": a_run, "index": 2}], "ties": false})));
    let pinned = "{changedCount: int, jumps: [{id: int, index: int, length: float}], lengths: {aboveMax: int, belowMin: int, count: int, longest: float, max: float, min: float, shortest: float}, passedOver: [], pathCount: int, paths: [{id: int, needlePoints: int}], unchangedCount: int}";
    assert_eq!(split, pinned, "replies changed shape; if on purpose, pin it");
    let joined = shape(&run(&mut s, "embroidery.joinRuns", json!({"needlePoints": [{"id": a_run, "index": 2}]})));
    let pinned = "{changedCount: int, dropped: [], lengths: {aboveMax: int, belowMin: int, count: int, longest: float, max: float, min: float, shortest: float}, pathCount: int, paths: [{id: int, needlePoints: int}], removed: []}";
    assert_eq!(joined, pinned, "replies changed shape; if on purpose, pin it");
    let dry = shape(&run(&mut s, "embroidery.restoreObject", json!({"ids": ids, "dryRun": true})));
    // A needle point moved: the object's stitches were edited since converting.
    let pinned = "{dryRun: bool, edited: [int], editedCount: int, restored: [int], restoredCount: int, skipped: [], skippedCount: int}";
    assert_eq!(dry, pinned, "replies changed shape; if on purpose, pin it");
    let restored = shape(&run(&mut s, "embroidery.restoreObject", json!({ "ids": ids })));
    let pinned = "{edited: [int], editedCount: int, restored: [int], restoredCount: int, skipped: [], skippedCount: int}";
    assert_eq!(restored, pinned, "replies changed shape; if on purpose, pin it");
}
