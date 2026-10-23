//! The Sewing order tab: the design's thread blocks in the order they are sewn, each with its
//! objects, and what the machine does between them; the design's figures in a footer.
//!
//! - **It reads the typed model** (`embroidery.sequence`'s reply read back into
//!   [`SewingOrder`]), once per plan, and the words come from its ids and numbers, never from the
//!   reply's English sentences.
//! - **Rows**: a block's header (its number, its thread's chip, name and palette number, a ΔE
//!   badge when the thread is a mismatch, its objects and stitches); a row per sewing unit (the
//!   object's name as the Layers panel names it, its stitches; under it what part it sews and how);
//!   a row for the thread change between blocks. Rows have three heights; only the rows in view
//!   are laid out (a list of 10,000 units scrolls as one of ten).
//! - **Selection, both ways.** A click selects the row's object (Shift extends to a range,
//!   Cmd/Ctrl adds or removes); a header selects its block's objects. A row whose object is
//!   selected, or inside one that is, is highlighted.
//! - Blocks collapse and expand (the header's triangle).

use std::collections::HashSet;
use std::sync::Arc;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::order::{OrderBlock, OrderUnit, SewingOrder};

use crate::VectorcraftApp;
use crate::embroidery::numbers;
use crate::theme::Tokens;

/// The rows' heights, points: a block's header, a unit's two lines, a thread change. Estimates
/// fitted to the mockups.
pub const HEADER_ROW: f32 = 28.0;
pub const UNIT_ROW: f32 = 36.0;
pub const CHANGE_ROW: f32 = 20.0;
/// How far a block's rows are indented under its header, points.
const INDENT: f32 = 24.0;
/// The thread chip's side, points.
const CHIP: f32 = 12.0;
/// The room kept under the list for the footer, points: its title, up to four rows of figures and
/// a note. An estimate fitted to the mockups.
const FOOTER: f32 = 180.0;
/// The room a figure takes in the footer, points: its widest value ("about 120 min") and a gap.
/// An estimate.
const FIGURE_WIDTH: f32 = 96.0;
/// The sizes of the rows' words, points: the interface's text and its small text.
const TEXT: f32 = 12.0;
const SMALL: f32 = 11.0;

/// One row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row {
    /// A block's header (its index in the order's blocks).
    Header(usize),
    /// A unit (its block's index, its index in the block's units).
    Unit(usize, usize),
    /// The thread change after a block.
    Change(usize),
}

impl Row {
    fn height(self) -> f32 {
        match self {
            Row::Header(_) => HEADER_ROW,
            Row::Unit(..) => UNIT_ROW,
            Row::Change(_) => CHANGE_ROW,
        }
    }
}

/// The rows of an order, with the blocks in `collapsed` showing their header alone, and where
/// each row starts (points from the list's top); the last entry of `tops` is the list's height.
pub(crate) fn rows(order: &SewingOrder, collapsed: &HashSet<usize>) -> (Vec<Row>, Vec<f32>) {
    let mut rows = Vec::new();
    for (b, block) in order.blocks.iter().enumerate() {
        rows.push(Row::Header(b));
        if !collapsed.contains(&block.block) {
            rows.extend((0..block.units.len()).map(|u| Row::Unit(b, u)));
        }
        if block.then.is_some() {
            rows.push(Row::Change(b));
        }
    }
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut y = 0.0;
    for r in &rows {
        tops.push(y);
        y += r.height();
    }
    tops.push(y);
    (rows, tops)
}

/// The tab's state: the order read for the plan shown, its rows, the collapsed blocks (by
/// number), and the unit row clicked last (for Shift-click ranges).
#[derive(Default)]
pub struct OrderState {
    read: Option<(u64, Arc<SewingOrder>)>,
    collapsed: HashSet<usize>,
    anchor: Option<(usize, usize)>,
    /// What the selection covers (the selected objects and everything inside them), for
    /// (document, revision).
    covered: Option<((u64, u64), Covered)>,
}

/// The ids a selection covers.
type Covered = Arc<HashSet<u64>>;

/// The query the tab asks the preview for: the whole order (as many units as a reply may list).
pub(crate) fn query() -> crate::embroidery::plan_worker::Query {
    crate::embroidery::plan_worker::Query { cmd: "embroidery.sequence", params: json!({ "limit": MAX_LIST_UNITS }) }
}

/// The most units the tab lists: as many as `embroidery.sequence` lists at most.
pub const MAX_LIST_UNITS: usize = 10_000;

/// The order of the plan of generation `generation` from `reply`, read once.
fn read(state: &mut OrderState, generation: u64, reply: &Value) -> Option<Arc<SewingOrder>> {
    if let Some((g, order)) = &state.read
        && *g == generation
    {
        return Some(order.clone());
    }
    let order = Arc::new(serde_json::from_value::<SewingOrder>(reply.clone()).ok()?);
    state.read = Some((generation, order.clone()));
    Some(order)
}

/// The ids the selection covers in `doc`: the selected objects and everything inside them.
fn covered(state: &mut OrderState, uid: u64, revision: u64, doc: &Document, selected: &[NodeId]) -> Arc<HashSet<u64>> {
    if let Some((k, set)) = &state.covered
        && *k == (uid, revision)
    {
        return set.clone();
    }
    let mut set = HashSet::new();
    for id in selected {
        if let Some(n) = doc.node(*id) {
            n.walk(&mut |c| {
                set.insert(c.id.0);
            });
        }
    }
    let set = Arc::new(set);
    state.covered = Some(((uid, revision), set.clone()));
    set
}

/// The tab's body, under the panel's header: the list, then the footer. `stale`: the plan shown
/// is of an earlier state (the list is dimmed). `reply` is `embroidery.sequence`'s for the plan
/// of generation `generation`, `stats` the figures.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, generation: u64, reply: &Value, stats: Option<&Value>, stale: bool) {
    let t = Tokens::get(ui.ctx());
    let Some(order) = read(&mut app.embroidery.order, generation, reply) else { return };
    ui.label(
        egui::RichText::new(crate::et!("Sewn from the bottom of the stack up; one thread's objects join when nothing lies between them."))
            .size(SMALL)
            .color(t.text_dim),
    );
    ui.add_space(4.0);
    let Some(st) = app.session.active() else { return };
    let (uid, revision, doc) = (st.uid, st.revision, st.doc.clone());
    let selected = st.selection.objects.clone();
    let covered = covered(&mut app.embroidery.order, uid, revision, &doc, &selected);
    let (rows, tops) = rows(&order, &app.embroidery.order.collapsed);
    let height = (ui.available_height() - FOOTER).max(HEADER_ROW * 2.0);
    let mut clicked: Option<(Row, egui::Modifiers)> = None;
    let mut toggle: Option<usize> = None;
    egui::ScrollArea::vertical().id_salt("embroidery-order").max_height(height).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let total = tops.last().copied().unwrap_or(0.0);
        ui.set_height(total);
        let width = ui.available_width();
        let origin = ui.min_rect().min;
        let first = tops.partition_point(|y| *y < viewport.min.y).saturating_sub(1);
        for (i, row) in rows.iter().enumerate().skip(first) {
            let Some(&top) = tops.get(i) else { break };
            if top > viewport.max.y {
                break;
            }
            let rect = Rect::from_min_size(origin + vec2(0.0, top), vec2(width, row.height()));
            let id = ui.id().with(("embroidery-row", i));
            let resp = ui.interact(rect, id, Sense::click());
            let lit = row_selected(&order, *row, &covered);
            let label = paint_row(app, ui, &doc, &order, *row, rect, lit, resp.hovered(), stale, &t);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, lit, &label));
            ui.ctx().accesskit_node_builder(id, |node| {
                node.set_role(egui::accesskit::Role::TreeItem);
                node.set_selected(lit);
                node.set_label(label.clone());
            });
            if resp.clicked() {
                if let Row::Header(b) = row
                    && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < rect.left() + INDENT)
                {
                    toggle = order.blocks.get(*b).map(|b| b.block);
                } else {
                    clicked = Some((*row, ui.input(|i| i.modifiers)));
                }
            }
        }
    });
    if let Some(block) = toggle
        && !app.embroidery.order.collapsed.remove(&block)
    {
        app.embroidery.order.collapsed.insert(block);
    }
    if let Some((row, mods)) = clicked {
        select(app, &order, &rows, row, mods);
    }
    if let Some(stats) = stats {
        footer(ui, stats, &t);
    }
}

/// Whether `row` is of the selection: a unit whose object is covered, a header all of whose
/// units are.
fn row_selected(order: &SewingOrder, row: Row, covered: &HashSet<u64>) -> bool {
    match row {
        Row::Unit(b, u) => order.blocks.get(b).and_then(|b| b.units.get(u)).is_some_and(|u| covered.contains(&u.id) || covered.contains(&u.node)),
        Row::Header(b) => {
            order.blocks.get(b).is_some_and(|b| !b.units.is_empty() && b.units.iter().all(|u| covered.contains(&u.id) || covered.contains(&u.node)))
        }
        Row::Change(_) => false,
    }
}

/// The objects `row` stands for: a unit's object, a header's block's objects (in order, once).
fn objects_of(order: &SewingOrder, row: Row) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    let mut add = |id: u64| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    match row {
        Row::Unit(b, u) => {
            if let Some(u) = order.blocks.get(b).and_then(|b| b.units.get(u)) {
                add(u.id);
            }
        }
        Row::Header(b) => order.blocks.get(b).into_iter().flat_map(|b| &b.units).for_each(|u| add(u.id)),
        Row::Change(_) => {}
    }
    ids
}

/// A click on `row` with `mods`: select its objects; with Shift, the unit rows from the one
/// clicked last; with Cmd/Ctrl, add or remove them.
fn select(app: &mut VectorcraftApp, order: &SewingOrder, rows: &[Row], row: Row, mods: egui::Modifiers) {
    let anchor = app.embroidery.order.anchor;
    let ids: Vec<u64> = match (row, anchor) {
        (Row::Unit(b, u), Some(a)) if mods.shift => {
            let at = |r: (usize, usize)| rows.iter().position(|x| *x == Row::Unit(r.0, r.1));
            let (Some(from), Some(to)) = (at(a), at((b, u))) else { return };
            let (lo, hi) = (from.min(to), from.max(to));
            let mut ids = Vec::new();
            for r in rows.get(lo..=hi).unwrap_or_default() {
                for id in objects_of(order, *r) {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            ids
        }
        _ => objects_of(order, row),
    };
    if let Row::Unit(b, u) = row
        && !mods.shift
    {
        app.embroidery.order.anchor = Some((b, u));
    }
    if ids.is_empty() {
        return;
    }
    let command = if mods.command { "select.toggle" } else { "select.set" };
    if let Err(e) = app.run(command, json!({ "ids": ids })) {
        app.status(e);
    }
}

/// Draw `row` in `rect` → what a screen reader says for it.
#[allow(clippy::too_many_arguments)]
fn paint_row(
    app: &VectorcraftApp,
    ui: &Ui,
    doc: &Document,
    order: &SewingOrder,
    row: Row,
    rect: Rect,
    selected: bool,
    hovered: bool,
    stale: bool,
    t: &Tokens,
) -> String {
    let p = ui.painter_at(rect);
    let lang = crate::i18n::current();
    let ink = |c: Color32| if stale { c.gamma_multiply(0.6) } else { c };
    if selected {
        p.rect_filled(rect, 0.0, t.row_selected);
    } else if hovered {
        p.rect_filled(rect, 0.0, t.hover);
    }
    match row {
        Row::Header(b) => {
            let Some(block) = order.blocks.get(b) else { return String::new() };
            let collapsed = app.embroidery.order.collapsed.contains(&block.block);
            let mid = rect.center().y;
            let arrow = Rect::from_center_size(pos2(rect.left() + 9.0, mid), vec2(12.0, 12.0));
            crate::icons::paint(ui, if collapsed { "chevron-right" } else { "chevron-down" }, arrow, ink(t.text_dim));
            let number = numbers::count(lang, block.block as u64);
            let mut x = rect.left() + INDENT;
            let g = p.text(pos2(x, mid), Align2::LEFT_CENTER, &number, FontId::proportional(SMALL), ink(t.text_dim));
            x = g.right() + 6.0;
            chip(&p, pos2(x, mid), block, t);
            x += CHIP + 6.0;
            let name = format!("{}  {} {}", block.thread.name, block.thread.palette.to_uppercase(), block.thread.index);
            let g = p.text(pos2(x, mid), Align2::LEFT_CENTER, &name, FontId::proportional(TEXT), ink(t.text_strong));
            x = g.right() + 6.0;
            if block.thread.mismatch {
                let badge = format!("ΔE {}", numbers::decimal(lang, block.thread.delta_e, 1));
                let g = p.text(pos2(x + 4.0, mid), Align2::LEFT_CENTER, &badge, FontId::proportional(SMALL), ink(t.warning));
                p.rect_stroke(g.expand2(vec2(3.0, 1.0)), 3.0, Stroke::new(1.0, ink(t.warning)), StrokeKind::Outside);
            }
            let objects = objects_of(order, row).len() as u64;
            let right = format!("{} · {}", crate::etn!(objects, "{count} object", "{count} objects"), numbers::count(lang, block.stitches as u64));
            p.text(pos2(rect.right() - 6.0, mid), Align2::RIGHT_CENTER, &right, FontId::proportional(SMALL), ink(t.text_dim));
            format!("{number} {name} {right}")
        }
        Row::Unit(b, u) => {
            let (Some(block), Some(unit)) = (order.blocks.get(b), order.blocks.get(b).and_then(|b| b.units.get(u))) else { return String::new() };
            let name = doc.node(NodeId(unit.id)).map_or_else(|| format!("#{}", unit.id), |n| crate::panels::layers::shown_name(app, n, lang));
            let x = rect.left() + INDENT;
            let top = rect.top() + UNIT_ROW * 0.3;
            let bottom = rect.top() + UNIT_ROW * 0.72;
            // Rows of one object that follow each other show its name once.
            let same = u > 0 && block.units.get(u - 1).is_some_and(|before| before.id == unit.id);
            if !same {
                p.text(pos2(x, top), Align2::LEFT_CENTER, &name, FontId::proportional(TEXT), ink(t.text));
            }
            let stitches = numbers::count(lang, unit.stitches as u64);
            p.text(pos2(rect.right() - 6.0, top), Align2::RIGHT_CENTER, &stitches, FontId::proportional(SMALL), ink(t.text));
            let how = how_sewn(block, unit);
            p.text(pos2(x, bottom), Align2::LEFT_CENTER, &how, FontId::proportional(SMALL), ink(t.text_dim));
            format!("{name} {how} {stitches}")
        }
        Row::Change(b) => {
            let trim = order.blocks.get(b).and_then(|b| b.then).is_some_and(|t| t.trim);
            let mid = rect.center().y;
            let icon = Rect::from_center_size(pos2(rect.left() + INDENT + 6.0, mid), vec2(12.0, 12.0));
            crate::icons::paint(ui, "scissors", icon, ink(t.text_dim));
            let words = if trim { crate::et!("Trim and thread change") } else { crate::et!("Thread change") };
            p.text(pos2(icon.right() + 6.0, mid), Align2::LEFT_CENTER, words, FontId::proportional(SMALL), ink(t.text_dim));
            words.to_string()
        }
    }
}

/// A block's thread chip: the thread's colour, with the art's colour in its lower right half
/// when they differ.
fn chip(p: &egui::Painter, left_mid: egui::Pos2, block: &OrderBlock, t: &Tokens) {
    let rect = Rect::from_min_size(pos2(left_mid.x, left_mid.y - CHIP / 2.0), vec2(CHIP, CHIP));
    let thread = hex(&block.thread.color).unwrap_or(t.text);
    p.rect_filled(rect, 2.0, thread);
    if block.thread.delta_e > 0.0
        && let Some(art) = hex(&block.thread.art)
    {
        p.add(egui::Shape::convex_polygon(vec![rect.right_top(), rect.right_bottom(), rect.left_bottom()], art, Stroke::NONE));
    }
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
}

/// `#rrggbb` as a colour.
fn hex(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#')?;
    let v = u32::from_str_radix(s, 16).ok().filter(|_| s.len() == 6)?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// A unit's second line: what part it sews (its number when the object has more than one of that
/// part in the block), how, and its jumps and trims.
fn how_sewn(block: &OrderBlock, unit: &OrderUnit) -> String {
    let several = block.units.iter().filter(|u| u.id == unit.id && u.part == unit.part).count() > 1;
    let part = match unit.part.as_str() {
        "fill" => crate::et!("fill"),
        "stroke" => crate::et!("stroke"),
        _ => crate::et!("from the file"),
    };
    let part = match unit.part_index.filter(|_| several) {
        Some(k) => format!("{part} {k}"),
        None => part.to_string(),
    };
    let mut words = vec![part];
    let sewn = match unit.sewn.as_str() {
        "running" => Some(crate::et!("running stitch")),
        "satin" => Some(crate::et!("satin")),
        "tatami" => Some(crate::et!("tatami")),
        _ => None,
    };
    words.extend(sewn.map(str::to_string));
    if unit.columns {
        words.push(crate::et!("narrow parts as satin").to_string());
    }
    if unit.jumps > 0 {
        words.push(crate::etn!(unit.jumps as u64, "{count} jump", "{count} jumps"));
    }
    if unit.trims > 0 {
        words.push(crate::etn!(unit.trims as u64, "{count} trim", "{count} trims"));
    }
    words.join(" · ")
}

/// The design's figures under the list: stitches, colours, thread changes, trims, jumps, the top
/// thread, the size and the time (the last two estimates, said so), and the speed the time is at.
fn footer(ui: &mut Ui, stats: &Value, t: &Tokens) {
    let lang = crate::i18n::current();
    let count = |k: &str| numbers::count(lang, stats[k].as_u64().unwrap_or(0));
    let about = |v: String| crate::embroidery::text::fill(crate::et!("about {value}"), &[("value", &v)]);
    let metres = about(format!("{} m", numbers::decimal(lang, stats["threadUsage"]["topMetres"].as_f64().unwrap_or(0.0), 0)));
    let minutes = about(format!("{} min", numbers::decimal(lang, stats["time"]["minutes"].as_f64().unwrap_or(0.0).ceil(), 0)));
    let size = match (stats["size"]["width"].as_f64(), stats["size"]["height"].as_f64()) {
        (Some(w), Some(h)) => format!("{} × {} mm", numbers::decimal(lang, w, 1), numbers::decimal(lang, h, 1)),
        _ => numbers::NOT_A_NUMBER.to_string(),
    };
    let speed = numbers::count(lang, stats["time"]["speed"].as_f64().unwrap_or(0.0).max(0.0) as u64);
    let cells = [
        (crate::et!("Stitches"), count("stitches")),
        (crate::et!("Colours"), count("threadCount")),
        (crate::et!("Thread changes"), count("colorChanges")),
        (crate::et!("Trims"), count("trims")),
        (crate::et!("Jumps"), count("jumps")),
        (crate::et!("Top thread"), metres),
        (crate::et!("Size"), size),
        (crate::et!("Time"), minutes),
    ];
    egui::Frame::NONE.fill(t.panel_darker).corner_radius(t.radius).inner_margin(8.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(crate::et!("Design")).size(TEXT).color(t.text_strong));
        // As many columns as fit, three at most (the mockups' wide panel); words never wrap.
        let columns = ((ui.available_width() / FIGURE_WIDTH).floor() as usize).clamp(1, 3);
        egui::Grid::new("embroidery-figures").num_columns(columns).spacing(vec2(12.0, 2.0)).show(ui, |ui| {
            for (k, (label, value)) in cells.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(*label).size(SMALL - 1.0).color(t.text_dim)).extend());
                    ui.add(egui::Label::new(egui::RichText::new(value).size(TEXT).color(t.text)).extend());
                });
                if k % columns == columns - 1 {
                    ui.end_row();
                }
            }
        });
        let note = crate::embroidery::text::fill(crate::et!("The time at {speed} stitches a minute, an estimate."), &[("speed", &speed)]);
        ui.label(egui::RichText::new(note).size(SMALL - 1.0).color(t.text_dim));
    });
}

#[cfg(test)]
mod tests;
