//! What the image window asks of the engine: a request of the settings it shows
//! ([`Session::digitize_window_request`]), a pipeline that keeps digitizing's stages between runs
//! ([`Session::digitize_pipeline`]), and Create of what a run made
//! ([`Session::apply_digitize_window`]), which is the command's Create: the same objects in place
//! of the image, as one undo step, with the window's palette made the document's in that step.

use std::sync::Arc;

use serde_json::Value;
use vectorcraft_doc::NodeId;
use vectorcraft_stitch::thread::Palette;

use super::command::{NOTHING_TO_MAKE, created_json, image_of, put_in_place};
use super::pipeline::{Outputs, Pipeline, WindowRequest};
use super::settings::{C, Settings};
use crate::cmd::bad;
use crate::{Result, Session};

impl Session {
    /// What a window digitizes image `id` (an image or an Image Trace object) with: the settings
    /// `params` gives (the command's, as `embroidery.digitizeImage` takes them, but `id`), matched to
    /// `palette` (`pec` or `jef`; `None`: the document's). Refused as the command refuses (no image,
    /// a locked one, a missing linked file, Global Edit on, a setting out of its range).
    pub fn digitize_window_request(&self, id: NodeId, params: &Value, palette: Option<&str>) -> Result<WindowRequest> {
        let mut o = super::super::object(params, C)?.into_owned();
        o.insert("id".into(), Value::from(id.0));
        let st = self.doc()?;
        let palette = match palette {
            None => super::super::thread_palette(&st.doc),
            Some(p) => Palette::from_id(p).ok_or_else(|| bad(C, format!("palette {p:?}: pec or jef")))?,
        };
        let settings = Settings::from_params(&o, palette)?;
        let (target, image, bytes) = image_of(self, &o)?;
        Ok(WindowRequest { uid: st.uid, original: st.doc.clone(), target, image, bytes, settings, palette })
    }

    /// A pipeline of digitizing's stages kept between runs, with a planning service of its own that
    /// shares the open documents' bound on plans.
    pub fn digitize_pipeline(&self) -> Pipeline {
        Pipeline::new(self.detached_planning())
    }

    /// What `outputs` made of `req` put in place as Create puts it, as one undo step "Digitize
    /// Image" (the new group selected, the window's palette made the document's in the same step):
    /// the command's reply with what was made (`created`). Refused when the document isn't the one
    /// the run was made of any more, nothing applied; or when there is nothing to make.
    pub fn apply_digitize_window(&mut self, req: &WindowRequest, outputs: &Outputs) -> Result<Value> {
        let mut reply = outputs.reply(req).ok_or_else(|| bad(C, "the image isn't digitized yet: wait for it"))?;
        let (of, doc, made) = outputs.made_for_create().ok_or_else(|| bad(C, NOTHING_TO_MAKE))?;
        if !Arc::ptr_eq(of, &req.original) {
            return Err(bad(C, "the document changed while the image was digitized: nothing was made"));
        }
        let doc = Arc::try_unwrap(doc).unwrap_or_else(|shared| (*shared).clone());
        put_in_place(self, req.uid, &req.original, doc, made)?;
        reply["created"] = created_json(made);
        Ok(reply)
    }
}
