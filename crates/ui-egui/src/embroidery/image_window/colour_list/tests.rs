//! The colour list: its rows read from the reply and heard whole, a mismatch drawn as the reply
//! says, selection by click, Shift, Cmd/Ctrl, the arrows and Space, the actions' reasons, Choose
//! Thread…'s list, and the choices it writes.

use egui::accesskit::{Node, Role};
use egui::{Modifiers, Pos2, Rect, Shape, vec2};
use serde_json::{Map, Value, json};

use super::*;

/// A reply's colour list: the white background, a red sewn in a thread the reply flags as unlike
/// it (the nearest there is), an orange whose thread is just under the line, a blue a person left
/// out, and a green the reply doesn't flag though its difference is past the line.
fn reply() -> Value {
    let thread = |index: u64, name: &str, color: &str, delta_e: f64, mismatch: bool| json!({"palette": "pec", "index": index, "name": name, "color": color, "deltaE": delta_e, "mismatch": mismatch});
    json!({"colors": [
        {"color": "#e0115f", "background": false, "sewn": true, "thread": thread(7, "Dark Fuchsia", "#c71585", 26.6, true), "reason": "nearest", "choice": null, "mergedInto": null, "share": 16.2, "areaCount": 2},
        {"color": "#f08214", "background": false, "sewn": true, "thread": thread(30, "Orange", "#f07814", 9.96, false), "reason": "nearest", "choice": null, "mergedInto": null, "share": 45.0, "areaCount": 1},
        {"color": "#ffffff", "background": true, "sewn": false, "thread": null, "reason": null, "choice": null, "mergedInto": null, "share": 33.4, "areaCount": 1},
        {"color": "#1428a0", "background": false, "sewn": false, "thread": thread(1, "Prussian Blue", "#1a0a94", 4.2, false), "reason": "chosen", "choice": 0, "mergedInto": null, "share": 5.4, "areaCount": 3},
        {"color": "#11aa22", "background": false, "sewn": true, "thread": thread(12, "Emerald Green", "#00a050", 12.0, false), "reason": "chosen", "choice": 1, "mergedInto": null, "share": 0.0, "areaCount": 1},
    ]})
}

fn en() -> Lang {
    Lang::EN
}

/// A context with the app's fonts and theme, screen readers on.
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx);
    crate::theme::apply(&ctx, Default::default());
    ctx.enable_accesskit();
    ctx
}

/// One frame drawing `rows` → the shapes each row painted (in order), and the screen readers' nodes.
fn drawn(rows: &[Row]) -> (Vec<Vec<Shape>>, Vec<Node>) {
    drawn_in(rows, 420.0)
}

/// [`drawn`] in a screen `width` points wide.
fn drawn_in(rows: &[Row], width: f32) -> (Vec<Vec<Shape>>, Vec<Node>) {
    fn flatten(s: Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let ctx = context();
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 400.0))), ..Default::default() };
    let mut rects = Vec::new();
    let mut out = ctx.run_ui(raw, |ui| {
        for row in rows {
            let d = draw_row(ui, row, false, en(), false);
            rects.push(d.resp.rect);
        }
    });
    out.textures_delta.clear();
    let mut all = Vec::new();
    for c in out.shapes {
        flatten(c.shape, &mut all);
    }
    let within = |s: &Shape, r: Rect| r.contains(s.visual_bounding_rect().center());
    let shapes = rects.iter().map(|&r| all.iter().filter(|s| within(s, r)).cloned().collect()).collect();
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
    (shapes, nodes)
}

/// The texts among `shapes`.
fn texts(shapes: &[Shape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

/// A screen reader hears each row whole: its colour and thread, ΔE, the mismatch's words, its
/// share and areas, and what is set about it; the background's row as it reads.
#[test]
fn a_screen_reader_hears_each_row_whole() {
    let rows = rows_of(&reply());
    assert!(rows.first().is_some_and(|r| r.background), "the background's row first");
    let (_, nodes) = drawn(&rows);
    let mut options: Vec<&Node> = nodes.iter().filter(|n| n.role() == Role::ListBoxOption).collect();
    options.sort_by(|a, b| a.bounds().map_or(0.0, |r| r.y0).total_cmp(&b.bounds().map_or(0.0, |r| r.y0)));
    let heard: Vec<&str> = options.into_iter().filter_map(Node::label).collect();
    assert_eq!(
        heard,
        [
            "Background · 33% of the frame · not sewn",
            "#e0115f in Dark Fuchsia  PEC 7, ΔE 26.6, unlike the image's color, no closer thread in Brother (PES), 16% of the frame, 2 areas",
            "#f08214 in Orange  PEC 30, ΔE 9.96, 45% of the frame, 1 area",
            "#1428a0 in Prussian Blue  PEC 1, ΔE 4.2, 5% of the frame, 3 areas, not sewn",
            "#11aa22 in Emerald Green  PEC 12, ΔE 12.0, 0% of the frame, 1 area",
        ]
    );
}

/// A mismatch is drawn as the reply says (never by a line of the window's own): the warning
/// triangle, amber words and an outlined badge; a difference past the line the reply doesn't flag
/// is drawn plain.
#[test]
fn a_mismatch_is_drawn_with_its_shape_and_read_from_the_reply() {
    let rows = rows_of(&reply());
    let (shapes, _) = drawn(&rows);
    let triangles = |s: &[Shape]| s.iter().filter(|s| matches!(s, Shape::Path(p) if p.points.len() == 3 && p.closed)).count();
    let red = &shapes[1];
    assert_eq!(triangles(red), 1, "the warning triangle");
    assert!(texts(red).iter().any(|t| t.starts_with("unlike the image's color · no closer thread in Brother (PES) · ")), "{:?}", texts(red));
    let outlined =
        |s: &[Shape]| s.iter().filter(|s| matches!(s, Shape::Rect(r) if r.stroke.width > 0.0 && r.fill == egui::Color32::TRANSPARENT)).count();
    let green = &shapes[4];
    assert_eq!(outlined(red), outlined(green) + 1, "the badge outlined, beside the colour square's border");
    assert_eq!(triangles(green), 0, "not flagged by the reply");
    assert!(!texts(green).iter().any(|t| t.contains("unlike")), "{:?}", texts(green));
    assert!(texts(green).contains(&"ΔE 12.0".to_string()));
    // A chosen thread past the line is unlike the colour, but no claim is made that none is closer.
    let mut chosen = rows[1].clone();
    chosen.reason = Some("chosen".into());
    assert!(!row_words(&chosen, en()).contains("no closer thread"));
}

/// The background's colour where the design encloses it is a row of its own, sewn: it says so on
/// screen and to screen readers; the background's row and the other colours don't.
#[test]
fn the_backgrounds_colour_inside_the_design_says_so() {
    let mut r = reply();
    let white = json!({"palette": "pec", "index": 9, "name": "White", "color": "#ffffff", "deltaE": 0.0, "mismatch": false});
    let inside = json!({"color": "#FFFFFF", "background": false, "sewn": true, "thread": white, "reason": "own", "choice": null, "mergedInto": null, "share": 0.1, "areaCount": 3});
    r["colors"].as_array_mut().expect("rows").push(inside);
    let rows = rows_of(&r);
    let marked: Vec<&str> = rows.iter().filter(|r| r.inside).map(|r| r.color.as_str()).collect();
    assert_eq!(marked, ["#FFFFFF"], "only the background's colour that isn't the background");
    let last = rows.last().expect("the row inside");
    let heard = row_words(last, en());
    assert!(heard.ends_with("0% of the frame, 3 areas, the background's color inside the design"), "{heard}");
    assert!(!row_words(&rows[1], en()).contains("inside the design"));
    let (shapes, _) = drawn(&rows);
    let seen = texts(shapes.last().expect("its shapes"));
    assert!(seen.iter().any(|t| t.ends_with(" · the background's color inside the design")), "{seen:?}");
}

/// A list longer than its room shows its scroll bar before the pointer is over it, in a column of
/// its own, as the Layers panel's list does; a list that fits shows none.
#[test]
fn a_list_that_scrolls_shows_its_bar() {
    fn flatten(s: Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let bars = |rows: usize| {
        let ctx = context();
        let mut seen = 0;
        // Two frames: the second knows the rows' height.
        for _ in 0..2 {
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 600.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                scrolled(ui, "rows", rows, ROW_HEIGHT, LIST_MAX_HEIGHT, |ui| {
                    for _ in 0..rows {
                        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::hover());
                    }
                });
            });
            out.textures_delta.clear();
            let mut all = Vec::new();
            out.shapes.into_iter().for_each(|c| flatten(c.shape, &mut all));
            // The rows paint nothing: a narrow, tall, visible rectangle is the bar's.
            seen = all.iter().filter(|s| matches!(s, Shape::Rect(r) if r.fill.a() > 0 && r.rect.width() < 10.0 && r.rect.height() > 30.0)).count();
        }
        seen
    };
    assert_eq!(bars(3), 0, "the rows fit: no bar");
    assert!(bars(10) > 0, "past its room: the bar shown");
}

/// Choose Thread…'s list, longer than its room, shows its scroll bar before the pointer is over it,
/// as a menu's list does: its rows end before the bar, and the bar within the picker's width, under
/// the search field.
#[test]
fn the_thread_choice_shows_its_bar() {
    fn flatten(s: Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.into_iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let ctx = context();
    let mut state = State { picker: Some(Picker { color: "#556b2f".into(), search: String::new(), opening: false }), ..State::default() };
    let mut bars = Vec::new();
    let mut rows_right = Vec::new();
    let mut field_right = None;
    // Two frames: the second knows the rows' height.
    for _ in 0..2 {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 700.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            let button = ui.button("Choose Thread…");
            picker(ui, &mut state, &button, "jef");
        });
        out.textures_delta.clear();
        let nodes: Vec<Node> = out.platform_output.accesskit_update.take().map(|u| u.nodes.into_iter().map(|(_, n)| n).collect()).unwrap_or_default();
        rows_right = nodes
            .iter()
            .filter(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("  JEF ")))
            .filter_map(|n| n.bounds().map(|b| b.x1))
            .collect();
        field_right = nodes.iter().find(|n| n.role() == Role::TextInput).and_then(|n| n.bounds().map(|b| b.x1 as f32));
        let mut all = Vec::new();
        out.shapes.into_iter().for_each(|c| flatten(c.shape, &mut all));
        bars = all
            .into_iter()
            .filter_map(|s| match s {
                Shape::Rect(r) if r.fill.a() > 0 && r.rect.width() < 10.0 && r.rect.height() > 30.0 => Some(r.rect),
                _ => None,
            })
            .collect();
    }
    assert!(!bars.is_empty(), "the bar shown");
    let bar_left = bars.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
    assert!(!rows_right.is_empty() && rows_right.iter().all(|&x| x as f32 <= bar_left), "rows {rows_right:?} end before the bar at {bar_left}");
    let bar_right = bars.iter().map(|r| r.right()).fold(f32::NEG_INFINITY, f32::max);
    let field_right = field_right.expect("the search field");
    assert!(bar_right <= field_right + 0.5, "the bar ends at {bar_right}, within the field's {field_right}");
}

/// A changed row's ↺ is placed over the row: the next row starts as far below it as after any other
/// row, not pulled up under the button.
#[test]
fn a_changed_row_keeps_the_next_row_in_its_place() {
    let rows = rows_of(&reply());
    let changed = rows.iter().find(|r| r.changed && !r.background).expect("a changed row").clone();
    let plain = rows.iter().find(|r| !r.changed && !r.background).expect("a plain row").clone();
    let ctx = context();
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(420.0, 400.0))), ..Default::default() };
    let (mut rects, mut spacing) = (Vec::new(), 0.0);
    let mut out = ctx.run_ui(raw, |ui| {
        spacing = ui.spacing().item_spacing.y;
        for row in [&plain, &changed, &plain] {
            rects.push(draw_row(ui, row, false, en(), false).resp.rect);
        }
    });
    out.textures_delta.clear();
    let gaps: Vec<f32> = rects.windows(2).map(|w| w[1].top() - w[0].bottom()).collect();
    assert_eq!(gaps, [spacing, spacing], "{rects:?}");
}

/// A click on a changed row's ↺ presses it.
#[test]
fn a_changed_rows_restore_is_pressed_by_a_click() {
    let rows = rows_of(&reply());
    let changed = rows.iter().find(|r| r.changed && !r.background).expect("a changed row").clone();
    let ctx = context();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(420.0, 400.0));
    // ↺ sits at the row's right end, mid-height (the row fills the screen's width from its top).
    let at = egui::pos2(screen.right() - RESTORE_ROOM / 2.0, ROW_HEIGHT / 2.0);
    let button = |pressed: bool| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    let mut restored = false;
    for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]] {
        let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            restored |= draw_row(ui, &changed, false, en(), false).restore;
        });
        out.textures_delta.clear();
    }
    assert!(restored, "↺ pressed");
}

/// A changed row's ↺ is named for screen readers by the row's thread, as the row writes it, not by
/// the colour's code.
#[test]
fn a_changed_rows_restore_is_named_by_its_thread() {
    let rows = rows_of(&reply());
    let changed = rows.iter().find(|r| r.changed && !r.background).expect("a changed row").clone();
    let (_, nodes) = drawn(std::slice::from_ref(&changed));
    let named: Vec<&str> = nodes.iter().filter(|n| n.role() == Role::Button).filter_map(|n| n.label()).collect();
    assert_eq!(named, ["Back to Auto: Prussian Blue  PEC 1"]);
}

/// A row too long for its room is cut short with "…" on screen, its thread's name before the badge;
/// screen readers still hear it whole.
#[test]
fn a_long_row_is_cut_short_and_heard_whole() {
    let rows = rows_of(&reply());
    let cut = |width: f32| {
        let (shapes, _) = drawn_in(&rows, width);
        shapes[1].iter().any(|s| matches!(s, Shape::Text(t) if t.galley.text().starts_with("unlike") && t.galley.elided))
    };
    assert!(!cut(1200.0), "room enough: whole");
    assert!(cut(220.0), "cut short with …");
    let (_, nodes) = drawn_in(&rows, 220.0);
    assert!(nodes.iter().any(|n| n.label().is_some_and(|l| l.ends_with("16% of the frame, 2 areas"))), "heard whole");
}

/// ΔE to one decimal, two where one would round onto the line.
#[test]
fn delta_e_never_reads_as_the_line_itself() {
    assert_eq!(delta_e_words(en(), 26.6), "ΔE 26.6");
    assert_eq!(delta_e_words(en(), 9.96), "ΔE 9.96");
    assert_eq!(delta_e_words(en(), 10.04), "ΔE 10.04");
    assert_eq!(delta_e_words(en(), 10.0), "ΔE 10.0");
    assert_eq!(delta_e_words(en(), 9.94), "ΔE 9.9");
}

/// A click selects a row alone, Shift a range from the last clicked, Cmd/Ctrl adds or takes one
/// away; ↑/↓ move it (with Shift, extend it), Space adds or takes away; the background's row is
/// never selected.
#[test]
fn rows_are_selected_by_click_shift_command_and_arrows() {
    let rows = rows_of(&reply());
    let colours = |s: &State| s.selected.iter().cloned().collect::<Vec<_>>();
    let mut s = State::default();
    click(&mut s, &rows, 1, Modifiers::NONE);
    assert_eq!(colours(&s), ["#e0115f"]);
    click(&mut s, &rows, 3, Modifiers::SHIFT);
    assert_eq!(colours(&s), ["#1428a0", "#e0115f", "#f08214"]);
    click(&mut s, &rows, 2, Modifiers::COMMAND);
    assert_eq!(colours(&s), ["#1428a0", "#e0115f"]);
    click(&mut s, &rows, 0, Modifiers::NONE);
    assert_eq!(colours(&s), ["#1428a0", "#e0115f"], "the background's row isn't selected");
    assert_eq!(step(&mut s, &rows, 3, false, false), Some(2));
    assert_eq!(colours(&s), ["#f08214"]);
    assert_eq!(step(&mut s, &rows, 2, false, false), Some(1));
    assert_eq!(step(&mut s, &rows, 1, false, false), None, "the background's row above");
    assert_eq!(colours(&s), ["#e0115f"]);
    assert_eq!(step(&mut s, &rows, 1, true, true), Some(2));
    assert_eq!(colours(&s), ["#e0115f", "#f08214"]);
    toggle(&mut s, &rows, 4);
    assert_eq!(colours(&s), ["#11aa22", "#e0115f", "#f08214"]);
    toggle(&mut s, &rows, 1);
    assert_eq!(colours(&s), ["#11aa22", "#f08214"]);
}

/// Space on the row with the keyboard's focus adds it to the selection, and takes it away again;
/// Enter on it selects it alone, as a click does. Through real frames, as egui counts Space and
/// Enter on a focused widget as its click too.
#[test]
fn space_on_the_focused_row_adds_it_to_the_selection_or_takes_it_away() {
    let rows = rows_of(&reply());
    let ctx = context();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(420.0, 400.0));
    let mut state = State::default();
    click(&mut state, &rows, 1, Modifiers::NONE);
    state.focus = Some(rows[2].color.clone());
    let frame = |state: &mut State, events: Vec<egui::Event>| {
        let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            for (k, row) in rows.iter().enumerate() {
                let drawn = draw_row(ui, row, state.selected.contains(&row.color), en(), state.focus.as_deref() == Some(row.color.as_str()));
                if drawn.focused_now {
                    state.focus = None;
                }
                row_input(ui, state, &rows, k, &drawn.resp);
            }
        });
        out.textures_delta.clear();
    };
    let press = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    let selected = |state: &State| state.selected.iter().cloned().collect::<Vec<_>>();
    frame(&mut state, vec![]);
    frame(&mut state, vec![]);
    frame(&mut state, vec![press(egui::Key::Space)]);
    assert_eq!(selected(&state), [rows[1].color.clone(), rows[2].color.clone()], "Space added the focused row");
    frame(&mut state, vec![press(egui::Key::Space)]);
    assert_eq!(selected(&state), [rows[1].color.clone()], "and took it away");
    frame(&mut state, vec![press(egui::Key::Enter)]);
    assert_eq!(selected(&state), [rows[2].color.clone()], "Enter selects it alone, as a click does");
}

/// Merge takes two or more, Choose Thread… one, Don't Sew one or more: each says why it can't.
/// Merged colours are sewn as the one with the most of the frame.
#[test]
fn merge_is_disabled_with_its_reason() {
    let rows = rows_of(&reply());
    let mut s = State::default();
    let none = s.chosen(&rows);
    assert_eq!(
        (merge_refused(&none), sew_refused(&none)),
        (Some("Select two or more colors to merge them."), Some("Select one or more colors first."))
    );
    click(&mut s, &rows, 1, Modifiers::NONE);
    let one = s.chosen(&rows);
    assert_eq!((merge_refused(&one), choose_refused(&one), sew_refused(&one)), (Some("Select two or more colors to merge them."), None, None));
    click(&mut s, &rows, 3, Modifiers::COMMAND);
    let two = s.chosen(&rows);
    assert_eq!((merge_refused(&two), choose_refused(&two)), (None, Some("Select one color to choose its thread.")));
    let mut f = Map::new();
    merge(&mut f, &two);
    assert_eq!(f[CHOICES], json!([{"color": "#1428a0", "mergeInto": "#e0115f"}]));
}

/// Choose Thread… lists the palette's threads nearest the colour first, and finds them by name
/// (any case) or by the start of their number.
#[test]
fn choose_thread_lists_threads_nearest_first_and_searches() {
    let all = picker_rows("pec", "#ffffff", "");
    assert_eq!(all.len(), 61);
    assert!(all.windows(2).all(|w| w[0].delta_e <= w[1].delta_e), "nearest first");
    assert_eq!(all.first().map(|t| t.name), Some("White"));
    let blue = picker_rows("pec", "#ffffff", "BLUE");
    assert!(!blue.is_empty() && blue.iter().all(|t| t.name.to_lowercase().contains("blue")));
    let ones = picker_rows("pec", "#ffffff", "1");
    assert!(!ones.is_empty() && ones.iter().all(|t| t.index.to_string().starts_with('1')));
    assert_eq!(ones.len(), 11, "1, and 10 to 19");
    assert!(picker_rows("pec", "not a colour", "").is_empty());
}

/// The list's actions write the engine's colour choices: a thread, not sewn and sewn again, back
/// to Auto, and the threads left out when the palette changes (merges and jobs kept).
#[test]
fn the_actions_write_colour_choices() {
    let mut f = Map::new();
    choose_thread(&mut f, "#e0115f", "pec", 12);
    assert_eq!(f[CHOICES], json!([{"color": "#e0115f", "thread": {"palette": "pec", "index": 12}}]));
    set_sewn(&mut f, &["#e0115f".to_string()], false);
    assert_eq!(f[CHOICES], json!([{"color": "#e0115f", "thread": {"palette": "pec", "index": 12}, "job": "none"}]));
    set_sewn(&mut f, &["#e0115f".to_string()], true);
    assert_eq!(f[CHOICES], json!([{"color": "#e0115f", "thread": {"palette": "pec", "index": 12}}]));
    reset(&mut f, "#E0115F");
    assert_eq!(f.get(CHOICES), None, "nothing left: no choices");
    choose_thread(&mut f, "#e0115f", "pec", 12);
    set_sewn(&mut f, &["#1428a0".to_string()], false);
    drop_threads(&mut f);
    assert_eq!(f[CHOICES], json!([{"color": "#1428a0", "job": "none"}]));
}
