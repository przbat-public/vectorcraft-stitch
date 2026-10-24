//! `embroidery.info`: the objects' embroidery settings as they are kept, and for each part (all
//! their fills, all their strokes) every value it is planned with and where it comes from, merged
//! over the objects: the value they share, or none and `mixed`; the layer it comes from (the
//! object, the call's option, the document and which part of its settings, the default; a rule is
//! a value, `"auto"`); how many set it themselves; and what it would be unset. Read without
//! planning (the stitch each part takes is the walk's own rule, `digitize::part_rule`); with
//! `plan`, also what the design's plan sews of each part.
//!
//! The Object tab reads it, and the stitch preview's planning thread answers it as it answers the
//! other read commands (`Planner::query`), with ids since a planner has no selection.

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{PartKey, PartKind, PartSettings};
use vectorcraft_doc::{AppearanceItem, Document, Node, NodeId, NodeKind};
use vectorcraft_geom::BezPath;
use vectorcraft_stitch::plan::{Params, SATIN_EDGE_UNDERLAY, SATIN_ZIGZAG_UNDERLAY, Style, TATAMI_UNDERLAY_MIN, Underlay, satin_auto_underlay};
use vectorcraft_stitch::{Command, pt_to_mm};

use super::digitize::part_rule;
use super::objects::{MAX_IDS, has_part, takers, value_json};
use super::options::stitch_id;
use super::plan::{Choice, DesignPlan, Part};
use super::resolve::report::Resolved;
use super::resolve::{DocLayer, Found, PartLayer, UnitSettings};
use super::stats::Page;
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
    Ok(with_options(reply(&doc, &ids, &ask.opts, plan.as_deref(), ask.page, warnings), &ask.opts))
}

/// The reply for objects `ids` of `doc` with the call's options `opts`, with what `plan` (the
/// design's, when planned) sews of each part.
pub(super) fn reply(doc: &Document, ids: &[NodeId], opts: &Options, plan: Option<&DesignPlan>, page: Page, warnings: Vec<String>) -> Value {
    let (take, skipped) = takers(doc, ids);
    let nodes: Vec<&Node> = take.iter().filter_map(|id| doc.node(*id)).collect();
    let layer = DocLayer::of(doc);
    let mut found = Found::default();
    let parts: Map<String, Value> = [PartKind::Fill, PartKind::Stroke]
        .into_iter()
        .map(|kind| (kind.id().to_string(), part_section(&nodes, kind, opts, &layer, plan, &mut found)))
        .collect();
    let warnings: Vec<Value> = warnings.into_iter().map(Value::from).chain(found.notes().map(Value::from)).collect();
    let mut out = json!({
        "objectCount": nodes.len(),
        "offset": page.offset,
        "truncated": page.truncated(nodes.len()),
        "objects": nodes.get(page.range(nodes.len())).unwrap_or_default().iter().map(|n| object_json(n)).collect::<Vec<_>>(),
        "skippedCount": skipped.len(),
        "skipped": skipped.iter().take(super::objects::MAX_SKIPPED).map(|(id, why)| json!({"id": id.0, "reason": why.id()})).collect::<Vec<_>>(),
        "trimAfter": trim_after(doc, ids),
        "planned": plan.is_some(),
        "warnings": warnings,
    });
    if let Value::Object(o) = &mut out {
        o.extend(parts);
    }
    out
}

/// An object as `objects` lists it: its kind, how many fills and strokes, whether its appearance is
/// sewn as pieces, its settings as kept (each part's keys by name; `null`: none), and the keys a
/// newer version wrote (`fill.grain`).
fn object_json(n: &Node) -> Value {
    let count = |kind: PartKind| n.appearance.items.iter().filter(|i| part_kind(i) == kind).count();
    let e = n.extras().embroidery.as_deref();
    let part = |kind: PartKind| e.and_then(|e| e.part(kind)).map_or(Value::Null, stored);
    let mut kept: Vec<String> = Vec::new();
    for kind in [PartKind::Fill, PartKind::Stroke] {
        if let Some(p) = e.and_then(|e| e.part(kind)) {
            kept.extend(p.extra.keys().map(|k| format!("{}.{k}", kind.id())));
            kept.extend(p.points.iter().flat_map(|pts| pts.extra.keys()).map(|k| format!("{}.points.{k}", kind.id())));
        }
    }
    json!({
        "id": n.id.0,
        "kind": if matches!(n.kind, NodeKind::Compound { .. }) { "compound" } else { "path" },
        "fills": count(PartKind::Fill),
        "strokes": count(PartKind::Stroke),
        "expanded": super::printed::needs_expanding(n),
        "settings": {"fill": part(PartKind::Fill), "stroke": part(PartKind::Stroke)},
        "kept": kept,
    })
}

/// A part's settings as kept, each set key by its name.
fn stored(p: &PartSettings) -> Value {
    PartKey::ALL.into_iter().filter_map(|k| p.get(k).map(|v| (k.name().to_string(), value_json(&v)))).collect::<Map<String, Value>>().into()
}

fn part_kind(item: &AppearanceItem) -> PartKind {
    match item {
        AppearanceItem::Fill(_) => PartKind::Fill,
        AppearanceItem::Stroke(_) => PartKind::Stroke,
    }
}

/// The cut after the objects as given (a group as one, as `embroidery.setTrimAfter` marks it; a
/// run of stitches opened from a file by its file's cut): whether they ask for it, how many do,
/// and whether they differ.
fn trim_after(doc: &Document, ids: &[NodeId]) -> Value {
    let mut seen = HashSet::new();
    let marks: Vec<bool> = ids
        .iter()
        .filter(|id| seen.insert(**id))
        .filter_map(|id| doc.node(*id))
        .map(|n| n.extras().embroidery.as_deref().is_some_and(|e| e.trim_after || e.stitches.as_ref().is_some_and(|r| r.trim_after)))
        .collect();
    let set = marks.iter().filter(|m| **m).count();
    let mixed = set > 0 && set < marks.len();
    json!({"value": if mixed { Value::Null } else { json!(set > 0) }, "setCount": set, "mixed": mixed})
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
    }
}

/// One object's part as reported: how it is sewn (`none`: not at all) and the rule that chose it,
/// every reported key, and its settings as kept.
struct PartRow<'a> {
    sewn: &'static str,
    rule: Option<&'static str>,
    keys: Vec<(&'static str, Resolved, Resolved)>,
    /// What a rule chose for a key whose value is `"auto"`, by the key's name.
    rules: Vec<(&'static str, Value)>,
    kept: Option<&'a PartSettings>,
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

/// The value the rows share, `None` when they differ.
fn common<T: PartialEq + Clone>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

/// Part `kind` of `nodes`, merged (`null` when none of them has the part): see the module's doc.
fn part_section(nodes: &[&Node], kind: PartKind, opts: &Options, layer: &DocLayer, plan: Option<&DesignPlan>, found: &mut Found) -> Value {
    let with: Vec<&Node> = nodes.iter().copied().filter(|n| has_part(n, kind)).collect();
    if with.is_empty() {
        return Value::Null;
    }
    let rows: Vec<PartRow> = with
        .iter()
        .map(|n| {
            let kept = n.extras().embroidery.as_deref().and_then(|e| e.part(kind));
            let object = kept.map_or_else(PartLayer::default, |p| PartLayer::read(p, found));
            let settings = UnitSettings::new(object, n.bbox_angle, opts, layer);
            // The part's first painted item of the kind, else its first, says how it is sewn.
            let items: Vec<&AppearanceItem> = n.appearance.items.iter().filter(|i| part_kind(i) == kind).collect();
            let item = items.iter().find(|i| i.visible()).or(items.first()).copied();
            let (sewn, rule, style) = match item {
                Some(item) if settings.sewn() => {
                    let (style, why) = part_rule(item, &path_of(n), settings.forcing());
                    (stitch_id(style), Some(rule_id(why)), style)
                }
                _ => ("none", None, Style::Fill),
            };
            // A satin's width: its stroke's.
            let width = match item {
                Some(AppearanceItem::Stroke(st)) if style == Style::Satin => pt_to_mm(st.width).max(0.0),
                _ => 0.0,
            };
            let rules = if sewn == "none" { Vec::new() } else { rules(style, &settings.params(style, width)) };
            PartRow { sewn, rule, keys: settings.report(style), rules, kept }
        })
        .collect();
    let item_count: usize = with.iter().map(|n| n.appearance.items.iter().filter(|i| part_kind(i) == kind).count()).sum();
    let mut settings = Map::new();
    for (k, (name, ..)) in rows.first().map(|r| r.keys.as_slice()).unwrap_or_default().iter().enumerate() {
        let at = |r: &PartRow| r.keys.get(k).cloned();
        let now: Vec<Resolved> = rows.iter().filter_map(|r| at(r).map(|(_, a, _)| a)).collect();
        let inherited: Vec<Resolved> = rows.iter().filter_map(|r| at(r).map(|(_, _, b)| b)).collect();
        let key = PartKey::from_name(name);
        let set_count = rows.iter().filter(|r| key.is_some_and(|key| r.kept.is_some_and(|p| p.get(key).is_some()))).count();
        let value = common(now.iter().map(|r| r.value.clone()));
        let source = common(now.iter().map(|r| r.source));
        let inherited = common(inherited.into_iter());
        settings.insert(
            name.to_string(),
            json!({
                "value": value.clone().unwrap_or(Value::Null),
                "source": source.map(|s| s.id()),
                "section": source.and_then(|s| s.section()),
                "setCount": set_count,
                "mixed": value.is_none(),
                "inherited": inherited.map(|r| json!({"value": r.value, "source": r.source.id(), "section": r.source.section()})),
                "rule": common(rows.iter().map(|r| r.rules.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))).flatten(),
            }),
        );
    }
    let point = |key: PartKey| {
        let values: Vec<Value> = rows.iter().map(|r| r.kept.and_then(|p| p.get(key)).as_ref().map_or(Value::Null, value_json)).collect();
        let set = values.iter().filter(|v| !v.is_null()).count();
        let value = common(values.into_iter());
        json!({"value": value.clone().unwrap_or(Value::Null), "setCount": set, "mixed": value.is_none()})
    };
    json!({
        "objectCount": with.len(),
        "itemCount": item_count,
        "sewn": common(rows.iter().map(|r| r.sewn)),
        "rule": common(rows.iter().map(|r| r.rule)).flatten(),
        "settings": settings,
        "entry": point(PartKey::Entry),
        "exit": point(PartKey::Exit),
        "stitches": plan.map(|p| sewn_of(p, &with, kind)),
    })
}

/// What `plan` sews of part `kind` of `nodes` (as a person selects them): its units and their
/// needle-down stitches, records, jumps and cuts.
fn sewn_of(plan: &DesignPlan, nodes: &[&Node], kind: PartKind) -> Value {
    let ids: HashSet<NodeId> = nodes.iter().map(|n| n.id).collect();
    let (mut units, mut stitches, mut records, mut jumps, mut trims) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for (i, u) in plan.units.iter().enumerate() {
        let of_kind = match (u.origin.part, kind) {
            (Part::Fill(_), PartKind::Fill) | (Part::Stroke(_), PartKind::Stroke) => true,
            (Part::Fill(_) | Part::Stroke(_) | Part::Literal, _) => false,
        };
        if !of_kind || !ids.contains(&u.origin.selected()) {
            continue;
        }
        units += 1;
        for s in plan.unit_stitches(i) {
            records += 1;
            match s.command {
                Command::Stitch => stitches += 1,
                Command::Jump => jumps += 1,
                Command::Trim => trims += 1,
                _ => {}
            }
        }
    }
    json!({"unitCount": units, "stitches": stitches, "records": records, "jumps": jumps, "trims": trims})
}
