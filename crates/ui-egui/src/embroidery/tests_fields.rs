//! The embroidery fields (`fields::number_field`, `fields::choice`) over the app's own: in no look
//! they draw as the app's fields; a look colours the value while it isn't edited, shows words in an
//! empty field and names the field for screen readers; a format writes its decimal mark, reads both
//! marks, and steps in its own size.

use std::ops::RangeInclusive;

use egui::accesskit::Node;
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, Shape, Ui, vec2};

use crate::embroidery::fields::{self, Look, MM_STEP, NumberFormat, UNIT_STEP};
use crate::widgets;

/// Any number: a field without bounds.
const ANY: RangeInclusive<f64> = f64::NEG_INFINITY..=f64::INFINITY;
const RED: Color32 = Color32::from_rgb(200, 0, 0);

/// A format with a point: the app's fields', stepping by one; `unit` after a space.
fn plain(unit: &str, decimals: usize) -> NumberFormat<'_> {
    NumberFormat { unit, space: " ", latin: unit, unit_first: false, decimals, mark: '.', group: ',', step: UNIT_STEP }
}

/// A context with the app's fonts and theme, screen readers on.
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    ctx.enable_accesskit();
    ctx
}

/// One frame of `draw` at `time` with `events` → its shapes in the order they are painted, and its
/// screen-reader nodes.
fn frame(ctx: &egui::Context, time: f64, events: Vec<Event>, draw: &mut dyn FnMut(&mut Ui)) -> (Vec<Shape>, Vec<Node>) {
    fn flatten(s: Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let raw = egui::RawInput {
        time: Some(time),
        focused: true,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| draw(ui));
    out.textures_delta.clear();
    let mut shapes = vec![];
    for c in out.shapes {
        flatten(c.shape, &mut shapes);
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
    (shapes, nodes)
}

/// The texts drawn, in the order painted, with the colour of their first section.
fn texts(shapes: &[Shape]) -> Vec<(String, Color32)> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color))),
            _ => None,
        })
        .collect()
}

/// In no look and the app's format, an embroidery number field draws exactly as the app's own.
#[test]
fn a_field_in_no_look_draws_as_the_apps_own() {
    let ctx = context();
    let own = frame(&ctx, 0.0, vec![], &mut |ui| {
        widgets::mixed_field(ui, "f", Some(0.38), " mm", 2, 90.0);
    });
    let embroidery = frame(&context(), 0.0, vec![], &mut |ui| {
        fields::number_field(ui, "f", Some(0.38), &plain("mm", 2), &Look::default(), 90.0, &ANY);
    });
    assert_eq!(format!("{:?}", own.0), format!("{:?}", embroidery.0));
    let t = crate::theme::Tokens::get(&ctx);
    assert_eq!(texts(&own.0), [("0.38 mm".to_string(), t.text_strong)]);
}

/// A look shows the value in its colour while the field isn't edited (over the app's own words, in
/// their place), and the app's strong text while it is; an empty field shows its words in the
/// value's colour; screen readers hear the field's name and where its value comes from.
#[test]
fn a_look_colours_the_value_and_names_the_field() {
    let look = Look { text: Some(RED), hint: Some("Auto"), name: Some("Row spacing"), description: Some("from the fabric") };
    let comma = NumberFormat { mark: ',', ..plain("mm", 2) };
    let ctx = context();
    let t = crate::theme::Tokens::get(&ctx);
    let mut at = Pos2::ZERO;
    let (shapes, nodes) = frame(&ctx, 0.0, vec![], &mut |ui| {
        fields::number_field(ui, "f", Some(0.38), &comma, &look, 90.0, &ANY);
        at = ui.min_rect().center();
    });
    let drawn = texts(&shapes);
    assert_eq!(drawn.last(), Some(&("0,38 mm".to_string(), RED)), "the value in the look's colour, over the app's: {drawn:?}");
    let places: Vec<Pos2> = shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some(t.pos),
            _ => None,
        })
        .collect();
    let (own, over) = (places.first(), places.last());
    assert_eq!(own, over, "in the app's words' place");
    let node = nodes.iter().find(|n| n.label() == Some("Row spacing")).expect("the field is named");
    assert_eq!(node.description(), Some("from the fabric"));
    // Being edited: the app's strong text alone.
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame(&ctx, 0.1, vec![Event::PointerMoved(at), button(true)], &mut |ui| {
        fields::number_field(ui, "f", Some(0.38), &comma, &look, 90.0, &ANY);
    });
    let (shapes, _) = frame(&ctx, 0.2, vec![button(false)], &mut |ui| {
        fields::number_field(ui, "f", Some(0.38), &comma, &look, 90.0, &ANY);
    });
    assert!(texts(&shapes).iter().all(|(_, c)| *c != RED), "edited in the app's text: {:?}", texts(&shapes));
    assert!(texts(&shapes).iter().any(|(w, c)| w == "0,38 mm" && *c == t.text_strong));
    let (shapes, _) = frame(&ctx, 0.3, vec![], &mut |ui| {
        fields::number_field(ui, "g", None, &comma, &look, 90.0, &ANY);
    });
    assert!(texts(&shapes).contains(&("Auto".to_string(), RED)), "the words in an empty field, in the value's colour");
}

/// A choice in a look shows its value in the look's colour, named for screen readers; its list
/// keeps the app's colours.
#[test]
fn a_choice_in_a_look() {
    let look = Look { text: Some(RED), name: Some("Stitch type"), ..Look::default() };
    let (shapes, nodes) = frame(&context(), 0.0, vec![], &mut |ui| {
        let before = ui.visuals().override_text_color;
        fields::choice(ui, "c", "Auto: tatami", &look, 120.0, |ui| ui.button("x").clicked().then_some(()));
        assert_eq!(ui.visuals().override_text_color, before, "the ui's colours as they were");
    });
    assert_eq!(texts(&shapes), [("Auto: tatami".to_string(), RED)]);
    let node = nodes.iter().find(|n| n.label() == Some("Stitch type")).expect("named");
    assert_eq!(node.role(), egui::accesskit::Role::ComboBox, "the combo itself is named");
}

/// A comma format writes its comma and reads both marks; a point format reads no comma, so "1,000"
/// is never one.
#[test]
fn a_format_writes_its_mark_and_reads_both() {
    let comma = NumberFormat { mark: ',', ..plain("mm", 2) };
    assert_eq!(comma.show(0.35), "0,35 mm");
    assert_eq!((comma.read("0,35"), comma.read("0.35 mm"), comma.read("1,5*2")), (Some(0.35), Some(0.35), Some(3.0)));
    let point = plain("mm", 2);
    assert_eq!((point.show(0.35), point.read("0.35"), point.read("1,000")), ("0.35 mm".to_string(), Some(0.35), None));
    let percent = NumberFormat { mark: ',', space: "\u{a0}", ..plain("%", 0) };
    assert_eq!(percent.read(&percent.show(120.0)), Some(120.0), "a no-break space reads as a space");
}

/// ↑ in a focused field steps by its format's step (a tenth of a millimetre: 0.38 up to 0.4, Shift
/// a whole one, Ctrl/Cmd a hundredth), a step of one by one.
#[test]
fn a_field_steps_in_its_own_size() {
    /// One frame of the field showing `value` → what it gives, and its centre.
    fn field(ctx: &egui::Context, format: &NumberFormat<'_>, time: f64, events: Vec<Event>, value: f64) -> (Option<f64>, Pos2) {
        let mut got = (None, Pos2::ZERO);
        frame(ctx, time, events, &mut |ui| {
            got.0 = fields::number_field(ui, "f", Some(value), format, &Look::default(), 90.0, &ANY);
            got.1 = ui.min_rect().center();
        });
        got
    }
    // From 0.38: up to the next step, then Shift up ten steps from there, then Ctrl/Cmd up a tenth
    // of a step.
    for (step, up, shift_up, command_up) in [(MM_STEP, 0.4, 1.4, 1.41), (UNIT_STEP, 1.0, 11.0, 11.1)] {
        let ctx = context();
        let format = NumberFormat { step, ..plain("mm", 2) };
        let at = field(&ctx, &format, 0.0, vec![], 0.38).1;
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let key = |modifiers| Event::Key { key: Key::ArrowUp, physical_key: None, pressed: true, repeat: false, modifiers };
        field(&ctx, &format, 0.1, vec![Event::PointerMoved(at), button(true)], 0.38);
        field(&ctx, &format, 0.2, vec![button(false)], 0.38);
        let stepped = field(&ctx, &format, 0.3, vec![key(Modifiers::NONE)], 0.38).0.expect("a step");
        assert!((stepped - up).abs() < 1e-9, "step {step}: up to {stepped}");
        let stepped = field(&ctx, &format, 0.4, vec![key(Modifiers::SHIFT)], stepped).0.expect("a step");
        assert!((stepped - shift_up).abs() < 1e-9, "step {step}: Shift up to {stepped}");
        let stepped = field(&ctx, &format, 0.5, vec![key(Modifiers::COMMAND)], stepped).0.expect("a step");
        assert!((stepped - command_up).abs() < 1e-9, "step {step}: Ctrl/Cmd up to {stepped}");
    }
}

/// A row of a choice whose words are `shown`.
fn choice_row<'a>(shown: &'a str, options: &'a [(String, String)]) -> crate::embroidery::rows::Row<'a> {
    use crate::embroidery::rows::{Field, Row};
    Row {
        key: "stitch",
        label: "Type",
        name: "Stitch: Type",
        tip: "",
        field: Field::Choice { shown, options, current: None },
        own: false,
        restore: None,
        aside: Some("fabric".into()),
        hint: None,
        description: String::new(),
        problem: None,
    }
}

/// A choice grows into the room its row has for its words (Document Setup's wide columns); where
/// they are cut, the cell's tooltip holds them whole.
#[test]
fn a_choice_grows_for_its_words_and_tips_them_when_cut() {
    use crate::embroidery::rows::{self, Columns};
    const WORDS: &str = "Edge run and tatami under the long rows of the fill";
    let options = vec![("a".to_string(), WORDS.to_string())];
    let ctx = context();
    // The words drawn whole (a cut galley keeps its whole text, marked elided).
    let whole = |shapes: &[Shape]| shapes.iter().any(|s| matches!(s, Shape::Text(t) if t.galley.text() == WORDS && !t.galley.elided));
    let drawn_in = |width: f32, time: f64, events: Vec<Event>| {
        frame(&ctx, time, events, &mut |ui| {
            ui.set_width(width);
            let row = choice_row(WORDS, &options);
            let cols = Columns::fit(ui, width, std::slice::from_ref(&row));
            rows::show(ui, &cols, &row);
        })
        .0
    };
    assert!(whole(&drawn_in(390.0, 0.0, vec![])), "whole where there is room");
    let narrow = drawn_in(220.0, 1.0, vec![]);
    assert!(!whole(&narrow), "cut in a narrow column");
    let at = narrow
        .iter()
        .find_map(|s| match s {
            Shape::Text(t) if t.galley.text() == WORDS => Some(t.visual_bounding_rect().center()),
            _ => None,
        })
        .expect("its words");
    let mut tipped = false;
    for k in 0..30 {
        let events = if k == 0 { vec![Event::PointerMoved(at)] } else { vec![] };
        tipped |= whole(&drawn_in(220.0, 1.1 + 0.1 * f64::from(k), events));
    }
    assert!(tipped, "whole in the tooltip");
}
