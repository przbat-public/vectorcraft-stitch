//! Machine embroidery in the document: the settings an object carries for it (`embroidery.
//! setTrimAfter`) and the document's own (`embroidery.setup` and `embroidery.settings`: the thread
//! palette, the start point, the label), the walk that plans the document's art as stitches, and
//! what is read from the plan: its threads (`embroidery.threads`), its figures
//! (`embroidery.stats`), its sewing order (`embroidery.sequence`) and its checks
//! (`embroidery.check`). The machine files
//! themselves are read and written in `cmd::fileio::embroidery` (`document.open`,
//! `document.export`).
//!
//! The parts: `vocabulary` (each setting's name, earlier name, file key and where it can be set),
//! [`options`] (the export's options, and the stitch planner's parameters they give an object),
//! `setup` (the document's embroidery settings: `embroidery.setup` and
//! `embroidery.settings`), [`digitize`] (the document's art planned as a design: its [`plan::DesignPlan`]),
//! [`unit`] (one sewing unit's stitches from what it is planned from), [`cache`] (each unit's
//! stitches kept between plans), [`service`] (each open document's planning, with what it keeps),
//! [`plan`] (the plan: sewing units, thread blocks, stitch numbers), [`sequence`] (the order the
//! blocks are sewn in), [`measure`] (a plan's records walked in sewing order), [`stats`] (a
//! design's figures: counts, size, thread, time), [`order`] (the sewing order as
//! `embroidery.sequence` lists it), [`check`] (the design checks, each rule with its source),
//! [`scene`] (the plan as the stitch preview draws it) and [`render`] (that drawing as a picture,
//! `embroidery.render`).

mod cache;
mod check;
pub(crate) mod derive;
pub(crate) mod digitize;
mod fabrics;
pub mod info;
mod measure;
mod objects;
pub(crate) mod options;
pub mod order;
pub mod plan;
mod printed;
pub mod render;
pub mod reorder;
mod resolve;
pub mod scene;
mod sequence;
pub(crate) mod service;
pub mod setup;
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
mod tests_derive;
#[cfg(test)]
mod tests_expanded;
#[cfg(test)]
mod tests_info;
#[cfg(test)]
mod tests_matrix;
#[cfg(test)]
mod tests_object;
#[cfg(test)]
mod tests_object_commands;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_scene;
#[cfg(test)]
mod tests_service;
#[cfg(test)]
mod tests_settings;
#[cfg(test)]
mod tests_setup;
#[cfg(test)]
mod tests_stats;
#[cfg(test)]
mod tests_threadlib;
#[cfg(test)]
mod tests_transform;
#[cfg(test)]
mod tests_view;
#[cfg(test)]
mod tests_walk;
pub mod threadlib;
mod unit;
pub mod view;
mod vocabulary;

use std::sync::Arc;

use serde_json::{Map, Value, json};
use vectorcraft_color::Color;
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::{Embroidery, ThreadPalette};
use vectorcraft_stitch::format::Format as StitchFormat;
use vectorcraft_stitch::thread::{MISMATCH_DELTA_E, Palette};

pub use self::objects::skip_reasons;
pub use self::options::{SettingField, distinct_underlays, fabric_field, part_field, setting_field};
use self::stats::Page;
use super::*;

// For the Check tab: the most hits it may ask a rule for, and the plan's millimetres as points; for
// the Sewing order tab, the most units a list may ask for.
pub use self::check::MAX_LIMIT;
pub use self::stats::MAX_LIST_LIMIT;
pub use options::Options;
pub use service::Planned;
pub(crate) use service::Services;
pub(crate) use unit::PlanError;
pub use vectorcraft_stitch::mm_to_pt;

/// The machine formats `embroidery.check {format}` checks for, as (id, the name the export lists
/// it by), in the export's order: the Check tab's choice of format.
pub fn check_formats() -> Vec<(&'static str, &'static str)> {
    StitchFormat::ALL.iter().map(|f| (f.id(), f.label())).collect()
}

/// A document's stitch planning, to hand to another thread: the planning service of the document
/// it was taken from (its kept units and last plan, shared with the commands), the snapshot it was
/// taken of, and the document planned of it, as the read commands plan it. Planning is `Send`: the
/// app's stitch preview can plan on a worker while commands plan on theirs, and a plan is
/// cancelled as a command is ([`crate::cancel`]).
#[derive(Clone)]
pub struct Planner {
    uid: u64,
    service: Arc<service::PlanService>,
    snapshot: Arc<Document>,
    doc: Arc<Document>,
}

/// What a plan is of: one open document's snapshot (held weakly, so a key never keeps a document
/// alive, and its memory can't be reused for another snapshot while the key is held), the options,
/// and the fonts' generation where the snapshot may outline type (a text clipping path is outlined
/// with the fonts installed; [`service::fonts_of`]: 0 elsewhere, so a font loaded for something
/// else never makes the plan shown out of date). Two keys
/// are equal when a plan of one is a plan of the other. Cheap to make every frame: nothing is
/// planned or copied.
#[derive(Clone, Debug)]
pub struct PlanKey {
    uid: u64,
    snapshot: std::sync::Weak<Document>,
    opts: Options,
    fonts: u64,
}

impl PlanKey {
    fn new(uid: u64, snapshot: &Arc<Document>, opts: &Options) -> Self {
        Self { uid, snapshot: Arc::downgrade(snapshot), opts: *opts, fonts: service::fonts_of(snapshot) }
    }

    /// The open document it is of ([`crate::DocState::uid`]).
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// The options planned with.
    pub fn options(&self) -> &Options {
        &self.opts
    }
}

impl PartialEq for PlanKey {
    fn eq(&self, o: &Self) -> bool {
        self.uid == o.uid && self.snapshot.ptr_eq(&o.snapshot) && self.opts == o.opts && self.fonts == o.fonts
    }
}

impl Planner {
    /// The plan with `opts`: the plan remembered when one was of this snapshot with these options
    /// (the commands' too), else planned, each unit's stitches reused where nothing it is planned
    /// from changed.
    pub fn plan(&self, opts: &Options) -> Result<Arc<plan::DesignPlan>> {
        self.planned(opts).map(|p| p.plan)
    }

    /// [`Planner::plan`], with its generation and its stitch scene ([`Planned`]).
    pub fn planned(&self, opts: &Options) -> Result<Planned> {
        self.planned_and_walked(opts).0
    }

    /// [`Planner::planned`], and whether this call walked the document for it (`false`: the plan,
    /// or the refusal, was remembered, or another caller's was waited for): only a walk's time
    /// says how long the design takes to plan.
    pub fn planned_and_walked(&self, opts: &Options) -> (Result<Planned>, bool) {
        let (planned, walked) = self.service.planned_and_walked(service::Of::whole(&self.snapshot), &self.doc, opts);
        (planned.map_err(PlanError::error), walked)
    }

    /// What the planner's plans with `opts` are of ([`PlanKey`]).
    pub fn key(&self, opts: &Options) -> PlanKey {
        PlanKey::new(self.uid, &self.snapshot, opts)
    }

    /// The document it plans: the snapshot as the export prepares it (without an opacity mask's
    /// editing layer, which no export writes). Plan positions and ids are this document's.
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
    /// The active document's planner: its planning service and its current snapshot, planned as
    /// the read commands plan the whole document.
    pub fn embroidery_planner(&mut self) -> Result<Planner> {
        let st = self.doc()?;
        let (uid, snapshot, doc) = (st.uid, st.doc.clone(), to_plan(st, false)?);
        Ok(Planner { uid, service: self.embroidery_plans.of(uid), snapshot, doc })
    }

    /// The key of the active document's plan with `opts` as it is now ([`PlanKey`]): cheap, for
    /// telling every frame whether the plan shown is of the document as it is.
    pub fn embroidery_plan_key(&self, opts: &Options) -> Option<PlanKey> {
        let st = self.active()?;
        Some(PlanKey::new(st.uid, &st.doc, opts))
    }

    /// The active document's stitch plan with `opts`, as the read commands plan it (the whole
    /// document): what a stitch preview draws.
    pub fn embroidery_plan(&mut self, opts: &Options) -> Result<Arc<plan::DesignPlan>> {
        self.embroidery_planner()?.plan(opts)
    }
}

/// The embroidery commands: the settings an object carries and the document's, and the threads,
/// figures, sewing order and checks of its design.
pub fn specs() -> Vec<CommandSpec> {
    let mut specs = vec![
        cmd!(
            "embroidery.setTrimAfter",
            "Embroidery Trim After",
            [],
            None,
            "{on?: bool (left out: on, unless every object is marked already, then off), ids?} whether the thread is cut after each object (or group) in ids or the selection when the document is stitched (document.export to dst, tap, t01, exp, pes, pec, jef or vp3), however near the next object starts; off, only a jump longer than the export's trimLength is cut, and a run of stitches opened from a file loses the cut the file had after it. One undo step; saved with the document → {on, changed}",
            has_selection,
            set_trim_after
        ),
        cmd!(
            "embroidery.set",
            "Embroidery Object Settings",
            [],
            None,
            "{ids?: [id…] (default: the selection; a group or layer stands for the paths and compound paths in it, a compound path's member for its compound path), part: \"fill\"|\"stroke\" (all of the objects' fills, or all their strokes), stitch?: \"auto\"|\"tatami\"|\"fill\"|\"satin\"|\"running\"|\"none\" (auto: chosen by the shape, even where document.export's stitch forces one; fill is tatami, and kept so; satin on a fill fills it as auto does; none: the part is not sewn), spacing?: mm (0.05 to 5), length?: mm (0.2 to 12), angle?: degrees counter-clockwise on the page, pull?: mm (0 to 2; or [left, right] of a satin, [start, end] of a fill's rows), pullPercent?: percent (0 to 50; one or two as pull), push?: mm (0 to 5; one or two: [start, end] of a satin as its path is drawn), expand?: mm (-2 to 2), narrowAsSatin?: mm (0 to 12), underlay?: true|false|\"auto\"|\"none\"|\"center\"|\"edge\"|\"edgeZigzag\"|\"edgeTatami\", splitLength?: mm (2 to 12)} set embroidery settings on the objects' fills or strokes, each as document.formats describes the option of that name: kept with the object (moved and turned with it), they beat document.export's options, the document's settings (embroidery.setup) and the defaults; null unsets one; a value out of its range is refused, not clamped, and so is document.export's earlier name of a setting, naming the one to use. Objects that take none (type, a symbol instance, whose art takes them, a run of stitches opened from a machine file…) and those without the part are skipped and listed. One undo step, none when nothing changes; saved with the document → {part, settings: {each key given: as stored, or null}, changedCount, unchangedCount, skippedCount, skipped: [{id, reason: \"literal\"|\"guide\"|\"type\"|\"image\"|\"symbolInstance\"|\"live\"|\"placed\"|\"intertwine\"|\"noPart\"}] (at most 1000)}",
            has_selection,
            objects::set
        ),
        cmd!(
            "embroidery.reset",
            "Reset Embroidery Object Settings",
            [],
            None,
            "{ids?: [id…] (default: the selection; groups and compound paths as embroidery.set takes them), part?: \"fill\"|\"stroke\" (default: both), keys?: [key…] (default: every setting embroidery.set takes; entry and exit only when named)} unset the objects' own embroidery settings, so that each comes from document.export's options, the document's settings (embroidery.setup) or the default again, as the Object tab's Restore from Fabric does; the cut after an object, and what a newer version wrote, stay. One undo step, none when nothing changes → {part: \"fill\"|\"stroke\"|null (both), keys, changedCount, unchangedCount, skippedCount, skipped: as embroidery.set lists them}",
            has_selection,
            objects::reset
        ),
        cmd!(
            "embroidery.setEntryExit",
            "Embroidery Entry and Exit",
            [],
            None,
            "{id?: object id (default: the one object selected; a compound path's member stands for its compound path), part: \"fill\"|\"stroke\", entry?: [x, y] (document points, within the coordinates a document holds)|null, exit?: [x, y]|null} where the needle goes in to sew the object's fill or stroke, and where it comes out: kept with the object, moved and turned with it. The part starts at the entry: exactly there where its stitches can start anywhere (on a fill's outline, half a row spacing inside it, where its edge run runs; anywhere on a closed line or satin ring), else at the nearer end of an open line or satin column (a fill's narrow parts sewn as satin too); the part ends at the exit as near as its stitches can reach it (a running line: an open one whose start is free at its end nearest the exit, one entered at a set point at its other end, a closed one carried on round to the exit in its own holes; a satin column turned round by a pass along its middle under its top stitching, a ring's top started at the exit; a fill's exit is kept, not yet planned toward). null unsets one (the part then starts nearest where the needle is); a point left out stays. Refused for a group, an object that takes no settings and a part it doesn't have. One undo step, none when nothing changes → {id, part, entry: [x, y] or null, exit: [x, y] or null, changedCount: 0 or 1}",
            has_doc,
            objects::set_entry_exit
        ),
        cmd!(
            "embroidery.copyFrom",
            "Apply Another Object's Embroidery Settings",
            [],
            None,
            "{source: object id (whose settings are copied), ids?: [id…] (default: the selection; groups and compound paths as embroidery.set takes them; the source itself is passed by), part?: \"fill\"|\"stroke\" (default: both), keys?: [key…] (default: every setting embroidery.set takes; entry and exit only when named)} give the objects the source's embroidery settings of those keys, set or unset as the source has them, on each of the parts they have, as the Object tab's Copy and Paste settings do (a paste: the source is read when this runs, as it is then); what a newer version wrote stays; a source value embroidery.set would refuse (a newer version's, out of range) is passed over, the objects keeping theirs. One undo step, none when nothing changes → {source, part: \"fill\"|\"stroke\"|null (both), keys, passedOver: [\"fill.stitch\"…] the values passed over, changedCount, unchangedCount, skippedCount, skipped: as embroidery.set lists them}",
            has_selection,
            objects::copy_from
        ),
        cmd!(
            "embroidery.setup",
            "Embroidery Setup",
            [],
            None,
            "{palette?: \"pec\"|\"jef\"|null (the thread palette the design's colours are matched to, the target machine format's: Brother's 61 threads, which PES files number, or Janome's 78, which JEF files number; null unsets it, and JEF is used), start?: [x, y] (where the machine starts the needle, in document points: a design opened from a machine file keeps its file's start point, and the DST, TAP, T01, EXP, PES and PEC exports start there; JEF and VP3, whose machines put the start at the hoop's centre, start at the design's centre whatever it is; null unsets it, and those exports start at the design's corner), label?: string (the design's name the export writes, as DST's and PEC's `LA:` label: a design opened from a machine file keeps its file's; null unsets it, and the export writes the document's title), fabric?: id (a preset embroidery.fabrics lists: its values copied in, the colour kept; the same preset again restores them)|{preset?: id|null (as stored, passed back; another preset's id is copied first; null: a fabric of one's own), tatami?: {pull?: mm (0 to 2), spacing?: mm for 40 wt thread (0.05 to 5; another thread scales it), length?: mm (0.2 to 12), underlay?: id}|null, satin?: {pull?, spacingPercent?: percent of the spacing a column's width picks (50 to 200), underlay?}|null, stabilizer?, topping?, needlePoint?: ids for information, thickness?: mm (0 to 5; embroidery.stats counts the thread through it), color?: \"#rrggbb\" (the stitch preview draws on it)|null} (merged value by value; null restores one to the preset's, or unsets it with no preset)|null (no fabric: the built-in values), threadWeight?: wt (10 to 150)|null, joinSameColor?: bool|null (objects of one colour join one block where nothing lies between them; default true), machine?: {jumpLength?, trimLength?, minStitch?, splitLength?: mm as document.export takes them, speed?: stitches a minute (60 to 1500; embroidery.stats times the design at it)}|null (merged value by value; null unsets one), settings?: {…} (the settings of its own reply, passed back)} change the document's embroidery settings in one undo step, saved with the document (resolved and overrides, which the reply only reports, are refused); objects with values of their own keep them; no params → no change → {settings: {palette: as set (null when unset; one a newer version wrote, as it is), start: [x, y] or null, label: string or null, fabric: {preset, tatami, satin, stabilizer, topping, needlePoint, thickness, color} as stored or null, threadWeight, joinSameColor, machine: {jumpLength, trimLength, minStitch, splitLength, speed} or null}, resolved: {palette: {value: \"pec\"|\"jef\" (the palette the stitch plan matches to), source: \"document\"|\"default\" (set by the document, or the default because it sets none this version knows)}, fabric: {preset: {id, known}|null, tatami: {pull, spacing (for the thread in use), length, underlay}, satin: {pull, spacingPercent, underlay}, thickness, stabilizer, topping, needlePoint: each {value, source, preset: the preset's value or null, changed}, color: {value, source}}, threadWeight, joinSameColor: {value, source}, machine: {jumpLength, trimLength, minStitch, splitLength, speed: each {value, source}}}, overrides: {pull, spacing, length, underlay: how many objects set it themselves (on a part the fabric gives it; symbols' art included), objectCount: how many set any of them}}",
            has_doc,
            setup::setup
        ),
        cmd!(
            query "embroidery.info",
            "Show Embroidery Object Settings",
            [],
            None,
            "{ids?: [id…] (default: the selection; groups and compound paths as embroidery.set takes them; none: an empty answer), plan?: false (true: also what the stitch plan sews of each part, planned as the read commands plan the whole document), offset?: the first object listed (default 0), limit?: the most objects listed (0 to 10000; default 1000), …the embroidery options that decide what is sewn as embroidery.threads takes them (document.export's place in where a value comes from; default none)} the objects' embroidery settings as kept, and for each part every value it is planned with, merged over the objects, without planning → {objectCount, offset, truncated, objects: [{id, kind: \"path\"|\"compound\", fills, strokes, expanded: sewn as pieces, settings: {fill, stroke: each key set, by name, or null}, kept: what it keeps that this version doesn't read, as file keys (a newer version's: branch, fill.grain, fill.points.rails; a stitch or underlay this version doesn't know: fill.stitch; embroidery: settings that are no object)}], skippedCount, skipped: [{id, reason}] as embroidery.set lists them, trimAfter: {value, setCount, mixed}, fill: {objectCount, itemCount, sewn: \"tatami\"|\"satin\"|\"running\"|\"none\"|null (when they differ), rule: the rule id embroidery.sequence names (\"strokeOutlined\" also for a stroke a brush or an effect reshapes on an object sewn as pieces)|null, reason: why nothing of it is sewn, \"off\" (set not to be)|\"unpainted\"|\"hidden\"|\"notPrinted\" (a template or non-printing layer)|null, settings: {stitch, spacing, length, angle, pull, pullPercent, push, expand, narrowAsSatin, underlay, splitLength: each {value (null when they differ; \"auto\" for a rule), source: \"object\"|\"export\"|\"document\"|\"default\"|null, section: \"fabric\"|\"machine\"|null (of the document's settings), setCount (how many objects set it, as this version reads their settings), mixed, inherited: {value, source, section} (what unsetting the objects' own gives)|null, rule: what a rule chose where value is \"auto\"|null, applies: whether the planner uses it for the stitch the part is sewn in (a running line's pull: false)|null when they differ}}, entry, exit: {value: [x, y] (document points, as embroidery.setEntryExit takes them)|null, setCount, mixed}, sewing: {unitCount, stitches, records, jumps, trims} (with plan)|null} or null (none has a fill), stroke: as fill, planned, options, warnings}",
            has_doc,
            info::info
        ),
        cmd!(
            query "embroidery.fabrics",
            "Embroidery Fabrics",
            [],
            None,
            "{} the fabric presets embroidery.setup {fabric} copies into the document (Wilcom's eight auto fabrics), each value with what it rests on → {default: id, caveat: that every per-fabric stitch value comes from one family of software, and that which of BERNINA's fabric classes a fabric is taken as is ours to say, fabrics: [{id, label, values: {tatami: {pull, spacing, length, underlay}, satin: {pull, spacingPercent, underlay} (as embroidery.setup {fabric} takes them back), stabilizer, topping, needlePoint, thickness} (null: none published, the built-in value), sources: [{key, kind: \"published\"|\"derived\"|\"estimate\", name, url, family, how: how it was worked out, why it is an estimate, or the class of BERNINA's a published value is printed for|null}], estimates: [key…]}]}",
            always,
            fabrics::fabrics
        ),
        cmd!(
            "embroidery.applyFabric",
            "Apply Embroidery Fabric to Objects",
            [],
            None,
            "{ids?: [id…] (default: the selection, else every object of the document; groups and compound paths as embroidery.set takes them), all?: true (every object of the document and its symbols' art, whatever is selected: what embroidery.settings' overrides count, Document Setup's bar; not with ids), keys?: [key…] (default: pull, spacing, length and underlay, the values a fabric sets; no others)} unset those of the objects' own settings, on fills and strokes, so that they follow the document's fabric again (Document Setup's bar). One undo step, none when nothing changes → {keys, changedCount, unchangedCount, skippedCount, skipped: as embroidery.set lists them}",
            has_doc,
            fabrics::apply_fabric
        ),
        cmd!(
            query "embroidery.settings",
            "Show Document Embroidery Settings",
            [],
            None,
            "{} the document's embroidery settings, read only → {settings, resolved, overrides} as embroidery.setup answers",
            has_doc,
            setup::settings
        ),
        cmd!(
            query "embroidery.threads",
            "Embroidery Threads",
            [],
            None,
            "{stitch?|stitchStyle?, spacing?|density?, length?|stitchLength?, angle?|stitchAngle?, pull?|pullCompensation?, pullPercent?|pullCompensationPercent?, push?|pushCompensation?, expand?, narrowAsSatin?, threadWeight?, underlay?, jumpLength?, trimLength?, minStitch?, splitLength? (the embroidery options of document.export, as document.formats describes them, each also under its earlier name: they decide which objects share a colour block; a value out of its range is refused; null leaves one to its default), offset?: the first block listed (default 0), limit?: the most blocks listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the thread each colour block of the design is sewn in, as the PES or JEF export of the document's palette writes it (embroidery.setup): the palette's thread nearest the colour the block's art is painted with, in sewing order, or, when a neighbouring block has that one, the nearest other thread within threshold → {palette: \"pec\"|\"jef\" (the palette in use), threshold: the colour difference past which a thread is flagged, blockCount, offset, truncated: whether blocks after the listed ones are left out, blocks: [{block: from 1 in sewing order, thread: {palette, index: from 1 as the format numbers it, name, color: \"#rrggbb\", art: \"#rrggbb\" (the colour the art is painted with), deltaE: the CIEDE2000 difference between the two, to two decimals (0: painted with the thread's own colour, a swatch of its thread library), mismatch: deltaE past threshold}, unitCount: the fills, strokes and stitch runs sewn in it}], options: the embroidery options planned with, warnings}",
            has_doc,
            threads
        ),
        cmd!(
            query "embroidery.stats",
            "Embroidery Statistics",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, speed?: stitches a minute the machine sews at (60 to 1500; default: the document's machine's (embroidery.setup), else 650, a home machine's default, an estimate), fabricThickness?: millimetres (0 to 5; default: the document's fabric's, else 0.5, an estimate: the top thread goes down through it and back up at each needle point), offset?: the first block listed (default 0), limit?: the most blocks listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the design's figures as the export sews it, totals and per block; lengths, positions and boxes are millimetres from the document's origin, y down, boxes [left, top, right, bottom] → {stitches: needle points, records: needle points, jumps and cuts (what record numbers count), jumps, trims, colorChanges, threadCount: distinct threads, blockCount, size: {width, height}, bounds, start, end: [x, y] (the first and last needle points), extents: {left, right, up, down} from the start, shortestStitch, longestStitch, longestJump, lengths: {bins: [{from, to, count}] (needle-down stitches by length: half a millimetre a bin up to 12.5, then up to 12.7 inclusive, then, to null, every longer one, as stitchTooLong counts them), median: to the tenth (null with no stitch)}, threadUsage: {topMetres, bobbinMetres, fabricThickness, estimate: what the estimate rests on}, time: {seconds, minutes, speed, slowSpeed, slowAbove, trimSeconds, includes: what the time counts, estimate: what the estimate rests on} (colour changes are counted, not timed), blocks: [{block: from 1, thread: {palette, index, name, color, art, deltaE, mismatch} as embroidery.threads gives it, stitches, records, jumps, trims, firstRecord, lastRecord: record numbers (from 1, every needle point, jump and cut taking one), topThreadMetres, bobbinThreadMetres, seconds}], offset, truncated: whether blocks after the listed ones are left out, options: the embroidery options planned with, warnings}",
            has_doc,
            stats_cmd
        ),
        cmd!(
            query "embroidery.sequence",
            "Embroidery Sewing Order",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, offset?: the first unit listed (default 0), limit?: the most units listed (0 to 10000; default 1000), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} the design's sewing order, read only (sewing order follows stacking order: move objects with node.move to change it), its units from offset in sewing order within their blocks; positions and boxes are millimetres from the document's origin, y down, boxes [left, top, right, bottom] → {blockCount, unitCount, offset, truncated: whether units after the listed ones are left out, blocks: [{block: from 1 in sewing order, thread: {palette, index, name, color, art, deltaE, mismatch} as embroidery.threads gives it, stitches: needle points, records: needle points, jumps and cuts, firstRecord, lastRecord: record numbers (from 1, every needle point, jump and cut taking one), jumps, trims, unitCount, units: [{unit: from 1, its position in this plan, id: the object a person selects (the symbol instance for art inside a symbol), node: the object sewn, instance: the symbol instance or null, part: \"fill\"|\"stroke\"|\"literal\", partIndex: which fill or stroke, from 1 in appearance order (null for literal), occurrence: which unit of the same id and part, from 1 in walk order (a symbol instance's paths, an expanded object's pieces; id, part, partIndex and occurrence tell a unit apart within a plan, and from one plan to the next while the object's own art keeps its number and order; node changes between plans for an expanded object's pieces and is shared by a symbol's instances, unit is a position in one plan), sewn: \"running\"|\"satin\"|\"tatami\"|\"literal\" (as the stitch option names them), columns: whether a fill's narrow parts are satin columns, rule: \"fill\"|\"wideStroke\"|\"narrowStroke\"|\"strokeFilled\"|\"strokeOutlined\"|\"forced\"|\"literal\" (the rule that chose how it is sewn), ruleValues: the values the rule turned on, why: the same as a sentence, stitches, records, firstRecord, lastRecord (null when it adds none: one needle point where the needle already is), jumps, trims, entry, exit: [x, y] (its first and last needle points), bounds}] (only the listed units), then: {colorChange: true, trim: whether the block ends with a cut} (null after the last block)}] (the blocks of the listed units), splitCount, split: [{id, blocks}] (objects whose units are sewn in more than one block; at most limit), options: the embroidery options planned with, warnings}",
            has_doc,
            sequence_cmd
        ),
        cmd!(
            query "embroidery.render",
            "Embroidery Picture",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, region?: \"design\" (default: the design's stitches with a 5 mm margin), artboard?: index from 0, rect?: [left, top, right, bottom] (points; give one of region, artboard and rect), scale?: pixels a point (above 0, at most 64; default 1), jumps?: true (jumps and cuts marked: two-tone dashes where the thread was cut, a thin line in the thread's colour where it lies, a × at each cut), needlePoints?: false (a dot at each needle point, where they stand at least 6 screen points apart), background?: \"#rrggbb\" (default: the fabric, the colour the document's embroidery settings give it, else white; null: transparent)} the design's stitches as the stitch preview draws them, as a PNG: each block in its palette thread's colour, threads as wide as they lie (0.4 mm for 40 wt), an edge for a thread close to the fabric; planned as the read commands plan → {dataBase64, width, height, region: [left, top, right, bottom] points, generation: the plan's (another plan, another number), options}",
            has_doc,
            render::render_cmd
        ),
        cmd!(
            query "embroidery.check",
            "Embroidery Check",
            [],
            None,
            "{…the embroidery options that decide what is sewn as embroidery.threads takes them, format?: \"dst\"|\"tap\"|\"t01\"|\"exp\"|\"pes\"|\"pec\"|\"jef\"|\"vp3\" (the format whose limits apply; default the format of the document's thread palette: pes for pec and jef for jef; with Janome's palette, the default, that is JEF, which holds any number of colour blocks, so formatColors checks PES's or VP3's limit only when that format is asked for), rules?: [id…] (the rules to run; default all of satinTooWide stitchTooLong formatStep fillStitchShort satinStitchShort runStitchShort stitchBelowNeedle stitchTiny shortStitchesInARow layers threadMismatch notSewn formatColors machineStitches machineColorChanges), limit?: the hits each rule lists (0 to 1000; default 20), selectedOnly?: false (the selected objects alone, in their layers, as document.export {selectedOnly} writes them)} checks the design as the export sews it, each rule with its threshold and where the threshold comes from; positions are millimetres from the document's origin, y down → {format, palette: the palette whose threads threadMismatch judges (the format's own when it numbers its threads, a PES's Brother threads matched to the art as its writer matches them; else the document's), rules: [{id, title, severity: error (the format can't hold it)|warning (quality may suffer)|info, threshold, unit: mm|stitches|units|deltaE|blocks|colorChanges (null: the rule measures nothing), measures: what each value measures, sources: [{name, url, value}] (the first is the one checked; where sources disagree each is listed; a format's limits name each format's own), hits: how many, objectCount: how many objects were hit, objects: [id…] (at most 1000), truncated: whether objects were left out, examples: [{record: record number (from 1, every needle point, jump and cut taking one), at: [x, y], value: the measure compared with the threshold, length: the stitch's own length when the value measures something else (formatStep), pieces: [fewest, most] records it is written as (formatStep), id: the object, note, from: [x, y] where a stitch hit's stitch starts (at is where it ends), block: the block a threadMismatch hit is about (from 1), thread: its thread as embroidery.threads gives it, objects: [id…] the objects a notSewn hit left art of out (at most 1000; null for other rules)}] (the first limit hits), measured: the extreme of what the rule judged, hit or not (the largest where too much is the fault, the smallest where too little; null when the rule measures nothing or judged nothing)}], summary: {errors, warnings, info: each {rules: how many rules found something, hits: how many hits they found}}, notChecked: [{id: \"density\"|\"penetrations\"|\"ties\", what, why}]}",
            has_doc,
            check_cmd
        ),
    ];
    specs.extend(view::specs());
    specs.extend(reorder::specs());
    specs
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
    // Only what is sewn is cut after: type, images and the like are passed over.
    let ids: Vec<NodeId> = ids.into_iter().filter(|id| d.node(*id).is_some_and(objects::cut_after_sewn)).collect();
    if ids.is_empty() {
        return Err(bad(C, "none of the objects given is sewn (type, images, placed files and guides aren't), so none is cut after"));
    }
    // Marked: asked for a cut after it, or a run of stitches whose file had one.
    let marked = |e: Option<&Embroidery>| e.is_some_and(|e| e.trim_after || e.stitches.as_ref().is_some_and(|r| r.trim_after));
    let on = on.unwrap_or_else(|| !ids.iter().all(|id| marked(d.node(*id).and_then(|n| n.extras().embroidery.as_deref()))));
    let changed = edit_counted(s, "Embroidery Trim After", |d| {
        objects::edit_each(d, &ids, |e| {
            e.set_trim_after(on);
            // Off: no cut after it, the one its file had included.
            if let Some(run) = e.stitches.as_mut().filter(|_| !on) {
                run.trim_after = false;
            }
        })
    })?;
    Ok(json!({ "on": on, "changed": changed }))
}

/// The keys of command `cmd`'s params `o` other than `own`, refused, naming the nearest of `own`.
pub(super) fn only(o: &Map<String, Value>, own: &[&str], cmd: &str) -> Result<()> {
    let Some(k) = o.keys().find(|k| !own.contains(&k.as_str())) else { return Ok(()) };
    Err(bad(
        cmd,
        match crate::cmd::params::nearest(k, own) {
            Some(near) => format!("unknown parameter `{k}`; did you mean `{near}`?"),
            None if own.is_empty() => format!("unknown parameter `{k}`: {cmd} takes no parameters"),
            None => format!("unknown parameter `{k}`: {cmd} takes {}", own.join(", ")),
        },
    ))
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

/// A thread record the document keeps on an opened run ([`vectorcraft_doc::embroidery::StitchRun::thread`])
/// as the machine formats take it.
pub(crate) fn file_thread(r: &vectorcraft_doc::embroidery::ThreadRecord) -> vectorcraft_stitch::ThreadRecord {
    vectorcraft_stitch::ThreadRecord {
        catalogue: r.catalogue.clone(),
        description: r.description.clone(),
        brand: r.brand.clone(),
        material: r.material,
        weight: r.weight,
    }
}

/// A thread record a machine file gave, as the document keeps it on the runs opened from it.
pub(crate) fn kept_thread(r: &vectorcraft_stitch::ThreadRecord) -> vectorcraft_doc::embroidery::ThreadRecord {
    vectorcraft_doc::embroidery::ThreadRecord {
        catalogue: r.catalogue.clone(),
        description: r.description.clone(),
        brand: r.brand.clone(),
        material: r.material,
        weight: r.weight,
        extra: Default::default(),
    }
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
    let (uid, snapshot, doc) = (st.uid, st.doc.clone(), to_plan(st, selected_only)?);
    // The plan is remembered by the snapshot and the objects it was cut to, in any order.
    let mut selection = if selected_only { st.selection.objects.clone() } else { Vec::new() };
    selection.sort_unstable();
    selection.dedup();
    let of = service::Of { snapshot: &snapshot, selection: &selection };
    let plan = s.embroidery_plans.of(uid).plan(of, &doc, opts).map_err(PlanError::error)?;
    Ok((plan, doc))
}

/// The document planned of `st`'s snapshot, as the export prepares it: the selected objects alone
/// when `selected_only` (in their layers and groups, as `document.export {selectedOnly}` writes
/// them), and without an opacity mask's editing layer. The snapshot itself when that is all of it.
fn to_plan(st: &crate::DocState, selected_only: bool) -> Result<Arc<Document>> {
    if !selected_only && matches!(st.doc.without_edit_modes(), std::borrow::Cow::Borrowed(_)) {
        return Ok(st.doc.clone());
    }
    let picked = crate::cmd::fileio::export_source(st, &json!({ SELECTED_ONLY: selected_only }))?;
    Ok(Arc::new(picked.without_edit_modes().into_owned()))
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

/// What a read command asks for besides its options: which reply, and what the reply takes.
enum Read {
    Threads(Page),
    Stats {
        asked: stats::Asked,
        page: Page,
    },
    Sequence(Page),
    Check {
        rules: Vec<&'static check::Rule>,
        format: Option<StitchFormat>,
        limit: usize,
    },
    /// `embroidery.info` of the objects `ids`, planning the design only when `plan` asks.
    Info {
        ids: Vec<vectorcraft_doc::NodeId>,
        page: Page,
        plan: bool,
    },
}

/// A read command's params: what it reads, the options it plans with, and whether it plans the
/// selected objects alone.
struct ReadParams {
    read: Read,
    opts: Options,
    selected_only: bool,
}

/// The read commands: `embroidery.threads`, `stats`, `sequence`, `check` and `info`.
const READ_COMMANDS: [&str; 5] = ["embroidery.threads", "embroidery.stats", "embroidery.sequence", "embroidery.check", "embroidery.info"];

/// Read command `cmd`'s params `p`, checked as the command checks them.
fn read_params(cmd: &str, p: &Value) -> Result<ReadParams> {
    let o = object(p, cmd)?;
    let selected_only = selected_only(&o, cmd)?;
    let (read, opts) = match cmd {
        "embroidery.threads" => (Read::Threads(Page::from_params(&o, cmd)?), options_in(&o, &LIST_KEYS, cmd)?),
        "embroidery.stats" => {
            let asked = stats::Asked::from_params(&o, cmd)?;
            (Read::Stats { asked, page: Page::from_params(&o, cmd)? }, options_in(&o, &STATS_KEYS, cmd)?)
        }
        "embroidery.sequence" => (Read::Sequence(Page::from_params(&o, cmd)?), options_in(&o, &LIST_KEYS, cmd)?),
        "embroidery.check" => {
            let opts = options_in(&o, &CHECK_KEYS, cmd)?;
            (check_params(&o)?, opts)
        }
        // A planner has no selection: the objects are given.
        "embroidery.info" => {
            let ask = info::Ask::from_params(&o, cmd)?;
            (Read::Info { ids: ask.ids.unwrap_or_default(), page: ask.page, plan: ask.plan }, ask.opts)
        }
        _ => return Err(bad(cmd, format!("not a read command: one of {}", READ_COMMANDS.join(", ")))),
    };
    Ok(ReadParams { read, opts, selected_only })
}

/// The reply of read `read` from `plan`, the plan of `doc` with `opts`: the read commands' own
/// replies, made the same way on any thread. Cancelled ([`EngineError::Cancelled`]) when the work
/// on this thread is: the walks stop early then ([`measure::each_until_cancelled`]), and a reply
/// cut short is never given.
fn reply(read: Read, plan: &plan::DesignPlan, doc: &Document, opts: &Options) -> Result<Value> {
    let v = match read {
        Read::Threads(page) => threads_reply(plan, doc, page),
        Read::Stats { asked, page } => {
            let (machine, thread) = asked.model(&resolve::DocLayer::of(doc));
            stats::stats(plan, machine, thread, page)
        }
        Read::Sequence(page) => order::sequence(plan, page),
        Read::Info { ids, page, .. } => info::to_json(&info::reply(doc, &ids, opts, Some(plan), page, Vec::new()))?,
        Read::Check { rules, format, limit } => {
            // The format of the document's thread palette, when none is asked for.
            let format = format.unwrap_or_else(|| thread_palette(doc).format());
            check::check(plan, &check::Ask { rules, format, limit })
        }
    };
    vectorcraft_doc::cancel::check()?;
    Ok(with_options(v, opts))
}

/// A read command run on the active document: its params read, its plan made (or the plan
/// remembered), its reply.
fn read_cmd(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let r = read_params(cmd, p)?;
    let (plan, doc) = planned(s, &r.opts, r.selected_only, cmd)?;
    reply(r.read, &plan, &doc, &r.opts)
}

impl Planner {
    /// The reply read command `cmd` (`embroidery.threads`, `stats`, `sequence`, `check` or `info`)
    /// gives with `params` for the planner's snapshot, made by the command's own code on any thread
    /// (the stitch preview's panel and status bar, and the Object tab, read the design off the
    /// interface thread). The plan is the one remembered, or planned. `selectedOnly` is refused: a
    /// planner has no selection. `info` plans only with `plan`, and answers the settings with the
    /// reason in `warnings` when the design can't be planned, as the command does.
    pub fn query(&self, cmd: &str, params: &Value) -> Result<Value> {
        let r = read_params(cmd, params)?;
        if r.selected_only {
            return Err(bad(cmd, format!("a planner plans the whole document: leave out `{SELECTED_ONLY}`")));
        }
        if let Read::Info { ids, page, plan } = &r.read {
            let (planned, mut warnings) = (if *plan { Some(self.planned(&r.opts)) } else { None }, Vec::new());
            let plan = match planned {
                Some(Ok(p)) => Some(p.plan),
                Some(Err(EngineError::Cancelled)) => return Err(EngineError::Cancelled),
                Some(Err(e)) => {
                    warnings.push(e.to_string());
                    None
                }
                None => None,
            };
            let v = info::to_json(&info::reply(&self.doc, ids, &r.opts, plan.as_deref(), *page, warnings))?;
            vectorcraft_doc::cancel::check()?;
            return Ok(with_options(v, &r.opts));
        }
        let planned = self.planned(&r.opts)?;
        reply(r.read, &planned.plan, &self.doc, &r.opts)
    }
}

fn stats_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    read_cmd(s, p, "embroidery.stats")
}

fn sequence_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    read_cmd(s, p, "embroidery.sequence")
}

fn check_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    read_cmd(s, p, "embroidery.check")
}

fn threads(s: &mut Session, p: &Value) -> Result<Value> {
    read_cmd(s, p, "embroidery.threads")
}

/// The keys `embroidery.check` takes besides the embroidery options.
const CHECK_KEYS: [&str; 4] = ["format", "rules", "limit", SELECTED_ONLY];

/// `embroidery.check`'s own params: the rules, the format and the hits listed.
fn check_params(o: &Map<String, Value>) -> Result<Read> {
    const C: &str = "embroidery.check";
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
    Ok(Read::Check { rules, format, limit })
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

/// `embroidery.threads`' reply (its options aside): `plan`'s blocks `page` lists, with the
/// threads of `d`'s palette.
fn threads_reply(plan: &plan::DesignPlan, d: &Document, page: Page) -> Value {
    let listed = page.range(plan.blocks.len());
    let blocks: Vec<Value> = listed
        .clone()
        .filter_map(|k| Some((k, plan.blocks.get(k)?)))
        // Numbered from 1, as a list of threads shows them.
        .map(|(k, b)| json!({"block": k + 1, "thread": stats::thread_json(&b.thread), "unitCount": b.units.len()}))
        .collect();
    json!({
        "palette": thread_palette(d).id(),
        "threshold": MISMATCH_DELTA_E,
        "blockCount": plan.blocks.len(),
        "offset": listed.start,
        "truncated": page.truncated(plan.blocks.len()),
        "blocks": blocks,
        "warnings": plan.messages(),
    })
}
