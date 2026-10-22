//! A plan's records walked in sewing order ([`each`]): every needle point, jump and cut with the
//! unit it belongs to, its number, and where the needle was before it, so that what is sewn can
//! be measured ([`Record::sewn`]: the thread laid by a needle-down stitch).

use vectorcraft_stitch::plan::Style;
use vectorcraft_stitch::{Command, Stitch};

use super::plan::DesignPlan;

/// One record of a plan, in sewing order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Record {
    /// Its number in the design ([`DesignPlan::number`]).
    pub(crate) number: usize,
    /// The unit it belongs to: an index into [`DesignPlan::units`].
    pub(crate) unit: usize,
    pub(crate) stitch: Stitch,
    /// The record before it in the design (the last of the block before, for a block's first),
    /// `None` for the design's first.
    pub(crate) before: Option<Stitch>,
    /// Whether the record before it is in its block: the move between two blocks is the machine's
    /// own, with the thread changed between them.
    pub(crate) same_block: bool,
}

impl Record {
    /// The length of thread a needle-down stitch lays, millimetres: from the needle point before
    /// it in its block to this one, both needle points. `None` for anything else (the first needle
    /// point after a jump or a cut, a block's first, a jump, a cut).
    pub(crate) fn sewn(&self) -> Option<f64> {
        let before = self.before.filter(|b| self.same_block && b.command == Command::Stitch)?;
        (self.stitch.command == Command::Stitch).then(|| before.distance(self.stitch))
    }

    /// How far the needle moves to reach it from the record before, millimetres (0 for the
    /// design's first).
    pub(crate) fn moved(&self) -> f64 {
        self.before.map_or(0.0, |b| b.distance(self.stitch))
    }
}

/// How a unit is sewn ([`super::plan::Unit::sewn`]), `None` for a literal run (points sewn as they
/// are).
pub(crate) fn style(plan: &DesignPlan, unit: usize) -> Option<Style> {
    plan.units.get(unit).and_then(|u| u.sewn())
}

/// Call `f` with every record of `plan`, in sewing order.
pub(crate) fn each(plan: &DesignPlan, mut f: impl FnMut(&Record)) {
    let mut before: Option<Stitch> = None;
    for block in &plan.blocks {
        for unit in block.units.clone() {
            let Some(u) = plan.units.get(unit) else { continue };
            for index in u.stitches.clone() {
                let Some(&stitch) = block.stitches.get(index) else { continue };
                let number = block.first_number.saturating_add(index);
                f(&Record { number, unit, stitch, before, same_block: index > 0 });
                before = Some(stitch);
            }
        }
    }
}
