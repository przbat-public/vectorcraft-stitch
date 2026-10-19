//! Real embroidery designs (`cargo xtask corpus --embroidery`, sha256-verified): machine files a
//! person digitized with Ink/Stitch, and the flat artwork each was digitized from.
//!
//! - Every machine file reads, at the size its own header states.
//! - Opened and exported again, every machine file keeps every needle point, trim and colour
//!   block.
//! - Each artwork, digitized by VectorCraft at the size of the person's design, comes out sound:
//!   the same size, every needle point on the artwork, no stitch too long or too short, no more
//!   trims and jumps than its ceilings (just above what was last measured, next to the person's
//!   counts), and a stitch count in a sane share of the person's (VectorCraft's default spacing is
//!   more open than Ink/Stitch's, so it sews fewer stitches).
//!
//! Local runs skip an absent corpus; `EMBROIDERY_CORPUS_REQUIRED=1` requires it. The files are
//! rechecked against the manifest before use.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::{Command, Pattern, quantize};

#[path = "../../../xtask/src/sha256.rs"]
mod sha256;

/// (artwork, the person's machine file, most trims, most jumps). Ceilings one above what was
/// measured when fills got pull compensation (their rows reach 0.2 mm past the edge), with the
/// person's counts: see the comments (trims, then jumps: a move with the needle up, cut or not).
/// (Before the ordering of fill regions and the trim length, when every jump longer than
/// `jumpLength` was cut and regions went nearest first: 55 trims in all; then 25 trims and 69
/// jumps; 56 jumps once a turn along a straight edge was sewn; 26 and 58 with rows 0.38 mm apart;
/// 27 and 57 with pull compensation on fills; now 26 and 57, a region's last row keeping its
/// last stitch.)
const DESIGNS: &[(&str, &str, usize, usize)] = &[
    ("bullseye-twemoji.svg", "bullseye-twemoji-inkstitch.pes", 1, 4), // 0 (the person's design: 6), 3 (8)
    ("fox-twemoji.clipart.svg", "fox-twemoji-inkstitch.pes", 4, 5),   // 3 (6), 4 (3)
    ("heart-suit-twemoji.clipart.svg", "heart-suit-twemoji-inkstitch.pes", 1, 1), // 0 (0), 0 (0)
    ("joystick-twemoji.svg", "joystick-twemoji-institch.pes", 5, 6),  // 4 (4), 5 (4)
    ("pool-8-ball-twemoji.svg", "pool-8-ball-twemoji-inkstitch.pes", 1, 1), // 0 (2), 0 (2)
    ("puzzle-piece-twemoji.svg", "puzzle-piece-twemoji-inkstitch.pes", 1, 1), // 0 (0), 0 (0)
    ("smiling-face-with-smiling-eyes-twemoji.clipart.svg", "smiling-face-with-smiling-eyes-twemoji-inkstitch.pes", 5, 6), // 4 (3), 5 (6)
    // Separate pieces (letters, seeds) are joined by tied jumps, cut only past `trimLength`.
    ("sos-button-fluent.clipart.svg", "sos-button-fluent-inkstitch.pes", 4, 7), // 3 (3), 6 (2)
    ("video-game-twemoji.svg", "video-game-twemoji-inkstitch.pes", 5, 10),      // 4 (11), 9 (12)
    ("watermelon-twemoji.clipart.svg", "watermelon-twemoji-inkstitch.pes", 3, 15), // 2 (2), 14 (2)
    ("zebra-twemoji.clipart.svg", "zebra-twemoji-inkstitch.pes", 7, 12),        // 6 (21), 11 (39)
];

/// The corpus, checked against its manifest; `None` (the tests skip) when it is absent and not
/// required.
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
        let bytes =
            std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("missing corpus file {name}: {e}; run `cargo xtask corpus --embroidery`"));
        assert_eq!(sha256::hex(&bytes), digest, "{name}: corpus file changed; run `cargo xtask corpus --embroidery`");
    }
    Some(dir)
}

fn format_of(path: &Path) -> Format {
    Format::from_extension(path.extension().and_then(|e| e.to_str()).unwrap()).unwrap()
}

fn machine_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("pes" | "jef")))
        .collect();
    v.sort();
    v
}

/// How many times the needle moves to another place with the thread on or cut (a run of jump
/// records is one move).
fn jumps(p: &Pattern) -> usize {
    p.blocks.iter().map(|b| b.stitches.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command != Command::Stitch).count()).sum()
}

/// Each block's needle points and trims, on the formats' 0.1 mm grid: what a machine sews.
fn sewn(p: &Pattern) -> Vec<(Vec<(i32, i32)>, usize)> {
    p.blocks
        .iter()
        .map(|b| {
            let points = b.stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (quantize(s.x), quantize(s.y))).collect();
            (points, b.stitches.iter().filter(|s| s.command == Command::Trim).count())
        })
        .collect()
}

/// The lengths of every stitch sewn (needle down at both ends), millimetres.
fn stitch_lengths(p: &Pattern) -> Vec<f64> {
    p.blocks
        .iter()
        .flat_map(|b| {
            b.stitches.windows(2).filter(|w| w[0].command == Command::Stitch && w[1].command == Command::Stitch).map(|w| w[0].distance(w[1]))
        })
        .collect()
}

fn decode(r: &Value) -> Vec<u8> {
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().unwrap()).unwrap()
}

fn open(s: &mut Session, name: &str, bytes: &[u8]) {
    s.execute("document.open", &json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes)})).unwrap();
}

/// The design size a PES file's PEC block header states, millimetres.
fn pec_header_size(bytes: &[u8]) -> (f64, f64) {
    let pec = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let block = pec + 512;
    let w = u16::from_le_bytes(bytes[block + 8..block + 10].try_into().unwrap());
    let h = u16::from_le_bytes(bytes[block + 10..block + 12].try_into().unwrap());
    (f64::from(w) / 10.0, f64::from(h) / 10.0)
}

#[test]
fn real_machine_files_read_at_their_stated_size() {
    let Some(dir) = corpus() else { return };
    for path in machine_files(&dir) {
        let bytes = std::fs::read(&path).unwrap();
        let format = format_of(&path);
        let name = path.file_name().unwrap().to_string_lossy();
        let p = format.read(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(p.sewn() > 1000, "{name}: {} stitches", p.sewn());
        if format == Format::Pes {
            let b = p.bounds().unwrap();
            let (w, h) = pec_header_size(&bytes);
            assert!(
                (b.width() - w).abs() <= 0.15 && (b.height() - h).abs() <= 0.15,
                "{name}: stitches span {:.1} × {:.1} mm, the header says {w} × {h}",
                b.width(),
                b.height()
            );
        }
    }
}

/// The same design in two formats (Ink/Stitch wrote the fox as PES and JEF) reads the same.
#[test]
fn a_design_reads_the_same_in_pes_and_jef() {
    let Some(dir) = corpus() else { return };
    let read = |name: &str| format_of(Path::new(name)).read(&std::fs::read(dir.join(name)).unwrap()).unwrap();
    let points = |p: &Pattern| sewn(p).into_iter().map(|(pts, _)| pts).collect::<Vec<_>>();
    assert_eq!(points(&read("fox-twemoji-inkstitch.pes")), points(&read("fox-twemoji-inkstitch.jef")));
}

#[test]
fn real_machine_files_are_written_back_unchanged() {
    let Some(dir) = corpus() else { return };
    for path in machine_files(&dir) {
        let bytes = std::fs::read(&path).unwrap();
        let format = format_of(&path);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut s = Session::new();
        open(&mut s, &name, &bytes);
        let again = decode(&s.execute("document.export", &json!({"format": format.id()})).unwrap());
        assert_eq!(sewn(&format.read(&again).unwrap()), sewn(&format.read(&bytes).unwrap()), "{name}");
    }
}

#[test]
fn artwork_digitizes_soundly_next_to_a_persons_design() {
    let Some(dir) = corpus() else { return };
    for &(art, theirs, max_trims, max_jumps) in DESIGNS {
        let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
        let target = person.bounds().unwrap();
        let mut s = Session::new();
        open(&mut s, art, &std::fs::read(dir.join(art)).unwrap());
        // At the person's size: scale the art so its stitched size matches the design's.
        let first = Format::Pes.read(&decode(&s.execute("document.export", &json!({"format": "pes"})).unwrap())).unwrap();
        let k = (target.width() / first.bounds().unwrap().width()).max(target.height() / first.bounds().unwrap().height());
        s.execute("select.all", &json!({})).unwrap();
        s.execute("object.scale", &json!({"sx": k * 100.0, "sy": k * 100.0, "strokes": true})).unwrap();
        let started = std::time::Instant::now();
        let r = s.execute("document.export", &json!({"format": "pes"})).unwrap();
        let took = started.elapsed().as_millis();
        let ours = Format::Pes.read(&decode(&r)).unwrap();
        let b = ours.bounds().unwrap();
        let lengths = stitch_lengths(&ours);
        let longest = lengths.iter().copied().fold(0.0, f64::max);
        let short = lengths.iter().filter(|l| **l > 1e-9 && **l < 0.3).count();
        let trims = ours.points().filter(|p| p.command == Command::Trim).count();
        let person_trims = person.points().filter(|p| p.command == Command::Trim).count();
        let (jumps, person_jumps) = (jumps(&ours), jumps(&person));
        let share = ours.sewn() as f64 / person.sewn() as f64;
        eprintln!(
            "{art:52} {:.1}×{:.1} mm  stitches {:5} ({:.0}% of the person's {})  trims {trims} (person {person_trims})  jumps {jumps} ({person_jumps})  longest {longest:.1} mm  short {short}  {took} ms",
            b.width(),
            b.height(),
            ours.sewn(),
            share * 100.0,
            person.sewn()
        );
        assert!((b.width() - target.width()).abs() < 1.5 && (b.height() - target.height()).abs() < 1.5, "{art}: {b:?} vs {target:?}");
        assert!(longest <= 10.0, "{art}: a {longest:.1} mm stitch");
        assert!(short <= 2, "{art}: {short} stitches shorter than 0.3 mm");
        assert!(trims <= max_trims, "{art}: {trims} trims, more than {max_trims} (the person's design has {person_trims})");
        assert!(jumps <= max_jumps, "{art}: {jumps} jumps, more than {max_jumps} (the person's design has {person_jumps})");
        assert!((0.3..0.8).contains(&share), "{art}: {} stitches, {:.0}% of the person's {}", ours.sewn(), share * 100.0, person.sewn());
    }
}
