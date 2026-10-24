//! The embroidery settings commands on objects (`docs/embroidery.md` › Settings on objects):
//! `embroidery.set` sets values of a part (all of an object's fills, or all its strokes) on the
//! objects given,
//! `embroidery.reset` unsets them, `embroidery.setEntryExit` sets where one object's part starts and
//! ends, and `embroidery.copyFrom` copies one object's values onto others.
//!
//! Every one takes the vocabulary's names alone (an earlier name `document.export` takes is refused,
//! naming the one to use), checks every value as the export's options are checked before anything
//! changes, unsets a value given as `null`, and makes one undo step, none when nothing changed. A
//! group or a layer stands for the paths and compound paths in it, a compound path's member for its
//! compound path; what takes no settings (type, a symbol instance, a run of stitches read from a
//! machine file…) is skipped and listed.

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{Embroidery, PartKey, PartKind, PartValue};
use vectorcraft_doc::{AppearanceItem, Document, MAX_COORD, Node, NodeId, NodeKind};
use vectorcraft_geom::Point;

use super::options::Others;
use super::resolve::Stitch;
use super::vocabulary::{self, Place};
use super::{Options, edit_counted, object, only};
use crate::cmd::{bad, targets};
use crate::{EngineError, Result, Session};

/// The most skipped objects a reply lists, the rest counted: a list a panel shows whole. An
/// estimate.
pub(crate) const MAX_SKIPPED: usize = 1000;

/// The most ids a settings command takes: twice the paths of the performance budget's 50,000-path
/// document, so that a list of junk can't keep a command walking the document for each. An
/// estimate.
pub(crate) const MAX_IDS: usize = 100_000;

/// The undo step `embroidery.set` makes.
pub(crate) const SET: &str = "Embroidery Object Settings";
/// The undo step `embroidery.reset` makes.
pub(crate) const RESET: &str = "Reset Embroidery Object Settings";
/// The undo step `embroidery.setEntryExit` makes.
pub(crate) const ENTRY_EXIT: &str = "Embroidery Entry and Exit";
/// The undo step `embroidery.copyFrom` makes: a paste of another object's settings.
pub(crate) const PASTE: &str = "Paste Embroidery Object Settings";

/// Why an object given takes no part settings, as the replies name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// A run of stitches read from a machine file: sewn as its points are.
    Literal,
    /// A guide: never sewn.
    Guide,
    /// Type: not sewn until lettering sews it (the project owner's choice, 2026-10-23).
    Type,
    Image,
    /// A symbol's settings are set on its art (an instance takes only the cut after it).
    SymbolInstance,
    /// A blend, an envelope, a mesh or a repeat: not sewn as it is.
    Live,
    Placed,
    Intertwine,
    /// It has no part of the kind asked for (no fill, or no stroke).
    NoPart,
}

impl Skip {
    /// Its id in replies.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Skip::Literal => "literal",
            Skip::Guide => "guide",
            Skip::Type => "type",
            Skip::Image => "image",
            Skip::SymbolInstance => "symbolInstance",
            Skip::Live => "live",
            Skip::Placed => "placed",
            Skip::Intertwine => "intertwine",
            Skip::NoPart => "noPart",
        }
    }
}

/// The objects a settings command takes, and those it skips with why.
pub(crate) type Takers = (Vec<NodeId>, Vec<(NodeId, Skip)>);

/// The objects `ids` stand for, as the settings commands take them, each once in the order met: a
/// path or a compound path itself; a compound path's member its compound path; the paths and
/// compound paths in a group or a layer, at any depth. What takes no settings is skipped, with why.
pub(crate) fn takers(d: &Document, ids: &[NodeId]) -> Takers {
    let (mut take, mut skip, mut seen) = (Vec::new(), Vec::new(), HashSet::new());
    for id in ids {
        let Some(n) = d.node(compound_of(d, *id)) else { continue };
        visit_takers(n, &mut take, &mut skip, &mut seen);
    }
    (take.into_iter().map(|n| n.id).collect(), skip)
}

/// [`takers`] of `n`, which need not be in a document (a symbol's art): the objects that take
/// settings into `take`, what is skipped into `skip`, each node once (`seen`).
pub(crate) fn visit_takers<'a>(n: &'a Node, take: &mut Vec<&'a Node>, skip: &mut Vec<(NodeId, Skip)>, seen: &mut HashSet<NodeId>) {
    if !seen.insert(n.id) {
        return;
    }
    let why = match &n.kind {
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
            for c in children {
                visit_takers(c, take, skip, seen);
            }
            return;
        }
        NodeKind::Path { guide: true, .. } => Skip::Guide,
        NodeKind::Path { .. } if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) => Skip::Literal,
        NodeKind::Path { .. } | NodeKind::Compound { .. } => {
            take.push(n);
            return;
        }
        NodeKind::Text(_) => Skip::Type,
        NodeKind::Image(_) => Skip::Image,
        NodeKind::SymbolInstance { .. } => Skip::SymbolInstance,
        NodeKind::PlacedDocument(_) => Skip::Placed,
        NodeKind::Intertwine { .. } => Skip::Intertwine,
        NodeKind::Blend { .. } | NodeKind::Envelope { .. } | NodeKind::Mesh(_) | NodeKind::Repeat(_) => Skip::Live,
    };
    skip.push((n.id, why));
}

/// The one object `id` stands for as the settings commands take it (its compound path, for a
/// member of one); refused when it takes no settings, or is a group of several.
fn one_taker(d: &Document, id: NodeId) -> Result<NodeId> {
    let given = d.node(id).ok_or(EngineError::NoNode(id))?.id;
    let id = compound_of(d, given);
    let (take, skipped) = takers(d, &[id]);
    if let Some((_, why)) = skipped.first() {
        return Err(EngineError::Other(format!("object {} takes no embroidery settings ({})", id.0, why.id())));
    }
    if take != [id] {
        return Err(EngineError::Other(format!("object {} is a group: give one of its objects", given.0)));
    }
    Ok(id)
}

/// The object `id` stands for: its compound path when it is one's member, else itself.
fn compound_of(d: &Document, id: NodeId) -> NodeId {
    d.parent_of(id).filter(|p| matches!(d.node(*p).map(|n| &n.kind), Some(NodeKind::Compound { .. }))).unwrap_or(id)
}

/// Whether `n` has a part of kind `kind`: a fill, or a stroke, in its appearance, whether it paints
/// or not (settings are kept for it either way, and take effect once it paints).
pub(crate) fn has_part(n: &Node, kind: PartKind) -> bool {
    n.appearance.items.iter().any(|i| part_kind(i) == kind)
}

/// The kind of part item `item` is of.
pub(crate) fn part_kind(item: &AppearanceItem) -> PartKind {
    match item {
        AppearanceItem::Fill(_) => PartKind::Fill,
        AppearanceItem::Stroke(_) => PartKind::Stroke,
    }
}

/// The part `v` names: `fill` or `stroke`.
fn part_param(v: &Value, cmd: &str) -> Result<PartKind> {
    v.as_str().and_then(PartKind::from_id).ok_or_else(|| bad(cmd, format!("`part` {v}: fill or stroke")))
}

/// The part keys `embroidery.set` takes, by name: every one but the points.
fn value_keys() -> impl Iterator<Item = PartKey> {
    PartKey::ALL.into_iter().filter(|k| !k.is_point())
}

/// The part key named `name`, as the settings commands take them (`points`: whether the entry and
/// exit points are among them); refused, saying what to use, for an earlier name, a design setting,
/// a point where none is taken and anything else.
fn key_param(name: &str, points: bool, cmd: &str) -> Result<PartKey> {
    if let Some(key) = PartKey::from_name(name).filter(|k| points || !k.is_point()) {
        return Ok(key);
    }
    let setting = vocabulary::SETTINGS.iter().find(|s| s.name == name);
    if PartKey::from_name(name).is_some_and(PartKey::is_point) {
        return Err(bad(cmd, format!("`{name}` is a point: set it with embroidery.setEntryExit")));
    }
    if let Some(s) = vocabulary::SETTINGS.iter().find(|s| s.earlier == Some(name)) {
        return Err(bad(cmd, format!("`{name}` is document.export's earlier name: {cmd} takes `{}`", s.name)));
    }
    if setting.is_some_and(|s| !s.places.contains(&Place::Part) && s.places.contains(&Place::Export)) {
        return Err(bad(cmd, format!("`{name}` is a design setting: set it with embroidery.setup, or give it to document.export")));
    }
    let names: Vec<&str> = PartKey::ALL.into_iter().filter(|k| points || !k.is_point()).map(PartKey::name).collect();
    Err(bad(cmd, format!("unknown setting `{name}`: the settings are {}", names.join(", "))))
}

/// `x`, with −0 as 0: a value set equal to another plans the same stitches, and is the same in the
/// cache's key.
fn canonical(x: f64) -> f64 {
    if x == 0.0 { 0.0 } else { x }
}

/// The value `v` gives part key `key` (not a point), checked as the export's option of that name
/// is (the same ranges and messages); `None` for null, which unsets it. `fill` is kept as `tatami`
/// and one number for both sides as two.
fn value_param(key: PartKey, v: &Value, cmd: &str) -> Result<Option<PartValue>> {
    if v.is_null() {
        return Ok(None);
    }
    if key == PartKey::Stitch {
        let id = v.as_str().filter(|id| Stitch::from_id(id).is_some()).ok_or_else(|| {
            let ids: Vec<&str> = Stitch::IDS.iter().map(|(id, _)| *id).collect();
            bad(cmd, format!("`stitch` {v}: one of {}", ids.join(", ")))
        })?;
        return Ok(Some(PartValue::Id(if id == "fill" { "tatami" } else { id }.to_string())));
    }
    let one = Map::from_iter([(key.name().to_string(), v.clone())]);
    let o = Options::from_object(&one, Others::Refuse(&[]), cmd)?;
    let number = |x: Option<f64>| x.map(|x| PartValue::Number(canonical(x)));
    let sides = |x: Option<[f64; 2]>| x.map(|[a, b]| PartValue::Sides([canonical(a), canonical(b)]));
    Ok(match key {
        PartKey::Spacing => number(o.spacing),
        PartKey::Length => number(o.length),
        PartKey::Angle => number(o.angle),
        PartKey::Pull => sides(o.pull),
        PartKey::PullPercent => sides(o.pull_percent),
        PartKey::Push => sides(o.push),
        PartKey::Expand => number(o.expand),
        PartKey::NarrowAsSatin => number(o.narrow_as_satin),
        PartKey::Underlay => o.underlay.map(|u| PartValue::Id(u.id().to_string())),
        PartKey::SplitLength => number(o.split_length),
        PartKey::Stitch | PartKey::Entry | PartKey::Exit => None,
    })
}

/// A part value as replies give it: an id, a number, `[left, right]`, or `[x, y]` (document
/// points).
pub(crate) fn value_json(v: &PartValue) -> Value {
    match v {
        PartValue::Id(id) => json!(id),
        PartValue::Number(x) => json!(x),
        PartValue::Sides([a, b]) => json!([a, b]),
        PartValue::Point(p) => json!([p.x, p.y]),
    }
}

/// The skipped objects as replies list them: at most [`MAX_SKIPPED`].
pub(super) fn skipped_json(skipped: &[(NodeId, Skip)]) -> Value {
    skipped.iter().take(MAX_SKIPPED).map(|(id, why)| json!({"id": id.0, "reason": why.id()})).collect()
}

/// Change the embroidery of each object `ids` as `f` does → how many changed: an object left with
/// none carries none. Only an object whose embroidery changes is copied (with the containers above
/// it): the others stay shared with the undo history, which counts what each step holds.
pub(super) fn edit_each(d: &mut Document, ids: &[NodeId], mut f: impl FnMut(&mut Embroidery)) -> Result<usize> {
    let none = Embroidery::default();
    let mut changed = 0;
    for id in ids {
        let before = d.node(*id).ok_or(EngineError::NoNode(*id))?.extras().embroidery.as_deref();
        let mut e = before.cloned().unwrap_or_default();
        f(&mut e);
        if e == *before.unwrap_or(&none) {
            continue;
        }
        let n = d.node_mut(*id).ok_or(EngineError::NoNode(*id))?;
        n.extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
        changed += 1;
    }
    Ok(changed)
}

/// The objects a settings command acts on: `ids` or the selection, each an object of the
/// document, as [`takers`] takes them; refused, naming `what`, when none is given.
pub(super) fn given(s: &Session, p: &Value, what: &str, cmd: &str) -> Result<Takers> {
    if let Some(n) = p.get("ids").and_then(Value::as_array).map(Vec::len).filter(|n| *n > MAX_IDS) {
        return Err(bad(cmd, format!("`ids` lists {n} objects: at most {MAX_IDS}")));
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(cmd, format!("select the objects {what}, or give ids")));
    }
    let d = &s.doc()?.doc;
    if let Some(missing) = ids.iter().find(|id| d.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    Ok(takers(d, &ids))
}

/// `embroidery.set`: the values given, on part `part` of the objects.
pub(super) fn set(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.set";
    let o = object(p, C)?;
    let kind = part_param(o.get("part").unwrap_or(&Value::Null), C)?;
    let mut values = Vec::new();
    for (k, v) in o.iter().filter(|(k, _)| !matches!(k.as_str(), "ids" | "part")) {
        let key = key_param(k, false, C)?;
        values.push((key, value_param(key, v, C)?));
    }
    if values.is_empty() {
        let names: Vec<&str> = value_keys().map(PartKey::name).collect();
        return Err(bad(C, format!("give a setting: {}", names.join(", "))));
    }
    let (take, mut skipped) = given(s, p, "to set", C)?;
    let d = &s.doc()?.doc;
    let (take, no_part): (Vec<NodeId>, Vec<NodeId>) = take.into_iter().partition(|id| d.node(*id).is_some_and(|n| has_part(n, kind)));
    skipped.extend(no_part.into_iter().map(|id| (id, Skip::NoPart)));
    if take.is_empty() {
        return Err(none_taken(&skipped, Some(kind)));
    }
    let changed = edit_counted(s, SET, |d| {
        edit_each(d, &take, |e| {
            e.edit_part(kind, |part| {
                for (key, value) in &values {
                    part.set(*key, value.clone());
                }
            })
        })
    })?;
    let settings: Map<String, Value> = values.iter().map(|(k, v)| (k.name().to_string(), v.as_ref().map_or(Value::Null, value_json))).collect();
    Ok(json!({
        "part": kind.id(),
        "settings": settings,
        "changedCount": changed,
        "unchangedCount": take.len().saturating_sub(changed),
        "skippedCount": skipped.len(),
        "skipped": skipped_json(&skipped),
    }))
}

/// The parts `v` names: `fill`, `stroke`, or both when left out.
fn parts_param(v: Option<&Value>, cmd: &str) -> Result<Vec<PartKind>> {
    match v {
        None | Some(Value::Null) => Ok(vec![PartKind::Fill, PartKind::Stroke]),
        Some(v) => Ok(vec![part_param(v, cmd)?]),
    }
}

/// The part named when only one is (`None`: both), as replies give it.
fn one_part(parts: &[PartKind]) -> Option<&'static str> {
    match parts {
        [one] => Some(one.id()),
        _ => None,
    }
}

/// The keys `v` names (each a part key, the points included); left out, every part value but the
/// points.
fn keys_param(v: Option<&Value>, cmd: &str) -> Result<Vec<PartKey>> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(value_keys().collect()) };
    let names = v.as_array().ok_or_else(|| bad(cmd, format!("`keys` {v}: a list of setting names")))?;
    let mut keys = Vec::new();
    for name in names {
        let name = name.as_str().ok_or_else(|| bad(cmd, format!("`keys` {v}: a list of setting names")))?;
        let key = key_param(name, true, cmd)?;
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// The refusal when none of the objects given takes settings (for part `part`, or at all): each
/// skipped one's id with why (the first [`NAMED`], then `…`), ids and reason ids that read alike in
/// every language.
pub(super) fn none_taken(skipped: &[(NodeId, Skip)], part: Option<PartKind>) -> EngineError {
    /// The skipped objects a refusal names: enough to see why, short enough to read.
    const NAMED: usize = 5;
    let mut named: Vec<String> = skipped.iter().take(NAMED).map(|(id, why)| format!("{} ({})", id.0, why.id())).collect();
    if skipped.len() > NAMED {
        named.push("…".into());
    }
    let named = named.join(", ");
    EngineError::Other(match part {
        Some(kind) => format!("none of these objects takes settings for its `{}`: {named}", kind.id()),
        None => format!("none of these objects takes embroidery settings: {named}"),
    })
}

/// `e` with its own values of `keys` unset on parts `kinds`, those `on` says a part takes. Only a
/// part kept as this version reads it: one it can't read stays as it is.
pub(super) fn unset(e: &mut Embroidery, kinds: &[PartKind], keys: &[PartKey], on: impl Fn(PartKind, PartKey) -> bool) {
    for kind in kinds {
        if e.part(*kind).is_some() {
            e.edit_part(*kind, |part| {
                for key in keys.iter().filter(|key| on(*kind, **key)) {
                    part.set(*key, None);
                }
            });
        }
    }
}

/// `embroidery.reset`: the objects' own values of `keys` unset, on `part` or both.
pub(super) fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.reset";
    let o = object(p, C)?;
    only(&o, &["ids", "part", "keys"], C)?;
    let parts = parts_param(o.get("part"), C)?;
    let keys = keys_param(o.get("keys"), C)?;
    let (take, skipped) = given(s, p, "to reset", C)?;
    if take.is_empty() {
        return Err(none_taken(&skipped, None));
    }
    let changed = edit_counted(s, RESET, |d| edit_each(d, &take, |e| unset(e, &parts, &keys, |_, _| true)))?;
    Ok(json!({
        "part": one_part(&parts),
        "keys": keys.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "changedCount": changed,
        "unchangedCount": take.len().saturating_sub(changed),
        "skippedCount": skipped.len(),
        "skipped": skipped_json(&skipped),
    }))
}

/// A point `v` gives (`what` names it): `[x, y]` in document points, each within the coordinates a
/// document holds, or null to unset it.
pub(super) fn point_param(v: &Value, what: &str, cmd: &str) -> Result<Option<Point>> {
    if v.is_null() {
        return Ok(None);
    }
    let refused = || bad(cmd, format!("`{what}` {v}: [x, y] in points, each within ±{MAX_COORD}, or null to unset it"));
    let [x, y] = v.as_array().map(Vec::as_slice).ok_or_else(refused)? else { return Err(refused()) };
    let (x, y) = (x.as_f64().ok_or_else(refused)?, y.as_f64().ok_or_else(refused)?);
    let within = |c: f64| c.is_finite() && c.abs() <= MAX_COORD;
    if !(within(x) && within(y)) {
        return Err(refused());
    }
    Ok(Some(Point::new(canonical(x), canonical(y))))
}

/// `embroidery.setEntryExit`: where one object's part starts and ends.
pub(super) fn set_entry_exit(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setEntryExit";
    let o = object(p, C)?;
    only(&o, &["id", "part", "entry", "exit"], C)?;
    let kind = part_param(o.get("part").unwrap_or(&Value::Null), C)?;
    let mut points = Vec::new();
    for (key, name) in [(PartKey::Entry, "entry"), (PartKey::Exit, "exit")] {
        if let Some(v) = o.get(name) {
            points.push((key, point_param(v, name, C)?.map(PartValue::Point)));
        }
    }
    if points.is_empty() {
        return Err(bad(C, "give entry, exit or both: [x, y] in points, or null to unset one"));
    }
    let d = &s.doc()?.doc;
    let id = match o.get("id").filter(|v| !v.is_null()) {
        Some(v) => NodeId(v.as_u64().ok_or_else(|| bad(C, format!("`id` {v}: an object id")))?),
        None => match s.doc()?.selection.objects.as_slice() {
            [one] => *one,
            _ => return Err(bad(C, "select one object, or give its id")),
        },
    };
    let id = one_taker(d, id)?;
    if !d.node(id).is_some_and(|n| has_part(n, kind)) {
        return Err(EngineError::Other(format!("object {} has no `{}`", id.0, kind.id())));
    }
    let changed = edit_counted(s, ENTRY_EXIT, |d| {
        edit_each(d, &[id], |e| {
            e.edit_part(kind, |part| {
                for (key, value) in &points {
                    part.set(*key, value.clone());
                }
            })
        })
    })?;
    let d = &s.doc()?.doc;
    let stored =
        |key: PartKey| d.node(id).and_then(|n| n.extras().embroidery.as_deref()?.part(kind)?.get(key)).as_ref().map_or(Value::Null, value_json);
    Ok(json!({"id": id.0, "part": kind.id(), "entry": stored(PartKey::Entry), "exit": stored(PartKey::Exit), "changedCount": changed}))
}

/// Value `v` of key `key` as `embroidery.set` (or, for a point, `embroidery.setEntryExit`) would
/// take it, `None` when it wouldn't: an id this version doesn't know, a value out of its range.
fn accepted(key: PartKey, v: &PartValue, cmd: &str) -> Option<PartValue> {
    let given = value_json(v);
    if key.is_point() {
        point_param(&given, key.name(), cmd).ok().flatten().map(PartValue::Point)
    } else {
        value_param(key, &given, cmd).ok().flatten()
    }
}

/// `embroidery.copyFrom`: the values of `keys` of object `source`, set or unset, onto the objects.
pub(super) fn copy_from(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.copyFrom";
    let o = object(p, C)?;
    only(&o, &["source", "ids", "part", "keys"], C)?;
    let source = match o.get("source") {
        Some(v) => NodeId(v.as_u64().ok_or_else(|| bad(C, format!("`source` {v}: an object id")))?),
        None => return Err(bad(C, "give `source`: the object whose settings are copied")),
    };
    let parts = parts_param(o.get("part"), C)?;
    let keys = keys_param(o.get("keys"), C)?;
    let d = &s.doc()?.doc;
    let source = one_taker(d, source)?;
    let from = d.node(source).and_then(|n| n.extras().embroidery.as_deref().cloned()).unwrap_or_default();
    let (take, mut skipped) = given(s, p, "to copy onto", C)?;
    let d = &s.doc()?.doc;
    // Each object takes the parts it has; one with none of them is skipped.
    let (take, no_part): (Vec<NodeId>, Vec<NodeId>) =
        take.into_iter().filter(|id| *id != source).partition(|id| d.node(*id).is_some_and(|n| parts.iter().any(|k| has_part(n, *k))));
    skipped.extend(no_part.into_iter().map(|id| (id, Skip::NoPart)));
    if take.is_empty() && skipped.is_empty() {
        return Err(bad(C, "the objects given are the source alone: give others to copy onto"));
    }
    if take.is_empty() {
        return Err(none_taken(&skipped, None));
    }
    let has: Vec<(NodeId, Vec<PartKind>)> =
        take.iter().map(|id| (*id, parts.iter().copied().filter(|k| d.node(*id).is_some_and(|n| has_part(n, *k))).collect())).collect();
    // What the source holds is copied only as `embroidery.set` would take it: a value a newer
    // version wrote, or one out of range, is passed over (the objects keep theirs) and listed.
    let mut copied: Vec<(PartKind, PartKey, Option<PartValue>)> = Vec::new();
    let mut passed_over: Vec<String> = Vec::new();
    for kind in &parts {
        for key in &keys {
            match from.part(*kind).and_then(|p| p.get(*key)) {
                None => copied.push((*kind, *key, None)),
                Some(v) => match accepted(*key, &v, C) {
                    Some(v) => copied.push((*kind, *key, Some(v))),
                    None => passed_over.push(format!("{}.{}", kind.id(), key.name())),
                },
            }
        }
    }
    let changed = edit_counted(s, PASTE, |d| {
        let mut changed = 0;
        for (id, kinds) in &has {
            changed += edit_each(d, &[*id], |e| {
                for kind in kinds {
                    e.edit_part(*kind, |part| {
                        for (_, key, value) in copied.iter().filter(|(k, ..)| k == kind) {
                            part.set(*key, value.clone());
                        }
                    });
                }
            })?;
        }
        Ok(changed)
    })?;
    Ok(json!({
        "source": source.0,
        "part": one_part(&parts),
        "keys": keys.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "passedOver": passed_over,
        "changedCount": changed,
        "unchangedCount": take.len().saturating_sub(changed),
        "skippedCount": skipped.len(),
        "skipped": skipped_json(&skipped),
    }))
}
