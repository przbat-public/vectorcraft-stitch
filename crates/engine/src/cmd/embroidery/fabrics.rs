//! The fabric presets as the commands give them (`vectorcraft_stitch::fabric`): `embroidery.fabrics`
//! lists them with what each value rests on, `embroidery.setup {fabric}` copies one into the
//! document ([`settings_of`]), and `embroidery.applyFabric` unsets the objects' own values of the
//! keys a fabric sets, so that they follow the document's fabric again.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::{Embroidery, FabricPreset, FabricSettings, FabricValues, PartKey, PartKind, StitchValues};
use vectorcraft_doc::{Document, Node, NodeId, NodeKind};
use vectorcraft_stitch::fabric::{Basis, DEFAULT_FABRIC, FABRICS, Fabric, ONE_FAMILY, Sourced, StitchPreset};

use super::objects::{edit_each, given, skipped_json, takers, visit_takers};
use super::setup::Overrides;
use super::{edit_counted, object, only};
use crate::cmd::bad;
use crate::{Result, Session};

/// The undo step `embroidery.applyFabric` makes.
pub(crate) const APPLY: &str = "Apply Embroidery Fabric to Objects";

/// The part keys a fabric sets, which `embroidery.applyFabric` unsets on objects by default: on a
/// part, those [`fabric_keys`] gives it.
pub(crate) const FABRIC_KEYS: [PartKey; 4] = [PartKey::Pull, PartKey::Spacing, PartKey::Length, PartKey::Underlay];

/// The keys a fabric gives part `kind`: a fill is sewn as tatami (the fabric's pull, rows' spacing,
/// stitch length and underlay); a stroke as a satin column or a running line, which take its satin
/// pull, spacing and underlay (a running line uses none of them) but no stitch length: no fabric
/// gives a running line's or a satin's.
pub(crate) fn fabric_keys(kind: PartKind) -> &'static [PartKey] {
    match kind {
        PartKind::Fill => &FABRIC_KEYS,
        PartKind::Stroke => &[PartKey::Pull, PartKey::Spacing, PartKey::Underlay],
    }
}

/// A preset's values for one kind of stitch, as a document keeps them.
fn stitch_values(p: &StitchPreset) -> StitchValues {
    StitchValues {
        pull: p.pull.map(|v| v.value),
        spacing: p.spacing.map(|v| v.value),
        spacing_percent: p.spacing_percent.map(|v| v.value),
        length: p.length.map(|v| v.value),
        underlay: p.underlay.map(|v| v.value.id().to_string()),
        ..StitchValues::default()
    }
}

/// Preset `f`'s values, as a document keeps them.
pub(crate) fn values_of(f: &Fabric) -> FabricValues {
    FabricValues {
        tatami: Some(stitch_values(&f.tatami)),
        satin: Some(stitch_values(&f.satin)),
        stabilizer: f.stabilizer.map(|v| v.value.to_string()),
        topping: f.topping.map(|v| v.value.to_string()),
        needle_point: f.needle_point.map(|v| v.value.to_string()),
        thickness: f.thickness.map(|v| v.value),
        ..FabricValues::default()
    }
}

/// The document's fabric once preset `f` is chosen: its values in force and as the preset's, the
/// colour `color` kept (a colour is no preset's value).
pub(crate) fn settings_of(f: &Fabric, color: Option<[u8; 3]>) -> FabricSettings {
    let values = values_of(f);
    FabricSettings {
        preset: Some(FabricPreset { id: f.id.to_string(), values: values.clone(), ..FabricPreset::default() }),
        values,
        color,
        ..FabricSettings::default()
    }
}

/// What a value rests on, as `embroidery.fabrics` says it.
fn basis_json(key: &str, b: &Basis) -> Value {
    match b {
        Basis::Published(p) => json!({"key": key, "kind": "published", "name": p.name, "url": p.url, "family": p.family.id(), "how": null}),
        Basis::PublishedFor(p, row) => json!({"key": key, "kind": "published", "name": p.name, "url": p.url, "family": p.family.id(), "how": row}),
        Basis::Derived(p, how) => json!({"key": key, "kind": "derived", "name": p.name, "url": p.url, "family": p.family.id(), "how": how}),
        Basis::Estimate(why) => json!({"key": key, "kind": "estimate", "name": null, "url": null, "family": null, "how": why}),
    }
}

/// One preset as `embroidery.fabrics` lists it.
fn fabric_json(f: &Fabric) -> Value {
    let (mut sources, mut estimates) = (Vec::new(), Vec::new());
    let mut note = |key: String, basis: &[Basis]| {
        if matches!(basis.first(), Some(Basis::Estimate(_))) {
            estimates.push(Value::from(key.clone()));
        }
        sources.extend(basis.iter().map(|b| basis_json(&key, b)));
    };
    let mut num = |key: String, v: Option<Sourced<f64>>| {
        v.map_or(Value::Null, |v| {
            note(key, v.basis);
            json!(v.value)
        })
    };
    // Each kind's own keys, as `embroidery.setup {fabric}` takes them back (tatami: pull, spacing,
    // length, underlay; satin: pull, spacingPercent, underlay).
    let stitch = |kind: &str, p: &StitchPreset, num: &mut dyn FnMut(String, Option<Sourced<f64>>) -> Value| {
        let mut o = Map::new();
        o.insert("pull".into(), num(format!("{kind}.pull"), p.pull));
        if kind == "tatami" {
            o.insert("spacing".into(), num(format!("{kind}.spacing"), p.spacing));
            o.insert("length".into(), num(format!("{kind}.length"), p.length));
        } else {
            o.insert("spacingPercent".into(), num(format!("{kind}.spacingPercent"), p.spacing_percent));
        }
        o.insert("underlay".into(), p.underlay.map_or(Value::Null, |u| json!(u.value.id())));
        Value::Object(o)
    };
    let tatami = stitch("tatami", &f.tatami, &mut num);
    let satin = stitch("satin", &f.satin, &mut num);
    let thickness = num("thickness".into(), f.thickness);
    // The ids and the underlays' sources, after the numbers'.
    let mut text = |key: &str, v: Option<Sourced<&'static str>>| {
        v.map_or(Value::Null, |v| {
            note(key.to_string(), v.basis);
            json!(v.value)
        })
    };
    let (stabilizer, topping, needle_point) = (text("stabilizer", f.stabilizer), text("topping", f.topping), text("needlePoint", f.needle_point));
    for (kind, p) in [("tatami", &f.tatami), ("satin", &f.satin)] {
        if let Some(u) = p.underlay {
            note(format!("{kind}.underlay"), u.basis);
        }
    }
    json!({
        "id": f.id,
        "label": f.label,
        "values": {"tatami": tatami, "satin": satin, "stabilizer": stabilizer, "topping": topping, "needlePoint": needle_point, "thickness": thickness},
        "sources": sources,
        "estimates": estimates,
    })
}

/// `embroidery.fabrics`: the presets, each value with what it rests on.
pub(super) fn fabrics(_: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.fabrics";
    only(&*object(p, C)?, &[], C)?;
    Ok(json!({"default": DEFAULT_FABRIC, "caveat": ONE_FAMILY, "fabrics": FABRICS.iter().map(fabric_json).collect::<Vec<_>>()}))
}

/// Every object of `doc` that takes settings: the paths and compound paths of its layers.
fn every_object(doc: &Document) -> Vec<NodeId> {
    let layers: Vec<NodeId> = doc.layers.iter().map(|l| l.id).collect();
    takers(doc, &layers).0
}

/// The objects of `doc`'s symbols' art that take settings: what the symbols' instances sew.
fn symbol_objects(doc: &Document) -> Vec<&Node> {
    let (mut take, mut seen) = (Vec::new(), HashSet::new());
    for sym in &doc.symbols {
        visit_takers(&sym.art, &mut take, &mut Vec::new(), &mut seen);
    }
    take
}

/// `e` with its own values of `keys` unset, on each part those a fabric gives it.
fn unset_fabric_keys(e: &mut Embroidery, keys: &[PartKey]) {
    super::objects::unset(e, &PartKind::ALL, keys, |kind, key| fabric_keys(kind).contains(&key));
}

/// [`unset_fabric_keys`] on every object of symbol art `n` that takes settings, copying only what
/// changes → how many objects changed.
fn unset_in_art(n: &mut Arc<Node>, keys: &[PartKey]) -> usize {
    /// The settings object `n` would have, when it takes settings and they change.
    fn changed(n: &Node, keys: &[PartKey]) -> Option<Embroidery> {
        let mut take = Vec::new();
        visit_takers(n, &mut take, &mut Vec::new(), &mut HashSet::new());
        let before = n.extras().embroidery.as_deref().filter(|_| take.first().is_some_and(|t| t.id == n.id))?;
        let mut e = before.clone();
        unset_fabric_keys(&mut e, keys);
        (e != *before).then_some(e)
    }
    /// Whether any object in `n` changes.
    fn any(n: &Node, keys: &[PartKey]) -> bool {
        changed(n, keys).is_some()
            || n.children().is_some_and(|c| matches!(n.kind, NodeKind::Group { .. } | NodeKind::Layer { .. }) && c.iter().any(|c| any(c, keys)))
    }
    match &n.kind {
        NodeKind::Path { .. } | NodeKind::Compound { .. } => match changed(n, keys) {
            Some(e) => {
                Arc::make_mut(n).extras_mut().embroidery = (!e.is_empty()).then(|| Box::new(e));
                1
            }
            None => 0,
        },
        NodeKind::Group { .. } | NodeKind::Layer { .. } if any(n, keys) => {
            Arc::make_mut(n).children_mut().into_iter().flatten().map(|c| unset_in_art(c, keys)).sum()
        }
        _ => 0,
    }
}

/// `embroidery.applyFabric`: the objects' own values of the fabric's keys unset, on each part those
/// the fabric gives it ([`fabric_keys`]), on the objects given, else the selected ones, else every
/// object of the document, its symbols' art included; with `all`, on every object whatever is
/// selected (what Document Setup's bar counts, [`overrides`]).
pub(super) fn apply_fabric(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.applyFabric";
    let o = object(p, C)?;
    only(&o, &["ids", "keys", "all"], C)?;
    let all = match o.get("all") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("`all` {v}: true or false")))?,
    };
    let ids_given = o.get("ids").is_some_and(|v| !v.is_null());
    if all && ids_given {
        return Err(bad(C, "give `ids` or `all`, not both"));
    }
    let keys = match o.get("keys").filter(|v| !v.is_null()) {
        None => FABRIC_KEYS.to_vec(),
        Some(v) => {
            let names = v.as_array().ok_or_else(|| bad(C, format!("`keys` {v}: a list of setting names")))?;
            let mut keys = Vec::new();
            for name in names {
                let key = name.as_str().and_then(PartKey::from_name).filter(|k| FABRIC_KEYS.contains(k)).ok_or_else(|| {
                    let ids: Vec<&str> = FABRIC_KEYS.iter().map(|k| k.name()).collect();
                    bad(C, format!("`keys` {name}: a fabric's settings are {}; others are unset with embroidery.reset", ids.join(", ")))
                })?;
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            keys
        }
    };
    // The objects given, else the selection (as every command's ids stand in for it), else every
    // object of the document; with `all`, every object whatever is selected (Document Setup's bar).
    let chosen = !all && (ids_given || !s.doc()?.selection.objects.is_empty());
    let (take, skipped) = if chosen { given(s, p, "to apply the fabric to", C)? } else { (every_object(&s.doc()?.doc), Vec::new()) };
    if take.is_empty() && !skipped.is_empty() {
        return Err(super::objects::none_taken(&skipped, None));
    }
    // The whole document's objects include its symbols' art, which its instances sew.
    let in_symbols = if chosen { 0 } else { symbol_objects(&s.doc()?.doc).len() };
    let changed = edit_counted(s, APPLY, |d| {
        let mut changed = edit_each(d, &take, |e| unset_fabric_keys(e, &keys))?;
        if !chosen {
            for sym in &mut d.symbols {
                changed += unset_in_art(&mut sym.art, &keys);
            }
        }
        Ok(changed)
    })?;
    Ok(json!({
        "keys": keys.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "changedCount": changed,
        "unchangedCount": (take.len() + in_symbols).saturating_sub(changed),
        "skippedCount": skipped.len(),
        "skipped": skipped_json(&skipped),
    }))
}

/// How many objects of `doc` (its symbols' art included) set each fabric key themselves, on a part
/// the fabric gives it ([`fabric_keys`]), and how many set any (`objectCount`): what Document
/// Setup's bar counts (`embroidery.setup`'s and `embroidery.settings`' `overrides`).
pub(crate) fn overrides(doc: &Document) -> Overrides {
    let ids = every_object(doc);
    let objects: Vec<&Node> = ids.iter().filter_map(|id| doc.node(*id)).chain(symbol_objects(doc)).collect();
    let sets = |n: &Node, key: PartKey| {
        n.extras().embroidery.as_deref().is_some_and(|e| {
            [PartKind::Fill, PartKind::Stroke].into_iter().any(|k| fabric_keys(k).contains(&key) && e.part(k).is_some_and(|p| p.get(key).is_some()))
        })
    };
    let count = |key: PartKey| objects.iter().filter(|n| sets(n, key)).count();
    Overrides {
        pull: count(PartKey::Pull),
        spacing: count(PartKey::Spacing),
        length: count(PartKey::Length),
        underlay: count(PartKey::Underlay),
        object_count: objects.iter().filter(|n| FABRIC_KEYS.iter().any(|k| sets(n, *k))).count(),
    }
}
