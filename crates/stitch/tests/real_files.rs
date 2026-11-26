//! The formats only read (SEW, HUS), checked on real files on the computer that holds them: the
//! check the project owner can run again on their own files (FORK.md › Decisions register, the
//! 2026-11-18 rows; docs/embroidery.md › Validation), which are never committed. Skipped unless
//! `EMBROIDERY_REAL_FILES` names a folder: every HUS and SEW file under it is read, and what the
//! test says names none of them.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use vectorcraft_stitch::format::{Format, Loss};

/// The variable that names the folder of real files.
const FOLDER: &str = "EMBROIDERY_REAL_FILES";
/// What macOS names the files it writes beside others on some drives (`._` and the file's name):
/// no designs.
const APPLE_DOUBLE: &str = "._";

/// What the check found on the project owner's HUS files, taken out of their archives into a
/// folder (docs/embroidery.md › The formats › SEW and HUS, checked on real files, 2026-11-18): the
/// files, those that open, those refused as holding no stitch (alphabets of moves only), and those
/// that end early (cut short in their last stream).
const OWNERS_HUS: Counts = Counts { files: 6_944, opened: 6_940, no_stitch: 4, ends_early: 2 };
/// The owner's files hold no SEW.
const OWNERS_SEW: Counts = Counts { files: 0, opened: 0, no_stitch: 0, ends_early: 0 };

/// What reading one format's files found.
#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    files: usize,
    opened: usize,
    no_stitch: usize,
    ends_early: usize,
}

/// Every HUS and SEW file under the folder opens, or is refused as holding no stitch, and none is
/// refused for anything else; the counts are those the docs give for the project owner's files
/// (on another folder they differ, and the test says what it found).
#[test]
fn real_hus_and_sew_files_open() {
    let Some(folder) = std::env::var_os(FOLDER) else {
        eprintln!("{FOLDER} names no folder: no real file read");
        return;
    };
    let mut dirs = vec![PathBuf::from(folder)];
    let mut files: Vec<(Format, PathBuf)> = Vec::new();
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if path.is_dir() {
                dirs.push(path);
            } else if let Some(f) = path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension)
                && Format::READ_ONLY.contains(&f)
                && !name.starts_with(APPLE_DOUBLE)
            {
                files.push((f, path));
            }
        }
    }
    let (mut hus, mut sew) = (Counts::default(), Counts::default());
    let mut refused = Vec::new();
    for (k, (format, path)) in files.iter().enumerate() {
        let counts = if *format == Format::Hus { &mut hus } else { &mut sew };
        counts.files += 1;
        let bytes = std::fs::read(path).unwrap();
        match format.read(&bytes) {
            Ok(_) => {
                counts.opened += 1;
                if format.losses(&bytes).contains(&Loss::EndsEarly) {
                    counts.ends_early += 1;
                }
            }
            Err(e) if e.to_string().contains("no stitch records") => counts.no_stitch += 1,
            // The file's place in the walk, not its name.
            Err(e) => refused.push(format!("file {k} ({format:?}): {e}")),
        }
    }
    eprintln!("HUS: {hus:?}\nSEW: {sew:?}");
    assert!(refused.is_empty(), "refused for more than holding no stitch:\n{}", refused.join("\n"));
    assert_eq!((hus, sew), (OWNERS_HUS, OWNERS_SEW), "the counts the docs give for the project owner's files");
}
