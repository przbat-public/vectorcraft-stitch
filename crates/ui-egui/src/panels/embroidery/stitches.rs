//! The Embroidery panel's Stitches tab (VectorCraft Stitch's manual stitches): the design's records
//! as the Sewing order and the Check tab number them, read from the plan the canvas shows, never by
//! asking a command per scroll position.
//!
//! - **Scope** (the project owner's choice, 2026-10-27): Selection (the selected objects, while
//!   stitches are edited the object edited) or the whole design. A click in the list never changes
//!   which rows it shows.
//! - **Filters with counts:** all, jumps, cuts, stops, and the stitches too short or too long by the
//!   one set of limits the canvas marks ([`stitch_limits`]). Each record's kind and judgement is one
//!   byte built with the plan on the planning thread ([`Query::StitchFlags`]); the rows a scope and filter
//!   list are worked out once per (plan, scope, filter, limits), never per frame ([`Listed`]).
//! - **Columns:** the record's number, its kind, X and Y in millimetres from the rulers' zero point
//!   (the project owner's choice), and the thread its stitch lays; a stitch past the limits carries
//!   the Check tab's warning mark. Rows of [`ROW_HEIGHT`] in a virtual list, only those in view
//!   drawn; past [`MAX_SCROLLED_ROWS`] the list is split into pages, as egui places rows in `f32`.
//! - **A click:** outside stitch editing it shows the stitch (a mark on the canvas, the view panned
//!   to it when it is out of view: the project owner's choice); while stitches are edited it selects
//!   the needle point the record is edited by, as `embroidery.selectNeedlePoints` by records does (a
//!   cut's or a stop's the one it follows; Shift a range, Cmd or Ctrl one more), the list lit from
//!   the document's selection. A double-click or Enter zooms to it, as the Check tab's Show does.
//! - **Go to #**, and at the foot, while editing: the selection, Move by X and Y (the project
//!   owner's choice) and the stitch actions; outside it Edit Stitches, or for planned stitches,
//!   read only, Convert to Manual Stitches.
//! - **Keyboard and screen readers:** the list is a grid of rows with their count and index; ↑ ↓,
//!   Page Up/Down, Home and End move the row in focus, Space selects it, Enter shows it.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::plan::DesignPlan;
use vectorcraft_engine::cmd::embroidery::stitch_list::{self, Filter, Flags, Listed, RecordKind, listed};
use vectorcraft_engine::cmd::embroidery::{LimitsKey, StitchLimits, mm_to_pt, pt_to_mm};
use vectorcraft_geom::Point;
use vectorcraft_tools::embroidery::{NeedleRef, SelectMode};

use crate::VectorcraftApp;
use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::plan_worker::Query;
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
/// The words' size ([`crate::embroidery::WORDS_PT`]), a column's room either side, the thread
/// chip's size and the marks' size, points.
const TEXT: f32 = crate::embroidery::WORDS_PT;
const CELL_PAD: f32 = 4.0;
const CHIP: f32 = 8.0;
const MARK: f32 = 11.0;
/// The list's least height, points: a few rows, whatever the panel's room.
const MIN_LIST: f32 = 6.0 * ROW_HEIGHT;
/// The ring of the row in focus while the list has the keyboard, and its quiet mark while it
/// hasn't, points (the canvas's selection rings' weights).
const FOCUS_RING_PT: f32 = 2.0;
const FOCUS_QUIET_PT: f32 = 1.0;
/// The tab's dim words' colour in theme `t`: the theme's, lifted where it needs to be to read by
/// 4.5:1 on the panel (12 pt words), as every dim word of the fork's panels is.
pub(crate) fn dim(t: &Tokens) -> Color32 {
    marks::readable_text(t.text_dim, &[t.panel], t.dark)
}

/// The corners of the list's outline while it has the keyboard, points.
const LIST_ROUND: u8 = 2;

/// The most rows a Shift-click's range selects at once (the status line says so past it): the
/// stitch commands' bound on a call's addresses is far above, this keeps one click's work and params
/// small.
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

/// What [`Listed`] is of: the document, the plan's generation, the scope's objects (`None`: the
/// whole design), the filter and the limits it judges by.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListKey {
    pub(crate) uid: u64,
    generation: u64,
    ids: Option<Vec<NodeId>>,
    filter: Filter,
    pub(crate) limits: LimitsKey,
}

/// The numberings worked out for document `uid`'s plan of `generation`.
struct KeptNumberings {
    uid: u64,
    generation: u64,
    numberings: Numberings,
}

/// One document's place in the list: the record in focus and the one a Shift-click's range starts
/// from, the page shown, the stitch shown on the canvas, Move by's two fields (millimetres), Go
/// to's words and what it last said.
#[derive(Default)]
pub struct DocList {
    pub focus: Option<usize>,
    anchor: Option<usize>,
    pub(crate) page: usize,
    pub shown: Option<usize>,
    move_by: [f64; 2],
    goto: String,
    /// Go to's answer, and the rows listed it was said of: shown while they are.
    said: Option<(String, ListKey)>,
    /// Scroll to the focus in the next frame, and give the list the keyboard (Go to's Enter).
    reveal: bool,
    /// The generation of the plan the record numbers above (focus, anchor, shown) are of: a newer
    /// plan finds them again ([`refind`]). `None` until the list is first drawn: of the plan drawn.
    of: Option<u64>,
}

/// The tab's state: each document's place, the rows listed (built once a key, [`ListKey`]), and
/// each unit's numbering for the plan shown (worked out once a unit, for the rows in view).
#[derive(Default)]
pub struct StitchesState {
    pub(crate) docs: HashMap<u64, DocList>,
    pub(crate) list: Option<(ListKey, Arc<Listed>)>,
    numbering: Option<KeptNumberings>,
    /// The pass the tab was last drawn in (`egui::Context::cumulative_pass_nr`): the canvas rings the
    /// record shown only while the tab shows, as the Check tab's marks.
    drawn_pass: Option<u64>,
    /// The last plan drawn with its flags, of document (uid): drawn dimmed while a newer plan's
    /// flags are being built, so the list, Go to and the foot stay (a field typed into keeps the
    /// keyboard).
    drawn: Option<(u64, Shown)>,
    /// How many times the rows were worked out (the tests count them: never on an idle frame).
    pub(crate) builds: usize,
    /// The foot's height as last drawn, outside stitch editing and while editing: the room kept for
    /// it under the list.
    foot: Foot,
}

/// The foot's heights as last drawn, points (0: not drawn yet), outside stitch editing and while
/// editing.
#[derive(Default)]
struct Foot {
    outside: f32,
    editing: f32,
}

impl Foot {
    /// The height kept for the foot outside editing, or while editing (`editing`).
    fn of(&mut self, editing: bool) -> &mut f32 {
        if editing { &mut self.editing } else { &mut self.outside }
    }
}

/// A unit's numbering: the path, its first needle point's place, each record's ordinal, and the
/// needle points its records before its own are edited by ([`stitch_list::leading`]).
#[derive(Clone)]
struct Numbered {
    node: NodeId,
    first: usize,
    ordinals: Arc<Vec<u32>>,
    leading: Arc<Vec<(usize, (NodeId, usize))>>,
}

/// The numberings worked out for the plan shown, by unit, and how many needle points each path they
/// name held in the document it was planned from (an address's `of`), by path.
#[derive(Default)]
struct Numberings {
    units: HashMap<usize, Option<Numbered>>,
    counts: HashMap<NodeId, Option<usize>>,
}

/// A needle point as a row names it: its path, its place, and how many needle points the path held
/// when the plan shown numbered them (the stitch commands refuse it when an edit since changed it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Address {
    node: NodeId,
    index: usize,
    of: usize,
}

/// The limits the tab, the canvas and the Check tab's format judge by, for the active document
/// ([`crate::embroidery::limits`]).
pub fn limits_of(app: &VectorcraftApp) -> Option<StitchLimits> {
    crate::embroidery::limits(app)
}

/// What the tab asks the planning thread for besides the figures: its records' flags for the limits.
pub fn query(app: &VectorcraftApp) -> Option<Query> {
    limits_of(app).map(Query::StitchFlags)
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
    let key = ListKey { uid: st.uid, generation, ids: ids.clone(), filter, limits: flags.limits() };
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
/// numbering in the document the plan shown was made from, worked out once a unit for the plan.
/// `None` while its path changed since (its node isn't the same now, as the canvas's record
/// numbers ask): an edit since may have moved its needle points.
fn edited_by(app: &mut VectorcraftApp, shown: &Shown, unit: usize, number: usize, kind: RecordKind) -> Option<Address> {
    let st = app.session.active()?;
    let kept = &mut app.embroidery.stitches.numbering;
    if kept.as_ref().is_none_or(|k| k.uid != st.uid || k.generation != shown.generation) {
        *kept = Some(KeptNumberings { uid: st.uid, generation: shown.generation, numberings: Numberings::default() });
    }
    let numberings = &mut kept.as_mut()?.numberings;
    let (plan, then) = (&shown.plan, &shown.document);
    let n = numberings
        .units
        .entry(unit)
        .or_insert_with(|| {
            stitch_list::numbering(plan, then, unit).map(|(node, first)| Numbered {
                node,
                first,
                ordinals: Arc::new(stitch_list::ordinals(plan, unit)),
                leading: Arc::new(stitch_list::leading(plan, then, unit, first)),
            })
        })
        .clone()?;
    let (node, index) = match n.leading.iter().find(|(r, _)| *r == number) {
        Some((_, at)) => *at,
        None => (n.node, stitch_list::edited_by(plan, unit, n.first, &n.ordinals, number, kind)?),
    };
    let was = then.node(node)?;
    if !std::ptr::eq(was, st.doc.node(node)?) {
        return None;
    }
    let of = (*numberings.counts.entry(node).or_insert_with(|| vectorcraft_engine::cmd::embroidery::needle_points_in(was)))?;
    Some(Address { node, index, of })
}

/// The record numbers document `uid`'s list keeps (the focus, the Shift anchor, the stitch shown),
/// of the plan `before`, for the plan `now`: while stitches are edited each found again by the needle
/// point it is edited by, when that is still there; else, or without `before`, dropped. A plan's
/// numbers are its own: after an edit, the same number is another stitch.
fn refind(app: &mut VectorcraftApp, uid: u64, before: Option<&Shown>, now: &Shown) {
    let editing = crate::embroidery::editing::editing(app).is_some();
    let Some(d) = app.embroidery.stitches.docs.get_mut(&uid) else { return };
    let Some(of) = d.of.filter(|g| *g != now.generation) else {
        d.of = Some(now.generation);
        return;
    };
    d.of = Some(now.generation);
    let again = |n: Option<usize>| -> Option<usize> {
        let before = before.filter(|b| b.generation == of && editing)?;
        let (node, index) = address_in(&before.plan, &before.document, n?)?;
        let (was, is) = (before.document.node(node)?, now.document.node(node)?);
        let count = vectorcraft_engine::cmd::embroidery::needle_points_in;
        // The same place while the path has as many needle points (a move keeps them in order);
        // after an insert or a delete, the needle point where it lay.
        let points = vectorcraft_engine::cmd::embroidery::needle_points_of;
        let index = if count(was) == count(is) {
            index
        } else {
            let at = points(was)?.nth(index)?;
            points(is)?.position(|p| p == at)?
        };
        record_in(&now.plan, &now.document, node, index)
    };
    (d.focus, d.anchor, d.shown) = (again(d.focus), again(d.anchor), again(d.shown));
}

/// The needle point record `number` of `plan` (made from `doc`) is edited by, as a click names it.
fn address_in(plan: &DesignPlan, doc: &Document, number: usize) -> Option<(NodeId, usize)> {
    let row = stitch_list::row(plan, number)?;
    let (node, first) = stitch_list::numbering(plan, doc, row.unit)?;
    if let Some((_, at)) = stitch_list::leading(plan, doc, row.unit, first).into_iter().find(|(r, _)| *r == number) {
        return Some(at);
    }
    let k = stitch_list::edited_by(plan, row.unit, first, &stitch_list::ordinals(plan, row.unit), number, row.kind)?;
    Some((node, k))
}

/// The needle-down record of `plan` (made from `doc`) that needle point `index` of path `node` is.
fn record_in(plan: &DesignPlan, doc: &Document, node: NodeId, index: usize) -> Option<usize> {
    plan.units.iter().enumerate().filter(|(_, u)| u.origin.node == node).find_map(|(unit, u)| {
        let (_, first) = stitch_list::numbering(plan, doc, unit)?;
        let ordinals = stitch_list::ordinals(plan, unit);
        let start = plan.blocks.get(u.block)?.first_number;
        u.own.clone().map(|at| start + at).find(|&n| {
            stitch_list::row(plan, n).is_some_and(|r| r.kind == RecordKind::Stitch)
                && stitch_list::edited_by(plan, unit, first, &ordinals, n, RecordKind::Stitch) == Some(index)
        })
    })
}

/// What the tab is drawn from: the plan shown (its generation, the document it was made from, and
/// whether it is of the document as it is) and the records' flags when the planning thread has
/// built them.
#[derive(Clone)]
pub struct Shown {
    pub plan: Arc<DesignPlan>,
    pub generation: u64,
    pub document: Arc<Document>,
    pub stale: bool,
    pub flags: Option<Arc<Flags>>,
}

/// What the tab keeps of a document closed since let go, each frame (as the stitch preview's plans
/// are), whether the tab shows or not: its place in the list, and what the tab keeps of one
/// document (its rows listed, their numberings, its last plan drawn) when it is that one.
pub fn forget_closed(app: &mut VectorcraftApp) {
    let open: std::collections::HashSet<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    let kept = &mut app.embroidery.stitches;
    kept.docs.retain(|uid, _| open.contains(uid));
    if kept.list.as_ref().is_some_and(|(key, _)| !open.contains(&key.uid)) {
        kept.list = None;
    }
    if kept.numbering.as_ref().is_some_and(|k| !open.contains(&k.uid)) {
        kept.numbering = None;
    }
    if kept.drawn.as_ref().is_some_and(|(uid, _)| !open.contains(uid)) {
        kept.drawn = None;
    }
}

/// Draw the tab.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, shown: Shown) {
    app.embroidery.stitches.drawn_pass = Some(ui.ctx().cumulative_pass_nr());
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
    // The plan shown once its flags are built; until then the last one drawn with its flags,
    // dimmed and only shown by a click, as the Order and Check tabs keep their last result.
    let kept = &mut app.embroidery.stitches.drawn;
    if kept.as_ref().is_some_and(|(u, _)| *u != uid) {
        *kept = None;
    }
    let shown = match (&shown.flags, kept.as_ref()) {
        (Some(_), _) => {
            let before = kept.replace((uid, shown.clone())).map(|(_, s)| s);
            refind(app, uid, before.as_ref(), &shown);
            shown
        }
        (None, Some((_, last))) => Shown { stale: true, ..last.clone() },
        (None, None) => {
            ui.label(egui::RichText::new(crate::et!("Listing stitches…")).size(TEXT).color(dim(&t)));
            let editing = crate::embroidery::editing::editing(app);
            foot_ui(app, ui, editing, &Listed::default(), lang, &t);
            return;
        }
    };
    let Some(flags) = shown.flags.clone() else { return };
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
            let text = egui::RichText::new(words).size(TEXT).color(if count == 0 && f != filter { dim(&t) } else { t.text });
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
            // The arrows named for screen readers by what they do, as their tooltips say it.
            let arrow = |ui: &mut Ui, on: bool, glyph: &str, name: &'static str| {
                let resp = ui.add_enabled(on, egui::Button::new(glyph)).on_hover_text(name);
                ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name));
                resp.clicked()
            };
            if arrow(ui, page > 0, "‹", crate::et!("Previous rows"))
                && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
            {
                d.page = page - 1;
            }
            ui.label(egui::RichText::new(words).size(TEXT));
            if arrow(ui, page + 1 < pages, "›", crate::et!("Next rows"))
                && let Some(d) = app.embroidery.stitches.docs.get_mut(&uid)
            {
                d.page = page + 1;
            }
        });
    }
    // The column heads, then the rows in view.
    let width = ui.available_width();
    let columns = Columns::fit(ui, width);
    let in_page = (total - (page * MAX_SCROLLED_ROWS).min(total)).min(MAX_SCROLLED_ROWS);
    let drawn = *app.embroidery.stitches.foot.of(editing.is_some());
    let foot = if drawn > 0.0 { drawn } else { foot_height(editing.is_some()) };
    // Whole rows: none cut by the foot's words under the list.
    let room = (((ui.available_height() - ROW_HEIGHT - foot) / ROW_HEIGHT).floor() * ROW_HEIGHT).max(MIN_LIST);
    let list_id = ui.id().with("embroidery-stitch-list");
    // The list itself takes the keyboard's focus (a row clicked gives it), so its keys are its own:
    // for screen readers it is the grid, its heads and rows under it and its row in focus its
    // active descendant. Its node is made before theirs, which hang under the first ancestor with
    // one.
    let list_rect = Rect::from_min_size(ui.cursor().min, vec2(width, ROW_HEIGHT + room));
    let list = ui.interact(list_rect, list_id, Sense::focusable_noninteractive());
    ui.ctx().accesskit_node_builder(list_id, |node| {
        node.set_role(egui::accesskit::Role::Grid);
        node.set_label(crate::et!("Stitches"));
        node.set_row_count(total);
        node.set_column_count(columns.count());
    });
    // Under it, as a grid holds them: the heads in a row of their own, the rows in a group.
    let under_list = || egui::UiBuilder::new().accessibility_parent(list_id);
    let role = |ui: &Ui, role: egui::accesskit::Role| ui.ctx().accesskit_node_builder(ui.unique_id(), |node| node.set_role(role));
    ui.scope_builder(under_list(), |ui| {
        role(ui, egui::accesskit::Role::Row);
        header(ui, &columns, &t);
    });
    // Whether the list has the keyboard: its row in focus is ringed only then (its keys are the
    // list's), else marked quietly (they reach the canvas).
    let has_keys = ui.memory(|m| m.has_focus(list_id));
    let mut clicked: Option<(usize, egui::Modifiers, bool)> = None;
    let focus = app.embroidery.stitches.docs.get(&uid).and_then(|d| d.focus);
    let reveal = app.embroidery.stitches.docs.get_mut(&uid).is_some_and(|d| std::mem::take(&mut d.reveal));
    let mut area = egui::ScrollArea::vertical().id_salt(list_id).max_height(room).auto_shrink([false, false]);
    if reveal && let Some(row) = focus.and_then(|f| listed.row_of(f)).filter(|r| r / MAX_SCROLLED_ROWS == page) {
        area = area.vertical_scroll_offset(((row % MAX_SCROLLED_ROWS) as f32 * ROW_HEIGHT - room / 2.0).max(0.0));
    }
    // No spacing between rows, in the list as in egui's sums: it places a row and works out the
    // rows in view from its height plus the spacing of the ui it is given.
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut focus_row: Option<egui::Id> = None;
    let scrolled = area.show_rows(ui, ROW_HEIGHT, in_page, |ui, range| {
        ui.scope_builder(under_list(), |ui| {
            role(ui, egui::accesskit::Role::RowGroup);
            // The rows in view, each with whether the needle point it is edited by is selected: those
            // asked of the session together, a look-up each.
            let rows: Vec<(usize, usize, stitch_list::Row, Option<NeedleRef>)> = range
                .filter_map(|i| {
                    let row_index = page * MAX_SCROLLED_ROWS + i;
                    let number = listed.record(row_index)?;
                    let row = stitch_list::row(&shown.plan, number)?;
                    let address = editing.and_then(|_| edited_by(app, &shown, row.unit, number, row.kind));
                    Some((row_index, number, row, address.map(|a| NeedleRef { id: a.node, index: a.index })))
                })
                .collect();
            let edited: Vec<NeedleRef> = rows.iter().filter_map(|(.., n)| *n).collect();
            let view = app.view_info();
            let mut selected_now = app.session.stitch_needles_selected(view, &edited).into_iter();
            for (row_index, number, row, address) in rows {
                let selected = address.is_some() && selected_now.next().unwrap_or(false);
                // A click selects, a double-click shows, but the rows are no Tab stops: the list takes
                // the keyboard, a row at a time.
                let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::CLICK);
                if focus == Some(number) {
                    focus_row = Some(resp.id);
                }
                let focus = match (focus == Some(number), has_keys) {
                    (false, _) => FocusMark::None,
                    (true, true) => FocusMark::Ring,
                    (true, false) => FocusMark::Quiet,
                };
                let look = RowLook { selected, focus, dimmed: shown.stale };
                let words = draw_row(app, ui, rect, &shown.plan, &row, &columns, &flags, look, lang, &t);
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
        })
    });
    ui.spacing_mut().item_spacing.y = spacing;
    if let Some(row) = focus_row {
        ui.ctx().accesskit_node_builder(list_id, |node| node.set_active_descendant(row.accesskit_id()));
    }
    // Outlined while it has the keyboard: its keys are the list's then, not the canvas's.
    if has_keys {
        let around = egui::Rect::from_min_max(list_rect.min, scrolled.inner_rect.max);
        ui.painter().rect_stroke(around, CornerRadius::same(LIST_ROUND), Stroke::new(FOCUS_RING_PT, t.accent), egui::StrokeKind::Inside);
    }
    if clicked.is_some() || reveal {
        list.request_focus();
    }
    // Its arrows and Escape locked to it: egui would move the focus to the next widget on an arrow
    // and drop it on Escape before the canvas's keys run (which then see no focus, and act).
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(list_id, egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    // A screenful for Page Up and Down: the rows the list shows.
    let screenful = ((room / ROW_HEIGHT) as usize).max(1);
    keys(app, ui, list_id, uid, &listed, &shown, screenful);
    if let Some((number, mods, double)) = clicked {
        click(app, &shown, &listed, number, mods, double);
    }
    let top = ui.cursor().top();
    foot_ui(app, ui, editing, &listed, lang, &t);
    // The room the next frame keeps for it; a frame at once when it changed (the list drawn this
    // frame had the old room), so the list never lies over the foot past one frame.
    let used = ui.cursor().top() - top;
    let kept = app.embroidery.stitches.foot.of(editing.is_some());
    if (*kept - used).abs() > FOOT_SETTLED {
        *kept = used;
        ui.ctx().request_repaint();
    }
}

/// The stitch shown from the list on the canvas, outside stitch editing: a two-tone ring at its
/// needle point and its stitch from the one before, while the tab shows.
pub fn draw_mark(app: &VectorcraftApp, painter: &egui::Painter, xf: &crate::canvas::Xf, pass: u64) {
    // Only while the tab shows (drawn in this pass or the last), and as the canvas's overlays: not
    // with Hide Edges, not in presentation mode.
    let drawn = app.embroidery.stitches.drawn_pass.is_some_and(|p| pass <= p.saturating_add(1));
    if app.ui.embroidery_panel.tab != super::Tab::Stitches || !drawn || !app.ui.view.edges || app.ui.screen_mode >= 3 {
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

/// `embroidery.showRecord {record, zoom?}`: record `record` of the plan the Stitches tab lists, in
/// focus, its row scrolled into view and the list given the keyboard, its stitch shown on the
/// canvas (a ring, the view panned to it when it lies out of view; `zoom`: zoomed to it), as Go to #
/// and a click outside stitch editing do → `{record, row}` (its place among the rows listed, from
/// 0). Refused when the record isn't listed (the scope or the filter leaves it out) and while the
/// plan's records aren't judged yet.
pub(crate) fn show_record_command(app: &mut VectorcraftApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    super::only_keys(p, &["record", "zoom"])?;
    let number = p
        .get("record")
        .and_then(serde_json::Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n >= 1)
        .ok_or("`record`: a record's number, from 1")?;
    let zoom = crate::menus::opt_bool(p, "zoom")?.unwrap_or(false);
    let uid = app.session.active().map(|st| st.uid).ok_or("no document")?;
    let limits = limits_of(app).ok_or("no document")?;
    let preview = app.embroidery.preview.shown(uid).ok_or("the stitches aren't planned yet")?;
    let flags = preview.planned.built_stitch_flags(&limits).ok_or("the records are being judged: ask again in a moment")?;
    let shown = Shown {
        plan: preview.planned.plan.clone(),
        generation: preview.planned.generation,
        document: preview.document.clone(),
        stale: false,
        flags: Some(flags.clone()),
    };
    let listed = rows(app, &shown.plan, shown.generation, &flags);
    let row = listed.row_of(number).ok_or_else(|| {
        let (scope, filter) = (app.ui.embroidery_panel.stitches_scope.id(), app.ui.embroidery_panel.stitches_filter.id());
        format!("record {number} isn't listed (scope {scope}, filter {filter})")
    })?;
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    (d.focus, d.page, d.reveal, d.of) = (Some(number), row / MAX_SCROLLED_ROWS, true, Some(shown.generation));
    show_stitch(app, &shown, number, zoom);
    Ok(json!({"record": number, "row": row}))
}

/// `ui.inspect`'s `embroideryPanel.stitches`: the scope, the filter, the rows listed (`null` until the
/// tab has listed them for this scope and filter) and the record in focus.
pub fn inspect(app: &VectorcraftApp) -> serde_json::Value {
    let uid = app.session.active().map(|st| st.uid);
    let d = uid.and_then(|u| app.embroidery.stitches.docs.get(&u));
    let (scope, filter) = (app.ui.embroidery_panel.stitches_scope, app.ui.embroidery_panel.stitches_filter);
    let rows = app
        .embroidery
        .stitches
        .list
        .as_ref()
        .filter(|(k, _)| Some(k.uid) == uid && k.filter == filter && k.ids.is_some() == (scope == Scope::Selection))
        .map(|(_, l)| l.len());
    json!({
        "scope": app.ui.embroidery_panel.stitches_scope.id(),
        "filter": app.ui.embroidery_panel.stitches_filter.id(),
        "rowCount": rows,
        "focus": d.and_then(|d| d.focus),
        "shown": d.and_then(|d| d.shown),
        "page": d.map_or(0, |d| d.page),
    })
}

mod foot;
mod keys;
mod row;
#[cfg(test)]
mod tests;

use foot::*;
#[cfg(test)]
pub(crate) use keys::reveal;
use keys::*;
use row::*;
