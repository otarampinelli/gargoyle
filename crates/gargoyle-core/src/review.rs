use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Finding, FindingKind};

/// What a reviewer returns for one check on one chunk of changes.
///
/// There is no pass/fail field: the engine decides that from `findings`,
/// so a model can never say "PASS" and list problems at the same time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewOutput {
    /// One sentence summarizing the review. If there are no findings, say why the changes pass.
    pub summary: String,
    /// Problems found. Leave empty if the changes pass; that is the expected outcome.
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// A rule that parsed output breaks, with a JSON Pointer to where.
///
/// Shape errors (wrong types, unknown fields, bad paths) are caught while
/// parsing. These are the rules that span several fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Violation {
    /// JSON Pointer to the offending value, e.g. `/findings/0/fix/patch`.
    pub pointer: String,
    pub message: &'static str,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pointer, self.message)
    }
}

impl ReviewOutput {
    /// Every rule this output breaks, in document order. Empty means valid.
    ///
    /// All violations are returned at once so an agent can fix them in one retry.
    pub fn violations(&self) -> Vec<Violation> {
        let mut out = Vec::new();
        if self.summary.trim().is_empty() {
            out.push(violation("/summary", "must not be empty"));
        }
        for (i, finding) in self.findings.iter().enumerate() {
            check_finding(&format!("/findings/{i}"), finding, &mut out);
        }
        out
    }
}

fn check_finding(at: &str, finding: &Finding, out: &mut Vec<Violation>) {
    if finding.title.trim().is_empty() {
        out.push(violation(format!("{at}/title"), "must not be empty"));
    }
    if finding.message.trim().is_empty() {
        out.push(violation(format!("{at}/message"), "must not be empty"));
    }
    if finding.location.end_line < finding.location.start_line {
        out.push(violation(
            format!("{at}/location/end_line"),
            "must be >= start_line",
        ));
    }
    let patch = finding.fix.as_ref().and_then(|fix| fix.patch.as_ref());
    match (finding.kind, patch) {
        (FindingKind::Local, None) => {
            out.push(violation(
                format!("{at}/fix/patch"),
                "is required for local findings",
            ));
        }
        (FindingKind::Design, Some(_)) => {
            out.push(violation(
                format!("{at}/fix/patch"),
                "must be omitted for design findings",
            ));
        }
        _ => {}
    }
}

fn violation(pointer: impl Into<String>, message: &'static str) -> Violation {
    Violation {
        pointer: pointer.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> ReviewOutput {
        serde_json::from_str(json).unwrap()
    }

    const LOCAL_FINDING: &str = r#"{
        "summary": "Hardcoded password.",
        "findings": [{
            "title": "Hardcoded DB password",
            "severity": "error",
            "confidence": 0.9,
            "kind": "local",
            "location": { "path": "src/db.ts", "start_line": 12, "end_line": 12 },
            "message": "The password is committed in source.",
            "fix": { "description": "Read it from the environment.", "patch": "--- a/src/db.ts\n+++ b/src/db.ts\n" }
        }]
    }"#;

    #[test]
    fn valid_output_has_no_violations() {
        assert_eq!(parse(LOCAL_FINDING).violations(), []);
    }

    #[test]
    fn findings_default_to_empty() {
        let output = parse(r#"{ "summary": "No issues." }"#);
        assert_eq!(output.findings, []);
        assert_eq!(output.violations(), []);
    }

    #[test]
    fn side_defaults_to_new() {
        let output = parse(LOCAL_FINDING);
        assert_eq!(output.findings[0].location.side, crate::Side::New);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = serde_json::from_str::<ReviewOutput>(r#"{ "summary": "x", "result": "PASS" }"#);
        assert!(err.is_err());
    }

    #[test]
    fn rejects_line_zero() {
        let json = LOCAL_FINDING.replace(r#""start_line": 12"#, r#""start_line": 0"#);
        assert!(serde_json::from_str::<ReviewOutput>(&json).is_err());
    }

    #[test]
    fn rejects_paths_outside_the_repo() {
        let json = LOCAL_FINDING.replace("src/db.ts\", \"start", "../db.ts\", \"start");
        assert!(serde_json::from_str::<ReviewOutput>(&json).is_err());
    }

    #[test]
    fn reports_every_cross_field_violation() {
        let json = LOCAL_FINDING
            .replace(r#""end_line": 12"#, r#""end_line": 3"#)
            .replace(r#", "patch": "--- a/src/db.ts\n+++ b/src/db.ts\n""#, "");
        let pointers: Vec<_> = parse(&json)
            .violations()
            .into_iter()
            .map(|v| v.pointer)
            .collect();
        assert_eq!(
            pointers,
            ["/findings/0/location/end_line", "/findings/0/fix/patch"]
        );
    }

    #[test]
    fn design_findings_must_not_carry_a_patch() {
        let json = LOCAL_FINDING.replace(r#""kind": "local""#, r#""kind": "design""#);
        let violations = parse(&json).violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].pointer, "/findings/0/fix/patch");
    }
}
