//! Document Setup's Embroidery tab (VectorCraft Stitch): the document's embroidery settings, edited
//! in the dialog and applied with its other changes on OK, one undo step ("Document Setup");
//! Cancel applies nothing. Upstream's `dialogs/document_setup.rs` reaches it through four marked
//! hooks: its tab ([`TAB`]), [`open`], [`body`] and OK's command ([`run`]).
//!
//! - **Fields** are flat keys in the dialog's fields, named by `embroidery.setup`'s paths
//!   (`embroidery.fabric` the fabric chosen, `embroidery.fabric.tatami.pull`,
//!   `embroidery.machine.jumpLength`, `embroidery.threadWeight`…; `embroidery.applyFabricTo` the
//!   bar's values), so agents set them with `ui.dialog.set` as any dialog's. What the tab opened
//!   with, the presets and the defaults are kept beside the dialog, in the app's state
//!   (`app.embroidery.setup`, [`Opened`]), never among its fields.
//! - **The fabric**: built-in values, one of the presets, or values of the document's own
//!   (Custom). Choosing a preset copies its values into the fields (the project owner's choice: it
//!   replaces values changed before in the dialog); changing a value of the built-in ones makes
//!   them the document's own. An agent's fields (`ui.dialog.set`) are taken the same way when the
//!   tab next looks ([`sync`]): a fabric chosen fills the values the agent didn't set. A value
//!   changed from its preset (the document's own copy of it, for the preset it opened with: a
//!   fabric is copied, not linked; or, without one, from its default) has the bar, strong text and
//!   ↺, which restores it; one as its preset gives it names what the preset's value rests on (the
//!   publications in its tooltip), or that it is an estimate.
//! - **The thread** (its weight, with the rows' spacing it gives; the palette machine files
//!   number), **the machine** (jumps, cuts, the shortest stitch, satin splits, the speed the time is
//!   estimated at), **the sewing order** (one thread's objects joined) and, when the document has
//!   them, the start and the name a file gave the design: a value set in the document has the bar
//!   and ↺ (unset, the default), one left to the default says so.
//! - **The bar**: while objects set the fabric's values themselves, how many for each value, and a
//!   choice to apply the fabric to them too, on OK in the same undo step (the owner's choice); amber
//!   once a value of the fabric changed here.
//! - **Room**: two columns where the window has room for the mockup's dialog (the fabric and the
//!   thread; the machine, the sewing order and the design), one column else, its two halves'
//!   fields lined up; the other tabs keep their 540 points. Either scrolls in the dialog's own way
//!   (`dialogs::modal::scrolled_rows`) where the window is too short, its buttons in view.
//! - **OK** runs one `command.batch {label: "Document Setup"}`: `document.setup` with the other
//!   tabs' fields, `embroidery.setup` with what changed here, then `embroidery.applyFabric {all,
//!   keys}` when chosen; only `document.setup` when nothing here changed.

use std::collections::BTreeMap;

use egui::{CornerRadius, Ui};
use serde_json::{Map, Value, json};
use vectorcraft_doc::embroidery::ThreadPalette;
use vectorcraft_engine::cmd::embroidery::setup::{self, DocumentSettings, Overrides, StoredFabric};
use vectorcraft_engine::cmd::embroidery::{SettingField, distinct_underlays, fabric_field, setting_field};

use crate::VectorcraftApp;
use crate::embroidery::rows::{self, Columns, Edit, Owned, OwnedField, Row};
use crate::embroidery::{fields, marks, numbers, text, vocabulary};
use crate::i18n::Lang;
use crate::state::Dialog;
use crate::theme::Tokens;

/// The tab's name among Document Setup's tabs: a plain catalogue row, as theirs.
pub(crate) const TAB: &str = "Embroidery";
/// What the tab's fields start with.
const PREFIX: &str = "embroidery.";

/// The fabric choices besides the presets: no fabric (the built-in values), and values of the
/// document's own, from no preset.
const BUILT_IN: &str = "builtIn";
const CUSTOM: &str = "custom";
/// The fabric's values the tab edits, as `embroidery.setup {fabric}` nests them (`kind.name`).
const FABRIC_VALUES: [&str; 8] =
    ["tatami.pull", "tatami.spacing", "tatami.length", "tatami.underlay", "satin.pull", "satin.spacingPercent", "satin.underlay", "stabilizer"];
/// The machine's values, as `embroidery.setup {machine}` nests them.
const MACHINE: [&str; 5] = ["machine.jumpLength", "machine.trimLength", "machine.minStitch", "machine.splitLength", "machine.speed"];
/// The document's other values the tab edits, each its default when unset.
const THREAD_WEIGHT: &str = "threadWeight";
const PALETTE: &str = "palette";
const JOIN: &str = "joinSameColor";
/// The design's start and name: shown (and unset) only when the document has them.
const START: &str = "start";
const LABEL: &str = "label";
/// The fabric values objects can set themselves, as `overrides` counts them and
/// `embroidery.applyFabric` unsets them; and the field holding those chosen in the bar (named
/// apart from the command).
const OVERRIDES: [&str; 4] = ["pull", "spacing", "length", "underlay"];
const APPLY: &str = "applyFabricTo";

/// The tab's width with two columns, points: about the mockup's dialog, 970 points (an estimate
/// from the drawing); the dialog's frame adds its margins. The window must have that much room
/// beside them ([`DIALOG_EDGES`]).
const TWO_COLUMNS: f32 = 970.0;
/// The one column's width, points: upstream's Document Setup width, its other tabs'.
const ONE_COLUMN: f32 = 540.0;
/// Between the two columns, points.
const COLUMN_GAP: f32 = 24.0;
/// What the window keeps around a dialog, points: the dialog frame's margin and its gap to the
/// window's edge, on both sides (upstream's `dialogs::MARGIN`, 22, and `EDGE_GAP`, 8).
const DIALOG_EDGES: f32 = 60.0;
/// The fabric choice's field, points: the presets' names are longer than a value's field.
const FABRIC_FIELD: f32 = 220.0;
/// The bar's padding, corners and the room between its lines, points.
const BAR_PAD: f32 = 10.0;
const BAR_RADIUS: u8 = 6;
const GAP: f32 = 8.0;
/// The size of the words under a row (a note), points.
const NOTE_TEXT: f32 = 11.0;
/// Decimals of a value in words when its field has none to say (a default's machine value).
const WORDS_DECIMALS: usize = 2;
/// Points in a millimetre: the design's start point is shown in millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Two numbers closer than this are the same value (a field's typed number and the opened one).
const SAME: f64 = 1e-9;

/// What the tab opened with, kept beside the dialog while it is open (`app.embroidery.setup`): the
/// document it is of (`uid`), its fields as they were, the values of a document that sets none, the
/// presets, and how many objects set the fabric's values.
#[derive(Clone, Debug, Default)]
pub(crate) struct Opened {
    uid: u64,
    fields: Map<String, Value>,
    defaults: Map<String, Value>,
    presets: Vec<Preset>,
    overrides: Overrides,
    /// The fabric chosen whose values the fields hold, and those values (by [`FABRIC_VALUES`]' keys)
    /// as the tab last wrote them: what an agent set since is told from them ([`sync`]).
    applied: String,
    synced: Map<String, Value>,
    /// The document's own copy of the preset its fabric was copied from (by [`FABRIC_VALUES`]' keys,
    /// as `embroidery.settings` reports it): what that preset's values are in this document, also
    /// where this version doesn't know the preset or gives it other values now.
    copy: Map<String, Value>,
    /// The document's fabric has a colour (the stitch preview's cloth): the built-in values keep it.
    colour: bool,
    /// The built-in values were chosen in the dialog since a preset was: values of the document's
    /// own made from them leave out the preset's values the tab doesn't show (thickness, topping,
    /// needle point).
    from_built_in: bool,
}

/// A fabric preset as the tab offers it: its id, its values (by [`FABRIC_VALUES`]' keys; null where
/// it publishes none) and what each rests on.
#[derive(Clone, Debug, Default)]
struct Preset {
    id: String,
    values: Map<String, Value>,
    notes: BTreeMap<String, Note>,
}

/// What a preset's value rests on: an estimate, or the families of the publications it comes from
/// (their names and addresses for the tooltip, data from `embroidery.fabrics`).
#[derive(Clone, Debug, Default)]
struct Note {
    estimate: bool,
    families: Vec<String>,
    sources: Vec<String>,
}

/// A field's name in the dialog: the tab's prefix and `key`, a fabric's value under `fabric.` as
/// `embroidery.setup` nests it (`embroidery.fabric.tatami.pull`).
fn field(key: &str) -> String {
    if FABRIC_VALUES.contains(&key) { format!("{PREFIX}fabric.{key}") } else { format!("{PREFIX}{key}") }
}

impl Opened {
    /// The tab as it opens on document `uid`'s settings `s`, the presets `fabrics`
    /// (`embroidery.fabrics`' reply) and the settings of a document that sets none (`d`).
    fn read(uid: u64, s: &DocumentSettings, fabrics: &Value, d: &DocumentSettings) -> Self {
        let mut defaults = Map::new();
        let df = &d.resolved.fabric;
        let pairs = [
            ("tatami.pull", json!(df.tatami.pull.value)),
            ("tatami.spacing", json!(df.tatami.spacing.value)),
            ("tatami.length", json!(df.tatami.length.value)),
            ("tatami.underlay", json!(df.tatami.underlay.value)),
            ("satin.pull", json!(df.satin.pull.value)),
            ("satin.spacingPercent", json!(df.satin.spacing_percent.value)),
            ("satin.underlay", json!(df.satin.underlay.value)),
            ("stabilizer", json!(df.stabilizer.value)),
            (THREAD_WEIGHT, json!(d.resolved.thread_weight.value)),
            (PALETTE, json!(d.resolved.palette.value)),
            (JOIN, json!(d.resolved.join_same_color.value)),
            ("machine.jumpLength", json!(d.resolved.machine.jump_length.value)),
            ("machine.trimLength", json!(d.resolved.machine.trim_length.value)),
            ("machine.minStitch", json!(d.resolved.machine.min_stitch.value)),
            ("machine.splitLength", json!(d.resolved.machine.split_length.value)),
            ("machine.speed", json!(d.resolved.machine.speed.value)),
        ];
        for (k, v) in pairs {
            defaults.insert(k.to_string(), v);
        }
        let mut o = Opened { uid, defaults, presets: presets(fabrics), overrides: s.overrides.clone(), ..Opened::default() };
        let stored = s.settings.fabric.as_ref();
        let choice = match stored {
            None => BUILT_IN.to_string(),
            Some(f) => f.preset.clone().unwrap_or_else(|| CUSTOM.to_string()),
        };
        for key in FABRIC_VALUES {
            let value = stored.and_then(|f| stored_value(f, key)).unwrap_or_else(|| o.base(&choice, key));
            o.fields.insert(field(key), value);
        }
        o.fields.insert(field("fabric"), json!(choice));
        o.synced = FABRIC_VALUES.iter().map(|k| (k.to_string(), o.was(k))).collect();
        o.applied = choice;
        o.copy = copy_of(s);
        o.colour = stored.is_some_and(|f| f.color.is_some());
        let r = &s.resolved;
        let values = [
            (THREAD_WEIGHT, json!(r.thread_weight.value)),
            (PALETTE, json!(r.palette.value)),
            (JOIN, json!(r.join_same_color.value)),
            ("machine.jumpLength", json!(r.machine.jump_length.value)),
            ("machine.trimLength", json!(r.machine.trim_length.value)),
            ("machine.minStitch", json!(r.machine.min_stitch.value)),
            ("machine.splitLength", json!(r.machine.split_length.value)),
            ("machine.speed", json!(r.machine.speed.value)),
            (START, json!(s.settings.start)),
            (LABEL, json!(s.settings.label)),
            (APPLY, json!([])),
        ];
        for (k, v) in values {
            o.fields.insert(field(k), v);
        }
        o
    }

    /// The preset `id`, when the tab has it.
    fn preset(&self, id: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.id == id)
    }

    /// What fabric value `key` is with fabric `choice` and nothing changed: its preset's value (the
    /// document's copy for the preset it opened with), else (no preset, or none published) its
    /// default.
    fn base(&self, choice: &str, key: &str) -> Value {
        let copied = self.copy.get(key).filter(|v| !v.is_null() && self.copied(choice));
        copied
            .or_else(|| self.preset(choice).and_then(|p| p.values.get(key)).filter(|v| !v.is_null()))
            .cloned()
            .unwrap_or_else(|| self.default_of(key))
    }

    /// Whether `choice` is the preset the document's fabric was copied from, with a copy to restore
    /// to (`embroidery.setup`'s null restores a value to it).
    fn copied(&self, choice: &str) -> bool {
        !self.copy.is_empty() && self.was("fabric").as_str() == Some(choice)
    }

    /// What choosing fabric `choice` puts in fabric value `key`'s field: its preset's value (the
    /// built-in values' default); for values of the document's own, those it opened with when it
    /// opened with them, else nothing (left as they are).
    fn values_for(&self, choice: &str, key: &str) -> Option<Value> {
        match choice {
            CUSTOM if self.was("fabric").as_str() == Some(CUSTOM) => Some(self.was(key)),
            CUSTOM => None,
            _ => Some(self.base(choice, key)),
        }
    }

    /// The fabric's choice is now `choice`, chosen as such.
    fn chose(&mut self, choice: &str) {
        self.applied = choice.to_string();
        self.from_built_in = choice == BUILT_IN;
    }

    /// The value `key` has in a document that sets none.
    fn default_of(&self, key: &str) -> Value {
        self.defaults.get(key).cloned().unwrap_or(Value::Null)
    }

    /// The value field `key` opened with.
    fn was(&self, key: &str) -> Value {
        self.fields.get(&field(key)).cloned().unwrap_or(Value::Null)
    }
}

/// The document's own copy of its fabric's preset in settings `s`, by [`FABRIC_VALUES`]' keys: the
/// values `embroidery.settings` reports as the preset's (none without a preset). The tatami spacing
/// is reported for the thread in use, and taken back to 40 wt thread, as the fields hold it.
fn copy_of(s: &DocumentSettings) -> Map<String, Value> {
    let f = &s.resolved.fabric;
    if f.preset.is_none() {
        return Map::new();
    }
    let at_40wt = |v: f64| v / setup::spacing_for_thread(1.0, s.resolved.thread_weight.value);
    let pairs = [
        ("tatami.pull", json!(f.tatami.pull.preset)),
        ("tatami.spacing", json!(f.tatami.spacing.preset.map(at_40wt))),
        ("tatami.length", json!(f.tatami.length.preset)),
        ("tatami.underlay", json!(f.tatami.underlay.preset)),
        ("satin.pull", json!(f.satin.pull.preset)),
        ("satin.spacingPercent", json!(f.satin.spacing_percent.preset)),
        ("satin.underlay", json!(f.satin.underlay.preset)),
        ("stabilizer", json!(f.stabilizer.preset)),
    ];
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Fabric value `key` (`kind.name`) as fabric `f` stores it, when set.
fn stored_value(f: &StoredFabric, key: &str) -> Option<Value> {
    match key {
        "tatami.pull" => f.tatami.pull.map(Value::from),
        "tatami.spacing" => f.tatami.spacing.map(Value::from),
        "tatami.length" => f.tatami.length.map(Value::from),
        "tatami.underlay" => f.tatami.underlay.clone().map(Value::from),
        "satin.pull" => f.satin.pull.map(Value::from),
        "satin.spacingPercent" => f.satin.spacing_percent.map(Value::from),
        "satin.underlay" => f.satin.underlay.clone().map(Value::from),
        "stabilizer" => f.stabilizer.clone().map(Value::from),
        _ => None,
    }
}

/// The presets `embroidery.fabrics` lists, with what the tab shows of each.
fn presets(fabrics: &Value) -> Vec<Preset> {
    let list = fabrics.get("fabrics").and_then(Value::as_array).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|f| {
            let id = f.get("id")?.as_str()?.to_string();
            let v = f.get("values")?;
            let mut values = Map::new();
            for key in FABRIC_VALUES {
                let value = match key.split_once('.') {
                    Some((kind, name)) => v.get(kind).and_then(|k| k.get(name)),
                    None => v.get(key),
                };
                values.insert(key.to_string(), value.cloned().unwrap_or(Value::Null));
            }
            let mut notes: BTreeMap<String, Note> = BTreeMap::new();
            for s in f.get("sources").and_then(Value::as_array).into_iter().flatten() {
                let Some(key) = s.get("key").and_then(Value::as_str) else { continue };
                let note = notes.entry(key.to_string()).or_default();
                if let Some(family) = s.get("family").and_then(Value::as_str).filter(|fam| !note.families.iter().any(|n| n == fam)) {
                    note.families.push(family.to_string());
                }
                let words = [s.get("name"), s.get("url")].into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(" — ");
                if !words.is_empty() {
                    note.sources.push(words);
                }
            }
            for key in f.get("estimates").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                notes.entry(key.to_string()).or_default().estimate = true;
            }
            Some(Preset { id, values, notes })
        })
        .collect()
}

/// Whether two values are the same: numbers within [`SAME`], anything else equal.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() <= SAME,
        _ => a == b,
    }
}

/// What the tab opened with, when Document Setup opened it for the active document (with no
/// document, none: the dialog has no tab fields).
fn opened(app: &VectorcraftApp) -> Option<&Opened> {
    let uid = app.session.active()?.uid;
    app.embroidery.setup.as_ref().filter(|o| o.uid == uid)
}

/// [`opened`], to change.
fn opened_mut(app: &mut VectorcraftApp) -> Option<&mut Opened> {
    let uid = app.session.active()?.uid;
    app.embroidery.setup.as_mut().filter(|o| o.uid == uid)
}

/// Document Setup opens: the tab's fields beside `fields` (upstream's `document.setup` reply), and
/// what it opened with kept in the app's state. With no document, nothing.
pub(crate) fn open(app: &mut VectorcraftApp, fields: &mut Value) {
    app.embroidery.setup = None;
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Ok(settings) = app.session.embroidery_settings() else { return };
    // With no presets listed (the command failing), the tab offers the built-in values alone.
    let fabrics = app.session.execute("embroidery.fabrics", &json!({})).unwrap_or(Value::Null);
    let opened = Opened::read(uid, &settings, &fabrics, &setup::default_settings());
    let Some(o) = fields.as_object_mut() else { return };
    o.extend(opened.fields.clone());
    app.embroidery.setup = Some(opened);
}

/// OK's command, run by upstream's `confirm` with `params` (the dialog's fields less its own):
/// `document.setup` with the other tabs' fields, and with it, in one undo step, what changed here
/// (`embroidery.setup`) and the fabric applied to the objects chosen in the bar
/// (`embroidery.applyFabric`); `document.setup` alone when nothing here changed, and as upstream
/// runs it without the tab (no document). Upstream's `confirm` closes the dialog when it is done.
pub(crate) fn run(app: &mut VectorcraftApp, params: Value) -> Result<Value, String> {
    let Value::Object(fields) = params else { return app.run("document.setup", params) };
    let Some(o) = opened(app) else {
        if app.embroidery.setup.is_some() && fields.keys().any(|k| k.starts_with(PREFIX)) {
            return Err("Document Setup's Embroidery tab is another document's: open Document Setup again".into());
        }
        return app.run("document.setup", Value::Object(fields));
    };
    check(&fields, o)?;
    // An agent's last fields, taken as the tab takes them (on a copy: a refused OK leaves the
    // dialog as it was).
    let (mut fields, mut o) = (fields, o.clone());
    sync(&mut fields, &mut o);
    let changes = changes(&fields, &o);
    let apply = chosen(&fields);
    let others: Map<String, Value> = fields.into_iter().filter(|(k, _)| !k.starts_with(PREFIX)).collect();
    let mut commands = vec![json!({"command": "document.setup", "params": others})];
    if !changes.is_empty() {
        commands.push(json!({"command": "embroidery.setup", "params": changes}));
    }
    if !apply.is_empty() {
        commands.push(json!({"command": "embroidery.applyFabric", "params": {"all": true, "keys": apply}}));
    }
    match commands.as_slice() {
        [only] => app.run("document.setup", only["params"].clone()),
        _ => app.run("command.batch", json!({"label": "Document Setup", "commands": commands})),
    }
}

/// Refuse what the tab can't take among the dialog's `fields` (an agent's, `ui.dialog.set`), as
/// `document.setup` refuses a setting it doesn't know: a field under the tab's prefix it doesn't
/// have (a misspelt name would be lost), a fabric it doesn't offer, a start or name other than
/// null (the tab only unsets them), a bar's list that isn't a list of its values.
fn check(fields: &Map<String, Value>, o: &Opened) -> Result<(), String> {
    let names: Vec<String> =
        ["fabric", THREAD_WEIGHT, PALETTE, JOIN, START, LABEL, APPLY].into_iter().chain(FABRIC_VALUES).chain(MACHINE).map(field).collect();
    if let Some(k) = fields.keys().find(|k| k.starts_with(PREFIX) && !names.contains(k)) {
        return Err(format!("`{}`: Document Setup's Embroidery tab has no such field", shortened(k)));
    }
    let choices = fabric_choices(o);
    match fields.get(&field("fabric")) {
        Some(Value::String(c)) if choices.contains(c) => {}
        Some(v) => return Err(format!("`{}` {}: one of {}", field("fabric"), shortened(&v.to_string()), choices.join(", "))),
        None => {}
    }
    for key in [START, LABEL] {
        if fields.get(&field(key)).is_some_and(|v| !v.is_null() && !same(v, &o.was(key))) {
            return Err(format!("`{}`: only null, which unsets it", field(key)));
        }
    }
    let list_ok = |v: &Value| v.as_array().is_some_and(|l| l.iter().all(|k| k.as_str().is_some_and(|k| OVERRIDES.contains(&k))));
    if fields.get(&field(APPLY)).is_some_and(|v| !list_ok(v)) {
        return Err(format!("`{}`: a list of {}", field(APPLY), OVERRIDES.join(", ")));
    }
    Ok(())
}

/// The most characters of a name or value an error repeats: a hostile one can be long. An
/// estimate.
const MAX_SHOWN: usize = 64;

/// `words` as an error repeats them: at most [`MAX_SHOWN`] characters, cut with "…".
fn shortened(words: &str) -> String {
    if words.chars().count() <= MAX_SHOWN { words.to_string() } else { format!("{}…", words.chars().take(MAX_SHOWN).collect::<String>()) }
}

/// The fabrics the tab offers: the built-in values, the presets, values of the document's own, and
/// the one it opened with (a preset this version doesn't know).
fn fabric_choices(o: &Opened) -> Vec<String> {
    let mut choices = vec![BUILT_IN.to_string(), CUSTOM.to_string()];
    choices.extend(o.presets.iter().map(|p| p.id.clone()));
    if let Some(c) = o.was("fabric").as_str().filter(|c| !choices.iter().any(|x| x == c)) {
        choices.push(c.to_string());
    }
    choices
}

/// The fabric values chosen in the bar to apply to the objects (`overrides`' keys), of the dialog's
/// `fields`.
fn chosen(fields: &Map<String, Value>) -> Vec<String> {
    let list = fields.get(&field(APPLY)).and_then(Value::as_array).cloned().unwrap_or_default();
    OVERRIDES.iter().filter(|k| list.iter().any(|v| v.as_str() == Some(k))).map(|k| k.to_string()).collect()
}

/// `embroidery.setup`'s params for what changed in the dialog's `fields` since it opened: each
/// value as the field holds it, or null where it went back to its default (unset).
fn changes(fields: &Map<String, Value>, o: &Opened) -> Map<String, Value> {
    let now = |key: &str| fields.get(&field(key)).cloned().unwrap_or(Value::Null);
    let mut out = Map::new();
    // The fabric. A preset chosen: the preset, less the values changed after (the tab copied its
    // values into the fields). The built-in values chosen: none. Values of the document's own
    // (Custom, or values changed on the built-in ones): all of them, whatever the fabric held. Else
    // what changed since it opened.
    let choice = now("fabric").as_str().unwrap_or(BUILT_IN).to_string();
    let chose = choice != o.was("fabric").as_str().unwrap_or(BUILT_IN);
    let unchanged = |against: &dyn Fn(&str) -> Value| FABRIC_VALUES.iter().all(|k| same(&now(k), &against(k)));
    let untouched = unchanged(&|k| o.was(k));
    let builtin = unchanged(&|k| o.default_of(k));
    let values = |against: &dyn Fn(&str) -> Option<Value>| {
        let mut fabric = Map::new();
        for key in FABRIC_VALUES {
            let value = now(key);
            if against(key).is_some_and(|a| same(&value, &a)) {
                continue;
            }
            match key.split_once('.') {
                Some((kind, name)) => {
                    if let Some(m) = fabric.entry(kind.to_string()).or_insert_with(|| json!({})).as_object_mut() {
                        m.insert(name.to_string(), value);
                    }
                }
                None => {
                    fabric.insert(key.to_string(), value);
                }
            }
        }
        fabric
    };
    // The preset's values the tab doesn't show, which go with it when the built-in values are
    // chosen (the project owner's choice): the colour, no preset's value, stays.
    let hidden = |f: &mut Map<String, Value>| {
        for key in ["topping", "needlePoint", "thickness"] {
            f.insert(key.into(), Value::Null);
        }
    };
    let fabric = match choice.as_str() {
        _ if !chose && untouched => None,
        BUILT_IN if (untouched || builtin) && !o.colour => Some(Value::Null),
        BUILT_IN if untouched || builtin => {
            let mut f: Map<String, Value> = ["preset", "tatami", "satin", "stabilizer"].into_iter().map(|k| (k.to_string(), Value::Null)).collect();
            hidden(&mut f);
            Some(Value::Object(f))
        }
        BUILT_IN | CUSTOM if chose || choice == BUILT_IN => {
            let mut f = values(&|_| None);
            f.insert("preset".into(), Value::Null);
            if o.from_built_in {
                hidden(&mut f);
            }
            Some(Value::Object(f))
        }
        _ if chose => {
            let mut f = values(&|k| Some(o.base(&choice, k)));
            f.insert("preset".into(), json!(choice));
            Some(Value::Object(f))
        }
        // What changed since it opened; a value back at the preset's (the document's copy, as ↺
        // puts it back) restored with null, as `embroidery.setup` restores it.
        _ => {
            let mut f = values(&|k| Some(o.was(k)));
            for key in FABRIC_VALUES.iter().filter(|k| o.copied(&choice) && same(&now(k), &o.base(&choice, k))) {
                let slot = match key.split_once('.') {
                    Some((kind, name)) => f.get_mut(kind).and_then(Value::as_object_mut).and_then(|m| m.get_mut(name)),
                    None => f.get_mut(*key),
                };
                if let Some(v) = slot {
                    *v = Value::Null;
                }
            }
            Some(Value::Object(f))
        }
    };
    if let Some(f) = fabric {
        out.insert("fabric".into(), f);
    }
    // The thread, the joining: unset where back at the default.
    for key in [THREAD_WEIGHT, PALETTE, JOIN] {
        let value = now(key);
        if !same(&value, &o.was(key)) {
            out.insert(key.into(), if same(&value, &o.default_of(key)) { Value::Null } else { value });
        }
    }
    let mut machine = Map::new();
    for key in MACHINE {
        let value = now(key);
        if !same(&value, &o.was(key)) {
            let name = key.trim_start_matches("machine.");
            machine.insert(name.into(), if same(&value, &o.default_of(key)) { Value::Null } else { value });
        }
    }
    if !machine.is_empty() {
        out.insert("machine".into(), Value::Object(machine));
    }
    for key in [START, LABEL] {
        if now(key).is_null() && !o.was(key).is_null() {
            out.insert(key.into(), Value::Null);
        }
    }
    out
}

/// Whether a value of the fabric (or the fabric chosen) changed since the dialog opened: the bar
/// turns amber.
fn fabric_changed(d: &Dialog, o: &Opened) -> bool {
    ["fabric"].into_iter().chain(FABRIC_VALUES).any(|k| !same(&d.fields.get(&field(k)).cloned().unwrap_or(Value::Null), &o.was(k)))
}

/// An edit of the tab: a field's new value.
type Change = (String, Value);

/// The tab's body: its rows in one or two columns, then the bar, scrolling in the dialog's own way
/// where the window is too short for them (its heading, tabs and buttons stay in view).
pub(crate) fn body(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let Some(o) = opened_mut(app) else {
        ui.label(crate::et!("No document"));
        return false;
    };
    sync(&mut d.fields, o);
    let o = &*o;
    let lang = crate::i18n::current();
    let width = width_for(ui.ctx().content_rect().width());
    let two = width >= TWO_COLUMNS;
    ui.set_min_width(width);
    ui.set_max_width(width);
    let mut edits: Vec<Change> = Vec::new();
    let (l, r) = (left_rows(d, o, lang), right_rows(d, o, lang));
    crate::dialogs::modal::scrolled_rows(ui, "embroidery-setup", |ui| {
        ui.set_max_width(width);
        if two {
            // The layout puts its item spacing on both sides of the gap too.
            let column = (width - COLUMN_GAP - 2.0 * ui.spacing().item_spacing.x) / 2.0;
            let (lc, rc) = (Columns::fit(ui, column, &l.rows(lang)), Columns::fit(ui, column, &r.rows(lang)));
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(column);
                    left(ui, d, o, &l, lc, column, lang, &mut edits);
                });
                ui.add_space(COLUMN_GAP);
                ui.vertical(|ui| {
                    ui.set_width(column);
                    right(ui, d, o, &r, rc, lang, &mut edits);
                });
            });
        } else {
            // One column: both halves' fields line up.
            let all: Vec<Row<'_>> = l.rows(lang).into_iter().chain(r.rows(lang)).collect();
            let cols = Columns::fit(ui, width, &all);
            left(ui, d, o, &l, cols, width, lang, &mut edits);
            right(ui, d, o, &r, cols, lang, &mut edits);
        }
        bar(ui, d, o, &mut edits);
    });
    if let Some(o) = opened_mut(app) {
        for (key, value) in edits {
            apply(&mut d.fields, o, &key, value);
        }
    }
    false
}

/// The tab's width in a window `window` points wide: two columns where the window has room for
/// them, else one column, as wide as Document Setup's other tabs where there is room for that.
pub(crate) fn width_for(window: f32) -> f32 {
    let room = (window - DIALOG_EDGES).max(0.0);
    if room >= TWO_COLUMNS { TWO_COLUMNS } else { ONE_COLUMN.min(room) }
}

/// Field `key` (without the prefix) of the dialog's `fields` given `value`, as the tab's rows give
/// it: a fabric chosen copies its values into all of them; a value changed on the built-in ones
/// makes them the document's own.
fn apply(fields: &mut Map<String, Value>, o: &mut Opened, key: &str, value: Value) {
    if key == "fabric" {
        if let Some(choice) = value.as_str() {
            for k in FABRIC_VALUES {
                if let Some(v) = o.values_for(choice, k) {
                    fields.insert(field(k), v);
                }
            }
            o.chose(choice);
        }
    } else if FABRIC_VALUES.contains(&key) && fields.get(&field("fabric")).and_then(Value::as_str) == Some(BUILT_IN) {
        // Made the document's own from the built-in values: still from them.
        fields.insert(field("fabric"), json!(CUSTOM));
        o.applied = CUSTOM.to_string();
    }
    fields.insert(field(key), value);
    o.synced = fabric_values(fields);
}

/// The dialog's `fields` an agent set with `ui.dialog.set` since the tab last wrote them, taken as
/// the tab's rows take them: a fabric chosen copies its values into the fields the agent left as
/// they were (those it set are its own, whichever it set first), and a value changed on the
/// built-in values makes them the document's own. So an agent's fabric and one value give what the
/// same choices in the tab give, and the tab shows the preset's values after an agent chose it.
fn sync(fields: &mut Map<String, Value>, o: &mut Opened) {
    // A fabric that isn't a choice's id is OK's to refuse.
    let Some(choice) = fields.get(&field("fabric")).and_then(Value::as_str).map(str::to_string) else { return };
    let now = |fields: &Map<String, Value>, k: &str| fields.get(&field(k)).cloned().unwrap_or(Value::Null);
    if choice != o.applied {
        for k in FABRIC_VALUES {
            let left = o.synced.get(k).is_some_and(|was| same(&now(fields, k), was));
            if let Some(v) = o.values_for(&choice, k).filter(|_| left) {
                fields.insert(field(k), v);
            }
        }
        o.chose(&choice);
    }
    if choice == BUILT_IN && FABRIC_VALUES.iter().any(|k| !same(&now(fields, k), &o.default_of(k))) {
        fields.insert(field("fabric"), json!(CUSTOM));
        o.applied = CUSTOM.to_string();
    }
    o.synced = fabric_values(fields);
}

/// The fabric's values the dialog's `fields` hold, by [`FABRIC_VALUES`]' keys.
fn fabric_values(fields: &Map<String, Value>) -> Map<String, Value> {
    FABRIC_VALUES.iter().map(|k| (k.to_string(), fields.get(&field(k)).cloned().unwrap_or(Value::Null))).collect()
}

/// The left column's rows (or the first part of one's): the fabric's choice, its values with the
/// headings over them, the thread's weight and palette.
struct Left {
    choice: String,
    choice_row: Owned,
    fabric: Vec<(Option<&'static str>, Owned)>,
    weight: Owned,
    palette: Owned,
}

impl Left {
    /// The rows to fit the columns to.
    fn rows(&self, lang: Lang) -> Vec<Row<'_>> {
        self.fabric.iter().map(|(_, r)| r).chain([&self.weight, &self.palette, &self.choice_row]).map(|r| r.row(lang)).collect()
    }
}

/// The right column's rows (or the rest of one's): the machine, and the design's start and name
/// when the document has them.
struct Right {
    machine: Vec<Owned>,
    design: Vec<Owned>,
}

impl Right {
    /// The rows to fit the columns to.
    fn rows(&self, lang: Lang) -> Vec<Row<'_>> {
        self.machine.iter().chain(&self.design).map(|r| r.row(lang)).collect()
    }
}

/// The value of field `key` in dialog `d`.
fn now(d: &Dialog, key: &str) -> Value {
    d.fields.get(&field(key)).cloned().unwrap_or(Value::Null)
}

/// The left column's rows for dialog `d`.
fn left_rows(d: &Dialog, o: &Opened, lang: Lang) -> Left {
    let choice = now(d, "fabric").as_str().unwrap_or(BUILT_IN).to_string();
    let fabric = FABRIC_VALUES
        .iter()
        .map(|key| {
            let heading = match *key {
                "tatami.pull" => Some(crate::et!("Tatami")),
                "satin.pull" => Some(crate::et!("Satin")),
                _ => None,
            };
            (heading, fabric_row(o, &choice, key, &now(d, key), lang))
        })
        .collect();
    let weight = number_row(o, THREAD_WEIGHT, &now(d, THREAD_WEIGHT), lang);
    let palette = number_row(o, PALETTE, &now(d, PALETTE), lang);
    let choice_row = fabric_choice(o, &choice);
    Left { choice, choice_row, fabric, weight, palette }
}

/// The right column's rows for dialog `d`.
fn right_rows(d: &Dialog, o: &Opened, lang: Lang) -> Right {
    let mut machine: Vec<Owned> = MACHINE.iter().map(|key| number_row(o, key, &now(d, key), lang)).collect();
    // Only a jump is cut: a cut length shorter than the jump length is refused on OK.
    let (jump, trim) = (now(d, "machine.jumpLength").as_f64(), now(d, "machine.trimLength").as_f64());
    if let (Some(jump), Some(trim)) = (jump, trim)
        && trim < jump
        && let Some(row) = machine.iter_mut().find(|r| r.key == "machine.trimLength" && r.problem.is_none())
    {
        let value = fields::format(lang, "mm", WORDS_DECIMALS).show(jump);
        row.problem = Some(text::fill(crate::et!("At least the jump length ({value}): only a jump is cut"), &[("value", &value)]));
    }
    let design = [START, LABEL].into_iter().filter(|k| !o.was(k).is_null()).map(|k| design_row(k, &now(d, k), lang)).collect();
    Right { machine, design }
}

/// The left column (or the first part of one), `width` points wide: the fabric and the thread.
#[allow(clippy::too_many_arguments)]
fn left(ui: &mut Ui, d: &Dialog, o: &Opened, l: &Left, cols: Columns, width: f32, lang: Lang, edits: &mut Vec<Change>) {
    let choice = l.choice.as_str();
    if let Some(edit) = rows::show(ui, &cols.wide(FABRIC_FIELD, width), &l.choice_row.row(lang)) {
        edits.push(edit_of("fabric", edit, o, choice));
    }
    note(ui, crate::et!("The values objects use where they set none of their own."));
    for (heading, row) in &l.fabric {
        if let Some(h) = heading {
            rows::heading(ui, h);
        }
        if let Some(edit) = rows::show(ui, &cols, &row.row(lang)) {
            edits.push(edit_of(row.key, edit, o, choice));
        }
    }
    rows::heading(ui, crate::et!("Thread"));
    if let Some(edit) = rows::show(ui, &cols, &l.weight.row(lang)) {
        edits.push(edit_of(THREAD_WEIGHT, edit, o, choice));
    }
    let spacing = now(d, "tatami.spacing").as_f64().zip(now(d, THREAD_WEIGHT).as_f64()).map(|(s, w)| setup::spacing_for_thread(s, w));
    if let Some(value) = spacing.map(|s| fields::format(lang, "mm", WORDS_DECIMALS).show(s)).filter(|v| !v.is_empty()) {
        note(ui, &text::fill(crate::et!("Rows' spacing for this thread: {value}"), &[("value", &value)]));
    }
    if let Some(edit) = rows::show(ui, &cols, &l.palette.row(lang)) {
        edits.push(edit_of(PALETTE, edit, o, choice));
    }
}

/// The right column (or the rest of one): the machine, the sewing order, and the design's start and
/// name when the document has them.
fn right(ui: &mut Ui, d: &Dialog, o: &Opened, r: &Right, cols: Columns, lang: Lang, edits: &mut Vec<Change>) {
    rows::heading(ui, crate::et!("Machine and file"));
    for row in &r.machine {
        if let Some(edit) = rows::show(ui, &cols, &row.row(lang)) {
            edits.push(edit_of(row.key, edit, o, ""));
        }
    }
    rows::heading(ui, crate::et!("Sewing order"));
    let join = now(d, JOIN).as_bool();
    let back = Tokens::get(ui.ctx()).panel;
    let check = ui.scope(|ui| rows::check(ui, crate::et!("Join one thread's objects when nothing lies between them"), join, back));
    if check.inner {
        edits.push((JOIN.into(), json!(join != Some(true))));
    }
    check.response.on_hover_text(vocabulary::setup_tip(JOIN));
    note(ui, crate::et!("Fewer thread changes and cuts: what lies above is sewn later anyway."));
    if !r.design.is_empty() {
        rows::heading(ui, crate::et!("Design"));
        for row in &r.design {
            if rows::show(ui, &cols, &row.row(lang)) == Some(Edit::Restore) {
                edits.push((row.key.into(), Value::Null));
            }
        }
    }
}

/// The change `edit` of field `key` makes: a number or a choice as given, ↺ the value back at its
/// base (the preset's or the default).
fn edit_of(key: &str, edit: Edit, o: &Opened, choice: &str) -> Change {
    let value = match edit {
        Edit::Number(v) => json!(v),
        Edit::Choice(id) => json!(id),
        Edit::Restore if FABRIC_VALUES.contains(&key) => o.base(choice, key),
        Edit::Restore => o.default_of(key),
    };
    (key.to_string(), value)
}

/// A note under a row: dim words, wrapped.
fn note(ui: &mut Ui, words: &str) {
    let t = Tokens::get(ui.ctx());
    let colour = marks::readable_text(t.text_dim, &[t.panel], t.dark);
    ui.add(egui::Label::new(egui::RichText::new(words).size(NOTE_TEXT).color(colour)).wrap());
}

/// "Restore {setting}": ↺'s name, by its field's name for screen readers ("Restore Satin: Pull
/// compensation"), so that two ↺ never read alike.
fn restore_words(label: &str) -> String {
    text::fill(crate::et!("Restore {setting}"), &[("setting", label)])
}

/// A number in words after ↺ or in a description: as its field writes it, with or without its unit.
fn number_words(key: &str, v: &Value, lang: Lang, unit: bool) -> String {
    match (field_of(key), v.as_f64()) {
        (Some(SettingField::Number { unit: name, decimals, .. }), Some(x)) => fields::format(lang, if unit { name } else { "" }, decimals).show(x),
        (_, Some(x)) => numbers::decimal(lang, x, WORDS_DECIMALS),
        (_, None) => words_of(key, v),
    }
}

/// A choice's value in words.
fn words_of(key: &str, v: &Value) -> String {
    let id = v.as_str().unwrap_or_default();
    match key {
        "tatami.underlay" | "satin.underlay" => vocabulary::underlay(id),
        "stabilizer" if v.is_null() => crate::et!("None").to_string(),
        "stabilizer" => vocabulary::information(id),
        PALETTE => vocabulary::palette(id),
        _ => id.to_string(),
    }
}

/// The field a setting is edited with: a fabric value's by its kind, the others' as the export and
/// the document take them.
fn field_of(key: &str) -> Option<SettingField> {
    match key.split_once('.') {
        Some(("machine", name)) => setting_field(name),
        Some((kind, name)) => fabric_field(kind, name),
        None => setting_field(key),
    }
}

/// The fabric's row: its choices (built-in values, the presets, the document's own when it has
/// them).
fn fabric_choice(o: &Opened, choice: &str) -> Owned {
    let mut options = vec![(BUILT_IN.to_string(), crate::et!("Built-in values").to_string())];
    options.extend(o.presets.iter().map(|p| (p.id.clone(), vocabulary::fabric(&p.id))));
    // Values of the document's own, while chosen or when the dialog opened with them (choosing them
    // again puts back the values it opened with).
    if choice == CUSTOM || o.was("fabric").as_str() == Some(CUSTOM) {
        options.push((CUSTOM.to_string(), crate::et!("Custom").to_string()));
    }
    if !options.iter().any(|(id, _)| id == choice) {
        // A preset this version doesn't know (a newer version's), kept as it is.
        options.push((choice.to_string(), vocabulary::fabric(choice)));
    }
    let shown = options.iter().find(|(id, _)| id == choice).map(|(_, w)| w.clone()).unwrap_or_default();
    let label = crate::et!("Fabric");
    Owned {
        key: "fabric",
        label,
        name: label.to_string(),
        tip: vocabulary::setup_tip("fabric"),
        field: OwnedField::Choice { shown, options, current: Some(choice.to_string()) },
        own: false,
        restore: None,
        aside: None,
        hint: None,
        description: String::new(),
        problem: None,
    }
}

/// The row of fabric value `key` (`value` in the field) with fabric `choice`: a value changed from
/// its preset (or its default) marked, ↺ restoring it; else what the preset's value rests on.
fn fabric_row(o: &Opened, choice: &str, key: &'static str, value: &Value, lang: Lang) -> Owned {
    let label = vocabulary::setup_label(key);
    let base = o.base(choice, key);
    let preset = o.preset(choice);
    let own = choice != BUILT_IN && !same(value, &base);
    let base_words = number_words(key, &base, lang, false);
    // A preset's value: one this version has, or the document's copy of one it doesn't know.
    let from_preset = preset.is_some() || o.copied(choice);
    let (aside, hint, description) = if own {
        let (hint, description) = match from_preset {
            true => (
                text::fill(crate::et!("preset {value}"), &[("value", &base_words)]),
                text::fill(crate::et!("Changed from the preset, which gives {value}"), &[("value", &number_words(key, &base, lang, true))]),
            ),
            false => (
                vocabulary::inherited("default", None, &base_words),
                text::fill(crate::et!("Set in the document; without it: {value}"), &[("value", &number_words(key, &base, lang, true))]),
            ),
        };
        (None, Some(hint), description)
    } else {
        let words = match (preset, key) {
            (_, "stabilizer") => crate::et!("information only").to_string(),
            (Some(p), _) => note_words(p.notes.get(key)),
            (None, _) if from_preset => vocabulary::source("document", Some("fabric")),
            (None, _) => vocabulary::source("default", None),
        };
        // The publications the preset's value rests on, each on a line (the badge's tooltip).
        let sources = preset.and_then(|p| p.notes.get(key)).map(|n| n.sources.join("\n")).filter(|s| !s.is_empty());
        let source = text::fill(crate::et!("Source: {name}"), &[("name", &words)]);
        let description = match sources {
            Some(s) => format!("{source}\n{s}"),
            None => source,
        };
        (Some(words), None, description)
    };
    Owned {
        key,
        label,
        name: label_name(key, label),
        tip: vocabulary::setup_tip(key),
        field: field_row(key, value, o),
        own,
        restore: own.then(|| restore_words(&label_name(key, label))),
        aside,
        hint,
        description,
        problem: out_of_range(key, value, lang),
    }
}

/// What a screen reader calls a fabric value's field: its label, with its kind of stitch where the
/// same label stands under both ("Satin: Pull compensation").
fn label_name(key: &str, label: &str) -> String {
    match key.split_once('.') {
        Some(("tatami", _)) => text::fill(crate::et!("{section}: {setting}"), &[("section", crate::et!("Tatami")), ("setting", label)]),
        Some(("satin", _)) => text::fill(crate::et!("{section}: {setting}"), &[("section", crate::et!("Satin")), ("setting", label)]),
        _ => label.to_string(),
    }
}

/// What a preset's value rests on, in a badge's words: an estimate, else its publications' families.
fn note_words(n: Option<&Note>) -> String {
    match n {
        Some(n) if n.estimate => crate::et!("estimate").to_string(),
        Some(n) if !n.families.is_empty() => n.families.iter().map(|f| family(f)).collect::<Vec<_>>().join(", "),
        _ => vocabulary::source("default", None),
    }
}

/// A publication family's name: a maker's name, the same in every language.
fn family(id: &str) -> String {
    match id {
        "wilcom" => "Wilcom".to_string(),
        "melco" => "Melco".to_string(),
        "sulky" => "Sulky".to_string(),
        "gunold" => "Gunold".to_string(),
        other => other.to_string(),
    }
}

/// The field of setting `key` holding `value`: a number in its unit and range, or a choice.
fn field_row(key: &str, value: &Value, o: &Opened) -> OwnedField {
    match field_of(key) {
        Some(SettingField::Number { min, max, unit, decimals }) => {
            OwnedField::Number { value: value.as_f64(), unit, decimals, bounds: (min, max), hint: None }
        }
        _ => {
            let mut ids: Vec<String> = match key {
                "tatami.underlay" => distinct_underlays("tatami").into_iter().map(str::to_string).collect(),
                "satin.underlay" => distinct_underlays("satin").into_iter().map(str::to_string).collect(),
                "stabilizer" => {
                    let mut ids: Vec<String> = Vec::new();
                    for p in &o.presets {
                        if let Some(id) = p.values.get(key).and_then(Value::as_str).filter(|id| !ids.iter().any(|i| i == id)) {
                            ids.push(id.to_string());
                        }
                    }
                    ids
                }
                PALETTE => ThreadPalette::KNOWN.iter().filter_map(ThreadPalette::id).map(str::to_string).collect(),
                _ => Vec::new(),
            };
            if let Some(c) = value.as_str().filter(|c| !ids.iter().any(|i| i == c)) {
                ids.push(c.to_string());
            }
            let options = ids.iter().map(|id| (id.clone(), words_of(key, &json!(id)))).collect();
            OwnedField::Choice { shown: words_of(key, value), options, current: value.as_str().map(str::to_string) }
        }
    }
}

/// The row of document value `key` (the thread's, the machine's): set in the document (not its
/// default) marked, ↺ unsetting it; else "default".
fn number_row(o: &Opened, key: &'static str, value: &Value, lang: Lang) -> Owned {
    let label = vocabulary::setup_label(key);
    let default = o.default_of(key);
    let own = !same(value, &default);
    let default_words = number_words(key, &default, lang, false);
    let (aside, hint, description) = if own {
        (
            None,
            Some(vocabulary::inherited("default", None, &default_words)),
            text::fill(crate::et!("Set in the document; without it: {value}"), &[("value", &number_words(key, &default, lang, true))]),
        )
    } else {
        let words = vocabulary::source("default", None);
        let description = text::fill(crate::et!("Source: {name}"), &[("name", &words)]);
        (Some(words), None, description)
    };
    Owned {
        key,
        label,
        name: label.to_string(),
        tip: vocabulary::setup_tip(key),
        field: field_row(key, value, o),
        own,
        restore: own.then(|| restore_words(label)),
        aside,
        hint,
        description,
        problem: out_of_range(key, value, lang),
    }
}

/// What is wrong with number `value` of setting `key` as typed: outside its range, which the
/// setting refuses on OK ("From 0 mm to 12 mm").
fn out_of_range(key: &str, value: &Value, lang: Lang) -> Option<String> {
    let Some(SettingField::Number { min, max, unit, decimals }) = field_of(key) else { return None };
    let v = value.as_f64()?;
    if (min..=max).contains(&v) {
        return None;
    }
    let f = fields::format(lang, unit, decimals);
    Some(text::fill(crate::et!("From {min} to {max}"), &[("min", &f.show(min)), ("max", &f.show(max))]))
}

/// The row of the design's start or name, read only, ↺ unsetting it (blank once unset).
fn design_row(key: &'static str, value: &Value, lang: Lang) -> Owned {
    let label = vocabulary::setup_label(key);
    let words = match (key, value) {
        (START, Value::Array(xy)) => {
            let mm = |v: Option<&Value>| v.and_then(Value::as_f64).map(|p| numbers::decimal(lang, p / PT_PER_MM, 1)).unwrap_or_default();
            text::fill(crate::et!("{x}, {y} mm"), &[("x", &mm(xy.first())), ("y", &mm(xy.get(1)))])
        }
        (_, Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let set = !value.is_null();
    Owned {
        key,
        label,
        name: label.to_string(),
        tip: vocabulary::setup_tip(key),
        field: OwnedField::Words(words),
        own: set,
        restore: set.then(|| restore_words(label)),
        aside: None,
        hint: None,
        description: String::new(),
        problem: None,
    }
}

/// The bar, while objects set the fabric's values themselves: how many for each value, and the
/// values to apply the fabric to them for too, on OK. Amber once a value of the fabric changed.
fn bar(ui: &mut Ui, d: &Dialog, o: &Opened, edits: &mut Vec<Change>) {
    let ov = &o.overrides;
    if ov.object_count == 0 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let changed = fabric_changed(d, o);
    let BarColours { fill, words, border } = BarColours::of(&t, changed);
    let chosen = chosen(&d.fields);
    ui.add_space(GAP);
    let count = crate::etn!(
        ov.object_count as u64,
        "{count} object sets its own values for the fabric.",
        "{count} objects set their own values for the fabric."
    );
    // A group for screen readers, named by its count; amber, it says the fabric changed, as its
    // mark does.
    ui.scope_builder(egui::UiBuilder::new(), |ui| {
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::Group);
            node.set_label(count.as_str());
            if changed {
                node.set_description(crate::et!("The fabric changed"));
            }
        });
        let frame = egui::Frame::NONE.fill(fill).stroke(egui::Stroke::new(1.0, border)).corner_radius(CornerRadius::same(BAR_RADIUS));
        frame.inner_margin(egui::Margin::same(BAR_PAD as i8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if changed {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(marks::MARK_SIZE, marks::MARK_SIZE), egui::Sense::hover());
                    marks::paint_on(ui.painter(), rect.center(), marks::MARK_SIZE, marks::Mark::Warning, &t, fill);
                }
                ui.add(egui::Label::new(egui::RichText::new(count).color(words)).wrap());
            });
            ui.add(egui::Label::new(egui::RichText::new(crate::et!("Changing the fabric doesn't change them.")).size(NOTE_TEXT).color(words)).wrap());
            ui.add_space(GAP / 2.0);
            ui.add(egui::Label::new(egui::RichText::new(crate::et!("Apply the fabric to them too:")).size(NOTE_TEXT).color(words)).wrap());
            for (key, n) in OVERRIDES.into_iter().zip([ov.pull, ov.spacing, ov.length, ov.underlay]) {
                if n == 0 {
                    continue;
                }
                let setting = vocabulary::override_label(key);
                let objects = crate::etn!(n as u64, "{count} object", "{count} objects");
                let words = text::fill(crate::et!("{setting}: {objects}"), &[("setting", setting), ("objects", &objects)]);
                let on = chosen.iter().any(|k| k == key);
                if rows::check(ui, &words, Some(on), fill) {
                    let mut list: Vec<String> = chosen.iter().filter(|k| k.as_str() != key).cloned().collect();
                    if !on {
                        list.push(key.to_string());
                    }
                    edits.push((APPLY.into(), json!(list)));
                }
            }
        });
    });
}

/// How strongly the bar's amber shows over the panel: a share of the warning colour. An estimate,
/// the words on it moved to read by 4.5:1 ([`marks::readable_text`]).
const BAR_TINT: f32 = 0.35;

/// The bar's colours, amber once a value of the fabric changed: its fill, its words (read by 4.5:1
/// on it), its border (the warning colour read by 3:1 on the panel; none while it isn't amber).
struct BarColours {
    fill: egui::Color32,
    words: egui::Color32,
    border: egui::Color32,
}

impl BarColours {
    fn of(t: &Tokens, changed: bool) -> Self {
        let fill = if changed { mix(t.panel, t.warning, BAR_TINT) } else { t.panel_darker };
        let words = marks::readable_text(t.text, &[fill, t.panel], t.dark);
        let border = if changed { marks::readable(t.warning, &[t.panel], t.dark) } else { egui::Color32::TRANSPARENT };
        Self { fill, words, border }
    }
}

/// `a` moved a share `t` of the way to `b`, channel by channel, opaque.
fn mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let channel = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, f32::from(u8::MAX)) as u8;
    egui::Color32::from_rgb(channel(a.r(), b.r()), channel(a.g(), b.g()), channel(a.b(), b.b()))
}

/// `embroidery.documentSetup {}`: Document Setup open at its Embroidery tab (the Object tab's
/// footer's Embroidery Settings…) → what `file.documentSetup` answers.
pub(crate) fn open_at_tab(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    crate::panels::embroidery::only_keys(p, &[])?;
    let r = app.run("file.documentSetup", json!({}))?;
    // The tab is there when Document Setup opened it (a document is open).
    if app.embroidery.setup.is_some()
        && let Some(d) = app.ui.dialog.as_mut()
    {
        d.fields.insert("tab".into(), json!(TAB));
    }
    Ok(r)
}

/// Give field `key` `value` in the open dialog's Embroidery tab, as its rows do (the pictures).
#[cfg(test)]
pub(crate) fn choose_value(app: &mut VectorcraftApp, key: &str, value: Value) {
    let uid = app.session.active().map(|st| st.uid);
    let (Some(d), Some(o)) = (app.ui.dialog.as_mut(), app.embroidery.setup.as_mut().filter(|o| Some(o.uid) == uid)) else { return };
    apply(&mut d.fields, o, key, value);
}

#[cfg(test)]
mod tests;
