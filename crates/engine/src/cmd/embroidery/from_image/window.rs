//! What the image window asks of the engine: a request of the settings it shows
//! ([`Session::digitize_window_request`]), a pipeline that keeps digitizing's stages between runs
//! ([`Session::digitize_pipeline`]), and Create of what a run made
//! ([`Session::apply_digitize_window`]), which is the command's Create: the same objects in place
//! of the image, as one undo step, with the window's palette and document settings (its fabric,
//! fill angle and joining one thread's objects) made the document's in that step.

use std::sync::Arc;

use serde_json::Value;
use vectorcraft_doc::embroidery::EmbroiderySetup;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_stitch::thread::Palette;

use super::command::{NOTHING_TO_MAKE, created_json, image_of, put_in_place};
use super::pipeline::{Outputs, Pipeline, WindowRequest};
use super::settings::{C, Settings};
use crate::cmd::bad;
use crate::cmd::embroidery::setup::DocumentSettings;
use crate::{Result, Session};

/// The document settings the window changes, as `embroidery.setup` names them: the fabric, the fill
/// angle and joining one thread's objects (the project owner's choice of 2026-11-12: changed in the
/// window, made the document's on Create, as its palette is).
const DOCUMENT_SETTINGS: [&str; 3] = ["fabric", "fillAngle", "joinSameColor"];

impl Session {
    /// What a window digitizes image `id` (an image or an Image Trace object) with: the settings
    /// `params` gives (the command's, as `embroidery.digitizeImage` takes them, but `id`), matched to
    /// `palette` (`pec` or `jef`; `None`: the document's), its objects sewn with the document's
    /// settings and `document`'s changes to them ([`DOCUMENT_SETTINGS`], as `embroidery.setup` takes
    /// them; null for none). Refused as the command refuses (no image, a locked one, a missing
    /// linked file, Global Edit on, a setting out of its range), and a document setting as
    /// `embroidery.setup` refuses it. Only the settings given, the defaults for the rest: never the
    /// ones a live object keeps, which the command starts from, since the window's resets (Start
    /// Over, Reset All, a ↺, Back to Auto) take a field out to mean its default.
    pub fn digitize_window_request(&self, id: NodeId, params: &Value, palette: Option<&str>, document: &Value) -> Result<WindowRequest> {
        let mut o = super::super::object(params, C)?.into_owned();
        o.insert("id".into(), Value::from(id.0));
        let st = self.doc()?;
        let palette = match palette {
            None => super::super::thread_palette(&st.doc),
            Some(p) => Palette::from_id(p).ok_or_else(|| bad(C, format!("palette {p:?}: pec or jef")))?,
        };
        let settings = Settings::from_params(&o, palette)?;
        let setup = window_setup(&st.doc, document)?;
        let (target, image, bytes) = image_of(self, &o)?;
        Ok(WindowRequest { uid: st.uid, original: st.doc.clone(), target, image, bytes, settings, palette, setup })
    }

    /// A pipeline of digitizing's stages kept between runs, with a planning service of its own that
    /// shares the open documents' bound on plans.
    pub fn digitize_pipeline(&self) -> Pipeline {
        Pipeline::new(self.detached_planning())
    }

    /// What `outputs` made of `req` put in place as Create puts it (`live`: kept a live object, Keep
    /// as Live Object), as one undo step "Digitize Image" (the new group selected, the window's
    /// palette and document settings made the document's in the same step):
    /// the reply the command's Create gives (its areas listed none), with what was made
    /// (`created`). Refused when the document isn't the one the run was made of any more, nothing
    /// applied; or when there is nothing to make.
    pub fn apply_digitize_window(&mut self, req: &WindowRequest, outputs: &Outputs, live: bool) -> Result<Value> {
        let mut reply = outputs.reply(req).ok_or_else(|| bad(C, "the image isn't digitized yet: wait for it"))?;
        let (of, doc, made) = outputs.made_for_create().ok_or_else(|| bad(C, NOTHING_TO_MAKE))?;
        if !Arc::ptr_eq(of, &req.original) {
            return Err(bad(C, "the document changed while the image was digitized: nothing was made"));
        }
        let mut doc = Arc::try_unwrap(doc).unwrap_or_else(|shared| (*shared).clone());
        let made = if live {
            let was = req.original.node(req.target).filter(|n| super::live::live_digitizing(n).is_some()).cloned();
            super::live::into_live(&mut doc, &made, was, &req.settings)?
        } else {
            made
        };
        put_in_place(self, req.uid, &req.original, doc, made.group)?;
        // The run's reply is its dry run's; the objects are made now, so it is no dry run any more.
        reply["dryRun"] = Value::Bool(false);
        reply["created"] = created_json(&made);
        Ok(reply)
    }
}

impl WindowRequest {
    /// The document's embroidery settings as the window's objects are planned with them (its
    /// palette and its changes in place), each with where it comes from, as `embroidery.settings`
    /// answers with them: what the window's Stitches step shows.
    pub fn document_settings(&self) -> DocumentSettings {
        let mut d = Document::clone(&self.original);
        d.embroidery = self.setup.clone();
        super::pipeline::with_palette(&mut d, self.palette);
        super::super::setup::document_settings(&d)
    }
}

/// `d`'s embroidery settings with the window's `document` settings applied, each checked as
/// `embroidery.setup` checks it; null, the document's own. Only [`DOCUMENT_SETTINGS`] are taken.
fn window_setup(d: &Document, document: &Value) -> Result<Option<Box<EmbroiderySetup>>> {
    match document {
        Value::Null => Ok(d.embroidery.clone()),
        Value::Object(o) if o.keys().all(|k| DOCUMENT_SETTINGS.contains(&k.as_str())) => super::super::setup::setup_with(d, document),
        _ => Err(bad(C, "the window's document settings are fabric, fillAngle and joinSameColor, as embroidery.setup takes them")),
    }
}
