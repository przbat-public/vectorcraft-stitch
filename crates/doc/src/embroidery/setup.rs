//! The document's embroidery settings beyond its thread palette, start point and label: the fabric
//! ([`FabricSettings`]) and the machine ([`MachineSettings`]); the thread's weight and the joining
//! of one colour's objects are plain values of [`super::EmbroiderySetup`].
//!
//! A fabric is copied into the document, not linked to its preset (the project owner's choice,
//! 2026-10-20): choosing one copies the preset's values, and keeps them as copied beside the values
//! in force ([`FabricPreset::values`]), so that a value changed since can be marked and restored
//! whatever a later version's presets say. Values are kept by the stitch they apply to (tatami
//! fills and satin columns, [`StitchValues`]), under the vocabulary's names in snake_case.
//!
//! Read as the rest of the document's embroidery settings are: a value of another type than this
//! version reads is kept as it was and read as unset, a key it doesn't know is kept, an id it
//! doesn't know (a preset, an underlay kind) is kept as written; nothing in them stops a file from
//! opening. Ranges are the commands' to check.

use serde_json::{Map, Value};

use super::{id, number, setup_object, take, with_kept};

/// What a fabric says for one kind of stitch: tatami fills ([`FabricSettings::tatami`]), or satin
/// columns ([`FabricSettings::satin`]). Each value `None` while unset (the built-in default).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StitchValues {
    /// Pull compensation, millimetres at each side.
    pub pull: Option<f64>,
    /// Tatami: millimetres between rows, for 40 wt thread (a finer or thicker thread scales it, as
    /// it scales the built-in spacing).
    pub spacing: Option<f64>,
    /// Satin: the spacing picked by a column's width, in percent (100: as picked).
    pub spacing_percent: Option<f64>,
    /// Tatami: the longest stitch of a row, millimetres.
    pub length: Option<f64>,
    /// What is sewn under it first: an underlay kind's id, kept as written.
    pub underlay: Option<String>,
    /// Keys this version doesn't know, and known keys whose value it can't read.
    pub extra: Map<String, Value>,
}

/// What a fabric says beyond its stitches' values, and its stitches' values: one set of them, as a
/// preset gives them ([`FabricPreset::values`]) or as they are in force (the fabric's own).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FabricValues {
    /// For tatami fills, and strokes sewn as the area they paint.
    pub tatami: Option<StitchValues>,
    /// For satin columns, and a fill's narrow parts sewn as satin.
    pub satin: Option<StitchValues>,
    /// What holds the fabric from below (information only): an id such as `cutAway`.
    pub stabilizer: Option<String>,
    /// What lies on top while it is sewn (information only): an id such as `waterSoluble`.
    pub topping: Option<String>,
    /// The needle's point for it (information only): an id such as `ball`.
    pub needle_point: Option<String>,
    /// How thick the fabric is with its stabiliser, millimetres: the top thread goes down through
    /// it at each needle point (`embroidery.stats` estimates thread by it).
    pub thickness: Option<f64>,
    /// Keys this version doesn't know, and known keys whose value it can't read.
    pub extra: Map<String, Value>,
}

/// The preset a fabric's values were copied from ([`FabricSettings::preset`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FabricPreset {
    /// Its id, kept as written (a newer version's preset included).
    pub id: String,
    /// Its values as they were copied: a value in force that differs is marked changed, and
    /// restoring it takes this one back.
    pub values: FabricValues,
    /// Keys this version doesn't know, and known keys whose value it can't read.
    pub extra: Map<String, Value>,
}

/// The document's fabric ([`super::EmbroiderySetup::fabric`]): its values in force, the preset they
/// were copied from, and its colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FabricSettings {
    /// The preset its values were copied from; `None`: a fabric of the person's own.
    pub preset: Option<FabricPreset>,
    /// The values in force.
    pub values: FabricValues,
    /// The fabric's colour (sRGB), which the stitch preview draws under the stitches; saved as
    /// `#rrggbb`. Not a preset's value: choosing a preset keeps it.
    pub color: Option<[u8; 3]>,
    /// Keys this version doesn't know, and known keys whose value it can't read.
    pub extra: Map<String, Value>,
}

/// The machine the design is sewn on ([`super::EmbroiderySetup::machine`]): what belongs to the
/// whole design rather than to an object. Each value `None` while unset (the built-in default).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MachineSettings {
    /// A longer move lifts the needle (a jump), millimetres.
    pub jump_length: Option<f64>,
    /// A longer jump cuts the thread first (a trim), millimetres: machines differ in whether, and
    /// past what length, they cut jumps themselves.
    pub trim_length: Option<f64>,
    /// Shorter stitches are left out, millimetres.
    pub min_stitch: Option<f64>,
    /// A longer satin stitch is split, millimetres.
    pub split_length: Option<f64>,
    /// Stitches a minute it sews at (`embroidery.stats` times the design by it).
    pub speed: Option<f64>,
    /// Keys this version doesn't know, and known keys whose value it can't read.
    pub extra: Map<String, Value>,
}

const PULL: &str = "pull";
const SPACING: &str = "spacing";
const SPACING_PERCENT: &str = "spacing_percent";
const LENGTH: &str = "length";
const UNDERLAY: &str = "underlay";
const TATAMI: &str = "tatami";
const SATIN: &str = "satin";
const STABILIZER: &str = "stabilizer";
const TOPPING: &str = "topping";
const NEEDLE_POINT: &str = "needle_point";
const THICKNESS: &str = "thickness";
const PRESET: &str = "preset";
const ID: &str = "id";
const VALUES: &str = "values";
const COLOR: &str = "color";
const JUMP_LENGTH: &str = "jump_length";
const TRIM_LENGTH: &str = "trim_length";
const MIN_STITCH: &str = "min_stitch";
const SPLIT_LENGTH: &str = "split_length";
const SPEED: &str = "speed";

/// A colour saved as `#rrggbb`.
fn color(v: &Value) -> Option<[u8; 3]> {
    let hex = v.as_str()?.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |k: usize| hex.get(k..k + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn num(key: &'static str, v: Option<f64>) -> Option<(&'static str, Value)> {
    v.map(|x| (key, Value::from(x)))
}

fn text(key: &'static str, v: Option<String>) -> Option<(&'static str, Value)> {
    v.map(|s| (key, Value::String(s)))
}

impl StitchValues {
    fn read(mut map: Map<String, Value>) -> Option<Self> {
        let pull = take(&mut map, PULL, number);
        let spacing = take(&mut map, SPACING, number);
        let spacing_percent = take(&mut map, SPACING_PERCENT, number);
        let length = take(&mut map, LENGTH, number);
        let underlay = take(&mut map, UNDERLAY, id);
        let v = Self { pull, spacing, spacing_percent, length, underlay, extra: map };
        (!v.is_empty()).then_some(v)
    }

    /// Whether nothing is set and nothing a newer version wrote is kept.
    pub fn is_empty(&self) -> bool {
        let Self { pull, spacing, spacing_percent, length, underlay, extra } = self;
        pull.is_none() && spacing.is_none() && spacing_percent.is_none() && length.is_none() && underlay.is_none() && extra.is_empty()
    }
}

impl From<StitchValues> for Value {
    fn from(v: StitchValues) -> Self {
        let StitchValues { pull, spacing, spacing_percent, length, underlay, extra } = v;
        let set = [num(PULL, pull), num(SPACING, spacing), num(SPACING_PERCENT, spacing_percent), num(LENGTH, length), text(UNDERLAY, underlay)];
        with_kept(set.into_iter().flatten(), extra)
    }
}

impl FabricValues {
    /// The values among `map`'s keys, the rest left in it.
    fn take_from(map: &mut Map<String, Value>) -> Self {
        let tatami = setup_object(map, TATAMI, StitchValues::read);
        let satin = setup_object(map, SATIN, StitchValues::read);
        let stabilizer = take(map, STABILIZER, id);
        let topping = take(map, TOPPING, id);
        let needle_point = take(map, NEEDLE_POINT, id);
        let thickness = take(map, THICKNESS, number);
        Self { tatami, satin, stabilizer, topping, needle_point, thickness, extra: Map::new() }
    }

    /// Whether nothing is set and nothing a newer version wrote is kept.
    pub fn is_empty(&self) -> bool {
        let Self { tatami, satin, stabilizer, topping, needle_point, thickness, extra } = self;
        tatami.as_ref().is_none_or(StitchValues::is_empty)
            && satin.as_ref().is_none_or(StitchValues::is_empty)
            && stabilizer.is_none()
            && topping.is_none()
            && needle_point.is_none()
            && thickness.is_none()
            && extra.is_empty()
    }

    /// Its values as keys of an object, the kept keys left out (each place writes them after its
    /// own).
    fn pairs(self) -> (Vec<(&'static str, Value)>, Map<String, Value>) {
        let Self { tatami, satin, stabilizer, topping, needle_point, thickness, extra } = self;
        let stitch = |key: &'static str, v: Option<StitchValues>| v.filter(|v| !v.is_empty()).map(|v| (key, Value::from(v)));
        let set = [
            stitch(TATAMI, tatami),
            stitch(SATIN, satin),
            text(STABILIZER, stabilizer),
            text(TOPPING, topping),
            text(NEEDLE_POINT, needle_point),
            num(THICKNESS, thickness),
        ];
        (set.into_iter().flatten().collect(), extra)
    }
}

impl From<FabricValues> for Value {
    fn from(v: FabricValues) -> Self {
        let (set, extra) = v.pairs();
        with_kept(set, extra)
    }
}

impl FabricPreset {
    fn read(mut map: Map<String, Value>) -> Option<Self> {
        // A preset is named: one without an id string is kept as it is, unread.
        let preset_id = take(&mut map, ID, id)?;
        let values = setup_object(&mut map, VALUES, |mut inner| {
            let mut v = FabricValues::take_from(&mut inner);
            v.extra = inner;
            Some(v)
        })
        .unwrap_or_default();
        Some(Self { id: preset_id, values, extra: map })
    }
}

impl From<FabricPreset> for Value {
    fn from(p: FabricPreset) -> Self {
        let values = (!p.values.is_empty()).then(|| (VALUES, Value::from(p.values)));
        with_kept(std::iter::once((ID, Value::String(p.id))).chain(values), p.extra)
    }
}

impl FabricSettings {
    /// The fabric as a file holds it, when it is an object; `None` when nothing is in it.
    pub(super) fn read(mut map: Map<String, Value>) -> Option<Self> {
        let preset = match map.remove(PRESET) {
            Some(Value::Object(p)) => match FabricPreset::read(p.clone()) {
                Some(p) => Some(p),
                None => {
                    map.insert(PRESET.to_string(), Value::Object(p));
                    None
                }
            },
            Some(Value::Null) | None => None,
            Some(other) => {
                map.insert(PRESET.to_string(), other);
                None
            }
        };
        let values = FabricValues::take_from(&mut map);
        let color = take(&mut map, COLOR, color);
        let f = Self { preset, values, color, extra: map };
        (!f.is_empty()).then_some(f)
    }

    /// Whether nothing is set and nothing a newer version wrote is kept.
    pub fn is_empty(&self) -> bool {
        let Self { preset, values, color, extra } = self;
        preset.is_none() && values.is_empty() && color.is_none() && extra.is_empty()
    }
}

impl From<FabricSettings> for Value {
    fn from(f: FabricSettings) -> Self {
        let FabricSettings { preset, values, color, extra } = f;
        let (mut set, values_kept) = values.pairs();
        if let Some(p) = preset {
            set.push((PRESET, Value::from(p)));
        }
        if let Some([r, g, b]) = color {
            set.push((COLOR, Value::String(format!("#{r:02x}{g:02x}{b:02x}"))));
        }
        let mut kept = extra;
        for (k, v) in values_kept {
            kept.entry(k).or_insert(v);
        }
        with_kept(set, kept)
    }
}

impl MachineSettings {
    /// The machine's values as a file holds them, when they are an object; `None` when nothing is
    /// in them.
    pub(super) fn read(mut map: Map<String, Value>) -> Option<Self> {
        let jump_length = take(&mut map, JUMP_LENGTH, number);
        let trim_length = take(&mut map, TRIM_LENGTH, number);
        let min_stitch = take(&mut map, MIN_STITCH, number);
        let split_length = take(&mut map, SPLIT_LENGTH, number);
        let speed = take(&mut map, SPEED, number);
        let m = Self { jump_length, trim_length, min_stitch, split_length, speed, extra: map };
        (!m.is_empty()).then_some(m)
    }

    /// Whether nothing is set and nothing a newer version wrote is kept.
    pub fn is_empty(&self) -> bool {
        let Self { jump_length, trim_length, min_stitch, split_length, speed, extra } = self;
        jump_length.is_none() && trim_length.is_none() && min_stitch.is_none() && split_length.is_none() && speed.is_none() && extra.is_empty()
    }
}

impl From<MachineSettings> for Value {
    fn from(m: MachineSettings) -> Self {
        let MachineSettings { jump_length, trim_length, min_stitch, split_length, speed, extra } = m;
        let set = [
            num(JUMP_LENGTH, jump_length),
            num(TRIM_LENGTH, trim_length),
            num(MIN_STITCH, min_stitch),
            num(SPLIT_LENGTH, split_length),
            num(SPEED, speed),
        ];
        with_kept(set.into_iter().flatten(), extra)
    }
}

/// The keys the document's fabric and machine values are saved under, each as the commands name it
/// and as the file saves it (for the engine to hold to its vocabulary).
pub(super) const FILE_KEYS: [(&str, &str); 15] = [
    ("pull", PULL),
    ("spacing", SPACING),
    ("spacingPercent", SPACING_PERCENT),
    ("length", LENGTH),
    ("underlay", UNDERLAY),
    ("stabilizer", STABILIZER),
    ("topping", TOPPING),
    ("needlePoint", NEEDLE_POINT),
    ("thickness", THICKNESS),
    ("color", COLOR),
    ("jumpLength", JUMP_LENGTH),
    ("trimLength", TRIM_LENGTH),
    ("minStitch", MIN_STITCH),
    ("splitLength", SPLIT_LENGTH),
    ("speed", SPEED),
];
