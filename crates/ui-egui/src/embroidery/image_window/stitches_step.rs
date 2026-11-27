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

/// The outlines' settings (the engine's names).
pub(crate) const COLOR_OUTLINES: &str = "colorOutlines";
pub(crate) const BORDER: &str = "border";
pub(crate) const BORDER_WIDTH: &str = "borderWidth";
pub(crate) const OUTLINE_THREAD: &str = "outlineThread";

/// A thread's swatch beside its words, and Choose Thread…'s list: its width and the most its rows
/// take before it scrolls (points), as the colour list's.
const SWATCH: f32 = 14.0;
const PICKER_WIDTH: f32 = 260.0;
const PICKER_LIST_HEIGHT: f32 = 240.0;

/// The keys of the rule rows (ids, never words).
const THIN_LINES: &str = "thinLines";
const NARROW: &str = "narrowAsSatin";
const SPACING: &str = "spacing";

/// The decimals a width in millimetres is written to (tenths: the formats' 0.1 mm), a rows'
/// spacing (hundredths, as Document Setup writes it), and the outlines' lengths (whole millimetres:
/// they run to metres).
const MM_DECIMALS: usize = 1;
const SPACING_DECIMALS: usize = 2;
const LENGTH_DECIMALS: usize = 0;

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
    let width = ui.available_width();
    let cols = Columns::fit(ui, width, &drawn);
    let mut edits: Vec<(&'static str, Edit)> = Vec::new();
    for (o, row) in owned.iter().zip(&drawn) {
        // The fabrics' names are longer than a value: the choice takes the room its row has, as
        // Document Setup's does.
        let cols = if o.key == FABRIC { cols.wide(crate::embroidery::setup_tab::FABRIC_FIELD, width) } else { cols };
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
    outlines_section(app, ui, d, &reply, lang);
    rows::heading(ui, crate::et!("Sewing order"));
    note(ui, crate::et!("From the background to the details. The Order tab changes it after Create."));
    if DOCUMENT_FIELDS.iter().any(|k| d.fields.contains_key(*k)) {
        note(ui, crate::et!("Create sets these in the document's embroidery settings."));
    }
    // The rest of the document's settings (the fabric's values, the thread, the machine): Document
    // Setup's tab, the window coming back when it closes.
    let label = crate::panels::embroidery::command_label(app, SETTINGS);
    let tip = crate::et!("Opens the Embroidery tab of Document Setup. This window comes back when Document Setup closes.");
    if ui.button(label).on_hover_text(tip).clicked() {
        super::step_aside(app, &d.fields);
        return;
    }
    changes_section(app, ui, d);
}

/// Decimals an area change's angle is listed with: a tenth of a degree, as the inspector shows it.
const ANGLE_DECIMALS: usize = 1;

/// Step 4's list of the changes to single areas (the inspector's): a row each, what it sets and the
/// area it applies to, or why it applies to none (it is kept, applying again when its area comes
/// back: the project owner's choice, 2026-11-16, „Zachować i pokazać (polecam)”), ↺ removing it;
/// Reset All; how to make one when there is none.
fn changes_section(app: &VectorcraftApp, ui: &mut Ui, d: &mut Dialog) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let list = d.fields.get(super::inspector::CHANGES).and_then(Value::as_array).cloned().unwrap_or_default();
    rows::heading(ui, crate::et!("Area changes"));
    if list.is_empty() {
        note(ui, crate::et!("Click an area in the preview to change its stitch or its angle, or to leave it out. The rest stays automatic."));
        return;
    }
    let shown = app.embroidery.image_window.live.as_ref().and_then(|l| l.schedule.shown.as_ref());
    let states = shown.and_then(|s| s.outputs.change_states(&s.request)).unwrap_or_default();
    // The changes the result shown was made with, whose states those are, by the area each is for:
    // the list may have changed since (a change removed, or set again and moved to the end), so each
    // row takes the state of the change for its own area, not that of the change in its place.
    let mut made_with = std::collections::HashMap::new();
    for (j, m) in shown.map(|s| s.request.changes()).unwrap_or_default().iter().enumerate() {
        if let Some(area) = area_of(m) {
            made_with.entry(area).or_insert(j);
        }
    }
    let degrees = fields::format(lang, "°", ANGLE_DECIMALS);
    let mut remove = None;
    for (i, c) in list.iter().enumerate() {
        let line = c["kind"] == "line";
        // Don't Sew said as the state it leaves the area in, "not sewn" (note 09's word, the button's
        // being "Don't sew"); its angle, which sews nothing, not said.
        let left_out = c["stitch"].as_str() == Some("none");
        let what: Vec<String> = if left_out {
            vec![crate::et!("Not sewn").to_string()]
        } else {
            [c["stitch"].as_str().map(|s| vocabulary::stitch(s, line)), c["angle"].as_f64().map(|a| degrees.show(a))].into_iter().flatten().collect()
        };
        let what = what.join(" · ");
        let state = area_of(c).and_then(|area| made_with.get(&area)).and_then(|&j| states.get(j));
        ui.horizontal(|ui| {
            if rows::restore(ui, crate::et!("Remove this change"), crate::et!("Remove this change")).clicked() {
                remove = Some(i);
            }
            ui.vertical(|ui| match state {
                Some(vectorcraft_engine::cmd::embroidery::AreaChangeState::Applies(facts)) => {
                    let name = super::preview::area_name(facts);
                    let words = if left_out { fill(crate::et!("{name} · not sewn"), &[("name", &name)]) } else { format!("{name} · {what}") };
                    ui.label(egui::RichText::new(words).color(t.text));
                }
                Some(vectorcraft_engine::cmd::embroidery::AreaChangeState::Kept(why)) => {
                    ui.label(egui::RichText::new(&what).color(t.text_dim));
                    ui.label(egui::RichText::new(kept_words(why, line)).small().color(t.text_dim));
                }
                None => {
                    ui.label(egui::RichText::new(&what).color(t.text_dim));
                }
            });
        });
    }
    if let Some(i) = remove {
        let kept: Vec<Value> = list.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| c.clone()).collect();
        if kept.is_empty() {
            d.fields.remove(super::inspector::CHANGES);
        } else {
            d.fields.insert(super::inspector::CHANGES.into(), Value::Array(kept));
        }
    }
    if ui.button(crate::et!("Reset All")).clicked() {
        d.fields.remove(super::inspector::CHANGES);
    }
}

/// The area an area change, as the window keeps it and the reply echoes it, is for: its point (the
/// numbers' bits), its colour in lower case and its kind, which find the area, whatever it sets.
fn area_of(c: &Value) -> Option<([u64; 2], String, String)> {
    let at = c.get("at")?.as_array()?;
    let point = [at.first()?.as_f64()?.to_bits(), at.get(1)?.as_f64()?.to_bits()];
    Some((point, c.get("color")?.as_str()?.to_ascii_lowercase(), c.get("kind")?.as_str()?.to_string()))
}

/// Why a change to an area applies to none, by the engine's id (`areaChanges[].why`); `line`: it was
/// set on a line.
fn kept_words(why: &str, line: bool) -> &'static str {
    match why {
        "otherColor" => crate::et!("Another color lies here now"),
        "otherKind" if line => crate::et!("Its area is filled now"),
        "otherKind" => crate::et!("Its area is a line now"),
        "notSewn" => crate::et!("Its color isn't sewn"),
        _ => crate::et!("No area here now"),
    }
}

/// The command Embroidery Settings… runs elsewhere (the Object tab's): Document Setup at its
/// Embroidery tab.
const SETTINGS: &str = "embroidery.documentSetup";

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

/// The thin lines' rule: a running stitch (along their middle, as the Shapes step says), below the
/// Shapes step's width.
fn lines_row(reply: &Value, lang: Lang) -> Owned {
    let below = reply["settings"]["thinLine"].as_f64().unwrap_or(0.0);
    let mm = fields::format(lang, "mm", MM_DECIMALS).show(below);
    let source = fill(crate::et!("Shapes: below {value}"), &[("value", &mm)]);
    let label = crate::et!("Thin lines");
    let field = OwnedField::Words(crate::et!("Running stitch").to_string());
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
    let field = OwnedField::Words(fill(crate::et!("Tatami, {value}"), &[("value", &mm)]));
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

/// The outlines: a running line along each colour edge and round the design, a satin border round
/// it with its width, the thread both are sewn in (the darkest of the design's, unless one is
/// chosen), and what they came to.
fn outlines_section(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, lang: Lang) {
    let t = Tokens::get(ui.ctx());
    rows::heading(ui, crate::et!("Outlines"));
    let on = |d: &Dialog, key: &str| d.fields.get(key).and_then(Value::as_bool).or(reply["settings"][key].as_bool()).unwrap_or(false);
    let colour = on(d, COLOR_OUTLINES);
    let tip = crate::et!("A running line along each edge between two colors, once, and round the design.");
    if rows::check_with_tip(ui, crate::et!("Color outlines (running stitch)"), Some(colour), t.panel, tip) {
        d.fields.insert(COLOR_OUTLINES.into(), json!(!colour));
    }
    let border = on(d, BORDER);
    let tip = crate::et!("A satin border round the whole design, its holes too, on its edge: the design grows by half its width.");
    if rows::check_with_tip(ui, crate::et!("Design border (satin)"), Some(border), t.panel, tip) {
        d.fields.insert(BORDER.into(), json!(!border));
    }
    if border {
        width_row(app, ui, d, reply, lang);
    }
    if reply["outlined"].is_object() {
        thread_row(ui, d, reply);
        figures_note(ui, reply, lang);
    }
}

/// The border's width: a field in millimetres over the engine's range, a value out of it refused.
fn width_row(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, reply: &Value, lang: Lang) {
    let Some((lo, hi)) = super::image_step::range_of(reply, BORDER_WIDTH) else { return };
    let value = d.fields.get(BORDER_WIDTH).and_then(Value::as_f64).or(reply["settings"][BORDER_WIDTH].as_f64()).unwrap_or(lo);
    let format = fields::format(lang, "mm", MM_DECIMALS);
    let mut taken = None;
    ui.horizontal(|ui| {
        crate::widgets::field_label(ui, crate::et!("Border width"));
        taken = fields::number_field(
            ui,
            ("digitize", BORDER_WIDTH),
            Some(value),
            &format,
            &fields::Look::default(),
            super::colours_step::FIELD_WIDTH,
            &(lo..=hi),
        );
    });
    if let Some(words) = fields::take_unreadable(ui) {
        super::refuse(app, words);
    }
    if let Some(v) = taken {
        super::colours_step::take(app, d, BORDER_WIDTH, v, (lo, hi), &format);
    }
}

/// The outlines' thread, as the result shown sews them: its colour, name and number (the darkest of
/// the design's unless one is chosen, said so), ↺ back to the darkest, and Choose Thread… with the
/// palette's threads, the darkest first.
fn thread_row(ui: &mut Ui, d: &mut Dialog, reply: &Value) {
    let t = Tokens::get(ui.ctx());
    let thread = &reply["outlined"]["thread"];
    let palette = thread["palette"].as_str().unwrap_or_default().to_string();
    let (name, index) = (thread["name"].as_str().unwrap_or_default(), thread["index"].as_u64().unwrap_or(0));
    let chosen = d.fields.get(OUTLINE_THREAD).is_some_and(|v| !v.is_null());
    let mut words = super::colour_list::thread_words(name, &palette, index);
    if !chosen {
        words = fill(crate::et!("{value} · darkest of the design"), &[("value", &words)]);
    }
    let mut picked = None;
    // The thread's words wrap within the column, its buttons on a line of their own, so that a long
    // name never widens the window.
    ui.horizontal_wrapped(|ui| {
        crate::widgets::field_label(ui, crate::et!("Outlines' thread"));
        if let Some(c) = thread["color"].as_str().and_then(super::image_step::parse) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(SWATCH, SWATCH), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), SWATCH / 2.0, c);
            ui.painter().circle_stroke(rect.center(), SWATCH / 2.0, egui::Stroke::new(1.0, t.border));
        }
        ui.label(&words);
    });
    ui.horizontal(|ui| {
        let back = crate::et!("Back to the darkest thread");
        if chosen && rows::restore(ui, back, back).clicked() {
            d.fields.remove(OUTLINE_THREAD);
        }
        let button = ui.button(crate::et!("Choose Thread…"));
        picked = picker(ui, &button, &palette);
    });
    if let Some(i) = picked {
        d.fields.insert(OUTLINE_THREAD.into(), json!({"palette": palette, "index": i}));
    }
}

/// Choose Thread…'s list for the outlines, below `button` once it is clicked and until a thread is
/// picked or a click lands outside: a search field and `palette`'s threads, the darkest first → the
/// thread picked (its number).
fn picker(ui: &mut Ui, button: &egui::Response, palette: &str) -> Option<usize> {
    let id = ui.make_persistent_id("digitize-outline-thread");
    let (mut open, mut search) = ui.data(|m| m.get_temp::<(bool, String)>(id)).unwrap_or_default();
    let opening = button.clicked() && !open;
    open |= button.clicked();
    let mut picked = None;
    egui::Popup::from_response(button)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .accessible_name(crate::et!("Choose Thread…"))
        .show(|ui| {
            ui.set_width(PICKER_WIDTH);
            let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text(crate::et!("Name or number")).desired_width(PICKER_WIDTH));
            if opening {
                field.request_focus();
            }
            // The darkest first (by CIE L*, as the outlines take the darkest of a design's
            // threads), each with its colour, as the colour list's Choose Thread… shows them.
            let darkest = vectorcraft_engine::cmd::embroidery::threads_darkest_first(palette);
            let threads = super::colour_list::searched(darkest, &search);
            let (t, lang) = (Tokens::get(ui.ctx()), crate::i18n::current());
            egui::ScrollArea::vertical().max_height(PICKER_LIST_HEIGHT).show(ui, |ui| {
                for thread in &threads {
                    if super::colour_list::picker_row(ui, thread, None, palette, lang, &t).clicked() {
                        picked = Some(thread.index);
                    }
                }
            });
        });
    let keep = open && picked.is_none();
    ui.data_mut(|m| m.insert_temp(id, (keep, if keep { search } else { String::new() })));
    picked
}

/// What the outlines came to, under their rows: the colour outlines' lines and length, the
/// border's length.
fn figures_note(ui: &mut Ui, reply: &Value, lang: Lang) {
    let o = &reply["outlined"];
    let mm = |v: &Value| fields::format(lang, "mm", LENGTH_DECIMALS).show(v["length"].as_f64().unwrap_or(0.0));
    if o["colorOutlines"].is_object() {
        let lines = o["colorOutlines"]["lines"].as_u64().unwrap_or(0);
        let words = crate::etn!(lines, "Color outlines: {count} line, {length}", "Color outlines: {count} lines, {length}");
        note(ui, &fill(&words, &[("length", &mm(&o["colorOutlines"]))]));
    }
    if o["border"].is_object() {
        note(ui, &fill(crate::et!("Border: {length}"), &[("length", &mm(&o["border"]))]));
    }
}
