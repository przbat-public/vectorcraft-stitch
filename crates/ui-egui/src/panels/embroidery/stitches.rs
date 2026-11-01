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
use std::ops::Range;
use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::plan::DesignPlan;
use vectorcraft_engine::cmd::embroidery::stitch_list::{self, Filter, Flags, RecordKind};
use vectorcraft_engine::cmd::embroidery::{LimitsKey, StitchLimits, mm_to_pt};
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
/// The words' size, a column's room either side, the thread chip's size and the marks' size,
/// points.
const TEXT: f32 = 12.0;
const CELL_PAD: f32 = 4.0;
const CHIP: f32 = 8.0;
const MARK: f32 = 11.0;
/// The list's least height, points: a few rows, whatever the panel's room.
const MIN_LIST: f32 = 6.0 * ROW_HEIGHT;
/// The ring of the row in focus while the list has the keyboard, and its quiet mark while it
/// hasn't, points (the canvas's selection rings' weights).
const FOCUS_RING_PT: f32 = 2.0;
const FOCUS_QUIET_PT: f32 = 1.0;
/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;
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

/// The rows a scope and a filter list: record numbers in sewing order, as ranges (every record of
/// them) or picked one by one; with each filter's count in the scope.
pub struct Listed {
    rows: Rows,
    pub counts: [usize; Filter::ALL.len()],
    /// The scope's first and last record numbers.
    pub span: Option<(usize, usize)>,
}

impl Default for Listed {
    /// No rows: before any plan's flags are built.
    fn default() -> Self {
        Self { rows: Rows::Picked(Vec::new()), counts: [0; Filter::ALL.len()], span: None }
    }
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
    // Each filter's count from the flags' counts by chunk (built on the planning thread), and the
    // filter's rows from the chunks it lists any of: no walk of every record here.
    let mut counts = [0usize; Filter::ALL.len()];
    let mut picked: Vec<u32> = Vec::new();
    for r in ranges {
        for (slot, n) in counts.iter_mut().zip(flags.counts(r.clone())) {
            *slot += n;
        }
        if filter != Filter::All {
            flags.each_listed(r.clone(), filter, |n| {
                if let Ok(n) = u32::try_from(n) {
                    picked.push(n);
                }
            });
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
    said: Option<String>,
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
    /// The foot's height as last drawn, points, outside stitch editing and while editing (0: not
    /// drawn yet): the room kept for it under the list.
    foot: [f32; 2],
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
        // Both lines, when the format's records are the nearer: a diagonal within them can still be
        // past the longest stitch.
        Filter::Long => (
            crate::et!("Too long").to_string(),
            Some(if limits.step < limits.longest {
                crate::embroidery::text::fill(
                    crate::et!("Longer than {longest} mm, or along x or y than one {format} record ({step} mm)"),
                    &[
                        ("longest", &numbers::decimal(lang, limits.longest, LENGTH_DECIMALS)),
                        ("format", limits.format.label()),
                        ("step", &numbers::decimal(lang, limits.step, LENGTH_DECIMALS)),
                    ],
                )
            } else {
                crate::embroidery::text::fill(
                    crate::et!("Longer than {value} mm"),
                    &[("value", &numbers::decimal(lang, limits.longest, LENGTH_DECIMALS))],
                )
            }),
        ),
    };
    let shown = crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &word), ("count", &n)]);
    let named = match full {
        Some(f) => crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &f), ("count", &n)]),
        None => shown.clone(),
    };
    (shown, named)
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
            ui.label(egui::RichText::new(crate::et!("Listing stitches…")).size(TEXT).color(t.text_dim));
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
            let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
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
    });
    ui.spacing_mut().item_spacing.y = spacing;
    // The list itself takes the keyboard's focus (a row clicked gives it), so its keys are its own.
    let list = ui.interact(scrolled.inner_rect, list_id, Sense::focusable_noninteractive());
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

/// How a row is marked as the list's focus: not at all, quietly (the list doesn't have the
/// keyboard: the keys reach the canvas), or ringed (they are the list's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusMark {
    None,
    Quiet,
    Ring,
}

/// How a row is drawn: lit when its needle point is selected, its focus mark, and dimmed while it
/// is of a plan an edit has left behind.
#[derive(Clone, Copy, Debug)]
struct RowLook {
    selected: bool,
    focus: FocusMark,
    dimmed: bool,
}

/// Draw one row → its name for screen readers ("Record 4695: stitch, X 25.1 mm, Y 23.2 mm, length
/// 2.61 mm, too short").
#[allow(clippy::too_many_arguments)]
fn draw_row(
    app: &VectorcraftApp,
    ui: &Ui,
    rect: Rect,
    plan: &DesignPlan,
    row: &stitch_list::Row,
    columns: &Columns,
    flags: &Flags,
    look: RowLook,
    lang: Lang,
    t: &Tokens,
) -> String {
    let RowLook { selected, focus, dimmed } = look;
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::ZERO, t.row_selected);
    }
    let font = egui::FontId::proportional(TEXT);
    let faded = |c: Color32| if dimmed { c.gamma_multiply(super::STALE_INK) } else { c };
    let ink = faded(if selected { t.text_strong } else { t.text });
    let origin = app.ruler_origin();
    let x_mm = (mm_to_pt(row.at.x) - origin.x) / PT_PER_MM;
    let y_mm = (mm_to_pt(row.at.y) - origin.y) / PT_PER_MM;
    let [short, long] = flags.judged(row.record);
    let mut x = rect.left();
    // The thread chip (of the plan the row is of) and the number.
    let colour = plan.blocks.get(row.block).map(|b| b.thread.color()).map_or(t.text_dim, |[r, g, b]| faded(Color32::from_rgb(r, g, b)));
    let chip = Rect::from_center_size(pos2(x + CELL_PAD + CHIP / 2.0, rect.center().y), vec2(CHIP, CHIP));
    p.rect(chip, CornerRadius::same(2), colour, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let number = numbers::count(lang, row.record as u64);
    p.text(pos2(x + columns.widths[0] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &number, font.clone(), ink);
    x += columns.widths[0];
    let kind = kind_word(row.kind);
    let dim = faded(marks::readable_text(t.text_dim, &[if selected { t.row_selected } else { t.panel }], t.dark));
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
    match focus {
        FocusMark::Ring => {
            let ring = marks::readable(t.accent, &[if selected { t.row_selected } else { t.panel }], t.dark);
            p.rect_stroke(rect.shrink(1.0), CornerRadius::same(2), Stroke::new(FOCUS_RING_PT, ring), egui::StrokeKind::Inside);
            p.rect_stroke(rect.shrink(3.0), CornerRadius::same(2), Stroke::new(1.0, t.panel), egui::StrokeKind::Inside);
        }
        FocusMark::Quiet => {
            p.rect_stroke(rect.shrink(1.0), CornerRadius::same(2), Stroke::new(FOCUS_QUIET_PT, t.border), egui::StrokeKind::Inside);
        }
        FocusMark::None => {}
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

/// A click on record `number`'s row of `listed`: the row in focus; while stitches are edited the
/// needle point it is edited by selected, as `embroidery.selectNeedlePoints` selects it by its
/// record (a jump's the one it lands on, a cut's or a stop's the one it follows; Shift: those of the
/// rows listed from the last clicked, at most [`RANGE_MAX`]; Cmd or Ctrl: one more or one less),
/// else the stitch shown on the canvas (a double-click: zoomed to).
fn click(app: &mut VectorcraftApp, shown: &Shown, listed: &Listed, number: usize, mods: egui::Modifiers, double: bool) {
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
            app.status(crate::etn!(
                1,
                "{count} run can't be edited: locked, hidden or cut by a clipping mask",
                "{count} runs can't be edited: locked, hidden or cut by a clipping mask"
            ));
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
fn ranges_of(picked: &std::collections::BTreeSet<(u64, usize, usize)>) -> Vec<serde_json::Value> {
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
/// a screenful, Home and End (and Cmd or Ctrl with ↑ ↓) the first and last, with Shift while
/// stitches are edited the selection extended to it; Space as a click on it (Shift extends, Cmd or
/// Ctrl takes it in or out); Enter shows it, zoomed to. Keys taken from egui, so neither the
/// canvas's arrows nor upstream's shortcuts see them.
fn keys(app: &mut VectorcraftApp, ui: &Ui, list: egui::Id, uid: u64, listed: &Listed, shown: &Shown, screenful: usize) {
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
fn go_to(app: &mut VectorcraftApp, ui: &mut Ui, uid: u64, listed: &Listed) {
    let lang = crate::i18n::current();
    let d = app.embroidery.stitches.docs.entry(uid).or_default();
    let mut text = d.goto.clone();
    let said = d.said.clone();
    let mut go = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::et!("Go to #")).size(TEXT));
        let resp = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0).char_limit(GO_TO_CHARS));
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
const GO_TO_CHARS: usize = 24;

/// The record number Go to # was given as `text`: a whole number as `lang` writes one (its group
/// mark or a space between threes), a `#` before it allowed; `None` for anything else ("1e9",
/// "4.5", a group of two).
fn record_number(text: &str, lang: Lang) -> Option<usize> {
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
const GROUP_DIGITS: usize = 3;

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
                    // Enter in the field moves them, by the value typed or, unchanged, by the one
                    // shown again.
                    let field = ui.id().with(("embroidery-move-by", k));
                    apply |= ui.memory(|m| m.has_focus(field)) && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if let Some(v) =
                        crate::embroidery::fields::number_field(ui, ("embroidery-move-by", k), Some(by[k]), &format, &look, MOVE_BY_FIELD, &bounds)
                        && v.is_finite()
                    {
                        // A value typed past the range is held to it, as the arrows and the scrub
                        // hold theirs.
                        by[k] = v.clamp(*bounds.start(), *bounds.end());
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
            for (k, item) in items.iter().enumerate() {
                let w = width(ui, item.label, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
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
            // Each action as the ≡ menu offers it: off when its command can't run now.
            let on: Vec<bool> = items.iter().map(|item| crate::menus::enabled(app, item.command)).collect();
            for row in rows {
                ui.horizontal(|ui| {
                    for k in row {
                        let (Some(item), Some(&on)) = (items.get(k), on.get(k)) else { continue };
                        if ui.add_enabled(on, egui::Button::new(item.label).wrap()).clicked() {
                            chosen = Some((item.command, item.params.clone()));
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
    let stitches = ids.iter().copied().find(|id| st.doc.node(*id).is_some_and(vectorcraft_tools::embroidery::contains_stitches));
    let planned = ids.iter().any(|id| st.doc.node(*id).is_some_and(|n| !vectorcraft_tools::embroidery::contains_stitches(n)));
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

#[cfg(test)]
mod tests;
