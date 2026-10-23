//! Opening a machine file: the design's stitches as art, one group per thread and a line per
//! run of stitches, each line marked as literal stitches so that it is written back unchanged.

use std::sync::Arc;

use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::embroidery::{Embroidery, EmbroiderySetup, StitchRun, ThreadPalette};
use vectorcraft_doc::{Appearance, AppearanceItem, Document, LayerColor, LineCap, LineJoin, Node, Shared};
use vectorcraft_geom::{BezPath, PathData, Point, Rect, Size};
use vectorcraft_stitch::format::{Format as StitchFormat, Loss};
use vectorcraft_stitch::plan::THREAD_WIDTH;
use vectorcraft_stitch::{Block, Command, Stitch, mm_to_pt};

use super::super::super::*;
use super::super::{Format, err};

/// The margin round an opened design on its new artboard, millimetres: a little room to see its
/// edges. It doesn't move the stitches as written: the file's start point moves with them, and
/// every writer measures a design from that or from its own bounds, so it is gone again on export.
const MARGIN: f64 = 5.0;
/// The most runs of stitches (each stitched without lifting the needle) a design opens as, one
/// object each: far more than real designs have (a few thousand), a bound on the objects a hostile
/// file can make (one for every few bytes of a stream of stitches and jumps).
const MAX_RUNS: usize = 100_000;

/// What reading a machine file left out, as opening it says it, one note each (in this order):
/// a file written from the document lacks it, so writing over the file asks first
/// ([`super::is_loss`]). The last is said for what a newer stitch crate reports and this list
/// doesn't name yet.
pub const LOSS_NOTES: [&str; 8] = [
    "the PES file's own section, which PE-Design edits, isn't kept: a PES written from this document holds its stitches alone",
    "the file's preview pictures aren't kept: a PES or PEC written from this document has blank ones",
    "thread numbers that name no thread of the palette (PEC's 62 to 64 mark appliqué) are written as the nearest thread",
    "the JEF header's date and version, and a hoop larger than the design needs, aren't kept",
    "the DST header's end position and multi-volume fields (AX, AY, MX, MY, PD) aren't kept",
    "threads that twist several colours are read in their first colour",
    "the file's several designs are read as one",
    "the file holds things this version doesn't read",
];

/// The note for `loss` ([`LOSS_NOTES`]).
fn loss_note(loss: Loss) -> &'static str {
    let k = match loss {
        Loss::PesSection => 0,
        Loss::Previews => 1,
        Loss::NotThreads => 2,
        Loss::JefHeader => 3,
        Loss::DstFields => 4,
        Loss::TwistedThreads => 5,
        Loss::Designs => 6,
        _ => 7,
    };
    LOSS_NOTES.get(k).copied().unwrap_or_default()
}

/// Read a design into a document: one group per thread, each run of stitches a thin line. The
/// notes say what reading it left out ([`LOSS_NOTES`]).
pub(in super::super) fn import(bytes: &[u8], f: &Format) -> Result<(Document, Vec<String>)> {
    let format = StitchFormat::from_id(f.id).ok_or_else(|| bad("document.open", format!("{} is not an embroidery format", f.label)))?;
    let pattern = format.read(bytes).map_err(err)?;
    let b = pattern.bounds().ok_or_else(|| err("the file has no stitches"))?;
    // Points per millimetre.
    let scale = mm_to_pt(1.0);
    let width = (b.width() + 2.0 * MARGIN) * scale;
    let height = (b.height() + 2.0 * MARGIN) * scale;
    // A design larger than a document holds would open, and then every edit would be refused.
    if !(width.is_finite() && height.is_finite()) || width > vectorcraft_doc::MAX_COORD || height > vectorcraft_doc::MAX_COORD {
        return Err(err(format!("the design is {:.0} × {:.0} mm, larger than a document holds", b.width(), b.height())));
    }
    let runs_in = |block: &Block| runs(block).len();
    if pattern.blocks.iter().map(runs_in).sum::<usize>() > MAX_RUNS {
        return Err(err(format!("the design has more than {MAX_RUNS} separate runs of stitches, more than a document opens")));
    }
    let mut doc = Document::new(width, height);
    // A design from a format that numbers its threads is in that format's palette: its colours
    // are matched to the threads the file names (`embroidery.threads`), not to another maker's.
    // The file's start point and its design's name (the document is named after the file) are
    // kept, for the export to write again.
    let palette = format.palette().and_then(|p| ThreadPalette::from_id(p.id()));
    let start = pattern.start.map(|s| Point::new((s.x + MARGIN) * scale, (s.y + MARGIN) * scale)).filter(|s| s.x.is_finite() && s.y.is_finite());
    let label = Some(pattern.name.clone()).filter(|n| !n.is_empty());
    let setup = EmbroiderySetup { palette, start, label, ..EmbroiderySetup::default() };
    if !setup.is_empty() {
        doc.embroidery = Some(Box::new(setup));
    }
    let mut layer = Node::layer(doc.alloc_id(), "Embroidery", LayerColor::Preset(0));
    let mut groups: Vec<Arc<Node>> = Vec::new();
    for block in &pattern.blocks {
        let color = Color::rgb8(block.thread.color[0], block.thread.color[1], block.thread.color[2]);
        // As wide as 40 wt thread lies: an opened file records no weight.
        let mut stroke = Appearance::basic(Paint::None, Paint::solid(color), THREAD_WIDTH * scale);
        // Round at the ends and the turns, as thread lies: a sharp turn mitred would draw a spike
        // far past the needle point (a satin's zigzag turns at every penetration).
        if let Some(AppearanceItem::Stroke(s)) = stroke.items.last_mut() {
            s.cap = LineCap::Round;
            s.join = LineJoin::Round;
        }
        // One appearance shared by the block's lines, not a copy each.
        let stroke = Shared::new(stroke);
        // The file's thread record, kept on each run for the export to write back.
        let record = block.thread.record.as_ref().map(crate::cmd::embroidery::kept_thread);
        let mut children: Vec<Arc<Node>> = Vec::new();
        for (run, trim_before, trim_after) in runs(block) {
            let mut bp = BezPath::new();
            for (i, s) in run.iter().enumerate() {
                let p = Point::new((s.x + MARGIN) * scale, (s.y + MARGIN) * scale);
                if i == 0 {
                    bp.move_to(p);
                } else {
                    bp.line_to(p);
                }
            }
            // A single stitch is a line that goes nowhere: still one needle point. Two at one spot
            // draw the same line, and say so.
            if let [only] = run.as_slice() {
                bp.line_to(Point::new((only.x + MARGIN) * scale, (only.y + MARGIN) * scale));
            }
            let double_point = matches!(run.as_slice(), [a, b] if a.distance(*b) < vectorcraft_stitch::SAME_POINT);
            let path = PathData::from_bezpath(&bp);
            let mut node = Node::path(doc.alloc_id(), path, Appearance::default());
            node.appearance = stroke.clone();
            // The line's points are the file's needle points: exported, they are sewn as they are,
            // so the design is written back unchanged.
            node.extras_mut().embroidery = Some(Box::new(Embroidery {
                stitches: Some(StitchRun { trim_before, trim_after, double_point, thread: record.clone(), ..StitchRun::default() }),
                ..Embroidery::default()
            }));
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
    let mut notes: Vec<String> = Vec::new();
    for note in format.losses(bytes).into_iter().map(loss_note) {
        if !notes.iter().any(|n| n == note) {
            notes.push(note.to_string());
        }
    }
    Ok((doc, notes))
}

/// The stitches of one block cut into runs at every lift of the needle (a jump, a trim, a stop),
/// each with whether the thread was cut before it and, for the block's last run, after it.
fn runs(block: &Block) -> Vec<(Vec<Stitch>, bool, bool)> {
    let mut out: Vec<(Vec<Stitch>, bool, bool)> = Vec::new();
    let mut cur: Vec<Stitch> = Vec::new();
    let mut trim = false;
    for s in &block.stitches {
        if s.command == Command::Stitch {
            cur.push(*s);
            continue;
        }
        if !cur.is_empty() {
            out.push((std::mem::take(&mut cur), std::mem::take(&mut trim), false));
        }
        if s.command == Command::Trim {
            trim = true;
        }
    }
    if !cur.is_empty() {
        out.push((cur, trim, false));
    } else if trim && let Some(last) = out.last_mut() {
        // A cut after the block's last stitches (before the thread is changed).
        last.2 = true;
    }
    out
}
