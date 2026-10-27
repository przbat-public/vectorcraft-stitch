//! How much memory embroidery settings hold, for the undo history's budget
//! (`Document::retained_bytes`, `crate::mem`): the document's settings and an object's, with what
//! they keep that a newer version wrote. Upstream's estimator calls these through one-line hooks
//! (FORK.md › One-line hooks).

use std::mem::size_of;

use crate::mem::{ALLOC, json_bytes, map_bytes};

use super::{Embroidery, EmbroiderySetup, FabricValues, StitchValues, ThreadPalette};

/// The document's embroidery settings: their box, and what they keep that a newer version wrote (a
/// palette, keys and settings this version can't read).
pub(crate) fn setup_bytes(e: &EmbroiderySetup) -> usize {
    let palette = match &e.palette {
        Some(ThreadPalette::Unknown(v)) => json_bytes(v),
        _ => 0,
    };
    let text = |t: &Option<String>| t.as_ref().map_or(0, String::capacity);
    let stitch = |s: &StitchValues| text(&s.underlay) + map_bytes(&s.extra);
    let values = |v: &FabricValues| {
        v.tatami.as_ref().map_or(0, stitch)
            + v.satin.as_ref().map_or(0, stitch)
            + text(&v.stabilizer)
            + text(&v.topping)
            + text(&v.needle_point)
            + map_bytes(&v.extra)
    };
    let fabric = e.fabric.as_deref().map_or(0, |f| {
        ALLOC
            + size_of::<super::FabricSettings>()
            + values(&f.values)
            + f.preset.as_ref().map_or(0, |p| p.id.capacity() + values(&p.values) + map_bytes(&p.extra))
            + map_bytes(&f.extra)
    });
    let machine = e.machine.as_deref().map_or(0, |m| ALLOC + size_of::<super::MachineSettings>() + map_bytes(&m.extra));
    ALLOC + size_of::<EmbroiderySetup>() + palette + fabric + machine + map_bytes(&e.extra) + e.unread.as_ref().map_or(0, json_bytes)
}

/// What an object's embroidery holds beyond its box: its parts' settings (their boxes, ids and
/// what a newer version wrote in them), and the keys and values this version can't read.
pub(crate) fn object_bytes(e: &Embroidery) -> usize {
    let thread = |t: &super::ThreadRecord| ALLOC + t.catalogue.len() + t.description.len() + t.brand.len() + map_bytes(&t.extra);
    let run = |r: &super::StitchRun| {
        map_bytes(&r.extra)
            + r.thread.as_ref().map_or(0, thread)
            + r.trims.capacity() * size_of::<usize>()
            + r.stops.capacity() * size_of::<(usize, usize)>()
    };
    // The original's node, as the file saved it: shared by the copies of the object, counted with
    // each (an estimate errs high rather than low).
    let original = |o: &super::Original| ALLOC + json_bytes(&o.node) + map_bytes(&o.extra);
    let text = |t: &Option<String>| t.as_ref().map_or(0, String::capacity);
    let part = |p: &super::PartSettings| {
        ALLOC
            + size_of::<super::PartSettings>()
            + text(&p.stitch)
            + text(&p.underlay)
            + map_bytes(&p.extra)
            + p.points.as_ref().map_or(0, |q| map_bytes(&q.extra))
    };
    map_bytes(&e.extra)
        + e.stitches.as_ref().map_or(0, run)
        + e.original.as_ref().map_or(0, original)
        + e.unread.as_ref().map_or(0, json_bytes)
        + e.fill.as_deref().map_or(0, part)
        + e.stroke.as_deref().map_or(0, part)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use vectorcraft_geom::{kurbo, shapes};

    use crate::embroidery::{Embroidery, EmbroiderySetup, PartKey, PartKind, PartValue};
    use crate::{Appearance, Document, Node};

    /// A document of one rectangle.
    fn doc_with_a_rect() -> Document {
        let mut d = Document::new(1000.0, 1000.0);
        let layer = d.layers[0].id;
        let id = d.alloc_id();
        let r = shapes::rectangle(kurbo::Rect::new(0.0, 0.0, 10.0, 10.0));
        d.insert(Some(layer), usize::MAX, Node::path(id, r, Appearance::default_art())).unwrap();
        d
    }

    /// The document's embroidery settings count, with what a newer version wrote in them.
    #[test]
    fn embroidery_settings_count() {
        let before = doc_with_a_rect();
        let mut after = before.clone();
        let same = after.retained_bytes(&before);
        let mut setup = EmbroiderySetup::default();
        setup.extra.insert("notes".into(), Value::String("x".repeat(10_000)));
        after.embroidery = Some(Box::new(setup));
        assert!(after.retained_bytes(&before) > same + 10_000);
    }

    /// The settings an object keeps for its parts count, one box each and what is in them.
    #[test]
    fn embroidery_part_settings_count() {
        let before = doc_with_a_rect();
        let mut after = before.clone();
        let same = after.retained_bytes(&before);
        let id = after.layers[0].children().and_then(|c| c.first()).map(|n| n.id).unwrap();
        let mut e = Embroidery::default();
        for kind in PartKind::ALL {
            e.edit_part(kind, |p| p.set(PartKey::Stitch, Some(PartValue::Id("x".repeat(1000)))));
        }
        after.node_mut(id).unwrap().extras_mut().embroidery = Some(Box::new(e));
        let part = size_of::<crate::embroidery::PartSettings>();
        assert!(after.retained_bytes(&before) >= same + 2 * (part + 1000), "{} against {same}", after.retained_bytes(&before));
    }

    /// The original a converted object keeps counts, with its node as the file saved it.
    #[test]
    fn a_kept_original_counts() {
        let before = doc_with_a_rect();
        let mut after = before.clone();
        let same = after.retained_bytes(&before);
        let id = after.layers[0].children().and_then(|c| c.first()).map(|n| n.id).unwrap();
        let node = std::sync::Arc::new(serde_json::json!({"name": "x".repeat(10_000)}));
        let original = crate::embroidery::Original { frame: vectorcraft_geom::Affine::IDENTITY, node, fingerprint: None, extra: Default::default() };
        let e = Embroidery { original: Some(original), ..Embroidery::default() };
        after.node_mut(id).unwrap().extras_mut().embroidery = Some(Box::new(e));
        assert!(after.retained_bytes(&before) > same + 10_000);
    }
}
