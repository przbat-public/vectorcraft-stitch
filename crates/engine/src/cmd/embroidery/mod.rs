//! Machine embroidery in the document: the settings an object carries for it and the document's
//! own (`embroidery.setup`: the thread palette), the walk that plans the document's art as
//! stitches, and the threads the plan sews it in (`embroidery.threads`). The machine files
//! themselves are read and written in `cmd::fileio::embroidery` (`document.open`,
//! `document.export`).
//!
//! The parts: [`options`] (the export's options, and the stitch planner's parameters they give an
//! object), [`digitize`] (the document's art planned as a design: its [`plan::DesignPlan`]),
//! [`plan`] (the plan: sewing units, thread blocks, stitch numbers) and [`sequence`] (the order
//! the blocks are sewn in).

pub(crate) mod digitize;
pub(crate) mod options;
pub mod plan;
mod sequence;
#[cfg(test)]
mod tests;

use serde_json::{Map, Value, json};
use vectorcraft_color::Color;
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::{Embroidery, ThreadPalette};
use vectorcraft_stitch::thread::{MISMATCH_DELTA_E, Palette};

use super::*;

pub(crate) use options::Options;

/// The embroidery settings an object carries, the document's, and the threads its design is sewn
/// in.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "embroidery.setTrimAfter",
            "Trim After",
            [],
            None,
            "{on: bool, ids?|id?} whether the thread is cut after each object (or group) in ids or the selection when the document is stitched (document.export to dst, exp, pes or jef), however near the next object starts; off, only a jump longer than the export's trimLength is cut. One undo step; saved with the document → {changed}",
            has_doc,
            set_trim_after
        ),
        cmd!(
            "embroidery.setup",
            "Embroidery Setup",
            [],
            None,
            "{palette?: \"pec\"|\"jef\"|null (the thread palette the design's colours are matched to, the target machine format's: Brother's 64 threads, which PES files number, or Janome's 78, which JEF files number; null unsets it, and JEF is used), threadPalette?: (reported only: ignored when passed back)} change the document's embroidery settings in one undo step, saved with the document; no params → the current settings → {palette: as set (null when unset; one a newer version wrote, as it is), threadPalette: \"pec\"|\"jef\" (the palette the stitch plan matches to: palette, else JEF)}",
            has_doc,
            setup
        ),
        cmd!(
            query "embroidery.threads",
            "Embroidery Threads",
            [],
            None,
            "{…the embroidery options of document.export (document.formats lists them; they decide which objects share a colour block)} the thread each colour block of the design is sewn in, as the PES or JEF export of the document's palette writes it (embroidery.setup): the palette's thread nearest the colour the block's art is painted with, in sewing order, or, when a neighbouring block has that one, the nearest other thread within threshold → {palette: \"pec\"|\"jef\", threshold: the colour difference past which a thread is flagged, blocks: [{block: from 1 in sewing order, art: \"#rrggbb\", thread: {palette, index: from 1 as the format numbers it, name, color: \"#rrggbb\"}, deltaE: the CIEDE2000 difference between the art's colour and the thread's, to two decimals (0: painted with the thread's own colour, a swatch of its thread library), mismatch: deltaE past threshold, units: the fills, strokes and stitch runs sewn in it}], warnings}",
            has_doc,
            threads
        ),
    ]
}

fn set_trim_after(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setTrimAfter";
    let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad(C, "`on` must be true or false"))?;
    let ids = targets(s, p)?;
    let changed = super::overprint::edit_counted(s, "Trim After", |d| {
        let mut changed = 0;
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { return Err(bad(C, format!("no object {}", id.0))) };
            let mut e = n.extras().embroidery.as_deref().cloned().unwrap_or_default();
            if e.trim_after == on {
                continue;
            }
            e.trim_after = on;
            n.extras_mut().embroidery = (e != Embroidery::default()).then(|| Box::new(e));
            changed += 1;
        }
        Ok(changed)
    })?;
    Ok(serde_json::json!({ "changed": changed }))
}

/// The palette the stitch plan matches a document's colours to when the document sets none, or
/// sets one this version doesn't know: Janome's JEF palette. The project owner's product choice
/// (2026-10-21), not a sourced default. Each format still writes its own palette's threads: a PES
/// export matches the art's colours to Brother's palette itself.
pub(crate) const DEFAULT_PALETTE: Palette = Palette::Jef;

/// The thread palette the document's colours are matched to: the one it sets
/// (`embroidery.setup`), else [`DEFAULT_PALETTE`].
pub(crate) fn thread_palette(d: &Document) -> Palette {
    d.embroidery.as_deref().and_then(|e| e.palette.as_ref()).and_then(ThreadPalette::id).and_then(Palette::from_id).unwrap_or(DEFAULT_PALETTE)
}

/// The document's embroidery settings as `embroidery.setup` reports them (and takes them back).
fn setup_json(d: &Document) -> Value {
    let palette = d.embroidery.as_deref().and_then(|e| e.palette.clone()).map(Value::from);
    json!({"palette": palette, "threadPalette": thread_palette(d).id()})
}

fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setup";
    let empty = Map::new();
    let o = match p {
        Value::Null => &empty,
        Value::Object(o) => o,
        _ => return Err(bad(C, "params must be an object")),
    };
    let d = &s.doc()?.doc;
    let mut new = d.embroidery.as_deref().cloned().unwrap_or_default();
    for (k, v) in o {
        match k.as_str() {
            "palette" => new.palette = palette_param(v, new.palette.as_ref())?,
            // Reported only.
            "threadPalette" => {}
            _ => return Err(bad(C, format!("unknown setting `{k}`: the settings are palette"))),
        }
    }
    let new = (!new.is_empty()).then(|| Box::new(new));
    if new == d.embroidery {
        return Ok(setup_json(d));
    }
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = new;
        Ok(())
    })?;
    Ok(setup_json(&s.doc()?.doc))
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

fn threads(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.threads";
    let opts = Options::from_params(p, C)?;
    let d = &s.doc()?.doc;
    let plan = digitize::digitize(d, &opts).map_err(|e| bad(C, e))?;
    let hex = |[r, g, b]: [u8; 3]| Color::rgb8(r, g, b).to_hex();
    // Two decimals: finer than any difference the eye sees.
    let rounded = |v: f64| (v * 100.0).round() / 100.0;
    let blocks: Vec<Value> = plan
        .blocks
        .iter()
        .enumerate()
        .map(|(k, b)| {
            let t = &b.thread;
            json!({
                // Numbered from 1, as a list of threads shows them.
                "block": k + 1,
                "art": hex(t.art),
                "thread": {"palette": t.palette.id(), "index": t.index, "name": t.name, "color": hex(t.color)},
                "deltaE": rounded(t.delta_e),
                "mismatch": t.mismatch(),
                "units": b.units.len(),
            })
        })
        .collect();
    Ok(json!({"palette": thread_palette(d).id(), "threshold": MISMATCH_DELTA_E, "blocks": blocks, "warnings": plan.warnings}))
}
