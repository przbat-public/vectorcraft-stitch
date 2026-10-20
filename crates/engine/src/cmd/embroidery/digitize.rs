//! The document walk that plans each visible object's stitches: what is sewn (what would
//! print), clip groups, the colour of each block, literal stitches read from files, and the limit
//! on a design's size.

use std::sync::Arc;

use vectorcraft_color::Paint;
use vectorcraft_doc::embroidery::StitchRun;
use vectorcraft_doc::{AppearanceItem, Document, LineCap, LineJoin, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, PathEl, Shape, kurbo};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, normalize, outline_stroke};
use vectorcraft_stitch::plan::{self, Params, Style};
use vectorcraft_stitch::{Block, Command, SAME_POINT, Stitch, Thread, pt_to_mm};

use super::Options;
use super::sequence::{CLEARANCE, Planned, Sequence};

const TEXT_LEFT_OUT: &str = "type is not stitched: outline it (Type › Create Outlines) and export again";
const IMAGES_LEFT_OUT: &str = "images are not stitched: trace them (Object › Image Trace) first";
const LIVE_LEFT_OUT: &str = "live blends, envelopes, meshes and repeats are not stitched: expand them first";
const EXPANDED_AWAY: &str = "a negative expand left nothing of some filled shapes, so they are not stitched: shrink them less";
const GRADIENT_LEFT_OUT: &str = "gradients and patterns are painted in the colour of their first stop; a design sews one thread per block";
/// A stroke at least this wide (millimetres) is sewn as a satin column, a narrower one as a running
/// line (unless `stitchStyle` says otherwise): below about a millimetre the two rails are too
/// close for the crossings to lie side by side. An estimate.
const SATIN_MIN_WIDTH: f64 = 1.0;
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
/// The miter limit given for the band that grows a fill ([`Digitizer::expanded`]): its joins are
/// round, which have none; this is a stroke's usual default.
const ROUND_JOINS: f64 = 4.0;

/// How much each narrow column's outline grows before it is cut out of its shape (millimetres): a
/// hair, so that it covers its own sides, which lie on the shape's outline, and leaves no sliver of
/// the shape there. Far below a thread's width.
const COLUMN_GROWTH: f64 = 0.05;

/// `area` without its narrow `columns` ([`plan::narrow_columns`]): what is left to fill, each
/// column's outline grown by [`COLUMN_GROWTH`] and cut out; and the rest and the grown columns
/// together, the area the needle travels in. (The rest's edges along the outline come from the
/// path operations, a hair off the outline as the planner flattens it: travelling in the rest
/// and the columns as the same operations give them, every row's end lies on or inside it.)
fn rest_of(area: &BezPath, rule: FillRule, columns: &[plan::Column]) -> (BezPath, BezPath) {
    let mut cut: Option<PathData> = None;
    for c in columns {
        let outline = PathData::from_bezpath(&c.outline());
        let band = outline_stroke(&outline, 2.0 * COLUMN_GROWTH, Cap::Round, Join::Round, ROUND_JOINS);
        let grown = boolean(&outline, FillRule::NonZero, &band, FillRule::NonZero, BoolOp::Union);
        cut = Some(match cut {
            Some(all) => boolean(&all, FillRule::NonZero, &grown, FillRule::NonZero, BoolOp::Union),
            None => grown,
        });
    }
    match cut {
        Some(cut) => {
            let rest = boolean(&PathData::from_bezpath(area), rule, &cut, FillRule::NonZero, BoolOp::Difference);
            let whole = boolean(&rest, FillRule::NonZero, &cut, FillRule::NonZero, BoolOp::Union);
            (rest.to_bezpath(), whole.to_bezpath())
        }
        None => (area.clone(), area.clone()),
    }
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

/// The stitches of `doc`'s visible art with `opts`: its blocks in sewing order ([`Sequence`]), and
/// the warnings for what was left out or approximated; an error when the design would need too
/// many stitches.
pub(crate) fn digitize(doc: &Document, opts: &Options) -> Result<(Vec<Block>, Vec<String>), String> {
    let seq = Sequence::new(opts.params(Style::Running, NO_WIDTH));
    let mut d = Digitizer { doc, opts, seq, warnings: Vec::new(), nest: 0, clip: None, owner: None, stitches: 0, failed: None };
    d.run();
    match d.failed {
        Some(e) => Err(e),
        None => Ok((d.seq.blocks(), d.warnings)),
    }
}

/// The document walk that plans each object's stitches.
struct Digitizer<'a> {
    doc: &'a Document,
    opts: &'a Options,
    /// The blocks planned so far, in sewing order.
    seq: Sequence,
    warnings: Vec<String>,
    /// Symbol art being stitched inside symbol instances.
    nest: u32,
    /// The region the clip groups around the current object leave visible (millimetres), `None`
    /// outside any clip group.
    clip: Option<(BezPath, FillRule)>,
    /// The group the walk is in: literal stitch runs ([`StitchRun`]) of one group are one colour
    /// block of the file they came from, and stay one.
    owner: Option<NodeId>,
    /// The stitches planned so far, against [`MAX_DESIGN_STITCHES`].
    stitches: usize,
    /// Why the design can't be stitched (too many stitches); the walk stops planning once set.
    failed: Option<String>,
}

impl Digitizer<'_> {
    fn run(&mut self) {
        for layer in &self.doc.layers {
            self.node(layer);
        }
    }

    fn warn(&mut self, w: &str) {
        if !self.warnings.iter().any(|x| x == w) {
            self.warnings.push(w.to_string());
        }
    }

    fn node(&mut self, n: &Node) {
        if !n.visible {
            return;
        }
        let added = self.seq.added();
        self.kind(n);
        // An object (or group) that asks for it: the thread is cut after its last stitch.
        if n.extras().embroidery.as_deref().is_some_and(|e| e.trim_after) && self.seq.added() > added {
            self.seq.trim_last();
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
                match n.extras().embroidery.as_deref().and_then(|e| e.stitches) {
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
                if self.nest >= 8 {
                    return;
                }
                let mut art = (*sym.art).clone();
                art.transform(*xf, false);
                self.nest += 1;
                self.node(&art);
                self.nest -= 1;
            }
            NodeKind::Text(_) => self.warn(TEXT_LEFT_OUT),
            NodeKind::Image(_) => self.warn(IMAGES_LEFT_OUT),
            _ => self.warn(LIVE_LEFT_OUT),
        }
    }

    /// A run of literal stitches (read from a machine file): the path's points sewn as they are,
    /// in the colour of its stroke, one block with the other runs of its group.
    fn literal(&mut self, n: &Node, bp: &BezPath, run: StitchRun) {
        if self.failed.is_some() {
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
        let mut mm = bp.clone();
        mm.apply_affine(Affine::scale(vectorcraft_stitch::MM_PER_PT));
        let mut points: Vec<Stitch> = Vec::new();
        kurbo::flatten(mm.iter(), LITERAL_FLATTEN, |el| {
            if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
                points.push(Stitch::sew(p));
            }
        });
        // A single stitch is stored as a line that goes nowhere: one needle point.
        if let [a, b] = points.as_slice()
            && a.distance(*b) < SAME_POINT
        {
            points.pop();
        }
        if let Some(clip) = &self.clip {
            points = cut_to(points, clip, &self.opts.params(Style::Running, NO_WIDTH));
        }
        if points.is_empty() {
            return;
        }
        self.stitches = self.stitches.saturating_add(points.len());
        if self.stitches > MAX_DESIGN_STITCHES {
            self.failed = Some(format!("the design would need more than {MAX_DESIGN_STITCHES} stitches"));
            return;
        }
        let mut block = Block::new(Thread::rgb(color));
        if run.trim_after
            && let Some(&last) = points.last()
        {
            points.push(Stitch::new(last.x, last.y, Command::Trim));
        }
        block.stitches = points;
        self.seq.add(Planned { block, literal: self.owner.map(|g| (g, run.trim_before)) });
    }

    /// A clip group, as it paints: the clipping path (its first child) paints its fills behind the
    /// clipped art and its strokes over it, unclipped; the rest is sewn only inside the clipping
    /// path. Nothing to clip by (an empty clipping path) hides the clipped art.
    fn clip_group(&mut self, children: &[Arc<Node>]) {
        let Some((clip, rest)) = children.split_first() else { return };
        let Some((mut region, rule)) = vectorcraft_render::effects::clip_outline(clip) else { return };
        region.apply_affine(Affine::scale(vectorcraft_stitch::MM_PER_PT));
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

    /// Stitch `bp` (document points) once per visible appearance item.
    fn shape(&mut self, n: &Node, bp: &BezPath, rule: FillRule) {
        let mut mm = bp.clone();
        mm.apply_affine(Affine::scale(vectorcraft_stitch::MM_PER_PT));
        for item in &n.appearance.items {
            if !item.visible() {
                continue;
            }
            match item {
                // A fill is filled unless `stitchStyle` asks for another way of stitching it.
                AppearanceItem::Fill(fill) => {
                    let style = self.forced().unwrap_or(Style::Fill);
                    let (area, area_rule) = if style == Style::Fill { self.expanded(&mm, rule) } else { (mm.clone(), rule) };
                    let (area, area_rule) = self.clipped(&area, area_rule);
                    self.colour(&area, area_rule, style, &fill.paint, NO_WIDTH, false);
                }
                AppearanceItem::Stroke(stroke) => {
                    let width = pt_to_mm(stroke.width).max(0.0);
                    match self.forced() {
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
                            let (area, area_rule) = self.expanded(&outline, FillRule::NonZero);
                            let (area, area_rule) = self.clipped(&area, area_rule);
                            self.colour(&area, area_rule, Style::Fill, &stroke.paint, NO_WIDTH, false);
                        }
                        // A satin column when it is wide enough to have two rails, a running line
                        // when not, unless `stitchStyle` says which.
                        forced => {
                            let style = forced.unwrap_or(if width >= SATIN_MIN_WIDTH { Style::Satin } else { Style::Running });
                            if style == Style::Satin && width > plan::MAX_SATIN_WIDTH {
                                let widest = plan::MAX_SATIN_WIDTH;
                                self.warn(&format!(
                                    "strokes wider than {widest} mm are sewn as satin columns {widest} mm wide: fill them instead (stitchStyle fill)"
                                ));
                            }
                            self.colour(&mm, rule, style, &stroke.paint, width, true);
                        }
                    }
                }
            }
        }
    }

    /// A filled area (millimetres) grown all round by the `expand` option (shrunk when it is
    /// negative), so that neighbouring fills meet without a gap: its union with a band twice as wide
    /// along its outlines, round at the corners, or what is left of it without that band. A subpath
    /// that encloses no area (sewn as a line) is left as it is. Unchanged when the option is 0. The
    /// clip groups around it cut the result ([`Digitizer::clipped`]): it grows only where it shows.
    fn expanded(&mut self, area: &BezPath, rule: FillRule) -> (BezPath, FillRule) {
        let e = self.opts.expand();
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

    /// The style `stitchStyle` forces, `None` for `auto`.
    fn forced(&self) -> Option<Style> {
        Style::from_id(&self.opts.stitch_style)
    }

    /// Plan one appearance item, or say what could not be stitched.
    /// `clip_stitches`: a stroke, whose stitches the clip groups around it cut (a fill's area was
    /// cut before planning).
    fn colour(&mut self, bp: &BezPath, rule: FillRule, style: Style, paint: &Paint, width: f64, clip_stitches: bool) {
        match solid(paint) {
            Colour::None => {}
            Colour::Solid(color) => self.plan(bp, rule, style, color, width, clip_stitches),
            Colour::Approx(color) => {
                self.warn(GRADIENT_LEFT_OUT);
                self.plan(bp, rule, style, color, width, clip_stitches);
            }
            Colour::Pattern => self.warn(GRADIENT_LEFT_OUT),
        }
    }

    fn plan(&mut self, bp: &BezPath, rule: FillRule, style: Style, color: [u8; 3], width: f64, clip_stitches: bool) {
        if self.failed.is_some() {
            return;
        }
        let params = self.opts.params(style, width);
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
        // A fill's narrow parts are satin columns, unless `stitchStyle` forces tatami.
        let columns = match (style, self.forced()) {
            (Style::Fill, None) => plan::narrow_columns(bp, rule, params.narrow),
            _ => Vec::new(),
        };
        let planned = if columns.is_empty() {
            plan::try_stitches_from(bp, rule, &params, from)
        } else {
            let satin = self.opts.params(Style::Satin, NO_WIDTH);
            let (rest, whole) = rest_of(bp, rule, &columns);
            plan::try_fill_with_columns(&whole, FillRule::NonZero, &rest, &columns, &params, &satin, from)
        };
        let mut stitches = match planned {
            Ok(s) => s,
            Err(e) => {
                self.failed = Some(e.to_string());
                return;
            }
        };
        // A stroke inside a clip group: only the stitches inside the clipping path are sewn. (A
        // filled area was cut to it exactly; its edge run lies on the cut, which a point test
        // would chop.)
        if clip_stitches && let Some(clip) = &self.clip {
            stitches = cut_to(stitches, clip, &params);
        }
        if stitches.is_empty() {
            return;
        }
        self.stitches = self.stitches.saturating_add(stitches.len());
        if self.stitches > MAX_DESIGN_STITCHES {
            self.failed =
                Some(format!("the design would need more than {MAX_DESIGN_STITCHES} stitches: use a longer stitch, wider spacing or less art"));
            return;
        }
        let mut block = Block::new(Thread::rgb(color));
        block.stitches = stitches;
        self.seq.add(Planned { block, literal: None });
    }
}

/// The stitches inside `region`: a run that leaves it is cut there and taken up again where it
/// comes back in, the needle lifted between as the planner lifts it (`p`'s jump and trim lengths,
/// the thread tied off and in around a long jump).
fn cut_to(stitches: Vec<Stitch>, region: &(BezPath, FillRule), p: &Params) -> Vec<Stitch> {
    let (path, rule) = region;
    let inside = |s: &Stitch| {
        let w = path.winding(s.point());
        match rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    // The stretches inside, each with the plan's own jumps and trims within it.
    let mut pieces: Vec<Vec<Stitch>> = vec![Vec::new()];
    for s in stitches {
        if s.command == Command::Stitch && !inside(&s) {
            if pieces.last().is_some_and(|p| !p.is_empty()) {
                pieces.push(Vec::new());
            }
            continue;
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(s);
        }
    }
    // A piece that ends on the plan's own jump or trim drops it: the joining lifts the needle.
    for piece in &mut pieces {
        while piece.last().is_some_and(|s| s.command != Command::Stitch) {
            piece.pop();
        }
    }
    plan::join_cut(pieces, p)
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
