//! The canvas in the stitch view: the stitches drawn as a layer of their own, over the fabric.
//!
//! - **What is drawn, bottom to top** ([`vectorcraft_engine::cmd::embroidery::view::layers`]): the
//!   fabric (white) on the artboards and under the stitches' box; the art faint, when Show Artwork
//!   Under Stitches is on, else the template layers alone; the stitches; the art's outlines, with
//!   View › Outline on. Guides, the grid and the selection are drawn by the canvas as always.
//! - **A raster and a texture of their own**, keyed by the document, the plan's generation, the
//!   view, the size and the look ([`LayerKey`]), never by the document's revision: a selection
//!   change, a hover or a guide moved draws the stitches again from the texture, uploading nothing.
//!   The art's texture is the canvas's own, rendered only when one of its passes is shown
//!   ([`ArtPass`]).
//! - **Off the interface thread when slow.** Light frames are drawn in line, as the canvas draws
//!   light art; slower ones on a thread of their own ([`LineWorker`]), the last frame shown
//!   meanwhile, moved into the new view. A document switch never draws the stitches in line where
//!   that thread runs: the fabric shows until they come, never another document's stitches.
//! - **Until the first plan comes** the art stays, so turning the preview on never flashes an
//!   empty canvas. A design that fails keeps its last good stitches, faded.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::{Color32, Pos2, Shape, pos2};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::cmd::embroidery::scene::{FABRIC, StitchScene};
use vectorcraft_engine::cmd::embroidery::view::{Layers, layers, paused};
use vectorcraft_geom::{Affine, Point, Rect};
use vectorcraft_render::Rendered;
use vectorcraft_render::lines::{LinePicture, LineRenderer, LineStyle, TagSet};

use super::stitch_preview::Freshness;
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::state::View;

/// A frame of stitches that took longer than this to draw, milliseconds, is drawn on the stitch
/// layer's thread from then on: the time the canvas lets its art render in line.
pub const INLINE_DRAW_MS: f64 = 8.0;

/// How much of the colour a faded picture keeps (the art under the stitches, the last good
/// stitches of a design that fails): half, as isolation mode dims the art around it.
pub const FADED_ALPHA: u8 = 128;

/// What the canvas's art does this frame: drawn as always, or one of the stitch view's passes.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtPass {
    /// The stitch view is drawn: the canvas lays the fabric and leaves its art to
    /// [`draw`].
    pub stitches: bool,
    /// The art's texture is wanted: drawn as always, faint under the stitches, its templates
    /// alone, or its outlines over them.
    pub render: bool,
    /// Objects the art leaves out ([`vectorcraft_render::RenderOptions::hidden`]): everything but
    /// the template layers, when only those show under the stitches.
    pub hidden: Vec<u64>,
    /// Template layers left out: under View › Outline their images would cover the stitches.
    pub hide_templates: bool,
}

impl ArtPass {
    fn art() -> Self {
        Self { stitches: false, render: true, hidden: Vec::new(), hide_templates: false }
    }
}

/// What a frame of stitches is of: when one of these changes, they are drawn again.
#[derive(Clone, Debug, PartialEq)]
struct LayerKey {
    uid: u64,
    generation: u64,
    zoom: f64,
    cx: f64,
    cy: f64,
    rot: f64,
    w: u32,
    h: u32,
    ppp: f32,
    jumps: bool,
    needle_points: bool,
    /// Isolation mode's group or layer: the stitches of the rest are faded.
    isolated: Option<u64>,
    anti_alias: bool,
}

/// A frame of stitches to draw on the thread, numbered in the order frames are made.
struct LayerJob {
    seq: u64,
    key: LayerKey,
    picture: Arc<LinePicture>,
    view: Affine,
    style: LineStyle,
}

/// A frame drawn on the thread, and how long it took.
struct LayerDone {
    seq: u64,
    key: LayerKey,
    img: Rendered,
    ms: f64,
}

/// The stitch layer's thread: it draws the newest frame asked for and drops the ones it replaces.
pub(crate) struct LineWorker {
    tx: Sender<LayerJob>,
    rx: Receiver<LayerDone>,
}

impl LineWorker {
    /// Start the thread, `None` where the stitches are drawn in line: on wasm, in unit tests, with
    /// `VECTORCRAFT_SYNC_RENDER` set, or if the thread can't start.
    fn spawn(ctx: &egui::Context) -> Option<Self> {
        if cfg!(target_arch = "wasm32") || cfg!(test) || std::env::var_os("VECTORCRAFT_SYNC_RENDER").is_some() {
            return None;
        }
        Self::start(ctx)
    }

    /// Start the thread.
    pub(crate) fn start(ctx: &egui::Context) -> Option<Self> {
        let (tx, jobs) = channel::<LayerJob>();
        let (done_tx, rx) = channel::<LayerDone>();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("vectorcraft-stitches".into())
            .spawn(move || {
                let mut r = LineRenderer::new();
                // The canvas is gone (its sender dropped): end the thread.
                while let Ok(mut job) = jobs.recv() {
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    let t0 = crate::now_ms();
                    // A panic in the drawing ends the thread (the guard logs it): the canvas finds the
                    // thread gone and draws in line from then on.
                    let Ok(img) = vectorcraft_engine::guard::catch_panic(|| r.render(&job.picture, job.key.w, job.key.h, job.view, &job.style))
                    else {
                        break;
                    };
                    if done_tx.send(LayerDone { seq: job.seq, key: job.key, img, ms: crate::now_ms() - t0 }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Self { tx, rx })
    }

    fn submit(&self, job: LayerJob) -> bool {
        self.tx.send(job).is_ok()
    }

    /// The newest frame finished since the last call; `Err` when the thread is gone.
    fn poll(&self) -> Result<Option<LayerDone>, ()> {
        let mut newest = None;
        loop {
            match self.rx.try_recv() {
                Ok(d) => newest = Some(d),
                Err(TryRecvError::Empty) => return Ok(newest),
                Err(TryRecvError::Disconnected) => return Err(()),
            }
        }
    }
}

/// The stitch layer's state: its texture and what it shows, its renderer and its thread.
#[derive(Default)]
pub struct StitchLayer {
    texture: Option<egui::TextureHandle>,
    key: Option<LayerKey>,
    /// The frame asked of the thread and not back yet (and its number): not asked again. Forgotten
    /// when the stitch view stops being drawn, or the thread is gone.
    asked: Option<(LayerKey, u64)>,
    /// The number of the last frame made (drawn in line or asked of the thread), and of the frame
    /// shown: a frame the thread finishes is shown only when it is newer than the one shown.
    seq: u64,
    shown_seq: u64,
    renderer: LineRenderer,
    worker: Option<LineWorker>,
    worker_started: bool,
    last_ms: f64,
    /// The units faded outside isolation mode's group, for (generation, isolated).
    faded: Option<((u64, u64), Arc<TagSet>)>,
    /// The last frame drew the stitch view.
    shown: bool,
    /// The units emphasised this frame ([`StitchLayer::emphasize`]), and the raster of them alone,
    /// for (the frame it goes over, the units).
    emphasis: Option<Vec<u32>>,
    emphasized: Option<((LayerKey, Vec<u32>), egui::TextureHandle)>,
    /// Frames of stitches uploaded so far (the tests count them).
    pub(crate) uploads: u64,
}

impl StitchLayer {
    /// Emphasise `units` (tags of the scene shown) in this frame's stitch view: the rest are drawn
    /// faded, by a tint of the frame already drawn, and these over them from a raster of their own
    /// (a Sewing order row hovered, a check's hits shown). The project owner's choice: only for a
    /// moment, never for the selection. Asked again every frame it lasts.
    pub fn emphasize(&mut self, units: Vec<u32>) {
        self.emphasis = Some(units);
    }

    /// The units asked to stand out in this frame, before it draws.
    #[cfg(test)]
    pub(crate) fn emphasis(&self) -> Option<&[u32]> {
        self.emphasis.as_deref()
    }

    /// Whether the last frame drew the stitch view.
    pub fn drawn(&self) -> bool {
        self.shown
    }

    /// Whether a frame of stitches asked of the layer's thread hasn't come back yet.
    pub fn waiting(&self) -> bool {
        self.shown && self.asked.is_some()
    }

    /// The generation of the plan whose stitches the texture holds.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> Option<u64> {
        self.key.as_ref().map(|k| k.generation)
    }

    /// Draw on `worker` from now on (the tests start the thread themselves).
    #[cfg(test)]
    pub(crate) fn use_worker(&mut self, worker: LineWorker) {
        self.worker = Some(worker);
        self.worker_started = true;
    }
}

/// The stitch view's layers for the active document, as the canvas draws them this frame.
fn stitch_layers(app: &VectorcraftApp) -> Option<(u64, Layers)> {
    let st = app.session.active()?;
    let view = app.session.stitch_view_of(st.uid);
    Some((st.uid, layers(view, paused(&st.doc), app.ui.view.outline)))
}

/// What the canvas's art does this frame, asked by the canvas before it renders its art. While the
/// stitch view is on it asks the preview for the plan; leaving it, the art's texture is dropped so
/// the art is rendered again before it shows (it may be of an earlier state, or templates alone).
pub fn art_pass(app: &mut VectorcraftApp) -> ArtPass {
    let pass = match stitch_layers(app) {
        Some((uid, l)) if l.stitches => {
            app.embroidery.preview.want_plan();
            if app.embroidery.preview.shown(uid).is_none() && !app.embroidery.preview.failed(uid) {
                // The first plan is on its way: the art stays meanwhile.
                ArtPass::art()
            } else if l.outlines {
                ArtPass { stitches: true, render: true, hidden: Vec::new(), hide_templates: true }
            } else if l.faint_art {
                ArtPass { stitches: true, render: true, hidden: Vec::new(), hide_templates: false }
            } else {
                let hidden = app.session.active().map(|st| all_but_templates(&st.doc)).unwrap_or_default();
                let render = app.ui.view.templates && hidden.as_ref().is_some();
                ArtPass { stitches: true, render, hidden: hidden.unwrap_or_default(), hide_templates: false }
            }
        }
        _ => ArtPass::art(),
    };
    let layer = &mut app.embroidery.layer;
    if layer.shown && !pass.stitches {
        app.canvas.texture = None;
    }
    // The stitch view no longer drawn: the frame asked of the thread is wanted no more.
    if !pass.stitches {
        layer.asked = None;
    }
    layer.shown = pass.stitches;
    pass
}

/// The objects to leave out so that only `doc`'s template layers are drawn: every top-level layer
/// with no template in it, and inside a layer that holds a template sublayer, the rest of it.
/// `None` when the document has no template layer (nothing of the art shows).
fn all_but_templates(doc: &Document) -> Option<Vec<u64>> {
    fn holds_template(n: &vectorcraft_doc::Node) -> bool {
        n.is_template() || n.children().is_some_and(|c| c.iter().any(|c| holds_template(c)))
    }
    fn hide(n: &vectorcraft_doc::Node, out: &mut Vec<u64>) {
        if n.is_template() {
            return;
        }
        match n.children() {
            Some(children) if holds_template(n) => children.iter().for_each(|c| hide(c, out)),
            _ => out.push(n.id.0),
        }
    }
    if !doc.layers.iter().any(|l| holds_template(l)) {
        return None;
    }
    let mut out = Vec::new();
    for l in &doc.layers {
        hide(l, &mut out);
    }
    Some(out)
}

/// Lay the fabric for the stitch view, in place of the paper: on the artboards (when they are
/// shown) and under the stitches' box (where they reach past the artboards) → whether it did (the
/// canvas then lays no paper).
pub fn paint_fabric(app: &VectorcraftApp, painter: &egui::Painter, xf: &Xf, doc: &Document) -> bool {
    if !app.embroidery.layer.shown {
        return false;
    }
    let fabric = Color32::from_rgb(FABRIC[0], FABRIC[1], FABRIC[2]);
    if app.ui.view.artboards {
        for ab in &doc.artboards {
            painter.add(Shape::convex_polygon(xf.quad(ab.rect), fabric, egui::Stroke::NONE));
        }
    }
    let uid = app.session.active().map(|st| st.uid);
    if let Some(design) = uid.and_then(|uid| app.embroidery.preview.shown(uid)).and_then(|s| s.planned.scene().bounds()) {
        painter.add(Shape::convex_polygon(xf.quad(design), fabric, egui::Stroke::NONE));
    }
    true
}

/// Draw the stitch view, after the fabric, where the canvas draws its art: the art faint or its
/// templates under the stitches, the stitches, the art's outlines over them.
pub fn draw(app: &mut VectorcraftApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf, v: &View, pass: &ArtPass) {
    // Asked for this frame only.
    let emphasis = app.embroidery.layer.emphasis.take();
    if !pass.stitches {
        return;
    }
    let outline = app.ui.view.outline;
    let faint = stitch_layers(app).is_some_and(|(_, l)| l.faint_art);
    if pass.render && !outline {
        draw_art(app, painter, xf, if faint { faded() } else { Color32::WHITE });
    }
    draw_stitches(app, ctx, painter, xf, v, emphasis);
    if pass.render && outline {
        draw_art(app, painter, xf, Color32::WHITE);
    }
}

/// A tint that keeps [`FADED_ALPHA`] of a picture's colour, over what lies under it.
fn faded() -> Color32 {
    Color32::from_white_alpha(FADED_ALPHA)
}

/// The canvas's art texture, as the canvas places it (moved into this view when it was rendered
/// for another), tinted `tint`.
fn draw_art(app: &VectorcraftApp, painter: &egui::Painter, xf: &Xf, tint: Color32) {
    let Some(uid) = app.session.active().map(|st| st.uid) else { return };
    let (Some(tex), Some(k)) = (&app.canvas.texture, &app.canvas.key) else { return };
    if k.doc != uid as usize {
        return;
    }
    let quad = match k.pixel {
        // Document pixels: over the document rect they cover.
        Some([x0, y0, x1, y1]) => xf.quad(Rect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64)),
        None => rendered_quad(xf, k.zoom, Point::new(k.cx, k.cy), k.rot),
    };
    textured_quad(painter, tex.id(), &quad, tint);
}

/// Where the corners of a frame rendered for a view at `zoom`, `center` and `rot` degrees go in
/// this view: the frame moved, scaled and turned into it until a new one comes.
fn rendered_quad(xf: &Xf, zoom: f64, center: Point, rot: f64) -> Vec<Pos2> {
    let old = Xf { rect: xf.rect, zoom, center, rot: rot.to_radians() };
    let r = xf.rect;
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].iter().map(|c| xf.to_screen(old.to_doc(*c))).collect()
}

/// One quad drawn with `tex` whole, its corners `quad` from the top left, clockwise.
fn textured_quad(p: &egui::Painter, tex: egui::TextureId, quad: &[Pos2], tint: Color32) {
    let mut mesh = egui::Mesh::with_texture(tex);
    for (pos, uv) in quad.iter().zip([pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: *pos, uv, color: tint });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(Shape::mesh(mesh));
}

/// The units of `scene` outside isolation mode's group or layer `isolated` in `doc`, faded.
fn outside(scene: &StitchScene, doc: &Document, isolated: NodeId) -> TagSet {
    let mut inside = std::collections::HashSet::new();
    if let Some(n) = doc.node(isolated) {
        n.walk(&mut |c| {
            inside.insert(c.id);
        });
    }
    TagSet::of(scene.units().iter().enumerate().filter(|(_, u)| !inside.contains(&u.object)).filter_map(|(i, _)| u32::try_from(i).ok()))
}

/// Draw the active document's stitches: their frame for this view drawn now, or on the thread
/// when slow (the last frame shown meanwhile, moved into this view); with `emphasis`, the rest faded
/// and those units over them.
fn draw_stitches(app: &mut VectorcraftApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf, v: &View, emphasis: Option<Vec<u32>>) {
    let Some(st) = app.session.active() else { return };
    let (uid, isolated, doc) = (st.uid, st.isolation, st.doc.clone());
    let view = app.session.stitch_view_of(uid);
    let anti_alias = app.session.prefs.anti_aliased_artwork;
    let now = ctx.input(|i| i.time);
    let failed = matches!(app.embroidery.preview.freshness(uid, now), Freshness::Failed { .. });
    let Some(shown) = app.embroidery.preview.shown(uid) else { return };
    let (scene, generation) = (shown.planned.scene(), shown.planned.generation);
    let layer = &mut app.embroidery.layer;
    if !layer.worker_started {
        layer.worker_started = true;
        layer.worker = LineWorker::spawn(ctx);
    }
    let ppp = ctx.pixels_per_point();
    let (w, h) = ((xf.rect.width() * ppp).round().max(1.0) as u32, (xf.rect.height() * ppp).round().max(1.0) as u32);
    let key = LayerKey {
        uid,
        generation,
        zoom: v.zoom,
        cx: v.center.x,
        cy: v.center.y,
        rot: v.rotation,
        w,
        h,
        ppp,
        jumps: view.jumps,
        needle_points: view.needle_points,
        isolated: isolated.map(|i| i.0),
        anti_alias,
    };
    // A frame the thread finished: shown when it is of this document and newer than the frame shown
    // (one drawn in line since may be).
    match layer.worker.as_ref().map(LineWorker::poll) {
        Some(Ok(Some(done))) => {
            if layer.asked.as_ref().is_some_and(|(_, seq)| *seq == done.seq) {
                layer.asked = None;
            }
            if done.key.uid == uid && done.seq > layer.shown_seq {
                layer.last_ms = done.ms;
                upload(layer, ctx, &done.img, done.key, done.seq);
            }
        }
        Some(Err(())) => {
            layer.worker = None;
            layer.asked = None;
        }
        _ => {}
    }
    if (layer.key.as_ref() != Some(&key) || layer.texture.is_none()) && layer.asked.as_ref().map(|(k, _)| k) != Some(&key) {
        let faded = isolated.map(|i| {
            let id = (generation, i.0);
            match &layer.faded {
                Some((k, set)) if *k == id => set.clone(),
                _ => {
                    let set = Arc::new(outside(&scene, &doc, i));
                    layer.faded = Some((id, set.clone()));
                    set
                }
            }
        });
        let style =
            LineStyle { faded: faded.map(|f| (*f).clone()), anti_alias, ..scene.style(FABRIC, view.jumps, view.needle_points, f64::from(ppp)) };
        let affine = Affine::translate((f64::from(w) / 2.0, f64::from(h) / 2.0))
            * Affine::rotate(v.rotation.to_radians())
            * Affine::scale(v.zoom * f64::from(ppp))
            * Affine::translate(-v.center.to_vec2());
        let switched = layer.key.as_ref().is_none_or(|k| k.uid != uid);
        let heavy = layer.last_ms > INLINE_DRAW_MS || switched;
        layer.seq += 1;
        let seq = layer.seq;
        let job = LayerJob { seq, key: key.clone(), picture: scene.picture().clone(), view: affine, style };
        match &layer.worker {
            Some(worker) if heavy => {
                if worker.submit(job) {
                    layer.asked = Some((key, seq));
                } else {
                    layer.worker = None;
                }
                // Never another document's stitches meanwhile.
                if switched {
                    layer.texture = None;
                    layer.key = None;
                }
            }
            _ => {
                let t0 = crate::now_ms();
                let img = layer.renderer.render(&job.picture, w, h, job.view, &job.style);
                layer.last_ms = crate::now_ms() - t0;
                // Newer than anything the thread is drawing: that frame is wanted no more.
                layer.asked = None;
                upload(layer, ctx, &img, key.clone(), seq);
            }
        }
    }
    let (Some(tex), Some(k)) = (&layer.texture, &layer.key) else { return };
    if k.uid != uid {
        return;
    }
    let quad = rendered_quad(xf, k.zoom, Point::new(k.cx, k.cy), k.rot);
    let tint = if failed || emphasis.is_some() { faded() } else { Color32::WHITE };
    textured_quad(painter, tex.id(), &quad, tint);
    if let Some(units) = emphasis {
        let k = k.clone();
        let wanted = (k.clone(), units);
        if layer.emphasized.as_ref().is_none_or(|(have, _)| *have != wanted) {
            let shown: std::collections::HashSet<u32> = wanted.1.iter().copied().collect();
            let rest = (0..scene.units().len()).filter_map(|i| u32::try_from(i).ok()).filter(|i| !shown.contains(i));
            let style =
                LineStyle { hidden: Some(TagSet::of(rest)), anti_alias, ..scene.style(FABRIC, view.jumps, view.needle_points, f64::from(k.ppp)) };
            let affine = Affine::translate((f64::from(k.w) / 2.0, f64::from(k.h) / 2.0))
                * Affine::rotate(k.rot.to_radians())
                * Affine::scale(k.zoom * f64::from(k.ppp))
                * Affine::translate((-k.cx, -k.cy));
            let img = layer.renderer.render(scene.picture(), k.w, k.h, affine, &style);
            let color = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            let tex = ctx.load_texture("emphasized stitches", color, egui::TextureOptions::LINEAR);
            layer.emphasized = Some((wanted, tex));
        }
        if let Some((_, tex)) = &layer.emphasized {
            textured_quad(painter, tex.id(), &quad, Color32::WHITE);
        }
    }
}

/// Make `img`, a frame for `key`, the layer's texture.
fn upload(layer: &mut StitchLayer, ctx: &egui::Context, img: &Rendered, key: LayerKey, seq: u64) {
    layer.shown_seq = seq;
    let color = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    match &mut layer.texture {
        Some(tex) => tex.set(color, egui::TextureOptions::LINEAR),
        None => layer.texture = Some(ctx.load_texture("stitches", color, egui::TextureOptions::LINEAR)),
    }
    layer.key = Some(key);
    layer.uploads += 1;
}

#[cfg(test)]
mod tests;
