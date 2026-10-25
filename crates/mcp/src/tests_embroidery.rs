//! The embroidery commands through MCP: listed, run like any other command, and their params
//! checked against their description.

use serde_json::{Value, json};

use crate::{Headless, call_tool};

fn text(r: &crate::ToolResult) -> Value {
    assert!(!r.is_error, "{r:?}");
    serde_json::from_str(r.content[0]["text"].as_str().unwrap()).unwrap()
}

/// `list_commands` lists the embroidery commands, and `run_command` runs them.
#[test]
fn the_embroidery_commands_are_reachable() {
    let mut h = Headless::with_document();
    let listed = text(&call_tool(&mut h, "list_commands", &json!({"filter": "embroidery."})));
    let ids: Vec<&str> = listed["commands"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    for id in ["embroidery.setTrimAfter", "embroidery.setup", "embroidery.settings", "embroidery.threads"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup", "params": {"palette": "jef"}})));
    assert_eq!((&r["settings"]["palette"], &r["resolved"]["palette"]), (&json!("jef"), &json!({"value": "jef", "source": "document"})));
    assert_eq!(h.session.doc().unwrap().doc.embroidery.as_deref().and_then(|e| e.palette.as_ref()).and_then(|p| p.id()), Some("jef"));
    // A misspelt key is refused before the command runs.
    let typo = call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup", "params": {"palete": "pec"}}));
    assert!(typo.is_error, "{typo:?}");
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.settings"})));
    assert_eq!(r["settings"]["palette"], "jef");
    // The resolved values are only reported: passing them back is refused.
    let back = call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup", "params": r}));
    assert!(back.is_error, "{back:?}");
    let rect = json!({"command": "shape.rectangle", "params": {"x": 10, "y": 10, "width": 50, "height": 40}});
    text(&call_tool(&mut h, "run_command", &rect));
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.threads", "params": {"stitchStyle": "fill"}})));
    assert_eq!(r["palette"], "jef");
    let blocks = r["blocks"].as_array().unwrap();
    assert!(!blocks.is_empty() && blocks.iter().all(|b| b["thread"]["palette"] == "jef" && b["thread"]["index"].as_u64().is_some()), "{r}");
    // So is a key that isn't one of the embroidery options.
    for given in [json!({"densitty": 0.4}), json!({"format": "pes"})] {
        let refused = call_tool(&mut h, "run_command", &json!({"command": "embroidery.threads", "params": given}));
        assert!(refused.is_error, "{refused:?}");
    }
}

/// The design's figures, sewing order and checks are reachable too, with no params or with
/// their own, and a key another of them takes is refused.
#[test]
fn the_stats_sequence_and_check_are_reachable() {
    let mut h = Headless::with_document();
    let listed = text(&call_tool(&mut h, "list_commands", &json!({"filter": "embroidery."})));
    let ids: Vec<&str> = listed["commands"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    for id in ["embroidery.stats", "embroidery.sequence", "embroidery.check"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let rect = json!({"command": "shape.rectangle", "params": {"x": 10, "y": 10, "width": 50, "height": 40}});
    text(&call_tool(&mut h, "run_command", &rect));
    let stats = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.stats"})));
    assert!(stats["stitches"].as_u64().unwrap() > 0, "{stats}");
    let stats = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.stats", "params": {"speed": 800}})));
    assert_eq!(stats["time"]["speed"], 800.0);
    let order = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.sequence", "params": {"stitchStyle": "fill"}})));
    assert_eq!(order["blocks"][0]["units"][0]["sewn"], "tatami");
    let check = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.check", "params": {"format": "dst", "limit": 3}})));
    assert_eq!(check["format"], "dst");
    for (id, given) in
        [("embroidery.stats", json!({"format": "pes"})), ("embroidery.check", json!({"speed": 800})), ("embroidery.sequence", json!({"speed": 800}))]
    {
        let refused = call_tool(&mut h, "run_command", &json!({"command": id, "params": given}));
        assert!(refused.is_error, "{id} {refused:?}");
    }
}

/// The stitch preview's view commands are reachable, set the setting the canvas reads, and refuse
/// a key they don't take.
#[test]
fn the_stitch_preview_commands_are_reachable() {
    let mut h = Headless::with_document();
    let listed = text(&call_tool(&mut h, "list_commands", &json!({"filter": "view.stitchPreview"})));
    let ids: Vec<&str> = listed["commands"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    for id in ["view.stitchPreview", "view.stitchPreview.jumps", "view.stitchPreview.needlePoints", "view.stitchPreview.artwork"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "view.stitchPreview", "params": {"on": true}})));
    assert_eq!((&r["on"], &r["shown"]), (&json!(true), &json!("stitches")), "{r}");
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "view.stitchPreview.artwork"})));
    assert_eq!(r["artwork"], true);
    assert!(h.session.stitch_view().on && h.session.stitch_view().artwork);
    let refused = call_tool(&mut h, "run_command", &json!({"command": "view.stitchPreview", "params": {"show": true}}));
    assert!(refused.is_error, "{refused:?}");
}

/// `embroidery.render` answers a picture of the stitches through MCP, its size bounded by the
/// scale, and refuses what it can't draw; the sew commands move an object by a step and refuse
/// ids that aren't the document's.
#[test]
fn the_picture_and_the_sew_commands_are_reachable() {
    let mut h = Headless::with_document();
    for (x, color) in [(10, "#d01818"), (80, "#2a6fdb")] {
        text(&call_tool(&mut h, "run_command", &json!({"command": "paint.setFill", "params": {"color": color}})));
        let rect = json!({"command": "shape.rectangle", "params": {"x": x, "y": 10, "width": 50, "height": 40}});
        text(&call_tool(&mut h, "run_command", &rect));
    }
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.render", "params": {"scale": 2, "jumps": false}})));
    let (w, h_px) = (r["width"].as_u64().unwrap(), r["height"].as_u64().unwrap());
    let region = r["region"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!((w as f64 - (region[2] - region[0]) * 2.0).abs() <= 2.0 && h_px > 0, "{w}×{h_px} for {region:?}");
    assert!(r["dataBase64"].as_str().is_some_and(|d| d.len() > 100), "a picture");
    for given in [json!({"scale": 1000}), json!({"rect": [0, 0, 1e308, 10]}), json!({"background": "red"}), json!({"zoom": 2})] {
        let refused = call_tool(&mut h, "run_command", &json!({"command": "embroidery.render", "params": given}));
        assert!(refused.is_error, "{given}: {refused:?}");
    }
    let ids: Vec<u64> = h.session.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    let moved = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.sewFirst", "params": {"ids": [ids[1]]}})));
    assert_eq!(moved["ids"], json!([ids[1]]), "{moved}");
    let after: Vec<u64> = h.session.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|n| n.id.0).collect();
    assert_eq!(after.first(), Some(&ids[1]), "sewn first: the bottom of the stack");
    for given in [json!({"ids": [999_999]}), json!({"ids": "all"}), json!({"ids": [ids[0]], "speed": 800})] {
        let refused = call_tool(&mut h, "run_command", &json!({"command": "embroidery.sewLast", "params": given}));
        assert!(refused.is_error, "{given}: {refused:?}");
    }
}

/// The settings commands on objects are listed and run through MCP: a part's values set, copied,
/// pointed and reset; an earlier name refused before the command runs.
#[test]
fn the_object_settings_commands_are_reachable() {
    let mut h = Headless::with_document();
    let listed = text(&call_tool(&mut h, "list_commands", &json!({"filter": "embroidery."})));
    let ids: Vec<&str> = listed["commands"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    for id in ["embroidery.set", "embroidery.reset", "embroidery.setEntryExit", "embroidery.copyFrom", "embroidery.info"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let rect = |x: u64| json!({"command": "shape.rectangle", "params": {"x": x, "y": 10, "width": 50, "height": 40}});
    let a = text(&call_tool(&mut h, "run_command", &rect(10)))["id"].clone();
    let b = text(&call_tool(&mut h, "run_command", &rect(100)))["id"].clone();
    let run = |h: &mut Headless, command: &str, params: Value| text(&call_tool(h, "run_command", &json!({"command": command, "params": params})));
    let r = run(&mut h, "embroidery.set", json!({"ids": [a], "part": "fill", "angle": 30, "pull": 0.3}));
    assert_eq!((&r["changedCount"], &r["settings"]["pull"]), (&json!(1), &json!([0.3, 0.3])));
    let r = run(&mut h, "embroidery.copyFrom", json!({"source": a, "ids": [b]}));
    assert_eq!(r["changedCount"], 1);
    let r = run(&mut h, "embroidery.setEntryExit", json!({"id": b, "part": "fill", "entry": [100, 10]}));
    assert_eq!(r["entry"], json!([100.0, 10.0]));
    let r = run(&mut h, "embroidery.info", json!({"ids": [a, b], "plan": true}));
    assert_eq!((&r["objectCount"], &r["fill"]["settings"]["angle"]["value"]), (&json!(2), &json!(30.0)));
    let r = run(&mut h, "embroidery.reset", json!({"ids": [a, b]}));
    assert_eq!(r["changedCount"], 2);
    let alias = call_tool(&mut h, "run_command", &json!({"command": "embroidery.set", "params": {"ids": [a], "part": "fill", "density": 0.4}}));
    assert!(alias.is_error, "{alias:?}");
}

/// The fabrics are listed, one chosen for the document and applied to the objects through MCP.
#[test]
fn the_fabric_commands_are_reachable() {
    let mut h = Headless::with_document();
    let run = |h: &mut Headless, command: &str, params: Value| text(&call_tool(h, "run_command", &json!({"command": command, "params": params})));
    let fabrics = run(&mut h, "embroidery.fabrics", json!({}));
    assert_eq!(fabrics["fabrics"].as_array().unwrap().len(), 8);
    let r = run(&mut h, "embroidery.setup", json!({"fabric": "denim"}));
    assert_eq!(r["resolved"]["fabric"]["preset"], json!({"id": "denim", "known": true}));
    let r = run(&mut h, "embroidery.applyFabric", json!({}));
    assert_eq!(r["changedCount"], 0);
}
