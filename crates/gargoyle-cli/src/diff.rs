//! `gargoyle diff`, prints the changes a review would cover

use std::io::Write;

use gargoyle_git::{CiEnv, FileDiff, parse_diff};

use crate::exit::Exit;
use crate::ui::{self, Ui};
use crate::workspace::{Workspace, describe, git_error};

/// prints everything that differs from the base, `flag` is --base if given
pub(crate) fn run(flag: Option<&str>) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let base = ws
        .repo
        .find_base(flag, ws.config.review.base.as_deref(), &CiEnv::from_env())
        .map_err(git_error)?;
    let against = format!("{} ({})", base.name, describe(base.source));
    let text = ws.repo.diff(&base.merge_base).map_err(git_error)?;
    let summary = summary(&parse_diff(&text)?);

    // only the diff goes to stdout so it can be piped, everything else to stderr
    if Ui::detect() == Ui::Fancy {
        eprintln!(
            "{} {}\n",
            console::style(format!("gargoyle diff · against {against}"))
                .bold()
                .for_stderr(),
            console::style(format!("· {summary}")).dim().for_stderr()
        );
    } else {
        eprintln!("comparing against {against}");
        eprintln!("{summary}");
    }
    let mut stdout = std::io::stdout().lock();
    if ui::stdout_is_terminal() && console::colors_enabled() {
        for line in text.lines() {
            writeln!(stdout, "{}", colored(line))?;
        }
    } else {
        stdout.write_all(text.as_bytes())?;
    }
    Ok(Exit::Clean)
}

/// a diff line in the colors git uses
fn colored(line: &str) -> console::StyledObject<&str> {
    let style = console::style(line);
    if line.starts_with("diff --git")
        || line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("rename ")
        || line.starts_with("index ")
    {
        style.bold()
    } else if line.starts_with("@@") {
        style.cyan()
    } else if line.starts_with('+') {
        style.green()
    } else if line.starts_with('-') {
        style.red()
    } else {
        style
    }
}

/// `2 files changed, +10 -3`, like git's own stat line but shorter
fn summary(files: &[FileDiff]) -> String {
    if files.is_empty() {
        return "no changes to review".to_owned();
    }
    let (added, removed) = files
        .iter()
        .map(FileDiff::line_counts)
        .fold((0, 0), |(added, removed), (a, r)| (added + a, removed + r));
    let noun = if files.len() == 1 { "file" } else { "files" };
    format!("{} {noun} changed, +{added} -{removed}", files.len())
}
