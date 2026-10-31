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

/// The seed the fork's property suites run with unless `PROPTEST_RNG_SEED` is set: any fixed one.
const PROPTEST_SEED: u64 = 0x5717_c4ed;

/// A property suite's config, as upstream's pathops suites take theirs: `cases` cases from a fixed
/// seed, so that a CI run never fails at random. Explore further locally with `PROPTEST_CASES=5000
/// PROPTEST_RNG_SEED=<n>`, and keep a minimal failure found that way as a test of its own.
pub fn proptest_config(cases: u32) -> proptest::test_runner::Config {
    let mut c = proptest::test_runner::Config { failure_persistence: None, ..proptest::test_runner::Config::default() };
    if std::env::var_os("PROPTEST_CASES").is_none() {
        c.cases = cases;
    }
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        c.rng_seed = proptest::test_runner::RngSeed::Fixed(PROPTEST_SEED);
    }
    c
}

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

/// The objects of [`points`], by what each is, and the entry and exit points set on them (document
/// points).
#[derive(Clone, Copy, Debug)]
pub struct Points {
    /// A square filled red, its fill's entry and exit set.
    pub fill: u64,
    pub fill_entry: [f64; 2],
    pub fill_exit: [f64; 2],
    /// An open line stroked blue as a satin column, its stroke's entry and exit set at its two
    /// ends.
    pub stroke: u64,
    pub stroke_entry: [f64; 2],
    pub stroke_exit: [f64; 2],
    /// A square filled blue and stroked red as a satin column, both parts' points set.
    pub both: u64,
    pub both_fill_entry: [f64; 2],
    pub both_stroke_exit: [f64; 2],
    /// A compound path (a green square with a square hole), its fill's entry set.
    pub compound: u64,
    pub compound_entry: [f64; 2],
    /// A group of two squares (red and blue), and the squares: no points set.
    pub group: u64,
    pub grouped: [u64; 2],
    /// A clip group (a circle clipping a red square) and the square it clips.
    pub clip_group: u64,
    pub clipped: u64,
    /// A symbol (a green square) placed once: the instance, which takes no points.
    pub instance: u64,
    /// A run of stitches opened from a machine file (a DST's), pasted in: the path (in the group
    /// the file's colour block made).
    pub run: u64,
    /// A red square on a locked layer.
    pub locked: u64,
    /// A hidden red square.
    pub hidden: u64,
    /// A line of type, which takes no settings.
    pub text: u64,
}

/// One document holding each kind of object the embroidery tool meets, with entry and exit points
/// set by `embroidery.setEntryExit` where an object takes them: a fill, a satin stroke, an object
/// filled and stroked (both parts set), a compound path, a group, a clip group, a symbol
/// instance, a run of stitches from a machine file, a locked object, a hidden one and type. Starts
/// a new document (a second one, for the machine file, is opened and closed on the way) and leaves
/// nothing selected.
pub fn points(run: Run) -> Points {
    // The run from a file first: a DST of one red line, opened, its run copied.
    blank(run);
    let line = stroke(run, RED, RUNNING_WEIGHT, &[(40.0, 40.0), (120.0, 40.0), (120.0, 80.0)]);
    let data = run("document.export", json!({"format": "dst"}))["dataBase64"].as_str().map(str::to_string);
    let data = data.unwrap_or_else(|| panic!("no dataBase64 exporting the line {line} as dst"));
    run("document.open", json!({"name": "line.dst", "dataBase64": data}));
    run("select.all", json!({}));
    run("edit.copy", json!({}));
    run("file.close", json!({}));
    run("file.close", json!({}));

    blank(run);
    let side = 20.0 * PT_PER_MM;
    /// The entry and the exit of `part` of object `id` set, each one given.
    fn set(run: Run, id: u64, part: &str, entry: Option<[f64; 2]>, exit: Option<[f64; 2]>) {
        let mut p = json!({"id": id, "part": part});
        if let Some(e) = entry {
            p["entry"] = json!(e);
        }
        if let Some(e) = exit {
            p["exit"] = json!(e);
        }
        run("embroidery.setEntryExit", p);
    }

    let fill = rect(run, RED, 20.0, 20.0, side, side);
    let (fill_entry, fill_exit) = ([20.0, 20.0 + side / 2.0], [20.0 + side, 20.0 + side / 2.0]);
    set(run, fill, "fill", Some(fill_entry), Some(fill_exit));

    let (a, b) = ((120.0, 30.0), (120.0 + 2.0 * side, 30.0));
    let line = stroke(run, BLUE, SATIN_WEIGHT, &[a, b]);
    let (stroke_entry, stroke_exit) = ([a.0, a.1], [b.0, b.1]);
    set(run, line, "stroke", Some(stroke_entry), Some(stroke_exit));

    let both = rect(run, BLUE, 280.0, 20.0, side, side);
    run("paint.setStroke", json!({"color": RED, "ids": [both]}));
    run("stroke.set", json!({"weight": SATIN_WEIGHT, "ids": [both]}));
    run("select.none", json!({}));
    let (both_fill_entry, both_stroke_exit) = ([280.0, 20.0], [280.0 + side, 20.0 + side]);
    set(run, both, "fill", Some(both_fill_entry), None);
    set(run, both, "stroke", None, Some(both_stroke_exit));

    let outer = rect(run, GREEN, 20.0, 120.0, side, side);
    let hole = rect(run, GREEN, 20.0 + side / 4.0, 120.0 + side / 4.0, side / 2.0, side / 2.0);
    run("select.set", json!({"ids": [outer, hole]}));
    let compound = id(&run("object.compoundPath.make", json!({})));
    run("select.none", json!({}));
    let compound_entry = [20.0 + side, 120.0 + side];
    set(run, compound, "fill", Some(compound_entry), None);

    let g1 = rect(run, RED, 120.0, 120.0, side / 2.0, side / 2.0);
    let g2 = rect(run, BLUE, 120.0 + side, 120.0, side / 2.0, side / 2.0);
    run("select.set", json!({"ids": [g1, g2]}));
    let group = id(&run("object.group", json!({})));
    run("select.none", json!({}));

    let clipped = rect(run, RED, 280.0, 120.0, side, side);
    run("paint.setFill", json!({"none": true}));
    run("paint.setStroke", json!({"none": true}));
    let circle = id(&run("shape.ellipse", json!({"x": 280.0 + side / 4.0, "y": 120.0, "width": side, "height": side})));
    run("select.set", json!({"ids": [clipped, circle]}));
    let clip_group = id(&run("object.clippingMask.make", json!({})));
    run("select.none", json!({}));

    let art = rect(run, GREEN, 20.0, 220.0, side, side);
    run("select.set", json!({"ids": [art]}));
    let instance = id(&run("symbol.new", json!({"name": "Points square"})));
    run("select.none", json!({}));

    let pasted = run("edit.paste", json!({}));
    let top = pasted["ids"].get(0).and_then(Value::as_u64).unwrap_or_else(|| panic!("nothing pasted: {pasted}"));
    run("object.move", json!({"ids": [top], "dx": 100.0, "dy": 180.0}));
    run("select.none", json!({}));
    // The file's colour block is a group: its path is the run.
    let mut run_id = top;
    while let Some(child) = run("document.node", json!({"id": run_id, "summary": true}))["children"].get(0).and_then(|c| c["id"].as_u64()) {
        run_id = child;
    }

    let hidden = rect(run, RED, 280.0, 220.0, side, side);
    run("select.set", json!({"ids": [hidden]}));
    run("object.hide", json!({}));
    run("select.none", json!({}));

    let text = id(&run("text.create", json!({"x": 20.0, "y": 330.0, "text": "Type"})));
    run("select.none", json!({}));

    let locked_layer = id(&run("layer.new", json!({})));
    let locked = rect(run, RED, 280.0, 300.0, side / 2.0, side / 2.0);
    run("layer.setProps", json!({"id": locked_layer, "locked": true}));
    Points {
        fill,
        fill_entry,
        fill_exit,
        stroke: line,
        stroke_entry,
        stroke_exit,
        both,
        both_fill_entry,
        both_stroke_exit,
        compound,
        compound_entry,
        group,
        grouped: [g1, g2],
        clip_group,
        clipped,
        instance,
        run: run_id,
        locked,
        hidden,
        text,
    }
}

/// The side of the document [`manual`] makes, points: room for four opened designs side by side
/// ([`three_threads`] apart is about 190 by 320 points) and the runs below them.
pub const MANUAL_PAGE: (f64, f64) = (1700.0, 800.0);
/// How far apart [`manual`] lays the four opened designs, points.
const DESIGN_STEP: f64 = 400.0;
/// Where [`manual`] lays the runs of its own, points from the page's top: below the designs.
const RUNS_TOP: f64 = 500.0;
/// Where [`manual`] lays the objects it converts to manual stitches, points: right of the runs,
/// side by side, two rows.
const CONVERTED_LEFT: f64 = 1100.0;
const CONVERTED_TOP: f64 = 500.0;
/// How far apart, and how large, the converted objects are, points.
const CONVERTED_STEP: f64 = 80.0;
const CONVERTED_SIDE: f64 = 50.0;
/// How far apart [`manual`] lays its own runs, points.
const RUN_STEP: f64 = 60.0;

/// A design opened from a machine file and pasted into [`manual`]'s document: the format it was
/// written in, its top object (the group holding the file's colour blocks' groups) and its runs.
#[derive(Clone, Debug)]
pub struct OpenedRuns {
    pub format: &'static str,
    pub top: u64,
    pub runs: Vec<u64>,
}

/// The objects of [`manual`], by what each is.
#[derive(Clone, Debug)]
pub struct Manual {
    /// [`three_threads`] (apart) written as DST, PES, JEF and VP3 and opened again, each pasted in.
    pub opened: Vec<OpenedRuns>,
    /// A run of one needle point: a line from the point to itself, as a machine file opens it.
    pub one_point: u64,
    /// A run of two needle points at one spot (a tack: `double_point`).
    pub double_point: u64,
    /// A run the thread is cut before (`trim_before`), and one it is cut after (`trim_after`).
    pub trim_before: u64,
    pub trim_after: u64,
    /// A run edited into a curve: its middle anchor's handles pulled out.
    pub curved: u64,
    /// A run inside a symbol's art: the instance placing it.
    pub instance: u64,
    /// A run on a locked layer.
    pub locked: u64,
    /// A hidden run.
    pub hidden: u64,
    /// A run the machine stops in (after its second needle point) and after (`stop_after`).
    pub stopped: u64,
    /// Objects converted to manual stitches, each a group of runs keeping the object's id.
    pub converted: Converted,
}

/// The objects [`manual`] converts to manual stitches, by what each was.
#[derive(Clone, Copy, Debug)]
pub struct Converted {
    /// A filled rectangle.
    pub fill: u64,
    /// A stroked open path (a running line).
    pub stroke: u64,
    /// A rectangle both filled and stroked (a satin column round it).
    pub both: u64,
    /// A compound path of two rectangles.
    pub compound: u64,
    /// A group of two rectangles in two threads, given to the command as the group: each member
    /// converted on its own, so the group holds two converted objects.
    pub group: u64,
    /// The clipped member of a clip group (a rectangle the clipping circle cuts).
    pub clip_member: u64,
    /// An object expanded for sewing: a rectangle with a second stroke.
    pub expanded: u64,
}

/// One document holding each kind of run of literal stitches the stitch commands meet: designs
/// opened from DST, PES, JEF and VP3 files (their colour blocks' groups and runs), runs of one
/// needle point and of two at one spot, runs with a cut before and after, a run the machine stops
/// in and after, a run edited into a curve, a run inside a symbol's art, a locked run and a hidden
/// one, and objects converted to manual stitches ([`Converted`]). Starts a new document,
/// [`MANUAL_PAGE`] in size (others are opened and closed on the way: it is the only one left
/// open, and active), and leaves nothing selected.
pub fn manual(run: Run) -> Manual {
    run("file.new", json!({"width": MANUAL_PAGE.0, "height": MANUAL_PAGE.1, "artboards": 1}));
    let mut opened = Vec::new();
    for (k, format) in ["dst", "pes", "jef", "vp3"].into_iter().enumerate() {
        // The design opened (two documents more), copied, both closed: the fixture is active again.
        opened_design(run, format);
        run("select.all", json!({}));
        run("edit.copy", json!({}));
        run("file.close", json!({}));
        run("file.close", json!({}));
        let top = pasted(run);
        run("object.move", json!({"ids": [top], "dx": DESIGN_STEP * k as f64, "dy": 0.0}));
        run("select.none", json!({}));
        opened.push(OpenedRuns { format, top, runs: runs_under(run, top) });
    }
    // A run of a DST's line, pasted six times: the runs of the fixture's own, made from it below.
    let line = [(0.0, 0.0), (30.0, 0.0), (60.0, 10.0), (90.0, 0.0)];
    blank(run);
    stroke(run, RED, RUNNING_WEIGHT, &line);
    let data = run("document.export", json!({"format": "dst"}))["dataBase64"].as_str().map(str::to_string);
    let data = data.unwrap_or_else(|| panic!("no dataBase64 exporting the line as dst"));
    run("document.open", json!({"name": "line.dst", "dataBase64": data}));
    run("select.all", json!({}));
    run("edit.copy", json!({}));
    run("file.close", json!({}));
    run("file.close", json!({}));
    let mut own = Vec::new();
    for k in 0..6 {
        let top = pasted(run);
        run("object.move", json!({"ids": [top], "dx": 20.0, "dy": RUNS_TOP + RUN_STEP * k as f64}));
        run("select.none", json!({}));
        let runs = runs_under(run, top);
        own.push(*runs.first().unwrap_or_else(|| panic!("no run pasted: {top}")));
    }
    let [one_point, double_point, trim_before, trim_after, curved, hidden] = own.try_into().unwrap_or_else(|o| panic!("six runs: {o:?}"));
    let top = pasted(run);
    run("object.move", json!({"ids": [top], "dx": 900.0, "dy": RUNS_TOP + 4.0 * RUN_STEP}));
    run("select.none", json!({}));
    let stopped = *runs_under(run, top).first().unwrap_or_else(|| panic!("no run pasted: {top}"));
    // The runs edited in the file, as a newer version or a file of another maker might hold them,
    // and opened again: the same ids.
    let data = run("document.serialize", json!({"format": "vectorcraft"}))["dataBase64"].as_str().map(str::to_string);
    let bytes = data.and_then(|d| vectorcraft_format::base64_decode(&d)).unwrap_or_else(|| panic!("no document serialized"));
    let mut file: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("the document isn't JSON: {e}"));
    edit_node(&mut file, one_point, |n| {
        let first = n["kind"]["path"]["subpaths"][0]["anchors"][0].clone();
        n["kind"]["path"]["subpaths"][0]["anchors"] = json!([first.clone(), first]);
    });
    edit_node(&mut file, double_point, |n| {
        let first = n["kind"]["path"]["subpaths"][0]["anchors"][0].clone();
        n["kind"]["path"]["subpaths"][0]["anchors"] = json!([first.clone(), first]);
        n["embroidery"]["stitches"]["double_point"] = json!(true);
    });
    edit_node(&mut file, trim_before, |n| n["embroidery"]["stitches"]["trim_before"] = json!(true));
    edit_node(&mut file, trim_after, |n| n["embroidery"]["stitches"]["trim_after"] = json!(true));
    edit_node(&mut file, stopped, |n| {
        // As this version writes a stop: its needle point's place and where it lies, with how many
        // subpaths and anchors the path has.
        let anchors = n["kind"]["path"]["subpaths"][0]["anchors"].as_array().map_or(0, Vec::len);
        let p = n["kind"]["path"]["subpaths"][0]["anchors"][1]["p"].clone();
        n["embroidery"]["stitches"]["stops"] = json!([[0, 1, p[0], p[1]]]);
        n["embroidery"]["stitches"]["shape"] = json!([1, anchors]);
        n["embroidery"]["stitches"]["stop_after"] = json!(true);
    });
    edit_node(&mut file, curved, |n| {
        let anchors = &mut n["kind"]["path"]["subpaths"][0]["anchors"];
        let middle = anchors.as_array().map_or(0, Vec::len) / 2;
        let p = anchors[middle]["p"].clone();
        let (x, y) = (p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0));
        anchors[middle]["in"] = json!([x - 10.0, y - 10.0]);
        anchors[middle]["out"] = json!([x + 10.0, y + 10.0]);
        anchors[middle]["kind"] = json!("Smooth");
    });
    let edited = vectorcraft_format::base64_encode(&serde_json::to_vec(&file).unwrap_or_default());
    run("document.open", json!({"name": "manual.vectorcraft", "dataBase64": edited}));
    // The built document closed: the edited one is the fixture.
    run("document.activate", json!({"index": 0}));
    run("file.close", json!({}));
    run("select.set", json!({"ids": [hidden]}));
    run("object.hide", json!({}));
    run("select.none", json!({}));
    // A run in a symbol's art, and one on a locked layer: each a DST line's run pasted.
    let top = pasted(run);
    run("object.move", json!({"ids": [top], "dx": 900.0, "dy": RUNS_TOP}));
    let instance = id(&run("symbol.new", json!({"name": "Manual run"})));
    run("select.none", json!({}));
    let converted = converted(run);
    let locked_layer = id(&run("layer.new", json!({})));
    let top = pasted(run);
    run("object.move", json!({"ids": [top], "dx": 900.0, "dy": RUNS_TOP + 2.0 * RUN_STEP}));
    run("select.none", json!({}));
    let locked = *runs_under(run, top).first().unwrap_or_else(|| panic!("no run pasted: {top}"));
    run("layer.setProps", json!({"id": locked_layer, "locked": true}));
    Manual { opened, one_point, double_point, trim_before, trim_after, curved, instance, locked, hidden, stopped, converted }
}

/// The objects [`manual`] converts to manual stitches, drawn right of its runs and converted each
/// on its own, by one command; nothing is left selected.
fn converted(run: Run) -> Converted {
    let at = |k: usize| (CONVERTED_LEFT + CONVERTED_STEP * (k % 4) as f64, CONVERTED_TOP + CONVERTED_STEP * (k / 4) as f64);
    let side = CONVERTED_SIDE;
    let (x, y) = at(0);
    let fill = rect(run, RED, x, y, side, side);
    let (x, y) = at(1);
    let stroke = stroke(run, BLUE, RUNNING_WEIGHT, &[(x, y), (x + side, y + side / 2.0), (x, y + side)]);
    let (x, y) = at(2);
    let both = rect(run, BLUE, x, y, side, side);
    run("paint.setStroke", json!({"color": RED, "ids": [both]}));
    run("stroke.set", json!({"weight": SATIN_WEIGHT, "ids": [both]}));
    run("select.none", json!({}));
    let (x, y) = at(3);
    let outer = rect(run, GREEN, x, y, side, side);
    let inner = rect(run, GREEN, x + side / 4.0, y + side / 4.0, side / 2.0, side / 2.0);
    run("select.set", json!({"ids": [outer, inner]}));
    let compound = id(&run("object.compoundPath.make", json!({})));
    run("select.none", json!({}));
    let (x, y) = at(4);
    let a = rect(run, RED, x, y, side / 2.0, side);
    let b = rect(run, BLUE, x + side / 2.0, y, side / 2.0, side);
    run("select.set", json!({"ids": [a, b]}));
    let group = id(&run("object.group", json!({})));
    run("select.none", json!({}));
    let (x, y) = at(5);
    let clip_member = rect(run, GREEN, x, y, side, side);
    run("paint.setFill", json!({"none": true}));
    run("paint.setStroke", json!({"none": true}));
    let circle = id(&run("shape.ellipse", json!({"x": x + side / 4.0, "y": y, "width": side, "height": side})));
    run("select.set", json!({"ids": [clip_member, circle]}));
    run("object.clippingMask.make", json!({}));
    run("select.none", json!({}));
    let (x, y) = at(6);
    let expanded = rect(run, RED, x, y, side, side);
    run("paint.setStroke", json!({"color": BLUE, "ids": [expanded]}));
    run("appearance.addStroke", json!({"ids": [expanded]}));
    run("select.none", json!({}));
    let given = [fill, stroke, both, compound, group, clip_member, expanded];
    let done = run("embroidery.convertToManual", json!({ "ids": given }));
    // The group's two members, not the group.
    assert_eq!(done["convertedCount"], json!(given.len() + 1), "every fixture object converted: {done}");
    run("select.none", json!({}));
    Converted { fill, stroke, both, compound, group, clip_member, expanded }
}

/// What the clipboard holds, pasted: the id of its top object (left selected).
fn pasted(run: Run) -> u64 {
    let pasted = run("edit.paste", json!({}));
    pasted["ids"].get(0).and_then(Value::as_u64).unwrap_or_else(|| panic!("nothing pasted: {pasted}"))
}

/// The runs of literal stitches under `id` (itself, when it is one), in stacking order.
pub fn runs_under(run: Run, id: u64) -> Vec<u64> {
    let node = run("document.node", json!({"id": id}));
    let mut out = Vec::new();
    collect_runs(&node, &mut out);
    out
}

/// The ids of the runs in the node JSON `node` (`document.node`'s reply), in stacking order.
fn collect_runs(node: &Value, out: &mut Vec<u64>) {
    if node.get("embroidery").and_then(|e| e.get("stitches")).is_some_and(Value::is_object)
        && let Some(id) = node["id"].as_u64()
    {
        out.push(id);
    }
    for child in node["children"].as_array().into_iter().flatten().chain(node["kind"]["children"].as_array().into_iter().flatten()) {
        collect_runs(child, out);
    }
}

/// Edit the node `id` of a saved document's JSON with `f`.
fn edit_node(v: &mut Value, id: u64, f: impl FnOnce(&mut Value) + Copy) {
    if v.get("id").and_then(Value::as_u64) == Some(id) && v.get("kind").is_some() {
        f(v);
        return;
    }
    match v {
        Value::Object(m) => m.values_mut().for_each(|c| edit_node(c, id, f)),
        Value::Array(a) => a.iter_mut().for_each(|c| edit_node(c, id, f)),
        _ => {}
    }
}

/// The keys an object's embroidery and the document's are read by: those this version reads, so
/// that random JSON holds them, with any value, often enough to test what they read as.
const EMBROIDERY_KEYS: [&str; 51] = [
    "stitches",
    "trim_after",
    "trim_before",
    "stop_after",
    "manual",
    "trims",
    "stops",
    "shape",
    "original",
    "frame",
    "node",
    "fingerprint",
    "exact",
    "transformed",
    "fill",
    "stroke",
    "points",
    "entry",
    "exit",
    "stitch",
    "spacing",
    "length",
    "angle",
    "pull",
    "pull_percent",
    "push",
    "expand",
    "narrow_as_satin",
    "underlay",
    "split_length",
    "palette",
    "start",
    "label",
    "fabric",
    "thread_weight",
    "join_same_color",
    "machine",
    "preset",
    "id",
    "values",
    "tatami",
    "satin",
    "spacing_percent",
    "stabilizer",
    "topping",
    "needle_point",
    "thickness",
    "color",
    "jump_length",
    "trim_length",
    "speed",
];

/// Any JSON value a few levels deep, as a newer version might write under a key this one reads:
/// every type, numbers finite (JSON has no other), keys that include the embroidery keys this
/// version reads (`stitches`, `trim_after`, `fill`, `angle`…), so that they come with any value.
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
    let key = prop_oneof!["[a-z_]{1,8}", prop::sample::select(EMBROIDERY_KEYS.to_vec()).prop_map(str::to_string)];
    leaf.prop_recursive(3, 24, 4, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(key.clone(), inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// One random stitch edit ([`arb_stitch_ops`]): its path and needle points picked by place among a
/// document's paths of stitches and their needle points (taken modulo their counts), so that most
/// address what is there and some don't (an index past the end is refused).
#[derive(Clone, Debug)]
pub enum StitchOp {
    Move { path: usize, at: usize, dx: f64, dy: f64 },
    Insert { path: usize, at: usize },
    Delete { path: usize, at: usize, count: usize },
    Split { path: usize, at: usize, ties: bool },
    Join { path: usize, at: usize },
    Function { path: usize, at: usize, stop: bool, on: Option<bool> },
    SplitLong { path: usize, at: usize, max: f64 },
    Select { from: usize, to: usize },
    Undo,
}

/// The paths of stitches of `doc`'s layers, in document order, each with its needle points (its
/// anchors, a run stored as a line to its own start one).
pub fn paths_of_stitches(doc: &vectorcraft_doc::Document) -> Vec<(u64, usize)> {
    runs_of_stitches(doc).into_iter().map(|(id, _, n)| (id, n)).collect()
}

/// [`paths_of_stitches`] with each path's runs: the needle point each starts at.
pub fn runs_of_stitches(doc: &vectorcraft_doc::Document) -> Vec<(u64, Vec<usize>, usize)> {
    let mut out = Vec::new();
    for l in &doc.layers {
        l.walk(&mut |n| {
            let Some(run) = n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()) else { return };
            let Some(path) = n.path_data() else { return };
            let alone = path.subpaths.len() == 1;
            let (mut starts, mut needles) = (Vec::new(), 0usize);
            for sp in &path.subpaths {
                let count = match sp.anchors.as_slice() {
                    [a, b] if a.p == b.p && !(alone && run.double_point) => 1,
                    all => all.len(),
                };
                if count > 0 {
                    starts.push(needles);
                }
                needles += count;
            }
            out.push((n.id.0, starts, needles));
        });
    }
    out
}

impl StitchOp {
    /// The command and params this edit is in `doc` (`None`: no path of stitches to edit).
    pub fn command(&self, doc: &vectorcraft_doc::Document) -> Option<(String, Value)> {
        let runs = runs_of_stitches(doc);
        let paths: Vec<(u64, usize)> = runs.iter().map(|(id, _, n)| (*id, *n)).collect();
        let pick = |path: usize, at: usize| {
            let (id, n) = paths.get(path % paths.len().max(1)).copied()?;
            // One past the end now and then: refused.
            Some((id, at % (n + 1)))
        };
        let c = |id: &str, p: Value| Some((id.to_string(), p));
        match *self {
            StitchOp::Move { path, at, dx, dy } => {
                let (id, k) = pick(path, at)?;
                c("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": k}], "dx": dx, "dy": dy}))
            }
            StitchOp::Insert { path, at } => {
                let (id, k) = pick(path, at)?;
                c("embroidery.insertNeedlePoints", json!({"id": id, "index": k, "at": [[10.0 + k as f64, 20.0]]}))
            }
            StitchOp::Delete { path, at, count } => {
                let (id, k) = pick(path, at)?;
                c("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": k, "to": k + count}]}))
            }
            StitchOp::Split { path, at, ties } => {
                let (id, k) = pick(path, at)?;
                c("embroidery.splitRun", json!({"needlePoints": [{"id": id, "index": k}], "ties": ties}))
            }
            StitchOp::Join { path, at } => {
                let (id, k) = pick(path, at)?;
                // A run's first, where there is more than one run, else wherever.
                let starts = runs.iter().find(|(r, ..)| *r == id).map(|(_, s, _)| s.as_slice()).unwrap_or_default();
                let k = match starts.get(1..).filter(|later| !later.is_empty()) {
                    Some(later) => later.get(at % later.len()).copied().unwrap_or(k),
                    None => k,
                };
                c("embroidery.joinRuns", json!({"needlePoints": [{"id": id, "index": k}]}))
            }
            StitchOp::Function { path, at, stop, on } => {
                let (id, k) = pick(path, at)?;
                c(
                    "embroidery.setMachineFunction",
                    json!({"needlePoints": [{"id": id, "index": k}], "function": if stop { "stop" } else { "trim" }, "on": on}),
                )
            }
            StitchOp::SplitLong { path, at, max } => {
                let (id, k) = pick(path, at)?;
                c("embroidery.splitLongStitches", json!({"ranges": [{"id": id, "from": 0, "to": k}], "maxLength": max}))
            }
            StitchOp::Select { from, to } => c("embroidery.selectNeedlePoints", json!({"records": [from.min(to) + 1, from.max(to) + 1]})),
            StitchOp::Undo => c("edit.undo", json!({})),
        }
    }
}

/// A random stitch edit ([`StitchOp`]).
pub fn arb_stitch_op() -> impl Strategy<Value = StitchOp> {
    let path = 0usize..64;
    let at = 0usize..400;
    prop_oneof![
        4 => (path.clone(), at.clone(), -5.0..5.0f64, -5.0..5.0f64).prop_map(|(path, at, dx, dy)| StitchOp::Move { path, at, dx, dy }),
        2 => (path.clone(), at.clone()).prop_map(|(path, at)| StitchOp::Insert { path, at }),
        2 => (path.clone(), at.clone(), 0usize..4).prop_map(|(path, at, count)| StitchOp::Delete { path, at, count }),
        2 => (path.clone(), at.clone(), any::<bool>()).prop_map(|(path, at, ties)| StitchOp::Split { path, at, ties }),
        2 => (path.clone(), at.clone()).prop_map(|(path, at)| StitchOp::Join { path, at }),
        2 => (path.clone(), at.clone(), any::<bool>(), proptest::option::of(any::<bool>())).prop_map(|(path, at, stop, on)| StitchOp::Function { path, at, stop, on }),
        1 => (path.clone(), at.clone(), 1.0..12.7f64).prop_map(|(path, at, max)| StitchOp::SplitLong { path, at, max }),
        1 => (0usize..2000, 0usize..2000).prop_map(|(from, to)| StitchOp::Select { from, to }),
        1 => Just(StitchOp::Undo),
    ]
}

/// A sequence of random stitch edits.
pub fn arb_stitch_ops(len: std::ops::Range<usize>) -> impl Strategy<Value = Vec<StitchOp>> {
    prop::collection::vec(arb_stitch_op(), len)
}
