//! Stitch editing's selection on the canvas (VectorCraft Stitch's manual stitches), drawn over the
//! stitch layer by the canvas's controls, never by the layer itself: a selection change draws no
//! stitches again and uploads no texture.
//!
//! - **The selection layer:** the selected needle points as filled circles in their layer's colour
//!   (lifted to 3:1 against the light edge, as the exit handle is) inside a two-tone edge, and the
//!   stitches touching them drawn two-tone in that colour; past [`SELECTION_SHAPES_MAX`] selected,
//!   the selection's box dashed in two tones instead. While a drag moves them, translated by the
//!   drag's move: nothing is built again until the release.
//! - **The current needle point** (`[` `]`, the last pressed) ringed in two tones; the needle point
//!   or the stitch under the pointer outlined; the marquee drawn dashed in two tones.
//! - **The range pills** at the selection's first and last needle points: their record numbers in
//!   the plan shown, while it is of them as they are ("#…" while it lags).
//! - **The live check** ([`bubble`]): beside the selection, how many needle points are selected and
//!   their numbers, the drag's move, and the stitches touching them against the one set of limits
//!   ([`stitch_limits`]): their shortest and longest with a neutral ✓, or how many are too short or
//!   too long with the Check tab's warning mark. Worked out from the document once a change; during
//!   a drag only the stitches with one end moved are measured again, at most
//!   [`RUBBER_BANDS_MAX`] a frame.
//!
//! Built from the document (`Selection.anchors`), once per revision ([`Kept`]), held while a
//! gesture is under way (its previews move the revision every frame).

use egui::{Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect as ERect, Shape, Stroke, StrokeKind, pos2, vec2};
use vectorcraft_doc::{Document, NodeId, NodeKind, Selection};
use vectorcraft_engine::cmd::embroidery::{StitchEditing, StitchLimits, stitch_limits};
use vectorcraft_geom::{Point, Rect, Vec2};

use super::handles::{dark, light};
use super::marks::{self, Mark};
use crate::VectorcraftApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// The most selected needle points drawn one by one; past it the selection's box is drawn (the
/// shapes a frame stay bounded, and a selection that large reads as an area anyway). An estimate.
pub const SELECTION_SHAPES_MAX: usize = 2_000;
/// The most stitches with one end moved that a drag measures again a frame (and draws stretched);
/// past it the live check waits for the release. As many as the changed objects' dashes a frame
/// (`overlay`'s), an estimate.
pub const RUBBER_BANDS_MAX: usize = 20_000;
/// A selected needle point's circle: its radius and its two-tone edge's widths, points (the edit
/// look's circles, a little larger, so a selected one covers its circle).
const SELECTED_RADIUS_PT: f32 = vectorcraft_render::lines::NEEDLE_CIRCLE_PT as f32 + 0.5;
const EDGE_DARK_PT: f32 = 2.5;
const EDGE_LIGHT_PT: f32 = 1.0;
/// A stitch touching the selection: its colour line's width and the dark line's under it, points.
const STITCH_PT: f32 = 1.5;
const STITCH_UNDER_PT: f32 = 3.0;
/// The current needle point's ring: how far out from the circle, and its three lines' widths,
/// points (dark outside, light, the colour inside), as the handles' focus ring.
const RING_OUT_PT: f32 = 4.0;
const RING_PT: [f32; 3] = [4.0, 2.5, 1.0];
/// The marquee's and the selection box's dashes and gaps, points, and the most dashes a frame.
const DASH: (f32, f32) = (4.0, 3.0);
const DASHES_MAX: usize = 2_000;
/// The live check: its words' size, its padding, its distance from the selection's box, and its
/// marks' size, points.
const BUBBLE_TEXT: f32 = 12.0;
const BUBBLE_PAD: f32 = 8.0;
const BUBBLE_GAP: f32 = 14.0;
const BUBBLE_MARK: f32 = 12.0;
/// The decimals of the bubble's millimetres: lengths to a hundredth (as the commands give them), a
/// move to a tenth (the formats' grid).
const LENGTH_DECIMALS: usize = 2;
const MOVE_DECIMALS: usize = 1;
/// Points in a millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A stitch touching the selection: its ends (document points) and which of them are selected.
#[derive(Clone, Copy, Debug)]
struct Touch {
    a: Point,
    b: Point,
    moves: [bool; 2],
}

/// The lengths of some stitches against the limits: their count, shortest and longest
/// (millimetres), and how many are too short and too long.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Judged {
    pub count: usize,
    pub shortest: Option<f64>,
    pub longest: Option<f64>,
    pub short: usize,
    pub long: usize,
}

impl Judged {
    /// Count the stitch from `a` to `b` (document points) against `limits`.
    fn add(&mut self, a: Point, b: Point, limits: &StitchLimits) {
        let (dx, dy) = ((b.x - a.x) / PT_PER_MM, (b.y - a.y) / PT_PER_MM);
        let mm = dx.hypot(dy);
        if !mm.is_finite() {
            return;
        }
        self.count += 1;
        self.shortest = Some(self.shortest.map_or(mm, |s| s.min(mm)));
        self.longest = Some(self.longest.map_or(mm, |l| l.max(mm)));
        self.short += usize::from(limits.short(mm));
        self.long += usize::from(limits.long(dx, dy));
    }

    fn join(mut self, o: Judged) -> Judged {
        self.count += o.count;
        self.shortest = match (self.shortest, o.shortest) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.longest = match (self.longest, o.longest) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.short += o.short;
        self.long += o.long;
        self
    }
}

/// The selection as drawn, built from the document once a revision ([`Kept::key`]).
#[derive(Default)]
pub struct Kept {
    key: Option<(u64, u64, [u64; 3])>,
    /// How many needle points being edited are selected, and the box they fall in.
    count: usize,
    bounds: Option<Rect>,
    /// The selected needle points and the stitches touching them, while at most
    /// [`SELECTION_SHAPES_MAX`] are selected (else empty: the box is drawn).
    points: Vec<Point>,
    touching: Vec<Touch>,
    /// The stitches with one end selected (a move stretches them), at most [`RUBBER_BANDS_MAX`]
    /// (`None`: more), and the stitches with both ends selected, judged (a move leaves them as
    /// they are).
    moved_ends: Option<Vec<Touch>>,
    fixed: Judged,
    /// The selection's layer's colour.
    color: [u8; 3],
    /// How many times it was built (the tests count them: an idle frame builds nothing).
    pub(crate) builds: usize,
}

/// Build what the selection of `sel` in `doc` draws: the selected needle points of the paths of
/// stitches inside `scope`, the stitches touching them, judged against `limits`.
fn build(doc: &Document, sel: &Selection, scope: NodeId, limits: &StitchLimits) -> Kept {
    let mut k = Kept::default();
    let mut moved: Vec<Touch> = Vec::new();
    let mut too_many_moved = false;
    for &id in &sel.objects {
        let Some(n) = doc.node(id) else { continue };
        let (NodeKind::Path { path, .. }, Some(marks)) = (&n.kind, n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref())) else {
            continue;
        };
        if !doc.ancestry(id).is_some_and(|a| a.contains(&scope)) {
            continue;
        }
        if k.count == 0 {
            k.color = doc.layer_color(id);
        }
        let (alone, double_point) = (path.subpaths.len() == 1, marks.double_point);
        let chosen = sel.anchors.get(&id);
        for (s, sp) in path.subpaths.iter().enumerate() {
            let len = vectorcraft_engine::cmd::embroidery::needle_count(&sp.anchors, alone, double_point);
            let picked = |a: usize| match chosen {
                Some(set) => set.contains(&(s, a)) || (len == 1 && sp.anchors.len() == 2 && set.contains(&(s, 1))),
                None => true,
            };
            for a in 0..len {
                if !picked(a) {
                    continue;
                }
                let Some(p) = sp.anchors.get(a).map(|x| x.p) else { continue };
                k.count += 1;
                k.bounds = Some(k.bounds.map_or(Rect::from_points(p, p), |b| b.union_pt(p)));
                if k.count <= SELECTION_SHAPES_MAX {
                    k.points.push(p);
                }
                // The stitch arriving (when the one before isn't selected: else it was counted
                // leaving that one) and the stitch leaving.
                let before = a.checked_sub(1).filter(|b| !picked(*b)).and_then(|b| sp.anchors.get(b)).map(|x| (x.p, false));
                let after = (a + 1 < len).then(|| sp.anchors.get(a + 1).map(|x| (x.p, picked(a + 1)))).flatten();
                for (q, q_selected, arriving) in before.map(|(q, s)| (q, s, true)).into_iter().chain(after.map(|(q, s)| (q, s, false))) {
                    let t = if arriving { Touch { a: q, b: p, moves: [q_selected, true] } } else { Touch { a: p, b: q, moves: [true, q_selected] } };
                    if t.moves == [true, true] {
                        k.fixed.add(t.a, t.b, limits);
                    } else if moved.len() < RUBBER_BANDS_MAX {
                        moved.push(t);
                    } else {
                        too_many_moved = true;
                    }
                    if k.count <= SELECTION_SHAPES_MAX && k.touching.len() < RUBBER_BANDS_MAX {
                        k.touching.push(t);
                    }
                }
            }
        }
    }
    if k.count > SELECTION_SHAPES_MAX {
        k.points.clear();
        k.touching.clear();
    }
    k.moved_ends = (!too_many_moved).then_some(moved);
    k
}

/// The selection kept for the active document, built again when its revision or the limits
/// changed, but not while a gesture is under way.
fn kept<'a>(app: &'a mut VectorcraftApp, scope: NodeId, limits: &StitchLimits) -> Option<&'a Kept> {
    let st = app.session.active()?;
    let key = (st.uid, st.revision, [limits.shortest.to_bits(), limits.longest.to_bits(), limits.step.to_bits()]);
    let gesture = st.interaction.is_some();
    let k = &app.embroidery.selection;
    if !(k.key == Some(key) || (gesture && k.key.is_some_and(|(u, ..)| u == st.uid))) {
        let mut built = build(&st.doc, &st.selection, scope, limits);
        built.key = Some(key);
        built.builds = app.embroidery.selection.builds + 1;
        app.embroidery.selection = built;
    }
    Some(&app.embroidery.selection)
}

/// The drag's move now (document points), from the tool's options (millimetres on the grid).
fn moved(app: &VectorcraftApp) -> Option<Vec2> {
    let m = app.session.tool_options().get("stitches")?.get("moved")?.as_array()?.clone();
    let (x, y) = (m.first()?.as_f64()?, m.get(1)?.as_f64()?);
    Some(Vec2::new(x * PT_PER_MM, y * PT_PER_MM))
}

/// What the live check says: how many needle points are selected and their numbers, the drag's
/// move, and the stitches touching them, judged; `None` with nothing selected.
pub fn bubble(app: &mut VectorcraftApp) -> Option<Vec<(Option<Mark>, String)>> {
    let editing = super::editing::editing(app)?;
    let st = app.session.active()?;
    let limits = stitch_limits(&st.doc, app.embroidery.check.format.get(&st.uid).copied());
    let by = moved(app);
    let lang = crate::i18n::current();
    let k = kept(app, editing.scope.object, &limits)?;
    if k.count == 0 {
        return None;
    }
    let mut lines = vec![(None, head(&editing, k.count, lang))];
    if let Some(d) = by {
        let words = super::text::fill(
            crate::et!("Moved X {x} · Y {y} mm"),
            &[
                ("x", &super::numbers::decimal(lang, d.x / PT_PER_MM, MOVE_DECIMALS)),
                ("y", &super::numbers::decimal(lang, d.y / PT_PER_MM, MOVE_DECIMALS)),
            ],
        );
        lines.push((None, words));
    }
    // The stitches touching the selection: those with both ends selected as they were, those with
    // one end moved measured again.
    let judged = match (&k.moved_ends, by) {
        (Some(ends), Some(d)) => {
            let mut j = Judged::default();
            for t in ends {
                let (a, b) = (if t.moves[0] { t.a + d } else { t.a }, if t.moves[1] { t.b + d } else { t.b });
                j.add(a, b, &limits);
            }
            Some(k.fixed.join(j))
        }
        (Some(ends), None) => {
            let mut j = Judged::default();
            for t in ends {
                j.add(t.a, t.b, &limits);
            }
            Some(k.fixed.join(j))
        }
        (None, _) => None,
    };
    match judged {
        Some(j) if j.short == 0 && j.long == 0 => {
            if let (Some(s), Some(l)) = (j.shortest, j.longest) {
                let words = format!(
                    "{} · {}",
                    super::text::fill(crate::et!("Shortest: {value} mm"), &[("value", &super::numbers::decimal(lang, s, LENGTH_DECIMALS))]),
                    super::text::fill(crate::et!("Longest: {value} mm"), &[("value", &super::numbers::decimal(lang, l, LENGTH_DECIMALS))])
                );
                lines.push((Some(Mark::Passed), words));
            }
        }
        Some(j) => {
            if j.short > 0 {
                let words = crate::etn!(j.short as u64, "{count} stitch shorter than {value} mm", "{count} stitches shorter than {value} mm");
                lines.push((
                    Some(Mark::Warning),
                    super::text::fill(&words, &[("value", &super::numbers::decimal(lang, limits.shortest, LENGTH_DECIMALS))]),
                ));
            }
            if j.long > 0 {
                lines.push((Some(Mark::Warning), crate::etn!(j.long as u64, "{count} stitch too long", "{count} stitches too long")));
            }
        }
        None => lines.push((None, crate::et!("Updating…").to_string())),
    }
    Some(lines)
}

/// The bubble's first line: the needle points selected and their record numbers ("#…" while the
/// plan shown lags behind them).
fn head(editing: &StitchEditing, count: usize, lang: crate::i18n::Lang) -> String {
    let n = crate::etn!(count as u64, "{count} needle point", "{count} needle points");
    let range = match editing.ends {
        Some([(_, Some(a)), (_, Some(b))]) if a == b => format!("#{}", super::numbers::count(lang, a as u64)),
        Some([(_, Some(a)), (_, Some(b))]) => format!("#{}–{}", super::numbers::count(lang, a as u64), super::numbers::count(lang, b as u64)),
        Some(_) => "#…".to_string(),
        None => return n,
    };
    format!("{n} · {range}")
}

/// The needle point `{id, index}` names in the tool's options (`v`), where it is in `doc`.
fn needle_at(doc: &Document, v: &serde_json::Value) -> Option<Point> {
    let id = NodeId(v.get("id")?.as_u64()?);
    let index = usize::try_from(v.get("index")?.as_u64()?).ok()?;
    let n = doc.node(id)?;
    let path = n.path_data()?;
    let alone = path.subpaths.len() == 1;
    let double_point = n.extras().embroidery.as_deref().and_then(|e| e.stitches.as_ref()).is_some_and(|s| s.double_point);
    let mut first = 0usize;
    for sp in &path.subpaths {
        let len = vectorcraft_engine::cmd::embroidery::needle_count(&sp.anchors, alone, double_point);
        if index < first + len {
            return sp.anchors.get(index - first).map(|a| a.p);
        }
        first += len;
    }
    None
}

/// Draw the selection while stitches are edited: the selected needle points and the stitches
/// touching them (or the selection's box), the current needle point's ring, the hover, the marquee,
/// the range pills and the live check.
pub fn draw(app: &mut VectorcraftApp, ui: &egui::Ui, painter: &egui::Painter, xf: &Xf) {
    let Some(editing) = super::editing::editing(app) else { return };
    let Some(st) = app.session.active() else { return };
    let limits = stitch_limits(&st.doc, app.embroidery.check.format.get(&st.uid).copied());
    let doc = st.doc.clone();
    let options = app.session.tool_options().get("stitches").cloned().unwrap_or_default();
    let by = moved(app).unwrap_or(Vec2::ZERO);
    let t = Tokens::get(ui.ctx());
    let clip = xf.rect;
    let Some(k) = kept(app, editing.scope.object, &limits) else { return };
    let [r, g, b] = k.color;
    let ink = marks::readable(Color32::from_rgb(r, g, b), &[light()], false);
    let screen = |p: Point| xf.to_screen(p + by);
    let mut budget = DASHES_MAX;
    if k.count > SELECTION_SHAPES_MAX {
        if let Some(b) = k.bounds {
            let corners: Vec<Pos2> =
                [(b.x0, b.y0), (b.x1, b.y0), (b.x1, b.y1), (b.x0, b.y1), (b.x0, b.y0)].iter().map(|(x, y)| screen(Point::new(*x, *y))).collect();
            super::overlay::dash(painter, &corners, Stroke::new(STITCH_UNDER_PT, dark()), DASH, clip, &mut budget);
            super::overlay::dash(painter, &corners, Stroke::new(STITCH_PT, ink), DASH, clip, &mut budget);
        }
    } else {
        for tch in &k.touching {
            let a = if tch.moves[0] { screen(tch.a) } else { xf.to_screen(tch.a) };
            let b = if tch.moves[1] { screen(tch.b) } else { xf.to_screen(tch.b) };
            if !clip.intersects(ERect::from_two_pos(a, b)) {
                continue;
            }
            painter.line_segment([a, b], Stroke::new(STITCH_UNDER_PT, dark()));
            painter.line_segment([a, b], Stroke::new(STITCH_PT, ink));
        }
        for p in &k.points {
            let c = screen(*p);
            if !clip.expand(SELECTED_RADIUS_PT + EDGE_DARK_PT).contains(c) {
                continue;
            }
            painter.circle(c, SELECTED_RADIUS_PT, ink, Stroke::new(EDGE_DARK_PT, dark()));
            painter.circle_stroke(c, SELECTED_RADIUS_PT, Stroke::new(EDGE_LIGHT_PT, light()));
        }
    }
    // The needle point or stitch under the pointer, and the current needle point's ring.
    let hover = options.get("hover").cloned().unwrap_or_default();
    if let Some(p) = hover.get("needlePoint").and_then(|v| needle_at(&doc, v)) {
        painter.circle_stroke(xf.to_screen(p), SELECTED_RADIUS_PT + RING_OUT_PT / 2.0, Stroke::new(STITCH_PT, dark()));
    } else if let Some(from) = hover.get("stitch").and_then(|v| needle_at(&doc, v)) {
        let to = hover.get("stitch").and_then(|v| {
            let mut next = v.clone();
            next["index"] = serde_json::json!(v.get("index")?.as_u64()? + 1);
            needle_at(&doc, &next)
        });
        if let Some(to) = to {
            painter.line_segment([xf.to_screen(from), xf.to_screen(to)], Stroke::new(STITCH_UNDER_PT, dark()));
            painter.line_segment([xf.to_screen(from), xf.to_screen(to)], Stroke::new(STITCH_PT, light()));
        }
    }
    if let Some(p) = options.get("current").and_then(|v| needle_at(&doc, v)) {
        let c = xf.to_screen(p);
        let radius = SELECTED_RADIUS_PT + RING_OUT_PT;
        for (width, colour) in RING_PT.iter().zip([dark(), light(), ink]) {
            painter.circle_stroke(c, radius, Stroke::new(*width, colour));
        }
    }
    // The marquee, dashed in two tones.
    if let Some(m) = options.get("marquee").and_then(|v| v.as_array()).and_then(|a| {
        let n: Vec<f64> = a.iter().filter_map(serde_json::Value::as_f64).collect();
        (n.len() == 4).then(|| Rect::new(n[0], n[1], n[2], n[3]))
    }) {
        let corners: Vec<Pos2> =
            [(m.x0, m.y0), (m.x1, m.y0), (m.x1, m.y1), (m.x0, m.y1), (m.x0, m.y0)].iter().map(|(x, y)| xf.to_screen(Point::new(*x, *y))).collect();
        painter.add(Shape::line(corners.clone(), Stroke::new(STITCH_PT, dark())));
        super::overlay::dash(painter, &corners, Stroke::new(STITCH_PT, light()), DASH, clip, &mut budget);
    }
    // The range pills at the selection's first and last needle points, by their numbers.
    if let Some([(a, na), (b, nb)]) = editing.ends {
        let lang = crate::i18n::current();
        let pill = |n: Option<usize>| n.map_or_else(|| "#…".to_string(), |n| format!("#{}", super::numbers::count(lang, n as u64)));
        let ink_edge = Stroke::new(1.0, dark());
        let at = |r: vectorcraft_tools::embroidery::NeedleRef| needle_at(&doc, &serde_json::json!({"id": r.id.0, "index": r.index})).map(screen);
        if let Some(pa) = at(a) {
            super::overlay::label(painter, pa + vec2(-RING_OUT_PT, -RING_OUT_PT), Align2::RIGHT_BOTTOM, &pill(na), ink, ink_edge);
        }
        if (a, na) != (b, nb)
            && let Some(pb) = at(b)
        {
            super::overlay::label(painter, pb + vec2(RING_OUT_PT, RING_OUT_PT), Align2::LEFT_TOP, &pill(nb), ink, ink_edge);
        }
    }
    let box_on_screen = k.bounds.map(|b| {
        let (p, q) = (screen(Point::new(b.x0, b.y0)), screen(Point::new(b.x1, b.y1)));
        ERect::from_two_pos(p, q)
    });
    let Some(lines) = bubble(app) else { return };
    let Some(sel) = box_on_screen else { return };
    draw_bubble(ui, painter, &t, clip, sel, &lines);
}

/// The live check's card beside the selection's box on screen (`sel`), on the side with the most
/// room, within the canvas (`clip`), never over the Contextual Task Bar.
fn draw_bubble(ui: &egui::Ui, painter: &egui::Painter, t: &Tokens, clip: ERect, sel: ERect, lines: &[(Option<Mark>, String)]) {
    let font = FontId::proportional(BUBBLE_TEXT);
    let galleys: Vec<_> = lines.iter().map(|(_, w)| painter.layout_no_wrap(w.clone(), font.clone(), t.text)).collect();
    let indent = BUBBLE_MARK + BUBBLE_PAD / 2.0;
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + indent + 2.0 * BUBBLE_PAD;
    let height = galleys.iter().map(|g| g.size().y).sum::<f32>() + 2.0 * BUBBLE_PAD;
    let size = vec2(width, height);
    let room_right = clip.right() - sel.right();
    let room_left = sel.left() - clip.left();
    let x = if room_right >= size.x + BUBBLE_GAP || room_right >= room_left { sel.right() + BUBBLE_GAP } else { sel.left() - BUBBLE_GAP - size.x };
    let mut rect = ERect::from_min_size(pos2(x, sel.top()), size);
    let shift = vec2(
        (clip.left() - rect.left()).max(0.0) - (rect.right() - clip.right()).max(0.0),
        (clip.top() - rect.top()).max(0.0) - (rect.bottom() - clip.bottom()).max(0.0),
    );
    rect = rect.translate(shift);
    if let Some(bar) = crate::canvas::task_bar_rect(ui.ctx())
        && bar.intersects(rect)
    {
        rect = rect.translate(vec2(0.0, bar.top() - rect.bottom() - BUBBLE_GAP / 2.0));
    }
    painter.rect(rect, CornerRadius::same(t.radius), t.panel, Stroke::new(1.0, t.border), StrokeKind::Inside);
    let mut y = rect.top() + BUBBLE_PAD;
    for ((mark, _), g) in lines.iter().zip(galleys) {
        if let Some(m) = mark {
            marks::paint(painter, pos2(rect.left() + BUBBLE_PAD + BUBBLE_MARK / 2.0, y + g.size().y / 2.0), BUBBLE_MARK, *m, t);
        }
        let h = g.size().y;
        painter.galley(pos2(rect.left() + BUBBLE_PAD + indent, y), g, t.text);
        y += h;
    }
    let words = lines.iter().map(|(_, w)| w.as_str()).collect::<Vec<_>>().join(". ");
    ui.ctx().accesskit_node_builder(Id::new("embroidery-live-check"), |node| {
        node.set_role(egui::accesskit::Role::Status);
        node.set_label(words);
    });
}

#[cfg(test)]
mod tests;
