//! The fork's items in the app's menus and title: View › Stitch Preview (⌘E) and its options, after
//! Pixel Preview, checked as the active document's stitch view is (and Show Check Marks, for the
//! check's marks on the canvas); the document tab's mode, "Stitches" while the canvas shows them;
//! and a person's Convert to Manual Stitches, Restore Object and Nearest Connection from the
//! command palette or a shortcut, run as the Embroidery panel runs them.

use serde_json::{Value, json};
use vectorcraft_engine::DocState;
use vectorcraft_engine::cmd::embroidery::view::{layers, paused};

use crate::VectorcraftApp;
use crate::menus::Item;

/// The commands a person's palette or shortcut runs as the Embroidery panel does.
const CONVERT: &str = "embroidery.convertToManual";
const RESTORE: &str = "embroidery.restoreObject";
const NEAREST: &str = "embroidery.nearestConnection";

/// A person's Convert to Manual Stitches, Restore Object or Nearest Connection (`id`, with no
/// params), from the command palette or a shortcut, run as the Embroidery panel runs it: Convert
/// and Nearest Connection off the interface thread, the status bar saying how far they have got
/// and Escape stopping them; Restore asking first when the stitches it would put back were edited
/// since converting, and saying what it restored → whether `id` was one of them. Agents' and
/// scripts' commands run in one go, as every command does.
pub fn invoked(app: &mut VectorcraftApp, id: &str) -> bool {
    match id {
        CONVERT => crate::embroidery::convert_job::start(app, &json!({})),
        NEAREST => crate::embroidery::nearest::start(app),
        RESTORE => restore_asking(app, Value::Null),
        _ => return false,
    }
    true
}

/// Restore Object on objects `ids` (null: the selection), as the Object tab's button and the
/// command palette run it: asking first when the stitches of an object it would put back were
/// edited since converting (a dry run says), else restoring at once and saying what it restored.
pub(crate) fn restore_asking(app: &mut VectorcraftApp, ids: Value) {
    let asked = match app.session.execute(RESTORE, &json!({"ids": ids, "dryRun": true})) {
        Ok(v) if v["editedCount"].as_u64().is_some_and(|n| n > 0) => v["restoredCount"].as_u64(),
        _ => None,
    };
    if let Some(count) = asked {
        // The question restores the same objects: the selection can't change while it is open.
        let count = usize::try_from(count).unwrap_or(usize::MAX);
        return crate::embroidery::restore_dialog::open(app, ids, count);
    }
    match app.run(RESTORE, json!({ "ids": ids })) {
        Ok(v) => app.status(crate::embroidery::restore_dialog::restored(&v)),
        Err(e) => app.status(e),
    }
}

/// View › Stitch Preview.
pub fn stitch_preview() -> Item {
    Item::Cmd("Stitch Preview", "view.stitchPreview", Value::Null)
}

/// View › Stitch Preview Options.
pub fn stitch_preview_options() -> Item {
    Item::Sub(
        "Stitch Preview Options",
        vec![
            Item::Cmd("Show Jumps and Cuts", "view.stitchPreview.jumps", Value::Null),
            Item::Cmd("Show Needle Points", "view.stitchPreview.needlePoints", Value::Null),
            Item::Cmd("Show Artwork Under Stitches", "view.stitchPreview.artwork", Value::Null),
            Item::Cmd("Show Check Marks", "view.checkMarks", Value::Null),
        ],
    )
}

/// Whether command `id`'s item is checked, for the stitch view's commands: as the active
/// document's stitch view is set, Stitch Preview only while the canvas can show the stitches (not
/// while an opacity mask or a pattern is edited, which shows the art, as the switch does); Show
/// Check Marks, as the session has it. `None` for any other command.
pub fn checked(app: &VectorcraftApp, id: &str) -> Option<bool> {
    if id == "view.checkMarks" {
        return Some(!app.embroidery.check.hide_marks);
    }
    let v = app.session.stitch_view();
    let paused = app.session.active().is_some_and(|st| paused(&st.doc).is_some());
    Some(match id {
        "view.stitchPreview" => v.on && !paused,
        "view.stitchPreview.jumps" => v.jumps,
        "view.stitchPreview.needlePoints" => v.needle_points,
        "view.stitchPreview.artwork" => v.artwork,
        _ => return None,
    })
}

/// Whether document `d`'s canvas shows its stitches (its tab's mode then says so).
pub fn shows_stitches(app: &VectorcraftApp, d: &DocState) -> bool {
    layers(app.session.stitch_view_of(d.uid), paused(&d.doc), app.ui.view.outline).stitches
}

/// The document tab's mode while the canvas shows the stitches, as in "(RGB/Stitches)".
pub fn tab_mode() -> &'static str {
    crate::et!("Stitches")
}
