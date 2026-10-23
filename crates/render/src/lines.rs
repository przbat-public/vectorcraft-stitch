//! Line pictures ([`LinePicture`]): polylines drawn in order as lines of a width, the moves between
//! them drawn as thin links, and points marked, all drawn by [`LineRenderer`] with a look chosen at
//! draw time ([`LineStyle`]). Fork-owned (VectorCraft Stitch): the stitch preview's drawing.
//!
//! Nothing here knows about stitches. The engine builds a picture from a design's stitch plan
//! (`cmd::embroidery::scene`): each run of needle points a line in its thread's slot, each jump a
//! link, each cut and stop a mark, with the plan's record numbers. The canvas, headless renders
//! and later views draw that picture through one function, so they show the same pixels.
//!
//! **Geometry only.** A picture holds points (`f32` offsets from an `f64` origin, in picture
//! units), each with its number, and lines of at most [`MAX_LINE_POINTS`] points (consecutive pieces
//! of one polyline share their joining point), each with its colour slot (`group`), an opaque tag
//! (the caller's: a sewing unit's index) and its laid width. Colours, the width's pixel minimum, which
//! tags are hidden or faded, a range of numbers, and whether links and marks show are the style's,
//! so changing any of them draws the same picture again without building it again.
//!
//! **Drawing.** Lines are stroked in order with round joins and caps, later lines over earlier
//! ones; consecutive lines of one colour are batched into paths of at most [`MAX_BATCH_SEGMENTS`]
//! segments (vello sorts each path's tiles and makes its strips on one thread, so one huge path
//! would serialise the frame), each batch culled by its lines' boxes. A colour close to the
//! backdrop ([`edge_against`]) is drawn over an edge a little wider, so white thread on white
//! fabric stays visible. Links and marks are drawn last, at a fixed size on screen, in two tones
//! (one of which contrasts with any colour under them).

use std::ops::Range;

use vectorcraft_geom::{Affine, BezPath, Point, Rect};
use vello_cpu::kurbo::{self, Shape};
use vello_cpu::peniko;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::{MAX_DEVICE, Rendered};

/// The most points a line holds: a run is cut into lines of at most this many, consecutive pieces
/// sharing their joining point, so that a line's box stays tight for culling (a fill's rows are one
/// run of thousands of points). On the 100 mm watermelon, 64-point pieces put a point in 13 boxes
/// on average (research for the stitch preview, measured on the design's PES); an estimate.
pub const MAX_LINE_POINTS: usize = 64;

/// The most segments one path is drawn with: consecutive lines of one colour are joined into paths
/// of at most this many, so that vello's threads share a frame's work and no path's tile sort grows
/// large. Measured (`vectorcraft-cli perf --embroidery` with `VECTORCRAFT_STITCH_SWEEP`, 12 cores,
/// 2026-10-23): from 64 segments to a whole run of one colour, frames of 10,000 to 500,000
/// stitches took the same time within noise (100,000 at fit 6.1 to 6.5 ms), so the size the
/// research proposed is kept; a path never grows with a design.
pub const MAX_BATCH_SEGMENTS: usize = 512;

/// The narrowest a laid line is drawn, device pixels: as the art renderer keeps a stroke at least
/// half a pixel wide, so a thread seen from afar still shows.
pub const MIN_LINE_PX: f64 = 0.5;

/// How wide a hairline is drawn, device pixels.
pub const HAIRLINE_PX: f64 = 1.0;

/// How much wider than its line an edge is drawn on each side, device pixels: the darker (or
/// lighter) rim a colour close to the backdrop gets. An estimate.
pub const EDGE_PX: f64 = 1.0;

/// The colour difference to the backdrop (CIEDE2000) under which a colour gets an edge: the line
/// the embroidery code draws between colours that look alike and colours that look clearly unlike
/// (`vectorcraft_stitch::thread::MISMATCH_DELTA_E`, an estimate).
pub const NEAR_BACKDROP_DELTA_E: f32 = 10.0;

/// The contrast an edge reaches against the backdrop: WCAG 2.1's for graphics (SC 1.4.11 Non-text
/// Contrast, <https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast.html>).
pub const EDGE_CONTRAST: f64 = 3.0;

/// The share of a faded line's colour mixed toward the backdrop: as the art around an isolated
/// group is dimmed ([`crate::ISOLATION_DIM`]).
pub const FADE: f32 = 1.0 - crate::ISOLATION_DIM;

/// How wide a link is drawn, screen points: as the canvas's 1 pt artboard edge. An estimate.
pub const LINK_PT: f64 = 1.0;
/// How long each dash of a link after a cut is, screen points (dark and light alternating, no
/// gap). An estimate.
pub const LINK_DASH_PT: f64 = 3.0;
/// How wide the contrasting halo on each side of a link the thread lies along is, screen points.
/// An estimate.
pub const LINK_HALO_PT: f64 = 0.75;
/// Links shorter than this on screen are left out, screen points: they would draw a speck. An
/// estimate.
pub const LINK_MIN_PT: f64 = 4.0;
/// Half the size of a mark (a cut's ×, a stop's square), screen points. An estimate.
pub const MARK_PT: f64 = 4.5;
/// How wide a mark's dark stroke is, screen points, and its light halo. Estimates.
pub const MARK_LINE_PT: f64 = 1.0;
pub const MARK_HALO_PT: f64 = 2.5;

/// How many lines are drawn between two checks of the cancel flag ([`vectorcraft_doc::cancel`]),
/// so that a frame for a view nobody waits for any longer stops soon. An estimate.
const CANCEL_EVERY: usize = 256;

/// One line: a polyline of a picture's points, in one colour slot, with its tag and laid width.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its points: a range of [`LinePicture`]'s.
    pub points: Range<u32>,
    /// Its colour slot: an index into the style's colours.
    pub group: u32,
    /// The caller's tag (the stitch preview: the sewing unit's index), which the style hides or
    /// fades by.
    pub tag: u32,
    /// How wide it is laid, picture units.
    pub width: f32,
    /// The box its points fall in, picture units (offsets from the origin): left, top, right,
    /// bottom.
    pub bounds: [f32; 4],
}

/// How a link is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// A thread lies along it (a move the thread isn't cut before): a thin solid line in its slot's
    /// colour, with a contrasting halo.
    Thread,
    /// Nothing lies along it (a move after a cut): two-tone dashes.
    Cut,
}

/// A link: a move from one point to another, drawn when the style asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub kind: LinkKind,
    pub group: u32,
    pub tag: u32,
    /// Its number (the stitch preview: the jump's record number).
    pub number: u32,
}

/// How a mark is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkKind {
    /// A two-tone × (a cut).
    Cross,
    /// A two-tone square (a stop).
    Square,
}

/// A marked point, drawn when the style asks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: [f32; 2],
    pub kind: MarkKind,
    pub tag: u32,
    pub number: u32,
}

/// A picture of lines, links and marks ([`LinePictureBuilder`] makes one). Immutable and shared
/// between threads; its `Debug` prints counts, not points.
#[derive(Clone, Default, PartialEq)]
pub struct LinePicture {
    /// Where its offsets start, picture units.
    origin: [f64; 2],
    /// Document points in a picture unit.
    to_doc: f64,
    points: Vec<[f32; 2]>,
    /// Each point's number, in step with `points`.
    numbers: Vec<u32>,
    /// The points a line passes straight through (one bit each, by index into `points`): a drawing
    /// of the whole picture leaves them out, which changes no pixel; one of a range of numbers keeps
    /// them, so a range can end there.
    straight: Vec<u64>,
    lines: Vec<Line>,
    links: Vec<Link>,
    marks: Vec<Mark>,
}

impl std::fmt::Debug for LinePicture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinePicture")
            .field("points", &self.points.len())
            .field("lines", &self.lines.len())
            .field("links", &self.links.len())
            .field("marks", &self.marks.len())
            .finish()
    }
}

/// Pictures are drawn on worker threads.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<LinePicture>();
};

impl LinePicture {
    /// Its lines, in drawing order.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// How many points its lines hold.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Whether point `i` is one its line passes straight through.
    pub fn is_straight(&self, i: usize) -> bool {
        self.straight.get(i / 64).is_some_and(|w| w & (1 << (i % 64)) != 0)
    }

    /// Point `i` in picture units, and its number.
    pub fn point(&self, i: usize) -> Option<(Point, u32)> {
        let p = self.points.get(i)?;
        Some((self.unoffset(*p), *self.numbers.get(i)?))
    }

    /// The points of `line`, picture units.
    pub fn line_points(&self, line: &Line) -> impl Iterator<Item = Point> + '_ {
        let r = line.points.start as usize..line.points.end as usize;
        self.points.get(r).unwrap_or_default().iter().map(|p| self.unoffset(*p))
    }

    /// Picture units to document points.
    pub fn to_doc(&self) -> Affine {
        Affine::scale(self.to_doc)
    }

    /// The offsets the picture keeps (from its origin) to document points.
    fn offsets_to_doc(&self) -> Affine {
        self.to_doc() * Affine::translate((self.origin[0], self.origin[1]))
    }

    /// The box its lines, links and marks fall in, document points (`None`: empty).
    pub fn bounds(&self) -> Option<Rect> {
        let mut b: Option<[f32; 4]> = None;
        let mut take = |r: [f32; 4]| {
            b = Some(b.map_or(r, |o| [o[0].min(r[0]), o[1].min(r[1]), o[2].max(r[2]), o[3].max(r[3])]));
        };
        for l in &self.lines {
            take(l.bounds);
        }
        for k in &self.links {
            take(box_of(k.from, k.to));
        }
        for m in &self.marks {
            take(box_of(m.at, m.at));
        }
        let [x0, y0, x1, y1] = b?;
        Some(self.offsets_to_doc().transform_rect_bbox(Rect::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1))))
    }

    /// About how many bytes it holds (for the memory bounds of whoever keeps pictures).
    pub fn byte_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.points.capacity() * std::mem::size_of::<[f32; 2]>()
            + self.numbers.capacity() * std::mem::size_of::<u32>()
            + self.straight.capacity() * std::mem::size_of::<u64>()
            + self.lines.capacity() * std::mem::size_of::<Line>()
            + self.links.capacity() * std::mem::size_of::<Link>()
            + self.marks.capacity() * std::mem::size_of::<Mark>()
    }

    fn unoffset(&self, [x, y]: [f32; 2]) -> Point {
        Point::new(self.origin[0] + f64::from(x), self.origin[1] + f64::from(y))
    }
}

/// The box of two offsets.
fn box_of(a: [f32; 2], b: [f32; 2]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

/// A line being built.
struct Open {
    group: u32,
    tag: u32,
    width: f32,
    start: usize,
}

/// Builds a [`LinePicture`]: lines point by point, links and marks. Points that aren't finite (or
/// that would not fit an offset from the origin) are left out: a line is broken there. A point
/// within the straight tolerance of the segment between its neighbours, going on the same way, is
/// marked as one its line passes straight through ([`LinePicture::is_straight`]).
pub struct LinePictureBuilder {
    picture: LinePicture,
    open: Option<Open>,
    /// How far off the segment between its neighbours a point may lie and still be passed straight
    /// through, picture units (0: none is).
    straight_tolerance: f32,
}

impl LinePictureBuilder {
    /// A picture whose points are offsets from `origin` (picture units: give a point near the
    /// picture, such as its first, so offsets stay small and exact), `to_doc` document points a
    /// picture unit.
    pub fn new(origin: Point, to_doc: f64) -> Self {
        let origin = if origin.x.is_finite() && origin.y.is_finite() { [origin.x, origin.y] } else { [0.0, 0.0] };
        let to_doc = if to_doc.is_finite() && to_doc > 0.0 { to_doc } else { 1.0 };
        Self { picture: LinePicture { origin, to_doc, ..LinePicture::default() }, open: None, straight_tolerance: 0.0 }
    }

    /// Mark the points a line passes straight through within `tolerance` picture units (far under a
    /// pixel at any zoom the picture is seen at, so leaving them out changes nothing drawn).
    pub fn straight_within(mut self, tolerance: f64) -> Self {
        self.straight_tolerance = if tolerance.is_finite() && tolerance > 0.0 { tolerance as f32 } else { 0.0 };
        self
    }

    /// Mark the point before the last of the open line when the line passes straight through it.
    fn mark_straight(&mut self, start: usize) {
        let n = self.picture.points.len();
        if self.straight_tolerance <= 0.0 || n < start + 3 {
            return;
        }
        let (Some(a), Some(m), Some(b)) = (self.picture.points.get(n - 3), self.picture.points.get(n - 2), self.picture.points.get(n - 1)) else {
            return;
        };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        let (mx, my) = (m[0] - a[0], m[1] - a[1]);
        // Between its neighbours (going on the same way), and off their segment by less than the
        // tolerance.
        let along = (mx * dx + my * dy) / len.max(f32::MIN_POSITIVE);
        let off = (mx * dy - my * dx).abs() / len.max(f32::MIN_POSITIVE);
        if len > 0.0 && along > 0.0 && along < len && off <= self.straight_tolerance {
            let i = n - 2;
            let word = i / 64;
            if self.picture.straight.len() <= word {
                self.picture.straight.resize(word + 1, 0);
            }
            if let Some(w) = self.picture.straight.get_mut(word) {
                *w |= 1 << (i % 64);
            }
        }
    }

    /// `p` as an offset from the origin, when it is a number that fits.
    fn offset(&self, p: Point) -> Option<[f32; 2]> {
        let (x, y) = ((p.x - self.picture.origin[0]) as f32, (p.y - self.picture.origin[1]) as f32);
        (x.is_finite() && y.is_finite()).then_some([x, y])
    }

    /// Start a line in colour slot `group`, tagged `tag`, laid `width` picture units wide (the line
    /// open before is ended).
    pub fn begin_line(&mut self, group: u32, tag: u32, width: f32) {
        self.end_line();
        let width = if width.is_finite() && width >= 0.0 { width } else { 0.0 };
        self.open = Some(Open { group, tag, width, start: self.picture.points.len() });
    }

    /// Add point `p`, numbered `number`, to the line open (none open: nothing). A point that isn't
    /// a number breaks the line: the next starts a new piece.
    pub fn point(&mut self, p: Point, number: u32) {
        let Some(open) = &self.open else { return };
        let (group, tag, width, start) = (open.group, open.tag, open.width, open.start);
        let Some(at) = self.offset(p) else {
            self.end_line();
            self.open = Some(Open { group, tag, width, start: self.picture.points.len() });
            return;
        };
        if self.picture.points.len().saturating_sub(start) >= MAX_LINE_POINTS {
            // Full: end it, and go on in a new piece from its last point.
            let last = self.picture.points.last().copied().zip(self.picture.numbers.last().copied());
            self.end_line();
            let start = self.picture.points.len();
            if let Some((q, n)) = last {
                self.picture.points.push(q);
                self.picture.numbers.push(n);
            }
            self.open = Some(Open { group, tag, width, start });
        }
        self.picture.points.push(at);
        self.picture.numbers.push(number);
        if let Some(open) = &self.open {
            let start = open.start;
            self.mark_straight(start);
        }
    }

    /// End the line open, keeping it when it has a point.
    pub fn end_line(&mut self) {
        let Some(open) = self.open.take() else { return };
        let (start, end) = (open.start, self.picture.points.len());
        let Some(pts) = self.picture.points.get(start..end).filter(|p| !p.is_empty()) else { return };
        let bounds = pts.iter().fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |b, p| {
            [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]
        });
        let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end)) else { return };
        self.picture.lines.push(Line { points: start..end, group: open.group, tag: open.tag, width: open.width, bounds });
    }

    /// Add a link from `from` to `to` (left out unless both are numbers).
    pub fn link(&mut self, from: Point, to: Point, kind: LinkKind, group: u32, tag: u32, number: u32) {
        if let (Some(from), Some(to)) = (self.offset(from), self.offset(to)) {
            self.picture.links.push(Link { from, to, kind, group, tag, number });
        }
    }

    /// Mark point `at` (left out unless it is a number).
    pub fn mark(&mut self, at: Point, kind: MarkKind, tag: u32, number: u32) {
        if let Some(at) = self.offset(at) {
            self.picture.marks.push(Mark { at, kind, tag, number });
        }
    }

    /// The picture, its open line ended.
    pub fn build(mut self) -> LinePicture {
        self.end_line();
        let p = &mut self.picture;
        p.points.shrink_to_fit();
        p.numbers.shrink_to_fit();
        p.straight.shrink_to_fit();
        p.lines.shrink_to_fit();
        p.links.shrink_to_fit();
        p.marks.shrink_to_fit();
        self.picture
    }
}

/// A set of tags (one bit each).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TagSet(Vec<u64>);

impl TagSet {
    /// The set of `tags`.
    pub fn of(tags: impl IntoIterator<Item = u32>) -> Self {
        let mut s = Self::default();
        for t in tags {
            s.insert(t);
        }
        s
    }

    pub fn insert(&mut self, tag: u32) {
        let (word, bit) = ((tag / 64) as usize, tag % 64);
        if self.0.len() <= word {
            self.0.resize(word + 1, 0);
        }
        if let Some(w) = self.0.get_mut(word) {
            *w |= 1 << bit;
        }
    }

    pub fn contains(&self, tag: u32) -> bool {
        self.0.get((tag / 64) as usize).is_some_and(|w| w & (1 << (tag % 64)) != 0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }
}

/// How lines are as wide as they are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Widths {
    /// Each line as wide as it is laid, never narrower than `min_px` device pixels.
    Laid { min_px: f64 },
    /// Every line a hairline ([`HAIRLINE_PX`]).
    Hairline,
}

/// How a picture is drawn: everything that changes what is seen without changing the picture.
#[derive(Clone, Debug, PartialEq)]
pub struct LineStyle {
    /// Each colour slot's colour (sRGB); a slot past the end is drawn black.
    pub colors: Vec<[u8; 3]>,
    /// Each colour slot's edge ([`edge_against`]); `None`, or a slot past the end, draws none.
    pub edges: Vec<Option<[u8; 3]>>,
    pub widths: Widths,
    /// The colour faded lines are mixed toward ([`FADE`]): the backdrop they are seen against.
    pub backdrop: [u8; 3],
    /// Tags not drawn.
    pub hidden: Option<TagSet>,
    /// Tags drawn faded toward the backdrop.
    pub faded: Option<TagSet>,
    /// Only the numbers in this range are drawn: a line's segment by its end point's number, a
    /// link and a mark by theirs.
    pub numbers: Option<Range<u32>>,
    pub links: bool,
    pub marks: bool,
    /// The two tones links and marks are drawn in: one of them contrasts with whatever is under
    /// them.
    pub dark: [u8; 3],
    pub light: [u8; 3],
    /// Device pixels in a screen point (the screen sizes of links and marks).
    pub px_per_pt: f64,
    /// Filled under everything (premultiplied RGBA); `None`, transparent.
    pub background: Option<[u8; 4]>,
    /// Smooth edges; off, a pixel is painted when a line covers at least half of it.
    pub anti_alias: bool,
}

impl Default for LineStyle {
    fn default() -> Self {
        Self {
            colors: Vec::new(),
            edges: Vec::new(),
            widths: Widths::Laid { min_px: MIN_LINE_PX },
            backdrop: [255, 255, 255],
            hidden: None,
            faded: None,
            numbers: None,
            links: true,
            marks: true,
            dark: [0x1e, 0x1e, 0x1e],
            light: [0xf5, 0xf5, 0xf5],
            px_per_pt: 1.0,
            background: None,
            anti_alias: true,
        }
    }
}

impl LineStyle {
    /// Whether tag `tag` is drawn.
    fn shows(&self, tag: u32) -> bool {
        !self.hidden.as_ref().is_some_and(|h| h.contains(tag))
    }

    fn fades(&self, tag: u32) -> bool {
        self.faded.as_ref().is_some_and(|f| f.contains(tag))
    }

    /// Whether number `n` is drawn.
    fn counts(&self, n: u32) -> bool {
        self.numbers.as_ref().is_none_or(|r| r.contains(&n))
    }

    /// Slot `group`'s colour, faded when `faded`.
    fn color(&self, group: u32, faded: bool) -> [u8; 3] {
        let c = self.colors.get(group as usize).copied().unwrap_or([0, 0, 0]);
        if faded { mix(c, self.backdrop, FADE) } else { c }
    }

    fn edge(&self, group: u32) -> Option<[u8; 3]> {
        self.edges.get(group as usize).copied().flatten()
    }
}

/// `a` mixed toward `b` by `share` (0: `a`, 1: `b`), in sRGB.
fn mix(a: [u8; 3], b: [u8; 3], share: f32) -> [u8; 3] {
    let share = share.clamp(0.0, 1.0);
    std::array::from_fn(|i| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * share).round().clamp(0.0, 255.0) as u8)
}

/// WCAG 2.1's relative luminance of an sRGB colour.
fn luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| f64::from(vectorcraft_color::cms::lab::srgb_to_linear(f32::from(v) / 255.0));
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG 2.1's contrast ratio between two sRGB colours (1 to 21).
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The steps an edge's colour is mixed toward black or white in, until it contrasts enough.
const EDGE_STEPS: u16 = 20;

/// The edge a line of `color` gets against `backdrop`: when the two look alike (CIEDE2000 under
/// [`NEAR_BACKDROP_DELTA_E`]), `color` mixed toward black or white (whichever contrasts more with
/// the backdrop) until it reaches [`EDGE_CONTRAST`] against it; `None` for a colour that stands out
/// already.
pub fn edge_against(color: [u8; 3], backdrop: [u8; 3]) -> Option<[u8; 3]> {
    use vectorcraft_color::cms::lab::{delta_e2000, srgb_to_lab};
    let lab = |c: [u8; 3]| srgb_to_lab(c.map(|v| f32::from(v) / 255.0));
    if delta_e2000(lab(color), lab(backdrop)) >= NEAR_BACKDROP_DELTA_E {
        return None;
    }
    let toward = if contrast([0, 0, 0], backdrop) >= contrast([255, 255, 255], backdrop) { [0, 0, 0] } else { [255, 255, 255] };
    let edge = (1..=EDGE_STEPS)
        .map(|k| mix(color, toward, f32::from(k) / f32::from(EDGE_STEPS)))
        .find(|e| contrast(*e, backdrop) >= EDGE_CONTRAST)
        .unwrap_or(toward);
    Some(edge)
}

/// What one frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineStats {
    /// Lines drawn and lines left out because they lie outside the view.
    pub drawn: usize,
    pub culled: usize,
    /// Paths stroked (edges included).
    pub paths: usize,
    pub links: usize,
    pub marks: usize,
    pub micros: u64,
}

/// Draws [`LinePicture`]s, reusing its render contexts between frames.
pub struct LineRenderer {
    /// Rasterizer worker threads (0: on the calling thread), as [`crate::Renderer::threads`].
    pub threads: u16,
    /// The most segments a path is drawn with ([`MAX_BATCH_SEGMENTS`]; measurements sweep it).
    pub batch_segments: usize,
    ctx: Option<RenderContext>,
    resources: Resources,
    pub stats: LineStats,
}

impl Default for LineRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// The state of the batch of lines being joined into one path.
struct Batch {
    path: BezPath,
    segments: usize,
}

impl LineRenderer {
    pub fn new() -> Self {
        Self {
            threads: crate::default_threads(),
            batch_segments: MAX_BATCH_SEGMENTS,
            ctx: None,
            resources: Resources::new(),
            stats: LineStats::default(),
        }
    }

    /// Draw `picture` into a `width`×`height` image through `view` (document points to device
    /// pixels), as `style` says.
    pub fn render(&mut self, picture: &LinePicture, width: u32, height: u32, view: Affine, style: &LineStyle) -> Rendered {
        let start = crate::now();
        let (w, h) = (width.clamp(1, u32::from(u16::MAX)) as u16, height.clamp(1, u32::from(u16::MAX)) as u16);
        let threads = self.threads;
        let mut ctx = match self.ctx.take() {
            Some(mut c) if c.width() == w && c.height() == h && usize::from(threads > 0) == usize::from(c.is_multi_threaded()) => {
                c.reset();
                c
            }
            _ => RenderContext::new_with(w, h, vello_cpu::RenderSettings { num_threads: threads, ..Default::default() }),
        };
        ctx.set_aliasing_threshold((!style.anti_alias).then_some(127));
        self.stats = LineStats::default();
        if let Some(bg) = style.background {
            ctx.set_transform(Affine::IDENTITY);
            ctx.set_paint(peniko::Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
            ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        }
        // Picture units, and the offsets the lines keep, to device pixels.
        let to_device = view * picture.to_doc();
        let offsets = view * picture.offsets_to_doc();
        // Device pixels in a picture unit, and the view in offsets.
        let scale = to_device.determinant().abs().sqrt();
        if scale.is_finite() && scale > 0.0 {
            let visible = offsets.inverse().transform_rect_bbox(Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
            self.draw_lines(&mut ctx, picture, offsets, scale, visible, style);
            if style.links {
                self.draw_links(&mut ctx, picture, to_device, w, h, style);
            }
            if style.marks {
                self.draw_marks(&mut ctx, picture, to_device, w, h, style);
            }
        }
        ctx.flush();
        let mut pm = Pixmap::new(w, h);
        ctx.render_with(&mut pm, &mut self.resources, vello_cpu::RasterizerSettings::default());
        self.ctx = Some(ctx);
        self.stats.micros = crate::now().saturating_sub(start);
        Rendered { width: u32::from(w), height: u32::from(h), pixels: pm.data_as_u8_slice().to_vec() }
    }

    /// The lines, in order: runs of lines of one colour slot (and fade) batched into paths; a slot
    /// with an edge has all its run's edges drawn first, then its colour over them, so the edge
    /// never shows between two lines of one colour.
    fn draw_lines(&mut self, ctx: &mut RenderContext, p: &LinePicture, offsets: Affine, scale: f64, visible: Rect, style: &LineStyle) {
        ctx.set_transform(offsets);
        let lines = p.lines();
        let mut i = 0;
        let mut since_check = 0;
        while i < lines.len() {
            let Some(first) = lines.get(i) else { break };
            let faded = style.fades(first.tag);
            let key = (first.group, faded);
            let end = lines.get(i..).map_or(i, |rest| i + rest.iter().take_while(|l| (l.group, style.fades(l.tag)) == key).count());
            let run = lines.get(i..end).unwrap_or_default();
            if let Some(edge) = style.edge(first.group).filter(|_| !faded) {
                self.stroke_run(ctx, p, run, scale, visible, style, edge, EDGE_PX * 2.0, false);
            }
            self.stroke_run(ctx, p, run, scale, visible, style, style.color(first.group, faded), 0.0, true);
            since_check += run.len();
            if since_check >= CANCEL_EVERY {
                since_check = 0;
                if vectorcraft_doc::cancel::is_cancelled() {
                    return;
                }
            }
            i = end.max(i + 1);
        }
    }

    /// Stroke `run`'s lines in `color`, `extra_px` device pixels wider than drawn, in batches of at
    /// most [`MAX_BATCH_SEGMENTS`] segments; `count` the lines in the stats.
    #[allow(clippy::too_many_arguments)]
    fn stroke_run(
        &mut self,
        ctx: &mut RenderContext,
        p: &LinePicture,
        run: &[Line],
        scale: f64,
        visible: Rect,
        style: &LineStyle,
        color: [u8; 3],
        extra_px: f64,
        count: bool,
    ) {
        ctx.set_paint(peniko::Color::from_rgb8(color[0], color[1], color[2]));
        let mut batch = Batch { path: BezPath::new(), segments: 0 };
        let mut width = None;
        for line in run {
            if !style.shows(line.tag) {
                continue;
            }
            let px = match style.widths {
                Widths::Laid { min_px } => (f64::from(line.width) * scale).max(min_px),
                Widths::Hairline => HAIRLINE_PX,
            } + extra_px;
            // A line's half width and a pixel round its box, picture units.
            let grow = (px / 2.0 + 1.0) / scale;
            let b = Rect::new(f64::from(line.bounds[0]), f64::from(line.bounds[1]), f64::from(line.bounds[2]), f64::from(line.bounds[3]));
            let b = b.inflate(grow, grow);
            if b.x1 < visible.x0 || b.x0 > visible.x1 || b.y1 < visible.y0 || b.y0 > visible.y1 {
                if count {
                    self.stats.culled += 1;
                }
                continue;
            }
            let device = b.size().width.max(b.size().height) * scale;
            if !(device.is_finite() && device <= MAX_DEVICE) {
                continue;
            }
            // One width a path: a line of another width ends the batch.
            let w = px / scale;
            if width.is_some_and(|x: f64| (x - w).abs() > f64::EPSILON) || batch.segments >= self.batch_segments {
                self.flush(ctx, &mut batch, width);
            }
            width = Some(w);
            batch.segments += add_line(&mut batch.path, p, line, style, w);
            if count {
                self.stats.drawn += 1;
            }
        }
        self.flush(ctx, &mut batch, width);
    }

    /// Stroke `batch` `width` picture units wide, and empty it.
    fn flush(&mut self, ctx: &mut RenderContext, batch: &mut Batch, width: Option<f64>) {
        if let Some(width) = width.filter(|_| !batch.path.elements().is_empty()) {
            ctx.set_stroke(kurbo::Stroke::new(width));
            ctx.stroke_path(&batch.path);
            self.stats.paths += 1;
        }
        batch.path.truncate(0);
        batch.segments = 0;
    }

    /// The links, at a fixed size on screen, over the lines.
    fn draw_links(&mut self, ctx: &mut RenderContext, p: &LinePicture, to_device: Affine, w: u16, h: u16, style: &LineStyle) {
        ctx.set_transform(Affine::IDENTITY);
        let pt = style.px_per_pt.max(f64::MIN_POSITIVE);
        let screen = Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
        for k in p.links() {
            if !style.shows(k.tag) || !style.counts(k.number) {
                continue;
            }
            let (a, b) = (to_device * p.unoffset(k.from), to_device * p.unoffset(k.to));
            if !on_screen(a, b, screen) || a.distance(b) < LINK_MIN_PT * pt {
                continue;
            }
            let Some(line) = clipped(a, b, screen.inflate(LINK_DASH_PT * pt, LINK_DASH_PT * pt)) else { continue };
            let line = kurbo::Line::new(line.0, line.1);
            match k.kind {
                LinkKind::Thread => {
                    let color = style.color(k.group, style.fades(k.tag));
                    let halo = if contrast(color, style.dark) >= contrast(color, style.light) { style.dark } else { style.light };
                    stroke_line(ctx, line, halo, (LINK_PT + 2.0 * LINK_HALO_PT) * pt, None);
                    stroke_line(ctx, line, color, LINK_PT * pt, None);
                }
                LinkKind::Cut => {
                    stroke_line(ctx, line, style.dark, LINK_PT * pt, None);
                    stroke_line(ctx, line, style.light, LINK_PT * pt, Some(LINK_DASH_PT * pt));
                }
            }
            self.stats.links += 1;
        }
    }

    /// The marks, at a fixed size on screen, over everything.
    fn draw_marks(&mut self, ctx: &mut RenderContext, p: &LinePicture, to_device: Affine, w: u16, h: u16, style: &LineStyle) {
        ctx.set_transform(Affine::IDENTITY);
        let pt = style.px_per_pt.max(f64::MIN_POSITIVE);
        let r = MARK_PT * pt;
        let screen = Rect::new(-r, -r, f64::from(w) + r, f64::from(h) + r);
        for m in p.marks() {
            if !style.shows(m.tag) || !style.counts(m.number) {
                continue;
            }
            let c = to_device * p.unoffset(m.at);
            if !(c.x.is_finite() && c.y.is_finite() && screen.contains(c)) {
                continue;
            }
            let mut path = BezPath::new();
            match m.kind {
                MarkKind::Cross => {
                    path.move_to((c.x - r, c.y - r));
                    path.line_to((c.x + r, c.y + r));
                    path.move_to((c.x - r, c.y + r));
                    path.line_to((c.x + r, c.y - r));
                }
                MarkKind::Square => path.extend(kurbo::Rect::new(c.x - r, c.y - r, c.x + r, c.y + r).path_elements(0.1)),
            }
            for (color, width) in [(style.light, MARK_HALO_PT * pt), (style.dark, MARK_LINE_PT * pt)] {
                ctx.set_paint(peniko::Color::from_rgb8(color[0], color[1], color[2]));
                ctx.set_stroke(kurbo::Stroke::new(width));
                ctx.stroke_path(&path);
            }
            self.stats.marks += 1;
        }
    }
}

/// How long the segment a lone point is drawn with is, a share of the line's width: a stroke of no
/// length draws nothing, one this short draws a dot as wide as the line (its round ends).
const DOT: f64 = 1e-3;

/// Add `line`'s segments within the style's numbers to `path` (a lone point as a dot, the line
/// `width` picture units wide); how many segments were added.
fn add_line(path: &mut BezPath, p: &LinePicture, line: &Line, style: &LineStyle, width: f64) -> usize {
    let r = line.points.start as usize..line.points.end as usize;
    let (Some(points), Some(numbers)) = (p.points.get(r.clone()), p.numbers.get(r)) else { return 0 };
    let at = |q: &[f32; 2]| (f64::from(q[0]), f64::from(q[1]));
    if let ([only], [n]) = (points, numbers) {
        if style.counts(*n) {
            let (x, y) = at(only);
            path.move_to((x, y));
            path.line_to((x + width * DOT, y));
            return 1;
        }
        return 0;
    }
    let mut added = 0;
    let mut pen_down = false;
    // The whole picture leaves out the points its lines pass straight through; a range keeps them.
    let skip_straight = style.numbers.is_none();
    let last = points.len().saturating_sub(1);
    // The point before, of those kept.
    let mut before: Option<&[f32; 2]> = None;
    for (k, (q, n)) in points.iter().zip(numbers).enumerate() {
        if skip_straight && k > 0 && k < last && p.is_straight(line.points.start as usize + k) {
            continue;
        }
        // A segment is drawn when its end point's number is.
        match before.filter(|_| style.counts(*n)) {
            Some(b) => {
                if !pen_down {
                    path.move_to(at(b));
                }
                path.line_to(at(q));
                pen_down = true;
                added += 1;
            }
            None => pen_down = false,
        }
        before = Some(q);
    }
    added
}

/// Whether segment `a`–`b` (device pixels) may touch `screen`.
fn on_screen(a: Point, b: Point, screen: Rect) -> bool {
    let finite = [a.x, a.y, b.x, b.y].iter().all(|v| v.is_finite() && v.abs() <= MAX_DEVICE);
    finite && !(a.x.max(b.x) < screen.x0 || a.x.min(b.x) > screen.x1 || a.y.max(b.y) < screen.y0 || a.y.min(b.y) > screen.y1)
}

/// Segment `a`–`b` cut to `r` (Liang–Barsky), so a long move far off screen isn't dashed whole.
fn clipped(a: Point, b: Point, r: Rect) -> Option<(Point, Point)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.x - r.x0), (dx, r.x1 - a.x), (-dy, a.y - r.y0), (dy, r.y1 - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then(|| (a + (b - a) * t0, a + (b - a) * t1))
}

/// Stroke `line` in `color`, `width` device pixels wide, dashed `dash` pixels on and off when given.
fn stroke_line(ctx: &mut RenderContext, line: kurbo::Line, color: [u8; 3], width: f64, dash: Option<f64>) {
    ctx.set_paint(peniko::Color::from_rgb8(color[0], color[1], color[2]));
    let stroke = kurbo::Stroke::new(width).with_caps(kurbo::Cap::Butt);
    ctx.set_stroke(match dash {
        Some(d) => stroke.with_dashes(0.0, [d, d]),
        None => stroke,
    });
    ctx.stroke_path(&line.to_path(0.1));
}

#[cfg(test)]
mod tests;
