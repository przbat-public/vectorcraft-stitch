//! The canvas's bounds of each open document measured only where an edit changed it (VectorCraft
//! Stitch): one [`VisualBounds`] per open document, by its uid, which upstream's
//! `VectorcraftApp::content_bounds` and `selection_bounds` and the Navigator's region measure
//! through (marked hooks). Switching between documents measures nothing again; a document's cache
//! goes in the frame after it closes ([`forget_closed`]), as the stitch preview lets a closed
//! document's plans go. Each holds an entry (80 bytes) for each object of its document, down to the
//! objects measured whole.

use std::collections::{HashMap, HashSet};

use vectorcraft_doc::visual_bounds::VisualBounds;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::Rect;

use crate::VectorcraftApp;

/// The visual bounds kept for each open document, by its uid.
#[derive(Default)]
pub struct Bounds {
    docs: HashMap<u64, VisualBounds>,
}

impl Bounds {
    /// The visual bounds of all of `doc`'s art (document `uid`), as [`Document::art_bounds`] gives
    /// them.
    pub fn art(&mut self, uid: u64, doc: &Document) -> Option<Rect> {
        self.docs.entry(uid).or_default().art_bounds(doc)
    }

    /// The visual bounds of `ids` in `doc` (document `uid`), as [`Document::bounds_of`] with
    /// `visual` gives them.
    pub fn of(&mut self, uid: u64, doc: &Document, ids: &[NodeId]) -> Option<Rect> {
        self.docs.entry(uid).or_default().bounds_of(doc, ids)
    }

    /// How many objects document `uid`'s cache has measured since it was made
    /// ([`VisualBounds::measured`]): for tests and gates.
    pub fn measured(&self, uid: u64) -> u64 {
        self.docs.get(&uid).map_or(0, VisualBounds::measured)
    }
}

/// Each frame: the caches of the documents closed since let go.
pub fn forget_closed(app: &mut VectorcraftApp) {
    let open: HashSet<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    app.embroidery.bounds.docs.retain(|uid, _| open.contains(uid));
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_engine::Session;

    use crate::{Services, VectorcraftApp};

    /// The active document's art with its artboards, and its selection, as the full walk measures
    /// them: what `content_bounds` and `selection_bounds` must answer.
    fn walked(app: &VectorcraftApp) -> (Option<vectorcraft_geom::Rect>, Option<vectorcraft_geom::Rect>) {
        let st = app.session.active().unwrap();
        let boards = st.doc.artboards.iter().map(|a| a.rect).reduce(|r, a| r.union(a));
        (vectorcraft_geom::union_opt(st.doc.art_bounds(), boards), st.doc.bounds_of(&st.selection.objects, true))
    }

    /// The canvas's scroll-bar span and selection frame come from the per-document cache and answer
    /// as the full walk does after an edit, an undo, a symbol made and renamed, another document and
    /// a document closed; an edit measures only what it changed, a switch back measures nothing,
    /// and a closed document's cache goes at the next frame.
    #[test]
    fn the_canvas_measures_through_the_cache_and_answers_as_the_full_walk() {
        let mut app = VectorcraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
        let first = app.session.active().unwrap().uid;
        let a = app.run("shape.rectangle", json!({"x": 10, "y": 10, "width": 40, "height": 40})).unwrap()["id"].as_u64().unwrap();
        app.run("shape.rectangle", json!({"x": 60, "y": 10, "width": 40, "height": 40})).unwrap();
        let check = |app: &mut VectorcraftApp, what: &str| {
            let want = walked(app);
            assert_eq!((app.content_bounds(), app.selection_bounds()), want, "{what}");
        };
        check(&mut app, "drawn");
        app.run("select.set", json!({"ids": [a]})).unwrap();
        check(&mut app, "one selected");
        let before = app.embroidery.bounds.measured(first);
        app.run("object.move", json!({"dx": 300, "dy": 0})).unwrap();
        check(&mut app, "moved");
        // Only what changed: the rectangle and its layer.
        assert_eq!(app.embroidery.bounds.measured(first) - before, 2, "an edit measures the rectangle and its layer");
        app.run("edit.undo", json!({})).unwrap();
        check(&mut app, "undone");
        app.run("symbol.new", json!({"name": "Mark"})).unwrap();
        check(&mut app, "the selection made a symbol");
        app.run("symbol.options", json!({"name": "Mark", "newName": "Sign"})).unwrap();
        check(&mut app, "the symbol renamed");
        let measured = app.embroidery.bounds.measured(first);
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        let second = app.session.active().unwrap().uid;
        app.run("shape.rectangle", json!({"x": 5, "y": 5, "width": 10, "height": 10})).unwrap();
        check(&mut app, "another document");
        // The engine's command closes it outright (the interface's asks first about unsaved edits).
        app.session.execute("file.close", &json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().uid, first, "the first document shown again");
        check(&mut app, "that document closed, the first one shown again");
        assert_eq!(app.embroidery.bounds.measured(first), measured, "switching back measured nothing again");
        super::forget_closed(&mut app);
        assert!(!app.embroidery.bounds.docs.contains_key(&second), "the closed document's cache is let go");
        assert!(app.embroidery.bounds.docs.contains_key(&first));
    }
}
