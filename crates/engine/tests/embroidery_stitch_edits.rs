//! The stitch commands on every kind of run, and random sequences of them on opened designs:
//!
//! - junk aimed at the manual fixture's real runs (opened, the fixture's own, converted, curved,
//!   locked, inside a symbol) never makes a command panic, and the session stays sound;
//! - every edit, a stitch command or upstream's Delete, Cut Path, Scissors and Join the hooks take
//!   over, on an opened design or the manual fixture, takes at most one undo step, none when it
//!   answers that nothing changed or is refused, and changes nothing without one; undo gives back
//!   the document and the selection bit for bit; the selection never names an anchor that is gone;
//!   a path a stitch command changed is written straight with no empty subpath and holds the needle
//!   points its reply says, and every other path keeps its node;
//! - what is edited is what is sewn: each needle point the design's records name is where the
//!   record sews it, after any edits;
//! - split then join, insert then delete, and a move there and back give the path (or the export)
//!   back.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod embroidery_common;

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_engine::{EngineError, Session};
use vectorcraft_testkit::embroidery::{self as fx, StitchOp, paths_of_stitches};
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
            ("embroidery.moveNeedlePoints", json!({"ranges": [{"id": id, "from": 0, "to": u64::MAX}], "dy": 0.1})),
            ("embroidery.deleteNeedlePoints", json!({"ranges": [{"id": id, "from": u32::MAX, "to": u64::MAX}]})),
            ("embroidery.setMachineFunction", json!({"needlePoints": [{"id": id, "index": u64::MAX}], "function": "trim"})),
            ("embroidery.insertNeedlePoints", json!({"id": id, "index": u64::MAX})),
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
            ("embroidery.selectNeedlePoints", json!({"rect": [-1e6, -1e6, 1e6, 1e6], "ids": [id], "mode": "add"})),
            ("embroidery.selectNeedlePoints", json!({"rect": [0, 0, f64::MAX, 1], "ids": [id]})),
            ("embroidery.selectNeedlePoints", json!({"polygon": [[0, 0], [1e5, 0], [0, 1e5], [1e5, 1e5]], "ids": [id], "mode": "toggle"})),
            ("embroidery.selectNeedlePoints", json!({"polygon": (0..5000).map(|k| json!([k, k % 7])).collect::<Vec<_>>(), "ids": [id]})),
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

/// `command` with `params` on a copy of `doc`, after `prepare` (a selection, say) when given: a
/// caught panic, an internal error or an unsound session after it is a failure.
fn swept(doc: &vectorcraft_doc::Document, prepare: Option<(&str, &Value)>, command: &str, params: &Value, failures: &mut Vec<String>) {
    let mut t = Session::new();
    t.add_document(doc.clone(), None);
    if let Some((c, p)) = prepare {
        let _ = t.execute(c, p);
    }
    match vectorcraft_testkit::catch_quiet(|| t.execute(command, params)) {
        Err(msg) => failures.push(format!("PANIC {command} {params}: {msg}")),
        Ok(Err(e @ EngineError::Internal { .. })) => failures.push(format!("{command} {params}: {e}")),
        Ok(_) => {
            if let Err(e) = check_session(&t) {
                failures.push(format!("{command} {params}: {e}"));
            }
        }
    }
}

/// Upstream's commands the hooks take over (Delete of anchors, Cut Path at Anchors, Scissors, Join,
/// Edit › Clear), Convert, Restore and Stop After, aimed at every kind of run the manual fixture
/// holds, and every embroidery command's structured junk with its ids made the fixture's own:
/// never a panic or an internal error, the session sound after each.
#[test]
fn hooked_commands_convert_restore_and_junk_on_every_kind_of_run_never_panic() {
    let mut s = Session::new();
    let m = fx::manual(&mut runner(&mut s));
    let c = m.converted;
    let doc = s.doc().unwrap().doc.clone();
    let mut ids: Vec<u64> =
        vec![m.opened[0].runs[0], m.one_point, m.double_point, m.trim_before, m.trim_after, m.curved, m.instance, m.locked, m.hidden, m.stopped];
    ids.extend(doc.node(vectorcraft_doc::NodeId(c.fill)).unwrap().children().unwrap().iter().map(|n| n.id.0));
    ids.extend([c.fill, c.group, c.clip_member, m.opened[1].top]);
    let mut failures = Vec::new();
    for (k, &id) in ids.iter().enumerate() {
        let other = ids[(k + 1) % ids.len()];
        for anchor in [[0, 0], [0, 1], [0, 2], [1, 0], [7, 99]] {
            let select = json!({"id": id, "anchors": [anchor]});
            swept(&doc, Some(("select.anchors", &select)), "path.deleteAnchors", &json!({}), &mut failures);
            swept(&doc, Some(("select.anchors", &select)), "path.cutAtAnchors", &json!({}), &mut failures);
            swept(&doc, None, "path.split", &json!({"id": id, "subpath": anchor[0], "anchor": anchor[1]}), &mut failures);
        }
        for (command, params) in [
            ("path.split", json!({"id": id, "subpath": 0, "segment": 0, "t": 0.5})),
            ("path.join", json!({"ids": [id, other]})),
            ("path.join", json!({"ids": [id, id]})),
            ("edit.clear", json!({"ids": [id]})),
            ("embroidery.convertToManual", json!({"ids": [id]})),
            ("embroidery.restoreObject", json!({"ids": [id]})),
            ("embroidery.restoreObject", json!({"ids": [id, other], "dryRun": true})),
            ("embroidery.setStopAfter", json!({"ids": [id]})),
            ("embroidery.setTrimAfter", json!({"ids": [id], "on": false})),
        ] {
            swept(&doc, None, command, &params, &mut failures);
        }
    }
    // The junk every embroidery command takes, aimed at the fixture's own objects.
    for (command, params) in embroidery_common::junk::structured() {
        swept(&doc, None, command, &params, &mut failures);
        let mut aimed = params.clone();
        if let Some(o) = aimed.as_object_mut() {
            if o.contains_key("ids") {
                o.insert("ids".into(), json!(ids));
            }
            if o.contains_key("id") {
                o.insert("id".into(), json!(ids[0]));
            }
        }
        if aimed != params {
            swept(&doc, None, command, &aimed, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

/// Upstream's commands that the fork's hooks turn into stitch edits on a path of stitches: Delete
/// of direct-selected anchors, Cut Path at Anchors, Scissors and Join (⌘J).
#[derive(Clone, Debug)]
enum Hooked {
    Delete { path: usize, at: usize },
    Cut { path: usize, at: usize },
    Scissors { path: usize, at: usize },
    Join { path: usize },
}

impl Hooked {
    /// The anchor selection it needs first (`None`: none), and the command, in `doc`; `None` when
    /// it has no path of stitches to act on.
    fn commands(&self, doc: &vectorcraft_doc::Document) -> Option<(Option<Value>, String, Value)> {
        let paths = paths_of_stitches(doc);
        let path_at = |path: usize| paths.get(path % paths.len().max(1)).map(|(id, _)| *id);
        // An anchor of path `id`: its subpath and its place there.
        let anchor = |id: u64, at: usize| {
            let p = doc.node(vectorcraft_doc::NodeId(id))?.path_data()?.clone();
            let lens: Vec<usize> = p.subpaths.iter().map(|sp| sp.anchors.len()).filter(|n| *n > 0).collect();
            let s = at % lens.len().max(1);
            Some((s, at % lens.get(s)?))
        };
        match *self {
            Hooked::Delete { path, at } | Hooked::Cut { path, at } => {
                let id = path_at(path)?;
                let (s, a) = anchor(id, at)?;
                let command = if matches!(self, Hooked::Delete { .. }) { "path.deleteAnchors" } else { "path.cutAtAnchors" };
                Some((Some(json!({"id": id, "anchors": [[s, a]]})), command.into(), json!({})))
            }
            Hooked::Scissors { path, at } => {
                let id = path_at(path)?;
                let (s, a) = anchor(id, at)?;
                Some((None, "path.split".into(), json!({"id": id, "subpath": s, "anchor": a})))
            }
            Hooked::Join { path } => {
                let (a, b) = (path_at(path)?, path_at(path + 1)?);
                (a != b).then(|| (None, "path.join".into(), json!({"ids": [a, b]})))
            }
        }
    }
}

/// A random edit: a stitch command, or upstream's command a hook turns into one.
#[derive(Clone, Debug)]
enum Edit {
    Stitch(StitchOp),
    Hooked(Hooked),
}

fn arb_edit() -> impl Strategy<Value = Edit> {
    let (path, at) = (0usize..64, 0usize..400);
    prop_oneof![
        6 => vectorcraft_testkit::embroidery::arb_stitch_op().prop_map(Edit::Stitch),
        1 => (path.clone(), at.clone()).prop_map(|(path, at)| Edit::Hooked(Hooked::Delete { path, at })),
        1 => (path.clone(), at.clone()).prop_map(|(path, at)| Edit::Hooked(Hooked::Cut { path, at })),
        1 => (path.clone(), at.clone()).prop_map(|(path, at)| Edit::Hooked(Hooked::Scissors { path, at })),
        1 => path.prop_map(|path| Edit::Hooked(Hooked::Join { path })),
    ]
}

/// Each path of stitches of `doc` by its id, as the document shares it.
fn shared(doc: &vectorcraft_doc::Document) -> std::collections::HashMap<u64, std::sync::Arc<vectorcraft_doc::Node>> {
    fn walk(nodes: &[std::sync::Arc<vectorcraft_doc::Node>], out: &mut std::collections::HashMap<u64, std::sync::Arc<vectorcraft_doc::Node>>) {
        for n in nodes {
            if n.extras().embroidery.as_deref().is_some_and(|e| e.stitches.is_some()) && n.path_data().is_some() {
                out.insert(n.id.0, n.clone());
            }
            if let Some(children) = n.children() {
                walk(children, out);
            }
        }
    }
    let mut out = std::collections::HashMap::new();
    walk(&doc.layers, &mut out);
    out
}

/// The selection's anchors all name anchors their paths hold.
fn selection_is_fresh(s: &Session) -> Result<(), String> {
    let st = s.doc().unwrap();
    for (id, anchors) in &st.selection.anchors {
        let Some(path) = st.doc.node(*id).and_then(|n| n.path_data()) else { return Err(format!("anchors of {id:?}, which holds no path")) };
        for a in anchors {
            let &(subpath, anchor) = a;
            if path.subpaths.get(subpath).is_none_or(|sp| anchor >= sp.anchors.len()) {
                return Err(format!("a stale anchor {a:?} of {id:?}"));
            }
        }
    }
    Ok(())
}

/// A path an edit wrote: no empty subpath, and straight (each anchor a corner: its handles at it).
fn written_straight(n: &vectorcraft_doc::Node) -> Result<(), String> {
    let path = n.path_data().ok_or("no path")?;
    for sp in &path.subpaths {
        if sp.anchors.is_empty() {
            return Err(format!("{:?}: an empty subpath", n.id));
        }
        if sp.anchors.iter().any(|a| a.h_in != a.p || a.h_out != a.p) {
            return Err(format!("{:?}: a handle pulled out", n.id));
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(vectorcraft_testkit::embroidery::proptest_config(24))]

    /// Random stitch edits, and upstream's Delete, Cut Path, Scissors and Join that the hooks turn
    /// into stitch edits, on a design opened from a machine file or on the manual fixture
    /// (converted runs, stops, one-point and tack runs, groups): never a panic; at most one undo
    /// step each, none when refused (the document as it was) and no change without one; undo gives
    /// back the document and the selection bit for bit; the selection never names an anchor that
    /// is gone; every path a stitch command changed is written straight, with no empty subpath, and
    /// holds the needle points its reply says, every other keeping its node; the session stays
    /// sound, every record that names a needle point is sewn where it is, and every format writes
    /// the design.
    #[test]
    fn random_stitch_edits_keep_their_promises(fixture in prop::sample::select(vec!["dst", "pes", "jef", "vp3", "manual"]), edits in prop::collection::vec(arb_edit(), 1..12)) {
        let mut s = Session::new();
        if fixture == "manual" {
            fx::manual(&mut runner(&mut s));
        } else {
            fx::opened_design(&mut runner(&mut s), fixture);
        }
        for edit in &edits {
            let doc = s.doc().unwrap().doc.clone();
            let (command, params, stitch) = match edit {
                Edit::Stitch(op) => {
                    let Some((command, params)) = op.command(&doc) else { continue };
                    (command, params, Some(op))
                }
                Edit::Hooked(h) => {
                    let Some((select, command, params)) = h.commands(&doc) else { continue };
                    if let Some(select) = select {
                        no_panic("select.anchors", &s.execute("select.anchors", &select));
                    }
                    (command, params, None)
                }
            };
            let before = state(&s);
            let nodes = shared(&s.doc().unwrap().doc);
            let steps = s.doc().unwrap().history.undo.len();
            let r = s.execute(&command, &params);
            no_panic(&format!("{command} {params}"), &r);
            check_session(&s).map_err(TestCaseError::fail)?;
            selection_is_fresh(&s).map_err(TestCaseError::fail)?;
            if matches!(stitch, Some(StitchOp::Undo | StitchOp::Select { .. })) {
                continue;
            }
            let after = s.doc().unwrap().history.undo.len();
            if after == steps {
                prop_assert!(state(&s).0 == before.0, "{} {}: no step, so no change", command, params);
            }
            match &r {
                Err(_) => {
                    prop_assert_eq!(after, steps, "{} {}: refused, no step", command, params);
                }
                Ok(v) => {
                    prop_assert!(after <= steps + 1, "{} {}: one step at most", command, params);
                    if v["changedCount"] == json!(0) {
                        prop_assert_eq!(after, steps, "{} {}: nothing changed, no step: {}", command, params, v);
                    }
                    // A stitch command's paths: those it changed written straight and counted as it
                    // says, every other the very node it was.
                    if stitch.is_some() && after == steps + 1 {
                        let now = shared(&s.doc().unwrap().doc);
                        let listed: std::collections::HashMap<u64, u64> = v["paths"].as_array().into_iter().flatten()
                            .filter_map(|p| Some((p["id"].as_u64()?, p["needlePoints"].as_u64()?))).collect();
                        for (id, node) in &now {
                            match (listed.get(id), nodes.get(id)) {
                                (Some(&count), _) => {
                                    written_straight(node).map_err(TestCaseError::fail)?;
                                    let held = paths_of_stitches(&s.doc().unwrap().doc).into_iter().find(|(p, _)| p == id).map(|(_, n)| n as u64);
                                    prop_assert_eq!(held, Some(count), "{} {}: path {} holds what its reply says", command, params, id);
                                }
                                (None, Some(old)) => prop_assert!(std::sync::Arc::ptr_eq(node, old), "{} {}: path {} unchanged, yet copied", command, params, id),
                                (None, None) => {}
                            }
                        }
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
