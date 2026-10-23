//! `gargoyle checks list` and `gargoyle checks validate`.

use std::path::Path;

use gargoyle_core::{Check, CheckError, CheckSpec, load_checks};
use serde::Serialize;

use crate::exit::Exit;
use crate::output::{out, print_json};

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
            out!("{} checks valid in {}", checks.len(), dir.display())?;
            Ok(Exit::Clean)
        }
        Err(errors) => Ok(report(&errors)),
    }
}

fn report(errors: &[CheckError]) -> Exit {
    for error in errors {
        eprintln!("error: {error}");
    }
    Exit::Config
}

fn print_table(checks: &[Check]) -> anyhow::Result<()> {
    let width = checks
        .iter()
        .map(|c| c.id().as_str().len())
        .max()
        .unwrap_or(0);
    for check in checks {
        let spec = &check.spec;
        out!(
            "{:<width$}  v{:<3} {:<7}  {}",
            check.id().as_str(),
            spec.version,
            spec.severity.as_str(),
            spec.description
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
