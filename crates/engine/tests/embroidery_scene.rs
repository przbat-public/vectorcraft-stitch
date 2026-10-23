//! The stitch scene on the real designs (`cargo xtask corpus --embroidery`): each artwork digitized
//! at the size of the person's design, and the person's design opened as stitches. What the scene
//! draws is what `embroidery.stats` counts: its needle points, its jumps and its cuts.
//!
//! Local runs skip an absent corpus; `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use serde_json::json;
use vectorcraft_engine::cmd::embroidery::scene::scene;
use vectorcraft_engine::{EmbroideryOptions, Session};
use vectorcraft_render::lines::MarkKind;

mod embroidery_common;
use embroidery_common::{DESIGNS, artwork, corpus, persons, run};

/// The scene of the active document's plan counts what `embroidery.stats` counts.
fn agrees(s: &mut Session, name: &str) {
    let plan = s.embroidery_plan(&EmbroideryOptions::default()).unwrap();
    let sc = scene(&plan);
    let stats = run(s, "embroidery.stats", json!({"limit": 0}));
    let p = sc.picture();
    let numbers: BTreeSet<u32> = p.lines().iter().flat_map(|l| l.points.clone()).filter_map(|i| p.point(i as usize).map(|(_, n)| n)).collect();
    assert_eq!(numbers.len() as u64, stats["stitches"].as_u64().unwrap(), "{name}: needle points");
    assert_eq!(p.links().len() as u64, stats["jumps"].as_u64().unwrap(), "{name}: jumps");
    let cuts = p.marks().iter().filter(|m| m.kind == MarkKind::Cross).count();
    assert_eq!(cuts as u64, stats["trims"].as_u64().unwrap(), "{name}: cuts");
    assert_eq!(sc.units().len(), plan.units.len(), "{name}: units");
}

#[test]
fn the_scene_draws_what_the_figures_count_on_the_real_designs() {
    let Some(dir) = corpus() else { return };
    for (art, theirs) in DESIGNS {
        agrees(&mut artwork(&dir, art, theirs), art);
        agrees(&mut persons(&dir, theirs), theirs);
    }
}
