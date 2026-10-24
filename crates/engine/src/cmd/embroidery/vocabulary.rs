//! The embroidery settings' one vocabulary (the project owner's choice, 2026-10-22): each setting's
//! name in commands and replies (camelCase), the earlier name `document.export` also takes it
//! under, its key in the `.vectorcraft` file (the name in snake_case, as every key the file keeps
//! for embroidery), and where it can be set. One row per setting, so that a value set on an
//! object, given to one export and kept in the document has one name everywhere, and a value
//! resolved for an object can say which of those it came from. The units, the ranges and the words
//! the interface shows are in `docs/embroidery.md` › Settings.
//!
//! The document crate keeps the file keys it reads and writes as its own constants (it sits below
//! the engine); a test holds them to this table.

/// Where a setting can be set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
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

const EXPORT: &[Place] = &[Place::Export];

/// Every setting, the export's options first, in the order `document.formats` lists them.
pub(crate) const SETTINGS: [Setting; 18] = [
    Setting { name: "stitch", earlier: Some("stitchStyle"), file: "stitch", places: EXPORT },
    Setting { name: "spacing", earlier: Some("density"), file: "spacing", places: EXPORT },
    Setting { name: "length", earlier: Some("stitchLength"), file: "length", places: EXPORT },
    Setting { name: "angle", earlier: Some("stitchAngle"), file: "angle", places: EXPORT },
    Setting { name: "pull", earlier: Some("pullCompensation"), file: "pull", places: EXPORT },
    Setting { name: "pullPercent", earlier: Some("pullCompensationPercent"), file: "pull_percent", places: EXPORT },
    Setting { name: "push", earlier: Some("pushCompensation"), file: "push", places: EXPORT },
    Setting { name: "expand", earlier: None, file: "expand", places: EXPORT },
    Setting { name: "narrowAsSatin", earlier: None, file: "narrow_as_satin", places: EXPORT },
    Setting { name: "threadWeight", earlier: None, file: "thread_weight", places: EXPORT },
    Setting { name: "underlay", earlier: None, file: "underlay", places: EXPORT },
    Setting { name: "jumpLength", earlier: None, file: "jump_length", places: EXPORT },
    Setting { name: "trimLength", earlier: None, file: "trim_length", places: EXPORT },
    Setting { name: "minStitch", earlier: None, file: "min_stitch", places: EXPORT },
    Setting { name: "palette", earlier: None, file: "palette", places: &[Place::Document] },
    Setting { name: "start", earlier: None, file: "start", places: &[Place::Document] },
    Setting { name: "label", earlier: None, file: "label", places: &[Place::Document] },
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

    /// The export's options are those `document.formats` lists for an embroidery format and the
    /// export options dialog edits, in that order.
    #[test]
    fn the_export_options_are_the_formats() {
        let listed: Vec<&str> = crate::cmd::fileio::embroidery::OPTIONS.iter().map(|o| o.name).collect();
        let ours: Vec<&str> = options().map(|s| s.name).collect();
        assert_eq!(ours, listed);
        for name in ours {
            assert!(super::super::dialog_field(name).is_some(), "{name}");
        }
    }
}
