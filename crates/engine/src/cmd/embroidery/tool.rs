//! What the embroidery tool reads of the stitch plan ([`vectorcraft_tools::embroidery::PlanFacts`]):
//! the engine answers from the plan the interface draws ([`crate::Session::show_embroidery_plan`]),
//! else, with no interface, from the newest plan it remembers of the whole document with the stitch
//! preview's options; built once a plan ([`ToolFacts`]), never planning to answer. An answer is of
//! the document as it is when the plan is of the snapshot the document is now, else stale (drawn
//! dim until the next plan): any edit moves Auto points, a neighbour's or the document's settings
//! too. A fill's angle needs no plan: it is read from the document as it is, as
//! `embroidery.info {plan: false}` reports it.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;

use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;
use vectorcraft_stitch::{Command, mm_to_pt};
use vectorcraft_tools::embroidery::{FillAngle, NeedleHit, NeedlePoints, NeedleRef, NeedleScope, PlanFacts, PlannedEnds, ThreadHit};

use super::needle_index::NeedleIndex;
use super::plan::{DesignPlan, Part};
use super::service::Planned;

/// One plan's answers: where each object's part is sewn (document points: its first and last
/// needle points, where the needle comes from before it and goes to after it along the thread), the
/// design's trim length (document points), and the snapshot the plan is of.
pub(crate) struct ToolFacts {
    pub(crate) generation: u64,
    snapshot: Arc<Document>,
    ends: HashMap<(NodeId, PartKind), Sewn>,
    trim: f64,
    /// The plan, for its stitch scene: built when a press first asks what lies under it (in the
    /// app the planning thread has built it already).
    planned: Planned,
    /// The record numbers the plan gives each path of stitches' needle points, worked out the first
    /// time a path being edited asks ([`ToolFacts::numbers_of`]): bounded by the paths edited.
    numbers: std::sync::Mutex<HashMap<NodeId, Option<Arc<Vec<usize>>>>>,
}

/// Where one object's part is sewn, and its neighbours along the thread.
#[derive(Clone, Copy)]
struct Sewn {
    first: Point,
    last: Point,
    before: Option<Point>,
    after: Option<Point>,
}

impl ToolFacts {
    /// The answers of `planned`, the plan of `snapshot`: each object's part (the object a person
    /// selects) from the first needle point of its first unit to the last of its last.
    pub(crate) fn of(snapshot: Arc<Document>, planned: &Planned) -> Self {
        let plan = &planned.plan;
        let in_points = |p: Point| Point::new(mm_to_pt(p.x), mm_to_pt(p.y));
        // Each unit's first and last needle points (where it really starts after an exact meet,
        // [`super::plan::DesignPlan::needle_ends`]), its block, and whether it ends with a cut.
        let units: Vec<Option<(Point, Point, usize, bool)>> = (0..plan.units.len())
            .map(|i| {
                let (first, last) = plan.needle_ends(i)?;
                let cut = plan.unit_stitches(i).last().is_some_and(|s| s.command == Command::Trim);
                Some((in_points(first), in_points(last), plan.units.get(i)?.block, cut))
            })
            .collect();
        // Along the thread: the unit before in the same block, with no cut after it.
        let before = |i: usize| {
            let (_, _, block, _) = (*units.get(i)?)?;
            let (_, last, b, cut) = (*units.get(i.checked_sub(1)?)?)?;
            (b == block && !cut).then_some(last)
        };
        let after = |i: usize| {
            let (_, _, block, cut) = (*units.get(i)?)?;
            let (first, _, b, _) = (*units.get(i + 1)?)?;
            (b == block && !cut).then_some(first)
        };
        let mut ends: HashMap<(NodeId, PartKind), Sewn> = HashMap::new();
        for (i, u) in plan.units.iter().enumerate() {
            let kind = match u.origin.part {
                Part::Fill(_) => PartKind::Fill,
                Part::Stroke(_) => PartKind::Stroke,
                Part::Literal | Part::Manual => continue,
            };
            let Some(Some((first, last, _, _))) = units.get(i).copied() else { continue };
            ends.entry((u.origin.selected(), kind))
                .and_modify(|e| {
                    e.last = last;
                    e.after = after(i);
                })
                .or_insert(Sewn { first, last, before: before(i), after: after(i) });
        }
        let doc_layer = super::resolve::DocLayer::of(&snapshot);
        let trim = mm_to_pt(super::resolve::DesignSettings::new(&super::Options::default(), &doc_layer).0.connect.trim);
        Self { generation: planned.generation, snapshot, ends, trim, planned: planned.clone(), numbers: std::sync::Mutex::default() }
    }

    /// The record numbers the plan gives path `id`'s needle points, in order (`None`: the plan
    /// sews no unit of it as its needle points), worked out the first time it is asked.
    fn numbers_of(&self, id: NodeId) -> Option<Arc<Vec<usize>>> {
        // A lock poisoned by a panic elsewhere: no numbers, never a second panic.
        let mut kept = self.numbers.lock().ok()?;
        if let Some(v) = kept.get(&id) {
            return v.clone();
        }
        let v = numbered(&self.planned.plan, id).map(Arc::new);
        kept.insert(id, v.clone());
        v
    }
}

/// The record numbers `plan` gives path `id`'s needle points, in order, as `embroidery.stitches`
/// numbers them: its unit's own needle-down records, after the record before them where the unit
/// starts where the needle already was (its first needle point left to the unit before). `None`
/// when no unit sews the path as its needle points.
fn numbered(plan: &DesignPlan, id: NodeId) -> Option<Vec<usize>> {
    let u = plan.units.iter().find(|u| u.origin.node == id && matches!(u.origin.part, Part::Manual | Part::Literal))?;
    let block = plan.blocks.get(u.block)?;
    let mut out = Vec::new();
    if u.met {
        out.push(block.first_number + u.own.start.checked_sub(1)?);
    }
    for i in u.own.clone() {
        if block.stitches.get(i).is_some_and(|s| s.command == Command::Stitch) {
            out.push(block.first_number + i);
        }
    }
    Some(out)
}

impl crate::Session {
    /// The interface draws `planned`, the plan of `snapshot`, for document `uid` (its stitch view,
    /// or the embroidery tool's handles in the art view): the embroidery tool's Auto points and its
    /// picking by stitch read that plan, so what is drawn is what is caught (the project's settled
    /// choice). Called once a plan the interface takes in; without it (agents, tests) the tool reads
    /// the newest plan remembered.
    pub fn show_embroidery_plan(&mut self, uid: u64, snapshot: Arc<Document>, planned: super::Planned) {
        self.embroidery_plans.show(uid, snapshot, planned);
    }

    /// The embroidery tool's kept answers dropped (the tool was put away): a plan, its scene and its
    /// snapshot, which the plans' memory budget doesn't count.
    pub(crate) fn drop_embroidery_tool_facts(&mut self) {
        self.embroidery_plans.drop_facts();
    }

    /// What stitch editing edits now (the embroidery tool working inside isolation of an object
    /// holding paths of stitches, in the stitch view, `view`): the object, its paths of stitches and
    /// needle points, how many of them are selected and the selection's first and last along the
    /// thread with their record numbers in the plan shown; `None` while it isn't editing. Never
    /// plans; walks the selected needle points once (the interface asks once a revision).
    pub fn stitch_editing(&mut self, view: crate::ViewInfo) -> Option<StitchEditing> {
        if self.tool_id() != vectorcraft_tools::embroidery::ID {
            return None;
        }
        self.with_tool_cx(view, |_, cx| {
            let n = cx.embroidery.and_then(|f| f.needle_points())?;
            let mut selected = 0usize;
            let mut ends: Option<[NeedleRef; 2]> = None;
            // Along the thread: the paths in the order they are sewn, then the needle points.
            let along = |r: NeedleRef| (n.order(r.id), r.index);
            for id in cx.selection.objects.iter().copied().filter(|id| n.holds(*id)) {
                let (count, lo, hi) = match cx.selection.anchors.get(&id) {
                    Some(anchors) => {
                        let ks: Vec<usize> = anchors.iter().filter_map(|a| n.of_anchor(id, *a)).collect();
                        let lo = ks.iter().copied().min();
                        let hi = ks.iter().copied().max();
                        // A run of one needle point is two anchors: counted once.
                        let mut unique = ks;
                        unique.dedup();
                        (unique.len(), lo, hi)
                    }
                    None => (n.count(id), Some(0), n.count(id).checked_sub(1)),
                };
                selected += count;
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    let (a, b) = (NeedleRef { id, index: lo }, NeedleRef { id, index: hi });
                    ends = Some(match ends {
                        None => [a, b],
                        // The selection lists paths in the order they were selected: the first
                        // and last are the ones sewn first and last.
                        Some([f, l]) => [if along(a) < along(f) { a } else { f }, if along(l) < along(b) { b } else { l }],
                    });
                }
            }
            let ends = ends.map(|[a, b]| [(a, n.number(a)), (b, n.number(b))]);
            Some(StitchEditing { scope: n.scope(), selected, ends })
        })
    }

    /// The embroidery tool's handles as the canvas draws them, while it is the active tool (else
    /// none), in `view`: the list the tool catches ([`vectorcraft_tools::embroidery::handles`]), of
    /// the part its options show, and its options (`hover`, `focus`, `drag`). Never plans.
    pub fn embroidery_handles(&mut self, view: crate::ViewInfo) -> (Vec<vectorcraft_tools::embroidery::Handle>, serde_json::Value) {
        if self.tool_id() != vectorcraft_tools::embroidery::ID {
            return (Vec::new(), serde_json::Value::Null);
        }
        self.with_tool_cx(view, |tool, cx| {
            let opts = tool.options();
            let part = opts["part"].as_str().and_then(PartKind::from_id);
            (vectorcraft_tools::embroidery::handles(cx, part), opts)
        })
    }
}

/// What stitch editing edits now ([`crate::Session::stitch_editing`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StitchEditing {
    pub scope: NeedleScope,
    /// How many needle points being edited are selected.
    pub selected: usize,
    /// The selection's first and last needle points along the thread, each with its record number
    /// in the plan shown (`None` while that plan lags behind an edit of its path).
    pub ends: Option<[(NeedleRef, Option<usize>); 2]>,
}

/// [`ToolFacts`] (when a plan is remembered) read against the document as it is now.
pub(crate) struct FactsNow<'a> {
    facts: Option<Arc<ToolFacts>>,
    doc: &'a Document,
    /// Whether the canvas draws the stitches (the stitch view): only then does a press pick by them.
    drawn: bool,
    /// The document's settings layer, read when an angle is first asked for.
    layer: OnceCell<super::resolve::DocLayer>,
    /// The needle points stitch editing works on, while it does.
    needles: Option<NeedlesNow<'a>>,
}

impl<'a> FactsNow<'a> {
    /// The answers of `facts` (none before a first plan) and of the document `doc`, whose stitches
    /// the canvas draws or not (`drawn`).
    pub(crate) fn new(facts: Option<Arc<ToolFacts>>, doc: &'a Document, drawn: bool) -> Self {
        Self { facts, doc, drawn, layer: OnceCell::new(), needles: None }
    }
}

/// The needle points stitch editing works on ([`NeedleIndex`]), read against the document as it is,
/// with record numbers from the plan the canvas shows ([`ToolFacts`]) where it is of the path as it
/// is.
pub(crate) struct NeedlesNow<'a> {
    index: Arc<NeedleIndex>,
    facts: Option<Arc<ToolFacts>>,
    doc: &'a Document,
}

impl NeedlePoints for NeedlesNow<'_> {
    fn scope(&self) -> NeedleScope {
        self.index.scope()
    }

    fn holds(&self, id: NodeId) -> bool {
        self.index.holds(id)
    }

    fn needle_at(&self, at: Point, tol: f64) -> Option<NeedleHit> {
        self.index.needle_at(at, tol)
    }

    fn thread_at(&self, at: Point, tol: f64) -> Option<ThreadHit> {
        self.index.thread_at(at, tol)
    }

    fn point(&self, n: NeedleRef) -> Option<Point> {
        self.index.point(n)
    }

    fn step(&self, n: NeedleRef, forward: bool) -> Option<NeedleRef> {
        self.index.step(n, forward)
    }

    fn first(&self) -> Option<NeedleRef> {
        self.index.first()
    }

    fn run(&self, n: NeedleRef) -> Option<(usize, usize)> {
        self.index.run(n)
    }

    fn order(&self, id: NodeId) -> Option<usize> {
        self.index.order_of(id)
    }

    fn anchors(&self, n: NeedleRef) -> Vec<(usize, usize)> {
        self.index.anchors(n)
    }

    fn of_anchor(&self, id: NodeId, anchor: (usize, usize)) -> Option<usize> {
        self.index.of_anchor(id, anchor)
    }

    fn count(&self, id: NodeId) -> usize {
        self.index.count(id)
    }

    fn number(&self, n: NeedleRef) -> Option<usize> {
        let facts = self.facts.as_deref()?;
        // The plan is of the path as it is: the same node in its snapshot as in the document.
        let (kept, now) = (facts.snapshot.node(n.id)?, self.doc.node(n.id)?);
        if !std::ptr::eq(kept, now) {
            return None;
        }
        let numbers = facts.numbers_of(n.id)?;
        if numbers.len() != self.index.count(n.id) {
            return None;
        }
        numbers.get(n.index).copied()
    }
}

impl super::service::Services {
    /// The embroidery tool's answers for the document `st` while `tool` is the active tool (none
    /// for another tool): from a plan already made with the stitch preview's options, and whether
    /// the canvas draws the stitches (`outline`: the outline view, which draws none).
    pub(crate) fn facts_now<'a>(&mut self, tool: &str, st: &'a crate::DocState, outline: bool) -> Option<FactsNow<'a>> {
        if tool != vectorcraft_tools::embroidery::ID {
            return None;
        }
        let facts = self.tool_facts(st.uid, &super::Options::default());
        let drawn = super::view::layers(self.view(st.uid), super::view::paused(&st.doc), outline).stitches;
        // Needle points are edited where they are drawn: in the stitch view.
        let needles = if drawn { self.needle_index(st) } else { None };
        let mut now = FactsNow::new(facts.clone(), &st.doc, drawn);
        now.needles = needles.map(|index| NeedlesNow { index, facts, doc: &st.doc });
        Some(now)
    }
}

impl PlanFacts for FactsNow<'_> {
    fn generation(&self) -> u64 {
        self.facts.as_ref().map_or(0, |f| f.generation)
    }

    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds> {
        let facts = self.facts.as_deref()?;
        let sewn = facts.ends.get(&(object, part))?;
        // Of the document as it is: a plan of an earlier snapshot is stale for every object (an
        // edit of a neighbour, or of the document's settings, moves Auto points too).
        let current = std::ptr::eq(&*facts.snapshot, self.doc);
        let trim = facts.trim.is_finite().then_some(facts.trim);
        Some(PlannedEnds { first: sewn.first, last: sewn.last, current, before: sewn.before, after: sewn.after, trim })
    }

    fn part_at(&self, at: Point, tol: f64) -> Option<(NodeId, Option<PartKind>)> {
        // In the art view a press picks by the art alone: the stitches aren't drawn.
        if !self.drawn {
            return None;
        }
        self.facts.as_deref()?.planned.scene().pick(at, tol)
    }

    fn snap(&self, object: NodeId, part: PartKind, at: Point) -> Option<Point> {
        Some(super::points::snapped(self.doc.node(object)?, part, at))
    }

    fn fill_angle(&self, object: NodeId) -> Option<FillAngle> {
        let n = self.doc.node(object)?;
        let layer = self.layer.get_or_init(|| super::resolve::DocLayer::of(self.doc));
        let (degrees, set) = super::info::fill_rows(self.doc, n, &super::Options::default(), layer)?;
        Some(FillAngle { degrees, set })
    }

    fn needle_points(&self) -> Option<&dyn NeedlePoints> {
        self.needles.as_ref().map(|n| n as &dyn NeedlePoints)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_doc::NodeId;
    use vectorcraft_doc::embroidery::PartKind;
    use vectorcraft_tools::embroidery::PlanFacts;

    use super::super::test_support::{PT_PER_MM, blank, run};
    use super::*;

    /// A part's answer is where `embroidery.sequence` says its first unit starts and its last ends;
    /// after an edit of the object, the same answer from the plan before, not current.
    #[test]
    fn a_parts_ends_are_where_the_plan_sews_it() {
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}))["id"].as_u64().unwrap();
        let unit = run(&mut s, "embroidery.sequence", json!({}))["blocks"][0]["units"][0].clone();
        let opts = super::super::Options::default();
        let uid = s.doc().unwrap().uid;
        let facts = s.embroidery_plans.tool_facts(uid, &opts).expect("the plan the sequence made");
        let doc = s.doc().unwrap().doc.clone();
        let now = FactsNow::new(Some(facts.clone()), &doc, true);
        let e = now.ends(NodeId(id), PartKind::Fill).unwrap();
        let mm = |v: f64| v / PT_PER_MM;
        assert!(
            (mm(e.first.x) - unit["entry"][0].as_f64().unwrap()).abs() < 0.01 && (mm(e.last.y) - unit["exit"][1].as_f64().unwrap()).abs() < 0.01,
            "{e:?} {unit}"
        );
        assert!(e.current);
        assert_eq!(now.ends(NodeId(id), PartKind::Stroke), None, "no stroke sewn");
        run(&mut s, "object.move", json!({"ids": [id], "dx": 10, "dy": 0}));
        let doc = s.doc().unwrap().doc.clone();
        let facts = s.embroidery_plans.tool_facts(uid, &opts).expect("the plan before, still remembered");
        let now = FactsNow::new(Some(facts.clone()), &doc, true);
        assert!(!now.ends(NodeId(id), PartKind::Fill).unwrap().current, "of the object as it was");
    }

    /// Through the session: with the embroidery tool, a drag of a fill's Auto entry (where the plan
    /// sews it) to the middle of its top side sets the entry there, snapped onto the outline, one
    /// undo step; the facts are filled only while the tool is active.
    #[test]
    fn the_tool_drags_an_auto_entry_through_the_session() {
        use crate::ViewInfo;
        use vectorcraft_tools::{PointerEvent, PointerKind};
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": [id]}));
        run(&mut s, "embroidery.sequence", json!({}));
        let v = ViewInfo::default();
        s.select_tool("embroidery", v).unwrap();
        // Pressed on the entry's handle: the drag moves the point by the drag, from where it is.
        let (handles, _) = s.embroidery_handles(v);
        let entry = handles.iter().find(|h| h.id.kind == vectorcraft_tools::embroidery::Kind::Entry).expect("the entry's handle");
        let (x, y) = (entry.at.x, entry.at.y);
        let undo = s.doc().unwrap().history.undo.len();
        for (kind, at) in
            [(PointerKind::Down, (x, y)), (PointerKind::Drag, (55.0, 25.0)), (PointerKind::Drag, (60.0, 23.0)), (PointerKind::Up, (60.0, 23.0))]
        {
            s.pointer(&PointerEvent::new(kind, at.0, at.1), v).unwrap();
        }
        let fill = run(&mut s, "embroidery.info", json!({"ids": [id]}))["objects"][0]["settings"]["fill"].clone();
        assert_eq!(fill["entry"], json!([60.0, 20.0]), "snapped onto the top side: {fill}");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    }

    /// Along the thread: the second square's part comes from where the first ends, and the first
    /// goes on to where the second starts; across a colour change, from nowhere; the design's trim
    /// length comes with them.
    #[test]
    fn a_parts_neighbours_along_the_thread() {
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let a = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
        let b = run(&mut s, "shape.rectangle", json!({"x": 100, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": []}));
        run(&mut s, "paint.setFill", json!({"color": "#2233cc"}));
        let c = run(&mut s, "shape.rectangle", json!({"x": 180, "y": 20, "width": 60, "height": 40}))["id"].as_u64().unwrap();
        run(&mut s, "embroidery.sequence", json!({}));
        let uid = s.doc().unwrap().uid;
        let facts = s.embroidery_plans.tool_facts(uid, &super::super::Options::default()).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let now = FactsNow::new(Some(facts.clone()), &doc, true);
        let ends = |id: u64| now.ends(NodeId(id), PartKind::Fill).unwrap();
        let (ea, eb, ec) = (ends(a), ends(b), ends(c));
        assert_eq!((ea.before, ea.after, eb.before), (None, Some(eb.first), Some(ea.last)));
        assert_eq!((eb.after, ec.before), (None, None), "a colour change between");
        assert!((ea.trim.unwrap() - 10.0 * PT_PER_MM).abs() < 1e-9, "the default 10 mm: {:?}", ea.trim);
    }

    /// A press on the stitches picks the object sewn on top there: of two overlapping squares, the
    /// later where they overlap, the earlier beside it, none off the stitches, and none in the art
    /// view (the stitches aren't drawn).
    #[test]
    fn a_press_on_the_stitches_picks_the_object_on_top() {
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let a = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 80}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": []}));
        run(&mut s, "paint.setFill", json!({"color": "#2233cc"}));
        let b = run(&mut s, "shape.rectangle", json!({"x": 60, "y": 60, "width": 80, "height": 80}))["id"].as_u64().unwrap();
        run(&mut s, "embroidery.sequence", json!({}));
        let uid = s.doc().unwrap().uid;
        let facts = s.embroidery_plans.tool_facts(uid, &super::super::Options::default()).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let now = FactsNow::new(Some(facts.clone()), &doc, true);
        // Rows lie 0.38 mm apart: a point's tolerance is wider than the gaps between them.
        let tol = 1.0;
        let fill = Some(PartKind::Fill);
        assert_eq!(now.part_at(Point::new(80.0, 80.0), tol), Some((NodeId(b), fill)), "where they overlap");
        assert_eq!(now.part_at(Point::new(35.0, 35.0), tol), Some((NodeId(a), fill)));
        assert_eq!(now.part_at(Point::new(200.0, 200.0), tol), None);
        let art = FactsNow::new(Some(facts.clone()), &doc, false);
        assert_eq!(art.part_at(Point::new(80.0, 80.0), tol), None, "in the art view, by the art alone");
    }

    /// The tool put away (another tool chosen), the answers it kept go: they hold a plan the plans'
    /// memory budget no longer counts. Taken up again, it answers afresh.
    #[test]
    fn put_away_the_tool_keeps_no_answers() {
        use crate::ViewInfo;
        use vectorcraft_tools::{PointerEvent, PointerKind};
        let mut s = blank();
        run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}));
        run(&mut s, "embroidery.sequence", json!({}));
        let v = ViewInfo::default();
        s.select_tool("embroidery", v).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Move, 30.0, 30.0), v).unwrap();
        assert_eq!(s.embroidery_plans.kept_facts(), 1, "kept while it is in use");
        s.select_tool("selection", v).unwrap();
        assert_eq!(s.embroidery_plans.kept_facts(), 0);
        s.select_tool("embroidery", v).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Move, 30.0, 30.0), v).unwrap();
        assert_eq!(s.embroidery_plans.kept_facts(), 1);
    }

    /// No plan remembered, no answers: nothing plans to answer.
    #[test]
    fn no_plan_no_answers() {
        let mut s = blank();
        run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}));
        let uid = s.doc().unwrap().uid;
        assert!(s.embroidery_plans.tool_facts(uid, &super::super::Options::default()).is_none());
    }

    /// Through the session, with no plan made: a filled rectangle's rows show as an Auto arrow at
    /// the default 135°; turned 30° (counter-clockwise), its Auto arrow turns with it to 165°, as
    /// `embroidery.info {plan: false}` reports the angle; an angle set is the arrow's, set; a
    /// dragged head sets the angle it points at, one undo step.
    #[test]
    fn an_object_turned_turns_its_auto_arrow() {
        use crate::ViewInfo;
        use vectorcraft_tools::embroidery::{Kind, State};
        use vectorcraft_tools::{PointerEvent, PointerKind};
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": [id]}));
        let v = ViewInfo::default();
        s.select_tool("embroidery", v).unwrap();
        let arrow = |s: &mut crate::Session| {
            let (hs, _) = s.embroidery_handles(v);
            let h = hs.into_iter().find(|h| h.id.kind == Kind::Angle).expect("the fill's arrow");
            (h.state, h.arrow.unwrap().degrees, h.point)
        };
        let reported = |s: &mut crate::Session| run(s, "embroidery.info", json!({"ids": [id]}))["fill"]["settings"]["angle"]["value"].clone();
        assert_eq!(arrow(&mut s).0, State::Auto);
        assert!((arrow(&mut s).1 - 135.0).abs() < 1e-9, "{:?}", arrow(&mut s));
        run(&mut s, "object.rotate", json!({"ids": [id], "angle": 30}));
        let (state, degrees, _) = arrow(&mut s);
        assert_eq!(state, State::Auto);
        assert!((degrees - 165.0).abs() < 1e-9, "turned with the object: {degrees}");
        assert!((reported(&mut s).as_f64().unwrap() - degrees).abs() < 1e-9, "as info reports it");
        run(&mut s, "embroidery.set", json!({"ids": [id], "part": "fill", "angle": 20}));
        assert_eq!((arrow(&mut s).0, arrow(&mut s).1), (State::Set, 20.0));
        // The head dragged to straight above the centre: 90°, one undo step.
        let (_, _, head) = arrow(&mut s);
        let centre = s.doc().unwrap().doc.bounds_of(&[NodeId(id)], false).unwrap().center();
        let undo = s.doc().unwrap().history.undo.len();
        for (kind, at) in [
            (PointerKind::Down, head),
            (PointerKind::Drag, Point::new(centre.x, centre.y - 60.0)),
            (PointerKind::Up, Point::new(centre.x, centre.y - 60.0)),
        ] {
            s.pointer(&PointerEvent::new(kind, at.x, at.y), v).unwrap();
        }
        assert_eq!(reported(&mut s), json!(90.0));
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
    }

    /// Undo mid-drag settles the drag and takes it back: the tool lets the handle go (not busy),
    /// the moves after it preview nothing, and the release changes nothing.
    #[test]
    fn undo_mid_drag_lets_the_handle_go() {
        use vectorcraft_tools::{PointerEvent, PointerKind};
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let id = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}))["id"].as_u64().unwrap();
        run(&mut s, "embroidery.setEntryExit", json!({"id": id, "part": "fill", "entry": [20, 40]}));
        run(&mut s, "select.set", json!({"ids": [id]}));
        let view = crate::ViewInfo::default();
        s.select_tool("embroidery", view).unwrap();
        let entry = s.embroidery_handles(view).0.into_iter().find(|h| h.id.kind == vectorcraft_tools::embroidery::Kind::Entry).unwrap().at;
        let undo = s.doc().unwrap().history.undo.len();
        s.pointer(&PointerEvent::new(PointerKind::Down, entry.x, entry.y), view).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Drag, entry.x, entry.y + 15.0), view).unwrap();
        assert!(s.tool_busy(), "dragging");
        run(&mut s, "edit.undo", json!({}));
        assert!(!s.tool_busy(), "the handle let go");
        s.pointer(&PointerEvent::new(PointerKind::Drag, entry.x, entry.y + 25.0), view).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Up, entry.x, entry.y + 25.0), view).unwrap();
        assert!(s.doc().unwrap().interaction.is_none() && s.doc().unwrap().history.undo.len() <= undo, "nothing kept");
    }

    /// A press on a symbol's instance selects it (it takes Trim After, the Object tab offers it),
    /// though it shows no handle: the selection isn't emptied.
    #[test]
    fn a_press_on_a_symbol_instance_selects_it() {
        use vectorcraft_tools::{PointerEvent, PointerKind};
        let mut s = blank();
        run(&mut s, "paint.setStroke", json!({"none": true}));
        run(&mut s, "paint.setFill", json!({"color": "#cc2222"}));
        let square = run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 40, "height": 40}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": [square]}));
        run(&mut s, "symbol.new", json!({"name": "Square"}));
        let placed = run(&mut s, "symbol.place", json!({"name": "Square", "x": 200, "y": 200}))["id"].as_u64().unwrap();
        run(&mut s, "select.set", json!({"ids": []}));
        let view = crate::ViewInfo::default();
        s.select_tool("embroidery", view).unwrap();
        let at = s.doc().unwrap().doc.bounds_of(&[NodeId(placed)], false).unwrap().center();
        s.pointer(&PointerEvent::new(PointerKind::Down, at.x, at.y), view).unwrap();
        s.pointer(&PointerEvent::new(PointerKind::Up, at.x, at.y), view).unwrap();
        assert_eq!(s.doc().unwrap().selection.objects, vec![NodeId(placed)]);
    }
}
