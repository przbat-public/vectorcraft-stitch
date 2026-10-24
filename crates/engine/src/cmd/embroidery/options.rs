//! The embroidery options, as the export and the embroidery commands take them, and the stitch
//! planner's parameters they give an object ([`Options::params`]).
//!
//! One vocabulary for the export's options and the settings objects carry (the project owner's
//! choice, 2026-10-22; `docs/embroidery.md` › Architecture): `stitch`, `spacing`, `length`,
//! `angle`, `pull`, `pullPercent`, `push`, `expand`, `narrowAsSatin`, `threadWeight`, `underlay`,
//! `jumpLength`, `trimLength`, `minStitch` and `splitLength`. The earlier names (`stitchStyle`, `density`,
//! `stitchLength`, `stitchAngle`, `pullCompensation`, `pullCompensationPercent`,
//! `pushCompensation`) are taken as well (the vocabulary's table, `super::vocabulary`), each as it
//! was: `stitchAngle` the planner's clockwise angle, `180 − angle`. A value
//! outside the planner's range is refused, naming the range, rather than clamped; `null` leaves an
//! option to its default.

use serde_json::{Map, Value, json};
use vectorcraft_stitch::plan::{
    DENSITY_RANGE, JUMP_RANGE, MAX_SATIN_STITCH_RANGE, MAX_TRIM, MIN_STITCH_RANGE, NARROW_RANGE, PERCENT, PULL_PERCENT_RANGE, PULL_RANGE, PUSH_RANGE,
    Params, STITCH_LENGTH_RANGE, Style, THREAD_WEIGHT, THREAD_WEIGHT_RANGE, Underlay,
};

use super::vocabulary;
use crate::Result;
use crate::cmd::bad;

/// How far a filled shape grows all round when `expand` is left out (millimetres): not at all, as
/// in Ink/Stitch, whose fills offer the same setting ("Expand", to close gaps between shapes,
/// <https://inkstitch.org/docs/stitches/fill-stitch/>). Pull compensation already stretches a
/// fill's rows; expanding is for art whose shapes are drawn apart.
pub(crate) const EXPAND_DEFAULT: f64 = 0.0;
/// The most a filled shape grows or shrinks (millimetres): more than closing gaps between shapes
/// needs, short of redrawing them.
pub(crate) const MAX_EXPAND: f64 = 2.0;

/// Degrees in a half turn: a fill's rows are lines, the same whichever of a half turn's two
/// directions they are said to run in.
const HALF_TURN: f64 = 180.0;

/// The planner's angle for a fill angle the commands take (degrees counter-clockwise on the page,
/// as every angle in VectorCraft and the project owner's choice, 2026-10-23): the planner counts
/// clockwise as seen (its y axis points down the page, as the document's does), so the same rows
/// are at `180 − angle`. The one place the two are converted, both ways (it is its own inverse). A
/// half turn rather than a plain change of sign, so that the default rows (135° here, 45° to the
/// planner, `plan::FILL_ANGLE`) keep their bits.
pub(crate) fn planner_angle(angle: f64) -> f64 {
    HALF_TURN - angle
}

/// Option value `v` given under `given` as option `name` holds it: the earlier name of `angle`,
/// `stitchAngle`, keeps its earlier meaning, the planner's clockwise angle (the project owner's
/// choice, 2026-10-24), so that scripts, agents and recorded Actions that give it sew as before;
/// `angle` is [`planner_angle`] of it. Every other value as it is given.
fn as_held(name: &str, given: &str, v: &Value) -> Value {
    match v.as_f64() {
        Some(a) if name == "angle" => json!(if given == name { a } else { planner_angle(a) }),
        _ => v.clone(),
    }
}

/// The values `stitch` takes, and the style each forces: `auto` (none forced: fills tatami, strokes
/// satin or running by their width); `tatami` and `fill` (a fill tatami, a stroke the area it
/// paints, filled); `satin`; `running`.
const STITCHES: [(&str, Option<Style>); 5] =
    [("auto", None), ("tatami", Some(Style::Fill)), ("fill", Some(Style::Fill)), ("satin", Some(Style::Satin)), ("running", Some(Style::Running))];

/// The word the `stitch` option takes for `style` (the first of [`STITCHES`] that forces it):
/// `tatami`, `satin`, `running`.
pub(crate) fn stitch_id(style: Style) -> &'static str {
    STITCHES.iter().find(|(_, s)| *s == Some(style)).map_or("auto", |(id, _)| id)
}

/// How the export options dialog edits an embroidery option, so that what it writes is never
/// refused: a choice from the values the option takes, or a number within the range the planner
/// takes, in `unit`, to `decimals` places.
#[derive(Clone, Debug, PartialEq)]
pub enum DialogField {
    Choice(Vec<&'static str>),
    Number { min: f64, max: f64, unit: &'static str, decimals: usize },
}

/// The number of degrees `angle` turns at most each way in the dialog: a full turn (the planner
/// takes any angle; a full turn either way is every direction).
const FULL_TURN: f64 = 360.0;

/// [`DialogField`] of option `name` (as `document.formats` lists it); `None` for a name that is no
/// embroidery option.
pub fn dialog_field(name: &str) -> Option<DialogField> {
    let mm = |(min, max): (f64, f64)| DialogField::Number { min, max, unit: "mm", decimals: 2 };
    Some(match name {
        "stitch" => DialogField::Choice(["auto", "tatami", "satin", "running"].to_vec()),
        "underlay" => DialogField::Choice(Underlay::ALL.iter().map(|u| u.id()).collect()),
        "spacing" => mm(DENSITY_RANGE),
        "length" => mm(STITCH_LENGTH_RANGE),
        "angle" => DialogField::Number { min: -FULL_TURN, max: FULL_TURN, unit: "°", decimals: 1 },
        "pull" => mm(PULL_RANGE),
        "pullPercent" => DialogField::Number { min: PULL_PERCENT_RANGE.0, max: PULL_PERCENT_RANGE.1, unit: "%", decimals: 1 },
        "push" => mm(PUSH_RANGE),
        "expand" => mm((-MAX_EXPAND, MAX_EXPAND)),
        "narrowAsSatin" => mm(NARROW_RANGE),
        "threadWeight" => DialogField::Number { min: THREAD_WEIGHT_RANGE.0, max: THREAD_WEIGHT_RANGE.1, unit: "wt", decimals: 0 },
        "jumpLength" => mm(JUMP_RANGE),
        "trimLength" => mm((JUMP_RANGE.0, MAX_TRIM)),
        "minStitch" => mm(MIN_STITCH_RANGE),
        "splitLength" => mm(MAX_SATIN_STITCH_RANGE),
        _ => return None,
    })
}

/// The embroidery options, as they come: each one left out keeps the style's own value. Two are
/// equal when every value is (a plan made with one is the plan made with the other). Made from
/// the commands' params ([`Options::from_json`]), so that they are checked as the commands check
/// them; and, laid over one another ([`Options::over`]), a layer of where a unit's settings come
/// from (`super::resolve`): an object's own, the document's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The style `stitch` forces; `None` for `auto`.
    pub(crate) stitch: Option<Style>,
    pub(crate) spacing: Option<f64>,
    pub(crate) length: Option<f64>,
    pub(crate) angle: Option<f64>,
    pub(crate) pull: Option<[f64; 2]>,
    pub(crate) pull_percent: Option<[f64; 2]>,
    pub(crate) push: Option<[f64; 2]>,
    pub(crate) expand: Option<f64>,
    pub(crate) narrow_as_satin: Option<f64>,
    pub(crate) thread_weight: Option<f64>,
    pub(crate) underlay: Option<Underlay>,
    pub(crate) jump_length: Option<f64>,
    pub(crate) trim_length: Option<f64>,
    pub(crate) min_stitch: Option<f64>,
    /// The longest satin stitch before it is split, millimetres.
    pub(crate) split_length: Option<f64>,
    /// A satin's share of the spacing picked by its width, percent: the document's fabric gives
    /// it (no command option does).
    pub(crate) spacing_percent: Option<f64>,
}

/// What to do with a key that is no option.
#[derive(Clone, Copy)]
pub(crate) enum Others<'a> {
    /// Refuse it, unless it is one of these (the command's own keys).
    Refuse(&'a [&'a str]),
    /// Pass it by, unless it looks like a misspelt option (the export's params, which hold every
    /// format's options): then refuse it, naming the option.
    PassUnlessNear,
}

impl Options {
    /// The options in `p`, an object of them under the names the commands take (null: none), and
    /// no other key, each checked against its range as the commands check it.
    pub fn from_json(p: &Value) -> Result<Self> {
        Self::from_params(p, "embroidery options")
    }

    /// The options in command params `p` (an object of them, or null for none), and no other key;
    /// `cmd` names the command in the errors.
    pub(crate) fn from_params(p: &Value, cmd: &str) -> Result<Self> {
        match p {
            Value::Null => Ok(Self::default()),
            Value::Object(o) => Self::from_object(o, Others::Refuse(&[]), cmd),
            _ => Err(bad(cmd, "params must be an object")),
        }
    }

    /// The options among the keys of `o`, each checked against its range; what a key that is no
    /// option does is `others`'. `cmd` names the command in the errors.
    pub(crate) fn from_object(o: &Map<String, Value>, others: Others<'_>, cmd: &str) -> Result<Self> {
        let mut out = Self::default();
        for (key, v) in o {
            let Some(option) = vocabulary::options().find(|n| n.name == key || n.earlier == Some(key.as_str())) else {
                let names: Vec<&str> = vocabulary::options().flat_map(|n| std::iter::once(n.name).chain(n.earlier)).collect();
                match others {
                    Others::Refuse(own) if own.contains(&key.as_str()) => continue,
                    Others::Refuse(_) => {
                        return Err(bad(
                            cmd,
                            match crate::cmd::params::nearest(key, &names) {
                                Some(near) => format!("unknown embroidery option `{key}`; did you mean `{near}`?"),
                                None => format!("unknown embroidery option `{key}`; the options are: {}", names.join(", ")),
                            },
                        ));
                    }
                    Others::PassUnlessNear => match crate::cmd::params::nearest(key, &names) {
                        Some(near) => return Err(bad(cmd, format!("unknown option `{key}`; did you mean the embroidery option `{near}`?"))),
                        None => continue,
                    },
                }
            };
            // The same option under both its names, with different values (as the option holds
            // them: `stitchAngle` 30 is `angle` 150).
            if let Some(other) = option.earlier.filter(|e| e != key).or(Some(option.name).filter(|n| n != key))
                && let Some(w) = o.get(other)
                && as_held(option.name, other, w) != as_held(option.name, key, v)
            {
                return Err(bad(cmd, format!("`{}` and `{}` name the same option: give one", option.name, option.earlier.unwrap_or(other))));
            }
            out.set(option.name, key, v, cmd)?;
        }
        if let (Some(trim), jump) = (out.trim_length, out.jump_length.unwrap_or(Params::for_style(Style::Fill).jump))
            && trim < jump
        {
            return Err(bad(cmd, format!("`trimLength` {trim}: at least `jumpLength` ({jump} mm), since only a jump is cut")));
        }
        Ok(out)
    }

    /// Set option `name` (its name, not its earlier one), given as `given`, to `v`, checked; null
    /// leaves it unset. The errors name it as it was given.
    fn set(&mut self, name: &str, given: &str, v: &Value, cmd: &str) -> Result<()> {
        if v.is_null() {
            return Ok(());
        }
        let number = |range: (f64, f64), unit: &str| -> Result<f64> {
            v.as_f64()
                .filter(|x| (range.0..=range.1).contains(x))
                .ok_or_else(|| bad(cmd, format!("`{given}` {v}: a number of {unit} from {} to {}", range.0, range.1)))
        };
        let sides = |range: (f64, f64)| -> Result<[f64; 2]> {
            let side = |x: &Value| x.as_f64().filter(|x| (range.0..=range.1).contains(x));
            let pair = match v {
                Value::Array(a) => match a.as_slice() {
                    [a, b] => side(a).zip(side(b)).map(|(a, b)| [a, b]),
                    _ => None,
                },
                one => side(one).map(|x| [x; 2]),
            };
            pair.ok_or_else(|| bad(cmd, format!("`{given}` {v}: a number, or two numbers (one per side), each from {} to {}", range.0, range.1)))
        };
        match name {
            "stitch" => {
                let id = v.as_str().unwrap_or_default();
                self.stitch = STITCHES.iter().find(|(s, _)| *s == id).map(|(_, style)| *style).ok_or_else(|| {
                    let ids: Vec<&str> = STITCHES.iter().map(|(s, _)| *s).collect();
                    bad(cmd, format!("`{given}` {v}: one of {}", ids.join(", ")))
                })?;
            }
            "spacing" => self.spacing = Some(number(DENSITY_RANGE, "millimetres")?),
            "length" => self.length = Some(number(STITCH_LENGTH_RANGE, "millimetres")?),
            "angle" => {
                v.as_f64().ok_or_else(|| bad(cmd, format!("`{given}` {v}: a number of degrees")))?;
                self.angle = as_held(name, given, v).as_f64();
            }
            "pull" => self.pull = Some(sides(PULL_RANGE)?),
            "pullPercent" => self.pull_percent = Some(sides(PULL_PERCENT_RANGE)?),
            "push" => self.push = Some(sides(PUSH_RANGE)?),
            "expand" => self.expand = Some(number((-MAX_EXPAND, MAX_EXPAND), "millimetres")?),
            "narrowAsSatin" => self.narrow_as_satin = Some(number(NARROW_RANGE, "millimetres")?),
            "threadWeight" => self.thread_weight = Some(number(THREAD_WEIGHT_RANGE, "wt")?),
            "underlay" => {
                self.underlay = Some(match v {
                    Value::Bool(true) => Underlay::Auto,
                    Value::Bool(false) => Underlay::None,
                    _ => v.as_str().and_then(Underlay::from_id).ok_or_else(|| {
                        let ids: Vec<&str> = Underlay::ALL.iter().map(|u| u.id()).collect();
                        bad(cmd, format!("`underlay` {v}: true, false or one of {}", ids.join(", ")))
                    })?,
                });
            }
            "jumpLength" => self.jump_length = Some(number(JUMP_RANGE, "millimetres")?),
            "trimLength" => self.trim_length = Some(number((JUMP_RANGE.0, MAX_TRIM), "millimetres")?),
            "minStitch" => self.min_stitch = Some(number(MIN_STITCH_RANGE, "millimetres")?),
            "splitLength" => self.split_length = Some(number(MAX_SATIN_STITCH_RANGE, "millimetres")?),
            _ => {}
        }
        Ok(())
    }

    /// The options given, under their names, as a reply echoes what it was planned with (left-out
    /// options are the planner's defaults, which `document.formats` lists).
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        let mut put = |k: &str, v: Option<Value>| {
            if let Some(v) = v {
                o.insert(k.to_string(), v);
            }
        };
        put("stitch", Some(json!(self.stitch.map_or("auto", stitch_id))));
        put("spacing", self.spacing.map(Value::from));
        put("length", self.length.map(Value::from));
        put("angle", self.angle.map(Value::from));
        put("pull", self.pull.map(|v| json!(v)));
        put("pullPercent", self.pull_percent.map(|v| json!(v)));
        put("push", self.push.map(|v| json!(v)));
        put("expand", self.expand.map(Value::from));
        put("narrowAsSatin", self.narrow_as_satin.map(Value::from));
        put("threadWeight", self.thread_weight.map(Value::from));
        put("underlay", self.underlay.map(|u| json!(u.id())));
        put("jumpLength", self.jump_length.map(Value::from));
        put("trimLength", self.trim_length.map(Value::from));
        put("minStitch", self.min_stitch.map(Value::from));
        put("splitLength", self.split_length.map(Value::from));
        Value::Object(o)
    }

    /// These options over `lower`: each one given here, else `lower`'s. Both are taken apart field
    /// by field, so that an option added to [`Options`] fails to compile here until it is laid over
    /// too.
    pub(crate) fn over(&self, lower: &Options) -> Options {
        let Options {
            stitch,
            spacing,
            length,
            angle,
            pull,
            pull_percent,
            push,
            expand,
            narrow_as_satin,
            thread_weight,
            underlay,
            jump_length,
            trim_length,
            min_stitch,
            split_length,
            spacing_percent,
        } = *self;
        let Options {
            stitch: stitch_below,
            spacing: spacing_below,
            length: length_below,
            angle: angle_below,
            pull: pull_below,
            pull_percent: pull_percent_below,
            push: push_below,
            expand: expand_below,
            narrow_as_satin: narrow_below,
            thread_weight: thread_weight_below,
            underlay: underlay_below,
            jump_length: jump_below,
            trim_length: trim_below,
            min_stitch: min_stitch_below,
            split_length: split_below,
            spacing_percent: spacing_percent_below,
        } = *lower;
        Options {
            stitch: stitch.or(stitch_below),
            spacing: spacing.or(spacing_below),
            length: length.or(length_below),
            angle: angle.or(angle_below),
            pull: pull.or(pull_below),
            pull_percent: pull_percent.or(pull_percent_below),
            push: push.or(push_below),
            expand: expand.or(expand_below),
            narrow_as_satin: narrow_as_satin.or(narrow_below),
            thread_weight: thread_weight.or(thread_weight_below),
            underlay: underlay.or(underlay_below),
            jump_length: jump_length.or(jump_below),
            trim_length: trim_length.or(trim_below),
            min_stitch: min_stitch.or(min_stitch_below),
            split_length: split_length.or(split_below),
            spacing_percent: spacing_percent.or(spacing_percent_below),
        }
    }

    pub(crate) fn params(&self, style: Style, width: f64) -> Params {
        // The style's defaults for the thread: its spacing follows the thread's thickness.
        let d = Params::for_thread(style, self.thread_weight.unwrap_or(THREAD_WEIGHT));
        Params {
            style,
            density: self.spacing.unwrap_or(d.density),
            // A spacing given is used as it is; left out, a satin picks it by its width.
            auto_spacing: d.auto_spacing && self.spacing.is_none(),
            stitch_length: self.length.unwrap_or(d.stitch_length),
            angle: self.angle.map(planner_angle).unwrap_or(d.angle),
            // A running line has no compensation, whatever is given.
            pull: if style == Style::Running { d.pull } else { self.pull.unwrap_or(d.pull) },
            pull_percent: if style == Style::Running { d.pull_percent } else { self.pull_percent.unwrap_or(d.pull_percent) },
            // Push compensation shortens satin columns only: a fill's rows have no column ends.
            push: if style == Style::Satin { self.push.unwrap_or(d.push) } else { d.push },
            // Only a fill has narrow parts to sew as satin.
            narrow: if style == Style::Fill { self.narrow_as_satin.unwrap_or(d.narrow) } else { d.narrow },
            underlay: self.underlay.unwrap_or(d.underlay),
            jump: self.jump_length.unwrap_or(d.jump),
            trim: self.trim_length.unwrap_or(d.trim),
            min_stitch: self.min_stitch.unwrap_or(d.min_stitch),
            max_satin: self.split_length.unwrap_or(d.max_satin),
            // A share of the spacing picked by width, in percent (100 is all of it, exactly).
            spacing_scale: self.spacing_percent.map_or(d.spacing_scale, |p| p / PERCENT),
            width,
            ..d
        }
        .sane()
    }

    /// The style `stitch` forces, `None` for `auto`.
    pub(crate) fn forced(&self) -> Option<Style> {
        self.stitch
    }

    /// How far a filled shape grows all round before it is stitched (millimetres, negative to
    /// shrink), within ±[`MAX_EXPAND`] (options read from params are; others are clamped); nothing
    /// when not finite.
    pub(crate) fn expand(&self) -> f64 {
        let e = self.expand.unwrap_or(EXPAND_DEFAULT);
        if e.is_finite() { e.clamp(-MAX_EXPAND, MAX_EXPAND) } else { EXPAND_DEFAULT }
    }
}
