//! The stitch preview's view setting: per document, toggled and set by its commands, view state
//! (not undoable, not journaled, never marking a document modified), dropped on close, kept through
//! Revert; paused while an opacity mask or a pattern is edited; the layers the canvas draws; and
//! the options it plans with, from the last embroidery export.

use serde_json::{Value, json};
use vectorcraft_testkit::embroidery as fx;

use super::view::{Layers, OptionsSource, Paused, StitchView, layers};
use super::*;

fn runner(s: &mut Session) -> impl FnMut(&str, Value) -> Value + '_ {
    |c, p| s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

#[test]
fn the_stitch_view_is_toggled_and_set_per_document() {
    let mut s = Session::new();
    assert!(s.execute("view.stitchPreview", &json!({})).is_err(), "no document");
    fx::blank(&mut runner(&mut s));
    let first = s.active().unwrap().uid;
    let r = s.execute("view.stitchPreview", &json!({})).unwrap();
    assert_eq!(
        r,
        json!({"on": true, "jumps": true, "needlePoints": false, "artwork": false, "shown": "stitches", "pausedBy": null, "options": {"stitch": "auto"}, "optionsFrom": "defaults"})
    );
    assert_eq!(s.execute("view.stitchPreview", &json!({"on": true})).unwrap()["on"], true, "idempotent");
    assert_eq!(s.execute("view.stitchPreview", &json!({"on": null})).unwrap()["on"], false, "null toggles");
    // The options are kept, and set, while the preview is off.
    assert_eq!(s.execute("view.stitchPreview.jumps", &json!({})).unwrap()["jumps"], false);
    assert_eq!(s.execute("view.stitchPreview.needlePoints", &json!({"on": true})).unwrap()["needlePoints"], true);
    assert_eq!(s.execute("view.stitchPreview.artwork", &json!({"on": true})).unwrap()["artwork"], true);
    let set = s.stitch_view();
    assert!(!set.on && !set.jumps && set.needle_points && set.artwork);
    // Another document has its own; switching back returns the first's.
    fx::blank(&mut runner(&mut s));
    let second = s.active().unwrap().uid;
    assert_eq!(s.stitch_view(), StitchView::default());
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    assert_eq!(s.stitch_view_of(first), set);
    assert!(s.stitch_view_of(second).on);
    for bad in [json!({"on": "yes"}), json!({"on": 1}), json!({"zoom": 2}), json!([true])] {
        assert!(s.execute("view.stitchPreview", &bad).is_err(), "{bad}");
        assert!(s.execute("view.stitchPreview.jumps", &bad).is_err(), "{bad}");
    }
    assert_eq!(crate::cmd::find_command("view.stitchPreview").unwrap().shortcut, Some("Cmd+E"));
}

#[test]
fn the_stitch_view_is_view_state() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    fx::rect(&mut runner(&mut s), fx::RED, 10.0, 10.0, 50.0, 50.0);
    s.doc_mut().unwrap().mark_saved();
    let undo = s.doc().unwrap().history.undo.len();
    for id in ["view.stitchPreview", "view.stitchPreview.jumps", "view.stitchPreview.needlePoints", "view.stitchPreview.artwork"] {
        s.execute(id, &json!({})).unwrap();
        assert!(!crate::cmd::find_command(id).unwrap().journal, "{id} isn't journaled");
    }
    let st = s.doc().unwrap();
    assert_eq!(st.history.undo.len(), undo, "not undoable");
    assert!(!st.is_dirty(), "not a change to save");
    let uid = st.uid;
    // Revert keeps it (the document keeps its uid); closing drops it.
    let view = s.stitch_view();
    assert!(view.on);
    s.execute("file.revert", &json!({})).ok();
    assert_eq!(s.stitch_view_of(uid), view);
    s.execute("file.close", &json!({})).unwrap();
    assert!(!s.embroidery_plans.uids().contains(&uid), "dropped with the document");
}

#[test]
fn editing_an_opacity_mask_or_a_pattern_pauses_the_preview() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let a = fx::rect(&mut runner(&mut s), fx::RED, 10.0, 10.0, 50.0, 50.0);
    fx::rect(&mut runner(&mut s), fx::BLUE, 20.0, 20.0, 50.0, 50.0);
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("select.all", &json!({})).unwrap();
    s.execute("transparency.makeOpacityMask", &json!({})).unwrap();
    s.execute("transparency.editOpacityMask", &json!({"id": a})).unwrap();
    let r = s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    assert_eq!((&r["shown"], &r["pausedBy"]), (&json!("art"), &json!("opacityMask")));
    assert_eq!(view::paused(&s.doc().unwrap().doc), Some(Paused::OpacityMask));
    // A pattern being made (and so edited): paused too, the setting kept for afterwards.
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    let a = fx::rect(&mut runner(&mut s), fx::RED, 10.0, 10.0, 50.0, 50.0);
    s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    s.execute("select.set", &json!({"ids": [a]})).unwrap();
    s.execute("object.pattern.make", &json!({})).unwrap();
    let r = s.execute("view.stitchPreview", &json!({"on": true})).unwrap();
    assert_eq!((&r["shown"], &r["pausedBy"]), (&json!("art"), &json!("pattern")));
    s.execute("object.pattern.cancel", &json!({})).unwrap();
    assert_eq!(s.execute("view.stitchPreview", &json!({"on": true})).unwrap()["shown"], "stitches");
}

/// The layers the canvas draws, as a matrix: the art alone with the preview off or paused; else
/// the fabric and the stitches, the art faint under them when asked (not with Outline, whose
/// outlines go over them).
#[test]
fn the_canvas_layers_follow_the_setting() {
    let art = Layers { art: true, fabric: false, faint_art: false, stitches: false, outlines: false };
    let on = StitchView { on: true, ..StitchView::default() };
    let with_art = StitchView { artwork: true, ..on };
    for paused in [None, Some(Paused::OpacityMask), Some(Paused::Pattern)] {
        for outline in [false, true] {
            assert_eq!(layers(StitchView::default(), paused, outline), art, "off");
            for view in [on, with_art] {
                let l = layers(view, paused, outline);
                if paused.is_some() {
                    assert_eq!(l, art, "paused");
                    continue;
                }
                assert!(!l.art && l.fabric && l.stitches);
                assert_eq!(l.outlines, outline);
                assert_eq!(l.faint_art, view.artwork && !outline);
            }
        }
    }
}

/// The preview plans with the options of the document's last embroidery export, else the last
/// embroidery export's of any document, else the defaults; another format's export, or one whose
/// options are refused, changes nothing; closing a document forgets its own.
#[test]
fn the_preview_plans_with_the_last_embroidery_exports_options() {
    let mut s = Session::new();
    fx::blank(&mut runner(&mut s));
    fx::rect(&mut runner(&mut s), fx::RED, 10.0, 10.0, 50.0, 50.0);
    let first = s.active().unwrap().uid;
    let defaults = (Options::default(), OptionsSource::Defaults);
    assert_eq!(s.stitch_preview_options(first), defaults);
    s.execute("document.export", &json!({"format": "png"})).unwrap();
    assert_eq!(s.stitch_preview_options(first), defaults, "not an embroidery format");
    s.execute("document.export", &json!({"format": "dst", "spacing": 0.5})).unwrap();
    let ours = Options::from_json(&json!({"spacing": 0.5})).unwrap();
    assert_eq!(s.stitch_preview_options(first), (ours.clone(), OptionsSource::Document));
    assert!(s.execute("document.export", &json!({"format": "dst", "spacing": "wide"})).is_err());
    assert_eq!(s.stitch_preview_options(first), (ours.clone(), OptionsSource::Document), "a refused export notes nothing");
    // Nor one that can't be written.
    let nowhere = std::env::temp_dir().join("vectorcraft-no-such-folder").join("a.dst");
    let failed = json!({"format": "dst", "spacing": 0.3, "path": nowhere.to_string_lossy()});
    assert!(s.execute("document.export", &failed).is_err());
    assert_eq!(s.stitch_preview_options(first), (ours.clone(), OptionsSource::Document), "an export not written notes nothing");
    let r = s.execute("view.stitchPreview", &json!({})).unwrap();
    assert_eq!((&r["options"], &r["optionsFrom"]), (&json!({"stitch": "auto", "spacing": 0.5}), &json!("document")));
    // Another document plans with the app's last, until it is exported itself.
    fx::blank(&mut runner(&mut s));
    fx::rect(&mut runner(&mut s), fx::BLUE, 10.0, 10.0, 50.0, 50.0);
    let second = s.active().unwrap().uid;
    assert_eq!(s.stitch_preview_options(second), (ours.clone(), OptionsSource::App));
    s.execute("document.export", &json!({"format": "pes", "length": 3})).unwrap();
    let theirs = Options::from_json(&json!({"length": 3})).unwrap();
    assert_eq!(s.stitch_preview_options(second), (theirs.clone(), OptionsSource::Document));
    assert_eq!(s.stitch_preview_options(first), (ours, OptionsSource::Document), "the first keeps its own");
    s.execute("file.close", &json!({})).unwrap();
    assert_eq!(s.stitch_preview_options(second), (theirs, OptionsSource::App), "closing forgets the document's own");
}
