//! `embroidery.nearestConnection`: entry and exit points set on the design's parts (the objects
//! given, else the selected ones, else every object) for the shortest moves between them, in the
//! sewing order as it is.
//!
//! - **The chain.** The plan's units in sewing order ([`DesignPlan::units`]), each with where it can
//!   start and end (the walk records [`plan::ends`] of each, [`super::digitize::walked`]) and how the
//!   needle comes to it: along the thread inside a block, or across a cut (a colour change, a Trim
//!   After), where only the frame's travel counts.
//! - **What it may choose.** A part in scope that can take points of its own (one unit for its kind
//!   of part, not a symbol's art, not a stroke a clip group cuts, not of a shape the planner's rules
//!   there don't say) is free: any of its starts and ends. A part with no points set outside the scope
//!   follows (it starts nearest the end before it); the rest are sewn as they are.
//! - **The search.** [`plan::joins`]: exact over the points offered, each part's own start and end
//!   first, so that ties keep them.
//! - **Checked by planning.** The points are written on a copy, the copy planned (the units kept from
//!   earlier plans), and a stretch between two cuts that saves less than [`MIN_JOIN_GAIN`] as
//!   planned, or comes out longer, gets its old points back (a part that had none, where it started
//!   and ended). The search then runs again on its own result until a round changes nothing (at most
//!   [`MAX_ROUNDS`]), so a second run changes nothing. One undo step, none when nothing changes;
//!   `dryRun` answers without writing.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{PartKey, PartKind, PartValue};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;
use vectorcraft_stitch::plan::{self, Ends, JoinShape, Link, Role, Through};
use vectorcraft_stitch::{Command, SAME_POINT, mm_to_pt};

use super::digitize::{UnitEnds, Walked};
use super::objects::{edit_each, given};
use super::plan::{DesignPlan, Origin, Part};
use super::unit::PlanError;
use super::{object, options_in};
use crate::cmd::bad;
use crate::{Result, Session};

/// The command's id.
const C: &str = "embroidery.nearestConnection";

/// The undo step it makes, and its label.
pub(super) const LABEL: &str = "Nearest Connection";

/// The least a stretch of the design's moves (between two cuts) must save to change its points, in
/// the planner's units (about millimetres of move): a tenth of a millimetre, the formats' step. Less
/// is rounding, and a second run changes nothing (an estimate).
const MIN_JOIN_GAIN: f64 = 0.1;

/// The most parts and skipped objects a reply lists.
const MAX_LISTED: usize = 1000;

/// One unit of the chain: what the search may choose of it, and for a free one the part its points
/// are written on and what is written for its own start and end (the points it has, else where it
/// starts and ends now).
struct Joint {
    shape: JoinShape,
    part: Option<(NodeId, PartKind)>,
    own: (Point, Point),
}

/// The kind of part a unit sews, `None` for a literal run.
fn kind_of(part: Part) -> Option<PartKind> {
    match part {
        Part::Fill(_) => Some(PartKind::Fill),
        Part::Stroke(_) => Some(PartKind::Stroke),
        Part::Literal => None,
    }
}

/// Unit `i`'s first and last needle points (millimetres), `None` when it has none.
fn needle_ends(plan: &DesignPlan, i: usize) -> Option<(Point, Point)> {
    let mut needle = plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch).map(|s| s.point());
    let first = needle.next()?;
    Some((first, needle.next_back().unwrap_or(first)))
}

/// How the needle comes to each unit of `plan` that has needle points of its own: `None` for a unit
/// with none (left out of the chain), else the first's [`Link::First`], a cut after a colour change
/// or after a unit that ends with a cut, the thread otherwise.
fn links(plan: &DesignPlan) -> Vec<Option<Link>> {
    let mut out = Vec::with_capacity(plan.units.len());
    let mut before: Option<(usize, bool)> = None;
    for (i, u) in plan.units.iter().enumerate() {
        if needle_ends(plan, i).is_none() {
            out.push(None);
            continue;
        }
        out.push(Some(match before {
            None => Link::First,
            Some((block, cut)) if cut || block != u.block => Link::Cut,
            Some(_) => Link::Thread,
        }));
        let cut = plan.unit_stitches(i).last().is_some_and(|s| s.command == Command::Trim);
        before = Some((u.block, cut));
    }
    out
}

/// The chain of `walked`'s plan: each unit with needle points of its own, by its index, as a
/// [`Joint`]; the objects in `scope` (`None`: every object) that it can't join, with why, into
/// `skipped`.
fn chain(walked: &Walked, scope: Option<&HashSet<NodeId>>, skipped: &mut Vec<(NodeId, &'static str)>) -> Vec<(usize, Joint)> {
    let plan = &walked.plan;
    let recorded: HashMap<(Origin, usize), &UnitEnds> = walked.ends.iter().map(|e| ((e.origin, e.occurrence), e)).collect();
    // How many units each object's kind of part is sewn as: one pair of points can serve one.
    let mut units_of: HashMap<(NodeId, PartKind), usize> = HashMap::new();
    for u in &plan.units {
        if let Some(kind) = kind_of(u.origin.part) {
            *units_of.entry((u.origin.selected(), kind)).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for ((i, u), link) in plan.units.iter().enumerate().zip(links(plan)) {
        let (Some(link), Some((first, last))) = (link, needle_ends(plan, i)) else { continue };
        let id = u.origin.selected();
        let in_scope = scope.is_none_or(|sc| sc.contains(&id));
        let kind = kind_of(u.origin.part);
        let record = recorded.get(&(u.origin, u.occurrence)).copied();
        let why = match (kind, record) {
            (None, _) => Some("literal"),
            (Some(_), None) => Some("unmodelled"),
            (Some(k), Some(r)) => r
                .fixed
                .or_else(|| (units_of.get(&(id, k)).copied().unwrap_or(0) > 1).then_some("pieces"))
                .or_else(|| r.ends.is_none().then_some("unmodelled")),
        };
        let free = in_scope && why.is_none();
        let ends = record.and_then(|r| r.ends.clone());
        let (shape, part) = match (free, kind, ends) {
            (true, Some(k), Some(mut e)) => {
                // Its own points first: those it holds (where the points it can take lie, as the
                // others do), else where it starts and ends now.
                e.own_first(u.asked.entry.unwrap_or(first), u.asked.exit.unwrap_or(last));
                (JoinShape { ends: e, link, role: Role::Free }, Some((id, k)))
            }
            // A line or a column with no points set follows the end before it; a fill's end, and
            // the rest, as they are now.
            (_, _, Some(e)) if u.asked.entry.is_none() && u.asked.exit.is_none() && e.through != Through::Free && why != Some("literal") => {
                let mut e = e.natural();
                e.own_first(first, last);
                (JoinShape { ends: e, link, role: Role::Follows }, None)
            }
            _ => (JoinShape { ends: Ends::fixed(first, last), link, role: Role::Free }, None),
        };
        if in_scope && let Some(why) = why.filter(|_| !free) {
            skipped.push((id, why));
        }
        let own = (
            u.asked.entry.unwrap_or(shape.ends.starts.first().copied().unwrap_or(first)),
            u.asked.exit.unwrap_or(shape.ends.ends.first().copied().unwrap_or(last)),
        );
        out.push((i, Joint { shape, part, own }));
    }
    out
}

/// The stretches of `chain` (each from a [`Link::First`] or [`Link::Cut`] to the next), as index
/// ranges.
fn stretches(chain: &[(usize, Joint)]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    for (k, (_, j)) in chain.iter().enumerate().skip(1) {
        if j.shape.link != Link::Thread {
            out.push(from..k);
            from = k;
        }
    }
    if from < chain.len() {
        out.push(from..chain.len());
    }
    out
}

/// What a plan's moves between its units inside blocks come to: how many moves, how many lift the
/// needle (a jump), how many are cut (longer than the trim length), their length and the longest
/// (millimetres), and the cost the search weighs them by.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Moves {
    connections: usize,
    jumps: usize,
    trims: usize,
    length: f64,
    longest: f64,
    cost: f64,
}

impl Moves {
    fn add(&mut self, d: f64, trim: f64) {
        self.connections += 1;
        self.jumps += usize::from(d > SAME_POINT);
        self.trims += usize::from(d > trim);
        self.length += d;
        self.longest = self.longest.max(d);
        self.cost += plan::move_cost(d, Link::Thread, trim);
    }

    fn json(&self) -> Value {
        json!({"connections": self.connections, "jumps": self.jumps, "trims": self.trims, "length": round_mm(self.length), "longest": round_mm(self.longest)})
    }
}

/// `v` to a hundredth of a millimetre, as the replies give lengths.
fn round_mm(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A stretch of a plan's units between two cuts: its units' keys, and its moves.
type Stretch = (Vec<(Origin, usize)>, Moves);

/// The moves of `plan` inside its blocks, stretch by stretch (as [`links`] splits them).
fn measured(plan: &DesignPlan, trim: f64) -> Vec<Stretch> {
    let mut out: Vec<Stretch> = Vec::new();
    let mut before: Option<Point> = None;
    for ((i, u), link) in plan.units.iter().enumerate().zip(links(plan)) {
        let (Some(link), Some((first, last))) = (link, needle_ends(plan, i)) else { continue };
        if link != Link::Thread || out.is_empty() {
            out.push((Vec::new(), Moves::default()));
        }
        let Some((keys, moves)) = out.last_mut() else { continue };
        keys.push((u.origin, u.occurrence));
        if let (Link::Thread, Some(b)) = (link, before) {
            moves.add(b.distance(first), trim);
        }
        before = Some(last);
    }
    out
}

/// The moves of every stretch added up.
fn total(stretches: &[Stretch]) -> Moves {
    let mut t = Moves::default();
    for (_, m) in stretches {
        t.connections += m.connections;
        t.jumps += m.jumps;
        t.trims += m.trims;
        t.length += m.length;
        t.longest = t.longest.max(m.longest);
        t.cost += m.cost;
    }
    t
}

/// A point in millimetres as the document keeps it: in points.
fn in_points(p: Point) -> Point {
    Point::new(mm_to_pt(p.x), mm_to_pt(p.y))
}

/// The entry and exit `part` of object `id` holds in `d`.
fn held(d: &Document, id: NodeId, kind: PartKind) -> (Option<PartValue>, Option<PartValue>) {
    let part = d.node(id).and_then(|n| n.extras().embroidery.as_deref()?.part(kind).cloned());
    (part.as_ref().and_then(|p| p.get(PartKey::Entry)), part.as_ref().and_then(|p| p.get(PartKey::Exit)))
}

/// Set `entry` and `exit` on part `kind` of object `id` in `d` → whether that changed it.
fn write(d: &mut Document, id: NodeId, kind: PartKind, entry: Option<PartValue>, exit: Option<PartValue>) -> Result<bool> {
    let changed = edit_each(d, &[id], |e| {
        e.edit_part(kind, |part| {
            part.set(PartKey::Entry, entry.clone());
            part.set(PartKey::Exit, exit.clone());
        })
    })?;
    Ok(changed > 0)
}

/// What one round of the search did to a copy of the document: the copy with its parts' new points
/// (each stretch that saved too little as planned given its old points back), the parts written
/// (each its unit's index in the plan before, its object, its kind of part), the moves before and
/// after, and how many stretches came out longer and were restored.
struct Round {
    doc: Document,
    parts: Vec<(usize, NodeId, PartKind)>,
    before: Vec<Stretch>,
    after: Vec<Stretch>,
    restored: usize,
}

/// How many rounds the search runs on its own result at most, until one changes nothing: each
/// round's points, once planned, can open a better join that the points offered only stood near
/// (an estimate; a second command then finds nothing left).
const MAX_ROUNDS: usize = 3;

/// One round of the search on `doc` (planned as `walked`), its parts in `scope` (`None`: every
/// object), with `service`'s kept units and the export's `opts`; what it can't join, with why, into
/// `skipped` when asked.
fn round(
    service: &super::service::PlanService,
    doc: &Document,
    walked: &Walked,
    scope: Option<&HashSet<NodeId>>,
    opts: &super::Options,
    skipped: &mut Vec<(NodeId, &'static str)>,
) -> Result<Round> {
    let trim = walked.trim;
    let joints = chain(walked, scope, skipped);
    let was = measured(&walked.plan, trim);
    // The search, over points thinned where the chain is too long to weigh them all.
    let mut shapes: Vec<JoinShape> = joints.iter().map(|(_, j)| j.shape.clone()).collect();
    if let Some(n) = plan::thinned_to(&shapes) {
        shapes.iter_mut().filter(|sh| sh.role == Role::Free).for_each(|sh| sh.ends = sh.ends.thinned(n));
    }
    let mut chosen_points: Vec<(usize, usize)> = match plan::joins(&shapes, trim) {
        Some(j) => j.chosen,
        None => vec![(0, 0); shapes.len()],
    };
    // A stretch the search saves too little on keeps its own points.
    for r in stretches(&joints) {
        let (Some(part), Some(chosen)) = (shapes.get(r.clone()), chosen_points.get(r.clone())) else { continue };
        let own = vec![(0, 0); part.len()];
        let (Some(new), Some(old)) = (plan::joined_cost(part, chosen, trim), plan::joined_cost(part, &own, trim)) else { continue };
        if old - new < MIN_JOIN_GAIN
            && let Some(c) = chosen_points.get_mut(r)
        {
            c.iter_mut().for_each(|x| *x = (0, 0));
        }
    }
    // Written on a copy: each free part's chosen points, in document points.
    let mut d = doc.clone();
    let mut parts: Vec<(usize, NodeId, PartKind)> = Vec::new();
    for (k, ((i, j), shape)) in joints.iter().zip(&shapes).enumerate() {
        let Some((id, kind)) = j.part else { continue };
        let Some(&(s_, e_)) = chosen_points.get(k) else { continue };
        let (entry, exit) = if (s_, e_) == (0, 0) {
            // Its own points: those it holds kept as they are (not taken through millimetres and
            // back), where it starts and ends now for those it doesn't.
            let (held_entry, held_exit) = held(doc, id, kind);
            (held_entry.unwrap_or(PartValue::Point(in_points(j.own.0))), held_exit.unwrap_or(PartValue::Point(in_points(j.own.1))))
        } else {
            let (entry, exit) = (shape.ends.starts.get(s_).copied().unwrap_or(j.own.0), shape.ends.ends.get(e_).copied().unwrap_or(j.own.1));
            (PartValue::Point(in_points(entry)), PartValue::Point(in_points(exit)))
        };
        write(&mut d, id, kind, Some(entry), Some(exit))?;
        parts.push((*i, id, kind));
    }
    // Checked by planning the copy: a stretch that saves less than MIN_JOIN_GAIN as planned (or
    // comes out longer) keeps its points. Where the points offered only stand near where a part
    // starts now, that also keeps a later run from moving it for nothing. Those that came out longer
    // are counted as restored.
    let plan_of = |d: &Document| -> Result<Walked> { service.walk_kept(&d.without_edit_modes(), opts, false).map_err(PlanError::error) };
    let mut now = measured(&plan_of(&d)?.plan, trim);
    let same_shape = was.len() == now.len() && was.iter().zip(&now).all(|(a, b)| a.0 == b.0);
    let saves = |a: &Moves, b: &Moves| a.cost - b.cost >= MIN_JOIN_GAIN;
    let longer = |a: &Moves, b: &Moves| b.cost > a.cost + 1e-9;
    let (back, all_back, restored): (HashSet<(Origin, usize)>, bool, usize) = if same_shape {
        let keys = was.iter().zip(&now).filter(|(a, b)| !saves(&a.1, &b.1)).flat_map(|(a, _)| a.0.iter().copied()).collect();
        (keys, false, was.iter().zip(&now).filter(|(a, b)| longer(&a.1, &b.1)).count())
    } else if !saves(&total(&was), &total(&now)) {
        (HashSet::new(), true, if longer(&total(&was), &total(&now)) { was.len() } else { 0 })
    } else {
        (HashSet::new(), false, 0)
    };
    if all_back || !back.is_empty() {
        for (i, id, kind) in &parts {
            let key = walked.plan.units.get(*i).map(|u| (u.origin, u.occurrence));
            if all_back || key.is_some_and(|k| back.contains(&k)) {
                // The points it held; one it didn't hold is set where it started or ended before:
                // left to Auto, it would start nearest wherever the needle now is (across a colour
                // change, the end of the object drawn before it, which may have moved).
                let (entry, exit) = held(doc, *id, *kind);
                let was = needle_ends(&walked.plan, *i);
                let entry = entry.or_else(|| was.map(|(first, _)| PartValue::Point(in_points(first))));
                let exit = exit.or_else(|| was.map(|(_, last)| PartValue::Point(in_points(last))));
                write(&mut d, *id, *kind, entry, exit)?;
            }
        }
        now = measured(&plan_of(&d)?.plan, trim);
    }
    Ok(Round { doc: d, parts, before: was, after: now, restored })
}

/// `embroidery.nearestConnection`.
pub(super) fn nearest_connection(s: &mut Session, p: &Value) -> Result<Value> {
    let o = object(p, C)?;
    let opts = options_in(&o, &["ids", "all", "dryRun"], C)?;
    let flag = |k: &str| -> Result<bool> {
        match o.get(k) {
            None | Some(Value::Null) => Ok(false),
            Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("`{k}` {v}: true or false"))),
        }
    };
    let (all, dry_run) = (flag("all")?, flag("dryRun")?);
    let ids_given = o.get("ids").is_some_and(|v| !v.is_null());
    if all && ids_given {
        return Err(bad(C, "give `ids` or `all`, not both"));
    }
    // The objects given, else the selected ones, else every object (the project owner's choice,
    // 2026-10-25); with `all`, every object whatever is selected.
    let chosen = !all && (ids_given || !s.doc()?.selection.objects.is_empty());
    let mut skipped: Vec<(NodeId, &'static str)> = Vec::new();
    let scope: Option<HashSet<NodeId>> = if chosen {
        let (take, skip) = given(s, p, "to join", C)?;
        skipped.extend(skip.into_iter().map(|(id, why)| (id, why.id())));
        Some(take.into_iter().collect())
    } else {
        None
    };
    let (uid, original) = {
        let st = s.doc()?;
        (st.uid, st.doc.clone())
    };
    let service = s.embroidery_plans.of(uid);
    // Rounds on the search's own result, until one changes nothing.
    let mut d: Document = (*original).clone();
    let mut first: Option<Vec<Stretch>> = None;
    let mut last: Option<Round> = None;
    let mut restored = 0usize;
    for k in 0..MAX_ROUNDS {
        let walked = service.walk_kept(&d.without_edit_modes(), &opts, true).map_err(PlanError::error)?;
        if k == 0
            && let Some(sc) = &scope
        {
            // What is in scope and has no unit isn't sewn.
            let sewn: HashSet<NodeId> = walked.plan.units.iter().map(|u| u.origin.selected()).collect();
            let mut not_sewn: Vec<NodeId> = sc.iter().copied().filter(|id| !sewn.contains(id)).collect();
            not_sewn.sort_unstable();
            skipped.extend(not_sewn.into_iter().map(|id| (id, "notSewn")));
        }
        let mut why = Vec::new();
        let r = round(&service, &d, &walked, scope.as_ref(), &opts, if k == 0 { &mut skipped } else { &mut why })?;
        restored += r.restored;
        let moved = r.parts.iter().any(|(_, id, kind)| held(&d, *id, *kind) != held(&r.doc, *id, *kind));
        first.get_or_insert_with(|| r.before.clone());
        d = r.doc.clone();
        last = Some(r);
        if !moved {
            break;
        }
    }
    let mut seen = HashSet::new();
    skipped.retain(|(id, _)| seen.insert(*id));
    let (was, now) = (first.unwrap_or_default(), last.as_ref().map(|r| r.after.clone()).unwrap_or_default());
    let parts: Vec<(NodeId, PartKind)> = last.as_ref().map(|r| r.parts.iter().map(|(_, id, kind)| (*id, *kind)).collect()).unwrap_or_default();

    // What changed against the document as it was.
    let mut changed = 0usize;
    let mut listed = Vec::new();
    for (id, kind) in &parts {
        let (is_entry, is_exit) = held(&d, *id, *kind);
        if held(&original, *id, *kind) != (is_entry.clone(), is_exit.clone()) {
            changed += 1;
        }
        if listed.len() < MAX_LISTED {
            let point = |v: Option<PartValue>| match v {
                Some(PartValue::Point(q)) => json!([q.x, q.y]),
                _ => Value::Null,
            };
            listed.push(json!({"id": id.0, "part": kind.id(), "entry": point(is_entry), "exit": point(is_exit)}));
        }
    }
    let applied = changed > 0 && !dry_run;
    if applied {
        s.edit(LABEL, |doc, _| {
            *doc = d;
            Ok(())
        })?;
    }
    let (b, a) = (total(&was), total(&now));
    Ok(json!({
        "applied": applied,
        "changedCount": changed,
        "unchangedCount": parts.len() - changed,
        "skippedCount": skipped.len(),
        "skipped": skipped.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect::<Vec<_>>(),
        "restoredCount": restored,
        "before": b.json(),
        "after": a.json(),
        "saved": {"jumps": b.jumps as i64 - a.jumps as i64, "trims": b.trims as i64 - a.trims as i64, "length": round_mm(b.length - a.length)},
        "parts": listed,
    }))
}
