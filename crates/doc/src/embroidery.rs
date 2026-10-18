//! Machine embroidery data an object carries ([`crate::node::NodeExtras::embroidery`]).
//!
//! The stitches themselves are planned outside the document model (the `vectorcraft-stitch`
//! crate, through the engine's embroidery export; see `docs/embroidery.md`). What an object keeps
//! here is what that export must know about it beyond its geometry and paint.

use serde::{Deserialize, Serialize};

/// What machine embroidery makes of an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Embroidery {
    /// The object's points are needle points, sewn exactly as they are, in order, instead of
    /// planning stitches from its shape: a run of stitches read from a machine file, so that the
    /// design is written back unchanged. `None`: the object is art, stitched by the export's rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stitches: Option<StitchRun>,
}

/// A run of literal stitches ([`Embroidery::stitches`]): the object's points in order, the needle
/// down at each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StitchRun {
    /// The thread was cut before the run, as the file had it; otherwise the needle jumped to the
    /// run's first point with the thread still on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trim_before: bool,
    /// The thread was cut after the run, at the end of its colour block, as the file had it
    /// (machines and digitizers often cut before the thread is changed).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trim_after: bool,
}
