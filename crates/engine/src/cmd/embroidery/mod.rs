//! Machine embroidery in the document: the settings an object carries for it, and the walk that
//! plans the document's art as stitches. The machine files themselves are read and written in
//! `cmd::fileio::embroidery` (`document.open`, `document.export`).
//!
//! The parts: [`options`] (the export's options, and the stitch planner's parameters they give an
//! object), [`digitize`] (the document's art planned as stitches) and [`sequence`] (the order the
//! blocks are sewn in).

pub(crate) mod digitize;
pub(crate) mod options;
mod sequence;

use serde_json::Value;
use vectorcraft_doc::embroidery::Embroidery;

use super::*;

pub(crate) use options::Options;

/// The embroidery settings an object carries.
pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "embroidery.setTrimAfter",
        "Trim After",
        [],
        None,
        "{on: bool, ids?|id?} whether the thread is cut after each object (or group) in ids or the selection when the document is stitched (document.export to dst, exp, pes or jef), however near the next object starts; off, only a jump longer than the export's trimLength is cut. One undo step; saved with the document → {changed}",
        has_doc,
        set_trim_after
    )]
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
