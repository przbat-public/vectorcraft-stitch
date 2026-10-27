//! The sewing order as `embroidery.sequence` lists it: the blocks in the order they are sewn, each
//! with its thread and its units in order, and for each unit what it sews, how it is sewn and why
//! (the rule that chose its stitch, as an id with its values and as a sentence), its stitches and
//! where it starts and ends.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::{MAX_SATIN_WIDTH, Style};

use super::digitize::SATIN_MIN_WIDTH;
use super::options::stitch_id;
use super::plan::{BlockThread, Choice, DesignPlan, Part, Unit};
use super::stats::{self, Page, mm};

/// What part of its object a unit sews, as an id (`fill`, `stroke`, `literal`), and which fill or
/// stroke, counted from 1 in appearance order (`None` for a literal run). With the object's id and
/// the unit's `occurrence` it tells a unit apart ([`super::plan::Unit::occurrence`]); `unit` is a
/// position in one plan, and `node` (the object sewn) changes between plans for an expanded
/// object's pieces.
pub(crate) fn part_of(p: Part) -> (&'static str, Option<usize>) {
    match p {
        Part::Fill(k) => ("fill", Some(k + 1)),
        Part::Stroke(k) => ("stroke", Some(k + 1)),
        Part::Literal => ("literal", None),
    }
}

/// How a unit is sewn, in the words the `stitch` option takes (`running`, `satin`, `tatami`), or
/// `literal` for points sewn as they are.
pub(crate) fn sewn_of(u: &Unit) -> &'static str {
    u.sewn().map_or("literal", stitch_id)
}

/// The rule that chose how a unit is sewn: its id, the values it turned on, and the same as a
/// sentence for display.
pub(crate) fn rule_of(u: &Unit) -> (&'static str, Value, String) {
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
        Choice::Forced(by) => {
            let stitch = u.params.map(|p| p.style).map_or("auto", stitch_id);
            ("forced", json!({"stitch": stitch, "source": by.id()}), format!("stitch {stitch}"))
        }
        Choice::SatinFilled(by) => {
            let why = if u.columns.is_some() {
                "stitch satin: a fill has no width to sew a satin across, so it is filled, its narrow parts as satin columns"
            } else {
                "stitch satin: a fill has no width to sew a satin across, so it is filled"
            };
            ("forced", json!({"stitch": stitch_id(Style::Satin), "source": by.id()}), why.into())
        }
        Choice::Literal => ("literal", json!({}), "stitches read from a machine file, sewn as they are".into()),
    }
}

/// A block's thread as every command lists it ([`stats::thread_json`]'s keys): the palette thread
/// (`palette`, `index` from 1 as the format numbers it, `name`, `color`), the art's colour, their
/// CIEDE2000 difference as shown and whether it is a mismatch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadInfo {
    pub palette: String,
    pub index: usize,
    pub name: String,
    pub color: String,
    pub art: String,
    pub delta_e: f64,
    pub mismatch: bool,
}

impl ThreadInfo {
    pub(crate) fn of(t: &BlockThread) -> Self {
        let hex = super::hex;
        Self {
            palette: t.palette().id().to_string(),
            index: t.index(),
            name: t.name().to_string(),
            color: hex(t.color()),
            art: hex(t.art),
            delta_e: t.shown_delta_e(),
            mismatch: t.mismatch(),
        }
    }
}

/// The sewing order as `embroidery.sequence` answers with it, typed: the reply is this serialised,
/// and the Embroidery panel reads it back, so the two can't drift apart. Lengths and points are
/// millimetres, to the hundredth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SewingOrder {
    pub block_count: usize,
    pub unit_count: usize,
    /// The first unit listed, and whether units after the last listed were left out.
    pub offset: usize,
    pub truncated: bool,
    pub blocks: Vec<OrderBlock>,
    /// The objects sewn in more than one block, and how many there are.
    pub split_count: usize,
    pub split: Vec<SplitObject>,
    /// What the plan couldn't sew, and why.
    pub warnings: Vec<String>,
}

/// A thread block in the sewing order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderBlock {
    /// Its number, from 1.
    pub block: usize,
    pub thread: ThreadInfo,
    pub stitches: usize,
    pub records: usize,
    pub first_record: usize,
    pub last_record: usize,
    pub jumps: usize,
    pub trims: usize,
    pub unit_count: usize,
    /// Its units listed (those in the page asked for).
    pub units: Vec<OrderUnit>,
    /// What the machine does before the next block (`None` after the last).
    pub then: Option<Then>,
}

/// What the machine does between two blocks: a colour change, after a cut when the block ends
/// with one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Then {
    pub color_change: bool,
    pub trim: bool,
}

/// A sewing unit in the sewing order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderUnit {
    /// Its number, from 1.
    pub unit: usize,
    /// The object a person selects for it, the object sewn, and the outermost symbol instance it
    /// is drawn through.
    pub id: u64,
    pub node: u64,
    pub instance: Option<u64>,
    /// What part it sews (`fill`, `stroke`, `literal`) and which, from 1; with `id` and
    /// `occurrence` it tells the unit apart from one plan to the next.
    pub part: String,
    pub part_index: Option<usize>,
    pub occurrence: usize,
    /// How it is sewn (`running`, `satin`, `tatami`, `literal`), with satin columns or not.
    pub sewn: String,
    pub columns: bool,
    /// Why: the rule's id, the values it turned on, and a sentence.
    pub rule: String,
    pub rule_values: Value,
    pub why: String,
    pub stitches: usize,
    pub records: usize,
    pub first_record: Option<usize>,
    pub last_record: Option<usize>,
    pub jumps: usize,
    pub trims: usize,
    pub entry: Option<[f64; 2]>,
    pub exit: Option<[f64; 2]>,
    pub bounds: Option<[f64; 4]>,
    /// The entry and exit points the unit was planned toward (its part's, when its object sets
    /// them), millimetres, and how far its first and last needle points miss them.
    pub asked: EntryExit<[f64; 2]>,
    pub missed_by: EntryExit<f64>,
}

/// An entry's and an exit's (each `null` when there is none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EntryExit<T> {
    pub entry: Option<T>,
    pub exit: Option<T>,
}

/// An object sewn in more than one block: its id and the blocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitObject {
    pub id: u64,
    pub blocks: Vec<usize>,
}

/// `p` as a reply's point: millimetres to the hundredth.
fn at(p: vectorcraft_geom::Point) -> [f64; 2] {
    [mm(p.x), mm(p.y)]
}

impl SewingOrder {
    /// The plan's sewing order: the units `page` lists, in sewing order, within their blocks.
    pub(crate) fn of(plan: &DesignPlan, page: Page) -> Self {
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
        let blocks = plan
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.units.start < listed.end && listed.start < b.units.end)
            .map(|(k, b)| {
                let units = (b.units.start.max(listed.start)..b.units.end.min(listed.end))
                    .filter_map(|i| Some((i, plan.units.get(i)?)))
                    .map(|(i, u)| {
                        let t = units.get(i).copied().unwrap_or_default();
                        // Where it starts and ends: after an exact meet, where the needle already was.
                        let ends = plan.needle_ends(i);
                        // A unit of one needle point where the needle already is adds no record: no numbers.
                        let numbers = plan.numbers(i).filter(|n| !n.is_empty());
                        let (part, part_index) = part_of(u.origin.part);
                        let (rule, rule_values, why) = rule_of(u);
                        OrderUnit {
                            unit: i + 1,
                            id: u.origin.selected().0,
                            node: u.origin.node.0,
                            instance: u.origin.instance.map(|n| n.0),
                            part: part.to_string(),
                            part_index,
                            occurrence: u.occurrence,
                            sewn: sewn_of(u).to_string(),
                            columns: u.columns.is_some(),
                            rule: rule.to_string(),
                            rule_values,
                            why,
                            stitches: t.needle,
                            records: u.stitches.len(),
                            first_record: numbers.as_ref().map(|n| n.start),
                            last_record: numbers.as_ref().map(|n| n.end.saturating_sub(1)),
                            jumps: t.jumps,
                            trims: t.trims,
                            entry: ends.map(|e| at(e.0)),
                            exit: ends.map(|e| at(e.1)),
                            bounds: u.bounds.map(|r| [mm(r.x0), mm(r.y0), mm(r.x1), mm(r.y1)]),
                            asked: EntryExit { entry: u.asked.entry.map(at), exit: u.asked.exit.map(at) },
                            missed_by: {
                                let [entry, exit] = plan.missed(i);
                                EntryExit { entry: entry.map(mm), exit: exit.map(mm) }
                            },
                        }
                    })
                    .collect();
                let t = blocks_counted.get(k).copied().unwrap_or_default();
                OrderBlock {
                    block: k + 1,
                    thread: ThreadInfo::of(&b.thread),
                    stitches: t.needle,
                    records: b.stitches.len(),
                    first_record: b.first_number,
                    last_record: b.last_number(),
                    jumps: t.jumps,
                    trims: t.trims,
                    unit_count: b.units.len(),
                    units,
                    then: (k + 1 < plan.blocks.len())
                        .then(|| Then { color_change: true, trim: b.stitches.last().is_some_and(|s| s.command == Command::Trim) }),
                }
            })
            .collect();
        let split: Vec<SplitObject> = blocks_of.into_iter().filter(|(_, b)| b.len() > 1).map(|(id, blocks)| SplitObject { id, blocks }).collect();
        let split_count = split.len();
        Self {
            block_count: plan.blocks.len(),
            unit_count: plan.units.len(),
            offset: listed.start,
            truncated: page.truncated(plan.units.len()),
            blocks,
            split_count,
            split: split.into_iter().take(page.limit).collect(),
            warnings: plan.messages(),
        }
    }
}

/// The plan's sewing order → `embroidery.sequence`'s answer: the units `page` lists, in sewing
/// order, within their blocks ([`SewingOrder`] serialised).
pub(crate) fn sequence(plan: &DesignPlan, page: Page) -> Value {
    serde_json::to_value(SewingOrder::of(plan, page)).unwrap_or(Value::Null)
}
