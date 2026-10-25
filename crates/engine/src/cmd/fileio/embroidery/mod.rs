//! Embroidery files: `document.open` reads a design's stitches as paths, `document.export`
//! stitches the document's art and writes a machine file (DST, TAP, T01, EXP, PES, PEC, JEF or
//! VP3).
//!
//! Reading draws each colour block as a group of thin lines in the thread's colour, so the design
//! looks like it will sew, and can be measured, traced over or recoloured. Writing takes every
//! visible filled or stroked path and plans its stitches: fills with a tatami fill (their narrow
//! parts as satin columns that follow the shape), strokes with a satin column when they are wide
//! enough (1 mm) and a running line when not, `stitch` overrides that. Type, images and live art
//! are not stitched (their outlines would be needed); each is listed in the export's warnings.
//! Objects of the same art colour are sewn together where nothing lies between them, so the
//! machine changes colour as few times as the artwork allows.
//!
//! Options (`cmd::embroidery::options`, where their earlier names are also taken): `stitch`
//! (`auto`, `tatami` or `fill`, `satin`, `running`), `spacing` (mm between rows or rail crossings),
//! `length` (mm), `angle` (degrees), `pull` (mm at each side of satin columns and fill rows, one
//! value or one per side), `pullPercent` (the same, a share of each stitch), `push` (mm off each
//! end of satin columns), `expand` (mm all round a fill), `narrowAsSatin` (mm: a fill's narrower
//! parts are satin columns), `threadWeight` (wt, the spacing follows it), `underlay` (true, false or
//! a kind), `jumpLength` (mm before the needle lifts), `trimLength` (mm before a jump cuts the
//! thread), `minStitch` (mm) and `splitLength` (mm: a longer satin stitch is split). A value
//! outside its range is refused.
//!
//! The parts: [`import`] (a design read as art, with notes of what reading it left out, which
//! make writing over the file ask first: [`is_loss`]) and [`encode`] (the export: its options, and
//! the stitches `cmd::embroidery` plans from the document's art, written in the format).

mod import;

use serde_json::Value;
use vectorcraft_doc::Document;
use vectorcraft_stitch::format::Format as StitchFormat;

use super::super::*;
use super::{Encoded, Format, FormatOption};
use crate::cmd::embroidery::options::Others;
use crate::cmd::embroidery::{Options, PlanError, service};

pub use import::LOSS_NOTES;
pub(super) use import::import;

/// Whether `note`, said when a file was opened, says that reading a machine file left something
/// out ([`LOSS_NOTES`]): writing over that file then asks first.
pub(super) fn is_loss(note: &str) -> bool {
    LOSS_NOTES.contains(&note)
}

const C: &str = "document.export";

const STITCH: FormatOption = FormatOption {
    name: "stitch",
    ty: "string",
    default: "\"auto\"",
    description: "how the art is stitched: auto (fills tatami, strokes 1 mm wide or more satin, narrower ones running), tatami or fill (a fill tatami, a stroke the area it paints filled), satin or running; earlier name stitchStyle",
};
// Defaults are the stitch planner's (`plan::Params::for_style`): an option left out keeps the
// style's own value. Those that every style that uses them shares are given as JSON defaults
// (checked against the planner by `defaults_are_the_planners`); the stitch length differs by
// style, so its default is `null` and its description says which. Each range is the planner's
// (`plan::*_RANGE`, checked by `ranges_are_the_planners`): a value outside it is refused.
const SPACING: FormatOption = FormatOption {
    name: "spacing",
    ty: "number",
    default: "0.38",
    description: "millimetres between threads: between the rows of a fill; between two penetrations on one rail of a satin column (peak to peak: its crossings lie half that apart). Left out, a satin column picks it by its width: 0.5 under 2 mm, 0.45 under 4, 0.4 under 6, 0.37 wider; 0.05 to 5; earlier name density",
};
const LENGTH: FormatOption = FormatOption {
    name: "length",
    ty: "number",
    default: "null",
    description: "the longest stitch of a running line or a fill row, millimetres (default: 4 in fills, 2.5 in running lines); 0.2 to 12; earlier name stitchLength",
};
const ANGLE: FormatOption = FormatOption {
    name: "angle",
    ty: "number",
    default: "135",
    description: "the direction a fill's rows run, degrees counter-clockwise on the page as every angle in VectorCraft (0: left to right, 90: bottom to top; the default runs from the top left to the bottom right; any angle, taken modulo 360); earlier name stitchAngle, which keeps its earlier meaning: degrees clockwise as seen, the same rows at 180 − angle (until 2026-10-24 angle counted so too)",
};
const PULL: FormatOption = FormatOption {
    name: "pull",
    ty: "number|array",
    default: "0.2",
    description: "pull compensation: how much farther the stitches reach at each side so the fabric's pull leaves no gap, millimetres: each rail of a satin column, each end of a fill's rows. One value for both sides, or two ([left, right] of a satin as its path runs, [start, end] of a fill's rows); 0 to 2; earlier name pullCompensation",
};
const PULL_PERCENT: FormatOption = FormatOption {
    name: "pullPercent",
    ty: "number|array",
    default: "0",
    description: "pull compensation that grows with the stitch, on top of pull: percent of a satin column's width, or of a fill row's length, added at each side (at most 2 mm a side with pull); one value or two, as pull; 0 to 50; earlier name pullCompensationPercent",
};
const PUSH: FormatOption = FormatOption {
    name: "push",
    ty: "number|array",
    default: "0",
    description: "push compensation: how much shorter an open satin column is sewn at each end, millimetres, since sewing pushes the fabric out there (a column keeps at least half its length, and a column long enough to sew stays so); one value or two ([start, end] as its path is drawn); 0 to 5; earlier name pushCompensation",
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
    description: "a filled shape's parts narrower than this are sewn as satin columns that follow the shape (the rest stays tatami), millimetres; 0 sews every fill as tatami, as stitch tatami does; 0 to 12",
};
const WEIGHT: FormatOption = FormatOption {
    name: "threadWeight",
    ty: "number",
    default: "40",
    description: "the thread's weight (wt, kilometres per kilogram: 40 is standard embroidery thread, a larger number a finer thread): the default spacing of fills and satin columns follows its thickness (0.44 mm rows for 30 wt, 0.31 for 60); 10 to 150",
};
const UNDERLAY: FormatOption = FormatOption {
    name: "underlay",
    ty: "boolean",
    default: "true",
    description: "what is sewn under the art first: true (auto: under a satin column a centre run, an edge run from 2 mm wide, an edge run and a zigzag from 4.5 mm; under a fill an edge run, and sparse tatami when it is at least 10 mm across), false (none), or one kind: none, auto, center, edge, edgeZigzag (a satin's; a fill takes edgeTatami), edgeTatami (a fill's, whatever its size; a satin takes edgeZigzag)",
};
const JUMP: FormatOption = FormatOption {
    name: "jumpLength",
    ty: "number",
    default: "5",
    description: "a move longer than this lifts the needle (a jump) instead of being sewn, tied off before and in after so it can be cut, millimetres; 0.5 to 20",
};
const TRIM: FormatOption = FormatOption {
    name: "trimLength",
    ty: "number",
    default: "10",
    description: "a jump longer than this cuts the thread (a trim); shorter ones are left for the machine or a person to cut, millimetres; from jumpLength to 1000",
};
const MIN_STITCH: FormatOption =
    FormatOption { name: "minStitch", ty: "number", default: "0.4", description: "shorter stitches are left out, millimetres; 0.05 to 3" };
const SPLIT: FormatOption = FormatOption {
    name: "splitLength",
    ty: "number",
    default: "10",
    description: "a satin stitch longer than this is split into shorter ones, their needle points spread so they don't line up into a seam, millimetres; 2 to 12",
};

/// The options the embroidery writers read.
pub const OPTIONS: &[FormatOption] =
    &[STITCH, SPACING, LENGTH, ANGLE, PULL, PULL_PERCENT, PUSH, EXPAND, NARROW, WEIGHT, UNDERLAY, JUMP, TRIM, MIN_STITCH, SPLIT];

/// The entry of `f` in the formats `document.open` and `document.export` know: as the stitch crate
/// describes it ([`StitchFormat::spec`]), read and written, with the embroidery options.
pub(super) const fn format(f: StitchFormat) -> Format {
    let s = f.spec();
    Format { id: s.id, label: s.label, extensions: s.extensions, mime: s.mime, read: true, write: true, raster: false, options: OPTIONS }
}

/// Whether `id` names an embroidery format: one the stitch crate reads and writes.
pub(crate) fn is_format(id: &str) -> bool {
    StitchFormat::from_id(id).is_some()
}

/// `file.formatOptions`' value of an option of format `id`: for an embroidery format, unset
/// (`null`), whatever was saved or is the default. Its stitches take the objects' settings, then
/// the document's, then the defaults; an option given to one export is for that export alone
/// (the project owner's choice, 2026-10-23), so no value is offered for every export to copy.
pub(crate) fn unset_option(id: &str, value: &mut Value) {
    if is_format(id) {
        *value = Value::Null;
    }
}

/// The embroidery options in the export's params `p`, checked: the export's own keys (and other
/// formats' options) pass, a key that looks like a misspelt embroidery option is refused.
pub(crate) fn options(p: &Value) -> Result<Options> {
    match p {
        Value::Object(o) => Options::from_object(o, Others::PassUnlessNear, C),
        Value::Null => Ok(Options::default()),
        _ => Err(bad(C, "params must be an object")),
    }
}

/// Write a machine file from the document's visible art: its stitch plan, as the format writes it.
pub(super) fn encode(doc: &Document, f: &Format, p: &Value) -> Result<Encoded> {
    let opts = options(p)?;
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad(C, format!("{} is not an embroidery format", f.label)))?;
    let plan = service::plan_afresh(doc, &opts).map_err(PlanError::error)?;
    if plan.is_empty() {
        // What was left out says why, when something was. The document's, not the parameters'.
        return Err(EngineError::Other(if plan.messages().is_empty() {
            "nothing to stitch: the document has no visible filled or stroked paths".to_string()
        } else {
            format!("nothing to stitch: {}", plan.messages().join("; "))
        }));
    }
    // What the format can't hold is the design's limit, not a wrong parameter.
    let warnings = plan.messages();
    let bytes = format.write(&plan.into_pattern()).map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(Encoded { warnings, ..Encoded::one(bytes) })
}

#[cfg(test)]
mod tests {
    use vectorcraft_stitch::plan::{
        DENSITY_RANGE, JUMP_RANGE, MAX_TRIM, MIN_STITCH_RANGE, NARROW_RANGE, PULL_PERCENT_RANGE, PULL_RANGE, PUSH_RANGE, Params, STITCH_LENGTH_RANGE,
        Style, THREAD_WEIGHT, THREAD_WEIGHT_RANGE, Underlay,
    };

    use super::*;
    use crate::cmd::embroidery::options::{EXPAND_DEFAULT, MAX_EXPAND};

    /// The defaults `document.formats` lists are the planner's, for every style that uses them.
    #[test]
    fn defaults_are_the_planners() {
        let num = |o: &FormatOption| o.default.parse::<f64>().unwrap();
        let (fill, satin, running) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin), Params::for_style(Style::Running));
        assert_eq!(num(&SPACING), fill.density);
        assert_eq!(num(&SPACING), satin.density);
        // The satin spacing by width the description gives is the planner's.
        for (w, spacing) in [(1.0, "0.5 under 2"), (3.0, "0.45 under 4"), (5.0, "0.4 under 6"), (9.0, "0.37 wider")] {
            assert!(
                SPACING.description.contains(spacing) && spacing.starts_with(&vectorcraft_stitch::plan::satin_spacing(w).to_string()),
                "{spacing}"
            );
        }
        // A density given turns the satin's auto spacing off.
        let given = Options { spacing: Some(0.3), ..Options::default() };
        assert!(Options::default().params(Style::Satin, 3.0).auto_spacing);
        assert!(!given.params(Style::Satin, 3.0).auto_spacing);
        // Counter-clockwise on the page, the planner's clockwise: the default rows keep their bits.
        assert_eq!(num(&ANGLE), crate::cmd::embroidery::options::planner_angle(fill.angle));
        assert_eq!(crate::cmd::embroidery::options::planner_angle(num(&ANGLE)).to_bits(), fill.angle.to_bits());
        assert_eq!(Options { angle: Some(num(&ANGLE)), ..Options::default() }.params(Style::Fill, 0.0), fill.sane());
        for side in 0..2 {
            assert_eq!(num(&PULL), satin.pull[side]);
            assert_eq!(num(&PULL), fill.pull[side]);
            assert_eq!(num(&PULL_PERCENT), satin.pull_percent[side]);
            assert_eq!(num(&PULL_PERCENT), fill.pull_percent[side]);
            assert_eq!(num(&PUSH), satin.push[side]);
        }
        assert_eq!(num(&EXPAND), EXPAND_DEFAULT);
        assert!(EXPAND.description.contains(&format!("-{MAX_EXPAND} to {MAX_EXPAND}")));
        assert_eq!(num(&WEIGHT), THREAD_WEIGHT);
        assert_eq!(num(&NARROW), fill.narrow);
        assert_eq!((satin.narrow, running.narrow), (0.0, 0.0));
        assert!(NARROW.description.contains("0 to 12"));
        assert_eq!(Options { narrow_as_satin: Some(99.0), ..Options::default() }.params(Style::Fill, 0.0).narrow, 12.0);
        // The spacings the thread weight's description gives are the planner's.
        for (weight, rows) in [(30.0, "0.44 mm rows for 30 wt"), (60.0, "0.31 for 60")] {
            let spacing = Params::for_thread(Style::Fill, weight).density;
            assert!(WEIGHT.description.contains(rows) && rows.starts_with(&format!("{spacing:.2}")), "{rows}: {spacing}");
        }
        assert_eq!(UNDERLAY.default == "true", fill.underlay == Underlay::Auto && satin.underlay == Underlay::Auto);
        for p in [fill, satin, running] {
            assert_eq!(num(&JUMP), p.jump);
            assert_eq!(num(&TRIM), p.trim);
            assert_eq!(num(&MIN_STITCH), p.min_stitch);
            assert_eq!(num(&SPLIT), p.max_satin);
        }
        assert!(LENGTH.description.contains(&format!("{} in fills", fill.stitch_length)));
        assert!(LENGTH.description.contains(&format!("{} in running lines", running.stitch_length)));
        // Nothing given: each style plans with its own values.
        let o = Options::default();
        assert_eq!(o.params(Style::Fill, 0.0), fill.sane());
        assert_eq!(o.params(Style::Running, 0.0), running.sane());
    }

    /// A field edits every option, and whatever it can write is taken: each choice, and each number
    /// at both ends of its range (a negative `expand` among them).
    #[test]
    fn a_field_writes_only_what_the_options_take() {
        use crate::cmd::embroidery::{SettingField, setting_field};
        for o in OPTIONS {
            let field = setting_field(o.name).unwrap_or_else(|| panic!("{}", o.name));
            let values: Vec<Value> = match field {
                SettingField::Choice(ids) => ids.iter().map(|id| serde_json::json!(id)).collect(),
                SettingField::Number { min, max, .. } => vec![serde_json::json!(min), serde_json::json!(max)],
            };
            for v in values {
                let mut p = serde_json::Map::new();
                p.insert(o.name.into(), v.clone());
                // A trim length is at least the jump length: with the shortest trim, the shortest jump.
                if o.name == "trimLength" {
                    p.insert("jumpLength".into(), serde_json::json!(JUMP_RANGE.0));
                }
                assert!(options(&Value::Object(p)).is_ok(), "{} {v}", o.name);
            }
        }
        assert!(matches!(setting_field("expand"), Some(SettingField::Number { min, .. }) if min < 0.0));
        assert_eq!(setting_field("format"), None);
        // The document's own: Document Setup writes them, and they are taken.
        for name in ["spacingPercent", "speed", "thickness"] {
            assert!(matches!(setting_field(name), Some(SettingField::Number { .. })), "{name}");
        }
    }

    /// Compensation takes one value for both sides or one per side, applies to the styles that
    /// have it, and anything else is refused with a message.
    #[test]
    fn compensation_options_take_one_value_or_two() {
        let o = options(&serde_json::json!({"pull": [0.3, 0.1], "pullCompensationPercent": 5, "push": [0.4, 0.2]})).unwrap();
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
        // `expand` out of its range is refused; a value that is not a number grows nothing.
        assert!(options(&serde_json::json!({"expand": 9})).is_err());
        assert_eq!(options(&serde_json::json!({"expand": -2})).unwrap().expand(), -MAX_EXPAND);
        assert_eq!(Options { expand: Some(f64::NAN), ..Options::default() }.expand(), 0.0);
    }

    /// Each option's range in `document.formats` is the planner's: its ends are accepted, a value
    /// just past them refused with the range named, instead of being clamped.
    #[test]
    fn ranges_are_the_planners() {
        let ranges = [
            (&SPACING, DENSITY_RANGE),
            (&LENGTH, STITCH_LENGTH_RANGE),
            (&PULL, PULL_RANGE),
            (&PULL_PERCENT, PULL_PERCENT_RANGE),
            (&PUSH, PUSH_RANGE),
            (&EXPAND, (-MAX_EXPAND, MAX_EXPAND)),
            (&NARROW, NARROW_RANGE),
            (&WEIGHT, THREAD_WEIGHT_RANGE),
            (&JUMP, JUMP_RANGE),
            (&MIN_STITCH, MIN_STITCH_RANGE),
            (&SPLIT, vectorcraft_stitch::plan::MAX_SATIN_STITCH_RANGE),
        ];
        for (o, (lo, hi)) in ranges {
            assert!(o.description.contains(&format!("{lo} to {hi}")), "{}: {}", o.name, o.description);
            for ok in [lo, hi] {
                assert!(options(&serde_json::json!({ o.name: ok })).is_ok(), "{} {ok}", o.name);
            }
            for past in [lo - 0.01, hi + 0.01] {
                let e = options(&serde_json::json!({ o.name: past })).unwrap_err().to_string();
                assert!(e.contains(&format!("from {lo} to {hi}")), "{} {past}: {e}", o.name);
            }
        }
        assert!(TRIM.description.contains(&format!("from jumpLength to {MAX_TRIM}")));
        assert!(options(&serde_json::json!({"trimLength": 3})).is_err(), "less than the default jump length");
        assert!(options(&serde_json::json!({"trimLength": 3, "jumpLength": 2})).is_ok());
    }

    /// The earlier names are taken as well, a value under both names must agree, null leaves an
    /// option to its default, `underlay` takes a kind, and a key that looks like a misspelt option
    /// is refused while the export's own keys pass.
    #[test]
    fn earlier_names_and_kinds_are_taken() {
        let old = options(&serde_json::json!({"stitchStyle": "satin", "density": 0.5, "stitchLength": 3, "stitchAngle": 30, "pullCompensation": 0.3, "pullCompensationPercent": 10, "pushCompensation": 0.4})).unwrap();
        // `stitchAngle` keeps its earlier meaning, clockwise: 30 is `angle` 150.
        let new =
            options(&serde_json::json!({"stitch": "satin", "spacing": 0.5, "length": 3, "angle": 150, "pull": 0.3, "pullPercent": 10, "push": 0.4}))
                .unwrap();
        assert_eq!(old, new);
        assert!(options(&serde_json::json!({"angle": 150, "stitchAngle": 30})).is_ok(), "the same rows under both names");
        assert!(options(&serde_json::json!({"angle": 30, "stitchAngle": 30})).is_err());
        assert_eq!(new.to_json()["stitch"], serde_json::json!("satin"));
        assert!(options(&serde_json::json!({"spacing": 0.5, "density": 0.4})).is_err());
        assert!(options(&serde_json::json!({"spacing": 0.5, "density": 0.5})).is_ok());
        let unset = options(&serde_json::json!({"stitch": null, "spacing": null, "underlay": null})).unwrap();
        assert_eq!(unset, Options::default());
        assert_eq!(options(&serde_json::json!({"stitch": "tatami"})).unwrap().forced(), Some(Style::Fill));
        for (v, kind) in [
            (serde_json::json!(true), Underlay::Auto),
            (serde_json::json!(false), Underlay::None),
            (serde_json::json!("edgeZigzag"), Underlay::EdgeZigzag),
        ] {
            assert_eq!(options(&serde_json::json!({ "underlay": v })).unwrap().params(Style::Satin, 3.0).underlay, kind);
        }
        assert!(options(&serde_json::json!({"underlay": "zigzag"})).is_err());
        let e = options(&serde_json::json!({"format": "dst", "path": "x.dst", "densty": 0.5})).unwrap_err().to_string();
        assert!(e.contains("did you mean the embroidery option `density`"), "{e}");
        assert!(options(&serde_json::json!({"format": "dst", "path": "x.dst", "selectedOnly": true, "artboard": 0})).is_ok());
    }
}
