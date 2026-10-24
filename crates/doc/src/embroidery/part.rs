//! The embroidery settings an object keeps for its fills and its strokes ([`PartSettings`]): how
//! each part is sewn, each value set or unset, and where its needle goes in and comes out.
//!
//! One set of settings for all of an object's fills and one for all of its strokes (the project
//! owner's choice, 2026-10-23): the plan numbers an object's fills and strokes in appearance order,
//! a number that changes when one is added, removed or reordered, so settings are kept by the kind
//! of part instead. A basic appearance, the only kind sewn as it is, has one of each at most.
//!
//! Values are kept in the vocabulary's units (millimetres, degrees counter-clockwise on the page,
//! percent), under the vocabulary's names in snake_case ([`PartKey`]), and read as the rest of an
//! object's embroidery is read: a value of another type than this version reads is kept as it was
//! and read as unset, a key this version doesn't know is kept, and an id it doesn't know (a stitch
//! kind of a newer version) is kept as it was written; nothing in them stops a file from opening.
//! Their ranges are the commands' to check: a number of the right type is read whatever it is.

use serde_json::{Map, Value};
use vectorcraft_geom::Point;

use super::{id, number, point, sides, take, with_kept};

/// The parts of an object settings are kept for: its fills and its strokes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartKind {
    Fill,
    Stroke,
}

impl PartKind {
    /// Both, fills first.
    pub const ALL: [Self; 2] = [Self::Fill, Self::Stroke];

    /// Its name in commands, replies and the file: `fill`, `stroke`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Stroke => "stroke",
        }
    }

    /// The part `id` names ([`PartKind::id`]).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// Each setting a part keeps: its name in commands and replies, and its key in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartKey {
    /// How the part is sewn: an id of the vocabulary (`auto`, `tatami`, `satin`, `running`,
    /// `none`), kept as it was written.
    Stitch,
    /// Millimetres between threads.
    Spacing,
    /// The longest stitch, millimetres.
    Length,
    /// The direction a fill's rows run, degrees counter-clockwise on the page.
    Angle,
    /// Pull compensation, millimetres a side.
    Pull,
    /// Pull compensation as a share of the stitch, percent a side.
    PullPercent,
    /// Push compensation, millimetres an end.
    Push,
    /// How far a filled shape grows all round, millimetres.
    Expand,
    /// The widest part of a fill sewn as a satin column, millimetres.
    NarrowAsSatin,
    /// What is sewn under it first: an id of the vocabulary, kept as it was written.
    Underlay,
    /// The longest satin stitch before it is split, millimetres.
    SplitLength,
    /// Where the needle goes in to sew the part, document points.
    Entry,
    /// Where the needle comes out, document points.
    Exit,
}

impl PartKey {
    /// Every one, in the order replies list them.
    pub const ALL: [Self; 13] = [
        Self::Stitch,
        Self::Spacing,
        Self::Length,
        Self::Angle,
        Self::Pull,
        Self::PullPercent,
        Self::Push,
        Self::Expand,
        Self::NarrowAsSatin,
        Self::Underlay,
        Self::SplitLength,
        Self::Entry,
        Self::Exit,
    ];

    /// Its name in commands and replies (camelCase).
    pub fn name(self) -> &'static str {
        match self {
            Self::Stitch => "stitch",
            Self::Spacing => "spacing",
            Self::Length => "length",
            Self::Angle => "angle",
            Self::Pull => "pull",
            Self::PullPercent => "pullPercent",
            Self::Push => "push",
            Self::Expand => "expand",
            Self::NarrowAsSatin => "narrowAsSatin",
            Self::Underlay => "underlay",
            Self::SplitLength => "splitLength",
            Self::Entry => "entry",
            Self::Exit => "exit",
        }
    }

    /// Its key in the file, in the part's object (the points' in its `points` object): the name in
    /// snake_case.
    pub fn file(self) -> &'static str {
        match self {
            Self::Stitch => STITCH,
            Self::Spacing => SPACING,
            Self::Length => LENGTH,
            Self::Angle => ANGLE,
            Self::Pull => PULL,
            Self::PullPercent => PULL_PERCENT,
            Self::Push => PUSH,
            Self::Expand => EXPAND,
            Self::NarrowAsSatin => NARROW_AS_SATIN,
            Self::Underlay => UNDERLAY,
            Self::SplitLength => SPLIT_LENGTH,
            Self::Entry => ENTRY,
            Self::Exit => EXIT,
        }
    }

    /// The key whose name is `name` ([`PartKey::name`]).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Whether it is a point ([`PartKey::Entry`], [`PartKey::Exit`]), kept in the part's `points`.
    pub fn is_point(self) -> bool {
        matches!(self, Self::Entry | Self::Exit)
    }
}

/// A part's setting's value ([`PartSettings::get`], [`PartSettings::set`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PartValue {
    /// An id ([`PartKey::Stitch`], [`PartKey::Underlay`]).
    Id(String),
    /// A number.
    Number(f64),
    /// One number a side ([`PartKey::Pull`], [`PartKey::PullPercent`], [`PartKey::Push`]).
    Sides([f64; 2]),
    /// A point, document points ([`PartKey::Entry`], [`PartKey::Exit`]).
    Point(Point),
}

/// How one part of an object (its fills, or its strokes) is sewn, each setting `None` while unset:
/// an unset value comes from the export's option, else the document's settings, else the built-in
/// default. Saved under the part's key (`fill`, `stroke`) in the object's embroidery.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartSettings {
    /// How it is sewn ([`PartKey::Stitch`]).
    pub stitch: Option<String>,
    pub spacing: Option<f64>,
    pub length: Option<f64>,
    /// Degrees counter-clockwise on the page; turns with the object.
    pub angle: Option<f64>,
    pub pull: Option<[f64; 2]>,
    pub pull_percent: Option<[f64; 2]>,
    pub push: Option<[f64; 2]>,
    pub expand: Option<f64>,
    pub narrow_as_satin: Option<f64>,
    /// What is sewn under it first ([`PartKey::Underlay`]).
    pub underlay: Option<String>,
    pub split_length: Option<f64>,
    /// Where its needle goes in and comes out, and positions a newer version keeps: saved as
    /// `points`. `None`: none.
    pub points: Option<PartPoints>,
    /// Keys this version doesn't know, and known keys whose value it can't read, kept as they are.
    pub extra: super::Kept,
}

/// A part's positions ([`PartSettings::points`]), document points, saved as `[x, y]`: they move
/// with the object, and so does every `[x, y]` in it, those under keys this version doesn't read
/// included (a newer version's satin rails or angle lines, `docs/embroidery.md` › Settings and
/// transforms).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartPoints {
    /// Where the needle goes in to sew the part.
    pub entry: Option<Point>,
    /// Where it comes out.
    pub exit: Option<Point>,
    /// Keys this version doesn't read (and an entry or exit that isn't a point), kept as they are.
    pub extra: super::Kept,
}

const STITCH: &str = "stitch";
const SPACING: &str = "spacing";
const LENGTH: &str = "length";
const ANGLE: &str = "angle";
const PULL: &str = "pull";
const PULL_PERCENT: &str = "pull_percent";
const PUSH: &str = "push";
const EXPAND: &str = "expand";
const NARROW_AS_SATIN: &str = "narrow_as_satin";
const UNDERLAY: &str = "underlay";
const SPLIT_LENGTH: &str = "split_length";
/// The key a part keeps its positions under ([`PartSettings::points`]).
pub(super) const POINTS: &str = "points";
const ENTRY: &str = "entry";
const EXIT: &str = "exit";

impl PartPoints {
    /// The points as a file holds them, when they are an object: its entry and exit when each is
    /// `[x, y]`, its other keys kept. `None` when nothing is in it.
    fn read(mut map: Map<String, Value>) -> Option<Self> {
        let entry = take(&mut map, ENTRY, point);
        let exit = take(&mut map, EXIT, point);
        let p = Self { entry, exit, extra: map.into() };
        (!p.is_empty()).then_some(p)
    }

    /// Whether nothing is in it, read or kept.
    pub fn is_empty(&self) -> bool {
        let Self { entry, exit, extra } = self;
        entry.is_none() && exit.is_none() && extra.is_empty()
    }
}

impl From<PartPoints> for Value {
    fn from(p: PartPoints) -> Self {
        let xy = |q: Point| serde_json::json!([q.x, q.y]);
        let set = p.entry.map(|q| (ENTRY, xy(q))).into_iter().chain(p.exit.map(|q| (EXIT, xy(q))));
        with_kept(set, std::sync::Arc::unwrap_or_clone(p.extra))
    }
}

impl PartSettings {
    /// The settings as a file holds them under a part's key, when they are an object (see
    /// [`PartSettings`]). `None` when nothing is in them, read or kept: such a part is unset.
    pub(super) fn read(mut map: Map<String, Value>) -> Option<Self> {
        let stitch = take(&mut map, STITCH, id);
        let spacing = take(&mut map, SPACING, number);
        let length = take(&mut map, LENGTH, number);
        let angle = take(&mut map, ANGLE, number);
        let pull = take(&mut map, PULL, sides);
        let pull_percent = take(&mut map, PULL_PERCENT, sides);
        let push = take(&mut map, PUSH, sides);
        let expand = take(&mut map, EXPAND, number);
        let narrow_as_satin = take(&mut map, NARROW_AS_SATIN, number);
        let underlay = take(&mut map, UNDERLAY, id);
        let split_length = take(&mut map, SPLIT_LENGTH, number);
        // Points that aren't an object are kept as they are, unread.
        let points = match map.remove(POINTS) {
            Some(Value::Object(points)) => PartPoints::read(points),
            Some(Value::Null) | None => None,
            Some(other) => {
                map.insert(POINTS.to_string(), other);
                None
            }
        };
        let part = Self {
            stitch,
            spacing,
            length,
            angle,
            pull,
            pull_percent,
            push,
            expand,
            narrow_as_satin,
            underlay,
            split_length,
            points,
            extra: map.into(),
        };
        (!part.is_empty()).then_some(part)
    }

    /// Whether nothing is set and nothing a newer version wrote is kept: the object then keeps no
    /// such part. Every field is named, so that one added and left out here fails to compile.
    pub fn is_empty(&self) -> bool {
        let Self { stitch, spacing, length, angle, pull, pull_percent, push, expand, narrow_as_satin, underlay, split_length, points, extra } = self;
        stitch.is_none()
            && spacing.is_none()
            && length.is_none()
            && angle.is_none()
            && pull.is_none()
            && pull_percent.is_none()
            && push.is_none()
            && expand.is_none()
            && narrow_as_satin.is_none()
            && underlay.is_none()
            && split_length.is_none()
            && points.as_ref().is_none_or(PartPoints::is_empty)
            && extra.is_empty()
    }

    /// The value of `key`, `None` while unset.
    pub fn get(&self, key: PartKey) -> Option<PartValue> {
        let point = |f: fn(&PartPoints) -> Option<Point>| self.points.as_ref().and_then(f).map(PartValue::Point);
        match key {
            PartKey::Stitch => self.stitch.clone().map(PartValue::Id),
            PartKey::Spacing => self.spacing.map(PartValue::Number),
            PartKey::Length => self.length.map(PartValue::Number),
            PartKey::Angle => self.angle.map(PartValue::Number),
            PartKey::Pull => self.pull.map(PartValue::Sides),
            PartKey::PullPercent => self.pull_percent.map(PartValue::Sides),
            PartKey::Push => self.push.map(PartValue::Sides),
            PartKey::Expand => self.expand.map(PartValue::Number),
            PartKey::NarrowAsSatin => self.narrow_as_satin.map(PartValue::Number),
            PartKey::Underlay => self.underlay.clone().map(PartValue::Id),
            PartKey::SplitLength => self.split_length.map(PartValue::Number),
            PartKey::Entry => point(|p| p.entry),
            PartKey::Exit => point(|p| p.exit),
        }
    }

    /// Set `key` to `value`, or unset it (`None`). A value of another type than the key takes
    /// (an id for a number, say) changes nothing, and `false` says so. This replaces whatever a
    /// newer version wrote under the key in a form this version can't read, so that a file never
    /// holds the key twice.
    pub fn set(&mut self, key: PartKey, value: Option<PartValue>) -> bool {
        let number = |v: &Option<PartValue>| match v {
            Some(PartValue::Number(x)) => Some(Some(*x)),
            None => Some(None),
            _ => None,
        };
        let sides = |v: &Option<PartValue>| match v {
            Some(PartValue::Sides(x)) => Some(Some(*x)),
            None => Some(None),
            _ => None,
        };
        let id = |v: &Option<PartValue>| match v {
            Some(PartValue::Id(x)) => Some(Some(x.clone())),
            None => Some(None),
            _ => None,
        };
        let point = |v: &Option<PartValue>| match v {
            Some(PartValue::Point(p)) => Some(Some(*p)),
            None => Some(None),
            _ => None,
        };
        let fits = match key {
            PartKey::Stitch => id(&value).map(|v| self.stitch = v),
            PartKey::Spacing => number(&value).map(|v| self.spacing = v),
            PartKey::Length => number(&value).map(|v| self.length = v),
            PartKey::Angle => number(&value).map(|v| self.angle = v),
            PartKey::Pull => sides(&value).map(|v| self.pull = v),
            PartKey::PullPercent => sides(&value).map(|v| self.pull_percent = v),
            PartKey::Push => sides(&value).map(|v| self.push = v),
            PartKey::Expand => number(&value).map(|v| self.expand = v),
            PartKey::NarrowAsSatin => number(&value).map(|v| self.narrow_as_satin = v),
            PartKey::Underlay => id(&value).map(|v| self.underlay = v),
            PartKey::SplitLength => number(&value).map(|v| self.split_length = v),
            PartKey::Entry | PartKey::Exit => point(&value).map(|v| {
                // Points kept unread (not an object) are replaced by this version's when one is set.
                if self.points.is_none() {
                    if v.is_none() {
                        return;
                    }
                    super::unkeep(&mut self.extra, POINTS);
                }
                let points = self.points.get_or_insert_with(PartPoints::default);
                super::unkeep(&mut points.extra, key.file());
                match key {
                    PartKey::Entry => points.entry = v,
                    _ => points.exit = v,
                }
                if points.is_empty() {
                    self.points = None;
                }
            }),
        }
        .is_some();
        if fits && !key.is_point() {
            super::unkeep(&mut self.extra, key.file());
        }
        fits
    }
}

impl From<PartSettings> for Value {
    /// The settings as they are saved: each value set (per-side values as two numbers), then the
    /// kept keys, which never override a value set.
    fn from(p: PartSettings) -> Self {
        let PartSettings { stitch, spacing, length, angle, pull, pull_percent, push, expand, narrow_as_satin, underlay, split_length, points, extra } =
            p;
        let num = |key: &'static str, v: Option<f64>| v.map(|x| (key, Value::from(x)));
        let pair = |key: &'static str, v: Option<[f64; 2]>| v.map(|[a, b]| (key, serde_json::json!([a, b])));
        let text = |key: &'static str, v: Option<String>| v.map(|s| (key, Value::String(s)));
        let set = [
            text(STITCH, stitch),
            num(SPACING, spacing),
            num(LENGTH, length),
            num(ANGLE, angle),
            pair(PULL, pull),
            pair(PULL_PERCENT, pull_percent),
            pair(PUSH, push),
            num(EXPAND, expand),
            num(NARROW_AS_SATIN, narrow_as_satin),
            text(UNDERLAY, underlay),
            num(SPLIT_LENGTH, split_length),
            points.filter(|p| !p.is_empty()).map(|p| (POINTS, Value::from(p))),
        ];
        with_kept(set.into_iter().flatten(), std::sync::Arc::unwrap_or_clone(extra))
    }
}
