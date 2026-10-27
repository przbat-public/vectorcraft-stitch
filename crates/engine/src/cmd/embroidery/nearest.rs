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
//!   there don't say, not locked) is free: any of its starts and ends. A part outside the scope with
//!   no points set follows, as it is sewn: it starts nearest the end before it, so its stitches
//!   change when that end moves (the search weighs where a line, a column or a ring then ends; a
//!   fill is taken where it starts and ends now, as where its rows end isn't known before it is
//!   planned, and the plan checks what it sews). One outside the scope that holds points sews as it
//!   did; the rest are sewn as they are.
//! - **The search.** [`plan::joins`]: exact over the points offered, each part's own start and end
//!   first, so that ties keep them. The moves are measured as it measures them ([`plan::apart`]).
//!   The design's first part, planned from nowhere (as it is drawn), and a stroke a clip group cuts
//!   (its ends aren't its stitches') never follow: they are sewn as they are.
//! - **Its own points first.** Every part it joins gets points (the project owner's choice,
//!   2026-10-25), and gets them before the search: where it starts and ends now. The search then
//!   weighs the design as it is kept, and a second run starts where the first one ended (a part that
//!   started where the needle was starts at a point taken through document points and back, which
//!   can tip a tie in its plan; one that holds its points can open a join that one following the
//!   needle didn't).
//! - **Checked by planning.** The points are written on a copy, the copy planned (the units kept from
//!   earlier plans), and a stretch between two cuts that saves less than [`MIN_JOIN_GAIN`] as
//!   planned (the move across the cut into it weighed lightly, as the search weighs it), its moves
//!   and its needle points weighed together ([`plan::stitches_cost`]: the search
//!   takes a fill's way through at no cost, and a fill aimed at an exit far from where its rows end
//!   travels hidden to get there), or comes out longer, gets its old points back. The plan is
//!   checked again after that: a round whose plan comes out costlier than before in all, or sews its
//!   units in other blocks (the sewing order changed, which this command never does), changes
//!   nothing, and so does the command when the points set first come out costlier than the design
//!   was. The search then runs again on its own result until a round changes nothing, at most
//!   [`MAX_ROUNDS`] times: a second run then changes nothing, unless the last round still moved
//!   something (each round only keeps a stretch that saves at least [`MIN_JOIN_GAIN`], so the rounds
//!   settle; three is a bound on the work, not a proof). One undo step, none when nothing changes;
//!   `dryRun` answers without writing. What it answers is what the design then sews.
//! - **Planning little.** A round writes points only on the parts the search moves; the others stay
//!   as they are, so their units are the ones kept. Every plan it makes uses a copy of the units kept
//!   ([`super::service::PlanService::scratch`]), so trials and dry runs never push the document's own
//!   units out.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::{PartKey, PartKind, PartValue};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Point;
use vectorcraft_stitch::plan::{self, Ends, JoinShape, Link, Role, Through};
use vectorcraft_stitch::{Command, SAME_POINT, mm_to_pt};

use super::cache::UnitCache;
use super::digitize::{UnitEnds, Walked};
use super::objects::{edit_each, given};
use super::plan::{DesignPlan, Origin, Part};
use super::unit::PlanError;
use super::{object, options_in};
use crate::cmd::bad;
use crate::cmd::layer::locked_within;
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

/// Unit `i`'s first and last needle points (millimetres, [`DesignPlan::needle_ends`]: where it
/// really starts after an exact meet), `None` when it has none.
fn needle_ends(plan: &DesignPlan, i: usize) -> Option<(Point, Point)> {
    plan.needle_ends(i)
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

/// The chain of `walked`'s plan (of `doc`): each unit with needle points of its own, by its index,
/// as a [`Joint`]; the objects in `scope` (`None`: every object) that it can't join, with why, into
/// `skipped`. A locked object is sewn as it is, as the sewing order's moves leave it (`locked`).
fn chain(doc: &Document, walked: &Walked, scope: Option<&HashSet<NodeId>>, skipped: &mut Vec<(NodeId, &'static str)>) -> Vec<(usize, Joint)> {
    let plan = &walked.plan;
    let recorded: HashMap<(Origin, usize), &UnitEnds> = walked.ends.iter().map(|e| ((e.origin, e.occurrence), e)).collect();
    // How many units each object's kind of part is sewn as: one pair of points can serve one.
    let mut units_of: HashMap<(NodeId, PartKind), usize> = HashMap::new();
    for u in &plan.units {
        if let Some(kind) = u.origin.part.kind() {
            *units_of.entry((u.origin.selected(), kind)).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    for ((i, u), link) in plan.units.iter().enumerate().zip(links(plan)) {
        let (Some(link), Some((first, last))) = (link, needle_ends(plan, i)) else { continue };
        let id = u.origin.selected();
        let in_scope = scope.is_none_or(|sc| sc.contains(&id));
        let kind = u.origin.part.kind();
        let record = recorded.get(&(u.origin, u.occurrence)).copied();
        let why = match (kind, record) {
            _ if locked_within(doc, id) => Some("locked"),
            (None, _) if u.origin.part == Part::Manual => Some("manual"),
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
            // the rest, as they are now. The design's first part follows nothing (it is planned
            // from nowhere: as it is drawn), and a stroke a clip group cuts ends where the clip
            // leaves it, not where its path does.
            (_, _, Some(e))
                if u.asked.entry.is_none()
                    && u.asked.exit.is_none()
                    && e.through != Through::Free
                    && link != Link::First
                    && !matches!(why, Some("literal" | "clipped")) =>
            {
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
/// (millimetres), the units' needle points, and the cost the search weighs the moves by, with the
/// needle points' ([`plan::stitches_cost`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Moves {
    connections: usize,
    jumps: usize,
    trims: usize,
    length: f64,
    longest: f64,
    stitches: usize,
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

    /// A unit's `n` needle points.
    fn sew(&mut self, n: usize) {
        self.stitches += n;
        self.cost += plan::stitches_cost(n);
    }

    fn json(&self) -> Value {
        json!({
            "connections": self.connections,
            "jumps": self.jumps,
            "trims": self.trims,
            "length": round_mm(self.length),
            "longest": round_mm(self.longest),
            "stitches": self.stitches,
        })
    }
}

/// `v` to a hundredth of a millimetre, as the replies give lengths.
fn round_mm(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A stretch of a plan's units between two cuts: its units' keys, and its moves.
type Stretch = (Vec<(Origin, usize)>, Moves);

/// The moves of `plan` inside its blocks, stretch by stretch (as [`links`] splits them), and in
/// each stretch's cost the move across the cut into it, lightly ([`plan::move_cost`]).
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
        moves.sew(plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch).count());
        match (link, before) {
            (Link::Thread, Some(b)) => moves.add(plan::apart(b, first), trim),
            // Across a cut only the frame travels: weighed lightly, as the search weighs it, in the
            // stretch it leads into (no connection of its own).
            (Link::Cut, Some(b)) => moves.cost += plan::move_cost(plan::apart(b, first), Link::Cut, trim),
            _ => {}
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
        t.stitches += m.stitches;
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

/// What one round of the search did to a copy of the document: the copy with the new points of the
/// parts it moved and of those right after them (each stretch that saved too little as planned
/// given its old points back), its free parts (each its unit's index in the plan before, its
/// object and its kind of part), its moves after, and how many stretches came out longer and were
/// restored (each by its first unit's key, so that a stretch restored in two rounds counts once).
struct Round {
    doc: Document,
    parts: Vec<(usize, NodeId, PartKind)>,
    after: Vec<Stretch>,
    restored: Vec<(Origin, usize)>,
}

/// How many rounds the search runs on its own result at most, until one changes nothing: each
/// round's points, once planned, can open a better join that the points offered only stood near
/// (an estimate; a second command then finds nothing left).
const MAX_ROUNDS: usize = 3;

/// One round of the search on `doc` (planned as `walked`), its parts in `scope` (`None`: every
/// object), with the `units` kept and the export's `opts`; what it can't join, with why, into
/// `skipped` when asked.
fn round(
    units: &Mutex<UnitCache>,
    doc: &Document,
    walked: &Walked,
    scope: Option<&HashSet<NodeId>>,
    opts: &super::Options,
    skipped: &mut Vec<(NodeId, &'static str)>,
) -> Result<Round> {
    let trim = walked.trim;
    let joints = chain(doc, walked, scope, skipped);
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
    // Written on a copy, in document points: each free part the search moved (every free part holds
    // its points by now, so none follows a moved end); the others are left as they are, their units
    // kept.
    let mut d = doc.clone();
    let mut parts: Vec<(usize, NodeId, PartKind)> = Vec::new();
    let mut written: HashSet<usize> = HashSet::new();
    for (k, ((i, j), shape)) in joints.iter().zip(&shapes).enumerate() {
        let Some((id, kind)) = j.part else { continue };
        let Some(&(s_, e_)) = chosen_points.get(k) else { continue };
        parts.push((*i, id, kind));
        if (s_, e_) == (0, 0) {
            continue;
        }
        written.insert(*i);
        // A point it keeps stays as it holds it (not taken through millimetres and back).
        let (held_entry, held_exit) = held(doc, id, kind);
        let pick = |held: Option<PartValue>, at: usize, of: &[Point], own: Point| match held {
            Some(v) if at == 0 => v,
            _ => PartValue::Point(in_points(of.get(at).copied().unwrap_or(own))),
        };
        let entry = pick(held_entry, s_, &shape.ends.starts, j.own.0);
        let exit = pick(held_exit, e_, &shape.ends.ends, j.own.1);
        write(&mut d, id, kind, Some(entry), Some(exit))?;
    }
    // Checked by planning the copy: a stretch that saves less than MIN_JOIN_GAIN as planned (or
    // comes out longer) keeps its points. Where the points offered only stand near where a part
    // starts now, that also keeps a later run from moving it for nothing. Those that came out longer
    // are counted as restored.
    let plan_of =
        |d: &Document| -> Result<Walked> { super::digitize::walked(&d.without_edit_modes(), opts, Some(units), false).map_err(PlanError::error) };
    let mut now = measured(&plan_of(&d)?.plan, trim);
    // Units sewn in other blocks: the sewing order changed, which this command never does (the
    // project owner's choice, 2026-10-25). The round changes nothing.
    let unchanged = |parts, restored| Round { doc: doc.clone(), parts, after: was.clone(), restored };
    if !(was.len() == now.len() && was.iter().zip(&now).all(|(a, b)| a.0 == b.0)) {
        return Ok(unchanged(parts, Vec::new()));
    }
    let back: HashSet<(Origin, usize)> = was.iter().zip(&now).filter(|(a, b)| !saves(&a.1, &b.1)).flat_map(|(a, _)| a.0.iter().copied()).collect();
    let restored: Vec<(Origin, usize)> = was.iter().zip(&now).filter(|(a, b)| longer(&a.1, &b.1)).filter_map(|(a, _)| a.0.first().copied()).collect();
    if !back.is_empty() {
        for (i, id, kind) in &parts {
            let key = walked.plan.units.get(*i).map(|u| (u.origin, u.occurrence));
            if written.contains(i) && key.is_some_and(|k| back.contains(&k)) {
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
    // Checked again after the restore: a round whose plan comes out costlier than before, in all,
    // or with its units in other blocks, changes nothing.
    if worse(&was, &now) {
        return Ok(unchanged(parts, restored));
    }
    Ok(Round { doc: d, parts, after: now, restored })
}

/// Whether moves `b` save enough on `a` to keep them: at least [`MIN_JOIN_GAIN`], their needle
/// points weighed in.
fn saves(a: &Moves, b: &Moves) -> bool {
    a.cost - b.cost >= MIN_JOIN_GAIN
}

/// Whether moves `now` are worse than `was`: their units sewn in other blocks, or costlier in all
/// (past float noise).
fn worse(was: &[Stretch], now: &[Stretch]) -> bool {
    let same_shape = was.len() == now.len() && was.iter().zip(now).all(|(a, b)| a.0 == b.0);
    !same_shape || longer(&total(was), &total(now))
}

/// Whether moves `b` cost more than `a` (past float noise).
fn longer(a: &Moves, b: &Moves) -> bool {
    b.cost > a.cost + COST_NOISE
}

/// How much more a plan's moves may cost than another's and still cost the same: float noise,
/// summing the same moves in another order.
const COST_NOISE: f64 = 1e-9;

/// `embroidery.nearestConnection`: prepared, searched and applied in one go ([`NearestJob`]).
pub(super) fn nearest_connection(s: &mut Session, p: &Value) -> Result<Value> {
    let job = prepare(s, p)?;
    let found = job.search()?;
    apply(s, found)
}

/// Nearest Connection asked for, ready to search on any thread ([`NearestJob::search`]): the
/// document as it was (its uid and snapshot), the objects in scope (`None`: every object), what in
/// scope it can't join so far, the embroidery options, a copy of the units kept
/// ([`super::service::PlanService::scratch`]), and whether it only answers.
pub struct NearestJob {
    uid: u64,
    original: std::sync::Arc<Document>,
    scope: Option<HashSet<NodeId>>,
    skipped: Vec<(NodeId, &'static str)>,
    opts: super::Options,
    units: Mutex<UnitCache>,
    dry_run: bool,
}

/// What a search found ([`NearestJob::search`]), to apply to the document it was made of
/// ([`Session::apply_nearest_connection`]): the document with the points written, the parts it
/// joins, the moves before and after, how many stretches were restored, and what it can't join.
pub struct NearestFound {
    uid: u64,
    original: std::sync::Arc<Document>,
    doc: Document,
    parts: Vec<(NodeId, PartKind)>,
    was: Vec<Stretch>,
    now: Vec<Stretch>,
    restored: usize,
    skipped: Vec<(NodeId, &'static str)>,
    dry_run: bool,
}

/// What [`NearestJob::search`] reports to the progress sink as it goes
/// ([`vectorcraft_doc::cancel::progress`]): the planning passes done of [`NEAREST_PASSES`]; each
/// pass's own progress comes as a plan's does.
pub const NEAREST_PROGRESS: &str = "nearestConnection";

/// How many planning passes a search makes at most, as its progress counts them: the design as it
/// is, then with every part's own points, then a walk and a trial plan each round (a restore's
/// plan more, uncounted).
pub const NEAREST_PASSES: u64 = 2 + 2 * MAX_ROUNDS as u64;

/// The job `p` asks for on the active document (the params checked, the scope taken from the
/// selection as it is now).
fn prepare(s: &mut Session, p: &Value) -> Result<NearestJob> {
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
    let units = s.embroidery_plans.of(uid).scratch();
    Ok(NearestJob { uid, original, scope, skipped, opts, units, dry_run })
}

impl NearestJob {
    /// The search, on the document as it was: the points written on a copy, round after round
    /// (this module's rules), and checked by planning. Any thread runs it; inside a
    /// [`vectorcraft_doc::cancel::scope`], a cancel stops it between passes or inside one (an error
    /// then, nothing found).
    pub fn search(&self) -> Result<NearestFound> {
        let Self { uid, original, scope, opts, units, dry_run, .. } = self;
        let mut skipped = self.skipped.clone();
        let passes = std::cell::Cell::new(0u64);
        let walk = |d: &Document| {
            vectorcraft_doc::cancel::progress(passes.get(), NEAREST_PASSES, NEAREST_PROGRESS);
            passes.set(passes.get() + 1);
            super::digitize::walked(&d.without_edit_modes(), opts, Some(units), true).map_err(PlanError::error)
        };
        // The design as it is: what its moves come to, and what in scope isn't sewn or can't be
        // joined.
        let walked = walk(original)?;
        let was = measured(&walked.plan, walked.trim);
        if let Some(sc) = scope {
            let sewn: HashSet<NodeId> = walked.plan.units.iter().map(|u| u.origin.selected()).collect();
            let mut not_sewn: Vec<NodeId> = sc.iter().copied().filter(|id| !sewn.contains(id)).collect();
            not_sewn.sort_unstable();
            skipped.extend(not_sewn.into_iter().map(|id| (id, "notSewn")));
        }
        // Every part it joins gets points (the project owner's choice, 2026-10-25), first where it
        // starts and ends now, so the search weighs the design as it is kept: a part that started
        // where the needle was then starts at a point taken through document points and back,
        // which can land a float's width off and tip a tie in its plan (two rows as near the
        // needle), and a part held can open a join that one following the needle didn't. Run
        // again, the command then starts where this run ended.
        let mut d: Document = (**original).clone();
        let mut froze = false;
        for (_, j) in chain(&d, &walked, scope.as_ref(), &mut skipped) {
            let Some((id, kind)) = j.part else { continue };
            let (entry, exit) = held(&d, id, kind);
            if entry.is_none() || exit.is_none() {
                let entry = entry.unwrap_or(PartValue::Point(in_points(j.own.0)));
                let exit = exit.unwrap_or(PartValue::Point(in_points(j.own.1)));
                froze |= write(&mut d, id, kind, Some(entry), Some(exit))?;
            }
        }
        // Rounds on the search's own result, until one changes nothing.
        let mut planned = (!froze).then_some(walked);
        let mut last: Option<Round> = None;
        let mut restored: HashSet<(Origin, usize)> = HashSet::new();
        for _ in 0..MAX_ROUNDS {
            let walked = match planned.take() {
                Some(w) => w,
                None => walk(&d)?,
            };
            vectorcraft_doc::cancel::progress(passes.get(), NEAREST_PASSES, NEAREST_PROGRESS);
            passes.set(passes.get() + 1);
            let r = round(units, &d, &walked, scope.as_ref(), opts, &mut Vec::new())?;
            restored.extend(r.restored.iter().copied());
            let moved = r.parts.iter().any(|(_, id, kind)| held(&d, *id, *kind) != held(&r.doc, *id, *kind));
            d = r.doc.clone();
            last = Some(r);
            if !moved {
                break;
            }
        }
        let mut seen = HashSet::new();
        skipped.retain(|(id, _)| seen.insert(*id));
        let mut now = last.as_ref().map_or_else(|| was.clone(), |r| r.after.clone());
        let parts: Vec<(NodeId, PartKind)> = last.as_ref().map(|r| r.parts.iter().map(|(_, id, kind)| (*id, *kind)).collect()).unwrap_or_default();
        // The rounds never make their own input worse; the points set first could (a tie tipped
        // the other way), and then the command changes nothing.
        if worse(&was, &now) {
            d = (**original).clone();
            now = was.clone();
        }
        Ok(NearestFound { uid: *uid, original: original.clone(), doc: d, parts, was, now, restored: restored.len(), skipped, dry_run: *dry_run })
    }
}

/// What `found` changed applied to its document as one undo step, when there is a change and it is
/// no dry run, and the reply: refused when the document changed since the search took it (it
/// searched an earlier one, so nothing is applied).
fn apply(s: &mut Session, found: NearestFound) -> Result<Value> {
    let NearestFound { uid, original, doc: d, parts, was, now, restored, skipped, dry_run } = found;
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
        let st = s.doc()?;
        if st.uid != uid || !std::sync::Arc::ptr_eq(&st.doc, &original) {
            return Err(bad(C, "the document changed while the search ran: nothing was applied"));
        }
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
        "saved": {
            "jumps": b.jumps as i64 - a.jumps as i64,
            "trims": b.trims as i64 - a.trims as i64,
            "length": round_mm(b.length - a.length),
            "stitches": b.stitches as i64 - a.stitches as i64,
        },
        "parts": listed,
    }))
}

impl Session {
    /// Nearest Connection on the active document as `embroidery.nearestConnection {params}` takes
    /// them, ready to search off the interface thread ([`NearestJob::search`]); what it finds is
    /// applied with [`Session::apply_nearest_connection`]. The command runs the three in one go.
    pub fn nearest_connection_job(&mut self, p: &Value) -> Result<NearestJob> {
        prepare(self, p)
    }

    /// Whether what a search found is of the active document as it is now: the one it searched,
    /// unchanged since.
    pub fn nearest_connection_current(&self, found: &NearestFound) -> bool {
        self.doc().is_ok_and(|st| st.uid == found.uid && std::sync::Arc::ptr_eq(&st.doc, &found.original))
    }

    /// What a search found applied, as the command applies it (one undo step "Nearest
    /// Connection"), and the command's reply: refused when the document changed meanwhile.
    pub fn apply_nearest_connection(&mut self, found: NearestFound) -> Result<Value> {
        apply(self, found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A round is undone when its plan is worse after its restore: costlier in all (past float
    /// noise), or with its units in other stretches (the sewing order changed, which the command
    /// never does); a plan that costs the same or less, in the same stretches, stands.
    #[test]
    fn a_worse_or_reordered_round_is_undone() {
        let stretch = |n: usize, cost: f64| -> Stretch { (Vec::new(), Moves { connections: n, cost, ..Moves::default() }) };
        let was = vec![stretch(1, 10.0), stretch(2, 5.0)];
        assert!(!worse(&was, &was), "the same");
        assert!(!worse(&was, &[stretch(1, 9.0), stretch(2, 5.0)]), "cheaper");
        assert!(!worse(&was, &[stretch(1, 10.0 + COST_NOISE / 2.0), stretch(2, 5.0)]), "noise");
        assert!(worse(&was, &[stretch(1, 10.0), stretch(2, 5.5)]), "costlier");
        assert!(worse(&was, &[stretch(3, 14.0)]), "its units in other stretches");
    }

    /// A stretch's needle points count with its moves: one that saves 5 mm of jumping by travelling
    /// 60 needle points more hidden through a fill (120 mm of travel, weighed 30) doesn't save, and
    /// is restored; one that saves the same with no needle point more does.
    #[test]
    fn needle_points_count_with_the_moves() {
        let moves = |d: f64, n: usize| {
            let mut m = Moves::default();
            m.add(d, 10.0);
            m.sew(n);
            m
        };
        let before = moves(6.0, 1000);
        assert!(saves(&before, &moves(1.0, 1000)), "5 mm shorter, as many needle points");
        assert!(!saves(&before, &moves(1.0, 1060)), "5 mm shorter, 60 needle points more");
        assert_eq!(moves(1.0, 1060).json()["stitches"], serde_json::json!(1060));
    }

    /// Out of the scope and with no points set, the design's first part is sewn as it is drawn (it
    /// is planned from nowhere), so the search never takes it for one it may turn round; the part
    /// after it, out of scope too, follows the end before it.
    #[test]
    fn the_designs_first_part_never_follows() {
        use serde_json::json;
        let mut s = super::super::test_support::blank();
        let mut line = |d: &str| {
            super::super::test_support::run(&mut s, "paint.setFill", json!({"none": true}));
            super::super::test_support::run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
            let id = super::super::test_support::run(&mut s, "path.create", json!({ "d": d }))["id"].as_u64().unwrap();
            super::super::test_support::run(&mut s, "stroke.set", json!({"weight": 1.0, "ids": [id]}));
            NodeId(id)
        };
        line("M 20 20 L 120 20");
        line("M 120 40 L 20 40");
        let third = line("M 20 60 L 120 60");
        let doc = s.doc().unwrap().doc.clone();
        let walked = super::super::digitize::walked(&doc, &super::super::Options::default(), None, true).unwrap();
        let scope: HashSet<NodeId> = [third].into_iter().collect();
        let joints = chain(&doc, &walked, Some(&scope), &mut Vec::new());
        let roles: Vec<(Role, usize)> = joints.iter().map(|(_, j)| (j.shape.role, j.shape.ends.starts.len())).collect();
        assert_eq!(roles, vec![(Role::Free, 1), (Role::Follows, 2), (Role::Free, 2)], "as drawn, following, free");
    }
}
