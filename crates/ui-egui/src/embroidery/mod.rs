//! The machine-embroidery interface of VectorCraft Stitch (this fork): what the app shows of a
//! design's stitches. Fork-owned, so that upstream files carry only one-line hooks into it.
//!
//! - [`numbers`]: numbers as the interface language writes them, in the embroidery interface's
//!   read-only text.
//! - [`text`]: the embroidery interface's words, in the catalogues' `embroidery` context
//!   ([`crate::et!`], [`crate::etn!`]).

pub mod numbers;
pub mod text;
