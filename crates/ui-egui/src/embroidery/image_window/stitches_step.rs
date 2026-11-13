//! The image window's fourth step, the Stitches: the document settings its design is sewn with
//! that the window changes (the fabric, the fill angle, joining one thread's objects: the project
//! owner's choice of 2026-11-12, as the palette, changed here, the preview using them at once, and
//! made the document's on Create, the window saying so); the rules that choose each area's stitch,
//! read only, each saying where its value comes from (the owner's choice: they change with the
//! fabric, or through Embroidery Settings…); and the sewing order, from the background to the
//! details (the owner's choice: the Order tab changes it after Create).

use egui::Ui;
use serde_json::{Map, Value, json};
use vectorcraft_engine::cmd::embroidery::setup::DocumentSettings;

use super::colours_step::note;
use crate::VectorcraftApp;
use crate::embroidery::fields;
use crate::embroidery::rows::{self, Columns, Edit, Marks, Owned, OwnedField};
use crate::embroidery::text::fill;
use crate::embroidery::vocabulary;
use crate::i18n::Lang;
use crate::state::Dialog;
use crate::theme::Tokens;

/// The document settings the window changes, in `embroidery.setup`'s names: absent, the
/// document's own.
pub(crate) const FABRIC: &str = "fabric";
pub(crate) const FILL_ANGLE: &str = "fillAngle";
pub(crate) const JOIN: &str = "joinSameColor";
pub(crate) const DOCUMENT_FIELDS: [&str; 3] = [FABRIC, FILL_ANGLE, JOIN];

/// The fabric of no preset's values (the built-in ones), and values of the document's own from no
/// preset, as the window's field holds them and as Document Setup's choice names them; the first
/// is `embroidery.setup`'s null.
pub(crate) const BUILT_IN: &str = "builtIn";
const CUSTOM: &str = "custom";

/// The keys of the rule rows (ids, never words).
const THIN_LINES: &str = "thinLines";
const NARROW: &str = "narrowAsSatin";
const SPACING: &str = "spacing";

/// The decimals a width in millimetres is written to (tenths: the formats' 0.1 mm), and a rows'
/// spacing (hundredths, as Document Setup writes it).
const MM_DECIMALS: usize = 1;
const SPACING_DECIMALS: usize = 2;

/// `embroidery.setup`'s params for the document settings `params` (the window's) change: each
/// field held, the built-in fabric as null.
pub(crate) fn document_of(params: &Value) -> Value {
    let mut o = Map::new();
    for k in DOCUMENT_FIELDS {
        if let Some(v) = params.get(k).filter(|v| !v.is_null()) {
            let v = if k == FABRIC && v == BUILT_IN { Value::Null } else { v.clone() };
            o.insert(k.into(), v);
        }
    }
    Value::Object(o)
}

/// The document's own fabric, as Document Setup names the choices: `builtIn`, `custom`, or a
/// preset's id.
fn document_fabric(document: &DocumentSettings) -> String {
    match &document.settings.fabric {
        None => BUILT_IN.to_string(),
        Some(f) => f.preset.clone().unwrap_or_else(|| CUSTOM.to_string()),
    }
}

/// The fill angle the window's fields choose, else the document's as the plan uses it: a number of
/// degrees, or `acrossShape`.
fn angle_now(d: &Dialog, document: &DocumentSettings) -> Value {
    d.fields.get(FILL_ANGLE).cloned().unwrap_or_else(|| document.resolved.fill_angle.value.clone())
}

/// Field `key` set to `value`, or left out when that is the document's own (`own`).
fn set_or_document(d: &mut Dialog, key: &str, value: Value, own: &Value) {
    if value == *own {
        d.fields.remove(key);
    } else {
        d.fields.insert(key.into(), value);
    }
}

/// The step's controls, for dialog `d`.
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let Some(reply) = live.reply().cloned() else { return };
    let Some(planned) = live.document_settings().cloned() else { return };
    let presets = live.fabric_presets(&mut app.session);
    let Some(document) = live.own_settings(&app.session).cloned() else { return };
    let mut owned = vec![fabric_row(d, &document, &presets), lines_row(&reply, lang), narrow_row(lang), wider_row(&planned, lang)];
    owned.extend(crate::embroidery::fill_angle::rows(&angle_now(d, &document)));
    let drawn: Vec<rows::Row<'_>> = owned.iter().map(|o| o.row(lang)).collect();
    let cols = Columns::fit(ui, ui.available_width(), &drawn);
    let mut edits: Vec<(&'static str, Edit)> = Vec::new();
    for (o, row) in owned.iter().zip(&drawn) {
        if let Some(edit) = rows::show(ui, &cols, row) {
            edits.push((o.key, edit));
        }
    }
    if let Some(words) = fields::take_unreadable(ui) {
        super::refuse(app, words);
    }
    for (key, edit) in edits {
        edited(app, d, &document, key, edit);
    }
    let join = d.fields.get(JOIN).and_then(Value::as_bool).unwrap_or(document.resolved.join_same_color.value);
    if rows::check(ui, crate::et!("Join one thread's objects when nothing lies between them"), Some(join), t.panel) {
        set_or_document(d, JOIN, json!(!join), &json!(document.resolved.join_same_color.value));
    }
    rows::heading(ui, crate::et!("Sewing order"));
    note(ui, crate::et!("From the background to the details. The Order tab changes it after Create."));
    if DOCUMENT_FIELDS.iter().any(|k| d.fields.contains_key(*k)) {
        note(ui, crate::et!("Create sets these in the document's embroidery settings."));
    }
}

/// The fabric's row: the built-in values, the presets (`presets`, their ids), and the document's
/// own values while it has them; the window's choice, else the document's.
fn fabric_row(d: &Dialog, document: &DocumentSettings, presets: &[String]) -> Owned {
    let own = document_fabric(document);
    let choice = d.fields.get(FABRIC).and_then(Value::as_str).map_or_else(|| own.clone(), str::to_string);
    let mut options = vec![(BUILT_IN.to_string(), crate::et!("Built-in values").to_string())];
    options.extend(presets.iter().map(|id| (id.clone(), vocabulary::fabric(id))));
    if own == CUSTOM {
        options.push((CUSTOM.to_string(), crate::et!("Custom").to_string()));
    }
    for id in [&own, &choice] {
        if !options.iter().any(|(o, _)| o == id) {
            // A preset this version doesn't know (a newer version's), the document's own.
            options.push((id.clone(), vocabulary::fabric(id)));
        }
    }
    let shown = options.iter().find(|(id, _)| *id == choice).map(|(_, w)| w.clone()).unwrap_or_default();
    let label = crate::et!("Fabric");
    let field = OwnedField::Choice { shown, options, current: Some(choice) };
    Owned::new(FABRIC, label, label.to_string(), vocabulary::setup_tip("fabric"), field, Marks::inherited(None, None))
}

/// The thin lines' rule: sewn as a running line along their middle, below the Shapes step's width.
fn lines_row(reply: &Value, lang: Lang) -> Owned {
    let below = reply["settings"]["thinLine"].as_f64().unwrap_or(0.0);
    let mm = fields::format(lang, "mm", MM_DECIMALS).show(below);
    let source = fill(crate::et!("Shapes: below {value}"), &[("value", &mm)]);
    let label = crate::et!("Thin lines");
    let field = OwnedField::Words(crate::et!("Running stitch along the middle").to_string());
    Owned::new(THIN_LINES, label, label.to_string(), crate::et!("Set in the Shapes step."), field, Marks::inherited(Some(source), None))
}

/// The narrow parts' rule: a fill's parts narrower than the planner's built-in width are satin.
fn narrow_row(lang: Lang) -> Owned {
    let mm = fields::format(lang, "mm", MM_DECIMALS).show(vectorcraft_engine::cmd::embroidery::NARROW_SATIN);
    let label = vocabulary::label(NARROW, None);
    let field = OwnedField::Words(fill(crate::et!("below {value}"), &[("value", &mm)]));
    Owned::new(NARROW, label, label.to_string(), vocabulary::tip(NARROW), field, Marks::inherited(Some(vocabulary::source("default", None)), None))
}

/// The wider parts' rule: tatami, its rows as far apart as the fabric the window plans with
/// (`planned`, the window's copy of the document's settings) has them, and where that comes from.
fn wider_row(planned: &DocumentSettings, lang: Lang) -> Owned {
    let spacing = &planned.resolved.fabric.tatami.spacing;
    let mm = fields::format(lang, "mm", SPACING_DECIMALS).show(spacing.value);
    let label = vocabulary::label(SPACING, Some("tatami"));
    let field = OwnedField::Words(fill(crate::et!("Tatami, rows {value} apart"), &[("value", &mm)]));
    let source = vocabulary::source(&spacing.source, Some("fabric"));
    Owned::new(SPACING, label, label.to_string(), vocabulary::tip(SPACING), field, Marks::inherited(Some(source), None))
}

/// What a row's person did, written into the window's fields: a fabric, or the fill angle (its
/// degrees refused past their range, said in the status bar); each left out where it is the
/// document's own.
fn edited(app: &mut VectorcraftApp, d: &mut Dialog, document: &DocumentSettings, key: &str, edit: Edit) {
    let own_angle = document.resolved.fill_angle.value.clone();
    if let (FABRIC, Edit::Choice(id)) = (key, &edit) {
        return set_or_document(d, FABRIC, json!(id), &json!(document_fabric(document)));
    }
    match crate::embroidery::fill_angle::edited(key, &edit, &angle_now(d, document)) {
        Ok(Some(value)) => set_or_document(d, FILL_ANGLE, value, &own_angle),
        Ok(None) => {}
        Err(words) => super::refuse(app, words),
    }
}
