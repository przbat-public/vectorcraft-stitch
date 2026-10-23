//! The Check tab's stitch-length chart: the design's needle-down stitches by length
//! (`embroidery.stats`' `lengths`), a bar per bin, half a millimetre each up to 12.5 mm, then up to
//! 12.7 mm, then every longer stitch, which sits in the "too long" band: the longest stitch the
//! check lets pass (`stitchTooLong`'s threshold) is where the band starts (the project owner's
//! choice), so the band's bar counts that rule's hits exactly.
//!
//! - The median heads the chart; hovering a bar says its range and count.
//! - Bars are as tall as their count against the tallest; a bin with stitches is never thinner than
//!   [`MIN_BAR_PT`], so that a few long stitches still show.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::Value;

use crate::embroidery::marks::{self, Mark};
use crate::embroidery::numbers;
use crate::embroidery::text::fill;
use crate::theme::Tokens;

/// The chart's height, its bars' area, points. An estimate fitted to the mockup's chart.
pub const CHART_PT: f32 = 64.0;
/// The shortest bar of a bin with stitches, points: one stitch still shows.
pub const MIN_BAR_PT: f32 = 2.0;
/// The gap between bars, points.
const BAR_GAP_PT: f32 = 1.0;
/// Room round the chart in its card, and for the axis's numbers under it, points.
const PAD_PT: f32 = 8.0;
const AXIS_PT: f32 = 14.0;
/// The axis's numbers, millimetres: every 5 mm, and where the band starts.
const AXIS_EVERY_MM: f64 = 5.0;
/// The size of the chart's words, points.
const SMALL: f32 = 11.0;
/// How strongly the "too long" band tints the chart's background behind it: a quarter of the
/// warning colour. An estimate: visible, quieter than the bars.
const BAND_TINT: f32 = 0.25;

/// A bin as the reply gives it: from, to (none for the last, open bin), how many stitches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bin {
    pub from: f64,
    pub to: Option<f64>,
    pub count: u64,
}

/// The bins of `embroidery.stats`' reply, and the median; none when the reply has no stitch
/// lengths (an older engine) or no stitch.
pub fn bins(stats: &Value) -> Option<(Vec<Bin>, Option<f64>)> {
    let lengths = &stats["lengths"];
    let bins: Vec<Bin> = lengths["bins"]
        .as_array()?
        .iter()
        .filter_map(|b| {
            Some(Bin {
                from: b["from"].as_f64().filter(|v| v.is_finite())?,
                to: b["to"].as_f64().filter(|v| v.is_finite()),
                count: b["count"].as_u64()?,
            })
        })
        .collect();
    (bins.iter().any(|b| b.count > 0)).then_some((bins, lengths["median"].as_f64()))
}

/// The chart, under the rules: `stats` gives the bins, `too_long` the threshold the band starts
/// at (the check's `stitchTooLong`; none: no band).
pub fn show(ui: &mut Ui, stats: &Value, too_long: Option<f64>, stale: bool) {
    let Some((bins, median)) = bins(stats) else { return };
    let t = Tokens::get(ui.ctx());
    let lang = crate::i18n::current();
    let ink = |c: Color32| if stale { c.gamma_multiply(0.6) } else { c };
    let title = crate::et!("Stitch lengths");
    let median_words = median.map(|m| fill(crate::et!("Median: {value} mm"), &[("value", &numbers::decimal(lang, m, 1))]));
    egui::Frame::NONE.fill(t.panel_darker).corner_radius(t.cr()).inner_margin(PAD_PT).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(SMALL).strong().color(ink(t.text)));
            if let Some(m) = &median_words {
                ui.label(egui::RichText::new(format!("· {m}")).size(SMALL).color(ink(t.text_dim)));
            }
        });
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, CHART_PT + AXIS_PT), Sense::hover());
        let area = Rect::from_min_size(rect.min, vec2(width, CHART_PT));
        let p = ui.painter_at(rect);
        let n = bins.len().max(1) as f32;
        let bar = (width / n - BAR_GAP_PT).max(1.0);
        let tallest = bins.iter().map(|b| b.count).max().unwrap_or(1).max(1) as f32;
        let warning = marks::colour(&t, Mark::Warning);
        // The band: from the bin that starts at the threshold to the chart's end.
        let band_at = too_long.and_then(|limit| bins.iter().position(|b| (b.from - limit).abs() < 1e-6));
        if let Some(k) = band_at {
            let x = area.left() + k as f32 * (width / n);
            let band = Rect::from_min_max(pos2(x, area.top()), area.right_bottom());
            p.rect_filled(band, 0.0, ink(warning.gamma_multiply(BAND_TINT)));
            let words = marks::text_colour(&t, Mark::Warning);
            p.text(band.right_top() + vec2(-2.0, 1.0), Align2::RIGHT_TOP, crate::et!("Too long"), FontId::proportional(SMALL - 1.0), ink(words));
        }
        p.line_segment([area.left_bottom(), area.right_bottom()], Stroke::new(1.0, ink(t.border)));
        let pointer = resp.hover_pos();
        let mut hovered = None;
        for (k, b) in bins.iter().enumerate() {
            let x = area.left() + k as f32 * (width / n);
            let h = if b.count == 0 { 0.0 } else { ((b.count as f32 / tallest) * CHART_PT).max(MIN_BAR_PT) };
            let column = Rect::from_min_max(pos2(x, area.top()), pos2(x + bar, area.bottom()));
            let shape = Rect::from_min_max(pos2(x, area.bottom() - h), pos2(x + bar, area.bottom()));
            let colour = if band_at.is_some_and(|at| k >= at) { warning } else { t.accent };
            if b.count > 0 {
                p.rect_filled(shape, 0.0, ink(colour));
            }
            if pointer.is_some_and(|at| column.contains(at)) {
                hovered = Some(*b);
            }
        }
        // The axis: every 5 mm, and the band's start.
        let marks_at: Vec<f64> = bins
            .iter()
            .filter(|b| (b.from / AXIS_EVERY_MM).fract().abs() < 1e-9 || band_at.is_some_and(|at| bins.get(at).is_some_and(|s| s.from == b.from)))
            .map(|b| b.from)
            .collect();
        for from in marks_at {
            let Some(k) = bins.iter().position(|b| b.from == from) else { continue };
            let x = area.left() + k as f32 * (width / n);
            p.text(
                pos2(x, area.bottom() + 2.0),
                Align2::CENTER_TOP,
                numbers::decimal(lang, from, 1),
                FontId::proportional(SMALL - 2.0),
                ink(t.text_dim),
            );
        }
        let said = [Some(title.to_string()), median_words.clone()].into_iter().flatten().collect::<Vec<_>>().join(". ");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &said));
        // A figure for a screen reader, its bins in words (a pointer reads one at a time).
        let described: Vec<String> = bins.iter().filter(|b| b.count > 0).map(bin_words).collect();
        ui.ctx().accesskit_node_builder(resp.id, |node| {
            node.set_role(egui::accesskit::Role::Figure);
            node.set_description(described.join("; "));
        });
        if let Some(b) = hovered {
            resp.on_hover_text(bin_words(&b));
        }
    });
}

/// A bin in words: its range and how many stitches it has.
pub fn bin_words(b: &Bin) -> String {
    let lang = crate::i18n::current();
    let stitches = crate::etn!(b.count, "{count} stitch", "{count} stitches");
    let from = numbers::decimal(lang, b.from, 1);
    match b.to {
        Some(to) => {
            fill(crate::et!("{from}–{to} mm: {stitches}"), &[("from", &from), ("to", &numbers::decimal(lang, to, 1)), ("stitches", &stitches)])
        }
        None => fill(crate::et!("Over {value} mm: {stitches}"), &[("value", &from), ("stitches", &stitches)]),
    }
}
