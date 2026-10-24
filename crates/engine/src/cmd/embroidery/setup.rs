//! The document's embroidery settings: `embroidery.setup` changes them, `embroidery.settings`
//! reads them (the thread palette, the start point, the label, the fabric, the thread's weight,
//! the joining of one colour's objects, the machine), each as stored and as the stitch plan uses
//! it, and how many objects set the fabric's values themselves.
//!
//! A fabric is copied, not linked (the project owner's choice): choosing a preset copies its values
//! into the document as the values in force and as the preset's (the base a changed value is told
//! from, and ↺ restores); a value changed afterwards changes the document alone. A fabric's tatami
//! spacing is for 40 wt thread and scales with the document's thread weight (the owner's choice,
//! 2026-10-23), so it is reported for the thread in use.

use serde_json::{Map, Value, json};
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::{FabricSettings, MachineSettings, StitchValues, ThreadPalette};
use vectorcraft_stitch::fabric::{FABRICS, fabric};
use vectorcraft_stitch::plan::{
    FABRIC_SPACING_RANGE, JUMP_RANGE, MAX_SATIN_STITCH_RANGE, MAX_TRIM, MIN_STITCH_RANGE, PULL_RANGE, Params, SPACING_PERCENT_AS_PICKED,
    SPACING_PERCENT_RANGE, STITCH_LENGTH_RANGE, Style, THREAD_WEIGHT, THREAD_WEIGHT_RANGE, Underlay, thread_scale,
};
use vectorcraft_stitch::thread::Palette;

use super::fabrics::{overrides, settings_of};
use super::stats::{FABRIC_THICKNESS, FABRIC_THICKNESS_RANGE, SPEED, SPEED_RANGE};
use super::{hex, object, thread_palette};
use crate::cmd::bad;
use crate::{Result, Session};

const C: &str = "embroidery.setup";

/// Where a resolved setting's value comes from: the document sets it, or it is the default.
fn source(set: bool) -> &'static str {
    if set { "document" } else { "default" }
}

/// The document's embroidery settings as `embroidery.setup` and `embroidery.settings` report them:
/// each as the document stores it (`settings`, which can be passed back to `embroidery.setup`), and
/// the value the stitch plan uses with where it comes from (`resolved`).
fn settings_json(d: &Document) -> Value {
    let e = d.embroidery.as_deref();
    let stored = e.and_then(|e| e.palette.as_ref());
    let known = stored.and_then(ThreadPalette::id).and_then(Palette::from_id).is_some();
    let start = e.and_then(|e| e.start).map(|p| json!([p.x, p.y]));
    let label = e.and_then(|e| e.label.clone());
    let fabric = e.and_then(|e| e.fabric.as_deref());
    let weight = e.and_then(|e| e.thread_weight);
    let join = e.and_then(|e| e.join_same_color);
    let machine = e.and_then(|e| e.machine.as_deref());
    let resolved_weight = weight.unwrap_or(THREAD_WEIGHT);
    json!({
        "settings": {
            "palette": stored.cloned().map(Value::from), "start": start, "label": label,
            "fabric": fabric.map(fabric_json), "threadWeight": weight, "joinSameColor": join, "machine": machine.map(machine_json),
        },
        "resolved": {
            "palette": {"value": thread_palette(d).id(), "source": source(known)},
            "fabric": resolved_fabric(fabric, resolved_weight),
            "threadWeight": {"value": resolved_weight, "source": source(weight.is_some())},
            "joinSameColor": {"value": join.unwrap_or(true), "source": source(join.is_some())},
            "machine": resolved_machine(machine),
        },
        "overrides": overrides(d),
    })
}

/// A kind of stitch's values as a fabric keeps them, by name (`tatami`: pull, spacing, length,
/// underlay; `satin`: pull, spacingPercent, underlay); `null` when unset.
fn stitch_json(v: Option<&StitchValues>, tatami: bool) -> Value {
    let v = v.cloned().unwrap_or_default();
    if tatami {
        json!({"pull": v.pull, "spacing": v.spacing, "length": v.length, "underlay": v.underlay})
    } else {
        json!({"pull": v.pull, "spacingPercent": v.spacing_percent, "underlay": v.underlay})
    }
}

/// The document's fabric as stored, by name: the preset it was copied from (its id), its values in
/// force, its colour.
fn fabric_json(f: &FabricSettings) -> Value {
    let v = &f.values;
    json!({
        "preset": f.preset.as_ref().map(|p| p.id.clone()),
        "tatami": stitch_json(v.tatami.as_ref(), true),
        "satin": stitch_json(v.satin.as_ref(), false),
        "stabilizer": v.stabilizer, "topping": v.topping, "needlePoint": v.needle_point, "thickness": v.thickness,
        "color": f.color.map(hex),
    })
}

/// The machine's values as stored, by name.
fn machine_json(m: &MachineSettings) -> Value {
    json!({"jumpLength": m.jump_length, "trimLength": m.trim_length, "minStitch": m.min_stitch, "splitLength": m.split_length, "speed": m.speed})
}

/// The fabric's values the plan sews with, and those for information (its thickness, stabiliser,
/// topping and needle point), each `{value, source, preset, changed}`: in force (the document's),
/// else the built-in default (none for the ids); the preset's value, for one copied from a preset;
/// and whether the two differ. A tatami spacing, kept for 40 wt, is given for the thread `weight`.
/// The colour, which is no preset's value, is `{value, source}`.
fn resolved_fabric(f: Option<&FabricSettings>, weight: f64) -> Value {
    let base = f.and_then(|f| f.preset.as_ref()).map(|p| &p.values);
    let scale = thread_scale(weight);
    let (fill, satin) = (Params::for_thread(Style::Fill, weight), Params::for_style(Style::Satin));
    let entry = |now: Option<Value>, preset: Option<Value>, default: Value| {
        let changed = base.is_some() && now != preset;
        let source = source(now.is_some());
        json!({"value": now.unwrap_or(default), "source": source, "preset": preset, "changed": changed})
    };
    let num = |x: Option<f64>| x.map(Value::from);
    let id = |x: &Option<String>| x.clone().map(Value::from);
    let stitch = |tatami: bool| {
        let now = f.and_then(|f| if tatami { f.values.tatami.clone() } else { f.values.satin.clone() }).unwrap_or_default();
        let was = base.and_then(|b| if tatami { b.tatami.clone() } else { b.satin.clone() });
        let was = |g: fn(&StitchValues) -> Option<Value>| was.as_ref().and_then(g);
        if tatami {
            json!({
                "pull": entry(num(now.pull), was(|s| s.pull.map(Value::from)), json!(fill.pull[0])),
                "spacing": entry(num(now.spacing.map(|s| s * scale)), was(|s| s.spacing.map(Value::from)).and_then(|v| v.as_f64()).map(|s| json!(s * scale)), json!(fill.density)),
                "length": entry(num(now.length), was(|s| s.length.map(Value::from)), json!(fill.stitch_length)),
                "underlay": entry(id(&now.underlay), was(|s| s.underlay.clone().map(Value::from)), json!(fill.underlay.id())),
            })
        } else {
            json!({
                "pull": entry(num(now.pull), was(|s| s.pull.map(Value::from)), json!(satin.pull[0])),
                "spacingPercent": entry(num(now.spacing_percent), was(|s| s.spacing_percent.map(Value::from)), json!(SPACING_PERCENT_AS_PICKED)),
                "underlay": entry(id(&now.underlay), was(|s| s.underlay.clone().map(Value::from)), json!(satin.underlay.id())),
            })
        }
    };
    let values = f.map(|f| &f.values);
    let color = f.and_then(|f| f.color);
    // The values for information: none by default but the thickness the thread estimate takes.
    let information = |now: Option<&Option<String>>, was: Option<&Option<String>>| entry(now.and_then(id), was.and_then(id), Value::Null);
    json!({
        "preset": f.and_then(|f| f.preset.as_ref()).map(|p| json!({"id": p.id, "known": fabric(&p.id).is_some()})),
        "tatami": stitch(true),
        "satin": stitch(false),
        "thickness": entry(num(values.and_then(|v| v.thickness)), num(base.and_then(|b| b.thickness)), json!(FABRIC_THICKNESS)),
        "stabilizer": information(values.map(|v| &v.stabilizer), base.map(|b| &b.stabilizer)),
        "topping": information(values.map(|v| &v.topping), base.map(|b| &b.topping)),
        "needlePoint": information(values.map(|v| &v.needle_point), base.map(|b| &b.needle_point)),
        "color": {"value": hex(color.unwrap_or(super::scene::FABRIC)), "source": source(color.is_some())},
    })
}

/// The machine's values the plan sews with, each `{value, source}`: the document's, else the
/// built-in default.
fn resolved_machine(m: Option<&MachineSettings>) -> Value {
    let d = Params::for_style(Style::Running);
    let m = m.cloned().unwrap_or_default();
    let entry = |v: Option<f64>, default: f64| json!({"value": v.unwrap_or(default), "source": source(v.is_some())});
    json!({
        "jumpLength": entry(m.jump_length, d.jump),
        "trimLength": entry(m.trim_length, d.trim),
        "minStitch": entry(m.min_stitch, d.min_stitch),
        "splitLength": entry(m.split_length, d.max_satin),
        "speed": entry(m.speed, SPEED),
    })
}

pub(super) fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(settings_json(&s.doc()?.doc))
}

pub(super) fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    let o = object(p, C)?;
    let d = &s.doc()?.doc;
    let mut new = d.embroidery.as_deref().cloned().unwrap_or_default();
    // The settings given, at the top or under `settings` (the reply's, passed back), each once.
    let mut given: Map<String, Value> = Map::new();
    for (k, v) in o.iter() {
        let more = match (k.as_str(), v) {
            ("settings", Value::Object(inner)) => inner.clone(),
            ("settings", _) => return Err(bad(C, format!("`settings` {v}: an object of settings, as the reply gives it"))),
            ("resolved", _) => return Err(bad(C, "`resolved` is read only: pass back `settings`")),
            ("overrides", _) => return Err(bad(C, "`overrides` is read only: pass back `settings`")),
            _ => Map::from_iter([(k.clone(), v.clone())]),
        };
        for (k, v) in more {
            if given.get(&k).is_some_and(|w| *w != v) {
                return Err(bad(C, format!("`{k}` given twice, with different values")));
            }
            given.insert(k, v);
        }
    }
    // What equals the document's own settings as the reply reports them changes nothing, and isn't
    // checked again: a value a newer version wrote (or one out of this version's range) passes
    // back, and a null where nothing is set keeps it unset (it doesn't restore a preset's value).
    let stored = settings_json(d).get("settings").cloned().unwrap_or_default();
    let given = unchanged_left_out(given, &stored);
    for (k, v) in &given {
        match k.as_str() {
            "palette" => new.set_palette(palette_param(v, new.palette.as_ref())?),
            "start" => new.set_start(start_param(v)?),
            "label" => new.set_label(label_param(v)?),
            "fabric" => new.set_fabric(fabric_param(v, new.fabric.as_deref())?),
            "threadWeight" => new.set_thread_weight(number(v, "threadWeight", THREAD_WEIGHT_RANGE, "wt")?),
            "joinSameColor" => new.set_join_same_color(match v {
                Value::Null => None,
                Value::Bool(on) => Some(*on),
                _ => return Err(bad(C, format!("`joinSameColor` {v}: true, false or null"))),
            }),
            "machine" => new.set_machine(machine_param(v, new.machine.as_deref())?),
            _ => {
                return Err(bad(
                    C,
                    format!("unknown setting `{k}`: the settings are palette, start, label, fabric, threadWeight, joinSameColor and machine"),
                ));
            }
        }
    }
    let new = (!new.is_empty()).then(|| Box::new(new));
    // A file may hold settings with nothing in them (`"embroidery": {}`): as good as none, so
    // asking for the settings doesn't edit the document.
    if new.as_deref() == d.embroidery.as_deref().filter(|e| !e.is_empty()) {
        return Ok(settings_json(d));
    }
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = new;
        Ok(())
    })?;
    Ok(settings_json(&s.doc()?.doc))
}

/// `given` less what equals `stored` (the reply's `settings`, as passed back), key by key at any
/// depth: an object left with nothing is left out too.
fn unchanged_left_out(given: Map<String, Value>, stored: &Value) -> Map<String, Value> {
    given
        .into_iter()
        .filter_map(|(k, v)| match (v, stored.get(&k)) {
            (Value::Object(o), Some(s @ Value::Object(_))) => {
                let left = unchanged_left_out(o, s);
                (!left.is_empty()).then_some((k, Value::Object(left)))
            }
            (v, Some(s)) if v == *s => None,
            (v, _) => Some((k, v)),
        })
        .collect()
}

/// The number `v` gives `key`, within `range` (in `unit`), or null (`None`).
fn number(v: &Value, key: &str, (lo, hi): (f64, f64), unit: &str) -> Result<Option<f64>> {
    if v.is_null() {
        return Ok(None);
    }
    v.as_f64()
        .filter(|x| x.is_finite() && (lo..=hi).contains(x))
        .map(Some)
        .ok_or_else(|| bad(C, format!("`{key}` {v}: a number of {unit} from {lo} to {hi}, or null")))
}

/// The most characters a fabric's stabiliser, topping or needle point may have (ids for
/// information, kept as written). An estimate.
const MAX_ID: usize = 100;

/// The fabric `v` gives over `current`: a preset's id (its values copied in, the colour kept; the
/// same preset again restores its values), an object of values merged key by key (`null` restores
/// a value to the preset's, or unsets it without a preset; `preset` as stored is passed back, and
/// another preset's id is copied first), or null for none.
fn fabric_param(v: &Value, current: Option<&FabricSettings>) -> Result<Option<FabricSettings>> {
    let color = current.and_then(|f| f.color);
    let choose = |id: &str| -> Result<FabricSettings> {
        if let Some(f) = fabric(id) {
            return Ok(settings_of(f, color));
        }
        // A preset this version doesn't know (a newer version's), passed back: kept as it is.
        if let Some(cur) = current.filter(|c| c.preset.as_ref().is_some_and(|p| p.id == id)) {
            return Ok(cur.clone());
        }
        let ids: Vec<&str> = FABRICS.iter().map(|f| f.id).collect();
        Err(bad(C, format!("fabric {id:?}: one of {} (or null for none)", ids.join(", "))))
    };
    let o = match v {
        Value::Null => return Ok(None),
        Value::String(id) => return choose(id).map(Some),
        Value::Object(o) => o,
        _ => return Err(bad(C, format!("fabric {v}: a preset's id, an object of values, or null"))),
    };
    let mut f = match o.get("preset") {
        None => current.cloned().unwrap_or_default(),
        // A fabric of one's own: the values stay, with no preset to tell changes from.
        Some(Value::Null) => FabricSettings { preset: None, ..current.cloned().unwrap_or_default() },
        Some(Value::String(id)) if current.and_then(|c| c.preset.as_ref()).is_some_and(|p| p.id == *id) => current.cloned().unwrap_or_default(),
        Some(Value::String(id)) => choose(id)?,
        Some(other) => return Err(bad(C, format!("fabric's `preset` {other}: a preset's id, or null"))),
    };
    let base = f.preset.as_ref().map(|p| p.values.clone()).unwrap_or_default();
    for (k, v) in o {
        match k.as_str() {
            "preset" => {}
            "tatami" => f.values.tatami = stitch_param(v, base.tatami.as_ref(), f.values.tatami.take(), true)?,
            "satin" => f.values.satin = stitch_param(v, base.satin.as_ref(), f.values.satin.take(), false)?,
            "stabilizer" | "topping" | "needlePoint" => {
                let value = match v {
                    Value::Null => None,
                    Value::String(s) if s.chars().count() <= MAX_ID => Some(s.clone()),
                    _ => return Err(bad(C, format!("fabric's `{k}` {v}: an id of at most {MAX_ID} characters, or null"))),
                };
                let (slot, was) = match k.as_str() {
                    "stabilizer" => (&mut f.values.stabilizer, &base.stabilizer),
                    "topping" => (&mut f.values.topping, &base.topping),
                    _ => (&mut f.values.needle_point, &base.needle_point),
                };
                *slot = value.or_else(|| was.clone());
            }
            "thickness" => f.values.thickness = number(v, "thickness", FABRIC_THICKNESS_RANGE, "millimetres")?.or(base.thickness),
            "color" => f.color = color_param(v)?,
            _ => {
                return Err(bad(
                    C,
                    format!("fabric's `{k}`: the fabric's values are preset, tatami, satin, stabilizer, topping, needlePoint, thickness and color"),
                ));
            }
        }
    }
    Ok(Some(f))
}

/// A kind of stitch's values `v` gives (`tatami`: pull, spacing, length, underlay; `satin`: pull,
/// spacingPercent, underlay) over `current`, each checked; `null` restores one to the preset's
/// (`base`), or unsets it without a preset; `v` null restores them all.
fn stitch_param(v: &Value, base: Option<&StitchValues>, current: Option<StitchValues>, tatami: bool) -> Result<Option<StitchValues>> {
    let kind = if tatami { "tatami" } else { "satin" };
    let base = base.cloned().unwrap_or_default();
    let o = match v {
        Value::Null => return Ok(Some(base).filter(|b| !b.is_empty())),
        Value::Object(o) => o,
        _ => return Err(bad(C, format!("fabric's `{kind}` {v}: an object of values, or null"))),
    };
    let mut s = current.unwrap_or_default();
    for (k, v) in o {
        let key = format!("{kind}.{k}");
        match (k.as_str(), tatami) {
            ("pull", _) => s.pull = number(v, &key, PULL_RANGE, "millimetres")?.or(base.pull),
            ("spacing", true) => s.spacing = number(v, &key, FABRIC_SPACING_RANGE, "millimetres for 40 wt thread")?.or(base.spacing),
            ("length", true) => s.length = number(v, &key, STITCH_LENGTH_RANGE, "millimetres")?.or(base.length),
            ("spacingPercent", false) => s.spacing_percent = number(v, &key, SPACING_PERCENT_RANGE, "percent")?.or(base.spacing_percent),
            ("underlay", _) => {
                s.underlay = match v {
                    Value::Null => base.underlay.clone(),
                    _ => Some(v.as_str().and_then(Underlay::from_id).map(|u| u.id().to_string()).ok_or_else(|| {
                        let ids: Vec<&str> = Underlay::ALL.iter().map(|u| u.id()).collect();
                        bad(C, format!("`{key}` {v}: one of {}, or null", ids.join(", ")))
                    })?),
                }
            }
            _ => {
                let keys = if tatami { "pull, spacing, length and underlay" } else { "pull, spacingPercent and underlay" };
                return Err(bad(C, format!("fabric's `{key}`: {kind} takes {keys}")));
            }
        }
    }
    Ok(Some(s).filter(|s| !s.is_empty()))
}

/// A colour `v` gives: exactly `#rrggbb`, or null to unset it.
fn color_param(v: &Value) -> Result<Option<[u8; 3]>> {
    if v.is_null() {
        return Ok(None);
    }
    v.as_str()
        .filter(|h| h.len() == 7 && h.starts_with('#') && h.bytes().skip(1).all(|b| b.is_ascii_hexdigit()))
        .and_then(vectorcraft_color::Color::from_hex)
        .map(|c| {
            let [r, g, b, _] = c.to_rgba8(1.0);
            Some([r, g, b])
        })
        .ok_or_else(|| bad(C, format!("fabric's `color` {v}: \"#rrggbb\", or null")))
}

/// The machine's values `v` gives over `current`, each checked and merged key by key (`null`
/// unsets one), or null for none; a trim length is at least the jump length, as an export's.
fn machine_param(v: &Value, current: Option<&MachineSettings>) -> Result<Option<MachineSettings>> {
    let o = match v {
        Value::Null => return Ok(None),
        Value::Object(o) => o,
        _ => return Err(bad(C, format!("machine {v}: an object of values, or null"))),
    };
    let mut m = current.cloned().unwrap_or_default();
    for (k, v) in o {
        match k.as_str() {
            "jumpLength" => m.jump_length = number(v, "machine.jumpLength", JUMP_RANGE, "millimetres")?,
            "trimLength" => m.trim_length = number(v, "machine.trimLength", (JUMP_RANGE.0, MAX_TRIM), "millimetres")?,
            "minStitch" => m.min_stitch = number(v, "machine.minStitch", MIN_STITCH_RANGE, "millimetres")?,
            "splitLength" => m.split_length = number(v, "machine.splitLength", MAX_SATIN_STITCH_RANGE, "millimetres")?,
            "speed" => m.speed = number(v, "machine.speed", SPEED_RANGE, "stitches a minute")?,
            _ => return Err(bad(C, format!("machine's `{k}`: the machine's values are jumpLength, trimLength, minStitch, splitLength and speed"))),
        }
    }
    let jump = m.jump_length.unwrap_or(Params::for_style(Style::Running).jump);
    if let Some(trim) = m.trim_length.filter(|t| *t < jump) {
        return Err(bad(C, format!("`trimLength` {trim}: at least `jumpLength` ({jump} mm), since only a jump is cut")));
    }
    Ok(Some(m))
}

/// The palette `v` names: `pec`, `jef`, or null to unset it. The palette already set stays as it
/// is when it is passed back, even one a newer version wrote.
fn palette_param(v: &Value, current: Option<&ThreadPalette>) -> Result<Option<ThreadPalette>> {
    if v.is_null() {
        return Ok(None);
    }
    let p = ThreadPalette::from(v.clone());
    if p.id().is_none() && current != Some(&p) {
        let known: Vec<&str> = ThreadPalette::KNOWN.iter().filter_map(ThreadPalette::id).collect();
        return Err(bad(C, format!("palette {v}: one of {} (or null to unset it)", known.join(", "))));
    }
    Ok(Some(p))
}

/// The start point `v` gives: `[x, y]` in document points, within the coordinates a document
/// holds, or null to unset it.
fn start_param(v: &Value) -> Result<Option<vectorcraft_geom::Point>> {
    if v.is_null() {
        return Ok(None);
    }
    let refused =
        || bad("embroidery.setup", format!("start {v}: [x, y] in points, each within ±{}, or null to unset it", vectorcraft_doc::MAX_COORD));
    let [x, y] = v.as_array().map(Vec::as_slice).ok_or_else(refused)? else { return Err(refused()) };
    let (x, y) = (x.as_f64().ok_or_else(refused)?, y.as_f64().ok_or_else(refused)?);
    let within = |c: f64| c.is_finite() && c.abs() <= vectorcraft_doc::MAX_COORD;
    if !(within(x) && within(y)) {
        return Err(refused());
    }
    Ok(Some(vectorcraft_geom::Point::new(x, y)))
}

/// The most characters a design's name set by `embroidery.setup` may have: far more than any
/// format writes (DST and PEC labels hold 16), a bound on what a document keeps.
const MAX_LABEL: usize = 1000;

/// The design's name `v` gives: a string of at most [`MAX_LABEL`] characters, or null to unset it.
fn label_param(v: &Value) -> Result<Option<String>> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.chars().count() <= MAX_LABEL => Ok(Some(s.clone())),
        _ => Err(bad("embroidery.setup", format!("label {v}: a name of at most {MAX_LABEL} characters, or null to unset it"))),
    }
}
