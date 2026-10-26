//! What the embroidery tool reads of the stitch plan ([`vectorcraft_tools::embroidery::PlanFacts`]):
//! the engine answers from the newest plan it remembers of the whole document with the stitch
//! preview's options (the plan the canvas shows), built once a plan ([`ToolFacts`]) and never
//! planning to answer. Each answer says whether the plan is of the object as it is now, by node
//! identity against the snapshot planned (an edit replaces the node), as the canvas's markers judge
//! it.

use std::collections::HashMap;
use std::sync::Arc;

use vectorcraft_doc::embroidery::PartKind;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;
use vectorcraft_stitch::{Command, mm_to_pt};
use vectorcraft_tools::embroidery::{PlanFacts, PlannedEnds};

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
        // Each unit's first and last needle points, its block, and whether it ends with a cut.
        let units: Vec<Option<(Point, Point, usize, bool)>> = (0..plan.units.len())
            .map(|i| {
                let stitches = plan.unit_stitches(i);
                let mut needle = stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| in_points(s.point()));
                let first = needle.next()?;
                let cut = stitches.last().is_some_and(|s| s.command == Command::Trim);
                Some((first, needle.next_back().unwrap_or(first), plan.units.get(i)?.block, cut))
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
        Self { generation: planned.generation, snapshot, ends, trim }
    }
}

impl crate::Session {
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

/// [`ToolFacts`] read against the document as it is now.
pub(crate) struct FactsNow<'a> {
    pub(crate) facts: &'a ToolFacts,
    pub(crate) doc: &'a Document,
}

impl PlanFacts for FactsNow<'_> {
    fn generation(&self) -> u64 {
        self.facts.generation
    }

    fn ends(&self, object: NodeId, part: PartKind) -> Option<PlannedEnds> {
        let sewn = self.facts.ends.get(&(object, part))?;
        let current = match (self.doc.node(object), self.facts.snapshot.node(object)) {
            (Some(a), Some(b)) => std::ptr::eq(a, b),
            _ => false,
        };
        let trim = self.facts.trim.is_finite().then_some(self.facts.trim);
        Some(PlannedEnds { first: sewn.first, last: sewn.last, current, before: sewn.before, after: sewn.after, trim })
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
        let now = FactsNow { facts: &facts, doc: &doc };
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
        let now = FactsNow { facts: &facts, doc: &doc };
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
        let unit = run(&mut s, "embroidery.sequence", json!({}))["blocks"][0]["units"][0].clone();
        let (x, y) = (unit["entry"][0].as_f64().unwrap() * PT_PER_MM, unit["entry"][1].as_f64().unwrap() * PT_PER_MM);
        let v = ViewInfo::default();
        s.select_tool("embroidery", v).unwrap();
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
        let now = FactsNow { facts: &facts, doc: &doc };
        let ends = |id: u64| now.ends(NodeId(id), PartKind::Fill).unwrap();
        let (ea, eb, ec) = (ends(a), ends(b), ends(c));
        assert_eq!((ea.before, ea.after, eb.before), (None, Some(eb.first), Some(ea.last)));
        assert_eq!((eb.after, ec.before), (None, None), "a colour change between");
        assert!((ea.trim.unwrap() - 10.0 * PT_PER_MM).abs() < 1e-9, "the default 10 mm: {:?}", ea.trim);
    }

    /// No plan remembered, no answers: nothing plans to answer.
    #[test]
    fn no_plan_no_answers() {
        let mut s = blank();
        run(&mut s, "shape.rectangle", json!({"x": 20, "y": 20, "width": 80, "height": 50}));
        let uid = s.doc().unwrap().uid;
        assert!(s.embroidery_plans.tool_facts(uid, &super::super::Options::default()).is_none());
    }
}
