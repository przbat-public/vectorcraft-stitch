//! Restore Object's question, asked only when the stitches of an object converted to manual
//! stitches were edited since converting (the project owner's choice): restoring discards those
//! edits, which Undo brings back. Restore (or Enter) runs `embroidery.restoreObject` on the objects
//! it was asked for; Escape or Cancel changes nothing. The Enter that opened it, on the Object tab's
//! button, is its Restore only once let go and pressed again.
//!
//! It is one of upstream's dialogs ([`SPEC`], a row of the dialog registry). It is of the document
//! it was opened on, and closes as Cancel when that is no longer the active one.
//!
//! Fields: `ids` (the objects, as the Object tab gives them), `count` (the converted objects among
//! them), `uid` (the document).

use serde_json::{Value, json};

use crate::VectorcraftApp;
use crate::dialogs::{AFTER_PANEL, DialogResult, DialogSpec};
use crate::state::Dialog;

/// The dialog's kind.
pub const KIND: &str = "embroidery.restoreObject";

/// Its row of the dialog registry.
pub(crate) const SPEC: DialogSpec = DialogSpec { heading, body, confirm, ok: Some("Restore"), min_width: 300.0, ..DialogSpec::FORM };

/// Ask before restoring the `count` converted objects among `ids` of the active document.
pub(crate) fn open(app: &mut VectorcraftApp, ids: Value, count: usize) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    // Opened by Enter on the tab's button: that Enter, still held, isn't its Restore.
    app.ui.dialog = Some(Dialog::new(KIND, json!({"ids": ids, "count": count, "uid": uid, AFTER_PANEL: true})));
}

fn heading(d: &Dialog) -> String {
    let count = d.fields.get("count").and_then(Value::as_u64).unwrap_or(1);
    crate::etn!(count, "Restore {count} object?", "Restore {count} objects?")
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    // Its document closed, or another made active: closed as Cancel.
    if app.session.active().map(|st| st.uid) != d.fields.get("uid").and_then(Value::as_u64) {
        return true;
    }
    ui.label(crate::et!("Changes made to the stitches since converting will be lost; Undo brings them back."));
    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> DialogResult {
    let ids = d.fields.get("ids").cloned().unwrap_or(Value::Null);
    app.ui.dialog = None;
    let done = app.run("embroidery.restoreObject", json!({ "ids": ids }));
    if let Ok(v) = &done {
        app.status(restored(v));
    }
    done
}

/// The status bar's words after restoring, from the command's reply.
pub(crate) fn restored(reply: &Value) -> String {
    let count = reply["restoredCount"].as_u64().unwrap_or(0);
    crate::etn!(count, "{count} object restored", "{count} objects restored")
}
