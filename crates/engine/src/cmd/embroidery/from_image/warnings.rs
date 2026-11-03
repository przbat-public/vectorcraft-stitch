//! What a digitizing reply warns of: each warning an id a window words in its own language, the
//! values that wording needs, and the English sentence beside them for a reader with no window (an
//! agent reads the sentence).

use serde_json::{Value, json};

/// One warning of a reply.
#[derive(Clone, Debug)]
pub(super) struct Warning {
    /// Which warning it is: a camel-case word that stays the same from one version to the next.
    pub(super) id: &'static str,
    /// What the wording needs (counts, sizes, colours), an object.
    pub(super) values: Value,
    /// The English sentence.
    pub(super) message: String,
}

impl Warning {
    /// Warning `id` with `values`, said in English as `message`.
    pub(super) fn new(id: &'static str, values: Value, message: impl Into<String>) -> Self {
        Self { id, values, message: message.into() }
    }

    /// The warning as the reply gives it.
    pub(super) fn json(&self) -> Value {
        json!({"id": self.id, "values": self.values, "message": self.message})
    }
}
