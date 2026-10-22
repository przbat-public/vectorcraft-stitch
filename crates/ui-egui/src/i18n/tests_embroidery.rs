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

/// The embroidery rows of the export options dialog (labels, tips and the names in its lists) are
/// translated in every language.
#[test]
fn embroidery_export_dialog_is_translated() {
    let source = include_str!("../dialogs/embroidery_options.rs");
    let code = source.find("#[cfg(test)]").map_or(source, |at| &source[..at]);
    let keys: Vec<String> = code.match_indices("tl!(\"").filter_map(|(at, m)| literal(&code[at + m.len()..])).collect();
    assert!(keys.len() >= 30, "{keys:?}");
    // A row each (a translation may be the English word itself: "Satin" in German or French).
    for info in LANGUAGES.iter().filter(|l| l.code != "en") {
        let lang = Lang::from_code(info.code).unwrap();
        let (entries, _) = super::catalog::parse_entries(lang.0.source);
        let rows: std::collections::HashSet<&str> = entries.iter().filter(|(ctx, _, _)| ctx.is_empty()).map(|(_, src, _)| src.as_str()).collect();
        let untranslated: Vec<&String> = keys.iter().filter(|k| !rows.contains(k.as_str())).collect();
        assert!(untranslated.is_empty(), "{}: {untranslated:#?}", info.code);
    }
}
