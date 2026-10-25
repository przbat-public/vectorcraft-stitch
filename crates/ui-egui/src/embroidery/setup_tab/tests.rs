//! Document Setup's Embroidery tab drawn headless: its two layouts and the other tabs' width; a
//! fabric chosen (in the dialog, and by an agent's field) and a value changed, marked and restored;
//! OK one undo step with the other tabs' changes; Cancel changing nothing; agents' flat fields;
//! the bar counting objects and applying the fabric to them on OK; the bar read in every theme.

use egui::{Event, Pos2, Rect, vec2};
use serde_json::json;
use vectorcraft_engine::Session;

use super::*;
use crate::theme::{Accent, Brightness, Scheme};

/// The window's height in these tests, points.
const HEIGHT: f32 = 900.0;
/// A window wide enough for two columns, and one too narrow, points: a laptop's and a small one.
const WIDE: f32 = 1440.0;
const NARROW: f32 = 800.0;

fn app() -> VectorcraftApp {
    let mut app = VectorcraftApp::new(Session::new(), Default::default());
    app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
    app
}

/// A filled rectangle at `x` → its id.
fn rect(app: &mut VectorcraftApp, x: f64) -> u64 {
    app.run("paint.setStroke", json!({"none": true})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": x, "y": 20, "width": 40, "height": 30})).unwrap()["id"].as_u64().unwrap();
    app.run("select.set", json!({"ids": []})).unwrap();
    id
}

/// Document Setup open at its Embroidery tab.
fn open_tab(app: &mut VectorcraftApp) {
    app.run("file.documentSetup", json!({})).unwrap();
    app.ui.dialog.as_mut().unwrap().fields.insert("tab".into(), json!(TAB));
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    ctx.enable_accesskit();
    ctx
}

/// What a frame drew: its words with their boxes, and the screen readers' nodes with their ids.
struct Drawn {
    words: Vec<(String, Rect)>,
    nodes: Vec<(egui::accesskit::NodeId, egui::accesskit::Node)>,
}

impl Drawn {
    fn at(&self, what: &str) -> Rect {
        self.words.iter().find(|(w, _)| w == what).unwrap_or_else(|| panic!("no {what:?} in {:?}", self.words)).1
    }

    fn has(&self, what: &str) -> bool {
        self.words.iter().any(|(w, _)| w.contains(what))
    }

    /// The span of everything drawn, left to right.
    fn width(&self) -> f32 {
        let (l, r) = self.words.iter().fold((f32::MAX, f32::MIN), |(l, r), (_, b)| (l.min(b.left()), r.max(b.right())));
        r - l
    }

    /// A click on the node named `name`, as a screen reader asks for one.
    fn click(&self, name: &str) -> Event {
        let id = self.nodes.iter().find(|(_, n)| n.label() == Some(name)).map(|(id, _)| *id).unwrap_or_else(|| panic!("nothing is named {name:?}"));
        Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target_node: id,
            target_tree: egui::accesskit::TreeId::ROOT,
            data: None,
        })
    }
}

/// One frame of the dialog in a window `width` points wide, with `events`.
fn frame(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32, events: Vec<Event>) -> Drawn {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, HEIGHT))), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| crate::dialogs::show(app, ui.ctx()));
    out.textures_delta.clear();
    let mut words = vec![];
    let mut pending: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(s) = pending.pop() {
        match s {
            egui::Shape::Vec(v) => pending.extend(v),
            egui::Shape::Text(t) => words.push((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => {}
        }
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
    Drawn { words, nodes }
}

/// Frames until the dialog has settled (its size, the fonts) → the last.
fn ready(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Drawn {
    for _ in 0..3 {
        frame(app, ctx, width, vec![]);
    }
    frame(app, ctx, width, vec![])
}

fn dialog(app: &mut VectorcraftApp) -> &mut Dialog {
    app.ui.dialog.as_mut().unwrap()
}

/// An edit made in the tab, as its rows make them.
fn edit(app: &mut VectorcraftApp, key: &str, value: Value) {
    choose_value(app, key, value);
}

fn settings(app: &mut VectorcraftApp) -> Value {
    app.run("embroidery.settings", json!({})).unwrap()
}

fn steps(app: &VectorcraftApp) -> usize {
    app.session.active().unwrap().history.undo.len()
}

/// A preset's value `kind.name`, as `embroidery.fabrics` lists it.
fn preset_value(app: &mut VectorcraftApp, id: &str, kind: &str, name: &str) -> Value {
    let r = app.run("embroidery.fabrics", json!({})).unwrap();
    r["fabrics"].as_array().unwrap().iter().find(|f| f["id"] == json!(id)).unwrap()["values"][kind][name].clone()
}

/// Where the window has room, two columns (the machine's heading beside the fabric); else one
/// (below it); the other tabs keep their width.
#[test]
fn the_embroidery_tab_draws_in_both_layouts() {
    let ctx = context();
    let mut app = app();
    open_tab(&mut app);
    let wide = ready(&mut app, &ctx, WIDE);
    let (fabric, machine) = (wide.at("Fabric"), wide.at("Machine and file"));
    assert!(
        machine.left() > fabric.right() + COLUMN_GAP && machine.top() < fabric.bottom() + 2.0 * HEIGHT / 10.0,
        "two columns: {fabric:?} {machine:?}"
    );
    assert!(wide.width() > ONE_COLUMN, "{}", wide.width());
    let narrow = ready(&mut app, &ctx, NARROW);
    let (fabric, machine) = (narrow.at("Fabric"), narrow.at("Machine and file"));
    assert!(machine.top() > fabric.bottom() && (machine.left() - fabric.left()).abs() < COLUMN_GAP, "one column: {fabric:?} {machine:?}");
    dialog(&mut app).fields.insert("tab".into(), json!("General"));
    let general = ready(&mut app, &ctx, WIDE);
    assert!(general.width() <= ONE_COLUMN + 1.0, "the other tabs keep their width: {}", general.width());
}

/// Choosing a preset copies its values into the fields; OK keeps it as the document's fabric,
/// unchanged; an agent that sets the choice alone gets the preset as it is.
#[test]
fn choosing_a_fabric_copies_its_values() {
    let mut app = app();
    let denim = preset_value(&mut app, "denim", "tatami", "pull");
    open_tab(&mut app);
    edit(&mut app, "fabric", json!("denim"));
    assert_eq!(dialog(&mut app).fields[&field("tatami.pull")], denim);
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    assert_eq!(
        (s["settings"]["fabric"]["preset"].clone(), s["resolved"]["fabric"]["tatami"]["pull"]["changed"].clone()),
        (json!("denim"), json!(false))
    );
    assert_eq!(steps(&app), 1);
    // An agent's field.
    open_tab(&mut app);
    dialog(&mut app).fields.insert(field("fabric"), json!("fleece"));
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    assert_eq!(
        (s["settings"]["fabric"]["preset"].clone(), s["resolved"]["fabric"]["tatami"]["pull"]["changed"].clone()),
        (json!("fleece"), json!(false))
    );
}

/// A value changed from its preset is marked (↺, its words strong) and kept apart on OK; ↺
/// restores the preset's value.
#[test]
fn a_changed_value_is_marked_and_restored() {
    let ctx = context();
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
    let length = preset_value(&mut app, "cotton", "tatami", "length");
    open_tab(&mut app);
    edit(&mut app, "tatami.length", json!(3.0));
    let drawn = ready(&mut app, &ctx, WIDE);
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", vocabulary::setup_label("tatami.length"))]);
    frame(&mut app, &ctx, WIDE, vec![drawn.click(&restore)]);
    ready(&mut app, &ctx, WIDE);
    assert_eq!(dialog(&mut app).fields[&field("tatami.length")], length);
    edit(&mut app, "tatami.length", json!(3.0));
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    assert_eq!(
        (s["settings"]["fabric"]["tatami"]["length"].clone(), s["resolved"]["fabric"]["tatami"]["length"]["changed"].clone()),
        (json!(3.0), json!(true))
    );
}

/// OK with the other tabs' changes and the fabric's is one undo step, and undo takes both back;
/// changes on the built-in values make them the document's own.
#[test]
fn ok_is_one_undo_step_with_the_other_tabs() {
    let mut app = app();
    open_tab(&mut app);
    dialog(&mut app).fields.insert("units".into(), json!("Millimeters"));
    edit(&mut app, "tatami.pull", json!(0.3));
    assert_eq!(dialog(&mut app).fields[&field("fabric")], json!(CUSTOM));
    edit(&mut app, "machine.jumpLength", json!(6.0));
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none());
    let s = settings(&mut app);
    assert_eq!((s["settings"]["fabric"]["tatami"]["pull"].clone(), s["settings"]["fabric"]["preset"].clone()), (json!(0.3), json!(null)));
    assert_eq!(s["settings"]["machine"]["jumpLength"], json!(6.0));
    assert_eq!((app.session.active().unwrap().doc.units, steps(&app)), (vectorcraft_doc::Unit::Millimeters, 1));
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(
        (settings(&mut app)["settings"]["fabric"].clone(), app.session.active().unwrap().doc.units),
        (json!(null), vectorcraft_doc::Unit::Points)
    );
}

/// Cancel applies nothing; OK with nothing changed here runs Document Setup as upstream does.
#[test]
fn cancel_changes_nothing() {
    let mut app = app();
    let before = settings(&mut app);
    open_tab(&mut app);
    edit(&mut app, "fabric", json!("cap"));
    edit(&mut app, "threadWeight", json!(30.0));
    crate::dialogs::cancel(&mut app);
    assert_eq!((settings(&mut app), steps(&app)), (before.clone(), 0));
    open_tab(&mut app);
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!((settings(&mut app), steps(&app)), (before, 0));
}

/// Agents set the flat fields, named by `embroidery.setup`'s paths, with `ui.dialog.set` and
/// confirm; a value back at its default unsets it.
#[test]
fn agents_set_flat_fields() {
    let mut app = app();
    app.run("embroidery.setup", json!({"threadWeight": 30, "machine": {"speed": 900}})).unwrap();
    open_tab(&mut app);
    let speed = opened(&app).unwrap().default_of("machine.speed");
    let d = dialog(&mut app);
    d.fields.insert("embroidery.machine.trimLength".into(), json!(12.0));
    d.fields.insert("embroidery.palette".into(), json!("pec"));
    d.fields.insert("embroidery.fabric.tatami.length".into(), json!(3.0));
    d.fields.insert("embroidery.machine.speed".into(), speed);
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    let m = &s["settings"]["machine"];
    assert_eq!((m["trimLength"].clone(), m["speed"].clone(), s["settings"]["palette"].clone()), (json!(12.0), json!(null), json!("pec")));
    assert_eq!(s["settings"]["fabric"]["tatami"]["length"], json!(3.0));
    assert_eq!(s["settings"]["threadWeight"], json!(30.0), "left as it was");
}

/// While objects set the fabric's values themselves, the bar counts them by value; the values
/// chosen in it are applied to them on OK, in the same undo step; amber once the fabric changed.
#[test]
fn the_bar_counts_objects_and_applies_the_fabric_on_ok() {
    let ctx = context();
    let mut app = app();
    let (a, b) = (rect(&mut app, 20.0), rect(&mut app, 100.0));
    app.run("embroidery.set", json!({"ids": [a, b], "part": "fill", "pull": 0.3})).unwrap();
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "length": 3})).unwrap();
    let before = steps(&app);
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    assert!(drawn.has("2 objects set their own values for the fabric."), "{:?}", drawn.words);
    let pull = text::fill(crate::et!("{setting}: {objects}"), &[("setting", vocabulary::label("pull", None)), ("objects", "2 objects")]);
    assert!(drawn.has(&pull), "{pull}: {:?}", drawn.words);
    frame(&mut app, &ctx, WIDE, vec![drawn.click(&pull)]);
    ready(&mut app, &ctx, WIDE);
    assert_eq!(dialog(&mut app).fields[&field(APPLY)], json!(["pull"]));
    edit(&mut app, "fabric", json!("denim"));
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    assert_eq!((s["overrides"]["pull"].clone(), s["overrides"]["length"].clone()), (json!(0), json!(1)));
    assert_eq!(steps(&app), before + 1);
}

/// The bar's words read by 4.5:1 on it, amber or not, in every theme.
#[test]
fn the_bar_reads_in_every_theme() {
    for scheme in Scheme::ALL {
        for b in Brightness::ALL {
            for accent in Accent::ALL {
                let t = Tokens::for_scheme(scheme, b, accent);
                for fill in [mix(t.panel, t.warning, BAR_TINT), t.panel_darker] {
                    let words = marks::readable_text(t.text, &[fill, t.panel], t.dark);
                    assert!(marks::contrast(words, fill) >= marks::TEXT_CONTRAST - 0.01, "{scheme:?} {b:?} {accent:?}");
                }
            }
        }
    }
}

/// Screen readers hear each field by its label, with its kind of stitch where both kinds have it,
/// and where its value comes from; a changed value says what its preset gives, and ↺ is a button.
#[test]
fn screen_readers_hear_the_tab() {
    let ctx = context();
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
    open_tab(&mut app);
    edit(&mut app, "tatami.length", json!(3.0));
    let drawn = ready(&mut app, &ctx, WIDE);
    let node =
        |name: &str| drawn.nodes.iter().map(|(_, n)| n).find(|n| n.label() == Some(name)).unwrap_or_else(|| panic!("nothing is named {name:?}"));
    let pull = node("Satin: Pull compensation");
    assert!(pull.description().is_some_and(|d| d.starts_with("Source: ")), "{:?}", pull.description());
    let length = node("Tatami: Stitch length");
    assert!(length.description().is_some_and(|d| d.starts_with("Changed from the preset, which gives")), "{:?}", length.description());
    assert_eq!(node("Restore Stitch length").role(), egui::accesskit::Role::Button);
    let jump = node("Jump (needle up) from");
    assert_eq!(jump.description(), Some("Source: default"));
}

/// What the tab opened with stays beside the dialog: its fields are the settings alone, so an agent
/// reading them (`ui.dialog.set`'s reply) sees no presets or defaults, and can't make the tab
/// unreadable by writing to them.
#[test]
fn the_tabs_state_is_not_among_the_dialogs_fields() {
    let mut app = app();
    open_tab(&mut app);
    let fields = &dialog(&mut app).fields;
    assert!(fields.keys().all(|k| !k.starts_with("__")), "{:?}", fields.keys().collect::<Vec<_>>());
    assert!(fields.keys().filter(|k| k.starts_with(PREFIX)).all(|k| !k.contains("presets") && !k.contains("defaults")));
    assert!(opened(&app).is_some(), "kept in the app's state");
    crate::dialogs::cancel(&mut app);
    app.session.execute("file.close", &json!({})).unwrap();
    assert!(opened(&app).is_none(), "with no document, none");
}

/// OK runs upstream's own steps around the tab's command: the dialog closes once it is done. A tab
/// opened for another document than the one active is refused, and the dialog stays.
#[test]
fn ok_is_upstreams_with_the_tabs_command() {
    let mut app = app();
    open_tab(&mut app);
    edit(&mut app, "threadWeight", json!(30.0));
    app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
    let e = crate::dialogs::confirm(&mut app).unwrap_err();
    assert!(e.contains("another document"), "{e}");
    assert!(app.ui.dialog.is_some(), "the dialog stays");
    crate::dialogs::cancel(&mut app);
    open_tab(&mut app);
    edit(&mut app, "threadWeight", json!(30.0));
    crate::dialogs::confirm(&mut app).unwrap();
    assert!(app.ui.dialog.is_none(), "closed by upstream's confirm");
    assert_eq!(settings(&mut app)["settings"]["threadWeight"], json!(30.0));
}

/// An agent choosing a fabric and setting one value (`ui.dialog.set`, then OK) stores what the
/// same choices in the tab store; after an agent chose a fabric, the tab shows the preset's values;
/// an agent's value on the built-in values makes them the document's own.
#[test]
fn an_agents_fabric_and_value_are_taken_as_the_tabs() {
    let stored = |agent: bool| {
        let mut app = app();
        app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
        open_tab(&mut app);
        if agent {
            let d = dialog(&mut app);
            d.fields.insert("embroidery.fabric".into(), json!("denim"));
            d.fields.insert("embroidery.fabric.tatami.pull".into(), json!(0.3));
        } else {
            edit(&mut app, "fabric", json!("denim"));
            edit(&mut app, "tatami.pull", json!(0.3));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        settings(&mut app)["settings"]["fabric"].clone()
    };
    let ui = stored(false);
    assert_eq!(stored(true), ui);
    assert_eq!((ui["preset"].clone(), ui["tatami"]["pull"].clone()), (json!("denim"), json!(0.3)));
    // The fabric alone: the preset's values shown, none marked as changed.
    let ctx = context();
    let mut app = app();
    let denim = preset_value(&mut app, "denim", "tatami", "spacing");
    open_tab(&mut app);
    dialog(&mut app).fields.insert("embroidery.fabric".into(), json!("denim"));
    let drawn = ready(&mut app, &ctx, WIDE);
    assert_eq!(dialog(&mut app).fields[&field("tatami.spacing")], denim);
    assert!(!drawn.nodes.iter().any(|(_, n)| n.label().is_some_and(|l| l.starts_with("Restore"))), "nothing marked as changed");
    // A value on the built-in values.
    crate::dialogs::cancel(&mut app);
    open_tab(&mut app);
    dialog(&mut app).fields.insert("embroidery.fabric.tatami.length".into(), json!(3.0));
    ready(&mut app, &ctx, WIDE);
    assert_eq!(dialog(&mut app).fields[&field("fabric")], json!(CUSTOM));
}

/// OK refuses what the tab can't take among an agent's fields, with the reason, as
/// `document.setup` refuses a setting it doesn't know; the dialog stays and nothing changes.
#[test]
fn ok_refuses_fields_the_tab_cant_take() {
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": "denim"})).unwrap();
    let before = settings(&mut app);
    for (key, value, words) in [
        ("embroidery.machine.jumplength", json!(6.0), "no such field"),
        ("embroidery.tatami.pull", json!(0.3), "no such field"),
        ("embroidery.fabric", json!(5), "one of builtIn"),
        ("embroidery.fabric", json!(null), "one of builtIn"),
        ("embroidery.fabric", json!("velvet"), "one of builtIn"),
        ("embroidery.label", json!("A name"), "only null"),
        ("embroidery.start", json!([1, 2]), "only null"),
        ("embroidery.applyFabricTo", json!("pull"), "a list of pull"),
        ("embroidery.applyFabricTo", json!(["angle"]), "a list of pull"),
    ] {
        open_tab(&mut app);
        dialog(&mut app).fields.insert(key.into(), value.clone());
        let e = crate::dialogs::confirm(&mut app).unwrap_err();
        assert!(e.contains(words), "{key} {value}: {e}");
        assert!(app.ui.dialog.is_some(), "{key}: the dialog stays");
        crate::dialogs::cancel(&mut app);
        assert_eq!(settings(&mut app), before, "{key}: nothing changed");
    }
    // A long name is cut in the reason.
    open_tab(&mut app);
    dialog(&mut app).fields.insert(format!("embroidery.{}", "x".repeat(10_000)), json!(1));
    let e = crate::dialogs::confirm(&mut app).unwrap_err();
    assert!(e.chars().count() < 200, "{} characters", e.chars().count());
}

/// A document's fabric is copied from its preset, not linked: its values are marked as changed,
/// and ↺ restores them, against the document's own copy. A preset this version doesn't know (a
/// newer version's) and one whose values this version has changed (an older copy) both show their
/// copy's values unmarked; ↺ on one changed in the dialog puts the copy's value back, and OK then
/// restores it (null), the preset kept.
#[test]
fn the_marks_follow_the_documents_copy_of_its_preset() {
    /// The document's copy of cotton's tatami length and its value in force made `length`, its
    /// preset's id `id`.
    fn copied(app: &mut VectorcraftApp, id: &str, length: f64) {
        app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
        app.session
            .edit("Older copy", |d, _| {
                let f = d.embroidery.as_deref_mut().unwrap().fabric.as_deref_mut().unwrap();
                f.values.tatami.as_mut().unwrap().length = Some(length);
                let p = f.preset.as_mut().unwrap();
                p.id = id.to_string();
                p.values.tatami.as_mut().unwrap().length = Some(length);
                Ok(())
            })
            .unwrap();
    }
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", vocabulary::setup_label("tatami.length"))]);
    for id in ["velvet", "cotton"] {
        let ctx = context();
        let mut app = app();
        copied(&mut app, id, 3.3);
        open_tab(&mut app);
        let drawn = ready(&mut app, &ctx, WIDE);
        assert!(!drawn.nodes.iter().any(|(_, n)| n.label() == Some(restore.as_str())), "{id}: as its copy gives it, unmarked");
        edit(&mut app, "tatami.length", json!(4.0));
        let drawn = ready(&mut app, &ctx, WIDE);
        frame(&mut app, &ctx, WIDE, vec![drawn.click(&restore)]);
        ready(&mut app, &ctx, WIDE);
        assert_eq!(dialog(&mut app).fields[&field("tatami.length")], json!(3.3), "{id}: ↺ puts the copy's value back");
        crate::dialogs::confirm(&mut app).unwrap();
        let s = settings(&mut app);
        assert_eq!(
            (s["settings"]["fabric"]["preset"].clone(), s["settings"]["fabric"]["tatami"]["length"].clone()),
            (json!(id), json!(3.3)),
            "{id}: nothing changed"
        );
        // Changed in the document, restored in the dialog: OK restores the copy's value.
        app.run("embroidery.setup", json!({"fabric": {"tatami": {"length": 4.0}}})).unwrap();
        open_tab(&mut app);
        let drawn = ready(&mut app, &ctx, WIDE);
        frame(&mut app, &ctx, WIDE, vec![drawn.click(&restore)]);
        ready(&mut app, &ctx, WIDE);
        crate::dialogs::confirm(&mut app).unwrap();
        let s = settings(&mut app);
        let length = &s["resolved"]["fabric"]["tatami"]["length"];
        assert_eq!(
            (length["value"].clone(), length["changed"].clone(), s["settings"]["fabric"]["preset"].clone()),
            (json!(3.3), json!(false), json!(id))
        );
    }
}
