//! builds the review plan: drops files that shouldn't be reviewed,
//! routes the rest to checks and packs them into tasks that fit each check's budget

use std::collections::BTreeMap;
use std::slice;

use gargoyle_git::{FileDiff, FileStatus};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::plan::{PlanSchema, RunId, TaskCheck, TaskId};
use crate::prompt::{render_file, render_prompt};
use crate::{Check, Config, PlanBase, RepoPath, ReviewPlan, Skipped, Task, TaskMode};

// files nobody reviews by hand
const BUILT_IN_EXCLUDES: [(&str, &str); 18] = [
    ("**/Cargo.lock", "lockfile"),
    ("**/package-lock.json", "lockfile"),
    ("**/npm-shrinkwrap.json", "lockfile"),
    ("**/yarn.lock", "lockfile"),
    ("**/pnpm-lock.yaml", "lockfile"),
    ("**/bun.lock", "lockfile"),
    ("**/poetry.lock", "lockfile"),
    ("**/Pipfile.lock", "lockfile"),
    ("**/uv.lock", "lockfile"),
    ("**/Gemfile.lock", "lockfile"),
    ("**/composer.lock", "lockfile"),
    ("**/go.sum", "lockfile"),
    ("**/flake.lock", "lockfile"),
    ("**/*.min.js", "minified"),
    ("**/*.min.css", "minified"),
    ("**/*.map", "source map"),
    ("**/*.snap", "snapshot"),
    ("**/__snapshots__/**", "snapshot"),
];

/// everything a plan is built from
#[derive(Debug)]
pub struct PlanInput<'a> {
    pub base: PlanBase,
    pub files: &'a [FileDiff],
    pub checks: &'a [Check],
    pub config: &'a Config,
}

/// a plan and the prompt of each task, in the same order as `plan.tasks`
#[derive(Debug)]
pub struct Plan {
    pub plan: ReviewPlan,
    pub prompts: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    #[error(
        "this review needs about {needed} tokens, more than review.max_tokens ({limit}), \
         raise it in gargoyle.toml or pick fewer checks with --checks"
    )]
    OverBudget { needed: u64, limit: u32 },
    #[error("invalid glob {glob:?}: {source}")]
    InvalidGlob {
        glob: String,
        source: Box<globset::Error>,
    },
}

/// files of one task and their rendered changes
#[derive(Debug, Default)]
struct Group {
    paths: Vec<RepoPath>,
    code: String,
}

/// a reviewable file and its diff
type File<'a> = (RepoPath, &'a FileDiff);

/// checks and the files they review together
type CheckGroup<'c, 'f> = (Vec<&'c Check>, Vec<&'f File<'f>>);

pub fn build_plan(input: &PlanInput<'_>) -> Result<Plan, PlanError> {
    let mut skipped = Vec::new();
    let files = reviewable_files(input.files, &input.config.files.exclude, &mut skipped)?;
    let schema = serde_json::to_string_pretty(&crate::schema::review_output()).unwrap_or_default();

    let mut checks: Vec<&Check> = input.checks.iter().collect();
    checks.sort_by(|a, b| a.id().cmp(b.id()));
    let routed = route(&checks, &files, &mut skipped)?;

    let groups = match input.config.review.tasks {
        TaskMode::PerCheck => routed,
        TaskMode::Grouped => group_by_files(&files, &routed),
    };

    let mut drafts = Vec::new();
    for (checks, files) in groups {
        for chunk in pack(&checks, &files, &schema, &mut skipped) {
            let prompt = render_prompt(&checks, &chunk.code, &schema);
            drafts.push((checks.clone(), chunk.paths, prompt));
        }
    }

    // the same changes and checks always give the same run id
    let mut hasher = blake3::Hasher::new();
    hasher.update(input.base.commit.as_bytes());
    for (_, _, prompt) in &drafts {
        hasher.update(prompt.as_bytes());
    }
    let run_id = RunId::from_hash(&hasher.finalize());

    let mut tasks: Vec<Task> = Vec::new();
    let mut prompts = Vec::new();
    for (checks, paths, prompt) in drafts {
        let id = match input.config.review.tasks {
            TaskMode::Grouped => TaskId::grouped(tasks.len() + 1),
            TaskMode::PerCheck => {
                let check = checks[0].id();
                let number = 1 + tasks.iter().filter(|t| t.check(check).is_some()).count();
                TaskId::new(check, number)
            }
        };
        let checks = checks
            .iter()
            .map(|check| TaskCheck {
                id: check.id().clone(),
                name: check.spec.name.clone(),
                min_confidence: check
                    .spec
                    .min_confidence
                    .unwrap_or(input.config.review.min_confidence),
            })
            .collect();
        tasks.push(Task {
            checks,
            paths,
            prompt_path: run_id.prompt_path(&id),
            answer_path: run_id.answer_path(&id),
            estimated_tokens: estimate_tokens(&prompt),
            id,
        });
        prompts.push(prompt);
    }

    let needed: u64 = tasks.iter().map(|t| u64::from(t.estimated_tokens)).sum();
    let limit = input.config.review.max_tokens.get();
    if needed > u64::from(limit) {
        return Err(PlanError::OverBudget { needed, limit });
    }

    let plan = ReviewPlan {
        schema: PlanSchema::V1,
        run_id,
        base: input.base.clone(),
        tasks,
        skipped,
    };
    Ok(Plan { plan, prompts })
}

/// every check with the files its `applies_to` matches, files no check wants are skipped
fn route<'c, 'f>(
    checks: &[&'c Check],
    files: &'f [File<'f>],
    skipped: &mut Vec<Skipped>,
) -> Result<Vec<CheckGroup<'c, 'f>>, PlanError> {
    let mut routed = Vec::new();
    for &check in checks {
        let include = glob_set(&check.spec.applies_to.globs)?;
        let exclude = glob_set(&check.spec.applies_to.exclude)?;
        let matched: Vec<_> = files
            .iter()
            .filter(|(path, _)| include.is_match(path.as_str()) && !exclude.is_match(path.as_str()))
            .collect();
        routed.push((vec![check], matched));
    }
    for (path, _) in files {
        if !routed
            .iter()
            .any(|(_, matched)| matched.iter().any(|(p, _)| p == path))
        {
            skipped.push(Skipped {
                path: path.to_string(),
                check_id: None,
                reason: "no check applies to it".to_owned(),
            });
        }
    }
    routed.retain(|(_, matched)| !matched.is_empty());
    Ok(routed)
}

/// files that the same checks apply to go into one group, with all of those checks
fn group_by_files<'c, 'f>(
    files: &'f [File<'f>],
    routed: &[CheckGroup<'c, 'f>],
) -> Vec<CheckGroup<'c, 'f>> {
    // keyed by the positions of the checks, so the order is stable
    let mut groups: BTreeMap<Vec<usize>, Vec<&'f File<'f>>> = BTreeMap::new();
    for file in files {
        let key: Vec<usize> = routed
            .iter()
            .enumerate()
            .filter(|(_, (_, matched))| matched.iter().any(|(p, _)| *p == file.0))
            .map(|(i, _)| i)
            .collect();
        if !key.is_empty() {
            groups.entry(key).or_default().push(file);
        }
    }
    groups
        .into_iter()
        .map(|(key, files)| {
            let checks = key.iter().map(|&i| routed[i].0[0]).collect();
            (checks, files)
        })
        .collect()
}

/// rough token count, about 4 characters per token for code and english
pub(crate) fn estimate_tokens(text: &str) -> u32 {
    u32::try_from(text.chars().count().div_ceil(4)).unwrap_or(u32::MAX)
}

/// the files worth reviewing, sorted by path, every other one goes to `skipped`
fn reviewable_files<'a>(
    files: &'a [FileDiff],
    exclude: &[String],
    skipped: &mut Vec<Skipped>,
) -> Result<Vec<(RepoPath, &'a FileDiff)>, PlanError> {
    let built_in = glob_set(BUILT_IN_EXCLUDES.map(|(glob, _)| glob))?;
    let excluded = glob_set(exclude)?;

    let mut sorted: Vec<&FileDiff> = files.iter().collect();
    sorted.sort_by(|a, b| a.path().cmp(b.path()));

    let mut out = Vec::new();
    for file in sorted {
        let path = file.path();
        let reason = if file.status() == FileStatus::Deleted {
            Some("deleted")
        } else if file.binary {
            Some("binary file")
        } else if file.hunks.is_empty() {
            Some("no content changes")
        } else if let Some(&i) = built_in.matches(path).first() {
            Some(BUILT_IN_EXCLUDES[i].1)
        } else if excluded.is_match(path) {
            Some("matches files.exclude")
        } else {
            None
        };
        let result = match reason {
            Some(reason) => Err(reason),
            None => RepoPath::try_from(path.to_owned()).map_err(|_| "path can't be reviewed"),
        };
        match result {
            Ok(repo_path) => out.push((repo_path, file)),
            Err(reason) => skipped.push(Skipped {
                path: path.to_owned(),
                check_id: None,
                reason: reason.to_owned(),
            }),
        }
    }
    Ok(out)
}

/// splits files into chunks that fit the checks' `context.max_tokens`,
/// a file too big for one task is split by hunk, a hunk too big is skipped
fn pack(
    checks: &[&Check],
    files: &[&File<'_>],
    schema: &str,
    skipped: &mut Vec<Skipped>,
) -> Vec<Group> {
    // a group shows as much code as its smallest check would alone, so grouping
    // never gives a check more code to look at than it asked for.
    // function and file context come with tree-sitter later, every mode is hunk mode for now
    let budget = checks
        .iter()
        .map(|check| {
            let fixed = estimate_tokens(&render_prompt(&[check], "", schema));
            check.spec.context.max_tokens.get().saturating_sub(fixed)
        })
        .min()
        .unwrap_or(0);

    let mut groups = Vec::new();
    let mut current = Group::default();
    let mut used = 0;
    let mut add = |path: &RepoPath, text: String, groups: &mut Vec<Group>| {
        let tokens = estimate_tokens(&text);
        if used > 0 && used + tokens > budget {
            groups.push(std::mem::take(&mut current));
            used = 0;
        }
        if current.paths.last() != Some(path) {
            current.paths.push(path.clone());
        }
        current.code.push_str(&text);
        used += tokens;
    };

    for (path, file) in files {
        let whole = render_file(file, path.as_str(), &file.hunks);
        if estimate_tokens(&whole) <= budget {
            add(path, whole, &mut groups);
            continue;
        }
        for hunk in &file.hunks {
            let piece = render_file(file, path.as_str(), slice::from_ref(hunk));
            let tokens = estimate_tokens(&piece);
            if tokens <= budget {
                add(path, piece, &mut groups);
            } else {
                let ids: Vec<_> = checks.iter().map(|check| check.id().as_str()).collect();
                skipped.push(Skipped {
                    path: path.to_string(),
                    check_id: (checks.len() == 1).then(|| checks[0].id().clone()),
                    reason: format!(
                        "the change at line {} is about {tokens} tokens, \
                         too big for the context.max_tokens of {}",
                        hunk.new_start,
                        ids.join(", ")
                    ),
                });
            }
        }
    }
    if !current.paths.is_empty() {
        groups.push(current);
    }
    groups
}

fn glob_set<S: AsRef<str>>(globs: impl IntoIterator<Item = S>) -> Result<GlobSet, PlanError> {
    let mut set = GlobSetBuilder::new();
    for glob in globs {
        let glob = glob.as_ref();
        let fail = |err| PlanError::InvalidGlob {
            glob: glob.to_owned(),
            source: Box::new(err),
        };
        // `*` stops at `/`, like in .gitignore
        set.add(
            GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map_err(fail)?,
        );
    }
    set.build().map_err(|err| PlanError::InvalidGlob {
        glob: String::new(),
        source: Box::new(err),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gargoyle_git::parse_diff;
    use std::path::Path;

    fn check(id: &str, extra: &str) -> Check {
        let text = format!(
            "---\nschema: gargoyle/check@1\nid: {id}\nversion: 1\nname: {id}\n\
             description: x\n{extra}---\nLook for problems.\n"
        );
        Check::parse(Path::new(&format!("{id}.md")), &text).unwrap()
    }

    fn file_diff(path: &str, added: usize) -> String {
        let lines = "+some line of code\n".repeat(added);
        format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n\
             +++ b/{path}\n@@ -0,0 +1,{added} @@\n{lines}"
        )
    }

    fn plan(diff: &str, checks: &[Check], config: &Config) -> Result<Plan, PlanError> {
        let files = parse_diff(diff).unwrap();
        build_plan(&PlanInput {
            base: PlanBase {
                name: "main".into(),
                commit: "abc".into(),
            },
            files: &files,
            checks,
            config,
        })
    }

    fn skipped(plan: &Plan) -> Vec<(&str, &str)> {
        plan.plan
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.reason.as_str()))
            .collect()
    }

    #[test]
    fn skips_lockfiles_and_excluded_files() {
        let mut config = Config::default();
        config.files.exclude = vec!["dist/**".into()];
        let diff = [
            file_diff("Cargo.lock", 1),
            file_diff("dist/app.js", 1),
            file_diff("src/app.min.js", 1),
            file_diff("src/main.rs", 1),
        ]
        .concat();
        let plan = plan(&diff, &[check("quality", "")], &config).unwrap();
        assert_eq!(
            skipped(&plan),
            [
                ("Cargo.lock", "lockfile"),
                ("dist/app.js", "matches files.exclude"),
                ("src/app.min.js", "minified"),
            ]
        );
        assert_eq!(plan.plan.tasks.len(), 1);
        assert_eq!(plan.plan.tasks[0].paths[0].as_str(), "src/main.rs");
    }

    #[test]
    fn routes_files_by_the_checks_globs() {
        let diff = [file_diff("README.md", 1), file_diff("src/main.rs", 1)].concat();
        let checks = [
            check("docs", "applies_to:\n  globs: [\"**/*.md\"]\n"),
            check("code", "applies_to:\n  exclude: [\"**/*.md\"]\n"),
        ];
        let plan = plan(&diff, &checks, &per_check()).unwrap();
        let routed: Vec<_> = plan
            .plan
            .tasks
            .iter()
            .map(|t| (t.id.as_str(), t.paths[0].as_str()))
            .collect();
        assert_eq!(routed, [("code-1", "src/main.rs"), ("docs-1", "README.md")]);
    }

    fn per_check() -> Config {
        let mut config = Config::default();
        config.review.tasks = TaskMode::PerCheck;
        config
    }

    /// what each task reviews: its id, files and checks
    fn tasks(plan: &Plan) -> Vec<(&str, Vec<&str>, String)> {
        plan.plan
            .tasks
            .iter()
            .map(|t| {
                let paths = t.paths.iter().map(RepoPath::as_str).collect();
                (t.id.as_str(), paths, t.check_list())
            })
            .collect()
    }

    #[test]
    fn groups_files_with_every_check_that_applies_to_them() {
        let diff = [
            file_diff("README.md", 1),
            file_diff("src/a.rs", 1),
            file_diff("src/b.rs", 1),
        ]
        .concat();
        let checks = [
            check("code", "applies_to:\n  exclude: [\"**/*.md\"]\n"),
            check("security", ""),
        ];
        let plan = plan(&diff, &checks, &Config::default()).unwrap();
        assert_eq!(
            tasks(&plan),
            [
                (
                    "review-1",
                    vec!["src/a.rs", "src/b.rs"],
                    "code, security".to_owned()
                ),
                ("review-2", vec!["README.md"], "security".to_owned()),
            ]
        );
        assert!(plan.prompts[0].contains("these checks only: code, security"));
    }

    #[test]
    fn grouping_reads_the_diff_once_instead_of_once_per_check() {
        let diff = file_diff("src/a.rs", 20);
        let checks = [check("a", ""), check("b", ""), check("c", "")];
        let grouped = plan(&diff, &checks, &Config::default()).unwrap();
        let separate = plan(&diff, &checks, &per_check()).unwrap();
        assert_eq!(
            (grouped.plan.tasks.len(), separate.plan.tasks.len()),
            (1, 3)
        );
        assert_eq!(grouped.prompts[0].matches("+some line of code").count(), 20);
        let tokens =
            |plan: &Plan| -> u32 { plan.plan.tasks.iter().map(|t| t.estimated_tokens).sum() };
        assert!(
            tokens(&grouped) * 2 < tokens(&separate),
            "grouping should cost far less"
        );
    }

    #[test]
    fn files_no_check_wants_are_skipped_not_lost() {
        let diff = [file_diff("README.md", 1), file_diff("src/a.rs", 1)].concat();
        let checks = [check("code", "applies_to:\n  exclude: [\"**/*.md\"]\n")];
        let plan = plan(&diff, &checks, &Config::default()).unwrap();
        assert_eq!(skipped(&plan), [("README.md", "no check applies to it")]);
    }

    /// a check with room for `code_tokens` of changes on top of its own prompt
    fn check_with_room(code_tokens: u32) -> Check {
        let mut check = check("x", "");
        let schema = serde_json::to_string_pretty(&crate::schema::review_output()).unwrap();
        let fixed = estimate_tokens(&render_prompt(&[&check], "", &schema));
        check.spec.context.max_tokens = std::num::NonZeroU32::new(fixed + code_tokens).unwrap();
        check
    }

    #[test]
    fn splits_files_across_tasks_to_fit_the_budget() {
        // each file is about 300 tokens, so only one fits in each task
        let diff = [file_diff("a.rs", 60), file_diff("b.rs", 60)].concat();
        let plan = plan(&diff, &[check_with_room(500)], &Config::default()).unwrap();
        let ids: Vec<_> = plan.plan.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["review-1", "review-2"]);
        assert_eq!(plan.prompts.len(), 2);
        assert!(plan.prompts[0].contains("### a.rs (added)"));
    }

    #[test]
    fn a_hunk_too_big_for_the_check_is_skipped_not_cut() {
        let plan = plan(
            &file_diff("big.rs", 200),
            &[check_with_room(500)],
            &Config::default(),
        )
        .unwrap();
        assert_eq!(plan.plan.tasks.len(), 0);
        assert_eq!(
            plan.plan.skipped[0].check_id.as_ref().unwrap().as_str(),
            "x"
        );
    }

    #[test]
    fn refuses_to_go_over_the_review_budget() {
        let mut config = Config::default();
        config.review.max_tokens = std::num::NonZeroU32::new(100).unwrap();
        let err = plan(&file_diff("a.rs", 1), &[check("x", "")], &config).unwrap_err();
        assert!(
            matches!(err, PlanError::OverBudget { limit: 100, .. }),
            "{err}"
        );
    }

    #[test]
    fn same_input_gives_the_same_plan() {
        let diff = file_diff("a.rs", 3);
        let checks = [check("x", "")];
        let first = plan(&diff, &checks, &Config::default()).unwrap();
        let second = plan(&diff, &checks, &Config::default()).unwrap();
        assert_eq!(first.plan, second.plan);
        assert_eq!(first.prompts, second.prompts);
    }

    #[test]
    fn deleted_and_binary_files_are_skipped() {
        let diff = "diff --git a/old.rs b/old.rs\ndeleted file mode 100644\n--- a/old.rs\n\
                    +++ /dev/null\n@@ -1 +0,0 @@\n-gone\n\
                    diff --git a/logo.png b/logo.png\nnew file mode 100644\n\
                    Binary files /dev/null and b/logo.png differ\n";
        let plan = plan(diff, &[check("x", "")], &Config::default()).unwrap();
        assert_eq!(
            skipped(&plan),
            [("logo.png", "binary file"), ("old.rs", "deleted")]
        );
    }
}
