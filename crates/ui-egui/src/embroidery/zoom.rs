//! View › Zoom to Box (`view.zoomTo`, a UI command): the Check tab's Show zooms to a rule's hits
//! with it, and agents to any box.

use serde_json::{Value, json};
use vectorcraft_doc::MAX_COORD;
use vectorcraft_engine::cmd::embroidery::render::corners;
use vectorcraft_geom::{Point, Rect};

use crate::VectorcraftApp;
use crate::canvas::{FIT_MARGIN, fit_zoom};

/// `view.zoomTo {rect: [left, top, right, bottom], margin?}`: centre box `rect` (document points,
/// each within ±[`MAX_COORD`]) in the window and zoom so that it fills it less `margin` screen
/// points on each side (default [`FIT_MARGIN`]), within the zoom's range → `{zoom}` (percent).
pub(crate) fn zoom_to(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let bad_rect =
        || format!("rect: [left, top, right, bottom] in document points, each within ±{MAX_COORD}, right at or past left and bottom at or below top");
    let [left, top, right, bottom] = p.get("rect").and_then(corners).filter(|[l, t, r, b]| r >= l && b >= t).ok_or_else(bad_rect)?;
    let margin = match p.get("margin").filter(|m| !m.is_null()) {
        None => FIT_MARGIN,
        Some(m) => m.as_f64().filter(|m| m.is_finite() && *m >= 0.0).ok_or("margin: screen points, 0 or more")?,
    };
    let Some(canvas) = app.canvas_rect else { return Err("no canvas is shown".into()) };
    let target = Rect::new(left, top, right, bottom);
    let zoom = fit_zoom(app, canvas, target, margin);
    // Halfway along each side, from its start: never the sum of two corners, which could overflow.
    let center = Point::new(left + (right - left) / 2.0, top + (bottom - top) / 2.0);
    if !(center.x.is_finite() && center.y.is_finite()) {
        return Err(bad_rect());
    }
    let v = app.view_mut().ok_or("no document")?;
    v.center = center;
    v.fitted = true;
    v.zoom = zoom;
    Ok(json!({ "zoom": zoom * 100.0 }))
}
