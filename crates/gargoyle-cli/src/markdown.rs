//! the report as markdown, what an agent shows the user in chat: a table to see
//! everything at once, then one numbered section per finding to triage by number

use std::fmt::Write as _;

use gargoyle_core::{Report, ReportedFinding, Severity, Side};

pub(crate) fn render(report: &Report) -> String {
    let mut out = String::new();
    let total = report.findings.len();
    let heading = match total {
        0 => "Gargoyle found no problems".to_owned(),
        1 => "Gargoyle found 1 problem".to_owned(),
        n => format!("Gargoyle found {n} problems"),
    };
    let _ = writeln!(out, "## {heading}\n");

    let mut about = Vec::new();
    if total > 0 {
        about.push(counts(report));
    }
    about.push(format!("compared against `{}`", report.base.name));
    about.push(format!("run `{}`", report.run_id));
    let _ = writeln!(out, "{}\n", about.join(" · "));

    if !report.missing_tasks.is_empty() {
        let tasks: Vec<_> = report
            .missing_tasks
            .iter()
            .map(gargoyle_core::TaskId::as_str)
            .collect();
        let _ = writeln!(
            out,
            "> **Incomplete:** no answer yet for {}, so this only covers the tasks that answered.\n",
            tasks.join(", ")
        );
    }

    if total > 0 {
        out.push_str("| # | Severity | Problem | Where | Check |\n|---|---|---|---|---|\n");
        for (i, reported) in report.findings.iter().enumerate() {
            let finding = &reported.finding;
            let _ = writeln!(
                out,
                "| {} | {} {} | {} | `{}` | {} |",
                i + 1,
                icon(finding.severity),
                finding.severity.as_str(),
                cell(&finding.title),
                location(reported),
                checks(reported)
            );
        }
        out.push('\n');
    }

    for (i, reported) in report.findings.iter().enumerate() {
        section(&mut out, i + 1, reported);
    }

    footer(&mut out, report);
    out
}

/// one finding, with everything needed to decide whether to fix it
fn section(out: &mut String, number: usize, reported: &ReportedFinding) {
    let finding = &reported.finding;
    let _ = writeln!(
        out,
        "### {} {number}. {}\n",
        icon(finding.severity),
        one_line(&finding.title)
    );
    let _ = writeln!(out, "`{}` · {}\n", location(reported), checks(reported));
    let _ = writeln!(out, "{}\n", finding.message.trim());
    if let Some(fix) = &finding.fix {
        let _ = writeln!(out, "**Fix:** {}\n", fix.description.trim());
        if let Some(patch) = &fix.patch {
            let fence = fence(patch);
            let _ = writeln!(out, "{fence}diff\n{}\n{fence}\n", patch.trim_end());
        }
    }
}

fn footer(out: &mut String, report: &Report) {
    let mut notes = Vec::new();
    let passed: Vec<_> = report
        .checks
        .iter()
        .filter(|check| check.passed)
        .map(|check| check.check_id.as_str())
        .collect();
    if !passed.is_empty() {
        notes.push(format!("**Passed:** {}", passed.join(", ")));
    }
    if !report.skipped.is_empty() {
        let skipped: Vec<_> = report
            .skipped
            .iter()
            .map(|s| format!("`{}` ({})", s.path, s.reason))
            .collect();
        notes.push(format!("**Not reviewed:** {}", skipped.join(", ")));
    }
    if report.low_confidence > 0 {
        let n = report.low_confidence;
        let noun = if n == 1 { "finding" } else { "findings" };
        notes.push(format!("**Left out:** {n} {noun} under `min_confidence`"));
    }
    for note in notes {
        let _ = writeln!(out, "{note}  ");
    }
}

pub(crate) fn icon(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "🔴",
        Severity::Warning => "🟡",
        Severity::Info => "🔵",
    }
}

/// `2 errors, 1 warning`, most severe first, leaving out severities with none
pub(crate) fn counts(report: &Report) -> String {
    let mut parts = Vec::new();
    for severity in [Severity::Error, Severity::Warning, Severity::Info] {
        let n = report
            .findings
            .iter()
            .filter(|f| f.finding.severity == severity)
            .count();
        if n > 0 {
            let s = if n == 1 { "" } else { "s" };
            parts.push(format!("{n} {}{s}", severity.as_str()));
        }
    }
    parts.join(", ")
}

/// `src/a.rs:12-14`, marked when the lines are in the old file
pub(crate) fn location(reported: &ReportedFinding) -> String {
    let location = &reported.finding.location;
    let mut out = format!("{}:{}", location.path, location.start_line);
    if location.end_line != location.start_line {
        let _ = write!(out, "-{}", location.end_line);
    }
    if location.side == Side::Old {
        out.push_str(" (removed lines)");
    }
    out
}

/// the check that reported it, then the others that found the same problem
pub(crate) fn checks(reported: &ReportedFinding) -> String {
    let mut out = reported.check_id.to_string();
    for other in &reported.also_flagged_by {
        let _ = write!(out, ", {other}");
    }
    out
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// text a model wrote, made safe for a table cell
fn cell(text: &str) -> String {
    one_line(text).replace('|', "\\|")
}

/// a code fence longer than any run of backticks in `code`, so the code can't close it
fn fence(code: &str) -> String {
    let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(findings: &serde_json::Value, missing: &[&str]) -> Report {
        serde_json::from_value(serde_json::json!({
            "schema": "gargoyle/report@1",
            "run_id": "4656c46c699a",
            "base": { "name": "origin/main", "commit": "abc" },
            "findings": findings,
            "checks": [
                { "check_id": "anti-slop", "name": "Anti-Slop", "tasks": 1, "findings": 0, "passed": true },
                { "check_id": "security-review", "name": "Security", "tasks": 1, "findings": 1, "passed": false }
            ],
            "skipped": [{ "path": "package-lock.json", "reason": "lockfile" }],
            "missing_tasks": missing,
            "low_confidence": 0
        }))
        .unwrap()
    }

    fn finding(title: &str, severity: &str, patch: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "fingerprint": "f",
            "check_id": "security-review",
            "also_flagged_by": ["correctness"],
            "finding": {
                "title": title,
                "severity": severity,
                "confidence": 0.9,
                "kind": if patch.is_some() { "local" } else { "design" },
                "location": { "path": "src/orders/repo.ts", "start_line": 112, "end_line": 117 },
                "message": "The status goes straight into the SQL.",
                "fix": { "description": "Bind it as a parameter.", "patch": patch }
            }
        })
    }

    #[test]
    fn a_table_then_one_section_per_finding() {
        let report = report(
            &serde_json::json!([
                finding(
                    "SQL built from | the query",
                    "error",
                    Some("--- a/x\n+++ b/x\n")
                ),
                finding("Slow loop", "warning", None),
            ]),
            &[],
        );
        let text = render(&report);
        assert!(text.starts_with("## Gargoyle found 2 problems\n\n1 error, 1 warning · compared against `origin/main`"), "{text}");
        assert!(
            text.contains(
                "| 1 | 🔴 error | SQL built from \\| the query | `src/orders/repo.ts:112-117` | security-review, correctness |"
            ),
            "{text}"
        );
        assert!(text.contains("### 🟡 2. Slow loop\n"), "{text}");
        assert!(
            text.contains("**Fix:** Bind it as a parameter.\n\n```diff\n--- a/x\n+++ b/x\n```"),
            "{text}"
        );
        assert!(text.contains("**Passed:** anti-slop"), "{text}");
        assert!(
            text.contains("**Not reviewed:** `package-lock.json` (lockfile)"),
            "{text}"
        );
    }

    #[test]
    fn no_findings_has_no_table() {
        let text = render(&report(&serde_json::json!([]), &[]));
        assert!(
            text.starts_with("## Gargoyle found no problems\n\ncompared against"),
            "{text}"
        );
        assert!(!text.contains("| # |"), "{text}");
    }

    #[test]
    fn an_incomplete_review_says_so_first() {
        let text = render(&report(&serde_json::json!([]), &["review-2"]));
        let note = text
            .find("**Incomplete:** no answer yet for review-2")
            .unwrap();
        assert!(note < text.find("**Passed:**").unwrap(), "{text}");
    }

    #[test]
    fn a_patch_with_backticks_gets_a_longer_fence() {
        assert_eq!(fence("+let a = 1;"), "```");
        assert_eq!(fence("+```js\n+code\n+```"), "````");
    }
}
