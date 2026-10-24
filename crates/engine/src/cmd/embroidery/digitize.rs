//! The document walk that plans each visible object's stitches into the design's plan: what is
//! sewn (what would print), the sewing units (each fill and stroke of an object, each run of
//! literal stitches read from a file) and the settings each is planned with, clip groups, the
//! colour of each block, and the limit on a design's size.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use vectorcraft_color::Paint;
use vectorcraft_doc::cancel;
use vectorcraft_doc::embroidery::{PartKind, StitchRun, ThreadPalette};
use vectorcraft_doc::{AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind, StrokeAlign, StrokeLayer};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, PathEl, Point, Rect, Shape, kurbo};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, normalize, outline_stroke};
use vectorcraft_render::effects::stroke::outline_region;
use vectorcraft_stitch::plan::{self, Style};
use vectorcraft_stitch::{Block, Command, SAME_POINT, Stitch, Thread, pt_to_mm};

use super::Options;
use super::cache::{Shaped, Shared, UnitCache, UnitKey, clip_words, path as path_words, rule_word};
use super::plan::{Choice, DesignPlan, ForcedBy, MAX_WARNED_OBJECTS, Origin, Part};
use super::resolve::{DesignSettings, DocLayer, Found, PartLayer, UnitSettings};
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
/// line (unless the `stitch` option says otherwise): the planner's narrowest satin column.
pub(super) use vectorcraft_stitch::plan::SATIN_MIN_WIDTH;
/// How far a literal stitch line's curves are flattened into needle points (millimetres): the
/// lines a design opens as are straight, so this only matters for one edited into curves; far
/// below the 0.1 mm a machine counts in. The planner flattens the art it stitches as finely (its
/// own `FLATTEN`, in `vectorcraft_stitch::plan`), for the same reason.
const LITERAL_FLATTEN: f64 = 0.05;
/// The column width given for what is not a satin column: fills and running lines have none.
const NO_WIDTH: f64 = 0.0;
/// The first word of a kept area's key ([`Digitizer::area`]), saying what the path operations made
/// the area from: a fill's own area, a stroke's painted area (dashed, aligned, of varying width,
/// with arrowheads), a stroke filled at its width.
const AREA_FILL: u64 = 0;
const AREA_STROKE_PAINTED: u64 = 1;
const AREA_STROKE_FILLED: u64 = 2;
/// What a clip group inside another leaves visible: its region with the outer one's.
const AREA_CLIPS: u64 = 3;
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
    // The document's embroidery settings, read once a plan.
    let doc_layer = DocLayer::of(doc);
    let (design, trim_note) = DesignSettings::new(opts, &doc_layer);
    let seq = Sequence::new(design.connect, design.join);
    // The units this plan uses and adds are its own: the cache drops none for another of them.
    if let Some(mut kept) = cache.and_then(|c| c.lock().ok()) {
        kept.begin();
    }
    // The document as it prints: appearances expanded, what can't be sewn said.
    let sewable = super::printed::sewable(doc, opts.forced());
    // Notes that aren't about the art (a thread palette this version doesn't know): told with the
    // warnings, but not counted as art left out.
    let mut notes: Vec<String> = doc_layer.notes.iter().chain(trim_note.as_ref()).map(|n| n.to_string()).collect();
    if let Some(ThreadPalette::Unknown(palette)) = doc.embroidery.as_deref().and_then(|e| e.palette.as_ref()) {
        let default = super::DEFAULT_PALETTE.id();
        notes.push(format!(
            "the document's thread palette {palette} is not one this version knows, so its threads are matched to the {default} palette"
        ));
    }
    let warnings: Vec<String> = sewable.warnings.iter().map(|w| w.to_string()).collect();
    // The objects what the expansion said is about, as many as a warning lists.
    let mut warned: BTreeMap<String, Vec<NodeId>> = BTreeMap::new();
    for (w, id) in &sewable.about {
        let ids = warned.entry(w.to_string()).or_default();
        if ids.len() < MAX_WARNED_OBJECTS && !ids.contains(id) {
            ids.push(*id);
        }
    }
    let printed = &*sewable.doc;
    let mut symbols = HashMap::new();
    let total = printed.layers.iter().map(|l| units_under(printed, l, 0, &mut symbols)).fold(0u64, u64::saturating_add);
    let progress = Progress { done: 0, total };
    let mut d = Digitizer {
        doc: printed,
        expanded: &sewable.expanded,
        opts,
        doc_layer: &doc_layer,
        design,
        seq,
        warnings,
        warned,
        object: None,
        nest: 0,
        placements: 0,
        clip: None,
        owner: None,
        instance: None,
        stitches: 0,
        failed: None,
        cache,
        progress,
        occurrences: HashMap::new(),
        found: Found::default(),
    };
    d.run();
    notes.extend(d.found.notes().map(String::from));
    match d.failed {
        Some(e) => Err(e),
        None => {
            // Named as the file it was opened from named it, else after the document.
            let name = doc.embroidery.as_ref().and_then(|e| e.label.clone()).unwrap_or_else(|| doc.title.clone());
            let mut plan = d.seq.plan(name, d.warnings, super::thread_palette(doc));
            plan.warned = d.warned;
            plan.notes = notes;
            plan.start = doc.embroidery.as_ref().and_then(|e| e.start).map(|p| Point::new(pt_to_mm(p.x), pt_to_mm(p.y)));
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
    settings: UnitSettings<'a>,
    style: Style,
    why: Choice,
    width: f64,
    clip_stitches: bool,
}

/// The region the clip groups around an object leave visible (millimetres, with its fill rule),
/// its words as a key part shared by the keys of everything inside ([`Shared`]), and its bounds:
/// each worked out once a clip group, not once an object.
struct Clip {
    region: (BezPath, FillRule),
    key: Arc<Shared>,
    bounds: Rect,
}

/// The document walk that plans each object's stitches.
struct Digitizer<'a> {
    /// The document as it prints ([`super::printed::sewable`]).
    doc: &'a Document,
    /// The objects expanded in it: a person selects each of them, whatever pieces it became.
    expanded: &'a HashSet<NodeId>,
    /// The export's options (or a read command's), the layer every unit's settings take after the
    /// object's own ([`Digitizer::settings`]).
    opts: &'a Options,
    /// The document's embroidery settings, the layer after the export's.
    doc_layer: &'a DocLayer,
    /// What belongs to the design rather than to a unit: the jump and trim lengths between
    /// objects, and across the cuts in a literal run.
    design: DesignSettings,
    /// The blocks planned so far, in sewing order.
    seq: Sequence,
    warnings: Vec<String>,
    /// The objects each warning is about ([`DesignPlan::warned`]).
    warned: BTreeMap<String, Vec<NodeId>>,
    /// The object the walk is in, as a person selects it (a symbol's instance for its art), `None`
    /// above the objects (in a layer, between its children).
    object: Option<NodeId>,
    /// Symbol art being stitched inside symbol instances.
    nest: u32,
    /// The symbol instances placed so far, against [`MAX_SYMBOL_PLACEMENTS`].
    placements: u64,
    /// The region the clip groups around the current object leave visible, `None` outside any
    /// clip group.
    clip: Option<Clip>,
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
    /// How many units the plan holds so far of each object a person selects and part
    /// ([`Origin::selected`], [`Origin::part`]): the next one's [`Facts::occurrence`] is one more.
    occurrences: HashMap<(NodeId, Part), usize>,
    /// What reading the objects' settings found that the plan says.
    found: Found,
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

    /// The settings sewing unit `unit` of `n` (the node walked: a symbol's art as its instance
    /// places it, an expanded object's piece) is planned with: the one place a unit's stitch style,
    /// stitch parameters and `expand` are decided, in the order decided (`resolve`): the object's
    /// own value, then the export's option, then the document's embroidery settings, then the
    /// planner's defaults. The object's are those `n` keeps for the kind of part the unit sews.
    fn settings(&mut self, n: &Node, unit: &Origin) -> UnitSettings<'a> {
        let kind = match unit.part {
            Part::Fill(_) => Some(PartKind::Fill),
            Part::Stroke(_) => Some(PartKind::Stroke),
            Part::Literal => None,
        };
        let part = kind.and_then(|k| n.extras().embroidery.as_deref()?.part(k));
        let object = part.map_or_else(PartLayer::default, |p| PartLayer::read(p, &mut self.found));
        // An expanded object's pieces have its box ([`super::derive::Expanding`]).
        UnitSettings::new(object, n.bbox_angle, self.opts, self.doc_layer)
    }

    /// What the unit that sews `part` of `n` sews.
    fn origin(&self, n: &Node, part: Part) -> Origin {
        Origin { node: n.id, part, instance: self.instance }
    }

    /// The next unit of `origin`'s selected object and part to go into the plan: which one it is,
    /// from 1 in walk order. A symbol's art placed by one instance, and an expanded object's pieces,
    /// share their object and often their part ("fill 1"): this tells them apart.
    fn occurrence(&mut self, origin: &Origin) -> usize {
        let n = self.occurrences.entry((origin.selected(), origin.part)).or_insert(0);
        *n += 1;
        *n
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

    /// Say `w` once, about the object the walk is in.
    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
        let Some(object) = self.object else { return };
        let ids = self.warned.entry(w.to_string()).or_default();
        if ids.len() < MAX_WARNED_OBJECTS && !ids.contains(&object) {
            ids.push(object);
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
        // What it leaves out is about it (or the instance placing it); a layer is no object.
        let outer_object = self.object;
        if !matches!(n.kind, NodeKind::Layer { .. }) {
            self.object = Some(self.instance.unwrap_or(n.id));
        }
        self.kind(n);
        self.instance = outer;
        self.object = outer_object;
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
            points = cut_to(points, &clip.region, &self.design.connect);
        }
        if points.is_empty() || !self.count(points.len()) {
            return;
        }
        // The file's thread record for the run's block, written back by a format that has one.
        let mut block = Block::new(Thread { record: run.thread.as_ref().map(super::file_thread), ..Thread::rgb(color) });
        if run.trim_after
            && let Some(&last) = points.last()
        {
            points.push(Stitch::new(last.x, last.y, Command::Trim));
        }
        block.stitches = points;
        let origin = self.origin(n, Part::Literal);
        let literal = self.owner.map(|g| (g, run.trim_before));
        let occurrence = self.occurrence(&origin);
        let facts = Facts { origin, occurrence, params: None, columns: None, satin_widths: None, why: Choice::Literal };
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
        // Inside another clip group, what both leave visible (kept between plans as a fill's area
        // is: the outer region is the key's shared part).
        let inner = match self.clip.is_some() {
            false => (region, rule),
            true => {
                let what = || {
                    let mut k = vec![AREA_CLIPS];
                    path_words(&mut k, &region);
                    k.push(rule_word(rule));
                    Some(k)
                };
                // The outer region first: the path the intersection returns (its contours' order
                // and starting points), not only its area, decides the stitches, and the pinned
                // exports were made with this order.
                let shaped = self.area(what, |d| match &d.clip {
                    Some(Clip { region: (outer, outer_rule), .. }) => {
                        let both = boolean(&PathData::from_bezpath(outer), *outer_rule, &PathData::from_bezpath(&region), rule, BoolOp::Intersect);
                        Shaped { area: both.to_bezpath(), rule: FillRule::NonZero, expanded_away: false }
                    }
                    None => Shaped { area: region.clone(), rule, expanded_away: false },
                });
                (shaped.area.clone(), shaped.rule)
            }
        };
        let key = self.shared(clip_words(&inner.0, inner.1));
        let bounds = inner.0.bounding_box();
        let outer = self.clip.replace(Clip { region: inner, key, bounds });
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
            let settings = self.settings(n, &unit);
            // A part its object sets not to be sewn: no unit, and nothing to say.
            if !settings.sewn() {
                continue;
            }
            let (style, why) = part_rule(item, bp, settings.forcing());
            match item {
                AppearanceItem::Fill(fill) => {
                    // Paint that isn't sewn is said so (or passed by) before any path operation
                    // runs on its area.
                    if !matches!(solid(&fill.paint), Colour::Solid(_) | Colour::Approx(_)) {
                        self.colour(Sewing { unit, settings, style, why, width: NO_WIDTH, clip_stitches: false }, &mm, rule, &fill.paint);
                        continue;
                    }
                    // A fill sewn as a running line follows its outline, as a stroke does: the
                    // clip groups around it cut its stitches, rather than its area, whose cut
                    // edge would be sewn as a line the art doesn't have.
                    if style == Style::Running {
                        self.colour(Sewing { unit, settings, style, why, width: NO_WIDTH, clip_stitches: true }, &mm, rule, &fill.paint);
                        continue;
                    }
                    let e = if style == Style::Fill { settings.expand() } else { 0.0 };
                    let what = || {
                        let mut k = vec![AREA_FILL];
                        path_words(&mut k, &mm);
                        k.extend([rule_word(rule), e.to_bits()]);
                        Some(k)
                    };
                    let shaped = self.area(what, |d| d.expanded_and_clipped(&mm, rule, e));
                    self.colour(Sewing { unit, settings, style, why, width: NO_WIDTH, clip_stitches: false }, &shaped.area, shaped.rule, &fill.paint);
                }
                AppearanceItem::Stroke(stroke) => {
                    let width = pt_to_mm(stroke.width).max(0.0);
                    // The area it paints off its path, as on the canvas, filled, its narrow parts
                    // satin columns that follow it.
                    if why == Choice::StrokeOutlined {
                        let e = settings.expand();
                        // Every setting of the stroke, serialised, goes into the key. Serialising a
                        // stroke doesn't fail; if it ever did, the stroke would be outlined again at
                        // every plan.
                        let what = || {
                            let settings = serde_json::to_vec(stroke).ok()?;
                            let mut k = vec![AREA_STROKE_PAINTED];
                            path_words(&mut k, bp);
                            k.extend([rule_word(rule), e.to_bits(), settings.len() as u64]);
                            k.extend(settings.chunks(8).map(|c| c.iter().rev().fold(0u64, |w, b| (w << 8) | u64::from(*b))));
                            Some(k)
                        };
                        let shaped = self.area(what, |d| {
                            let outline = outline_region(&PathData::from_bezpath(bp), rule, stroke).to_bezpath();
                            d.expanded_and_clipped(&in_mm(&outline), FillRule::NonZero, e)
                        });
                        let sewing =
                            Sewing { unit, settings, style: Style::Fill, why: Choice::StrokeOutlined, width: NO_WIDTH, clip_stitches: false };
                        self.colour(sewing, &shaped.area, shaped.rule, &stroke.paint);
                        continue;
                    }
                    match why {
                        // A stroke filled is the area it paints, filled: its outline at its width,
                        // caps and joins (not the inside of the path it follows).
                        Choice::StrokeFilled => {
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
                            let (e, miter) = (settings.expand(), stroke.miter_limit);
                            let what = || {
                                let mut k = vec![AREA_STROKE_FILLED];
                                path_words(&mut k, &mm);
                                k.extend([width.to_bits(), cap as u64, join as u64, miter.to_bits(), e.to_bits()]);
                                Some(k)
                            };
                            let shaped = self.area(what, |d| {
                                let outline = outline_stroke(&PathData::from_bezpath(&mm), width, cap, join, miter).to_bezpath();
                                d.expanded_and_clipped(&outline, FillRule::NonZero, e)
                            });
                            let sewing =
                                Sewing { unit, settings, style: Style::Fill, why: Choice::StrokeFilled, width: NO_WIDTH, clip_stitches: false };
                            self.colour(sewing, &shaped.area, shaped.rule, &stroke.paint);
                        }
                        // A satin column or a running line along its path.
                        _ => {
                            if style == Style::Satin && width > plan::MAX_SATIN_WIDTH {
                                let widest = plan::MAX_SATIN_WIDTH;
                                self.warn(&format!(
                                    "strokes wider than {widest} mm are sewn as satin columns {widest} mm wide: fill them instead (stitch fill)"
                                ));
                            }
                            self.colour(Sewing { unit, settings, style, why, width, clip_stitches: true }, &mm, rule, &stroke.paint);
                        }
                    }
                }
            }
        }
    }

    /// The area a unit fills. `make` runs the path operations that make it (`expand`, the clip
    /// groups around it, a stroke's outline); `what` gives its key: the path and every value those
    /// operations take (the clip groups' region is added here, as the key's shared part). The area
    /// is kept between plans by that key, so the operations don't run again for an object that
    /// hasn't changed. Without a cache (a plan made afresh), or when `what` gives no key, it is
    /// made each time. Warns when a negative `expand` left nothing of it.
    fn area(&mut self, what: impl FnOnce() -> Option<Vec<u64>>, make: impl FnOnce(&Self) -> Shaped) -> Arc<Shaped> {
        let key = self.cache.and_then(|_| what()).map(|own| UnitKey::new(own, self.clip.as_ref().map(|c| c.key.clone())));
        let kept = key.as_ref().and_then(|k| self.cache.and_then(|c| c.lock().ok()).and_then(|mut c| c.shape(k)));
        let shaped = match kept {
            Some(shaped) => shaped,
            None => {
                let shaped = Arc::new(make(self));
                // A cache that a panic elsewhere left locked is passed by.
                if let (Some(key), Some(mut c)) = (key, self.cache.and_then(|c| c.lock().ok())) {
                    c.keep_shape(key, shaped.clone());
                }
                shaped
            }
        };
        if shaped.expanded_away {
            self.warn(EXPANDED_AWAY);
        }
        shaped
    }

    /// `words` as a key part the keys of everything in a clip group share ([`UnitCache::shared`]).
    fn shared(&self, words: Vec<u64>) -> Arc<Shared> {
        match self.cache.and_then(|c| c.lock().ok()) {
            Some(mut c) => c.shared(words),
            None => Shared::of(words),
        }
    }

    /// A filled area grown by `e` ([`expanded`]), then cut by the clip groups around it
    /// ([`Digitizer::clipped`]): it grows only where it shows.
    fn expanded_and_clipped(&self, area: &BezPath, rule: FillRule, e: f64) -> Shaped {
        let (area, rule, expanded_away) = expanded(area, rule, e);
        let (area, rule) = self.clipped(&area, rule);
        Shaped { area, rule, expanded_away }
    }

    /// A filled area (millimetres) as the clip groups around it leave it: its intersection with
    /// the clipping path, cut exactly before it is planned; the area itself outside any.
    fn clipped(&self, area: &BezPath, rule: FillRule) -> (BezPath, FillRule) {
        match &self.clip {
            Some(Clip { region: (clip, clip_rule), .. }) => {
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
        let Sewing { unit, settings, style, why, width, clip_stitches } = sewing;
        let params = settings.params(style, width);
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
        if let (Some(r), Some(clip)) = (estimate, &self.clip) {
            estimate = Some(r.intersect(clip.bounds));
        }
        // Where its object sets it to start, else where the needle is.
        let from = settings.entry().or_else(|| self.seq.needle(self.seq.target(color, estimate)));
        let request = UnitRequest {
            shape: bp,
            rule,
            params,
            // A fill's narrow parts are satin columns, unless the `stitch` option forces tatami
            // (on a fill, or on a stroke filled).
            columns: (style == Style::Fill && settings.forced() != Some(Style::Fill)).then(|| settings.params(Style::Satin, NO_WIDTH)),
            from,
            clip: if clip_stitches { self.clip.as_ref().map(|c| &c.region) } else { None },
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
        let occurrence = self.occurrence(&unit);
        let facts = Facts { origin: unit, occurrence, params: Some(params), columns, satin_widths, why };
        self.seq.add(Planned { facts, block, literal: None });
    }

    /// `request`'s stitches: those the cache keeps for it, else planned, and kept.
    fn planned(&self, request: &UnitRequest<'_>) -> Result<Arc<UnitStitches>, PlanError> {
        let Some(cache) = self.cache else { return plan_unit(request).map(Arc::new) };
        let key = UnitKey::of(request, self.clip.as_ref().filter(|_| request.clip.is_some()).map(|c| &c.key));
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

/// A filled area (millimetres) grown all round by `e` millimetres, the unit's `expand` (shrunk
/// when it is negative), so that neighbouring fills meet without a gap: its union with a band
/// twice as wide along its outlines, round at the corners, or what is left of it without that
/// band. A subpath that encloses no area (sewn as a line) is left as it is. Unchanged when `e`
/// is 0. Also whether it left nothing (a negative `expand`).
fn expanded(area: &BezPath, rule: FillRule, e: f64) -> (BezPath, FillRule, bool) {
    if e.abs() < SAME_POINT {
        return (area.clone(), rule, false);
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
        return (area.clone(), rule, false);
    }
    // The region the shape covers, by its rule: overlapping or crossing outlines resolved into
    // its edge, so the band runs along that edge only (a band along an outline inside the
    // region would cut a groove through it).
    let region = normalize(&PathData::from_bezpath(&shapes), rule);
    let band = outline_stroke(&region, 2.0 * e.abs(), Cap::Round, Join::Round, ROUND_JOINS);
    let op = if e > 0.0 { BoolOp::Union } else { BoolOp::Difference };
    let mut out = boolean(&region, FillRule::NonZero, &band, FillRule::NonZero, op).to_bezpath();
    let away = out.elements().is_empty() && lines.elements().is_empty();
    out.extend(lines);
    (out, FillRule::NonZero, away)
}

/// How part `item` of an object whose path is `bp` (document points) is sewn, and the rule that
/// chose it, `forced` being the style its settings force and the setting that forces it (`None`:
/// the rule): the one place this is decided, for the walk and for `embroidery.info`.
///
/// - A fill is filled, unless a style is forced. A satin needs a width, which a fill doesn't have:
///   forced on a fill, it is filled, with a fill's settings, `expand` and narrow parts, as without.
/// - A stroke that paints an area its path doesn't run down the middle of (dashed, aligned inside
///   or outside, of varying width, with arrowheads) is that area, filled, unless a style other than
///   tatami is forced. Forced tatami, a stroke is the area it paints at its width, filled. Else it is
///   a satin column when wide enough to have two rails, a running line when not, unless forced.
pub(crate) fn part_rule(item: &AppearanceItem, bp: &BezPath, forced: Option<(Style, ForcedBy)>) -> (Style, Choice) {
    match item {
        AppearanceItem::Fill(_) => match forced {
            Some((Style::Satin, by)) => (Style::Fill, Choice::SatinFilled(by)),
            Some((style, by)) => (style, Choice::Forced(by)),
            None => (Style::Fill, Choice::Fill),
        },
        AppearanceItem::Stroke(stroke) if paints_off_its_path(stroke, bp) && matches!(forced, None | Some((Style::Fill, _))) => {
            (Style::Fill, Choice::StrokeOutlined)
        }
        AppearanceItem::Stroke(stroke) => match forced {
            Some((Style::Fill, _)) => (Style::Fill, Choice::StrokeFilled),
            Some((style, by)) => (style, Choice::Forced(by)),
            None if pt_to_mm(stroke.width).max(0.0) >= SATIN_MIN_WIDTH => (Style::Satin, Choice::WideStroke),
            None => (Style::Running, Choice::NarrowStroke),
        },
    }
}

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

/// Whether an appearance item is painted at all (not `none`): only painted items are sewn, and
/// counted as the plan's progress.
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
