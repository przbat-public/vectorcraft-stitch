//! The settings of digitizing from an image: one row per setting, with its name in commands
//! (camelCase), its key in files (the name in snake_case, held so at compile time), the window's
//! step it belongs to, what it takes, its default and where the default comes from.
//!
//! Every size threshold is in millimetres or square millimetres; only the crop, which is about
//! the picture itself, counts the image's own pixels ("what is about the picture is in its pixels;
//! what is about the embroidery is in millimetres"). Smoothing and corners are upstream Image
//! Trace's own: how loosely an outline fits the working copy's pixels (a tolerance in its pixels,
//! so looser in millimetres on a copy of fewer pixels a millimetre) and how sharp a turn is kept
//! (an angle). A value outside a setting's range is refused,
//! naming the range, rather than clamped, as the other embroidery commands do.

use serde_json::{Map, Value, json};
use vectorcraft_stitch::thread::{Palette, PaletteThread};

use super::super::vocabulary::is_snake_case_of;
use crate::Result;
use crate::cmd::bad;

/// The command these settings are given to, as its errors name it.
pub(super) const C: &str = "embroidery.digitizeImage";

/// The window's step a setting belongs to: a change to it recomputes that step and the steps after
/// it, never those before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Step {
    /// The picture: its size and crop.
    Image,
    /// Its colours and the threads they are sewn in, and the smallest area kept.
    Colors,
    /// The shapes traced from it.
    Shapes,
}

impl Step {
    /// Its id in replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Colors => "colors",
            Self::Shapes => "shapes",
        }
    }
}

/// What a setting takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    /// A number in `unit`, from `range.0` to `range.1` (both taken); `default` when left out
    /// (`None`: the setting is unset unless given).
    Number { unit: &'static str, range: (f64, f64), default: Option<f64> },
    /// A whole number from `range.0` to `range.1`, `default` when left out.
    Count { range: (u32, u32), default: u32 },
    /// One of `ids`; the first is the default.
    Choice { ids: &'static [&'static str] },
    /// `[width, height]` in millimetres, each within [`SIZE_RANGE_MM`].
    Size,
    /// `[x0, y0, x1, y1]`, whole pixels of the image, `x0 < x1` and `y0 < y1`, inside it.
    Rect,
    /// `"corners"` (found where the image's corners agree), `"none"`, or a colour `"#rrggbb"`.
    Background,
    /// A list of choices for the image's colours ([`ColourChoice`]), at most [`MAX_COLOUR_CHOICES`].
    Choices,
}

/// One setting.
#[derive(Clone, Copy, Debug)]
pub(super) struct Setting {
    /// Its name in commands and replies.
    pub(super) name: &'static str,
    /// Its key in files: the name in snake_case.
    pub(super) key: &'static str,
    /// The window's step it belongs to.
    pub(super) step: Step,
    /// What it takes, and its default.
    pub(super) kind: Kind,
    /// Where its default comes from, or that it is an estimate.
    pub(super) source: &'static str,
}

/// The smallest and the largest size of a design (millimetres): above nothing, up to a metre. The
/// largest hoops machines take are about 400 to 500 mm a side (an estimate, from the hoop sizes
/// machine makers publish); a metre leaves room for designs sewn in several hoopings, which this
/// doesn't do, and keeps a size typed by mistake (a picture placed at its 72 ppi size, say) from
/// asking for a working copy of millions of pixels a side.
pub(super) const SIZE_RANGE_MM: (f64, f64) = (f64::MIN_POSITIVE, 1_000.0);

/// The smallest area kept (square millimetres), unless given: the project owner's choice
/// (2026-11-01, „1,0 mm² (polecam)”). A disc 1 mm across, the narrowest column a satin is sewn as
/// (Melco's design checker, `plan::SATIN_MIN_WIDTH`), is 0.79 mm²; 1.0 mm² still keeps a dot
/// 1.2 mm across and removes a JPEG's 8 × 8 pixel blocks at 10 pixels a millimetre (0.64 mm²). On
/// the real designs' artwork it keeps every region but four 0.27 mm² slivers the artwork's shapes
/// leave (measured on 2026-11-02, before this code was written).
pub(super) const MIN_AREA_MM2: f64 = 1.0;

/// The range of the smallest area kept (square millimetres): from nothing (every speck kept) to
/// 50 mm², a disc 8 mm across, enough to simplify any logo. An estimate.
pub(super) const MIN_AREA_RANGE_MM2: (f64, f64) = (0.0, 50.0);

/// How many colours the image is reduced to, unless given: Wilcom's help on auto-digitizing gives
/// five or six colours at most for most designs
/// (<https://help.wilcom.com/portal/en/kb/articles/autodigitizing-concept>), and it is upstream
/// Image Trace's own default.
pub(super) const COLORS: u32 = 6;

/// The fewest colours an image is reduced to: one colour and the background, as upstream's Color
/// mode takes at least.
pub(super) const MIN_COLORS: u32 = 2;

/// The most colours an image is reduced to: as many as the larger of the two thread palettes has
/// threads (Janome's 78); [`Settings::from_params`] holds a document's colours to its own palette's
/// count.
pub(super) const MAX_COLORS: u32 = 78;

/// How smooth the traced outlines are, 0 to 100, unless given: upstream Image Trace's default
/// Paths of 50, a fit within 0.33 pixels of the edge (smoothing is 100 less Paths).
pub(super) const SMOOTHING: f64 = 50.0;

/// How sharp a turn is kept as a corner, 0 to 100, unless given: upstream Image Trace's default
/// Corners of 75, a corner at a turn of 49° or more.
pub(super) const CORNERS: f64 = 75.0;

/// How far the colour sewn first reaches under the one sewn after it, where they meet
/// (millimetres), unless given: the project owner's choice (2026-11-01, „0,4 mm (polecam)”). One
/// fill row (0.38 mm, `plan::FILL_SPACING`), the overlap a fill's own regions get, rounded to the
/// formats' 0.1 mm; it covers the bare wedges of up to 0.35 mm the planner leaves near an edge
/// square to its rows, and lies between Embird's 0.3 mm example of an overlay
/// (<https://www.embird.net/studio/manual/0800trace_tut.htm>) and Wilcom's 0.5 mm default for the
/// pieces Flatten and Divide cut apart
/// (<https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Modifying/reshape/reshape-19.htm>).
pub(super) const OVERLAP_MM: f64 = 0.4;

/// The range of the overlap (millimetres): none to as much as a fill may grow (`expand`'s 2 mm).
pub(super) const OVERLAP_RANGE_MM: (f64, f64) = (0.0, super::super::options::MAX_EXPAND);

/// The range of smoothing and corners: upstream's sliders' 0 to 100.
pub(super) const PERCENT_RANGE: (f64, f64) = (0.0, 100.0);

/// How the image's colours are found, the first the default:
/// - `color`: at most `colors` colours that best represent the image (upstream's Color mode with
///   the Limited palette), the default, as the engine plan's colour step has it;
/// - `logo`: flat art traced with exactly its own colours, rare but distinct ones kept, at most
///   `colors` (upstream's Color mode with the Automatic palette, not its Flat Logo mode, whose
///   shapes are stacked instead of abutting: the project owner's choice, 2026-11-01);
/// - `grayscale`: `colors` greys (upstream's Grayscale mode).
pub(super) const MODES: &[&str] = &["color", "logo", "grayscale"];

/// The most choices for the image's colours one call takes: as many as a trace has colours at most
/// (upstream Image Trace's 256).
pub(super) const MAX_COLOUR_CHOICES: usize = 256;

/// What a colour of the image is sewn as ([`ColourChoice::job`]): the project owner's four jobs
/// (2026-11-01: "each colour has a job (Auto, Fill, Lines, Don't sew)").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Job {
    /// By the planner's rules: a fill as tatami, its narrow parts as satin or a running line; a thin
    /// line along its middle.
    Auto,
    /// As tatami, its narrow parts too, its thin lines filled.
    Fill,
    /// Every area of it along its middle, as one running line whatever its width (a centre line,
    /// `super::centre`), sewn after the fills it touches, the colours beside it reaching its middle
    /// under it.
    Lines,
    /// Not sewn: its objects are made, set not to be sewn, so that it can be sewn later.
    None,
}

impl Job {
    /// Every job, as the commands list them.
    pub(super) const ALL: [Self; 4] = [Self::Auto, Self::Fill, Self::Lines, Self::None];

    /// Its id in commands and replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fill => "fill",
            Self::Lines => "lines",
            Self::None => "none",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.id() == id)
    }
}

/// A person's choice for one of the image's colours, found again by colour: the thread it is sewn
/// in, another colour it is merged into, or what it is sewn as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ColourChoice {
    /// The image's colour it is for (the colours within `thread::MISMATCH_DELTA_E` of it take it).
    pub(super) color: [u8; 3],
    pub(super) thread: Option<PaletteThread>,
    /// The colour whose thread and areas it joins.
    pub(super) merge_into: Option<[u8; 3]>,
    pub(super) job: Option<Job>,
}

/// What is round the art and never sewn ([`Kind::Background`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BackgroundSetting {
    /// Transparency, or the colour the image's corners agree on.
    Corners,
    /// None: every area is sewn.
    None,
    /// This colour (its areas touching the image's edge).
    Colour([u8; 3]),
}

/// Every setting, in the window's order.
pub(super) const SETTINGS: [Setting; 12] = [
    Setting {
        name: "width",
        key: "width",
        step: Step::Image,
        kind: Kind::Number { unit: "mm", range: SIZE_RANGE_MM, default: None },
        source: "unset: the image's size as placed, as it lies on the page (with one of width and height, the other follows the picture's proportions)",
    },
    Setting {
        name: "height",
        key: "height",
        step: Step::Image,
        kind: Kind::Number { unit: "mm", range: SIZE_RANGE_MM, default: None },
        source: "unset: the image's size as placed, as it lies on the page",
    },
    Setting {
        name: "fit",
        key: "fit",
        step: Step::Image,
        kind: Kind::Size,
        source: "unset: the largest size within [width, height] millimetres on the page, the picture's proportions kept (a sewing field)",
    },
    Setting { name: "crop", key: "crop", step: Step::Image, kind: Kind::Rect, source: "unset: the whole image" },
    Setting {
        name: "background",
        key: "background",
        step: Step::Image,
        kind: Kind::Background,
        source: "the project owner's choice (2026-11-01): only what touches the image's edge, found at its corners",
    },
    Setting {
        name: "mode",
        key: "mode",
        step: Step::Colors,
        kind: Kind::Choice { ids: MODES },
        source: "the default: the colours that best represent the image, upstream Image Trace's Color mode with its Limited palette",
    },
    Setting {
        name: "colors",
        key: "colors",
        step: Step::Colors,
        kind: Kind::Count { range: (MIN_COLORS, MAX_COLORS), default: COLORS },
        source: "Wilcom's help on auto-digitizing: five or six colours at most for most designs; upstream Image Trace's default",
    },
    Setting { name: "colorChoices", key: "color_choices", step: Step::Colors, kind: Kind::Choices, source: "unset: no choices" },
    Setting {
        name: "minArea",
        key: "min_area",
        step: Step::Colors,
        kind: Kind::Number { unit: "mm²", range: MIN_AREA_RANGE_MM2, default: Some(MIN_AREA_MM2) },
        source: "the project owner's choice (2026-11-01), measured on the real designs' artwork",
    },
    Setting {
        name: "smoothing",
        key: "smoothing",
        step: Step::Shapes,
        kind: Kind::Number { unit: "", range: PERCENT_RANGE, default: Some(SMOOTHING) },
        source: "upstream Image Trace's default Paths (smoothing is 100 less Paths)",
    },
    Setting {
        name: "corners",
        key: "corners",
        step: Step::Shapes,
        kind: Kind::Number { unit: "", range: PERCENT_RANGE, default: Some(CORNERS) },
        source: "upstream Image Trace's default Corners",
    },
    Setting {
        name: "overlap",
        key: "overlap",
        step: Step::Shapes,
        kind: Kind::Number { unit: "mm", range: OVERLAP_RANGE_MM, default: Some(OVERLAP_MM) },
        source: "the project owner's choice (2026-11-01): one fill row, rounded to the formats' 0.1 mm",
    },
];

// Every file key is its setting's name in snake_case.
const _: () = {
    let mut k = 0;
    while k < SETTINGS.len() {
        assert!(is_snake_case_of(SETTINGS[k].key, SETTINGS[k].name), "a file key that isn't its name in snake_case");
        k += 1;
    }
};

/// The keys the command takes besides the settings: the image (`id`), whether it only answers, the
/// areas listed (`offset`, `limit`), whether the region map and the areas' outlines are given
/// (`regions`, `outlines`), and a point whose area is asked (`at`).
pub(super) const OTHER_KEYS: [&str; 7] = ["id", "dryRun", "offset", "limit", "regions", "outlines", "at"];

/// How large the design is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Size {
    /// The image's size as placed.
    Placed,
    /// This wide (millimetres), the height following the picture's proportions.
    Width(f64),
    /// This high (millimetres), the width following.
    Height(f64),
    /// The largest size within `[width, height]` millimetres, the proportions kept.
    Fit(f64, f64),
}

/// How the image's colours are found ([`MODES`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Color,
    Logo,
    Grayscale,
}

impl Mode {
    /// Its id in commands and replies.
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Logo => "logo",
            Self::Grayscale => "grayscale",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        [Self::Color, Self::Logo, Self::Grayscale].into_iter().find(|m| m.id() == id)
    }
}

/// The settings one run of the command digitizes with, each given or its default.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Settings {
    pub(super) size: Size,
    /// `[x0, y0, x1, y1]` in the image's own pixels; `None`: the whole image.
    pub(super) crop: Option<[u32; 4]>,
    pub(super) background: BackgroundSetting,
    pub(super) mode: Mode,
    pub(super) choices: Vec<ColourChoice>,
    pub(super) colors: u32,
    /// Square millimetres.
    pub(super) min_area: f64,
    pub(super) smoothing: f64,
    pub(super) corners: f64,
    /// Millimetres.
    pub(super) overlap: f64,
}

/// The row of setting `name`. Every name the settings read is in [`SETTINGS`] (a test holds it),
/// so the stand-in is never taken.
fn row(name: &str) -> &'static Setting {
    SETTINGS.iter().find(|s| s.name == name).unwrap_or(&SETTINGS[0])
}

/// What `setting` takes, in words, for an error.
fn wants(setting: &Setting) -> String {
    match setting.kind {
        Kind::Number { unit, range: (_, hi), .. } if setting.name == "width" || setting.name == "height" => {
            format!("a number of {unit} above 0, at most {hi}")
        }
        Kind::Number { unit, range: (lo, hi), .. } => {
            let unit = if unit.is_empty() { String::new() } else { format!(" {unit}") };
            format!("a number from {lo} to {hi}{unit}")
        }
        Kind::Count { range: (lo, hi), .. } => format!("a whole number from {lo} to {hi}"),
        Kind::Choice { ids } => format!("one of {}", ids.join(", ")),
        Kind::Size => format!("[width, height] in mm, each above 0, at most {}", SIZE_RANGE_MM.1),
        Kind::Rect => "[x0, y0, x1, y1]: whole pixels of the image, x0 < x1 and y0 < y1".into(),
        Kind::Background => "\"corners\", \"none\" or a colour \"#rrggbb\"".into(),
        Kind::Choices => format!(
            "a list of at most {MAX_COLOUR_CHOICES} choices {{color: \"#rrggbb\", thread?: {{palette, index}}, mergeInto?: \"#rrggbb\", job?: \"auto\"|\"fill\"|\"lines\"|\"none\"}}, mergeInto alone"
        ),
    }
}

/// How a colour is written, `#rrggbb`: its length, its digits' base, where each channel's digits
/// start, and how many each has.
const HEX_COLOUR_LEN: usize = 7;
const HEX_RADIX: u32 = 16;
const HEX_CHANNELS: [usize; 3] = [1, 3, 5];
const HEX_DIGITS: usize = 2;

/// The colour `s` writes as `#rrggbb`, exactly.
pub(super) fn hex_colour(s: &str) -> Option<[u8; 3]> {
    let ok = s.len() == HEX_COLOUR_LEN && s.starts_with('#') && s.bytes().skip(1).all(|b| b.is_ascii_hexdigit());
    let byte = |k: usize| s.get(k..k + HEX_DIGITS).and_then(|h| u8::from_str_radix(h, HEX_RADIX).ok());
    if !ok {
        return None;
    }
    let [r, g, b] = HEX_CHANNELS;
    Some([byte(r)?, byte(g)?, byte(b)?])
}

/// The background `o` asks for.
fn background(o: &Map<String, Value>) -> Result<BackgroundSetting> {
    let s = row("background");
    match o.get("background") {
        None | Some(Value::Null) => Ok(BackgroundSetting::Corners),
        Some(v) => match v.as_str() {
            Some("corners") => Ok(BackgroundSetting::Corners),
            Some("none") => Ok(BackgroundSetting::None),
            Some(h) => hex_colour(h).map(BackgroundSetting::Colour).ok_or_else(|| refused(s, v)),
            None => Err(refused(s, v)),
        },
    }
}

/// One of the choices `v` gives, for a document in `document`'s threads, or why it is refused: a
/// thread of another palette is no thread the design can be sewn in.
fn choice(v: &Value, s: &Setting, document: Palette) -> Result<ColourChoice> {
    let o = v.as_object().ok_or_else(|| refused(s, v))?;
    if let Some(k) = o.keys().find(|k| !["color", "thread", "mergeInto", "job"].contains(&k.as_str())) {
        return Err(bad(C, format!("`colorChoices` {v}: unknown key `{k}`; a choice takes color, thread, mergeInto and job")));
    }
    let colour = |key: &str| -> Result<Option<[u8; 3]>> {
        match o.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(c) => {
                c.as_str().and_then(hex_colour).map(Some).ok_or_else(|| bad(C, format!("`colorChoices` {v}: `{key}` is a colour \"#rrggbb\"")))
            }
        }
    };
    let color = colour("color")?.ok_or_else(|| bad(C, format!("`colorChoices` {v}: each choice names the image's `color` it is for")))?;
    let thread = match o.get("thread") {
        None | Some(Value::Null) => None,
        Some(t) => {
            let palette = t.get("palette").and_then(Value::as_str).and_then(Palette::from_id);
            let index = t.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
            let chosen = palette.zip(index).and_then(|(p, i)| PaletteThread::new(p, i));
            let chosen = chosen
                .ok_or_else(|| bad(C, format!("`colorChoices` {v}: `thread` is {{palette: \"pec\"|\"jef\", index}}, a thread of that palette")))?;
            if chosen.palette() != document {
                return Err(bad(
                    C,
                    format!(
                        "`colorChoices` {v}: the thread is of the {} palette, the document's is {}: choose one of its threads, or change the palette with embroidery.setup",
                        chosen.palette().id(),
                        document.id()
                    ),
                ));
            }
            Some(chosen)
        }
    };
    let job = match o.get("job") {
        None | Some(Value::Null) => None,
        Some(j) => Some(
            j.as_str()
                .and_then(Job::from_id)
                .ok_or_else(|| bad(C, format!("`colorChoices` {v}: `job` is \"auto\", \"fill\", \"lines\" or \"none\"")))?,
        ),
    };
    let merge_into = colour("mergeInto")?;
    if merge_into.is_some() && (thread.is_some() || job.is_some()) {
        return Err(bad(C, format!("`colorChoices` {v}: a colour merged into another takes that one's thread and job: give `mergeInto` alone")));
    }
    Ok(ColourChoice { color, thread, merge_into, job })
}

/// The choices for the image's colours `o` gives, for a document in `palette`'s threads.
fn choices(o: &Map<String, Value>, palette: Palette) -> Result<Vec<ColourChoice>> {
    let s = row("colorChoices");
    match o.get("colorChoices") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(list)) if list.len() <= MAX_COLOUR_CHOICES => list.iter().map(|v| choice(v, s, palette)).collect(),
        Some(v) => Err(refused(s, v)),
    }
}

/// The error for `v`, given for `setting`.
fn refused(setting: &Setting, v: &Value) -> crate::EngineError {
    bad(C, format!("`{}` {v}: {}", setting.name, wants(setting)))
}

/// The number `o` gives setting `name`, within its range; its default when left out or null.
fn number(o: &Map<String, Value>, name: &str) -> Result<Option<f64>> {
    let s = row(name);
    let Kind::Number { range: (lo, hi), default, .. } = s.kind else { return Ok(None) };
    match o.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|x| x.is_finite() && (lo..=hi).contains(x)).map(Some).ok_or_else(|| refused(s, v)),
    }
}

/// A size `[width, height]` in millimetres, each within [`SIZE_RANGE_MM`].
fn size_pair(o: &Map<String, Value>, name: &str) -> Result<Option<(f64, f64)>> {
    let s = row(name);
    let (lo, hi) = SIZE_RANGE_MM;
    match o.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let pair = v.as_array().filter(|a| a.len() == 2).and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
            pair.filter(|&(w, h)| [w, h].iter().all(|x| x.is_finite() && (lo..=hi).contains(x))).map(Some).ok_or_else(|| refused(s, v))
        }
    }
}

/// The crop `[x0, y0, x1, y1]` in the image's whole pixels; checked against the image itself where
/// the image is known ([`super::working`]).
fn rect(o: &Map<String, Value>, name: &str) -> Result<Option<[u32; 4]>> {
    let s = row(name);
    match o.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let whole = |x: &Value| x.as_u64().and_then(|n| u32::try_from(n).ok());
            let r = v
                .as_array()
                .filter(|a| a.len() == 4)
                .and_then(|a| Some([whole(a.first()?)?, whole(a.get(1)?)?, whole(a.get(2)?)?, whole(a.get(3)?)?]));
            r.filter(|[x0, y0, x1, y1]| x0 < x1 && y0 < y1).map(Some).ok_or_else(|| refused(s, v))
        }
    }
}

impl Settings {
    /// The settings `o` gives (the command's params: any key not a setting nor one of
    /// [`OTHER_KEYS`] is refused, naming the nearest), each checked against its row; `palette` is
    /// the document's thread palette, whose thread count holds the colours.
    pub(super) fn from_params(o: &Map<String, Value>, palette: Palette) -> Result<Self> {
        let own: Vec<&str> = SETTINGS.iter().map(|s| s.name).chain(OTHER_KEYS).collect();
        super::super::only(o, &own, C)?;
        let (width, height, fit) = (number(o, "width")?, number(o, "height")?, size_pair(o, "fit")?);
        let size = match (width, height, fit) {
            (None, None, None) => Size::Placed,
            (Some(w), None, None) => Size::Width(w),
            (None, Some(h), None) => Size::Height(h),
            (None, None, Some((w, h))) => Size::Fit(w, h),
            _ => return Err(bad(C, "give one of `width`, `height` and `fit`: the other side follows the picture's proportions")),
        };
        let mode = match o.get("mode") {
            None | Some(Value::Null) => Mode::Color,
            Some(v) => v.as_str().and_then(Mode::from_id).ok_or_else(|| refused(row("mode"), v))?,
        };
        let colors_row = row("colors");
        let Kind::Count { range: (lo, _), default } = colors_row.kind else { return Err(bad(C, "the colour count has no range")) };
        let hi = most_colors(palette);
        let colors = match o.get("colors") {
            None | Some(Value::Null) => default,
            Some(v) => v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| (lo..=hi).contains(n)).ok_or_else(|| {
                bad(C, format!("`colors` {v}: a whole number from {lo} to {hi}, the threads of the document's palette ({})", palette.id()))
            })?,
        };
        Ok(Self {
            size,
            crop: rect(o, "crop")?,
            background: background(o)?,
            mode,
            choices: choices(o, palette)?,
            colors,
            min_area: number(o, "minArea")?.unwrap_or(MIN_AREA_MM2),
            smoothing: number(o, "smoothing")?.unwrap_or(SMOOTHING),
            corners: number(o, "corners")?.unwrap_or(CORNERS),
            overlap: number(o, "overlap")?.unwrap_or(OVERLAP_MM),
        })
    }

    /// Every setting as used, by its name, defaults filled in: what a reply echoes, and what a
    /// window starts from.
    pub(super) fn echo(&self) -> Value {
        let (width, height, fit) = match self.size {
            Size::Placed => (Value::Null, Value::Null, Value::Null),
            Size::Width(w) => (json!(w), Value::Null, Value::Null),
            Size::Height(h) => (Value::Null, json!(h), Value::Null),
            Size::Fit(w, h) => (Value::Null, Value::Null, json!([w, h])),
        };
        json!({
            "width": width,
            "height": height,
            "fit": fit,
            "crop": self.crop,
            "background": match self.background {
                BackgroundSetting::Corners => json!("corners"),
                BackgroundSetting::None => json!("none"),
                BackgroundSetting::Colour(c) => json!(super::super::hex(c)),
            },
            "mode": self.mode.id(),
            "colorChoices": self.choices.iter().map(|c| json!({
                "color": super::super::hex(c.color),
                "thread": c.thread.map(|t| json!({"palette": t.palette().id(), "index": t.index()})),
                "mergeInto": c.merge_into.map(super::super::hex),
                "job": c.job.map(Job::id),
            })).collect::<Vec<_>>(),
            "colors": self.colors,
            "minArea": self.min_area,
            "smoothing": self.smoothing,
            "corners": self.corners,
            "overlap": self.overlap,
        })
    }
}

/// The settings' table as a reply lists it: each setting's name, file key, step, what it takes,
/// its default and its source.
/// The most colours a design in `palette`'s threads takes: its threads, at most [`MAX_COLORS`]
/// (Brother's 61, not PEC's 64 entries, three of which are no thread; Janome's 78).
pub(super) fn most_colors(palette: Palette) -> u32 {
    let threads = (1..=palette.threads().len()).filter(|&i| palette.is_thread(i)).count();
    MAX_COLORS.min(u32::try_from(threads).unwrap_or(MAX_COLORS))
}

/// Each setting's row as the reply gives it, the colours' range `palette`'s own ([`most_colors`]).
pub(super) fn table(palette: Palette) -> Value {
    Value::Array(
        SETTINGS
            .iter()
            .map(|s| {
                let (unit, range, default): (&str, Value, Value) = match s.kind {
                    Kind::Number { unit, range, default } => (unit, json!([range.0, range.1]), json!(default)),
                    Kind::Count { range, default } => ("", json!([range.0, most_colors(palette)]), json!(default)),
                    Kind::Choice { ids } => ("", json!(ids), json!(ids.first())),
                    Kind::Size => ("mm", json!([SIZE_RANGE_MM.0, SIZE_RANGE_MM.1]), Value::Null),
                    Kind::Rect => ("px", Value::Null, Value::Null),
                    Kind::Background => ("", json!(["corners", "none", "#rrggbb"]), json!("corners")),
                    Kind::Choices => ("", json!([0, MAX_COLOUR_CHOICES]), json!([])),
                };
                json!({"name": s.name, "key": s.key, "step": s.step.id(), "unit": unit, "range": range, "default": default, "source": s.source})
            })
            .collect(),
    )
}
