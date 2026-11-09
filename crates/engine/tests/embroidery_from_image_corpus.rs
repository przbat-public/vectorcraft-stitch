//! Digitizing from an image on the real designs' artwork (`cargo xtask corpus --embroidery`,
//! checked against its manifest), measured against three references: the art itself, for shape and
//! colour; VectorCraft's own digitizing of the same art (the vector path), so that what differs is
//! what the image stage loses; and the person's design, for counts. Each artwork, at the size of
//! the person's design, is drawn by the engine as a PNG at 10 pixels a millimetre over its box and
//! a 5 mm margin, placed over that same box, and digitized by Create in the art's own number of
//! colours, in Brother's palette (the person's designs are PES) and again in Janome's, whose
//! nearer threads merge colours of four of the designs.
//!
//! For every design, in both palettes:
//! - the silhouette's IoU against the art (needle-down stitches drawn 0.4 mm wide, where 40 wt
//!   threads touch) is at least the vector path's less [`IOU_MARGIN`];
//! - the colour agreement (each flat pixel of the art under the thread the vector path sews its
//!   colour in) at least the vector path's less [`AGREEMENT_MARGIN`] (the image path paints its
//!   objects in their threads' colours, which can lie nearer another of the art's colours, so both
//!   are measured by thread);
//! - the needle points are [`STITCHES_RANGE`] times the vector path's (the bullseye's art stacks
//!   whole discs, which the vector path sews one over another and the image path as abutting
//!   rings: 0.69 times);
//! - the fabric that shows between two of the art's colours (pixels at least 0.5 mm inside its
//!   silhouette, within 0.3 mm of flat pixels of both, that no stitch reaches) is at most the
//!   vector path's plus [`FABRIC_MARGIN_MM2`], with the overlap; the figure with none is printed
//!   beside it, and so are the fabric inside the art in all and the share of its seams sewn twice;
//! - the overlap reaches at least [`MIN_REACH_MM`] under the next colour on average along the
//!   edges colours share, measured from the outlines (what they cover at 0.4 mm past what they
//!   cover at none, by that length): with the overlap off, this fails;
//! - the art as a JPEG of quality 55, and drawn at 4 pixels a millimetre, keeps the clean
//!   picture's IoU and colour agreement within [`JPEG_MARGIN`] and [`COARSE_MARGIN`] (the plan's
//!   validation note), the agreement by colour (a scan's colours drift, and the nearest thread can
//!   be another as near);
//! - in Brother's palette, the threads number within one of the person's, each of theirs counted
//!   once however many colour blocks it sews;
//! - the check finds no error, and the dry run's figures are Create's.
//!
//! What the thresholds were calibrated on (2026-11-02, before any of this code was written): the
//! IoU, agreement and stitch margins were set so that upstream's plain Image Trace passes them,
//! with no overlap, no region rules and no threads chosen together, so they bound regressions
//! against that trace and can't see the overlap; the fabric between colours can't either (the
//! planner's stitches, drawn a thread wide, already cover a seam from both sides: 0.9 mm² at most
//! with no overlap). The reach does, and the fabric margin and the reach were measured on these
//! designs on 2026-11-03, the JPEG and coarse margins taken from the plan's note.
//!
//! A table of the measures is printed. Local runs skip an absent corpus;
//! `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
//!
//! The real designs' artwork makes no thin line at 10 pixels a millimetre (centre lines moved none
//! of its figures there, 2026-11-07), so line art is drawn here too, with no corpus: a T,
//! an X, an S curve, hatch lines and a ring 0.5 mm wide, digitized against their vector path, every
//! line sewn as a running line along its middle as the vector path sews it
//! ([`line_art_digitized_stays_on_the_vector_path`]).
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::json;
use vectorcraft_engine::EmbroideryOptions;
use vectorcraft_stitch::format::Format;

mod embroidery_common;
use embroidery_common::images::{Art, Sewn, art_picture, art_picture_at, cover, painted_centre, placed_over, thread_of};
use embroidery_common::{DESIGNS, artwork, corpus, run};

/// How much lower than the vector path's the image path's silhouette IoU may be: 0.02, the
/// largest gap measured before any code was 0.013 (the video game, 2026-11-02) and is 0.011 now
/// (the fox, 2026-11-03), with room for the renderer's anti-aliasing to differ between systems.
const IOU_MARGIN: f64 = 0.02;

/// How much lower than the vector path's its colour agreement may be: 0.02, the largest gap
/// measured before any code was 0.011 (2026-11-02) and is 0.009 now (the bullseye, 2026-11-03).
const AGREEMENT_MARGIN: f64 = 0.02;

/// The needle points the image path may sew, as a share of the vector path's: the bullseye's
/// abutting rings sew 0.69 of its stacked discs with the overlap (0.68 before it, 2026-11-02); an
/// overlap of 0.4 mm adds a little elsewhere.
const STITCHES_RANGE: (f64, f64) = (0.6, 1.4);

/// How far the threads may number from the person's: the art's colours and the person's threads,
/// each counted once, differ by one at most on every design.
const THREADS_OFF: usize = 1;

/// The fewest colours the command takes: a design of one colour is asked for two, the
/// anti-aliased edge the second, which joins the first as a sliver.
const MIN_COLORS: usize = 2;

/// How much more fabric may show between the art's colours where the image path sews it, with the
/// overlap, than where the vector path does (square millimetres): 1.5, over the most measured on
/// 2026-11-03, 1.1 on the pool ball, where the overlap moves the end of a satin column the planner
/// sews in each notch of the 8's waist so that it no longer reaches the corner (0.3 there with no
/// overlap; the vector path leaves 0.1 at most): the planner's to fix, on its own (the project
/// owner's choice, 2026-11-03).
const FABRIC_MARGIN_MM2: f64 = 1.5;

/// How far, on average, the overlap must reach under the next colour along the edges colours share
/// (millimetres), on a design of two colours or more: 0.25, under the 0.29 to 0.38 measured on
/// 2026-11-03 at 0.4 mm, the strip narrowing at its round ends and under thin neighbours. None
/// reached is no overlap.
const MIN_REACH_MM: f64 = 0.25;

/// The overlap the gates are taken at (millimetres), the default, and none, whose fabric is
/// printed beside it.
const OVERLAP_MM: f64 = 0.4;
const NO_OVERLAP_MM: f64 = 0.0;

/// The JPEG the plan's validation note measures the art as (its quality), and how much lower than
/// the clean picture's its IoU and colour agreement may be: 0.01, the note's.
const JPEG_QUALITY: u8 = 55;
const JPEG_MARGIN: f64 = 0.01;

/// The coarse copy the note measures the art at (pixels a millimetre, the coarse-image warning's
/// line), and how much lower than the clean picture's its IoU and agreement may be: 0.02, the
/// note's.
const COARSE_PX_PER_MM: f64 = 4.0;
const COARSE_MARGIN: f64 = 0.02;

/// The palettes the designs are digitized in: Brother's, as the person's designs are PES, and
/// Janome's, whose nearer threads merge colours of four of the designs.
const PALETTES: [&str; 2] = ["pec", "jef"];

/// `png` drawn over white, as a JPEG of `quality`: a scan of the art as a camera or a scanner saves
/// it.
fn jpeg_of(png: &[u8], quality: u8) -> Vec<u8> {
    let img = image::load_from_memory(png).unwrap().to_rgba8();
    let over_white = |c: u8, a: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
    let rgb: Vec<u8> = img.pixels().flat_map(|p| [over_white(p[0], p[3]), over_white(p[1], p[3]), over_white(p[2], p[3])]).collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(&rgb, img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// The most areas a reply lists: every area of the real designs.
const LISTED: u64 = 10_000;

/// The outlines of reply `r`'s areas, measured: their areas added (square millimetres).
fn outlined(r: &serde_json::Value) -> f64 {
    use vectorcraft_geom::kurbo::{BezPath, Shape};
    r["areas"].as_array().unwrap().iter().filter_map(|a| a["outline"].as_str()).map(|d| BezPath::from_svg(d).unwrap().area().abs()).sum()
}

/// The edges along which reply `r`'s areas grow under the ones sewn after them (millimetres): each
/// edge between two sewn areas, the later's, but an island's sewn over the area round it.
fn grown_along(r: &serde_json::Value) -> f64 {
    let areas = r["areas"].as_array().unwrap();
    let sewn = |n: &serde_json::Value| areas.iter().any(|a| &a["area"] == n && a["sewn"] == json!(true));
    let mut edge = 0.0;
    for a in areas.iter().filter(|a| a["sewn"] == json!(true)) {
        for t in a["touches"].as_array().unwrap().iter().filter(|t| t["under"] == json!(true) && sewn(&t["area"])) {
            if !(a["sewnOver"] == json!(true) && a["inside"] == t["area"]) {
                edge += t["edge"].as_f64().unwrap();
            }
        }
    }
    edge
}

/// Picture `bytes` (named `name`) placed over `rect`, digitized by Create in `palette` with `colours`
/// colours and `overlap` millimetres of overlap: what it sews, and whether the dry run answered
/// Create's figures, with Create's reply; the dry run's lists its outlines.
fn digitized(
    name: &str,
    bytes: &[u8],
    rect: embroidery_common::images::Box,
    palette: &str,
    colours: usize,
    overlap: f64,
) -> (vectorcraft_stitch::Pattern, bool, serde_json::Value) {
    let mut image = placed_over(name, bytes, rect);
    run(&mut image, "embroidery.setup", json!({"palette": palette}));
    let params = json!({"colors": colours.max(MIN_COLORS), "overlap": overlap});
    let mut dry = params.clone();
    dry["dryRun"] = json!(true);
    dry["outlines"] = json!(true);
    dry["limit"] = json!(LISTED);
    let answered = run(&mut image, "embroidery.digitizeImage", dry);
    let made = run(&mut image, "embroidery.digitizeImage", params);
    let errors = run(&mut image, "embroidery.check", json!({"limit": 0}))["summary"]["errors"]["hits"].as_u64().unwrap();
    let mut made = made;
    made["checkErrors"] = json!(errors);
    made["outlined"] = json!(outlined(&answered));
    made["grownAlong"] = json!(grown_along(&answered));
    (image.embroidery_plan(&EmbroideryOptions::default()).unwrap().to_pattern(), answered["stats"] == made["stats"], made)
}

#[test]
fn digitized_artwork_stays_near_the_vector_path() {
    let Some(dir) = corpus() else { return };
    let mut failed = Vec::new();
    eprintln!(
        "design\tpalette\tcolours\tIoU vector, image\tagreement vector, image\tstitches vector, image (×)\tthreads person, image\tfabric between colours mm² vector, image, at 0 (inside in all)\tseams sewn twice vector, image, at 0\treach mm along mm\tJPEG IoU, agreement by colour (PNG's)\t4 px/mm IoU, agreement by colour"
    );
    for palette in PALETTES {
        for &(art_name, theirs) in DESIGNS {
            let design = art_name.split('-').next().unwrap();
            let mut s = artwork(&dir, art_name, theirs);
            run(&mut s, "embroidery.setup", json!({"palette": palette}));
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
            let (_, coarse_png) = art_picture_at(&mut s, COARSE_PX_PER_MM);
            let mut art = Art::of(&png, rect, colours.clone());
            let by_thread = Sewn::Thread(&threads);
            let (sewn, same, made) = digitized("art.png", &png, rect, palette, colours.len(), OVERLAP_MM);
            if !same {
                failed.push(format!("{design} in {palette}: the dry run's figures aren't Create's"));
            }
            let (bare, _, flush) = digitized("art.png", &png, rect, palette, colours.len(), NO_OVERLAP_MM);
            // How far the overlap reaches under the next colour, on average along the edges it grows
            // along: what the outlines cover at 0.4 mm past what they cover at none, by that length.
            let along = made["grownAlong"].as_f64().unwrap();
            let reach = if along > 0.0 { (made["outlined"].as_f64().unwrap() - flush["outlined"].as_f64().unwrap()) / along } else { 0.0 };
            let (jpeg, _, _) = digitized("art.jpg", &jpeg_of(&png, JPEG_QUALITY), rect, palette, colours.len(), OVERLAP_MM);
            let (coarse, _, _) = digitized("art.png", &coarse_png, rect, palette, colours.len(), OVERLAP_MM);
            let (v, d) = (cover(&vector, &mut art, 0.0, 0.0, by_thread), cover(&sewn, &mut art, 0.0, 0.0, by_thread));
            let b = cover(&bare, &mut art, 0.0, 0.0, by_thread);
            // The JPEG and the coarse copy against the clean picture, by colour: a scan's colours
            // drift, and its nearest thread can be another as near.
            let by_colour = |p: &vectorcraft_stitch::Pattern, art: &mut Art| cover(p, art, 0.0, 0.0, Sewn::Colour);
            let (dc, j, c) = (by_colour(&sewn, &mut art), by_colour(&jpeg, &mut art), by_colour(&coarse, &mut art));
            let (vs, ds) = (vector.sewn() as f64, sewn.sewn() as f64);
            let threads_sewn = made["stats"]["threadCount"].as_u64().unwrap() as usize;
            let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
            // The person's threads, each once however many colour blocks it sews.
            let mut theirs_threads: Vec<[u8; 3]> = person.blocks.iter().map(|b| b.thread.color).collect();
            theirs_threads.sort_unstable();
            theirs_threads.dedup();
            eprintln!(
                "{design}\t{palette}\t{}\t{:.3}, {:.3}\t{:.3}, {:.3}\t{vs}, {ds} ({:.2})\t{}, {threads_sewn}\t{:.1}, {:.1}, {:.1} ({:.1}, {:.1}, {:.1})\t{:.2}, {:.2}, {:.2}\t{reach:.2} along {along:.0}\t{:.3}, {:.3} ({:.3})\t{:.3}, {:.3}",
                colours.len(),
                v.iou,
                d.iou,
                v.agreement,
                d.agreement,
                ds / vs,
                theirs_threads.len(),
                v.seam_fabric_mm2,
                d.seam_fabric_mm2,
                b.seam_fabric_mm2,
                v.fabric_mm2,
                d.fabric_mm2,
                b.fabric_mm2,
                v.seams_doubled,
                d.seams_doubled,
                b.seams_doubled,
                j.iou,
                j.agreement,
                dc.agreement,
                c.iou,
                c.agreement
            );
            let mut fail = |why: String| failed.push(format!("{design} in {palette}: {why}"));
            if d.iou < v.iou - IOU_MARGIN {
                fail(format!("IoU {:.3}, the vector path's {:.3}", d.iou, v.iou));
            }
            if d.agreement < v.agreement - AGREEMENT_MARGIN {
                fail(format!("colour agreement {:.3}, the vector path's {:.3}", d.agreement, v.agreement));
            }
            if !(STITCHES_RANGE.0..=STITCHES_RANGE.1).contains(&(ds / vs)) {
                fail(format!("{ds} needle points, {:.2} times the vector path's", ds / vs));
            }
            if d.seam_fabric_mm2 > v.seam_fabric_mm2 + FABRIC_MARGIN_MM2 {
                fail(format!("{:.1} mm² of fabric shows between colours, the vector path's {:.1}", d.seam_fabric_mm2, v.seam_fabric_mm2));
            }
            if along > 0.0 && reach < MIN_REACH_MM {
                fail(format!("the overlap reaches {reach:.2} mm under the next colour along {along:.0} mm"));
            }
            if j.iou < d.iou - JPEG_MARGIN || j.agreement < dc.agreement - JPEG_MARGIN {
                fail(format!("as a JPEG: IoU {:.3}, agreement {:.3}, the PNG's {:.3}, {:.3}", j.iou, j.agreement, d.iou, dc.agreement));
            }
            if c.iou < d.iou - COARSE_MARGIN || c.agreement < dc.agreement - COARSE_MARGIN {
                fail(format!("at 4 px/mm: IoU {:.3}, agreement {:.3}, the 10's {:.3}, {:.3}", c.iou, c.agreement, d.iou, dc.agreement));
            }
            // The person's threads are Brother's: in Janome's, colours whose nearest thread is one
            // thread merge, and fewer are sewn.
            if palette == "pec" && threads_sewn.abs_diff(theirs_threads.len()) > THREADS_OFF {
                fail(format!("{threads_sewn} threads, the person's {}", theirs_threads.len()));
            }
            if made["checkErrors"] != json!(0) {
                fail(format!("the check finds {} errors", made["checkErrors"]));
            }
            if palette == "pec" {
                // The person's design is drawn where its box's centre meets the art's (for the table).
                let (ax, ay) = painted_centre(&art);
                let pb = person.bounds().unwrap();
                let p = cover(&person, &mut art, ax - (pb.x0 + pb.x1) / 2.0, ay - (pb.y0 + pb.y1) / 2.0, Sewn::Colour);
                eprintln!(
                    "{design}\tthe person's design: IoU {:.3}, agreement {:.3}, {} needle points, {:.1} mm² of fabric",
                    p.iou,
                    p.agreement,
                    person.sewn(),
                    p.fabric_mm2
                );
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// The line art [`line_art_digitized_stays_on_the_vector_path`] draws (document points), stroked
/// [`LINE_PT`] wide with round caps and joins: a letter T and an X, each of pieces meeting end to
/// end at its junction (so that the vector path routes it too); an S curve; three hatch lines apart;
/// and a ring.
const LINE_ART: [&str; 4] = [
    "M 30 30 L 80 30 M 80 30 L 130 30 M 80 30 L 80 110",
    "M 160 30 L 200 70 M 200 70 L 240 30 M 200 70 L 160 110 M 200 70 L 240 110",
    "M 30 170 C 80 130 110 230 160 180",
    "M 200 150 L 280 150 M 200 170 L 280 170 M 200 190 L 280 190",
];
const RING_PT: (f64, f64, f64) = (300.0, 30.0, 80.0);

/// How wide the line art is drawn (points): half a millimetre, a fine pen's line.
const LINE_PT: f64 = 0.5 * 72.0 / 25.4;

/// How near the vector path's stitches a needle point of the image path must lie to be on the line
/// (millimetres), and how near the image path's stitches a needle point of the vector path must lie
/// to be covered: three working pixels (at 10 a millimetre), and half a thread's width more.
const ON_LINE_MM: f64 = 0.3;
const COVERED_MM: f64 = 0.5;

/// The share of needle points on the line and covered that the image path must reach.
const ON_LINE_SHARE: f64 = 0.98;

/// The needle points the image path may sew, as a share of the vector path's.
const LINE_STITCHES_RANGE: (f64, f64) = (0.8, 1.25);

/// The needle-down stitches of `p` as segments (millimetres): each from the needle point before it.
fn stitch_segments(p: &vectorcraft_stitch::Pattern) -> Vec<(vectorcraft_geom::Point, vectorcraft_geom::Point)> {
    let mut out = Vec::new();
    for b in &p.blocks {
        for w in b.stitches.windows(2) {
            if let [a, q] = w
                && q.command == vectorcraft_stitch::Command::Stitch
            {
                out.push((a.point(), q.point()));
            }
        }
    }
    out
}

/// The share of `p`'s needle points within `reach` millimetres of one of `segments`.
fn share_near(p: &vectorcraft_stitch::Pattern, segments: &[(vectorcraft_geom::Point, vectorcraft_geom::Point)], reach: f64) -> f64 {
    let to = |q: vectorcraft_geom::Point, (a, b): (vectorcraft_geom::Point, vectorcraft_geom::Point)| {
        let ab = b - a;
        let t = if ab.hypot2() > 0.0 { ((q - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) } else { 0.0 };
        q.distance(a + ab * t)
    };
    let points: Vec<vectorcraft_geom::Point> =
        p.blocks.iter().flat_map(|b| &b.stitches).filter(|q| q.command == vectorcraft_stitch::Command::Stitch).map(|q| q.point()).collect();
    let near = points.iter().filter(|&&q| segments.iter().any(|&s| to(q, s) <= reach)).count();
    near as f64 / points.len().max(1) as f64
}

/// How many jumps `p` makes.
fn jumps(p: &vectorcraft_stitch::Pattern) -> usize {
    p.blocks.iter().flat_map(|b| &b.stitches).filter(|q| q.command == vectorcraft_stitch::Command::Jump).count()
}

/// How many moves `p` makes between stitches: each run of jumps one move, however many records a
/// long one takes.
fn moves(p: &vectorcraft_stitch::Pattern) -> usize {
    let jump: Vec<bool> = p.blocks.iter().flat_map(|b| &b.stitches).map(|q| q.command == vectorcraft_stitch::Command::Jump).collect();
    (0..jump.len()).filter(|&k| jump[k] && (k == 0 || !jump[k - 1])).count()
}

/// How many separate drawings [`LINE_ART`] and the ring make, counted here from the drawing itself
/// rather than by the planner: its pieces joined where one ends at another's end (as the T's and
/// the X's do).
fn drawings() -> usize {
    use vectorcraft_geom::kurbo::{BezPath, PathEl};
    let mut ends: Vec<(vectorcraft_geom::Point, vectorcraft_geom::Point)> = Vec::new();
    for d in LINE_ART {
        let (mut first, mut last) = (None, None);
        for el in BezPath::from_svg(d).unwrap().elements() {
            match *el {
                PathEl::MoveTo(p) => {
                    ends.extend(first.zip(last));
                    (first, last) = (Some(p), Some(p));
                }
                PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => last = Some(p),
                PathEl::ClosePath => last = first,
            }
        }
        ends.extend(first.zip(last));
    }
    fn root(up: &[usize], mut i: usize) -> usize {
        while up[i] != i {
            i = up[i];
        }
        i
    }
    let mut up: Vec<usize> = (0..ends.len()).collect();
    for i in 0..ends.len() {
        for j in 0..i {
            let ((a0, a1), (b0, b1)) = (ends[i], ends[j]);
            if [a0, a1].iter().any(|a| [b0, b1].iter().any(|b| a.distance(*b) < 1e-9)) {
                let (ri, rj) = (root(&up, i), root(&up, j));
                up[ri] = rj;
            }
        }
    }
    let pieces = (0..ends.len()).filter(|&i| root(&up, i) == i).count();
    // And the ring, a drawing of its own.
    pieces + 1
}

/// Line art drawn here (no corpus needed), drawn as a picture as the real designs' artwork is and
/// digitized, against its vector path (the strokes sewn as drawn): its thin lines are sewn along
/// their middle through their junctions, as the vector path sews them, so that every one is a
/// running line as the vector path's are, the image path makes no more jumps than the vector path,
/// and no more moves than it takes to go from each of the drawing's seven separate drawings to the
/// next (counted in the test from the drawing, not by the planner that sews both paths), its needle
/// points lie on the vector path's stitches and cover them, and it sews
/// about as many (measured 2026-11-07: 208 needle points to the vector path's 205, 6 jumps each,
/// every needle point on the line and covered). With centre lines off, each line is a fill whose
/// narrow parts the planner sews along their middle: as near, but no running line.
#[test]
fn line_art_digitized_stays_on_the_vector_path() {
    let mut s = vectorcraft_engine::Session::new();
    run(&mut s, "file.new", json!({"width": 420, "height": 260, "artboards": 1}));
    for d in LINE_ART {
        run(&mut s, "paint.setFill", json!({"none": true}));
        run(&mut s, "paint.setStroke", json!({"color": "#141414"}));
        let id = run(&mut s, "path.create", json!({"d": d}))["id"].clone();
        run(&mut s, "stroke.set", json!({"ids": [id], "weight": LINE_PT, "cap": "round", "join": "round"}));
        run(&mut s, "select.none", json!({}));
    }
    let (x, y, d) = RING_PT;
    let ring = run(&mut s, "shape.ellipse", json!({"x": x, "y": y, "width": d, "height": d}))["id"].clone();
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [ring]}));
    run(&mut s, "paint.setStroke", json!({"color": "#141414", "ids": [ring]}));
    run(&mut s, "stroke.set", json!({"ids": [ring], "weight": LINE_PT}));
    run(&mut s, "select.none", json!({}));
    let vector_plan = s.embroidery_plan(&EmbroideryOptions::default()).unwrap();
    let vector = vector_plan.to_pattern();
    let (rect, png) = art_picture(&mut s);
    let (image, same, _) = digitized("lines.png", &png, rect, "pec", MIN_COLORS, OVERLAP_MM);
    // Each path's units by the style they are sewn in: every one a running line.
    let mut made = placed_over("lines.png", &png, rect);
    run(&mut made, "embroidery.setup", json!({"palette": "pec"}));
    run(&mut made, "embroidery.digitizeImage", json!({"colors": MIN_COLORS, "overlap": OVERLAP_MM}));
    let image_plan = made.embroidery_plan(&EmbroideryOptions::default()).unwrap();
    let running = Some(vectorcraft_stitch::plan::Style::Running);
    let vector_styles: Vec<_> = vector_plan.units.iter().map(|u| u.sewn()).collect();
    let image_styles: Vec<_> = image_plan.units.iter().map(|u| u.sewn()).collect();
    assert!(vector_styles.iter().all(|&s| s == running), "the vector path: {vector_styles:?}");
    assert!(image_styles.iter().all(|&s| s == running), "the image path's lines aren't running lines: {image_styles:?}");
    let on_line = share_near(&image, &stitch_segments(&vector), ON_LINE_MM);
    let covered = share_near(&vector, &stitch_segments(&image), COVERED_MM);
    let (vs, is) = (vector.sewn() as f64, image.sewn() as f64);
    eprintln!(
        "line art: needle points vector {vs}, image {is} ({:.2}); jumps vector {}, image {}; on the line {on_line:.3}, covered {covered:.3}",
        is / vs,
        jumps(&vector),
        jumps(&image)
    );
    assert!(same, "the dry run's figures aren't Create's");
    assert!(jumps(&image) <= jumps(&vector), "the image path jumps {} times, the vector path {}", jumps(&image), jumps(&vector));
    let apart = drawings();
    assert_eq!(apart, 7, "the line art's drawings");
    assert!(moves(&image) < apart, "the image path moves {} times between {apart} drawings", moves(&image));
    assert!(on_line >= ON_LINE_SHARE && covered >= ON_LINE_SHARE, "on the line {on_line:.3}, covered {covered:.3}");
    assert!((LINE_STITCHES_RANGE.0..=LINE_STITCHES_RANGE.1).contains(&(is / vs)), "{is} needle points, the vector path's {vs}");
}
