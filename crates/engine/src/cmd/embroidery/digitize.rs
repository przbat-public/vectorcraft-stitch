//! The document walk that plans each visible object's stitches into the design's plan: what is
//! sewn (what would print), the sewing units (each fill and stroke of an object, each run of
//! literal stitches read from a file) and the settings each is planned with, clip groups, the
//! colour of each block, and the limit on a design's size.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use vectorcraft_color::Paint;
use vectorcraft_doc::cancel;
use vectorcraft_doc::embroidery::{StitchRun, ThreadPalette};
use vectorcraft_doc::{AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind, StrokeAlign, StrokeLayer};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, PathEl, Shape, kurbo};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, normalize, outline_stroke};
use vectorcraft_render::effects::stroke::outline_region;
use vectorcraft_stitch::plan::{self, Style};
use vectorcraft_stitch::{Block, Command, SAME_POINT, Stitch, Thread, pt_to_mm};

use super::Options;
use super::cache::{UnitCache, UnitKey};
use super::plan::{Choice, DesignPlan, Origin, Part};
use super::sequence::{CLEARANCE, Facts, Planned, Sequence};
use super::unit::{PlanError, ROUND_JOINS, UnitRequest, UnitStitches, cut_to, plan_unit};

const TEXT_LEFT_OUT: &str = "type is not stitched: outline it (Type › Create Outlines) and export again";
const IMAGES_LEFT_OUT: &str = "images are not stitched: trace them (Object › Image Trace) first";
const LIVE_LEFT_OUT: &str = "live blends, envelopes, meshes and repeats are not stitched: expand them first";
const PLACED_LEFT_OUT: &str = "placed documents are not stitched: embed them first (Links › Embed)";
const INTERTWINE_LEFT_OUT: &str = "intertwined objects are not stitched: release them first (Object › Intertwine › Release)";
const EXPANDED_AWAY: &str = "a negative expand left nothing of some filled shapes, so they are not stitched: shrink them less";
const GRADIENT_LEFT_OUT: &str = "gradients and patterns are painted in the colour of their first stop; a design sews one thread per block";
const OUT_OF_RANGE: &str = "art far outside the document's range (or with coordinates that aren't numbers) is not stitched";
/// A stroke at least this wide (millimetres) is sewn as a satin column, a narrower one as a running
/// line (unless the `stitch` option says otherwise): below about a millimetre the two rails are too
/// close for the crossings to lie side by side. An estimate.
pub(super) const SATIN_MIN_WIDTH: f64 = 1.0;
/// How far a literal stitch line's curves are flattened into needle points (millimetres): the
/// lines a design opens as are straight, so this only matters for one edited into curves; far
/// below the 0.1 mm a machine counts in. The planner flattens the art it stitches as finely (its
/// own `FLATTEN`, in `vectorcraft_stitch::plan`), for the same reason.
const LITERAL_FLATTEN: f64 = 0.05;
/// The column width given for what is not a satin column: fills and running lines have none.
const NO_WIDTH: f64 = 0.0;
/// The most stitches a design may have: far beyond any hoop (large designs run to a few hundred
/// thousand), and a bound on the memory an export can take.
const MAX_DESIGN_STITCHES: usize = 2_000_000;
/// How deep symbols placed in symbols are sewn: deeper art is left out, a bound on a walk through
/// symbols that place one another.
const MAX_SYMBOL_NESTING: u32 = 8;
/// The most symbol instances a walk places, nested ones included: far more than a real design
/// places (a symbol scattered a few thousand times, each holding a few others), and a bound on a
/// file whose symbol places itself several times at every level, whose placements multiply by the
/// nesting (10 a level for 8 levels: 10^8 placements, each a copy of the symbol's art). An estimate.
const MAX_SYMBOL_PLACEMENTS: u64 = 100_000;
const PLACEMENTS_LEFT_OUT: &str = "symbols placed more than 100,000 times (placing one another) are left out past that";
/// `bp` (document points) in millimetres, as the stitch planner counts.
fn in_mm(bp: &BezPath) -> BezPath {
    let mut mm = bp.clone();
    mm.apply_affine(Affine::scale(vectorcraft_stitch::MM_PER_PT));
    mm
}

/// `path`'s subpaths, each a path of its own (as drawn: closed or not).
fn subpaths(path: &BezPath) -> Vec<BezPath> {
    let mut out: Vec<BezPath> = Vec::new();
    for el in path.elements() {
        match el {
            PathEl::MoveTo(_) => out.push(BezPath::from_vec(vec![*el])),
            _ => match out.last_mut() {
                Some(sub) => sub.push(*el),
                None => out.push(BezPath::from_vec(vec![*el])),
            },
        }
    }
    out
}

/// The plan of `doc`'s visible art with `opts`: its sewing units in the blocks they are sewn in,
/// in sewing order ([`Sequence`]), each block in a thread of the document's palette
/// ([`super::thread_palette`]), and the warnings for what was left out or approximated; an error
/// when the design would need too many stitches, or when the plan is cancelled
/// ([`vectorcraft_doc::cancel`]: checked before each unit, and while a fill's narrow parts are
/// looked for and cut out). Each unit planned is reported as a step of the plan's progress. With
/// a `cache`, each unit's stitches are taken from it when it keeps them for exactly what the unit
/// is planned from, and kept there when it doesn't ([`UnitCache`]); without, every unit is planned
/// afresh.
pub(crate) fn digitize(doc: &Document, opts: &Options, cache: Option<&Mutex<UnitCache>>) -> Result<DesignPlan, PlanError> {
    let seq = Sequence::new(opts.params(Style::Running, NO_WIDTH));
    // The document as it prints: appearances expanded, what can't be sewn said.
    let sewable = super::printed::sewable(doc);
    // Not about the art: said, but not counted as art left out.
    let mut notes = Vec::new();
    if let Some(ThreadPalette::Unknown(palette)) = doc.embroidery.as_deref().and_then(|e| e.palette.as_ref()) {
        let default = super::DEFAULT_PALETTE.id();
        notes.push(format!(
            "the document's thread palette {palette} is not one this version knows, so its threads are matched to the {default} palette"
        ));
    }
    let warnings: Vec<String> = sewable.warnings.iter().map(|w| w.to_string()).collect();
    let printed = &*sewable.doc;
    let mut symbols = HashMap::new();
    let total = printed.layers.iter().map(|l| units_under(printed, l, 0, &mut symbols)).fold(0u64, u64::saturating_add);
    let progress = Progress { done: 0, total };
    let mut d = Digitizer {
        doc: printed,
        expanded: &sewable.expanded,
        opts,
        seq,
        warnings,
        nest: 0,
        placements: 0,
        clip: None,
        owner: None,
        instance: None,
        stitches: 0,
        failed: None,
        cache,
        progress,
    };
    d.run();
    match d.failed {
        Some(e) => Err(e),
        None => {
            let mut plan = d.seq.plan(doc.title.clone(), d.warnings, super::thread_palette(doc));
            plan.notes = notes;
            Ok(plan)
        }
    }
}

/// How one sewing unit is sewn: what it is, the settings it is planned with
/// ([`Digitizer::settings`]), its stitch style and the rule that chose it, its satin column's width ([`NO_WIDTH`] for fills
/// and running lines), and whether the clip groups around it cut its stitches (a stroke's; a fill's
/// area is cut before it is planned).
#[derive(Clone, Copy)]
struct Sewing<'a> {
    unit: Origin,
    opts: &'a Options,
    style: Style,
    why: Choice,
    width: f64,
    clip_stitches: bool,
}

/// The document walk that plans each object's stitches.
struct Digitizer<'a> {
    /// The document as it prints ([`super::printed::sewable`]).
    doc: &'a Document,
    /// The objects expanded in it: a person selects each of them, whatever pieces it became.
    expanded: &'a HashSet<NodeId>,
    /// The export's options. What a unit is sewn with comes from [`Digitizer::settings`]; these
    /// are read directly only for what belongs to the design rather than to a unit: the jump and
    /// trim lengths between objects, and across the cuts in a literal run.
    opts: &'a Options,
    /// The blocks planned so far, in sewing order.
    seq: Sequence,
    warnings: Vec<String>,
    /// Symbol art being stitched inside symbol instances.
    nest: u32,
    /// The symbol instances placed so far, against [`MAX_SYMBOL_PLACEMENTS`].
    placements: u64,
    /// The region the clip groups around the current object leave visible (millimetres), `None`
    /// outside any clip group.
    clip: Option<(BezPath, FillRule)>,
    /// The group the walk is in: literal stitch runs ([`StitchRun`]) of one group are one colour
    /// block of the file they came from, and stay one.
    owner: Option<NodeId>,
    /// The symbol instance in the document whose art the walk is in (the outermost, where symbols
    /// nest), `None` outside symbols: its units are placed by it ([`Origin::instance`]).
    instance: Option<NodeId>,
    /// The stitches planned so far, against [`MAX_DESIGN_STITCHES`].
    stitches: usize,
    /// Why the design can't be stitched (too many stitches, or the plan was cancelled); the walk
    /// stops planning once set.
    failed: Option<PlanError>,
    /// The units kept from earlier plans of the document, `None` to plan every unit afresh.
    cache: Option<&'a Mutex<UnitCache>>,
    /// The units the walk has come to so far, of about how many.
    progress: Progress,
}

/// How far a plan has got: the units come to, of about how many ([`units_under`]).
struct Progress {
    done: u64,
    total: u64,
}

/// What a plan's progress is reported as doing.
const PLANNING: &str = "Planning the stitches";

/// About how many units the walk plans under `n` (`nest` symbols deep), the progress it reports:
/// each visible, painted fill and stroke of a visible path, each literal run, a symbol's art once
/// for each of its instances (counted once per symbol and depth, `symbols`). What the walk then
/// leaves out (a pattern, a negative expand that leaves nothing, a stroke cut away by a clip
/// group, placements past [`MAX_SYMBOL_PLACEMENTS`]) is counted all the same.
fn units_under(doc: &Document, n: &Node, nest: u32, symbols: &mut HashMap<(String, u32), u64>) -> u64 {
    if !n.visible {
        return 0;
    }
    let items = || n.appearance.items.iter().filter(|i| i.visible() && painted(i)).count() as u64;
    match &n.kind {
        NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. } => 0,
        NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
            children.iter().map(|c| units_under(doc, c, nest, symbols)).fold(0, u64::saturating_add)
        }
        NodeKind::Path { guide: true, .. } => 0,
        NodeKind::Path { .. } if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) => 1,
        NodeKind::Path { .. } | NodeKind::Compound { .. } => items(),
        // A symbol's units at a depth are counted once: a symbol that places itself many times at
        // every level would otherwise be walked as often as it is placed.
        NodeKind::SymbolInstance { symbol, .. } if nest < MAX_SYMBOL_NESTING => {
            let key = (symbol.clone(), nest);
            if let Some(&known) = symbols.get(&key) {
                return known;
            }
            let units = doc.symbols.iter().find(|s| &s.name == symbol).map_or(0, |s| units_under(doc, &s.art, nest + 1, symbols));
            symbols.insert(key, units);
            units
        }
        _ => 0,
    }
}

impl<'a> Digitizer<'a> {
    fn run(&mut self) {
        for layer in &self.doc.layers {
            self.node(layer);
        }
    }

    /// The settings sewing unit `unit` is planned with: the one place a unit's stitch style,
    /// stitch parameters and `expand` are decided. Today every unit takes the export's options.
    /// Embroidery settings will be resolved here, by the unit's origin, in the order decided
    /// (`docs/embroidery.md` › Architecture): the object's own value, then the export's option,
    /// then the document's embroidery settings, then the planner's defaults.
    fn settings(&self, _unit: &Origin) -> &'a Options {
        self.opts
    }

    /// What the unit that sews `part` of `n` sews.
    fn origin(&self, n: &Node, part: Part) -> Origin {
        Origin { node: n.id, part, instance: self.instance }
    }

    /// One more unit to plan, reported as a step of the plan's progress; `false` when the walk
    /// stops planning (too many stitches already, or the plan was cancelled).
    fn step(&mut self) -> bool {
        if self.failed.is_some() {
            return false;
        }
        if cancel::is_cancelled() {
            self.failed = Some(PlanError::Cancelled);
            return false;
        }
        let p = &mut self.progress;
        p.done = p.done.saturating_add(1);
        cancel::progress(p.done, p.total.max(p.done), PLANNING);
        true
    }

    /// Count `n` more stitches against [`MAX_DESIGN_STITCHES`]; `false`, and the walk stops,
    /// when the design would need more.
    fn count(&mut self, n: usize) -> bool {
        self.stitches = self.stitches.saturating_add(n);
        if self.stitches > MAX_DESIGN_STITCHES {
            self.failed = Some(PlanError::TooLarge(format!(
                "the design would need more than {MAX_DESIGN_STITCHES} stitches: use a longer stitch, wider spacing or less art"
            )));
            return false;
        }
        true
    }

    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
    }

    fn node(&mut self, n: &Node) {
        // Nothing more is planned once the design can't be (too many stitches, cancelled): the
        // rest of the walk would only copy symbol art.
        // Nothing at no opacity prints.
        if !n.visible || n.opacity <= 0.0 || self.failed.is_some() {
            return;
        }
        let added = self.seq.added();
        // The pieces an expanded object became are placed by it, as a symbol's art is by its
        // instance.
        let outer = self.instance;
        if outer.is_none() && self.expanded.contains(&n.id) {
            self.instance = Some(n.id);
        }
        self.kind(n);
        self.instance = outer;
        // An object (or group) that asks for it: the thread is cut after its last stitch.
        if n.extras().embroidery.as_deref().is_some_and(|e| e.trim_after) && self.seq.added() > added {
            self.seq.trim_after_since(added);
        }
    }

    fn kind(&mut self, n: &Node) {
        match &n.kind {
            // Template and non-printing layers are references, not art: a machine sews only what
            // would print.
            NodeKind::Layer { template: true, .. } | NodeKind::Layer { printable: false, .. } => {}
            NodeKind::Layer { children, clip: true, .. } | NodeKind::Group { children, clip: true } => self.clip_group(children),
            NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
                let outer = self.owner.replace(n.id);
                for c in children {
                    self.node(c);
                }
                self.owner = outer;
            }
            NodeKind::Path { path, rule, guide, .. } => {
                if *guide {
                    return;
                }
                match n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()) {
                    Some(run) => self.literal(n, &path.to_bezpath(), run),
                    None => self.shape(n, &path.to_bezpath(), *rule),
                }
            }
            NodeKind::Compound { children, rule } => {
                let mut bp = BezPath::new();
                for c in children {
                    if let Some(p) = c.path_data() {
                        bp.extend(p.to_bezpath());
                    }
                }
                self.shape(n, &bp, *rule);
            }
            NodeKind::SymbolInstance { symbol, xf } => {
                let Some(sym) = self.doc.symbols.iter().find(|s| &s.name == symbol) else { return };
                if self.nest >= MAX_SYMBOL_NESTING {
                    return;
                }
                self.placements += 1;
                if self.placements > MAX_SYMBOL_PLACEMENTS {
                    self.warn(PLACEMENTS_LEFT_OUT);
                    return;
                }
                // Its art as the instance shows it: stained when the instance is.
                let mut art = vectorcraft_brush::instance_art(&sym.art, n);
                art.transform(*xf, false);
                self.nest += 1;
                // Its art is placed by the outermost instance: the one in the document.
                let outer = self.instance;
                self.instance = Some(outer.unwrap_or(n.id));
                self.node(&art);
                self.instance = outer;
                self.nest -= 1;
            }
            NodeKind::Text(_) => self.warn(TEXT_LEFT_OUT),
            NodeKind::Image(_) => self.warn(IMAGES_LEFT_OUT),
            NodeKind::PlacedDocument(_) => self.warn(PLACED_LEFT_OUT),
            NodeKind::Intertwine { .. } => self.warn(INTERTWINE_LEFT_OUT),
            _ => self.warn(LIVE_LEFT_OUT),
        }
    }

    /// A run of literal stitches (read from a machine file): the path's points sewn as they are,
    /// in the colour of its stroke, one block with the other runs of its group.
    fn literal(&mut self, n: &Node, bp: &BezPath, run: &StitchRun) {
        if !self.step() {
            return;
        }
        let paint = n.appearance.items.iter().filter(|i| i.visible()).find_map(|i| match i {
            AppearanceItem::Stroke(s) => Some(&s.paint),
            AppearanceItem::Fill(_) => None,
        });
        let color = match paint.map(solid) {
            Some(Colour::Solid(c) | Colour::Approx(c)) => c,
            // Unpainted (or a pattern): it shows nothing, so it sews nothing.
            _ => return,
        };
        let mm = in_mm(bp);
        if !plan::in_range(&mm) {
            self.warn(OUT_OF_RANGE);
            return;
        }
        let mut points: Vec<Stitch> = Vec::new();
        kurbo::flatten(mm.iter(), LITERAL_FLATTEN, |el| {
            if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
                points.push(Stitch::sew(p));
            }
        });
        // A single stitch is stored as a line that goes nowhere: one needle point, unless the run
        // says it is two at that spot.
        if let [a, b] = points.as_slice()
            && a.distance(*b) < SAME_POINT
            && !run.double_point
        {
            points.pop();
        }
        if let Some(clip) = &self.clip {
            points = cut_to(points, clip, &self.opts.params(Style::Running, NO_WIDTH));
        }
        if points.is_empty() || !self.count(points.len()) {
            return;
        }
        let mut block = Block::new(Thread::rgb(color));
        if run.trim_after
            && let Some(&last) = points.last()
        {
            points.push(Stitch::new(last.x, last.y, Command::Trim));
        }
        block.stitches = points;
        let origin = self.origin(n, Part::Literal);
        let literal = self.owner.map(|g| (g, run.trim_before));
        let facts = Facts { origin, params: None, columns: None, satin_widths: None, why: Choice::Literal };
        self.seq.add(Planned { facts, block, literal });
    }

    /// A clip group, as it paints: the clipping path (its first child) paints its fills behind the
    /// clipped art and its strokes over it, unclipped; the rest is sewn only inside the clipping
    /// path. Nothing to clip by (an empty clipping path) hides the clipped art.
    fn clip_group(&mut self, children: &[Arc<Node>]) {
        let Some((clip, rest)) = children.split_first() else { return };
        let Some((region, rule)) = vectorcraft_render::effects::clip_outline(clip) else { return };
        let region = in_mm(&region);
        // A clipping path out of range clips nothing that can be stitched: its art is left out.
        if !plan::in_range(&region) {
            self.warn(OUT_OF_RANGE);
            return;
        }
        let paint = clip.clip_paint();
        if let Some(fill) = &paint.fill {
            self.node(fill);
        }
        let inner = match &self.clip {
            None => (region, rule),
            Some((outer, outer_rule)) => {
                let both = boolean(&PathData::from_bezpath(outer), *outer_rule, &PathData::from_bezpath(&region), rule, BoolOp::Intersect);
                (both.to_bezpath(), FillRule::NonZero)
            }
        };
        let outer = self.clip.replace(inner);
        for c in rest {
            self.node(c);
        }
        self.clip = outer;
        if let Some(stroke) = &paint.stroke {
            self.node(stroke);
        }
    }

    /// Stitch `bp` (document points) once per visible appearance item: each one a sewing unit,
    /// one fill or one stroke of `n`.
    fn shape(&mut self, n: &Node, bp: &BezPath, rule: FillRule) {
        let mm = in_mm(bp);
        if !plan::in_range(&mm) {
            if n.appearance.items.iter().any(|i| i.visible() && painted(i)) {
                self.warn(OUT_OF_RANGE);
            }
            return;
        }
        // Which of the object's fills, or of its strokes, each item is.
        let (mut fills, mut strokes) = (0, 0);
        for item in &n.appearance.items {
            let part = match item {
                AppearanceItem::Fill(_) => {
                    fills += 1;
                    Part::Fill(fills - 1)
                }
                AppearanceItem::Stroke(_) => {
                    strokes += 1;
                    Part::Stroke(strokes - 1)
                }
            };
            if !item.visible() {
                continue;
            }
            if painted(item) && !self.step() {
                return;
            }
            let unit = self.origin(n, part);
            let opts = self.settings(&unit);
            match item {
                // A fill is filled unless the `stitch` option asks for another way of stitching it.
                AppearanceItem::Fill(fill) => {
                    let (style, why) = match opts.forced() {
                        // A satin needs a width, which a fill doesn't have: it is filled, with a
                        // fill's settings, `expand` and narrow parts, as without the option.
                        Some(Style::Satin) => (Style::Fill, Choice::SatinFilled),
                        Some(style) => (style, Choice::Forced),
                        None => (Style::Fill, Choice::Fill),
                    };
                    // Paint that isn't sewn is said so (or passed by) before any path operation
                    // runs on its area.
                    if !matches!(solid(&fill.paint), Colour::Solid(_) | Colour::Approx(_)) {
                        self.colour(Sewing { unit, opts, style, why, width: NO_WIDTH, clip_stitches: false }, &mm, rule, &fill.paint);
                        continue;
                    }
                    // A fill sewn as a running line follows its outline, as a stroke does: the
                    // clip groups around it cut its stitches, rather than its area, whose cut
                    // edge would be sewn as a line the art doesn't have.
                    if style == Style::Running {
                        self.colour(Sewing { unit, opts, style, why, width: NO_WIDTH, clip_stitches: true }, &mm, rule, &fill.paint);
                        continue;
                    }
                    let (area, area_rule) = if style == Style::Fill { self.expanded(&mm, rule, opts.expand()) } else { (mm.clone(), rule) };
                    let (area, area_rule) = self.clipped(&area, area_rule);
                    self.colour(Sewing { unit, opts, style, why, width: NO_WIDTH, clip_stitches: false }, &area, area_rule, &fill.paint);
                }
                AppearanceItem::Stroke(stroke) => {
                    let width = pt_to_mm(stroke.width).max(0.0);
                    // A stroke that paints an area its path doesn't run down the middle of
                    // (dashed, aligned inside or outside, of varying width, with arrowheads): that
                    // area, as on the canvas, is filled, its narrow parts satin columns that
                    // follow it; unless the `stitch` option sews it along its path.
                    if paints_off_its_path(stroke, bp) && matches!(opts.forced(), None | Some(Style::Fill)) {
                        let outline = outline_region(&PathData::from_bezpath(bp), rule, stroke).to_bezpath();
                        let (area, area_rule) = self.expanded(&in_mm(&outline), FillRule::NonZero, opts.expand());
                        let (area, area_rule) = self.clipped(&area, area_rule);
                        let sewing = Sewing { unit, opts, style: Style::Fill, why: Choice::StrokeOutlined, width: NO_WIDTH, clip_stitches: false };
                        self.colour(sewing, &area, area_rule, &stroke.paint);
                        continue;
                    }
                    match opts.forced() {
                        // A stroke filled is the area it paints, filled: its outline at its width,
                        // caps and joins (not the inside of the path it follows).
                        Some(Style::Fill) => {
                            let cap = match stroke.cap {
                                LineCap::Butt => Cap::Butt,
                                LineCap::Round => Cap::Round,
                                LineCap::Square => Cap::Square,
                            };
                            let join = match stroke.join {
                                LineJoin::Miter => Join::Miter,
                                LineJoin::Round => Join::Round,
                                LineJoin::Bevel => Join::Bevel,
                            };
                            let outline = outline_stroke(&PathData::from_bezpath(&mm), width, cap, join, stroke.miter_limit).to_bezpath();
                            let (area, area_rule) = self.expanded(&outline, FillRule::NonZero, opts.expand());
                            let (area, area_rule) = self.clipped(&area, area_rule);
                            let sewing = Sewing { unit, opts, style: Style::Fill, why: Choice::StrokeFilled, width: NO_WIDTH, clip_stitches: false };
                            self.colour(sewing, &area, area_rule, &stroke.paint);
                        }
                        // A satin column when it is wide enough to have two rails, a running line
                        // when not, unless the `stitch` option says which.
                        forced => {
                            let (style, why) = match forced {
                                Some(style) => (style, Choice::Forced),
                                None if width >= SATIN_MIN_WIDTH => (Style::Satin, Choice::WideStroke),
                                None => (Style::Running, Choice::NarrowStroke),
                            };
                            if style == Style::Satin && width > plan::MAX_SATIN_WIDTH {
                                let widest = plan::MAX_SATIN_WIDTH;
                                self.warn(&format!(
                                    "strokes wider than {widest} mm are sewn as satin columns {widest} mm wide: fill them instead (stitch fill)"
                                ));
                            }
                            self.colour(Sewing { unit, opts, style, why, width, clip_stitches: true }, &mm, rule, &stroke.paint);
                        }
                    }
                }
            }
        }
    }

    /// A filled area (millimetres) grown all round by `e` millimetres, the unit's `expand` (shrunk
    /// when it is negative), so that neighbouring fills meet without a gap: its union with a band
    /// twice as wide along its outlines, round at the corners, or what is left of it without that
    /// band. A subpath that encloses no area (sewn as a line) is left as it is. Unchanged when `e`
    /// is 0. The clip groups around it cut the result ([`Digitizer::clipped`]): it grows only where
    /// it shows.
    fn expanded(&mut self, area: &BezPath, rule: FillRule, e: f64) -> (BezPath, FillRule) {
        if e.abs() < SAME_POINT {
            return (area.clone(), rule);
        }
        let (mut shapes, mut lines) = (BezPath::new(), BezPath::new());
        for sub in subpaths(area) {
            // Closed, as paint closes it; whether it encloses an area is judged closed too (an
            // open subpath's area would depend on where it lies on the page).
            let mut closed = sub.clone();
            if closed.elements().last() != Some(&PathEl::ClosePath) {
                closed.close_path();
            }
            if closed.area().abs() >= plan::MIN_AREA {
                shapes.extend(closed);
            } else {
                lines.extend(sub);
            }
        }
        if shapes.elements().is_empty() {
            return (area.clone(), rule);
        }
        // The region the shape covers, by its rule: overlapping or crossing outlines resolved into
        // its edge, so the band runs along that edge only (a band along an outline inside the
        // region would cut a groove through it).
        let region = normalize(&PathData::from_bezpath(&shapes), rule);
        let band = outline_stroke(&region, 2.0 * e.abs(), Cap::Round, Join::Round, ROUND_JOINS);
        let op = if e > 0.0 { BoolOp::Union } else { BoolOp::Difference };
        let mut out = boolean(&region, FillRule::NonZero, &band, FillRule::NonZero, op).to_bezpath();
        if out.elements().is_empty() && lines.elements().is_empty() {
            self.warn(EXPANDED_AWAY);
        }
        out.extend(lines);
        (out, FillRule::NonZero)
    }

    /// A filled area (millimetres) as the clip groups around it leave it: its intersection with
    /// the clipping path, cut exactly before it is planned; the area itself outside any.
    fn clipped(&self, area: &BezPath, rule: FillRule) -> (BezPath, FillRule) {
        match &self.clip {
            Some((clip, clip_rule)) => {
                let cut = boolean(&PathData::from_bezpath(area), rule, &PathData::from_bezpath(clip), *clip_rule, BoolOp::Intersect);
                (cut.to_bezpath(), FillRule::NonZero)
            }
            None => (area.clone(), rule),
        }
    }

    /// Plan one appearance item as `sewing` says, or say what could not be stitched.
    fn colour(&mut self, sewing: Sewing<'_>, bp: &BezPath, rule: FillRule, paint: &Paint) {
        match solid(paint) {
            Colour::None => {}
            Colour::Solid(color) => self.plan(sewing, bp, rule, color),
            Colour::Approx(color) => {
                self.warn(GRADIENT_LEFT_OUT);
                self.plan(sewing, bp, rule, color);
            }
            Colour::Pattern => self.warn(GRADIENT_LEFT_OUT),
        }
    }

    fn plan(&mut self, sewing: Sewing<'_>, bp: &BezPath, rule: FillRule, color: [u8; 3]) {
        if self.failed.is_some() {
            return;
        }
        let Sewing { unit, opts, style, why, width, clip_stitches } = sewing;
        let params = opts.params(style, width);
        // Where the needle will be: at the end of the block this object is likely to join (its
        // bounds estimated from the path's), so it starts as near there as it can.
        let bounds = bp.bounding_box();
        let [left, right] = params.compensation(match style {
            Style::Satin => params.width,
            // A fill row is at most as long as the shape is across.
            Style::Running | Style::Fill => bounds.width().hypot(bounds.height()),
        });
        let reach = match style {
            Style::Satin => params.width.min(plan::MAX_SATIN_WIDTH) / 2.0,
            Style::Running | Style::Fill => 0.0,
        } + left.max(right)
            + CLEARANCE;
        let mut estimate = Some(bounds).filter(|r| r.is_finite() && reach.is_finite()).map(|r| r.inflate(reach, reach));
        if let (Some(r), Some((clip, _))) = (estimate, &self.clip) {
            estimate = Some(r.intersect(clip.bounding_box()));
        }
        let from = self.seq.needle(self.seq.target(color, estimate));
        let request = UnitRequest {
            shape: bp,
            rule,
            params,
            // A fill's narrow parts are satin columns, unless the `stitch` option forces tatami
            // (on a fill, or on a stroke filled).
            columns: (style == Style::Fill && opts.forced() != Some(Style::Fill)).then(|| opts.params(Style::Satin, NO_WIDTH)),
            from,
            clip: if clip_stitches { self.clip.as_ref() } else { None },
        };
        let planned = match self.planned(&request) {
            Ok(planned) => planned,
            Err(e) => {
                self.failed = Some(e);
                return;
            }
        };
        // Kept in the cache too, the stitches are copied into the block.
        let (stitches, columns, widths) = match Arc::try_unwrap(planned) {
            Ok(UnitStitches { stitches, columns, widths }) => (stitches, columns, widths),
            Err(kept) => (kept.stitches.clone(), kept.columns, kept.widths),
        };
        let satin_widths = match plan::sewn_style(&params) {
            Style::Satin => Some([params.width; 2]),
            Style::Fill => widths,
            Style::Running => None,
        };
        if stitches.is_empty() || !self.count(stitches.len()) {
            return;
        }
        let mut block = Block::new(Thread::rgb(color));
        block.stitches = stitches;
        let facts = Facts { origin: unit, params: Some(params), columns, satin_widths, why };
        self.seq.add(Planned { facts, block, literal: None });
    }

    /// `request`'s stitches: those the cache keeps for it, else planned, and kept.
    fn planned(&self, request: &UnitRequest<'_>) -> Result<Arc<UnitStitches>, PlanError> {
        let Some(cache) = self.cache else { return plan_unit(request).map(Arc::new) };
        let key = UnitKey::of(request);
        // A cache that a panic elsewhere left locked is passed by: planning afresh is always right.
        if let Some(kept) = cache.lock().ok().and_then(|mut c| c.get(&key)) {
            return Ok(kept);
        }
        let planned = Arc::new(plan_unit(request)?);
        if let Ok(mut c) = cache.lock() {
            c.insert(key, planned.clone());
        }
        Ok(planned)
    }
}

/// Whether an appearance item is painted at all (not `none`): only painted items are sewn, and
/// counted as the plan's progress.
/// Whether stroke `st` of path `bp` paints an area its path doesn't run down the middle of: dashed,
/// aligned inside or outside a closed path, of varying width, or with arrowheads.
fn paints_off_its_path(st: &StrokeLayer, bp: &BezPath) -> bool {
    let closed = bp.elements().iter().any(|e| matches!(e, PathEl::ClosePath));
    st.dash.is_some()
        || st.profile.is_some()
        || st.pressure.is_some()
        || st.start_arrow.is_some()
        || st.end_arrow.is_some()
        || (closed && st.align != StrokeAlign::Center)
}

fn painted(item: &AppearanceItem) -> bool {
    let paint = match item {
        AppearanceItem::Fill(f) => &f.paint,
        AppearanceItem::Stroke(s) => &s.paint,
    };
    !matches!(paint, Paint::None)
}

/// A paint's stitched colour.
enum Colour {
    /// Painted nothing: no stitches.
    None,
    /// A flat colour, exactly.
    Solid([u8; 3]),
    /// A gradient: its first stop's colour, which the caller warns is an approximation.
    Approx([u8; 3]),
    /// A pattern: not stitched.
    Pattern,
}

fn solid(paint: &Paint) -> Colour {
    match paint {
        Paint::None => Colour::None,
        Paint::Solid { color, .. } => {
            let [r, g, b, _] = color.to_rgba8(1.0);
            Colour::Solid([r, g, b])
        }
        Paint::Gradient(g) => match g.gradient.stops.first() {
            Some(stop) => {
                let [r, g, b, _] = stop.color.to_rgba8(1.0);
                Colour::Approx([r, g, b])
            }
            None => Colour::None,
        },
        Paint::Pattern { .. } => Colour::Pattern,
    }
}
