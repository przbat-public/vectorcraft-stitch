//! `embroidery.stitches`: the design's records, numbered as the Sewing order and the Check tab
//! number them (every needle point, jump, cut and stop, from 1), each with where it is, how long a
//! stitch it ends, the object it sews and, where the stitch commands can edit it, the needle point
//! they address it by (`{id, index}`): a needle point itself, a jump the needle point it lands on,
//! a cut or a stop the needle point before it. Paged and filtered; also what
//! `embroidery.selectNeedlePoints` selects by.
//!
//! A unit's records number the needle points of the path it sews only where that path is sewn as
//! its needle points: a run of literal stitches in the document's layers (not a symbol's art), that
//! can be edited, not cut by a clip group (manual stitches never are), and whose needle-down records
//! are as many as its needle points. Elsewhere `index` is `null` and `editable` false.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_stitch::Command;

use super::measure;
use super::needles;
use super::plan::{DesignPlan, Part};
use super::stats::Page;
use crate::cmd::bad;

/// The keys `embroidery.stitches` takes besides the embroidery options.
pub(super) const KEYS: [&str; 8] = ["from", "to", "ids", "kinds", "shorterThan", "longerThan", Page::KEYS[0], Page::KEYS[1]];

/// The kinds of record, as the reply names them.
const KINDS: [&str; 4] = ["stitch", "jump", "trim", "stop"];

/// What `embroidery.stitches` (and `selectNeedlePoints` by records) asks for.
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    /// The records from and to (record numbers, inclusive).
    pub(crate) from: Option<usize>,
    pub(crate) to: Option<usize>,
    /// The objects whose records it lists: the object a person selects, or the path sewn.
    pub(crate) ids: Option<BTreeSet<u64>>,
    /// The kinds it lists (none: all).
    pub(crate) kinds: Option<BTreeSet<&'static str>>,
    /// Only the records whose length (millimetres) is under `shorter`, or over `longer`; a record
    /// with no length is passed by when either is given.
    pub(crate) shorter: Option<f64>,
    pub(crate) longer: Option<f64>,
    pub(crate) page: Page,
}

impl Ask {
    /// What `o` asks for, refused by `cmd` when a value isn't one it takes.
    pub(crate) fn from_params(o: &Map<String, Value>, cmd: &str) -> crate::Result<Self> {
        let record = |key: &str| -> crate::Result<Option<usize>> {
            match o.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n >= 1)
                    .map(Some)
                    .ok_or_else(|| bad(cmd, format!("`{key}` {v}: a record number, from 1"))),
            }
        };
        let length = |key: &str| -> crate::Result<Option<f64>> {
            match o.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => v
                    .as_f64()
                    .filter(|x| x.is_finite() && *x >= 0.0)
                    .map(Some)
                    .ok_or_else(|| bad(cmd, format!("`{key}` {v}: millimetres, 0 or more"))),
            }
        };
        let (from, to) = (record("from")?, record("to")?);
        if let (Some(f), Some(t)) = (from, to)
            && f > t
        {
            return Err(bad(cmd, format!("`from` {f} after `to` {t}")));
        }
        let ids = match o.get("ids") {
            None | Some(Value::Null) => None,
            Some(v) => {
                let items = v.as_array().ok_or_else(|| bad(cmd, format!("`ids` {v}: a list of object ids")))?;
                if items.len() > needles::MAX_PATHS {
                    return Err(bad(cmd, format!("`ids` lists {} objects: at most {}", items.len(), needles::MAX_PATHS)));
                }
                Some(
                    items
                        .iter()
                        .map(|i| i.as_u64().ok_or_else(|| bad(cmd, format!("`ids` entry {i}: an object's id"))))
                        .collect::<crate::Result<_>>()?,
                )
            }
        };
        let kinds = match o.get("kinds") {
            None | Some(Value::Null) => None,
            Some(v) => {
                let items = v.as_array().ok_or_else(|| bad(cmd, format!("`kinds` {v}: a list of {}", KINDS.join(", "))))?;
                let kind = |i: &Value| {
                    i.as_str()
                        .and_then(|k| KINDS.iter().find(|n| **n == k).copied())
                        .ok_or_else(|| bad(cmd, format!("`kinds` entry {i}: one of {}", KINDS.join(", "))))
                };
                Some(items.iter().map(kind).collect::<crate::Result<_>>()?)
            }
        };
        Ok(Self { from, to, ids, kinds, shorter: length("shorterThan")?, longer: length("longerThan")?, page: Page::from_params(o, cmd)? })
    }
}

/// One record as the reply lists it.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(crate) record: usize,
    pub(crate) kind: &'static str,
    pub(crate) at: [f64; 2],
    pub(crate) length: Option<f64>,
    pub(crate) id: NodeId,
    pub(crate) node: NodeId,
    pub(crate) block: usize,
    pub(crate) unit: usize,
    /// The needle point it is (or lands on, or follows) in the path `node`, where the stitch
    /// commands can edit it.
    pub(crate) index: Option<usize>,
}

/// Each unit's path, where its records number that path's needle points ([`self`]'s rule): the
/// path, and the needle point its first own needle-down record is (1 when it starts where the
/// needle already was: its first needle point is the record before, left to the unit before).
fn numbered(plan: &DesignPlan, doc: &Document) -> Vec<Option<(NodeId, usize)>> {
    let mut sewn = vec![0usize; plan.units.len()];
    for b in &plan.blocks {
        for k in b.units.clone() {
            let Some(u) = plan.units.get(k) else { continue };
            let count = b.stitches.get(u.own.clone()).map_or(0, |s| s.iter().filter(|s| s.command == Command::Stitch).count());
            if let Some(slot) = sewn.get_mut(k) {
                *slot = count;
            }
        }
    }
    let mut lengths: HashMap<NodeId, Option<usize>> = HashMap::new();
    plan.units
        .iter()
        .zip(sewn)
        .map(|(u, sewn)| {
            let node = u.origin.node;
            let manual = matches!(u.origin.part, Part::Manual);
            if !(manual || matches!(u.origin.part, Part::Literal))
                || u.origin.instance.is_some_and(|i| i != node && !doc.node(i).is_some_and(super::digitize::converted))
            {
                return None;
            }
            let len = *lengths.entry(node).or_insert_with(|| {
                let n = doc.node(node)?;
                let clipped = !manual && doc.ancestry(node).unwrap_or_default().iter().any(|a| doc.node(*a).is_some_and(|a| a.clips()));
                (doc.is_editable(node) && !clipped).then(|| needles::read(n).ok().map(|st| st.len())).flatten()
            });
            let first = usize::from(u.met);
            (len == Some(sewn + first)).then_some((node, first))
        })
        .collect()
}

/// The rows of `plan` (of `doc`) that `ask` matches, in sewing order, as `f` takes them: with how
/// many records there are. Walks every record once; stops early when the work is cancelled.
pub(crate) fn rows(plan: &DesignPlan, doc: &Document, ask: &Ask, mut f: impl FnMut(Row)) -> usize {
    let numbered = numbered(plan, doc);
    // Each unit's needle-down records so far among its own.
    let mut counts = vec![0usize; plan.units.len()];
    let mut total = 0usize;
    measure::each_until_cancelled(plan, |r| {
        total += 1;
        let Some(u) = plan.units.get(r.unit) else { return };
        let Some(block) = plan.blocks.get(u.block) else { return };
        let kind = match r.stitch.command {
            Command::Stitch => "stitch",
            Command::Jump => "jump",
            Command::Trim => "trim",
            Command::Stop | Command::ColorChange => "stop",
        };
        // Its place in the path the unit sews, where the records number it.
        let place = r.number.checked_sub(block.first_number).filter(|i| u.own.contains(i));
        let index = match (numbered.get(r.unit).copied().flatten(), place) {
            (Some((_, first)), Some(_)) => {
                let count = counts.get(r.unit).copied().unwrap_or(0);
                match r.stitch.command {
                    Command::Stitch => {
                        if let Some(c) = counts.get_mut(r.unit) {
                            *c += 1;
                        }
                        Some(first + count)
                    }
                    // A jump: the needle point it lands on.
                    Command::Jump => Some(first + count),
                    // A cut or a stop: the needle point before it.
                    _ => (first + count).checked_sub(1),
                }
            }
            _ => None,
        };
        let length = match r.stitch.command {
            Command::Stitch => r.sewn(),
            Command::Jump => Some(r.moved()),
            _ => None,
        };
        if ask.from.is_some_and(|from| r.number < from) || ask.to.is_some_and(|to| r.number > to) {
            return;
        }
        if ask.kinds.as_ref().is_some_and(|k| !k.contains(kind)) {
            return;
        }
        let id = u.origin.selected();
        if ask.ids.as_ref().is_some_and(|ids| !ids.contains(&id.0) && !ids.contains(&u.origin.node.0)) {
            return;
        }
        if (ask.shorter.is_some() || ask.longer.is_some())
            && !length.is_some_and(|l| ask.shorter.is_none_or(|s| l < s) && ask.longer.is_none_or(|g| l > g))
        {
            return;
        }
        f(Row { record: r.number, kind, at: [r.stitch.x, r.stitch.y], length, id, node: u.origin.node, block: u.block + 1, unit: r.unit + 1, index });
    });
    total
}

/// `embroidery.stitches`' reply: the rows `ask` matches, the page it asks for.
pub(crate) fn list(plan: &DesignPlan, doc: &Document, ask: &Ask) -> Value {
    let page = ask.page.range(usize::MAX);
    let mut matched = 0usize;
    let mut listed = Vec::new();
    let total = rows(plan, doc, ask, |row| {
        if page.contains(&matched) {
            listed.push(json!({
                "record": row.record,
                "kind": row.kind,
                "at": row.at,
                "length": row.length,
                "id": row.id.0,
                "node": row.node.0,
                "block": row.block,
                "unit": row.unit,
                "index": row.index,
                "editable": row.index.is_some(),
            }));
        }
        matched += 1;
    });
    json!({
        "recordCount": total,
        "matchedCount": matched,
        "offset": ask.page.offset,
        "truncated": ask.page.truncated(matched),
        "records": listed,
    })
}

pub(super) const SELECT: &str = "embroidery.selectNeedlePoints";
/// The keys `embroidery.selectNeedlePoints` takes besides the embroidery options.
const SELECT_KEYS: [&str; 8] = ["records", "needlePoints", "ranges", "ids", "kinds", "shorterThan", "longerThan", "mode"];
/// The keys that select by records (planning the design).
const BY_RECORDS: [&str; 5] = ["records", "ids", "kinds", "shorterThan", "longerThan"];

/// `embroidery.selectNeedlePoints {records?, ids?, kinds?, shorterThan?, longerThan? | needlePoints?,
/// ranges?, mode?, …the embroidery options}`: needle points selected (`Selection.anchors`, as
/// Direct Selection selects anchors) by the records that sew them (planning the design, as
/// `embroidery.stitches` lists them; a record the stitch commands can't edit passed over and
/// counted), or by their addresses (planning nothing). `mode`: set (the default), add, toggle or
/// subtract. A selection, not an undo step.
pub(super) fn select(s: &mut crate::Session, p: &Value) -> crate::Result<Value> {
    let o = super::object(p, SELECT)?;
    let mode = match o.get("mode") {
        None | Some(Value::Null) => "set",
        Some(v) => ["set", "add", "toggle", "subtract"]
            .into_iter()
            .find(|m| v.as_str() == Some(m))
            .ok_or_else(|| bad(SELECT, format!("`mode` {v}: set, add, toggle or subtract")))?,
    };
    let by_records = BY_RECORDS.iter().any(|k| o.contains_key(*k));
    let by_address = o.contains_key("needlePoints") || o.contains_key("ranges");
    if by_records == by_address {
        return Err(bad(SELECT, "give records (records, ids, kinds, shorterThan, longerThan) or addresses (needlePoints, ranges): one or the other"));
    }
    let mut picked: std::collections::BTreeMap<NodeId, BTreeSet<usize>> = Default::default();
    let mut passed = 0usize;
    if by_address {
        super::only(&o, &["needlePoints", "ranges", "mode"], SELECT)?;
        let doc = s.doc()?.doc.clone();
        let t = needles::targets(&doc, &vectorcraft_doc::Selection::default(), &o, SELECT)?;
        for (id, _, k) in t.paths {
            picked.entry(id).or_default().extend(k);
        }
    } else {
        let opts = super::options_in(&o, &SELECT_KEYS, SELECT)?;
        let mut asked = Map::new();
        if let Some(v) = o.get("records").filter(|v| !v.is_null()) {
            match v.as_array().map(Vec::as_slice) {
                Some([from, to]) => {
                    asked.insert("from".into(), from.clone());
                    asked.insert("to".into(), to.clone());
                }
                _ => return Err(bad(SELECT, format!("`records` {v}: [from, to], record numbers from 1"))),
            }
        }
        for k in ["ids", "kinds", "shorterThan", "longerThan"] {
            if let Some(v) = o.get(k) {
                asked.insert(k.into(), v.clone());
            }
        }
        let ask = Ask::from_params(&asked, SELECT)?;
        let (plan, doc) = super::planned(s, &opts, false, SELECT)?;
        let mut count = 0usize;
        rows(&plan, &doc, &ask, |row| match row.index {
            Some(k) if row.kind != "trim" && row.kind != "stop" && count < needles::MAX_NEEDLE_POINTS => {
                if picked.entry(row.node).or_default().insert(k) {
                    count += 1;
                }
            }
            Some(_) => {}
            None => passed += 1,
        });
        vectorcraft_doc::cancel::check()?;
    }
    let mut selected = 0usize;
    s.select(|d, sel| {
        if mode == "set" {
            sel.clear();
        }
        for (id, k) in &picked {
            let Some(st) = d.node(*id).and_then(|n| needles::read(n).ok()) else { continue };
            let now = needles::selected_in(sel, *id, &st);
            let next: BTreeSet<usize> = match mode {
                "toggle" => now.symmetric_difference(k).copied().collect(),
                "subtract" => now.difference(k).copied().collect(),
                _ => now.union(k).copied().collect(),
            };
            selected += next.len();
            needles::select_in(sel, *id, &st, &next);
        }
    })?;
    Ok(json!({"selectedCount": selected, "pathCount": picked.len(), "passedOver": passed}))
}
