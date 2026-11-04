//! What the tests of digitizing from an image share: the art drawn as a picture over its box, a
//! picture placed over that same box, and how a design's stitches cover the art, its silhouette
//! and its colours, on the grid of 10 pixels a millimetre the corpus's checks use.

use std::collections::HashMap;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::color::delta_e_rgb;
use vectorcraft_stitch::{Command, Pattern};

use super::run;

/// The grid the measures are taken on (pixels a millimetre): the formats' tenth of a millimetre,
/// the resolution digitizing aims for. As `ppi`, the export's unit: 254.
pub const PX_PER_MM: f64 = 10.0;
const MM_PER_INCH: f64 = 25.4;
const PT_PER_MM: f64 = 72.0 / 25.4;

/// The margin round the art the picture is drawn with (millimetres): room for its background.
pub const MARGIN_MM: f64 = 5.0;

/// The grid the picture's box is snapped to (millimetres): the formats' tenth, so that its pixels
/// fall on whole tenths.
const SNAP_MM: f64 = 0.1;

/// Half the width a needle-down stitch covers (millimetres): half the 0.40 mm at which 40 wt
/// threads touch.
pub const HALF_THREAD_MM: f64 = 0.2;

/// How near a colour of the art a pixel must be to count as flat in the colour agreement (CIEDE2000):
/// the art's anti-aliased edges are left out.
pub const FLAT_DELTA_E: f64 = 2.0;

/// An alpha past half: a pixel of the art.
const HALF_ALPHA: u8 = 127;

/// How far inside the art's silhouette a pixel must lie to count where fabric would show
/// (millimetres): half a millimetre, so that the silhouette's own edge, where a fill's rows end
/// and anti-aliasing paints, is left out and what is measured is the art's inside, between its
/// colours.
pub const INSIDE_MM: f64 = 0.5;

/// How near two of the art's colours a pixel must lie to be between them, where the overlap is for
/// (millimetres): 0.3 mm, so that the seam's anti-aliased pixels and a thread's width either side
/// of it count.
pub const SEAM_MM: f64 = 0.3;

/// A box in millimetres from the document's origin, y down.
#[derive(Clone, Copy, Debug)]
pub struct Box {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Box {
    /// As `[x, y, width, height]` in points, as `document.export` and `file.place` take a rectangle.
    pub fn points(self) -> Value {
        json!([self.left * PT_PER_MM, self.top * PT_PER_MM, (self.right - self.left) * PT_PER_MM, (self.bottom - self.top) * PT_PER_MM])
    }
}

/// The art of `s` (every object) drawn as a PNG at [`PX_PER_MM`] over its drawn box and
/// [`MARGIN_MM`] round it, the box on the 0.1 mm grid: the box, and the PNG.
pub fn art_picture(s: &mut Session) -> (Box, Vec<u8>) {
    art_picture_at(s, PX_PER_MM)
}

/// [`art_picture`] drawn at `px_per_mm` pixels a millimetre.
pub fn art_picture_at(s: &mut Session, px_per_mm: f64) -> (Box, Vec<u8>) {
    run(s, "select.all", json!({}));
    let b = run(s, "document.exportSelection", json!({"format": "svg"}))["bounds"].clone();
    run(s, "select.none", json!({}));
    let mm = |i: usize| b[i].as_f64().unwrap() / PT_PER_MM;
    let (x, y, w, h) = (mm(0), mm(1), mm(2), mm(3));
    let down = |v: f64| (v / SNAP_MM).floor() * SNAP_MM;
    let up = |v: f64| (v / SNAP_MM).ceil() * SNAP_MM;
    let r = Box { left: down(x - MARGIN_MM), top: down(y - MARGIN_MM), right: up(x + w + MARGIN_MM), bottom: up(y + h + MARGIN_MM) };
    let png = run(s, "document.export", json!({"format": "png", "ppi": px_per_mm * MM_PER_INCH, "rect": r.points()}));
    (r, vectorcraft_format::base64_decode(png["dataBase64"].as_str().unwrap()).unwrap())
}

/// A session holding picture `bytes` (a file named `name`) placed over `r`, selected, in a
/// document large enough to hold it.
pub fn placed_over(name: &str, bytes: &[u8], r: Box) -> Session {
    let mut s = Session::new();
    let side = |v: f64| (v + MARGIN_MM) * PT_PER_MM;
    run(&mut s, "file.new", json!({"width": side(r.right), "height": side(r.bottom), "artboards": 1}));
    run(&mut s, "file.place", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes), "rect": r.points()}));
    run(&mut s, "select.all", json!({}));
    s
}

/// The art as a picture over its box: its pixels at [`PX_PER_MM`] and its own colours.
pub struct Art {
    pub width: usize,
    pub height: usize,
    /// Its top-left corner (millimetres).
    pub left: f64,
    pub top: f64,
    rgba: Vec<u8>,
    colours: Vec<[u8; 3]>,
    /// Each colour seen, with the art's colour nearest it and how far.
    nearest: HashMap<[u8; 3], (usize, f64)>,
}

impl Art {
    /// The art drawn as PNG `png` over `r`, in its own `colours`.
    pub fn of(png: &[u8], r: Box, colours: Vec<[u8; 3]>) -> Self {
        let img = image::load_from_memory(png).unwrap().to_rgba8();
        let (width, height) = (img.width() as usize, img.height() as usize);
        Self { width, height, left: r.left, top: r.top, rgba: img.into_raw(), colours, nearest: HashMap::new() }
    }

    /// Whether pixel `i` is the art's.
    pub fn painted(&self, i: usize) -> bool {
        self.rgba.get(i * 4 + 3).is_some_and(|&a| a > HALF_ALPHA)
    }

    fn rgb(&self, i: usize) -> [u8; 3] {
        [self.rgba[i * 4], self.rgba[i * 4 + 1], self.rgba[i * 4 + 2]]
    }

    /// The art's colour nearest `c`, and how far it is (CIEDE2000).
    fn near(&mut self, c: [u8; 3]) -> (usize, f64) {
        if let Some(&v) = self.nearest.get(&c) {
            return v;
        }
        let v = self
            .colours
            .iter()
            .enumerate()
            .map(|(k, &o)| (k, delta_e_rgb(c, o)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((usize::MAX, f64::INFINITY));
        self.nearest.insert(c, v);
        v
    }
}

/// Each pixel's top-most block of `p` (`None`: no stitch), every needle-down stitch drawn
/// [`HALF_THREAD_MM`] wide on each side, the pattern moved by `(dx, dy)` millimetres; and which
/// blocks' colours cover it, as bits of `colour_of`'s numbers (the art's colours, at most 64).
fn covered(p: &Pattern, art: &Art, dx: f64, dy: f64, colour_of: &[usize]) -> (Vec<Option<usize>>, Vec<u64>) {
    let (w, h) = (art.width as i64, art.height as i64);
    let mut top = vec![None; art.width * art.height];
    let mut colours = vec![0u64; art.width * art.height];
    let r = HALF_THREAD_MM * PX_PER_MM;
    for (k, b) in p.blocks.iter().enumerate() {
        for pair in b.stitches.windows(2) {
            if pair[0].command != Command::Stitch || pair[1].command != Command::Stitch {
                continue;
            }
            let at = |s: &vectorcraft_stitch::Stitch| ((s.x + dx - art.left) * PX_PER_MM, (s.y + dy - art.top) * PX_PER_MM);
            let ((ax, ay), (bx, by)) = (at(&pair[0]), at(&pair[1]));
            let (x0, x1) = (((ax.min(bx) - r).floor() as i64).max(0), ((ax.max(bx) + r).ceil() as i64).min(w - 1));
            let (y0, y1) = (((ay.min(by) - r).floor() as i64).max(0), ((ay.max(by) + r).ceil() as i64).min(h - 1));
            let (vx, vy) = (bx - ax, by - ay);
            let l2 = vx * vx + vy * vy;
            for py in y0..=y1 {
                for px in x0..=x1 {
                    let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
                    let t = if l2 > 0.0 { (((cx - ax) * vx + (cy - ay) * vy) / l2).clamp(0.0, 1.0) } else { 0.0 };
                    let (qx, qy) = (ax + t * vx - cx, ay + t * vy - cy);
                    if qx * qx + qy * qy <= r * r {
                        top[(py * w + px) as usize] = Some(k);
                        colours[(py * w + px) as usize] |= colour_of.get(k).map_or(0, |&c| 1u64 << c.min(63));
                    }
                }
            }
        }
    }
    (top, colours)
}

/// How a design's stitches cover the art.
#[derive(Clone, Copy, Debug)]
pub struct Cover {
    /// The silhouette's intersection over union: the art's pixels against those its stitches cover.
    pub iou: f64,
    /// The share of the art's flat pixels (within [`FLAT_DELTA_E`] of one of its colours) whose
    /// top-most stitch is in the thread asked for that colour.
    pub agreement: f64,
    /// Where fabric shows (square millimetres): the art's pixels at least [`INSIDE_MM`] inside its
    /// silhouette that no stitch reaches, as the pre-code measure took it, and of them those
    /// between two of its colours (within [`SEAM_MM`] of flat pixels of both), where the overlap
    /// is for; the rest lie between a fill's own rows.
    pub fabric_mm2: f64,
    pub seam_fabric_mm2: f64,
    /// The share of the art's pixels between two of its colours (inside, as above) that stitches of
    /// two or more of its colours cover: sewn twice where colours meet, as the overlap is for.
    pub seams_doubled: f64,
}

/// Which of `mask`'s pixels lie within `reach` pixels of one of its own, in a square: dilated along
/// the rows and then the columns.
fn dilated(mask: &[bool], w: usize, h: usize, reach: usize) -> Vec<bool> {
    let along = |m: &[bool], step: usize, len: usize, lines: usize, across: usize| {
        let mut out = vec![false; m.len()];
        for line in 0..lines {
            let at = |k: usize| line * across + k * step;
            // The distance back to the nearest set pixel, then forward.
            let mut last: Option<usize> = None;
            for k in 0..len {
                if m[at(k)] {
                    last = Some(k);
                }
                out[at(k)] = last.is_some_and(|l| k - l <= reach);
            }
            let mut next: Option<usize> = None;
            for k in (0..len).rev() {
                if m[at(k)] {
                    next = Some(k);
                }
                out[at(k)] |= next.is_some_and(|n| n - k <= reach);
            }
        }
        out
    };
    let rows = along(mask, 1, w, h, w);
    along(&rows, w, h, w, 1)
}

/// Which of the art's pixels lie between two of its colours: within [`SEAM_MM`] of flat pixels of
/// two of them.
fn seams(art: &mut Art) -> Vec<bool> {
    let (w, h) = (art.width, art.height);
    let reach = (SEAM_MM * PX_PER_MM).round() as usize;
    let flat: Vec<Option<usize>> = (0..w * h)
        .map(|i| {
            if !art.painted(i) {
                return None;
            }
            let (k, de) = art.near(art.rgb(i));
            (de <= FLAT_DELTA_E).then_some(k)
        })
        .collect();
    let mut near = vec![0u8; w * h];
    for k in 0..art.colours.len() {
        let mask: Vec<bool> = flat.iter().map(|&f| f == Some(k)).collect();
        for (n, d) in near.iter_mut().zip(dilated(&mask, w, h, reach)) {
            *n = n.saturating_add(u8::from(d));
        }
    }
    near.iter().map(|&n| n >= 2).collect()
}

/// Which of the art's pixels lie at least [`INSIDE_MM`] inside its silhouette: those whose square
/// of that half-width round them is all the art's, eroded along the rows and then the columns.
fn inside(art: &Art) -> Vec<bool> {
    let reach = (INSIDE_MM * PX_PER_MM).round() as usize;
    let (w, h) = (art.width, art.height);
    let erode = |mask: &[bool], step: usize, len: usize, lines: usize, across: usize| {
        let mut out = vec![false; mask.len()];
        for line in 0..lines {
            // The run of the art's pixels ending at each place along the line, both ways.
            let at = |k: usize| line * across + k * step;
            let mut run = vec![0usize; len];
            let mut n = 0;
            for k in 0..len {
                n = if mask[at(k)] { n + 1 } else { 0 };
                run[k] = n;
            }
            let mut m = 0;
            for k in (0..len).rev() {
                m = if mask[at(k)] { m + 1 } else { 0 };
                out[at(k)] = run[k] > reach && m > reach;
            }
        }
        out
    };
    let painted: Vec<bool> = (0..w * h).map(|i| art.painted(i)).collect();
    let rows = erode(&painted, 1, w, h, w);
    erode(&rows, w, h, w, 1)
}

/// The thread a block of a planned design is sewn in: its number in the palette, as the plan chose
/// it (`None`: a design read from a file, whose threads are its colours).
pub fn thread_of(b: &vectorcraft_stitch::Block) -> Option<usize> {
    b.thread.chosen.map(|t| t.index())
}

/// What a block of a design stands for, to the colour agreement: the art's colour nearest the
/// block's own, or the palette thread it is sewn in.
#[derive(Clone, Copy, Debug)]
pub enum Sewn<'a> {
    /// The art's colours: each block's colour stands for the art's colour nearest it.
    Colour,
    /// The threads: each of the art's colours asks for the thread `threads` gives it (by its place
    /// in the art's colours), and a block stands for the thread it is sewn in.
    Thread(&'a [Option<usize>]),
}

/// How `p`, moved by `(dx, dy)` millimetres, covers `art`, its blocks standing for what `sewn`
/// says.
pub fn cover(p: &Pattern, art: &mut Art, dx: f64, dy: f64, sewn: Sewn) -> Cover {
    let colour_of: Vec<usize> = p.blocks.iter().map(|b| art.near(b.thread.color).0).collect();
    let (top, colours) = covered(p, art, dx, dy, &colour_of);
    // What each block stands for, and what each of the art's colours asks for, as one number.
    let (block, asked): (Vec<Option<usize>>, Vec<Option<usize>>) = match sewn {
        Sewn::Colour => (p.blocks.iter().map(|b| Some(art.near(b.thread.color).0)).collect(), (0..art.colours.len()).map(Some).collect()),
        Sewn::Thread(threads) => (p.blocks.iter().map(thread_of).collect(), threads.to_vec()),
    };
    let (mut both, mut either, mut flat, mut agree) = (0usize, 0usize, 0usize, 0usize);
    for (i, &t) in top.iter().enumerate() {
        let painted = art.painted(i);
        both += usize::from(painted && t.is_some());
        either += usize::from(painted || t.is_some());
        if painted {
            let (k, de) = art.near(art.rgb(i));
            if de <= FLAT_DELTA_E {
                flat += 1;
                let wanted = asked.get(k).copied().flatten();
                agree += usize::from(wanted.is_some() && t.and_then(|b| block.get(b).copied().flatten()) == wanted);
            }
        }
    }
    let (inside, seams) = (inside(art), seams(art));
    let bare: Vec<bool> = inside.iter().zip(&top).map(|(inside, t)| *inside && t.is_none()).collect();
    let mm2 = |n: usize| n as f64 / (PX_PER_MM * PX_PER_MM);
    let seam_inside: Vec<bool> = inside.iter().zip(&seams).map(|(i, s)| *i && *s).collect();
    let doubled = seam_inside.iter().zip(&colours).filter(|(s, c)| **s && c.count_ones() >= 2).count();
    Cover {
        iou: both as f64 / either.max(1) as f64,
        agreement: agree as f64 / flat.max(1) as f64,
        fabric_mm2: mm2(bare.iter().filter(|b| **b).count()),
        seam_fabric_mm2: mm2(bare.iter().zip(&seams).filter(|(b, s)| **b && **s).count()),
        seams_doubled: doubled as f64 / seam_inside.iter().filter(|s| **s).count().max(1) as f64,
    }
}

/// The middle of the art's painted box (millimetres).
pub fn painted_centre(art: &Art) -> (f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for i in (0..art.width * art.height).filter(|&i| art.painted(i)) {
        let (x, y) = (i % art.width, i / art.width);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    (art.left + (x0 + x1 + 1) as f64 / 2.0 / PX_PER_MM, art.top + (y0 + y1 + 1) as f64 / 2.0 / PX_PER_MM)
}
