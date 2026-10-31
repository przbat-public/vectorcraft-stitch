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
use vectorcraft_doc::embroidery::{Embroidery, Exact, Mark, Original, StitchRun, shape_of};
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

/// The most objects a reply lists in `converted` and in `skipped` (their counts are whole).
use super::needles::MAX_LISTED;

/// Why an object given isn't converted, as the reply names it ([`Skip::code`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Skip {
    /// Stitches read from a machine file (or an object holding some): edit them as they are.
    Literal,
    /// Manual stitches already (a converted object, or a run of one).
    Manual,
    /// A symbol's instance: every instance plans its symbol's art from where its needle is, so no
    /// one frozen plan stands for them all.
    SymbolInstance,
    /// A clip group's clipping path: it clips, and converting it would change what is sewn.
    ClippingPath,
    /// Part of an object the plan sews whole (a piece of an expanded appearance).
    InsideObject,
    /// Locked, or on a locked layer or artboard.
    Locked,
    /// Nothing of it is sewn (hidden, unpainted, type, an image, every part off).
    NotSewn,
    /// Its stitches hold what a run of manual stitches can't (two needle points at one spot in a run
    /// of several): converting would change them.
    Unfreezable,
}

impl Skip {
    /// As the reply names it.
    fn code(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Manual => "manual",
            Self::SymbolInstance => "symbolInstance",
            Self::ClippingPath => "clippingPath",
            Self::InsideObject => "insideObject",
            Self::Locked => "locked",
            Self::NotSewn => "notSewn",
            Self::Unfreezable => "unfreezable",
        }
    }
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
        let e = c.extras().embroidery.as_deref();
        let none = StitchRun::default();
        let run = e.and_then(|e| e.stitches.as_ref()).unwrap_or(&none);
        eat(path.subpaths.len() as u64);
        for sp in &path.subpaths {
            eat(sp.anchors.len() as u64);
            for a in &sp.anchors {
                let p = back * a.p;
                eat(step(p.x));
                eat(step(p.y));
            }
        }
        for m in run.trims.iter().chain(&run.stops) {
            eat(m.subpath as u64);
            eat(m.anchor as u64);
        }
        // What comes after the run, wherever it is kept: its own Trim After and Stop After too.
        let after = e.map(|e| e.after()).unwrap_or_default();
        eat(u64::from(run.trim_before) | u64::from(after.trim) << 1 | u64::from(after.stop) << 2 | u64::from(run.double_point) << 3);
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
    skipped: &mut Vec<(NodeId, Skip)>,
) {
    if converted(n) || is_manual_run(n) {
        skipped.push((n.id, Skip::Manual));
        return;
    }
    if matches!(n.kind, NodeKind::SymbolInstance { .. }) {
        skipped.push((n.id, Skip::SymbolInstance));
        return;
    }
    if matches!(n.kind, NodeKind::Path { clipping: true, .. }) {
        skipped.push((n.id, Skip::ClippingPath));
        return;
    }
    if let Some(units) = by_object.get(&n.id) {
        let part = |p: Part| units.iter().any(|&i| plan.units.get(i).is_some_and(|u| u.origin.part == p));
        let why = if part(Part::Literal) {
            Some(Skip::Literal)
        } else if part(Part::Manual) {
            Some(Skip::Manual)
        } else if !doc.is_editable(n.id) {
            Some(Skip::Locked)
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
        NodeKind::Path { .. } if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) => skipped.push((n.id, Skip::Literal)),
        _ => {
            // Part of an object sewn whole, or not sewn at all.
            let inside = doc.ancestry(n.id).unwrap_or_default().iter().any(|a| *a != n.id && by_object.contains_key(a));
            skipped.push((n.id, if inside { Skip::InsideObject } else { Skip::NotSewn }));
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
        let path = run_path(&f);
        // Each cut kept at the first needle point of the run it comes before, where it lies.
        let trims: Vec<Mark> = f.trims.iter().filter_map(|&s| path.subpaths.get(s)?.anchors.first().map(|a| Mark::at(s, 0, a.p))).collect();
        let shape = (!trims.is_empty()).then(|| shape_of(&path));
        let mut node = Node::path(doc.alloc_id(), path, Appearance::default());
        node.appearance = looks.entry(art).or_insert_with(|| Shared::new(run_look(art))).clone();
        let exact = exact_millimetres(&f);
        let run = StitchRun { manual: true, trims, shape, double_point: f.double_point, exact, ..StitchRun::default() };
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
    let job = prepare(s, p)?;
    let planned = job.plan()?;
    apply(s, planned)
}

/// Convert to Manual Stitches asked for, ready to plan on any thread ([`ConvertJob::plan`], the
/// slow part: the whole design planned afresh): the document as it was (its uid and snapshot), what
/// is planned of it, the embroidery options and the objects given.
pub struct ConvertJob {
    uid: u64,
    original: Arc<Document>,
    planned: Arc<Document>,
    opts: super::Options,
    ids: Vec<NodeId>,
}

/// A convert's plan ([`ConvertJob::plan`]), to apply to the document it was made of
/// ([`Session::apply_convert_to_manual`]).
pub struct ConvertPlanned {
    uid: u64,
    original: Arc<Document>,
    ids: Vec<NodeId>,
    plan: DesignPlan,
}

impl ConvertJob {
    /// The design planned afresh, as the export plans it (what is written into the document never
    /// depends on the units kept between plans); cancelled with the work on this thread
    /// ([`vectorcraft_doc::cancel`]), reporting each unit planned as a plan does.
    pub fn plan(&self) -> Result<ConvertPlanned> {
        let plan = service::plan_afresh(&self.planned, &self.opts).map_err(super::PlanError::error)?;
        Ok(ConvertPlanned { uid: self.uid, original: self.original.clone(), ids: self.ids.clone(), plan })
    }
}

/// The job `p` asks for on the active document (the params checked, the objects taken from the
/// selection as it is now).
fn prepare(s: &mut Session, p: &Value) -> Result<ConvertJob> {
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
    if let Some(missing) = ids.iter().find(|id| st.doc.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    Ok(ConvertJob { uid: st.uid, original: st.doc.clone(), planned: to_plan(st, false)?, opts, ids })
}

/// Whether what a convert planned is of the active document as it is now: the one it planned,
/// unchanged since.
fn current(s: &Session, planned: &ConvertPlanned) -> bool {
    s.doc().is_ok_and(|st| st.uid == planned.uid && Arc::ptr_eq(&st.doc, &planned.original))
}

impl Session {
    /// Convert to Manual Stitches with params `p`, ready to plan on another thread
    /// ([`ConvertJob::plan`]): the interface's way to convert without waiting on the plan.
    pub fn convert_to_manual_job(&mut self, p: &Value) -> Result<ConvertJob> {
        prepare(self, p)
    }

    /// Whether what a convert planned is of the active document as it is now.
    pub fn convert_to_manual_current(&self, planned: &ConvertPlanned) -> bool {
        current(self, planned)
    }

    /// What a convert planned, applied as the command applies it (one undo step "Convert to Manual
    /// Stitches"), and the command's reply: refused when the document changed meanwhile.
    pub fn apply_convert_to_manual(&mut self, planned: ConvertPlanned) -> Result<Value> {
        apply(self, planned)
    }
}

/// What a convert planned, applied to the active document as the command applies it (one undo
/// step), refused when the document changed since it was planned.
fn apply(s: &mut Session, planned: ConvertPlanned) -> Result<Value> {
    if !current(s, &planned) {
        return Err(EngineError::Other("the document changed while it was planned: nothing was converted".into()));
    }
    let ConvertPlanned { original: doc, ids, plan, .. } = planned;
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
                skipped.push((t.id, Skip::Unfreezable));
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
    let listed: Vec<Value> = skipped.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why.code()})).collect();
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
fn nothing_converted(skipped: &[(NodeId, Skip)]) -> EngineError {
    let named: Vec<String> = skipped.iter().take(MAX_LISTED).map(|(id, why)| format!("{} ({})", id.0, why.code())).collect();
    EngineError::Other(format!("nothing selected can be converted to manual stitches: {}", named.join(", ")))
}

const RESTORE: &str = "embroidery.restoreObject";
/// The undo step's name, the command's label.
const RESTORE_LABEL: &str = "Restore Object";

/// Why a group given isn't restored, as the reply names it ([`NotRestored::code`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotRestored {
    /// Not an object converted to manual stitches (nothing kept to restore).
    NotConverted,
    /// Stitches read from a machine file: there is no object behind them.
    Literal,
    /// The object kept doesn't read as an object (a damaged file, or a newer version's object).
    Unreadable,
    /// The object places a symbol the document no longer has.
    MissingSymbol,
    /// Locked, or in a locked group or layer.
    Locked,
}

impl NotRestored {
    /// As the reply names it.
    fn code(self) -> &'static str {
        match self {
            Self::NotConverted => "notConverted",
            Self::Literal => "literal",
            Self::Unreadable => "unreadable",
            Self::MissingSymbol => "missingSymbol",
            Self::Locked => "locked",
        }
    }
}

/// The converted objects `n` stands for, to restore: itself when it is one, the one a manual run is
/// in, those a group or a layer holds; what isn't one, with why, into `skipped`.
fn restorable(doc: &Document, n: &Node, out: &mut Vec<NodeId>, skipped: &mut Vec<(NodeId, NotRestored)>) {
    if converted(n) {
        out.push(n.id);
        return;
    }
    if let Some(run) = n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()) {
        match doc.parent_of(n.id).and_then(|p| doc.node(p)).filter(|p| run.manual && converted(p)) {
            Some(group) => out.push(group.id),
            None => skipped.push((n.id, if run.manual { NotRestored::NotConverted } else { NotRestored::Literal })),
        }
        return;
    }
    match &n.kind {
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
            let found = out.len();
            for c in children {
                restorable(doc, c, out, &mut Vec::new());
            }
            if out.len() == found {
                skipped.push((n.id, NotRestored::NotConverted));
            }
        }
        _ => skipped.push((n.id, NotRestored::NotConverted)),
    }
}

/// The objects converted to manual stitches among `ids` of `doc` (a manual run standing for its
/// object), summed up for `embroidery.info` ([`super::info::ManualInfo`]); `None` when there are
/// none.
pub(crate) fn summary(doc: &Document, ids: &[NodeId]) -> Option<super::info::ManualInfo> {
    let mut groups = Vec::new();
    for id in ids {
        if let Some(n) = doc.node(*id) {
            restorable(doc, n, &mut groups, &mut Vec::new());
        }
    }
    let mut seen = HashSet::new();
    groups.retain(|g| seen.insert(*g));
    if groups.is_empty() {
        return None;
    }
    let mut out = super::info::ManualInfo { objects: groups.len(), restorable: 0, edited: 0, runs: 0, needle_points: 0, scale: None };
    let mut scale: Option<[f64; 2]> = None;
    let mut known = true;
    for g in groups.iter().filter_map(|g| doc.node(*g)) {
        out.restorable += usize::from(doc.is_editable(g.id));
        out.edited += usize::from(edited(g));
        for c in g.children().into_iter().flatten() {
            if let Ok(st) = super::needles::read(c) {
                out.runs += st.runs.iter().filter(|r| !r.needles.is_empty()).count();
                out.needle_points += st.len();
            }
        }
        let frame = g.extras().embroidery.as_deref().and_then(|e| e.original.as_ref()).map(|o| o.frame);
        match frame.and_then(vectorcraft_doc::embroidery::scale_factors) {
            Some([least, most]) => scale = Some(scale.map_or([least, most], |[l, m]| [l.min(least), m.max(most)])),
            None => known = false,
        }
    }
    out.scale = scale.filter(|_| known);
    Some(out)
}

/// Whether the stitches of converted group `group` were edited since converting: their fingerprint
/// ([`fingerprint`], the needle points taken back by the original's frame) is no longer the one kept,
/// or none was kept.
pub(crate) fn edited(group: &Node) -> bool {
    let Some(o) = group.extras().embroidery.as_deref().and_then(|e| e.original.as_ref()) else { return false };
    o.fingerprint.is_none() || fingerprint(group, o.frame) != o.fingerprint
}

/// The object converted group `group` (in `doc`) was converted from, as it is to be put back: read
/// from the original kept, transformed by the original's frame (moved, turned and scaled as the
/// stitches were since, strokes and corners as `scaling` says), taking the group's id, name,
/// visibility and lock, and its Trim After and Stop After (the marks the manual object can change
/// itself). Its members keep their ids where the document doesn't use them (they are free right
/// after converting), else take new ones. Or why it can't be.
fn restored(doc: &mut Document, group: &Node, scaling: vectorcraft_doc::Scaling) -> std::result::Result<Node, NotRestored> {
    let e = group.extras().embroidery.as_deref();
    let Some(o) = e.and_then(|e| e.original.as_ref()) else { return Err(NotRestored::NotConverted) };
    let mut n: Node = serde_json::from_value((*o.node).clone()).map_err(|_| NotRestored::Unreadable)?;
    let mut missing = false;
    n.walk(&mut |c| {
        if let NodeKind::SymbolInstance { symbol, .. } = &c.kind {
            missing |= !doc.symbols.iter().any(|s| &s.name == symbol);
        }
    });
    if missing {
        return Err(NotRestored::MissingSymbol);
    }
    if o.frame != Affine::IDENTITY {
        n.transform(o.frame, scaling);
    }
    n.id = group.id;
    n.name = group.name.clone();
    n.visible = group.visible;
    n.locked = group.locked;
    // Members keep their ids where free and already handed out; else new ones.
    let issued = doc.peek_next_id();
    let mut taken: HashSet<u64> = HashSet::new();
    fn renumber(n: &mut Node, doc: &mut Document, issued: u64, taken: &mut HashSet<u64>, top: bool) {
        if !top && (n.id.0 >= issued || doc.node(n.id).is_some() || !taken.insert(n.id.0)) {
            n.id = doc.alloc_id();
        }
        for c in n.children_mut().into_iter().flatten() {
            renumber(Arc::make_mut(c), doc, issued, taken, false);
        }
    }
    renumber(&mut n, doc, issued, &mut taken, true);
    let (cut, stop) = e.map_or((false, false), |e| (e.trim_after, e.stop_after));
    let mut marks = n.extras().embroidery.as_deref().cloned().unwrap_or_default();
    marks.set_trim_after(cut);
    marks.set_stop_after(stop);
    n.extras_mut().embroidery = (!marks.is_empty()).then(|| Box::new(marks));
    Ok(n)
}

/// `embroidery.restoreObject {ids?, strokes?, corners?, dryRun?}`: each converted object given (else
/// selected) put back, planned again at its size now, its stitch edits discarded, in one undo step;
/// with `dryRun`, only which would be, and which were edited since converting.
pub(super) fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let o = object(p, RESTORE)?;
    super::only(&o, &["ids", "strokes", "corners", "dryRun"], RESTORE)?;
    for key in ["strokes", "corners", "dryRun"] {
        if let Some(v) = o.get(key).filter(|v| !v.is_boolean() && !v.is_null()) {
            return Err(bad(RESTORE, format!("`{key}` {v}: true or false")));
        }
    }
    let dry = o.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
    if let Some(n) = o.get("ids").and_then(Value::as_array).map(Vec::len).filter(|n| *n > MAX_IDS) {
        return Err(bad(RESTORE, format!("`ids` lists {n} objects: at most {MAX_IDS}")));
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(RESTORE, "select the objects converted to manual stitches to restore, or give ids"));
    }
    let doc = s.doc()?.doc.clone();
    if let Some(missing) = ids.iter().find(|id| doc.node(**id).is_none()) {
        return Err(EngineError::NoNode(*missing));
    }
    let (mut groups, mut skipped) = (Vec::new(), Vec::new());
    for id in &ids {
        if let Some(n) = doc.node(*id) {
            restorable(&doc, n, &mut groups, &mut skipped);
        }
    }
    let mut seen = HashSet::new();
    groups.retain(|g| seen.insert(*g));
    let locked = |g: &NodeId| doc.ancestry(*g).unwrap_or_default().iter().any(|a| doc.node(*a).is_some_and(|n| n.locked));
    let (open, closed): (Vec<NodeId>, Vec<NodeId>) = groups.into_iter().partition(|g| !locked(g));
    skipped.extend(closed.into_iter().map(|g| (g, NotRestored::Locked)));
    let edited_ids: Vec<u64> = open.iter().filter(|g| doc.node(**g).is_some_and(edited)).map(|g| g.0).collect();
    let listed = |v: &[(NodeId, NotRestored)]| -> Vec<Value> {
        v.iter().take(MAX_LISTED).map(|(id, why)| json!({"id": id.0, "reason": why.code()})).collect()
    };
    if dry {
        let ids: Vec<u64> = open.iter().take(MAX_LISTED).map(|g| g.0).collect();
        return Ok(json!({
            "restored": ids, "restoredCount": open.len(), "edited": edited_ids, "editedCount": edited_ids.len(),
            "skippedCount": skipped.len(), "skipped": listed(&skipped), "dryRun": true,
        }));
    }
    let scaling = crate::cmd::object::scaling(s, p);
    let mut restored_ids: Vec<u64> = Vec::new();
    s.edit(RESTORE_LABEL, |d, sel| {
        for g in &open {
            let Some(group) = d.node(*g).cloned() else { continue };
            match restored(d, &group, scaling) {
                Ok(n) => {
                    let Some(slot) = d.node_mut(*g) else { continue };
                    *slot = n;
                    sel.anchors.remove(g);
                    restored_ids.push(g.0);
                }
                Err(why) => skipped.push((*g, why)),
            }
        }
        if restored_ids.is_empty() {
            return Err(nothing_restored(&skipped));
        }
        d.fix_next_id();
        Ok(())
    })?;
    let count = restored_ids.len();
    restored_ids.truncate(MAX_LISTED);
    Ok(json!({
        "restored": restored_ids, "restoredCount": count, "edited": edited_ids, "editedCount": edited_ids.len(),
        "skippedCount": skipped.len(), "skipped": listed(&skipped),
    }))
}

/// The error when nothing given could be restored, naming each object and why.
fn nothing_restored(skipped: &[(NodeId, NotRestored)]) -> EngineError {
    let named: Vec<String> = skipped.iter().take(MAX_LISTED).map(|(id, why)| format!("{} ({})", id.0, why.code())).collect();
    EngineError::Other(format!("nothing selected was converted to manual stitches: {}", named.join(", ")))
}
