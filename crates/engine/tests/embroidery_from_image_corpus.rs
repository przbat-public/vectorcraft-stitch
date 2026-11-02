//! Digitizing from an image on the real designs' artwork (`cargo xtask corpus --embroidery`,
//! checked against its manifest), measured against three references: the art itself, for shape and
//! colour; VectorCraft's own digitizing of the same art (the vector path), so that what differs is
//! what the image stage loses; and the person's design, for counts. Each artwork, at the size of
//! the person's design, is drawn by the engine as a PNG at 10 pixels a millimetre over its box and
//! a 5 mm margin, placed over that same box, and digitized by Create in the art's own number of
//! colours, in Brother's palette (the person's designs are PES).
//!
//! For every design, with the thresholds calibrated on the artwork before any of this code was
//! written (2026-11-02):
//! - the silhouette's IoU against the art (needle-down stitches drawn 0.4 mm wide, where 40 wt
//!   threads touch) is at least the vector path's less [`IOU_MARGIN`];
//! - the colour agreement (each flat pixel of the art under the thread the vector path sews its
//!   colour in) at least the vector path's less [`AGREEMENT_MARGIN`] (the image path paints its
//!   objects in their threads' colours, which can lie nearer another of the art's colours, so both
//!   are measured by thread);
//! - the needle points are [`STITCHES_RANGE`] times the vector path's (the bullseye's art stacks
//!   whole discs, which the vector path sews one over another and the image path as abutting
//!   rings: 0.68 times);
//! - the threads number within one of the person's;
//! - the check finds no error;
//! - the dry run's figures are Create's.
//!
//! A table of the measures is printed. Local runs skip an absent corpus;
//! `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use vectorcraft_engine::EmbroideryOptions;
use vectorcraft_stitch::format::Format;

mod embroidery_common;
use embroidery_common::images::{Art, Sewn, art_picture, cover, painted_centre, placed_over, thread_of};
use embroidery_common::{DESIGNS, artwork, corpus, run};

/// How much lower than the vector path's the image path's silhouette IoU may be: 0.02, the
/// largest gap measured before the overlap was 0.013 (the video game), with room for the
/// renderer's anti-aliasing to differ between systems.
const IOU_MARGIN: f64 = 0.02;

/// How much lower than the vector path's its colour agreement may be: 0.02, the largest gap
/// measured 0.011.
const AGREEMENT_MARGIN: f64 = 0.02;

/// The needle points the image path may sew, as a share of the vector path's: the bullseye's
/// abutting rings sew 0.68 of its stacked discs; an overlap of 0.4 mm adds a little elsewhere.
const STITCHES_RANGE: (f64, f64) = (0.6, 1.4);

/// How far the threads may number from the person's: the art's colours and the person's threads
/// differ by one at most on every design.
const THREADS_OFF: usize = 1;

/// The fewest colours the command takes: a design of one colour is asked for two, the
/// anti-aliased edge the second, which joins the first as a sliver.
const MIN_COLORS: usize = 2;

#[test]
fn digitized_artwork_stays_near_the_vector_path() {
    let Some(dir) = corpus() else { return };
    let mut failed = Vec::new();
    eprintln!("design\tcolours\tIoU vector, image\tagreement vector, image\tstitches vector, image (×)\tthreads person, image");
    for &(art_name, theirs) in DESIGNS {
        let design = art_name.split('-').next().unwrap();
        let mut s = artwork(&dir, art_name, theirs);
        run(&mut s, "embroidery.setup", json!({"palette": "pec"}));
        let vector = s.embroidery_plan(&EmbroideryOptions::default()).unwrap().to_pattern();
        // The art's colours, each with the thread the vector path sews it in.
        let (mut colours, mut threads): (Vec<[u8; 3]>, Vec<Option<usize>>) = (Vec::new(), Vec::new());
        for b in &vector.blocks {
            if !colours.contains(&b.thread.color) {
                colours.push(b.thread.color);
                threads.push(thread_of(b));
            }
        }
        let (rect, png) = art_picture(&mut s);
        let mut art = Art::of(&png, rect, colours.clone());
        let mut image = placed_over("art.png", &png, rect);
        run(&mut image, "embroidery.setup", json!({"palette": "pec"}));
        let params = json!({"colors": colours.len().max(MIN_COLORS)});
        let mut dry = params.clone();
        dry["dryRun"] = json!(true);
        let answered = run(&mut image, "embroidery.digitizeImage", dry);
        let made = run(&mut image, "embroidery.digitizeImage", params);
        if answered["stats"] != made["stats"] {
            failed.push(format!("{design}: the dry run's figures aren't Create's"));
        }
        let digitized = image.embroidery_plan(&EmbroideryOptions::default()).unwrap().to_pattern();
        let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
        let by_thread = Sewn::Thread(&threads);
        let (v, d) = (cover(&vector, &mut art, 0.0, 0.0, by_thread), cover(&digitized, &mut art, 0.0, 0.0, by_thread));
        let (vs, ds) = (vector.sewn() as f64, digitized.sewn() as f64);
        let threads = made["stats"]["threadCount"].as_u64().unwrap() as usize;
        let errors = run(&mut image, "embroidery.check", json!({"limit": 0}))["summary"]["errors"]["hits"].as_u64().unwrap();
        eprintln!(
            "{design}\t{}\t{:.3}, {:.3}\t{:.3}, {:.3}\t{vs}, {ds} ({:.2})\t{}, {threads}",
            colours.len(),
            v.iou,
            d.iou,
            v.agreement,
            d.agreement,
            ds / vs,
            person.blocks.len()
        );
        if d.iou < v.iou - IOU_MARGIN {
            failed.push(format!("{design}: IoU {:.3}, the vector path's {:.3}", d.iou, v.iou));
        }
        if d.agreement < v.agreement - AGREEMENT_MARGIN {
            failed.push(format!("{design}: colour agreement {:.3}, the vector path's {:.3}", d.agreement, v.agreement));
        }
        if !(STITCHES_RANGE.0..=STITCHES_RANGE.1).contains(&(ds / vs)) {
            failed.push(format!("{design}: {ds} needle points, {:.2} times the vector path's", ds / vs));
        }
        if threads.abs_diff(person.blocks.len()) > THREADS_OFF {
            failed.push(format!("{design}: {threads} threads, the person's {}", person.blocks.len()));
        }
        if errors > 0 {
            failed.push(format!("{design}: the check finds {errors} errors"));
        }
        // The person's design is drawn where its box's centre meets the art's (for the table).
        let (ax, ay) = painted_centre(&art);
        let pb = person.bounds().unwrap();
        let p = cover(&person, &mut art, ax - (pb.x0 + pb.x1) / 2.0, ay - (pb.y0 + pb.y1) / 2.0, Sewn::Colour);
        eprintln!("{design}\tthe person's design: IoU {:.3}, agreement {:.3}, {} needle points", p.iou, p.agreement, person.sewn());
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
