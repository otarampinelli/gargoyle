//! `gargoyle checks list` and `gargoyle checks validate`.

use std::path::Path;

use gargoyle_core::{Check, CheckError, CheckSpec, load_checks};
use serde::Serialize;

use crate::exit::Exit;
use crate::output::{out, print_json};
use crate::ui;

pub(crate) fn list(dir: &Path, json: bool) -> anyhow::Result<Exit> {
    let checks = match load_checks(dir) {
        Ok(checks) => checks,
        Err(errors) => return Ok(report(&errors)),
    };

    if json {
        let listed: Vec<_> = checks.iter().map(Listed::from).collect();
        print_json(&listed)?;
    } else if checks.is_empty() {
        eprintln!("No checks found in {}", dir.display());
    } else {
        print_table(&checks)?;
    }
    Ok(Exit::Clean)
}

pub(crate) fn validate(dir: &Path) -> anyhow::Result<Exit> {
    match load_checks(dir) {
        Ok(checks) => {
            let message = format!("{} checks valid in {}", checks.len(), dir.display());
            if ui::stdout_is_terminal() {
                out!("{} {message}", console::style("✔").green())?;
            } else {
                out!("{message}")?;
            }
            Ok(Exit::Clean)
        }
        Err(errors) => Ok(report(&errors)),
    }
}

fn report(errors: &[CheckError]) -> Exit {
    let label = console::style("error:").red().bold().for_stderr();
    for error in errors {
        eprintln!("{label} {error}");
    }
    Exit::Config
}

/// one check per line, at a terminal with a header, colors and descriptions that wrap
fn print_table(checks: &[Check]) -> anyhow::Result<()> {
    let width = checks
        .iter()
        .map(|c| c.id().as_str().len())
        .max()
        .unwrap_or(0)
        .max(5);
    let terminal = ui::stdout_width();
    if terminal.is_some() {
        let header = format!(
            "{:<width$}  {:<4} {:<7}  {}",
            "CHECK", "VER", "LEVEL", "WHAT IT CATCHES"
        );
        out!("{}", console::style(header).dim().bold())?;
    }
    // id, version and severity take this many columns before the description
    let indent = " ".repeat(width + 16);
    for check in checks {
        let spec = &check.spec;
        let description = match terminal {
            Some(columns) => ui::wrap(&spec.description, columns, &indent),
            None => spec.description.clone(),
        };
        out!(
            "{}  v{:<3} {}  {}",
            console::style(format!("{:<width$}", check.id().as_str())).bold(),
            spec.version,
            ui::severity_style(spec.severity).apply_to(format!("{:<7}", spec.severity.as_str())),
            description
        )?;
    }
    Ok(())
}

/// One entry of `checks list --json`: the frontmatter plus where it came from.
#[derive(Debug, Serialize)]
struct Listed<'a> {
    #[serde(flatten)]
    spec: &'a CheckSpec,
    path: &'a Path,
}

impl<'a> From<&'a Check> for Listed<'a> {
    fn from(check: &'a Check) -> Self {
        Self {
            spec: &check.spec,
            path: &check.path,
        }
    }
}
