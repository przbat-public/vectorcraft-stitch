//! The Control bar while the Embroidery Tool is active (the project owner's choice, 2026-10-25: the
//! part picked there too, and Nearest Connection beside it): which part's points show, Fill or
//! Stroke, kept in step with the Object tab's switch ([`super::handles`]), and Nearest Connection,
//! which searches off the interface thread ([`super::nearest::start`]) and waits while one runs; and
//! the tool's hint on the status bar ([`HINT`]).

use egui::{Ui, vec2};

use crate::VectorcraftApp;
use crate::panels::embroidery::object::Part;
use crate::panels::embroidery::{Segment, command_label, segments};

/// The tool's hint on the status bar, as upstream's `hint_for` gives every tool's: the keys and
/// gestures (in bold) and what they do, each translated as upstream's hints are.
pub(crate) const HINT: &[(&str, bool)] = &[
    ("Drag", true),
    (" a point to move it, or an arrow's head to turn the rows  |  ", false),
    ("Alt+Click", true),
    (" a point or an arrow to put it back to Auto  |  ", false),
    ("Click", true),
    (" an object to show its points  |  ", false),
    ("[ ]", true),
    (" step from point to point, the arrows nudge it, Enter types it", false),
];

/// The hint's id while the tool edits stitches (in isolation of a stitch object): `hint_for`'s arm
/// for [`EDITING_HINT`], which [`hint_id`] picks.
pub(crate) const EDITING_HINT_ID: &str = "embroidery.editingStitches";

/// The tool's hint while it edits stitches: the gestures and keys of stitch editing (the project
/// owner's choices, 2026-10-27), each translated as upstream's hints are.
pub(crate) const EDITING_HINT: &[(&str, bool)] = &[
    ("Drag", true),
    (" needle points, or a frame round them  |  ", false),
    ("Alt+Click", true),
    (" a stitch to insert a needle point  |  ", false),
    ("←↑→↓", true),
    (" move by 0.1 mm  |  ", false),
    ("Esc", true),
    (" deselects, then ends editing", false),
];

/// The hint the status bar shows for `tool`: the stitch-editing hint ([`EDITING_HINT_ID`]) while the
/// Embroidery Tool edits stitches, else the tool's own (upstream's choice by the tool's id).
pub(crate) fn hint_id(app: &mut VectorcraftApp, tool: &'static str) -> &'static str {
    if tool == ID_OF_TOOL && super::editing::editing(app).is_some() { EDITING_HINT_ID } else { tool }
}

/// The Embroidery Tool's id.
const ID_OF_TOOL: &str = vectorcraft_tools::embroidery::ID;

/// The command its button runs: its label is the menu's.
const NEAREST: &str = "embroidery.nearestConnection";

/// The part switch's size and its words' size, points (the Control bar's row and words).
const SWITCH_WIDTH: f32 = 120.0;
const SWITCH_HEIGHT: f32 = 20.0;
const SWITCH_TEXT: f32 = 12.0;

/// The button's least width, points.
const BUTTON_WIDTH: f32 = 64.0;

/// Draw the tool's options on the Control bar.
pub fn control_bar(app: &mut VectorcraftApp, ui: &mut Ui) {
    let shown = app.ui.embroidery_panel.part;
    let labels = [(Part::Fill, crate::et!("Fill")), (Part::Stroke, crate::et!("Stroke"))];
    let items: Vec<Segment<'_>> = labels
        .iter()
        .map(|(part, label)| Segment {
            id: ui.id().with(("embroidery-tool-part", part.id())),
            label,
            full: label,
            selected: shown == *part,
            enabled: true,
        })
        .collect();
    let chosen = ui.allocate_ui(vec2(SWITCH_WIDTH, SWITCH_HEIGHT), |ui| segments(ui, SWITCH_HEIGHT, SWITCH_TEXT, &items)).inner;
    if let Some((part, _)) = chosen.and_then(|i| labels.get(i)) {
        app.ui.embroidery_panel.part = *part;
    }
    ui.separator();
    let free = app.session.active().is_some() && !super::nearest::running(app);
    let label = command_label(app, NEAREST);
    if ui.add_enabled_ui(free, |ui| crate::widgets::flat_button_fit(ui, &label, BUTTON_WIDTH)).inner.clicked() {
        super::nearest::start(app);
    }
    ui.separator();
}

#[cfg(test)]
mod tests {
    use egui::accesskit::{Action, ActionRequest, NodeId, TreeId};
    use serde_json::json;
    use vectorcraft_engine::Session;

    use super::*;
    use crate::Services;

    /// One frame of the Control bar's options with `events` → its named nodes and whether each is
    /// on.
    fn bar(app: &mut VectorcraftApp, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<(NodeId, String, bool)> {
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            ui.horizontal(|ui| crate::toolbar::control_bar_options(app, ui));
        });
        out.textures_delta.clear();
        let tree = out.platform_output.accesskit_update.expect("an accessibility tree");
        tree.nodes.iter().filter_map(|(id, n)| Some((*id, n.label()?.trim().to_string(), !n.is_disabled()))).collect()
    }

    fn click(id: NodeId) -> egui::Event {
        egui::Event::AccessKitActionRequest(ActionRequest { action: Action::Click, target_node: id, target_tree: TreeId::ROOT, data: None })
    }

    /// With the Embroidery Tool active, the Control bar picks the part (the Object tab's, which the
    /// tool follows) and runs Nearest Connection, which applies as one undo step once it finishes;
    /// its button waits while it runs.
    #[test]
    fn the_tools_bar_picks_the_part_and_runs_nearest_connection() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 400, "artboards": 1})).unwrap();
        app.run("paint.setFill", json!({"none": true})).unwrap();
        app.run("paint.setStroke", json!({"color": "#2233cc"})).unwrap();
        let pt = |v: f64| (30.0 + v) * 72.0 / 25.4;
        for ((ax, ay), (bx, by)) in [((-20.0, 10.0), (0.0, 0.0)), ((1.0, 0.0), (-6.0, 0.0)), ((7.0, 0.0), (27.0, 0.0))] {
            let d = format!("M {} {} L {} {}", pt(ax), pt(ay), pt(bx), pt(by));
            let id = app.run("path.create", json!({ "d": d })).unwrap()["id"].clone();
            app.run("stroke.set", json!({"weight": 1.0, "ids": [id]})).unwrap();
        }
        app.run("select.set", json!({"ids": []})).unwrap();
        app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
        let find = |nodes: &[(NodeId, String, bool)], name: &str| {
            nodes.iter().find(|n| n.1 == name).cloned().unwrap_or_else(|| panic!("{name} in {nodes:?}"))
        };
        let nodes = bar(&mut app, &ctx, vec![]);
        assert_eq!(app.ui.embroidery_panel.part, Part::Fill);
        bar(&mut app, &ctx, vec![click(find(&nodes, "Stroke").0)]);
        assert_eq!(app.ui.embroidery_panel.part, Part::Stroke, "the Object tab's part, which the tool follows");
        let steps = app.session.doc().unwrap().history.undo.len();
        let nodes = bar(&mut app, &ctx, vec![]);
        let (button, _, on) = find(&nodes, "Nearest Connection");
        assert!(on);
        bar(&mut app, &ctx, vec![click(button)]);
        if crate::embroidery::nearest::running(&app) {
            assert!(!find(&bar(&mut app, &ctx, vec![]), "Nearest Connection").2, "waits while it runs");
        }
        for _ in 0..1000 {
            if !crate::embroidery::nearest::running(&app) {
                break;
            }
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| crate::embroidery::nearest::poll(&mut app, ui.ctx()));
            out.textures_delta.clear();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(app.session.doc().unwrap().history.undo.len(), steps + 1, "{}", app.ui.status);
    }
}
