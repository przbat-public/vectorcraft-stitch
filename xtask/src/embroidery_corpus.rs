//! `cargo xtask corpus --embroidery`: real embroidery designs from the Emoji Embroidery Project
//! (University of Geneva and Arbores Tech, by Daniel K. Schneider et al.), each checked against
//! `xtask/embroidery-corpus.sha256`. They land in `corpus/embroidery/` (gitignored, never
//! committed). The engine's `embroidery_corpus` test uses them when present.
//!
//! Most designs come as a pair: the original flat artwork (an SVG close to the emoji it is drawn
//! from) and the machine file a person digitized from it with the open-source Ink/Stitch. The
//! machine files test reading real files and writing them back unchanged; the artwork is
//! digitized by VectorCraft and compared with the person's design. Some designs come as VP3 and
//! PES files of one digitizing, which test that the VP3 reader reads what the PES says. Only files
//! made with Ink/Stitch are listed (the project's other machine files come from commercial
//! software).
//!
//! Licences: the designs and machine files are CC BY-SA 4.0, as the mirror's index says
//! (<https://arbores.tech/e/e/>); the project's English page (edutechwiki.unige.ch/en/
//! Emoji_Embroidery_Project) says CC BY-SA in its introduction but CC BY-NC-SA in its Copyright
//! section, so the mirror's statement is the one cited. The artwork derives from Twemoji (CC BY
//! 4.0) or Fluent Emoji (MIT); the two radish designs in `z_candidates` come from MOREmoji and
//! svgrepo.com artwork whose licences the project doesn't state (unverified). They are used
//! locally as test inputs, never redistributed.
//! Files are fetched from the project's mirror at arbores.tech (the same tree as
//! tecfa.unige.ch/guides/embroid/emoji/AAA_EMOJIS). The manifest pins every file's bytes: a file
//! the site changes fails the fetch instead of changing what the tests read. Moving to new files:
//! change the list, re-run, update the manifest with the new hashes after checking the files.
//!
//! `cargo xtask ci` fetches and checks them before the tests ([`for_ci`]) and requires them there
//! (`EMBROIDERY_CORPUS_REQUIRED=1`), so the tests on the real designs never pass without running.

use std::path::Path;
use std::process::Command;

use crate::{root, run, sha256};

/// Where the project's files are served.
const BASE: &str = "https://arbores.tech/e/e";
/// The project's page, cited in SOURCES.md.
const PROJECT: &str = "https://edutechwiki.unige.ch/en/Emoji_Embroidery_Project";

/// (folder in the project, file, licence).
const FILES: &[(&str, &str, &str)] = &[
    ("activities/game", "bullseye-twemoji.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "bullseye-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("activities/game", "heart-suit-twemoji.clipart.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "heart-suit-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("activities/game", "joystick-twemoji.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "joystick-twemoji-institch.pes", "CC BY-SA 4.0"),
    ("activities/game", "pool-8-ball-twemoji.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "pool-8-ball-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("activities/game", "puzzle-piece-twemoji.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "puzzle-piece-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("activities/game", "video-game-twemoji.svg", "Twemoji artwork, CC BY 4.0"),
    ("activities/game", "video-game-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("food_drink/food-fruit", "watermelon-twemoji.clipart.svg", "Twemoji artwork, CC BY 4.0"),
    ("food_drink/food-fruit", "watermelon-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("animals_nature/animal-mammal", "fox-twemoji.clipart.svg", "Twemoji artwork, CC BY 4.0"),
    ("animals_nature/animal-mammal", "fox-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("animals_nature/animal-mammal", "fox-twemoji-inkstitch.jef", "CC BY-SA 4.0"),
    ("animals_nature/animal-mammal", "zebra-twemoji.clipart.svg", "Twemoji artwork, CC BY 4.0"),
    ("animals_nature/animal-mammal", "zebra-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    // The same designs as VP3 and PES (the mouse only as VP3: its PES is another digitizing).
    ("animals_nature/animal-mammal", "cat-face-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("animals_nature/animal-mammal", "cat-face-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("animals_nature/animal-mammal", "mouse-face-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("people_body/person", "boy-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("people_body/person", "boy-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("people_body/person", "girl-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("people_body/person", "girl-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("people_body/person", "old-man-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("people_body/person", "old-man-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("people_body/person", "old-woman-twemoji-inkstitch.vp3", "CC BY-SA 4.0"),
    ("people_body/person", "old-woman-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("z_candidates", "radish-MOREmoji-inkstitch.vp3", "CC BY-SA 4.0 (machine file); artwork licence unverified"),
    ("z_candidates", "radish-MOREmoji-inkstitch.pes", "CC BY-SA 4.0 (machine file); artwork licence unverified"),
    ("z_candidates", "radish-svgrepo-com-inkstitch.vp3", "CC BY-SA 4.0 (machine file); artwork licence unverified"),
    ("z_candidates", "radish-svgrepo-com-inkstitch.pes", "CC BY-SA 4.0 (machine file); artwork licence unverified"),
    ("smileys_emotion/face-smiling", "smiling-face-with-smiling-eyes-twemoji.clipart.svg", "Twemoji artwork, CC BY 4.0"),
    ("smileys_emotion/face-smiling", "smiling-face-with-smiling-eyes-twemoji-inkstitch.pes", "CC BY-SA 4.0"),
    ("symbols/alphanum", "sos-button-fluent.clipart.svg", "Fluent Emoji artwork, MIT"),
    ("symbols/alphanum", "sos-button-fluent-inkstitch.pes", "CC BY-SA 4.0"),
];

const MANIFEST: &str = "xtask/embroidery-corpus.sha256";

/// The variable the engine's tests on the real designs read: `1` requires the corpus (a test fails
/// without it), `0` lets them pass without it.
pub const REQUIRED: &str = "EMBROIDERY_CORPUS_REQUIRED";

/// Whether the gate is told to run without the real designs (`EMBROIDERY_CORPUS_REQUIRED=0`).
fn opted_out() -> bool {
    std::env::var(REQUIRED).is_ok_and(|v| v == "0")
}

/// The gate's step: the real designs fetched (only what is missing) and checked against the
/// manifest, so that the tests that use them run; an error that says how to fetch them when they
/// can't be. Asked to run without them (`EMBROIDERY_CORPUS_REQUIRED=0`), it says so and passes.
pub fn for_ci() -> Result<(), String> {
    if opted_out() {
        eprintln!("embroidery corpus: {REQUIRED}=0, so the tests on the real designs pass without running");
        return Ok(());
    }
    fetch().map_err(|e| {
        format!(
            "{e}\nThe gate runs the embroidery tests on the real designs: fetch them with `cargo xtask corpus --embroidery` (or copy a checked corpus/embroidery in), or set {REQUIRED}=0 to run the gate without them"
        )
    })
}

/// The value the gate's test step gives [`REQUIRED`]: `1` unless the gate was told to run without
/// the real designs.
pub fn required_for_tests() -> &'static str {
    if opted_out() { "0" } else { "1" }
}

pub fn fetch() -> Result<(), String> {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let expected: Vec<(&str, &str)> = manifest.lines().filter_map(|l| l.split_once("  ")).collect();
    let dest = root.join("corpus").join("embroidery");
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut sources = format!(
        "# Embroidery designs\n\nFrom the Emoji Embroidery Project ({PROJECT}), by Daniel K. Schneider et al. (University of Geneva, Arbores Tech).\nFetched by `cargo xtask corpus --embroidery` and checked against `{MANIFEST}`. Gitignored; never commit or redistribute these files.\n\n| File | Source | Licence |\n|---|---|---|\n"
    );
    for (folder, name, licence) in FILES {
        let sha = expected.iter().find(|(_, n)| n == name).map(|(s, _)| *s).ok_or_else(|| format!("{name} is missing from {MANIFEST}"))?;
        let out = dest.join(name);
        let url = format!("{BASE}/{folder}/{name}");
        if !verified(&out, sha) {
            let mut curl = Command::new("curl");
            curl.args(["-fsSL", "--retry", "3", "-A", "VectorCraft-dev", "-o"]).arg(&out).arg(&url);
            run(curl, &format!("curl {url}"))?;
            if !verified(&out, sha) {
                let _ = std::fs::remove_file(&out);
                return Err(format!("{name}: sha256 does not match {MANIFEST}"));
            }
        }
        sources.push_str(&format!("| {name} | {url} | {licence} |\n"));
    }
    std::fs::write(dest.join("SOURCES.md"), sources).map_err(|e| format!("SOURCES.md: {e}"))?;
    println!("Embroidery: {} files verified in {}", FILES.len(), dest.display());
    Ok(())
}

fn verified(path: &Path, sha: &str) -> bool {
    std::fs::read(path).is_ok_and(|b| sha256::hex(&b) == sha)
}
