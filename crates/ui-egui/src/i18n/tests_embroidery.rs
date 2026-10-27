//! The fork's messages (machine embroidery) in every language: the export's warnings and errors,
//! and what opening a machine file says it left out, whose text the message scan in `tests` doesn't
//! read (they are constants, or joined as they are said).

use super::*;

/// The embroidery export's warnings, and its error when nothing is left to stitch, whose text the
/// scan above doesn't read (they are constants, or joined as the export runs), are translated whole
/// in every language.
#[test]
fn embroidery_export_messages_are_translated() {
    use serde_json::json;
    use vectorcraft_doc::embroidery::{EmbroiderySetup, ThreadPalette};
    use vectorcraft_engine::{EngineError, Session};
    let export = |s: &mut Session, expand: f64| s.execute("document.export", &json!({"format": "dst", "expand": expand}));
    // Nothing to stitch is the document's error, not the parameters'.
    let error = |e: EngineError| match e {
        EngineError::Other(msg) => msg,
        e => panic!("{e}"),
    };
    let rectangle = |s: &mut Session, x: u64, y: u64, width: u64, height: u64| {
        s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": width, "height": height})).unwrap()["id"].clone()
    };
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let mut messages = vec![error(export(&mut s, 0.0).unwrap_err())];
    s.execute("text.create", &json!({"x": 10, "y": 300, "text": "Stitch"})).unwrap();
    messages.push(error(export(&mut s, 0.0).unwrap_err()));
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    s.execute("paint.setFill", &json!({"color": "#cc2222"})).unwrap();
    rectangle(&mut s, 20, 20, 100, 60);
    // A sliver that a negative expand leaves nothing of, a picture, a blend and a gradient.
    rectangle(&mut s, 20, 100, 100, 2);
    let picture = rectangle(&mut s, 150, 20, 30, 30);
    s.execute("select.set", &json!({"ids": [picture]})).unwrap();
    s.execute("object.rasterize", &json!({})).unwrap();
    let keys = [rectangle(&mut s, 20, 150, 10, 10), rectangle(&mut s, 120, 150, 10, 10)];
    s.execute("object.blend.make", &json!({"ids": keys, "steps": 1})).unwrap();
    let gradient = rectangle(&mut s, 150, 100, 30, 30);
    s.execute("select.set", &json!({"ids": [gradient]})).unwrap();
    s.execute("paint.setFill", &json!({"gradient": {}})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    // An intertwine.
    let pair = [rectangle(&mut s, 200, 150, 20, 20), rectangle(&mut s, 210, 155, 20, 20)];
    s.execute("object.intertwine.make", &json!({"ids": pair})).unwrap();
    s.execute("select.none", &json!({})).unwrap();
    // A stroke too wide for a satin column (160 pt is 56 mm).
    s.execute("paint.setFill", &json!({"none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"color": "#2233cc"})).unwrap();
    s.execute("shape.line", &json!({"x1": 20, "y1": 400, "x2": 280, "y2": 400})).unwrap();
    s.execute("stroke.set", &json!({"weight": 160})).unwrap();
    // A thread palette this version doesn't know.
    s.edit("Embroidery Setup", |d, _| {
        let palette = Some(ThreadPalette::Unknown(json!("madeira")));
        d.embroidery = Some(Box::new(EmbroiderySetup { palette, ..EmbroiderySetup::default() }));
        Ok(())
    })
    .unwrap();
    let r = export(&mut s, -1.0).unwrap();
    let warnings: Vec<String> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str().map(str::to_string)).collect();
    assert_eq!(warnings.len(), 8, "{warnings:#?}");
    messages.extend(warnings);
    let english = ["stitch", "sewn", "painted", "this version", "expand them", "trace them", "outline it"];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for m in &messages {
            let shown = message(lang, m);
            let left = english.iter().find(|w| shown.replace("stitch fill", "").contains(**w));
            if shown == *m || left.is_some() {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated embroidery messages:\n{}", untranslated.join("\n"));
}

/// What a plan says of the document's and the objects' embroidery settings it can't take as they
/// are (some from a newer version, some out of range, a trim length below the jump length) is
/// translated whole in every language.
#[test]
fn embroidery_settings_notes_are_translated() {
    use serde_json::json;
    use vectorcraft_doc::NodeId;
    use vectorcraft_doc::embroidery::{Embroidery, EmbroiderySetup, MachineSettings};
    use vectorcraft_engine::Session;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("paint.setStroke", &json!({"none": true})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 100, "height": 60})).unwrap()["id"].as_u64().unwrap();
    s.edit("Embroidery Setup", |d, _| {
        let e: Embroidery = serde_json::from_value(json!({"fill": {"pull": 5.0, "grain": 45}})).unwrap();
        if let Some(n) = d.node_mut(NodeId(id)) {
            n.extras_mut().embroidery = Some(Box::new(e));
        }
        let machine = MachineSettings { jump_length: Some(7.0), trim_length: Some(3.0), ..MachineSettings::default() };
        let mut setup = EmbroiderySetup { thread_weight: Some(1000.0), machine: Some(Box::new(machine)), ..EmbroiderySetup::default() };
        setup.extra.insert("from_a_newer_version".into(), json!(true));
        d.embroidery = Some(Box::new(setup));
        Ok(())
    })
    .unwrap();
    let r = s.execute("document.export", &json!({"format": "dst"})).unwrap();
    let notes: Vec<&str> = r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
    assert_eq!(notes.len(), 5, "{notes:#?}");
    let english = ["this version", "embroidery settings", "jump length", "nearest value", "objects"];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for m in &notes {
            let shown = message(lang, m);
            if shown == *m || english.iter().any(|w| shown.contains(*w)) {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated notes:\n{}", untranslated.join("\n"));
}

/// What the settings commands on objects say when the objects given take none (a person running
/// them from the command palette meets it) is translated whole in every language.
#[test]
fn embroidery_object_settings_messages_are_translated() {
    use serde_json::json;
    use vectorcraft_doc::{AppearanceItem, NodeId};
    use vectorcraft_engine::{EngineError, Session};
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = |v: serde_json::Value| v["id"].as_u64().unwrap();
    let text = id(s.execute("text.create", &json!({"x": 10, "y": 300, "text": "Stitch"})).unwrap());
    let a = id(s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 50, "height": 40})).unwrap());
    let b = id(s.execute("shape.rectangle", &json!({"x": 90, "y": 20, "width": 50, "height": 40})).unwrap());
    s.edit("No Stroke", |d, _| {
        if let Some(n) = d.node_mut(NodeId(b)) {
            n.appearance.items.retain(|i| matches!(i, AppearanceItem::Fill(_)));
        }
        Ok(())
    })
    .unwrap();
    let group = id(s.execute("object.group", &json!({"ids": [a, b]})).unwrap());
    let error = |r: Result<serde_json::Value, EngineError>| match r {
        Err(EngineError::Other(m)) => m,
        r => panic!("{r:?}"),
    };
    let messages = [
        error(s.execute("embroidery.set", &json!({"ids": [text], "part": "fill", "angle": 3}))),
        error(s.execute("embroidery.reset", &json!({"ids": [text]}))),
        error(s.execute("embroidery.setEntryExit", &json!({"id": text, "part": "fill", "entry": [0, 0]}))),
        error(s.execute("embroidery.setEntryExit", &json!({"id": group, "part": "fill", "entry": [0, 0]}))),
        error(s.execute("embroidery.setEntryExit", &json!({"id": b, "part": "stroke", "entry": [0, 0]}))),
    ];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for m in &messages {
            let shown = message(lang, m);
            if shown == *m || ["none of these", "embroidery settings", "is a group", "has no", "takes no"].iter().any(|w| shown.contains(*w)) {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated messages:\n{}", untranslated.join("\n"));
}

/// What the manual-stitch commands say when they can't act (a person running them from the command
/// palette, or the Object tab, meets it) is translated whole in every language.
#[test]
fn manual_stitch_messages_are_translated() {
    use serde_json::json;
    use vectorcraft_engine::{EngineError, Session};
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let text = s.execute("text.create", &json!({"x": 10, "y": 300, "text": "Stitch"})).unwrap()["id"].as_u64().unwrap();
    let messages = match s.execute("embroidery.convertToManual", &json!({ "ids": [text] })) {
        Err(EngineError::Other(m)) => vec![m],
        r => panic!("{r:?}"),
    };
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for m in &messages {
            let shown = message(lang, m);
            if shown == *m || ["nothing selected", "manual stitches"].iter().any(|w| shown.contains(*w)) {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated messages:\n{}", untranslated.join("\n"));
}

/// What a plan says of a stroke an effect reshapes whose running or satin stitch can't follow its
/// path is translated whole in every language.
#[test]
fn embroidery_stroke_path_warning_is_translated() {
    use serde_json::json;
    use vectorcraft_engine::Session;
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 100, "height": 60})).unwrap()["id"].clone();
    s.execute("appearance.addStroke", &json!({"ids": [id]})).unwrap();
    s.execute("effect.apply", &json!({"effect": "distort.roughen", "ids": [id], "item": 1})).unwrap();
    s.execute("embroidery.set", &json!({"ids": [id], "part": "stroke", "stitch": "running"})).unwrap();
    let r = s.execute("document.export", &json!({"format": "dst"})).unwrap();
    let warning =
        r["warnings"].as_array().unwrap().iter().filter_map(|w| w.as_str()).find(|w| w.contains("reshaped by an effect")).unwrap().to_string();
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let shown = message(Lang::from_code(info.code).unwrap(), &warning);
        if shown == warning || ["brush", "reshaped", "can't follow", "stitch"].iter().any(|w| shown.contains(*w)) {
            untranslated.push(format!("{}: {shown}", info.code));
        }
    }
    assert!(untranslated.is_empty(), "untranslated:\n{}", untranslated.join("\n"));
}

/// What opening a machine file says it left out (which makes writing over the file ask first) is
/// translated whole in every language.
#[test]
fn embroidery_open_losses_are_translated() {
    // Phrases of the English notes no other language shares.
    let english = ["kept", "written as", "are read", "doesn't read", "this document", "nearest thread", "stitches"];
    let mut untranslated = Vec::new();
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        for note in vectorcraft_engine::EMBROIDERY_LOSS_NOTES {
            let shown = message(lang, note);
            if shown == note || english.iter().any(|w| shown.contains(*w)) {
                untranslated.push(format!("{}: {shown}", info.code));
            }
        }
    }
    assert!(untranslated.is_empty(), "untranslated notes:\n{}", untranslated.join("\n"));
}

/// The machine-file errors (the stitch crate's open and export errors, the embroidery import's and
/// the size limits), which upstream's message scan doesn't read (it covers neither the stitch crate
/// nor these error forms): every one has an `@msg` row in every language, and an error as it is
/// shown, its numbers filled in, comes out translated.
#[test]
fn machine_file_errors_are_translated() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    let mut dirs = vec![root.join("stitch").join("src")];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).unwrap().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            if path.is_dir() {
                dirs.push(path);
            } else if name.ends_with(".rs") && !name.starts_with("tests") {
                files.push(path);
            }
        }
    }
    let engine = root.join("engine").join("src").join("cmd");
    files.extend([
        engine.join("fileio").join("embroidery").join("import.rs"),
        engine.join("fileio").join("embroidery").join("mod.rs"),
        engine.join("embroidery").join("digitize.rs"),
    ]);
    let markers = ["Error::Format(", "Error::TooLarge(", "err(", "Other(", "Err(", "ok_or(", "ok_or_else(|| ", "bad(C, "];
    let mut messages = std::collections::BTreeSet::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        // Up to the inline test module.
        let code = text.find("#[cfg(test)]\nmod tests {").map_or(text.as_str(), |at| &text[..at]);
        for marker in markers {
            for (at, _) in code.match_indices(marker) {
                let rest = code[at + marker.len()..].trim_start();
                let rest = rest.strip_prefix("format!(").map_or(rest, str::trim_start);
                let Some(lit) = rest.strip_prefix('"').and_then(literal) else { continue };
                if lit.as_bytes().windows(3).any(|w| w.iter().all(u8::is_ascii_lowercase)) {
                    messages.insert(template(&lit));
                }
            }
        }
    }
    assert!(messages.len() >= 35, "the scan found only {}: {messages:#?}", messages.len());
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        let (entries, _) = super::catalog::parse_entries(lang.0.source);
        let rows: std::collections::HashSet<&str> = entries.iter().filter(|(ctx, _, _)| ctx == "@msg").map(|(_, src, _)| src.as_str()).collect();
        let missing: Vec<&String> = messages.iter().filter(|m| !rows.contains(m.as_str())).collect();
        assert!(missing.is_empty(), "{}: {} untranslated machine-file errors: {missing:#?}", info.code, missing.len());
    }
    // As they are shown: numbers in place of the placeholders.
    let de = Lang::from_code("de").unwrap();
    assert_eq!(message(de, "the JEF file is cut short: 12 bytes"), "die JEF-Datei ist abgeschnitten: 12 Bytes");
    let too_large = message(de, "the design is 400 × 300 mm, larger than Tajima DST can hold (1000 mm a side)");
    assert!(too_large.contains("Tajima DST") && !too_large.contains("larger than"), "{too_large}");
}

/// A string literal's text up to its closing quote, escapes resolved.
fn literal(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = s.chars();
    loop {
        match it.next()? {
            '"' => return Some(out),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
}

/// A format string as a catalogue template: `{}` becomes `{_1}`, `{_2}`…, and format specs go.
fn template(s: &str) -> String {
    let (mut out, mut unnamed, mut rest) = (String::new(), 0, s);
    while let Some(a) = rest.find('{') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        let Some(b) = after.find('}') else { break };
        match after[..b].split(':').next().unwrap_or("") {
            "" => {
                unnamed += 1;
                out.push_str(&format!("{{_{unnamed}}}"));
            }
            name => out.push_str(&format!("{{{name}}}")),
        }
        rest = &after[b + 1..];
    }
    out.push_str(rest);
    out
}

/// The strings the embroidery interface shows through `et!("…")` and the counts it shows through
/// `etn!(n, "…", "…")` in this crate's sources (test files and inline test modules aside): what
/// [`every_embroidery_string_is_translated`] checks.
#[derive(Default)]
struct EmbroideryText {
    strings: std::collections::BTreeSet<String>,
    plurals: std::collections::BTreeSet<(String, String)>,
}

/// Whether `code[..at]` ends a macro name of its own (not inside a longer name, such as `set!(`).
fn starts_a_name(code: &str, at: usize) -> bool {
    code[..at].chars().next_back().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// [`EmbroideryText`] of `source`, its comment lines left out (a doc comment may show the macros).
/// A macro's string may follow it on the next line, where rustfmt puts a long one.
fn embroidery_text_in(source: &str, out: &mut EmbroideryText) {
    let code: String = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    let code = code.as_str();
    for (at, m) in code.match_indices("et!(") {
        let rest = code[at + m.len()..].trim_start();
        if starts_a_name(code, at)
            && let Some(lit) = rest.strip_prefix('"').and_then(literal)
        {
            out.strings.insert(lit);
        }
    }
    for (at, m) in code.match_indices("etn!(") {
        if !starts_a_name(code, at) {
            continue;
        }
        let rest = &code[at + m.len()..];
        // The count, then its two forms.
        let Some(first) = rest.find('"') else { continue };
        let Some(one) = literal(&rest[first + 1..]) else { continue };
        let after = &rest[first + 1..];
        let Some(end) = after.find('"').and_then(|e| after[e + 1..].find('"').map(|f| e + 1 + f)) else { continue };
        let Some(other) = literal(&after[end + 1..]) else { continue };
        out.plurals.insert((one, other));
    }
}

/// [`EmbroideryText`] of every source file of this crate.
fn embroidery_text() -> EmbroideryText {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = EmbroideryText::default();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                stack.push(path);
            } else if name.ends_with(".rs") && !name.starts_with("tests") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                embroidery_text_in(shipped(&text), &mut out);
            }
        }
    }
    out
}

/// How many plural forms `info`'s rule tells apart.
fn plural_forms(info: &LangInfo) -> usize {
    let forms: std::collections::BTreeSet<usize> = (0..=1000u64).map(info.plural).collect();
    forms.len()
}

/// Every string the embroidery interface shows has its `embroidery` row, and every count its
/// `@plural` row with as many forms as the language has, in each catalogue that claims complete
/// menus: a missing row would show English (the lookup is strict).
#[test]
fn every_embroidery_string_is_translated() {
    let text = embroidery_text();
    let context = crate::embroidery::text::CONTEXT;
    for info in LANGUAGES.iter().filter(|l| l.complete_menus) {
        let cat = info.catalog();
        let missing: Vec<&String> = text.strings.iter().filter(|s| cat.contextual(context, s).is_none()).collect();
        assert!(missing.is_empty(), "{}: {} untranslated embroidery strings: {missing:#?}", info.code, missing.len());
        let (entries, _) = super::catalog::parse_entries(info.source);
        for (one, other) in &text.plurals {
            let key = format!("{one}|{other}");
            let row = entries.iter().find(|(ctx, src, _)| ctx == "@plural" && *src == key);
            let forms = row.map(|(_, _, tr)| tr.split('|').count());
            assert_eq!(forms, Some(plural_forms(info)), "{}: the plural row of {key:?}", info.code);
        }
    }
}

/// Every `embroidery` row keeps its source's ellipsis (upstream's consistency test checks plain
/// rows only).
#[test]
fn embroidery_rows_keep_their_ellipsis() {
    let context = crate::embroidery::text::CONTEXT;
    for info in LANGUAGES.iter().filter(|l| !l.source.is_empty()) {
        let (entries, _) = super::catalog::parse_entries(info.source);
        for (_, src, tr) in entries.iter().filter(|(ctx, _, _)| ctx == context) {
            assert_eq!(src.ends_with('…'), tr.ends_with('…'), "{}: ellipsis in {src:?}", info.code);
        }
    }
}

/// The embroidery interface's files show their strings through `et!` and `etn!`, never `tl!`,
/// whose plain rows may mean something else ("Art" is the Art brush).
#[test]
fn the_embroidery_interface_never_uses_plain_rows() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // The fork's own interface: its module and its panel.
    let mut stack = vec![src.join("embroidery"), src.join("panels").join("embroidery")];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let code = shipped(&text);
                let plain = code.match_indices("tl!(").any(|(at, _)| starts_a_name(code, at));
                assert!(!plain, "{} shows a string through tl!", path.display());
            }
        }
    }
}

/// The scan finds what the macros are given, a string on the line after its macro too, and nothing
/// inside longer names.
#[test]
fn the_embroidery_text_scan_reads_both_macros() {
    let mut out = EmbroideryText::default();
    let code = "/// crate::et!(\"Comment\")\nlet a = crate::et!(\"Stitches\"); set!(\"no\");\nlet b = crate::et!(\n    \"Long\"\n);\ncrate::etn!(\n    n as u64,\n    \"{count} stitch\",\n    \"{count} stitches\"\n);";
    embroidery_text_in(code, &mut out);
    assert_eq!(out.strings.iter().collect::<Vec<_>>(), ["Long", "Stitches"]);
    assert_eq!(out.plurals.iter().collect::<Vec<_>>(), [&("{count} stitch".to_string(), "{count} stitches".to_string())]);
}

/// A source file's shipped code: all of it before its first test module (`#[cfg(test)]` then a
/// `mod`). A `#[cfg(test)]` on a helper further up doesn't end it, so the strings after one are
/// still read.
fn shipped(text: &str) -> &str {
    text.find("#[cfg(test)]\nmod ").map_or(text, |at| text.get(..at).unwrap_or(text))
}

/// The scans read past a test helper, up to the test module.
#[test]
fn the_scans_read_past_a_test_helper() {
    let code = "let a = crate::et!(\"Before\");\n#[cfg(test)]\nfn helper() {}\nlet b = crate::et!(\"After\");\n#[cfg(test)]\nmod tests;\nlet c = crate::et!(\"Test\");";
    let mut out = EmbroideryText::default();
    embroidery_text_in(shipped(code), &mut out);
    assert_eq!(out.strings.iter().collect::<Vec<_>>(), ["After", "Before"]);
}
