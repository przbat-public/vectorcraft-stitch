//! Embroidery files: `document.open` reads a design's stitches as paths, `document.export`
//! stitches the document's art and writes a machine file (DST, EXP, PES or JEF).
//!
//! Reading draws each colour block as a group of thin lines in the thread's colour, so the design
//! looks like it will sew, and can be measured, traced over or recoloured. Writing takes every
//! visible filled or stroked path and plans its stitches: fills with a tatami fill, strokes with
//! a satin column when they are wide enough (1 mm) and a running line when not, `stitchStyle`
//! overrides that. Type, images and live art are not stitched (their outlines would be needed);
//! each is listed in the export's warnings. Blocks of the same thread are sewn together, so the
//! machine changes colour as few times as the artwork allows.
//!
//! Options: `stitchStyle` (`auto`, `fill`, `satin`, `running`), `density` (mm between rows or rail
//! crossings), `stitchLength` (mm), `stitchAngle` (degrees), `pullCompensation` (mm, satin),
//! `underlay`, `jumpLength` (mm before the thread is cut) and `minStitch` (mm).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::embroidery::{Embroidery, StitchRun};
use vectorcraft_doc::{Appearance, AppearanceItem, Document, LayerColor, LineCap, LineJoin, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, PathEl, Point, Rect, Shape, Size, kurbo};
use vectorcraft_pathops::{BoolOp, Cap, Join, boolean, outline_stroke};
use vectorcraft_stitch::format::Format as StitchFormat;
use vectorcraft_stitch::plan::{self, Params, Style};
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread, mm_to_pt, pt_to_mm};

use super::super::*;
use super::{Encoded, Format, FormatOption, err};

const C: &str = "document.export";

const STYLE: FormatOption = FormatOption {
    name: "stitchStyle",
    ty: "string",
    default: "\"auto\"",
    description: "how the art is stitched: auto (fills filled, wide strokes satin, narrow ones running), fill, satin or running",
};
// Defaults are the stitch planner's (`plan::Params::for_style`): an option left out keeps the
// style's own value. Those that every style that uses them shares are given as JSON defaults
// (checked against the planner by `defaults_are_the_planners`); the stitch length differs by
// style, so its default is `null` and its description says which.
const DENSITY: FormatOption = FormatOption {
    name: "density",
    ty: "number",
    default: "0.4",
    description: "millimetres between neighbouring threads: rows of a fill, or the crossings of a satin column (left out, a satin column picks it by its width: 0.5 under 2 mm, 0.45 under 4, 0.4 under 6, 0.37 wider)",
};
const STITCH_LENGTH: FormatOption = FormatOption {
    name: "stitchLength",
    ty: "number",
    default: "null",
    description: "the longest stitch of a running line or a fill row, millimetres (default: 4 in fills, 2.5 in running lines)",
};
const ANGLE: FormatOption =
    FormatOption { name: "stitchAngle", ty: "number", default: "45", description: "the direction a fill's rows run, degrees (0: left to right)" };
const PULL: FormatOption = FormatOption {
    name: "pullCompensation",
    ty: "number",
    default: "0.2",
    description: "how much wider satin columns are stitched so the fabric's pull leaves no gap, millimetres",
};
const UNDERLAY: FormatOption = FormatOption {
    name: "underlay",
    ty: "boolean",
    default: "true",
    description: "sew an underlay first: an edge run under fills, a centre run under satin columns",
};
const JUMP: FormatOption =
    FormatOption { name: "jumpLength", ty: "number", default: "5", description: "a move longer than this cuts the thread and jumps, millimetres" };
const MIN_STITCH: FormatOption =
    FormatOption { name: "minStitch", ty: "number", default: "0.4", description: "shorter stitches are left out, millimetres" };

/// The options the embroidery writers read.
pub const OPTIONS: &[FormatOption] = &[STYLE, DENSITY, STITCH_LENGTH, ANGLE, PULL, UNDERLAY, JUMP, MIN_STITCH];

const TEXT_LEFT_OUT: &str = "type is not stitched: outline it (Type › Create Outlines) and export again";
const IMAGES_LEFT_OUT: &str = "images are not stitched: trace them (Object › Image Trace) first";
const LIVE_LEFT_OUT: &str = "live blends, envelopes, meshes and repeats are not stitched: expand them first";
const GRADIENT_LEFT_OUT: &str = "gradients and patterns are painted in the colour of their first stop; a design sews one thread per block";
const THREAD_WIDTH: f64 = 0.4;
/// The most stitches a design may have: far beyond any hoop (large designs run to a few hundred
/// thousand), and a bound on the memory an export can take.
const MAX_DESIGN_STITCHES: usize = 2_000_000;

/// The export options, as they come: each one left out keeps the style's own value.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    stitch_style: String,
    density: Option<f64>,
    stitch_length: Option<f64>,
    stitch_angle: Option<f64>,
    pull_compensation: Option<f64>,
    underlay: Option<bool>,
    jump_length: Option<f64>,
    min_stitch: Option<f64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stitch_style: "auto".into(),
            density: None,
            stitch_length: None,
            stitch_angle: None,
            pull_compensation: None,
            underlay: None,
            jump_length: None,
            min_stitch: None,
        }
    }
}

impl Options {
    fn params(&self, style: Style, width: f64) -> Params {
        let d = Params::for_style(style);
        Params {
            style,
            density: self.density.unwrap_or(d.density),
            // A density given is used as it is; left out, a satin picks it by its width.
            auto_spacing: d.auto_spacing && self.density.is_none(),
            stitch_length: self.stitch_length.unwrap_or(d.stitch_length),
            angle: self.stitch_angle.unwrap_or(d.angle),
            pull: self.pull_compensation.unwrap_or(d.pull),
            underlay: self.underlay.unwrap_or(d.underlay),
            jump: self.jump_length.unwrap_or(d.jump),
            min_stitch: self.min_stitch.unwrap_or(d.min_stitch),
            width,
            ..d
        }
        .sane()
    }
}

/// The options in `p`, checked.
fn options(p: &Value) -> Result<Options> {
    let o = Options::deserialize(p).map_err(|e| bad(C, format!("embroidery options: {e}")))?;
    if Style::from_id(&o.stitch_style).is_none() && o.stitch_style != "auto" {
        return Err(bad(C, format!("stitchStyle `{}`: auto, fill, satin or running", o.stitch_style)));
    }
    Ok(o)
}

/// Read a design into a document: one group per thread, each run of stitches a thin line.
pub(super) fn import(bytes: &[u8], f: &Format) -> Result<(Document, Vec<String>)> {
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad("document.open", format!("{} is not an embroidery format", f.label)))?;
    let pattern = format.read(bytes).map_err(err)?;
    let b = pattern.bounds().ok_or_else(|| err("the file has no stitches"))?;
    const MARGIN: f64 = 5.0;
    let scale = mm_to_pt(1.0);
    let width = (b.width() + 2.0 * MARGIN) * scale;
    let height = (b.height() + 2.0 * MARGIN) * scale;
    let mut doc = Document::new(width, height);
    if !pattern.name.is_empty() {
        doc.title = pattern.name.clone();
    }
    let mut layer = Node::layer(doc.alloc_id(), "Embroidery", LayerColor::Preset(0));
    let mut groups: Vec<Arc<Node>> = Vec::new();
    for block in &pattern.blocks {
        let color = Color::rgb8(block.thread.color[0], block.thread.color[1], block.thread.color[2]);
        let mut stroke = Appearance::basic(Paint::None, Paint::solid(color), THREAD_WIDTH * scale);
        if let Some(AppearanceItem::Stroke(s)) = stroke.items.last_mut() {
            s.cap = LineCap::Round;
        }
        let mut children: Vec<Arc<Node>> = Vec::new();
        for (run, trim_before) in runs(block) {
            let mut bp = BezPath::new();
            for (i, s) in run.iter().enumerate() {
                let p = Point::new((s.x + MARGIN) * scale, (s.y + MARGIN) * scale);
                if i == 0 {
                    bp.move_to(p);
                } else {
                    bp.line_to(p);
                }
            }
            // A single stitch is a line that goes nowhere: still one needle point.
            if let [only] = run.as_slice() {
                bp.line_to(Point::new((only.x + MARGIN) * scale, (only.y + MARGIN) * scale));
            }
            let path = PathData::from_bezpath(&bp);
            let mut node = Node::path(doc.alloc_id(), path, stroke.clone());
            // The line's points are the file's needle points: exported, they are sewn as they are,
            // so the design is written back unchanged.
            node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(StitchRun { trim_before }) }));
            children.push(Arc::new(node));
        }
        if children.is_empty() {
            continue;
        }
        let mut group = Node::group(doc.alloc_id(), children);
        group.name = Some(if block.thread.name.is_empty() { "Thread".to_string() } else { block.thread.name.clone() });
        groups.push(Arc::new(group));
    }
    if groups.is_empty() {
        return Err(err("the design has no stitches to draw"));
    }
    if let Some(children) = layer.children_mut() {
        *children = groups;
    }
    doc.layers = vec![Arc::new(layer)];
    if let Some(a) = doc.artboards.first_mut() {
        a.rect = Rect::from_origin_size(Point::ZERO, Size::new(width, height));
    }
    doc.fix_next_id();
    Ok((doc, Vec::new()))
}

/// The stitches of one block cut into runs at every lift of the needle (a jump, a trim, a stop),
/// each with whether the thread was cut before it.
fn runs(block: &Block) -> Vec<(Vec<Stitch>, bool)> {
    let mut out: Vec<(Vec<Stitch>, bool)> = Vec::new();
    let mut cur: Vec<Stitch> = Vec::new();
    let mut trim = false;
    for s in &block.stitches {
        if s.command == Command::Stitch {
            cur.push(*s);
            continue;
        }
        if !cur.is_empty() {
            out.push((std::mem::take(&mut cur), std::mem::take(&mut trim)));
        }
        if s.command == Command::Trim {
            trim = true;
        }
    }
    if !cur.is_empty() {
        out.push((cur, trim));
    }
    out
}

/// Write a machine file from the document's visible art.
pub(super) fn encode(doc: &Document, f: &Format, p: &Value) -> Result<Encoded> {
    let opts = options(p)?;
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad(C, format!("{} is not an embroidery format", f.label)))?;
    let mut d = Digitizer { doc, opts: &opts, blocks: Vec::new(), warnings: Vec::new(), nest: 0, clip: None, owner: None, stitches: 0, failed: None };
    d.run();
    if let Some(e) = d.failed {
        return Err(bad(C, e));
    }
    if d.blocks.is_empty() {
        return Err(bad(C, "nothing to stitch: the document has no visible filled or stroked paths"));
    }
    let blocks = group(d.blocks);
    let pattern = Pattern { name: doc.title.clone(), blocks };
    let bytes = format.write(&pattern).map_err(|e| bad(C, e.to_string()))?;
    Ok(Encoded { warnings: d.warnings, ..Encoded::one(bytes) })
}

/// The document walk that plans each object's stitches.
struct Digitizer<'a> {
    doc: &'a Document,
    opts: &'a Options,
    blocks: Vec<Planned>,
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
        kurbo::flatten(mm.iter(), 0.05, |el| {
            if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
                points.push(Stitch::sew(p));
            }
        });
        // A single stitch is stored as a line that goes nowhere: one needle point.
        if let [a, b] = points.as_slice()
            && a.distance(*b) < 1e-9
        {
            points.pop();
        }
        if let Some(clip) = &self.clip {
            points = cut_to(points, clip);
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
        block.stitches = points;
        self.blocks.push(Planned { block, literal: self.owner.map(|g| (g, run.trim_before)) });
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
                    let (area, area_rule) = self.clipped(&mm, rule);
                    self.colour(&area, area_rule, style, &fill.paint, 0.0, false);
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
                            let (area, area_rule) = self.clipped(&outline, FillRule::NonZero);
                            self.colour(&area, area_rule, Style::Fill, &stroke.paint, 0.0, false);
                        }
                        // A satin column when it is wide enough to have two rails, a running line
                        // when not, unless `stitchStyle` says which.
                        forced => {
                            let style = forced.unwrap_or(if width >= 1.0 { Style::Satin } else { Style::Running });
                            self.colour(&mm, rule, style, &stroke.paint, width, true);
                        }
                    }
                }
            }
        }
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
        let mut stitches = match plan::try_stitches(bp, rule, &params) {
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
            stitches = cut_to(stitches, clip);
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
        self.blocks.push(Planned { block, literal: None });
    }
}

/// The stitches inside `region`: a run that leaves it is cut there (a trim) and taken up again (a
/// jump) where it comes back in.
fn cut_to(stitches: Vec<Stitch>, region: &(BezPath, FillRule)) -> Vec<Stitch> {
    let (path, rule) = region;
    let inside = |s: &Stitch| {
        let w = path.winding(s.point());
        match rule {
            FillRule::NonZero => w != 0,
            FillRule::EvenOdd => w % 2 != 0,
        }
    };
    let mut out: Vec<Stitch> = Vec::with_capacity(stitches.len());
    let mut lifted = false;
    for s in stitches {
        if s.command != Command::Stitch {
            // The plan's own trims and jumps: the needle is up until the next stitch kept.
            lifted = true;
            continue;
        }
        if !inside(&s) {
            lifted = true;
            continue;
        }
        if lifted && let Some(last) = out.last().copied() {
            out.push(Stitch::new(last.x, last.y, Command::Trim));
            out.push(Stitch::jump(s.point()));
        }
        lifted = false;
        out.push(s);
    }
    out
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

/// One object's stitches, in document order.
struct Planned {
    block: Block,
    /// A literal run read from a machine file: the group it belongs to (that file's colour block)
    /// and whether the thread was cut before it. `None`: stitches planned from art.
    literal: Option<(NodeId, bool)>,
}

/// Blocks of one thread sewn in one pass where the stacking allows it: a block joins an earlier
/// block of its thread only when it overlaps none of the blocks sewn between the two, so art that
/// lies on top is still sewn last (thread sewn later covers thread sewn earlier, as paint does).
/// The machine changes colour as few times as that allows, not once per object.
///
/// Literal runs (a design read from a file) are left as the file had them: the runs of one group
/// make one block, joined by the jumps (and trims) between them, and never merge with anything
/// else.
fn group(planned: Vec<Planned>) -> Vec<Block> {
    /// How far apart two blocks' stitches must be not to touch: a thread's width and a satin's pull.
    const CLEARANCE: f64 = 1.0;
    // Each block so far, its bounds, and the group of the literal runs it holds.
    let mut out: Vec<(Block, Option<Rect>, Option<NodeId>)> = Vec::new();
    for Planned { block: b, literal } in planned {
        let Some(&first) = b.stitches.first() else { continue };
        let bounds = b.bounds().map(|r| r.inflate(CLEARANCE, CLEARANCE));
        if let Some((owner, trim_before)) = literal {
            match out.last_mut() {
                Some((merged, mb, Some(o))) if *o == owner && merged.thread.color == b.thread.color => {
                    if let Some(last) = merged.stitches.last().copied()
                        && trim_before
                    {
                        merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    }
                    merged.stitches.push(Stitch::jump(first.point()));
                    merged.stitches.extend(b.stitches.iter().copied());
                    *mb = union(*mb, bounds);
                }
                _ => out.push((b, bounds, Some(owner))),
            }
            continue;
        }
        let target = out.iter().rposition(|(m, _, lit)| lit.is_none() && m.thread.color == b.thread.color).filter(|&j| {
            out.iter().skip(j + 1).all(|(_, other, _)| match (bounds, other) {
                (Some(a), Some(o)) => !overlaps(a, *o),
                _ => false,
            })
        });
        match target.and_then(|j| out.get_mut(j)) {
            Some((merged, mb, _)) => {
                if let Some(last) = merged.stitches.last().copied() {
                    merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                    merged.stitches.push(Stitch::jump(first.point()));
                }
                merged.stitches.extend(b.stitches.iter().copied());
                *mb = union(*mb, bounds);
            }
            None => out.push((b, bounds, None)),
        }
    }
    out.into_iter().map(|(b, ..)| b).collect()
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.union(y)),
        (x, y) => x.or(y),
    }
}

/// Whether two boxes share any area (touching edges do not count).
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults `document.formats` lists are the planner's, for every style that uses them.
    #[test]
    fn defaults_are_the_planners() {
        let num = |o: &FormatOption| o.default.parse::<f64>().unwrap();
        let (fill, satin, running) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin), Params::for_style(Style::Running));
        assert_eq!(num(&DENSITY), fill.density);
        assert_eq!(num(&DENSITY), satin.density);
        // The satin spacing by width the description gives is the planner's.
        for (w, spacing) in [(1.0, "0.5 under 2"), (3.0, "0.45 under 4"), (5.0, "0.4 under 6"), (9.0, "0.37 wider")] {
            assert!(DENSITY.description.contains(spacing) && spacing.starts_with(&plan::satin_spacing(w).to_string()), "{spacing}");
        }
        // A density given turns the satin's auto spacing off.
        let given = Options { density: Some(0.3), ..Options::default() };
        assert!(Options::default().params(Style::Satin, 3.0).auto_spacing);
        assert!(!given.params(Style::Satin, 3.0).auto_spacing);
        assert_eq!(num(&ANGLE), fill.angle);
        assert_eq!(num(&PULL), satin.pull);
        assert_eq!(UNDERLAY.default == "true", fill.underlay && satin.underlay);
        for p in [fill, satin, running] {
            assert_eq!(num(&JUMP), p.jump);
            assert_eq!(num(&MIN_STITCH), p.min_stitch);
        }
        assert!(STITCH_LENGTH.description.contains(&format!("{} in fills", fill.stitch_length)));
        assert!(STITCH_LENGTH.description.contains(&format!("{} in running lines", running.stitch_length)));
        // Nothing given: each style plans with its own values.
        let o = Options::default();
        assert_eq!(o.params(Style::Fill, 0.0), fill.sane());
        assert_eq!(o.params(Style::Running, 0.0), running.sane());
    }
}
