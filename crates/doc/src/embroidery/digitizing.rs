//! What a live object of a digitized image was made with ([`Digitizing`]), saved under the group's
//! `digitizing` key in its embroidery.

use serde_json::{Map, Value};

use super::Kept;

/// The keys of [`Digitizing`] as the file saves it.
const SETTINGS: &str = "settings";
const FINGERPRINT: &str = "fingerprint";
const SIZE: &str = "size";

/// A fingerprint's hexadecimal digits: a 64-bit number, as an original's ([`super::Original`]).
const FINGERPRINT_DIGITS: usize = 16;

/// What a live object of a digitized image (`embroidery.digitizeImage {live: true}`) was made with:
/// the settings it is digitized again with, by their file keys (the engine reads and checks them;
/// this crate keeps them as they are), a fingerprint of the objects it made (what tells them
/// edited since), and its size in millimetres when it was made (what tells it scaled since).
///
/// Everything in it is in the source image's own frame (a change's point is in the image's
/// pixels), so no transform moves it, and it holds neither a `points` member nor a `frame`: an
/// older version, which keeps it unread among its kept keys, moves only those on a transform, so it
/// leaves it as this one does.
#[derive(Clone, Debug, PartialEq)]
pub struct Digitizing {
    pub settings: Kept,
    /// `None`: unknown (a file that kept none), so its objects are taken to be edited.
    pub fingerprint: Option<u64>,
    /// `[width, height]`, millimetres.
    pub size: Option<[f64; 2]>,
    /// Keys this version doesn't read, kept as they are.
    pub extra: Kept,
}

impl Digitizing {
    /// The digitizing a file holds under the `digitizing` key, when it is one this version reads:
    /// an object whose settings are an object (a fingerprint that isn't sixteen hexadecimal digits,
    /// or a size that isn't two finite numbers above 0, is kept, unread). `None` otherwise (kept as
    /// it is).
    pub(super) fn read(v: &Value) -> Option<Self> {
        let map = v.as_object()?;
        let settings = map.get(SETTINGS)?.as_object()?.clone();
        // Digits only: `from_str_radix` would take a sign before fifteen of them.
        let digits = |f: &&str| f.len() == FINGERPRINT_DIGITS && f.bytes().all(|b| b.is_ascii_hexdigit());
        let fingerprint = map.get(FINGERPRINT).and_then(Value::as_str).filter(digits).and_then(|f| u64::from_str_radix(f, 16).ok());
        let size = map.get(SIZE).and_then(Value::as_array).filter(|s| s.len() == 2).and_then(|s| {
            let side = |i: usize| s.get(i).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0);
            Some([side(0)?, side(1)?])
        });
        let read = |k: &str| k == SETTINGS || (k == FINGERPRINT && fingerprint.is_some()) || (k == SIZE && size.is_some());
        let extra: Map<String, Value> = map.iter().filter(|(k, _)| !read(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        Some(Self { settings: settings.into(), fingerprint, size, extra: extra.into() })
    }
}

impl From<Digitizing> for Value {
    fn from(d: Digitizing) -> Self {
        let mut out = std::sync::Arc::unwrap_or_clone(d.extra);
        out.insert(SETTINGS.into(), Value::Object(std::sync::Arc::unwrap_or_clone(d.settings)));
        if let Some(f) = d.fingerprint {
            out.insert(FINGERPRINT.into(), Value::String(format!("{f:016x}")));
        }
        if let Some([w, h]) = d.size {
            out.insert(SIZE.into(), serde_json::json!([w, h]));
        }
        Value::Object(out)
    }
}
