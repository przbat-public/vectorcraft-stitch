//! The stitch preview as a picture: `embroidery.render`, and the control channel's
//! `ui.render {stitchPreview}`, draw the active document's stitches through one function
//! ([`picture`]), the scene the canvas draws, in the same look, so an agent or a test sees what a
//! person sees (and can look with a locked screen).

use serde_json::{Map, Value, json};
use vectorcraft_geom::{Affine, Rect};
use vectorcraft_render::Rendered;
use vectorcraft_render::lines::LineRenderer;
use vectorcraft_stitch::mm_to_pt;

use super::scene::FABRIC;
use super::*;

/// The margin round a design drawn whole, millimetres: as an opened design's artboard has (a little
/// room to see its edges).
pub const DESIGN_MARGIN_MM: f64 = 5.0;

/// The part of the document a picture of the stitches shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Region {
    /// The design's stitches, with [`DESIGN_MARGIN_MM`] round them.
    Design,
    /// One artboard (0-based).
    Artboard(usize),
    /// A box of the document, points.
    Rect(Rect),
}

/// What a picture of the stitches marks besides them, as the stitch preview's options do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    /// Jumps and cuts.
    pub jumps: bool,
    /// A dot at each needle point, once they stand apart.
    pub needle_points: bool,
}

/// The active document's stitches as the stitch preview draws them, planned with `opts`: `region`
/// at `scale` pixels a point, marked as `look` asks, on `background` (`None`: transparent). The
/// picture, the region drawn (document points) and the plan's generation.
pub fn picture(
    s: &mut Session,
    region: Region,
    scale: f64,
    look: Look,
    background: Option<[u8; 3]>,
    opts: &Options,
) -> Result<(Rendered, Rect, u64)> {
    let planner = s.embroidery_planner()?;
    let planned = planner.planned(opts)?;
    let scene = planned.scene();
    let doc = planner.document();
    let r = match region {
        Region::Design => {
            let margin = mm_to_pt(DESIGN_MARGIN_MM);
            let nothing = || EngineError::Other("nothing to draw: the document sews no stitch".into());
            scene.bounds().map(|b| b.inflate(margin, margin)).ok_or_else(nothing)?
        }
        Region::Artboard(i) => doc.artboards.get(i).map(|a| a.rect).ok_or_else(|| EngineError::Other("no such artboard".into()))?,
        Region::Rect(r) => r,
    };
    let (w, h) = vectorcraft_render::raster_size(r, scale).map_err(EngineError::Other)?;
    let view = Affine::scale(scale) * Affine::translate((-r.x0, -r.y0));
    let mut style = scene.style(FABRIC, look.jumps, look.needle_points, scale);
    style.background = background.map(|[r, g, b]| [r, g, b, 255]);
    let img = LineRenderer::new().render(scene.picture(), w, h, view, &style);
    Ok((img, r, planned.generation))
}

/// The keys `embroidery.render` takes besides the embroidery options.
const KEYS: [&str; 7] = ["artboard", "rect", "region", "scale", "jumps", "needlePoints", "background"];

/// The most pixels a point `embroidery.render` draws at: as the canvas's closest zoom (6,400 %).
const MAX_SCALE: f64 = 64.0;

pub(super) fn render_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "embroidery.render";
    let o = object(p, C)?;
    let opts = options_in(&o, &KEYS, C)?;
    let region = region_param(&o)?;
    let scale = match o.get("scale") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v
            .as_f64()
            .filter(|k| k.is_finite() && *k > 0.0 && *k <= MAX_SCALE)
            .ok_or_else(|| bad(C, format!("scale {v}: a number of pixels a point, above 0 and at most {MAX_SCALE}")))?,
    };
    let flag = |key: &str, default: bool| match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| bad(C, format!("{key} {v}: true or false"))),
    };
    let look = Look { jumps: flag("jumps", true)?, needle_points: flag("needlePoints", false)? };
    let background = match o.get("background") {
        None => Some(FABRIC),
        Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .filter(|h| h.starts_with('#'))
                .and_then(vectorcraft_color::Color::from_hex)
                .map(|c| {
                    let [r, g, b, _] = c.to_rgba8(1.0);
                    [r, g, b]
                })
                .ok_or_else(|| bad(C, format!("background {v}: \"#rrggbb\", or null for none")))?,
        ),
    };
    let (img, r, generation) = picture(s, region, scale, look, background, &opts)?;
    let png = img.to_png().map_err(EngineError::Other)?;
    Ok(with_options(
        json!({
            "dataBase64": vectorcraft_format::base64_encode(&png),
            "width": img.width,
            "height": img.height,
            "region": [r.x0, r.y0, r.x1, r.y1],
            "generation": generation,
        }),
        &opts,
    ))
}

/// The region `embroidery.render`'s params ask for: `rect`, `artboard` or `region`, at most one.
fn region_param(o: &Map<String, Value>) -> Result<Region> {
    const C: &str = "embroidery.render";
    let given: Vec<&str> = ["rect", "artboard", "region"].into_iter().filter(|k| o.get(*k).is_some_and(|v| !v.is_null())).collect();
    if given.len() > 1 {
        return Err(bad(C, format!("give one of {}", given.join(", "))));
    }
    if let Some(v) = o.get("rect").filter(|v| !v.is_null()) {
        let refused = || bad(C, format!("rect {v}: [left, top, right, bottom] in points, each within ±{}", vectorcraft_doc::MAX_COORD));
        let nums: Vec<f64> = v.as_array().ok_or_else(refused)?.iter().map(Value::as_f64).collect::<Option<_>>().ok_or_else(refused)?;
        let [l, t, r, b] = nums.as_slice() else { return Err(refused()) };
        let within = |c: &f64| c.is_finite() && c.abs() <= vectorcraft_doc::MAX_COORD;
        if ![l, t, r, b].into_iter().all(within) || r <= l || b <= t {
            return Err(refused());
        }
        return Ok(Region::Rect(Rect::new(*l, *t, *r, *b)));
    }
    if let Some(v) = o.get("artboard").filter(|v| !v.is_null()) {
        let i = v.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(C, format!("artboard {v}: an artboard's index, from 0")))?;
        return Ok(Region::Artboard(i));
    }
    match o.get("region") {
        None | Some(Value::Null) => Ok(Region::Design),
        Some(v) if v.as_str() == Some("design") => Ok(Region::Design),
        Some(v) => Err(bad(C, format!("region {v}: \"design\" (or give artboard or rect)"))),
    }
}
