//! What the tests on the real embroidery designs share: the corpus (`cargo xtask corpus
//! --embroidery`, checked against its manifest), the designs, and the session helpers.
//!
//! `cargo xtask ci` fetches the corpus and requires it (`EMBROIDERY_CORPUS_REQUIRED=1`), so these
//! tests never pass there without running; a local `cargo test` skips an absent corpus with a note.
// Not every test file uses every helper.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::format::Format;

#[path = "../../../../xtask/src/sha256.rs"]
pub mod sha256;

/// The real designs: each artwork, and the machine file a person digitized from it, whose size the
/// artwork is scaled to.
pub const DESIGNS: &[(&str, &str)] = &[
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

/// The corpus, checked against its manifest; `None` (the test skips) when it is absent and not
/// required.
pub fn corpus() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/embroidery");
    let required = std::env::var_os("EMBROIDERY_CORPUS_REQUIRED").is_some_and(|v| v != "0");
    if !dir.exists() {
        assert!(!required, "required embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        eprintln!("embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        return None;
    }
    for line in include_str!("../../../../xtask/embroidery-corpus.sha256").lines().filter(|l| !l.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("malformed embroidery corpus manifest");
        let bytes =
            std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("missing corpus file {name}: {e}; run `cargo xtask corpus --embroidery`"));
        assert_eq!(sha256::hex(&bytes), digest, "{name}: corpus file changed; run `cargo xtask corpus --embroidery`");
    }
    Some(dir)
}

/// Run `command`, panicking with its params on an error.
pub fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

/// Open `bytes` as a file named `name`.
pub fn open(s: &mut Session, name: &str, bytes: &[u8]) {
    run(s, "document.open", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes)}));
}

/// The document exported as `format` with `options`, or the error.
pub fn export(s: &mut Session, format: Format, options: &Value) -> Result<Vec<u8>, String> {
    let mut p = options.clone();
    p["format"] = json!(format.id());
    let r = s.execute("document.export", &p).map_err(|e| e.to_string())?;
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().ok_or("no dataBase64")?).ok_or_else(|| "dataBase64 is not base64".to_string())
}

/// Millimetres in a point (1/72 in), the unit the documents count in.
const MM_PER_PT: f64 = 25.4 / 72.0;

/// A session holding artwork `art` (in `dir`) scaled to the size of the person's design `theirs`:
/// the art's own bounds (as the renderer measures what it draws) scaled to the design's, so that
/// the scale depends on the art alone, never on how VectorCraft stitches it. Titled after the
/// artwork's file, without its extension, whatever title an opened file gets: the label the
/// machine files carry is the title.
pub fn artwork(dir: &Path, art: &str, theirs: &str) -> Session {
    let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
    let target = person.bounds().unwrap();
    let mut s = Session::new();
    open(&mut s, art, &std::fs::read(dir.join(art)).unwrap());
    run(&mut s, "file.info", json!({"title": art.trim_end_matches(".svg")}));
    run(&mut s, "select.all", json!({}));
    let drawn = run(&mut s, "document.exportSelection", json!({"format": "svg"}))["bounds"].clone();
    let (width, height) = (drawn[2].as_f64().unwrap() * MM_PER_PT, drawn[3].as_f64().unwrap() * MM_PER_PT);
    let k = (target.width() / width).max(target.height() / height);
    run(&mut s, "object.scale", json!({"sx": k * 100.0, "sy": k * 100.0, "strokes": true}));
    run(&mut s, "select.none", json!({}));
    s
}

/// The person's design, opened as stitches.
pub fn persons(dir: &Path, theirs: &str) -> Session {
    let mut s = Session::new();
    open(&mut s, theirs, &std::fs::read(dir.join(theirs)).unwrap());
    s
}
