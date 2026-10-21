//! The planning service on the real designs (`cargo xtask corpus --embroidery`): each artwork
//! digitized at the size of the person's design, and the person's design opened as stitches.
//!
//! - `embroidery.stats`, `embroidery.sequence` and `embroidery.check` run on every one and agree
//!   with each other; VectorCraft's own plans never have a stitch the planner leaves out (0.3 mm or
//!   less), one longer than Melco's 12.7 mm, or a satin wider than 10 mm. The hit counts are
//!   printed (`--nocapture`); `docs/embroidery.md` › Checks records them.
//! - A plan after an edit, its unchanged units taken from what earlier plans kept, is the plan a
//!   session that never planned the document makes.
//!
//! Local runs skip an absent corpus; `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::format::Format;

#[path = "../../../xtask/src/sha256.rs"]
mod sha256;

/// The real designs: each artwork, and the machine file a person digitized from it.
const DESIGNS: &[(&str, &str)] = &[
    ("bullseye-twemoji.svg", "bullseye-twemoji-inkstitch.pes"),
    ("fox-twemoji.clipart.svg", "fox-twemoji-inkstitch.pes"),
    ("heart-suit-twemoji.clipart.svg", "heart-suit-twemoji-inkstitch.pes"),
    ("joystick-twemoji.svg", "joystick-twemoji-institch.pes"),
    ("pool-8-ball-twemoji.svg", "pool-8-ball-twemoji-inkstitch.pes"),
    ("puzzle-piece-twemoji.svg", "puzzle-piece-twemoji-inkstitch.pes"),
    ("smiling-face-with-smiling-eyes-twemoji.clipart.svg", "smiling-face-with-smiling-eyes-twemoji-inkstitch.pes"),
    ("sos-button-fluent.clipart.svg", "sos-button-fluent-inkstitch.pes"),
    ("video-game-twemoji.svg", "video-game-twemoji-inkstitch.pes"),
    ("watermelon-twemoji.clipart.svg", "watermelon-twemoji-inkstitch.pes"),
    ("zebra-twemoji.clipart.svg", "zebra-twemoji-inkstitch.pes"),
];

/// Points per millimetre: the documents count points, the designs millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// How far an object is moved to edit it, millimetres: a nudge.
const NUDGE: f64 = 0.5;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn export(s: &mut Session, format: Format) -> Vec<u8> {
    let r = run(s, "document.export", json!({"format": format.id()}));
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()
}

fn open(s: &mut Session, name: &str, bytes: &[u8]) {
    run(s, "document.open", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes)}));
}

/// The real designs, checked against their manifest; `None` (the test skips) when they are absent
/// and not required.
fn corpus() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/embroidery");
    let required = std::env::var_os("EMBROIDERY_CORPUS_REQUIRED").is_some_and(|v| v != "0");
    if !dir.exists() {
        assert!(!required, "required embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        eprintln!("embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        return None;
    }
    for line in include_str!("../../../xtask/embroidery-corpus.sha256").lines().filter(|l| !l.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("malformed embroidery corpus manifest");
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("missing corpus file {name}: {e}"));
        assert_eq!(sha256::hex(&bytes), digest, "{name}: corpus file changed; run `cargo xtask corpus --embroidery`");
    }
    Some(dir)
}

/// A session holding artwork `art` scaled to the size of the person's design `theirs`, as
/// `embroidery_output.rs` scales it.
fn artwork(dir: &std::path::Path, art: &str, theirs: &str) -> Session {
    let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
    let target = person.bounds().unwrap();
    let mut s = Session::new();
    open(&mut s, art, &std::fs::read(dir.join(art)).unwrap());
    let first = Format::Pes.read(&export(&mut s, Format::Pes)).unwrap().bounds().unwrap();
    let k = (target.width() / first.width()).max(target.height() / first.height());
    run(&mut s, "select.all", json!({}));
    run(&mut s, "object.scale", json!({"sx": k * 100.0, "sy": k * 100.0, "strokes": true}));
    run(&mut s, "select.none", json!({}));
    s
}

/// The person's design, opened as stitches.
fn persons(dir: &std::path::Path, theirs: &str) -> Session {
    let mut s = Session::new();
    open(&mut s, theirs, &std::fs::read(dir.join(theirs)).unwrap());
    s
}

/// A rule's hits in a check's answer.
fn hits(check: &Value, id: &str) -> u64 {
    check["rules"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).map_or_else(|| panic!("no rule {id}"), |r| r["hits"].as_u64().unwrap())
}

/// The three commands run on every real design and agree with each other; VectorCraft's own plans
/// keep within what its planner promises. Prints each design's hits.
#[test]
fn stats_sequence_and_check_run_on_the_real_designs() {
    let Some(dir) = corpus() else { return };
    for &(art, theirs) in DESIGNS {
        for (who, mut s) in [("VectorCraft", artwork(&dir, art, theirs)), ("person", persons(&dir, theirs))] {
            let stats = run(&mut s, "embroidery.stats", json!({}));
            let order = run(&mut s, "embroidery.sequence", json!({}));
            let check = run(&mut s, "embroidery.check", json!({"format": "pes"}));
            let blocks = order["blocks"].as_array().unwrap();
            assert_eq!(stats["blocks"].as_u64().unwrap() as usize, blocks.len(), "{art} {who}");
            let per_block: u64 = stats["perBlock"].as_array().unwrap().iter().map(|b| b["stitches"].as_u64().unwrap()).sum();
            let listed: u64 = blocks.iter().map(|b| b["stitches"].as_u64().unwrap()).sum();
            let units: u64 = blocks.iter().flat_map(|b| b["units"].as_array().unwrap()).map(|u| u["stitches"].as_u64().unwrap()).sum();
            assert_eq!((per_block, listed, units), (stats["stitches"].as_u64().unwrap(), per_block, per_block), "{art} {who}");
            assert_eq!(check["summary"]["errors"], json!(0), "{art} {who}: {}", check["rules"]);
            if who == "VectorCraft" {
                for id in ["stitchTiny", "stitchTooLong", "satinTooWide", "formatStep", "notSewn"] {
                    assert_eq!(hits(&check, id), 0, "{art}: {id}");
                }
            }
            let fired: Vec<String> = check["rules"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["hits"].as_u64().unwrap() > 0)
                .map(|r| format!("{} {} ({} objects)", r["id"].as_str().unwrap(), r["hits"], r["objects"].as_array().unwrap().len()))
                .collect();
            println!("{art} ({who}): {} needle points, {} blocks; {}", stats["stitches"], blocks.len(), fired.join(", "));
        }
    }
}

/// Nudge each object in turn: the plan made after the nudge, with every unit the nudge didn't
/// change taken from what the plans before kept, is the plan a session that never planned the
/// document makes of the same document.
#[test]
fn a_plan_after_an_edit_is_a_fresh_plan_on_the_real_designs() {
    let Some(dir) = corpus() else { return };
    for &(art, theirs) in DESIGNS {
        let mut kept = artwork(&dir, art, theirs);
        let ids: Vec<u64> = run(&mut kept, "embroidery.sequence", json!({}))["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|b| b["units"].as_array().unwrap().iter().map(|u| u["id"].as_u64().unwrap()))
            .collect();
        let mut ids = ids;
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let nudge = json!({"ids": [id], "dx": NUDGE * PT_PER_MM, "dy": 0});
            run(&mut kept, "object.move", nudge.clone());
            let after = run(&mut kept, "embroidery.sequence", json!({}));
            // The same document, made the same way, in a session that has planned nothing.
            let mut fresh = artwork(&dir, art, theirs);
            run(&mut fresh, "object.move", nudge);
            assert_eq!(after, run(&mut fresh, "embroidery.sequence", json!({})), "{art}: after nudging {id}");
            run(&mut kept, "edit.undo", json!({}));
        }
    }
}
