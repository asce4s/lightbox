//! The ordered list of edits for one photo.

use crate::edit::{EditOp, hash};
use serde::{Deserialize, Serialize};

/// The edit stack stored in `edits.ops_json` (SPEC §5.1, §6.6.1).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct EditStack {
    /// Incremented on every change to the stack.
    pub version: u32,
    /// Operations in application order. EXIF orientation is applied before the first one.
    pub ops: Vec<EditOp>,
}

impl EditStack {
    /// A stack with no edits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the stack contains no operations.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// `blake3(canonical JSON of ops)` as lowercase hex (SPEC §6.6.1). The version is not part
    /// of the hash, so identical edits always hash identically.
    pub fn hash(&self) -> String {
        hash::edit_stack_hash(&self.ops)
    }

    /// The hash to use in thumbnail cache keys: empty for an unedited photo.
    pub fn cache_hash(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            self.hash()
        }
    }
}
