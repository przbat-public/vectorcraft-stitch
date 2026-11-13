//! The fill angle's rows, shared by the image window's Stitches step and Document Setup's
//! Embroidery tab: Fixed (with its degrees, counter-clockwise on the page) or Across the shape
//! (`embroidery.setup`'s `fillAngle`: a number of degrees, or `"acrossShape"`).

use serde_json::{Value, json};

use crate::embroidery::rows::{Edit, Marks, Owned, OwnedField};
use crate::embroidery::text::fill;
use crate::embroidery::vocabulary;

/// The rule across each shape (`embroidery.setup`'s id), and the choice of a fixed angle.
pub(crate) const ACROSS: &str = "acrossShape";
const FIXED: &str = "fixed";

/// The keys of the two rows (ids, never words).
pub(crate) const CHOICE: &str = "fillAngle";
pub(crate) const DEGREES: &str = "fillAngle.degrees";

/// The degrees a fill's rows run at when Fixed is chosen from across the shape with no angle of
/// the document's own: the planner's built-in rows, 135° (`embroidery.settings`' resolved default).
pub(crate) const DEFAULT_DEGREES: f64 = 135.0;

/// The rows for fill angle `value` (degrees, or `acrossShape`): the choice, and the degrees while
/// Fixed is chosen.
pub(crate) fn rows(value: &Value) -> Vec<Owned> {
    let current = if value == ACROSS { ACROSS } else { FIXED };
    let options = vec![(FIXED.to_string(), crate::et!("Fixed").to_string()), (ACROSS.to_string(), crate::et!("Across the shape").to_string())];
    let shown = options.iter().find(|(id, _)| id == current).map(|(_, w)| w.clone()).unwrap_or_default();
    let label = crate::et!("Fill angle");
    let tip = crate::et!("The direction of a fill's rows: at a fixed angle on the page, or across each shape's length.");
    let field = OwnedField::Choice { shown, options, current: Some(current.to_string()) };
    let mut out = vec![Owned::new(CHOICE, label, label.to_string(), tip, field, Marks::inherited(None, None))];
    if let (Some(degrees), Some(vectorcraft_engine::cmd::embroidery::SettingField::Number { min, max, decimals, .. })) =
        (value.as_f64(), vectorcraft_engine::cmd::embroidery::setting_field("angle"))
    {
        let label = vocabulary::label("angle", None);
        let field = OwnedField::Number { value: Some(degrees), unit: "°", decimals, bounds: (min, max), hint: None };
        out.push(Owned::new(
            DEGREES,
            label,
            label.to_string(),
            crate::et!("The fills' rows, counter-clockwise on the page."),
            field,
            Marks::inherited(None, None),
        ));
    }
    out
}

/// The fill angle row `key`'s `edit` makes of `own` (the angle before it, as the plan uses it):
/// `Ok(None)` when it changes nothing, `Err(words)` when the degrees typed are out of range.
pub(crate) fn edited(key: &str, edit: &Edit, own: &Value) -> Result<Option<Value>, String> {
    match (key, edit) {
        (CHOICE, Edit::Choice(id)) if id == ACROSS => Ok(Some(json!(ACROSS))),
        (CHOICE, Edit::Choice(_)) => Ok(Some(own.as_f64().map_or(json!(DEFAULT_DEGREES), Value::from))),
        (DEGREES, Edit::Number(v)) => {
            let Some(vectorcraft_engine::cmd::embroidery::SettingField::Number { min, max, decimals, .. }) =
                vectorcraft_engine::cmd::embroidery::setting_field("angle")
            else {
                return Ok(None);
            };
            if v.is_finite() && (min..=max).contains(v) {
                return Ok(Some(json!(v)));
            }
            let f = crate::embroidery::fields::format(crate::i18n::current(), "°", decimals);
            Err(fill(
                crate::et!("{value} is out of range: {low} to {high}."),
                &[("value", &f.show(*v)), ("low", &f.show(min)), ("high", &f.show(max))],
            ))
        }
        _ => Ok(None),
    }
}
