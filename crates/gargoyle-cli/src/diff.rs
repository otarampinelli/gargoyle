//! `gargoyle diff`, prints the changes a review would cover

use std::io::Write;

use gargoyle_git::{CiEnv, FileDiff, parse_diff};

use crate::exit::Exit;
use crate::workspace::{Workspace, describe, git_error};

/// prints everything that differs from the base, `flag` is --base if given
pub(crate) fn run(flag: Option<&str>) -> anyhow::Result<Exit> {
    let ws = Workspace::open()?;
    let base = ws
        .repo
        .find_base(flag, ws.config.review.base.as_deref(), &CiEnv::from_env())
        .map_err(git_error)?;
    eprintln!(
        "comparing against {} ({})",
        base.name,
        describe(base.source)
    );

    let text = ws.repo.diff(&base.merge_base).map_err(git_error)?;
    eprintln!("{}", summary(&parse_diff(&text)?));
    // only the diff goes to stdout so it can be piped, everything else to stderr
    std::io::stdout().lock().write_all(text.as_bytes())?;
    Ok(Exit::Clean)
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
