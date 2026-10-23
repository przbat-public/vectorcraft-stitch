//! Numbers as the interface language writes them, in the embroidery interface's read-only text
//! (the Embroidery panel, the status bar's counters, the canvas's labels): Polish "0,38 mm",
//! "100,4 mm", "4752" and "14 752", English "0.38 mm" and "4,752". The project owner's choice
//! (2026-10-23): read-only text follows the language, while input fields keep the app's point, which
//! is all their parser takes.
//!
//! Each language's decimal mark, group mark and minimum grouping digits are CLDR's ([`SYMBOLS`]),
//! as ICU 78.3 / CLDR 48 writes them (the Unicode CLDR number symbols chart,
//! <https://www.unicode.org/cldr/charts/latest/by_type/numbers.symbols.html>). A number that isn't
//! finite is written "–", and −0 as 0.

use crate::i18n::Lang;

/// What a number is written as when it isn't one (NaN, ±∞): a dash, as a table shows a missing
/// value.
pub const NOT_A_NUMBER: &str = "–";

/// No-break space (U+00A0): the group mark of Czech, Russian, Ukrainian, Swedish, Finnish,
/// Georgian, Bulgarian, Hungarian and Polish (CLDR).
const NO_BREAK_SPACE: char = '\u{a0}';
/// Narrow no-break space (U+202F): French's group mark (CLDR).
const NARROW_NO_BREAK_SPACE: char = '\u{202f}';

/// The digits in a group: thousands.
const GROUP: usize = 3;

/// How a language writes numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbols {
    /// Between the whole part and the fraction.
    pub decimal: char,
    /// Between groups of three digits.
    pub group: char,
    /// The fewest digits the leftmost group must have for the number to be grouped at all
    /// (CLDR's `minimumGroupingDigits`): 2 leaves four-digit numbers whole ("4752"), as Polish,
    /// Spanish and Italian write them.
    pub min_grouping: usize,
}

/// English's: the point, the comma, grouped from four digits. Also the fallback for a language
/// missing from [`SYMBOLS`] (a test keeps every registered language in it).
const ENGLISH: Symbols = Symbols { decimal: '.', group: ',', min_grouping: 1 };
const COMMA_POINT: Symbols = Symbols { decimal: ',', group: '.', min_grouping: 1 };
const COMMA_POINT_FROM_FIVE: Symbols = Symbols { decimal: ',', group: '.', min_grouping: 2 };
const COMMA_SPACE: Symbols = Symbols { decimal: ',', group: NO_BREAK_SPACE, min_grouping: 1 };
const COMMA_SPACE_FROM_FIVE: Symbols = Symbols { decimal: ',', group: NO_BREAK_SPACE, min_grouping: 2 };
const COMMA_NARROW_SPACE: Symbols = Symbols { decimal: ',', group: NARROW_NO_BREAK_SPACE, min_grouping: 1 };

/// Each interface language's number symbols, by its code (CLDR 48, read through ICU 78.3's
/// `Intl.NumberFormat` on 2026-10-22; Polish checked in CLDR's own data: decimal ",",
/// `minimumGroupingDigits` 2).
pub const SYMBOLS: &[(&str, Symbols)] = &[
    ("en", ENGLISH),
    ("ja", ENGLISH),
    ("ko", ENGLISH),
    ("zh-hans", ENGLISH),
    ("zh-hant", ENGLISH),
    ("el", COMMA_POINT),
    ("vi", COMMA_POINT),
    ("de", COMMA_POINT),
    ("pt-br", COMMA_POINT),
    ("nl", COMMA_POINT),
    ("da", COMMA_POINT),
    ("id", COMMA_POINT),
    ("es", COMMA_POINT_FROM_FIVE),
    ("it", COMMA_POINT_FROM_FIVE),
    ("cs", COMMA_SPACE),
    ("ru", COMMA_SPACE),
    ("uk", COMMA_SPACE),
    ("sv", COMMA_SPACE),
    ("fi", COMMA_SPACE),
    ("ka", COMMA_SPACE_FROM_FIVE),
    ("bg", COMMA_SPACE_FROM_FIVE),
    ("hu", COMMA_SPACE_FROM_FIVE),
    ("pl", COMMA_SPACE_FROM_FIVE),
    ("fr", COMMA_NARROW_SPACE),
];

/// `lang`'s number symbols.
pub fn symbols(lang: Lang) -> Symbols {
    SYMBOLS.iter().find(|(code, _)| *code == lang.code()).map_or(ENGLISH, |(_, s)| *s)
}

/// `digits` (ASCII digits only) grouped by threes with `s`'s group mark, when there are enough of
/// them ([`Symbols::min_grouping`]).
fn grouped(digits: &str, s: Symbols) -> String {
    let n = digits.len();
    if n < GROUP + s.min_grouping {
        return digits.to_string();
    }
    let mut out = String::with_capacity(n + n / GROUP * s.group.len_utf8());
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(GROUP) {
            out.push(s.group);
        }
        out.push(c);
    }
    out
}

/// A count as `lang` writes it: "4,752" in English, "4752" and "14 752" in Polish.
pub fn count(lang: Lang, n: u64) -> String {
    grouped(&n.to_string(), symbols(lang))
}

/// `v` as `lang` writes it, to at most `places` decimals with trailing zeros left out ("0.38",
/// "12.7", "1"), its whole part grouped as [`count`] groups: Polish "0,38", "12 345,6". Not a
/// number: [`NOT_A_NUMBER`].
pub fn decimal(lang: Lang, v: f64, places: usize) -> String {
    if !v.is_finite() {
        return NOT_A_NUMBER.to_string();
    }
    let s = symbols(lang);
    let text = format!("{v:.places$}");
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    let fraction = fraction.trim_end_matches('0');
    // Rounded to nothing (−0.001 to two places, or −0 itself): no sign before a zero.
    let zero = whole.bytes().all(|b| b == b'0') && fraction.is_empty();
    let mut out = String::new();
    if !zero {
        out.push_str(sign);
    }
    out.push_str(&grouped(whole, s));
    if !fraction.is_empty() {
        out.push(s.decimal);
        out.push_str(fraction);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(code: &str) -> Lang {
        Lang::from_code(code).unwrap_or_else(|| panic!("{code} is registered"))
    }

    #[test]
    fn counts_are_grouped_as_each_language_groups_them() {
        let nb = NO_BREAK_SPACE;
        let nnb = NARROW_NO_BREAK_SPACE;
        let cases: &[(&str, u64, String)] = &[
            ("en", 4752, "4,752".into()),
            ("en", 999, "999".into()),
            ("en", 1_234_567, "1,234,567".into()),
            ("pl", 4752, "4752".into()),
            ("pl", 14752, format!("14{nb}752")),
            ("pl", 500_000, format!("500{nb}000")),
            ("de", 4752, "4.752".into()),
            ("de", 14752, "14.752".into()),
            ("es", 4752, "4752".into()),
            ("es", 12345, "12.345".into()),
            ("fr", 4752, format!("4{nnb}752")),
            ("ja", 14752, "14,752".into()),
            ("ru", 4752, format!("4{nb}752")),
            ("en", 0, "0".into()),
        ];
        for (code, n, want) in cases {
            assert_eq!(count(lang(code), *n), *want, "{code} {n}");
        }
        assert_eq!(count(lang("en"), u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn decimals_take_the_languages_mark_and_lose_trailing_zeros() {
        let nb = NO_BREAK_SPACE;
        let cases: &[(&str, f64, usize, String)] = &[
            ("en", 0.38, 2, "0.38".into()),
            ("pl", 0.38, 2, "0,38".into()),
            ("pl", 100.4, 1, "100,4".into()),
            ("pl", 12345.6, 1, format!("12{nb}345,6")),
            ("de", 12345.6, 1, "12.345,6".into()),
            ("en", 2.50, 2, "2.5".into()),
            ("en", 1.0, 2, "1".into()),
            ("en", 12.704, 2, "12.7".into()),
            ("en", -1.5, 1, "-1.5".into()),
            ("en", -0.0, 2, "0".into()),
            ("en", -0.001, 2, "0".into()),
            ("ja", 0.38, 2, "0.38".into()),
        ];
        for (code, v, places, want) in cases {
            assert_eq!(decimal(lang(code), *v, *places), *want, "{code} {v}");
        }
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(decimal(lang("pl"), v, 2), NOT_A_NUMBER);
        }
        // Far beyond any measure: written out, never a panic.
        assert!(decimal(lang("en"), f64::MAX, 2).len() > 300);
    }

    #[test]
    fn every_language_has_number_symbols() {
        let codes: Vec<&str> = SYMBOLS.iter().map(|(c, _)| *c).collect();
        for l in Lang::all() {
            assert!(codes.contains(&l.code()), "{} has no number symbols", l.code());
        }
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "each language once");
    }

    /// The marks numbers are written with have glyphs in each interface font's first font, so
    /// they show as marks, not as boxes or a fallback's glyph.
    #[test]
    fn number_marks_have_glyphs_in_the_interface_fonts() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        ctx.fonts_mut(|fonts| {
            let families: Vec<_> = fonts.definitions().families.iter().map(|(f, stack)| (f.clone(), stack.first().cloned())).collect();
            // The monospace font shows coordinates, written with the app's point.
            for (family, first) in families.into_iter().filter(|(_, first)| first.as_deref() != Some("JetBrainsMono")) {
                let Some(first) = first else { continue };
                let mut font = fonts.fonts.font(&family);
                let chars = font.characters();
                for (_, s) in SYMBOLS {
                    for ch in [s.decimal, s.group] {
                        assert!(chars.get(&ch).is_some_and(|f| f.contains(&first)), "{first} ({family:?}) has no {:?}", ch);
                    }
                }
                assert!(NOT_A_NUMBER.chars().all(|ch| chars.get(&ch).is_some_and(|f| f.contains(&first))));
            }
        });
    }
}
