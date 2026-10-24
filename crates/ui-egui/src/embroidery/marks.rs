//! Severity marks: how much a check's finding matters, said by shape and glyph first and by colour
//! last, so that colour is never the only cue (WCAG 2.1, 1.4.1 Use of Color:
//! <https://www.w3.org/WAI/WCAG21/Understanding/use-of-color.html>).
//!
//! - an error is a filled circle with "!";
//! - a warning, a filled triangle with "!";
//! - a note, a ring with "i";
//! - a passed check, a tick, neutral in the dim text colour (the owner's choice: no green).
//!
//! Drawn in code from the theme's tokens, glyphs included (no font, no asset). The error and warning
//! colours are the theme's `error` and `warning`, darkened on the light themes (lightened on the
//! dark) only as far as they need to reach [`NON_TEXT_CONTRAST`] against the panel behind them:
//! upstream's amber has about 1.5:1 on the light themes.

use egui::{Color32, Painter, Pos2, Shape, Stroke, vec2};

use crate::theme::Tokens;

/// The contrast a mark keeps against the panel behind it, and a glyph against its mark: WCAG 2.1's
/// 1.4.11 Non-text Contrast, 3:1
/// (<https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast.html>).
pub const NON_TEXT_CONTRAST: f32 = 3.0;

/// The contrast text keeps against what it is drawn on: WCAG 2.1's 1.4.3 Contrast (Minimum), 4.5:1
/// for text of normal size (<https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html>).
pub const TEXT_CONTRAST: f32 = 4.5;

/// How finely a colour is moved towards black or white to reach the contrast: twentieths.
const CONTRAST_STEPS: u8 = 20;

/// The size of a mark in a panel row, points: the height of the row's text. An estimate.
pub const MARK_SIZE: f32 = 14.0;

/// A glyph's stroke as a share of the mark's size: bold enough to read at [`MARK_SIZE`]. An
/// estimate.
const GLYPH_WEIGHT: f32 = 0.14;

/// How a finding matters, or that a check found nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    Error,
    Warning,
    Info,
    Passed,
}

impl Mark {
    /// The mark of a rule of `severity` ("error", "warning", "info") with `hits`: passed with none.
    pub fn of(severity: &str, hits: u64) -> Self {
        match severity {
            _ if hits == 0 => Self::Passed,
            "error" => Self::Error,
            "warning" => Self::Warning,
            _ => Self::Info,
        }
    }

    /// The order the Check tab lists rules in: problems first (errors, warnings, notes), then the
    /// checks that passed.
    pub fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Info => 2,
            Self::Passed => 3,
        }
    }
}

/// The colour `mark` is drawn in on the panel `t` describes (its plain, darker, hovered and
/// selected backgrounds).
pub fn colour(t: &Tokens, mark: Mark) -> Color32 {
    readable(base(t, mark), &backgrounds(t), t.dark)
}

/// The colour of text that speaks for `mark` (an error's or a warning's words) on the panel `t`
/// describes: the mark's colour, moved only as far as it takes to read by [`TEXT_CONTRAST`].
pub fn text_colour(t: &Tokens, mark: Mark) -> Color32 {
    towards(base(t, mark), &backgrounds(t), t.dark, TEXT_CONTRAST)
}

/// The colour of text on a fill (a label's pill): black or white, whichever reads better (one of
/// them always reaches [`TEXT_CONTRAST`]).
pub fn text_on(fill: Color32) -> Color32 {
    if contrast(Color32::BLACK, fill) >= contrast(Color32::WHITE, fill) { Color32::BLACK } else { Color32::WHITE }
}

/// The theme's colour for `mark`, before it is made to stand out.
fn base(t: &Tokens, mark: Mark) -> Color32 {
    match mark {
        Mark::Error => t.error,
        Mark::Warning => t.warning,
        Mark::Info | Mark::Passed => t.text_dim,
    }
}

/// What a mark in a panel row is drawn on.
fn backgrounds(t: &Tokens) -> [Color32; 4] {
    [t.panel, t.panel_darker, t.hover, t.row_selected]
}

/// The glyph's colour on a filled mark of `fill`: the theme's text or panel colour, whichever
/// stands out more.
fn glyph_on(t: &Tokens, fill: Color32) -> Color32 {
    if contrast(t.text, fill) >= contrast(t.panel, fill) { t.text } else { t.panel }
}

/// Draw `mark` centred on `centre`, `size` points across, on the panel `t` describes.
pub fn paint(p: &Painter, centre: Pos2, size: f32, mark: Mark, t: &Tokens) {
    paint_in(p, centre, size, mark, t, colour(t, mark));
}

/// Draw `mark` on a background of `back` (the canvas's marks draw it on a white disc): its colour
/// made to stand out from `back` instead of the panel.
pub fn paint_on(p: &Painter, centre: Pos2, size: f32, mark: Mark, t: &Tokens, back: Color32) {
    paint_in(p, centre, size, mark, t, readable(base(t, mark), &[back], luminance(back) < 0.5));
}

fn paint_in(p: &Painter, centre: Pos2, size: f32, mark: Mark, t: &Tokens, c: Color32) {
    let r = size / 2.0;
    let weight = (size * GLYPH_WEIGHT).max(1.0);
    match mark {
        Mark::Error => {
            p.circle_filled(centre, r, c);
            bang(p, centre, r, weight, glyph_on(t, c));
        }
        Mark::Warning => {
            // A triangle on its base, its glyph lowered to the triangle's middle.
            let pts = vec![centre + vec2(0.0, -r), centre + vec2(r, r * 0.8), centre + vec2(-r, r * 0.8)];
            p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
            bang(p, centre + vec2(0.0, r * 0.18), r * 0.8, weight, glyph_on(t, c));
        }
        Mark::Info => {
            p.circle_stroke(centre, r - weight / 2.0, Stroke::new(weight, c));
            let dot = weight * 0.7;
            p.circle_filled(centre + vec2(0.0, -r * 0.42), dot, c);
            p.line_segment([centre + vec2(0.0, -r * 0.1), centre + vec2(0.0, r * 0.5)], Stroke::new(weight, c));
        }
        Mark::Passed => {
            let pts = vec![centre + vec2(-r * 0.75, 0.0), centre + vec2(-r * 0.2, r * 0.55), centre + vec2(r * 0.8, -r * 0.6)];
            p.add(Shape::line(pts, Stroke::new(weight * 1.2, c)));
        }
    }
}

/// An exclamation mark `r` tall around `centre`: a bar, then a dot.
fn bang(p: &Painter, centre: Pos2, r: f32, weight: f32, c: Color32) {
    p.line_segment([centre + vec2(0.0, -r * 0.55), centre + vec2(0.0, r * 0.15)], Stroke::new(weight, c));
    p.circle_filled(centre + vec2(0.0, r * 0.45), weight * 0.7, c);
}

/// `c`, moved towards white on dark backgrounds or black on light ones only as far as it takes to
/// stand out by [`NON_TEXT_CONTRAST`] from each of `backs`.
pub fn readable(c: Color32, backs: &[Color32], dark: bool) -> Color32 {
    towards(c, backs, dark, NON_TEXT_CONTRAST)
}

/// `c`, moved towards white (`dark` backgrounds) or black only as far as it takes to stand out by
/// `ratio` from each of `backs`; all the way when even that falls short.
fn towards(c: Color32, backs: &[Color32], dark: bool, ratio: f32) -> Color32 {
    let target = if dark { Color32::WHITE } else { Color32::BLACK };
    let worst = |c: Color32| backs.iter().map(|b| contrast(c, *b)).fold(f32::INFINITY, f32::min);
    (0..=CONTRAST_STEPS).map(|k| mix(c, target, f32::from(k) / f32::from(CONTRAST_STEPS))).find(|m| worst(*m) >= ratio).unwrap_or(target)
}

/// `a` mixed with `b` by `k` (0: `a`, 1: `b`), channel by channel.
fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let m = |p: u8, q: u8| (f32::from(p) + (f32::from(q) - f32::from(p)) * k).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// WCAG 2.1's relative luminance of an opaque colour
/// (<https://www.w3.org/TR/WCAG21/#dfn-relative-luminance>).
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let s = f32::from(v) / 255.0;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG 2.1's contrast ratio of two opaque colours, 1 to 21
/// (<https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio>).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[cfg(test)]
mod tests {
    use egui::epaint::{CircleShape, PathShape};

    use super::*;
    use crate::theme::{Accent, Brightness, Scheme};

    fn every_theme() -> Vec<Tokens> {
        let mut all = vec![];
        for scheme in Scheme::ALL {
            for b in Brightness::ALL {
                for accent in Accent::ALL {
                    all.push(Tokens::for_scheme(scheme, b, accent));
                }
            }
        }
        all
    }

    /// The shapes a mark is drawn with, by kind: (filled circles, rings, filled polygons, open
    /// lines).
    fn kinds(mark: Mark) -> (usize, usize, usize, usize) {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let t = Tokens::get(ui.ctx());
            paint(ui.painter(), egui::pos2(50.0, 50.0), MARK_SIZE, mark, &t);
        });
        out.textures_delta.clear();
        let mut pending: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
        let mut k = (0, 0, 0, 0);
        while let Some(s) = pending.pop() {
            match s {
                Shape::Vec(v) => pending.extend(v),
                Shape::Circle(CircleShape { fill, stroke, .. }) if fill != Color32::TRANSPARENT && stroke.is_empty() => k.0 += 1,
                Shape::Circle(_) => k.1 += 1,
                Shape::Path(PathShape { closed: true, .. }) => k.2 += 1,
                Shape::Path(_) | Shape::LineSegment { .. } => k.3 += 1,
                _ => {}
            }
        }
        k
    }

    /// Each mark has a shape of its own, so that severity never rests on colour alone.
    #[test]
    fn each_mark_has_its_own_shape() {
        let all = [Mark::Error, Mark::Warning, Mark::Info, Mark::Passed].map(kinds);
        // Error: a filled circle, its glyph a bar and a dot; warning: a filled triangle; note: a
        // ring; passed: a tick, nothing else.
        assert_eq!(all, [(2, 0, 0, 1), (1, 0, 1, 1), (1, 1, 0, 1), (0, 0, 0, 1)]);
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    /// Every mark stands out by 3:1 from every background a panel row has, in every theme, scheme
    /// and accent; a glyph by 3:1 from its mark.
    #[test]
    fn marks_reach_three_to_one_in_every_theme() {
        for t in every_theme() {
            for mark in [Mark::Error, Mark::Warning, Mark::Info, Mark::Passed] {
                let c = colour(&t, mark);
                for back in backgrounds(&t) {
                    let ratio = contrast(c, back);
                    assert!(ratio >= NON_TEXT_CONTRAST, "{mark:?} {c:?} on {back:?}: {ratio}");
                }
                if matches!(mark, Mark::Error | Mark::Warning) {
                    let ratio = contrast(glyph_on(&t, c), c);
                    assert!(ratio >= NON_TEXT_CONTRAST, "{mark:?}'s glyph: {ratio}");
                }
            }
        }
    }

    /// An error's and a warning's words read by 4.5:1 on every background a panel row has, in every
    /// theme; a label's words by 4.5:1 on any fill.
    #[test]
    fn text_reaches_four_and_a_half_to_one() {
        for t in every_theme() {
            for mark in [Mark::Error, Mark::Warning] {
                let c = text_colour(&t, mark);
                for back in backgrounds(&t) {
                    let ratio = contrast(c, back);
                    assert!(ratio >= TEXT_CONTRAST, "{mark:?}'s words {c:?} on {back:?}: {ratio}");
                }
            }
            for fill in
                [t.accent, t.error, t.warning, Color32::from_rgb(79, 128, 255), Color32::YELLOW, Color32::GREEN, Color32::from_rgb(0, 255, 255)]
            {
                assert!(contrast(text_on(fill), fill) >= TEXT_CONTRAST, "{fill:?}");
            }
        }
    }

    /// A colour that already stands out is the theme's own: the dark themes' red and amber.
    #[test]
    fn a_colour_is_moved_only_as_far_as_it_needs() {
        let dark = Tokens::for_brightness(Brightness::Dark);
        assert_eq!(colour(&dark, Mark::Warning), dark.warning);
        assert_eq!(colour(&dark, Mark::Error), dark.error);
        let light = Tokens::for_brightness(Brightness::Light);
        let amber = colour(&light, Mark::Warning);
        assert_ne!(amber, light.warning, "the light theme's amber is darkened");
        // Not past what it needs: a step less falls short.
        let back = backgrounds(&light);
        let short = |c: Color32| back.iter().any(|b| contrast(c, *b) < NON_TEXT_CONTRAST);
        let k = (0..=CONTRAST_STEPS).find(|k| mix(light.warning, Color32::BLACK, f32::from(*k) / f32::from(CONTRAST_STEPS)) == amber);
        let k = k.expect("a step of the mix");
        assert!(k > 0 && short(mix(light.warning, Color32::BLACK, f32::from(k - 1) / f32::from(CONTRAST_STEPS))));
    }

    /// On a white disc, every mark stands out by 3:1, whatever the theme.
    #[test]
    fn marks_on_white_reach_three_to_one() {
        for t in every_theme() {
            for mark in [Mark::Error, Mark::Warning, Mark::Info, Mark::Passed] {
                let c = readable(base(&t, mark), &[Color32::WHITE], false);
                assert!(contrast(c, Color32::WHITE) >= NON_TEXT_CONTRAST, "{mark:?}");
            }
        }
    }

    #[test]
    fn contrast_is_wcags() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-6);
        // #767676 on white is the well-known 4.54:1.
        assert!((contrast(Color32::from_gray(0x76), Color32::WHITE) - 4.54).abs() < 0.01);
    }

    #[test]
    fn problems_come_first() {
        assert_eq!(Mark::of("error", 0), Mark::Passed);
        assert_eq!(Mark::of("warning", 2), Mark::Warning);
        assert_eq!(Mark::of("info", 1), Mark::Info);
        assert_eq!(Mark::of("unknown", 1), Mark::Info);
        let mut marks = [Mark::Passed, Mark::Info, Mark::Error, Mark::Warning];
        marks.sort_by_key(|m| m.rank());
        assert_eq!(marks, [Mark::Error, Mark::Warning, Mark::Info, Mark::Passed]);
    }
}
