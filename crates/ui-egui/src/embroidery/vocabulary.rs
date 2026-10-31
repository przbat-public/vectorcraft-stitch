//! The words of the embroidery settings in the interface language: each setting's label, each
//! choice's name, where a value comes from, what Auto chose, and why a part or an object isn't
//! sewn. The engine's ids in (`embroidery.info`'s, `embroidery.settings`'), the catalogues' words
//! out ([`crate::et!`]): the engine's English is never shown. An id this version doesn't know (a
//! newer version's) reads as itself, so nothing is hidden.

/// The most characters of an id this version doesn't know (a newer version's), or of a file's
/// design name, the interface shows: a hostile file can hold any string, which would be laid out
/// on every frame. An estimate.
pub const MAX_SHOWN: usize = 64;

/// `words` as the interface shows an id it doesn't know or a file's name: at most [`MAX_SHOWN`]
/// characters, cut with "…".
pub fn shortened(words: &str) -> String {
    if words.chars().count() <= MAX_SHOWN { words.to_string() } else { format!("{}…", words.chars().take(MAX_SHOWN).collect::<String>()) }
}

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

/// A fabric value objects set themselves, as Document Setup's bar counts them by `overrides`' keys:
/// a fill's or a stroke's, so by its plain name ("Spacing" counts a fill's rows' and a satin's;
/// "Underlay", never the stitch's "Type").
pub fn override_label(key: &str) -> &'static str {
    match key {
        "underlay" => crate::et!("Underlay"),
        other => label(other, None),
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
        other => shortened(other),
    }
}

/// What a part sewn by the rule (Auto) is sewn as, in words after "Auto:": `sewn` as
/// `embroidery.info` reports it, by `rule` (a stroke a brush or an effect reshapes is its outline,
/// filled).
pub fn sewn_as(sewn: &str, rule: Option<&str>) -> String {
    match (sewn, rule) {
        (_, Some("strokeOutlined")) => crate::et!("filled outline").to_string(),
        ("tatami", _) => crate::et!("tatami").to_string(),
        ("satin", _) => crate::et!("satin").to_string(),
        ("running", _) => crate::et!("running stitch").to_string(),
        (other, _) => shortened(other),
    }
}

/// "Auto: {choice}": what Auto chose.
pub fn auto(choice: &str) -> String {
    crate::embroidery::text::fill(crate::et!("Auto: {choice}"), &[("choice", choice)])
}

/// An underlay's name in a list (`underlay`'s ids).
pub fn underlay(id: &str) -> String {
    match id {
        "auto" => crate::et!("Auto").to_string(),
        "none" => crate::et!("None").to_string(),
        "center" => crate::et!("Center run").to_string(),
        "edge" => crate::et!("Edge run").to_string(),
        "edgeZigzag" => crate::et!("Edge run + zigzag").to_string(),
        "edgeTatami" => crate::et!("Edge run + tatami").to_string(),
        other => shortened(other),
    }
}

/// What Auto chose for an underlay, in words after "Auto:": a satin's kind by its width (`chose`),
/// or a fill's by its size (no kind: an edge run, and a tatami layer under a large fill).
pub fn underlay_chosen(chose: Option<&str>) -> String {
    match chose {
        Some("center") => crate::et!("center run").to_string(),
        Some("edge") => crate::et!("edge run").to_string(),
        Some("edgeZigzag") => crate::et!("edge run + zigzag").to_string(),
        Some("edgeTatami") => crate::et!("edge run + tatami").to_string(),
        Some("none") => crate::et!("none").to_string(),
        Some(other) => shortened(other),
        None => crate::et!("by size").to_string(),
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
        (other, _) => shortened(other),
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
        // Said only of manual stitches outside the object they were converted from (ungrouped,
        // moved out or pasted alone): those inside it are said with Restore Object.
        "manual" => crate::et!("Manual stitches are sewn as they are placed; the object they were converted from isn't kept with them."),
        "type" => crate::et!("Type isn't sewn yet: create outlines to sew it."),
        "image" => crate::et!("Images aren't sewn."),
        "symbolInstance" => crate::et!("A symbol's objects take its settings: edit the symbol to set them."),
        "guide" => crate::et!("Guides aren't sewn."),
        "placed" => crate::et!("Placed files aren't sewn."),
        "live" | "intertwine" => crate::et!("Not sewn as it is: expand it to sew it."),
        _ => crate::et!("It takes no embroidery settings."),
    }
}

/// A fabric preset's name (`embroidery.fabrics`' ids); a preset this version doesn't know by its id.
pub fn fabric(id: &str) -> String {
    match id {
        "cotton" => crate::et!("Cotton (light woven)").to_string(),
        "denim" => crate::et!("Denim or heavy twill").to_string(),
        "stretchKnit" => crate::et!("Stretch knit").to_string(),
        "pique" => crate::et!("Piqué (polo knit)").to_string(),
        "fleece" => crate::et!("Fleece").to_string(),
        "silk" => crate::et!("Silk or satin fabric").to_string(),
        "nylon" => crate::et!("Nylon or sports fabric").to_string(),
        "cap" => crate::et!("Cap (structured)").to_string(),
        other => shortened(other),
    }
}

/// A document setting's label in Document Setup's Embroidery tab, by its field there without the
/// `embroidery.` prefix: a fabric value by its kind of stitch and name (`tatami.pull`), the thread,
/// the machine's values (`machine.jumpLength`), the design's start and name.
pub fn setup_label(key: &str) -> &'static str {
    match key {
        "tatami.pull" | "satin.pull" => label("pull", None),
        "tatami.spacing" => label("spacing", Some("tatami")),
        "satin.spacingPercent" => label("spacing", Some("satin")),
        "tatami.length" => label("length", None),
        "tatami.underlay" | "satin.underlay" => crate::et!("Underlay"),
        "stabilizer" => crate::et!("Stabilizer"),
        "threadWeight" => crate::et!("Thread weight"),
        "palette" => crate::et!("Thread palette"),
        "machine.jumpLength" => crate::et!("Jump (needle up) from"),
        "machine.trimLength" => crate::et!("Cut the thread from"),
        "machine.minStitch" => crate::et!("Shortest stitch"),
        "machine.splitLength" => label("splitLength", None),
        "machine.speed" => crate::et!("Speed for the time estimate"),
        "start" => crate::et!("Start point"),
        "label" => crate::et!("Design name"),
        _ => crate::et!("Setting"),
    }
}

/// What a document setting does, as its tooltip in Document Setup says it ([`setup_label`]'s keys).
pub fn setup_tip(key: &str) -> &'static str {
    match key {
        "fabric" => crate::et!("The fabric the design is sewn on: its values are what objects use where they set none of their own."),
        "tatami.pull" | "satin.pull" => tip("pull"),
        "tatami.spacing" => crate::et!("The distance between a fill's rows, for 40 wt thread: another thread scales it."),
        "satin.spacingPercent" => crate::et!("A satin column's spacing as a share of the spacing its width picks."),
        "tatami.length" => tip("length"),
        "tatami.underlay" | "satin.underlay" => tip("underlay"),
        "stabilizer" => crate::et!("What to hoop under the fabric: for information, it changes no stitch."),
        "threadWeight" => crate::et!("The thread's weight: a thinner thread (a higher number) sews its rows closer."),
        "palette" => crate::et!("The threads a machine file numbers: Janome's in JEF files, Brother's in PES files."),
        "machine.jumpLength" => crate::et!("A move longer than this lifts the needle instead of sewing it."),
        "machine.trimLength" => crate::et!("A jump longer than this cuts the thread first."),
        "machine.minStitch" => crate::et!("Stitches shorter than this are left out."),
        "machine.splitLength" => tip("splitLength"),
        "machine.speed" => crate::et!("Stitches a minute: the time a design takes to sew is estimated at it."),
        "joinSameColor" => {
            crate::et!("Objects of one thread are sewn one after another when nothing between them overlaps them: fewer thread changes and cuts.")
        }
        "start" => crate::et!("Where the machine starts the design, as the file it was opened from gave it."),
        "label" => crate::et!("The design's name, as the file it was opened from gave it."),
        _ => "",
    }
}

/// A thread palette's name (`embroidery.setup {palette}`'s ids).
pub fn palette(id: &str) -> String {
    match id {
        "jef" => crate::et!("Janome (JEF)").to_string(),
        "pec" => crate::et!("Brother (PES)").to_string(),
        other => shortened(other),
    }
}

/// A fabric's stabiliser in words (`embroidery.fabrics`' ids); one this version doesn't know by its
/// id.
pub fn information(id: &str) -> String {
    match id {
        "tearAway" => crate::et!("Tear-away").to_string(),
        "cutAway" => crate::et!("Cut-away").to_string(),
        "adhesiveTearAway" => crate::et!("Adhesive tear-away").to_string(),
        "none" => crate::et!("None").to_string(),
        other => shortened(other),
    }
}
