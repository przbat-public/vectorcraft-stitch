//! `embroidery.info`: the objects' embroidery settings as they are kept, and for each part (all
//! their fills, all their strokes) every value it is planned with and where it comes from, merged
//! over the objects: the value they share, or none and `mixed`; the layer it comes from (the
//! object, the call's option, the document and which part of its settings, the default; a rule is
//! a value, `"auto"`); how many set it themselves; and what it would be unset. Read without
//! planning (the stitch each part takes is the walk's own rule, `digitize::part_rule`); with
//! `plan`, also what the design's plan sews of each part.
//!
//! The reply is [`ObjectSettings`] serialised: the Object tab reads it back typed, through
//! [`Session::embroidery_info`] on the interface thread, or through the stitch preview's planning
//! thread as the other read commands (`Planner::query`), with ids since a planner has no selection.
//! Each object's values are merged as it is read, so that a reply about a whole document's objects
//! holds a few values per key, not a row per object.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartSettings, PartValue};
use vectorcraft_doc::{AppearanceItem, Document, Node, NodeId, NodeKind};
use vectorcraft_geom::BezPath;
use vectorcraft_stitch::plan::{Params, SATIN_EDGE_UNDERLAY, SATIN_ZIGZAG_UNDERLAY, Style, TATAMI_UNDERLAY_MIN, Underlay, satin_auto_underlay};
use vectorcraft_stitch::{Command, pt_to_mm};

use super::digitize::{outlined_rule, part_rule, sews};
use super::objects::{MAX_IDS, MAX_SKIPPED, has_part, part_kind, takers, value_json};
use super::options::stitch_id;
use super::plan::{Choice, DesignPlan, Part};
use super::printed::{keeps_its_path, needs_expanding};
use super::resolve::report::{REPORTED, Resolved, Source};
use super::resolve::{DocLayer, Found, PartLayer, Stitch, UnitSettings};
use super::stats::{LIST_LIMIT, Page};
use super::{Options, object, options_in, planned, with_options};
use crate::cmd::bad;
use crate::{EngineError, Result, Session};

/// The keys `embroidery.info` takes besides the embroidery options.
pub(super) const KEYS: [&str; 4] = ["ids", "plan", Page::KEYS[0], Page::KEYS[1]];

/// What `embroidery.info` is asked: the objects (`None`: the selection), whether to plan, the
/// stretch of objects listed, and the call's options.
pub(super) struct Ask {
    pub(super) ids: Option<Vec<NodeId>>,
    pub(super) plan: bool,
    pub(super) page: Page,
    pub(super) opts: Options,
}

impl Ask {
    /// The ask params `o` make, checked as the command checks them.
    pub(super) fn from_params(o: &Map<String, Value>, cmd: &str) -> Result<Self> {
        let opts = options_in(o, &KEYS, cmd)?;
        let plan = match o.get("plan") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or_else(|| bad(cmd, format!("`plan` {v}: true or false")))?,
        };
        let ids = match o.get("ids") {
            None | Some(Value::Null) => None,
            Some(Value::Array(a)) if a.len() > MAX_IDS => return Err(bad(cmd, format!("`ids` lists {} objects: at most {MAX_IDS}", a.len()))),
            Some(Value::Array(a)) => {
                Some(a.iter().map(|v| v.as_u64().map(NodeId).ok_or_else(|| bad(cmd, format!("`ids` {v}: object ids")))).collect::<Result<_>>()?)
            }
            Some(v) => return Err(bad(cmd, format!("`ids` {v}: a list of object ids"))),
        };
        Ok(Self { ids, plan, page: Page::from_params(o, cmd)?, opts })
    }
}

/// `embroidery.info` on the active document.
pub(super) fn info(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.info";
    let o = object(p, C)?;
    let ask = Ask::from_params(&o, C)?;
    let ids = match &ask.ids {
        Some(ids) => ids.clone(),
        None => s.doc()?.selection.objects.clone(),
    };
    let d = &s.doc()?.doc;
    if let Some(missing) = ids.iter().find(|id| d.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    let (mut warnings, mut plan, mut doc) = (Vec::new(), None, d.clone());
    if ask.plan {
        // The settings are the answer even when the design can't be planned.
        match planned(s, &ask.opts, false, C) {
            Ok((p, d)) => (plan, doc) = (Some(p), d),
            Err(EngineError::Cancelled) => return Err(EngineError::Cancelled),
            Err(e) => warnings.push(e.to_string()),
        }
    }
    let reply = reply(&doc, &ids, &ask.opts, plan.as_deref(), ask.page, warnings);
    Ok(with_options(to_json(&reply)?, &ask.opts))
}

/// A typed reply as the command gives it.
pub(super) fn to_json(reply: &ObjectSettings) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| EngineError::Other(format!("embroidery.info: {e}")))
}

impl Session {
    /// `embroidery.info` of objects `ids` of the active document (each listed up to the reply's
    /// usual limit), typed, with the call's options `opts`; with `plan`, what the plan sews of each
    /// part. A design that can't be planned leaves the settings and says why in `warnings`.
    pub fn embroidery_info(&mut self, ids: &[NodeId], plan: bool, opts: &Options) -> Result<ObjectSettings> {
        let d = &self.doc()?.doc;
        if let Some(missing) = ids.iter().find(|id| d.node(**id).is_none()) {
            return Err(EngineError::NoNode(*missing));
        }
        let (mut warnings, mut planned_with, mut doc) = (Vec::new(), None, d.clone());
        if plan {
            match planned(self, opts, false, "embroidery.info") {
                Ok((p, d)) => (planned_with, doc) = (Some(p), d),
                Err(EngineError::Cancelled) => return Err(EngineError::Cancelled),
                Err(e) => warnings.push(e.to_string()),
            }
        }
        Ok(reply(&doc, ids, opts, planned_with.as_deref(), Page { offset: 0, limit: LIST_LIMIT }, warnings))
    }

    /// The settings of objects `ids` merged, as [`Session::embroidery_info`] gives them without
    /// planning, but with no object listed (`objects` empty; the skipped ones still are): what the
    /// Object tab reads, which merges and never lists.
    pub fn embroidery_merged(&mut self, ids: &[NodeId]) -> Result<ObjectSettings> {
        let d = &self.doc()?.doc;
        if let Some(missing) = ids.iter().find(|id| d.node(**id).is_none()) {
            return Err(EngineError::NoNode(*missing));
        }
        Ok(reply(d, ids, &Options::default(), None, Page { offset: 0, limit: 0 }, Vec::new()))
    }
}

/// `embroidery.info`'s reply, its options aside: the objects it takes, listed from `offset`, those
/// it passes over, the cut after them, and each part merged over them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectSettings {
    pub object_count: usize,
    pub offset: usize,
    pub truncated: bool,
    pub objects: Vec<ObjectInfo>,
    pub skipped_count: usize,
    /// At most 1,000 (`MAX_SKIPPED`), each with its reason, as `embroidery.set` lists them.
    pub skipped: Vec<SkippedObject>,
    pub trim_after: Merged<bool>,
    /// Whether the machine stops after the objects, read as `trim_after` is.
    pub stop_after: Merged<bool>,
    /// The objects converted to manual stitches among those given (a manual run standing for the
    /// object it was converted from): `None` when there are none.
    pub manual: Option<ManualInfo>,
    /// Whether the design was planned (`sewing` counted).
    pub planned: bool,
    pub warnings: Vec<String>,
    /// `None` when none of the objects has the part.
    pub fill: Option<PartInfo>,
    pub stroke: Option<PartInfo>,
}

/// The objects converted to manual stitches among a selection: how many, how many can be restored
/// (not locked), how many were edited since converting (Restore then asks first), their runs and
/// needle points, and the least and the most their stitches were stretched by since converting
/// (their originals' frames' singular values; `None` when one can't say).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualInfo {
    pub objects: usize,
    pub restorable: usize,
    pub edited: usize,
    pub runs: usize,
    pub needle_points: usize,
    pub scale: Option<[f64; 2]>,
}

/// One object as listed: its kind (`path` or `compound`), how many fills and strokes, whether the
/// walk sews its appearance as pieces, its settings as kept (each part's keys by name, `None`:
/// none), and what it keeps that this version doesn't read, as file keys (`branch`, `fill.grain`,
/// `fill.stitch` for an id this version doesn't know, `embroidery` for settings that are no
/// object).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub id: u64,
    pub kind: String,
    pub fills: usize,
    pub strokes: usize,
    pub expanded: bool,
    pub settings: KeptParts,
    pub kept: Vec<String>,
}

/// An object's parts' settings as kept, each set key by its name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptParts {
    pub fill: Option<Map<String, Value>>,
    pub stroke: Option<Map<String, Value>>,
}

/// An object passed over, and why (`type`, `instance`, `literal`, `live`…: a list that may grow).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkippedObject {
    pub id: u64,
    pub reason: String,
}

/// A value merged over objects: the one they share (`None` when they differ, or none sets it),
/// how many set it, and whether they differ.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Merged<T> {
    pub value: Option<T>,
    pub set_count: usize,
    pub mixed: bool,
}

/// One part merged over the objects that have it: how many, how many items of it, how the walk
/// sews it (`sewn`: `tatami`, `satin`, `running`, `none`; `rule`, as `embroidery.sequence` names
/// it; `reason` when nothing of it is sewn: `off`, `unpainted`, `hidden`, `notPrinted`; each
/// `None` when they differ, lists that may grow), every key it is planned with, its entry and exit
/// points (document points), and with `plan` what the plan sews of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartInfo {
    pub object_count: usize,
    pub item_count: usize,
    pub sewn: Option<String>,
    pub rule: Option<String>,
    pub reason: Option<String>,
    /// By key: `stitch`, `spacing`, `length`, `angle`, `pull`, `pullPercent`, `push`, `expand`,
    /// `narrowAsSatin`, `underlay`, `splitLength`.
    pub settings: BTreeMap<String, KeyInfo>,
    pub entry: Merged<[f64; 2]>,
    pub exit: Merged<[f64; 2]>,
    pub sewing: Option<Sewing>,
}

/// One key of a part: its value (`null` when they differ, `"auto"` for a rule's), where it comes
/// from (`object`, `export`, `document` with its `section`, `fabric` or `machine`, or `default`; a
/// list that may grow), how many objects set it as this version reads them, what unsetting it
/// gives, what a rule chose where the value is `"auto"`, and whether the planner uses it for the
/// stitch the part is sewn in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub value: Value,
    pub source: Option<String>,
    pub section: Option<String>,
    pub set_count: usize,
    pub mixed: bool,
    pub inherited: Option<Inherited>,
    pub rule: Option<Value>,
    pub applies: Option<bool>,
}

/// What a key is unset: its value and where that comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inherited {
    pub value: Value,
    pub source: String,
    pub section: Option<String>,
}

/// What the plan sews of a part: its units and their needle-down stitches, records, jumps and cuts,
/// and how far their stitches miss the entry and exit points they were planned toward (the largest
/// miss of its units, millimetres; `null` each where none was set).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sewing {
    pub unit_count: usize,
    pub stitches: usize,
    pub records: usize,
    pub jumps: usize,
    pub trims: usize,
    pub missed_by: super::order::EntryExit<f64>,
}

/// The reply for objects `ids` of `doc` with the call's options `opts`, with what `plan` (the
/// design's, when planned) sews of each part.
pub(super) fn reply(doc: &Document, ids: &[NodeId], opts: &Options, plan: Option<&DesignPlan>, page: Page, warnings: Vec<String>) -> ObjectSettings {
    let (take, skipped) = takers(doc, ids);
    let nodes: Vec<&Node> = take.iter().filter_map(|id| doc.node(*id)).collect();
    let layer = DocLayer::of(doc);
    let mut found = Found::default();
    let fill = part_section(doc, &nodes, PartKind::Fill, opts, &layer, plan, &mut found);
    let stroke = part_section(doc, &nodes, PartKind::Stroke, opts, &layer, plan, &mut found);
    ObjectSettings {
        object_count: nodes.len(),
        offset: page.offset,
        truncated: page.truncated(nodes.len()),
        objects: nodes.get(page.range(nodes.len())).unwrap_or_default().iter().map(|n| object_info(n)).collect(),
        skipped_count: skipped.len(),
        skipped: skipped.iter().take(MAX_SKIPPED).map(|(id, why)| SkippedObject { id: id.0, reason: why.id().to_string() }).collect(),
        trim_after: trim_after(doc, ids),
        stop_after: stop_after(doc, ids),
        manual: super::manual::summary(doc, ids),
        planned: plan.is_some(),
        warnings: warnings.into_iter().chain(found.notes().map(String::from)).collect(),
        fill,
        stroke,
    }
}

/// Object `n` as listed ([`ObjectInfo`]).
fn object_info(n: &Node) -> ObjectInfo {
    let count = |kind: PartKind| n.appearance.items.iter().filter(|i| part_kind(i) == kind).count();
    let e = n.extras().embroidery.as_deref();
    let part = |kind: PartKind| e.and_then(|e| e.part(kind)).map(stored);
    ObjectInfo {
        id: n.id.0,
        kind: if matches!(n.kind, NodeKind::Compound { .. }) { "compound" } else { "path" }.to_string(),
        fills: count(PartKind::Fill),
        strokes: count(PartKind::Stroke),
        expanded: needs_expanding(n),
        settings: KeptParts { fill: part(PartKind::Fill), stroke: part(PartKind::Stroke) },
        kept: e.map(kept).unwrap_or_default(),
    }
}

/// What settings `e` keep that this version doesn't read, as paths of their file keys: keys a newer
/// version wrote, of the object (`branch`), of a part (`fill.grain`) or of its points
/// (`fill.points.rails`); a part's stitch or underlay this version doesn't know (`fill.stitch`); and
/// `embroidery` when the settings are no object at all.
fn kept(e: &Embroidery) -> Vec<String> {
    let mut kept: Vec<String> = e.unread.iter().map(|_| "embroidery".to_string()).collect();
    kept.extend(e.extra.keys().cloned());
    for kind in [PartKind::Fill, PartKind::Stroke] {
        let Some(p) = e.part(kind) else { continue };
        let k = kind.id();
        if p.stitch.as_deref().is_some_and(|id| Stitch::from_id(id).is_none()) {
            kept.push(format!("{k}.stitch"));
        }
        if p.underlay.as_deref().is_some_and(|id| Underlay::from_id(id).is_none()) {
            kept.push(format!("{k}.underlay"));
        }
        kept.extend(p.extra.keys().map(|key| format!("{k}.{key}")));
        kept.extend(p.points.iter().flat_map(|pts| pts.extra.keys()).map(|key| format!("{k}.points.{key}")));
    }
    kept
}

/// A part's settings as kept, each set key by its name.
fn stored(p: &PartSettings) -> Map<String, Value> {
    PartKey::ALL.into_iter().filter_map(|k| p.get(k).map(|v| (k.name().to_string(), value_json(&v)))).collect()
}

/// The cut after the objects as given that are sewn (a group as one, as `embroidery.setTrimAfter`
/// marks it; a run of stitches opened from a file by its file's cut; type, images and the like left
/// out, as the command passes them over): whether they ask for it, how many do, and whether they
/// differ.
fn trim_after(doc: &Document, ids: &[NodeId]) -> Merged<bool> {
    let mut seen = HashSet::new();
    let (mut given, mut set) = (0usize, 0usize);
    for n in ids.iter().filter(|id| seen.insert(**id)).filter_map(|id| doc.node(*id)).filter(|n| super::objects::cut_after_sewn(n)) {
        given += 1;
        set += usize::from(n.extras().embroidery.as_deref().is_some_and(|e| e.trim_after || e.stitches.as_ref().is_some_and(|r| r.trim_after)));
    }
    let mixed = set > 0 && set < given;
    Merged { value: (!mixed).then_some(set > 0), set_count: set, mixed }
}

/// The stop after the objects as given that are sewn, read as [`trim_after`] reads the cut.
fn stop_after(doc: &Document, ids: &[NodeId]) -> Merged<bool> {
    let mut seen = HashSet::new();
    let (mut given, mut set) = (0usize, 0usize);
    for n in ids.iter().filter(|id| seen.insert(**id)).filter_map(|id| doc.node(*id)).filter(|n| super::objects::cut_after_sewn(n)) {
        given += 1;
        set += usize::from(n.extras().embroidery.as_deref().is_some_and(|e| e.stop_after || e.stitches.as_ref().is_some_and(|r| r.stop_after)));
    }
    let mixed = set > 0 && set < given;
    Merged { value: (!mixed).then_some(set > 0), set_count: set, mixed }
}

/// `n`'s path, document points: its own, or its compound path's members' together.
fn path_of(n: &Node) -> BezPath {
    match &n.kind {
        NodeKind::Compound { children, .. } => {
            let mut bp = BezPath::new();
            for c in children {
                if let Some(p) = c.path_data() {
                    bp.extend(p.to_bezpath());
                }
            }
            bp
        }
        _ => n.path_data().map(|p| p.to_bezpath()).unwrap_or_default(),
    }
}

/// The rule id a choice is reported by, as `embroidery.sequence` names it.
pub(crate) fn rule_id(why: Choice) -> &'static str {
    match why {
        Choice::Fill => "fill",
        Choice::WideStroke => "wideStroke",
        Choice::NarrowStroke => "narrowStroke",
        Choice::StrokeFilled => "strokeFilled",
        Choice::StrokeOutlined => "strokeOutlined",
        Choice::Forced(_) | Choice::SatinFilled(_) => "forced",
        Choice::Literal => "literal",
        Choice::Manual => "manual",
    }
}

/// Why the walk sews nothing of object `n` of `doc`, whatever its parts: `hidden` (it, or a group or
/// layer it is in, is hidden or has no opacity), `notPrinted` (on a template or non-printing
/// layer); `None` when it is printed.
fn unprinted(doc: &Document, n: &Node) -> Option<&'static str> {
    let above: Vec<&Node> = doc.ancestry(n.id).unwrap_or_default().iter().filter_map(|id| doc.node(*id)).collect();
    if !n.visible || n.opacity <= 0.0 || above.iter().any(|a| !a.visible || a.opacity <= 0.0) {
        return Some("hidden");
    }
    let unprinted_layer = |a: &&Node| matches!(a.kind, NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. });
    above.iter().any(unprinted_layer).then_some("notPrinted")
}

/// What Auto chose for a part sewn in `style` with `p`, by key: a satin's underlay by its width and
/// its spacing by its width (and the fabric's share of it); a fill's underlay by its size, which
/// only the planned area tells (`chose` null: the edge run always, a tatami layer too where the
/// fill is at least [`TATAMI_UNDERLAY_MIN`] across).
fn rules(style: Style, p: &Params) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    match (style, p.underlay) {
        (Style::Satin, Underlay::Auto) => out.push((
            "underlay",
            json!({"id": "underlayByWidth", "chose": satin_auto_underlay(p.width).id(), "values": {"edgeFrom": SATIN_EDGE_UNDERLAY, "zigzagFrom": SATIN_ZIGZAG_UNDERLAY}}),
        )),
        (Style::Fill, Underlay::Auto) => out.push(("underlay", json!({"id": "underlayBySize", "chose": null, "values": {"tatamiFrom": TATAMI_UNDERLAY_MIN}}))),
        _ => {}
    }
    if style == Style::Satin && p.auto_spacing {
        out.push(("spacing", json!({"id": "spacingByWidth", "chose": p.column_spacing(p.width), "values": {"share": p.spacing_scale}})));
    }
    out
}

/// What objects share, merged as each is added: nothing yet, the one value, or several.
enum Common<T> {
    Empty,
    One(T),
    Mixed,
}

impl<T: PartialEq> Common<T> {
    fn add(&mut self, v: T) {
        match self {
            Common::Empty => *self = Common::One(v),
            Common::One(x) if *x == v => {}
            Common::One(_) | Common::Mixed => *self = Common::Mixed,
        }
    }

    /// The value they share, `None` when they differ (or none was added).
    fn get(self) -> Option<T> {
        match self {
            Common::One(v) => Some(v),
            Common::Empty | Common::Mixed => None,
        }
    }
}

/// One key of a part, merged as each object is added ([`KeyInfo`]).
struct KeyMerge {
    value: Common<Value>,
    source: Common<Source>,
    set: usize,
    inherited: Common<Resolved>,
    rule: Common<Option<Value>>,
    applies: Common<bool>,
}

impl KeyMerge {
    fn new() -> Self {
        Self { value: Common::Empty, source: Common::Empty, set: 0, inherited: Common::Empty, rule: Common::Empty, applies: Common::Empty }
    }

    fn info(self) -> KeyInfo {
        let value = self.value.get();
        let source = self.source.get();
        KeyInfo {
            mixed: value.is_none(),
            value: value.unwrap_or(Value::Null),
            source: source.map(|s| s.id().to_string()),
            section: source.and_then(|s| s.section()).map(String::from),
            set_count: self.set,
            inherited: self.inherited.get().map(|r| Inherited {
                value: r.value,
                source: r.source.id().to_string(),
                section: r.source.section().map(String::from),
            }),
            rule: self.rule.get().flatten(),
            applies: self.applies.get(),
        }
    }
}

/// An entry or exit point merged as each object is added.
struct PointMerge {
    value: Common<Option<[f64; 2]>>,
    set: usize,
}

impl PointMerge {
    fn add(&mut self, p: Option<&PartSettings>, key: PartKey) {
        let point = p.and_then(|p| p.get(key)).and_then(|v| match v {
            PartValue::Point(p) => Some([p.x, p.y]),
            PartValue::Id(_) | PartValue::Number(_) | PartValue::Sides(_) => None,
        });
        self.set += usize::from(point.is_some());
        self.value.add(point);
    }

    fn merged(self) -> Merged<[f64; 2]> {
        let value = self.value.get();
        Merged { mixed: value.is_none(), value: value.flatten(), set_count: self.set }
    }
}

/// Part `kind` of `nodes` of `doc`, merged ([`PartInfo`]; `None` when none of them has the part).
fn part_section(
    doc: &Document,
    nodes: &[&Node],
    kind: PartKind,
    opts: &Options,
    layer: &DocLayer,
    plan: Option<&DesignPlan>,
    found: &mut Found,
) -> Option<PartInfo> {
    let (mut object_count, mut item_count) = (0usize, 0usize);
    let (mut sewn, mut rule, mut reason) = (Common::Empty, Common::Empty, Common::Empty);
    let mut keys: Vec<(&'static str, KeyMerge)> = REPORTED.iter().map(|k| (*k, KeyMerge::new())).collect();
    let (mut entry, mut exit) = (PointMerge { value: Common::Empty, set: 0 }, PointMerge { value: Common::Empty, set: 0 });
    let mut with = Vec::new();
    for n in nodes.iter().copied().filter(|n| has_part(n, kind)) {
        object_count += 1;
        item_count += n.appearance.items.iter().filter(|i| part_kind(i) == kind).count();
        if plan.is_some() {
            with.push(n.id);
        }
        let kept = n.extras().embroidery.as_deref().and_then(|e| e.part(kind));
        let object = kept.map_or_else(PartLayer::default, |p| PartLayer::read(p, found));
        let settings = UnitSettings::new(object, n.bbox_angle, opts, layer);
        let item = n.appearance.items.iter().find(|i| part_kind(i) == kind && sews(i));
        let (sewn_as, chose, why_not, style) = sewn_as(doc, n, kind, &settings);
        sewn.add(sewn_as);
        rule.add(chose);
        reason.add(why_not);
        // A satin's width: its stroke's.
        let width = match item {
            Some(AppearanceItem::Stroke(st)) if style == Style::Satin => pt_to_mm(st.width).max(0.0),
            _ => 0.0,
        };
        let rules = if sewn_as == "none" { Vec::new() } else { rules(style, &settings.params(style, width)) };
        for reported in settings.report(style) {
            let Some((_, k)) = keys.iter_mut().find(|(name, _)| *name == reported.name) else { continue };
            // Set: what the object's own settings give, as this version reads them (a stitch or an
            // underlay it doesn't know is kept unread, and the layers below give the value).
            k.set += usize::from(reported.now.source == Source::Object);
            k.value.add(reported.now.value.clone());
            k.source.add(reported.now.source);
            k.inherited.add(reported.inherited);
            k.rule.add(rules.iter().find(|(key, _)| *key == reported.name).map(|(_, v)| v.clone()));
            // A part not sewn uses none of its keys but the stitch that says so.
            k.applies.add(if sewn_as == "none" { reported.name == "stitch" } else { reported.applies });
        }
        entry.add(kept, PartKey::Entry);
        exit.add(kept, PartKey::Exit);
    }
    if object_count == 0 {
        return None;
    }
    Some(PartInfo {
        object_count,
        item_count,
        sewn: sewn.get().map(String::from),
        rule: rule.get().flatten().map(String::from),
        reason: reason.get().flatten().map(String::from),
        settings: keys.into_iter().map(|(name, k)| (name.to_string(), k.info())).collect(),
        entry: entry.merged(),
        exit: exit.merged(),
        sewing: plan.map(|p| sewn_of(p, &with, kind)),
    })
}

/// How part `kind` of `n` is sewn with `settings`, without a plan: the stitch's id (`none` when it
/// isn't), the rule that chose it, why it isn't sewn, and its style. The part's first item of the
/// kind the walk sews says how: as it is, or, for a stroke the walk outlines first (on an object it
/// expands, reshaped by a brush or an effect), as the area it paints.
fn sewn_as(doc: &Document, n: &Node, kind: PartKind, settings: &UnitSettings) -> (&'static str, Option<&'static str>, Option<&'static str>, Style) {
    let item = n.appearance.items.iter().find(|i| part_kind(i) == kind && sews(i));
    let outlined = |item: &AppearanceItem| kind == PartKind::Stroke && needs_expanding(n) && !keeps_its_path(&n.appearance, item);
    match (unprinted(doc, n), item) {
        (Some(why), _) => ("none", None, Some(why), Style::Fill),
        (None, _) if !settings.sewn() => ("none", None, Some("off"), Style::Fill),
        (None, None) => ("none", None, Some("unpainted"), Style::Fill),
        (None, Some(item)) => {
            let (style, why) = if outlined(item) { outlined_rule(settings.forcing()) } else { part_rule(item, &path_of(n), settings.forcing()) };
            (stitch_id(style), Some(rule_id(why)), None, style)
        }
    }
}

/// The rows of `n`'s fill, when it is sewn as tatami (rows at an angle): their angle (degrees
/// counter-clockwise on the page, as the commands take it: the object's own, else the export's or
/// the default rows, turned with the object) and whether the object sets it. Read from the document
/// alone with options `opts` over the document's `layer`, as `embroidery.info {plan: false}`
/// reports the fill's `angle`; `None` for a fill sewn otherwise, or not at all.
pub(crate) fn fill_rows(doc: &Document, n: &Node, opts: &Options, layer: &DocLayer) -> Option<(f64, bool)> {
    if !has_part(n, PartKind::Fill) {
        return None;
    }
    let mut found = Found::default();
    let kept = n.extras().embroidery.as_deref().and_then(|e| e.part(PartKind::Fill));
    let object = kept.map_or_else(PartLayer::default, |p| PartLayer::read(p, &mut found));
    let settings = UnitSettings::new(object, n.bbox_angle, opts, layer);
    let (sewn, _, _, style) = sewn_as(doc, n, PartKind::Fill, &settings);
    if sewn == "none" || style != Style::Fill {
        return None;
    }
    let angle = settings.angle(&object);
    let degrees = angle.value.as_f64().filter(|a| a.is_finite())?;
    Some((degrees, angle.source == Source::Object))
}

/// What `plan` sews of part `kind` of objects `ids` (as a person selects them).
fn sewn_of(plan: &DesignPlan, ids: &[NodeId], kind: PartKind) -> Sewing {
    let ids: HashSet<NodeId> = ids.iter().copied().collect();
    let mut out = Sewing::default();
    for (i, u) in plan.units.iter().enumerate() {
        let of_kind = match (u.origin.part, kind) {
            (Part::Fill(_), PartKind::Fill) | (Part::Stroke(_), PartKind::Stroke) => true,
            (Part::Fill(_) | Part::Stroke(_) | Part::Literal | Part::Manual, _) => false,
        };
        if of_kind && ids.contains(&u.origin.selected()) {
            out.add(plan, i);
        }
    }
    out
}

impl Sewing {
    /// Unit `unit` of `plan` counted in.
    fn add(&mut self, plan: &DesignPlan, unit: usize) {
        self.unit_count += 1;
        let [entry, exit] = plan.missed(unit);
        let larger = |kept: Option<f64>, new: Option<f64>| match (kept, new) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.missed_by.entry = larger(self.missed_by.entry, entry.map(super::stats::mm));
        self.missed_by.exit = larger(self.missed_by.exit, exit.map(super::stats::mm));
        for s in plan.unit_stitches(unit) {
            self.records += 1;
            match s.command {
                Command::Stitch => self.stitches += 1,
                Command::Jump => self.jumps += 1,
                Command::Trim => self.trims += 1,
                _ => {}
            }
        }
    }
}

/// What a plan sews of the objects selected, by part: their fills, their strokes, the runs of
/// stitches opened from a machine file among them, and their manual stitches. Kept by part, as
/// `embroidery.info` reports each part's sewing, which its test checks it against; the Object tab
/// counts all of it ([`SelectionSewing::stitches`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectionSewing {
    pub fill: Sewing,
    pub stroke: Sewing,
    pub literal: Sewing,
    pub manual: Sewing,
}

impl SelectionSewing {
    /// The needle-down stitches of all of it.
    pub fn stitches(&self) -> usize {
        self.fill.stitches + self.stroke.stitches + self.literal.stitches + self.manual.stitches
    }
}

/// What `plan`, the plan of `doc`, sews of objects `selected` as a person selects them: each one, or
/// what a group or a layer among them holds (a symbol instance's art, an expanded object's pieces
/// by the object selected). Read from a plan already made (the Object tab's count, from the plan the
/// stitch preview shows), never planned: a walk of the selected objects and of the plan's units.
pub fn selection_sewing(doc: &Document, plan: &DesignPlan, selected: &[NodeId]) -> SelectionSewing {
    // Sorted and searched: on the interface thread, for every plan that lands, a hash set's SipHash
    // was the cost of a large selection.
    let mut under: Vec<NodeId> = Vec::new();
    for n in selected.iter().filter_map(|id| doc.node(*id)) {
        n.walk(&mut |c| under.push(c.id));
    }
    under.sort_unstable();
    under.dedup();
    let mut out = SelectionSewing::default();
    for (i, u) in plan.units.iter().enumerate().filter(|(_, u)| under.binary_search(&u.origin.selected()).is_ok()) {
        match u.origin.part {
            Part::Fill(_) => out.fill.add(plan, i),
            Part::Stroke(_) => out.stroke.add(plan, i),
            Part::Literal => out.literal.add(plan, i),
            Part::Manual => out.manual.add(plan, i),
        }
    }
    out
}
