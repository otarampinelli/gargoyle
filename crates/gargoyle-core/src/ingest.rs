//! checks a reviewer's answer for one task before it counts

use crate::{ReviewOutput, Task};

/// parses and validates the answer to `task`, or every problem with it at once,
/// so the reviewer can fix them all in one retry
pub fn ingest(task: &Task, answer: &str) -> Result<ReviewOutput, Vec<String>> {
    let Some(json) = extract_json(answer) else {
        return Err(vec!["no JSON object found in the answer".to_owned()]);
    };
    let mut output: ReviewOutput =
        serde_json::from_str(json).map_err(|err| vec![format!("invalid JSON: {err}")])?;

    let mut problems: Vec<String> = output
        .violations()
        .iter()
        .map(ToString::to_string)
        .collect();
    for (i, finding) in output.findings.iter().enumerate() {
        if !task.paths.contains(&finding.location.path) {
            let allowed: Vec<_> = task.paths.iter().map(crate::RepoPath::as_str).collect();
            problems.push(format!(
                "/findings/{i}/location/path: {:?} is not in this task, use one of: {}",
                finding.location.path.as_str(),
                allowed.join(", ")
            ));
        }
        match &finding.check {
            Some(id) if task.check(id).is_none() => problems.push(format!(
                "/findings/{i}/check: {:?} is not a check of this task, use one of: {}",
                id.as_str(),
                task.check_list()
            )),
            None if task.checks.len() > 1 => problems.push(format!(
                "/findings/{i}/check: is required, this task covers several checks, use one of: {}",
                task.check_list()
            )),
            _ => {}
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    // with a single check there's only one answer, so the reviewer may leave it out
    if let [only] = task.checks.as_slice() {
        for finding in &mut output.findings {
            finding.check.get_or_insert_with(|| only.id.clone());
        }
    }
    Ok(output)
}

/// the JSON object in an answer, models like to wrap it in prose or code fences
fn extract_json(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    answer.get(start..=end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{TaskCheck, TaskId};
    use crate::{CheckId, Confidence, RepoPath};

    /// a task on src/a.rs with the given checks
    fn task_with(checks: &[&str]) -> Task {
        let checks: Vec<_> = checks
            .iter()
            .map(|id| TaskCheck {
                id: CheckId::try_from((*id).to_owned()).unwrap(),
                name: (*id).to_owned(),
                min_confidence: Confidence::try_from(0.5).unwrap(),
            })
            .collect();
        Task {
            id: TaskId::new(&checks[0].id, 1),
            checks,
            paths: vec![RepoPath::try_from("src/a.rs".to_owned()).unwrap()],
            prompt_path: String::new(),
            answer_path: String::new(),
            estimated_tokens: 1,
        }
    }

    fn task() -> Task {
        task_with(&["x"])
    }

    fn answer(path: &str) -> String {
        format!(
            r#"{{"summary": "one problem", "findings": [{{
                "title": "bad", "severity": "error", "confidence": 0.9, "kind": "design",
                "location": {{"path": "{path}", "start_line": 3, "end_line": 3}},
                "message": "it's bad"
            }}]}}"#
        )
    }

    #[test]
    fn accepts_a_valid_answer_in_a_code_fence() {
        let text = format!("Here you go:\n```json\n{}\n```", answer("src/a.rs"));
        assert_eq!(ingest(&task(), &text).unwrap().findings.len(), 1);
    }

    #[test]
    fn fills_in_the_check_of_a_single_check_task() {
        let output = ingest(&task(), &answer("src/a.rs")).unwrap();
        assert_eq!(output.findings[0].check.as_ref().unwrap().as_str(), "x");
    }

    #[test]
    fn a_grouped_task_needs_the_check_of_every_finding() {
        let task = task_with(&["x", "y"]);
        let problems = ingest(&task, &answer("src/a.rs")).unwrap_err();
        assert!(
            problems[0].contains("/findings/0/check: is required"),
            "{problems:?}"
        );

        let named = answer("src/a.rs").replace(r#""title""#, r#""check": "y", "title""#);
        assert_eq!(
            ingest(&task, &named).unwrap().findings[0]
                .check
                .as_ref()
                .unwrap()
                .as_str(),
            "y"
        );

        let unknown = answer("src/a.rs").replace(r#""title""#, r#""check": "z", "title""#);
        let problems = ingest(&task, &unknown).unwrap_err();
        assert!(problems[0].contains("use one of: x, y"), "{problems:?}");
    }

    #[test]
    fn rejects_files_outside_the_task() {
        let problems = ingest(&task(), &answer("src/other.rs")).unwrap_err();
        assert!(problems[0].contains("use one of: src/a.rs"), "{problems:?}");
    }

    #[test]
    fn rejects_the_old_pass_fail_format() {
        let problems = ingest(&task(), "RESULT: PASS\nAll good.").unwrap_err();
        assert_eq!(problems, ["no JSON object found in the answer"]);
    }

    #[test]
    fn reports_schema_errors() {
        let problems = ingest(&task(), r#"{"summary": "x", "result": "PASS"}"#).unwrap_err();
        assert!(
            problems[0].contains("unknown field `result`"),
            "{problems:?}"
        );
    }
}
