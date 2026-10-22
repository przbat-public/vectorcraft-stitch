//! The read commands' replies, pinned by shape: every key and the type of its value, so that a
//! change is a deliberate one. Replies change only by addition (`docs/mcp.md` › Embroidery): a test
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
        "{blockCount: int, blocks: [{block: int, bobbinThreadMetres: float, firstRecord: int, jumps: int, lastRecord: int, records: int, seconds: int, stitches: int, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, topThreadMetres: float, trims: int}], bounds: [float], colorChanges: int, end: [float], extents: {down: float, left: float, right: float, up: float}, jumps: int, longestJump: float, longestStitch: float, offset: int, options: {stitch: string}, records: int, shortestStitch: float, size: {height: float, width: float}, start: [float], stitches: int, threadCount: int, threadUsage: {bobbinMetres: float, estimate: string, fabricThickness: float, topMetres: float}, time: {estimate: string, includes: string, minutes: float, seconds: int, slowAbove: int, slowSpeed: int, speed: int, trimSeconds: float}, trims: int, truncated: bool, warnings: []}",
    ),
    (
        "embroidery.sequence",
        "{blockCount: int, blocks: [{block: int, firstRecord: int, jumps: int, lastRecord: int, records: int, stitches: int, then: null, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, trims: int, unitCount: int, units: [{bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, node: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {satinFrom: float, widest: float, width: float}, sewn: string, stitches: int, trims: int, unit: int, why: string} | {bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, node: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {satinFrom: float}, sewn: string, stitches: int, trims: int, unit: int, why: string}]} | {block: int, firstRecord: int, jumps: int, lastRecord: int, records: int, stitches: int, then: {colorChange: bool, trim: bool}, thread: {art: string, color: string, deltaE: float, index: int, mismatch: bool, name: string, palette: string}, trims: int, unitCount: int, units: [{bounds: [float], columns: bool, entry: [float], exit: [float], firstRecord: int, id: int, instance: null, jumps: int, lastRecord: int, node: int, part: string, partIndex: int, records: int, rule: string, ruleValues: {narrowAsSatin: float}, sewn: string, stitches: int, trims: int, unit: int, why: string}]}], offset: int, options: {stitch: string}, split: [], splitCount: int, truncated: bool, unitCount: int, warnings: []}",
    ),
    (
        "embroidery.check",
        "{format: string, notChecked: [{what: string, why: string}], options: {length: float, stitch: string}, palette: string, rules: [{examples: [], hits: int, id: string, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: int, title: string, truncated: bool, unit: string} | {examples: [], hits: int, id: string, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: null, title: string, truncated: bool, unit: null} | {examples: [], hits: int, id: string, measures: string, objectCount: int, objects: [], severity: string, sources: [{name: string, url: string, value: string}], threshold: null, title: string, truncated: bool, unit: string} | {examples: [{at: [float], id: int, length: float, note: string, pieces: [int], record: int, value: float}], hits: int, id: string, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [{at: [float], id: int, length: null, note: null, pieces: null, record: int, value: float}], hits: int, id: string, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string} | {examples: [{at: null, id: int, length: null, note: string, pieces: null, record: int, value: float}], hits: int, id: string, measures: string, objectCount: int, objects: [int], severity: string, sources: [{name: string, url: string, value: string}], threshold: float, title: string, truncated: bool, unit: string}], summary: {errors: {hits: int, rules: int}, info: {hits: int, rules: int}, warnings: {hits: int, rules: int}}}",
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
