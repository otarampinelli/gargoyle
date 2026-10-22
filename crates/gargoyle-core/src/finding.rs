use std::num::NonZeroU32;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Confidence, RepoPath, Severity};

// Doc comments on these types become `description`s in the JSON Schema,
// which models read. Write them for the model.

/// One problem found in the changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// Short stable slug for the kind of problem, if the check lists one (e.g. "hardcoded-secret").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// A short title, under 80 characters.
    pub title: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub kind: FindingKind,
    pub location: Location,
    /// One or two sentences: what is wrong and why it matters.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
    /// The exact code that shows the problem, copied from the diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// Whether the fix is a small code change or a broader design change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FindingKind {
    /// Fixable with a small patch to the changed lines. Requires `fix.patch`.
    Local,
    /// Needs broader changes (new files, refactors, config). Must not include `fix.patch`.
    Design,
}

/// Where a finding is, in the new or old version of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: RepoPath,
    /// First line of the problem, 1-based.
    pub start_line: NonZeroU32,
    /// Last line of the problem, 1-based. Same as `start_line` for a single line.
    pub end_line: NonZeroU32,
    #[serde(default)]
    pub side: Side,
}

/// Which side of the diff the line numbers refer to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// The changed file after the diff (the usual case).
    #[default]
    New,
    /// The file before the diff, for problems with removed lines.
    Old,
}

/// How to fix a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fix {
    /// One or two sentences on how to fix it.
    pub description: String,
    /// A unified diff (`--- a/path`, `+++ b/path`, `@@` hunks) that applies the fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}
