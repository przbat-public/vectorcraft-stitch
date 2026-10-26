//! The stitch scene ([`StitchScene`]): a design's stitch plan as the stitch preview draws it, a
//! line picture for the renderer ([`vectorcraft_render::lines`], which knows nothing about
//! stitches) and what each of its tags means.
//!
//! **Built from the plan's records**, walked in sewing order ([`super::measure::each`], as the
//! figures and the checks walk them), so that what is drawn is what is counted and exported:
//! - each stretch of needle points a thread lies between is a line in its block's colour slot,
//!   tagged with its unit, as wide as the unit's thread lies ([`plan::thread_width`]; 40 wt for a
//!   run read from a file). Whether a thread lies from the record before is decided per record
//!   ([`super::measure::Record::sewn`]): a unit that starts where the needle already is has its
//!   first stitch sewn from the unit before's last needle point, so its line starts there (a
//!   lead-in), where the unit's own runs ([`super::plan::Unit::runs`]) would leave a gap;
//! - each jump is a link from the record before, a thread lying along it unless the thread was cut
//!   since the last needle point;
//! - each cut is a × mark, each stop a square mark;
//! - every point, link and mark keeps its record number, so a range of numbers can be drawn.
//!
//! Lines never cross units (a line begins anew at a unit's first record), and the picture cuts them
//! into pieces of at most `MAX_LINE_POINTS` points. The colours are not in the scene: each block's
//! thread and art colours are kept beside it ([`SceneBlock`]), and the drawing's style picks them,
//! so a change of colours or look never builds the scene again.
//!
//! Positions are the plan's millimetres, as offsets from the design's first point; the picture maps
//! them to document points (`mm_to_pt`).

use std::collections::HashMap;
use std::sync::Arc;

use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::{Point, Rect};
use vectorcraft_render::lines::{LinePicture, LinePictureBuilder, LineStyle, LinkKind, MarkKind, edge_against};
use vectorcraft_stitch::plan::{self, THREAD_WIDTH};
use vectorcraft_stitch::{Command, mm_to_pt};

use super::measure;
use super::plan::{DesignPlan, UnitKey};

/// How far off the line drawn in their place the needle points left out may lie when the whole
/// design is drawn, millimetres: 2 µm, under the formats' 0.1 mm, and under a tenth of a pixel up
/// to about 880 % on a screen of two pixels a point (a millimetre is 2.83 points), past which every
/// point is drawn ([`vectorcraft_render::lines::STRAIGHT_SKIP_PX`]). An estimate.
pub const STRAIGHT_TOLERANCE_MM: f64 = 0.002;

/// The fabric the stitches are drawn on when the document's fabric has no colour (sRGB): white, the
/// colour an artboard is drawn in.
pub const FABRIC: [u8; 3] = [255, 255, 255];

/// The fabric the stitches of `doc` are drawn on (sRGB): its fabric's colour (the document's
/// embroidery settings), else [`FABRIC`].
pub fn fabric_color(doc: &Document) -> [u8; 3] {
    doc.embroidery.as_deref().and_then(|e| e.fabric.as_deref()).and_then(|f| f.color).unwrap_or(FABRIC)
}

/// The two tones jumps and cuts are drawn in: near-black and near-white, so one of them contrasts
/// with any thread, the same in every theme (they lie on thread and fabric, not on the interface).
/// Estimates.
pub const MARK_DARK: [u8; 3] = [0x1e, 0x1e, 0x1e];
pub const MARK_LIGHT: [u8; 3] = [0xf5, 0xf5, 0xf5];

/// One thread block as drawn: its thread's colour and its art's (sRGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneBlock {
    pub thread: [u8; 3],
    pub art: [u8; 3],
}

/// One sewing unit as drawn: the object a person selects for it, its key, where it starts and ends
/// and the box its stitches fall in (document points).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneUnit {
    pub object: NodeId,
    pub key: UnitKey,
    pub block: usize,
    /// Its first and last needle points: where it enters and leaves the fabric (`None`: it has
    /// none of its own).
    pub entry: Option<Point>,
    pub exit: Option<Point>,
    pub bounds: Option<Rect>,
    /// The entry and exit points it was planned toward (its part's, when its object sets them),
    /// and how far its first and last needle points miss them, millimetres
    /// ([`DesignPlan::missed`]).
    pub asked: [Option<Point>; 2],
    pub missed: [Option<f64>; 2],
}

/// Where a selected object is sewn ([`StitchScene::ends_of`]): its units by tag, in sewing order,
/// and where the needle enters and leaves it (document points; `None` when it sews nothing).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectEnds {
    pub object: NodeId,
    pub units: Vec<u32>,
    pub entry: Option<Point>,
    pub exit: Option<Point>,
}

/// A design's stitches as the stitch preview draws them: the picture (its lines tagged with unit
/// indices, its colour slots the block indices) and what the tags and slots are. Immutable, and
/// shared between threads.
#[derive(Clone, Debug, Default)]
pub struct StitchScene {
    picture: Arc<LinePicture>,
    blocks: Vec<SceneBlock>,
    units: Vec<SceneUnit>,
    /// The picture's box, worked out once (the canvas lays the fabric under it every frame).
    bounds: Option<Rect>,
}

/// Scenes are built and drawn on worker threads.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<StitchScene>();
};

impl StitchScene {
    /// The picture the renderer draws.
    pub fn picture(&self) -> &Arc<LinePicture> {
        &self.picture
    }

    /// Each block's colours, by colour slot.
    pub fn blocks(&self) -> &[SceneBlock] {
        &self.blocks
    }

    /// The object a person selects whose stitches lie on top under `at` (document points), within
    /// `tol` (document points) past a thread's laid half width: the embroidery tool's press on the
    /// stitches ([`LinePicture::hit`]). `None` where no stitch lies.
    pub fn pick(&self, at: Point, tol: f64) -> Option<NodeId> {
        let tag = self.picture.hit(at, tol)?;
        self.units.get(usize::try_from(tag).ok()?).map(|u| u.object)
    }

    /// Each unit, by tag (the plan's unit index).
    pub fn units(&self) -> &[SceneUnit] {
        &self.units
    }

    /// The colours the drawing paints each block in: each thread's (what the machine sews).
    pub fn thread_colors(&self) -> Vec<[u8; 3]> {
        self.blocks.iter().map(|b| b.thread).collect()
    }

    /// The box everything it draws falls in, document points (`None`: it draws nothing).
    pub fn bounds(&self) -> Option<Rect> {
        self.bounds
    }

    /// Where each of `selected` (objects of `doc`, as a selection holds them) is sewn: its units,
    /// those of the objects inside it included, in sewing order, the needle's entry (its first
    /// unit's first needle point) and its exit (its last unit's last). An object that sews nothing
    /// has no units. One walk of each selected object and one pass over the units: the Sewing order
    /// list and the canvas's markers read the same ends.
    pub fn ends_of(&self, doc: &Document, selected: &[NodeId]) -> Vec<ObjectEnds> {
        let mut owner: HashMap<NodeId, usize> = HashMap::new();
        for (i, id) in selected.iter().enumerate() {
            if let Some(n) = doc.node(*id) {
                n.walk(&mut |c| {
                    owner.entry(c.id).or_insert(i);
                });
            }
        }
        let mut ends: Vec<ObjectEnds> = selected.iter().map(|&object| ObjectEnds { object, units: Vec::new(), entry: None, exit: None }).collect();
        for (tag, u) in self.units.iter().enumerate() {
            let (Some(e), Ok(tag)) = (owner.get(&u.object).and_then(|i| ends.get_mut(*i)), u32::try_from(tag)) else { continue };
            e.units.push(tag);
            e.entry = e.entry.or(u.entry);
            e.exit = u.exit.or(e.exit);
        }
        ends
    }

    /// How the stitch preview draws it on `fabric`: each block in its thread's colour, an edge for
    /// a thread close to the fabric, jumps and cuts when `jumps`, at `px_per_pt` device pixels a
    /// screen point. The rest is the drawing's defaults.
    pub fn style(&self, fabric: [u8; 3], jumps: bool, needle_points: bool, px_per_pt: f64) -> LineStyle {
        let colors = self.thread_colors();
        let edges = colors.iter().map(|c| edge_against(*c, fabric)).collect();
        LineStyle {
            colors,
            edges,
            backdrop: fabric,
            links: jumps,
            marks: jumps,
            needle_points,
            dark: MARK_DARK,
            light: MARK_LIGHT,
            px_per_pt,
            ..LineStyle::default()
        }
    }

    /// About how many bytes it holds (the planning service's memory bound counts it).
    pub fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.picture.byte_size()
            + self.blocks.capacity() * std::mem::size_of::<SceneBlock>()
            + self.units.capacity() * std::mem::size_of::<SceneUnit>()
    }
}

/// `p` millimetres as document points.
fn to_pt(p: Point) -> Point {
    Point::new(mm_to_pt(p.x), mm_to_pt(p.y))
}

/// The scene of `plan`.
pub fn scene(plan: &DesignPlan) -> StitchScene {
    let first = plan.blocks.iter().flat_map(|b| b.stitches.first()).map(|s| s.point()).find(|p| p.x.is_finite() && p.y.is_finite());
    let mut b = LinePictureBuilder::new(first.unwrap_or(Point::ZERO), mm_to_pt(1.0)).straight_within(STRAIGHT_TOLERANCE_MM);
    // Each unit's laid width, millimetres.
    let widths: Vec<f32> = plan.units.iter().map(|u| u.params.map_or(THREAD_WIDTH, |p| plan::thread_width(p.thread_weight)) as f32).collect();
    // The unit whose line is open, and whether the thread was cut since the last needle point.
    let mut open: Option<usize> = None;
    let mut cut = false;
    measure::each(plan, |r| {
        let unit = r.unit;
        let Some(u) = plan.units.get(unit) else { return };
        let (group, tag) = (u32::try_from(u.block).unwrap_or(u32::MAX), u32::try_from(unit).unwrap_or(u32::MAX));
        let number = u32::try_from(r.number).unwrap_or(u32::MAX);
        let at = r.stitch.point();
        match r.stitch.command {
            Command::Stitch => {
                match (r.sewn().is_some(), r.before) {
                    // The thread goes on from the needle point before, in this unit's line.
                    (true, _) if open == Some(unit) => {}
                    // From the needle point before, which the unit before sewed: a lead-in.
                    (true, Some(before)) => {
                        b.begin_line(group, tag, widths.get(unit).copied().unwrap_or(0.0));
                        b.point(before.point(), number.saturating_sub(1));
                    }
                    // After a jump, a cut or a thread change: a new line.
                    _ => b.begin_line(group, tag, widths.get(unit).copied().unwrap_or(0.0)),
                }
                b.point(at, number);
                open = Some(unit);
                cut = false;
            }
            Command::Jump => {
                b.end_line();
                open = None;
                // A move between blocks is the machine's own, with the thread changed: none is
                // drawn (the plan holds no jump there).
                if let Some(before) = r.before.filter(|_| r.same_block) {
                    b.link(before.point(), at, if cut { LinkKind::Cut } else { LinkKind::Thread }, group, tag, number);
                }
            }
            Command::Trim => {
                b.end_line();
                open = None;
                b.mark(at, MarkKind::Cross, tag, number);
                cut = true;
            }
            Command::Stop => {
                b.end_line();
                open = None;
                b.mark(at, MarkKind::Square, tag, number);
            }
            Command::ColorChange => {
                b.end_line();
                open = None;
            }
        }
    });
    let blocks = plan.blocks.iter().map(|k| SceneBlock { thread: k.thread.color(), art: k.thread.art }).collect();
    let units = plan
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let mut needle = plan.unit_stitches(i).iter().filter(|s| s.command == Command::Stitch).map(|s| to_pt(s.point()));
            let entry = needle.next();
            let exit = needle.next_back().or(entry);
            let bounds = u.bounds.map(|r| Rect::from_points(to_pt(Point::new(r.x0, r.y0)), to_pt(Point::new(r.x1, r.y1))));
            let asked = [u.asked.entry.map(to_pt), u.asked.exit.map(to_pt)];
            SceneUnit { object: u.origin.selected(), key: u.key(), block: u.block, entry, exit, bounds, asked, missed: plan.missed(i) }
        })
        .collect();
    let picture = b.build();
    let bounds = picture.bounds();
    StitchScene { picture: Arc::new(picture), blocks, units, bounds }
}
