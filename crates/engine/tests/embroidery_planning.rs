//! The planning service on the real designs (`cargo xtask corpus --embroidery`): each artwork
//! digitized at the size of the person's design, and the person's design opened as stitches.
//!
//! - `embroidery.stats`, `embroidery.sequence` and `embroidery.check` run on every one and agree
//!   with each other; VectorCraft's own plans never have a stitch the planner leaves out (0.3 mm or
//!   less), one longer than Melco's 12.7 mm, or a satin wider than 10 mm, and their default settings
//!   pass Melco's fill, satin and walk-stitch limits, which opened stitches (no settings) never
//!   fire. The hit counts are printed (`--nocapture`); `docs/embroidery.md` › Checks records them.
//! - A plan after an edit, its unchanged units taken from what earlier plans kept, is the plan a
//!   session that never planned the document makes.
//! - The thread estimate is near Madeira's figures a 1,000 stitches, top and bobbin.
//!
//! Local runs skip an absent corpus; `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};

mod embroidery_common;
use embroidery_common::{DESIGNS, artwork, corpus, persons, run};

/// Points per millimetre: the documents count points, the designs millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// How far an object is moved to edit it, millimetres: a nudge.
const NUDGE: f64 = 0.5;

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
            // Every block and unit listed: the most a reply may hold.
            let stats = run(&mut s, "embroidery.stats", json!({"limit": 10_000}));
            let order = run(&mut s, "embroidery.sequence", json!({"limit": 10_000}));
            assert_eq!((&stats["truncated"], &order["truncated"]), (&json!(false), &json!(false)), "{art} {who}");
            let check = run(&mut s, "embroidery.check", json!({"format": "pes"}));
            let blocks = order["blocks"].as_array().unwrap();
            assert_eq!(stats["blockCount"].as_u64().unwrap() as usize, blocks.len(), "{art} {who}");
            let per_block: u64 = stats["blocks"].as_array().unwrap().iter().map(|b| b["stitches"].as_u64().unwrap()).sum();
            let listed: u64 = blocks.iter().map(|b| b["stitches"].as_u64().unwrap()).sum();
            let units: u64 = blocks.iter().flat_map(|b| b["units"].as_array().unwrap()).map(|u| u["stitches"].as_u64().unwrap()).sum();
            assert_eq!((per_block, listed, units), (stats["stitches"].as_u64().unwrap(), per_block, per_block), "{art} {who}");
            assert_eq!(check["summary"]["errors"]["rules"], json!(0), "{art} {who}: {}", check["rules"]);
            for id in ["fillStitchShort", "satinStitchShort", "runStitchShort"] {
                assert_eq!(hits(&check, id), 0, "{art} {who}: {id}");
            }
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
                .map(|r| format!("{} {} ({} objects)", r["id"].as_str().unwrap(), r["hits"], r["objectCount"]))
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
        let ids: Vec<u64> = run(&mut kept, "embroidery.sequence", json!({"limit": 10_000}))["blocks"]
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
            let after = run(&mut kept, "embroidery.sequence", json!({"limit": 10_000}));
            // The same document, made the same way, in a session that has planned nothing.
            let mut fresh = artwork(&dir, art, theirs);
            run(&mut fresh, "object.move", nudge);
            assert_eq!(after, run(&mut fresh, "embroidery.sequence", json!({"limit": 10_000})), "{art}: after nudging {id}");
            run(&mut kept, "edit.undo", json!({}));
        }
    }
}

/// Madeira's rule of thumb for standard-length stitches, metres of thread a 1,000 stitches: about
/// 5 of top thread and 3 of bobbin (density sheet,
/// <https://www.madeirausa.com/_resources/common/userfiles/file/Resources/TH.Reads/Density.pdf>).
const MADEIRA_TOP: f64 = 5.0;
const MADEIRA_BOBBIN: f64 = 3.0;
/// How far a design's thread a 1,000 needle points may be from Madeira's, as a share: VectorCraft's
/// plans of the real designs come within 12 % of both (4.7 to 5.6 m top, 2.8 to 3.4 m bobbin), the
/// person's designs (denser, shorter stitches) within 39 %.
const FROM_MADEIRA: f64 = 0.4;

/// The thread estimate on the real designs is near Madeira's figures a 1,000 stitches, top and
/// bobbin, for VectorCraft's plans and the person's designs alike.
#[test]
fn thread_on_the_real_designs_is_near_madeiras_figures() {
    let Some(dir) = corpus() else { return };
    let near = |m: f64, madeira: f64| (m - madeira).abs() <= FROM_MADEIRA * madeira;
    for &(art, theirs) in DESIGNS {
        for (who, mut s) in [("VectorCraft", artwork(&dir, art, theirs)), ("person", persons(&dir, theirs))] {
            let v = run(&mut s, "embroidery.stats", json!({}));
            let thousands = v["stitches"].as_f64().unwrap() / 1000.0;
            let (top, bobbin) =
                (v["threadUsage"]["topMetres"].as_f64().unwrap() / thousands, v["threadUsage"]["bobbinMetres"].as_f64().unwrap() / thousands);
            println!("{art} ({who}): {top:.2} m top, {bobbin:.2} m bobbin a 1,000 needle points");
            assert!(near(top, MADEIRA_TOP) && near(bobbin, MADEIRA_BOBBIN), "{art} {who}: {top:.2} m top, {bobbin:.2} m bobbin a 1,000");
        }
    }
}
