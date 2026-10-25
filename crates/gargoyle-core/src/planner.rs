//! builds the review plan: drops files that shouldn't be reviewed,
//! routes the rest to checks and packs them into tasks of about the same size

use std::collections::BTreeMap;
use std::slice;

use gargoyle_git::{FileDiff, FileStatus};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::plan::{PlanSchema, RunId, TaskCheck, TaskId};
use crate::prompt::{render_file, render_prompt};
use crate::related::{Shown, related};
use crate::{
    Check, Config, PathInstruction, PlanBase, RepoPath, ReviewPlan, Skipped, Sources, Task,
    TaskMode,
};

/// code one task shows, a fixed size instead of a setting,
/// it only splits the work, a change bigger than this still gets a task of its own,
/// related code gets up to the same size again
pub(crate) const CODE_TOKENS: u32 = 16_000;

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
    /// the code index and what it needs, none for a plain diff review
    pub sources: Option<Sources<'a>>,
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
    #[error("invalid glob {glob:?}: {source}")]
    InvalidGlob {
        glob: String,
        source: Box<globset::Error>,
    },
}

/// files of one task, the hunks it shows, and their rendered changes
#[derive(Debug, Default)]
struct Group<'a> {
    paths: Vec<RepoPath>,
    shown: Vec<Shown<'a>>,
    code: String,
}

/// a reviewable file and its diff
type File<'a> = (RepoPath, &'a FileDiff);

/// checks and the files they review together
type CheckGroup<'c, 'f> = (Vec<&'c Check>, Vec<&'f File<'f>>);

/// a path instruction and the glob that picks its files
type Instruction<'a> = (GlobSet, &'a PathInstruction);

pub fn build_plan(input: &PlanInput<'_>) -> Result<Plan, PlanError> {
    plan_with(input, CODE_TOKENS)
}

/// the plan with tasks of about `code_tokens` of code each
fn plan_with(input: &PlanInput<'_>, code_tokens: u32) -> Result<Plan, PlanError> {
    let mut skipped = Vec::new();
    let files = reviewable_files(input.files, &input.config.files.exclude, &mut skipped)?;
    let schema = serde_json::to_string_pretty(&crate::schema::review_output()).unwrap_or_default();
    let instructions = instructions_matchers(&input.config.instructions)?;

    let mut checks: Vec<&Check> = input.checks.iter().collect();
    checks.sort_by(|a, b| a.id().cmp(b.id()));
    let routed = route(&checks, &files, &mut skipped)?;

    let groups = match input.config.review.tasks {
        TaskMode::PerCheck => routed,
        TaskMode::Grouped => group_by_files(&files, &routed),
    };

    let mut drafts = Vec::new();
    for (checks, files) in groups {
        for chunk in pack(&files, code_tokens) {
            let matched = instructions_for(&instructions, &chunk.paths);
            // up to a task's size of related code, so a small change still gets its callers
            let related = input.sources.as_ref().map_or_else(String::new, |sources| {
                related(sources, &chunk.shown, &input.config.context, code_tokens)
            });
            let prompt = render_prompt(&checks, &matched, &chunk.code, &related, &schema);
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

/// splits files into chunks of about `budget` tokens of code, a file too big for one
/// task is split by hunk, and a hunk bigger than a task gets a task of its own,
/// so nothing is ever left out for its size
fn pack<'f>(files: &[&'f File<'f>], budget: u32) -> Vec<Group<'f>> {
    let mut groups = Vec::new();
    let mut current = Group::default();
    let mut used = 0;
    let mut add = |shown: Shown<'f>, path: &RepoPath, text: String, groups: &mut Vec<Group<'f>>| {
        let tokens = estimate_tokens(&text);
        if used > 0 && used + tokens > budget {
            groups.push(std::mem::take(&mut current));
            used = 0;
        }
        if current.paths.last() != Some(path) {
            current.paths.push(path.clone());
        }
        current.shown.push(shown);
        current.code.push_str(&text);
        used += tokens;
    };

    for &(path, file) in files {
        let shown = |hunks| Shown {
            path: path.as_str(),
            file,
            hunks,
        };
        let whole = render_file(file, path.as_str(), &file.hunks, false);
        if estimate_tokens(&whole) <= budget || file.hunks.len() == 1 {
            add(shown(&file.hunks), path, whole, &mut groups);
            continue;
        }
        for hunk in &file.hunks {
            let piece = render_file(file, path.as_str(), slice::from_ref(hunk), true);
            add(shown(slice::from_ref(hunk)), path, piece, &mut groups);
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

/// compiles each instruction glob once, the same way checks match files
fn instructions_matchers(
    instructions: &[PathInstruction],
) -> Result<Vec<Instruction<'_>>, PlanError> {
    instructions
        .iter()
        .map(|instruction| Ok((glob_set([&instruction.path])?, instruction)))
        .collect()
}

/// the instructions for any of 'paths', in the order of the config
fn instructions_for<'a>(
    instructions: &[Instruction<'a>],
    paths: &[RepoPath],
) -> Vec<&'a PathInstruction> {
    instructions
        .iter()
        .filter(|(glob, _)| paths.iter().any(|path| glob.is_match(path.as_str())))
        .map(|&(_, instruction)| instruction)
        .collect()
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
        plan_in_tasks_of(diff, checks, config, CODE_TOKENS)
    }

    /// a plan with tasks of about `code_tokens` of code, small enough to split in tests
    fn plan_in_tasks_of(
        diff: &str,
        checks: &[Check],
        config: &Config,
        code_tokens: u32,
    ) -> Result<Plan, PlanError> {
        let files = parse_diff(diff).unwrap();
        let input = PlanInput {
            base: PlanBase {
                name: "main".into(),
                commit: "abc".into(),
            },
            files: &files,
            checks,
            config,
            sources: None,
        };
        plan_with(&input, code_tokens)
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

    #[test]
    fn splits_files_across_tasks_to_fit_the_budget() {
        // each file is about 300 tokens, so only one fits in each task
        let diff = [file_diff("a.rs", 60), file_diff("b.rs", 60)].concat();
        let plan = plan_in_tasks_of(&diff, &[check("x", "")], &Config::default(), 500).unwrap();
        let ids: Vec<_> = plan.plan.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["review-1", "review-2"]);
        assert_eq!(plan.prompts.len(), 2);
        assert!(plan.prompts[0].contains("### a.rs (added)"));
    }

    #[test]
    fn a_file_split_across_tasks_is_marked_as_partly_shown() {
        let hunk = |start: usize| -> String {
            format!(
                "@@ -{start},0 +{start},60 @@\n{}",
                "+some line of code\n".repeat(60)
            )
        };
        let diff = format!(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n{}{}",
            hunk(1),
            hunk(100)
        );
        let plan = plan_in_tasks_of(&diff, &[check("x", "")], &Config::default(), 500).unwrap();
        assert_eq!(plan.plan.tasks.len(), 2);
        assert!(
            plan.prompts
                .iter()
                .all(|p| p.contains("### a.rs (modified, partly shown"))
        );
    }

    #[test]
    fn a_change_bigger_than_a_task_gets_its_own_task() {
        let diff = [file_diff("a.rs", 1), file_diff("big.rs", 200)].concat();
        let plan = plan_in_tasks_of(&diff, &[check("x", "")], &Config::default(), 500).unwrap();
        assert_eq!(skipped(&plan), []);
        assert_eq!(
            tasks(&plan),
            [
                ("review-1", vec!["a.rs"], "x".to_owned()),
                ("review-2", vec!["big.rs"], "x".to_owned()),
            ]
        );
        // its only change, so the whole file is shown
        assert!(
            plan.prompts[1].contains("### big.rs (added)\n"),
            "{}",
            plan.prompts[1]
        );
        assert_eq!(plan.prompts[1].matches("+some line of code").count(), 200);
    }

    #[test]
    fn tasks_get_the_related_code_of_their_changes() {
        use gargoyle_git::TreeFile;
        use gargoyle_index::{Index, grammar_of, parse};

        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
        write("cart.rs", "fn total() -> u64 {\n    2\n}\n");
        write("shop.rs", "fn pay() {\n    total();\n}\n");
        let tree = ["cart.rs", "shop.rs"].map(|path| TreeFile {
            path: path.to_owned(),
            blob: None,
        });
        let (index, _) = Index::update(dir.path(), &tree).unwrap();
        let old = parse(
            grammar_of("cart.rs").unwrap(),
            "fn total() -> u64 {\n    1\n}\n",
        );
        let before = BTreeMap::from([("cart.rs".to_owned(), old)]);
        let root = dir.path().to_owned();
        let read = move |path: &str| std::fs::read_to_string(root.join(path)).ok();

        let files = parse_diff(
            "diff --git a/cart.rs b/cart.rs\n--- a/cart.rs\n+++ b/cart.rs\n\
             @@ -1,3 +1,3 @@\n fn total() -> u64 {\n-    1\n+    2\n }\n",
        )
        .unwrap();
        let checks = [check("x", "")];
        let config = Config::default();
        let input = PlanInput {
            base: PlanBase {
                name: "main".into(),
                commit: "abc".into(),
            },
            files: &files,
            checks: &checks,
            config: &config,
            sources: Some(Sources {
                index: &index,
                before: &before,
                read: &read,
            }),
        };
        let plan = build_plan(&input).unwrap();
        let prompt = &plan.prompts[0];
        assert!(
            prompt.contains("#### Used in shop.rs:2, in `fn pay()`"),
            "{prompt}"
        );
        assert!(prompt.find("## Changes") < prompt.find("## Related Code"));
        assert!(prompt.find("## Related Code") < prompt.find("## How to Report"));
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

    /// the prompt of the task that reviews `path`
    fn prompt_for<'p>(plan: &'p Plan, path: &str) -> &'p str {
        plan.plan
            .tasks
            .iter()
            .zip(&plan.prompts)
            .find(|(task, _)| task.paths.iter().any(|p| p.as_str() == path))
            .map(|(_, prompt)| prompt.as_str())
            .unwrap()
    }

    fn with_instruction(path: &str, text: &str) -> Config {
        Config {
            instructions: vec![PathInstruction {
                path: path.into(),
                text: text.into(),
            }],
            ..Config::default()
        }
    }

    #[test]
    fn path_instructions_only_go_to_tasks_with_matching_files() {
        let diff = [file_diff("README.md", 1), file_diff("src/api/users.rs", 1)].concat();
        let checks = [
            check("code", "applies_to:\n  exclude: [\"**/*.md\"]\n"),
            check("docs", "applies_to:\n  globs: [\"**/*.md\"]\n"),
        ];
        let config = with_instruction("src/api/**", "Validate every input.");
        let plan = plan(&diff, &checks, &config).unwrap();

        let api = prompt_for(&plan, "src/api/users.rs");
        assert!(api.contains("## Project Instructions"), "{api}");
        assert!(api.contains("### `src/api/**`\n\nValidate every input."));
        assert!(api.contains("checks or project instructions above"));

        let docs = prompt_for(&plan, "README.md");
        assert!(!docs.contains("Project Instructions"), "{docs}");
    }

    #[test]
    fn instruction_globs_dont_cross_folders() {
        let config = with_instruction("src/*.rs", "Keep it short.");
        let plan = plan(
            &file_diff("src/api/users.rs", 1),
            &[check("x", "")],
            &config,
        )
        .unwrap();
        assert!(!plan.prompts[0].contains("Keep it short."));
    }
}
