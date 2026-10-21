//! The embroidery export's options, as they come, and the stitch planner's parameters they give an
//! object ([`Options::params`]).

use serde::Deserialize;
use serde_json::Value;
use vectorcraft_stitch::plan::THREAD_WEIGHT;
use vectorcraft_stitch::plan::{Params, Style};

use crate::Result;
use crate::cmd::bad;

/// How far a filled shape grows all round when `expand` is left out (millimetres): not at all, as
/// in Ink/Stitch, whose fills offer the same setting ("Expand", to close gaps between shapes,
/// <https://inkstitch.org/docs/stitches/fill-stitch/>). Pull compensation already stretches a
/// fill's rows; expanding is for art whose shapes are drawn apart.
pub(crate) const EXPAND_DEFAULT: f64 = 0.0;
/// The most a filled shape grows or shrinks (millimetres): more than closing gaps between shapes
/// needs, short of redrawing them.
pub(crate) const MAX_EXPAND: f64 = 2.0;

/// A value for both sides of a stitch, or one for each.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged, expecting = "a number, or two numbers (one per side)")]
pub(crate) enum Sides {
    Both(f64),
    Each([f64; 2]),
}

impl Sides {
    fn pair(self) -> [f64; 2] {
        match self {
            Self::Both(v) => [v; 2],
            Self::Each(v) => v,
        }
    }
}

/// The export options, as they come: each one left out keeps the style's own value.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Options {
    pub(crate) stitch_style: String,
    pub(crate) density: Option<f64>,
    pub(crate) stitch_length: Option<f64>,
    pub(crate) stitch_angle: Option<f64>,
    pub(crate) pull_compensation: Option<Sides>,
    pub(crate) pull_compensation_percent: Option<Sides>,
    pub(crate) push_compensation: Option<Sides>,
    pub(crate) expand: Option<f64>,
    pub(crate) narrow_as_satin: Option<f64>,
    pub(crate) thread_weight: Option<f64>,
    pub(crate) underlay: Option<bool>,
    pub(crate) jump_length: Option<f64>,
    pub(crate) trim_length: Option<f64>,
    pub(crate) min_stitch: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stitch_style: "auto".into(),
            density: None,
            stitch_length: None,
            stitch_angle: None,
            pull_compensation: None,
            pull_compensation_percent: None,
            push_compensation: None,
            expand: None,
            narrow_as_satin: None,
            thread_weight: None,
            underlay: None,
            jump_length: None,
            trim_length: None,
            min_stitch: None,
        }
    }
}

impl Options {
    /// The options in command params `p` (as `document.export` takes them), checked; `cmd` names
    /// the command in the errors.
    pub(crate) fn from_params(p: &Value, cmd: &str) -> Result<Self> {
        let o = Self::deserialize(p).map_err(|e| bad(cmd, format!("embroidery options: {e}")))?;
        if o.forced().is_none() && o.stitch_style != "auto" {
            return Err(bad(cmd, format!("stitchStyle `{}`: auto, fill, satin or running", o.stitch_style)));
        }
        Ok(o)
    }

    pub(crate) fn params(&self, style: Style, width: f64) -> Params {
        // The style's defaults for the thread: its spacing follows the thread's thickness.
        let d = Params::for_thread(style, self.thread_weight.unwrap_or(THREAD_WEIGHT));
        Params {
            style,
            density: self.density.unwrap_or(d.density),
            // A density given is used as it is; left out, a satin picks it by its width.
            auto_spacing: d.auto_spacing && self.density.is_none(),
            stitch_length: self.stitch_length.unwrap_or(d.stitch_length),
            angle: self.stitch_angle.unwrap_or(d.angle),
            // A running line has no compensation, whatever is given.
            pull: if style == Style::Running { d.pull } else { self.pull_compensation.map_or(d.pull, Sides::pair) },
            pull_percent: if style == Style::Running { d.pull_percent } else { self.pull_compensation_percent.map_or(d.pull_percent, Sides::pair) },
            // Push compensation shortens satin columns only: a fill's rows have no column ends.
            push: if style == Style::Satin { self.push_compensation.map_or(d.push, Sides::pair) } else { d.push },
            // Only a fill has narrow parts to sew as satin.
            narrow: if style == Style::Fill { self.narrow_as_satin.unwrap_or(d.narrow) } else { d.narrow },
            underlay: self.underlay.unwrap_or(d.underlay),
            jump: self.jump_length.unwrap_or(d.jump),
            trim: self.trim_length.unwrap_or(d.trim),
            min_stitch: self.min_stitch.unwrap_or(d.min_stitch),
            width,
            ..d
        }
        .sane()
    }

    /// The style `stitchStyle` forces, `None` for `auto`.
    pub(crate) fn forced(&self) -> Option<Style> {
        Style::from_id(&self.stitch_style)
    }

    /// How far a filled shape grows all round before it is stitched (millimetres, negative to
    /// shrink), within ±[`MAX_EXPAND`]; nothing when not finite.
    pub(crate) fn expand(&self) -> f64 {
        let e = self.expand.unwrap_or(EXPAND_DEFAULT);
        if e.is_finite() { e.clamp(-MAX_EXPAND, MAX_EXPAND) } else { EXPAND_DEFAULT }
    }
}
