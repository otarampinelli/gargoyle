//! the repo and config every review command starts from

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::Context;
use gargoyle_core::{CONFIG_FILE, Check, Config, ReviewPlan, RunId, load_checks};
use gargoyle_git::{BaseSource, GitError, Repo};

use crate::exit::config_error;

#[derive(Debug)]
pub(crate) struct Workspace {
    pub(crate) repo: Repo,
    pub(crate) config: Config,
}

impl Workspace {
    /// the repo around the current directory and its gargoyle.toml
    pub(crate) fn open() -> anyhow::Result<Self> {
        let repo = Repo::discover(Path::new(".")).map_err(git_error)?;
        let config =
            Config::load_or_default(&repo.root().join(CONFIG_FILE)).map_err(config_error)?;
        Ok(Self { repo, config })
    }

    /// a path relative to the repo root
    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.repo.root().join(relative)
    }

    pub(crate) fn checks(&self) -> anyhow::Result<Vec<Check>> {
        let dir = self.repo.root().join(&self.config.checks.dir);
        load_checks(&dir).map_err(|errors| {
            for error in &errors {
                eprintln!("error: {error}");
            }
            config_error("fix the check files above, `gargoyle checks validate` lists them")
        })
    }

    pub(crate) fn load_plan(&self, run: &str) -> anyhow::Result<ReviewPlan> {
        let Some(run_id) = RunId::parse(run) else {
            return Err(config_error(format!(
                "{run:?} is not a run id, `gargoyle prepare` prints one"
            )));
        };
        let path = self.path(&run_id.plan_path());
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                return Err(config_error(format!(
                    "there's no run {run_id}, start one with `gargoyle prepare`"
                )));
            }
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };
        serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }
}

/// things you can fix in your setup exit with 2, a broken git exits with 3
pub(crate) fn git_error(err: GitError) -> anyhow::Error {
    if err.is_config() {
        config_error(err)
    } else {
        err.into()
    }
}

/// why this base, for the `comparing against` line
pub(crate) fn describe(source: BaseSource) -> &'static str {
    match source {
        BaseSource::Flag => "from --base",
        BaseSource::Config => "from review.base",
        BaseSource::CiEnv(var) => var,
        BaseSource::RemoteHead => "the remote's default branch",
        BaseSource::Fallback => "detected",
    }
}
