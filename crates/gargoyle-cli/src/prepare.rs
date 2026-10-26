//! `gargoyle prepare`, builds the review plan and writes one prompt per task

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::time::Instant;

use anyhow::Context;
use gargoyle_core::{
    Check, Guideline, Plan, PlanBase, PlanInput, RUNS_DIR, ReviewPlan, Sources, build_plan,
};
use gargoyle_git::{CiEnv, FileDiff, Repo, parse_diff};
use gargoyle_index::{INDEX_DIR, Index, Symbols, grammar_of, parse};

use crate::exit::{Exit, config_error};
use crate::output::print_json;
use crate::ui::{self, Ui};
use crate::workspace::{Workspace, describe, git_error};

pub(crate) fn run(flag: Option<&str>, only: &[String], json: bool) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let checks = pick_checks(ws.checks()?, only)?;
    let ui = Ui::detect();
    ui.intro("gargoyle prepare")?;
    let base = ws
        .repo
        .find_base(flag, ws.config.review.base.as_deref(), &CiEnv::from_env())
        .map_err(git_error)?;
    let against = format!("{} ({})", base.name, describe(base.source));
    ui.step(
        format!("Comparing against {against}"),
        format!("comparing against {against}"),
    )?;

    // whole files, the planner keeps the definitions the changes are in
    let files = parse_diff(&ws.repo.full_diff(&base.merge_base).map_err(git_error)?)?;

    let index = build_index(&ws, ui);
    let before = match &index {
        Some(_) => old_symbols(&ws.repo, &base.merge_base, &files),
        None => BTreeMap::new(),
    };
    let root = ws.repo.root().to_owned();
    let read = move |path: &str| fs::read_to_string(root.join(path)).ok();
    let sources = index.as_ref().map(|index| Sources {
        index,
        before: &before,
        read: &read,
    });

    let guidelines = read_guidelines(&ws, ui)?;
    let input = PlanInput {
        base: PlanBase {
            name: base.name,
            commit: base.merge_base,
        },
        files: &files,
        checks: &checks,
        config: &ws.config,
        sources,
        guidelines: &guidelines,
    };
    let Plan { plan, prompts } = build_plan(&input).map_err(config_error)?;
    write_run(&ws, &plan, &prompts)?;

    if json {
        print_json(&plan)?;
        // stdout is the plan, so the closing line only shows at a terminal, on stderr
        if ui == Ui::Fancy {
            ui.outro(format!("Run {} is ready", plan.run_id))?;
        }
    } else if ui == Ui::Fancy {
        fancy_summary(ui, &plan, files.is_empty())?;
    } else {
        std::io::stdout()
            .lock()
            .write_all(summary(&plan, files.is_empty()).as_bytes())?;
    }
    Ok(Exit::Clean)
}

/// brings the code index up to date, none when it can't be built,
/// then the review still runs, just without related code
fn build_index(ws: &Workspace, ui: Ui) -> Option<Index> {
    let first = !ws.path(INDEX_DIR).exists();
    let spinner = if first {
        ui.spinner(
            "Indexing the repo for related code, the first run reads every file",
            Some("indexing the repo for related code, the first run reads every file"),
        )
    } else {
        ui.spinner("Updating the code index", None)
    };
    let started = Instant::now();
    match Index::build(&ws.repo) {
        Ok((index, update)) => {
            if update.parsed > 0 {
                let later = if update.first {
                    ", later runs only read what changed"
                } else {
                    ""
                };
                let message = format!(
                    "indexed {} of {} files in {:.1}s{later}",
                    update.parsed,
                    update.files,
                    started.elapsed().as_secs_f64()
                );
                spinner.stop(capitalize(&message), Some(&message));
            } else {
                spinner.stop("The code index is up to date", None);
            }
            Some(index)
        }
        Err(err) => {
            spinner.fail(format!(
                "reviewing without related code, the index failed: {err}"
            ));
            None
        }
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// the docs `context.guidelines` lists, as they are on disk, one that can't be read
/// is left out with a warning, so a typo in the config doesn't go unnoticed
fn read_guidelines(ws: &Workspace, ui: Ui) -> anyhow::Result<Vec<Guideline>> {
    let mut guidelines = Vec::new();
    for path in &ws.config.context.guidelines {
        match fs::read_to_string(ws.path(path.as_str())) {
            Ok(text) if text.trim().is_empty() => {}
            Ok(text) => guidelines.push(Guideline {
                path: path.clone(),
                text,
            }),
            Err(err) => ui.warn(format!("reviewing without the guideline {path}: {err}"))?,
        }
    }
    Ok(guidelines)
}

/// what each changed file defined at the base, so removed and reshaped definitions
/// are known, a file git can't show is left out
fn old_symbols(repo: &Repo, base: &str, files: &[FileDiff]) -> BTreeMap<String, Symbols> {
    files
        .iter()
        .filter_map(|file| {
            let path = file.old_path.as_deref()?;
            let grammar = grammar_of(path)?;
            let text = repo.show(base, path).ok()??;
            Some((path.to_owned(), parse(grammar, &text)))
        })
        .collect()
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

/// the plan in a frame at a terminal, then what to do next
fn fancy_summary(ui: Ui, plan: &ReviewPlan, no_changes: bool) -> anyhow::Result<()> {
    for skipped in &plan.skipped {
        let check = skipped
            .check_id
            .as_ref()
            .map(|id| format!(" for {id}"))
            .unwrap_or_default();
        ui.note(format!(
            "Not reviewing {}{check}: {}",
            skipped.path, skipped.reason
        ))?;
    }
    if plan.tasks.is_empty() {
        return ui.outro(if no_changes {
            "No changes to review"
        } else {
            "Nothing to review, every changed file was skipped"
        });
    }

    let tokens: u64 = plan
        .tasks
        .iter()
        .map(|t| u64::from(t.estimated_tokens))
        .sum();
    let count = plan.tasks.len();
    let noun = if count == 1 { "task" } else { "tasks" };
    let width = plan
        .tasks
        .iter()
        .map(|t| t.id.as_str().len())
        .max()
        .unwrap_or(0);
    // the frame takes about 6 columns, the id column the rest of the indent
    let room = console::Term::stderr()
        .size_checked()
        .map_or(100, |(_, columns)| usize::from(columns))
        .saturating_sub(6);
    let indent = " ".repeat(width + 2);
    let mut body = String::new();
    for task in &plan.tasks {
        let paths: Vec<_> = task
            .paths
            .iter()
            .map(gargoyle_core::RepoPath::as_str)
            .collect();
        let _ = writeln!(
            body,
            "{}  {}\n{indent}{}",
            console::style(format!("{:<width$}", task.id.as_str())).bold(),
            ui::wrap(&paths.join(", "), room, &indent),
            console::style(ui::wrap(&task.check_list(), room, &indent)).dim()
        );
    }
    ui.block(
        format!(
            "Run {} · {count} {noun} · about {}k tokens",
            plan.run_id,
            tokens.div_ceil(1000)
        ),
        body.trim_end(),
    )?;
    ui.outro("Ready. Your agent runs these tasks with `gargoyle guide`, or run /gargoyle")
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
