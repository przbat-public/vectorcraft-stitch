//! The Embroidery panel's Stitches tab (VectorCraft Stitch's manual stitches): the design's records
//! as the Sewing order and the Check tab number them, read from the plan the canvas shows, never by
//! asking a command per scroll position.
//!
//! - **Scope** (the project owner's choice, 2026-10-27): Selection (the selected objects, while
//!   stitches are edited the object edited) or the whole design. A click in the list never changes
//!   which rows it shows.
//! - **Filters with counts:** all, jumps, cuts, stops, and the stitches too short or too long by the
//!   one set of limits the canvas marks ([`stitch_limits`]). Each record's kind and judgement is one
//!   byte built with the plan on the planning thread ([`STITCH_FLAGS`]); the rows a scope and filter
//!   list are worked out once per (plan, scope, filter, limits), never per frame ([`Listed`]).
//! - **Columns:** the record's number, its kind, X and Y in millimetres from the rulers' zero point
//!   (the project owner's choice), and the thread its stitch lays; a stitch past the limits carries
//!   the Check tab's warning mark. Rows of [`ROW_HEIGHT`] in a virtual list, only those in view
//!   drawn; past [`MAX_SCROLLED_ROWS`] the list is split into pages, as egui places rows in `f32`.
//! - **A click:** outside stitch editing it shows the stitch (a mark on the canvas, the view panned
//!   to it when it is out of view: the project owner's choice); while stitches are edited it selects
//!   the needle point (Shift a range, Cmd or Ctrl one more), the list lit from the document's
//!   selection. A double-click or Enter zooms to it, as the Check tab's Show does.
//! - **Go to #**, and at the foot, while editing: the selection, Move by X and Y (the project
//!   owner's choice) and the stitch actions; outside it Edit Stitches, or for planned stitches,
//!   read only, Convert to Manual Stitches.
//! - **Keyboard and screen readers:** the list is a grid of rows with their count and index; ↑ ↓,
//!   Page Up/Down, Home and End move the row in focus, Space selects it, Enter shows it.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::cmd::embroidery::plan::DesignPlan;
use vectorcraft_engine::cmd::embroidery::stitch_list::{self, Filter, Flags, RecordKind};
use vectorcraft_engine::cmd::embroidery::{StitchLimits, mm_to_pt, stitch_limits};
use vectorcraft_geom::Point;

use crate::VectorcraftApp;
use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::plan_worker::{Query, STITCH_FLAGS};
use crate::i18n::Lang;
use crate::theme::Tokens;

/// A row's height, points: the mockup's row pitch, a whole number so rows start on whole points.
pub const ROW_HEIGHT: f32 = 20.0;
/// The most rows one page scrolls through: 400,000 rows of [`ROW_HEIGHT`] reach 8,000,000 points,
/// under 2²³, where `f32` still places a row to half a point (egui lays rows out in `f32`).
pub const MAX_SCROLLED_ROWS: usize = 400_000;
/// The decimals of X and Y (a tenth of a millimetre, the formats' grid) and of a length (a
/// hundredth, as the commands give lengths).
const PLACE_DECIMALS: usize = 1;
const LENGTH_DECIMALS: usize = 2;
/// The words' size, a column's room either side, the thread chip's size and the marks' size,
/// points.
const TEXT: f32 = 12.0;
const CELL_PAD: f32 = 4.0;
const CHIP: f32 = 8.0;
const MARK: f32 = 11.0;
/// The list's least height, points: a few rows, whatever the panel's room.
const MIN_LIST: f32 = 6.0 * ROW_HEIGHT;
/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// The most needle points a Shift-click's range selects at once: the stitch commands' bound on a
/// call's addresses is far above, this keeps one click's params small.
const RANGE_MAX: usize = 100_000;

/// Where the list's rows come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The selected objects (while stitches are edited: the object edited).
    #[default]
    Selection,
    /// The whole design.
    Design,
}

impl Scope {
    pub fn id(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::Design => "design",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Selection, Self::Design].into_iter().find(|s| s.id() == id)
    }
}

/// The rows a scope and a filter list: record numbers in sewing order, as ranges (every record of
/// them) or picked one by one; with each filter's count in the scope.
pub struct Listed {
    rows: Rows,
    pub counts: [usize; Filter::ALL.len()],
    /// The scope's first and last record numbers.
    pub span: Option<(usize, usize)>,
}

enum Rows {
    /// Every record of the ranges, and how many come before each range.
    Ranges(Vec<Range<usize>>, Vec<usize>),
    Picked(Vec<u32>),
}

impl Listed {
    /// How many rows.
    pub fn len(&self) -> usize {
        match &self.rows {
            Rows::Ranges(r, before) => before.last().copied().unwrap_or(0) + r.last().map_or(0, |l| l.len()),
            Rows::Picked(p) => p.len(),
        }
    }

    /// Whether it lists no row.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Row `i`'s record number.
    pub fn record(&self, i: usize) -> Option<usize> {
        match &self.rows {
            Rows::Ranges(ranges, before) => {
                let k = before.partition_point(|b| *b <= i).checked_sub(1)?;
                let r = ranges.get(k)?;
                let n = r.start + (i - before.get(k)?);
                (n < r.end).then_some(n)
            }
            Rows::Picked(p) => p.get(i).map(|n| *n as usize),
        }
    }

    /// The row of record `number`, when it is listed.
    pub fn row_of(&self, number: usize) -> Option<usize> {
        match &self.rows {
            Rows::Ranges(ranges, before) => {
                let k = ranges.partition_point(|r| r.end <= number);
                let r = ranges.get(k).filter(|r| r.contains(&number))?;
                Some(before.get(k)? + (number - r.start))
            }
            Rows::Picked(p) => {
                let n = u32::try_from(number).ok()?;
                p.binary_search(&n).ok()
            }
        }
    }
}

/// The rows of `ranges` (record numbers; `None`: every record of `flags`) under `filter`, and every
/// filter's count, in one pass over the scope's records.
pub fn listed(flags: &Flags, ranges: Option<&[Range<usize>]>, filter: Filter) -> Listed {
    let whole = 1..flags.len() + 1;
    let ranges: &[Range<usize>] = ranges.unwrap_or(std::slice::from_ref(&whole));
    let mut counts = [0usize; Filter::ALL.len()];
    let mut picked: Vec<u32> = Vec::new();
    for r in ranges {
        for n in r.clone() {
            for (slot, f) in counts.iter_mut().zip(Filter::ALL) {
                if flags.matches(n, f) {
                    *slot += 1;
                }
            }
            if filter != Filter::All
                && flags.matches(n, filter)
                && let Ok(n) = u32::try_from(n)
            {
                picked.push(n);
            }
        }
    }
    let span = ranges.first().zip(ranges.last()).and_then(|(a, b)| (b.end > a.start).then(|| (a.start, b.end - 1)));
    let rows = if filter == Filter::All {
        let mut before = Vec::with_capacity(ranges.len());
        let mut sum = 0;
        for r in ranges {
            before.push(sum);
            sum += r.len();
        }
        Rows::Ranges(ranges.to_vec(), before)
    } else {
        Rows::Picked(picked)
    };
    Listed { rows, counts, span }
}

/// What [`Listed`] is of: the document, the plan's generation, the scope's objects (`None`: the
/// whole design), the filter and the limits' bits.
type ListKey = (u64, u64, Option<Vec<NodeId>>, Filter, [u64; 3]);

/// One document's place in the list: the record in focus and the one a Shift-click's range starts
/// from, the page shown, the stitch shown on the canvas, Move by's two fields (millimetres), Go
/// to's words and what it last said.
#[derive(Default)]
pub struct DocList {
    pub focus: Option<usize>,
    anchor: Option<usize>,
    page: usize,
    pub shown: Option<usize>,
    move_by: [f64; 2],
    goto: String,
    said: Option<String>,
    /// Scroll to the focus in the next frame.
    reveal: bool,
}

/// The tab's state: each document's place, the rows listed (built once a key, [`ListKey`]), and
/// each unit's numbering for the plan shown (worked out once a unit, for the rows in view).
#[derive(Default)]
pub struct StitchesState {
    pub(crate) docs: HashMap<u64, DocList>,
    list: Option<(ListKey, Arc<Listed>)>,
    numbering: Option<(u64, u64, HashMap<usize, Option<Numbered>>)>,
    /// How many times the rows were worked out (the tests count them: never on an idle frame).
    pub(crate) builds: usize,
    /// The foot's height as last drawn, points, outside stitch editing and while editing (0: not
    /// drawn yet): the room kept for it under the list.
    foot: [f32; 2],
}

/// A unit's numbering: the path, its first needle point's place, and each record's ordinal.
#[derive(Clone)]
struct Numbered {
    node: NodeId,
    first: usize,
    ordinals: Arc<Vec<u32>>,
}

/// The limits the tab, the canvas and the Check tab's format judge by, for the active document.
pub fn limits_of(app: &VectorcraftApp) -> Option<StitchLimits> {
    let st = app.session.active()?;
    Some(stitch_limits(&st.doc, app.embroidery.check.format.get(&st.uid).copied()))
}

/// What the tab asks the planning thread for besides the figures: its records' flags for the limits.
pub fn query(app: &VectorcraftApp) -> Option<Query> {
    limits_of(app).map(|l| Query { cmd: STITCH_FLAGS, params: stitch_list::limits_params(&l) })
}

/// The objects the Selection scope lists: while stitches are edited the object edited, else the
/// selected objects.
fn scope_ids(app: &mut VectorcraftApp) -> Vec<NodeId> {
    if let Some(e) = crate::embroidery::editing::editing(app) {
        return vec![e.scope.object];
    }
    app.session.active().map(|st| st.selection.objects.clone()).unwrap_or_default()
}

/// The rows listed for document `uid`'s plan `plan` of generation `generation`, worked out again
/// only when the key changed.
fn rows(app: &mut VectorcraftApp, plan: &DesignPlan, generation: u64, flags: &Arc<Flags>) -> Arc<Listed> {
    let scope = app.ui.embroidery_panel.stitches_scope;
    let filter = app.ui.embroidery_panel.stitches_filter;
    let ids = (scope == Scope::Selection).then(|| scope_ids(app));
    let Some(st) = app.session.active() else { return Arc::new(listed(flags, Some(&[]), filter)) };
    let key: ListKey = (st.uid, generation, ids.clone(), filter, flags.limits());
    if let Some((k, l)) = &app.embroidery.stitches.list
        && *k == key
    {
        return l.clone();
    }
    let ranges = ids.map(|ids| stitch_list::scope(plan, &st.doc, &ids));
    let made = Arc::new(listed(flags, ranges.as_deref(), filter));
    app.embroidery.stitches.list = Some((key, made.clone()));
    app.embroidery.stitches.builds += 1;
    made
}

/// The needle point record `number` (of `kind`, in unit `unit`) is edited by, from the unit's
/// numbering, worked out once a unit for the plan of `generation`.
fn edited_by(app: &mut VectorcraftApp, plan: &DesignPlan, generation: u64, unit: usize, number: usize, kind: RecordKind) -> Option<(NodeId, usize)> {
    let st = app.session.active()?;
    let (uid, doc) = (st.uid, st.doc.clone());
    let kept = &mut app.embroidery.stitches.numbering;
    if kept.as_ref().is_none_or(|(u, g, _)| *u != uid || *g != generation) {
        *kept = Some((uid, generation, HashMap::new()));
    }
    let (_, _, units) = kept.as_mut()?;
    let n = units
        .entry(unit)
        .or_insert_with(|| {
            stitch_list::numbering(plan, &doc, unit).map(|(node, first)| Numbered {
                node,
                first,
                ordinals: Arc::new(stitch_list::ordinals(plan, unit)),
            })
        })
        .clone()?;
    stitch_list::edited_by(plan, unit, n.first, &n.ordinals, number, kind).map(|k| (n.node, k))
}

/// Whether needle point `index` of path `node` is selected in the active document.
fn is_selected(app: &VectorcraftApp, node: NodeId, index: usize) -> bool {
    let Some(st) = app.session.active() else { return false };
    let Some(n) = st.doc.node(node) else { return false };
    let Some(path) = n.path_data() else { return false };
    let tack = path.subpaths.len() == 1 && n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|s| s.double_point);
    let mut first = 0usize;
    for (s, sp) in path.subpaths.iter().enumerate() {
        let len = vectorcraft_engine::cmd::embroidery::needle_count(&sp.anchors, tack);
        if index < first + len {
            return match st.selection.anchors.get(&node) {
                Some(anchors) => anchors.contains(&(s, index - first)) || (len == 1 && anchors.contains(&(s, 1))),
                None => st.selection.objects.contains(&node),
            };
        }
        first += len;
    }
    false
}

/// A kind's word.
fn kind_word(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Stitch => crate::et!("stitch"),
        RecordKind::Jump => crate::et!("jump"),
        RecordKind::Trim => crate::et!("trim"),
        RecordKind::Stop => crate::et!("stop"),
    }
}

/// A filter's chip words, with its count.
fn chip_words(filter: Filter, count: usize, limits: &StitchLimits, lang: Lang) -> (String, String) {
    let n = numbers::count(lang, count as u64);
    let (word, full) = match filter {
        Filter::All => (crate::et!("All").to_string(), None),
        Filter::Jumps => (crate::et!("Jumps").to_string(), None),
        Filter::Trims => (crate::et!("Trims").to_string(), None),
        Filter::Stops => (crate::et!("Stops").to_string(), None),
        Filter::Short => (
            crate::et!("Too short").to_string(),
            Some(crate::embroidery::text::fill(
                crate::et!("Shorter than {value} mm"),
                &[("value", &numbers::decimal(lang, limits.shortest, LENGTH_DECIMALS))],
            )),
        ),
        Filter::Long => (
            crate::et!("Too long").to_string(),
            Some(crate::embroidery::text::fill(
                crate::et!("Longer than {value} mm"),
                &[("value", &numbers::decimal(lang, limits.longest.min(limits.step), LENGTH_DECIMALS))],
            )),
        ),
    };
    let shown = crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &word), ("count", &n)]);
    let named = match full {
        Some(f) => crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &f), ("count", &n)]),
        None => shown.clone(),
    };
    (shown, named)
}

/// What the tab is drawn from: the plan shown (its generation, and whether it is of the document
/// as it is) and the records' flags when the planning thread has built them.
pub struct Shown {
    pub plan: Arc<DesignPlan>,
    pub generation: u64,
    pub stale: bool,
    pub flags: Option<Arc<Flags>>,
}

/// Draw the tab.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, shown: Shown) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some(limits) = limits_of(app) else { return };
    // The scope: Selection | Whole design.
    let scopes = [(Scope::Selection, crate::et!("Selection")), (Scope::Design, crate::et!("Whole design"))];
    let items: Vec<super::Segment<'_>> = scopes
        .iter()
        .map(|(s, label)| super::Segment {
            id: ui.id().with(("embroidery-stitches-scope", s.id())),
            label,
            full: label,
            selected: app.ui.embroidery_panel.stitches_scope == *s,
            enabled: true,
        })
        .collect();
    if let Some((s, _)) = super::segments(ui, ROW_HEIGHT, TEXT, &items).and_then(|i| scopes.get(i)) {
        app.ui.embroidery_panel.stitches_scope = *s;
    }
    ui.add_space(CELL_PAD);
    let Some(flags) = shown.flags.clone() else {
        ui.label(egui::RichText::new(crate::et!("Listing stitches…")).size(TEXT).color(t.text_dim));
        return;
    };
    let listed = rows(app, &shown.plan, shown.generation, &flags);
    if app.ui.embroidery_panel.stitches_scope == Scope::Selection && listed.span.is_none() {
        let words = if scope_ids(app).is_empty() {
            crate::et!("Select objects to list their stitches.")
        } else {
            crate::et!("The selection sews no stitches.")
        };
        ui.label(egui::RichText::new(words).size(TEXT).color(t.text));
        return;
    }
    // The filters, each with its count.
    let filter = app.ui.embroidery_panel.stitches_filter;
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        for (k, f) in Filter::ALL.into_iter().enumerate() {
            let count = listed.counts.get(k).copied().unwrap_or(0);
            let (words, named) = chip_words(f, count, &limits, lang);
            let text = egui::RichText::new(words).size(TEXT).color(if count == 0 && f != filter { t.text_dim } else { t.text });
            let resp = ui.selectable_label(f == filter, text);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, f == filter, &named));
            if resp.on_hover_text(&named).clicked() {
                chosen = Some(f);
            }
        }
    });
    if let Some(f) = chosen {
        app.ui.embroidery_panel.stitches_filter = f;
        if let Some(d) = app.embroidery.stitches.docs.get_mut(&uid) {
            d.page = 0;
        }
        return;
    }
    go_to(app, ui, uid, &listed);
    let editing = crate::embroidery::editing::editing(app);
    let total = listed.len();
    let pages = total.div_ceil(MAX_SCROLLED_ROWS).max(1);
    let doc_list = app.embroidery.stitches.docs.entry(uid).or_default();
    doc_list.page = doc_list.page.min(pages - 1);
    let page = doc_list.page;
    if pages > 1 {
        ui.horizontal(|ui| {
            let first = page * MAX_SCROLLED_ROWS + 1;
            let last = ((page + 1) * MAX_SCROLLED_ROWS).min(total);
            let words = crate::embroidery::text::fill(
                crate::et!("Rows {first}–{last} of {total}"),
                &[
                    ("first", &numbers::count(lang, first as u64)),
                    ("last", &numbers::count(lang, last as u64)),
                    ("total", &numbers::count(lang, total as u64)),
                ],
            );
            if ui.add_enabled(page > 0, egui::Button::new("‹")).on_hover_text(crate::et!("Previous rows")).clicked()
                && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
            {
                d.page = page - 1;
            }
            ui.label(egui::RichText::new(words).size(TEXT));
            if ui.add_enabled(page + 1 < pages, egui::Button::new("›")).on_hover_text(crate::et!("Next rows")).clicked()
                && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
            {
                d.page = page + 1;
            }
        });
    }
    // The column heads, then the rows in view.
    let width = ui.available_width();
    let columns = Columns::fit(ui, width);
    header(ui, &columns, &t);
    let in_page = (total - (page * MAX_SCROLLED_ROWS).min(total)).min(MAX_SCROLLED_ROWS);
    let drawn = app.embroidery.stitches.foot[usize::from(editing.is_some())];
    let foot = if drawn > 0.0 { drawn } else { foot_height(editing.is_some()) };
    // Whole rows: none cut by the foot's words under the list.
    let room = (((ui.available_height() - foot) / ROW_HEIGHT).floor() * ROW_HEIGHT).max(MIN_LIST);
    let list_id = ui.id().with("embroidery-stitch-list");
    let mut clicked: Option<(usize, egui::Modifiers, bool)> = None;
    let focus = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.focus);
    let reveal = app.embroidery.stitches.docs.get_mut(&uid).is_some_and(|d| std::mem::take(&mut d.reveal));
    let mut area = egui::ScrollArea::vertical().id_salt(list_id).max_height(room).auto_shrink([false, false]);
    if reveal && let Some(row) = focus.and_then(|f| listed.row_of(f)).filter(|r| r / MAX_SCROLLED_ROWS == page) {
        area = area.vertical_scroll_offset(((row % MAX_SCROLLED_ROWS) as f32 * ROW_HEIGHT - room / 2.0).max(0.0));
    }
    let grid = list_id.with("grid");
    ui.ctx().accesskit_node_builder(grid, |node| {
        node.set_role(egui::accesskit::Role::Grid);
        node.set_label(crate::et!("Stitches"));
        node.set_row_count(total);
        node.set_column_count(columns.count());
    });
    // No spacing between rows, in the list as in egui's sums: it places a row and works out the
    // rows in view from its height plus the spacing of the ui it is given.
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let scrolled = area.show_rows(ui, ROW_HEIGHT, in_page, |ui, range| {
        for i in range {
            let row_index = page * MAX_SCROLLED_ROWS + i;
            let Some(number) = listed.record(row_index) else { continue };
            let Some(row) = stitch_list::row(&shown.plan, number) else { continue };
            let address = editing.and_then(|_| edited_by(app, &shown.plan, shown.generation, row.unit, number, row.kind));
            let selected = address.is_some_and(|(node, k)| is_selected(app, node, k));
            let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
            let words = draw_row(app, ui, rect, &row, &columns, &flags, selected, focus == Some(number), lang, &t);
            ui.ctx().accesskit_node_builder(resp.id, |node| {
                node.set_role(egui::accesskit::Role::Row);
                node.set_row_index(row_index);
                node.set_selected(selected);
                node.set_label(words.clone());
            });
            if resp.double_clicked() {
                clicked = Some((number, ui.input(|i| i.modifiers), true));
            } else if resp.clicked() {
                clicked = Some((number, ui.input(|i| i.modifiers), false));
            }
        }
    });
    // The list itself takes the keyboard's focus (a row clicked gives it), so its keys are its own.
    ui.spacing_mut().item_spacing.y = spacing;
    let list = ui.interact(scrolled.inner_rect, list_id, Sense::focusable_noninteractive());
    if clicked.is_some() {
        list.request_focus();
    }
    // Its arrows and Escape locked to it: egui would move the focus to the next widget on an arrow
    // and drop it on Escape before the canvas's keys run (which then see no focus, and act).
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(list_id, egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    keys(app, ui, list_id, uid, &listed, &shown);
    if let Some((number, mods, double)) = clicked {
        click(app, &shown, number, mods, double);
    }
    let top = ui.cursor().top();
    foot_ui(app, ui, editing, &listed, lang, &t);
    // The room the next frame keeps for it; a frame at once when it changed (the list drawn this
    // frame had the old room), so the list never lies over the foot past one frame.
    let used = ui.cursor().top() - top;
    if let Some(kept) = app.embroidery.stitches.foot.get_mut(usize::from(editing.is_some()))
        && (*kept - used).abs() > FOOT_SETTLED
    {
        *kept = used;
        ui.ctx().request_repaint();
    }
}

/// The columns drawn at a width: every column with words where they fit (the kinds' words in the
/// language drawn), else the kind as a glyph, else without X and Y (in the row's name for screen
/// readers). A head wider than its column is cut short with "…", whole for screen readers.
struct Columns {
    xy: bool,
    words: bool,
    widths: [f32; 5],
}

/// The column widths, points, of the widest layout: the chip and the number, the kind, X, Y and
/// the length, then a glyph's kind; estimates the language tests check at 220, 256 and 300 pt.
const NUMBER_W: f32 = 72.0;
const KIND_W: f32 = 64.0;
const KIND_GLYPH_W: f32 = 20.0;
const PLACE_W: f32 = 50.0;
const LENGTH_W: f32 = 48.0;

impl Columns {
    fn fit(ui: &Ui, width: f32) -> Self {
        let all = NUMBER_W + KIND_W + 2.0 * PLACE_W + LENGTH_W;
        let glyphs = NUMBER_W + KIND_GLYPH_W + 2.0 * PLACE_W + LENGTH_W;
        let font = egui::FontId::proportional(TEXT);
        let kinds = [RecordKind::Stitch, RecordKind::Jump, RecordKind::Trim, RecordKind::Stop];
        let widest =
            kinds.iter().map(|k| ui.painter().layout_no_wrap(kind_word(*k).to_string(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max);
        let mut c = if width >= all && widest + 2.0 * CELL_PAD <= KIND_W {
            Self { xy: true, words: true, widths: [NUMBER_W, KIND_W, PLACE_W, PLACE_W, LENGTH_W] }
        } else if width >= glyphs {
            Self { xy: true, words: false, widths: [NUMBER_W, KIND_GLYPH_W, PLACE_W, PLACE_W, LENGTH_W] }
        } else {
            Self { xy: false, words: false, widths: [NUMBER_W, KIND_GLYPH_W, 0.0, 0.0, LENGTH_W] }
        };
        // The room left over goes to the heads cut short, each column widened to its head in turn:
        // the length, X, Y, then the kind.
        let heads = heads();
        let mut spare = width - c.widths.iter().sum::<f32>();
        for k in [4, 2, 3, 1] {
            let (Some(head), Some(w)) = (heads.get(k), c.widths.get(k).copied()) else { continue };
            if spare <= 0.0 || w <= 0.0 || (k == 1 && !c.words) {
                continue;
            }
            let need = ui.painter().layout_no_wrap(head.to_string(), font.clone(), Color32::WHITE).size().x + 2.0 * CELL_PAD - w;
            if need > 0.0
                && let Some(slot) = c.widths.get_mut(k)
            {
                let grow = need.min(spare);
                *slot += grow;
                spare -= grow;
            }
        }
        c
    }

    fn count(&self) -> usize {
        if self.xy { 5 } else { 3 }
    }
}

/// The columns' heads: the number, the kind, X, Y and the length.
fn heads() -> [&'static str; 5] {
    ["#", crate::et!("Type"), crate::et!("X (mm)"), crate::et!("Y (mm)"), crate::et!("Length (mm)")]
}

/// The column heads.
fn header(ui: &mut Ui, columns: &Columns, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let heads = heads();
    let mut x = rect.left();
    for (k, (head, w)) in heads.iter().zip(columns.widths).enumerate() {
        if w <= 0.0 || (k == 1 && !columns.words) {
            x += w;
            continue;
        }
        let at = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, ROW_HEIGHT));
        let mut job = egui::text::LayoutJob::simple_singleline(head.to_string(), egui::FontId::proportional(TEXT), t.text_dim);
        job.wrap =
            egui::text::TextWrapping { max_width: (w - 2.0 * CELL_PAD).max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let galley = ui.painter().layout_job(job);
        let size = galley.size();
        ui.painter().galley(at.right_center() - vec2(CELL_PAD + size.x, size.y / 2.0), galley, t.text_dim);
        let id = ui.id().with(("embroidery-stitch-head", k));
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::ColumnHeader);
            node.set_label(*head);
        });
        x += w;
    }
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.divider));
}

/// Draw one row → its name for screen readers ("Record 4695: stitch, X 25.1 mm, Y 23.2 mm, length
/// 2.61 mm, too short").
#[allow(clippy::too_many_arguments)]
fn draw_row(
    app: &VectorcraftApp,
    ui: &Ui,
    rect: Rect,
    row: &stitch_list::Row,
    columns: &Columns,
    flags: &Flags,
    selected: bool,
    focused: bool,
    lang: Lang,
    t: &Tokens,
) -> String {
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::ZERO, t.row_selected);
    }
    let font = egui::FontId::proportional(TEXT);
    let ink = if selected { t.text_strong } else { t.text };
    let origin = app.ruler_origin();
    let x_mm = (mm_to_pt(row.at.x) - origin.x) / PT_PER_MM;
    let y_mm = (mm_to_pt(row.at.y) - origin.y) / PT_PER_MM;
    let [short, long] = flags.judged(row.record);
    let mut x = rect.left();
    // The thread chip and the number.
    let colour = app
        .embroidery
        .preview
        .shown(app.session.active().map_or(0, |st| st.uid))
        .and_then(|s| s.planned.plan.blocks.get(row.block).map(|b| b.thread.color()))
        .map_or(t.text_dim, |[r, g, b]| Color32::from_rgb(r, g, b));
    let chip = Rect::from_center_size(pos2(x + CELL_PAD + CHIP / 2.0, rect.center().y), vec2(CHIP, CHIP));
    p.rect(chip, CornerRadius::same(2), colour, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let number = numbers::count(lang, row.record as u64);
    p.text(pos2(x + columns.widths[0] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &number, font.clone(), ink);
    x += columns.widths[0];
    let kind = kind_word(row.kind);
    let dim = marks::readable_text(t.text_dim, &[if selected { t.row_selected } else { t.panel }], t.dark);
    if columns.words {
        let colour = if row.kind == RecordKind::Stitch { ink } else { dim };
        p.text(pos2(x + columns.widths[1] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, kind, font.clone(), colour);
    } else {
        let glyph = match row.kind {
            RecordKind::Stitch => "·",
            RecordKind::Jump => "⤳",
            RecordKind::Trim => "✂",
            RecordKind::Stop => "■",
        };
        p.text(pos2(x + columns.widths[1] / 2.0, rect.center().y), egui::Align2::CENTER_CENTER, glyph, font.clone(), dim);
    }
    x += columns.widths[1];
    let (xs, ys) = (numbers::fixed(lang, x_mm, PLACE_DECIMALS), numbers::fixed(lang, y_mm, PLACE_DECIMALS));
    if columns.xy {
        p.text(pos2(x + columns.widths[2] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &xs, font.clone(), ink);
        x += columns.widths[2];
        p.text(pos2(x + columns.widths[3] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &ys, font.clone(), ink);
        x += columns.widths[3];
    }
    let length = row.length.map(|l| numbers::fixed(lang, l, LENGTH_DECIMALS));
    let shown = length.clone().unwrap_or_else(|| "–".to_string());
    p.text(pos2(x + columns.widths[4] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &shown, font, ink);
    if short || long {
        marks::paint(p, pos2(x + MARK / 2.0, rect.center().y), MARK, Mark::Warning, t);
    }
    if focused {
        let ring = marks::readable(t.accent, &[if selected { t.row_selected } else { t.panel }], t.dark);
        p.rect_stroke(rect.shrink(1.0), CornerRadius::same(2), Stroke::new(2.0, ring), egui::StrokeKind::Inside);
        p.rect_stroke(rect.shrink(3.0), CornerRadius::same(2), Stroke::new(1.0, t.panel), egui::StrokeKind::Inside);
    }
    let mut name = crate::embroidery::text::fill(
        crate::et!("Record {number}: {type}, X {x} mm, Y {y} mm"),
        &[("number", &number), ("type", kind), ("x", &xs), ("y", &ys)],
    );
    if let Some(l) = length {
        name.push_str(&crate::embroidery::text::fill(crate::et!(", length {value} mm"), &[("value", &l)]));
    }
    if short {
        name.push_str(crate::et!(", too short"));
    }
    if long {
        name.push_str(crate::et!(", too long"));
    }
    name
}

/// A click on record `number`'s row: the row in focus; while stitches are edited its needle point
/// selected (Shift: the range from the last clicked; Cmd or Ctrl: one more or one less), else the
/// stitch shown on the canvas (a double-click: zoomed to).
fn click(app: &mut VectorcraftApp, shown: &Shown, number: usize, mods: egui::Modifiers, double: bool) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let anchor = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.anchor);
    {
        let d = app.embroidery.stitches.docs.entry(uid).or_default();
        d.focus = Some(number);
        if !mods.shift {
            d.anchor = Some(number);
        }
    }
    let editing = crate::embroidery::editing::editing(app).is_some();
    if !editing || double {
        show_stitch(app, shown, number, double);
        return;
    }
    let range = match (mods.shift, anchor) {
        (true, Some(a)) => a.min(number)..a.max(number) + 1,
        _ => number..number + 1,
    };
    let mut points: Vec<serde_json::Value> = Vec::new();
    for n in range.take(RANGE_MAX) {
        let Some(row) = stitch_list::row(&shown.plan, n) else { continue };
        if matches!(row.kind, RecordKind::Trim | RecordKind::Stop) {
            continue;
        }
        if let Some((node, k)) = edited_by(app, &shown.plan, shown.generation, row.unit, n, row.kind) {
            points.push(json!({"id": node.0, "index": k}));
        }
    }
    if points.is_empty() {
        app.status(crate::et!("This stitch can't be edited: it is planned from its object's shape."));
        return;
    }
    let mode = if mods.command || mods.ctrl { "toggle" } else { "set" };
    if let Err(e) = app.run("embroidery.selectNeedlePoints", json!({"needlePoints": points, "mode": mode})) {
        app.status(e);
    }
}

/// Record `number` shown on the canvas: marked, and the view panned to it when it lies out of view
/// (`zoom`: zoomed to it, as the Check tab's Show zooms, at least [`super::check::SHOW_MIN_BOX_MM`]
/// across).
fn show_stitch(app: &mut VectorcraftApp, shown: &Shown, number: usize, zoom: bool) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some(row) = stitch_list::row(&shown.plan, number) else { return };
    app.embroidery.stitches.docs.entry(uid).or_default().shown = Some(number);
    let at = Point::new(mm_to_pt(row.at.x), mm_to_pt(row.at.y));
    if zoom {
        let half = mm_to_pt(super::check::SHOW_MIN_BOX_MM) / 2.0;
        let rect = [at.x - half, at.y - half, at.x + half, at.y + half];
        if let Err(e) = app.run("view.zoomTo", json!({"rect": rect, "margin": mm_to_pt(super::check::SHOW_MARGIN_MM)})) {
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
/// a screenful, Home and End the first and last; Space as a click on it; Enter shows it, zoomed to.
/// Keys taken from egui, so neither the canvas's arrows nor upstream's shortcuts see them.
fn keys(app: &mut VectorcraftApp, ui: &Ui, list: egui::Id, uid: u64, listed: &Listed, shown: &Shown) {
    if !ui.memory(|m| m.has_focus(list)) {
        return;
    }
    let pressed = |key: egui::Key| ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
    // Escape first: the list keeps the focus otherwise (its keys are locked to it).
    if pressed(egui::Key::Escape) {
        ui.memory_mut(|m| m.surrender_focus(list));
        return;
    }
    let screenful = (ui.available_height() / ROW_HEIGHT).max(1.0) as usize;
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
    if pressed(egui::Key::Space) {
        click(app, shown, focus, egui::Modifiers::NONE, false);
        return;
    }
    let target = if pressed(egui::Key::ArrowDown) {
        Some((row + 1).min(last))
    } else if pressed(egui::Key::ArrowUp) {
        Some(row.saturating_sub(1))
    } else if pressed(egui::Key::PageDown) {
        Some((row + screenful).min(last))
    } else if pressed(egui::Key::PageUp) {
        Some(row.saturating_sub(screenful))
    } else if pressed(egui::Key::Home) {
        Some(0)
    } else if pressed(egui::Key::End) {
        Some(last)
    } else {
        None
    };
    if let Some(r) = target
        && let Some(n) = listed.record(r)
        && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
    {
        d.focus = Some(n);
        d.page = r / MAX_SCROLLED_ROWS;
        d.reveal = true;
    }
}

/// Go to #: a record's number or a range typed ("4695", "4695–4700"), in focus and shown (a range:
/// its first).
fn go_to(app: &mut VectorcraftApp, ui: &mut Ui, uid: u64, listed: &Listed) {
    let lang = crate::i18n::current();
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    let mut text = d.goto.clone();
    let said = d.said.clone();
    let mut go = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::et!("Go to #")).size(TEXT));
        let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
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
    let first = text.split(['–', '-']).next().map(str::trim).unwrap_or_default();
    let digits: String = first.chars().filter(char::is_ascii_digit).collect();
    let number = digits.parse::<u64>().ok().and_then(|n| usize::try_from(n).ok());
    match number.and_then(|n| listed.row_of(n).map(|r| (n, r))) {
        Some((n, r)) => {
            d.focus = Some(n);
            d.page = r / MAX_SCROLLED_ROWS;
            d.reveal = true;
            d.said = None;
        }
        None => {
            let (a, b) = listed.span.unwrap_or((0, 0));
            d.said = Some(crate::embroidery::text::fill(
                crate::et!("No record numbered {value} here: these are #{first}–{last}."),
                &[("value", first), ("first", &numbers::count(lang, a as u64)), ("last", &numbers::count(lang, b as u64))],
            ));
        }
    }
}

/// How much the foot's height may change, points, before the next frame lays the list out again:
/// less than a pixel.
const FOOT_SETTLED: f32 = 0.5;

/// The foot's height before it is first drawn, points: room kept under the list.
fn foot_height(editing: bool) -> f32 {
    if editing { 5.0 * ROW_HEIGHT } else { 3.0 * ROW_HEIGHT }
}

/// The foot: while stitches are edited, the selection, Move by X and Y with Apply, and the stitch
/// actions; else Edit Stitches for stitches that can be edited, or for planned stitches a line
/// saying they are planned and Convert to Manual Stitches.
fn foot_ui(
    app: &mut VectorcraftApp,
    ui: &mut Ui,
    editing: Option<vectorcraft_engine::cmd::embroidery::StitchEditing>,
    listed: &Listed,
    lang: Lang,
    t: &Tokens,
) {
    ui.add_space(CELL_PAD);
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    if let Some(e) = editing {
        if e.selected > 0 {
            ui.label(
                egui::RichText::new(crate::etn!(e.selected as u64, "{count} needle point selected", "{count} needle points selected")).size(TEXT),
            );
            let mut by = app.embroidery.stitches.docs.entry(uid).or_default().move_by;
            let mut apply = false;
            let format = crate::embroidery::fields::format(lang, "mm", PLACE_DECIMALS);
            let bounds = -MOVE_BY_MAX_MM..=MOVE_BY_MAX_MM;
            // "Move by" on a line of its own; under it the two fields, with Apply beside them where
            // it fits (the panel's narrowest is 220 points), else under them.
            ui.label(egui::RichText::new(crate::et!("Move by")).size(TEXT));
            let apply_words = crate::et!("Apply");
            let width = |ui: &Ui, words: &str, style: egui::TextStyle| {
                ui.painter().layout_no_wrap(words.to_string(), style.resolve(ui.style()), Color32::WHITE).size().x
            };
            let gap = ui.spacing().item_spacing.x;
            let fields = width(ui, "X", egui::TextStyle::Body) + width(ui, "Y", egui::TextStyle::Body) + 2.0 * MOVE_BY_FIELD + 3.0 * gap;
            let button = width(ui, apply_words, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
            let one_row = ui.available_width() >= fields + gap + button;
            ui.horizontal(|ui| {
                for (k, axis) in ["X", "Y"].into_iter().enumerate() {
                    ui.label(axis);
                    let name = format!("{} {axis}", crate::et!("Move by"));
                    let look = crate::embroidery::fields::Look { name: Some(&name), ..Default::default() };
                    if let Some(v) =
                        crate::embroidery::fields::number_field(ui, ("embroidery-move-by", k), Some(by[k]), &format, &look, MOVE_BY_FIELD, &bounds)
                    {
                        by[k] = v;
                        apply |= ui.input(|i| i.key_pressed(egui::Key::Enter));
                    }
                }
                if one_row {
                    apply |= ui.button(apply_words).clicked();
                }
            });
            if !one_row {
                apply |= ui.button(apply_words).clicked();
            }
            app.embroidery.stitches.docs.entry(uid).or_default().move_by = by;
            if apply
                && by != [0.0, 0.0]
                && let Err(err) = app.run("embroidery.moveNeedlePoints", json!({"dx": by[0], "dy": by[1]}))
            {
                app.status(err);
            }
            // The stitch actions in rows of whole buttons; one wider than the panel (a long word at
            // its narrowest) on a row of its own, its words wrapped.
            let items = crate::embroidery::task_bar::items(app).unwrap_or_default();
            let full = ui.available_width();
            let mut rows: Vec<Vec<usize>> = Vec::new();
            let mut used = 0.0;
            for (k, (label, ..)) in items.iter().enumerate() {
                let w = width(ui, label, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
                match rows.last_mut() {
                    Some(row) if used + gap + w <= full => {
                        row.push(k);
                        used += gap + w;
                    }
                    _ => {
                        rows.push(vec![k]);
                        used = w;
                    }
                }
            }
            let mut chosen = None;
            for row in rows {
                ui.horizontal(|ui| {
                    for (label, _, id, params) in row.iter().filter_map(|k| items.get(*k)) {
                        if ui.add(egui::Button::new(*label).wrap()).clicked() {
                            chosen = Some((*id, params.clone()));
                        }
                    }
                });
            }
            if let Some((id, params)) = chosen {
                crate::menus::invoke(app, id, params);
            }
        } else {
            ui.label(egui::RichText::new(crate::et!("Click a row or a needle point to select it.")).size(TEXT).color(t.text_dim));
        }
        return;
    }
    let ids = scope_ids(app);
    let Some(st) = app.session.active() else { return };
    let stitches = ids.iter().copied().find(|id| st.doc.node(*id).is_some_and(vectorcraft_tools::embroidery::holds_stitches));
    let planned = ids.iter().any(|id| st.doc.node(*id).is_some_and(|n| !vectorcraft_tools::embroidery::holds_stitches(n)));
    if let Some(id) = stitches {
        if ui.add(egui::Button::new(crate::et!("Edit Stitches")).wrap()).clicked() {
            crate::embroidery::editing::start(app, id);
        }
    } else if planned && listed.span.is_some() {
        ui.label(
            egui::RichText::new(crate::et!("Planned from the shape: convert the object to manual stitches to edit them."))
                .size(TEXT)
                .color(t.text_dim),
        );
        let label = super::command_label(app, "embroidery.convertToManual");
        if ui.add(egui::Button::new(label).wrap()).clicked() {
            crate::embroidery::convert_job::start(app, &json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()}));
        }
    }
}

/// The largest move Move by takes, millimetres: a needle point stays within any hoop. An estimate.
const MOVE_BY_MAX_MM: f64 = 1000.0;
/// A Move by field's width, points: room for "−123,4 mm".
const MOVE_BY_FIELD: f32 = 72.0;

/// The stitch shown from the list on the canvas, outside stitch editing: a two-tone ring at its
/// needle point and its stitch from the one before, while the tab shows.
pub fn draw_mark(app: &VectorcraftApp, painter: &egui::Painter, xf: &crate::canvas::Xf) {
    if app.ui.embroidery_panel.tab != super::Tab::Stitches {
        return;
    }
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let Some(number) = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.shown) else { return };
    let Some(shown) = app.embroidery.preview.shown(uid) else { return };
    let Some(row) = stitch_list::row(&shown.planned.plan, number) else { return };
    let at = xf.to_screen(Point::new(mm_to_pt(row.at.x), mm_to_pt(row.at.y)));
    let (dark, light) = (crate::embroidery::handles::dark(), crate::embroidery::handles::light());
    if row.length.is_some()
        && row.kind == RecordKind::Stitch
        && let Some(before) = number.checked_sub(1).and_then(|n| stitch_list::row(&shown.planned.plan, n))
    {
        let from = xf.to_screen(Point::new(mm_to_pt(before.at.x), mm_to_pt(before.at.y)));
        painter.line_segment([from, at], Stroke::new(MARK_LINE_UNDER, dark));
        painter.line_segment([from, at], Stroke::new(MARK_LINE, light));
    }
    painter.circle_stroke(at, MARK_RING, Stroke::new(MARK_LINE_UNDER, dark));
    painter.circle_stroke(at, MARK_RING, Stroke::new(MARK_LINE, light));
}

/// The shown stitch's ring's radius and its two lines' widths, points.
const MARK_RING: f32 = 7.0;
const MARK_LINE: f32 = 1.5;
const MARK_LINE_UNDER: f32 = 3.5;

/// `ui.inspect`'s `embroideryPanel.stitches`: the scope, the filter, the rows listed and the record
/// in focus.
pub fn inspect(app: &VectorcraftApp) -> serde_json::Value {
    let uid = app.session.active().map(|st| st.uid);
    let d = uid.and_then(|u| app.embroidery.stitches.docs.get(&u));
    let rows = app.embroidery.stitches.list.as_ref().filter(|(k, _)| Some(k.0) == uid).map(|(_, l)| l.len());
    json!({
        "scope": app.ui.embroidery_panel.stitches_scope.id(),
        "filter": app.ui.embroidery_panel.stitches_filter.id(),
        "rowCount": rows,
        "focus": d.and_then(|d| d.focus),
        "shown": d.and_then(|d| d.shown),
        "page": d.map_or(0, |d| d.page),
    })
}

#[cfg(test)]
mod tests;
