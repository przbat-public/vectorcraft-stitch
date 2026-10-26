//! Random documents with embroidery among their edits: the generated editing of the other
//! property tests (`vectorcraft_testkit::strategies::arb_ops`) with embroidery steps between its
//! steps (a cut after random objects, the document's thread palette, start point and label, a
//! machine file placed). Every embroidery step is done (a cut after an object that isn't sewn
//! refused, as the command refuses it), the document saves and reads back as it was (its
//! embroidery included), and its embroidery is read, checked and exported, or refused, without a
//! panic.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_testkit::fixtures;
use vectorcraft_testkit::invariants::check_native_roundtrip;
use vectorcraft_testkit::strategies::arb_ops;

/// One embroidery step.
#[derive(Clone, Debug)]
enum Step {
    /// A cut after the object at an index of the document's objects, on or off (refused for one
    /// that isn't sewn).
    TrimAfter(usize, bool),
    /// The thread palette: Brother's, Janome's or unset.
    Palette(Option<bool>),
    /// The start point, in points, or unset.
    Start(Option<(f64, f64)>),
    /// The design's label, or unset.
    Label(Option<String>),
    /// A small design placed from a machine file: its runs literal stitches.
    Place,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0usize..32, any::<bool>()).prop_map(|(i, on)| Step::TrimAfter(i, on)),
        1 => prop::option::of(any::<bool>()).prop_map(Step::Palette),
        1 => prop::option::of((-50.0..450.0f64, -50.0..350.0f64)).prop_map(Step::Start),
        1 => prop::option::of("[A-Za-z0-9 ]{0,12}").prop_map(Step::Label),
        1 => Just(Step::Place),
    ]
}

/// The ids of the objects on the document's layers, and in its groups.
fn objects(s: &Session) -> Vec<u64> {
    fn walk(n: &vectorcraft_doc::Node, out: &mut Vec<u64>) {
        out.push(n.id.0);
        for c in n.children().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    for layer in &s.doc().unwrap().doc.layers {
        for c in layer.children().into_iter().flatten() {
            walk(c, &mut out);
        }
    }
    out
}

/// Whether a cut after `n` means something, as `embroidery.setTrimAfter` takes it: paths and
/// compound paths, stitches from a file, a symbol's instances, groups and layers are sewn; type,
/// images, placed files, guides and live objects aren't.
fn sewn(n: &vectorcraft_doc::Node) -> bool {
    use vectorcraft_doc::NodeKind;
    matches!(
        n.kind,
        NodeKind::Path { guide: false, .. }
            | NodeKind::Compound { .. }
            | NodeKind::SymbolInstance { .. }
            | NodeKind::Group { .. }
            | NodeKind::Layer { .. }
    )
}

/// A DST of a few stitches in two runs with a cut between them, written here.
fn small_dst() -> String {
    use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};
    let mut b = Block::new(Thread::rgb([200, 30, 30]));
    b.stitches = vec![
        Stitch::new(0.0, 0.0, Command::Stitch),
        Stitch::new(3.0, 0.0, Command::Stitch),
        Stitch::new(3.0, 3.0, Command::Stitch),
        Stitch::new(3.0, 3.0, Command::Trim),
        Stitch::new(20.0, 5.0, Command::Jump),
        Stitch::new(20.0, 5.0, Command::Stitch),
        Stitch::new(24.0, 5.0, Command::Stitch),
    ];
    let p = Pattern { blocks: vec![b], ..Pattern::default() };
    vectorcraft_format::base64_encode(&vectorcraft_stitch::format::Format::Dst.write(&p).unwrap())
}

impl Step {
    fn apply(&self, s: &mut Session) -> vectorcraft_engine::Result<Value> {
        match self {
            Step::TrimAfter(i, on) => {
                let ids = objects(s);
                let Some(&id) = ids.get(i % ids.len().max(1)) else { return Ok(Value::Null) };
                let sewn = s.doc().unwrap().doc.node(vectorcraft_doc::NodeId(id)).is_some_and(sewn);
                match s.execute("embroidery.setTrimAfter", &json!({"on": on, "ids": [id]})) {
                    // Nothing it was given is sewn: refused, with the reason, and nothing changed.
                    Err(vectorcraft_engine::EngineError::BadParams { .. }) if !sewn => Ok(Value::Null),
                    done => done,
                }
            }
            Step::Palette(p) => s.execute("embroidery.setup", &json!({"palette": p.map(|pec| if pec { "pec" } else { "jef" })})),
            Step::Start(p) => s.execute("embroidery.setup", &json!({"start": p.map(|(x, y)| json!([x, y]))})),
            Step::Label(l) => s.execute("embroidery.setup", &json!({"label": l})),
            Step::Place => s.execute("file.place", &json!({"name": "design.dst", "dataBase64": small_dst()})),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// Random editing with embroidery steps among it: the document saves and reads back as it
    /// was, and its embroidery is read, checked and exported, or refused, without a panic.
    #[test]
    fn random_documents_with_embroidery_save_and_read_back(ops in arb_ops(5..30), steps in prop::collection::vec(step(), 1..8)) {
        let mut s = fixtures::session();
        let every = (ops.len() / steps.len()).max(1);
        let mut steps = steps.iter();
        for (k, op) in ops.iter().enumerate() {
            let _ = op.apply(&mut s);
            if k % every == 0 && let Some(step) = steps.next() {
                let done = step.apply(&mut s);
                prop_assert!(done.is_ok(), "{:?}: {:?}", step, done);
            }
        }
        for step in steps {
            let done = step.apply(&mut s);
            prop_assert!(done.is_ok(), "{:?}: {:?}", step, done);
        }
        let d = s.doc().unwrap().doc.clone();
        check_native_roundtrip(&d).map_err(TestCaseError::fail)?;
        for command in ["embroidery.threads", "embroidery.stats", "embroidery.sequence", "embroidery.check", "embroidery.setup"] {
            let _ = s.execute(command, &json!({}));
        }
        for format in ["dst", "pes", "vp3"] {
            let _ = s.execute("document.export", &json!({"format": format}));
        }
    }
}
