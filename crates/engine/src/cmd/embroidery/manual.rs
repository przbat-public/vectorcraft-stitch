//! Manual stitches: an object converted to them (`embroidery.convertToManual`), its stitches then
//! sewn as they are placed, and the object restored (`embroidery.restoreObject`).
//!
//! Converting freezes the plan: each of the object's sewing units becomes a run of literal stitches
//! marked manual, its own stitches as the plan sews them (ties, underlay, top stitching, travels,
//! its jumps and cuts; [`DesignPlan::own_stitches`]), one path a unit in walk order, its subpaths
//! its runs. The object becomes a group of them keeping its id, which keeps the object as the file
//! saved it ([`Original`]) to restore it. The move into each unit and the cut after the object are
//! the walk's to make again, by the same rules as before (a manual run is placed as art is), so the
//! export right after converting writes the same bytes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Value, json};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::embroidery::{Embroidery, Exact, Original, StitchRun};
use vectorcraft_doc::{Appearance, AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind, Shared};
use vectorcraft_geom::{Affine, Anchor, PathData, Point, SubPath};
use vectorcraft_stitch::plan::THREAD_WIDTH;
use vectorcraft_stitch::{Command, MM_PER_PT, SAME_POINT, Stitch, mm_to_pt};

use super::digitize::converted;
use super::objects::MAX_IDS;
use super::plan::{DesignPlan, Part};
use super::{object, options_in, service, to_plan};
use crate::cmd::{bad, targets};
use crate::{EngineError, Result, Session};

const CONVERT: &str = "embroidery.convertToManual";
/// The undo step's name, the command's label.
const CONVERT_LABEL: &str = "Convert to Manual Stitches";

/// The most objects a reply lists in `converted` and in `skipped` (their counts are whole): as many
/// as the settings commands list.
const MAX_LISTED: usize = 1000;

/// Why an object given isn't converted, as the reply names it.
mod reason {
    /// Stitches read from a machine file (or an object holding some): edit them as they are.
    pub(super) const LITERAL: &str = "literal";
    /// Manual stitches already (a converted object, or a run of one).
    pub(super) const MANUAL: &str = "manual";
    /// A symbol's instance: every instance plans its symbol's art from where its needle is, so no
    /// one frozen plan stands for them all.
    pub(super) const SYMBOL_INSTANCE: &str = "symbolInstance";
    /// A clip group's clipping path: it clips, and converting it would change what is sewn.
    pub(super) const CLIPPING_PATH: &str = "clippingPath";
    /// Part of an object the plan sews whole (a piece of an expanded appearance).
    pub(super) const INSIDE_OBJECT: &str = "insideObject";
    /// Locked, or on a locked layer or artboard.
    pub(super) const LOCKED: &str = "locked";
    /// Nothing of it is sewn (hidden, unpainted, type, an image, every part off).
    pub(super) const NOT_SEWN: &str = "notSewn";
    /// Its stitches hold what a run of manual stitches can't (two needle points at one spot in a run
    /// of several): converting would change them.
    pub(super) const UNFREEZABLE: &str = "unfreezable";
}

/// A unit's own stitches as a run of manual stitches holds them: its runs' needle points
/// (millimetres) and the runs the needle reaches after a cut.
struct Frozen {
    subpaths: Vec<Vec<Point>>,
    trims: Vec<usize>,
    double_point: bool,
}

/// A unit's own stitches ([`DesignPlan::own_stitches`]) as a run of manual stitches, or `None` when
/// they hold what one can't: they must start with a needle point, every jump must land on the next
/// needle point, every cut lie at the needle point before it and be followed by a jump, and nothing
/// else (a stop, a thread change) may come. A run of two needle points at one spot is one needle
/// point when read back, unless it is the only run (marked `double_point`): one such run among
/// several can't be frozen.
fn frozen(own: &[Stitch]) -> Option<Frozen> {
    let finite = |s: &Stitch| s.x.is_finite() && s.y.is_finite();
    let (mut subpaths, mut trims) = (vec![Vec::new()], Vec::new());
    let (mut cut, mut jump): (bool, Option<Point>) = (false, None);
    for s in own {
        if !finite(s) {
            return None;
        }
        match s.command {
            Command::Stitch => {
                if let Some(to) = jump.take() {
                    if to.distance(s.point()) > SAME_POINT {
                        return None;
                    }
                    if std::mem::take(&mut cut) {
                        trims.push(subpaths.len());
                    }
                    subpaths.push(Vec::new());
                } else if cut {
                    return None;
                }
                subpaths.last_mut()?.push(s.point());
            }
            Command::Jump if jump.is_none() && !subpaths.last()?.is_empty() => jump = Some(s.point()),
            Command::Trim if !cut && jump.is_none() => {
                let at = *subpaths.last()?.last()?;
                if at.distance(s.point()) > SAME_POINT {
                    return None;
                }
                cut = true;
            }
            _ => return None,
        }
    }
    if cut || jump.is_some() || subpaths.iter().any(Vec::is_empty) {
        return None;
    }
    let tack = |sub: &Vec<Point>| matches!(sub.as_slice(), [a, b] if a.distance(*b) < SAME_POINT);
    let double_point = subpaths.len() == 1 && subpaths.first().is_some_and(tack);
    if subpaths.len() > 1 && subpaths.iter().any(tack) {
        return None;
    }
    Some(Frozen { subpaths, trims, double_point })
}

/// The path of a run of manual stitches: each run a subpath of corner anchors at its needle points
/// (document points); a run of one needle point a line from the point to itself, as a machine file
/// opens one.
fn run_path(f: &Frozen) -> PathData {
    let corner = |p: &Point| Anchor::corner(Point::new(mm_to_pt(p.x), mm_to_pt(p.y)));
    let subpaths = f
        .subpaths
        .iter()
        .map(|sub| {
            let mut anchors: Vec<Anchor> = sub.iter().map(corner).collect();
            if let [only] = anchors.as_slice() {
                anchors.push(*only);
            }
            SubPath { anchors, closed: false }
        })
        .collect();
    PathData::new(subpaths)
}

/// The needle points of `f` whose millimetres its document points can't give back exactly (read as
/// the walk reads them, the document point times [`MM_PER_PT`]): each coordinate that would come back
/// a unit in the last place off, to be kept exactly ([`StitchRun::exact`]), so that what is sewn
/// right after converting is what was planned, to the bit.
fn exact_millimetres(f: &Frozen) -> Vec<Exact> {
    let back = |v: f64| mm_to_pt(v) * MM_PER_PT;
    let mut out = Vec::new();
    for (s, sub) in f.subpaths.iter().enumerate() {
        // A run of one needle point is stored as two anchors at it: both are read.
        let anchors = if sub.len() == 1 { 2 } else { sub.len() };
        for a in 0..anchors {
            let Some(p) = sub.get(a).or(sub.first()) else { continue };
            let (x, y) = ((back(p.x) != p.x).then_some(p.x), (back(p.y) != p.y).then_some(p.y));
            if x.is_some() || y.is_some() {
                out.push(Exact { subpath: s, anchor: a, x, y });
            }
        }
    }
    out
}

/// The look of a run of manual stitches in thread colour `art`: a thin line as wide as 40 wt thread
/// lies, round at the ends and turns, as an opened machine file's runs are drawn.
fn run_look(art: [u8; 3]) -> Appearance {
    let color = Color::rgb8(art[0], art[1], art[2]);
    let mut look = Appearance::basic(Paint::None, Paint::solid(color), mm_to_pt(THREAD_WIDTH));
    if let Some(AppearanceItem::Stroke(s)) = look.items.last_mut() {
        s.cap = LineCap::Round;
        s.join = LineJoin::Round;
    }
    look
}

/// The step a fingerprint rounds a needle point to, points: a ten-thousandth of a point (about 35
/// nanometres), far below anything sewn, coarse enough that the rounding of transforms composed and
/// undone (the original's frame) never moves a needle point to the next step but where it lies on
/// the edge between two. An estimate.
const FINGERPRINT_STEP: f64 = 1e-4;
/// FNV-1a's 64-bit offset basis and prime (Fowler, Noll and Vo's published constants): a hash whose
/// value never changes between versions or platforms, so a fingerprint a file keeps stays true.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The fingerprint of the stitches of `group` (a converted object), its needle points taken back
/// to where they were converted by `frame` (its original's), with each run's cuts and stops: what
/// tells stitches edited since converting from stitches only moved, turned or scaled with their
/// object. `None` when `frame` can't be undone.
pub(crate) fn fingerprint(group: &Node, frame: Affine) -> Option<u64> {
    if frame.determinant().abs() <= f64::EPSILON || !frame.determinant().is_finite() {
        return None;
    }
    let back = frame.inverse();
    let mut h = FNV_OFFSET;
    let mut eat = |word: u64| {
        for byte in word.to_le_bytes() {
            h = (h ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    };
    let step = |v: f64| (v / FINGERPRINT_STEP).round() as i64 as u64;
    for c in group.children().into_iter().flatten() {
        let Some(path) = c.path_data() else { continue };
        let run = c.extras().embroidery.as_deref().and_then(|e| e.stitches.clone()).unwrap_or_default();
        eat(path.subpaths.len() as u64);
        for sp in &path.subpaths {
            eat(sp.anchors.len() as u64);
            for a in &sp.anchors {
                let p = back * a.p;
                eat(step(p.x));
                eat(step(p.y));
            }
        }
        for k in &run.trims {
            eat(*k as u64);
        }
        for (s, a) in &run.stops {
            eat(*s as u64);
            eat(*a as u64);
        }
        eat(u64::from(run.trim_before) | u64::from(run.trim_after) << 1 | u64::from(run.stop_after) << 2 | u64::from(run.double_point) << 3);
    }
    Some(h)
}

/// One object to convert: its id, and its units (indices into the plan's), in walk order.
struct Target {
    id: NodeId,
    units: Vec<usize>,
}

/// The objects `n` stands for, to convert: itself when the plan sews it as an object (`by_object`:
/// its units), what a group or a layer holds otherwise; what isn't converted, with why, into
/// `skipped`.
fn collect(
    doc: &Document,
    n: &Node,
    by_object: &HashMap<NodeId, Vec<usize>>,
    plan: &DesignPlan,
    out: &mut Vec<Target>,
    skipped: &mut Vec<(NodeId, &'static str)>,
) {
    if converted(n) || is_manual_run(n) {
        skipped.push((n.id, reason::MANUAL));
        return;
    }
    if matches!(n.kind, NodeKind::SymbolInstance { .. }) {
        skipped.push((n.id, reason::SYMBOL_INSTANCE));
        return;
    }
    if matches!(n.kind, NodeKind::Path { clipping: true, .. }) {
        skipped.push((n.id, reason::CLIPPING_PATH));
        return;
    }
    if let Some(units) = by_object.get(&n.id) {
        let part = |p: Part| units.iter().any(|&i| plan.units.get(i).is_some_and(|u| u.origin.part == p));
        let why = if part(Part::Literal) {
            Some(reason::LITERAL)
        } else if part(Part::Manual) {
            Some(reason::MANUAL)
        } else if !doc.is_editable(n.id) {
            Some(reason::LOCKED)
        } else {
            None
        };
        match why {
            Some(why) => skipped.push((n.id, why)),
            None => out.push(Target { id: n.id, units: units.clone() }),
        }
        return;
    }
    match &n.kind {
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
            for c in children {
                collect(doc, c, by_object, plan, out, skipped);
            }
        }
        NodeKind::Path { .. } if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) => skipped.push((n.id, reason::LITERAL)),
        _ => {
            // Part of an object sewn whole, or not sewn at all.
            let inside = doc.ancestry(n.id).unwrap_or_default().iter().any(|a| *a != n.id && by_object.contains_key(a));
            skipped.push((n.id, if inside { reason::INSIDE_OBJECT } else { reason::NOT_SEWN }));
        }
    }
}

/// Whether `n` is a run of manual stitches.
fn is_manual_run(n: &Node) -> bool {
    n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|r| r.manual)
}

/// What converting one object made: the group standing for it, and its counts.
struct Made {
    group: Node,
    units: usize,
    runs: usize,
    needle_points: usize,
}

/// The group standing for object `n` converted to manual stitches: a run of manual stitches for
/// each of its units (`units`, in walk order, of `plan`), each in its unit's thread's art colour (so
/// it is matched to the same thread), the object kept to be restored, its Trim After and Stop After
/// on the group (the walk cuts and stops after its last stitch, as before). `None` when one of its
/// units can't be frozen ([`frozen`]). The runs' ids are taken from `doc`.
fn convert_one(doc: &mut Document, n: &Node, units: &[usize], plan: &DesignPlan) -> Option<Made> {
    let mut looks: HashMap<[u8; 3], Shared<Appearance>> = HashMap::new();
    let mut children: Vec<Arc<Node>> = Vec::with_capacity(units.len());
    let (mut runs, mut needle_points) = (0, 0);
    let mut frozen_units = Vec::with_capacity(units.len());
    for &u in units {
        let unit = plan.units.get(u)?;
        let f = frozen(&plan.own_stitches(u))?;
        frozen_units.push((unit.thread.art, f));
    }
    for (art, f) in frozen_units {
        runs += f.subpaths.len();
        needle_points += f.subpaths.iter().map(Vec::len).sum::<usize>();
        let mut node = Node::path(doc.alloc_id(), run_path(&f), Appearance::default());
        node.appearance = looks.entry(art).or_insert_with(|| Shared::new(run_look(art))).clone();
        let exact = exact_millimetres(&f);
        let run = StitchRun { manual: true, trims: f.trims, double_point: f.double_point, exact, ..StitchRun::default() };
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(run), ..Embroidery::default() }));
        children.push(Arc::new(node));
    }
    let e = n.extras().embroidery.as_deref();
    let mut group = Node::group(n.id, children);
    group.name = n.name.clone();
    group.visible = n.visible;
    group.locked = n.locked;
    let original = Original { frame: Affine::IDENTITY, node: Arc::new(serde_json::to_value(n).ok()?), fingerprint: None, extra: Default::default() };
    let mut marks = Embroidery { trim_after: e.is_some_and(|e| e.trim_after), stop_after: e.is_some_and(|e| e.stop_after), ..Embroidery::default() };
    marks.original = Some(Original { fingerprint: fingerprint(&group, Affine::IDENTITY), ..original });
    group.extras_mut().embroidery = Some(Box::new(marks));
    Some(Made { group, units: units.len(), runs, needle_points })
}

/// `embroidery.convertToManual {ids?, …the embroidery options}`: each object given (else selected)
/// frozen into manual stitches as the plan sews it with the options (as the read commands take
/// them; none: as the stitch preview plans), in one undo step.
pub(super) fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let o = object(p, CONVERT)?;
    let opts = options_in(&o, &["ids"], CONVERT)?;
    if let Some(n) = o.get("ids").and_then(Value::as_array).map(Vec::len).filter(|n| *n > MAX_IDS) {
        return Err(bad(CONVERT, format!("`ids` lists {n} objects: at most {MAX_IDS}")));
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(CONVERT, "select the objects to convert to manual stitches, or give ids"));
    }
    let st = s.doc()?;
    let doc = st.doc.clone();
    if let Some(missing) = ids.iter().find(|id| doc.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    // Planned afresh, as the export plans: what is written into the document never depends on the
    // units kept between plans.
    let planned = to_plan(st, false)?;
    let plan = service::plan_afresh(&planned, &opts).map_err(super::PlanError::error)?;
    let mut by_object: HashMap<NodeId, Vec<usize>> = HashMap::new();
    for (i, u) in plan.units.iter().enumerate() {
        by_object.entry(u.origin.selected()).or_default().push(i);
    }
    for units in by_object.values_mut() {
        units.sort_by_key(|&i| plan.units.get(i).map_or(usize::MAX, |u| u.walked));
    }
    let (mut targets_found, mut skipped) = (Vec::new(), Vec::new());
    let mut seen: HashSet<NodeId> = HashSet::new();
    for id in &ids {
        let Some(n) = doc.node(*id) else { continue };
        let mut found = Vec::new();
        collect(&doc, n, &by_object, &plan, &mut found, &mut skipped);
        targets_found.extend(found.into_iter().filter(|t| seen.insert(t.id)));
    }
    skipped.dedup();
    let mut converted_list: Vec<Value> = Vec::new();
    let mut total_points = 0usize;
    let made = s.edit(CONVERT_LABEL, |d, sel| {
        let mut made = 0usize;
        for t in &targets_found {
            let Some(n) = d.node(t.id).cloned() else { continue };
            let Some(m) = convert_one(d, &n, &t.units, &plan) else {
                skipped.push((t.id, reason::UNFREEZABLE));
                continue;
            };
            let Some(slot) = d.node_mut(t.id) else { continue };
            *slot = m.group;
            sel.anchors.remove(&t.id);
            made += 1;
            total_points += m.needle_points;
            if converted_list.len() < MAX_LISTED {
                converted_list.push(json!({"id": t.id.0, "units": m.units, "runs": m.runs, "needlePoints": m.needle_points}));
            }
        }
        if made == 0 {
            return Err(nothing_converted(&skipped));
        }
        Ok(made)
    })?;
    let listed: Vec<Value> = skipped.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why})).collect();
    Ok(json!({
        "converted": converted_list,
        "convertedCount": made,
        "needlePoints": total_points,
        "skippedCount": skipped.len(),
        "skipped": listed,
    }))
}

/// The error when nothing given could be converted, naming each object and why (as many as a reply
/// lists).
fn nothing_converted(skipped: &[(NodeId, &'static str)]) -> EngineError {
    let named: Vec<String> = skipped.iter().take(MAX_LISTED).map(|(id, why)| format!("{} ({why})", id.0)).collect();
    EngineError::Other(format!("nothing selected can be converted to manual stitches: {}", named.join(", ")))
}
