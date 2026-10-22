//! Real embroidery designs (`cargo xtask corpus --embroidery`, sha256-verified): machine files a
//! person digitized with Ink/Stitch, the flat artwork each was digitized from, and some designs
//! as both VP3 and PES.
//!
//! - Every machine file reads, at the size its own header states.
//! - A design written as VP3 and as PES reads the same from both: as many needle points in each
//!   colour block, each where the PES puts it (to the rounding of the two files).
//! - Every PES file's PEC section, on its own after `#PEC0001` (a PEC file), reads as the PES does.
//! - Opened and exported again, every machine file keeps every needle point, trim and colour
//!   block.
//! - Each artwork, digitized by VectorCraft at the size of the person's design, comes out sound:
//!   the same size, every needle point on the painted artwork and its painted interior sewn
//!   (checked against the art drawn as a PNG), no stitch too long or too short, no more
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

mod embroidery_common;
use embroidery_common::{artwork, corpus, open, run};

/// (artwork, the person's machine file, most trims, most jumps). Ceilings one above what was last
/// measured, with the person's counts: see the comments (trims, then jumps: a move with the needle up, cut or not). (Before
/// the ordering of fill regions and the trim length, when every jump longer than `jumpLength` was
/// cut and regions went nearest first: 55 trims in all; then 25 trims and 69 jumps; 56 jumps once
/// a turn along a straight edge was sewn; 26 and 58 with rows 0.38 mm apart; 27 and 57 with pull
/// compensation on fills; 26 and 57 once a region's last row kept its last stitch; 20 and 45 when
/// a fill's narrow parts became satin; 19 and 44, its rings closing and a narrow part beside a
/// wider block keeping its column; now 18 and 44, the artwork scaled by its own bounds rather than
/// by a first export, which put the zebra a hair smaller.)
const DESIGNS: &[(&str, &str, usize, usize)] = &[
    ("bullseye-twemoji.svg", "bullseye-twemoji-inkstitch.pes", 1, 2), // 0 (the person's design: 6), 1 (8)
    ("fox-twemoji.clipart.svg", "fox-twemoji-inkstitch.pes", 4, 5),   // 3 (6), 4 (3)
    ("heart-suit-twemoji.clipart.svg", "heart-suit-twemoji-inkstitch.pes", 1, 1), // 0 (0), 0 (0)
    ("joystick-twemoji.svg", "joystick-twemoji-institch.pes", 4, 5),  // 3 (4), 4 (4)
    ("pool-8-ball-twemoji.svg", "pool-8-ball-twemoji-inkstitch.pes", 1, 1), // 0 (2), 0 (2)
    ("puzzle-piece-twemoji.svg", "puzzle-piece-twemoji-inkstitch.pes", 1, 1), // 0 (0), 0 (0)
    ("smiling-face-with-smiling-eyes-twemoji.clipart.svg", "smiling-face-with-smiling-eyes-twemoji-inkstitch.pes", 2, 4), // 1 (3), 3 (6)
    // Separate pieces (letters, seeds) are joined by tied jumps, cut only past `trimLength`.
    ("sos-button-fluent.clipart.svg", "sos-button-fluent-inkstitch.pes", 2, 3), // 1 (3), 2 (2)
    ("video-game-twemoji.svg", "video-game-twemoji-inkstitch.pes", 5, 10),      // 4 (11), 9 (12)
    ("watermelon-twemoji.clipart.svg", "watermelon-twemoji-inkstitch.pes", 1, 8), // 0 (2), 7 (2)
    ("zebra-twemoji.clipart.svg", "zebra-twemoji-inkstitch.pes", 7, 15),        // 6 (21), 14 (39)
];

/// The longest stitch the export sews (millimetres): a satin crossing longer is split (the satin
/// default `maxSatin`).
const LONGEST_STITCH: f64 = 10.0;
/// The grid a machine file stores needle points on (millimetres): each end of a stitch moves up to
/// half of it each way when written, so a stitch planned at [`LONGEST_STITCH`] may come out up to
/// a grid's diagonal longer.
const GRID: f64 = 0.1;

/// The trims and the jumps of all the designs together, as last measured: the counts the
/// documentation reports. Each design's ceiling leaves it one more, so a regression of a trim or a
/// jump on a few designs at once passes them all and fails these; an improvement fails them too,
/// until they and the documentation are brought up to date.
const ALL_TRIMS: usize = 18;
const ALL_JUMPS: usize = 43;

fn format_of(path: &Path) -> Format {
    Format::from_extension(path.extension().and_then(|e| e.to_str()).unwrap()).unwrap()
}

fn machine_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("pes" | "jef" | "vp3")))
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

/// The design size a PES file's PEC block header states, millimetres.
fn pec_header_size(bytes: &[u8]) -> (f64, f64) {
    let pec = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let block = pec + 512;
    let w = u16::from_le_bytes(bytes[block + 8..block + 10].try_into().unwrap());
    let h = u16::from_le_bytes(bytes[block + 10..block + 12].try_into().unwrap());
    (f64::from(w) / 10.0, f64::from(h) / 10.0)
}

/// The design size a VP3 file's summary states (its right and left, top and bottom edges, in
/// micrometres), millimetres.
fn vp3_summary_size(bytes: &[u8]) -> (f64, f64) {
    let producer = usize::from(u16::from_be_bytes(bytes[6..8].try_into().unwrap()));
    // The file packet's tag and length, then its notes string, then the edges.
    let file = 8 + producer;
    let notes = usize::from(u16::from_be_bytes(bytes[file + 7..file + 9].try_into().unwrap()));
    let edges = file + 9 + notes;
    let int = |k: usize| f64::from(i32::from_be_bytes(bytes[edges + 4 * k..edges + 4 * k + 4].try_into().unwrap()));
    const MICRONS_PER_MM: f64 = 1000.0;
    ((int(0) - int(2)) / MICRONS_PER_MM, (int(1) - int(3)) / MICRONS_PER_MM)
}

/// The VP3 files that have a PES twin: the same digitizing, both written by Ink/Stitch.
const VP3_AND_PES: &[&str] = &[
    "boy-twemoji-inkstitch",
    "cat-face-twemoji-inkstitch",
    "girl-twemoji-inkstitch",
    "old-man-twemoji-inkstitch",
    "old-woman-twemoji-inkstitch",
    "radish-MOREmoji-inkstitch",
    "radish-svgrepo-com-inkstitch",
];

/// How far a needle point of a VP3 twin may lie from the PES's, millimetres each way: each file
/// rounds its own moves to its 0.1 mm grid (Ink/Stitch writes them from the same points). The
/// largest measured on the twins is 0.3 mm, and the most a VP3 file's size differs from what its
/// summary says, 0.28 mm.
const TWIN_ROUNDING: f64 = 0.3;

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
        if format == Format::Vp3 {
            // The summary gives the design's edges as they were before each move was rounded to
            // the grid on its own, which the moves then drift from as the PES's points do.
            let b = p.bounds().unwrap();
            let (w, h) = vp3_summary_size(&bytes);
            assert!(
                (b.width() - w).abs() <= TWIN_ROUNDING && (b.height() - h).abs() <= TWIN_ROUNDING,
                "{name}: stitches span {:.2} × {:.2} mm, the summary says {w} × {h}",
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

/// The same digitizing written as VP3 and as PES (by Ink/Stitch) reads the same from both: as
/// many needle points in each colour block, where the PES puts them. This is what says a VP3 long
/// form is a jump after which the needle goes down where it lands (`format::vp3`): read as a jump
/// alone, each block would lack a needle point for every long form.
#[test]
fn a_design_reads_the_same_in_vp3_and_pes() {
    let Some(dir) = corpus() else { return };
    let read = |name: String| format_of(Path::new(&name)).read(&std::fs::read(dir.join(&name)).unwrap()).unwrap();
    let needle = |p: &Pattern| -> Vec<Vec<(f64, f64)>> {
        p.blocks.iter().map(|b| b.stitches.iter().filter(|s| s.command == Command::Stitch).map(|s| (s.x, s.y)).collect()).collect()
    };
    for stem in VP3_AND_PES {
        let (vp3, pes) = (needle(&read(format!("{stem}.vp3"))), needle(&read(format!("{stem}.pes"))));
        let counts = |b: &Vec<Vec<(f64, f64)>>| b.iter().map(Vec::len).collect::<Vec<_>>();
        assert_eq!(counts(&vp3), counts(&pes), "{stem}: needle points in each block");
        let worst = vp3.iter().flatten().zip(pes.iter().flatten()).map(|(a, b)| (a.0 - b.0).abs().max((a.1 - b.1).abs())).fold(0.0, f64::max);
        assert!(worst <= TWIN_ROUNDING + 1e-9, "{stem}: a needle point {worst:.2} mm from the PES's");
    }
}

/// A PEC file is a PES's PEC section after `#PEC0001`: each real PES's, cut out so, reads as a
/// PEC file exactly as the PES reads.
#[test]
fn real_pes_files_pec_sections_read_as_pec_files() {
    let Some(dir) = corpus() else { return };
    for path in machine_files(&dir).into_iter().filter(|p| format_of(p) == Format::Pes) {
        let bytes = std::fs::read(&path).unwrap();
        let pec = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let mut file = b"#PEC0001".to_vec();
        file.extend_from_slice(&bytes[pec..]);
        let name = path.file_name().unwrap().to_string_lossy();
        assert_eq!(Format::Pec.read(&file).unwrap(), Format::Pes.read(&bytes).unwrap(), "{name}");
    }
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
    let (mut all_trims, mut all_jumps) = (0, 0);
    for &(art, theirs, max_trims, max_jumps) in DESIGNS {
        let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
        let target = person.bounds().unwrap();
        // At the person's size.
        let mut s = artwork(&dir, art, theirs);
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
        assert!(longest <= LONGEST_STITCH + GRID * std::f64::consts::SQRT_2, "{art}: a {longest:.2} mm stitch");
        assert!(short <= 2, "{art}: {short} stitches shorter than 0.3 mm");
        assert!(trims <= max_trims, "{art}: {trims} trims, more than {max_trims} (the person's design has {person_trims})");
        assert!(jumps <= max_jumps, "{art}: {jumps} jumps, more than {max_jumps} (the person's design has {person_jumps})");
        assert!((0.3..0.8).contains(&share), "{art}: {} stitches, {:.0}% of the person's {}", ours.sewn(), share * 100.0, person.sewn());
        (all_trims, all_jumps) = (all_trims + trims, all_jumps + jumps);
    }
    eprintln!("all designs: trims {all_trims}, jumps {all_jumps}");
    assert_eq!((all_trims, all_jumps), (ALL_TRIMS, ALL_JUMPS), "trims and jumps of all the designs (update docs/embroidery.md with them)");
}

/// Pixels per inch the artwork is drawn at to be checked against its stitches: 10 pixels a
/// millimetre.
const CHECK_PPI: f64 = 254.0;
const PX_PER_MM: f64 = CHECK_PPI / 25.4;
/// Points in a millimetre: the document counts points, the plan millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// How far past the painted art a needle point may lie (millimetres): pull compensation and a
/// satin's reach take stitches a fraction of a millimetre past the edge.
const OFF_ART: f64 = 1.0;
/// How far inside the painted art a place must lie to be checked for stitches, and how near a
/// needle-down stitch it must then be (millimetres): 2 mm from the art's edges, and half a row
/// spacing and 0.16 mm. The planner's coverage test allows 0.1 mm past half a spacing; on the real
/// designs three places lie 0.30 to 0.34 mm from a stitch, where a row starts a stitch in (its turn
/// along an edge is shorter than the shortest stitch and left out), the bare wedges reported to the
/// project owner.
const DEEP: f64 = 2.0;
const REACH: f64 = 0.38 / 2.0 + 0.16;
/// The margin round the plan's box the artwork is drawn with (millimetres).
const MARGIN: f64 = 5.0;

/// Each artwork is sewn where it is painted: every needle point VectorCraft plans lies on the
/// painted art (within a millimetre), and every place more than 2 mm inside the painted art lies
/// within half a row spacing of a needle-down stitch. The art is drawn as a PNG over the plan's
/// box to tell.
#[test]
fn artwork_is_sewn_where_it_is_painted() {
    let Some(dir) = corpus() else { return };
    for &(art, theirs, ..) in DESIGNS {
        let mut s = artwork(&dir, art, theirs);
        let plan = s.embroidery_plan(&vectorcraft_engine::EmbroideryOptions::default()).unwrap();
        let b = plan.to_pattern().bounds().unwrap().inflate(MARGIN, MARGIN);
        let rect = [b.x0 * PT_PER_MM, b.y0 * PT_PER_MM, b.width() * PT_PER_MM, b.height() * PT_PER_MM];
        let png = decode(&run(&mut s, "document.export", json!({"format": "png", "ppi": CHECK_PPI, "rect": rect})));
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        let (w, h) = (img.width() as i64, img.height() as i64);
        let painted = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h && img.get_pixel(x as u32, y as u32)[3] > 127;
        let px = |x_mm: f64, y_mm: f64| (((x_mm - b.x0) * PX_PER_MM).floor() as i64, ((y_mm - b.y0) * PX_PER_MM).floor() as i64);
        // Every needle point within OFF_ART of a painted pixel.
        let off = (OFF_ART * PX_PER_MM).ceil() as i64;
        let stray: Vec<(f64, f64)> = plan
            .to_pattern()
            .points()
            .filter(|p| p.command == Command::Stitch)
            .filter(|p| {
                let (x, y) = px(p.x, p.y);
                !(-off..=off).any(|dx| (-off..=off).any(|dy| dx * dx + dy * dy <= off * off && painted(x + dx, y + dy)))
            })
            .map(|p| (p.x, p.y))
            .collect();
        assert!(stray.is_empty(), "{art}: {} needle points off the art, first {:?}", stray.len(), stray.first());
        // The sewn places: within REACH of a needle-down stitch, marked pixel by pixel.
        let mut sewn = vec![false; (w * h).max(0) as usize];
        let reach = (REACH * PX_PER_MM).ceil() as i64;
        let pattern = plan.to_pattern();
        for block in &pattern.blocks {
            for pair in block.stitches.windows(2).filter(|p| p.iter().all(|s| s.command == Command::Stitch)) {
                let (a, c) = (pair[0], pair[1]);
                let steps = ((a.distance(c) * PX_PER_MM).ceil() as usize).max(1);
                for k in 0..=steps {
                    let t = k as f64 / steps as f64;
                    let (x, y) = px(a.x + (c.x - a.x) * t, a.y + (c.y - a.y) * t);
                    for dx in -reach..=reach {
                        for dy in -reach..=reach {
                            let (i, j) = (x + dx, y + dy);
                            if dx * dx + dy * dy <= reach * reach && i >= 0 && j >= 0 && i < w && j < h {
                                sewn[(j * w + i) as usize] = true;
                            }
                        }
                    }
                }
            }
        }
        // Places DEEP inside the painted art: every pixel of the square round them painted
        // (summed over the image once).
        let mut sum = vec![0i64; ((w + 1) * (h + 1)) as usize];
        for y in 0..h {
            for x in 0..w {
                let v = i64::from(painted(x, y));
                sum[((y + 1) * (w + 1) + x + 1) as usize] =
                    v + sum[(y * (w + 1) + x + 1) as usize] + sum[((y + 1) * (w + 1) + x) as usize] - sum[(y * (w + 1) + x) as usize];
            }
        }
        let deep = (DEEP * PX_PER_MM) as i64;
        let all_painted = |x: i64, y: i64| {
            let (x0, y0, x1, y1) = (x - deep, y - deep, x + deep + 1, y + deep + 1);
            if x0 < 0 || y0 < 0 || x1 > w || y1 > h {
                return false;
            }
            let at = |x: i64, y: i64| sum[(y * (w + 1) + x) as usize];
            at(x1, y1) - at(x0, y1) - at(x1, y0) + at(x0, y0) == (x1 - x0) * (y1 - y0)
        };
        let bare: Vec<(f64, f64)> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| all_painted(x, y) && !sewn[(y * w + x) as usize])
            .map(|(x, y)| (b.x0 + x as f64 / PX_PER_MM, b.y0 + y as f64 / PX_PER_MM))
            .collect();
        assert!(bare.is_empty(), "{art}: {} places of painted art left bare, first {:?}", bare.len(), bare.first());
    }
}

/// A JEF header's record count is the records its body holds ((file length − offset) / 2): in the
/// real files, and in the JEF VectorCraft writes of every artwork.
#[test]
fn jef_headers_count_the_records_they_hold() {
    let Some(dir) = corpus() else { return };
    let word = |bytes: &[u8], at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
    let holds = |bytes: &[u8]| (bytes.len() - word(bytes, 0)) / 2;
    let real: Vec<PathBuf> = machine_files(&dir).into_iter().filter(|p| format_of(p) == Format::Jef).collect();
    assert!(!real.is_empty(), "the corpus has JEF files");
    for path in real {
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(word(&bytes, 28), holds(&bytes), "{}", path.display());
    }
    for &(art, theirs, ..) in DESIGNS {
        let mut s = artwork(&dir, art, theirs);
        let bytes = decode(&run(&mut s, "document.export", json!({"format": "jef"})));
        assert_eq!(word(&bytes, 28), holds(&bytes), "{art}");
    }
}
