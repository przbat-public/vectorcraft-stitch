//! The Contextual Task Bar's stitch actions: only while stitches are edited with needle points
//! selected; Split Long Stitches as the format the Check tab checks for writes long stitches, when
//! its records are shorter than the longest stitch.

use serde_json::json;
use vectorcraft_doc::NodeId;
use vectorcraft_engine::Session;

use crate::{Services, VectorcraftApp};

#[test]
fn the_stitch_actions_show_only_while_needle_points_are_selected() {
    let mut app = VectorcraftApp::new(Session::new(), Services::default());
    app.embroidery.preview.pin_light();
    app.run("file.new", json!({"width": 300, "height": 300, "artboards": 1})).unwrap();
    let id = app.run("shape.rectangle", json!({"x": 60, "y": 60, "width": 120, "height": 90})).unwrap()["id"].as_u64().unwrap();
    app.run("embroidery.convertToManual", json!({"ids": [id]})).unwrap();
    app.run("tool.select", json!({"tool": "embroidery"})).unwrap();
    assert!(super::items(&mut app).is_none(), "not editing");
    app.run("object.isolate", json!({"id": id})).unwrap();
    app.run("view.stitchPreview", json!({"on": true})).unwrap();
    assert!(super::items(&mut app).is_none(), "editing, nothing selected");
    let run = app.session.active().unwrap().doc.node(NodeId(id)).unwrap().children().unwrap()[0].id;
    app.run("embroidery.selectNeedlePoints", json!({"needlePoints": [{"id": run.0, "index": 1}]})).unwrap();
    let items = super::items(&mut app).expect("the stitch actions");
    assert_eq!(items.len(), 7);
    let long = |app: &mut VectorcraftApp| super::items(app).unwrap().into_iter().find(|i| i.2 == "embroidery.splitLongStitches").unwrap().3;
    assert_eq!(long(&mut app), json!({}), "Janome's JEF, the default palette's format: its records as long as the longest stitch");
    let uid = app.session.active().unwrap().uid;
    app.embroidery.check.format.insert(uid, "dst");
    assert_eq!(long(&mut app), json!({"format": "dst"}), "DST's records are shorter: split as its writer would");
    app.run("tool.select", json!({"tool": "selection"})).unwrap();
    assert!(super::items(&mut app).is_none(), "another tool");
}
