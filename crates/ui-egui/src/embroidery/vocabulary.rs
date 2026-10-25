//! The words of the embroidery settings in the interface language: each setting's label, each
//! choice's name, where a value comes from, and why a part or an object isn't sewn. The engine's
//! ids in (`embroidery.info`'s, `embroidery.settings`'), the catalogues' words out
//! ([`crate::et!`]): the engine's English is never shown. An id this version doesn't know (a
//! newer version's) reads as itself, so nothing is hidden.

/// A part setting's label in the Object tab, by its key as `embroidery.set` names it, for a part
/// sewn as `sewn` (`tatami`, `satin`, `running`; a fill's spacing is its rows').
pub fn label(key: &str, sewn: Option<&str>) -> &'static str {
    match key {
        "stitch" | "underlay" => crate::et!("Type"),
        "angle" => crate::et!("Angle"),
        "spacing" if sewn == Some("tatami") => crate::et!("Row spacing"),
        "spacing" => crate::et!("Spacing"),
        "length" => crate::et!("Stitch length"),
        "narrowAsSatin" => crate::et!("Narrow parts as satin"),
        "pull" => crate::et!("Pull compensation"),
        "pullPercent" => crate::et!("Pull compensation (%)"),
        "push" => crate::et!("Push compensation"),
        "expand" => crate::et!("Expand fills"),
        "splitLength" => crate::et!("Split satin stitches from"),
        "entry" => crate::et!("Entry"),
        "exit" => crate::et!("Exit"),
        _ => crate::et!("Setting"),
    }
}

/// What a part setting does, as its tooltip says it.
pub fn tip(key: &str) -> &'static str {
    match key {
        "stitch" => crate::et!("How the part is sewn: Auto picks by its shape and width."),
        "angle" => crate::et!("The direction of the fill's rows, counter-clockwise; it turns with the object."),
        "spacing" => crate::et!("The distance between rows, or between a satin's stitches."),
        "length" => crate::et!("The longest stitch along a row or a running line."),
        "narrowAsSatin" => crate::et!("Parts of a fill narrower than this are sewn as satin columns; 0 sews none."),
        "underlay" => crate::et!("The stitches sewn first, under the top stitching, to hold the fabric."),
        "pull" => crate::et!("Widens each row or column at its ends, where the thread pulls the fabric in."),
        "pullPercent" => crate::et!("Widens each column by a share of its width."),
        "push" => crate::et!("Shortens an open satin column at its ends, where the thread pushes out."),
        "expand" => crate::et!("Grows the filled area all round, so that shapes drawn apart meet."),
        "splitLength" => crate::et!("Satin stitches longer than this are split in two."),
        "entry" => crate::et!("Where the needle goes in to sew the part."),
        "exit" => crate::et!("Where the needle comes out after the part."),
        _ => "",
    }
}

/// A stitch's name in a list (`stitch`'s ids): a stroke's tatami is its area filled.
pub fn stitch(id: &str, stroke: bool) -> String {
    match id {
        "auto" => crate::et!("Auto").to_string(),
        "tatami" | "fill" if stroke => crate::et!("Tatami fill").to_string(),
        "tatami" | "fill" => crate::et!("Tatami").to_string(),
        "satin" => crate::et!("Satin").to_string(),
        "running" => crate::et!("Running stitch").to_string(),
        "none" => crate::et!("Don't sew").to_string(),
        other => other.to_string(),
    }
}

/// An underlay's name in a list (`underlay`'s ids).
pub fn underlay(id: &str) -> String {
    match id {
        "auto" => crate::et!("Auto").to_string(),
        "none" => crate::et!("None").to_string(),
        "center" => crate::et!("Centre run").to_string(),
        "edge" => crate::et!("Edge run").to_string(),
        "edgeZigzag" => crate::et!("Edge run + zigzag").to_string(),
        "edgeTatami" => crate::et!("Edge run + tatami").to_string(),
        other => other.to_string(),
    }
}

/// Where a value comes from, as its badge names it: `embroidery.info`'s `source` and `section`.
pub fn source(source: &str, section: Option<&str>) -> String {
    match (source, section) {
        ("document", Some("fabric")) => crate::et!("fabric").to_string(),
        ("document", Some("machine")) => crate::et!("machine").to_string(),
        ("document", _) => crate::et!("document").to_string(),
        ("export", _) => crate::et!("export").to_string(),
        ("default", _) => crate::et!("default").to_string(),
        ("object", _) => crate::et!("set").to_string(),
        (other, _) => other.to_string(),
    }
}

/// What a value set on the objects would be unset: where it would come from, then its value in
/// words (`value`, already written as the field writes it), "fabric 0,20".
pub fn inherited(source: &str, section: Option<&str>, value: &str) -> String {
    let template = match (source, section) {
        ("document", Some("fabric")) => crate::et!("fabric {value}"),
        ("document", Some("machine")) => crate::et!("machine {value}"),
        ("document", _) => crate::et!("document {value}"),
        ("export", _) => crate::et!("export {value}"),
        _ => crate::et!("default {value}"),
    };
    crate::embroidery::text::fill(template, &[("value", value)])
}

/// Why nothing of a part is sewn: `embroidery.info`'s `reason`.
pub fn not_sewn(reason: Option<&str>) -> &'static str {
    match reason {
        Some("off") => crate::et!("Not sewn: set to Don't sew."),
        Some("unpainted") => crate::et!("Not sewn: it has no paint."),
        Some("hidden") => crate::et!("Not sewn: it is hidden."),
        Some("notPrinted") => crate::et!("Not sewn: it is on a template or non-printing layer."),
        _ => crate::et!("Not sewn."),
    }
}

/// Why an object takes no settings, as the Object tab says it: `embroidery.info`'s `skipped`
/// reasons.
pub fn takes_none(reason: &str) -> &'static str {
    match reason {
        "literal" => crate::et!("Stitches from a file are sewn as the file has them."),
        "type" => crate::et!("Type isn't sewn yet: create outlines to sew it."),
        "image" => crate::et!("Images aren't sewn."),
        "symbolInstance" => crate::et!("A symbol's objects take its settings: edit the symbol to set them."),
        "guide" => crate::et!("Guides aren't sewn."),
        "placed" => crate::et!("Placed files aren't sewn."),
        "live" | "intertwine" => crate::et!("Not sewn as it is: expand it to sew it."),
        _ => crate::et!("It takes no embroidery settings."),
    }
}
