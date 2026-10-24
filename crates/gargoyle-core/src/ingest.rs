//! checks a reviewer's answer for one task before it counts

use std::collections::BTreeMap;

use crate::{Reference, RepoPath, ReviewOutput, Task};

/// parses and validates the answer to `task`, or every problem with it at once,
/// so the reviewer can fix them all in one retry,
/// `read_file` returns a file's current text, to check each finding's references
pub fn ingest(
    task: &Task,
    answer: &str,
    read_file: impl Fn(&RepoPath) -> Option<String>,
) -> Result<ReviewOutput, Vec<String>> {
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
    let mut files = BTreeMap::new();
    for (i, finding) in output.findings.iter().enumerate() {
        for (j, reference) in finding.references.iter().enumerate() {
            let text = files
                .entry(reference.path.clone())
                .or_insert_with(|| read_file(&reference.path));
            if let Err(problem) = check_reference(reference, text.as_deref()) {
                problems.push(format!("/findings/{i}/references/{j}: {problem}"));
            }
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

/// a reference must point at a line that exists and has the code it quotes,
/// so a reviewer can't back a finding with code it made up
fn check_reference(reference: &Reference, text: Option<&str>) -> Result<(), String> {
    let path = reference.path.as_str();
    let Some(text) = text else {
        return Err(format!("{path:?} doesn't exist"));
    };
    let wanted = squash(&reference.code);
    if wanted.is_empty() {
        return Err("`code` must not be empty".to_owned());
    }
    let number = reference.line.get();
    let index = usize::try_from(number - 1).unwrap_or(usize::MAX);
    let Some(line) = text.lines().nth(index) else {
        let count = text.lines().count();
        return Err(format!(
            "{path} has {count} lines, there's no line {number}"
        ));
    };
    if squash(line).contains(&wanted) {
        Ok(())
    } else {
        Err(format!(
            "line {number} of {path} is `{}`, not `{}`, copy the line exactly",
            line.trim(),
            reference.code.trim()
        ))
    }
}

/// the text with every run of whitespace as one space, so indentation doesn't matter
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

    fn no_files(_: &RepoPath) -> Option<String> {
        None
    }

    /// `answer` on src/a.rs with one reference
    fn with_reference(path: &str, line: u32, code: &str) -> String {
        answer("src/a.rs").replace(
            r#""message": "it's bad""#,
            &format!(
                r#""message": "it's bad", "references": [{{"path": "{path}", "line": {line}, "code": "{code}"}}]"#
            ),
        )
    }

    fn caller_file(path: &RepoPath) -> Option<String> {
        (path.as_str() == "src/cart.rs")
            .then(|| "fn total() {\n    let p = parse(a, b);\n}\n".to_owned())
    }

    #[test]
    fn accepts_references_that_match_the_file() {
        let text = with_reference("src/cart.rs", 2, "let p = parse(a,  b);");
        let output = ingest(&task(), &text, caller_file).unwrap();
        assert_eq!(output.findings[0].references[0].line.get(), 2);
    }

    #[test]
    fn rejects_references_to_code_that_isnt_there() {
        let cases = [
            (
                with_reference("src/gone.rs", 2, "x"),
                "\"src/gone.rs\" doesn't exist",
            ),
            (
                with_reference("src/cart.rs", 9, "x"),
                "src/cart.rs has 3 lines, there's no line 9",
            ),
            (
                with_reference("src/cart.rs", 1, "let p = parse(a, b);"),
                "line 1 of src/cart.rs is `fn total() {`",
            ),
            (
                with_reference("src/cart.rs", 1, " "),
                "`code` must not be empty",
            ),
        ];
        for (text, expected) in cases {
            let problems = ingest(&task(), &text, caller_file).unwrap_err();
            assert!(
                problems[0].starts_with("/findings/0/references/0: ")
                    && problems[0].contains(expected),
                "{problems:?}"
            );
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
        assert_eq!(ingest(&task(), &text, no_files).unwrap().findings.len(), 1);
    }

    #[test]
    fn fills_in_the_check_of_a_single_check_task() {
        let output = ingest(&task(), &answer("src/a.rs"), no_files).unwrap();
        assert_eq!(output.findings[0].check.as_ref().unwrap().as_str(), "x");
    }

    #[test]
    fn a_grouped_task_needs_the_check_of_every_finding() {
        let task = task_with(&["x", "y"]);
        let problems = ingest(&task, &answer("src/a.rs"), no_files).unwrap_err();
        assert!(
            problems[0].contains("/findings/0/check: is required"),
            "{problems:?}"
        );

        let named = answer("src/a.rs").replace(r#""title""#, r#""check": "y", "title""#);
        assert_eq!(
            ingest(&task, &named, no_files).unwrap().findings[0]
                .check
                .as_ref()
                .unwrap()
                .as_str(),
            "y"
        );

        let unknown = answer("src/a.rs").replace(r#""title""#, r#""check": "z", "title""#);
        let problems = ingest(&task, &unknown, no_files).unwrap_err();
        assert!(problems[0].contains("use one of: x, y"), "{problems:?}");
    }

    #[test]
    fn rejects_files_outside_the_task() {
        let problems = ingest(&task(), &answer("src/other.rs"), no_files).unwrap_err();
        assert!(problems[0].contains("use one of: src/a.rs"), "{problems:?}");
    }

    #[test]
    fn rejects_the_old_pass_fail_format() {
        let problems = ingest(&task(), "RESULT: PASS\nAll good.", no_files).unwrap_err();
        assert_eq!(problems, ["no JSON object found in the answer"]);
    }

    #[test]
    fn reports_schema_errors() {
        let problems =
            ingest(&task(), r#"{"summary": "x", "result": "PASS"}"#, no_files).unwrap_err();
        assert!(
            problems[0].contains("unknown field `result`"),
            "{problems:?}"
        );
    }
}
