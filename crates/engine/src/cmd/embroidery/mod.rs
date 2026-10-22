//! Machine embroidery in the document: the settings an object carries for it and the document's
//! own (`embroidery.setup`: the thread palette), the walk that plans the document's art as
//! stitches, and the threads the plan sews it in (`embroidery.threads`). The machine files
//! themselves are read and written in `cmd::fileio::embroidery` (`document.open`,
//! `document.export`).
//!
//! The parts: [`options`] (the export's options, and the stitch planner's parameters they give an
//! object), [`digitize`] (the document's art planned as a design: its [`plan::DesignPlan`]),
//! [`unit`] (one sewing unit's stitches from what it is planned from), [`cache`] (each unit's
//! stitches kept between plans), [`service`] (each open document's planning, with what it keeps),
//! [`plan`] (the plan: sewing units, thread blocks, stitch numbers), [`sequence`] (the order the
//! blocks are sewn in), [`measure`] (a plan's records walked in sewing order), [`stats`] (a
//! design's figures: counts, size, thread, time), [`order`] (the sewing order as
//! `embroidery.sequence` lists it) and [`check`] (the design checks, each rule with its source).

mod cache;
mod check;
pub(crate) mod digitize;
mod measure;
pub(crate) mod options;
mod order;
pub mod plan;
mod sequence;
pub(crate) mod service;
mod stats;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_check;
#[cfg(test)]
mod tests_contract;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_service;
#[cfg(test)]
mod tests_stats;
mod unit;

use std::sync::Arc;

use serde_json::{Map, Value, json};
use vectorcraft_color::Color;
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::{Embroidery, ThreadPalette};
use vectorcraft_stitch::format::Format as StitchFormat;
use vectorcraft_stitch::thread::{MISMATCH_DELTA_E, Palette};

use self::stats::Page;
use super::*;

pub use options::Options;
pub(crate) use service::Services;
pub(crate) use unit::PlanError;

/// A document's stitch planning, to hand to another thread: the planning service of the document
/// it was taken from (its kept units and last plan, shared with the commands) and the snapshot to
/// plan. Planning is `Send`: the app's stitch preview can plan on a worker while commands plan on
/// theirs, and a plan is cancelled as a command is ([`crate::cancel`]).
#[derive(Clone)]
pub struct Planner {
    service: Arc<service::PlanService>,
    doc: Arc<Document>,
}

impl Planner {
    /// The snapshot's plan with `opts`: the last plan when it was of this snapshot with these
    /// options, else planned, each unit's stitches reused where nothing it is planned from changed.
    pub fn plan(&self, opts: &Options) -> Result<Arc<plan::DesignPlan>> {
        self.service.plan(&self.doc, opts).map_err(PlanError::error)
    }

    /// The document snapshot it plans.
    pub fn document(&self) -> &Arc<Document> {
        &self.doc
    }
}

/// The planner is handed to other threads.
const _: () = {
    const fn sent<T: Send + Sync>() {}
    sent::<Planner>();
};

impl Session {
    /// The active document's planner: its planning service and its current snapshot.
    pub fn embroidery_planner(&mut self) -> Result<Planner> {
        let st = self.doc()?;
        let (uid, doc) = (st.uid, st.doc.clone());
        Ok(Planner { service: self.embroidery_plans.of(uid), doc })
    }

    /// The active document's stitch plan with `opts`, as the read commands plan it (the whole
    /// document): what a stitch preview draws.
    pub fn embroidery_plan(&mut self, opts: &Options) -> Result<Arc<plan::DesignPlan>> {
        self.embroidery_planner()?.plan(opts)
    }
}

/// The embroidery settings an object carries, the document's, and the threads its design is sewn
/// in.
pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "embroidery.setTrimAfter",
            "Embroidery Trim After",
            [],
            None,
            "{on?: bool (left out: on, unless every object is marked already, then off), ids?|id?} whether the thread is cut after each object (or group) in ids or the selection when the document is stitched (document.export to dst, tap, t01, exp, pes, pec, jef or vp3), however near the next object starts; off, only a jump longer than the export's trimLength is cut, and a run of stitches opened from a file loses the cut the file had after it. One undo step; saved with the document → {on, changed}",
            has_selection,
            set_trim_after
        ),
        cmd!(
            "embroidery.setup",
            "Embroidery Setup",
            [],
            None,
            "{palette?: \"pec\"|\"jef\"|null (the thread palette the design's colours are matched to, the target machine format's: Brother's 61 threads, which PES files number, or Janome's 78, which JEF files number; null unsets it, and JEF is used), settings?: {palette?} (the settings of its own reply, passed back)} change the document's embroidery settings in one undo step, saved with the document (resolved, which the reply only reports, is refused); no params → no change → {settings: {palette: as set (null when unset; one a newer version wrote, as it is)}, resolved: {palette: {value: \"pec\"|\"jef\" (the palette the stitch plan matches to), source: \"document\"|\"default\" (set by the document, or the default because it sets none this version knows)}}}",
            has_doc,
            setup
        ),
        cmd!(
            query "embroidery.settings",
            "Show Embroidery Settings",
            [],
            None,
            "{} the document's embroidery settings, read only → {settings, resolved} as embroidery.setup answers",
            has_doc,
            settings
        ),
        cmd!(
            query "embroidery.threads",
            "Embroidery Threads",
            [],
            None,
            "{stitch?|stitchStyle?, spacing?|density?, length?|stitchLength?, angle?|stitchAngle?, pull?|pullCompensation?, pullPercent?|pullCompensationPercent?, push?|pushCompensation?, expand?, narrowAsSatin?, threadWeight?, underlay?, jumpLength?, trimLength?, minStitch? (the embroidery options of document.export, as document.formats describes them, each also under its earlier name: they decide which objects share a colour block; a value out of its range is refused; null leaves one to its default), offset?: the first block listed (default 0), limit?: the most blocks listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the thread each colour block of the design is sewn in, as the PES or JEF export of the document's palette writes it (embroidery.setup): the palette's thread nearest the colour the block's art is painted with, in sewing order, or, when a neighbouring block has that one, the nearest other thread within threshold → {palette: \"pec\"|\"jef\" (the palette in use), threshold: the colour difference past which a thread is flagged, blockCount, offset, truncated: whether blocks after the listed ones are left out, blocks: [{block: from 1 in sewing order, thread: {palette, index: from 1 as the format numbers it, name, color: \"#rrggbb\", art: \"#rrggbb\" (the colour the art is painted with), deltaE: the CIEDE2000 difference between the two, to two decimals (0: painted with the thread's own colour, a swatch of its thread library), mismatch: deltaE past threshold}, unitCount: the fills, strokes and stitch runs sewn in it}], options: the embroidery options planned with, warnings}",
            has_doc,
            threads
        ),
        cmd!(
            query "embroidery.stats",
            "Embroidery Statistics",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, speed?: stitches a minute the machine sews at (60 to 1500; default 650, a home machine's default, an estimate), fabricThickness?: millimetres (0 to 5; default 0.5, an estimate: the top thread goes down through it and back up at each needle point), offset?: the first block listed (default 0), limit?: the most blocks listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the design's figures as the export sews it, totals and per block; lengths, positions and boxes are millimetres from the document's origin, y down, boxes [left, top, right, bottom] → {stitches: needle points, records: needle points, jumps and cuts (what record numbers count), jumps, trims, colorChanges, threadCount: distinct threads, blockCount, size: {width, height}, bounds, start, end: [x, y] (the first and last needle points), extents: {left, right, up, down} from the start, shortestStitch, longestStitch, longestJump, threadUsage: {topMetres, bobbinMetres, fabricThickness, estimate: what the estimate rests on}, time: {seconds, minutes, speed, slowSpeed, slowAbove, trimSeconds, includes: what the time counts, estimate: what the estimate rests on} (colour changes are counted, not timed), blocks: [{block: from 1, thread: {palette, index, name, color, art, deltaE, mismatch} as embroidery.threads gives it, stitches, records, jumps, trims, firstRecord, lastRecord: record numbers (from 1, every needle point, jump and cut taking one), topThreadMetres, bobbinThreadMetres, seconds}], offset, truncated: whether blocks after the listed ones are left out, options: the embroidery options planned with, warnings}",
            has_doc,
            stats_cmd
        ),
        cmd!(
            query "embroidery.sequence",
            "Embroidery Sewing Order",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, offset?: the first unit listed (default 0), limit?: the most units listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the design's sewing order, read only (sewing order follows stacking order: move objects with node.move to change it), its units from offset in sewing order within their blocks; positions and boxes are millimetres from the document's origin, y down, boxes [left, top, right, bottom] → {blockCount, unitCount, offset, truncated: whether units after the listed ones are left out, blocks: [{block: from 1 in sewing order, thread: {palette, index, name, color, art, deltaE, mismatch} as embroidery.threads gives it, stitches: needle points, records: needle points, jumps and cuts, firstRecord, lastRecord: record numbers (from 1, every needle point, jump and cut taking one), jumps, trims, unitCount, units: [{unit: from 1, its position in this plan, id: the object a person selects (the symbol instance for art inside a symbol), node: the object sewn, instance: the symbol instance or null, part: \"fill\"|\"stroke\"|\"literal\", partIndex: which fill or stroke, from 1 in appearance order (null for literal; id, part and partIndex tell a unit apart from one plan to the next), sewn: \"running\"|\"satin\"|\"tatami\"|\"literal\" (as the stitch option names them), columns: whether a fill's narrow parts are satin columns, rule: \"fill\"|\"wideStroke\"|\"narrowStroke\"|\"strokeFilled\"|\"forced\"|\"literal\" (the rule that chose how it is sewn), ruleValues: the values the rule turned on, why: the same as a sentence, stitches, records, firstRecord, lastRecord (null when it adds none: one needle point where the needle already is), jumps, trims, entry, exit: [x, y] (its first and last needle points), bounds}] (only the listed units), then: {colorChange: true, trim: whether the block ends with a cut} (null after the last block)}] (the blocks of the listed units), splitCount, split: [{id, blocks}] (objects whose units are sewn in more than one block; at most limit), options: the embroidery options planned with, warnings}",
            has_doc,
            sequence_cmd
        ),
        cmd!(
            query "embroidery.check",
            "Embroidery Check",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, format?: \"dst\"|\"tap\"|\"t01\"|\"exp\"|\"pes\"|\"pec\"|\"jef\"|\"vp3\" (the format whose limits apply; default the format of the document's thread palette: pes for pec and jef for jef), rules?: [id…] (the rules to run; default all of satinTooWide stitchTooLong formatStep fillStitchShort satinStitchShort runStitchShort stitchBelowNeedle stitchTiny shortStitchesInARow layers threadMismatch notSewn formatColors machineStitches machineColorChanges), limit?: the hits each rule lists (0 to 1000; default 20), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} checks the design as the export sews it, each rule with its threshold and where the threshold comes from; positions are millimetres from the document's origin, y down → {format, palette: the palette whose threads threadMismatch judges (the format's own when it numbers its threads, a PES's Brother threads matched to the art as its writer matches them; else the document's), rules: [{id, title, severity: error (the format can't hold it)|warning (quality may suffer)|info, threshold, unit: mm|stitches|units|deltaE|blocks|colorChanges (null: the rule measures nothing), measures: what each value measures, sources: [{name, url, value}] (the first is the one checked; where sources disagree each is listed; a format's limits name each format's own), hits: how many, objectCount: how many objects were hit, objects: [id…] (at most 1000), truncated: whether objects were left out, examples: [{record: record number (from 1, every needle point, jump and cut taking one), at: [x, y], value: the measure compared with the threshold, length: the stitch's own length when the value measures something else (formatStep), pieces: [fewest, most] records it is written as (formatStep), id: the object, note}] (the first limit hits)}], summary: {errors, warnings, info: each {rules: how many rules found something, hits: how many hits they found}}, notChecked: [{what, why}]}",
            has_doc,
            check_cmd
        ),
    ]
}

fn set_trim_after(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setTrimAfter";
    let o = object(p, C)?;
    let on = match o.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(C, format!("`on` {v}: true or false")))?),
    };
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(C, "select the objects whose thread is cut after them, or give ids"));
    }
    // Every id names an object, before anything changes.
    let d = &s.doc()?.doc;
    if let Some(missing) = ids.iter().find(|id| d.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    // Marked: asked for a cut after it, or a run of stitches whose file had one.
    let marked = |e: Option<&Embroidery>| e.is_some_and(|e| e.trim_after || e.stitches.as_ref().is_some_and(|r| r.trim_after));
    let on = on.unwrap_or_else(|| !ids.iter().all(|id| marked(d.node(*id).and_then(|n| n.extras().embroidery.as_deref()))));
    let changed = edit_counted(s, "Embroidery Trim After", |d| {
        let mut changed = 0;
        for id in &ids {
            let n = d.node_mut(*id).ok_or(EngineError::NoNode(*id))?;
            let mut e = n.extras().embroidery.as_deref().cloned().unwrap_or_default();
            let before = e.clone();
            e.set_trim_after(on);
            // Off: no cut after it, the one its file had included.
            if let Some(run) = e.stitches.as_mut().filter(|_| !on) {
                run.trim_after = false;
            }
            if e == before {
                continue;
            }
            n.extras_mut().embroidery = (e != Embroidery::default()).then(|| Box::new(e));
            changed += 1;
        }
        Ok(changed)
    })?;
    Ok(json!({ "on": on, "changed": changed }))
}

/// Edit the active document as `f` says, on a copy, in one undo step labelled `label` when `f`
/// changed something: `f` answers how many objects it changed, and none takes no step.
fn edit_counted(s: &mut Session, label: &str, f: impl FnOnce(&mut Document) -> Result<usize>) -> Result<usize> {
    let mut d = (*s.doc()?.doc).clone();
    let n = f(&mut d)?;
    if n > 0 {
        s.edit(label, |doc, _| {
            *doc = d;
            Ok(())
        })?;
    }
    Ok(n)
}

/// The palette the stitch plan matches a document's colours to when the document sets none, or
/// sets one this version doesn't know: Janome's JEF palette. The project owner's product choice
/// (2026-10-21), not a sourced default. Each format still writes its own palette's threads: a PES
/// export matches the art's colours to Brother's palette itself.
pub(crate) const DEFAULT_PALETTE: Palette = Palette::Jef;

/// The thread palette the document's colours are matched to: the one it sets
/// (`embroidery.setup`), else [`DEFAULT_PALETTE`].
pub(crate) fn thread_palette(d: &Document) -> Palette {
    d.embroidery.as_deref().and_then(|e| e.palette.as_ref()).and_then(ThreadPalette::id).and_then(Palette::from_id).unwrap_or(DEFAULT_PALETTE)
}

/// Where a resolved setting's value comes from: the document sets it, or it is the default.
fn source(set: bool) -> &'static str {
    if set { "document" } else { "default" }
}

/// The document's embroidery settings as `embroidery.setup` and `embroidery.settings` report them:
/// each as the document stores it (`settings`, which can be passed back to `embroidery.setup`), and
/// the value the stitch plan uses with where it comes from (`resolved`).
fn settings_json(d: &Document) -> Value {
    let stored = d.embroidery.as_deref().and_then(|e| e.palette.as_ref());
    let known = stored.and_then(ThreadPalette::id).and_then(Palette::from_id).is_some();
    json!({
        "settings": {"palette": stored.cloned().map(Value::from)},
        "resolved": {"palette": {"value": thread_palette(d).id(), "source": source(known)}},
    })
}

fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(settings_json(&s.doc()?.doc))
}

fn setup(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setup";
    let o = object(p, C)?;
    let d = &s.doc()?.doc;
    let mut new = d.embroidery.as_deref().cloned().unwrap_or_default();
    // The settings given, at the top or under `settings` (the reply's, passed back), each once.
    let mut given: Map<String, Value> = Map::new();
    for (k, v) in o.iter() {
        let more = match (k.as_str(), v) {
            ("settings", Value::Object(inner)) => inner.clone(),
            ("settings", _) => return Err(bad(C, format!("`settings` {v}: an object of settings, as the reply gives it"))),
            ("resolved", _) => return Err(bad(C, "`resolved` is read only: pass back `settings`")),
            _ => Map::from_iter([(k.clone(), v.clone())]),
        };
        for (k, v) in more {
            if given.get(&k).is_some_and(|w| *w != v) {
                return Err(bad(C, format!("`{k}` given twice, with different values")));
            }
            given.insert(k, v);
        }
    }
    for (k, v) in &given {
        match k.as_str() {
            "palette" => new.set_palette(palette_param(v, new.palette.as_ref())?),
            _ => return Err(bad(C, format!("unknown setting `{k}`: the only setting is palette"))),
        }
    }
    let new = (!new.is_empty()).then(|| Box::new(new));
    // A file may hold settings with nothing in them (`"embroidery": {}`): as good as none, so
    // asking for the settings doesn't edit the document.
    if new.as_deref() == d.embroidery.as_deref().filter(|e| !e.is_empty()) {
        return Ok(settings_json(d));
    }
    s.edit("Embroidery Setup", |d, _| {
        d.embroidery = new;
        Ok(())
    })?;
    Ok(settings_json(&s.doc()?.doc))
}

/// The palette `v` names: `pec`, `jef`, or null to unset it. The palette already set stays as it
/// is when it is passed back, even one a newer version wrote.
fn palette_param(v: &Value, current: Option<&ThreadPalette>) -> Result<Option<ThreadPalette>> {
    if v.is_null() {
        return Ok(None);
    }
    let p = ThreadPalette::from(v.clone());
    if p.id().is_none() && current != Some(&p) {
        let known: Vec<&str> = ThreadPalette::KNOWN.iter().filter_map(ThreadPalette::id).collect();
        return Err(bad("embroidery.setup", format!("palette {v}: one of {} (or null to unset it)", known.join(", "))));
    }
    Ok(Some(p))
}

/// The key the read commands take to plan the selection alone.
const SELECTED_ONLY: &str = "selectedOnly";

/// Whether params `o` ask for the selected objects alone (`selectedOnly`, default false); an
/// error naming `cmd` for a value that isn't true or false.
fn selected_only(o: &Map<String, Value>, cmd: &str) -> Result<bool> {
    match o.get(SELECTED_ONLY) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, format!("`{SELECTED_ONLY}` {v}: true or false"))),
    }
}

/// The active document's plan with `opts`, made by the document's planning service, and the
/// document it is the plan of: the document as the export prepares it, the selected objects alone
/// when `selected_only` (in their layers and groups, as `document.export {selectedOnly}` writes
/// them), and without the editing layer of an opacity mask, which no export writes. `cmd` names
/// the command in the errors.
fn planned(s: &mut Session, opts: &Options, selected_only: bool, cmd: &str) -> Result<(Arc<plan::DesignPlan>, Arc<Document>)> {
    let st = s.doc()?;
    if selected_only && st.selection.objects.is_empty() {
        return Err(bad(cmd, format!("select something to plan (or leave out `{SELECTED_ONLY}`)")));
    }
    let uid = st.uid;
    let doc = if !selected_only && matches!(st.doc.without_edit_modes(), std::borrow::Cow::Borrowed(_)) {
        // The snapshot itself, so that the commands reading it one after another plan it once.
        st.doc.clone()
    } else {
        let picked = crate::cmd::fileio::export_source(st, &json!({ SELECTED_ONLY: selected_only }))?;
        Arc::new(picked.without_edit_modes().into_owned())
    };
    let plan = s.embroidery_plans.of(uid).plan(&doc, opts).map_err(PlanError::error)?;
    Ok((plan, doc))
}

/// The keys of `p`, an object or null (none), as a map; another value is refused, naming `cmd`.
fn object<'a>(p: &'a Value, cmd: &str) -> Result<std::borrow::Cow<'a, Map<String, Value>>> {
    match p {
        Value::Null => Ok(std::borrow::Cow::Owned(Map::new())),
        Value::Object(o) => Ok(std::borrow::Cow::Borrowed(o)),
        _ => Err(bad(cmd, "params must be an object")),
    }
}

/// The embroidery options in `o`, an object of them and of the command's own keys (`own`), which
/// the options leave aside; any other key is refused.
fn options_in(o: &Map<String, Value>, own: &[&str], cmd: &str) -> Result<Options> {
    Options::from_object(o, options::Others::Refuse(own), cmd)
}

/// The keys `embroidery.stats` takes besides the embroidery options.
const STATS_KEYS: [&str; 5] = ["speed", "fabricThickness", Page::KEYS[0], Page::KEYS[1], SELECTED_ONLY];

/// The keys a read command that lists a stretch takes besides the embroidery options.
const LIST_KEYS: [&str; 3] = [Page::KEYS[0], Page::KEYS[1], SELECTED_ONLY];

fn stats_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.stats";
    let o = object(p, C)?;
    let (machine, thread) = stats::model(&o, C)?;
    let page = Page::from_params(&o, C)?;
    let opts = options_in(&o, &STATS_KEYS, C)?;
    let (plan, _) = planned(s, &opts, selected_only(&o, C)?, C)?;
    Ok(with_options(stats::stats(&plan, machine, thread, page), &opts))
}

fn sequence_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.sequence";
    let o = object(p, C)?;
    let page = Page::from_params(&o, C)?;
    let opts = options_in(&o, &LIST_KEYS, C)?;
    let (plan, _) = planned(s, &opts, selected_only(&o, C)?, C)?;
    Ok(with_options(order::sequence(&plan, page), &opts))
}

/// The keys `embroidery.check` takes besides the embroidery options.
const CHECK_KEYS: [&str; 4] = ["format", "rules", "limit", SELECTED_ONLY];

fn check_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.check";
    let o = object(p, C)?;
    let opts = options_in(&o, &CHECK_KEYS, C)?;
    let format = match o.get("format") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(StitchFormat::from_id).ok_or_else(|| {
            let ids: Vec<&str> = StitchFormat::ALL.iter().map(|f| f.id()).collect();
            bad(C, format!("format {v}: one of {}", ids.join(", ")))
        })?),
    };
    let rules: Vec<&'static check::Rule> = match o.get("rules") {
        None | Some(Value::Null) => check::RULES.iter().collect(),
        Some(Value::Array(ids)) => {
            let mut rules: Vec<&'static check::Rule> = Vec::new();
            for id in ids {
                let rule = id.as_str().and_then(check::rule).ok_or_else(|| {
                    let known: Vec<&str> = check::RULES.iter().map(|r| r.id).collect();
                    bad(C, format!("rule {id}: one of {}", known.join(", ")))
                })?;
                if !rules.iter().any(|r| r.id == rule.id) {
                    rules.push(rule);
                }
            }
            rules
        }
        Some(v) => return Err(bad(C, format!("rules {v}: a list of rule ids"))),
    };
    let limit = match o.get("limit") {
        None | Some(Value::Null) => check::LIMIT,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= check::MAX_LIMIT)
            .ok_or_else(|| bad(C, format!("limit {v}: a whole number from 0 to {}", check::MAX_LIMIT)))?,
    };
    let (plan, doc) = planned(s, &opts, selected_only(&o, C)?, C)?;
    // The format of the document's thread palette, when none is asked for.
    let format = format.unwrap_or_else(|| thread_palette(&doc).format());
    Ok(with_options(check::check(&plan, &check::Ask { rules, format, limit }), &opts))
}

/// A read command's answer `v` with the options it was planned with, under their names
/// (`options`; those left out are the planner's defaults).
fn with_options(mut v: Value, opts: &Options) -> Value {
    if let Value::Object(o) = &mut v {
        o.insert("options".into(), opts.to_json());
    }
    v
}

/// An sRGB colour as the commands give it: `#rrggbb`.
pub(crate) fn hex([r, g, b]: [u8; 3]) -> String {
    Color::rgb8(r, g, b).to_hex()
}

fn threads(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.threads";
    let o = object(p, C)?;
    let page = Page::from_params(&o, C)?;
    let opts = options_in(&o, &LIST_KEYS, C)?;
    let (plan, d) = planned(s, &opts, selected_only(&o, C)?, C)?;
    let listed = page.range(plan.blocks.len());
    let blocks: Vec<Value> = listed
        .clone()
        .filter_map(|k| Some((k, plan.blocks.get(k)?)))
        // Numbered from 1, as a list of threads shows them.
        .map(|(k, b)| json!({"block": k + 1, "thread": stats::thread_json(&b.thread), "unitCount": b.units.len()}))
        .collect();
    Ok(with_options(
        json!({
            "palette": thread_palette(&d).id(),
            "threshold": MISMATCH_DELTA_E,
            "blockCount": plan.blocks.len(),
            "offset": listed.start,
            "truncated": page.truncated(plan.blocks.len()),
            "blocks": blocks,
            "warnings": plan.warnings,
        }),
        &opts,
    ))
}
