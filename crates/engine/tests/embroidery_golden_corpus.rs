//! The embroidery export of a seeded corpus of random documents (the random editing of the other
//! property tests, `arb_ops`: shapes, paths, groups, clip groups, compound paths, Pathfinder,
//! strokes, gradients, transparency, effects, transforms), in all eight formats, against hashes of
//! the files' bytes recorded with this file at the merge base of manual stitches (commit c8610efe,
//! before any of their code): an oracle independent of the new code, on documents the pinned
//! exports don't hold.
//!
//! - As it is, every document exports as it did at the merge base: nothing manual stitches added
//!   changes a design that doesn't use them.
//! - Every object that can be converted to manual stitches converted, it exports the same bytes,
//!   whatever its kept original then holds (each original swapped for a rectangle elsewhere before
//!   the export, which restoring then shows): a converted object is sewn as its stitches, never
//!   planned again from its original.
//! - Converted and restored, it exports the same bytes again.
//!
//! To record the hashes again (only for a change meant to move the stitches of designs that use
//! nothing new, said in its commit), at the merge base:
//! `VECTORCRAFT_EMBROIDERY_GOLDEN_RECORD=1 cargo test -p vectorcraft-engine --test embroidery_golden_corpus the_corpus_exports_as_at_the_merge_base`.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Value, json};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_testkit::fixtures;
use vectorcraft_testkit::strategies::{Op, arb_ops};

/// How many documents the corpus holds, and how many editing steps each is made with.
const DOCS: usize = 40;
const STEPS: std::ops::Range<usize> = 5..40;

/// How many objects converting every document of the corpus converts: pinned, since an object
/// that stops being converted stays art that exports the same bytes, which no hash would show.
const CONVERTED_IN_CORPUS: u64 = 132;

/// The corpus's seed: any fixed one.
const SEED: [u8; 32] = *b"vectorcraft-stitch export corpus";

/// The formats every document is exported in.
const FORMATS: [&str; 8] = ["dst", "tap", "t01", "exp", "pes", "pec", "jef", "vp3"];

/// The file the hashes are kept in, beside this one.
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/embroidery_golden_corpus.txt");

/// The rectangle every kept original is swapped for (points: left, top, width, height): away from
/// the corpus's art, so that a stitch planned from it would be elsewhere.
const FOREIGN_RECT: [f64; 4] = [300.0, 200.0, 40.0, 30.0];

/// The corpus's editing, the same on every run and every target.
fn corpus() -> Vec<Vec<Op>> {
    let mut runner = TestRunner::new_with_rng(Config::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &SEED));
    (0..DOCS).map(|_| arb_ops(STEPS).new_tree(&mut runner).unwrap().current()).collect()
}

/// Fails on a command that caught a panic: refused steps are part of random editing, bugs aren't.
fn no_panic(what: &str, r: &vectorcraft_engine::Result<Value>) {
    if let Err(e @ EngineError::Internal { .. }) = r {
        panic!("{what}: {e}");
    }
}

/// A new session with the document `ops` make.
fn document(ops: &[Op]) -> Session {
    let mut s = fixtures::session();
    for op in ops {
        no_panic(&format!("{op:?}"), &op.apply(&mut s));
    }
    s
}

/// FNV-1a, 64 bits: a hash of a file's bytes that no Rust release changes.
fn fnv(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

/// Document `k`'s exports, a line each: the format, then its bytes' hash and length, or that the
/// export was refused.
fn exports(k: usize, s: &mut Session) -> Vec<String> {
    FORMATS
        .iter()
        .map(|format| {
            let r = s.execute("document.export", &json!({ "format": format }));
            no_panic(&format!("document {k}, {format}"), &r);
            match r.ok().and_then(|v| v["dataBase64"].as_str().and_then(vectorcraft_format::base64_decode)) {
                Some(bytes) => format!("{k} {format} {:016x} {}", fnv(&bytes), bytes.len()),
                None => format!("{k} {format} refused"),
            }
        })
        .collect()
}

/// The hashes recorded at the merge base.
fn golden() -> Vec<String> {
    std::fs::read_to_string(GOLDEN).expect("the recorded hashes").lines().map(str::to_string).collect()
}

/// `got` against the recorded lines, the first that differ said.
fn assert_as_recorded(got: &[String], what: &str) {
    let want = golden();
    let differ: Vec<String> = got.iter().zip(&want).filter(|(g, w)| g != w).take(8).map(|(g, w)| format!("{g}  (recorded: {w})")).collect();
    assert!(
        differ.is_empty() && got.len() == want.len(),
        "{what}: {} lines against {}, first differences:\n{}",
        got.len(),
        want.len(),
        differ.join("\n")
    );
}

/// Every object of `s`'s document selected and converted to manual stitches → how many were.
fn convert_all(k: usize, s: &mut Session) -> u64 {
    no_panic(&format!("document {k}: select all"), &s.execute("select.all", &json!({})));
    let r = s.execute("embroidery.convertToManual", &json!({}));
    no_panic(&format!("document {k}: convert"), &r);
    r.map(|v| v["convertedCount"].as_u64().unwrap_or(0)).unwrap_or(0)
}

/// The node of id `id` in a saved file's JSON.
fn node_json(v: &Value, id: u64) -> Option<&Value> {
    if v.get("id").and_then(Value::as_u64) == Some(id) && v.get("kind").is_some() {
        return Some(v);
    }
    match v {
        Value::Object(m) => m.values().find_map(|c| node_json(c, id)),
        Value::Array(a) => a.iter().find_map(|c| node_json(c, id)),
        _ => None,
    }
}

/// A filled rectangle at [`FOREIGN_RECT`], as a file holds it.
fn foreign_node() -> Value {
    let mut s = fixtures::session();
    let [x, y, width, height] = FOREIGN_RECT;
    let id = s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": width, "height": height})).unwrap()["id"].as_u64().unwrap();
    let file: Value = serde_json::from_slice(&vectorcraft_format::save(&s.doc().unwrap().doc, false)).unwrap();
    node_json(&file, id).unwrap().clone()
}

/// Every kept original in `v` (a saved file's JSON) swapped for `foreign` under the original's own
/// id → how many.
fn scramble_originals(v: &mut Value, foreign: &Value) -> usize {
    match v {
        Value::Object(m) => {
            let mut n = 0;
            if let Some(node) = m.get_mut("original").and_then(|o| o.get_mut("node")) {
                let id = node.get("id").cloned().unwrap_or(Value::Null);
                *node = foreign.clone();
                node["id"] = id;
                n += 1;
            }
            n + m.values_mut().map(|c| scramble_originals(c, foreign)).sum::<usize>()
        }
        Value::Array(a) => a.iter_mut().map(|c| scramble_originals(c, foreign)).sum(),
        _ => 0,
    }
}

/// Every document exports as it did at the merge base (or, recording, the hashes are written).
#[test]
fn the_corpus_exports_as_at_the_merge_base() {
    let got: Vec<String> = corpus().iter().enumerate().flat_map(|(k, ops)| exports(k, &mut document(ops))).collect();
    if std::env::var_os("VECTORCRAFT_EMBROIDERY_GOLDEN_RECORD").is_some() {
        std::fs::write(GOLDEN, got.join("\n") + "\n").unwrap();
        return;
    }
    assert_as_recorded(&got, "as it is");
}

/// Every object that can be converted converted, each document exports the bytes it did at the
/// merge base, its kept originals swapped for a rectangle elsewhere first: a converted object is
/// never planned from its original. Restoring them then changes the exports, so the swap is one an
/// export planned from the originals would show.
#[test]
fn converted_the_corpus_exports_the_same_whatever_its_originals_hold() {
    let foreign = foreign_node();
    let (mut converted, mut scrambled, mut shown) = (0, 0, 0);
    let mut got = Vec::new();
    for (k, ops) in corpus().iter().enumerate() {
        let mut s = document(ops);
        let here = convert_all(k, &mut s);
        converted += here;
        let mut file: Value = serde_json::from_slice(&vectorcraft_format::save(&s.doc().unwrap().doc, false)).unwrap();
        scrambled += scramble_originals(&mut file, &foreign);
        let doc = vectorcraft_format::load(&serde_json::to_vec(&file).unwrap()).unwrap();
        let mut swapped = Session::new();
        swapped.add_document(doc, None);
        let sewn = exports(k, &mut swapped);
        if here > 0 {
            no_panic(&format!("document {k}: select all"), &swapped.execute("select.all", &json!({})));
            no_panic(&format!("document {k}: restore"), &swapped.execute("embroidery.restoreObject", &json!({})));
            shown += usize::from(exports(k, &mut swapped) != sewn);
        }
        got.extend(sewn);
    }
    assert_eq!(converted, CONVERTED_IN_CORPUS, "objects converted");
    assert_eq!(scrambled as u64, converted, "{scrambled} originals swapped");
    assert!(shown > 0, "restoring the swapped originals changed no export");
    assert_as_recorded(&got, "converted");
}

/// Converted, then restored, each document exports the bytes it did at the merge base.
#[test]
fn converted_and_restored_the_corpus_exports_the_same() {
    let mut got = Vec::new();
    for (k, ops) in corpus().iter().enumerate() {
        let mut s = document(ops);
        if convert_all(k, &mut s) > 0 {
            no_panic(&format!("document {k}: select all"), &s.execute("select.all", &json!({})));
            let r = s.execute("embroidery.restoreObject", &json!({}));
            no_panic(&format!("document {k}: restore"), &r);
            assert!(r.is_ok(), "document {k}: {r:?}");
        }
        got.extend(exports(k, &mut s));
    }
    assert_as_recorded(&got, "converted and restored");
}
