//! json schemas for gaygoyle published formats, generated from rust types
//!
//! the generated files live in 'schemas/' at the repo root, a test fails if
//! they drift from the types, see 'test/schemas,rs'

use schemars::{Schema, schema_for};

use crate::ReviewOutput;

/// schemas for what a review retursn
pub fn review_output() -> Schema {
    schema_for!(ReviewOutput)
}
