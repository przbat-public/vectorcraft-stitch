//! What each value a part is planned with is, and where it comes from (`embroidery.info`): the four
//! layers in the decided order, the object, the export's option, the document (with the part of its
//! settings that gives the value) and the default. Values are reported as the commands name and
//! take them (millimetres, degrees counter-clockwise on the page, ids); a rule (Auto) is a value,
//! `"auto"`, never a source: the source says which layer left the choice to the rule.

use serde_json::{Value, json};
use vectorcraft_stitch::plan::{FILL_ANGLE, Params, Style, THREAD_WEIGHT};

use super::super::options::{EXPAND_DEFAULT, planner_angle, stitch_id};
use super::{PartLayer, Stitch, UnitSettings};

/// Where a resolved value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The object's own setting for the part.
    Object,
    /// The option given to the call (an export's, a read command's).
    Export,
    /// The document's embroidery settings, and which part of them.
    Document(Section),
    /// The built-in default.
    Default,
}

/// The part of the document's embroidery settings a value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    /// The fabric's values, by the stitch.
    Fabric,
    /// The machine's.
    Machine,
}

impl Source {
    /// Its id in replies.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Source::Object => "object",
            Source::Export => "export",
            Source::Document(_) => "document",
            Source::Default => "default",
        }
    }

    /// The part of the document's settings, for a value the document gives.
    pub(crate) fn section(self) -> Option<&'static str> {
        match self {
            Source::Document(Section::Fabric) => Some("fabric"),
            Source::Document(Section::Machine) => Some("machine"),
            Source::Object | Source::Export | Source::Default => None,
        }
    }
}

/// One value as reported, and where it comes from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Resolved {
    pub(crate) value: Value,
    pub(crate) source: Source,
}

/// The part keys a report holds, in the order replies list them.
pub(crate) const REPORTED: [&str; 11] =
    ["stitch", "spacing", "length", "angle", "pull", "pullPercent", "push", "expand", "narrowAsSatin", "underlay", "splitLength"];

/// One reported key of a part: its value and source, the same without the object's own value (what
/// unsetting it gives), and whether the planner uses it for the part's stitch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reported {
    pub(crate) name: &'static str,
    pub(crate) now: Resolved,
    pub(crate) inherited: Resolved,
    pub(crate) applies: bool,
}

/// Whether the planner uses part key `key` for a part sewn in `style` (a fill's narrow parts are
/// satin columns: a satin's keys reach them too). A key it doesn't use is still reported, as set.
pub(crate) fn applies(style: Style, key: &str) -> bool {
    match key {
        // Only a fill's rows run at an angle, have narrow parts, and grow as an area.
        "angle" | "narrowAsSatin" | "expand" => style == Style::Fill,
        // A running line has no compensation, spacing, underlay or satin to split.
        "spacing" | "pull" | "pullPercent" | "push" | "underlay" | "splitLength" => style != Style::Running,
        _ => true,
    }
}

/// The first of the four layers that has a value: the object's, the export's, the document's (from
/// `section`), else the default.
fn first(object: Option<Value>, export: Option<Value>, doc: Option<(Value, Section)>, default: Value) -> Resolved {
    match (object, export, doc) {
        (Some(value), _, _) => Resolved { value, source: Source::Object },
        (None, Some(value), _) => Resolved { value, source: Source::Export },
        (None, None, Some((value, section))) => Resolved { value, source: Source::Document(section) },
        (None, None, None) => Resolved { value: default, source: Source::Default },
    }
}

impl UnitSettings<'_> {
    /// The angle a fill's rows run at with the object's layer `object`, as the report gives it, and
    /// where it comes from: read alone (the embroidery tool's arrow asks for nothing more). An angle
    /// the object doesn't set turns with it ([`UnitSettings::above_the_document`]); the default rows
    /// (135° on the page) too.
    pub(crate) fn angle(&self, object: &PartLayer) -> Resolved {
        let turned = |a: f64| if self.turn != 0.0 && self.turn.is_finite() { a + self.turn } else { a };
        match object.values.angle {
            Some(a) => Resolved { value: json!(a), source: Source::Object },
            None => first(None, self.export.angle.map(|a| json!(turned(a))), None, json!(turned(planner_angle(FILL_ANGLE)))),
        }
    }

    /// Every reported key of a part sewn in `style` ([`Reported`]).
    pub(crate) fn report(&self, style: Style) -> Vec<Reported> {
        let now = self.values(style, &self.object);
        let inherited = self.values(style, &PartLayer::default());
        now.into_iter().zip(inherited).map(|((name, now), (_, inherited))| Reported { name, now, inherited, applies: applies(style, name) }).collect()
    }

    /// The reported keys, each named with its value, for a part sewn in `style` with the object's
    /// layer `object`, in the order of [`REPORTED`].
    fn values(&self, style: Style, object: &PartLayer) -> Vec<(&'static str, Resolved)> {
        let (o, e) = (&object.values, self.export);
        let weight = e.thread_weight.or(self.doc.every.thread_weight).unwrap_or(THREAD_WEIGHT);
        let doc = self.doc.for_style(style, weight);
        let d = Params::for_thread(style, weight);
        let num = |x: Option<f64>| x.map(Value::from);
        let sides = |x: Option<[f64; 2]>| x.map(|[a, b]| json!([a, b]));
        let fabric = |v: Option<Value>| v.map(|v| (v, Section::Fabric));
        let stitch = first(
            object.stitch.map(|s| {
                json!(match s {
                    Stitch::Auto => "auto",
                    Stitch::Style(style) => stitch_id(style),
                    Stitch::None => "none",
                })
            }),
            e.stitch.map(|s| json!(stitch_id(s))),
            None,
            json!("auto"),
        );
        // A satin's spacing given by no layer is picked by its width (the fabric's percent of it).
        let spacing_default = if d.auto_spacing { json!("auto") } else { json!(d.density) };
        let doc_spacing = match style {
            Style::Satin => doc.spacing_percent.map(|_| json!("auto")),
            Style::Fill | Style::Running => num(doc.spacing),
        };
        let angle = self.angle(object);
        vec![
            ("stitch", stitch),
            ("spacing", first(num(o.spacing), num(e.spacing), fabric(doc_spacing), spacing_default)),
            ("length", first(num(o.length), num(e.length), fabric(num(doc.length)), json!(d.stitch_length))),
            ("angle", angle),
            ("pull", first(sides(o.pull), sides(e.pull), fabric(sides(doc.pull)), json!(d.pull))),
            ("pullPercent", first(sides(o.pull_percent), sides(e.pull_percent), None, json!(d.pull_percent))),
            ("push", first(sides(o.push), sides(e.push), None, json!(d.push))),
            ("expand", first(num(o.expand), num(e.expand), None, json!(EXPAND_DEFAULT))),
            // As a fill takes it, whatever the part: only a fill has narrow parts.
            ("narrowAsSatin", first(num(o.narrow_as_satin), num(e.narrow_as_satin), None, json!(Params::for_style(Style::Fill).narrow))),
            (
                "underlay",
                first(
                    o.underlay.map(|u| json!(u.id())),
                    e.underlay.map(|u| json!(u.id())),
                    fabric(doc.underlay.map(|u| json!(u.id()))),
                    json!(d.underlay.id()),
                ),
            ),
            (
                "splitLength",
                first(num(o.split_length), num(e.split_length), doc.split_length.map(|s| (json!(s), Section::Machine)), json!(d.max_satin)),
            ),
        ]
    }
}
