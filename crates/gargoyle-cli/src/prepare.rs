//! `gargoyle prepare`, builds the review plan and writes one prompt per task

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;

use anyhow::Context;
use gargoyle_core::{Check, Plan, PlanBase, PlanInput, RUNS_DIR, ReviewPlan, build_plan};
use gargoyle_git::{CiEnv, parse_diff};

use crate::exit::{Exit, config_error};
use crate::output::print_json;
use crate::workspace::{Workspace, describe, git_error};

pub(crate) fn run(flag: Option<&str>, only: &[String], json: bool) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let checks = pick_checks(ws.checks()?, only)?;
    let base = ws
        .repo
        .find_base(flag, ws.config.review.base.as_deref(), &CiEnv::from_env())
        .map_err(git_error)?;
    eprintln!(
        "comparing against {} ({})",
        base.name,
        describe(base.source)
    );

    let files = parse_diff(&ws.repo.diff(&base.merge_base).map_err(git_error)?)?;
    let input = PlanInput {
        base: PlanBase {
            name: base.name,
            commit: base.merge_base,
        },
        files: &files,
        checks: &checks,
        config: &ws.config,
        sources: None,
    };
    let Plan { plan, prompts } = build_plan(&input).map_err(config_error)?;
    write_run(&ws, &plan, &prompts)?;

    if json {
        print_json(&plan)?;
    } else {
        std::io::stdout()
            .lock()
            .write_all(summary(&plan, files.is_empty()).as_bytes())?;
    }
    Ok(Exit::Clean)
}

/// only the checks named in --checks, or all of them
fn pick_checks(checks: Vec<Check>, only: &[String]) -> anyhow::Result<Vec<Check>> {
    if only.is_empty() {
        return Ok(checks);
    }
    if let Some(unknown) = only
        .iter()
        .find(|id| !checks.iter().any(|c| c.id().as_str() == id.as_str()))
    {
        let known: Vec<_> = checks.iter().map(|c| c.id().as_str()).collect();
        return Err(config_error(format!(
            "unknown check `{unknown}`, the checks are: {}",
            known.join(", ")
        )));
    }
    Ok(checks
        .into_iter()
        .filter(|c| only.iter().any(|id| id == c.id().as_str()))
        .collect())
}

fn write_run(ws: &Workspace, plan: &ReviewPlan, prompts: &[String]) -> anyhow::Result<()> {
    let runs = ws.path(RUNS_DIR);
    fs::create_dir_all(&runs).with_context(|| format!("creating {}", runs.display()))?;
    // runs are scratch space, git should never see them, so they never get reviewed either
    let ignore = runs.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "*\n").with_context(|| format!("writing {}", ignore.display()))?;
    }

    // the same changes give the same run id, so a repeated prepare starts that run over
    let dir = ws.path(&plan.run_id.dir());
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("clearing {}", dir.display()))?;
    }
    for sub in ["prompts", "answers", "results"] {
        fs::create_dir_all(dir.join(sub)).with_context(|| format!("creating {}", dir.display()))?;
    }
    for (task, prompt) in plan.tasks.iter().zip(prompts) {
        let path = ws.path(&task.prompt_path);
        fs::write(&path, prompt).with_context(|| format!("writing {}", path.display()))?;
    }
    let path = ws.path(&plan.run_id.plan_path());
    let json = serde_json::to_string_pretty(plan)? + "\n";
    fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
}

fn summary(plan: &ReviewPlan, no_changes: bool) -> String {
    let mut out = String::new();
    if plan.tasks.is_empty() {
        out.push_str(if no_changes {
            "no changes to review\n"
        } else {
            "nothing to review, every changed file was skipped\n"
        });
    } else {
        let tokens: u64 = plan
            .tasks
            .iter()
            .map(|t| u64::from(t.estimated_tokens))
            .sum();
        let _ = writeln!(
            out,
            "run {}: {} tasks, about {}k tokens",
            plan.run_id,
            plan.tasks.len(),
            tokens.div_ceil(1000)
        );
        let width = plan
            .tasks
            .iter()
            .map(|t| t.id.as_str().len())
            .max()
            .unwrap_or(0);
        for task in &plan.tasks {
            let paths: Vec<_> = task
                .paths
                .iter()
                .map(gargoyle_core::RepoPath::as_str)
                .collect();
            let _ = writeln!(
                out,
                "  {:<width$}  {} · {}",
                task.id.as_str(),
                paths.join(", "),
                task.check_list()
            );
        }
    }
    for skipped in &plan.skipped {
        let check = skipped
            .check_id
            .as_ref()
            .map(|id| format!(" for {id}"))
            .unwrap_or_default();
        let _ = writeln!(out, "skipped {}{check}: {}", skipped.path, skipped.reason);
    }
    out
}
