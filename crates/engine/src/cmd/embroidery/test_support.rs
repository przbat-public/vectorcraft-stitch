//! What the embroidery unit tests share: running commands, a blank document, and plans built unit
//! by unit.

use serde_json::{Value, json};
use vectorcraft_doc::NodeId;
use vectorcraft_geom::Point;
use vectorcraft_stitch::plan::{Params, Style};
use vectorcraft_stitch::thread::Palette;
use vectorcraft_stitch::{Block, Stitch, Thread};

use super::plan::{Choice, DesignPlan, Origin, Part};
use super::sequence::{Facts, Planned, Sequence};
use super::stats::{MAX_LIST_LIMIT, Page};
use crate::Session;

/// Points per millimetre: the documents count points, the designs millimetres.
pub(super) const PT_PER_MM: f64 = 72.0 / 25.4;

/// A list's first items, as many as a reply may hold.
pub(super) const ALL: Page = Page { offset: 0, limit: MAX_LIST_LIMIT };

/// Run `command`, panicking with its params on an error.
pub(super) fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// A session holding a new 300 × 300 pt document with one artboard.
pub(super) fn blank() -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": 300, "height": 300, "artboards": 1}));
    s
}

/// A unit of `style` in `color` sewing needle points at `points` (millimetres), whose narrowest
/// and widest satin columns are `satin_widths`.
pub(super) fn unit(node: u64, style: Style, color: [u8; 3], points: &[(f64, f64)], satin_widths: Option<[f64; 2]>) -> Planned {
    planned(node, Some(Params::for_style(style)), color, points, satin_widths)
}

/// A unit planned with `params` (`None`: a literal run read from a file), as [`unit`] makes one.
pub(super) fn planned(node: u64, params: Option<Params>, color: [u8; 3], points: &[(f64, f64)], satin_widths: Option<[f64; 2]>) -> Planned {
    let mut block = Block::new(Thread::rgb(color));
    block.stitches = points.iter().map(|&(x, y)| Stitch::sew(Point::new(x, y))).collect();
    let origin = Origin { node: NodeId(node), part: Part::Fill(0), instance: None };
    let why = if params.is_some() { Choice::Forced } else { Choice::Literal };
    Planned { facts: Facts { origin, occurrence: 1, params, columns: None, satin_widths, why }, block, literal: None }
}

/// The plan of `units`, added in order, in Janome's palette.
pub(super) fn plan(units: Vec<Planned>) -> DesignPlan {
    let mut seq = Sequence::new(Params::for_style(Style::Fill), true);
    for u in units {
        seq.add(u);
    }
    seq.plan("test".into(), Vec::new(), Palette::Jef)
}
