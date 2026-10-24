//! The document's embroidery settings: `embroidery.setup` changes them, `embroidery.settings`
//! reads them (the thread palette, the start point, the label), each as stored and as the stitch
//! plan uses it.

use serde_json::{Map, Value, json};
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::ThreadPalette;
use vectorcraft_stitch::thread::Palette;

use super::{object, thread_palette};
use crate::cmd::bad;
use crate::{Result, Session};

/// Where a resolved setting's value comes from: the document sets it, or it is the default.
fn source(set: bool) -> &'static str {
    if set { "document" } else { "default" }
}

/// The document's embroidery settings as `embroidery.setup` and `embroidery.settings` report them:
/// each as the document stores it (`settings`, which can be passed back to `embroidery.setup`), and
/// the value the stitch plan uses with where it comes from (`resolved`).
fn settings_json(d: &Document) -> Value {
    let stored = d.embroidery.as_deref().and_then(|e| e.palette.as_ref());
    let known = stored.and_then(ThreadPalette::id).and_then(Palette::from_id).is_some();
    let start = d.embroidery.as_deref().and_then(|e| e.start).map(|p| json!([p.x, p.y]));
    let label = d.embroidery.as_deref().and_then(|e| e.label.clone());
    json!({
        "settings": {"palette": stored.cloned().map(Value::from), "start": start, "label": label},
        "resolved": {"palette": {"value": thread_palette(d).id(), "source": source(known)}},
    })
}

pub(super) fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(settings_json(&s.doc()?.doc))
}

pub(super) fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setup";
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
            _ => Map::from_iter([(k.clone(), v.clone())]),
        };
        for (k, v) in more {
            if given.get(&k).is_some_and(|w| *w != v) {
                return Err(bad(C, format!("`{k}` given twice, with different values")));
            }
            given.insert(k, v);
        }
    }
    for (k, v) in &given {
        match k.as_str() {
            "palette" => new.set_palette(palette_param(v, new.palette.as_ref())?),
            "start" => new.set_start(start_param(v)?),
            "label" => new.set_label(label_param(v)?),
            _ => return Err(bad(C, format!("unknown setting `{k}`: the settings are palette, start and label"))),
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

/// The palette `v` names: `pec`, `jef`, or null to unset it. The palette already set stays as it
/// is when it is passed back, even one a newer version wrote.
fn palette_param(v: &Value, current: Option<&ThreadPalette>) -> Result<Option<ThreadPalette>> {
    if v.is_null() {
        return Ok(None);
    }
    let p = ThreadPalette::from(v.clone());
    if p.id().is_none() && current != Some(&p) {
        let known: Vec<&str> = ThreadPalette::KNOWN.iter().filter_map(ThreadPalette::id).collect();
        return Err(bad("embroidery.setup", format!("palette {v}: one of {} (or null to unset it)", known.join(", "))));
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
