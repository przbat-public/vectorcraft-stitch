//! The status bar's embroidery counters: "4,752 stitches · 4 colours · 3 thread changes · about
//! 7 min" at the bar's right end, for the active document, while its stitches are wanted (the
//! stitch view, or the Embroidery panel, open: the project owner's choice). Nothing is computed
//! for them alone: the scheduler adds `embroidery.stats` to every plan wanted
//! ([`super::stitch_preview::figures_query`]), so the bar never starts a plan, and the figures are
//! read once per plan.
//!
//! - **States** ([`State`]): hidden; waiting (blank, so that a quick plan never flashes);
//!   planning, with its progress, once a plan has taken [`super::stitch_preview::STALE_NOTICE_SECS`];
//!   the figures; stale (dimmed, after "Updating…", once a newer plan has taken that long);
//!   nothing to sew; can't be planned (the reason in the tooltip).
//! - **Room.** The longest of five wordings that fits ([`Variant`]); the render and interface
//!   timings give way first, then the counters shorten, then they hide. The X/Y readout and the
//!   message keep the room the bar keeps for them (its fixed slots, `chrome::status_readouts`). The
//!   slot keeps its widest width while its wording stays, so that changing digits move nothing to
//!   its left.
//! - **Cadence.** New figures show at most [`FIGURES_EVERY_SECS`] apart.
//! - A click opens the Embroidery panel at its Sewing order ([`act`], at the frame's end: the bar is
//!   drawn with the app only read); the tooltip lists the figures.

use std::cell::{Cell, RefCell};

use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2};
use serde_json::{Value, json};

use super::numbers;
use super::stitch_preview::Freshness;
use super::text::fill;
use crate::VectorcraftApp;
use crate::theme::Tokens;

/// The shortest time between two sets of figures shown, seconds. An estimate: four readable
/// updates a second while a design is edited.
pub const FIGURES_EVERY_SECS: f64 = 0.25;
/// The counters' text size, points: as the bar's message.
const TEXT: f32 = 11.0;

/// The figures the counters show, read from `embroidery.stats`' reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Figures {
    pub stitches: u64,
    pub colors: u64,
    pub thread_changes: u64,
    pub trims: u64,
    pub jumps: u64,
    pub minutes: f64,
    pub speed: u64,
    pub size: Option<(f64, f64)>,
}

impl Figures {
    /// The figures of a stats reply; none when it isn't one.
    pub fn of(stats: &Value) -> Option<Self> {
        let n = |k: &str| stats[k].as_u64();
        Some(Self {
            stitches: n("stitches")?,
            colors: n("threadCount").unwrap_or(0),
            thread_changes: n("colorChanges").unwrap_or(0),
            trims: n("trims").unwrap_or(0),
            jumps: n("jumps").unwrap_or(0),
            minutes: stats["time"]["minutes"].as_f64().filter(|m| m.is_finite() && *m >= 0.0).unwrap_or(0.0),
            speed: stats["time"]["speed"].as_u64().unwrap_or(0),
            size: stats["size"]["width"].as_f64().zip(stats["size"]["height"].as_f64()),
        })
    }
}

/// What the counters show.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Hidden,
    Waiting,
    Planning(Option<f64>),
    Figures(Figures),
    Stale(Figures),
    Nothing,
    Failed(String),
}

impl State {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Waiting => "waiting",
            Self::Planning(_) => "planning",
            Self::Figures(_) => "figures",
            Self::Stale(_) => "stale",
            Self::Nothing => "nothing",
            Self::Failed(_) => "failed",
        }
    }
}

/// The wordings of the figures, longest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// "Embroidery: stitches · colours · thread changes · time".
    Named,
    /// "stitches · colours · thread changes · time".
    All,
    /// "stitches · colours · time".
    NoChanges,
    /// "stitches · time".
    Time,
    /// "stitches".
    Stitches,
}

impl Variant {
    pub const ALL: [Variant; 5] = [Variant::Named, Variant::All, Variant::NoChanges, Variant::Time, Variant::Stitches];
}

/// The figures in `variant`'s words.
pub fn words(f: &Figures, variant: Variant) -> String {
    let stitches = crate::etn!(f.stitches, "{count} stitch", "{count} stitches");
    let colors = crate::etn!(f.colors, "{count} colour", "{count} colours");
    let changes = crate::etn!(f.thread_changes, "{count} thread change", "{count} thread changes");
    let time = minutes_words(f.minutes);
    match variant {
        Variant::Named => fill(crate::et!("Embroidery: {figures}"), &[("figures", &[stitches, colors, changes, time].join(" · "))]),
        Variant::All => [stitches, colors, changes, time].join(" · "),
        Variant::NoChanges => [stitches, colors, time].join(" · "),
        Variant::Time => [stitches, time].join(" · "),
        Variant::Stitches => stitches,
    }
}

/// A sewing time as the Sewing order's footer writes it: "about 7 min", whole minutes up.
pub fn minutes_words(minutes: f64) -> String {
    let lang = crate::i18n::current();
    let whole = if minutes.is_finite() { minutes.max(0.0).ceil() } else { 0.0 };
    fill(crate::et!("about {value} min"), &[("value", &numbers::decimal(lang, whole, 0))])
}

/// The counters' state across frames: the figures shown and since when, the slot's width. Kept in
/// cells: the status bar draws them with the app only read.
#[derive(Default)]
pub struct Counters {
    /// The figures shown: of which document and plan, and when they were shown.
    shown: RefCell<Option<(u64, u64, Figures, f64)>>,
    /// The slot: its document, wording and widest width so far.
    slot: Cell<Option<(u64, Variant, f32)>>,
    /// What the last frame showed, for `ui.inspect`.
    last: RefCell<Option<(State, String)>>,
    /// The counters were clicked this frame: [`act`] opens the panel.
    clicked: Cell<bool>,
    /// The room the counters had in the last frame and their shortest wording's width, points (the
    /// tests hold them to show whenever it fits).
    #[cfg(test)]
    pub(crate) fit: Cell<Option<(f32, f32)>>,
}

impl Counters {
    /// Whether the status bar said something last frame (the canvas's notice then leaves planning
    /// and failure to it).
    pub fn says(&self) -> bool {
        self.last.borrow().as_ref().is_some_and(|(_, words)| !words.is_empty())
    }

    /// `ui.inspect`'s `stitchPreview.counters`: the state, the figures and the words drawn.
    pub fn inspect(&self) -> Value {
        let last = self.last.borrow();
        let Some((state, drawn)) = last.as_ref() else { return json!({ "state": "hidden" }) };
        let mut v = json!({ "state": state.id(), "shown": drawn });
        if let State::Figures(f) | State::Stale(f) = state {
            v["stitches"] = json!(f.stitches);
            v["colors"] = json!(f.colors);
            v["threadChanges"] = json!(f.thread_changes);
            v["trims"] = json!(f.trims);
            v["minutes"] = json!(f.minutes);
            v["speed"] = json!(f.speed);
        }
        if let State::Failed(e) = state {
            v["error"] = json!(e);
        }
        v
    }
}

/// What the counters show for the active document at time `now`, and in how many seconds to look
/// again when held back.
fn state(app: &VectorcraftApp, now: f64) -> (State, Option<f64>) {
    let preview = &app.embroidery.preview;
    let Some(uid) = app.session.active().map(|st| st.uid).filter(|_| preview.plan_wanted()) else { return (State::Hidden, None) };
    let freshness = preview.freshness(uid, now);
    let Some(shown) = preview.shown(uid) else {
        return match freshness {
            Freshness::Failed { error } => (State::Failed(error), None),
            Freshness::Pending { progress, notice: true } => (State::Planning(progress), None),
            _ => (State::Waiting, None),
        };
    };
    if let Freshness::Failed { error } = freshness {
        return (State::Failed(error), None);
    }
    if shown.planned.plan.is_empty() {
        return (State::Nothing, None);
    }
    let generation = shown.planned.generation;
    let latest = shown.reply(&super::stitch_preview::figures_query()).and_then(|r| r.result.as_ref().ok()).and_then(|v| Figures::of(v));
    let stale = matches!(freshness, Freshness::Stale { notice: true, .. });
    let mut shown = app.embroidery.counters.shown.borrow_mut();
    // New figures at most every so often: the ones shown stay until then.
    let mut wait = None;
    match (latest, &*shown) {
        (Some(f), Some((u, g, _, at))) if *u == uid && *g != generation => {
            let left = FIGURES_EVERY_SECS - (now - at);
            if left > 0.0 && left.is_finite() {
                wait = Some(left);
            } else {
                *shown = Some((uid, generation, f, now));
            }
        }
        (Some(f), Some((u, _, _, _))) if *u == uid => {
            if let Some(s) = shown.as_mut() {
                s.2 = f;
            }
        }
        (Some(f), _) => *shown = Some((uid, generation, f, now)),
        (None, _) => {}
    }
    match shown.as_ref().filter(|(u, ..)| *u == uid) {
        Some((_, _, f, _)) if stale => (State::Stale(f.clone()), wait),
        Some((_, _, f, _)) => (State::Figures(f.clone()), wait),
        None => (State::Waiting, wait),
    }
}

/// The counters' words in a state, longest first, with the colour to draw them in: one wording
/// for a state that isn't figures.
fn wordings(state: &State, t: &Tokens) -> Vec<(Option<Variant>, String, Color32)> {
    match state {
        State::Hidden | State::Waiting => Vec::new(),
        State::Planning(progress) => {
            let mut words = crate::et!("Planning stitches…").to_string();
            if let Some(p) = progress.filter(|p| p.is_finite()) {
                // Never 100 % before the plan is in.
                let whole = (p * 100.0).clamp(0.0, 99.0).floor() as u64;
                words = format!("{words} {}", numbers::percent(crate::i18n::current(), whole));
            }
            vec![(None, words, t.text_dim)]
        }
        State::Figures(f) => Variant::ALL.iter().map(|v| (Some(*v), words(f, *v), t.text)).collect(),
        // The figures of an earlier state, said to be updated: the canvas's notice leaves that to
        // the bar.
        State::Stale(f) => Variant::ALL.iter().map(|v| (Some(*v), format!("{} {}", crate::et!("Updating…"), words(f, *v)), t.text_dim)).collect(),
        State::Nothing => vec![(None, crate::et!("Nothing to sew").to_string(), t.text_dim)],
        State::Failed(_) => {
            vec![(None, format!("⚠ {}", crate::et!("Stitches can't be planned")), super::marks::text_colour(t, super::marks::Mark::Warning))]
        }
    }
}

/// The tooltip: the figures, what the time rests on, and what a click does; or why the stitches
/// can't be planned.
fn tooltip(state: &State) -> String {
    let lang = crate::i18n::current();
    let open = crate::et!("Click to open the Embroidery panel.");
    match state {
        State::Figures(f) | State::Stale(f) => {
            let mut lines = vec![
                [
                    crate::etn!(f.stitches, "{count} stitch", "{count} stitches"),
                    crate::etn!(f.jumps, "{count} jump", "{count} jumps"),
                    crate::etn!(f.trims, "{count} trim", "{count} trims"),
                ]
                .join(" · "),
                [
                    crate::etn!(f.colors, "{count} colour", "{count} colours"),
                    crate::etn!(f.thread_changes, "{count} thread change", "{count} thread changes"),
                ]
                .join(" · "),
            ];
            if let Some((w, h)) = f.size {
                lines.push(fill(
                    crate::et!("{width} × {height} mm"),
                    &[("width", &numbers::decimal(lang, w, 1)), ("height", &numbers::decimal(lang, h, 1))],
                ));
            }
            lines.push(fill(crate::et!("The time at {speed} stitches a minute, an estimate."), &[("speed", &numbers::count(lang, f.speed))]));
            if matches!(state, State::Stale(_)) {
                lines.push(crate::et!("These are the stitches before the last change; the new ones are being planned.").to_string());
            }
            lines.push(open.to_string());
            lines.join("\n")
        }
        State::Failed(e) => format!("{}\n{}", crate::i18n::msg(e), open),
        _ => open.to_string(),
    }
}

/// The counters at the right end of the status bar's `area`, before its timings, which give way
/// to them: `keep` is the room the bar keeps for its message and X/Y readout, `timings` the
/// timings' (each with its gap). → the area left for the rest of the bar, and whether the timings
/// keep their place.
pub fn show(app: &VectorcraftApp, ui: &mut Ui, area: Rect, keep: f32, timings: f32) -> (Rect, bool) {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let (state, wait) = state(app, now);
    if let Some(wait) = wait {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    let counters = &app.embroidery.counters;
    let choices = wordings(&state, &t);
    let uid = app.session.active().map_or(0, |st| st.uid);
    let hidden = |state: State| {
        counters.slot.set(None);
        counters.last.replace(Some((state, String::new())));
        (area, true)
    };
    if choices.is_empty() {
        return hidden(state);
    }
    // The room the counters have: the area less what the bar keeps and the separator beside them.
    let gap = ui.spacing().item_spacing.x;
    let separator = gap * 2.0 + 1.0;
    let rest = area.width() - keep - separator;
    let measure = |text: &str| ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(TEXT), Color32::WHITE).size().x;
    let widths: Vec<f32> = choices.iter().map(|(_, w, _)| measure(w)).collect();
    #[cfg(test)]
    counters.fit.set(widths.last().map(|w| (rest, *w)));
    let fits = |k: usize, room: f32| widths.get(k).is_some_and(|w| *w <= room);
    // With the timings, the three longest wordings; without, any.
    let with_timings = rest - timings;
    let pick = (0..choices.len().min(3))
        .find(|k| fits(*k, with_timings))
        .map(|k| (k, true))
        .or_else(|| (0..choices.len()).find(|k| fits(*k, rest)).map(|k| (k, false)));
    let Some((k, keep_timings)) = pick else { return hidden(state) };
    let Some((variant, text, colour)) = choices.get(k).cloned() else { return (area, true) };
    let natural = widths.get(k).copied().unwrap_or_default();
    // The slot keeps its widest width while the document and the wording stay.
    let width = match counters.slot.get() {
        Some((u, v, w)) if u == uid && Some(v) == variant => w.max(natural),
        _ => natural,
    };
    counters.slot.set(variant.map(|v| (uid, v, width)));
    let rect = Rect::from_min_max(pos2(area.right() - width, area.top()), area.right_bottom());
    let resp = ui.interact(rect, ui.id().with("embroidery-counters"), Sense::click());
    ui.painter().text(rect.right_center(), Align2::RIGHT_CENTER, &text, FontId::proportional(TEXT), colour);
    let said = format!("{text}. {}", crate::et!("Click to open the Embroidery panel."));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &said));
    let tip = tooltip(&state);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    if resp.clicked() {
        counters.clicked.set(true);
    }
    // The line between the counters and the rest of the bar, drawn as a separator draws it.
    ui.painter().vline(rect.left() - separator / 2.0, area.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
    counters.last.replace(Some((state, text)));
    (Rect::from_min_max(area.min, pos2(rect.left() - separator, area.bottom())), keep_timings)
}

/// A click on the counters this frame opens the Embroidery panel at its Sewing order.
pub fn act(app: &mut VectorcraftApp) {
    if app.embroidery.counters.clicked.take()
        && let Err(e) = app.run("ui.embroideryPanel", json!({ "tab": "order" }))
    {
        app.status(e);
    }
}

#[cfg(test)]
mod tests;
