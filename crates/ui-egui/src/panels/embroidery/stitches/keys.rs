//! The Stitches tab's input: a click on a row (the needle point it is edited by selected, or its
//! stitch shown), the list's keys, Go to #, and the stitch shown on the canvas.

use super::*;

/// A click on record `number`'s row of `listed`: the row in focus; while stitches are edited the
/// needle point it is edited by selected, as `embroidery.selectNeedlePoints` selects it by its
/// record (a jump's the one it lands on, a cut's or a stop's the one it follows; Shift: those of the
/// rows listed from the last clicked, at most [`RANGE_MAX`]; Cmd or Ctrl: one more or one less),
/// else the stitch shown on the canvas (a double-click: zoomed to).
pub(super) fn click(app: &mut VectorcraftApp, shown: &Shown, listed: &Listed, number: usize, mods: egui::Modifiers, double: bool) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let anchor = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.anchor);
    {
        let d = app.embroidery.stitches.docs.entry(uid).or_default();
        d.focus = Some(number);
        if !mods.shift {
            d.anchor = Some(number);
        }
    }
    let editing = crate::embroidery::editing::editing(app);
    // Outside editing, a double-click, or while the plan shown lags an edit (its rows may name
    // needle points the edit moved): the stitch shown.
    let Some(editing) = editing.filter(|_| !double && !shown.stale) else {
        show_stitch(app, shown, number, double);
        return;
    };
    // The rows from the last clicked to this one, as the list shows them (its filter's): rows, not
    // the records between them. A last clicked the list doesn't show now leaves this one alone.
    let (from, to) = match (mods.shift, anchor.and_then(|a| listed.row_of(a)), listed.row_of(number)) {
        (true, Some(a), Some(b)) => (a.min(b), a.max(b)),
        (_, _, Some(b)) => (b, b),
        _ => return,
    };
    let cut = to - from >= RANGE_MAX;
    // Each needle point once: a cut follows the needle point the stitch before it is, and Cmd
    // would otherwise take it away again.
    let mut picked: std::collections::BTreeSet<(u64, usize, usize)> = std::collections::BTreeSet::new();
    for n in (from..=to).take(RANGE_MAX).filter_map(|r| listed.record(r)) {
        let Some(row) = stitch_list::row(&shown.plan, n) else { continue };
        if let Some(a) = edited_by(app, shown, row.unit, n, row.kind) {
            picked.insert((a.node.0, a.index, a.of));
        }
    }
    // Only needle points being edited: what lies outside the object edited (the Whole design scope
    // lists it) is left as it is.
    let ids: Vec<NodeId> = picked.iter().map(|(id, ..)| NodeId(*id)).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let view = app.view_info();
    let held: std::collections::HashSet<u64> = ids.iter().zip(app.session.stitch_edits(view, &ids)).filter(|(_, h)| *h).map(|(id, _)| id.0).collect();
    picked.retain(|(id, ..)| held.contains(id));
    let ranges = ranges_of(&picked);
    if ranges.is_empty() {
        // A row of another object: its stitch shown, as outside editing.
        let inside = stitch_list::row(&shown.plan, number)
            .and_then(|r| app.session.active().and_then(|st| st.doc.ancestry(r.node)))
            .is_some_and(|a| a.contains(&editing.scope.object));
        if !inside {
            show_stitch(app, shown, number, false);
            return;
        }
        // Why: planned from its object's shape, or a run of the object that can't be edited.
        use vectorcraft_engine::cmd::embroidery::plan::Part;
        let planned = stitch_list::row(&shown.plan, number)
            .and_then(|r| shown.plan.units.get(r.unit))
            .is_some_and(|u| !matches!(u.origin.part, Part::Manual | Part::Literal));
        if planned {
            app.status(crate::et!("This stitch can't be edited: it is planned from its object's shape."));
        } else {
            // Why its run can't be, as the chip says it (a run the index doesn't name: as a locked
            // one, the commonest).
            let view = app.view_info();
            let why = stitch_list::row(&shown.plan, number).and_then(|r| app.session.stitch_refusal(view, r.node));
            app.status(crate::embroidery::editing::refused_words(why.unwrap_or(vectorcraft_tools::embroidery::Refusal::Locked), 1));
        }
        return;
    }
    let mode = if mods.command || mods.ctrl { SelectMode::Toggle } else { SelectMode::Set };
    match app.run("embroidery.selectNeedlePoints", json!({"ranges": ranges, "mode": mode.id()})) {
        Err(e) => app.status(e),
        Ok(_) if cut => {
            let count = numbers::count(crate::i18n::current(), RANGE_MAX as u64);
            app.status(crate::embroidery::text::fill(
                crate::et!("Only the first {count} rows of the range are selected: a click selects no more."),
                &[("count", &count)],
            ));
        }
        Ok(_) => {}
    }
}

/// `picked` (path, needle point, the path's needle points), in order, as `ranges` entries: each
/// run of consecutive needle points of a path in one, so a range of a whole run is one entry.
pub(super) fn ranges_of(picked: &std::collections::BTreeSet<(u64, usize, usize)>) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let mut run: Option<(u64, usize, usize, usize)> = None;
    for &(id, k, of) in picked {
        run = match run {
            Some((rid, from, to, rof)) if rid == id && rof == of && to.checked_add(1) == Some(k) => Some((rid, from, k, rof)),
            done => {
                if let Some((rid, from, to, rof)) = done {
                    out.push(json!({"id": rid, "from": from, "to": to, "of": rof}));
                }
                Some((id, k, k, of))
            }
        };
    }
    if let Some((rid, from, to, rof)) = run {
        out.push(json!({"id": rid, "from": from, "to": to, "of": rof}));
    }
    out
}

/// Record `number` shown on the canvas: marked, and the view panned to it when it lies out of view
/// (`zoom`: zoomed to it, as the Check tab's Show zooms, at least [`super::super::check::SHOW_MIN_BOX_MM`]
/// across).
pub(super) fn show_stitch(app: &mut VectorcraftApp, shown: &Shown, number: usize, zoom: bool) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some(row) = stitch_list::row(&shown.plan, number) else { return };
    app.embroidery.stitches.docs.entry(uid).or_default().shown = Some(number);
    let at = Point::new(mm_to_pt(row.at.x), mm_to_pt(row.at.y));
    if zoom {
        let half = mm_to_pt(super::super::check::SHOW_MIN_BOX_MM) / 2.0;
        let rect = [at.x - half, at.y - half, at.x + half, at.y + half];
        if let Err(e) = app.run("view.zoomTo", json!({"rect": rect, "margin": mm_to_pt(super::super::check::SHOW_MARGIN_MM)})) {
            app.status(e);
        }
        return;
    }
    let (Some(canvas), Some(view)) = (app.canvas_rect, app.view().copied()) else { return };
    let xf = crate::canvas::Xf::new(canvas, &view);
    if !canvas.shrink(ROW_HEIGHT).contains(xf.to_screen(at))
        && let Some(v) = app.view_mut()
    {
        v.center = at;
    }
}

/// Record `number` in focus, its row scrolled into view and its stitch marked on the canvas, as a
/// click on its row outside stitch editing leaves them (the pictures of the tab).
#[cfg(test)]
pub(crate) fn reveal(app: &mut VectorcraftApp, number: usize) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    (d.focus, d.shown, d.page, d.reveal) = (Some(number), Some(number), number.saturating_sub(1) / MAX_SCROLLED_ROWS, true);
}

/// The keys while the list has the keyboard's focus (a row clicked gives it; Escape gives it
/// back to the canvas, whose keys then act on the canvas again): ↑ ↓ the row in focus, Page Up/Down
/// a screenful, Home and End (and Cmd or Ctrl with ↑ ↓) the first and last, with Shift while
/// stitches are edited the selection extended to it; Space as a click on it (Shift extends, Cmd or
/// Ctrl takes it in or out); Enter shows it, zoomed to. Keys taken from egui, so neither the
/// canvas's arrows nor upstream's shortcuts see them.
pub(super) fn keys(app: &mut VectorcraftApp, ui: &Ui, list: egui::Id, uid: u64, listed: &Listed, shown: &Shown, screenful: usize) {
    if !ui.memory(|m| m.has_focus(list)) {
        return;
    }
    // egui matches a key ignoring Shift and Alt: they, and Cmd or Ctrl, are read apart.
    let mods = ui.input(|i| i.modifiers);
    let jump = mods.command || mods.ctrl;
    let pressed = |key: egui::Key| {
        ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, key) || i.consume_key(egui::Modifiers::COMMAND, key) || i.consume_key(egui::Modifiers::CTRL, key)
        })
    };
    // Escape first: the list keeps the focus otherwise (its keys are locked to it).
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        ui.memory_mut(|m| m.surrender_focus(list));
        return;
    }
    let last = listed.len().saturating_sub(1);
    // No row of this list in focus (filtered away, or none yet): ↓ and Home start at the first.
    let Some((focus, row)) = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.focus).and_then(|f| listed.row_of(f).map(|r| (f, r))) else {
        if (pressed(egui::Key::ArrowDown) || pressed(egui::Key::Home))
            && let Some(n) = listed.record(0)
            && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
        {
            (d.focus, d.page, d.reveal) = (Some(n), 0, true);
        }
        return;
    };
    if pressed(egui::Key::Enter) {
        show_stitch(app, shown, focus, true);
        return;
    }
    // Space as a click on the row: Shift extends the range from the last clicked, Cmd or Ctrl
    // takes it in or out.
    if pressed(egui::Key::Space) {
        let as_click = egui::Modifiers { shift: mods.shift, command: mods.command, ctrl: mods.ctrl, ..egui::Modifiers::NONE };
        click(app, shown, listed, focus, as_click, false);
        return;
    }
    let target = if pressed(egui::Key::ArrowDown) {
        Some(if jump { last } else { row.saturating_add(1).min(last) })
    } else if pressed(egui::Key::ArrowUp) {
        Some(if jump { 0 } else { row.saturating_sub(1) })
    } else if pressed(egui::Key::PageDown) {
        Some(row.saturating_add(screenful).min(last))
    } else if pressed(egui::Key::PageUp) {
        Some(row.saturating_sub(screenful))
    } else if pressed(egui::Key::Home) {
        Some(0)
    } else if pressed(egui::Key::End) {
        Some(last)
    } else {
        None
    };
    let Some(n) = target.and_then(|r| listed.record(r)) else { return };
    // With Shift while stitches are edited, the selection extends from the last clicked to it, as
    // a Shift-click does.
    if mods.shift && crate::embroidery::editing::editing(app).is_some() {
        click(app, shown, listed, n, egui::Modifiers::SHIFT, false);
    }
    if let (Some(r), Some(d)) = (listed.row_of(n), app.embroidery.stitches.docs.get_mut(&uid)) {
        d.focus = Some(n);
        d.page = r / MAX_SCROLLED_ROWS;
        d.reveal = true;
    }
}

/// Go to #: a record's number or a range typed ("4695", "4695–4700"), in focus and shown (a range:
/// its first).
pub(super) fn go_to(app: &mut VectorcraftApp, ui: &mut Ui, uid: u64, listed: &Listed) {
    let lang = crate::i18n::current();
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    let mut text = d.goto.clone();
    let said = d.said.clone();
    let mut go = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::et!("Go to #")).size(TEXT));
        let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(GO_TO_FIELD).char_limit(GO_TO_CHARS));
        // Its name for screen readers (the words before it are only "Go to #").
        ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(crate::et!("Record number")));
        go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    });
    if let Some(s) = said {
        ui.label(egui::RichText::new(s).size(TEXT).color(Tokens::get(ui.ctx()).text_dim));
    }
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    d.goto = text.clone();
    if !go {
        return;
    }
    // A range's first record (as the chip writes one), else the number alone.
    let first = text.split(['–', '-']).next().map(str::trim).unwrap_or_default();
    let number = record_number(first, lang);
    let (a, b) = listed.span.unwrap_or((0, 0));
    match number.map(|n| (n, listed.row_of(n))) {
        Some((n, Some(r))) => {
            d.focus = Some(n);
            d.page = r / MAX_SCROLLED_ROWS;
            d.reveal = true;
            d.said = None;
        }
        // In the scope, but not among the rows the filter lists.
        Some((n, None)) if (a..=b).contains(&n) => {
            d.said = Some(crate::embroidery::text::fill(
                crate::et!("Record #{value} is left out by the filter chosen."),
                &[("value", &numbers::count(lang, n as u64))],
            ));
        }
        _ => {
            let typed: String = first.chars().take(GO_TO_CHARS).collect();
            d.said = Some(crate::embroidery::text::fill(
                crate::et!("No record numbered {value} here: these are #{first}–{last}."),
                &[("value", &typed), ("first", &numbers::count(lang, a as u64)), ("last", &numbers::count(lang, b as u64))],
            ));
        }
    }
}

/// The most characters Go to # takes, and so says back: a record number of the largest design,
/// grouped, with a `#` before it and room to spare. An estimate.
pub(super) const GO_TO_CHARS: usize = 24;
/// The Go to # field's width, points: room for a seven-digit record number grouped.
const GO_TO_FIELD: f32 = 90.0;

/// The record number Go to # was given as `text`: a whole number as `lang` writes one (its group
/// mark or a space between threes), a `#` before it allowed; `None` for anything else ("1e9",
/// "4.5", a group of two).
pub(super) fn record_number(text: &str, lang: Lang) -> Option<usize> {
    let group = numbers::symbols(lang).group;
    let t = text.trim().trim_start_matches('#').trim();
    let parts: Vec<&str> = t.split(|c: char| c == group || c.is_whitespace()).collect();
    let (head, rest) = parts.split_first()?;
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    // Grouped, each group after the first three digits, the first one to three.
    let well_grouped = rest.is_empty() || (head.len() <= GROUP_DIGITS && rest.iter().all(|p| p.len() == GROUP_DIGITS));
    if !(digits(head) && rest.iter().all(|p| digits(p)) && well_grouped) {
        return None;
    }
    let whole: String = parts.concat();
    whole.parse::<u64>().ok().and_then(|n| usize::try_from(n).ok())
}

/// How many digits a group of a number written grouped holds.
pub(super) const GROUP_DIGITS: usize = 3;
