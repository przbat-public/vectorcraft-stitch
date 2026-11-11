//! The image window's preview: the picture as each step shows it, and Compare.
//!
//! - **Original**: the working copy, the picture as it is digitized (cropped, at its size).
//! - **Colors**: each area in its thread's own colour (the project owner's choice, 2026-11-11), the
//!   background and the colours not sewn showing the fabric.
//! - **Shapes**: the shapes as they are sewn (the overlap in them; a line's centre line), each in
//!   its thread's colour, over the original faded.
//! - **Stitches**: the planned stitches on the fabric, and under them the honesty line: the preview
//!   isn't a test sew-out.
//! - **Compare**: the original left of a split, the step's view right of it; the split moves by a
//!   drag or by the keys (←/→ 5 %, with Shift 1 %, Home and End), a screen reader's splitter.
//!
//! Zoom (Fit, 50 to 400 %: 100 % is the design at its size as the canvas's 100 % shows it) and pan
//! (a drag); a scale bar of a round length in millimetres. The area under the pointer is outlined
//! two-tone (dark under light, as stitch editing marks its selection) and named in a tooltip and for
//! screen readers ("Dark Fuchsia 2 · 470 mm² · top left"). Textures are uploaded once a result: a
//! view switched or the split dragged uploads nothing; the shapes and the stitches are drawn again
//! when the zoom, the pan or the preview's size changes.

use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use vectorcraft_engine::cmd::embroidery::{AreaFacts, AreaPlace, DigitizeOutputs, DigitizeStage};
use vectorcraft_geom::{Affine, Point};
use vectorcraft_render::lines::{LinePicture, LineRenderer, LineStyle, Widths, edge_against};

use super::schedule::Freshness;
use super::worker::Painted;
use crate::VectorcraftApp;
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::state::Dialog;
use crate::theme::Tokens;

/// The preview's views, by their ids in the dialog's `view` field.
pub(crate) const VIEWS: [&str; 5] = ["original", "colors", "shapes", "stitches", "compare"];

/// The zoom field's values besides Fit (percent): as the app's Save for Web dialog offers them.
pub(crate) const ZOOMS: [f64; 4] = [50.0, 100.0, 200.0, 400.0];

/// The dialog's fields of the zoom ("fit" or a percent) and of Compare's split (0 to 1, the share
/// of the preview's width the original takes).
pub(crate) const ZOOM: &str = "zoom";
pub(crate) const SPLIT: &str = "split";
const FIT: &str = "fit";

/// Compare's split when the window opens: half and half.
const FIRST_SPLIT: f64 = 0.5;

/// Compare's split stepped by the keys (shares of the width): ←/→ a twentieth, with Shift a
/// hundredth. An estimate, after the WAI-ARIA window splitter pattern
/// (<https://www.w3.org/WAI/ARIA/apg/patterns/windowsplitter/>).
pub(crate) const SPLIT_STEP: f64 = 0.05;
pub(crate) const SPLIT_FINE_STEP: f64 = 0.01;

/// The space left round the design at Fit (points).
const FIT_MARGIN: f32 = 16.0;

/// The heights of the view tabs' strip and of the strip under the picture (the scale bar, the zoom,
/// the honesty line), and their words' size (points).
const TABS_HEIGHT: f32 = 24.0;
const FOOT_HEIGHT: f32 = 24.0;
const WORDS: f32 = 12.0;

/// The scale bar's lengths (millimetres): the one drawn is the longest no longer than
/// [`SCALE_BAR_MAX_PT`] on screen, or the shortest. Round lengths, as a map's scale bar has.
const SCALE_LENGTHS_MM: [f64; 6] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0];
/// The longest a scale bar is drawn (points). An estimate, twice the shortest length it aims for
/// (60 points, the length a short label sits under).
pub(crate) const SCALE_BAR_MAX_PT: f64 = 120.0;

/// How much of its colour the original keeps under the Shapes view's outlines (of 255): about a
/// third, as isolation dims the art round it. An estimate.
const FADED_ORIGINAL: u8 = 90;

/// The hovered area's outline: a dark line under a light one, as stitch editing marks its
/// selection (points), in two fixed tones that contrast whatever lies under them.
const OUTLINE_DARK_WIDTH: f32 = 3.0;
const OUTLINE_LIGHT_WIDTH: f32 = 1.0;
const OUTLINE_DARK: Color32 = Color32::from_rgb(20, 20, 20);
const OUTLINE_LIGHT: Color32 = Color32::from_rgb(245, 245, 245);

/// Compare's handle: its round grip's radius and the split line's width (points); at least the
/// 24-point target WCAG 2.2 asks for.
const GRIP_RADIUS: f32 = 12.0;
const SPLIT_LINE_WIDTH: f32 = 2.0;

/// How finely a hovered outline follows its curves on screen (points).
const HOVER_FLATTEN_PT: f64 = 0.25;

/// Millimetres in a point (1/72 inch).
const MM_PER_PT: f64 = 25.4 / 72.0;

/// The preview's state between frames: the textures of the result shown, the lines drawn, the pan,
/// and the area under the pointer.
#[derive(Default)]
pub(crate) struct Preview {
    original: Option<(u64, TextureHandle)>,
    painted: Option<(u64, TextureHandle)>,
    /// The Shapes view's lines of the shapes shown (by their number), and their colours.
    outlines: Option<(u64, Arc<LinePicture>, Vec<[u8; 3]>)>,
    /// The Shapes and the Stitches views as last drawn, each by what it was drawn of.
    shapes_drawn: Option<(LinesKey, TextureHandle)>,
    stitches_drawn: Option<(LinesKey, TextureHandle)>,
    renderer: LineRenderer,
    /// How far the design is moved from the middle of the preview (points).
    pan: Vec2,
    /// The area under the pointer, as last found.
    pub(crate) hovered: Option<AreaFacts>,
    /// The picture's pane and the design's view on it, as last drawn.
    pub(crate) last_view: Option<(Rect, ViewXf)>,
    /// How many textures it has uploaded (the tests count them).
    pub(crate) uploads: u64,
}

/// What a drawing of lines is of: which output (its number), the image's size in pixels, and the
/// view (the scale and the offset, as bits).
#[derive(Clone, Debug, PartialEq)]
struct LinesKey {
    number: u64,
    size: [u32; 2],
    view: [u64; 6],
}

/// A view of the preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Original,
    Colors,
    Shapes,
    Stitches,
    Compare,
}

impl View {
    /// The view the dialog's field names (Compare when it names none it knows).
    pub(crate) fn of(id: &str) -> Self {
        match id {
            "original" => Self::Original,
            "colors" => Self::Colors,
            "shapes" => Self::Shapes,
            "stitches" => Self::Stitches,
            _ => Self::Compare,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Original => crate::et!("Original"),
            Self::Colors => crate::et!("Colors"),
            Self::Shapes => crate::et!("Shapes"),
            Self::Stitches => crate::et!("Stitches"),
            Self::Compare => crate::et!("Compare"),
        }
    }

    /// The view Compare shows right of its split at step `step`: the step's own (the colours for
    /// the image's step, which has none but the original).
    pub(crate) fn of_step(step: u64) -> Self {
        match step {
            3 => Self::Shapes,
            4 => Self::Stitches,
            _ => Self::Colors,
        }
    }
}

/// The design on screen: a point of the document at `doc * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewXf {
    pub(crate) scale: f64,
    pub(crate) offset: Vec2,
}

impl ViewXf {
    pub(crate) fn to_screen(self, p: Point) -> Pos2 {
        pos2((p.x * self.scale) as f32 + self.offset.x, (p.y * self.scale) as f32 + self.offset.y)
    }

    fn to_doc(self, p: Pos2) -> Point {
        Point::new(f64::from(p.x - self.offset.x) / self.scale, f64::from(p.y - self.offset.y) / self.scale)
    }

    /// Document points to the device pixels of an image of the pane `pane` at `ppp` pixels a point.
    fn to_pixels(self, pane: Rect, ppp: f32) -> Affine {
        let s = self.scale * f64::from(ppp);
        Affine::new([s, 0.0, 0.0, s, f64::from((self.offset.x - pane.left()) * ppp), f64::from((self.offset.y - pane.top()) * ppp)])
    }
}

/// The scale bar's length (millimetres) for a design drawn `pt_per_mm` screen points a millimetre.
pub(crate) fn scale_length(pt_per_mm: f64) -> f64 {
    SCALE_LENGTHS_MM.iter().copied().take_while(|&mm| mm * pt_per_mm <= SCALE_BAR_MAX_PT).last().unwrap_or(SCALE_LENGTHS_MM[0])
}

/// Compare's split after key `key` (with Shift: `fine`), from `split`; `None` for a key it doesn't
/// take.
pub(crate) fn split_after(split: f64, key: egui::Key, fine: bool) -> Option<f64> {
    let step = if fine { SPLIT_FINE_STEP } else { SPLIT_STEP };
    let next = match key {
        egui::Key::ArrowLeft => split - step,
        egui::Key::ArrowRight => split + step,
        egui::Key::Home => 0.0,
        egui::Key::End => 1.0,
        _ => return None,
    };
    Some(next.clamp(0.0, 1.0))
}

/// The design's box in document points: the working copy's corners on the page.
fn design_box(outputs: &DigitizeOutputs) -> Option<vectorcraft_geom::Rect> {
    let (w, h, _) = outputs.working_rgba()?;
    let xf = outputs.working_to_doc()?;
    let corners = [(0.0, 0.0), (f64::from(w), 0.0), (0.0, f64::from(h)), (f64::from(w), f64::from(h))].map(|(x, y)| xf * Point::new(x, y));
    let (mut l, mut t, mut r, mut b) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in corners {
        (l, t, r, b) = (l.min(c.x), t.min(c.y), r.max(c.x), b.max(c.y));
    }
    (r > l && b > t).then(|| vectorcraft_geom::Rect::new(l, t, r, b))
}

/// The view of the design in `pane`, zoomed as `zoom` says ("fit" or a percent), moved by `pan`.
pub(crate) fn view_of(design: vectorcraft_geom::Rect, pane: Rect, zoom: &Value, pan: Vec2) -> ViewXf {
    let fit = (f64::from(pane.width() - 2.0 * FIT_MARGIN) / design.width()).min(f64::from(pane.height() - 2.0 * FIT_MARGIN) / design.height());
    let scale = zoom.as_f64().filter(|z| z.is_finite() && *z > 0.0).map_or(fit, |percent| percent / 100.0).max(f64::MIN_POSITIVE);
    let middle = design.center();
    let offset = pane.center().to_vec2() + pan - vec2((middle.x * scale) as f32, (middle.y * scale) as f32);
    ViewXf { scale, offset }
}

/// One quad drawn with `tex` whole, its corners `quad` from the top left, clockwise, tinted.
fn textured_quad(p: &egui::Painter, tex: egui::TextureId, quad: [Pos2; 4], tint: Color32) {
    let mut mesh = egui::Mesh::with_texture(tex);
    for (pos, uv) in quad.iter().zip([pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: *pos, uv, color: tint });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    p.add(Shape::mesh(mesh));
}

/// The working copy's corners on screen, from the top left, clockwise.
fn working_quad(outputs: &DigitizeOutputs, xf: ViewXf) -> Option<[Pos2; 4]> {
    let (w, h, _) = outputs.working_rgba()?;
    let to_doc = outputs.working_to_doc()?;
    let (w, h) = (f64::from(w), f64::from(h));
    Some([(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|(x, y)| xf.to_screen(to_doc * Point::new(x, y))))
}

impl Preview {
    /// The original's texture for `outputs`, uploaded once an image stage.
    fn original(&mut self, ctx: &egui::Context, outputs: &DigitizeOutputs) -> Option<egui::TextureId> {
        let number = outputs.number(DigitizeStage::Image)?;
        if self.original.as_ref().is_none_or(|(n, _)| *n != number) {
            let (w, h, rgba) = outputs.working_rgba()?;
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba);
            self.original = Some((number, ctx.load_texture("digitize-original", img, egui::TextureOptions::LINEAR)));
            self.uploads += 1;
        }
        self.original.as_ref().map(|(_, t)| t.id())
    }

    /// The Colors view's texture for `outputs`: the raster `painted` on the thread when it is of
    /// these colours, else painted here; uploaded once a colours stage.
    fn painted(&mut self, ctx: &egui::Context, outputs: &DigitizeOutputs, painted: Option<&Arc<Painted>>) -> Option<egui::TextureId> {
        let number = outputs.number(DigitizeStage::Colours)?;
        if self.painted.as_ref().is_none_or(|(n, _)| *n != number) {
            let img = match painted.filter(|p| p.number == number) {
                Some(p) => egui::ColorImage::from_rgba_unmultiplied([p.width as usize, p.height as usize], &p.rgba),
                None => {
                    let (w, h, rgba) = outputs.painted_rgba()?;
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba)
                }
            };
            self.painted = Some((number, ctx.load_texture("digitize-colors", img, egui::TextureOptions::NEAREST)));
            self.uploads += 1;
        }
        self.painted.as_ref().map(|(_, t)| t.id())
    }

    /// The Shapes view's lines drawn for `outputs` in `pane` at `xf`.
    fn shapes(&mut self, ctx: &egui::Context, outputs: &DigitizeOutputs, pane: Rect, xf: ViewXf, fabric: [u8; 3]) -> Option<egui::TextureId> {
        let number = outputs.number(DigitizeStage::Shapes)?;
        if self.outlines.as_ref().is_none_or(|(n, _, _)| *n != number) {
            let (picture, colors) = outputs.outlines()?;
            self.outlines = Some((number, Arc::new(picture), colors));
        }
        let (_, picture, colors) = self.outlines.as_ref()?;
        let ppp = ctx.pixels_per_point();
        let key = lines_key(number, pane, ppp, xf);
        if self.shapes_drawn.as_ref().is_none_or(|(k, _)| *k != key) {
            let edges = colors.iter().map(|c| edge_against(*c, fabric)).collect();
            let style = LineStyle {
                colors: colors.clone(),
                edges,
                widths: Widths::Hairline,
                backdrop: fabric,
                px_per_pt: f64::from(ppp),
                ..LineStyle::default()
            };
            let img = self.renderer.render(picture, key.size[0], key.size[1], xf.to_pixels(pane, ppp), &style);
            let image = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            self.shapes_drawn = Some((key, ctx.load_texture("digitize-shapes", image, egui::TextureOptions::LINEAR)));
            self.uploads += 1;
        }
        self.shapes_drawn.as_ref().map(|(_, t)| t.id())
    }

    /// The Stitches view drawn for `outputs` in `pane` at `xf`, on `fabric`.
    fn stitches(&mut self, ctx: &egui::Context, outputs: &DigitizeOutputs, pane: Rect, xf: ViewXf, fabric: [u8; 3]) -> Option<egui::TextureId> {
        let number = outputs.number(DigitizeStage::Plan)?;
        let planned = outputs.planned()?;
        let ppp = ctx.pixels_per_point();
        let key = lines_key(number, pane, ppp, xf);
        if self.stitches_drawn.as_ref().is_none_or(|(k, _)| *k != key) {
            let scene = planned.scene();
            let style =
                LineStyle { background: Some([fabric[0], fabric[1], fabric[2], u8::MAX]), ..scene.style(fabric, false, false, f64::from(ppp)) };
            let img = self.renderer.render(scene.picture(), key.size[0], key.size[1], xf.to_pixels(pane, ppp), &style);
            let image = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
            self.stitches_drawn = Some((key, ctx.load_texture("digitize-stitches", image, egui::TextureOptions::LINEAR)));
            self.uploads += 1;
        }
        self.stitches_drawn.as_ref().map(|(_, t)| t.id())
    }
}

/// What a drawing of output `number` in `pane` at `ppp` pixels a point and `xf` is keyed by.
fn lines_key(number: u64, pane: Rect, ppp: f32, xf: ViewXf) -> LinesKey {
    let px = |v: f32| (v * ppp).round().max(1.0) as u32;
    let bits = |v: f32| f64::from(v).to_bits();
    LinesKey {
        number,
        size: [px(pane.width()), px(pane.height())],
        view: [
            xf.scale.to_bits(),
            bits(xf.offset.x - pane.left()),
            bits(xf.offset.y - pane.top()),
            bits(ppp),
            bits(pane.width()),
            bits(pane.height()),
        ],
    }
}

/// A place in words, for screen readers.
fn place_words(place: AreaPlace) -> &'static str {
    match place {
        AreaPlace::TopLeft => crate::et!("top left"),
        AreaPlace::Top => crate::et!("top"),
        AreaPlace::TopRight => crate::et!("top right"),
        AreaPlace::Left => crate::et!("left"),
        AreaPlace::Middle => crate::et!("middle"),
        AreaPlace::Right => crate::et!("right"),
        AreaPlace::BottomLeft => crate::et!("bottom left"),
        AreaPlace::Bottom => crate::et!("bottom"),
        AreaPlace::BottomRight => crate::et!("bottom right"),
    }
}

/// An area's name: its thread and its number among that thread's areas in sewing order ("Dark
/// Fuchsia 2", the owner's choice), the background, or a colour not sewn.
pub(crate) fn area_name(facts: &AreaFacts) -> String {
    let lang = crate::i18n::current();
    match (&facts.thread_name, facts.own_number) {
        _ if facts.background => crate::et!("Background").to_string(),
        (Some(thread), Some(n)) => fill(crate::et!("{thread} {number}"), &[("thread", thread), ("number", &numbers::count(lang, n as u64))]),
        (Some(thread), None) => thread.clone(),
        (None, _) => crate::et!("Not sewn").to_string(),
    }
}

/// An area in a line, for its tooltip and for screen readers: its name, its size and its place
/// ("Dark Fuchsia 2 · 470 mm² · top left"), or that it isn't sewn.
pub(crate) fn area_line(facts: &AreaFacts) -> String {
    let lang = crate::i18n::current();
    let size = fill(crate::et!("{value} mm²"), &[("value", &numbers::decimal(lang, facts.area_mm2, 1))]);
    let name = area_name(facts);
    if !facts.sewn {
        return fill(crate::et!("{name} · not sewn"), &[("name", &name)]);
    }
    fill(crate::et!("{name} · {size} · {place}"), &[("name", &name), ("size", &size), ("place", place_words(facts.place))])
}

/// The preview, drawn in the preview's region of `ui`, for dialog `d` (its view, zoom and split
/// fields changed in place).
pub(crate) fn show(app: &mut VectorcraftApp, ui: &mut Ui, d: &mut Dialog, now: f64) {
    let t = Tokens::get(ui.ctx());
    let region = ui.max_rect();
    let view = View::of(d.str(super::VIEW).as_str());
    // The view tabs.
    let tabs_rect = Rect::from_min_size(region.min, vec2(region.width(), TABS_HEIGHT));
    ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
        let views: Vec<View> = VIEWS.iter().map(|v| View::of(v)).collect();
        let items: Vec<crate::panels::embroidery::Segment<'_>> = views
            .iter()
            .map(|v| crate::panels::embroidery::Segment {
                id: ui.scope_id().with(("digitize-view", v.label())),
                label: v.label(),
                full: v.label(),
                selected: *v == view,
                enabled: true,
            })
            .collect();
        if let Some(k) = crate::panels::embroidery::segments(ui, TABS_HEIGHT, WORDS, &items) {
            d.fields.insert(super::VIEW.into(), json!(VIEWS.get(k).copied().unwrap_or("compare")));
        }
    });
    let pane =
        Rect::from_min_max(pos2(region.left(), tabs_rect.bottom() + super::frame::GAP / 2.0), pos2(region.right(), region.bottom() - FOOT_HEIGHT));
    let foot = Rect::from_min_max(pos2(region.left(), pane.bottom()), region.max);
    let Some(live) = app.embroidery.image_window.live.as_mut() else { return };
    let doc = app.session.active().map(|st| st.doc.clone());
    let fabric = doc.as_deref().map_or([255, 255, 255], vectorcraft_engine::cmd::embroidery::scene::fabric_color);
    let painter = ui.painter_at(pane);
    painter.rect_filled(pane, f32::from(t.radius), t.panel_darker);
    let Some(shown) = live.schedule.shown.clone() else {
        painter.text(pane.center(), egui::Align2::CENTER_CENTER, crate::et!("Digitizing…"), egui::FontId::proportional(WORDS), t.text_dim);
        return;
    };
    let outputs = &shown.outputs;
    let Some(design) = design_box(outputs) else { return };
    let zoom = d.fields.get(ZOOM).cloned().unwrap_or(json!(FIT));
    let resp = ui.interact(pane, ui.scope_id().with("digitize-pane"), Sense::click_and_drag());
    if resp.dragged() && zoom != json!(FIT) {
        live.preview.pan += resp.drag_delta();
    }
    let xf = view_of(design, pane, &zoom, live.preview.pan);
    live.preview.last_view = Some((pane, xf));
    let ctx = ui.ctx().clone();
    let fabric32 = Color32::from_rgb(fabric[0], fabric[1], fabric[2]);
    let split = d.f64(SPLIT, FIRST_SPLIT).clamp(0.0, 1.0);
    let split_x = pane.left() + pane.width() * split as f32;
    let step = d.fields.get(super::STEP).and_then(Value::as_u64).unwrap_or(2);
    let draw = |preview: &mut Preview, painter: &egui::Painter, which: View| {
        let quad = working_quad(outputs, xf);
        match which {
            View::Original | View::Compare => {
                painter.rect_filled(pane, 0.0, fabric32);
                if let (Some(tex), Some(q)) = (preview.original(&ctx, outputs), quad) {
                    textured_quad(painter, tex, q, Color32::WHITE);
                }
            }
            View::Colors => {
                painter.rect_filled(pane, 0.0, fabric32);
                if let (Some(tex), Some(q)) = (preview.painted(&ctx, outputs, live_painted(&shown)), quad) {
                    textured_quad(painter, tex, q, Color32::WHITE);
                }
            }
            View::Shapes => {
                painter.rect_filled(pane, 0.0, fabric32);
                if let (Some(tex), Some(q)) = (preview.original(&ctx, outputs), quad) {
                    textured_quad(painter, tex, q, Color32::from_white_alpha(FADED_ORIGINAL));
                }
                if let Some(tex) = preview.shapes(&ctx, outputs, pane, xf, fabric) {
                    painter.image(tex, pane, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
            View::Stitches => {
                painter.rect_filled(pane, 0.0, fabric32);
                if let Some(tex) = preview.stitches(&ctx, outputs, pane, xf, fabric) {
                    painter.image(tex, pane, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
        }
    };
    if view == View::Compare {
        let left = Rect::from_min_max(pane.min, pos2(split_x, pane.bottom()));
        let right = Rect::from_min_max(pos2(split_x, pane.top()), pane.max);
        draw(&mut live.preview, &painter.with_clip_rect(left), View::Original);
        draw(&mut live.preview, &painter.with_clip_rect(right), View::of_step(step));
        if let Some(moved) = splitter(ui, pane, split) {
            d.fields.insert(SPLIT.into(), json!(moved));
        }
    } else {
        draw(&mut live.preview, &painter, view);
    }
    // The area under the pointer, outlined two-tone and named.
    live.preview.hovered = None;
    if let Some(at) = resp.hover_pos() {
        let doc_pt = xf.to_doc(at);
        let working = outputs.working_to_doc().map(|a| a.inverse() * doc_pt);
        if let Some(p) = working.filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x.is_finite() && p.y.is_finite()) {
            let (x, y) = (p.x.floor() as u32, p.y.floor() as u32);
            live.preview.hovered = outputs.area_at(x, y);
            if let Some(path) = outputs.outline_at(x, y) {
                outline(&painter, &path, xf);
            }
        }
    }
    if let Some(facts) = &live.preview.hovered {
        let words = area_line(facts);
        resp.clone().on_hover_text(&words);
        ui.ctx().accesskit_node_builder(resp.id, |node| node.set_description(words.clone()));
    }
    ui.ctx().accesskit_node_builder(resp.id, |node| {
        node.set_role(egui::accesskit::Role::Image);
        node.set_label(view.label());
    });
    if let Freshness::Pending { notice: true, progress } = live.schedule.freshness(now) {
        let words = super::words::recomputing(progress);
        let galley = painter.layout_no_wrap(words, egui::FontId::proportional(WORDS), t.text);
        let at = Rect::from_min_size(pos2(pane.right() - galley.size().x - 16.0, pane.top() + 8.0), galley.size() + vec2(8.0, 4.0));
        painter.rect_filled(at, f32::from(t.radius), t.panel);
        painter.galley(at.min + vec2(4.0, 2.0), galley, t.text);
    }
    foot_row(ui, foot, d, xf, view == View::Stitches || (view == View::Compare && View::of_step(step) == View::Stitches));
}

/// The Colors view's raster the thread painted for the colours shown, if any.
fn live_painted(shown: &super::schedule::Shown) -> Option<&Arc<Painted>> {
    shown.painted.as_ref()
}

/// The hovered area's outline `path` (document points) drawn two-tone at `xf`.
fn outline(painter: &egui::Painter, path: &vectorcraft_geom::BezPath, xf: ViewXf) {
    let mut lines: Vec<Vec<Pos2>> = Vec::new();
    vectorcraft_geom::kurbo::flatten(path, HOVER_FLATTEN_PT / xf.scale.max(f64::MIN_POSITIVE), |el| match el {
        vectorcraft_geom::kurbo::PathEl::MoveTo(p) => lines.push(vec![xf.to_screen(p)]),
        vectorcraft_geom::kurbo::PathEl::LineTo(p) => {
            if let Some(l) = lines.last_mut() {
                l.push(xf.to_screen(p));
            }
        }
        vectorcraft_geom::kurbo::PathEl::ClosePath => {
            if let Some(l) = lines.last_mut()
                && let Some(&first) = l.first()
            {
                l.push(first);
            }
        }
        _ => {}
    });
    for (width, colour) in [(OUTLINE_DARK_WIDTH, OUTLINE_DARK), (OUTLINE_LIGHT_WIDTH, OUTLINE_LIGHT)] {
        for l in &lines {
            painter.add(Shape::line(l.clone(), Stroke::new(width, colour)));
        }
    }
}

/// Compare's split: its line and grip in `pane` at `split`, dragged or moved by the keys while it
/// has the keyboard; a splitter for screen readers → the split moved to.
fn splitter(ui: &mut Ui, pane: Rect, split: f64) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let x = pane.left() + pane.width() * split as f32;
    let grip = Rect::from_center_size(pos2(x, pane.center().y), Vec2::splat(2.0 * GRIP_RADIUS));
    let band = Rect::from_min_max(pos2(x - GRIP_RADIUS, pane.top()), pos2(x + GRIP_RADIUS, pane.bottom()));
    let id = ui.scope_id().with("digitize-split");
    let resp = ui.interact(band, id, Sense::click_and_drag());
    let mut moved = None;
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        moved = Some(f64::from((p.x - pane.left()) / pane.width().max(1.0)).clamp(0.0, 1.0));
    }
    if resp.clicked() {
        resp.request_focus();
    }
    if resp.has_focus() {
        let keys = [egui::Key::ArrowLeft, egui::Key::ArrowRight, egui::Key::Home, egui::Key::End];
        for key in keys {
            let shift = ui.input(|i| i.modifiers.shift);
            let mods = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
            if ui.input_mut(|i| i.consume_key(mods, key)) {
                moved = split_after(moved.unwrap_or(split), key, shift);
            }
        }
    }
    let painter = ui.painter();
    painter.line_segment([pos2(x, pane.top()), pos2(x, pane.bottom())], Stroke::new(SPLIT_LINE_WIDTH, OUTLINE_LIGHT));
    painter.circle(grip.center(), GRIP_RADIUS, t.panel, Stroke::new(1.0, t.border));
    if resp.has_focus() {
        painter.circle_stroke(grip.center(), GRIP_RADIUS + 2.0, Stroke::new(2.0, t.accent));
    }
    let value = (moved.unwrap_or(split) * 100.0).round();
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Splitter);
        node.set_label(crate::et!("Compare: the original on the left"));
        node.set_numeric_value(value);
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
    });
    moved
}

/// The strip under the picture: the scale bar, the honesty line under the stitches, and the zoom.
fn foot_row(ui: &mut Ui, foot: Rect, d: &mut Dialog, xf: ViewXf, stitches: bool) {
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let painter = ui.painter_at(foot);
    // The scale bar, two-tone.
    let pt_per_mm = xf.scale / MM_PER_PT;
    let mm = scale_length(pt_per_mm);
    let length = (mm * pt_per_mm) as f32;
    let y = foot.center().y;
    let a = pos2(foot.left() + 4.0, y);
    let b = pos2(a.x + length, y);
    painter.line_segment([a, b], Stroke::new(3.0, OUTLINE_DARK));
    painter.line_segment([a, b], Stroke::new(1.0, OUTLINE_LIGHT));
    let label = fill(crate::et!("{value} mm"), &[("value", &numbers::decimal(lang, mm, 0))]);
    painter.text(pos2(b.x + 6.0, y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(WORDS), t.text_dim);
    if stitches {
        painter.text(
            foot.center(),
            egui::Align2::CENTER_CENTER,
            crate::et!("The preview shows the planned stitches. Sew a test piece before production."),
            egui::FontId::proportional(WORDS - 1.0),
            t.text_dim,
        );
    }
    // The zoom: Fit or a percent.
    let current = d.fields.get(ZOOM).cloned().unwrap_or(json!(FIT));
    let shown = |z: &Value| match z.as_f64() {
        Some(p) => numbers::percent(lang, p.round() as u64),
        None => crate::et!("Fit").to_string(),
    };
    let zoom_rect = Rect::from_min_max(pos2(foot.right() - 110.0, foot.top() + 2.0), pos2(foot.right(), foot.bottom() - 2.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(zoom_rect), |ui| {
        let look = crate::embroidery::fields::Look { name: Some(crate::et!("Zoom")), ..Default::default() };
        let chosen = crate::embroidery::fields::choice(ui, "digitize-zoom", &shown(&current), &look, zoom_rect.width(), |ui| {
            let mut picked = None;
            for z in std::iter::once(json!(FIT)).chain(ZOOMS.iter().map(|p| json!(p))) {
                if ui.selectable_label(z == current, shown(&z)).clicked() {
                    picked = Some(z);
                }
            }
            picked
        });
        if let Some(z) = chosen {
            d.fields.insert(ZOOM.into(), z);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scale bar is the longest round length no longer than its most on screen, or the
    /// shortest.
    #[test]
    fn the_scale_bar_picks_a_round_length() {
        let pt_per_mm = 1.0 / MM_PER_PT;
        assert_eq!(scale_length(pt_per_mm), 20.0, "100 %: 20 mm is 57 points, 50 mm 142");
        assert_eq!(scale_length(4.0 * pt_per_mm), 10.0, "400 %");
        assert_eq!(scale_length(0.1 * pt_per_mm), 50.0, "far out: the longest");
        assert_eq!(scale_length(200.0), 1.0, "close in: the shortest");
    }

    /// Compare's split takes the arrows (a twentieth, with Shift a hundredth), Home and End,
    /// within the preview.
    #[test]
    fn the_splitter_takes_its_keys() {
        let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(near(split_after(0.5, egui::Key::ArrowLeft, false), 0.45));
        assert!(near(split_after(0.5, egui::Key::ArrowRight, true), 0.51));
        assert!(near(split_after(0.5, egui::Key::Home, false), 0.0));
        assert!(near(split_after(0.5, egui::Key::End, false), 1.0));
        assert!(near(split_after(0.02, egui::Key::ArrowLeft, false), 0.0));
        assert_eq!(split_after(0.5, egui::Key::A, false), None);
    }

    /// Fit shows the whole design in the middle of the pane, inside its margin; a zoom in percent
    /// is the canvas's (100 %: a point a point).
    #[test]
    fn fit_shows_the_whole_design_and_percent_is_the_canvass() {
        let design = vectorcraft_geom::Rect::new(10.0, 20.0, 110.0, 100.0);
        let pane = Rect::from_min_size(pos2(50.0, 30.0), vec2(600.0, 400.0));
        let xf = view_of(design, pane, &json!("fit"), Vec2::ZERO);
        let (a, b) = (xf.to_screen(Point::new(10.0, 20.0)), xf.to_screen(Point::new(110.0, 100.0)));
        assert!(pane.shrink(FIT_MARGIN - 0.01).contains(a) && pane.shrink(FIT_MARGIN - 0.01).contains(b), "{a:?} {b:?}");
        assert!(((a + (b - a) / 2.0) - pane.center()).length() < 0.01, "centred");
        let full = view_of(design, pane, &json!(100.0), Vec2::ZERO);
        assert!((full.scale - 1.0).abs() < 1e-12);
        // The screen is in f32: back from it, a point comes within f32's rounding.
        assert!((xf.to_doc(xf.to_screen(Point::new(42.0, 7.0))) - Point::new(42.0, 7.0)).hypot() < 1e-3);
    }
}
