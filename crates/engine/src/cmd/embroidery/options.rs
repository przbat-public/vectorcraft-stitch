//! The embroidery options, as the export and the embroidery commands take them, and the stitch
//! planner's parameters they give an object ([`Options::params`]).
//!
//! One vocabulary for the export's options and the settings objects will carry (the project owner's
//! choice, 2026-10-22; `docs/embroidery.md` › Architecture): `stitch`, `spacing`, `length`,
//! `angle`, `pull`, `pullPercent`, `push`, `expand`, `narrowAsSatin`, `threadWeight`, `underlay`,
//! `jumpLength`, `trimLength` and `minStitch`. The earlier names (`stitchStyle`, `density`,
//! `stitchLength`, `stitchAngle`, `pullCompensation`, `pullCompensationPercent`,
//! `pushCompensation`) are taken as well ([`OPTIONS`]). A value outside the planner's range is
//! refused, naming the range, rather than clamped; `null` leaves an option to its default.

use serde_json::{Map, Value, json};
use vectorcraft_stitch::plan::{
    DENSITY_RANGE, JUMP_RANGE, MAX_TRIM, MIN_STITCH_RANGE, NARROW_RANGE, PULL_PERCENT_RANGE, PULL_RANGE, PUSH_RANGE, Params, STITCH_LENGTH_RANGE,
    Style, THREAD_WEIGHT, THREAD_WEIGHT_RANGE, Underlay,
};

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

/// One option's names: the one it has, and the earlier one it is also taken under.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Name {
    pub(crate) name: &'static str,
    pub(crate) earlier: Option<&'static str>,
}

/// Every option, by its name and its earlier name, in the order `document.formats` lists them.
pub(crate) const OPTIONS: [Name; 14] = [
    Name { name: "stitch", earlier: Some("stitchStyle") },
    Name { name: "spacing", earlier: Some("density") },
    Name { name: "length", earlier: Some("stitchLength") },
    Name { name: "angle", earlier: Some("stitchAngle") },
    Name { name: "pull", earlier: Some("pullCompensation") },
    Name { name: "pullPercent", earlier: Some("pullCompensationPercent") },
    Name { name: "push", earlier: Some("pushCompensation") },
    Name { name: "expand", earlier: None },
    Name { name: "narrowAsSatin", earlier: None },
    Name { name: "threadWeight", earlier: None },
    Name { name: "underlay", earlier: None },
    Name { name: "jumpLength", earlier: None },
    Name { name: "trimLength", earlier: None },
    Name { name: "minStitch", earlier: None },
];

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

/// The embroidery options, as they come: each one left out keeps the style's own value. Two are
/// equal when every value is (a plan made with one is the plan made with the other).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Options {
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
    /// The options in command params `p` (an object of them, or null for none), and no other key;
    /// `cmd` names the command in the errors.
    #[cfg(test)]
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
            let Some(option) = OPTIONS.iter().find(|n| n.name == key || n.earlier == Some(key.as_str())) else {
                let names: Vec<&str> = OPTIONS.iter().flat_map(|n| std::iter::once(n.name).chain(n.earlier)).collect();
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
            // The same option under both its names, with different values.
            if let Some(other) = option.earlier.filter(|e| e != key).or(Some(option.name).filter(|n| n != key))
                && let Some(w) = o.get(other)
                && w != v
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
            "angle" => self.angle = Some(v.as_f64().ok_or_else(|| bad(cmd, format!("`{given}` {v}: a number of degrees")))?),
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
            _ => {}
        }
        Ok(())
    }

    /// The options given, under their names, as a reply echoes what it was planned with (left-out
    /// options are the planner's defaults, which `document.formats` lists).
    pub(crate) fn to_json(&self) -> Value {
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
        Value::Object(o)
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
            angle: self.angle.unwrap_or(d.angle),
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
