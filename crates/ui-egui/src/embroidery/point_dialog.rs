//! The Entry Point and Exit Point dialog: a point's X and Y typed in the rulers' unit and from their
//! origin (as the Transform panel shows positions), or Auto, the point left to the planner. The
//! embroidery tool asks for it (Enter on a point, or a double-click on one,
//! [`vectorcraft_tools::embroidery::POINT_DIALOG`]), and so do the Object tab's Entry and Exit rows;
//! OK (or Enter) sets the point with `embroidery.setEntryExit {…, snap: true}`, taken to where the
//! stitches can start or end, as a drag places it; Escape or Cancel changes nothing.
//!
//! It is one of upstream's dialogs ([`SPEC`], a row of the dialog registry): the shared frame, its
//! first field (X) taking the keyboard, the Enter that opened it not taken for its OK, and
//! `ui.inspect` and `ui.dialog.set` / `ui.dialog.confirm` reaching it as every dialog. It is of
//! the document it was opened on, and closes as Cancel when that is no longer the active one.
//!
//! Fields: `kind` ("entry" or "exit"), `id` (the object), `part` ("fill" or "stroke"), `x` and `y`
//! (document points), `auto` (the point left to the planner: OK unsets it), `uid` (the document).

use serde_json::{Value, json};

use crate::VectorcraftApp;
use crate::dialogs::{AFTER_PANEL, DialogResult, DialogSpec};
use crate::state::Dialog;

/// The dialog's kind, as the tool asks for it.
pub const KIND: &str = vectorcraft_tools::embroidery::POINT_DIALOG;

/// The fields' width, points.
const FIELD_WIDTH: f32 = 96.0;

/// Its row of the dialog registry.
pub(crate) const SPEC: DialogSpec = DialogSpec { heading, body, confirm, min_width: 260.0, ..DialogSpec::FORM };

/// Whether `kind` names this dialog.
pub fn opens(kind: &str) -> bool {
    kind == KIND
}

/// Open the dialog on the tool's params `p` (`{kind, id, part, at: [x, y], set}`), on the active
/// document; params that don't say which point open nothing.
pub fn open(app: &mut VectorcraftApp, p: &Value) {
    let at = |i: usize| p["at"].get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
    let (Some(kind), Some(id), Some(part), Some(x), Some(y)) = (p["kind"].as_str(), p["id"].as_u64(), p["part"].as_str(), at(0), at(1)) else {
        return;
    };
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    if !matches!(kind, "entry" | "exit") {
        return;
    }
    let auto = !p["set"].as_bool().unwrap_or(false);
    // Opened by Enter on a point: that Enter, still held, isn't its OK.
    let fields = json!({"kind": kind, "id": id, "part": part, "x": x, "y": y, "auto": auto, "uid": uid, AFTER_PANEL: true});
    app.ui.dialog = Some(Dialog::new(KIND, fields));
}

fn heading(d: &Dialog) -> String {
    if d.str("kind") == "entry" { crate::et!("Entry Point") } else { crate::et!("Exit Point") }.to_string()
}

/// X and Y (typing either one leaves Auto), Auto, and a note on where the point is taken.
fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    // Its document closed, or another made active: closed as Cancel.
    if app.session.active().map(|st| st.uid) != d.fields.get("uid").and_then(Value::as_u64) {
        return true;
    }
    let units = app.session.general_unit();
    let origin = app.ruler_origin();
    egui::Grid::new("embroidery-point-grid").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
        for (label, name, key, origin) in [("X:", "X", "x", origin.x), ("Y:", "Y", "y", origin.y)] {
            crate::widgets::dim_label(ui, label);
            let id = ("embroidery-point", key);
            if let Some(v) =
                crate::widgets::num_field(ui, id, Some(d.f64(key, 0.0) - origin), units, FIELD_WIDTH).map(|v| v + origin).filter(|v| v.is_finite())
            {
                d.fields.insert(key.into(), json!(v));
                d.fields.insert("auto".into(), json!(false));
            }
            ui.ctx().accesskit_node_builder(ui.id().with(id), |node| node.set_label(name));
            ui.end_row();
        }
    });
    let mut auto = d.bool("auto");
    let label =
        if d.str("kind") == "entry" { crate::et!("Auto (nearest where the needle is)") } else { crate::et!("Auto (where the plan ends the part)") };
    if ui.checkbox(&mut auto, label).changed() {
        d.fields.insert("auto".into(), json!(auto));
    }
    ui.label(egui::RichText::new(crate::et!("The point is taken to the nearest place the stitches can start or end.")).small());
    false
}

/// The command OK runs: the point set where typed (snapped as a drag is), or unset for Auto.
fn command(d: &Dialog) -> Value {
    let mut p = json!({"id": d.fields.get("id"), "part": d.fields.get("part"), "snap": true});
    let point = if d.bool("auto") { Value::Null } else { json!([d.f64("x", 0.0), d.f64("y", 0.0)]) };
    p[if d.str("kind") == "entry" { "entry" } else { "exit" }] = point;
    p
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> DialogResult {
    let r = app.run("embroidery.setEntryExit", command(d));
    app.ui.dialog = None;
    r
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_engine::Session;

    use super::*;
    use crate::Services;

    /// The tool's params open the dialog on the point, of the active document; junk opens nothing;
    /// OK's command sets the typed point (snapped), or unsets it for Auto.
    #[test]
    fn the_dialog_opens_on_the_tools_point_and_sets_it() {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 200, "height": 200, "artboards": 1})).unwrap();
        for junk in [
            json!({}),
            json!({"kind": "rail", "id": 1, "part": "fill", "at": [0, 0]}),
            json!({"kind": "entry", "id": 1, "part": "fill", "at": ["far", 0]}),
        ] {
            open(&mut app, &junk);
            assert!(app.ui.dialog.is_none(), "{junk}");
        }
        open(&mut app, &json!({"kind": "exit", "id": 7, "part": "stroke", "at": [30.5, 40.0], "set": true}));
        let d = app.ui.dialog.clone().unwrap();
        assert_eq!((d.kind.as_str(), d.fields.get("uid").and_then(Value::as_u64)), (KIND, app.session.active().map(|st| st.uid)));
        assert_eq!(command(&d), json!({"id": 7, "part": "stroke", "exit": [30.5, 40.0], "snap": true}));
        let mut auto = d;
        auto.fields.insert("auto".into(), json!(true));
        assert_eq!(command(&auto), json!({"id": 7, "part": "stroke", "exit": null, "snap": true}));
        open(&mut app, &json!({"kind": "entry", "id": 7, "part": "fill", "at": [1, 2]}));
        assert!(app.ui.dialog.as_ref().unwrap().bool("auto"), "a point not set opens as Auto");
    }
}
