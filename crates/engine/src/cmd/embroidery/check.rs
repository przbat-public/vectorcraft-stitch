//! Design checks (`embroidery.check`): rules that flag what a machine may not sew well, or what a
//! format can't hold, each with its threshold and where the threshold comes from ([`RULES`]).
//! Where two sources give different thresholds, both are listed and the first is the one checked.
//!
//! Melco's design checker works on a design's objects and their settings: its fill, satin and
//! walk-stitch limits are checked here on each sewing unit's settings (its fill or running stitch
//! length, its satin columns' width), and report the objects; stitches read from a machine file
//! have no such settings, so those three rules pass them by. The other length rules measure
//! needle-down stitches: the thread laid from one needle point to the next without lifting the
//! needle ([`super::measure::Record::sewn`]), a unit's underlay, travel and tie stitches included.
//!
//! Not checked: "density too tight" and "too many needle penetrations in an area", which Melco's
//! design checker names without a number ([`NOT_CHECKED`]; `docs/embroidery.md` › Using it ›
//! Checks).

use std::collections::{BTreeSet, HashMap};

use serde_json::{Value, json};
use vectorcraft_geom::Point;
use vectorcraft_stitch::Command;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::thread::MISMATCH_DELTA_E;

use super::hex;
use super::measure::{self, Record};
use super::plan::DesignPlan;
use super::stats::{mm, point};

/// How much a finding matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Severity {
    /// The design can't be sewn, or the format can't hold it.
    Error,
    /// The design sews, but its quality may suffer.
    Warning,
    /// Worth knowing.
    Info,
}

impl Severity {
    fn id(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Where a threshold comes from: who says it, where, and the value they give.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Source {
    pub(crate) name: &'static str,
    pub(crate) url: &'static str,
    pub(crate) value: &'static str,
}

/// One check.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rule {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) severity: Severity,
    /// What it is checked against, and its unit (`None`: no number; for the format's own limits,
    /// the format's value).
    pub(crate) threshold: Option<f64>,
    pub(crate) unit: &'static str,
    /// Its sources, the one checked first.
    pub(crate) sources: &'static [Source],
}

// The thresholds, each from the source its rule lists first.

/// Melco DesignShop v11's design checker flags these: satin stitches wider than 10 mm, any stitch
/// over 12.7 mm, fill stitches smaller than 2.5 mm, satin stitches shorter than 1 mm and walk
/// (running) stitches shorter than 1.5 mm (user manual, "Design Checker Tab", p. 38).
const MELCO_CHECKER: &str = "http://www.melco-service.com/docs/DSv11_Manual.pdf";
/// The widest satin column Melco's checker lets pass (millimetres).
const SATIN_WIDEST: f64 = 10.0;
/// The longest stitch it lets pass (millimetres).
const LONGEST_STITCH: f64 = 12.7;
/// The shortest fill, satin and running (walk) stitches it lets pass (millimetres), checked on
/// each object's settings: its fill stitch length, its satin columns' width (a satin stitch spans
/// the column), its running stitch length.
const FILL_SHORTEST: f64 = 2.5;
const SATIN_SHORTEST: f64 = 1.0;
const RUN_SHORTEST: f64 = 1.5;
/// Melco's machine manual: keep stitches longer than the needle's blade, 0.75 mm for a 75/11
/// needle; most needles are 0.7 to 0.8 mm, so 1 mm or longer; ideally walk stitches 1.5 mm, satin
/// 1 mm and fill 2 mm or longer (Melco OS manual, "Stitch Size vs. Needle Size", pp. 301–302).
const MELCO_MACHINE: &str = "https://www.melco-service.com/docs/X/melco-os-ai-en.pdf";
/// A 75/11 needle's blade, millimetres (the same manual).
const NEEDLE_BLADE: f64 = 0.75;
/// A stitch shorter than this is short in a row of short stitches ([`MOST_SHORT_IN_A_ROW`]): the
/// 1 mm Melco's machine manual keeps stitches above for most needles.
const SHORT: f64 = 1.0;
/// Ricoma's digitizing tips delete stitches under 0.3 mm
/// (<https://www.ricoma.cn/cn/ndetail13_539>), and Brother's embroidery machines delete stitches
/// of the set pitch or less, 0.3 mm as they come (PRT23 manual, p. 107).
const TINY: f64 = 0.3;
/// Wilcom's stitch shortening allows up to five short stitches in a row
/// (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/quality/quality-15.htm>).
const MOST_SHORT_IN_A_ROW: usize = 5;
/// Brother's PRT51 manual warns that stitching sewn over three times or more may break the thread
/// or the needle (<https://download.brother.com/welcome/doch100486/prt51_om02ja.pdf>, data
/// precautions and troubleshooting).
const LAYERS: usize = 3;
/// The cells layers are counted in, millimetres square: a few rows of fill wide, so that two units
/// sewn on the same spot share a cell. An estimate.
const LAYER_CELL: f64 = 1.0;
/// The most points one stitch is counted at for layers: every half cell along a stitch up to 32 mm
/// long, more sparsely along a longer one (which no machine sews well, and `stitchTooLong` flags).
/// A bound on the work a stitch read from a file can ask for, however long it is.
const MAX_LAYER_SAMPLES: usize = 64;
/// The most cells layers are counted over: four million square millimetres, far more stitched area
/// than any hoop holds. Past it, new cells are not counted: a bound on the memory a design read
/// from a file can ask for.
const MAX_LAYER_CELLS: usize = 4_000_000;
/// Brother's PR680W shows `.pes` files of up to 500,000 stitches and 127 colour changes
/// (<https://download.brother.com/welcome/doch102285/884t23_om01en.pdf>, specifications): one
/// machine's limits, not the format's.
const MACHINE_STITCHES: f64 = 500_000.0;
/// The colour changes the same machine shows at most.
const MACHINE_COLOR_CHANGES: f64 = 127.0;

/// The grid the formats count in, millimetres: the writers round each end of a stitch to it before
/// splitting a long one, so a stitch's length on the grid is within one step of its own. A stitch no
/// longer than a format's step is never split; how many pieces a longer one makes is certain only
/// when that holds for both roundings.
const GRID: f64 = 1.0 / vectorcraft_stitch::UNITS_PER_MM;

/// The hits listed per rule when `limit` is left out: enough to show where a rule bites.
pub(crate) const LIMIT: usize = 20;
/// The most hits per rule `limit` may ask for: the output is bounded whatever the design.
pub(crate) const MAX_LIMIT: usize = 1000;
/// The most objects a rule lists as hit (the count of hits is never cut).
const MAX_OBJECTS: usize = 1000;

/// How the two Melco manuals are named in the sources the checks list.
const MELCO_CHECKER_NAME: &str = "Melco DesignShop v11 user manual, Design Checker Tab, p. 38";
const MELCO_MACHINE_NAME: &str = "Melco OS manual, Stitch Size vs. Needle Size, pp. 301–302";

/// Every rule, in the order the checks list them.
pub(crate) const RULES: &[Rule] = &[
    Rule {
        id: "satinTooWide",
        title: "Satin column wider than the widest a satin sews well",
        severity: Severity::Warning,
        threshold: Some(SATIN_WIDEST),
        unit: "mm",
        sources: &[
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "satin stitches wider than 10 mm" },
            Source {
                name: "Ricoma digitizing tips",
                url: "https://www.ricoma.cn/cn/ndetail13_604",
                value: "shapes 12 mm and wider are usually not satin",
            },
        ],
    },
    Rule {
        id: "stitchTooLong",
        title: "Stitch longer than the longest a machine sews well",
        severity: Severity::Warning,
        threshold: Some(LONGEST_STITCH),
        unit: "mm",
        sources: &[Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "any stitch over 12.7 mm" }],
    },
    Rule {
        id: "formatStep",
        title: "Stitch longer than one record of the format holds: written as several shorter stitches",
        severity: Severity::Info,
        threshold: None,
        unit: "mm along x or y",
        sources: &[Source {
            name: "EduTechWiki's pages on the formats (DST, TAP and T01: 12.1 mm a record; EXP, JEF and VP3: 12.7 mm; PES and PEC: 204.7 mm)",
            url: "https://edutechwiki.unige.ch/en/Embroidery_format_DST",
            value: "the largest move one stitch record holds",
        }],
    },
    Rule {
        id: "fillStitchShort",
        title: "Fill stitch length set shorter than a fill sews well",
        severity: Severity::Warning,
        threshold: Some(FILL_SHORTEST),
        unit: "mm, the object's fill stitch length",
        sources: &[
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "fill stitches smaller than 2.5 mm" },
            Source { name: MELCO_MACHINE_NAME, url: MELCO_MACHINE, value: "fill stitches ideally 2 mm or longer" },
        ],
    },
    Rule {
        id: "satinStitchShort",
        title: "Satin column narrower than a satin stitch sews well",
        severity: Severity::Warning,
        threshold: Some(SATIN_SHORTEST),
        unit: "mm, the object's narrowest satin column",
        sources: &[
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "satin stitches shorter than 1 mm" },
            Source { name: MELCO_MACHINE_NAME, url: MELCO_MACHINE, value: "satin stitches ideally 1 mm or longer" },
        ],
    },
    Rule {
        id: "runStitchShort",
        title: "Running stitch length set shorter than a running line sews well",
        severity: Severity::Warning,
        threshold: Some(RUN_SHORTEST),
        unit: "mm, the object's running stitch length",
        sources: &[
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "walk stitches shorter than 1.5 mm" },
            Source { name: MELCO_MACHINE_NAME, url: MELCO_MACHINE, value: "walk stitches ideally 1.5 mm or longer" },
        ],
    },
    Rule {
        id: "stitchBelowNeedle",
        title: "Stitch shorter than the needle's blade is thick (a 75/11 needle)",
        // Information for now: VectorCraft's own fill rows end in remainders of 0.4 to 0.75 mm,
        // which this flags in nearly every fill. Once the planner spreads a row's remainder over
        // its last stitches (FORK.md's work list), it becomes a warning again.
        severity: Severity::Info,
        threshold: Some(NEEDLE_BLADE),
        unit: "mm",
        sources: &[Source {
            name: MELCO_MACHINE_NAME,
            url: MELCO_MACHINE,
            value: "keep stitches longer than the needle's blade: 0.75 mm for a 75/11 needle",
        }],
    },
    Rule {
        id: "stitchTiny",
        title: "Stitch so short that machines delete it",
        severity: Severity::Warning,
        threshold: Some(TINY),
        unit: "mm",
        sources: &[
            Source { name: "Ricoma digitizing tips", url: "https://www.ricoma.cn/cn/ndetail13_539", value: "delete stitches under 0.3 mm" },
            Source {
                name: "Brother PRT23 manual, p. 107",
                url: "https://download.brother.com/welcome/doch102354/prt23_om01ja.pdf",
                value: "the machine deletes stitches of 0.3 mm or less, as it comes",
            },
        ],
    },
    Rule {
        id: "shortStitchesInARow",
        title: "More short stitches in a row than stitch shortening allows (each shorter than 1 mm)",
        severity: Severity::Warning,
        threshold: Some(MOST_SHORT_IN_A_ROW as f64),
        unit: "stitches",
        sources: &[
            Source {
                name: "Wilcom EmbroideryStudio help, Apply stitch shortening",
                url: "https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/quality/quality-15.htm",
                value: "up to five short stitches in a row",
            },
            Source { name: MELCO_MACHINE_NAME, url: MELCO_MACHINE, value: "stitches 1 mm or longer" },
        ],
    },
    Rule {
        id: "layers",
        title: "Three or more layers of stitching on one spot",
        severity: Severity::Warning,
        threshold: Some(LAYERS as f64),
        unit: "sewing units over a 1 mm square",
        sources: &[Source {
            name: "Brother PRT51 manual (Japanese), data precautions and troubleshooting",
            url: "https://download.brother.com/welcome/doch100486/prt51_om02ja.pdf",
            value: "stitching sewn over three times or more may break the thread or the needle",
        }],
    },
    Rule {
        id: "threadMismatch",
        title: "Thread that looks unlike the art's colour",
        severity: Severity::Warning,
        threshold: Some(MISMATCH_DELTA_E),
        unit: "ΔE (CIEDE2000)",
        sources: &[Source {
            name: "the project owner's choice (docs/embroidery.md › Threads)",
            url: "",
            value: "a thread farther than ΔE 10 as shown",
        }],
    },
    Rule {
        id: "notSewn",
        title: "Art that is not sewn, or sewn approximately",
        severity: Severity::Warning,
        threshold: None,
        unit: "",
        sources: &[Source {
            name: "the stitch plan's warnings (docs/embroidery.md › What is not stitched)",
            url: "",
            value: "type, images, live art, patterns and gradients",
        }],
    },
    Rule {
        id: "formatColors",
        title: "More colour blocks than the format holds",
        severity: Severity::Error,
        threshold: None,
        unit: "blocks",
        sources: &[
            Source {
                name: "EduTechWiki, Embroidery format PEC (its header counts the colours in a byte)",
                url: "https://edutechwiki.unige.ch/en/Embroidery_format_PEC",
                value: "PES and PEC: 255 colours",
            },
            Source {
                name: "EduTechWiki, Embroidery format VP3 (its summary counts the colour blocks in a 16-bit word)",
                url: "https://edutechwiki.unige.ch/en/Embroidery_format_VP3",
                value: "VP3: 65,535 colour blocks",
            },
        ],
    },
    Rule {
        id: "machineStitches",
        title: "More stitches than one home machine shows",
        severity: Severity::Info,
        threshold: Some(MACHINE_STITCHES),
        unit: "stitches",
        sources: &[Source {
            name: "Brother PR680W manual, specifications",
            url: "https://download.brother.com/welcome/doch102285/884t23_om01en.pdf",
            value: "up to 500,000 stitches",
        }],
    },
    Rule {
        id: "machineColorChanges",
        title: "More colour changes than one home machine shows",
        severity: Severity::Info,
        threshold: Some(MACHINE_COLOR_CHANGES),
        unit: "colour changes",
        sources: &[Source {
            name: "Brother PR680W manual, specifications",
            url: "https://download.brother.com/welcome/doch102285/884t23_om01en.pdf",
            value: "up to 127 colour changes",
        }],
    },
];

/// One finding: where (a stitch number and a point, millimetres), the unit, and a measure.
#[derive(Clone, Debug, Default)]
struct Hit {
    number: Option<usize>,
    at: Option<Point>,
    unit: Option<usize>,
    value: Option<f64>,
    note: Option<String>,
}

impl Hit {
    /// A hit at record `r`, measuring `value`.
    fn at(r: &Record, value: f64) -> Self {
        Self { number: Some(r.number), at: Some(r.stitch.point()), unit: Some(r.unit), value: Some(value), note: None }
    }
}

/// A rule's findings: how many, the objects hit, and the first few.
#[derive(Default)]
struct Found {
    count: usize,
    objects: BTreeSet<u64>,
    listed: Vec<Hit>,
}

/// What a check is asked for: the rules to run, the format whose limits apply, and how many hits
/// each lists.
pub(crate) struct Ask {
    pub(crate) rules: Vec<&'static Rule>,
    pub(crate) format: Format,
    pub(crate) limit: usize,
}

/// A check under way.
struct Checker<'a> {
    plan: &'a DesignPlan,
    ask: &'a Ask,
    found: HashMap<&'static str, Found>,
    /// The short stitches in a row so far, and the first of them.
    short_run: Option<(usize, Record)>,
    /// The units sewn over each cell so far, and the last of them ([`LAYER_CELL`]).
    cells: HashMap<(i64, i64), (usize, usize)>,
}

impl<'a> Checker<'a> {
    fn new(plan: &'a DesignPlan, ask: &'a Ask) -> Self {
        let found = ask.rules.iter().map(|r| (r.id, Found::default())).collect();
        Self { plan, ask, found, short_run: None, cells: HashMap::new() }
    }

    fn wants(&self, id: &str) -> bool {
        self.found.contains_key(id)
    }

    /// Count `hit` for rule `id`, when it was asked for.
    fn add(&mut self, id: &'static str, hit: Hit) {
        let Some(f) = self.found.get_mut(id) else { return };
        f.count += 1;
        if let Some(u) = hit.unit.and_then(|u| self.plan.units.get(u))
            && f.objects.len() < MAX_OBJECTS
        {
            f.objects.insert(u.origin.selected().0);
        }
        if f.listed.len() < self.ask.limit {
            f.listed.push(hit);
        }
    }

    /// Name the objects units `units` sew among those rule `id` hit (while its list has room).
    fn add_objects(&mut self, id: &'static str, units: std::ops::Range<usize>) {
        let plan = self.plan;
        let Some(f) = self.found.get_mut(id) else { return };
        for u in units.filter_map(|u| plan.units.get(u)) {
            if f.objects.len() >= MAX_OBJECTS {
                break;
            }
            f.objects.insert(u.origin.selected().0);
        }
    }

    /// The rules on one record: its stitch's length, and the short stitches in a row and the
    /// layers it adds to.
    fn record(&mut self, r: &Record) {
        let Some(length) = r.sewn() else {
            // No thread laid from the record before (a jump, a cut, the needle point after one, a
            // block's first after the thread change): a row of short stitches ends here.
            self.end_short_run();
            return;
        };
        if length > LONGEST_STITCH {
            self.add("stitchTooLong", Hit::at(r, length));
        }
        if let Some(before) = r.before {
            let step = self.ask.format.max_step();
            let longest = (r.stitch.x - before.x).abs().max((r.stitch.y - before.y).abs());
            if longest > step {
                let fewest = ((longest - GRID) / step).ceil().max(1.0);
                let most = ((longest + GRID) / step).ceil();
                let note = if fewest == most {
                    format!("written as {most} stitches")
                } else {
                    format!("written as {fewest} or {most} stitches: this close to a multiple of the step, rounding to the format's 0.1 mm decides")
                };
                self.add("formatStep", Hit { note: Some(note), ..Hit::at(r, length) });
            }
        }
        if length < NEEDLE_BLADE {
            self.add("stitchBelowNeedle", Hit::at(r, length));
        }
        if length <= TINY {
            self.add("stitchTiny", Hit::at(r, length));
        }
        if length < SHORT {
            self.short_run = Some(match self.short_run.take() {
                Some((n, first)) => (n + 1, first),
                None => (1, *r),
            });
        } else {
            self.end_short_run();
        }
        if let Some(before) = r.before
            && self.wants("layers")
        {
            self.cover(before.point(), r);
        }
    }

    /// The end of a row of short stitches: a hit when it was longer than allowed.
    fn end_short_run(&mut self) {
        if let Some((n, first)) = self.short_run.take()
            && n > MOST_SHORT_IN_A_ROW
        {
            self.add("shortStitchesInARow", Hit::at(&first, n as f64));
        }
    }

    /// Count the unit of the stitch from `a` to record `r` over each [`LAYER_CELL`] it crosses,
    /// once per unit, and a hit where a cell's count reaches [`LAYERS`].
    fn cover(&mut self, a: Point, r: &Record) {
        let b = r.stitch.point();
        let length = a.distance(b);
        if !length.is_finite() {
            return;
        }
        // Samples half a cell apart, so that no cell the stitch crosses is skipped, at most
        // MAX_LAYER_SAMPLES of them.
        let samples = ((length / (LAYER_CELL / 2.0)).ceil().max(1.0) as usize).min(MAX_LAYER_SAMPLES);
        for i in 0..=samples {
            let p = a.lerp(b, i as f64 / samples as f64);
            let cell = ((p.x / LAYER_CELL).floor() as i64, (p.y / LAYER_CELL).floor() as i64);
            if self.cells.len() >= MAX_LAYER_CELLS && !self.cells.contains_key(&cell) {
                continue;
            }
            let (count, last) = self.cells.entry(cell).or_insert((0, usize::MAX));
            if *last == r.unit {
                continue;
            }
            *count += 1;
            *last = r.unit;
            if *count == LAYERS {
                let centre = Point::new((cell.0 as f64 + 0.5) * LAYER_CELL, (cell.1 as f64 + 0.5) * LAYER_CELL);
                self.add("layers", Hit { number: Some(r.number), at: Some(centre), unit: Some(r.unit), value: Some(LAYERS as f64), note: None });
            }
        }
    }

    /// The rules on whole units (their settings), blocks and the design.
    fn rest(&mut self) {
        let plan = self.plan;
        for (k, u) in plan.units.iter().enumerate() {
            // At the unit: its first stitch and needle point.
            let first = plan.numbers(k).map(|n| n.start);
            let at = plan.unit_stitches(k).iter().find(|s| s.command == Command::Stitch).map(|s| s.point());
            let hit = |value: f64| Hit { number: first, at, unit: Some(k), value: Some(value), note: None };
            if let Some([narrowest, widest]) = u.satin_widths {
                if widest > SATIN_WIDEST {
                    self.add("satinTooWide", hit(widest));
                }
                if narrowest < SATIN_SHORTEST {
                    self.add("satinStitchShort", hit(narrowest));
                }
            }
            // A literal run has no settings: it is sewn as its points are. A unit is judged by how
            // it is sewn: a fill under `stitchStyle: satin` is filled, with its fill stitch length.
            match (u.sewn(), u.params) {
                (Some(Style::Fill), Some(p)) if p.stitch_length < FILL_SHORTEST => self.add("fillStitchShort", hit(p.stitch_length)),
                (Some(Style::Running), Some(p)) if p.stitch_length < RUN_SHORTEST => self.add("runStitchShort", hit(p.stitch_length)),
                _ => {}
            }
        }
        for (k, b) in plan.blocks.iter().enumerate() {
            if b.thread.mismatch() {
                let note = format!("block {}: {} {} for the art's {}", k + 1, b.thread.palette.id(), b.thread.index, hex(b.thread.art));
                let hit = Hit {
                    number: Some(b.first_number),
                    unit: Some(b.units.start),
                    value: Some(b.thread.shown_delta_e()),
                    note: Some(note),
                    ..Hit::default()
                };
                self.add("threadMismatch", hit);
                // Every object sewn in the thread is hit, not only the block's first.
                self.add_objects("threadMismatch", b.units.clone());
            }
        }
        for w in &plan.warnings {
            self.add("notSewn", Hit { note: Some(w.clone()), ..Hit::default() });
        }
        let blocks = plan.blocks.len();
        if let Some(most) = self.ask.format.max_colors()
            && blocks > most
        {
            let note = format!("{} holds {most}", self.ask.format.label());
            self.add("formatColors", Hit { value: Some(blocks as f64), note: Some(note), ..Hit::default() });
        }
        let needle: usize = plan.blocks.iter().map(|b| b.needle_points()).sum();
        if needle as f64 > MACHINE_STITCHES {
            self.add("machineStitches", Hit { value: Some(needle as f64), ..Hit::default() });
        }
        let changes = blocks.saturating_sub(1);
        if changes as f64 > MACHINE_COLOR_CHANGES {
            self.add("machineColorChanges", Hit { value: Some(changes as f64), ..Hit::default() });
        }
    }

    /// The findings, rule by rule in the order asked, and how many rules of each severity found
    /// something.
    fn into_json(mut self) -> Value {
        let (mut errors, mut warnings, mut notes) = (0, 0, 0);
        let rules: Vec<Value> = self
            .ask
            .rules
            .iter()
            .map(|rule| {
                let f = self.found.remove(rule.id).unwrap_or_default();
                if f.count > 0 {
                    match rule.severity {
                        Severity::Error => errors += 1,
                        Severity::Warning => warnings += 1,
                        Severity::Info => notes += 1,
                    }
                }
                let threshold = match rule.id {
                    "formatStep" => Some(self.ask.format.max_step()),
                    "formatColors" => self.ask.format.max_colors().map(|c| c as f64),
                    _ => rule.threshold,
                };
                let objects: Vec<u64> = f.objects.into_iter().collect();
                json!({
                    "id": rule.id,
                    "title": rule.title,
                    "severity": rule.severity.id(),
                    "threshold": threshold,
                    "unit": rule.unit,
                    "sources": rule.sources.iter().map(|s| json!({"name": s.name, "url": s.url, "value": s.value})).collect::<Vec<_>>(),
                    "hits": f.count,
                    "objects": objects,
                    "first": f.listed.iter().map(|h| self.hit_json(h)).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "format": self.ask.format.id(),
            "rules": rules,
            "summary": {"errors": errors, "warnings": warnings, "info": notes},
            "notChecked": NOT_CHECKED.iter().map(|(what, why)| json!({"what": what, "why": why})).collect::<Vec<_>>(),
        })
    }

    fn hit_json(&self, h: &Hit) -> Value {
        let id = h.unit.and_then(|u| self.plan.units.get(u)).map(|u| u.origin.selected().0);
        json!({"number": h.number, "at": h.at.map(point), "value": h.value.map(mm), "id": id, "note": h.note})
    }
}

/// What Melco's design checker flags that is not checked here, and why.
const NOT_CHECKED: [(&str, &str); 2] = [
    ("density too tight", "Melco's design checker names it without a threshold"),
    ("too many needle penetrations in an area", "Melco's design checker names it without a threshold"),
];

/// Run `ask`'s rules on `plan` → each rule with its findings, how many rules of each severity
/// found something, and what is not checked.
pub(crate) fn check(plan: &DesignPlan, ask: &Ask) -> Value {
    let mut c = Checker::new(plan, ask);
    measure::each(plan, |r| c.record(r));
    c.end_short_run();
    c.rest();
    c.into_json()
}

/// The rule `id` names.
pub(crate) fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id)
}
