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

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_stitch::Command;

use super::measure;
use super::needles;
use super::plan::{DesignPlan, Part, Unit};
use super::stats::Page;
use crate::cmd::bad;

/// The keys `embroidery.stitches` takes besides the embroidery options.
pub(super) const KEYS: [&str; 8] = ["from", "to", "ids", "kinds", "shorterThan", "longerThan", Page::KEYS[0], Page::KEYS[1]];

/// A kind of record, as the reply names it (and the Stitches tab lists it, [`super::stitch_list`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    /// A needle point: the needle goes down.
    Stitch,
    /// The needle moves with it up.
    Jump,
    /// The thread is cut.
    Trim,
    /// The machine stops (a thread change, or a stop written as one).
    Stop,
}

impl RecordKind {
    const ALL: [Self; 4] = [Self::Stitch, Self::Jump, Self::Trim, Self::Stop];

    /// As the reply and `kinds` name it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Stitch => "stitch",
            Self::Jump => "jump",
            Self::Trim => "trim",
            Self::Stop => "stop",
        }
    }

    /// The kind of a record of command `c`.
    pub(crate) fn of(c: Command) -> Self {
        match c {
            Command::Stitch => Self::Stitch,
            Command::Jump => Self::Jump,
            Command::Trim => Self::Trim,
            Command::Stop | Command::ColorChange => Self::Stop,
        }
    }
}

/// What `embroidery.stitches` (and `selectNeedlePoints` by records) asks for.
#[derive(Clone, Debug)]
pub(crate) struct Ask {
    /// The records from and to (record numbers, inclusive).
    pub(crate) from: Option<usize>,
    pub(crate) to: Option<usize>,
    /// The objects whose records it lists: the object a person selects, or the path sewn.
    pub(crate) ids: Option<BTreeSet<u64>>,
    /// The kinds it lists (none: all).
    pub(crate) kinds: Option<BTreeSet<RecordKind>>,
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
                let names: Vec<&str> = RecordKind::ALL.iter().map(|k| k.name()).collect();
                let items = v.as_array().ok_or_else(|| bad(cmd, format!("`kinds` {v}: a list of {}", names.join(", "))))?;
                let kind = |i: &Value| {
                    i.as_str()
                        .and_then(|k| RecordKind::ALL.into_iter().find(|n| n.name() == k))
                        .ok_or_else(|| bad(cmd, format!("`kinds` entry {i}: one of {}", names.join(", "))))
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
    pub(crate) kind: RecordKind,
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
    // The units a path of stitches the commands can edit sews, and those paths found in one walk:
    // what can be edited measured once for them all, each path counted without reading it.
    let literal = |u: &Unit| {
        let node = u.origin.node;
        (matches!(u.origin.part, Part::Manual | Part::Literal)
            && !u.origin.instance.is_some_and(|i| i != node && !doc.node(i).is_some_and(super::digitize::is_converted)))
        .then_some(node)
    };
    let nodes: HashSet<NodeId> = plan.units.iter().filter_map(literal).collect();
    let paths = doc.index_paths(nodes.iter().copied());
    let editable = doc.editable();
    let mut lengths: HashMap<NodeId, Option<usize>> = HashMap::new();
    plan.units
        .iter()
        .zip(sewn)
        .map(|(u, sewn)| {
            let node = literal(u)?;
            let manual = matches!(u.origin.part, Part::Manual);
            let len = *lengths.entry(node).or_insert_with(|| {
                let path = paths.get(&node)?;
                let n = doc.node_at(path)?;
                let ancestors = doc.ancestry_at(path).unwrap_or_default();
                let clipped = !manual && ancestors.iter().any(|a| doc.node(*a).is_some_and(|a| a.clips()));
                (editable(node) && !clipped).then(|| needles::check(n).ok()).flatten()
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
    // The needle point the last needle-down record numbered is, in its block: a cut before the next
    // unit's own records (an opened design's run's) follows it.
    let mut last_needle: Option<(usize, NodeId, usize)> = None;
    measure::each_until_cancelled(plan, |r| {
        total += 1;
        let Some(u) = plan.units.get(r.unit) else { return };
        let Some(block) = plan.blocks.get(u.block) else { return };
        let kind = RecordKind::of(r.stitch.command);
        // Its place in the path the unit sews, where the records number it.
        let at = r.number.checked_sub(block.first_number);
        let place = at.filter(|i| u.own.contains(i));
        // Before the unit's own records (an opened design's run, which the cut and the jump onto it
        // the file had come before): the jump lands on its first needle point, a cut follows the
        // needle point numbered last in the block.
        let leading = at.is_some_and(|i| i < u.own.start);
        let mut node = u.origin.node;
        let index = match (numbered.get(r.unit).copied().flatten(), place) {
            (Some((_, first)), Some(_)) => {
                let count = counts.get(r.unit).copied().unwrap_or(0);
                match r.stitch.command {
                    Command::Stitch => {
                        if let Some(c) = counts.get_mut(r.unit) {
                            *c += 1;
                        }
                        last_needle = Some((u.block, node, first + count));
                        Some(first + count)
                    }
                    // A jump: the needle point it lands on.
                    Command::Jump => Some(first + count),
                    // A cut or a stop: the needle point before it.
                    _ => (first + count).checked_sub(1),
                }
            }
            (Some((_, first)), None) if leading => match r.stitch.command {
                Command::Jump => Some(first),
                Command::Trim | Command::Stop => match last_needle.filter(|(b, ..)| *b == u.block) {
                    Some((_, before, k)) => {
                        node = before;
                        Some(k)
                    }
                    None => None,
                },
                _ => None,
            },
            _ => None,
        };
        if index.is_none() && r.stitch.command == Command::Stitch {
            last_needle = None;
        }
        let length = match r.stitch.command {
            Command::Stitch => r.sewn(),
            Command::Jump => Some(r.moved()),
            _ => None,
        };
        if ask.from.is_some_and(|from| r.number < from) || ask.to.is_some_and(|to| r.number > to) {
            return;
        }
        if ask.kinds.as_ref().is_some_and(|k| !k.contains(&kind)) {
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
        f(Row { record: r.number, kind, at: [r.stitch.x, r.stitch.y], length, id, node, block: u.block + 1, unit: r.unit + 1, index });
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
                "kind": row.kind.name(),
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
        "warnings": plan.messages(),
    })
}

pub(super) const SELECT: &str = "embroidery.selectNeedlePoints";
/// The keys `embroidery.selectNeedlePoints` takes besides the embroidery options.
const SELECT_KEYS: [&str; 10] = ["records", "needlePoints", "ranges", "ids", "kinds", "shorterThan", "longerThan", "mode", "rect", "polygon"];
/// The keys that select by records (planning the design).
const BY_RECORDS: [&str; 5] = ["records", "ids", "kinds", "shorterThan", "longerThan"];
/// The keys that select by a shape (a marquee or a lasso), and the scope they take.
const BY_SHAPE: [&str; 2] = ["rect", "polygon"];

/// `embroidery.selectNeedlePoints {records?, ids?, kinds?, shorterThan?, longerThan? | needlePoints?,
/// ranges? | rect?, polygon?, ids?, mode?, …the embroidery options}`: needle points selected
/// (`Selection.anchors`, as Direct Selection selects anchors) by the records that sew them
/// (planning the design, as `embroidery.stitches` lists them: a needle point itself, a jump the one
/// it lands on, a cut or a stop the one it follows; a record the stitch commands can't edit passed
/// over and counted), by their addresses (planning nothing), or by a shape: those inside a rectangle
/// or a polygon (a marquee, a lasso), of the paths of stitches inside `ids`, else inside the object
/// isolated, else in the whole document (planning nothing; the paths that can't be edited passed
/// over and counted). `mode`: set (the default), add, toggle or subtract. A selection, not an undo
/// step.
pub(super) fn select(s: &mut crate::Session, p: &Value) -> crate::Result<Value> {
    let o = super::object(p, SELECT)?;
    let mode = match o.get("mode") {
        None | Some(Value::Null) => "set",
        Some(v) => ["set", "add", "toggle", "subtract"]
            .into_iter()
            .find(|m| v.as_str() == Some(m))
            .ok_or_else(|| bad(SELECT, format!("`mode` {v}: set, add, toggle or subtract")))?,
    };
    let by_shape = BY_SHAPE.iter().any(|k| o.contains_key(*k));
    let by_records = !by_shape && BY_RECORDS.iter().any(|k| o.contains_key(*k));
    let by_address = o.contains_key("needlePoints") || o.contains_key("ranges");
    if usize::from(by_shape) + usize::from(by_records) + usize::from(by_address) != 1 {
        return Err(bad(
            SELECT,
            "give records (records, ids, kinds, shorterThan, longerThan), addresses (needlePoints, ranges) or a shape (rect or polygon, and ids): one of them",
        ));
    }
    let mut picked: std::collections::BTreeMap<NodeId, BTreeSet<usize>> = Default::default();
    let mut passed = 0usize;
    if by_shape {
        super::only(&o, &["rect", "polygon", "ids", "mode"], SELECT)?;
        let area = shape(&o)?;
        let st = s.doc()?;
        let doc = st.doc.clone();
        let roots: Vec<NodeId> = match o.get("ids").filter(|v| !v.is_null()) {
            Some(v) => {
                let items = v.as_array().ok_or_else(|| bad(SELECT, format!("`ids` {v}: a list of ids")))?;
                if items.len() > needles::MAX_PATHS {
                    return Err(bad(SELECT, format!("`ids` lists {}: at most {}", items.len(), needles::MAX_PATHS)));
                }
                items
                    .iter()
                    .map(|i| i.as_u64().map(NodeId).ok_or_else(|| bad(SELECT, format!("`ids` entry {i}: an id"))))
                    .collect::<crate::Result<_>>()?
            }
            None => st.isolation.map_or_else(|| doc.layers.iter().map(|l| l.id).collect(), |i| vec![i]),
        };
        if let Some(index) = super::needle_index::NeedleIndex::of(&doc, &roots, None) {
            passed = index.scope().refused;
            for (id, k) in index.within(&area) {
                picked.entry(id).or_default().extend(k);
            }
        }
    } else if by_address {
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
            Some(k) if count < needles::MAX_NEEDLE_POINTS => {
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
            let ix = st.index();
            let now = needles::selected_in(sel, *id, &ix);
            let next: BTreeSet<usize> = match mode {
                "toggle" => now.symmetric_difference(k).copied().collect(),
                "subtract" => now.difference(k).copied().collect(),
                _ => now.union(k).copied().collect(),
            };
            selected += next.len();
            needles::select_in(sel, *id, &ix, &next);
        }
    })?;
    Ok(json!({"selectedCount": selected, "pathCount": picked.len(), "passedOver": passed}))
}

/// The shape `o` gives a selection by a shape: `rect` `[x0, y0, x1, y1]` or `polygon` `[[x, y]…]`
/// (3 to [`MAX_LASSO_POINTS`] points), millimetres from the document's origin, y down, every one a
/// number within the document's coordinates; as document points.
fn shape(o: &Map<String, Value>) -> crate::Result<super::needle_index::Area> {
    use super::needle_index::{Area, Lasso, MAX_LASSO_POINTS};
    use vectorcraft_geom::{Point, Rect};
    let number = |v: &Value| v.as_f64().map(vectorcraft_stitch::mm_to_pt).filter(|x| x.is_finite() && x.abs() <= vectorcraft_doc::MAX_COORD);
    let point = |v: &Value| match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => Some(Point::new(number(x)?, number(y)?)),
        _ => None,
    };
    match (o.get("rect"), o.get("polygon")) {
        (Some(r), None) => match r.as_array().map(Vec::as_slice) {
            Some([x0, y0, x1, y1]) => {
                let (Some(x0), Some(y0), Some(x1), Some(y1)) = (number(x0), number(y0), number(x1), number(y1)) else {
                    return Err(bad(SELECT, format!("`rect` {r}: four numbers within the document's coordinates")));
                };
                Ok(Area::Rect(Rect::from_points(Point::new(x0, y0), Point::new(x1, y1))))
            }
            _ => Err(bad(SELECT, format!("`rect` {r}: [x0, y0, x1, y1], millimetres"))),
        },
        (None, Some(v)) => {
            let items = v.as_array().ok_or_else(|| bad(SELECT, format!("`polygon` {v}: a list of [x, y]")))?;
            if !(3..=MAX_LASSO_POINTS).contains(&items.len()) {
                return Err(bad(SELECT, format!("`polygon` has {} points: 3 to {MAX_LASSO_POINTS}", items.len())));
            }
            let points: Vec<Point> = items
                .iter()
                .map(|p| point(p).ok_or_else(|| bad(SELECT, format!("`polygon` point {p}: [x, y], millimetres within the document's coordinates"))))
                .collect::<crate::Result<_>>()?;
            Lasso::new(&points).map(Area::Lasso).ok_or_else(|| bad(SELECT, "`polygon` encloses no area"))
        }
        _ => Err(bad(SELECT, "give rect or polygon, not both")),
    }
}
