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
//! - **Show** selects a rule's objects (or one hit's) and zooms to its hits, or to the objects when
//!   it has none to point at; their stitches stand out on the canvas for a moment. What was shown
//!   last is the tab's focus, which Next and Previous Finding step through (`embroidery.nextHit`,
//!   `embroidery.previousHit`), never on a check of an older plan.
//! - Under the rules, the stitch lengths' chart ([`super::lengths`]).
//! - The check is asked for only while the tab is open ([`super::queries`]); a newer plan's check
//!   replaces the last, which stays dimmed until then.

use std::collections::{HashMap, HashSet};

use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::{check_formats, mm_to_pt};
use vectorcraft_geom::{Point, Rect as DocRect};

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
/// How long Show stands the stitches it shows out on the canvas, seconds: only for a moment (the
/// project owner's choice). An estimate: long enough to find them, short enough not to linger.
pub const SHOW_EMPHASIS_SECS: f64 = 1.5;
/// The room Show leaves round what it zooms to, and the smallest box it zooms to, millimetres:
/// estimates, so that one stitch isn't zoomed to the zoom's limit.
pub const SHOW_MARGIN_MM: f64 = 3.0;
pub const SHOW_MIN_BOX_MM: f64 = 10.0;

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
    /// What Show showed last.
    pub(crate) focus: Option<Focus>,
    /// The pass the tab was last drawn in (`egui::Context::cumulative_pass_nr`): the canvas marks
    /// the findings only while the tab shows.
    pub(crate) drawn_pass: Option<u64>,
    /// View › Stitch Preview Options › Show Check Marks turned off, for the session.
    pub(crate) hide_marks: bool,
}

/// What Show showed last: a rule's findings, or one of its hits (by its place in the rule's listed
/// hits), in a document; the objects whose stitches it stands out, and since when (the frame's
/// clock; set by the first frame that draws it); their units in the plan shown (the scene's tags),
/// found once for a plan's generation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Focus {
    pub(crate) uid: u64,
    pub(crate) rule: String,
    pub(crate) hit: Option<usize>,
    pub(crate) objects: Vec<u64>,
    pub(crate) since: Option<f64>,
    pub(crate) units: Option<(u64, std::sync::Arc<Vec<u32>>)>,
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
    pub reply: Option<(&'a Result<std::sync::Arc<Value>, String>, bool)>,
    /// The design's figures (`embroidery.stats`), for the stitch-length chart.
    pub stats: Option<&'a Value>,
    pub stale: bool,
}

/// The reply's rules, problems first: errors, warnings and notes with hits, then those that passed,
/// each group in the reply's order.
pub fn ordered(reply: &Value) -> Vec<&Value> {
    let mut rules: Vec<&Value> = reply["rules"].as_array().map(|r| r.iter().filter(|r| r["id"].is_string()).collect()).unwrap_or_default();
    rules.sort_by_key(|r| mark_of(r).rank());
    rules
}

pub(crate) fn mark_of(rule: &Value) -> Mark {
    Mark::of(rule["severity"].as_str().unwrap_or_default(), rule["hits"].as_u64().unwrap_or(0))
}

/// The name of the format whose limits a reply checked, as the export lists it.
pub(crate) fn format_label(id: &str) -> &'static str {
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
pub(crate) fn hit_line(app: &VectorcraftApp, doc: Option<&Document>, id: &str, hit: &Value, lang: Lang) -> Option<String> {
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
pub(crate) fn severity_name(mark: Mark) -> &'static str {
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
    Show(String, Option<usize>),
    Step(bool),
}

/// A rule's listed hits.
pub(crate) fn listed(rule: &Value) -> &[Value] {
    rule["examples"].as_array().map(Vec::as_slice).unwrap_or_default()
}

/// The rule `id` of a reply.
pub(crate) fn rule_of<'a>(reply: &'a Value, id: &str) -> Option<&'a Value> {
    reply["rules"].as_array()?.iter().find(|r| r["id"] == id)
}

/// Where a hit is, millimetres: where its stitch starts and ends, or its one point.
pub(crate) fn hit_points(hit: &Value) -> Vec<Point> {
    let point = |v: &Value| Some(Point::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?)).filter(|p| p.x.is_finite() && p.y.is_finite());
    [point(&hit["from"]), point(&hit["at"])].into_iter().flatten().collect()
}

/// The objects a hit is about: its own, or those whose art a `notSewn` hit left out.
fn hit_objects(hit: &Value) -> Vec<u64> {
    match hit["objects"].as_array() {
        Some(ids) => ids.iter().filter_map(Value::as_u64).collect(),
        None => hit["id"].as_u64().into_iter().collect(),
    }
}

/// Whether Show has something to show for `rule`: hits, with objects or points.
fn showable(rule: &Value) -> bool {
    rule["hits"].as_u64().unwrap_or(0) > 0
        && (rule["objects"].as_array().is_some_and(|o| !o.is_empty())
            || listed(rule).iter().any(|h| !hit_points(h).is_empty() || !hit_objects(h).is_empty()))
}

/// Show rule `id`'s findings in document `uid` (`hit`: one of them): select their objects (those
/// still in the document), zoom to their hits (or to the objects when they point at none), and
/// focus them: their stitches stand out for a moment, and Next and Previous Finding go on from
/// them.
pub(crate) fn show_findings(app: &mut VectorcraftApp, uid: u64, reply: &Value, id: &str, hit: Option<usize>) -> Result<Value, String> {
    let rule = rule_of(reply, id).ok_or("no such rule in the check")?;
    let hits = listed(rule);
    let shown: Vec<&Value> = match hit {
        Some(k) => vec![hits.get(k).ok_or("no such finding in the check")?],
        None => hits.iter().collect(),
    };
    let objects: Vec<u64> = match hit {
        Some(_) => shown.iter().flat_map(|h| hit_objects(h)).collect(),
        None => rule["objects"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect(),
    };
    let doc = app.session.doc().map_err(|e| e.to_string())?.doc.clone();
    let present: Vec<u64> = objects.iter().copied().filter(|o| doc.node(NodeId(*o)).is_some()).collect();
    // Where to zoom, in document points: the hits, else the objects.
    let points: Vec<Point> = shown.iter().flat_map(|h| hit_points(h)).map(|p| Point::new(mm_to_pt(p.x), mm_to_pt(p.y))).collect();
    let around = match points.split_first() {
        Some((first, rest)) => Some(rest.iter().fold(DocRect::from_points(*first, *first), |r, p| r.union_pt(*p))),
        None => present.iter().filter_map(|o| doc.node(NodeId(*o)).and_then(|n| n.geometric_bounds())).reduce(|a, b| a.union(b)),
    };
    if !present.is_empty() {
        app.run("select.set", json!({ "ids": present }))?;
    }
    if let Some(r) = around {
        let margin = mm_to_pt(SHOW_MARGIN_MM);
        let least = mm_to_pt(SHOW_MIN_BOX_MM);
        let r = r.inflate(margin, margin);
        let (w, h) = (r.width().max(least), r.height().max(least));
        let c = r.center();
        let rect = [c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0];
        app.run("view.zoomTo", json!({ "rect": rect }))?;
    }
    app.embroidery.check.focus = Some(Focus { uid, rule: id.to_string(), hit, objects: present, since: None, units: None });
    Ok(json!({ "rule": id, "hit": hit }))
}

/// `embroidery.showFinding {rule, hit?}`: Show from the Check tab, of rule `rule`'s findings, or of
/// its listed hit `hit` (from 0), on the active document's check as the tab shows it → `{rule, hit}`.
pub(crate) fn show_finding_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    super::only_keys(p, &["rule", "hit"])?;
    let rule = p.get("rule").and_then(Value::as_str).ok_or("`rule` must be a check rule's id")?.to_string();
    let hit = match p.get("hit") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().and_then(|k| usize::try_from(k).ok()).ok_or("`hit` must be a listed hit's place, from 0")?),
    };
    let (uid, reply) = current_check(app)
        .map(|(uid, r)| (uid, r.clone()))
        .ok_or("no check of the document as it is: show the Embroidery panel's Check tab and wait for the stitches to be planned")?;
    show_findings(app, uid, &reply, &rule, hit)
}

/// The active document's check as shown, when it is the plan's own and the plan is of the document
/// as it is (no change since, even one no frame has drawn yet): the check that Next and Previous
/// Finding step through and the canvas marks.
pub(crate) fn current_check(app: &VectorcraftApp) -> Option<(u64, &Value)> {
    let st = app.session.active()?;
    let preview = &app.embroidery.preview;
    let shown = preview.shown(st.uid)?;
    let crate::embroidery::stitch_preview::ReplyOf { result, current, .. } = shown.reply(&query(&app.embroidery.check, st.uid))?;
    let fresh = current && preview.is_fresh(st.uid) && std::sync::Arc::ptr_eq(&shown.document, &st.doc);
    fresh.then(|| result.as_ref().ok().map(|r| (st.uid, &**r))).flatten()
}

/// Whether Next and Previous Finding can step: a finding shown in the active document, and its
/// check the plan's own.
pub(crate) fn can_step(app: &VectorcraftApp) -> bool {
    let Some(focus) = &app.embroidery.check.focus else { return false };
    current_check(app).is_some_and(|(uid, reply)| uid == focus.uid && rule_of(reply, &focus.rule).is_some_and(|r| !listed(r).is_empty()))
}

/// `embroidery.nextHit` and `embroidery.previousHit`: show the next (`forward`) or previous listed
/// hit of the rule shown last, round from the last to the first → `{rule, hit}`.
pub(crate) fn step(app: &mut VectorcraftApp, forward: bool) -> Result<Value, String> {
    let focus = app.embroidery.check.focus.clone().ok_or("no finding is shown: show one in the Embroidery panel's Check tab")?;
    let (uid, reply) = current_check(app)
        .filter(|(uid, _)| *uid == focus.uid)
        .map(|(uid, r)| (uid, r.clone()))
        .ok_or("the check is out of date: wait for the stitches to be planned")?;
    let n = rule_of(&reply, &focus.rule).map(|r| listed(r).len()).unwrap_or(0);
    if n == 0 {
        return Err("no finding is shown: show one in the Embroidery panel's Check tab".into());
    }
    let next = match (focus.hit, forward) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(k), true) => (k + 1) % n,
        (Some(k), false) => (k + n - 1) % n,
    };
    show_findings(app, uid, &reply, &focus.rule, Some(next))
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
    app.embroidery.check.drawn_pass = Some(ui.ctx().cumulative_pass_nr());
    let reply = match result {
        Ok(reply) => &**reply,
        Err(e) => {
            ui.label(
                egui::RichText::new(crate::et!("Can't check the design:"))
                    .color(crate::embroidery::marks::text_colour(&t, crate::embroidery::marks::Mark::Error)),
            );
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
    // What was shown last: its index dropped when the hit is no longer listed; its stitches stood
    // out for a moment.
    if let Some(f) = app.embroidery.check.focus.as_mut().filter(|f| f.uid == shown.uid) {
        if f.hit.is_some_and(|k| rule_of(reply, &f.rule).is_none_or(|r| k >= listed(r).len())) {
            f.hit = None;
        }
        let since = *f.since.get_or_insert(now);
        if now - since < SHOW_EMPHASIS_SECS {
            if let Some(planned) = app.embroidery.preview.shown(shown.uid).map(|s| &s.planned) {
                if f.units.as_ref().is_none_or(|(g, _)| *g != planned.generation) {
                    let objects: HashSet<u64> = f.objects.iter().copied().collect();
                    let units = planned
                        .plan
                        .units
                        .iter()
                        .enumerate()
                        .filter(|(_, u)| objects.contains(&u.origin.selected().0))
                        .filter_map(|(k, _)| u32::try_from(k).ok())
                        .collect();
                    f.units = Some((planned.generation, std::sync::Arc::new(units)));
                }
                if let Some((_, units)) = f.units.as_ref().filter(|(_, u)| !u.is_empty()) {
                    app.embroidery.layer.emphasize(units.to_vec());
                }
            }
            ui.ctx().request_repaint();
        }
    }
    let focus = app.embroidery.check.focus.clone().filter(|f| f.uid == shown.uid);
    let doc = app.session.doc().ok().map(|st| st.doc.clone());
    egui::ScrollArea::vertical().id_salt("embroidery-check").auto_shrink([false, false]).show(ui, |ui| {
        for rule in ordered(reply) {
            row(app, ui, rule, format_label(format), stale, doc.as_deref(), focus.as_ref(), &mut acts);
        }
        ui.add_space(8.0);
        if let Some(stats) = shown.stats {
            let too_long = rule_of(reply, "stitchTooLong").and_then(|r| r["threshold"].as_f64());
            super::lengths::show(ui, stats, too_long, stale);
            ui.add_space(8.0);
        }
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
            Act::Show(id, hit) => {
                if let Err(e) = show_findings(app, shown.uid, reply, &id, hit) {
                    app.ui.status = crate::i18n::msg(&e);
                }
            }
            Act::Step(forward) => {
                if let Err(e) = step(app, forward) {
                    app.ui.status = crate::i18n::msg(&e);
                }
            }
        }
    }
}

/// A rule's row: its mark, title and findings; opened, its sources and first hits.
#[allow(clippy::too_many_arguments)]
fn row(
    app: &VectorcraftApp,
    ui: &mut Ui,
    rule: &Value,
    format: &str,
    stale: bool,
    doc: Option<&Document>,
    focus: Option<&Focus>,
    acts: &mut Vec<Act>,
) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let ink = |c: Color32| if stale { c.gamma_multiply(STALE_INK) } else { c };
    let id = rule["id"].as_str().unwrap_or_default().to_string();
    let mark = mark_of(rule);
    let open = app.embroidery.check.open.contains(&id);
    let focused = focus.filter(|f| f.rule == id);
    let width = ui.available_width();
    let left = ROW_PAD + MARK_SIZE + MARK_GAP;
    // Show, for a rule with something to show, at the row's right.
    let show_words = crate::et!("Show");
    let show_g = showable(rule).then(|| ui.painter().layout_no_wrap(show_words.to_string(), FontId::proportional(SMALL), ink(t.accent)));
    let show_w = show_g.as_ref().map_or(0.0, |g| g.rect.width() + ROW_PAD);
    let wrap = (width - left - CHEVRON - show_w - 2.0 * ROW_PAD).max(MARK_SIZE);
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
    if focused.is_some() {
        p.rect_filled(rect, 0.0, t.row_selected);
    } else if resp.hovered() {
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
    // Show sits over the row, so it takes the clicks on it.
    let mut showing = false;
    if let Some(g) = show_g {
        let at = Rect::from_min_size(pos2(chevron.left() - ROW_PAD - g.rect.width(), top), g.rect.size()).expand(2.0);
        let button = ui.interact(at, ui.id().with(("embroidery-check-show", &id)), Sense::click());
        let words = if button.hovered() { ink(t.accent_strong) } else { ink(t.accent) };
        ui.painter().galley(at.min + vec2(2.0, 2.0), g, words);
        button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !stale, format!("{show_words}: {heading}")));
        if button.clicked() && !stale {
            acts.push(Act::Show(id.clone(), None));
            showing = true;
        }
    }
    if resp.clicked() && !showing {
        acts.push(Act::Toggle(id.clone()));
    }
    if open {
        details(app, ui, rule, &id, doc, stale, focused, acts);
    }
}

/// An opened rule: where its threshold comes from, and its first hits.
#[allow(clippy::too_many_arguments)]
fn details(
    app: &VectorcraftApp,
    ui: &mut Ui,
    rule: &Value,
    id: &str,
    doc: Option<&Document>,
    stale: bool,
    focused: Option<&Focus>,
    acts: &mut Vec<Act>,
) {
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
                let link = ui.add(egui::Label::new(egui::RichText::new(&words).size(SMALL).color(ink(t.accent))).sense(Sense::click()));
                link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &words));
                let link = link.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{says}\n{url}"));
                if link.clicked() {
                    acts.push(Act::Open(url.to_string()));
                }
            }
            if id == "notSewn" {
                return;
            }
            let hits = listed(rule);
            // Stepping through the hits of the rule shown last.
            if let Some(f) = focused
                && !hits.is_empty()
            {
                ui.horizontal(|ui| {
                    let back = ui
                        .add_enabled(!stale, egui::Button::new(egui::RichText::new("‹").size(SMALL)).small())
                        .on_hover_text(crate::et!("Previous Finding"));
                    back.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !stale, crate::et!("Previous Finding")));
                    let place = f.hit.map_or_else(|| "–".to_string(), |k| numbers::count(lang, k as u64 + 1));
                    let of = fill(crate::et!("{index} of {count}"), &[("index", &place), ("count", &numbers::count(lang, hits.len() as u64))]);
                    ui.label(egui::RichText::new(of).size(SMALL).color(ink(t.text_dim)));
                    let on = ui
                        .add_enabled(!stale, egui::Button::new(egui::RichText::new("›").size(SMALL)).small())
                        .on_hover_text(crate::et!("Next Finding"));
                    on.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !stale, crate::et!("Next Finding")));
                    if back.clicked() {
                        acts.push(Act::Step(false));
                    }
                    if on.clicked() {
                        acts.push(Act::Step(true));
                    }
                });
            }
            let current = focused.and_then(|f| f.hit);
            let mut lines = 0u64;
            for (k, h) in hits.iter().enumerate() {
                // The first few, and the one shown when it is past them.
                if k >= HITS_SHOWN && current != Some(k) {
                    continue;
                }
                let Some(line) = hit_line(app, doc, id, h, lang) else { continue };
                lines += 1;
                let text = egui::RichText::new(&line).size(SMALL).color(ink(if current == Some(k) { t.text_strong } else { t.text }));
                let text = if current == Some(k) { text.background_color(t.row_selected) } else { text };
                // A button for a screen reader: it shows the hit.
                let hit = ui.add(egui::Label::new(text).sense(Sense::click()));
                hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !stale, &line));
                let clicked = hit.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
                if clicked && !stale {
                    acts.push(Act::Show(id.to_string(), Some(k)));
                }
            }
            let total = rule["hits"].as_u64().unwrap_or(0);
            let rest = total.saturating_sub(lines);
            if lines > 0 && rest > 0 {
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
