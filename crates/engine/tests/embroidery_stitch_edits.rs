//! The stitch commands on every kind of run, and random sequences of them on opened designs:
//!
//! - junk aimed at the manual fixture's real runs (opened, the fixture's own, converted, curved,
//!   locked, inside a symbol) never makes a command panic, and the session stays sound;
//! - every edit takes at most one undo step, none when it answers that nothing changed or is
//!   refused (the document then as it was), and undo gives back the document and the selection bit
//!   for bit;
//! - what is edited is what is sewn: each needle point the design's records name is where the
//!   record sews it, after any edits;
//! - split then join, insert then delete, and a move there and back give the path (or the export)
//!   back.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_testkit::embroidery::{self as fx, StitchOp, arb_stitch_ops, paths_of_stitches};
use vectorcraft_testkit::invariants::check_session;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The active document as a file holds it, and the selection.
fn state(s: &Session) -> (Vec<u8>, vectorcraft_doc::Selection) {
    let st = s.doc().unwrap();
    (vectorcraft_format::save(&st.doc, false), st.selection.clone())
}

/// Fails on a command that caught a panic.
fn no_panic(what: &str, r: &vectorcraft_engine::Result<Value>) {
    if let Err(e @ EngineError::Internal { .. }) = r {
        panic!("{what}: {e}");
    }
}

/// Each record `embroidery.stitches` names a needle point of is sewn where that needle point is.
fn records_name_their_needle_points(s: &mut Session) -> Result<(), String> {
    let doc = s.doc().unwrap().doc.clone();
    let mut offset = 0;
    loop {
        let r =
            s.execute("embroidery.stitches", &json!({"offset": offset, "limit": 10_000, "kinds": ["stitch", "jump"]})).map_err(|e| e.to_string())?;
        let rows = r["records"].as_array().cloned().unwrap_or_default();
        for row in &rows {
            let Some(k) = row["index"].as_u64() else { continue };
            let node = doc.node(vectorcraft_doc::NodeId(row["node"].as_u64().unwrap())).ok_or("no node")?;
            let path = node.path_data().ok_or("no path")?;
            let alone = path.subpaths.len() == 1;
            let double = node.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|r| r.double_point);
            let needles: Vec<vectorcraft_geom::Point> = path
                .subpaths
                .iter()
                .flat_map(|sp| match sp.anchors.as_slice() {
                    [a, b] if a.p.distance(b.p) * vectorcraft_stitch::MM_PER_PT < vectorcraft_stitch::SAME_POINT && !(alone && double) => vec![a.p],
                    all => all.iter().map(|a| a.p).collect(),
                })
                .collect();
            let p = needles.get(k as usize).ok_or(format!("{row}: no needle point {k}"))?;
            let at = (row["at"][0].as_f64().unwrap(), row["at"][1].as_f64().unwrap());
            let mm = (p.x * vectorcraft_stitch::MM_PER_PT, p.y * vectorcraft_stitch::MM_PER_PT);
            if (mm.0 - at.0).abs() > 1e-6 || (mm.1 - at.1).abs() > 1e-6 {
                return Err(format!("{row}: needle point {k} is at {mm:?}"));
            }
        }
        offset += rows.len();
        if r["truncated"] != json!(true) {
            return Ok(());
        }
    }
}

/// Junk aimed at every kind of run the manual fixture holds: never a panic, the session sound.
#[test]
fn stitch_commands_on_every_kind_of_run_never_panic() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let c = m.converted;
    let doc = s.doc().unwrap().doc.clone();
    let mut ids: Vec<u64> =
        vec![m.opened[0].runs[0], m.one_point, m.double_point, m.trim_before, m.curved, m.instance, m.locked, m.hidden, m.stopped];
    ids.extend(doc.node(vectorcraft_doc::NodeId(c.fill)).unwrap().children().unwrap().iter().map(|n| n.id.0));
    ids.extend([c.group, c.clip_member, m.opened[1].top]);
    let mut failures = Vec::new();
    for &id in &ids {
        for (command, params) in [
            ("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 0}], "dx": 1e308})),
            ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 1_999_999}], "dy": 0.1})),
            ("embroidery.moveNeedlePoints", json!({"needlePoints": [{"id": id, "index": 1}], "to": [f64::MAX, 0]})),
            ("embroidery.insertNeedlePoints", json!({"id": id, "index": 0, "at": [[0, 0], [1e9, 1e9]]})),
            ("embroidery.insertNeedlePoints", json!({"id": id, "index": 1})),
            ("embroidery.insertNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 50}]})),
            ("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 1_000_000}]})),
            ("embroidery.deleteNeedlePoints", json!({"needlePoints": [{"id": id, "index": 0}]})),
            ("embroidery.splitRun", json!({"ranges": [{"id": id, "from": 0, "to": 30}]})),
            ("embroidery.joinRuns", json!({"ranges": [{"id": id, "from": 0, "to": 30}]})),
            ("embroidery.joinRuns", json!({"ids": [id, id]})),
            ("embroidery.setMachineFunction", json!({"ranges": [{"id": id, "from": 0, "to": 30}], "function": "stop"})),
            ("embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": 2}], "function": "trim", "on": false})),
            ("embroidery.splitLongStitches", json!({"ranges": [{"id": id, "from": 0, "to": 30}], "maxLength": 1})),
            ("embroidery.splitLongStitches", json!({"ranges": [{"id": id, "from": 0, "to": 30}], "format": "pes"})),
            ("embroidery.selectNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": 3}], "mode": "toggle"})),
            ("embroidery.stitches", json!({"ids": [id], "limit": 5})),
        ] {
            let mut t = Session::new();
            t.add_document((*doc).clone(), None);
            let r = vectorcraft_testkit::catch_quiet(|| t.execute(command, &params));
            match r {
                Err(msg) => failures.push(format!("PANIC {command} {params}: {msg}")),
                Ok(Err(e @ EngineError::Internal { .. })) => failures.push(format!("{command} {params}: {e}")),
                Ok(_) => {
                    if let Err(e) = check_session(&t) {
                        failures.push(format!("{command} {params}: {e}"));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]

    /// Random stitch edits on a design opened from a machine file: never a panic; at most one undo
    /// step each, none when refused (the document as it was) or when nothing changed; undo gives
    /// back the document and the selection bit for bit; the session stays sound, every record that
    /// names a needle point is sewn where it is, and every format writes the design.
    #[test]
    fn random_stitch_edits_keep_their_promises(format in prop::sample::select(vec!["dst", "pes", "jef", "vp3"]), ops in arb_stitch_ops(1..12)) {
        let mut s = Session::new();
        fx::opened_design(&mut runner(&mut s), format);
        for op in &ops {
            let doc = s.doc().unwrap().doc.clone();
            let Some((command, params)) = op.command(&doc) else { continue };
            let before = state(&s);
            let steps = s.doc().unwrap().history.undo.len();
            let r = s.execute(&command, &params);
            no_panic(&format!("{command} {params}"), &r);
            check_session(&s).map_err(TestCaseError::fail)?;
            if matches!(op, StitchOp::Undo | StitchOp::Select { .. }) {
                continue;
            }
            let after = s.doc().unwrap().history.undo.len();
            match &r {
                Err(_) => {
                    prop_assert_eq!(after, steps, "{} {}: refused, no step", command, params);
                    prop_assert!(state(&s).0 == before.0, "{} {}: refused, the document as it was", command, params);
                }
                Ok(v) => {
                    prop_assert!(after <= steps + 1, "{} {}: one step at most", command, params);
                    if v["changedCount"] == json!(0) {
                        prop_assert_eq!(after, steps, "{} {}: nothing changed, no step: {}", command, params, v);
                    }
                    if after == steps + 1 {
                        let done = state(&s);
                        s.execute("edit.undo", &json!({})).unwrap();
                        let undone = state(&s);
                        prop_assert!(undone.0 == before.0, "{} {}: undo gives the document back", command, params);
                        prop_assert_eq!(&undone.1, &before.1, "{} {}: and the selection", command, params);
                        s.execute("edit.redo", &json!({})).unwrap();
                        prop_assert!(state(&s).0 == done.0, "{} {}: redo", command, params);
                    }
                }
            }
        }
        records_name_their_needle_points(&mut s).map_err(TestCaseError::fail)?;
        for f in vectorcraft_stitch::format::Format::ALL {
            let r = s.execute("document.export", &json!({"format": f.id()}));
            prop_assert!(r.is_ok() || paths_of_stitches(&s.doc().unwrap().doc).is_empty(), "{}: {:?}", f.id(), r);
        }
    }

    /// On an opened design: split (without ties) then join, and insert then delete, give the path
    /// back as it was; a move there and back gives the export back.
    #[test]
    fn edits_and_their_inverses_give_the_design_back(format in prop::sample::select(vec!["dst", "pes", "jef", "vp3"]), path in 0usize..64, at in 0usize..400, d in (-3.0..3.0f64, -3.0..3.0f64)) {
        let mut s = Session::new();
        fx::opened_design(&mut runner(&mut s), format);
        let paths = paths_of_stitches(&s.doc().unwrap().doc);
        let (id, n) = paths[path % paths.len()];
        prop_assume!(n >= 3);
        let k = 1 + at % (n - 2);
        let saved = state(&s).0;
        let split = s.execute("embroidery.splitRun", &json!({"needlePoints": [{"id": id, "index": k}], "ties": false})).unwrap();
        let landing = split["jumps"][0]["index"].as_u64().unwrap();
        s.execute("embroidery.joinRuns", &json!({"needlePoints": [{"id": id, "index": landing}]})).unwrap();
        prop_assert!(state(&s).0 == saved, "split then join");
        s.execute("embroidery.insertNeedlePoints", &json!({"id": id, "index": k})).unwrap();
        s.execute("embroidery.deleteNeedlePoints", &json!({"needlePoints": [{"id": id, "index": k}]})).unwrap();
        prop_assert!(state(&s).0 == saved, "insert then delete");
        let export = |s: &mut Session| s.execute("document.export", &json!({"format": format})).unwrap()["dataBase64"].clone();
        let before = export(&mut s);
        s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": k}], "dx": d.0, "dy": d.1})).unwrap();
        s.execute("embroidery.moveNeedlePoints", &json!({"needlePoints": [{"id": id, "index": k}], "dx": -d.0, "dy": -d.1})).unwrap();
        prop_assert_eq!(export(&mut s), before, "a move there and back");
    }
}
