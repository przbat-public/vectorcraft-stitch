//! Machine embroidery data an object carries ([`crate::node::NodeExtras::embroidery`]), and the
//! document's embroidery settings ([`crate::Document::embroidery`]).
//!
//! The stitches themselves are planned outside the document model (the `vectorcraft-stitch`
//! crate, through the engine's embroidery export; see `docs/embroidery.md`). What an object keeps
//! here is what that export must know about it beyond its geometry and paint, and what the
//! document keeps is what applies to the whole design.

use serde::{Deserialize, Serialize};

/// What machine embroidery makes of an object.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Embroidery {
    /// The object's points are needle points, sewn exactly as they are, in order, instead of
    /// planning stitches from its shape: a run of stitches read from a machine file, so that the
    /// design is written back unchanged. `None`: the object is art, stitched by the export's rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stitches: Option<StitchRun>,
    /// The thread is cut after the object's stitches, however near the next object starts
    /// (otherwise only a jump longer than the export's trim length is cut). A run of literal
    /// stitches says the same with [`StitchRun::trim_after`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trim_after: bool,
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

/// The document's embroidery settings ([`crate::Document::embroidery`]): what the stitch plan and
/// the embroidery export take from the document rather than from each object. A setting left
/// unset takes the built-in default.
///
/// A file saved by a newer version opens and saves here without losing what this version doesn't
/// know: keys it doesn't read are kept as they are ([`EmbroiderySetup::unknown`]), and so is a
/// palette it doesn't know ([`ThreadPalette::Unknown`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbroiderySetup {
    /// The thread palette the design's colours are matched to: the target machine format's.
    /// `None`: unset (the stitch plan takes Brother's PEC palette).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<ThreadPalette>,
    /// Keys a newer version wrote that this one doesn't read, kept so that saving loses nothing.
    #[serde(flatten)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

impl EmbroiderySetup {
    /// Whether nothing is set and nothing unknown kept: the document then carries none.
    pub fn is_empty(&self) -> bool {
        self.palette.is_none() && self.unknown.is_empty()
    }
}

/// A thread palette the document's colours are matched to, as the file names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "serde_json::Value", into = "serde_json::Value")]
pub enum ThreadPalette {
    /// Brother's 64 threads, as PEC (and so PES) files number them: `"pec"`.
    Pec,
    /// Janome's 78 threads, as JEF files number them: `"jef"`.
    Jef,
    /// A palette this version doesn't know, as a newer one wrote it: kept as it is, and matched
    /// as if none were set.
    Unknown(serde_json::Value),
}

impl ThreadPalette {
    /// The palettes this version knows.
    pub const KNOWN: [Self; 2] = [Self::Pec, Self::Jef];

    /// Its name in files and parameters (`pec`, `jef`); `None` for one this version doesn't know.
    pub fn id(&self) -> Option<&'static str> {
        match self {
            Self::Pec => Some("pec"),
            Self::Jef => Some("jef"),
            Self::Unknown(_) => None,
        }
    }
}

impl From<serde_json::Value> for ThreadPalette {
    fn from(v: serde_json::Value) -> Self {
        match v.as_str() {
            Some("pec") => Self::Pec,
            Some("jef") => Self::Jef,
            _ => Self::Unknown(v),
        }
    }
}

impl From<ThreadPalette> for serde_json::Value {
    fn from(p: ThreadPalette) -> Self {
        match p {
            ThreadPalette::Unknown(v) => v,
            known => known.id().map_or(serde_json::Value::Null, serde_json::Value::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The settings round-trip through JSON as they were written: a palette this version knows,
    /// one it doesn't (kept as it is, whatever its form) and keys it doesn't read.
    #[test]
    fn settings_round_trip_with_what_a_newer_version_wrote() {
        for written in [
            json!({"palette": "jef"}),
            json!({"palette": "pec", "fabric": {"preset": "knit", "pull": 0.35}, "threadWeight": 60}),
            json!({"palette": "madeira-polyneon"}),
            json!({"palette": {"chart": "madeira", "line": "polyneon"}, "machine": "pr1050x"}),
        ] {
            let read: EmbroiderySetup = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written);
        }
        let read: EmbroiderySetup = serde_json::from_value(json!({"palette": "jef", "fabric": "knit"})).unwrap();
        assert_eq!((read.palette, read.unknown.get("fabric")), (Some(ThreadPalette::Jef), Some(&json!("knit"))));
        let unknown: EmbroiderySetup = serde_json::from_value(json!({"palette": 7})).unwrap();
        assert_eq!(unknown.palette, Some(ThreadPalette::Unknown(json!(7))));
        assert_eq!(unknown.palette.and_then(|p| p.id()), None);
    }

    /// Nothing set reads and writes as an empty object, and a palette of null is none.
    #[test]
    fn nothing_set_is_empty() {
        let none: EmbroiderySetup = serde_json::from_value(json!({"palette": null})).unwrap();
        assert!(none.is_empty() && none == EmbroiderySetup::default());
        assert_eq!(serde_json::to_value(&none).unwrap(), json!({}));
        assert_eq!(ThreadPalette::KNOWN.map(|p| p.id()), [Some("pec"), Some("jef")]);
    }
}
