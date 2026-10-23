//! `gargoyle diff`, prints the changes a review would cover

use std::io::Write;
use std::path::Path;

use gargoyle_core::{CONFIG_FILE, Config};
use gargoyle_git::{BaseSource, CiEnv, FileDiff, GitError, Repo, parse_diff};

use crate::exit::Exit;

/// prints everything that differs from the base, `flag` is --base if given
pub(crate) fn run(flag: Option<&str>) -> anyhow::Result<Exit> {
    let repo = match Repo::discover(Path::new(".")) {
        Ok(repo) => repo,
        Err(err) => return git_error(err),
    };
    let config = match Config::load_or_default(&repo.root().join(CONFIG_FILE)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(Exit::Config);
        }
    };

    let base = match repo.find_base(flag, config.review.base.as_deref(), &CiEnv::from_env()) {
        Ok(base) => base,
        Err(err) => return git_error(err),
    };
    eprintln!(
        "comparing against {} ({})",
        base.name,
        describe(base.source)
    );

    let text = match repo.diff(&base.merge_base) {
        Ok(text) => text,
        Err(err) => return git_error(err),
    };
    eprintln!("{}", summary(&parse_diff(&text)?));
    // only the diff goes to stdout so it can be piped, everything else to stderr
    std::io::stdout().lock().write_all(text.as_bytes())?;
    Ok(Exit::Clean)
}

/// things you can fix in your setup exit with 2, a broken git exits with 3
fn git_error(err: GitError) -> anyhow::Result<Exit> {
    if err.is_config() {
        eprintln!("error: {err}");
        Ok(Exit::Config)
    } else {
        Err(err.into())
    }
}

/// e.g. 2 files changed, +10 -3, like git's own stat line but shorter
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

fn describe(source: BaseSource) -> &'static str {
    match source {
        BaseSource::Flag => "from --base",
        BaseSource::Config => "from review.base",
        BaseSource::CiEnv(var) => var,
        BaseSource::RemoteHead => "the remote's default branch",
        BaseSource::Fallback => "detected",
    }
}
