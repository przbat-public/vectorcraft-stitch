//! The Stitches tab's rows: the columns fitted to the panel's width, the head, a row drawn (its
//! words, its marks, the focus and the selection), and the words of a record's kind and of the
//! filters' chips.

use super::*;

/// A kind's word.
pub(super) fn kind_word(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Stitch => crate::et!("stitch"),
        RecordKind::Jump => crate::et!("jump"),
        RecordKind::Trim => crate::et!("trim"),
        RecordKind::Stop => crate::et!("stop"),
    }
}

/// A filter's chip words, with its count.
pub(super) fn chip_words(filter: Filter, count: usize, limits: &StitchLimits, lang: Lang) -> (String, String) {
    let n = numbers::count(lang, count as u64);
    let (word, full) = match filter {
        Filter::All => (crate::et!("All").to_string(), None),
        Filter::Jumps => (crate::et!("Jumps").to_string(), None),
        Filter::Trims => (crate::et!("Trims").to_string(), None),
        Filter::Stops => (crate::et!("Stops").to_string(), None),
        Filter::Short => (
            crate::et!("Too short").to_string(),
            Some(crate::embroidery::text::fill(
                crate::et!("Shorter than {value} mm"),
                &[("value", &numbers::decimal(lang, limits.shortest, LENGTH_DECIMALS))],
            )),
        ),
        // Both lines, when the format's records are the nearer: a diagonal within them can still be
        // past the longest stitch.
        Filter::Long => (
            crate::et!("Too long").to_string(),
            Some(if limits.step < limits.longest {
                crate::embroidery::text::fill(
                    crate::et!("Longer than {longest} mm, or along x or y than one {format} record ({step} mm)"),
                    &[
                        ("longest", &numbers::decimal(lang, limits.longest, LENGTH_DECIMALS)),
                        ("format", limits.format.label()),
                        ("step", &numbers::decimal(lang, limits.step, LENGTH_DECIMALS)),
                    ],
                )
            } else {
                crate::embroidery::text::fill(
                    crate::et!("Longer than {value} mm"),
                    &[("value", &numbers::decimal(lang, limits.longest, LENGTH_DECIMALS))],
                )
            }),
        ),
    };
    let shown = crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &word), ("count", &n)]);
    let named = match full {
        Some(f) => crate::embroidery::text::fill(crate::et!("{filter} {count}"), &[("filter", &f), ("count", &n)]),
        None => shown.clone(),
    };
    (shown, named)
}

/// The columns drawn at a width: every column with words where they fit (the kinds' words in the
/// language drawn), else the kind as a glyph, else without X and Y (in the row's name for screen
/// readers). A head wider than its column is cut short with "…", whole for screen readers.
pub(super) struct Columns {
    layout: Layout,
    widths: [f32; COLUMNS],
}

/// How the columns are laid out at a width ([`Columns::fit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// Every column, the kinds in words.
    Words,
    /// Every column, the kinds as glyphs.
    Glyphs,
    /// The kinds as glyphs and no X and Y (they go in the row's name for screen readers).
    Narrow,
}

/// A hairline's width, points: the head's rule, the chip's edge, the focus ring's inset and the
/// panel's colour inside it.
const HAIRLINE: f32 = 1.0;
/// The corners of the chip and of the focus ring, points.
const ROUND: u8 = 2;

/// The columns, in order: the chip and the number, the kind, X, Y and the length.
const COLUMNS: usize = 5;
/// Each column's place ([`COLUMNS`]' order).
const NUMBER: usize = 0;
const KIND: usize = 1;
const X: usize = 2;
const Y: usize = 3;
const LENGTH: usize = 4;
/// The columns the narrow layout draws: the number, the kind and the length.
const NARROW_COLUMNS: usize = 3;

/// The column widths, points, of the widest layout: the chip and the number, the kind, X, Y and
/// the length, then a glyph's kind; estimates the language tests check at 220, 256 and 300 pt.
pub(super) const NUMBER_W: f32 = 72.0;
pub(super) const KIND_W: f32 = 64.0;
pub(super) const KIND_GLYPH_W: f32 = 20.0;
pub(super) const PLACE_W: f32 = 50.0;
pub(super) const LENGTH_W: f32 = 48.0;

impl Columns {
    pub(super) fn fit(ui: &Ui, width: f32) -> Self {
        let all = NUMBER_W + KIND_W + 2.0 * PLACE_W + LENGTH_W;
        let glyphs = NUMBER_W + KIND_GLYPH_W + 2.0 * PLACE_W + LENGTH_W;
        let font = egui::FontId::proportional(TEXT);
        let kinds = [RecordKind::Stitch, RecordKind::Jump, RecordKind::Trim, RecordKind::Stop];
        let widest =
            kinds.iter().map(|k| ui.painter().layout_no_wrap(kind_word(*k).to_string(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max);
        let mut c = if width >= all && widest + 2.0 * CELL_PAD <= KIND_W {
            Self { layout: Layout::Words, widths: [NUMBER_W, KIND_W, PLACE_W, PLACE_W, LENGTH_W] }
        } else if width >= glyphs {
            Self { layout: Layout::Glyphs, widths: [NUMBER_W, KIND_GLYPH_W, PLACE_W, PLACE_W, LENGTH_W] }
        } else {
            Self { layout: Layout::Narrow, widths: [NUMBER_W, KIND_GLYPH_W, 0.0, 0.0, LENGTH_W] }
        };
        // The room left over goes to the heads cut short, each column widened to its head in turn:
        // the length, X, Y, then the kind.
        let heads = heads();
        let mut spare = width - c.widths.iter().sum::<f32>();
        for k in [LENGTH, X, Y, KIND] {
            let (Some(head), Some(w)) = (heads.get(k), c.widths.get(k).copied()) else { continue };
            if spare <= 0.0 || w <= 0.0 || (k == KIND && c.layout != Layout::Words) {
                continue;
            }
            let need = ui.painter().layout_no_wrap(head.to_string(), font.clone(), Color32::WHITE).size().x + 2.0 * CELL_PAD - w;
            if need > 0.0
                && let Some(slot) = c.widths.get_mut(k)
            {
                let grow = need.min(spare);
                *slot += grow;
                spare -= grow;
            }
        }
        c
    }

    pub(super) fn count(&self) -> usize {
        if self.layout == Layout::Narrow { NARROW_COLUMNS } else { COLUMNS }
    }
}

/// The columns' heads: the number, the kind, X, Y and the length.
pub(super) fn heads() -> [&'static str; 5] {
    ["#", crate::et!("Type"), crate::et!("X (mm)"), crate::et!("Y (mm)"), crate::et!("Length (mm)")]
}

/// The column heads.
pub(super) fn header(ui: &mut Ui, columns: &Columns, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let heads = heads();
    let mut x = rect.left();
    for (k, (head, w)) in heads.iter().zip(columns.widths).enumerate() {
        if w <= 0.0 {
            continue;
        }
        let at = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, ROW_HEIGHT));
        x += w;
        // A widget of the heads' row, so that its node hangs under the grid the row is drawn for:
        // every column with room has its head for screen readers, as many as the grid says.
        let id = ui.interact(at, ui.id().with(("embroidery-stitch-head", k)), Sense::hover()).id;
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::ColumnHeader);
            node.set_label(*head);
        });
        // The kind's head drawn only where its words are (a glyph's column is too narrow for it).
        if k == KIND && columns.layout != Layout::Words {
            continue;
        }
        let mut job = egui::text::LayoutJob::simple_singleline(head.to_string(), egui::FontId::proportional(TEXT), dim(t));
        job.wrap =
            egui::text::TextWrapping { max_width: (w - 2.0 * CELL_PAD).max(0.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let galley = ui.painter().layout_job(job);
        let size = galley.size();
        ui.painter().galley(at.right_center() - vec2(CELL_PAD + size.x, size.y / 2.0), galley, dim(t));
    }
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(HAIRLINE, t.divider));
}

/// How a row is marked as the list's focus: not at all, quietly (the list doesn't have the
/// keyboard: the keys reach the canvas), or ringed (they are the list's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FocusMark {
    None,
    Quiet,
    Ring,
}

/// How a row is drawn: lit when its needle point is selected, its focus mark, and dimmed while it
/// is of a plan an edit has left behind.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowLook {
    pub(super) selected: bool,
    pub(super) focus: FocusMark,
    pub(super) dimmed: bool,
}

/// Draw one row → its name for screen readers ("Record 4695: stitch, X 25.1 mm, Y 23.2 mm, length
/// 2.61 mm, too short").
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_row(
    app: &VectorcraftApp,
    ui: &Ui,
    rect: Rect,
    plan: &DesignPlan,
    row: &stitch_list::Row,
    columns: &Columns,
    flags: &Flags,
    look: RowLook,
    lang: Lang,
    t: &Tokens,
) -> String {
    let RowLook { selected, focus, dimmed } = look;
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::ZERO, t.row_selected);
    }
    let font = egui::FontId::proportional(TEXT);
    let faded = |c: Color32| if dimmed { c.gamma_multiply(super::super::STALE_INK) } else { c };
    let ink = faded(if selected { t.text_strong } else { t.text });
    let origin = app.ruler_origin();
    let x_mm = pt_to_mm(mm_to_pt(row.at.x) - origin.x);
    let y_mm = pt_to_mm(mm_to_pt(row.at.y) - origin.y);
    let stitch_list::Judgement { short, long } = flags.judged(row.record);
    let mut x = rect.left();
    // The thread chip (of the plan the row is of) and the number.
    let colour = plan.blocks.get(row.block).map(|b| b.thread.color()).map_or(t.text_dim, |[r, g, b]| faded(Color32::from_rgb(r, g, b)));
    let chip = Rect::from_center_size(pos2(x + CELL_PAD + CHIP / 2.0, rect.center().y), vec2(CHIP, CHIP));
    p.rect(chip, CornerRadius::same(ROUND), colour, Stroke::new(HAIRLINE, t.border), egui::StrokeKind::Inside);
    let number = numbers::count(lang, row.record as u64);
    p.text(pos2(x + columns.widths[NUMBER] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &number, font.clone(), ink);
    x += columns.widths[NUMBER];
    let kind = kind_word(row.kind);
    let dim = faded(marks::readable_text(t.text_dim, &[if selected { t.row_selected } else { t.panel }], t.dark));
    if columns.layout == Layout::Words {
        let colour = if row.kind == RecordKind::Stitch { ink } else { dim };
        p.text(pos2(x + columns.widths[KIND] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, kind, font.clone(), colour);
    } else {
        let glyph = match row.kind {
            RecordKind::Stitch => "·",
            RecordKind::Jump => "⤳",
            RecordKind::Trim => "✂",
            RecordKind::Stop => "■",
        };
        p.text(pos2(x + columns.widths[KIND] / 2.0, rect.center().y), egui::Align2::CENTER_CENTER, glyph, font.clone(), dim);
    }
    x += columns.widths[KIND];
    let (xs, ys) = (numbers::fixed(lang, x_mm, PLACE_DECIMALS), numbers::fixed(lang, y_mm, PLACE_DECIMALS));
    if columns.layout != Layout::Narrow {
        p.text(pos2(x + columns.widths[X] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &xs, font.clone(), ink);
        x += columns.widths[X];
        p.text(pos2(x + columns.widths[Y] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &ys, font.clone(), ink);
        x += columns.widths[Y];
    }
    let length = row.length.map(|l| numbers::fixed(lang, l, LENGTH_DECIMALS));
    let shown = length.clone().unwrap_or_else(|| "–".to_string());
    p.text(pos2(x + columns.widths[LENGTH] - CELL_PAD, rect.center().y), egui::Align2::RIGHT_CENTER, &shown, font, ink);
    if short || long {
        marks::paint(p, pos2(x + MARK / 2.0, rect.center().y), MARK, Mark::Warning, t);
    }
    match focus {
        FocusMark::Ring => {
            let ring = marks::readable(t.accent, &[if selected { t.row_selected } else { t.panel }], t.dark);
            p.rect_stroke(rect.shrink(HAIRLINE), CornerRadius::same(ROUND), Stroke::new(FOCUS_RING_PT, ring), egui::StrokeKind::Inside);
            // The panel's colour inside it: the ring reads on a selected row's colour too.
            let inner = HAIRLINE + FOCUS_RING_PT;
            p.rect_stroke(rect.shrink(inner), CornerRadius::same(ROUND), Stroke::new(HAIRLINE, t.panel), egui::StrokeKind::Inside);
        }
        FocusMark::Quiet => {
            p.rect_stroke(rect.shrink(HAIRLINE), CornerRadius::same(ROUND), Stroke::new(FOCUS_QUIET_PT, t.border), egui::StrokeKind::Inside);
        }
        FocusMark::None => {}
    }
    let mut name = crate::embroidery::text::fill(
        crate::et!("Record {number}: {type}, X {x} mm, Y {y} mm"),
        &[("number", &number), ("type", kind), ("x", &xs), ("y", &ys)],
    );
    if let Some(l) = length {
        name.push_str(&crate::embroidery::text::fill(crate::et!(", length {value} mm"), &[("value", &l)]));
    }
    if short {
        name.push_str(crate::et!(", too short"));
    }
    if long {
        name.push_str(crate::et!(", too long"));
    }
    name
}
