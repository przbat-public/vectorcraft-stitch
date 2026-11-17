//! Digitizing from an image: changes to single areas (`areaChanges`), each kept by a point inside
//! its area (the image's own pixels) and the area's colour, so that it is found again after any
//! setting changes the areas.

use serde_json::{Value, json};
use vectorcraft_doc::embroidery::PartSettings;
use vectorcraft_doc::{Document, ImageBlob, ImageObject, Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point};
use vectorcraft_stitch::MM_PER_PT;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_trace as tr;

use super::changes::{Applied, Found, KeptWhy, found, per_area};
use super::command::digitized;
use super::settings::{AreaStitch, Settings, table};
use super::stages::Digitized;
use crate::Session;

const WHITE: [u8; 4] = [255, 255, 255, 255];
const ORANGE: [u8; 4] = [240, 130, 20, 255];
const NAVY: [u8; 4] = [20, 40, 120, 255];

/// A tenth of a millimetre a pixel: the working copy is the picture, pixel for pixel.
const MM_PER_PX: f64 = 0.1;

/// The settings `params` give, for a document in Janome's threads.
fn settings(params: Value) -> crate::Result<Settings> {
    let o = params.as_object().cloned().unwrap_or_default();
    Settings::from_params(&o, Palette::Jef)
}

/// The settings table's row for the area changes.
fn row() -> Value {
    table(Palette::Jef).as_array().and_then(|rows| rows.iter().find(|r| r["name"] == "areaChanges").cloned()).expect("a row for areaChanges")
}

/// A change that sews a filled orange area as tatami.
fn tatami() -> Value {
    json!({"at": [12.5, 40.0], "color": "#f08214", "kind": "fill", "stitch": "tatami"})
}

#[test]
fn area_changes_are_read_and_echoed() {
    let changes = json!([
        {"at": [12.5, 40.0], "color": "#f08214", "kind": "fill", "stitch": "tatami", "angle": 30.0},
        {"at": [100.0, 7.25], "color": "#14287a", "kind": "line", "stitch": "satin"},
        {"at": [3.0, 4.0], "color": "#ffffff", "kind": "fill", "angle": -45.5},
    ]);
    let s = settings(json!({"areaChanges": changes})).expect("taken");
    assert_eq!(s.echo()["areaChanges"], changes, "echoed as given");
    assert_eq!(settings(json!({})).expect("none").echo()["areaChanges"], json!([]), "none by default");
}

#[test]
fn an_area_change_out_of_shape_is_refused() {
    settings(json!({"areaChanges": [tatami()]})).expect("a change of a fill's stitch is taken");
    let refused = |change: Value| {
        let e = settings(json!({"areaChanges": [change.clone()]})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("areaChanges"), "{change}: {e}");
    };
    // A stitch of the other kind of area.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "stitch": "running"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "line", "stitch": "tatami"}));
    // An angle on a line, whose stitches follow it.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "line", "angle": 30.0}));
    // An angle past a full turn.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "angle": 400.0}));
    // A point that isn't two numbers, a colour that isn't #rrggbb, a kind of no area.
    refused(json!({"at": [1.0], "color": "#f08214", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, "2"], "color": "#f08214", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, 1.0], "color": "orange", "kind": "fill", "stitch": "none"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "island", "stitch": "none"}));
    // A change that changes nothing, and a key no change takes.
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill"}));
    refused(json!({"at": [1.0, 1.0], "color": "#f08214", "kind": "fill", "stitch": "none", "thread": 3}));
    // At most as many changes as the table says.
    let most = usize::try_from(row()["range"][1].as_u64().expect("a most")).expect("a count");
    settings(json!({"areaChanges": vec![tatami(); most]})).expect("as many as the table says");
    let e = settings(json!({"areaChanges": vec![tatami(); most + 1]})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains(&format!("{} changes", most + 1)) && !e.contains("tatami"), "one more is refused, said by its count: {e}");
}

#[test]
fn the_settings_table_lists_area_changes() {
    let r = row();
    assert_eq!((&r["key"], &r["step"], &r["default"]), (&json!("area_changes"), &json!("stitches"), &json!([])));
    assert!(!r["source"].as_str().unwrap_or_default().is_empty(), "a source");
}

fn inside(x: u32, y: u32, [x0, y0, x1, y1]: [u32; 4]) -> bool {
    (x0..x1).contains(&x) && (y0..y1).contains(&y)
}

fn hex(c: [u8; 4]) -> String {
    super::super::hex([c[0], c[1], c[2]])
}

/// What digitizing makes of `raster`, placed at a tenth of a millimetre a pixel, with `params`.
fn digitize(raster: &tr::Raster, params: &Value) -> Digitized {
    let png = raster.encode_png();
    let xf = Affine::scale(MM_PER_PX / MM_PER_PT);
    let image = ImageObject {
        key: "picture".into(),
        width: raster.width,
        height: raster.height,
        xf,
        link: None,
        placement: Default::default(),
        pixels: None,
    };
    let settings = settings(params.clone()).expect("settings");
    digitized(&image, &png, &settings, Palette::Jef, &|_, _| true).expect("digitized")
}

/// An orange square, a navy one apart from it, an orange bar 2 mm wide and a navy line 0.6 mm wide
/// (sewn along its middle), on white.
fn picture() -> tr::Raster {
    tr::Raster::from_fn(400, 200, |x, y| {
        if inside(x, y, [20, 20, 140, 180]) || inside(x, y, [300, 20, 320, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) || inside(x, y, [350, 20, 356, 180]) {
            NAVY
        } else {
            WHITE
        }
    })
}

/// The changes `list` gives.
fn changes(list: Value) -> Vec<super::settings::AreaChange> {
    settings(json!({"areaChanges": list})).expect("changes").changes
}

/// The area under working pixel `(x, y)` of `d`.
fn area(d: &Digitized, x: u32, y: u32) -> usize {
    d.areas().regions.at(x, y).expect("an area") as usize
}

#[test]
fn a_change_finds_its_area_at_its_point() {
    let d = digitize(&picture(), &json!({}));
    let c = changes(json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"}]));
    assert_eq!(found(&c, &d), vec![Found::Applies(area(&d, 80, 100))]);
}

#[test]
fn a_change_finds_its_area_within_a_millimetre() {
    let d = digitize(&picture(), &json!({}));
    // 0.6 mm left of the orange bar, on the white: the bar is the nearest orange within 1 mm.
    let c = changes(json!([{"at": [294.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]));
    assert_eq!(found(&c, &d), vec![Found::Applies(area(&d, 310, 100))]);
    // 1.5 mm left of it: too far.
    let c = changes(json!([{"at": [285.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]));
    assert_eq!(found(&c, &d), vec![Found::Kept(KeptWhy::NoArea)]);
}

#[test]
fn a_change_at_another_colours_area_is_kept_as_another_colour() {
    let d = digitize(&picture(), &json!({}));
    // An orange change in the navy square's middle, far from any orange.
    let c = changes(json!([{"at": [210.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"}]));
    assert_eq!(found(&c, &d), vec![Found::Kept(KeptWhy::OtherColor)]);
}

#[test]
fn a_change_outside_the_crop_has_no_area() {
    let d = digitize(&picture(), &json!({"crop": [0, 0, 150, 200]}));
    let c = changes(json!([
        {"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "tatami"},
        {"at": [5.5, 5.5], "color": hex(WHITE), "kind": "fill", "stitch": "tatami"},
    ]));
    assert_eq!(found(&c, &d), vec![Found::Kept(KeptWhy::NoArea), Found::Kept(KeptWhy::NoArea)], "past the crop; on the background");
}

#[test]
fn a_change_set_on_a_fill_is_kept_on_a_line() {
    let d = digitize(&picture(), &json!({}));
    let c = changes(json!([
        {"at": [353.0, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "tatami"},
        {"at": [353.0, 100.5], "color": hex(NAVY), "kind": "line", "stitch": "satin"},
    ]));
    let line = area(&d, 353, 100);
    assert_eq!(found(&c, &d), vec![Found::Kept(KeptWhy::OtherKind), Found::Applies(line)]);
}

/// A thin area of a colour set to Fill is made a filled shape (no centre line), so a change is
/// found on it as a fill's, and one set on a line is kept as of the other kind.
#[test]
fn a_fill_colours_thin_area_takes_a_fills_changes() {
    let d = digitize(&picture(), &json!({"colorChoices": [{"color": hex(NAVY), "job": "fill"}]}));
    let c = changes(json!([
        {"at": [353.0, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "tatami"},
        {"at": [353.0, 100.5], "color": hex(NAVY), "kind": "line", "stitch": "running"},
    ]));
    let thin = area(&d, 353, 100);
    assert_eq!(found(&c, &d), vec![Found::Applies(thin), Found::Kept(KeptWhy::OtherKind)]);
}

#[test]
fn a_change_on_a_colour_not_sewn_is_kept() {
    let d = digitize(&picture(), &json!({"colorChoices": [{"color": hex(ORANGE), "job": "none"}]}));
    let c = changes(json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"}]));
    assert_eq!(found(&c, &d), vec![Found::Kept(KeptWhy::NotSewn)]);
}

#[test]
fn two_changes_on_one_area_the_later_wins_key_by_key() {
    let d = digitize(&picture(), &json!({}));
    let c = changes(json!([
        {"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami", "angle": 10.0},
        {"at": [60.5, 60.5], "color": hex(ORANGE), "kind": "fill", "angle": 30.0},
        {"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "none"},
    ]));
    let f = found(&c, &d);
    let all = per_area(&c, &f, d.areas().areas.len());
    let (orange, navy) = (area(&d, 80, 100), area(&d, 210, 100));
    assert_eq!(all[orange], Some(Applied { stitch: Some(AreaStitch::Tatami), angle: Some(30.0) }));
    assert_eq!(all[navy], Some(Applied { stitch: Some(AreaStitch::None), angle: None }));
    assert_eq!(all.iter().flatten().count(), 2, "the others unchanged");
}

/// A session holding `raster` placed at a tenth of a millimetre a pixel at the origin, selected.
fn placed(raster: &tr::Raster) -> Session {
    let mut s = super::super::test_support::blank();
    let png = raster.encode_png();
    let (width, height) = (raster.width, raster.height);
    s.edit("place", |d, sel| {
        d.images.insert("picture".into(), ImageBlob::new("image/png", png));
        let id = d.alloc_id();
        let layer = d.default_layer();
        let xf = Affine::scale(MM_PER_PX / MM_PER_PT);
        let image = ImageObject { key: "picture".into(), width, height, xf, link: None, placement: Default::default(), pixels: None };
        d.insert(layer, 0, Node::new(id, NodeKind::Image(Box::new(image))))?;
        sel.set([id]);
        Ok(())
    })
    .unwrap();
    s
}

/// The object Create made in `group` whose box as drawn (a stroke's width in it) holds `(x, y)`
/// millimetres, the smallest such.
fn object_at(d: &Document, group: NodeId, x: f64, y: f64) -> Node {
    let at = Point::new(x / MM_PER_PT, y / MM_PER_PT);
    let objects = d.node(group).and_then(Node::children).into_iter().flatten().flat_map(|b| b.children().into_iter().flatten());
    objects
        .filter_map(|o| d.bounds_of(&[o.id], true).filter(|r| r.contains(at)).map(|r| (r.area(), (**o).clone())))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, o)| o)
        .expect("an object there")
}

/// The settings `node` sets of its fill and its stroke.
fn parts(node: &Node) -> (Option<PartSettings>, Option<PartSettings>) {
    let e = node.extras().embroidery.as_deref();
    (e.and_then(|e| e.fill.as_deref().cloned()), e.and_then(|e| e.stroke.as_deref().cloned()))
}

/// Create sews each changed area as its change asks, as that area's own settings: a filled area as
/// tatami (its narrow parts too), as satin (narrow parts as satin up to its widest part and the
/// overlap), its rows at an angle; a line not sewn. The areas no change applies to are left as
/// their colour's job leaves them.
#[test]
fn create_sews_each_changed_area_as_its_change_asks() {
    assert!(super::super::resolve::Stitch::IDS.iter().any(|(id, _)| *id == super::create::SATIN), "a stitch the objects take");
    let mut s = placed(&picture());
    let changes = json!([
        {"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"},
        {"at": [310.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"},
        {"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "angle": 30.0},
        {"at": [353.0, 100.5], "color": hex(NAVY), "kind": "line", "stitch": "none"},
    ]);
    let r = s.execute("embroidery.digitizeImage", &json!({"areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let tatami = parts(&object_at(&d, group, 8.0, 10.0)).0;
    assert_eq!(tatami.map(|p| (p.narrow_as_satin, p.stitch, p.angle)), Some((Some(0.0), None, None)), "tatami, its narrow parts too");
    let satin = parts(&object_at(&d, group, 31.0, 10.0)).0.and_then(|p| p.narrow_as_satin).expect("satin set");
    assert!((2.0..=12.0).contains(&satin), "satin up to the bar's 2 mm and its overlap: {satin}");
    let angled = parts(&object_at(&d, group, 21.0, 10.0)).0;
    assert_eq!(angled.map(|p| (p.angle, p.narrow_as_satin, p.stitch)), Some((Some(30.0), None, None)), "its rows at 30°");
    let line = parts(&object_at(&d, group, 35.3, 10.0)).1;
    assert_eq!(line.and_then(|p| p.stitch).as_deref(), Some("none"), "the line not sewn");
}

/// A satin width is the area's widest part and the overlap on either side, in tenths of a
/// millimetre rounded up, at most 12 mm.
#[test]
fn a_satin_width_covers_the_whole_area() {
    let mut s = placed(&picture());
    let changes = json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let square = parts(&object_at(&d, group, 8.0, 10.0)).0.and_then(|p| p.narrow_as_satin);
    assert_eq!(square, Some(12.0), "a 12 mm square: at most 12 mm, its wider parts tatami");
}

/// Satin's width is measured on the shape the area is made as: a 10 mm square with a 3 mm dot sewn
/// over it is sewn solid, so its satin covers the 10 mm and the overlap, not the ring round the dot.
#[test]
fn a_satin_width_is_measured_with_the_islands_sewn_over_it() {
    let picture = tr::Raster::from_fn(200, 200, |x, y| {
        if inside(x, y, [85, 85, 115, 115]) {
            NAVY
        } else if inside(x, y, [50, 50, 150, 150]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let changes = json!([{"at": [60.5, 60.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let square = parts(&object_at(&d, group, 6.0, 6.0)).0.and_then(|p| p.narrow_as_satin).expect("satin set");
    assert!((10.0..=11.0).contains(&square), "the 10 mm square and its overlap, the dot filled in: {square}");
}

/// A satin width is the area's widest part and the default overlap (0.4 mm) on each side, rounded up
/// to a tenth of a millimetre: the orange bar's, as the reply measures it.
#[test]
fn a_satin_width_is_the_widest_part_and_the_overlap_rounded_up() {
    let mut s = placed(&picture());
    let dry = s.execute("embroidery.digitizeImage", &json!({"dryRun": true})).unwrap();
    let bar = dry["areas"].as_array().and_then(|a| a.iter().find(|a| a["at"][0].as_f64().is_some_and(|x| (300.0..320.0).contains(&x))).cloned());
    let widest = bar.and_then(|b| b["widest"].as_f64()).expect("the bar's widest part");
    let changes = json!([{"at": [310.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let satin = parts(&object_at(&d, group, 31.0, 10.0)).0.and_then(|p| p.narrow_as_satin).expect("satin set");
    let wanted = ((widest + 2.0 * super::settings::OVERLAP_MM) * 10.0 - 1e-9).ceil() / 10.0;
    assert!((satin - wanted).abs() < 1e-9, "{widest} mm and 0.4 mm a side, rounded up: {wanted}, not {satin}");
}

/// An orange bar 4 mm wide between two navy stripes 3 mm wide sewn as lines, on white: the bar
/// grows under each stripe to its middle, so it is made about 7 mm wide.
fn bar_between_lines(bar_px: u32) -> tr::Raster {
    let bottom = 50 + bar_px;
    tr::Raster::from_fn(200, bottom + 50, |x, y| {
        if inside(x, y, [20, 20, 180, 50]) || inside(x, y, [20, bottom, 180, bottom + 30]) {
            NAVY
        } else if inside(x, y, [20, 50, 180, bottom]) {
            ORANGE
        } else {
            WHITE
        }
    })
}

/// Satin's width covers how far an area reaches under the centre lines beside it, whatever the
/// overlap: the 4 mm bar between two 3 mm stripes is made about 7.1 mm wide, so its satin is that
/// and the overlap, not the 4 mm and the overlap.
#[test]
fn a_satin_width_covers_the_lines_an_area_grows_under() {
    let mut s = placed(&bar_between_lines(40));
    let lines = json!([{"color": hex(NAVY), "job": "lines"}]);
    let changes = json!([{"at": [100.5, 70.5], "color": hex(ORANGE), "kind": "fill", "stitch": "satin"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "colorChoices": lines, "areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let bar = parts(&object_at(&d, group, 10.0, 7.0)).0.and_then(|p| p.narrow_as_satin).expect("satin set");
    assert!(bar >= 7.0, "the bar as made, under half of each stripe: {bar}");
    // With no overlap: the bands' half pixel past the middle on each side counted too (7.1 mm made).
    let mut s = placed(&bar_between_lines(40));
    let r = s.execute("embroidery.digitizeImage", &json!({"colors": 2, "overlap": 0.0, "colorChoices": lines, "areaChanges": changes})).unwrap();
    let d = s.doc().unwrap().doc.clone();
    let group = NodeId(r["created"]["group"].as_u64().unwrap());
    let bar = parts(&object_at(&d, group, 10.0, 7.0)).0.and_then(|p| p.narrow_as_satin).expect("satin set");
    assert!(bar >= 7.1 - 1e-9, "the bar as made with no overlap, 7.1 mm: {bar}");
}

/// A colour whose only area a change leaves out sews no thread: the reply counts the threads sewn
/// without it, and the outlines' thread is the darkest of those sewn, not its own.
#[test]
fn a_colour_left_out_area_by_area_sews_no_thread() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 140, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) {
            NAVY
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let dont_sew = json!([{"at": [210.5, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "none"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"dryRun": true, "colors": 2, "colorOutlines": true, "areaChanges": dont_sew})).unwrap();
    assert_eq!(r["colorCount"], json!(1), "the orange's thread alone");
    let orange = r["colors"].as_array().and_then(|c| c.iter().find(|c| c["color"] == json!(hex(ORANGE)))).map(|c| c["thread"]["index"].clone());
    assert_eq!(Some(r["outlined"]["thread"]["index"].clone()), orange, "the outlines in the orange's thread: {}", r["outlined"]);
    let navy = r["colors"].as_array().and_then(|c| c.iter().find(|c| c["color"] == json!(hex(NAVY)))).map(|c| c["sewnShare"].clone());
    assert_eq!(navy, Some(json!(0.0)), "none of the navy sewn");
}

/// A colour merged into another, its area joined with that one's, sews nothing once the joined
/// area is left out: neither colour counts a thread, the merged one has no share sewn, and the
/// outlines take the thread still sewn.
#[test]
fn colours_joined_into_one_area_left_out_sew_no_thread() {
    const BLUE: [u8; 4] = [40, 60, 200, 255];
    let picture = tr::Raster::from_fn(400, 200, |x, y| {
        if inside(x, y, [20, 20, 120, 180]) {
            NAVY
        } else if inside(x, y, [120, 20, 220, 180]) {
            BLUE
        } else if inside(x, y, [260, 20, 380, 180]) {
            ORANGE
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let merged = json!([{"color": hex(BLUE), "mergeInto": hex(NAVY)}]);
    let dont_sew = json!([{"at": [70.5, 100.5], "color": hex(NAVY), "kind": "fill", "stitch": "none"}]);
    let r = s
        .execute(
            "embroidery.digitizeImage",
            &json!({"dryRun": true, "colors": 3, "colorChoices": merged, "colorOutlines": true, "areaChanges": dont_sew}),
        )
        .unwrap();
    assert_eq!(r["areaChanges"][0]["applies"], json!(true), "{}", r["areaChanges"]);
    assert_eq!(r["colorCount"], json!(1), "the orange's thread alone");
    let colour = |c: [u8; 4]| r["colors"].as_array().and_then(|l| l.iter().find(|x| x["color"] == json!(hex(c))).cloned()).unwrap_or(Value::Null);
    assert_eq!(colour(BLUE)["sewnShare"], json!(0.0), "the merged colour: none sewn: {}", colour(BLUE));
    assert_eq!(r["outlined"]["thread"]["index"], colour(ORANGE)["thread"]["index"], "the outlines in the orange's thread");
}

/// An area a change leaves out is sewn before none of the areas it touches, in the reply: the orange
/// square, sewn before the navy one against it, is no longer "under" it once left out.
#[test]
fn an_area_left_out_is_sewn_before_none_it_touches() {
    let picture = tr::Raster::from_fn(300, 200, |x, y| {
        if inside(x, y, [20, 20, 160, 180]) {
            ORANGE
        } else if inside(x, y, [160, 50, 260, 150]) {
            NAVY
        } else {
            WHITE
        }
    });
    let mut s = placed(&picture);
    let navy_under = |r: &Value| {
        let navy = r["areas"].as_array().and_then(|a| a.iter().find(|a| a["color"] == json!(hex(NAVY))).cloned()).expect("the navy square");
        navy["touches"].as_array().map(|t| t.iter().map(|t| t["under"].clone()).collect::<Vec<_>>()).unwrap_or_default()
    };
    let plain = s.execute("embroidery.digitizeImage", &json!({"dryRun": true, "colors": 2})).unwrap();
    assert_eq!(navy_under(&plain), vec![json!(true)], "the orange sewn before it");
    let dont_sew = json!([{"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "none"}]);
    let r = s.execute("embroidery.digitizeImage", &json!({"dryRun": true, "colors": 2, "areaChanges": dont_sew})).unwrap();
    assert_eq!(navy_under(&r), vec![json!(false)], "the orange left out: sewn before nothing");
    // Its order: taken as not sewn, and the navy sewn after nothing for it.
    let area = |c: [u8; 4]| r["areas"].as_array().and_then(|a| a.iter().find(|a| a["color"] == json!(hex(c))).cloned()).expect("an area");
    assert_eq!(area(ORANGE)["order"]["taken"], json!("notSewn"));
    assert_eq!(area(NAVY)["order"]["after"], json!([]), "{}", area(NAVY)["order"]);
}

/// The reply says which changes apply, and to which area by its number; each area lists what the
/// changes applied to it set.
#[test]
fn the_reply_says_which_changes_apply() {
    let mut s = placed(&picture());
    let changes = json!([
        {"at": [80.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "tatami"},
        {"at": [210.5, 100.5], "color": hex(ORANGE), "kind": "fill", "stitch": "none"},
    ]);
    let r = s.execute("embroidery.digitizeImage", &json!({"dryRun": true, "areaChanges": changes})).unwrap();
    let states = r["areaChanges"].as_array().expect("states");
    assert_eq!(states.get(1), Some(&json!({"applies": false, "area": null, "why": "otherColor"})));
    assert_eq!(states.first().map(|s| s["applies"].clone()), Some(json!(true)));
    let number = states.first().map(|s| s["area"].clone()).unwrap_or_default();
    let areas = r["areas"].as_array().expect("areas");
    let listed = areas.iter().find(|a| a["area"] == number).expect("its area listed");
    assert_eq!(listed["change"], json!({"stitch": "tatami"}));
    assert!(areas.iter().filter(|a| a["area"] != number).all(|a| a["change"].is_null()), "the others unchanged");
}
