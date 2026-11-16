//! Digitizing from an image: changes to single areas (`areaChanges`), each kept by a point inside
//! its area (the image's own pixels) and the area's colour, so that it is found again after any
//! setting changes the areas.

use serde_json::{Value, json};
use vectorcraft_stitch::thread::Palette;

use super::settings::{Settings, table};

/// The settings `params` give, for a document in Janome's threads.
fn settings(params: Value) -> crate::Result<Settings> {
    let o = params.as_object().cloned().unwrap_or_default();
    Settings::from_params(&o, Palette::Jef)
}

/// The settings table's row for the area changes.
fn row() -> Value {
    table(Palette::Jef).as_array().and_then(|rows| rows.iter().find(|r| r["name"] == "areaChanges").cloned()).expect("a row for areaChanges")
}

/// A change that sews a filled orange area as tatami.
fn tatami() -> Value {
    json!({"at": [12.5, 40.0], "color": "#f08214", "kind": "fill", "stitch": "tatami"})
}

#[test]
fn area_changes_are_read_and_echoed() {
    let changes = json!([
        {"at": [12.5, 40.0], "color": "#f08214", "kind": "fill", "stitch": "tatami", "angle": 30.0},
        {"at": [100.0, 7.25], "color": "#14287a", "kind": "line", "stitch": "satin"},
        {"at": [3.0, 4.0], "color": "#ffffff", "kind": "fill", "angle": -45.5},
    ]);
    let s = settings(json!({"areaChanges": changes})).expect("taken");
    assert_eq!(s.echo()["areaChanges"], changes, "echoed as given");
    assert_eq!(settings(json!({})).expect("none").echo()["areaChanges"], json!([]), "none by default");
}

#[test]
fn an_area_change_out_of_shape_is_refused() {
    settings(json!({"areaChanges": [tatami()]})).expect("a change of a fill's stitch is taken");
    let refused = |change: Value| {
        let e = settings(json!({"areaChanges": [change.clone()]})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("areaChanges"), "{change}: {e}");
    };
    // A stitch of the other kind of area.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "stitch": "running"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "line", "stitch": "tatami"}));
    // An angle on a line, whose stitches follow it.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "line", "angle": 30.0}));
    // An angle past a full turn.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "angle": 400.0}));
    // A point that isn't two numbers, a colour that isn't #rrggbb, a kind of no area.
    refused(json!({"at": [1.0], "color": "#f08214", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, "2"], "color": "#f08214", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, 1.0], "color": "orange", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "island", "stitch": "none"}));
    // A change that changes nothing, and a key no change takes.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "stitch": "none", "thread": 3}));
    // At most as many changes as the table says.
    let most = usize::try_from(row()["range"][1].as_u64().expect("a most")).expect("a count");
    settings(json!({"areaChanges": vec![tatami(); most]})).expect("as many as the table says");
    assert!(settings(json!({"areaChanges": vec![tatami(); most + 1]})).is_err(), "one more is refused");
}

#[test]
fn the_settings_table_lists_area_changes() {
    let r = row();
    assert_eq!((&r["key"], &r["step"], &r["default"]), (&json!("area_changes"), &json!("stitches"), &json!([])));
    assert!(!r["source"].as_str().unwrap_or_default().is_empty(), "a source");
}
