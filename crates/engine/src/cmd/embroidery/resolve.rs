//! Where each value a sewing unit is planned with comes from (the project owner's choice,
//! 2026-10-20): the object's own setting for its part, then the option given to this one export or
//! read command, then the document's embroidery settings, then the built-in default. *Auto* (the
//! stitch chosen by the shape, the underlay by the width, a satin's spacing by its width) is a
//! rule the planner applies, never a value written in.
//!
//! Each layer is a set of the embroidery options ([`Options`]), one value or none for each, laid
//! over the next ([`Options::over`]) and turned into the planner's parameters by the one
//! conversion every unit takes ([`Options::params`]): with nothing set but the export's options,
//! a unit is planned exactly as before, by construction.
//!
//! What belongs to the design rather than to a unit (the jump and trim lengths between objects,
//! the cuts across a literal run's clipping) is resolved once a plan ([`DesignSettings`]).

use vectorcraft_stitch::plan::{Params, Style};

use super::Options;

/// The column width given for what is not a satin column: fills and running lines have none.
const NO_WIDTH: f64 = 0.0;

/// One sewing unit's settings: the call's options (the export's, a read command's), the layers
/// below them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UnitSettings<'a> {
    export: &'a Options,
}

impl<'a> UnitSettings<'a> {
    /// The settings of a unit planned with the call's options `export`.
    pub(crate) fn new(export: &'a Options) -> Self {
        Self { export }
    }

    /// Every layer, the highest first, as one set of options.
    fn merged(&self) -> Options {
        *self.export
    }

    /// The style the settings force, `None` for the rule (Auto).
    pub(crate) fn forced(&self) -> Option<Style> {
        self.merged().forced()
    }

    /// How far a filled area grows all round before it is stitched (millimetres).
    pub(crate) fn expand(&self) -> f64 {
        self.merged().expand()
    }

    /// The planner's parameters for a unit sewn in `style`, a satin column `width` wide.
    pub(crate) fn params(&self, style: Style, width: f64) -> Params {
        self.merged().params(style, width)
    }
}

/// What belongs to the design rather than to a unit, resolved once a plan: the jump and trim
/// lengths between objects and across a literal run's cuts, the shortest stitch, as the
/// parameters of a running line carry them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DesignSettings {
    /// A running line's parameters with the design's jump, trim and shortest stitch.
    pub(crate) connect: Params,
}

impl DesignSettings {
    /// The design's settings with the call's options `export`.
    pub(crate) fn new(export: &Options) -> Self {
        Self { connect: export.params(Style::Running, NO_WIDTH) }
    }
}
