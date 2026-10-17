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
use vectorcraft_doc::{Appearance, AppearanceItem, Document, LayerColor, LineCap, Node, NodeKind};
use vectorcraft_geom::{Affine, BezPath, FillRule, PathData, Point, Rect, Size};
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
const DENSITY: FormatOption = FormatOption {
    name: "density",
    ty: "number",
    default: "0.3",
    description: "millimetres between neighbouring threads: rows of a fill, or the crossings of a satin column",
};
const STITCH_LENGTH: FormatOption = FormatOption {
    name: "stitchLength",
    ty: "number",
    default: "3.5",
    description: "the longest stitch of a running line or a fill row, millimetres",
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
    FormatOption { name: "minStitch", ty: "number", default: "0.5", description: "shorter stitches are left out, millimetres" };

/// The options the embroidery writers read.
pub const OPTIONS: &[FormatOption] = &[STYLE, DENSITY, STITCH_LENGTH, ANGLE, PULL, UNDERLAY, JUMP, MIN_STITCH];

const TEXT_LEFT_OUT: &str = "type is not stitched: outline it (Type › Create Outlines) and export again";
const IMAGES_LEFT_OUT: &str = "images are not stitched: trace them (Object › Image Trace) first";
const LIVE_LEFT_OUT: &str = "live blends, envelopes, meshes and repeats are not stitched: expand them first";
const GRADIENT_LEFT_OUT: &str = "gradients and patterns are painted in the colour of their first stop; a design sews one thread per block";
const THREAD_WIDTH: f64 = 0.4;

/// The export options, as they come.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Options {
    stitch_style: String,
    density: f64,
    stitch_length: f64,
    stitch_angle: f64,
    pull_compensation: f64,
    underlay: bool,
    jump_length: f64,
    min_stitch: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            stitch_style: "auto".into(),
            density: 0.3,
            stitch_length: 3.5,
            stitch_angle: 45.0,
            pull_compensation: 0.2,
            underlay: true,
            jump_length: 5.0,
            min_stitch: 0.5,
        }
    }
}

impl Options {
    fn params(&self, style: Style, width: f64) -> Params {
        let mut p = Params::for_style(style);
        p.style = style;
        p.density = self.density;
        p.stitch_length = self.stitch_length;
        p.angle = self.stitch_angle;
        p.pull = self.pull_compensation;
        p.underlay = self.underlay;
        p.jump = self.jump_length;
        p.min_stitch = self.min_stitch;
        p.width = width;
        p.sane()
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
        for run in runs(block) {
            let mut bp = BezPath::new();
            for (i, s) in run.iter().enumerate() {
                let p = Point::new((s.x + MARGIN) * scale, (s.y + MARGIN) * scale);
                if i == 0 {
                    bp.move_to(p);
                } else {
                    bp.line_to(p);
                }
            }
            if bp.elements().len() < 2 {
                continue;
            }
            let path = PathData::from_bezpath(&bp);
            children.push(Arc::new(Node::path(doc.alloc_id(), path, stroke.clone())));
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

/// The stitches of one block cut into runs at every lift of the needle.
fn runs(block: &Block) -> Vec<Vec<Stitch>> {
    let mut out: Vec<Vec<Stitch>> = Vec::new();
    let mut cur: Vec<Stitch> = Vec::new();
    for s in &block.stitches {
        if s.command == Command::Stitch {
            cur.push(*s);
            continue;
        }
        if cur.len() >= 2 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Write a machine file from the document's visible art.
pub(super) fn encode(doc: &Document, f: &Format, p: &Value) -> Result<Encoded> {
    let opts = options(p)?;
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad(C, format!("{} is not an embroidery format", f.label)))?;
    let mut d = Digitizer { doc, opts: &opts, blocks: Vec::new(), warnings: Vec::new(), nest: 0 };
    d.run();
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
    blocks: Vec<Block>,
    warnings: Vec<String>,
    /// Symbol art being stitched inside symbol instances.
    nest: u32,
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
            NodeKind::Layer { children, .. } | NodeKind::Group { children, .. } => {
                for c in children {
                    self.node(c);
                }
            }
            NodeKind::Path { path, rule, guide, .. } => {
                if !*guide {
                    self.shape(n, &path.to_bezpath(), *rule);
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
                    self.colour(&mm, rule, style, &fill.paint, 0.0);
                }
                AppearanceItem::Stroke(stroke) => {
                    let width = pt_to_mm(stroke.width).max(0.0);
                    // A stroke is a satin column when it is wide enough to have two rails, a
                    // running line when not, unless `stitchStyle` says otherwise.
                    let style = self.forced().unwrap_or(if width >= 1.0 { Style::Satin } else { Style::Running });
                    self.colour(&mm, rule, style, &stroke.paint, width);
                }
            }
        }
    }

    /// The style `stitchStyle` forces, `None` for `auto`.
    fn forced(&self) -> Option<Style> {
        Style::from_id(&self.opts.stitch_style)
    }

    /// Plan one appearance item, or say what could not be stitched.
    fn colour(&mut self, bp: &BezPath, rule: FillRule, style: Style, paint: &Paint, width: f64) {
        match solid(paint) {
            Colour::None => {}
            Colour::Solid(color) => self.plan(bp, rule, style, color, width),
            Colour::Approx(color) => {
                self.warn(GRADIENT_LEFT_OUT);
                self.plan(bp, rule, style, color, width);
            }
            Colour::Pattern => self.warn(GRADIENT_LEFT_OUT),
        }
    }

    fn plan(&mut self, bp: &BezPath, rule: FillRule, style: Style, color: [u8; 3], width: f64) {
        let params = self.opts.params(style, width);
        let stitches = plan::stitches(bp, rule, &params);
        if stitches.is_empty() {
            return;
        }
        let mut block = Block::new(Thread::rgb(color));
        block.stitches = stitches;
        self.blocks.push(block);
    }
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

/// Blocks sewn in one pass per thread, in the order the threads first appear: the machine changes
/// colour once per thread, not once per object.
fn group(blocks: Vec<Block>) -> Vec<Block> {
    let mut order: Vec<[u8; 3]> = Vec::new();
    for b in &blocks {
        if !order.contains(&b.thread.color) {
            order.push(b.thread.color);
        }
    }
    let mut out: Vec<Block> = Vec::new();
    for color in order {
        let mut merged = Block::new(Thread::rgb(color));
        for b in blocks.iter().filter(|b| b.thread.color == color) {
            if let (Some(last), Some(first)) = (merged.stitches.last().copied(), b.stitches.first().copied()) {
                merged.stitches.push(Stitch::new(last.x, last.y, Command::Trim));
                merged.stitches.push(Stitch::jump(first.point()));
            }
            merged.stitches.extend(b.stitches.iter().copied());
        }
        if !merged.stitches.is_empty() {
            out.push(merged);
        }
    }
    out
}
