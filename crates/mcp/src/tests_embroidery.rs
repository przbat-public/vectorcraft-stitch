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
    assert_eq!(r, json!({"settings": {"palette": "jef"}, "resolved": {"palette": {"value": "jef", "source": "document"}}}));
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
