//! the review plan, what `gargoyle prepare` writes and the agent runs

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CheckId, Confidence, RepoPath};

/// where runs live, relative to the repo root
pub const RUNS_DIR: &str = ".gargoyle/runs";

/// every task of one review, each task is one check on a group of files
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewPlan {
    /// Format version of this file. Always `gargoyle/plan@1`.
    pub schema: PlanSchema,
    pub run_id: RunId,
    pub base: PlanBase,
    pub tasks: Vec<Task>,
    /// Files left out of the review and why. Nothing is dropped silently.
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanSchema {
    #[serde(rename = "gargoyle/plan@1")]
    V1,
}

/// What the review compares against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanBase {
    /// The ref as written, like `origin/main`.
    pub name: String,
    /// The merge base commit. The review covers every change after it.
    pub commit: String,
}

/// One check on a group of files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Unique in the run, like `review-1`, or `security-review-1` with one task per check.
    pub id: TaskId,
    /// The checks this task reviews the files against.
    pub checks: Vec<TaskCheck>,
    /// Files this task reviews. Findings must point at one of them.
    pub paths: Vec<RepoPath>,
    /// The reviewer's prompt, relative to the repo root.
    pub prompt_path: String,
    /// Where the reviewer writes its JSON answer, relative to the repo root.
    pub answer_path: String,
    /// Rough size of the prompt, at 4 characters per token.
    pub estimated_tokens: u32,
}

/// A check a task reviews against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskCheck {
    pub id: CheckId,
    pub name: String,
    /// Findings below this confidence are left out of the report.
    pub min_confidence: Confidence,
}

/// A file left out of the review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Skipped {
    pub path: String,
    /// Set when only this check skipped the file, like a change too large for its budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_id: Option<CheckId>,
    pub reason: String,
}

/// Id of one review run, 12 hex characters taken from a hash of its prompts.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct RunId(String);

/// Id of a task inside a run, like `security-review-1`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct TaskId(String);

impl RunId {
    /// a run id typed by a user or an agent, none unless it's 12 lowercase hex characters,
    /// so it can't point a path outside the runs dir
    pub fn parse(id: &str) -> Option<Self> {
        let valid = id.len() == 12 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        valid.then(|| Self(id.to_owned()))
    }

    pub(crate) fn from_hash(hash: &blake3::Hash) -> Self {
        Self(hash.to_hex()[..12].to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// the run's folder, relative to the repo root
    pub fn dir(&self) -> String {
        format!("{RUNS_DIR}/{}", self.0)
    }

    pub fn plan_path(&self) -> String {
        format!("{}/plan.json", self.dir())
    }

    pub fn prompt_path(&self, task: &TaskId) -> String {
        format!("{}/prompts/{task}.md", self.dir())
    }

    pub fn answer_path(&self, task: &TaskId) -> String {
        format!("{}/answers/{task}.json", self.dir())
    }

    /// the validated answer, written by `gargoyle ingest`
    pub fn result_path(&self, task: &TaskId) -> String {
        format!("{}/results/{task}.json", self.dir())
    }

    pub fn report_path(&self) -> String {
        format!("{}/report.json", self.dir())
    }
}

impl TaskId {
    /// a task of a single check, like `security-review-1`
    pub(crate) fn new(check: &CheckId, number: usize) -> Self {
        Self(format!("{check}-{number}"))
    }

    /// a task of a group of checks, like `review-1`
    pub(crate) fn grouped(number: usize) -> Self {
        Self(format!("review-{number}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ReviewPlan {
    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id.as_str() == id)
    }
}

impl Task {
    pub fn check(&self, id: &CheckId) -> Option<&TaskCheck> {
        self.checks.iter().find(|check| &check.id == id)
    }

    /// the check ids, like `correctness, simplicity`, for messages
    pub fn check_list(&self) -> String {
        let ids: Vec<_> = self.checks.iter().map(|check| check.id.as_str()).collect();
        ids.join(", ")
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_must_be_short_hex() {
        assert!(RunId::parse("0123456789ab").is_some());
        for bad in [
            "",
            "0123456789aB",
            "0123456789abc",
            "../../../etc",
            "0123456789a/",
        ] {
            assert!(RunId::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn paths_live_in_the_run_dir() {
        let run = RunId::parse("0123456789ab").unwrap();
        let task = TaskId::new(&CheckId::try_from("simplicity".to_owned()).unwrap(), 2);
        assert_eq!(
            run.prompt_path(&task),
            ".gargoyle/runs/0123456789ab/prompts/simplicity-2.md"
        );
    }
}
