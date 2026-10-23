//! Machine embroidery data an object carries ([`crate::node::NodeExtras::embroidery`]), and the
//! document's embroidery settings ([`crate::Document::embroidery`]).
//!
//! The stitches themselves are planned outside the document model (the `vectorcraft-stitch`
//! crate, through the engine's embroidery export; see `docs/embroidery.md`). What an object keeps
//! here is what that export must know about it beyond its geometry and paint, and what the
//! document keeps is what applies to the whole design.

use serde::{Deserialize, Serialize};
use vectorcraft_geom::Point;

/// What machine embroidery makes of an object.
///
/// Saved under the node's `embroidery` key, with snake_case keys as every node key is (the commands
/// take camelCase). A file saved by a newer version opens and saves here without losing what this
/// version doesn't know, as the document's settings do ([`EmbroiderySetup`]): keys it doesn't read
/// are kept ([`Embroidery::extra`]), so is a known key whose value has another type than this
/// version reads (kept under its name, read as unset), and so is a value that isn't an object
/// ([`Embroidery::unread`]). No form of it stops a file from opening.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "serde_json::Value", into = "serde_json::Value")]
pub struct Embroidery {
    /// The object's points are needle points, sewn exactly as they are, in order, instead of
    /// planning stitches from its shape: a run of stitches read from a machine file, so that the
    /// design is written back unchanged. `None`: the object is art, stitched by the export's rules.
    pub stitches: Option<StitchRun>,
    /// The thread is cut after the object's stitches, however near the next object starts
    /// (otherwise only a jump longer than the export's trim length is cut). A run of literal
    /// stitches says the same with [`StitchRun::trim_after`].
    pub trim_after: bool,
    /// Keys this version doesn't know (written by a newer one), and known keys whose value this
    /// version can't read, kept as they are so that saving doesn't lose them.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// What a newer version wrote when it is not an object, which this version can't read:
    /// nothing is taken from it, and it is written back as it was while nothing is set here.
    pub unread: Option<serde_json::Value>,
}

/// A run of literal stitches ([`Embroidery::stitches`]): the object's points in order, the needle
/// down at each. Read and kept as [`Embroidery`] is: what this version doesn't read stays in
/// [`StitchRun::extra`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StitchRun {
    /// The thread was cut before the run, as the file had it; otherwise the needle jumped to the
    /// run's first point with the thread still on.
    pub trim_before: bool,
    /// The thread was cut after the run, at the end of its colour block, as the file had it
    /// (machines and digitizers often cut before the thread is changed).
    pub trim_after: bool,
    /// The run is two needle points at one spot (a tack). Its line, from a point to the same point,
    /// looks the same as a single stitch's, so this tells them apart: without it, a line that goes
    /// nowhere is one needle point.
    pub double_point: bool,
    /// The thread the file recorded for the run's colour block beside its colour (a VP3 thread
    /// record), written back by a format that has one. `None`: the file recorded none.
    pub thread: Option<ThreadRecord>,
    /// Keys this version doesn't read, kept as they are.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A thread as a machine file recorded it beside its colour ([`StitchRun::thread`]), saved as
/// `{"catalogue", "description", "brand", "material", "weight"}` (its strings, and the material's
/// code and the weight as bytes), with the keys this version doesn't read kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadRecord {
    /// The maker's catalogue number.
    pub catalogue: String,
    /// The thread's description.
    pub description: String,
    /// The maker.
    pub brand: String,
    /// The material's code (5 is rayon).
    pub material: u8,
    /// The thread's weight (40 wt is standard embroidery thread).
    pub weight: u8,
    /// Keys this version doesn't read, kept as they are.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ThreadRecord {
    /// The record a file holds under a run's `thread` key, when it is one this version reads: an
    /// object with each known key, three strings and two bytes (0 to 255). `None` otherwise (kept
    /// as it is, so that it saves back as it was).
    fn read(v: &serde_json::Value) -> Option<Self> {
        let mut map = v.as_object()?.clone();
        let text = |map: &mut serde_json::Map<String, serde_json::Value>, key: &str| match map.remove(key) {
            Some(serde_json::Value::String(s)) => Some(s),
            _ => None,
        };
        let byte = |map: &mut serde_json::Map<String, serde_json::Value>, key: &str| map.remove(key)?.as_u64().and_then(|n| u8::try_from(n).ok());
        let catalogue = text(&mut map, CATALOGUE)?;
        let description = text(&mut map, DESCRIPTION)?;
        let brand = text(&mut map, BRAND)?;
        let material = byte(&mut map, MATERIAL)?;
        let weight = byte(&mut map, WEIGHT)?;
        Some(Self { catalogue, description, brand, material, weight, extra: map })
    }
}

impl From<ThreadRecord> for serde_json::Value {
    fn from(t: ThreadRecord) -> Self {
        let set = [
            (CATALOGUE, serde_json::Value::from(t.catalogue)),
            (DESCRIPTION, serde_json::Value::from(t.description)),
            (BRAND, serde_json::Value::from(t.brand)),
            (MATERIAL, serde_json::Value::from(t.material)),
            (WEIGHT, serde_json::Value::from(t.weight)),
        ];
        with_kept(set, t.extra)
    }
}

impl Embroidery {
    /// Set whether the thread is cut after the object. This replaces whatever a newer version wrote
    /// under that key in a form this version can't read.
    pub fn set_trim_after(&mut self, on: bool) {
        self.trim_after = on;
        self.extra.remove(TRIM_AFTER);
    }
}

/// The keys an object's embroidery and a run of stitches are saved under.
const STITCHES: &str = "stitches";
const TRIM_AFTER: &str = "trim_after";
const TRIM_BEFORE: &str = "trim_before";
const DOUBLE_POINT: &str = "double_point";
const THREAD: &str = "thread";
/// The keys of a thread record ([`ThreadRecord`]).
const CATALOGUE: &str = "catalogue";
const DESCRIPTION: &str = "description";
const BRAND: &str = "brand";
const MATERIAL: &str = "material";
const WEIGHT: &str = "weight";

/// Take boolean `key` out of `map`: its value when it is one (a missing key is false); a value of
/// another type stays in `map`, to be written back as it was.
fn take_flag(map: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    match map.get(key) {
        Some(serde_json::Value::Bool(b)) => {
            let b = *b;
            map.remove(key);
            b
        }
        Some(serde_json::Value::Null) => {
            map.remove(key);
            false
        }
        _ => false,
    }
}

/// One object of the pairs `set`, then the kept keys (which don't override them).
fn with_kept(
    set: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    kept: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    let mut map: serde_json::Map<String, serde_json::Value> = set.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    for (k, v) in kept {
        map.entry(k).or_insert(v);
    }
    serde_json::Value::Object(map)
}

impl From<serde_json::Map<String, serde_json::Value>> for StitchRun {
    fn from(mut map: serde_json::Map<String, serde_json::Value>) -> Self {
        let trim_before = take_flag(&mut map, TRIM_BEFORE);
        let trim_after = take_flag(&mut map, TRIM_AFTER);
        let double_point = take_flag(&mut map, DOUBLE_POINT);
        // A record of another shape is kept as it is, unread.
        let thread = map.get(THREAD).and_then(ThreadRecord::read);
        if thread.is_some() {
            map.remove(THREAD);
        }
        Self { trim_before, trim_after, double_point, thread, extra: map }
    }
}

impl From<StitchRun> for serde_json::Value {
    /// The run as it is saved: its cuts when there are, then the keys kept.
    fn from(r: StitchRun) -> Self {
        let flag = |key: &'static str, on: bool| on.then_some((key, serde_json::Value::Bool(true)));
        let flags = flag(TRIM_BEFORE, r.trim_before).into_iter().chain(flag(TRIM_AFTER, r.trim_after)).chain(flag(DOUBLE_POINT, r.double_point));
        let thread = r.thread.map(|t| (THREAD, serde_json::Value::from(t)));
        with_kept(flags.chain(thread), r.extra)
    }
}

impl From<serde_json::Value> for Embroidery {
    /// An object's embroidery as a file holds it: its known keys, those of another type and the
    /// unknown ones kept ([`Embroidery::extra`]); anything but an object kept as it is
    /// ([`Embroidery::unread`]).
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Object(mut map) => {
                let stitches = match map.remove(STITCHES) {
                    Some(serde_json::Value::Object(run)) => Some(StitchRun::from(run)),
                    Some(serde_json::Value::Null) | None => None,
                    Some(other) => {
                        // Another shape: kept under its name, and the object is art.
                        map.insert(STITCHES.to_string(), other);
                        None
                    }
                };
                let trim_after = take_flag(&mut map, TRIM_AFTER);
                Self { stitches, trim_after, extra: map, unread: None }
            }
            serde_json::Value::Null => Self::default(),
            other => Self { unread: Some(other), ..Self::default() },
        }
    }
}

impl From<Embroidery> for serde_json::Value {
    /// The object's embroidery as it is saved: its run and its cut when it has them, then the kept
    /// keys; or what a newer version wrote that this one can't read, as it was, while nothing is
    /// set.
    fn from(e: Embroidery) -> Self {
        match e.unread {
            Some(unread) if e.stitches.is_none() && !e.trim_after && e.extra.is_empty() => unread,
            _ => {
                let run = e.stitches.map(|r| (STITCHES, serde_json::Value::from(r)));
                let cut = e.trim_after.then_some((TRIM_AFTER, serde_json::Value::Bool(true)));
                with_kept(run.into_iter().chain(cut), e.extra)
            }
        }
    }
}

/// The document's embroidery settings ([`crate::Document::embroidery`]): what the stitch plan and
/// the embroidery export take from the document rather than from each object. A setting left
/// unset takes the built-in default.
///
/// A file saved by a newer version opens and saves here without losing what this version doesn't
/// know, whatever its shape: keys it doesn't read are kept as they are ([`EmbroiderySetup::extra`]),
/// and so is a palette it doesn't know ([`ThreadPalette::Unknown`]) and settings that aren't an
/// object at all ([`EmbroiderySetup::unread`]). No form of them stops a file from opening.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "serde_json::Value", into = "serde_json::Value")]
pub struct EmbroiderySetup {
    /// The thread palette the design's colours are matched to: the target machine format's.
    /// `None`: unset (the stitch plan takes Janome's JEF palette).
    pub palette: Option<ThreadPalette>,
    /// Where the machine starts the needle, in document points: the start point (origin) of the
    /// machine file the design was opened from, which the DST, TAP, T01, EXP, PES and PEC exports
    /// start from again (JEF and VP3 start at the design's centre). `None`: they start at the
    /// design's corner.
    pub start: Option<Point>,
    /// The design's name a machine file gave (DST's and PEC's `LA:` label), which the export
    /// writes again. `None`: the export writes the document's title.
    pub label: Option<String>,
    /// Keys this version doesn't know (written by a newer one), kept so that saving doesn't lose
    /// them, as [`crate::Document::extra`] keeps the document's.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// The settings as a newer version wrote them when they are not an object (`"embroidery": 5`),
    /// which this version can't read: nothing is taken from them, and they are written back as they
    /// are while nothing is set here. Setting the palette replaces them ([`EmbroiderySetup::set_palette`]).
    pub unread: Option<serde_json::Value>,
}

/// The keys the palette and the start point are saved under.
const PALETTE: &str = "palette";
const START: &str = "start";
const LABEL: &str = "label";

impl EmbroiderySetup {
    /// Whether nothing is set and nothing a newer version wrote is kept: the document then carries
    /// none.
    pub fn is_empty(&self) -> bool {
        self.palette.is_none() && self.start.is_none() && self.label.is_none() && self.extra.is_empty() && self.unread.is_none()
    }

    /// Set the palette, or unset it (`None`). A palette set replaces settings a newer version wrote
    /// in a form this version can't read ([`EmbroiderySetup::unread`]): saved, they are this
    /// version's object from then on.
    pub fn set_palette(&mut self, palette: Option<ThreadPalette>) {
        if palette.is_some() {
            self.unread = None;
        }
        self.palette = palette;
    }

    /// Set where the machine starts the needle (document points), or unset it (`None`); like the
    /// palette, a start set replaces settings this version can't read.
    pub fn set_start(&mut self, start: Option<Point>) {
        if start.is_some() {
            self.unread = None;
        }
        self.start = start;
    }

    /// Set the design's name the export writes, or unset it (`None`: the document's title); like
    /// the palette, a label set replaces settings this version can't read.
    pub fn set_label(&mut self, label: Option<String>) {
        if label.is_some() {
            self.unread = None;
        }
        self.label = label;
    }
}

impl From<serde_json::Value> for EmbroiderySetup {
    /// The settings as a file holds them: an object's palette (`null`: unset) and its other keys;
    /// anything else kept as it is ([`EmbroiderySetup::unread`]). A key given twice reads as its
    /// last value, as JSON readers do.
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Object(mut map) => {
                let palette = map.remove(PALETTE).filter(|p| !p.is_null()).map(ThreadPalette::from);
                // A start point that isn't two finite numbers is kept as it is, unread.
                let start = map.get(START).and_then(point);
                if start.is_some() {
                    map.remove(START);
                }
                // So is a label that isn't a string.
                let label = map.get(LABEL).and_then(|l| l.as_str()).map(str::to_string);
                if label.is_some() {
                    map.remove(LABEL);
                }
                Self { palette, start, label, extra: map, unread: None }
            }
            serde_json::Value::Null => Self::default(),
            other => Self { unread: Some(other), ..Self::default() },
        }
    }
}

impl From<EmbroiderySetup> for serde_json::Value {
    /// The settings as they are saved: an object of the palette and the kept keys (the palette
    /// first, and winning over a kept key of its name), or what a newer version wrote that this one
    /// can't read, as it was, while nothing is set.
    fn from(s: EmbroiderySetup) -> Self {
        match s.unread {
            Some(unread) if s.palette.is_none() && s.start.is_none() && s.label.is_none() && s.extra.is_empty() => unread,
            _ => {
                let mut map = serde_json::Map::new();
                if let Some(palette) = s.palette {
                    map.insert(PALETTE.to_string(), palette.into());
                }
                if let Some(start) = s.start {
                    map.insert(START.to_string(), serde_json::json!([start.x, start.y]));
                }
                if let Some(label) = s.label {
                    map.insert(LABEL.to_string(), serde_json::Value::String(label));
                }
                for (k, v) in s.extra {
                    map.entry(k).or_insert(v);
                }
                serde_json::Value::Object(map)
            }
        }
    }
}

/// A point saved as `[x, y]`; `None` for anything else, or numbers that aren't finite.
fn point(v: &serde_json::Value) -> Option<Point> {
    let [x, y] = v.as_array()?.as_slice() else { return None };
    let (x, y) = (x.as_f64()?, y.as_f64()?);
    (x.is_finite() && y.is_finite()).then(|| Point::new(x, y))
}

/// A thread palette the document's colours are matched to, as the file names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "serde_json::Value", into = "serde_json::Value")]
pub enum ThreadPalette {
    /// Brother's 61 threads, as PEC (and so PES) files number them: `"pec"`.
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

    /// The palette this version knows by name `id` ([`ThreadPalette::id`]).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::KNOWN.into_iter().find(|p| p.id() == Some(id))
    }
}

impl From<serde_json::Value> for ThreadPalette {
    fn from(v: serde_json::Value) -> Self {
        match v.as_str().and_then(Self::from_id) {
            Some(known) => known,
            None => Self::Unknown(v),
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
            json!({"palette": "pec", "start": [14.5, -3.0], "label": "ACME_01"}),
            json!({"label": 7}),
            json!({"start": "centre", "hoop": [100, 100]}),
            json!({"palette": "madeira-polyneon"}),
            json!({"palette": {"chart": "madeira", "line": "polyneon"}, "machine": "pr1050x"}),
        ] {
            let read: EmbroiderySetup = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written);
        }
        let read: EmbroiderySetup = serde_json::from_value(json!({"palette": "jef", "fabric": "knit"})).unwrap();
        assert_eq!((read.palette, read.extra.get("fabric")), (Some(ThreadPalette::Jef), Some(&json!("knit"))));
        let start: EmbroiderySetup = serde_json::from_value(json!({"start": [14.5, -3.0]})).unwrap();
        assert_eq!((start.start, start.extra.len()), (Some(Point::new(14.5, -3.0)), 0));
        // Not two finite numbers: kept as it was, unread.
        for unread in [json!("centre"), json!([1, 2, 3]), json!([1, "2"]), json!({"x": 1})] {
            let read: EmbroiderySetup = serde_json::from_value(json!({ "start": unread.clone() })).unwrap();
            assert_eq!((read.start, read.extra.get("start")), (None, Some(&unread)));
        }
        let unknown: EmbroiderySetup = serde_json::from_value(json!({"palette": 7})).unwrap();
        assert_eq!(unknown.palette, Some(ThreadPalette::Unknown(json!(7))));
        assert_eq!(unknown.palette.and_then(|p| p.id()), None);
    }

    /// An object's embroidery round-trips as it was written: its run and cut, keys this version
    /// doesn't know, known keys of another type (read as unset), and a value that isn't an object.
    #[test]
    fn an_objects_embroidery_round_trips_with_what_a_newer_version_wrote() {
        for written in [
            json!({"trim_after": true}),
            json!({"stitches": {"trim_before": true, "trim_after": true}}),
            json!({"stitches": {"trim_before": true, "density": 4}, "fill": {"angle": 30}}),
            json!({"stitches": {"thread": {"catalogue": "2296", "description": "Pink", "brand": "Robison-Anton", "material": 5, "weight": 40}}}),
            json!({"stitches": {"thread": {"catalogue": "1", "description": "", "brand": "", "material": 0, "weight": 30, "twist": [1, 2]}}}),
            json!({"stitches": {"thread": {"catalogue": 2296}}}),
            json!({"stitches": {"thread": {"catalogue": "2296", "description": "Pink", "brand": "", "material": 5}}}),
            json!({"stitches": {"thread": {"catalogue": "2296", "description": "Pink", "brand": "", "material": 5.0, "weight": 40}}}),
            json!({"stitches": {"thread": {"weight": 400}}}),
            json!({"stitches": {"thread": "Pink"}}),
            json!({"trim_after": "auto", "stitches": [1, 2]}),
            json!(5),
            json!("satin"),
        ] {
            let read: Embroidery = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written, "{written}");
        }
        let odd: Embroidery = serde_json::from_value(json!({"trim_after": "auto", "stitches": [1, 2]})).unwrap();
        assert_eq!((odd.trim_after, odd.stitches.is_none(), odd.extra.len()), (false, true, 2));
        let run: Embroidery = serde_json::from_value(json!({"stitches": {"trim_after": true, "density": 4}})).unwrap();
        assert_eq!(run.stitches.map(|r| (r.trim_after, r.trim_before, r.extra.len())), Some((true, false, 1)));
        // A thread record is read when it is whole; else kept as it was.
        let whole = json!({"catalogue": "2296", "description": "Pink", "brand": "Robison-Anton", "material": 5, "weight": 40});
        let read: Embroidery = serde_json::from_value(json!({ "stitches": { "thread": whole } })).unwrap();
        let thread = read.stitches.and_then(|r| r.thread).unwrap();
        assert_eq!((thread.catalogue.as_str(), thread.brand.as_str(), thread.material, thread.weight), ("2296", "Robison-Anton", 5, 40));
        let partial: Embroidery = serde_json::from_value(json!({"stitches": {"thread": {"catalogue": "2296"}}})).unwrap();
        assert!(partial.stitches.is_some_and(|r| r.thread.is_none() && r.extra.contains_key("thread")));
        // Unread, then set: the object replaces it.
        let mut unread: Embroidery = serde_json::from_value(json!(5)).unwrap();
        unread.trim_after = true;
        assert_eq!(serde_json::to_value(&unread).unwrap(), json!({"trim_after": true}));
    }

    /// Settings of another shape than an object, as a newer version might write them, read as
    /// nothing set and write back as they were; setting the palette replaces them. A key given
    /// twice reads as its last value.
    #[test]
    fn settings_of_another_shape_are_kept_as_they_are() {
        for written in [json!(5), json!("pec"), json!(["pec"]), json!(true)] {
            let read: EmbroiderySetup = serde_json::from_value(written.clone()).unwrap();
            assert_eq!((read.palette.clone(), read.extra.len(), read.unread.as_ref()), (None, 0, Some(&written)));
            assert!(!read.is_empty(), "{written}: kept");
            assert_eq!(serde_json::to_value(&read).unwrap(), written);
            let mut set = read;
            set.set_palette(Some(ThreadPalette::Pec));
            assert_eq!(serde_json::to_value(&set).unwrap(), json!({"palette": "pec"}), "{written}");
        }
        let twice: EmbroiderySetup = serde_json::from_str(r#"{"palette": "jef", "palette": "pec"}"#).unwrap();
        assert_eq!(twice.palette, Some(ThreadPalette::Pec));
        // A kept key of the palette's name never doubles it.
        let mut odd = EmbroiderySetup { palette: Some(ThreadPalette::Jef), ..EmbroiderySetup::default() };
        odd.extra.insert(PALETTE.to_string(), json!("pec"));
        assert_eq!(serde_json::to_value(&odd).unwrap(), json!({"palette": "jef"}));
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
