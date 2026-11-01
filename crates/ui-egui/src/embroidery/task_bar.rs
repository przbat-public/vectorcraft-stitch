//! The Contextual Task Bar while stitches are edited (VectorCraft Stitch's manual stitches): under
//! the selected needle points, the stitch actions in place of upstream's (the project owner's
//! choice, 2026-10-27: the actions live in the Contextual Task Bar, the Stitches tab and the menus,
//! not in a bar of their own). Each runs its stitch command on the selection, one undo step. The
//! bar is upstream's, reached by three marked lines in `canvas.rs` (the hook list in FORK.md); it
//! hides itself while a drag is under way, as always.

use serde_json::{Value, json};

use crate::VectorcraftApp;

/// One of the bar's items: its words (and the panel's ≡ menu's, where they say more), its icon, its
/// command and the command's params.
#[derive(Clone, Debug)]
pub struct Item {
    pub label: &'static str,
    pub menu_label: Option<&'static str>,
    pub icon: &'static str,
    pub command: &'static str,
    pub params: Value,
}

impl Item {
    fn new(label: &'static str, icon: &'static str, command: &'static str, params: Value) -> Self {
        Self { label, menu_label: None, icon, command, params }
    }

    /// As upstream's bar lists its items: words, icon, command and params.
    pub fn upstream(self) -> (&'static str, &'static str, &'static str, Value) {
        (self.label, self.icon, self.command, self.params)
    }
}

/// Whether the bar shows for the Embroidery Tool now: while it edits stitches and needle points are
/// selected.
pub fn shows(app: &mut VectorcraftApp) -> bool {
    super::editing::embroidery_tool(app) && super::editing::editing(app).is_some_and(|e| e.selected > 0)
}

/// The bar's items in place of upstream's while stitches are edited with needle points selected:
/// Insert, Delete, Split Run, Join Runs, Trim, Stop and Split Long Stitches (by both lines of the
/// one set of limits when the format the Check tab checks for has records shorter than the longest
/// stitch: as its writer cuts them, and within the longest stitch; else within the longest stitch);
/// `None` otherwise (upstream's items).
pub fn items(app: &mut VectorcraftApp) -> Option<Vec<Item>> {
    if !shows(app) {
        return None;
    }
    let limits = super::limits(app)?;
    let split_long = if limits.step < limits.longest { json!({"format": limits.format.id(), "maxLength": limits.longest}) } else { json!({}) };
    let function = "embroidery.setMachineFunction";
    Some(vec![
        Item::new(crate::et!("Insert"), "pen-tool-add", "embroidery.insertNeedlePoints", json!({})),
        Item::new(crate::et!("Delete"), "pen-tool-delete", "embroidery.deleteNeedlePoints", json!({})),
        Item::new(crate::et!("Split Run"), "dc-line-cut", "embroidery.splitRun", json!({})),
        Item::new(crate::et!("Join Runs"), "dc-join", "embroidery.joinRuns", json!({})),
        Item {
            menu_label: Some(crate::et!("Trim After the Selected Needle Points")),
            ..Item::new(crate::et!("Trim"), "scissors", function, json!({"function": "trim"}))
        },
        Item {
            menu_label: Some(crate::et!("Stop After the Selected Needle Points")),
            ..Item::new(crate::et!("Stop"), "square", function, json!({"function": "stop"}))
        },
        Item::new(crate::et!("Split Long Stitches"), "dc-pf-divide", "embroidery.splitLongStitches", split_long),
    ])
}

#[cfg(test)]
mod tests;
