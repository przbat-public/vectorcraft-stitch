//! A design's figures (`embroidery.stats`): its counts, size, stitch lengths, the thread it uses
//! and how long it takes to sew, for the whole design and each block.
//!
//! The thread and the time are estimates on models whose shape follows what Wilcom documents for
//! its own estimates, each number a named constant with its source or marked as an estimate:
//! - **Thread** ([`Thread`]): Wilcom's thread usage takes the fabric's thickness as an input and a
//!   bobbin factor by the design's stitches, 100 % for a mix, about 125 % for a design all of run
//!   or tatami stitches, about 65 % for one all of satin
//!   (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Basics/threads/threads-5.htm>),
//!   and gives no formula. Here each needle-down stitch lays its length of top thread on the face
//!   of the fabric; on the back, its length of top thread pulled under, a share of it under a
//!   satin as thread makers tell a balanced stitch ([`BOBBIN_SHARE_SATIN`]) and the other styles in
//!   proportion to Wilcom's factors; and every penetration takes top thread down through the
//!   fabric and back up ([`PASSES`]). The bobbin thread is a share of the top thread: Madeira's
//!   figures for standard stitches, about 3 m of bobbin to 5 m of top thread a 1,000 stitches
//!   ([`BOBBIN_PER_TOP`]).
//! - **Time** ([`Machine`]): Wilcom times a design with a machine's normal speed, a slow speed for
//!   stitches longer than a set length, and the time a trim, a colour change, a stop, and the
//!   design's start and end take
//!   (<https://docs.wilcom.com/embroiderystudio/e4/en/MainHelp/Setup/hardware/Configure_machine_runtime_settings.htm>),
//!   and gives no values. Here each needle point and jump is one turn of the machine at the
//!   normal or slow speed, and each trim takes [`TRIM_SECONDS`]. Thread changes are counted apart
//!   and not timed: on a home single-needle machine a person rethreads, which takes as long as it
//!   takes. Brother's machines leave thread changes out of their own estimate too, and trims as
//!   well (<https://support.brother.com/g/s/hf/htmldoc/hsm/om/888p5x/en/06_08_01.html>), so their
//!   display differs from this one by the trims. The start and the end of the design are not timed.
//!   Both count the plan's records: a file may hold more (a DST trim is three jump records, a move
//!   longer than one record holds is several, the VP3 writer adds a zero-length stitch after each
//!   long move), which `embroidery.check`'s `formatStep` counts for a stitch.

use serde_json::{Map, Value, json};
use vectorcraft_geom::{Point, Rect};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::Style;

use super::check::LONGEST_STITCH;
use super::measure::{self, Record};
use super::plan::{BlockThread, DesignPlan};

/// The speed a design is timed at when none is given, stitches a minute: a home single-needle
/// machine sewing at its default setting. The default of Janome's Memory Craft 230E's
/// maximum-speed setting, which runs from 400 to 650 spm
/// (<https://www.janome.ca/wp-content/uploads/2025/02/inst-manual-mc230e.pdf>; the manual also
/// advises slowing down for small designs); Brother's EV1 manual suggests 600 spm for one task
/// (<https://download.brother.com/welcome/doch103134/evugk1_om01en.pdf>), and commercial machines
/// usually run at 700 to 800 spm (Wilcom's blog cited for [`TRIM_SECONDS`]). An estimate for home
/// machines in general.
pub(crate) const SPEED: f64 = 650.0;
/// The speed stitches and jumps longer than [`SLOW_ABOVE`] are timed at, stitches a minute: the
/// lowest Janome's home machines can be set to (400 spm, the same manual). How much a home machine
/// slows for a long stitch is not published: an estimate.
const SLOW_SPEED: f64 = 400.0;
/// Stitches and jumps longer than this (millimetres) are timed at [`SLOW_SPEED`]. Tajima's
/// TMEZ-SC keeps its top speed up to 4 mm stitches (<https://www.tajima.com/product/tmez-sc/>),
/// an industrial machine's figure: an estimate for home machines.
const SLOW_ABOVE: f64 = 4.0;
/// How long a trim takes, seconds: 3 to 4 seconds depending on the machine, as Wilcom's blog
/// puts it (<https://wilcom.com/resources/blog/high-impact-low-stitch-count-designs>). The middle.
const TRIM_SECONDS: f64 = 3.5;
/// The speeds `speed` may be given, stitches a minute: from 60 (the slowest embroidery speed of
/// Janome's Memory Craft 9850, <https://www.janome.com/product/memory-craft-9850/>) to 1,500 (Happy's
/// HCU2-1501 single-head industrial machine, a dealer's listing:
/// <https://shop.coldesi.com/products/happyjapan-hcu2-1501-embroidery-machine-hcu2-1501>). Bounds on
/// what is asked, not settings.
const SPEED_RANGE: (f64, f64) = (60.0, 1500.0);

/// The fabric's thickness when none is given (millimetres): about that of a medium cotton or a
/// T-shirt jersey. An estimate; Wilcom asks for it and gives no default.
pub(crate) const FABRIC_THICKNESS: f64 = 0.5;
/// The thicknesses `fabricThickness` may be given (millimetres): none to 5 mm, past what a home
/// embroidery machine sews through. Bounds on what is asked.
const FABRIC_THICKNESS_RANGE: (f64, f64) = (0.0, 5.0);
/// How many times the fabric's thickness of top thread each penetration takes: down through the
/// fabric to the bobbin thread and back up. An estimate.
const PASSES: f64 = 2.0;
/// The share of the back of a satin stitch that is bobbin thread: a third, the rest top thread
/// pulled under, the balance thread makers tell embroiderers to set the tension to (Madeira,
/// <https://www.madeira.com/fileadmin/user_upload/Downloads/HowTo/EN_Avoid_Puckering.pdf>;
/// American & Efird,
/// <http://www.amefird.com/wp-content/uploads/2010/02/EmbroideryUnqualityMatrix-2-11-10.pdf>).
const BOBBIN_SHARE_SATIN: f64 = 1.0 / 3.0;
/// Wilcom's bobbin factors (the thread usage page cited above): a design of mixed stitches, one all
/// of run or tatami stitches, one all of satin. Their ratios scale [`BOBBIN_SHARE_SATIN`] to the
/// other styles.
const BOBBIN_FACTOR_MIXED: f64 = 1.0;
const BOBBIN_FACTOR_RUN_OR_TATAMI: f64 = 1.25;
const BOBBIN_FACTOR_SATIN: f64 = 0.65;
/// The bobbin thread a design takes, as a share of its top thread: Madeira's rule of thumb for
/// standard-length stitches, about 3 m of bobbin and 5 m of upper thread a 1,000 stitches (density
/// sheet, <https://www.madeirausa.com/_resources/common/userfiles/file/Resources/TH.Reads/Density.pdf>).
/// The project owner's choice of calibration (2026-10-22); still an estimate. Wilcom's bobbin
/// factors alone (125 % for fills) would give the real designs' plans about 4 m of bobbin thread a
/// 1,000 needle points, against Madeira's 3; with this share they take 2.8 to 3.4 m.
const BOBBIN_PER_TOP: f64 = 3.0 / 5.0;

/// What the thread figures rest on, as the answer says it: an estimate.
const THREAD_ESTIMATE: &str = "an estimate: top thread, each needle-down stitch's length on the face, the share of its length on the back that is top thread pulled under (two thirds under a satin, the balance thread makers teach, scaled by Wilcom's bobbin factors for the other stitches), and twice fabricThickness at every needle point; bobbin thread, 3/5 of the top thread (Madeira's 3 m to 5 m a 1,000 standard stitches)";
/// What the time rests on, as the answer says it: an estimate.
const TIME_ESTIMATE: &str = "an estimate: each needle point and jump of the plan one turn of a home machine at speed (650 by default, a home machine's default maximum), slowSpeed past slowAbove mm, and trimSeconds a trim; not timed: thread changes, the design's start and end; a file may hold more records than the plan (a DST trim is three, a long move several)";

/// Millimetres in a metre.
const MM_PER_M: f64 = 1000.0;
/// Seconds in a minute.
const SECONDS_PER_MINUTE: f64 = 60.0;
/// Lengths are given to the hundredth of a millimetre (finer than the 0.1 mm the formats count
/// in), metres of thread to the centimetre, times to the second and minutes to the tenth.
const HUNDREDTHS: f64 = 100.0;
const TENTHS: f64 = 10.0;

/// The machine a design is timed on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Machine {
    /// Stitches a minute.
    pub(crate) speed: f64,
}

/// The fabric a design's thread is estimated for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Thread {
    /// Millimetres.
    pub(crate) fabric: f64,
}

/// The share of a needle-down stitch's length on the back that is bobbin thread, the rest top
/// thread pulled under, by how its unit is sewn (`None`: a literal run, whose style is not known: a
/// mix).
fn bobbin_share(style: Option<Style>) -> f64 {
    let factor = match style {
        Some(Style::Satin) => BOBBIN_FACTOR_SATIN,
        Some(Style::Running | Style::Fill) => BOBBIN_FACTOR_RUN_OR_TATAMI,
        None => BOBBIN_FACTOR_MIXED,
    };
    BOBBIN_SHARE_SATIN * factor / BOBBIN_FACTOR_SATIN
}

/// What a stretch of a design holds and takes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Tally {
    /// Needle points (jumps and cuts are counted apart).
    pub(crate) needle: usize,
    pub(crate) jumps: usize,
    pub(crate) trims: usize,
    /// The shortest and longest needle-down stitch, the longest jump (millimetres).
    pub(crate) shortest: Option<f64>,
    pub(crate) longest: Option<f64>,
    pub(crate) longest_jump: Option<f64>,
    /// Top and bobbin thread, millimetres.
    pub(crate) top: f64,
    pub(crate) bobbin: f64,
    /// Seconds.
    pub(crate) seconds: f64,
}

impl Tally {
    /// Count record `r`: a needle point, a jump or a cut.
    fn count(&mut self, r: &Record) {
        match r.stitch.command {
            Command::Stitch => self.needle += 1,
            Command::Jump => self.jumps += 1,
            Command::Trim => self.trims += 1,
            Command::ColorChange | Command::Stop => {}
        }
    }

    /// Count record `r`, of a unit sewn in `style`, and measure it: the stitch it lays, the
    /// thread it takes and the time it takes.
    fn add(&mut self, r: &Record, style: Option<Style>, machine: Machine, thread: Thread) {
        self.count(r);
        let turn = |length: f64| SECONDS_PER_MINUTE / if length > SLOW_ABOVE { SLOW_SPEED.min(machine.speed) } else { machine.speed };
        match r.stitch.command {
            Command::Stitch => {
                self.seconds += turn(r.moved());
                let mut top = PASSES * thread.fabric;
                if let Some(length) = r.sewn() {
                    self.shortest = Some(self.shortest.map_or(length, |s| s.min(length)));
                    self.longest = Some(self.longest.map_or(length, |s| s.max(length)));
                    top += length * (2.0 - bobbin_share(style));
                }
                self.top += top;
                self.bobbin += top * BOBBIN_PER_TOP;
            }
            Command::Jump => {
                let length = r.moved();
                self.longest_jump = Some(self.longest_jump.map_or(length, |s| s.max(length)));
                self.seconds += turn(length);
            }
            Command::Trim => self.seconds += TRIM_SECONDS,
            Command::ColorChange | Command::Stop => {}
        }
    }

    fn merge(&mut self, o: &Tally) {
        let pick = |a: Option<f64>, b: Option<f64>, f: fn(f64, f64) -> f64| match (a, b) {
            (Some(x), Some(y)) => Some(f(x, y)),
            (x, y) => x.or(y),
        };
        self.needle += o.needle;
        self.jumps += o.jumps;
        self.trims += o.trims;
        self.shortest = pick(self.shortest, o.shortest, f64::min);
        self.longest = pick(self.longest, o.longest, f64::max);
        self.longest_jump = pick(self.longest_jump, o.longest_jump, f64::max);
        self.top += o.top;
        self.bobbin += o.bobbin;
        self.seconds += o.seconds;
    }
}

/// The figures of each block of `plan`, in sewing order, and of each unit.
pub(crate) fn tally(plan: &DesignPlan, machine: Machine, thread: Thread) -> (Vec<Tally>, Vec<Tally>) {
    // How each unit is sewn, worked out once (not for every record: it resolves the unit's
    // parameters).
    let styles: Vec<Option<Style>> = (0..plan.units.len()).map(|u| measure::style(plan, u)).collect();
    tally_with(plan, |t, r| t.add(r, styles.get(r.unit).copied().flatten(), machine, thread))
}

/// The needle points, jumps and cuts of each block of `plan` and of each unit, without measuring
/// them.
pub(crate) fn counts(plan: &DesignPlan) -> (Vec<Tally>, Vec<Tally>) {
    tally_with(plan, Tally::count)
}

/// Each block's and each unit's tally, each record taken in by `take`.
fn tally_with(plan: &DesignPlan, mut take: impl FnMut(&mut Tally, &Record)) -> (Vec<Tally>, Vec<Tally>) {
    let mut blocks = vec![Tally::default(); plan.blocks.len()];
    let mut units = vec![Tally::default(); plan.units.len()];
    measure::each_until_cancelled(plan, |r| {
        if let Some(t) = units.get_mut(r.unit) {
            take(t, r);
        }
    });
    for (k, b) in plan.blocks.iter().enumerate() {
        if let (Some(total), Some(of)) = (blocks.get_mut(k), units.get(b.units.clone())) {
            for u in of {
                total.merge(u);
            }
        }
    }
    (blocks, units)
}

/// `v` millimetres to the hundredth.
pub(crate) fn mm(v: f64) -> f64 {
    (v * HUNDREDTHS).round() / HUNDREDTHS
}

/// `v` millimetres of thread as metres, to the centimetre.
fn metres(v: f64) -> f64 {
    (v / MM_PER_M * HUNDREDTHS).round() / HUNDREDTHS
}

/// A point as `[x, y]` millimetres.
pub(crate) fn point(p: Point) -> Value {
    json!([mm(p.x), mm(p.y)])
}

/// A box as `[left, top, right, bottom]` millimetres.
pub(crate) fn rect(r: Rect) -> Value {
    json!([mm(r.x0), mm(r.y0), mm(r.x1), mm(r.y1)])
}

/// A block's thread as every command lists it: the palette thread (`palette`, `index` from 1 as
/// the format numbers it, `name`, `color`), the colour the art is painted with, their CIEDE2000
/// difference as shown, and whether it is past the mismatch threshold.
pub(crate) fn thread_json(t: &BlockThread) -> Value {
    let hex = super::hex;
    json!({"palette": t.palette().id(), "index": t.index(), "name": t.name(), "color": hex(t.color()), "art": hex(t.art), "deltaE": t.shown_delta_e(), "mismatch": t.mismatch()})
}

/// The largest whole number a JSON number holds exactly (2^53).
const EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// `x` as a JSON number: a whole number as an integer (`800`, not `800.0`), any other as it is.
pub(crate) fn number(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < EXACT_INTEGER { json!(x as i64) } else { json!(x) }
}

/// A stretch of a list a command answers with: `offset` items skipped, at most `limit` listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) offset: usize,
    pub(crate) limit: usize,
}

/// The items a list holds when `limit` is left out: a whole design of ordinary size, in a reply
/// of a few hundred kilobytes at most.
pub(crate) const LIST_LIMIT: usize = 1000;
/// The most items `limit` may ask for: the reply stays bounded (a few megabytes) whatever the
/// design.
pub(crate) const MAX_LIST_LIMIT: usize = 10_000;

impl Page {
    /// The keys a command that answers with a list takes for it.
    pub(crate) const KEYS: [&'static str; 2] = ["offset", "limit"];

    /// The stretch `p` asks for (`offset`, default 0; `limit`, default [`LIST_LIMIT`], at most
    /// [`MAX_LIST_LIMIT`]); an error naming `cmd` for another value.
    pub(crate) fn from_params(p: &Map<String, Value>, cmd: &str) -> crate::Result<Self> {
        let whole = |key: &str, default: usize, most: usize| -> crate::Result<usize> {
            match p.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n <= most)
                    .ok_or_else(|| crate::cmd::bad(cmd, format!("`{key}` {v}: a whole number from 0 to {most}"))),
            }
        };
        Ok(Self { offset: whole("offset", 0, usize::MAX)?, limit: whole("limit", LIST_LIMIT, MAX_LIST_LIMIT)? })
    }

    /// The indices of a list of `len` items it lists.
    pub(crate) fn range(self, len: usize) -> std::ops::Range<usize> {
        let start = self.offset.min(len);
        start..start.saturating_add(self.limit).min(len)
    }

    /// Whether it leaves out items of a list of `len` after the ones it lists.
    pub(crate) fn truncated(self, len: usize) -> bool {
        self.range(len).end < len
    }
}

/// The machine and the fabric `p` asks for (`speed`, `fabricThickness`), or the defaults; an
/// error naming `cmd` for a value out of its range.
pub(crate) fn model(p: &Map<String, Value>, cmd: &str) -> crate::Result<(Machine, Thread)> {
    let number = |key: &str, default: f64, (lo, hi): (f64, f64), unit: &str| -> crate::Result<f64> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v
                .as_f64()
                .filter(|x| x.is_finite() && (lo..=hi).contains(x))
                .ok_or_else(|| crate::cmd::bad(cmd, format!("`{key}` {v}: a number of {unit} from {lo} to {hi}"))),
        }
    };
    let speed = number("speed", SPEED, SPEED_RANGE, "stitches a minute")?;
    let fabric = number("fabricThickness", FABRIC_THICKNESS, FABRIC_THICKNESS_RANGE, "millimetres")?;
    Ok((Machine { speed }, Thread { fabric }))
}

/// The width of the stitch-length chart's bins, millimetres. An estimate: half a millimetre shows
/// where a design's stitches gather (most are sewn 2 to 4 mm long) in a chart a panel wide.
pub(crate) const LENGTH_BIN_MM: f64 = 0.5;
/// The half-millimetre bins, up to 12.5 mm; then one up to the longest stitch the check allows
/// ([`LONGEST_STITCH`], 12.7 mm, inclusive), and one for every longer stitch, which holds exactly
/// the `stitchTooLong` hits.
const EVEN_BINS: usize = 25;
pub(crate) const LENGTH_BINS: usize = EVEN_BINS + 2;
/// The resolution the median is found at, millimetres.
const MEDIAN_STEP_MM: f64 = 0.1;
/// The median's bins: a tenth of a millimetre each up to 13 mm, and one for longer stitches.
const MEDIAN_BINS: usize = 131;

/// Needle-down stitch lengths counted into the chart's bins, and finely for the median.
struct Lengths {
    bins: [u64; LENGTH_BINS],
    fine: Vec<u64>,
    count: u64,
}

impl Default for Lengths {
    fn default() -> Self {
        Self { bins: [0; LENGTH_BINS], fine: vec![0; MEDIAN_BINS], count: 0 }
    }
}

impl Lengths {
    fn add(&mut self, length: f64) {
        if !length.is_finite() || length < 0.0 {
            return;
        }
        // As the check compares: too long is longer than the longest allowed.
        let bin = if length > LONGEST_STITCH { EVEN_BINS + 1 } else { ((length / LENGTH_BIN_MM).floor() as usize).min(EVEN_BINS) };
        let fine = ((length / MEDIAN_STEP_MM).floor() as usize).min(MEDIAN_BINS - 1);
        if let (Some(b), Some(f)) = (self.bins.get_mut(bin), self.fine.get_mut(fine)) {
            *b += 1;
            *f += 1;
            self.count += 1;
        }
    }

    /// `{bins: [{from, to, count}], median}`: the last bin's `to` is null (and longer); the median
    /// to the tenth of a millimetre (the middle of its tenth), null with no stitch.
    fn to_json(&self) -> Value {
        let edge = |k: usize| match k {
            k if k <= EVEN_BINS => Some(k as f64 * LENGTH_BIN_MM),
            k if k == EVEN_BINS + 1 => Some(LONGEST_STITCH),
            _ => None,
        };
        let bins: Vec<Value> =
            self.bins.iter().enumerate().map(|(k, n)| json!({"from": edge(k).map(mm), "to": edge(k + 1).map(mm), "count": n})).collect();
        let mut seen = 0;
        let half = self.count.div_ceil(2);
        let median = (self.count > 0)
            .then(|| {
                self.fine.iter().position(|n| {
                    seen += n;
                    seen >= half
                })
            })
            .flatten()
            .map(|k| mm((k as f64 + 0.5) * MEDIAN_STEP_MM));
        json!({"bins": bins, "median": median})
    }
}

/// The design's figures ([`tally`]) as `embroidery.stats` gives them, its blocks those `page`
/// lists.
pub(crate) fn stats(plan: &DesignPlan, machine: Machine, thread: Thread, page: Page) -> Value {
    let (blocks, _) = tally(plan, machine, thread);
    let mut total = Tally::default();
    for b in &blocks {
        total.merge(b);
    }
    // The needle points: the design's box, where it starts and ends; the stitches' lengths.
    let mut bounds: Option<Rect> = None;
    let (mut start, mut end): (Option<Point>, Option<Point>) = (None, None);
    let mut lengths = Lengths::default();
    measure::each_until_cancelled(plan, |r| {
        if let Some(length) = r.sewn() {
            lengths.add(length);
        }
        let p = r.stitch.point();
        if r.stitch.command != Command::Stitch || !(p.x.is_finite() && p.y.is_finite()) {
            return;
        }
        let one = Rect::from_points(p, p);
        bounds = Some(bounds.map_or(one, |b| b.union(one)));
        start = start.or(Some(p));
        end = Some(p);
    });
    let extents = match (bounds, start) {
        (Some(b), Some(s)) => json!({"left": mm(s.x - b.x0), "right": mm(b.x1 - s.x), "up": mm(s.y - b.y0), "down": mm(b.y1 - s.y)}),
        _ => Value::Null,
    };
    let mut threads: Vec<(&str, usize)> = plan.blocks.iter().map(|b| (b.thread.palette().id(), b.thread.index())).collect();
    threads.sort_unstable();
    threads.dedup();
    let listed = page.range(plan.blocks.len());
    let per_block: Vec<Value> = listed
        .clone()
        .filter_map(|k| Some((k, plan.blocks.get(k)?, blocks.get(k)?)))
        .map(|(k, b, t)| {
            json!({
                "block": k + 1,
                "thread": thread_json(&b.thread),
                "stitches": t.needle,
                "records": b.stitches.len(),
                "jumps": t.jumps,
                "trims": t.trims,
                "firstRecord": b.first_number,
                "lastRecord": b.last_number(),
                "topThreadMetres": metres(t.top),
                "bobbinThreadMetres": metres(t.bobbin),
                "seconds": t.seconds.round() as u64,
            })
        })
        .collect();
    let color_changes = plan.blocks.len().saturating_sub(1);
    json!({
        "stitches": total.needle,
        "records": plan.len(),
        "jumps": total.jumps,
        "trims": total.trims,
        "colorChanges": color_changes,
        "threadCount": threads.len(),
        "blockCount": plan.blocks.len(),
        "size": bounds.map_or(Value::Null, |b| json!({"width": mm(b.width()), "height": mm(b.height())})),
        "bounds": bounds.map_or(Value::Null, rect),
        "start": start.map_or(Value::Null, point),
        "end": end.map_or(Value::Null, point),
        "extents": extents,
        "shortestStitch": total.shortest.map(mm),
        "longestStitch": total.longest.map(mm),
        "longestJump": total.longest_jump.map(mm),
        "lengths": lengths.to_json(),
        "threadUsage": {
            "topMetres": metres(total.top),
            "bobbinMetres": metres(total.bobbin),
            "fabricThickness": number(thread.fabric),
            "estimate": THREAD_ESTIMATE,
        },
        "time": {
            "seconds": total.seconds.round() as u64,
            "minutes": (total.seconds / SECONDS_PER_MINUTE * TENTHS).round() / TENTHS,
            "speed": number(machine.speed),
            "slowSpeed": number(SLOW_SPEED.min(machine.speed)),
            "slowAbove": number(SLOW_ABOVE),
            "trimSeconds": number(TRIM_SECONDS),
            "includes": "needle points and jumps at speed (slowSpeed past slowAbove mm) and trims; not the colour changes (colorChanges, each a stop to rethread), nor the start and end of the design",
            "estimate": TIME_ESTIMATE,
        },
        "blocks": per_block,
        "offset": listed.start,
        "truncated": page.truncated(plan.blocks.len()),
        "warnings": plan.messages(),
    })
}
