//! The fork's items in the app's menus and title: View › Stitch Preview (⌘E) and its options, after
//! Pixel Preview, checked as the active document's stitch view is (and Show Check Marks, for the
//! check's marks on the canvas); and the document tab's mode, "Stitches" while the canvas shows
//! them.

use serde_json::Value;
use vectorcraft_engine::DocState;
use vectorcraft_engine::cmd::embroidery::view::{layers, paused};

use crate::VectorcraftApp;
use crate::menus::Item;

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
