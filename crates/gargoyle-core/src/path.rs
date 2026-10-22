use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// A file path relative to the repository root, using forward slashes.
///
/// Model output is untrusted. Validating paths when they are parsed means
/// nothing later (like applying a fix) can be pointed outside the repo.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoPath(String);

impl RepoPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Returns why `path` is not a valid repo path, or `None` if it is.
fn problem(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        Some("is empty")
    } else if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        Some("must be relative to the repository root")
    } else if path.contains('\\') {
        Some("must use forward slashes")
    } else if path.contains('\0') {
        Some("must not contain NUL bytes")
    } else if path
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        Some("must not contain empty, '.' or '..' segments")
    } else {
        None
    }
}

/// Error returned when a string is not a valid [`RepoPath`].
#[derive(Debug, thiserror::Error)]
#[error("invalid path {path:?}: {reason}")]
pub struct InvalidRepoPath {
    path: String,
    reason: &'static str,
}

impl TryFrom<String> for RepoPath {
    type Error = InvalidRepoPath;

    fn try_from(path: String) -> Result<Self, Self::Error> {
        match problem(&path) {
            Some(reason) => Err(InvalidRepoPath { path, reason }),
            None => Ok(Self(path)),
        }
    }
}

impl From<RepoPath> for String {
    fn from(path: RepoPath) -> Self {
        path.0
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for RepoPath {
    fn schema_name() -> Cow<'static, str> {
        "RepoPath".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "description": "File path relative to the repository root, with forward slashes, e.g. \"src/app.ts\"."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str) -> Result<RepoPath, InvalidRepoPath> {
        RepoPath::try_from(path.to_owned())
    }

    #[test]
    fn accepts_relative_paths() {
        assert_eq!(parse("src/app.ts").unwrap().as_str(), "src/app.ts");
        assert!(parse(".github/workflows/ci.yml").is_ok());
    }

    #[test]
    fn rejects_paths_that_escape_the_repo() {
        for bad in [
            "",
            "/etc/passwd",
            "C:/x",
            "a/../b",
            "../x",
            "a//b",
            "./a",
            "a\\b",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn validates_during_deserialization() {
        assert!(serde_json::from_str::<RepoPath>("\"../secret\"").is_err());
    }
}
