//! The sewing order as `embroidery.sequence` lists it: the blocks in the order they are sewn, each
//! with its thread and its units in order, and for each unit what it sews, how it is sewn and why
//! (the rule that chose its stitch), its stitches and where it starts and ends.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::{MAX_SATIN_WIDTH, Style};

use super::digitize::SATIN_MIN_WIDTH;
use super::plan::{Choice, DesignPlan, Part, Unit};
use super::stats::{self, mm, point, rect, thread_json};

/// What part of its object a unit sews, as the list names it: its fills and strokes counted from
/// 1 in appearance order.
fn part(p: Part) -> String {
    match p {
        Part::Fill(k) => format!("fill {}", k + 1),
        Part::Stroke(k) => format!("stroke {}", k + 1),
        Part::Literal => "stitches".into(),
    }
}

/// How a unit is sewn.
fn sewn(u: &Unit) -> &'static str {
    match (u.sewn(), u.columns) {
        (None, _) => "literal",
        (Some(Style::Running), _) => "running",
        (Some(Style::Satin), _) => "satin",
        (Some(Style::Fill), Some(_)) => "tatami with satin columns",
        (Some(Style::Fill), None) => "tatami",
    }
}

/// Why it is sewn that way: the rule that chose its stitch.
fn why(u: &Unit) -> String {
    let narrow = u.params.map_or(0.0, |p| p.narrow);
    let width = u.params.map_or(0.0, |p| p.width);
    match u.why {
        Choice::Fill if u.columns.is_some() => {
            format!("a fill is tatami; its parts narrower than {} mm are satin columns that follow the shape (narrowAsSatin)", mm(narrow))
        }
        Choice::Fill if narrow > 0.0 => format!("a fill is tatami; it has no part narrower than {} mm to sew as satin (narrowAsSatin)", mm(narrow)),
        Choice::Fill => "a fill is tatami (narrowAsSatin 0: no satin columns)".into(),
        Choice::WideStroke if width >= MAX_SATIN_WIDTH => {
            format!("a stroke at least {SATIN_MIN_WIDTH} mm wide is a satin column; this one is sewn {MAX_SATIN_WIDTH} mm wide, the widest")
        }
        Choice::WideStroke => format!("a stroke at least {SATIN_MIN_WIDTH} mm wide is a satin column, here {} mm", mm(width)),
        Choice::NarrowStroke => format!("a stroke narrower than {SATIN_MIN_WIDTH} mm is a running line"),
        Choice::StrokeFilled => "stitchStyle fill: the area the stroke paints is filled".into(),
        Choice::Forced => match (u.params.map(|p| p.style), u.sewn()) {
            (Some(Style::Satin), Some(Style::Fill)) => "stitchStyle satin: a fill has no width to sew a satin across, so it is filled".into(),
            (asked, _) => format!("stitchStyle {}", asked.map_or("auto", Style::id)),
        },
        Choice::Literal => "stitches read from a machine file, sewn as they are".into(),
    }
}

/// The plan's sewing order → `embroidery.sequence`'s answer.
pub(crate) fn sequence(plan: &DesignPlan) -> Value {
    let (blocks_counted, units) = stats::counts(plan);
    // Each object (as a person selects it) and the blocks its units are in.
    let mut blocks_of: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for u in &plan.units {
        let blocks = blocks_of.entry(u.origin.selected().0).or_default();
        if !blocks.contains(&(u.block + 1)) {
            blocks.push(u.block + 1);
        }
    }
    let blocks: Vec<Value> = plan
        .blocks
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let list: Vec<Value> = b
                .units
                .clone()
                .filter_map(|i| Some((i, plan.units.get(i)?)))
                .map(|(i, u)| {
                    let t = units.get(i).copied().unwrap_or_default();
                    let needle = || plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch);
                    // A unit of one needle point where the needle already is adds no record: no numbers.
                    let numbers = plan.numbers(i).filter(|n| !n.is_empty());
                    json!({
                        "unit": i + 1,
                        "id": u.origin.selected().0,
                        "node": u.origin.node.0,
                        "instance": u.origin.instance.map(|n| n.0),
                        "part": part(u.origin.part),
                        "sewn": sewn(u),
                        "why": why(u),
                        "stitches": t.needle,
                        "first": numbers.as_ref().map(|n| n.start),
                        "last": numbers.as_ref().map(|n| n.end.saturating_sub(1)),
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
                "first": b.first_number,
                "last": b.last_number(),
                "jumps": t.jumps,
                "trims": t.trims,
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
    json!({"blocks": blocks, "split": split, "warnings": plan.warnings})
}
