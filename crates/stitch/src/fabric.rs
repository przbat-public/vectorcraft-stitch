//! Fabric presets: what a fabric is sewn with, by the kind of stitch (tatami fills and satin
//! columns: pull compensation, spacing, stitch length, underlay), and what is said of it for
//! information (stabiliser, topping, needle point, thickness). A document copies a preset's values
//! when a person chooses it (`embroidery.setup {fabric}`); nothing links them afterwards.
//!
//! The fabrics are the eight auto fabrics Wilcom EmbroideryStudio names ([`WILCOM_FABRICS`]): cotton
//! (light woven, the default), denim and heavy twill, stretch knit, piqué (polo knit), fleece, silk
//! and satin fabric, nylon and sports fabric, cap backing. Their names here are our own words for
//! the fabrics, never product names.
//!
//! Every value is a published fact with its publication, worked out from published facts (said
//! how), or an estimate (said why) ([`Sourced`], [`Basis`]): the facts are cited, never a table
//! copied. Where no source gives a value for a fabric, the preset leaves it unset, and the built-in
//! default applies.
//!
//! **One family.** Every value that changes from fabric to fabric in how the stitches are made
//! comes from one family of software: Wilcom's own help and BERNINA's presets, and BERNINA's
//! embroidery software is Wilcom's (its V9 release notes carry Wilcom's copyright,
//! [`BERNINA_RELEASE_NOTES`]). No independent source confirms them ([`ONE_FAMILY`]). The base
//! spacing (0.38 mm) is the one value two families give (Wilcom and Melco).
//!
//! The default fabric, [`DEFAULT_FABRIC`], equals the planner's built-in values exactly, so that a
//! document set to it sews what a document without a fabric sews, bit for bit.

use crate::plan::{FILL_SPACING, FILL_STITCH, PULL, Underlay};

/// Who publishes a value, and the family of software or the maker it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Publication {
    /// What it is, in a few words.
    pub name: &'static str,
    /// Where it is read.
    pub url: &'static str,
    pub family: Family,
}

/// The families the publications belong to: values from one family confirm nothing of one another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Wilcom EmbroideryStudio's help, and BERNINA's software, which is Wilcom's.
    Wilcom,
    /// Melco's machine manual.
    Melco,
    /// Sulky's stabiliser chart.
    Sulky,
}

impl Family {
    /// Its id in replies.
    pub fn id(self) -> &'static str {
        match self {
            Self::Wilcom => "wilcom",
            Self::Melco => "melco",
            Self::Sulky => "sulky",
        }
    }
}

/// What a preset's value rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    /// Printed so by the publication.
    Published(&'static Publication),
    /// Printed so by the publication for the row the words name (one of BERNINA's fabric classes:
    /// which class a fabric is taken as is ours to say, since the classes name no fabrics).
    PublishedFor(&'static Publication, &'static str),
    /// Worked out from what the publication prints, as the words say.
    Derived(&'static Publication, &'static str),
    /// An estimate, for the reason the words give.
    Estimate(&'static str),
}

/// A preset's value, and what it rests on: the first basis is the one taken, the others agree or
/// differ as their words say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sourced<T: 'static> {
    pub value: T,
    pub basis: &'static [Basis],
}

/// What a fabric says for one kind of stitch. `None`: nothing published for it, the built-in
/// default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StitchPreset {
    /// Pull compensation, millimetres at each side.
    pub pull: Option<Sourced<f64>>,
    /// Tatami: millimetres between rows for 40 wt thread (another thread scales it, as it scales
    /// the built-in spacing).
    pub spacing: Option<Sourced<f64>>,
    /// Satin: the spacing the planner picks by a column's width, percent.
    pub spacing_percent: Option<Sourced<f64>>,
    /// Tatami: the longest stitch of a row, millimetres.
    pub length: Option<Sourced<f64>>,
    /// What is sewn under it first.
    pub underlay: Option<Sourced<Underlay>>,
}

/// A fabric preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fabric {
    /// Its id in commands and files.
    pub id: &'static str,
    /// Its name in English (the interface translates it).
    pub label: &'static str,
    /// For tatami fills, and strokes sewn as the area they paint.
    pub tatami: StitchPreset,
    /// For satin columns, and a fill's narrow parts sewn as satin.
    pub satin: StitchPreset,
    /// What holds the fabric from below: `tearAway`, `cutAway`, `adhesiveTearAway` (information).
    pub stabilizer: Option<Sourced<&'static str>>,
    /// What lies on top while it is sewn: `none`, `waterSoluble` (information).
    pub topping: Option<Sourced<&'static str>>,
    /// The needle's point: `universal`, `ballPoint` (information).
    pub needle_point: Option<Sourced<&'static str>>,
    /// How thick the fabric is with its stabiliser, millimetres (the thread estimate takes it).
    pub thickness: Option<Sourced<f64>>,
}

// ---------------------------------------------------------------- the publications

/// Wilcom's indicative pull compensation by fabric.
pub const WILCOM_PULL: Publication = Publication {
    name: "Wilcom EmbroideryStudio 28 help: Apply automatic pull compensation",
    url: "https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Quality/stabilizing/stabilizing-17.htm",
    family: Family::Wilcom,
};
/// Wilcom's tatami defaults.
pub const WILCOM_TATAMI: Publication = Publication {
    name: "Wilcom: Mastering basic stitch types, satin stitch and tatami stitch",
    url: "https://wilcom.com/resources/blog/mastering-basic-stitch-types-satin-stitch-and-tatami-stitch",
    family: Family::Wilcom,
};
/// Wilcom's eight auto fabrics: the fabrics these presets are.
pub const WILCOM_FABRICS: Publication = Publication {
    name: "Wilcom EmbroideryStudio 28 help: About auto fabrics",
    url: "https://docs.wilcom.com/embroiderystudio/28/en/OnlineHelp/Digitizing/properties/properties-9.htm",
    family: Family::Wilcom,
};
/// BERNINA's fabric presets, eight classes (F1 to F8) with their stitch values.
pub const BERNINA_PRESETS: Publication = Publication {
    name: "BERNINA Embroidery Software V9 help: Sticheinstellungen (fabric presets)",
    url: "https://softwarehelp.bernina.com/de/MainHelp/PART_Digitizing/properties/Sticheinstellungen.htm",
    family: Family::Wilcom,
};
/// BERNINA's V9 release notes, which carry Wilcom's copyright: BERNINA's software is Wilcom's.
pub const BERNINA_RELEASE_NOTES: Publication = Publication {
    name: "BERNINA Embroidery Software V9 release notes",
    url: "https://softwarehelp.bernina.com/en/downloads/ReleaseNotes.pdf",
    family: Family::Wilcom,
};
/// Melco's machine manual: a material's Acti-feed minimum (its thickness with its stabiliser, less
/// two points of 0.1 mm), and its standard fill density (pp. 182–183).
pub const MELCO_OS: Publication = Publication {
    name: "Melco OS manual: Acti-feed minimum by material",
    url: "https://www.melco-service.com/docs/X/melco-os-ai-en.pdf",
    family: Family::Melco,
};
/// Sulky's stabiliser chart: needle, backing and topping by fabric.
pub const SULKY_CHART: Publication = Publication {
    name: "Sulky: Stabilizing-at-a-Glance, computerized machine embroidery",
    url: "https://www.sulky.com/fullpanel/uploads/files/stabilizer-at-a-glance-2017.pdf",
    family: Family::Sulky,
};

/// Said of every preset (`embroidery.fabrics` gives it with them).
pub const ONE_FAMILY: &str = "Every per-fabric stitch value comes from one family of software, Wilcom's (BERNINA's embroidery software is Wilcom's): no independent source confirms them, and which of BERNINA's fabric classes a fabric is taken as is ours to say (the classes name no fabrics). Sew a test on the fabric.";

// ---------------------------------------------------------------- the facts

/// Wilcom's pull compensation for drills and cotton, millimetres ([`WILCOM_PULL`]).
const WILCOM_PULL_COTTON: f64 = 0.20;
/// Wilcom's pull compensation for T-shirts, millimetres ([`WILCOM_PULL`]).
const WILCOM_PULL_T_SHIRT: f64 = 0.35;
/// Wilcom's pull compensation for fleece and jumpers, millimetres ([`WILCOM_PULL`]).
const WILCOM_PULL_FLEECE: f64 = 0.40;

/// BERNINA's tatami (Steppstich) row spacing for its medium woven class (F7), millimetres
/// ([`BERNINA_PRESETS`]): the base the other classes' spacings are compared with.
const BERNINA_SPACING_MEDIUM_WOVEN: f64 = 0.4;
/// BERNINA's tatami row spacing for its heavy knit class (F3), millimetres.
const BERNINA_SPACING_HEAVY_KNIT: f64 = 0.35;
/// BERNINA's tatami row spacing for its light woven class (F6), millimetres.
const BERNINA_SPACING_LIGHT_WOVEN: f64 = 0.5;
/// BERNINA's tatami stitch length for its heavy knit class (F3), millimetres.
const BERNINA_LENGTH_HEAVY_KNIT: f64 = 3.8;
/// BERNINA's tatami stitch length for its light woven class (F6), millimetres.
const BERNINA_LENGTH_LIGHT_WOVEN: f64 = 4.2;
/// BERNINA's tatami pull compensation for its heavy woven class (F8), millimetres.
const BERNINA_TATAMI_PULL_HEAVY_WOVEN: f64 = 0.17;
/// BERNINA's tatami pull compensation for its light and medium knit classes (F1, F2), millimetres.
const BERNINA_TATAMI_PULL_KNIT: f64 = 0.2;
/// BERNINA's tatami pull compensation for its heavy knit class (F3), millimetres.
const BERNINA_TATAMI_PULL_HEAVY_KNIT: f64 = 0.25;
/// BERNINA's satin pull compensation for its medium knit class (F2), millimetres.
const BERNINA_SATIN_PULL_MEDIUM_KNIT: f64 = 0.25;
/// BERNINA's satin pull compensation for narrow columns of its light woven class (F6),
/// millimetres (0.25 for wide ones).
const BERNINA_SATIN_PULL_LIGHT_WOVEN: f64 = 0.2;
/// BERNINA's satin auto spacing for wide columns of its medium woven class (F7), percent: the base.
const BERNINA_SATIN_SPACING_MEDIUM_WOVEN: f64 = 90.0;
/// BERNINA's satin auto spacing for wide columns of its light woven class (F6), percent.
const BERNINA_SATIN_SPACING_LIGHT_WOVEN: f64 = 100.0;

/// Points of 0.1 mm Melco's Acti-feed minimum takes off a material's thickness ([`MELCO_OS`]).
const MELCO_MINIMUM_UNDER: f64 = 2.0;
/// Millimetres in one of Melco's points.
const MELCO_POINT_MM: f64 = 0.1;
/// Melco's Acti-feed minimum for broadcloth with cut-away, points: 3 to 4.
const MELCO_BROADCLOTH: (f64, f64) = (3.0, 4.0);
/// For a T-shirt with two cut-aways, points: 3 to 5.
const MELCO_T_SHIRT: (f64, f64) = (3.0, 5.0);
/// For a polo shirt with cut-away, points: 4 to 8.
const MELCO_POLO: (f64, f64) = (4.0, 8.0);
/// For a structured cap, points: 8 to 12.
const MELCO_STRUCTURED_CAP: (f64, f64) = (8.0, 12.0);

/// The thickness of a material Melco gives the Acti-feed minimum of as `range` points: its middle,
/// plus the two points the minimum takes off, in millimetres.
const fn melco_thickness(range: (f64, f64)) -> f64 {
    ((range.0 + range.1) / 2.0 + MELCO_MINIMUM_UNDER) * MELCO_POINT_MM
}

/// The percent of the planner's own satin spacing (by width) that leaves it as it is.
const AS_PICKED: f64 = 100.0;

/// The thickness of fleece with its stabiliser, millimetres: no published value; between Melco's
/// polo (up to 1.0 mm) and its towel (from 0.8 mm). An estimate.
const FLEECE_THICKNESS: f64 = 1.0;

// ---------------------------------------------------------------- what each value rests on

const COTTON_PULL: &[Basis] = &[Basis::Published(&WILCOM_PULL), Basis::Derived(&BERNINA_PRESETS, "the medium woven class (F7) gives the same 0.2")];
const BASE_SPACING: &[Basis] = &[Basis::Published(&WILCOM_TATAMI), Basis::Derived(&MELCO_OS, "a standard fill density of 3.8 points, 0.38 mm")];
const BASE_LENGTH: &[Basis] = &[Basis::Published(&WILCOM_TATAMI), Basis::Derived(&BERNINA_PRESETS, "the medium woven class (F7) gives the same 4.0")];
const AUTO_UNDERLAY: &[Basis] =
    &[Basis::Estimate("the planner's own rule: under a fill an edge run and, from 10 mm across, sparse tatami; under a satin by its width")];
/// Cotton's tatami underlay: the default must sew as a document without a fabric does, though the
/// class it is taken as sews more.
const COTTON_TATAMI_UNDERLAY: &[Basis] = &[Basis::Estimate(
    "the planner's own rule (an edge run and, from 10 mm across, sparse tatami), kept so that the default sews as a document without a fabric; BERNINA's medium woven class (F7) sews an edge run and a tatami layer under every fill",
)];
const AS_PICKED_SATIN: &[Basis] =
    &[Basis::Estimate("the planner's own spacing by a column's width; the classes' percentages are compared with the medium woven class's 90 %")];
/// What a fabric taken as one of BERNINA's classes rests on for its tatami rows' spacing and stitch
/// length and its tatami underlay, each naming the class.
struct ClassBases {
    spacing: &'static [Basis],
    length: &'static [Basis],
    tatami_layer: &'static [Basis],
}

/// BERNINA's heavy woven class (F8): denim.
const HEAVY_WOVEN: ClassBases = ClassBases {
    spacing: &[Basis::Derived(&BERNINA_PRESETS, "the heavy woven class (F8): 0.4 mm rows, as the medium woven class's: the same share of 0.38 mm")],
    length: &[Basis::PublishedFor(&BERNINA_PRESETS, "the heavy woven class (F8): 4.0 mm")],
    tatami_layer: &[Basis::Estimate(
        "BERNINA's heavy woven class (F8) sews a tatami layer under every fill; the planner's nearest kind is an edge run and a tatami layer",
    )],
};
/// BERNINA's light knit class (F1): stretch knit.
const LIGHT_KNIT: ClassBases = ClassBases {
    spacing: &[Basis::Derived(&BERNINA_PRESETS, "the light knit class (F1): 0.4 mm rows, as the medium woven class's: the same share of 0.38 mm")],
    length: &[Basis::PublishedFor(&BERNINA_PRESETS, "the light knit class (F1): 4.0 mm")],
    tatami_layer: &[Basis::Estimate(
        "BERNINA's light knit class (F1) sews a tatami layer under every fill; the planner's nearest kind is an edge run and a tatami layer",
    )],
};
/// Stretch knit's satin spacing: the light knit class's.
const LIGHT_KNIT_SATIN_SHARE: &[Basis] =
    &[Basis::Derived(&BERNINA_PRESETS, "the light knit class's (F1) satin spacing is the medium woven class's (90 % for narrow and wide columns)")];
/// BERNINA's medium knit class (F2): piqué.
const MEDIUM_KNIT: ClassBases = ClassBases {
    spacing: &[Basis::Derived(&BERNINA_PRESETS, "the medium knit class (F2): 0.4 mm rows, as the medium woven class's: the same share of 0.38 mm")],
    length: &[Basis::PublishedFor(&BERNINA_PRESETS, "the medium knit class (F2): 4.0 mm")],
    tatami_layer: &[Basis::Estimate(
        "BERNINA's medium knit class (F2) sews a tatami layer under every fill; the planner's nearest kind is an edge run and a tatami layer",
    )],
};
/// Piqué's satin spacing: the medium knit class's.
const MEDIUM_KNIT_SATIN_SHARE: &[Basis] =
    &[Basis::Derived(&BERNINA_PRESETS, "the medium knit class's (F2) satin spacing is the medium woven class's (90 % for narrow and wide columns)")];
/// BERNINA's heavy knit class (F3): fleece.
const HEAVY_KNIT: ClassBases = ClassBases {
    spacing: &[Basis::Derived(&BERNINA_PRESETS, "the heavy knit class's (F3) 0.35 mm rows against the medium woven class's 0.4: 87.5 % of 0.38 mm")],
    length: &[Basis::PublishedFor(&BERNINA_PRESETS, "the heavy knit class (F3): 3.8 mm")],
    tatami_layer: &[Basis::Estimate(
        "BERNINA's heavy knit class (F3) sews a tatami layer under every fill; the planner's nearest kind is an edge run and a tatami layer",
    )],
};
/// Why silk is sewn as BERNINA's light woven class: no source says so (the project owner's choice,
/// 2026-10-24: each silk stitch value says it first).
const SILK_AS_LIGHT_WOVEN: &str = "silk taken as BERNINA's light woven class (F6); no source maps silk to a class";
const SULKY: &[Basis] = &[Basis::Published(&SULKY_CHART)];

// ---------------------------------------------------------------- the presets

/// The fabric a document without one is sewn as: [`FABRICS`]' first.
pub const DEFAULT_FABRIC: &str = "cotton";

/// The presets, in the order Wilcom lists its auto fabrics ([`WILCOM_FABRICS`]).
pub const FABRICS: [Fabric; 8] = [
    // Wilcom's "cotton (light woven)": Wilcom's guidance for cotton and BERNINA's medium woven class
    // give the planner's built-in values, its underlay aside (the class sews an edge run and a
    // tatami layer under every fill; the default keeps the planner's rule, as a document without a
    // fabric sews). BERNINA's light woven class (F6) is set looser (0.5 mm
    // rows of 4.2 mm, 0.4 mm of tatami pull): the silk preset takes it, as an estimate (no source
    // maps a fabric to a class; the default must keep the built-in values).
    Fabric {
        id: "cotton",
        label: "Cotton (light woven)",
        tatami: StitchPreset {
            pull: Some(Sourced { value: PULL, basis: COTTON_PULL }),
            spacing: Some(Sourced { value: FILL_SPACING, basis: BASE_SPACING }),
            spacing_percent: None,
            length: Some(Sourced { value: FILL_STITCH, basis: BASE_LENGTH }),
            underlay: Some(Sourced { value: Underlay::Auto, basis: COTTON_TATAMI_UNDERLAY }),
        },
        satin: StitchPreset {
            pull: Some(Sourced { value: PULL, basis: COTTON_PULL }),
            spacing: None,
            spacing_percent: Some(Sourced { value: AS_PICKED, basis: AS_PICKED_SATIN }),
            length: None,
            underlay: Some(Sourced { value: Underlay::Auto, basis: AUTO_UNDERLAY }),
        },
        stabilizer: Some(Sourced { value: "tearAway", basis: SULKY }),
        topping: Some(Sourced { value: "none", basis: SULKY }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "universal or sharp")] }),
        thickness: Some(Sourced {
            value: melco_thickness(MELCO_BROADCLOTH),
            basis: &[Basis::Derived(&MELCO_OS, "broadcloth with cut-away, 3 to 4 points and the 2 the minimum takes off: 0.5 to 0.6 mm")],
        }),
    },
    // BERNINA's heavy woven class (F8).
    Fabric {
        id: "denim",
        label: "Denim or heavy twill",
        tatami: StitchPreset {
            pull: Some(Sourced {
                value: BERNINA_TATAMI_PULL_HEAVY_WOVEN,
                basis: &[Basis::PublishedFor(&BERNINA_PRESETS, "the heavy woven class (F8)")],
            }),
            spacing: Some(Sourced { value: FILL_SPACING, basis: HEAVY_WOVEN.spacing }),
            spacing_percent: None,
            length: Some(Sourced { value: FILL_STITCH, basis: HEAVY_WOVEN.length }),
            underlay: Some(Sourced { value: Underlay::EdgeTatami, basis: HEAVY_WOVEN.tatami_layer }),
        },
        satin: StitchPreset {
            pull: Some(Sourced {
                value: WILCOM_PULL_COTTON,
                basis: &[Basis::Published(&WILCOM_PULL), Basis::Derived(&BERNINA_PRESETS, "the heavy woven class (F8) gives the same 0.2")],
            }),
            spacing: None,
            spacing_percent: Some(Sourced {
                value: AS_PICKED,
                basis: &[Basis::Estimate(
                    "BERNINA's heavy woven class spaces wide columns 2 % and narrow ones 7 % more than its medium woven class; one share for every width, within the width table's own precision",
                )],
            }),
            length: None,
            underlay: Some(Sourced { value: Underlay::Auto, basis: AUTO_UNDERLAY }),
        },
        stabilizer: Some(Sourced { value: "tearAway", basis: SULKY }),
        topping: Some(Sourced { value: "none", basis: SULKY }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "a 100/16 denim needle, universal or sharp")] }),
        thickness: None,
    },
    // BERNINA's light knit class (F1); Wilcom's T-shirt pull for satins.
    Fabric {
        id: "stretchKnit",
        label: "Stretch knit",
        tatami: StitchPreset {
            pull: Some(Sourced { value: BERNINA_TATAMI_PULL_KNIT, basis: &[Basis::PublishedFor(&BERNINA_PRESETS, "the light knit class (F1)")] }),
            spacing: Some(Sourced { value: FILL_SPACING, basis: LIGHT_KNIT.spacing }),
            spacing_percent: None,
            length: Some(Sourced { value: FILL_STITCH, basis: LIGHT_KNIT.length }),
            underlay: Some(Sourced { value: Underlay::EdgeTatami, basis: LIGHT_KNIT.tatami_layer }),
        },
        satin: StitchPreset {
            pull: Some(Sourced {
                value: WILCOM_PULL_T_SHIRT,
                basis: &[
                    Basis::Published(&WILCOM_PULL),
                    Basis::Derived(
                        &BERNINA_PRESETS,
                        "the light knit class (F1) gives 0.25; the larger is taken: a gap shows more than a slightly wider column",
                    ),
                ],
            }),
            spacing: None,
            spacing_percent: Some(Sourced { value: AS_PICKED, basis: LIGHT_KNIT_SATIN_SHARE }),
            length: None,
            underlay: Some(Sourced { value: Underlay::Auto, basis: AUTO_UNDERLAY }),
        },
        stabilizer: Some(Sourced { value: "cutAway", basis: SULKY }),
        topping: Some(Sourced { value: "waterSoluble", basis: SULKY }),
        needle_point: Some(Sourced { value: "ballPoint", basis: &[Basis::Derived(&SULKY_CHART, "universal or ball point")] }),
        thickness: Some(Sourced {
            value: melco_thickness(MELCO_T_SHIRT),
            basis: &[Basis::Derived(&MELCO_OS, "a T-shirt with two cut-aways, 3 to 5 points and the 2 the minimum takes off: 0.5 to 0.7 mm")],
        }),
    },
    // BERNINA's medium knit class (F2).
    Fabric {
        id: "pique",
        label: "Piqué (polo knit)",
        tatami: StitchPreset {
            pull: Some(Sourced { value: BERNINA_TATAMI_PULL_KNIT, basis: &[Basis::PublishedFor(&BERNINA_PRESETS, "the medium knit class (F2)")] }),
            spacing: Some(Sourced { value: FILL_SPACING, basis: MEDIUM_KNIT.spacing }),
            spacing_percent: None,
            length: Some(Sourced { value: FILL_STITCH, basis: MEDIUM_KNIT.length }),
            underlay: Some(Sourced { value: Underlay::EdgeTatami, basis: MEDIUM_KNIT.tatami_layer }),
        },
        satin: StitchPreset {
            pull: Some(Sourced {
                value: BERNINA_SATIN_PULL_MEDIUM_KNIT,
                basis: &[
                    Basis::PublishedFor(&BERNINA_PRESETS, "the medium knit class (F2)"),
                    Basis::Estimate("Wilcom's help gives no value for polo knits"),
                ],
            }),
            spacing: None,
            spacing_percent: Some(Sourced { value: AS_PICKED, basis: MEDIUM_KNIT_SATIN_SHARE }),
            length: None,
            underlay: Some(Sourced { value: Underlay::Auto, basis: AUTO_UNDERLAY }),
        },
        stabilizer: Some(Sourced { value: "cutAway", basis: SULKY }),
        topping: Some(Sourced { value: "waterSoluble", basis: SULKY }),
        needle_point: Some(Sourced { value: "ballPoint", basis: SULKY }),
        thickness: Some(Sourced {
            value: melco_thickness(MELCO_POLO),
            basis: &[Basis::Derived(&MELCO_OS, "a polo shirt with cut-away, 4 to 8 points and the 2 the minimum takes off: 0.6 to 1.0 mm")],
        }),
    },
    // BERNINA's heavy knit class (F3); Wilcom's fleece pull for satins.
    Fabric {
        id: "fleece",
        label: "Fleece",
        tatami: StitchPreset {
            pull: Some(Sourced {
                value: BERNINA_TATAMI_PULL_HEAVY_KNIT,
                basis: &[Basis::PublishedFor(&BERNINA_PRESETS, "the heavy knit class (F3)")],
            }),
            spacing: Some(Sourced { value: FILL_SPACING * (BERNINA_SPACING_HEAVY_KNIT / BERNINA_SPACING_MEDIUM_WOVEN), basis: HEAVY_KNIT.spacing }),
            spacing_percent: None,
            length: Some(Sourced { value: BERNINA_LENGTH_HEAVY_KNIT, basis: HEAVY_KNIT.length }),
            underlay: Some(Sourced { value: Underlay::EdgeTatami, basis: HEAVY_KNIT.tatami_layer }),
        },
        satin: StitchPreset {
            pull: Some(Sourced {
                value: WILCOM_PULL_FLEECE,
                basis: &[
                    Basis::Published(&WILCOM_PULL),
                    Basis::Derived(
                        &BERNINA_PRESETS,
                        "the heavy knit class (F3) gives 0.35; the larger is taken: a gap shows more than a slightly wider column",
                    ),
                ],
            }),
            spacing: None,
            spacing_percent: Some(Sourced {
                value: AS_PICKED,
                basis: &[Basis::Estimate(
                    "BERNINA's heavy knit class spaces narrow columns as its medium woven class and wide ones 3 % closer; one share for every width",
                )],
            }),
            length: None,
            underlay: Some(Sourced { value: Underlay::Auto, basis: AUTO_UNDERLAY }),
        },
        stabilizer: Some(Sourced { value: "adhesiveTearAway", basis: SULKY }),
        topping: Some(Sourced { value: "waterSoluble", basis: SULKY }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "universal or sharp")] }),
        thickness: Some(Sourced {
            value: FLEECE_THICKNESS,
            basis: &[Basis::Estimate("no published value: between Melco's polo, up to 1.0 mm, and its towel, from 0.8 mm")],
        }),
    },
    // Wilcom's "satin / silk", taken as BERNINA's light woven class (F6), set looser: an estimate,
    // since no source maps silk to a class ([`SILK_AS_LIGHT_WOVEN`] first in each stitch value).
    Fabric {
        id: "silk",
        label: "Silk or satin fabric",
        tatami: StitchPreset {
            pull: Some(Sourced {
                value: PULL,
                basis: &[
                    Basis::Estimate(SILK_AS_LIGHT_WOVEN),
                    Basis::Estimate(
                        "BERNINA's light woven class gives 0.4, twice its medium woven class's and against the rule its other classes keep (stabler fabrics, less pull); the medium woven class's 0.2 is taken",
                    ),
                ],
            }),
            spacing: Some(Sourced {
                value: FILL_SPACING * (BERNINA_SPACING_LIGHT_WOVEN / BERNINA_SPACING_MEDIUM_WOVEN),
                basis: &[
                    Basis::Estimate(SILK_AS_LIGHT_WOVEN),
                    Basis::Derived(
                        &BERNINA_PRESETS,
                        "the light woven class's (F6) 0.5 mm rows against the medium woven class's 0.4: 125 % of 0.38 mm",
                    ),
                ],
            }),
            spacing_percent: None,
            length: Some(Sourced {
                value: BERNINA_LENGTH_LIGHT_WOVEN,
                basis: &[Basis::Estimate(SILK_AS_LIGHT_WOVEN), Basis::PublishedFor(&BERNINA_PRESETS, "the light woven class (F6): 4.2 mm")],
            }),
            underlay: Some(Sourced {
                value: Underlay::Auto,
                basis: &[
                    Basis::Estimate(SILK_AS_LIGHT_WOVEN),
                    Basis::Estimate(
                        "BERNINA's light woven class sews a sparse tatami layer (rows 5 mm apart) under every fill; the planner's tatami layer is denser, so its own rule is kept",
                    ),
                ],
            }),
        },
        satin: StitchPreset {
            pull: Some(Sourced {
                value: BERNINA_SATIN_PULL_LIGHT_WOVEN,
                basis: &[
                    Basis::Estimate(SILK_AS_LIGHT_WOVEN),
                    Basis::Derived(
                        &BERNINA_PRESETS,
                        "the light woven class's (F6) narrow columns' 0.2 (wide ones 0.25): of its two values the one nearer its medium woven class's (0.2), so that one value for every width moves no further from the default than the class asks",
                    ),
                ],
            }),
            spacing: None,
            spacing_percent: Some(Sourced {
                value: AS_PICKED * (BERNINA_SATIN_SPACING_LIGHT_WOVEN / BERNINA_SATIN_SPACING_MEDIUM_WOVEN),
                basis: &[
                    Basis::Estimate(SILK_AS_LIGHT_WOVEN),
                    Basis::Derived(
                        &BERNINA_PRESETS,
                        "the light woven class's (F6) wide columns at 100 % (narrow ones 108 %) against its medium woven class's 90 %: 111 %; of its two values the one nearer the medium woven class's, as for the pull",
                    ),
                ],
            }),
            length: None,
            underlay: Some(Sourced {
                value: Underlay::Auto,
                basis: &[Basis::Estimate(SILK_AS_LIGHT_WOVEN), Basis::Estimate("the planner's own rule: under a satin by its width")],
            }),
        },
        stabilizer: Some(Sourced { value: "adhesiveTearAway", basis: SULKY }),
        topping: Some(Sourced { value: "none", basis: SULKY }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "universal or ball point")] }),
        thickness: None,
    },
    // Wilcom's "nylon / sports fabric": no published stitch values.
    Fabric {
        id: "nylon",
        label: "Nylon or sports fabric",
        tatami: StitchPreset { pull: None, spacing: None, spacing_percent: None, length: None, underlay: None },
        satin: StitchPreset { pull: None, spacing: None, spacing_percent: None, length: None, underlay: None },
        stabilizer: Some(Sourced { value: "tearAway", basis: &[Basis::Derived(&SULKY_CHART, "an unlined nylon windbreaker")] }),
        topping: Some(Sourced { value: "none", basis: SULKY }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "universal or sharp, 75/11 to 80/12")] }),
        thickness: None,
    },
    // Wilcom's "cap backing / structured cap": no published stitch values.
    Fabric {
        id: "cap",
        label: "Cap (structured)",
        tatami: StitchPreset { pull: None, spacing: None, spacing_percent: None, length: None, underlay: None },
        satin: StitchPreset { pull: None, spacing: None, spacing_percent: None, length: None, underlay: None },
        stabilizer: Some(Sourced { value: "adhesiveTearAway", basis: &[Basis::Derived(&SULKY_CHART, "hats")] }),
        topping: Some(Sourced { value: "waterSoluble", basis: &[Basis::Derived(&SULKY_CHART, "hats")] }),
        needle_point: Some(Sourced { value: "universal", basis: &[Basis::Derived(&SULKY_CHART, "hats: universal or sharp, 90/14")] }),
        thickness: Some(Sourced {
            value: melco_thickness(MELCO_STRUCTURED_CAP),
            basis: &[Basis::Derived(&MELCO_OS, "a structured cap, 8 to 12 points and the 2 the minimum takes off: 1.0 to 1.4 mm")],
        }),
    },
];

/// The preset of id `id`.
pub fn fabric(id: &str) -> Option<&'static Fabric> {
    FABRICS.iter().find(|f| f.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{
        DENSITY_RANGE, PULL_RANGE, Params, SPACING_PERCENT_RANGE, STITCH_LENGTH_RANGE, Style, THREAD_WEIGHT_RANGE, satin_spacing, thread_scale,
    };

    /// Every value of a preset, with what it rests on.
    fn values(f: &Fabric) -> Vec<(&'static str, Option<f64>, &'static [Basis])> {
        let n = |name, v: Option<Sourced<f64>>| (name, v.map(|s| s.value), v.map_or(&[][..], |s| s.basis));
        let t = |name, v: Option<Sourced<&'static str>>| (name, None, v.map_or(&[][..], |s| s.basis));
        let u = |name, v: Option<Sourced<Underlay>>| (name, None, v.map_or(&[][..], |s| s.basis));
        vec![
            n("tatami.pull", f.tatami.pull),
            n("tatami.spacing", f.tatami.spacing),
            n("tatami.spacingPercent", f.tatami.spacing_percent),
            n("tatami.length", f.tatami.length),
            u("tatami.underlay", f.tatami.underlay),
            n("satin.pull", f.satin.pull),
            n("satin.spacing", f.satin.spacing),
            n("satin.spacingPercent", f.satin.spacing_percent),
            n("satin.length", f.satin.length),
            u("satin.underlay", f.satin.underlay),
            t("stabilizer", f.stabilizer),
            t("topping", f.topping),
            t("needlePoint", f.needle_point),
            n("thickness", f.thickness),
        ]
    }

    /// The default fabric is the planner's built-in values exactly: a document set to it sews what
    /// a document without a fabric sews.
    #[test]
    fn the_default_fabric_is_the_planners_defaults() {
        let f = fabric(DEFAULT_FABRIC).unwrap();
        assert_eq!(FABRICS[0].id, DEFAULT_FABRIC);
        let (fill, satin) = (Params::for_style(Style::Fill), Params::for_style(Style::Satin));
        let bits = |v: Option<Sourced<f64>>| v.map(|s| s.value.to_bits());
        assert_eq!(bits(f.tatami.pull), Some(fill.pull[0].to_bits()));
        assert_eq!(bits(f.tatami.spacing), Some(fill.density.to_bits()));
        assert_eq!(bits(f.tatami.length), Some(fill.stitch_length.to_bits()));
        assert_eq!(f.tatami.underlay.map(|u| u.value), Some(fill.underlay));
        assert_eq!(bits(f.satin.pull), Some(satin.pull[0].to_bits()));
        assert_eq!(f.satin.spacing_percent.map(|s| s.value), Some(100.0));
        assert_eq!(f.satin.underlay.map(|u| u.value), Some(satin.underlay));
        assert_eq!((f.tatami.spacing_percent, f.satin.spacing, f.satin.length), (None, None, None));
    }

    /// Ids are unique; every value lies within the planner's ranges, and so does its spacing for
    /// every thread the planner takes; every value has a basis, every estimate its reason, every
    /// publication its address.
    #[test]
    fn every_value_is_in_range_and_sourced() {
        let mut ids: Vec<&str> = FABRICS.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), FABRICS.len());
        let within = |(lo, hi): (f64, f64), v: f64| (lo..=hi).contains(&v);
        for f in &FABRICS {
            assert!(!f.label.is_empty(), "{}", f.id);
            for (name, value, basis) in values(f) {
                let set = !basis.is_empty();
                assert!(set || value.is_none(), "{} {name}: a value without a basis", f.id);
                for b in basis {
                    match b {
                        Basis::Published(p) => {
                            assert!(p.url.starts_with("https://"), "{} {name}", f.id);
                            // BERNINA's values are its classes': each says which class it is.
                            assert_ne!(*p, &BERNINA_PRESETS, "{} {name}: which of BERNINA's classes?", f.id);
                        }
                        Basis::PublishedFor(p, how) | Basis::Derived(p, how) => {
                            assert!(p.url.starts_with("https://") && !how.is_empty(), "{} {name}", f.id);
                            assert!(*p != &BERNINA_PRESETS || how.contains("(F"), "{} {name}: which of BERNINA's classes? {how}", f.id);
                        }
                        Basis::Estimate(why) => assert!(!why.is_empty(), "{} {name}", f.id),
                    }
                }
                let Some(v) = value else { continue };
                match name {
                    "tatami.pull" | "satin.pull" => assert!(within(PULL_RANGE, v), "{} {name} {v}", f.id),
                    "tatami.length" => assert!(within(STITCH_LENGTH_RANGE, v), "{} {name} {v}", f.id),
                    "tatami.spacingPercent" | "satin.spacingPercent" => assert!(within(SPACING_PERCENT_RANGE, v), "{} {name} {v}", f.id),
                    "thickness" => assert!(v > 0.0 && v < 5.0, "{} {name} {v}", f.id),
                    _ => {}
                }
            }
            for weight in [THREAD_WEIGHT_RANGE.0, 40.0, THREAD_WEIGHT_RANGE.1] {
                if let Some(s) = f.tatami.spacing {
                    assert!(within(DENSITY_RANGE, s.value * thread_scale(weight)), "{} tatami spacing at {weight} wt", f.id);
                }
                if let Some(p) = f.satin.spacing_percent {
                    for width in [1.0, 3.0, 5.0, 9.0] {
                        let spacing = satin_spacing(width) * thread_scale(weight) * p.value / 100.0;
                        assert!(within(DENSITY_RANGE, spacing), "{} satin spacing {width} mm at {weight} wt", f.id);
                    }
                }
            }
        }
        assert_eq!(fabric("silk").and_then(|f| f.tatami.spacing).map(|s| (s.value * 1e4).round() / 1e4), Some(0.475));
        assert_eq!(fabric("fleece").and_then(|f| f.tatami.spacing).map(|s| (s.value * 1e4).round() / 1e4), Some(0.3325));
        assert_eq!(fabric("pique").and_then(|f| f.thickness).map(|s| (s.value * 100.0).round() / 100.0), Some(0.8));
        assert!(fabric("felt").is_none());
    }

    /// Silk's stitch values are an estimate first: no source maps silk to the class they come from.
    #[test]
    fn silks_stitch_values_are_estimates() {
        let silk = fabric("silk").unwrap();
        let mut bases: Vec<&[Basis]> = Vec::new();
        for p in [&silk.tatami, &silk.satin] {
            bases.extend([p.pull, p.spacing, p.spacing_percent, p.length].into_iter().flatten().map(|v| v.basis));
            bases.extend(p.underlay.map(|u| u.basis));
        }
        assert_eq!(bases.len(), 7);
        assert!(bases.iter().all(|b| b.first() == Some(&Basis::Estimate(SILK_AS_LIGHT_WOVEN))), "{bases:?}");
    }
}
