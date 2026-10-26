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

/// What a frame drew: its words with their boxes (and the boxes they are clipped to), and the
/// screen readers' nodes with their ids.
struct Drawn {
    words: Vec<(String, Rect)>,
    clips: Vec<Rect>,
    /// The fills of the boxes drawn.
    fills: Vec<egui::Color32>,
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
    frame_in(app, ctx, vec2(width, HEIGHT), events)
}

/// One frame of the dialog in a window of `size` points, with `events`.
fn frame_in(app: &mut VectorcraftApp, ctx: &egui::Context, size: egui::Vec2, events: Vec<Event>) -> Drawn {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), events, ..Default::default() };
    let mut out = ctx.run_ui(raw, |ui| crate::dialogs::show(app, ui.ctx()));
    out.textures_delta.clear();
    let (mut words, mut clips, mut fills) = (vec![], vec![], vec![]);
    let mut pending: Vec<(Rect, egui::Shape)> = out.shapes.into_iter().map(|c| (c.clip_rect, c.shape)).collect();
    while let Some((clip, s)) = pending.pop() {
        match s {
            egui::Shape::Vec(v) => pending.extend(v.into_iter().map(|s| (clip, s))),
            egui::Shape::Text(t) => {
                words.push((t.galley.text().to_string(), t.visual_bounding_rect()));
                clips.push(clip);
            }
            egui::Shape::Rect(r) => fills.push(r.fill),
            _ => {}
        }
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
    Drawn { words, clips, fills, nodes }
}

/// Frames until the dialog has settled (its size, the fonts) → the last.
fn ready(app: &mut VectorcraftApp, ctx: &egui::Context, width: f32) -> Drawn {
    for _ in 0..3 {
        frame(app, ctx, width, vec![]);
    }
    frame(app, ctx, width, vec![])
}

/// The dialog gives its first field the focus when it opens, and that field's help stays hidden
/// (it would cover the next row before anything is typed); a Tab shows the help of what it moves
/// to: the field's label (the scrub's, which Tab stops on), then the next row's field.
#[test]
fn the_first_fields_focus_shows_no_help_until_a_tab_moves_it() {
    let ctx = context();
    let mut app = app();
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    assert!(ctx.memory(|m| m.focused()).is_some(), "the dialog gives a field the focus");
    let (pull, spacing) = (vocabulary::setup_tip("tatami.pull"), vocabulary::setup_tip("tatami.spacing"));
    assert!(!drawn.has(pull), "no help shown for the focus the dialog gave");
    let tab = |app: &mut VectorcraftApp, tip: &str| {
        let key = Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let mut shown = frame(app, &ctx, WIDE, vec![key]).has(tip);
        for _ in 0..3 {
            shown |= frame(app, &ctx, WIDE, vec![]).has(tip);
        }
        shown
    };
    assert!(tab(&mut app, pull), "the help, on the field's label");
    assert!(tab(&mut app, spacing), "the next field's help");
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
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", &label_name("tatami.length", vocabulary::setup_label("tatami.length")))]);
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
    app.run("embroidery.set", json!({"ids": [b], "part": "fill", "underlay": "none", "spacing": 0.5})).unwrap();
    let before = steps(&app);
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    assert!(drawn.has("2 objects set their own values for the fabric."), "{:?}", drawn.words);
    // Each value by its own name: the underlay never as the stitch's "Type"; the spacing counts a
    // satin's too.
    for (key, objects) in [("underlay", "1 object"), ("spacing", "1 object"), ("length", "1 object")] {
        let words = text::fill(crate::et!("{setting}: {objects}"), &[("setting", vocabulary::override_label(key)), ("objects", objects)]);
        assert!(drawn.words.iter().any(|(w, _)| *w == words), "{words}: {:?}", drawn.words);
    }
    assert_eq!((vocabulary::override_label("underlay"), vocabulary::override_label("spacing")), ("Underlay", "Spacing"));
    let pull = text::fill(crate::et!("{setting}: {objects}"), &[("setting", vocabulary::label("pull", None)), ("objects", "2 objects")]);
    assert!(drawn.has(&pull), "{pull}: {:?}", drawn.words);
    frame(&mut app, &ctx, WIDE, vec![drawn.click(&pull)]);
    ready(&mut app, &ctx, WIDE);
    assert_eq!(dialog(&mut app).fields[&field(APPLY)], json!(["pull"]));
    edit(&mut app, "fabric", json!("denim"));
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app);
    assert_eq!(
        (s["overrides"]["pull"].clone(), s["overrides"]["length"].clone(), s["overrides"]["underlay"].clone()),
        (json!(0), json!(1), json!(1))
    );
    assert_eq!(steps(&app), before + 1);
}

/// The bar, amber or not, reads in every theme: its words by 4.5:1 on it, its amber border by 3:1
/// on the panel, and its checkboxes' words by 4.5:1, their boxes' borders and a ticked box by 3:1,
/// on the bar.
#[test]
fn the_bar_reads_in_every_theme() {
    for scheme in Scheme::ALL {
        for b in Brightness::ALL {
            for accent in Accent::ALL {
                let t = Tokens::for_scheme(scheme, b, accent);
                for changed in [true, false] {
                    let at = format!("{scheme:?} {b:?} {accent:?} changed {changed}");
                    let bar = BarColours::of(&t, changed);
                    assert!(marks::contrast(bar.words, bar.fill) >= marks::TEXT_CONTRAST - 0.01, "{at}: words");
                    if changed {
                        assert!(marks::contrast(bar.border, t.panel) >= marks::NON_TEXT_CONTRAST - 0.01, "{at}: border");
                    }
                    let check = rows::CheckColours::on(&t, bar.fill);
                    assert!(marks::contrast(check.words, bar.fill) >= marks::TEXT_CONTRAST - 0.01, "{at}: a box's words");
                    assert!(marks::contrast(check.border, bar.fill) >= marks::NON_TEXT_CONTRAST - 0.01, "{at}: a box's border");
                    assert!(marks::contrast(check.ticked, bar.fill) >= marks::NON_TEXT_CONTRAST - 0.01, "{at}: a ticked box");
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
    assert_eq!(node("Restore Tatami: Stitch length").role(), egui::accesskit::Role::Button);
    let jump = node("Jump (needle up) from");
    assert_eq!(
        jump.description(),
        Some(format!("Source: default\n{}", vocabulary::setup_tip("machine.jumpLength")).as_str()),
        "its source, then its help"
    );
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
    let restore = text::fill(crate::et!("Restore {setting}"), &[("setting", &label_name("tatami.length", vocabulary::setup_label("tatami.length")))]);
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

/// Choosing the built-in values keeps the fabric's colour (the stitch preview's cloth), as choosing
/// a preset does, and the preset's values the tab doesn't show go with the rest (the project
/// owner's choice): also when a value is then changed, making them the document's own.
#[test]
fn the_built_in_values_keep_the_fabrics_colour() {
    for changed in [false, true] {
        let mut app = app();
        app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
        app.run("embroidery.setup", json!({"fabric": {"color": "#102030"}})).unwrap();
        assert!(settings(&mut app)["settings"]["fabric"]["thickness"].is_number(), "cotton's thickness");
        open_tab(&mut app);
        edit(&mut app, "fabric", json!(BUILT_IN));
        if changed {
            edit(&mut app, "tatami.pull", json!(0.3));
        }
        crate::dialogs::confirm(&mut app).unwrap();
        let f = settings(&mut app)["settings"]["fabric"].clone();
        assert_eq!(f["color"], json!("#102030"), "changed {changed}: the colour kept");
        assert_eq!(f["preset"], json!(null), "changed {changed}");
        for key in ["thickness", "topping", "needlePoint", "stabilizer"] {
            assert_eq!(f[key], json!(null), "changed {changed}: {key} gone with the preset");
        }
        assert_eq!(f["tatami"]["pull"], if changed { json!(0.3) } else { json!(null) }, "changed {changed}");
    }
}

/// A document's own values stay in the fabric's menu after another fabric is tried: choosing them
/// again puts back the values the dialog opened with.
#[test]
fn custom_stays_in_the_menu_and_puts_its_values_back() {
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": {"tatami": {"pull": 0.3}}})).unwrap();
    let before = settings(&mut app);
    open_tab(&mut app);
    assert_eq!(dialog(&mut app).fields[&field("fabric")], json!(CUSTOM));
    edit(&mut app, "fabric", json!("denim"));
    let o = opened(&app).unwrap().clone();
    let row = fabric_choice(&o, "denim");
    let OwnedField::Choice { options, .. } = &row.field else { panic!("a choice") };
    assert!(options.iter().any(|(id, _)| id == CUSTOM), "{options:?}");
    edit(&mut app, "fabric", json!(CUSTOM));
    assert_eq!(dialog(&mut app).fields[&field("tatami.pull")], json!(0.3));
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(settings(&mut app), before, "as it was");
}

/// In a short window (a 15-inch laptop's at 150 %, 1280 × 650 points), both layouts keep the
/// dialog's OK on the screen, and their rows scroll: the bar's last box can be brought into view.
#[test]
fn a_short_window_keeps_ok_on_screen_and_the_rows_scroll() {
    const SHORT: f32 = 650.0;
    for width in [1280.0, NARROW] {
        let ctx = context();
        let mut app = app();
        let (a, b) = (rect(&mut app, 20.0), rect(&mut app, 100.0));
        app.run("embroidery.set", json!({"ids": [a, b], "part": "fill", "pull": 0.3, "spacing": 0.5, "length": 3})).unwrap();
        open_tab(&mut app);
        let size = vec2(width, SHORT);
        for _ in 0..4 {
            frame_in(&mut app, &ctx, size, vec![]);
        }
        let drawn = frame_in(&mut app, &ctx, size, vec![]);
        let ok = drawn.at("OK");
        assert!(ok.bottom() <= SHORT, "{width}: OK on the screen: {ok:?}");
        let middle = Pos2::new(width / 2.0, SHORT / 2.0);
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -400.0),
            phase: egui::TouchPhase::Move,
            modifiers: Default::default(),
        };
        frame_in(&mut app, &ctx, size, vec![Event::PointerMoved(middle)]);
        for _ in 0..10 {
            frame_in(&mut app, &ctx, size, vec![wheel.clone()]);
        }
        let drawn = frame_in(&mut app, &ctx, size, vec![]);
        let boxes: Vec<(Rect, Rect)> =
            drawn.words.iter().zip(&drawn.clips).filter(|((w, _), _)| w.ends_with(": 2 objects")).map(|((_, r), c)| (*r, *c)).collect();
        let (last, clip) = boxes.iter().max_by(|a, b| a.0.top().total_cmp(&b.0.top())).copied().expect("the bar's boxes");
        assert!(clip.contains_rect(last) && last.bottom() <= SHORT, "{width}: the bar's last box scrolled into view: {last:?} in {clip:?}");
    }
}

/// A value typed outside its range, and a cut length shorter than the jump length, are marked under
/// their field at once, with what they may be, and screen readers hear it with the field: before
/// OK refuses them.
#[test]
fn a_value_ok_would_refuse_is_marked_at_once() {
    let ctx = context();
    let mut app = app();
    open_tab(&mut app);
    edit(&mut app, "tatami.pull", json!(99.0));
    edit(&mut app, "machine.jumpLength", json!(8.0));
    edit(&mut app, "machine.trimLength", json!(6.0));
    let drawn = ready(&mut app, &ctx, WIDE);
    let range = text::fill(crate::et!("From {min} to {max}"), &[("min", "0 mm"), ("max", "2 mm")]);
    assert!(drawn.has(&range), "{range}: {:?}", drawn.words);
    let trim = text::fill(crate::et!("At least the jump length ({value}): only a jump is cut"), &[("value", "8 mm")]);
    assert!(drawn.has(&trim), "{trim}: {:?}", drawn.words);
    let node = drawn.nodes.iter().map(|(_, n)| n).find(|n| n.label() == Some("Cut the thread from")).unwrap();
    assert!(node.description().is_some_and(|d| d.starts_with(&trim)), "{:?}", node.description());
    assert!(crate::dialogs::confirm(&mut app).is_err(), "OK still refuses them");
}

/// Screen readers hear the bar as a group named by its count, and once the fabric changed, that
/// it did (as its mark shows); two ↺ of the same label under both kinds of stitch read apart.
#[test]
fn screen_readers_hear_the_bar_and_each_restore_apart() {
    let ctx = context();
    let mut app = app();
    let a = rect(&mut app, 20.0);
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "pull": 0.3})).unwrap();
    app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
    open_tab(&mut app);
    let group = |drawn: &Drawn| {
        drawn
            .nodes
            .iter()
            .map(|(_, n)| n.clone())
            .find(|n| n.role() == egui::accesskit::Role::Group && n.label() == Some("1 object sets its own values for the fabric."))
            .expect("the bar's group")
    };
    assert_eq!(group(&ready(&mut app, &ctx, WIDE)).description(), None, "nothing changed");
    edit(&mut app, "tatami.pull", json!(0.5));
    edit(&mut app, "satin.pull", json!(0.5));
    let drawn = ready(&mut app, &ctx, WIDE);
    assert_eq!(group(&drawn).description(), Some(crate::et!("The fabric changed")));
    let names: Vec<&str> = drawn.nodes.iter().filter_map(|(_, n)| n.label()).filter(|l| l.starts_with("Restore ")).collect();
    assert!(names.contains(&"Restore Tatami: Pull compensation") && names.contains(&"Restore Satin: Pull compensation"), "{names:?}");
}

/// An opened file's start point names X and Y, so that a language's decimal comma can't run the
/// two numbers together ("12,5, 3,2"); ↺ on the start and the name unsets them on OK.
#[test]
fn the_start_point_names_x_and_y_and_restore_unsets_it() {
    let ctx = context();
    let mut app = app();
    let pt = |mm: f64| mm * 72.0 / 25.4;
    app.run("embroidery.setup", json!({"start": [pt(12.5), pt(3.2)], "label": "Rose"})).unwrap();
    crate::i18n::set_current(crate::i18n::Lang::from_code("pl").unwrap());
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    let start = text::fill(crate::et!("X {x} · Y {y} mm"), &[("x", "12,5"), ("y", "3,2")]);
    crate::i18n::set_current(crate::i18n::Lang::EN);
    assert!(drawn.words.iter().any(|(w, _)| *w == start), "{start}: {:?}", drawn.words);
    crate::dialogs::cancel(&mut app);
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    for key in ["start", "label"] {
        let name = text::fill(crate::et!("Restore {setting}"), &[("setting", vocabulary::setup_label(key))]);
        frame(&mut app, &ctx, WIDE, vec![drawn.click(&name)]);
    }
    ready(&mut app, &ctx, WIDE);
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app)["settings"].clone();
    assert_eq!((s["start"].clone(), s["label"].clone()), (json!(null), json!(null)));
}

/// The tab's fields open with the document's settings, each read by its key from the settings as
/// `embroidery.settings` gives them: a fabric value as stored, the thread's and the machine's as in
/// force; the defaults are a blank document's.
#[test]
fn the_fields_open_with_the_documents_settings() {
    let mut app = app();
    app.run("embroidery.setup", json!({"fabric": "cotton", "threadWeight": 60, "joinSameColor": false, "machine": {"jumpLength": 6}})).unwrap();
    app.run("embroidery.setup", json!({"fabric": {"tatami": {"length": 3.3}, "stabilizer": "cutAway"}})).unwrap();
    open_tab(&mut app);
    let s = settings(&mut app);
    let d = dialog(&mut app).fields.clone();
    assert_eq!(d["embroidery.fabric.tatami.length"], json!(3.3));
    assert_eq!(d["embroidery.fabric.stabilizer"], s["settings"]["fabric"]["stabilizer"]);
    assert_eq!(d["embroidery.fabric.satin.pull"], s["settings"]["fabric"]["satin"]["pull"]);
    assert_eq!((d["embroidery.threadWeight"].clone(), d["embroidery.joinSameColor"].clone()), (json!(60.0), json!(false)));
    assert_eq!(d["embroidery.machine.jumpLength"], json!(6.0));
    assert_eq!(d["embroidery.machine.speed"], s["resolved"]["machine"]["speed"]["value"]);
    let o = opened(&app).unwrap();
    let blank = setup::default_settings();
    assert_eq!(o.default_of("threadWeight"), json!(blank.resolved.thread_weight.value));
    assert_eq!(o.default_of("tatami.pull"), json!(blank.resolved.fabric.tatami.pull.value));
}

/// OK's other branches: a preset chosen over another then a value changed stores the last preset
/// with that value; a value of the document's own edited is stored, no preset; Join and the thread
/// weight are stored, the weight's spacing said under it.
#[test]
fn ok_stores_each_kind_of_change() {
    let ctx = context();
    // Preset → preset → a value.
    let new = app;
    let mut app = new();
    app.run("embroidery.setup", json!({"fabric": "cotton"})).unwrap();
    open_tab(&mut app);
    edit(&mut app, "fabric", json!("denim"));
    edit(&mut app, "fabric", json!("fleece"));
    edit(&mut app, "tatami.pull", json!(0.4));
    crate::dialogs::confirm(&mut app).unwrap();
    let f = settings(&mut app)["settings"]["fabric"].clone();
    assert_eq!((f["preset"].clone(), f["tatami"]["pull"].clone()), (json!("fleece"), json!(0.4)));
    // A value of the document's own edited.
    let mut app = new();
    app.run("embroidery.setup", json!({"fabric": {"tatami": {"pull": 0.3}}})).unwrap();
    open_tab(&mut app);
    edit(&mut app, "tatami.length", json!(3.2));
    crate::dialogs::confirm(&mut app).unwrap();
    let f = settings(&mut app)["settings"]["fabric"].clone();
    assert_eq!((f["preset"].clone(), f["tatami"]["pull"].clone(), f["tatami"]["length"].clone()), (json!(null), json!(0.3), json!(3.2)));
    // Join and the thread weight, its spacing said under it.
    let mut app = new();
    open_tab(&mut app);
    edit(&mut app, "threadWeight", json!(60.0));
    let drawn = ready(&mut app, &ctx, WIDE);
    let spacing = setup::spacing_for_thread(dialog(&mut app).fields[&field("tatami.spacing")].as_f64().unwrap(), 60.0);
    let note = text::fill(
        crate::et!("Rows' spacing for this thread: {value}"),
        &[("value", &fields::format(crate::i18n::Lang::EN, "mm", WORDS_DECIMALS).show(spacing))],
    );
    assert!(drawn.has(&note), "{note}: {:?}", drawn.words);
    let join = crate::et!("Join one thread's objects when nothing lies between them");
    frame(&mut app, &ctx, WIDE, vec![drawn.click(join)]);
    ready(&mut app, &ctx, WIDE);
    crate::dialogs::confirm(&mut app).unwrap();
    let s = settings(&mut app)["settings"].clone();
    assert_eq!((s["threadWeight"].clone(), s["joinSameColor"].clone()), (json!(60.0), json!(false)));
}

/// A batch that fails partway (the cut shorter than the jump, refused by `embroidery.setup` after
/// `document.setup` ran) is rolled back whole: the other tab's change too; the dialog stays.
#[test]
fn a_batch_that_fails_partway_is_rolled_back() {
    let mut app = app();
    let before = steps(&app);
    open_tab(&mut app);
    dialog(&mut app).fields.insert("units".into(), json!("Millimeters"));
    edit(&mut app, "machine.jumpLength", json!(8.0));
    edit(&mut app, "machine.trimLength", json!(6.0));
    assert!(crate::dialogs::confirm(&mut app).is_err());
    assert!(app.ui.dialog.is_some(), "the dialog stays");
    assert_eq!((app.session.active().unwrap().doc.units, steps(&app)), (vectorcraft_doc::Unit::Points, before), "nothing applied");
}

/// The bar applies the fabric to every object that sets its values, whatever is selected
/// (`applyFabric {all: true}`): with one of two selected, the other is cleared too.
#[test]
fn the_bar_applies_the_fabric_whatever_is_selected() {
    let ctx = context();
    let mut app = app();
    let (a, b) = (rect(&mut app, 20.0), rect(&mut app, 100.0));
    app.run("embroidery.set", json!({"ids": [a, b], "part": "fill", "pull": 0.3})).unwrap();
    app.run("select.set", json!({"ids": [a]})).unwrap();
    open_tab(&mut app);
    let drawn = ready(&mut app, &ctx, WIDE);
    let pull = text::fill(crate::et!("{setting}: {objects}"), &[("setting", vocabulary::override_label("pull")), ("objects", "2 objects")]);
    frame(&mut app, &ctx, WIDE, vec![drawn.click(&pull)]);
    ready(&mut app, &ctx, WIDE);
    crate::dialogs::confirm(&mut app).unwrap();
    for id in [a, b] {
        let info = app.run("embroidery.info", json!({"ids": [id]})).unwrap();
        assert_eq!(info["fill"]["settings"]["pull"]["setCount"], json!(0), "{id}: its own pull cleared");
    }
}

/// The bar turns amber once a value of the fabric changes in the dialog (the project owner's
/// choice), not before.
#[test]
fn the_bar_turns_amber_once_the_fabric_changes() {
    let ctx = context();
    let mut app = app();
    let a = rect(&mut app, 20.0);
    app.run("embroidery.set", json!({"ids": [a], "part": "fill", "pull": 0.3})).unwrap();
    open_tab(&mut app);
    let t = Tokens::get(&ctx);
    let amber = BarColours::of(&t, true).fill;
    assert!(!ready(&mut app, &ctx, WIDE).fills.contains(&amber), "not amber yet");
    edit(&mut app, "tatami.length", json!(3.0));
    assert!(ready(&mut app, &ctx, WIDE).fills.contains(&amber), "amber once the fabric changed");
}

/// Junk in any of the tab's fields (an agent's `ui.dialog.set`: wrong types, huge numbers, long
/// strings, lists where numbers go) draws and confirms without a panic: OK refuses what it can't
/// take, with the reason.
#[test]
fn junk_in_the_tabs_fields_draws_and_confirms_without_a_panic() {
    use vectorcraft_testkit::strategies::junk_values;
    let ctx = context();
    let mut app = app();
    rect(&mut app, 20.0);
    open_tab(&mut app);
    let keys: Vec<String> = dialog(&mut app).fields.keys().filter(|k| k.starts_with(PREFIX)).cloned().collect();
    assert!(keys.len() >= 15, "{keys:?}");
    crate::dialogs::cancel(&mut app);
    for key in &keys {
        for junk in junk_values().into_iter().chain([json!(1e308), json!("x".repeat(10_000)), json!([1, 2, 3])]) {
            open_tab(&mut app);
            dialog(&mut app).fields.insert(key.clone(), junk.clone());
            frame(&mut app, &ctx, WIDE, vec![]);
            let _ = crate::dialogs::confirm(&mut app);
            if app.ui.dialog.is_some() {
                crate::dialogs::cancel(&mut app);
            }
        }
    }
}

/// `embroidery.documentSetup` (the Object tab's Embroidery Settings…, and agents) opens Document
/// Setup at its Embroidery tab; an agent then sets a flat field with `ui.dialog.set` and confirms.
#[test]
fn document_setup_opens_at_the_embroidery_tab_for_an_agent() {
    let mut app = app();
    app.run("embroidery.documentSetup", json!({})).unwrap();
    assert_eq!(dialog(&mut app).fields["tab"], json!(TAB));
    dialog(&mut app).fields.insert("embroidery.threadWeight".into(), json!(30));
    crate::dialogs::confirm(&mut app).unwrap();
    assert_eq!(settings(&mut app)["settings"]["threadWeight"], json!(30.0));
    assert!(app.run("embroidery.documentSetup", json!({"tab": "x"})).is_err(), "it takes no parameters");
}
