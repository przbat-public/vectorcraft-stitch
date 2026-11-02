//! The Stitches tab's foot: while stitches are edited, Move by and the stitch actions in rows of
//! whole buttons; else Edit Stitches, or why the scope's stitches can't be edited.

use super::{CELL_PAD, Color32, Lang, Listed, PLACE_DECIMALS, ROW_HEIGHT, TEXT, Tokens, Ui, VectorcraftApp, dim, json, scope_ids};

/// How much the foot's height may change, points, before the next frame lays the list out again:
/// less than a pixel.
pub(super) const FOOT_SETTLED: f32 = 0.5;

/// The foot's height before it is first drawn, points: room kept under the list, rows of
/// [`ROW_HEIGHT`] for its lines (while editing: the selection, Move by and up to three of stitch
/// actions; else a line of words and a button). An estimate; the frame drawn keeps the real one.
pub(super) fn foot_height(editing: bool) -> f32 {
    /// The rows kept while editing, and outside it.
    const EDITING_ROWS: f32 = 5.0;
    const OUTSIDE_ROWS: f32 = 3.0;
    if editing { EDITING_ROWS * ROW_HEIGHT } else { OUTSIDE_ROWS * ROW_HEIGHT }
}

/// The foot: while stitches are edited, the selection, Move by X and Y with Apply, and the stitch
/// actions; else Edit Stitches for stitches that can be edited, or for planned stitches a line
/// saying they are planned and Convert to Manual Stitches.
pub(super) fn foot_ui(
    app: &mut VectorcraftApp,
    ui: &mut Ui,
    editing: Option<vectorcraft_engine::cmd::embroidery::StitchEditing>,
    listed: &Listed,
    lang: Lang,
    t: &Tokens,
) {
    ui.add_space(CELL_PAD);
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    if let Some(e) = editing {
        if e.selected > 0 {
            ui.label(
                egui::RichText::new(crate::etn!(e.selected as u64, "{count} needle point selected", "{count} needle points selected")).size(TEXT),
            );
            let mut by = app.embroidery.stitches.docs.entry(uid).or_default().move_by;
            let mut apply = false;
            let format = crate::embroidery::fields::format(lang, "mm", PLACE_DECIMALS);
            let bounds = -MOVE_BY_MAX_MM..=MOVE_BY_MAX_MM;
            // "Move by" on a line of its own; under it the two fields, with Apply beside them where
            // it fits (the panel's narrowest is 220 points), else under them.
            ui.label(egui::RichText::new(crate::et!("Move by")).size(TEXT));
            let apply_words = crate::et!("Apply");
            let width = |ui: &Ui, words: &str, style: egui::TextStyle| {
                ui.painter().layout_no_wrap(words.to_string(), style.resolve(ui.style()), Color32::WHITE).size().x
            };
            let gap = ui.spacing().item_spacing.x;
            let fields = width(ui, "X", egui::TextStyle::Body) + width(ui, "Y", egui::TextStyle::Body) + 2.0 * MOVE_BY_FIELD + 3.0 * gap;
            let button = width(ui, apply_words, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
            let one_row = ui.available_width() >= fields + gap + button;
            ui.horizontal(|ui| {
                for (k, axis) in ["X", "Y"].into_iter().enumerate() {
                    ui.label(axis);
                    let name = format!("{} {axis}", crate::et!("Move by"));
                    let look = crate::embroidery::fields::Look { name: Some(&name), ..Default::default() };
                    // Enter in the field moves them, by the value typed or, unchanged, by the one
                    // shown again.
                    let field = ui.scope_id().with(("embroidery-move-by", k));
                    apply |= ui.memory(|m| m.has_focus(field)) && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if let Some(v) =
                        crate::embroidery::fields::number_field(ui, ("embroidery-move-by", k), Some(by[k]), &format, &look, MOVE_BY_FIELD, &bounds)
                        && v.is_finite()
                    {
                        // A value typed past the range is held to it, as the arrows and the scrub
                        // hold theirs.
                        by[k] = v.clamp(*bounds.start(), *bounds.end());
                    }
                }
                if one_row {
                    apply |= ui.button(apply_words).clicked();
                }
            });
            if !one_row {
                apply |= ui.button(apply_words).clicked();
            }
            app.embroidery.stitches.docs.entry(uid).or_default().move_by = by;
            if apply
                && by != [0.0, 0.0]
                && let Err(err) = app.run(vectorcraft_tools::embroidery::MOVE, json!({"dx": by[0], "dy": by[1]}))
            {
                app.status(err);
            }
            // The stitch actions in rows of whole buttons; one wider than the panel (a long word at
            // its narrowest) on a row of its own, its words wrapped.
            let items = crate::embroidery::task_bar::items(app).unwrap_or_default();
            let full = ui.available_width();
            let mut rows: Vec<Vec<usize>> = Vec::new();
            let mut used = 0.0;
            for (k, item) in items.iter().enumerate() {
                let w = width(ui, item.label, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
                match rows.last_mut() {
                    Some(row) if used + gap + w <= full => {
                        row.push(k);
                        used += gap + w;
                    }
                    _ => {
                        rows.push(vec![k]);
                        used = w;
                    }
                }
            }
            let mut chosen = None;
            // Each action as the ≡ menu offers it: off when its command can't run now.
            let on: Vec<bool> = items.iter().map(|item| crate::menus::enabled(app, item.command)).collect();
            for row in rows {
                ui.horizontal(|ui| {
                    for k in row {
                        let (Some(item), Some(&on)) = (items.get(k), on.get(k)) else { continue };
                        if ui.add_enabled(on, egui::Button::new(item.label).wrap()).clicked() {
                            chosen = Some((item.command, item.params.clone()));
                        }
                    }
                });
            }
            if let Some((id, params)) = chosen {
                crate::menus::invoke(app, id, params);
            }
        } else {
            ui.label(egui::RichText::new(crate::et!("Click a row or a needle point to select it.")).size(TEXT).color(dim(t)));
        }
        return;
    }
    let ids = scope_ids(app);
    let Some(st) = app.session.active() else { return };
    let stitches = ids.iter().copied().find(|id| st.doc.node(*id).is_some_and(vectorcraft_tools::embroidery::contains_stitches));
    let planned = ids.iter().any(|id| st.doc.node(*id).is_some_and(|n| !vectorcraft_tools::embroidery::contains_stitches(n)));
    if let Some(id) = stitches {
        if ui.add(egui::Button::new(crate::et!("Edit Stitches")).wrap()).clicked() {
            crate::embroidery::editing::start(app, id);
        }
    } else if planned && listed.span.is_some() {
        ui.label(
            egui::RichText::new(crate::et!("Planned from the shape: convert the object to manual stitches to edit them.")).size(TEXT).color(dim(t)),
        );
        let label = super::super::command_label(app, "embroidery.convertToManual");
        if ui.add(egui::Button::new(label).wrap()).clicked() {
            crate::embroidery::convert_job::start(app, &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}));
        }
    }
}

/// The largest move Move by takes, millimetres: a needle point stays within any hoop. An estimate.
pub(super) const MOVE_BY_MAX_MM: f64 = 1000.0;
/// A Move by field's width, points: room for "−123,4 mm".
pub(super) const MOVE_BY_FIELD: f32 = 72.0;
