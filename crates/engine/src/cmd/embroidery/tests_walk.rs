//! The document walk's branches, one test each: paint that isn't sewn, clipping paths that hide
//! everything or nest, symbols that place one another without bound, literal runs beside art of
//! their colour, and the texts and counts the read commands give for them.

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use vectorcraft_color::Paint;
use vectorcraft_doc::NodeId;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::plan::{MAX_SATIN_WIDTH, Params, Style};

use super::sequence::Sequence;
use super::test_support::{blank, run, unit};
use super::*;

/// A rectangle filled with `color`, unstroked → its id.
fn rect(s: &mut Session, color: &str, x: f64, y: f64, w: f64, h: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": color}));
    let id = run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}))["id"].as_u64().unwrap();
    run(s, "select.none", json!({}));
    id
}

/// The warnings a read command gives.
fn warnings(v: &Value) -> Vec<String> {
    v["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_string)).collect()
}

/// A pattern fill isn't sewn, and says so.
#[test]
fn a_pattern_fill_is_not_sewn() {
    let mut s = blank();
    let id = rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    s.edit("Pattern", |d, _| {
        if let Some(fill) = d.node_mut(NodeId(id)).and_then(|n| n.appearance.fill_at_mut(None)) {
            fill.paint = Paint::Pattern { pattern: "Dots".into(), xf: Default::default() };
        }
        Ok(())
    })
    .unwrap();
    let v = run(&mut s, "embroidery.threads", json!({}));
    assert_eq!(v["blockCount"], json!(0));
    assert!(warnings(&v).iter().any(|w| w.contains("patterns")), "{v}");
}

/// A clipping path that encloses nothing hides its clipped art: nothing is sewn.
#[test]
fn an_empty_clipping_path_hides_its_art() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 100.0, 100.0);
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "path.create", json!({"d": "M 40 40 L 80 40"}));
    run(&mut s, "select.all", json!({}));
    run(&mut s, "object.clippingMask.make", json!({}));
    assert_eq!(run(&mut s, "embroidery.threads", json!({}))["blockCount"], json!(0));
}

/// Clip groups inside clip groups sew what both leave visible: their intersection.
#[test]
fn nested_clip_groups_sew_their_intersection() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 0.0, 0.0, 200.0, 200.0);
    let clip = |s: &mut Session, x: f64, y: f64, w: f64, h: f64| {
        run(s, "paint.setFill", json!({"none": true}));
        run(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h}));
        run(s, "select.all", json!({}));
        run(s, "object.clippingMask.make", json!({}));
        run(s, "select.none", json!({}));
    };
    // The left half, then the top half of what that shows.
    clip(&mut s, 0.0, 0.0, 100.0, 200.0);
    clip(&mut s, 0.0, 0.0, 200.0, 100.0);
    let r = run(&mut s, "document.export", json!({"format": "dst"}));
    let p = Format::Dst.read(&vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()).unwrap();
    let b = p.bounds().unwrap();
    // 100 pt is 35.3 mm: the quarter, not a half.
    let quarter = vectorcraft_stitch::pt_to_mm(100.0);
    assert!((b.width() - quarter).abs() < 1.0 && (b.height() - quarter).abs() < 1.0, "{b:?}");
}

/// Symbols that place one another many times at every level (nine symbols, each placing the next
/// ten times: 10^8 placements, each a copy of art) are planned quickly: the walk stops placing
/// symbols past its bound and says so, and the progress estimate counts each symbol once per depth.
/// (A symbol that places itself is taken apart when a file is opened.)
#[test]
fn symbols_placing_one_another_are_walked_within_bounds() {
    let mut s = blank();
    s.edit("Chain", |d, _| {
        let layer = d.layers[0].id;
        for level in 0..9u64 {
            let name = format!("Level {level}");
            let next = format!("Level {}", level + 1);
            let children: Vec<std::sync::Arc<vectorcraft_doc::Node>> = if level == 8 {
                Vec::new()
            } else {
                (0..10u64)
                    .map(|k| {
                        let xf = vectorcraft_geom::Affine::translate((k as f64 * 5.0, 0.0));
                        let kind = vectorcraft_doc::NodeKind::SymbolInstance { symbol: next.clone(), xf };
                        std::sync::Arc::new(vectorcraft_doc::Node::new(NodeId(1_000_000 + level * 100 + k), kind))
                    })
                    .collect()
            };
            let art = std::sync::Arc::new(vectorcraft_doc::Node::group(NodeId(2_000_000 + level), children));
            d.symbols.push(vectorcraft_doc::Symbol { name, art });
        }
        let top = vectorcraft_doc::NodeKind::SymbolInstance { symbol: "Level 0".into(), xf: vectorcraft_geom::Affine::IDENTITY };
        d.insert(Some(layer), 0, vectorcraft_doc::Node::new(NodeId(3_000_000), top))?;
        Ok(())
    })
    .unwrap();
    let start = Instant::now();
    let v = run(&mut s, "embroidery.threads", json!({}));
    assert!(start.elapsed() < Duration::from_secs(30), "{:?}", start.elapsed());
    assert!(warnings(&v).iter().any(|w| w.starts_with("symbols placed more than")), "{v}");
}

/// Art planned in the colour of a run opened from a file never joins the run's block: the file's
/// blocks are written back as they were.
#[test]
fn art_never_joins_an_opened_runs_block() {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    let mut a = Block::new(Thread::rgb([204, 34, 34]));
    for i in 0..10 {
        a.stitches.push(Stitch::new(f64::from(i) * 2.0, 0.0, Command::Stitch));
    }
    let file = Format::Dst.write(&Pattern { blocks: vec![a], ..Pattern::default() }).unwrap();
    let mut s = Session::new();
    s.execute("document.open", &json!({"name": "run.dst", "dataBase64": vectorcraft_format::base64_encode(&file)})).unwrap();
    // The run's colour is DST's first default; art in that very colour, far from it.
    let color = run(&mut s, "embroidery.threads", json!({}))["blocks"][0]["thread"]["art"].as_str().unwrap().to_string();
    rect(&mut s, &color, 200.0, 200.0, 40.0, 30.0);
    assert_eq!(run(&mut s, "embroidery.threads", json!({}))["blockCount"], json!(2));
}

/// A unit with no box joins the last block of its colour only when that block is the last one:
/// with nothing known of where it lies, a block sewn since might be under it.
#[test]
fn a_unit_without_bounds_joins_only_the_last_block() {
    const RED: [u8; 3] = [200, 30, 30];
    let mut seq = Sequence::new(Params::for_style(Style::Running));
    seq.add(unit(1, Style::Running, RED, &[(0.0, 0.0), (2.0, 0.0)], None));
    assert_eq!(seq.target(RED, None), Some(0));
    seq.add(unit(2, Style::Running, [30, 30, 200], &[(50.0, 0.0), (52.0, 0.0)], None));
    assert_eq!(seq.target(RED, None), None);
}

/// The sewing order's sentences for a fill with no satin columns asked for, and for a stroke wider
/// than the widest satin.
#[test]
fn the_sewing_order_says_why_at_the_edges() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc"}));
    run(&mut s, "shape.line", json!({"x1": 20, "y1": 200, "x2": 280, "y2": 200}));
    // 160 pt is 56 mm, past the widest satin.
    run(&mut s, "stroke.set", json!({"weight": 160}));
    let order = run(&mut s, "embroidery.sequence", json!({"narrowAsSatin": 0}));
    let units: Vec<&Value> = order["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).collect();
    let why: Vec<&str> = units.iter().map(|u| u["why"].as_str().unwrap()).collect();
    assert_eq!(why[0], "a fill is tatami (narrowAsSatin 0: no satin columns)");
    assert_eq!(why[1], format!("a stroke at least 1 mm wide is a satin column; this one is sewn {MAX_SATIN_WIDTH} mm wide, the widest"));
    assert_eq!(units[1]["ruleValues"]["widest"], json!(MAX_SATIN_WIDTH));
}

/// The check's summary counts, for each severity, the rules that found something and their hits.
#[test]
fn the_check_summary_counts_rules_and_hits() {
    let mut s = blank();
    rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    rect(&mut s, "#2233cc", 120.0, 20.0, 60.0, 40.0);
    let v = run(&mut s, "embroidery.check", json!({"length": 2, "format": "dst"}));
    let rules = v["rules"].as_array().unwrap();
    for (severity, key) in [("error", "errors"), ("warning", "warnings"), ("info", "info")] {
        let found: Vec<&Value> = rules.iter().filter(|r| r["severity"] == json!(severity) && r["hits"].as_u64().unwrap() > 0).collect();
        let hits: u64 = found.iter().map(|r| r["hits"].as_u64().unwrap()).sum();
        assert_eq!((&v["summary"][key]["rules"], &v["summary"][key]["hits"]), (&json!(found.len()), &json!(hits)), "{severity}");
    }
    // Both rectangles' 2 mm fill stitches are short: two hits of one rule.
    let short = rules.iter().find(|r| r["id"] == json!("fillStitchShort")).unwrap();
    assert_eq!((&short["hits"], &short["objectCount"]), (&json!(2), &json!(2)));
}
