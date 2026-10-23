//! json schemas for gargoyle published formats, generated from rust types
//!
//! the generated files live in 'schemas/' at the repo root, a test fails if
//! they drift from the types, see 'tests/schemas.rs'

use schemars::{Schema, schema_for};

use crate::{CheckSpec, Config, Report, ReviewOutput, ReviewPlan};

/// schemas for what a review returns
pub fn review_output() -> Schema {
    schema_for!(ReviewOutput)
}

/// schema for check file frontmatter
pub fn check() -> Schema {
    schema_for!(CheckSpec)
}

/// schema for gargoyle.toml
/// schemas/config.v1.json
pub fn config() -> Schema {
    schema_for!(Config)
}

/// schema for a run's plan.json, `schemas/plan.v1.json`
pub fn plan() -> Schema {
    schema_for!(ReviewPlan)
}

/// schema for `gargoyle report --format json`, `schemas/report.v1.json`
pub fn report() -> Schema {
    schema_for!(Report)
}
