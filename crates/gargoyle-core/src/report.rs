//! turns validated answers into the final report: confidence filter,
//! fingerprints, dedup across checks and a stable order

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::plan::TaskId;
use crate::{CheckId, Finding, PlanBase, RepoPath, ReviewOutput, ReviewPlan, RunId, Side, Skipped};

/// Everything a review found, after dedup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// Format version of this file. Always `gargoyle/report@1`.
    pub schema: ReportSchema,
    pub run_id: RunId,
    pub base: PlanBase,
    /// Most severe first.
    pub findings: Vec<ReportedFinding>,
    pub checks: Vec<CheckSummary>,
    pub skipped: Vec<Skipped>,
    /// Tasks with no answer yet. The review is incomplete while this isn't empty.
    pub missing_tasks: Vec<TaskId>,
    /// Findings left out for being under their task's `min_confidence`.
    pub low_confidence: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReportSchema {
    #[serde(rename = "gargoyle/report@1")]
    V1,
}

/// A finding with the identity that makes it trackable across runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportedFinding {
    /// Stable across runs while the code around the finding doesn't change, even if it moves.
    pub fingerprint: String,
    pub check_id: CheckId,
    /// Other checks that reported the same problem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_flagged_by: Vec<CheckId>,
    pub finding: Finding,
}

/// How one check did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckSummary {
    pub check_id: CheckId,
    pub name: String,
    pub tasks: usize,
    /// Findings this check reported, including ones merged into another check's.
    pub findings: usize,
    /// Every task of this check has an answer and none of them has a finding.
    pub passed: bool,
}

/// builds the report, `read_file` returns a file's current text for fingerprints
pub fn build_report(
    plan: &ReviewPlan,
    results: &BTreeMap<TaskId, ReviewOutput>,
    read_file: impl Fn(&RepoPath) -> Option<String>,
) -> Report {
    let mut missing_tasks = Vec::new();
    let mut low_confidence = 0;
    let mut files: BTreeMap<RepoPath, Option<String>> = BTreeMap::new();
    let mut merged: Vec<ReportedFinding> = Vec::new();
    // the task and check behind each merged finding, a reviewer that reports two
    // findings under one check means two problems, even on the same lines
    let mut sources: Vec<BTreeSet<(&TaskId, &CheckId)>> = Vec::new();

    for task in &plan.tasks {
        let Some(output) = results.get(&task.id) else {
            missing_tasks.push(task.id.clone());
            continue;
        };
        for finding in &output.findings {
            // ingest makes sure every finding names one of the task's checks
            let check = finding
                .check
                .as_ref()
                .and_then(|id| task.check(id))
                .or(task.checks.first());
            let Some(check) = check else { continue };
            if finding.confidence < check.min_confidence {
                low_confidence += 1;
                continue;
            }
            let path = &finding.location.path;
            let text = files.entry(path.clone()).or_insert_with(|| read_file(path));
            let candidate = ReportedFinding {
                fingerprint: fingerprint(&check.id, finding, text.as_deref()),
                check_id: check.id.clone(),
                also_flagged_by: Vec::new(),
                finding: finding.clone(),
            };
            let source = (&task.id, &check.id);
            let same = merged
                .iter()
                .zip(&sources)
                .position(|(seen, from)| !from.contains(&source) && same_problem(seen, &candidate));
            if let Some(i) = same {
                merge(&mut merged[i], candidate);
                sources[i].insert(source);
            } else {
                merged.push(candidate);
                sources.push(BTreeSet::from([source]));
            }
        }
    }

    merged.sort_by(|a, b| {
        let (fa, fb) = (&a.finding, &b.finding);
        fb.severity
            .cmp(&fa.severity)
            .then_with(|| fa.location.path.cmp(&fb.location.path))
            .then_with(|| fa.location.start_line.cmp(&fb.location.start_line))
            .then_with(|| a.check_id.cmp(&b.check_id))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });

    Report {
        schema: ReportSchema::V1,
        run_id: plan.run_id.clone(),
        base: plan.base.clone(),
        checks: summarize_checks(plan, &merged, &missing_tasks),
        findings: merged,
        skipped: plan.skipped.clone(),
        missing_tasks,
        low_confidence,
    }
}

/// hash of the check, rule, path and the code around the finding, line numbers
/// and titles are left out because they change between runs
fn fingerprint(check: &CheckId, finding: &Finding, file: Option<&str>) -> String {
    let location = &finding.location;
    let context = match (location.side, file) {
        (Side::New, Some(text)) => {
            let start = usize::try_from(location.start_line.get()).unwrap_or(usize::MAX);
            let lines: Vec<_> = text.lines().collect();
            let from = start.saturating_sub(3).min(lines.len());
            let to = (start + 2).min(lines.len());
            normalize(&lines[from..to].join("\n"))
        }
        // removed code isn't on disk anymore, the evidence is the best anchor left
        _ => normalize(finding.evidence.as_deref().unwrap_or_default()),
    };

    let mut hasher = blake3::Hasher::new();
    for part in [
        check.as_str(),
        finding.rule.as_deref().unwrap_or_default(),
        location.path.as_str(),
        &context,
    ] {
        hasher.update(part.as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

/// collapses whitespace so reformatting doesn't change a fingerprint
fn normalize(code: &str) -> String {
    let lines = code
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "));
    lines.collect::<Vec<_>>().join("\n")
}

/// same fingerprint, or the same spot with the same rule or a similar title
fn same_problem(a: &ReportedFinding, b: &ReportedFinding) -> bool {
    if a.fingerprint == b.fingerprint {
        return true;
    }
    let (la, lb) = (&a.finding.location, &b.finding.location);
    let overlaps = la.path == lb.path
        && la.side == lb.side
        && la.start_line <= lb.end_line
        && lb.start_line <= la.end_line;
    let same_rule = a.finding.rule.is_some() && a.finding.rule == b.finding.rule;
    overlaps && (same_rule || similarity(&a.finding.title, &b.finding.title) >= 0.5)
}

/// keeps the most severe, then most confident, version and remembers every check
fn merge(seen: &mut ReportedFinding, candidate: ReportedFinding) {
    let mut checks: BTreeSet<CheckId> = seen.also_flagged_by.drain(..).collect();
    checks.insert(seen.check_id.clone());
    checks.insert(candidate.check_id.clone());

    let rank = |f: &Finding| (f.severity, f.confidence.get());
    if rank(&candidate.finding) > rank(&seen.finding) {
        *seen = candidate;
    }
    checks.remove(&seen.check_id);
    seen.also_flagged_by = checks.into_iter().collect();
}

// small words say nothing about what the problem is
const FILLER_WORDS: [&str; 16] = [
    "a", "an", "and", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on", "the",
    "to",
];

/// share of words two titles have in common, from 0.0 to 1.0
fn similarity(a: &str, b: &str) -> f64 {
    let words = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .filter(|word| !word.is_empty() && !FILLER_WORDS.contains(&word.as_str()))
            .collect()
    };
    let (a, b) = (words(a), words(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // titles have a handful of words
    let share = a.intersection(&b).count() as f64 / union as f64;
    share
}

fn summarize_checks(
    plan: &ReviewPlan,
    findings: &[ReportedFinding],
    missing: &[TaskId],
) -> Vec<CheckSummary> {
    let mut checks: Vec<CheckSummary> = Vec::new();
    for task in &plan.tasks {
        let answered = !missing.contains(&task.id);
        for check in &task.checks {
            if let Some(summary) = checks.iter_mut().find(|c| c.check_id == check.id) {
                summary.tasks += 1;
                summary.passed &= answered;
                continue;
            }
            let flagged = findings
                .iter()
                .filter(|f| f.check_id == check.id || f.also_flagged_by.contains(&check.id))
                .count();
            checks.push(CheckSummary {
                check_id: check.id.clone(),
                name: check.name.clone(),
                tasks: 1,
                findings: flagged,
                passed: answered && flagged == 0,
            });
        }
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{PlanSchema, Task, TaskCheck};
    use crate::{Confidence, RunId};

    fn id(text: &str) -> CheckId {
        CheckId::try_from(text.to_owned()).unwrap()
    }

    fn task_check(check: &str) -> TaskCheck {
        TaskCheck {
            id: id(check),
            name: check.to_owned(),
            min_confidence: Confidence::try_from(0.5).unwrap(),
        }
    }

    fn plan(checks: &[&str]) -> ReviewPlan {
        let tasks = checks
            .iter()
            .map(|check| Task {
                id: TaskId::new(&id(check), 1),
                checks: vec![task_check(check)],
                paths: vec![RepoPath::try_from("src/a.rs".to_owned()).unwrap()],
                prompt_path: String::new(),
                answer_path: String::new(),
                estimated_tokens: 1,
            })
            .collect();
        ReviewPlan {
            schema: PlanSchema::V1,
            run_id: RunId::parse("0123456789ab").unwrap(),
            base: PlanBase {
                name: "main".into(),
                commit: "abc".into(),
            },
            tasks,
            skipped: Vec::new(),
        }
    }

    fn output(findings: &[(&str, &str, u32, &str, f64)]) -> ReviewOutput {
        let findings = findings
            .iter()
            .map(|(title, rule, line, severity, confidence)| {
                format!(
                    r#"{{"title": "{title}", "rule": "{rule}", "severity": "{severity}",
                    "confidence": {confidence}, "kind": "design", "message": "m",
                    "location": {{"path": "src/a.rs", "start_line": {line}, "end_line": {line}}}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        serde_json::from_str(&format!(r#"{{"summary": "s", "findings": [{findings}]}}"#)).unwrap()
    }

    fn results(answers: Vec<(&str, ReviewOutput)>) -> BTreeMap<TaskId, ReviewOutput> {
        answers
            .into_iter()
            .map(|(check, out)| (TaskId::new(&id(check), 1), out))
            .collect()
    }

    const FILE: &str = "fn a() {}\nfn b() {}\nlet password = \"hunter2\";\nfn c() {}\n";

    fn report(plan: &ReviewPlan, results: &BTreeMap<TaskId, ReviewOutput>) -> Report {
        build_report(plan, results, |_| Some(FILE.to_owned()))
    }

    #[test]
    fn merges_the_same_problem_from_two_checks() {
        let plan = plan(&["quality", "security"]);
        let results = results(vec![
            (
                "quality",
                output(&[("Hardcoded password", "", 3, "warning", 0.8)]),
            ),
            (
                "security",
                output(&[("Password hardcoded in source", "", 3, "error", 0.9)]),
            ),
        ]);
        let report = report(&plan, &results);
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.check_id.as_str(), "security");
        assert_eq!(finding.also_flagged_by, [id("quality")]);
        assert_eq!(
            report.checks.iter().map(|c| c.findings).collect::<Vec<_>>(),
            [1, 1]
        );
    }

    #[test]
    fn keeps_different_problems_on_the_same_line() {
        let plan = plan(&["quality", "security"]);
        let results = results(vec![
            (
                "quality",
                output(&[("Unclear variable name", "naming", 3, "info", 0.8)]),
            ),
            (
                "security",
                output(&[("Hardcoded password", "secret", 3, "error", 0.9)]),
            ),
        ]);
        assert_eq!(report(&plan, &results).findings.len(), 2);
    }

    #[test]
    fn keeps_two_problems_one_check_reported_on_the_same_lines() {
        let plan = plan(&["security"]);
        let results = results(vec![(
            "security",
            output(&[
                (
                    "Status is spliced into SQL",
                    "sql-injection",
                    3,
                    "error",
                    0.9,
                ),
                ("Sort is spliced into SQL", "sql-injection", 3, "error", 0.9),
            ]),
        )]);
        assert_eq!(report(&plan, &results).findings.len(), 2);
    }

    #[test]
    fn small_words_dont_count_in_title_similarity() {
        let a = "Hardcoded database password";
        let b = "Password hardcoded in the source";
        assert!(similarity(a, b) >= 0.5, "{}", similarity(a, b));
        assert!(similarity("Missing test for parser", "Unused import") < 0.5);
    }

    #[test]
    fn a_grouped_task_reports_each_finding_under_its_own_check() {
        let mut plan = plan(&["quality"]);
        plan.tasks[0].checks.push(TaskCheck {
            min_confidence: Confidence::try_from(0.9).unwrap(),
            ..task_check("security")
        });
        let mut answer = output(&[
            ("Unclear name", "naming", 1, "info", 0.8),
            ("Hardcoded password", "secret", 3, "error", 0.95),
            ("Maybe a secret", "secret", 4, "warning", 0.8),
        ]);
        for (finding, check) in answer
            .findings
            .iter_mut()
            .zip(["quality", "security", "security"])
        {
            finding.check = Some(id(check));
        }
        let report = report(&plan, &results(vec![("quality", answer)]));

        let found: Vec<_> = report
            .findings
            .iter()
            .map(|f| f.check_id.as_str())
            .collect();
        assert_eq!(found, ["security", "quality"]);
        // security asks for 0.9, so its 0.8 finding is left out
        assert_eq!(report.low_confidence, 1);
        let summary: Vec<_> = report
            .checks
            .iter()
            .map(|c| (c.check_id.as_str(), c.findings))
            .collect();
        assert_eq!(summary, [("quality", 1), ("security", 1)]);
    }

    #[test]
    fn drops_low_confidence_findings() {
        let plan = plan(&["quality"]);
        let results = results(vec![("quality", output(&[("Maybe", "", 1, "info", 0.2)]))]);
        let report = report(&plan, &results);
        assert_eq!((report.findings.len(), report.low_confidence), (0, 1));
    }

    #[test]
    fn lists_tasks_without_an_answer() {
        let report = report(&plan(&["quality"]), &BTreeMap::new());
        assert_eq!(report.missing_tasks, [TaskId::new(&id("quality"), 1)]);
    }

    #[test]
    fn fingerprint_survives_code_moving_down() {
        let finding = &output(&[("Hardcoded password", "secret", 3, "error", 0.9)]).findings[0];
        let before = fingerprint(&id("security"), finding, Some(FILE));

        let mut moved = finding.clone();
        moved.location.start_line = std::num::NonZeroU32::new(13).unwrap();
        moved.location.end_line = moved.location.start_line;
        moved.title = "A different title".into();
        let shifted = format!("{}{FILE}", "// new line\n".repeat(10));
        assert_eq!(before, fingerprint(&id("security"), &moved, Some(&shifted)));
    }

    #[test]
    fn same_input_gives_the_same_report() {
        let plan = plan(&["quality", "security"]);
        let results = results(vec![
            (
                "quality",
                output(&[("b", "", 4, "info", 0.9), ("a", "", 1, "error", 0.9)]),
            ),
            ("security", output(&[("c", "", 2, "warning", 0.9)])),
        ]);
        let first = serde_json::to_string(&report(&plan, &results)).unwrap();
        let second = serde_json::to_string(&report(&plan, &results)).unwrap();
        assert_eq!(first, second);
        let severities: Vec<_> = report(&plan, &results)
            .findings
            .iter()
            .map(|f| f.finding.severity.as_str())
            .collect();
        assert_eq!(severities, ["error", "warning", "info"]);
    }
}
