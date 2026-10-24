//! Junk params for the embroidery commands, which `command_sweep`'s structured junk runs on every
//! fixture with the rest: never a panic, the document stays sound.

use serde_json::{Value, json};

/// The embroidery commands' junk cases: (command, params).
pub fn structured() -> Vec<(&'static str, Value)> {
    vec![
        ("embroidery.setup", json!({"palette": {"chart": ["madeira", null]}, "threadPalette": 1e308})),
        ("embroidery.setup", json!({"palette": "PEC", "fabric": "knit"})),
        ("embroidery.setup", json!({"palette": [null], "threadPalette": "jef"})),
        ("embroidery.setup", json!({"palette": "jef", "threadPalette": {"palette": "pec"}})),
        ("embroidery.setup", json!({"settings": {"palette": {"chart": [1e308]}}, "resolved": {"palette": 5}})),
        ("embroidery.setup", json!({"settings": [null], "palette": "pec"})),
        ("embroidery.setup", json!({"settings": {"palette": "jef", "fabric": null}, "palette": "jef"})),
        ("embroidery.settings", json!({"palette": "pec"})),
        ("embroidery.threads", json!({"stitchStyle": 5, "density": [1e308], "threadWeight": -1e308})),
        ("embroidery.threads", json!({"pullCompensation": [1e308, f64::NAN], "expand": 1e308, "trimLength": -1})),
        ("embroidery.stats", json!({"speed": 1e308, "fabricThickness": -1e308, "density": [1e308]})),
        ("embroidery.stats", json!({"speed": "fast", "fabricThickness": {"mm": 1}, "stitchLength": -1e308})),
        ("embroidery.stats", json!({"speed": 60, "fabricThickness": 0, "stitchStyle": "satin", "jumpLength": 1e308})),
        ("embroidery.sequence", json!({"stitchStyle": [5], "narrowAsSatin": 1e308, "minStitch": -1})),
        ("embroidery.check", json!({"format": ["pes"], "rules": [1, null, {"id": "layers"}], "limit": 1e308})),
        ("embroidery.check", json!({"format": "vp9", "rules": "all", "limit": -5})),
        ("embroidery.check", json!({"format": "dst", "rules": ["layers", "layers", "stitchTiny"], "limit": 1000, "density": 0.05})),
        ("embroidery.render", json!({"scale": 1e308, "rect": [0, 0, 1e308, f64::NAN], "background": 5})),
        ("embroidery.render", json!({"scale": 64, "rect": [-4e6, -4e6, 4e6, 4e6], "jumps": "yes"})),
        ("embroidery.render", json!({"artboard": 1e9, "region": "everything", "background": "#zzzzzz"})),
        ("embroidery.render", json!({"rect": [10, 10, 5, 5], "artboard": 0})),
        ("embroidery.render", json!({"scale": 0.0001, "region": "design", "background": null, "jumps": false})),
        ("embroidery.render", json!({"rect": [0, "x", 10, 10, 20], "needlePoints": "yes"})),
        ("embroidery.sewEarlier", json!({"ids": "all"})),
        ("embroidery.sewEarlier", json!({"ids": [1e308, -1, null, "x", {"id": 1}]})),
        ("embroidery.sewLater", json!({"ids": [u64::MAX, u64::MAX], "density": 1e308})),
        ("embroidery.sewLater", json!({"ids": {"0": 1}, "spacing": "close"})),
        ("embroidery.sewFirst", json!({"ids": too_many_ids()})),
        ("embroidery.sewFirst", json!({"ids": [], "stitchStyle": [5]})),
        ("embroidery.sewLast", json!({"ids": [1, 1, 1, 1], "pullCompensation": -1e308})),
        ("embroidery.sewLast", json!({"selectedOnly": true})),
        ("view.stitchPreview", json!({"on": "yes"})),
        ("view.stitchPreview", json!({"on": true, "jumps": false})),
        ("view.stitchPreview.jumps", json!({"on": [true]})),
        ("view.stitchPreview.needlePoints", json!({"on": 1e308})),
        ("view.stitchPreview.artwork", json!({"on": null, "artwork": null})),
        ("view.stitchPreview.artwork", json!([true])),
    ]
}

/// More ids than a move takes: refused before anything is looked up.
fn too_many_ids() -> Vec<u64> {
    (0..100_001).collect()
}
