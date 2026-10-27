//! The Contextual Task Bar while stitches are edited (VectorCraft Stitch's manual stitches): under
//! the selected needle points, the stitch actions in place of upstream's (the project owner's
//! choice, 2026-10-27: the actions live in the Contextual Task Bar, the Stitches tab and the menus,
//! not in a bar of their own). Each runs its stitch command on the selection, one undo step. The
//! bar is upstream's, reached by three marked lines in `canvas.rs` (the hook list in FORK.md); it
//! hides itself while a drag is under way, as always.

use serde_json::{Value, json};

use crate::VectorcraftApp;

/// One of the bar's items: its words, its icon, its command and the command's params.
pub type Item = (&'static str, &'static str, &'static str, Value);

/// Whether the bar shows for the Embroidery Tool now: while it edits stitches and needle points are
/// selected.
pub fn shows(app: &mut VectorcraftApp) -> bool {
    app.session.tool_id() == vectorcraft_tools::embroidery::ID && super::editing::editing(app).is_some_and(|e| e.selected > 0)
}

/// The bar's items in place of upstream's while stitches are edited with needle points selected:
/// Insert, Delete, Split Run, Join Runs, Trim, Stop and Split Long Stitches (long as the format the
/// Check tab checks for writes them, when its records are shorter than the longest stitch); `None`
/// otherwise (upstream's items).
pub fn items(app: &mut VectorcraftApp) -> Option<Vec<Item>> {
    if !shows(app) {
        return None;
    }
    let st = app.session.active()?;
    let limits = vectorcraft_engine::cmd::embroidery::stitch_limits(&st.doc, app.embroidery.check.format.get(&st.uid).copied());
    let split_long = if limits.step < limits.longest { json!({"format": limits.format.id()}) } else { json!({}) };
    Some(vec![
        (crate::et!("Insert"), "pen-tool-add", "embroidery.insertNeedlePoints", json!({})),
        (crate::et!("Delete"), "pen-tool-delete", "embroidery.deleteNeedlePoints", json!({})),
        (crate::et!("Split Run"), "dc-line-cut", "embroidery.splitRun", json!({})),
        (crate::et!("Join Runs"), "dc-join", "embroidery.joinRuns", json!({})),
        (crate::et!("Trim"), "scissors", "embroidery.setMachineFunction", json!({"function": "trim"})),
        (crate::et!("Stop"), "square", "embroidery.setMachineFunction", json!({"function": "stop"})),
        (crate::et!("Split Long Stitches"), "dc-pf-divide", "embroidery.splitLongStitches", split_long),
    ])
}

#[cfg(test)]
mod tests;
