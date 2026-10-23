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
//!   timings give way first, then the counters shorten, then they hide. The X/Y readout and the last
//!   message keep their room ([`MIN_MESSAGE_ROOM`] for the message). The slot keeps its widest width
//!   while its wording stays, so that changing digits move nothing to its left.
//! - **Cadence.** New figures show at most [`FIGURES_EVERY_SECS`] apart.
//! - A click opens the Embroidery panel at its Sewing order; the tooltip lists the figures.

use egui::{Align2, Color32, FontId, Sense, Ui, vec2};
use serde_json::{Value, json};

use super::numbers;
use super::stitch_preview::Freshness;
use super::text::fill;
use crate::VectorcraftApp;
use crate::theme::Tokens;

/// The least room kept for the status bar's last message (or background job) beside the counters,
/// points. An estimate: about ten characters at 11 points; the whole message shows on hover.
pub const MIN_MESSAGE_ROOM: f32 = 60.0;
/// The shortest time between two sets of figures shown, seconds. An estimate: four readable
/// updates a second while a design is edited.
pub const FIGURES_EVERY_SECS: f64 = 0.25;
/// The counters' text size, points: as the bar's message.
const TEXT: f32 = 11.0;
/// The X/Y readout's and the timings' size, points: as the bar draws them.
const READOUT: f32 = 10.5;

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
    fill(crate::et!("about {value}"), &[("value", &format!("{} min", numbers::decimal(lang, whole, 0)))])
}

/// The counters' state across frames: the figures shown and since when, the slot's width.
#[derive(Default)]
pub struct Counters {
    /// The figures shown: of which document and plan, and when they were shown.
    shown: Option<(u64, u64, Figures, f64)>,
    /// The slot: its document, wording and widest width so far.
    slot: Option<(u64, Variant, f32)>,
    /// What the last frame showed, for `ui.inspect`.
    last: Option<(State, String)>,
}

impl Counters {
    /// Whether the status bar said something last frame (the canvas's notice then leaves planning
    /// and failure to it).
    pub fn says(&self) -> bool {
        self.last.as_ref().is_some_and(|(_, words)| !words.is_empty())
    }

    /// `ui.inspect`'s `stitchPreview.counters`: the state, the figures and the words drawn.
    pub fn inspect(&self) -> Value {
        let Some((state, drawn)) = &self.last else { return json!({ "state": "hidden" }) };
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
fn state(app: &mut VectorcraftApp, now: f64) -> (State, Option<f64>) {
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
    let counters = &mut app.embroidery.counters;
    // New figures at most every so often: the ones shown stay until then.
    let mut wait = None;
    match (latest, &counters.shown) {
        (Some(f), Some((u, g, _, at))) if *u == uid && *g != generation => {
            let left = FIGURES_EVERY_SECS - (now - at);
            if left > 0.0 && left.is_finite() {
                wait = Some(left);
            } else {
                counters.shown = Some((uid, generation, f, now));
            }
        }
        (Some(f), Some((u, _, _, _))) if *u == uid => {
            if let Some(s) = counters.shown.as_mut() {
                s.2 = f;
            }
        }
        (Some(f), _) => counters.shown = Some((uid, generation, f, now)),
        (None, _) => {}
    }
    match counters.shown.as_ref().filter(|(u, ..)| *u == uid) {
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
                words = format!("{words} {}%", (p * 100.0).clamp(0.0, 99.0).floor());
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

/// The counters in the status bar's right-to-left group, drawn first (rightmost): the room left
/// is shared with the timings `timings`, the X/Y readout and the last message. → whether the
/// timings keep their place.
pub fn show(app: &mut VectorcraftApp, ui: &mut Ui, timings: &egui::RichText) -> bool {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let (state, wait) = state(app, now);
    if let Some(wait) = wait {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    let choices = wordings(&state, &t);
    let uid = app.session.active().map_or(0, |st| st.uid);
    if choices.is_empty() {
        app.embroidery.counters.slot = None;
        app.embroidery.counters.last = Some((state, String::new()));
        return true;
    }
    // The room the group has, less what the other items keep.
    let gap = ui.spacing().item_spacing.x;
    let measure = |text: &str, font: FontId| ui.painter().layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x;
    let timings_w =
        egui::WidgetText::from(timings.clone()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x;
    let readout_w = app.hover_doc.map_or(0.0, |p| {
        let u = app.session.general_unit();
        measure(&format!("X: {}  Y: {}", u.readout(p.x), u.readout(p.y)), crate::theme::mono(READOUT)) + gap
    });
    let message = !app.ui.status.is_empty() || !app.background.jobs.is_empty() || !app.opening.pending.is_empty();
    let separator = ui.spacing().item_spacing.x * 2.0 + 1.0;
    let rest = ui.available_width() - readout_w - if message { MIN_MESSAGE_ROOM + gap } else { 0.0 } - separator;
    let widths: Vec<f32> = choices.iter().map(|(_, w, _)| measure(w, FontId::proportional(TEXT))).collect();
    let fits = |k: usize, room: f32| widths.get(k).is_some_and(|w| *w <= room);
    // With the timings, the three longest wordings; without, any.
    let with_timings = rest - timings_w - gap;
    let pick = (0..choices.len().min(3))
        .find(|k| fits(*k, with_timings))
        .map(|k| (k, true))
        .or_else(|| (0..choices.len()).find(|k| fits(*k, rest)).map(|k| (k, false)));
    let Some((k, keep_timings)) = pick else {
        app.embroidery.counters.slot = None;
        app.embroidery.counters.last = Some((state, String::new()));
        return true;
    };
    let Some((variant, text, colour)) = choices.get(k).cloned() else { return true };
    let natural = widths.get(k).copied().unwrap_or_default();
    // The slot keeps its widest width while the document and the wording stay.
    let width = match app.embroidery.counters.slot {
        Some((u, v, w)) if u == uid && Some(v) == variant => w.max(natural),
        _ => natural,
    };
    app.embroidery.counters.slot = variant.map(|v| (uid, v, width));
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ui.available_height()), Sense::click());
    ui.painter().text(rect.right_center(), Align2::RIGHT_CENTER, &text, FontId::proportional(TEXT), colour);
    let said = format!("{text}. {}", crate::et!("Click to open the Embroidery panel."));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &said));
    let tip = tooltip(&state);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    if resp.clicked()
        && let Err(e) = app.run("ui.embroideryPanel", json!({ "tab": "order" }))
    {
        app.status(e);
    }
    ui.separator();
    app.embroidery.counters.last = Some((state, text));
    keep_timings
}

#[cfg(test)]
mod tests;
