//! The numbers of the embroidery fields (the Embroidery panel's Object tab, Document Setup's
//! Embroidery tab): they write the interface language's decimal mark and read both marks (the
//! project owner's choice, 2026-10-23): a comma language writes "0,38 mm" and reads "0,38" and
//! "0.38"; a point language writes "0.38 mm" and reads the point alone, since "1,000" would otherwise
//! read as one. Every other field of the app keeps its point.
//!
//! Each unit steps in a size of its own (↑/↓, the wheel, a drag on the field's label; Shift ten
//! steps, Ctrl/Cmd a tenth): a tenth of a millimetre, a degree, a percent, a thread weight, ten
//! stitches a minute. Units are the engine's field table's (`setting_field`'s `unit`).

use crate::embroidery::numbers;
use crate::i18n::Lang;
use crate::widgets::NumberFormat;

/// A millimetre field's step: a tenth of a millimetre (a hundredth with Ctrl/Cmd, one with Shift).
/// Embroidery lengths run from a twentieth of a millimetre to a dozen: a step of one would leap
/// across most of them. An estimate.
pub const MM_STEP: f64 = 0.1;
/// A speed field's step, stitches a minute: ten (a hundred with Shift). Machines are set in tens. An
/// estimate.
pub const SPEED_STEP: f64 = 10.0;
/// The step of a field in degrees, percent or thread weight: one.
pub const UNIT_STEP: f64 = 1.0;

/// What follows a number in each unit, as the fields show it: a space before a unit's name, none
/// before a degree sign, and a percent as the language spaces it ([`numbers::PERCENT_SPACES`]).
const MM: &str = " mm";
const DEGREES: &str = "°";
const WEIGHT: &str = " wt";
const SPEED: &str = " /min";
const PERCENT: &str = "%";
const PERCENT_SPACED: &str = "\u{a0}%";
const PERCENT_NARROW: &str = "\u{202f}%";

/// The format of an embroidery field in `unit` to `decimals` places, as `lang` writes numbers.
pub(crate) fn format(lang: Lang, unit: &str, decimals: usize) -> NumberFormat<'static> {
    let (suffix, step) = match unit {
        "mm" => (MM, MM_STEP),
        "°" => (DEGREES, UNIT_STEP),
        "wt" => (WEIGHT, UNIT_STEP),
        "/min" => (SPEED, SPEED_STEP),
        "%" => (percent(lang), UNIT_STEP),
        _ => ("", UNIT_STEP),
    };
    NumberFormat { suffix, decimals, mark: numbers::symbols(lang).decimal, step }
}

/// A percent sign after a number, spaced as `lang` spaces it.
fn percent(lang: Lang) -> &'static str {
    match numbers::PERCENT_SPACES.iter().find(|(code, _)| *code == lang.code()).map(|(_, c)| *c) {
        Some('\u{a0}') => PERCENT_SPACED,
        Some(_) => PERCENT_NARROW,
        None => PERCENT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> Lang {
        Lang::from_code(code).unwrap()
    }

    /// A comma language writes its comma and reads both marks; a point language writes and reads
    /// the point alone, so "1,000" isn't read as one.
    #[test]
    fn fields_write_the_languages_mark_and_read_both() {
        let pl = format(lang("pl"), "mm", 2);
        assert_eq!(pl.show(0.38), "0,38 mm");
        assert_eq!((pl.read("0,35"), pl.read("0.35"), pl.read("0,35 mm"), pl.read("1,5*2")), (Some(0.35), Some(0.35), Some(0.35), Some(3.0)));
        let en = format(Lang::EN, "mm", 2);
        assert_eq!(en.show(0.38), "0.38 mm");
        assert_eq!((en.read("0.35"), en.read("1,000")), (Some(0.35), None));
        assert_eq!(format(lang("de"), "%", 0).show(100.0), "100\u{a0}%");
        assert_eq!(format(lang("fr"), "%", 0).show(100.0), "100\u{202f}%");
        assert_eq!(format(lang("de"), "%", 0).read("120 %"), Some(120.0));
        assert_eq!(format(Lang::EN, "°", 1).show(135.0), "135°");
        assert_eq!(format(Lang::EN, "/min", 0).show(700.0), "700 /min");
    }

    /// What a field shows reads back as the value, in every language and unit.
    #[test]
    fn what_a_field_shows_reads_back() {
        for info in &crate::i18n::LANGUAGES {
            let Some(lang) = Lang::from_code(info.code) else { continue };
            for (unit, value) in [("mm", 0.38), ("°", 135.5), ("%", 120.0), ("wt", 40.0), ("/min", 650.0), ("mm", -1.25)] {
                let f = format(lang, unit, 2);
                assert_eq!(f.read(&f.show(value)), Some(value), "{} {unit}: {:?}", info.code, f.show(value));
            }
        }
    }

    /// Millimetres step a tenth, speeds ten, the rest one.
    #[test]
    fn each_unit_steps_in_its_own_size() {
        assert_eq!(format(Lang::EN, "mm", 2).step, MM_STEP);
        assert_eq!(format(Lang::EN, "/min", 0).step, SPEED_STEP);
        for unit in ["°", "%", "wt"] {
            assert_eq!(format(Lang::EN, unit, 0).step, UNIT_STEP, "{unit}");
        }
    }
}
