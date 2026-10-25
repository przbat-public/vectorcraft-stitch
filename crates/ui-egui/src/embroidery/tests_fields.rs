//! A field's look and number format chosen by its caller (`widgets::Look`, `widgets::NumberFormat`,
//! `widgets::number_field`, `widgets::combo_look`), as the embroidery fields take them: the app's
//! own fields draw as before; a look colours the value and the frame, shows a hint in an empty
//! field and names the field for screen readers; a format writes its decimal mark, reads both
//! marks, and steps in its own size.

use std::ops::RangeInclusive;

use egui::accesskit::Node;
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, Shape, Ui, vec2};

use crate::widgets::{self, Look, NumberFormat};

/// Any number: a field without bounds.
const ANY: RangeInclusive<f64> = f64::NEG_INFINITY..=f64::INFINITY;
const RED: Color32 = Color32::from_rgb(200, 0, 0);
const BLUE: Color32 = Color32::from_rgb(0, 0, 200);

/// A context with the app's fonts and theme, screen readers on.
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    ctx.enable_accesskit();
    ctx
}

/// One frame of `draw` at `time` with `events` → its shapes, flattened, and its screen-reader nodes.
fn frame(ctx: &egui::Context, time: f64, events: Vec<Event>, draw: &mut dyn FnMut(&mut Ui)) -> (Vec<Shape>, Vec<Node>) {
    let raw = egui::RawInput {
        time: Some(time),
        focused: true,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 200.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| draw(ui));
    out.textures_delta.clear();
    let (mut shapes, mut pending): (Vec<Shape>, Vec<Shape>) = (vec![], out.shapes.into_iter().map(|c| c.shape).collect());
    while let Some(s) = pending.pop() {
        match s {
            Shape::Vec(v) => pending.extend(v),
            other => shapes.push(other),
        }
    }
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
    (shapes, nodes)
}

/// The texts drawn, with the colour of their first section.
fn texts(shapes: &[Shape]) -> Vec<(String, Color32)> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color))),
            _ => None,
        })
        .collect()
}

/// The app's own number field and a number field in the plain format and look draw the same.
#[test]
fn a_plain_number_field_draws_as_the_apps_own() {
    let ctx = context();
    let own = frame(&ctx, 0.0, vec![], &mut |ui| {
        widgets::mixed_field(ui, "f", Some(0.38), " mm", 2, 90.0);
    });
    let plain = frame(&context(), 0.0, vec![], &mut |ui| {
        widgets::number_field(ui, "f", Some(0.38), &NumberFormat::plain(" mm", 2), &Look::PLAIN, 90.0, &ANY);
    });
    assert_eq!(format!("{:?}", own.0), format!("{:?}", plain.0));
    let t = crate::theme::Tokens::get(&ctx);
    assert_eq!(texts(&own.0), [("0.38 mm".to_string(), t.text_strong)]);
    assert!(own.0.iter().any(|s| matches!(s, Shape::Rect(r) if r.stroke.color == t.input_border)), "the input border");
}

/// A look colours the value and the frame; an empty field shows its hint in the value's colour;
/// screen readers hear the field's name and where its value comes from.
#[test]
fn a_look_colours_the_value_and_the_frame_and_names_the_field() {
    let look = Look { text: Some(RED), border: Some(BLUE), hint: Some("Auto"), name: Some("Row spacing"), description: Some("from the fabric") };
    let comma = NumberFormat { mark: ',', ..NumberFormat::plain(" mm", 2) };
    let ctx = context();
    let (shapes, nodes) = frame(&ctx, 0.0, vec![], &mut |ui| {
        widgets::number_field(ui, "f", Some(0.38), &comma, &look, 90.0, &ANY);
    });
    assert_eq!(texts(&shapes), [("0,38 mm".to_string(), RED)]);
    assert!(shapes.iter().any(|s| matches!(s, Shape::Rect(r) if r.stroke.color == BLUE)), "the frame in the look's colour");
    let node = nodes.iter().find(|n| n.label() == Some("Row spacing")).expect("the field is named");
    assert_eq!(node.description(), Some("from the fabric"));
    let (shapes, _) = frame(&ctx, 0.1, vec![], &mut |ui| {
        widgets::number_field(ui, "g", None, &comma, &look, 90.0, &ANY);
    });
    assert!(texts(&shapes).contains(&("Auto".to_string(), RED)), "the hint in an empty field, in the value's colour");
}

/// A combo in a look shows its value in the look's colour inside the look's frame, named for
/// screen readers.
#[test]
fn a_combo_in_a_look() {
    let look = Look { text: Some(RED), border: Some(BLUE), name: Some("Stitch type"), ..Look::PLAIN };
    let (shapes, nodes) = frame(&context(), 0.0, vec![], &mut |ui| {
        widgets::combo_look(ui, "c", "Auto: tatami", &look, 120.0, false, |ui| ui.button("x").clicked().then_some(()));
    });
    assert_eq!(texts(&shapes), [("Auto: tatami".to_string(), RED)]);
    assert!(shapes.iter().any(|s| matches!(s, Shape::Rect(r) if r.stroke.color == BLUE)));
    assert!(nodes.iter().any(|n| n.label() == Some("Stitch type")), "named");
}

/// A comma format writes its comma and reads both marks; a point format reads no comma, so "1,000"
/// is never one.
#[test]
fn a_format_writes_its_mark_and_reads_both() {
    let comma = NumberFormat { mark: ',', ..NumberFormat::plain(" mm", 2) };
    assert_eq!(comma.show(0.35), "0,35 mm");
    assert_eq!((comma.read("0,35"), comma.read("0.35 mm"), comma.read("1,5*2")), (Some(0.35), Some(0.35), Some(3.0)));
    let point = NumberFormat::plain(" mm", 2);
    assert_eq!((point.show(0.35), point.read("0.35"), point.read("1,000")), ("0.35 mm".to_string(), Some(0.35), None));
    let percent = NumberFormat { mark: ',', ..NumberFormat::plain("\u{a0}%", 0) };
    assert_eq!(percent.read(&percent.show(120.0)), Some(120.0), "a no-break space reads as a space");
}

/// ↑ in a focused field steps by its format's step (a tenth: 0.38 up to 0.4, Shift a whole one),
/// and the app's fields by one.
#[test]
fn a_field_steps_in_its_own_size() {
    /// One frame of the field showing `value` → what it gives, and its centre.
    fn field(ctx: &egui::Context, format: &NumberFormat<'_>, time: f64, events: Vec<Event>, value: f64) -> (Option<f64>, Pos2) {
        let mut got = (None, Pos2::ZERO);
        frame(ctx, time, events, &mut |ui| {
            got.0 = widgets::number_field(ui, "f", Some(value), format, &Look::PLAIN, 90.0, &ANY);
            got.1 = ui.min_rect().center();
        });
        got
    }
    // From 0.38: up to the next step, then Shift up ten steps from there.
    for (step, up, shift_up) in [(0.1, 0.4, 1.4), (1.0, 1.0, 11.0)] {
        let ctx = context();
        let format = NumberFormat { step, ..NumberFormat::plain(" mm", 2) };
        let at = field(&ctx, &format, 0.0, vec![], 0.38).1;
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let key = |modifiers| Event::Key { key: Key::ArrowUp, physical_key: None, pressed: true, repeat: false, modifiers };
        field(&ctx, &format, 0.1, vec![Event::PointerMoved(at), button(true)], 0.38);
        field(&ctx, &format, 0.2, vec![button(false)], 0.38);
        let stepped = field(&ctx, &format, 0.3, vec![key(Modifiers::NONE)], 0.38).0.expect("a step");
        assert!((stepped - up).abs() < 1e-9, "step {step}: up to {stepped}");
        let stepped = field(&ctx, &format, 0.4, vec![key(Modifiers::SHIFT)], stepped).0.expect("a step");
        assert!((stepped - shift_up).abs() < 1e-9, "step {step}: Shift up to {stepped}");
    }
}
