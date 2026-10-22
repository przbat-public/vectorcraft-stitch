//! The sewing order as `embroidery.sequence` lists it: the blocks in the order they are sewn, each
//! with its thread and its units in order, and for each unit what it sews, how it is sewn and why
//! (the rule that chose its stitch, as an id with its values and as a sentence), its stitches and
//! where it starts and ends.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::{MAX_SATIN_WIDTH, Style};

use super::digitize::SATIN_MIN_WIDTH;
use super::options::stitch_id;
use super::plan::{Choice, DesignPlan, Part, Unit};
use super::stats::{self, Page, mm, point, rect, thread_json};

/// What part of its object a unit sews, as an id (`fill`, `stroke`, `literal`), and which fill or
/// stroke, counted from 1 in appearance order (`None` for a literal run). With the object's id,
/// the pair is what tells a unit apart from one plan to the next; `unit` is a position in one
/// plan.
fn part(p: Part) -> (&'static str, Option<usize>) {
    match p {
        Part::Fill(k) => ("fill", Some(k + 1)),
        Part::Stroke(k) => ("stroke", Some(k + 1)),
        Part::Literal => ("literal", None),
    }
}

/// How a unit is sewn, in the words the `stitch` option takes (`running`, `satin`, `tatami`), or
/// `literal` for points sewn as they are.
fn sewn(u: &Unit) -> &'static str {
    u.sewn().map_or("literal", stitch_id)
}

/// The rule that chose how a unit is sewn: its id, the values it turned on, and the same as a
/// sentence for display.
fn rule(u: &Unit) -> (&'static str, Value, String) {
    let narrow = u.params.map_or(0.0, |p| p.narrow);
    let width = u.params.map_or(0.0, |p| p.width);
    match u.why {
        Choice::Fill => {
            let why = if u.columns.is_some() {
                format!("a fill is tatami; its parts narrower than {} mm are satin columns that follow the shape (narrowAsSatin)", mm(narrow))
            } else if narrow > 0.0 {
                format!("a fill is tatami; it has no part narrower than {} mm to sew as satin (narrowAsSatin)", mm(narrow))
            } else {
                "a fill is tatami (narrowAsSatin 0: no satin columns)".into()
            };
            ("fill", json!({"narrowAsSatin": mm(narrow)}), why)
        }
        Choice::WideStroke => {
            let why = if width >= MAX_SATIN_WIDTH {
                format!("a stroke at least {SATIN_MIN_WIDTH} mm wide is a satin column; this one is sewn {MAX_SATIN_WIDTH} mm wide, the widest")
            } else {
                format!("a stroke at least {SATIN_MIN_WIDTH} mm wide is a satin column, here {} mm", mm(width))
            };
            ("wideStroke", json!({"width": mm(width), "satinFrom": SATIN_MIN_WIDTH, "widest": MAX_SATIN_WIDTH}), why)
        }
        Choice::NarrowStroke => {
            ("narrowStroke", json!({"satinFrom": SATIN_MIN_WIDTH}), format!("a stroke narrower than {SATIN_MIN_WIDTH} mm is a running line"))
        }
        Choice::StrokeFilled => ("strokeFilled", json!({}), "stitch fill: the area the stroke paints is filled".into()),
        Choice::StrokeOutlined => {
            let why = if u.columns.is_some() {
                "a dashed, aligned, varying or arrowed stroke: the area it paints is filled, its narrow parts as satin columns"
            } else {
                "a dashed, aligned, varying or arrowed stroke: the area it paints is filled"
            };
            ("strokeOutlined", json!({}), why.into())
        }
        Choice::Forced => {
            let stitch = u.params.map(|p| p.style).map_or("auto", stitch_id);
            ("forced", json!({"stitch": stitch}), format!("stitch {stitch}"))
        }
        Choice::SatinFilled => {
            let why = if u.columns.is_some() {
                "stitch satin: a fill has no width to sew a satin across, so it is filled, its narrow parts as satin columns"
            } else {
                "stitch satin: a fill has no width to sew a satin across, so it is filled"
            };
            ("forced", json!({"stitch": stitch_id(Style::Satin)}), why.into())
        }
        Choice::Literal => ("literal", json!({}), "stitches read from a machine file, sewn as they are".into()),
    }
}

/// The plan's sewing order → `embroidery.sequence`'s answer: the units `page` lists, in sewing
/// order, within their blocks.
pub(crate) fn sequence(plan: &DesignPlan, page: Page) -> Value {
    let (blocks_counted, units) = stats::counts(plan);
    // Each object (as a person selects it) and the blocks its units are in.
    let mut blocks_of: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for u in &plan.units {
        let blocks = blocks_of.entry(u.origin.selected().0).or_default();
        if !blocks.contains(&(u.block + 1)) {
            blocks.push(u.block + 1);
        }
    }
    let listed = page.range(plan.units.len());
    let blocks: Vec<Value> = plan
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.units.start < listed.end && listed.start < b.units.end)
        .map(|(k, b)| {
            let list: Vec<Value> = (b.units.start.max(listed.start)..b.units.end.min(listed.end))
                .filter_map(|i| Some((i, plan.units.get(i)?)))
                .map(|(i, u)| {
                    let t = units.get(i).copied().unwrap_or_default();
                    let needle = || plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch);
                    // A unit of one needle point where the needle already is adds no record: no numbers.
                    let numbers = plan.numbers(i).filter(|n| !n.is_empty());
                    let (part, part_index) = part(u.origin.part);
                    let (rule, values, why) = rule(u);
                    json!({
                        "unit": i + 1,
                        "id": u.origin.selected().0,
                        "node": u.origin.node.0,
                        "instance": u.origin.instance.map(|n| n.0),
                        "part": part,
                        "partIndex": part_index,
                        "sewn": sewn(u),
                        "columns": u.columns.is_some(),
                        "rule": rule,
                        "ruleValues": values,
                        "why": why,
                        "stitches": t.needle,
                        "records": u.stitches.len(),
                        "firstRecord": numbers.as_ref().map(|n| n.start),
                        "lastRecord": numbers.as_ref().map(|n| n.end.saturating_sub(1)),
                        "jumps": t.jumps,
                        "trims": t.trims,
                        "entry": needle().next().map(|s| point(s.point())),
                        "exit": needle().next_back().map(|s| point(s.point())),
                        "bounds": u.bounds.map(rect),
                    })
                })
                .collect();
            let t = blocks_counted.get(k).copied().unwrap_or_default();
            json!({
                "block": k + 1,
                "thread": thread_json(&b.thread),
                "stitches": t.needle,
                "records": b.stitches.len(),
                "firstRecord": b.first_number,
                "lastRecord": b.last_number(),
                "jumps": t.jumps,
                "trims": t.trims,
                "unitCount": b.units.len(),
                "units": list,
                // What the machine does before the next block: a colour change, after a cut when the
                // block ends with one.
                "then": (k + 1 < plan.blocks.len()).then(|| json!({
                    "colorChange": true,
                    "trim": b.stitches.last().is_some_and(|s| s.command == Command::Trim),
                })),
            })
        })
        .collect();
    let split: Vec<Value> = blocks_of.into_iter().filter(|(_, b)| b.len() > 1).map(|(id, blocks)| json!({"id": id, "blocks": blocks})).collect();
    let split_count = split.len();
    let split: Vec<Value> = split.into_iter().take(page.limit).collect();
    json!({
        "blockCount": plan.blocks.len(),
        "unitCount": plan.units.len(),
        "offset": listed.start,
        "truncated": page.truncated(plan.units.len()),
        "blocks": blocks,
        "splitCount": split_count,
        "split": split,
        "warnings": plan.messages(),
    })
}
