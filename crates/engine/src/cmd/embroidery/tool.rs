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
use vectorcraft_tools::embroidery::{FillAngle, PlanFacts, PlannedEnds};

use super::plan::Part;
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
                Part::Literal => continue,
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
        Self { generation: planned.generation, snapshot, ends, trim, planned: planned.clone() }
    }
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

    /// The embroidery tool's kept answers dropped (the tool was put away).
    pub fn drop_embroidery_tool_facts(&mut self) {
        self.embroidery_plans.drop_facts();
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

/// [`ToolFacts`] (when a plan is remembered) read against the document as it is now.
pub(crate) struct FactsNow<'a> {
    facts: Option<&'a ToolFacts>,
    doc: &'a Document,
    /// Whether the canvas draws the stitches (the stitch view): only then does a press pick by them.
    drawn: bool,
    /// The document's settings layer, read when an angle is first asked for.
    layer: OnceCell<super::resolve::DocLayer>,
}

impl<'a> FactsNow<'a> {
    /// The answers of `facts` (none before a first plan) and of the document `doc`, whose stitches
    /// the canvas draws or not (`drawn`).
    pub(crate) fn new(facts: Option<&'a ToolFacts>, doc: &'a Document, drawn: bool) -> Self {
        Self { facts, doc, drawn, layer: OnceCell::new() }
    }
}

impl PlanFacts for FactsNow<'_> {
    fn generation(&self) -> u64 {
        self.facts.map_or(0, |f| f.generation)
    }

    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds> {
        let facts = self.facts?;
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
        self.facts?.planned.scene().pick(at, tol)
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
        let now = FactsNow::new(Some(&facts), &doc, true);
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
        let now = FactsNow::new(Some(&facts), &doc, true);
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
        let now = FactsNow::new(Some(&facts), &doc, true);
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
        let now = FactsNow::new(Some(&facts), &doc, true);
        // Rows lie 0.38 mm apart: a point's tolerance is wider than the gaps between them.
        let tol = 1.0;
        let fill = Some(PartKind::Fill);
        assert_eq!(now.part_at(Point::new(80.0, 80.0), tol), Some((NodeId(b), fill)), "where they overlap");
        assert_eq!(now.part_at(Point::new(35.0, 35.0), tol), Some((NodeId(a), fill)));
        assert_eq!(now.part_at(Point::new(200.0, 200.0), tol), None);
        let art = FactsNow::new(Some(&facts), &doc, false);
        assert_eq!(art.part_at(Point::new(80.0, 80.0), tol), None, "in the art view, by the art alone");
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
