use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use globset::Glob;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CheckId, Confidence, Severity};

/// Frontmatter of a check file (`checks/<id>.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    /// Format version of this file. Must be `gargoyle/check@1`.
    pub schema: CheckSchema,
    pub id: CheckId,
    /// Bump when the instructions change meaningfully. Invalidates cached results.
    pub version: NonZeroU32,
    /// Human-readable name.
    pub name: String,
    /// One sentence: what this check catches.
    pub description: String,
    /// Default severity for findings from this check.
    #[serde(default = "default_severity")]
    pub severity: Severity,
    /// Which changed files this check reviews.
    #[serde(default)]
    pub applies_to: AppliesTo,
    /// Kinds of change this check matters most for. Only used to rank checks, never to skip them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// Drop findings below this confidence. Overrides the global setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<Confidence>,
}

/// Version of the check file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CheckSchema {
    #[serde(rename = "gargoyle/check@1")]
    V1,
}

/// Which changed files a check reviews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppliesTo {
    /// Glob patterns of files to review. Defaults to every file.
    #[serde(default = "all_files")]
    pub globs: Vec<String>,
    /// Glob patterns of files to skip, even if they match `globs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

impl Default for AppliesTo {
    fn default() -> Self {
        Self {
            globs: all_files(),
            exclude: Vec::new(),
        }
    }
}

fn default_severity() -> Severity {
    Severity::Warning
}

fn all_files() -> Vec<String> {
    vec!["**/*".to_owned()]
}

/// A check loaded from disk: its frontmatter plus its review instructions.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub spec: CheckSpec,
    /// The markdown after the frontmatter, trimmed.
    pub instructions: String,
    /// Where the check was loaded from.
    pub path: PathBuf,
}

/// A problem with one check file.
#[derive(Debug, thiserror::Error)]
#[error("{}: {kind}", .path.display())]
pub struct CheckError {
    pub path: PathBuf,
    pub kind: CheckErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CheckErrorKind {
    #[error("could not read: {0}")]
    Read(std::io::Error),
    #[error("missing frontmatter: the file must start with a `---` line and close it with another")]
    MissingFrontmatter,
    #[error("invalid frontmatter: {0}")]
    Frontmatter(Box<serde_saphyr::Error>),
    #[error("id `{id}` must match the file name `{expected}`")]
    IdMismatch { id: CheckId, expected: String },
    #[error("invalid glob {glob:?}: {source}")]
    InvalidGlob {
        glob: String,
        source: Box<globset::Error>,
    },
    #[error("no review instructions after the frontmatter")]
    EmptyInstructions,
}

impl Check {
    /// Parses a check from the text of the file at `path`. Doesn't touch the filesystem.
    pub fn parse(path: &Path, text: &str) -> Result<Self, CheckError> {
        let fail = |kind: CheckErrorKind| CheckError {
            path: path.to_owned(),
            kind,
        };

        let (frontmatter, body) =
            split_frontmatter(text).ok_or_else(|| fail(CheckErrorKind::MissingFrontmatter))?;
        let spec: CheckSpec = serde_saphyr::from_str(frontmatter)
            .map_err(|err| fail(CheckErrorKind::Frontmatter(Box::new(err))))?;

        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        if spec.id.as_str() != stem {
            let expected = stem.into_owned();
            return Err(fail(CheckErrorKind::IdMismatch {
                id: spec.id,
                expected,
            }));
        }

        for glob in spec.applies_to.globs.iter().chain(&spec.applies_to.exclude) {
            if let Err(err) = Glob::new(glob) {
                let kind = CheckErrorKind::InvalidGlob {
                    glob: glob.clone(),
                    source: Box::new(err),
                };
                return Err(fail(kind));
            }
        }

        let instructions = body.trim();
        if instructions.is_empty() {
            return Err(fail(CheckErrorKind::EmptyInstructions));
        }

        Ok(Self {
            spec,
            instructions: instructions.to_owned(),
            path: path.to_owned(),
        })
    }

    pub fn id(&self) -> &CheckId {
        &self.spec.id
    }
}

/// Loads every `*.md` check in `dir`, sorted by file name.
///
/// Returns every problem at once instead of stopping at the first,
/// so `gargoyle checks validate` can show them all.
pub fn load_checks(dir: &Path) -> Result<Vec<Check>, Vec<CheckError>> {
    let read_error = |path: &Path, err| {
        vec![CheckError {
            path: path.to_owned(),
            kind: CheckErrorKind::Read(err),
        }]
    };

    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| read_error(dir, err))? {
        let path = entry.map_err(|err| read_error(dir, err))?.path();
        if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();

    let mut checks = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let loaded = fs::read_to_string(&path)
            .map_err(|err| CheckError {
                path: path.clone(),
                kind: CheckErrorKind::Read(err),
            })
            .and_then(|text| Check::parse(&path, &text));
        match loaded {
            Ok(check) => checks.push(check),
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(checks)
    } else {
        Err(errors)
    }
}

/// Splits `---\n<yaml>\n---\n<body>` into `(yaml, body)`.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "---
schema: gargoyle/check@1
id: security
version: 1
name: Security
description: Flag security issues
---

Look for hardcoded secrets.
";

    fn parse(text: &str) -> Result<Check, CheckError> {
        Check::parse(Path::new("checks/security.md"), text)
    }

    fn error_kind(text: &str) -> CheckErrorKind {
        parse(text).unwrap_err().kind
    }

    #[test]
    fn parses_a_minimal_check_with_defaults() {
        let check = parse(MINIMAL).unwrap();
        assert_eq!(check.id().as_str(), "security");
        assert_eq!(check.instructions, "Look for hardcoded secrets.");
        assert_eq!(check.spec.severity, Severity::Warning);
        assert_eq!(check.spec.applies_to.globs, ["**/*"]);
    }

    #[test]
    fn accepts_crlf_line_endings() {
        assert!(parse(&MINIMAL.replace('\n', "\r\n")).is_ok());
    }

    #[test]
    fn requires_frontmatter() {
        let kind = error_kind("Look for hardcoded secrets.\n");
        assert!(matches!(kind, CheckErrorKind::MissingFrontmatter));
    }

    #[test]
    fn rejects_unknown_fields() {
        let kind = error_kind(&MINIMAL.replace("version: 1", "version: 1\nseverty: error"));
        assert!(kind.to_string().contains("severty"), "{kind}");
    }

    #[test]
    fn rejects_unsupported_schema_versions() {
        let kind = error_kind(&MINIMAL.replace("check@1", "check@2"));
        assert!(kind.to_string().contains("gargoyle/check@1"), "{kind}");
    }

    #[test]
    fn id_must_match_the_file_name() {
        let kind = error_kind(&MINIMAL.replace("id: security", "id: secrets"));
        assert!(matches!(kind, CheckErrorKind::IdMismatch { .. }));
    }

    #[test]
    fn rejects_invalid_globs() {
        let text = MINIMAL.replace(
            "version: 1",
            "version: 1\napplies_to:\n  globs: [\"src/[\"]",
        );
        assert!(matches!(
            error_kind(&text),
            CheckErrorKind::InvalidGlob { .. }
        ));
    }

    #[test]
    fn requires_instructions() {
        let text = MINIMAL.replace("Look for hardcoded secrets.", "");
        assert!(matches!(
            error_kind(&text),
            CheckErrorKind::EmptyInstructions
        ));
    }

    #[test]
    fn context_is_not_up_to_a_check() {
        // every task shows the changed definitions whole, with the same related code
        let text = MINIMAL.replace("version: 1", "version: 1\ncontext:\n  mode: hunk");
        assert!(matches!(error_kind(&text), CheckErrorKind::Frontmatter(_)));
    }

    #[test]
    fn loads_checks_sorted_and_reports_every_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| fs::write(dir.path().join(name), text).unwrap();
        write("security.md", MINIMAL);
        write("alpha.md", &MINIMAL.replace("id: security", "id: alpha"));
        write("broken.md", "no frontmatter");
        write("empty.md", "---\n---\n");
        write("notes.txt", "ignored: not markdown");

        let errors = load_checks(dir.path()).unwrap_err();
        let names: Vec<_> = errors.iter().map(|e| e.path.file_name().unwrap()).collect();
        assert_eq!(names, ["broken.md", "empty.md"]);

        fs::remove_file(dir.path().join("broken.md")).unwrap();
        fs::remove_file(dir.path().join("empty.md")).unwrap();
        let checks = load_checks(dir.path()).unwrap();
        let ids: Vec<_> = checks.iter().map(|c| c.id().as_str()).collect();
        assert_eq!(ids, ["alpha", "security"]);
    }
}
