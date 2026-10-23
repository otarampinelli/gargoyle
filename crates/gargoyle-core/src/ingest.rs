//! checks a reviewer's answer for one task before it counts

use crate::{ReviewOutput, Task};

/// parses and validates the answer to `task`, or every problem with it at once,
/// so the reviewer can fix them all in one retry
pub fn ingest(task: &Task, answer: &str) -> Result<ReviewOutput, Vec<String>> {
    let Some(json) = extract_json(answer) else {
        return Err(vec!["no JSON object found in the answer".to_owned()]);
    };
    let output: ReviewOutput =
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
    }
    if problems.is_empty() {
        Ok(output)
    } else {
        Err(problems)
    }
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
    use crate::plan::TaskId;
    use crate::{CheckId, Confidence, RepoPath};

    fn task() -> Task {
        let check = CheckId::try_from("x".to_owned()).unwrap();
        Task {
            id: TaskId::new(&check, 1),
            check_id: check,
            check_name: "X".into(),
            paths: vec![RepoPath::try_from("src/a.rs".to_owned()).unwrap()],
            min_confidence: Confidence::try_from(0.5).unwrap(),
            prompt_path: String::new(),
            answer_path: String::new(),
            estimated_tokens: 1,
        }
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
