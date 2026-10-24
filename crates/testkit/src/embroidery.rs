//! Embroidery documents for tests, built by commands: shapes in thread colours, the structures a
//! stitch plan maps back to objects (groups, clip groups, symbols placed twice, an object both
//! filled and stroked, locked and hidden art, type), and a design opened from a machine file; and
//! the JSON a newer version might write under an embroidery key ([`arb_json`]).
//!
//! Every builder takes the command runner as a closure (`run(command, params) → reply`, panicking
//! on an error) rather than a session, so that the same fixtures serve the engine's own unit tests
//! (which see the engine crate apart from the one this crate links), its integration tests, the
//! interface's and MCP's: `&mut |c, p| exec(&mut session, c, p)`.
//!
//! The sizes keep away from the planner's thresholds (a satin column's width, `narrowAsSatin`, the
//! stitch length), so that a last-bit difference in a maths library can't flip how a shape is
//! sewn: tests assert relations (the counts the commands give), never the planner's numbers.

use proptest::prelude::*;
use serde_json::{Value, json};

/// Runs one command with its params and gives its reply (panicking on an error).
pub type Run<'a> = &'a mut dyn FnMut(&str, Value) -> Value;

/// Points in a millimetre: the documents count points (1/72 in), the plans millimetres.
pub const PT_PER_MM: f64 = 72.0 / 25.4;

/// The side of the documents [`blank`] makes, in points: room for every fixture below.
pub const PAGE: f64 = 400.0;

/// A stroke weight sewn as a satin column, points: 4 mm, well inside the satin's range (from
/// 1.5 mm, `SATIN_MIN_WIDTH`, to 10 mm, `MAX_SATIN_WIDTH`).
pub const SATIN_WEIGHT: f64 = 4.0 * PT_PER_MM;

/// A stroke weight sewn as a running line, points: 0.5 mm, well under the satin's 1.5 mm.
pub const RUNNING_WEIGHT: f64 = 0.5 * PT_PER_MM;

/// The three thread colours the fixtures paint with: far apart, so that each matches its own
/// thread whichever palette the document uses.
pub const RED: &str = "#d01818";
pub const BLUE: &str = "#1830c8";
pub const GREEN: &str = "#20a030";

/// The id in a command's reply.
pub fn id(reply: &Value) -> u64 {
    reply["id"].as_u64().unwrap_or_else(|| panic!("no id in {reply}"))
}

/// A new [`PAGE`]-sized document with one artboard.
pub fn blank(run: Run) {
    run("file.new", json!({"width": PAGE, "height": PAGE, "artboards": 1}));
}

/// A rectangle filled with `color` and no stroke, at `x, y` (points), `w × h` points; nothing is
/// left selected.
pub fn rect(run: Run, color: &str, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run("paint.setStroke", json!({"none": true}));
    run("paint.setFill", json!({"color": color}));
    let id = id(&run("shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h})));
    run("select.none", json!({}));
    id
}

/// An open path through `points` (points), stroked `weight` points wide in `color`, with no fill;
/// nothing is left selected.
pub fn stroke(run: Run, color: &str, weight: f64, points: &[(f64, f64)]) -> u64 {
    let d: Vec<String> = points.iter().enumerate().map(|(i, (x, y))| format!("{} {x} {y}", if i == 0 { "M" } else { "L" })).collect();
    run("paint.setFill", json!({"none": true}));
    run("paint.setStroke", json!({"color": color}));
    let id = id(&run("path.create", json!({"d": d.join(" ")})));
    run("stroke.set", json!({"weight": weight, "ids": [id]}));
    run("select.none", json!({}));
    id
}

/// A letter T filled with `color`, its top-left corner at `x, y` (points): a fill whose stem is
/// narrow enough to be sewn as a satin column (6 mm, under `narrowAsSatin`'s 7 mm) and whose bar
/// is not (20 mm tall).
pub fn letter_t(run: Run, color: &str, x: f64, y: f64) -> u64 {
    let (bar_w, bar_h, stem_w, stem_h) = (60.0 * PT_PER_MM / 2.0, 20.0 * PT_PER_MM, 6.0 * PT_PER_MM, 30.0 * PT_PER_MM);
    let stem_x = x + (bar_w - stem_w) / 2.0;
    let d = format!(
        "M {x} {y} L {} {y} L {} {} L {} {} L {} {} L {stem_x} {} L {stem_x} {} L {x} {} Z",
        x + bar_w,
        x + bar_w,
        y + bar_h,
        stem_x + stem_w,
        y + bar_h,
        stem_x + stem_w,
        y + bar_h + stem_h,
        y + bar_h + stem_h,
        y + bar_h,
        y + bar_h,
    );
    run("paint.setStroke", json!({"none": true}));
    run("paint.setFill", json!({"color": color}));
    let id = id(&run("path.create", json!({"d": d})));
    run("select.none", json!({}));
    id
}

/// Three squares in three colours, bottom to top red, blue, red. With `overlap` the blue lies on
/// both reds, so red is sewn in two blocks; without, the blue lies apart and both reds join one
/// block, sewn ahead of the blue. Their ids, bottom to top.
pub fn three_threads(run: Run, overlap: bool) -> [u64; 3] {
    let side = 30.0 * PT_PER_MM;
    let red = rect(run, RED, 20.0, 20.0, side, side);
    let (bx, by) = if overlap { (20.0 + side / 2.0, 20.0 + side / 2.0) } else { (20.0, 250.0) };
    let blue = rect(run, BLUE, bx, by, side, side);
    let again = rect(run, RED, 20.0 + side, 20.0 + side / 4.0, side, side);
    [red, blue, again]
}

/// The objects of [`structures`], by what each is.
#[derive(Clone, Copy, Debug)]
pub struct Structures {
    /// A group of two squares (red and blue) and the squares.
    pub group: u64,
    pub grouped: [u64; 2],
    /// A clip group: a circle clipping a red square.
    pub clip_group: u64,
    /// The symbol (a green square) placed twice: both instances.
    pub instances: [u64; 2],
    /// A square both filled (blue) and stroked (red, as a satin column).
    pub filled_and_stroked: u64,
    /// A layer locked with a red square on it.
    pub locked_layer: u64,
    pub locked: u64,
    /// A hidden square.
    pub hidden: u64,
    /// A line of type, which isn't sewn.
    pub text: u64,
}

/// One document holding each structure a sewing unit maps back to its object through: a group, a
/// clip group, a symbol placed twice, an object both filled and stroked, a locked layer's art, a
/// hidden object, and type (left out of the plan, with a warning). Starts a new document.
pub fn structures(run: Run) -> Structures {
    blank(run);
    let side = 20.0 * PT_PER_MM;
    let a = rect(run, RED, 20.0, 20.0, side, side);
    let b = rect(run, BLUE, 20.0 + side * 1.5, 20.0, side, side);
    run("select.set", json!({"ids": [a, b]}));
    let group = id(&run("object.group", json!({})));
    run("select.none", json!({}));

    let clipped = rect(run, RED, 200.0, 20.0, side, side);
    run("paint.setFill", json!({"none": true}));
    run("paint.setStroke", json!({"none": true}));
    let circle = id(&run("shape.ellipse", json!({"x": 200.0 + side / 4.0, "y": 20.0, "width": side, "height": side})));
    run("select.set", json!({"ids": [clipped, circle]}));
    let clip_group = id(&run("object.clippingMask.make", json!({})));
    run("select.none", json!({}));

    let art = rect(run, GREEN, 20.0, 120.0, side, side);
    run("select.set", json!({"ids": [art]}));
    let first = id(&run("symbol.new", json!({"name": "Fixture square"})));
    run("select.none", json!({}));
    let second = id(&run("symbol.place", json!({"name": "Fixture square", "x": 120.0, "y": 120.0})));
    run("select.none", json!({}));

    let both = rect(run, BLUE, 200.0, 120.0, side, side);
    run("paint.setStroke", json!({"color": RED, "ids": [both]}));
    run("stroke.set", json!({"weight": SATIN_WEIGHT, "ids": [both]}));
    run("select.none", json!({}));

    let hidden = rect(run, GREEN, 300.0, 120.0, side, side);
    run("select.set", json!({"ids": [hidden]}));
    run("object.hide", json!({}));
    run("select.none", json!({}));

    let text = id(&run("text.create", json!({"x": 20.0, "y": 300.0, "text": "Type"})));
    run("select.none", json!({}));

    let locked_layer = id(&run("layer.new", json!({})));
    let locked = rect(run, RED, 300.0, 250.0, side, side);
    run("layer.setProps", json!({"id": locked_layer, "locked": true}));
    Structures { group, grouped: [a, b], clip_group, instances: [first, second], filled_and_stroked: both, locked_layer, locked, hidden, text }
}

/// A design opened from a machine file: [`three_threads`] (apart) exported as `format` (a format
/// id, `"pes"`, `"dst"`…) and opened again, so its lines are literal stitch runs. Starts a new
/// document and leaves the opened one active.
pub fn opened_design(run: Run, format: &str) {
    blank(run);
    three_threads(run, false);
    let data = run("document.export", json!({"format": format}))["dataBase64"].as_str().map(str::to_string);
    let data = data.unwrap_or_else(|| panic!("no dataBase64 exporting {format}"));
    run("document.open", json!({"name": format!("design.{format}"), "dataBase64": data}));
}

/// Any JSON value a few levels deep, as a newer version might write under a key this one reads:
/// every type, numbers finite (JSON has no other), keys that include the embroidery keys this
/// version reads (`stitches`, `trim_after`, `trim_before`), so that they come with any value.
pub fn arb_json() -> impl Strategy<Value = Value> {
    /// Quarter steps: floats JSON writes and reads back exactly.
    const QUARTERS: i32 = 4;
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        (-4000i32..4000).prop_map(|n| Value::from(f64::from(n) / f64::from(QUARTERS))),
        "[a-z_]{0,8}".prop_map(Value::from),
    ];
    let key = prop_oneof!["[a-z_]{1,8}", Just("stitches".to_string()), Just("trim_after".to_string()), Just("trim_before".to_string())];
    leaf.prop_recursive(3, 24, 4, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(key.clone(), inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}
