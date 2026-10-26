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
use super::plan::{BlockThread, DesignPlan};
use super::stats::{mm, number, point};

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

impl Rule {
    /// Whether its values are counts (stitches, units, blocks, colour changes): given as integers.
    fn counts(&self) -> bool {
        matches!(self.unit, Some("stitches" | "units" | "blocks" | "colorChanges"))
    }

    /// A value of its unit as the answer gives it: a count as an integer, a measure to the
    /// hundredth.
    fn value(&self, v: f64) -> Value {
        if self.counts() { number(v) } else { json!(mm(v)) }
    }
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
    /// The unit of the threshold and of each hit's value, as a token (`mm`, `stitches`, `units`,
    /// `deltaE`, `blocks`, `colorChanges`); `None` for a rule that measures nothing.
    pub(crate) unit: Option<&'static str>,
    /// What each hit's value measures, for display.
    pub(crate) measures: &'static str,
    /// Which extreme of the values it judges it reports as `measured` (`None`: it measures
    /// nothing): the largest for a limit not to pass, the smallest for one not to fall under.
    pub(crate) extreme: Option<Extreme>,
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
pub(super) const LONGEST_STITCH: f64 = 12.7;
/// The shortest fill, satin and running (walk) stitches it lets pass (millimetres), checked on
/// each object's settings: its fill stitch length, its satin columns' width (a satin stitch spans
/// the column), its running stitch length.
const FILL_SHORTEST: f64 = 2.5;
const SATIN_SHORTEST: f64 = 1.0;
const RUN_SHORTEST: f64 = 1.5;
/// Melco's machine manual: needles are 7 to 8 points (0.7 to 0.8 mm) thick, so keep stitches at
/// 1 mm or longer; ideally walk stitches 1.5 mm, satin 1 mm and fill 2 mm or longer (Melco OS
/// manual, "Stitch Size vs. Needle Size", pp. 301–302); a 75/11 needle's blade is 0.75 mm ("Needle
/// Sizes", p. 202).
const MELCO_MACHINE: &str = "https://www.melco-service.com/docs/X/melco-os-ai-en.pdf";
/// A 75/11 needle's blade, millimetres (the same manual, p. 202).
const NEEDLE_BLADE: f64 = 0.75;
/// A stitch shorter than this is short in a row of short stitches ([`MOST_SHORT_IN_A_ROW`]): the
/// 1 mm Melco's machine manual keeps stitches above for most needles.
const SHORT: f64 = 1.0;
/// Brother's embroidery machines delete stitches of the set pitch or less, 0.3 mm as they come
/// (PRT23 manual, printed p. 108, p. 110 of the PDF): the source checked, so a stitch of 0.3 mm is
/// flagged. Ricoma's digitizing tips advise deleting stitches under 0.3 mm while making a design
/// (<https://www.ricoma.cn/cn/ndetail13_539>): advice to digitizers, not what a machine does.
const TINY: f64 = 0.3;
/// The most stitches shorter than [`SHORT`] in a row before they are flagged. An estimate: no
/// source gives a limit for runs of tiny stitches. Five is the most consecutive shortened satin
/// stitches Wilcom's Shortening effect allows on a tight curve
/// (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/quality/quality-15.htm>), a
/// setting of that effect, not a quality limit: background for the estimate.
const MOST_SHORT_IN_A_ROW: usize = 5;
/// How far a unit's stitches may miss the entry or exit point it was planned toward before the
/// miss is pointed out (millimetres): about two thread widths and the formats' 0.1 mm grid, past
/// which a person sees the needle go in or out elsewhere than where they put it. An estimate.
pub const POINT_MISS_MM: f64 = 1.0;
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
pub(super) const MAX_LAYER_SAMPLES: usize = 64;
/// The most cells layers are counted over as one grid over the design's box ([`Layers`]): four
/// million square millimetres (2 m a side), far more than any hoop holds, at 5 bytes a cell 20 MB.
/// A larger box (only a file's stitches spread far apart) is counted in a table of the cells
/// stitched, at most [`MAX_SPARSE_LAYER_CELLS`] of them. Bounds on the memory a design read from a
/// file can ask for.
const MAX_LAYER_CELLS: usize = 4_000_000;
/// The most cells the table counts: past it, new cells are not counted. A million entries keep a
/// hash table at 2^21 slots of 16 bytes (about 36 MB): its next growth, to 2^22, comes at 1.8
/// million.
const MAX_SPARSE_LAYER_CELLS: usize = 1_000_000;
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
pub const MAX_LIMIT: usize = 1000;
/// The most objects a rule lists as hit; `objectCount` counts them all, and the count of hits is
/// never cut.
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
        unit: Some("mm"),
        measures: "the object's widest satin column",
        extreme: Some(Extreme::Largest),
        sources: &[
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "satin stitches wider than 10 mm" },
            Source {
                name: "Wilcom, Mastering basic stitch types: satin stitch and tatami stitch",
                url: "https://wilcom.com/resources/blog/mastering-basic-stitch-types-satin-stitch-and-tatami-stitch",
                value: "keep satin widths within 10 mm (auto split at 7 mm)",
            },
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
        unit: Some("mm"),
        measures: "a needle-down stitch's length",
        extreme: Some(Extreme::Largest),
        sources: &[Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "any stitch over 12.7 mm" }],
    },
    Rule {
        id: "formatStep",
        title: "Stitch longer than one record of the format holds: written as several shorter stitches",
        severity: Severity::Info,
        threshold: None,
        unit: Some("mm"),
        measures: "a stitch's longer extent along x or y (its length is given apart)",
        extreme: Some(Extreme::Largest),
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
        unit: Some("mm"),
        measures: "the object's fill stitch length",
        extreme: Some(Extreme::Smallest),
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
        unit: Some("mm"),
        measures: "the object's narrowest satin column",
        extreme: Some(Extreme::Smallest),
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
        unit: Some("mm"),
        measures: "the object's running stitch length",
        extreme: Some(Extreme::Smallest),
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
        unit: Some("mm"),
        measures: "a needle-down stitch's length",
        extreme: Some(Extreme::Smallest),
        sources: &[Source {
            name: MELCO_MACHINE_NAME,
            url: MELCO_MACHINE,
            value: "keep stitches longer than the needle's blade (pp. 301–302): 0.75 mm for a 75/11 needle (p. 202)",
        }],
    },
    Rule {
        id: "stitchTiny",
        title: "Stitch so short that machines delete it",
        severity: Severity::Warning,
        threshold: Some(TINY),
        unit: Some("mm"),
        measures: "a needle-down stitch's length",
        extreme: Some(Extreme::Smallest),
        sources: &[
            Source {
                name: "Brother PRT23 manual, printed p. 108 (p. 110 of the PDF)",
                url: "https://download.brother.com/welcome/doch102354/prt23_om01ja.pdf",
                value: "the machine deletes stitches of 0.3 mm or less, as it comes",
            },
            Source {
                name: "Ricoma digitizing tips (advice to digitizers)",
                url: "https://www.ricoma.cn/cn/ndetail13_539",
                value: "delete stitches under 0.3 mm while making the design",
            },
        ],
    },
    Rule {
        id: "shortStitchesInARow",
        title: "Many short stitches in a row (each shorter than 1 mm)",
        severity: Severity::Warning,
        threshold: Some(MOST_SHORT_IN_A_ROW as f64),
        unit: Some("stitches"),
        measures: "short stitches in a row",
        extreme: Some(Extreme::Largest),
        sources: &[
            Source {
                name: "an estimate; background: Wilcom EmbroideryStudio help, Apply stitch shortening",
                url: "https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/quality/quality-15.htm",
                value: "its Shortening effect shortens up to five satin stitches in a row on a tight curve (a setting, not a quality limit)",
            },
            Source { name: MELCO_MACHINE_NAME, url: MELCO_MACHINE, value: "stitches 1 mm or longer" },
        ],
    },
    Rule {
        id: "layers",
        title: "Three or more layers of stitching on one spot",
        severity: Severity::Warning,
        threshold: Some(LAYERS as f64),
        unit: Some("units"),
        measures: "sewing units over one 1 mm square",
        extreme: Some(Extreme::Largest),
        sources: &[
            Source {
                name: "Brother PRT51 manual (Japanese), data precautions and troubleshooting",
                url: "https://download.brother.com/welcome/doch100486/prt51_om02ja.pdf",
                value: "stitching sewn over three times or more may break the thread or the needle",
            },
            Source { name: MELCO_CHECKER_NAME, url: MELCO_CHECKER, value: "too many layers, without a number" },
        ],
    },
    Rule {
        id: "pointMissed",
        title: "Entry or exit point the stitches miss",
        severity: Severity::Info,
        threshold: Some(POINT_MISS_MM),
        unit: Some("mm"),
        measures: "how far a unit's first or last needle point lies from the entry or exit point it was planned toward",
        extreme: Some(Extreme::Largest),
        sources: &[Source {
            name: "the project owner's choice (docs/embroidery.md › What is not stitched): a part ends as near its exit as its stitches reach",
            url: "",
            value: "missed by more than 1 mm (an estimate)",
        }],
    },
    Rule {
        id: "threadMismatch",
        title: "Thread that looks unlike the art's colour",
        severity: Severity::Warning,
        threshold: Some(MISMATCH_DELTA_E),
        unit: Some("deltaE"),
        measures: "the CIEDE2000 difference between the art's colour and its block's thread",
        extreme: Some(Extreme::Largest),
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
        unit: None,
        measures: "nothing: each hit is a warning of the stitch plan",
        extreme: None,
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
        unit: Some("blocks"),
        measures: "the design's colour blocks",
        extreme: Some(Extreme::Largest),
        sources: &[
            Source {
                name: "EduTechWiki, Embroidery format PEC (its header counts the colours in a byte)",
                url: "https://edutechwiki.unige.ch/en/Embroidery_format_PEC",
                value: "PES and PEC: 255 colours",
            },
            Source {
                name: "KDE Community Wiki, Viking/Husqvarna Pfaff/Singer 3 VP3 (a 16-bit thread-change count)",
                url: "https://community.kde.org/Projects/Liberty/File_Formats/Viking_Pfaff",
                value: "VP3: 65,535 colour blocks",
            },
            Source {
                name: "Jason Weiler, VP3 File Format (a 2-byte colour count)",
                url: "http://www.jasonweiler.com/VP3FileFormatInfo.html",
                value: "VP3: 65,535 colour blocks",
            },
            Source {
                name: "EduTechWiki, Embroidery format VP3 (the summary's count is a byte, the design's 16 bits)",
                url: "https://edutechwiki.unige.ch/en/Embroidery_format_VP3",
                value: "VP3: 255 colour blocks, read as a byte",
            },
        ],
    },
    Rule {
        id: "machineStitches",
        title: "More stitches than one home machine shows",
        severity: Severity::Info,
        threshold: Some(MACHINE_STITCHES),
        unit: Some("stitches"),
        measures: "the design's needle points",
        extreme: Some(Extreme::Largest),
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
        unit: Some("colorChanges"),
        measures: "the design's colour changes",
        extreme: Some(Extreme::Largest),
        sources: &[Source {
            name: "Brother PR680W manual, specifications",
            url: "https://download.brother.com/welcome/doch102285/884t23_om01en.pdf",
            value: "up to 127 colour changes",
        }],
    },
];

/// No unit yet over a cell ([`Layers`]).
const NO_UNIT: u32 = u32::MAX;

/// The units sewn over each cell ([`LAYER_CELL`]) so far, each counted once (up to [`LAYERS`]),
/// and the last of them: a grid over the design's box, or for a box of more than
/// [`MAX_LAYER_CELLS`] cells a table of the cells stitched.
enum Layers {
    Grid { x0: i64, y0: i64, width: usize, count: Vec<u8>, last: Vec<u32> },
    Table(HashMap<(i64, i64), (u8, u32)>),
}

impl Layers {
    /// Counting over `plan`'s stitches: a grid over the cells their box takes, when it is small
    /// enough.
    fn new(plan: &DesignPlan) -> Self {
        let cells = plan
            .blocks
            .iter()
            .flat_map(|b| b.stitches.iter())
            .map(|s| (s.x / LAYER_CELL, s.y / LAYER_CELL))
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .fold(None, |b: Option<[f64; 4]>, (x, y)| Some(b.map_or([x, y, x, y], |[x0, y0, x1, y1]| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)])));
        // A cell more on every side: a point along a stitch may round a hair past its ends.
        let grid = cells.and_then(|[x0, y0, x1, y1]| {
            let (x0, y0) = (x0.floor() - 1.0, y0.floor() - 1.0);
            let (width, height) = (x1.floor() + 2.0 - x0, y1.floor() + 2.0 - y0);
            (width * height <= MAX_LAYER_CELLS as f64).then_some((x0 as i64, y0 as i64, width as usize, height as usize))
        });
        match grid {
            Some((x0, y0, width, height)) => Self::Grid { x0, y0, width, count: vec![0; width * height], last: vec![NO_UNIT; width * height] },
            None => Self::Table(HashMap::new()),
        }
    }

    /// Count unit `unit` over `cell`: how many units have sewn over it, when this is the first time
    /// this unit is counted there; `None` when it was counted there already, or the cell is not
    /// counted.
    fn add(&mut self, cell: (i64, i64), unit: u32) -> Option<usize> {
        let (count, last) = match self {
            Self::Grid { x0, y0, width, count, last } => {
                let (x, y) = (usize::try_from(cell.0 - *x0).ok()?, usize::try_from(cell.1 - *y0).ok()?);
                let k = y.checked_mul(*width)?.checked_add(x).filter(|_| x < *width)?;
                (count.get_mut(k)?, last.get_mut(k)?)
            }
            Self::Table(cells) => {
                if cells.len() >= MAX_SPARSE_LAYER_CELLS && !cells.contains_key(&cell) {
                    return None;
                }
                let (count, last) = cells.entry(cell).or_insert((0, NO_UNIT));
                (count, last)
            }
        };
        if *last == unit {
            return None;
        }
        *last = unit;
        *count = count.saturating_add(1);
        Some(usize::from(*count))
    }
}

/// The samples a stitch `length` millimetres long is counted at for layers, besides its start: half
/// a cell apart, so that no cell it crosses is skipped, at most [`MAX_LAYER_SAMPLES`].
pub(super) fn layer_samples(length: f64) -> usize {
    ((length / (LAYER_CELL / 2.0)).ceil().max(1.0) as usize).min(MAX_LAYER_SAMPLES)
}

/// The cells of the grid layers are counted on for `plan`, `None` when its box is too large for
/// one (they are counted in a table).
#[cfg(test)]
pub(super) fn layer_grid_cells(plan: &DesignPlan) -> Option<usize> {
    match Layers::new(plan) {
        Layers::Grid { count, .. } => Some(count.len()),
        Layers::Table(_) => None,
    }
}

/// One finding: where (a record number and a point, millimetres), the unit, and a measure.
#[derive(Clone, Debug, Default)]
struct Hit {
    record: Option<usize>,
    at: Option<Point>,
    /// Where a stitch hit starts (its end is `at`).
    from: Option<Point>,
    /// The block a thread hit is about (from 1), and its thread as the check judged it.
    block: Option<usize>,
    thread: Option<Value>,
    /// The objects a hit about no stitch is about (`notSewn`: what the walk left out of them).
    objects: Option<Vec<u64>>,
    unit: Option<usize>,
    value: Option<f64>,
    /// A stitch's own length, when the value measures something else (`formatStep`).
    length: Option<f64>,
    /// The fewest and the most records a stitch is written as (`formatStep`).
    pieces: Option<[u64; 2]>,
    note: Option<String>,
}

impl Hit {
    /// A hit at record `r`, measuring `value`.
    fn at(r: &Record, value: f64) -> Self {
        Self {
            record: Some(r.number),
            at: Some(r.stitch.point()),
            from: r.before.map(|b| b.point()),
            unit: Some(r.unit),
            value: Some(value),
            ..Self::default()
        }
    }
}

/// A rule's findings: how many, every object hit, the first few hits, and the extreme measured
/// over everything it judged (the longest stitch for a rule on long stitches, the shortest for one
/// on short stitches), hit or not.
#[derive(Default)]
struct Found {
    count: usize,
    objects: BTreeSet<u64>,
    listed: Vec<Hit>,
    measured: Option<f64>,
    /// Its rule's extreme ([`Rule::extreme`]).
    extreme: Option<Extreme>,
}

/// Which extreme of the values a rule judges it reports ([`Rule::extreme`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Extreme {
    Largest,
    Smallest,
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
    /// The blocks' threads as the format asked for writes them ([`threads_judged`]).
    threads: Vec<BlockThread>,
    found: HashMap<&'static str, Found>,
    /// The short stitches in a row so far, and the first of them.
    short_run: Option<(usize, Record)>,
    /// The units sewn over each cell so far, and the last of them, when layers are counted.
    layers: Option<Layers>,
    /// The extremes over every stitch so far, for the rules' `measured`.
    extremes: Extremes,
}

/// The extremes measured over a design's stitches, kept apart from the findings (a stitch costs no
/// lookup), and given to their rules once the stitches are walked ([`Checker::rest`]).
#[derive(Default)]
struct Extremes {
    longest: Option<f64>,
    shortest: Option<f64>,
    /// The longest extent along x or y.
    step: Option<f64>,
    /// The most short stitches in a row.
    short_run: Option<f64>,
    /// The most units over one cell.
    layers: Option<f64>,
}

/// `m`, raised to `v` when `v` is larger (`more`) or lowered when smaller.
fn extreme(m: &mut Option<f64>, v: f64, more: bool) {
    if v.is_finite() {
        *m = Some(m.map_or(v, |m| if more { m.max(v) } else { m.min(v) }));
    }
}

impl<'a> Checker<'a> {
    fn new(plan: &'a DesignPlan, ask: &'a Ask) -> Self {
        let found = ask.rules.iter().map(|r| (r.id, Found { extreme: r.extreme, ..Found::default() })).collect();
        let layers = ask.rules.iter().any(|r| r.id == "layers").then(|| Layers::new(plan));
        Self { plan, ask, threads: threads_judged(plan, ask.format), found, short_run: None, layers, extremes: Extremes::default() }
    }

    /// Count `hit` for rule `id`, when it was asked for.
    fn add(&mut self, id: &'static str, hit: Hit) {
        let Some(f) = self.found.get_mut(id) else { return };
        f.count += 1;
        if let Some(u) = hit.unit.and_then(|u| self.plan.units.get(u)) {
            f.objects.insert(u.origin.selected().0);
        }
        if f.listed.len() < self.ask.limit {
            f.listed.push(hit);
        }
    }

    /// Note value `v` judged by rule `id`, hit or not: the extreme it reports as `measured`.
    fn judged(&mut self, id: &'static str, v: f64) {
        let Some(f) = self.found.get_mut(id) else { return };
        if let Some(e) = f.extreme {
            extreme(&mut f.measured, v, e == Extreme::Largest);
        }
    }

    /// Name the objects units `units` sew among those rule `id` hit.
    fn add_objects(&mut self, id: &'static str, units: std::ops::Range<usize>) {
        let plan = self.plan;
        let Some(f) = self.found.get_mut(id) else { return };
        f.objects.extend(units.filter_map(|u| plan.units.get(u)).map(|u| u.origin.selected().0));
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
        extreme(&mut self.extremes.longest, length, true);
        extreme(&mut self.extremes.shortest, length, false);
        if length > LONGEST_STITCH {
            self.add("stitchTooLong", Hit::at(r, length));
        }
        if let Some(before) = r.before {
            let step = self.ask.format.max_step();
            let longest = (r.stitch.x - before.x).abs().max((r.stitch.y - before.y).abs());
            extreme(&mut self.extremes.step, longest, true);
            if longest > step {
                let fewest = ((longest - GRID) / step).ceil().max(1.0);
                let most = ((longest + GRID) / step).ceil();
                let note = if fewest == most {
                    format!("written as {most} stitches")
                } else {
                    format!("written as {fewest} or {most} stitches: this close to a multiple of the step, rounding to the format's 0.1 mm decides")
                };
                // The measure compared with the step, and the stitch's length beside it.
                let pieces = [fewest, most].map(|n| n as u64);
                self.add("formatStep", Hit { length: Some(length), pieces: Some(pieces), note: Some(note), ..Hit::at(r, longest) });
            }
        }
        // "Longer than the blade": a stitch as long as the blade is flagged.
        if length <= NEEDLE_BLADE {
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
            && self.layers.is_some()
        {
            self.cover(before.point(), r);
        }
    }

    /// The end of a row of short stitches: a hit when it was longer than allowed.
    fn end_short_run(&mut self) {
        if let Some((n, first)) = self.short_run.take() {
            extreme(&mut self.extremes.short_run, n as f64, true);
            if n > MOST_SHORT_IN_A_ROW {
                self.add("shortStitchesInARow", Hit::at(&first, n as f64));
            }
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
        let samples = layer_samples(length);
        let unit = u32::try_from(r.unit).unwrap_or(NO_UNIT - 1);
        for i in 0..=samples {
            let p = a.lerp(b, i as f64 / samples as f64);
            let cell = ((p.x / LAYER_CELL).floor() as i64, (p.y / LAYER_CELL).floor() as i64);
            let Some(layers) = self.layers.as_mut() else { return };
            let Some(count) = layers.add(cell, unit) else { continue };
            extreme(&mut self.extremes.layers, count as f64, true);
            if count == LAYERS {
                let centre = Point::new((cell.0 as f64 + 0.5) * LAYER_CELL, (cell.1 as f64 + 0.5) * LAYER_CELL);
                self.add(
                    "layers",
                    Hit { record: Some(r.number), at: Some(centre), unit: Some(r.unit), value: Some(LAYERS as f64), ..Hit::default() },
                );
            }
        }
    }

    /// The rules on whole units (their settings), blocks and the design.
    fn rest(&mut self) {
        let plan = self.plan;
        let e = std::mem::take(&mut self.extremes);
        for (id, v) in [
            ("stitchTooLong", e.longest),
            ("stitchBelowNeedle", e.shortest),
            ("stitchTiny", e.shortest),
            ("formatStep", e.step),
            ("shortStitchesInARow", e.short_run),
            ("layers", e.layers),
        ] {
            if let Some(v) = v {
                self.judged(id, v);
            }
        }
        for (k, u) in plan.units.iter().enumerate() {
            // At the unit: its first stitch and needle point.
            // None for a unit that adds no record (one needle point where the needle already is):
            // the number after it is the next unit's.
            let first = plan.numbers(k).filter(|n| !n.is_empty()).map(|n| n.start);
            let at = plan.unit_stitches(k).iter().find(|s| s.command == Command::Stitch).map(|s| s.point());
            let hit = |value: f64| Hit { record: first, at, unit: Some(k), value: Some(value), ..Hit::default() };
            if let Some([narrowest, widest]) = u.satin_widths {
                self.judged("satinTooWide", widest);
                self.judged("satinStitchShort", narrowest);
                if widest > SATIN_WIDEST {
                    self.add("satinTooWide", hit(widest));
                }
                if narrowest < SATIN_SHORTEST {
                    self.add("satinStitchShort", hit(narrowest));
                }
            }
            // A literal run has no settings: it is sewn as its points are. A unit is judged by how
            // it is sewn: a fill under `stitchStyle: satin` is filled, with its fill stitch length.
            // A point set where the stitches can't meet it: where it is, and where the needle goes
            // in or out instead.
            let [entry, exit] = plan.missed(k);
            let needle = || plan.unit_stitches(k).iter().filter(|s| s.command == Command::Stitch).map(|s| s.point());
            for (asked, missed, from, note) in [(u.asked.entry, entry, needle().next(), "entry"), (u.asked.exit, exit, needle().next_back(), "exit")]
            {
                let Some(missed) = missed else { continue };
                self.judged("pointMissed", missed);
                if missed > POINT_MISS_MM {
                    self.add("pointMissed", Hit { at: asked, from, note: Some(note.to_string()), ..hit(missed) });
                }
            }
            match (u.sewn(), u.params) {
                (Some(Style::Fill), Some(p)) => {
                    self.judged("fillStitchShort", p.stitch_length);
                    if p.stitch_length < FILL_SHORTEST {
                        self.add("fillStitchShort", hit(p.stitch_length));
                    }
                }
                (Some(Style::Running), Some(p)) => {
                    self.judged("runStitchShort", p.stitch_length);
                    if p.stitch_length < RUN_SHORTEST {
                        self.add("runStitchShort", hit(p.stitch_length));
                    }
                }
                _ => {}
            }
        }
        let threads = std::mem::take(&mut self.threads);
        for (k, (b, thread)) in plan.blocks.iter().zip(&threads).enumerate() {
            self.judged("threadMismatch", thread.shown_delta_e());
            if thread.mismatch() {
                let note = format!("block {}: {} {} for the art's {}", k + 1, thread.palette().id(), thread.index(), hex(thread.art));
                let hit = Hit {
                    record: Some(b.first_number),
                    unit: Some(b.units.start),
                    value: Some(thread.shown_delta_e()),
                    block: Some(k + 1),
                    thread: Some(super::stats::thread_json(thread)),
                    note: Some(note),
                    ..Hit::default()
                };
                self.add("threadMismatch", hit);
                // Every object sewn in the thread is hit, not only the block's first.
                self.add_objects("threadMismatch", b.units.clone());
            }
        }
        for w in &plan.warnings {
            let objects: Vec<u64> = plan.warned.get(w).map(|ids| ids.iter().map(|id| id.0).collect()).unwrap_or_default();
            if let Some(f) = self.found.get_mut("notSewn") {
                f.objects.extend(&objects);
            }
            self.add("notSewn", Hit { note: Some(w.clone()), objects: Some(objects), ..Hit::default() });
        }
        let blocks = plan.blocks.len();
        let needle: usize = plan.blocks.iter().map(|b| b.needle_points()).sum();
        let changes = blocks.saturating_sub(1);
        for (id, v) in [("formatColors", blocks), ("machineStitches", needle), ("machineColorChanges", changes)] {
            self.judged(id, v as f64);
        }
        if let Some(most) = self.ask.format.max_colors()
            && blocks > most
        {
            let note = format!("{} holds {most}", self.ask.format.label());
            self.add("formatColors", Hit { value: Some(blocks as f64), note: Some(note), ..Hit::default() });
        }
        if needle as f64 > MACHINE_STITCHES {
            self.add("machineStitches", Hit { value: Some(needle as f64), ..Hit::default() });
        }
        if changes as f64 > MACHINE_COLOR_CHANGES {
            self.add("machineColorChanges", Hit { value: Some(changes as f64), ..Hit::default() });
        }
    }

    /// The findings, rule by rule in the order asked, and for each severity how many rules found
    /// something and how many hits they found.
    fn into_json(mut self) -> Value {
        // Rules and hits by severity: error, warning, info.
        let mut totals = [(0usize, 0usize); 3];
        let rules: Vec<Value> = self
            .ask
            .rules
            .iter()
            .map(|rule| {
                let f = self.found.remove(rule.id).unwrap_or_default();
                if f.count > 0 {
                    let at = match rule.severity {
                        Severity::Error => 0,
                        Severity::Warning => 1,
                        Severity::Info => 2,
                    };
                    if let Some((rules, hits)) = totals.get_mut(at) {
                        *rules += 1;
                        *hits += f.count;
                    }
                }
                let threshold = match rule.id {
                    "formatStep" => Some(self.ask.format.max_step()),
                    "formatColors" => self.ask.format.max_colors().map(|c| c as f64),
                    _ => rule.threshold,
                };
                let object_count = f.objects.len();
                let objects: Vec<u64> = f.objects.into_iter().take(MAX_OBJECTS).collect();
                json!({
                    "id": rule.id,
                    "title": rule.title,
                    "severity": rule.severity.id(),
                    "threshold": threshold.map(|t| rule.value(t)),
                    "unit": rule.unit,
                    "measures": rule.measures,
                    "sources": rule.sources.iter().map(|s| json!({"name": s.name, "url": s.url, "value": s.value})).collect::<Vec<_>>(),
                    "hits": f.count,
                    "objectCount": object_count,
                    "objects": objects,
                    "truncated": object_count > MAX_OBJECTS,
                    "examples": f.listed.iter().map(|h| self.hit_json(rule, h)).collect::<Vec<_>>(),
                    "measured": f.measured.map(|v| rule.value(v)),
                })
            })
            .collect();
        let [errors, warnings, info] = totals.map(|(rules, hits)| json!({"rules": rules, "hits": hits}));
        json!({
            "format": self.ask.format.id(),
            "palette": self.ask.format.palette().map_or_else(|| self.plan.blocks.first().map(|b| b.thread.palette().id()), |p| Some(p.id())),
            "rules": rules,
            "summary": {"errors": errors, "warnings": warnings, "info": info},
            "notChecked": NOT_CHECKED.iter().map(|(id, what, why)| json!({"id": id, "what": what, "why": why})).collect::<Vec<_>>(),
        })
    }

    fn hit_json(&self, rule: &Rule, h: &Hit) -> Value {
        let id = h.unit.and_then(|u| self.plan.units.get(u)).map(|u| u.origin.selected().0);
        json!({
            "record": h.record,
            "at": h.at.map(point),
            "from": h.from.map(point),
            "block": h.block,
            "thread": h.thread,
            "objects": h.objects,
            "value": h.value.map(|v| rule.value(v)),
            "length": h.length.map(mm),
            "pieces": h.pieces,
            "id": id,
            "note": h.note,
        })
    }
}

/// What Melco's design checker flags that is not checked here, and why.
const NOT_CHECKED: [(&str, &str, &str); 3] = [
    ("density", "density too tight", "Melco's design checker names it without a threshold"),
    ("penetrations", "too many needle penetrations in an area", "Melco's design checker names it without a threshold"),
    ("ties", "too many or too few tie stitches (missed trims or starts)", "Melco's design checker names it without a threshold"),
];

/// The threads a check judges: those the format writes, matched to its own palette when it numbers
/// its threads from one other than the plan's (a PES of a document in Janome's palette writes
/// Brother's threads); the plan's for a format that names no threads.
fn threads_judged(plan: &DesignPlan, format: Format) -> Vec<BlockThread> {
    match format.palette() {
        Some(palette) => plan.threads_in(palette),
        None => plan.blocks.iter().map(|b| b.thread.clone()).collect(),
    }
}

/// Run `ask`'s rules on `plan` → each rule with its findings, how many rules of each severity
/// found something, and what is not checked.
pub(crate) fn check(plan: &DesignPlan, ask: &Ask) -> Value {
    let mut c = Checker::new(plan, ask);
    measure::each_until_cancelled(plan, |r| c.record(r));
    c.end_short_run();
    c.rest();
    c.into_json()
}

/// The rule `id` names.
pub(crate) fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.id == id)
}
