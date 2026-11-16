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
    assert!(settings(json!({"areaChanges": vec![tatami(); most + 1]})).is_err(), "one more is refused");
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
