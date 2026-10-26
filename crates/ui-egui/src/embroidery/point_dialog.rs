//! The Entry Point and Exit Point dialog: a point's X and Y typed in the rulers' unit and from their
//! origin (as the Transform panel shows positions), or Auto, the point left to the planner. The
//! embroidery tool asks for it (Enter on a point, or a double-click on one,
//! [`vectorcraft_tools::embroidery::POINT_DIALOG`]); OK sets the point with
//! `embroidery.setEntryExit {…, snap: true}`, taken to where the stitches can start or end, as a
//! drag places it; Escape or Cancel changes nothing.

use egui::{Align2, Key, vec2};
use serde_json::{Value, json};

use crate::VectorcraftApp;

/// The fields' width, points.
const FIELD_WIDTH: f32 = 96.0;

/// The dialog open: which point (its kind, object and part), its X and Y (document points), and
/// whether it is left to the planner.
#[derive(Clone, Debug, PartialEq)]
pub struct PointDialog {
    kind: String,
    id: u64,
    part: String,
    x: f64,
    y: f64,
    auto: bool,
}

impl PointDialog {
    /// The dialog on point `kind` of part `part` of object `id`, at (`x`, `y`), Auto or not (the
    /// tests' expectations).
    #[cfg(test)]
    pub(crate) fn new(kind: &str, id: u64, part: &str, x: f64, y: f64, auto: bool) -> Self {
        Self { kind: kind.into(), id, part: part.into(), x, y, auto }
    }

    /// The dialog with Auto ticked or not (the tests tick it as a click would).
    #[cfg(test)]
    pub(crate) fn set_auto(self, auto: bool) -> Self {
        Self { auto, ..self }
    }
}

/// Whether the dialog is open: it has the keyboard then.
pub fn is_open(app: &VectorcraftApp) -> bool {
    app.embroidery.point_dialog.is_some()
}

/// Whether `kind` names this dialog.
pub fn opens(kind: &str) -> bool {
    kind == vectorcraft_tools::embroidery::POINT_DIALOG
}

/// Open the dialog on the tool's params `p` (`{kind, id, part, at: [x, y], set}`); params that
/// don't say which point open nothing.
pub fn open(app: &mut VectorcraftApp, p: &Value) {
    let at = |i: usize| p["at"].get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
    let (Some(kind), Some(id), Some(part), Some(x), Some(y)) = (p["kind"].as_str(), p["id"].as_u64(), p["part"].as_str(), at(0), at(1)) else {
        return;
    };
    if !matches!(kind, "entry" | "exit") {
        return;
    }
    let auto = !p["set"].as_bool().unwrap_or(false);
    app.embroidery.point_dialog = Some(PointDialog { kind: kind.into(), id, part: part.into(), x, y, auto });
}

/// The command OK runs: the point set where typed (snapped as a drag is), or unset for Auto.
fn command(d: &PointDialog) -> Value {
    let mut p = json!({"id": d.id, "part": d.part, "snap": true});
    p[d.kind.as_str()] = if d.auto { Value::Null } else { json!([d.x, d.y]) };
    p
}

/// Draw the dialog while it is open; OK (or Enter) sets the point, Cancel (or Escape) closes it.
pub fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(mut d) = app.embroidery.point_dialog.clone() else { return };
    let units = app.session.general_unit();
    let origin = app.ruler_origin();
    let title = if d.kind == "entry" { crate::et!("Entry Point") } else { crate::et!("Exit Point") };
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Grid::new("embroidery-point-grid").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
            for (label, id, value, origin, x) in
                [("X:", "embroidery-point-x", d.x, origin.x, true), ("Y:", "embroidery-point-y", d.y, origin.y, false)]
            {
                crate::widgets::dim_label(ui, label);
                let edited = ui.add_enabled_ui(!d.auto, |ui| crate::widgets::num_field(ui, id, Some(value - origin), units, FIELD_WIDTH)).inner;
                if let Some(v) = edited.map(|v| v + origin).filter(|v| v.is_finite()) {
                    if x {
                        d.x = v;
                    } else {
                        d.y = v;
                    }
                }
                ui.end_row();
            }
        });
        ui.checkbox(&mut d.auto, crate::et!("Auto (nearest where the needle is)"));
        ui.label(egui::RichText::new(crate::et!("The point is taken to the nearest place the stitches can start or end.")).small());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ok = ui.button(crate::et!("OK")).clicked();
            cancel = ui.button(crate::et!("Cancel")).clicked();
        });
    });
    let (enter, escape) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if cancel || escape {
        app.embroidery.point_dialog = None;
        return;
    }
    if ok || enter {
        app.embroidery.point_dialog = None;
        if let Err(e) = app.run("embroidery.setEntryExit", command(&d)) {
            app.status(crate::i18n::msg(&e));
        }
        return;
    }
    app.embroidery.point_dialog = Some(d);
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_engine::Session;

    use super::*;
    use crate::Services;

    /// The tool's params open the dialog on the point; junk opens nothing; OK's command sets the
    /// typed point (snapped), or unsets it for Auto.
    #[test]
    fn the_dialog_opens_on_the_tools_point_and_sets_it() {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        for junk in [
            json!({}),
            json!({"kind": "rail", "id": 1, "part": "fill", "at": [0, 0]}),
            json!({"kind": "entry", "id": 1, "part": "fill", "at": ["far", 0]}),
        ] {
            open(&mut app, &junk);
            assert!(app.embroidery.point_dialog.is_none(), "{junk}");
        }
        open(&mut app, &json!({"kind": "exit", "id": 7, "part": "stroke", "at": [30.5, 40.0], "set": true}));
        let d = app.embroidery.point_dialog.clone().unwrap();
        assert_eq!(command(&d), json!({"id": 7, "part": "stroke", "exit": [30.5, 40.0], "snap": true}));
        assert_eq!(command(&PointDialog { auto: true, ..d }), json!({"id": 7, "part": "stroke", "exit": null, "snap": true}));
        open(&mut app, &json!({"kind": "entry", "id": 7, "part": "fill", "at": [1, 2]}));
        assert!(app.embroidery.point_dialog.as_ref().unwrap().auto, "a point not set opens as Auto");
    }
}
