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
        ("embroidery.convertToManual", json!({"ids": [1e308, -1, null, "x"], "density": 1e308})),
        ("embroidery.convertToManual", json!({"ids": too_many_ids(), "stitchStyle": [5]})),
        ("embroidery.convertToManual", json!({"ids": {"0": 1}, "stitchLength": -1e308, "underlay": "deep"})),
        ("embroidery.restoreObject", json!({"ids": [1e308, -1, null], "strokes": "yes", "dryRun": 1})),
        ("embroidery.restoreObject", json!({"ids": too_many_ids(), "corners": [true]})),
        ("embroidery.restoreObject", json!({"dryRun": true, "frame": [1e308, 0, 0, 1, 0, 0]})),
        ("embroidery.setStopAfter", json!({"on": "yes", "ids": [1e308, -1, null]})),
        ("embroidery.setStopAfter", json!({"ids": too_many_ids(), "on": false})),
        ("embroidery.setStopAfter", json!({"on": [true], "stopAfter": 1})),
        ("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": 1e308, "index": -1}, null, "x"], "dx": f64::NAN})),
        ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": 1, "from": 9, "to": 2}], "to": [1e308, 0]})),
        ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": 1, "from": 0, "to": 1e18}], "dy": 1})),
        ("embroidery.moveNeedlePoints", json!({"needlePoints": {"id": 1}, "dx": [1]})),
        ("embroidery.moveNeedlePoints", json!({"dx": 1e308, "dy": -1e308})),
        // Integer extremes, as integers (a float that size isn't a whole number to the commands): a
        // range whose count would overflow, and indices and ids no path or object can have.
        ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": 1, "from": 0, "to": u64::MAX}], "dx": 1})),
        ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": u64::MAX, "from": u32::MAX, "to": u32::MAX}], "dy": 1})),
        ("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": 2, "from": 1, "to": u64::MAX}]})),
        ("embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": u64::MAX, "index": u64::MAX}]})),
        ("embroidery.insertNeedlePoints", json!({"id": 1, "index": u64::MAX})),
        ("embroidery.insertNeedlePoints", json!({"id": u32::MAX, "index": u32::MAX, "at": [[0, 0]]})),
        ("embroidery.insertNeedlePoints", json!({"ranges": [{"id": 1, "from": 0, "to": u64::MAX}]})),
        ("embroidery.splitRun", json!({"ranges": [{"id": 1, "from": 0, "to": u64::MAX}]})),
        ("embroidery.joinRuns", json!({"ranges": [{"id": 1, "from": u64::MAX, "to": u64::MAX}]})),
        ("embroidery.joinRuns", json!({"ids": [u64::MAX, u32::MAX]})),
        ("embroidery.setMachineFunction", json!({"function": "trim", "ranges": [{"id": 1, "from": 0, "to": u64::MAX}]})),
        ("embroidery.splitLongStitches", json!({"ranges": [{"id": 2, "from": 0, "to": u64::MAX}], "maxLength": 1})),
        ("embroidery.selectNeedlePoints", json!({"ranges": [{"id": 1, "from": 0, "to": u64::MAX}], "mode": "add"})),
        ("embroidery.selectNeedlePoints", json!({"records": [u64::MAX, u64::MAX]})),
        ("embroidery.stitches", json!({"from": u64::MAX, "to": u64::MAX, "offset": u64::MAX, "limit": u64::MAX})),
        ("embroidery.insertNeedlePoints", json!({"id": 1e308, "index": -1, "at": [[f64::NAN, 0]]})),
        ("embroidery.insertNeedlePoints", json!({"id": 1, "index": 1e18, "at": "here"})),
        ("embroidery.insertNeedlePoints", json!({"id": 1, "index": 0, "at": vec![[0.0, 0.0]; 10_001]})),
        ("embroidery.insertNeedlePoints", json!({"at": [[1, 2, 3]], "ranges": [{"id": 1, "from": 0, "to": 1e18}]})),
        ("embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": 1, "index": 0}, {"id": 1, "index": 0}], "ranges": "all"})),
        ("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": 1, "from": 0, "to": 1_999_999}, {"id": 2, "from": 0, "to": 1}]})),
        ("embroidery.deleteNeedlePoints", json!({"needlePoints": [], "index": 0})),
        ("embroidery.splitRun", json!({"ranges": [{"id": 1, "from": 0, "to": 1_999_999}], "ties": "yes"})),
        ("embroidery.splitRun", json!({"needlePoints": [{"id": 1e308, "index": 1}], "ties": false})),
        ("embroidery.joinRuns", json!({"ids": [1, 1, 1], "needlePoints": [{"id": 1, "index": 1}]})),
        ("embroidery.joinRuns", json!({"ids": [1e308, -1]})),
        ("embroidery.joinRuns", json!({"ids": [1]})),
        ("embroidery.joinRuns", json!({"ranges": [{"id": 2, "from": 0, "to": 5}]})),
        ("embroidery.setMachineFunction", json!({"function": "colorChange", "needlePoints": [{"id": 1, "index": 0}]})),
        ("embroidery.setMachineFunction", json!({"function": "stop", "on": 1, "ties": [false], "ranges": [{"id": 1, "from": 0, "to": 9}]})),
        ("embroidery.setMachineFunction", json!({"function": "trim", "needlePoints": [{"id": 1e308, "index": 1e308}]})),
        ("embroidery.splitLongStitches", json!({"maxLength": f64::NAN, "format": "dst"})),
        ("embroidery.splitLongStitches", json!({"maxLength": 1e-300, "ranges": [{"id": 1, "from": 0, "to": 1_999_999}]})),
        ("embroidery.splitLongStitches", json!({"format": ["pes"], "needlePoints": [{"id": 1, "index": 1}]})),
        ("embroidery.stitches", json!({"from": 1e308, "to": -1, "kinds": "all", "limit": 1e308})),
        ("embroidery.stitches", json!({"ids": [1e308, null], "shorterThan": f64::NAN, "longerThan": [1]})),
        ("embroidery.stitches", json!({"from": 3, "to": 1_000_000_000, "kinds": ["stop", "jump"], "density": 0.05})),
        ("embroidery.selectNeedlePoints", json!({"records": [1, 1e308], "mode": "toggle"})),
        ("embroidery.selectNeedlePoints", json!({"records": "all", "needlePoints": [{"id": 1, "index": 0}]})),
        ("embroidery.selectNeedlePoints", json!({"shorterThan": 1e308, "mode": "subtract", "stitchStyle": "satin"})),
        // A selection by a shape (a marquee, a lasso) given as nothing, as words, nested, both at
        // once, beside addresses or records, with points that aren't numbers, and with ids repeated,
        // nested in lists or too many.
        ("embroidery.selectNeedlePoints", json!({"rect": null})),
        ("embroidery.selectNeedlePoints", json!({"polygon": null, "mode": "add"})),
        ("embroidery.selectNeedlePoints", json!({"rect": "all", "mode": "toggle"})),
        ("embroidery.selectNeedlePoints", json!({"polygon": "lasso"})),
        ("embroidery.selectNeedlePoints", json!({"rect": [[0, 0], [100, 100]]})),
        ("embroidery.selectNeedlePoints", json!({"polygon": [[[0, 0]], [[100, 0]], [[100, 100]]]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [0, null, "x", 1e308]})),
        ("embroidery.selectNeedlePoints", json!({"polygon": [[0, 0], null, [1e308, 0], "x", [5, f64::NAN]]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [0, 0, 100, 100], "polygon": [[0, 0], [100, 0], [100, 100]]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [0, 0, 100, 100], "needlePoints": [{"id": 1, "index": 0}]})),
        ("embroidery.selectNeedlePoints", json!({"polygon": [[0, 0], [100, 0], [100, 100]], "ranges": [{"id": 1, "from": 0, "to": 3}]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [0, 0, 100, 100], "records": [1, 2], "kinds": ["trim"]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [-1e6, -1e6, 1e6, 1e6], "ids": [1, 1, 1, 2, 2, 3]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [-1e6, -1e6, 1e6, 1e6], "ids": [[1], [2, [3]], {"id": 4}]})),
        ("embroidery.selectNeedlePoints", json!({"rect": [-1e6, -1e6, 1e6, 1e6], "ids": too_many_ids(), "mode": "subtract"})),
        ("embroidery.selectNeedlePoints", json!({"polygon": [[0, 0], [1e6, 0], [0, 1e6]], "ids": [u64::MAX, 0, 1, 2, 3, 4, 5, 6, 7, 8]})),
        // A lasso of as many points as one may have, zigzagging: each edge across half its height
        // (refused as too intricate) and each across a sliver of it (taken); and one point more.
        ("embroidery.selectNeedlePoints", json!({"polygon": zigzag(LASSO_POINTS_MAX, ZIGZAG_DEEP_MM)})),
        ("embroidery.selectNeedlePoints", json!({"polygon": zigzag(LASSO_POINTS_MAX, ZIGZAG_SHALLOW_MM)})),
        ("embroidery.selectNeedlePoints", json!({"polygon": zigzag(LASSO_POINTS_MAX + 1, ZIGZAG_SHALLOW_MM), "mode": "add"})),
        ("embroidery.sewEarlier", json!({"ids": "all"})),
        ("embroidery.sewEarlier", json!({"ids": [1e308, -1, null, "x", {"id": 1}]})),
        ("embroidery.sewLater", json!({"ids": [u64::MAX, u64::MAX], "density": 1e308})),
        ("embroidery.sewLater", json!({"ids": {"0": 1}, "spacing": "close"})),
        ("embroidery.sewFirst", json!({"ids": too_many_ids()})),
        ("embroidery.sewFirst", json!({"ids": [], "stitchStyle": [5]})),
        ("embroidery.sewLast", json!({"ids": [1, 1, 1, 1], "pullCompensation": -1e308})),
        ("embroidery.sewLast", json!({"selectedOnly": true})),
        ("embroidery.set", json!({"part": "fill", "angle": 1e308, "pull": [f64::NAN, 1e308], "ids": [1, 2]})),
        // Too many ids with every other parameter valid: refused on the ids themselves.
        ("embroidery.set", json!({"part": "fill", "angle": 10, "ids": too_many_ids()})),
        ("embroidery.set", json!({"part": ["fill"], "stitch": {"auto": true}, "spacing": -1e308})),
        ("embroidery.set", json!({"part": "stroke", "underlay": [null], "splitLength": "long", "ids": [u64::MAX]})),
        ("embroidery.set", json!({"part": "fill", "pullPercent": [1, 2, 3], "push": {"0": 1}, "narrowAsSatin": 1e308, "ids": [1, 1, 1]})),
        ("embroidery.set", json!({"part": "fill", "stitch": "none", "angle": -1e308, "expand": null})),
        ("embroidery.reset", json!({"part": "both", "keys": "all"})),
        ("embroidery.reset", json!({"keys": [null, 5, "density", "angle"], "ids": [1e308]})),
        ("embroidery.reset", json!({"keys": ["entry", "exit", "entry"], "ids": too_many_ids()})),
        ("embroidery.setEntryExit", json!({"id": -1, "part": "fill", "entry": [f64::NAN, 1e308]})),
        ("embroidery.setEntryExit", json!({"id": u64::MAX, "part": "stroke", "entry": "here", "exit": [0]})),
        ("embroidery.setEntryExit", json!({"part": "fill", "entry": [-4e6, 4e6], "exit": null})),
        ("embroidery.setEntryExit", json!({"part": "stroke", "entry": [1e6, -1e6], "exit": [0, 0], "snap": true})),
        ("embroidery.setEntryExit", json!({"id": 1, "part": "fill", "exit": [5, 5], "snap": [true]})),
        ("embroidery.nearestConnection", json!({"ids": [1e308, -1, null], "dryRun": true})),
        ("embroidery.nearestConnection", json!({"ids": too_many_ids()})),
        ("embroidery.nearestConnection", json!({"all": [true], "dryRun": 1, "density": 1e308})),
        ("embroidery.nearestConnection", json!({"all": true, "stitchStyle": "satin", "jumpLength": 1e308, "trimLength": -1})),
        ("embroidery.nearestConnection", json!({"all": true, "dryRun": false, "splitLength": f64::NAN})),
        ("embroidery.digitizeImage", json!({"id": u64::MAX, "dryRun": true})),
        ("embroidery.digitizeImage", json!({"id": 1, "dryRun": 1, "width": 1e308, "crop": [0, 0, u32::MAX, u32::MAX]})),
        ("embroidery.digitizeImage", json!({"fit": [f64::NAN, -1], "colors": 1e308, "minArea": -1, "smoothing": "smooth"})),
        ("embroidery.digitizeImage", json!({"colorChoices": [{"color": "#zzzzzz", "thread": {"palette": "pec", "index": u64::MAX}}, null, 5]})),
        ("embroidery.digitizeImage", json!({"colorChoices": (0..300).map(|_| json!({"color": "#000000", "job": "none"})).collect::<Vec<_>>()})),
        (
            "embroidery.digitizeImage",
            json!({"colorChoices": [{"color": "#000000", "job": "lines"}, {"color": "#ffffff", "job": ["lines"]}], "dryRun": true}),
        ),
        (
            "embroidery.digitizeImage",
            json!({"colorChoices": [{"color": "#141414", "job": "lines", "mergeInto": "#ffffff"}], "overlap": 0, "outlines": true}),
        ),
        ("embroidery.digitizeImage", json!({"crop": [10, 10, 5, 5], "at": [1e308, f64::NAN], "overlap": 1e308, "mode": "photo", "background": [1]})),
        ("embroidery.digitizeImage", json!({"offset": u64::MAX, "limit": u64::MAX, "regions": "yes", "outlines": [true], "height": -1e-300})),
        ("embroidery.copyFrom", json!({"source": "first", "ids": [1], "keys": ["angle", "angle"]})),
        ("embroidery.copyFrom", json!({"source": 1, "ids": too_many_ids()})),
        ("embroidery.copyFrom", json!({"source": u64::MAX, "part": "both"})),
        ("embroidery.copyFrom", json!({"source": 1, "keys": [], "part": "stroke"})),
        ("embroidery.info", json!({"ids": [1], "plan": "yes"})),
        ("embroidery.info", json!({"ids": too_many_ids()})),
        ("embroidery.info", json!({"ids": [1e308, -1], "offset": -1, "limit": 1e9})),
        ("embroidery.info", json!({"plan": true, "selectedOnly": true, "density": [1e308]})),
        ("embroidery.info", json!({"ids": [u64::MAX, 0, 1], "plan": true, "limit": 0})),
        ("embroidery.setup", json!({"fabric": {"tatami": {"pull": 1e308, "spacing": -1}, "color": 5}, "threadWeight": "fine"})),
        ("embroidery.setup", json!({"fabric": {"preset": ["stretchKnit"], "satin": [null]}, "machine": {"speed": 1e308}})),
        ("embroidery.setup", json!({"machine": {"jumpLength": 20, "trimLength": 0.5}, "joinSameColor": 1})),
        ("embroidery.setup", json!({"fillAngle": 1e308, "settings": {"fillAngle": "alongShape"}})),
        ("embroidery.setup", json!({"fillAngle": [f64::NAN], "palette": "pec"})),
        ("embroidery.setup", json!({"fillAngle": "acrossShape", "machine": {"minStitch": -1}})),
        ("embroidery.setup", json!({"fillAngle": {"value": 30}, "settings": {"fillAngle": 1e308}})),
        ("embroidery.setup", json!({"fabric": {"preset": "unknownPreset", "thickness": f64::NAN, "needlePoint": "x".repeat(1000)}})),
        ("embroidery.setup", json!({"fabric": "cotton", "machine": null, "threadWeight": 150, "joinSameColor": false})),
        ("embroidery.fabrics", json!({"id": "cotton"})),
        ("embroidery.applyFabric", json!({"keys": ["angle"], "ids": [1]})),
        ("embroidery.applyFabric", json!({"ids": too_many_ids()})),
        ("embroidery.applyFabric", json!({"keys": "all", "ids": [1e308]})),
        ("embroidery.applyFabric", json!({"keys": ["pull", "pull", null]})),
        ("embroidery.applyFabric", json!({"all": 1})),
        ("embroidery.applyFabric", json!({"all": true, "ids": []})),
        ("embroidery.applyFabric", json!({"all": [true], "keys": ["underlay"]})),
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

/// The most points a lasso may have: the engine's own limit (a selection by `polygon`).
const LASSO_POINTS_MAX: usize = 4096;
/// A zigzag's step along its base and the height of the point that closes it, millimetres.
const ZIGZAG_STEP_MM: f64 = 0.02;
const ZIGZAG_CLOSE_MM: f64 = 100.0;
/// A zigzag whose every edge spans half the lasso's height, and one whose edges span a sliver of
/// it, millimetres.
const ZIGZAG_DEEP_MM: f64 = 100.0;
const ZIGZAG_SHALLOW_MM: f64 = 0.05;

/// A lasso of `n` points: a zigzag `depth` millimetres deep along its base, closed by a point above
/// it.
fn zigzag(n: usize, depth: f64) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = (0..n.saturating_sub(1)).map(|k| [k as f64 * ZIGZAG_STEP_MM, if k % 2 == 0 { 0.0 } else { depth }]).collect();
    points.push([0.0, -ZIGZAG_CLOSE_MM]);
    points
}
