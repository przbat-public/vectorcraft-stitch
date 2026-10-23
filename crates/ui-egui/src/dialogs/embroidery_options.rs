//! The embroidery rows of the export options dialog ([`super::save_options`]), for the eight
//! machine formats: each option's label and tip in the interface language, a list for `stitch`
//! and `underlay`, and each number kept within the range the planner takes
//! ([`vectorcraft_engine::cmd::embroidery::dialog_field`]), a negative `expand` among them, so that
//! the dialog never writes what the export refuses.

use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::{DialogField, dialog_field};
use vectorcraft_engine::cmd::fileio::FormatOption;

use crate::state::Dialog;
use crate::theme::Tokens;
use crate::widgets;

/// The width of a list, as the dialog's other lists are.
const LIST_WIDTH: f32 = 254.0;
/// The width of a number field, as the dialog's other number fields are.
const NUMBER_WIDTH: f32 = 80.0;

/// Embroidery option `name`'s label; `None` for another format's option.
pub(super) fn label(name: &str) -> Option<String> {
    let label = match name {
        "stitch" => tl!("Stitch:"),
        "spacing" => tl!("Spacing:"),
        "length" => tl!("Stitch Length:"),
        "angle" => tl!("Fill Angle:"),
        "pull" => tl!("Pull Compensation:"),
        "pullPercent" => tl!("Pull Compensation (%):"),
        "push" => tl!("Push Compensation:"),
        "expand" => tl!("Expand Fills:"),
        "narrowAsSatin" => tl!("Narrow Parts as Satin:"),
        "threadWeight" => tl!("Thread Weight:"),
        "underlay" => tl!("Underlay:"),
        "jumpLength" => tl!("Jump Length:"),
        "trimLength" => tl!("Trim Length:"),
        "minStitch" => tl!("Shortest Stitch:"),
        _ => return None,
    };
    Some(label.to_string())
}

/// What embroidery option `name` does, as its row's tip.
fn tip(name: &str) -> &'static str {
    match name {
        "stitch" => tl!(
            "How shapes are sewn: Automatic fills shapes with tatami and sews strokes as satin columns or running lines by their width; another choice sews every shape that way"
        ),
        "spacing" => tl!("The distance between fill rows and between satin stitches; left as it is, a satin column picks it by its width"),
        "length" => tl!("The length of the stitches along fill rows and running lines"),
        "angle" => tl!("The direction of the fill rows"),
        "pull" => tl!("Widens satin columns and fill rows on each side, to make up for the fabric pulling in"),
        "pullPercent" => tl!("Widens satin columns and fill rows by a share of their width, to make up for the fabric pulling in"),
        "push" => tl!("Shortens satin columns at their ends, where the thread pushes outward"),
        "expand" => tl!("Grows each filled shape all round to close gaps between shapes; a negative value shrinks it"),
        "narrowAsSatin" => tl!("Parts of a fill narrower than this are sewn as satin columns; 0 sews none"),
        "threadWeight" => tl!("The thread's weight: thinner thread (a higher number) packs the rows closer"),
        "underlay" => tl!("The stitches sewn first to hold the fabric: chosen by each shape, none, or one kind for every shape"),
        "jumpLength" => tl!("A move longer than this lifts the needle instead of being sewn"),
        "trimLength" => tl!("A jump longer than this cuts the thread first"),
        "minStitch" => tl!("Shorter stitches are left out"),
        _ => "",
    }
}

/// How a value of a list is shown.
fn choice(name: &str, id: &str) -> &'static str {
    match (name, id) {
        (_, "auto") => tl!("Automatic"),
        ("stitch", "tatami") => tl!("Tatami Fill"),
        ("stitch", "satin") => tl!("Satin"),
        ("stitch", "running") => tl!("Running Stitch"),
        ("underlay", "none") => tl!("None"),
        ("underlay", "center") => tl!("Centre Run"),
        ("underlay", "edge") => tl!("Edge Run"),
        ("underlay", "edgeZigzag") => tl!("Edge Run and Zigzag"),
        ("underlay", "edgeTatami") => tl!("Edge Run and Tatami"),
        _ => "",
    }
}

/// Draw embroidery option `o`'s row, whose value is `value`, into `d`; false (nothing drawn) for
/// another format's option.
pub(super) fn row(ui: &mut egui::Ui, d: &mut Dialog, o: &FormatOption, value: &Value) -> bool {
    let (Some(field), Some(label)) = (dialog_field(o.name), label(o.name)) else { return false };
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(&label).color(t.text_dim)).on_hover_text(tip(o.name));
    match field {
        DialogField::Choice(ids) => {
            // `underlay` is also taken as true (chosen by the shape) or false (none).
            let current = match value {
                Value::Bool(true) => "auto",
                Value::Bool(false) => "none",
                Value::String(s) => s.as_str(),
                _ => "auto",
            };
            let names: Vec<&str> = ids.iter().map(|id| choice(o.name, id)).collect();
            let at = ids.iter().position(|id| *id == current).unwrap_or(0);
            let shown = names.get(at).copied().unwrap_or_default();
            if let Some(id) = widgets::dropdown(ui, ("save-option", o.name), shown, &names, LIST_WIDTH).and_then(|i| ids.get(i)) {
                d.fields.insert(o.name.into(), json!(id));
            }
        }
        DialogField::Number { min, max, unit, decimals } => {
            // One value for both sides (a per-side pair shows its first).
            let x = value.as_f64().or_else(|| value.as_array().and_then(|a| a.first()).and_then(Value::as_f64)).unwrap_or(0.0);
            let field = ui.scope(|ui| widgets::range_field(ui, ("save-option", o.name), x, min..=max, unit, decimals, NUMBER_WIDTH));
            field.response.on_hover_text(tip(o.name));
            if let Some(x) = field.inner {
                d.fields.insert(o.name.into(), json!(x));
            }
        }
    }
    ui.end_row();
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_engine::Session;
    use vectorcraft_engine::cmd::fileio;

    use super::*;
    use crate::{Services, VectorcraftApp, dialogs, theme};

    /// Every embroidery option has a row: a label, a tip, and a list whose every value has a name or
    /// a number field. A machine format's dialog draws them, keeping a negative `expand` and a chosen
    /// stitch as set.
    #[test]
    fn every_embroidery_option_has_a_row() {
        let f = fileio::format("dst").unwrap();
        for o in f.options {
            assert!(label(o.name).is_some() && !tip(o.name).is_empty(), "{}", o.name);
            if let Some(DialogField::Choice(ids)) = dialog_field(o.name) {
                assert!(ids.iter().all(|id| !choice(o.name, id).is_empty()), "{}", o.name);
            }
        }
        assert_eq!(label("compress"), None);
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut fields = dialogs::save_options::option_values(&mut app, f);
        fields.insert("path".into(), json!("design.dst"));
        fields.insert("format".into(), json!("dst"));
        fields.insert("expand".into(), json!(-1.5));
        fields.insert("stitch".into(), json!("satin"));
        app.ui.dialog = Some(Dialog { kind: dialogs::save_options::KIND.into(), fields });
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| dialogs::show(&mut app, ui.ctx()));
        out.textures_delta.clear();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.fields["expand"].clone(), d.str("stitch")), (json!(-1.5), "satin".to_string()));
    }
}
