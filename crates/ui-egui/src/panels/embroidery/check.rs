//! The Check tab: the design checked as the export sews it (`embroidery.check`), rule by rule.
//!
//! - **Problems first**: the rules that found errors, then warnings, then notes, then those that
//!   passed, each group in the engine's order ([`ordered`]).
//! - **Words from ids and numbers.** A rule's title is the interface's own sentence for its id, with
//!   its threshold (for a format's limits, the format's own) written as the language writes numbers;
//!   the engine's English title shows only for a rule this version doesn't know. Under it, what was
//!   found (how many stitches, objects, spots…), or, for a rule that passed, the extreme it measured
//!   ("Longest: 4.6 mm"). The plan's warnings for art that isn't sewn are listed as they are said.
//! - **Opened**, a rule lists its threshold's published sources (the first is the one checked; each a
//!   link) and the first hits.
//! - **The format** whose limits apply is the document's thread palette's by default, as the
//!   command picks it; another can be chosen for the document, for the session.
//! - The check is asked for only while the tab is open ([`super::queries`]); a newer plan's check
//!   replaces the last, which stays dimmed until then.

use std::collections::{HashMap, HashSet};

use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::check_formats;

use crate::VectorcraftApp;
use crate::embroidery::marks::{self, MARK_SIZE, Mark};
use crate::embroidery::numbers;
use crate::embroidery::plan_worker::Query;
use crate::embroidery::text::fill;
use crate::i18n::Lang;
use crate::theme::Tokens;

/// The hits asked for per rule: enough for the canvas's marks and for stepping through them, within
/// the engine's [`vectorcraft_engine::cmd::embroidery::MAX_LIMIT`]. An estimate.
pub const CHECK_LIST_LIMIT: usize = 100;
/// The hits an opened rule lists before "and … more". An estimate: a screenful of a panel.
pub const HITS_SHOWN: usize = 10;
/// The decimals a measure is written to: the hundredth of a millimetre the reply gives.
const PLACES: usize = 2;
/// Room around a rule's words, between its mark and its words, and for the opening triangle,
/// points. Estimates fitted to the Sewing order's rows.
const ROW_PAD: f32 = 6.0;
const MARK_GAP: f32 = 8.0;
const CHEVRON: f32 = 12.0;
/// The sizes of the rows' words, points: the interface's text and its small text.
const TEXT: f32 = 12.0;
const SMALL: f32 = 11.0;
/// How much a result checked on an older plan is dimmed, as the Sewing order dims its rows.
const STALE_INK: f32 = 0.6;

/// The rules this version words itself, in the engine's order: any other the engine lists shows
/// its own English title.
pub const WORDED: [&str; 15] = [
    "satinTooWide",
    "stitchTooLong",
    "formatStep",
    "fillStitchShort",
    "satinStitchShort",
    "runStitchShort",
    "stitchBelowNeedle",
    "stitchTiny",
    "shortStitchesInARow",
    "layers",
    "threadMismatch",
    "notSewn",
    "formatColors",
    "machineStitches",
    "machineColorChanges",
];

/// The tab's own state: the format chosen per document and the rules opened.
#[derive(Default)]
pub struct CheckState {
    /// The format chosen for each document (by uid), for the session: an id of [`check_formats`].
    pub(crate) format: HashMap<u64, &'static str>,
    /// The rules opened, by id.
    pub(crate) open: HashSet<String>,
    /// Since when the check asked for has been waiting (seconds, the frame's clock), for the busy
    /// mark ([`super::SHOW_BUSY_AFTER_SECS`]).
    waiting_since: Option<f64>,
}

/// What the tab asks the planning thread for: the check, with the hits it lists and the format
/// chosen for document `uid` (none: the command's own choice, the palette's).
pub fn query(state: &CheckState, uid: u64) -> Query {
    let mut params = json!({ "limit": CHECK_LIST_LIMIT });
    if let Some(format) = state.format.get(&uid) {
        params["format"] = json!(format);
    }
    Query { cmd: "embroidery.check", params }
}

/// What the tab shows: the check's reply for the plan shown (and whether it is that plan's), and
/// whether the plan itself is out of date.
pub struct Shown<'a> {
    pub uid: u64,
    pub reply: Option<(&'a Result<Value, String>, bool)>,
    pub stale: bool,
}

/// The reply's rules, problems first: errors, warnings and notes with hits, then those that passed,
/// each group in the reply's order.
pub fn ordered(reply: &Value) -> Vec<&Value> {
    let mut rules: Vec<&Value> = reply["rules"].as_array().map(|r| r.iter().filter(|r| r["id"].is_string()).collect()).unwrap_or_default();
    rules.sort_by_key(|r| mark_of(r).rank());
    rules
}

fn mark_of(rule: &Value) -> Mark {
    Mark::of(rule["severity"].as_str().unwrap_or_default(), rule["hits"].as_u64().unwrap_or(0))
}

/// The name of the format whose limits a reply checked, as the export lists it.
fn format_label(id: &str) -> &'static str {
    check_formats().into_iter().find(|(f, _)| *f == id).map_or("", |(_, label)| label)
}

/// `v` as `lang` writes a measure.
fn measure(lang: Lang, v: &Value) -> Option<String> {
    v.as_f64().map(|v| numbers::decimal(lang, v, PLACES))
}

/// Rule `rule`'s title in the interface's language: its sentence, with its threshold and the
/// format checked for (`format`, its name).
pub fn title(rule: &Value, format: &str, lang: Lang) -> String {
    let id = rule["id"].as_str().unwrap_or_default();
    let hits = rule["hits"].as_u64().unwrap_or(0);
    let value = measure(lang, &rule["threshold"]);
    let template = match (id, value.is_some()) {
        ("satinTooWide", true) => crate::et!("Satin columns wider than {value} mm"),
        ("stitchTooLong", true) => crate::et!("Stitches longer than {value} mm"),
        ("formatStep", true) => crate::et!("Stitches longer than one {format} record ({value} mm)"),
        ("fillStitchShort", true) => crate::et!("Fill stitch length under {value} mm"),
        ("satinStitchShort", true) => crate::et!("Satin columns narrower than {value} mm"),
        ("runStitchShort", true) => crate::et!("Running stitch length under {value} mm"),
        ("stitchBelowNeedle", true) => crate::et!("Stitches shorter than the needle is thick ({value} mm)"),
        ("stitchTiny", true) => crate::et!("Stitches of {value} mm or less, which machines leave out"),
        ("shortStitchesInARow", true) => crate::et!("More than {value} short stitches in a row"),
        ("layers", true) => crate::et!("{value} or more layers of stitches on one spot"),
        ("threadMismatch", true) => crate::et!("Threads unlike the art's colour (over {value})"),
        ("notSewn", _) if hits == 0 => crate::et!("Everything is sewn"),
        ("notSewn", _) => crate::et!("Art that isn't sewn"),
        ("formatColors", true) => crate::et!("More colour blocks than {format} holds ({value})"),
        ("formatColors", false) => crate::et!("{format} holds any number of colour blocks"),
        ("machineStitches", true) => crate::et!("More stitches than a machine shows ({value})"),
        ("machineColorChanges", true) => crate::et!("More thread changes than a machine shows ({value})"),
        _ => return rule["title"].as_str().unwrap_or(id).to_string(),
    };
    // A colour difference is written with its symbol, which no language translates.
    let value = match (id, value) {
        ("threadMismatch", Some(v)) => Some(delta_e(&v)),
        (_, v) => v,
    };
    fill(template, &[("value", value.as_deref().unwrap_or_default()), ("format", format)])
}

/// A colour difference as written: "ΔE 10".
fn delta_e(value: &str) -> String {
    format!("ΔE {value}")
}

/// What rule `rule` found, in a line: how many of what, and in how many objects; for a rule that
/// passed, the extreme it measured (or that it found nothing).
pub fn found(rule: &Value, lang: Lang) -> String {
    let id = rule["id"].as_str().unwrap_or_default();
    let hits = rule["hits"].as_u64().unwrap_or(0);
    let object_count = rule["objectCount"].as_u64().unwrap_or(0);
    let objects = crate::etn!(object_count, "{count} object", "{count} objects");
    let measured = &rule["measured"];
    let count = measured.as_u64().unwrap_or(0);
    // The design's own figures, against a format's or a machine's limit, found or not.
    match id {
        "formatColors" if measured.is_u64() => return crate::etn!(count, "{count} thread block", "{count} thread blocks"),
        "machineStitches" if measured.is_u64() => return crate::etn!(count, "{count} stitch", "{count} stitches"),
        "machineColorChanges" if measured.is_u64() => return crate::etn!(count, "{count} thread change", "{count} thread changes"),
        _ => {}
    }
    if hits == 0 {
        let Some(value) = measure(lang, measured) else { return crate::et!("No findings").to_string() };
        let template = match id {
            "stitchTooLong" | "formatStep" => crate::et!("Longest: {value} mm"),
            "satinTooWide" => crate::et!("Widest: {value} mm"),
            "satinStitchShort" => crate::et!("Narrowest: {value} mm"),
            "fillStitchShort" | "runStitchShort" | "stitchBelowNeedle" | "stitchTiny" => crate::et!("Shortest: {value} mm"),
            "shortStitchesInARow" => crate::et!("At most {value} in a row"),
            "layers" => crate::et!("At most {value} on one spot"),
            "threadMismatch" => crate::et!("Largest difference: {value}"),
            _ => crate::et!("No findings"),
        };
        let value = if id == "threadMismatch" { delta_e(&value) } else { value };
        return fill(template, &[("value", &value)]);
    }
    let what = match id {
        "stitchTooLong" | "formatStep" | "stitchBelowNeedle" | "stitchTiny" => crate::etn!(hits, "{count} stitch", "{count} stitches"),
        "shortStitchesInARow" => crate::etn!(hits, "{count} row", "{count} rows"),
        "layers" => crate::etn!(hits, "{count} spot", "{count} spots"),
        "threadMismatch" => crate::etn!(hits, "{count} thread", "{count} threads"),
        // Rules on objects' settings, and art left out: the objects are what was found.
        _ => return objects,
    };
    format!("{what} · {objects}")
}

/// A hit of rule `id` in a line: where it is and what it measured.
fn hit_line(app: &VectorcraftApp, doc: Option<&Document>, id: &str, hit: &Value, lang: Lang) -> Option<String> {
    let value = measure(lang, &hit["value"]).unwrap_or_default();
    let record = hit["record"].as_u64().map(|n| numbers::count(lang, n)).unwrap_or_default();
    let name = || {
        let object = hit["id"].as_u64()?;
        let shown = doc.and_then(|d| d.node(NodeId(object))).map(|n| crate::panels::layers::shown_name(app, n, lang));
        Some(shown.unwrap_or_else(|| format!("#{object}")))
    };
    let line = match id {
        "stitchTooLong" | "formatStep" | "stitchBelowNeedle" | "stitchTiny" => {
            fill(crate::et!("Stitch {record}: {value} mm"), &[("record", &record), ("value", &value)])
        }
        "shortStitchesInARow" => fill(crate::et!("From stitch {record}: {value} in a row"), &[("record", &record), ("value", &value)]),
        "layers" => fill(crate::et!("Stitch {record}"), &[("record", &record)]),
        "satinTooWide" | "fillStitchShort" | "satinStitchShort" | "runStitchShort" => {
            fill(crate::et!("{name}: {value} mm"), &[("name", &name()?), ("value", &value)])
        }
        "threadMismatch" => {
            let thread = &hit["thread"];
            let words = format!(
                "{} {} {}",
                thread["name"].as_str().unwrap_or_default(),
                thread["palette"].as_str().unwrap_or_default().to_uppercase(),
                thread["index"].as_u64().unwrap_or(0)
            );
            let block = numbers::count(lang, hit["block"].as_u64().unwrap_or(0));
            fill(crate::et!("Block {block}: {thread}, {value}"), &[("block", &block), ("thread", words.trim()), ("value", &delta_e(&value))])
        }
        _ => return None,
    };
    Some(line)
}

/// A severity's name, said with each rule to a screen reader.
fn severity_name(mark: Mark) -> &'static str {
    match mark {
        Mark::Error => crate::et!("Error"),
        Mark::Warning => crate::et!("Warning"),
        Mark::Info => crate::et!("Note"),
        Mark::Passed => crate::et!("Passed"),
    }
}

/// The header's summary: how many rules found errors, warnings and notes; that nothing was found.
fn summary(reply: &Value) -> String {
    let [errors, warnings, notes] = ["errors", "warnings", "info"].map(|k| reply["summary"][k]["rules"].as_u64().unwrap_or(0));
    let parts: Vec<String> = [
        (errors, crate::etn!(errors, "{count} error", "{count} errors")),
        (warnings, crate::etn!(warnings, "{count} warning", "{count} warnings")),
        (notes, crate::etn!(notes, "{count} note", "{count} notes")),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(_, words)| words)
    .collect();
    if parts.is_empty() { crate::et!("No problems found").to_string() } else { parts.join(" · ") }
}

/// What a check that doesn't run is, by its id.
fn not_checked_name(item: &Value) -> String {
    match item["id"].as_str().unwrap_or_default() {
        "density" => crate::et!("stitch density").to_string(),
        "penetrations" => crate::et!("needle penetrations in an area").to_string(),
        "ties" => crate::et!("tie stitches").to_string(),
        _ => item["what"].as_str().unwrap_or_default().to_string(),
    }
}

/// What a click in the tab does, once the tab is drawn.
enum Act {
    Toggle(String),
    Format(&'static str),
    Open(String),
}

/// The tab.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, shown: Shown) {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let Some((result, current)) = shown.reply else {
        // Asked for, not answered yet (a format chosen, the tab just opened): blank at first, so
        // that a quick check never flashes a busy mark.
        let since = *app.embroidery.check.waiting_since.get_or_insert(now);
        if now - since >= super::SHOW_BUSY_AFTER_SECS {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(11.0).color(t.accent));
                ui.label(egui::RichText::new(crate::et!("Checking…")).size(SMALL).color(t.text_dim));
            });
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(super::SHOW_BUSY_AFTER_SECS));
        return;
    };
    app.embroidery.check.waiting_since = None;
    let reply = match result {
        Ok(reply) => reply,
        Err(e) => {
            ui.label(egui::RichText::new(crate::et!("Can't check the design:")).color(t.error));
            ui.label(egui::RichText::new(crate::i18n::msg(e)).size(SMALL).color(t.text));
            return;
        }
    };
    let stale = shown.stale || !current;
    let format = reply["format"].as_str().unwrap_or_default();
    let mut acts = vec![];
    // The summary, then the format whose limits apply, each on a line of its own: a narrow panel
    // has no room for both.
    ui.label(egui::RichText::new(summary(reply)).size(SMALL).color(if stale { t.text_dim } else { t.text }));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::et!("Checked for")).size(SMALL).color(t.text_dim));
        let chosen = app.embroidery.check.format.get(&shown.uid).copied().unwrap_or(format);
        egui::ComboBox::from_id_salt(("embroidery-check-format", shown.uid)).selected_text(format_label(chosen)).show_ui(ui, |ui| {
            for (id, label) in check_formats() {
                if ui.selectable_label(id == chosen, label).clicked() {
                    acts.push(Act::Format(id));
                }
            }
        });
    });
    ui.add_space(4.0);
    let doc = app.session.doc().ok().map(|st| st.doc.clone());
    egui::ScrollArea::vertical().id_salt("embroidery-check").auto_shrink([false, false]).show(ui, |ui| {
        for rule in ordered(reply) {
            row(app, ui, rule, format_label(format), stale, doc.as_deref(), &mut acts);
        }
        ui.add_space(8.0);
        not_checked(ui, reply, &t);
    });
    for act in acts {
        match act {
            Act::Toggle(id) => {
                if !app.embroidery.check.open.remove(&id) {
                    app.embroidery.check.open.insert(id);
                }
            }
            Act::Format(id) => {
                app.embroidery.check.format.insert(shown.uid, id);
            }
            Act::Open(url) => app.open_url(&url),
        }
    }
}

/// A rule's row: its mark, title and findings; opened, its sources and first hits.
fn row(app: &VectorcraftApp, ui: &mut Ui, rule: &Value, format: &str, stale: bool, doc: Option<&Document>, acts: &mut Vec<Act>) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let ink = |c: Color32| if stale { c.gamma_multiply(STALE_INK) } else { c };
    let id = rule["id"].as_str().unwrap_or_default().to_string();
    let mark = mark_of(rule);
    let open = app.embroidery.check.open.contains(&id);
    let width = ui.available_width();
    let left = ROW_PAD + MARK_SIZE + MARK_GAP;
    let wrap = (width - left - CHEVRON - 2.0 * ROW_PAD).max(MARK_SIZE);
    let heading = title(rule, format, lang);
    let mut lines = vec![found(rule, lang)];
    // The art left out, as the plan says it.
    if id == "notSewn" {
        lines.extend(rule["examples"].as_array().into_iter().flatten().filter_map(|h| h["note"].as_str()).map(crate::i18n::msg));
    }
    let below = lines.join("\n");
    let title_g = ui.painter().layout(heading.clone(), FontId::proportional(TEXT), ink(t.text), wrap);
    let below_g = ui.painter().layout(below.clone(), FontId::proportional(SMALL), ink(t.text_dim), wrap);
    let height = 2.0 * ROW_PAD + title_g.rect.height() + 2.0 + below_g.rect.height();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 0.0, t.hover);
    }
    let first_line = title_g.rows.first().map_or(TEXT, |r| r.rect().height());
    marks::paint(&p, pos2(rect.left() + ROW_PAD + MARK_SIZE / 2.0, rect.top() + ROW_PAD + first_line / 2.0), MARK_SIZE, mark, &t);
    let top = rect.top() + ROW_PAD;
    p.galley(pos2(rect.left() + left, top), title_g.clone(), ink(t.text));
    p.galley(pos2(rect.left() + left, top + title_g.rect.height() + 2.0), below_g, ink(t.text_dim));
    let chevron = Rect::from_center_size(pos2(rect.right() - ROW_PAD - CHEVRON / 2.0, top + first_line / 2.0), vec2(CHEVRON, CHEVRON));
    crate::icons::paint(ui, if open { "chevron-down" } else { "chevron-right" }, chevron, ink(t.text_dim));
    let said = format!("{}: {heading}. {below}", severity_name(mark));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, &said));
    if resp.clicked() {
        acts.push(Act::Toggle(id.clone()));
    }
    if open {
        details(app, ui, rule, &id, doc, stale, acts);
    }
}

/// An opened rule: where its threshold comes from, and its first hits.
fn details(app: &VectorcraftApp, ui: &mut Ui, rule: &Value, id: &str, doc: Option<&Document>, stale: bool, acts: &mut Vec<Act>) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let ink = |c: Color32| if stale { c.gamma_multiply(STALE_INK) } else { c };
    let indent = ROW_PAD + MARK_SIZE + MARK_GAP;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            // The published sources, each a link: the first is the one checked. A source with no
            // address (the project's own choice, the plan's warnings) is no citation to show.
            let sources = rule["sources"].as_array().into_iter().flatten().filter(|s| s["url"].as_str().is_some_and(|u| !u.is_empty()));
            for (k, source) in sources.enumerate() {
                let name = source["name"].as_str().unwrap_or_default();
                let words =
                    if k == 0 { fill(crate::et!("Source: {name}"), &[("name", name)]) } else { fill(crate::et!("Also: {name}"), &[("name", name)]) };
                let url = source["url"].as_str().unwrap_or_default();
                // What the source says, as it says it.
                let says = source["value"].as_str().unwrap_or_default();
                let link = ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL).color(ink(t.accent))).sense(Sense::click()));
                let link = link.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{says}\n{url}"));
                if link.clicked() {
                    acts.push(Act::Open(url.to_string()));
                }
            }
            if id == "notSewn" {
                return;
            }
            let hits = rule["examples"].as_array().map(Vec::as_slice).unwrap_or_default();
            let lines: Vec<String> = hits.iter().take(HITS_SHOWN).filter_map(|h| hit_line(app, doc, id, h, lang)).collect();
            for line in &lines {
                ui.label(egui::RichText::new(line).size(SMALL).color(ink(t.text)));
            }
            let total = rule["hits"].as_u64().unwrap_or(0);
            let rest = total.saturating_sub(lines.len() as u64);
            if !lines.is_empty() && rest > 0 {
                ui.label(egui::RichText::new(crate::etn!(rest, "and {count} more", "and {count} more")).size(SMALL).color(ink(t.text_dim)));
            }
        });
    });
    ui.add_space(4.0);
}

/// What isn't checked, and why.
fn not_checked(ui: &mut Ui, reply: &Value, t: &Tokens) {
    let names: Vec<String> = reply["notChecked"].as_array().into_iter().flatten().map(not_checked_name).filter(|n| !n.is_empty()).collect();
    if names.is_empty() {
        return;
    }
    let words = fill(crate::et!("Not checked: {list}"), &[("list", &names.join(", "))]);
    ui.add(egui::Label::new(egui::RichText::new(words).size(SMALL).color(t.text_dim)).wrap())
        .on_hover_text(crate::et!("No source gives a number to check these against."));
}

#[cfg(test)]
mod tests;
