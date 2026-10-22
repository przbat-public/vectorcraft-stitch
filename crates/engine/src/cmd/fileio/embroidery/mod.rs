//! Embroidery files: `document.open` reads a design's stitches as paths, `document.export`
//! stitches the document's art and writes a machine file (DST, TAP, T01, EXP, PES, PEC, JEF or
//! VP3).
//!
//! Reading draws each colour block as a group of thin lines in the thread's colour, so the design
//! looks like it will sew, and can be measured, traced over or recoloured. Writing takes every
//! visible filled or stroked path and plans its stitches: fills with a tatami fill, strokes with
//! a satin column when they are wide enough (1 mm) and a running line when not, `stitchStyle`
//! overrides that. Type, images and live art are not stitched (their outlines would be needed);
//! each is listed in the export's warnings. Blocks of the same thread are sewn together, so the
//! machine changes colour as few times as the artwork allows.
//!
//! Options: `stitchStyle` (`auto`, `fill`, `satin`, `running`), `density` (mm between rows or rail
//! crossings), `stitchLength` (mm), `stitchAngle` (degrees), `pullCompensation` (mm at each side of
//! satin columns and fill rows, one value or one per side), `pullCompensationPercent` (the same, a
//! share of each stitch), `pushCompensation` (mm off each end of satin columns), `expand` (mm all
//! round a fill), `narrowAsSatin` (mm: a fill's narrower parts are satin columns), `threadWeight`
//! (wt, the spacing follows it), `underlay`, `jumpLength` (mm before
//! the needle lifts), `trimLength` (mm before a jump cuts the thread) and `minStitch` (mm).
//!
//! The parts: [`import`] (a design read as art) and [`encode`] (the export: its options, and the
//! stitches `cmd::embroidery` plans from the document's art, written in the format).

mod import;

use serde_json::Value;
use vectorcraft_doc::Document;
use vectorcraft_stitch::format::Format as StitchFormat;

use super::super::*;
use super::{Encoded, Format, FormatOption};
use crate::cmd::embroidery::{Options, service};

pub(super) use import::import;

const C: &str = "document.export";

const STYLE: FormatOption = FormatOption {
    name: "stitchStyle",
    ty: "string",
    default: "\"auto\"",
    description: "how the art is stitched: auto (fills filled, wide strokes satin, narrow ones running), fill, satin or running",
};
// Defaults are the stitch planner's (`plan::Params::for_style`): an option left out keeps the
// style's own value. Those that every style that uses them shares are given as JSON defaults
// (checked against the planner by `defaults_are_the_planners`); the stitch length differs by
// style, so its default is `null` and its description says which.
const DENSITY: FormatOption = FormatOption {
    name: "density",
    ty: "number",
    default: "0.38",
    description: "millimetres between threads: between the rows of a fill; between two penetrations on one rail of a satin column (peak to peak: its crossings lie half that apart). Left out, a satin column picks it by its width: 0.5 under 2 mm, 0.45 under 4, 0.4 under 6, 0.37 wider",
};
const STITCH_LENGTH: FormatOption = FormatOption {
    name: "stitchLength",
    ty: "number",
    default: "null",
    description: "the longest stitch of a running line or a fill row, millimetres (default: 4 in fills, 2.5 in running lines)",
};
const ANGLE: FormatOption =
    FormatOption { name: "stitchAngle", ty: "number", default: "45", description: "the direction a fill's rows run, degrees (0: left to right)" };
const PULL: FormatOption = FormatOption {
    name: "pullCompensation",
    ty: "number",
    default: "0.2",
    description: "how much farther the stitches reach at each side so the fabric's pull leaves no gap, millimetres: each rail of a satin column, each end of a fill's rows. One value for both sides, or two ([left, right] of a satin as its path runs, [start, end] of a fill's rows); 0 to 2 (a negative value counts as 0)",
};
const PULL_PERCENT: FormatOption = FormatOption {
    name: "pullCompensationPercent",
    ty: "number",
    default: "0",
    description: "pull compensation that grows with the stitch, on top of pullCompensation: percent of a satin column's width, or of a fill row's length, added at each side (at most 2 mm a side with pullCompensation); one value or two, as pullCompensation; 0 to 50",
};
const PUSH: FormatOption = FormatOption {
    name: "pushCompensation",
    ty: "number",
    default: "0",
    description: "how much shorter an open satin column is sewn at each end, millimetres, since sewing pushes the fabric out there (a column keeps at least half its length, and a column long enough to sew stays so); one value or two ([start, end] as its path is drawn); 0 to 5 (a negative value counts as 0)",
};
const EXPAND: FormatOption = FormatOption {
    name: "expand",
    ty: "number",
    default: "0",
    description: "how far a filled shape grows all round before it is stitched, millimetres, so that neighbouring shapes meet without a gap (negative: it shrinks); -2 to 2",
};
const NARROW: FormatOption = FormatOption {
    name: "narrowAsSatin",
    ty: "number",
    default: "7",
    description: "a filled shape's parts narrower than this are sewn as satin columns that follow the shape (the rest stays tatami), millimetres; 0 sews every fill as tatami, as stitchStyle fill does; 0 to 12",
};
const THREAD_WEIGHT_OPTION: FormatOption = FormatOption {
    name: "threadWeight",
    ty: "number",
    default: "40",
    description: "the thread's weight (wt, kilometres per kilogram: 40 is standard embroidery thread, a larger number a finer thread): the default spacing of fills and satin columns follows its thickness (0.44 mm rows for 30 wt, 0.31 for 60); 10 to 150",
};
const UNDERLAY: FormatOption = FormatOption {
    name: "underlay",
    ty: "boolean",
    default: "true",
    description: "sew an underlay first: an edge run under fills, a centre run under satin columns",
};
const JUMP: FormatOption = FormatOption {
    name: "jumpLength",
    ty: "number",
    default: "5",
    description: "a move longer than this lifts the needle (a jump) instead of being sewn, tied off before and in after so it can be cut, millimetres",
};
const TRIM: FormatOption = FormatOption {
    name: "trimLength",
    ty: "number",
    default: "10",
    description: "a jump longer than this cuts the thread (a trim); shorter ones are left for the machine or a person to cut, millimetres; never less than jumpLength (a smaller value acts as jumpLength)",
};
const MIN_STITCH: FormatOption =
    FormatOption { name: "minStitch", ty: "number", default: "0.4", description: "shorter stitches are left out, millimetres" };

/// The options the embroidery writers read.
pub const OPTIONS: &[FormatOption] =
    &[STYLE, DENSITY, STITCH_LENGTH, ANGLE, PULL, PULL_PERCENT, PUSH, EXPAND, NARROW, THREAD_WEIGHT_OPTION, UNDERLAY, JUMP, TRIM, MIN_STITCH];

/// The entry of `f` in the formats `document.open` and `document.export` know: as the stitch crate
/// describes it ([`StitchFormat::spec`]), read and written, with the embroidery options.
pub(super) const fn format(f: StitchFormat) -> Format {
    let s = f.spec();
    Format { id: s.id, label: s.label, extensions: s.extensions, mime: s.mime, read: true, write: true, raster: false, options: OPTIONS }
}

/// Whether `id` names an embroidery format: one the stitch crate reads and writes.
pub(super) fn is_format(id: &str) -> bool {
    StitchFormat::from_id(id).is_some()
}

/// The options in `p`, checked.
fn options(p: &Value) -> Result<Options> {
    Options::from_params(p, C)
}

/// Write a machine file from the document's visible art: its stitch plan, as the format writes it.
pub(super) fn encode(doc: &Document, f: &Format, p: &Value) -> Result<Encoded> {
    let opts = options(p)?;
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad(C, format!("{} is not an embroidery format", f.label)))?;
    let plan = service::plan_afresh(doc, &opts).map_err(|e| e.error(C))?;
    if plan.is_empty() {
        // What was left out says why, when something was.
        return Err(bad(
            C,
            if plan.warnings.is_empty() {
                "nothing to stitch: the document has no visible filled or stroked paths".to_string()
            } else {
                format!("nothing to stitch: {}", plan.warnings.join("; "))
            },
        ));
    }
    let bytes = format.write(&plan.to_pattern()).map_err(|e| bad(C, e.to_string()))?;
    Ok(Encoded { warnings: plan.warnings, ..Encoded::one(bytes) })
}

#[cfg(test)]
mod tests {
    use vectorcraft_stitch::plan::{Params, Style, THREAD_WEIGHT};

    use super::*;
    use crate::cmd::embroidery::options::{EXPAND_DEFAULT, MAX_EXPAND};

    /// The defaults `document.formats` lists are the planner's, for every style that uses them.
    #[test]
    fn defaults_are_the_planners() {
        let num = |o: &FormatOption| o.default.parse::<f64>().unwrap();
        let (fill, satin, running) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin), Params::for_style(Style::Running));
        assert_eq!(num(&DENSITY), fill.density);
        assert_eq!(num(&DENSITY), satin.density);
        // The satin spacing by width the description gives is the planner's.
        for (w, spacing) in [(1.0, "0.5 under 2"), (3.0, "0.45 under 4"), (5.0, "0.4 under 6"), (9.0, "0.37 wider")] {
            assert!(
                DENSITY.description.contains(spacing) && spacing.starts_with(&vectorcraft_stitch::plan::satin_spacing(w).to_string()),
                "{spacing}"
            );
        }
        // A density given turns the satin's auto spacing off.
        let given = Options { density: Some(0.3), ..Options::default() };
        assert!(Options::default().params(Style::Satin, 3.0).auto_spacing);
        assert!(!given.params(Style::Satin, 3.0).auto_spacing);
        assert_eq!(num(&ANGLE), fill.angle);
        for side in 0..2 {
            assert_eq!(num(&PULL), satin.pull[side]);
            assert_eq!(num(&PULL), fill.pull[side]);
            assert_eq!(num(&PULL_PERCENT), satin.pull_percent[side]);
            assert_eq!(num(&PULL_PERCENT), fill.pull_percent[side]);
            assert_eq!(num(&PUSH), satin.push[side]);
        }
        assert_eq!(num(&EXPAND), EXPAND_DEFAULT);
        assert!(EXPAND.description.contains(&format!("-{MAX_EXPAND} to {MAX_EXPAND}")));
        assert_eq!(num(&THREAD_WEIGHT_OPTION), THREAD_WEIGHT);
        assert_eq!(num(&NARROW), fill.narrow);
        assert_eq!((satin.narrow, running.narrow), (0.0, 0.0));
        assert!(NARROW.description.contains("0 to 12"));
        assert_eq!(Options { narrow_as_satin: Some(99.0), ..Options::default() }.params(Style::Fill, 0.0).narrow, 12.0);
        // The spacings the thread weight's description gives are the planner's.
        for (weight, rows) in [(30.0, "0.44 mm rows for 30 wt"), (60.0, "0.31 for 60")] {
            let spacing = Params::for_thread(Style::Fill, weight).density;
            assert!(THREAD_WEIGHT_OPTION.description.contains(rows) && rows.starts_with(&format!("{spacing:.2}")), "{rows}: {spacing}");
        }
        assert_eq!(UNDERLAY.default == "true", fill.underlay && satin.underlay);
        for p in [fill, satin, running] {
            assert_eq!(num(&JUMP), p.jump);
            assert_eq!(num(&TRIM), p.trim);
            assert_eq!(num(&MIN_STITCH), p.min_stitch);
        }
        assert!(STITCH_LENGTH.description.contains(&format!("{} in fills", fill.stitch_length)));
        assert!(STITCH_LENGTH.description.contains(&format!("{} in running lines", running.stitch_length)));
        // Nothing given: each style plans with its own values.
        let o = Options::default();
        assert_eq!(o.params(Style::Fill, 0.0), fill.sane());
        assert_eq!(o.params(Style::Running, 0.0), running.sane());
    }

    /// Compensation takes one value for both sides or one per side, applies to the styles that
    /// have it, and anything else is refused with a message.
    #[test]
    fn compensation_options_take_one_value_or_two() {
        let o = options(&serde_json::json!({"pullCompensation": [0.3, 0.1], "pullCompensationPercent": 5, "pushCompensation": [0.4, 0.2]})).unwrap();
        let (satin, fill, running) = (o.params(Style::Satin, 3.0), o.params(Style::Fill, 0.0), o.params(Style::Running, 0.0));
        assert_eq!((satin.pull, satin.pull_percent, satin.push), ([0.3, 0.1], [5.0, 5.0], [0.4, 0.2]));
        assert_eq!((fill.pull, fill.pull_percent, fill.push), ([0.3, 0.1], [5.0, 5.0], [0.0, 0.0]), "a fill has no column ends to push");
        assert_eq!((running.pull, running.pull_percent, running.push), ([0.0; 2], [0.0; 2], [0.0; 2]), "a running line has no compensation");
        let o = options(&serde_json::json!({"pullCompensation": 0.25})).unwrap();
        assert_eq!(o.params(Style::Satin, 3.0).pull, [0.25, 0.25]);
        for bad in [serde_json::json!({"pullCompensation": "wide"}), serde_json::json!({"pushCompensation": [1, 2, 3]})] {
            let e = options(&bad).unwrap_err().to_string();
            assert!(e.contains("a number, or two numbers"), "{e}");
        }
        // The thread's weight sets the spacing a satin and a fill default to; a density given wins.
        let fine = options(&serde_json::json!({"threadWeight": 60})).unwrap();
        assert_eq!(fine.params(Style::Fill, 0.0).density, Params::for_thread(Style::Fill, 60.0).density);
        assert_eq!(fine.params(Style::Satin, 3.0).thread_weight, 60.0);
        let given = options(&serde_json::json!({"threadWeight": 60, "density": 0.5})).unwrap();
        assert_eq!(given.params(Style::Fill, 0.0).density, 0.5);
        // `expand` is clamped and a value that is not a number grows nothing.
        assert_eq!(options(&serde_json::json!({"expand": 9})).unwrap().expand(), MAX_EXPAND);
        assert_eq!(options(&serde_json::json!({"expand": -9})).unwrap().expand(), -MAX_EXPAND);
        assert_eq!(Options { expand: Some(f64::NAN), ..Options::default() }.expand(), 0.0);
    }
}
