//! Digitizing from an image: the colours, their threads, the background, and the choices for them.

use serde_json::{Value, json};
use vectorcraft_doc::{ImageBlob, ImageObject, Node, NodeKind};
use vectorcraft_geom::Affine;
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::{Palette, mismatched, shown_delta_e};
use vectorcraft_trace as tr;

use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [220, 30, 40, 255];
const GREEN: [u8; 4] = [30, 150, 60, 255];
const BLUE: [u8; 4] = [30, 70, 200, 255];
const YELLOW: [u8; 4] = [250, 220, 20, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

/// A session holding `raster` placed at a tenth of a millimetre a pixel, selected.
fn placed(raster: &tr::Raster) -> Session {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(0.1 / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    s
}

/// Four squares of colour, 8 mm each, in a margin of `margin` (2 mm wide), with a square of `inner`
/// (2 mm) in the red one.
fn quarters(margin: [u8; 4], inner: [u8; 4]) -> tr::Raster {
    tr::Raster::from_fn(200, 200, |x, y| {
        if !(20..180).contains(&x) || !(20..180).contains(&y) {
            return margin;
        }
        match (x < 100, y < 100) {
            (true, true) if (50..70).contains(&x) && (50..70).contains(&y) => inner,
            (true, true) => RED,
            (false, true) => GREEN,
            (true, false) => BLUE,
            (false, false) => YELLOW,
        }
    })
}

/// `#rrggbb` as sRGB.
fn rgb(v: &Value) -> [u8; 3] {
    let h = v.as_str().unwrap();
    [1, 3, 5].map(|k| u8::from_str_radix(&h[k..k + 2], 16).unwrap())
}

fn dry(s: &mut Session, params: Value) -> Value {
    let mut p = params;
    p["dryRun"] = json!(true);
    s.execute("embroidery.digitizeImage", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn hex(c: [u8; 4]) -> Value {
    json!(super::super::hex([c[0], c[1], c[2]]))
}

/// The row of `colors` for `color` (its sewn areas, or the background's).
fn row(r: &Value, color: [u8; 4], background: bool) -> &Value {
    r["colors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["color"] == hex(color) && c["background"] == json!(background))
        .unwrap_or_else(|| panic!("{color:?}: {}", r["colors"]))
}

/// A white page round four colours: the white touching the image's edge is the background, found at
/// its corners and never sewn; each colour gets a thread of its own, with its difference from it;
/// the shares add up.
#[test]
fn a_white_page_is_the_background() {
    let mut s = placed(&quarters(WHITE, RED));
    let r = dry(&mut s, json!({"colors": 4}));
    assert_eq!(r["image"]["background"], json!({"from": "corners", "color": hex(WHITE)}));
    let bg = row(&r, WHITE, true);
    assert_eq!((&bg["sewn"], &bg["thread"]), (&json!(false), &Value::Null));
    assert_eq!(r["colorCount"], json!(4));
    let threads: Vec<u64> = [RED, GREEN, BLUE, YELLOW].iter().map(|&c| row(&r, c, false)["thread"]["index"].as_u64().unwrap()).collect();
    let mut distinct = threads.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 4, "a thread each: {threads:?}");
    for c in [RED, GREEN, BLUE, YELLOW] {
        let t = &row(&r, c, false)["thread"];
        let thread = Palette::Jef.get(t["index"].as_u64().unwrap() as usize).unwrap().0;
        let de = vectorcraft_stitch::color::delta_e_rgb([c[0], c[1], c[2]], thread);
        assert_eq!(t["deltaE"], json!(vectorcraft_stitch::thread::shown_delta_e(de)));
        assert_eq!(t["mismatch"], json!(mismatched(de)));
        assert_eq!(row(&r, c, false)["alternatives"].as_array().unwrap().len(), vectorcraft_stitch::thread::ALTERNATIVES_SHOWN);
    }
    let share: f64 = r["colors"].as_array().unwrap().iter().map(|c| c["share"].as_f64().unwrap()).sum();
    assert!((share - 100.0).abs() < 0.1, "{share}");
    let sewn: f64 = r["colors"].as_array().unwrap().iter().filter_map(|c| c["sewnShare"].as_f64()).sum();
    assert!((sewn - 100.0).abs() < 0.1, "{sewn}");
}

/// White inside the design stays a colour of its own, sewn: only the white touching the image's
/// edge is the background.
#[test]
fn white_inside_the_design_is_sewn() {
    let mut s = placed(&quarters(WHITE, WHITE));
    let r = dry(&mut s, json!({"colors": 5}));
    assert_eq!(row(&r, WHITE, true)["sewn"], json!(false));
    let inner = row(&r, WHITE, false);
    assert_eq!((&inner["sewn"], &inner["areaCount"]), (&json!(true), &json!(1)));
    assert!(inner["thread"].is_object());
    // Without a background, the margin is sewn too: one white row.
    let r = dry(&mut s, json!({"colors": 5, "background": "none"}));
    assert_eq!(r["image"]["background"]["from"], json!("none"));
    assert_eq!(row(&r, WHITE, false)["areaCount"], json!(2));
    assert!(r["colors"].as_array().unwrap().iter().all(|c| c["background"] == json!(false)));
}

/// A transparent margin is the background by itself (it is never traced); corners that disagree
/// give none, with a warning.
#[test]
fn transparency_and_corners_that_disagree() {
    let mut s = placed(&quarters(CLEAR, RED));
    let r = dry(&mut s, json!({"colors": 4}));
    assert_eq!(r["image"]["background"], json!({"from": "transparency", "color": null}));
    assert!(r["colors"].as_array().unwrap().iter().all(|c| c["background"] == json!(false)));
    assert_eq!(r["colorCount"], json!(4));
    // The four colours to the edge: each corner another.
    let corners = tr::Raster::from_fn(200, 200, |x, y| match (x < 100, y < 100) {
        (true, true) => RED,
        (false, true) => GREEN,
        (true, false) => BLUE,
        (false, false) => YELLOW,
    });
    let mut s = placed(&corners);
    let r = dry(&mut s, json!({"colors": 4}));
    assert_eq!(r["image"]["background"]["from"], json!("none"));
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("corners don't agree")), "{}", r["warnings"]);
    assert_eq!(r["colorCount"], json!(4));
}

/// A person's choices are found again by colour: a thread chosen is kept, a colour left unsewn is
/// still listed, one merged into another takes its thread, and a choice no colour takes is listed
/// as unused.
#[test]
fn choices_for_colours_are_found_by_colour() {
    let mut s = placed(&quarters(WHITE, RED));
    let chosen = Palette::Jef.threads().iter().position(|(_, n)| *n == "Pink").map(|i| i + 1).unwrap();
    let r = dry(
        &mut s,
        json!({"colors": 4, "colorChoices": [
            {"color": "#dd1f29", "thread": {"palette": "jef", "index": chosen}},
            {"color": hex(GREEN), "job": "none"},
            {"color": hex(BLUE), "mergeInto": hex(YELLOW)},
            {"color": "#000000", "job": "fill"},
        ]}),
    );
    let red = row(&r, RED, false);
    assert_eq!(
        (&red["thread"]["index"], &red["reason"], &red["choice"]),
        (&json!(chosen), &json!("chosen"), &json!(0)),
        "within ΔE 10 of the choice"
    );
    let green = row(&r, GREEN, false);
    assert_eq!((&green["sewn"], &green["job"]), (&json!(false), &json!("none")));
    let blue = row(&r, BLUE, false);
    assert_eq!((&blue["mergedInto"], &blue["reason"]), (&hex(YELLOW), &json!("merged")));
    // Sewn in yellow's thread, its own difference from it.
    let (bt, yt) = (&blue["thread"], &row(&r, YELLOW, false)["thread"]);
    assert_eq!((&bt["index"], &bt["name"]), (&yt["index"], &yt["name"]));
    let own = vectorcraft_stitch::color::delta_e_rgb([BLUE[0], BLUE[1], BLUE[2]], rgb(&bt["color"]));
    assert_eq!((&bt["deltaE"], &bt["mismatch"]), (&json!(shown_delta_e(own)), &json!(mismatched(own))), "{bt}");
    assert_eq!(r["colorCount"], json!(2), "red and yellow: green unsewn, blue merged");
    assert_eq!(r["unusedChoices"], json!([{"choice": 3, "reason": "noSuchColor"}]));
    assert_eq!(r["settings"]["colorChoices"].as_array().unwrap().len(), 4);
    // Refused: a choice without its colour, a thread that isn't one, a merge with a thread.
    for bad in [
        json!([{"job": "none"}]),
        json!([{"color": hex(RED), "thread": {"palette": "pec", "index": 63}}]),
        json!([{"color": hex(RED), "mergeInto": hex(BLUE), "job": "fill"}]),
        json!([{"color": hex(RED), "colour": hex(BLUE)}]),
        json!({"color": hex(RED)}),
    ] {
        let mut p = json!({"dryRun": true, "colorChoices": bad.clone()});
        p["colors"] = json!(4);
        assert!(s.execute("embroidery.digitizeImage", &p).is_err(), "{bad}");
    }
}

/// Two colours whose only thread within ΔE 10 is the same one are sewn in it, merged: Janome's
/// palette has nothing near a dark grey but Black. A colour no thread comes near is flagged.
#[test]
fn colours_with_one_thread_are_merged_and_far_ones_flagged() {
    let (black, grey) = ([0, 0, 0, 255], [0x29, 0x2f, 0x33, 255]);
    let picture = tr::Raster::from_fn(200, 100, |x, y| match ((20..90).contains(&x), (110..180).contains(&x), (20..80).contains(&y)) {
        (true, _, true) => black,
        (_, true, true) => grey,
        _ => WHITE,
    });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 2}));
    let g = row(&r, grey, false);
    assert_eq!((&g["mergedInto"], &g["reason"]), (&hex(black), &json!("shared")), "{}", r["colors"]);
    assert_eq!(g["thread"]["index"], row(&r, black, false)["thread"]["index"]);
    // Sewn in Black, 12.43 from it: flagged, with its own nearest threads.
    let de = vectorcraft_stitch::color::delta_e_rgb([0x29, 0x2f, 0x33], [0, 0, 0]);
    assert!(mismatched(de), "{de}");
    assert_eq!((&g["thread"]["deltaE"], &g["thread"]["mismatch"]), (&json!(shown_delta_e(de)), &json!(true)), "{g}");
    let alternatives: Vec<f64> = g["alternatives"].as_array().unwrap().iter().map(|a| a["deltaE"].as_f64().unwrap()).collect();
    assert!(alternatives.windows(2).all(|w| w[0] <= w[1]) && alternatives[0] >= de - 0.01, "the grey's own: {alternatives:?}");
    assert_eq!(r["colorCount"], json!(1));
    // The fox's ears, far from every Brother thread.
    let ears = [0xa0, 0x04, 0x1e, 255];
    let picture = tr::Raster::from_fn(100, 100, |x, y| if (20..80).contains(&x) && (20..80).contains(&y) { ears } else { WHITE });
    let mut s = placed(&picture);
    s.execute("embroidery.setup", &json!({"palette": "pec"})).unwrap();
    let r = dry(&mut s, json!({"colors": 2}));
    let t = &row(&r, ears, false)["thread"];
    assert_eq!((&t["palette"], &t["mismatch"]), (&json!("pec"), &json!(true)), "{t}");
}

/// Greys mode sews in grey threads only; logo mode on an image that isn't flat art traces it again
/// in the colours asked, and says so.
#[test]
fn greys_and_a_logo_that_is_no_logo() {
    let mut s = placed(&quarters(WHITE, RED));
    let r = dry(&mut s, json!({"mode": "grayscale", "colors": 4}));
    for c in r["colors"].as_array().unwrap().iter().filter(|c| c["sewn"] == json!(true)) {
        let index = c["thread"]["index"].as_u64().unwrap() as usize;
        assert!(Palette::Jef.is_neutral(index), "{c}");
    }
    let gradient = tr::Raster::from_fn(300, 100, |x, y| [(x * 255 / 299) as u8, (y * 255 / 99) as u8, 128, 255]);
    let mut s = placed(&gradient);
    let r = dry(&mut s, json!({"mode": "logo", "colors": 4, "background": "none"}));
    assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("flat art")), "{}", r["warnings"]);
    assert!(r["colorCount"].as_u64().unwrap() <= 4, "{}", r["colorCount"]);
}

/// Merges a person asks for that end where they began leave no colour of its own: refused, naming
/// the colours.
#[test]
fn a_loop_of_merges_is_refused() {
    let mut s = placed(&quarters(WHITE, RED));
    let p = json!({"dryRun": true, "colors": 4, "colorChoices": [
        {"color": hex(BLUE), "mergeInto": hex(YELLOW)}, {"color": hex(YELLOW), "mergeInto": hex(BLUE)}]});
    let e = s.execute("embroidery.digitizeImage", &p).unwrap_err().to_string();
    assert!(e.contains("loop") && e.contains(&hex(BLUE).as_str().unwrap().to_string()), "{e}");
}

/// A colour merged into one that shares its thread ends at the colour holding the thread, and is
/// sewn as it is: in Janome's palette a large grey and a darker grey both share Black with black,
/// and the grey a person set to Fill takes black's job.
#[test]
fn merges_end_at_the_colour_holding_the_thread() {
    let (black, grey, dark) = ([0, 0, 0, 255], [0x29, 0x2f, 0x33, 255], [0x1a, 0x1c, 0x1e, 255]);
    let picture =
        tr::Raster::from_fn(300, 100, |x, y| match ((20..60).contains(&x), (90..200).contains(&x), (220..280).contains(&x), (20..80).contains(&y)) {
            (true, _, _, true) => black,
            (_, true, _, true) => grey,
            (_, _, true, true) => dark,
            _ => WHITE,
        });
    let mut s = placed(&picture);
    let r = dry(&mut s, json!({"colors": 3, "colorChoices": [{"color": hex(grey), "job": "fill"}]}));
    let rows = r["colors"].as_array().unwrap();
    for row in rows.iter().filter(|c| !c["mergedInto"].is_null()) {
        let into = rows.iter().find(|c| c["color"] == row["mergedInto"]).unwrap();
        assert!(into["mergedInto"].is_null(), "{row} ends at a merged colour: {into}");
        assert_eq!((&row["thread"]["index"], &row["job"]), (&into["thread"]["index"], &into["job"]), "{row}");
    }
    assert_eq!(rows.iter().filter(|c| !c["mergedInto"].is_null()).count(), 2, "{}", r["colors"]);
    assert_eq!(row(&r, grey, false)["job"], json!("auto"), "black's job");
}
