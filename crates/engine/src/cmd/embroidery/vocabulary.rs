//! The embroidery settings' one vocabulary (the project owner's choice, 2026-10-22): each setting's
//! name in commands and replies (camelCase), the earlier name `document.export` also takes it
//! under, its key in the `.vectorcraft` file (the name in snake_case, as every key the file keeps
//! for embroidery), and where it can be set. One row per setting, so that a value set on an
//! object, given to one export and kept in the document has one name everywhere, and a value
//! resolved for an object can say which of those it came from. The units and ranges are in
//! `docs/embroidery.md` (the export's options' table, Settings on objects, Document settings) and
//! `super::setting_field`; the words the interface shows are the catalogues'.
//!
//! The document crate keeps the file keys it reads and writes as its own constants (it sits below
//! the engine); a test holds them to this table.

/// Where a setting can be set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// On an object's fill or stroke ([`vectorcraft_doc::embroidery::PartSettings`]).
    Part,
    /// Given to one export or one read command, for that call only (`document.export`'s options).
    Export,
    /// In the document's embroidery settings (`embroidery.setup`).
    Document,
    /// On an object, for the whole object (`embroidery.setTrimAfter`).
    Object,
}

/// One setting of the vocabulary.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Setting {
    /// Its name in commands and replies.
    pub(crate) name: &'static str,
    /// The earlier name `document.export` also takes it under; replies never use it.
    pub(crate) earlier: Option<&'static str>,
    /// Its key in the `.vectorcraft` file: the name in snake_case (held so at compile time).
    pub(crate) file: &'static str,
    /// Where it can be set.
    pub(crate) places: &'static [Place],
}

const PART_AND_EXPORT: &[Place] = &[Place::Part, Place::Export];
const EVERYWHERE: &[Place] = &[Place::Part, Place::Export, Place::Document];
const EXPORT_AND_DOCUMENT: &[Place] = &[Place::Export, Place::Document];
const PART: &[Place] = &[Place::Part];
const DOCUMENT: &[Place] = &[Place::Document];

/// Every setting, the export's options first, in the order `document.formats` lists them.
pub(crate) const SETTINGS: [Setting; 29] = [
    Setting { name: "stitch", earlier: Some("stitchStyle"), file: "stitch", places: PART_AND_EXPORT },
    Setting { name: "spacing", earlier: Some("density"), file: "spacing", places: EVERYWHERE },
    Setting { name: "length", earlier: Some("stitchLength"), file: "length", places: EVERYWHERE },
    Setting { name: "angle", earlier: Some("stitchAngle"), file: "angle", places: PART_AND_EXPORT },
    Setting { name: "pull", earlier: Some("pullCompensation"), file: "pull", places: EVERYWHERE },
    Setting { name: "pullPercent", earlier: Some("pullCompensationPercent"), file: "pull_percent", places: PART_AND_EXPORT },
    Setting { name: "push", earlier: Some("pushCompensation"), file: "push", places: PART_AND_EXPORT },
    Setting { name: "expand", earlier: None, file: "expand", places: PART_AND_EXPORT },
    Setting { name: "narrowAsSatin", earlier: None, file: "narrow_as_satin", places: PART_AND_EXPORT },
    Setting { name: "threadWeight", earlier: None, file: "thread_weight", places: EXPORT_AND_DOCUMENT },
    Setting { name: "underlay", earlier: None, file: "underlay", places: EVERYWHERE },
    Setting { name: "jumpLength", earlier: None, file: "jump_length", places: EXPORT_AND_DOCUMENT },
    Setting { name: "trimLength", earlier: None, file: "trim_length", places: EXPORT_AND_DOCUMENT },
    Setting { name: "minStitch", earlier: None, file: "min_stitch", places: EXPORT_AND_DOCUMENT },
    Setting { name: "splitLength", earlier: None, file: "split_length", places: EVERYWHERE },
    Setting { name: "entry", earlier: None, file: "entry", places: PART },
    Setting { name: "exit", earlier: None, file: "exit", places: PART },
    Setting { name: "spacingPercent", earlier: None, file: "spacing_percent", places: DOCUMENT },
    Setting { name: "stabilizer", earlier: None, file: "stabilizer", places: DOCUMENT },
    Setting { name: "topping", earlier: None, file: "topping", places: DOCUMENT },
    Setting { name: "needlePoint", earlier: None, file: "needle_point", places: DOCUMENT },
    Setting { name: "thickness", earlier: None, file: "thickness", places: DOCUMENT },
    Setting { name: "color", earlier: None, file: "color", places: DOCUMENT },
    Setting { name: "joinSameColor", earlier: None, file: "join_same_color", places: DOCUMENT },
    Setting { name: "speed", earlier: None, file: "speed", places: DOCUMENT },
    Setting { name: "palette", earlier: None, file: "palette", places: DOCUMENT },
    Setting { name: "start", earlier: None, file: "start", places: DOCUMENT },
    Setting { name: "label", earlier: None, file: "label", places: DOCUMENT },
    Setting { name: "trimAfter", earlier: None, file: "trim_after", places: &[Place::Object] },
];

impl Setting {
    /// Whether it can be set at `place`.
    pub(crate) fn at(&self, place: Place) -> bool {
        self.places.contains(&place)
    }
}

/// The export's options: the settings given to one export or read command, as `document.formats`
/// lists them.
pub(crate) fn options() -> impl Iterator<Item = &'static Setting> {
    SETTINGS.iter().filter(|s| s.at(Place::Export))
}

/// Whether `file` is `name` in snake_case: each capital letter `_` and its small letter, every
/// other character itself.
const fn is_snake_case_of(file: &str, name: &str) -> bool {
    let (file, name) = (file.as_bytes(), name.as_bytes());
    let (mut i, mut j) = (0, 0);
    while j < name.len() {
        let c = name[j];
        if c.is_ascii_uppercase() {
            if i + 1 >= file.len() || file[i] != b'_' || file[i + 1] != c.to_ascii_lowercase() {
                return false;
            }
            i += 2;
        } else {
            if i >= file.len() || file[i] != c {
                return false;
            }
            i += 1;
        }
        j += 1;
    }
    i == file.len()
}

// Every file key is its setting's name in snake_case.
const _: () = {
    let mut k = 0;
    while k < SETTINGS.len() {
        assert!(is_snake_case_of(SETTINGS[k].file, SETTINGS[k].name), "a file key that isn't its name in snake_case");
        k += 1;
    }
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The groups settings are kept in, not settings themselves: each one's name in commands and
    /// replies, its key in the file, and where it is (an object's fills and strokes; the document's
    /// fabric and machine).
    const GROUPS: [(&str, &str, Place); 4] = [
        ("fill", "fill", Place::Object),
        ("stroke", "stroke", Place::Object),
        ("fabric", "fabric", Place::Document),
        ("machine", "machine", Place::Document),
    ];

    /// No name or earlier name is given twice, and the snake-case check holds what it should.
    #[test]
    fn names_are_given_once() {
        let mut names: Vec<&str> = SETTINGS.iter().flat_map(|s| std::iter::once(s.name).chain(s.earlier)).collect();
        let all = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all, "a name given twice");
        assert!(is_snake_case_of("narrow_as_satin", "narrowAsSatin"));
        for (file, name) in [("narrowassatin", "narrowAsSatin"), ("narrow_as_satin_", "narrowAsSatin"), ("narrow_as", "narrowAsSatin")] {
            assert!(!is_snake_case_of(file, name), "{file}");
        }
    }

    /// The settings the document crate keeps on an object are the vocabulary's, under its file
    /// keys: those of its fills and strokes, and the cut after it.
    #[test]
    fn an_objects_settings_are_the_vocabularys() {
        let mut kept: Vec<&str> = Vec::new();
        for (name, file) in vectorcraft_doc::embroidery::file_keys() {
            let s = SETTINGS.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}: not in the vocabulary"));
            assert_eq!(s.file, file, "{name}");
            assert!(s.at(Place::Part) || s.at(Place::Object), "{name}");
            kept.push(name);
        }
        let mut on_objects: Vec<&str> = SETTINGS.iter().filter(|s| s.at(Place::Part) || s.at(Place::Object)).map(|s| s.name).collect();
        kept.sort_unstable();
        on_objects.sort_unstable();
        assert_eq!(kept, on_objects, "the vocabulary's settings on objects are those the document keeps");
    }

    /// The settings the document crate keeps in the document are the vocabulary's, under its file
    /// keys, and those are all the vocabulary's document settings.
    #[test]
    fn the_documents_settings_are_the_vocabularys() {
        let mut kept: Vec<&str> = Vec::new();
        for (name, file) in vectorcraft_doc::embroidery::setup_file_keys() {
            match GROUPS.iter().find(|(n, ..)| *n == name) {
                Some((_, group_file, place)) => assert_eq!((*group_file, *place), (file, Place::Document), "{name}"),
                None => {
                    let s = SETTINGS.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}: not in the vocabulary"));
                    assert_eq!(s.file, file, "{name}");
                    assert!(s.at(Place::Document), "{name}");
                }
            }
            kept.push(name);
        }
        let groups = GROUPS.iter().filter(|(.., place)| *place == Place::Document).map(|(name, ..)| *name);
        let mut in_document: Vec<&str> = SETTINGS.iter().filter(|s| s.at(Place::Document)).map(|s| s.name).chain(groups).collect();
        kept.sort_unstable();
        in_document.sort_unstable();
        assert_eq!(kept, in_document);
    }

    /// Every setting an object's part takes but its points has a field the Object tab edits it
    /// with, ranged as `embroidery.set` takes it (an object's `stitch` with `none`, never `fill`);
    /// every fabric value one as `embroidery.setup {fabric}` takes it; and the underlays offered
    /// under a stitch are the kinds it sews apart.
    #[test]
    fn every_part_and_fabric_setting_has_a_field() {
        use super::super::{SettingField, distinct_underlays, fabric_field, part_field};
        for s in SETTINGS.iter().filter(|s| s.at(Place::Part)) {
            let field = part_field(s.name);
            assert_eq!(field.is_none(), matches!(s.name, "entry" | "exit"), "{}", s.name);
        }
        assert_eq!(part_field("stitch"), Some(SettingField::Choice(vec!["auto", "tatami", "satin", "running", "none"])));
        assert_eq!((part_field("threadWeight"), part_field("nope")), (None, None), "not a part's");
        let Some(SettingField::Number { min, max, .. }) = fabric_field("tatami", "spacing") else { panic!("a number") };
        assert_eq!((min, max), vectorcraft_stitch::plan::FABRIC_SPACING_RANGE);
        for (kind, name) in [("tatami", "pull"), ("tatami", "length"), ("tatami", "underlay"), ("satin", "pull"), ("satin", "spacingPercent")] {
            assert!(fabric_field(kind, name).is_some(), "{kind}.{name}");
        }
        assert_eq!((fabric_field("satin", "length"), fabric_field("tatami", "spacingPercent")), (None, None));
        assert_eq!(distinct_underlays("tatami"), ["auto", "none", "edge", "edgeTatami"]);
        assert_eq!(distinct_underlays("satin"), ["auto", "none", "center", "edge", "edgeZigzag"]);
        assert_eq!(distinct_underlays("running").len(), vectorcraft_stitch::plan::Underlay::ALL.len());
    }

    /// The export's options are those `document.formats` lists for an embroidery format, in that
    /// order, each with a field to edit it.
    #[test]
    fn the_export_options_are_the_formats() {
        let listed: Vec<&str> = crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).collect();
        let ours: Vec<&str> = options().map(|s| s.name).collect();
        assert_eq!(ours, listed);
        for name in ours {
            assert!(super::super::setting_field(name).is_some(), "{name}");
        }
    }
}
