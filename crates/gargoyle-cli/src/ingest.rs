//! `gargoyle ingest`, validates a reviewer's answer for one task and keeps it

use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::Context;

use crate::exit::{Exit, config_error};
use crate::output::out;
use crate::ui;
use crate::workspace::Workspace;

/// `file` is the answer, stdin when it's missing or `-`
pub(crate) fn run(run: &str, task: &str, file: Option<&Path>) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let plan = ws.load_plan(run)?;
    let Some(task) = plan.task(task) else {
        let ids: Vec<_> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
        return Err(config_error(format!(
            "run {} has no task {task:?}, its tasks are: {}",
            plan.run_id,
            ids.join(", ")
        )));
    };

    let answer = match file {
        Some(path) if path != Path::new("-") => {
            // answer paths in the plan are relative to the repo root, not to where you are
            let path = if path.is_relative() && !path.exists() {
                ws.repo.root().join(path)
            } else {
                path.to_owned()
            };
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
        }
        _ => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("reading the answer from stdin")?;
            text
        }
    };

    match gargoyle_core::ingest(task, &answer) {
        Ok(output) => {
            let path = ws.path(&plan.run_id.result_path(&task.id));
            let json = serde_json::to_string_pretty(&output)? + "\n";
            fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
            let noun = if output.findings.len() == 1 {
                "finding"
            } else {
                "findings"
            };
            // agents read this line, it always starts with `ok`
            let mark = if ui::stdout_is_terminal() {
                format!("{} ", console::style("✔").green())
            } else {
                String::new()
            };
            out!("{mark}ok: {} {noun} for {}", output.findings.len(), task.id)?;
            Ok(Exit::Clean)
        }
        Err(problems) => {
            let label = console::style("error:").red().bold().for_stderr();
            for problem in problems {
                eprintln!("{label} {problem}");
            }
            eprintln!("fix the answer and run `gargoyle ingest` again");
            Ok(Exit::InvalidOutput)
        }
    }
}
