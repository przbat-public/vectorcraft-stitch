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
    for id in ["embroidery.setTrimAfter", "embroidery.setup", "embroidery.threads"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup", "params": {"palette": "jef"}})));
    assert_eq!(r, json!({"palette": "jef", "threadPalette": "jef"}));
    assert_eq!(h.session.doc().unwrap().doc.embroidery.as_deref().and_then(|e| e.palette.as_ref()).and_then(|p| p.id()), Some("jef"));
    // A misspelt key is refused before the command runs.
    let typo = call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup", "params": {"palete": "pec"}}));
    assert!(typo.is_error, "{typo:?}");
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.setup"})));
    assert_eq!(r["palette"], "jef");
    let rect = json!({"command": "shape.rectangle", "params": {"x": 10, "y": 10, "width": 50, "height": 40}});
    text(&call_tool(&mut h, "run_command", &rect));
    let r = text(&call_tool(&mut h, "run_command", &json!({"command": "embroidery.threads", "params": {"stitchStyle": "fill"}})));
    assert_eq!(r["palette"], "jef");
    assert!(r["blocks"].as_array().unwrap().iter().all(|b| b["thread"]["palette"] == "jef" && b["thread"]["index"].as_u64().is_some()), "{r}");
}
