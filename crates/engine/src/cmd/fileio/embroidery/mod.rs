//! Embroidery files: `document.open` reads a design's stitches as paths, `document.export`
//! stitches the document's art and writes a machine file (DST, EXP, PES or JEF).
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
//! crossings), `stitchLength` (mm), `stitchAngle` (degrees), `pullCompensation` (mm, satin),
//! `underlay`, `jumpLength` (mm before the needle lifts), `trimLength` (mm before a jump cuts the
//! thread) and `minStitch` (mm).
//!
//! The parts: [`import`] (a design read as art), [`digitize`] (the document's art planned as
//! stitches) and [`sequence`] (the order the blocks are sewn in).

mod digitize;
mod import;
mod sequence;

use serde::Deserialize;
use serde_json::Value;
use vectorcraft_doc::Document;
use vectorcraft_doc::embroidery::Embroidery;
use vectorcraft_stitch::Pattern;
use vectorcraft_stitch::format::Format as StitchFormat;
use vectorcraft_stitch::plan::{Params, Style};

use super::super::*;
use super::{Encoded, Format, FormatOption};

pub(super) use import::import;

/// The embroidery settings an object carries.
pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "embroidery.setTrimAfter",
        "Trim After",
        [],
        None,
        "{on: bool, ids?|id?} whether the thread is cut after each object (or group) in ids or the selection when the document is stitched (document.export to dst, exp, pes or jef), however near the next object starts; off, only a jump longer than the export's trimLength is cut. One undo step; saved with the document → {changed}",
        has_doc,
        set_trim_after
    )]
}

fn set_trim_after(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.setTrimAfter";
    let on = p.get("on").and_then(Value::as_bool).ok_or_else(|| bad(C, "`on` must be true or false"))?;
    let ids = targets(s, p)?;
    let changed = super::super::overprint::edit_counted(s, "Trim After", |d| {
        let mut changed = 0;
        for id in &ids {
            let Some(n) = d.node_mut(*id) else { return Err(bad(C, format!("no object {}", id.0))) };
            let mut e = n.extras().embroidery.as_deref().cloned().unwrap_or_default();
            if e.trim_after == on {
                continue;
            }
            e.trim_after = on;
            n.extras_mut().embroidery = (e != Embroidery::default()).then(|| Box::new(e));
            changed += 1;
        }
        Ok(changed)
    })?;
    Ok(serde_json::json!({ "changed": changed }))
}

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
    description: "millimetres between neighbouring threads: rows of a fill, or the crossings of a satin column (left out, a satin column picks it by its width: 0.5 under 2 mm, 0.45 under 4, 0.4 under 6, 0.37 wider)",
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
    description: "how much wider satin columns are stitched so the fabric's pull leaves no gap, millimetres",
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
pub const OPTIONS: &[FormatOption] = &[STYLE, DENSITY, STITCH_LENGTH, ANGLE, PULL, UNDERLAY, JUMP, TRIM, MIN_STITCH];

/// The export options, as they come: each one left out keeps the style's own value.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    stitch_style: String,
    density: Option<f64>,
    stitch_length: Option<f64>,
    stitch_angle: Option<f64>,
    pull_compensation: Option<f64>,
    underlay: Option<bool>,
    jump_length: Option<f64>,
    trim_length: Option<f64>,
    min_stitch: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stitch_style: "auto".into(),
            density: None,
            stitch_length: None,
            stitch_angle: None,
            pull_compensation: None,
            underlay: None,
            jump_length: None,
            trim_length: None,
            min_stitch: None,
        }
    }
}

impl Options {
    fn params(&self, style: Style, width: f64) -> Params {
        let d = Params::for_style(style);
        Params {
            style,
            density: self.density.unwrap_or(d.density),
            // A density given is used as it is; left out, a satin picks it by its width.
            auto_spacing: d.auto_spacing && self.density.is_none(),
            stitch_length: self.stitch_length.unwrap_or(d.stitch_length),
            angle: self.stitch_angle.unwrap_or(d.angle),
            pull: self.pull_compensation.unwrap_or(d.pull),
            underlay: self.underlay.unwrap_or(d.underlay),
            jump: self.jump_length.unwrap_or(d.jump),
            trim: self.trim_length.unwrap_or(d.trim),
            min_stitch: self.min_stitch.unwrap_or(d.min_stitch),
            width,
            ..d
        }
        .sane()
    }
}

/// The options in `p`, checked.
fn options(p: &Value) -> Result<Options> {
    let o = Options::deserialize(p).map_err(|e| bad(C, format!("embroidery options: {e}")))?;
    if Style::from_id(&o.stitch_style).is_none() && o.stitch_style != "auto" {
        return Err(bad(C, format!("stitchStyle `{}`: auto, fill, satin or running", o.stitch_style)));
    }
    Ok(o)
}

/// Write a machine file from the document's visible art.
pub(super) fn encode(doc: &Document, f: &Format, p: &Value) -> Result<Encoded> {
    let opts = options(p)?;
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad(C, format!("{} is not an embroidery format", f.label)))?;
    let (blocks, warnings) = digitize::digitize(doc, &opts).map_err(|e| bad(C, e))?;
    if blocks.is_empty() {
        return Err(bad(C, "nothing to stitch: the document has no visible filled or stroked paths"));
    }
    let pattern = Pattern { name: doc.title.clone(), blocks };
    let bytes = format.write(&pattern).map_err(|e| bad(C, e.to_string()))?;
    Ok(Encoded { warnings, ..Encoded::one(bytes) })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(num(&PULL), satin.pull);
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
}
