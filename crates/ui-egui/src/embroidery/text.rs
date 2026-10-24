//! The embroidery interface's words: every string the Embroidery panel, the stitch preview's notes
//! and marks and the status bar's counters show goes through [`crate::et!`] (a string) or
//! [`crate::etn!`] (a count with its noun), never through `tl!`.
//!
//! **One catalogue context.** Their rows sit under the catalogues' `embroidery` context
//! ([`CONTEXT`]), looked up strictly: never another sense's plain row. Upstream's plain rows give
//! short English words other meanings: "Art" is the Art brush ("Artystyczny" in Polish), "Exit" is
//! quitting ("Zakończ"), "Threads" are threaded text frames ("Wątki"). With a strict lookup the
//! Art/Stitches switch can't borrow them, and a missing row shows English, which the completeness
//! test (`i18n::tests_embroidery`) turns into a failure for every complete catalogue.
//!
//! **Templates, not pieces.** A sentence is one row with named placeholders, filled by [`fill`] in
//! one pass (a name holding `{count}` stays as it is). A counted noun is a whole plural row whose
//! count is `{count}`, written as the language writes numbers ([`super::numbers::count`]): Polish
//! "4752 ściegi", "12 345 ściegów". Units go inside the templates ("{value} mm"). Engine prose (a
//! rule's title, a unit's `why`) is never shown: the interface words its ids.
//!
//! Command labels, the panel's name and undo steps keep their plain rows, which upstream's tests
//! require.

use crate::i18n::{self, Lang};

/// The catalogue context of every embroidery interface string.
pub const CONTEXT: &str = "embroidery";

/// The placeholder a plural row's count goes in.
pub const COUNT: &str = "count";

/// An embroidery interface string in the language the interface is drawn in: its [`CONTEXT`] row,
/// strictly, else the English (`crate::et!("Stitches")`).
#[macro_export]
macro_rules! et {
    ($s:literal) => {
        $crate::i18n::t_name($crate::embroidery::text::CONTEXT, $s)
    };
}

/// A count with its noun in the language the interface is drawn in: `crate::etn!(n, "{count}
/// stitch", "{count} stitches")`, the language's plural form of the `@plural` row, its `{count}`
/// written as the language writes numbers.
#[macro_export]
macro_rules! etn {
    ($n:expr, $one:literal, $other:literal $(,)?) => {
        $crate::embroidery::text::plural($n, $one, $other)
    };
}

/// [`crate::etn!`]: `n` with its noun in the current language.
pub fn plural(n: u64, one: &str, other: &str) -> String {
    plural_in(i18n::current(), n, one, other)
}

/// [`plural`] in `lang`.
pub fn plural_in(lang: Lang, n: u64, one: &str, other: &str) -> String {
    // The form for `n`; `trn` fills only `{n}`, so `{count}` is still there.
    let form = i18n::trn(lang, n, one, other);
    fill(&form, &[(COUNT, &super::numbers::count(lang, n))])
}

/// `template` with each `{name}` placeholder `args` names replaced by its value, in one pass: a
/// value is never read again for placeholders (an object named "{count}" is shown as it is).
/// Placeholders without a value are left as written.
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let (before, from) = rest.split_at(open);
        out.push_str(before);
        let value = from.find('}').and_then(|close| {
            let name = from.get(1..close)?;
            args.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v, close))
        });
        match value {
            Some((v, close)) => {
                out.push_str(v);
                rest = from.get(close + 1..).unwrap_or_default();
            }
            None => {
                out.push('{');
                rest = from.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled_in_one_pass() {
        assert_eq!(fill("{object}: {count} places", &[("count", "3"), ("object", "Rind")]), "Rind: 3 places");
        // A name that holds a placeholder is shown as it is.
        assert_eq!(fill("{object}: {count} places", &[("object", "{count}"), ("count", "3")]), "{count}: 3 places");
        assert_eq!(fill("{unknown} {", &[("count", "3")]), "{unknown} {");
        assert_eq!(fill("", &[]), "");
        assert_eq!(fill("{count}{count}", &[("count", "1")]), "11");
    }

    #[test]
    fn a_count_is_written_as_its_language_writes_numbers() {
        let pl = Lang::from_code("pl").unwrap();
        // Without a row, the English forms, with the language's digits.
        assert_eq!(plural_in(Lang::EN, 4752, "{count} test stitch", "{count} test stitches"), "4,752 test stitches");
        assert_eq!(plural_in(Lang::EN, 1, "{count} test stitch", "{count} test stitches"), "1 test stitch");
        assert_eq!(plural_in(pl, 4752, "{count} test stitch", "{count} test stitches"), "4752 test stitches");
        assert_eq!(plural_in(pl, 14752, "{count} test stitch", "{count} test stitches"), "14\u{a0}752 test stitches");
    }
}
