//! The stitch preview's view setting, one per open document ([`StitchView`]): whether the canvas
//! shows the document's stitches instead of its art, and how. View state like Show Transparency
//! Grid: per document, neither saved nor undoable, never marking the document modified and never
//! written to the journal; dropped when the document closes, kept through Revert (the document
//! keeps its uid). The commands set it (`view.stitchPreview` and its options), so menus, shortcuts,
//! the control channel and MCP reach the same state.
//!
//! [`layers`] says, as a pure function the canvas and its tests share, what the canvas draws for
//! a setting: the stitches or the art, the art faint under the stitches, outlines over them.
//!
//! The preview plans with the options of the document's last embroidery export, else those of the
//! last embroidery export of any document, else the defaults ([`OptionsSource`], the project
//! owner's choice): remembered while the app runs, never saved in the file.

use serde_json::{Value, json};

use super::*;

/// The stitch preview's setting for one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct StitchView {
    /// The canvas shows the stitches (the Art | Stitches switch, View › Stitch Preview).
    pub on: bool,
    /// Jumps and cuts are marked on the stitches (on by default, the project owner's choice).
    pub jumps: bool,
    /// Needle points are marked, where they stand apart on screen (off by default).
    pub needle_points: bool,
    /// The art is drawn faint under the stitches (off by default, the project owner's choice: the
    /// gaps between rows stay visible).
    pub artwork: bool,
}

impl Default for StitchView {
    fn default() -> Self {
        Self { on: false, jumps: true, needle_points: false, artwork: false }
    }
}

/// Why the stitches aren't shown although the preview is on: an edit mode whose art isn't what is
/// sewn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paused {
    /// An opacity mask is being edited: its editing layer is no art, and the export leaves it out.
    OpacityMask,
    /// A pattern tile is being edited.
    Pattern,
}

impl Paused {
    /// The id the replies give it.
    pub fn id(self) -> &'static str {
        match self {
            Self::OpacityMask => "opacityMask",
            Self::Pattern => "pattern",
        }
    }
}

/// What pauses the stitch preview in `doc`, if anything.
pub fn paused(doc: &Document) -> Option<Paused> {
    if doc.mask_edit.is_some() {
        Some(Paused::OpacityMask)
    } else if doc.pattern_edit.is_some() {
        Some(Paused::Pattern)
    } else {
        None
    }
}

/// What the canvas draws, layer by layer, bottom to top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    /// The art as usual (Preview or Outline): the stitches aren't shown.
    pub art: bool,
    /// The fabric under the stitches (where there are artboards, and under the design's box).
    pub fabric: bool,
    /// The art drawn faint under the stitches.
    pub faint_art: bool,
    /// The stitches.
    pub stitches: bool,
    /// The art's outlines over the stitches (View › Outline with the stitches shown).
    pub outlines: bool,
}

/// What the canvas draws for `view`, `paused` (an edit mode), and whether View › Outline is on.
/// Templates and guides are drawn by the canvas as always (the art's layer draws templates).
pub fn layers(view: StitchView, paused: Option<Paused>, outline: bool) -> Layers {
    if !view.on || paused.is_some() {
        return Layers { art: true, fabric: false, faint_art: false, stitches: false, outlines: false };
    }
    Layers { art: false, fabric: true, faint_art: view.artwork && !outline, stitches: true, outlines: outline }
}

/// Where the options the stitch preview plans with come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsSource {
    /// The document's last embroidery export.
    Document,
    /// The last embroidery export of another document, while the app runs.
    App,
    /// No embroidery export yet: the defaults.
    Defaults,
}

impl OptionsSource {
    /// The id the replies give it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::App => "app",
            Self::Defaults => "defaults",
        }
    }
}

/// Note an export of the active document in `format` with `params` (`document.export`'s), once
/// it is written: when it is an embroidery format, its options are the ones the stitch preview
/// plans with from now on ([`note_export_of`]).
pub fn note_export(s: &mut Session, format: &str, params: &Value) {
    if let Some(uid) = s.active().map(|st| st.uid) {
        note_export_of(s, uid, format, params);
    }
}

/// Note an export of open document `uid` in `format` with `params` (the app's background export's,
/// once written, whichever document is active then): when it is an embroidery format, its options
/// are the ones that document's stitch preview plans with from now on. Params the export would
/// refuse note nothing, nor a document closed meanwhile.
pub fn note_export_of(s: &mut Session, uid: u64, format: &str, params: &Value) {
    if !crate::cmd::fileio::embroidery::is_format(format) || !s.documents().iter().any(|d| d.uid == uid) {
        return;
    }
    let Ok(opts) = crate::cmd::fileio::embroidery::options(params) else { return };
    s.embroidery_plans.note_export(uid, opts);
}

impl Session {
    /// The options document `uid`'s stitch preview plans with, and where they come from.
    pub fn stitch_preview_options(&self, uid: u64) -> (Options, OptionsSource) {
        match self.embroidery_plans.exported(uid) {
            (Some(own), _) => (own.clone(), OptionsSource::Document),
            (None, Some(last)) => (last.clone(), OptionsSource::App),
            (None, None) => (Options::default(), OptionsSource::Defaults),
        }
    }

    /// The active document's stitch view (the default with no document).
    pub fn stitch_view(&self) -> StitchView {
        self.active().map_or_else(StitchView::default, |st| self.stitch_view_of(st.uid))
    }

    /// Document `uid`'s stitch view.
    pub fn stitch_view_of(&self, uid: u64) -> StitchView {
        self.embroidery_plans.view(uid)
    }
}

/// The view commands.
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "view.stitchPreview",
            "Stitch Preview",
            ["View"],
            Some("Cmd+E"),
            "{on?: bool (default: toggle)} show the active document's design as the stitches it is sewn with instead of its art (planned as the read commands plan it); each open document has its own setting, view state that is neither saved nor undoable; the canvas, ui.screenshot and ui.render {stitchPreview} show it, exports never do; while an opacity mask or a pattern is edited the art shows → {on, jumps, needlePoints, artwork, shown: \"stitches\"|\"art\", pausedBy: null|\"opacityMask\"|\"pattern\", options: {…the embroidery options it plans with}, optionsFrom: \"document\" (this document's last embroidery export)|\"app\" (another document's, while the app runs)|\"defaults\"}",
            has_doc,
            |s, p| set(s, p, "view.stitchPreview", |v, on| v.on = on, |v| v.on)
        ),
        cmd!(
            query "view.stitchPreview.jumps",
            "Show Jumps and Cuts",
            ["View", "Stitch Preview Options"],
            None,
            "{on?: bool (default: toggle)} the stitch preview marks its jumps (moves with the needle up: two-tone dashes where the thread was cut, a thin line where it lies) and a × at each cut (on by default) → {as view.stitchPreview}",
            has_doc,
            |s, p| set(s, p, "view.stitchPreview.jumps", |v, on| v.jumps = on, |v| v.jumps)
        ),
        cmd!(
            query "view.stitchPreview.needlePoints",
            "Show Needle Points",
            ["View", "Stitch Preview Options"],
            None,
            "{on?: bool (default: toggle)} the stitch preview marks each needle point, from the zoom at which they stand apart → {as view.stitchPreview}",
            has_doc,
            |s, p| set(s, p, "view.stitchPreview.needlePoints", |v, on| v.needle_points = on, |v| v.needle_points)
        ),
        cmd!(
            query "view.stitchPreview.artwork",
            "Show Artwork Under Stitches",
            ["View", "Stitch Preview Options"],
            None,
            "{on?: bool (default: toggle)} the stitch preview draws the art faint under the stitches (off by default, so gaps between rows stay visible) → {as view.stitchPreview}",
            has_doc,
            |s, p| set(s, p, "view.stitchPreview.artwork", |v, on| v.artwork = on, |v| v.artwork)
        ),
    ]
}

/// Set (or toggle, with `on` left out or null) the active document's stitch view's switch that
/// `put` sets and `get` reads → the whole setting.
fn set(s: &mut Session, p: &Value, cmd: &str, put: fn(&mut StitchView, bool), get: fn(&StitchView) -> bool) -> Result<Value> {
    let o = object(p, cmd)?;
    if let Some(k) = o.keys().find(|k| *k != "on") {
        return Err(bad(cmd, format!("unknown key `{k}`: the only one is on")));
    }
    let on = match o.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad(cmd, format!("`on` {v}: true or false")))?),
    };
    let st = s.doc()?;
    let uid = st.uid;
    let paused = paused(&st.doc);
    let mut view = s.embroidery_plans.view(uid);
    let on = on.unwrap_or(!get(&view));
    put(&mut view, on);
    s.embroidery_plans.set_view(uid, view);
    Ok(view_json(view, paused, s.stitch_preview_options(uid)))
}

/// A stitch view as the commands answer with it, with the options the preview plans with and
/// where they come from.
pub fn view_json(view: StitchView, paused: Option<Paused>, (opts, from): (Options, OptionsSource)) -> Value {
    let shown = if layers(view, paused, false).stitches { "stitches" } else { "art" };
    json!({
        "on": view.on,
        "jumps": view.jumps,
        "needlePoints": view.needle_points,
        "artwork": view.artwork,
        "shown": shown,
        "pausedBy": paused.map(Paused::id),
        "options": opts.to_json(),
        "optionsFrom": from.id(),
    })
}
