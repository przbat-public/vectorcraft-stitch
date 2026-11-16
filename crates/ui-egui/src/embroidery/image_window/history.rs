//! Undo inside the image window (the project owner's choice, 2026-11-16, „Tak, ograniczona historia
//! (polecam)”): Cmd/Ctrl+Z undoes and Shift+Cmd/Ctrl+Z redoes the window's settings and its changes
//! to single areas, the last [`MAX_UNDO_STEPS`] steps. The window is a dialog, which the document's
//! Undo doesn't reach; the document's history is untouched (Create stays one step of it).
//!
//! A step is what the settings came to when they last changed with no pointer button held: a
//! slider dragged, or a crop drawn, is one step. What the window shows (its step, its view, its
//! zoom) is no setting and isn't undone.

use std::collections::VecDeque;

use serde_json::{Map, Value};

/// How many steps the window's history keeps: the project owner's choice (2026-11-16).
pub(crate) const MAX_UNDO_STEPS: usize = 100;

/// Whether field `key` is one a step holds: the engine's settings, the palette and the document
/// settings the window changes.
fn is_setting(key: &str) -> bool {
    super::SETTING_FIELDS.contains(&key) || key == super::PALETTE || super::stitches_step::DOCUMENT_FIELDS.contains(&key)
}

/// The settings `fields` hold.
fn settings(fields: &Map<String, Value>) -> Map<String, Value> {
    fields.iter().filter(|(k, _)| is_setting(k)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// `fields` with the settings `to` holds, those it lacks taken out.
fn restore(fields: &mut Map<String, Value>, to: &Map<String, Value>) {
    fields.retain(|k, _| !is_setting(k));
    fields.extend(to.iter().map(|(k, v)| (k.clone(), v.clone())));
}

/// The window's history: the settings before each step (the newest last), the steps undone (to
/// redo), and the settings as last noted.
#[derive(Default)]
pub(crate) struct History {
    past: VecDeque<Map<String, Value>>,
    future: Vec<Map<String, Value>>,
    last: Option<Map<String, Value>>,
}

impl History {
    /// The window's `fields` as a frame left them: a step when their settings differ from the last
    /// noted, unless `held` (a pointer button down: a drag goes on), when the step waits for it.
    pub(crate) fn note(&mut self, fields: &Map<String, Value>, held: bool) {
        let now = settings(fields);
        match self.last.take() {
            Some(last) if last != now && !held => {
                self.past.push_back(last);
                while self.past.len() > MAX_UNDO_STEPS {
                    self.past.pop_front();
                }
                self.future.clear();
                self.last = Some(now);
            }
            Some(last) => self.last = Some(last),
            None => self.last = Some(now),
        }
    }

    /// Cmd/Ctrl+Z: `fields` given the settings they had before the last step; `false` when there
    /// is none.
    pub(crate) fn undo(&mut self, fields: &mut Map<String, Value>) -> bool {
        let Some(before) = self.past.pop_back() else { return false };
        self.future.push(settings(fields));
        restore(fields, &before);
        self.last = Some(before);
        true
    }

    /// Shift+Cmd/Ctrl+Z: the last step undone done again; `false` when there is none.
    pub(crate) fn redo(&mut self, fields: &mut Map<String, Value>) -> bool {
        let Some(after) = self.future.pop() else { return false };
        self.past.push_back(settings(fields));
        restore(fields, &after);
        self.last = Some(after);
        true
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fields(colors: u64) -> Map<String, Value> {
        json!({"colors": colors, "step": 2, "view": "compare"}).as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn a_step_is_undone_and_redone_leaving_what_the_window_shows() {
        let mut h = History::default();
        h.note(&fields(6), false);
        h.note(&fields(4), false);
        let mut f = fields(4);
        f.insert("view".into(), json!("stitches"));
        assert!(h.undo(&mut f));
        assert_eq!((f["colors"].clone(), f["view"].clone()), (json!(6), json!("stitches")), "the setting back, the view as it is");
        assert!(h.redo(&mut f));
        assert_eq!(f["colors"], json!(4));
        assert!(!h.redo(&mut f), "nothing more to redo");
    }

    #[test]
    fn a_drag_is_one_step() {
        let mut h = History::default();
        h.note(&fields(6), false);
        for c in [5, 4, 3] {
            h.note(&fields(c), true);
        }
        h.note(&fields(3), false);
        let mut f = fields(3);
        assert!(h.undo(&mut f));
        assert_eq!(f["colors"], json!(6), "back past the whole drag");
        assert!(!h.undo(&mut f));
    }

    #[test]
    fn the_history_keeps_its_last_steps() {
        let mut h = History::default();
        for c in 0..=(MAX_UNDO_STEPS as u64 + 5) {
            h.note(&fields(c), false);
        }
        let mut f = fields(MAX_UNDO_STEPS as u64 + 5);
        let mut undone = 0;
        while h.undo(&mut f) {
            undone += 1;
        }
        assert_eq!((undone, f["colors"].clone()), (MAX_UNDO_STEPS, json!(5)), "the oldest steps dropped");
    }

    #[test]
    fn a_new_step_drops_what_could_be_redone() {
        let mut h = History::default();
        h.note(&fields(6), false);
        h.note(&fields(4), false);
        let mut f = fields(4);
        h.undo(&mut f);
        h.note(&fields(8), false);
        let mut f = fields(8);
        assert!(!h.redo(&mut f));
    }
}
