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
//!   of the fabric; on the back, its length is shared between the bobbin thread and the top thread
//!   pulled under, a third of it bobbin under a satin, as thread makers tell a balanced stitch
//!   ([`BOBBIN_SHARE_SATIN`]), and the other styles in proportion to Wilcom's factors; and every
//!   penetration takes top thread down through the fabric and back up ([`PASSES`]).
//! - **Time** ([`Machine`]): Wilcom times a design with a machine's normal speed, a slow speed for
//!   stitches longer than a set length, and the time a trim, a colour change, a stop, and the
//!   design's start and end take
//!   (<https://docs.wilcom.com/embroiderystudio/e4/en/MainHelp/Setup/hardware/Configure_machine_runtime_settings.htm>),
//!   and gives no values. Here each needle point and jump is one turn of the machine at the
//!   normal or slow speed, and each trim takes [`TRIM_SECONDS`]. Thread changes are counted apart
//!   and not timed: on a home single-needle machine a person rethreads, which takes as long as it
//!   takes. Brother's machines leave thread changes out of their own estimate too
//!   (<https://support.brother.com/g/s/hf/htmldoc/hsm/om/888p5x/en/06_08_01.html>). The start and
//!   the end of the design are not timed.

use serde_json::{Map, Value, json};
use vectorcraft_geom::{Point, Rect};
use vectorcraft_stitch::Command;
use vectorcraft_stitch::plan::Style;

use super::measure::{self, Record};
use super::plan::{BlockThread, DesignPlan};

/// The speed a design is timed at when none is given, stitches a minute: a home single-needle
/// machine sewing at its default setting. Janome's Memory Craft 230E sews from 400 to 650 spm and
/// defaults to 650
/// (<https://www.janome.ca/wp-content/uploads/2025/02/inst-manual-mc230e.pdf>, as do its 200E,
/// 300E and 350E); Brother's EV1 manual suggests 600 spm for one task
/// (<https://download.brother.com/welcome/doch103134/evugk1_om01en.pdf>). An estimate for home
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
/// The speeds `speed` may be given, stitches a minute: from 60 (the slowest setting of Janome's
/// Memory Craft 9850) to 1,500 (Happy's fastest single-head industrial machine). Bounds on what is
/// asked, not settings.
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

/// What the thread figures rest on, as the answer says it: an estimate.
const THREAD_ESTIMATE: &str = "an estimate: each needle-down stitch's length on the face, its length on the back shared between bobbin and top thread (a third bobbin under a satin, the balance thread makers teach, scaled by Wilcom's bobbin factors for the other stitches), and twice fabricThickness of top thread at every needle point";
/// What the time rests on, as the answer says it: an estimate.
const TIME_ESTIMATE: &str = "an estimate: each needle point and jump one turn of a home machine at speed (a default setting, 650 by default), slowSpeed past slowAbove mm, and trimSeconds a trim; not timed: thread changes, the design's start and end";

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

/// The share of a needle-down stitch's length laid on the back as bobbin thread, by how its unit
/// is sewn (`None`: a literal run, whose style is not known: a mix).
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
    /// Needle points, jumps and cuts.
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
                self.top += PASSES * thread.fabric;
                if let Some(length) = r.sewn() {
                    self.shortest = Some(self.shortest.map_or(length, |s| s.min(length)));
                    self.longest = Some(self.longest.map_or(length, |s| s.max(length)));
                    let share = bobbin_share(style);
                    self.top += length * (2.0 - share);
                    self.bobbin += length * share;
                }
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
    tally_with(plan, |t, r| t.add(r, measure::style(plan, r.unit), machine, thread))
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
    measure::each(plan, |r| {
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

/// A block's thread as the commands list it.
pub(crate) fn thread_json(t: &BlockThread) -> Value {
    let hex = super::hex;
    json!({"palette": t.palette.id(), "index": t.index, "name": t.name, "color": hex(t.color), "art": hex(t.art), "deltaE": t.shown_delta_e(), "mismatch": t.mismatch()})
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

/// The design's figures ([`tally`]) as `embroidery.stats` gives them.
pub(crate) fn stats(plan: &DesignPlan, machine: Machine, thread: Thread) -> Value {
    let (blocks, _) = tally(plan, machine, thread);
    let mut total = Tally::default();
    for b in &blocks {
        total.merge(b);
    }
    // The needle points: the design's box, where it starts and ends.
    let mut bounds: Option<Rect> = None;
    let (mut start, mut end): (Option<Point>, Option<Point>) = (None, None);
    measure::each(plan, |r| {
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
    let mut threads: Vec<(&str, usize)> = plan.blocks.iter().map(|b| (b.thread.palette.id(), b.thread.index)).collect();
    threads.sort_unstable();
    threads.dedup();
    let per_block: Vec<Value> = plan
        .blocks
        .iter()
        .zip(&blocks)
        .enumerate()
        .map(|(k, (b, t))| {
            json!({
                "block": k + 1,
                "thread": thread_json(&b.thread),
                "stitches": t.needle,
                "jumps": t.jumps,
                "trims": t.trims,
                "first": b.first_number,
                "last": b.first_number + b.stitches.len().saturating_sub(1),
                "topThread": metres(t.top),
                "bobbinThread": metres(t.bobbin),
                "seconds": t.seconds.round(),
            })
        })
        .collect();
    json!({
        "stitches": total.needle,
        "records": plan.len(),
        "jumps": total.jumps,
        "trims": total.trims,
        "colorChanges": plan.blocks.len().saturating_sub(1),
        "threads": threads.len(),
        "blocks": plan.blocks.len(),
        "size": bounds.map_or(Value::Null, |b| json!({"width": mm(b.width()), "height": mm(b.height())})),
        "bounds": bounds.map_or(Value::Null, rect),
        "start": start.map_or(Value::Null, point),
        "end": end.map_or(Value::Null, point),
        "extents": extents,
        "shortestStitch": total.shortest.map(mm),
        "longestStitch": total.longest.map(mm),
        "longestJump": total.longest_jump.map(mm),
        "thread": {"top": metres(total.top), "bobbin": metres(total.bobbin), "fabricThickness": thread.fabric, "estimate": THREAD_ESTIMATE},
        "time": {
            "seconds": total.seconds.round(),
            "minutes": (total.seconds / SECONDS_PER_MINUTE * TENTHS).round() / TENTHS,
            "speed": machine.speed,
            "slowSpeed": SLOW_SPEED.min(machine.speed),
            "slowAbove": SLOW_ABOVE,
            "trimSeconds": TRIM_SECONDS,
            "threadChanges": plan.blocks.len().saturating_sub(1),
            "includes": "needle points and jumps at speed (slowSpeed past slowAbove mm) and trims; not the thread changes (threadChanges, each a stop to rethread), nor the start and end of the design",
            "estimate": TIME_ESTIMATE,
        },
        "perBlock": per_block,
        "warnings": plan.warnings,
    })
}
