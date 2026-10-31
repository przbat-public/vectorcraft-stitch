//! Machine embroidery data an object carries ([`crate::node::NodeExtras::embroidery`]), and the
//! document's embroidery settings ([`crate::Document::embroidery`]).
//!
//! The stitches themselves are planned outside the document model (the `vectorcraft-stitch`
//! crate, through the engine's embroidery export; see `docs/embroidery.md`). What an object keeps
//! here is what that export must know about it beyond its geometry and paint (how its fills and
//! strokes are sewn: [`PartSettings`]), and what the document keeps is what applies to the whole
//! design.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vectorcraft_geom::Point;

mod marks;
mod mem;
pub mod outline;
mod part;
mod setup;
pub mod takers;
mod transform;

pub(crate) use mem::{object_bytes, setup_bytes};

/// `kept` without `key`, copied only when it holds it (it may be shared).
pub(crate) fn unkeep(kept: &mut Kept, key: &str) {
    if kept.contains_key(key) {
        std::sync::Arc::make_mut(kept).remove(key);
    }
}

/// What an object's settings keep that this version doesn't read (keys a newer version wrote, known
/// keys of another type), as they are, shared: settings copied onto many objects (each piece cut
/// from an object, each object pasted onto) share it rather than each holding a copy, whatever a
/// file put there. Changed through [`Arc::make_mut`].
pub type Kept = std::sync::Arc<serde_json::Map<String, serde_json::Value>>;

pub use marks::{Mark, Placed, SAME_SPOT, shape_of};
pub use part::{PartKey, PartKind, PartPoints, PartSettings, PartValue};
pub use setup::{FabricPreset, FabricSettings, FabricValues, MachineSettings, StitchValues};
pub use transform::scale_factors;

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
    /// The machine stops after the object's stitches (Stop After), as for an appliqué piece to be
    /// laid down, the needle up and the thread kept: a stop where the thread changes, or at the
    /// design's end, is that one. A run of literal stitches says the same with
    /// [`StitchRun::stop_after`].
    pub stop_after: bool,
    /// How the object's fills are sewn: every fill in its appearance ([`PartSettings`]). `None`:
    /// nothing set. A run of literal stitches takes none: its points are sewn as they are.
    pub fill: Option<Box<PartSettings>>,
    /// How its strokes are sewn, as [`Embroidery::fill`].
    pub stroke: Option<Box<PartSettings>>,
    /// The object this one was converted from (Convert to Manual Stitches: a group of manual runs
    /// standing in for it), kept to be restored ([`Original`]). `None`: nothing to restore.
    pub original: Option<Original>,
    /// Keys this version doesn't know (written by a newer one), and known keys whose value this
    /// version can't read, kept as they are so that saving doesn't lose them.
    pub extra: Kept,
    /// What a newer version wrote when it is not an object, which this version can't read:
    /// nothing is taken from it, and it is written back as it was while nothing is set here.
    pub unread: Option<serde_json::Value>,
}

/// A run of literal stitches ([`Embroidery::stitches`]): the object's points in order, the needle
/// down at each. Read and kept as [`Embroidery`] is: what this version doesn't read stays in
/// [`StitchRun::extra`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StitchRun {
    /// The thread is cut before the run, as the file had it (or, for manual stitches, as a stitch
    /// edit passed a cut to it); otherwise the needle jumps to the run's first point with the thread
    /// still on, cut only when the move is long.
    pub trim_before: bool,
    /// The thread was cut after the run, at the end of its colour block, as the file had it
    /// (machines and digitizers often cut before the thread is changed).
    pub trim_after: bool,
    /// The run is two needle points at one spot (a tack). Its line, from a point to the same point,
    /// looks the same as a single stitch's, so this tells them apart: without it, a line that goes
    /// nowhere is one needle point.
    pub double_point: bool,
    /// The run was made in VectorCraft (converted from an object, Convert to Manual Stitches), not
    /// read from a machine file: its stitches are sewn as they are, but placed in the sewing order
    /// as art is (joining a block of its colour where the stacking allows, the move into it cut when
    /// long), and never cut again by a clip group. Absent: a file's run, whose blocks stay as the
    /// file had them. Opening a machine file never sets it.
    pub manual: bool,
    /// The needle points the needle reaches after the thread is cut, each the first of a subpath
    /// after the first; it jumps to every other subpath with the thread kept. Kept by where each
    /// lies ([`Mark`]; [`StitchRun::placed`] finds them on the path as it is now). In the order a
    /// file gives them; a bare subpath number (an earlier form) is that subpath's first anchor.
    pub trims: Vec<Mark>,
    /// The needle points after which the machine stops, the needle then lifted to the next. Kept
    /// by where each lies ([`Mark`]), in the order a file gives them.
    pub stops: Vec<Mark>,
    /// How many subpaths and anchors the path had when its cuts and stops were written
    /// ([`shape_of`]): while it has as many, a mark whose needle point was moved stays at its place.
    /// Saved as `[subpaths, anchors]`, with the marks.
    pub shape: Option<[usize; 2]>,
    /// The machine stops after the run's last needle point (and after its cut, when it has one).
    pub stop_after: bool,
    /// The exact millimetres of needle points that the document points they are stored in can't give
    /// back (a manual run's, as the plan it was frozen from sewed them): a point stored in document
    /// points and read in millimetres may come back a unit in the last place off, and one exactly
    /// halfway between two tenths of a millimetre would then be written a tenth off. Each applies
    /// while its needle point lies where it was stored (within a picometre): a transform of the
    /// object keeps them, so moved and moved back, the run sews as it did.
    pub exact: Vec<Exact>,
    /// The transforms of the run since its stitches were made (read from a machine file, or
    /// converted), composed: recorded from the first one that does more than move it, so that
    /// how much its stitches were scaled is known ([`scale_factors`]). `None`: only moved since,
    /// if at all. Saved as `{"frame": [...]}`, which every version of the fork composes with each
    /// transform by the convention for kept data, so an older version keeps it right.
    pub transformed: Option<vectorcraft_geom::Affine>,
    /// The thread the file recorded for the run's colour block beside its colour (a VP3 thread
    /// record), written back by a format that has one. `None`: the file recorded none.
    pub thread: Option<ThreadRecord>,
    /// Keys this version doesn't read, kept as they are.
    pub extra: Kept,
}

/// A needle point's exact millimetres ([`StitchRun::exact`]): the anchor (subpath, anchor, from 0)
/// and the coordinates its document points can't give back, saved as `[subpath, anchor, x, y]`, a
/// coordinate that comes back as it was `null`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exact {
    pub subpath: usize,
    pub anchor: usize,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

impl Exact {
    /// An entry a file holds, when it is one this version reads: two indices, then each coordinate
    /// a finite number or null, not both null.
    fn read(v: &serde_json::Value) -> Option<Self> {
        let coordinate = |v: &serde_json::Value| match v {
            serde_json::Value::Null => Some(None),
            v => v.as_f64().filter(|x| x.is_finite()).map(Some),
        };
        match v.as_array()?.as_slice() {
            [s, a, x, y] => {
                let (x, y) = (coordinate(x)?, coordinate(y)?);
                (x.is_some() || y.is_some()).then_some(Self { subpath: index(s)?, anchor: index(a)?, x, y })
            }
            _ => None,
        }
    }
}

/// The object a manual object was converted from ([`Embroidery::original`]), saved as
/// `{"frame", "node", "fingerprint"}` with the keys this version doesn't read kept.
///
/// Every version of the fork moves it by the convention for kept data (`transform.rs`): an object
/// holding a `frame` has the frame composed with every transform and nothing else in it moves. So
/// an older version, which keeps it unread, moves it just as this one does.
#[derive(Clone, Debug, PartialEq)]
pub struct Original {
    /// The original's own coordinates to the document's: the identity when converted, composed
    /// with every transform of the manual object since (moved, turned, scaled with it).
    pub frame: vectorcraft_geom::Affine,
    /// The object as the file saved it when it was converted: parsed only when it is restored, so
    /// that what a newer version wrote in it is kept, whatever it is; shared, not copied, by the
    /// copies of the manual object.
    pub node: std::sync::Arc<serde_json::Value>,
    /// The stitches as converted, summed up (their needle points in the original's coordinates,
    /// their cuts and stops): what tells stitches edited since converting from stitches only moved
    /// with their object. `None`: unknown (a file that kept none), so read as edited.
    pub fingerprint: Option<u64>,
    /// Keys this version doesn't read, kept as they are.
    pub extra: Kept,
}

impl Original {
    /// The original a file holds under the `original` key, when it is one this version reads: an
    /// object with a frame of six finite numbers and a node that is an object (a fingerprint that
    /// isn't sixteen hexadecimal digits is kept, unread). `None` otherwise (kept as it is).
    fn read(v: &serde_json::Value) -> Option<Self> {
        let mut map = v.as_object()?.clone();
        let frame = map.get(FRAME).and_then(frame)?;
        if !map.get(NODE).is_some_and(serde_json::Value::is_object) {
            return None;
        }
        map.remove(FRAME);
        let node = std::sync::Arc::new(map.remove(NODE).unwrap_or_default());
        let fingerprint =
            map.get(FINGERPRINT).and_then(|f| f.as_str()).filter(|f| f.len() == FINGERPRINT_DIGITS).and_then(|f| u64::from_str_radix(f, 16).ok());
        if fingerprint.is_some() {
            map.remove(FINGERPRINT);
        }
        Some(Self { frame, node, fingerprint, extra: map.into() })
    }
}

impl From<Original> for serde_json::Value {
    fn from(o: Original) -> Self {
        let set = [
            Some((FRAME, serde_json::json!(o.frame.as_coeffs()))),
            Some((NODE, std::sync::Arc::unwrap_or_clone(o.node))),
            o.fingerprint.map(|f| (FINGERPRINT, serde_json::Value::String(format!("{f:016x}")))),
        ];
        with_kept(set.into_iter().flatten(), std::sync::Arc::unwrap_or_clone(o.extra))
    }
}

/// A frame (six finite numbers, `Affine::new`'s order).
fn frame(v: &serde_json::Value) -> Option<vectorcraft_geom::Affine> {
    let items = v.as_array()?;
    let coeffs: Vec<f64> = items.iter().map(serde_json::Value::as_f64).collect::<Option<_>>()?;
    let coeffs: [f64; 6] = coeffs.try_into().ok()?;
    coeffs.iter().all(|c| c.is_finite()).then(|| vectorcraft_geom::Affine::new(coeffs))
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
    pub extra: Kept,
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
        Some(Self { catalogue, description, brand, material, weight, extra: map.into() })
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
        with_kept(set, std::sync::Arc::unwrap_or_clone(t.extra))
    }
}

/// What comes after an object's stitches: the thread cut, the machine stopped, each or both
/// ([`Embroidery::after`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct After {
    pub trim: bool,
    pub stop: bool,
}

impl After {
    /// Whether anything comes after.
    pub fn any(self) -> bool {
        self.trim || self.stop
    }
}

impl std::ops::BitOr for After {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self { trim: self.trim || other.trim, stop: self.stop || other.stop }
    }
}

impl StitchRun {
    /// The cut and the stop after the run, as its file had them (or as an edit wrote them: a stop
    /// after its last needle point is written as its stop after it).
    pub fn after(&self) -> After {
        After { trim: self.trim_after, stop: self.stop_after }
    }
}

impl Embroidery {
    /// Set whether the thread is cut after the object. This replaces whatever a newer version wrote
    /// under that key in a form this version can't read.
    pub fn set_trim_after(&mut self, on: bool) {
        self.trim_after = on;
        unkeep(&mut self.extra, TRIM_AFTER);
    }

    /// Set whether the machine stops after the object, as [`Embroidery::set_trim_after`] sets the cut.
    pub fn set_stop_after(&mut self, on: bool) {
        self.stop_after = on;
        unkeep(&mut self.extra, STOP_AFTER);
    }

    /// The object's own Trim After and Stop After.
    pub fn own_after(&self) -> After {
        After { trim: self.trim_after, stop: self.stop_after }
    }

    /// Set the object's own Trim After and Stop After to `after`, each only where it differs.
    pub fn set_own_after(&mut self, after: After) {
        if self.trim_after != after.trim {
            self.set_trim_after(after.trim);
        }
        if self.stop_after != after.stop {
            self.set_stop_after(after.stop);
        }
    }

    /// What comes after the object's stitches, wherever it is kept: its own Trim After and Stop
    /// After, and its run's cut and stop after it (a file's, or an edit's). What the Object tab
    /// shows and Restore's fingerprint reads.
    pub fn after(&self) -> After {
        self.own_after() | self.stitches.as_ref().map(StitchRun::after).unwrap_or_default()
    }

    /// The settings of the object's part `kind`, when it keeps any.
    pub fn part(&self, kind: PartKind) -> Option<&PartSettings> {
        match kind {
            PartKind::Fill => self.fill.as_deref(),
            PartKind::Stroke => self.stroke.as_deref(),
        }
    }

    /// Change the settings of part `kind` as `f` does, giving back what `f` gives. A part left with
    /// something in it replaces what a newer version wrote under its key in a form this version
    /// can't read (kept unread, or the whole record unread); a part left with nothing in it is
    /// dropped.
    pub fn edit_part<T>(&mut self, kind: PartKind, f: impl FnOnce(&mut PartSettings) -> T) -> T {
        let slot = match kind {
            PartKind::Fill => &mut self.fill,
            PartKind::Stroke => &mut self.stroke,
        };
        let mut part = slot.take().map(|b| *b).unwrap_or_default();
        let out = f(&mut part);
        if !part.is_empty() {
            *slot = Some(Box::new(part));
            unkeep(&mut self.extra, kind.id());
            self.unread = None;
        }
        out
    }

    /// Whether nothing is set and nothing a newer version wrote is kept: the object then carries
    /// none. Every field is named, so that one added and left out here fails to compile.
    pub fn is_empty(&self) -> bool {
        self.nothing_set() && self.unread.is_none()
    }

    /// Whether nothing is set, whatever is kept: what makes the writer give back a value it
    /// couldn't read as it was. Every field is named, so that one added and left out here fails to
    /// compile.
    fn nothing_set(&self) -> bool {
        let Self { stitches, trim_after, stop_after, fill, stroke, original, extra, unread: _ } = self;
        stitches.is_none() && !trim_after && !stop_after && fill.is_none() && stroke.is_none() && original.is_none() && extra.is_empty()
    }
}

/// The settings an object keeps, each as the commands name it and as the file saves it, for the
/// engine to hold to its vocabulary: the cut after the object, and each part's keys
/// ([`PartKey`]; a point's in the part's `points`).
pub fn file_keys() -> impl Iterator<Item = (&'static str, &'static str)> {
    [("trimAfter", TRIM_AFTER), ("stopAfter", STOP_AFTER)].into_iter().chain(PartKey::ALL.into_iter().map(|k| (k.name(), k.file())))
}

/// The settings the document keeps, each as the commands name it and as the file saves it, for
/// the engine to hold to its vocabulary: its own keys, then its fabric's and machine's values.
pub fn setup_file_keys() -> impl Iterator<Item = (&'static str, &'static str)> {
    let own = [
        ("palette", PALETTE),
        ("start", START),
        ("label", LABEL),
        ("fabric", FABRIC),
        ("threadWeight", THREAD_WEIGHT),
        ("joinSameColor", JOIN_SAME_COLOR),
        ("machine", MACHINE),
    ];
    own.into_iter().chain(setup::FILE_KEYS)
}

/// The keys an object's embroidery and a run of stitches are saved under.
const STITCHES: &str = "stitches";
const TRIM_AFTER: &str = "trim_after";
const STOP_AFTER: &str = "stop_after";
const ORIGINAL: &str = "original";
const TRIM_BEFORE: &str = "trim_before";
const DOUBLE_POINT: &str = "double_point";
const MANUAL: &str = "manual";
const TRIMS: &str = "trims";
const STOPS: &str = "stops";
const SHAPE: &str = "shape";
const EXACT: &str = "exact";
const TRANSFORMED: &str = "transformed";
const THREAD: &str = "thread";
/// The keys of an original ([`Original`]); a run's transforms are a frame too.
const FRAME: &str = "frame";
const NODE: &str = "node";
const FINGERPRINT: &str = "fingerprint";
/// A fingerprint's hexadecimal digits: a 64-bit number.
const FINGERPRINT_DIGITS: usize = 16;
/// The most entries a run's `trims` or `stops` list is read with: one for every needle point of
/// the largest design (2 million stitches, as a machine file's reader takes); a longer list is
/// kept as it is, unread, a bound on what a hostile file makes this version hold.
pub const MAX_MARKS: usize = 2_000_000;
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

/// Take part `kind`'s settings out of `map` when they are an object ([`PartSettings`]); settings
/// of another shape stay in `map`, to be written back as they were, and settings with nothing in
/// them read as none.
fn take_part(map: &mut serde_json::Map<String, serde_json::Value>, kind: PartKind) -> Option<Box<PartSettings>> {
    match map.remove(kind.id()) {
        Some(serde_json::Value::Object(part)) => PartSettings::read(part).map(Box::new),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => {
            map.insert(kind.id().to_string(), other);
            None
        }
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

/// An index a list of a run's marks holds: a whole number from 0 that this platform's indices
/// hold.
fn index(v: &serde_json::Value) -> Option<usize> {
    v.as_u64().and_then(|n| usize::try_from(n).ok())
}

/// Take list `key` out of `map` when every entry is one `read` reads and there are at most
/// [`MAX_MARKS`] of them (a missing key, or a null, is an empty list); a list of another shape, or
/// longer, stays in `map`, to be written back as it was.
fn take_list<T>(map: &mut serde_json::Map<String, serde_json::Value>, key: &str, read: impl Fn(&serde_json::Value) -> Option<T>) -> Vec<T> {
    let read = match map.get(key) {
        None | Some(serde_json::Value::Null) => Some(Vec::new()),
        Some(serde_json::Value::Array(items)) if items.len() <= MAX_MARKS => items.iter().map(read).collect(),
        Some(_) => None,
    };
    match read {
        Some(list) => {
            map.remove(key);
            list
        }
        None => Vec::new(),
    }
}

impl From<serde_json::Map<String, serde_json::Value>> for StitchRun {
    fn from(mut map: serde_json::Map<String, serde_json::Value>) -> Self {
        let trim_before = take_flag(&mut map, TRIM_BEFORE);
        let trim_after = take_flag(&mut map, TRIM_AFTER);
        let double_point = take_flag(&mut map, DOUBLE_POINT);
        let manual = take_flag(&mut map, MANUAL);
        let stop_after = take_flag(&mut map, STOP_AFTER);
        // A bare subpath number (an earlier form) is a cut before that subpath's first anchor.
        let trims = take_list(&mut map, TRIMS, |v| index(v).map(|s| Mark::place(s, 0)).or_else(|| Mark::read(v)));
        let stops = take_list(&mut map, STOPS, Mark::read);
        let shape = match map.get(SHAPE).and_then(|v| v.as_array()).map(Vec::as_slice) {
            Some([subpaths, anchors]) => index(subpaths).zip(index(anchors)).map(|(s, a)| [s, a]),
            _ => None,
        };
        if shape.is_some() || map.get(SHAPE).is_some_and(serde_json::Value::is_null) {
            map.remove(SHAPE);
        }
        let exact = take_list(&mut map, EXACT, Exact::read);
        // An object holding only a frame; anything else a newer version wrote there is kept as it
        // is, unread (and composed with each transform all the same, by the convention).
        let transformed = map.get(TRANSFORMED).and_then(|v| v.as_object()).filter(|m| m.len() == 1).and_then(|m| m.get(FRAME)).and_then(frame);
        if transformed.is_some() {
            map.remove(TRANSFORMED);
        }
        // A record of another shape is kept as it is, unread.
        let thread = map.get(THREAD).and_then(ThreadRecord::read);
        if thread.is_some() {
            map.remove(THREAD);
        }
        Self { trim_before, trim_after, double_point, manual, trims, stops, shape, stop_after, exact, transformed, thread, extra: map.into() }
    }
}

impl From<StitchRun> for serde_json::Value {
    /// The run as it is saved: its cuts, stops and mark when there are, then the keys kept.
    fn from(r: StitchRun) -> Self {
        let flag = |key: &'static str, on: bool| on.then_some((key, serde_json::Value::Bool(true)));
        let flags = flag(TRIM_BEFORE, r.trim_before)
            .into_iter()
            .chain(flag(TRIM_AFTER, r.trim_after))
            .chain(flag(DOUBLE_POINT, r.double_point))
            .chain(flag(MANUAL, r.manual))
            .chain(flag(STOP_AFTER, r.stop_after));
        let marks =
            |key: &'static str, list: &[Mark]| (!list.is_empty()).then(|| (key, serde_json::Value::Array(list.iter().map(|m| m.json()).collect())));
        let trims = marks(TRIMS, &r.trims);
        let stops = marks(STOPS, &r.stops);
        let shape = r.shape.map(|shape| (SHAPE, serde_json::json!(shape)));
        let exact = (!r.exact.is_empty())
            .then(|| (EXACT, serde_json::Value::Array(r.exact.iter().map(|e| serde_json::json!([e.subpath, e.anchor, e.x, e.y])).collect())));
        let transformed = r.transformed.map(|f| (TRANSFORMED, serde_json::json!({ FRAME: f.as_coeffs() })));
        let thread = r.thread.map(|t| (THREAD, serde_json::Value::from(t)));
        with_kept(
            flags.chain(trims).chain(stops).chain(shape).chain(exact).chain(transformed).chain(thread),
            std::sync::Arc::unwrap_or_clone(r.extra),
        )
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
                let stop_after = take_flag(&mut map, STOP_AFTER);
                let fill = take_part(&mut map, PartKind::Fill);
                let stroke = take_part(&mut map, PartKind::Stroke);
                // An original of another shape is kept as it is, unread: nothing to restore.
                let original = map.get(ORIGINAL).and_then(Original::read);
                if original.is_some() {
                    map.remove(ORIGINAL);
                }
                Self { stitches, trim_after, stop_after, fill, stroke, original, extra: map.into(), unread: None }
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
        if e.nothing_set()
            && let Some(unread) = e.unread
        {
            return unread;
        }
        let Embroidery { stitches, trim_after, stop_after, fill, stroke, original, extra, unread: _ } = e;
        let run = stitches.map(|r| (STITCHES, serde_json::Value::from(r)));
        let cut = trim_after.then_some((TRIM_AFTER, serde_json::Value::Bool(true)));
        let stop = stop_after.then_some((STOP_AFTER, serde_json::Value::Bool(true)));
        let part = |kind: PartKind, p: Option<Box<PartSettings>>| p.filter(|p| !p.is_empty()).map(|p| (kind.id(), serde_json::Value::from(*p)));
        let parts = part(PartKind::Fill, fill).into_iter().chain(part(PartKind::Stroke, stroke));
        let original = original.map(|o| (ORIGINAL, serde_json::Value::from(o)));
        with_kept(run.into_iter().chain(cut).chain(stop).chain(parts).chain(original), std::sync::Arc::unwrap_or_clone(extra))
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
    /// The fabric the design is sewn on: its values, copied from a preset or the person's own
    /// ([`FabricSettings`]). `None`: the built-in defaults.
    pub fabric: Option<Box<FabricSettings>>,
    /// The thread's weight (wt): the spacing of fills and satin columns follows its thickness.
    /// `None`: 40 wt.
    pub thread_weight: Option<f64>,
    /// Whether objects of one colour are sewn in one block where nothing lies between them; off,
    /// only consecutive objects of one colour are. `None`: on.
    pub join_same_color: Option<bool>,
    /// The machine's values: jumps, trims, the shortest stitch, the satin split, its speed
    /// ([`MachineSettings`]). `None`: the built-in defaults.
    pub machine: Option<Box<MachineSettings>>,
    /// Keys this version doesn't know (written by a newer one), and known keys whose value this
    /// version can't read, kept so that saving doesn't lose them, as [`crate::Document::extra`]
    /// keeps the document's.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// The settings as a newer version wrote them when they are not an object (`"embroidery": 5`),
    /// which this version can't read: nothing is taken from them, and they are written back as they
    /// are while nothing is set here. Setting the palette replaces them ([`EmbroiderySetup::set_palette`]).
    pub unread: Option<serde_json::Value>,
}

/// The keys the document's settings are saved under.
const PALETTE: &str = "palette";
const START: &str = "start";
const LABEL: &str = "label";
const FABRIC: &str = "fabric";
const THREAD_WEIGHT: &str = "thread_weight";
const JOIN_SAME_COLOR: &str = "join_same_color";
const MACHINE: &str = "machine";

impl EmbroiderySetup {
    /// Whether nothing is set and nothing a newer version wrote is kept: the document then carries
    /// none. Every field is named, so that one added and left out here fails to compile.
    pub fn is_empty(&self) -> bool {
        self.nothing_set() && self.unread.is_none()
    }

    /// Whether nothing is set, whatever is kept unread: what makes the writer give back a value it
    /// couldn't read as it was.
    fn nothing_set(&self) -> bool {
        let Self { palette, start, label, fabric, thread_weight, join_same_color, machine, extra, unread: _ } = self;
        palette.is_none()
            && start.is_none()
            && label.is_none()
            && fabric.is_none()
            && thread_weight.is_none()
            && join_same_color.is_none()
            && machine.is_none()
            && extra.is_empty()
    }

    /// Set the fabric, or unset it (`None`, or one with nothing in it: the built-in defaults). Like
    /// the palette, a fabric set replaces settings this version can't read, and a value a newer
    /// version wrote under its key in a form this version can't read.
    pub fn set_fabric(&mut self, fabric: Option<FabricSettings>) {
        self.fabric = self.replacing(FABRIC, fabric.filter(|f| !f.is_empty())).map(Box::new);
    }

    /// Set the thread's weight (wt), or unset it (`None`: 40 wt), as [`EmbroiderySetup::set_fabric`]
    /// sets the fabric.
    pub fn set_thread_weight(&mut self, weight: Option<f64>) {
        self.thread_weight = self.replacing(THREAD_WEIGHT, weight);
    }

    /// Set whether objects of one colour join one block where nothing lies between them, or unset
    /// it (`None`: they do), as [`EmbroiderySetup::set_fabric`] sets the fabric.
    pub fn set_join_same_color(&mut self, on: Option<bool>) {
        self.join_same_color = self.replacing(JOIN_SAME_COLOR, on);
    }

    /// Set the machine's values, or unset them (`None`, or values with nothing in them: the
    /// built-in defaults), as [`EmbroiderySetup::set_fabric`] sets the fabric.
    pub fn set_machine(&mut self, machine: Option<MachineSettings>) {
        self.machine = self.replacing(MACHINE, machine.filter(|m| !m.is_empty())).map(Box::new);
    }

    /// `value`, about to be set under `key`: when it is set, what this version couldn't read (the
    /// settings unread, a kept value of the key) goes.
    fn replacing<T>(&mut self, key: &str, value: Option<T>) -> Option<T> {
        if value.is_some() {
            self.unread = None;
            self.extra.remove(key);
        }
        value
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

    /// Set where the machine starts the needle (document points), or unset it (`None`), as
    /// [`EmbroiderySetup::set_fabric`] sets the fabric: a start set replaces settings this version
    /// can't read, and a start of another shape kept under its key.
    pub fn set_start(&mut self, start: Option<Point>) {
        self.start = self.replacing(START, start);
    }

    /// Set the design's name the export writes, or unset it (`None`: the document's title), as
    /// [`EmbroiderySetup::set_start`] sets the start.
    pub fn set_label(&mut self, label: Option<String>) {
        self.label = self.replacing(LABEL, label);
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
                let fabric = setup_object(&mut map, FABRIC, FabricSettings::read).map(Box::new);
                let thread_weight = take(&mut map, THREAD_WEIGHT, number);
                let join_same_color = take(&mut map, JOIN_SAME_COLOR, Value::as_bool);
                let machine = setup_object(&mut map, MACHINE, MachineSettings::read).map(Box::new);
                Self { palette, start, label, fabric, thread_weight, join_same_color, machine, extra: map, unread: None }
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
        if s.nothing_set()
            && let Some(unread) = s.unread
        {
            return unread;
        }
        let EmbroiderySetup { palette, start, label, fabric, thread_weight, join_same_color, machine, extra, unread: _ } = s;
        let set = [
            palette.map(|p| (PALETTE, Value::from(p))),
            start.map(|p| (START, serde_json::json!([p.x, p.y]))),
            label.map(|l| (LABEL, Value::String(l))),
            fabric.filter(|f| !f.is_empty()).map(|f| (FABRIC, Value::from(*f))),
            thread_weight.map(|w| (THREAD_WEIGHT, Value::from(w))),
            join_same_color.map(|on| (JOIN_SAME_COLOR, Value::Bool(on))),
            machine.filter(|m| !m.is_empty()).map(|m| (MACHINE, Value::from(*m))),
        ];
        with_kept(set.into_iter().flatten(), extra)
    }
}

/// Take key `key` out of `map` when `read` reads its value (a null is unset and goes too); a
/// value it can't read stays in `map`, to be written back as it was.
fn take<T>(map: &mut Map<String, Value>, key: &str, read: impl Fn(&Value) -> Option<T>) -> Option<T> {
    let value = map.get(key)?;
    if value.is_null() {
        map.remove(key);
        return None;
    }
    let read = read(value)?;
    map.remove(key);
    Some(read)
}

/// A number (JSON's are finite).
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

/// One number for both sides, or two: `[a, b]`.
fn sides(v: &Value) -> Option<[f64; 2]> {
    match v {
        Value::Array(a) => match a.as_slice() {
            [a, b] => Some([number(a)?, number(b)?]),
            _ => None,
        },
        one => number(one).map(|x| [x; 2]),
    }
}

/// An id: a string.
fn id(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// Take key `key`'s object out of the document's settings `map` and read it with `read` (`None`
/// when nothing is in it); a value that isn't an object stays in `map`, to be written back as it
/// was.
fn setup_object<T>(map: &mut Map<String, Value>, key: &str, read: impl FnOnce(Map<String, Value>) -> Option<T>) -> Option<T> {
    match map.remove(key) {
        Some(Value::Object(inner)) => read(inner),
        Some(Value::Null) | None => None,
        Some(other) => {
            map.insert(key.to_string(), other);
            None
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
            json!({"palette": "pec", "fabric": {"preset": "knit", "pull": 0.35}, "thread_weight": 60.0, "hoopSize": 100}),
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
        // A start or a label set replaces one of another shape kept under its key, so that unsetting
        // it later doesn't bring the old one back.
        let mut odd: EmbroiderySetup = serde_json::from_value(json!({"start": "centre", "label": 7})).unwrap();
        odd.set_start(Some(Point::new(1.0, 2.0)));
        odd.set_label(Some("ACME".into()));
        assert!(odd.extra.is_empty(), "{:?}", odd.extra);
        odd.set_start(None);
        assert_eq!(serde_json::to_value(&odd).unwrap(), json!({"label": "ACME"}));
    }

    /// An object's embroidery round-trips as it was written: its run and cut, keys this version
    /// doesn't know, known keys of another type (read as unset), and a value that isn't an object.
    #[test]
    fn an_objects_embroidery_round_trips_with_what_a_newer_version_wrote() {
        for written in [
            json!({"trim_after": true}),
            json!({"stitches": {"trim_before": true, "trim_after": true}}),
            json!({"stitches": {"trim_before": true, "density": 4}, "fill": {"angle": 30.0}}),
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

    /// A run's mark, cuts, stops and stop, and an object's Stop After and kept original, round-trip
    /// as they were written; of another shape (a mark that isn't true or false, a list holding
    /// something that isn't an index, an original without a frame of six numbers and a node), they
    /// are kept as they were and read as unset, so nothing a newer version or another maker wrote is
    /// lost.
    #[test]
    fn marks_stops_and_originals_round_trip() {
        let original = json!({"frame": [1.0, 0.0, 0.0, 1.0, 10.0, -5.5], "node": {"id": 7, "kind": {"type": "path"}}, "fingerprint": "00000000deadbeef", "made": "x"});
        for written in [
            json!({"stitches": {"manual": true, "trims": [[2, 0, 1.5, -2.0], [1, 0]], "stops": [[0, 3, 4.0, 5.0], [1, 0]], "shape": [3, 12], "stop_after": true}}),
            json!({ "stop_after": true, "original": original.clone() }),
            json!({"stitches": {"stops": [[0, 3, 4.0]], "shape": [3], "trims": [[1, 0, "x", 2.0]]}}),
            json!({"stitches": {"manual": "yes", "trims": "all", "stops": [[1]], "stop_after": 1}}),
            json!({"stitches": {"trims": [1, -2], "stops": [[0, 1.5]]}}),
            json!({"original": 7, "stop_after": "after"}),
            json!({"original": {"frame": [1, 2], "node": {}}}),
            json!({"original": {"frame": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "node": [1]}}),
            json!({"original": {"frame": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "node": {}, "fingerprint": "beef"}}),
            json!({"stitches": {"manual": true, "exact": [[0, 1, 51.75, null], [2, 0, null, 3.5]]}}),
            json!({"stitches": {"exact": [[0, 1, null, null]]}}),
        ] {
            let read: Embroidery = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written, "{written}");
        }
        let read: Embroidery = serde_json::from_value(
            json!({"stitches": {"manual": true, "trims": [[2, 0, 1.5, -2.0]], "stops": [[0, 3, 4.0, 5.0], [1, 0]], "shape": [3, 12], "stop_after": true}}),
        )
        .unwrap();
        let run = read.stitches.unwrap();
        let (trims, stops) = (vec![Mark::at(2, 0, Point::new(1.5, -2.0))], vec![Mark::at(0, 3, Point::new(4.0, 5.0)), Mark::place(1, 0)]);
        assert_eq!((run.manual, run.trims, run.stops, run.shape, run.stop_after, run.extra.len()), (true, trims, stops, Some([3, 12]), true, 0));
        // A bare subpath number, as an earlier form wrote a cut: that subpath's first anchor.
        let earlier: Embroidery = serde_json::from_value(json!({"stitches": {"trims": [2, 1]}})).unwrap();
        assert_eq!(earlier.stitches.unwrap().trims, [Mark::place(2, 0), Mark::place(1, 0)]);
        let junk: Embroidery =
            serde_json::from_value(json!({"stitches": {"manual": "yes", "trims": [1, -2], "stops": [[1]], "stop_after": 1}})).unwrap();
        let run = junk.stitches.unwrap();
        assert_eq!((run.manual, run.trims.len(), run.stops.len(), run.stop_after, run.extra.len()), (false, 0, 0, false, 4));
        let read: Embroidery = serde_json::from_value(json!({ "stop_after": true, "original": original })).unwrap();
        let o = read.original.unwrap();
        assert!(read.stop_after);
        assert_eq!(
            (o.frame, o.fingerprint, o.extra.get("made")),
            (vectorcraft_geom::Affine::new([1.0, 0.0, 0.0, 1.0, 10.0, -5.5]), Some(0xdead_beef), Some(&json!("x")))
        );
        let odd: Embroidery = serde_json::from_value(json!({"original": {"frame": [1, 2], "node": {}}})).unwrap();
        assert!(odd.original.is_none() && odd.extra.contains_key("original"), "kept, nothing to restore");
        let exact: Embroidery =
            serde_json::from_value(json!({"stitches": {"manual": true, "exact": [[0, 1, 51.75, null], [2, 0, null, 3.5]]}})).unwrap();
        let e = exact.stitches.unwrap().exact;
        assert_eq!(e, [Exact { subpath: 0, anchor: 1, x: Some(51.75), y: None }, Exact { subpath: 2, anchor: 0, x: None, y: Some(3.5) }]);
        for junk in [json!([[0, 1, null, null]]), json!([[0, 1, "x", 2.0]]), json!([[0, 1, 2.0]]), json!("all")] {
            let read: Embroidery = serde_json::from_value(json!({ "stitches": { "exact": junk.clone() } })).unwrap();
            let run = read.stitches.unwrap();
            assert!(run.exact.is_empty() && run.extra.get("exact") == Some(&junk), "{junk}: kept unread");
        }
        let short: Embroidery =
            serde_json::from_value(json!({"original": {"frame": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], "node": {}, "fingerprint": "beef"}})).unwrap();
        let o = short.original.unwrap();
        assert_eq!((o.fingerprint, o.extra.get("fingerprint")), (None, Some(&json!("beef"))), "a fingerprint unread is kept");
    }

    /// A run a file saved before runs were marked (no `manual`), and one a machine file opened, is a
    /// file's: sewn as its file had it. Stop After set replaces what a newer version wrote under its
    /// key in a form this version can't read.
    #[test]
    fn a_run_without_the_mark_is_a_files() {
        let old: Embroidery = serde_json::from_value(json!({"stitches": {"trim_before": true}})).unwrap();
        let run = old.stitches.unwrap();
        assert!(!run.manual && run.trims.is_empty() && run.stops.is_empty() && !run.stop_after);
        let mut odd: Embroidery = serde_json::from_value(json!({"stop_after": "after"})).unwrap();
        odd.set_stop_after(true);
        assert_eq!(serde_json::to_value(&odd).unwrap(), json!({"stop_after": true}));
        odd.set_stop_after(false);
        assert!(odd.is_empty());
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

    /// An object's part settings round-trip as the commands write them, and as a newer version
    /// might write them: its stitch kind, numbers, values of another type and keys this version
    /// doesn't know, each kept where it was.
    #[test]
    fn part_settings_round_trip_with_what_a_newer_version_wrote() {
        let ours = json!({
            "fill": {"angle": 30.0, "points": {"entry": [412.5, 168.25], "exit": [398.0, 170.5]}, "pull": [0.35, 0.35]},
            "stroke": {"length": 2.0, "stitch": "running", "split_length": 7.0, "underlay": "edgeZigzag"},
            "trim_after": true
        });
        let newer = json!({
            "areas": [{"color": "#d01818", "fill": {"stitch": "satin"}, "points": {"at": [120.0, 88.5]}}],
            "branch": true,
            "fill": {
                "motif": {"name": "wave", "scale": 1.5},
                "points": {"angle_lines": [[[0.0, 0.0], [30.0, 10.0]]], "entry": [10.0, 20.0]},
                "spacing": "auto",
                "stitch": "motif"
            },
            "stroke": {"pull": [1, 2, 3], "points": "nearest", "underlay": 5}
        });
        for written in [ours, newer, json!({"fill": "satin", "stroke": [1]})] {
            let read: Embroidery = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written, "{written}");
        }
        let read: Embroidery =
            serde_json::from_value(json!({"fill": {"motif": 1, "spacing": "auto", "stitch": "motif", "points": {"entry": [10.0, 20.0]}}})).unwrap();
        let fill = read.part(PartKind::Fill).unwrap();
        assert_eq!(
            (fill.stitch.as_deref(), fill.spacing, fill.get(PartKey::Entry)),
            (Some("motif"), None, Some(PartValue::Point(Point::new(10.0, 20.0))))
        );
        assert_eq!(fill.extra.keys().collect::<Vec<_>>(), ["motif", "spacing"]);
        // A part of another shape is kept, and no part is read.
        let odd: Embroidery = serde_json::from_value(json!({"fill": "satin"})).unwrap();
        assert_eq!((odd.part(PartKind::Fill), odd.extra.get("fill")), (None, Some(&json!("satin"))));
    }

    /// What the commands may write in any form reads in one and is written in it: a number as a
    /// float, one value for both sides as two. Nulls and parts (and points) with nothing in them
    /// read as unset and are left out.
    #[test]
    fn part_settings_are_written_in_one_form() {
        for (written, saved) in [
            (
                json!({"fill": {"angle": 30, "pull": 0.35, "points": {"entry": [1, 2]}}}),
                json!({"fill": {"angle": 30.0, "pull": [0.35, 0.35], "points": {"entry": [1.0, 2.0]}}}),
            ),
            (json!({"fill": {}, "stroke": {"points": {}}}), json!({})),
            (json!({"fill": null, "stroke": {"angle": null, "points": {"exit": null}}, "trim_after": true}), json!({"trim_after": true})),
        ] {
            let read: Embroidery = serde_json::from_value(written.clone()).unwrap();
            let once = serde_json::to_value(&read).unwrap();
            assert_eq!(once, saved, "{written}");
            let again: Embroidery = serde_json::from_value(once.clone()).unwrap();
            assert_eq!(serde_json::to_value(&again).unwrap(), once, "{written}: saved twice");
        }
        let empty: Embroidery = serde_json::from_value(json!({"fill": {}})).unwrap();
        assert!(empty.is_empty() && empty == Embroidery::default());
    }

    /// Setting a value of a part makes the part, replacing what a newer version wrote there in a
    /// form this version can't read; unsetting every value leaves nothing behind, and a value of
    /// another type than its key takes changes nothing.
    #[test]
    fn a_part_set_and_unset_leaves_nothing_behind() {
        let mut e = Embroidery::default();
        e.edit_part(PartKind::Fill, |p| {
            assert!(p.set(PartKey::Angle, Some(PartValue::Number(30.0))));
            assert!(p.set(PartKey::Entry, Some(PartValue::Point(Point::new(1.0, 2.0)))));
            assert!(!p.set(PartKey::Spacing, Some(PartValue::Id("auto".into()))), "an id is no spacing");
        });
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"fill": {"angle": 30.0, "points": {"entry": [1.0, 2.0]}}}));
        e.edit_part(PartKind::Fill, |p| {
            for key in PartKey::ALL {
                assert!(p.set(key, None), "{key:?}");
            }
        });
        assert!(e.is_empty() && e == Embroidery::default(), "{e:?}");
        // What a newer version wrote under a key, in a form this version can't read, is replaced
        // when the key is set, and kept when another is.
        let mut kept: Embroidery = serde_json::from_value(json!({"fill": {"spacing": "auto", "points": "nearest", "angle": 3.0}})).unwrap();
        kept.edit_part(PartKind::Fill, |p| p.set(PartKey::Length, Some(PartValue::Number(2.5))));
        assert_eq!(serde_json::to_value(&kept).unwrap(), json!({"fill": {"angle": 3.0, "length": 2.5, "points": "nearest", "spacing": "auto"}}));
        kept.edit_part(PartKind::Fill, |p| {
            p.set(PartKey::Spacing, Some(PartValue::Number(0.4)));
            p.set(PartKey::Exit, Some(PartValue::Point(Point::new(5.0, 6.0))));
        });
        assert_eq!(
            serde_json::to_value(&kept).unwrap(),
            json!({"fill": {"angle": 3.0, "length": 2.5, "points": {"exit": [5.0, 6.0]}, "spacing": 0.4}})
        );
        // A record of another shape, and a part of another shape, are replaced by a part set.
        for written in [json!(5), json!({"fill": "satin", "branch": true})] {
            let mut e: Embroidery = serde_json::from_value(written.clone()).unwrap();
            e.edit_part(PartKind::Fill, |p| p.set(PartKey::Stitch, Some(PartValue::Id("tatami".into()))));
            let mut want = json!({"fill": {"stitch": "tatami"}});
            if written.get("branch").is_some() {
                want["branch"] = json!(true);
            }
            assert_eq!(serde_json::to_value(&e).unwrap(), want, "{written}");
        }
        // A part edited to nothing changes nothing kept elsewhere.
        let mut unread: Embroidery = serde_json::from_value(json!("satin")).unwrap();
        unread.edit_part(PartKind::Stroke, |p| p.set(PartKey::Angle, None));
        assert_eq!(serde_json::to_value(&unread).unwrap(), json!("satin"));
    }

    /// Each part setting's name is the one the commands take, and its file key the name in
    /// snake_case.
    #[test]
    fn part_keys_are_the_names_in_snake_case() {
        for (name, file) in file_keys() {
            let snake: String = name.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect();
            assert_eq!(file, snake, "{name}");
        }
        assert_eq!(PartKey::ALL.map(PartKey::name).iter().filter_map(|n| PartKey::from_name(n)).count(), PartKey::ALL.len());
        assert_eq!(PartKind::ALL.map(|k| PartKind::from_id(k.id())), [Some(PartKind::Fill), Some(PartKind::Stroke)]);
    }

    /// The document's fabric, thread weight, joining and machine round-trip as the commands write
    /// them and as a newer version might: a preset this version doesn't know, an underlay kind it
    /// doesn't know, numbers out of any range, values of another type and keys it doesn't know,
    /// each kept where it was.
    #[test]
    fn document_settings_round_trip_with_what_a_newer_version_wrote() {
        let ours = json!({
            "fabric": {
                "color": "#f4efe6",
                "preset": {"id": "knit", "values": {
                    "needle_point": "ball", "satin": {"pull": 0.35, "spacing_percent": 100.0, "underlay": "auto"},
                    "stabilizer": "cutAway", "tatami": {"length": 4.0, "pull": 0.2, "spacing": 0.38, "underlay": "edgeTatami"},
                    "thickness": 0.6, "topping": "waterSoluble"
                }},
                "needle_point": "ball", "satin": {"pull": 0.35, "spacing_percent": 100.0, "underlay": "auto"},
                "stabilizer": "cutAway", "tatami": {"length": 3.5, "pull": 0.2, "spacing": 0.38, "underlay": "edgeTatami"},
                "thickness": 0.6, "topping": "waterSoluble"
            },
            "join_same_color": false,
            "machine": {"jump_length": 5.0, "min_stitch": 0.4, "speed": 700.0, "split_length": 7.0, "trim_length": 10.0},
            "palette": "jef",
            "thread_weight": 60.0
        });
        let newer = json!({
            "fabric": {
                "color": "white", "grain": 45,
                "preset": {"id": "cork", "values": {"tatami": {"density_curve": [1, 2]}}, "vendor": "x"},
                "satin": {"spacing_percent": "auto", "underlay": "doubleZigzag"},
                "tatami": {"length": -40.0, "pull": [0.2, 0.3]},
                "thickness": "thick"
            },
            "join_same_color": "byThread",
            "machine": {"jump_length": 500.0, "model": "pr1050x", "trim_length": "never"},
            "thread_weight": "heavy"
        });
        // A colour with signs in it is no `#rrggbb` (a hex parser takes a sign): kept as written.
        let signed: EmbroiderySetup = serde_json::from_value(json!({"fabric": {"color": "#+a+b+c"}})).unwrap();
        assert_eq!(signed.fabric.as_deref().and_then(|f| f.color), None);
        assert_eq!(serde_json::to_value(&signed).unwrap(), json!({"fabric": {"color": "#+a+b+c"}}));
        for written in [ours, newer, json!({"fabric": "knit", "machine": [5]}), json!({"fabric": {"preset": {"values": {}}}})] {
            let read: EmbroiderySetup = serde_json::from_value(written.clone()).unwrap();
            assert_eq!(serde_json::to_value(&read).unwrap(), written, "{written}");
        }
        let read: EmbroiderySetup =
            serde_json::from_value(json!({"fabric": {"preset": {"id": "cork"}, "tatami": {"length": -40.0}}, "thread_weight": "heavy"})).unwrap();
        let fabric = read.fabric.as_deref().unwrap();
        assert_eq!(fabric.preset.as_ref().map(|p| p.id.as_str()), Some("cork"));
        assert_eq!(fabric.values.tatami.as_ref().and_then(|t| t.length), Some(-40.0), "any number is read; ranges are the commands'");
        assert_eq!((read.thread_weight, read.extra.get("thread_weight")), (None, Some(&json!("heavy"))));
    }

    /// The document's settings are written in one form (numbers as floats), and what has nothing in
    /// it (an empty fabric, machine or stitch's values; nulls) reads as unset and is left out.
    #[test]
    fn document_settings_are_written_in_one_form() {
        for (written, saved) in [
            (json!({"thread_weight": 40, "machine": {"trim_length": 12}}), json!({"thread_weight": 40.0, "machine": {"trim_length": 12.0}})),
            (json!({"fabric": {"tatami": {}, "satin": {"pull": null}}, "machine": {}, "join_same_color": null}), json!({})),
            (json!({"fabric": {"preset": {"id": "woven", "values": {}}}}), json!({"fabric": {"preset": {"id": "woven"}}})),
        ] {
            let read: EmbroiderySetup = serde_json::from_value(written.clone()).unwrap();
            let once = serde_json::to_value(&read).unwrap();
            assert_eq!(once, saved, "{written}");
            let again: EmbroiderySetup = serde_json::from_value(once.clone()).unwrap();
            assert_eq!(serde_json::to_value(&again).unwrap(), once, "{written}: saved twice");
        }
        let empty: EmbroiderySetup = serde_json::from_value(json!({"fabric": {}, "machine": {"speed": null}})).unwrap();
        assert!(empty.is_empty() && empty == EmbroiderySetup::default());
    }

    /// Setting the fabric, the thread weight, the joining or the machine replaces settings this
    /// version couldn't read and a value of another type kept under the key; unsetting them leaves
    /// nothing behind.
    #[test]
    fn document_settings_set_replace_what_couldnt_be_read() {
        let mut s: EmbroiderySetup =
            serde_json::from_value(json!({"thread_weight": "heavy", "join_same_color": 1, "fabric": "knit", "machine": 5, "hoop": 1})).unwrap();
        s.set_thread_weight(Some(60.0));
        s.set_join_same_color(Some(false));
        s.set_fabric(Some(FabricSettings { color: Some([255, 255, 255]), ..FabricSettings::default() }));
        s.set_machine(Some(MachineSettings { speed: Some(800.0), ..MachineSettings::default() }));
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({"fabric": {"color": "#ffffff"}, "hoop": 1, "join_same_color": false, "machine": {"speed": 800.0}, "thread_weight": 60.0})
        );
        s.set_thread_weight(None);
        s.set_join_same_color(None);
        s.set_fabric(Some(FabricSettings::default()));
        s.set_machine(None);
        assert_eq!(serde_json::to_value(&s).unwrap(), json!({"hoop": 1}));
        let mut unread: EmbroiderySetup = serde_json::from_value(json!(5)).unwrap();
        unread.set_thread_weight(Some(30.0));
        assert_eq!(serde_json::to_value(&unread).unwrap(), json!({"thread_weight": 30.0}));
    }

    /// Each of the document's settings is saved under its name in snake_case.
    #[test]
    fn document_keys_are_the_names_in_snake_case() {
        for (name, file) in setup_file_keys() {
            let snake: String = name.chars().flat_map(|c| if c.is_ascii_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect();
            assert_eq!(file, snake, "{name}");
        }
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
